//! Several peers on a simulated network in one process.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bazzica_core::bots::{Agent, HeuristicAgent};
use bazzica_core::{Azione, Fase, Variante};
use bazzica_net::*;

type H = u32;
type Filtro = fn(&Messaggio) -> bool;

/// Full-mesh test network. Messages go through the JSON codec and are delivered in order.
struct Rete {
    nodi: BTreeMap<H, Nodo<H>>,
    prossimo: H,
    coda: VecDeque<(H, H, Vec<u8>)>,
    ora: u64,
    rifiuti: BTreeMap<H, Vec<String>>,
    voci: Vec<Voce>,
    effimeri: BTreeMap<H, Vec<(IdGiocatore, Effimero)>>,
    /// Drop the next message matching (to, predicate).
    da_perdere: Vec<(H, Filtro)>,
}

fn io(id: &str) -> Partecipante {
    Partecipante {
        id: id.into(),
        nome: id.to_uppercase(),
        avatar: "🦊".into(),
    }
}

fn config() -> Config {
    Config {
        ritardo_bot: (100, 300),
        pausa_fine_mano: 500,
        pausa_palo: 3000,
    }
}

impl Rete {
    fn new() -> Rete {
        Rete {
            nodi: BTreeMap::new(),
            prossimo: 1,
            coda: VecDeque::new(),
            ora: 1_000,
            rifiuti: BTreeMap::new(),
            voci: vec![],
            effimeri: BTreeMap::new(),
            da_perdere: vec![],
        }
    }

    /// Adds a node and connects it to everyone already on the network.
    fn entra(&mut self, nodo: Nodo<H>) -> H {
        let h = self.prossimo;
        self.prossimo += 1;
        let altri: Vec<H> = self.nodi.keys().copied().collect();
        self.nodi.insert(h, nodo);
        for a in altri {
            self.nodi.get_mut(&a).unwrap().connesso(self.ora, h);
            self.nodi.get_mut(&h).unwrap().connesso(self.ora, a);
        }
        self.pompa();
        h
    }

    fn crea(&mut self, id: &str, imp: Impostazioni) -> H {
        self.entra(Nodo::crea(io(id), imp, 7, config()))
    }

    fn unisci(&mut self, id: &str) -> H {
        let seme = id.bytes().map(u64::from).sum();
        self.entra(Nodo::unisciti(io(id), seme, config()))
    }

    fn esci(&mut self, h: H) {
        self.nodi.remove(&h);
        self.coda.retain(|(a, b, _)| *a != h && *b != h);
        for n in self.nodi.values_mut() {
            n.disconnesso(self.ora, &h);
        }
        self.pompa();
    }

    /// Cuts every connection of `h` (like a phone going to sleep) and hands back its node.
    fn isola(&mut self, h: H) -> Nodo<H> {
        let mut n = self.nodi.remove(&h).unwrap();
        self.coda.retain(|(a, b, _)| *a != h && *b != h);
        for (k, o) in self.nodi.iter_mut() {
            o.disconnesso(self.ora, &h);
            n.disconnesso(self.ora, k);
        }
        let _ = n.uscite();
        self.pompa();
        n
    }

    fn nodo(&self, h: H) -> &Nodo<H> {
        &self.nodi[&h]
    }

    fn tavolo(&self, h: H) -> &Tavolo {
        self.nodi[&h].tavolo().expect("replica present")
    }

    fn richiedi(&mut self, h: H, r: Richiesta) {
        let ora = self.ora;
        self.nodi.get_mut(&h).unwrap().richiedi(ora, r);
        self.pompa();
    }

