//! Choosing what a card takes (also by touching the cards on the table) and booking a play
//! ahead of one's turn. Pure logic: the screen in `partita.rs` only draws it.

use bazzica_core::bazzica::opzioni_bazzica;
use bazzica_core::game::EsitoGiocata;
use bazzica_core::{Azione, Card, Fase, GameState, Seat};

/// What the player meant when booking a card: the play is made only if it still means this.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intento {
    /// The card stays on the table.
    Posa,
    /// The card takes exactly these cards.
    Prendi(Vec<Card>),
    /// An ace that sweeps whatever is on the table.
    Sparecchia,
}

/// A play booked while waiting for one's turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prenotazione {
    pub carta: Card,
    pub intento: Intento,
}

/// What the local player is choosing. Valid only for the deal it was made in (`chiave`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scelte {
    /// Hand number, turn number and seat these choices belong to.
    pub chiave: (u32, u8, Seat),
    /// Card picked from the hand, not played yet.
    pub carta: Option<Card>,
    /// Table cards picked for the capture.
    pub presa: Vec<Card>,
    pub prenotata: Option<Prenotazione>,
    /// Bazzica to call as soon as the turn comes (the value given to the 7 bello).
    pub bazzica: Option<Option<u8>>,
    /// A booked play was dropped because it no longer meant the same thing.
    pub cambiata: bool,
}

impl Scelte {
    pub fn chiave_di(st: &GameState, seat: Seat) -> (u32, u8, Seat) {
        (st.mano_n, st.turno, seat)
    }

    /// The choices that still apply to this deal and hand (stale ones are dropped).
    pub fn valide(&self, st: &GameState, seat: Seat) -> Scelte {
        let chiave = Scelte::chiave_di(st, seat);
        if self.chiave != chiave {
            return Scelte {
                chiave,
                ..Scelte::default()
            };
        }
        let mano = &st.mani[usize::from(seat)];
        let mut s = self.clone();
        // a card picked for the exchange is not a card picked for the play
        let passata = st.fase == Fase::Scambio && st.scambio_passata[usize::from(seat)].is_some();
        if passata || s.carta.is_some_and(|c| !mano.contains(&c)) {
            s.carta = None;
            s.presa.clear();
        }
        if s.prenotata
            .as_ref()
            .is_some_and(|p| !mano.contains(&p.carta))
            || st.fase != Fase::Gioco
        {
            s.prenotata = None;
        }
        if s.bazzica.is_some_and(|v| !bazzica_chiamabile(st, seat, v)) {
            s.bazzica = None;
        }
        s
    }

    /// Fresh choices for the same deal.
    pub fn vuote(&self) -> Scelte {
        Scelte {
            chiave: self.chiave,
            ..Scelte::default()
        }
    }
}

fn stessa_presa(a: &[Card], b: &[Card]) -> bool {
    a.len() == b.len() && a.iter().all(|c| b.contains(c))
}

/// Whether `seat` may still call this Bazzica in this turn (now or when its turn comes).
pub fn bazzica_chiamabile(st: &GameState, seat: Seat, sette: Option<u8>) -> bool {
    let s = usize::from(seat);
    st.fase == Fase::Gioco
        && !st.giocato_turno[s]
        && st.bazzica[s].is_none()
        && opzioni_bazzica(&st.mani[s], &st.regole)
            .iter()
            .any(|o| o.sette == sette)
}

/// The state as `seat` will find it once the booked Bazzica is called (it changes what the
/// 7 bello and, with Pascal, the aces can take).
pub fn stato_previsto(st: &GameState, seat: Seat, bazzica: Option<Option<u8>>) -> GameState {
    let mut previsto = st.clone();
    if let Some(sette) = bazzica
        && bazzica_chiamabile(st, seat, sette)
    {
        previsto.corrente = seat;
        let _ = previsto.apply(&Azione::Bazzica { seat, sette });
        previsto.corrente = st.corrente;
    }
    previsto
}

/// What `carta` could take right now; `[[]]` when it would stay on the table.
pub fn opzioni(st: &GameState, seat: Seat, carta: Card) -> Vec<Vec<Card>> {
    if st.fase != Fase::Gioco || !st.mani[usize::from(seat)].contains(&carta) {
        return Vec::new();
    }
    st.opzioni_presa(seat, carta)
}

/// True when the card has nothing to take.
pub fn solo_posa(opzioni: &[Vec<Card>]) -> bool {
    opzioni.iter().all(Vec::is_empty)
}

