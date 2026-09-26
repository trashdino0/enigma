//! Entry wheel (Eintrittswalze, ETW).
//!
//! The ETW connects the keyboard/lamps to the rotor stack. Wehrmacht and
//! Naval M3/M4 machines use the identity mapping (`A -> A`); commercial and
//! Railway variants (Enigma D/K) use `QWERTZUIOASDFGHJKPYXCVBNML`.
//!
//! Abstracted as its own type (rather than hardcoding identity) so later
//! stories can support D/Railway variants and custom ETWs without touching
//! the rotor/reflector hot loop. Like the other components it is two
//! stack-allocated `[u8; 26]` tables — no heap in [`EntryWheel::forward`] or
//! [`EntryWheel::backward`].

use std::str::FromStr;

use crate::{invert_wiring, parse_wiring, EnigmaError};

/// Fixed entry-wheel substitution (need not be reciprocal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryWheel {
    forward_map: [u8; 26],
    backward_map: [u8; 26],
}

impl EntryWheel {
    /// Build from a parsed permutation wiring.
    pub fn new(wiring: [u8; 26]) -> Self {
        let backward_map = invert_wiring(&wiring);
        Self {
            forward_map: wiring,
            backward_map,
        }
    }

    /// Build from a 26-letter string (validates permutation).
    pub fn from_wiring(wiring: &str) -> Result<Self, EnigmaError> {
        Ok(Self::new(parse_wiring(wiring)?))
    }

    /// Wehrmacht/Naval identity ETW (`A -> A, B -> B, ...`).
    pub fn identity() -> Self {
        Self::from_wiring(crate::data::ETW_IDENTITY).expect("identity ETW is valid")
    }

    /// Commercial/Railway QWERTZ ETW (`A -> Q, B -> W, ...`).
    pub fn qwertz() -> Self {
        Self::from_wiring(crate::data::ETW_QWERTZ).expect("QWERTZ ETW is valid")
    }

    /// Custom ETW mapping (validated permutation).
    pub fn custom(wiring: &str) -> Result<Self, EnigmaError> {
        Self::from_wiring(wiring)
    }

    /// Keyboard -> rotor stack. Zero-alloc, `#[inline]`.
    #[inline]
    pub fn forward(&self, c: u8) -> u8 {
        debug_assert!(c < 26);
        self.forward_map[c as usize]
    }

    /// Rotor stack -> lamp. Zero-alloc, `#[inline]`.
    #[inline]
    pub fn backward(&self, c: u8) -> u8 {
        debug_assert!(c < 26);
        self.backward_map[c as usize]
    }
}

impl Default for EntryWheel {
    fn default() -> Self {
        Self::identity()
    }
}

impl FromStr for EntryWheel {
    type Err = EnigmaError;

    fn from_str(wiring: &str) -> Result<Self, Self::Err> {
        Self::from_wiring(wiring)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_maps_every_letter_to_itself() {
        let etw = EntryWheel::identity();
        for c in 0..26 {
            assert_eq!(etw.forward(c), c);
            assert_eq!(etw.backward(c), c);
        }
    }

    #[test]
    fn qwertz_entry_wheel_matches_history() {
        // QWERTZUIOASDFGHJKPYXCVBNML: A->Q(16), B->W(22), Z->L(11).
        let etw = EntryWheel::qwertz();
        assert_eq!(etw.forward(0), 16); // A -> Q
        assert_eq!(etw.forward(1), 22); // B -> W
        assert_eq!(etw.forward(25), 11); // Z -> L
    }

    #[test]
    fn forward_backward_roundtrip() {
        for etw in [EntryWheel::identity(), EntryWheel::qwertz()] {
            for c in 0..26 {
                assert_eq!(etw.backward(etw.forward(c)), c);
            }
        }
    }

    #[test]
    fn invalid_etw_rejected() {
        assert!(EntryWheel::from_wiring("ABC").is_err());
        assert!(EntryWheel::from_wiring("AAAAAAAAAAAAAAAAAAAAAAAAAA").is_err());
    }
}
