//! `enigma` CLI: historically exact M3/M4 encrypt/decrypt/solve.
//!
//! Encryption and decryption are the same operation (reflector symmetry), so
//! `encrypt` and `decrypt` share one code path and differ only in name, for
//! operator clarity. Example:
//!
//! ```text
//! enigma encrypt --rotors I II III --rings AAA --pos AAA \
//!   --reflector B --text AAAAA
//! # BDZGO
//! ```
//!
//! Solvers:
//!
//! ```text
//! enigma solve-crib --rotors I II III IV V --crib WETTER --lang de \
//!   --input cipher.txt --checkpoint-file crib.json
//! enigma solve-blind --rotors I II III --lang en --input cipher.txt
//! ```

use std::fmt;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use enigma_config::{AppConfig, ResolvedMachine};
use enigma_core::config::{parse_letters, parse_rotor_name, EtwKind, ReflectorKind};
use enigma_core::rotor::HistoricalRotor;
use enigma_solver::crib::{build_crib_config, encode_text, solve_crib};
use enigma_solver::hillclimb::{solve_blind, solve_blind_pool, BlindConfig, BlindPoolConfig};
use enigma_solver::score::{Lang, QuadgramScorer};

#[derive(Parser, Debug)]
#[command(name = "enigma", version, about = "Historically exact Enigma M3/M4")]
struct Cli {
    /// TOML config file (machine setups, profiles, solver defaults).
    /// CLI flags override the file: flags > --profile > [machine].
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// Named profile from the config file (inherits unset fields).
    #[arg(long, global = true)]
    profile: Option<String>,

    /// Named solver preset from the config file (overlays [solver]).
    #[arg(long, global = true)]
    solver_preset: Option<String>,

    /// Transliterate German umlauts before encrypting (ä→ae, ö→oe, ü→ue, ß→ss).
    #[arg(long, global = true)]
    transliterate: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Encipher plaintext (steps rotors from the given positions).
    Encrypt(RunArgs),
    /// Decipher ciphertext (identical operation; same settings as encryption).
    Decrypt(RunArgs),
    /// Known-plaintext search over rotor orders x positions (Bombe-style).
    SolveCrib(SolveCribArgs),
    /// Ciphertext-only attack: position scan + plugboard hill-climb.
    SolveBlind(SolveBlindArgs),
}

#[derive(Args, Debug)]
struct RunArgs {
    /// Rotor order, left -> right: 3 for M3, 4 for M4 (4th = Beta/Gamma).
    /// Optional when --config provides it.
    #[arg(long, num_args = 0..=4, value_name = "ROTOR")]
    rotors: Vec<String>,

    /// Ring settings, one letter per rotor (e.g. AAA).
    #[arg(long)]
    rings: Option<String>,

    /// Start window positions, one letter per rotor (e.g. AAA).
    #[arg(long)]
    pos: Option<String>,

    /// Reflector: B, C, Thin-B, Thin-C (M4 needs a thin reflector).
    #[arg(long)]
    reflector: Option<String>,

    /// Plugboard pairs, e.g. "AV BS CG" (empty = unpatched).
    #[arg(long)]
    plugs: Option<String>,

    /// Entry wheel: identity (Wehrmacht/Naval) or qwertz (D/K/Railway).
    #[arg(long)]
    etw: Option<String>,

    /// Input text (takes precedence over --input and stdin).
    #[arg(long, conflicts_with = "input")]
    text: Option<String>,

    /// Input file (default: stdin).
    #[arg(long)]
    input: Option<PathBuf>,

    /// Output file (default: stdout).
    #[arg(long)]
    output: Option<PathBuf>,
}

/// Shared `--lang de|en` scoring-table flag (German is the historical default).
#[derive(Args, Debug)]
struct LangArg {
    /// Scoring language: de (German, historical default) or en (English).
    #[arg(long)]
    lang: Option<String>,
}

#[derive(Args, Debug)]
struct SolveCribArgs {
    /// Rotor pool to permute (taken 3 per order).
    #[arg(long, num_args = 3..=8, required = true, value_name = "ROTOR")]
    rotors: Vec<String>,

