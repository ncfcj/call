//! O log das requisições: tem o contexto para investigar e não tem segredos.

mod common;

use std::{
    io::Write,
    sync::{Arc, Mutex},
};

use axum::http::StatusCode;
use call_server::trace;
use common::{app, call, register};
use serde_json::{Value, json};
use sqlx::PgPool;

#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl Write for Log {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn requests_are_logged_with_context_and_without_secrets(db: PgPool) {
    let log = Log::default();
    let writer = log.clone();
    // O `sqlx::test` roda numa thread só, então o subscriber da thread pega tudo.
    let _guard = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .json()
            .with_env_filter("call_server=info,tower_http=info")
            .with_writer(move || writer.clone())
            .finish(),
    );
    let app = app(db).layer(trace::layer());

    let me = register(&app, "ana").await;
    let token = me.body["access_token"].as_str().unwrap();
    let user = me.body["user"]["id"].as_str().unwrap();
    let post = async |uri: &str, body: Value| {
        call(&app, "POST", uri, Some(body), Some(token), None).await.body
    };
    let guild = post("/guilds", json!({"name": "G"})).await;
    let guild_id = guild["id"].as_str().unwrap();
    let uri = format!("/guilds/{guild_id}/channels");
    let channels = call(&app, "GET", &uri, None, Some(token), None).await.body;
    let channel_id = channels[0]["id"].as_str().unwrap();
    post(&format!("/channels/{channel_id}/messages"), json!({"content": "texto-secreto"})).await;
    post("/invites/convite-secreto", Value::Null).await;
    call(&app, "GET", "/ws?ticket=ticket-secreto", None, None, None).await;
    let wrong = json!({"username": "usuario-secreto", "password": "senha-secreta"});
    let login = call(&app, "POST", "/auth/login", Some(wrong), None, None).await;
    assert_eq!(login.status, StatusCode::UNAUTHORIZED);

    let text = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    let secrets = ["secret", "senha-forte-1", token, me.refresh_cookie.as_deref().unwrap()];
    for (i, secret) in secrets.into_iter().enumerate() {
        // Sem o segredo na mensagem: a falha do teste não pode ser ela mesma um vazamento.
        assert!(!text.contains(secret), "o segredo nº {i} apareceu no log");
    }
    let lines: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let sent = lines
        .iter()
        .find(|l| l["span"]["method"] == "POST" && l["span"]["route"] == "/channels/{id}/messages")
        .unwrap_or_else(|| panic!("sem o log da mensagem: {text}"));
    assert_eq!(sent["span"]["user"], user);
    assert_eq!(sent["span"]["guild"], guild_id);
    assert_eq!(sent["span"]["channel"], channel_id);
    assert!(sent["span"]["request_id"].is_string());
    assert_eq!(sent["fields"]["status"], 201);
    assert!(
        lines.iter().any(|l| l["fields"]["message"] == "login recusado"
            && l["fields"]["ip"] == "127.0.0.1"
            && l["level"] == "WARN"),
        "{text}"
    );
}
