use std::sync::Arc;

use axum::http::{HeaderName, HeaderValue};
use sqlx::PgPool;

use crate::{
    auth::jwt::JwtKeys,
    limit::Limits,
    voice::LiveKit,
    voice_state::VoiceStates,
    ws::{hub::Hub, ticket::Tickets},
};

/// Estado compartilhado entre os handlers. Clonar é barato (tudo pesado está atrás de `Arc`).
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub jwt: Arc<JwtKeys>,
    pub cookie_secure: bool,
    /// Origens aceitas no upgrade do WebSocket (as mesmas do CORS).
    pub web_origins: Arc<Vec<HeaderValue>>,
    pub tickets: Arc<Tickets>,
    pub hub: Arc<Hub>,
    pub livekit: Arc<LiveKit>,
    pub voice_states: Arc<VoiceStates>,
    pub limits: Arc<Limits>,
    /// Header com o IP do client, escrito por um proxy na frente (ver `limit.rs`).
    pub client_ip_header: Option<HeaderName>,
    /// Código exigido para criar conta. Sem ele, o cadastro é aberto.
    pub signup_code: Option<String>,
    /// Prazo das mensagens, para o client avisar até onde vai o histórico.
    pub message_ttl_days: u32,
}

impl AppState {
    pub fn new(
        db: PgPool,
        jwt_secret: &str,
        cookie_secure: bool,
        web_origins: Vec<HeaderValue>,
        livekit: LiveKit,
        message_ttl_days: u32,
    ) -> Self {
        Self {
            db,
            jwt: Arc::new(JwtKeys::new(jwt_secret.as_bytes())),
            cookie_secure,
            web_origins: Arc::new(web_origins),
            tickets: Arc::new(Tickets::new()),
            hub: Arc::new(Hub::new()),
            livekit: Arc::new(livekit),
            voice_states: Arc::new(VoiceStates::new()),
            limits: Arc::default(),
            client_ip_header: None,
            signup_code: None,
            message_ttl_days,
        }
    }
}
