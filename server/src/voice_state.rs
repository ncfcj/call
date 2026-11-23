//! Quem está em cada canal de voz, com o mute e o deaf, em memória.
//!
//! Duas fontes alimentam o mapa:
//!
//! - **Webhook do LiveKit** (`participant_joined` e `participant_left`): é a fonte da verdade para
//!   a presença. É ele que tira da lista quem fechou a aba ou perdeu a conexão. Cada entrada vem
//!   com o `sid` da conexão no LiveKit; a saída só vale se for da mesma conexão. Assim, quando a
//!   mesma pessoa entra por uma segunda aba e o LiveKit derruba a primeira, a saída da aba antiga
//!   não a tira da lista.
//! - **Client, pelo WebSocket** (`VOICE_STATE_UPDATE`): o mute e o deaf (o LiveKit não manda
//!   webhook para isso) e o canal em que está. O canal declarado repõe o mapa depois que o
//!   servidor reinicia. O client não tira ninguém de um canal que veio do webhook.
//!
//! Como o hub do WebSocket, só funciona com uma instância do servidor.

use std::{collections::HashMap, sync::Mutex};

use proto::{ChannelId, GuildId, UserId, VoiceState};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VoiceFlags {
    pub muted: bool,
    pub deafened: bool,
}

#[derive(Debug, Clone)]
struct Seat {
    guild: GuildId,
    channel: ChannelId,
    /// `sid` da conexão no LiveKit; `None` quando o lugar veio só do aviso do client.
    sid: Option<String>,
}

#[derive(Default)]
struct Inner {
    seats: HashMap<UserId, Seat>,
    /// Último mute e deaf de cada usuário, mesmo fora de uma chamada.
    flags: HashMap<UserId, VoiceFlags>,
}

impl Inner {
    fn state(&self, user: UserId, guild: GuildId, channel: Option<ChannelId>) -> VoiceState {
        let flags = self.flags.get(&user).copied().unwrap_or_default();
        VoiceState {
            guild_id: guild,
            user_id: user,
            channel_id: channel,
            muted: flags.muted,
            deafened: flags.deafened,
        }
    }

    fn left(&self, user: UserId, seat: &Seat) -> VoiceState {
        self.state(user, seat.guild, None)
    }
}

#[derive(Default)]
pub struct VoiceStates {
    inner: Mutex<Inner>,
}

impl VoiceStates {
    pub fn new() -> Self {
        Self::default()
    }

    /// `user` entrou em `channel`. Devolve os eventos a publicar (no servidor de cada um): a
    /// saída do canal anterior, se mudou de servidor, e a entrada. Nada, se já estava lá.
    pub fn join(
        &self,
        guild: GuildId,
        channel: ChannelId,
        user: UserId,
        sid: Option<String>,
    ) -> Vec<VoiceState> {
        let mut inner = self.inner.lock().unwrap();
        let mut events = Vec::new();
        if let Some(seat) = inner.seats.get_mut(&user)
            && seat.channel == channel
        {
            if sid.is_some() {
                seat.sid = sid;
            }
            return events;
        }
        if let Some(old) = inner.seats.remove(&user)
            && old.guild != guild
        {
            // No mesmo servidor, a entrada no canal novo já diz que saiu do antigo.
            events.push(inner.left(user, &old));
        }
        inner.seats.insert(user, Seat { guild, channel, sid });
        events.push(inner.state(user, guild, Some(channel)));
        events
    }

    /// O LiveKit avisou que a conexão `sid` de `user` saiu de `channel`.
    pub fn leave(&self, channel: ChannelId, user: UserId, sid: &str) -> Option<VoiceState> {
        let mut inner = self.inner.lock().unwrap();
        let seat = inner.seats.get(&user)?;
        let same_connection = seat.sid.as_deref().is_none_or(|s| s == sid);
        if seat.channel != channel || !same_connection {
            return None;
        }
        let seat = inner.seats.remove(&user)?;
        Some(inner.left(user, &seat))
    }

    /// O client disse que não está em chamada nenhuma. Só tira um lugar que veio do próprio
    /// client; o que veio do webhook espera a saída pelo LiveKit (pode ser outra aba).
    pub fn leave_declared(&self, user: UserId) -> Option<VoiceState> {
        let mut inner = self.inner.lock().unwrap();
        if inner.seats.get(&user)?.sid.is_some() {
            return None;
        }
        let seat = inner.seats.remove(&user)?;
        Some(inner.left(user, &seat))
    }

    /// Guarda o mute e o deaf. Devolve o evento, se mudou e a pessoa está numa chamada.
    pub fn set_flags(&self, user: UserId, flags: VoiceFlags) -> Option<VoiceState> {
        let mut inner = self.inner.lock().unwrap();
        if inner.flags.insert(user, flags) == Some(flags) {
            return None;
        }
        let seat = inner.seats.get(&user)?;
        Some(inner.state(user, seat.guild, Some(seat.channel)))
    }

    /// Todos que estão na voz em `guild`.
    pub fn guild(&self, guild: GuildId) -> Vec<VoiceState> {
        let inner = self.inner.lock().unwrap();
        let mut states: Vec<VoiceState> = inner
            .seats
            .iter()
            .filter(|(_, seat)| seat.guild == guild)
            .map(|(user, seat)| inner.state(*user, guild, Some(seat.channel)))
            .collect();
        states.sort_by_key(|s| s.user_id);
        states
    }

