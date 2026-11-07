//! The table: header (code, invite link, QR, players) and the lobby (seats, settings).

use bazzica_core::Variante;
use bazzica_core::game::NOMI_POSTI;
use bazzica_net::tavolo::AVATAR_BOT;
use bazzica_net::{Impostazioni, Posto, Richiesta, StatoTavolo};
use dioxus::prelude::*;

use crate::{Ctx, Vista, link_tavolo, partita};

#[component]
pub fn Tavolo(vista: Vista) -> Element {
    let in_gioco = vista.tavolo.partita.is_some() && vista.tavolo.stato != StatoTavolo::Lobby;
    rsx! {
        div { class: "schermo",
            Intestazione { vista: vista.clone() }
            if in_gioco {
                partita::Partita { vista }
            } else {
                Stanza { vista }
            }
        }
    }
}

#[component]
fn Intestazione(vista: Vista) -> Element {
    let ctx = use_context::<Ctx>();
    let mut qr = use_signal(|| false);
    let codice = ctx
        .conn
        .read()
        .as_ref()
        .map(|c| c.codice.clone())
        .unwrap_or_default();
    let link = link_tavolo(&codice);
    let link_copia = link.clone();
    let svg = use_memo(move || qr_svg(&link_tavolo(&codice)));
    let codice = ctx
        .conn
        .read()
        .as_ref()
        .map(|c| c.codice.clone())
        .unwrap_or_default();
    rsx! {
        header { class: "intestazione",
            div { class: "titolo",
                b { "{vista.tavolo.impostazioni.nome}" }
                span { class: "codice", "{codice}" }
            }
            div { class: "azioni",
                button {
                    class: "piccolo",
                    onclick: move |_| {
                        copia(&link_copia);
                        ctx.avvisa("Link copiato: mandalo agli amici");
                    },
                    "Link"
                }
                button { class: "piccolo", onclick: move |_| qr.toggle(), "QR" }
                button { class: "piccolo secondario", onclick: move |_| ctx.esci(), "Esci" }
            }
        }
        if qr() {
            div { class: "qr", onclick: move |_| qr.set(false),
                div { class: "qr-box",
                    div { dangerous_inner_html: "{svg}" }
                    p { "Inquadra per entrare nel tavolo " b { "{codice}" } }
                    p { class: "nota", "{link}" }
                }
            }
        }
    }
}

fn qr_svg(testo: &str) -> String {
    use qrcode::QrCode;
    use qrcode::render::svg;
    QrCode::new(testo.as_bytes())
        .map(|c| c.render::<svg::Color>().min_dimensions(220, 220).build())
        .unwrap_or_default()
}

fn copia(testo: &str) {
    if let Some(w) = web_sys::window() {
        let _ = w.navigator().clipboard().write_text(testo);
    }
}

pub fn avatar_posto(v: &Vista, s: usize) -> String {
    match &v.tavolo.posti[s] {
        Some(Posto::Umano { id }) => v
            .tavolo
            .partecipante(id)
            .map_or_else(String::new, |p| p.avatar.clone()),
        Some(Posto::Bot) => AVATAR_BOT.into(),
        None => String::new(),
    }
}

#[component]
fn Stanza(vista: Vista) -> Element {
    let ctx = use_context::<Ctx>();
    let t = &vista.tavolo;
    let completi = t.posti.iter().all(Option::is_some);
    let creatore = t.creatore() == &vista.io;
    rsx! {
        div { class: "stanza",
            if let Some(da) = &t.terminata_da {
                div { class: "banner", "Partita interrotta da {vista.nome(da)}." }
            }
            div { class: "pannello",
                h3 { "Posti" }
                div { class: "croce",
                    for s in [0usize, 3, 1, 2] {
                        Sedia { vista: vista.clone(), seat: s, creatore }
                    }
                    div { class: "centro",
                        span { class: "sq0", "{t.impostazioni.squadre[0]}" }
                        br {}
                        "Nord–Sud"
                        br {}
                        span { class: "sq1", "{t.impostazioni.squadre[1]}" }
                        br {}
                        "Est–Ovest"
                    }
                }
                button {
                    class: "primario",
                    disabled: !completi,
                    onclick: move |_| ctx.richiedi(Richiesta::Avvia),
                    if completi { "Inizia la partita" } else { "Servono 4 giocatori (anche bot)" }
                }
            }
            Regole { imp: t.impostazioni.clone() }
            Presenti { vista: vista.clone() }
        }
    }
}

