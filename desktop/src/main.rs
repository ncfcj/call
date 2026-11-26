//! App desktop do Call: a mesma API da web, com UI nativa em iced.

// Sem a janela do console no build de release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod session;
mod theme;

use api_client::{ApiClient, ApiError};
use iced::{
    Border, Element, Length, Size, Task,
    widget::{button, column, container, row, text, text_input},
};
use proto::{LoginRequest, RegisterRequest, User};

/// A API de produção; `CALL_API_URL` troca, por exemplo para `http://localhost:3000`.
const DEFAULT_API_URL: &str = "https://call-70hr.onrender.com";

fn main() -> iced::Result {
    iced::application(boot, update, view)
        .title("Call")
        .theme(|_: &State| theme::theme())
        .window_size(Size::new(1100.0, 720.0))
        .run()
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
    Home(User),
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
    Logout,
    LoggedOut,
}

struct State {
    api: ApiClient,
    screen: Screen,
}

fn boot() -> (State, Task<Message>) {
    let url = std::env::var("CALL_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_owned());
    let api = ApiClient::new(url);
    let task = match session::load(api.base_url()) {
        Some(cookie) => {
            api.restore_refresh_cookie(&cookie);
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
    (State { api, screen: Screen::Booting }, task)
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    let api = &state.api;
    match message {
        Message::Booted(user, error) => {
            state.screen = match user {
                Some(user) => Screen::Home(user),
                None => Screen::Auth(Auth::new(error)),
            };
            save_session(api);
        }
        Message::Authed(Ok(user)) => {
            save_session(api);
            state.screen = Screen::Home(user);
        }
        Message::Authed(Err(message)) => {
            if let Screen::Auth(auth) = &mut state.screen {
                auth.busy = false;
                auth.error = Some(message);
            }
        }
        Message::Logout => {
            // Sai já do cofre e da tela: se o app fechar antes de a API responder, a sessão não
            // volta na próxima abertura.
            session::save(api.base_url(), None);
            state.screen = Screen::Auth(Auth::new(None));
            let api = api.clone();
            return Task::perform(async move { api.logout().await }, |_| Message::LoggedOut);
        }
        Message::LoggedOut => save_session(api),
        other => {
            if let Screen::Auth(auth) = &mut state.screen {
                return edit_auth(api, auth, other);
            }
        }
    }
    Task::none()
}

/// O cookie muda a cada refresh e some no logout; o cofre acompanha.
fn save_session(api: &ApiClient) {
    session::save(api.base_url(), api.refresh_cookie().as_deref());
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
        Screen::Booting => text("Conectando ao servidor...").color(theme::MUTED).into(),
        Screen::Auth(auth) => auth_view(auth),
        Screen::Home(user) => card(
            column![
                text!("Olá, {}", user.display_name).size(28),
                text("Servidores, canais e chat chegam na próxima etapa.").color(theme::MUTED),
                button("Sair").on_press(Message::Logout).style(button::secondary),
            ]
            .spacing(16),
        ),
    };
    container(content).center(Length::Fill).into()
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
    container(content)
        .padding(32)
        .max_width(380)
        .style(|_| container::Style {
            background: Some(theme::SURFACE_CARD.into()),
            border: Border { radius: 12.0.into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
}
