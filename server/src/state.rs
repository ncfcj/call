use std::sync::Arc;

use sqlx::PgPool;

use crate::auth::jwt::JwtKeys;

/// Estado compartilhado entre os handlers. Clonar é barato (o pool e as chaves são `Arc`).
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub jwt: Arc<JwtKeys>,
    pub cookie_secure: bool,
}

impl AppState {
    pub fn new(db: PgPool, jwt_secret: &str, cookie_secure: bool) -> Self {
        Self { db, jwt: Arc::new(JwtKeys::new(jwt_secret.as_bytes())), cookie_secure }
    }
}
