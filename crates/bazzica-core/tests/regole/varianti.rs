//! Pascal, Curie (Piccola and Grande), Avogadro, game end and Margherita Hack
//! (`tests/test_varianti.py`).

use super::*;
use bazzica_core::{Variante, Vincitore};

fn pa() -> RulesConfig {
    RulesConfig::preset(Variante::Pascal, false)
}
fn cu() -> RulesConfig {
    RulesConfig::preset(Variante::Curie, false)
}
fn av() -> RulesConfig {
    RulesConfig::preset(Variante::Avogadro, false)
}

// ------------------------------------------------------------------ Pascal

#[test]
fn pascal_asso_di_bazzica_vale_1() {
    let mut st = Mk::new()
        .rules(pa())
        .mano(0, &["C1", "S3", "B5"])
        .tavola(&["D5", "S2"])
        .build();
    st.apply(&bazzica(0, None)).unwrap();
    assert_eq!(presa_opts(&st, 0, "C1"), [Vec::<Card>::new()]);
    let mut st = Mk::new()
        .rules(pa())
        .mano(0, &["C1", "S3", "B5"])
        .tavola(&["D5", "S1"])
        .build();
    st.apply(&bazzica(0, None)).unwrap();
    assert_eq!(presa_opts(&st, 0, "C1"), [cc(&["S1"])]);
}

#[test]
fn pascal_asso_senza_bazzica_piglia_tutto() {
    let st = Mk::new()
        .rules(pa())
        .mano(0, &["C1", "S3", "B5"])
        .tavola(&["D5", "S2"])
        .build();
    assert_eq!(presa_opts(&st, 0, "C1"), [sorted(cc(&["D5", "S2"]))]);
}

// ------------------------------------------------------------------ Curie: Piccola

fn piccola_state(rules: RulesConfig, tavola: &[&str]) -> GameState {
    let mut st = Mk::new()
        .rules(rules)
        .mano(0, &["C4", "C8", "B2"])
        .mano(1, &["S4", "S9", "B10"])
        .tavola(tavola)
        .build();
    st.apply(&play(0, "C4", &[])).unwrap(); // the 4 stays on the table
    st
}

#[test]
fn piccola() {
    let mut st = piccola_state(cu(), &["D9"]);
    let ev = st.apply(&play(1, "S4", &["C4"])).unwrap();
    let g = giocata(&ev);
    assert!(g.esito.piccola && !g.esito.scopa);
    assert_eq!(origini_su(&st.prese[1]), ["piccola"]);
}

#[test]
fn piccola_che_sparecchia_vale_una_sola_scopa() {
    let mut st = piccola_state(cu(), &[]);
    let ev = st.apply(&play(1, "S4", &["C4"])).unwrap();
    let g = giocata(&ev);
    assert!(g.esito.piccola && !g.esito.scopa && g.esito.sparecchio);
    assert_eq!(n_su(&st.prese[1]), 1);
}

#[test]
fn niente_piccola_senza_curie() {
    let mut st = piccola_state(pa(), &["D9"]);
    assert!(
        !giocata(&st.apply(&play(1, "S4", &["C4"])).unwrap())
            .esito
            .piccola
    );
}

#[test]
fn piccola_solo_carta_del_giocatore_precedente() {
    let mut st = Mk::new()
        .rules(cu())
        .mano(0, &["C4", "C8", "B2"])
        .mano(1, &["S9"])
        .mano(2, &["S4", "B9", "B8"])
        .tavola(&["D10"])
        .build();
    st.apply(&play(0, "C4", &[])).unwrap();
    st.apply(&play(1, "S9", &[])).unwrap();
    assert!(
        !giocata(&st.apply(&play(2, "S4", &["C4"])).unwrap())
            .esito
            .piccola
    );
}

