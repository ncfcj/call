//! Testes de integração da autenticação, contra um Postgres de verdade.
//!
//! `#[sqlx::test]` cria um banco novo por teste (a partir de `DATABASE_URL`) e aplica as
//! migrations. Rode com o `docker compose up -d` ligado.

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use call_server::{routes, state::AppState};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

const SECRET: &str = "segredo-de-teste-com-mais-de-32-bytes";

fn app(db: PgPool) -> Router {
    routes::router(AppState::new(db, SECRET, true))
}

struct Reply {
    status: StatusCode,
    /// Valor do cookie `refresh_token` no `Set-Cookie`, se houver (vazio quando é apagado).
    refresh_cookie: Option<String>,
    set_cookie: Option<String>,
    body: Value,
}

async fn call(
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

async fn register(app: &Router, username: &str) -> Reply {
    let body = json!({"username": username, "display_name": "Fulano", "password": "senha-forte-1"});
    call(app, "POST", "/auth/register", Some(body), None, None).await
}

#[sqlx::test(migrations = "../migrations")]
async fn register_opens_a_session(db: PgPool) {
    let app = app(db);
    let r = register(&app, "  Fulano ").await;

    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(r.body["user"]["username"], "fulano", "username normalizado");
    assert_eq!(r.body["expires_in"], 900);
    assert!(r.body.get("password_hash").is_none());

    let cookie = r.set_cookie.unwrap();
    for attr in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/auth", "Max-Age="] {
        assert!(cookie.contains(attr), "cookie sem {attr}: {cookie}");
    }

    let token = r.body["access_token"].as_str().unwrap();
    let me = call(&app, "GET", "/users/@me", None, Some(token), None).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["username"], "fulano");
}

#[sqlx::test(migrations = "../migrations")]
async fn duplicate_username_is_rejected_case_insensitively(db: PgPool) {
    let app = app(db);
    assert_eq!(register(&app, "fulano").await.status, StatusCode::CREATED);
    let r = register(&app, "FULANO").await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.body["code"], "username_taken");
}

#[sqlx::test(migrations = "../migrations")]
async fn invalid_input_is_a_validation_error(db: PgPool) {
    let app = app(db);
    let cases = [
        json!({"username": "a", "display_name": "X", "password": "senha-forte-1"}),
        json!({"username": "com espaco", "display_name": "X", "password": "senha-forte-1"}),
        json!({"username": "fulano", "display_name": "  ", "password": "senha-forte-1"}),
        json!({"username": "fulano", "display_name": "X", "password": "curta"}),
    ];
    for body in cases {
        let r = call(&app, "POST", "/auth/register", Some(body.clone()), None, None).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(r.body["code"], "validation");
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn login_checks_the_password(db: PgPool) {
    let app = app(db);
    register(&app, "fulano").await;

    let ok = json!({"username": "Fulano", "password": "senha-forte-1"});
    let r = call(&app, "POST", "/auth/login", Some(ok), None, None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.refresh_cookie.is_some());

    // Senha errada e usuário inexistente dão a mesma resposta.
    for body in [
        json!({"username": "fulano", "password": "senha-errada"}),
        json!({"username": "ninguem", "password": "senha-forte-1"}),
    ] {
        let r = call(&app, "POST", "/auth/login", Some(body), None, None).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
        assert_eq!(r.body["code"], "invalid_credentials");
        assert!(r.set_cookie.is_none());
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn refresh_rotates_the_token(db: PgPool) {
    let app = app(db);
    let first = register(&app, "fulano").await.refresh_cookie.unwrap();

    let r = call(&app, "POST", "/auth/refresh", None, None, Some(&first)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["user"]["username"], "fulano");
    let second = r.refresh_cookie.unwrap();
    assert_ne!(first, second);

    let me = call(&app, "GET", "/users/@me", None, r.body["access_token"].as_str(), None).await;
    assert_eq!(me.status, StatusCode::OK);

    // Dentro da janela de tolerância, o token velho dá 401 mas não derruba a sessão.
    let stale = call(&app, "POST", "/auth/refresh", None, None, Some(&first)).await;
    assert_eq!(stale.status, StatusCode::UNAUTHORIZED);
    assert!(stale.set_cookie.is_none(), "não pode apagar o cookie novo da outra aba");
    let r = call(&app, "POST", "/auth/refresh", None, None, Some(&second)).await;
    assert_eq!(r.status, StatusCode::OK);
}

#[sqlx::test(migrations = "../migrations")]
async fn reusing_an_old_token_revokes_the_whole_family(db: PgPool) {
    let app = app(db.clone());
    let first = register(&app, "fulano").await.refresh_cookie.unwrap();
    let second =
        call(&app, "POST", "/auth/refresh", None, None, Some(&first)).await.refresh_cookie.unwrap();

    // Simula o token velho voltando bem depois da janela de tolerância (roubo).
    sqlx::query(
        "UPDATE refresh_tokens SET used_at = now() - interval '1 minute' WHERE used_at IS NOT NULL",
    )
    .execute(&db)
    .await
    .unwrap();
    let reuse = call(&app, "POST", "/auth/refresh", None, None, Some(&first)).await;
    assert_eq!(reuse.status, StatusCode::UNAUTHORIZED);

    // O token mais novo, que era válido, também morreu.
    let r = call(&app, "POST", "/auth/refresh", None, None, Some(&second)).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../migrations")]
async fn expired_refresh_token_is_rejected(db: PgPool) {
    let app = app(db.clone());
    let token = register(&app, "fulano").await.refresh_cookie.unwrap();
    sqlx::query("UPDATE refresh_tokens SET expires_at = now() - interval '1 second'")
        .execute(&db)
        .await
        .unwrap();
    let r = call(&app, "POST", "/auth/refresh", None, None, Some(&token)).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../migrations")]
async fn logout_ends_the_session(db: PgPool) {
    let app = app(db);
    let token = register(&app, "fulano").await.refresh_cookie.unwrap();

    let r = call(&app, "POST", "/auth/logout", None, None, Some(&token)).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(r.refresh_cookie.as_deref(), Some(""), "cookie apagado");

    let r = call(&app, "POST", "/auth/refresh", None, None, Some(&token)).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // Logout sem cookie também é 204.
    let r = call(&app, "POST", "/auth/logout", None, None, None).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
}

#[sqlx::test(migrations = "../migrations")]
async fn protected_route_requires_a_valid_token(db: PgPool) {
    let app = app(db);
    for bearer in [None, Some("lixo"), Some("a.b.c")] {
        let r = call(&app, "GET", "/users/@me", None, bearer, None).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
        assert_eq!(r.body["code"], "unauthorized");
    }
}
