//! First screen: name, avatar, create or join a table.

use bazzica_net::tavolo::{AVATAR, MAX_NOME};
use bazzica_rete::nostr::{normalizza_codice, nuovo_codice};
use dioxus::prelude::*;

use crate::carte::SceltaMazzo;
use crate::connessione::Connessione;
use crate::{Ctx, codice_da_url, imposta_codice_url};

#[component]
pub fn Home() -> Element {
    let ctx = use_context::<Ctx>();
    let mut profilo = ctx.profilo;
    let mut codice = use_signal(|| codice_da_url().unwrap_or_default());

    let nome_ok = !profilo.read().nome.trim().is_empty();

    let entra = move |codice_tavolo: String, crea: bool| {
        let p = profilo.read().clone();
        if p.nome.trim().is_empty() {
            ctx.avvisa("Scrivi il tuo nome");
            return;
        }
        p.salva();
        imposta_codice_url(&codice_tavolo);
        let c = if crea {
            Connessione::crea(&p, &codice_tavolo)
        } else {
            Connessione::unisciti(&p, &codice_tavolo)
        };
        let mut conn = ctx.conn;
        conn.set(Some(c));
    };

    // opened from an invitation link with a known name: join straight away
    use_hook(move || {
        if let Some(c) = codice_da_url()
            && !profilo.peek().nome.trim().is_empty()
        {
            entra(c, false);
        }
    });

    rsx! {
        div { class: "schermo centrato",
            h1 { class: "logo", "Bazzica" }
            p { class: "sottotitolo", "Il gioco di carte, tra amici, senza server." }
            div { class: "pannello",
                label { "Il tuo nome" }
                input {
                    value: "{profilo.read().nome}",
                    maxlength: MAX_NOME as i64,
                    placeholder: "Nome",
                    oninput: move |e| profilo.write().nome = e.value(),
                }
                label { "Avatar" }
                div { class: "avatar-griglia",
                    for a in AVATAR {
                        button {
                            class: if profilo.read().avatar == a { "avatar scelto" } else { "avatar" },
                            onclick: move |_| profilo.write().avatar = a.to_string(),
                            "{a}"
                        }
                    }
                }
            }
            div { class: "pannello",
                label { "Carte" }
                SceltaMazzo {}
            }
            div { class: "pannello",
                button {
                    class: "primario",
                    disabled: !nome_ok,
                    onclick: move |_| entra(nuovo_codice(), true),
                    "Crea un tavolo"
                }
                div { class: "separatore", "oppure" }
                label { "Codice del tavolo" }
                div { class: "riga",
                    input {
                        value: "{codice}",
                        placeholder: "ABCD2345",
                        autocapitalize: "characters",
                        oninput: move |e| codice.set(e.value().to_uppercase()),
                    }
                    button {
                        disabled: !nome_ok || normalizza_codice(&codice.read()).len() < 4,
                        onclick: move |_| entra(normalizza_codice(&codice.read()), false),
                        "Entra"
                    }
                }
            }
            p { class: "nota",
                "Su iPhone: Condividi → Aggiungi alla schermata Home per averla come app."
            }
        }
    }
}
