//! The small subset of Nostr (NIP-01) used for signaling, plus the table-room encryption.
//!
//! Peers of a table publish short-lived (ephemeral-kind) events tagged with a value derived from
//! the table code. The content is encrypted with a key derived from the same code, so relays only
//! see random-looking blobs from throwaway keys: no names, no IP addresses, no game data.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use k256::schnorr::SigningKey;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Ephemeral range (20000-29999): relays forward these events but do not store them.
pub const KIND_SEGNALE: u32 = 25_319;

pub fn casuale<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("system randomness");
    b
}

pub fn esa(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A throwaway Nostr identity (one per session).
pub struct Chiavi {
    sk: SigningKey,
}

impl Chiavi {
    pub fn nuove() -> Chiavi {
        loop {
            if let Ok(sk) = SigningKey::from_slice(&casuale::<32>()) {
                return Chiavi { sk };
            }
        }
    }

    /// x-only public key, hex.
    pub fn pubkey(&self) -> String {
        esa(&self.sk.verifying_key().to_bytes())
    }

    /// Builds and signs an event (NIP-01).
    pub fn evento(&self, kind: u32, tags: Value, content: &str, created_at: u64) -> Value {
        let pubkey = self.pubkey();
        let serial = json!([0, pubkey, created_at, kind, tags, content]);
        let id: [u8; 32] = Sha256::digest(serial.to_string().as_bytes()).into();
        let sig = self.sk.sign_raw(&id, &casuale::<32>()).expect("signing");
        json!({
            "id": esa(&id),
            "pubkey": pubkey,
            "created_at": created_at,
            "kind": kind,
            "tags": tags,
            "content": content,
            "sig": esa(&sig.to_bytes()),
        })
    }
}

/// The secrets derived from a table code.
#[derive(Clone)]
pub struct Stanza {
    chiave: [u8; 32],
    /// Value of the `t` tag that identifies the room on the relays.
    pub etichetta: String,
}

impl Stanza {
    pub fn new(codice: &str) -> Stanza {
        let codice = normalizza_codice(codice);
        let chiave: [u8; 32] = Sha256::digest(format!("bazzica/stanza/v1/{codice}")).into();
        let etichetta =
            esa(&Sha256::digest([b"bazzica/etichetta/v1/".as_slice(), &chiave].concat())[..16]);
        Stanza { chiave, etichetta }
    }

    pub fn cifra(&self, testo: &[u8]) -> String {
        let nonce = casuale::<12>();
        let c = ChaCha20Poly1305::new(&Key::from(self.chiave));
        let mut out = nonce.to_vec();
        out.extend(c.encrypt(&Nonce::from(nonce), testo).expect("encryption"));
        B64.encode(out)
    }

    pub fn decifra(&self, s: &str) -> Option<Vec<u8>> {
        let b = B64.decode(s).ok()?;
        if b.len() < 12 {
            return None;
        }
        let (nonce, ct) = b.split_at(12);
        let nonce: [u8; 12] = nonce.try_into().ok()?;
        let c = ChaCha20Poly1305::new(&Key::from(self.chiave));
        c.decrypt(&Nonce::from(nonce), ct).ok()
    }
}

/// Table codes are case-insensitive and ignore spaces and dashes.
pub fn normalizza_codice(codice: &str) -> String {
    codice
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// A new table code: 8 characters from an alphabet without look-alikes (~40 bits).
pub fn nuovo_codice() -> String {
    const ALFABETO: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    casuale::<8>()
        .iter()
        .map(|b| ALFABETO[usize::from(*b) % ALFABETO.len()] as char)
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;
    use k256::schnorr::{Signature, VerifyingKey};

    fn da_esa(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn evento_firmato_correttamente() {
        let k = Chiavi::nuove();
        let tags = json!([["t", "abc"]]);
        let e = k.evento(
            KIND_SEGNALE,
            tags.clone(),
            "ciao \"mondo\" è",
            1_700_000_000,
        );
        let serial = json!([
            0,
            e["pubkey"],
            1_700_000_000u64,
            KIND_SEGNALE,
            tags,
            "ciao \"mondo\" è"
        ]);
        let id = Sha256::digest(serial.to_string().as_bytes());
        assert_eq!(e["id"], esa(&id));
        let vk = VerifyingKey::from_slice(&da_esa(e["pubkey"].as_str().unwrap())).unwrap();
        let sig = Signature::try_from(da_esa(e["sig"].as_str().unwrap()).as_slice()).unwrap();
        vk.verify_raw(&id, &sig).unwrap();
        assert_eq!(e["pubkey"].as_str().unwrap().len(), 64);
        assert_eq!(e["sig"].as_str().unwrap().len(), 128);
    }

    #[test]
    fn stanza_cifra_e_decifra() {
        let s = Stanza::new("ab-cd 23");
        assert_eq!(s.etichetta, Stanza::new("ABCD23").etichetta);
        assert_ne!(s.etichetta, Stanza::new("ABCD24").etichetta);
        let c = s.cifra(b"segreto");
        assert_eq!(s.decifra(&c).unwrap(), b"segreto");
        assert!(Stanza::new("ALTRO").decifra(&c).is_none());
        assert!(s.decifra("bm9uIHZhbGlkbw==").is_none());
    }

    #[test]
    fn codici() {
        let c = nuovo_codice();
        assert_eq!(c.len(), 8);
        assert_eq!(normalizza_codice(&c), c);
    }
}
