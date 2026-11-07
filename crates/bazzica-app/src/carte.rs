//! Card pictures (the original SVG deck in `public/carte`).

use bazzica_core::Card;
use dioxus::prelude::*;

#[component]
pub fn Carta(
    carta: Card,
    #[props(default)] classe: String,
    #[props(default)] onclick: Option<EventHandler<()>>,
) -> Element {
    let src = format!("carte/{}.svg", carta.codice());
    let cliccabile = if onclick.is_some() { " cliccabile" } else { "" };
    rsx! {
        img {
            class: "carta {classe}{cliccabile}",
            src,
            alt: carta.nome(),
            title: carta.nome(),
            draggable: "false",
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
    rsx! {
        img { class: "carta dorso {classe}", src: "carte/dorso.svg", alt: "carta coperta", draggable: "false" }
    }
}
