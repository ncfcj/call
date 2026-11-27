//! Push-to-Talk: uma tecla global, que vale com o Call em segundo plano, e abre o microfone só
//! enquanto está apertada.

use std::time::Duration;

use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey},
};
use iced::{Subscription, futures::SinkExt, keyboard};

pub struct Ptt {
    manager: GlobalHotKeyManager,
    hotkey: HotKey,
    /// O nome da tecla, como "F8" ou "KeyV".
    pub label: String,
    pub held: bool,
}

impl Ptt {
    /// Registra a tecla no Windows. Precisa rodar na thread da janela (o `update` do iced).
    pub fn new(code: Code) -> Result<Self, String> {
        let manager = GlobalHotKeyManager::new().map_err(|err| err.to_string())?;
        let hotkey = HotKey::new(None, code);
        manager
            .register(hotkey)
            .map_err(|_| format!("a tecla {code} já é usada por outro programa"))?;
        Ok(Self { manager, hotkey, label: code.to_string(), held: false })
    }
}

impl Drop for Ptt {
    fn drop(&mut self) {
        let _ = self.manager.unregister(self.hotkey);
    }
}

/// A tecla física de um aperto no iced, no formato da `global-hotkey` (os dois usam os nomes do
/// W3C, como "F8" e "KeyV").
pub fn code_of(event: &keyboard::Event) -> Option<Code> {
    match event {
        keyboard::Event::KeyPressed {
            physical_key: keyboard::key::Physical::Code(code), ..
        } => format!("{code:?}").parse().ok(),
        _ => None,
    }
}

/// Apertou (`true`) e soltou (`false`) a tecla registrada.
pub fn events() -> Subscription<bool> {
    Subscription::run(|| {
        iced::stream::channel(8, async |mut out| {
            // shortcut: confere a fila a cada 20 ms (o iced não acorda sozinho com ela); só roda
            // com o Push-to-Talk ligado.
            loop {
                while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                    if out.send(event.state == HotKeyState::Pressed).await.is_err() {
                        return;
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use iced::keyboard::key::Code as IcedCode;

    use super::*;

    #[test]
    fn iced_key_names_parse_as_hotkey_codes() {
        for (iced, hotkey) in [
            (IcedCode::F8, Code::F8),
            (IcedCode::KeyV, Code::KeyV),
            (IcedCode::Backquote, Code::Backquote),
            (IcedCode::CapsLock, Code::CapsLock),
            (IcedCode::Numpad0, Code::Numpad0),
        ] {
            assert_eq!(format!("{iced:?}").parse::<Code>().ok(), Some(hotkey));
        }
    }
}
