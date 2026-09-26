//! Enigma rotor (Walze) with ring setting and position.
//!
//! Signal math (all `u8` in `0..26`, zero heap):
//!
//! ```text
//! forward:  shifted = (c + pos - ring) mod 26
//!           wired   = WIRING[shifted]
//!           out     = (wired - pos + ring) mod 26
//! ```
//!
//! `backward` uses the inverse wiring. Ring and position are validated at
//! construction / mutation time, so the hot loop needs no bounds checks
//! beyond the fixed `[u8; 26]` indexing.
//!
//! # Turnover
//!
//! [`Rotor::at_notch`] compares the *window position* to the notch letters
//! and ignores the ring — notch and alphabet ring are fixed together, so the
//! turnover window letter is ring-independent (see [`crate::data`]).

use crate::{data::RotorSpec, invert_wiring, parse_wiring, EnigmaError};

/// Historic rotor identity (M3 I-V, Naval VI-VIII, M4 thin Beta/Gamma).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HistoricalRotor {
    I,
    II,
    III,
    IV,
    V,
    VI,
    VII,
    VIII,
    Beta,
    Gamma,
}

impl HistoricalRotor {
    /// Static wiring/notch spec for this rotor.
    #[inline]
    pub const fn spec(self) -> RotorSpec {
        match self {
            Self::I => crate::data::ROTOR_I,
            Self::II => crate::data::ROTOR_II,
            Self::III => crate::data::ROTOR_III,
            Self::IV => crate::data::ROTOR_IV,
            Self::V => crate::data::ROTOR_V,
            Self::VI => crate::data::ROTOR_VI,
            Self::VII => crate::data::ROTOR_VII,
            Self::VIII => crate::data::ROTOR_VIII,
            Self::Beta => crate::data::ROTOR_BETA,
            Self::Gamma => crate::data::ROTOR_GAMMA,
        }
    }
}

/// A single Enigma rotor.
///
/// Zero heap: wiring/inverse are `[u8; 26]`, notches are at most two `u8`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rotor {
    wiring: [u8; 26],
    inverse: [u8; 26],
    notches: [u8; 2],
    notch_count: usize,
    /// Whether the pawl can step this rotor. `false` for M4 Beta/Gamma
    /// 4th rotors (manually settable, never step).
    steps: bool,
    /// `Ringstellung`, 0-25.
    ring: u8,
    /// Window position (`Grundstellung`), 0-25.
    position: u8,
}

impl Rotor {
    /// Build from an already-parsed permutation wiring.
    pub fn new(
        wiring: [u8; 26],
        notches: &[u8],
        ring: u8,
        position: u8,
        steps: bool,
    ) -> Result<Self, EnigmaError> {
        if ring >= 26 {
            return Err(EnigmaError::InvalidRing(ring));
        }
        if position >= 26 {
            return Err(EnigmaError::InvalidPosition(position));
        }
        if notches.len() > 2 {
            return Err(EnigmaError::InvalidNotch(255));
        }
        // ponytail: minimum impl — permutation check lives in `from_wiring_str`;
        // raw-array callers (benches, tests) guarantee permutation by construction.
        let mut notch_arr = [0u8; 2];
        for (i, &n) in notches.iter().enumerate() {
            if n >= 26 {
                return Err(EnigmaError::InvalidNotch(n));
            }
            notch_arr[i] = n;
        }
        let inverse = invert_wiring(&wiring);
        Ok(Self {
            wiring,
            inverse,
            notches: notch_arr,
            notch_count: notches.len(),
            steps,
            ring,
            position,
        })
    }

    /// Build from a 26-letter wiring string (validates permutation).
    pub fn from_wiring_str(
        wiring: &str,
        notches: &[u8],
        ring: u8,
        position: u8,
        steps: bool,
    ) -> Result<Self, EnigmaError> {
        let parsed = parse_wiring(wiring)?;
        Self::new(parsed, notches, ring, position, steps)
    }

    /// Build a historic rotor with ring setting and window position.
    pub fn historical(kind: HistoricalRotor, ring: u8, position: u8) -> Result<Self, EnigmaError> {
        let spec: RotorSpec = kind.spec();
        Self::from_wiring_str(spec.wiring, spec.notches, ring, position, spec.steps)
    }

    /// Build from a [`RotorSpec`] (historic or caller-defined custom spec).
    pub fn from_spec(spec: &RotorSpec, ring: u8, position: u8) -> Result<Self, EnigmaError> {
        Self::from_wiring_str(spec.wiring, spec.notches, ring, position, spec.steps)
    }

    /// Right-to-left pass (keyboard -> reflector). Zero-alloc, `#[inline]`.
    #[inline]
    pub fn forward(&self, c: u8) -> u8 {
        debug_assert!(c < 26);
        let shifted = (c + 26 + self.position - self.ring) % 26;
        let wired = self.wiring[shifted as usize];
        (wired + 26 - self.position + self.ring) % 26
    }

    /// Left-to-right return pass (reflector -> keyboard). Zero-alloc, `#[inline]`.
    #[inline]
    pub fn backward(&self, c: u8) -> u8 {
        debug_assert!(c < 26);
        let shifted = (c + 26 + self.position - self.ring) % 26;
        let wired = self.inverse[shifted as usize];
        (wired + 26 - self.position + self.ring) % 26
    }

