//! Heuristic bot (port of `bazzica/bots/heuristic.py`): weighs captures, the risk of
//! leaving an opponent a Scopa, Bazziche, the card exchange and partner signals.
//!
//! Floating-point expressions keep the reference evaluation order so that decisions are
//! bit-identical to the Python bot.

use std::collections::BTreeMap;

use super::Agent;
use crate::bazzica::{OpzioneBazzica, opzioni_bazzica, puo_dire_che_palo};
use crate::cards::{Card, DUE_DENARI, MAZZO, RE_BELLO, SETTE_BELLO, Seme};
use crate::game::{Azione, BazzicaChiamata, Fase, GameState, Osservazione};
use crate::prng::Mulberry32;
use crate::rules::RulesConfig;

/// Indicative value (in points) of having `c` in one's capture pile.
pub fn valore_carta(c: Card, r: &RulesConfig) -> f64 {
    if c == Card::IGNOTA {
        return 0.06;
    }
    let mut v = 0.06;
    if c.seme() == Seme::Denari {
        v += 0.12;
        if r.federico && c.val() <= 3 {
            v += 0.25;
        }
    }
    v += match c.val() {
        7 => 0.3,
        6 => 0.18,
        1 => 0.15,
        5 => 0.06,
        _ => 0.0,
    };
    if c == SETTE_BELLO {
        v += 1.2;
    } else if c == RE_BELLO {
        v += 1.0;
    } else if c == DUE_DENARI {
        v -= if r.fermi { 3.0 } else { 4.5 };
    }
    v
}

fn comb(n: u64, k: u64) -> u64 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    (0..k).fold(1u64, |acc, i| acc * (n - i) / (i + 1))
}

/// Probability that a hand of `n_mano` cards drawn from `n_pool` holds at least one of
/// `k` specific cards.
pub fn p_almeno_uno(n_pool: usize, k: usize, n_mano: usize) -> f64 {
    if k == 0 || n_mano == 0 || n_pool == 0 {
        return 0.0;
    }
    let n_mano = n_mano.min(n_pool);
    if n_pool < k + n_mano {
        return 1.0;
    }
    let (n_pool, k, n_mano) = (n_pool as u64, k as u64, n_mano as u64);
    1.0 - comb(n_pool - k, n_mano) as f64 / comb(n_pool, n_mano) as f64
}

pub fn miglior_bazzica(cards: &[Card], r: &RulesConfig) -> u32 {
    opzioni_bazzica(cards, r)
        .iter()
        .map(|o| o.scope)
        .max()
        .unwrap_or(0)
}

fn somma_valori_positivi(carte: &[Card], r: &RulesConfig) -> f64 {
    carte
        .iter()
        .fold(0.0, |acc, &c| acc + valore_carta(c, r).max(0.0))
}

pub struct HeuristicAgent {
    rng: Mulberry32,
}

