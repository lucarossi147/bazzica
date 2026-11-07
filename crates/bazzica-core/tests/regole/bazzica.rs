//! Bazziche, the 7 bello wildcard, credit, "Che Palo!" (`tests/test_bazzica.py`).

use super::*;
use bazzica_core::Variante;
use bazzica_core::bazzica::{TipoBazzica, opzioni_bazzica, puo_dire_che_palo};
use bazzica_core::game::DatiEvento;
use bazzica_core::scoring::Origine;
use std::collections::BTreeMap;

fn v() -> RulesConfig {
    RulesConfig::default()
}

fn tipi(mano: &[&str], r: &RulesConfig) -> Vec<(Option<u8>, TipoBazzica, u32)> {
    opzioni_bazzica(&cc(mano), r)
        .into_iter()
        .map(|o| (o.sette, o.tipo, o.scope))
        .collect()
}

use TipoBazzica::*;

#[test]
fn tipi_base() {
    assert_eq!(tipi(&["C1", "S3", "B5"], &v()), [(None, Diverse, 2)]);
    assert_eq!(tipi(&["C1", "S3", "B6"], &v()), []); // sum 10
    assert_eq!(tipi(&["C2", "S2", "B5"], &v()), [(None, Coppia, 3)]);
    assert_eq!(tipi(&["C2", "S2", "B6"], &v()), []); // pair, sum 10
    assert_eq!(tipi(&["C9", "S9", "B9"], &v()), [(None, Tris, 7)]);
    assert_eq!(tipi(&["C2", "S2", "B2"], &v()), [(None, Tris, 7)]); // not cumulative
}

#[test]
fn sette_bello_jolly() {
    let d: BTreeMap<_, _> = tipi(&["D7", "S1", "B3"], &v())
        .into_iter()
        .map(|(s, t, n)| (s.unwrap(), (t, n)))
        .collect();
    assert_eq!(d[&5], (Diverse, 2));
    assert_eq!(d[&1], (Coppia, 3));
    assert_eq!(d[&3], (Coppia, 3));
    assert!(!d.contains_key(&7)); // 1+3+7 = 11
    assert_eq!(tipi(&["D7", "S10", "B10"], &v()), [(Some(10), Tris, 7)]);
    assert_eq!(tipi(&["D7", "S9", "B10"], &v()), []);
}

#[test]
fn einstein_tris_con_sette_bello_vale_5() {
    let e = RulesConfig::preset(Variante::Einstein, false);
    assert_eq!(tipi(&["D7", "S10", "B10"], &e), [(Some(10), Tris, 5)]);
    assert!(tipi(&["D7", "S7", "B7"], &e).contains(&(Some(7), Tris, 5)));
    assert_eq!(tipi(&["C9", "S9", "B9"], &e), [(None, Tris, 7)]);
    let d: BTreeMap<_, _> = tipi(&["D7", "S3", "B3"], &e)
        .into_iter()
        .map(|(s, _, n)| (s.unwrap(), n))
        .collect();
    assert_eq!((d[&3], d[&1]), (5, 3));
}

fn baz_state(mano0: &[&str], tavola: &[&str]) -> Mk {
    Mk::new()
        .mano(0, mano0)
        .mano(1, &["C9", "S9", "B9"])
        .mano(2, &["D4", "D5", "D6"])
        .mano(3, &["C10", "S10", "B10"])
        .tavola(tavola)
}

const MANO0: &[&str] = &["C1", "S3", "B5"];

fn gioca_carta(st: &GameState, seat: Seat, carta: &str) -> Vec<Vec<Card>> {
    st.legal_actions(seat)
        .into_iter()
        .filter_map(|a| match a {
            Azione::Gioca {
                carta: x, presa, ..
            } if x == c(carta) => Some(presa),
            _ => None,
        })
        .collect()
}

