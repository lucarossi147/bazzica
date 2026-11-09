//! Several peers on a simulated network in one process.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bazzica_core::bots::{Agent, HeuristicAgent};
use bazzica_core::{Azione, Fase, Variante};
use bazzica_net::*;

type H = u32;
type Filtro = fn(&Messaggio) -> bool;

/// Test network. Messages go through the JSON codec and are delivered in order, only along
/// links (a full mesh unless built otherwise with [`Rete::entra_con`] / [`Rete::scollega_coppia`]).
struct Rete {
    nodi: BTreeMap<H, Nodo<H>>,
    prossimo: H,
    /// Open links, as `(lower, higher)` handle pairs.
    collegamenti: BTreeSet<(H, H)>,
    coda: VecDeque<(H, H, Vec<u8>)>,
    ora: u64,
    rifiuti: BTreeMap<H, Vec<String>>,
    voci: Vec<Voce>,
    effimeri: BTreeMap<H, Vec<(IdGiocatore, Effimero)>>,
    /// Drop the next message matching (from, if given; to; predicate).
    da_perdere: Vec<(Option<H>, H, Filtro)>,
    /// Every message sent so far, for counting (from, to, message).
    inviati: Vec<(H, H, Messaggio)>,
}

fn coppia(a: H, b: H) -> (H, H) {
    (a.min(b), a.max(b))
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
        attesa_istantanea_ms: 500,
        attesa_allineamento_ms: 1000,
        attesa_richiesta_ms: 1000,
        attesa_sostituzione_ms: 60_000,
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
            inviati: vec![],
            collegamenti: BTreeSet::new(),
        }
    }

    /// Adds a node and connects it to everyone already on the network.
    fn entra(&mut self, nodo: Nodo<H>) -> H {
        self.entra_con(nodo, None)
    }

    /// Adds a node connected to `con` only (everyone when `None`).
    fn entra_con(&mut self, nodo: Nodo<H>, con: Option<&[H]>) -> H {
        let h = self.prossimo;
        self.prossimo += 1;
        let altri: Vec<H> = match con {
            Some(c) => c.to_vec(),
            None => self.nodi.keys().copied().collect(),
        };
        self.nodi.insert(h, nodo);
        for a in altri {
            self.collega_senza_pompare(a, h);
        }
        self.pompa();
        h
    }

    /// Adds several nodes at once: all links open before any message is delivered.
    fn entrano_insieme(&mut self, nodi: Vec<Nodo<H>>) -> Vec<H> {
        let nuovi: Vec<H> = nodi
            .into_iter()
            .map(|n| {
                let h = self.prossimo;
                self.prossimo += 1;
                self.nodi.insert(h, n);
                h
            })
            .collect();
        let tutti: Vec<H> = self.nodi.keys().copied().collect();
        for &h in &nuovi {
            for &a in &tutti {
                if a != h {
                    self.collega_senza_pompare(a, h);
                }
            }
        }
        self.pompa();
        nuovi
    }

    fn collega_senza_pompare(&mut self, a: H, b: H) {
        if self.collegamenti.insert(coppia(a, b)) {
            self.nodi.get_mut(&a).unwrap().connesso(self.ora, b);
            self.nodi.get_mut(&b).unwrap().connesso(self.ora, a);
        }
    }

    /// Opens the link a–b (both nodes see a new connection).
    fn collega(&mut self, a: H, b: H) {
        self.collega_senza_pompare(a, b);
        self.pompa();
    }

    /// Cuts the link a–b only (partial mesh); queued messages on it are lost.
    fn scollega_coppia(&mut self, a: H, b: H) {
        if !self.collegamenti.remove(&coppia(a, b)) {
            return;
        }
        self.coda.retain(|(x, y, _)| coppia(*x, *y) != coppia(a, b));
        self.nodi.get_mut(&a).unwrap().disconnesso(self.ora, &b);
        self.nodi.get_mut(&b).unwrap().disconnesso(self.ora, &a);
        self.pompa();
    }

    fn collegati(&self, a: H, b: H) -> bool {
        self.collegamenti.contains(&coppia(a, b))
    }

    /// Drops the next message from `da` to `a` that matches `filtro`.
    fn perdi(&mut self, da: H, a: H, filtro: Filtro) {
        self.da_perdere.push((Some(da), a, filtro));
    }

    /// Drops the next message to `a`, from anyone, that matches `filtro`.
    fn perdi_verso(&mut self, a: H, filtro: Filtro) {
        self.da_perdere.push((None, a, filtro));
    }

    fn crea(&mut self, id: &str, imp: Impostazioni) -> H {
        self.entra(Nodo::crea(io(id), imp, 7, config()))
    }

    fn unisci(&mut self, id: &str) -> H {
        let seme = id.bytes().map(u64::from).sum::<u64>() + 1000 * u64::from(self.prossimo);
        self.entra(Nodo::unisciti(io(id), seme, config()))
    }

    /// Removes `h` and its links; returns the handles it was linked to.
    fn stacca(&mut self, h: H) -> Vec<H> {
        let vicini: Vec<H> = self
            .collegamenti
            .iter()
            .filter_map(|&(x, y)| (x == h).then_some(y).or((y == h).then_some(x)))
            .collect();
        self.collegamenti.retain(|&(x, y)| x != h && y != h);
        self.coda.retain(|(a, b, _)| *a != h && *b != h);
        vicini
    }

    fn esci(&mut self, h: H) {
        self.nodi.remove(&h);
        for v in self.stacca(h) {
            self.nodi.get_mut(&v).unwrap().disconnesso(self.ora, &h);
        }
        self.pompa();
    }

    /// Cuts every connection of `h` (like a phone going to sleep) and hands back its node.
    fn isola(&mut self, h: H) -> Nodo<H> {
        let mut n = self.nodi.remove(&h).unwrap();
        for v in self.stacca(h) {
            self.nodi.get_mut(&v).unwrap().disconnesso(self.ora, &h);
            n.disconnesso(self.ora, &v);
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

    /// Moves every node's output to the wire queue (or the test's records). True if any.
    fn raccogli(&mut self) -> bool {
        let mut mosso = false;
        let handles: Vec<H> = self.nodi.keys().copied().collect();
        for h in handles {
            for u in self.nodi.get_mut(&h).unwrap().uscite() {
                mosso = true;
                match u {
                    Uscita::Invia { a, msg } => {
                        let dest: Vec<H> = match a {
                            Destinatario::Tutti => self
                                .nodi
                                .keys()
                                .copied()
                                .filter(|x| *x != h && self.collegati(h, *x))
                                .collect(),
                            // like matchbox: a packet for a peer we are not linked to is dropped
                            Destinatario::Uno(p) => {
                                self.collegati(h, p).then_some(p).into_iter().collect()
                            }
                        };
                        for d in dest {
                            if let Some(i) = self.da_perdere.iter().position(|(s, t, f)| {
                                s.is_none_or(|s| s == h) && *t == d && f(&msg)
                            }) {
                                self.da_perdere.remove(i);
                                continue;
                            }
                            self.inviati.push((h, d, msg.clone()));
                            self.coda.push_back((h, d, msg.codifica()));
                        }
                    }
                    Uscita::Rifiutata { motivo } => self.rifiuti.entry(h).or_default().push(motivo),
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
        mosso
    }

    /// Collects pending output, then delivers exactly one queued message. False if none.
    fn consegna_uno(&mut self) -> bool {
        self.raccogli();
        let Some((da, a, b)) = self.coda.pop_front() else {
            return false;
        };
        let msg = Messaggio::decodifica(&b).expect("wire round-trip");
        if let Some(n) = self.nodi.get_mut(&a) {
            n.ricevi(self.ora, da, msg);
        }
        true
    }

    /// Delivers everything until the network is quiet. I1 is checked after every message,
    /// I2 once the network is quiet.
    fn pompa(&mut self) {
        while self.raccogli() | self.consegna_uno() {
            for n in self.nodi.values() {
                n.verifica_invarianti().unwrap();
            }
        }
        self.verifica_invarianti();
    }

    /// I1 on every node, and I2: no two nodes host the same epoch.
    fn verifica_invarianti(&self) {
        for n in self.nodi.values() {
            n.verifica_invarianti().unwrap();
        }
        let mut regie = BTreeSet::new();
        for n in self.nodi.values().filter(|n| n.e_host()) {
            assert!(
                regie.insert(n.epoca()),
                "I2: two hosts at epoch {}",
                n.epoca()
            );
        }
    }

    /// Advances the clock by `ms` and ticks `h` alone, delivering nothing (its output stays
    /// queued in the node until the next `pompa`).
    fn tick_solo(&mut self, h: H, ms: u64) {
        self.ora += ms;
        let ora = self.ora;
        self.nodi.get_mut(&h).unwrap().tick(ora);
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

    /// Nodes still acting (a tab superseded by a newer one of the same identity is frozen).
    fn attivi(&self) -> impl Iterator<Item = &Nodo<H>> {
        self.nodi.values().filter(|n| !n.sostituito())
    }

    fn assert_sincronizzati(&self) {
        let impronte: BTreeSet<(u64, String)> = self
            .attivi()
            .map(|n| {
                let t = n.tavolo().expect("replica");
                (t.n, t.impronta())
            })
            .collect();
        assert_eq!(impronte.len(), 1, "replicas differ: {impronte:?}");
        let host: BTreeSet<_> = self.attivi().map(|n| n.host().cloned()).collect();
        assert_eq!(host.len(), 1, "peers disagree on the host: {host:?}");
    }

    /// Humans on peer `h` play one move if it is their turn (like the UI would).
    fn gioca_umani(&mut self, h: H, ack: bool) -> bool {
        let n = self.nodo(h);
        let t = n.tavolo().unwrap();
        if t.stato != StatoTavolo::InCorso || n.in_pausa() || n.sostituito() {
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
            host: Regia {
                id: "a".into(),
                sessione: Sessione::default(),
            },
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
            sessione: Sessione::default(),
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
    r.perdi_verso(c, |m| matches!(m, Messaggio::Voce { .. }));
    r.perdi_verso(b, |m| matches!(m, Messaggio::Voce { .. }));
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
    // the mismatching entry is never committed: c stays where it was until the snapshot
    assert!(r.consegna_uno());
    assert_eq!(r.tavolo(c), &t, "the replica moved before the hash check");
    r.nodo(c).verifica_invarianti().unwrap();
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
    r.perdi_verso(b, |m| matches!(m, Messaggio::Voce { .. }));
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

#[test]
fn strumenti_della_rete() {
    let mut r = Rete::new();
    let a = r.crea("a", imp(Variante::Vanilla));
    let b = r.entra_con(Nodo::unisciti(io("b"), 2, config()), Some(&[a]));
    let c = r.entra_con(Nodo::unisciti(io("c"), 3, config()), Some(&[a]));
    assert!(!r.collegati(b, c), "partial mesh: b and c only reach a");
    r.assert_sincronizzati();

    // a loss on one directed link only: c falls behind, then catches up on the next entry
    r.perdi(a, c, |m| matches!(m, Messaggio::Voce { .. }));
    r.richiedi(b, Richiesta::Siedi { posto: 0 });
    assert_eq!(r.tavolo(c).n + 1, r.tavolo(b).n);
    r.richiedi(b, Richiesta::Siedi { posto: 1 });
    r.assert_sincronizzati();

    // one message at a time: b's request reaches a, a's entry is still on the wire
    let (n, ora) = (r.tavolo(a).n, r.ora);
    r.nodi
        .get_mut(&b)
        .unwrap()
        .richiedi(ora, Richiesta::Siedi { posto: 2 });
    assert!(r.consegna_uno());
    assert_eq!((r.tavolo(a).n, r.tavolo(b).n), (n + 1, n));
    r.pompa();
    r.assert_sincronizzati();

    // a node ticks alone: nothing is delivered
    r.tick_solo(a, 5);
    assert!(r.coda.is_empty() && !r.consegna_uno());

    // cutting and reopening one link
    r.scollega_coppia(a, b);
    assert!(r.nodo(b).host().is_none(), "b reaches nobody");
    r.collega(a, b);
    r.assert_sincronizzati();
    assert!(r.inviati.iter().any(|(_, d, _)| *d == c));
}

/// Makes `c`'s replica differ from the host's at the same `n`, by handing it a doctored
/// snapshot as if it came from the host. Returns the host's handle.
fn devia(r: &mut Rete, c: H) -> H {
    let h = r.host_h();
    let mut falso = r.tavolo(c).clone();
    falso.impostazioni.nome = "Tavolo falso".into();
    let msg = Messaggio::Istantanea {
        epoca: r.nodo(c).epoca(),
        host: r.nodo(c).host_regia().unwrap().clone(),
        tavolo: Box::new(falso),
    };
    r.coda.push_back((h, c, msg.codifica()));
    r.pompa();
    assert_ne!(r.tavolo(c).impronta(), r.tavolo(h).impronta());
    h
}

/// Plays and advances time until everyone agrees (or gives up after `giri` rounds).
fn fino_a_sincronizzati(r: &mut Rete, giri: usize) {
    for _ in 0..giri {
        let impronte: BTreeSet<_> = r
            .nodi
            .values()
            .map(|n| n.tavolo().map(Tavolo::impronta))
            .collect();
        if impronte.len() == 1 {
            break;
        }
        let handles: Vec<H> = r.nodi.keys().copied().collect();
        if !handles
            .into_iter()
            .fold(false, |m, h| r.gioca_umani(h, true) | m)
        {
            r.avanza(10_000);
        }
    }
    r.assert_sincronizzati();
}

#[test]
fn richiesta_istantanea_persa_si_ripete() {
    let (mut r, a, _, c) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(20);
    devia(&mut r, c);
    // c notices on the next entry and asks for a snapshot, which is lost
    r.perdi(a, c, |m| matches!(m, Messaggio::Istantanea { .. }));
    let n = r.tavolo(a).n;
    r.gioca_fino(n + 1);
    assert!(
        r.da_perdere.is_empty(),
        "the snapshot was requested and lost"
    );
    fino_a_sincronizzati(&mut r, 50);
    r.gioca_fino(u64::MAX);
    r.assert_sincronizzati();
}

#[test]
fn host_esce_mentre_chiedo_istantanea() {
    let (mut r, a, b, c) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(20);
    devia(&mut r, c);
    // the next entry reaches c, whose snapshot request is still on the wire when a leaves
    r.perdi(c, a, |m| matches!(m, Messaggio::ChiediDa { da: 0 }));
    let n = r.tavolo(a).n;
    r.gioca_fino(n + 1);
    assert!(r.da_perdere.is_empty());
    r.esci(a);
    assert!(r.nodo(b).e_host());
    fino_a_sincronizzati(&mut r, 50);
}

#[test]
fn chiedi_da_a_chi_non_e_host() {
    let (mut r, a, b, c) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(20);
    devia(&mut r, c);
    // a follower gets a snapshot request: it does not answer, and c still gets in sync
    r.coda
        .push_back((c, b, Messaggio::ChiediDa { da: 0 }.codifica()));
    r.perdi(c, a, |m| matches!(m, Messaggio::ChiediDa { da: 0 }));
    let n = r.tavolo(a).n;
    r.gioca_fino(n + 1);
    assert!(
        !r.inviati
            .iter()
            .any(|(da, a2, m)| *da == b && *a2 == c && matches!(m, Messaggio::Istantanea { .. }))
    );
    fino_a_sincronizzati(&mut r, 50);
}

/// A hosts and watches with B and C; four bots play, so a bot move is always due.
fn tavolo_tutti_bot() -> (Rete, H, H, H) {
    let mut r = Rete::new();
    let a = r.crea("a", imp(Variante::Vanilla));
    let b = r.unisci("b");
    let c = r.unisci("c");
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
    (r, a, b, c)
}

/// `h` leaves; the others notice, but nothing is delivered yet.
fn esci_senza_pompare(r: &mut Rete, h: H) {
    r.nodi.remove(&h);
    let ora = r.ora;
    for v in r.stacca(h) {
        r.nodi.get_mut(&v).unwrap().disconnesso(ora, &h);
    }
}

#[test]
fn voce_duplicata_diversa_forza_resync() {
    let (mut r, a, _, c) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(20);
    devia(&mut r, c);
    // the host repeats its last entry: c holds a different state at that position
    let t = r.tavolo(a).clone();
    let msg = Messaggio::Voce {
        epoca: r.nodo(a).epoca(),
        n: t.n,
        voce: r.voci.last().unwrap().clone(),
        impronta: t.impronta(),
    };
    r.coda.push_back((a, c, msg.codifica()));
    r.pompa();
    r.assert_sincronizzati();
    assert_eq!(r.tavolo(c).n, t.n);
}

#[test]
fn nuovo_host_non_agisce_prima_di_allinearsi() {
    let (mut r, a, b, c) = tavolo_tutti_bot();
    r.gioca_fino(30);
    // b misses a's next entry, c applies it
    r.perdi(a, b, |m| matches!(m, Messaggio::Voce { .. }));
    r.avanza(10_000);
    let (n_c, impronta_c) = (r.tavolo(c).n, r.tavolo(c).impronta());
    assert_eq!(r.tavolo(b).n + 1, n_c);
    // a leaves; b claims and ticks past the bot delay before hearing from anyone
    esci_senza_pompare(&mut r, a);
    r.tick_solo(b, 10_000);
    assert_eq!(
        r.tavolo(b).n + 1,
        n_c,
        "the new host sequenced before aligning"
    );
    r.pompa();
    r.assert_sincronizzati();
    assert!(r.nodo(b).e_host());
    assert_eq!(
        (r.tavolo(b).n, r.tavolo(b).impronta()),
        (n_c, impronta_c),
        "c's entry was kept"
    );
    r.gioca_fino(u64::MAX);
    r.assert_sincronizzati();
}
#[test]
fn allineamento_scade() {
    let (mut r, a, b, c) = tavolo_tutti_bot();
    r.gioca_fino(30);
    let n = r.tavolo(b).n;
    // c's answer to the claim is lost: b waits for it, but not forever
    r.perdi(c, b, |m| matches!(m, Messaggio::Allineato { .. }));
    r.esci(a);
    assert!(r.nodo(b).e_host());
    assert!(
        r.da_perdere.is_empty(),
        "c answered and the answer was lost"
    );
    r.tick_solo(b, config().attesa_allineamento_ms - 1);
    assert_eq!(r.tavolo(b).n, n, "still aligning");
    r.gioca_fino(n + 5);
    assert!(r.tavolo(b).n >= n + 5, "the host resumed after the timeout");
    r.gioca_fino(u64::MAX);
    r.assert_sincronizzati();
}

fn quanti_host(r: &Rete) -> usize {
    r.nodi.values().filter(|n| n.e_host()).count()
}

#[test]
fn stessa_identita_due_schede() {
    let (mut r, a, _, _) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(20);
    // a second tab of a opens the same table
    let a2 = r.unisci("a");
    assert_eq!(quanti_host(&r), 1);
    assert!(
        !r.nodo(a).e_host() && !r.nodo(a2).e_host(),
        "a host by id, not by election"
    );
    r.assert_sincronizzati();
    r.gioca_fino(u64::MAX);
    r.assert_sincronizzati();
    assert_eq!(r.tavolo(a2).stato, StatoTavolo::Finita);
}

#[test]
fn host_ricarica_la_pagina() {
    let (mut r, a, b, _) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(20);
    let epoca = r.nodo(b).epoca();
    // the host reloads: the new page connects before the old link times out
    let a2 = r.unisci("a");
    assert_eq!(quanti_host(&r), 1);
    assert!(
        r.nodo(b).e_host() && r.nodo(b).epoca() > epoca,
        "taken over by election"
    );
    r.esci(a);
    assert_eq!(quanti_host(&r), 1);
    r.assert_sincronizzati();
    r.gioca_fino(u64::MAX);
    r.assert_sincronizzati();
    assert!(!r.nodo(a2).e_host());
}

#[test]
fn richieste_alla_sessione_giusta() {
    let (mut r, a, _, _) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(20);
    let a2 = r.unisci("a");
    assert!(r.nodo(a).sostituito() && !r.nodo(a2).sostituito());
    let (host, prima) = (r.host_h(), r.inviati.len());
    let n = r.tavolo(host).n;
    // b.s and a2.s requests reach the live host, never the superseded tab
    r.gioca_fino(n + 10);
    assert!(r.tavolo(host).n >= n + 10);
    assert!(r.rifiuti.is_empty(), "{:?}", r.rifiuti);
    let al_vecchio = r.inviati[prima..]
        .iter()
        .filter(|(_, d, m)| *d == a && matches!(m, Messaggio::Richiesta { .. }))
        .count();
    assert_eq!(al_vecchio, 0, "requests sent to the superseded tab");
    // the superseded tab refuses to act
    r.richiedi(a, Richiesta::Prosegui);
    assert_eq!(
        r.rifiuti[&a],
        ["Questa partita è aperta in un'altra scheda"]
    );
}

fn quanti_host_dopo(r: &Rete, dal: usize) -> usize {
    r.inviati[dal..]
        .iter()
        .filter_map(|(da, _, m)| matches!(m, Messaggio::Host { .. }).then_some(*da))
        .collect::<BTreeSet<_>>()
        .len()
}

#[test]
fn entrano_insieme_poi_host_esce() {
    let mut r = Rete::new();
    let a = r.crea("a", imp(Variante::Vanilla));
    // b and c scan the code together: they meet before either has the replica
    let bc = r.entrano_insieme(vec![
        Nodo::unisciti(io("b"), 2, config()),
        Nodo::unisciti(io("c"), 3, config()),
    ]);
    let (b, c) = (bc[0], bc[1]);
    r.assert_sincronizzati();
    assert!(
        r.nodo(c).peer_con_replica().contains("b"),
        "c knows b holds a replica"
    );
    assert!(r.nodo(b).peer_con_replica().contains("c"));
    let prima = r.inviati.len();
    r.esci(a);
    assert_eq!(quanti_host_dopo(&r, prima), 1, "only b claims");
    assert!(r.nodo(b).e_host() && !r.nodo(c).e_host());
    r.assert_sincronizzati();
}

/// Four at the table, the host a, all seated with the default timers.
fn quattro_umani() -> (Rete, [H; 4]) {
    let mut r = Rete::new();
    let a = r.crea("a", imp(Variante::Vanilla));
    let b = r.unisci("b");
    let c = r.unisci("c");
    let d = r.unisci("d");
    for (h, s) in [(a, 0), (b, 1), (c, 2), (d, 3)] {
        r.richiedi(h, Richiesta::Siedi { posto: s });
    }
    r.richiedi(a, Richiesta::Avvia);
    (r, [a, b, c, d])
}

#[test]
fn rete_parziale_nessuno_split_brain() {
    let (mut r, [a, b, c, d]) = quattro_umani();
    r.gioca_fino(10);
    r.scollega_coppia(b, c);
    let n = r.tavolo(b).n;
    r.esci(a);
    // b and c cannot see each other: at most one of them may sequence
    assert!(quanti_host(&r) <= 1);
    for _ in 0..5 {
        r.avanza(60_000);
    }
    let sequenziano: Vec<H> = [b, c, d]
        .into_iter()
        .filter(|&h| r.tavolo(h).n > n)
        .collect();
    assert!(sequenziano.len() <= 1, "split brain: {sequenziano:?}");
}

#[test]
fn partizione_due_a_due() {
    let (mut r, [a, b, c, d]) = quattro_umani();
    r.gioca_fino(10);
    for (x, y) in [(a, c), (a, d), (b, c), (b, d)] {
        r.scollega_coppia(x, y);
    }
    assert!(r.nodo(a).e_host());
    assert!(
        r.nodo(c).host().is_none() && r.nodo(d).host().is_none(),
        "the minority side does not elect"
    );
    let n = r.tavolo(c).n;
    for _ in 0..5 {
        r.avanza(60_000);
    }
    assert_eq!(r.tavolo(c).n, n);
    assert_eq!(r.tavolo(d).n, n);
}

#[test]
fn richiesta_al_vecchio_host_viene_rispedita() {
    let mut r = Rete::new();
    let a = r.crea("a", imp(Variante::Vanilla));
    let b = r.unisci("b");
    let c = r.unisci("c");
    // c's request is lost on the way to a, then a leaves and b takes over
    r.perdi(c, a, |m| matches!(m, Messaggio::Richiesta { .. }));
    r.richiedi(c, Richiesta::Siedi { posto: 1 });
    assert!(r.da_perdere.is_empty());
    r.esci(a);
    assert!(r.nodo(b).e_host());
    for _ in 0..5 {
        r.avanza(5_000);
    }
    assert_eq!(r.tavolo(b).posti_di("c"), [1], "applied by the new host");
    let siedi = r
        .voci
        .iter()
        .filter(|v| matches!(v, Voce::Siedi { id, .. } if id == "c"))
        .count();
    assert_eq!(siedi, 1, "exactly once");
    assert!(r.rifiuti.is_empty(), "{:?}", r.rifiuti);
    r.assert_sincronizzati();
}

#[test]
fn richiesta_duplicata_ignorata() {
    let mut r = Rete::new();
    let a = r.crea("a", imp(Variante::Vanilla));
    let c = r.unisci("c");
    // the same request (same rid) reaches the host twice
    let msg = Messaggio::Richiesta {
        rid: 1,
        richiesta: Richiesta::Siedi { posto: 1 },
    };
    r.coda.push_back((c, a, msg.codifica()));
    r.coda.push_back((c, a, msg.codifica()));
    r.pompa();
    assert_eq!(r.tavolo(a).posti_di("c"), [1]);
    assert!(r.rifiuti.is_empty(), "{:?}", r.rifiuti);
    r.assert_sincronizzati();
}

/// Bots move until it is seat `s`'s turn to play a card.
fn fino_al_turno_di(r: &mut Rete, s: u8) {
    for _ in 0..200 {
        let st = r.tavolo(r.host_h()).partita.clone().unwrap();
        if st.fase == Fase::Gioco && st.required_actors().contains(&s) {
            return;
        }
        r.avanza(1_000);
    }
    panic!("seat {s} never got the turn");
}

#[test]
fn richiesta_non_piu_valida_scartata() {
    let (mut r, a, b, _) = tavolo_tipico(Variante::Vanilla);
    fino_al_turno_di(&mut r, 2);
    let st = r.tavolo(b).partita.clone().unwrap();
    let mossa = st
        .legal_actions(2)
        .into_iter()
        .find(|m| matches!(m, Azione::Gioca { .. }))
        .unwrap();
    // b's move and its resends are lost until the turn timer plays for b
    for _ in 0..50 {
        r.perdi(b, a, |m| matches!(m, Messaggio::Richiesta { .. }));
    }
    r.richiedi(b, Richiesta::Azione(mossa));
    assert_eq!(r.nodo(b).richieste_in_corso(), 1);
    while !r
        .voci
        .iter()
        .any(|v| matches!(v, Voce::Scaduto { seat: Some(2), .. }))
    {
        r.avanza(1_000);
    }
    r.da_perdere.clear();
    r.avanza(5_000);
    assert_eq!(r.nodo(b).richieste_in_corso(), 0, "dropped, not resent");
    assert!(!r.rifiuti.contains_key(&b), "{:?}", r.rifiuti);
    r.assert_sincronizzati();
}

#[test]
fn doppio_tocco_una_sola_richiesta() {
    let (mut r, a, b, _) = tavolo_tipico(Variante::Vanilla);
    fino_al_turno_di(&mut r, 2);
    let st = r.tavolo(b).partita.clone().unwrap();
    let mossa = st.legal_actions(2).remove(0);
    let (ora, prima) = (r.ora, r.inviati.len());
    let nodo = r.nodi.get_mut(&b).unwrap();
    let rid = nodo.richiedi(ora, Richiesta::Azione(mossa.clone()));
    assert_eq!(nodo.richiedi(ora, Richiesta::Azione(mossa)), rid);
    r.pompa();
    let mandate = r.inviati[prima..]
        .iter()
        .filter(|(da, d, m)| *da == b && *d == a && matches!(m, Messaggio::Richiesta { .. }))
        .count();
    assert_eq!(mandate, 1);
    assert!(r.rifiuti.is_empty(), "{:?}", r.rifiuti);
    assert_eq!(r.nodo(b).richieste_in_corso(), 0, "acknowledged");
}

/// Plays (humans answering at once, but never acknowledging) until the first hand ends.
fn fino_a_fine_mano(r: &mut Rete) {
    for _ in 0..10_000 {
        let t = r.tavolo(r.host_h()).clone();
        if t.partita.as_ref().unwrap().fase == Fase::FineMano {
            return;
        }
        let handles: Vec<H> = r.nodi.keys().copied().collect();
        if !handles
            .into_iter()
            .fold(false, |m, h| r.gioca_umani(h, false) | m)
        {
            r.avanza(10_000);
        }
    }
    panic!("the hand never ended");
}

#[test]
fn fine_mano_scade() {
    let (mut r, a, _, _) = tavolo_tipico(Variante::Vanilla);
    fino_a_fine_mano(&mut r);
    let mano = r.tavolo(a).partita.as_ref().unwrap().mano_n;
    // nobody acknowledges the summary: after the timer the next hand starts anyway
    for _ in 0..10 {
        r.avanza(60_000);
    }
    let st = r.tavolo(a).partita.clone().unwrap();
    assert!(st.mano_n > mano, "still waiting at the end of hand {mano}");
    r.assert_sincronizzati();
}

#[test]
fn fine_mano_senza_timer_aspetta() {
    let (mut r, a, _, _) = tavolo_tipico(Variante::Vanilla);
    r.richiedi(a, Richiesta::Termina);
    r.richiedi(
        a,
        Richiesta::Imposta(Impostazioni {
            timer_fine_mano: 0,
            ..imp(Variante::Vanilla)
        }),
    );
    r.richiedi(a, Richiesta::Avvia);
    fino_a_fine_mano(&mut r);
    for _ in 0..10 {
        r.avanza(3_600_000);
    }
    let st = r.tavolo(a).partita.clone().unwrap();
    assert_eq!(
        (st.fase, st.mano_n),
        (Fase::FineMano, 1),
        "waits for everyone"
    );
}

#[test]
fn giocatore_sparito_sostituito() {
    let (mut r, a, b, _) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(20);
    r.esci(b);
    assert!(r.nodo(a).in_pausa());
    // too early: b may still come back
    r.richiedi(a, Richiesta::Sostituisci { posto: 2 });
    assert_eq!(
        r.rifiuti[&a].last().unwrap(),
        "Aspetta ancora un po': potrebbe ricollegarsi"
    );
    r.avanza(config().attesa_sostituzione_ms);
    assert_eq!(r.nodo(a).sostituibili(), ["b"]);
    r.richiedi(a, Richiesta::Sostituisci { posto: 2 });
    let t = r.tavolo(a);
    assert!(t.e_bot(2) && t.sostituiti[2].as_deref() == Some("b"));
    assert!(!r.nodo(a).in_pausa());
    r.gioca_fino(u64::MAX);
    r.assert_sincronizzati();
    assert_eq!(r.tavolo(a).stato, StatoTavolo::Finita);
}

#[test]
fn giocatore_torna_e_riprende_il_posto() {
    let (mut r, a, b, c) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(20);
    r.esci(b);
    r.avanza(config().attesa_sostituzione_ms);
    r.richiedi(a, Richiesta::Sostituisci { posto: 2 });
    r.gioca_fino(40);
    // c never sat there: refused; b comes back and takes its seat again
    r.richiedi(c, Richiesta::Riprendi { posto: 2 });
    assert_eq!(r.rifiuti[&c].last().unwrap(), "Questo posto non era tuo");
    let b2 = r.unisci("b");
    r.richiedi(b2, Richiesta::Riprendi { posto: 2 });
    assert_eq!(r.tavolo(a).posti_di("b"), [2]);
    assert!(r.tavolo(a).sostituiti[2].is_none());
    r.gioca_fino(u64::MAX);
    r.assert_sincronizzati();
}

// ------------------------------------------------------------------ untrusted input (P13)

/// A replica `c` would accept from the host, with `guasta` applied to it.
fn istantanea_guasta(r: &Rete, c: H, guasta: impl Fn(&mut Tavolo)) -> Vec<u8> {
    let mut t = r.tavolo(c).clone();
    guasta(&mut t);
    Messaggio::Istantanea {
        epoca: r.nodo(c).epoca(),
        host: r.nodo(c).host_regia().unwrap().clone(),
        tavolo: Box::new(t),
    }
    .codifica()
}

#[test]
fn istantanee_guaste_rifiutate() {
    let (mut r, a, _, c) = tavolo_tipico(Variante::Vanilla);
    r.gioca_fino(20);
    let prima = r.tavolo(c).clone();
    let guasti: [fn(&mut Tavolo); 3] = [
        |t| t.partita.as_mut().unwrap().mani[0][0] = bazzica_core::Card::IGNOTA,
        |t| t.partita.as_mut().unwrap().corrente = 9,
        |t| t.partita.as_mut().unwrap().mazzo.clear(),
    ];
    for guasta in guasti {
        let b = istantanea_guasta(&r, c, guasta);
        r.coda.push_back((a, c, b));
        r.pompa();
        assert_eq!(r.tavolo(c), &prima, "an invalid snapshot was adopted");
    }
    r.gioca_fino(u64::MAX);
    r.assert_sincronizzati();
}

#[test]
fn striscia_con_posto_impossibile() {
    let (mut r, a, _, c) = tavolo_tipico(Variante::Vanilla);
    let m = Messaggio::Effimero(Effimero::Striscia {
        seat: 255,
        verso: 2,
    });
    r.coda.push_back((a, c, m.codifica()));
    r.pompa();
    assert!(r.effimeri.get(&c).is_none_or(Vec::is_empty));
}

#[test]
fn host_con_epoca_enorme_ignorato() {
    let (mut r, a, b, c) = tavolo_tipico(Variante::Vanilla);
    let m = Messaggio::Host {
        epoca: u64::MAX,
        n: 0,
    };
    r.coda.push_back((b, a, m.codifica()));
    r.coda.push_back((b, c, m.codifica()));
    r.pompa();
    assert!(r.nodo(a).e_host(), "a stays host");
    assert_eq!(r.nodo(c).host().map(String::as_str), Some("a"));
    r.gioca_fino(u64::MAX);
    r.assert_sincronizzati();
}

/// Changes one random leaf of a JSON value (numbers get extreme or seat-like values).
fn muta(v: &mut serde_json::Value, rng: &mut bazzica_core::Mulberry32) {
    use serde_json::Value;
    let figli = match v {
        Value::Array(a) if !a.is_empty() => a.len(),
        Value::Object(o) if !o.is_empty() => o.len(),
        _ => 0,
    };
    if figli == 0 {
        *v = match rng.next_u32() % 8 {
            0 => Value::from(255),
            1 => Value::from(9),
            2 => Value::from(u64::MAX),
            3 => Value::from(-1),
            4 => Value::from("?"),
            5 => Value::Bool(rng.next_u32().is_multiple_of(2)),
            6 => Value::Array(vec![]),
            _ => Value::from(rng.next_u32() % 50),
        };
        return;
    }
    let i = rng.next_u32() as usize % figli;
    match v {
        Value::Array(a) => muta(&mut a[i], rng),
        Value::Object(o) => {
            let k = o.keys().nth(i).unwrap().clone();
            muta(o.get_mut(&k).unwrap(), rng);
        }
        _ => unreachable!(),
    }
}

#[test]
fn messaggi_mutati_non_fanno_danni() {
    let (mut r, a, b, c) = tavolo_tipico(Variante::Vanilla);
    let x = r.unisci("x");
    r.gioca_fino(30);
    let mut veri: Vec<serde_json::Value> = r
        .inviati
        .iter()
        .map(|(_, _, m)| serde_json::to_value(m).unwrap())
        .collect();
    // kinds a normal game may not have sent yet
    for m in [
        Messaggio::Effimero(Effimero::Striscia { seat: 1, verso: 0 }),
        Messaggio::Effimero(Effimero::Frase { seat: 0, indice: 1 }),
        Messaggio::Host { epoca: 1, n: 3 },
        Messaggio::Allineato {
            epoca: 1,
            n: 3,
            impronta: "x".into(),
        },
        Messaggio::Presenti {
            ids: ["a".to_string()].into(),
        },
    ] {
        veri.push(serde_json::to_value(m).unwrap());
    }
    let mut per_tipo: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    for v in veri {
        let tipo = format!(
            "{}{}",
            v["msg"],
            v.get("effetto").map_or(String::new(), |e| e.to_string())
        );
        per_tipo.entry(tipo).or_default().push(v);
    }
    let tipi: Vec<Vec<serde_json::Value>> = per_tipo.into_values().collect();
    // fixed in CI; `SEME_FUZZ=<n>` explores other seeds
    let seme = std::env::var("SEME_FUZZ")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2026);
    let mut rng = bazzica_core::Mulberry32::new(seme);
    let mut consegnati = 0;
    for _ in 0..1_000_000 {
        if consegnati == 10_000 {
            break;
        }
        // a kind of message first, so rare kinds get mutated as often as entries
        let tipo = &tipi[rng.next_u32() as usize % tipi.len()];
        let mut v = tipo[rng.next_u32() as usize % tipo.len()].clone();
        for _ in 0..1 + rng.next_u32() % 3 {
            muta(&mut v, &mut rng);
        }
        let Ok(m) = serde_json::from_value::<Messaggio>(v) else {
            continue;
        };
        // x, a participant, attacks c only; a and b are honest
        consegnati += 1;
        r.coda.push_back((x, c, m.codifica()));
        r.consegna_uno();
        r.coda.retain(|(da, _, _)| *da != c);
        let _ = r.nodi.get_mut(&c).unwrap().uscite();
        // what the UI does with the replica must not panic either
        if let Some(st) = r.nodo(c).tavolo().and_then(|t| t.partita.as_ref()) {
            for s in 0..4 {
                let _ = st.observation(s);
                let _ = st.legal_actions(s);
            }
        }
    }
    assert_eq!(consegnati, 10_000, "mutated messages delivered");
    r.pompa();
    let onesti: BTreeSet<_> = [a, b]
        .iter()
        .map(|h| (r.tavolo(*h).n, r.tavolo(*h).impronta()))
        .collect();
    assert_eq!(onesti.len(), 1, "honest peers diverged");
}
