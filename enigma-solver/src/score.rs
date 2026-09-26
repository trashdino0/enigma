//! Language fitness: quadgram log-probabilities + index of coincidence.
//!
//! Quadgram tables (German primary, English secondary) are embedded at
//! compile time and parsed once into a flat `26^4` array, so scoring one
//! character is a single indexed load — no hashing, no heap, `Sync` for
//! rayon-parallel search.

use std::str::FromStr;

/// Solver language: selects the embedded quadgram table (`--lang de|en`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    /// German — the primary historical Enigma language.
    De,
    /// English.
    En,
}

impl Lang {
    /// Parse `de`, `german`, `en`, `english` (case-insensitive).
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "de" | "german" | "deutsch" => Ok(Self::De),
            "en" | "english" => Ok(Self::En),
            _ => Err(format!("unknown language {s:?} (expected de|en)")),
        }
    }

    /// Raw embedded `QUAD COUNT` table for this language.
    const fn table(self) -> &'static str {
        match self {
            Self::De => include_str!("../data/german_quadgrams.txt"),
            Self::En => include_str!("../data/english_quadgrams.txt"),
        }
    }
}

impl FromStr for Lang {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Number of possible quadgrams (`26^4`); the score array covers all of them.
pub const QUAD_SPACE: usize = 26 * 26 * 26 * 26;

/// Quadgram fitness scorer: sums log-probabilities over sliding 4-char windows.
///
/// Higher (less negative) = more like the language. Unseen quadgrams score a
/// fixed floor (`ln(0.01/total)`, following Lyons' `ngram_score.py`).
#[derive(Debug, Clone)]
pub struct QuadgramScorer {
    /// `log P(quad)` indexed by `((a*26+b)*26+c)*26+d`.
    scores: Vec<f32>,
    floor: f32,
}

impl QuadgramScorer {
    /// Parse the embedded table for `lang` (one-time setup cost).
    pub fn new(lang: Lang) -> Self {
        let mut counts = vec![0u64; QUAD_SPACE];
        let mut total: u64 = 0;
        for line in lang.table().lines() {
            let line = line.trim();
            // Skip blanks, malformed lines, and non-A-Z quads: the German
            // table contains UTF-8 umlaut quadgrams (e.g. ÜBER) that can never
            // occur in our 26-letter machine domain.
            let Some((quad, count)) = line.split_once([' ', '\t']) else {
                continue;
            };
            let bytes = quad.as_bytes();
            if bytes.len() != 4 || !bytes.iter().all(u8::is_ascii_uppercase) {
                continue;
            }
            let idx = quad_index(
                bytes[0] - b'A',
                bytes[1] - b'A',
                bytes[2] - b'A',
                bytes[3] - b'A',
            );
            let count: u64 = count.trim().parse().expect("quadgram count parses");
            counts[idx] = count;
            total += count;
        }
        let total_f = total as f32;
        let floor = (0.01 / total_f).ln();
        let scores = counts
            .iter()
            .map(|&c| {
                if c == 0 {
                    floor
                } else {
                    (c as f32 / total_f).ln()
                }
            })
            .collect();
        Self { scores, floor }
    }

    /// Score `0..26` bytes. Hot loop: one indexed load per character.
    #[inline]
    pub fn score_bytes(&self, text: &[u8]) -> f32 {
        if text.len() < 4 {
            return self.floor * text.len().max(1) as f32;
        }
        let mut acc = 0.0f32;
        for w in text.windows(4) {
            acc += self.scores[quad_index(w[0], w[1], w[2], w[3])];
        }
        acc
    }

    /// Score text: keeps `A-Z` (uppercasing `a-z`), drops everything else.
    pub fn score_str(&self, text: &str) -> f32 {
        let bytes: Vec<u8> = text
            .bytes()
            .filter_map(|b| {
                if b.is_ascii_lowercase() {
                    Some(b - b'a')
                } else if b.is_ascii_uppercase() {
                    Some(b - b'A')
                } else {
                    None
                }
            })
            .collect();
        self.score_bytes(&bytes)
    }

    /// The unseen-quadgram floor (for tests/diagnostics).
    pub fn floor(&self) -> f32 {
        self.floor
    }
}

/// Flat index for quad `(a, b, c, d)` in `0..26` domain.
#[inline]
pub const fn quad_index(a: u8, b: u8, c: u8, d: u8) -> usize {
    (((a as usize) * 26 + b as usize) * 26 + c as usize) * 26 + d as usize
}

/// Index of coincidence: `sum f_i (f_i - 1) / (n (n - 1))` over `0..26` bytes.
///
/// Random text ~= 0.038; German/English ~= 0.076. Useful as a cheap
/// pre-filter before quadgram scoring.
pub fn index_of_coincidence(text: &[u8]) -> f64 {
    let n = text.len();
    if n < 2 {
        return 0.0;
    }
    let mut freq = [0u64; 26];
    for &c in text {
        freq[c as usize] += 1;
    }
    let numer: u64 = freq.iter().map(|&f| f * f.saturating_sub(1)).sum();
    numer as f64 / (n as f64 * (n as f64 - 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_language_prefers_itself() {
        let de = QuadgramScorer::new(Lang::De);
        let en = QuadgramScorer::new(Lang::En);
        let german = "DIEGEHEIMENACHRICHTMUSSDEFENDERNOCHHEUTEABENDUEBERMITTELTWERDEN";
        let english = "THESECRETORDERSMUSTBECARRIEDOUTBYTHETROOPSBEFORESUNRISEATDAWNX";
        assert!(
            de.score_str(german) > de.score_str(english),
            "DE table should prefer German"
        );
        assert!(
            en.score_str(english) > en.score_str(german),
            "EN table should prefer English"
        );
    }

    #[test]
    fn language_beats_random() {
        let en = QuadgramScorer::new(Lang::En);
        // Fixed pseudo-random string (xorshift, seed 42) — deterministic.
        let mut x: u32 = 42;
        let mut rand = String::new();
        for _ in 0..60 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            rand.push((b'A' + (x % 26) as u8) as char);
        }
        assert!(
            en.score_str("THEQUICKBROWNFOXJUMPSOVERTHELAZYDOGANDRUNSTOTHEHILLS")
                > en.score_str(&rand)
        );
    }

    #[test]
    fn ioc_separates_text_from_noise() {
        let text: Vec<u8> = b"THEWEATHERREPORTFORTHENEXTFEWDAYSFORECASTSSUNSHINEANDWARM\
            TEMPERATURESWITHAGENTLEBREEZEFROMTHEWESTINTHEAFTERNOONANDCLEAR\
            SKIESOVERNIGHTTHEOUTLOOKFORTHEWEEKENDREMAINSPLEASANTWITHLITTLECHANCEOFRAINX"
            .iter()
            .map(|b| b - b'A')
            .collect();
        assert!(text.len() > 150);
        let noise: Vec<u8> = (0..200usize).map(|i| ((i * 7 + 3) % 26) as u8).collect();
        assert!(index_of_coincidence(&text) > 0.055);
        assert!(index_of_coincidence(&noise) < 0.05);
    }
}
