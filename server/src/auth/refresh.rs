//! Refresh tokens rotativos com detecção de reuso.
//!
//! O token é um valor aleatório de 256 bits; o banco guarda só o SHA-256 dele, então um
//! vazamento do banco não entrega sessões válidas.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use proto::UserId;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

pub const REFRESH_TTL_DAYS: i64 = 30;

/// Janela em que reapresentar um token recém-usado não é tratado como roubo. Cobre duas abas
/// pedindo refresh ao mesmo tempo com o mesmo cookie: a segunda recebe 401 e tenta de novo
/// com o cookie novo, sem derrubar a sessão.
const REUSE_GRACE_SECS: i64 = 10;

fn hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn generate() -> (String, Vec<u8>) {
    let bytes: [u8; 32] = rand::random();
    let token = URL_SAFE_NO_PAD.encode(bytes);
    let token_hash = hash(&token);
    (token, token_hash)
}

async fn insert(
    tx: &mut Transaction<'_, Postgres>,
    user: UserId,
    family: Uuid,
) -> sqlx::Result<String> {
    let (token, token_hash) = generate();
    sqlx::query(
        "INSERT INTO refresh_tokens (id, user_id, family_id, token_hash, expires_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(user.0)
    .bind(family)
    .bind(token_hash)
    .bind(Utc::now() + Duration::days(REFRESH_TTL_DAYS))
    .execute(&mut **tx)
    .await?;
    Ok(token)
}

/// Abre uma família nova (login ou cadastro) e devolve o primeiro token.
pub async fn start_session(db: &PgPool, user: UserId) -> AppResult<String> {
    let mut tx = db.begin().await?;
    let token = insert(&mut tx, user, Uuid::now_v7()).await?;
    tx.commit().await?;
    Ok(token)
}

#[derive(sqlx::FromRow)]
struct TokenRow {
    id: Uuid,
    user_id: Uuid,
    family_id: Uuid,
    expires_at: DateTime<Utc>,
    used_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
}

/// Troca um refresh token válido por outro da mesma família.
///
/// Reapresentar um token já usado (fora da janela de tolerância) revoga a família inteira:
/// ou o ladrão ou o dono legítimo está com um token velho, e não dá para saber qual.
pub async fn rotate(db: &PgPool, token: &str) -> AppResult<(UserId, String)> {
    let mut tx = db.begin().await?;

    // FOR UPDATE serializa refreshes concorrentes do mesmo token.
    let row: Option<TokenRow> = sqlx::query_as(
        "SELECT id, user_id, family_id, expires_at, used_at, revoked_at
         FROM refresh_tokens WHERE token_hash = $1 FOR UPDATE",
    )
    .bind(hash(token))
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = row else { return Err(AppError::Unauthorized) };
    let now = Utc::now();

    if row.revoked_at.is_some() || row.expires_at <= now {
        return Err(AppError::Unauthorized);
    }

    if let Some(used_at) = row.used_at {
        if now - used_at > Duration::seconds(REUSE_GRACE_SECS) {
            tracing::warn!(user = %row.user_id, family = %row.family_id, "refresh token reusado: revogando a família");
            revoke_family(&mut tx, row.family_id).await?;
            tx.commit().await?;
        }
        return Err(AppError::Unauthorized);
    }

    sqlx::query("UPDATE refresh_tokens SET used_at = now() WHERE id = $1")
        .bind(row.id)
        .execute(&mut *tx)
        .await?;
    let user = UserId(row.user_id);
    let next = insert(&mut tx, user, row.family_id).await?;
    tx.commit().await?;
    Ok((user, next))
}

/// Logout: revoga a família do token apresentado. Token desconhecido não é erro.
pub async fn end_session(db: &PgPool, token: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    let family: Option<Uuid> =
        sqlx::query_scalar("SELECT family_id FROM refresh_tokens WHERE token_hash = $1")
            .bind(hash(token))
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(family) = family {
        revoke_family(&mut tx, family).await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn revoke_family(tx: &mut Transaction<'_, Postgres>, family: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = now() WHERE family_id = $1 AND revoked_at IS NULL",
    )
    .bind(family)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
