//! App desktop do Call: a mesma API da web, com UI nativa em iced.

// Sem a janela do console no build de release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod chat;
mod gateway;
mod ptt;
mod session;
mod theme;
mod update;
mod voice;

use api_client::{ApiClient, ApiError};
use chat::Chat;
use iced::{
    Element, Length, Size, Subscription, Task,
    widget::{button, column, container, row, text, text_input},
};
use proto::{LoginRequest, RegisterRequest, User};

/// A API de produção; `CALL_API_URL` troca, por exemplo para `http://localhost:3000`.
const DEFAULT_API_URL: &str = "https://call-70hr.onrender.com";

fn main() -> iced::Result {
    log::set_logger(&Logger).ok();
    log::set_max_level(log::LevelFilter::Info);
    // Sem isso, o rustls entra em pânico ao abrir a primeira conexão TLS do LiveKit.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    iced::application(boot, update, view)
        .title("Call")
        .theme(|_: &State| theme::theme())
        .subscription(subscription)
        .window(iced::window::Settings {
            size: Size::new(1100.0, 720.0),
            icon: iced::window::icon::from_rgba(
                include_bytes!("../assets/icon-64.rgba").to_vec(),
                64,
                64,
            )
            .ok(),
            ..Default::default()
        })
        .run()
}

/// Avisos e erros no console (o build de debug tem um): o que o LiveKit conta ao conectar.
struct Logger;

impl log::Log for Logger {
    fn enabled(&self, meta: &log::Metadata<'_>) -> bool {
        meta.level() <= log::Level::Warn || meta.target().starts_with("call_desktop")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            #[allow(clippy::print_stderr, reason = "é o destino do log")]
            {
                eprintln!("[{}] {}: {}", record.level(), record.target(), record.args());
            }
        }
    }

    fn flush(&self) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Login,
    Register,
}

#[derive(Default)]
struct Form {
    username: String,
    display_name: String,
    password: String,
    invite_code: String,
}

struct Auth {
    mode: Mode,
    form: Form,
    busy: bool,
    error: Option<String>,
}

impl Auth {
    fn new(error: Option<String>) -> Self {
        Self { mode: Mode::Login, form: Form::default(), busy: false, error }
    }
}

enum Screen {
    /// Conferindo se a sessão salva ainda vale. O Render free pode levar um minuto para acordar.
    Booting,
    Auth(Auth),
    Home(Box<Chat>),
}

#[derive(Debug, Clone)]
enum Message {
    /// Fim do refresh da abertura: sem usuário, vai para o login, com o erro se houver.
    Booted(Option<User>, Option<String>),
    Mode(Mode),
    Username(String),
    DisplayName(String),
    Password(String),
    InviteCode(String),
    Submit,
    Authed(Result<User, String>),
    LoggedOut,
    /// Mensagem do chat da sessão `epoch`; as de uma sessão anterior são descartadas.
    Chat(u64, chat::Msg),
    /// Hora de procurar uma versão nova (a cada 6 h, com o app aberto).
    CheckUpdate,
    /// Resultado da procura. `true` na abertura: aí instala sem perguntar.
    UpdateFound(Option<Box<cargo_packager_updater::Update>>, bool),
    Install,
    /// Só chega se a instalação falhar: dando certo, o app fecha para o instalador.
    Installed(Result<(), String>),
}

struct State {
    api: ApiClient,
    screen: Screen,
    /// Conta os logins, para separar as tarefas de uma sessão das da anterior.
    epoch: u64,
    /// O cookie que está no cofre, para só regravar quando o refresh o trocar.
    saved: Option<String>,
    /// Release nova, se houver, e o estado da instalação dela.
    update: Option<Box<cargo_packager_updater::Update>>,
    updating: Option<Result<(), String>>,
}

