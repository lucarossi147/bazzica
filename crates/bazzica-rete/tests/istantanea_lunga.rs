//! A snapshot of a long game is far above one data-channel message: it must travel in pieces.

use bazzica_core::bots::{Agent, HeuristicAgent};
use bazzica_core::{Azione, GameState, RulesConfig, Variante};
use bazzica_net::{
    Impostazioni, Messaggio, Partecipante, Posto, Regia, Sessione, StatoTavolo, Tavolo,
};
use bazzica_rete::frammenti::{MAX_PACCHETTO, Ricompositore, spezza};

/// A bot-only table that has played `mani` hands (nobody reaches the winning score).
fn partita_lunga(mani: usize) -> Tavolo {
    let regole = RulesConfig {
        punti_vittoria: 100_000,
        ..RulesConfig::preset(Variante::Vanilla, false)
    };
    let (mut st, _) = GameState::new_game(regole, 1, None);
    let mut agente = HeuristicAgent::new(1);
    for _ in 0..100_000 {
        if st.storico_mani.len() >= mani {
            break;
        }
        if let Some(a) = st.system_action() {
            st.apply(&a).unwrap();
            continue;
        }
        let s = st.required_actors()[0];
        let legali: Vec<Azione> = st
            .legal_actions(s)
            .into_iter()
            .filter(|a| !matches!(a, Azione::Attendi { .. } | Azione::ChePalo { .. }))
            .collect();
        let a = agente.choose_action(&st.observation(s), &legali);
        st.apply(&a).unwrap();
    }
    assert_eq!(st.storico_mani.len(), mani);
    let io = Partecipante {
        id: "a".into(),
        nome: "A".into(),
        avatar: "🦊".into(),
    };
    let mut t = Tavolo::nuovo(io, Impostazioni::default());
    t.posti = [
        Some(Posto::Bot),
        Some(Posto::Bot),
        Some(Posto::Bot),
        Some(Posto::Bot),
    ];
    t.stato = StatoTavolo::InCorso;
    t.partita = Some(st);
    t
}

#[test]
fn istantanea_di_venti_mani_in_frammenti() {
    let msg = Messaggio::Istantanea {
        epoca: 0,
        host: Regia {
            id: "a".into(),
            sessione: Sessione::default(),
        },
        tavolo: Box::new(partita_lunga(20)),
    };
    let byte = msg.codifica();
    println!("snapshot after 20 hands: {} bytes", byte.len());
    let pacchetti = spezza(&byte, 1);
    assert!(pacchetti.len() > 1, "large enough to need fragments");
    assert!(pacchetti.iter().all(|p| p.len() <= MAX_PACCHETTO));
    let mut r = Ricompositore::default();
    let intero = pacchetti
        .iter()
        .fold(None, |acc, p| acc.or(r.ricevi(&0u8, p)))
        .unwrap();
    assert_eq!(Messaggio::decodifica(&intero), Some(msg));
}
