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
    /// LiveKit: URL que os clients usam e as chaves para assinar os tokens de voz.
    pub livekit: crate::voice::LiveKit,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    /// Lê a configuração por uma função de consulta (o ambiente, ou um mapa nos testes).
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let database_url = var("DATABASE_URL").context("defina DATABASE_URL")?;

        let bind_addrs = var("BIND_ADDR")
            .unwrap_or_else(|| "127.0.0.1:3000,[::1]:3000".to_owned())
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

        let web_origins = var("WEB_ORIGIN")
            // O `trunk serve --open` pode abrir a página por qualquer um destes.
            .unwrap_or_else(|| {
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

        let jwt_secret = var("JWT_SECRET").context("defina JWT_SECRET")?;
        if jwt_secret.len() < 32 {
            bail!("JWT_SECRET precisa ter pelo menos 32 caracteres");
        }

        let cookie_secure = match var("COOKIE_SECURE").as_deref() {
            None | Some("true") => true,
            Some("false") => false,
            Some(other) => bail!("COOKIE_SECURE deve ser true ou false, não {other:?}"),
        };

        let livekit = crate::voice::LiveKit::new(
            var("LIVEKIT_URL").context("defina LIVEKIT_URL (ex.: ws://localhost:7880)")?,
            var("LIVEKIT_API_KEY").context("defina LIVEKIT_API_KEY")?,
            var("LIVEKIT_API_SECRET").context("defina LIVEKIT_API_SECRET")?,
        );

        Ok(Self { database_url, bind_addrs, web_origins, jwt_secret, cookie_secure, livekit })
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const SECRET: &str = "segredo-de-teste-com-mais-de-32-bytes";

    fn base() -> HashMap<&'static str, String> {
        HashMap::from([
            ("DATABASE_URL", "postgres://x".to_owned()),
            ("JWT_SECRET", SECRET.to_owned()),
            ("LIVEKIT_URL", "ws://localhost:7880".to_owned()),
            ("LIVEKIT_API_KEY", "devkey".to_owned()),
            ("LIVEKIT_API_SECRET", "secret".to_owned()),
        ])
    }

    fn load(vars: &HashMap<&str, String>) -> anyhow::Result<Config> {
        Config::from_vars(|name| vars.get(name).cloned())
    }

    fn err(vars: &HashMap<&str, String>) -> String {
        format!("{:#}", load(vars).unwrap_err())
    }

    #[test]
    fn defaults_listen_on_both_loopbacks_and_accept_the_dev_origins() {
        let c = load(&base()).unwrap();
        let addrs: Vec<String> = c.bind_addrs.iter().map(ToString::to_string).collect();
        assert_eq!(addrs, ["127.0.0.1:3000", "[::1]:3000"]);
        let origins: Vec<&str> = c.web_origins.iter().map(|o| o.to_str().unwrap()).collect();
        assert_eq!(
            origins,
            ["http://localhost:8080", "http://127.0.0.1:8080", "http://[::1]:8080"]
        );
        assert!(c.cookie_secure, "Secure por padrão");
        assert_eq!(c.livekit.url, "ws://localhost:7880");
        assert_eq!(c.database_url, "postgres://x");
        assert_eq!(c.jwt_secret, SECRET);
    }

    #[test]
    fn lists_are_split_on_commas_and_trimmed() {
        let mut v = base();
        v.insert("BIND_ADDR", " 0.0.0.0:80 , ,[::]:80 ".into());
        v.insert("WEB_ORIGIN", "https://a.dev, https://b.dev".into());
        let c = load(&v).unwrap();
        assert_eq!(c.bind_addrs.len(), 2);
        let origins: Vec<&str> = c.web_origins.iter().map(|o| o.to_str().unwrap()).collect();
        assert_eq!(origins, ["https://a.dev", "https://b.dev"]);
    }

    #[test]
    fn invalid_values_are_rejected_with_a_clear_message() {
        let cases = [
            ("BIND_ADDR", "localhost", "BIND_ADDR inválido"),
            ("BIND_ADDR", " , ", "pelo menos um endereço"),
            ("WEB_ORIGIN", "http://a\nb", "origem inválida"),
            ("WEB_ORIGIN", ",", "pelo menos uma origem"),
            ("JWT_SECRET", "curto", "32 caracteres"),
            ("COOKIE_SECURE", "sim", "true ou false"),
        ];
        for (name, value, expected) in cases {
            let mut v = base();
            v.insert(name, value.into());
            let message = err(&v);
            assert!(message.contains(expected), "{name}={value:?}: {message}");
        }
    }

    #[test]
    fn required_variables_must_exist() {
        for name in
            ["DATABASE_URL", "JWT_SECRET", "LIVEKIT_URL", "LIVEKIT_API_KEY", "LIVEKIT_API_SECRET"]
        {
            let mut v = base();
            v.remove(name);
            assert!(err(&v).contains(name), "faltando {name}");
        }
    }

    #[test]
    fn cookie_secure_can_be_turned_off_for_development() {
        let mut v = base();
        v.insert("COOKIE_SECURE", "false".into());
        assert!(!load(&v).unwrap().cookie_secure);
        v.insert("COOKIE_SECURE", "true".into());
        assert!(load(&v).unwrap().cookie_secure);
    }

    #[test]
    fn secret_of_exactly_32_bytes_is_enough() {
        let mut v = base();
        v.insert("JWT_SECRET", "a".repeat(32));
        assert!(load(&v).is_ok());
        v.insert("JWT_SECRET", "a".repeat(31));
        assert!(load(&v).is_err());
    }

    async fn preflight(c: &Config, origin: &str) -> Option<String> {
        use axum::{Router, body::Body, http::Request, routing::post};
        use tower::ServiceExt;
        let app = Router::new().route("/auth/login", post(|| async {})).layer(c.cors_layer());
        let req = Request::builder()
            .method("OPTIONS")
            .uri("/auth/login")
            .header("Origin", origin)
            .header("Access-Control-Request-Method", "POST")
            .header("Access-Control-Request-Headers", "content-type,authorization")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        let get = |name| res.headers().get(name).map(|v| v.to_str().unwrap().to_owned());
        assert!(
            get("access-control-allow-origin").is_none()
                || get("access-control-allow-credentials").as_deref() == Some("true")
        );
        get("access-control-allow-origin")
    }

    #[tokio::test]
    async fn cors_accepts_only_the_configured_origins_with_credentials() {
        let c = load(&base()).unwrap();
        for origin in ["http://localhost:8080", "http://127.0.0.1:8080", "http://[::1]:8080"] {
            assert_eq!(preflight(&c, origin).await.as_deref(), Some(origin));
        }
        assert_eq!(preflight(&c, "https://site-malicioso.example").await, None);
    }
}
