//! Who meets whom in a room, and how a failed or dropped pair tries again. Pure: the signaller
//! feeds it decoded envelopes, matchbox requests, link changes and the clock, and carries out
//! the returned [`Azione`]s.
//!
//! For each pair the lower id makes the WebRTC offer (matchbox's `NewPeer`); the higher one
//! asks for it with an announcement addressed to the other.
//!
//! Retries: matchbox remembers every remote `PeerId` it ever handshaked with and will not accept
//! a second offer from it. So each attempt of a pair has a number `t`, carried in the envelopes,
//! and matchbox sees the remote under an alias derived from `(id, t)`: every retry is a new
//! peer to it. The offerer starts attempt `t + 1` when the link drops, when the handshake has
//! not connected after [`ATTESA_HANDSHAKE`], or when the answerer asks for it; at most
//! [`MAX_FALLIMENTI`] failures in a row. The answerer asks for `t + 1` when the link drops.

use std::collections::BTreeMap;

use matchbox_socket::{PeerEvent, PeerId, PeerRequest, PeerSignal};
use serde::{Deserialize, Serialize};

/// Seconds a handshake may take before the offerer tries again.
pub const ATTESA_HANDSHAKE: u64 = 20;
/// Failed attempts in a row after which a pair stops retrying (until the other side asks).
pub const MAX_FALLIMENTI: u32 = 3;

/// What travels, encrypted, in the Nostr events of a room.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Busta {
    /// "I am in the room" (`a: None`, on join and every keep-alive), or, addressed to the
    /// offerer, "please offer me attempt `tentativo`".
    Annuncio {
        da: PeerId,
        a: Option<PeerId>,
        #[serde(default)]
        tentativo: u32,
    },
    /// A WebRTC offer, answer or ICE candidate of attempt `tentativo`.
    Segnale {
        da: PeerId,
        a: PeerId,
        s: PeerSignal,
        #[serde(default)]
        tentativo: u32,
    },
    /// "I am leaving".
    Addio { da: PeerId },
}

impl Busta {
    pub fn mittente(&self) -> PeerId {
        match self {
            Busta::Annuncio { da, .. } | Busta::Segnale { da, .. } | Busta::Addio { da } => *da,
        }
    }

