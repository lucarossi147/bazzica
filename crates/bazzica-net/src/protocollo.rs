//! Wire messages exchanged between peers (JSON over a reliable, ordered channel).

use bazzica_core::{Azione, Seat};
use serde::{Deserialize, Serialize};

use crate::tavolo::{IdGiocatore, Impostazioni, Partecipante, Tavolo, Voce};

/// Bumped on any incompatible change to the messages or to the replicated state.
///
/// - 3: `Allineato`, the answer to a `Host` claim (sync barrier of a new host).
pub const VERSIONE_PROTOCOLLO: u32 = 3;

/// What a player asks the host to do. The host checks permissions and turns it into a [`Voce`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "richiesta", rename_all = "snake_case")]
pub enum Richiesta {
    Siedi { posto: Seat },
    Alzati { posto: Seat },
    Bot { posto: Seat, aggiungi: bool },
    Imposta(Impostazioni),
    Avvia,
    Azione(Azione),
    Prosegui,
    Termina,
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
        host: IdGiocatore,
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
}

impl Messaggio {
    pub fn codifica(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serializable")
    }

    pub fn decodifica(b: &[u8]) -> Option<Messaggio> {
        serde_json::from_slice(b).ok()
    }
}
