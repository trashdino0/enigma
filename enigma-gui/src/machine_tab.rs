//! Machine tab: dropdown/picker configuration plus live typing.
//!
//! The text buffer syncs incrementally (one keypress = one encipher) and
//! falls back to rebuild + replay for deletes, pastes, and mid-text edits —
//! rotor stepping is one-way, like the real machine.

use eframe::egui;
use enigma_config::{AppConfig, MachineSection};
use enigma_core::{
    config::MachineConfig,
    machine::{EnigmaMachine, SignalTrace},
    pos_to_char,
};

const SLOT_ROTORS: [&str; 8] = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII"];
const FOURTH_CHOICES: [&str; 3] = ["—", "Beta", "Gamma"];
const REFLECTORS: [&str; 4] = ["B", "C", "Thin-B", "Thin-C"];
const ETWS: [&str; 2] = ["identity", "qwertz"];

pub struct MachineTab {
    slots: [String; 4],
    rings: String,
    positions: String,
    reflector: String,
    plugs: String,
    etw: String,
    machine: Option<EnigmaMachine>,
    start_positions: Vec<u8>,
    summary: String,
    error: Option<String>,
    input: String,
    letters: Vec<char>,
    cipher_letters: Vec<char>,
    last_trace: Option<SignalTrace>,
}

impl Default for MachineTab {
    fn default() -> Self {
        Self {
            slots: ["I".into(), "II".into(), "III".into(), "—".into()],
            rings: "AAA".into(),
            positions: "AAA".into(),
            reflector: "B".into(),
            plugs: String::new(),
            etw: "identity".into(),
            machine: None,
            start_positions: Vec::new(),
            summary: "Press “Load machine”.".into(),
            error: None,
            input: String::new(),
            letters: Vec::new(),
            cipher_letters: Vec::new(),
            last_trace: None,
        }
    }
}