#[test]
fn primo_giocatore_del_turno_non_fa_piccola() {
    // the last play of turn 1 leaves a 4; the first player of turn 2 takes it
    let rules = RulesConfig {
        scambio: false,
        ..cu()
    };
    let mut st = Mk::new()
        .rules(rules)
        .mano(3, &["C4"])
        .tavola(&["D9"])
        .corrente(3)
        .tutti_giocato()
        .build();
    st.apply(&play(3, "C4", &[])).unwrap();
    assert_eq!((st.turno, st.corrente), (2, 0));
    st.mani[0] = cc(&["S4", "B9", "B8"]);
    assert!(
        !giocata(&st.apply(&play(0, "S4", &["C4"])).unwrap())
            .esito
            .piccola
    );
}

#[test]
fn piccola_con_sette_bello_di_bazzica() {
    let mut st = Mk::new()
        .rules(cu())
        .mano(0, &["C4", "C8", "B10"])
        .mano(1, &["D7", "S1", "B3"])
        .tavola(&["S9"])
        .build();
    st.apply(&play(0, "C4", &[])).unwrap();
    st.apply(&bazzica(1, Some(4))).unwrap(); // 4 + 1 + 3 = 8: three different
    assert!(
        giocata(&st.apply(&play(1, "D7", &["C4"])).unwrap())
            .esito
            .piccola
    );
}

#[test]
fn piccola_e_grande_vale_solo_grande() {
    // Piccola and Grande together are only possible with a Bazzica's 7 bello
    let mut st = Mk::new()
        .rules(cu())
        .mano(0, &["D4", "C8", "B2"])
        .mano(1, &["D7", "S1", "B3"])
        .tavola(&["S9"])
        .build();
    st.apply(&play(0, "D4", &[])).unwrap();
    st.apply(&bazzica(1, Some(4))).unwrap();
    let ev = st.apply(&play(1, "D7", &["D4"])).unwrap();
    let g = giocata(&ev);
    assert!(g.esito.grande && !g.esito.piccola);
    assert_eq!(origini_su(&st.prese[1]), ["grande", "grande"]);
}

// ------------------------------------------------------------------ Curie: Grande

#[test]
fn grande_presa_multipla_stesso_seme() {
    let mut st = Mk::new()
        .rules(cu())
        .mano(0, &["C7", "S9", "B10"])
        .tavola(&["C3", "C4", "S10"])
        .build();
    let ev = st.apply(&play(0, "C7", &["C3", "C4"])).unwrap();
    let g = giocata(&ev);
    assert!(g.esito.grande && !g.esito.scopa);
    assert_eq!(n_su(&st.prese[0]), 3);
}

#[test]
fn grande_che_sparecchia_conta_solo_la_grande() {
    let mut st = Mk::new()
        .rules(cu())
        .mano(0, &["C7", "S9", "B10"])
        .tavola(&["C3", "C4"])
        .build();
    let ev = st.apply(&play(0, "C7", &["C3", "C4"])).unwrap();
    let g = giocata(&ev);
    assert!(g.esito.grande && !g.esito.scopa);
    assert_eq!(n_su(&st.prese[0]), 3);
}

#[test]
fn grande_con_sette_bello_su_denari() {
    let mut st = Mk::new()
        .rules(cu())
        .mano(0, &["D7", "S1", "B3"])
        .tavola(&["D5", "S9"])
        .build();
    st.apply(&bazzica(0, Some(5))).unwrap();
    assert!(
        giocata(&st.apply(&play(0, "D7", &["D5"])).unwrap())
            .esito
            .grande
    );
    assert_eq!(
        origini_su(&st.prese[0])
            .iter()
            .filter(|o| *o == "grande")
            .count(),
        2
    );
}

#[test]
fn grande_con_sette_bello_somma_di_denari() {
    let mut st = Mk::new()
        .rules(cu())
        .mano(0, &["D7", "S1", "B3"])
        .tavola(&["D2", "D3", "S10"])
        .build();
    st.apply(&bazzica(0, Some(5))).unwrap();
    assert!(
        giocata(&st.apply(&play(0, "D7", &["D2", "D3"])).unwrap())
            .esito
            .grande
    );
    assert_eq!(
        origini_su(&st.prese[0])
            .iter()
            .filter(|o| *o == "grande")
            .count(),
        3
    );
}

