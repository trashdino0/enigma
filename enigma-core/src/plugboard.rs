//! Plugboard (Steckerbrett): reciprocal letter swaps before/after the rotors.
//!
//! The plugboard is static during encipherment — it never steps. At most 10
//! pairs; each letter appears at most once; no self-steckers. Stored as a
//! single `[u8; 26]` identity map with swaps applied, so [`Plugboard::swap`]
//! is one indexed load — zero heap.

use std::str::FromStr;

use crate::EnigmaError;

/// Reciprocal plugboard substitution. Zero heap (`[u8; 26]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plugboard {
    map: [u8; 26],
}

impl Plugboard {
    /// Unpatched plugboard (identity).
    pub fn new() -> Self {
        Self {
            map: identity_map(),
        }
    }

    /// Build from 0-25 letter pairs (each pair connects both ways).
    ///
    /// Rejects self-steckers, reused letters, and more than 10 pairs.
    pub fn from_pairs(pairs: &[(u8, u8)]) -> Result<Self, EnigmaError> {
        if pairs.len() > 10 {
            return Err(EnigmaError::TooManyPlugPairs(pairs.len()));
        }
        let mut map = identity_map();
        let mut used = [false; 26];
        for &(a, b) in pairs {
            if a >= 26 || b >= 26 {
                return Err(EnigmaError::InvalidPlugPair(format!("{a}-{b}")));
            }
            if a == b {
                return Err(EnigmaError::PlugSelfConnected(a));
            }
            if used[a as usize] {
                return Err(EnigmaError::PlugLetterReused(a));
            }
            if used[b as usize] {
                return Err(EnigmaError::PlugLetterReused(b));
            }
            used[a as usize] = true;
            used[b as usize] = true;
            map[a as usize] = b;
            map[b as usize] = a;
        }
        Ok(Self { map })
    }

    /// Parse `"AV BS CG"`, `"AV,BS,CG"` or `""` (empty = unpatched).
    ///
    /// Tokens are split on whitespace and commas; each must be exactly two
    /// `A-Z` letters.
    pub fn from_wiring(s: &str) -> Result<Self, EnigmaError> {
        let mut pairs = Vec::new();
        for token in s
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|t| !t.is_empty())
        {
            let bytes = token.as_bytes();
            if bytes.len() != 2 || !bytes[0].is_ascii_uppercase() || !bytes[1].is_ascii_uppercase()
            {
                return Err(EnigmaError::InvalidPlugPair(token.to_string()));
            }
            pairs.push((bytes[0] - b'A', bytes[1] - b'A'));
        }
        Self::from_pairs(&pairs)
    }

    /// Apply the plugboard swap. Zero-alloc, `#[inline]`.
    #[inline]
    pub fn swap(&self, c: u8) -> u8 {
        debug_assert!(c < 26);
        self.map[c as usize]
    }

    /// Number of connected pairs (0-10).
    pub fn pair_count(&self) -> usize {
        self.map
            .iter()
            .enumerate()
            .filter(|(i, &m)| m as usize != *i)
            .count()
            / 2
    }
}

impl Default for Plugboard {
    fn default() -> Self {
        Self::new()
    }
}

impl FromStr for Plugboard {
    type Err = EnigmaError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_wiring(s)
    }
}

const fn identity_map() -> [u8; 26] {
    let mut map = [0u8; 26];
    let mut i = 0;
    while i < 26 {
        map[i] = i as u8;
        i += 1;
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_passes_everything_through() {
        let pb = Plugboard::new();
        for c in 0..26 {
            assert_eq!(pb.swap(c), c);
        }
        assert_eq!(pb.pair_count(), 0);
    }

    #[test]
    fn pairs_are_reciprocal() {
        let pb = Plugboard::from_pairs(&[(0, 21), (1, 18)]).expect("valid"); // AV BS
        assert_eq!(pb.swap(0), 21);
        assert_eq!(pb.swap(21), 0);
        assert_eq!(pb.swap(1), 18);
        assert_eq!(pb.swap(18), 1);
        assert_eq!(pb.swap(2), 2); // untouched
        assert_eq!(pb.pair_count(), 2);
    }

    #[test]
    fn string_forms_parse() {
        for s in ["AV BS CG", "AV,BS,CG", "AV, BS  CG", ""] {
            let pb = Plugboard::from_wiring(s).expect("valid");
            if s.is_empty() {
                assert_eq!(pb.pair_count(), 0);
            } else {
                assert_eq!(pb.pair_count(), 3);
                assert_eq!(pb.swap(0), 21); // A <-> V
            }
        }
        // `FromStr` agrees with `from_wiring`.
        let a: Plugboard = "AV BS".parse().expect("valid");
        let b = Plugboard::from_wiring("AV BS").expect("valid");
        assert_eq!(a, b);
    }

    #[test]
    fn invalid_boards_rejected() {
        assert!(Plugboard::from_pairs(&[(0, 0)]).is_err()); // self-stecker
        assert!(Plugboard::from_pairs(&[(0, 1), (1, 2)]).is_err()); // reuse
        let eleven: Vec<(u8, u8)> = (0..11).map(|i| (i * 2, i * 2 + 1)).collect();
        assert!(Plugboard::from_pairs(&eleven).is_err()); // 11 pairs
        assert!(Plugboard::from_wiring("A").is_err());
        assert!(Plugboard::from_wiring("av").is_err()); // lowercase
        assert!(Plugboard::from_wiring("A1").is_err());
    }
}
