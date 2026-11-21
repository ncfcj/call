mod auth;
mod channels;
mod guilds;
mod health;
mod messages;
mod users;

use axum::{
    Router,
    routing::{delete, get, patch, post},
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
        .route("/users/@me/guilds", get(guilds::list_mine))
        .route("/guilds", post(guilds::create))
        .route("/guilds/{id}", get(guilds::get).patch(guilds::update).delete(guilds::delete))
        .route("/guilds/{id}/members", get(guilds::members))
        .route("/guilds/{id}/members/@me", delete(guilds::leave))
        .route("/guilds/{id}/channels", get(channels::list).post(channels::create))
        .route("/invites/{code}", post(guilds::join))
        .route("/channels/{id}", patch(channels::update).delete(channels::delete))
        .route("/channels/{id}/messages", get(messages::list).post(messages::create))
        .with_state(state)
}
