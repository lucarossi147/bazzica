//! Bazzica evaluation (calling bonus Scope from the 3 cards in hand) and "Che Palo!".

use serde::{Deserialize, Serialize};

use crate::cards::{Card, SETTE_BELLO};
use crate::rules::RulesConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TipoBazzica {
    Tris,
    Coppia,
    Diverse,
}

impl TipoBazzica {
    pub fn nome(self) -> &'static str {
        match self {
            TipoBazzica::Tris => "Tris",
            TipoBazzica::Coppia => "Coppia con somma ≤ 9",
            TipoBazzica::Diverse => "Tre carte diverse con somma ≤ 9",
        }
    }
}

/// A callable Bazzica. `sette` is the value assigned to the 7 bello, if held.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpzioneBazzica {
    pub sette: Option<u8>,
    pub tipo: TipoBazzica,
    pub scope: u32,
}

impl OpzioneBazzica {
    pub fn descrizione(&self) -> String {
        let mut s = format!("{} — {} Scope", self.tipo.nome(), self.scope);
        if let Some(v) = self.sette {
            s.push_str(&format!(" (7 bello = {v})"));
        }
        s
    }
}

/// Best Bazzica for three values; `con_sette_bello` matters for Einstein (triple = 5).
pub fn valuta_valori(
    valori: &[u8],
    rules: &RulesConfig,
    con_sette_bello: bool,
) -> Option<(TipoBazzica, u32)> {
    let [a, b, c] = *valori else { return None };
    let somma = a + b + c;
    let distinti = 1 + u8::from(b != a) + u8::from(c != a && c != b);
    match distinti {
        1 => Some((
            TipoBazzica::Tris,
            if rules.einstein && con_sette_bello {
                5
            } else {
                7
            },
        )),
        2 if somma <= 9 => Some((TipoBazzica::Coppia, 3)),
        3 if somma <= 9 => Some((TipoBazzica::Diverse, 2)),
        _ => None,
    }
}

fn valori_con_sette(mano: &[Card], v: u8) -> Vec<u8> {
    mano.iter()
        .map(|&c| if c == SETTE_BELLO { v } else { c.val() })
        .collect()
}

/// Bazziche callable with a 3-card hand; with the 7 bello, one option per value 1..=10
/// that yields a valid Bazzica.
pub fn opzioni_bazzica(mano: &[Card], rules: &RulesConfig) -> Vec<OpzioneBazzica> {
    if mano.len() != 3 {
        return Vec::new();
    }
    if mano.contains(&SETTE_BELLO) {
        (1..=10)
            .filter_map(|v| {
                valuta_valori(&valori_con_sette(mano, v), rules, true).map(|(tipo, scope)| {
                    OpzioneBazzica {
                        sette: Some(v),
                        tipo,
                        scope,
                    }
                })
            })
            .collect()
    } else {
        let valori: Vec<u8> = mano.iter().map(|c| c.val()).collect();
        valuta_valori(&valori, rules, false)
            .map(|(tipo, scope)| OpzioneBazzica {
                sette: None,
                tipo,
                scope,
            })
            .into_iter()
            .collect()
    }
}

/// True if the hand is not a Bazzica but is one card away from one.
pub fn puo_dire_che_palo(mano: &[Card], rules: &RulesConfig) -> bool {
    if mano.len() != 3 || !opzioni_bazzica(mano, rules).is_empty() {
        return false;
    }
    (0..3).any(|i| {
        let resto: Vec<Card> = mano
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, c)| *c)
            .collect();
        let ha_sette = resto.contains(&SETTE_BELLO);
        (1..=10).any(|w| {
            if ha_sette {
                (1..=10).any(|v| {
                    let mut valori = valori_con_sette(&resto, v);
                    valori.push(w);
                    valuta_valori(&valori, rules, true).is_some()
                })
            } else {
                let mut valori: Vec<u8> = resto.iter().map(|c| c.val()).collect();
                valori.push(w);
                valuta_valori(&valori, rules, false).is_some()
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::Seme::*;
    use crate::rules::Variante;

    fn c(s: crate::cards::Seme, v: u8) -> Card {
        Card::new(s, v)
    }

    #[test]
    fn kinds() {
        let r = RulesConfig::default();
        assert_eq!(
            valuta_valori(&[5, 5, 5], &r, false),
            Some((TipoBazzica::Tris, 7))
        );
        assert_eq!(
            valuta_valori(&[2, 2, 5], &r, false),
            Some((TipoBazzica::Coppia, 3))
        );
        assert_eq!(
            valuta_valori(&[1, 3, 5], &r, false),
            Some((TipoBazzica::Diverse, 2))
        );
        assert_eq!(valuta_valori(&[2, 3, 5], &r, false), None);
        let e = RulesConfig::preset(Variante::Einstein, false);
        assert_eq!(
            valuta_valori(&[5, 5, 5], &e, true),
            Some((TipoBazzica::Tris, 5))
        );
    }

    #[test]
    fn sette_bello_options() {
        let r = RulesConfig::default();
        let ops = opzioni_bazzica(&[SETTE_BELLO, c(Spade, 1), c(Bastoni, 3)], &r);
        // 7 bello as 1 or 3 -> pair; as 2,4,5 -> three different values with sum <= 9
        let settes: Vec<_> = ops.iter().map(|o| o.sette.unwrap()).collect();
        assert_eq!(settes, [1, 2, 3, 4, 5]);
        assert_eq!(
            ops[0].descrizione(),
            "Coppia con somma ≤ 9 — 3 Scope (7 bello = 1)"
        );
        assert!(puo_dire_che_palo(
            &[c(Coppe, 5), c(Spade, 5), c(Bastoni, 6)],
            &r
        ));
    }
}
