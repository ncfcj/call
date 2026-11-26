//! Servidores, canais e chat: o mesmo estado e as mesmas ações do `web/src/store.rs`, com a
//! mesma disposição da `web/src/views/chat.rs`. A voz chega na etapa 4c.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use api_client::{ApiClient, ApiError};
use call_core::{
    ConnState, MessageLog, Typing,
    ui::{initials, starts_group, typing_label},
};
use iced::{
    Border, Element, Font, Length, Task,
    font::Weight,
    futures::channel::mpsc::UnboundedSender,
    widget::{
        Column, button, center, checkbox, column, container, row, scrollable, text, text_input,
        tooltip,
    },
};
use proto::{
    Channel, ChannelId, ChannelKind, ClientEvent, CreateChannelRequest, ErrorCode, Guild, GuildId,
    Member, Message, MessagesQuery, PresenceStatus, ServerEvent, User, UserId,
    message::{MESSAGES_PAGE_DEFAULT, MESSAGES_PAGE_MAX, validate_message_content},
};

use crate::{gateway, theme};

/// O `ApiError` não é `Clone`, e as mensagens do iced precisam ser.
pub type Res<T> = Result<T, Arc<ApiError>>;

/// Qual página do histórico uma busca trouxe.
#[derive(Debug, Clone, Copy)]
pub enum Page {
    /// A primeira, ao abrir o canal.
    Latest,
    /// `before=`: o botão de carregar as antigas.
    Older,
    /// `after=`: o que chegou enquanto a conexão estava caída.
    After,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Gateway(gateway::Event),
    /// A cada segundo: vence os avisos de digitação e de erro.
    Tick,
    Guilds(Res<Vec<Guild>>),
    Channels(GuildId, Res<Vec<Channel>>),
    Members(GuildId, Res<Vec<Member>>),
    Page(ChannelId, Page, Res<Vec<Message>>),
    /// O texto volta para a caixa se o envio falhar.
    Sent(String, Res<Message>),
    Joined(Res<Guild>),
    ChannelCreated(GuildId, ChannelKind, Res<Channel>),
    Left(GuildId, Res<()>),
    SelectGuild(GuildId),
    SelectChannel(ChannelId),
    LoadOlder,
    Draft(String),
    Send,
    Dialog(bool),
    GuildName(String),
    InviteCode(String),
    CreateGuild,
    JoinGuild,
    ChannelName(String),
    ChannelVoice(bool),
    CreateChannel,
    CopyInvite(String),
    LeaveGuild,
    /// Tratado pelo `main`, que é quem guarda a sessão.
    Logout,
}

/// O diálogo de criar ou entrar num servidor e o formulário de canal novo.
#[derive(Default)]
struct Forms {
    adding: bool,
    busy: bool,
    guild_name: String,
    invite: String,
    channel_name: String,
    channel_voice: bool,
}

pub struct Chat {
    me: User,
    api: ApiClient,
    guilds: Vec<Guild>,
    channels: HashMap<GuildId, Vec<Channel>>,
    members: HashMap<GuildId, Vec<Member>>,
    online: HashMap<GuildId, HashSet<UserId>>,
    logs: HashMap<ChannelId, MessageLog>,
    unread: HashSet<ChannelId>,
    typing: Typing,
    selected_guild: Option<GuildId>,
    selected_channel: Option<ChannelId>,
    conn: ConnState,
    ws: Option<UnboundedSender<ClientEvent>>,
    message_ttl_days: Option<u32>,
    notice: Option<(String, Instant)>,
    draft: String,
    last_typing: Option<Instant>,
    loading_older: bool,
    forms: Forms,
    /// Relógio do [`Typing`], em milissegundos desde a abertura.
    started: Instant,
    /// Um refresh foi recusado: o `main` volta para o login.
    pub expired: bool,
}

/// O aviso de digitação vale 8 s nos outros clients; reenvia um pouco antes.
const TYPING_RESEND: Duration = Duration::from_secs(7);
const NOTICE_TTL: Duration = Duration::from_secs(6);

