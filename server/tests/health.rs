//! `GET /health`: usado pelo monitoramento (Fase 3) e para conferir o banco.

mod common;

use axum::http::StatusCode;
use common::{app, call};
use sqlx::PgPool;

#[sqlx::test(migrations = "../migrations")]
async fn health_reports_the_database(db: PgPool) {
    let app = app(db.clone());
    let r = call(&app, "GET", "/health", None, None, None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["status"], "ok");
    assert_eq!(r.body["database"], true);
    assert_eq!(r.body["version"], env!("CARGO_PKG_VERSION"));

    // Sem banco, continua respondendo, mas avisa.
    db.close().await;
    let r = call(&app, "GET", "/health", None, None, None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["status"], "degraded");
    assert_eq!(r.body["database"], false);
}
