use wasm_bindgen::{JsCast, JsValue, prelude::*};

use crate::{AudioDevices, VoiceEvent};

// `VoiceSession` vem de `js/voice.js`, carregado pelo web/index.html no global `CallVoice`.
#[wasm_bindgen(js_namespace = CallVoice)]
extern "C" {
    type VoiceSession;

    #[wasm_bindgen(constructor)]
    fn new(on_event: &js_sys::Function) -> VoiceSession;

    #[wasm_bindgen(method, catch)]
    #[allow(clippy::too_many_arguments)]
    async fn connect(
        this: &VoiceSession,
        url: &str,
        token: &str,
        mic_enabled: bool,
        deafened: bool,
        input_id: &str,
        output_id: &str,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(static_method_of = VoiceSession, js_name = listDevices)]
    async fn list_devices_json() -> JsValue;

    #[wasm_bindgen(method, catch, js_name = switchDevice)]
    async fn switch_device(
        this: &VoiceSession,
        kind: &str,
        device_id: &str,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, js_name = setDeafened)]
    fn set_deafened(this: &VoiceSession, deafened: bool);

    #[wasm_bindgen(method, catch, js_name = setMicrophoneEnabled)]
    async fn set_microphone_enabled(this: &VoiceSession, enabled: bool)
    -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, js_name = startAudio)]
    async fn start_audio(this: &VoiceSession) -> JsValue;

    #[wasm_bindgen(method)]
    async fn disconnect(this: &VoiceSession) -> JsValue;
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct VoiceError(String);

impl From<JsValue> for VoiceError {
    fn from(value: JsValue) -> Self {
        let text = value
            .dyn_ref::<js_sys::Error>()
            .map(|e| String::from(e.message()))
            .or_else(|| value.as_string())
            .unwrap_or_else(|| "erro desconhecido na voz".to_owned());
        Self(text)
    }
}

/// Uma sala de voz do LiveKit. Os eventos chegam ao callback passado em [`VoiceRoom::new`].
pub struct VoiceRoom {
    session: VoiceSession,
    // Mantém o callback vivo enquanto a sala existir.
    _on_event: Closure<dyn FnMut(String)>,
}

impl VoiceRoom {
    pub fn new(mut on_event: impl FnMut(VoiceEvent) + 'static) -> Self {
        let closure = Closure::<dyn FnMut(String)>::new(move |json: String| {
            if let Some(event) = VoiceEvent::parse(&json) {
                on_event(event);
            }
        });
        let session = VoiceSession::new(closure.as_ref().unchecked_ref());
        Self { session, _on_event: closure }
    }

    /// Entra na sala. Com `mic_enabled`, abre o microfone; se ele falhar, entra só ouvindo e
    /// avisa com `VoiceEvent::MicError`. Com `deafened`, o áudio dos outros já entra mudo.
    pub async fn connect(
        &self,
        url: &str,
        token: &str,
        opts: &ConnectOptions,
    ) -> Result<(), VoiceError> {
        self.session
            .connect(
                url,
                token,
                opts.mic_enabled,
                opts.deafened,
                opts.input.as_deref().unwrap_or(""),
                opts.output.as_deref().unwrap_or(""),
            )
            .await?;
        Ok(())
    }

    /// Troca o microfone durante a chamada. `None` volta ao padrão do sistema.
    pub async fn switch_input(&self, device: Option<&str>) -> Result<(), VoiceError> {
        self.session.switch_device("audioinput", device.unwrap_or("")).await?;
        Ok(())
    }

    /// Troca a saída de áudio durante a chamada. `None` volta ao padrão do sistema.
    pub async fn switch_output(&self, device: Option<&str>) -> Result<(), VoiceError> {
        self.session.switch_device("audiooutput", device.unwrap_or("")).await?;
        Ok(())
    }

    /// Deaf: silencia o áudio de todos os outros (sem sair da sala), ou volta a ouvir.
    pub fn set_deafened(&self, deafened: bool) {
        self.session.set_deafened(deafened);
    }

    pub async fn set_microphone_enabled(&self, enabled: bool) -> Result<(), VoiceError> {
        self.session.set_microphone_enabled(enabled).await?;
        Ok(())
    }

    /// Destrava o áudio bloqueado pelo navegador. Chame a partir de um clique.
    pub async fn start_audio(&self) {
        self.session.start_audio().await;
    }

    pub async fn disconnect(&self) {
        self.session.disconnect().await;
    }
}

/// Como entrar na sala: estado do microfone e do som, e os dispositivos escolhidos.
#[derive(Debug, Clone, Default)]
pub struct ConnectOptions {
    pub mic_enabled: bool,
    pub deafened: bool,
    /// Microfone (`deviceId`); `None` é o padrão do sistema.
    pub input: Option<String>,
    /// Saída de áudio (`deviceId`); `None` é o padrão do sistema.
    pub output: Option<String>,
}

/// Lista microfones e saídas de áudio. Pede a permissão do microfone se ainda não houver.
pub async fn list_devices() -> AudioDevices {
    VoiceSession::list_devices_json()
        .await
        .as_string()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}
