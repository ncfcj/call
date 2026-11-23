//! Tokens do LiveKit, assinados aqui com a chave da API, sem chamar o LiveKit.

use std::time::Duration;

use livekit_api::{
    access_token::{AccessToken, TokenVerifier, VideoGrants},
    webhooks::WebhookReceiver,
};
use livekit_protocol::WebhookEvent;
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
            // can_update_own_metadata fica no padrão do LiveKit, que é não poder.
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

    /// Confere a assinatura de um webhook do LiveKit (JWT no `Authorization` com o SHA-256 do
    /// corpo, assinado com a mesma chave) e devolve o evento.
    pub fn receive_webhook(&self, body: &str, authorization: &str) -> Option<WebhookEvent> {
        let token = authorization.strip_prefix("Bearer ").unwrap_or(authorization);
        let receiver =
            WebhookReceiver::new(TokenVerifier::with_api_key(&self.api_key, &self.api_secret));
        match receiver.receive(body, token) {
            Ok(event) => Some(event),
            Err(err) => {
                tracing::warn!(error = %err, "webhook do LiveKit recusado");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use proto::UserId;
    use sha2::{Digest, Sha256};
    use uuid::Uuid;

    use super::*;

    fn livekit() -> LiveKit {
        LiveKit::new("ws://lk", "chave", "segredo-do-livekit")
    }

    fn user() -> User {
        User { id: UserId(Uuid::from_u128(7)), username: "ana".into(), display_name: "Ana".into() }
    }

    #[test]
    fn join_token_allows_only_the_microphone_in_that_room() {
        let channel = ChannelId(Uuid::from_u128(42));
        let jwt = livekit().join_token(&user(), channel).unwrap();
        let claims =
            TokenVerifier::with_api_key("chave", "segredo-do-livekit").verify(&jwt).unwrap();
        assert_eq!(claims.iss, "chave");
        assert_eq!(claims.sub, user().id.to_string());
        assert_eq!(claims.name, "Ana");
        assert_eq!(claims.video.room, channel.to_string());
        assert!(claims.video.room_join);
        // Explícitos no token, sem depender do padrão do servidor do LiveKit.
        assert_eq!(claims.video.can_publish, Some(true));
        assert_eq!(claims.video.can_subscribe, Some(true));
        assert_eq!(claims.video.can_publish_data, Some(false));
        assert!(!claims.video.can_publish_data() && !claims.video.can_update_own_metadata());
        assert_eq!(claims.video.can_publish_sources, ["microphone"]);
        assert!(!claims.video.room_admin && !claims.video.room_create && !claims.video.room_list);
        assert_eq!(claims.exp - claims.nbf, usize::try_from(VOICE_TOKEN_TTL.as_secs()).unwrap());
        assert_eq!(claims.room_config.unwrap().max_participants, VOICE_MAX_PARTICIPANTS);
    }

    #[test]
    fn token_signed_with_another_secret_is_rejected() {
        let jwt = livekit().join_token(&user(), ChannelId(Uuid::nil())).unwrap();
        assert!(TokenVerifier::with_api_key("chave", "outro-segredo").verify(&jwt).is_err());
    }

    fn signed(body: &str, secret: &str) -> String {
        let hash = STANDARD.encode(Sha256::digest(body.as_bytes()));
        AccessToken::with_api_key("chave", secret).with_sha256(&hash).to_jwt().unwrap()
    }

    #[test]
    fn webhook_with_the_right_signature_is_read() {
        let body = r#"{"event":"participant_joined","room":{"name":"sala"},"participant":{"sid":"PA_1","identity":"u1"}}"#;
        let auth = signed(body, "segredo-do-livekit");
        for header in [auth.clone(), format!("Bearer {auth}")] {
            let event = livekit().receive_webhook(body, &header).unwrap();
            assert_eq!(event.event, "participant_joined");
            assert_eq!(event.room.unwrap().name, "sala");
            assert_eq!(event.participant.unwrap().identity, "u1");
        }
    }

    #[test]
    fn webhook_is_rejected_when_signature_or_body_do_not_match() {
        let body = r#"{"event":"room_finished","room":{"name":"sala"}}"#;
        assert!(livekit().receive_webhook(body, &signed(body, "segredo-errado")).is_none());
        let other = r#"{"event":"room_finished","room":{"name":"outra"}}"#;
        assert!(livekit().receive_webhook(other, &signed(body, "segredo-do-livekit")).is_none());
        assert!(livekit().receive_webhook(body, "").is_none());
    }

    #[test]
    fn debug_never_shows_the_secret() {
        let text = format!("{:?}", livekit());
        assert!(text.contains("chave") && text.contains("ws://lk"));
        assert!(!text.contains("segredo-do-livekit"));
    }
}
