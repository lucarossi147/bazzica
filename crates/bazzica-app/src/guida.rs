//! The tutorial: chapters from `bazzica_core::guida`, one step at a time, with the examples
//! drawn as real cards. Optional: offered once per variant, always reachable from a button.

use bazzica_core::Variante;
use bazzica_core::cards::nomi;
use bazzica_core::guida::{self, CAPITOLI, Capitolo, Esempio, Presa, carta, carte};
use dioxus::prelude::*;

use crate::carte::Carta;
use crate::profilo::{leggi, scrivi};

/// Which chapters the tutorial shows and where it opens.
#[derive(Clone, PartialEq)]
pub struct Apertura {
    pub capitoli: Vec<&'static Capitolo>,
    pub inizio: usize,
}

impl Apertura {
    /// Every chapter, from the basics.
    pub fn tutto() -> Apertura {
        Apertura {
            capitoli: CAPITOLI.iter().collect(),
            inizio: 0,
        }
    }

    /// What a table plays, opened at the variant's own chapter (the basics stay one tap away).
    pub fn per(v: Variante, margherita: bool) -> Apertura {
        let capitoli = guida::percorso(v, margherita);
        let inizio = capitoli
            .iter()
            .position(|c| c.variante == Some(v))
            .unwrap_or(0);
        Apertura { capitoli, inizio }
    }
}

const CHIAVE_VISTE: &str = "bazzica.guide";

/// Variants whose tutorial this device has already been offered.
pub fn gia_offerta(v: Variante) -> bool {
    leggi(CHIAVE_VISTE).is_some_and(|s| s.split(',').any(|n| n == v.nome()))
}

pub fn segna_offerta(v: Variante) {
    if !gia_offerta(v) {
        let mut viste = leggi(CHIAVE_VISTE).unwrap_or_default();
        if !viste.is_empty() {
            viste.push(',');
        }
        viste.push_str(v.nome());
        scrivi(CHIAVE_VISTE, &viste);
    }
}

#[component]
pub fn Guida(apertura: Apertura, chiudi: EventHandler<()>) -> Element {
    let mut cap = use_signal(|| apertura.inizio);
    let mut passo = use_signal(|| 0usize);
    let capitoli = apertura.capitoli.clone();
    let c = capitoli[cap().min(capitoli.len() - 1)];
    let p = &c.passi[passo().min(c.passi.len() - 1)];
    let primo = cap() == 0 && passo() == 0;
    let ultimo = cap() + 1 == capitoli.len() && passo() + 1 == c.passi.len();
    let n_cap = capitoli.len();
    let lunghezze: Vec<usize> = capitoli.iter().map(|c| c.passi.len()).collect();
    let lunghezze2 = lunghezze.clone();
    let avanti = move |_| {
        if passo() + 1 < lunghezze[cap()] {
            passo += 1;
        } else if cap() + 1 < n_cap {
            cap += 1;
            passo.set(0);
        }
    };
    let indietro = move |_| {
        if passo() > 0 {
            passo -= 1;
        } else if cap() > 0 {
            cap -= 1;
            passo.set(lunghezze2[cap()] - 1);
        }
    };
    rsx! {
        div { class: "velo", onclick: move |_| chiudi.call(()),
            div {
                class: "finestra guida",
                role: "dialog",
                "aria-label": "Come si gioca",
                onclick: move |e| e.stop_propagation(),
                nav { class: "capitoli",
                    for (i, k) in capitoli.iter().enumerate() {
                        button {
                            class: if i == cap() { "capitolo scelto" } else { "capitolo" },
                            onclick: move |_| {
                                cap.set(i);
                                passo.set(0);
                            },
                            "{k.titolo}"
                        }
                    }
                }
                if passo() == 0 {
                    p { class: "intro", "{c.intro}" }
                }
                h2 { "{p.titolo}" }
                p { class: "testo", "{p.testo}" }
                if let Some(e) = &p.esempio {
                    VistaEsempio { esempio: e }
                }
                div { class: "avanzamento",
                    span { class: "nota", "{passo() + 1} di {c.passi.len()}" }
                    div { class: "puntini",
                        for i in 0..c.passi.len() {
                            span { class: if i == passo() { "puntino acceso" } else { "puntino" } }
                        }
                    }
                }
                div { class: "riga",
                    button { class: "secondario", disabled: primo, onclick: indietro, "Indietro" }
                    if ultimo {
                        button { class: "primario", onclick: move |_| chiudi.call(()), "Ho capito" }
                    } else {
                        button { class: "primario", onclick: avanti, "Avanti" }
                    }
                }
                button { class: "chiudi", "aria-label": "Chiudi la guida", onclick: move |_| chiudi.call(()), "×" }
            }
        }
    }
}