impl HeuristicAgent {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Mulberry32::new(seed),
        }
    }

    /// Cards that could be in other players' hands or in the deck.
    fn ignote(obs: &Osservazione) -> Vec<Card> {
        let mut viste = [false; 41];
        let mut vedi = |c: Card| viste[c.index()] = true;
        obs.mano
            .iter()
            .chain(&obs.tavola)
            .chain(&obs.carte_iniziali_tavola)
            .for_each(|&c| vedi(c));
        for g in &obs.giocate_mano {
            vedi(g.1);
            g.2.iter().for_each(|&c| vedi(c));
        }
        for o in obs.altri.values() {
            o.carte.iter().flatten().for_each(|&c| vedi(c));
        }
        if let Some(c) = obs.scambio.mia_passata {
            vedi(c);
        }
        MAZZO
            .iter()
            .copied()
            .filter(|c| !viste[c.index()])
            .collect()
    }

    /// Risk (in points) that the next opponent exploits the table left behind.
    fn rischio(
        obs: &Osservazione,
        tavola_dopo: &[Card],
        lasciata: Option<Card>,
        r: &RulesConfig,
        ignote: &[Card],
    ) -> f64 {
        if tavola_dopo.is_empty() {
            return 0.0;
        }
        let opp = (obs.seat + 1) % 4;
        let o = &obs.altri[&opp];
        let n_opp = o.n as usize;
        if n_opp == 0 {
            return 0.0;
        }
        let rivelata = o.carte.as_deref();
        let pool: &[Card] = match rivelata {
            Some(v) if !v.is_empty() => v,
            _ => ignote,
        };
        let pascal_opp = r.pascal && obs.bazziche.contains_key(&opp);
        let mut vincenti = [false; 41];
        let mut aggiungi = |val: u8| {
            pool.iter()
                .filter(|c| c.val() == val)
                .for_each(|c| vincenti[c.index()] = true);
        };
        let valori: Vec<u8> = tavola_dopo.iter().map(|c| c.val()).collect();
        if !valori.contains(&1) && !pascal_opp {
            aggiungi(1);
        }
        if valori.len() == 1 {
            aggiungi(valori[0]);
        } else {
            let s: u32 = valori.iter().map(|&v| u32::from(v)).sum();
            if s <= 10 {
                aggiungi(s as u8);
            }
        }
        let n_vincenti = vincenti.iter().filter(|&&x| x).count();
        let p = match rivelata {
            Some(_) => {
                if n_vincenti > 0 {
                    1.0
                } else {
                    0.0
                }
            }
            None => p_almeno_uno(ignote.len(), n_vincenti, n_opp),
        };
        let mut rischio = p * (1.0 + somma_valori_positivi(tavola_dopo, r));
        // An opponent's Piccola on the card just left.
        if let Some(c) = lasciata.filter(|_| r.curie) {
            let k = pool.iter().filter(|x| x.val() == c.val()).count();
            let pp = match rivelata {
                Some(_) => {
                    if k > 0 {
                        1.0
                    } else {
                        0.0
                    }
                }
                None => p_almeno_uno(ignote.len(), k, n_opp),
            };
            rischio += 0.6 * pp;
        }
        // The next opponent will capture something anyway: expected loss of valuable cards.
        let presa_gen = 0.35 * somma_valori_positivi(tavola_dopo, r);
        rischio + presa_gen * 0.5
    }

    fn valuta_giocata(obs: &Osservazione, st: &GameState, a: &Azione, ignote: &[Card]) -> f64 {
        let r = &st.regole;
        let Azione::Gioca { carta, presa, .. } = a else {
            unreachable!()
        };
        let pre = st.anteprima(a).unwrap();
        let mut gain = 0.0;
        let (tavola_dopo, lasciata) = if !presa.is_empty() {
            for &c in std::iter::once(carta).chain(presa) {
                gain += valore_carta(c, r);
            }
            gain += f64::from(pre.scoperte);
            (
                st.tavola
                    .iter()
                    .copied()
                    .filter(|c| !presa.contains(c))
                    .collect::<Vec<_>>(),
                None,
            )
        } else {
            gain -= 0.5 * valore_carta(*carta, r);
            let mut t = st.tavola.clone();
            t.push(*carta);
            (t, Some(*carta))
        };
        if !pre.ultima_giocata {
            gain -= Self::rischio(obs, &tavola_dopo, lasciata, r, ignote);
        }
        // keep the ace (sweeps the table) for later if it captures nothing now
        if presa.is_empty() && carta.val() == 1 {
            gain -= 0.4;
        }
        gain
    }

    fn miglior_giocata(
        &mut self,
        obs: &Osservazione,
        st: &GameState,
        gioc: &[Azione],
        ignote: &[Card],
    ) -> (Option<Azione>, f64) {
        let (mut best, mut best_v) = (None, -1e9);
        for a in gioc {
            let v = Self::valuta_giocata(obs, st, a, ignote) + self.rng.random() * 1e-3;
            if v > best_v {
                best = Some(a.clone());
                best_v = v;
            }
        }
        (best, best_v)
    }

    /// Exclaims once per turn, before the first card, only for a real "palo".
    fn vuole_palo(obs: &Osservazione) -> bool {
        let s = obs.seat;
        if obs.fase != Fase::Gioco || obs.che_palo.contains(&s) || obs.giocato_turno[usize::from(s)]
        {
            return false;
        }
        if obs.bazziche.contains_key(&s) {
            return false;
        }
        puo_dire_che_palo(&obs.mano, &obs.regole) && Self::palo_significativo(&obs.mano)
    }

    /// A real "palo": a pair (missing the triple) or holding the 7 bello.
    fn palo_significativo(mano: &[Card]) -> bool {
        let mut valori: Vec<u8> = mano.iter().map(|c| c.val()).collect();
        valori.sort_unstable();
        valori.dedup();
        valori.len() < mano.len() || mano.contains(&SETTE_BELLO)
    }

    fn scegli_bazzica(
        &mut self,
        obs: &Osservazione,
        st: &GameState,
        bazziche: &[Azione],
        gioc: &[Azione],
        ignote: &[Card],
    ) -> Azione {
        let seat = obs.seat;
        let ops: BTreeMap<Option<u8>, OpzioneBazzica> =
            opzioni_bazzica(&st.mani[usize::from(seat)], &st.regole)
                .into_iter()
                .map(|o| (o.sette, o))
                .collect();
        let (_, v_senza) = self.miglior_giocata(obs, st, gioc, ignote);
        let (mut best, mut best_v) = (None, v_senza);
        for a in bazziche {
            let Azione::Bazzica { sette, .. } = a else {
                unreachable!()
            };
            let op = &ops[sette];
            let mut st2 = GameState::da_osservazione(obs);
            st2.bazzica[usize::from(seat)] = Some(BazzicaChiamata {
                tipo: op.tipo,
                scope: op.scope,
                sette: op.sette,
                carte: obs.mano.clone(),
            });
            let gioc2 = giocate(&st2, seat);
            let (_, v) = self.miglior_giocata(obs, &st2, &gioc2, ignote);
            let tot = f64::from(op.scope) + v;
            if tot > best_v {
                best = Some(a.clone());
                best_v = tot;
            }
        }
        match best {
            Some(a) => a,
            None => self.miglior_giocata(obs, st, gioc, ignote).0.unwrap(),
        }
    }

    /// Expected Scope of a Bazzica with the kept cards plus an unknown card from the partner.
    fn attesa_bazzica(kept: &[Card], r: &RulesConfig, ignote: &[Card]) -> f64 {
        if kept.len() >= 3 {
            return f64::from(miglior_bazzica(&kept[..3], r));
        }
        if ignote.is_empty() {
            return 0.0;
        }
        let mut tot = 0.0;
        for &c in ignote {
            let mut mano = kept.to_vec();
            mano.push(c);
            tot += f64::from(miglior_bazzica(&mano, r));
        }
        tot / ignote.len() as f64
    }

    fn scambio(&mut self, obs: &Osservazione, legal: &[Azione]) -> Azione {
        let r = &obs.regole;
        let sc = &obs.scambio;
        let p = usize::from(obs.compagno);
        let mano = &obs.mano;
        let compagno_passato = sc.passati[p];
        let compagno_veloce = sc.veloci[p];
        let mut candidati: Vec<Card> = legal
            .iter()
            .filter_map(|a| match a {
                Azione::Passa { carta, .. } => Some(*carta),
                _ => None,
            })
            .collect();
        candidati.sort_by_key(|c| c.codice());
        candidati.dedup();
        let ignote = Self::ignote(obs);
        let dealt: Vec<Card> = sc
            .distribuite
            .iter()
            .copied()
            .filter(|c| mano.contains(c))
            .collect();

        // With an interesting pair and the partner yet to pass, signal right away.
        if !compagno_passato {
            let coppia = dealt
                .iter()
                .map(|c| c.val())
                .find(|&v| dealt.iter().filter(|c| c.val() == v).count() >= 2);
            if let Some(v) = coppia
                && !dealt.contains(&SETTE_BELLO)
                && miglior_bazzica(&dealt, r) < 7
                && let Some(&altra) = dealt.iter().find(|c| c.val() != v)
            {
                return Azione::Passa {
                    seat: obs.seat,
                    carta: altra,
                    veloce: true,
                };
            }
            if let Some(a) = legal.iter().find(|a| matches!(a, Azione::Attendi { .. }))
                && miglior_bazzica(&dealt, r) < 7
            {
                return a.clone();
            }
        }

        let (mut best, mut best_v) = (None, -1e9);
        for c in candidati {
            let kept: Vec<Card> = mano.iter().copied().filter(|&x| x != c).collect();
            let mut v = if sc.ricevuta.is_none() && !compagno_passato {
                Self::attesa_bazzica(&kept, r, &ignote)
            } else if kept.len() == 3 {
                f64::from(miglior_bazzica(&kept, r))
            } else {
                Self::attesa_bazzica(&kept, r, &ignote)
            };
            // value for the partner
            if compagno_veloce {
                if c == SETTE_BELLO {
                    v += 3.0;
                }
                v += f64::from(10 - c.val()) * 0.08;
            } else {
                v += f64::from(10 - c.val()) * 0.03;
                if c == SETTE_BELLO {
                    v += 0.2;
                }
            }
            v += self.rng.random() * 1e-3;
            if v > best_v {
                best = Some(c);
                best_v = v;
            }
        }
        Azione::Passa {
            seat: obs.seat,
            carta: best.expect("at least one card to pass"),
            veloce: false,
        }
    }
}

