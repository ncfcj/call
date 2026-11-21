use std::time::Duration;

/// Espera exponencial entre tentativas de reconexão: 1 s, 2 s, 4 s ... até 30 s.
///
/// O jitter (de 0 a 25 % a mais) vem de fora, para manter este tipo determinístico nos testes
/// e evitar que todos os clients reconectem no mesmo instante depois de um restart.
#[derive(Debug, Clone)]
pub struct Backoff {
    attempt: u32,
}

impl Backoff {
    pub const BASE: Duration = Duration::from_secs(1);
    pub const MAX: Duration = Duration::from_secs(30);

    pub fn new() -> Self {
        Self { attempt: 0 }
    }

    /// Próxima espera. `jitter` vai de 0.0 a 1.0 (ex.: `Math.random()`).
    pub fn next_delay(&mut self, jitter: f64) -> Duration {
        let exp = Self::BASE.saturating_mul(1 << self.attempt.min(16));
        let base = exp.min(Self::MAX);
        self.attempt = self.attempt.saturating_add(1);
        base.mul_f64(1.0 + 0.25 * jitter.clamp(0.0, 1.0)).min(Self::MAX)
    }

    /// Conectou: a próxima queda volta a esperar só 1 s.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_exponentially_up_to_the_cap() {
        let mut b = Backoff::new();
        let secs: Vec<u64> = (0..8).map(|_| b.next_delay(0.0).as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 16, 30, 30, 30]);
    }

    #[test]
    fn jitter_adds_at_most_a_quarter_and_never_passes_the_cap() {
        let mut b = Backoff::new();
        assert_eq!(b.next_delay(1.0), Duration::from_millis(1250));
        for _ in 0..10 {
            assert!(b.next_delay(1.0) <= Backoff::MAX);
        }
    }

    #[test]
    fn reset_starts_over() {
        let mut b = Backoff::new();
        b.next_delay(0.0);
        b.next_delay(0.0);
        b.reset();
        assert_eq!(b.next_delay(0.0), Duration::from_secs(1));
    }
}
