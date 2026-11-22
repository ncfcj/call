/// Mute e deaf, com as regras do Discord.
///
/// - **Mute** fecha só o seu microfone.
/// - **Deaf** silencia os outros para você e também fecha o seu microfone, mas lembra se você
///   estava mutado antes: ao sair do deaf, o microfone volta a como estava.
/// - Desmutar estando em deaf sai do deaf também (não faz sentido falar sem ouvir).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VoiceControls {
    pub muted: bool,
    pub deafened: bool,
}

impl VoiceControls {
    /// O microfone deve estar aberto?
    pub fn mic_open(self) -> bool {
        !self.muted && !self.deafened
    }

    /// O que a UI mostra no botão do microfone: fechado por mute ou por deaf.
    pub fn mic_shown_muted(self) -> bool {
        !self.mic_open()
    }

    pub fn toggle_mute(&mut self) {
        if self.deafened {
            self.deafened = false;
            self.muted = false;
        } else {
            self.muted = !self.muted;
        }
    }

    pub fn toggle_deaf(&mut self) {
        self.deafened = !self.deafened;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mute_closes_only_the_mic() {
        let mut c = VoiceControls::default();
        assert!(c.mic_open());
        c.toggle_mute();
        assert_eq!(c, VoiceControls { muted: true, deafened: false });
        assert!(!c.mic_open());
        c.toggle_mute();
        assert!(c.mic_open());
    }

    #[test]
    fn deaf_closes_the_mic_and_restores_it() {
        let mut c = VoiceControls::default();
        c.toggle_deaf();
        assert!(!c.mic_open(), "deaf também fecha o microfone");
        c.toggle_deaf();
        assert!(c.mic_open(), "saindo do deaf o microfone volta a abrir");

        c.toggle_mute();
        c.toggle_deaf();
        c.toggle_deaf();
        assert!(!c.mic_open(), "quem estava mutado continua mutado depois do deaf");
    }

    #[test]
    fn unmuting_while_deaf_leaves_deaf() {
        let mut c = VoiceControls { muted: true, deafened: true };
        c.toggle_mute();
        assert_eq!(c, VoiceControls::default());
        assert!(c.mic_open());
    }
}
