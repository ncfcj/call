//! Tela principal: servidores, canais, mensagens e membros.

use call_core::ConnState;
use leptos::{
    ev::{KeyboardEvent, SubmitEvent},
    html,
    prelude::*,
    task::spawn_local,
};
use proto::{
    Channel, ChannelId, ChannelKind, ClientEvent, Guild, Message, message::validate_message_content,
};

use crate::store::{Store, now_ms};

#[component]
pub fn ChatPage() -> impl IntoView {
    let store = expect_context::<Store>();
    let adding = RwSignal::new(false);

    view! {
        <div class="layout">
            <GuildBar adding />
            <ChannelSidebar />
            {move || match store.selected_guild.get() {
                Some(_) => view! { <MessagePane /> <MemberList /> }.into_any(),
                None => view! {
                    <section class="empty">
                        <p>"Você ainda não está em nenhum servidor."</p>
                        <button class="primary" on:click=move |_| adding.set(true)>
                            "Criar ou entrar num servidor"
                        </button>
                    </section>
                }
                .into_any(),
            }}
            <Show when=move || adding.get()>
                <AddGuildDialog adding />
            </Show>
        </div>
    }
}

fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

#[component]
fn GuildBar(adding: RwSignal<bool>) -> impl IntoView {
    let store = expect_context::<Store>();
    view! {
        <nav class="guild-bar">
            <For
                each=move || store.guilds.get()
                key=|g: &Guild| (g.id, g.name.clone())
                children=move |guild: Guild| {
                    let id = guild.id;
                    view! {
                        <button
                            class="guild-icon"
                            class:active=move || store.selected_guild.get() == Some(id)
                            title=guild.name.clone()
                            on:click=move |_| store.select_guild(id)
                        >
                            {initials(&guild.name)}
                        </button>
                    }
                }
            />
            <button class="guild-icon add" title="Criar ou entrar num servidor" on:click=move |_| adding.set(true)>
                "+"
            </button>
        </nav>
    }
}

#[component]
fn AddGuildDialog(adding: RwSignal<bool>) -> impl IntoView {
    let store = expect_context::<Store>();
    let name = RwSignal::new(String::new());
    let code = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    let create = move |ev: SubmitEvent| {
        ev.prevent_default();
        let value = name.get_untracked().trim().to_owned();
        if value.is_empty() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            if store.create_guild(value).await {
                adding.set(false);
            }
            busy.set(false);
        });
    };
    let join = move |ev: SubmitEvent| {
        ev.prevent_default();
        let value = code.get_untracked().trim().to_owned();
        if value.is_empty() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            if store.join_invite(value).await {
                adding.set(false);
            }
            busy.set(false);
        });
    };

    view! {
        <div class="overlay" on:click=move |_| adding.set(false)>
            <div class="dialog" on:click=|ev| ev.stop_propagation()>
                <form on:submit=create>
                    <h2>"Criar um servidor"</h2>
                    <input bind:value=name placeholder="Nome do servidor" maxlength="100" />
                    <button class="primary" type="submit" disabled=move || busy.get()>"Criar"</button>
                </form>
                <hr />
                <form on:submit=join>
                    <h2>"Entrar com um convite"</h2>
                    <input bind:value=code placeholder="Código, ex.: hCXDYXjN" />
                    <button class="primary" type="submit" disabled=move || busy.get()>"Entrar"</button>
                </form>
                <button class="link" on:click=move |_| adding.set(false)>"Cancelar"</button>
            </div>
        </div>
    }
}

