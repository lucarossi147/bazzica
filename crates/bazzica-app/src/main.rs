//! Bazzica in the browser: every device runs the engine; tables are peer-to-peer
//! (WebRTC, introduced through public Nostr relays).

mod carte;
mod collegamento;
mod connessione;
mod fx;
mod guida;
mod home;
mod partita;
mod profilo;
mod riapertura;
mod stanza;

use std::collections::BTreeSet;
use std::rc::Rc;
use std::time::Duration;

use bazzica_core::{Evento, Fase, Seat};
use bazzica_net::tavolo::FRASI;
use bazzica_net::{Effimero, IdGiocatore, Richiesta, StatoTavolo, Tavolo, Uscita, Voce};
use bazzica_rete::matchbox::PeerId;
use dioxus::prelude::*;
use serde_json::json;

use carte::Mazzo;
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
    /// Card deck shown on this device.
    pub mazzo: Signal<Mazzo>,
    pub muto: Signal<bool>,
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

/// Shown when this device follows no host: the host left and this side of the network cannot
/// elect one (no majority), or nobody else is here.
pub const SENZA_REGIA: &str =
    "Non riesco a raggiungere la regia: aspetto che si ricolleghi qualcuno";

impl Vista {
    /// Whether this user may make request `r` (the same rule the host applies).
    pub fn puo(&self, r: &Richiesta) -> bool {
        let creatore_presente = self.connessi.contains(self.tavolo.creatore());
        bazzica_net::tavolo::permesso(
            &self.tavolo,
            &self.io,
            r,
            creatore_presente,
            self.host.as_deref(),
        )
        .is_ok()
    }

    pub fn nome(&self, id: &str) -> String {
        self.tavolo
            .partecipante(id)
            .map_or_else(|| "?".into(), |p| p.nome.clone())
    }
}

impl Ctx {
    /// Whether another tab of this identity took the table over.
    pub fn sostituita(&self) -> bool {
        let _ = self.versione.read();
        self.conn
            .read()
            .as_ref()
            .is_some_and(|c| c.sessione.borrow().nodo().sostituito())
    }

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

    /// A quick phrase or a card drag (cosmetic, sent straight to everyone), rate-limited.
    pub fn effimero(&self, e: Effimero) {
        thread_local! {
            static ULTIMO: std::cell::Cell<[u64; 2]> = const { std::cell::Cell::new([0; 2]) };
        }
        let (i, pausa) = match e {
            Effimero::Frase { .. } => (0, 3000),
            Effimero::Striscia { .. } => (1, 1000),
        };
        let adesso = ora();
        let mut ultimo = ULTIMO.get();
        if adesso < ultimo[i] + pausa {
            return;
        }
        ultimo[i] = adesso;
        ULTIMO.set(ultimo);
        let Some(c) = self.conn.peek().clone() else {
            return;
        };
        match c.effimero(e) {
            Ok(uscite) => self.gestisci(&c, uscite),
            Err(err) => self.avvisa(err.0),
        }
    }

    pub fn avvisa(&self, testo: impl Into<String>) {
        let mut a = self.avviso;
        a.set(Some((testo.into(), ora())));
    }

