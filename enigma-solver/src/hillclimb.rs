//! Ciphertext-only (blind) solver: position scan + plugboard hill-climb.
//!
//! The rotor order is fixed (pass candidates from an outer loop or known
//! ranges); for each of the `top_positions` best-scoring unplugged positions,
//! a greedy plugboard search runs with `restarts` random restarts:
//!
//! - moves: add a pair, drop a pair, re-terminate one end of a pair, swap the
//!   ends of two pairs — the classic plugboard neighbourhood;
//! - fitness: [`QuadgramScorer`] log-probability of the full decrypt;
//! - cap: at most `max_plugs` pairs; stops when no move improves.
//!
//! Positions (outer) and restarts (inner) both run on rayon. Final ranking is
//! fully deterministic: `(score desc, positions asc, plugs asc)`.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use enigma_core::{
    config::{EtwKind, ReflectorKind},
    etw::EntryWheel,
    machine::EnigmaMachine,
    plugboard::Plugboard,
    reflector::Reflector,
    rotor::{HistoricalRotor, Rotor},
    EnigmaError,
};

use crate::score::QuadgramScorer;

/// Blind search setup. `order` is exactly 3 rotors (M3) or 4 with a
/// non-stepping Beta/Gamma first (M4); `rings` matches its length.
#[derive(Debug, Clone)]
pub struct BlindConfig {
    /// Ciphertext as `0..26` bytes (needs ~150+ chars for reliable scoring).
    pub cipher: Vec<u8>,
    /// Fixed rotor order, left -> right.
    pub order: Vec<HistoricalRotor>,
    /// Fixed ring settings, one per rotor (slots in `scan_rings` are tried).
    pub rings: Vec<u8>,
    /// Ring slots (0-indexed, left -> right) to scan exhaustively (max 676 combos).
    pub scan_rings: Vec<usize>,
    /// Fixed reflector.
    pub reflector: ReflectorKind,
    /// Fixed entry wheel.
    pub etw: EtwKind,
    /// Plugboard pair cap (0 = skip the climb, positions only).
    pub max_plugs: usize,
    /// How many unplugged positions enter the plug climb.
    pub top_positions: usize,
    /// Random restarts per position (restart 0 always starts unplugged).
    pub restarts: usize,
    /// PRNG seed (deterministic runs).
    pub seed: u64,
    /// How many winners to return.
    pub top_n: usize,
}

/// One recovered setting with its decrypt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlindCandidate {
    /// Rotor names left -> right (fixed order, or the winning pool order).
    pub order: Vec<String>,
    /// Window positions left -> right (`0..26`).
    pub positions: Vec<u8>,
    /// Ring settings left -> right (`0..26`, searched when scanning).
    pub rings: Vec<u8>,
    /// Plugboard pairs, sorted.
    pub plugs: Vec<(u8, u8)>,
    /// Quadgram fitness of `plaintext`.
    pub score: f32,
    /// Full decrypt at these settings.
    pub plaintext: String,
}

/// One climbed restart: `(positions, plugs, score)`.
type ClimbOut = (Vec<u8>, Vec<(u8, u8)>, f32);

/// Minimal deterministic PRNG (xorshift64*) — avoids a `rand` dependency.
#[derive(Debug, Clone)]
struct XorShift64(u64);

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self(seed | 0x9E3779B97F4A7C15)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn shuffle(&mut self, v: &mut [u8]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i + 1);
            v.swap(i, j);
        }
    }
}

/// Pre-parsed machine parts for one fixed (order, reflector, etw):
/// per-candidate work is plugboard + decrypt only. Wirings parse once here
/// instead of per position (the old `Rotor::historical` hot path).
#[derive(Debug, Clone)]
struct SearchParts {
    etw: EntryWheel,
    reflector: Reflector,
    parsed: Vec<ParsedRotor>,
    rotor_count: usize,
}

