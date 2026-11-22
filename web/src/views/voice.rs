//! Voz na barra de canais: o canal clicável, quem está nele e o painel da chamada.

use leptos::prelude::*;
use proto::Channel;

use crate::{
    store::Store,
    voice::{VoiceState, VoiceStatus},
};

/// Um canal de voz. Clicar entra na chamada; quem está nela aparece logo abaixo.
#[component]
pub fn VoiceChannel(channel: Channel) -> impl IntoView {
    let store = expect_context::<Store>();
    let id = channel.id;
    let guild = channel.guild_id;
    let here = move || store.in_voice_channel(id);
    let participants = move || {
        store.voice.with(|v| match v {
            Some(v) if v.channel == id => v
                .participants
                .iter()
                .map(|p| (p.clone(), v.speaking.contains(&p.identity)))
                .collect(),
            _ => Vec::new(),
        })
    };

    view! {
        <button
            class="channel voice"
            class:active=here
            title=move || if here() { "Você está nesta chamada" } else { "Entrar na chamada" }
            on:click=move |_| store.join_voice(guild, id)
        >
            "🔊 " {channel.name}
        </button>
        <ul class="voice-members">
            {move || {
                participants()
                    .into_iter()
                    .map(|(p, speaking)| {
                        // O deaf dos outros só aparece com o estado de voz no WebSocket.
                        let deaf = p.local && store.voice_controls.with(|c| c.deafened);
                        let icon = if deaf {
                            Some(("Sem som e sem microfone", "🎧✕"))
                        } else if !p.mic {
                            Some(("Microfone fechado", "🔇"))
                        } else {
                            None
                        };
                        view! {
                            <li class:speaking=speaking class:muted=!p.mic>
                                <span class="voice-dot"></span>
                                {p.name}
                                {icon.map(|(title, text)| view! { <span class="mic-off" title=title>{text}</span> })}
                            </li>
                        }
                    })
                    .collect_view()
            }}
        </ul>
    }
}

/// Painel acima do usuário, enquanto houver uma chamada: estado da conexão e botão de sair.
#[component]
pub fn VoicePanel() -> impl IntoView {
    let store = expect_context::<Store>();
    let channel_name = move |state: &VoiceState| {
        store.channels.with(|c| {
            c.get(&state.guild)
                .and_then(|list| list.iter().find(|ch| ch.id == state.channel))
                .map(|ch| ch.name.clone())
                .unwrap_or_default()
        })
    };

    view! {
        {move || {
            store
                .voice
                .get()
                .map(|state| {
                    let (class, text) = match state.status {
                        VoiceStatus::Connecting => ("connecting", "Conectando à voz..."),
                        VoiceStatus::Connected => ("online", "Voz conectada"),
                        VoiceStatus::Reconnecting => ("offline", "Voz reconectando..."),
                    };
                    let name = channel_name(&state);
                    view! {
                        <section class="voice-panel">
                            <div>
                                <span class=format!("conn {class}")>{text}</span>
                                <small>"🔊 " {name}</small>
                            </div>
                            <Show when=move || state.audio_blocked>
                                <button
                                    class="primary small"
                                    title="O navegador bloqueou o som até um clique"
                                    on:click=move |_| store.unlock_audio()
                                >
                                    "Ativar som"
                                </button>
                            </Show>
                            <button class="hangup" title="Sair da chamada" on:click=move |_| store.leave_voice()>
                                "Sair"
                            </button>
                        </section>
                    }
                })
        }}
    }
}

/// Botões de microfone e de fone, no painel do usuário. Valem dentro e fora de uma chamada.
/// Atalhos: Ctrl+Shift+M (microfone) e Ctrl+Shift+D (fone).
#[component]
pub fn VoiceButtons() -> impl IntoView {
    let store = expect_context::<Store>();
    let controls = move || store.voice_controls.get();
    view! {
        <div class="voice-buttons">
            <button
                class="icon-button"
                class:off=move || controls().mic_shown_muted()
                aria-pressed=move || controls().mic_shown_muted().to_string()
                title=move || {
                    if controls().mic_shown_muted() { "Abrir o microfone (Ctrl+Shift+M)" } else { "Fechar o microfone (Ctrl+Shift+M)" }
                }
                on:click=move |_| store.toggle_mute()
            >
                {move || if controls().mic_shown_muted() { "🔇" } else { "🎤" }}
            </button>
            <button
                class="icon-button"
                class:off=move || controls().deafened
                aria-pressed=move || controls().deafened.to_string()
                title=move || {
                    if controls().deafened { "Voltar a ouvir (Ctrl+Shift+D)" } else { "Parar de ouvir (Ctrl+Shift+D)" }
                }
                on:click=move |_| store.toggle_deaf()
            >
                {move || if controls().deafened { "🎧✕" } else { "🎧" }}
            </button>
        </div>
    }
}

/// Atalhos globais de mute e deaf.
pub fn install_shortcuts(store: Store) {
    let handle = window_event_listener(leptos::ev::keydown, move |ev| {
        if !(ev.ctrl_key() && ev.shift_key()) || ev.repeat() {
            return;
        }
        match ev.key().to_ascii_lowercase().as_str() {
            "m" => {
                ev.prevent_default();
                store.toggle_mute();
            }
            "d" => {
                ev.prevent_default();
                store.toggle_deaf();
            }
            _ => {}
        }
    });
    // Vive enquanto o app estiver montado.
    std::mem::forget(handle);
}
