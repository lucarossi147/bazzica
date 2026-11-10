//! A matchbox [`Signaller`] that uses public Nostr relays instead of a signaling server.
//!
//! Every peer publishes encrypted envelopes ([`Busta`]) to all relays and listens on all of
//! them. Who offers to whom, and how failed pairs retry, is decided by [`Incontri`].
//!
//! Only live events count: what a relay replays from storage (before its end-of-stored-events
//! marker) comes from peers that may be long gone, so it is ignored. Clocks are not compared
//! for that, so peers whose clocks disagree still meet ([`ETA_MASSIMA`] only drops absurd
//! timestamps, and counts them for the "wrong clock" hint).
//!
//! The session tells the signaller about WebRTC links that opened or closed, and when it must
//! say goodbye, through [`Notizie`]; the signaller looks at them at least once a second.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use futures::stream::SelectAll;
use futures::{FutureExt, SinkExt, StreamExt};
use matchbox_socket::async_trait::async_trait;
use matchbox_socket::{
    PeerEvent, PeerId, PeerRequest, SignalingError, Signaller, SignallerBuilder,
};
use serde_json::{Value, json};
use sync_wrapper::SyncWrapper;

use crate::ice::{DiagnosiCondivisa, aggiorna};
use crate::incontri::{Azione, Busta, Incontri};
use crate::nostr::{Chiavi, KIND_SEGNALE, Stanza, casuale};
use crate::ws;

/// Public relays tried by default (any that accept connections are used). Every envelope goes
/// to all of them, so one working relay shared by two peers is enough: keep the list long,
/// since public relays come and go, rate-limit, or accept connections yet drop events.
pub const RELAY_PREDEFINITI: [&str; 7] = [
    "wss://relay.snort.social",
    "wss://offchain.pub",
    "wss://nostr-pub.wellorder.net",
    "wss://relay.primal.net",
    "wss://relay.damus.io",
    "wss://nos.lol",
    "wss://nostr.mom",
];

/// Events stamped further than this from the local clock (s) are ignored and counted.
pub const ETA_MASSIMA: u64 = 600;

/// From the session to the signaller.
#[derive(Debug, Default)]
pub struct Notizie {
    /// WebRTC links that opened (`true`) or closed, by matchbox peer.
    pub collegamenti: VecDeque<(PeerId, bool)>,
    /// Say goodbye to the room and stop.
    pub chiudi: bool,
}

pub type NotizieCondivise = Arc<Mutex<Notizie>>;

#[derive(Debug, Clone)]
pub struct NostrSignallerBuilder {
    pub relays: Vec<String>,
    /// Progress of the signaling, for the UI.
    pub diagnosi: DiagnosiCondivisa,
    pub notizie: NotizieCondivise,
    /// The room derived from the code, kept: deriving it is deliberately slow, and matchbox
    /// makes a new signaller on every reconnection attempt.
    pub stanza: Arc<Mutex<Option<(String, Stanza)>>>,
}

impl Default for NostrSignallerBuilder {
    fn default() -> Self {
        NostrSignallerBuilder {
            relays: RELAY_PREDEFINITI.map(String::from).to_vec(),
            diagnosi: DiagnosiCondivisa::default(),
            notizie: NotizieCondivise::default(),
            stanza: Arc::default(),
        }
    }
}

fn errore(m: impl Into<String>) -> SignalingError {
    SignalingError::UserImplementationError(m.into())
}

pub fn adesso() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1000.0) as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}

/// A relay frame, as far as the signaller cares.
#[derive(Debug, PartialEq)]
enum Frame {
    Evento {
        id: String,
        creato: u64,
        contenuto: String,
    },
    /// End of the stored events of our subscription: what follows is live.
    FineStorico,
    /// A notice, a closed subscription, or a refused event.
    Avviso,
    Altro,
}