/// Wiring parsed once; [`Rotor::new`] assembles positions cheaply per candidate.
#[derive(Debug, Clone)]
struct ParsedRotor {
    wiring: [u8; 26],
    notches: [u8; 2],
    notch_count: usize,
    steps: bool,
}

impl SearchParts {
    fn build(cfg: &BlindConfig) -> Result<Self, EnigmaError> {
        if cfg.cipher.len() < 25 {
            return Err(EnigmaError::SolverSetup(format!(
                "ciphertext ({} chars) too short for statistical scoring (need >= 25)",
                cfg.cipher.len()
            )));
        }
        // Reuse MachineConfig validation for order/rings shape.
        let probe = enigma_core::config::MachineConfig::new(
            cfg.order.clone(),
            cfg.rings.clone(),
            vec![0; cfg.order.len()],
            cfg.reflector,
            vec![],
            cfg.etw.clone(),
        )?;
        // Parse wirings once; positions assemble per candidate via Rotor::new.
        let mut parsed = Vec::with_capacity(cfg.order.len());
        for &kind in &cfg.order {
            let spec = kind.spec();
            let wiring = enigma_core::parse_wiring_table(spec.wiring)?;
            let mut notches = [0u8; 2];
            for (i, &n) in spec.notches.iter().enumerate() {
                notches[i] = n;
            }
            parsed.push(ParsedRotor {
                wiring,
                notches,
                notch_count: spec.notches.len(),
                steps: spec.steps,
            });
        }
        Ok(Self {
            etw: probe.etw.build()?,
            reflector: probe.reflector.build()?,
            parsed,
            rotor_count: cfg.order.len(),
        })
    }

    fn rotors_at(&self, positions: &[u8], rings: &[u8]) -> Result<Vec<Rotor>, EnigmaError> {
        self.parsed
            .iter()
            .zip(rings)
            .zip(positions)
            .map(|((p, &ring), &pos)| {
                Rotor::new(p.wiring, &p.notches[..p.notch_count], ring, pos, p.steps)
            })
            .collect()
    }

    fn decrypt(
        &self,
        positions: &[u8],
        rings: &[u8],
        plugs: &[(u8, u8)],
        cipher: &[u8],
    ) -> Vec<u8> {
        let rotors = self
            .rotors_at(positions, rings)
            .expect("parts pre-validated");
        let pb = Plugboard::from_pairs(plugs).expect("plug candidates valid by construction");
        let mut machine = EnigmaMachine::new(self.etw.clone(), rotors, self.reflector.clone(), pb)
            .expect("parts pre-validated");
        cipher.iter().map(|&c| machine.encipher_char(c)).collect()
    }
}

pub(crate) fn sorted_pairs(mut pairs: Vec<(u8, u8)>) -> Vec<(u8, u8)> {
    for (a, b) in &mut pairs {
        if a > b {
            std::mem::swap(a, b);
        }
    }
    pairs.sort();
    pairs
}

fn used_letters(pairs: &[(u8, u8)]) -> [bool; 26] {
    let mut used = [false; 26];
    for &(a, b) in pairs {
        used[a as usize] = true;
        used[b as usize] = true;
    }
    used
}

