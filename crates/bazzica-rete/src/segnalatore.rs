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

/// Public relays tried by default (any that accept connections are used).
pub const RELAY_PREDEFINITI: [&str; 4] = [
    "wss://relay.damus.io",
    "wss://nos.lol",
    "wss://relay.primal.net",
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
    noti: HashSet<PeerId>,
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
            noti: HashSet::new(),
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
        let mut vivi = vec![];
        for mut tx in std::mem::take(self.uscite.get_mut()) {
            if tx.send(frame.clone()).await.is_ok() {
                vivi.push(tx);
            }
        }
        *self.uscite.get_mut() = vivi;
        if self.uscite.get_mut().is_empty() {
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
            let visti = self.noti.len();
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
                Some(Busta::Annuncio { da, .. }) => {
                    if !self.noti.insert(da) {
                        continue;
                    }
                    if self.io < da {
                        return Ok(PeerEvent::NewPeer(da));
                    }
                    // the other side offers: make sure it knows about us
                    let io = self.io;
                    self.pubblica(&Busta::Annuncio {
                        da: io,
                        a: Some(da),
                    })
                    .await?;
                }
                Some(Busta::Segnale { da, s, .. }) => {
                    self.noti.insert(da);
                    return Ok(PeerEvent::Signal {
                        sender: da,
                        data: s,
                    });
                }
                Some(Busta::Addio { da }) if self.noti.remove(&da) => {
                    return Ok(PeerEvent::PeerLeft(da));
                }
                _ => {}
            }
        }
    }
}

/// How long to wait for each relay before giving up on it.
const ATTESA_RELAY: std::time::Duration = std::time::Duration::from_secs(5);

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
