use std::sync::LazyLock;

use anyhow::{Context, anyhow};
use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};

/// Hash de uma senha qualquer, usado quando o usuário não existe. Assim o login demora o mesmo
/// tempo com ou sem usuário, e o tempo de resposta não revela quais usernames existem.
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    Argon2::default()
        .hash_password(b"senha-que-ninguem-usa")
        .expect("hash da senha fictícia")
        .to_string()
});

/// Gera o hash PHC (`$argon2id$...`) com salt aleatório. Roda fora do runtime async,
/// porque o Argon2 ocupa a CPU de propósito.
pub async fn hash(password: String) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|h| h.to_string())
            .map_err(|e| anyhow!("argon2: {e}"))
    })
    .await
    .context("tarefa de hash")?
}

/// Confere a senha. Com `stored = None` (usuário inexistente), compara com o hash fictício
/// e devolve `false`.
pub async fn verify(password: String, stored: Option<String>) -> anyhow::Result<bool> {
    tokio::task::spawn_blocking(move || {
        let exists = stored.is_some();
        let stored = stored.unwrap_or_else(|| DUMMY_HASH.clone());
        let parsed = PasswordHash::new(&stored).map_err(|e| anyhow!("hash inválido: {e}"))?;
        let ok = Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok();
        Ok(exists && ok)
    })
    .await
    .context("tarefa de verificação")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hash_then_verify() {
        let h = hash("correta123".into()).await.unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify("correta123".into(), Some(h.clone())).await.unwrap());
        assert!(!verify("errada123".into(), Some(h)).await.unwrap());
    }

    #[tokio::test]
    async fn missing_user_never_verifies() {
        assert!(!verify("senha-que-ninguem-usa".into(), None).await.unwrap());
    }
}
