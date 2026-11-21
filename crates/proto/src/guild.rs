//! Servidores (guilds), membros, canais e convites: corpos de REST e regras de validação.

use serde::{Deserialize, Serialize};

use crate::{ChannelId, GuildId, User, UserId};

/// Servidor como os membros o veem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guild {
    pub id: GuildId,
    pub name: String,
    pub owner_id: UserId,
    /// Código para `POST /invites/{code}`. Qualquer membro pode compartilhar.
    pub invite_code: String,
}

/// `POST /guilds`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGuildRequest {
    pub name: String,
}

/// `PATCH /guilds/{id}`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateGuildRequest {
    pub name: String,
}

/// Item de `GET /guilds/{id}/members`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub user: User,
    pub is_owner: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelKind {
    Text,
    Voice,
}

impl ChannelKind {
    /// Valor da coluna `channels.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Voice => "voice",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "text" => Some(Self::Text),
            "voice" => Some(Self::Voice),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub guild_id: GuildId,
    pub name: String,
    pub kind: ChannelKind,
    /// Ordem na barra lateral, crescente.
    pub position: i32,
}

/// `POST /guilds/{id}/channels`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChannelRequest {
    pub name: String,
    pub kind: ChannelKind,
}

/// `PATCH /channels/{id}`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateChannelRequest {
    pub name: String,
}

pub const GUILD_NAME_LEN: std::ops::RangeInclusive<usize> = 1..=100;
pub const CHANNEL_NAME_LEN: std::ops::RangeInclusive<usize> = 1..=100;

/// Valida um nome de servidor já sem espaços nas pontas.
pub fn validate_guild_name(name: &str) -> Result<(), &'static str> {
    if !GUILD_NAME_LEN.contains(&name.chars().count()) {
        return Err("o nome do servidor precisa ter de 1 a 100 caracteres");
    }
    Ok(())
}

/// Valida um nome de canal já sem espaços nas pontas.
pub fn validate_channel_name(name: &str) -> Result<(), &'static str> {
    if !CHANNEL_NAME_LEN.contains(&name.chars().count()) {
        return Err("o nome do canal precisa ter de 1 a 100 caracteres");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_kind_wire_format_matches_db_column() {
        for kind in [ChannelKind::Text, ChannelKind::Voice] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
            assert_eq!(ChannelKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(ChannelKind::parse("video"), None);
    }

    #[test]
    fn name_rules() {
        assert!(validate_guild_name("Amigos").is_ok());
        assert!(validate_guild_name("").is_err());
        assert!(validate_guild_name(&"a".repeat(101)).is_err());
        assert!(validate_channel_name("geral").is_ok());
        assert!(validate_channel_name("").is_err());
    }
}
