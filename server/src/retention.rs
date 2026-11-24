//! Retenção: apaga de vez as mensagens mais velhas que `MESSAGE_TTL_DAYS`.

use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// Cada rodada acorda o compute do Neon por uns 5 min; de hora em hora gastaria a cota à toa.
const INTERVAL: Duration = Duration::from_secs(21_600); // 6 h
/// Mensagens por `DELETE`, para não segurar muitas linhas numa transação só.
const BATCH: i64 = 1000;

/// Roda para sempre: um expurgo na subida e outro a cada [`INTERVAL`].
pub async fn run(db: PgPool, ttl_days: u32) {
    let mut interval = tokio::time::interval(INTERVAL);
    loop {
        interval.tick().await;
        match purge(&db, ttl_days, Utc::now()).await {
            Ok(deleted) => tracing::info!(deleted, ttl_days, "expurgo de mensagens"),
            Err(err) => tracing::error!(error = ?err, "falha no expurgo de mensagens"),
        }
    }
}

/// Apaga as mensagens vencidas em `now`. Devolve quantas apagou.
pub async fn purge(db: &PgPool, ttl_days: u32, now: DateTime<Utc>) -> sqlx::Result<u64> {
    let cutoff = first_id_at(now - TimeDelta::days(ttl_days.into()));
    let mut deleted = 0;
    loop {
        let batch = sqlx::query(
            "DELETE FROM messages
             WHERE id IN (SELECT id FROM messages WHERE id < $1 ORDER BY id LIMIT $2)",
        )
        .bind(cutoff)
        .bind(BATCH)
        .execute(db)
        .await?
        .rows_affected();
        if batch == 0 {
            return Ok(deleted);
        }
        deleted += batch;
    }
}

/// Menor UUIDv7 possível em `at`: os 48 bits mais altos são o tempo em milissegundos.
fn first_id_at(at: DateTime<Utc>) -> Uuid {
    let ms = u64::try_from(at.timestamp_millis()).unwrap_or(0);
    Uuid::from_u128(u128::from(ms) << 80)
}

#[cfg(test)]
mod tests {
    use uuid::{NoContext, Timestamp};

    use super::*;

    #[test]
    fn cutoff_splits_ids_by_their_millisecond() {
        let at = DateTime::from_timestamp_millis(1_800_000_000_123).unwrap();
        let id =
            |ms: u32| Uuid::new_v7(Timestamp::from_unix(NoContext, 1_800_000_000, ms * 1_000_000));
        assert!(id(122) < first_id_at(at));
        assert!(id(123) >= first_id_at(at));
    }

    #[test]
    fn a_ttl_before_1970_deletes_nothing() {
        assert_eq!(first_id_at(DateTime::from_timestamp_millis(-1).unwrap()), Uuid::nil());
    }
}
