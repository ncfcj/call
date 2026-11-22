//! Tokens do LiveKit, assinados aqui com a chave da API, sem chamar o LiveKit.

use std::time::Duration;

use livekit_api::access_token::{AccessToken, VideoGrants};
use proto::{ChannelId, User};

/// Validade do token para entrar na sala. Curta: só precisa durar até o client conectar.
pub const VOICE_TOKEN_TTL: Duration = Duration::from_secs(5 * 60);

/// Máximo de pessoas numa sala de voz (premissa do MVP: grupos pequenos).
pub const VOICE_MAX_PARTICIPANTS: u32 = 10;

/// Configuração do LiveKit: onde os clients conectam e as chaves para assinar tokens.
#[derive(Clone)]
pub struct LiveKit {
    pub url: String,
    api_key: String,
    api_secret: String,
}

impl std::fmt::Debug for LiveKit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Sem o segredo, para não vazar em logs.
        f.debug_struct("LiveKit").field("url", &self.url).field("api_key", &self.api_key).finish()
    }
}

impl LiveKit {
    pub fn new(
        url: impl Into<String>,
        api_key: impl Into<String>,
        api_secret: impl Into<String>,
    ) -> Self {
        Self { url: url.into(), api_key: api_key.into(), api_secret: api_secret.into() }
    }

    /// Token para `user` entrar no canal de voz `channel`: só áudio do microfone, sem dados.
    pub fn join_token(&self, user: &User, channel: ChannelId) -> anyhow::Result<String> {
        let grants = VideoGrants {
            room_join: true,
            room: channel.to_string(),
            can_publish: Some(true),
            can_subscribe: Some(true),
            can_publish_data: Some(false),
            can_publish_sources: vec!["microphone".to_owned()],
            can_update_own_metadata: Some(false),
            ..Default::default()
        };
        // Vale quando é este join que cria a sala.
        let room = livekit_protocol::RoomConfiguration {
            max_participants: VOICE_MAX_PARTICIPANTS,
            ..Default::default()
        };
        AccessToken::with_api_key(&self.api_key, &self.api_secret)
            .with_identity(&user.id.to_string())
            .with_name(&user.display_name)
            .with_ttl(VOICE_TOKEN_TTL)
            .with_grants(grants)
            .with_room_config(room)
            .to_jwt()
            .map_err(|err| anyhow::anyhow!("não assinou o token do LiveKit: {err}"))
    }
}
