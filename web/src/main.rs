use api_client::ApiClient;
use call_core::ApiStatus;
use leptos::prelude::*;
use leptos::task::spawn_local;

/// URL da API. Em build: `$env:CALL_API_URL="https://api.exemplo.com"; trunk build --release`
const API_URL: &str = match option_env!("CALL_API_URL") {
    Some(url) => url,
    None => "http://127.0.0.1:3000",
};

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let api = ApiClient::new(API_URL);
    let (status, set_status) = signal(ApiStatus::Checking);

    spawn_local(async move {
        let next = match api.health().await {
            Ok(health) => ApiStatus::from_health(&health),
            Err(err) => ApiStatus::Offline { reason: err.to_string() },
        };
        set_status.set(next);
    });

    view! {
        <main class="app">
            <h1>"Call"</h1>
            <p>"Fase 0: o client Leptos conversa com a API em " <code>{API_URL}</code></p>
            {move || status_view(status.get())}
        </main>
    }
}

fn status_view(status: ApiStatus) -> impl IntoView {
    let (class, text) = match status {
        ApiStatus::Checking => ("status", "Verificando a API...".to_owned()),
        ApiStatus::Online { version, database: true } => {
            ("status online", format!("API online (v{version}), banco conectado"))
        }
        ApiStatus::Online { version, database: false } => {
            ("status degraded", format!("API online (v{version}), mas sem banco"))
        }
        ApiStatus::Offline { reason } => ("status offline", format!("API indisponível: {reason}")),
    };
    view! { <p class=class>{text}</p> }
}
