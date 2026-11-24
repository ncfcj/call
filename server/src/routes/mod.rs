mod auth;
mod channels;
mod guilds;
mod health;
mod messages;
mod users;
pub(crate) mod voice;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, header},
    middleware::map_response,
    routing::{delete, get, patch, post},
};
use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};

use crate::{error::json_errors, state::AppState, ws};

/// Folgado para uma mensagem de 4000 caracteres, mesmo toda escapada no JSON (48 KB).
pub(crate) const MAX_BODY: usize = 64 * 1024;

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
        .route("/channels/{id}/voice/token", post(voice::token))
        .route("/livekit/webhook", post(voice::webhook))
        .route("/ws/ticket", post(ws::ticket))
        .route("/ws", get(ws::upgrade))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(map_response(json_errors))
        .with_state(state)
}

/// Serve o client compilado em `dir` no que não é rota da API. O navegador sempre confere se
/// mudou (304), porque o `index.html` não leva hash no nome.
pub fn serve_web(router: Router, dir: &str) -> Router {
    let files = Router::new().fallback_service(ServeDir::new(dir).precompressed_gzip());
    router.fallback_service(files.layer(SetResponseHeaderLayer::overriding(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache"),
    )))
}
