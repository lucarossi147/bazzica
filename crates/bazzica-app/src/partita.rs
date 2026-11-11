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
use crate::premossa::{self, Intento, Prenotazione, Scelte};
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
    let ctx = use_context::<Ctx>();
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
    // the card being chosen and the plays booked ahead: drawn by both the hand and the table
    let scelte = use_signal(Scelte::default);
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
            if vista.host.is_none() && !finita {
                div { class: "banner", "{crate::SENZA_REGIA}" }
            }
            if !vista.disconnessi.is_empty() && !finita {
                Pausa { vista: vista.clone() }
            }
            for posto in seggi_da_riprendere(t, &vista.io) {
                div { class: "banner",
                    "Un bot gioca al tuo posto. "
                    button {
                        class: "piccolo",
                        onclick: move |_| ctx.richiedi(Richiesta::Riprendi { posto }),
                        "Riprendi il posto"
                    }
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
                    Centro {
                        vista: vista.clone(),
                        obs: obs.clone(),
                        st: st.clone(),
                        scelte,
                        interattivo: !spettatore && !finita && vista.disconnessi.is_empty(),
                        apri: move |f| finestra.set(Some(f)),
                    }
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
                    scelte,
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

/// What the hand and the table need to know about the card being chosen.
struct Quadro {
    /// The choices that still apply to this deal.
    sc: Scelte,
    /// The position as the player will find it (with the booked Bazzica already called).
    previsto: GameState,
    /// Card picked from the hand during the play.
    carta: Option<Card>,
    /// What that card can take.
    opzioni: Vec<Vec<Card>>,
    /// Table cards picked so far.
    presa: Vec<Card>,
}

fn quadro(st: &GameState, seat: Seat, scelte: &Scelte) -> Quadro {
    let sc = scelte.valide(st, seat);
    let previsto = premossa::stato_previsto(st, seat, sc.bazzica);
    let carta = sc.carta.filter(|_| st.fase == Fase::Gioco);
    let opzioni = carta
        .map(|c| premossa::opzioni(&previsto, seat, c))
        .unwrap_or_default();
    let presa = premossa::presa_valida(&opzioni, &sc.presa);
    Quadro {
        sc,
        previsto,
        carta,
        opzioni,
        presa,
    }
}

#[component]
fn Centro(
    vista: Vista,
    obs: Osservazione,
    st: GameState,
    scelte: Signal<Scelte>,
    interattivo: bool,
    apri: EventHandler<Finestra>,
) -> Element {
    let ultima = obs.giocate_mano.last().cloned();
    let verso =
        (obs.fase == Fase::Gioco).then(|| LATI[usize::from(relativo(obs.corrente, obs.seat))]);
    let q = interattivo.then(|| quadro(&st, obs.seat, &scelte.read()));
    // while a card that can take something is picked, the table shows what it may take
    let scegliendo = q
        .as_ref()
        .is_some_and(|q| q.carta.is_some() && !premossa::solo_posa(&q.opzioni));
    let prendibili = q
        .as_ref()
        .map(|q| premossa::prendibili(&q.opzioni))
        .unwrap_or_default();
    let libera = q.as_ref().is_some_and(|q| q.opzioni.len() > 1);
    let prenotate: Vec<Card> = match q.as_ref().and_then(|q| q.sc.prenotata.as_ref()) {
        Some(Prenotazione {
            intento: Intento::Prendi(v),
            ..
        }) => v.clone(),
        Some(Prenotazione {
            intento: Intento::Sparecchia,
            ..
        }) => obs.tavola.clone(),
        _ => vec![],
    };
    let classe_tavola = if scegliendo {
        "tavola scegliendo"
    } else {
        "tavola"
    };
    rsx! {
        div { class: "{classe_tavola}", "data-verso": verso,
            if obs.tavola.is_empty() {
                span { class: "vuota", "tavola vuota" }
            }
            for c in obs.tavola.iter().copied() {
                Carta {
                    key: "{c}",
                    carta: c,
                    classe: {
                        let stato = match &q {
                            Some(q) if scegliendo && q.presa.contains(&c) => " presa-scelta",
                            Some(_) if scegliendo && prendibili.contains(&c) => " prendibile",
                            Some(_) if scegliendo => " spenta",
                            _ if prenotate.contains(&c) => " prenotata",
                            _ => "",
                        };
                        format!("in-tavola{stato}")
                    },
                    onclick: q
                        .as_ref()
                        .filter(|_| scegliendo && libera && prendibili.contains(&c))
                        .map(|q| {
                            let (sc, opzioni, presa) = (q.sc.clone(), q.opzioni.clone(), q.presa.clone());
                            let mut scelte = scelte;
                            EventHandler::new(move |_| {
                                scelte.set(Scelte {
                                    presa: premossa::tocca_tavola(&opzioni, &presa, c),
                                    cambiata: false,
                                    ..sc.clone()
                                });
                            })
                        }),
                }
            }
        }
        // while choosing what to take the table needs all the room there is
        div { class: if scegliendo { "dettagli nascosti" } else { "dettagli" },
            span { class: "pila mazzo", title: "Carte ancora nel mazzo",
                span { class: "pila-nome", "Mazzo" }
                b { "{obs.carte_nel_mazzo}" }
            }
            for sq in 0..2usize {
                span { class: "pila sq{sq}", "data-prese": "{sq}", title: "Prese di {vista.tavolo.impostazioni.squadre[sq]}",
                    span { class: "pila-nome", "Prese" }
                    b { "{obs.prese[sq].n}" }
                    if !obs.prese[sq].scoperte.is_empty() {
                        span { class: "pila-scope", title: "Scope segnate", "{obs.prese[sq].scoperte.len()} sc." }
                    }
                    if obs.credito[sq] > 0 {
                        span { class: "pila-credito", title: "Scope di Bazzica ancora da segnare", "+{obs.credito[sq]} cred." }
                    }
                }
            }
        }
        if let Some(GiocataMano(s, carta, presa)) = ultima.filter(|_| !scegliendo) {
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
    let miei = vista.tavolo.posti_di(&vista.io);
    let mio = st.fase == Fase::Gioco && miei.contains(&st.corrente);
    let testo = match st.fase {
        Fase::Scambio => {
            if st.scambio_passata[usize::from(seat)].is_some() {
                "Scambio: aspettiamo gli altri".to_string()
            } else {
                format!(
                    "Scambio: scegli la carta per {}",
                    vista.tavolo.nome_posto((seat + 2) % 4)
                )
            }
        }
        Fase::Gioco => {
            if mio {
                "Tocca a te".to_string()
            } else {
                let chi = st.corrente;
                let verbo = if vista.tavolo.e_bot(chi) {
                    "pensa"
                } else {
                    "sta giocando"
                };
                format!(
                    "{} {} {verbo}",
                    avatar_posto(&vista, usize::from(chi)),
                    vista.tavolo.nome_posto(chi)
                )
            }
        }
        Fase::FineMano => "Fine della mano".to_string(),
        Fase::FinePartita => "Partita finita".to_string(),
    };
    // the host's own deadline (also after a takeover), not when this device saw the turn change
    let adesso = *ctx.ora.read();
    let restanti = ctx
        .conn
        .peek()
        .as_ref()
        .and_then(|c| crate::secondi_timer(c, adesso))
        .filter(|_| vista.disconnessi.is_empty())
        .map(|(_, _, secondi)| secondi);
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

/// A decision taken in the hand: what to remember and, if it is my turn, what to play.
#[derive(Clone, PartialEq)]
struct Mossa {
    azione: Option<Azione>,
    dopo: Scelte,
}

fn esegui(ctx: Ctx, mut scelte: Signal<Scelte>, m: Mossa) {
    scelte.set(m.dopo);
    if let Some(a) = m.azione {
        ctx.richiedi(Richiesta::Azione(a));
    }
}

/// The game as this device knows it right now (outside a render).
fn partita_ora(ctx: &Ctx) -> Option<GameState> {
    let c = ctx.conn.peek().clone()?;
    let s = c.sessione.borrow();
    s.nodo().tavolo()?.partita.clone()
}

/// Pause before a booked play starts, so that the play before it can be seen.
const PAUSA_PRENOTATA_MS: u64 = 450;

#[component]
fn Mano(st: GameState, seat: Seat, attivo: bool, scelte: Signal<Scelte>) -> Element {
    let ctx = use_context::<Ctx>();
    let mut da_confermare = use_signal(|| None::<Azione>);
    let legali = if attivo {
        st.legal_actions(seat)
    } else {
        vec![]
    };
    let mano = st.mani[usize::from(seat)].clone();
    let q = quadro(&st, seat, &scelte.read());
    let sc = q.sc.clone();
    let in_gioco = attivo && st.fase == Fase::Gioco;
    let mio_turno = in_gioco && st.corrente == seat;
    // with a Bazzica about to be called by itself, the card waits for it
    let subito = mio_turno && sc.bazzica.is_none();

    // booked plays: made by themselves when the turn comes, if they still mean the same
    let automatica = attivo
        .then(|| premossa::automatica(&st, seat, &sc))
        .flatten();
    let ripiego = (attivo && premossa::saltata(&st, seat, &sc)).then(|| Scelte {
        carta: sc.prenotata.as_ref().map(|p| p.carta),
        presa: vec![],
        prenotata: None,
        cambiata: true,
        ..sc.clone()
    });
    use_effect(use_reactive!(|(automatica, ripiego)| {
        let mut scelte = scelte;
        if let Some(nuove) = ripiego {
            scelte.set(nuove);
        } else if let Some(a) = automatica {
            spawn(async move {
                futures_timer::Delay::new(std::time::Duration::from_millis(PAUSA_PRENOTATA_MS))
                    .await;
                // still booked (not cancelled meanwhile) and still the same play
                let Some(ora) = partita_ora(&ctx) else { return };
                let valide = scelte.peek().valide(&ora, seat);
                if premossa::automatica(&ora, seat, &valide).as_ref() != Some(&a) {
                    return;
                }
                let dopo = match a {
                    Azione::Bazzica { .. } => Scelte {
                        bazzica: None,
                        ..valide
                    },
                    _ => valide.vuote(),
                };
                esegui(
                    ctx,
                    scelte,
                    Mossa {
                        azione: Some(a),
                        dopo,
                    },
                );
            });
        }
    }));

    // what confirming `carta` on `presa` does: play it now, or book it for my turn
    let mossa = |carta: Card, presa: &[Card]| -> Mossa {
        if subito {
            Mossa {
                azione: Some(Azione::Gioca {
                    seat,
                    carta,
                    presa: presa.to_vec(),
                }),
                dopo: sc.vuote(),
            }
        } else {
            Mossa {
                azione: None,
                dopo: Scelte {
                    carta: None,
                    presa: vec![],
                    prenotata: Some(Prenotazione {
                        carta,
                        intento: premossa::intento(&q.previsto, seat, carta, presa),
                    }),
                    cambiata: false,
                    ..sc.clone()
                },
            }
        }
    };

    // the exchange: how to pass the picked card
    let passate: Vec<Azione> = sc
        .carta
        .filter(|_| st.fase == Fase::Scambio)
        .map(|c| {
            legali
                .iter()
                .filter(|a| matches!(a, Azione::Passa { carta, .. } if *carta == c))
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    // Bazzica calls (also bookable before my turn) and "che palo": buttons above the hand
    let mut altre = azioni_extra(&legali, st.fase);
    if sc.bazzica.is_some() {
        altre.retain(|a| !matches!(a, Azione::Bazzica { .. }));
    } else if in_gioco && !mio_turno {
        for o in bazzica_core::bazzica::opzioni_bazzica(&mano, &st.regole) {
            if premossa::bazzica_chiamabile(&st, seat, o.sette) {
                altre.push(Azione::Bazzica {
                    seat,
                    sette: o.sette,
                });
            }
        }
    }
    altre.sort_by_key(|a| !matches!(a, Azione::Bazzica { .. }));
    let conferma_bazzica = da_confermare().filter(|a| {
        matches!(a, Azione::Bazzica { sette, .. } if in_gioco && sc.bazzica.is_none()
            && premossa::bazzica_chiamabile(&st, seat, *sette))
    });

    let si_sceglie = attivo
        && match st.fase {
            Fase::Gioco => true,
            Fase::Scambio => legali.iter().any(|a| matches!(a, Azione::Passa { .. })),
            _ => false,
        };
    let ricevuta = st.scambio_ricevuta[usize::from(seat)];
    let decisa = premossa::decisa(&q.opzioni, &q.presa).cloned();
    // while a choice is open its panel takes the place of the extra buttons
    let scegliendo = conferma_bazzica.is_some() || q.carta.is_some() || !passate.is_empty();
    let prenotata = sc.prenotata.clone();
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
                                        da_confermare.set(Some(a.clone()));
                                    } else {
                                        ctx.richiedi(Richiesta::Azione(a.clone()));
                                    }
                                }
                            },
                            {etichetta(&st, &a)}
                        }
                    }
                }
            }
            if let Some(a) = conferma_bazzica {
                div { class: "opzioni", role: "dialog",
                    p { class: "conferma", b { {etichetta(&st, &a)} } }
                    p { class: "nota",
                        "Chiamandola giochi a carte scoperte fino alla fine del turno."
                        if !mio_turno { " Non tocca ancora a te: la chiamo io appena arriva il tuo turno." }
                    }
                    button {
                        class: "opzione decisa",
                        onclick: {
                            let (a, sc) = (a.clone(), sc.clone());
                            move |_| {
                                da_confermare.set(None);
                                let Azione::Bazzica { sette, .. } = a else { return };
                                let m = if mio_turno {
                                    Mossa { azione: Some(a.clone()), dopo: sc.clone() }
                                } else {
                                    // the 7 bello and the aces may take something else now
                                    Mossa {
                                        azione: None,
                                        dopo: Scelte { bazzica: Some(sette), presa: vec![], ..sc.clone() },
                                    }
                                };
                                esegui(ctx, scelte, m);
                            }
                        },
                        if mio_turno { "Chiama Bazzica" } else { "Prenota la Bazzica" }
                    }
                    button { class: "secondario", onclick: move |_| da_confermare.set(None), "Non ancora" }
                }
            } else if let Some(carta) = q.carta {
                div { class: "opzioni", role: "dialog",
                    if sc.cambiata {
                        p { class: "nota cambiata", "La tavola è cambiata: la giocata prenotata non vale più. Scegli di nuovo." }
                    }
                    p { class: "nota",
                        if !subito { "Non tocca ancora a te: prenota la giocata e parte da sola al tuo turno. " }
                        if q.opzioni.len() > 1 {
                            "Scegli cosa prendere: tocca le carte in tavola o una combinazione."
                        } else if subito {
                            "Tocca di nuovo la carta o il pulsante per giocarla."
                        }
                    }
                    for presa in q.opzioni.iter().cloned() {
                        OpzionePresa {
                            st: q.previsto.clone(),
                            seat,
                            carta,
                            presa: presa.clone(),
                            decisa: decisa.as_ref() == Some(&presa),
                            subito,
                            scegli: {
                                let m = mossa(carta, &presa);
                                move |_| esegui(ctx, scelte, m.clone())
                            },
                        }
                    }
                    button {
                        class: "secondario",
                        onclick: {
                            let sc = sc.clone();
                            let mut scelte = scelte;
                            move |_| scelte.set(Scelte { carta: None, presa: vec![], cambiata: false, ..sc.clone() })
                        },
                        "Cambia carta"
                    }
                }
            } else if !passate.is_empty() {
                div { class: "opzioni", role: "dialog",
                    p { class: "nota", "Come la passi?" }
                    for a in passate.iter().cloned() {
                        button {
                            class: "opzione",
                            onclick: {
                                let m = Mossa { azione: Some(a.clone()), dopo: sc.vuote() };
                                move |_| esegui(ctx, scelte, m.clone())
                            },
                            {etichetta(&st, &a)}
                        }
                    }
                    button {
                        class: "secondario",
                        onclick: {
                            let sc = sc.clone();
                            let mut scelte = scelte;
                            move |_| scelte.set(Scelte { carta: None, ..sc.clone() })
                        },
                        "Cambia carta"
                    }
                }
            }
            if !scegliendo && (prenotata.is_some() || sc.bazzica.is_some()) {
                div { class: "prenotazioni", role: "status",
                    if let Some(sette) = sc.bazzica {
                        div { class: "prenotazione",
                            span { class: "clessidra", "⏳" }
                            span { class: "cosa",
                                "Bazzica prenotata"
                                if let Some(v) = sette { " (7 bello = {v})" }
                            }
                            button {
                                class: "piccolo secondario",
                                onclick: {
                                    let sc = sc.clone();
                                    let mut scelte = scelte;
                                    move |_| scelte.set(Scelte { bazzica: None, ..sc.clone() })
                                },
                                "Annulla"
                            }
                        }
                    }
                    if let Some(p) = prenotata.clone() {
                        div { class: "prenotazione",
                            span { class: "clessidra", "⏳" }
                            span { class: "cosa",
                                "Prenotata: "
                                Carta { carta: p.carta, classe: "pollice", traccia: false }
                                match &p.intento {
                                    Intento::Posa => rsx! { " resta in tavola" },
                                    Intento::Sparecchia => rsx! { " piglia tutto" },
                                    Intento::Prendi(v) => rsx! {
                                        " prende "
                                        for c in v.iter().copied() {
                                            Carta { carta: c, classe: "pollice", traccia: false }
                                        }
                                    },
                                }
                            }
                            button {
                                class: "piccolo secondario",
                                onclick: {
                                    let sc = sc.clone();
                                    let mut scelte = scelte;
                                    move |_| scelte.set(Scelte { prenotata: None, ..sc.clone() })
                                },
                                "Annulla"
                            }
                        }
                    }
                }
            }
            if !scegliendo {
                Chiacchiere { st: st.clone(), seat }
            }
            div { class: "carte-mano", "data-seat": "{seat}",
                for c in mano {
                    Carta {
                        key: "{c}",
                        carta: c,
                        classe: format!(
                            "in-mano{}{}{}",
                            if sc.carta == Some(c) { " scelta" } else { "" },
                            if prenotata.as_ref().is_some_and(|p| p.carta == c) { " prenotata" } else { "" },
                            if Some(c) == ricevuta { " ricevuta" } else { "" },
                        ),
                        onclick: si_sceglie.then(|| {
                            let sc = sc.clone();
                            let scelta = sc.carta == Some(c);
                            let era_prenotata = prenotata.as_ref().is_some_and(|p| p.carta == c);
                            // a second tap on the picked card confirms what has been decided
                            let conferma = (scelta && q.carta == Some(c))
                                .then(|| decisa.as_ref().map(|presa| mossa(c, presa)))
                                .flatten();
                            let mut scelte = scelte;
                            EventHandler::new(move |_| {
                                da_confermare.set(None);
                                if era_prenotata {
                                    scelte.set(Scelte { prenotata: None, cambiata: false, ..sc.clone() });
                                } else if let Some(m) = conferma.clone() {
                                    esegui(ctx, scelte, m);
                                } else if scelta {
                                    scelte.set(Scelte { carta: None, presa: vec![], cambiata: false, ..sc.clone() });
                                } else {
                                    scelte.set(Scelte {
                                        carta: Some(c),
                                        presa: vec![],
                                        prenotata: None,
                                        cambiata: false,
                                        ..sc.clone()
                                    });
                                }
                            })
                        }),
                    }
                }
            }
        }
    }
}

