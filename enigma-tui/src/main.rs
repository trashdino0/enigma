//! `enigma-tui`: live Enigma terminal.
//!
//! Type `A-Z` to encipher with visible stepping; the signal-path pane shows
//! every stage of the last keypress. `Backspace` undoes (rebuild + replay),
//! `Ctrl-R` resets to the configured start, `Esc`/`Ctrl-C` quits.
//!
//! ```text
//! enigma-tui --rotors I II III --rings AAA --pos AAA --reflector B
//! enigma-tui solve-crib --rotors I II III --crib WETTER --lang de \
//!   --cipher CIPHERTEXT...
//! ```

mod app;

use std::io;

use app::{format_trace, App};
use clap::{Args, Parser, Subcommand};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use enigma_core::config::MachineConfig;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Frame, Terminal,
};

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use enigma_solver::crib::{build_crib_config, load_checkpoint, solve_crib, CribCandidate};
use enigma_solver::score::{Lang, QuadgramScorer};

/// Live terminal: bare flags type interactively, `solve-crib` watches a search.
#[derive(Parser, Debug)]
#[command(name = "enigma-tui", version, about = "Live Enigma M3/M4 terminal")]
struct Cli {
    #[command(subcommand)]
    command: Option<TuiCommand>,

    #[command(flatten)]
    type_args: TypeArgs,
}

/// Same machine flags as the CLI; no I/O flags (the terminal *is* the I/O).
#[derive(Args, Debug)]
struct TypeArgs {
    /// Rotor order, left -> right: 3 for M3, 4 for M4 (4th = Beta/Gamma).
    #[arg(long, num_args = 3..=4, required = false, value_name = "ROTOR")]
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
}

#[derive(Subcommand, Debug)]
enum TuiCommand {
    /// Watch a known-plaintext search: progress gauge + live top candidates.
    SolveCrib(SolveTuiArgs),
}

#[derive(Args, Debug)]
struct SolveTuiArgs {
    /// Rotor pool to permute (taken 3 per order).
    #[arg(long, num_args = 1..=8, required = true, value_name = "ROTOR")]
    rotors: Vec<String>,

    /// M4 fixed thin 4th rotor (Beta/Gamma), prepended to every order.
    #[arg(long, value_name = "ROTOR")]
    fourth: Option<String>,

    /// Fixed ring settings, one letter per rotor (3, or 4 with --fourth).
    #[arg(long, default_value = "AAA")]
    rings: String,

    /// Fixed reflector: B, C, Thin-B, Thin-C.
    #[arg(long, default_value = "B")]
    reflector: String,

    /// Assumed-known plugboard pairs, e.g. "AV BS CG".
    #[arg(long, default_value = "")]
    plugs: String,

    /// Fixed entry wheel: identity or qwertz.
    #[arg(long, default_value = "identity")]
    etw: String,

    /// Ciphertext to attack.
    #[arg(long, required = true)]
    cipher: String,

    /// Known plaintext fragment.
    #[arg(long, required = true)]
    crib: String,

    /// Crib offset in the ciphertext (default: scan every offset).
    #[arg(long)]
    crib_offset: Option<usize>,

    /// Scoring language: de (default) or en.
    #[arg(long, default_value = "de")]
    lang: String,

    /// How many top candidates to show.
    #[arg(long, default_value_t = 5)]
    top: usize,
}

fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // config
            Constraint::Length(5), // rotor windows
            Constraint::Length(4), // signal path (wraps on narrow terms)
            Constraint::Min(6),    // input/output panes
            Constraint::Length(3), // help
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(app.config_summary()).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Enigma — configuration "),
        ),
        rows[0],
    );

    // Rotor windows: one column per rotor, window letter emphasized.
    let names = app.rotor_names();
    let windows = app.window_letters();
    let rings = app.ring_letters();
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Ratio(1, names.len() as u32); names.len()])
        .split(rows[1]);
    for (i, cell) in cols.iter().enumerate() {
        let body = vec![
            Line::from(vec![Span::styled(
                windows[i].to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(format!("ring {}", rings[i])),
        ];
        frame.render_widget(
            Paragraph::new(body)
                .block(Block::default().borders(Borders::ALL).title(format!(
                    " {} ",
                    names.get(i).map(String::as_str).unwrap_or("?")
                )))
                .centered(),
            *cell,
        );
    }

    let trace_line = app
        .last_trace
        .as_ref()
        .map(format_trace)
        .unwrap_or_else(|| "Type A-Z to encipher — the signal path appears here.".to_string());
    frame.render_widget(
        Paragraph::new(trace_line).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Signal path "),
        ),
        rows[2],
    );

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[3]);
    frame.render_widget(
        Paragraph::new(app.input_text())
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(" Input ")),
        panes[0],
    );
    frame.render_widget(
        Paragraph::new(app.output_text())
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(" Output ")),
        panes[1],
    );

    frame.render_widget(
        Paragraph::new("Type A-Z to encipher • Backspace undo • Ctrl-R reset • Esc quit").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Keys ")
                .style(Style::default().fg(Color::DarkGray)),
        ),
        rows[4],
    );
}

fn run_tui(app: &mut App) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = (|| -> io::Result<()> {
        loop {
            terminal.draw(|f| draw(f, app))?;
            let Event::Key(key) = event::read()? else {
                continue;
            };
            // Ctrl-C quits from anywhere.
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                return Ok(());
            }
            match key.code {
                KeyCode::Esc => return Ok(()),
                KeyCode::Backspace => app.backspace(),
                KeyCode::Char('r') | KeyCode::Char('R')
                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.reset();
                }
                KeyCode::Char(c) if c.is_ascii_alphabetic() && key.modifiers.is_empty() => {
                    app.type_letter(c.to_ascii_uppercase());
                }
                _ => {}
            }
        }
    })();

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Some(TuiCommand::SolveCrib(args)) => {
            if let Err(e) = run_solve_screen(args) {
                eprintln!("enigma-tui: {e}");
                std::process::exit(1);
            }
        }
        None => {
            let t = &cli.type_args;
            if t.rotors.is_empty() {
                eprintln!("enigma-tui: --rotors I II III required (or use solve-crib)");
                std::process::exit(2);
            }
            let names: Vec<&str> = t.rotors.iter().map(String::as_str).collect();
            let config = match MachineConfig::from_strings(
                &names,
                &t.rings,
                &t.pos,
                &t.reflector,
                &t.plugs,
                &t.etw,
            ) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("enigma-tui: {e}");
                    std::process::exit(1);
                }
            };
            let mut app = match App::new(config, t.rotors.clone()) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("enigma-tui: {e}");
                    std::process::exit(1);
                }
            };
            if let Err(e) = run_tui(&mut app) {
                eprintln!("enigma-tui: {e}");
                std::process::exit(1);
            }
        }
    }
}

enum SolveMsg {
    Tick(usize, usize),
    Done,
    Failed(String),
}

struct SolveScreen {
    subtitle: String,
    crib_len: usize,
    done: usize,
    total: usize,
    started: Instant,
    checkpoint: PathBuf,
    finished: bool,
    error: Option<String>,
}

