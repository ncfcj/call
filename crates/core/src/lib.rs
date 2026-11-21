//! Estado do client, independente da UI.
//!
//! A Fase 1 traz aqui o estado de sessão, servidores, canais e mensagens, alimentado
//! pelos eventos `proto::ws::ServerEvent`. A UI (Leptos) só lê e reage a esse estado.

/// Estado da conexão com a API, exibido na barra de status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiStatus {
    Checking,
    Online { version: String, database: bool },
    Offline { reason: String },
}

impl ApiStatus {
    pub fn from_health(health: &proto::HealthResponse) -> Self {
        Self::Online { version: health.version.clone(), database: health.database }
    }
}
