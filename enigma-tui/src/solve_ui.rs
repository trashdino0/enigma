//! Solver progress screens: a worker thread searches while this thread draws.
//!
//! Quitting a screen ends the process along with its worker; rerun long
//! searches through the CLI with `--checkpoint-file` to resume.

use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use enigma_core::pos_to_char;
use enigma_solver::{
    crib::{load_checkpoint, solve_crib, CribCandidate, CribConfig},
    hillclimb::{solve_blind_pool, BlindCandidate, BlindPoolConfig, BlindProgress},
    score::{Lang, QuadgramScorer},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Frame, Terminal,
};

// ---------------------------------------------------------------------------
// Crib screen
// ---------------------------------------------------------------------------

enum CribMsg {
    Tick(usize, usize),
    Done,
    Failed(String),
}

struct CribScreen {
    subtitle: String,
    crib_len: usize,
    done: usize,
    total: usize,
    started: Instant,
    checkpoint: PathBuf,
    finished: bool,
    error: Option<String>,
}

fn draw_crib(frame: &mut Frame, s: &CribScreen) {
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
                "{}/{} units • {:.0}s",
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
        format!("FAILED: {e} • Q/Esc menu")
    } else {
        "Q/Esc menu (ends the search)".to_string()
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

/// Watch a crib search; `Esc`/`Q` returns to the menu (worker ends too).
pub fn run_crib(cfg: CribConfig, lang: Lang, subtitle: String) -> io::Result<()> {
    let crib_len = cfg.crib.len();
    let checkpoint: PathBuf =
        std::env::temp_dir().join(format!("enigma-tui-solve-{}.json", std::process::id()));

    let (tx, rx) = mpsc::channel::<CribMsg>();
    let ckpt_path = checkpoint.clone();
    std::thread::spawn(move || {
        let scorer = QuadgramScorer::new(lang);
        let cb = |done: usize, total: usize| {
            let _ = tx.send(CribMsg::Tick(done, total));
        };
        match solve_crib(&cfg, &scorer, Some(&ckpt_path), Some(&cb)) {
            Ok(_) => {
                let _ = tx.send(CribMsg::Done);
            }
            Err(e) => {
                let _ = tx.send(CribMsg::Failed(e.to_string()));
            }
        }
    });

    with_terminal(|terminal| {
        let mut screen = CribScreen {
            subtitle,
            crib_len,
            done: 0,
            total: 1,
            started: Instant::now(),
            checkpoint: checkpoint.clone(),
            finished: false,
            error: None,
        };
        loop {
            for msg in rx.try_iter() {
                match msg {
                    CribMsg::Tick(done, total) => {
                        screen.done = done;
                        screen.total = total.max(1);
                    }
                    CribMsg::Done => screen.finished = true,
                    CribMsg::Failed(e) => {
                        screen.finished = true;
                        screen.error = Some(e);
                    }
                }
            }
            terminal.draw(|f| draw_crib(f, &screen))?;
            if poll_quit()? {
                break;
            }
        }
        Ok(())
    })?;
    let _ = std::fs::remove_file(&checkpoint);
    Ok(())
}

// ---------------------------------------------------------------------------
// Blind screen
// ---------------------------------------------------------------------------

enum BlindMsg {
    Progress(BlindProgress),
    Done(Vec<BlindCandidate>),
    Failed(String),
}

struct BlindScreen {
    subtitle: String,
    scan: (usize, usize),
    climb: (usize, usize),
    orders: (usize, usize),
    started: Instant,
    winners: Vec<BlindCandidate>,
    finished: bool,
    error: Option<String>,
}

fn positions_str(positions: &[u8]) -> String {
    positions.iter().map(|&p| pos_to_char(p)).collect()
}

fn plugs_str(plugs: &[(u8, u8)]) -> String {
    if plugs.is_empty() {
        return "-".to_string();
    }
    plugs
        .iter()
        .map(|&(a, b)| format!("{}{}", pos_to_char(a), pos_to_char(b)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn draw_blind(frame: &mut Frame, s: &BlindScreen) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Length(5), // stage counters
            Constraint::Min(6),    // winners
            Constraint::Length(3), // help
        ])
        .split(frame.area());

    frame.render_widget(
        Paragraph::new(s.subtitle.clone()).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Blind search "),
        ),
        rows[0],
    );

    let stage_line = |label: &str, (done, total): (usize, usize)| {
        let pct = if total == 0 {
            0.0
        } else {
            (done as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
        };
        format!("{label}: {done}/{total} ({pct:.0}%)")
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(stage_line("scan  ", s.scan)),
            Line::from(stage_line("climb ", s.climb)),
            Line::from(format!(
                "{} • {:.0}s",
                stage_line("orders", s.orders),
                s.started.elapsed().as_secs_f32()
            )),
        ])
        .block(Block::default().borders(Borders::ALL).title(" Stages ")),
        rows[1],
    );

    let mut lines: Vec<Line> = Vec::new();
    for (i, cand) in s.winners.iter().enumerate() {
        lines.push(Line::from(format!(
            "#{} {} pos {} rings {} plugs {} score {:.1}",
            i + 1,
            cand.order.join(" "),
            positions_str(&cand.positions),
            positions_str(&cand.rings),
            plugs_str(&cand.plugs),
            cand.score
        )));
        let preview: String = cand.plaintext.chars().take(160).collect();
        let ellipsis = if cand.plaintext.chars().count() > 160 {
            "…"
        } else {
            ""
        };
        lines.push(Line::from(format!("  {preview}{ellipsis}")));
    }
    if lines.is_empty() {
        lines.push(Line::from(if s.finished {
            "search finished — no winners".to_string()
        } else {
            "searching — winners appear when the first order completes...".to_string()
        }));
    }
    let title = if s.finished {
        " Results "
    } else {
        " Winners (live) "
    };
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(title)),
        rows[2],
    );

    let help = if let Some(e) = &s.error {
        format!("FAILED: {e} • Q/Esc menu")
    } else {
        "Q/Esc menu (ends the search)".to_string()
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

/// Watch a blind pool search; `Esc`/`Q` returns to the menu.
pub fn run_blind(mut cfg: BlindPoolConfig, lang: Lang, subtitle: String) -> io::Result<()> {
    use enigma_solver::hillclimb::load_blind_checkpoint;
    let checkpoint: PathBuf =
        std::env::temp_dir().join(format!("enigma-tui-blind-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&checkpoint);
    cfg.checkpoint = Some(checkpoint.clone());
    let (tx, rx) = mpsc::channel::<BlindMsg>();
    std::thread::spawn(move || {
        let scorer = QuadgramScorer::new(lang);
        let cb = |p: BlindProgress| {
            let _ = tx.send(BlindMsg::Progress(p));
        };
        match solve_blind_pool(&cfg, &scorer, Some(&cb)) {
            Ok(winners) => {
                let _ = tx.send(BlindMsg::Done(winners));
            }
            Err(e) => {
                let _ = tx.send(BlindMsg::Failed(e.to_string()));
            }
        }
    });

    with_terminal(|terminal| {
        let mut screen = BlindScreen {
            subtitle,
            scan: (0, 1),
            climb: (0, 1),
            orders: (0, 1),
            started: Instant::now(),
            winners: Vec::new(),
            finished: false,
            error: None,
        };
        loop {
            for msg in rx.try_iter() {
                match msg {
                    BlindMsg::Progress(p) => match p {
                        BlindProgress::Scan { done, total } => {
                            screen.scan = (done, total.max(1));
                        }
                        BlindProgress::Climb { done, total } => {
                            screen.climb = (done, total.max(1));
                        }
                        BlindProgress::Order { done, total } => {
                            screen.orders = (done, total.max(1));
                        }
                    },
                    BlindMsg::Done(winners) => {
                        screen.finished = true;
                        screen.winners = winners;
                    }
                    BlindMsg::Failed(e) => {
                        screen.finished = true;
                        screen.error = Some(e);
                    }
                }
            }
            // Live winners from the worker's checkpoint file.
            if !screen.finished {
                if let Some(ckpt) = load_blind_checkpoint(&checkpoint) {
                    screen.winners = ckpt.best;
                }
            }
            terminal.draw(|f| draw_blind(f, &screen))?;
            if poll_quit()? {
                break;
            }
        }
        Ok(())
    })?;
    let _ = std::fs::remove_file(&checkpoint);
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared terminal plumbing
// ---------------------------------------------------------------------------

/// Set up the alternate screen, run `body`, then always restore the terminal.
fn with_terminal(
    body: impl FnOnce(&mut Terminal<CrosstermBackend<std::io::Stdout>>) -> io::Result<()>,
) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = body(&mut terminal);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

/// Non-blocking quit check: `Esc`, `Q`, or `Ctrl-C` (which exits outright).
fn poll_quit() -> io::Result<bool> {
    if event::poll(Duration::from_millis(120))? {
        let Event::Key(key) = event::read()? else {
            return Ok(false);
        };
        if !crate::keys::is_press(&key) {
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            std::process::exit(0);
        }
        if matches!(
            key.code,
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q')
        ) {
            return Ok(true);
        }
    }
    Ok(false)
}