#[test]
fn bazzica_facoltativa_e_prima_della_prima_carta() {
    let mut st = baz_state(MANO0, &["D9"]).build();
    let acts = st.legal_actions(0);
    assert!(acts.iter().any(|a| matches!(a, Azione::Bazzica { .. })));
    let g = acts
        .iter()
        .find(|a| matches!(a, Azione::Gioca { .. }))
        .unwrap()
        .clone();
    st.apply(&g).unwrap();
    // no longer possible in the next round
    st.corrente = 0;
    st.mani[0].push(c("B2"));
    assert!(
        !st.legal_actions(0)
            .iter()
            .any(|a| matches!(a, Azione::Bazzica { .. }))
    );
}

#[test]
fn bazzica_gira_carte_e_crea_credito() {
    let mut st = baz_state(MANO0, &["D9"]).prese(0, &["C2"]).build();
    let ev = st.apply(&bazzica(0, None)).unwrap();
    assert!(matches!(ev[0].dati, DatiEvento::Bazzica { girate: 1, .. }));
    assert_eq!(st.credito[0], 1);
    assert_eq!(
        st.prese[0][0],
        Entrata(c("C2"), true, Some(Origine::Bazzica))
    );
}

#[test]
fn bazzica_mano_visibile_a_tutti() {
    let mut st = baz_state(MANO0, &["D9"]).build();
    assert!(st.observation(1).altri[&0].carte.is_none());
    st.apply(&bazzica(0, None)).unwrap();
    let o = st.observation(1);
    assert_eq!(
        sorted(o.altri[&0].carte.clone().unwrap()),
        sorted(cc(MANO0))
    );
    assert!(o.altri[&2].carte.is_none());
}

#[test]
fn credito_riscosso_con_le_prese() {
    let mut st = baz_state(MANO0, &["D9"]).build();
    st.apply(&bazzica(0, None)).unwrap(); // 2 Scope, no cards yet: credit 2
    assert_eq!(st.credito[0], 2);
    st.tavola = cc(&["D3"]);
    st.apply(&play(0, "S3", &["D3"])).unwrap();
    // sweep: S3 face up (Scopa) + D3 pays 1 credit
    assert_eq!(st.credito[0], 1);
    assert_eq!(origini_su(&st.prese[0]), ["bazzica", "sparecchio"]);
}

#[test]
fn credito_parziale_e_perso_a_fine_mano() {
    let mut st = Mk::new()
        .mano(3, &["C9"])
        .tavola(&["D3", "S6"])
        .ultima()
        .credito([0, 5])
        .build();
    let ev = st.apply(&play(3, "C9", &["D3", "S6"])).unwrap();
    // last play: no Scopa; 3 cards pay 3 credit, 2 are lost
    let rie = fine_mano(&ev);
    assert_eq!(rie.credito_perso, [0, 2]);
    assert_eq!(rie.squadre[1].scope_dettaglio.bazzica, 3);
}

#[test]
fn credito_riscosso_con_carte_residue() {
    let mut st = Mk::new()
        .mano(3, &["C9"])
        .tavola(&["D3", "S5"])
        .ultima()
        .credito([2, 0])
        .build();
    st.ultima_presa_squadra = Some(0);
    let ev = st.apply(&play(3, "C9", &[])).unwrap();
    let rie = fine_mano(&ev);
    assert_eq!(rie.squadre[0].scope_dettaglio.bazzica, 2);
    assert_eq!(rie.credito_perso, [0, 0]);
}

#[test]
fn sette_bello_prende_con_valore_assegnato() {
    let mut st = baz_state(&["D7", "S1", "B3"], &["C5", "S2", "C3"]).build();
    st.apply(&bazzica(0, Some(5))).unwrap();
    assert_eq!(gioca_carta(&st, 0, "D7"), [cc(&["C5"])]); // the single 5 is mandatory
}

#[test]
fn sette_bello_combinazione_con_valore_assegnato() {
    let mut st = baz_state(&["D7", "S1", "B3"], &["C2", "S3"]).build();
    st.apply(&bazzica(0, Some(5))).unwrap();
    let opts: Vec<_> = gioca_carta(&st, 0, "D7").into_iter().map(sorted).collect();
    assert_eq!(opts, [sorted(cc(&["C2", "S3"]))]);
}

