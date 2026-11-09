//! One peer of a table, as a sans-I/O state machine.
//!
//! The driver feeds it transport events ([`Nodo::connesso`], [`Nodo::disconnesso`],
//! [`Nodo::ricevi`]), local user requests ([`Nodo::richiedi`]) and the clock ([`Nodo::tick`]),
//! then drains [`Nodo::uscite`] to send messages and update the UI. Time is in milliseconds
//! from any fixed origin; the node never reads a clock or a socket itself.
//!
//! Replication model: the host numbers every change (human moves, bot moves, timer expiries,
//! lobby edits) and broadcasts it with the replica hash. Every peer applies the same entries.
//! If the host leaves, the first connected participant in join order takes over with a higher
//! `epoca`; peers that are ahead hand it their replica, peers that are behind catch up.

use std::collections::{BTreeMap, BTreeSet};

use bazzica_core::bots::{Agent, HeuristicAgent};
use bazzica_core::game::compagno;
use bazzica_core::{Azione, Evento, Fase, Mulberry32, Seat};

use crate::protocollo::{Effimero, Messaggio, Richiesta, VERSIONE_PROTOCOLLO};
use crate::tavolo::{
    Errore, FRASI, IdGiocatore, Impostazioni, Partecipante, StatoTavolo, Tavolo, TipoTimer, Voce,
};