fn classifica(frame: &str) -> Frame {
    let Ok(v) = serde_json::from_str::<Value>(frame) else {
        return Frame::Altro;
    };
    let evento = || -> Option<Frame> {
        let ev = v.get(2)?;
        Some(Frame::Evento {
            id: ev.get("id")?.as_str()?.to_owned(),
            creato: ev.get("created_at")?.as_u64()?,
            contenuto: ev.get("content")?.as_str()?.to_owned(),
        })
    };
    match v.get(0).and_then(Value::as_str) {
        Some("EVENT") => evento().unwrap_or(Frame::Altro),
        Some("EOSE") => Frame::FineStorico,
        Some("NOTICE" | "CLOSED") => Frame::Avviso,
        // a relay refusing our event (e.g. rate limit)
        Some("OK") if v.get(2) == Some(&Value::Bool(false)) => Frame::Avviso,
        _ => Frame::Altro,
    }
}

/// Whether an event stamped `creato` is plausible on a device whose clock says `adesso`.
fn eta_plausibile(creato: u64, adesso: u64) -> bool {
    creato.abs_diff(adesso) <= ETA_MASSIMA
}

/// Seconds between attempts to reconnect a relay that closed or never answered.
const RIPROVA_RELAY: u64 = 30;

/// One relay: its connection while open, and when to try again otherwise.
struct Relay {
    url: String,
    uscita: Option<ws::Uscita>,
    /// Its stored events are over: what it sends now is live.
    dal_vivo: bool,
    prossimo_tentativo: u64,
}

/// Frames of every relay, tagged with the relay's index; `None` once that relay closed.
type Entrate = SelectAll<futures::stream::BoxStream<'static, (usize, Option<String>)>>;

/// Connects to a relay and subscribes to the room: the sender and the tagged frames.
async fn apri_relay(
    url: &str,
    i: usize,
    stanza: &Stanza,
) -> Result<
    (
        ws::Uscita,
        futures::stream::BoxStream<'static, (usize, Option<String>)>,
    ),
    String,
> {
    let (mut tx, rx) = connetti_entro(url, ATTESA_RELAY).await?;
    // stored events are ignored anyway: `since` only bounds what relays replay
    let filtro = json!({"kinds": [KIND_SEGNALE], "#t": [stanza.etichetta], "since": adesso().saturating_sub(ETA_MASSIMA)});
    let req = json!(["REQ", "bazzica", filtro]).to_string();
    if !invia_entro(&mut tx, req, ATTESA_RELAY).await {
        return Err("subscription refused".into());
    }
    let fine = futures::stream::once(async move { (i, None) });
    Ok((tx, rx.map(move |f| (i, Some(f))).chain(fine).boxed()))
}

pub struct NostrSignaller {
    stanza: Stanza,
    chiavi: Chiavi,
    io: PeerId,
    relay: SyncWrapper<Vec<Relay>>,
    entrate: SyncWrapper<Entrate>,
    pendenti: VecDeque<PeerEvent>,
    incontri: Incontri,
    visti: HashSet<String>,
    diagnosi: DiagnosiCondivisa,
    notizie: NotizieCondivise,
}

