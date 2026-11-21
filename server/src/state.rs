use std::sync::Arc;

use axum::http::HeaderValue;
use sqlx::PgPool;

use crate::{
    auth::jwt::JwtKeys,
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
}

impl AppState {
    pub fn new(
        db: PgPool,
        jwt_secret: &str,
        cookie_secure: bool,
        web_origins: Vec<HeaderValue>,
    ) -> Self {
        Self {
            db,
            jwt: Arc::new(JwtKeys::new(jwt_secret.as_bytes())),
            cookie_secure,
            web_origins: Arc::new(web_origins),
            tickets: Arc::new(Tickets::new()),
            hub: Arc::new(Hub::new()),
        }
    }
}
