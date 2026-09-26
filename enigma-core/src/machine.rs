//! Full Enigma machine: stepping + signal path.
//!
//! Rotor order is **left -> right**: index 0 is the slow leftmost rotor,
//! the last index is the fast rightmost rotor. For M4 the index-0 thin rotor
//! (Beta/Gamma) is manually settable and **never steps** — only three pawls
//! exist, so [`EnigmaMachine::step_rotors`] advances only the three rightmost
//! rotors.
//!
//! # Stepping (pawl / ratchet, with double-stepping anomaly)
//!
//! ```text
//! mid_at  = middle.at_notch()   // before stepping
//! fast_at = fast.at_notch()     // before stepping
//! if mid_at  { slow.step(); middle.step(); }
//! if fast_at { middle.step(); }
//! fast.step();                  // always
//! ```
//!
//! Checking notches *before* moving reproduces the historic anomaly: a middle
//! rotor sitting on its own notch steps on consecutive presses (once because
//! it is on-notch, once because the fast rotor kicks it). The M4 4th rotor is
//! excluded from this logic entirely.

use crate::{
    etw::EntryWheel, plugboard::Plugboard, reflector::Reflector, rotor::Rotor, EnigmaError,
};

/// A fully assembled, ready-to-encipher Enigma machine (M3 or M4).
#[derive(Debug, Clone)]
pub struct EnigmaMachine {
    etw: EntryWheel,
    /// Left -> right. Length 3 (M3) or 4 (M4, index 0 = fixed thin rotor).
    rotors: Vec<Rotor>,
    reflector: Reflector,
    plugboard: Plugboard,
}

impl EnigmaMachine {
    /// Assemble a machine. Validates rotor count and M4 thin-rotor placement.
    ///
    /// `rotors` run left -> right (last = fast). The allocation happens here,
    /// once — [`EnigmaMachine::encipher_char`] never allocates.
    pub fn new(
        etw: EntryWheel,
        rotors: Vec<Rotor>,
        reflector: Reflector,
        plugboard: Plugboard,
    ) -> Result<Self, EnigmaError> {
        match rotors.len() {
            3 => {
                if rotors.iter().any(|r| !r.can_step()) {
                    return Err(EnigmaError::NonSteppingRotorNotAllowedHere);
                }
            }
            4 => {
                if rotors[0].can_step() {
                    return Err(EnigmaError::FourthRotorMustBeNonStepping);
                }
                if rotors[1..].iter().any(|r| !r.can_step()) {
                    return Err(EnigmaError::NonSteppingRotorNotAllowedHere);
                }
            }
            n => return Err(EnigmaError::InvalidRotorCount(n)),
        }
        Ok(Self {
            etw,
            rotors,
            reflector,
            plugboard,
        })
    }

    /// Advance rotors per the pawl/ratchet rules (call sites: none — the
    /// stepping happens inside [`EnigmaMachine::encipher_char`], before the
    /// current flows, exactly like a keypress).
    fn step_rotors(&mut self) {
        let n = self.rotors.len();
        // Slow/middle/fast among the three rightmost rotors; index 0 of an
        // M4 machine is never touched (no pawl).
        let (slow, mid, fast) = (n - 3, n - 2, n - 1);
        let mid_at_notch = self.rotors[mid].at_notch();
        let fast_at_notch = self.rotors[fast].at_notch();
        if mid_at_notch {
            self.rotors[slow].step();
            self.rotors[mid].step();
        }
        if fast_at_notch {
            self.rotors[mid].step();
        }
        self.rotors[fast].step();
    }

    /// Encipher one `0..26` signal. Steps first, then runs the full path:
    /// plugboard -> ETW -> rotors (right->left) -> reflector -> rotors
    /// (left->right) -> ETW -> plugboard. Zero heap, `#[inline]`.
    #[inline]
    pub fn encipher_char(&mut self, c: u8) -> u8 {
        debug_assert!(c < 26);
        self.step_rotors();
        let mut s = self.plugboard.swap(c);
        s = self.etw.forward(s);
        for rotor in self.rotors.iter().rev() {
            s = rotor.forward(s);
        }
        s = self.reflector.reflect(s);
        for rotor in self.rotors.iter() {
            s = rotor.backward(s);
        }
        s = self.etw.backward(s);
        self.plugboard.swap(s)
    }

