mod auth;
mod health;
mod users;

use axum::{
    Router,
    routing::{get, post},
};

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/refresh", post(auth::refresh))
        .route("/auth/logout", post(auth::logout))
        .route("/users/@me", get(users::me))
        .with_state(state)
}