    /// Delivers everything until the network is quiet.
    fn pompa(&mut self) {
        loop {
            let mut mosso = false;
            let handles: Vec<H> = self.nodi.keys().copied().collect();
            for h in handles {
                for u in self.nodi.get_mut(&h).unwrap().uscite() {
                    mosso = true;
                    match u {
                        Uscita::Invia { a, msg } => {
                            let dest: Vec<H> = match a {
                                Destinatario::Tutti => {
                                    self.nodi.keys().copied().filter(|x| *x != h).collect()
                                }
                                Destinatario::Uno(p) => vec![p],
                            };
                            for d in dest {
                                if let Some(i) =
                                    self.da_perdere.iter().position(|(t, f)| *t == d && f(&msg))
                                {
                                    self.da_perdere.remove(i);
                                    continue;
                                }
                                self.coda.push_back((h, d, msg.codifica()));
                            }
                        }
                        Uscita::Rifiutata { motivo } => {
                            self.rifiuti.entry(h).or_default().push(motivo)
                        }
                        Uscita::Applicata { voce, .. } if self.nodi[&h].e_host() => {
                            self.voci.push(voce)
                        }
                        Uscita::Effimero { da, effetto } => {
                            self.effimeri.entry(h).or_default().push((da, effetto))
                        }
                        _ => {}
                    }
                }
            }
            if let Some((da, a, b)) = self.coda.pop_front() {
                mosso = true;
                let msg = Messaggio::decodifica(&b).expect("wire round-trip");
                if let Some(n) = self.nodi.get_mut(&a) {
                    n.ricevi(self.ora, da, msg);
                }
            }
            if !mosso {
                break;
            }
        }
    }

    /// Advances the clock to the next scheduled wake-up (or by `max` ms) and ticks everyone.
    fn avanza(&mut self, max: u64) {
        let prossima = self
            .nodi
            .values()
            .filter_map(Nodo::prossima_sveglia)
            .min()
            .unwrap_or(u64::MAX);
        self.ora = prossima.min(self.ora + max).max(self.ora);
        let ora = self.ora;
        for n in self.nodi.values_mut() {
            n.tick(ora);
        }
        self.pompa();
    }

    fn assert_sincronizzati(&self) {
        let impronte: BTreeSet<(u64, String)> = self
            .nodi
            .values()
            .map(|n| {
                let t = n.tavolo().expect("replica");
                (t.n, t.impronta())
            })
            .collect();
        assert_eq!(impronte.len(), 1, "replicas differ: {impronte:?}");
        let host: BTreeSet<_> = self.nodi.values().map(|n| n.host().cloned()).collect();
        assert_eq!(host.len(), 1, "peers disagree on the host: {host:?}");
    }

    /// Humans on peer `h` play one move if it is their turn (like the UI would).
    fn gioca_umani(&mut self, h: H, ack: bool) -> bool {
        let n = self.nodo(h);
        let t = n.tavolo().unwrap();
        if t.stato != StatoTavolo::InCorso || n.in_pausa() {
            return false;
        }
        let st = t.partita.as_ref().unwrap();
        if st.fase == Fase::FineMano {
            if ack && !t.ack.contains(&n.io().id) && !t.posti_di(&n.io().id).is_empty() {
                self.richiedi(h, Richiesta::Prosegui);
                return true;
            }
            return false;
        }
        for s in t.posti_di(&n.io().id) {
            if !st.required_actors().contains(&s) {
                continue;
            }
            let legal: Vec<Azione> = st
                .legal_actions(s)
                .into_iter()
                .filter(|a| !matches!(a, Azione::Attendi { .. } | Azione::ChePalo { .. }))
                .collect();
            if legal.is_empty() {
                continue;
            }
            let a = HeuristicAgent::new(t.n).choose_action(&st.observation(s), &legal);
            self.richiedi(h, Richiesta::Azione(a));
            return true;
        }
        false
    }

    /// Runs the game until it ends (or `limite` entries), humans answering immediately.
    fn gioca_fino(&mut self, limite: u64) {
        for _ in 0..200_000 {
            let host = self.host_h();
            if self.tavolo(host).stato != StatoTavolo::InCorso || self.tavolo(host).n >= limite {
                return;
            }
            let handles: Vec<H> = self.nodi.keys().copied().collect();
            let mut mosso = false;
            for h in handles {
                mosso |= self.gioca_umani(h, true);
            }
            if !mosso {
                self.avanza(10_000);
            }
        }
        let t = self.tavolo(self.host_h());
        let st = t.partita.as_ref().unwrap();
        panic!(
            "game did not end: n={} fase={:?} corrente={} req={:?} opt={:?} sveglia={:?} pausa={}",
            t.n,
            st.fase,
            st.corrente,
            st.required_actors(),
            st.optional_actors(),
            self.nodo(self.host_h()).prossima_sveglia(),
            self.nodo(self.host_h()).in_pausa()
        );
    }

    fn host_h(&self) -> H {
        *self
            .nodi
            .iter()
            .find(|(_, n)| n.e_host())
            .expect("a host")
            .0
    }
}