#[test]
fn grande_asso_piglia_tutto_stesso_seme() {
    let mut st = Mk::new()
        .rules(cu())
        .mano(0, &["B1", "S9", "C10"])
        .tavola(&["B5", "B8"])
        .build();
    let ev = st.apply(&play(0, "B1", &["B5", "B8"])).unwrap();
    let g = giocata(&ev);
    assert!(g.esito.grande && !g.esito.scopa);
    assert_eq!(n_su(&st.prese[0]), 3);
}

#[test]
fn niente_grande_se_semi_diversi() {
    let mut st = Mk::new()
        .rules(cu())
        .mano(0, &["B1", "S9", "C10"])
        .tavola(&["B5", "C8"])
        .build();
    let ev = st.apply(&play(0, "B1", &["B5", "C8"])).unwrap();
    let g = giocata(&ev);
    assert!(!g.esito.grande && g.esito.scopa);
}

#[test]
fn grande_asso_in_bazzica_senza_pascal_non_vale() {
    let r = RulesConfig {
        curie: true,
        ..RulesConfig::preset(Variante::Heisenberg, false)
    };
    let mut st = Mk::new()
        .rules(r)
        .mano(0, &["B1", "S3", "C5"])
        .tavola(&["B5", "B8"])
        .build();
    st.apply(&bazzica(0, None)).unwrap();
    let ev = st.apply(&play(0, "B1", &["B5", "B8"])).unwrap();
    let g = giocata(&ev);
    assert!(!g.esito.grande && g.esito.scopa);
}

#[test]
fn ultima_giocata_niente_piccola_ne_grande() {
    let mut st = Mk::new()
        .rules(cu())
        .mano(3, &["C7"])
        .tavola(&["C3", "C4"])
        .ultima()
        .build();
    let ev = st.apply(&play(3, "C7", &["C3", "C4"])).unwrap();
    let g = giocata(&ev);
    assert!(!g.esito.grande && !g.esito.scopa);
    let mut st = Mk::new()
        .rules(cu())
        .mano(3, &["C4"])
        .tavola(&["S4"])
        .ultima()
        .piccola_target("S4")
        .build();
    assert!(
        !giocata(&st.apply(&play(3, "C4", &["S4"])).unwrap())
            .esito
            .piccola
    );
}

// ------------------------------------------------------------------ Avogadro

#[test]
fn avogadro_asso_preso_fa_scopa() {
    let mut st = Mk::new()
        .rules(av())
        .mano(0, &["C6", "S9", "B10"])
        .tavola(&["D1", "S1", "B5"])
        .build();
    let ev = st.apply(&play(0, "C6", &["D1", "B5"])).unwrap();
    assert_eq!(giocata(&ev).esito.assi, cc(&["D1"]));
    assert_eq!(origini_su(&st.prese[0]), ["asso"]);
}

#[test]
fn avogadro_asso_su_asso_due_scope() {
    let mut st = Mk::new()
        .rules(av())
        .mano(0, &["C1", "S9", "B10"])
        .tavola(&["D1", "B5"])
        .build();
    let ev = st.apply(&play(0, "C1", &["D1"])).unwrap();
    assert_eq!(
        sorted(giocata(&ev).esito.assi.clone()),
        sorted(cc(&["C1", "D1"]))
    );
    assert_eq!(n_su(&st.prese[0]), 2);
}

#[test]
fn avogadro_asso_che_piglia_tutto_non_raddoppia() {
    let mut st = Mk::new()
        .rules(av())
        .mano(0, &["C1", "S9", "B10"])
        .tavola(&["D2", "S5"])
        .build();
    let ev = st.apply(&play(0, "C1", &["D2", "S5"])).unwrap();
    let g = giocata(&ev);
    assert!(g.esito.scopa && g.esito.assi.is_empty());
    assert_eq!(n_su(&st.prese[0]), 1);
}

#[test]
fn avogadro_asso_in_grande_non_raddoppia() {
    let mut st = Mk::new()
        .rules(av())
        .mano(0, &["C7", "S9", "B10"])
        .tavola(&["C1", "C6", "S10"])
        .build();
    let ev = st.apply(&play(0, "C7", &["C1", "C6"])).unwrap();
    let g = giocata(&ev);
    assert!(g.esito.grande && g.esito.assi.is_empty());
    assert_eq!(n_su(&st.prese[0]), 3);
}

