//! The tutorial: one chapter per variant, in cumulative order like the variants themselves.
//!
//! Examples are plain data (card codes) so they read like the rules; [`verifica`] replays each
//! one on the engine, and the tests make sure every example tells the truth.

use crate::bazzica::{TipoBazzica, opzioni_bazzica};
use crate::game::{Azione, BazzicaChiamata};
use crate::scoring::napola;
use crate::{Card, GameState, Mulberry32, RulesConfig, Variante};

/// A chapter of the tutorial. `variante` is `None` for Margherita Hack (it applies to any).
#[derive(Debug, PartialEq, Eq)]
pub struct Capitolo {
    pub variante: Option<Variante>,
    pub titolo: &'static str,
    pub intro: &'static str,
    pub passi: &'static [Passo],
}

#[derive(Debug, PartialEq, Eq)]
pub struct Passo {
    pub titolo: &'static str,
    pub testo: &'static str,
    pub esempio: Option<Esempio>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Esempio {
    /// A play: these cards on the table, this card played, these cards captured.
    Presa(Presa),
    /// Three cards in hand and the best Bazzica they make.
    Bazzica {
        mano: &'static [&'static str],
        scope: u32,
    },
    /// Captured denari and the points of the Napola they make.
    Napola {
        carte: &'static [&'static str],
        punti: u32,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct Presa {
    pub tavola: &'static [&'static str],
    pub giocata: &'static str,
    pub presa: &'static [&'static str],
    /// The player has called a Bazzica this turn.
    pub bazzica: bool,
    /// The card the previous player has just laid down (for the Piccola).
    pub appena_giocata: Option<&'static str>,
    pub esito: Esito,
}

/// What the play scores, as the engine reports it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Esito {
    pub scopa: bool,
    pub piccola: bool,
    pub grande: bool,
    pub assi: usize,
}

impl Esito {
    pub const NULLO: Esito = Esito {
        scopa: false,
        piccola: false,
        grande: false,
        assi: 0,
    };
    pub const SCOPA: Esito = Esito {
        scopa: true,
        ..Esito::NULLO
    };
}

impl Presa {
    const BASE: Presa = Presa {
        tavola: &[],
        giocata: "",
        presa: &[],
        bazzica: false,
        appena_giocata: None,
        esito: Esito::NULLO,
    };
}

const fn passo(titolo: &'static str, testo: &'static str) -> Passo {
    Passo {
        titolo,
        testo,
        esempio: None,
    }
}

const fn con(titolo: &'static str, testo: &'static str, esempio: Esempio) -> Passo {
    Passo {
        titolo,
        testo,
        esempio: Some(esempio),
    }
}

