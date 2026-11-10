//! How peers reach each other: STUN always, plus a TURN relay when one is configured.
//!
//! STUN only finds a direct route; between phones on different networks (mobile data,
//! carrier NAT) there often is none, and only a TURN server relaying the traffic works.
//! TURN credentials come from the app's settings, else from the build (`BAZZICA_TURN_*`
//! environment variables, from GitHub secrets). No credential lives in the source: a relay's
//! password shipped in a public repository is anyone's quota.

use std::sync::{Arc, Mutex};

use matchbox_socket::RtcIceServerConfig;
use serde::{Deserialize, Serialize};

/// Public STUN servers (no account needed).
pub const STUN: [&str; 2] = [
    "stun:stun.l.google.com:19302",
    "stun:stun.cloudflare.com:3478",
];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    /// `turn:` / `turns:` URLs of one provider.
    pub urls: Vec<String>,
    pub username: String,
    pub credential: String,
}

impl Turn {
    /// From free text: URLs separated by spaces, commas or new lines. `None` if no URL is a
    /// TURN URL.
    pub fn nuovo(urls: &str, username: &str, credential: &str) -> Option<Turn> {
        let urls: Vec<String> = urls
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(str::trim)
            .filter(|u| u.starts_with("turn:") || u.starts_with("turns:"))
            .map(String::from)
            .collect();
        (!urls.is_empty()).then(|| Turn {
            urls,
            username: username.trim().into(),
            credential: credential.trim().into(),
        })
    }

    /// The one baked into this build from the `BAZZICA_TURN_*` variables, if any.
    pub fn di_compilazione() -> Option<Turn> {
        Turn::da_ambiente([
            option_env!("BAZZICA_TURN_URLS"),
            option_env!("BAZZICA_TURN_USERNAME"),
            option_env!("BAZZICA_TURN_CREDENTIAL"),
        ])
    }

    /// The build's relay when its URLs are set and non-blank (CI passes unset secrets as
    /// empty strings).
    fn da_ambiente([urls, utente, segreto]: [Option<&str>; 3]) -> Option<Turn> {
        let urls = urls.filter(|u| !u.trim().is_empty())?;
        Turn::nuovo(
            urls,
            utente.unwrap_or_default(),
            segreto.unwrap_or_default(),
        )
    }
}

/// The single ICE entry matchbox accepts: STUN URLs plus the TURN ones, with its credentials
/// (browsers ignore credentials on STUN URLs).
pub fn server_ice(turn: Option<&Turn>) -> RtcIceServerConfig {
    RtcIceServerConfig {
        urls: STUN
            .iter()
            .map(|s| s.to_string())
            .chain(turn.into_iter().flat_map(|t| t.urls.iter().cloned()))
            .collect(),
        username: turn.map(|t| t.username.clone()),
        credential: turn.map(|t| t.credential.clone()),
    }
}

/// What the signaling has achieved so far, for the "connecting…" screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Diagnosi {
    /// Relays tried and relays that answered (`None` until the attempt is over).
    pub relay_totali: usize,
    pub relay_collegati: Option<usize>,
    /// Other peers seen in the room through the relays.
    pub peer_visti: usize,
    /// Live events ignored because their time is far from this device's clock.
    pub scartati_orario: usize,
}

/// Shared between the signaller (which writes) and the session (which reads).
pub type DiagnosiCondivisa = Arc<Mutex<Diagnosi>>;

pub fn leggi(d: &DiagnosiCondivisa) -> Diagnosi {
    d.lock().map(|g| *g).unwrap_or_default()
}

pub fn aggiorna(d: &DiagnosiCondivisa, f: impl FnOnce(&mut Diagnosi)) {
    if let Ok(mut g) = d.lock() {
        f(&mut g);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_dal_testo() {
        let t = Turn::nuovo(
            "turn:a.example:3478?transport=udp, turns:a.example:443\nstun:x:1",
            " utente ",
            "segreto",
        )
        .unwrap();
        assert_eq!(
            t.urls,
            ["turn:a.example:3478?transport=udp", "turns:a.example:443"]
        );
        assert_eq!(
            (t.username.as_str(), t.credential.as_str()),
            ("utente", "segreto")
        );
        assert_eq!(Turn::nuovo("stun:x:1", "u", "p"), None);
        assert_eq!(Turn::nuovo("  ", "u", "p"), None);
    }

    #[test]
    fn turn_di_compilazione() {
        // no secrets (or blank ones, as CI passes unset secrets): no relay
        assert_eq!(Turn::da_ambiente([None; 3]), None);
        assert_eq!(Turn::da_ambiente([Some(" "), Some(""), Some("")]), None);
        let proprio = Turn::da_ambiente([Some("turn:mio:3478"), Some("u"), None]).unwrap();
        assert_eq!(
            proprio,
            Turn {
                urls: vec!["turn:mio:3478".into()],
                username: "u".into(),
                credential: "".into()
            }
        );
        // tests are built without the secrets
        assert_eq!(Turn::di_compilazione(), None);
    }

    #[test]
    fn ice_con_e_senza_turn() {
        let solo_stun = server_ice(None);
        assert_eq!(solo_stun.urls.len(), STUN.len());
        assert_eq!(solo_stun.username, None);
        let t = Turn::nuovo("turn:t:3478", "u", "p").unwrap();
        let con = server_ice(Some(&t));
        assert_eq!(con.urls.last().unwrap(), "turn:t:3478");
        assert_eq!(con.urls.len(), STUN.len() + 1);
        assert_eq!(con.credential.as_deref(), Some("p"));
    }

    #[test]
    fn diagnosi_condivisa() {
        let d = DiagnosiCondivisa::default();
        aggiorna(&d, |g| g.peer_visti = 2);
        assert_eq!(leggi(&d).peer_visti, 2);
    }
}
