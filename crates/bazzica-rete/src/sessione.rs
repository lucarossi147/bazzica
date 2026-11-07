//! Glue between a matchbox WebRTC socket and a [`Nodo`].
//!
//! The UI owns a [`Sessione`], spawns the returned message-loop future, and calls
//! [`Sessione::aggiorna`] regularly (every animation frame or ~50 ms).

use std::sync::Arc;

use bazzica_net::{Destinatario, Effimero, Errore, Messaggio, Nodo, Richiesta, Uscita};
use matchbox_socket::{MessageLoopFuture, PeerId, PeerState, RtcIceServerConfig, WebRtcSocket};

use crate::segnalatore::NostrSignallerBuilder;

/// Public STUN servers (no account needed). No TURN relay for now.
pub const STUN: [&str; 2] = [
    "stun:stun.l.google.com:19302",
    "stun:stun.cloudflare.com:3478",
];

pub struct Sessione {
    socket: WebRtcSocket,
    nodo: Nodo<PeerId>,
}

impl Sessione {
    /// Opens the room `codice` and attaches `nodo` to it.
    pub fn apri(
        codice: &str,
        segnalatore: NostrSignallerBuilder,
        nodo: Nodo<PeerId>,
    ) -> (Sessione, MessageLoopFuture) {
        let (socket, ciclo) = WebRtcSocket::builder(crate::nostr::normalizza_codice(codice))
            .signaller_builder(Arc::new(segnalatore))
            .ice_server(RtcIceServerConfig {
                urls: STUN.map(String::from).to_vec(),
                username: None,
                credential: None,
            })
            .signaling_keep_alive_interval(Some(std::time::Duration::from_secs(5)))
            .reconnect_attempts(Some(3))
            .add_reliable_channel()
            .build();
        (Sessione { socket, nodo }, ciclo)
    }

    pub fn nodo(&self) -> &Nodo<PeerId> {
        &self.nodo
    }

    /// Processes the network and the clock; returns what the UI must react to.
    pub fn aggiorna(&mut self, ora: u64) -> Vec<Uscita<PeerId>> {
        if let Ok(cambi) = self.socket.try_update_peers() {
            for (p, stato) in cambi {
                match stato {
                    PeerState::Connected => self.nodo.connesso(ora, p),
                    PeerState::Disconnected => self.nodo.disconnesso(ora, &p),
                }
            }
        }
        if let Ok(canale) = self.socket.get_channel_mut(0) {
            for (p, pacchetto) in canale.receive() {
                match Messaggio::decodifica(&pacchetto) {
                    Some(m) => self.nodo.ricevi(ora, p, m),
                    None => log::warn!("messaggio non valido da {p}"),
                }
            }
        }
        self.nodo.tick(ora);
        self.svuota()
    }

    pub fn richiedi(&mut self, ora: u64, r: Richiesta) -> Vec<Uscita<PeerId>> {
        self.nodo.richiedi(ora, r);
        self.svuota()
    }

    pub fn effimero(&mut self, ora: u64, e: Effimero) -> Result<Vec<Uscita<PeerId>>, Errore> {
        self.nodo.effimero(ora, e)?;
        Ok(self.svuota())
    }

    /// Sends the node's outgoing messages; returns the rest.
    fn svuota(&mut self) -> Vec<Uscita<PeerId>> {
        let mut resto = vec![];
        let connessi: Vec<PeerId> = self.socket.connected_peers().collect();
        for u in self.nodo.uscite() {
            match u {
                Uscita::Invia { a, msg } => {
                    let pacchetto: matchbox_socket::Packet = msg.codifica().into();
                    let Ok(canale) = self.socket.get_channel_mut(0) else {
                        continue;
                    };
                    match a {
                        Destinatario::Tutti => {
                            for p in &connessi {
                                canale.send(pacchetto.clone(), *p);
                            }
                        }
                        Destinatario::Uno(p) => canale.send(pacchetto, p),
                    }
                }
                altro => resto.push(altro),
            }
        }
        resto
    }
}