    /// M4 fixed thin 4th rotor (Beta/Gamma), prepended to every order.
    #[arg(long, value_name = "ROTOR")]
    fourth: Option<String>,

    /// Fixed ring settings, one letter per rotor (3, or 4 with --fourth).
    #[arg(long)]
    rings: Option<String>,

    /// Fixed reflector: B, C, Thin-B, Thin-C (M4 needs a thin reflector).
    #[arg(long)]
    reflector: Option<String>,

    /// Assumed-known plugboard pairs, e.g. "AV BS CG" (empty = none).
    #[arg(long)]
    plugs: Option<String>,

    /// Fixed entry wheel: identity (Wehrmacht/Naval) or qwertz (D/K/Railway).
    #[arg(long)]
    etw: Option<String>,

    /// Known plaintext fragment (crib).
    #[arg(long, required = true)]
    crib: String,

    /// Crib offset in the ciphertext (default: scan every offset).
    #[arg(long)]
    crib_offset: Option<usize>,

    /// Ciphertext (takes precedence over --input and stdin).
    #[arg(long, conflicts_with = "input")]
    cipher: Option<String>,

    /// Ciphertext file (default: stdin).
    #[arg(long)]
    input: Option<PathBuf>,

    /// Minimum crib matches to shortlist (default: full crib length).
    /// Ignored with --infer-plugs.
    #[arg(long)]
    min_matches: Option<usize>,

    /// Ring slots to scan exhaustively, e.g. "0,1" (0-indexed, max 676 combos).
    #[arg(long)]
    ring_scan: Option<String>,

    /// Recover unknown plugs by crib-anchored hill-climbing.
    #[arg(long, default_value_t = false)]
    infer_plugs: bool,

    /// Positions per unit entering plug inference.
    #[arg(long)]
    plug_top: Option<usize>,

    /// Plugboard pair cap for inference.
    #[arg(long)]
    max_plugs: Option<usize>,

    /// How many top candidates to print.
    #[arg(long)]
    top: Option<usize>,

    /// JSON checkpoint file for resuming long (M4) searches.
    #[arg(long)]
    checkpoint_file: Option<PathBuf>,

    /// Write top candidates as pretty JSON to this file (in addition to text).
    #[arg(long)]
    output_json: Option<PathBuf>,

    #[command(flatten)]
    lang: LangArg,
}

#[derive(Args, Debug)]
struct SolveBlindArgs {
    /// Fixed rotor order, left -> right (3, or 4 with Beta/Gamma first).
    /// Omit when --pool searches orders.
    #[arg(long, num_args = 3..=4, value_name = "ROTOR", required_unless_present = "pool", conflicts_with = "pool")]
    rotors: Vec<String>,

    /// Rotor pool for order search (taken 3 per order). Alternative to --rotors.
    #[arg(long, num_args = 3..=8, value_name = "ROTOR")]
    pool: Vec<String>,

    /// M4 fixed thin 4th rotor (Beta/Gamma) for pool mode.
    #[arg(long, value_name = "ROTOR")]
    fourth: Option<String>,

    /// Fixed ring settings, one letter per rotor.
    #[arg(long)]
    rings: Option<String>,

    /// Fixed reflector: B, C, Thin-B, Thin-C.
    #[arg(long)]
    reflector: Option<String>,

    /// Fixed entry wheel: identity or qwertz.
    #[arg(long)]
    etw: Option<String>,

    /// Ciphertext (takes precedence over --input and stdin).
    #[arg(long, conflicts_with = "input")]
    cipher: Option<String>,

    /// Ciphertext file (default: stdin).
    #[arg(long)]
    input: Option<PathBuf>,

    /// Plugboard pair cap (0 = positions only, no plug climb).
    #[arg(long)]
    max_plugs: Option<usize>,

    /// Ring slots to scan exhaustively, e.g. "0,1" (0-indexed, max 676 combos).
    #[arg(long)]
    ring_scan: Option<String>,

    /// JSON checkpoint file resuming per finished pool order (pool mode).
    #[arg(long)]
    checkpoint_file: Option<PathBuf>,

