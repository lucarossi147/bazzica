//! Card pictures, in two interchangeable decks ("skins").

use bazzica_core::Card;
use dioxus::prelude::*;

use crate::Ctx;
use crate::profilo::{leggi, scrivi};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mazzo {
    /// The original SVG deck drawn for this project.
    Originale,
    /// Traditional Piacentine (scanned deck).
    Piacentine,
}

impl Mazzo {
    pub const TUTTI: [Mazzo; 2] = [Mazzo::Originale, Mazzo::Piacentine];

    pub fn carica() -> Mazzo {
        match leggi("bazzica.mazzo").as_deref() {
            Some("piacentine") => Mazzo::Piacentine,
            _ => Mazzo::Originale,
        }
    }

    pub fn salva(self) {
        scrivi("bazzica.mazzo", self.chiave());
    }

    pub fn chiave(self) -> &'static str {
        match self {
            Mazzo::Originale => "originale",
            Mazzo::Piacentine => "piacentine",
        }
    }

    pub fn nome(self) -> &'static str {
        match self {
            Mazzo::Originale => "Disegnate",
            Mazzo::Piacentine => "Piacentine",
        }
    }

    pub fn src(self, codice: &str) -> String {
        match self {
            Mazzo::Originale => format!("carte/{codice}.svg"),
            Mazzo::Piacentine => format!("piacentine/{codice}.webp"),
        }
    }
}

#[component]
pub fn Carta(
    carta: Card,
    #[props(default)] classe: String,
    #[props(default)] onclick: Option<EventHandler<()>>,
    /// Tracked by the flying-card animations; off for thumbnails and tutorial examples.
    #[props(default = true)]
    traccia: bool,
) -> Element {
    let mazzo = (use_context::<Ctx>().mazzo)();
    let cliccabile = if onclick.is_some() { " cliccabile" } else { "" };
    rsx! {
        img {
            class: "carta {mazzo.chiave()} {classe}{cliccabile}",
            src: mazzo.src(carta.codice()),
            alt: carta.nome(),
            title: carta.nome(),
            draggable: "false",
            "data-carta": traccia.then(|| carta.codice()),
            onclick: move |_| {
                if let Some(f) = onclick {
                    f.call(());
                }
            },
        }
    }
}

#[component]
pub fn Dorso(#[props(default)] classe: String) -> Element {
    let mazzo = (use_context::<Ctx>().mazzo)();
    rsx! {
        img {
            class: "carta dorso {mazzo.chiave()} {classe}",
            src: mazzo.src("dorso"),
            alt: "carta coperta",
            draggable: "false",
        }
    }
}

/// Deck picker (a per-device preference).
#[component]
pub fn SceltaMazzo() -> Element {
    let mut mazzo = use_context::<Ctx>().mazzo;
    rsx! {
        div { class: "scelta-mazzo",
            for m in Mazzo::TUTTI {
                button {
                    class: if mazzo() == m { "mazzo scelto" } else { "mazzo" },
                    onclick: move |_| {
                        m.salva();
                        mazzo.set(m);
                    },
                    img { src: m.src("denari_01"), alt: "" }
                    img { src: m.src("coppe_10"), alt: "" }
                    span { "{m.nome()}" }
                }
            }
        }
    }
}