/// All single-move neighbours of `pairs` within the `max_plugs` cap.
pub(crate) fn neighbours(pairs: &[(u8, u8)], max_plugs: usize) -> Vec<Vec<(u8, u8)>> {
    let mut out = Vec::new();
    let used = used_letters(pairs);
    let free: Vec<u8> = (0..26u8).filter(|&c| !used[c as usize]).collect();

    // 1. Add a pair.
    if pairs.len() < max_plugs {
        for (i, &a) in free.iter().enumerate() {
            for &b in &free[i + 1..] {
                let mut next = pairs.to_vec();
                next.push((a, b));
                out.push(next);
            }
        }
    }
    // 2. Drop a pair (backtrack).
    for i in 0..pairs.len() {
        let mut next = pairs.to_vec();
        next.remove(i);
        out.push(next);
    }
    // 3. Re-terminate one end at a free letter.
    for i in 0..pairs.len() {
        let (x, y) = pairs[i];
        for &z in &free {
            let mut nx = pairs.to_vec();
            nx[i] = (x, z);
            out.push(nx);
            let mut ny = pairs.to_vec();
            ny[i] = (z, y);
            out.push(ny);
        }
    }
    // 4. Swap ends of two pairs: (a,x),(b,y) -> (a,b),(x,y) and (a,y),(b,x).
    for i in 0..pairs.len() {
        for j in (i + 1)..pairs.len() {
            let (a, x) = pairs[i];
            let (b, y) = pairs[j];
            let mut n1 = pairs.to_vec();
            n1[i] = (a.min(b), a.max(b));
            n1[j] = (x.min(y), x.max(y));
            out.push(n1);
            let mut n2 = pairs.to_vec();
            n2[i] = (a.min(y), a.max(y));
            n2[j] = (b.min(x), b.max(x));
            out.push(n2);
        }
    }
    out
}

/// Greedy climb from `start` until no neighbour improves (200-iteration cap).
fn climb(
    parts: &SearchParts,
    cipher: &[u8],
    scorer: &QuadgramScorer,
    positions: &[u8],
    rings: &[u8],
    start: Vec<(u8, u8)>,
    max_plugs: usize,
) -> (Vec<(u8, u8)>, f32) {
    let mut best_pairs = sorted_pairs(start);
    let mut best_plain = parts.decrypt(positions, rings, &best_pairs, cipher);
    let mut best_score = scorer.score_bytes(&best_plain);
    for _ in 0..200 {
        let mut improved = false;
        // Deterministic evaluation order; ties keep the incumbent.
        let mut cands = neighbours(&best_pairs, max_plugs);
        cands.sort();
        cands.dedup();
        for cand in cands {
            let cand = sorted_pairs(cand);
            let plain = parts.decrypt(positions, rings, &cand, cipher);
            let score = scorer.score_bytes(&plain);
            if score > best_score {
                best_score = score;
                best_plain = plain;
                best_pairs = cand;
                improved = true;
            }
        }
        if !improved {
            break;
        }
    }
    let _ = best_plain;
    (best_pairs, best_score)
}

fn random_start(rng: &mut XorShift64, max_plugs: usize) -> Vec<(u8, u8)> {
    if max_plugs == 0 {
        return vec![];
    }
    let mut letters: Vec<u8> = (0..26u8).collect();
    rng.shuffle(&mut letters);
    let count = rng.below(max_plugs + 1);
    letters
        .as_chunks::<2>()
        .0
        .iter()
        .take(count)
        .map(|w| (w[0].min(w[1]), w[0].max(w[1])))
        .collect()
}

/// Blind search progress, reported through the `on_progress` callback:
/// coarse enough to stay cheap (scan ticks every 2048 positions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlindProgress {
    /// Unplugged position scan: `(positions_done, positions_total)`.
    Scan { done: usize, total: usize },
    /// Plug climb: `(positions_climbed, positions_kept)`.
    Climb { done: usize, total: usize },
    /// Pool order finished: `(orders_done, orders_total)`.
    Order { done: usize, total: usize },
}