    /// Unplugged positions entering the plug climb.
    #[arg(long)]
    top_positions: Option<usize>,

    /// Random restarts per position (plus one unplugged start).
    #[arg(long)]
    restarts: Option<usize>,

    /// PRNG seed (deterministic runs).
    #[arg(long)]
    seed: Option<u64>,

    /// How many winners to print.
    #[arg(long)]
    top: Option<usize>,

    /// Write winners as pretty JSON to this file (in addition to text).
    #[arg(long)]
    output_json: Option<PathBuf>,

    #[command(flatten)]
    lang: LangArg,
}

/// CLI failure modes: bad config, bad input, solver setup, I/O.
#[derive(Debug)]
enum CliError {
    Enigma(enigma_core::EnigmaError),
    Config(enigma_config::ConfigError),
    Io(io::Error),
    Msg(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Enigma(e) => write!(f, "{e}"),
            Self::Config(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Msg(m) => write!(f, "{m}"),
        }
    }
}

impl From<enigma_core::EnigmaError> for CliError {
    fn from(e: enigma_core::EnigmaError) -> Self {
        Self::Enigma(e)
    }
}

impl From<enigma_config::ConfigError> for CliError {
    fn from(e: enigma_config::ConfigError) -> Self {
        Self::Config(e)
    }
}

impl From<io::Error> for CliError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Loaded `--config` file plus `--profile`/`--solver-preset`, shared by subcommands.
struct FileCtx {
    file: Option<AppConfig>,
    profile: Option<String>,
    preset: Option<String>,
}

impl FileCtx {
    fn load(
        config: Option<&PathBuf>,
        profile: Option<&str>,
        preset: Option<&str>,
    ) -> Result<Self, CliError> {
        let file = config
            .map(|p| AppConfig::load(p.as_path()))
            .transpose()
            .map_err(CliError::Config)?;
        Ok(Self {
            file,
            profile: profile.map(str::to_string),
            preset: preset.map(str::to_string),
        })
    }

    /// Machine fields: flags > profile > [machine] > legacy flag defaults.
    fn machine_fields(
        &self,
        rotors: &[String],
        rings: Option<&str>,
        pos: Option<&str>,
        reflector: Option<&str>,
        plugs: Option<&str>,
        etw: Option<&str>,
    ) -> Result<ResolvedMachine, CliError> {
        if let Some(file) = &self.file {
            let mut resolved = file
                .machine_for(self.profile.as_deref())
                .map_err(CliError::Config)?;
            if !rotors.is_empty() {
                resolved.rotors = rotors.to_vec();
            }
            if let Some(v) = rings {
                resolved.rings = v.to_string();
            }
            if let Some(v) = pos {
                resolved.positions = v.to_string();
            }
            if let Some(v) = reflector {
                resolved.reflector = v.to_string();
            }
            if let Some(v) = plugs {
                resolved.plugs = v.to_string();
            }
            if let Some(v) = etw {
                resolved.etw = v.to_string();
            }
            return Ok(resolved);
        }
        if rotors.is_empty() {
            return Err(CliError::Msg(
                "--rotors is required without --config".into(),
            ));
        }
        Ok(ResolvedMachine {
            rotors: rotors.to_vec(),
            rings: rings.unwrap_or("AAA").to_string(),
            positions: pos.unwrap_or("AAA").to_string(),
            reflector: reflector.unwrap_or("B").to_string(),
            plugs: plugs.unwrap_or("").to_string(),
            etw: etw.unwrap_or("identity").to_string(),
        })
    }

    /// Merged `[solver]` + `--solver-preset` overlay (or blank defaults).
    fn solver_section(&self) -> Result<enigma_config::SolverSection, CliError> {
        match &self.file {
            Some(file) => file
                .solver_for(self.preset.as_deref())
                .map_err(CliError::Config),
            None => {
                if self.preset.is_some() {
                    return Err(CliError::Msg("--solver-preset needs --config".into()));
                }
                Ok(enigma_config::SolverSection::default())
            }
        }
    }

