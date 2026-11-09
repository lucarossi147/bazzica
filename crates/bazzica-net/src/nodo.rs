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
//!
//! Untrusted input: anything decoded from the network may be corrupt or forged, and must not
//! crash the node or its UI. Snapshots are checked ([`Tavolo::valida`]: every card once, seats
//! in range) before adoption; entries naming a seat that does not exist are refused; wire seats
//! are checked before any arithmetic ([`posto`]); a peer holding a replica follows a host claim,
//! an entry or a snapshot of a new epoch only from a participant of the table and at most
//! `SALTO_MAX_EPOCA` epochs ahead. Identities are still self-declared (messages are not signed).
//!
//! Requests: a follower keeps each request until the host acknowledges it (`Ricevuta`, sent
//! after the entry) or refuses it (`Rifiuto`), and resends it with the same id to the new host
//! after a takeover or after [`Config::attesa_richiesta_ms`]. The host ignores ids it already
//! handled for that session (acknowledging them again). A request identical to a pending one
//! (a double tap) is not sent again, and a move that is no longer legal locally is dropped.
//!
//! Elections: candidates are peers known to hold a replica (`Ciao.replica`, refreshed by
//! `HoReplica` when a peer gets its first snapshot). A peer claims only with a strict majority
//! of the participants the host last reported connected (`Presenti`, sent by the host whenever
//! that set changes), so two sides of a split network never both elect. Claims are not relayed:
//! in a partial mesh (two peers that reach the host but not each other) the table may stay
//! without a host until a link comes back; the app then says it cannot reach the host.
//!
//! Sessions: an identity may be open in two places (two tabs, a reload before the old link
//! timed out). Each node has a [`Sessione`] (birth time, then a nonce) sent in `Ciao`; a peer
//! that sees two sessions of one identity keeps the newer, forgets the older and sends it
//! `Sostituito`, after which that node stops acting until [`Nodo::riprendi`]. The host is a
//! [`Regia`] (identity and session), so a snapshot naming another session of our identity never
//! makes us host: a host that reloads is replaced through a normal election.
//!
//! Sync barrier: a peer that wins an election sequences nothing (no bot plan, no timer, no
//! request, no `Entra`) until every introduced peer answered its `Host` claim with `Allineato`
//! (sent after the peer's longer snapshot or catch-up request) or
//! [`Config::attesa_allineamento_ms`] passed. Requests that arrive meanwhile are queued, not
//! refused. A peer that reported the same length with another hash gets the host's snapshot
//! (the host's replica wins ties). A follower that receives an entry at a position it already
//! holds compares the hashes and resyncs on a difference.
//!
//! Resync: a follower whose replica mismatches the host's hash asks for a snapshot
//! (`ChiediDa { da: 0 }`) and stops asking for missing entries until it arrives. An unanswered
//! request is sent again after [`Config::attesa_istantanea_ms`] to whoever hosts by then, and is
//! dropped when the host changes or the asked peer disconnects. A non-host that receives
//! `ChiediDa` ignores it: the retry reaches the right host, so no extra message is needed.

use std::collections::{BTreeMap, BTreeSet};

use bazzica_core::bots::{Agent, HeuristicAgent};
use bazzica_core::game::compagno;
use bazzica_core::{Azione, Evento, Fase, Mulberry32, Seat};

use crate::protocollo::{Effimero, Messaggio, Regia, Richiesta, Sessione, VERSIONE_PROTOCOLLO};
use crate::tavolo::{
    Errore, FRASI, IdGiocatore, Impostazioni, Partecipante, Posto, StatoTavolo, Tavolo, TipoTimer,
    Voce, normalizza, permesso,
};

/// How far ahead of ours an epoch may be (a host claim, an entry, a snapshot).
const SALTO_MAX_EPOCA: u64 = 16;

/// Why a superseded node refuses everything.
const SOSTITUITO: &str = "Questa partita è aperta in un'altra scheda";