/// Ciphertext-only search. Returns the top `top_n` by
/// `(score desc, positions asc, rings asc, plugs asc)` — deterministic.
pub fn solve_blind(
    cfg: &BlindConfig,
    scorer: &QuadgramScorer,
    on_progress: Option<&(dyn Fn(BlindProgress) + Sync)>,
) -> Result<Vec<BlindCandidate>, EnigmaError> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let parts = SearchParts::build(cfg)?;
    let n = parts.rotor_count;
    let combos = crate::crib::ring_combos(&cfg.rings, &cfg.scan_rings)?;
    let npos: usize = 26usize.pow(n as u32);
    let total = npos * combos.len();

    // Stage 1: unplugged scan over every (position, rings) pair.
    let scanned_tick = AtomicUsize::new(0);
    let mut scanned: Vec<(f32, Vec<u8>, usize)> = (0..total)
        .into_par_iter()
        .map(|idx| {
            let combo = idx / npos;
            let pos_idx = idx % npos;
            let mut digits = vec![0u8; n];
            let mut rest = pos_idx;
            for d in (0..n).rev() {
                digits[d] = (rest % 26) as u8;
                rest /= 26;
            }
            let rings = &combos[combo];
            let plain = parts.decrypt(&digits, rings, &[], &cfg.cipher);
            let done = scanned_tick.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(cb) = on_progress {
                if done.is_multiple_of(2048) || done == total {
                    cb(BlindProgress::Scan { done, total });
                }
            }
            (scorer.score_bytes(&plain), digits, combo)
        })
        .collect();
    scanned.par_sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
    });
    scanned.truncate(cfg.top_positions.max(1));

    // Stage 2: plug climb per surviving (position, rings) x restart.
    let climbed_tick = AtomicUsize::new(0);
    let climb_total = scanned.len();
    let mut results: Vec<BlindCandidate> = scanned
        .into_par_iter()
        .flat_map(|(_, positions, combo)| {
            let rings = combos[combo].clone();
            let restarts: Vec<Vec<(u8, u8)>> = {
                let mut v = vec![vec![]];
                for r in 1..=cfg.restarts {
                    let mut rng =
                        XorShift64::new(cfg.seed ^ ((positions_hash(&positions) << 32) | r as u64));
                    v.push(random_start(&mut rng, cfg.max_plugs));
                }
                v
            };
            let climbed: Vec<ClimbOut> = restarts
                .into_par_iter()
                .map(|start| {
                    let (plugs, score) = climb(
                        &parts,
                        &cfg.cipher,
                        scorer,
                        &positions,
                        &rings,
                        start,
                        cfg.max_plugs,
                    );
                    (positions.clone(), plugs, score)
                })
                .collect();
            let done = climbed_tick.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(cb) = on_progress {
                cb(BlindProgress::Climb {
                    done,
                    total: climb_total,
                });
            }
            climbed
                .into_iter()
                .map(|(positions, plugs, score)| (positions, rings.clone(), plugs, score))
                .collect::<Vec<_>>()
        })
        .map(|(positions, rings, plugs, score)| {
            let plain = parts.decrypt(&positions, &rings, &plugs, &cfg.cipher);
            let text: String = plain.iter().map(|&p| enigma_core::pos_to_char(p)).collect();
            BlindCandidate {
                order: crate::crib::order_names(&cfg.order),
                positions,
                rings,
                plugs,
                score,
                plaintext: text,
            }
        })
        .collect();

    results.par_sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.positions.cmp(&b.positions))
            .then(a.rings.cmp(&b.rings))
            .then(a.plugs.cmp(&b.plugs))
    });
    // Identical restarts converge to the same winner; show it once.
    results.dedup_by(|a, b| a.positions == b.positions && a.rings == b.rings && a.plugs == b.plugs);
    results.truncate(cfg.top_n.max(1));
    Ok(results)
}

