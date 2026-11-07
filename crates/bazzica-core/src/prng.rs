//! mulberry32: the PRNG shared with the Python reference engine (`bazzica/engine/prng.py`).
//!
//! It drives the first dealer, every shuffle and the bots' tie-breaks, so a whole game is
//! reproducible from its seed. Every operation is on unsigned 32-bit integers and must stay
//! bit-identical to the Python implementation.

use serde::{Deserialize, Serialize};

/// Serialized as its bare 32-bit state, like `st.rng.stato` in the Python traces.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Mulberry32 {
    pub stato: u32,
}

impl Mulberry32 {
    pub fn new(seed: u64) -> Self {
        Self { stato: seed as u32 }
    }

    pub fn next_u32(&mut self) -> u32 {
        self.stato = self.stato.wrapping_add(0x6D2B_79F5);
        let mut t = self.stato;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        t ^ (t >> 14)
    }

    /// Float in `[0, 1)` with 32 bits of resolution.
    pub fn random(&mut self) -> f64 {
        f64::from(self.next_u32()) / 4_294_967_296.0
    }

    /// Integer in `[0, n)` by multiply-and-shift (no floating point).
    pub fn randbelow(&mut self, n: u32) -> u32 {
        assert!(n > 0, "n must be positive");
        ((u64::from(self.next_u32()) * u64::from(n)) >> 32) as u32
    }

    /// Fisher–Yates from the last element down to the second.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.randbelow(i as u32 + 1) as usize;
            items.swap(i, j);
        }
    }

    pub fn choice<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.randbelow(items.len() as u32) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Same vectors as tests/test_prng.py.
    #[test]
    fn reference_vectors() {
        let mut r = Mulberry32::new(1);
        let got: Vec<u32> = (0..5).map(|_| r.next_u32()).collect();
        assert_eq!(
            got,
            [2693262067, 11749833, 2265367787, 4213581821, 4159151403]
        );
        let mut r = Mulberry32::new(0xDEAD_BEEF);
        let got: Vec<u32> = (0..3).map(|_| r.next_u32()).collect();
        assert_eq!(got, [4043151706, 1147597007, 3315858022]);
    }

    #[test]
    fn javascript_reference() {
        assert_eq!(Mulberry32::new(1).random(), 0.6270739405881613);
    }

    #[test]
    fn shuffle_randbelow_random() {
        let mut r = Mulberry32::new(42);
        let mut v: Vec<u32> = (0..10).collect();
        r.shuffle(&mut v);
        assert_eq!(v, [0, 7, 3, 5, 2, 1, 8, 9, 4, 6]);
        assert_eq!(r.randbelow(40), 18);
        assert_eq!(r.random(), 0.24992373422719538);
    }
}
