//! Agents: a common interface plus the heuristic and random bots.
//!
//! An agent receives exactly the [`Osservazione`] a human in that seat would see, and the
//! list of legal actions, and returns one of them.

mod heuristic;
mod runner;

pub use heuristic::HeuristicAgent;
pub use runner::{StalloError, gioca_partita};

use crate::game::{Azione, Osservazione};
use crate::prng::Mulberry32;

pub trait Agent: Send {
    fn choose_action(&mut self, obs: &Osservazione, legal: &[Azione]) -> Azione;

    /// Current PRNG state (checked by the conformance traces).
    fn rng_stato(&self) -> u32;
}

/// Plays a uniformly random legal action, never choosing to wait or exclaim.
pub struct RandomAgent {
    rng: Mulberry32,
}

impl RandomAgent {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Mulberry32::new(seed),
        }
    }
}

impl Agent for RandomAgent {
    fn choose_action(&mut self, _obs: &Osservazione, legal: &[Azione]) -> Azione {
        let scelte: Vec<Azione> = legal
            .iter()
            .filter(|a| !matches!(a, Azione::Attendi { .. } | Azione::ChePalo { .. }))
            .cloned()
            .collect();
        let scelte = if scelte.is_empty() {
            legal.to_vec()
        } else {
            scelte
        };
        self.rng.choice(&scelte).clone()
    }

    fn rng_stato(&self) -> u32 {
        self.rng.stato
    }
}

/// Builds an agent from its trace name: `"euristico"` or `"casuale"`.
pub fn crea_agente(tipo: &str, seed: u64) -> Box<dyn Agent> {
    match tipo {
        "euristico" => Box::new(HeuristicAgent::new(seed)),
        "casuale" => Box::new(RandomAgent::new(seed)),
        other => panic!("unknown agent type {other:?}"),
    }
}
