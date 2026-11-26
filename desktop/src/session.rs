//! O cookie de refresh entre uma abertura do app e outra, no Cofre de Credenciais do Windows.
//! Fora do Windows não há onde guardar, e a sessão dura só enquanto o app está aberto.

/// Uma entrada por API, para a sessão local e a de produção não se misturarem.
#[cfg(windows)]
fn entry(api_url: &str) -> Option<keyring::Entry> {
    keyring::Entry::new("Call", api_url).ok()
}

pub fn load(api_url: &str) -> Option<String> {
    #[cfg(windows)]
    return entry(api_url)?.get_password().ok();
    #[cfg(not(windows))]
    {
        let _ = api_url;
        None
    }
}

/// Guarda o cookie, ou apaga a entrada com `None`. Uma falha do cofre só faz a sessão não
/// sobreviver a fechar o app, então é ignorada.
pub fn save(api_url: &str, cookie: Option<&str>) {
    #[cfg(windows)]
    if let Some(entry) = entry(api_url) {
        let _ = match cookie {
            Some(cookie) => entry.set_password(cookie),
            None => entry.delete_credential(),
        };
    }
    #[cfg(not(windows))]
    let _ = (api_url, cookie);
}
