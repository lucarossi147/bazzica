//! Rule tests ported from the Python suite in `tests/` (one module per Python file).

mod bazzica;
mod bot;
mod prese;
mod punteggio;
mod scambio;
mod simulazione;
mod varianti;

use bazzica_core::game::{DatiEvento, GiocataEvento};
use bazzica_core::scoring::{Entrata, Riepilogo};
use bazzica_core::{
    Azione, Card, Evento, Fase, GameState, MAZZO, Mulberry32, RulesConfig, Seat, Seme,
};

/// `"D7"` -> 7 of denari, `"B10"` -> king of bastoni.
pub fn c(s: &str) -> Card {
    let seme = match &s[..1] {
        "D" => Seme::Denari,
        "C" => Seme::Coppe,
        "S" => Seme::Spade,
        "B" => Seme::Bastoni,
        _ => panic!("bad suit in {s}"),
    };
    Card::new(seme, s[1..].parse().unwrap())
}

pub fn cc(xs: &[&str]) -> Vec<Card> {
    xs.iter().map(|s| c(s)).collect()
}

pub fn sorted(mut v: Vec<Card>) -> Vec<Card> {
    v.sort();
    v
}

pub fn play(seat: Seat, carta: &str, presa: &[&str]) -> Azione {
    Azione::Gioca {
        seat,
        carta: c(carta),
        presa: cc(presa),
    }
}

pub fn bazzica(seat: Seat, sette: Option<u8>) -> Azione {
    Azione::Bazzica { seat, sette }
}

/// Builder for test positions, mirroring `make_state` in `tests/conftest.py`.
pub struct Mk {
    rules: RulesConfig,
    mani: [Vec<Card>; 4],
    tavola: Vec<Card>,
    turno: u8,
    mazziere: Seat,
    corrente: Option<Seat>,
    prese: [Vec<Card>; 2],
    credito: [u32; 2],
    mazzo: Option<Vec<Card>>,
    giocato: Option<[bool; 4]>,
    piccola_target: Option<Card>,
    fase: Fase,
}

impl Default for Mk {
    fn default() -> Self {
        Self::new()
    }
}

impl Mk {
    pub fn new() -> Mk {
        Mk {
            rules: RulesConfig::default(),
            mani: Default::default(),
            tavola: Vec::new(),
            turno: 1,
            mazziere: 3,
            corrente: None,
            prese: Default::default(),
            credito: [0, 0],
            mazzo: None,
            giocato: None,
            piccola_target: None,
            fase: Fase::Gioco,
        }
    }
    pub fn rules(mut self, r: RulesConfig) -> Mk {
        self.rules = r;
        self
    }
    pub fn mano(mut self, seat: Seat, carte: &[&str]) -> Mk {
        self.mani[usize::from(seat)] = cc(carte);
        self
    }
    pub fn tavola(mut self, carte: &[&str]) -> Mk {
        self.tavola = cc(carte);
        self
    }
    pub fn turno(mut self, t: u8) -> Mk {
        self.turno = t;
        self
    }
    pub fn mazziere(mut self, s: Seat) -> Mk {
        self.mazziere = s;
        self
    }
    pub fn corrente(mut self, s: Seat) -> Mk {
        self.corrente = Some(s);
        self
    }
    pub fn prese(mut self, t: usize, carte: &[&str]) -> Mk {
        self.prese[t] = cc(carte);
        self
    }
    pub fn credito(mut self, c: [u32; 2]) -> Mk {
        self.credito = c;
        self
    }
    pub fn mazzo_vuoto(mut self) -> Mk {
        self.mazzo = Some(Vec::new());
        self
    }
    pub fn tutti_giocato(mut self) -> Mk {
        self.giocato = Some([true; 4]);
        self
    }
    pub fn piccola_target(mut self, s: &str) -> Mk {
        self.piccola_target = Some(c(s));
        self
    }
    pub fn fase(mut self, f: Fase) -> Mk {
        self.fase = f;
        self
    }
    /// Last card of the hand: turn 3, seat 3 (the dealer) to play, empty deck.
    pub fn ultima(self) -> Mk {
        self.turno(3).corrente(3).tutti_giocato().mazzo_vuoto()
    }

    pub fn build(self) -> GameState {
        let mut st = GameState::vuoto(self.rules, 0, Mulberry32::new(0));
        let usate: Vec<Card> = self
            .mani
            .iter()
            .flatten()
            .chain(&self.tavola)
            .chain(self.prese.iter().flatten())
            .copied()
            .collect();
        st.mazzo = self.mazzo.unwrap_or_else(|| {
            MAZZO
                .iter()
                .copied()
                .filter(|c| !usate.contains(c))
                .collect()
        });
        st.fase = self.fase;
        st.mazziere = self.mazziere;
        st.primo_mazziere = self.mazziere;
        st.turno = self.turno;
        st.mano_n = 1;
        st.tavola = self.tavola;
        st.distribuite = self.mani.clone();
        st.mani = self.mani;
        st.prese = self
            .prese
            .map(|p| p.into_iter().map(Entrata::coperta).collect());
        st.credito = self.credito;
        st.corrente = self.corrente.unwrap_or((self.mazziere + 1) % 4);
        st.piccola_target = self.piccola_target;
        if let Some(g) = self.giocato {
            st.giocato_turno = g;
        }
        st
    }
}

pub fn giocata(ev: &[Evento]) -> &GiocataEvento {
    ev.iter()
        .find_map(|e| match &e.dati {
            DatiEvento::Giocata(g) => Some(g),
            _ => None,
        })
        .expect("no giocata event")
}

pub fn fine_mano(ev: &[Evento]) -> &Riepilogo {
    ev.iter()
        .find_map(|e| match &e.dati {
            DatiEvento::FineMano { riepilogo } => Some(&**riepilogo),
            _ => None,
        })
        .expect("no fine_mano event")
}

/// Sorted captures available for `carta` (each capture sorted too).
pub fn presa_opts(st: &GameState, seat: Seat, carta: &str) -> Vec<Vec<Card>> {
    let mut out: Vec<Vec<Card>> = st
        .legal_actions(seat)
        .into_iter()
        .filter_map(|a| match a {
            Azione::Gioca {
                carta: x, presa, ..
            } if x == c(carta) => Some(sorted(presa)),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

/// Sorted origins of the face-up cards in a capture pile.
pub fn origini_su(prese: &[Entrata]) -> Vec<String> {
    let mut v: Vec<String> = prese
        .iter()
        .filter(|e| e.su())
        .map(|e| {
            serde_json::to_value(e.2)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    v.sort();
    v
}

pub fn n_su(prese: &[Entrata]) -> usize {
    prese.iter().filter(|e| e.su()).count()
}