pub static CAPITOLI: [Capitolo; 9] = [
    Capitolo {
        variante: Some(Variante::Vanilla),
        titolo: "Le basi",
        intro: "La Bazzica è una Scopa a coppie con un colpo in più: la Bazzica, che fa segnare Scope in base alle carte che hai in mano.",
        passi: &[
            passo(
                "Squadre e mani",
                "Si gioca in quattro, due contro due: il compagno siede di fronte. Ogni mano ha 3 turni; a ogni turno ricevi 3 carte e le giochi una alla volta. Vince la prima squadra che a fine mano arriva a 21 punti ed è in vantaggio.",
            ),
            con(
                "Prendere",
                "Giochi una carta: se in tavola c'è una carta dello stesso valore la prendi. Se ce n'è una sola uguale devi prendere quella.",
                Esempio::Presa(Presa {
                    tavola: &["coppe_03", "spade_04", "bastoni_07"],
                    giocata: "denari_07",
                    presa: &["bastoni_07"],
                    ..Presa::BASE
                }),
            ),
            con(
                "Prendere con una somma",
                "Se non ci sono carte uguali puoi prendere più carte la cui somma fa il valore della tua. Fante 8, cavallo 9, re 10. Quando le combinazioni sono più di una scegli tu.",
                Esempio::Presa(Presa {
                    tavola: &["coppe_03", "spade_05", "denari_06", "bastoni_02"],
                    giocata: "bastoni_08",
                    presa: &["denari_06", "bastoni_02"],
                    ..Presa::BASE
                }),
            ),
            con(
                "Scopa",
                "Se con la presa sparecchi il tavolo fai Scopa: vale un punto. Non vale con l'ultima carta dell'ultimo turno.",
                Esempio::Presa(Presa {
                    tavola: &["coppe_06"],
                    giocata: "spade_06",
                    presa: &["coppe_06"],
                    esito: Esito::SCOPA,
                    ..Presa::BASE
                }),
            ),
            con(
                "L'asso piglia tutto",
                "L'asso sparecchia il tavolo e fa Scopa. Se però in tavola c'è già un asso, prende solo quello.",
                Esempio::Presa(Presa {
                    tavola: &["coppe_04", "spade_09", "denari_02"],
                    giocata: "bastoni_01",
                    presa: &["coppe_04", "spade_09", "denari_02"],
                    esito: Esito::SCOPA,
                    ..Presa::BASE
                }),
            ),
            passo(
                "Il punteggio",
                "A fine mano ogni squadra conta fino a 5 punti di mazzo: più carte, più denari, primiera, 7 bello e re bello. Poi un punto per ogni Scopa. Chi ha preso il 2 di denari perde 5 punti.",
            ),
            con(
                "La Bazzica",
                "Prima della tua prima carta del turno puoi chiamare Bazzica, se le 3 carte lo permettono. Tre carte diverse con somma fino a 9 valgono 2 Scope; una coppia con somma fino a 9 ne vale 3; un tris 7. Chiamandola giochi a carte scoperte per il resto del turno.",
                Esempio::Bazzica {
                    mano: &["coppe_03", "spade_03", "bastoni_02"],
                    scope: 3,
                },
            ),
            con(
                "Il 7 bello fa da jolly",
                "Per chiamare la Bazzica puoi dare al 7 bello il valore che ti serve, da 1 a 10. Quando lo giochi prende con quel valore; se resta in tavola torna a valere 7.",
                Esempio::Bazzica {
                    mano: &["denari_07", "spade_05", "bastoni_05"],
                    scope: 7,
                },
            ),
            passo(
                "Credito e Che Palo!",
                "Le Scope della Bazzica si segnano girando carte della tua presa: se non ne hai abbastanza restano a credito fino alla prossima presa, ma a fine mano il credito si perde. A carte e gesti non si parla: l'unica cosa che puoi dire è \"Che Palo!\", quando ti manca una carta per una Bazzica.",
            ),
        ],
    },
    Capitolo {
        variante: Some(Variante::Heisenberg),
        titolo: "Heisenberg",
        intro: "Come la Vanilla, con lo scambio di una carta con il compagno a ogni turno.",
        passi: &[
            passo(
                "Lo scambio",
                "Dopo la distribuzione ognuno passa una carta al compagno. Le Bazziche si chiamano con le carte che hai dopo lo scambio.",
            ),
            passo(
                "Passare veloce",
                "Passare la carta \"veloce\" è l'unico segnale concesso: di solito vuol dire che ti manca poco per una Bazzica grossa e ti farebbe comodo il 7 bello o una carta bassa.",
            ),
        ],
    },
    Capitolo {
        variante: Some(Variante::Einstein),
        titolo: "Einstein",
        intro: "Come la Heisenberg, ma il tris fatto con il 7 bello vale meno.",
        passi: &[con(
            "Tris con il 7 bello",
            "Se fai un tris dando al 7 bello il valore delle altre due carte, la Bazzica vale 5 Scope invece di 7.",
            Esempio::Bazzica {
                mano: &["denari_07", "spade_05", "bastoni_05"],
                scope: 5,
            },
        )],
    },
    Capitolo {
        variante: Some(Variante::Fermi),
        titolo: "Fermi",
        intro: "Come la Einstein, con un 2 di denari meno cattivo e il Napoleone.",
        passi: &[
            passo(
                "Il 2 di denari",
                "Vale -5 più un punto per ogni punto di mazzo fatto dalla squadra che l'ha preso: con 3 punti di mazzo vale -2.",
            ),
            con(
                "Napoleone",
                "Chi prende tutti i denari fa Napoleone, che vale 10 punti.",
                Esempio::Napola {
                    carte: &[
                        "denari_01",
                        "denari_02",
                        "denari_03",
                        "denari_04",
                        "denari_05",
                        "denari_06",
                        "denari_07",
                        "denari_08",
                        "denari_09",
                        "denari_10",
                    ],
                    punti: 10,
                },
            ),
        ],
    },
    Capitolo {
        variante: Some(Variante::Pascal),
        titolo: "Pascal",
        intro: "Come la Fermi, ma chi chiama Bazzica perde l'asso piglia tutto.",
        passi: &[con(
            "Assi in Bazzica",
            "Se hai chiamato Bazzica, i tuoi assi valgono semplicemente 1: prendono solo un altro asso o niente.",
            Esempio::Presa(Presa {
                tavola: &["coppe_04", "spade_06"],
                giocata: "bastoni_01",
                presa: &[],
                bazzica: true,
                ..Presa::BASE
            }),
        )],
    },
    Capitolo {
        variante: Some(Variante::Curie),
        titolo: "Curie",
        intro: "Come la Pascal, con due nuovi colpi: la Piccola e la Grande.",
        passi: &[
            con(
                "Piccola",
                "Se prendi subito, con una carta dello stesso valore, la carta appena calata dal giocatore prima di te, fai Piccola: vale una Scopa. Non vale per il primo giocatore del turno né per l'ultima giocata della mano.",
                Esempio::Presa(Presa {
                    tavola: &["spade_02", "coppe_06"],
                    giocata: "bastoni_06",
                    presa: &["coppe_06"],
                    appena_giocata: Some("coppe_06"),
                    esito: Esito {
                        piccola: true,
                        ..Esito::NULLO
                    },
                    ..Presa::BASE
                }),
            ),
            con(
                "Grande",
                "Se la carta giocata e tutte quelle prese sono dello stesso seme fai Grande: ogni carta della presa vale una Scopa. Se sparecchi, conta solo la Grande.",
                Esempio::Presa(Presa {
                    tavola: &["coppe_02", "coppe_03"],
                    giocata: "coppe_05",
                    presa: &["coppe_02", "coppe_03"],
                    esito: Esito {
                        grande: true,
                        ..Esito::NULLO
                    },
                    ..Presa::BASE
                }),
            ),
        ],
    },
    Capitolo {
        variante: Some(Variante::FedericoII),
        titolo: "Federico II",
        intro: "Come la Curie, con la Napola.",
        passi: &[con(
            "Napola",
            "Se prendi asso, 2 e 3 di denari fai Napola: vale un punto per ogni denaro consecutivo dall'asso. Il Napoleone è la Napola al 10.",
            Esempio::Napola {
                carte: &[
                    "denari_01",
                    "denari_02",
                    "denari_03",
                    "denari_04",
                    "denari_05",
                    "denari_09",
                ],
                punti: 5,
            },
        )],
    },
    Capitolo {
        variante: Some(Variante::Avogadro),
        titolo: "Avogadro",
        intro: "Come la Federico II, con gli assi che fanno sempre Scopa.",
        passi: &[con(
            "Ogni asso è una Scopa",
            "Ogni asso che prendi si segna come Scopa, a meno che non stia già facendo Scopa per conto suo. Non vale nell'ultima giocata della mano.",
            Esempio::Presa(Presa {
                tavola: &["spade_01", "coppe_04", "bastoni_09"],
                giocata: "denari_05",
                presa: &["spade_01", "coppe_04"],
                esito: Esito {
                    assi: 1,
                    ..Esito::NULLO
                },
                ..Presa::BASE
            }),
        )],
    },
    Capitolo {
        variante: None,
        titolo: "Margherita Hack",
        intro: "Si aggiunge a qualunque variante e cambia solo la durata della partita.",
        passi: &[passo(
            "Quattro mani",
            "Non si gioca fino a 21: si fanno 4 mani, così ognuno fa il mazziere una volta. Vince chi ha più punti alla fine; a parità è pareggio.",
        )],
    },
];