/// Order-search setup: like [`BlindConfig`] but the rotor order is permuted
/// from a pool (M3: taken 3; M4 with `fourth`: taken 3 + fixed thin rotor).
/// Rings stay fixed per slot — `Ringstellung` is a slot setting, independent
/// of which rotor sits there.
#[derive(Debug, Clone)]
pub struct BlindPoolConfig {
    /// Ciphertext as `0..26` bytes.
    pub cipher: Vec<u8>,
    /// Rotor pool to permute (distinct stepping rotors).
    pub rotor_pool: Vec<HistoricalRotor>,
    /// M4 fixed thin rotor (Beta/Gamma) prepended to every order.
    pub fourth: Option<HistoricalRotor>,
    /// Fixed ring settings, one per rotor (3, or 4 with `fourth`).
    pub rings: Vec<u8>,
    /// Fixed reflector.
    pub reflector: ReflectorKind,
    /// Fixed entry wheel.
    pub etw: EtwKind,
    /// Plugboard pair cap.
    pub max_plugs: usize,
    /// Unplugged positions entering the plug climb, per order.
    pub top_positions: usize,
    /// Random restarts per position.
    pub restarts: usize,
    /// PRNG seed (xored with the per-order index for diversity).
    pub seed: u64,
    /// Ring slots to scan (shared by every order).
    pub scan_rings: Vec<usize>,
    /// Winners kept per order before the global merge.
    pub per_order_top: usize,
    /// Global winners returned.
    pub top_n: usize,
    /// JSON checkpoint resuming per finished order (long M4 runs).
    pub checkpoint: Option<std::path::PathBuf>,
}

/// Blind search over every pool order: each order runs [`solve_blind`] (itself
/// rayon-parallel; rayon nests safely), winners merge globally by score.
/// Deterministic for a fixed seed.
pub fn solve_blind_pool(
    cfg: &BlindPoolConfig,
    scorer: &QuadgramScorer,
    on_progress: Option<&(dyn Fn(BlindProgress) + Sync)>,
) -> Result<Vec<BlindCandidate>, EnigmaError> {
    if cfg.rotor_pool.len() < 3 {
        return Err(EnigmaError::SolverSetup(format!(
            "rotor pool needs >= 3 rotors, got {}",
            cfg.rotor_pool.len()
        )));
    }
    if let Some(fourth) = cfg.fourth {
        if fourth.spec().steps {
            return Err(EnigmaError::FourthRotorMustBeNonStepping);
        }
    }
    let rotor_count = if cfg.fourth.is_some() { 4 } else { 3 };
    if cfg.rings.len() != rotor_count {
        return Err(EnigmaError::ConfigLengthMismatch {
            field: "rings",
            expected: rotor_count,
            got: cfg.rings.len(),
        });
    }
    let orders: Vec<Vec<HistoricalRotor>> = crate::crib::order_permutations(&cfg.rotor_pool, 3)
        .into_iter()
        .map(|mut o| {
            if let Some(fourth) = cfg.fourth {
                let mut full = vec![fourth];
                full.append(&mut o);
                full
            } else {
                o
            }
        })
        .collect();

    // Resume: skip finished orders, seed winners.
    let mut checkpoint = cfg
        .checkpoint
        .as_deref()
        .and_then(load_blind_checkpoint)
        .unwrap_or_default();
    let mut merged: Vec<BlindCandidate> = checkpoint.best.clone();
    let live: Vec<(usize, Vec<HistoricalRotor>)> = orders
        .into_iter()
        .enumerate()
        .filter(|(_, order)| {
            let key = order_key(order);
            !checkpoint.done_orders.contains(&key)
        })
        .collect();
    let order_total = live.len() + checkpoint.done_orders.len();

    // Sequential over orders (checkpoint granularity); rayon inside each.
    for (idx, order) in live {
        let single = BlindConfig {
            cipher: cfg.cipher.clone(),
            order: order.clone(),
            rings: cfg.rings.clone(),
            scan_rings: cfg.scan_rings.clone(),
            reflector: cfg.reflector,
            etw: cfg.etw.clone(),
            max_plugs: cfg.max_plugs,
            top_positions: cfg.top_positions,
            restarts: cfg.restarts,
            seed: cfg.seed ^ (idx as u64).wrapping_mul(0x9E3779B97F4A7C15),
            top_n: cfg.per_order_top,
        };
        // Inner progress stays silent; the pool reports per order.
        let mut out = solve_blind(&single, scorer, None).unwrap_or_default();
        merged.append(&mut out);
        merged.par_sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.order.cmp(&b.order))
                .then(a.positions.cmp(&b.positions))
                .then(a.rings.cmp(&b.rings))
        });
        merged.truncate(cfg.top_n.max(1));
        checkpoint.done_orders.push(order_key(&order));
        checkpoint.best = merged.clone();
        let done = checkpoint.done_orders.len();
        if let Some(path) = &cfg.checkpoint {
            let _ = std::fs::write(path, serde_json::to_string(&checkpoint).unwrap_or_default());
        }
        if let Some(cb) = on_progress {
            cb(BlindProgress::Order {
                done,
                total: order_total,
            });
        }
    }
    Ok(merged)
}

