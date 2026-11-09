//! The game table, seen from the local player's seat.

use bazzica_core::cards::nomi;
use bazzica_core::game::{GIOCATE_PER_TURNO, GiocataMano, Osservazione};
use bazzica_core::scoring::{Peso, Riepilogo};
use bazzica_core::{Azione, Card, Fase, GameState, Seat, Vincitore};
use bazzica_net::tavolo::FRASI;
use bazzica_net::{Effimero, Posto, Richiesta, StatoTavolo};
use dioxus::prelude::*;

use crate::carte::{Carta, Dorso};
use crate::guida::{Apertura, Guida, Offerta};
use crate::stanza::avatar_posto;
use crate::{Ctx, Vista};

/// Side of the screen a seat sits on, seen from `io`: 0 me, 1 left, 2 across, 3 right.
fn relativo(seat: Seat, io: Seat) -> u8 {
    (seat + 4 - io) % 4
}

const LATI: [&str; 4] = ["basso", "sx", "alto", "dx"];

/// Overlays the game screen can open on top of the table.
#[derive(Clone, Copy, PartialEq)]
enum Finestra {
    Guida,
    Riepilogo,
    Giocate,
}

#[component]
pub fn Partita(vista: Vista) -> Element {
    let t = &vista.tavolo;
    let st = t.partita.as_ref().expect("game in progress");
    let miei = t.posti_di(&vista.io);
    let spettatore = miei.is_empty();
    // with several seats (one device passed around), show the one that must act
    let seat = crate::posto_vista(t, &vista.io);
    let obs = st.observation(seat);
    let pos = |d: u8| (seat + d) % 4;
    let finita = t.stato == StatoTavolo::Finita;
    let mut finestra = use_signal(|| None::<Finestra>);
    let mut mostrato = use_signal(|| None::<Seat>);
    let mio_turno = st.fase == Fase::Gioco && miei.contains(&st.corrente);
    imposta_titolo(if mio_turno {
        "● Tocca a te · Bazzica"
    } else {
        "Bazzica"
    });
    // one device, several seats: hide the hand until the next player takes the phone
    let da_passare = miei.len() > 1
        && matches!(st.fase, Fase::Gioco | Fase::Scambio)
        && mostrato() != Some(seat);
    let regole = &obs.regole;
    let apertura = Apertura::per(regole.nome_variante, regole.margherita_hack);

    rsx! {
        div { class: "partita",
            Punteggi {
                vista: vista.clone(),
                obs: obs.clone(),
                apri: move |f| finestra.set(Some(f)),
            }
            if !vista.disconnessi.is_empty() && !finita {
                div { class: "banner",
                    "In pausa: aspettiamo "
                    {vista.disconnessi.iter().map(|id| vista.nome(id)).collect::<Vec<_>>().join(", ")}
                }
            }
            if !spettatore && !finita {
                Offerta { variante: regole.nome_variante, margherita: regole.margherita_hack }
            }
            div { class: "campo",
                div { class: "area-alto", Giocatore { vista: vista.clone(), obs: obs.clone(), seat: pos(2) } }
                div { class: "area-sx", Giocatore { vista: vista.clone(), obs: obs.clone(), seat: pos(1) } }
                div { class: "area-dx", Giocatore { vista: vista.clone(), obs: obs.clone(), seat: pos(3) } }
                div { class: "area-centro",
                    Centro { vista: vista.clone(), obs: obs.clone(), apri: move |f| finestra.set(Some(f)) }
                }
            }
            Turno { vista: vista.clone(), st: st.clone(), obs: obs.clone(), seat }
            if spettatore {
                div { class: "mano spettatore", "Stai guardando la partita." }
            } else {
                Mano {
                    key: "{seat}",
                    st: st.clone(),
                    seat,
                    attivo: vista.disconnessi.is_empty(),
                }
            }
            if da_passare {
                div { class: "velo passa-telefono", onclick: move |_| mostrato.set(Some(seat)),
                    div { class: "finestra",
                        h2 { "Passa a {t.nome_posto(seat)}" }
                        p { "Tocca quando sei pronto a vedere le tue carte." }
                    }
                }
            }
            if finita {
                Fine { vista: vista.clone() }
            } else if st.fase == Fase::FineMano {
                FineMano { vista: vista.clone() }
            }
            match finestra() {
                Some(Finestra::Guida) => rsx! {
                    Guida { apertura, chiudi: move |_| finestra.set(None) }
                },
                Some(Finestra::Riepilogo) => rsx! {
                    if let Some(r) = obs.storico_mani.last().cloned() {
                        div { class: "velo", onclick: move |_| finestra.set(None),
                            div { class: "finestra", onclick: move |e| e.stop_propagation(),
                                h2 { "Riepilogo della mano {r.mano}" }
                                TabellaRiepilogo { r, squadre: t.impostazioni.squadre.clone() }
                                button { class: "primario", onclick: move |_| finestra.set(None), "Torna al tavolo" }
                            }
                        }
                    }
                },
                Some(Finestra::Giocate) => rsx! {
                    Giocate { vista: vista.clone(), obs: obs.clone(), chiudi: move |_| finestra.set(None) }
                },
                None => rsx! {},
            }
        }
    }
}

