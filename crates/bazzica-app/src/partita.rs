//! The game table, seen from the local player's seat.

use bazzica_core::game::Osservazione;
use bazzica_core::scoring::Riepilogo;
use bazzica_core::{Azione, Card, Fase, GameState, Seat, Vincitore};
use bazzica_net::tavolo::FRASI;
use bazzica_net::{Effimero, Posto, Richiesta, StatoTavolo};
use dioxus::prelude::*;

use crate::carte::{Carta, Dorso};
use crate::stanza::avatar_posto;
use crate::{Ctx, Vista};

#[component]
pub fn Partita(vista: Vista) -> Element {
    let t = &vista.tavolo;
    let st = t.partita.as_ref().expect("game in progress");
    let miei = t.posti_di(&vista.io);
    let spettatore = miei.is_empty();
    // with several seats (testing alone), show the one that must act
    let seat = crate::posto_vista(t, &vista.io);
    let obs = st.observation(seat);
    let pos = |d: u8| (seat + d) % 4;
    let finita = t.stato == StatoTavolo::Finita;

    rsx! {
        div { class: "partita",
            Punteggi { vista: vista.clone(), obs: obs.clone() }
            if !vista.disconnessi.is_empty() && !finita {
                div { class: "banner",
                    "In pausa: aspettiamo "
                    {vista.disconnessi.iter().map(|id| vista.nome(id)).collect::<Vec<_>>().join(", ")}
                }
            }
            div { class: "campo",
                div { class: "area-alto", Avversario { vista: vista.clone(), obs: obs.clone(), seat: pos(2) } }
                div { class: "area-sx", Avversario { vista: vista.clone(), obs: obs.clone(), seat: pos(1) } }
                div { class: "area-dx", Avversario { vista: vista.clone(), obs: obs.clone(), seat: pos(3) } }
                div { class: "area-centro", Centro { vista: vista.clone(), obs: obs.clone() } }
            }
            Stato { vista: vista.clone(), st: st.clone(), seat }
            if spettatore {
                div { class: "mano spettatore", "Stai guardando la partita." }
            } else {
                Mano { st: st.clone(), seat, attivo: vista.disconnessi.is_empty() }
            }
            if finita {
                Fine { vista: vista.clone() }
            } else if st.fase == Fase::FineMano {
                FineMano { vista: vista.clone() }
            }
        }
    }
}

#[component]
fn Punteggi(vista: Vista, obs: Osservazione) -> Element {
    let imp = &vista.tavolo.impostazioni;
    let obiettivo = if obs.regole.margherita_hack {
        format!("mano {} di 4", obs.mano_n)
    } else {
        format!("mano {} · a {}", obs.mano_n, obs.regole.punti_vittoria)
    };
    rsx! {
        div { class: "punteggi",
            div { class: "squadra sq0", span { "{imp.squadre[0]}" } b { "{obs.punteggi[0]}" } }
            div { class: "info", "{obs.regole.descrizione()}" br {} "{obiettivo}" }
            div { class: "squadra sq1", b { "{obs.punteggi[1]}" } span { "{imp.squadre[1]}" } }
        }
    }
}

#[component]
fn Avversario(vista: Vista, obs: Osservazione, seat: Seat) -> Element {
    let s = usize::from(seat);
    let nome = vista.tavolo.nome_posto(seat);
    let avatar = avatar_posto(&vista, s);
    let altro = obs.altri.get(&seat);
    let n = altro.map_or(0, |a| a.n);
    let scoperte = altro.and_then(|a| a.carte.clone());
    let di_turno = obs.fase == Fase::Gioco && obs.corrente == seat;
    let scollegato =
        matches!(&vista.tavolo.posti[s], Some(Posto::Umano { id }) if !vista.connessi.contains(id));
    let classe = format!(
        "giocatore sq{}{}{}",
        seat % 2,
        if di_turno { " di-turno" } else { "" },
        if scollegato { " scollegato" } else { "" }
    );
    rsx! {
        div { class: "{classe}", "data-seat": "{seat}",
            div { class: "chi",
                span { class: "avatar-piccolo", "{avatar}" }
                span { "{nome}" }
                if obs.mazziere == seat { span { class: "tag", "mazziere" } }
                if obs.che_palo.contains(&seat) { span { class: "tag palo", "Che palo!" } }
                if obs.fase == Fase::Scambio && obs.scambio.passati[s] {
                    span { class: "tag", if obs.scambio.veloci[s] { "passata veloce" } else { "passata" } }
                }
            }
            if let Some(b) = obs.bazziche.get(&seat) {
                div { class: "bazzica-chiamata", "Bazzica: {b.tipo.nome()} · {b.scope} scope" }
            }
            div { class: "carte-avversario",
                if let Some(carte) = scoperte {
                    for c in carte { Carta { carta: c, classe: "mini" } }
                } else {
                    for _ in 0..n { Dorso { classe: "mini" } }
                }
            }
        }
    }
}

