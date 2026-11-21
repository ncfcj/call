//! Testes de integração de servidores, canais, convites e histórico de mensagens.

mod common;

use axum::{Router, http::StatusCode};
use common::{Reply, app, call, register};
use serde_json::{Value, json};
use sqlx::PgPool;

/// Cadastra um usuário e devolve o access token.
async fn user(app: &Router, username: &str) -> String {
    let r = register(app, username).await;
    assert_eq!(r.status, StatusCode::CREATED);
    r.body["access_token"].as_str().unwrap().to_owned()
}

async fn get(app: &Router, token: &str, uri: &str) -> Reply {
    call(app, "GET", uri, None, Some(token), None).await
}

async fn send(app: &Router, token: &str, method: &str, uri: &str, body: Option<Value>) -> Reply {
    call(app, method, uri, body, Some(token), None).await
}

/// Cria um servidor e devolve o corpo da resposta.
async fn create_guild(app: &Router, token: &str, name: &str) -> Value {
    let r = send(app, token, "POST", "/guilds", Some(json!({"name": name}))).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    r.body
}

fn id(v: &Value) -> &str {
    v["id"].as_str().unwrap()
}

/// O primeiro canal de texto do servidor (o `geral` padrão).
async fn text_channel(app: &Router, token: &str, guild: &Value) -> String {
    let r = get(app, token, &format!("/guilds/{}/channels", id(guild))).await;
    let channels = r.body.as_array().unwrap();
    let text = channels.iter().find(|c| c["kind"] == "text").unwrap();
    id(text).to_owned()
}

#[sqlx::test(migrations = "../migrations")]
async fn new_guild_has_owner_and_default_channels(db: PgPool) {
    let app = app(db);
    let ana = user(&app, "ana").await;
    let guild = create_guild(&app, &ana, "  Amigos ").await;
    assert_eq!(guild["name"], "Amigos");
    assert_eq!(guild["invite_code"].as_str().unwrap().len(), 8);

    let mine = get(&app, &ana, "/users/@me/guilds").await;
    assert_eq!(mine.status, StatusCode::OK);
    assert_eq!(mine.body, json!([guild]));

    let channels = get(&app, &ana, &format!("/guilds/{}/channels", id(&guild))).await.body;
    let summary: Vec<_> = channels
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["name"].clone(), c["kind"].clone()))
        .collect();
    assert_eq!(summary, [(json!("geral"), json!("text")), (json!("Geral"), json!("voice"))]);

    let members = get(&app, &ana, &format!("/guilds/{}/members", id(&guild))).await.body;
    assert_eq!(members.as_array().unwrap().len(), 1);
    assert_eq!(members[0]["user"]["username"], "ana");
    assert_eq!(members[0]["is_owner"], true);
}

