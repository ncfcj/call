//! Mensagens de canais de texto: corpos de REST, paginação do histórico e regras de validação.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{ChannelId, MessageId, User};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub channel_id: ChannelId,
    /// `None` quando a conta do autor foi apagada.
    pub author: Option<User>,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
}

/// `POST /channels/{id}/messages`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageRequest {
    pub content: String,
}

/// Query de `GET /channels/{id}/messages`.
///
/// - Sem cursor: as `limit` mensagens mais recentes.
/// - `before`: as `limit` imediatamente anteriores a esse id (rolar o histórico para cima).
/// - `after`: as `limit` imediatamente posteriores (recuperar o que perdeu ao reconectar).
///
/// A resposta vem sempre em ordem cronológica (da mais antiga para a mais nova).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessagesQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<MessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<MessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

pub const MESSAGE_LEN: std::ops::RangeInclusive<usize> = 1..=4000;
pub const MESSAGES_PAGE_DEFAULT: u32 = 50;
pub const MESSAGES_PAGE_MAX: u32 = 100;

/// Valida o conteúdo de uma mensagem. Espaços nas pontas são mantidos, mas uma mensagem só de
/// espaços é recusada.
pub fn validate_message_content(content: &str) -> Result<(), &'static str> {
    if content.trim().is_empty() {
        return Err("a mensagem não pode ficar vazia");
    }
    if !MESSAGE_LEN.contains(&content.chars().count()) {
        return Err("a mensagem pode ter no máximo 4000 caracteres");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_rules() {
        assert!(validate_message_content("oi").is_ok());
        assert!(validate_message_content("   ").is_err());
        assert!(validate_message_content(&"a".repeat(4000)).is_ok());
        assert!(validate_message_content(&"a".repeat(4001)).is_err());
    }
}
