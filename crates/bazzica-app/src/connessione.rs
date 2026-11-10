//! The live table session: WebRTC socket + protocol node, pumped by a timer loop.

use std::cell::RefCell;
use std::rc::Rc;

use bazzica_net::{Config, Effimero, Errore, Impostazioni, Nodo, Richiesta, Uscita};
use bazzica_rete::matchbox::PeerId;
use bazzica_rete::nostr::casuale;
use bazzica_rete::{NostrSignallerBuilder, Sessione, Turn};
use dioxus::core::spawn_forever;

use crate::profilo::{Profilo, leggi, scrivi};
use crate::riapertura::Cicli;

const CHIAVE_TURN: &str = "bazzica.turn";

/// The TURN relay this device uses: its own setting, else the one built into the app.
pub fn turn_attivo() -> Option<Turn> {
    turn_salvato().or_else(Turn::di_compilazione)
}

/// The TURN relay set on this device (not the built-in one).
pub fn turn_salvato() -> Option<Turn> {
    leggi(CHIAVE_TURN).and_then(|s| serde_json::from_str(&s).ok())
}

pub fn salva_turn(turn: Option<&Turn>) {
    scrivi(
        CHIAVE_TURN,
        &turn.map_or_else(String::new, |t| {
            serde_json::to_string(t).unwrap_or_default()
        }),
    );
}

/// Milliseconds since the epoch (the node only needs a monotonic-enough clock).
pub fn ora() -> u64 {
    js_sys::Date::now() as u64
}

pub fn seme() -> u64 {
    u64::from_le_bytes(casuale::<8>())
}

pub struct Connessione {
    pub codice: String,
    pub sessione: RefCell<Sessione>,
    /// The message loops; the current one ending (signaling lost, no peers) means reopen.
    pub cicli: Rc<RefCell<Cicli>>,
    /// When the current connection attempt started (ms), for the diagnostics.
    pub aperta: RefCell<u64>,
}

impl Connessione {
    pub fn crea(profilo: &Profilo, codice: &str) -> Rc<Connessione> {
        let imp = Impostazioni {
            nome: format!("Tavolo di {}", profilo.nome),
            ..Default::default()
        };
        let nodo = Nodo::crea(profilo.partecipante(), imp, seme(), Config::default());
        Self::apri(codice, nodo)
    }

    pub fn unisciti(profilo: &Profilo, codice: &str) -> Rc<Connessione> {
        let nodo = Nodo::unisciti(profilo.partecipante(), seme(), Config::default());
        Self::apri(codice, nodo)
    }

    fn apri(codice: &str, nodo: Nodo<PeerId>) -> Rc<Connessione> {
        let (sessione, ciclo) = Sessione::apri(
            codice,
            NostrSignallerBuilder::default(),
            nodo,
            turn_attivo(),
        );
        let c = Rc::new(Connessione {
            codice: codice.to_owned(),
            sessione: RefCell::new(sessione),
            cicli: Rc::default(),
            aperta: RefCell::new(ora()),
        });
        c.avvia_ciclo(ciclo);
        c
    }

    fn avvia_ciclo(&self, ciclo: bazzica_rete::matchbox::MessageLoopFuture) {
        let cicli = self.cicli.clone();
        let mia = cicli.borrow_mut().avvia();
        log::info!(
            "apertura della connessione al tavolo {} (generazione {mia})",
            self.codice
        );
        // not tied to the component that asked (it may unmount while the loop must go on)
        spawn_forever(async move {
            if let Err(e) = ciclo.await {
                log::warn!("connessione caduta (generazione {mia}): {e}");
            }
            // a loop replaced by `riapri` also ends here: only the current one counts
            cicli.borrow_mut().finito(mia);
        });
    }

    /// Leaving the table: the signaller says goodbye, so the others see us gone at once.
    /// The connection is kept a few seconds for that, then dropped.
    pub fn chiudi(self: Rc<Self>) {
        self.sessione.borrow_mut().chiudi();
        spawn_forever(async move {
            futures_timer::Delay::new(std::time::Duration::from_secs(3)).await;
            drop(self);
        });
    }

    /// Takes the table back from another tab of the same identity.
    pub fn riprendi(&self) {
        self.sessione.borrow_mut().riprendi(ora());
        self.riapri();
    }

    /// Rebuilds the transport, keeping the replica (after sleep or a lost signaling link).
    pub fn riapri(&self) {
        self.riapri_con(turn_attivo());
    }

    /// Rebuilds the transport with another TURN relay (after the settings changed).
    pub fn riapri_con(&self, turn: Option<Turn>) {
        let ciclo = self.sessione.borrow_mut().riapri(
            ora(),
            &self.codice,
            NostrSignallerBuilder::default(),
            turn,
        );
        *self.aperta.borrow_mut() = ora();
        self.avvia_ciclo(ciclo);
    }

    pub fn aggiorna(&self) -> Vec<Uscita<PeerId>> {
        self.sessione.borrow_mut().aggiorna(ora())
    }

    pub fn richiedi(&self, r: Richiesta) -> Vec<Uscita<PeerId>> {
        self.sessione.borrow_mut().richiedi(ora(), r)
    }

    pub fn effimero(&self, e: Effimero) -> Result<Vec<Uscita<PeerId>>, Errore> {
        self.sessione.borrow_mut().effimero(ora(), e)
    }
}