fn imp(variante: Variante) -> Impostazioni {
    Impostazioni {
        variante,
        ..Default::default()
    }
}

/// A, B humans at seats 0 and 2 (team 0), bots at 1 and 3, C watching.
fn tavolo_tipico(variante: Variante) -> (Rete, H, H, H) {
    let mut r = Rete::new();
    let a = r.crea("a", imp(variante));
    let b = r.unisci("b");
    let c = r.unisci("c");
    r.richiedi(a, Richiesta::Siedi { posto: 0 });
    r.richiedi(b, Richiesta::Siedi { posto: 2 });
    r.richiedi(
        c,
        Richiesta::Bot {
            posto: 1,
            aggiungi: true,
        },
    );
    r.richiedi(
        a,
        Richiesta::Bot {
            posto: 3,
            aggiungi: true,
        },
    );
    r.richiedi(b, Richiesta::Avvia);
    (r, a, b, c)
}

#[test]
fn lobby_replicata() {
    let mut r = Rete::new();
    let a = r.crea("a", imp(Variante::Vanilla));
    let b = r.unisci("b");
    let c = r.unisci("c");
    r.richiedi(b, Richiesta::Siedi { posto: 1 });
    r.richiedi(c, Richiesta::Siedi { posto: 1 });
    assert_eq!(r.rifiuti[&c], ["Posto occupato"]);
    r.richiedi(c, Richiesta::Alzati { posto: 1 });
    assert_eq!(
        r.rifiuti[&c].last().unwrap(),
        "Non puoi liberare questo posto"
    );
    r.richiedi(a, Richiesta::Alzati { posto: 1 }); // the creator can
    r.richiedi(c, Richiesta::Avvia);
    assert_eq!(
        r.rifiuti[&c].last().unwrap(),
        "Servono 4 giocatori (umani o bot)"
    );
    let t = r.tavolo(b);
    assert_eq!(
        t.partecipanti
            .iter()
            .map(|p| p.nome.as_str())
            .collect::<Vec<_>>(),
        ["A", "B", "C"]
    );
    assert!(t.posti.iter().all(Option::is_none));
    r.assert_sincronizzati();
    assert!(r.nodo(a).e_host());
}

#[test]
fn partita_completa_tra_quattro_peer() {
    for variante in [Variante::Vanilla, Variante::Heisenberg, Variante::Avogadro] {
        let (mut r, _, _, _) = tavolo_tipico(variante);
        r.gioca_fino(u64::MAX);
        r.assert_sincronizzati();
        let t = r.tavolo(r.host_h());
        assert_eq!(t.stato, StatoTavolo::Finita);
        assert_eq!(t.partita.as_ref().unwrap().fase, Fase::FinePartita);
        // bots moved and no human seat was ever played by someone else
        assert!(
            r.voci
                .iter()
                .any(|v| matches!(v, Voce::Azione(Azione::Gioca { seat: 1, .. })))
        );
        assert!(r.rifiuti.is_empty(), "{:?}", r.rifiuti);
        // snapshots must fit in one WebRTC message (Safari: 64 KiB to be safe)
        let istantanea = Messaggio::Istantanea {
            epoca: 0,
            host: "a".into(),
            tavolo: Box::new(t.clone()),
        };
        let dim = istantanea.codifica().len();
        println!("{variante:?}: snapshot {dim} bytes");
        assert!(dim < 64 * 1024, "snapshot too large: {dim} bytes");
    }
}

#[test]
fn solo_il_proprietario_gioca_il_suo_posto() {
    let (mut r, a, b, _) = tavolo_tipico(Variante::Vanilla);
    let st = r.tavolo(a).partita.clone().unwrap();
    let mossa = st.legal_actions(st.corrente).into_iter().next().unwrap();
    let altro = if st.corrente == 0 { b } else { a };
    r.richiedi(altro, Richiesta::Azione(mossa));
    assert_eq!(r.rifiuti[&altro], ["Non controlli questo posto"]);
    r.rifiuti.clear();
    r.richiedi(a, Richiesta::Azione(Azione::NuovaMano));
    assert_eq!(r.rifiuti[&a], ["Azione riservata al sistema"]);
}

