//! Voz pelo SDK Rust do LiveKit, como o `web/src/voice.rs`. O microfone e a saída são os do
//! Windows (WASAPI), pelo `PlatformAudio`, com o cancelamento de eco, o ganho e a redução de
//! ruído do libwebrtc. O áudio dos outros toca sozinho na saída escolhida.

use std::{
    collections::HashSet,
    fmt,
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
};

use api_client::ApiClient;
use call_core::ui::device_options;
use iced::{Subscription, futures::stream};
use livekit::{
    PlatformAudio,
    prelude::{
        LocalAudioTrack, LocalTrack, RemoteTrack, Room, RoomEvent, RoomOptions,
        TrackPublishOptions, TrackSource,
    },
};
use proto::{ChannelId, GuildId};
use tokio::sync::mpsc::UnboundedReceiver;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Connected,
    Reconnecting,
}

/// Alguém na sala do LiveKit, com o estado do microfone.
#[derive(Debug, Clone)]
pub struct Participant {
    /// O id do usuário, como o servidor põe no token.
    pub identity: String,
    pub name: String,
    pub local: bool,
    pub mic: bool,
}

/// Um microfone ou uma saída na lista de escolha. `id` vazio é o padrão do sistema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

/// Microfone e saída escolhidos (`None` = padrão do sistema).
#[derive(Debug, Clone, Default)]
pub struct Prefs {
    pub input: Option<String>,
    pub output: Option<String>,
}

/// A sala conectada. As mensagens do iced precisam ser `Clone`, daí o `Arc`.
#[derive(Clone)]
pub struct Handle {
    room: Arc<Room>,
    mic: LocalAudioTrack,
    audio: PlatformAudio,
    /// Os eventos da sala, até a subscription pegá-los.
    events: Arc<Mutex<Option<UnboundedReceiver<RoomEvent>>>>,
}

impl Handle {
    /// Fecha a sala. Sem isso, os outros veem você na chamada até o LiveKit desistir.
    pub async fn close(self) {
        let _ = self.room.close().await;
    }
}

impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("voice::Handle")
    }
}

/// O que a sala avisa e a tela precisa saber.
#[derive(Debug, Clone)]
pub enum Event {
    /// Entrou ou saiu alguém, ou mudou um microfone: relê a sala.
    Changed,
    Speakers(Vec<String>),
    Reconnecting,
    Reconnected,
    Disconnected,
}

/// A chamada atual.
pub struct Call {
    pub guild: GuildId,
    pub channel: ChannelId,
    pub status: Status,
    pub participants: Vec<Participant>,
    /// Identidades de quem está falando agora.
    pub speaking: HashSet<String>,
    /// Muda a cada entrada; eventos e respostas de uma sala antiga são ignorados.
    pub session: u64,
    handle: Option<Handle>,
}

impl Call {
    pub fn new(guild: GuildId, channel: ChannelId, session: u64) -> Self {
        Self {
            guild,
            channel,
            status: Status::Connecting,
            participants: Vec::new(),
            speaking: HashSet::new(),
            session,
            handle: None,
        }
    }

    pub fn connected(&mut self, handle: Handle, deafened: bool) {
        self.handle = Some(handle);
        self.status = Status::Connected;
        self.refresh(deafened);
    }

    /// Relê quem está na sala e aplica o deaf a quem entrou.
    pub fn refresh(&mut self, deafened: bool) {
        let Some(handle) = &self.handle else { return };
        let local = handle.room.local_participant();
        let mut list = vec![Participant {
            identity: local.identity().to_string(),
            name: local.name(),
            local: true,
            mic: !handle.mic.is_muted(),
        }];
        for remote in handle.room.remote_participants().values() {
            let mut mic = false;
            for publication in remote.track_publications().values() {
                if publication.source() == TrackSource::Microphone && !publication.is_muted() {
                    mic = true;
                }
                if let Some(RemoteTrack::Audio(track)) = publication.track() {
                    if deafened { track.disable() } else { track.enable() }
                }
            }
            let name = remote.name();
            let identity = remote.identity().to_string();
            list.push(Participant {
                name: if name.is_empty() { identity.clone() } else { name },
                identity,
                local: false,
                mic,
            });
        }
        self.participants = list;
    }

    pub fn set_mic(&self, open: bool) {
        if let Some(handle) = &self.handle {
            if open { handle.mic.unmute() } else { handle.mic.mute() }
        }
    }

    pub fn audio(&self) -> Option<&PlatformAudio> {
        self.handle.as_ref().map(|h| &h.audio)
    }

