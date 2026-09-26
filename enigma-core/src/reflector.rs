//! Enigma reflector (Umkehrwalze, UKW).
//!
//! A reflector is a fixed reciprocal substitution with no self-mapping:
//! if `A -> Y` then `Y -> A`, and no letter maps to itself. It has no ring
//! or position — it never steps, including the M4 thin reflectors.
//!
//! Historic wirings live in [`crate::data`]; constructors below validate
//! reciprocity so a typo'd custom reflector fails fast instead of breaking
//! the machine's decrypt-symmetry property.

use std::str::FromStr;

use crate::{parse_wiring, EnigmaError};

/// Fixed reciprocal substitution. Zero heap (`[u8; 26]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reflector {
    wiring: [u8; 26],
}

impl Reflector {
    /// Build from a parsed reciprocal wiring.
    pub fn new(wiring: [u8; 26]) -> Result<Self, EnigmaError> {
        for (i, &to) in wiring.iter().enumerate() {
            let from = i as u8;
            if to == from {
                return Err(EnigmaError::NotReciprocal { from, to });
            }
            if wiring[to as usize] != from {
                return Err(EnigmaError::NotReciprocal { from, to });
            }
        }
        Ok(Self { wiring })
    }

    /// Build from a 26-letter string (validates permutation + reciprocity).
    pub fn from_wiring(wiring: &str) -> Result<Self, EnigmaError> {
        let parsed = parse_wiring(wiring)?;
        Self::new(parsed)
    }

    /// Wehrmacht `UKW-A` (early war).
    pub fn a() -> Result<Self, EnigmaError> {
        Self::from_wiring(crate::data::REFLECTOR_A)
    }

    /// Wehrmacht `UKW-B` (standard M3).
    pub fn b() -> Result<Self, EnigmaError> {
        Self::from_wiring(crate::data::REFLECTOR_B)
    }

    /// Wehrmacht `UKW-C` (alternate M3).
    pub fn c() -> Result<Self, EnigmaError> {
        Self::from_wiring(crate::data::REFLECTOR_C)
    }

    /// M4 thin `UKW-B` (use with Beta/Gamma 4th rotor).
    pub fn thin_b() -> Result<Self, EnigmaError> {
        Self::from_wiring(crate::data::REFLECTOR_THIN_B)
    }

    /// M4 thin `UKW-C` (use with Beta/Gamma 4th rotor).
    pub fn thin_c() -> Result<Self, EnigmaError> {
        Self::from_wiring(crate::data::REFLECTOR_THIN_C)
    }

    /// Custom reciprocal wiring (validated).
    pub fn custom(wiring: &str) -> Result<Self, EnigmaError> {
        Self::from_wiring(wiring)
    }

    /// Reflect a signal. Zero-alloc, `#[inline]`.
    #[inline]
    pub fn reflect(&self, c: u8) -> u8 {
        debug_assert!(c < 26);
        self.wiring[c as usize]
    }
}

impl FromStr for Reflector {
    type Err = EnigmaError;

    fn from_str(wiring: &str) -> Result<Self, Self::Err> {
        Self::from_wiring(wiring)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflector_b_known_pairs() {
        // B: (AY)(BR)(CU)... — A<->Y, B<->R.
        let b = Reflector::b().expect("valid UKW-B");
        assert_eq!(b.reflect(0), 24); // A -> Y
        assert_eq!(b.reflect(24), 0); // Y -> A
        assert_eq!(b.reflect(1), 17); // B -> R
    }

    #[test]
    fn all_historic_reflectors_reciprocal_and_deranged() {
        for r in [
            Reflector::a(),
            Reflector::b(),
            Reflector::c(),
            Reflector::thin_b(),
            Reflector::thin_c(),
        ] {
            let r = r.expect("historic reflector valid");
            for c in 0..26 {
                let o = r.reflect(c);
                assert_ne!(o, c, "reflector must never map {c} to itself");
                assert_eq!(r.reflect(o), c, "reciprocity failed at {c}");
            }
        }
    }

    #[test]
    fn non_reciprocal_wiring_rejected() {
        // Identity-ish / rotor wiring is not reciprocal.
        assert!(Reflector::from_wiring("EKMFLGDQVZNTOWYHXUSPAIBRCJ").is_err());
        // Self-mapping at A.
        assert!(Reflector::from_wiring("ABCDEFGHIJKLMNOPQRSTUVWXYZ").is_err());
    }
}