#[component]
fn Centro(vista: Vista, obs: Osservazione) -> Element {
    let ultima = obs.giocate_mano.last().cloned();
    rsx! {
        div { class: "tavola",
            if obs.tavola.is_empty() {
                span { class: "vuota", "tavola vuota" }
            }
            for c in obs.tavola.iter().copied() {
                Carta { carta: c, classe: "in-tavola" }
            }
        }
        div { class: "dettagli",
            span { "Mazzo {obs.carte_nel_mazzo} · Prese " }
            for sq in 0..2usize {
                if sq == 1 { span { "–" } }
                span { class: "sq{sq}", "data-prese": "{sq}",
                    "{obs.prese[sq].n}"
                    if !obs.prese[sq].scoperte.is_empty() { " ({obs.prese[sq].scoperte.len()} sc.)" }
                    if obs.credito[sq] > 0 { " +{obs.credito[sq]} cred." }
                }
            }
        }
        if let Some(g) = ultima {
            div { class: "ultima",
                "{vista.tavolo.nome_posto(g.0)} ha giocato {g.1.nome()}"
                if !g.2.is_empty() { " e preso {bazzica_core::cards::nomi(&g.2)}" }
            }
        }
    }
}

/// Whose turn it is, and the countdown.
#[component]
fn Stato(vista: Vista, st: GameState, seat: Seat) -> Element {
    let ctx = use_context::<Ctx>();
    let imp = &vista.tavolo.impostazioni;
    let (testo, durata) = match st.fase {
        Fase::Scambio => {
            let mio = !st.scambio_passata[usize::from(seat)].is_none();
            (
                if mio {
                    "Scambio: aspettiamo gli altri".to_string()
                } else {
                    "Scambio: passa una carta al compagno".to_string()
                },
                imp.timer_scambio,
            )
        }
        Fase::Gioco => {
            let durata = if vista.tavolo.e_bot(st.corrente) {
                0
            } else {
                imp.timer_giocata
            };
            if vista.tavolo.posti_di(&vista.io).contains(&st.corrente) {
                ("Tocca a te".to_string(), durata)
            } else {
                (
                    format!("Tocca a {}", vista.tavolo.nome_posto(st.corrente)),
                    durata,
                )
            }
        }
        Fase::FineMano => ("Fine della mano".to_string(), 0),
        Fase::FinePartita => ("Partita finita".to_string(), 0),
    };
    let restanti = if durata > 0 && vista.disconnessi.is_empty() {
        let (_, inizio) = ctx.turno.read().clone();
        let passati = ctx.ora.read().saturating_sub(inizio) / 1000;
        Some(u64::from(durata).saturating_sub(passati))
    } else {
        None
    };
    rsx! {
        div { class: "stato",
            span { "{testo}" }
            if let Some(r) = restanti {
                span { class: if r <= 5 { "timer urgente" } else { "timer" }, "{r} s" }
            }
        }
    }
}

#[component]
fn Mano(st: GameState, seat: Seat, attivo: bool) -> Element {
    let ctx = use_context::<Ctx>();
    let mut scelta = use_signal(|| None::<Card>);
    let legali = if attivo {
        st.legal_actions(seat)
    } else {
        vec![]
    };
    let mano = st.mani[usize::from(seat)].clone();
    let giocabili = |c: Card| -> Vec<Azione> {
        legali
            .iter()
            .filter(|a| matches!(a, Azione::Gioca { carta, .. } | Azione::Passa { carta, .. } if *carta == c))
            .cloned()
            .collect()
    };
    // Bazzica calls, "che palo"...: always-available buttons
    let altre: Vec<Azione> = legali
        .iter()
        .filter(|a| matches!(a, Azione::Bazzica { .. } | Azione::ChePalo { .. }))
        .cloned()
        .collect();
    let opzioni = scelta().map(giocabili).unwrap_or_default();
    // the chosen card is no longer playable (turn passed, hand changed)
    if scelta().is_some() && opzioni.is_empty() {
        scelta.set(None);
    }
    let mut fai = move |a: Azione| {
        scelta.set(None);
        ctx.richiedi(Richiesta::Azione(a));
    };
    let ricevuta = st.scambio_ricevuta[usize::from(seat)];
    rsx! {
        div { class: "mano",
            if !altre.is_empty() {
                div { class: "azioni-extra",
                    for a in altre {
                        button {
                            class: if matches!(a, Azione::ChePalo { .. }) { "speciale palo" } else { "speciale" },
                            onclick: {
                                let a = a.clone();
                                move |_| fai(a.clone())
                            },
                            {etichetta(&st, &a)}
                        }
                    }
                }
            }
            if !opzioni.is_empty() {
                div { class: "opzioni",
                    for a in opzioni.iter().cloned() {
                        button {
                            class: "opzione",
                            onclick: {
                                let a = a.clone();
                                move |_| fai(a.clone())
                            },
                            {etichetta(&st, &a)}
                        }
                    }
                    button { class: "secondario", onclick: move |_| scelta.set(None), "Annulla" }
                }
            }
            Chiacchiere { st: st.clone(), seat }
            div { class: "carte-mano", "data-seat": "{seat}",
                for c in mano {
                    Carta {
                        carta: c,
                        classe: format!(
                            "in-mano{}{}",
                            if scelta() == Some(c) { " scelta" } else { "" },
                            if Some(c) == ricevuta { " ricevuta" } else { "" },
                        ),
                        onclick: {
                            let azioni = giocabili(c);
                            (!azioni.is_empty()).then(|| EventHandler::new(move |_| {
                                if azioni.len() == 1 {
                                    fai(azioni[0].clone());
                                } else {
                                    scelta.set(Some(c));
                                }
                            }))
                        },
                    }
                }
            }
        }
    }
}

