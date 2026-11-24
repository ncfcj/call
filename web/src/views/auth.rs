//! Tela de entrada: login e cadastro.

use leptos::{ev::SubmitEvent, prelude::*, task::spawn_local};
use proto::{
    LoginRequest, RegisterRequest,
    auth::{normalize_username, validate_display_name, validate_password, validate_username},
};

use crate::store::Store;

#[component]
pub fn AuthPage() -> impl IntoView {
    let store = expect_context::<Store>();
    let registering = RwSignal::new(false);
    let username = RwSignal::new(String::new());
    let display_name = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let invite_code = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);

    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        error.set(None);
        let user = normalize_username(&username.get_untracked());
        let pass = password.get_untracked();
        let name = display_name.get_untracked().trim().to_owned();
        let code = invite_code.get_untracked().trim().to_owned();
        let register = registering.get_untracked();

        // As mesmas regras do servidor, para errar antes de ir à rede.
        let invalid = if register {
            validate_username(&user)
                .and_then(|()| validate_display_name(&name))
                .and_then(|()| validate_password(&pass))
                .err()
        } else {
            None
        };
        if let Some(msg) = invalid {
            error.set(Some(msg.to_owned()));
            return;
        }

        busy.set(true);
        spawn_local(async move {
            let api = store.api();
            let result = if register {
                api.register(&RegisterRequest {
                    username: user,
                    display_name: name,
                    password: pass,
                    invite_code: (!code.is_empty()).then_some(code),
                })
                .await
            } else {
                api.login(&LoginRequest { username: user, password: pass }).await
            };
            busy.set(false);
            match result {
                Ok(me) => store.start_session(me),
                Err(err) => error.set(Some(err.to_string())),
            }
        });
    };

    view! {
        <main class="auth">
            <form class="auth-card" on:submit=submit>
                <h1>"Call"</h1>
                <div class="tabs">
                    <button
                        type="button"
                        class:active=move || !registering.get()
                        on:click=move |_| registering.set(false)
                    >
                        "Entrar"
                    </button>
                    <button
                        type="button"
                        class:active=move || registering.get()
                        on:click=move |_| registering.set(true)
                    >
                        "Criar conta"
                    </button>
                </div>
                <label>
                    "Usuário"
                    <input bind:value=username autocomplete="username" required />
                </label>
                <Show when=move || registering.get()>
                    <label>
                        "Nome de exibição"
                        <input bind:value=display_name autocomplete="nickname" />
                    </label>
                </Show>
                <label>
                    "Senha"
                    <input
                        type="password"
                        bind:value=password
                        autocomplete=move || {
                            if registering.get() { "new-password" } else { "current-password" }
                        }
                        required
                    />
                </label>
                <Show when=move || registering.get()>
                    <label>
                        "Código de convite (se o Call pedir)"
                        <input bind:value=invite_code autocomplete="off" />
                    </label>
                </Show>
                <Show when=move || error.get().is_some()>
                    <p class="form-error">{move || error.get()}</p>
                </Show>
                <button class="primary" type="submit" disabled=move || busy.get()>
                    {move || if registering.get() { "Criar conta" } else { "Entrar" }}
                </button>
            </form>
        </main>
    }
}
