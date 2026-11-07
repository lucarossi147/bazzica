//! Deck points, Primiera, Scope, 2 of denari, Fermi, Napola (`tests/test_punteggio.py`).

use super::*;
use bazzica_core::Variante;
use bazzica_core::scoring::{Origine, napola, primiera, riepilogo_mano};

fn p(cards: &[Card], faceup: &[&str]) -> Vec<Entrata> {
    let fu = cc(faceup);
    cards
        .iter()
        .map(|&x| {
            if fu.contains(&x) {
                Entrata(x, true, Some(Origine::Sparecchio))
            } else {
                Entrata::coperta(x)
            }
        })
        .collect()
}

fn split(team0: &[Card]) -> (Vec<Card>, Vec<Card>) {
    MAZZO.iter().copied().partition(|x| team0.contains(x))
}

fn rie(a: &[Card], b: &[Card], variante: Variante) -> Riepilogo {
    riepilogo_mano(
        &[p(a, &[]), p(b, &[])],
        &RulesConfig::preset(variante, false),
    )
}

fn semi(s: Seme) -> Vec<Card> {
    MAZZO.iter().copied().filter(|x| x.seme() == s).collect()
}

#[test]
fn carte_pari_e_vinte() {
    let r = rie(&MAZZO[..20], &MAZZO[20..], Variante::Vanilla);
    assert_eq!(r.squadre[0].punti_mazzo_dettaglio.carte, 0);
    assert_eq!(r.squadre[1].punti_mazzo_dettaglio.carte, 0);
    let r = rie(&MAZZO[..21], &MAZZO[21..], Variante::Vanilla);
    assert_eq!(r.squadre[0].punti_mazzo_dettaglio.carte, 1);
}

#[test]
fn denari_pari() {
    let den = ["D1", "D2", "D3", "D4", "D5"];
    let (a, b) = split(&cc(&[&den[..], &["C1", "C2"]].concat()));
    let r = rie(&a, &b, Variante::Vanilla);
    assert_eq!(r.squadre[0].punti_mazzo_dettaglio.denari, 0);
    assert_eq!(r.squadre[1].punti_mazzo_dettaglio.denari, 0);
    let (a, b) = split(&cc(&[&den[..], &["D6"]].concat()));
    assert_eq!(
        rie(&a, &b, Variante::Vanilla).squadre[0]
            .punti_mazzo_dettaglio
            .denari,
        1
    );
}

#[test]
fn sette_bello_re_bello_e_due_denari() {
    let (a, b) = split(&cc(&["D7", "D10", "D2"]));
    let r = rie(&a, &b, Variante::Vanilla);
    let s0 = &r.squadre[0];
    assert_eq!(s0.punti_mazzo_dettaglio.sette_bello, 1);
    assert_eq!(s0.punti_mazzo_dettaglio.re_bello, 1);
    assert_eq!(s0.punti_due_denari, -5);
}

#[test]
fn primiera_calcolo() {
    let (pt, det, manc) = primiera(&cc(&["D7", "D6", "C1", "S5", "B10", "B2"]));
    assert_eq!(pt, 21 + 16 + 15 + 12);
    assert!(manc.is_empty());
    assert_eq!(det.bastoni.unwrap().carta, c("B2"));
}

#[test]
fn primiera_vinta_e_pari() {
    // team 0 holds all the 7s: it wins the Primiera
    let (a, b) = split(&cc(&["D7", "C7", "S7", "B7"]));
    assert_eq!(
        rie(&a, &b, Variante::Vanilla).squadre[0]
            .punti_mazzo_dettaglio
            .primiera,
        1
    );
    // tied Primiera: same best cards per suit
    let a = cc(&["D7", "C7", "S6", "B6"]);
    let b = cc(&["S7", "B7", "D6", "C6"]);
    let resto: Vec<Card> = MAZZO
        .iter()
        .copied()
        .filter(|x| !a.contains(x) && !b.contains(x) && x.val() >= 8)
        .collect();
    let r = rie(
        &[&a[..], &resto[..6]].concat(),
        &[&b[..], &resto[6..]].concat(),
        Variante::Vanilla,
    );
    assert_eq!(r.squadre[0].primiera, r.squadre[1].primiera);
    assert_eq!(r.squadre[0].punti_mazzo_dettaglio.primiera, 0);
    assert_eq!(r.squadre[1].punti_mazzo_dettaglio.primiera, 0);
}

