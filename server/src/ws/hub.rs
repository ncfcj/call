//! Fan-out em memória: conexões, inscrições por servidor (guild) e presença.
//!
//! Cada conexão tem uma fila própria (`mpsc` limitado). Publicar num servidor serializa o evento
//! uma vez e o põe na fila de cada conexão inscrita. Uma conexão lenta demais, com a fila
//! cheia, é desligada: o client reconecta e recarrega o que perdeu pelo histórico.
//!
//! Só funciona com uma instância do servidor. Para escalar horizontalmente, troque este módulo
//! por Redis pub/sub ou NATS mantendo a mesma interface.
//!
//! Presença: um usuário está online num servidor enquanto tiver ao menos uma conexão inscrita
//! nele. O `Mutex` nunca é segurado durante um `await`.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use proto::{GuildId, PresenceStatus, ServerEvent, UserId};
use tokio::sync::mpsc;

/// Eventos que cabem na fila de uma conexão antes de ela ser considerada lenta.
const QUEUE_CAPACITY: usize = 256;

pub type ConnId = u64;

/// Evento já serializado em JSON, compartilhado entre as filas.
pub type Outbound = std::sync::Arc<str>;

struct Conn {
    user: UserId,
    tx: mpsc::Sender<Outbound>,
    guilds: HashSet<GuildId>,
}

#[derive(Default)]
struct Inner {
    conns: HashMap<ConnId, Conn>,
    guilds: HashMap<GuildId, HashSet<ConnId>>,
}

impl Inner {
    /// Quantas conexões de `user` estão inscritas em `guild`.
    fn user_conns_in(&self, guild: GuildId, user: UserId) -> usize {
        self.guilds.get(&guild).map_or(0, |conns| {
            conns.iter().filter(|id| self.conns.get(id).is_some_and(|c| c.user == user)).count()
        })
    }

    /// Tira `conn` de `guild`. Devolve o evento de offline, se era a última conexão do usuário.
    fn leave(&mut self, conn: ConnId, guild: GuildId) -> Option<ServerEvent> {
        let user = self.conns.get_mut(&conn)?.user;
        if let Some(c) = self.conns.get_mut(&conn) {
            c.guilds.remove(&guild);
        }
        let set = self.guilds.get_mut(&guild)?;
        if !set.remove(&conn) {
            return None;
        }
        if set.is_empty() {
            self.guilds.remove(&guild);
        }
        (self.user_conns_in(guild, user) == 0).then_some(ServerEvent::PresenceUpdate {
            guild_id: guild,
            user_id: user,
            status: PresenceStatus::Offline,
        })
    }

    /// Põe o evento na fila de cada conexão inscrita em `guild`, menos `except`.
    /// Devolve as conexões lentas, que precisam ser desligadas.
    fn send(&self, guild: GuildId, event: &ServerEvent, except: Option<ConnId>) -> Vec<ConnId> {
        let Some(conns) = self.guilds.get(&guild) else { return Vec::new() };
        let json: Outbound = match serde_json::to_string(event) {
            Ok(json) => json.into(),
            Err(err) => {
                tracing::error!(?err, "evento WS não serializou");
                return Vec::new();
            }
        };
        let mut slow = Vec::new();
        for id in conns.iter().filter(|id| Some(**id) != except) {
            if let Some(conn) = self.conns.get(id)
                && let Err(mpsc::error::TrySendError::Full(_)) = conn.tx.try_send(json.clone())
            {
                slow.push(*id);
            }
        }
        slow
    }
}

#[derive(Default)]
pub struct Hub {
    inner: Mutex<Inner>,
    next_id: AtomicU64,
}

