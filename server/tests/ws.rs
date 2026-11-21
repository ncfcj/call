//! Testes de integração do WebSocket: um servidor de verdade numa porta livre e clients WS reais.

mod common;

use std::{net::SocketAddr, time::Duration};

use axum::{Router, http::StatusCode};
use common::{WEB_ORIGIN, app, call, register};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{self, Message, client::IntoClientRequest, http::HeaderValue},
};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Quanto esperar por um evento que deve chegar, e por silêncio quando nada deve chegar.
const WAIT: Duration = Duration::from_secs(3);
const SILENCE: Duration = Duration::from_millis(300);

struct Server {
    app: Router,
    addr: SocketAddr,
}

/// Sobe o mesmo router numa porta livre. As chamadas REST usam o mesmo estado (o mesmo hub).
async fn serve(db: PgPool) -> Server {
    let app = app(db);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, app.clone()).into_future());
    Server { app, addr }
}

struct User {
    token: String,
    id: String,
}

async fn user(server: &Server, name: &str) -> User {
    let r = register(&server.app, name).await;
    assert_eq!(r.status, StatusCode::CREATED);
    User {
        token: r.body["access_token"].as_str().unwrap().to_owned(),
        id: r.body["user"]["id"].as_str().unwrap().to_owned(),
    }
}

async fn rest(server: &Server, u: &User, method: &str, uri: &str, body: Option<Value>) -> Value {
    let r = call(&server.app, method, uri, body, Some(&u.token), None).await;
    assert!(r.status.is_success(), "{method} {uri}: {} {}", r.status, r.body);
    r.body
}

async fn ticket(server: &Server, u: &User) -> String {
    let body = rest(server, u, "POST", "/ws/ticket", None).await;
    assert_eq!(body["expires_in"], 30);
    body["ticket"].as_str().unwrap().to_owned()
}

async fn open(server: &Server, ticket: &str, origin: &str) -> Result<Socket, tungstenite::Error> {
    let mut req = format!("ws://{}/ws?ticket={ticket}", server.addr).into_client_request().unwrap();
    req.headers_mut().insert("Origin", HeaderValue::from_str(origin).unwrap());
    connect_async(req).await.map(|(socket, _)| socket)
}

/// Status HTTP de um upgrade recusado.
fn rejected_status(result: Result<Socket, tungstenite::Error>) -> u16 {
    match result {
        Err(tungstenite::Error::Http(res)) => res.status().as_u16(),
        Err(other) => panic!("erro inesperado: {other}"),
        Ok(_) => panic!("o upgrade deveria ter sido recusado"),
    }
}

struct Client {
    socket: Socket,
}

impl Client {
    /// Conecta e confere o `HELLO`.
    async fn connect(server: &Server, u: &User) -> Self {
        let socket = open(server, &ticket(server, u).await, WEB_ORIGIN).await.unwrap();
        let mut client = Self { socket };
        let hello = client.next().await;
        assert_eq!(hello["t"], "HELLO");
        assert_eq!(hello["d"]["heartbeat_interval_ms"], 30_000);
        assert_eq!(hello["d"]["user_id"], u.id.as_str());
        client
    }

    async fn send(&mut self, event: Value) {
        self.socket.send(Message::text(event.to_string())).await.unwrap();
    }

    /// Próximo evento de texto, com prazo.
    async fn next(&mut self) -> Value {
        loop {
            let msg = tokio::time::timeout(WAIT, self.socket.next())
                .await
                .expect("nenhum evento chegou no prazo")
                .expect("conexão fechada")
                .unwrap();
            if let Message::Text(text) = msg {
                return serde_json::from_str(&text).unwrap();
            }
        }
    }

    /// Próximo evento do tipo `t`, pulando os outros (ex.: presença).
    async fn next_of(&mut self, t: &str) -> Value {
        loop {
            let event = self.next().await;
            if event["t"] == t {
                return event;
            }
        }
    }

    /// Nenhum evento chega num intervalo curto.
    async fn assert_silent(&mut self) {
        if let Ok(Some(Ok(Message::Text(text)))) =
            tokio::time::timeout(SILENCE, self.socket.next()).await
        {
            panic!("evento inesperado: {text}");
        }
    }

