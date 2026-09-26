//! Known-plaintext (crib) solver: Bombe-style search over rotor orders x positions.
//!
//! Given ciphertext plus a suspected plaintext fragment (*crib*), this tries
//! every rotor order from the pool and every start position (fixed rings,
//! reflector, plugs, and entry wheel are assumed known — pass them in).
//! Candidates are ranked by crib matches, quadgram score breaking ties.
//!
//! Long M4 searches support `--checkpoint-file` resume: after each rotor order
//! completes, `{done_orders, best}` is written as JSON; restarting with the
//! same file skips finished orders. Orders are processed sequentially while
//! positions within one order run on rayon — checkpoint granularity with
//! full core utilization.

use std::path::Path;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use enigma_core::{
    config::{parse_letters, parse_rotor_name, EtwKind, MachineConfig, ReflectorKind},
    machine::EnigmaMachine,
    plugboard::Plugboard,
    pos_to_char,
    rotor::HistoricalRotor,
    EnigmaError,
};

use crate::score::QuadgramScorer;

/// Crib search setup. `cipher`/`crib` are `0..26` bytes (see
/// [`encode_text`]). Rings, reflector, plugs, and entry wheel are fixed;
/// orders come from `rotor_pool` (distinct stepping rotors) and positions are
/// exhaustively searched.
#[derive(Debug, Clone)]
pub struct CribConfig {
    /// Ciphertext as `0..26` bytes.
    pub cipher: Vec<u8>,
    /// Known plaintext fragment as `0..26` bytes.
    pub crib: Vec<u8>,
    /// Crib placement in the cipher. `None` scans every possible offset.
    pub crib_offset: Option<usize>,
    /// Rotor pool to permute (M3: taken 3; M4 with `fourth`: taken 3 + fixed 4th).
    pub rotor_pool: Vec<HistoricalRotor>,
    /// M4 fixed thin rotor (Beta/Gamma) prepended to every order. `None` = M3.
    pub fourth: Option<HistoricalRotor>,
    /// Fixed ring settings, one per rotor (3 for M3, 4 for M4 incl. 4th).
    pub rings: Vec<u8>,
    /// Fixed reflector.
    pub reflector: ReflectorKind,
    /// Assumed-known plugboard pairs.
    pub plugs: Vec<(u8, u8)>,
    /// Fixed entry wheel.
    pub etw: EtwKind,
    /// How many top candidates to return.
    pub top_n: usize,
    /// Minimum crib matches to shortlist a candidate.
    pub min_matches: usize,
}

/// One recovered setting, JSON-serializable for checkpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CribCandidate {
    /// Rotor names left -> right.
    pub order: Vec<String>,
    /// Window positions left -> right (`0..26`).
    pub positions: Vec<u8>,
    /// Crib letters matching at the crib offset.
    pub matches: usize,
    /// Quadgram fitness of the full decrypt (tiebreak / display).
    pub score: f32,
}

impl CribCandidate {
    /// Positions as a window string, e.g. `"KDO"`.
    pub fn positions_str(&self) -> String {
        self.positions.iter().map(|&p| pos_to_char(p)).collect()
    }
}

/// `{done_orders, best}` checkpoint written after each rotor order.
///
/// Public so other frontends (e.g. the TUI progress view) can read the live
/// best list while a search runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CribCheckpoint {
    /// Rotor orders (as name lists) already fully searched.
    pub done_orders: Vec<Vec<String>>,
    /// Current global top candidates.
    pub best: Vec<CribCandidate>,
}

