//! A matchbox [`Signaller`] that uses public Nostr relays instead of a signaling server.
//!
//! Every peer publishes encrypted "envelopes" to all relays and listens on all of them:
//! - `annuncio`: "I am in the room" (on join and on every keep-alive);
//! - `segnale`: a WebRTC offer/answer/ICE candidate for one peer;
//! - `addio`: "I am leaving".
//!
//! For each pair, the peer with the lower id makes the WebRTC offer (matchbox's `NewPeer`).

use std::collections::{HashSet, VecDeque};

use futures::stream::SelectAll;
use futures::{SinkExt, StreamExt};
use matchbox_socket::async_trait::async_trait;
use matchbox_socket::{
    PeerEvent, PeerId, PeerRequest, PeerSignal, SignalingError, Signaller, SignallerBuilder,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sync_wrapper::SyncWrapper;

use crate::ice::{DiagnosiCondivisa, aggiorna};
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

#[derive(Debug, Clone)]
pub struct NostrSignallerBuilder {
    pub relays: Vec<String>,
    /// Progress of the signaling, for the UI.
    pub diagnosi: DiagnosiCondivisa,
}

impl Default for NostrSignallerBuilder {
    fn default() -> Self {
        NostrSignallerBuilder {
            relays: RELAY_PREDEFINITI.map(String::from).to_vec(),
            diagnosi: DiagnosiCondivisa::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Busta {
    Annuncio {
        da: PeerId,
        a: Option<PeerId>,
    },
    Segnale {
        da: PeerId,
        a: PeerId,
        s: PeerSignal,
    },
    Addio {
        da: PeerId,
    },
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

pub struct NostrSignaller {
    stanza: Stanza,
    chiavi: Chiavi,
    io: PeerId,
    uscite: SyncWrapper<Vec<ws::Uscita>>,
    entrate: SyncWrapper<SelectAll<ws::Entrata>>,
    pendenti: VecDeque<PeerEvent>,
    incontri: Incontri,
    visti: HashSet<String>,
    diagnosi: DiagnosiCondivisa,
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl SignallerBuilder for NostrSignallerBuilder {
    async fn new_signaller(
        &self,
        _attempts: Option<u16>,
        codice: String,
    ) -> Result<Box<dyn Signaller>, SignalingError> {
        let stanza = Stanza::new(&codice);
        let totali = self.relays.len();
        // matchbox retries with a new signaller: keep the last outcome visible meanwhile
        aggiorna(&self.diagnosi, |d| d.relay_totali = totali);
        let connessioni =
            futures::future::join_all(self.relays.iter().map(|r| connetti_entro(r, ATTESA_RELAY)))
                .await;
        let mut uscite = vec![];
        let mut entrate = SelectAll::new();
        let filtro = json!({"kinds": [KIND_SEGNALE], "#t": [stanza.etichetta], "since": adesso().saturating_sub(30)});
        let req = json!(["REQ", "bazzica", filtro]).to_string();
        for (relay, c) in self.relays.iter().zip(connessioni) {
            match c {
                Ok((mut tx, rx)) => {
                    if tx.send(req.clone()).await.is_ok() {
                        uscite.push(tx);
                        entrate.push(rx);
                    }
                }
                Err(e) => log::warn!("relay {relay}: {e}"),
            }
        }
        let collegati = uscite.len();
        aggiorna(&self.diagnosi, |d| d.relay_collegati = Some(collegati));
        if uscite.is_empty() {
            return Err(errore("nessun relay Nostr raggiungibile"));
        }
        let io = PeerId(uuid::Uuid::from_bytes(casuale::<16>()));
        let mut s = NostrSignaller {
            stanza,
            chiavi: Chiavi::nuove(),
            io,
            uscite: SyncWrapper::new(uscite),
            entrate: SyncWrapper::new(entrate),
            pendenti: VecDeque::from([PeerEvent::IdAssigned(io)]),
            incontri: Incontri::new(io),
            visti: HashSet::new(),
            diagnosi: self.diagnosi.clone(),
        };
        s.pubblica(&Busta::Annuncio { da: io, a: None }).await?;
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
        // comes in, so the senders must never leave `uscite` while we await.
        let uscite = self.uscite.get_mut();
        let mut i = 0;
        while i < uscite.len() {
            if invia_entro(&mut uscite[i], frame.clone(), ATTESA_RELAY).await {
                i += 1;
            } else {
                drop(uscite.remove(i));
            }
        }
        if uscite.is_empty() {
            return Err(errore("tutti i relay Nostr sono caduti"));
        }
        Ok(())
    }

    /// Decodes a relay frame into an envelope addressed to us (or to everyone).
    fn decodifica(&mut self, frame: &str) -> Option<Busta> {
        let v: Value = serde_json::from_str(frame).ok()?;
        match v.get(0)?.as_str()? {
            "EVENT" => {}
            "NOTICE" | "CLOSED" => {
                log::warn!("relay: {frame}");
                return None;
            }
            // a relay refusing our event (e.g. rate limit)
            "OK" if v.get(2) == Some(&Value::Bool(false)) => {
                log::warn!("relay: {frame}");
                return None;
            }
            _ => return None,
        }
        let ev = v.get(2)?;
        let id = ev.get("id")?.as_str()?.to_owned();
        if !self.visti.insert(id) {
            return None; // same event from another relay
        }
        if self.visti.len() > 10_000 {
            self.visti.clear();
        }
        let testo = self.stanza.decifra(ev.get("content")?.as_str()?)?;
        let b: Busta = serde_json::from_slice(&testo).ok()?;
        let (da, a) = match &b {
            Busta::Annuncio { da, a } => (*da, *a),
            Busta::Segnale { da, a, .. } => (*da, Some(*a)),
            Busta::Addio { da } => (*da, None),
        };
        (da != self.io && a.is_none_or(|a| a == self.io)).then_some(b)
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl Signaller for NostrSignaller {
    async fn send(&mut self, request: PeerRequest) -> Result<(), SignalingError> {
        let b = match request {
            PeerRequest::Signal { receiver, data } => Busta::Segnale {
                da: self.io,
                a: receiver,
                s: data,
            },
            // keep-alives double as presence announcements for late joiners
            PeerRequest::KeepAlive => Busta::Annuncio {
                da: self.io,
                a: None,
            },
        };
        self.pubblica(&b).await
    }

    async fn next_message(&mut self) -> Result<PeerEvent, SignalingError> {
        loop {
            let visti = self.incontri.noti.len();
            aggiorna(&self.diagnosi, |d| d.peer_visti = visti);
            if let Some(e) = self.pendenti.pop_front() {
                return Ok(e);
            }
            let frame = self
                .entrate
                .get_mut()
                .next()
                .await
                .ok_or(SignalingError::StreamExhausted)?;
            match self.decodifica(&frame) {
                Some(Busta::Annuncio { da, .. }) => match self.incontri.annuncio(da) {
                    Reazione::Offri => return Ok(PeerEvent::NewPeer(da)),
                    Reazione::Chiedi => {
                        let io = self.io;
                        self.pubblica(&Busta::Annuncio {
                            da: io,
                            a: Some(da),
                        })
                        .await?;
                    }
                    Reazione::Niente => {}
                },
                Some(Busta::Segnale { da, s, .. }) => {
                    self.incontri.segnale(da);
                    return Ok(PeerEvent::Signal {
                        sender: da,
                        data: s,
                    });
                }
                Some(Busta::Addio { da }) if self.incontri.noti.remove(&da) => {
                    return Ok(PeerEvent::PeerLeft(da));
                }
                _ => {}
            }
        }
    }
}

/// Who meets whom: for each pair the lower id offers (matchbox's `NewPeer`), the higher one
/// tells it to. Any envelope may be lost, so the higher one keeps asking on every announcement
/// of the other until a signal from it arrives.
struct Incontri {
    io: PeerId,
    /// Peers seen in the room.
    noti: HashSet<PeerId>,
    /// Peers we made an offer to.
    offerti: HashSet<PeerId>,
    /// Peers that sent us a WebRTC signal (their offer reached us).
    segnalati: HashSet<PeerId>,
}

#[derive(Debug, PartialEq, Eq)]
enum Reazione {
    /// Make the WebRTC offer.
    Offri,
    /// Send a directed announcement so that the other side offers.
    Chiedi,
    Niente,
}

impl Incontri {
    fn new(io: PeerId) -> Incontri {
        Incontri {
            io,
            noti: HashSet::new(),
            offerti: HashSet::new(),
            segnalati: HashSet::new(),
        }
    }

    /// An announcement (broadcast or directed to us) from `da`.
    fn annuncio(&mut self, da: PeerId) -> Reazione {
        self.noti.insert(da);
        if self.io < da {
            if self.offerti.insert(da) {
                Reazione::Offri
            } else {
                Reazione::Niente
            }
        } else if self.segnalati.contains(&da) {
            Reazione::Niente
        } else {
            Reazione::Chiedi
        }
    }

    fn segnale(&mut self, da: PeerId) {
        self.noti.insert(da);
        self.segnalati.insert(da);
    }
}

/// How long to wait for each relay before giving up on it.
const ATTESA_RELAY: std::time::Duration = std::time::Duration::from_secs(5);

/// Sends a frame to one relay; false when the relay failed or stalled (then it is dropped).
async fn invia_entro(tx: &mut ws::Uscita, frame: String, attesa: std::time::Duration) -> bool {
    use futures::FutureExt;
    futures::select! {
        r = tx.send(frame).fuse() => r.is_ok(),
        _ = futures_timer::Delay::new(attesa).fuse() => false,
    }
}

async fn connetti_entro(
    url: &str,
    attesa: std::time::Duration,
) -> Result<(ws::Uscita, ws::Entrata), String> {
    use futures::FutureExt;
    futures::select! {
        c = ws::connetti(url).fuse() => c,
        _ = futures_timer::Delay::new(attesa).fuse() => Err("timeout".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> PeerId {
        PeerId(uuid::Uuid::from_bytes([n; 16]))
    }

    /// A host waiting in the room and a joiner, exchanging announcements over relays that lose
    /// the first `persi` directed announcements. Returns the round in which the offer is made.
    fn incontro(host: PeerId, ospite: PeerId, mut persi: usize) -> Option<usize> {
        let mut p = [Incontri::new(host), Incontri::new(ospite)];
        for giro in 0..10 {
            // each side announces itself (join, then every keep-alive) and the other reacts
            for (chi, altro) in [(1, 0), (0, 1)] {
                let da = p[chi].io;
                match p[altro].annuncio(da) {
                    Reazione::Offri => return Some(giro),
                    Reazione::Chiedi if persi > 0 => persi -= 1,
                    // the directed announcement arrives: the other side (lower id) offers
                    Reazione::Chiedi => {
                        let io = p[altro].io;
                        if p[chi].annuncio(io) == Reazione::Offri {
                            return Some(giro);
                        }
                    }
                    Reazione::Niente => {}
                }
            }
        }
        None
    }

    #[test]
    fn la_coppia_si_incontra_anche_se_si_perdono_buste() {
        for (host, ospite) in [(id(1), id(2)), (id(2), id(1))] {
            assert_eq!(incontro(host, ospite, 0), Some(0));
            assert!(incontro(host, ospite, 3).is_some(), "{host} {ospite}");
        }
    }

    #[test]
    fn una_sola_offerta_per_peer() {
        let mut basso = Incontri::new(id(1));
        assert_eq!(basso.annuncio(id(2)), Reazione::Offri);
        assert_eq!(basso.annuncio(id(2)), Reazione::Niente);
        let mut alto = Incontri::new(id(2));
        assert_eq!(alto.annuncio(id(1)), Reazione::Chiedi);
        assert_eq!(alto.annuncio(id(1)), Reazione::Chiedi);
        alto.segnale(id(1));
        assert_eq!(alto.annuncio(id(1)), Reazione::Niente);
        assert_eq!(alto.noti.len(), 1);
    }
}
