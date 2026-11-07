//! Bazzica in the browser: every device runs the engine; tables are peer-to-peer
//! (WebRTC, introduced through public Nostr relays).

mod carte;
mod connessione;
mod home;
mod partita;
mod profilo;
mod stanza;

use std::collections::BTreeSet;
use std::rc::Rc;
use std::time::Duration;

use bazzica_core::Fase;
use bazzica_net::{IdGiocatore, Richiesta, StatoTavolo, Tavolo, Uscita};
use bazzica_rete::matchbox::PeerId;
use dioxus::prelude::*;

use connessione::{Connessione, ora};
use profilo::Profilo;

fn main() {
    // the network crates log through `log`; show it in the browser console
    let livello = if profilo::leggi("bazzica.debug").is_some() {
        log::Level::Debug
    } else {
        log::Level::Info
    };
    let _ = console_log::init_with_level(livello);
    dioxus::launch(App);
}

/// App-wide state, shared through the context.
#[derive(Clone, Copy)]
pub struct Ctx {
    pub profilo: Signal<Profilo>,
    pub conn: Signal<Option<Rc<Connessione>>>,
    /// Bumped whenever the replica or the set of connected peers changes.
    pub versione: Signal<u64>,
    pub avviso: Signal<Option<(String, u64)>>,
    /// Coarse clock (4 Hz) for countdowns.
    pub ora: Signal<u64>,
    /// Key of the turn timer currently running and when this device saw it start.
    pub turno: Signal<(String, u64)>,
}

/// What the screens render: a copy of the replica plus connection facts.
#[derive(Clone, PartialEq)]
pub struct Vista {
    pub tavolo: Tavolo,
    pub io: IdGiocatore,
    pub host: Option<IdGiocatore>,
    pub connessi: BTreeSet<IdGiocatore>,
    pub disconnessi: Vec<IdGiocatore>,
}

impl Vista {
    pub fn nome(&self, id: &str) -> String {
        self.tavolo
            .partecipante(id)
            .map_or_else(|| "?".into(), |p| p.nome.clone())
    }
}

impl Ctx {
    pub fn vista(&self) -> Option<Vista> {
        let _ = self.versione.read();
        let c = self.conn.read().clone()?;
        let s = c.sessione.borrow();
        let n = s.nodo();
        Some(Vista {
            tavolo: n.tavolo()?.clone(),
            io: n.io().id.clone(),
            host: n.host().cloned(),
            connessi: n.connessi(),
            disconnessi: n.disconnessi(),
        })
    }

    pub fn richiedi(&self, r: Richiesta) {
        let Some(c) = self.conn.peek().clone() else {
            return;
        };
        let uscite = c.richiedi(r);
        self.gestisci(&c, uscite);
    }

    pub fn avvisa(&self, testo: impl Into<String>) {
        let mut a = self.avviso;
        a.set(Some((testo.into(), ora())));
    }

    fn gestisci(&self, c: &Connessione, uscite: Vec<Uscita<PeerId>>) {
        let mut cambiato = false;
        for u in uscite {
            match u {
                Uscita::Applicata { .. } | Uscita::Sincronizzata => cambiato = true,
                Uscita::Rifiutata { motivo } => self.avvisa(motivo),
                _ => {}
            }
        }
        if cambiato {
            if let Some(t) = c.sessione.borrow().nodo().tavolo() {
                let chiave = chiave_turno(t);
                if self.turno.peek().0 != chiave {
                    let mut turno = self.turno;
                    turno.set((chiave, ora()));
                }
            }
            let mut v = self.versione;
            v += 1;
        }
    }

    pub fn esci(&self) {
        let mut conn = self.conn;
        conn.set(None);
        imposta_codice_url("");
    }
}

