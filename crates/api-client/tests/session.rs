//! A sessão do `ApiClient` contra uma API falsa local: o access token fica guardado depois do
//! login, um 401 faz um refresh e repete a chamada uma vez, e um refresh recusado encerra a
//! sessão. Só no alvo nativo (a API falsa usa axum e tokio).
#![cfg(not(target_arch = "wasm32"))]

use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU16, AtomicUsize, Ordering},
    },
};

use api_client::{ApiClient, ApiError};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use proto::{LoginRequest, User, UserId};
use serde_json::json;

/// O que a API falsa viu e como deve responder ao refresh.
#[derive(Default)]
struct Fake {
    refreshes: AtomicUsize,
    /// Status do refresh: 200 (padrão), 401 ou 500.
    refresh_status: AtomicU16,
    /// `Authorization` de cada `GET /users/@me/guilds`.
    seen_auth: Mutex<Vec<Option<String>>>,
    /// `Cookie` de cada refresh.
    seen_cookie: Mutex<Vec<Option<String>>>,
}

type Shared = Arc<Fake>;

fn auth_body(token: &str) -> serde_json::Value {
    let user =
        User { id: UserId(uuid::Uuid::nil()), username: "ana".into(), display_name: "Ana".into() };
    json!({"access_token": token, "expires_in": 900, "user": user})
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({"code": "unauthorized", "message": "sessão inválida"})))
        .into_response()
}

async fn login() -> Response {
    // "velho" faz o papel de um access token já vencido.
    let cookie = "refresh_token=r1; Path=/auth; HttpOnly; SameSite=Lax; Max-Age=3600";
    ([(header::SET_COOKIE, cookie)], Json(auth_body("velho"))).into_response()
}

async fn refresh(State(fake): State<Shared>, headers: HeaderMap) -> Response {
    fake.refreshes.fetch_add(1, Ordering::SeqCst);
    let cookie = headers.get(header::COOKIE).map(|v| v.to_str().unwrap().to_owned());
    fake.seen_cookie.lock().unwrap().push(cookie);
    match fake.refresh_status.load(Ordering::SeqCst) {
        0 | 200 => Json(auth_body("novo")).into_response(),
        401 => StatusCode::UNAUTHORIZED.into_response(),
        other => StatusCode::from_u16(other).unwrap().into_response(),
    }
}

/// Só aceita o token "novo".
async fn guilds(State(fake): State<Shared>, headers: HeaderMap) -> Response {
    let auth = headers.get(header::AUTHORIZATION).map(|v| v.to_str().unwrap().to_owned());
    fake.seen_auth.lock().unwrap().push(auth.clone());
    if auth.as_deref() == Some("Bearer novo") {
        Json(json!([])).into_response()
    } else {
        unauthorized()
    }
}

async fn start() -> (ApiClient, Shared) {
    let fake = Shared::default();
    let app = Router::new()
        .route("/auth/login", post(login))
        .route("/auth/refresh", post(refresh))
        .route("/users/@me/guilds", get(guilds))
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, app).into_future());
    (ApiClient::new(format!("http://{addr}")), fake)
}

#[tokio::test]
async fn refresh_cookie_survives_a_new_client() {
    let (api, fake) = start().await;
    assert_eq!(api.refresh_cookie(), None, "sem sessão, sem cookie");
    sign_in(&api).await;
    let saved = api.refresh_cookie().unwrap();
    assert_eq!(saved, "refresh_token=r1");

    // Como abrir o app de novo: um cliente novo com o cookie salvo.
    let reopened = ApiClient::new(api.base_url());
    reopened.restore_refresh_cookie(&saved);
    assert_eq!(reopened.refresh().await.unwrap().username, "ana");
    assert_eq!(fake.seen_cookie.lock().unwrap().clone(), [Some(saved)]);
}

async fn sign_in(api: &ApiClient) {
    let req = LoginRequest { username: "ana".into(), password: "senha-forte-1".into() };
    assert_eq!(api.login(&req).await.unwrap().username, "ana");
}

#[tokio::test]
async fn expired_access_token_is_refreshed_and_the_call_repeated_once() {
    let (api, fake) = start().await;
    sign_in(&api).await;

    assert!(api.my_guilds().await.unwrap().is_empty());
    assert_eq!(fake.refreshes.load(Ordering::SeqCst), 1);
    let seen = fake.seen_auth.lock().unwrap().clone();
    assert_eq!(seen, [Some("Bearer velho".to_owned()), Some("Bearer novo".to_owned())]);

    // O token novo fica guardado: a próxima chamada já vai com ele, sem refresh.
    api.my_guilds().await.unwrap();
    assert_eq!(fake.refreshes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn without_a_session_there_is_no_refresh() {
    let (api, fake) = start().await;
    let err = api.my_guilds().await.unwrap_err();
    assert!(err.is_unauthorized());
    assert_eq!(fake.refreshes.load(Ordering::SeqCst), 0);
    assert_eq!(fake.seen_auth.lock().unwrap().clone(), [None], "sem Authorization");
}

#[tokio::test]
async fn refused_refresh_ends_the_session() {
    let (api, fake) = start().await;
    sign_in(&api).await;
    fake.refresh_status.store(401, Ordering::SeqCst);

    let err = api.my_guilds().await.unwrap_err();
    assert!(err.is_unauthorized(), "{err:?}");
    // Sem token, a próxima chamada vai sem Authorization e não tenta outro refresh.
    let _ = api.my_guilds().await;
    assert_eq!(fake.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(fake.seen_auth.lock().unwrap().last().cloned().flatten(), None);
}

#[tokio::test]
async fn refresh_server_error_is_reported_as_is() {
    let (api, fake) = start().await;
    fake.refresh_status.store(500, Ordering::SeqCst);
    assert!(matches!(api.refresh().await, Err(ApiError::Status(500))));
    fake.refresh_status.store(200, Ordering::SeqCst);
    assert_eq!(api.refresh().await.unwrap().username, "ana");
}

#[tokio::test]
async fn api_errors_carry_the_server_message() {
    let (api, _fake) = start().await;
    match api.my_guilds().await {
        Err(ApiError::Api { status, body }) => {
            assert_eq!(status, 401);
            assert_eq!(body.message, "sessão inválida");
        }
        other => panic!("esperava ApiError::Api, veio {other:?}"),
    }
}
