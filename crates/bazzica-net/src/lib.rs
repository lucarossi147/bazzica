//! Transport-agnostic table protocol for Bazzica.
//!
//! - [`tavolo`]: the replicated table (lobby + game) and its log entries.
//! - [`protocollo`]: wire messages.
//! - [`nodo`]: one peer as a sans-I/O state machine (host/sequencer, followers, bots, timers,
//!   host takeover, resync).
//!
//! The crate does no I/O: a driver connects it to WebRTC, a test network, or anything else.

pub mod nodo;
pub mod protocollo;
pub mod tavolo;

pub use nodo::{Config, Destinatario, Nodo, Uscita};
pub use protocollo::{
    Effimero, Messaggio, Regia, Richiesta, Sessione, TimerInCorso, VERSIONE_PROTOCOLLO,
};
pub use tavolo::{
    Errore, IdGiocatore, Impostazioni, Partecipante, Posto, Ritmo, StatoTavolo, Tavolo, TipoTimer,
    Voce,
};