    /// Solver scalar: flag > preset > [solver] > hardcoded default.
    fn solver_opt(
        &self,
        flag: Option<usize>,
        file: impl FnOnce(&enigma_config::SolverSection) -> Option<usize>,
        default: usize,
    ) -> Result<usize, CliError> {
        let base = self.solver_section()?;
        Ok(flag.or_else(|| file(&base)).unwrap_or(default))
    }

    fn solver_lang(&self, flag: Option<&str>) -> Result<Lang, CliError> {
        let base = self.solver_section()?;
        let name = flag
            .map(str::to_string)
            .or(base.lang.clone())
            .unwrap_or_else(|| "de".into());
        Lang::parse(&name).map_err(CliError::Msg)
    }

    fn solver_seed(&self, flag: Option<u64>) -> Result<u64, CliError> {
        let base = self.solver_section()?;
        Ok(flag.or(base.seed).unwrap_or(1))
    }
}

/// Parse `"0,1"` into ring slot indices.
fn parse_slots(s: &str) -> Result<Vec<usize>, CliError> {
    s.split(',')
        .map(|part| {
            part.trim().parse::<usize>().map_err(|_| {
                CliError::Msg(format!(
                    "bad --ring-scan slot {part:?} (expected e.g. \"0,1\")"
                ))
            })
        })
        .collect()
}

fn transliterated(text: String, on: bool) -> String {
    if on {
        enigma_core::transliterate_de(&text)
    } else {
        text
    }
}

fn read_text(text: Option<&String>, input: Option<&PathBuf>) -> Result<String, CliError> {
    if let Some(text) = text {
        return Ok(text.clone());
    }
    if let Some(path) = input {
        return Ok(std::fs::read_to_string(path)?);
    }
    // Stdin (piped or interactive; Ctrl-D / Ctrl-Z ends).
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)?;
    Ok(buf)
}

fn parse_pool(names: &[String]) -> Result<Vec<HistoricalRotor>, CliError> {
    names
        .iter()
        .map(|s| parse_rotor_name(s).map_err(CliError::Enigma))
        .collect()
}

fn run(args: &RunArgs, ctx: &FileCtx, transliterate: bool) -> Result<String, CliError> {
    let resolved = ctx.machine_fields(
        &args.rotors,
        args.rings.as_deref(),
        args.pos.as_deref(),
        args.reflector.as_deref(),
        args.plugs.as_deref(),
        args.etw.as_deref(),
    )?;
    // Config files may define custom rotors; flag-only setups stay historic.
    let mut machine = match &ctx.file {
        Some(file) => file.build_machine(&resolved)?,
        None => resolved.to_machine_config()?.build_machine()?,
    };
    let input = read_text(args.text.as_ref(), args.input.as_ref())?;
    Ok(machine.encipher_str(&transliterated(input, transliterate)))
}