#[component]
fn ChannelSidebar() -> impl IntoView {
    let store = expect_context::<Store>();
    let guild = Memo::new(move |_| {
        let id = store.selected_guild.get()?;
        store.guilds.with(|g| g.iter().find(|x| x.id == id).cloned())
    });
    let channels = move |kind: ChannelKind| {
        let Some(id) = store.selected_guild.get() else { return Vec::new() };
        store.channels.with(|c| {
            c.get(&id)
                .map(|list| list.iter().filter(|ch| ch.kind == kind).cloned().collect())
                .unwrap_or_default()
        })
    };
    let is_owner = move || {
        let me = store.me.with(|m| m.as_ref().map(|u| u.id));
        guild.with(|g| g.as_ref().map(|g| g.owner_id)) == me && me.is_some()
    };

    view! {
        <aside class="channels">
            <Show when=move || guild.get().is_some()>
                <header>
                    <h2>{move || guild.get().map(|g| g.name)}</h2>
                    <p class="invite" title="Mande este código para quem quiser entrar">
                        "Convite: " <code>{move || guild.get().map(|g| g.invite_code)}</code>
                    </p>
                </header>
                <h3>"Canais de texto"</h3>
                <For
                    each=move || channels(ChannelKind::Text)
                    key=|c: &Channel| (c.id, c.name.clone())
                    children=move |channel: Channel| {
                        let id = channel.id;
                        view! {
                            <button
                                class="channel"
                                class:active=move || store.selected_channel.get() == Some(id)
                                class:unread=move || store.unread.with(|u| u.contains(&id))
                                on:click=move |_| store.select_channel(id)
                            >
                                "# " {channel.name}
                            </button>
                        }
                    }
                />
                <h3>"Canais de voz"</h3>
                <For
                    each=move || channels(ChannelKind::Voice)
                    key=|c: &Channel| (c.id, c.name.clone())
                    children=move |channel: Channel| {
                        view! { <super::voice::VoiceChannel channel /> }
                    }
                />
                <Show when=is_owner>
                    <NewChannelForm />
                </Show>
                <Show when=move || !is_owner()>
                    <button
                        class="link danger"
                        on:click=move |_| {
                            if let Some(id) = store.selected_guild.get_untracked() {
                                spawn_local(store.leave_guild(id));
                            }
                        }
                    >
                        "Sair do servidor"
                    </button>
                </Show>
            </Show>
            <super::voice::VoicePanel />
            <UserPanel />
        </aside>
    }
}

#[component]
fn NewChannelForm() -> impl IntoView {
    let store = expect_context::<Store>();
    let name = RwSignal::new(String::new());
    let voice = RwSignal::new(false);
    let submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        let value = name.get_untracked().trim().to_owned();
        let Some(guild) = store.selected_guild.get_untracked() else { return };
        if value.is_empty() {
            return;
        }
        let kind = if voice.get_untracked() { ChannelKind::Voice } else { ChannelKind::Text };
        spawn_local(async move {
            if store.create_channel(guild, value, kind).await {
                name.set(String::new());
            }
        });
    };
    view! {
        <form class="new-channel" on:submit=submit>
            <input bind:value=name placeholder="Novo canal" maxlength="100" />
            <label title="Canal de voz">
                <input type="checkbox" bind:checked=voice /> "voz"
            </label>
            <button type="submit">"+"</button>
        </form>
    }
}

#[component]
fn UserPanel() -> impl IntoView {
    let store = expect_context::<Store>();
    let status = move || match store.conn.get() {
        ConnState::Connected => ("online", "Conectado".to_owned()),
        ConnState::Connecting => ("connecting", "Conectando...".to_owned()),
        ConnState::Reconnecting { in_secs } => {
            ("offline", format!("Sem conexão. Tentando em {in_secs} s"))
        }
    };
    view! {
        <footer class="user-panel">
            <div>
                <strong>{move || store.me.get().map(|u| u.display_name)}</strong>
                <span class=move || format!("conn {}", status().0)>{move || status().1}</span>
            </div>
            <super::voice::VoiceButtons />
            <button class="link" on:click=move |_| store.logout()>"Sair"</button>
        </footer>
    }
}

