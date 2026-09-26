//! Raw historic Enigma wirings and turnover data.
//!
//! Sources: `Enigma rotor details` (Wikipedia), David Hamer's rotor wiring
//! tables, Crypto Museum wiring pages. All wirings are given per convention
//! with `Ringstellung = A`.
//!
//! # Notch convention
//!
//! We store **notch letters**: the window letter at which the rotor's notch
//! is engaged *before* stepping (e.g. Rotor I notch `Q`). On the next
//! keypress the neighbour steps and this rotor advances to `Q -> R`.
//! Some references instead tabulate the **turnover letter** (`R` for Rotor I,
//! i.e. notch + 1). Both describe the same mechanics; the unit tests pin the
//! notch convention.
//!
//! # Ringstellung vs turnover
//!
//! Notch and alphabet ring are fixed together (Crypto Museum), so the
//! turnover *window letter* is independent of the ring setting. The ring only
//! offsets the internal wiring. [`crate::rotor::Rotor::at_notch`] therefore
//! compares the window position against the notch and ignores the ring.

/// Static spec for one historic rotor type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotorSpec {
    /// 26-letter wiring, input from the right -> output to the left.
    pub wiring: &'static str,
    /// Notch window letters as 0-25 (`Q` = 16). Empty for non-stepping rotors.
    pub notches: &'static [u8],
    /// Whether the pawl/ratchet can step this rotor (false for Beta/Gamma 4th rotors).
    pub steps: bool,
}

// Notch letters as 0-25 constants for readability.
const Q: u8 = 16;
const E: u8 = 4;
const V: u8 = 21;
const J: u8 = 9;
const Z: u8 = 25;
const M: u8 = 12;

pub const ROTOR_I: RotorSpec = RotorSpec {
    wiring: "EKMFLGDQVZNTOWYHXUSPAIBRCJ",
    notches: &[Q],
    steps: true,
};
pub const ROTOR_II: RotorSpec = RotorSpec {
    wiring: "AJDKSIRUXBLHWTMCQGZNPYFVOE",
    notches: &[E],
    steps: true,
};
pub const ROTOR_III: RotorSpec = RotorSpec {
    wiring: "BDFHJLCPRTXVZNYEIWGAKMUSQO",
    notches: &[V],
    steps: true,
};
pub const ROTOR_IV: RotorSpec = RotorSpec {
    wiring: "ESOVPZJAYQUIRHXLNFTGKDCMWB",
    notches: &[J],
    steps: true,
};
pub const ROTOR_V: RotorSpec = RotorSpec {
    wiring: "VZBRGITYUPSDNHLXAWMJQOFECK",
    notches: &[Z],
    steps: true,
};
pub const ROTOR_VI: RotorSpec = RotorSpec {
    wiring: "JPGVOUMFYQBENHZRDKASXLICTW",
    notches: &[Z, M],
    steps: true,
};
pub const ROTOR_VII: RotorSpec = RotorSpec {
    wiring: "NZJHGRCXMYSWBOUFAIVLPEKQDT",
    notches: &[Z, M],
    steps: true,
};
pub const ROTOR_VIII: RotorSpec = RotorSpec {
    wiring: "FKQHTLXOCBJSPDZRAMEWNIUYGV",
    notches: &[Z, M],
    steps: true,
};

/// M4 thin 4th rotor. Manually settable, never steps (no pawl).
pub const ROTOR_BETA: RotorSpec = RotorSpec {
    wiring: "LEYJVCNIXWPBQMDRTAKZGFUHOS",
    notches: &[],
    steps: false,
};
/// M4 thin 4th rotor. Manually settable, never steps (no pawl).
pub const ROTOR_GAMMA: RotorSpec = RotorSpec {
    wiring: "FSOKANUERHMBTIYCWLQPZXVGJD",
    notches: &[],
    steps: false,
};

/// Wehrmacht/Marine entry wheel: identity mapping.
pub const ETW_IDENTITY: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
/// Commercial / Railway (Enigma D/K) entry wheel in QWERTZ order.
pub const ETW_QWERTZ: &str = "QWERTZUIOASDFGHJKPYXCVBNML";

pub const REFLECTOR_A: &str = "EJMZALYXVBWFCRQUONTSPIKHGD";
pub const REFLECTOR_B: &str = "YRUHQSLDPXNGOKMIEBFZCWVJAT";
pub const REFLECTOR_C: &str = "FVPJIAOYEDRZXWGCTKUQSBNMHL";
/// M4 thin reflector (use with Beta/Gamma).
pub const REFLECTOR_THIN_B: &str = "ENKQAUYWJICOPBLMDXZVFTHRGS";
/// M4 thin reflector (use with Beta/Gamma).
pub const REFLECTOR_THIN_C: &str = "RDOBJNTKVEHMLFCWZAXGYIPSUQ";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_rotor_wirings_are_26_chars() {
        for spec in [
            ROTOR_I,
            ROTOR_II,
            ROTOR_III,
            ROTOR_IV,
            ROTOR_V,
            ROTOR_VI,
            ROTOR_VII,
            ROTOR_VIII,
            ROTOR_BETA,
            ROTOR_GAMMA,
        ] {
            assert_eq!(spec.wiring.len(), 26, "wiring {}", spec.wiring);
        }
    }

    #[test]
    fn notch_tables_match_history() {
        assert_eq!(ROTOR_I.notches, &[16]); // Q
        assert_eq!(ROTOR_II.notches, &[4]); // E
        assert_eq!(ROTOR_III.notches, &[21]); // V
        assert_eq!(ROTOR_IV.notches, &[9]); // J
        assert_eq!(ROTOR_V.notches, &[25]); // Z
        assert_eq!(ROTOR_VI.notches, &[25, 12]); // Z, M
        assert!(ROTOR_BETA.notches.is_empty());
    }
}
