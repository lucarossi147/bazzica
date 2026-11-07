//! Captures, the sweeping ace, Scope, last play, leftover cards (`tests/test_prese.py`).

use super::*;
use bazzica_core::scoring::Origine;
use bazzica_core::{Variante, game::DatiEvento};

#[test]
fn presa_singola_obbligatoria() {
    let st = Mk::new()
        .mano(0, &["C5", "S9", "B10"])
        .tavola(&["D5", "S2", "S3"])
        .build();
    assert_eq!(presa_opts(&st, 0, "C5"), [cc(&["D5"])]);
    assert!(st.clone().apply(&play(0, "C5", &["S2", "S3"])).is_err());
}

#[test]
fn presa_singola_scelta_tra_piu_uguali() {
    let st = Mk::new()
        .mano(0, &["C5", "S9", "B10"])
        .tavola(&["D5", "S5", "S3"])
        .build();
    assert_eq!(presa_opts(&st, 0, "C5"), [cc(&["D5"]), cc(&["S5"])]);
}

#[test]
fn combinazioni_a_scelta() {
    let st = Mk::new()
        .mano(0, &["C7", "S9", "B10"])
        .tavola(&["D2", "S5", "S3", "B4"])
        .build();
    let opts = presa_opts(&st, 0, "C7");
    assert!(opts.contains(&sorted(cc(&["D2", "S5"]))));
    assert!(opts.contains(&sorted(cc(&["S3", "B4"]))));
    assert_eq!(opts.len(), 2);
}

#[test]
fn carta_che_non_prende_resta_in_tavola() {
    let mut st = Mk::new()
        .mano(0, &["C9", "S9", "B10"])
        .tavola(&["D2", "S5"])
        .build();
    assert_eq!(presa_opts(&st, 0, "C9"), [Vec::<Card>::new()]);
    st.apply(&play(0, "C9", &[])).unwrap();
    assert!(st.tavola.contains(&c("C9")));
    assert_eq!(st.piccola_target, Some(c("C9")));
}

#[test]
fn asso_piglia_tutto_e_scopa() {
    let mut st = Mk::new()
        .mano(0, &["C1", "S9", "B10"])
        .tavola(&["D2", "S5", "B7"])
        .build();
    assert_eq!(presa_opts(&st, 0, "C1"), [sorted(cc(&["D2", "S5", "B7"]))]);
    let ev = st.apply(&play(0, "C1", &["D2", "S5", "B7"])).unwrap();
    assert!(st.tavola.is_empty());
    assert_eq!(
        st.prese[0][0],
        Entrata(c("C1"), true, Some(Origine::Sparecchio))
    );
    assert!(giocata(&ev).esito.scopa);
}

#[test]
fn asso_con_asso_in_tavola_prende_solo_asso() {
    let mut st = Mk::new()
        .mano(0, &["C1", "S9", "B10"])
        .tavola(&["D1", "S5", "B1"])
        .build();
    assert_eq!(presa_opts(&st, 0, "C1"), [cc(&["D1"]), cc(&["B1"])]);
    let ev = st.apply(&play(0, "C1", &["D1"])).unwrap();
    assert!(!giocata(&ev).esito.scopa);
    assert_eq!(sorted(st.tavola.clone()), sorted(cc(&["S5", "B1"])));
}

#[test]
fn asso_che_prende_unico_asso_sparecchia_e_fa_scopa() {
    let mut st = Mk::new()
        .mano(0, &["C1", "S9", "B10"])
        .tavola(&["D1"])
        .build();
    let ev = st.apply(&play(0, "C1", &["D1"])).unwrap();
    assert!(giocata(&ev).esito.scopa);
}

#[test]
fn asso_su_tavolo_vuoto_resta_e_non_fa_scopa() {
    let mut st = Mk::new().mano(0, &["C1", "S9", "B10"]).build();
    assert_eq!(presa_opts(&st, 0, "C1"), [Vec::<Card>::new()]);
    let ev = st.apply(&play(0, "C1", &[])).unwrap();
    assert_eq!(st.tavola, cc(&["C1"]));
    assert!(!giocata(&ev).esito.scopa && st.prese[0].is_empty());
}

#[test]
fn scopa_normale() {
    let mut st = Mk::new()
        .mano(0, &["C7", "S9", "B10"])
        .tavola(&["D3", "S4"])
        .build();
    let ev = st.apply(&play(0, "C7", &["D3", "S4"])).unwrap();
    assert!(giocata(&ev).esito.scopa);
    assert_eq!(n_su(&st.prese[0]), 1);
}

#[test]
fn ultima_giocata_non_fa_scopa() {
    let mut st = Mk::new()
        .mano(3, &["C7"])
        .tavola(&["D3", "S4"])
        .ultima()
        .build();
    let ev = st.apply(&play(3, "C7", &["D3", "S4"])).unwrap();
    let g = giocata(&ev);
    assert!(g.ultima_giocata && !g.esito.scopa);
    assert_eq!(n_su(&st.prese[1]), 0);
}

#[test]
fn ultima_del_primo_turno_fa_scopa() {
    let mut st = Mk::new()
        .mano(3, &["C7"])
        .tavola(&["D3", "S4"])
        .corrente(3)
        .tutti_giocato()
        .build();
    let ev = st.apply(&play(3, "C7", &["D3", "S4"])).unwrap();
    let g = giocata(&ev);
    assert!(g.esito.scopa && !g.ultima_giocata);
    assert_eq!(st.turno, 2);
}

#[test]
fn carte_residue_alla_squadra_dell_ultima_presa() {
    // last capture by Nord-Sud (team 0); the last card captures nothing
    let mut st = Mk::new()
        .mano(3, &["C9"])
        .tavola(&["D3", "S5"])
        .ultima()
        .build();
    st.ultima_presa_squadra = Some(0);
    let ev = st.apply(&play(3, "C9", &[])).unwrap();
    let squadra = ev
        .iter()
        .find_map(|e| match &e.dati {
            DatiEvento::CarteResidue { squadra, .. } => Some(*squadra),
            _ => None,
        })
        .unwrap();
    assert_eq!(squadra, 0);
    let carte: Vec<Card> = st.prese[0].iter().map(Entrata::carta).collect();
    assert_eq!(sorted(carte), sorted(cc(&["D3", "S5", "C9"])));
    assert_eq!(n_su(&st.prese[0]), 0);
}

#[test]
fn gioco_in_senso_orario_e_mazziere_ultimo() {
    let mut st = Mk::new()
        .mano(0, &["C9"])
        .mano(1, &["S9"])
        .mano(2, &["B9"])
        .mano(3, &["D9"])
        .tavola(&["D1"])
        .build();
    for s in 0..4 {
        assert_eq!(st.corrente, s);
        let a = st
            .legal_actions(s)
            .into_iter()
            .find(|a| matches!(a, Azione::Gioca { .. }))
            .unwrap();
        st.apply(&a).unwrap();
    }
}

#[test]
fn nuova_mano_ruota_il_mazziere() {
    let (mut st, _) =
        GameState::new_game(RulesConfig::preset(Variante::Vanilla, false), 1, Some(2));
    st.fase = Fase::FineMano;
    st.apply(&st.system_action().unwrap()).unwrap();
    assert_eq!((st.mazziere, st.corrente, st.tavola.len()), (3, 0, 4));
}
