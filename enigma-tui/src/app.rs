//! TUI application state: typing, undo-by-replay, reset, signal-path text.
//!
//! All rendering input is derived here as plain strings so the trace
//! formatting is unit-testable without a terminal.

use enigma_core::{
    config::MachineConfig,
    machine::{EnigmaMachine, SignalTrace},
    pos_to_char, EnigmaError,
};

/// Live Enigma session: owns the machine plus the typed buffer.
///
/// Undo is implemented as rebuild-from-config + replay, because rotor
/// stepping is one-way (like the real machine — there is no un-keypress).
#[derive(Debug)]
pub struct App {
    config: MachineConfig,
    rotor_names: Vec<String>,
    machine: EnigmaMachine,
    /// Plaintext letters typed so far (already uppercased).
    pub input: Vec<char>,
    /// Cipher letters aligned 1:1 with `input`.
    pub output: Vec<char>,
    /// Most recent keypress trace for the signal-path pane.
    pub last_trace: Option<SignalTrace>,
}

impl App {
    /// Start a session from a validated config.
    pub fn new(config: MachineConfig, rotor_names: Vec<String>) -> Result<Self, EnigmaError> {
        let machine = config.build_machine()?;
        Ok(Self {
            config,
            rotor_names,
            machine,
            input: Vec::new(),
            output: Vec::new(),
            last_trace: None,
        })
    }

    /// One-line config summary for the header.
    pub fn config_summary(&self) -> String {
        self.config.to_string()
    }

    /// Current window letters, left -> right (e.g. `['A','B','C']`).
    pub fn window_letters(&self) -> Vec<char> {
        self.machine
            .positions()
            .iter()
            .map(|&p| pos_to_char(p))
            .collect()
    }

    /// Ring letters, left -> right.
    pub fn ring_letters(&self) -> Vec<char> {
        self.config.rings.iter().map(|&p| pos_to_char(p)).collect()
    }

    /// Rotor names, left -> right.
    pub fn rotor_names(&self) -> &[String] {
        &self.rotor_names
    }

    /// Encipher one letter key (assumes caller uppercased A-Z).
    pub fn type_letter(&mut self, c: char) {
        debug_assert!(c.is_ascii_uppercase());
        let (out, trace) = self.machine.encipher_char_traced(c as u8 - b'A');
        self.input.push(c);
        self.output.push(pos_to_char(out));
        self.last_trace = Some(trace);
    }

    /// Undo the last letter by rebuilding and replaying the buffer.
    pub fn backspace(&mut self) {
        if self.input.pop().is_none() {
            return;
        }
        self.rebuild_and_replay();
    }

    /// Clear the buffer and return rotors to the configured start.
    pub fn reset(&mut self) {
        self.input.clear();
        self.rebuild_and_replay();
    }

    fn rebuild_and_replay(&mut self) {
        // Config was valid at construction; rebuild cannot fail on shape.
        // If it somehow does, keep the old machine rather than panicking.
        if let Ok(mut fresh) = self.config.build_machine() {
            let mut out = Vec::with_capacity(self.input.len());
            let mut last = None;
            for &c in &self.input {
                let (o, t) = fresh.encipher_char_traced(c as u8 - b'A');
                out.push(pos_to_char(o));
                last = Some(t);
            }
            self.machine = fresh;
            self.output = out;
            self.last_trace = last;
        }
    }

    /// Full input/output text for the panes.
    pub fn input_text(&self) -> String {
        self.input.iter().collect()
    }

    /// Cipher text aligned with the input.
    pub fn output_text(&self) -> String {
        self.output.iter().collect()
    }
}

/// Render a [`SignalTrace`] as a one-line `STAGE:letter` chain, e.g.
///
/// ```text
/// IN:A STB:A ETW:A R3:C R2:X R1:M UKW:Q R1:Z R2:P R3:K ETW:K STB:K OUT:K
/// ```
///
/// `R1` is the leftmost rotor, `R{n}` the fast rightmost; `STB` = plugboard,
/// `UKW` = reflector. Pure function for tests and the TUI pane.
pub fn format_trace(t: &SignalTrace) -> String {
    let n = t.rotor_count;
    let c = |p: u8| pos_to_char(p);
    let mut parts = Vec::with_capacity(2 * n + 7);
    parts.push(format!("IN:{}", c(t.input)));
    parts.push(format!("STB:{}", c(t.plug_in)));
    parts.push(format!("ETW:{}", c(t.etw_in)));
    // Forward pass runs fast -> slow; label by left->right rotor number.
    for i in 0..n {
        parts.push(format!("R{}:{}", n - i, c(t.rotor_fwd[i])));
    }
    parts.push(format!("UKW:{}", c(t.reflected)));
    for i in 0..n {
        parts.push(format!("R{}:{}", i + 1, c(t.rotor_bwd[i])));
    }
    parts.push(format!("ETW:{}", c(t.etw_out)));
    parts.push(format!("STB:{}", c(t.output)));
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m3_app() -> App {
        let config =
            MachineConfig::from_strings(&["I", "II", "III"], "AAA", "AAA", "B", "", "identity")
                .unwrap();
        App::new(config, vec!["I".into(), "II".into(), "III".into()]).unwrap()
    }

    #[test]
    fn typing_advances_windows_and_output_matches_cli_vector() {
        let mut app = m3_app();
        for c in "AAAAA".chars() {
            app.type_letter(c);
        }
        assert_eq!(app.output_text(), "BDZGO");
        assert_eq!(app.window_letters(), vec!['A', 'A', 'F']);
    }

    #[test]
    fn backspace_replays_correctly() {
        let mut app = m3_app();
        for c in "ABC".chars() {
            app.type_letter(c);
        }
        let full_out = app.output_text();
        let full_pos = app.window_letters();
        app.backspace();
        assert_eq!(app.input_text(), "AB");
        assert_eq!(app.output_text(), &full_out[..2]);
        // Re-typing C must reproduce the exact third letter and windows.
        app.type_letter('C');
        assert_eq!(app.output_text(), full_out);
        assert_eq!(app.window_letters(), full_pos);
    }

    #[test]
    fn reset_returns_to_start() {
        let mut app = m3_app();
        for c in "HELLO".chars() {
            app.type_letter(c);
        }
        app.reset();
        assert!(app.input_text().is_empty());
        assert_eq!(app.window_letters(), vec!['A', 'A', 'A']);
    }

    #[test]
    fn trace_format_labels_stages() {
        let mut app = m3_app();
        app.type_letter('A');
        let line = format_trace(app.last_trace.as_ref().unwrap());
        assert!(line.starts_with("IN:A "), "{line}");
        assert!(line.contains("UKW:"), "{line}");
        assert!(
            line.ends_with(&format!("STB:{}", app.output_text())),
            "{line}"
        );
        // 3 rotors -> R1..R3 each appear twice (forward + backward).
        assert_eq!(line.matches("R1:").count(), 2, "{line}");
        assert_eq!(line.matches("R3:").count(), 2, "{line}");
    }
}