    fn gestisci(&self, c: &Connessione, uscite: Vec<Uscita<PeerId>>) {
        let mut cambiato = false;
        let mut effetti: Vec<serde_json::Value> = vec![];
        for u in uscite {
            match u {
                Uscita::Applicata { voce, eventi } => {
                    cambiato = true;
                    effetti.extend(eventi_visibili(c, &voce, &eventi));
                }
                Uscita::Sincronizzata | Uscita::Sostituita => cambiato = true,
                Uscita::Rifiutata { motivo } => self.avvisa(motivo),
                Uscita::Effimero { effetto, .. } => {
                    effetti.push(effimero_json(&effetto));
                }
                Uscita::Invia { .. } => {}
            }
        }
        if !effetti.is_empty() {
            // remember where everything is before the new state is drawn
            fx::istantanea();
        }
        if cambiato {
            if let Some(t) = c.sessione.borrow().nodo().tavolo() {
                let chiave = chiave_turno(t);
                if self.turno.peek().0 != chiave {
                    let mut turno = self.turno;
                    turno.set((chiave, ora()));
                    if tocca_a_me(t, &self.profilo.peek().id) {
                        fx::turno();
                    }
                }
            }
            let mut v = self.versione;
            v += 1;
        }
        if !effetti.is_empty() {
            let contesto = contesto_fx(c, (self.mazzo)());
            fx::eventi(
                &serde_json::Value::Array(effetti).to_string(),
                &contesto.to_string(),
            );
        }
    }

    pub fn esci(&self) {
        if let Some(c) = self.conn.peek().clone() {
            c.chiudi();
        }
        let mut conn = self.conn;
        conn.set(None);
        imposta_codice_url("");
    }
}

/// Seat this device shows the game from: the one that must act now, else its first seat.
pub fn posto_vista(t: &Tavolo, io: &str) -> Seat {
    let miei = t.posti_di(io);
    let st = t.partita.as_ref();
    miei.iter()
        .copied()
        .find(|s| st.is_some_and(|st| st.required_actors().contains(s)))
        .or(miei.first().copied())
        .unwrap_or(0)
}

fn tocca_a_me(t: &Tavolo, io: &str) -> bool {
    match (&t.stato, &t.partita) {
        (StatoTavolo::InCorso, Some(st)) => {
            st.fase == Fase::Gioco && t.posti_di(io).contains(&st.corrente)
        }
        _ => false,
    }
}

/// Seconds left on my turn, when it is my turn and the play timer is on.
fn secondi_rimasti(c: &Connessione, ctx: Ctx, adesso: u64) -> Option<u64> {
    let s = c.sessione.borrow();
    let n = s.nodo();
    let t = n.tavolo()?;
    let durata = u64::from(t.impostazioni.timer_giocata);
    if durata == 0 || n.in_pausa() || !tocca_a_me(t, &n.io().id) {
        return None;
    }
    let inizio = ctx.turno.peek().1;
    Some(durata.saturating_sub(adesso.saturating_sub(inizio) / 1000))
}

/// Engine events this device may see (private ones only for its own seats), as JSON.
fn eventi_visibili(c: &Connessione, voce: &Voce, eventi: &[Evento]) -> Vec<serde_json::Value> {
    let s = c.sessione.borrow();
    let n = s.nodo();
    let Some(t) = n.tavolo() else {
        return vec![];
    };
    let miei = t.posti_di(&n.io().id);
    let mut out = vec![];
    if let Voce::Scaduto { timer, seat, .. } = voce {
        out.push(json!({"tipo": "tempo_scaduto", "fase": timer, "seat": seat}));
    }
    for e in eventi {
        let visibile = e
            .visibile
            .as_ref()
            .is_none_or(|v| v.iter().any(|s| miei.contains(s)));
        if visibile && let Ok(v) = serde_json::to_value(e) {
            out.push(v);
        }
    }
    out
}

fn effimero_json(e: &Effimero) -> serde_json::Value {
    match e {
        Effimero::Frase { seat, indice } => json!({
            "tipo": "frase",
            "seat": seat,
            "testo": FRASI.get(usize::from(*indice)).copied().unwrap_or(""),
        }),
        Effimero::Striscia { seat, verso } => {
            json!({"tipo": "striscia", "seat": seat, "verso": verso})
        }
    }
}