fn draw_solve(frame: &mut Frame, s: &SolveScreen) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Length(3), // progress gauge
            Constraint::Min(6),    // candidates
            Constraint::Length(3), // help
        ])
        .split(frame.area());

    frame.render_widget(
        Paragraph::new(s.subtitle.clone()).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Crib search "),
        ),
        rows[0],
    );

    let ratio = if s.total == 0 {
        0.0
    } else {
        (s.done as f64 / s.total as f64).clamp(0.0, 1.0)
    };
    frame.render_widget(
        Gauge::default()
            .block(Block::default().borders(Borders::ALL).title(" Progress "))
            .gauge_style(Style::default().fg(Color::Yellow))
            .ratio(ratio)
            .label(format!(
                "{}/{} orders • {:.0}s",
                s.done,
                s.total,
                s.started.elapsed().as_secs_f32()
            )),
        rows[1],
    );

    // Live best list comes from the worker's checkpoint file (written per
    // finished rotor order), so the UI needs no shared solver state.
    let mut lines: Vec<Line> = Vec::new();
    if let Some(ckpt) = load_checkpoint(&s.checkpoint) {
        for (i, cand) in ckpt.best.iter().enumerate() {
            lines.push(Line::from(format!(
                "#{} {} pos {} matches {}/{} score {:.1}",
                i + 1,
                cand.order.join(" "),
                candidate_positions(cand),
                cand.matches,
                s.crib_len,
                cand.score
            )));
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(if s.finished {
            "search finished — no candidates (try a longer crib)".to_string()
        } else {
            "searching — first candidates appear after the first rotor order...".to_string()
        }));
    }
    let title = if s.finished {
        " Results "
    } else {
        " Top candidates (live) "
    };
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title)),
        rows[2],
    );

    let help = if let Some(e) = &s.error {
        format!("FAILED: {e} • Q/Esc quit")
    } else {
        "Q/Esc quit (quitting stops the display; the search thread ends with the process)"
            .to_string()
    };
    frame.render_widget(
        Paragraph::new(help).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Keys ")
                .style(Style::default().fg(Color::DarkGray)),
        ),
        rows[3],
    );
}

fn candidate_positions(cand: &CribCandidate) -> String {
    cand.positions_str()
}

/// Run a crib search on a worker thread; this thread renders progress.
/// Quitting ends the process along with the worker; rerun the same search
/// through the CLI with `--checkpoint-file` to resume where it stopped.
fn run_solve_screen(args: SolveTuiArgs) -> io::Result<()> {
    let lang =
        Lang::parse(&args.lang).map_err(|m| io::Error::new(io::ErrorKind::InvalidInput, m))?;
    let cfg = build_crib_config(
        &args.rotors,
        args.fourth.as_deref(),
        &args.rings,
        &args.reflector,
        &args.plugs,
        &args.etw,
        &args.cipher,
        &args.crib,
        args.crib_offset,
        args.top,
        None,
    )
    .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
    let crib_len = cfg.crib.len();
    let subtitle = format!(
        "pool {} • crib {} chars • lang {}",
        args.rotors.join(" "),
        crib_len,
        args.lang,
    );
    let checkpoint: PathBuf =
        std::env::temp_dir().join(format!("enigma-tui-solve-{}.json", std::process::id()));

    let (tx, rx) = mpsc::channel::<SolveMsg>();
    let ckpt_path = checkpoint.clone();
    std::thread::spawn(move || {
        let scorer = QuadgramScorer::new(lang);
        let cb = |done: usize, total: usize| {
            let _ = tx.send(SolveMsg::Tick(done, total));
        };
        match solve_crib(&cfg, &scorer, Some(&ckpt_path), Some(&cb)) {
            Ok(_) => {
                let _ = tx.send(SolveMsg::Done);
            }
            Err(e) => {
                let _ = tx.send(SolveMsg::Failed(e.to_string()));
            }
        }
    });

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut screen = SolveScreen {
        subtitle,
        crib_len,
        done: 0,
        total: 1,
        started: Instant::now(),
        checkpoint: checkpoint.clone(),
        finished: false,
        error: None,
    };
    let result = (|| -> io::Result<()> {
        loop {
            // Drain progress messages without blocking the UI.
            for msg in rx.try_iter() {
                match msg {
                    SolveMsg::Tick(done, total) => {
                        screen.done = done;
                        screen.total = total.max(1);
                    }
                    SolveMsg::Done => screen.finished = true,
                    SolveMsg::Failed(e) => {
                        screen.finished = true;
                        screen.error = Some(e);
                    }
                }
            }
            terminal.draw(|f| draw_solve(f, &screen))?;
            if event::poll(Duration::from_millis(120))? {
                let Event::Key(key) = event::read()? else {
                    continue;
                };
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(());
                }
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(()),
                    _ => {}
                }
            }
        }
    })();

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    let _ = std::fs::remove_file(&checkpoint);
    result
}
