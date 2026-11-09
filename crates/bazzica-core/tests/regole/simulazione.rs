//! Bot-only simulations with invariant checks (`tests/test_simulazione.py`).
//! Games per variant: `BAZZICA_SIM` (default 250, i.e. 2,000 games).

use super::*;
use bazzica_core::bots::{Agent, HeuristicAgent, RandomAgent, gioca_partita};
use bazzica_core::game::DatiEvento;
use bazzica_core::{Variante, Vincitore};

fn controlla_invarianti(st: &GameState, events: &[Evento]) {
    // cards "in transit" during the exchange belong to nobody yet
    let in_viaggio = (0..4).filter_map(|x| {
        let p = (x + 2) % 4;
        (st.fase == Fase::Scambio && st.scambio_ricevuta[p].is_none())
            .then_some(st.scambio_passata[x])
            .flatten()
    });
    let mut tutte: Vec<Card> = st
        .mazzo
        .iter()
        .chain(&st.tavola)
        .chain(st.mani.iter().flatten())
        .copied()
        .chain(st.prese.iter().flatten().map(Entrata::carta))
        .chain(in_viaggio)
        .collect();
    tutte.sort();
    assert_eq!(tutte, *MAZZO, "cards not conserved");
    // the current turn's plays are exactly the cards no longer in hand
    if st.fase == Fase::Gioco {
        let obs = st.observation(0);
        let in_mano: usize = st.mani.iter().map(Vec::len).sum();
        assert_eq!(obs.giocate_turno().len(), 12 - in_mano);
        for s in 0..4u8 {
            let mie = obs.giocate_turno().iter().filter(|g| g.0 == s).count();
            assert_eq!(mie, 3 - st.mani[usize::from(s)].len());
            assert_eq!(obs.ultima_di(s).is_some(), mie > 0);
        }
    }
    for e in events {
        match &e.dati {
            DatiEvento::FineMano { riepilogo } => {
                for t in 0..2 {
                    let s = &riepilogo.squadre[t];
                    assert_eq!(s.scope as usize, n_su(&st.prese[t]));
                    assert_eq!(
                        s.totale,
                        s.punti_mazzo + s.scope as i32 + s.punti_due_denari + s.napola as i32
                    );
                }
                assert!(riepilogo.squadre[0].punti_mazzo + riepilogo.squadre[1].punti_mazzo <= 5);
            }
            DatiEvento::Giocata(g) if !g.presa.is_empty() && (g.esito.scopa || g.esito.piccola) => {
                assert!(!g.girate.is_empty());
            }
            _ => {}
        }
    }
    // information security: no observation exposes another player's hidden hand
    for s in 0..4 {
        for (k, a) in st.observation(s).altri {
            if st.bazzica[usize::from(k)].is_none() {
                assert!(a.carte.is_none());
            }
        }
    }
}

#[test]
fn simulazione_solo_bot() {
    let n: u64 = std::env::var("BAZZICA_SIM")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(250);
    for (vi, variante) in Variante::TUTTE.into_iter().enumerate() {
        for i in 0..n {
            let seed = 1000 * vi as u64 + i;
            let rules = RulesConfig::preset(variante, i % 5 == 0);
            let mut agents: [Box<dyn Agent>; 4] = std::array::from_fn(|k| -> Box<dyn Agent> {
                if i % 3 == 0 {
                    Box::new(RandomAgent::new(seed + k as u64))
                } else {
                    Box::new(HeuristicAgent::new(seed + k as u64))
                }
            });
            let st = gioca_partita(rules.clone(), &mut agents, seed, controlla_invarianti).unwrap();
            assert_eq!(st.fase, Fase::FinePartita);
            let [a, b] = st.punteggi;
            if rules.margherita_hack {
                assert_eq!(st.mano_n, 4);
            } else {
                assert!(a != b && a.max(b) >= 21);
                assert_eq!(
                    st.vincitore,
                    Some(Vincitore::Squadra(if a > b { 0 } else { 1 }))
                );
            }
            assert_eq!(st.storico_mani.iter().map(|r| r.totali[0]).sum::<i32>(), a);
        }
    }
}
