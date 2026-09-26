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
use enigma_core::config::{parse_letters, parse_rotor_name, EtwKind, MachineConfig, ReflectorKind};
use enigma_core::rotor::HistoricalRotor;
use enigma_solver::crib::{build_crib_config, encode_text, solve_crib};
use enigma_solver::hillclimb::{solve_blind, solve_blind_pool, BlindConfig, BlindPoolConfig};
use enigma_solver::score::{Lang, QuadgramScorer};

#[derive(Parser, Debug)]
#[command(name = "enigma", version, about = "Historically exact Enigma M3/M4")]
struct Cli {
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
    #[arg(long, num_args = 3..=4, required = true, value_name = "ROTOR")]
    rotors: Vec<String>,

    /// Ring settings, one letter per rotor (e.g. AAA).
    #[arg(long, default_value = "AAA")]
    rings: String,

    /// Start window positions, one letter per rotor (e.g. AAA).
    #[arg(long, default_value = "AAA")]
    pos: String,

    /// Reflector: B, C, Thin-B, Thin-C (M4 needs a thin reflector).
    #[arg(long, default_value = "B")]
    reflector: String,

    /// Plugboard pairs, e.g. "AV BS CG" (empty = unpatched).
    #[arg(long, default_value = "")]
    plugs: String,

    /// Entry wheel: identity (Wehrmacht/Naval) or qwertz (D/K/Railway).
    #[arg(long, default_value = "identity")]
    etw: String,

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
    #[arg(long, default_value = "de")]
    lang: String,
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
    #[arg(long, default_value = "AAA")]
    rings: String,

    /// Fixed reflector: B, C, Thin-B, Thin-C (M4 needs a thin reflector).
    #[arg(long, default_value = "B")]
    reflector: String,

    /// Assumed-known plugboard pairs, e.g. "AV BS CG" (empty = none).
    #[arg(long, default_value = "")]
    plugs: String,

    /// Fixed entry wheel: identity (Wehrmacht/Naval) or qwertz (D/K/Railway).
    #[arg(long, default_value = "identity")]
    etw: String,

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
    #[arg(long)]
    min_matches: Option<usize>,

    /// How many top candidates to print.
    #[arg(long, default_value_t = 5)]
    top: usize,

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
    #[arg(long, default_value = "AAA")]
    rings: String,

    /// Fixed reflector: B, C, Thin-B, Thin-C.
    #[arg(long, default_value = "B")]
    reflector: String,

    /// Fixed entry wheel: identity or qwertz.
    #[arg(long, default_value = "identity")]
    etw: String,

    /// Ciphertext (takes precedence over --input and stdin).
    #[arg(long, conflicts_with = "input")]
    cipher: Option<String>,

    /// Ciphertext file (default: stdin).
    #[arg(long)]
    input: Option<PathBuf>,

    /// Plugboard pair cap (0 = positions only, no plug climb).
    #[arg(long, default_value_t = 10)]
    max_plugs: usize,

    /// Unplugged positions entering the plug climb.
    #[arg(long, default_value_t = 10)]
    top_positions: usize,

    /// Random restarts per position (plus one unplugged start).
    #[arg(long, default_value_t = 3)]
    restarts: usize,

    /// PRNG seed (deterministic runs).
    #[arg(long, default_value_t = 1)]
    seed: u64,

    /// How many winners to print.
    #[arg(long, default_value_t = 3)]
    top: usize,

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
    Io(io::Error),
    Msg(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Enigma(e) => write!(f, "{e}"),
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

impl From<io::Error> for CliError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
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

fn run(args: &RunArgs) -> Result<String, CliError> {
    let names: Vec<&str> = args.rotors.iter().map(String::as_str).collect();
    let config = MachineConfig::from_strings(
        &names,
        &args.rings,
        &args.pos,
        &args.reflector,
        &args.plugs,
        &args.etw,
    )?;
    let mut machine = config.build_machine()?;
    let input = read_text(args.text.as_ref(), args.input.as_ref())?;
    Ok(machine.encipher_str(&input))
}

fn run_solve_crib(args: &SolveCribArgs) -> Result<(), CliError> {
    let cipher_raw = read_text(args.cipher.as_ref(), args.input.as_ref())?;
    let lang = Lang::parse(&args.lang.lang).map_err(CliError::Msg)?;
    let cfg = build_crib_config(
        &args.rotors,
        args.fourth.as_deref(),
        &args.rings,
        &args.reflector,
        &args.plugs,
        &args.etw,
        &cipher_raw,
        &args.crib,
        args.crib_offset,
        args.top,
        args.min_matches,
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
        println!(
            "#{} order {} pos {} matches {}/{} score {:.1}",
            i + 1,
            cand.order.join(" "),
            cand.positions_str(),
            cand.matches,
            cfg.crib.len(),
            cand.score
        );
    }
    if let Some(path) = &args.output_json {
        let json = serde_json::to_string_pretty(&best).map_err(|e| CliError::Msg(e.to_string()))?;
        std::fs::write(path, json)?;
    }
    Ok(())
}

fn run_solve_blind(args: &SolveBlindArgs) -> Result<(), CliError> {
    let cipher_raw = read_text(args.cipher.as_ref(), args.input.as_ref())?;
    let cipher = encode_text(&cipher_raw);
    let lang = Lang::parse(&args.lang.lang).map_err(CliError::Msg)?;
    let rings = parse_letters(&args.rings)?;
    let reflector = ReflectorKind::parse(&args.reflector)?;
    let etw = EtwKind::parse(&args.etw)?;
    let scorer = QuadgramScorer::new(lang);
    eprintln!("blind: scanning positions...");
    if args.pool.is_empty() {
        let order = parse_pool(&args.rotors)?;
        let cfg = BlindConfig {
            cipher,
            order,
            rings,
            reflector,
            etw,
            max_plugs: args.max_plugs,
            top_positions: args.top_positions,
            restarts: args.restarts,
            seed: args.seed,
            top_n: args.top,
        };
        let best = solve_blind(&cfg, &scorer)?;
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
            rings,
            reflector,
            etw,
            max_plugs: args.max_plugs,
            top_positions: args.top_positions,
            restarts: args.restarts,
            seed: args.seed,
            per_order_top: 1,
            top_n: args.top,
        };
        let best = solve_blind_pool(&cfg, &scorer)?;
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
    let result = match &cli.command {
        Command::Encrypt(args) | Command::Decrypt(args) => run(args).and_then(|out| {
            if let Some(path) = &args.output {
                std::fs::write(path, &out).map_err(CliError::Io)
            } else {
                print!("{out}");
                Ok(())
            }
        }),
        Command::SolveCrib(args) => run_solve_crib(args),
        Command::SolveBlind(args) => run_solve_blind(args),
    };
    if let Err(e) = result {
        eprintln!("enigma: {e}");
        std::process::exit(1);
    }
}
