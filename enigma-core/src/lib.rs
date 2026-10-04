//! Historically exact Enigma M3/M4 core.
//!
//! Components: [`rotor`], [`reflector`], [`etw`] (Story 1) plus
//! [`plugboard`], [`machine`], and [`config`] (Story 2). Solvers arrive later.
//!
//! # Signal domain
//!
//! The hot path uses `u8` values in `0..26` (`0 = A`). All mappings are
//! stack-allocated `[u8; 26]` lookups — no `Vec`/`String`/heap in
//! [`rotor::Rotor::forward`], [`rotor::Rotor::backward`],
//! [`reflector::Reflector::reflect`], [`etw::EntryWheel`] or
//! [`plugboard::Plugboard::swap`] methods, nor in
//! [`machine::EnigmaMachine::encipher_char`] (the rotor `Vec` is allocated
//! once at construction, never per character).

pub mod config;
pub mod data;
pub mod etw;
pub mod machine;
pub mod plugboard;
pub mod reflector;
pub mod rotor;

use std::fmt;

/// Errors for invalid Enigma configurations.
///
/// Deliberately std-only (no `thiserror` dependency): the constructors below
/// are the only fallible path, never the per-character hot loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnigmaError {
    /// Wiring string is not exactly 26 characters.
    InvalidWiringLength(usize),
    /// Wiring contains a byte outside `A-Z` at the given index.
    InvalidWiringChar { index: usize, byte: u8 },
    /// Wiring is not a permutation of A-Z (duplicate / missing letter).
    NotAPermutation,
    /// Reflector wiring is not reciprocal (`A->Y` requires `Y->A`) or maps a letter to itself.
    NotReciprocal { from: u8, to: u8 },
    /// Notch value outside `0..26`.
    InvalidNotch(u8),
    /// Rotor position outside `0..26`.
    InvalidPosition(u8),
    /// Ring setting outside `0..26`.
    InvalidRing(u8),
    /// Unknown historic rotor name (expected `I`-`VIII`, `Beta`, `Gamma`).
    InvalidRotorName(String),
    /// Unknown reflector name (expected `A`/`B`/`C`/`Thin-B`/`Thin-C`).
    InvalidReflectorName(String),
    /// Unknown entry-wheel spec (expected `identity`/`qwertz`/26-letter wiring).
    InvalidEtwSpec(String),
    /// Machine needs exactly 3 (M3) or 4 (M4) rotors.
    InvalidRotorCount(usize),
    /// The same stepping rotor was fitted twice (rotors must be distinct).
    DuplicateRotor,
    /// A 4-rotor (M4) machine needs a non-stepping Beta/Gamma as 4th rotor.
    FourthRotorMustBeNonStepping,
    /// Beta/Gamma thin rotors are only valid as the 4th rotor of an M4 machine.
    NonSteppingRotorNotAllowedHere,
    /// More than 10 plugboard pairs.
    TooManyPlugPairs(usize),
    /// Plugboard cannot connect a letter to itself.
    PlugSelfConnected(u8),
    /// Plugboard letter used in two pairs.
    PlugLetterReused(u8),
    /// Plugboard pair must be two `A-Z` letters (e.g. `AV`).
    InvalidPlugPair(String),
    /// `rotors` / `rings` / `positions` lengths disagree.
    ConfigLengthMismatch {
        field: &'static str,
        expected: usize,
        got: usize,
    },
    /// Solver setup problem (empty cipher, crib longer than cipher, ...).
    SolverSetup(String),
}