    pub fn destinatario(&self) -> Option<PeerId> {
        match self {
            Busta::Annuncio { a, .. } => *a,
            Busta::Segnale { a, .. } => Some(*a),
            Busta::Addio { .. } => None,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Azione {
    /// Hand an event to matchbox.
    Emetti(PeerEvent),
    /// Publish an envelope to the relays.
    Pubblica(Busta),
}

/// One remote peer of the room.
#[derive(Clone, Debug, Default)]
struct Coppia {
    tentativo: u32,
    collegata: bool,
    /// Offerer: when the current attempt was offered (s).
    offerta: Option<u64>,
    /// Answerer: a signal of the current attempt arrived.
    segnale: bool,
    fallimenti: u32,
}

pub struct Incontri {
    io: PeerId,
    coppie: BTreeMap<PeerId, Coppia>,
    /// Alias shown to matchbox -> (remote id, attempt).
    alias: BTreeMap<PeerId, (PeerId, u32)>,
}

/// The `PeerId` matchbox sees for attempt `t` with `da` (attempt 0 is `da` itself).
pub fn alias(da: PeerId, t: u32) -> PeerId {
    if t == 0 {
        return da;
    }
    let k = u128::from(t).wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835);
    PeerId(uuid::Uuid::from_u128(da.0.as_u128() ^ k))
}

impl Incontri {
    pub fn new(io: PeerId) -> Incontri {
        Incontri {
            io,
            coppie: BTreeMap::new(),
            alias: BTreeMap::new(),
        }
    }

    /// Peers seen in the room.
    pub fn visti(&self) -> usize {
        self.coppie.len()
    }

    fn offro(&self, da: PeerId) -> bool {
        self.io < da
    }

    fn alias_di(&mut self, da: PeerId, t: u32) -> PeerId {
        let a = alias(da, t);
        self.alias.insert(a, (da, t));
        a
    }

    /// The offerer starts attempt `t` with `da`.
    fn offri(&mut self, da: PeerId, t: u32, ora: u64) -> Azione {
        let c = self.coppie.entry(da).or_default();
        c.tentativo = t;
        c.offerta = Some(ora);
        c.collegata = false;
        Azione::Emetti(PeerEvent::NewPeer(self.alias_di(da, t)))
    }

    /// The answerer asks `da` for an offer of the current attempt.
    fn chiedi(&self, da: PeerId) -> Azione {
        Azione::Pubblica(Busta::Annuncio {
            da: self.io,
            a: Some(da),
            tentativo: self.coppie.get(&da).map_or(0, |c| c.tentativo),
        })
    }

    /// An envelope from the room, already checked to come from someone else and to be
    /// addressed to us or to everyone.
    pub fn busta(&mut self, b: Busta, ora: u64) -> Vec<Azione> {
        match b {
            Busta::Annuncio { da, a, tentativo } => {
                let c = self.coppie.entry(da).or_default().clone();
                if self.offro(da) {
                    let chiede_altro = a == Some(self.io) && tentativo > c.tentativo;
                    if c.offerta.is_none() && c.fallimenti == 0 {
                        vec![self.offri(da, c.tentativo, ora)]
                    } else if chiede_altro && !c.collegata {
                        vec![self.offri(da, tentativo, ora)]
                    } else {
                        vec![]
                    }
                } else if c.segnale || c.collegata {
                    vec![]
                } else {
                    vec![self.chiedi(da)]
                }
            }
            Busta::Segnale {
                da, s, tentativo, ..
            } => {
                let c = self.coppie.entry(da).or_default();
                if tentativo < c.tentativo {
                    return vec![]; // an old attempt
                }
                if tentativo > c.tentativo {
                    *c = Coppia {
                        tentativo,
                        fallimenti: c.fallimenti,
                        ..Coppia::default()
                    };
                }
                c.segnale = true;
                let sender = self.alias_di(da, tentativo);
                vec![Azione::Emetti(PeerEvent::Signal { sender, data: s })]
            }
            Busta::Addio { da } => match self.coppie.remove(&da) {
                Some(c) => {
                    let alias = alias(da, c.tentativo);
                    vec![Azione::Emetti(PeerEvent::PeerLeft(alias))]
                }
                None => vec![],
            },
        }
    }

    /// A request of matchbox, as an envelope.
    pub fn richiesta(&self, r: PeerRequest) -> Busta {
        match r {
            PeerRequest::Signal { receiver, data } => {
                let (a, tentativo) = self.alias.get(&receiver).copied().unwrap_or((receiver, 0));
                Busta::Segnale {
                    da: self.io,
                    a,
                    s: data,
                    tentativo,
                }
            }
            // keep-alives double as presence announcements for late joiners
            PeerRequest::KeepAlive => Busta::Annuncio {
                da: self.io,
                a: None,
                tentativo: 0,
            },
        }
    }

    pub fn addio(&self) -> Busta {
        Busta::Addio { da: self.io }
    }

    /// The WebRTC link with matchbox peer `alias` opened.
    pub fn collegato(&mut self, alias: PeerId) {
        if let Some((da, t)) = self.alias.get(&alias).copied()
            && let Some(c) = self.coppie.get_mut(&da)
            && c.tentativo == t
        {
            c.collegata = true;
            c.fallimenti = 0;
        }
    }

    /// The WebRTC link with matchbox peer `alias` closed: try the next attempt.
    pub fn perso(&mut self, alias: PeerId, ora: u64) -> Vec<Azione> {
        let Some((da, t)) = self.alias.get(&alias).copied() else {
            return vec![];
        };
        let Some(c) = self.coppie.get_mut(&da) else {
            return vec![];
        };
        if c.tentativo != t {
            return vec![];
        }
        c.collegata = false;
        c.segnale = false;
        c.offerta = None;
        c.fallimenti += 1;
        if c.fallimenti > MAX_FALLIMENTI {
            return vec![];
        }
        c.tentativo = t + 1;
        if self.offro(da) {
            vec![self.offri(da, t + 1, ora)]
        } else {
            vec![self.chiedi(da)]
        }
    }

    /// Offers that have not connected in time are made again (offerer side).
    pub fn scadenze(&mut self, ora: u64) -> Vec<Azione> {
        let scadute: Vec<(PeerId, u32)> = self
            .coppie
            .iter()
            .filter(|(_, c)| !c.collegata && c.offerta.is_some_and(|t| ora >= t + ATTESA_HANDSHAKE))
            .map(|(da, c)| (*da, c.tentativo))
            .collect();
        let mut azioni = vec![];
        for (da, t) in scadute {
            let c = self.coppie.get_mut(&da).expect("present");
            c.offerta = None;
            c.fallimenti += 1;
            if c.fallimenti <= MAX_FALLIMENTI {
                azioni.push(self.offri(da, t + 1, ora));
            }
        }
        azioni
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> PeerId {
        PeerId(uuid::Uuid::from_bytes([n; 16]))
    }

    fn annuncio(da: PeerId) -> Busta {
        Busta::Annuncio {
            da,
            a: None,
            tentativo: 0,
        }
    }

    fn nuovi(azioni: &[Azione]) -> Vec<PeerId> {
        azioni
            .iter()
            .filter_map(|a| match a {
                Azione::Emetti(PeerEvent::NewPeer(p)) => Some(*p),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn il_piu_basso_offre_l_altro_chiede() {
        let mut basso = Incontri::new(id(1));
        assert_eq!(nuovi(&basso.busta(annuncio(id(2)), 0)), [id(2)]);
        assert!(basso.busta(annuncio(id(2)), 1).is_empty(), "offers once");
        let mut alto = Incontri::new(id(2));
        let chiede = Azione::Pubblica(Busta::Annuncio {
            da: id(2),
            a: Some(id(1)),
            tentativo: 0,
        });
        assert_eq!(alto.busta(annuncio(id(1)), 0), [chiede]);
        assert_eq!(alto.visti(), 1);
    }

    #[test]
    fn pari_perso_si_ritenta_con_un_alias_nuovo() {
        let mut basso = Incontri::new(id(1));
        basso.busta(annuncio(id(2)), 0);
        basso.collegato(id(2));
        let azioni = basso.perso(id(2), 100);
        let nuovo = nuovi(&azioni);
        assert_eq!(nuovo, [alias(id(2), 1)]);
        assert_ne!(nuovo[0], id(2), "matchbox must see a peer it never met");
        // the answerer asks for the same next attempt on its side
        let mut alto = Incontri::new(id(2));
        alto.busta(
            Busta::Segnale {
                da: id(1),
                a: id(2),
                s: PeerSignal::Offer("o".into()),
                tentativo: 0,
            },
            0,
        );
        alto.collegato(id(1));
        let chiede = alto.perso(id(1), 100);
        assert_eq!(
            chiede,
            [Azione::Pubblica(Busta::Annuncio {
                da: id(2),
                a: Some(id(1)),
                tentativo: 1
            })]
        );
        // the answer of attempt 1 goes to the alias, and back to the remote id on the wire
        let segnale = alto.busta(
            Busta::Segnale {
                da: id(1),
                a: id(2),
                s: PeerSignal::Offer("o2".into()),
                tentativo: 1,
            },
            101,
        );
        let a1 = alias(id(1), 1);
        assert_eq!(
            segnale,
            [Azione::Emetti(PeerEvent::Signal {
                sender: a1,
                data: PeerSignal::Offer("o2".into())
            })]
        );
        let risposta = alto.richiesta(PeerRequest::Signal {
            receiver: a1,
            data: PeerSignal::Answer("r".into()),
        });
        assert_eq!(
            risposta,
            Busta::Segnale {
                da: id(2),
                a: id(1),
                s: PeerSignal::Answer("r".into()),
                tentativo: 1
            }
        );
    }

    #[test]
    fn l_offerente_risponde_a_chi_chiede_un_tentativo_nuovo() {
        let mut basso = Incontri::new(id(1));
        basso.busta(annuncio(id(2)), 0);
        let chiede = Busta::Annuncio {
            da: id(2),
            a: Some(id(1)),
            tentativo: 1,
        };
        assert_eq!(nuovi(&basso.busta(chiede.clone(), 5)), [alias(id(2), 1)]);
        assert!(basso.busta(chiede, 6).is_empty(), "already on attempt 1");
    }

    #[test]
    fn handshake_bloccato_si_ritenta_fino_al_limite() {
        let mut basso = Incontri::new(id(1));
        basso.busta(annuncio(id(2)), 0);
        assert!(basso.scadenze(ATTESA_HANDSHAKE - 1).is_empty());
        let mut ora = 0;
        for t in 1..=MAX_FALLIMENTI {
            ora += ATTESA_HANDSHAKE;
            assert_eq!(nuovi(&basso.scadenze(ora)), [alias(id(2), t)]);
        }
        ora += ATTESA_HANDSHAKE;
        assert!(basso.scadenze(ora).is_empty(), "gives up after the cap");
        assert!(basso.busta(annuncio(id(2)), ora).is_empty());
    }

    #[test]
    fn segnali_di_un_tentativo_vecchio_ignorati() {
        let mut alto = Incontri::new(id(2));
        let segnale = |t| Busta::Segnale {
            da: id(1),
            a: id(2),
            s: PeerSignal::Offer("o".into()),
            tentativo: t,
        };
        assert_eq!(alto.busta(segnale(2), 0).len(), 1);
        assert!(alto.busta(segnale(1), 0).is_empty());
    }

    #[test]
    fn addio_rimuove_il_pari() {
        let mut basso = Incontri::new(id(1));
        basso.busta(annuncio(id(2)), 0);
        assert_eq!(
            basso.busta(Busta::Addio { da: id(2) }, 1),
            [Azione::Emetti(PeerEvent::PeerLeft(id(2)))]
        );
        assert_eq!(basso.visti(), 0);
        assert!(basso.busta(Busta::Addio { da: id(2) }, 2).is_empty());
        // if it comes back, it is a new pair
        assert_eq!(nuovi(&basso.busta(annuncio(id(2)), 3)), [id(2)]);
        assert_eq!(basso.addio(), Busta::Addio { da: id(1) });
    }

    #[test]
    fn keep_alive_e_un_annuncio() {
        let basso = Incontri::new(id(1));
        assert_eq!(basso.richiesta(PeerRequest::KeepAlive), annuncio(id(1)));
        let b = Busta::Segnale {
            da: id(1),
            a: id(2),
            s: PeerSignal::Answer("x".into()),
            tentativo: 0,
        };
        assert_eq!((b.mittente(), b.destinatario()), (id(1), Some(id(2))));
        assert_eq!(annuncio(id(3)).destinatario(), None);
    }
}
