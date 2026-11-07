//! Heuristic bot behaviour (`tests/test_bot.py`).

use super::*;
use bazzica_core::Variante;
use bazzica_core::bots::{Agent, HeuristicAgent};

fn scegli(st: &GameState, seat: Seat) -> Azione {
    HeuristicAgent::new(1).choose_action(&st.observation(seat), &st.legal_actions(seat))
}

fn scambio(mano0: &[&str], mano2: &[&str]) -> GameState {
    Mk::new()
        .rules(RulesConfig::preset(Variante::Heisenberg, false))
        .fase(Fase::Scambio)
        .mano(0, mano0)
        .mano(2, mano2)
        .build()
}

#[test]
fn bot_chiama_bazzica() {
    let st = Mk::new()
        .mano(0, &["C9", "S9", "B9"])
        .tavola(&["D4"])
        .build();
    assert!(matches!(scegli(&st, 0), Azione::Bazzica { .. }));
}

#[test]
fn bot_sceglie_valore_7_bello_migliore() {
    let st = Mk::new()
        .mano(0, &["D7", "S10", "B10"])
        .tavola(&["C4"])
        .build();
    assert_eq!(
        scegli(&st, 0),
        Azione::Bazzica {
            seat: 0,
            sette: Some(10)
        }
    );
}

#[test]
fn bot_prende_la_scopa() {
    let st = Mk::new()
        .mano(0, &["C7", "S9", "B10"])
        .tavola(&["D3", "S4"])
        .build();
    assert!(matches!(scegli(&st, 0), Azione::Gioca { carta, .. } if carta == c("C7")));
}

#[test]
fn bot_evita_due_di_denari() {
    let st = Mk::new()
        .mano(0, &["C2", "S8", "B10"])
        .tavola(&["D2", "S5", "S1"])
        .build();
    assert!(matches!(scegli(&st, 0), Azione::Gioca { carta, .. } if carta != c("C2")));
}

#[test]
fn bot_che_palo() {
    let st = Mk::new()
        .mano(0, &["C5", "S5", "B6"])
        .tavola(&["D9"])
        .build();
    assert_eq!(scegli(&st, 0), Azione::ChePalo { seat: 0 });
}

#[test]
fn bot_passa_veloce_con_coppia() {
    let st = scambio(&["C9", "S9", "B4"], &["D1", "D2", "D3"]);
    assert_eq!(
        scegli(&st, 0),
        Azione::Passa {
            seat: 0,
            carta: c("B4"),
            veloce: true
        }
    );
}

#[test]
fn bot_risponde_al_passa_veloce_col_7_bello() {
    let mut st = scambio(&["C9", "S9", "B4"], &["D7", "S10", "C8"]);
    st.apply(&Azione::Passa {
        seat: 0,
        carta: c("B4"),
        veloce: true,
    })
    .unwrap();
    assert!(matches!(scegli(&st, 2), Azione::Passa { carta, .. } if carta == c("D7")));
}
