//! Bot-only games (simulations), with the same actor order as the reference runner:
//! optional actors first, then required ones.

use super::Agent;
use crate::game::{Evento, Fase, GameState};
use crate::rules::RulesConfig;

#[derive(Debug)]
pub struct StalloError(pub &'static str);

/// Plays a full game with 4 agents; `on_events` sees the state after each batch of events.
pub fn gioca_partita(
    rules: RulesConfig,
    agents: &mut [Box<dyn Agent>; 4],
    seed: u64,
    mut on_events: impl FnMut(&GameState, &[Evento]),
) -> Result<GameState, StalloError> {
    let (mut st, events) = GameState::new_game(rules, seed, None);
    on_events(&st, &events);
    for _ in 0..100_000 {
        if st.fase == Fase::FinePartita {
            return Ok(st);
        }
        if let Some(sa) = st.system_action() {
            let events = st.apply(&sa).expect("system action is always legal");
            on_events(&st, &events);
            continue;
        }
        let attore = st
            .optional_actors()
            .into_iter()
            .chain(st.required_actors())
            .map(|s| (s, st.legal_actions(s)))
            .find(|(_, legal)| !legal.is_empty());
        let Some((s, legal)) = attore else {
            return Err(StalloError("nobody can act"));
        };
        let a = agents[usize::from(s)].choose_action(&st.observation(s), &legal);
        let events = st.apply(&a).expect("agents choose legal actions");
        on_events(&st, &events);
    }
    Err(StalloError("too many actions"))
}
