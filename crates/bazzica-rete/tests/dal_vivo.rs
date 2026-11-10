//! End-to-end over the real internet: public Nostr relays for signaling, native WebRTC for data.
//! Ignored by default (needs network). Run with
//! `cargo test -p bazzica-rete --test dal_vivo -- --ignored --nocapture`.

use std::time::{Duration, Instant};

use bazzica_core::bots::{Agent, HeuristicAgent};
use bazzica_core::{Azione, Fase};
use bazzica_net::{Config, Impostazioni, Nodo, Partecipante, Richiesta, StatoTavolo};
use bazzica_rete::nostr::nuovo_codice;
use bazzica_rete::{NostrSignallerBuilder, Sessione, Turn};

fn io(id: &str) -> Partecipante {
    Partecipante {
        id: id.into(),
        nome: id.to_uppercase(),
        avatar: "🦊".into(),
    }
}

fn config() -> Config {
    Config {
        ritardo_bot: (20, 50),
        pausa_fine_mano: 100,
        pausa_palo: 3000,
        ..Config::default()
    }
}

#[test]
#[ignore]
fn quattro_peer_su_nostr_e_webrtc() {
    let codice = nuovo_codice();
    println!("stanza {codice}");
    let inizio = Instant::now();
    let ora = || inizio.elapsed().as_millis() as u64;
    let mut sessioni = vec![];
    for (i, id) in ["a", "b", "c", "d"].into_iter().enumerate() {
        let nodo = if i == 0 {
            Nodo::crea(io(id), Impostazioni::default(), 1, config())
        } else {
            Nodo::unisciti(io(id), i as u64, config())
        };
        let (s, ciclo) = Sessione::apri(
            &codice,
            NostrSignallerBuilder::default(),
            nodo,
            Turn::di_compilazione(),
        );
        std::thread::spawn(move || {
            if let Err(e) = smol::block_on(ciclo) {
                println!("ciclo terminato: {e}");
            }
        });
        sessioni.push(s);
    }
    let attendi = |sessioni: &mut Vec<Sessione>, cosa: &str, f: &dyn Fn(&[Sessione]) -> bool| {
        let t0 = Instant::now();
        while !f(sessioni) {
            assert!(t0.elapsed() < Duration::from_secs(60), "timeout: {cosa}");
            for s in sessioni.iter_mut() {
                s.aggiorna(ora());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        println!("{:>6} ms  {cosa}", ora());
    };
    attendi(&mut sessioni, "all four peers have the replica", &|s| {
        s.iter()
            .all(|x| x.nodo().tavolo().is_some_and(|t| t.partecipanti.len() == 4))
    });
    for (i, s) in sessioni.iter_mut().enumerate() {
        s.richiedi(ora(), Richiesta::Siedi { posto: i as u8 });
    }
    attendi(&mut sessioni, "seated", &|s| {
        s.iter()
            .all(|x| x.nodo().tavolo().unwrap().posti.iter().all(Option::is_some))
    });
    sessioni[0].richiedi(ora(), Richiesta::Avvia);
    attendi(&mut sessioni, "started", &|s| {
        s.iter()
            .all(|x| x.nodo().tavolo().unwrap().stato == StatoTavolo::InCorso)
    });
    // play: every peer answers for its seat
    let t0 = Instant::now();
    loop {
        let finita = sessioni
            .iter()
            .all(|x| x.nodo().tavolo().unwrap().stato == StatoTavolo::Finita);
        if finita {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(300), "game too slow");
        for (i, s) in sessioni.iter_mut().enumerate() {
            s.aggiorna(ora());
            let t = s.nodo().tavolo().unwrap();
            let Some(st) = t.partita.as_ref() else {
                continue;
            };
            let seat = i as u8;
            if t.stato != StatoTavolo::InCorso {
                continue;
            }
            if st.fase == Fase::FineMano {
                if !t.ack.contains(&s.nodo().io().id) {
                    s.richiedi(ora(), Richiesta::Prosegui);
                }
                continue;
            }
            if !st.required_actors().contains(&seat) {
                continue;
            }
            let legal: Vec<Azione> = st
                .legal_actions(seat)
                .into_iter()
                .filter(|a| !matches!(a, Azione::Attendi { .. } | Azione::ChePalo { .. }))
                .collect();
            if let Some(a) = (!legal.is_empty())
                .then(|| HeuristicAgent::new(t.n).choose_action(&st.observation(seat), &legal))
            {
                s.richiedi(ora(), Richiesta::Azione(a));
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let impronte: Vec<String> = sessioni
        .iter()
        .map(|s| s.nodo().tavolo().unwrap().impronta())
        .collect();
    println!(
        "{:>6} ms  game over, n = {}",
        ora(),
        sessioni[0].nodo().tavolo().unwrap().n
    );
    assert!(impronte.windows(2).all(|w| w[0] == w[1]), "{impronte:?}");
}

/// A player joining a table that already exists (its host has been alone in the room for a
/// while; `ATTESA=<seconds>` changes how long, default 15) gets the replica.
#[test]
#[ignore]
fn entra_in_un_tavolo_gia_creato() {
    let codice = nuovo_codice();
    let inizio = Instant::now();
    let ora = || inizio.elapsed().as_millis() as u64;
    let apri = |nodo| {
        let (s, ciclo) = Sessione::apri(
            &codice,
            NostrSignallerBuilder::default(),
            nodo,
            Turn::di_compilazione(),
        );
        std::thread::spawn(move || smol::block_on(ciclo));
        s
    };
    let mut host = apri(Nodo::crea(io("a"), Impostazioni::default(), 1, config()));
    while inizio.elapsed()
        < Duration::from_secs(
            std::env::var("ATTESA")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(15),
        )
    {
        host.aggiorna(ora());
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut ospite = apri(Nodo::unisciti(io("b"), 2, config()));
    let t0 = Instant::now();
    while !ospite
        .nodo()
        .tavolo()
        .is_some_and(|t| t.partecipanti.len() == 2)
    {
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "timeout: host {:?}, guest {:?}",
            host.diagnosi(),
            ospite.diagnosi()
        );
        host.aggiorna(ora());
        ospite.aggiorna(ora());
        std::thread::sleep(Duration::from_millis(20));
    }
    println!("joined after {:?}", t0.elapsed());
}