#[sqlx::test(migrations = "../migrations")]
async fn invalid_guild_name_is_rejected(db: PgPool) {
    let app = app(db);
    let ana = user(&app, "ana").await;
    for name in ["   ", &"a".repeat(101)] {
        let r = send(&app, &ana, "POST", "/guilds", Some(json!({"name": name}))).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(r.body["code"], "validation");
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn invite_code_joins_the_guild(db: PgPool) {
    let app = app(db);
    let ana = user(&app, "ana").await;
    let bia = user(&app, "bia").await;
    let guild = create_guild(&app, &ana, "Amigos").await;
    let invite = format!("/invites/{}", guild["invite_code"].as_str().unwrap());

    let r = send(&app, &bia, "POST", &invite, None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body, guild);
    // Entrar de novo não duplica nem dá erro.
    assert_eq!(send(&app, &bia, "POST", &invite, None).await.status, StatusCode::OK);

    assert_eq!(get(&app, &bia, "/users/@me/guilds").await.body, json!([guild]));
    let members = get(&app, &bia, &format!("/guilds/{}/members", id(&guild))).await.body;
    let names: Vec<_> =
        members.as_array().unwrap().iter().map(|m| &m["user"]["username"]).collect();
    assert_eq!(names, [&json!("ana"), &json!("bia")]);

    let r = send(&app, &bia, "POST", "/invites/naoexiste", None).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../migrations")]
async fn outsider_cannot_see_the_guild(db: PgPool) {
    let app = app(db);
    let ana = user(&app, "ana").await;
    let eve = user(&app, "eve").await;
    let guild = create_guild(&app, &ana, "Amigos").await;
    let channel = text_channel(&app, &ana, &guild).await;
    let g = id(&guild);

    assert_eq!(get(&app, &eve, "/users/@me/guilds").await.body, json!([]));
    for uri in [
        format!("/guilds/{g}"),
        format!("/guilds/{g}/channels"),
        format!("/guilds/{g}/members"),
        format!("/channels/{channel}/messages"),
    ] {
        let r = get(&app, &eve, &uri).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(r.body["code"], "not_found");
    }
    let r = send(
        &app,
        &eve,
        "POST",
        &format!("/channels/{channel}/messages"),
        Some(json!({"content": "oi"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = send(&app, &eve, "DELETE", &format!("/guilds/{g}"), None).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../migrations")]
async fn only_the_owner_manages_guild_and_channels(db: PgPool) {
    let app = app(db);
    let ana = user(&app, "ana").await;
    let bia = user(&app, "bia").await;
    let guild = create_guild(&app, &ana, "Amigos").await;
    let g = id(&guild).to_owned();
    send(&app, &bia, "POST", &format!("/invites/{}", guild["invite_code"].as_str().unwrap()), None)
        .await;
    let channel = text_channel(&app, &ana, &guild).await;

    let rename = Some(json!({"name": "Novo nome"}));
    let new_channel = Some(json!({"name": "jogos", "kind": "voice"}));
    let attempts = [
        ("PATCH", format!("/guilds/{g}"), rename.clone()),
        ("DELETE", format!("/guilds/{g}"), None),
        ("POST", format!("/guilds/{g}/channels"), new_channel.clone()),
        ("PATCH", format!("/channels/{channel}"), rename.clone()),
        ("DELETE", format!("/channels/{channel}"), None),
    ];
    for (method, uri, body) in attempts {
        let r = send(&app, &bia, method, &uri, body).await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{method} {uri}");
        assert_eq!(r.body["code"], "forbidden");
    }

    let r = send(&app, &ana, "PATCH", &format!("/guilds/{g}"), rename.clone()).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["name"], "Novo nome");

    let r = send(&app, &ana, "POST", &format!("/guilds/{g}/channels"), new_channel).await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(r.body["kind"], "voice");
    assert_eq!(r.body["position"], 2, "vai para o fim da lista");

    let r = send(&app, &ana, "PATCH", &format!("/channels/{channel}"), rename).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["name"], "Novo nome");

    let r = send(&app, &ana, "DELETE", &format!("/channels/{channel}"), None).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let channels = get(&app, &ana, &format!("/guilds/{g}/channels")).await.body;
    assert_eq!(channels.as_array().unwrap().len(), 2);

    let r = send(&app, &ana, "DELETE", &format!("/guilds/{g}"), None).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(get(&app, &ana, &format!("/guilds/{g}")).await.status, StatusCode::NOT_FOUND);
    assert_eq!(get(&app, &bia, "/users/@me/guilds").await.body, json!([]));
}

#[sqlx::test(migrations = "../migrations")]
async fn members_leave_but_the_owner_does_not(db: PgPool) {
    let app = app(db);
    let ana = user(&app, "ana").await;
    let bia = user(&app, "bia").await;
    let guild = create_guild(&app, &ana, "Amigos").await;
    let g = id(&guild);
    send(&app, &bia, "POST", &format!("/invites/{}", guild["invite_code"].as_str().unwrap()), None)
        .await;
    let leave = format!("/guilds/{g}/members/@me");

    let r = send(&app, &ana, "DELETE", &leave, None).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    let r = send(&app, &bia, "DELETE", &leave, None).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(get(&app, &bia, &format!("/guilds/{g}")).await.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../migrations")]
async fn message_history_pages_in_both_directions(db: PgPool) {
    let app = app(db);
    let ana = user(&app, "ana").await;
    let guild = create_guild(&app, &ana, "Amigos").await;
    let channel = text_channel(&app, &ana, &guild).await;
    let uri = format!("/channels/{channel}/messages");

    let mut ids = Vec::new();
    for n in 1..=5 {
        let r = send(&app, &ana, "POST", &uri, Some(json!({"content": format!("msg {n}")}))).await;
        assert_eq!(r.status, StatusCode::CREATED);
        assert_eq!(r.body["author"]["username"], "ana");
        ids.push(id(&r.body).to_owned());
    }
    let contents = |r: &Reply| -> Vec<String> {
        r.body
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["content"].as_str().unwrap().to_owned())
            .collect()
    };

    let all = get(&app, &ana, &uri).await;
    assert_eq!(contents(&all), ["msg 1", "msg 2", "msg 3", "msg 4", "msg 5"]);

    let latest = get(&app, &ana, &format!("{uri}?limit=2")).await;
    assert_eq!(contents(&latest), ["msg 4", "msg 5"]);

    let older = get(&app, &ana, &format!("{uri}?limit=2&before={}", ids[3])).await;
    assert_eq!(contents(&older), ["msg 2", "msg 3"]);

    let missed = get(&app, &ana, &format!("{uri}?limit=2&after={}", ids[1])).await;
    assert_eq!(contents(&missed), ["msg 3", "msg 4"]);

    let none = get(&app, &ana, &format!("{uri}?after={}", ids[4])).await;
    assert_eq!(none.body, json!([]));
}

#[sqlx::test(migrations = "../migrations")]
async fn invalid_message_requests_are_rejected(db: PgPool) {
    let app = app(db);
    let ana = user(&app, "ana").await;
    let guild = create_guild(&app, &ana, "Amigos").await;
    let channel = text_channel(&app, &ana, &guild).await;
    let uri = format!("/channels/{channel}/messages");

    for content in ["   ", &"a".repeat(4001)] {
        let r = send(&app, &ana, "POST", &uri, Some(json!({"content": content}))).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    let some_id = id(&guild);
    for query in ["limit=0", "limit=101", &format!("before={some_id}&after={some_id}")] {
        let r = get(&app, &ana, &format!("{uri}?{query}")).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{query}");
    }

    let channels = get(&app, &ana, &format!("/guilds/{}/channels", id(&guild))).await.body;
    let voice = channels.as_array().unwrap().iter().find(|c| c["kind"] == "voice").unwrap();
    let r = get(&app, &ana, &format!("/channels/{}/messages", id(voice))).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrations = "../migrations")]
async fn messages_outlive_their_author(db: PgPool) {
    let app = app(db.clone());
    let ana = user(&app, "ana").await;
    let bia = user(&app, "bia").await;
    let guild = create_guild(&app, &ana, "Amigos").await;
    send(&app, &bia, "POST", &format!("/invites/{}", guild["invite_code"].as_str().unwrap()), None)
        .await;
    let channel = text_channel(&app, &ana, &guild).await;
    let uri = format!("/channels/{channel}/messages");
    send(&app, &bia, "POST", &uri, Some(json!({"content": "tchau"}))).await;

    sqlx::query("DELETE FROM users WHERE username = 'bia'").execute(&db).await.unwrap();
    let r = get(&app, &ana, &uri).await;
    assert_eq!(r.body[0]["content"], "tchau");
    assert_eq!(r.body[0]["author"], Value::Null);
}
