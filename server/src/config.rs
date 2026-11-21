use std::net::SocketAddr;

use anyhow::{Context, bail};
use axum::http::{HeaderValue, Method, header};
use tower_http::cors::{AllowOrigin, CorsLayer};

/// Configuração lida das variáveis de ambiente (ou do `.env` em desenvolvimento).
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    pub web_origins: Vec<HeaderValue>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url = std::env::var("DATABASE_URL").context("defina DATABASE_URL")?;

        let bind_addr = std::env::var("BIND_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:3000".to_owned())
            .parse()
            .context("BIND_ADDR inválido (ex.: 127.0.0.1:3000)")?;

        let web_origins = std::env::var("WEB_ORIGIN")
            .unwrap_or_else(|_| "http://localhost:8080".to_owned())
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| HeaderValue::from_str(s).with_context(|| format!("origem inválida: {s}")))
            .collect::<anyhow::Result<Vec<_>>>()?;
        if web_origins.is_empty() {
            bail!("WEB_ORIGIN precisa de pelo menos uma origem");
        }

        Ok(Self { database_url, bind_addr, web_origins })
    }

    /// CORS restrito às origens do client. Lembrete: CORS não protege WebSocket;
    /// o handler de WS da Fase 1 valida o header `Origin` por conta própria.
    pub fn cors_layer(&self) -> CorsLayer {
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(self.web_origins.clone()))
            .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
            .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
            .allow_credentials(true)
    }
}
