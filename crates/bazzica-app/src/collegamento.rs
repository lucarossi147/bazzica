//! Joining a table, step by step: what worked, what is still going on, and what to do
//! when a step fails. Also the TURN relay setting that fixes the commonest failure.

use bazzica_rete::Turn;
use dioxus::prelude::*;

use crate::Ctx;
use crate::connessione::{salva_turn, turn_attivo, turn_salvato};

/// Seconds to wait before calling a step failed.
const PAZIENZA_PEER: u64 = 20;
const PAZIENZA_DIRETTO: u64 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Esito {
    Fatto,
    InCorso,
    Fallito,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Passo {
    pub esito: Esito,
    pub testo: String,
}

/// Everything the diagnosis looks at.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fatti {
    pub relay_totali: usize,
    pub relay_collegati: Option<usize>,
    pub peer_visti: usize,
    /// Live events ignored for a time far from this device's clock.
    pub scartati_orario: usize,
    pub peer_collegati: usize,
    pub versione_altrui: Option<u32>,
    pub secondi: u64,
    pub con_turn: bool,
}

fn passo(esito: Esito, testo: impl Into<String>) -> Passo {
    Passo {
        esito,
        testo: testo.into(),
    }
}

/// The steps reached so far; the last one is the current one (or the one that failed).
pub fn passi(f: &Fatti) -> Vec<Passo> {
    use Esito::*;
    let mut v = vec![];
    match f.relay_collegati {
        None => {
            v.push(passo(InCorso, "Contatto i server di incontro"));
            return v;
        }
        Some(0) => {
            v.push(passo(
                Fallito,
                "Nessun server di incontro raggiungibile: controlla la connessione a internet",
            ));
            return v;
        }
        Some(n) => v.push(passo(
            Fatto,
            format!("Server di incontro: {n} di {}", f.relay_totali),
        )),
    }
    if f.peer_visti == 0 {
        v.push(if f.secondi < PAZIENZA_PEER {
            passo(InCorso, "Cerco gli altri giocatori del tavolo")
        } else if f.scartati_orario > 0 {
            passo(
                Fallito,
                "L'orologio di questo dispositivo sembra sbagliato: controlla data e ora, poi premi Riprova",
            )
        } else {
            passo(
                Fallito,
                "Al tavolo non risponde nessuno: controlla il codice, e chi l'ha creato tenga l'app aperta in primo piano",
            )
        });
        return v;
    }
    v.push(passo(
        Fatto,
        if f.peer_visti == 1 {
            "Trovato un giocatore".to_string()
        } else {
            format!("Trovati {} giocatori", f.peer_visti)
        },
    ));
    if let Some(altra) = f.versione_altrui {
        v.push(passo(
            Fallito,
            format!(
                "Un giocatore ha un'altra versione dell'app ({altra}): ricaricate la pagina tutti"
            ),
        ));
        return v;
    }
    v.push(if f.peer_collegati > 0 {
        passo(Fatto, "Collegati: ricevo il tavolo")
    } else if f.secondi < PAZIENZA_DIRETTO {
        passo(InCorso, "Mi collego ai giocatori trovati")
    } else if f.con_turn {
        passo(
            Fallito,
            "Non riesco a collegarmi: chi ha creato il tavolo tenga l'app aperta in primo piano, poi premi Riprova. Se non basta, provate sulla stessa Wi-Fi o controllate il server TURN",
        )
    } else {
        passo(
            Fallito,
            "Non riesco a collegarmi direttamente: succede spesso tra reti diverse (dati mobili). Provate sulla stessa Wi-Fi, o impostate un server TURN qui sotto",
        )
    });
    v
}

/// True when a TURN relay is what is missing.
pub fn serve_turn(f: &Fatti) -> bool {
    passi(f).last().is_some_and(|p| {
        p.esito == Esito::Fallito && f.peer_visti > 0 && f.versione_altrui.is_none()
    })
}

/// What this device's connection has achieved, read from the live session.
pub fn fatti(ctx: Ctx) -> Option<Fatti> {
    let adesso = (ctx.ora)();
    let c = ctx.conn.read().clone()?;
    let s = c.sessione.borrow();
    let d = s.diagnosi();
    Some(Fatti {
        relay_totali: d.relay_totali,
        relay_collegati: d.relay_collegati,
        peer_visti: d.peer_visti,
        scartati_orario: d.scartati_orario,
        peer_collegati: s.peer_collegati(),
        versione_altrui: s.nodo().versione_altrui(),
        secondi: adesso.saturating_sub(*c.aperta.borrow()) / 1000,
        con_turn: s.turn().is_some(),
    })
}

#[component]
pub fn Attesa(codice: String) -> Element {
    let ctx = use_context::<Ctx>();
    let f = fatti(ctx).unwrap_or_default();
    let passi = passi(&f);
    let fallito = passi.last().is_some_and(|p| p.esito == Esito::Fallito);
    rsx! {
        div { class: "schermo centrato",
            h1 { class: "logo", "Bazzica" }
            div { class: "pannello",
                p { "Entro nel tavolo " b { "{codice}" } }
                ol { class: "passi-collegamento", "aria-live": "polite",
                    for p in passi {
                        li { class: match p.esito {
                                Esito::Fatto => "fatto",
                                Esito::InCorso => "in-corso",
                                Esito::Fallito => "fallito",
                            },
                            "{p.testo}"
                        }
                    }
                }
                if fallito {
                    button {
                        onclick: move |_| {
                            if let Some(c) = ctx.conn.peek().clone() {
                                c.riapri();
                            }
                        },
                        "Riprova"
                    }
                }
                details { open: serve_turn(&f),
                    summary { "Server TURN" }
                    ServerTurn {}
                }
                button { class: "secondario", onclick: move |_| ctx.esci(), "Annulla" }
            }
        }
    }
}

