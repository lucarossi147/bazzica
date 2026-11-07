//! Who I am on this device: a stable random id, a name and an avatar (kept in localStorage).

use bazzica_net::Partecipante;
use bazzica_net::tavolo::AVATAR;
use bazzica_rete::nostr::{casuale, esa};

fn archivio() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// `?profilo=nome` in the URL keeps a separate identity (several players in one browser).
fn prefisso() -> String {
    let cerca = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    cerca
        .trim_start_matches('?')
        .split('&')
        .find_map(|kv| kv.strip_prefix("profilo="))
        .map_or_else(String::new, |p| format!("{p}."))
}

pub fn leggi(chiave: &str) -> Option<String> {
    archivio()?
        .get_item(&format!("{}{chiave}", prefisso()))
        .ok()
        .flatten()
}

pub fn scrivi(chiave: &str, valore: &str) {
    if let Some(a) = archivio() {
        let _ = a.set_item(&format!("{}{chiave}", prefisso()), valore);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Profilo {
    pub id: String,
    pub nome: String,
    pub avatar: String,
}

impl Profilo {
    pub fn carica() -> Profilo {
        let id = leggi("bazzica.id").unwrap_or_else(|| {
            let id = esa(&casuale::<12>());
            scrivi("bazzica.id", &id);
            id
        });
        Profilo {
            id,
            nome: leggi("bazzica.nome").unwrap_or_default(),
            avatar: leggi("bazzica.avatar")
                .filter(|a| AVATAR.contains(&a.as_str()))
                .unwrap_or_else(|| AVATAR[0].into()),
        }
    }

    pub fn salva(&self) {
        scrivi("bazzica.nome", &self.nome);
        scrivi("bazzica.avatar", &self.avatar);
    }

    pub fn partecipante(&self) -> Partecipante {
        Partecipante {
            id: self.id.clone(),
            nome: self.nome.clone(),
            avatar: self.avatar.clone(),
        }
    }
}
