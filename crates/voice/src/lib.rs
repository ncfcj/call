//! Ponte com o `livekit-client` (JS). A voz usa o WebRTC do navegador (ou do WebView no Tauri).
//!
//! O SDK Rust do LiveKit não é usado aqui porque depende do libwebrtc nativo e não compila para
//! wasm32. A parte JavaScript fica em `js/voice.js` e é o único lugar que conhece a API do
//! LiveKit; daqui para cima tudo é tipado.
//!
//! - [`VoiceRoom`] (só wasm32): `connect`, `disconnect`, `set_microphone_enabled`, `start_audio`.
//! - [`VoiceEvent`]: o que acontece na sala, entregue ao callback passado em [`VoiceRoom::new`].

use serde::Deserialize;

#[cfg(target_arch = "wasm32")]
mod room;
#[cfg(target_arch = "wasm32")]
pub use room::{VoiceError, VoiceRoom};

/// Alguém na sala de voz.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct VoiceParticipant {
    /// Identidade no LiveKit: o id do usuário.
    pub identity: String,
    /// Nome de exibição, vindo do token.
    pub name: String,
    /// É você.
    pub local: bool,
    /// Está com o microfone publicado e aberto.
    pub mic: bool,
}

/// Eventos da sala, no mesmo formato que `js/voice.js` emite.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VoiceEvent {
    /// A lista completa de quem está na sala mudou.
    Participants(Vec<VoiceParticipant>),
    /// Quem está falando agora (identidades), do mais alto para o mais baixo.
    ActiveSpeakers(Vec<String>),
    /// A conexão com o LiveKit caiu e ele está tentando voltar sozinho.
    Reconnecting,
    Reconnected,
    /// Saiu da sala: por pedido seu, porque o servidor encerrou, ou porque não conseguiu voltar.
    Disconnected {
        reason: String,
    },
    /// O microfone não abriu (permissão negada ou sem dispositivo). Você continua ouvindo.
    MicError {
        message: String,
    },
    /// O navegador bloqueou a reprodução de áudio até um clique (`start_audio`).
    AudioBlocked(bool),
}

impl VoiceEvent {
    /// Lê um evento do JSON da ponte JS. Evento desconhecido vira `None`.
    pub fn parse(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_bridge_wire_format() {
        let json =
            r#"{"t":"PARTICIPANTS","d":[{"identity":"u1","name":"Ana","local":true,"mic":true}]}"#;
        assert_eq!(
            VoiceEvent::parse(json),
            Some(VoiceEvent::Participants(vec![VoiceParticipant {
                identity: "u1".into(),
                name: "Ana".into(),
                local: true,
                mic: true,
            }]))
        );
        assert_eq!(
            VoiceEvent::parse(r#"{"t":"ACTIVE_SPEAKERS","d":["u1","u2"]}"#),
            Some(VoiceEvent::ActiveSpeakers(vec!["u1".into(), "u2".into()]))
        );
        assert_eq!(VoiceEvent::parse(r#"{"t":"RECONNECTING"}"#), Some(VoiceEvent::Reconnecting));
        assert_eq!(
            VoiceEvent::parse(r#"{"t":"DISCONNECTED","d":{"reason":"1"}}"#),
            Some(VoiceEvent::Disconnected { reason: "1".into() })
        );
        assert_eq!(
            VoiceEvent::parse(r#"{"t":"AUDIO_BLOCKED","d":true}"#),
            Some(VoiceEvent::AudioBlocked(true))
        );
        assert_eq!(VoiceEvent::parse(r#"{"t":"NOVO_EVENTO"}"#), None);
    }
}
