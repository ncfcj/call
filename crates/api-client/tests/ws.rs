//! O WebSocket nativo (app desktop) contra uma API falsa: pede o ticket, abre o socket com ele,
//! lê os eventos tipados e manda os do client.
#![cfg(not(target_arch = "wasm32"))]

use api_client::{ApiClient, ws};
use axum::{
    Json, Router,
    extract::{Query, WebSocketUpgrade, ws::Message},
    response::Response,
    routing::{any, post},
};
use futures::{SinkExt, StreamExt};
use proto::{ClientEvent, ServerEvent, UserId, WsQuery};
use serde_json::json;

/// Manda HELLO, responde HEARTBEAT com HEARTBEAT_ACK e ignora o que não entende.
async fn upgrade(ws: WebSocketUpgrade, Query(q): Query<WsQuery>) -> Response {
    assert_eq!(q.ticket, "t1", "o socket abre com o ticket que a API deu");
    ws.on_upgrade(|mut socket| async move {
        let hello = ServerEvent::Hello {
            heartbeat_interval_ms: 1_000,
            user_id: UserId(uuid::Uuid::nil()),
            message_ttl_days: Some(30),
        };
        let text = serde_json::to_string(&hello).unwrap();
        socket.send(Message::Text(text.into())).await.unwrap();
        socket.send(Message::Text("lixo".into())).await.unwrap();
        while let Some(Ok(Message::Text(text))) = socket.recv().await {
            if serde_json::from_str::<ClientEvent>(&text).unwrap() == ClientEvent::Heartbeat {
                let ack = serde_json::to_string(&ServerEvent::HeartbeatAck).unwrap();
                socket.send(Message::Text(ack.into())).await.unwrap();
            }
        }
    })
}

#[tokio::test]
async fn connects_with_a_ticket_and_exchanges_typed_events() {
    let app = Router::new()
        .route("/ws/ticket", post(|| async { Json(json!({"ticket": "t1", "expires_in": 30})) }))
        .route("/ws", any(upgrade));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let api = ApiClient::new(format!("http://{addr}"));
    let (sink, mut events) = ws::connect(&api).await.unwrap();
    let mut sink = Box::pin(sink);
    assert!(matches!(
        events.next().await,
        Some(ServerEvent::Hello { heartbeat_interval_ms: 1_000, message_ttl_days: Some(30), .. })
    ));
    sink.send(ClientEvent::Heartbeat).await.unwrap();
    assert_eq!(events.next().await, Some(ServerEvent::HeartbeatAck), "pula o que não entende");

    // Fechar o sink encerra a conexão, e o stream termina.
    sink.close().await.unwrap();
    assert_eq!(events.next().await, None);
}
