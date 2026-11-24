//! Limite de requisições, de tamanho do corpo, e formato dos erros que vêm do próprio axum.

mod common;

use axum::{
    Router,
    body::Body,
    http::{Request, Response, StatusCode, header},
};
use common::{app, call, register};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

async fn raw(app: &Router, method: &str, uri: &str, ip: &str, body: String) -> Response<Body> {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("X-Forwarded-For", ip)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap();
    app.clone().oneshot(req).await.unwrap()
}

async fn json_of(res: Response<Body>) -> Value {
    assert_eq!(res.headers()[header::CONTENT_TYPE], "application/json");
    serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

async fn token(app: &Router, name: &str) -> String {
    register(app, name).await.body["access_token"].as_str().unwrap().to_owned()
}

#[sqlx::test(migrations = "../migrations")]
async fn public_routes_are_limited_per_ip(db: PgPool) {
    let app = app(db);
    // O login gasta Argon2 (mais de 6 s no total reporiam o saldo), por isso fica com a folga.
    let cases = [
        ("/auth/register", 10, json!({"username": "", "display_name": "", "password": ""})),
        ("/auth/refresh", 30, Value::Null),
        ("/auth/login", 10, json!({"username": "ninguem", "password": "x"})),
    ];
    for (uri, burst, body) in cases {
        let mut seen = Vec::new();
        // Sobra de 5: o saldo pode repor alguma enquanto o teste roda.
        for _ in 0..burst + 5 {
            seen.push(raw(&app, "POST", uri, "203.0.113.1", body.to_string()).await);
        }
        assert!(
            seen[..burst].iter().all(|r| r.status() != StatusCode::TOO_MANY_REQUESTS),
            "{uri}: o burst passa"
        );
        let refused = seen.pop().unwrap();
        assert_eq!(refused.status(), StatusCode::TOO_MANY_REQUESTS, "{uri}");
        let wait: u64 = refused.headers()[header::RETRY_AFTER].to_str().unwrap().parse().unwrap();
        assert!((1..=30).contains(&wait), "{uri}: {wait}");
        assert_eq!(json_of(refused).await["code"], "rate_limited");

        let other = raw(&app, "POST", uri, "203.0.113.2", body.to_string()).await;
        assert_ne!(other.status(), StatusCode::TOO_MANY_REQUESTS, "{uri}: outro IP não é afetado");
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn logged_in_routes_are_limited_per_user(db: PgPool) {
    let app = app(db);
    let ana = token(&app, "ana").await;
    let bia = token(&app, "bia").await;
    let missing = "/channels/00000000-0000-0000-0000-000000000000";
    let cases = [
        ("/ws/ticket".to_owned(), 30, None),
        (format!("{missing}/messages"), 10, Some(json!({"content": "oi"}))),
        (format!("{missing}/voice/token"), 10, None),
    ];
    for (uri, burst, body) in cases {
        let mut last = StatusCode::OK;
        for i in 0..burst + 5 {
            last = call(&app, "POST", &uri, body.clone(), Some(&ana), None).await.status;
            assert!(i >= burst || last != StatusCode::TOO_MANY_REQUESTS, "{uri}: o burst passa");
        }
        assert_eq!(last, StatusCode::TOO_MANY_REQUESTS, "{uri}");
        let other = call(&app, "POST", &uri, body, Some(&bia), None).await;
        assert_ne!(other.status, StatusCode::TOO_MANY_REQUESTS, "{uri}: outra pessoa passa");
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn framework_errors_use_the_api_format(db: PgPool) {
    let app = app(db);
    let token = token(&app, "ana").await;
    let big = json!({"username": "ana", "password": "x".repeat(64 * 1024)}).to_string();
    let cases = [
        ("POST", "/auth/login", "{", 400, "validation", "requisição inválida"),
        ("POST", "/auth/login", "{}", 422, "validation", "requisição inválida"),
        ("GET", "/auth/login", "", 405, "validation", "requisição inválida"),
        ("GET", "/nada", "", 404, "not_found", "não encontrado"),
        ("POST", "/auth/login", &big, 413, "validation", "requisição grande demais"),
    ];
    for (method, uri, body, status, code, message) in cases {
        let res = raw(&app, method, uri, "203.0.113.1", body.to_owned()).await;
        assert_eq!(res.status(), status, "{method} {uri}");
        assert_eq!(json_of(res).await, json!({"code": code, "message": message}));
    }
    // Na URL e na query (o `call` falha se o corpo não for JSON).
    for uri in [
        "/guilds/nao-e-uuid",
        "/channels/00000000-0000-0000-0000-000000000000/messages?limit=muitas",
    ] {
        let r = call(&app, "GET", uri, None, Some(&token), None).await;
        assert_eq!((r.status, &r.body["code"]), (StatusCode::BAD_REQUEST, &json!("validation")));
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn longest_message_fits_the_body_limit(db: PgPool) {
    let app = app(db);
    let token = token(&app, "ana").await;
    let guild =
        call(&app, "POST", "/guilds", Some(json!({"name": "G1"})), Some(&token), None).await;
    let uri = format!("/guilds/{}/channels", guild.body["id"].as_str().unwrap());
    let channels = call(&app, "GET", &uri, None, Some(&token), None).await;
    let text = channels.body.as_array().unwrap().iter().find(|c| c["kind"] == "text").unwrap();
    let escaped = format!(r#"{{"content":"{}"}}"#, r"😀".repeat(4000));
    let req = Request::post(format!("/channels/{}/messages", text["id"].as_str().unwrap()))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(escaped))
        .unwrap();
    assert_eq!(app.oneshot(req).await.unwrap().status(), StatusCode::CREATED);
}
