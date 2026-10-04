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
    /// Slots in `scan_rings` are exhaustively tried (26 each, max 676 combos).
    pub rings: Vec<u8>,
    /// Ring slots (0-indexed, left -> right) to scan exhaustively.
    pub scan_rings: Vec<usize>,
    /// Fixed reflector.
    pub reflector: ReflectorKind,
    /// Assumed-known plugboard pairs (ignored when `infer_plugs`).
    pub plugs: Vec<(u8, u8)>,
    /// Fixed entry wheel.
    pub etw: EtwKind,
    /// How many top candidates to return.
    pub top_n: usize,
    /// Minimum crib matches to shortlist a candidate (ignored when inferring).
    pub min_matches: usize,
    /// Recover unknown plugs by crib-anchored hill-climbing.
    pub infer_plugs: bool,
    /// Positions per unit entering plug inference.
    pub plug_top: usize,
    /// Plugboard pair cap for inference.
    pub max_plugs: usize,
}

/// One recovered setting, JSON-serializable for checkpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CribCandidate {
    /// Rotor names left -> right.
    pub order: Vec<String>,
    /// Window positions left -> right (`0..26`).
    pub positions: Vec<u8>,
    /// Ring settings left -> right (`0..26`, searched when scanning).
    pub rings: Vec<u8>,
    /// Plugboard pairs (assumed or inferred).
    pub plugs: Vec<(u8, u8)>,
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

    /// Rings as a string, e.g. `"AAA"`.
    pub fn rings_str(&self) -> String {
        self.rings.iter().map(|&p| pos_to_char(p)).collect()
    }
}