/// Quick phrases and the "striscia" (allowed communication, cosmetic only).
#[component]
fn Chiacchiere(st: GameState, seat: Seat) -> Element {
    let ctx = use_context::<Ctx>();
    let mut aperto = use_signal(|| false);
    if st.fase != Fase::Gioco {
        return rsx! {};
    }
    let precedente = (seat + 3) % 4;
    let puo_strisciare = st.corrente == precedente;
    rsx! {
        div { class: "chiacchiere",
            button { class: "piccolo chiacchiera", onclick: move |_| aperto.toggle(), "💬" }
            if puo_strisciare {
                button {
                    class: "piccolo chiacchiera",
                    title: "Metti fretta al giocatore prima di te",
                    onclick: move |_| ctx.effimero(Effimero::Striscia { seat, verso: precedente }),
                    "Striscia"
                }
            }
        }
        if aperto() {
            div { class: "frasi",
                for (i, f) in FRASI.iter().enumerate() {
                    button {
                        class: "piccolo",
                        onclick: move |_| {
                            aperto.set(false);
                            ctx.effimero(Effimero::Frase { seat, indice: i as u8 });
                        },
                        "{f}"
                    }
                }
            }
        }
    }
}

/// Button text for an action, with what the play would achieve.
fn etichetta(st: &GameState, a: &Azione) -> String {
    match a {
        Azione::Gioca { carta, presa, .. } => {
            let mut s = if presa.is_empty() {
                format!("Gioca {} senza prendere", carta.nome())
            } else {
                format!("Prendi {}", bazzica_core::cards::nomi(presa))
            };
            if let Some(ant) = st.anteprima(a) {
                let e = &ant.esito;
                for (si, t) in [
                    (e.scopa, "Scopa!"),
                    (e.piccola, "Piccola!"),
                    (e.grande, "Grande!"),
                    (e.sparecchio, "Sparecchio"),
                ] {
                    if si {
                        s.push_str(" · ");
                        s.push_str(t);
                    }
                }
            }
            s
        }
        Azione::Passa { carta, veloce, .. } => {
            if *veloce {
                format!("Passa {} veloce (subito, senza aspettare)", carta.nome())
            } else {
                format!("Passa {} al compagno", carta.nome())
            }
        }
        Azione::Bazzica { seat, sette } => {
            let opz =
                bazzica_core::bazzica::opzioni_bazzica(&st.mani[usize::from(*seat)], &st.regole);
            opz.iter()
                .find(|o| o.sette == *sette)
                .map_or_else(|| a.descrivi(), |o| format!("Bazzica: {}", o.descrizione()))
        }
        _ => a.descrivi(),
    }
}

#[component]
fn FineMano(vista: Vista) -> Element {
    let ctx = use_context::<Ctx>();
    let t = &vista.tavolo;
    let st = t.partita.as_ref().unwrap();
    let Some(r) = st.storico_mani.last().cloned() else {
        return rsx! {};
    };
    let seduto = t.umani().contains(&vista.io);
    let confermato = t.ack.contains(&vista.io);
    let mancano: Vec<String> = t
        .umani()
        .into_iter()
        .filter(|id| !t.ack.contains(id))
        .map(|id| vista.nome(&id))
        .collect();
    rsx! {
        div { class: "velo",
            div { class: "finestra",
                h2 { "Fine della mano {r.mano}" }
                TabellaRiepilogo { r: r.clone(), squadre: t.impostazioni.squadre.clone() }
                if seduto && !confermato {
                    button { class: "primario", onclick: move |_| ctx.richiedi(Richiesta::Prosegui), "Prosegui" }
                } else if !mancano.is_empty() {
                    p { class: "nota", "Aspettiamo: {mancano.join(\", \")}" }
                }
            }
        }
    }
}

