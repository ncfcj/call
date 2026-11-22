//! Tickets de conexão do WebSocket: uso único e válidos por 30 s.
//!
//! O navegador não manda `Authorization` no upgrade do WebSocket, e um JWT na URL vazaria
//! para os logs. Por isso o client troca o access token por um ticket curto e de uso único.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use proto::UserId;

pub const TICKET_TTL: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct Tickets {
    inner: Mutex<HashMap<String, (UserId, Instant)>>,
}

impl Tickets {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn issue(&self, user: UserId) -> String {
        self.issue_at(user, Instant::now())
    }

    fn issue_at(&self, user: UserId, now: Instant) -> String {
        let bytes: [u8; 32] = rand::random();
        let ticket = URL_SAFE_NO_PAD.encode(bytes);
        let mut inner = self.inner.lock().unwrap();
        // Limpa os vencidos aqui mesmo: o mapa só cresce com tickets emitidos e não usados.
        inner.retain(|_, (_, expires)| *expires > now);
        inner.insert(ticket.clone(), (user, now + TICKET_TTL));
        ticket
    }

    /// Consome o ticket. Um ticket usado, vencido ou desconhecido devolve `None`.
    pub fn redeem(&self, ticket: &str) -> Option<UserId> {
        self.redeem_at(ticket, Instant::now())
    }

    fn redeem_at(&self, ticket: &str, now: Instant) -> Option<UserId> {
        let (user, expires) = self.inner.lock().unwrap().remove(ticket)?;
        (expires > now).then_some(user)
    }

    #[cfg(test)]
    fn expire_all(&self) {
        let past = Instant::now() - Duration::from_secs(1);
        for (_, expires) in self.inner.lock().unwrap().values_mut() {
            *expires = past;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn ticket_is_single_use() {
        let tickets = Tickets::new();
        let user = UserId(Uuid::from_u128(1));
        let ticket = tickets.issue(user);
        assert_eq!(tickets.redeem(&ticket), Some(user));
        assert_eq!(tickets.redeem(&ticket), None);
        assert_eq!(tickets.redeem("inventado"), None);
    }

    #[test]
    fn expired_ticket_is_rejected() {
        let tickets = Tickets::new();
        let ticket = tickets.issue(UserId(Uuid::from_u128(1)));
        tickets.expire_all();
        assert_eq!(tickets.redeem(&ticket), None);
    }

    #[test]
    fn ticket_expires_exactly_after_the_ttl() {
        let tickets = Tickets::new();
        let user = UserId(Uuid::from_u128(1));
        let t0 = Instant::now();
        let ticket = tickets.issue_at(user, t0);
        assert_eq!(
            tickets.redeem_at(&ticket, t0 + TICKET_TTL - Duration::from_millis(1)),
            Some(user)
        );
        let ticket = tickets.issue_at(user, t0);
        assert_eq!(tickets.redeem_at(&ticket, t0 + TICKET_TTL), None, "vence no instante exato");
    }

    #[test]
    fn issuing_forgets_expired_tickets_but_keeps_valid_ones() {
        let tickets = Tickets::new();
        let user = UserId(Uuid::from_u128(1));
        let t0 = Instant::now();
        let old = tickets.issue_at(user, t0);
        let recent = tickets.issue_at(user, t0 + Duration::from_secs(10));
        // Em t0 + 30 s o primeiro venceu (no instante exato) e o segundo ainda vale.
        tickets.issue_at(user, t0 + TICKET_TTL);
        let kept: Vec<String> = tickets.inner.lock().unwrap().keys().cloned().collect();
        assert!(!kept.contains(&old), "o vencido sai do mapa");
        assert!(kept.contains(&recent), "o que ainda vale fica");
        assert_eq!(kept.len(), 2);
    }
}
