//! Estado global da UI: sinais do Leptos, mais as ações que falam com a API.
//!
//! Os componentes leem os sinais e chamam as ações. Os eventos do WebSocket entram por
//! [`Store::apply`]. A lógica testável (histórico, digitação, backoff) fica no `call-core`.

use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use api_client::{ApiClient, ApiError};
use call_core::{ConnState, MessageLog, Typing, VoiceControls};
use futures::channel::mpsc::UnboundedSender;
use leptos::{prelude::*, task::spawn_local};
use proto::{
    Channel, ChannelId, ChannelKind, ClientEvent, CreateChannelRequest, ErrorCode, Guild, GuildId,
    Member, MessagesQuery, PresenceStatus, ServerEvent, User, UserId,
    message::{MESSAGES_PAGE_DEFAULT, MESSAGES_PAGE_MAX},
};

use crate::{gateway, voice::VoiceState};

pub fn now_ms() -> f64 {
    js_sys::Date::now()
}

#[derive(Clone, Copy)]
pub struct Store {
    pub api: StoredValue<ApiClient, LocalStorage>,
    /// `true` enquanto o app descobre, pelo refresh, se ainda há uma sessão.
    pub booting: RwSignal<bool>,
    pub me: RwSignal<Option<User>>,
    pub guilds: RwSignal<Vec<Guild>>,
    pub channels: RwSignal<HashMap<GuildId, Vec<Channel>>>,
    pub members: RwSignal<HashMap<GuildId, Vec<Member>>>,
    pub online: RwSignal<HashMap<GuildId, HashSet<UserId>>>,
    pub logs: RwSignal<HashMap<ChannelId, MessageLog>>,
    /// Dias até o servidor apagar uma mensagem (vem no `HELLO`).
    pub message_ttl_days: RwSignal<Option<u32>>,
    pub unread: RwSignal<HashSet<ChannelId>>,
    pub typing: RwSignal<Typing>,
    /// Relógio de 1 s, para os avisos de digitação vencerem na tela.
    pub now: RwSignal<f64>,
    pub selected_guild: RwSignal<Option<GuildId>>,
    pub selected_channel: RwSignal<Option<ChannelId>>,
    pub conn: RwSignal<ConnState>,
    /// Aviso para o usuário (erro de uma ação). Some sozinho.
    pub notice: RwSignal<Option<String>>,
    /// Fila de saída do WebSocket aberto, se houver.
    pub ws_tx: StoredValue<Option<UnboundedSender<ClientEvent>>>,
    /// Muda a cada login e logout. Tarefas de uma sessão antiga param quando o veem mudar.
    pub epoch: StoredValue<u64>,
    /// Chamada de voz atual, se houver (ver `voice.rs`).
    pub voice: RwSignal<Option<VoiceState>>,
    pub voice_room: StoredValue<Option<Rc<::voice::VoiceRoom>>, LocalStorage>,
    /// Muda a cada entrada e saída da voz; eventos de uma sala antiga são ignorados.
    pub voice_session: StoredValue<u64>,
    /// Mute e deaf. Valem entre chamadas e ficam guardados no navegador.
    pub voice_controls: RwSignal<VoiceControls>,
    /// Microfone e saída de áudio escolhidos, guardados no navegador.
    pub audio_prefs: RwSignal<crate::voice::AudioPrefs>,
    /// Quem está na voz em cada servidor, vindo do servidor (webhook do LiveKit e avisos dos
    /// clients). É o que mostra quem está num canal para quem não está nele.
    pub voice_states: RwSignal<HashMap<GuildId, HashMap<UserId, proto::VoiceState>>>,
}