#[derive(Clone, Debug)]
pub struct Config {
    /// Bot "thinking" time range at the normal pace, in ms (the table's [`Ritmo`] scales it).
    pub ritardo_bot: (u64, u64),
    /// Pause before the next hand in bot-only games, in ms.
    pub pausa_fine_mano: u64,
    /// Minimum ms between two "che palo" calls of the same seat.
    pub pausa_palo: u64,
    /// How long a snapshot request may go unanswered before it is sent again, in ms.
    pub attesa_istantanea_ms: u64,
    /// How long a new host waits for its peers' `Allineato` before sequencing, in ms.
    pub attesa_allineamento_ms: u64,
    /// How long a request may go unacknowledged before it is sent again, in ms.
    pub attesa_richiesta_ms: u64,
    /// How long a seated human must be away before a bot may take their seat, in ms.
    pub attesa_sostituzione_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            ritardo_bot: (1300, 2200),
            pausa_fine_mano: 2000,
            pausa_palo: 3000,
            attesa_istantanea_ms: 3000,
            attesa_allineamento_ms: 2000,
            attesa_richiesta_ms: 3000,
            attesa_sostituzione_ms: 60_000,
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
    /// A newer session of this identity took over: see [`Nodo::sostituito`].
    Sostituita,
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
    sessione: Sessione,
    /// Whether it holds a replica (only those can take over as host).
    replica: bool,
}

/// A freshly elected host waiting for its peers' replicas before sequencing anything.
#[derive(Clone, Debug)]
struct Allineamento<P> {
    /// Peers that have not answered the claim yet.
    attese: BTreeSet<P>,
    /// When the host stops waiting, in ms.
    scadenza: u64,
    /// What each peer reported: its replica length and hash.
    rapporti: Vec<(P, u64, String)>,
    /// Work that would sequence entries, done when the barrier ends.
    in_coda: Vec<InCoda<P>>,
}

#[derive(Clone, Debug)]
enum InCoda<P> {
    /// A request from a peer (`None`: the local user).
    Richiesta {
        da: Option<P>,
        rid: u64,
        id: IdGiocatore,
        richiesta: Richiesta,
    },
    /// A participant said hello and may need an `Entra` entry.
    Entra(Partecipante),
}

/// A request of the local user sent to the host and not acknowledged yet.
#[derive(Clone, Debug)]
struct Pendente {
    richiesta: Richiesta,
    /// When it was last sent, in ms.
    inviata: u64,
}

