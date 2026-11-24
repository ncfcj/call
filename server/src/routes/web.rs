//! O servidor entregando o client compilado, na mesma origem da API.

use std::path::Path;

use anyhow::Context;
use axum::{
    Router,
    http::{HeaderName, HeaderValue, header},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};

fn set(name: HeaderName, value: HeaderValue) -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(name, value)
}

/// Serve o client compilado em `dir` no que não é rota da API, com os cabeçalhos de segurança em
/// todas as respostas. O navegador sempre confere se mudou (304), porque o `index.html` não leva
/// hash no nome.
pub fn serve_web(router: Router, dir: &str, livekit_url: &str) -> anyhow::Result<Router> {
    let index = std::fs::read_to_string(Path::new(dir).join("index.html"))
        .with_context(|| format!("WEB_DIR sem index.html: {dir}"))?;
    let csp = csp(&index, livekit_url).parse().context("LIVEKIT_URL inválida para a CSP")?;

    let files = Router::new()
        .fallback_service(ServeDir::new(dir).precompressed_gzip())
        .layer(set(header::CACHE_CONTROL, HeaderValue::from_static("no-cache")));
    Ok(router
        .fallback_service(files)
        .layer(set(header::CONTENT_SECURITY_POLICY, csp))
        .layer(set(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(set(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"))))
}

/// Só roda script do próprio site e o inline que o Trunk gera (pelo hash, que muda a cada build).
fn csp(index: &str, livekit_url: &str) -> String {
    let mut scripts = String::new();
    // O navegador normaliza as quebras de linha antes de calcular o hash.
    for script in index.replace("\r\n", "\n").split("<script").skip(1) {
        if let Some((attrs, rest)) = script.split_once('>')
            && !attrs.contains("src=")
            && let Some((code, _)) = rest.split_once("</script>")
        {
            scripts += &format!(" 'sha256-{}'", STANDARD.encode(Sha256::digest(code)));
        }
    }
    // O SDK do LiveKit usa o WebSocket e também HTTP no mesmo host.
    let mut livekit = format!("{livekit_url} {}", livekit_url.replacen("ws", "http", 1));
    if livekit_url.trim_end_matches('/').ends_with(".livekit.cloud") {
        // O LiveKit Cloud manda o client para o host da região mais próxima.
        livekit += " wss://*.livekit.cloud https://*.livekit.cloud";
    }
    format!(
        "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'{scripts}; \
         connect-src 'self' {livekit}; object-src 'none'; base-uri 'none'; form-action 'self'; \
         frame-ancestors 'none'"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csp_allows_only_the_inline_scripts_of_the_page() {
        let csp = csp(
            "<script src=\"/a.js\"></script><script type=\"module\">\r\nabc</script>",
            "ws://localhost:7880",
        );
        // printf '\nabc' | openssl dgst -sha256 -binary | base64
        let hash = "'sha256-8505v8mSh7hIeA1BEOtmPw58NErkH0ajkMQTW4ncJ1Y='";
        assert_eq!(
            csp,
            format!(
                "default-src 'self'; script-src 'self' 'wasm-unsafe-eval' {hash}; \
                 connect-src 'self' ws://localhost:7880 http://localhost:7880; \
                 object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'"
            )
        );
    }

    #[test]
    fn livekit_cloud_may_redirect_to_a_regional_host() {
        let csp = csp("", "wss://call-x.livekit.cloud/");
        assert!(csp.contains(
            "connect-src 'self' wss://call-x.livekit.cloud/ https://call-x.livekit.cloud/ \
             wss://*.livekit.cloud https://*.livekit.cloud;"
        ));
        assert!(!super::csp("", "wss://livekit.cloud.example").contains('*'));
    }
}
