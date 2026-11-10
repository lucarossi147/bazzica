//! Wire messages exchanged between peers (JSON over a reliable, ordered channel).

use std::collections::BTreeSet;

use bazzica_core::{Azione, Seat};
use serde::{Deserialize, Serialize};

use crate::tavolo::{IdGiocatore, Impostazioni, Partecipante, Tavolo, Voce};

/// Bumped on any incompatible change to the messages or to the replicated state.
///
/// - 3: `Allineato`, the answer to a `Host` claim (sync barrier of a new host).
/// - 4: [`Sessione`] in `Ciao`, the host's [`Regia`] in `Istantanea`, `Sostituito`.
/// - 5: `HoReplica` (fixes stale replica flags), `Presenti` (the host's view, for the
///   majority rule of elections).
/// - 6: `Ricevuta`, the host's acknowledgement of a request.
/// - 7: end-of-hand timer (`TipoTimer::FineMano`, `Impostazioni::timer_fine_mano`), seat
///   replacement (`Voce::Sostituisci`, `Voce::Riprendi`, `Tavolo::sostituiti`).
pub const VERSIONE_PROTOCOLLO: u32 = 7;

/// One run of the app (a page load) for an identity. When two runs of the same identity are
/// connected (two tabs, a reload before the old link timed out), the newer one wins: later
/// `nata` (ms on the device's clock, which same-device tabs share), then the larger `nonce`.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Sessione {
    pub nata: u64,
    pub nonce: u64,
}

/// Who hosts: a participant in one of its sessions (another tab of the same id is not host).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Regia {
    pub id: IdGiocatore,
    pub sessione: Sessione,
}

/// What a player asks the host to do. The host checks permissions and turns it into a [`Voce`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "richiesta", rename_all = "snake_case")]
pub enum Richiesta {
    Siedi {
        posto: Seat,
    },
    Alzati {
        posto: Seat,
    },
    Bot {
        posto: Seat,
        aggiungi: bool,
    },
    Imposta(Impostazioni),
    Avvia,
    Azione(Azione),
    Prosegui,
    Termina,
    /// A bot takes the seat of a human who has been away for a while.
    Sostituisci {
        posto: Seat,
    },
    /// Take back one's own seat from the bot that replaced us.
    Riprendi {
        posto: Seat,
    },
}

/// Cosmetic effects that do not change the game: sent straight to everyone, never logged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "effetto", rename_all = "snake_case")]
pub enum Effimero {
    /// A quick chat phrase (index into [`crate::tavolo::FRASI`]).
    Frase { seat: Seat, indice: u8 },
    /// A card "dragged" on the table to hurry the previous player.
    Striscia { seat: Seat, verso: Seat },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "msg", rename_all = "snake_case")]
pub enum Messaggio {
    /// First message on every new connection: who I am.
    Ciao {
        versione: u32,
        io: Partecipante,
        sessione: Sessione,
        /// Whether the sender already holds a replica of the table.
        replica: bool,
    },
    /// Player -> host.
    Richiesta {
        rid: u64,
        richiesta: Richiesta,
    },
    /// Host -> player: the request was refused.
    Rifiuto {
        rid: u64,
        motivo: String,
    },
    /// Host -> player: the request was handled (applied, or a duplicate of one that was).
    /// Until this or `Rifiuto` arrives, the player resends it (same `rid`) to whoever hosts.
    Ricevuta {
        rid: u64,
    },
    /// Host -> everyone: entry number `n` of the log and the replica hash after applying it.
    Voce {
        epoca: u64,
        n: u64,
        voce: Voce,
        impronta: String,
    },
    /// The whole replica, sent to newcomers, to peers that fell behind and on resync.
    Istantanea {
        epoca: u64,
        host: Regia,
        tavolo: Box<Tavolo>,
    },
    /// "I am the host now" (after the previous host left). `n` is the claimer's log length.
    Host {
        epoca: u64,
        n: u64,
    },
    /// Follower -> host: please resend entries from `da` (or a snapshot).
    ChiediDa {
        da: u64,
    },
    /// Follower -> new host: "I follow your claim at `epoca`; my replica is at `n` with this
    /// hash" (`n` 0 and an empty hash without a replica). The host sequences nothing until
    /// every peer answered or the wait expired.
    Allineato {
        epoca: u64,
        n: u64,
        impronta: String,
    },
    Effimero(Effimero),
    /// "A newer session of your identity is here": the receiver stops acting.
    Sostituito,
    /// "I hold a replica now" (sent once, when the first snapshot is adopted), so peers that
    /// met us earlier count us as a possible host.
    HoReplica,
    /// Host -> everyone: who the host sees connected (itself included). Elections need a
    /// strict majority of this set, so two sides of a split network never both elect.
    Presenti {
        ids: BTreeSet<IdGiocatore>,
    },
}

impl Messaggio {
    pub fn codifica(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serializable")
    }

    pub fn decodifica(b: &[u8]) -> Option<Messaggio> {
        serde_json::from_slice(b).ok()
    }
}
