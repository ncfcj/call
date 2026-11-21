//! Estado do client, independente da UI.
//!
//! Aqui fica a lógica que dá para testar sem navegador: o histórico de um canal sem duplicatas,
//! o backoff da reconexão e quem está digitando. A UI (Leptos) guarda esses tipos em sinais e
//! só reage a eles.

mod backoff;
mod messages;
mod typing;

pub use backoff::Backoff;
pub use messages::MessageLog;
pub use typing::Typing;

/// Estado do WebSocket, exibido na barra lateral.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    Connecting,
    Connected,
    /// Caiu; tenta de novo em `in_secs` segundos.
    Reconnecting {
        in_secs: u32,
    },
}