/// TURN relay of this device. Saved locally; applied to the open connection right away.
#[component]
pub fn ServerTurn() -> Element {
    let ctx = use_context::<Ctx>();
    let salvato = turn_salvato();
    let mut urls = use_signal(|| {
        salvato
            .as_ref()
            .map(|t| t.urls.join(" "))
            .unwrap_or_default()
    });
    let mut utente = use_signal(|| {
        salvato
            .as_ref()
            .map(|t| t.username.clone())
            .unwrap_or_default()
    });
    let mut segreto = use_signal(|| {
        salvato
            .as_ref()
            .map(|t| t.credential.clone())
            .unwrap_or_default()
    });
    let incorporato = Turn::di_compilazione().is_some();
    let applica = move |t: Option<Turn>| {
        salva_turn(t.as_ref());
        if let Some(c) = ctx.conn.peek().clone() {
            c.riapri_con(turn_attivo());
        }
    };
    let nuovo = Turn::nuovo(&urls(), &utente(), &segreto());
    rsx! {
        div { class: "server-turn",
            p { class: "nota",
                "Tra reti diverse i telefoni spesso non si vedono direttamente: un server TURN fa da ponte. "
                if incorporato {
                    "Questa versione ne ha già uno; qui puoi usarne un altro."
                } else {
                    "Inserisci i dati del tuo (stessi dati su tutti i telefoni)."
                }
            }
            label { "Indirizzi (turn: o turns:)" }
            input {
                value: "{urls}",
                placeholder: "turn:esempio.it:3478",
                autocapitalize: "none",
                oninput: move |e| urls.set(e.value()),
            }
            div { class: "riga",
                div {
                    label { "Utente" }
                    input { value: "{utente}", autocapitalize: "none", oninput: move |e| utente.set(e.value()) }
                }
                div {
                    label { "Password" }
                    input { r#type: "password", value: "{segreto}", oninput: move |e| segreto.set(e.value()) }
                }
            }
            div { class: "riga",
                button {
                    disabled: nuovo.is_none(),
                    onclick: move |_| {
                        applica(nuovo.clone());
                        ctx.avvisa("Server TURN salvato: mi ricollego");
                    },
                    "Usa questo server"
                }
                if salvato.is_some() {
                    button {
                        class: "secondario",
                        onclick: move |_| {
                            urls.set(String::new());
                            utente.set(String::new());
                            segreto.set(String::new());
                            applica(None);
                            ctx.avvisa("Server TURN rimosso");
                        },
                        "Rimuovi"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ultimo(f: &Fatti) -> Passo {
        passi(f).pop().unwrap()
    }

    fn con_relay() -> Fatti {
        Fatti {
            relay_totali: 7,
            relay_collegati: Some(3),
            ..Default::default()
        }
    }

    #[test]
    fn i_relay_vengono_prima_di_tutto() {
        assert_eq!(ultimo(&Fatti::default()).esito, Esito::InCorso);
        let nessuno = Fatti {
            relay_collegati: Some(0),
            ..Default::default()
        };
        assert_eq!(ultimo(&nessuno).esito, Esito::Fallito);
        assert!(!serve_turn(&nessuno));
        assert_eq!(passi(&con_relay())[0].testo, "Server di incontro: 3 di 7");
    }

    #[test]
    fn nessuno_al_tavolo_dopo_un_po_e_un_errore() {
        let mut f = con_relay();
        assert_eq!(ultimo(&f).esito, Esito::InCorso);
        f.secondi = PAZIENZA_PEER;
        assert_eq!(ultimo(&f).esito, Esito::Fallito);
        assert!(
            !serve_turn(&f),
            "the code may be wrong: TURN would not help"
        );
        f.scartati_orario = 3;
        assert!(
            ultimo(&f).testo.contains("orologio"),
            "a wrong clock is named"
        );
    }

    #[test]
    fn trovati_ma_non_collegati_serve_turn() {
        let mut f = Fatti {
            peer_visti: 2,
            ..con_relay()
        };
        assert_eq!(ultimo(&f).esito, Esito::InCorso);
        f.secondi = PAZIENZA_DIRETTO;
        assert!(serve_turn(&f));
        assert!(ultimo(&f).testo.contains("server TURN qui sotto"));
        f.con_turn = true;
        assert!(ultimo(&f).testo.contains("tenga l'app aperta"));
        f.peer_collegati = 1;
        assert_eq!(ultimo(&f).esito, Esito::Fatto);
        assert!(!serve_turn(&f));
    }

    #[test]
    fn versione_diversa() {
        let f = Fatti {
            peer_visti: 1,
            versione_altrui: Some(1),
            ..con_relay()
        };
        let p = ultimo(&f);
        assert_eq!(p.esito, Esito::Fallito);
        assert!(p.testo.contains("ricaricate"));
        assert!(!serve_turn(&f));
        assert_eq!(passi(&f)[1].testo, "Trovato un giocatore");
    }
}