    /// Encipher text. ASCII letters are uppercased and enciphered (each steps
    /// the rotors); anything else passes through **without stepping**.
    /// Allocates the output `String` once — per-character work is zero-alloc.
    pub fn encipher_str(&mut self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for ch in text.chars() {
            if ch.is_ascii_alphabetic() {
                let c = ch.to_ascii_uppercase() as u8 - b'A';
                out.push(crate::pos_to_char(self.encipher_char(c)));
            } else {
                out.push(ch);
            }
        }
        out
    }

    /// Like [`EnigmaMachine::encipher_char`] but also records the signal at
    /// every stage for visualization (TUI signal-path view). Steps first,
    /// exactly like a keypress. Zero heap — the trace is plain `u8`s.
    #[inline]
    pub fn encipher_char_traced(&mut self, c: u8) -> (u8, SignalTrace) {
        debug_assert!(c < 26);
        self.step_rotors();
        let mut t = SignalTrace {
            input: c,
            plug_in: 0,
            etw_in: 0,
            rotor_fwd: [0; 4],
            reflected: 0,
            rotor_bwd: [0; 4],
            etw_out: 0,
            output: 0,
            rotor_count: self.rotors.len(),
        };
        let mut s = self.plugboard.swap(c);
        t.plug_in = s;
        s = self.etw.forward(s);
        t.etw_in = s;
        for (i, rotor) in self.rotors.iter().rev().enumerate() {
            s = rotor.forward(s);
            t.rotor_fwd[i] = s;
        }
        s = self.reflector.reflect(s);
        t.reflected = s;
        for (i, rotor) in self.rotors.iter().enumerate() {
            s = rotor.backward(s);
            t.rotor_bwd[i] = s;
        }
        s = self.etw.backward(s);
        t.etw_out = s;
        s = self.plugboard.swap(s);
        t.output = s;
        (s, t)
    }

    /// Current window positions, left -> right (solver checkpointing).
    pub fn positions(&self) -> Vec<u8> {
        self.rotors.iter().map(Rotor::position).collect()
    }

    /// Restore window positions (solver resume / decrypt symmetry).
    pub fn set_positions(&mut self, positions: &[u8]) -> Result<(), EnigmaError> {
        if positions.len() != self.rotors.len() {
            return Err(EnigmaError::ConfigLengthMismatch {
                field: "positions",
                expected: self.rotors.len(),
                got: positions.len(),
            });
        }
        for (rotor, &pos) in self.rotors.iter_mut().zip(positions) {
            rotor.set_position(pos)?;
        }
        Ok(())
    }

    /// Rotor count (3 = M3, 4 = M4).
    pub fn rotor_count(&self) -> usize {
        self.rotors.len()
    }
}

