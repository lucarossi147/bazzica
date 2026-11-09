//! When the browser driver rebuilds the WebRTC transport. Pure decisions, tested natively.

/// The message loops of one table: each transport rebuild starts a new generation, and only
/// the current one may report that the connection dropped (an old loop also ends, after its
/// socket was replaced, and must not trigger another rebuild).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cicli {
    generazione: u64,
    caduta: bool,
}

impl Cicli {
    /// A new loop starts: returns its generation.
    pub fn avvia(&mut self) -> u64 {
        self.generazione += 1;
        self.caduta = false;
        self.generazione
    }

    /// The loop of generation `mia` ended.
    pub fn finito(&mut self, mia: u64) {
        if mia == self.generazione {
            self.caduta = true;
        }
    }

    /// The current loop ended: time to rebuild the transport.
    pub fn caduta(&self) -> bool {
        self.caduta
    }

    pub fn generazione(&self) -> u64 {
        self.generazione
    }
}

/// Minimum time between two rebuilds, in ms.
pub const PAUSA_MS: u64 = 5_000;
/// A page hidden longer than this was probably suspended (phones): rebuild, in ms.
pub const SONNO_MS: u64 = 3_000;

/// Whether to rebuild the transport now: its loop ended or the page slept, at most once
/// every [`PAUSA_MS`].
pub fn deve_riaprire(caduta: bool, adesso: u64, ultima: u64, dormito_ms: Option<u64>) -> bool {
    let sonno = dormito_ms.is_some_and(|d| d > SONNO_MS);
    (caduta || sonno) && adesso > ultima + PAUSA_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_ciclo_sostituito_non_segnala_la_caduta() {
        let mut c = Cicli::default();
        let prima = c.avvia();
        let seconda = c.avvia();
        c.finito(prima);
        assert!(!c.caduta(), "the replaced loop ended: not a drop");
        c.finito(seconda);
        assert!(c.caduta());
        c.avvia();
        assert!(!c.caduta(), "a new loop starts clean");
        assert_eq!(c.generazione(), 3);
    }

    #[test]
    fn riapre_dopo_la_pausa() {
        assert!(!deve_riaprire(false, 100_000, 0, None));
        assert!(deve_riaprire(true, 100_000, 0, None));
        assert!(
            !deve_riaprire(true, 4_000, 0, None),
            "too soon after the last one"
        );
        assert!(deve_riaprire(false, 100_000, 0, Some(SONNO_MS + 1)));
        assert!(!deve_riaprire(false, 100_000, 0, Some(SONNO_MS)));
    }
}