/// `{done_orders, best}` checkpoint written after each search unit.
///
/// Public so other frontends (e.g. the TUI progress view) can read the live
/// best list while a search runs. Keys look like `"I II III@AAA"`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CribCheckpoint {
    /// Search-unit keys already fully searched.
    pub done_orders: Vec<String>,
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
    scan_rings: &[usize],
    infer_plugs: bool,
    plug_top: usize,
    max_plugs: usize,
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
        scan_rings: scan_rings.to_vec(),
        reflector,
        plugs: plug_pairs,
        etw,
        top_n,
        min_matches,
        infer_plugs,
        plug_top,
        max_plugs,
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
    // Ring combos multiply each order (usually exactly one combo).
    let combos = ring_combos(&cfg.rings, &cfg.scan_rings)?;
    let units: Vec<(Vec<String>, Vec<HistoricalRotor>, Vec<u8>)> =
        order_permutations(&cfg.rotor_pool, 3)
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
            .flat_map(|order| {
                let names: Vec<String> = order.iter().map(|&r| rotor_name(r)).collect();
                combos
                    .iter()
                    .map(|rings| (names.clone(), order.clone(), rings.clone()))
                    .collect::<Vec<_>>()
            })
            .collect();

    // Resume: skip finished units, seed best list.
    let mut checkpoint = checkpoint_path
        .and_then(load_checkpoint)
        .unwrap_or_default();
    let mut best = checkpoint.best.clone();
    let mut done = checkpoint.done_orders.len();
    let total = units.len();

    for (names, order, rings) in &units {
        let key = unit_key(names, rings);
        if checkpoint.done_orders.contains(&key) {
            continue;
        }
        // Base machine at all-A positions; clone + set per candidate position.
        let base_config = MachineConfig::new(
            order.clone(),
            rings.clone(),
            vec![0; rotor_count],
            cfg.reflector,
            cfg.plugs.clone(),
            cfg.etw.clone(),
        )?;
        let base_machine = base_config.build_machine()?;
        let total_positions: usize = 26usize.pow(rotor_count as u32);

        // Positions scan (parallel): best crib matches per position.
        let mut scanned: Vec<(usize, Vec<u8>)> = (0..total_positions)
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
                Some((best_offset_matches(&cfg.crib, &plain, &offsets), digits))
            })
            .collect();

        let mut found: Vec<CribCandidate> = if cfg.infer_plugs {
            // Keep the most promising positions, then recover plugs on the crib.
            scanned.sort_by_key(|a| std::cmp::Reverse(a.0));
            scanned.truncate(cfg.plug_top.max(1));
            scanned
                .into_par_iter()
                .map(|(_, digits)| {
                    let (plugs, matches) = climb_plugs_crib(
                        &base_machine,
                        &digits,
                        &cfg.cipher,
                        &cfg.crib,
                        &offsets,
                        &cfg.plugs,
                        cfg.max_plugs,
                    );
                    CribCandidate {
                        order: names.clone(),
                        positions: digits,
                        rings: rings.clone(),
                        plugs,
                        matches,
                        score: 0.0, // filled in below
                    }
                })
                .collect()
        } else {
            scanned
                .into_iter()
                .filter(|(matches, _)| *matches >= cfg.min_matches)
                .map(|(matches, digits)| CribCandidate {
                    order: names.clone(),
                    positions: digits,
                    rings: rings.clone(),
                    plugs: cfg.plugs.clone(),
                    matches,
                    score: 0.0, // filled in below
                })
                .collect()
        };

        // Quadgram tiebreak only for shortlisted candidates (cheap).
        for cand in &mut found {
            let plain = decrypt_with(&base_machine, &cand.positions, &cand.plugs, &cfg.cipher);
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

        checkpoint.done_orders.push(key);
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

/// Ring combinations: `base` with every value tried in `slots`.
/// Capped at 676 (two slots) — beyond that the search stops being feasible.
pub(crate) fn ring_combos(base: &[u8], slots: &[usize]) -> Result<Vec<Vec<u8>>, EnigmaError> {
    for &s in slots {
        if s >= base.len() {
            return Err(EnigmaError::SolverSetup(format!(
                "ring slot {s} out of range for {} rotors",
                base.len()
            )));
        }
    }
    let mut combos = vec![base.to_vec()];
    for &slot in slots {
        let mut next = Vec::with_capacity(combos.len() * 26);
        for combo in &combos {
            for v in 0..26u8 {
                let mut c = combo.clone();
                c[slot] = v;
                next.push(c);
            }
        }
        combos = next;
        if combos.len() > 676 {
            return Err(EnigmaError::SolverSetup(
                "ring scan exceeds 676 combinations; scan at most 2 slots".into(),
            ));
        }
    }
    Ok(combos)
}

/// Checkpoint key for one search unit (order + rings).
fn unit_key(names: &[String], rings: &[u8]) -> String {
    format!(
        "{}@{}",
        names.join(" "),
        rings.iter().map(|&p| pos_to_char(p)).collect::<String>()
    )
}

/// Best crib match count over all offsets.
fn best_offset_matches(crib: &[u8], plain: &[u8], offsets: &[usize]) -> usize {
    offsets
        .iter()
        .map(|&off| {
            crib.iter()
                .zip(&plain[off..off + crib.len()])
                .filter(|(a, b)| a == b)
                .count()
        })
        .max()
        .unwrap_or(0)
}

/// Decrypt with explicit positions + plugs (plug-climb evaluations).
fn decrypt_with(
    base: &EnigmaMachine,
    positions: &[u8],
    plugs: &[(u8, u8)],
    cipher: &[u8],
) -> Vec<u8> {
    use enigma_core::plugboard::Plugboard;
    let mut machine = base.clone();
    if machine.set_positions(positions).is_err() {
        return Vec::new();
    }
    if let Ok(pb) = Plugboard::from_pairs(plugs) {
        machine.set_plugboard(pb);
    }
    cipher.iter().map(|&c| machine.encipher_char(c)).collect()
}

/// Greedy plug recovery anchored on the crib: maximize crib matches from
/// `start` plugs. Returns the pairs and their match count.
fn climb_plugs_crib(
    base: &EnigmaMachine,
    positions: &[u8],
    cipher: &[u8],
    crib: &[u8],
    offsets: &[usize],
    start: &[(u8, u8)],
    max_plugs: usize,
) -> (Vec<(u8, u8)>, usize) {
    use crate::hillclimb::{neighbours, sorted_pairs};
    let score = |pairs: &[(u8, u8)]| -> usize {
        best_offset_matches(crib, &decrypt_with(base, positions, pairs, cipher), offsets)
    };
    let mut best_pairs = sorted_pairs(start.to_vec());
    let mut best_score = score(&best_pairs);
    for _ in 0..200 {
        let mut cands = neighbours(&best_pairs, max_plugs);
        cands.sort();
        cands.dedup();
        let mut improved = false;
        for cand in cands {
            let cand = sorted_pairs(cand);
            let s = score(&cand);
            if s > best_score {
                best_score = s;
                best_pairs = cand;
                improved = true;
            }
        }
        if !improved {
            break;
        }
    }
    (best_pairs, best_score)
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

    fn base_crib_config(cipher: Vec<u8>, crib: Vec<u8>) -> CribConfig {
        CribConfig {
            cipher,
            crib,
            crib_offset: None,
            rotor_pool: vec![
                HistoricalRotor::I,
                HistoricalRotor::II,
                HistoricalRotor::III,
            ],
            fourth: None,
            rings: vec![0, 0, 0],
            scan_rings: vec![],
            reflector: ReflectorKind::B,
            plugs: vec![],
            etw: EtwKind::Identity,
            top_n: 3,
            min_matches: 0, // set per test below
            infer_plugs: false,
            plug_top: 50,
            max_plugs: 10,
        }
    }

    #[test]
    fn crib_recovers_order_and_positions() {
        // True key: I II III, rings AAA, pos KDO, UKW-B, no plugs.
        let plaintext = "DIEANGRIFFBEGINNTBEIMORGENGRAUENALLEEINHEITENMELDENBEREITSCHAFT";
        let (cipher, _) = encipher_helper(&["I", "II", "III"], "AAA", "KDO", plaintext);
        let crib = encode_text("MORGENGRAUEN");
        let mut cfg = base_crib_config(cipher, crib.clone());
        cfg.min_matches = crib.len();
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
        let mkcfg = || {
            let mut cfg = base_crib_config(cipher.clone(), crib.clone());
            cfg.min_matches = crib.len();
            cfg
        };
        let scorer = QuadgramScorer::new(Lang::De);
        let first = solve_crib(&mkcfg(), &scorer, Some(&path), None).unwrap();
        assert!(path.exists(), "checkpoint file should be written");
        let second = solve_crib(&mkcfg(), &scorer, Some(&path), None).unwrap();
        assert_eq!(first, second);
        assert_eq!(first[0].positions_str(), "BNK");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ring_scan_recovers_unknown_ring() {
        // True rings DAA; search assumes AAA but scans slot 0.
        let plaintext = "DIEANGRIFFBEGINNTBEIMORGENGRAUENALLEEINHEITENMELDENBEREITSCHAFT";
        let (cipher, _) = encipher_helper(&["I", "II", "III"], "DAA", "KDO", plaintext);
        let crib = encode_text("MORGENGRAUEN");
        let mut cfg = base_crib_config(cipher.clone(), crib.clone());
        cfg.min_matches = crib.len();
        cfg.scan_rings = vec![0];
        let scorer = QuadgramScorer::new(Lang::De);
        let best = solve_crib(&cfg, &scorer, None, None).unwrap();
        assert!(!best.is_empty());
        let top = &best[0];
        assert_eq!(top.matches, crib.len());
        // Ring/position pairs form equivalence classes (same pos-ring offset
        // enciphers identically until turnover diverges), so any full-match
        // key must decrypt the WHOLE message — assert that instead of exact
        // equality with the true key.
        let names: Vec<&str> = top.order.iter().map(String::as_str).collect();
        let mcfg = MachineConfig::from_strings(
            &names,
            &top.rings_str(),
            &top.positions_str(),
            "B",
            "",
            "identity",
        )
        .unwrap();
        let mut m = mcfg.build_machine().unwrap();
        let encoded = encode_text(plaintext);
        let decrypted: Vec<u8> = cipher.iter().map(|&c| m.encipher_char(c)).collect();
        assert_eq!(decrypted, encoded);
    }

    #[test]
    fn ring_scan_cap_rejects_three_slots() {
        let mut cfg = base_crib_config(vec![0; 30], encode_text("ABCDEF"));
        cfg.scan_rings = vec![0, 1, 2];
        let scorer = QuadgramScorer::new(Lang::De);
        assert!(solve_crib(&cfg, &scorer, None, None).is_err());
    }

    #[test]
    fn infer_plugs_recovers_positions_and_plugs() {
        // True key: I II III, AAA, pos MKL, plugs AV BS CG.
        let plaintext = "DIEANGRIFFBEGINNTBEIMORGENGRAUENALLEEINHEITENMELDENBEREITSCHAFTX";
        let machine_cfg = MachineConfig::from_strings(
            &["I", "II", "III"],
            "AAA",
            "MKL",
            "B",
            "AV BS CG",
            "identity",
        )
        .unwrap();
        let mut m = machine_cfg.build_machine().unwrap();
        let cipher = encode_text(&m.encipher_str(plaintext));
        let crib = encode_text("MORGENGRAUEN");
        let mut cfg = base_crib_config(cipher, crib.clone());
        cfg.infer_plugs = true;
        cfg.plug_top = 30;
        cfg.max_plugs = 6;
        let scorer = QuadgramScorer::new(Lang::De);
        let best = solve_crib(&cfg, &scorer, None, None).unwrap();
        assert!(!best.is_empty());
        let top = &best[0];
        assert_eq!(top.positions_str(), "MKL", "positions recovered");
        assert_eq!(top.matches, crib.len(), "full crib recovered");
        for pair in [("A", "V"), ("B", "S"), ("C", "G")] {
            let (a, b) = (
                pair.0.chars().next().unwrap() as u8 - b'A',
                pair.1.chars().next().unwrap() as u8 - b'A',
            );
            assert!(
                top.plugs.contains(&(a.min(b), a.max(b))),
                "plug {}{} recovered, got {:?}",
                pair.0,
                pair.1,
                top.plugs
            );
        }
    }
}