#[derive(Clone, Debug)]
pub struct Config {
    /// Bot "thinking" time range at the normal pace, in ms (the table's [`Ritmo`] scales it).
    pub ritardo_bot: (u64, u64),
    /// Pause before the next hand in bot-only games, in ms.
    pub pausa_fine_mano: u64,
    /// Minimum ms between two "che palo" calls of the same seat.
    pub pausa_palo: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            ritardo_bot: (1300, 2200),
            pausa_fine_mano: 2000,
            pausa_palo: 3000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destinatario<P> {
    Tutti,
    Uno(P),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Uscita<P> {
    /// Send a message on the transport.
    Invia {
        a: Destinatario<P>,
        msg: Messaggio,
    },
    /// A log entry was applied to the local replica (engine events included, unfiltered).
    Applicata {
        voce: Voce,
        eventi: Vec<Evento>,
    },
    /// The local replica was replaced by a snapshot.
    Sincronizzata,
    /// A request of the local user was refused.
    Rifiutata {
        motivo: String,
    },
    Effimero {
        da: IdGiocatore,
        effetto: Effimero,
    },
}

/// A connected peer that introduced itself.
#[derive(Clone, Debug)]
struct Remoto {
    id: IdGiocatore,
    /// Whether it holds a replica (only those can take over as host).
    replica: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Piano {
    Bot(Seat),
    NuovaMano,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChiaveTimer {
    tipo: TipoTimer,
    mano: u32,
    turno: u8,
    giocate: usize,
    seat: Option<Seat>,
}

pub struct Nodo<P> {
    io: Partecipante,
    config: Config,
    rng: Mulberry32,
    ora: u64,
    tavolo: Option<Tavolo>,
    /// Applied entries `log_base+1 ..= log_base+log.len()`, kept to resend to late peers.
    log: Vec<(Voce, String)>,
    log_base: u64,
    epoca: u64,
    host: Option<IdGiocatore>,
    /// Connected peers and, once they said hello, who they are.
    peers: BTreeMap<P, Option<Remoto>>,
    rid: u64,
    attesa_istantanea: bool,
    // host only
    piano: Option<(u64, u64, Piano)>,
    timer: Option<(ChiaveTimer, u64)>,
    ultimo_palo: [Option<u64>; 4],
    uscite: Vec<Uscita<P>>,
    /// Protocol version of a peer we had to ignore (it runs another build of the app).
    versione_altrui: Option<u32>,
}

impl<P: Clone + Ord> Nodo<P> {
    /// Creates a new table, hosted by this peer.
    pub fn crea(io: Partecipante, impostazioni: Impostazioni, seme: u64, config: Config) -> Self {
        let mut n = Self::vuoto(io, seme, config);
        n.tavolo = Some(Tavolo::nuovo(n.io.clone(), impostazioni));
        n.host = Some(n.io.id.clone());
        n
    }

    /// Joins an existing table: the replica arrives from the host after connecting.
    pub fn unisciti(io: Partecipante, seme: u64, config: Config) -> Self {
        Self::vuoto(io, seme, config)
    }

    fn vuoto(io: Partecipante, seme: u64, config: Config) -> Self {
        Nodo {
            io,
            config,
            rng: Mulberry32::new(seme),
            ora: 0,
            tavolo: None,
            log: vec![],
            log_base: 0,
            epoca: 0,
            host: None,
            peers: BTreeMap::new(),
            rid: 0,
            attesa_istantanea: false,
            piano: None,
            timer: None,
            ultimo_palo: [None; 4],
            uscite: vec![],
            versione_altrui: None,
        }
    }

    // ------------------------------------------------------------------ queries

    pub fn io(&self) -> &Partecipante {
        &self.io
    }

    /// Set when a connected peer runs an incompatible build: both must reload the app.
    pub fn versione_altrui(&self) -> Option<u32> {
        self.versione_altrui
    }

    pub fn tavolo(&self) -> Option<&Tavolo> {
        self.tavolo.as_ref()
    }

    pub fn host(&self) -> Option<&IdGiocatore> {
        self.host.as_ref()
    }

    pub fn epoca(&self) -> u64 {
        self.epoca
    }

    pub fn e_host(&self) -> bool {
        self.host.as_ref() == Some(&self.io.id)
    }

    /// Ids of the participants reachable from here (this peer included).
    pub fn connessi(&self) -> BTreeSet<IdGiocatore> {
        let mut s: BTreeSet<_> = self
            .peers
            .values()
            .flatten()
            .map(|r| r.id.clone())
            .collect();
        s.insert(self.io.id.clone());
        s
    }

    /// Seated humans that are not connected: while any exists the game is paused.
    pub fn disconnessi(&self) -> Vec<IdGiocatore> {
        let Some(t) = &self.tavolo else { return vec![] };
        let c = self.connessi();
        t.umani().into_iter().filter(|id| !c.contains(id)).collect()
    }

    pub fn in_pausa(&self) -> bool {
        self.tavolo
            .as_ref()
            .is_some_and(|t| t.stato == StatoTavolo::InCorso)
            && !self.disconnessi().is_empty()
    }

    /// Running turn timer: (kind, seat, deadline in ms). Only known on the host.
    pub fn timer(&self) -> Option<(TipoTimer, Option<Seat>, u64)> {
        self.timer.as_ref().map(|(k, t)| (k.tipo, k.seat, *t))
    }

    /// When the driver must call [`Nodo::tick`] next.
    pub fn prossima_sveglia(&self) -> Option<u64> {
        [self.piano.map(|p| p.0), self.timer.as_ref().map(|t| t.1)]
            .into_iter()
            .flatten()
            .min()
    }

    /// Checks the node's internal invariants (for tests and debug assertions):
    /// I1, the log covers the replica exactly (`tavolo.n == log_base + log.len()`).
    pub fn verifica_invarianti(&self) -> Result<(), String> {
        match &self.tavolo {
            Some(t) if t.n != self.log_base + self.log.len() as u64 => Err(format!(
                "I1: {} n={} but log_base={} + log={}",
                self.io.id,
                t.n,
                self.log_base,
                self.log.len()
            )),
            _ => Ok(()),
        }
    }

    fn controlla(&self) {
        if let Err(e) = self.verifica_invarianti() {
            debug_assert!(false, "{e}");
        }
    }

    pub fn uscite(&mut self) -> Vec<Uscita<P>> {
        std::mem::take(&mut self.uscite)
    }

    // ------------------------------------------------------------------ inputs

    pub fn connesso(&mut self, ora: u64, p: P) {
        self.ora = ora;
        self.peers.insert(p.clone(), None);
        self.invia(
            Destinatario::Uno(p),
            Messaggio::Ciao {
                versione: VERSIONE_PROTOCOLLO,
                io: self.io.clone(),
                replica: self.tavolo.is_some(),
            },
        );
    }

    pub fn disconnesso(&mut self, ora: u64, p: &P) {
        self.ora = ora;
        if self.peers.remove(p).is_none() {
            return;
        }
        let host_presente = self
            .host
            .as_ref()
            .is_some_and(|h| self.connessi().contains(h));
        if !host_presente {
            self.elezione();
        }
        self.ripianifica();
    }

    /// Drops every connection (the transport was rebuilt, e.g. after the device slept).
    pub fn scollega_tutti(&mut self, ora: u64) {
        let peers: Vec<P> = self.peers.keys().cloned().collect();
        for p in peers {
            self.disconnesso(ora, &p);
        }
    }

    pub fn tick(&mut self, ora: u64) {
        self.ora = ora;
        while self.e_host() {
            if let Some((t, _, piano)) = self.piano
                && t <= ora
            {
                self.piano = None;
                self.esegui_piano(piano);
            } else if let Some((k, t)) = self.timer.clone()
                && t <= ora
            {
                self.timer = None;
                self.scadenza(k);
            } else {
                break;
            }
        }
        self.controlla();
    }

    /// A request of the local user. Returns the request id (refusals arrive as [`Uscita::Rifiutata`]).
    pub fn richiedi(&mut self, ora: u64, r: Richiesta) -> u64 {
        self.ora = ora;
        self.rid += 1;
        if self.e_host() {
            let id = self.io.id.clone();
            if let Err(e) = self.esegui(&id, r) {
                self.uscite.push(Uscita::Rifiutata { motivo: e.0 });
            }
        } else if let Some(p) = self.peer_host() {
            self.invia(
                Destinatario::Uno(p),
                Messaggio::Richiesta {
                    rid: self.rid,
                    richiesta: r,
                },
            );
        } else {
            self.uscite.push(Uscita::Rifiutata {
                motivo: "Nessun host raggiungibile".into(),
            });
        }
        self.controlla();
        self.rid
    }

    /// A cosmetic effect of the local user (phrase, card drag): sent to everyone directly.
    pub fn effimero(&mut self, ora: u64, effetto: Effimero) -> Result<(), Errore> {
        self.ora = ora;
        self.valida_effimero(&self.io.id, &effetto)?;
        self.invia(Destinatario::Tutti, Messaggio::Effimero(effetto.clone()));
        self.uscite.push(Uscita::Effimero {
            da: self.io.id.clone(),
            effetto,
        });
        Ok(())
    }

    pub fn ricevi(&mut self, ora: u64, p: P, msg: Messaggio) {
        self.ora = ora;
        self.ricevi_messaggio(p, msg);
        self.controlla();
    }

    fn ricevi_messaggio(&mut self, p: P, msg: Messaggio) {
        if !self.peers.contains_key(&p) {
            return;
        }
        if let Messaggio::Ciao {
            versione,
            io,
            replica,
        } = msg
        {
            if versione == VERSIONE_PROTOCOLLO {
                self.ciao(p, io, replica);
            } else {
                self.versione_altrui = Some(versione);
            }
            return;
        }
        // everything else only from peers that introduced themselves
        let Some(Some(remoto)) = self.peers.get_mut(&p) else {
            return;
        };
        if matches!(
            msg,
            Messaggio::Voce { .. } | Messaggio::Istantanea { .. } | Messaggio::Host { .. }
        ) {
            remoto.replica = true;
        }
        let da = remoto.id.clone();
        match msg {
            Messaggio::Ciao { .. } => unreachable!(),
            Messaggio::Richiesta { rid, richiesta } => {
                if self.e_host()
                    && let Err(e) = self.esegui(&da, richiesta)
                {
                    self.invia(
                        Destinatario::Uno(p),
                        Messaggio::Rifiuto { rid, motivo: e.0 },
                    );
                }
            }
            Messaggio::Rifiuto { motivo, .. } => self.uscite.push(Uscita::Rifiutata { motivo }),
            Messaggio::Voce {
                epoca,
                n,
                voce,
                impronta,
            } => self.voce_ricevuta(p, da, epoca, n, voce, impronta),
            Messaggio::Istantanea {
                epoca,
                host,
                tavolo,
            } => self.istantanea_ricevuta(epoca, host, *tavolo),
            Messaggio::Host { epoca, n } => self.host_ricevuto(p, da, epoca, n),
            Messaggio::ChiediDa { da: da_n } => self.chiedi_da(p, da_n),
            Messaggio::Effimero(e) => {
                if self.valida_effimero(&da, &e).is_ok() {
                    self.uscite.push(Uscita::Effimero { da, effetto: e });
                }
            }
        }
    }

    // ------------------------------------------------------------------ message handling

    fn ciao(&mut self, p: P, io: Partecipante, replica: bool) {
        let remoto = Remoto {
            id: io.id.clone(),
            replica,
        };
        self.peers.insert(p.clone(), Some(remoto));
        if self.host.is_none() {
            self.elezione();
        }
        if !self.e_host() {
            return;
        }
        let Some(t) = &self.tavolo else { return };
        if t.partecipante(&io.id) != Some(&io) {
            let _ = self.aggiungi(Voce::Entra(io));
        }
        self.manda_istantanea(Destinatario::Uno(p));
        self.ripianifica();
    }

    fn voce_ricevuta(
        &mut self,
        p: P,
        da: IdGiocatore,
        epoca: u64,
        n: u64,
        voce: Voce,
        impronta: String,
    ) {
        if epoca < self.epoca || (epoca == self.epoca && self.host.as_ref() != Some(&da)) {
            return;
        }
        if epoca > self.epoca {
            self.segui(epoca, da);
        }
        let Some(t) = &mut self.tavolo else {
            return self.chiedi(p, 0);
        };
        if n <= t.n {
            return;
        }
        if n > t.n + 1 {
            let da_n = t.n + 1;
            return self.chiedi(p, da_n);
        }
        // commit only a state whose hash matches the host's (I4)
        match t.applicata(&voce) {
            Ok((nuovo, eventi)) if nuovo.impronta() == impronta => {
                *t = nuovo;
                self.log.push((voce.clone(), impronta));
                self.uscite.push(Uscita::Applicata { voce, eventi });
            }
            // diverged or rejected: the host's replica wins
            _ => self.chiedi(p, 0),
        }
    }

    fn chiedi(&mut self, p: P, da: u64) {
        if self.attesa_istantanea && da == 0 {
            return;
        }
        self.attesa_istantanea |= da == 0;
        self.invia(Destinatario::Uno(p), Messaggio::ChiediDa { da });
    }

    fn istantanea_ricevuta(&mut self, epoca: u64, host: IdGiocatore, tavolo: Tavolo) {
        if self.e_host() && host == self.io.id {
            // a follower that was ahead of us when we took over
            if epoca == self.epoca && self.tavolo.as_ref().is_none_or(|t| tavolo.n > t.n) {
                self.adotta(tavolo);
                self.manda_istantanea(Destinatario::Tutti);
                self.ripianifica();
            }
            return;
        }
        let accetta = epoca > self.epoca
            || (epoca == self.epoca
                && (self.host.is_none()
                    || self.host.as_ref() == Some(&host)
                    || self.rango(&host) < self.rango(self.host.as_ref().unwrap())));
        if !accetta {
            return;
        }
        self.segui(epoca, host);
        self.adotta(tavolo);
    }

    fn host_ricevuto(&mut self, p: P, da: IdGiocatore, epoca: u64, n: u64) {
        let accetta = epoca > self.epoca
            || (epoca == self.epoca
                && self
                    .host
                    .as_ref()
                    .is_none_or(|h| self.rango(&da) < self.rango(h)));
        if !accetta {
            if self.e_host() {
                // we are the better host for this epoch: tell the claimer
                let mio_n = self.tavolo.as_ref().map_or(0, |t| t.n);
                self.invia(
                    Destinatario::Uno(p),
                    Messaggio::Host {
                        epoca: self.epoca,
                        n: mio_n,
                    },
                );
            }
            return;
        }
        self.segui(epoca, da.clone());
        let Some(t) = &self.tavolo else {
            return self.chiedi(p, 0);
        };
        if t.n > n {
            let msg = Messaggio::Istantanea {
                epoca,
                host: da,
                tavolo: Box::new(t.clone()),
            };
            self.invia(Destinatario::Uno(p), msg);
        } else if t.n < n {
            let da_n = t.n + 1;
            self.chiedi(p, da_n);
        }
    }

    fn chiedi_da(&mut self, p: P, da: u64) {
        if !self.e_host() {
            return;
        }
        let Some(t) = &self.tavolo else { return };
        if da > t.n {
            return;
        }
        if da > self.log_base {
            let voci: Vec<_> = self.log[(da - self.log_base - 1) as usize..]
                .iter()
                .enumerate()
                .map(|(i, (voce, impronta))| Messaggio::Voce {
                    epoca: self.epoca,
                    n: da + i as u64,
                    voce: voce.clone(),
                    impronta: impronta.clone(),
                })
                .collect();
            for m in voci {
                self.invia(Destinatario::Uno(p.clone()), m);
            }
        } else {
            self.manda_istantanea(Destinatario::Uno(p));
        }
    }

    fn valida_effimero(&self, id: &str, e: &Effimero) -> Result<(), Errore> {
        let t = self
            .tavolo
            .as_ref()
            .ok_or_else(|| Errore("Nessun tavolo".into()))?;
        let st = match (&t.stato, &t.partita) {
            (StatoTavolo::InCorso, Some(st)) => st,
            _ => return Err(Errore("Nessuna partita in corso".into())),
        };
        let seat = match e {
            Effimero::Frase { seat, indice } => {
                if usize::from(*indice) >= FRASI.len() {
                    return Err(Errore("Frase sconosciuta".into()));
                }
                if st.fase == Fase::Scambio {
                    return Err(Errore("Durante lo scambio non si può parlare".into()));
                }
                *seat
            }
            Effimero::Striscia { seat, verso } => {
                if *verso != (*seat + 3) % 4 {
                    return Err(Errore(
                        "Si striscia solo verso il giocatore precedente".into(),
                    ));
                }
                *seat
            }
        };
        if !t.posti_di(id).contains(&seat) {
            return Err(Errore("Non controlli questo posto".into()));
        }
        Ok(())
    }

    // ------------------------------------------------------------------ host role

    fn rango(&self, id: &str) -> usize {
        self.tavolo
            .as_ref()
            .and_then(|t| t.partecipanti.iter().position(|p| p.id == id))
            .unwrap_or(usize::MAX)
    }

    fn peer_host(&self) -> Option<P> {
        let h = self.host.as_ref()?;
        self.peers
            .iter()
            .find(|(_, r)| r.as_ref().is_some_and(|r| &r.id == h))
            .map(|(p, _)| p.clone())
    }

    /// Follows `host` from `epoca` on (stepping down if we were host).
    fn segui(&mut self, epoca: u64, host: IdGiocatore) {
        self.epoca = epoca;
        self.host = Some(host);
        if !self.e_host() {
            self.piano = None;
            self.timer = None;
        }
    }

    /// The host is gone: the first connected participant in join order takes over.
    /// A peer with nobody around waits instead, and runs the election again on the next hello.
    fn elezione(&mut self) {
        let Some(t) = &self.tavolo else { return };
        let connessi = self.connessi();
        // only peers holding a replica can host (a freshly opened page has none)
        let mut con_replica: BTreeSet<&IdGiocatore> = self
            .peers
            .values()
            .flatten()
            .filter(|r| r.replica)
            .map(|r| &r.id)
            .collect();
        con_replica.insert(&self.io.id);
        let candidato = t
            .partecipanti
            .iter()
            .find(|p| con_replica.contains(&p.id))
            .map(|p| p.id.clone());
        // alone (e.g. a phone that just woke up): never claim, our replica may be stale
        if connessi.len() > 1 && candidato.as_ref() == Some(&self.io.id) {
            self.epoca += 1;
            self.host = Some(self.io.id.clone());
            let n = t.n;
            self.invia(
                Destinatario::Tutti,
                Messaggio::Host {
                    epoca: self.epoca,
                    n,
                },
            );
        } else {
            self.host = None;
        }
    }

    fn adotta(&mut self, tavolo: Tavolo) {
        self.log.clear();
        self.log_base = tavolo.n;
        self.tavolo = Some(tavolo);
        self.attesa_istantanea = false;
        self.uscite.push(Uscita::Sincronizzata);
        self.controlla();
    }

    fn manda_istantanea(&mut self, a: Destinatario<P>) {
        let (Some(t), Some(host)) = (&self.tavolo, &self.host) else {
            return;
        };
        let msg = Messaggio::Istantanea {
            epoca: self.epoca,
            host: host.clone(),
            tavolo: Box::new(t.clone()),
        };
        for (p, r) in self.peers.iter_mut() {
            if let Some(r) = r
                && (a == Destinatario::Tutti || a == Destinatario::Uno(p.clone()))
            {
                r.replica = true;
            }
        }
        self.invia(a, msg);
    }

    fn invia(&mut self, a: Destinatario<P>, msg: Messaggio) {
        self.uscite.push(Uscita::Invia { a, msg });
    }

    /// Appends an entry to the log (host only), applies it and broadcasts it.
    fn aggiungi(&mut self, voce: Voce) -> Result<(), Errore> {
        let t = self
            .tavolo
            .as_mut()
            .ok_or_else(|| Errore("Nessun tavolo".into()))?;
        let eventi = t.applica(&voce)?;
        let impronta = t.impronta();
        let n = t.n;
        self.log.push((voce.clone(), impronta.clone()));
        self.invia(
            Destinatario::Tutti,
            Messaggio::Voce {
                epoca: self.epoca,
                n,
                voce: voce.clone(),
                impronta,
            },
        );
        self.uscite.push(Uscita::Applicata { voce, eventi });
        self.ripianifica();
        Ok(())
    }

    /// Checks who asked and turns a request into a log entry.
    fn esegui(&mut self, id: &IdGiocatore, r: Richiesta) -> Result<(), Errore> {
        let t = self
            .tavolo
            .as_ref()
            .ok_or_else(|| Errore("Nessun tavolo".into()))?;
        let voce = match r {
            Richiesta::Siedi { posto } => Voce::Siedi {
                id: id.clone(),
                posto,
            },
            Richiesta::Alzati { posto } => {
                let tuo = t.posti_di(id).contains(&posto);
                if !(tuo || t.e_bot(posto) || id == t.creatore()) {
                    return Err(Errore("Non puoi liberare questo posto".into()));
                }
                Voce::Alzati { posto }
            }
            Richiesta::Bot { posto, aggiungi } => Voce::Bot { posto, aggiungi },
            Richiesta::Imposta(imp) => Voce::Imposta(imp),
            Richiesta::Avvia => Voce::Avvia {
                seed: u64::from(self.rng.next_u32() >> 2),
            },
            Richiesta::Azione(a) => {
                let st = match (&t.stato, &t.partita) {
                    (StatoTavolo::InCorso, Some(st)) => st,
                    _ => return Err(Errore("Nessuna partita in corso".into())),
                };
                if self.in_pausa() {
                    return Err(Errore(
                        "Partita in pausa: in attesa di riconnessione".into(),
                    ));
                }
                let seat = a
                    .seat()
                    .ok_or_else(|| Errore("Azione riservata al sistema".into()))?;
                if !t.posti_di(id).contains(&seat) {
                    return Err(Errore("Non controlli questo posto".into()));
                }
                if let Azione::ChePalo { .. } = a {
                    if st.fase == Fase::Scambio {
                        return Err(Errore("Durante lo scambio non si può parlare".into()));
                    }
                    let s = usize::from(seat);
                    if self.ultimo_palo[s].is_some_and(|u| self.ora < u + self.config.pausa_palo) {
                        return Ok(()); // anti-spam: ignored
                    }
                    self.ultimo_palo[s] = Some(self.ora);
                }
                Voce::Azione(a)
            }
            Richiesta::Prosegui => {
                if !t.umani().contains(id) {
                    return Err(Errore("Non sei seduto al tavolo".into()));
                }
                if t.ack.contains(id) {
                    return Ok(());
                }
                Voce::Prosegui { id: id.clone() }
            }
            Richiesta::Termina => {
                if id != t.creatore() && !t.umani().contains(id) {
                    return Err(Errore(
                        "Solo chi è seduto al tavolo può terminare la partita".into(),
                    ));
                }
                Voce::Termina { da: id.clone() }
            }
        };
        self.aggiungi(voce)
    }

    /// Recomputes the next bot move / system step and the running timer (host only).
    fn ripianifica(&mut self) {
        let attivo = self.e_host()
            && !self.in_pausa()
            && self
                .tavolo
                .as_ref()
                .is_some_and(|t| t.stato == StatoTavolo::InCorso);
        if !attivo {
            self.piano = None;
            self.timer = None;
            return;
        }
        let t = self.tavolo.as_ref().unwrap();
        let st = t.partita.as_ref().unwrap();

        let piano = if st.fase == Fase::FineMano {
            let umani = t.umani();
            umani.is_subset(&t.ack).then(|| {
                let attesa = if umani.is_empty() {
                    self.config.pausa_fine_mano
                } else {
                    0
                };
                (attesa, Piano::NuovaMano)
            })
        } else {
            bot_da_far_agire(t).map(|(s, attende)| {
                let ritmo = t.impostazioni.ritmo;
                let (lo, hi) = self.config.ritardo_bot;
                let (lo, hi) = (ritmo.scala(lo), ritmo.scala(hi));
                let mut r = lo + (self.rng.random() * (hi - lo) as f64) as u64;
                if attende && st.fase != Fase::Scambio {
                    r = r.min(300);
                }
                (r, Piano::Bot(s))
            })
        };
        let n = t.n;
        self.piano = match (self.piano, piano) {
            (Some(vecchio @ (_, vn, vp)), Some((_, p))) if vn == n && vp == p => Some(vecchio),
            (_, Some((attesa, p))) => Some((self.ora + attesa, n, p)),
            (_, None) => None,
        };

        let chiave = chiave_timer(t);
        if chiave.as_ref() != self.timer.as_ref().map(|t| &t.0) {
            self.timer = chiave.map(|k| {
                let secondi = match k.tipo {
                    TipoTimer::Scambio => t.impostazioni.timer_scambio,
                    TipoTimer::Giocata => t.impostazioni.timer_giocata,
                };
                (k, self.ora + u64::from(secondi) * 1000)
            });
        }
    }

    /// Deterministic per-decision seed, so a bot move does not depend on who is hosting.
    fn seme_decisione(&self) -> u64 {
        let t = self.tavolo.as_ref().unwrap();
        let seed = t.partita.as_ref().map_or(0, |p| p.seed);
        seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ t.n
    }

    fn esegui_piano(&mut self, piano: Piano) {
        let t = self.tavolo.as_ref().unwrap();
        let st = t.partita.as_ref().unwrap();
        let voce = match piano {
            Piano::NuovaMano => st.system_action().map(Voce::Azione),
            Piano::Bot(s) => {
                let legal = st.legal_actions(s);
                (!legal.is_empty()).then(|| {
                    let mut agente = HeuristicAgent::new(self.seme_decisione());
                    Voce::Azione(agente.choose_action(&st.observation(s), &legal))
                })
            }
        };
        match voce {
            Some(v) => self.aggiungi(v).expect("bot/system actions are legal"),
            None => self.ripianifica(),
        }
    }

    /// A turn timer expired: act on the late players' behalf.
    fn scadenza(&mut self, k: ChiaveTimer) {
        let t = self.tavolo.as_ref().unwrap();
        let mut st = t.partita.clone().unwrap();
        let mut rng = Mulberry32::new(self.seme_decisione());
        let mut azioni = vec![];
        match k.tipo {
            TipoTimer::Scambio => {
                // whoever has not passed yet passes a random card
                for s in 0..4 {
                    if st.scambio_passata[usize::from(s)].is_some() {
                        continue;
                    }
                    let passe: Vec<Azione> = st
                        .legal_actions(s)
                        .into_iter()
                        .filter(|a| matches!(a, Azione::Passa { veloce: false, .. }))
                        .collect();
                    if !passe.is_empty() {
                        let a = rng.choice(&passe).clone();
                        st.apply(&a).expect("legal");
                        azioni.push(a);
                    }
                }
            }
            TipoTimer::Giocata => {
                let s = k.seat.expect("play timer has a seat");
                let gioca: Vec<Azione> = st
                    .legal_actions(s)
                    .into_iter()
                    .filter(|a| matches!(a, Azione::Gioca { .. }))
                    .collect();
                if !gioca.is_empty() {
                    let mut agente = HeuristicAgent::new(self.seme_decisione());
                    azioni.push(agente.choose_action(&st.observation(s), &gioca));
                }
            }
        }
        if azioni.is_empty() {
            return self.ripianifica();
        }
        self.aggiungi(Voce::Scaduto {
            timer: k.tipo,
            seat: k.seat,
            azioni,
        })
        .expect("timeout actions are legal");
    }
}

/// The bot seat that should act next, and whether it may just wait.
fn bot_da_far_agire(t: &Tavolo) -> Option<(Seat, bool)> {
    let st = t.partita.as_ref()?;
    let candidati = st.optional_actors().into_iter().chain(st.required_actors());
    for s in candidati {
        if !t.e_bot(s) {
            continue;
        }
        let i = usize::from(s);
        if st.fase == Fase::Scambio
            && st.scambio_attesa[i]
            && st.scambio_passata[usize::from(compagno(s))].is_none()
        {
            continue;
        }
        let legal = st.legal_actions(s);
        if !legal.is_empty() {
            let attende = legal.iter().any(|a| matches!(a, Azione::Attendi { .. }));
            return Some((s, attende));
        }
    }
    None
}

fn chiave_timer(t: &Tavolo) -> Option<ChiaveTimer> {
    let st = t.partita.as_ref()?;
    let imp = &t.impostazioni;
    if st.fase == Fase::Scambio && imp.timer_scambio > 0 {
        return Some(ChiaveTimer {
            tipo: TipoTimer::Scambio,
            mano: st.mano_n,
            turno: st.turno,
            giocate: 0,
            seat: None,
        });
    }
    if st.fase == Fase::Gioco && imp.timer_giocata > 0 && !t.e_bot(st.corrente) {
        return Some(ChiaveTimer {
            tipo: TipoTimer::Giocata,
            mano: st.mano_n,
            turno: st.turno,
            giocate: st.giocate_mano.len(),
            seat: Some(st.corrente),
        });
    }
    None
}