#[test]
fn il_timer_gioca_per_chi_non_risponde() {
    let (mut r, a, _, _) = tavolo_tipico(Variante::Vanilla);
    // nobody answers: bots move, then the human whose turn it is times out
    for _ in 0..20 {
        r.avanza(60_000);
    }
    let scaduti = r
        .voci
        .iter()
        .filter(|v| {
            matches!(
                v,
                Voce::Scaduto {
                    timer: TipoTimer::Giocata,
                    ..
                }
            )
        })
        .count();
    assert!(scaduti >= 2, "only {scaduti} timeouts");
    r.assert_sincronizzati();
    assert!(r.nodo(a).e_host());
}

#[test]
fn timer_dello_scambio() {
    let (mut r, _, _, _) = tavolo_tipico(Variante::Heisenberg);
    assert_eq!(r.tavolo(1).partita.as_ref().unwrap().fase, Fase::Scambio);
    let scaduto = |r: &Rete| {
        r.voci.iter().any(|v| {
            matches!(
                v,
                Voce::Scaduto {
                    timer: TipoTimer::Scambio,
                    ..
                }
            )
        })
    };
    while !scaduto(&r) {
        assert!(r.ora < 100_000, "exchange timer never fired");
        r.avanza(31_000);
    }
    assert_eq!(r.tavolo(1).partita.as_ref().unwrap().fase, Fase::Gioco);
    r.assert_sincronizzati();
}

#[test]
fn senza_timer_si_aspetta() {
    let mut r = Rete::new();
    let a = r.crea(
        "a",
        Impostazioni {
            timer_giocata: 0,
            ..imp(Variante::Vanilla)
        },
    );
    r.richiedi(a, Richiesta::Siedi { posto: 0 });
    for s in 1..4 {
        r.richiedi(
            a,
            Richiesta::Bot {
                posto: s,
                aggiungi: true,
            },
        );
    }
    r.richiedi(a, Richiesta::Avvia);
    for _ in 0..50 {
        r.avanza(3_600_000);
    }
    let st = r.tavolo(a).partita.as_ref().unwrap();
    assert_eq!((st.fase, st.corrente), (Fase::Gioco, 0));
    assert!(r.nodo(a).prossima_sveglia().is_none());
}

#[test]
fn partita_solo_bot_osservata() {
    let mut r = Rete::new();
    let a = r.crea("a", imp(Variante::FedericoII));
    let _b = r.unisci("b");
    for s in 0..4 {
        r.richiedi(
            a,
            Richiesta::Bot {
                posto: s,
                aggiungi: true,
            },
        );
    }
    r.richiedi(a, Richiesta::Avvia);
    r.gioca_fino(u64::MAX);
    r.assert_sincronizzati();
    assert_eq!(r.tavolo(a).stato, StatoTavolo::Finita);
}

/// Simulated time a bot-only game takes at a given bot pace.
fn durata_partita_bot(ritmo: Ritmo) -> u64 {
    let mut r = Rete::new();
    let a = r.crea(
        "a",
        Impostazioni {
            ritmo,
            ..imp(Variante::Vanilla)
        },
    );
    for s in 0..4 {
        r.richiedi(
            a,
            Richiesta::Bot {
                posto: s,
                aggiungi: true,
            },
        );
    }
    let inizio = r.ora;
    r.richiedi(a, Richiesta::Avvia);
    r.gioca_fino(u64::MAX);
    assert_eq!(r.tavolo(a).stato, StatoTavolo::Finita);
    r.ora - inizio
}

#[test]
fn il_ritmo_rallenta_o_accelera_i_bot() {
    let [calmo, normale, svelto] = Ritmo::TUTTI.map(durata_partita_bot);
    assert!(
        calmo > normale && normale > svelto,
        "{calmo} {normale} {svelto}"
    );
}

#[test]
fn versione_diversa_viene_segnalata() {
    let mut n: Nodo<H> = Nodo::unisciti(io("a"), 1, config());
    n.connesso(0, 7);
    n.ricevi(
        0,
        7,
        Messaggio::Ciao {
            versione: VERSIONE_PROTOCOLLO + 1,
            io: io("b"),
            replica: true,
        },
    );
    assert_eq!(n.versione_altrui(), Some(VERSIONE_PROTOCOLLO + 1));
    assert!(!n.connessi().contains("b"));
    assert!(n.tavolo().is_none());
}