/// The chapter that introduces a variant.
pub fn capitolo(v: Variante) -> &'static Capitolo {
    CAPITOLI
        .iter()
        .find(|c| c.variante == Some(v))
        .expect("every variant has a chapter")
}

/// Everything needed to learn a variant: the chapters from the basics up to it, in order,
/// plus Margherita Hack when it is on.
pub fn percorso(v: Variante, margherita: bool) -> Vec<&'static Capitolo> {
    let fino = Variante::TUTTE.iter().position(|x| *x == v).unwrap_or(0);
    Variante::TUTTE[..=fino]
        .iter()
        .map(|v| capitolo(*v))
        .chain(margherita.then(|| &CAPITOLI[CAPITOLI.len() - 1]))
        .collect()
}

/// Rules a chapter's examples are played with.
pub fn regole(c: &Capitolo) -> RulesConfig {
    RulesConfig::preset(
        c.variante.unwrap_or(Variante::Vanilla),
        c.variante.is_none(),
    )
}

pub fn carta(codice: &str) -> Result<Card, String> {
    Card::from_codice(codice).ok_or_else(|| format!("unknown card {codice:?}"))
}

pub fn carte(codici: &[&str]) -> Result<Vec<Card>, String> {
    codici.iter().map(|c| carta(c)).collect()
}

