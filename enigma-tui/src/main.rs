//! `enigma-tui`: menu-driven Enigma terminal — no CLI flags required.
//!
//! The menu offers three modes, each with an in-TUI configuration form:
//! type (interactive enciphering), crib search, and blind search. CLI flags
//! below only *prefill* the forms.
//!
//! ```text
//! enigma-tui
//! enigma-tui --rotors I II III --reflector B --lang de
//! ```

mod app;
mod config_form;
mod solve_ui;
mod type_ui;

use std::io;

use clap::Parser;
use config_form::{ConfigForm, FormMode, Prefill, Ready};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};

/// All flags are optional prefills for the in-TUI forms.
#[derive(Parser, Debug)]
#[command(
    name = "enigma-tui",
    version,
    about = "Menu-driven Enigma M3/M4 terminal"
)]
struct Cli {
    /// Prefill rotors (space-separated names).
    #[arg(long, num_args = 1..=8, value_name = "ROTOR")]
    rotors: Vec<String>,

    /// Prefill ring settings (e.g. AAA).
    #[arg(long)]
    rings: Option<String>,

    /// Prefill start window positions (e.g. AAA).
    #[arg(long)]
    pos: Option<String>,

    /// Prefill reflector (B, C, Thin-B, Thin-C).
    #[arg(long)]
    reflector: Option<String>,

    /// Prefill plugboard pairs (e.g. "AV BS CG").
    #[arg(long)]
    plugs: Option<String>,

    /// Prefill entry wheel (identity / qwertz).
    #[arg(long)]
    etw: Option<String>,

    /// Prefill scoring language (de / en).
    #[arg(long)]
    lang: Option<String>,
}

fn main() {
    let cli = Cli::parse();
    let prefill = Prefill {
        rotors: cli.rotors,
        rings: cli.rings,
        positions: cli.pos,
        reflector: cli.reflector,
        plugs: cli.plugs,
        etw: cli.etw,
        lang: cli.lang,
    };
    if let Err(e) = menu_loop(&prefill) {
        eprintln!("enigma-tui: {e}");
        std::process::exit(1);
    }
}

const MENU_ITEMS: [(&str, &str); 3] = [
    ("1", "Type — encipher interactively with live signal path"),
    (
        "2",
        "Solve — known plaintext (crib search over orders × positions)",
    ),
    (
        "3",
        "Solve — ciphertext only (position scan + plugboard climb)",
    ),
];

fn draw_menu(frame: &mut Frame, selected: usize, prefill_summary: &str) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(frame.area());

    frame.render_widget(
        Paragraph::new("Machine, crib solver, and blind solver — configure everything below.")
            .block(Block::default().borders(Borders::ALL).title(" Enigma ")),
        rows[0],
    );

    let mut lines = Vec::new();
    for (i, (key, desc)) in MENU_ITEMS.iter().enumerate() {
        let marker = if i == selected { ">" } else { " " };
        let style = if i == selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::styled(format!("{marker} [{key}] {desc}"), style));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(format!("defaults: {prefill_summary}")));
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Modes ")),
        rows[1],
    );

    frame.render_widget(
        Paragraph::new("↑↓ move • Enter open • 1/2/3 shortcut • Q quit").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Keys ")
                .style(Style::default().fg(Color::DarkGray)),
        ),
        rows[2],
    );
}

fn menu_loop(prefill: &Prefill) -> io::Result<()> {
    let summary = format!(
        "rotors {} • rings {} • pos {} • reflector {} • lang {}",
        if prefill.rotors.is_empty() {
            "I II III".to_string()
        } else {
            prefill.rotors.join(" ")
        },
        prefill.rings.as_deref().unwrap_or("AAA"),
        prefill.positions.as_deref().unwrap_or("AAA"),
        prefill.reflector.as_deref().unwrap_or("B"),
        prefill.lang.as_deref().unwrap_or("de"),
    );
    let mut selected = 0usize;
    loop {
        // One terminal session per menu visit; mode screens manage their own
        // sessions and return here afterwards.
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let choice = (|| -> io::Result<Option<usize>> {
            loop {
                terminal.draw(|f| draw_menu(f, selected, &summary))?;
                let Event::Key(key) = event::read()? else {
                    continue;
                };
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    std::process::exit(0);
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                        return Ok(None);
                    }
                    KeyCode::Up => {
                        selected = selected.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        selected = (selected + 1).min(MENU_ITEMS.len() - 1);
                    }
                    KeyCode::Enter => return Ok(Some(selected)),
                    KeyCode::Char('1') => return Ok(Some(0)),
                    KeyCode::Char('2') => return Ok(Some(1)),
                    KeyCode::Char('3') => return Ok(Some(2)),
                    _ => {}
                }
            }
        })();

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        match choice? {
            None => return Ok(()),
            Some(0) => run_mode(FormMode::Type, prefill)?,
            Some(1) => run_mode(FormMode::Crib, prefill)?,
            Some(2) => run_mode(FormMode::Blind, prefill)?,
            Some(_) => {}
        }
    }
}

