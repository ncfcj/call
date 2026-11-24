//! O app no navegador: monta a UI e escolhe a URL da API.

use api_client::ApiClient;
use leptos::prelude::*;

use crate::{store::Store, views};

/// URL da API. Em build: `$env:CALL_API_URL="https://api.exemplo.com"; trunk build --release`.
///
/// Vazia, é a origem da própria página (o servidor entrega o client, como no Render).
/// Sem ela, usa o mesmo host da página na porta 3000. O host precisa bater: o navegador trata
/// `localhost` e `127.0.0.1` como sites diferentes e aí não manda o cookie de refresh.
fn api_url() -> String {
    if let Some(url) = option_env!("CALL_API_URL") {
        if !url.is_empty() {
            return url.to_owned();
        }
        if let Ok(origin) = window().location().origin() {
            return origin;
        }
    }
    let location = window().location();
    let protocol = location.protocol().unwrap_or_else(|_| "http:".to_owned());
    let host = location.hostname().unwrap_or_else(|_| "localhost".to_owned());
    format!("{protocol}//{host}:3000")
}

pub fn start() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let store = Store::new(ApiClient::new(api_url()));
    provide_context(store);
    store.boot();
    views::voice::install_shortcuts(store);

    view! {
        <Show when=move || store.notice.get().is_some()>
            <div class="notice" on:click=move |_| store.notice.set(None)>
                {move || store.notice.get()}
            </div>
        </Show>
        <Show
            when=move || !store.booting.get()
            fallback=|| view! { <div class="splash">"Carregando..."</div> }
        >
            <Show when=move || store.me.get().is_some() fallback=views::auth::AuthPage>
                <views::chat::ChatPage />
            </Show>
        </Show>
    }
}
