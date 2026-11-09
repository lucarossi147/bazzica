//! Glue between a matchbox WebRTC socket and a [`Nodo`].
//!
//! The UI owns a [`Sessione`], spawns the returned message-loop future, and calls
//! [`Sessione::aggiorna`] regularly (every animation frame or ~50 ms).

use std::sync::Arc;

use bazzica_net::{Destinatario, Effimero, Errore, Messaggio, Nodo, Richiesta, Uscita};
use matchbox_socket::{MessageLoopFuture, PeerId, PeerState, WebRtcSocket};

use crate::ice::{Diagnosi, DiagnosiCondivisa, Turn, leggi, server_ice};
use crate::segnalatore::{NostrSignallerBuilder, NotizieCondivise};

/// How long a replaced socket is kept, so its signaller can say goodbye to the room (ms).
const CONGEDO_MS: u64 = 3_000;

pub struct Sessione {
    socket: WebRtcSocket,
    nodo: Nodo<PeerId>,
    turn: Option<Turn>,
    diagnosi: DiagnosiCondivisa,
    notizie: NotizieCondivise,
    /// Replaced sockets saying goodbye, dropped at the given time.
    congedi: Vec<(WebRtcSocket, u64)>,
}

impl Sessione {
    /// Opens the room `codice` and attaches `nodo` to it.
    pub fn apri(
        codice: &str,
        segnalatore: NostrSignallerBuilder,
        nodo: Nodo<PeerId>,
        turn: Option<Turn>,
    ) -> (Sessione, MessageLoopFuture) {
        let diagnosi = segnalatore.diagnosi.clone();
        let notizie = segnalatore.notizie.clone();
        let (socket, ciclo) = Self::socket(codice, segnalatore, turn.as_ref());
        (
            Sessione {
                socket,
                nodo,
                turn,
                diagnosi,
                notizie,
                congedi: vec![],
            },
            ciclo,
        )
    }

    /// How far the signaling got (relays reached, peers seen).
    pub fn diagnosi(&self) -> Diagnosi {
        leggi(&self.diagnosi)
    }

    /// Peers with an open WebRTC link.
    pub fn peer_collegati(&self) -> usize {
        self.socket.connected_peers().count()
    }

    pub fn turn(&self) -> Option<&Turn> {
        self.turn.as_ref()
    }

    /// Rebuilds the WebRTC socket (after the page slept or signaling died), keeping the replica.
    /// `turn` replaces the TURN relay in use (pass the current one to keep it).
    pub fn riapri(
        &mut self,
        ora: u64,
        codice: &str,
        segnalatore: NostrSignallerBuilder,
        turn: Option<Turn>,
    ) -> MessageLoopFuture {
        self.nodo.scollega_tutti(ora);
        self.chiudi();
        self.diagnosi = segnalatore.diagnosi.clone();
        self.notizie = segnalatore.notizie.clone();
        let (socket, ciclo) = Self::socket(codice, segnalatore, turn.as_ref());
        let vecchio = std::mem::replace(&mut self.socket, socket);
        self.congedi.push((vecchio, ora + CONGEDO_MS));
        self.turn = turn;
        ciclo
    }

    fn socket(
        codice: &str,
        segnalatore: NostrSignallerBuilder,
        turn: Option<&Turn>,
    ) -> (WebRtcSocket, MessageLoopFuture) {
        WebRtcSocket::builder(crate::nostr::normalizza_codice(codice))
            .signaller_builder(Arc::new(segnalatore))
            .ice_server(server_ice(turn))
            .signaling_keep_alive_interval(Some(std::time::Duration::from_secs(15)))
            .reconnect_attempts(Some(3))
            .add_reliable_channel()
            .build()
    }

    /// Asks the signaller to say goodbye to the room and stop (leaving the table). Keep the
    /// session alive for a few seconds afterwards so the goodbye can go out.
    pub fn chiudi(&mut self) {
        if let Ok(mut n) = self.notizie.lock() {
            n.chiudi = true;
        }
    }

    pub fn nodo(&self) -> &Nodo<PeerId> {
        &self.nodo
    }

    /// Takes the table back from another tab of the same identity; reopen the socket next.
    pub fn riprendi(&mut self, ora: u64) {
        self.nodo.riprendi(ora);
    }

    /// Processes the network and the clock; returns what the UI must react to.
    pub fn aggiorna(&mut self, ora: u64) -> Vec<Uscita<PeerId>> {
        self.congedi.retain(|(_, fino)| ora < *fino);
        if let Ok(cambi) = self.socket.try_update_peers() {
            for (p, stato) in cambi {
                let aperto = stato == PeerState::Connected;
                // the signaller retries pairs whose link closed
                if let Ok(mut n) = self.notizie.lock() {
                    n.collegamenti.push_back((p, aperto));
                }
                if aperto {
                    self.nodo.connesso(ora, p);
                } else {
                    self.nodo.disconnesso(ora, &p);
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