/// Hora local `HH:MM` de uma mensagem.
fn clock(message: &Message) -> String {
    let date = js_sys::Date::new(&(message.created_at.timestamp_millis() as f64).into());
    format!("{:02}:{:02}", date.get_hours(), date.get_minutes())
}

/// Mostra o cabeçalho (autor e hora) quando muda o autor ou passam 5 min.
fn starts_group(prev: Option<&Message>, message: &Message) -> bool {
    let Some(prev) = prev else { return true };
    let same_author = prev.author.as_ref().map(|a| a.id) == message.author.as_ref().map(|a| a.id);
    let gap = message.created_at - prev.created_at;
    !same_author || gap.num_minutes() >= 5
}

#[component]
fn MessagePane() -> impl IntoView {
    let store = expect_context::<Store>();
    let list = NodeRef::<html::Div>::new();
    let stick_to_bottom = RwSignal::new(true);
    let loading_older = RwSignal::new(false);

    let channel = Memo::new(move |_| {
        let guild = store.selected_guild.get()?;
        let id = store.selected_channel.get()?;
        store.channels.with(|c| c.get(&guild)?.iter().find(|ch| ch.id == id).cloned())
    });
    let rows = Memo::new(move |_| {
        let Some(id) = store.selected_channel.get() else { return Vec::new() };
        store.logs.with(|logs| {
            let Some(log) = logs.get(&id) else { return Vec::new() };
            let msgs = log.messages();
            msgs.iter()
                .enumerate()
                .map(|(i, m)| (m.clone(), starts_group(i.checked_sub(1).map(|p| &msgs[p]), m)))
                .collect::<Vec<_>>()
        })
    });
    let has_older = move || {
        store
            .selected_channel
            .get()
            .is_some_and(|id| store.logs.with(|l| l.get(&id).is_some_and(|log| log.has_older)))
    };

    // Mensagem nova: desce até o fim, se o usuário já estava lá. Ao trocar de canal, desce sempre.
    Effect::new(move |prev: Option<Option<ChannelId>>| {
        let current = store.selected_channel.get();
        rows.track();
        let switched = prev.is_some_and(|p| p != current);
        if switched {
            stick_to_bottom.set(true);
        }
        if stick_to_bottom.get_untracked() {
            request_animation_frame(move || {
                if let Some(el) = list.get_untracked() {
                    el.set_scroll_top(el.scroll_height());
                }
            });
        }
        current
    });

    let on_scroll = move |_| {
        if let Some(el) = list.get_untracked() {
            let distance = el.scroll_height() - el.scroll_top() - el.client_height();
            stick_to_bottom.set(distance < 40);
        }
    };

    let load_older = move |_| {
        let Some(id) = store.selected_channel.get_untracked() else { return };
        let Some(el) = list.get_untracked() else { return };
        let before = el.scroll_height();
        loading_older.set(true);
        spawn_local(async move {
            store.load_older(id).await;
            loading_older.set(false);
            // Mantém na tela a mensagem que o usuário estava lendo.
            request_animation_frame(move || {
                if let Some(el) = list.get_untracked() {
                    el.set_scroll_top(el.scroll_top() + el.scroll_height() - before);
                }
            });
        });
    };

    let typing_line = move || {
        let guild = store.selected_guild.get()?;
        let id = store.selected_channel.get()?;
        let me = store.me.with(|m| m.as_ref().map(|u| u.id));
        let names: Vec<String> = store
            .typing
            .with(|t| t.active(id, store.now.get()))
            .into_iter()
            .filter(|u| Some(*u) != me)
            .map(|u| store.display_name(guild, u))
            .collect();
        match names.as_slice() {
            [] => None,
            [one] => Some(format!("{one} está digitando...")),
            many => Some(format!("{} estão digitando...", many.join(", "))),
        }
    };

    view! {
        <section class="messages">
            <header>
                <h2>{move || channel.get().map(|c| format!("# {}", c.name))}</h2>
            </header>
            <div class="message-list" node_ref=list on:scroll=on_scroll>
                <Show when=has_older>
                    <button class="link older" on:click=load_older disabled=move || loading_older.get()>
                        "Carregar mensagens antigas"
                    </button>
                </Show>
                <For
                    each=move || rows.get()
                    key=|(m, head): &(Message, bool)| (m.id, *head, m.content.clone())
                    children=move |(message, head): (Message, bool)| {
                        let author = message
                            .author
                            .as_ref()
                            .map(|a| a.display_name.clone())
                            .unwrap_or_else(|| "conta apagada".to_owned());
                        let time = clock(&message);
                        let content = message.content;
                        view! {
                            <article class="message" class:grouped=!head>
                                <Show when=move || head>
                                    <div class="meta">
                                        <strong>{author.clone()}</strong>
                                        <time>{time.clone()}</time>
                                    </div>
                                </Show>
                                <p>{content}</p>
                            </article>
                        }
                    }
                />
            </div>
            <p class="typing">{typing_line}</p>
            <Composer />
        </section>
    }
}

