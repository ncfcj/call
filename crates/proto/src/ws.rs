//! WebSocket: ticket de conexão e envelope de eventos.
//!
//! Formato no fio: `{"t": "<TIPO>", "d": { ... }}`.
//!
//! Fluxo: `POST /ws/ticket` (com Bearer) devolve um ticket de uso único válido por 30 s;
//! `GET /ws?ticket=...` faz o upgrade. O servidor manda `HELLO`, e o client manda `HEARTBEAT`
//! nesse intervalo e um `SUBSCRIBE` para cada servidor (guild) que quer acompanhar.

use serde::{Deserialize, Serialize};

use crate::{ChannelId, ErrorCode, GuildId, Message, UserId, VoiceState};

/// Resposta de `POST /ws/ticket`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WsTicketResponse {
    pub ticket: String,
    /// Segundos até o ticket expirar.
    pub expires_in: u64,
}

/// Query de `GET /ws`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsQuery {
    pub ticket: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresenceStatus {
    Online,
    Offline,
}

/// Eventos enviados pelo servidor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ServerEvent {
    /// Primeira mensagem após o upgrade: diz de quanto em quanto tempo o client manda heartbeat.
    /// Sem nenhuma mensagem do client por duas vezes esse intervalo, o servidor fecha a conexão.
    Hello {
        heartbeat_interval_ms: u64,
        user_id: UserId,
    },
    HeartbeatAck,
    /// Resposta a `SUBSCRIBE`: a partir daqui chegam os eventos do servidor. `online` lista
    /// quem tem uma conexão inscrita nele agora, incluindo você; `voice`, quem está em cada
    /// canal de voz.
    Subscribed {
        guild_id: GuildId,
        online: Vec<UserId>,
        #[serde(default)]
        voice: Vec<VoiceState>,
    },
    /// Alguém entrou, saiu ou trocou de canal de voz, ou mudou o mute ou o deaf.
    VoiceStateUpdate(VoiceState),
    /// Mensagem nova num canal de texto do servidor.
    MessageCreate(Message),
    /// Alguém começou a digitar. O client mostra o aviso por uns 8 s.
    TypingStart {
        guild_id: GuildId,
        channel_id: ChannelId,
        user_id: UserId,
    },
    /// Alguém ficou online ou offline neste servidor.
    PresenceUpdate {
        guild_id: GuildId,
        user_id: UserId,
        status: PresenceStatus,
    },
    /// Um evento do client foi recusado. A conexão continua aberta.
    Error {
        code: ErrorCode,
        message: String,
    },
}

/// Eventos enviados pelo client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClientEvent {
    Heartbeat,
    /// Passa a receber os eventos de um servidor do qual você é membro.
    Subscribe {
        guild_id: GuildId,
    },
    /// Você começou a digitar num canal de texto. Mande de novo a cada ~8 s enquanto digita.
    TypingStart {
        channel_id: ChannelId,
    },
    /// Seu estado de voz: em que canal está (ou `None`) e o mute e o deaf. Mande ao conectar e a
    /// cada mudança. A presença no canal também vem do LiveKit (webhook), que é quem decide a
    /// saída; o aviso do client serve para o servidor saber, depois de reiniciar, quem já estava
    /// numa chamada.
    VoiceStateUpdate {
        channel_id: Option<ChannelId>,
        muted: bool,
        deafened: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn hello_wire_format() {
        let user_id = UserId(Uuid::nil());
        let json =
            serde_json::to_string(&ServerEvent::Hello { heartbeat_interval_ms: 30_000, user_id })
                .unwrap();
        assert_eq!(
            json,
            r#"{"t":"HELLO","d":{"heartbeat_interval_ms":30000,"user_id":"00000000-0000-0000-0000-000000000000"}}"#
        );
    }

    #[test]
    fn client_events_roundtrip() {
        let json = r#"{"t":"HEARTBEAT"}"#;
        assert_eq!(serde_json::from_str::<ClientEvent>(json).unwrap(), ClientEvent::Heartbeat);

        let json = r#"{"t":"SUBSCRIBE","d":{"guild_id":"00000000-0000-0000-0000-000000000000"}}"#;
        assert_eq!(
            serde_json::from_str::<ClientEvent>(json).unwrap(),
            ClientEvent::Subscribe { guild_id: GuildId(Uuid::nil()) }
        );
    }

    #[test]
    fn voice_state_wire_format() {
        let json =
            r#"{"t":"VOICE_STATE_UPDATE","d":{"channel_id":null,"muted":true,"deafened":false}}"#;
        assert_eq!(
            serde_json::from_str::<ClientEvent>(json).unwrap(),
            ClientEvent::VoiceStateUpdate { channel_id: None, muted: true, deafened: false }
        );
        // Um SUBSCRIBED de um servidor antigo, sem `voice`, ainda é lido.
        let json = r#"{"t":"SUBSCRIBED","d":{"guild_id":"00000000-0000-0000-0000-000000000000","online":[]}}"#;
        assert!(matches!(
            serde_json::from_str::<ServerEvent>(json).unwrap(),
            ServerEvent::Subscribed { voice, .. } if voice.is_empty()
        ));
    }

    #[test]
    fn presence_wire_format() {
        let event = ServerEvent::PresenceUpdate {
            guild_id: GuildId(Uuid::nil()),
            user_id: UserId(Uuid::nil()),
            status: PresenceStatus::Online,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["t"], "PRESENCE_UPDATE");
        assert_eq!(json["d"]["status"], "online");
    }
}
