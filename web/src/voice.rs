//! Chamada de voz: entra e sai do canal, e acompanha quem está na sala.
//!
//! Só existe uma chamada por vez. Cada entrada abre uma "sessão de voz" numerada; eventos de
//! uma sala antiga (que ainda está fechando) chegam com o número velho e são ignorados.

use std::{collections::HashSet, rc::Rc};

use call_core::VoiceControls;
use leptos::{prelude::*, task::spawn_local};
use proto::{ChannelId, GuildId};
use voice::{VoiceEvent, VoiceParticipant, VoiceRoom};

use crate::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceStatus {
    Connecting,
    Connected,
    Reconnecting,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceState {
    pub guild: GuildId,
    pub channel: ChannelId,
    pub status: VoiceStatus,
    pub participants: Vec<VoiceParticipant>,
    /// Identidades de quem está falando agora.
    pub speaking: HashSet<String>,
    /// O navegador bloqueou o áudio até um clique.
    pub audio_blocked: bool,
}

impl Store {
    fn is_voice_current(self, session: u64) -> bool {
        self.voice_session.get_value() == session
    }

    pub fn in_voice_channel(self, channel: ChannelId) -> bool {
        self.voice.with(|v| v.as_ref().is_some_and(|v| v.channel == channel))
    }

    /// Entra no canal de voz. Se já estiver em outro, sai dele antes.
    pub fn join_voice(self, guild: GuildId, channel: ChannelId) {
        if self.voice.with_untracked(|v| v.as_ref().is_some_and(|v| v.channel == channel)) {
            return;
        }
        self.leave_voice();
        let session = self.voice_session.get_value();
        self.voice.set(Some(VoiceState {
            guild,
            channel,
            status: VoiceStatus::Connecting,
            participants: Vec::new(),
            speaking: HashSet::new(),
            audio_blocked: false,
        }));
        let room = Rc::new(VoiceRoom::new(move |event| self.on_voice_event(session, event)));
        self.voice_room.set_value(Some(room.clone()));

        spawn_local(async move {
            let token = match self.api().voice_token(channel).await {
                Ok(token) => token,
                Err(err) => {
                    if self.is_voice_current(session) {
                        self.drop_voice();
                    }
                    return self.report(err);
                }
            };
            if !self.is_voice_current(session) {
                return;
            }
            let controls = self.voice_controls.get_untracked();
            let connected = room
                .connect(&token.url, &token.token, controls.mic_open(), controls.deafened)
                .await;
            if let Err(err) = connected {
                room.disconnect().await;
                if self.is_voice_current(session) {
                    self.drop_voice();
                    self.notify(format!("Não consegui entrar na voz: {err}"));
                }
                return;
            }
            if !self.is_voice_current(session) {
                // Saiu (ou trocou de canal) enquanto conectava.
                room.disconnect().await;
                return;
            }
            self.voice.update(|v| {
                if let Some(v) = v {
                    v.status = VoiceStatus::Connected;
                }
            });
        });
    }

    /// Sai da chamada, se estiver numa.
    pub fn leave_voice(self) {
        self.voice_session.update_value(|s| *s += 1);
        self.voice.set(None);
        if let Some(room) = self.voice_room.get_value() {
            self.voice_room.set_value(None);
            spawn_local(async move { room.disconnect().await });
        }
    }

    pub fn toggle_mute(self) {
        self.voice_controls.update(VoiceControls::toggle_mute);
        self.apply_controls();
    }

    pub fn toggle_deaf(self) {
        self.voice_controls.update(VoiceControls::toggle_deaf);
        self.apply_controls();
    }

    /// Guarda o mute e o deaf e aplica na sala aberta, se houver.
    fn apply_controls(self) {
        let controls = self.voice_controls.get_untracked();
        save_controls(controls);
        let Some(room) = self.voice_room.get_value() else { return };
        room.set_deafened(controls.deafened);
        spawn_local(async move {
            if let Err(err) = room.set_microphone_enabled(controls.mic_open()).await {
                self.notify(format!("O microfone não abriu ({err})."));
            }
        });
    }

    /// Destrava o áudio bloqueado pelo navegador. Chamado por um clique.
    pub fn unlock_audio(self) {
        if let Some(room) = self.voice_room.get_value() {
            spawn_local(async move { room.start_audio().await });
        }
    }

    /// Esquece a sala atual sem chamar o LiveKit (ela já caiu ou nem conectou).
    fn drop_voice(self) {
        self.voice_session.update_value(|s| *s += 1);
        self.voice.set(None);
        self.voice_room.set_value(None);
    }

    fn on_voice_event(self, session: u64, event: VoiceEvent) {
        if !self.is_voice_current(session) {
            return;
        }
        match event {
            VoiceEvent::Participants(list) => self.voice.update(|v| {
                if let Some(v) = v {
                    v.participants = list;
                }
            }),
            VoiceEvent::ActiveSpeakers(ids) => self.voice.update(|v| {
                if let Some(v) = v {
                    v.speaking = ids.into_iter().collect();
                }
            }),
            VoiceEvent::Reconnecting => self.set_voice_status(VoiceStatus::Reconnecting),
            VoiceEvent::Reconnected => self.set_voice_status(VoiceStatus::Connected),
            VoiceEvent::AudioBlocked(blocked) => self.voice.update(|v| {
                if let Some(v) = v {
                    v.audio_blocked = blocked;
                }
            }),
            VoiceEvent::MicError { message } => {
                self.notify(format!("O microfone não abriu ({message}). Você está só ouvindo."));
            }
            VoiceEvent::Disconnected { reason } => {
                let was_connecting = self.voice.with_untracked(|v| {
                    v.as_ref().is_some_and(|v| v.status == VoiceStatus::Connecting)
                });
                self.drop_voice();
                // Durante a entrada, quem avisa é o erro do `connect`.
                if !was_connecting {
                    leptos::logging::log!("voz desconectada: motivo {reason}");
                    self.notify("Você saiu da chamada de voz: a conexão caiu.");
                }
            }
        }
    }

    fn set_voice_status(self, status: VoiceStatus) {
        self.voice.update(|v| {
            if let Some(v) = v {
                v.status = status;
            }
        });
    }
}

const CONTROLS_KEY: &str = "call.voice.controls";

fn storage() -> Option<web_sys::Storage> {
    window().local_storage().ok().flatten()
}

/// Mute e deaf guardados no navegador (`"muted,deafened"`, com 0 e 1).
pub fn load_controls() -> VoiceControls {
    let saved = storage().and_then(|s| s.get_item(CONTROLS_KEY).ok().flatten()).unwrap_or_default();
    let mut flags = saved.split(',').map(|v| v == "1");
    VoiceControls { muted: flags.next().unwrap_or(false), deafened: flags.next().unwrap_or(false) }
}

fn save_controls(c: VoiceControls) {
    if let Some(s) = storage() {
        let _ =
            s.set_item(CONTROLS_KEY, &format!("{},{}", u8::from(c.muted), u8::from(c.deafened)));
    }
}