#[component]
fn VistaEsempio(esempio: &'static Esempio) -> Element {
    match esempio {
        Esempio::Presa(p) => rsx! { EsempioPresa { p } },
        Esempio::Bazzica { mano, scope } => rsx! {
            div { class: "esempio",
                div { class: "es-riga",
                    span { class: "es-etichetta", "In mano" }
                    Carte { codici: mano, classe: "in-tavola" }
                }
                p { class: "es-esito", "Bazzica da {scope} Scope" }
            }
        },
        Esempio::Napola { carte, punti } => rsx! {
            div { class: "esempio",
                div { class: "es-riga",
                    span { class: "es-etichetta", "Prese" }
                    Carte { codici: carte, classe: "in-tavola fitte" }
                }
                p { class: "es-esito", "{punti} punti" }
            }
        },
    }
}

#[component]
fn Carte(codici: &'static [&'static str], classe: &'static str) -> Element {
    let carte = carte(codici).unwrap_or_default();
    rsx! {
        div { class: "es-carte",
            for c in carte {
                Carta { carta: c, classe, traccia: false }
            }
        }
    }
}

#[component]
fn EsempioPresa(p: &'static Presa) -> Element {
    let presa = carte(p.presa).unwrap_or_default();
    let appena = p.appena_giocata.and_then(|c| carta(c).ok());
    let tavola = carte(p.tavola).unwrap_or_default();
    let giocata = carta(p.giocata).ok();
    let esito = &p.esito;
    let mut risultato = if presa.is_empty() {
        "Non prendi niente: la carta resta in tavola".to_string()
    } else {
        format!("Prendi {}", nomi(&presa))
    };
    for (si, t) in [
        (esito.scopa, "Scopa!"),
        (esito.piccola, "Piccola!"),
        (esito.grande, "Grande!"),
        (esito.assi > 0, "l'asso fa Scopa"),
    ] {
        if si {
            risultato.push_str(" · ");
            risultato.push_str(t);
        }
    }
    rsx! {
        div { class: "esempio",
            div { class: "es-riga",
                span { class: "es-etichetta", "In tavola" }
                div { class: "es-carte",
                    for c in tavola {
                        Carta {
                            carta: c,
                            traccia: false,
                            classe: format!(
                                "in-tavola{}{}",
                                if presa.contains(&c) { " presa" } else { " lasciata" },
                                if appena == Some(c) { " appena" } else { "" },
                            ),
                        }
                    }
                }
            }
            div { class: "es-riga",
                span { class: "es-etichetta", "Giochi" }
                if let Some(g) = giocata {
                    div { class: "es-carte", Carta { carta: g, classe: "in-tavola giocata", traccia: false } }
                }
                if p.bazzica {
                    span { class: "tag", "hai chiamato Bazzica" }
                }
                if appena.is_some() {
                    span { class: "tag", "bordo tratteggiato: appena calata" }
                }
            }
            p { class: "es-esito", "{risultato}" }
        }
    }
}

/// One-time offer at the start of a game with a variant this device has not seen yet.
#[component]
pub fn Offerta(variante: Variante, margherita: bool) -> Element {
    let mut mostra = use_signal(move || !gia_offerta(variante));
    let mut aperta = use_signal(|| false);
    rsx! {
        if mostra() {
            div { class: "offerta-guida",
                span { "Prima partita a {variante.nome()}? Le regole in un minuto." }
                button {
                    class: "piccolo speciale",
                    onclick: move |_| {
                        segna_offerta(variante);
                        mostra.set(false);
                        aperta.set(true);
                    },
                    "Apri la guida"
                }
                button {
                    class: "piccolo chiacchiera",
                    onclick: move |_| {
                        segna_offerta(variante);
                        mostra.set(false);
                    },
                    "No, grazie"
                }
            }
        }
        if aperta() {
            Guida { apertura: Apertura::per(variante, margherita), chiudi: move |_| aperta.set(false) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_guida_si_apre_sul_capitolo_della_variante() {
        let a = Apertura::per(Variante::Curie, true);
        assert_eq!(a.capitoli[a.inizio].titolo, "Curie");
        assert_eq!(a.capitoli.first().unwrap().titolo, "Le basi");
        assert_eq!(a.capitoli.last().unwrap().titolo, "Margherita Hack");
        let v = Apertura::per(Variante::Vanilla, false);
        assert_eq!((v.inizio, v.capitoli.len()), (0, 1));
        assert_eq!(Apertura::tutto().capitoli.len(), CAPITOLI.len());
    }
}