impl Hub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra uma conexão nova. A fila fecha (o `recv` devolve `None`) quando o hub desliga a
    /// conexão por lentidão.
    pub fn register(&self, user: UserId) -> (ConnId, mpsc::Receiver<Outbound>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
        let conn = Conn { user, tx, guilds: HashSet::new() };
        self.inner.lock().unwrap().conns.insert(id, conn);
        (id, rx)
    }

    /// Remove a conexão de tudo e avisa os servidores em que o usuário ficou offline.
    pub fn unregister(&self, conn: ConnId) {
        let mut inner = self.inner.lock().unwrap();
        let guilds: Vec<GuildId> =
            inner.conns.get(&conn).map(|c| c.guilds.iter().copied().collect()).unwrap_or_default();
        let mut events = Vec::new();
        for guild in guilds {
            if let Some(event) = inner.leave(conn, guild) {
                events.push((guild, event));
            }
        }
        inner.conns.remove(&conn);
        Self::deliver(&mut inner, events);
    }

    /// Inscreve a conexão no servidor. Quem checa se o usuário é membro é quem chama.
    /// Devolve quem está online no servidor, incluindo o próprio usuário.
    pub fn subscribe(&self, conn: ConnId, guild: GuildId) -> Vec<UserId> {
        let mut inner = self.inner.lock().unwrap();
        let Some(user) = inner.conns.get_mut(&conn).map(|c| {
            c.guilds.insert(guild);
            c.user
        }) else {
            return Vec::new();
        };
        let came_online = inner.user_conns_in(guild, user) == 0;
        inner.guilds.entry(guild).or_default().insert(conn);

        if came_online {
            let event = ServerEvent::PresenceUpdate {
                guild_id: guild,
                user_id: user,
                status: PresenceStatus::Online,
            };
            let slow = inner.send(guild, &event, Some(conn));
            Self::drop_slow(&mut inner, slow);
        }

        let mut online: Vec<UserId> = inner.guilds[&guild]
            .iter()
            .filter_map(|id| inner.conns.get(id).map(|c| c.user))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        online.sort();
        online
    }

    /// Envia o evento a todas as conexões inscritas no servidor, menos `except`.
    pub fn publish(&self, guild: GuildId, event: &ServerEvent, except: Option<ConnId>) {
        let mut inner = self.inner.lock().unwrap();
        let slow = inner.send(guild, event, except);
        Self::drop_slow(&mut inner, slow);
    }

    /// O usuário saiu do servidor: as conexões dele param de receber os eventos de lá.
    pub fn remove_member(&self, guild: GuildId, user: UserId) {
        let mut inner = self.inner.lock().unwrap();
        let conns: Vec<ConnId> = inner.guilds.get(&guild).map_or_else(Vec::new, |set| {
            set.iter()
                .copied()
                .filter(|id| inner.conns.get(id).is_some_and(|c| c.user == user))
                .collect()
        });
        let mut events = Vec::new();
        for conn in conns {
            if let Some(event) = inner.leave(conn, guild) {
                events.push((guild, event));
            }
        }
        Self::deliver(&mut inner, events);
    }

    /// O servidor foi apagado: ninguém mais recebe eventos dele.
    pub fn remove_guild(&self, guild: GuildId) {
        let mut inner = self.inner.lock().unwrap();
        for conn in inner.guilds.remove(&guild).unwrap_or_default() {
            if let Some(c) = inner.conns.get_mut(&conn) {
                c.guilds.remove(&guild);
            }
        }
    }

    fn deliver(inner: &mut Inner, events: Vec<(GuildId, ServerEvent)>) {
        let mut slow = Vec::new();
        for (guild, event) in events {
            slow.extend(inner.send(guild, &event, None));
        }
        Self::drop_slow(inner, slow);
    }

    /// Desliga conexões lentas: tirar o `Sender` do mapa fecha a fila, e o loop da conexão
    /// encerra o socket. A presença delas é atualizada para os outros.
    fn drop_slow(inner: &mut Inner, slow: Vec<ConnId>) {
        let mut pending = slow;
        while let Some(conn) = pending.pop() {
            let Some(guilds) = inner.conns.get(&conn).map(|c| c.guilds.clone()) else { continue };
            tracing::warn!(conn, "conexão WS lenta demais; desligando");
            let mut events = Vec::new();
            for guild in guilds {
                if let Some(event) = inner.leave(conn, guild) {
                    events.push((guild, event));
                }
            }
            inner.conns.remove(&conn);
            for (guild, event) in events {
                pending.extend(inner.send(guild, &event, None));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn user(n: u128) -> UserId {
        UserId(Uuid::from_u128(n))
    }

    fn drain(rx: &mut mpsc::Receiver<Outbound>) -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        while let Ok(json) = rx.try_recv() {
            out.push(serde_json::from_str(&json).unwrap());
        }
        out
    }

    #[test]
    fn presence_follows_the_last_connection() {
        let hub = Hub::new();
        let guild = GuildId(Uuid::from_u128(9));
        let (a, mut rx_a) = hub.register(user(1));
        hub.subscribe(a, guild);

        let (b1, mut rx_b1) = hub.register(user(2));
        let (b2, _rx_b2) = hub.register(user(2));
        assert_eq!(hub.subscribe(b1, guild), vec![user(1), user(2)]);
        hub.subscribe(b2, guild);
        let events = drain(&mut rx_a);
        assert_eq!(events.len(), 1, "a segunda aba não repete o online: {events:?}");
        assert_eq!(events[0]["d"]["status"], "online");
        assert!(drain(&mut rx_b1).is_empty(), "quem entra não recebe o próprio online");

        hub.unregister(b1);
        assert!(drain(&mut rx_a).is_empty(), "ainda tem outra aba aberta");
        hub.unregister(b2);
        let events = drain(&mut rx_a);
        assert_eq!(events[0]["d"]["status"], "offline");
    }

    #[test]
    fn slow_connection_is_dropped() {
        let hub = Hub::new();
        let guild = GuildId(Uuid::from_u128(9));
        let (a, mut rx_a) = hub.register(user(1));
        hub.subscribe(a, guild);
        let (b, _rx_b) = hub.register(user(2));
        hub.subscribe(b, guild);
        drain(&mut rx_a);

        // `b` não lê nada: a fila enche e o hub desliga a conexão.
        for _ in 0..=QUEUE_CAPACITY {
            hub.publish(guild, &ServerEvent::HeartbeatAck, Some(a));
        }
        let events = drain(&mut rx_a);
        assert_eq!(events.last().unwrap()["d"]["status"], "offline");
        assert!(!hub.inner.lock().unwrap().conns.contains_key(&b));
    }

    #[test]
    fn publish_skips_the_sender_and_other_guilds() {
        let hub = Hub::new();
        let guild = GuildId(Uuid::from_u128(9));
        let other = GuildId(Uuid::from_u128(8));
        let (a, mut rx_a) = hub.register(user(1));
        let (b, mut rx_b) = hub.register(user(2));
        let (c, mut rx_c) = hub.register(user(3));
        hub.subscribe(a, guild);
        hub.subscribe(b, guild);
        hub.subscribe(c, other);
        drain(&mut rx_a);
        drain(&mut rx_b);
        drain(&mut rx_c);

        hub.publish(guild, &ServerEvent::HeartbeatAck, Some(a));
        assert!(drain(&mut rx_a).is_empty(), "quem enviou não recebe o próprio evento");
        assert_eq!(drain(&mut rx_b).len(), 1);
        assert!(drain(&mut rx_c).is_empty(), "outro servidor não recebe");
    }

    #[test]
    fn removed_member_stops_receiving_and_goes_offline() {
        let hub = Hub::new();
        let guild = GuildId(Uuid::from_u128(9));
        let (a, mut rx_a) = hub.register(user(1));
        let (b, mut rx_b) = hub.register(user(2));
        hub.subscribe(a, guild);
        hub.subscribe(b, guild);
        drain(&mut rx_a);

        hub.remove_member(guild, user(2));
        let events = drain(&mut rx_a);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["d"]["status"], "offline");
        drain(&mut rx_b);
        hub.publish(guild, &ServerEvent::HeartbeatAck, None);
        assert!(drain(&mut rx_b).is_empty());
        assert_eq!(drain(&mut rx_a).len(), 1);
    }

    #[test]
    fn removed_guild_reaches_nobody() {
        let hub = Hub::new();
        let guild = GuildId(Uuid::from_u128(9));
        let (a, mut rx_a) = hub.register(user(1));
        hub.subscribe(a, guild);
        hub.remove_guild(guild);
        hub.publish(guild, &ServerEvent::HeartbeatAck, None);
        assert!(drain(&mut rx_a).is_empty());
        assert_eq!(hub.subscribe(a, guild), vec![user(1)], "pode se inscrever de novo");
    }

    #[test]
    fn unknown_connection_cannot_subscribe() {
        let hub = Hub::new();
        assert!(hub.subscribe(999, GuildId(Uuid::from_u128(9))).is_empty());
        hub.unregister(999);
    }

    #[test]
    fn subscribe_lists_each_online_user_once_in_order() {
        let hub = Hub::new();
        let guild = GuildId(Uuid::from_u128(9));
        let (b1, _r1) = hub.register(user(2));
        let (b2, _r2) = hub.register(user(2));
        let (a, _r3) = hub.register(user(1));
        hub.subscribe(b1, guild);
        hub.subscribe(b2, guild);
        assert_eq!(hub.subscribe(a, guild), vec![user(1), user(2)]);
    }
}