#[component]
fn Sedia(vista: Vista, seat: usize, creatore: bool) -> Element {
    let ctx = use_context::<Ctx>();
    let posto = seat as u8;
    let area = ["n", "e", "s", "o"][seat];
    let classe = format!("posto area-{area} sq{}", seat % 2);
    let contenuto = match &vista.tavolo.posti[seat] {
        None => rsx! {
            span { class: "vuoto", "libero" }
            div { class: "riga",
                button { class: "piccolo", onclick: move |_| ctx.richiedi(Richiesta::Siedi { posto }), "Siediti" }
                button { class: "piccolo secondario", onclick: move |_| ctx.richiedi(Richiesta::Bot { posto, aggiungi: true }), "Bot" }
            }
        },
        Some(Posto::Bot) => rsx! {
            span { class: "nome", "{AVATAR_BOT} Bot" }
            button { class: "piccolo secondario", onclick: move |_| ctx.richiedi(Richiesta::Bot { posto, aggiungi: false }), "Togli" }
        },
        Some(Posto::Umano { id }) => {
            let mio = id == &vista.io;
            let collegato = vista.connessi.contains(id);
            let nome = vista.nome(id);
            let avatar = avatar_posto(&vista, seat);
            rsx! {
                span { class: if collegato { "nome" } else { "nome scollegato" },
                    "{avatar} {nome}"
                    if mio { " (tu)" }
                }
                if mio || creatore {
                    button { class: "piccolo secondario", onclick: move |_| ctx.richiedi(Richiesta::Alzati { posto }),
                        if mio { "Alzati" } else { "Libera" }
                    }
                }
            }
        }
    };
    rsx! {
        div { class: "{classe}",
            span { class: "etichetta", "{NOMI_POSTI[seat]}" }
            {contenuto}
        }
    }
}

#[component]
fn Regole(imp: Impostazioni) -> Element {
    let ctx = use_context::<Ctx>();
    let imposta = move |nuove: Impostazioni| ctx.richiedi(Richiesta::Imposta(nuove));
    let durate = [0u32, 10, 15, 20, 30, 45, 60, 90];
    let (i1, i2, i3, i4, i5, i6) = (
        imp.clone(),
        imp.clone(),
        imp.clone(),
        imp.clone(),
        imp.clone(),
        imp.clone(),
    );
    rsx! {
        div { class: "pannello",
            h3 { "Regole" }
            label { "Variante" }
            select {
                onchange: move |e| {
                    if let Some(v) = Variante::from_nome(&e.value()) {
                        imposta(Impostazioni { variante: v, ..i1.clone() });
                    }
                },
                for v in Variante::TUTTE {
                    option { value: v.nome(), selected: v == imp.variante, "{v.nome()}" }
                }
            }
            label { class: "spunta",
                input {
                    r#type: "checkbox",
                    checked: imp.margherita,
                    onchange: move |e| imposta(Impostazioni { margherita: e.checked(), ..i2.clone() }),
                }
                "Margherita Hack (4 mani, vince chi ha più punti)"
            }
            div { class: "riga",
                div {
                    label { "Tempo per giocare" }
                    select {
                        onchange: move |e| imposta(Impostazioni { timer_giocata: e.value().parse().unwrap_or(15), ..i3.clone() }),
                        for d in durate {
                            option { value: "{d}", selected: d == imp.timer_giocata, {durata(d)} }
                        }
                    }
                }
                div {
                    label { "Tempo per lo scambio" }
                    select {
                        onchange: move |e| imposta(Impostazioni { timer_scambio: e.value().parse().unwrap_or(30), ..i4.clone() }),
                        for d in durate {
                            option { value: "{d}", selected: d == imp.timer_scambio, {durata(d)} }
                        }
                    }
                }
            }
            div { class: "riga",
                div {
                    label { "Squadra Nord–Sud" }
                    input {
                        value: "{imp.squadre[0]}",
                        maxlength: 20,
                        onchange: move |e| {
                            let mut n = i5.clone();
                            n.squadre[0] = e.value();
                            imposta(n);
                        },
                    }
                }
                div {
                    label { "Squadra Est–Ovest" }
                    input {
                        value: "{imp.squadre[1]}",
                        maxlength: 20,
                        onchange: move |e| {
                            let mut n = i6.clone();
                            n.squadre[1] = e.value();
                            imposta(n);
                        },
                    }
                }
            }
        }
    }
}

fn durata(s: u32) -> String {
    if s == 0 {
        "nessun limite".into()
    } else {
        format!("{s} s")
    }
}

#[component]
pub fn Presenti(vista: Vista) -> Element {
    rsx! {
        div { class: "pannello",
            h3 { "Al tavolo" }
            ul { class: "presenti",
                for p in vista.tavolo.partecipanti.iter() {
                    li { class: if vista.connessi.contains(&p.id) { "" } else { "scollegato" },
                        "{p.avatar} {p.nome}"
                        if p.id == vista.io { " (tu)" }
                        if vista.host.as_ref() == Some(&p.id) { span { class: "tag", "regia" } }
                        if !vista.connessi.contains(&p.id) { span { class: "tag", "non collegato" } }
                    }
                }
            }
        }
    }
}
