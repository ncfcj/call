use std::collections::HashMap;

use proto::{ChannelId, UserId};

/// Quem está digitando em cada canal. Cada `TYPING_START` vale por [`Typing::TTL_MS`].
///
/// O tempo vem de fora (milissegundos, ex.: `Date.now()`) para o tipo ser testável.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Typing {
    until: HashMap<(ChannelId, UserId), f64>,
}

impl Typing {
    pub const TTL_MS: f64 = 8_000.0;

    pub fn start(&mut self, channel: ChannelId, user: UserId, now_ms: f64) {
        self.until.insert((channel, user), now_ms + Self::TTL_MS);
    }

    /// A mensagem do usuário chegou: ele parou de digitar.
    pub fn stop(&mut self, channel: ChannelId, user: UserId) {
        self.until.remove(&(channel, user));
    }

    /// Quem está digitando no canal agora, em ordem estável.
    pub fn active(&self, channel: ChannelId, now_ms: f64) -> Vec<UserId> {
        let mut users: Vec<UserId> = self
            .until
            .iter()
            .filter(|((c, _), until)| *c == channel && **until > now_ms)
            .map(|((_, u), _)| *u)
            .collect();
        users.sort();
        users
    }

    /// Esquece os avisos vencidos.
    pub fn prune(&mut self, now_ms: f64) {
        self.until.retain(|_, until| *until > now_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn typing_expires_and_stops_on_message() {
        let channel = ChannelId(Uuid::from_u128(1));
        let (ana, bia) = (UserId(Uuid::from_u128(10)), UserId(Uuid::from_u128(11)));
        let mut typing = Typing::default();
        typing.start(channel, ana, 0.0);
        typing.start(channel, bia, 5_000.0);
        assert_eq!(typing.active(channel, 1_000.0), [ana, bia]);
        assert_eq!(typing.active(channel, 9_000.0), [bia], "o da ana venceu");
        assert!(typing.active(ChannelId(Uuid::nil()), 1_000.0).is_empty());

        typing.stop(channel, bia);
        assert!(typing.active(channel, 9_000.0).is_empty());
        typing.prune(9_000.0);
        assert_eq!(typing, Typing::default());
    }

    #[test]
    fn notice_expires_exactly_after_the_ttl() {
        let channel = ChannelId(Uuid::from_u128(1));
        let ana = UserId(Uuid::from_u128(10));
        let mut typing = Typing::default();
        typing.start(channel, ana, 1_000.0);
        assert_eq!(typing.active(channel, 8_999.0), [ana]);
        assert!(typing.active(channel, 9_000.0).is_empty(), "vence no instante exato");
    }

    #[test]
    fn prune_keeps_the_notices_still_valid() {
        let channel = ChannelId(Uuid::from_u128(1));
        let (ana, bia) = (UserId(Uuid::from_u128(10)), UserId(Uuid::from_u128(11)));
        let mut typing = Typing::default();
        typing.start(channel, ana, 0.0);
        typing.start(channel, bia, 4_000.0);
        typing.prune(8_000.0);
        assert_eq!(typing.active(channel, 8_000.0), [bia], "o da ana venceu em 8 s");
        typing.prune(1_000.0);
        assert_eq!(typing.active(channel, 1_000.0), [bia], "prune não apaga o que ainda vale");
    }
}
