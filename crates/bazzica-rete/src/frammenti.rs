//! Splits large messages into fragments that fit one data-channel message, and joins them.
//!
//! Browsers and SCTP stacks may refuse or mangle messages above 64 KiB, and a snapshot grows
//! with every hand. Messages up to [`MAX_PACCHETTO`] bytes travel unchanged (JSON, so they
//! start with `{`); longer ones are cut into fragments that start with [`FRAMMENTO`]:
//! `[0x00, id: u32, i: u16, n: u16, data…]` (little endian), at most [`MAX_PACCHETTO`] bytes each.

use std::collections::BTreeMap;

/// Largest packet put on the data channel, in bytes.
pub const MAX_PACCHETTO: usize = 16 * 1024;
/// First byte of a fragment (a JSON message never starts with it).
pub const FRAMMENTO: u8 = 0;
const INTESTAZIONE: usize = 1 + 4 + 2 + 2;

/// The packets that carry `dati`: itself when small enough, else fragments of message `id`.
pub fn spezza(dati: &[u8], id: u32) -> Vec<Vec<u8>> {
    if dati.len() <= MAX_PACCHETTO {
        return vec![dati.to_vec()];
    }
    let pezzi: Vec<&[u8]> = dati.chunks(MAX_PACCHETTO - INTESTAZIONE).collect();
    let n = u16::try_from(pezzi.len()).expect("messages are far below 1 GiB");
    pezzi
        .into_iter()
        .enumerate()
        .map(|(i, pezzo)| {
            let mut p = Vec::with_capacity(INTESTAZIONE + pezzo.len());
            p.push(FRAMMENTO);
            p.extend(id.to_le_bytes());
            p.extend((i as u16).to_le_bytes());
            p.extend(n.to_le_bytes());
            p.extend(pezzo);
            p
        })
        .collect()
}

/// A message being received in fragments.
#[derive(Debug)]
struct Parziale {
    id: u32,
    n: u16,
    dati: Vec<u8>,
    prossimo: u16,
}

/// Joins fragments, per sender. The channel is reliable and ordered, so fragments of a message
/// arrive in order and back to back; anything else (a gap, a stray fragment) drops the partial
/// message instead of guessing, and never panics.
#[derive(Debug)]
pub struct Ricompositore<P> {
    parziali: BTreeMap<P, Parziale>,
}

impl<P> Default for Ricompositore<P> {
    fn default() -> Self {
        Ricompositore {
            parziali: BTreeMap::new(),
        }
    }
}

impl<P: Ord + Clone> Ricompositore<P> {
    /// A packet from `da`: the whole message once it is complete.
    pub fn ricevi(&mut self, da: &P, pacchetto: &[u8]) -> Option<Vec<u8>> {
        if pacchetto.first() != Some(&FRAMMENTO) {
            self.parziali.remove(da); // a whole message cannot interrupt a fragmented one
            return Some(pacchetto.to_vec());
        }
        let (id, i, n, pezzo) = intestazione(pacchetto)?;
        let p = match self.parziali.remove(da) {
            Some(p) if p.id == id && p.n == n && p.prossimo == i => p,
            _ if i == 0 && n > 0 => Parziale {
                id,
                n,
                dati: vec![],
                prossimo: 0,
            },
            _ => return None,
        };
        let mut p = p;
        p.dati.extend_from_slice(pezzo);
        p.prossimo += 1;
        if p.prossimo == p.n {
            return Some(p.dati);
        }
        self.parziali.insert(da.clone(), p);
        None
    }

    /// Forgets a sender (its link closed).
    pub fn dimentica(&mut self, da: &P) {
        self.parziali.remove(da);
    }
}

fn intestazione(p: &[u8]) -> Option<(u32, u16, u16, &[u8])> {
    let h = p.get(1..INTESTAZIONE)?;
    let id = u32::from_le_bytes(h[0..4].try_into().ok()?);
    let i = u16::from_le_bytes(h[4..6].try_into().ok()?);
    let n = u16::from_le_bytes(h[6..8].try_into().ok()?);
    (i < n).then_some((id, i, n, &p[INTESTAZIONE..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn andata_e_ritorno(dati: &[u8]) {
        let pacchetti = spezza(dati, 7);
        assert!(pacchetti.iter().all(|p| p.len() <= MAX_PACCHETTO));
        let mut r = Ricompositore::default();
        let mut uscita = None;
        for (k, p) in pacchetti.iter().enumerate() {
            let fuori = r.ricevi(&1u8, p);
            assert_eq!(fuori.is_some(), k + 1 == pacchetti.len());
            uscita = uscita.or(fuori);
        }
        assert_eq!(uscita.unwrap(), dati);
    }

    #[test]
    fn dimensioni_ai_bordi() {
        for dim in [0, 1, MAX_PACCHETTO, MAX_PACCHETTO + 1, 1024 * 1024] {
            let dati: Vec<u8> = (0..dim).map(|i| b'{' + (i % 7) as u8).collect();
            andata_e_ritorno(&dati);
        }
        assert_eq!(spezza(b"{}", 1), vec![b"{}".to_vec()], "small: unchanged");
    }

    #[test]
    fn frammenti_fuori_posto_scartati_senza_panico() {
        let dati = vec![b'x'; 3 * MAX_PACCHETTO];
        let p = spezza(&dati, 1);
        let mut r = Ricompositore::default();
        assert_eq!(r.ricevi(&1u8, &p[1]), None, "starts in the middle");
        assert_eq!(r.ricevi(&1u8, &p[0]), None);
        assert_eq!(r.ricevi(&1u8, &p[2]), None, "a gap drops the message");
        assert_eq!(r.ricevi(&1u8, &p[3]), None);
        // a fresh start still works, and senders do not mix
        assert_eq!(r.ricevi(&2u8, &p[0]), None);
        for q in &p[1..] {
            r.ricevi(&1u8, q);
        }
        let ultimo = p[1..].iter().fold(None, |acc, q| acc.or(r.ricevi(&2u8, q)));
        assert_eq!(ultimo.unwrap(), dati);
        // garbage
        assert_eq!(r.ricevi(&3u8, &[FRAMMENTO]), None);
        assert_eq!(
            r.ricevi(&3u8, &[FRAMMENTO, 0, 0, 0, 0, 5, 0, 2, 0]),
            None,
            "i >= n"
        );
        assert_eq!(
            r.ricevi(&3u8, &[FRAMMENTO, 0, 0, 0, 0, 0, 0, 0, 0]),
            None,
            "n = 0"
        );
        r.dimentica(&2u8);
    }

    #[test]
    fn un_messaggio_intero_interrompe_un_frammentato() {
        let p = spezza(&vec![b'x'; 2 * MAX_PACCHETTO], 1);
        let mut r = Ricompositore::default();
        r.ricevi(&1u8, &p[0]);
        assert_eq!(r.ricevi(&1u8, b"{}"), Some(b"{}".to_vec()));
        assert_eq!(r.ricevi(&1u8, &p[1]), None);
    }
}
