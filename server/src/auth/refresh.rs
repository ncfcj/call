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

/// O que fazer com um refresh token apresentado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// Válido: marca como usado e emite o próximo.
    Rotate,
    /// Vencido, revogado, ou reusado dentro da janela de tolerância (outra aba): só recusa.
    Reject,
    /// Reusado fora da janela: sinal de roubo; revoga a família inteira.
    RevokeFamily,
}

fn classify(
    expires_at: DateTime<Utc>,
    used_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Verdict {
    if revoked_at.is_some() || expires_at <= now {
        return Verdict::Reject;
    }
    match used_at {
        None => Verdict::Rotate,
        Some(used_at) if now - used_at > Duration::seconds(REUSE_GRACE_SECS) => {
            Verdict::RevokeFamily
        }
        Some(_) => Verdict::Reject,
    }
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

    match classify(row.expires_at, row.used_at, row.revoked_at, Utc::now()) {
        Verdict::Rotate => {}
        Verdict::Reject => return Err(AppError::Unauthorized),
        Verdict::RevokeFamily => {
            tracing::warn!(user = %row.user_id, family = %row.family_id, "refresh token reusado: revogando a família");
            revoke_family(&mut tx, row.family_id).await?;
            tx.commit().await?;
            return Err(AppError::Unauthorized);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).unwrap()
    }

    #[test]
    fn token_is_256_random_bits_and_only_its_hash_is_kept() {
        let (token, token_hash) = generate();
        assert_eq!(token.len(), 43, "32 bytes em base64url sem padding");
        assert!(token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        assert_eq!(token_hash, hash(&token));
        assert_eq!(token_hash.len(), 32, "SHA-256");
        let (other, _) = generate();
        assert_ne!(token, other);
    }

    #[test]
    fn hash_is_sha256_of_the_token() {
        // SHA-256 de "abc", vetor de teste do NIST.
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let hex: String = hash("abc").iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, expected);
    }

    #[test]
    fn fresh_token_rotates() {
        assert_eq!(classify(at(100), None, None, at(0)), Verdict::Rotate);
    }

    #[test]
    fn expired_or_revoked_token_is_rejected() {
        assert_eq!(classify(at(0), None, None, at(0)), Verdict::Reject, "vence no instante exato");
        assert_eq!(classify(at(-1), None, None, at(0)), Verdict::Reject);
        assert_eq!(classify(at(100), None, Some(at(-5)), at(0)), Verdict::Reject);
        // Revogado e reusado: só recusa (a família já foi revogada).
        assert_eq!(classify(at(100), Some(at(-60)), Some(at(-5)), at(0)), Verdict::Reject);
    }

    #[test]
    fn reuse_within_the_grace_window_is_only_rejected() {
        assert_eq!(classify(at(100), Some(at(-1)), None, at(0)), Verdict::Reject);
        assert_eq!(
            classify(at(100), Some(at(-10)), None, at(0)),
            Verdict::Reject,
            "10 s ainda é tolerado"
        );
    }

    #[test]
    fn reuse_after_the_grace_window_revokes_the_family() {
        assert_eq!(classify(at(100), Some(at(-11)), None, at(0)), Verdict::RevokeFamily);
        assert_eq!(classify(at(100), Some(at(-3600)), None, at(0)), Verdict::RevokeFamily);
    }
}
