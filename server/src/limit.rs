//! Limite de requisições por IP (rotas sem login) e por usuário (rotas com login).
//!
//! shortcut: em memória, some num restart e vale por instância; trocar por Redis se houver 2 servidores.

use std::{
    collections::HashMap,
    hash::Hash,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{HeaderMap, request::Parts},
};
use proto::UserId;

use crate::{
    error::{AppError, AppResult},
    state::AppState,
};

/// A partir deste tamanho, cada checagem limpa antes as chaves que já não limitam nada.
const PURGE_AT: usize = 10_000;

/// `burst` requisições de uma vez; depois, uma a cada `interval` (algoritmo GCRA).
pub struct RateLimit<K> {
    burst: u32,
    interval: Duration,
    /// Quando o saldo de cada chave volta a ficar cheio.
    full_at: Mutex<HashMap<K, Instant>>,
}

impl<K: Eq + Hash> RateLimit<K> {
    fn new(burst: u32, interval: Duration) -> Self {
        Self { burst, interval, full_at: Mutex::default() }
    }

    /// Conta uma requisição de `key`, ou recusa com `429`.
    pub fn check(&self, key: K) -> AppResult<()> {
        self.check_at(key, Instant::now())
    }

    fn check_at(&self, key: K, now: Instant) -> AppResult<()> {
        let mut full_at = self.full_at.lock().unwrap();
        if full_at.len() >= PURGE_AT {
            full_at.retain(|_, at| *at > now);
        }
        let at = full_at.get(&key).map_or(now, |at| now.max(*at)) + self.interval;
        let excess = at.duration_since(now).saturating_sub(self.interval * self.burst);
        if !excess.is_zero() {
            // Arredonda para cima, em segundos inteiros.
            return Err(AppError::RateLimited { retry_after: excess.as_millis().div_ceil(1000) });
        }
        full_at.insert(key, at);
        Ok(())
    }
}

pub struct Limits {
    pub login: RateLimit<IpAddr>,
    pub register: RateLimit<IpAddr>,
    pub refresh: RateLimit<IpAddr>,
    pub ws_ticket: RateLimit<UserId>,
    pub message: RateLimit<UserId>,
    pub voice_token: RateLimit<UserId>,
}

impl Default for Limits {
    fn default() -> Self {
        let secs = Duration::from_secs;
        Self {
            login: RateLimit::new(10, secs(6)),
            register: RateLimit::new(10, secs(30)),
            refresh: RateLimit::new(30, secs(2)),
            ws_ticket: RateLimit::new(30, secs(2)),
            message: RateLimit::new(10, secs(1)),
            voice_token: RateLimit::new(10, secs(6)),
        }
    }
}

/// IP de quem chamou, como chave de limite.
pub struct ClientIp(pub IpAddr);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let ConnectInfo(peer) = ConnectInfo::<SocketAddr>::from_request_parts(parts, state)
            .await
            .map_err(|err| AppError::Internal(err.into()))?;
        Ok(Self(client_ip(peer.ip(), &parts.headers)))
    }
}

/// O `X-Forwarded-For` só vale quando a conexão vem do Caddy, na mesma máquina; de fora, qualquer
/// um o inventaria. Vale o último valor, o único que o Caddy escreveu.
fn client_ip(peer: IpAddr, headers: &HeaderMap) -> IpAddr {
    let forwarded = headers
        .get_all("x-forwarded-for")
        .iter()
        .next_back()
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .and_then(|v| v.trim().parse().ok());
    let ip = match forwarded {
        Some(ip) if peer.to_canonical().is_loopback() => ip,
        _ => peer,
    };
    match ip.to_canonical() {
        // Uma conexão IPv6 doméstica tem um /64 inteiro: trocar de endereço nele não zera o limite.
        IpAddr::V6(v6) if !v6.is_loopback() => {
            let [a, b, c, d, ..] = v6.segments();
            Ipv6Addr::new(a, b, c, d, 0, 0, 0, 0).into()
        }
        ip => ip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: Duration = Duration::from_secs(1);

    fn retry_after(result: AppResult<()>) -> u128 {
        match result {
            Err(AppError::RateLimited { retry_after }) => retry_after,
            other => panic!("esperava RateLimited, veio {other:?}"),
        }
    }

    #[test]
    fn burst_then_one_per_interval() {
        let limit = RateLimit::new(3, SEC * 2);
        let t0 = Instant::now();
        for _ in 0..3 {
            limit.check_at("a", t0).unwrap();
        }
        assert_eq!(retry_after(limit.check_at("a", t0)), 2);
        assert_eq!(retry_after(limit.check_at("a", t0 + SEC / 2)), 2, "1,5 s arredonda para cima");
        limit.check_at("b", t0).unwrap();

        limit.check_at("a", t0 + SEC * 2).unwrap();
        assert_eq!(retry_after(limit.check_at("a", t0 + SEC * 2)), 2, "só repôs uma");
        // Parado, o saldo enche até o burst e não passa dele.
        for _ in 0..3 {
            limit.check_at("a", t0 + SEC * 100).unwrap();
        }
        retry_after(limit.check_at("a", t0 + SEC * 100));
    }

    #[test]
    fn idle_keys_are_forgotten() {
        let limit = RateLimit::new(1, SEC);
        let t0 = Instant::now();
        for key in 0..PURGE_AT - 1 {
            limit.check_at(key, t0).unwrap();
        }
        limit.check_at(PURGE_AT - 1, t0 + SEC).unwrap();
        assert_eq!(limit.full_at.lock().unwrap().len(), PURGE_AT, "abaixo do limite não limpa");
        limit.check_at(PURGE_AT, t0 + SEC).unwrap();
        assert_eq!(limit.full_at.lock().unwrap().len(), 2, "ficam só as que ainda limitam");
    }

    fn ip(peer: &str, forwarded: &[&str]) -> String {
        let mut headers = HeaderMap::new();
        for value in forwarded {
            headers.append("x-forwarded-for", value.parse().unwrap());
        }
        client_ip(peer.parse().unwrap(), &headers).to_string()
    }

    #[test]
    fn forwarded_header_counts_only_from_the_local_proxy() {
        for proxy in ["127.0.0.1", "::1", "::ffff:127.0.0.1"] {
            assert_eq!(ip(proxy, &["203.0.113.7"]), "203.0.113.7", "{proxy}");
        }
        assert_eq!(ip("198.51.100.1", &["203.0.113.7"]), "198.51.100.1");
        assert_eq!(ip("127.0.0.1", &["1.1.1.1, 203.0.113.7"]), "203.0.113.7");
        assert_eq!(ip("127.0.0.1", &["1.1.1.1", "203.0.113.7"]), "203.0.113.7");
        assert_eq!(ip("127.0.0.1", &["lixo"]), "127.0.0.1");
        assert_eq!(ip("::1", &[]), "::1");
    }

    #[test]
    fn ipv6_counts_per_64_prefix() {
        assert_eq!(ip("2001:db8:1:2:3:4:5:6", &[]), "2001:db8:1:2::");
        assert_eq!(ip("::ffff:203.0.113.7", &[]), "203.0.113.7");
    }
}
