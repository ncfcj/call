//! Testes de integração do token de voz.

mod common;

use axum::{Router, http::StatusCode};
use common::{LIVEKIT_KEY, LIVEKIT_SECRET, LIVEKIT_URL, app, call, register};
use livekit_api::access_token::TokenVerifier;
use serde_json::{Value, json};
use sqlx::PgPool;

struct User {
    token: String,
    id: String,
}

async fn user(app: &Router, name: &str) -> User {
    let r = register(app, name).await;
    User {
        token: r.body["access_token"].as_str().unwrap().to_owned(),
        id: r.body["user"]["id"].as_str().unwrap().to_owned(),
    }
}

/// Servidor da ana, com os canais padrão. Devolve (texto, voz).
async fn guild(app: &Router, ana: &User) -> (Value, String, String) {
    let guild =
        call(app, "POST", "/guilds", Some(json!({"name": "Amigos"})), Some(&ana.token), None)
            .await
            .body;
    let uri = format!("/guilds/{}/channels", guild["id"].as_str().unwrap());
    let channels = call(app, "GET", &uri, None, Some(&ana.token), None).await.body;
    let id_of = |kind: &str| {
        channels.as_array().unwrap().iter().find(|c| c["kind"] == kind).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    (guild, id_of("text"), id_of("voice"))
}

fn token_uri(channel: &str) -> String {
    format!("/channels/{channel}/voice/token")
}

#[sqlx::test(migrations = "../migrations")]
async fn member_gets_a_token_for_the_voice_room(db: PgPool) {
    let app = app(db);
    let ana = user(&app, "ana").await;
    let (_, _, voice) = guild(&app, &ana).await;

    let r = call(&app, "POST", &token_uri(&voice), None, Some(&ana.token), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["url"], LIVEKIT_URL);
    assert_eq!(r.body["expires_in"], 300);

    let claims = TokenVerifier::with_api_key(LIVEKIT_KEY, LIVEKIT_SECRET)
        .verify(r.body["token"].as_str().unwrap())
        .expect("token assinado com a chave do LiveKit");
    assert_eq!(claims.sub, ana.id, "identidade = id do usuário");
    assert_eq!(claims.name, "Fulano", "nome de exibição");
    assert_eq!(claims.video.room, voice, "sala = id do canal");
    assert!(claims.video.room_join);
    assert!(claims.video.can_publish() && claims.video.can_subscribe());
    assert!(!claims.video.can_publish_data());
    assert_eq!(claims.video.can_publish_sources, ["microphone"], "só áudio");
    assert!(!claims.video.room_admin && !claims.video.room_create);
    assert_eq!(claims.exp - claims.nbf, 300);
    assert_eq!(claims.room_config.unwrap().max_participants, 10);
}

#[sqlx::test(migrations = "../migrations")]
async fn invite_member_also_gets_a_token(db: PgPool) {
    let app = app(db);
    let ana = user(&app, "ana").await;
    let bia = user(&app, "bia").await;
    let (guild, _, voice) = guild(&app, &ana).await;
    let invite = format!("/invites/{}", guild["invite_code"].as_str().unwrap());
    call(&app, "POST", &invite, None, Some(&bia.token), None).await;

    let r = call(&app, "POST", &token_uri(&voice), None, Some(&bia.token), None).await;
    assert_eq!(r.status, StatusCode::OK);
}

#[sqlx::test(migrations = "../migrations")]
async fn token_is_refused_outside_voice_channels_of_my_guilds(db: PgPool) {
    let app = app(db);
    let ana = user(&app, "ana").await;
    let eve = user(&app, "eve").await;
    let (_, text, voice) = guild(&app, &ana).await;

    let r = call(&app, "POST", &token_uri(&voice), None, Some(&eve.token), None).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "quem não é membro não vê o canal");

    let r = call(&app, "POST", &token_uri(&text), None, Some(&ana.token), None).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "canal de texto não tem chamada");

    let r = call(&app, "POST", &token_uri(&voice), None, None, None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}