#[test]
fn sette_bello_torna_7_se_non_prende() {
    let mut st = baz_state(&["D7", "S1", "B3"], &["C9"]).build();
    st.apply(&bazzica(0, Some(5))).unwrap();
    st.apply(&play(0, "D7", &[])).unwrap();
    assert!(st.tavola.contains(&c("D7")));
    // the next player takes the 7 bello with a 7 (it is worth 7 again, not 5)
    st.mani[1] = cc(&["C7", "S9", "B9"]);
    assert_eq!(gioca_carta(&st, 1, "C7"), [cc(&["D7"])]);
}

#[test]
fn sette_bello_valore_1_non_piglia_tutto() {
    let mut st = baz_state(&["D7", "S1", "B3"], &["C5", "S2"]).build();
    st.apply(&bazzica(0, Some(1))).unwrap();
    assert_eq!(gioca_carta(&st, 0, "D7"), [Vec::<Card>::new()]);
    let mut st = baz_state(&["D7", "S1", "B3"], &["C5", "B1"]).build();
    st.apply(&bazzica(0, Some(1))).unwrap();
    assert_eq!(gioca_carta(&st, 0, "D7"), [cc(&["B1"])]);
}

#[test]
fn asso_in_bazzica_vanilla_piglia_tutto() {
    let mut st = baz_state(MANO0, &["C5", "S2"]).build();
    st.apply(&bazzica(0, None)).unwrap();
    let opts: Vec<_> = gioca_carta(&st, 0, "C1").into_iter().map(sorted).collect();
    assert_eq!(opts, [sorted(cc(&["C5", "S2"]))]);
}

// ---------------------------------------------------------------- Che Palo!

#[test]
fn che_palo_condizioni() {
    assert!(puo_dire_che_palo(&cc(&["C5", "S5", "B6"]), &v())); // example from the rules
    assert!(!puo_dire_che_palo(&cc(&["C1", "S3", "B5"]), &v())); // already a Bazzica
    assert!(!puo_dire_che_palo(&cc(&["C8", "S9", "B10"]), &v()));
    assert!(puo_dire_che_palo(&cc(&["D7", "S9", "B10"]), &v())); // 9 -> 10 makes a triple with the 7 bello
}

fn tipi_legali(st: &GameState, seat: Seat) -> Vec<&'static str> {
    st.legal_actions(seat).iter().map(Azione::tipo).collect()
}

#[test]
fn che_palo_sempre_disponibile() {
    let mut st = Mk::new()
        .mano(0, &["C8", "S9", "B10"])
        .mano(1, &["C5", "S5", "B6"])
        .mano(2, &["D8", "D9", "C10"])
        .mano(3, &["S8", "B8", "B9"])
        .tavola(&["D1"])
        .build();
    assert_eq!(st.corrente, 0);
    // off turn, with a meaningful palo: the bot is asked (attendi = do not exclaim)
    assert_eq!(tipi_legali(&st, 1), ["che_palo", "attendi"]);
    let ev = st.apply(&Azione::ChePalo { seat: 1 }).unwrap();
    assert!(matches!(ev[0].dati, DatiEvento::ChePalo { seat: 1 }) && ev[0].visibile.is_none());
    // the button stays available, even repeatedly
    assert_eq!(tipi_legali(&st, 1), ["che_palo"]);
    st.apply(&Azione::ChePalo { seat: 1 }).unwrap();
    // also with a hand far from a Bazzica and after playing
    assert!(tipi_legali(&st, 0).contains(&"che_palo"));
    st.apply(&play(0, "C8", &[])).unwrap();
    assert!(tipi_legali(&st, 0).contains(&"che_palo"));
}

#[test]
fn che_palo_durante_lo_scambio() {
    let st = Mk::new()
        .rules(RulesConfig::preset(Variante::Heisenberg, false))
        .fase(Fase::Scambio)
        .mano(0, &["C8", "S9", "B10"])
        .build();
    assert!(tipi_legali(&st, 0).contains(&"che_palo"));
}

#[test]
fn bot_offerta_palo_solo_se_sensato() {
    let st = Mk::new()
        .mano(0, &["C8", "S9", "B10"])
        .mano(1, &["C5", "S5", "B6"])
        .mano(2, &["D8", "D9", "C10"])
        .tavola(&["D1"])
        .build();
    assert_eq!(st.optional_actors(), [1]);
}