    /// A sala acabou (ou o canal foi apagado): todos saem. Devolve os eventos de saída.
    pub fn clear_channel(&self, channel: ChannelId) -> Vec<VoiceState> {
        let mut inner = self.inner.lock().unwrap();
        let users: Vec<UserId> =
            inner.seats.iter().filter(|(_, s)| s.channel == channel).map(|(u, _)| *u).collect();
        users
            .into_iter()
            .filter_map(|user| {
                let seat = inner.seats.remove(&user)?;
                Some(inner.left(user, &seat))
            })
            .collect()
    }

    /// O servidor foi apagado: ninguém mais aparece na voz dele.
    pub fn clear_guild(&self, guild: GuildId) {
        self.inner.lock().unwrap().seats.retain(|_, s| s.guild != guild);
    }

    /// `user` saiu do servidor (`guild`): se estava na voz dele, sai.
    pub fn remove_member(&self, guild: GuildId, user: UserId) -> Option<VoiceState> {
        let mut inner = self.inner.lock().unwrap();
        if inner.seats.get(&user)?.guild != guild {
            return None;
        }
        let seat = inner.seats.remove(&user)?;
        Some(inner.left(user, &seat))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn ids() -> (GuildId, ChannelId, ChannelId, UserId) {
        (
            GuildId(Uuid::from_u128(1)),
            ChannelId(Uuid::from_u128(10)),
            ChannelId(Uuid::from_u128(11)),
            UserId(Uuid::from_u128(100)),
        )
    }

    #[test]
    fn join_and_leave_by_the_same_connection() {
        let (g, c, _, u) = ids();
        let v = VoiceStates::new();
        let events = v.join(g, c, u, Some("PA_1".into()));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].channel_id, Some(c));
        assert!(v.join(g, c, u, Some("PA_1".into())).is_empty(), "entrar de novo não repete");
        assert_eq!(v.guild(g).len(), 1);

        let left = v.leave(c, u, "PA_1").unwrap();
        assert_eq!(left.channel_id, None);
        assert!(v.guild(g).is_empty());
    }

    #[test]
    fn leave_of_an_old_connection_is_ignored() {
        let (g, c, _, u) = ids();
        let v = VoiceStates::new();
        v.join(g, c, u, Some("PA_aba1".into()));
        // Segunda aba entra; o LiveKit derruba a primeira e avisa a saída dela depois.
        v.join(g, c, u, Some("PA_aba2".into()));
        assert!(v.leave(c, u, "PA_aba1").is_none());
        assert_eq!(v.guild(g).len(), 1);
        assert!(v.leave(c, u, "PA_aba2").is_some());
    }

    #[test]
    fn flags_follow_the_user_into_the_call() {
        let (g, c, _, u) = ids();
        let v = VoiceStates::new();
        let deaf = VoiceFlags { muted: false, deafened: true };
        assert!(v.set_flags(u, deaf).is_none(), "fora da chamada não publica");
        let joined = v.join(g, c, u, None);
        assert!(joined[0].deafened);
        assert!(v.set_flags(u, deaf).is_none(), "sem mudança não publica");
        let update = v.set_flags(u, VoiceFlags::default()).unwrap();
        assert!(!update.deafened && update.channel_id == Some(c));
    }

    #[test]
    fn declared_seat_is_removed_by_the_client_but_webhook_seat_is_not() {
        let (g, c, _, u) = ids();
        let v = VoiceStates::new();
        v.join(g, c, u, None);
        assert!(v.leave_declared(u).is_some());

        v.join(g, c, u, Some("PA_1".into()));
        assert!(v.leave_declared(u).is_none(), "pode ser outra aba na chamada");
        assert_eq!(v.guild(g).len(), 1);
    }

    #[test]
    fn moving_between_channels_and_guilds() {
        let (g, c1, c2, u) = ids();
        let g2 = GuildId(Uuid::from_u128(2));
        let v = VoiceStates::new();
        v.join(g, c1, u, None);
        let events = v.join(g, c2, u, None);
        assert_eq!(events.len(), 1, "no mesmo servidor, só a entrada no canal novo");
        assert_eq!(events[0].channel_id, Some(c2));

        let c3 = ChannelId(Uuid::from_u128(30));
        let events = v.join(g2, c3, u, None);
        assert_eq!(events.len(), 2, "saída do servidor antigo e entrada no novo");
        assert_eq!((events[0].guild_id, events[0].channel_id), (g, None));
        assert!(v.guild(g).is_empty());
    }

    #[test]
    fn clearing_a_channel_and_removing_a_member() {
        let (g, c, _, u) = ids();
        let other = UserId(Uuid::from_u128(200));
        let v = VoiceStates::new();
        v.join(g, c, u, None);
        v.join(g, c, other, None);
        assert!(v.remove_member(GuildId(Uuid::from_u128(9)), u).is_none());
        assert!(v.remove_member(g, u).is_some());
        assert_eq!(v.clear_channel(c).len(), 1);
        assert!(v.guild(g).is_empty());
    }

    #[test]
    fn clearing_a_guild_keeps_the_others() {
        let (g, c, _, u) = ids();
        let (g2, c2) = (GuildId(Uuid::from_u128(9)), ChannelId(Uuid::from_u128(10)));
        let other = UserId(Uuid::from_u128(200));
        let v = VoiceStates::new();
        v.join(g, c, u, None);
        v.join(g2, c2, other, None);
        v.clear_guild(g);
        assert!(v.guild(g).is_empty());
        assert_eq!(v.guild(g2).len(), 1);
    }
}
