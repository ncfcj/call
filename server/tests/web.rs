//! O servidor entregando o client na mesma origem (`WEB_DIR`).

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use call_server::routes;
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;

#[sqlx::test(migrations = "../migrations")]
async fn serves_the_client_without_hiding_the_api(db: PgPool) {
    let dir = std::env::temp_dir().join(format!("call-web-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "pagina").unwrap();
    std::fs::write(dir.join("app.wasm"), "original").unwrap();
    std::fs::write(dir.join("app.wasm.gz"), "comprimido").unwrap();
    let app = routes::serve_web(common::app(db.clone()), dir.to_str().unwrap(), "ws://lk").unwrap();

    let get = async |uri: &str| {
        let req = Request::get(uri).header(header::ACCEPT_ENCODING, "gzip").body(Body::empty());
        let res = app.clone().oneshot(req.unwrap()).await.unwrap();
        let (parts, body) = res.into_parts();
        (parts, String::from_utf8(body.collect().await.unwrap().to_bytes().into()).unwrap())
    };

    let (page, body) = get("/").await;
    assert_eq!((page.status, body.as_str()), (StatusCode::OK, "pagina"));
    assert_eq!(page.headers[header::CACHE_CONTROL], "no-cache");

    assert!(page.headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap().contains("ws://lk"));

    let (wasm, body) = get("/app.wasm").await;
    assert_eq!(body, "comprimido");
    assert_eq!(wasm.headers[header::CONTENT_TYPE], "application/wasm");

    assert_eq!(get("/nada").await.0.status, StatusCode::NOT_FOUND);
    let (health, body) = get("/health").await;
    assert!(body.contains("\"database\":true"));
    assert_eq!(health.headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(health.headers[header::REFERRER_POLICY], "no-referrer");
    assert_eq!(get("/users/@me").await.0.status, StatusCode::UNAUTHORIZED);
    std::fs::remove_dir_all(&dir).unwrap();

    let err = routes::serve_web(common::app(db), dir.to_str().unwrap(), "ws://lk").unwrap_err();
    assert!(err.to_string().contains("WEB_DIR sem index.html"));
}