impl NostrSignallerBuilder {
    fn stanza(&self, codice: &str) -> Stanza {
        let codice = &crate::nostr::normalizza_codice(codice);
        let mut cache = self.stanza.lock().unwrap_or_else(|e| e.into_inner());
        match &*cache {
            Some((c, s)) if c == codice => s.clone(),
            _ => {
                let s = Stanza::new(codice);
                *cache = Some((codice.to_owned(), s.clone()));
                s
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl SignallerBuilder for NostrSignallerBuilder {
    async fn new_signaller(
        &self,
        _attempts: Option<u16>,
        codice: String,
    ) -> Result<Box<dyn Signaller>, SignalingError> {
        let stanza = self.stanza(&codice);
        let totali = self.relays.len();
        // matchbox retries with a new signaller: keep the last outcome visible meanwhile
        aggiorna(&self.diagnosi, |d| d.relay_totali = totali);
        let aperti = futures::future::join_all(
            self.relays
                .iter()
                .enumerate()
                .map(|(i, url)| apri_relay(url, i, &stanza)),
        )
        .await;
        let mut relay = vec![];
        let mut entrate = SelectAll::new();
        for (url, aperto) in self.relays.iter().zip(aperti) {
            let uscita = match aperto {
                Ok((tx, rx)) => {
                    entrate.push(rx);
                    Some(tx)
                }
                Err(e) => {
                    log::warn!("relay {url}: {e}");
                    None
                }
            };
            relay.push(Relay {
                url: url.clone(),
                uscita,
                dal_vivo: false,
                prossimo_tentativo: adesso() + RIPROVA_RELAY,
            });
        }
        let collegati = relay.iter().filter(|r| r.uscita.is_some()).count();
        aggiorna(&self.diagnosi, |d| d.relay_collegati = Some(collegati));
        if collegati == 0 {
            return Err(errore("nessun relay Nostr raggiungibile"));
        }
        let io = PeerId(uuid::Uuid::from_bytes(casuale::<16>()));
        let mut s = NostrSignaller {
            stanza,
            chiavi: Chiavi::nuove(),
            io,
            relay: SyncWrapper::new(relay),
            entrate: SyncWrapper::new(entrate),
            pendenti: VecDeque::from([PeerEvent::IdAssigned(io)]),
            incontri: Incontri::new(io),
            visti: HashSet::new(),
            diagnosi: self.diagnosi.clone(),
            notizie: self.notizie.clone(),
        };
        let annuncio = s.incontri.richiesta(PeerRequest::KeepAlive);
        s.pubblica(&annuncio).await?;
        Ok(Box::new(s))
    }
}

impl NostrSignaller {
    async fn pubblica(&mut self, b: &Busta) -> Result<(), SignalingError> {
        let testo = serde_json::to_vec(b).expect("serializable");
        let tags = json!([["t", self.stanza.etichetta]]);
        let ev = self
            .chiavi
            .evento(KIND_SEGNALE, tags, &self.stanza.cifra(&testo), adesso());
        let frame = json!(["EVENT", ev]).to_string();
        // Cancel-safe: matchbox drops `next_message` (which publishes too) whenever a request
        // comes in, so the senders stay in their slots while we await.
        let ora = adesso();
        for r in self.relay.get_mut().iter_mut() {
            if let Some(tx) = &mut r.uscita
                && !invia_entro(tx, frame.clone(), ATTESA_RELAY).await
            {
                log::warn!("relay {}: invio fallito", r.url);
                chiudi_relay(r, ora);
            }
        }
        // with no relay left the envelope is lost: the pairing retries, and relays reconnect
        self.conta_relay();
        Ok(())
    }

    fn conta_relay(&mut self) {
        let collegati = self
            .relay
            .get_mut()
            .iter()
            .filter(|r| r.uscita.is_some())
            .count();
        aggiorna(&self.diagnosi, |d| d.relay_collegati = Some(collegati));
    }

    /// Relays that closed or never answered get a new connection, every [`RIPROVA_RELAY`] s.
    async fn riconnetti(&mut self) {
        let ora = adesso();
        let da_aprire: Vec<(usize, String)> = self
            .relay
            .get_mut()
            .iter()
            .enumerate()
            .filter(|(_, r)| r.uscita.is_none() && ora >= r.prossimo_tentativo)
            .map(|(i, r)| (i, r.url.clone()))
            .collect();
        if da_aprire.is_empty() {
            return;
        }
        let aperti = futures::future::join_all(
            da_aprire
                .iter()
                .map(|(i, url)| apri_relay(url, *i, &self.stanza)),
        )
        .await;
        for ((i, url), aperto) in da_aprire.into_iter().zip(aperti) {
            let r = &mut self.relay.get_mut()[i];
            match aperto {
                Ok((tx, rx)) => {
                    log::info!("relay {url}: ricollegato");
                    r.uscita = Some(tx);
                    r.dal_vivo = false;
                    self.entrate.get_mut().push(rx);
                }
                Err(_) => r.prossimo_tentativo = ora + RIPROVA_RELAY,
            }
        }
        self.conta_relay();
    }

    /// Decodes a frame of relay `relay` into a live envelope addressed to us (or to everyone).
    fn decodifica(&mut self, relay: usize, frame: &str) -> Option<Busta> {
        let (id, creato, contenuto) = match classifica(frame) {
            Frame::Evento {
                id,
                creato,
                contenuto,
            } => (id, creato, contenuto),
            Frame::FineStorico => {
                if let Some(r) = self.relay.get_mut().get_mut(relay) {
                    r.dal_vivo = true;
                }
                return None;
            }
            Frame::Avviso => {
                log::warn!("relay: {frame}");
                return None;
            }
            Frame::Altro => return None,
        };
        if !self.relay.get_mut().get(relay).is_some_and(|r| r.dal_vivo) {
            return None; // replayed from storage: possibly from a peer long gone
        }
        if !self.visti.insert(id) {
            return None; // same event from another relay
        }
        if self.visti.len() > 10_000 {
            self.visti.clear();
        }
        if !eta_plausibile(creato, adesso()) {
            log::debug!("evento con orario lontano: {creato} contro {}", adesso());
            aggiorna(&self.diagnosi, |d| d.scartati_orario += 1);
            return None;
        }
        let testo = self.stanza.decifra(&contenuto)?;
        let b: Busta = serde_json::from_slice(&testo).ok()?;
        (b.mittente() != self.io && b.destinatario().is_none_or(|a| a == self.io)).then_some(b)
    }

    /// Carries out the pairing decisions: events go to matchbox first (they must not be lost
    /// if this future is dropped while publishing), then envelopes are published.
    async fn esegui(&mut self, azioni: Vec<Azione>) -> Result<(), SignalingError> {
        let mut da_pubblicare = vec![];
        for a in azioni {
            match a {
                Azione::Emetti(e) => self.pendenti.push_back(e),
                Azione::Pubblica(b) => da_pubblicare.push(b),
            }
        }
        for b in da_pubblicare {
            self.pubblica(&b).await?;
        }
        Ok(())
    }

    /// Link changes and timeouts; `Err` once the session asked to close (after the goodbye).
    async fn notizie(&mut self) -> Result<(), SignalingError> {
        let (collegamenti, chiudi) = match self.notizie.lock() {
            Ok(mut n) => (std::mem::take(&mut n.collegamenti), n.chiudi),
            Err(_) => (VecDeque::new(), false),
        };
        if chiudi {
            let addio = self.incontri.addio();
            let _ = self.pubblica(&addio).await;
            return Err(SignalingError::StreamExhausted);
        }
        let ora = adesso();
        let mut azioni = vec![];
        for (p, aperto) in collegamenti {
            if aperto {
                self.incontri.collegato(p);
            } else {
                azioni.extend(self.incontri.perso(p, ora));
            }
        }
        azioni.extend(self.incontri.scadenze(ora));
        self.esegui(azioni).await?;
        self.riconnetti().await;
        Ok(())
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl Signaller for NostrSignaller {
    async fn send(&mut self, request: PeerRequest) -> Result<(), SignalingError> {
        let b = self.incontri.richiesta(request);
        self.pubblica(&b).await
    }

    async fn next_message(&mut self) -> Result<PeerEvent, SignalingError> {
        loop {
            let visti = self.incontri.visti();
            aggiorna(&self.diagnosi, |d| d.peer_visti = visti);
            if let Some(e) = self.pendenti.pop_front() {
                return Ok(e);
            }
            let mut un_secondo =
                futures_timer::Delay::new(std::time::Duration::from_secs(1)).fuse();
            // with every relay closed there is nothing to read: only the housekeeping, which
            // reconnects them (an empty `SelectAll` would end at once and spin)
            let prossimo = if self.entrate.get_mut().is_empty() {
                un_secondo.await;
                None
            } else {
                futures::select! {
                    f = self.entrate.get_mut().next().fuse() => f,
                    _ = un_secondo => None,
                }
            };
            match prossimo {
                Some((relay, None)) => {
                    let r = &mut self.relay.get_mut()[relay];
                    log::warn!("relay {}: connessione chiusa", r.url);
                    chiudi_relay(r, adesso());
                    self.conta_relay();
                }
                Some((relay, Some(frame))) => {
                    if let Some(b) = self.decodifica(relay, &frame) {
                        let azioni = self.incontri.busta(b, adesso());
                        self.esegui(azioni).await?;
                    }
                }
                None => self.notizie().await?,
            }
        }
    }
}

/// Marks a relay closed; it is tried again after [`RIPROVA_RELAY`] seconds.
fn chiudi_relay(r: &mut Relay, ora: u64) {
    r.uscita = None;
    r.dal_vivo = false;
    r.prossimo_tentativo = ora + RIPROVA_RELAY;
}

/// How long to wait for each relay before giving up on it.
const ATTESA_RELAY: std::time::Duration = std::time::Duration::from_secs(5);

/// Sends a frame to one relay; false when the relay failed or stalled (then it is dropped).
async fn invia_entro(tx: &mut ws::Uscita, frame: String, attesa: std::time::Duration) -> bool {
    futures::select! {
        r = tx.send(frame).fuse() => r.is_ok(),
        _ = futures_timer::Delay::new(attesa).fuse() => false,
    }
}

async fn connetti_entro(
    url: &str,
    attesa: std::time::Duration,
) -> Result<(ws::Uscita, ws::Entrata), String> {
    futures::select! {
        c = ws::connetti(url).fuse() => c,
        _ = futures_timer::Delay::new(attesa).fuse() => Err("timeout".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_stanza_si_deriva_una_volta() {
        let b = NostrSignallerBuilder::default();
        let prima = b.stanza("ABCD234567");
        let t = std::time::Instant::now();
        assert_eq!(b.stanza("abcd-2345 67").etichetta, prima.etichetta);
        assert!(t.elapsed() < std::time::Duration::from_millis(5), "cached");
        assert_eq!(
            NostrSignallerBuilder::default()
                .stanza("ABCD234567")
                .etichetta,
            prima.etichetta,
            "two builders, same room"
        );
        assert_ne!(b.stanza("ZZZZ234567").etichetta, prima.etichetta);
    }

    #[test]
    fn frame_dei_relay() {
        let ev = r#"["EVENT","bazzica",{"id":"ab","created_at":1700000000,"content":"xyz","kind":25319}]"#;
        assert_eq!(
            classifica(ev),
            Frame::Evento {
                id: "ab".into(),
                creato: 1_700_000_000,
                contenuto: "xyz".into()
            }
        );
        assert_eq!(classifica(r#"["EOSE","bazzica"]"#), Frame::FineStorico);
        assert_eq!(classifica(r#"["NOTICE","slow down"]"#), Frame::Avviso);
        assert_eq!(classifica(r#"["CLOSED","bazzica","bye"]"#), Frame::Avviso);
        assert_eq!(
            classifica(r#"["OK","ab",false,"rate-limited"]"#),
            Frame::Avviso
        );
        assert_eq!(classifica(r#"["OK","ab",true,""]"#), Frame::Altro);
        assert_eq!(
            classifica(r#"["EVENT","bazzica",{"id":"ab"}]"#),
            Frame::Altro
        );
        assert_eq!(classifica("non json"), Frame::Altro);
    }

    #[test]
    fn orologi_diversi_ma_plausibili() {
        let ora = 1_700_000_000;
        assert!(eta_plausibile(ora - 60, ora), "a minute behind");
        assert!(eta_plausibile(ora + 120, ora), "two minutes ahead");
        assert!(!eta_plausibile(ora - 20 * 60, ora));
        assert!(!eta_plausibile(ora + 20 * 60, ora));
    }
}