    async fn subscribe(&mut self, guild: &str) -> Value {
        self.send(json!({"t": "SUBSCRIBE", "d": {"guild_id": guild}})).await;
        self.next_of_any(&["SUBSCRIBED", "ERROR"]).await
    }

    async fn next_of_any(&mut self, ts: &[&str]) -> Value {
        loop {
            let event = self.next().await;
            if ts.iter().any(|t| event["t"] == *t) {
                return event;
            }
        }
    }
}

/// Servidor com a ana (dona) e a bia (membro). Devolve o id do servidor e do canal de texto.
async fn guild_with_two_members(server: &Server, ana: &User, bia: &User) -> (String, String) {
    let guild = rest(server, ana, "POST", "/guilds", Some(json!({"name": "Amigos"}))).await;
    let invite = format!("/invites/{}", guild["invite_code"].as_str().unwrap());
    rest(server, bia, "POST", &invite, None).await;
    let g = guild["id"].as_str().unwrap().to_owned();
    let channels = rest(server, ana, "GET", &format!("/guilds/{g}/channels"), None).await;
    let text = channels.as_array().unwrap().iter().find(|c| c["kind"] == "text").unwrap();
    (g, text["id"].as_str().unwrap().to_owned())
}

#[sqlx::test(migrations = "../migrations")]
async fn ticket_is_required_single_use_and_origin_checked(db: PgPool) {
    let server = serve(db).await;
    let ana = user(&server, "ana").await;

    assert_eq!(rejected_status(open(&server, "inventado", WEB_ORIGIN).await), 401);

    let t = ticket(&server, &ana).await;
    assert_eq!(rejected_status(open(&server, &t, "http://site-malicioso.example").await), 403);

    let t = ticket(&server, &ana).await;
    assert!(open(&server, &t, WEB_ORIGIN).await.is_ok());
    assert_eq!(rejected_status(open(&server, &t, WEB_ORIGIN).await), 401, "ticket já usado");

    let r = call(&server.app, "POST", "/ws/ticket", None, None, None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "ticket exige Bearer");
}

#[sqlx::test(migrations = "../migrations")]
async fn heartbeat_is_acknowledged_and_bad_events_do_not_close(db: PgPool) {
    let server = serve(db).await;
    let ana = user(&server, "ana").await;
    let mut ws = Client::connect(&server, &ana).await;

    ws.send(json!({"t": "HEARTBEAT"})).await;
    assert_eq!(ws.next().await["t"], "HEARTBEAT_ACK");

    ws.socket.send(Message::text("isso não é json")).await.unwrap();
    let err = ws.next().await;
    assert_eq!(err["t"], "ERROR");
    assert_eq!(err["d"]["code"], "validation");

    ws.send(json!({"t": "HEARTBEAT"})).await;
    assert_eq!(ws.next().await["t"], "HEARTBEAT_ACK", "a conexão continua viva");
}

#[sqlx::test(migrations = "../migrations")]
async fn messages_reach_members_and_not_outsiders(db: PgPool) {
    let server = serve(db).await;
    let ana = user(&server, "ana").await;
    let bia = user(&server, "bia").await;
    let eve = user(&server, "eve").await;
    let (g, channel) = guild_with_two_members(&server, &ana, &bia).await;

    let mut ws_ana = Client::connect(&server, &ana).await;
    let mut ws_bia = Client::connect(&server, &bia).await;
    let mut ws_eve = Client::connect(&server, &eve).await;
    assert_eq!(ws_ana.subscribe(&g).await["t"], "SUBSCRIBED");
    assert_eq!(ws_bia.subscribe(&g).await["t"], "SUBSCRIBED");

    let denied = ws_eve.subscribe(&g).await;
    assert_eq!(denied["t"], "ERROR");
    assert_eq!(denied["d"]["code"], "not_found");

    let body = json!({"content": "olá, bia!"});
    let sent =
        rest(&server, &ana, "POST", &format!("/channels/{channel}/messages"), Some(body)).await;

    for ws in [&mut ws_bia, &mut ws_ana] {
        let event = ws.next_of("MESSAGE_CREATE").await;
        assert_eq!(event["d"], sent, "o evento é a mesma mensagem que o REST devolveu");
        assert_eq!(event["d"]["author"]["username"], "ana");
    }
    ws_eve.assert_silent().await;
}