#[component]
fn Composer() -> impl IntoView {
    let store = expect_context::<Store>();
    let text = RwSignal::new(String::new());
    let last_typing = StoredValue::new(0.0_f64);

    let send = move || {
        let content = text.get_untracked();
        let Some(channel) = store.selected_channel.get_untracked() else { return };
        if let Err(msg) = validate_message_content(&content) {
            if !content.trim().is_empty() {
                store.notify(msg);
            }
            return;
        }
        text.set(String::new());
        last_typing.set_value(0.0);
        spawn_local(async move {
            if !store.send_message(channel, content.clone()).await {
                text.set(content); // devolve o texto para tentar de novo
            }
        });
    };

    let on_keydown = move |ev: KeyboardEvent| {
        if ev.key() == "Enter" && !ev.shift_key() {
            ev.prevent_default();
            send();
        }
    };
    // Avisa que está digitando no máximo a cada 7 s (o aviso vale 8 s nos outros clients).
    let on_input = move |_| {
        let Some(channel) = store.selected_channel.get_untracked() else { return };
        let now = now_ms();
        if now - last_typing.get_value() > 7_000.0 {
            last_typing.set_value(now);
            store.send_ws(ClientEvent::TypingStart { channel_id: channel });
        }
    };

    view! {
        <form class="composer" on:submit=move |ev: SubmitEvent| { ev.prevent_default(); send(); }>
            <textarea
                bind:value=text
                rows="1"
                placeholder="Mensagem (Enter envia, Shift+Enter quebra a linha)"
                on:keydown=on_keydown
                on:input=on_input
            ></textarea>
        </form>
    }
}

#[component]
fn MemberList() -> impl IntoView {
    let store = expect_context::<Store>();
    let groups = move || {
        let Some(guild) = store.selected_guild.get() else { return (Vec::new(), Vec::new()) };
        let online = store.online.with(|o| o.get(&guild).cloned().unwrap_or_default());
        store.members.with(|m| {
            m.get(&guild)
                .map(|list| list.iter().cloned().partition(|x| online.contains(&x.user.id)))
                .unwrap_or_default()
        })
    };
    let member_row = |member: proto::Member, on: bool| {
        view! {
            <li class:online=on>
                <span class="dot"></span>
                {member.user.display_name}
                <Show when=move || member.is_owner>
                    <span class="badge" title="Dono do servidor">"dono"</span>
                </Show>
            </li>
        }
    };
    view! {
        <aside class="members">
            <h3>{move || format!("Online — {}", groups().0.len())}</h3>
            <ul>{move || groups().0.into_iter().map(|m| member_row(m, true)).collect_view()}</ul>
            <h3>{move || format!("Offline — {}", groups().1.len())}</h3>
            <ul>{move || groups().1.into_iter().map(|m| member_row(m, false)).collect_view()}</ul>
        </aside>
    }
}