#[component]
fn TabellaRiepilogo(r: Riepilogo, squadre: [String; 2]) -> Element {
    let [a, b] = &r.squadre;
    let si = |x: bool| if x { "✓" } else { "" };
    let righe: Vec<(&str, String, String)> = vec![
        (
            "Carte",
            format!("{} ({})", a.carte, a.punti_mazzo_dettaglio.carte),
            format!("{} ({})", b.carte, b.punti_mazzo_dettaglio.carte),
        ),
        (
            "Denari",
            format!("{} ({})", a.denari, a.punti_mazzo_dettaglio.denari),
            format!("{} ({})", b.denari, b.punti_mazzo_dettaglio.denari),
        ),
        (
            "Primiera",
            format!("{} ({})", a.primiera, a.punti_mazzo_dettaglio.primiera),
            format!("{} ({})", b.primiera, b.punti_mazzo_dettaglio.primiera),
        ),
        (
            "Sette bello",
            si(a.sette_bello).into(),
            si(b.sette_bello).into(),
        ),
        ("Re bello", si(a.re_bello).into(), si(b.re_bello).into()),
        (
            "Due di denari",
            a.punti_due_denari.to_string(),
            b.punti_due_denari.to_string(),
        ),
        ("Scope", a.scope.to_string(), b.scope.to_string()),
        ("Napola", a.napola.to_string(), b.napola.to_string()),
        ("Totale mano", a.totale.to_string(), b.totale.to_string()),
        (
            "Punteggio",
            r.punteggi[0].to_string(),
            r.punteggi[1].to_string(),
        ),
    ];
    rsx! {
        table { class: "riepilogo",
            thead { tr { th {} th { class: "sq0", "{squadre[0]}" } th { class: "sq1", "{squadre[1]}" } } }
            tbody {
                for (nome, x, y) in righe {
                    tr { td { "{nome}" } td { "{x}" } td { "{y}" } }
                }
            }
        }
        if r.credito_perso.iter().any(|c| *c > 0) {
            p { class: "nota", "Credito di scope perso: {r.credito_perso[0]} / {r.credito_perso[1]}" }
        }
    }
}

#[component]
fn Fine(vista: Vista) -> Element {
    let ctx = use_context::<Ctx>();
    let t = &vista.tavolo;
    let st = t.partita.as_ref().unwrap();
    let titolo = match (&t.terminata_da, st.vincitore) {
        (Some(da), _) => format!("Partita interrotta da {}", vista.nome(da)),
        (None, Some(Vincitore::Squadra(s))) => {
            format!("Vince {}!", t.impostazioni.squadre[usize::from(s)])
        }
        (None, Some(Vincitore::Pari)) => "Pareggio!".to_string(),
        (None, None) => "Partita finita".to_string(),
    };
    let imp = t.impostazioni.clone();
    rsx! {
        div { class: "velo",
            div { class: "finestra",
                h2 { "{titolo}" }
                p { class: "punteggio-finale",
                    span { class: "sq0", "{imp.squadre[0]} {st.punteggi[0]}" }
                    " – "
                    span { class: "sq1", "{st.punteggi[1]} {imp.squadre[1]}" }
                }
                if let Some(r) = st.storico_mani.last().cloned() {
                    details {
                        summary { "Ultima mano" }
                        TabellaRiepilogo { r, squadre: imp.squadre.clone() }
                    }
                }
                div { class: "riga",
                    button { class: "primario", onclick: move |_| ctx.richiedi(Richiesta::Avvia), "Rivincita" }
                    button {
                        class: "secondario",
                        onclick: move |_| ctx.richiedi(Richiesta::Imposta(imp.clone())),
                        "Torna ai posti"
                    }
                }
            }
        }
    }
}

#[component]
pub fn Menu() -> Element {
    let ctx = use_context::<Ctx>();
    let mut conferma = use_signal(|| false);
    rsx! {
        div { class: "menu-partita",
            if conferma() {
                span { "Interrompere per tutti?" }
                button {
                    class: "piccolo pericolo",
                    onclick: move |_| {
                        conferma.set(false);
                        ctx.richiedi(Richiesta::Termina);
                    },
                    "Sì"
                }
                button { class: "piccolo secondario", onclick: move |_| conferma.set(false), "No" }
            } else {
                button { class: "piccolo secondario", onclick: move |_| conferma.set(true), "Interrompi" }
            }
        }
    }
}
