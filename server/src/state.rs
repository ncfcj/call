use sqlx::PgPool;

/// Estado compartilhado entre os handlers. Clonar é barato (o pool é um `Arc`).
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
}