impl Store {
    pub fn new(api: ApiClient) -> Self {
        Self {
            api: StoredValue::new_local(api),
            booting: RwSignal::new(true),
            me: RwSignal::new(None),
            guilds: RwSignal::new(Vec::new()),
            channels: RwSignal::new(HashMap::new()),
            members: RwSignal::new(HashMap::new()),
            online: RwSignal::new(HashMap::new()),
            logs: RwSignal::new(HashMap::new()),
            message_ttl_days: RwSignal::new(None),
            unread: RwSignal::new(HashSet::new()),
            typing: RwSignal::new(Typing::default()),
            now: RwSignal::new(now_ms()),
            selected_guild: RwSignal::new(None),
            selected_channel: RwSignal::new(None),
            conn: RwSignal::new(ConnState::Connecting),
            notice: RwSignal::new(None),
            ws_tx: StoredValue::new(None),
            epoch: StoredValue::new(0),
            voice: RwSignal::new(None),
            voice_room: StoredValue::new_local(None),
            voice_session: StoredValue::new(0),
            voice_controls: RwSignal::new(crate::voice::load_controls()),
            audio_prefs: RwSignal::new(crate::voice::load_prefs()),
            voice_states: RwSignal::new(HashMap::new()),
        }
    }

    pub fn api(&self) -> ApiClient {
        self.api.get_value()
    }

    pub fn is_current(&self, epoch: u64) -> bool {
        self.epoch.get_value() == epoch
    }

    // --- Sessão -----------------------------------------------------------------------------

    /// Ao abrir o app: o cookie de refresh ainda vale?
    pub fn boot(self) {
        spawn_local(async move {
            match self.api().refresh().await {
                Ok(user) => self.start_session(user),
                Err(err) if err.is_unauthorized() => {}
                Err(err) => self.notify(format!("API indisponível: {err}")),
            }
            self.booting.set(false);
        });
    }

    pub fn start_session(self, user: User) {
        self.reset();
        self.me.set(Some(user));
        let epoch = self.epoch.get_value();
        self.load_guilds();
        gateway::spawn(self, epoch);
        spawn_local(async move {
            while self.is_current(epoch) {
                gloo_timers::future::TimeoutFuture::new(1_000).await;
                let now = now_ms();
                self.now.set(now);
                self.typing.update(|t| t.prune(now));
            }
        });
    }

    pub fn logout(self) {
        let api = self.api();
        spawn_local(async move {
            let _ = api.logout().await;
        });
        self.reset();
    }

    /// O refresh foi recusado no meio do uso: volta para o login.
    fn session_expired(self) {
        if self.me.get_untracked().is_some() {
            self.reset();
            self.notify("Sua sessão expirou. Entre de novo.");
        }
    }

    /// Zera o estado e encerra as tarefas da sessão anterior (o WebSocket fecha).
    fn reset(self) {
        self.epoch.update_value(|e| *e += 1);
        self.leave_voice();
        self.close_ws();
        self.me.set(None);
        self.guilds.set(Vec::new());
        self.channels.set(HashMap::new());
        self.members.set(HashMap::new());
        self.online.set(HashMap::new());
        self.voice_states.set(HashMap::new());
        self.logs.set(HashMap::new());
        self.unread.set(HashSet::new());
        self.typing.set(Typing::default());
        self.selected_guild.set(None);
        self.selected_channel.set(None);
        self.conn.set(ConnState::Connecting);
    }

    pub fn close_ws(self) {
        if let Some(tx) = self.ws_tx.get_value() {
            tx.close_channel();
        }
        self.ws_tx.set_value(None);
    }