impl Chat {
    pub fn new(api: ApiClient, me: User) -> (Self, Task<Msg>) {
        let chat = Self {
            me,
            api,
            guilds: Vec::new(),
            channels: HashMap::new(),
            members: HashMap::new(),
            online: HashMap::new(),
            logs: HashMap::new(),
            unread: HashSet::new(),
            typing: Typing::default(),
            selected_guild: None,
            selected_channel: None,
            conn: ConnState::Connecting,
            ws: None,
            message_ttl_days: None,
            notice: None,
            draft: String::new(),
            last_typing: None,
            loading_older: false,
            forms: Forms::default(),
            started: Instant::now(),
            expired: false,
        };
        let task = chat.call(|api| async move { api.my_guilds().await }, Msg::Guilds);
        (chat, task)
    }

    fn now_ms(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }

    /// Uma chamada da API em segundo plano, com o erro embrulhado para caber numa mensagem.
    fn call<T, F>(
        &self,
        f: impl FnOnce(ApiClient) -> F,
        map: impl Fn(Res<T>) -> Msg + Send + 'static,
    ) -> Task<Msg>
    where
        F: Future<Output = Result<T, ApiError>> + Send + 'static,
        T: Send + 'static,
    {
        let fut = f(self.api.clone());
        Task::perform(async move { fut.await.map_err(Arc::new) }, map)
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), Instant::now()));
    }

    /// Mostra o erro de uma ação, ou marca a sessão como encerrada.
    fn report(&mut self, err: &ApiError) {
        if err.is_unauthorized() {
            self.expired = true;
        } else {
            self.notify(err.to_string());
        }
    }

    fn send_ws(&self, event: ClientEvent) {
        if let Some(tx) = &self.ws {
            let _ = tx.unbounded_send(event);
        }
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Gateway(event) => return self.on_gateway(event),
            Msg::Tick => {
                self.typing.prune(self.now_ms());
                if self.notice.as_ref().is_some_and(|(_, at)| at.elapsed() >= NOTICE_TTL) {
                    self.notice = None;
                }
            }
            Msg::Guilds(Ok(guilds)) => {
                self.guilds = guilds;
                for guild in &self.guilds {
                    self.send_ws(ClientEvent::Subscribe { guild_id: guild.id });
                }
                if let Some(first) = self.guilds.first() {
                    return self.select_guild(first.id);
                }
            }
            Msg::Channels(guild, Ok(channels)) => {
                self.channels.insert(guild, channels);
                if self.selected_guild == Some(guild) {
                    return self.select_first_text(guild);
                }
            }
            Msg::Members(guild, Ok(members)) => {
                self.members.insert(guild, members);
            }
            Msg::Page(channel, page, Ok(messages)) => return self.on_page(channel, page, messages),
            Msg::Sent(_, Ok(message)) => {
                if let Some(log) = self.logs.get_mut(&message.channel_id) {
                    log.merge([message]);
                }
            }
            Msg::Sent(content, Err(err)) => {
                if self.draft.is_empty() {
                    self.draft = content;
                }
                self.report(&err);
            }
            Msg::Joined(Ok(guild)) => {
                self.forms = Forms::default();
                let id = guild.id;
                if !self.guilds.iter().any(|g| g.id == id) {
                    self.guilds.push(guild);
                }
                self.send_ws(ClientEvent::Subscribe { guild_id: id });
                return self.select_guild(id);
            }
            Msg::Joined(Err(err)) => {
                self.forms.busy = false;
                match &*err {
                    ApiError::Api { body, .. } if body.code == ErrorCode::NotFound => {
                        self.notify("Convite não encontrado.");
                    }
                    other => self.report(other),
                }
            }
            Msg::ChannelCreated(guild, kind, Ok(channel)) => {
                let id = channel.id;
                self.channels.entry(guild).or_default().push(channel);
                self.forms.channel_name.clear();
                if kind == ChannelKind::Text {
                    return self.select_channel(id);
                }
            }
            Msg::Left(guild, Ok(())) => {
                self.guilds.retain(|g| g.id != guild);
                self.online.remove(&guild);
                if self.selected_guild == Some(guild) {
                    self.selected_channel = None;
                    self.selected_guild = None;
                    if let Some(next) = self.guilds.first() {
                        return self.select_guild(next.id);
                    }
                }
            }
            Msg::Page(_, page, Err(err)) => {
                if matches!(page, Page::Older) {
                    self.loading_older = false;
                }
                self.report(&err);
            }
            Msg::Guilds(Err(err))
            | Msg::Channels(_, Err(err))
            | Msg::Members(_, Err(err))
            | Msg::ChannelCreated(_, _, Err(err))
            | Msg::Left(_, Err(err)) => self.report(&err),
            Msg::SelectGuild(guild) => return self.select_guild(guild),
            Msg::SelectChannel(channel) => return self.select_channel(channel),
            Msg::LoadOlder => return self.load_older(),
            Msg::Draft(value) => {
                self.draft = value;
                // Avisa que está digitando no máximo a cada 7 s.
                if let Some(channel) = self.selected_channel
                    && !self.draft.is_empty()
                    && self.last_typing.is_none_or(|at| at.elapsed() > TYPING_RESEND)
                {
                    self.last_typing = Some(Instant::now());
                    self.send_ws(ClientEvent::TypingStart { channel_id: channel });
                }
            }
            Msg::Send => return self.send_message(),
            Msg::Dialog(open) => self.forms.adding = open,
            Msg::GuildName(value) => self.forms.guild_name = value,
            Msg::InviteCode(value) => self.forms.invite = value,
            Msg::CreateGuild => {
                let name = self.forms.guild_name.trim().to_owned();
                if !name.is_empty() && !self.forms.busy {
                    self.forms.busy = true;
                    return self.call(
                        move |api| async move { api.create_guild(&name).await },
                        Msg::Joined,
                    );
                }
            }
            Msg::JoinGuild => {
                let code = self.forms.invite.trim().to_owned();
                if !code.is_empty() && !self.forms.busy {
                    self.forms.busy = true;
                    return self
                        .call(move |api| async move { api.join_invite(&code).await }, Msg::Joined);
                }
            }
            Msg::ChannelName(value) => self.forms.channel_name = value,
            Msg::ChannelVoice(voice) => self.forms.channel_voice = voice,
            Msg::CreateChannel => {
                let name = self.forms.channel_name.trim().to_owned();
                let Some(guild) = self.selected_guild else { return Task::none() };
                if name.is_empty() {
                    return Task::none();
                }
                let kind =
                    if self.forms.channel_voice { ChannelKind::Voice } else { ChannelKind::Text };
                let req = CreateChannelRequest { name, kind };
                return self.call(
                    move |api| async move { api.create_channel(guild, &req).await },
                    move |res| Msg::ChannelCreated(guild, kind, res),
                );
            }
            Msg::CopyInvite(code) => {
                self.notify("Convite copiado.");
                return iced::clipboard::write(code);
            }
            Msg::LeaveGuild => {
                if let Some(guild) = self.selected_guild {
                    return self.call(
                        move |api| async move { api.leave_guild(guild).await },
                        move |res| Msg::Left(guild, res),
                    );
                }
            }
            Msg::Logout => {}
        }
        Task::none()
    }

    // --- WebSocket --------------------------------------------------------------------------

    fn on_gateway(&mut self, event: gateway::Event) -> Task<Msg> {
        match event {
            gateway::Event::Connected(tx) => {
                self.ws = Some(tx);
                self.conn = ConnState::Connected;
                return self.on_connected();
            }
            gateway::Event::Server(event) => return self.apply(event),
            gateway::Event::Disconnected => {
                // Sem conexão, não dá para saber quem está online.
                self.ws = None;
                self.online.clear();
                self.typing = Typing::default();
                self.conn = ConnState::Connecting;
            }
            gateway::Event::Reconnecting { in_secs } => {
                self.conn = ConnState::Reconnecting { in_secs };
            }
            gateway::Event::Unauthorized => self.expired = true,
        }
        Task::none()
    }

    /// Conectou (ou reconectou): inscreve em todos os servidores e recupera o que perdeu.
    fn on_connected(&mut self) -> Task<Msg> {
        for guild in &self.guilds {
            self.send_ws(ClientEvent::Subscribe { guild_id: guild.id });
        }
        self.online.clear();
        let catch_up = self.logs.keys().map(|&channel| self.catch_up(channel));
        // Quem entrou nos servidores enquanto a conexão estava caída.
        let members = self.members.keys().map(|&guild| self.load_members(guild));
        Task::batch(catch_up.chain(members).collect::<Vec<_>>())
    }

    fn apply(&mut self, event: ServerEvent) -> Task<Msg> {
        match event {
            ServerEvent::Hello { message_ttl_days, .. } => self.message_ttl_days = message_ttl_days,
            ServerEvent::Subscribed { guild_id, online, .. } => {
                self.online.insert(guild_id, online.into_iter().collect());
            }
            ServerEvent::PresenceUpdate { guild_id, user_id, status } => {
                let set = self.online.entry(guild_id).or_default();
                match status {
                    PresenceStatus::Online => set.insert(user_id),
                    PresenceStatus::Offline => set.remove(&user_id),
                };
                return self.ensure_member(guild_id, user_id);
            }
            ServerEvent::MessageCreate(message) => {
                let channel = message.channel_id;
                let mut task = Task::none();
                if let Some(author) = &message.author {
                    self.typing.stop(channel, author.id);
                    // Autor que entrou no servidor sem abrir um WebSocket (sem presença).
                    if let Some(guild) = self.guild_of(channel) {
                        task = self.ensure_member(guild, author.id);
                    }
                }
                let mine = message.author.as_ref().is_some_and(|a| a.id == self.me.id);
                if let Some(log) = self.logs.get_mut(&channel) {
                    log.merge([message]);
                }
                if !mine && self.selected_channel != Some(channel) {
                    self.unread.insert(channel);
                }
                return task;
            }
            ServerEvent::TypingStart { channel_id, user_id, .. } => {
                self.typing.start(channel_id, user_id, self.now_ms());
            }
            // A voz chega na etapa 4c; o servidor recusar um evento não muda a tela.
            ServerEvent::VoiceStateUpdate(_)
            | ServerEvent::HeartbeatAck
            | ServerEvent::Error { .. } => {}
        }
        Task::none()
    }

    // --- Servidores e canais ----------------------------------------------------------------

    fn guild_of(&self, channel: ChannelId) -> Option<GuildId> {
        self.channels
            .iter()
            .find(|(_, list)| list.iter().any(|c| c.id == channel))
            .map(|(guild, _)| *guild)
    }

    fn select_guild(&mut self, guild: GuildId) -> Task<Msg> {
        self.selected_guild = Some(guild);
        let channels = if self.channels.contains_key(&guild) {
            self.select_first_text(guild)
        } else {
            self.call(
                move |api| async move { api.channels(guild).await },
                move |res| Msg::Channels(guild, res),
            )
        };
        Task::batch([channels, self.load_members(guild)])
    }

    fn select_first_text(&mut self, guild: GuildId) -> Task<Msg> {
        let channels = self.channels.get(&guild).map(Vec::as_slice).unwrap_or_default();
        if self.selected_channel.is_some_and(|c| channels.iter().any(|ch| ch.id == c)) {
            return Task::none();
        }
        match channels.iter().find(|c| c.kind == ChannelKind::Text).map(|c| c.id) {
            Some(channel) => self.select_channel(channel),
            None => {
                self.selected_channel = None;
                Task::none()
            }
        }
    }

    fn load_members(&self, guild: GuildId) -> Task<Msg> {
        self.call(
            move |api| async move { api.members(guild).await },
            move |res| Msg::Members(guild, res),
        )
    }

    /// Recarrega os membros se `user` ainda não está na lista (entrou depois que ela foi carregada).
    fn ensure_member(&self, guild: GuildId, user: UserId) -> Task<Msg> {
        let known =
            self.members.get(&guild).is_none_or(|list| list.iter().any(|m| m.user.id == user));
        if known { Task::none() } else { self.load_members(guild) }
    }

    fn display_name(&self, guild: GuildId, user: UserId) -> String {
        self.members
            .get(&guild)
            .and_then(|list| list.iter().find(|m| m.user.id == user))
            .map_or_else(|| "alguém".to_owned(), |m| m.user.display_name.clone())
    }

    // --- Mensagens --------------------------------------------------------------------------

    fn fetch(&self, channel: ChannelId, page: Page, q: MessagesQuery) -> Task<Msg> {
        self.call(
            move |api| async move { api.messages(channel, &q).await },
            move |res| Msg::Page(channel, page, res),
        )
    }

    fn select_channel(&mut self, channel: ChannelId) -> Task<Msg> {
        self.selected_channel = Some(channel);
        self.unread.remove(&channel);
        if self.logs.contains_key(&channel) {
            return Task::none();
        }
        let q = MessagesQuery { limit: Some(MESSAGES_PAGE_DEFAULT), ..Default::default() };
        self.fetch(channel, Page::Latest, q)
    }

    fn load_older(&mut self) -> Task<Msg> {
        let Some(channel) = self.selected_channel else { return Task::none() };
        let Some(before) = self.logs.get(&channel).and_then(MessageLog::oldest_id) else {
            return Task::none();
        };
        if self.loading_older {
            return Task::none();
        }
        self.loading_older = true;
        let q =
            MessagesQuery { before: Some(before), limit: Some(MESSAGES_PAGE_DEFAULT), after: None };
        self.fetch(channel, Page::Older, q)
    }

    /// Busca, página a página, tudo o que chegou depois da última mensagem carregada.
    fn catch_up(&self, channel: ChannelId) -> Task<Msg> {
        let Some(after) = self.logs.get(&channel).and_then(MessageLog::newest_id) else {
            return Task::none();
        };
        let q = MessagesQuery { after: Some(after), limit: Some(MESSAGES_PAGE_MAX), before: None };
        self.fetch(channel, Page::After, q)
    }

    fn on_page(&mut self, channel: ChannelId, page: Page, messages: Vec<Message>) -> Task<Msg> {
        let len = messages.len();
        match page {
            Page::Latest => {
                let full = len == MESSAGES_PAGE_DEFAULT as usize;
                // Um MESSAGE_CREATE pode ter chegado durante a busca.
                match self.logs.get_mut(&channel) {
                    Some(log) => {
                        log.merge(messages);
                    }
                    None => {
                        self.logs.insert(channel, MessageLog::from_latest(messages, full));
                    }
                }
            }
            Page::Older => {
                self.loading_older = false;
                if let Some(log) = self.logs.get_mut(&channel) {
                    log.merge_older(messages, len == MESSAGES_PAGE_DEFAULT as usize);
                }
            }
            Page::After => {
                if let Some(log) = self.logs.get_mut(&channel) {
                    log.merge(messages);
                }
                if len == MESSAGES_PAGE_MAX as usize {
                    return self.catch_up(channel);
                }
            }
        }
        Task::none()
    }

    fn send_message(&mut self) -> Task<Msg> {
        let Some(channel) = self.selected_channel else { return Task::none() };
        if let Err(msg) = validate_message_content(&self.draft) {
            if !self.draft.trim().is_empty() {
                self.notify(msg);
            }
            return Task::none();
        }
        let content = std::mem::take(&mut self.draft);
        self.last_typing = None;
        let retry = content.clone();
        self.call(
            move |api| async move { api.send_message(channel, &content).await },
            move |res| Msg::Sent(retry.clone(), res),
        )
    }

    // --- Tela -------------------------------------------------------------------------------

    pub fn view(&self) -> Element<'_, Msg> {
        let content: Element<'_, Msg> = if self.forms.adding {
            center(self.add_guild_dialog()).into()
        } else if self.selected_guild.is_some() {
            row![self.message_pane(), self.member_list()].into()
        } else {
            center(
                column![
                    text("Você ainda não está em nenhum servidor."),
                    button("Criar ou entrar num servidor").on_press(Msg::Dialog(true)),
                ]
                .spacing(16)
                .align_x(iced::Alignment::Center),
            )
            .into()
        };
        let mut main = column![];
        if let Some((notice, _)) = &self.notice {
            let bar = container(text(notice)).padding(8).width(Length::Fill);
            main = main.push(bar.style(|_| panel(theme::DANGER_BG)));
        }
        row![self.guild_bar(), self.channel_sidebar(), main.push(content)]
            .height(Length::Fill)
            .into()
    }

    fn guild_bar(&self) -> Element<'_, Msg> {
        let mut bar = Column::new().spacing(8).padding(12).width(72);
        for guild in &self.guilds {
            let active = self.selected_guild == Some(guild.id);
            let icon = button(text(initials(&guild.name)).center().width(Length::Fill))
                .width(48)
                .height(48)
                .on_press(Msg::SelectGuild(guild.id))
                .style(if active { button::primary } else { button::secondary });
            bar = bar.push(tooltip(icon, text(&guild.name), tooltip::Position::Right));
        }
        let add = button(text("+").center().width(Length::Fill))
            .width(48)
            .height(48)
            .on_press(Msg::Dialog(true))
            .style(button::secondary);
        bar = bar.push(tooltip(add, "Criar ou entrar num servidor", tooltip::Position::Right));
        container(scrollable(bar)).height(Length::Fill).style(|_| panel(theme::SURFACE_DARK)).into()
    }

    fn channel_sidebar(&self) -> Element<'_, Msg> {
        let mut side = Column::new().spacing(6).padding(12);
        let guild = self.selected_guild.and_then(|id| self.guilds.iter().find(|g| g.id == id));
        if let Some(guild) = guild {
            side = side.push(text(&guild.name).size(18).font(bold())).push(
                row![
                    text!("Convite: {}", guild.invite_code).color(theme::MUTED),
                    button(text("copiar").size(12))
                        .padding([2, 6])
                        .on_press(Msg::CopyInvite(guild.invite_code.clone()))
                        .style(button::text),
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            );
            let channels = self.channels.get(&guild.id).map(Vec::as_slice).unwrap_or_default();
            side = side.push(heading("CANAIS DE TEXTO"));
            for channel in channels.iter().filter(|c| c.kind == ChannelKind::Text) {
                let active = self.selected_channel == Some(channel.id);
                let label = if self.unread.contains(&channel.id) {
                    text!("# {}  •", channel.name).font(bold())
                } else {
                    text!("# {}", channel.name)
                };
                side = side.push(
                    button(label)
                        .width(Length::Fill)
                        .on_press(Msg::SelectChannel(channel.id))
                        .style(if active { button::secondary } else { button::text }),
                );
            }
            side = side.push(heading("CANAIS DE VOZ"));
            for channel in channels.iter().filter(|c| c.kind == ChannelKind::Voice) {
                // shortcut: canal de voz só listado; entrar nele é a etapa 4c.
                side = side.push(text!("  {}", channel.name).color(theme::MUTED));
            }
            if guild.owner_id == self.me.id {
                side = side.push(
                    row![
                        text_input("Novo canal", &self.forms.channel_name)
                            .on_input(Msg::ChannelName)
                            .on_submit(Msg::CreateChannel)
                            .padding(6),
                        checkbox(self.forms.channel_voice)
                            .label("voz")
                            .on_toggle(Msg::ChannelVoice),
                        button("+").on_press(Msg::CreateChannel),
                    ]
                    .spacing(6)
                    .align_y(iced::Alignment::Center),
                );
            } else {
                side = side.push(
                    button(text("Sair do servidor").style(text::danger))
                        .on_press(Msg::LeaveGuild)
                        .style(button::text),
                );
            }
        }
        let (status, color) = match self.conn {
            ConnState::Connected => ("Conectado".to_owned(), theme::ONLINE),
            ConnState::Connecting => ("Conectando...".to_owned(), theme::MUTED),
            ConnState::Reconnecting { in_secs } => {
                (format!("Sem conexão. Tentando em {in_secs} s"), theme::DANGER)
            }
        };
        let user = row![
            column![text(&self.me.display_name).font(bold()), text(status).size(12).color(color)]
                .width(Length::Fill),
            button("Sair").on_press(Msg::Logout).style(button::text),
        ]
        .align_y(iced::Alignment::Center);
        column![
            scrollable(side).height(Length::Fill),
            container(user).padding(12).style(|_| panel(theme::SURFACE_DARK)),
        ]
        .width(240)
        .height(Length::Fill)
        .into()
    }

    fn add_guild_dialog(&self) -> Element<'_, Msg> {
        let busy = self.forms.busy;
        let body = column![
            text("Criar um servidor").size(20),
            text_input("Nome do servidor", &self.forms.guild_name)
                .on_input(Msg::GuildName)
                .on_submit(Msg::CreateGuild)
                .padding(10),
            button("Criar").on_press_maybe((!busy).then_some(Msg::CreateGuild)),
            text("Entrar com um convite").size(20),
            text_input("Código, ex.: hCXDYXjN", &self.forms.invite)
                .on_input(Msg::InviteCode)
                .on_submit(Msg::JoinGuild)
                .padding(10),
            button("Entrar").on_press_maybe((!busy).then_some(Msg::JoinGuild)),
            button("Cancelar").on_press(Msg::Dialog(false)).style(button::text),
        ]
        .spacing(12);
        container(body).padding(32).max_width(380).style(|_| card_style()).into()
    }

    fn message_pane(&self) -> Element<'_, Msg> {
        let (Some(guild), Some(channel_id)) = (self.selected_guild, self.selected_channel) else {
            return center(text("Este servidor não tem canal de texto.").color(theme::MUTED))
                .into();
        };
        let name = self
            .channels
            .get(&guild)
            .and_then(|list| list.iter().find(|c| c.id == channel_id))
            .map(|c| c.name.as_str())
            .unwrap_or_default();
        let log = self.logs.get(&channel_id);

        let mut list = Column::new().spacing(2).padding([8, 16]);
        if log.is_some_and(|l| l.has_older) {
            list = list.push(
                button("Carregar mensagens antigas")
                    .on_press_maybe((!self.loading_older).then_some(Msg::LoadOlder))
                    .style(button::text),
            );
        } else if let Some(days) = self.message_ttl_days.filter(|_| log.is_some()) {
            // Só no começo do histórico: antes disso, ainda há o que carregar.
            let unit = if days == 1 { "dia" } else { "dias" };
            list = list.push(
                text!("Mensagens com mais de {days} {unit} são apagadas.")
                    .size(12)
                    .color(theme::MUTED),
            );
        }
        let messages = log.map(MessageLog::messages).unwrap_or_default();
        for (i, message) in messages.iter().enumerate() {
            if starts_group(i.checked_sub(1).map(|p| &messages[p]), message) {
                let author =
                    message.author.as_ref().map_or("conta apagada", |a| a.display_name.as_str());
                let time = message.created_at.with_timezone(&chrono::Local).format("%H:%M");
                list = list.push(
                    row![
                        text(author).font(bold()),
                        text(time.to_string()).size(12).color(theme::MUTED),
                    ]
                    .spacing(8)
                    .padding(iced::Padding { top: 10.0, ..iced::Padding::ZERO })
                    .align_y(iced::Alignment::Center),
                );
            }
            list = list.push(text(&message.content));
        }

        let typing: Vec<String> = self
            .typing
            .active(channel_id, self.now_ms())
            .into_iter()
            .filter(|u| *u != self.me.id)
            .map(|u| self.display_name(guild, u))
            .collect();

        column![
            container(text!("# {name}").size(18).font(bold())).padding([12, 16]),
            // Ancorado embaixo: mensagem nova aparece, e carregar as antigas não pula a leitura.
            scrollable(list).anchor_bottom().height(Length::Fill).width(Length::Fill),
            container(text(typing_label(&typing).unwrap_or_default()).size(12).color(theme::MUTED))
                .padding([0, 16])
                .height(18),
            container(
                text_input("Mensagem (Enter envia)", &self.draft)
                    .on_input(Msg::Draft)
                    .on_submit(Msg::Send)
                    .padding(12),
            )
            .padding(iced::Padding { top: 0.0, right: 16.0, bottom: 16.0, left: 16.0 }),
        ]
        .width(Length::Fill)
        .into()
    }

    fn member_list(&self) -> Element<'_, Msg> {
        let Some(guild) = self.selected_guild else { return column![].into() };
        let online = self.online.get(&guild);
        let (on, off): (Vec<&Member>, Vec<&Member>) = self
            .members
            .get(&guild)
            .map(|list| list.iter().partition(|m| online.is_some_and(|o| o.contains(&m.user.id))))
            .unwrap_or_default();
        let row_of = |member: &Member, is_online: bool| {
            let dot = text("●").color(if is_online { theme::ONLINE } else { theme::MUTED });
            let mut line = row![dot, text(member.user.display_name.clone())].spacing(8);
            if member.is_owner {
                line = line.push(text("dono").size(11).color(theme::MUTED));
            }
            line.align_y(iced::Alignment::Center)
        };
        let mut col = Column::new().spacing(6).padding(12);
        col = col.push(heading(format!("ONLINE — {}", on.len())));
        for member in on {
            col = col.push(row_of(member, true));
        }
        col = col.push(heading(format!("OFFLINE — {}", off.len())));
        for member in off {
            col = col.push(row_of(member, false));
        }
        container(scrollable(col))
            .width(240)
            .height(Length::Fill)
            .style(|_| panel(theme::SURFACE_CARD))
            .into()
    }
}

fn bold() -> Font {
    Font { weight: Weight::Bold, ..Font::DEFAULT }
}

fn heading<'a>(label: impl text::IntoFragment<'a>) -> Element<'a, Msg> {
    container(text(label).size(12).font(bold()).color(theme::MUTED))
        .padding(iced::Padding { top: 12.0, ..iced::Padding::ZERO })
        .into()
}

fn panel(color: iced::Color) -> container::Style {
    container::Style { background: Some(color.into()), ..container::Style::default() }
}

pub fn card_style() -> container::Style {
    container::Style {
        background: Some(theme::SURFACE_CARD.into()),
        border: Border { radius: 12.0.into(), ..Border::default() },
        ..container::Style::default()
    }
}