fn imposta_titolo(titolo: &str) {
    if let Some(d) = web_sys::window().and_then(|w| w.document())
        && d.title() != titolo
    {
        d.set_title(titolo);
    }
}

#[component]
fn Punteggi(vista: Vista, obs: Osservazione, apri: EventHandler<Finestra>) -> Element {
    let imp = &vista.tavolo.impostazioni;
    let mano = if obs.regole.margherita_hack {
        format!("Mano {} di 4", obs.mano_n)
    } else {
        format!("Mano {} · a {}", obs.mano_n, obs.regole.punti_vittoria)
    };
    let turno =
        (obs.fase != Fase::FineMano && obs.turno > 0).then(|| format!("turno {} di 3", obs.turno));
    let c_e_riepilogo = !obs.storico_mani.is_empty();
    rsx! {
        div { class: "punteggi",
            div { class: "squadra sq0", span { "{imp.squadre[0]}" } b { "{obs.punteggi[0]}" } }
            div { class: "info",
                b { "{obs.regole.descrizione()}" }
                span { "{mano}" if let Some(t) = turno { " · {t}" } }
                div { class: "info-azioni",
                    button { class: "icona", title: "Regole di questa partita", "aria-label": "Regole di questa partita", onclick: move |_| apri.call(Finestra::Guida), "Regole" }
                    if c_e_riepilogo {
                        button { class: "icona", title: "Riepilogo dell'ultima mano", onclick: move |_| apri.call(Finestra::Riepilogo), "Ultima mano" }
                    }
                }
            }
            div { class: "squadra sq1", b { "{obs.punteggi[1]}" } span { "{imp.squadre[1]}" } }
        }
    }
}

