//! Tauri backend: thin command layer over enigma-core/solver/config.
//!
//! Typing state lives server-side ([`SessionState`]) because rotor stepping
//! is one-way: the frontend sends the whole input on every keystroke and the
//! backend diffs (single-step) or rewinds + replays (edits/pastes).
//! Searches run on worker threads and report through Tauri events.

use std::path::PathBuf;
use std::sync::Mutex;

use enigma_config::{AppConfig, MachineSection};
use enigma_core::{
    config::MachineConfig,
    machine::{EnigmaMachine, SignalTrace},
    pos_to_char,
};
use enigma_solver::{
    crib::{build_crib_config, encode_text, load_checkpoint, solve_crib, CribCandidate},
    hillclimb::{solve_blind_pool, BlindPoolConfig, BlindProgress},
    score::{Lang, QuadgramScorer},
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

// ---------------------------------------------------------------------------
// Typing session
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Session {
    machine: Option<EnigmaMachine>,
    start: Vec<u8>,
    letters: Vec<char>,
    cipher: Vec<char>,
    summary: String,
}

#[derive(Default)]
pub struct AppState {
    session: Mutex<Session>,
    crib_checkpoint: Mutex<Option<PathBuf>>,
    last_crib: Mutex<Option<StoredCrib>>,
}

/// Everything needed to re-decrypt a crib winner for preview/export.
#[derive(Clone)]
struct StoredCrib {
    rings: Vec<u8>,
    reflector: enigma_core::config::ReflectorKind,
    plugs: Vec<(u8, u8)>,
    etw: enigma_core::config::EtwKind,
    cipher: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineArgs {
    rotors: Vec<String>,
    rings: String,
    positions: String,
    reflector: String,
    plugs: String,
    etw: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceDto {
    input: u8,
    plug_in: u8,
    etw_in: u8,
    rotor_fwd: Vec<u8>,
    reflected: u8,
    rotor_bwd: Vec<u8>,
    etw_out: u8,
    output: u8,
    rotor_count: usize,
}

impl From<&SignalTrace> for TraceDto {
    fn from(t: &SignalTrace) -> Self {
        Self {
            input: t.input,
            plug_in: t.plug_in,
            etw_in: t.etw_in,
            rotor_fwd: t.rotor_fwd[..t.rotor_count].to_vec(),
            reflected: t.reflected,
            rotor_bwd: t.rotor_bwd[..t.rotor_count].to_vec(),
            etw_out: t.etw_out,
            output: t.output,
            rotor_count: t.rotor_count,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeResult {
    output: String,
    windows: String,
    trace: Option<TraceDto>,
}

fn layout_output(input: &str, cipher: &[char]) -> String {
    let mut out = String::with_capacity(input.len());
    let mut it = cipher.iter();
    for c in input.chars() {
        if c.is_ascii_alphabetic() {
            out.push(*it.next().unwrap_or(&'?'));
        } else {
            out.push(c);
        }
    }
    out
}

fn windows_of(machine: &EnigmaMachine) -> String {
    machine
        .positions()
        .iter()
        .map(|&p| pos_to_char(p))
        .collect()
}

#[tauri::command]
pub fn machine_load(args: MachineArgs, state: State<'_, AppState>) -> Result<String, String> {
    let names: Vec<&str> = args.rotors.iter().map(String::as_str).collect();
    let cfg = MachineConfig::from_strings(
        &names,
        &args.rings,
        &args.positions,
        &args.reflector,
        &args.plugs,
        &args.etw,
    )
    .map_err(|e| e.to_string())?;
    let machine = cfg.build_machine().map_err(|e| e.to_string())?;
    let summary = format!(
        "{} • rings {} • {:?} • {} • {:?}",
        names.join(" "),
        args.rings.to_ascii_uppercase(),
        cfg.reflector,
        if cfg.plugs.is_empty() {
            "no plugs".to_string()
        } else {
            format!("{} plugs", cfg.plugs.len())
        },
        cfg.etw,
    );
    let mut session = state.session.lock().map_err(|e| e.to_string())?;
    session.start = cfg.positions.clone();
    session.letters.clear();
    session.cipher.clear();
    session.summary = summary.clone();
    session.machine = Some(machine);
    Ok(summary)
}

#[tauri::command]
pub fn machine_type(full_text: String, state: State<'_, AppState>) -> Result<TypeResult, String> {
    let mut guard = state.session.lock().map_err(|e| e.to_string())?;
    let Session {
        machine,
        start,
        letters,
        cipher,
        ..
    } = &mut *guard;
    let Some(machine) = machine.as_mut() else {
        return Err("Load a machine first.".into());
    };
    let current: Vec<char> = full_text
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let mut last_trace = None;
    if current.len() == letters.len() + 1 && current.starts_with(letters) {
        if let Some(&c) = current.last() {
            let (out, trace) = machine.encipher_char_traced(c as u8 - b'A');
            letters.push(c);
            cipher.push(pos_to_char(out));
            last_trace = Some(TraceDto::from(&trace));
        }
    } else if current != *letters {
        let start = start.clone();
        machine.set_positions(&start).map_err(|e| e.to_string())?;
        letters.clear();
        cipher.clear();
        for &c in &current {
            let (out, trace) = machine.encipher_char_traced(c as u8 - b'A');
            letters.push(c);
            cipher.push(pos_to_char(out));
            last_trace = Some(TraceDto::from(&trace));
        }
    }
    Ok(TypeResult {
        output: layout_output(&full_text, cipher),
        windows: windows_of(machine),
        trace: last_trace,
    })
}

#[tauri::command]
pub fn machine_clear(state: State<'_, AppState>) -> Result<String, String> {
    let mut guard = state.session.lock().map_err(|e| e.to_string())?;
    let Session {
        machine,
        start,
        letters,
        cipher,
        ..
    } = &mut *guard;
    let Some(machine) = machine.as_mut() else {
        return Err("Load a machine first.".into());
    };
    let start = start.clone();
    machine.set_positions(&start).map_err(|e| e.to_string())?;
    letters.clear();
    cipher.clear();
    Ok(windows_of(machine))
}

// ---------------------------------------------------------------------------
// Config files
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionDto {
    rotors: Vec<String>,
    rings: String,
    positions: String,
    reflector: String,
    plugs: String,
    etw: String,
}

impl From<&MachineSection> for SectionDto {
    fn from(s: &MachineSection) -> Self {
        Self {
            rotors: s.rotors.clone().unwrap_or_default(),
            rings: s.rings.clone().unwrap_or_default(),
            positions: s.positions.clone().unwrap_or_default(),
            reflector: s.reflector.clone().unwrap_or_default(),
            plugs: s.plugs.clone().unwrap_or_default(),
            etw: s.etw.clone().unwrap_or_else(|| "identity".into()),
        }
    }
}

#[tauri::command]
fn config_read(path: String) -> Result<SectionDto, String> {
    Ok(SectionDto::from(
        &AppConfig::load(PathBuf::from(path).as_path())
            .map_err(|e| e.to_string())?
            .machine,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionArgs {
    rotors: Vec<String>,
    rings: String,
    positions: String,
    reflector: String,
    plugs: String,
    etw: String,
}

#[tauri::command]
fn config_write(path: String, section: SectionArgs) -> Result<(), String> {
    let cfg = AppConfig {
        machine: MachineSection {
            rotors: Some(section.rotors),
            rings: Some(section.rings),
            positions: Some(section.positions),
            reflector: Some(section.reflector),
            plugs: Some(section.plugs),
            etw: Some(section.etw),
        },
        ..AppConfig::default()
    };
    let text = cfg.to_toml_string().map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

#[tauri::command]
fn text_read(path: String) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

/// Read a bundled demo cipher (`crib_cipher.txt` / `blind_cipher.txt`),
/// searched next to the executable so it works from any working directory.
#[tauri::command]
fn demo_text(name: String) -> Result<String, String> {
    const ALLOWED: [&str; 2] = ["crib_cipher.txt", "blind_cipher.txt"];
    if !ALLOWED.contains(&name.as_str()) {
        return Err("unknown demo file".into());
    }
    let mut candidates = vec![PathBuf::from("examples").join(&name)];
    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors().take(4) {
            candidates.push(ancestor.join("examples").join(&name));
        }
    }
    for candidate in candidates {
        if let Ok(text) = std::fs::read_to_string(&candidate) {
            return Ok(text);
        }
    }
    Err("demo file not found next to the app.".into())
}

#[tauri::command]
fn text_write(path: String, contents: String) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Native dialogs (paths only — bytes move through the commands above)
// ---------------------------------------------------------------------------

#[tauri::command]
fn dialog_open_toml(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    Ok(app
        .dialog()
        .file()
        .add_filter("TOML config", &["toml"])
        .blocking_pick_file()
        .and_then(|p| p.into_path().ok())
        .and_then(|p| p.to_str().map(str::to_string)))
}

#[tauri::command]
fn dialog_save_toml(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    Ok(app
        .dialog()
        .file()
        .add_filter("TOML config", &["toml"])
        .set_file_name("machine.toml")
        .blocking_save_file()
        .and_then(|p| p.into_path().ok())
        .and_then(|p| p.to_str().map(str::to_string)))
}

#[tauri::command]
fn dialog_save_txt(app: AppHandle, name: String) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    Ok(app
        .dialog()
        .file()
        .add_filter("Text file", &["txt"])
        .set_file_name(&name)
        .blocking_save_file()
        .and_then(|p| p.into_path().ok())
        .and_then(|p| p.to_str().map(str::to_string)))
}

// ---------------------------------------------------------------------------
// Crib search worker
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CribParams {
    pool: Vec<String>,
    fourth: Option<String>,
    rings: String,
    reflector: String,
    plugs: String,
    etw: String,
    lang: String,
    cipher: String,
    crib: String,
    offset: Option<usize>,
    top: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CribTick {
    done: usize,
    total: usize,
}

#[tauri::command]
fn crib_start(
    params: CribParams,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    use enigma_core::config::EtwKind;
    let lang: Lang = params.lang.parse().map_err(|e: String| e)?;
    let cfg = build_crib_config(
        &params.pool,
        params.fourth.as_deref(),
        &params.rings,
        &params.reflector,
        &params.plugs,
        &params.etw,
        &params.cipher,
        &params.crib,
        params.offset,
        params.top,
        None,
    )
    .map_err(|e| e.to_string())?;
    // Sanity: ETW string round-trips (historic searches use named wheels).
    let _ = EtwKind::parse(&params.etw).map_err(|e| e.to_string())?;
    let checkpoint: PathBuf =
        std::env::temp_dir().join(format!("enigmasaurus-crib-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&checkpoint);
    {
        let mut slot = state.crib_checkpoint.lock().map_err(|e| e.to_string())?;
        *slot = Some(checkpoint.clone());
    }
    {
        let mut stored = state.last_crib.lock().map_err(|e| e.to_string())?;
        *stored = Some(StoredCrib {
            rings: cfg.rings.clone(),
            reflector: cfg.reflector,
            plugs: cfg.plugs.clone(),
            etw: cfg.etw.clone(),
            cipher: cfg.cipher.clone(),
        });
    }
    std::thread::spawn(move || {
        let scorer = QuadgramScorer::new(lang);
        let cb = |done: usize, total: usize| {
            let _ = app.emit("crib-progress", CribTick { done, total });
        };
        match solve_crib(&cfg, &scorer, Some(&checkpoint), Some(&cb)) {
            Ok(_) => {
                let _ = app.emit("crib-done", ());
            }
            Err(e) => {
                let _ = app.emit("crib-error", e.to_string());
            }
        }
    });
    Ok(params.top)
}

#[tauri::command]
fn crib_live(state: State<'_, AppState>) -> Result<Vec<CribCandidate>, String> {
    let slot = state.crib_checkpoint.lock().map_err(|e| e.to_string())?;
    let Some(path) = slot.as_ref() else {
        return Ok(Vec::new());
    };
    Ok(load_checkpoint(path).map(|c| c.best).unwrap_or_default())
}

/// Decrypt the full cipher with a winning crib setting (preview + export).
#[tauri::command]
fn crib_preview(
    order: Vec<String>,
    positions: Vec<u8>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    use enigma_core::{
        config::parse_rotor_name, machine::EnigmaMachine, plugboard::Plugboard, rotor::Rotor,
    };
    let stored = state.last_crib.lock().map_err(|e| e.to_string())?;
    let Some(stored) = stored.as_ref() else {
        return Err("Run a search first.".into());
    };
    let mut rotors = Vec::with_capacity(order.len());
    for (name, (&ring, &pos)) in order.iter().zip(stored.rings.iter().zip(positions.iter())) {
        rotors.push(
            Rotor::historical(
                parse_rotor_name(name).map_err(|e| e.to_string())?,
                ring,
                pos,
            )
            .map_err(|e| e.to_string())?,
        );
    }
    let machine_reflector = stored.reflector.build().map_err(|e| e.to_string())?;
    let etw = stored.etw.build().map_err(|e| e.to_string())?;
    let mut machine = EnigmaMachine::new(
        etw,
        rotors,
        machine_reflector,
        Plugboard::from_pairs(&stored.plugs).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(stored
        .cipher
        .iter()
        .map(|&c| pos_to_char(machine.encipher_char(c)))
        .collect())
}

// ---------------------------------------------------------------------------
// Blind search worker
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlindParams {
    pool: Vec<String>,
    fourth: Option<String>,
    rings: String,
    reflector: String,
    etw: String,
    lang: String,
    cipher: String,
    max_plugs: usize,
    top_positions: usize,
    restarts: usize,
    seed: u64,
    top: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlindTick {
    stage: String,
    done: usize,
    total: usize,
}

#[tauri::command]
fn blind_start(params: BlindParams, app: AppHandle) -> Result<(), String> {
    use enigma_core::config::{parse_letters, parse_rotor_name, EtwKind, ReflectorKind};
    let lang: Lang = params.lang.parse().map_err(|e: String| e)?;
    let mut rotor_pool = Vec::with_capacity(params.pool.len());
    for name in &params.pool {
        rotor_pool.push(parse_rotor_name(name).map_err(|e| e.to_string())?);
    }
    let fourth = params
        .fourth
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(parse_rotor_name)
        .transpose()
        .map_err(|e| e.to_string())?;
    let cfg = BlindPoolConfig {
        cipher: encode_text(&params.cipher),
        rotor_pool,
        fourth,
        rings: parse_letters(&params.rings).map_err(|e| e.to_string())?,
        reflector: ReflectorKind::parse(&params.reflector).map_err(|e| e.to_string())?,
        etw: EtwKind::parse(&params.etw).map_err(|e| e.to_string())?,
        max_plugs: params.max_plugs,
        top_positions: params.top_positions,
        restarts: params.restarts,
        seed: params.seed,
        per_order_top: 1,
        top_n: params.top,
    };
    std::thread::spawn(move || {
        let scorer = QuadgramScorer::new(lang);
        let cb = |p: BlindProgress| {
            let (stage, done, total) = match p {
                BlindProgress::Scan { done, total } => ("scan", done, total),
                BlindProgress::Climb { done, total } => ("climb", done, total),
                BlindProgress::Order { done, total } => ("orders", done, total),
            };
            let _ = app.emit(
                "blind-progress",
                BlindTick {
                    stage: stage.into(),
                    done,
                    total,
                },
            );
        };
        match solve_blind_pool(&cfg, &scorer, Some(&cb)) {
            Ok(winners) => {
                let _ = app.emit("blind-done", winners);
            }
            Err(e) => {
                let _ = app.emit("blind-error", e.to_string());
            }
        }
    });
    Ok(())
}

pub fn handlers() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        machine_load,
        machine_type,
        machine_clear,
        config_read,
        config_write,
        text_read,
        text_write,
        demo_text,
        dialog_open_toml,
        dialog_save_toml,
        dialog_save_txt,
        crib_start,
        crib_live,
        crib_preview,
        blind_start,
    ]
}

pub fn app_state() -> AppState {
    AppState::default()
}

/// Re-exported so `main.rs` stays three lines; unit-tested below.
#[cfg(test)]
pub fn layout_for_test(input: &str, cipher: &[char]) -> String {
    layout_output(input, cipher)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_keeps_non_letters() {
        assert_eq!(layout_for_test("AB CD!", &['X', 'Y', 'Z', 'W']), "XY ZW!");
    }

    #[test]
    fn trace_dto_covers_rotor_count() {
        let t = SignalTrace {
            input: 0,
            plug_in: 0,
            etw_in: 0,
            rotor_fwd: [1, 2, 3, 0],
            reflected: 4,
            rotor_bwd: [5, 6, 7, 0],
            etw_out: 8,
            output: 9,
            rotor_count: 3,
        };
        let dto = TraceDto::from(&t);
        assert_eq!(dto.rotor_fwd, vec![1, 2, 3]);
        assert_eq!(dto.rotor_bwd, vec![5, 6, 7]);
    }
}