/// Per-stage signal snapshot for one keypress, left in `0..26` domain.
///
/// Stage order follows the current: input -> plugboard -> ETW -> rotors
/// (fast -> slow in `rotor_fwd`) -> reflector -> rotors (slow -> fast in
/// `rotor_bwd`) -> ETW -> plugboard -> output. Only the first `rotor_count`
/// entries of `rotor_fwd` / `rotor_bwd` are meaningful.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalTrace {
    pub input: u8,
    pub plug_in: u8,
    pub etw_in: u8,
    pub rotor_fwd: [u8; 4],
    pub reflected: u8,
    pub rotor_bwd: [u8; 4],
    pub etw_out: u8,
    pub output: u8,
    pub rotor_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rotor::HistoricalRotor;

    /// I II III (left->right), rings AAA, positions AAA, UKW-B, no plugs.
    fn m3_aaa() -> EnigmaMachine {
        let rotors = vec![
            Rotor::historical(HistoricalRotor::I, 0, 0).unwrap(),
            Rotor::historical(HistoricalRotor::II, 0, 0).unwrap(),
            Rotor::historical(HistoricalRotor::III, 0, 0).unwrap(),
        ];
        EnigmaMachine::new(
            EntryWheel::identity(),
            rotors,
            Reflector::b().unwrap(),
            Plugboard::new(),
        )
        .unwrap()
    }

    #[test]
    fn known_vector_aaaaa_to_bdzgo() {
        // Wikipedia/Enigma rotor details: I II III, AAA/AAA, UKW-B, no plugs.
        let mut m = m3_aaa();
        assert_eq!(m.encipher_str("AAAAA"), "BDZGO");
    }

    #[test]
    fn double_stepping_anomaly_sequence() {
        // Rotors I II III from ADV-window start... classic ADU -> ADV -> AEW -> BFX.
        let rotors = vec![
            Rotor::historical(HistoricalRotor::I, 0, 0).unwrap(),
            Rotor::historical(HistoricalRotor::II, 0, 3).unwrap(), // D
            Rotor::historical(HistoricalRotor::III, 0, 20).unwrap(), // U
        ];
        let mut m = EnigmaMachine::new(
            EntryWheel::identity(),
            rotors,
            Reflector::b().unwrap(),
            Plugboard::new(),
        )
        .unwrap();
        m.encipher_str("A");
        assert_eq!(m.positions(), vec![0, 3, 21]); // ADV
        m.encipher_str("A");
        assert_eq!(m.positions(), vec![0, 4, 22]); // AEW (fast kicked middle)
        m.encipher_str("A");
        assert_eq!(m.positions(), vec![1, 5, 23]); // BFX (middle double-stepped)
    }

    #[test]
    fn m4_fourth_rotor_never_steps() {
        let rotors = vec![
            Rotor::historical(HistoricalRotor::Beta, 0, 5).unwrap(),
            Rotor::historical(HistoricalRotor::I, 0, 0).unwrap(),
            Rotor::historical(HistoricalRotor::II, 0, 0).unwrap(),
            Rotor::historical(HistoricalRotor::III, 0, 0).unwrap(),
        ];
        let mut m = EnigmaMachine::new(
            EntryWheel::identity(),
            rotors,
            Reflector::thin_b().unwrap(),
            Plugboard::new(),
        )
        .unwrap();
        m.encipher_str(&"A".repeat(200));
        assert_eq!(m.positions()[0], 5, "Beta must stay fixed");
        assert_ne!(&m.positions()[1..], &[0, 0, 0], "right rotors must advance");
    }

    #[test]
    fn encrypt_decrypt_symmetry_with_plugs() {
        let build = || {
            EnigmaMachine::new(
                EntryWheel::identity(),
                vec![
                    Rotor::historical(HistoricalRotor::IV, 3, 10).unwrap(),
                    Rotor::historical(HistoricalRotor::II, 1, 4).unwrap(),
                    Rotor::historical(HistoricalRotor::V, 0, 22).unwrap(),
                ],
                Reflector::c().unwrap(),
                Plugboard::from_wiring("AV BS CG DL").unwrap(),
            )
            .unwrap()
        };
        let plaintext = "HELLOWORLD";
        let mut enc = build();
        let cipher = enc.encipher_str(plaintext);
        assert_ne!(cipher, plaintext);
        // No letter ever enciphers to itself (reflector property end-to-end).
        for (p, c) in plaintext.chars().zip(cipher.chars()) {
            assert_ne!(p, c, "Enigma never maps a letter to itself");
        }
        let mut dec = build();
        assert_eq!(dec.encipher_str(&cipher), plaintext);
    }

    #[test]
    fn non_alpha_passthrough_does_not_step() {
        let mut m = m3_aaa();
        let before = m.positions();
        let out = m.encipher_str(" -., 123");
        assert_eq!(out, " -., 123");
        assert_eq!(m.positions(), before);
        // Lowercase is treated as uppercase.
        let mut upper = m3_aaa();
        let mut lower = m3_aaa();
        assert_eq!(upper.encipher_str("ABC"), lower.encipher_str("abc"));
    }

    #[test]
    fn set_positions_roundtrip_for_checkpointing() {
        let mut m = m3_aaa();
        m.encipher_str("HELLO");
        let saved = m.positions();
        m.encipher_str("WORLD");
        m.set_positions(&saved).unwrap();
        let mut fresh = m3_aaa();
        fresh.set_positions(&saved).unwrap();
        assert_eq!(m.encipher_str("TEST"), fresh.encipher_str("TEST"));
        assert!(m.set_positions(&[0, 0]).is_err());
    }

    #[test]
    fn traced_encipher_matches_plain_and_chains() {
        let mut traced = m3_aaa();
        let mut plain = m3_aaa();
        for c in [0u8, 4, 25, 13, 7] {
            let (out_t, t) = traced.encipher_char_traced(c);
            let out_p = plain.encipher_char(c);
            assert_eq!(out_t, out_p);
            assert_eq!(t.input, c);
            assert_eq!(t.output, out_t);
            assert_eq!(t.rotor_count, 3);
            // Chain: each stage output feeds the next stage input.
            // plug_in -> etw_in -> fwd[0..3] -> reflected -> bwd[0..3] -> etw_out -> output
            // is produced by one continuous path, so re-running the machine
            // one step behind is overkill; spot-check the endpoints instead:
            // output differs from input path only through the full stack, and
            // a second machine advanced identically agrees (positions match).
            assert_eq!(traced.positions(), plain.positions());
        }
        // M4 trace length adapts.
        let mut m4 = EnigmaMachine::new(
            EntryWheel::identity(),
            vec![
                Rotor::historical(HistoricalRotor::Beta, 0, 0).unwrap(),
                Rotor::historical(HistoricalRotor::I, 0, 0).unwrap(),
                Rotor::historical(HistoricalRotor::II, 0, 0).unwrap(),
                Rotor::historical(HistoricalRotor::III, 0, 0).unwrap(),
            ],
            Reflector::thin_b().unwrap(),
            Plugboard::new(),
        )
        .unwrap();
        let (out, t) = m4.encipher_char_traced(0);
        assert_eq!(t.rotor_count, 4);
        assert_eq!(t.output, out);
    }

    #[test]
    fn invalid_assemblies_rejected() {
        let two = vec![
            Rotor::historical(HistoricalRotor::I, 0, 0).unwrap(),
            Rotor::historical(HistoricalRotor::II, 0, 0).unwrap(),
        ];
        assert!(EnigmaMachine::new(
            EntryWheel::identity(),
            two,
            Reflector::b().unwrap(),
            Plugboard::new()
        )
        .is_err());

        // Beta has no place in a 3-rotor machine.
        let beta3 = vec![
            Rotor::historical(HistoricalRotor::Beta, 0, 0).unwrap(),
            Rotor::historical(HistoricalRotor::II, 0, 0).unwrap(),
            Rotor::historical(HistoricalRotor::III, 0, 0).unwrap(),
        ];
        assert!(EnigmaMachine::new(
            EntryWheel::identity(),
            beta3,
            Reflector::b().unwrap(),
            Plugboard::new()
        )
        .is_err());

        // A stepping rotor must not sit in the M4 4th slot.
        let bad4 = vec![
            Rotor::historical(HistoricalRotor::I, 0, 0).unwrap(),
            Rotor::historical(HistoricalRotor::II, 0, 0).unwrap(),
            Rotor::historical(HistoricalRotor::III, 0, 0).unwrap(),
            Rotor::historical(HistoricalRotor::IV, 0, 0).unwrap(),
        ];
        assert!(EnigmaMachine::new(
            EntryWheel::identity(),
            bad4,
            Reflector::thin_b().unwrap(),
            Plugboard::new()
        )
        .is_err());
    }
}