#[test]
fn primiera_seme_mancante_una_squadra() {
    // team 0 holds every spade: team 1 misses a suit -> Primiera to team 0
    let (a, b) = split(&[semi(Seme::Spade), cc(&["D10", "C10", "B10"])].concat());
    let r = rie(&a, &b, Variante::Vanilla);
    assert_eq!(r.squadre[1].semi_mancanti, [Seme::Spade]);
    assert_eq!(r.squadre[0].punti_mazzo_dettaglio.primiera, 1);
}

#[test]
fn primiera_seme_mancante_entrambe() {
    let coppe = semi(Seme::Coppe);
    let (mut a, b) = split(&semi(Seme::Spade));
    let b: Vec<Card> = b.into_iter().filter(|x| !coppe.contains(x)).collect();
    a.extend(coppe);
    let r = rie(&a, &b, Variante::Vanilla);
    assert!(!r.squadre[0].semi_mancanti.is_empty() && !r.squadre[1].semi_mancanti.is_empty());
    assert_eq!(r.squadre[0].punti_mazzo_dettaglio.primiera, 0);
    assert_eq!(r.squadre[1].punti_mazzo_dettaglio.primiera, 0);
}

#[test]
fn scope_contano_e_totale_negativo() {
    let (a, b) = split(&cc(&["D2", "C3"]));
    let r = riepilogo_mano(&[p(&a, &["C3"]), p(&b, &[])], &RulesConfig::default());
    let s0 = &r.squadre[0];
    assert_eq!(s0.scope, 1);
    assert_eq!(s0.totale, s0.punti_mazzo + 1 - 5);
    assert!(s0.totale < 0);
}

#[test]
fn fermi_due_denari_e_napoleone() {
    let (a, b) = split(&semi(Seme::Denari));
    let r = rie(&a, &b, Variante::Fermi);
    let s0 = &r.squadre[0];
    // denari, 7 bello, re bello (team 0 misses 3 suits: no Primiera) -> 3 deck points
    assert_eq!(s0.punti_mazzo, 3);
    assert_eq!(s0.punti_due_denari, -2);
    assert_eq!(
        (s0.napola, s0.napola_nome.as_deref()),
        (10, Some("Napoleone"))
    );
    // without Fermi there is no Napoleone
    let r = rie(&a, &b, Variante::Heisenberg);
    assert_eq!(
        (r.squadre[0].napola, r.squadre[0].punti_due_denari),
        (0, -5)
    );
}

#[test]
fn fermi_due_denari_non_conta_napola() {
    let (a, b) = split(&cc(&["D1", "D2", "D3", "D4", "D5"]));
    let r = rie(&a, &b, Variante::FedericoII);
    let s0 = &r.squadre[0];
    assert_eq!(
        (s0.napola, s0.napola_nome.as_deref()),
        (5, Some("Napola al 5"))
    );
    assert_eq!(s0.punti_due_denari, -5 + s0.punti_mazzo);
}

#[test]
fn napola_lunghezza() {
    assert_eq!(napola(&cc(&["D1", "D2"])), 0);
    assert_eq!(napola(&cc(&["D1", "D2", "D3"])), 3);
    assert_eq!(napola(&cc(&["D1", "D2", "D3", "D4", "D5", "D7"])), 5);
    let (a, b) = split(&cc(&["D1", "D2", "D3"]));
    assert_eq!(rie(&a, &b, Variante::Fermi).squadre[0].napola, 0); // Napola only from Federico II
}

#[test]
fn napoleone_in_federico_vale_10_non_cumulato() {
    let (a, b) = split(&semi(Seme::Denari));
    assert_eq!(rie(&a, &b, Variante::FedericoII).squadre[0].napola, 10);
}