#[test]
fn messaggio_perso_si_recupera() {
    let (mut r, _, b, c) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(30);
    r.da_perdere
        .push((c, |m| matches!(m, Messaggio::Voce { .. })));
    r.da_perdere
        .push((b, |m| matches!(m, Messaggio::Voce { .. })));
    r.gioca_fino(60);
    r.assert_sincronizzati();
    assert!(r.da_perdere.is_empty());
    r.gioca_fino(u64::MAX);
    r.assert_sincronizzati();
}

#[test]
fn impronta_diversa_forza_istantanea() {
    let (mut r, a, _, c) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(20);
    // forge an entry with a wrong hash, as if c's replica had drifted
    let st = r.tavolo(c).partita.clone().unwrap();
    let t = r.tavolo(c).clone();
    let voce = Voce::Azione(st.legal_actions(st.corrente).remove(0));
    let msg = Messaggio::Voce {
        epoca: r.nodo(c).epoca(),
        n: t.n + 1,
        voce,
        impronta: "deadbeefdeadbeef".into(),
    };
    r.coda.push_back((a, c, msg.codifica()));
    r.pompa();
    r.assert_sincronizzati();
    assert_eq!(r.tavolo(c).n, t.n);
}

#[test]
fn chi_arriva_tardi_riceve_lo_stato() {
    let (mut r, _, _, _) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(40);
    let d = r.unisci("d");
    r.assert_sincronizzati();
    assert!(r.tavolo(d).partecipante("d").is_some());
    r.gioca_fino(u64::MAX);
    r.assert_sincronizzati();
}

#[test]
fn host_seduto_esce_partita_in_pausa_poi_rientra() {
    let (mut r, a, b, c) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(40);
    let n = r.tavolo(b).n;
    r.esci(a);
    // b is next in join order and takes over; a's seat is empty, so the game pauses
    assert!(r.nodo(b).e_host());
    assert_eq!(r.nodo(b).epoca(), 1);
    assert_eq!(r.nodo(c).host().map(String::as_str), Some("b"));
    assert!(r.nodo(b).in_pausa());
    for _ in 0..10 {
        r.avanza(120_000);
    }
    assert_eq!(r.tavolo(b).n, n, "nothing happens while paused");
    r.richiedi(b, Richiesta::Prosegui);
    // a comes back (new connection, same identity) and gets the replica
    let a2 = r.unisci("a");
    r.assert_sincronizzati();
    assert!(!r.nodo(a2).e_host() && r.nodo(b).e_host());
    assert!(!r.nodo(b).in_pausa());
    r.gioca_fino(u64::MAX);
    r.assert_sincronizzati();
    assert_eq!(r.tavolo(a2).stato, StatoTavolo::Finita);
}

#[test]
fn host_non_seduto_esce_e_i_bot_continuano() {
    let mut r = Rete::new();
    let a = r.crea("a", imp(Variante::Vanilla));
    let b = r.unisci("b");
    let c = r.unisci("c");
    r.richiedi(b, Richiesta::Siedi { posto: 0 });
    r.richiedi(c, Richiesta::Siedi { posto: 1 });
    r.richiedi(
        a,
        Richiesta::Bot {
            posto: 2,
            aggiungi: true,
        },
    );
    r.richiedi(
        a,
        Richiesta::Bot {
            posto: 3,
            aggiungi: true,
        },
    );
    r.richiedi(a, Richiesta::Avvia);
    r.gioca_fino(30);
    r.esci(a);
    assert!(r.nodo(b).e_host() && !r.nodo(b).in_pausa());
    r.gioca_fino(u64::MAX);
    r.assert_sincronizzati();
    assert_eq!(r.tavolo(c).stato, StatoTavolo::Finita);
}

#[test]
fn nuovo_host_in_ritardo_recupera_da_chi_e_avanti() {
    let (mut r, a, b, c) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(30);
    // b misses the last entry from a, then a leaves before b catches up
    r.da_perdere
        .push((b, |m| matches!(m, Messaggio::Voce { .. })));
    r.richiedi(c, Richiesta::Prosegui); // harmless no-op request to keep traffic flowing
    r.gioca_fino(31);
    let n_c = r.tavolo(c).n;
    let n_b = r.tavolo(b).n;
    assert!(n_b <= n_c);
    r.esci(a);
    assert!(r.nodo(b).e_host());
    assert_eq!(
        r.tavolo(b).n,
        n_c,
        "the new host adopted c's longer replica"
    );
    r.assert_sincronizzati();
}