fn giocate(st: &GameState, seat: u8) -> Vec<Azione> {
    st.mani[usize::from(seat)]
        .iter()
        .flat_map(|&c| {
            st.opzioni_presa(seat, c)
                .into_iter()
                .map(move |presa| Azione::Gioca {
                    seat,
                    carta: c,
                    presa,
                })
        })
        .collect()
}

impl Agent for HeuristicAgent {
    fn choose_action(&mut self, obs: &Osservazione, legal: &[Azione]) -> Azione {
        let ha = |f: fn(&Azione) -> bool| legal.iter().any(f);
        if ha(|a| matches!(a, Azione::ChePalo { .. })) && Self::vuole_palo(obs) {
            return legal
                .iter()
                .find(|a| matches!(a, Azione::ChePalo { .. }))
                .unwrap()
                .clone();
        }
        if ha(|a| matches!(a, Azione::Passa { .. })) {
            return self.scambio(obs, legal);
        }
        if !ha(|a| matches!(a, Azione::Gioca { .. })) {
            // optional off-turn decision: nothing to do
            return legal
                .iter()
                .find(|a| matches!(a, Azione::Attendi { .. }))
                .unwrap_or(&legal[0])
                .clone();
        }
        let st = GameState::da_osservazione(obs);
        let ignote = Self::ignote(obs);
        let gioc: Vec<Azione> = legal
            .iter()
            .filter(|a| matches!(a, Azione::Gioca { .. }))
            .cloned()
            .collect();
        let bazziche: Vec<Azione> = legal
            .iter()
            .filter(|a| matches!(a, Azione::Bazzica { .. }))
            .cloned()
            .collect();
        if !bazziche.is_empty() {
            return self.scegli_bazzica(obs, &st, &bazziche, &gioc, &ignote);
        }
        self.miglior_giocata(obs, &st, &gioc, &ignote).0.unwrap()
    }

    fn rng_stato(&self) -> u32 {
        self.rng.stato
    }
}