/// Replays an example on the engine; `Err` says what does not match.
pub fn verifica(e: &Esempio, regole: &RulesConfig) -> Result<(), String> {
    match e {
        Esempio::Presa(p) => verifica_presa(p, regole),
        Esempio::Bazzica { mano, scope } => {
            let migliore = opzioni_bazzica(&carte(mano)?, regole)
                .iter()
                .map(|o| o.scope)
                .max();
            (migliore == Some(*scope))
                .then_some(())
                .ok_or_else(|| format!("{mano:?}: best Bazzica {migliore:?}, not {scope}"))
        }
        Esempio::Napola { carte: c, punti } => {
            let n = napola(&carte(c)?);
            (n == *punti)
                .then_some(())
                .ok_or_else(|| format!("{c:?}: Napola {n}, not {punti}"))
        }
    }
}

fn verifica_presa(p: &Presa, regole: &RulesConfig) -> Result<(), String> {
    let mut st = GameState::vuoto(regole.clone(), 0, Mulberry32::new(0));
    let giocata = carta(p.giocata)?;
    st.tavola = carte(p.tavola)?;
    st.mani[0] = vec![giocata, carta("denari_10")?];
    st.piccola_target = p.appena_giocata.map(carta).transpose()?;
    if p.bazzica {
        st.bazzica[0] = Some(BazzicaChiamata {
            tipo: TipoBazzica::Diverse,
            scope: 2,
            sette: None,
            carte: vec![],
        });
    }
    let presa = carte(p.presa)?;
    let ordina = |mut v: Vec<Card>| {
        v.sort();
        v
    };
    let opzioni: Vec<Vec<Card>> = st
        .opzioni_presa(0, giocata)
        .into_iter()
        .map(ordina)
        .collect();
    if !opzioni.contains(&ordina(presa.clone())) {
        return Err(format!(
            "{}: {:?} is not among {opzioni:?}",
            p.giocata, p.presa
        ));
    }
    let a = Azione::Gioca {
        seat: 0,
        carta: giocata,
        presa,
    };
    let e = st.anteprima(&a).expect("a play").esito;
    let esito = Esito {
        scopa: e.scopa,
        piccola: e.piccola,
        grande: e.grande,
        assi: e.assi.len(),
    };
    (esito == p.esito)
        .then_some(())
        .ok_or_else(|| format!("{}: scores {esito:?}, not {:?}", p.giocata, p.esito))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ogni_esempio_dice_il_vero() {
        for c in &CAPITOLI {
            for p in c.passi {
                if let Some(e) = &p.esempio {
                    verifica(e, &regole(c)).unwrap_or_else(|err| panic!("{}: {err}", p.titolo));
                }
            }
        }
    }

    #[test]
    fn ogni_variante_ha_il_suo_capitolo() {
        for v in Variante::TUTTE {
            assert_eq!(capitolo(v).variante, Some(v));
            assert!(!capitolo(v).passi.is_empty());
        }
    }

    #[test]
    fn il_percorso_parte_dalle_basi() {
        let titoli = |v, m| -> Vec<&str> { percorso(v, m).iter().map(|c| c.titolo).collect() };
        assert_eq!(titoli(Variante::Vanilla, false), ["Le basi"]);
        assert_eq!(
            titoli(Variante::Einstein, true),
            ["Le basi", "Heisenberg", "Einstein", "Margherita Hack"]
        );
        assert_eq!(percorso(Variante::Avogadro, false).len(), 8);
    }

    #[test]
    fn gli_esempi_sbagliati_vengono_scoperti() {
        let r = RulesConfig::default();
        let presa_sbagliata = Esempio::Presa(Presa {
            tavola: &["coppe_03", "spade_07"],
            giocata: "denari_07",
            presa: &["coppe_03"],
            ..Presa::BASE
        });
        assert!(verifica(&presa_sbagliata, &r).is_err());
        let esito_sbagliato = Esempio::Presa(Presa {
            tavola: &["coppe_03", "spade_07"],
            giocata: "denari_07",
            presa: &["spade_07"],
            esito: Esito::SCOPA,
            ..Presa::BASE
        });
        assert!(verifica(&esito_sbagliato, &r).is_err());
        let bazzica = Esempio::Bazzica {
            mano: &["coppe_09", "spade_08", "bastoni_02"],
            scope: 2,
        };
        assert!(verifica(&bazzica, &r).is_err());
        let napola = Esempio::Napola {
            carte: &["denari_01", "denari_02"],
            punti: 2,
        };
        assert!(verifica(&napola, &r).is_err());
        assert!(carta("jolly").is_err());
    }
}
