//! The replicated table: lobby, seats, settings and the game in progress.
//!
//! Every peer holds a [`Tavolo`] and applies the same ordered [`Voce`] log to it, so all
//! replicas stay identical. [`Tavolo::impronta`] is the hash peers compare to detect drift.

use std::collections::BTreeSet;

use bazzica_core::game::{Evento, NOMI_SQUADRE};
use bazzica_core::{Azione, Fase, GameState, RulesConfig, Seat, Variante};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Stable player identity, chosen by the device and kept across reconnections.
pub type IdGiocatore = String;

pub const NOMI_BOT: [&str; 4] = ["Bot Nord", "Bot Est", "Bot Sud", "Bot Ovest"];
pub const AVATAR: [&str; 12] = [
    "🦊", "🐻", "🐸", "🦁", "🐼", "🐵", "🦉", "🐙", "🐱", "🐲", "🐢", "🦭",
];
pub const AVATAR_BOT: &str = "🤖";
pub const FRASI: [&str; 10] = [
    "Veloce?!?",
    "Di#p?@co",
    "Praticamente pari",
    "Che culo...",
    "Aperta come...",
    "Faccio una giocata un po' particolare",
    "Stai giocando con paura",
    "Sei MATTO?!",
    "Ma hai tutti gli assi??",
    "CHE CULO",
];
pub const MAX_NOME: usize = 24;
pub const MAX_TIMER: u32 = 300;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Partecipante {
    pub id: IdGiocatore,
    pub nome: String,
    pub avatar: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Posto {
    Umano { id: IdGiocatore },
    Bot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Impostazioni {
    pub nome: String,
    pub variante: Variante,
    pub margherita: bool,
    /// Seconds for the Heisenberg exchange; 0 disables the timer.
    pub timer_scambio: u32,
    /// Seconds for each human play; 0 disables the timer.
    pub timer_giocata: u32,
    pub squadre: [String; 2],
    /// How long bots "think" before playing.
    #[serde(default)]
    pub ritmo: Ritmo,
    /// Seconds the end-of-hand summary waits for the players; 0 waits forever.
    #[serde(default = "trenta")]
    pub timer_fine_mano: u32,
}

fn trenta() -> u32 {
    30
}

/// Bot pace: a factor on the node's base bot delay, so everyone can follow the plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ritmo {
    Calmo,
    #[default]
    Normale,
    Svelto,
}

impl Ritmo {
    pub const TUTTI: [Ritmo; 3] = [Ritmo::Calmo, Ritmo::Normale, Ritmo::Svelto];

    pub fn nome(self) -> &'static str {
        match self {
            Ritmo::Calmo => "Calmo",
            Ritmo::Normale => "Normale",
            Ritmo::Svelto => "Svelto",
        }
    }

    pub fn da_nome(nome: &str) -> Option<Ritmo> {
        Ritmo::TUTTI.into_iter().find(|r| r.nome() == nome)
    }

    /// Scales a delay in ms (percent factors keep the arithmetic exact).
    pub fn scala(self, ms: u64) -> u64 {
        let percento = match self {
            Ritmo::Calmo => 160,
            Ritmo::Normale => 100,
            Ritmo::Svelto => 50,
        };
        ms * percento / 100
    }
}

impl Default for Impostazioni {
    fn default() -> Self {
        Impostazioni {
            nome: "Tavolo".into(),
            variante: Variante::Vanilla,
            margherita: false,
            timer_scambio: 30,
            timer_giocata: 15,
            squadre: NOMI_SQUADRE.map(String::from),
            ritmo: Ritmo::default(),
            timer_fine_mano: trenta(),
        }
    }
}

impl Impostazioni {
    /// Clamps free-text fields and durations to sane bounds.
    pub fn normalizzate(mut self) -> Self {
        self.nome = taglia(&self.nome, 40, "Tavolo");
        self.timer_scambio = self.timer_scambio.min(MAX_TIMER);
        self.timer_giocata = self.timer_giocata.min(MAX_TIMER);
        self.timer_fine_mano = self.timer_fine_mano.min(MAX_TIMER);
        for (i, s) in self.squadre.iter_mut().enumerate() {
            *s = taglia(s, 20, NOMI_SQUADRE[i]);
        }
        self
    }

    pub fn regole(&self) -> RulesConfig {
        RulesConfig::preset(self.variante, self.margherita)
    }
}