/// Load a checkpoint file written by [`solve_crib`]. Returns `None` when the
/// file is missing or corrupt (caller starts fresh).
pub fn load_checkpoint(path: &Path) -> Option<CribCheckpoint> {
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Build a [`CribConfig`] from human strings (shared by the CLI and TUI).
///
/// `pool` holds rotor names, `rings`/`cipher_text`/`crib_text` are letter
/// strings, `reflector`/`plugs`/`etw` use the config syntax. `min_matches`
/// defaults to the full crib length.
#[allow(clippy::too_many_arguments)]
pub fn build_crib_config(
    pool: &[String],
    fourth: Option<&str>,
    rings: &str,
    reflector: &str,
    plugs: &str,
    etw: &str,
    cipher_text: &str,
    crib_text: &str,
    crib_offset: Option<usize>,
    top_n: usize,
    min_matches: Option<usize>,
) -> Result<CribConfig, EnigmaError> {
    let rotor_pool = pool
        .iter()
        .map(|s| parse_rotor_name(s))
        .collect::<Result<Vec<_>, _>>()?;
    let fourth = fourth.map(parse_rotor_name).transpose()?;
    let rings = parse_letters(rings)?;
    let reflector = ReflectorKind::parse(reflector)?;
    let board = Plugboard::from_wiring(plugs)?;
    let mut plug_pairs = Vec::new();
    {
        let mut seen = [false; 26];
        for a in 0..26u8 {
            let b = board.swap(a);
            if b != a && !seen[a as usize] {
                plug_pairs.push((a, b));
                seen[a as usize] = true;
                seen[b as usize] = true;
            }
        }
    }
    let etw = EtwKind::parse(etw)?;
    let cipher = encode_text(cipher_text);
    let crib = encode_text(crib_text);
    let min_matches = min_matches.unwrap_or(crib.len());
    Ok(CribConfig {
        cipher,
        crib,
        crib_offset,
        rotor_pool,
        fourth,
        rings,
        reflector,
        plugs: plug_pairs,
        etw,
        top_n,
        min_matches,
    })
}

/// Encode text: keep `A-Z` (uppercasing `a-z`), drop everything else.
pub fn encode_text(s: &str) -> Vec<u8> {
    s.bytes()
        .filter_map(|b| {
            if b.is_ascii_lowercase() {
                Some(b - b'a')
            } else if b.is_ascii_uppercase() {
                Some(b - b'A')
            } else {
                None
            }
        })
        .collect()
}

/// Distinct permutations of `pool` taken `k`, in pool order (deterministic).
pub fn order_permutations(pool: &[HistoricalRotor], k: usize) -> Vec<Vec<HistoricalRotor>> {
    let mut out = Vec::new();
    let mut cur = Vec::with_capacity(k);
    let mut used = vec![false; pool.len()];
    fn rec(
        pool: &[HistoricalRotor],
        k: usize,
        cur: &mut Vec<HistoricalRotor>,
        used: &mut [bool],
        out: &mut Vec<Vec<HistoricalRotor>>,
    ) {
        if cur.len() == k {
            out.push(cur.clone());
            return;
        }
        for (i, &r) in pool.iter().enumerate() {
            if !used[i] {
                used[i] = true;
                cur.push(r);
                rec(pool, k, cur, used, out);
                cur.pop();
                used[i] = false;
            }
        }
    }
    rec(pool, k, &mut cur, &mut used, &mut out);
    out
}

fn rotor_name(r: HistoricalRotor) -> String {
    format!("{r:?}")
}

/// Rotor names left -> right, for candidate display (shared with hillclimb).
pub(crate) fn order_names(order: &[HistoricalRotor]) -> Vec<String> {
    order.iter().map(|&r| rotor_name(r)).collect()
}

/// Exhaustive crib search. Returns the top `top_n` candidates by
/// `(matches desc, quadgram score desc)`.
///
/// `checkpoint_path`: JSON resume file (created/updated per rotor order).
/// `on_progress`: called as `(orders_done, orders_total)` after each order.
pub fn solve_crib(
    cfg: &CribConfig,
    scorer: &QuadgramScorer,
    checkpoint_path: Option<&Path>,
    on_progress: Option<&(dyn Fn(usize, usize) + Sync)>,
) -> Result<Vec<CribCandidate>, EnigmaError> {
    if cfg.cipher.is_empty() {
        return Err(EnigmaError::SolverSetup("ciphertext is empty".into()));
    }
    if cfg.crib.is_empty() {
        return Err(EnigmaError::SolverSetup("crib is empty".into()));
    }
    if cfg.crib.len() > cfg.cipher.len() {
        return Err(EnigmaError::SolverSetup(format!(
            "crib ({} chars) longer than ciphertext ({} chars)",
            cfg.crib.len(),
            cfg.cipher.len()
        )));
    }
    let offsets: Vec<usize> = match cfg.crib_offset {
        Some(o) => {
            if o + cfg.crib.len() > cfg.cipher.len() {
                return Err(EnigmaError::SolverSetup(format!(
                    "crib offset {o} + crib len {} exceeds ciphertext len {}",
                    cfg.crib.len(),
                    cfg.cipher.len()
                )));
            }
            vec![o]
        }
        None => (0..=cfg.cipher.len() - cfg.crib.len()).collect(),
    };
    // Validate pool: distinct stepping rotors.
    {
        let mut seen = [false; 8];
        for &r in &cfg.rotor_pool {
            let idx = match r {
                HistoricalRotor::I => 0,
                HistoricalRotor::II => 1,
                HistoricalRotor::III => 2,
                HistoricalRotor::IV => 3,
                HistoricalRotor::V => 4,
                HistoricalRotor::VI => 5,
                HistoricalRotor::VII => 6,
                HistoricalRotor::VIII => 7,
                HistoricalRotor::Beta | HistoricalRotor::Gamma => {
                    return Err(EnigmaError::NonSteppingRotorNotAllowedHere);
                }
            };
            if seen[idx] {
                return Err(EnigmaError::DuplicateRotor);
            }
            seen[idx] = true;
        }
    }
    if let Some(fourth) = cfg.fourth {
        if fourth.spec().steps {
            return Err(EnigmaError::FourthRotorMustBeNonStepping);
        }
    }
    let rotor_count = if cfg.fourth.is_some() { 4 } else { 3 };
    if cfg.rotor_pool.len() < 3 {
        return Err(EnigmaError::SolverSetup(format!(
            "rotor pool needs >= 3 rotors, got {}",
            cfg.rotor_pool.len()
        )));
    }
    if cfg.rings.len() != rotor_count {
        return Err(EnigmaError::ConfigLengthMismatch {
            field: "rings",
            expected: rotor_count,
            got: cfg.rings.len(),
        });
    }

    // Full orders: M3 = pool P 3; M4 = [fourth] + pool P 3.
    let mut orders: Vec<Vec<HistoricalRotor>> = order_permutations(&cfg.rotor_pool, 3)
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
    // Deterministic: permutations already in pool order; M4 shares the suffix order.
    let _ = &mut orders;

    // Resume: skip finished orders, seed best list.
    let mut checkpoint = checkpoint_path
        .and_then(load_checkpoint)
        .unwrap_or_default();
    let mut best = checkpoint.best.clone();
    let mut done = checkpoint.done_orders.len();
    let total = orders.len();

    for order in &orders {
        let names: Vec<String> = order.iter().map(|&r| rotor_name(r)).collect();
        if checkpoint.done_orders.contains(&names) {
            continue;
        }
        // Base machine at all-A positions; clone + set per candidate position.
        let base_config = MachineConfig::new(
            order.clone(),
            cfg.rings.clone(),
            vec![0; rotor_count],
            cfg.reflector,
            cfg.plugs.clone(),
            cfg.etw.clone(),
        )?;
        let base_machine = base_config.build_machine()?;
        let total_positions: usize = 26usize.pow(rotor_count as u32);

        let mut found: Vec<CribCandidate> = (0..total_positions)
            .into_par_iter()
            .filter_map(|pos_idx| {
                let mut machine: EnigmaMachine = base_machine.clone();
                let mut digits = vec![0u8; rotor_count];
                let mut rest = pos_idx;
                for d in (0..rotor_count).rev() {
                    digits[d] = (rest % 26) as u8;
                    rest /= 26;
                }
                machine.set_positions(&digits).ok()?;
                // Decrypt full cipher once per position.
                let plain: Vec<u8> = cfg
                    .cipher
                    .iter()
                    .map(|&c| machine.encipher_char(c))
                    .collect();
                // Best offset for this position.
                let mut matches = 0usize;
                for &off in &offsets {
                    let m = cfg
                        .crib
                        .iter()
                        .zip(&plain[off..off + cfg.crib.len()])
                        .filter(|(a, b)| a == b)
                        .count();
                    matches = matches.max(m);
                }
                if matches >= cfg.min_matches {
                    Some(CribCandidate {
                        order: names.clone(),
                        positions: digits,
                        matches,
                        score: 0.0, // filled in after shortlist
                    })
                } else {
                    None
                }
            })
            .collect();

        // Quadgram tiebreak only for shortlisted candidates (cheap).
        for cand in &mut found {
            let mut machine = base_machine.clone();
            machine.set_positions(&cand.positions).ok();
            let plain: Vec<u8> = cfg
                .cipher
                .iter()
                .map(|&c| machine.encipher_char(c))
                .collect();
            cand.score = scorer.score_bytes(&plain);
        }
        best.extend(found);
        best.sort_by(|a, b| {
            b.matches.cmp(&a.matches).then(
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });
        best.truncate(cfg.top_n.max(1));

        checkpoint.done_orders.push(names);
        checkpoint.best = best.clone();
        done += 1;
        if let Some(path) = checkpoint_path {
            // Best-effort: a lost checkpoint only costs re-search, never correctness.
            let _ = std::fs::write(path, serde_json::to_string(&checkpoint).unwrap_or_default());
        }
        if let Some(cb) = on_progress {
            cb(done, total);
        }
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::{Lang, QuadgramScorer};
    use enigma_core::config::MachineConfig;

    fn encipher_helper(
        order: &[&str],
        rings: &str,
        pos: &str,
        text: &str,
    ) -> (Vec<u8>, MachineConfig) {
        let cfg = MachineConfig::from_strings(order, rings, pos, "B", "", "identity").unwrap();
        let mut m = cfg.build_machine().unwrap();
        let out = m.encipher_str(text);
        (encode_text(&out), cfg)
    }

    #[test]
    fn permutation_counts() {
        let pool = vec![
            HistoricalRotor::I,
            HistoricalRotor::II,
            HistoricalRotor::III,
        ];
        assert_eq!(order_permutations(&pool, 3).len(), 6);
        let pool5 = vec![
            HistoricalRotor::I,
            HistoricalRotor::II,
            HistoricalRotor::III,
            HistoricalRotor::IV,
            HistoricalRotor::V,
        ];
        assert_eq!(order_permutations(&pool5, 3).len(), 60);
    }

    #[test]
    fn crib_recovers_order_and_positions() {
        // True key: I II III, rings AAA, pos KDO, UKW-B, no plugs.
        let plaintext = "DIEANGRIFFBEGINNTBEIMORGENGRAUENALLEEINHEITENMELDENBEREITSCHAFT";
        let (cipher, _) = encipher_helper(&["I", "II", "III"], "AAA", "KDO", plaintext);
        let crib = encode_text("MORGENGRAUEN");
        let cfg = CribConfig {
            cipher,
            crib: crib.clone(),
            crib_offset: None,
            rotor_pool: vec![
                HistoricalRotor::I,
                HistoricalRotor::II,
                HistoricalRotor::III,
            ],
            fourth: None,
            rings: vec![0, 0, 0],
            reflector: ReflectorKind::B,
            plugs: vec![],
            etw: EtwKind::Identity,
            top_n: 3,
            min_matches: crib.len(),
        };
        let scorer = QuadgramScorer::new(Lang::De);
        let best = solve_crib(&cfg, &scorer, None, None).unwrap();
        assert!(!best.is_empty());
        let top = &best[0];
        assert_eq!(top.order, vec!["I", "II", "III"]);
        assert_eq!(top.positions_str(), "KDO");
        assert_eq!(top.matches, crib.len());
    }

    #[test]
    fn checkpoint_resume_gives_same_result() {
        let plaintext = "WETTERBERICHTFUERDIENAECHSTENTAGEVORHERGESAGTSONNIGUNDWARM";
        let (cipher, _) = encipher_helper(&["II", "I", "III"], "AAA", "BNK", plaintext);
        let crib = encode_text("NAECHSTEN");
        let path = std::env::temp_dir().join("enigma_crib_test_checkpoint.json");
        let _ = std::fs::remove_file(&path);
        let mkcfg = || CribConfig {
            cipher: cipher.clone(),
            crib: crib.clone(),
            crib_offset: None,
            rotor_pool: vec![
                HistoricalRotor::I,
                HistoricalRotor::II,
                HistoricalRotor::III,
            ],
            fourth: None,
            rings: vec![0, 0, 0],
            reflector: ReflectorKind::B,
            plugs: vec![],
            etw: EtwKind::Identity,
            top_n: 3,
            min_matches: crib.len(),
        };
        let scorer = QuadgramScorer::new(Lang::De);
        let first = solve_crib(&mkcfg(), &scorer, Some(&path), None).unwrap();
        assert!(path.exists(), "checkpoint file should be written");
        let second = solve_crib(&mkcfg(), &scorer, Some(&path), None).unwrap();
        assert_eq!(first, second);
        assert_eq!(first[0].positions_str(), "BNK");
        let _ = std::fs::remove_file(&path);
    }
}