fn run_solve_crib(
    args: &SolveCribArgs,
    ctx: &FileCtx,
    transliterate: bool,
) -> Result<(), CliError> {
    let cipher_raw = read_text(args.cipher.as_ref(), args.input.as_ref())?;
    let cipher_raw = transliterated(cipher_raw, transliterate);
    let crib_raw = transliterated(args.crib.clone(), transliterate);
    let lang = ctx.solver_lang(args.lang.lang.as_deref())?;
    let file_machine = match &ctx.file {
        Some(file) => Some(
            file.machine_for(ctx.profile.as_deref())
                .map_err(CliError::Config)?,
        ),
        None => None,
    };
    let rings = args
        .rings
        .clone()
        .or_else(|| file_machine.as_ref().map(|m| m.rings.clone()))
        .unwrap_or_else(|| "AAA".into());
    let reflector = args
        .reflector
        .clone()
        .or_else(|| file_machine.as_ref().map(|m| m.reflector.clone()))
        .unwrap_or_else(|| "B".into());
    let plugs = args
        .plugs
        .clone()
        .or_else(|| file_machine.as_ref().map(|m| m.plugs.clone()))
        .unwrap_or_default();
    let etw = args
        .etw
        .clone()
        .or_else(|| file_machine.as_ref().map(|m| m.etw.clone()))
        .unwrap_or_else(|| "identity".into());
    let top = ctx.solver_opt(args.top, |s| s.top, 5)?;
    let solver = ctx.solver_section()?;
    let min_matches = args.min_matches.or(solver.min_matches);
    let scan_rings = match &args.ring_scan {
        Some(s) => parse_slots(s)?,
        None => Vec::new(),
    };
    let max_plugs = args.max_plugs.or(solver.max_plugs).unwrap_or(10);
    let plug_top = args.plug_top.or(solver.top_positions).unwrap_or(50);
    let cfg = build_crib_config(
        &args.rotors,
        args.fourth.as_deref(),
        &rings,
        &reflector,
        &plugs,
        &etw,
        &cipher_raw,
        &crib_raw,
        args.crib_offset,
        top,
        min_matches,
        &scan_rings,
        args.infer_plugs,
        plug_top,
        max_plugs,
    )?;
    let scorer = QuadgramScorer::new(lang);
    let on_progress = |done: usize, total: usize| {
        eprint!("\rcrib: {done}/{total} rotor orders");
        let _ = io::stderr().flush();
    };
    let best = solve_crib(
        &cfg,
        &scorer,
        args.checkpoint_file.as_deref(),
        Some(&on_progress),
    )?;
    eprintln!();
    if best.is_empty() {
        println!("no candidates (try a longer crib or --min-matches)");
        return Ok(());
    }
    for (i, cand) in best.iter().enumerate() {
        let plugs = if cand.plugs.is_empty() {
            "-".to_string()
        } else {
            cand.plugs
                .iter()
                .map(|&(a, b)| {
                    format!(
                        "{}{}",
                        enigma_core::pos_to_char(a),
                        enigma_core::pos_to_char(b)
                    )
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        println!(
            "#{} order {} pos {} rings {} matches {}/{} plugs {} score {:.1}",
            i + 1,
            cand.order.join(" "),
            cand.positions_str(),
            cand.rings_str(),
            cand.matches,
            cfg.crib.len(),
            plugs,
            cand.score
        );
    }
    if let Some(path) = &args.output_json {
        let json = serde_json::to_string_pretty(&best).map_err(|e| CliError::Msg(e.to_string()))?;
        std::fs::write(path, json)?;
    }
    Ok(())
}

fn run_solve_blind(
    args: &SolveBlindArgs,
    ctx: &FileCtx,
    transliterate: bool,
) -> Result<(), CliError> {
    let cipher_raw = read_text(args.cipher.as_ref(), args.input.as_ref())?;
    let cipher = encode_text(&transliterated(cipher_raw, transliterate));
    if cipher.len() < 100 {
        eprintln!(
            "note: ciphertext is short ({} letters); plug search overfits easily — \
             try --max-plugs 0 first, and check --lang matches the message",
            cipher.len()
        );
    }
    let lang = ctx.solver_lang(args.lang.lang.as_deref())?;
    let file_machine = match &ctx.file {
        Some(file) => Some(
            file.machine_for(ctx.profile.as_deref())
                .map_err(CliError::Config)?,
        ),
        None => None,
    };
    let rings = args
        .rings
        .clone()
        .or_else(|| file_machine.as_ref().map(|m| m.rings.clone()))
        .unwrap_or_else(|| "AAA".into());
    let reflector = args
        .reflector
        .clone()
        .or_else(|| file_machine.as_ref().map(|m| m.reflector.clone()))
        .unwrap_or_else(|| "B".into());
    let etw = args
        .etw
        .clone()
        .or_else(|| file_machine.as_ref().map(|m| m.etw.clone()))
        .unwrap_or_else(|| "identity".into());
    let reflector = ReflectorKind::parse(&reflector)?;
    let etw = EtwKind::parse(&etw)?;
    let max_plugs = ctx.solver_opt(args.max_plugs, |s| s.max_plugs, 10)?;
    let top_positions = ctx.solver_opt(args.top_positions, |s| s.top_positions, 10)?;
    let restarts = ctx.solver_opt(args.restarts, |s| s.restarts, 3)?;
    let seed = ctx.solver_seed(args.seed)?;
    let top = ctx.solver_opt(args.top, |s| s.top, 3)?;
    let scan_rings = match &args.ring_scan {
        Some(s) => parse_slots(s)?,
        None => Vec::new(),
    };
    let scorer = QuadgramScorer::new(lang);
    let rings = parse_letters(&rings)?;
    let on_progress = |p: enigma_solver::hillclimb::BlindProgress| {
        use enigma_solver::hillclimb::BlindProgress as BP;
        match p {
            BP::Scan { done, total } => {
                eprint!("\rblind scan: {done}/{total} positions");
            }
            BP::Climb { done, total } => {
                eprint!("\rblind climb: {done}/{total} positions");
            }
            BP::Order { done, total } => {
                eprint!("\rblind orders: {done}/{total}");
            }
        }
        let _ = io::stderr().flush();
    };
    if args.pool.is_empty() {
        let order = parse_pool(&args.rotors)?;
        let cfg = BlindConfig {
            cipher,
            order,
            rings: rings.clone(),
            scan_rings: scan_rings.clone(),
            reflector,
            etw: etw.clone(),
            max_plugs,
            top_positions,
            restarts,
            seed,
            top_n: top,
        };
        let best = solve_blind(&cfg, &scorer, Some(&on_progress))?;
        eprintln!();
        print_blind(&best, args.output_json.as_deref())?;
    } else {
        let pool = parse_pool(&args.pool)?;
        let fourth = args
            .fourth
            .as_deref()
            .map(parse_rotor_name)
            .transpose()
            .map_err(CliError::Enigma)?;
        let cfg = BlindPoolConfig {
            cipher,
            rotor_pool: pool,
            fourth,
            rings: rings.clone(),
            scan_rings: scan_rings.clone(),
            reflector,
            etw: etw.clone(),
            max_plugs,
            top_positions,
            restarts,
            seed,
            per_order_top: 1,
            top_n: top,
            checkpoint: args.checkpoint_file.clone(),
        };
        let best = solve_blind_pool(&cfg, &scorer, Some(&on_progress))?;
        eprintln!();
        print_blind(&best, args.output_json.as_deref())?;
    }
    Ok(())
}

fn print_blind(
    best: &[enigma_solver::hillclimb::BlindCandidate],
    output_json: Option<&std::path::Path>,
) -> Result<(), CliError> {
    if best.is_empty() {
        println!("no candidates");
    }
    for (i, cand) in best.iter().enumerate() {
        let pos: String = cand
            .positions
            .iter()
            .map(|&p| enigma_core::pos_to_char(p))
            .collect();
        let plugs = cand
            .plugs
            .iter()
            .map(|&(a, b)| {
                format!(
                    "{}{}",
                    enigma_core::pos_to_char(a),
                    enigma_core::pos_to_char(b)
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "#{} order {} pos {pos} plugs {plugs} score {:.1}",
            i + 1,
            cand.order.join(" "),
            cand.score
        );
        println!("  {}", cand.plaintext);
    }
    if let Some(path) = output_json {
        let json = serde_json::to_string_pretty(&best).map_err(|e| CliError::Msg(e.to_string()))?;
        std::fs::write(path, json)?;
    }
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    let ctx = match FileCtx::load(
        cli.config.as_ref(),
        cli.profile.as_deref(),
        cli.solver_preset.as_deref(),
    ) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("enigma: {e}");
            std::process::exit(1);
        }
    };
    let transliterate = cli.transliterate;
    let result = match &cli.command {
        Command::Encrypt(args) | Command::Decrypt(args) => {
            run(args, &ctx, transliterate).and_then(|out| {
                if let Some(path) = &args.output {
                    std::fs::write(path, &out).map_err(CliError::Io)
                } else {
                    print!("{out}");
                    Ok(())
                }
            })
        }
        Command::SolveCrib(args) => run_solve_crib(args, &ctx, transliterate),
        Command::SolveBlind(args) => run_solve_blind(args, &ctx, transliterate),
    };
    if let Err(e) = result {
        eprintln!("enigma: {e}");
        std::process::exit(1);
    }
}