/// One thing the picked card can do, drawn with the cards themselves.
#[component]
fn OpzionePresa(
    st: GameState,
    seat: Seat,
    carta: Card,
    presa: Vec<Card>,
    decisa: bool,
    subito: bool,
    scegli: EventHandler<()>,
) -> Element {
    let e = premossa::esito(&st, seat, carta, &presa);
    let tutto = matches!(
        premossa::intento(&st, seat, carta, &presa),
        Intento::Sparecchia
    );
    let descrizione = etichetta(
        &st,
        &Azione::Gioca {
            seat,
            carta,
            presa: presa.clone(),
        },
    );
    let valore = st.valore_effettivo(seat, carta);
    rsx! {
        button {
            class: if decisa { "opzione decisa" } else { "opzione" },
            title: "{descrizione}",
            "aria-label": "{descrizione}",
            onclick: move |_| scegli.call(()),
            span { class: "opz-carte",
                Carta { carta, classe: "pollice", traccia: false }
                if valore != carta.val() { span { class: "opz-vale", "vale {valore}" } }
                if presa.is_empty() {
                    span { class: "opz-verbo", "resta in tavola" }
                } else {
                    span { class: "opz-verbo", if tutto { "piglia tutto" } else { "prende" } }
                    for c in presa.iter().copied() {
                        Carta { carta: c, classe: "pollice", traccia: false }
                    }
                    // thumbnails of the same value look alike: spell the cards out
                    span { class: "opz-nomi", "{nomi(&presa)}" }
                }
            }
            span { class: "opz-esito",
                if e.scopa { span { class: "premio", "Scopa!" } }
                if e.piccola { span { class: "premio", "Piccola!" } }
                if e.grande { span { class: "premio", "Grande!" } }
                if !e.assi.is_empty() { span { class: "premio", "asso a Scopa" } }
                span { class: "opz-fai", if subito { "Gioca" } else { "Prenota" } }
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
            let Azione::Gioca { seat, .. } = a else {
                return s;
            };
            let e = premossa::esito(st, *seat, *carta, presa);
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
                    if vista.puo(&Richiesta::Avvia) {
                        button { class: "primario", onclick: move |_| ctx.richiedi(Richiesta::Avvia), "Rivincita" }
                    }
                    if vista.puo(&Richiesta::Imposta(imp.clone())) {
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

/// Seats a bot took over from `io` during the game (it may take them back).
pub fn seggi_da_riprendere(t: &bazzica_net::Tavolo, io: &str) -> Vec<Seat> {
    (0..4u8)
        .filter(|&s| t.sostituiti[usize::from(s)].as_deref() == Some(io))
        .collect()
}

/// The game is paused for missing players; after a while each can be replaced by a bot.
#[component]
fn Pausa(vista: Vista) -> Element {
    let ctx = use_context::<Ctx>();
    // re-checked as time passes: replacing becomes possible after a delay
    let _ = ctx.ora.read();
    let sostituibili = ctx
        .conn
        .peek()
        .as_ref()
        .map(|c| c.sessione.borrow().nodo().sostituibili())
        .unwrap_or_default();
    let assenti = vista
        .disconnessi
        .iter()
        .map(|id| vista.nome(id))
        .collect::<Vec<_>>()
        .join(", ");
    rsx! {
        div { class: "banner",
            "In pausa: aspettiamo {assenti}"
            for id in sostituibili {
                for posto in vista.tavolo.posti_di(&id) {
                    button {
                        class: "piccolo",
                        onclick: move |_| ctx.richiedi(Richiesta::Sostituisci { posto }),
                        "Sostituisci {vista.nome(&id)} con un bot"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests_sostituzione {
    use super::*;
    use bazzica_net::{Impostazioni, Partecipante, Tavolo};

    #[test]
    fn solo_i_propri_posti_si_riprendono() {
        let io = Partecipante {
            id: "a".into(),
            nome: "A".into(),
            avatar: "🦊".into(),
        };
        let mut t = Tavolo::nuovo(io, Impostazioni::default());
        t.sostituiti = [None, Some("b".into()), None, Some("b".into())];
        assert_eq!(seggi_da_riprendere(&t, "b"), [1, 3]);
        assert!(seggi_da_riprendere(&t, "a").is_empty());
    }
}

/// The actions shown as buttons above the hand: Bazzica calls and "che palo". The engine lists
/// "che palo" as legal during the exchange too, but the host refuses it there: no button.
pub fn azioni_extra(legali: &[Azione], fase: Fase) -> Vec<Azione> {
    legali
        .iter()
        .filter(|a| match a {
            Azione::Bazzica { .. } => true,
            Azione::ChePalo { .. } => fase != Fase::Scambio,
            _ => false,
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests_azioni_extra {
    use super::*;

    #[test]
    fn niente_che_palo_durante_lo_scambio() {
        let legali = [
            Azione::ChePalo { seat: 0 },
            Azione::Bazzica {
                seat: 0,
                sette: None,
            },
            Azione::Attendi { seat: 0 },
        ];
        assert_eq!(azioni_extra(&legali, Fase::Gioco).len(), 2);
        assert_eq!(
            azioni_extra(&legali, Fase::Scambio),
            [Azione::Bazzica {
                seat: 0,
                sette: None
            }]
        );
    }
}
