//! Interactive encipher screen: live rotor windows + signal path.
//!
//! Returns to the menu on `Esc` (typing `Q`/`R` enciphers — every key stays a
//! valid Enigma input; reset is `Ctrl-R`).

use std::io;

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};

use crate::app::{format_trace, App};

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
        Paragraph::new("Type A-Z to encipher • Backspace undo • Ctrl-R reset • Esc menu").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Keys ")
                .style(Style::default().fg(Color::DarkGray)),
        ),
        rows[4],
    );
}

/// Run until `Esc` — the caller (menu) decides what happens next.
pub fn run_type(app: &mut App) -> io::Result<()> {
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
            if !crate::keys::is_press(&key) {
                continue;
            }
            // Ctrl-C quits the whole application.
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                std::process::exit(0);
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
