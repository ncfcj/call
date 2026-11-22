//! Voz: o token para entrar numa sala do LiveKit.
//!
//! O áudio nunca passa pelo call-server. O client pede um token (`POST
//! /channels/{id}/voice/token`) e conecta direto ao LiveKit com ele. A sala do LiveKit tem o
//! nome do id do canal de voz, e a identidade do participante é o id do usuário.

use serde::{Deserialize, Serialize};

use crate::{ChannelId, GuildId, UserId};

/// Resposta de `POST /channels/{id}/voice/token`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceTokenResponse {
    /// URL WebSocket do LiveKit (`ws://` em desenvolvimento, `wss://` em produção).
    pub url: String,
    /// JWT do LiveKit. Vale só para entrar: depois de conectado, o LiveKit renova sozinho.
    pub token: String,
    /// Segundos até o token deixar de servir para entrar.
    pub expires_in: u64,
}

/// Onde alguém está na voz, e com que microfone e som. Vai no `SUBSCRIBED` (todos os de um
/// servidor) e em cada `VOICE_STATE_UPDATE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceState {
    pub guild_id: GuildId,
    pub user_id: UserId,
    /// Canal de voz em que a pessoa está; `None` quando acabou de sair.
    pub channel_id: Option<ChannelId>,
    pub muted: bool,
    pub deafened: bool,
}
