//! Deterministic Bazzica state machine (port of `bazzica/engine/game.py`).
//!
//! Seats: 0 = Nord, 1 = Est, 2 = Sud, 3 = Ovest. Team = seat % 2. Play is clockwise:
//! after seat `s` comes `(s + 1) % 4`.
//!
//! Main API: [`GameState::new_game`], [`GameState::legal_actions`], [`GameState::apply`],
//! [`GameState::observation`], [`GameState::anteprima`].

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::bazzica::{OpzioneBazzica, TipoBazzica, opzioni_bazzica, puo_dire_che_palo};
use crate::cards::{Card, MAZZO, SETTE_BELLO, nomi};
use crate::prng::Mulberry32;
use crate::rules::RulesConfig;
use crate::scoring::{Entrata, Origine, Riepilogo, riepilogo_mano};

pub const NOMI_POSTI: [&str; 4] = ["Nord", "Est", "Sud", "Ovest"];
/// Default team names; a table may rename them.
pub const NOMI_SQUADRE: [&str; 2] = ["Squadra 1", "Squadra 2"];

pub type Seat = u8;

pub fn squadra(seat: Seat) -> u8 {
    seat % 2
}

pub fn compagno(seat: Seat) -> Seat {
    (seat + 2) % 4
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fase {
    Scambio,
    Gioco,
    FineMano,
    FinePartita,
}

/// Game result: a winning team (serialized as `0`/`1`) or a draw (`"pari"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vincitore {
    Squadra(u8),
    Pari,
}

impl Serialize for Vincitore {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Vincitore::Squadra(t) => s.serialize_u8(*t),
            Vincitore::Pari => s.serialize_str("pari"),
        }
    }
}

impl<'de> Deserialize<'de> for Vincitore {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match serde_json::Value::deserialize(d)? {
            serde_json::Value::String(s) if s == "pari" => Ok(Vincitore::Pari),
            serde_json::Value::Number(n) => match n.as_u64() {
                Some(t @ (0 | 1)) => Ok(Vincitore::Squadra(t as u8)),
                _ => Err(de::Error::custom("invalid winner")),
            },
            _ => Err(de::Error::custom("invalid winner")),
        }
    }
}

/// A Bazzica called this turn by a player.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BazzicaChiamata {
    pub tipo: TipoBazzica,
    pub scope: u32,
    pub sette: Option<u8>,
    pub carte: Vec<Card>,
}

/// One play of the current hand: `[seat, card, captured cards]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GiocataMano(pub Seat, pub Card, pub Vec<Card>);

// --------------------------------------------------------------------------- actions

/// A player action (or the system action `NuovaMano`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Azione {
    /// Play a card, capturing `presa` (empty: the card stays on the table).
    Gioca {
        seat: Seat,
        carta: Card,
        presa: Vec<Card>,
    },
    /// Call a Bazzica; `sette` is the value assigned to the 7 bello.
    Bazzica {
        seat: Seat,
        sette: Option<u8>,
    },
    ChePalo {
        seat: Seat,
    },
    /// Pass a card to the partner (Heisenberg exchange).
    Passa {
        seat: Seat,
        carta: Card,
        veloce: bool,
    },
    /// Decline an optional action for now.
    Attendi {
        seat: Seat,
    },
    NuovaMano,
}

impl Azione {
    pub fn tipo(&self) -> &'static str {
        match self {
            Azione::Gioca { .. } => "gioca",
            Azione::Bazzica { .. } => "bazzica",
            Azione::ChePalo { .. } => "che_palo",
            Azione::Passa { .. } => "passa",
            Azione::Attendi { .. } => "attendi",
            Azione::NuovaMano => "nuova_mano",
        }
    }

    pub fn seat(&self) -> Option<Seat> {
        match *self {
            Azione::Gioca { seat, .. }
            | Azione::Bazzica { seat, .. }
            | Azione::ChePalo { seat }
            | Azione::Passa { seat, .. }
            | Azione::Attendi { seat } => Some(seat),
            Azione::NuovaMano => None,
        }
    }

    /// Same action regardless of the order of the captured cards.
    pub fn equivalente(&self, other: &Azione) -> bool {
        match (self, other) {
            (
                Azione::Gioca {
                    seat: s1,
                    carta: c1,
                    presa: p1,
                },
                Azione::Gioca {
                    seat: s2,
                    carta: c2,
                    presa: p2,
                },
            ) => {
                let (mut a, mut b) = (p1.clone(), p2.clone());
                a.sort();
                b.sort();
                s1 == s2 && c1 == c2 && a == b
            }
            _ => self == other,
        }
    }

    pub fn descrivi(&self) -> String {
        match self {
            Azione::Gioca { carta, presa, .. } if presa.is_empty() => {
                format!("Gioca {} (resta in tavola)", carta.nome())
            }
            Azione::Gioca { carta, presa, .. } => {
                format!("Gioca {} prendendo {}", carta.nome(), nomi(presa))
            }
            Azione::Bazzica { sette: Some(v), .. } => {
                format!("Chiama Bazzica con il 7 bello che vale {v}")
            }
            Azione::Bazzica { sette: None, .. } => "Chiama Bazzica".to_owned(),
            Azione::ChePalo { .. } => "Esclama \"Che Palo!\"".to_owned(),
            Azione::Passa { carta, veloce, .. } => {
                format!(
                    "Passa {} al compagno{}",
                    carta.nome(),
                    if *veloce { " (veloce)" } else { "" }
                )
            }
            Azione::Attendi { .. } => "Attendi (non fare nulla per ora)".to_owned(),
            Azione::NuovaMano => "Inizia la Mano successiva".to_owned(),
        }
    }
}