    pub fn notify(self, text: impl Into<String>) {
        let text = text.into();
        self.notice.set(Some(text.clone()));
        spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(6_000).await;
            if self.notice.get_untracked().as_deref() == Some(text.as_str()) {
                self.notice.set(None);
            }
        });
    }

    /// Mostra o erro de uma ação, ou volta para o login se a sessão acabou.
    pub fn report(self, err: &ApiError) {
        if err.is_unauthorized() {
            self.session_expired();
        } else {
            self.notify(err.to_string());
        }
    }

    pub fn send_ws(self, event: ClientEvent) {
        if let Some(tx) = self.ws_tx.get_value() {
            let _ = tx.unbounded_send(event);
        }
    }

    // --- Servidores e canais ----------------------------------------------------------------

    fn load_guilds(self) {
        let epoch = self.epoch.get_value();
        spawn_local(async move {
            match self.api().my_guilds().await {
                Ok(guilds) if self.is_current(epoch) => {
                    let first = guilds.first().map(|g| g.id);
                    self.guilds.set(guilds);
                    for guild in self.guilds.get_untracked() {
                        self.send_ws(ClientEvent::Subscribe { guild_id: guild.id });
                    }
                    if let Some(first) = first {
                        self.select_guild(first);
                    }
                }
                Ok(_) => {}
                Err(err) => self.report(&err),
            }
        });
    }

    /// Conectou (ou reconectou): inscreve em todos os servidores e recupera o que perdeu.
    pub fn on_connected(self) {
        for guild in self.guilds.get_untracked() {
            self.send_ws(ClientEvent::Subscribe { guild_id: guild.id });
        }
        self.online.set(HashMap::new());
        // Diz ao servidor onde está na voz (se ele reiniciou, é assim que fica sabendo).
        self.send_voice_state();
        let channels: Vec<ChannelId> =
            self.logs.with_untracked(|logs| logs.keys().copied().collect());
        for channel in channels {
            self.catch_up(channel);
        }
        // Quem entrou nos servidores enquanto a conexão estava caída.
        let guilds: Vec<GuildId> = self.members.with_untracked(|m| m.keys().copied().collect());
        for guild in guilds {
            self.load_members(guild);
        }
    }

    /// Caiu: sem conexão, não dá para saber quem está online.
    pub fn on_disconnected(self) {
        self.online.set(HashMap::new());
        self.typing.set(Typing::default());
    }

    /// Servidor de um canal já carregado.
    fn guild_of(self, channel: ChannelId) -> Option<GuildId> {
        self.channels.with_untracked(|c| {
            c.iter().find(|(_, list)| list.iter().any(|ch| ch.id == channel)).map(|(g, _)| *g)
        })
    }

    /// Busca, página a página, tudo o que chegou depois da última mensagem carregada.
    fn catch_up(self, channel: ChannelId) {
        let epoch = self.epoch.get_value();
        spawn_local(async move {
            loop {
                let after =
                    self.logs.with_untracked(|l| l.get(&channel).and_then(|log| log.newest_id()));
                let Some(after) = after else { return };
                let q = MessagesQuery {
                    after: Some(after),
                    before: None,
                    limit: Some(MESSAGES_PAGE_MAX),
                };
                match self.api().messages(channel, &q).await {
                    Ok(page) if self.is_current(epoch) => {
                        let full = page.len() == MESSAGES_PAGE_MAX as usize;
                        self.logs.update(|logs| {
                            if let Some(log) = logs.get_mut(&channel) {
                                log.merge(page);
                            }
                        });
                        if !full {
                            return;
                        }
                    }
                    Ok(_) => return,
                    Err(err) => return self.report(&err),
                }
            }
        });
    }

    pub fn select_guild(self, guild: GuildId) {
        self.selected_guild.set(Some(guild));
        let epoch = self.epoch.get_value();
        let loaded = self.channels.with_untracked(|c| c.get(&guild).cloned());
        match loaded {
            Some(channels) => self.select_first_text(&channels),
            None => spawn_local(async move {
                match self.api().channels(guild).await {
                    Ok(channels) if self.is_current(epoch) => {
                        self.channels.update(|c| {
                            c.insert(guild, channels.clone());
                        });
                        if self.selected_guild.get_untracked() == Some(guild) {
                            self.select_first_text(&channels);
                        }
                    }
                    Ok(_) => {}
                    Err(err) => self.report(&err),
                }
            }),
        }
        self.load_members(guild);
    }

    fn select_first_text(self, channels: &[Channel]) {
        let current = self.selected_channel.get_untracked();
        if current.is_some_and(|c| channels.iter().any(|ch| ch.id == c)) {
            return;
        }
        match channels.iter().find(|c| c.kind == ChannelKind::Text) {
            Some(channel) => self.select_channel(channel.id),
            None => self.selected_channel.set(None),
        }
    }

    pub fn load_members(self, guild: GuildId) {
        let epoch = self.epoch.get_value();
        spawn_local(async move {
            match self.api().members(guild).await {
                Ok(members) if self.is_current(epoch) => self.members.update(|m| {
                    m.insert(guild, members);
                }),
                Ok(_) => {}
                Err(err) => self.report(&err),
            }
        });
    }

    pub fn select_channel(self, channel: ChannelId) {
        self.selected_channel.set(Some(channel));
        self.unread.update(|u| {
            u.remove(&channel);
        });
        if self.logs.with_untracked(|l| l.contains_key(&channel)) {
            return;
        }
        let epoch = self.epoch.get_value();
        spawn_local(async move {
            let q = MessagesQuery { limit: Some(MESSAGES_PAGE_DEFAULT), ..Default::default() };
            match self.api().messages(channel, &q).await {
                Ok(page) if self.is_current(epoch) => {
                    let full = page.len() == MESSAGES_PAGE_DEFAULT as usize;
                    self.logs.update(|logs| {
                        // Um MESSAGE_CREATE pode ter chegado durante a busca.
                        logs.entry(channel)
                            .and_modify(|log| {
                                log.merge(page.clone());
                            })
                            .or_insert_with(|| MessageLog::from_latest(page, full));
                    });
                }
                Ok(_) => {}
                Err(err) => self.report(&err),
            }
        });
    }

    /// Carrega a página anterior à mensagem mais antiga. Devolve quando terminar.
    pub async fn load_older(self, channel: ChannelId) {
        let Some(before) =
            self.logs.with_untracked(|l| l.get(&channel).and_then(|log| log.oldest_id()))
        else {
            return;
        };
        let q =
            MessagesQuery { before: Some(before), after: None, limit: Some(MESSAGES_PAGE_DEFAULT) };
        match self.api().messages(channel, &q).await {
            Ok(page) => {
                let full = page.len() == MESSAGES_PAGE_DEFAULT as usize;
                self.logs.update(|logs| {
                    if let Some(log) = logs.get_mut(&channel) {
                        log.merge_older(page, full);
                    }
                });
            }
            Err(err) => self.report(&err),
        }
    }

    /// Entrou num servidor novo (criou ou usou um convite).
    fn add_guild(self, guild: Guild) {
        let id = guild.id;
        self.guilds.update(|g| {
            if !g.iter().any(|x| x.id == id) {
                g.push(guild);
            }
        });
        self.send_ws(ClientEvent::Subscribe { guild_id: id });
        self.select_guild(id);
    }

    pub async fn create_guild(self, name: String) -> bool {
        match self.api().create_guild(&name).await {
            Ok(guild) => {
                self.add_guild(guild);
                true
            }
            Err(err) => {
                self.report(&err);
                false
            }
        }
    }

    pub async fn join_invite(self, code: String) -> bool {
        match self.api().join_invite(&code).await {
            Ok(guild) => {
                self.add_guild(guild);
                true
            }
            Err(ApiError::Api { body, .. }) if body.code == ErrorCode::NotFound => {
                self.notify("Convite não encontrado.");
                false
            }
            Err(err) => {
                self.report(&err);
                false
            }
        }
    }

    pub async fn leave_guild(self, guild: GuildId) {
        match self.api().leave_guild(guild).await {
            Ok(()) => {
                if self.voice.with_untracked(|v| v.as_ref().is_some_and(|v| v.guild == guild)) {
                    self.leave_voice();
                }
                self.guilds.update(|g| g.retain(|x| x.id != guild));
                self.online.update(|o| {
                    o.remove(&guild);
                });
                if self.selected_guild.get_untracked() == Some(guild) {
                    self.selected_channel.set(None);
                    match self.guilds.get_untracked().first() {
                        Some(next) => self.select_guild(next.id),
                        None => self.selected_guild.set(None),
                    }
                }
            }
            Err(err) => self.report(&err),
        }
    }

    pub async fn create_channel(self, guild: GuildId, name: String, kind: ChannelKind) -> bool {
        match self.api().create_channel(guild, &CreateChannelRequest { name, kind }).await {
            Ok(channel) => {
                let id = channel.id;
                self.channels.update(|c| c.entry(guild).or_default().push(channel));
                if kind == ChannelKind::Text {
                    self.select_channel(id);
                }
                true
            }
            Err(err) => {
                self.report(&err);
                false
            }
        }
    }

    // --- Mensagens --------------------------------------------------------------------------

    pub async fn send_message(self, channel: ChannelId, content: String) -> bool {
        match self.api().send_message(channel, &content).await {
            Ok(message) => {
                self.logs.update(|logs| {
                    if let Some(log) = logs.get_mut(&channel) {
                        log.merge([message]);
                    }
                });
                true
            }
            Err(err) => {
                self.report(&err);
                false
            }
        }
    }

    // --- Eventos do WebSocket ---------------------------------------------------------------

    pub fn apply(self, event: ServerEvent) {
        match event {
            ServerEvent::Hello { message_ttl_days, .. } => {
                self.message_ttl_days.set(message_ttl_days);
            }
            ServerEvent::HeartbeatAck => {}
            ServerEvent::Subscribed { guild_id, online, voice } => {
                self.online.update(|o| {
                    o.insert(guild_id, online.into_iter().collect());
                });
                for state in &voice {
                    self.ensure_member(guild_id, state.user_id);
                }
                self.voice_states.update(|v| {
                    v.insert(guild_id, voice.into_iter().map(|s| (s.user_id, s)).collect());
                });
            }
            ServerEvent::VoiceStateUpdate(state) => {
                if state.channel_id.is_some() {
                    self.ensure_member(state.guild_id, state.user_id);
                }
                self.voice_states.update(|v| {
                    let guild = v.entry(state.guild_id).or_default();
                    match state.channel_id {
                        Some(_) => guild.insert(state.user_id, state),
                        None => guild.remove(&state.user_id),
                    };
                });
            }
            ServerEvent::PresenceUpdate { guild_id, user_id, status } => {
                let known = self.members.with_untracked(|m| {
                    m.get(&guild_id).is_some_and(|list| list.iter().any(|x| x.user.id == user_id))
                });
                if !known {
                    // Alguém entrou no servidor depois que a lista de membros foi carregada.
                    self.load_members(guild_id);
                }
                self.online.update(|o| {
                    let set = o.entry(guild_id).or_default();
                    match status {
                        PresenceStatus::Online => set.insert(user_id),
                        PresenceStatus::Offline => set.remove(&user_id),
                    };
                });
            }
            ServerEvent::MessageCreate(message) => {
                let channel = message.channel_id;
                if let Some(author) = &message.author {
                    self.typing.update(|t| t.stop(channel, author.id));
                    // Autor que entrou no servidor sem abrir um WebSocket (sem presença).
                    if let Some(guild) = self.guild_of(channel) {
                        let known = self.members.with_untracked(|m| {
                            m.get(&guild)
                                .is_none_or(|list| list.iter().any(|x| x.user.id == author.id))
                        });
                        if !known {
                            self.load_members(guild);
                        }
                    }
                }
                let mine = message.author.as_ref().map(|a| a.id)
                    == self.me.with_untracked(|m| m.as_ref().map(|u| u.id));
                self.logs.update(|logs| {
                    if let Some(log) = logs.get_mut(&channel) {
                        log.merge([message]);
                    }
                });
                if !mine && self.selected_channel.get_untracked() != Some(channel) {
                    self.unread.update(|u| {
                        u.insert(channel);
                    });
                }
            }
            ServerEvent::TypingStart { channel_id, user_id, .. } => {
                self.typing.update(|t| t.start(channel_id, user_id, now_ms()));
            }
            ServerEvent::Error { message, .. } => {
                leptos::logging::warn!("WS recusou um evento: {message}");
            }
        }
    }

    /// Recarrega os membros se `user` ainda não está na lista (entrou depois que ela foi carregada).
    fn ensure_member(self, guild: GuildId, user: UserId) {
        let known = self.members.with_untracked(|m| {
            m.get(&guild).is_none_or(|list| list.iter().any(|x| x.user.id == user))
        });
        if !known {
            self.load_members(guild);
        }
    }

    /// Nome de exibição de um membro do servidor selecionado.
    pub fn display_name(self, guild: GuildId, user: UserId) -> String {
        self.members.with(|m| {
            m.get(&guild)
                .and_then(|list| list.iter().find(|x| x.user.id == user))
                .map(|x| x.user.display_name.clone())
                .unwrap_or_else(|| "alguém".to_owned())
        })
    }
}