impl fmt::Display for EnigmaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWiringLength(n) => {
                write!(f, "wiring must be 26 characters, got {n}")
            }
            Self::InvalidWiringChar { index, byte } => {
                write!(f, "wiring index {index}: byte {byte:#04X} is not A-Z")
            }
            Self::NotAPermutation => {
                write!(
                    f,
                    "wiring must be a permutation of A-Z (duplicate/missing letter)"
                )
            }
            Self::NotReciprocal { from, to } => write!(
                f,
                "reflector must be reciprocal: {} -> {} is not mirrored",
                (b'A' + from) as char,
                (b'A' + to) as char
            ),
            Self::InvalidNotch(n) => write!(f, "notch must be 0..26, got {n}"),
            Self::InvalidPosition(n) => write!(f, "position must be 0..26, got {n}"),
            Self::InvalidRing(n) => write!(f, "ring setting must be 0..26, got {n}"),
            Self::InvalidRotorName(n) => {
                write!(f, "unknown rotor name {n:?} (expected I-VIII, Beta, Gamma)")
            }
            Self::InvalidReflectorName(n) => write!(
                f,
                "unknown reflector {n:?} (expected A, B, C, Thin-B, Thin-C)"
            ),
            Self::InvalidEtwSpec(n) => write!(
                f,
                "unknown entry wheel {n:?} (expected identity, qwertz, or 26-letter wiring)"
            ),
            Self::InvalidRotorCount(n) => {
                write!(f, "machine needs 3 (M3) or 4 (M4) rotors, got {n}")
            }
            Self::DuplicateRotor => write!(f, "rotor order must not repeat a rotor"),
            Self::FourthRotorMustBeNonStepping => write!(
                f,
                "4-rotor (M4) machine needs Beta/Gamma as non-stepping 4th rotor"
            ),
            Self::NonSteppingRotorNotAllowedHere => write!(
                f,
                "Beta/Gamma thin rotors are only valid as the M4 4th rotor"
            ),
            Self::TooManyPlugPairs(n) => {
                write!(f, "plugboard allows at most 10 pairs, got {n}")
            }
            Self::PlugSelfConnected(p) => write!(
                f,
                "plugboard cannot connect {} to itself",
                (b'A' + p) as char
            ),
            Self::PlugLetterReused(p) => write!(
                f,
                "plugboard letter {} used in two pairs",
                (b'A' + p) as char
            ),
            Self::InvalidPlugPair(s) => {
                write!(f, "invalid plugboard pair {s:?} (expected two A-Z letters)")
            }
            Self::ConfigLengthMismatch {
                field,
                expected,
                got,
            } => write!(
                f,
                "config field {field}: expected {expected} entries, got {got}"
            ),
            Self::SolverSetup(msg) => write!(f, "solver setup: {msg}"),
        }
    }
}

impl std::error::Error for EnigmaError {}

/// Letter `A` = 0 .. `Z` = 25.
#[inline]
pub const fn char_to_pos(c: char) -> Option<u8> {
    if c.is_ascii_uppercase() {
        Some(c as u8 - b'A')
    } else {
        None
    }
}

/// Inverse of [`char_to_pos`]. Returns `b'?'` for out-of-range input (debug aid only).
#[inline]
pub const fn pos_to_char(p: u8) -> char {
    if p < 26 {
        (b'A' + p) as char
    } else {
        '?'
    }
}

/// Parse a 26-letter `A-Z` wiring string into 0-25 contacts.
///
/// Validates length, charset, and permutation. Stack-only.
pub fn parse_wiring_table(s: &str) -> Result<[u8; 26], EnigmaError> {
    let bytes = s.as_bytes();
    if bytes.len() != 26 {
        return Err(EnigmaError::InvalidWiringLength(bytes.len()));
    }
    let mut out = [0u8; 26];
    let mut seen = [false; 26];
    for (i, &b) in bytes.iter().enumerate() {
        if !b.is_ascii_uppercase() {
            return Err(EnigmaError::InvalidWiringChar { index: i, byte: b });
        }
        let p = b - b'A';
        if seen[p as usize] {
            return Err(EnigmaError::NotAPermutation);
        }
        seen[p as usize] = true;
        out[i] = p;
    }
    Ok(out)
}

pub(crate) fn parse_wiring(s: &str) -> Result<[u8; 26], EnigmaError> {
    parse_wiring_table(s)
}

/// Transliterate German text to the 26-letter machine domain: `ä→ae`,
/// `ö→oe`, `ü→ue`, `ß→ss` (both cases). Other characters pass through
/// untouched — pair with the usual letter filtering afterwards.
pub fn transliterate_de(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            'ä' => out.push_str("ae"),
            'ö' => out.push_str("oe"),
            'ü' => out.push_str("ue"),
            'ß' => out.push_str("ss"),
            'Ä' => out.push_str("AE"),
            'Ö' => out.push_str("OE"),
            'Ü' => out.push_str("UE"),
            _ => out.push(c),
        }
    }
    out
}

/// Invert a permutation wiring. Stack-only.
#[inline]
pub(crate) const fn invert_wiring(wiring: &[u8; 26]) -> [u8; 26] {
    let mut inv = [0u8; 26];
    let mut i = 0;
    while i < 26 {
        inv[wiring[i] as usize] = i as u8;
        i += 1;
    }
    inv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transliterate_covers_umlauts_both_cases() {
        assert_eq!(
            transliterate_de("Grüße aus München"),
            "Gruesse aus Muenchen"
        );
        assert_eq!(transliterate_de("ÄÖÜ"), "AEOEUE");
        assert_eq!(transliterate_de("ABC xyz 123!"), "ABC xyz 123!");
    }

    #[test]
    fn wiring_table_parses_and_validates() {
        let w = parse_wiring_table("EKMFLGDQVZNTOWYHXUSPAIBRCJ").unwrap();
        assert_eq!((w[0], w[1], w[25]), (4, 10, 9));
        assert!(parse_wiring_table("ABC").is_err());
        assert!(parse_wiring_table("AAAAAAAAAAAAAAAAAAAAAAAAAA").is_err());
    }
}
