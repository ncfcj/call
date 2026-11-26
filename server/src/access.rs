//! Quem pode ver e mexer em um servidor ou canal.
//!
//! Toda rota de servidor, canal ou mensagem passa por aqui; o WebSocket (`SUBSCRIBE`) e o token
//! de voz da Fase 2 também vão passar. Quem não é membro recebe `NotFound`, e não `Forbidden`,
//! para não revelar que o servidor ou canal existe.

use proto::{Channel, ChannelId, ChannelKind, GuildId, UserId};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

/// Prova de que o usuário é membro do servidor.
#[derive(Debug, Clone, Copy)]
pub struct GuildAccess {
    pub guild_id: GuildId,
    pub owner_id: UserId,
    pub user_id: UserId,
}

impl GuildAccess {
    pub fn is_owner(&self) -> bool {
        self.owner_id == self.user_id
    }

    /// Só o dono passa; os outros membros recebem `Forbidden`.
    pub fn require_owner(&self) -> AppResult<()> {
        if self.is_owner() {
            Ok(())
        } else {
            Err(AppError::Forbidden("só o dono do servidor pode fazer isso"))
        }
    }
}

/// Prova de que o usuário é membro do servidor dono do canal.
#[derive(Debug, Clone)]
pub struct ChannelAccess {
    pub channel: Channel,
    pub guild: GuildAccess,
}

/// Exige que `user` seja membro de `guild`.
pub async fn guild_member(db: &PgPool, guild: GuildId, user: UserId) -> AppResult<GuildAccess> {
    let owner: Option<Uuid> = sqlx::query_scalar(
        "SELECT g.owner_id FROM guilds g
         JOIN guild_members m ON m.guild_id = g.id AND m.user_id = $2
         WHERE g.id = $1",
    )
    .bind(guild.0)
    .bind(user.0)
    .fetch_optional(db)
    .await?;
    let owner = owner.ok_or(AppError::NotFound)?;
    tracing::Span::current().record("guild", tracing::field::display(guild));
    Ok(GuildAccess { guild_id: guild, owner_id: UserId(owner), user_id: user })
}

#[derive(sqlx::FromRow)]
pub struct ChannelRow {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub name: String,
    pub kind: String,
    pub position: i32,
}

impl TryFrom<ChannelRow> for Channel {
    type Error = AppError;

    fn try_from(row: ChannelRow) -> AppResult<Self> {
        let kind = ChannelKind::parse(&row.kind)
            .ok_or_else(|| anyhow::anyhow!("channels.kind desconhecido: {}", row.kind))?;
        Ok(Self {
            id: ChannelId(row.id),
            guild_id: GuildId(row.guild_id),
            name: row.name,
            kind,
            position: row.position,
        })
    }
}

#[derive(sqlx::FromRow)]
struct ChannelAccessRow {
    #[sqlx(flatten)]
    channel: ChannelRow,
    owner_id: Uuid,
}

/// Exige que `user` seja membro do servidor do canal.
pub async fn channel_member(
    db: &PgPool,
    channel: ChannelId,
    user: UserId,
) -> AppResult<ChannelAccess> {
    let row: Option<ChannelAccessRow> = sqlx::query_as(
        "SELECT c.id, c.guild_id, c.name, c.kind, c.position, g.owner_id
         FROM channels c
         JOIN guilds g ON g.id = c.guild_id
         JOIN guild_members m ON m.guild_id = c.guild_id AND m.user_id = $2
         WHERE c.id = $1",
    )
    .bind(channel.0)
    .bind(user.0)
    .fetch_optional(db)
    .await?;
    let row = row.ok_or(AppError::NotFound)?;
    tracing::Span::current()
        .record("guild", tracing::field::display(row.channel.guild_id))
        .record("channel", tracing::field::display(channel));
    let guild = GuildAccess {
        guild_id: GuildId(row.channel.guild_id),
        owner_id: UserId(row.owner_id),
        user_id: user,
    };
    Ok(ChannelAccess { channel: row.channel.try_into()?, guild })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access(owner: u128, user: u128) -> GuildAccess {
        GuildAccess {
            guild_id: GuildId(Uuid::from_u128(1)),
            owner_id: UserId(Uuid::from_u128(owner)),
            user_id: UserId(Uuid::from_u128(user)),
        }
    }

    #[test]
    fn only_the_owner_passes_require_owner() {
        assert!(access(7, 7).is_owner());
        assert!(access(7, 7).require_owner().is_ok());
        assert!(!access(7, 8).is_owner());
        let err = access(7, 8).require_owner().unwrap_err();
        assert!(matches!(err, AppError::Forbidden(_)));
    }

    fn row(kind: &str) -> ChannelRow {
        ChannelRow {
            id: Uuid::from_u128(10),
            guild_id: Uuid::from_u128(1),
            name: "geral".into(),
            kind: kind.into(),
            position: 3,
        }
    }

    #[test]
    fn channel_row_becomes_a_channel() {
        let channel = Channel::try_from(row("voice")).unwrap();
        assert_eq!(channel.id, ChannelId(Uuid::from_u128(10)));
        assert_eq!(channel.guild_id, GuildId(Uuid::from_u128(1)));
        assert_eq!(channel.name, "geral");
        assert_eq!(channel.kind, ChannelKind::Voice);
        assert_eq!(channel.position, 3);
        assert_eq!(Channel::try_from(row("text")).unwrap().kind, ChannelKind::Text);
    }

    #[test]
    fn unknown_channel_kind_is_an_internal_error() {
        let err = Channel::try_from(row("video")).unwrap_err();
        assert!(matches!(err, AppError::Internal(_)));
    }
}