impl MachineTab {
    /// Build the machine from the form. Field errors surface in the UI.
    fn load(&mut self) {
        self.error = None;
        let mut names: Vec<&str> = Vec::new();
        for s in &self.slots[..3] {
            names.push(s.as_str());
        }
        if self.slots[3] != "—" {
            names.push(self.slots[3].as_str());
        }
        match MachineConfig::from_strings(
            &names,
            &self.rings,
            &self.positions,
            &self.reflector,
            &self.plugs,
            &self.etw,
        ) {
            Ok(cfg) => match cfg.build_machine() {
                Ok(machine) => {
                    self.start_positions = cfg.positions.clone();
                    let rings: String = cfg.rings.iter().map(|&p| pos_to_char(p)).collect();
                    self.summary = format!(
                        "{} • rings {rings} • {:?} • {} • {:?}",
                        names.join(" "),
                        cfg.reflector,
                        if cfg.plugs.is_empty() {
                            "no plugs".to_string()
                        } else {
                            format!("{} plugs", cfg.plugs.len())
                        },
                        cfg.etw,
                    );
                    self.machine = Some(machine);
                    self.reset_buffers();
                }
                Err(e) => self.error = Some(e.to_string()),
            },
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn reset_buffers(&mut self) {
        self.input.clear();
        self.letters.clear();
        self.cipher_letters.clear();
        self.last_trace = None;
    }

    /// Current form as a config section (GUI "save config").
    fn to_section(&self) -> MachineSection {
        let mut rotors: Vec<String> = self.slots[..3].iter().map(|s| s.to_string()).collect();
        if self.slots[3] != "—" {
            rotors.push(self.slots[3].clone());
        }
        MachineSection {
            rotors: Some(rotors),
            rings: Some(self.rings.clone()),
            positions: Some(self.positions.clone()),
            reflector: Some(self.reflector.clone()),
            plugs: Some(self.plugs.clone()),
            etw: Some(self.etw.clone()),
        }
    }

    /// Fill the form from a config section, then load it (GUI "load config").
    /// Unknown slot names are kept verbatim so validation reports them.
    fn apply_section(&mut self, section: &MachineSection) {
        if let Some(rotors) = &section.rotors {
            for (i, slot) in self.slots.iter_mut().enumerate() {
                *slot = rotors.get(i).cloned().unwrap_or_else(|| "—".into());
            }
        }
        if let Some(v) = &section.rings {
            self.rings = v.clone();
        }
        if let Some(v) = &section.positions {
            self.positions = v.clone();
        }
        if let Some(v) = &section.reflector {
            self.reflector = v.clone();
        }
        if let Some(v) = &section.plugs {
            self.plugs = v.clone();
        }
        if let Some(v) = &section.etw {
            self.etw = v.clone();
        }
        self.load();
    }

    /// Display text of the current output (what "save output" writes).
    fn output_text(&self) -> String {
        rebuild_output(&self.input, &self.cipher_letters)
    }

    /// Sync the cipher buffer with the edited input text.
    fn sync(&mut self) {
        let Some(machine) = self.machine.as_mut() else {
            return;
        };
        let current: Vec<char> = self
            .input
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        if current == self.letters {
            return;
        }
        // Fast path: exactly one letter appended.
        if current.len() == self.letters.len() + 1 && current.starts_with(&self.letters) {
            if let Some(&c) = current.last() {
                let (out, trace) = machine.encipher_char_traced(c as u8 - b'A');
                self.letters.push(c);
                self.cipher_letters.push(pos_to_char(out));
                self.last_trace = Some(trace);
                return;
            }
        }
        // Slow path (delete, paste, mid-edit): rewind and replay everything.
        let start = self.start_positions.clone();
        let _ = machine.set_positions(&start);
        self.letters.clear();
        self.cipher_letters.clear();
        self.last_trace = None;
        for &c in &current {
            let (out, trace) = machine.encipher_char_traced(c as u8 - b'A');
            self.letters.push(c);
            self.cipher_letters.push(pos_to_char(out));
            self.last_trace = Some(trace);
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("machine_config").show(ui, |ui| {
            ui.heading("Configuration");
            for (i, label) in [
                "Rotor 1 (left)",
                "Rotor 2",
                "Rotor 3 (fast)",
                "Rotor 4 (M4)",
            ]
            .iter()
            .enumerate()
            {
                let choices = if i < 3 {
                    &SLOT_ROTORS[..]
                } else {
                    &FOURTH_CHOICES[..]
                };
                egui::ComboBox::from_label(*label)
                    .selected_text(&self.slots[i])
                    .show_ui(ui, |ui| {
                        for c in choices {
                            ui.selectable_value(&mut self.slots[i], c.to_string(), *c);
                        }
                    });
            }
            egui::ComboBox::from_label("Reflector")
                .selected_text(&self.reflector)
                .show_ui(ui, |ui| {
                    for r in REFLECTORS {
                        ui.selectable_value(&mut self.reflector, r.to_string(), r);
                    }
                });
            egui::ComboBox::from_label("Entry wheel")
                .selected_text(&self.etw)
                .show_ui(ui, |ui| {
                    for e in ETWS {
                        ui.selectable_value(&mut self.etw, e.to_string(), e);
                    }
                });
            ui.horizontal(|ui| {
                ui.label("Rings");
                ui.text_edit_singleline(&mut self.rings);
            });
            ui.horizontal(|ui| {
                ui.label("Positions");
                ui.text_edit_singleline(&mut self.positions);
            });
            ui.horizontal(|ui| {
                ui.label("Plugs");
                ui.text_edit_singleline(&mut self.plugs)
                    .on_hover_text("Pairs like AV BS CG — empty means unpatched");
            });
            if ui.button("Load machine").clicked() {
                self.load();
            }
            ui.horizontal(|ui| {
                if ui.button("Load config…").clicked() {
                    if let Some(path) = crate::dialogs::pick_toml() {
                        match AppConfig::load(&path) {
                            Ok(cfg) => self.apply_section(&cfg.machine),
                            Err(e) => self.error = Some(e.to_string()),
                        }
                    }
                }
                if ui.button("Save config…").clicked() {
                    let cfg = AppConfig {
                        machine: self.to_section(),
                        ..AppConfig::default()
                    };
                    match cfg.to_toml_string() {
                        Ok(text) => {
                            if let Some(path) = crate::dialogs::save_toml() {
                                if let Err(e) = crate::dialogs::write_text(&path, &text) {
                                    self.error = Some(e);
                                }
                            }
                        }
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            });
            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::RED, e);
            }
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.label(&self.summary);
            if self.machine.is_none() {
                ui.weak("Load a machine to start typing.");
                return;
            }
            // Rotor windows.
            let windows: Vec<char> = self
                .machine
                .as_ref()
                .map(|m| m.positions().iter().map(|&p| pos_to_char(p)).collect())
                .unwrap_or_default();
            ui.horizontal(|ui| {
                for w in windows {
                    ui.label(egui::RichText::new(w.to_string()).size(40.0).strong());
                }
            });
            let changed = ui
                .add(
                    egui::TextEdit::multiline(&mut self.input)
                        .hint_text("Type plaintext here…")
                        .desired_rows(6)
                        .desired_width(f32::INFINITY),
                )
                .changed();
            if changed {
                self.sync();
            }
            ui.label(egui::RichText::new("Output").strong());
            egui::ScrollArea::vertical()
                .max_height(160.0)
                .show(ui, |ui| {
                    ui.label(rebuild_output(&self.input, &self.cipher_letters));
                });
            if let Some(t) = &self.last_trace {
                ui.label(egui::RichText::new("Signal path").strong());
                ui.monospace(crate::trace_line(t));
            }
            ui.horizontal(|ui| {
                if ui.button("Clear").clicked() {
                    self.reset_buffers();
                    if let Some(m) = self.machine.as_mut() {
                        let start = self.start_positions.clone();
                        let _ = m.set_positions(&start);
                    }
                }
                if ui.button("Save output…").clicked() {
                    if let Some(path) = crate::dialogs::save_txt("message.txt") {
                        if let Err(e) = crate::dialogs::write_text(&path, &self.output_text()) {
                            self.error = Some(e);
                        }
                    }
                }
            });
        });
    }
}

/// Rebuild display text: cipher letters fill letter slots, the rest passes
/// through untouched. Pure — unit-tested below.
fn rebuild_output(input: &str, cipher_letters: &[char]) -> String {
    let mut out = String::with_capacity(input.len());
    let mut it = cipher_letters.iter();
    for c in input.chars() {
        if c.is_ascii_alphabetic() {
            out.push(*it.next().unwrap_or(&'?'));
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_mirrors_layout() {
        assert_eq!(rebuild_output("AB CD!", &['X', 'Y', 'Z', 'W']), "XY ZW!");
        assert_eq!(rebuild_output("", &[]), "");
    }

    #[test]
    fn default_loads_and_enciphers_vector() {
        let mut tab = MachineTab::default();
        tab.load();
        assert!(tab.error.is_none());
        assert!(tab.machine.is_some());
        tab.input = "AAAAA".into();
        tab.sync();
        assert_eq!(rebuild_output(&tab.input, &tab.cipher_letters), "BDZGO");
    }

    #[test]
    fn bad_config_surfaces_error() {
        let mut tab = MachineTab {
            slots: ["I".into(), "I".into(), "II".into(), "—".into()],
            ..MachineTab::default()
        };
        tab.load();
        assert!(tab.error.is_some());
        assert!(tab.machine.is_none());
    }

    #[test]
    fn section_roundtrip_through_file() {
        let tab = MachineTab::default();
        let cfg = AppConfig {
            machine: tab.to_section(),
            ..AppConfig::default()
        };
        let path = std::env::temp_dir().join("enigma_gui_section_test.toml");
        std::fs::write(&path, cfg.to_toml_string().expect("serializes")).unwrap();
        let back = AppConfig::load(&path).expect("loads");
        std::fs::remove_file(&path).ok();
        let mut tab2 = MachineTab {
            positions: "ZZZ".into(),
            ..MachineTab::default()
        };
        tab2.apply_section(&back.machine);
        assert_eq!(tab2.positions, "AAA");
        assert!(tab2.machine.is_some(), "reloaded section builds");
        assert_eq!(tab2.output_text(), "");
    }
}