/// Wire form shared with the Python engine: every field is always present.
#[derive(Serialize, Deserialize)]
struct AzioneDto {
    tipo: String,
    #[serde(default)]
    seat: Option<Seat>,
    #[serde(default)]
    carta: Option<Card>,
    #[serde(default)]
    presa: Option<Vec<Card>>,
    #[serde(default)]
    sette: Option<u8>,
    #[serde(default)]
    veloce: Option<bool>,
}

impl Serialize for Azione {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (carta, presa, sette, veloce) = match self {
            Azione::Gioca { carta, presa, .. } => (Some(*carta), presa.clone(), None, false),
            Azione::Bazzica { sette, .. } => (None, Vec::new(), *sette, false),
            Azione::Passa { carta, veloce, .. } => (Some(*carta), Vec::new(), None, *veloce),
            _ => (None, Vec::new(), None, false),
        };
        AzioneDto {
            tipo: self.tipo().to_owned(),
            seat: self.seat(),
            carta,
            presa: Some(presa),
            sette,
            veloce: Some(veloce),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for Azione {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let dto = AzioneDto::deserialize(d)?;
        let seat = || dto.seat.ok_or_else(|| de::Error::missing_field("seat"));
        let carta = || dto.carta.ok_or_else(|| de::Error::missing_field("carta"));
        Ok(match dto.tipo.as_str() {
            "gioca" => Azione::Gioca {
                seat: seat()?,
                carta: carta()?,
                presa: dto.presa.clone().unwrap_or_default(),
            },
            "bazzica" => Azione::Bazzica {
                seat: seat()?,
                sette: dto.sette,
            },
            "che_palo" => Azione::ChePalo { seat: seat()? },
            "passa" => Azione::Passa {
                seat: seat()?,
                carta: carta()?,
                veloce: dto.veloce.unwrap_or(false),
            },
            "attendi" => Azione::Attendi { seat: seat()? },
            "nuova_mano" => Azione::NuovaMano,
            other => return Err(de::Error::custom(format!("unknown action type {other:?}"))),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AzioneNonValida(pub String);

impl fmt::Display for AzioneNonValida {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AzioneNonValida {}

// --------------------------------------------------------------------------- events

/// Outcome flags of a play (also the core of [`Anteprima`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EsitoGiocata {
    pub scopa: bool,
    pub piccola: bool,
    pub grande: bool,
    /// Aces turned face up by the Avogadro rule.
    pub assi: Vec<Card>,
    pub credito_pagato: u32,
    pub sparecchio: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GiocataEvento {
    pub seat: Seat,
    pub carta: Card,
    pub valore: u8,
    pub presa: Vec<Card>,
    pub squadra: u8,
    #[serde(flatten)]
    pub esito: EsitoGiocata,
    pub girate: Vec<(Card, Origine)>,
    pub ultima_giocata: bool,
    /// Team credit after the play; only present when something was captured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credito: Option<u32>,
    pub tavola: Vec<Card>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum DatiEvento {
    InizioPartita {
        variante: String,
        mazziere: Seat,
    },
    NuovaMano {
        mano: u32,
        mazziere: Seat,
    },
    Distribuzione {
        mano: u32,
        turno: u8,
        mazziere: Seat,
        tavola: Vec<Card>,
        carte_per_giocatore: u8,
    },
    CarteRicevute {
        seat: Seat,
        carte: Vec<Card>,
    },
    InizioScambio {
        turno: u8,
    },
    InizioGiocate {
        turno: u8,
        corrente: Seat,
    },
    ScambioCarta {
        seat: Seat,
        a: Seat,
        carta: Card,
        veloce: bool,
    },
    Scambio {
        seat: Seat,
        a: Seat,
        veloce: bool,
    },
    ScambioRicevuta {
        seat: Seat,
        da: Seat,
        carta: Card,
    },
    ScambioCompletato {
        squadra: u8,
    },
    FineScambio {
        turno: u8,
    },
    ChePalo {
        seat: Seat,
    },
    Bazzica {
        seat: Seat,
        carte: Vec<Card>,
        tipo_bazzica: TipoBazzica,
        scope: u32,
        sette: Option<u8>,
        girate: u32,
        credito: u32,
        squadra: u8,
        descrizione: String,
    },
    Giocata(GiocataEvento),
    FineTurno {
        turno: u8,
    },
    CarteResidue {
        squadra: u8,
        carte: Vec<Card>,
        credito_pagato: u32,
    },
    FineMano {
        riepilogo: Box<Riepilogo>,
    },
    FinePartita {
        vincitore: Vincitore,
        punteggi: [i32; 2],
        mani: u32,
    },
}

/// A game event. `visibile`: `None` = everyone, otherwise the seats allowed to see it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evento {
    #[serde(flatten)]
    pub dati: DatiEvento,
    pub visibile: Option<Vec<Seat>>,
}

impl Evento {
    pub fn visibile_a(&self, seat: Seat) -> bool {
        self.visibile.as_ref().is_none_or(|v| v.contains(&seat))
    }
}

fn ev(dati: DatiEvento) -> Evento {
    Evento {
        dati,
        visibile: None,
    }
}

fn ev_privato(seat: Seat, dati: DatiEvento) -> Evento {
    Evento {
        dati,
        visibile: Some(vec![seat]),
    }
}

// --------------------------------------------------------------------------- previews

/// What a play would produce (for the UI and the bots).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anteprima {
    #[serde(flatten)]
    pub esito: EsitoGiocata,
    pub presa: Vec<Card>,
    pub valore: u8,
    pub ultima_giocata: bool,
    /// How many cards the play would turn face up.
    pub scoperte: u32,
}

// --------------------------------------------------------------------------- observation

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AltroGiocatore {
    pub n: u32,
    /// Visible only after the player called a Bazzica.
    pub carte: Option<Vec<Card>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreseVisibili {
    pub n: u32,
    pub scoperte: Vec<(Card, Option<Origine>)>,
    pub coperte: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScambioVisibile {
    pub passati: [bool; 4],
    pub veloci: [bool; 4],
    pub mia_passata: Option<Card>,
    pub ricevuta: Option<Card>,
    pub distribuite: Vec<Card>,
}

/// Everything a seat is allowed to know (what a human sees, and the bots' input).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Osservazione {
    pub seat: Seat,
    pub squadra: u8,
    pub compagno: Seat,
    pub fase: Fase,
    pub regole: RulesConfig,
    pub mano_n: u32,
    pub turno: u8,
    pub mazziere: Seat,
    pub corrente: Seat,
    pub punteggi: [i32; 2],
    pub mano: Vec<Card>,
    pub altri: BTreeMap<Seat, AltroGiocatore>,
    pub tavola: Vec<Card>,
    pub prese: [PreseVisibili; 2],
    pub credito: [u32; 2],
    pub bazziche: BTreeMap<Seat, BazzicaChiamata>,
    pub che_palo: Vec<Seat>,
    pub giocato_turno: [bool; 4],
    pub scambio: ScambioVisibile,
    pub piccola_target: Option<Card>,
    pub ultima_presa_squadra: Option<u8>,
    pub giocate_mano: Vec<GiocataMano>,
    pub carte_iniziali_tavola: Vec<Card>,
    pub carte_nel_mazzo: u32,
    pub storico_mani: Vec<Riepilogo>,
    pub vincitore: Option<Vincitore>,
}

// --------------------------------------------------------------------------- state

/// Full game state. Serializes to the same JSON as `stato_json` in `tools/esporta_tracce.py`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameState {
    pub regole: RulesConfig,
    pub seed: u64,
    pub rng: Mulberry32,
    pub fase: Fase,
    pub punteggi: [i32; 2],
    pub mano_n: u32,
    pub mazziere: Seat,
    pub primo_mazziere: Seat,
    pub turno: u8,
    pub mazzo: Vec<Card>,
    pub tavola: Vec<Card>,
    pub mani: [Vec<Card>; 4],
    pub distribuite: [Vec<Card>; 4],
    pub prese: [Vec<Entrata>; 2],
    pub credito: [u32; 2],
    pub corrente: Seat,
    pub giocato_turno: [bool; 4],
    pub bazzica: [Option<BazzicaChiamata>; 4],
    pub che_palo: [bool; 4],
    pub palo_considerato: [bool; 4],
    pub scambio_passata: [Option<Card>; 4],
    pub scambio_ricevuta: [Option<Card>; 4],
    pub scambio_veloce: [bool; 4],
    pub scambio_attesa: [bool; 4],
    pub piccola_target: Option<Card>,
    pub ultima_presa_squadra: Option<u8>,
    pub giocate_mano: Vec<GiocataMano>,
    pub storico_mani: Vec<Riepilogo>,
    pub vincitore: Option<Vincitore>,
    pub carte_iniziali_tavola: Vec<Card>,
}

fn rimuovi(v: &mut Vec<Card>, c: Card) {
    let i = v.iter().position(|x| *x == c).expect("card not present");
    v.remove(i);
}

impl GameState {
    /// A blank state with no cards dealt (for tools and tests that set up positions).
    pub fn vuoto(regole: RulesConfig, seed: u64, rng: Mulberry32) -> GameState {
        GameState {
            regole,
            seed,
            rng,
            fase: Fase::Gioco,
            punteggi: [0, 0],
            mano_n: 0,
            mazziere: 0,
            primo_mazziere: 0,
            turno: 0,
            mazzo: Vec::new(),
            tavola: Vec::new(),
            mani: Default::default(),
            distribuite: Default::default(),
            prese: Default::default(),
            credito: [0, 0],
            corrente: 0,
            giocato_turno: [false; 4],
            bazzica: Default::default(),
            che_palo: [false; 4],
            palo_considerato: [false; 4],
            scambio_passata: [None; 4],
            scambio_ricevuta: [None; 4],
            scambio_veloce: [false; 4],
            scambio_attesa: [false; 4],
            piccola_target: None,
            ultima_presa_squadra: None,
            giocate_mano: Vec::new(),
            storico_mani: Vec::new(),
            vincitore: None,
            carte_iniziali_tavola: Vec::new(),
        }
    }

    /// Starts a game. Without an explicit `mazziere` the first dealer is drawn from the seed.
    pub fn new_game(
        regole: RulesConfig,
        seed: u64,
        mazziere: Option<Seat>,
    ) -> (GameState, Vec<Evento>) {
        let mut rng = Mulberry32::new(seed);
        let mazziere = mazziere.unwrap_or_else(|| rng.randbelow(4) as Seat);
        let mut st = GameState::vuoto(regole, seed, rng);
        st.mazziere = mazziere;
        st.primo_mazziere = mazziere;
        let mut events = vec![ev(DatiEvento::InizioPartita {
            variante: st.regole.descrizione(),
            mazziere,
        })];
        events.extend(st.inizia_mano());
        (st, events)
    }

    fn inizia_mano(&mut self) -> Vec<Evento> {
        self.mano_n += 1;
        self.mazzo = MAZZO.clone();
        self.rng.shuffle(&mut self.mazzo);
        self.mani = Default::default();
        self.prese = Default::default();
        self.credito = [0, 0];
        self.turno = 0;
        self.ultima_presa_squadra = None;
        self.giocate_mano = Vec::new();
        self.fase = Fase::Gioco;
        let mut events = vec![ev(DatiEvento::NuovaMano {
            mano: self.mano_n,
            mazziere: self.mazziere,
        })];
        self.tavola = (0..4).map(|_| self.mazzo.pop().unwrap()).collect();
        self.carte_iniziali_tavola = self.tavola.clone();
        events.extend(self.inizia_turno());
        events
    }

    fn inizia_turno(&mut self) -> Vec<Evento> {
        self.turno += 1;
        self.giocato_turno = [false; 4];
        self.bazzica = Default::default();
        self.che_palo = [false; 4];
        self.palo_considerato = [false; 4];
        self.scambio_passata = [None; 4];
        self.scambio_ricevuta = [None; 4];
        self.scambio_veloce = [false; 4];
        self.scambio_attesa = [false; 4];
        self.piccola_target = None;
        let primo = (self.mazziere + 1) % 4;
        let mut events = vec![ev(DatiEvento::Distribuzione {
            mano: self.mano_n,
            turno: self.turno,
            mazziere: self.mazziere,
            tavola: self.tavola.clone(),
            carte_per_giocatore: 3,
        })];
        for i in 0..4 {
            let s = (primo + i) % 4;
            let carte: Vec<Card> = (0..3).map(|_| self.mazzo.pop().unwrap()).collect();
            self.mani[usize::from(s)] = carte.clone();
            self.distribuite[usize::from(s)] = carte.clone();
            events.push(ev_privato(s, DatiEvento::CarteRicevute { seat: s, carte }));
        }
        self.corrente = primo;
        if self.regole.scambio {
            self.fase = Fase::Scambio;
            events.push(ev(DatiEvento::InizioScambio { turno: self.turno }));
        } else {
            self.fase = Fase::Gioco;
            events.push(ev(DatiEvento::InizioGiocate {
                turno: self.turno,
                corrente: self.corrente,
            }));
        }
        events
    }

    // ----------------------------------------------------------------------- captures

    /// Value of a card for capturing (the 7 bello takes the value assigned in a Bazzica).
    pub fn valore_effettivo(&self, seat: Seat, carta: Card) -> u8 {
        match &self.bazzica[usize::from(seat)] {
            Some(BazzicaChiamata { sette: Some(v), .. }) if carta == SETTE_BELLO => *v,
            _ => carta.val(),
        }
    }

    /// The ace sweeps the table, except (Pascal) for a player who called Bazzica this turn.
    /// A 7 bello assigned the value 1 never sweeps.
    pub fn asso_piglia_tutto(&self, seat: Seat, carta: Card) -> bool {
        carta.is_asso() && !(self.regole.pascal && self.bazzica[usize::from(seat)].is_some())
    }

    /// Possible captures for `carta`; `[[]]` if the card stays on the table.
    pub fn opzioni_presa(&self, seat: Seat, carta: Card) -> Vec<Vec<Card>> {
        let tav = &self.tavola;
        if tav.is_empty() {
            return vec![Vec::new()];
        }
        if self.asso_piglia_tutto(seat, carta) {
            let assi: Vec<Vec<Card>> = tav
                .iter()
                .filter(|c| c.is_asso())
                .map(|c| vec![*c])
                .collect();
            return if assi.is_empty() {
                vec![tav.clone()]
            } else {
                assi
            };
        }
        let v = self.valore_effettivo(seat, carta);
        let singole: Vec<Vec<Card>> = tav
            .iter()
            .filter(|c| c.val() == v)
            .map(|c| vec![*c])
            .collect();
        if !singole.is_empty() {
            return singole;
        }
        let combo = combinazioni(tav, v);
        if combo.is_empty() {
            vec![Vec::new()]
        } else {
            combo
        }
    }

    // ----------------------------------------------------------------------- legal actions

    /// True if "Che Palo!" is *meaningful* (hand one card away from a Bazzica, before the
    /// player's first card, not yet exclaimed). Used to prompt the bots; humans may always
    /// exclaim.
    pub fn palo_sensato(&self, seat: Seat) -> bool {
        let s = usize::from(seat);
        self.fase == Fase::Gioco
            && !self.giocato_turno[s]
            && !self.che_palo[s]
            && self.bazzica[s].is_none()
            && puo_dire_che_palo(&self.mani[s], &self.regole)
    }

    /// Legal actions for `seat`. "Che Palo!" is always available during exchange and play.
    pub fn legal_actions(&self, seat: Seat) -> Vec<Azione> {
        let s = usize::from(seat);
        match self.fase {
            Fase::Scambio => {
                let mut acts = vec![Azione::ChePalo { seat }];
                if self.scambio_passata[s].is_some() {
                    return acts;
                }
                for &c in &self.distribuite[s] {
                    if self.mani[s].contains(&c) && Some(c) != self.scambio_ricevuta[s] {
                        acts.push(Azione::Passa {
                            seat,
                            carta: c,
                            veloce: false,
                        });
                        acts.push(Azione::Passa {
                            seat,
                            carta: c,
                            veloce: true,
                        });
                    }
                }
                let p = usize::from(compagno(seat));
                if self.scambio_passata[p].is_none()
                    && !self.scambio_attesa[p]
                    && !self.scambio_attesa[s]
                {
                    acts.push(Azione::Attendi { seat });
                }
                acts
            }
            Fase::Gioco => {
                let mut acts = vec![Azione::ChePalo { seat }];
                if seat == self.corrente {
                    if !self.giocato_turno[s] && self.bazzica[s].is_none() {
                        for op in opzioni_bazzica(&self.mani[s], &self.regole) {
                            acts.push(Azione::Bazzica {
                                seat,
                                sette: op.sette,
                            });
                        }
                    }
                    for &c in &self.mani[s] {
                        for presa in self.opzioni_presa(seat, c) {
                            acts.push(Azione::Gioca {
                                seat,
                                carta: c,
                                presa,
                            });
                        }
                    }
                } else if !self.palo_considerato[s] && self.palo_sensato(seat) {
                    acts.push(Azione::Attendi { seat });
                }
                acts
            }
            Fase::FineMano | Fase::FinePartita => Vec::new(),
        }
    }

    /// Seats that must act for the game to progress.
    pub fn required_actors(&self) -> Vec<Seat> {
        match self.fase {
            Fase::Scambio => (0..4)
                .filter(|&s| self.scambio_passata[usize::from(s)].is_none())
                .collect(),
            Fase::Gioco => vec![self.corrente],
            _ => Vec::new(),
        }
    }

    /// Off-turn seats to offer "Che Palo!" to (once) when it is meaningful (for the bots).
    pub fn optional_actors(&self) -> Vec<Seat> {
        if self.fase != Fase::Gioco {
            return Vec::new();
        }
        (0..4)
            .filter(|&s| {
                s != self.corrente && !self.palo_considerato[usize::from(s)] && self.palo_sensato(s)
            })
            .collect()
    }

    pub fn system_action(&self) -> Option<Azione> {
        (self.fase == Fase::FineMano).then_some(Azione::NuovaMano)
    }

    // ----------------------------------------------------------------------- apply

    /// Validates and applies an action, returning the events it produced.
    pub fn apply(&mut self, a: &Azione) -> Result<Vec<Evento>, AzioneNonValida> {
        let Some(seat) = a.seat() else {
            if self.fase != Fase::FineMano {
                return Err(AzioneNonValida("La Mano non è terminata".into()));
            }
            self.mazziere = (self.mazziere + 1) % 4;
            return Ok(self.inizia_mano());
        };
        if seat > 3 || !self.legal_actions(seat).iter().any(|x| x.equivalente(a)) {
            return Err(AzioneNonValida(format!(
                "Azione non valida: {}",
                a.descrivi()
            )));
        }
        let s = usize::from(seat);
        Ok(match a {
            Azione::Passa { carta, veloce, .. } => self.passa(seat, *carta, *veloce),
            Azione::Attendi { .. } => {
                if self.fase == Fase::Scambio {
                    self.scambio_attesa[s] = true;
                } else {
                    self.palo_considerato[s] = true;
                }
                Vec::new()
            }
            Azione::ChePalo { .. } => {
                self.che_palo[s] = true;
                self.palo_considerato[s] = true;
                vec![ev(DatiEvento::ChePalo { seat })]
            }
            Azione::Bazzica { sette, .. } => self.chiama_bazzica(seat, *sette),
            Azione::Gioca { carta, presa, .. } => self.gioca(seat, *carta, presa),
            Azione::NuovaMano => unreachable!(),
        })
    }

    /// The exchange is secret: the passed card stays "in transit" and reaches the partner
    /// only once the partner has passed too.
    fn passa(&mut self, seat: Seat, carta: Card, veloce: bool) -> Vec<Evento> {
        let (s, p) = (seat, compagno(seat));
        rimuovi(&mut self.mani[usize::from(s)], carta);
        self.scambio_passata[usize::from(s)] = Some(carta);
        self.scambio_veloce[usize::from(s)] = veloce;
        self.scambio_attesa[usize::from(s)] = false;
        let mut events = vec![
            ev_privato(
                s,
                DatiEvento::ScambioCarta {
                    seat: s,
                    a: p,
                    carta,
                    veloce,
                },
            ),
            ev(DatiEvento::Scambio {
                seat: s,
                a: p,
                veloce,
            }),
        ];
        if self.scambio_passata[usize::from(p)].is_some() {
            for x in [s, p] {
                let c = self.scambio_passata[usize::from(compagno(x))].unwrap();
                self.mani[usize::from(x)].push(c);
                self.scambio_ricevuta[usize::from(x)] = Some(c);
                events.push(ev_privato(
                    x,
                    DatiEvento::ScambioRicevuta {
                        seat: x,
                        da: compagno(x),
                        carta: c,
                    },
                ));
            }
            events.push(ev(DatiEvento::ScambioCompletato {
                squadra: squadra(s),
            }));
        }
        if self.scambio_passata.iter().all(Option::is_some) {
            self.fase = Fase::Gioco;
            self.corrente = (self.mazziere + 1) % 4;
            events.push(ev(DatiEvento::FineScambio { turno: self.turno }));
            events.push(ev(DatiEvento::InizioGiocate {
                turno: self.turno,
                corrente: self.corrente,
            }));
        }
        events
    }

    /// Turns up to `n` face-down cards of the team's pile (most recent first).
    fn gira(&mut self, t: u8, n: u32, origine: Origine) -> u32 {
        let mut girate = 0;
        for e in self.prese[usize::from(t)].iter_mut().rev() {
            if girate >= n {
                break;
            }
            if !e.su() {
                e.gira(origine);
                girate += 1;
            }
        }
        girate
    }

    fn chiama_bazzica(&mut self, seat: Seat, sette: Option<u8>) -> Vec<Evento> {
        let s = usize::from(seat);
        let op: OpzioneBazzica = opzioni_bazzica(&self.mani[s], &self.regole)
            .into_iter()
            .find(|o| o.sette == sette)
            .expect("validated Bazzica option");
        let t = squadra(seat);
        self.bazzica[s] = Some(BazzicaChiamata {
            tipo: op.tipo,
            scope: op.scope,
            sette: op.sette,
            carte: self.mani[s].clone(),
        });
        let girate = self.gira(t, op.scope, Origine::Bazzica);
        self.credito[usize::from(t)] += op.scope - girate;
        vec![ev(DatiEvento::Bazzica {
            seat,
            carte: self.mani[s].clone(),
            tipo_bazzica: op.tipo,
            scope: op.scope,
            sette: op.sette,
            girate,
            credito: self.credito[usize::from(t)],
            squadra: t,
            descrizione: op.descrizione(),
        })]
    }

    /// Computes (without changing the state) the outcome of a capture: the cards entering
    /// the capture pile, the flags, and the team's remaining Bazzica credit.
    pub fn risolvi_giocata(
        &self,
        s: Seat,
        c: Card,
        presa: &[Card],
        ultima: bool,
    ) -> (Vec<Entrata>, EsitoGiocata, u32) {
        let t = usize::from(squadra(s));
        let mut flags = EsitoGiocata::default();
        if presa.is_empty() {
            return (Vec::new(), flags, self.credito[t]);
        }
        let v_eff = self.valore_effettivo(s, c);
        let sparecchio = self.tavola.len() == presa.len();
        flags.sparecchio = sparecchio;
        let mut entrate: Vec<Entrata> = std::iter::once(c)
            .chain(presa.iter().copied())
            .map(Entrata::coperta)
            .collect();
        if !ultima {
            let r = &self.regole;
            let primo_seme = c.seme();
            let stesso_seme = entrate.iter().all(|e| e.carta().seme() == primo_seme);
            let asso_bazzica = c.is_asso() && self.bazzica[usize::from(s)].is_some();
            let grande = r.curie && stesso_seme && !asso_bazzica;
            let piccola = r.curie
                && !grande
                && self
                    .piccola_target
                    .is_some_and(|pt| presa == [pt] && pt.val() == v_eff);
            if grande {
                entrate.iter_mut().for_each(|e| e.gira(Origine::Grande));
                flags.grande = true;
            } else if piccola {
                entrate[0].gira(Origine::Piccola);
                flags.piccola = true;
            } else if sparecchio {
                entrate[0].gira(Origine::Sparecchio);
                flags.scopa = true;
            }
            if r.avogadro {
                for e in entrate.iter_mut() {
                    if e.carta().is_asso() && !e.su() {
                        e.gira(Origine::Asso);
                        flags.assi.push(e.carta());
                    }
                }
            }
        }
        let mut credito = self.credito[t];
        for e in entrate.iter_mut() {
            if credito > 0 && !e.su() {
                e.gira(Origine::Bazzica);
                credito -= 1;
                flags.credito_pagato += 1;
            }
        }
        (entrate, flags, credito)
    }

    fn gioca(&mut self, s: Seat, c: Card, presa: &[Card]) -> Vec<Evento> {
        let t = squadra(s);
        let v_eff = self.valore_effettivo(s, c);
        rimuovi(&mut self.mani[usize::from(s)], c);
        self.giocato_turno[usize::from(s)] = true;
        let ultima = self.turno == 3 && self.mani.iter().all(Vec::is_empty);
        let mut info = GiocataEvento {
            seat: s,
            carta: c,
            valore: v_eff,
            presa: presa.to_vec(),
            squadra: t,
            esito: EsitoGiocata::default(),
            girate: Vec::new(),
            ultima_giocata: ultima,
            credito: None,
            tavola: Vec::new(),
        };
        if presa.is_empty() {
            self.tavola.push(c);
            self.piccola_target = Some(c);
        } else {
            let (entrate, flags, credito) = self.risolvi_giocata(s, c, presa, ultima);
            info.esito = flags;
            for &x in presa {
                rimuovi(&mut self.tavola, x);
            }
            self.credito[usize::from(t)] = credito;
            info.girate = entrate
                .iter()
                .filter(|e| e.su())
                .map(|e| (e.0, e.2.unwrap()))
                .collect();
            info.credito = Some(credito);
            self.prese[usize::from(t)].extend(entrate);
            self.ultima_presa_squadra = Some(t);
            self.piccola_target = None;
        }
        self.giocate_mano.push(GiocataMano(s, c, presa.to_vec()));
        info.tavola = self.tavola.clone();
        let mut events = vec![ev(DatiEvento::Giocata(info))];

        if self.mani.iter().all(Vec::is_empty) {
            events.push(ev(DatiEvento::FineTurno { turno: self.turno }));
            if self.turno < 3 {
                events.extend(self.inizia_turno());
            } else {
                events.extend(self.fine_mano());
            }
        } else {
            self.corrente = (s + 1) % 4;
        }
        events
    }

    fn fine_mano(&mut self) -> Vec<Evento> {
        let mut events = Vec::new();
        if !self.tavola.is_empty() {
            let t = self.ultima_presa_squadra.unwrap_or(squadra(self.mazziere));
            let ti = usize::from(t);
            let mut pagato = 0;
            let entrate: Vec<Entrata> = self
                .tavola
                .iter()
                .map(|&x| {
                    let mut e = Entrata::coperta(x);
                    if self.credito[ti] > 0 {
                        e.gira(Origine::Bazzica);
                        self.credito[ti] -= 1;
                        pagato += 1;
                    }
                    e
                })
                .collect();
            self.prese[ti].extend(entrate);
            events.push(ev(DatiEvento::CarteResidue {
                squadra: t,
                carte: std::mem::take(&mut self.tavola),
                credito_pagato: pagato,
            }));
        }
        let mut rie = riepilogo_mano(&self.prese, &self.regole);
        rie.mano = self.mano_n;
        rie.mazziere = self.mazziere;
        rie.credito_perso = self.credito;
        self.credito = [0, 0];
        for t in 0..2 {
            self.punteggi[t] += rie.totali[t];
        }
        rie.punteggi = self.punteggi;
        self.storico_mani.push(rie.clone());
        events.push(ev(DatiEvento::FineMano {
            riepilogo: Box::new(rie),
        }));
        match self.controlla_fine() {
            Some(v) => {
                self.vincitore = Some(v);
                self.fase = Fase::FinePartita;
                events.push(ev(DatiEvento::FinePartita {
                    vincitore: v,
                    punteggi: self.punteggi,
                    mani: self.mano_n,
                }));
            }
            None => self.fase = Fase::FineMano,
        }
        events
    }

    /// Winner if the game is over given the current scores and hand count.
    pub fn controlla_fine(&self) -> Option<Vincitore> {
        let [a, b] = self.punteggi;
        if self.regole.margherita_hack {
            if self.mano_n < 4 {
                return None;
            }
            return Some(match a.cmp(&b) {
                std::cmp::Ordering::Greater => Vincitore::Squadra(0),
                std::cmp::Ordering::Less => Vincitore::Squadra(1),
                std::cmp::Ordering::Equal => Vincitore::Pari,
            });
        }
        if a == b {
            return None;
        }
        let lead = if a > b { 0 } else { 1 };
        (self.punteggi[lead] >= self.regole.punti_vittoria)
            .then_some(Vincitore::Squadra(lead as u8))
    }

    // ----------------------------------------------------------------------- views

    /// Information visible to `seat`.
    pub fn observation(&self, seat: Seat) -> Osservazione {
        let altri = (0..4)
            .filter(|&s| s != seat)
            .map(|s| {
                let mano = &self.mani[usize::from(s)];
                let visibile = self.bazzica[usize::from(s)].is_some();
                (
                    s,
                    AltroGiocatore {
                        n: mano.len() as u32,
                        carte: visibile.then(|| mano.clone()),
                    },
                )
            })
            .collect();
        let prese = [0, 1].map(|t| {
            let p = &self.prese[t];
            PreseVisibili {
                n: p.len() as u32,
                scoperte: p.iter().filter(|e| e.su()).map(|e| (e.0, e.2)).collect(),
                coperte: p.iter().filter(|e| !e.su()).count() as u32,
            }
        });
        let bazziche = (0..4)
            .filter_map(|s| self.bazzica[usize::from(s)].clone().map(|b| (s, b)))
            .collect();
        let s = usize::from(seat);
        Osservazione {
            seat,
            squadra: squadra(seat),
            compagno: compagno(seat),
            fase: self.fase,
            regole: self.regole.clone(),
            mano_n: self.mano_n,
            turno: self.turno,
            mazziere: self.mazziere,
            corrente: self.corrente,
            punteggi: self.punteggi,
            mano: self.mani[s].clone(),
            altri,
            tavola: self.tavola.clone(),
            prese,
            credito: self.credito,
            bazziche,
            che_palo: (0..4).filter(|&x| self.che_palo[usize::from(x)]).collect(),
            giocato_turno: self.giocato_turno,
            scambio: ScambioVisibile {
                passati: self.scambio_passata.map(|x| x.is_some()),
                veloci: self.scambio_veloce,
                mia_passata: self.scambio_passata[s],
                ricevuta: self.scambio_ricevuta[s],
                distribuite: self.distribuite[s].clone(),
            },
            piccola_target: self.piccola_target,
            ultima_presa_squadra: self.ultima_presa_squadra,
            giocate_mano: self.giocate_mano.clone(),
            carte_iniziali_tavola: self.carte_iniziali_tavola.clone(),
            carte_nel_mazzo: self.mazzo.len() as u32,
            storico_mani: self.storico_mani.clone(),
            vincitore: self.vincitore,
        }
    }

    /// True if `seat`'s next card is the last play of the hand.
    pub fn e_ultima_giocata(&self, seat: Seat) -> bool {
        self.turno == 3
            && self.mani[usize::from(seat)].len() == 1
            && self
                .mani
                .iter()
                .enumerate()
                .all(|(i, m)| i == usize::from(seat) || m.is_empty())
    }

    /// What a play would produce. `None` for actions other than [`Azione::Gioca`].
    pub fn anteprima(&self, a: &Azione) -> Option<Anteprima> {
        let Azione::Gioca { seat, carta, presa } = a else {
            return None;
        };
        let ultima = self.e_ultima_giocata(*seat);
        let (entrate, esito, _) = self.risolvi_giocata(*seat, *carta, presa, ultima);
        Some(Anteprima {
            esito,
            presa: presa.clone(),
            valore: self.valore_effettivo(*seat, *carta),
            ultima_giocata: ultima,
            scoperte: entrate.iter().filter(|e| e.su()).count() as u32,
        })
    }

    /// Rebuilds a *partial* state from a player's point of view. Hidden hands and
    /// face-down captures are filled with [`Card::IGNOTA`]. Used by the bots for previews.
    pub fn da_osservazione(obs: &Osservazione) -> GameState {
        let mut st = GameState::vuoto(obs.regole.clone(), 0, Mulberry32::new(0));
        for s in 0..4u8 {
            st.mani[usize::from(s)] = if s == obs.seat {
                obs.mano.clone()
            } else {
                let o = &obs.altri[&s];
                o.carte
                    .clone()
                    .unwrap_or_else(|| vec![Card::IGNOTA; o.n as usize])
            };
        }
        for (t, p) in obs.prese.iter().enumerate() {
            st.prese[t] = p
                .scoperte
                .iter()
                .map(|&(c, o)| Entrata(c, true, o))
                .chain((0..p.coperte).map(|_| Entrata::coperta(Card::IGNOTA)))
                .collect();
        }
        for (&s, b) in &obs.bazziche {
            st.bazzica[usize::from(s)] = Some(b.clone());
        }
        st.fase = obs.fase;
        st.punteggi = obs.punteggi;
        st.mano_n = obs.mano_n;
        st.mazziere = obs.mazziere;
        st.turno = obs.turno;
        st.tavola = obs.tavola.clone();
        st.credito = obs.credito;
        st.corrente = obs.corrente;
        st.giocato_turno = obs.giocato_turno;
        st.piccola_target = obs.piccola_target;
        st.ultima_presa_squadra = obs.ultima_presa_squadra;
        st
    }
}

/// All subsets of at least 2 table cards summing to `target`, in the reference order
/// (table sorted by value, stable; depth-first).
fn combinazioni(tavola: &[Card], target: u8) -> Vec<Vec<Card>> {
    let mut items = tavola.to_vec();
    items.sort_by_key(|c| c.val());
    let mut out = Vec::new();
    let mut acc = Vec::new();
    fn rec(items: &[Card], i: usize, rem: u8, acc: &mut Vec<Card>, out: &mut Vec<Vec<Card>>) {
        if rem == 0 {
            if acc.len() >= 2 {
                out.push(acc.clone());
            }
            return;
        }
        for j in i..items.len() {
            let v = items[j].val();
            if v > rem {
                break;
            }
            acc.push(items[j]);
            rec(items, j + 1, rem - v, acc, out);
            acc.pop();
        }
    }
    rec(&items, 0, target, &mut acc, &mut out);
    out
}