#[test]
fn effimeri() {
    let (mut r, a, b, c) = tavolo_tipico(Variante::Vanilla);
    let ora = r.ora;
    let frase = Effimero::Frase { seat: 0, indice: 1 };
    // b does not control seat 0
    assert!(
        r.nodi
            .get_mut(&b)
            .unwrap()
            .effimero(ora, frase.clone())
            .is_err()
    );
    r.nodi
        .get_mut(&a)
        .unwrap()
        .effimero(ora, frase.clone())
        .unwrap();
    r.pompa();
    for h in [a, b, c] {
        assert_eq!(r.effimeri[&h], [("a".to_string(), frase.clone())]);
    }
    // a forged effect for someone else's seat is dropped by the receivers
    let falso = Messaggio::Effimero(Effimero::Frase { seat: 2, indice: 0 });
    r.coda.push_back((a, c, falso.codifica()));
    r.pompa();
    assert_eq!(r.effimeri[&c].len(), 1);
}

/// A, the host, only watches; B and C play with two bots.
fn tavolo_host_spettatore() -> (Rete, H, H, H) {
    let mut r = Rete::new();
    let a = r.crea("a", imp(Variante::Vanilla));
    let b = r.unisci("b");
    let c = r.unisci("c");
    r.richiedi(b, Richiesta::Siedi { posto: 0 });
    r.richiedi(c, Richiesta::Siedi { posto: 1 });
    r.richiedi(
        a,
        Richiesta::Bot {
            posto: 2,
            aggiungi: true,
        },
    );
    r.richiedi(
        a,
        Richiesta::Bot {
            posto: 3,
            aggiungi: true,
        },
    );
    r.richiedi(a, Richiesta::Avvia);
    (r, a, b, c)
}

#[test]
fn spettatore_che_dorme_non_si_proclama_host() {
    let (mut r, a, _, _) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(20);
    let d = r.unisci("d");
    let dorme = r.isola(d);
    assert!(
        dorme.host().is_none(),
        "an isolated peer must not claim the host role"
    );
    r.gioca_fino(60);
    let d = r.entra(dorme);
    r.assert_sincronizzati();
    assert!(r.nodo(a).e_host() && !r.nodo(d).e_host());
    assert_eq!(r.nodo(a).epoca(), 0);
}

#[test]
fn host_che_dorme_non_impone_lo_stato_vecchio() {
    let (mut r, a, b, _) = tavolo_host_spettatore();
    r.gioca_fino(20);
    let dorme = r.isola(a);
    assert!(r.nodo(b).e_host());
    r.gioca_fino(80);
    let n = r.tavolo(b).n;
    let a = r.entra(dorme);
    r.assert_sincronizzati();
    assert!(r.nodo(b).e_host() && !r.nodo(a).e_host());
    assert_eq!(
        r.tavolo(a).n,
        n,
        "the stale replica of the old host was not adopted"
    );
    r.gioca_fino(u64::MAX);
    r.assert_sincronizzati();
}

#[test]
fn giocatore_che_dorme_mette_in_pausa_e_poi_riprende() {
    let (mut r, a, b, _) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(20);
    let n = r.tavolo(a).n;
    let dorme = r.isola(b);
    for _ in 0..10 {
        r.avanza(60_000);
    }
    assert!(r.nodo(a).in_pausa());
    assert_eq!(r.tavolo(a).n, n);
    r.entra(dorme);
    r.assert_sincronizzati();
    assert!(!r.nodo(a).in_pausa());
    r.gioca_fino(u64::MAX);
    r.assert_sincronizzati();
}

#[test]
fn rimasto_solo_poi_qualcuno_rientra() {
    // everyone but c leaves; c keeps the table and hands it to whoever comes back
    let (mut r, a, b, c) = tavolo_host_spettatore();
    r.gioca_fino(20);
    let n = r.tavolo(c).n;
    r.esci(a);
    r.esci(b);
    assert!(r.nodo(c).host().is_none());
    let b2 = r.unisci("b");
    r.assert_sincronizzati();
    assert!(r.nodo(c).e_host());
    assert_eq!(r.tavolo(b2).n, n, "b got c's replica");
}