    pub fn close(self) -> impl Future<Output = ()> + Send + 'static {
        let handle = self.handle;
        async move {
            if let Some(handle) = handle {
                handle.close().await;
            }
        }
    }

    pub fn subscription(&self) -> Subscription<Event> {
        match &self.handle {
            Some(handle) => Subscription::run_with(
                Key { session: self.session, handle: handle.clone() },
                |key| {
                    let events = key.handle.events.lock().unwrap().take();
                    stream::unfold(events, |events| async move {
                        let mut events = events?;
                        loop {
                            let event = match events.recv().await? {
                                RoomEvent::ActiveSpeakersChanged { speakers } => Event::Speakers(
                                    speakers.iter().map(|p| p.identity().to_string()).collect(),
                                ),
                                RoomEvent::Reconnecting => Event::Reconnecting,
                                RoomEvent::Reconnected => Event::Reconnected,
                                RoomEvent::Disconnected { .. } => Event::Disconnected,
                                RoomEvent::ParticipantConnected(_)
                                | RoomEvent::ParticipantDisconnected(_)
                                | RoomEvent::TrackSubscribed { .. }
                                | RoomEvent::TrackUnsubscribed { .. }
                                | RoomEvent::TrackPublished { .. }
                                | RoomEvent::TrackUnpublished { .. }
                                | RoomEvent::TrackMuted { .. }
                                | RoomEvent::TrackUnmuted { .. }
                                | RoomEvent::LocalTrackPublished { .. }
                                | RoomEvent::ParticipantNameChanged { .. } => Event::Changed,
                                _ => continue,
                            };
                            return Some((event, Some(events)));
                        }
                    })
                },
            ),
            None => Subscription::none(),
        }
    }
}

/// Uma subscription por sala.
struct Key {
    session: u64,
    handle: Handle,
}

impl Hash for Key {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.session.hash(state);
    }
}

/// Pede o token, abre o microfone e entra na sala.
pub async fn connect(
    api: ApiClient,
    channel: ChannelId,
    mic_open: bool,
    prefs: Prefs,
) -> Result<Handle, String> {
    let token = api.voice_token(channel).await.map_err(|err| err.to_string())?;
    let audio = PlatformAudio::new().map_err(|err| format!("sem áudio no sistema ({err})"))?;
    select_devices(&audio, &prefs);
    let mic = LocalAudioTrack::create_audio_track("microphone", audio.rtc_source());
    // Mutado antes de publicar já entra mudo, sem vazar nada.
    if !mic_open {
        mic.mute();
    }
    log::info!("voz: entrando na sala em {}", token.url);
    let (room, events) =
        timeout(Box::pin(Room::connect(&token.url, &token.token, RoomOptions::default())))
            .await?
            .map_err(|err| err.to_string())?;
    log::info!("voz: na sala; publicando o microfone");
    let publish = TrackPublishOptions { source: TrackSource::Microphone, ..Default::default() };
    let published =
        timeout(room.local_participant().publish_track(LocalTrack::Audio(mic.clone()), publish))
            .await
            .and_then(|res| res.map_err(|err| err.to_string()));
    if let Err(err) = published {
        let _ = room.close().await;
        return Err(format!("o microfone não abriu ({err})"));
    }
    log::info!("voz: conectado");
    Ok(Handle { room: Arc::new(room), mic, audio, events: Arc::new(Mutex::new(Some(events))) })
}

/// Sem resposta do LiveKit em 20 s, desiste em vez de ficar conectando para sempre.
async fn timeout<T>(fut: impl Future<Output = T>) -> Result<T, String> {
    tokio::time::timeout(std::time::Duration::from_secs(20), fut)
        .await
        .map_err(|_| "o servidor de voz não respondeu".to_owned())
}

/// Aplica a escolha. Um dispositivo que sumiu (fone desconectado) fica no padrão do sistema.
pub fn select_devices(audio: &PlatformAudio, prefs: &Prefs) {
    if let Some(id) = &prefs.input
        && let Some(device) = audio.recording_devices().find(|d| d.id.as_str() == id)
    {
        let _ = audio.switch_recording_device(&device.id);
    }
    if let Some(id) = &prefs.output
        && let Some(device) = audio.playout_devices().find(|d| d.id.as_str() == id)
    {
        let _ = audio.switch_playout_device(&device.id);
    }
}

/// Microfones e saídas, cada lista começando pelo padrão do sistema.
pub fn devices(audio: &PlatformAudio) -> (Vec<Device>, Vec<Device>) {
    let list = |items: Vec<(String, String)>, noun: &str| {
        let refs = items.iter().map(|(id, name)| (id.as_str(), name.as_str()));
        let mut out = vec![Device { id: String::new(), name: "Padrão do sistema".into() }];
        out.extend(device_options(refs, noun).into_iter().map(|(id, name)| Device { id, name }));
        out
    };
    let inputs = audio.recording_devices().map(|d| (d.id.as_str().to_owned(), d.name)).collect();
    let outputs = audio.playout_devices().map(|d| (d.id.as_str().to_owned(), d.name)).collect();
    (list(inputs, "Microfone"), list(outputs, "Saída"))
}
