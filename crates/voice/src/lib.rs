//! Ponte com o `livekit-client` (JS), implementada na Fase 2.
//!
//! API planejada, pequena de propósito:
//! - `connect(url, token)` e `disconnect()`
//! - `set_microphone_enabled(bool)` e `set_deafened(bool)`
//! - eventos: participantes entrando/saindo e quem está falando
//!
//! O SDK Rust do LiveKit não é usado aqui porque depende do libwebrtc nativo e não
//! compila para wasm32. Na web e no Tauri a voz usa o WebRTC do navegador/WebView.
