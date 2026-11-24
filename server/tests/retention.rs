//! Expurgo das mensagens vencidas, contra o histórico que a API devolve.

mod common;

use std::time::Duration;

use call_server::retention::{purge, run};
use chrono::{TimeDelta, Utc};
use common::{app, call, register};
use serde_json::json;
use sqlx::PgPool;
use uuid::{NoContext, Timestamp, Uuid};

#[sqlx::test(migrations = "../migrations")]
async fn expired_messages_leave_the_history(db: PgPool) {
    let app = app(db.clone());
    let token = register(&app, "ana").await.body["access_token"].as_str().unwrap().to_owned();
    let token = Some(token.as_str());
    let guild = call(&app, "POST", "/guilds", Some(json!({"name": "G1"})), token, None).await;
    let uri = format!("/guilds/{}/channels", guild.body["id"].as_str().unwrap());
    let channels = call(&app, "GET", &uri, None, token, None).await;
    let text = channels.body.as_array().unwrap().iter().find(|c| c["kind"] == "text").unwrap();
    let channel: Uuid = text["id"].as_str().unwrap().parse().unwrap();
    let uri = format!("/channels/{channel}/messages");

    let now = Utc::now();
    let insert = async |age: TimeDelta, content: &str| {
        let secs = u64::try_from((now - age).timestamp()).unwrap();
        sqlx::query("INSERT INTO messages (id, channel_id, content) VALUES ($1, $2, $3)")
            .bind(Uuid::new_v7(Timestamp::from_unix(NoContext, secs, 0)))
            .bind(channel)
            .bind(content)
            .execute(&db)
            .await
            .unwrap();
    };
    insert(TimeDelta::days(30) + TimeDelta::minutes(1), "vencida").await;
    insert(TimeDelta::days(30) - TimeDelta::minutes(1), "quase").await;
    call(&app, "POST", &uri, Some(json!({"content": "nova"})), token, None).await;

    assert_eq!(purge(&db, 30, now).await.unwrap(), 1);
    let history = call(&app, "GET", &uri, None, token, None).await;
    let contents: Vec<_> =
        history.body.as_array().unwrap().iter().map(|m| m["content"].clone()).collect();
    assert_eq!(contents, ["quase", "nova"]);

    // Mais de um lote (os ids baixos são de 1970).
    sqlx::query(
        "INSERT INTO messages (id, channel_id, content)
         SELECT lpad(to_hex(n), 32, '0')::uuid, $1, 'antiga' FROM generate_series(1, 2500) n",
    )
    .bind(channel)
    .execute(&db)
    .await
    .unwrap();
    assert_eq!(purge(&db, 30, now).await.unwrap(), 2500);
    assert_eq!(purge(&db, 30, now).await.unwrap(), 0);

    // A tarefa do servidor expurga assim que sobe.
    insert(TimeDelta::days(31), "vencida").await;
    let task = tokio::spawn(run(db.clone(), 30));
    let count = async || {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM messages").fetch_one(&db).await.unwrap()
    };
    let purged = async {
        while count().await != 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), purged).await.expect("não expurgou");
    task.abort();
}