pub fn taglia(s: &str, max: usize, predefinito: &str) -> String {
    let t: String = s.trim().chars().take(max).collect();
    if t.is_empty() { predefinito.into() } else { t }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatoTavolo {
    Lobby,
    InCorso,
    Finita,
}

/// Which timer expired.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipoTimer {
    Scambio,
    Giocata,
    /// The end-of-hand summary: on expiry everyone counts as having acknowledged.
    FineMano,
}

/// One entry of the replicated log. Only the sequencer (host) creates entries; it has already
/// checked who asked for it, so applying an entry only checks game legality.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "voce", rename_all = "snake_case")]
pub enum Voce {
    /// A participant joins the table (or updates name/avatar).
    Entra(Partecipante),
    Siedi {
        id: IdGiocatore,
        posto: Seat,
    },
    Alzati {
        posto: Seat,
    },
    Bot {
        posto: Seat,
        aggiungi: bool,
    },
    Imposta(Impostazioni),
    Avvia {
        seed: u64,
    },
    /// A game action: from a human, a bot (chosen by the host) or the end-of-hand system step.
    Azione(Azione),
    /// A turn timer expired: the listed actions were chosen on the player's behalf.
    Scaduto {
        timer: TipoTimer,
        seat: Option<Seat>,
        azioni: Vec<Azione>,
    },
    /// A human acknowledged the end-of-hand summary.
    Prosegui {
        id: IdGiocatore,
    },
    Termina {
        da: IdGiocatore,
    },
    /// A bot takes the seat of a human who left during the game.
    Sostituisci {
        posto: Seat,
    },
    /// The human whose seat a bot took is back and plays it again.
    Riprendi {
        posto: Seat,
        id: IdGiocatore,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Errore(pub String);

impl std::fmt::Display for Errore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Errore {}

fn errore<T>(m: &str) -> Result<T, Errore> {
    Err(Errore(m.into()))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tavolo {
    pub impostazioni: Impostazioni,
    /// Everyone who ever joined, in join order. The first connected one is the host.
    pub partecipanti: Vec<Partecipante>,
    pub posti: [Option<Posto>; 4],
    pub stato: StatoTavolo,
    pub partita: Option<GameState>,
    /// Humans who acknowledged the current end-of-hand summary.
    pub ack: BTreeSet<IdGiocatore>,
    pub terminata_da: Option<IdGiocatore>,
    /// For each seat a bot took over during the game: whose it was.
    #[serde(default)]
    pub sostituiti: [Option<IdGiocatore>; 4],
    /// Number of log entries applied so far.
    pub n: u64,
}

impl Tavolo {
    pub fn nuovo(creatore: Partecipante, impostazioni: Impostazioni) -> Tavolo {
        Tavolo {
            impostazioni: impostazioni.normalizzate(),
            partecipanti: vec![creatore],
            posti: Default::default(),
            stato: StatoTavolo::Lobby,
            partita: None,
            ack: BTreeSet::new(),
            terminata_da: None,
            sostituiti: Default::default(),
            n: 0,
        }
    }

    /// Short hash of the whole replica, compared by peers after every entry.
    pub fn impronta(&self) -> String {
        let json = serde_json::to_vec(self).expect("serializable");
        let h = Sha256::digest(&json);
        h[..8].iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn partecipante(&self, id: &str) -> Option<&Partecipante> {
        self.partecipanti.iter().find(|p| p.id == id)
    }

    pub fn creatore(&self) -> &IdGiocatore {
        &self.partecipanti[0].id
    }

    /// Seats controlled by `id`.
    pub fn posti_di(&self, id: &str) -> Vec<Seat> {
        (0..4)
            .filter(
                |&s| matches!(&self.posti[usize::from(s)], Some(Posto::Umano { id: x }) if x == id),
            )
            .collect()
    }

    pub fn umani(&self) -> BTreeSet<IdGiocatore> {
        self.posti
            .iter()
            .filter_map(|p| match p {
                Some(Posto::Umano { id }) => Some(id.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn e_bot(&self, seat: Seat) -> bool {
        matches!(self.posti[usize::from(seat)], Some(Posto::Bot))
    }

    /// Display name of a seat.
    pub fn nome_posto(&self, seat: Seat) -> String {
        match &self.posti[usize::from(seat)] {
            Some(Posto::Umano { id }) => self
                .partecipante(id)
                .map_or_else(|| "?".into(), |p| p.nome.clone()),
            Some(Posto::Bot) => NOMI_BOT[usize::from(seat)].into(),
            None => "—".into(),
        }
    }

    fn in_lobby(&mut self) -> Result<(), Errore> {
        match self.stato {
            StatoTavolo::InCorso => errore("La partita è già iniziata"),
            StatoTavolo::Finita => {
                self.stato = StatoTavolo::Lobby;
                self.partita = None;
                self.terminata_da = None;
                Ok(())
            }
            StatoTavolo::Lobby => Ok(()),
        }
    }

    fn partita_mut(&mut self) -> Result<&mut GameState, Errore> {
        match (&self.stato, self.partita.as_mut()) {
            (StatoTavolo::InCorso, Some(p)) => Ok(p),
            _ => errore("Nessuna partita in corso"),
        }
    }

    /// The table after one log entry, leaving `self` untouched.
    pub fn applicata(&self, voce: &Voce) -> Result<(Tavolo, Vec<Evento>), Errore> {
        let mut copia = self.clone();
        let eventi = copia.applica_interno(voce)?;
        copia.n += 1;
        Ok((copia, eventi))
    }

    /// Applies one log entry. On error the table is left unchanged.
    pub fn applica(&mut self, voce: &Voce) -> Result<Vec<Evento>, Errore> {
        let (nuovo, eventi) = self.applicata(voce)?;
        *self = nuovo;
        Ok(eventi)
    }

    fn applica_interno(&mut self, voce: &Voce) -> Result<Vec<Evento>, Errore> {
        match voce {
            Voce::Entra(p) => {
                let p = Partecipante {
                    id: p.id.clone(),
                    nome: taglia(&p.nome, MAX_NOME, "Giocatore"),
                    avatar: if AVATAR.contains(&p.avatar.as_str()) {
                        p.avatar.clone()
                    } else {
                        AVATAR[self.partecipanti.len() % AVATAR.len()].into()
                    },
                };
                match self.partecipanti.iter_mut().find(|x| x.id == p.id) {
                    Some(x) => *x = p,
                    None => self.partecipanti.push(p),
                }
                Ok(vec![])
            }
            Voce::Siedi { id, posto } => {
                self.in_lobby()?;
                if self.partecipante(id).is_none() {
                    return errore("Giocatore sconosciuto");
                }
                let p = self.posto_mut(*posto)?;
                if p.is_some() {
                    return errore("Posto occupato");
                }
                *p = Some(Posto::Umano { id: id.clone() });
                Ok(vec![])
            }
            Voce::Alzati { posto } => {
                self.in_lobby()?;
                *self.posto_mut(*posto)? = None;
                Ok(vec![])
            }
            Voce::Bot { posto, aggiungi } => {
                self.in_lobby()?;
                let p = self.posto_mut(*posto)?;
                if *aggiungi {
                    if p.is_some() {
                        return errore("Posto occupato");
                    }
                    *p = Some(Posto::Bot);
                } else if *p == Some(Posto::Bot) {
                    *p = None;
                }
                Ok(vec![])
            }
            Voce::Imposta(imp) => {
                self.in_lobby()?;
                self.impostazioni = imp.clone().normalizzate();
                Ok(vec![])
            }
            Voce::Avvia { seed } => {
                self.in_lobby()?;
                if self.posti.iter().any(Option::is_none) {
                    return errore("Servono 4 giocatori (umani o bot)");
                }
                let (st, eventi) = GameState::new_game(self.impostazioni.regole(), *seed, None);
                self.partita = Some(st);
                self.stato = StatoTavolo::InCorso;
                self.ack.clear();
                self.terminata_da = None;
                self.sostituiti = Default::default();
                Ok(eventi)
            }
            Voce::Azione(a) => {
                let st = self.partita_mut()?;
                let eventi = st.apply(a).map_err(|e| Errore(e.0))?;
                self.dopo_azione();
                Ok(eventi)
            }
            Voce::Scaduto {
                timer: TipoTimer::FineMano,
                ..
            } => {
                if self.partita_mut()?.fase != Fase::FineMano {
                    return errore("Non è la fine di una mano");
                }
                self.ack = self.umani();
                Ok(vec![])
            }
            Voce::Sostituisci { posto } => {
                self.partita_mut()?;
                let p = self.posto_mut(*posto)?;
                let Some(Posto::Umano { id }) = p.take() else {
                    return errore("Nessun giocatore da sostituire");
                };
                *p = Some(Posto::Bot);
                self.sostituiti[usize::from(*posto)] = Some(id);
                Ok(vec![])
            }
            Voce::Riprendi { posto, id } => {
                self.partita_mut()?;
                let i = usize::from(*posto);
                if self.sostituiti.get(i) != Some(&Some(id.clone())) || !self.e_bot(*posto) {
                    return errore("Questo posto non era tuo");
                }
                self.posti[i] = Some(Posto::Umano { id: id.clone() });
                self.sostituiti[i] = None;
                Ok(vec![])
            }
            Voce::Scaduto { azioni, .. } => {
                let st = self.partita_mut()?;
                let mut eventi = vec![];
                for a in azioni {
                    eventi.extend(st.apply(a).map_err(|e| Errore(e.0))?);
                }
                self.dopo_azione();
                Ok(eventi)
            }
            Voce::Prosegui { id } => {
                let st = self.partita_mut()?;
                if st.fase != Fase::FineMano {
                    return errore("Non è la fine di una mano");
                }
                self.ack.insert(id.clone());
                Ok(vec![])
            }
            Voce::Termina { da } => {
                self.partita_mut()?;
                self.stato = StatoTavolo::Finita;
                self.terminata_da = Some(da.clone());
                Ok(vec![])
            }
        }
    }

    fn posto_mut(&mut self, posto: Seat) -> Result<&mut Option<Posto>, Errore> {
        self.posti
            .get_mut(usize::from(posto))
            .ok_or_else(|| Errore("Posto inesistente".into()))
    }

    fn dopo_azione(&mut self) {
        let fase = self.partita.as_ref().map(|p| p.fase);
        if fase != Some(Fase::FineMano) {
            self.ack.clear();
        }
        if fase == Some(Fase::FinePartita) {
            self.stato = StatoTavolo::Finita;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn il_ritmo_scala_il_ritardo() {
        assert_eq!(Ritmo::Calmo.scala(1000), 1600);
        assert_eq!(Ritmo::Normale.scala(1000), 1000);
        assert_eq!(Ritmo::Svelto.scala(1000), 500);
        for r in Ritmo::TUTTI {
            assert_eq!(Ritmo::da_nome(r.nome()), Some(r));
        }
        assert_eq!(Ritmo::da_nome("boh"), None);
    }

    fn io(id: &str) -> Partecipante {
        Partecipante {
            id: id.into(),
            nome: id.into(),
            avatar: AVATAR[0].into(),
        }
    }

    #[test]
    fn applicata_non_tocca_il_tavolo() {
        let t = Tavolo::nuovo(io("a"), Impostazioni::default());
        let (dopo, _) = t
            .applicata(&Voce::Siedi {
                id: "a".into(),
                posto: 0,
            })
            .unwrap();
        assert_eq!(t, Tavolo::nuovo(io("a"), Impostazioni::default()));
        assert_eq!((dopo.n, dopo.posti_di("a")), (1, vec![0]));
        let errore = t.applicata(&Voce::Siedi {
            id: "x".into(),
            posto: 0,
        });
        assert_eq!(errore, Err(Errore("Giocatore sconosciuto".into())));
        assert_eq!(t, Tavolo::nuovo(io("a"), Impostazioni::default()));
    }

    #[test]
    fn impostazioni_senza_timer_fine_mano_aspettano_trenta_secondi() {
        let mut v = serde_json::to_value(Impostazioni::default()).unwrap();
        v.as_object_mut().unwrap().remove("timer_fine_mano");
        let imp: Impostazioni = serde_json::from_value(v).unwrap();
        assert_eq!(imp.timer_fine_mano, 30);
        let lungo = Impostazioni {
            timer_fine_mano: 10_000,
            ..Impostazioni::default()
        };
        assert_eq!(lungo.normalizzate().timer_fine_mano, MAX_TIMER);
    }

    #[test]
    fn impostazioni_senza_ritmo_usano_il_normale() {
        let mut v = serde_json::to_value(Impostazioni::default()).unwrap();
        v.as_object_mut().unwrap().remove("ritmo");
        let imp: Impostazioni = serde_json::from_value(v).unwrap();
        assert_eq!(imp.ritmo, Ritmo::Normale);
    }
}
