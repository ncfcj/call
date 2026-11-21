use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use proto::{
    ChannelKind, CreateGuildRequest, Guild, GuildId, Member, UpdateGuildRequest, User, UserId,
    guild::validate_guild_name,
};
use rand::RngExt;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    access::guild_member,
    auth::AuthUser,
    error::{AppError, AppResult},
    state::AppState,
};

/// Canais criados junto com todo servidor novo.
const DEFAULT_CHANNELS: [(&str, ChannelKind); 2] =
    [("geral", ChannelKind::Text), ("Geral", ChannelKind::Voice)];

const INVITE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
const INVITE_LEN: usize = 8;

/// Código de convite aleatório, sem caracteres fáceis de confundir (0/O, 1/l/I).
fn invite_code() -> String {
    let mut rng = rand::rng();
    (0..INVITE_LEN)
        .map(|_| INVITE_ALPHABET[rng.random_range(0..INVITE_ALPHABET.len())] as char)
        .collect()
}

#[derive(sqlx::FromRow)]
pub struct GuildRow {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub invite_code: String,
}

impl From<GuildRow> for Guild {
    fn from(row: GuildRow) -> Self {
        Self {
            id: GuildId(row.id),
            name: row.name,
            owner_id: UserId(row.owner_id),
            invite_code: row.invite_code,
        }
    }
}

async fn fetch_guild(db: &PgPool, id: GuildId) -> AppResult<Guild> {
    let row: Option<GuildRow> =
        sqlx::query_as("SELECT id, name, owner_id, invite_code FROM guilds WHERE id = $1")
            .bind(id.0)
            .fetch_optional(db)
            .await?;
    row.map(Guild::from).ok_or(AppError::NotFound)
}

fn clean_name(raw: &str) -> AppResult<String> {
    let name = raw.trim().to_owned();
    validate_guild_name(&name).map_err(AppError::Validation)?;
    Ok(name)
}

/// `POST /guilds`: cria o servidor, põe o dono como membro e cria os canais padrão.
pub async fn create(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(req): Json<CreateGuildRequest>,
) -> AppResult<(StatusCode, Json<Guild>)> {
    let name = clean_name(&req.name)?;
    let id = GuildId(Uuid::now_v7());

    // Uma colisão de código de convite desfaz a transação inteira; tenta de novo com outro.
    for _ in 0..3 {
        let code = invite_code();
        let mut tx = state.db.begin().await?;
        let inserted = sqlx::query(
            "INSERT INTO guilds (id, name, owner_id, invite_code) VALUES ($1, $2, $3, $4)",
        )
        .bind(id.0)
        .bind(&name)
        .bind(user.0)
        .bind(&code)
        .execute(&mut *tx)
        .await;
        match inserted {
            Ok(_) => {}
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => continue,
            Err(e) => return Err(e.into()),
        }

        sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
            .bind(id.0)
            .bind(user.0)
            .execute(&mut *tx)
            .await?;
        for (position, (channel, kind)) in DEFAULT_CHANNELS.iter().enumerate() {
            sqlx::query(
                "INSERT INTO channels (id, guild_id, name, kind, position)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(Uuid::now_v7())
            .bind(id.0)
            .bind(channel)
            .bind(kind.as_str())
            .bind(position as i32)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;

        let guild = Guild { id, name, owner_id: user, invite_code: code };
        return Ok((StatusCode::CREATED, Json(guild)));
    }
    Err(anyhow::anyhow!("não consegui gerar um código de convite único").into())
}

/// `GET /users/@me/guilds`: servidores do usuário, na ordem em que entrou.
pub async fn list_mine(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> AppResult<Json<Vec<Guild>>> {
    let rows: Vec<GuildRow> = sqlx::query_as(
        "SELECT g.id, g.name, g.owner_id, g.invite_code FROM guilds g
         JOIN guild_members m ON m.guild_id = g.id
         WHERE m.user_id = $1
         ORDER BY m.joined_at, g.id",
    )
    .bind(user.0)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(Guild::from).collect()))
}

/// `GET /guilds/{id}`
pub async fn get(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<GuildId>,
) -> AppResult<Json<Guild>> {
    guild_member(&state.db, id, user).await?;
    Ok(Json(fetch_guild(&state.db, id).await?))
}

/// `PATCH /guilds/{id}`: renomeia. Só o dono.
pub async fn update(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<GuildId>,
    Json(req): Json<UpdateGuildRequest>,
) -> AppResult<Json<Guild>> {
    guild_member(&state.db, id, user).await?.require_owner()?;
    let name = clean_name(&req.name)?;
    let row: GuildRow = sqlx::query_as(
        "UPDATE guilds SET name = $2 WHERE id = $1
         RETURNING id, name, owner_id, invite_code",
    )
    .bind(id.0)
    .bind(name)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(row.into()))
}

/// `DELETE /guilds/{id}`: apaga o servidor, com canais e mensagens. Só o dono.
pub async fn delete(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<GuildId>,
) -> AppResult<StatusCode> {
    guild_member(&state.db, id, user).await?.require_owner()?;
    sqlx::query("DELETE FROM guilds WHERE id = $1").bind(id.0).execute(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(sqlx::FromRow)]
struct MemberRow {
    id: Uuid,
    username: String,
    display_name: String,
    is_owner: bool,
}

/// `GET /guilds/{id}/members`
pub async fn members(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<GuildId>,
) -> AppResult<Json<Vec<Member>>> {
    guild_member(&state.db, id, user).await?;
    let rows: Vec<MemberRow> = sqlx::query_as(
        "SELECT u.id, u.username, u.display_name, (u.id = g.owner_id) AS is_owner
         FROM guild_members m
         JOIN users u ON u.id = m.user_id
         JOIN guilds g ON g.id = m.guild_id
         WHERE m.guild_id = $1
         ORDER BY m.joined_at, u.id",
    )
    .bind(id.0)
    .fetch_all(&state.db)
    .await?;
    let members = rows
        .into_iter()
        .map(|r| Member {
            user: User { id: UserId(r.id), username: r.username, display_name: r.display_name },
            is_owner: r.is_owner,
        })
        .collect();
    Ok(Json(members))
}

/// `DELETE /guilds/{id}/members/@me`: sai do servidor. O dono não sai; ele apaga o servidor.
pub async fn leave(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<GuildId>,
) -> AppResult<StatusCode> {
    if guild_member(&state.db, id, user).await?.is_owner() {
        return Err(AppError::Forbidden("o dono não pode sair do servidor; apague-o"));
    }
    sqlx::query("DELETE FROM guild_members WHERE guild_id = $1 AND user_id = $2")
        .bind(id.0)
        .bind(user.0)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /invites/{code}`: entra no servidor do convite. Quem já é membro recebe o mesmo 200.
pub async fn join(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(code): Path<String>,
) -> AppResult<Json<Guild>> {
    let row: Option<GuildRow> =
        sqlx::query_as("SELECT id, name, owner_id, invite_code FROM guilds WHERE invite_code = $1")
            .bind(&code)
            .fetch_optional(&state.db)
            .await?;
    let guild = Guild::from(row.ok_or(AppError::NotFound)?);
    sqlx::query(
        "INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
    )
    .bind(guild.id.0)
    .bind(user.0)
    .execute(&state.db)
    .await?;
    Ok(Json(guild))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_codes_use_the_unambiguous_alphabet() {
        let code = invite_code();
        assert_eq!(code.len(), INVITE_LEN);
        assert!(code.bytes().all(|b| INVITE_ALPHABET.contains(&b)));
        assert_ne!(invite_code(), code, "dois códigos seguidos iguais é improvável");
    }
}