fn boot() -> (State, Task<Message>) {
    let url = std::env::var("CALL_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_owned());
    let api = ApiClient::new(url);
    let saved = session::load(api.base_url());
    let task = match &saved {
        Some(cookie) => {
            api.restore_refresh_cookie(cookie);
            let api = api.clone();
            Task::perform(
                async move {
                    match api.refresh().await {
                        Ok(user) => (Some(user), None),
                        // Sessão vencida ou revogada: volta para o login, sem alarde.
                        Err(err) if err.is_unauthorized() => (None, None),
                        Err(err) => (None, Some(err.to_string())),
                    }
                },
                |(user, error)| Message::Booted(user, error),
            )
        }
        None => Task::done(Message::Booted(None, None)),
    };
    let check =
        Task::perform(update::check(), |found| Message::UpdateFound(found.map(Box::new), true));
    let state =
        State { api, screen: Screen::Booting, epoch: 0, saved, update: None, updating: None };
    (state, Task::batch([task, check]))
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::Booted(user, error) => {
            sync_session(state);
            return match user {
                Some(user) => start_chat(state, user),
                None => {
                    state.screen = Screen::Auth(Auth::new(error));
                    Task::none()
                }
            };
        }
        Message::Authed(Ok(user)) => {
            sync_session(state);
            return start_chat(state, user);
        }
        Message::Authed(Err(message)) => {
            if let Screen::Auth(auth) = &mut state.screen {
                auth.busy = false;
                auth.error = Some(message);
            }
        }
        Message::Chat(_, chat::Msg::Logout) => {
            // Sai já do cofre e da tela: se o app fechar antes de a API responder, a sessão não
            // volta na próxima abertura.
            session::save(state.api.base_url(), None);
            state.saved = None;
            let hang_up = leave_voice(state);
            state.screen = Screen::Auth(Auth::new(None));
            let api = state.api.clone();
            let logout = Task::perform(async move { api.logout().await }, |_| Message::LoggedOut);
            return Task::batch([hang_up, logout]);
        }
        Message::LoggedOut => sync_session(state),
        Message::CheckUpdate => {
            if state.updating.is_none() {
                return Task::perform(update::check(), |found| {
                    Message::UpdateFound(found.map(Box::new), false)
                });
            }
        }
        Message::UpdateFound(found, at_open) => {
            // Sem resposta (ex.: sem rede), não apaga um aviso que já está na tela.
            if found.is_some() {
                state.update = found;
            }
            // Na abertura instala sozinho, se não estiver numa chamada; depois, só avisa, para
            // não fechar o app no meio de uma conversa.
            let in_call = matches!(&state.screen, Screen::Home(chat) if chat.in_call());
            if at_open && !in_call && state.update.is_some() {
                return Task::done(Message::Install);
            }
        }
        Message::Install => {
            if let Some(found) = state.update.clone() {
                state.updating = Some(Ok(()));
                // Sai da chamada antes: o instalador fecha o app.
                let hang_up = leave_voice(state);
                let install = Task::perform(update::install(*found), Message::Installed);
                return Task::batch([hang_up, install]);
            }
        }
        Message::Installed(result) => state.updating = Some(result),
        Message::Chat(epoch, msg) => {
            let Screen::Home(chat) = &mut state.screen else { return Task::none() };
            if epoch != state.epoch {
                return Task::none();
            }
            let mut task = chat.update(msg).map(move |msg| Message::Chat(epoch, msg));
            if chat.expired {
                task = Task::batch([task, leave_voice(state)]);
                state.screen =
                    Screen::Auth(Auth::new(Some("Sua sessão expirou. Entre de novo.".into())));
            }
            sync_session(state);
            return task;
        }
        other => {
            if let Screen::Auth(auth) = &mut state.screen {
                return edit_auth(&state.api, auth, other);
            }
        }
    }
    Task::none()
}

/// Entrou: abre o chat de uma sessão nova.
fn start_chat(state: &mut State, user: User) -> Task<Message> {
    state.epoch += 1;
    let epoch = state.epoch;
    let (chat, task) = Chat::new(state.api.clone(), user);
    state.screen = Screen::Home(Box::new(chat));
    task.map(move |msg| Message::Chat(epoch, msg))
}

/// Fecha a chamada antes de sair da tela do chat. A resposta chega depois e é descartada.
fn leave_voice(state: &mut State) -> Task<Message> {
    let Screen::Home(chat) = &mut state.screen else { return Task::none() };
    chat.leave_voice().discard()
}

/// O WebSocket, a voz e o relógio do chat, só com a sessão aberta.
fn subscription(state: &State) -> Subscription<Message> {
    let check = iced::time::every(UPDATE_EVERY).map(|_| Message::CheckUpdate);
    let Screen::Home(chat) = &state.screen else { return check };
    let home = Subscription::batch([
        gateway::subscription(&state.api, state.epoch).map(chat::Msg::Gateway),
        chat.voice_subscription(),
        iced::time::every(std::time::Duration::from_secs(1)).map(|_| chat::Msg::Tick),
    ])
    .with(state.epoch)
    .map(|(epoch, msg)| Message::Chat(epoch, msg));
    Subscription::batch([check, home])
}

/// De quanto em quanto tempo, com o app aberto, procura uma versão nova.
const UPDATE_EVERY: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

