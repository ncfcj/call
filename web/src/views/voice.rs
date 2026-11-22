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
                        view! {
                            <li class:speaking=speaking class:muted=!p.mic>
                                <span class="voice-dot"></span>
                                {p.name}
                                {(!p.mic).then_some(view! { <span class="mic-off" title="Microfone fechado">"🔇"</span> })}
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
