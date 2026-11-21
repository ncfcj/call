use axum::{Json, extract::State};
use proto::{HealthResponse, HealthStatus};

use crate::state::AppState;

/// `GET /health`: responde sempre 200 e informa se o banco está acessível.
pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let database = sqlx::query("SELECT 1").execute(&state.db).await.is_ok();
    Json(HealthResponse {
        status: if database { HealthStatus::Ok } else { HealthStatus::Degraded },
        version: env!("CARGO_PKG_VERSION").to_owned(),
        database,
    })
}