/// Identifies the running turn timer, to estimate the countdown on every device.
fn chiave_turno(t: &Tavolo) -> String {
    match (&t.stato, &t.partita) {
        (StatoTavolo::InCorso, Some(st)) => match st.fase {
            Fase::Scambio => format!("s{}-{}", st.mano_n, st.turno),
            Fase::Gioco => format!(
                "g{}-{}-{}-{}",
                st.mano_n,
                st.turno,
                st.giocate_mano.len(),
                st.corrente
            ),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// Table code from the URL fragment (`…/bazzica/#ABCD2345`).
pub fn codice_da_url() -> Option<String> {
    let h = web_sys::window()?.location().hash().ok()?;
    let c = bazzica_rete::nostr::normalizza_codice(h.trim_start_matches('#'));
    (c.len() >= 4).then_some(c)
}

pub fn imposta_codice_url(codice: &str) {
    if let Some(w) = web_sys::window() {
        let _ = w.location().set_hash(codice);
    }
}

pub fn link_tavolo(codice: &str) -> String {
    let base = web_sys::window()
        .and_then(|w| {
            let l = w.location();
            Some(format!("{}{}", l.origin().ok()?, l.pathname().ok()?))
        })
        .unwrap_or_default();
    format!("{base}#{codice}")
}

fn documento_nascosto() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .is_some_and(|d| d.hidden())
}

#[component]
fn App() -> Element {
    let ctx = use_context_provider(|| Ctx {
        profilo: Signal::new(Profilo::carica()),
        conn: Signal::new(None),
        versione: Signal::new(0),
        avviso: Signal::new(None),
        ora: Signal::new(ora()),
        turno: Signal::new((String::new(), ora())),
    });

    // drive the network: ~20 times a second
    use_future(move || async move {
        let mut nascosto_da: Option<u64> = None;
        let mut ultima_riapertura = 0u64;
        let mut connessi = BTreeSet::new();
        loop {
            futures_timer::Delay::new(Duration::from_millis(50)).await;
            let adesso = ora();
            if adesso / 250 != *ctx.ora.peek() / 250 {
                let mut o = ctx.ora;
                o.set(adesso);
            }
            if let Some((_, t)) = *ctx.avviso.peek()
                && adesso > t + 4000
            {
                let mut a = ctx.avviso;
                a.set(None);
            }
            let Some(c) = ctx.conn.peek().clone() else {
                continue;
            };
            // phones suspend pages: rebuild the connection after a nap or when it died
            if documento_nascosto() {
                nascosto_da.get_or_insert(adesso);
                continue;
            }
            let dormito = nascosto_da.take().is_some_and(|t| adesso > t + 3000);
            if (dormito || *c.caduta.borrow()) && adesso > ultima_riapertura + 5000 {
                ultima_riapertura = adesso;
                c.riapri();
            }
            let uscite = c.aggiorna();
            ctx.gestisci(&c, uscite);
            let ora_connessi = c.sessione.borrow().nodo().connessi();
            if ora_connessi != connessi {
                connessi = ora_connessi;
                let mut v = ctx.versione;
                v += 1;
            }
        }
    });

    let conn = ctx.conn.read().clone();
    let vista = ctx.vista();
    rsx! {
        document::Stylesheet { href: asset!("/assets/style.css") }
        document::Meta { name: "theme-color", content: "#134a27" }
        match (conn, vista) {
            (None, _) => rsx! { home::Home {} },
            (Some(c), None) => rsx! { Attesa { codice: c.codice.clone() } },
            (Some(_), Some(v)) => rsx! { stanza::Tavolo { vista: v } },
        }
        if let Some((testo, _)) = ctx.avviso.read().clone() {
            div { class: "avviso", onclick: move |_| { let mut a = ctx.avviso; a.set(None) }, "{testo}" }
        }
    }
}

#[component]
fn Attesa(codice: String) -> Element {
    let ctx = use_context::<Ctx>();
    rsx! {
        div { class: "schermo centrato",
            h1 { class: "logo", "Bazzica" }
            div { class: "pannello",
                p { "Collegamento al tavolo " b { "{codice}" } "…" }
                p { class: "nota", "Serve che almeno un altro giocatore del tavolo sia collegato." }
                button { class: "secondario", onclick: move |_| ctx.esci(), "Annulla" }
            }
        }
    }
}