/// O cookie muda a cada refresh e some no logout; o cofre acompanha.
fn sync_session(state: &mut State) {
    let cookie = state.api.refresh_cookie();
    if cookie != state.saved {
        session::save(state.api.base_url(), cookie.as_deref());
        state.saved = cookie;
    }
}

fn edit_auth(api: &ApiClient, auth: &mut Auth, message: Message) -> Task<Message> {
    let form = &mut auth.form;
    match message {
        Message::Mode(mode) => {
            auth.mode = mode;
            auth.error = None;
        }
        Message::Username(v) => form.username = v,
        Message::DisplayName(v) => form.display_name = v,
        Message::Password(v) => form.password = v,
        Message::InviteCode(v) => form.invite_code = v,
        Message::Submit if !auth.busy => {
            auth.busy = true;
            auth.error = None;
            let api = api.clone();
            let username = form.username.trim().to_owned();
            let password = form.password.clone();
            if auth.mode == Mode::Login {
                let req = LoginRequest { username, password };
                return Task::perform(async move { api.login(&req).await }, authed);
            }
            let invite = form.invite_code.trim();
            let req = RegisterRequest {
                username,
                display_name: form.display_name.trim().to_owned(),
                password,
                invite_code: (!invite.is_empty()).then(|| invite.to_owned()),
            };
            return Task::perform(async move { api.register(&req).await }, authed);
        }
        _ => {}
    }
    Task::none()
}

fn authed(result: Result<User, ApiError>) -> Message {
    Message::Authed(result.map_err(|err| err.to_string()))
}

fn view(state: &State) -> Element<'_, Message> {
    let content = match &state.screen {
        Screen::Booting => container(text("Conectando ao servidor...").color(theme::MUTED))
            .center(Length::Fill)
            .into(),
        Screen::Auth(auth) => container(auth_view(auth)).center(Length::Fill).into(),
        Screen::Home(chat) => {
            let epoch = state.epoch;
            chat.view().map(move |msg| Message::Chat(epoch, msg))
        }
    };
    match &state.update {
        Some(found) => column![update_bar(found, state.updating.as_ref()), content].into(),
        None => content,
    }
}

/// Faixa no topo quando há uma versão nova.
fn update_bar<'a>(
    found: &'a cargo_packager_updater::Update,
    updating: Option<&'a Result<(), String>>,
) -> Element<'a, Message> {
    let action: Element<'a, Message> = match updating {
        None => button("Atualizar agora").on_press(Message::Install).into(),
        Some(Ok(())) => text("Baixando e instalando; o Call abre de novo sozinho...").into(),
        Some(Err(err)) => row![
            text!("A atualização falhou: {err}").color(theme::DANGER),
            button("Tentar de novo").on_press(Message::Install),
        ]
        .spacing(8)
        .into(),
    };
    container(
        row![
            text!("A versão {} do Call está disponível.", found.version).width(Length::Fill),
            action
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    )
    .padding([6, 12])
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(theme::SURFACE_CARD.into()),
        ..container::Style::default()
    })
    .into()
}

fn auth_view(auth: &Auth) -> Element<'_, Message> {
    let tab = |label, mode: Mode| {
        let style = if auth.mode == mode { button::primary } else { button::text };
        button(label).on_press(Message::Mode(mode)).style(style)
    };
    let field = |placeholder, value, on_input: fn(String) -> Message| {
        text_input(placeholder, value).on_input(on_input).on_submit(Message::Submit).padding(10)
    };
    let form = &auth.form;
    let register = auth.mode == Mode::Register;
    let mut fields = column![field("Usuário", &form.username, Message::Username)].spacing(10);
    if register {
        fields = fields.push(field("Nome de exibição", &form.display_name, Message::DisplayName));
    }
    fields = fields.push(field("Senha", &form.password, Message::Password).secure(true));
    if register {
        let label = "Código de convite (se o Call pedir)";
        fields = fields.push(field(label, &form.invite_code, Message::InviteCode));
    }
    let submit = match (auth.busy, register) {
        (true, _) => "Aguarde...",
        (false, false) => "Entrar",
        (false, true) => "Criar conta",
    };
    let mut body = column![
        text("Call").size(32),
        row![tab("Entrar", Mode::Login), tab("Criar conta", Mode::Register)].spacing(8),
        fields,
        button(text(submit).center())
            .width(Length::Fill)
            .padding(10)
            .on_press_maybe((!auth.busy).then_some(Message::Submit)),
    ]
    .spacing(16);
    if let Some(error) = &auth.error {
        body = body.push(text(error).style(text::danger));
    }
    card(body)
}

fn card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content).padding(32).max_width(380).style(|_| chat::card_style()).into()
}