/// What the effects need to know about the table and the viewer.
fn contesto_fx(c: &Connessione, mazzo: Mazzo) -> serde_json::Value {
    let s = c.sessione.borrow();
    let n = s.nodo();
    let Some(t) = n.tavolo() else {
        return json!({});
    };
    let io = &n.io().id;
    json!({
        "io": posto_vista(t, io),
        "miei": t.posti_di(io),
        "nomi": (0..4u8).map(|s| t.nome_posto(s)).collect::<Vec<_>>(),
        "squadre": t.impostazioni.squadre,
        "mazzo": mazzo.chiave(),
    })
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
        mazzo: Signal::new(Mazzo::carica()),
        muto: Signal::new(fx::muto()),
    });

    // drive the network: ~20 times a second
    use_future(move || async move {
        let mut nascosto_da: Option<u64> = None;
        let mut ultima_riapertura = 0u64;
        let mut senza_peer = riapertura::SenzaPeer::default();
        let mut connessi = BTreeSet::new();
        let mut ultimo_tic = 0u64;
        loop {
            futures_timer::Delay::new(Duration::from_millis(50)).await;
            let adesso = ora();
            // a hidden page keeps driving the node (the host must keep the game going;
            // browsers throttle this loop to about once a second), but skips UI-only work
            let nascosto = documento_nascosto();
            if !nascosto && adesso / 250 != *ctx.ora.peek() / 250 {
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
            // rebuild the transport when it is unhealthy, not merely because the page hid
            let dormito_ms = if nascosto {
                nascosto_da.get_or_insert(adesso);
                None
            } else {
                nascosto_da.take().map(|t| adesso - t)
            };
            let senza_peer_dal = {
                let s = c.sessione.borrow();
                senza_peer.osserva(adesso, s.peer_collegati(), s.nodo().tavolo().is_some())
            };
            let fatti = riapertura::FattiRete {
                adesso,
                ultima_riapertura,
                caduta: c.cicli.borrow().caduta(),
                dormito_ms,
                senza_peer_dal,
            };
            if riapertura::serve_riaprire(&fatti) {
                ultima_riapertura = adesso;
                senza_peer.riaperta();
                c.riapri();
            }
            let uscite = c.aggiorna();
            ctx.gestisci(&c, uscite);
            // last five seconds of my turn: tick
            if !nascosto
                && let Some(rimasti) = secondi_rimasti(&c, ctx, adesso)
                && (1..=5).contains(&rimasti)
                && rimasti != ultimo_tic
            {
                ultimo_tic = rimasti;
                fx::tic();
            }
            let ora_connessi = c.sessione.borrow().nodo().connessi();
            if ora_connessi != connessi {
                connessi = ora_connessi;
                let mut v = ctx.versione;
                v += 1;
            }
        }
    });

    let sostituita = ctx.sostituita();
    let conn = ctx.conn.read().clone();
    let vista = ctx.vista();
    rsx! {
        document::Stylesheet { href: asset!("/assets/style.css") }
        match (conn, vista) {
            (Some(_), _) if sostituita => rsx! { Sostituita {} },
            (None, _) => rsx! { home::Home {} },
            (Some(c), None) => rsx! { collegamento::Attesa { codice: c.codice.clone() } },
            (Some(_), Some(v)) => rsx! { stanza::Tavolo { vista: v } },
        }
        if let Some((testo, _)) = ctx.avviso.read().clone() {
            div { class: "avviso", onclick: move |_| { let mut a = ctx.avviso; a.set(None) }, "{testo}" }
        }
    }
}

/// This tab was superseded by another tab or page of the same player.
#[component]
fn Sostituita() -> Element {
    let ctx = use_context::<Ctx>();
    rsx! {
        div { class: "schermo centrato",
            h1 { class: "logo", "Bazzica" }
            div { class: "pannello",
                p { "Questa partita è aperta in un'altra scheda." }
                button {
                    onclick: move |_| {
                        if let Some(c) = ctx.conn.peek().clone() {
                            c.riprendi();
                        }
                    },
                    "Gioca qui"
                }
                button { class: "secondario", onclick: move |_| ctx.esci(), "Esci" }
            }
        }
    }
}