/// Checkpoint key for one pool order.
fn order_key(order: &[HistoricalRotor]) -> String {
    crate::crib::order_names(order).join(" ")
}

/// `{done_orders, best}` checkpoint written per finished pool order.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BlindCheckpoint {
    /// Order keys already fully searched.
    pub done_orders: Vec<String>,
    /// Current global winners.
    pub best: Vec<BlindCandidate>,
}

/// Load a blind checkpoint file. `None` when missing or corrupt.
pub fn load_blind_checkpoint(path: &std::path::Path) -> Option<BlindCheckpoint> {
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn positions_hash(positions: &[u8]) -> u64 {
    let mut h: u64 = 0;
    for &p in positions {
        h = h * 26 + p as u64;
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::Lang;

    const EN_TEXT: &str = "THEWEATHERREPORTFORTHENEXTFEWDAYSFORECASTSSUNSHINEANDWARM\
        TEMPERATURESWITHAgentleBREEZEFROMTHEWESTINTHEAFTERNOONANDCLEAR\
        SKIESOVERNIGHTTHEOUTLOOKFORTHEWEEKENDREMAINSPLEASANTWITHLITTLE\
        CHANCEOFRAINANDTHEHARVESTEXPECTEDTOCONTINUEWITHOUTINTERRUPTION\
        ALLPREPARATIONSMUSTBECOMPLETEDBEFORESUNSETONTHEEVEOFFRIDAYX";

    fn encipher_en(order: &[&str], pos: &str, plugs: &str, text: &str) -> Vec<u8> {
        encipher_en_ring(order, "AAA", pos, plugs, text)
    }

    fn encipher_en_ring(
        order: &[&str],
        rings: &str,
        pos: &str,
        plugs: &str,
        text: &str,
    ) -> Vec<u8> {
        let cfg = enigma_core::config::MachineConfig::from_strings(
            order, rings, pos, "B", plugs, "identity",
        )
        .unwrap();
        let mut m = cfg.build_machine().unwrap();
        super::super::crib::encode_text(&m.encipher_str(&text.to_ascii_uppercase()))
    }

    fn base_blind_config(cipher: Vec<u8>) -> BlindConfig {
        BlindConfig {
            cipher,
            order: vec![
                HistoricalRotor::I,
                HistoricalRotor::II,
                HistoricalRotor::III,
            ],
            rings: vec![0, 0, 0],
            scan_rings: vec![],
            reflector: ReflectorKind::B,
            etw: EtwKind::Identity,
            max_plugs: 0,
            top_positions: 2,
            restarts: 0,
            seed: 1,
            top_n: 1,
        }
    }

    #[test]
    fn blind_recovers_positions_and_plugs() {
        let cipher = encipher_en(&["I", "II", "III"], "MKL", "AV BS CG", EN_TEXT);
        let mut cfg = base_blind_config(cipher);
        cfg.max_plugs = 6;
        cfg.top_positions = 8;
        cfg.restarts = 3;
        cfg.seed = 0xC10C;
        cfg.top_n = 3;
        let scorer = QuadgramScorer::new(Lang::En);
        let best = solve_blind(&cfg, &scorer, None).unwrap();
        assert!(!best.is_empty());
        let top = &best[0];
        let expected: String = EN_TEXT
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        assert_eq!(top.plaintext, expected, "top candidate must fully decrypt");
        assert_eq!(
            top.positions,
            vec![12, 10, 11],
            "MKL recovered, got {:?}",
            top.positions
        );
    }

    #[test]
    fn short_cipher_rejected() {
        let cfg = base_blind_config(vec![0, 1, 2]);
        let scorer = QuadgramScorer::new(Lang::En);
        assert!(solve_blind(&cfg, &scorer, None).is_err());
    }

    #[test]
    fn blind_ring_scan_recovers_rings() {
        // True rings CAA; scan slot 0. Positions-only keeps it fast.
        let cipher = encipher_en_ring(&["I", "II", "III"], "CAA", "MKL", "", EN_TEXT);
        let mut cfg = base_blind_config(cipher);
        cfg.scan_rings = vec![0];
        cfg.top_positions = 5;
        let scorer = QuadgramScorer::new(Lang::En);
        let best = solve_blind(&cfg, &scorer, None).unwrap();
        assert!(!best.is_empty());
        // Full-decrypt check: any equivalent key must reproduce the text.
        let expected: String = EN_TEXT
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        assert_eq!(best[0].plaintext, expected);
    }

    #[test]
    fn pool_search_finds_true_order_positions_only() {
        // True key: II I III at BNK, no plugs; scan-only keeps it fast.
        let cipher = encipher_en(&["II", "I", "III"], "BNK", "", EN_TEXT);
        let cfg = BlindPoolConfig {
            cipher,
            rotor_pool: vec![
                HistoricalRotor::I,
                HistoricalRotor::II,
                HistoricalRotor::III,
            ],
            fourth: None,
            rings: vec![0, 0, 0],
            reflector: ReflectorKind::B,
            etw: EtwKind::Identity,
            max_plugs: 0,
            top_positions: 3,
            restarts: 0,
            seed: 7,
            scan_rings: vec![],
            per_order_top: 1,
            top_n: 2,
            checkpoint: None,
        };
        let scorer = QuadgramScorer::new(Lang::En);
        let best = solve_blind_pool(&cfg, &scorer, None).unwrap();
        assert!(!best.is_empty());
        assert_eq!(best[0].order, vec!["II", "I", "III"]);
        assert_eq!(best[0].positions, vec![1, 13, 10], "BNK recovered");
    }

    #[test]
    fn blind_checkpoint_resume_gives_same_result() {
        let cipher = encipher_en(&["II", "I", "III"], "BNK", "", EN_TEXT);
        let path = std::env::temp_dir().join("enigma_blind_test_checkpoint.json");
        let _ = std::fs::remove_file(&path);
        let mkcfg = || BlindPoolConfig {
            cipher: cipher.clone(),
            rotor_pool: vec![
                HistoricalRotor::I,
                HistoricalRotor::II,
                HistoricalRotor::III,
            ],
            fourth: None,
            rings: vec![0, 0, 0],
            scan_rings: vec![],
            reflector: ReflectorKind::B,
            etw: EtwKind::Identity,
            max_plugs: 0,
            top_positions: 3,
            restarts: 0,
            seed: 7,
            per_order_top: 1,
            top_n: 2,
            checkpoint: Some(path.clone()),
        };
        let scorer = QuadgramScorer::new(Lang::En);
        let first = solve_blind_pool(&mkcfg(), &scorer, None).unwrap();
        assert!(path.exists(), "checkpoint file should be written");
        let second = solve_blind_pool(&mkcfg(), &scorer, None).unwrap();
        assert_eq!(first.len(), second.len());
        assert_eq!(first[0].order, second[0].order);
        assert_eq!(first[0].positions, second[0].positions);
        let _ = std::fs::remove_file(&path);
    }
}
