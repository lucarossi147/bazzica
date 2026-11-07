//! WebRTC transport for Bazzica tables (matchbox), with signaling over public Nostr relays.
//!
//! Works in the browser (WASM) and natively. See [`sessione::Sessione`].

pub mod nostr;
pub mod segnalatore;
pub mod sessione;
mod ws;

pub use segnalatore::{NostrSignallerBuilder, RELAY_PREDEFINITI};
pub use sessione::Sessione;
