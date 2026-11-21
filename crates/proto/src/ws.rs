//! Envelope de eventos do WebSocket.
//!
//! Formato no fio: `{"t": "<TIPO>", "d": { ... }}`. A Fase 1 acrescenta
//! `Subscribe`, `MessageCreate`, `TypingStart` e `PresenceUpdate`.

use serde::{Deserialize, Serialize};

/// Eventos enviados pelo servidor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ServerEvent {
    /// Primeira mensagem após o upgrade: diz de quanto em quanto tempo o client manda heartbeat.
    Hello {
        heartbeat_interval_ms: u64,
    },
    HeartbeatAck,
}

/// Eventos enviados pelo client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClientEvent {
    Heartbeat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_wire_format() {
        let json =
            serde_json::to_string(&ServerEvent::Hello { heartbeat_interval_ms: 30_000 }).unwrap();
        assert_eq!(json, r#"{"t":"HELLO","d":{"heartbeat_interval_ms":30000}}"#);
    }

    #[test]
    fn heartbeat_roundtrip() {
        let json = r#"{"t":"HEARTBEAT"}"#;
        assert_eq!(serde_json::from_str::<ClientEvent>(json).unwrap(), ClientEvent::Heartbeat);
    }
}
