//! As escolhas do usuário que valem entre aberturas do app: microfone, saída e a tecla do
//! Push-to-Talk. Ficam num arquivo de texto em `%APPDATA%\Call\config.txt`, uma `chave=valor`
//! por linha.

use std::path::PathBuf;

use crate::voice::Prefs;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Settings {
    pub prefs: Prefs,
    /// O nome da tecla, como "F8".
    pub ptt: Option<String>,
}

fn path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join("Call").join("config.txt"))
}

/// Sem arquivo (primeira abertura) ou ilegível, volta ao padrão.
pub fn load() -> Settings {
    path().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| parse(&s)).unwrap_or_default()
}

pub fn save(settings: &Settings) {
    let Some(path) = path() else { return };
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, format(settings)));
    if let Err(err) = written {
        log::warn!("configurações: não gravei {} ({err})", path.display());
    }
}

fn parse(text: &str) -> Settings {
    let mut settings = Settings::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = Some(value.trim().to_owned()).filter(|v| !v.is_empty());
        match key.trim() {
            "input" => settings.prefs.input = value,
            "output" => settings.prefs.output = value,
            "ptt" => settings.ptt = value,
            _ => {}
        }
    }
    settings
}

fn format(settings: &Settings) -> String {
    let line =
        |key: &str, value: &Option<String>| format!("{key}={}\n", value.as_deref().unwrap_or(""));
    [
        line("input", &settings.prefs.input),
        line("output", &settings.prefs.output),
        line("ptt", &settings.ptt),
    ]
    .concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_a_round_trip() {
        let settings = Settings {
            prefs: Prefs { input: Some("{0.0.1.00000000}.{6a1b-77}".into()), output: None },
            ptt: Some("F8".into()),
        };
        assert_eq!(parse(&format(&settings)), settings);
    }

    #[test]
    fn unknown_or_broken_lines_are_ignored() {
        let settings = parse("lixo\nvolume=3\nptt=KeyV\r\ninput=\n");
        assert_eq!(settings.ptt.as_deref(), Some("KeyV"));
        assert_eq!(settings.prefs.input, None);
    }
}
