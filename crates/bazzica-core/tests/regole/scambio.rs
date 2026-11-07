//! Heisenberg variant: card exchange, fast pass, secrecy (`tests/test_scambio.py`).

use super::*;
use bazzica_core::Variante;
use bazzica_core::game::DatiEvento;

fn h() -> RulesConfig {
    RulesConfig::preset(Variante::Heisenberg, false)
}

fn st() -> GameState {
    Mk::new()
        .rules(h())
        .fase(Fase::Scambio)
        .mano(0, &["C1", "C2", "C3"])
        .mano(1, &["S1", "S2", "S3"])
        .mano(2, &["B1", "B2", "B3"])
        .mano(3, &["D4", "D5", "D6"])
        .tavola(&["C9"])
        .build()
}

fn passa(seat: Seat, carta: &str, veloce: bool) -> Azione {
    Azione::Passa {
        seat,
        carta: c(carta),
        veloce,
    }
}

fn passabili(st: &GameState, seat: Seat) -> Vec<Card> {
    let mut v: Vec<Card> = st
        .legal_actions(seat)
        .into_iter()
        .filter_map(|a| match a {
            Azione::Passa { carta, .. } => Some(carta),
            _ => None,
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

fn json<T: serde::Serialize>(x: &T) -> String {
    serde_json::to_string(x).unwrap()
}

#[test]
fn fase_scambio_dopo_distribuzione() {
    let (st, _) = GameState::new_game(h(), 3, None);
    assert_eq!(st.fase, Fase::Scambio);
    assert_eq!(st.required_actors(), [0, 1, 2, 3]);
}

#[test]
fn carta_del_compagno_invisibile_finche_non_passo() {
    let mut st = st();
    st.apply(&passa(0, "C1", false)).unwrap();
    // 2 cannot see 0's card until passing its own
    assert!(!st.mani[2].contains(&c("C1")));
    let o2 = st.observation(2);
    assert!(!o2.mano.contains(&c("C1")) && o2.scambio.ricevuta.is_none());
    assert!(!json(&o2).contains("coppe_01"));
    assert!(o2.scambio.passati[0]);
    // ...and cannot pass it
    assert_eq!(passabili(&st, 2), sorted(cc(&["B1", "B2", "B3"])));
    assert!(st.clone().apply(&passa(2, "C1", false)).is_err());
    let ev = st.apply(&passa(2, "B2", false)).unwrap();
    let mut ric: Vec<(Seat, Card)> = ev
        .iter()
        .filter_map(|e| match e.dati {
            DatiEvento::ScambioRicevuta { seat, carta, .. } => {
                assert_eq!(e.visibile, Some(vec![seat]));
                Some((seat, carta))
            }
            _ => None,
        })
        .collect();
    ric.sort();
    assert_eq!(ric, [(0, c("B2")), (2, c("C1"))]);
    assert_eq!(sorted(st.mani[2].clone()), sorted(cc(&["B1", "B3", "C1"])));
    assert_eq!(sorted(st.mani[0].clone()), sorted(cc(&["C2", "C3", "B2"])));
    assert_eq!(st.observation(2).scambio.ricevuta, Some(c("C1")));
}

#[test]
fn carta_ricevuta_non_ripassabile() {
    let mut st = st();
    st.apply(&passa(2, "B2", false)).unwrap();
    st.apply(&passa(0, "C3", false)).unwrap();
    assert!(passabili(&st, 0).is_empty());
    assert!(passabili(&st, 2).is_empty());
}

#[test]
fn passa_veloce_pubblico_ma_carta_segreta() {
    let mut st = st();
    let ev = st.apply(&passa(1, "S1", true)).unwrap();
    let privato = ev
        .iter()
        .find(|e| matches!(e.dati, DatiEvento::ScambioCarta { .. }))
        .unwrap();
    let pubblico = ev
        .iter()
        .find(|e| matches!(e.dati, DatiEvento::Scambio { .. }))
        .unwrap();
    assert_eq!(privato.visibile, Some(vec![1]));
    assert!(matches!(privato.dati, DatiEvento::ScambioCarta { carta, .. } if carta == c("S1")));
    assert!(pubblico.visibile.is_none());
    assert!(matches!(
        pubblico.dati,
        DatiEvento::Scambio { veloce: true, .. }
    ));
    assert!(
        serde_json::to_value(pubblico)
            .unwrap()
            .get("carta")
            .is_none()
    );
    for s in [0, 2, 3] {
        let o = st.observation(s);
        assert!(o.scambio.veloci[1]);
        assert!(!json(&o).contains("spade_01"));
    }
}

#[test]
fn giocate_iniziano_dopo_tutti_gli_scambi() {
    let mut st = st();
    for (s, carta) in [(0, "C1"), (1, "S1"), (2, "B1")] {
        st.apply(&passa(s, carta, false)).unwrap();
        assert_eq!(st.fase, Fase::Scambio);
    }
    st.apply(&passa(3, "D4", false)).unwrap();
    assert_eq!((st.fase, st.corrente), (Fase::Gioco, 0));
}

#[test]
fn attendi_non_crea_stallo() {
    let mut st = st();
    st.apply(&Azione::Attendi { seat: 0 }).unwrap();
    assert!(!st.legal_actions(2).contains(&Azione::Attendi { seat: 2 }));
    assert!(!st.legal_actions(0).contains(&Azione::Attendi { seat: 0 }));
}

#[test]
fn bazzica_con_la_mano_dopo_lo_scambio() {
    let mut st = st();
    for (s, carta) in [(2, "B1"), (0, "C3"), (1, "S3"), (3, "D6")] {
        st.apply(&passa(s, carta, false)).unwrap();
    }
    // 0 holds C1, C2, B1: pair of aces, sum 4 -> 3 Scope
    let baz: Vec<Azione> = st
        .legal_actions(0)
        .into_iter()
        .filter(|a| matches!(a, Azione::Bazzica { .. }))
        .collect();
    assert_eq!(baz.len(), 1);
    let ev = st.apply(&baz[0]).unwrap();
    assert!(matches!(ev[0].dati, DatiEvento::Bazzica { scope: 3, .. }));
}
