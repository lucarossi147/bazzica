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
/// A page hidden longer than this was probably suspended by the OS: rebuild, in ms. Shorter
/// hides (switching tabs or apps) keep the transport, so the host keeps its role.
pub const SONNO_MS: u64 = 30_000;
/// A table that had linked peers and has none for this long gets a new transport, in ms.
pub const SENZA_PEER_MS: u64 = 10_000;

/// What the driver knows about the transport at one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FattiRete {
    pub adesso: u64,
    pub ultima_riapertura: u64,
    /// The current message loop ended.
    pub caduta: bool,
    /// How long the page was hidden, on the first tick after it became visible again.
    pub dormito_ms: Option<u64>,
    /// Since when this table, which had linked peers, has none.
    pub senza_peer_dal: Option<u64>,
}

/// Whether to rebuild the transport now, judged by its health: the loop ended, the page was
/// suspended, or every peer link is gone for a while. At most once every [`PAUSA_MS`].
pub fn serve_riaprire(f: &FattiRete) -> bool {
    let motivo = f.caduta
        || f.dormito_ms.is_some_and(|d| d > SONNO_MS)
        || f.senza_peer_dal
            .is_some_and(|t| f.adesso >= t + SENZA_PEER_MS);
    motivo && f.adesso > f.ultima_riapertura + PAUSA_MS
}

/// Tracks since when a table that had linked peers has none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SenzaPeer {
    aveva: bool,
    dal: Option<u64>,
}

impl SenzaPeer {
    /// One tick: how many peers are linked and whether this device holds the replica.
    pub fn osserva(&mut self, adesso: u64, collegati: usize, replica: bool) -> Option<u64> {
        if collegati > 0 {
            *self = SenzaPeer {
                aveva: true,
                dal: None,
            };
        } else if self.aveva && replica {
            self.dal.get_or_insert(adesso);
        }
        self.dal
    }

    /// The transport was rebuilt: start over.
    pub fn riaperta(&mut self) {
        *self = SenzaPeer::default();
    }
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

    fn fatti() -> FattiRete {
        FattiRete {
            adesso: 100_000,
            ..Default::default()
        }
    }

    #[test]
    fn una_breve_assenza_non_riapre() {
        let f = FattiRete {
            dormito_ms: Some(10_000),
            ..fatti()
        };
        assert!(!serve_riaprire(&f), "a tab switch keeps the transport");
        assert!(!serve_riaprire(&fatti()));
    }

    #[test]
    fn riapre_se_il_ciclo_finisce_o_dopo_un_sonno_vero() {
        assert!(serve_riaprire(&FattiRete {
            caduta: true,
            ..fatti()
        }));
        assert!(serve_riaprire(&FattiRete {
            dormito_ms: Some(SONNO_MS + 1),
            ..fatti()
        }));
        assert!(
            !serve_riaprire(&FattiRete {
                caduta: true,
                ultima_riapertura: 99_000,
                ..fatti()
            }),
            "too soon after the last one"
        );
    }

    #[test]
    fn riapre_quando_restano_senza_peer() {
        let mut s = SenzaPeer::default();
        assert_eq!(
            s.osserva(0, 0, true),
            None,
            "never had peers: nothing to restore"
        );
        assert_eq!(s.osserva(1_000, 2, true), None);
        assert_eq!(
            s.osserva(2_000, 0, false),
            None,
            "no replica yet: still joining"
        );
        assert_eq!(s.osserva(3_000, 0, true), Some(3_000));
        assert_eq!(s.osserva(9_000, 0, true), Some(3_000));
        let quando = |adesso, dal| FattiRete {
            adesso,
            senza_peer_dal: dal,
            ..Default::default()
        };
        assert!(!serve_riaprire(&quando(12_000, Some(3_000))));
        assert!(serve_riaprire(&quando(13_000, Some(3_000))));
        s.riaperta();
        assert_eq!(s.osserva(14_000, 0, true), None);
    }
}
