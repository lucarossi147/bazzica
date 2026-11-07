//! Captured-card piles and end-of-hand scoring.

use serde::{Deserialize, Serialize};

use crate::cards::{Card, DUE_DENARI, RE_BELLO, SETTE_BELLO, Seme};
use crate::rules::RulesConfig;

/// Why a captured card was turned face up (i.e. counts as a Scopa).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origine {
    Sparecchio,
    Bazzica,
    Piccola,
    Grande,
    Asso,
}

/// A card in a team's capture pile: serialized as `[carta, faccia_in_su, origine]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entrata(pub Card, pub bool, pub Option<Origine>);

impl Entrata {
    pub fn coperta(carta: Card) -> Entrata {
        Entrata(carta, false, None)
    }

    pub fn carta(&self) -> Card {
        self.0
    }

    pub fn su(&self) -> bool {
        self.1
    }

    pub fn gira(&mut self, origine: Origine) {
        self.1 = true;
        self.2 = Some(origine);
    }
}

pub fn punti_primiera(c: Card) -> u32 {
    match c.val() {
        7 => 21,
        6 => 18,
        1 => 16,
        5 => 15,
        4 => 14,
        3 => 13,
        2 => 12,
        _ => 10,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigliorePrimiera {
    pub carta: Card,
    pub punti: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrimieraDettaglio {
    pub denari: Option<MigliorePrimiera>,
    pub coppe: Option<MigliorePrimiera>,
    pub spade: Option<MigliorePrimiera>,
    pub bastoni: Option<MigliorePrimiera>,
}

/// Returns (score, best card per suit, missing suits). Ties keep the first card found.
pub fn primiera(carte: &[Card]) -> (u32, PrimieraDettaglio, Vec<Seme>) {
    let mut migliori: [Option<MigliorePrimiera>; 4] = Default::default();
    let mut mancanti = Vec::new();
    for (i, s) in Seme::TUTTI.into_iter().enumerate() {
        let mut best: Option<Card> = None;
        for &c in carte.iter().filter(|c| c.seme() == s) {
            if best.is_none_or(|b| punti_primiera(c) > punti_primiera(b)) {
                best = Some(c);
            }
        }
        match best {
            Some(c) => migliori[i] = Some(MigliorePrimiera { carta: c, punti: punti_primiera(c) }),
            None => mancanti.push(s),
        }
    }
    let tot = migliori.iter().flatten().map(|m| m.punti).sum();
    let [denari, coppe, spade, bastoni] = migliori;
    (tot, PrimieraDettaglio { denari, coppe, spade, bastoni }, mancanti)
}

/// Length of the denari run starting from the ace (0 if shorter than A-2-3).
pub fn napola(carte: &[Card]) -> u32 {
    let n = (1..=10).take_while(|&v| carte.contains(&Card::new(Seme::Denari, v))).count() as u32;
    if n >= 3 { n } else { 0 }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeDettaglio {
    pub sparecchio: u32,
    pub bazzica: u32,
    pub piccola: u32,
    pub grande: u32,
    pub asso: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PuntiMazzo {
    pub carte: i32,
    pub denari: i32,
    pub primiera: i32,
    pub sette_bello: i32,
    pub re_bello: i32,
}

impl PuntiMazzo {
    fn totale(&self) -> i32 {
        self.carte + self.denari + self.primiera + self.sette_bello + self.re_bello
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiepilogoSquadra {
    pub carte: u32,
    pub denari: u32,
    pub primiera: u32,
    pub primiera_dettaglio: PrimieraDettaglio,
    pub semi_mancanti: Vec<Seme>,
    pub sette_bello: bool,
    pub re_bello: bool,
    pub due_denari: bool,
    pub scope_dettaglio: ScopeDettaglio,
    pub scope: u32,
    pub napola_len: u32,
    pub punti_mazzo_dettaglio: PuntiMazzo,
    pub punti_mazzo: i32,
    pub punti_due_denari: i32,
    pub napola: u32,
    pub napola_nome: Option<String>,
    pub totale: i32,
}

/// End-of-hand summary. `mano`, `mazziere`, `credito_perso` and `punteggi` are filled in
/// by the game when the hand closes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Riepilogo {
    pub squadre: [RiepilogoSquadra; 2],
    pub totali: [i32; 2],
    pub mano: u32,
    pub mazziere: u8,
    pub credito_perso: [u32; 2],
    pub punteggi: [i32; 2],
}

fn riepilogo_squadra(prese: &[Entrata]) -> RiepilogoSquadra {
    let carte: Vec<Card> = prese.iter().map(Entrata::carta).collect();
    let (pt, det, manc) = primiera(&carte);
    let mut scope = ScopeDettaglio::default();
    for e in prese.iter().filter(|e| e.su()) {
        let slot = match e.2.unwrap_or(Origine::Sparecchio) {
            Origine::Sparecchio => &mut scope.sparecchio,
            Origine::Bazzica => &mut scope.bazzica,
            Origine::Piccola => &mut scope.piccola,
            Origine::Grande => &mut scope.grande,
            Origine::Asso => &mut scope.asso,
        };
        *slot += 1;
    }
    let tot_scope = scope.sparecchio + scope.bazzica + scope.piccola + scope.grande + scope.asso;
    RiepilogoSquadra {
        carte: carte.len() as u32,
        denari: carte.iter().filter(|c| c.seme() == Seme::Denari).count() as u32,
        primiera: pt,
        primiera_dettaglio: det,
        semi_mancanti: manc,
        sette_bello: carte.contains(&SETTE_BELLO),
        re_bello: carte.contains(&RE_BELLO),
        due_denari: carte.contains(&DUE_DENARI),
        scope_dettaglio: scope,
        scope: tot_scope,
        napola_len: napola(&carte),
        punti_mazzo_dettaglio: PuntiMazzo::default(),
        punti_mazzo: 0,
        punti_due_denari: 0,
        napola: 0,
        napola_nome: None,
        totale: 0,
    }
}

/// Scores a hand from both teams' capture piles.
pub fn riepilogo_mano(prese: &[Vec<Entrata>; 2], rules: &RulesConfig) -> Riepilogo {
    let mut sq = [riepilogo_squadra(&prese[0]), riepilogo_squadra(&prese[1])];
    let mut punti = [PuntiMazzo::default(), PuntiMazzo::default()];
    let (a, b) = (&sq[0], &sq[1]);
    if a.carte > 20 {
        punti[0].carte = 1;
    } else if b.carte > 20 {
        punti[1].carte = 1;
    }
    if a.denari > 5 {
        punti[0].denari = 1;
    } else if b.denari > 5 {
        punti[1].denari = 1;
    }
    let (ma, mb) = (!a.semi_mancanti.is_empty(), !b.semi_mancanti.is_empty());
    match (ma, mb) {
        (true, false) => punti[1].primiera = 1,
        (false, true) => punti[0].primiera = 1,
        (false, false) if a.primiera > b.primiera => punti[0].primiera = 1,
        (false, false) if b.primiera > a.primiera => punti[1].primiera = 1,
        _ => {}
    }
    for t in 0..2 {
        punti[t].sette_bello = i32::from(sq[t].sette_bello);
        punti[t].re_bello = i32::from(sq[t].re_bello);
    }

    for (d, p) in sq.iter_mut().zip(punti) {
        d.punti_mazzo = p.totale();
        d.punti_mazzo_dettaglio = p;
        d.punti_due_denari = if d.due_denari { -5 + if rules.fermi { d.punti_mazzo } else { 0 } } else { 0 };
        if rules.federico && d.napola_len >= 3 {
            d.napola = d.napola_len;
            d.napola_nome = Some(if d.napola_len == 10 {
                "Napoleone".to_owned()
            } else {
                format!("Napola al {}", d.napola_len)
            });
        } else if rules.fermi && d.napola_len == 10 {
            d.napola = 10;
            d.napola_nome = Some("Napoleone".to_owned());
        }
        d.totale = d.punti_mazzo + d.scope as i32 + d.punti_due_denari + d.napola as i32;
    }
    let totali = [sq[0].totale, sq[1].totale];
    Riepilogo { squadre: sq, totali, mano: 0, mazziere: 0, credito_perso: [0, 0], punteggi: [0, 0] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primiera_and_napola() {
        let carte: Vec<Card> = ["denari_01", "denari_02", "denari_03", "denari_05", "coppe_07", "coppe_06"]
            .iter()
            .map(|s| Card::from_codice(s).unwrap())
            .collect();
        let (pt, det, manc) = primiera(&carte);
        assert_eq!(pt, 16 + 21);
        assert_eq!(det.denari.unwrap().carta.codice(), "denari_01");
        assert_eq!(manc, [Seme::Spade, Seme::Bastoni]);
        assert_eq!(napola(&carte), 3);
        assert_eq!(napola(&carte[1..]), 0);
    }
}