/// One of the other three players: name, cards, and what they last did this turn.
#[component]
fn Giocatore(vista: Vista, obs: Osservazione, seat: Seat) -> Element {
    let s = usize::from(seat);
    let nome = vista.tavolo.nome_posto(seat);
    let avatar = avatar_posto(&vista, s);
    let altro = obs.altri.get(&seat);
    let n = altro.map_or(0, |a| a.n);
    let scoperte = altro.and_then(|a| a.carte.clone());
    let di_turno = obs.fase == Fase::Gioco && obs.corrente == seat;
    let bot = vista.tavolo.e_bot(seat);
    let scollegato =
        matches!(&vista.tavolo.posti[s], Some(Posto::Umano { id }) if !vista.connessi.contains(id));
    let classe = format!(
        "giocatore sq{} lato-{}{}{}",
        seat % 2,
        LATI[usize::from(relativo(seat, obs.seat))],
        if di_turno { " di-turno" } else { "" },
        if scollegato { " scollegato" } else { "" }
    );
    rsx! {
        div { class: "{classe}", "data-seat": "{seat}",
            div { class: "chi",
                span { class: "avatar-piccolo", "{avatar}" }
                span { class: "nome", "{nome}" }
                if obs.mazziere == seat { span { class: "tag", title: "Mazziere", "mazziere" } }
                if obs.che_palo.contains(&seat) { span { class: "tag palo", "Che palo!" } }
                if obs.fase == Fase::Scambio && obs.scambio.passati[s] {
                    span { class: "tag", if obs.scambio.veloci[s] { "passata veloce" } else { "passata" } }
                }
            }
            if di_turno {
                span { class: "segnaturno", if bot { "pensa" } else { "sta giocando" } span { class: "puntini-attesa" } }
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
            UltimaGiocata { g: obs.ultima_di(seat).cloned() }
        }
    }
}

/// What a seat played last in this turn: the card and what it took.
#[component]
fn UltimaGiocata(g: Option<GiocataMano>) -> Element {
    let Some(GiocataMano(_, carta, presa)) = g else {
        return rsx! {};
    };
    let esito = if presa.is_empty() {
        "in tavola".to_string()
    } else {
        format!("prende {}", presa.len())
    };
    rsx! {
        div { class: "ultima-giocata", title: "Ultima giocata: {carta.nome()}, {esito}",
            Carta { carta, classe: "pollice", traccia: false }
            span { "{esito}" }
        }
    }
}

#[component]
fn Centro(vista: Vista, obs: Osservazione, apri: EventHandler<Finestra>) -> Element {
    let ultima = obs.giocate_mano.last().cloned();
    let verso =
        (obs.fase == Fase::Gioco).then(|| LATI[usize::from(relativo(obs.corrente, obs.seat))]);
    rsx! {
        div { class: "tavola", "data-verso": verso,
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
        if let Some(GiocataMano(s, carta, presa)) = ultima {
            button { class: "ultima", title: "Tutte le giocate della mano", onclick: move |_| apri.call(Finestra::Giocate),
                span { class: "sq{s % 2}", "{vista.tavolo.nome_posto(s)}" }
                " ha giocato {carta.nome()}"
                if !presa.is_empty() { " e preso {nomi(&presa)}" }
                span { class: "freccia", "Tutte le giocate della mano" }
            }
        }
    }
}

/// Every play of this hand, newest turn first, so nobody loses track of who played what.
#[component]
fn Giocate(vista: Vista, obs: Osservazione, chiudi: EventHandler<()>) -> Element {
    let turni: Vec<(usize, Vec<GiocataMano>)> = obs
        .giocate_mano
        .chunks(GIOCATE_PER_TURNO)
        .map(<[GiocataMano]>::to_vec)
        .enumerate()
        .collect();
    rsx! {
        div { class: "velo", onclick: move |_| chiudi.call(()),
            div { class: "finestra giocate", onclick: move |e| e.stop_propagation(),
                h2 { "Giocate della mano {obs.mano_n}" }
                if !obs.carte_iniziali_tavola.is_empty() {
                    p { class: "nota", "In tavola all'inizio: {nomi(&obs.carte_iniziali_tavola)}" }
                }
                for (i, giocate) in turni.into_iter().rev() {
                    h3 { "Turno {i + 1}" }
                    ol { class: "elenco-giocate",
                        for GiocataMano(s, carta, presa) in giocate {
                            li {
                                span { class: "chi sq{s % 2}", "{avatar_posto(&vista, usize::from(s))} {vista.tavolo.nome_posto(s)}" }
                                Carta { carta, classe: "pollice", traccia: false }
                                span {
                                    if presa.is_empty() {
                                        "{carta.nome()}, resta in tavola"
                                    } else {
                                        "{carta.nome()}, prende {nomi(&presa)}"
                                    }
                                }
                            }
                        }
                    }
                }
                button { class: "primario", onclick: move |_| chiudi.call(()), "Torna al tavolo" }
            }
        }
    }
}

/// Whose turn it is (big when it is mine), and the countdown.
#[component]
fn Turno(vista: Vista, st: GameState, obs: Osservazione, seat: Seat) -> Element {
    let ctx = use_context::<Ctx>();
    let imp = &vista.tavolo.impostazioni;
    let miei = vista.tavolo.posti_di(&vista.io);
    let mio = st.fase == Fase::Gioco && miei.contains(&st.corrente);
    let (testo, durata) = match st.fase {
        Fase::Scambio => {
            let passata = st.scambio_passata[usize::from(seat)].is_some();
            (
                if passata {
                    "Scambio: aspettiamo gli altri".to_string()
                } else {
                    format!(
                        "Scambio: scegli la carta per {}",
                        vista.tavolo.nome_posto((seat + 2) % 4)
                    )
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
            if mio {
                ("Tocca a te".to_string(), durata)
            } else {
                let chi = st.corrente;
                let verbo = if vista.tavolo.e_bot(chi) {
                    "pensa"
                } else {
                    "sta giocando"
                };
                (
                    format!(
                        "{} {} {verbo}",
                        avatar_posto(&vista, usize::from(chi)),
                        vista.tavolo.nome_posto(chi)
                    ),
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
    let altro = st.fase == Fase::Gioco && !mio;
    let classe = format!(
        "turno{}{}",
        if mio { " mio" } else { "" },
        if altro {
            format!(" verso-{}", LATI[usize::from(relativo(st.corrente, seat))])
        } else {
            String::new()
        },
    );
    rsx! {
        div { class: "{classe}", role: "status", "aria-live": "polite",
            if !miei.is_empty() {
                UltimaGiocata { g: obs.ultima_di(seat).cloned() }
            }
            span { class: "chi-gioca", "{testo}" }
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
    let mut da_confermare = use_signal(|| None::<Azione>);
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
    // Bazzica calls and "che palo": buttons above the hand
    let altre: Vec<Azione> = legali
        .iter()
        .filter(|a| matches!(a, Azione::Bazzica { .. } | Azione::ChePalo { .. }))
        .cloned()
        .collect();
    // a card chosen ahead of time stays chosen until it leaves the hand
    if scelta().is_some_and(|c| !mano.contains(&c)) {
        scelta.set(None);
    }
    if da_confermare().is_some_and(|a| !legali.contains(&a)) {
        da_confermare.set(None);
    }
    let opzioni = scelta().map(giocabili).unwrap_or_default();
    // cards can be picked during the play even when it is someone else's turn
    let si_sceglie = attivo
        && match st.fase {
            Fase::Gioco => true,
            Fase::Scambio => legali.iter().any(|a| matches!(a, Azione::Passa { .. })),
            _ => false,
        };
    let mut fai = move |a: Azione| {
        scelta.set(None);
        da_confermare.set(None);
        ctx.richiedi(Richiesta::Azione(a));
    };
    let ricevuta = st.scambio_ricevuta[usize::from(seat)];
    let in_attesa = scelta().is_some() && opzioni.is_empty() && st.fase == Fase::Gioco;
    // while a choice is open its panel takes the place of the extra buttons
    let scegliendo = da_confermare().is_some() || !opzioni.is_empty();
    rsx! {
        div { class: "mano",
            if !altre.is_empty() && !scegliendo {
                div { class: "azioni-extra",
                    for a in altre {
                        button {
                            class: if matches!(a, Azione::ChePalo { .. }) { "speciale palo" } else { "speciale" },
                            onclick: {
                                let a = a.clone();
                                move |_| {
                                    if matches!(a, Azione::Bazzica { .. }) {
                                        scelta.set(None);
                                        da_confermare.set(Some(a.clone()));
                                    } else {
                                        fai(a.clone());
                                    }
                                }
                            },
                            {etichetta(&st, &a)}
                        }
                    }
                }
            }
            if let Some(a) = da_confermare() {
                div { class: "opzioni", role: "dialog",
                    p { class: "conferma", b { {etichetta(&st, &a)} } }
                    p { class: "nota", "Chiamandola giochi a carte scoperte fino alla fine del turno." }
                    button { class: "opzione", onclick: { let a = a.clone(); move |_| fai(a.clone()) }, "Chiama Bazzica" }
                    button { class: "secondario", onclick: move |_| da_confermare.set(None), "Non ancora" }
                }
            } else if !opzioni.is_empty() {
                div { class: "opzioni", role: "dialog",
                    p { class: "nota",
                        if opzioni.len() == 1 {
                            "Tocca di nuovo la carta o il pulsante per confermare."
                        } else if st.fase == Fase::Scambio {
                            "Come la passi?"
                        } else {
                            "Scegli cosa prendere."
                        }
                    }
                    for a in opzioni.iter().cloned() {
                        button {
                            class: "opzione",
                            onclick: { let a = a.clone(); move |_| fai(a.clone()) },
                            {etichetta(&st, &a)}
                        }
                    }
                    button { class: "secondario", onclick: move |_| scelta.set(None), "Cambia carta" }
                }
            }
            if in_attesa {
                p { class: "suggerimento", "Carta scelta: quando tocca a te ti chiedo conferma." }
            }
            if !scegliendo {
                Chiacchiere { st: st.clone(), seat }
            }
            div { class: "carte-mano", "data-seat": "{seat}",
                for c in mano {
                    Carta {
                        carta: c,
                        classe: format!(
                            "in-mano{}{}",
                            if scelta() == Some(c) { " scelta" } else { "" },
                            if Some(c) == ricevuta { " ricevuta" } else { "" },
                        ),
                        onclick: si_sceglie.then(|| {
                            let azioni = giocabili(c);
                            EventHandler::new(move |_| {
                                da_confermare.set(None);
                                match (scelta() == Some(c), azioni.as_slice()) {
                                    // second tap on the chosen card confirms its only play
                                    (true, [unica]) => fai(unica.clone()),
                                    (true, _) => scelta.set(None),
                                    (false, _) => scelta.set(Some(c)),
                                }
                            })
                        }),
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
            button { class: "piccolo chiacchiera", "aria-label": "Frasi veloci", onclick: move |_| aperto.toggle(), "💬" }
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
                format!("Gioca {} e prendi {}", carta.nome(), nomi(presa))
            };
            if let Some(ant) = st.anteprima(a) {
                let e = &ant.esito;
                for (si, t) in [
                    (e.scopa, "Scopa!"),
                    (e.piccola, "Piccola!"),
                    (e.grande, "Grande!"),
                    (!e.assi.is_empty(), "asso a Scopa"),
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
    rsx! {
        table { class: "riepilogo",
            thead { tr { th {} th { class: "sq0", "{squadre[0]}" } th { class: "sq1", "{squadre[1]}" } } }
            tbody {
                for riga in r.righe() {
                    tr {
                        class: match riga.peso {
                            Peso::Voce => "",
                            Peso::Dettaglio => "dettaglio",
                            Peso::Subtotale => "subtotale",
                            Peso::Totale => "totale",
                        },
                        td { "{riga.nome}" }
                        td { "{riga.valori[0]}" }
                        td { "{riga.valori[1]}" }
                    }
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i_lati_seguono_chi_guarda() {
        // seen from seat 2: seat 3 sits on the left, seat 0 across, seat 1 on the right
        let lati: Vec<&str> = (0..4).map(|s| LATI[usize::from(relativo(s, 2))]).collect();
        assert_eq!(lati, ["alto", "dx", "basso", "sx"]);
        for io in 0..4 {
            assert_eq!(relativo(io, io), 0);
        }
    }
}
