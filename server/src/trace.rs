//! Log de cada requisição, com um id e quem, em que servidor e em que canal.

use axum::{extract::MatchedPath, http::Request};
use tower_http::{
    classify::{ServerErrorsAsFailures, SharedClassifier},
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing::{Level, Span, field::Empty};
use uuid::Uuid;

/// Os campos vazios são preenchidos em `AuthUser` e em `access.rs`.
fn span<B>(req: &Request<B>) -> Span {
    // Só o padrão da rota: a URL traz segredos (o ticket do WebSocket, o código do convite).
    let route = req.extensions().get::<MatchedPath>().map_or("-", MatchedPath::as_str);
    tracing::info_span!(
        "http",
        request_id = %Uuid::now_v7(),
        method = %req.method(),
        route,
        user = Empty,
        guild = Empty,
        channel = Empty,
    )
}

type MakeSpan<B> = fn(&Request<B>) -> Span;

pub fn layer<B>() -> TraceLayer<SharedClassifier<ServerErrorsAsFailures>, MakeSpan<B>> {
    TraceLayer::new_for_http()
        .make_span_with(span as MakeSpan<B>)
        .on_response(DefaultOnResponse::new().level(Level::INFO))
}
