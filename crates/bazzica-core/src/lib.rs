//! Pure rules engine for the Bazzica card game: no I/O, no networking, fully deterministic.
//!
//! This is a faithful port of the Python reference engine in `bazzica/engine`. Its JSON
//! output (state, observations, events, actions) matches the Python one exactly and is
//! checked against the recorded games in `testdata/tracce`.

pub mod bazzica;
pub mod cards;
pub mod prng;
pub mod rules;
pub mod scoring;

pub use cards::{Card, MAZZO, RE_BELLO, SETTE_BELLO, DUE_DENARI, Seme};
pub use prng::Mulberry32;
pub use rules::{RulesConfig, Variante};
