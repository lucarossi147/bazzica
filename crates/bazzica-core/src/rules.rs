//! Rule configuration: each named variant is a cumulative set of flags.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Named variants, in cumulative order: each one adds a rule to the previous one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Variante {
    Vanilla,
    Heisenberg,
    Einstein,
    Fermi,
    Pascal,
    Curie,
    #[serde(rename = "Federico II")]
    FedericoII,
    Avogadro,
}

impl Variante {
    pub const TUTTE: [Variante; 8] = [
        Variante::Vanilla,
        Variante::Heisenberg,
        Variante::Einstein,
        Variante::Fermi,
        Variante::Pascal,
        Variante::Curie,
        Variante::FedericoII,
        Variante::Avogadro,
    ];

    pub fn nome(self) -> &'static str {
        match self {
            Variante::Vanilla => "Vanilla",
            Variante::Heisenberg => "Heisenberg",
            Variante::Einstein => "Einstein",
            Variante::Fermi => "Fermi",
            Variante::Pascal => "Pascal",
            Variante::Curie => "Curie",
            Variante::FedericoII => "Federico II",
            Variante::Avogadro => "Avogadro",
        }
    }

    pub fn from_nome(nome: &str) -> Option<Variante> {
        Variante::TUTTE.into_iter().find(|v| v.nome() == nome)
    }
}

impl fmt::Display for Variante {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.nome())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulesConfig {
    /// Heisenberg: card exchange with the partner each turn.
    pub scambio: bool,
    /// A triple containing the 7 bello is worth 5 Scope.
    pub einstein: bool,
    /// 2 of denari = -5 + deck points; Napoleone = 10.
    pub fermi: bool,
    /// Aces of a player who called Bazzica are worth 1.
    pub pascal: bool,
    /// Piccola and Grande.
    pub curie: bool,
    /// Napola.
    pub federico: bool,
    /// Every captured ace is a Scopa.
    pub avogadro: bool,
    /// Exactly 4 hands.
    pub margherita_hack: bool,
    pub punti_vittoria: i32,
    pub nome_variante: Variante,
}

impl RulesConfig {
    pub fn preset(variante: Variante, margherita_hack: bool) -> RulesConfig {
        let livello = Variante::TUTTE.iter().position(|v| *v == variante).unwrap();
        let on = |v: Variante| livello >= Variante::TUTTE.iter().position(|x| *x == v).unwrap();
        RulesConfig {
            scambio: on(Variante::Heisenberg),
            einstein: on(Variante::Einstein),
            fermi: on(Variante::Fermi),
            pascal: on(Variante::Pascal),
            curie: on(Variante::Curie),
            federico: on(Variante::FedericoII),
            avogadro: on(Variante::Avogadro),
            margherita_hack,
            punti_vittoria: 21,
            nome_variante: variante,
        }
    }

    pub fn descrizione(&self) -> String {
        let mut s = self.nome_variante.nome().to_owned();
        if self.margherita_hack {
            s.push_str(" + Margherita Hack");
        }
        s
    }
}

impl Default for RulesConfig {
    fn default() -> Self {
        RulesConfig::preset(Variante::Vanilla, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_cumulative() {
        let v = RulesConfig::preset(Variante::Vanilla, false);
        assert!(!v.scambio && !v.avogadro);
        let f = RulesConfig::preset(Variante::Fermi, true);
        assert!(f.scambio && f.einstein && f.fermi && !f.pascal && f.margherita_hack);
        assert_eq!(f.descrizione(), "Fermi + Margherita Hack");
        let a = RulesConfig::preset(Variante::Avogadro, false);
        assert!(
            a.scambio && a.einstein && a.fermi && a.pascal && a.curie && a.federico && a.avogadro
        );
        assert_eq!(
            serde_json::to_value(Variante::FedericoII).unwrap(),
            "Federico II"
        );
    }
}