/// Form screen → validated config → mode screen → back to menu.
fn run_mode(mode: FormMode, prefill: &Prefill) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = (|| -> io::Result<()> {
        let mut form = ConfigForm::new(mode, prefill);
        loop {
            terminal.draw(|f| draw_form(f, &form))?;
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                std::process::exit(0);
            }
            if form.editing {
                match key.code {
                    KeyCode::Enter => form.toggle_edit(),
                    KeyCode::Esc => form.toggle_edit(),
                    KeyCode::Backspace => form.backspace(),
                    KeyCode::Char(c) if key.modifiers.is_empty() => form.push_char(c),
                    _ => {}
                }
                continue;
            }
            match key.code {
                KeyCode::Up => form.move_up(),
                KeyCode::Down => form.move_down(),
                KeyCode::Tab => form.move_down(),
                KeyCode::BackTab => form.move_up(),
                KeyCode::Enter => form.toggle_edit(),
                KeyCode::Esc => return Ok(()),
                KeyCode::F(5) => {
                    // Validated: leave the form session; the mode screen
                    // runs its own session and we return to the menu after.
                    if let Some(ready) = start_mode(&mut form)? {
                        return launch(ready);
                    }
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

/// Validate the form and build the mode input. Errors stay on the form.
fn start_mode(form: &mut ConfigForm) -> io::Result<Option<Ready>> {
    let ready = match form.mode {
        FormMode::Type => match form.build_type() {
            Ok((cfg, echo)) => match crate::app::App::new(cfg, echo) {
                Ok(app) => Ready::Type(app),
                Err(e) => {
                    form.error = Some(e.to_string());
                    return Ok(None);
                }
            },
            Err(e) => {
                form.error = Some(e);
                return Ok(None);
            }
        },
        FormMode::Crib => match form.build_crib() {
            Ok((cfg, lang, subtitle)) => Ready::Crib(cfg, lang, subtitle),
            Err(e) => {
                form.error = Some(e);
                return Ok(None);
            }
        },
        FormMode::Blind => match form.build_blind() {
            Ok((cfg, lang, subtitle)) => Ready::Blind(cfg, lang, subtitle),
            Err(e) => {
                form.error = Some(e);
                return Ok(None);
            }
        },
    };
    form.error = None;
    Ok(Some(ready))
}

/// Hand a validated config to its screen (own terminal session inside).
fn launch(ready: Ready) -> io::Result<()> {
    match ready {
        Ready::Type(mut app) => crate::type_ui::run_type(&mut app),
        Ready::Crib(cfg, lang, subtitle) => crate::solve_ui::run_crib(cfg, lang, subtitle),
        Ready::Blind(cfg, lang, subtitle) => crate::solve_ui::run_blind(cfg, lang, subtitle),
    }
}

fn display_value(value: &str) -> String {
    // Long cipher pastes would drown the form; show the tail.
    if value.chars().count() > 80 {
        format!(
            "…{}",
            value
                .chars()
                .skip(value.chars().count() - 77)
                .collect::<String>()
        )
    } else {
        value.to_string()
    }
}

fn draw_form(frame: &mut Frame, form: &ConfigForm) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(frame.area());

    frame.render_widget(
        Paragraph::new("Edit each field, then F5 to start.").block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", form.title())),
        ),
        rows[0],
    );

    let mut lines = Vec::new();
    for (i, field) in form.fields.iter().enumerate() {
        let cursor = if i == form.cursor { ">" } else { " " };
        let editing = if i == form.cursor && form.editing {
            " *editing*"
        } else {
            ""
        };
        let style = if i == form.cursor {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::styled(
            format!(
                "{cursor} {:<12} {:<30} ({}){editing}",
                field.label,
                display_value(&field.value),
                field.hint
            ),
            style,
        ));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(" Settings ")),
        rows[1],
    );

    let error: &str = form.error.as_deref().unwrap_or("");
    frame.render_widget(
        Paragraph::new(error).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Status ")
                .style(Style::default().fg(Color::Red)),
        ),
        rows[2],
    );

    frame.render_widget(
        Paragraph::new("↑↓/Tab move • Enter edit • Esc back • F5 start • Ctrl-C quit").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Keys ")
                .style(Style::default().fg(Color::DarkGray)),
        ),
        rows[3],
    );
}
