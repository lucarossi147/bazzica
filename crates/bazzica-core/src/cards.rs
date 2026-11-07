//! Piacentine cards. A card serializes as `"{seme}_{valore:02}"` (e.g. `"denari_07"`),
//! the same convention as the Python engine and the image file names.

use std::fmt;
use std::sync::LazyLock;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Seme {
    Denari,
    Coppe,
    Spade,
    Bastoni,
}

impl Seme {
    pub const TUTTI: [Seme; 4] = [Seme::Denari, Seme::Coppe, Seme::Spade, Seme::Bastoni];

    pub fn nome(self) -> &'static str {
        match self {
            Seme::Denari => "denari",
            Seme::Coppe => "coppe",
            Seme::Spade => "spade",
            Seme::Bastoni => "bastoni",
        }
    }
}

/// A card, stored as its index in [`MAZZO`] (`seme * 10 + valore - 1`).
///
/// [`Card::IGNOTA`] is the `"?"` placeholder used when rebuilding a partial state from a
/// player's observation (hidden hands and face-down captures); it has no value or suit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Card(u8);

pub const SETTE_BELLO: Card = Card(6);
pub const RE_BELLO: Card = Card(9);
pub const DUE_DENARI: Card = Card(1);

/// The 40-card deck in canonical order: denari 1..10, coppe, spade, bastoni.
pub static MAZZO: LazyLock<Vec<Card>> = LazyLock::new(|| (0..40).map(Card).collect());

static CODICI: LazyLock<Vec<String>> = LazyLock::new(|| {
    let mut v: Vec<String> = Seme::TUTTI
        .iter()
        .flat_map(|s| (1..=10).map(move |n| format!("{}_{n:02}", s.nome())))
        .collect();
    v.push("?".to_owned());
    v
});

const NOMI_VALORE: [&str; 10] = [
    "Asso", "Due", "Tre", "Quattro", "Cinque", "Sei", "Sette", "Fante", "Cavallo", "Re",
];

impl Card {
    pub const IGNOTA: Card = Card(40);

    pub fn new(seme: Seme, valore: u8) -> Card {
        assert!((1..=10).contains(&valore), "invalid card value {valore}");
        Card(seme as u8 * 10 + valore - 1)
    }

    pub fn val(self) -> u8 {
        assert!(self != Card::IGNOTA, "value of an unknown card");
        self.0 % 10 + 1
    }

    pub fn seme(self) -> Seme {
        assert!(self != Card::IGNOTA, "suit of an unknown card");
        Seme::TUTTI[usize::from(self.0 / 10)]
    }

    pub fn is_asso(self) -> bool {
        self.val() == 1
    }

    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    /// Wire code, e.g. `"denari_07"`.
    pub fn codice(self) -> &'static str {
        &CODICI[self.index()]
    }

    pub fn from_codice(s: &str) -> Option<Card> {
        CODICI.iter().position(|c| c == s).map(|i| Card(i as u8))
    }

    /// Human-readable Italian name, e.g. "Sette bello" or "Fante di coppe".
    pub fn nome(self) -> String {
        match self {
            SETTE_BELLO => "Sette bello".to_owned(),
            RE_BELLO => "Re bello".to_owned(),
            _ => format!(
                "{} di {}",
                NOMI_VALORE[usize::from(self.val() - 1)],
                self.seme().nome()
            ),
        }
    }
}

/// Comma-separated card names ("nessuna" when empty).
pub fn nomi(cards: &[Card]) -> String {
    if cards.is_empty() {
        return "nessuna".to_owned();
    }
    cards
        .iter()
        .map(|c| c.nome())
        .collect::<Vec<_>>()
        .join(", ")
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.codice())
    }
}

impl Serialize for Card {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.codice())
    }
}

impl<'de> Deserialize<'de> for Card {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Card, D::Error> {
        let s = String::deserialize(d)?;
        Card::from_codice(&s).ok_or_else(|| de::Error::custom(format!("unknown card {s:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_and_values() {
        assert_eq!(SETTE_BELLO.codice(), "denari_07");
        assert_eq!(RE_BELLO.codice(), "denari_10");
        assert_eq!(DUE_DENARI.codice(), "denari_02");
        assert_eq!(MAZZO[39].codice(), "bastoni_10");
        let c = Card::new(Seme::Spade, 8);
        assert_eq!((c.val(), c.seme()), (8, Seme::Spade));
        assert_eq!(Card::from_codice("spade_08"), Some(c));
        assert_eq!(c.nome(), "Fante di spade");
        assert_eq!(Card::IGNOTA.codice(), "?");
    }
}
