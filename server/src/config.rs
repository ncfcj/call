use std::net::SocketAddr;

use anyhow::{Context, bail};
use axum::http::{HeaderValue, Method, header};
use tower_http::cors::{AllowOrigin, CorsLayer};

/// Configuração lida das variáveis de ambiente (ou do `.env` em desenvolvimento).
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    /// Endereços em que o servidor escuta. Em desenvolvimento, IPv4 e IPv6 do loopback: o
    /// `localhost` resolve primeiro para `::1`, e o Firefox não tenta o `127.0.0.1` depois.
    pub bind_addrs: Vec<SocketAddr>,
    pub web_origins: Vec<HeaderValue>,
    /// Segredo HS256 dos access tokens. Pelo menos 32 bytes.
    pub jwt_secret: String,
    /// `Secure` no cookie de refresh. Só desligue em desenvolvimento sem HTTPS.
    pub cookie_secure: bool,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url = std::env::var("DATABASE_URL").context("defina DATABASE_URL")?;

        let bind_addrs = std::env::var("BIND_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:3000,[::1]:3000".to_owned())
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse().with_context(|| format!("BIND_ADDR inválido: {s} (ex.: 127.0.0.1:3000)"))
            })
            .collect::<anyhow::Result<Vec<SocketAddr>>>()?;
        if bind_addrs.is_empty() {
            bail!("BIND_ADDR precisa de pelo menos um endereço");
        }

        let web_origins = std::env::var("WEB_ORIGIN")
            // O `trunk serve --open` pode abrir a página por qualquer um destes.
            .unwrap_or_else(|_| {
                "http://localhost:8080,http://127.0.0.1:8080,http://[::1]:8080".to_owned()
            })
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| HeaderValue::from_str(s).with_context(|| format!("origem inválida: {s}")))
            .collect::<anyhow::Result<Vec<_>>>()?;
        if web_origins.is_empty() {
            bail!("WEB_ORIGIN precisa de pelo menos uma origem");
        }

        let jwt_secret = std::env::var("JWT_SECRET").context("defina JWT_SECRET")?;
        if jwt_secret.len() < 32 {
            bail!("JWT_SECRET precisa ter pelo menos 32 caracteres");
        }

        let cookie_secure = match std::env::var("COOKIE_SECURE").as_deref() {
            Err(_) | Ok("true") => true,
            Ok("false") => false,
            Ok(other) => bail!("COOKIE_SECURE deve ser true ou false, não {other:?}"),
        };

        Ok(Self { database_url, bind_addrs, web_origins, jwt_secret, cookie_secure })
    }

    /// CORS restrito às origens do client. Lembrete: CORS não protege WebSocket;
    /// o handler de WS valida o header `Origin` por conta própria.
    pub fn cors_layer(&self) -> CorsLayer {
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(self.web_origins.clone()))
            .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
            .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
            .allow_credentials(true)
    }
}