/// The option the player has settled on: the only one there is, or the one whose cards are
/// all picked on the table.
pub fn decisa<'a>(opzioni: &'a [Vec<Card>], presa: &[Card]) -> Option<&'a Vec<Card>> {
    match opzioni {
        [unica] => Some(unica),
        _ => opzioni
            .iter()
            .find(|o| !o.is_empty() && stessa_presa(o, presa)),
    }
}

/// The picked table cards, as far as they still fit one of the options.
pub fn presa_valida(opzioni: &[Vec<Card>], presa: &[Card]) -> Vec<Card> {
    if let [unica] = opzioni {
        return unica.clone();
    }
    let compatibile =
        !presa.is_empty() && opzioni.iter().any(|o| presa.iter().all(|c| o.contains(c)));
    if compatibile {
        presa.to_vec()
    } else {
        Vec::new()
    }
}

/// Table cards that belong to some option (touching one picks it).
pub fn prendibili(opzioni: &[Vec<Card>]) -> Vec<Card> {
    let mut out: Vec<Card> = Vec::new();
    for &c in opzioni.iter().flatten() {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// The picked cards after touching table card `c`: a picked card is put back, a new one is
/// added (starting over if it does not go with the others), and when only one option is left
/// it is completed.
pub fn tocca_tavola(opzioni: &[Vec<Card>], presa: &[Card], c: Card) -> Vec<Card> {
    if opzioni.len() <= 1 {
        return presa_valida(opzioni, presa);
    }
    if presa.contains(&c) {
        return Vec::new();
    }
    let con = |sel: &[Card]| -> Vec<&Vec<Card>> {
        opzioni
            .iter()
            .filter(|o| sel.iter().all(|x| o.contains(x)))
            .collect()
    };
    let mut nuova = presa.to_vec();
    nuova.push(c);
    let mut adatte = con(&nuova);
    if adatte.is_empty() {
        nuova = vec![c];
        adatte = con(&nuova);
    }
    match adatte.as_slice() {
        [] => presa.to_vec(),
        [unica] => (*unica).clone(),
        _ => nuova,
    }
}

/// The meaning of playing `carta` on `presa` in the given position.
pub fn intento(st: &GameState, seat: Seat, carta: Card, presa: &[Card]) -> Intento {
    if presa.is_empty() {
        Intento::Posa
    } else if st.asso_piglia_tutto(seat, carta) && presa.len() == st.tavola.len() {
        Intento::Sparecchia
    } else {
        Intento::Prendi(presa.to_vec())
    }
}

/// The booked play, if it is `seat`'s turn and the play still means what was booked.
pub fn risolvi(st: &GameState, seat: Seat, p: &Prenotazione) -> Option<Azione> {
    if st.fase != Fase::Gioco || st.corrente != seat {
        return None;
    }
    let opz = opzioni(st, seat, p.carta);
    let presa = match &p.intento {
        Intento::Posa => opz.iter().find(|o| o.is_empty())?,
        Intento::Prendi(voluta) => opz.iter().find(|o| stessa_presa(o, voluta))?,
        Intento::Sparecchia => opz.iter().find(|o| {
            !o.is_empty() && o.len() == st.tavola.len() && st.asso_piglia_tutto(seat, p.carta)
        })?,
    };
    Some(Azione::Gioca {
        seat,
        carta: p.carta,
        presa: presa.clone(),
    })
}

/// What should be played by itself now: the booked Bazzica first, then the booked card.
pub fn automatica(st: &GameState, seat: Seat, s: &Scelte) -> Option<Azione> {
    if st.fase != Fase::Gioco || st.corrente != seat {
        return None;
    }
    if let Some(sette) = s.bazzica
        && bazzica_chiamabile(st, seat, sette)
    {
        return Some(Azione::Bazzica { seat, sette });
    }
    s.prenotata.as_ref().and_then(|p| risolvi(st, seat, p))
}

/// True when the booked card can no longer be played as booked: the player must choose again.
pub fn saltata(st: &GameState, seat: Seat, s: &Scelte) -> bool {
    st.fase == Fase::Gioco
        && st.corrente == seat
        && s.prenotata.is_some()
        && automatica(st, seat, s).is_none()
}

/// What a play would achieve. Off turn only what does not depend on the plays still to come
/// is shown (a Scopa or a Piccola may vanish before the turn comes).
pub fn esito(st: &GameState, seat: Seat, carta: Card, presa: &[Card]) -> EsitoGiocata {
    let s = usize::from(seat);
    // the dealer plays last: its last card of the third turn closes the hand
    let ultima = st.turno == 3 && st.mani[s].len() == 1 && seat == st.mazziere;
    let (_, mut e, _) = st.risolvi_giocata(seat, carta, presa, ultima);
    if st.corrente != seat {
        e.scopa = false;
        e.piccola = false;
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use bazzica_core::cards::Seme::{self, *};
    use bazzica_core::prng::Mulberry32;
    use bazzica_core::{RulesConfig, Variante};

    fn c(s: Seme, v: u8) -> Card {
        Card::new(s, v)
    }

    /// Seat 0 holds `mano`, the table holds `tavola`, and it is `corrente`'s turn.
    fn stato(variante: Variante, mano: &[Card], tavola: &[Card], corrente: Seat) -> GameState {
        let mut st = GameState::vuoto(RulesConfig::preset(variante, false), 1, Mulberry32::new(1));
        st.fase = Fase::Gioco;
        st.turno = 1;
        st.mano_n = 1;
        st.mazziere = 3;
        st.mani[0] = mano.to_vec();
        st.tavola = tavola.to_vec();
        st.corrente = corrente;
        st
    }

    #[test]
    fn si_sceglie_la_combinazione_toccando_la_tavola() {
        let (due, cinque, tre, quattro) = (c(Coppe, 2), c(Spade, 5), c(Bastoni, 3), c(Coppe, 4));
        let st = stato(
            Variante::Vanilla,
            &[c(Spade, 7)],
            &[due, cinque, tre, quattro],
            0,
        );
        let opz = opzioni(&st, 0, c(Spade, 7));
        assert_eq!(opz.len(), 2);
        assert!(decisa(&opz, &[]).is_none());
        // one card of a combination is enough: the rest follows
        let presa = tocca_tavola(&opz, &[], tre);
        assert!(stessa_presa(&presa, &[tre, quattro]));
        assert!(decisa(&opz, &presa).is_some());
        // a card of the other combination starts over
        let presa = tocca_tavola(&opz, &presa, due);
        assert!(stessa_presa(&presa, &[due, cinque]));
        // touching a picked card puts everything back
        assert!(tocca_tavola(&opz, &presa, due).is_empty());
    }

    #[test]
    fn con_piu_combinazioni_sulla_stessa_carta_si_continua_a_scegliere() {
        // 9 on 1 2 3 5 6: 1+2+6, 1+3+5, 3+6
        let t = [
            c(Coppe, 1),
            c(Coppe, 2),
            c(Coppe, 3),
            c(Spade, 5),
            c(Spade, 6),
        ];
        let st = stato(Variante::Vanilla, &[c(Bastoni, 9)], &t, 0);
        let opz = opzioni(&st, 0, c(Bastoni, 9));
        assert_eq!(opz.len(), 3);
        let presa = tocca_tavola(&opz, &[], t[4]);
        assert_eq!(presa, [t[4]], "the 6 is in two combinations");
        assert!(decisa(&opz, &presa).is_none());
        let presa = tocca_tavola(&opz, &presa, t[2]);
        assert!(stessa_presa(&presa, &[t[2], t[4]]));
        assert!(decisa(&opz, &presa).is_some());
    }

    #[test]
    fn la_carta_singola_e_obbligata_e_gia_scelta() {
        let st = stato(
            Variante::Vanilla,
            &[c(Spade, 7)],
            &[c(Coppe, 7), c(Bastoni, 3), c(Coppe, 4)],
            0,
        );
        let opz = opzioni(&st, 0, c(Spade, 7));
        assert_eq!(opz, [vec![c(Coppe, 7)]]);
        assert_eq!(presa_valida(&opz, &[]), [c(Coppe, 7)]);
        assert_eq!(decisa(&opz, &[]), Some(&vec![c(Coppe, 7)]));
    }

    #[test]
    fn la_prenotazione_si_gioca_solo_se_vale_ancora() {
        let sette = c(Spade, 7);
        let (tre, quattro) = (c(Bastoni, 3), c(Coppe, 4));
        let mut st = stato(Variante::Vanilla, &[sette, c(Coppe, 9)], &[tre, quattro], 3);
        let p = Prenotazione {
            carta: sette,
            intento: intento(&st, 0, sette, &[tre, quattro]),
        };
        assert_eq!(risolvi(&st, 0, &p), None, "not my turn yet");
        st.corrente = 0;
        assert_eq!(
            risolvi(&st, 0, &p),
            Some(Azione::Gioca {
                seat: 0,
                carta: sette,
                presa: vec![tre, quattro]
            })
        );
        // somebody laid a 7: the single card must be taken, the booked capture is gone
        st.tavola.push(c(Coppe, 7));
        assert_eq!(risolvi(&st, 0, &p), None);
        let s = Scelte {
            chiave: Scelte::chiave_di(&st, 0),
            prenotata: Some(p),
            ..Scelte::default()
        };
        assert!(saltata(&st, 0, &s));
    }

    #[test]
    fn una_carta_prenotata_per_restare_in_tavola_non_prende_a_sorpresa() {
        let sette = c(Spade, 7);
        let mut st = stato(Variante::Vanilla, &[sette], &[c(Coppe, 9)], 3);
        let p = Prenotazione {
            carta: sette,
            intento: intento(&st, 0, sette, &[]),
        };
        assert_eq!(p.intento, Intento::Posa);
        st.corrente = 0;
        assert!(risolvi(&st, 0, &p).is_some());
        st.tavola.push(c(Coppe, 7));
        assert_eq!(risolvi(&st, 0, &p), None);
    }

    #[test]
    fn l_asso_prenotato_sparecchia_anche_se_la_tavola_cambia() {
        let asso = c(Spade, 1);
        let mut st = stato(Variante::Vanilla, &[asso], &[c(Coppe, 9), c(Coppe, 4)], 3);
        let p = Prenotazione {
            carta: asso,
            intento: intento(&st, 0, asso, &st.tavola.clone()),
        };
        assert_eq!(p.intento, Intento::Sparecchia);
        st.corrente = 0;
        st.tavola.push(c(Bastoni, 5));
        let Some(Azione::Gioca { presa, .. }) = risolvi(&st, 0, &p) else {
            panic!("the ace still sweeps");
        };
        assert_eq!(presa.len(), 3);
        // an ace on the table: the booked sweep is gone
        st.tavola.push(c(Coppe, 1));
        assert_eq!(risolvi(&st, 0, &p), None);
        // an empty table: the ace would just be laid down
        st.tavola.clear();
        assert_eq!(risolvi(&st, 0, &p), None);
    }

    #[test]
    fn la_bazzica_prenotata_viene_prima_della_carta_e_cambia_il_sette_bello() {
        let sb = bazzica_core::cards::SETTE_BELLO;
        let mano = [sb, c(Spade, 1), c(Bastoni, 3)];
        let (cinque, sette) = (c(Coppe, 5), c(Coppe, 7));
        let mut st = stato(Variante::Vanilla, &mano, &[cinque, sette], 3);
        assert!(bazzica_chiamabile(&st, 0, Some(5)));
        assert!(!bazzica_chiamabile(&st, 0, Some(9)));
        // without the Bazzica the 7 bello takes the 7, with "7 bello = 5" it takes the 5
        assert_eq!(opzioni(&st, 0, sb), [vec![sette]]);
        let previsto = stato_previsto(&st, 0, Some(Some(5)));
        assert_eq!(previsto.corrente, 3);
        assert_eq!(opzioni(&previsto, 0, sb), [vec![cinque]]);

        let s = Scelte {
            chiave: Scelte::chiave_di(&st, 0),
            bazzica: Some(Some(5)),
            prenotata: Some(Prenotazione {
                carta: sb,
                intento: Intento::Prendi(vec![cinque]),
            }),
            ..Scelte::default()
        };
        assert_eq!(automatica(&st, 0, &s), None, "not my turn yet");
        st.corrente = 0;
        let a = automatica(&st, 0, &s).unwrap();
        assert_eq!(
            a,
            Azione::Bazzica {
                seat: 0,
                sette: Some(5)
            }
        );
        st.apply(&a).unwrap();
        let s = s.valide(&st, 0);
        assert_eq!(s.bazzica, None, "called: nothing left to book");
        assert_eq!(
            automatica(&st, 0, &s),
            Some(Azione::Gioca {
                seat: 0,
                carta: sb,
                presa: vec![cinque]
            })
        );
    }

    #[test]
    fn le_scelte_di_un_altro_turno_non_valgono() {
        let sette = c(Spade, 7);
        let mut st = stato(Variante::Vanilla, &[sette], &[], 3);
        let s = Scelte {
            chiave: Scelte::chiave_di(&st, 0),
            carta: Some(sette),
            ..Scelte::default()
        };
        assert_eq!(s.valide(&st, 0).carta, Some(sette));
        st.turno = 2;
        assert_eq!(s.valide(&st, 0).carta, None);
        st.turno = 1;
        st.mani[0].clear();
        assert_eq!(s.valide(&st, 0).carta, None);
    }

    #[test]
    fn fuori_turno_non_si_promettono_scope() {
        let sette = c(Spade, 7);
        let mut st = stato(Variante::Vanilla, &[sette], &[c(Coppe, 7)], 3);
        assert!(!esito(&st, 0, sette, &[c(Coppe, 7)]).scopa);
        st.corrente = 0;
        assert!(esito(&st, 0, sette, &[c(Coppe, 7)]).scopa);
    }
}
