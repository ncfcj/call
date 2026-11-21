//! Helpers dos testes de integração: monta o app e faz chamadas HTTP sem abrir socket.

#![allow(dead_code)] // cada arquivo de teste usa uma parte

use axum::{
    Router,
    body::Body,
    http::{HeaderValue, Request, StatusCode, header},
};
use call_server::{routes, state::AppState};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

/// Origem aceita no upgrade do WebSocket nos testes.
pub const WEB_ORIGIN: &str = "http://localhost:8080";

const SECRET: &str = "segredo-de-teste-com-mais-de-32-bytes";

pub fn app(db: PgPool) -> Router {
    routes::router(AppState::new(db, SECRET, true, vec![HeaderValue::from_static(WEB_ORIGIN)]))
}

pub struct Reply {
    pub status: StatusCode,
    /// Valor do cookie `refresh_token` no `Set-Cookie`, se houver (vazio quando é apagado).
    pub refresh_cookie: Option<String>,
    pub set_cookie: Option<String>,
    pub body: Value,
}

pub async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    bearer: Option<&str>,
    refresh_cookie: Option<&str>,
) -> Reply {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(token) = bearer {
        req = req.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(value) = refresh_cookie {
        req = req.header(header::COOKIE, format!("refresh_token={value}"));
    }
    let req = match body {
        Some(json) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };

    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let set_cookie = res.headers().get(header::SET_COOKIE).map(|v| v.to_str().unwrap().to_owned());
    let refresh_cookie = set_cookie.as_deref().and_then(|c| {
        c.strip_prefix("refresh_token=").map(|rest| rest.split(';').next().unwrap().to_owned())
    });
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() };
    Reply { status, refresh_cookie, set_cookie, body }
}

pub async fn register(app: &Router, username: &str) -> Reply {
    let body = json!({"username": username, "display_name": "Fulano", "password": "senha-forte-1"});
    call(app, "POST", "/auth/register", Some(body), None, None).await
}