/// A snapshot asked for and not received yet.
#[derive(Clone, Debug)]
struct RichiestaIstantanea<P> {
    /// The peer asked.
    a: P,
    epoca: u64,
    /// When it was asked, in ms.
    dal: u64,
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
    /// This run of the app; `nata` is set by the first input.
    sessione: Sessione,
    /// A newer session of our identity took over: this node stops acting.
    sostituito: bool,
    /// Since when each seated human has been away (ms), by this node's observation.
    assenti_dal: BTreeMap<IdGiocatore, u64>,
    /// Who the host last reported connected (itself included): elections need a strict
    /// majority of it. Empty until the first report.
    presenti: BTreeSet<IdGiocatore>,
    tavolo: Option<Tavolo>,
    /// Applied entries `log_base+1 ..= log_base+log.len()`, kept to resend to late peers.
    log: Vec<(Voce, String)>,
    log_base: u64,
    epoca: u64,
    host: Option<Regia>,
    /// Connected peers and, once they said hello, who they are.
    peers: BTreeMap<P, Option<Remoto>>,
    rid: u64,
    /// Requests sent to the host and not acknowledged, by request id.
    pendenti: BTreeMap<u64, Pendente>,
    attesa_istantanea: Option<RichiestaIstantanea<P>>,
    // host only
    /// Last request id handled per session of each participant, to ignore duplicates.
    ultimi_rid: BTreeMap<Regia, u64>,
    /// Set from a won election until the peers answered: nothing is sequenced meanwhile.
    allineamento: Option<Allineamento<P>>,
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
        n.host = Some(n.regia());
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
            // not from `rng`, so bot timings stay as they were
            sessione: Sessione {
                nata: 0,
                nonce: seme.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(17),
            },
            sostituito: false,
            presenti: BTreeSet::new(),
            assenti_dal: BTreeMap::new(),
            tavolo: None,
            log: vec![],
            log_base: 0,
            epoca: 0,
            host: None,
            peers: BTreeMap::new(),
            rid: 0,
            pendenti: BTreeMap::new(),
            ultimi_rid: BTreeMap::new(),
            attesa_istantanea: None,
            allineamento: None,
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
        self.host.as_ref().map(|h| &h.id)
    }

    /// The host with its session (which of its tabs hosts).
    pub fn host_regia(&self) -> Option<&Regia> {
        self.host.as_ref()
    }

    /// Ids of the peers this node counts as possible hosts (they hold a replica).
    pub fn peer_con_replica(&self) -> BTreeSet<IdGiocatore> {
        self.peers
            .values()
            .flatten()
            .filter(|r| r.replica)
            .map(|r| r.id.clone())
            .collect()
    }

    /// Set when another tab or page of the same identity took over this table.
    pub fn sostituito(&self) -> bool {
        self.sostituito
    }

    fn regia(&self) -> Regia {
        Regia {
            id: self.io.id.clone(),
            sessione: self.sessione,
        }
    }

    pub fn epoca(&self) -> u64 {
        self.epoca
    }

    pub fn e_host(&self) -> bool {
        !self.sostituito && self.host.as_ref() == Some(&self.regia())
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

    /// Seated humans away long enough for a bot to take their seat.
    pub fn sostituibili(&self) -> Vec<IdGiocatore> {
        self.disconnessi()
            .into_iter()
            .filter(|id| {
                self.assenti_dal
                    .get(id)
                    .is_some_and(|&dal| self.ora >= dal + self.config.attesa_sostituzione_ms)
            })
            .collect()
    }

    /// Keeps track of since when each seated human is away.
    fn aggiorna_assenze(&mut self) {
        let assenti: BTreeSet<IdGiocatore> = self.disconnessi().into_iter().collect();
        self.assenti_dal.retain(|id, _| assenti.contains(id));
        for id in assenti {
            self.assenti_dal.entry(id).or_insert(self.ora);
        }
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
        let istantanea = self
            .attesa_istantanea
            .as_ref()
            .map(|r| r.dal + self.config.attesa_istantanea_ms);
        [
            self.piano.map(|p| p.0),
            self.timer.as_ref().map(|t| t.1),
            istantanea,
            self.allineamento.as_ref().map(|a| a.scadenza),
            self.pendenti
                .values()
                .map(|x| x.inviata + self.config.attesa_richiesta_ms)
                .min(),
        ]
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

    /// Every input starts here: the clock, and this session's birth time on the first one.
    fn orologio(&mut self, ora: u64) {
        self.ora = ora;
        if self.sessione.nata == 0 {
            let prima = self.regia();
            self.sessione.nata = ora.max(1);
            if self.host.as_ref() == Some(&prima) {
                self.host = Some(self.regia());
            }
        }
    }

    pub fn connesso(&mut self, ora: u64, p: P) {
        self.orologio(ora);
        self.peers.insert(p.clone(), None);
        self.invia(
            Destinatario::Uno(p),
            Messaggio::Ciao {
                versione: VERSIONE_PROTOCOLLO,
                io: self.io.clone(),
                sessione: self.sessione,
                replica: self.tavolo.is_some(),
            },
        );
    }

    pub fn disconnesso(&mut self, ora: u64, p: &P) {
        self.orologio(ora);
        self.scollega(p);
        self.aggiorna_assenze();
    }

    /// Forgets peer `p` (gone, or superseded by a newer session of the same identity).
    fn scollega(&mut self, p: &P) {
        if self.peers.remove(p).is_none() {
            return;
        }
        if self.attesa_istantanea.as_ref().is_some_and(|r| &r.a == p) {
            self.attesa_istantanea = None;
        }
        let host_presente = self.host.as_ref().is_some_and(|h| self.raggiungibile(h));
        if !host_presente {
            self.elezione();
        }
        self.aggiorna_presenti();
        if let Some(a) = &mut self.allineamento {
            a.attese.remove(p);
            if a.attese.is_empty() {
                return self.fine_allineamento();
            }
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
        self.orologio(ora);
        self.aggiorna_assenze();
        if self.sostituito {
            return;
        }
        self.ripeti_istantanea();
        self.ripeti_richieste(false);
        if self
            .allineamento
            .as_ref()
            .is_some_and(|a| ora >= a.scadenza)
        {
            self.fine_allineamento();
        }
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
        self.orologio(ora);
        self.rid += 1;
        if self.sostituito {
            self.uscite.push(Uscita::Rifiutata {
                motivo: SOSTITUITO.into(),
            });
        } else if self.e_host() {
            let id = self.io.id.clone();
            self.richiesta_al_host(None, self.rid, id, r);
        } else if self.host.is_none() {
            self.uscite.push(Uscita::Rifiutata {
                motivo: "Nessun host raggiungibile".into(),
            });
        } else if let Some((&rid, _)) = self.pendenti.iter().find(|(_, x)| x.richiesta == r) {
            // a double tap: the same request is already on its way
            return rid;
        } else {
            // sent now if the host is reachable, else when it is (see `ripeti_richieste`)
            self.pendenti.insert(
                self.rid,
                Pendente {
                    richiesta: r.clone(),
                    inviata: self.ora,
                },
            );
            if let Some(p) = self.peer_host() {
                self.invia(
                    Destinatario::Uno(p),
                    Messaggio::Richiesta {
                        rid: self.rid,
                        richiesta: r,
                    },
                );
            }
        }
        self.controlla();
        self.rid
    }

    /// Requests sent to the host and not acknowledged yet (for an "invio…" hint in the UI).
    pub fn richieste_in_corso(&self) -> usize {
        self.pendenti.len()
    }

    /// Sends the pending requests again (same ids) to the current host: all of them when
    /// `tutte`, else those unacknowledged for [`Config::attesa_richiesta_ms`]. Drops the ones
    /// that no longer make sense (a move that is not legal any more, e.g. the timer played it).
    fn ripeti_richieste(&mut self, tutte: bool) {
        let superate: Vec<u64> = self
            .pendenti
            .iter()
            .filter(|(_, x)| !ancora_sensata(self.tavolo.as_ref(), &x.richiesta))
            .map(|(rid, _)| *rid)
            .collect();
        for rid in superate {
            self.pendenti.remove(&rid);
        }
        let Some(p) = self.peer_host() else { return };
        let (ora, attesa) = (self.ora, self.config.attesa_richiesta_ms);
        let mut da_mandare = vec![];
        for (rid, x) in self.pendenti.iter_mut() {
            if tutte || ora >= x.inviata + attesa {
                x.inviata = ora;
                da_mandare.push(Messaggio::Richiesta {
                    rid: *rid,
                    richiesta: x.richiesta.clone(),
                });
            }
        }
        for m in da_mandare {
            self.invia(Destinatario::Uno(p.clone()), m);
        }
    }

    /// A cosmetic effect of the local user (phrase, card drag): sent to everyone directly.
    pub fn effimero(&mut self, ora: u64, effetto: Effimero) -> Result<(), Errore> {
        self.orologio(ora);
        if self.sostituito {
            return Err(Errore(SOSTITUITO.into()));
        }
        self.valida_effimero(&self.io.id, &effetto)?;
        self.invia(Destinatario::Tutti, Messaggio::Effimero(effetto.clone()));
        self.uscite.push(Uscita::Effimero {
            da: self.io.id.clone(),
            effetto,
        });
        Ok(())
    }

    pub fn ricevi(&mut self, ora: u64, p: P, msg: Messaggio) {
        self.orologio(ora);
        self.ricevi_messaggio(p, msg);
        self.aggiorna_assenze();
        self.controlla();
    }

    fn ricevi_messaggio(&mut self, p: P, msg: Messaggio) {
        if !self.peers.contains_key(&p) {
            return;
        }
        if let Messaggio::Sostituito = msg {
            return self.sostituisci_me();
        }
        if self.sostituito {
            return;
        }
        if let Messaggio::Ciao {
            versione,
            io,
            sessione,
            replica,
        } = msg
        {
            if versione == VERSIONE_PROTOCOLLO {
                self.ciao(p, io, sessione, replica);
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
            Messaggio::Voce { .. }
                | Messaggio::Istantanea { .. }
                | Messaggio::Host { .. }
                | Messaggio::HoReplica
        ) {
            remoto.replica = true;
        }
        let da = remoto.id.clone();
        let regia = Regia {
            id: da.clone(),
            sessione: remoto.sessione,
        };
        match msg {
            Messaggio::Ciao { .. } | Messaggio::Sostituito => unreachable!(),
            Messaggio::Richiesta { rid, richiesta } => {
                if !self.e_host() {
                    return;
                }
                // a resend of a request already handled: acknowledge it again, nothing else
                let ultimo = self.ultimi_rid.entry(regia).or_default();
                if rid <= *ultimo {
                    return self.invia(Destinatario::Uno(p), Messaggio::Ricevuta { rid });
                }
                *ultimo = rid;
                self.richiesta_al_host(Some(p), rid, da, richiesta);
            }
            Messaggio::Ricevuta { rid } => {
                self.pendenti.remove(&rid);
            }
            Messaggio::Rifiuto { rid, motivo } => {
                self.pendenti.remove(&rid);
                self.uscite.push(Uscita::Rifiutata { motivo });
            }
            Messaggio::Voce {
                epoca,
                n,
                voce,
                impronta,
            } => self.voce_ricevuta(p, regia, epoca, n, voce, impronta),
            Messaggio::Istantanea {
                epoca,
                host,
                tavolo,
            } => self.istantanea_ricevuta(epoca, host, *tavolo),
            Messaggio::Host { epoca, n } => self.host_ricevuto(p, regia, epoca, n),
            Messaggio::ChiediDa { da: da_n } => self.chiedi_da(p, da_n),
            Messaggio::Allineato { epoca, n, impronta } => {
                self.allineato_ricevuto(p, epoca, n, impronta)
            }
            Messaggio::HoReplica => {}
            Messaggio::Presenti { ids } => {
                if self.host.as_ref() == Some(&regia) {
                    self.presenti = ids;
                }
            }
            Messaggio::Effimero(e) => {
                if self.valida_effimero(&da, &e).is_ok() {
                    self.uscite.push(Uscita::Effimero { da, effetto: e });
                }
            }
        }
    }

    /// A newer session of our identity is here: stop acting until [`Nodo::riprendi`].
    fn sostituisci_me(&mut self) {
        self.sostituito = true;
        self.uscite.push(Uscita::Sostituita);
        self.host = None;
        self.piano = None;
        self.timer = None;
        self.allineamento = None;
        self.attesa_istantanea = None;
    }

    /// Takes the table back after [`Nodo::sostituito`]: this becomes the newest session, so
    /// once the driver reconnects, the peers drop the other one.
    pub fn riprendi(&mut self, ora: u64) {
        self.orologio(ora);
        self.sostituito = false;
        self.sessione = Sessione {
            nata: ora.max(self.sessione.nata + 1),
            nonce: self.sessione.nonce,
        };
    }

    /// Whether `h` is this session or a connected peer in that very session.
    fn raggiungibile(&self, h: &Regia) -> bool {
        *h == self.regia()
            || self
                .peers
                .values()
                .flatten()
                .any(|r| r.id == h.id && r.sessione == h.sessione)
    }

    // ------------------------------------------------------------------ message handling

    fn ciao(&mut self, p: P, io: Partecipante, sessione: Sessione, replica: bool) {
        // one live session per identity: the newest wins
        if io.id == self.io.id {
            if sessione > self.sessione {
                self.sostituisci_me();
            } else if sessione < self.sessione {
                self.invia(Destinatario::Uno(p), Messaggio::Sostituito);
            }
            return;
        }
        let doppione = self.peers.iter().find_map(|(q, r)| {
            r.as_ref()
                .filter(|r| *q != p && r.id == io.id && r.sessione != sessione)
                .map(|r| (q.clone(), r.sessione))
        });
        if let Some((q, vecchia)) = doppione {
            if sessione < vecchia {
                // p is the stale one: never introduce it
                return self.invia(Destinatario::Uno(p), Messaggio::Sostituito);
            }
            self.invia(Destinatario::Uno(q.clone()), Messaggio::Sostituito);
            self.scollega(&q);
        }
        let remoto = Remoto {
            id: io.id.clone(),
            sessione,
            replica,
        };
        self.peers.insert(p.clone(), Some(remoto));
        if self.host.is_none() {
            self.elezione();
        }
        if !self.e_host() {
            return;
        }
        self.aggiorna_presenti();
        if self.tavolo.is_none() {
            return;
        }
        match &mut self.allineamento {
            Some(a) => a.in_coda.push(InCoda::Entra(io)),
            None => self.entra(io),
        }
        self.manda_istantanea(Destinatario::Uno(p));
        self.ripianifica();
    }

    /// Records a participant that said hello, if it is new or changed (host only).
    fn entra(&mut self, io: Partecipante) {
        if self.tavolo.as_ref().is_some_and(|t| {
            let attuale = t.partecipante(&io.id);
            attuale != Some(&normalizza(&io, attuale, t.partecipanti.len()))
        }) {
            let _ = self.aggiungi(Voce::Entra(io));
        }
    }

    /// A request reaching the host (`da`: the peer it came from, `None` for the local user):
    /// queued while aligning, otherwise turned into an entry or refused.
    fn richiesta_al_host(
        &mut self,
        da: Option<P>,
        rid: u64,
        id: IdGiocatore,
        richiesta: Richiesta,
    ) {
        if let Some(a) = &mut self.allineamento {
            a.in_coda.push(InCoda::Richiesta {
                da,
                rid,
                id,
                richiesta,
            });
            return;
        }
        // after the entry, so the requester sees it before the acknowledgement
        match (self.esegui(&id, richiesta), da) {
            (Ok(()), Some(p)) => self.invia(Destinatario::Uno(p), Messaggio::Ricevuta { rid }),
            (Ok(()), None) => {}
            (Err(e), Some(p)) => self.invia(
                Destinatario::Uno(p),
                Messaggio::Rifiuto { rid, motivo: e.0 },
            ),
            (Err(e), None) => self.uscite.push(Uscita::Rifiutata { motivo: e.0 }),
        }
    }

    /// A follower answered our claim.
    fn allineato_ricevuto(&mut self, p: P, epoca: u64, n: u64, impronta: String) {
        if !self.e_host() || epoca != self.epoca {
            return;
        }
        let Some(a) = &mut self.allineamento else {
            return;
        };
        a.attese.remove(&p);
        a.rapporti.push((p, n, impronta));
        if a.attese.is_empty() {
            self.fine_allineamento();
        }
    }

    /// Everyone answered (or the wait expired): fix the peers that hold another replica of
    /// the same length, then do the queued work and start sequencing.
    fn fine_allineamento(&mut self) {
        let Some(a) = self.allineamento.take() else {
            return;
        };
        if let Some(t) = &self.tavolo {
            let (mio_n, mia) = (t.n, t.impronta());
            for (p, n, impronta) in a.rapporti {
                if n == mio_n && impronta != mia {
                    self.manda_istantanea(Destinatario::Uno(p));
                }
            }
        }
        for lavoro in a.in_coda {
            match lavoro {
                InCoda::Richiesta {
                    da,
                    rid,
                    id,
                    richiesta,
                } => self.richiesta_al_host(da, rid, id, richiesta),
                InCoda::Entra(io) => self.entra(io),
            }
        }
        self.ripianifica();
    }

    fn voce_ricevuta(&mut self, p: P, da: Regia, epoca: u64, n: u64, voce: Voce, impronta: String) {
        if epoca < self.epoca || (epoca == self.epoca && self.host.as_ref() != Some(&da)) {
            return;
        }
        if !self.epoca_plausibile(epoca, &da.id) {
            return;
        }
        if epoca > self.epoca {
            self.segui(epoca, da);
        }
        let Some(t) = &mut self.tavolo else {
            return self.chiedi(p, 0);
        };
        if n <= t.n {
            // already applied: but did we apply the same thing?
            let nostra = if n > self.log_base {
                Some(self.log[(n - self.log_base - 1) as usize].1.clone())
            } else {
                (n == t.n).then(|| t.impronta())
            };
            if nostra.is_some_and(|i| i != impronta) {
                self.chiedi(p, 0);
            }
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

    /// Whether `chi` may lead epoch `epoca`: a participant of our replica, and no wild jump
    /// ahead (a peer cannot make itself host forever with `u64::MAX`). Without a replica we
    /// cannot judge, and a table may well be at a high epoch after many takeovers.
    fn epoca_plausibile(&self, epoca: u64, chi: &str) -> bool {
        self.tavolo.as_ref().is_none_or(|t| {
            epoca <= self.epoca.saturating_add(SALTO_MAX_EPOCA) && t.partecipante(chi).is_some()
        })
    }

    /// Asks `p` for the entries from `da` on, or for a snapshot when `da` is 0.
    fn chiedi(&mut self, p: P, da: u64) {
        if da == 0 {
            if self.istantanea_in_arrivo(&p) {
                return;
            }
            self.attesa_istantanea = Some(RichiestaIstantanea {
                a: p.clone(),
                epoca: self.epoca,
                dal: self.ora,
            });
        } else if self.attesa_istantanea.is_some() {
            // a snapshot is coming: entries applied to a diverged replica would only mismatch
            return;
        }
        self.invia(Destinatario::Uno(p), Messaggio::ChiediDa { da });
    }

    /// Whether a snapshot was asked of `p` in this epoch and may still arrive.
    fn istantanea_in_arrivo(&self, p: &P) -> bool {
        self.attesa_istantanea.as_ref().is_some_and(|r| {
            &r.a == p
                && r.epoca == self.epoca
                && self.ora < r.dal + self.config.attesa_istantanea_ms
        })
    }

    /// Sends an expired snapshot request again, to whoever hosts now.
    fn ripeti_istantanea(&mut self) {
        let scaduta = self
            .attesa_istantanea
            .as_ref()
            .is_some_and(|r| self.ora >= r.dal + self.config.attesa_istantanea_ms);
        if !scaduta {
            return;
        }
        self.attesa_istantanea = None;
        if let Some(p) = self.peer_host() {
            self.chiedi(p, 0);
        }
    }

    fn istantanea_ricevuta(&mut self, epoca: u64, host: Regia, tavolo: Tavolo) {
        if tavolo.valida().is_err() || !self.epoca_plausibile(epoca, &host.id) {
            return; // corrupt or forged: it must not crash us later
        }
        // a snapshot naming another session of our id never makes us host
        if self.e_host() && host == self.regia() {
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
                    || self.rango(&host.id) < self.rango(&self.host.as_ref().unwrap().id)));
        if !accetta {
            return;
        }
        self.segui(epoca, host);
        self.adotta(tavolo);
    }

    fn host_ricevuto(&mut self, p: P, da: Regia, epoca: u64, n: u64) {
        if !self.epoca_plausibile(epoca, &da.id) {
            return;
        }
        let accetta = epoca > self.epoca
            || (epoca == self.epoca
                && self
                    .host
                    .as_ref()
                    .is_none_or(|h| self.rango(&da.id) < self.rango(&h.id)));
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
        let (mio_n, mia) = match &self.tavolo {
            None => {
                self.chiedi(p.clone(), 0);
                (0, String::new())
            }
            Some(t) if t.n > n => {
                let msg = Messaggio::Istantanea {
                    epoca,
                    host: da,
                    tavolo: Box::new(t.clone()),
                };
                let stato = (t.n, t.impronta());
                self.invia(Destinatario::Uno(p.clone()), msg);
                stato
            }
            Some(t) => {
                let stato = (t.n, t.impronta());
                if t.n < n {
                    self.chiedi(p.clone(), t.n + 1);
                }
                stato
            }
        };
        // sent after the snapshot or the catch-up request: the channel is ordered
        self.invia(
            Destinatario::Uno(p),
            Messaggio::Allineato {
                epoca,
                n: mio_n,
                impronta: mia,
            },
        );
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
                let seat = posto(*seat).ok_or_else(|| Errore("Posto inesistente".into()))?;
                if *verso != (seat + 3) % 4 {
                    return Err(Errore(
                        "Si striscia solo verso il giocatore precedente".into(),
                    ));
                }
                seat
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
            .find(|(_, r)| {
                r.as_ref()
                    .is_some_and(|r| r.id == h.id && r.sessione == h.sessione)
            })
            .map(|(p, _)| p.clone())
    }

    /// Follows `host` from `epoca` on (stepping down if we were host).
    fn segui(&mut self, epoca: u64, host: Regia) {
        let nuovo = self.host.as_ref() != Some(&host);
        if (epoca, Some(&host)) != (self.epoca, self.host.as_ref()) {
            self.attesa_istantanea = None;
        }
        self.epoca = epoca;
        self.host = Some(host);
        if !self.e_host() {
            self.piano = None;
            self.timer = None;
            // queued requests of others are dropped (their senders retry with the new host);
            // our own become pending again
            let in_coda = self.allineamento.take().map(|a| a.in_coda);
            for lavoro in in_coda.into_iter().flatten() {
                if let InCoda::Richiesta {
                    da: None,
                    rid,
                    richiesta,
                    ..
                } = lavoro
                {
                    let inviata = self.ora;
                    self.pendenti.insert(rid, Pendente { richiesta, inviata });
                }
            }
            if nuovo {
                self.ripeti_richieste(true);
            }
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
        // a strict majority of whom the host last saw: two sides of a split never both elect
        let maggioranza = maggioranza(&connessi, &self.presenti);
        // alone (e.g. a phone that just woke up): never claim, our replica may be stale
        if connessi.len() > 1 && maggioranza && candidato.as_ref() == Some(&self.io.id) {
            self.epoca += 1;
            self.host = Some(self.regia());
            let n = t.n;
            // Wait for every introduced peer, not only those flagged with a replica: the
            // flags can be stale, and a peer without a replica answers too (with n = 0).
            self.allineamento = Some(Allineamento {
                attese: self
                    .peers
                    .iter()
                    .filter(|(_, r)| r.is_some())
                    .map(|(p, _)| p.clone())
                    .collect(),
                scadenza: self.ora + self.config.attesa_allineamento_ms,
                rapporti: vec![],
                // our own pending requests are now ours to sequence
                in_coda: std::mem::take(&mut self.pendenti)
                    .into_iter()
                    .map(|(rid, x)| InCoda::Richiesta {
                        da: None,
                        rid,
                        id: self.io.id.clone(),
                        richiesta: x.richiesta,
                    })
                    .collect(),
            });
            self.invia(
                Destinatario::Tutti,
                Messaggio::Host {
                    epoca: self.epoca,
                    n,
                },
            );
            self.aggiorna_presenti();
        } else {
            self.host = None;
            self.allineamento = None;
        }
    }

    /// Host only: tells everyone who it sees connected, when that changed.
    fn aggiorna_presenti(&mut self) {
        if !self.e_host() {
            return;
        }
        let connessi = self.connessi();
        if connessi != self.presenti {
            self.presenti = connessi.clone();
            self.invia(Destinatario::Tutti, Messaggio::Presenti { ids: connessi });
        }
    }

    fn adotta(&mut self, tavolo: Tavolo) {
        self.log.clear();
        self.log_base = tavolo.n;
        if self.tavolo.replace(tavolo).is_none() {
            // peers that met us before we had a replica must learn we can host now
            self.invia(Destinatario::Tutti, Messaggio::HoReplica);
        }
        self.attesa_istantanea = None;
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
        let creatore_presente = self.connessi().contains(t.creatore());
        let host = self.host.as_ref().map(|h| h.id.as_str());
        permesso(t, id, &r, creatore_presente, host)?;
        let voce = match r {
            Richiesta::Siedi { posto } => Voce::Siedi {
                id: id.clone(),
                posto,
            },
            Richiesta::Alzati { posto } => Voce::Alzati { posto },
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
                let Some(seat) = a.seat() else {
                    return Err(Errore("Azione riservata al sistema".into()));
                };
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
                if t.ack.contains(id) {
                    return Ok(());
                }
                Voce::Prosegui { id: id.clone() }
            }
            Richiesta::Termina => Voce::Termina { da: id.clone() },
            Richiesta::Sostituisci { posto } => {
                let assente = match t.posti.get(usize::from(posto)) {
                    Some(Some(Posto::Umano { id: a })) => a.clone(),
                    _ => return Err(Errore("Nessun giocatore da sostituire".into())),
                };
                if !self.sostituibili().contains(&assente) {
                    return Err(Errore(
                        "Aspetta ancora un po': potrebbe ricollegarsi".into(),
                    ));
                }
                Voce::Sostituisci { posto }
            }
            Richiesta::Riprendi { posto } => Voce::Riprendi {
                posto,
                id: id.clone(),
            },
        };
        self.aggiungi(voce)
    }

    /// Recomputes the next bot move / system step and the running timer (host only).
    fn ripianifica(&mut self) {
        let attivo = self.e_host()
            && self.allineamento.is_none()
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
                    TipoTimer::FineMano => t.impostazioni.timer_fine_mano,
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
            // the summary waited long enough: everyone counts as having seen it (no actions)
            TipoTimer::FineMano => {}
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
        if azioni.is_empty() && k.tipo != TipoTimer::FineMano {
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
    if st.fase == Fase::FineMano && imp.timer_fine_mano > 0 && !t.umani().is_subset(&t.ack) {
        return Some(ChiaveTimer {
            tipo: TipoTimer::FineMano,
            mano: st.mano_n,
            turno: st.turno,
            giocate: 0,
            seat: None,
        });
    }
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

/// A seat number from the wire, if it names a seat.
pub fn posto(s: u8) -> Option<Seat> {
    (s < 4).then_some(s)
}

/// Whether a pending request is still worth sending: a move must still be legal locally.
fn ancora_sensata(t: Option<&Tavolo>, r: &Richiesta) -> bool {
    match r {
        Richiesta::Azione(a) => t
            .and_then(|t| t.partita.as_ref())
            .zip(a.seat())
            .is_some_and(|(st, s)| st.legal_actions(s).contains(a)),
        _ => true,
    }
}

/// Whether `raggiunti` holds a strict majority of `vista` (an empty view gives no constraint).
fn maggioranza(raggiunti: &BTreeSet<IdGiocatore>, vista: &BTreeSet<IdGiocatore>) -> bool {
    vista.is_empty() || 2 * raggiunti.intersection(vista).count() > vista.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insieme(ids: &[&str]) -> BTreeSet<IdGiocatore> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn maggioranza_stretta() {
        let quattro = insieme(&["a", "b", "c", "d"]);
        assert!(maggioranza(&insieme(&["b", "c", "d"]), &quattro));
        assert!(
            !maggioranza(&insieme(&["c", "d"]), &quattro),
            "half is not enough"
        );
        assert!(!maggioranza(&insieme(&["b"]), &insieme(&["a", "b"])));
        assert!(maggioranza(
            &insieme(&["b", "c"]),
            &insieme(&["a", "b", "c"])
        ));
        assert!(
            maggioranza(&insieme(&["b", "x", "y"]), &insieme(&["b"])),
            "outsiders do not count against"
        );
        assert!(maggioranza(&insieme(&["b"]), &BTreeSet::new()));
    }
}