#[sqlx::test(migrations = "../migrations")]
async fn typing_reaches_the_others(db: PgPool) {
    let server = serve(db).await;
    let ana = user(&server, "ana").await;
    let bia = user(&server, "bia").await;
    let (g, channel) = guild_with_two_members(&server, &ana, &bia).await;

    let mut ws_ana = Client::connect(&server, &ana).await;
    let mut ws_bia = Client::connect(&server, &bia).await;
    ws_ana.subscribe(&g).await;
    ws_bia.subscribe(&g).await;

    ws_bia.send(json!({"t": "TYPING_START", "d": {"channel_id": channel}})).await;
    let typing = ws_ana.next_of("TYPING_START").await;
    assert_eq!(typing["d"]["user_id"], bia.id.as_str());
    assert_eq!(typing["d"]["channel_id"], channel.as_str());
    assert_eq!(typing["d"]["guild_id"], g.as_str());
    ws_bia.assert_silent().await;
}

#[sqlx::test(migrations = "../migrations")]
async fn presence_goes_online_and_offline(db: PgPool) {
    let server = serve(db).await;
    let ana = user(&server, "ana").await;
    let bia = user(&server, "bia").await;
    let (g, _) = guild_with_two_members(&server, &ana, &bia).await;

    let mut ws_ana = Client::connect(&server, &ana).await;
    let subscribed = ws_ana.subscribe(&g).await;
    assert_eq!(subscribed["d"]["online"], json!([ana.id]));

    let mut ws_bia = Client::connect(&server, &bia).await;
    let subscribed = ws_bia.subscribe(&g).await;
    let online = subscribed["d"]["online"].as_array().unwrap();
    assert_eq!(online.len(), 2);

    let event = ws_ana.next_of("PRESENCE_UPDATE").await;
    assert_eq!(event["d"]["user_id"], bia.id.as_str());
    assert_eq!(event["d"]["status"], "online");

    ws_bia.socket.close(None).await.unwrap();
    let event = ws_ana.next_of("PRESENCE_UPDATE").await;
    assert_eq!(event["d"]["user_id"], bia.id.as_str());
    assert_eq!(event["d"]["status"], "offline");
}

#[sqlx::test(migrations = "../migrations")]
async fn leaving_the_guild_stops_its_events(db: PgPool) {
    let server = serve(db).await;
    let ana = user(&server, "ana").await;
    let bia = user(&server, "bia").await;
    let (g, channel) = guild_with_two_members(&server, &ana, &bia).await;

    let mut ws_ana = Client::connect(&server, &ana).await;
    let mut ws_bia = Client::connect(&server, &bia).await;
    ws_ana.subscribe(&g).await;
    ws_bia.subscribe(&g).await;
    ws_ana.next_of("PRESENCE_UPDATE").await;

    rest(&server, &bia, "DELETE", &format!("/guilds/{g}/members/@me"), None).await;
    let event = ws_ana.next_of("PRESENCE_UPDATE").await;
    assert_eq!(event["d"]["status"], "offline");

    let body = json!({"content": "a bia saiu"});
    rest(&server, &ana, "POST", &format!("/channels/{channel}/messages"), Some(body)).await;
    ws_ana.next_of("MESSAGE_CREATE").await;
    ws_bia.assert_silent().await;
}

#[sqlx::test(migrations = "../migrations")]
async fn messages_survive_a_server_restart(db: PgPool) {
    let server = serve(db.clone()).await;
    let ana = user(&server, "ana").await;
    let bia = user(&server, "bia").await;
    let (_, channel) = guild_with_two_members(&server, &ana, &bia).await;
    let uri = format!("/channels/{channel}/messages");
    rest(&server, &ana, "POST", &uri, Some(json!({"content": "antes do restart"}))).await;
    drop(server);

    // Estado novo (hub, tickets) sobre o mesmo banco, como depois de reiniciar o processo.
    let restarted = serve(db).await;
    let history = rest(&restarted, &bia, "GET", &uri, None).await;
    assert_eq!(history[0]["content"], "antes do restart");
}