#[test]
fn avogadro_ultima_giocata_e_residue_niente_scopa() {
    let mut st = Mk::new()
        .rules(av())
        .mano(3, &["C6"])
        .tavola(&["D1", "B5", "S1", "C9"])
        .ultima()
        .build();
    st.ultima_presa_squadra = Some(0);
    let ev = st.apply(&play(3, "C6", &["D1", "B5"])).unwrap();
    assert!(giocata(&ev).esito.assi.is_empty());
    let rie = fine_mano(&ev);
    assert_eq!(rie.squadre[1].scope, 0); // leftovers (S1, C9) to team 1, last capture
    assert_eq!(rie.squadre[0].scope, 0);
}

#[test]
fn avogadro_credito_dopo_assi() {
    let mut st = Mk::new()
        .rules(av())
        .mano(0, &["C6", "S9", "B10"])
        .tavola(&["D1", "B5", "S8"])
        .credito([1, 0])
        .build();
    st.apply(&play(0, "C6", &["D1", "B5"])).unwrap();
    assert_eq!(origini_su(&st.prese[0]), ["asso", "bazzica"]);
    assert_eq!(st.credito[0], 0);
}

// ------------------------------------------------------------------ game end

fn chiudi(st: &mut GameState, punti: [i32; 2]) -> Option<Vincitore> {
    st.punteggi = punti;
    st.controlla_fine()
}

#[test]
fn fine_partita_21_e_vantaggio() {
    let (mut st, _) = GameState::new_game(RulesConfig::default(), 1, None);
    assert_eq!(chiudi(&mut st, [21, 15]), Some(Vincitore::Squadra(0)));
    assert_eq!(chiudi(&mut st, [20, 15]), None);
    assert_eq!(chiudi(&mut st, [22, 25]), Some(Vincitore::Squadra(1)));
    assert_eq!(chiudi(&mut st, [10, 21]), Some(Vincitore::Squadra(1)));
}

#[test]
fn spareggio_ad_oltranza() {
    let (mut st, _) = GameState::new_game(RulesConfig::default(), 1, None);
    assert_eq!(chiudi(&mut st, [23, 23]), None);
}

#[test]
fn margherita_hack_quattro_mani_e_parita() {
    let (mut st, _) = GameState::new_game(RulesConfig::preset(Variante::Vanilla, true), 1, None);
    st.mano_n = 3;
    assert_eq!(chiudi(&mut st, [40, 5]), None);
    st.mano_n = 4;
    assert_eq!(chiudi(&mut st, [10, 10]), Some(Vincitore::Pari));
    assert_eq!(chiudi(&mut st, [3, 10]), Some(Vincitore::Squadra(1)));
}

#[test]
fn margherita_hack_ognuno_fa_il_mazziere() {
    let (mut st, _) = GameState::new_game(RulesConfig::preset(Variante::Vanilla, true), 5, None);
    let mut mazzieri: Vec<(u32, Seat)> = Vec::new();
    while st.fase != Fase::FinePartita {
        if let Some(sa) = st.system_action() {
            st.apply(&sa).unwrap();
            continue;
        }
        if st.turno == 1
            && !st.giocato_turno.iter().any(|&g| g)
            && mazzieri.last().is_none_or(|m| m.0 != st.mano_n)
        {
            mazzieri.push((st.mano_n, st.mazziere));
        }
        let s = st.required_actors()[0];
        let a = st
            .legal_actions(s)
            .into_iter()
            .find(|a| matches!(a, Azione::Gioca { .. } | Azione::Passa { .. }))
            .unwrap();
        st.apply(&a).unwrap();
    }
    assert_eq!(st.mano_n, 4);
    let mut m: Vec<Seat> = mazzieri.iter().map(|x| x.1).collect();
    m.sort();
    assert_eq!(m, [0, 1, 2, 3]);
}
