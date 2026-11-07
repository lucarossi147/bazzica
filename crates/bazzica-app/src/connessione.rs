//! The live table session: WebRTC socket + protocol node, pumped by a timer loop.

use std::cell::RefCell;
use std::rc::Rc;

use bazzica_net::{Config, Impostazioni, Nodo, Richiesta, Uscita};
use bazzica_rete::matchbox::PeerId;
use bazzica_rete::nostr::casuale;
use bazzica_rete::{NostrSignallerBuilder, Sessione};
use dioxus::core::spawn_forever;

use crate::profilo::Profilo;

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
    /// Set when the message loop ended (signaling lost and no peers): time to reopen.
    pub caduta: Rc<RefCell<bool>>,
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
        let (sessione, ciclo) = Sessione::apri(codice, NostrSignallerBuilder::default(), nodo);
        let c = Rc::new(Connessione {
            codice: codice.to_owned(),
            sessione: RefCell::new(sessione),
            caduta: Rc::new(RefCell::new(false)),
        });
        c.avvia_ciclo(ciclo);
        c
    }

    fn avvia_ciclo(&self, ciclo: bazzica_rete::matchbox::MessageLoopFuture) {
        let caduta = self.caduta.clone();
        *caduta.borrow_mut() = false;
        log::info!("apertura della connessione al tavolo {}", self.codice);
        // not tied to the component that asked (it may unmount while the loop must go on)
        spawn_forever(async move {
            if let Err(e) = ciclo.await {
                log::warn!("connessione caduta: {e}");
            }
            *caduta.borrow_mut() = true;
        });
    }

    /// Rebuilds the transport, keeping the replica (after sleep or a lost signaling link).
    pub fn riapri(&self) {
        let ciclo = self.sessione.borrow_mut().riapri(
            ora(),
            &self.codice,
            NostrSignallerBuilder::default(),
        );
        self.avvia_ciclo(ciclo);
    }

    pub fn aggiorna(&self) -> Vec<Uscita<PeerId>> {
        self.sessione.borrow_mut().aggiorna(ora())
    }

    pub fn richiedi(&self, r: Richiesta) -> Vec<Uscita<PeerId>> {
        self.sessione.borrow_mut().richiedi(ora(), r)
    }
}