    /// Whether the notch is engaged at the current window position.
    ///
    /// Ring-independent by design. Non-stepping rotors (Beta/Gamma) never report `true`.
    #[inline]
    pub fn at_notch(&self) -> bool {
        if !self.steps {
            return false;
        }
        self.notches[..self.notch_count].contains(&self.position)
    }

    /// Advance the window position by one (0-25 wrap).
    #[inline]
    pub fn step(&mut self) {
        self.position = (self.position + 1) % 26;
    }

    #[inline]
    pub const fn position(&self) -> u8 {
        self.position
    }

    #[inline]
    pub const fn ring(&self) -> u8 {
        self.ring
    }

    #[inline]
    pub const fn can_step(&self) -> bool {
        self.steps
    }

    /// Set window position. Validated; hot loop stays branch-free.
    pub fn set_position(&mut self, position: u8) -> Result<(), EnigmaError> {
        if position >= 26 {
            return Err(EnigmaError::InvalidPosition(position));
        }
        self.position = position;
        Ok(())
    }

    /// Set ring setting. Validated; hot loop stays branch-free.
    pub fn set_ring(&mut self, ring: u8) -> Result<(), EnigmaError> {
        if ring >= 26 {
            return Err(EnigmaError::InvalidRing(ring));
        }
        self.ring = ring;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rotor_i_aaa() -> Rotor {
        Rotor::historical(HistoricalRotor::I, 0, 0).expect("valid rotor I")
    }

    #[test]
    fn rotor_i_wiring_at_aaa_matches_history() {
        // Wikipedia: rotor I at A/A encodes A->E, B->K, K->N.
        let r = rotor_i_aaa();
        assert_eq!(r.forward(0), 4); // A -> E
        assert_eq!(r.forward(1), 10); // B -> K
        assert_eq!(r.forward(10), 13); // K -> N
    }

    #[test]
    fn position_offset_shifts_wiring() {
        // Rotor I in B-position: A enters at B (wired to K), offset K -> J.
        let r = Rotor::historical(HistoricalRotor::I, 0, 1).expect("valid");
        assert_eq!(r.forward(0), 9); // J
    }

    #[test]
    fn ring_setting_rotates_wiring() {
        // Rotor I, ring B, pos A: A -> (25 wired to J=9) -> K.
        let r = Rotor::historical(HistoricalRotor::I, 1, 0).expect("valid");
        assert_eq!(r.forward(0), 10); // K
    }

    #[test]
    fn backward_inverts_forward_at_any_setting() {
        for (ring, pos) in [(0, 0), (0, 5), (7, 19), (25, 25)] {
            let r = Rotor::historical(HistoricalRotor::III, ring, pos).expect("valid");
            for c in 0..26 {
                assert_eq!(r.backward(r.forward(c)), c, "ring {ring} pos {pos} c {c}");
            }
        }
    }

    #[test]
    fn notch_detection_uses_window_position_only() {
        let at_q = Rotor::historical(HistoricalRotor::I, 0, 16).expect("valid"); // Q
        assert!(at_q.at_notch());
        let at_r = Rotor::historical(HistoricalRotor::I, 0, 17).expect("valid"); // R
        assert!(!at_r.at_notch());
        // Same window letter with a different ring still engages (ring-independent).
        let at_q_ring_b = Rotor::historical(HistoricalRotor::I, 1, 16).expect("valid");
        assert!(at_q_ring_b.at_notch());
    }

    #[test]
    fn double_notch_rotors_vi_vii_viii() {
        let at_z = Rotor::historical(HistoricalRotor::VI, 0, 25).expect("valid");
        let at_m = Rotor::historical(HistoricalRotor::VI, 0, 12).expect("valid");
        let at_a = Rotor::historical(HistoricalRotor::VI, 0, 0).expect("valid");
        assert!(at_z.at_notch());
        assert!(at_m.at_notch());
        assert!(!at_a.at_notch());
    }

    #[test]
    fn beta_gamma_never_report_notch_and_never_step_flag() {
        let beta = Rotor::historical(HistoricalRotor::Beta, 0, 0).expect("valid");
        assert!(!beta.can_step());
        assert!(!beta.at_notch());
        for pos in 0..26 {
            let b = Rotor::historical(HistoricalRotor::Beta, 0, pos).expect("valid");
            assert!(!b.at_notch(), "beta pos {pos}");
        }
    }

    #[test]
    fn custom_rotor_spec_roundtrips() {
        let spec = RotorSpec {
            wiring: "QWERTYUIOPASDFGHJKLZXCVBNM",
            notches: &[4],
            steps: true,
        };
        let r = Rotor::from_spec(&spec, 0, 0).expect("valid custom");
        for c in 0..26 {
            assert_eq!(r.backward(r.forward(c)), c);
        }
    }

    #[test]
    fn invalid_wiring_rejected() {
        assert!(Rotor::from_wiring_str("ABC", &[16], 0, 0, true).is_err());
        // Duplicate A, missing B.
        assert!(Rotor::from_wiring_str("AAAAAAAAAAAAAAAAAAAAAAAAAA", &[16], 0, 0, true).is_err());
        assert!(Rotor::historical(HistoricalRotor::I, 26, 0).is_err());
        assert!(Rotor::historical(HistoricalRotor::I, 0, 26).is_err());
    }
}
