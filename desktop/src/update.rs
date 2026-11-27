//! Atualização automática: o app confere se há uma release nova no GitHub ao abrir e a cada 6 h.
//! Na abertura (fora de uma chamada) instala sozinho; depois, mostra um aviso com "Atualizar".
//! Instalar baixa o instalador, confere a assinatura (minisign) com a chave pública abaixo e o
//! roda em modo passivo, que fecha o app, instala e abre a versão nova.
//!
//! A release publica um `latest.json` (ver `.github/workflows/release.yml`) com a versão, o link
//! do instalador e a assinatura dele.

use cargo_packager_updater::{
    Config, Update, WindowsConfig, WindowsUpdateInstallMode, check_update,
};

/// Chave pública das releases, gerada por `cargo packager signer generate`. A chave privada
/// fica só nos segredos do GitHub Actions. É o arquivo `.pub` inteiro, em base64.
const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEIxMjQ5RTlFNjg3OTBEMUIKUldRYkRYbG9ucDRrc1lzRllrekdmM0lHN3VoaUxUL3dDOHJVZVdZbURXYXNRWWtCeTh4UVdHT1oK";
const ENDPOINT: &str = "https://github.com/ncfcj/call/releases/latest/download/latest.json";

/// A release nova, se houver. Sem rede ou sem release, `None` (só fica no log).
pub async fn check() -> Option<Update> {
    // O build de desenvolvimento (`cargo run`) não se atualiza.
    if cfg!(debug_assertions) {
        return None;
    }
    tokio::task::spawn_blocking(|| {
        let config = Config {
            endpoints: vec![ENDPOINT.parse().ok()?],
            pubkey: PUBKEY.to_owned(),
            windows: Some(WindowsConfig {
                installer_args: None,
                install_mode: Some(WindowsUpdateInstallMode::Passive),
            }),
        };
        let current = env!("CARGO_PKG_VERSION").parse().ok()?;
        check_update(current, config)
            .inspect_err(|err| log::warn!("atualização: não consegui conferir ({err})"))
            .ok()
            .flatten()
    })
    .await
    .ok()
    .flatten()
}

/// Baixa, confere a assinatura e roda o instalador. Se der certo, o processo termina aqui
/// dentro e o instalador abre a versão nova.
pub async fn install(update: Update) -> Result<(), String> {
    tokio::task::spawn_blocking(move || update.download_and_install())
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())
}
