//! Crib solver tab: form, background search, live results table.

use std::path::PathBuf;
use std::sync::mpsc;

use eframe::egui;
use enigma_config::MachineSection;
use enigma_core::pos_to_char;
use enigma_solver::{
    crib::{build_crib_config, load_checkpoint, solve_crib, CribCandidate, CribConfig},
    score::{Lang, QuadgramScorer},
};

const REFLECTORS: [&str; 4] = ["B", "C", "Thin-B", "Thin-C"];
const ETWS: [&str; 2] = ["identity", "qwertz"];
const LANGS: [&str; 2] = ["de", "en"];
const FOURTHS: [&str; 3] = ["", "Beta", "Gamma"];

/// Built-in demo: German crib hunt that always succeeds.
const DEMO_CIPHER: &str = include_str!("../../examples/crib_cipher.txt");
const DEMO_CRIB: &str = "MORGENGRAUEN";

enum CribMsg {
    Tick(usize, usize),
    Done,
    Failed(String),
}

pub struct CribTab {
    pool: String,
    fourth: String,
    rings: String,
    reflector: String,
    plugs: String,
    etw: String,
    lang: String,
    cipher: String,
    crib: String,
    offset: String,
    top: String,
    error: Option<String>,
    running: Option<mpsc::Receiver<CribMsg>>,
    done: usize,
    total: usize,
    checkpoint: Option<PathBuf>,
    finished: bool,
    results: Vec<CribCandidate>,
    selected: usize,
    last_cfg: Option<CribConfig>,
    preview: String,
}

impl Default for CribTab {
    fn default() -> Self {
        Self {
            pool: "I II III".into(),
            fourth: String::new(),
            rings: "AAA".into(),
            reflector: "B".into(),
            plugs: String::new(),
            etw: "identity".into(),
            lang: "de".into(),
            cipher: String::new(),
            crib: String::new(),
            offset: String::new(),
            top: "5".into(),
            error: None,
            running: None,
            done: 0,
            total: 1,
            checkpoint: None,
            finished: false,
            results: Vec::new(),
            selected: 0,
            last_cfg: None,
            preview: String::new(),
        }
    }
}

impl CribTab {
    fn start(&mut self) {
        self.error = None;
        if enigma_solver::crib::encode_text(&self.cipher).is_empty() {
            self.error = Some("Paste a ciphertext first — or press “Fill demo”.".into());
            return;
        }
        if enigma_solver::crib::encode_text(&self.crib).is_empty() {
            self.error = Some("Type the guessed word (crib) first.".into());
            return;
        }
        let pool: Vec<String> = self.pool.split_whitespace().map(str::to_string).collect();
        let fourth = if self.fourth.trim().is_empty() {
            None
        } else {
            Some(self.fourth.trim())
        };
        let offset = if self.offset.trim().is_empty() {
            None
        } else {
            match self.offset.trim().parse::<usize>() {
                Ok(o) => Some(o),
                Err(_) => {
                    self.error = Some(format!("offset must be a number, got {:?}", self.offset));
                    return;
                }
            }
        };
        let top: usize = match self.top.trim().parse() {
            Ok(t) => t,
            Err(_) => {
                self.error = Some(format!("winners must be a number, got {:?}", self.top));
                return;
            }
        };
        let lang = match Lang::parse(&self.lang) {
            Ok(l) => l,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let cfg = match build_crib_config(
            &pool,
            fourth,
            &self.rings,
            &self.reflector,
            &self.plugs,
            &self.etw,
            &self.cipher,
            &self.crib,
            offset,
            top,
            None,
            &[],
            false,
            50,
            10,
        ) {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(e.to_string());
                return;
            }
        };
        self.last_cfg = Some(cfg.clone());
        self.results.clear();
        self.selected = 0;
        self.preview.clear();
        self.finished = false;
        let checkpoint: PathBuf =
            std::env::temp_dir().join(format!("enigma-gui-crib-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&checkpoint);
        self.checkpoint = Some(checkpoint.clone());
        let (tx, rx) = mpsc::channel::<CribMsg>();
        self.running = Some(rx);
        self.done = 0;
        self.total = 1;
        std::thread::spawn(move || {
            let scorer = QuadgramScorer::new(lang);
            let cb = |done: usize, total: usize| {
                let _ = tx.send(CribMsg::Tick(done, total));
            };
            match solve_crib(&cfg, &scorer, Some(&checkpoint), Some(&cb)) {
                Ok(_) => {
                    let _ = tx.send(CribMsg::Done);
                }
                Err(e) => {
                    let _ = tx.send(CribMsg::Failed(e.to_string()));
                }
            }
        });
    }

    fn poll(&mut self, ui: &egui::Ui) {
        let mut done_flag = false;
        if let Some(rx) = &self.running {
            for msg in rx.try_iter() {
                match msg {
                    CribMsg::Tick(done, total) => {
                        self.done = done;
                        self.total = total.max(1);
                    }
                    CribMsg::Done => {
                        done_flag = true;
                        self.finished = true;
                    }
                    CribMsg::Failed(e) => {
                        done_flag = true;
                        self.finished = true;
                        self.error = Some(e);
                    }
                }
            }
            // Live best list from the worker's checkpoint file.
            if let Some(path) = &self.checkpoint.clone() {
                if let Some(ckpt) = load_checkpoint(path) {
                    self.results = ckpt.best;
                }
            }
            ui.ctx().request_repaint();
        }
        if done_flag {
            self.running = None;
            self.update_preview();
        }
    }
    /// Fill machine fields from a config section (pool/fourth split).
    fn apply_machine_section(&mut self, section: &MachineSection) {
        if let Some(rotors) = &section.rotors {
            let (fourth, pool) = crate::dialogs::split_rotors(rotors);
            self.pool = pool;
            self.fourth = fourth;
        }
        if let Some(v) = &section.rings {
            self.rings = v.clone();
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
    }

    fn update_preview(&mut self) {
        self.preview.clear();
        let (Some(cfg), Some(cand)) = (self.last_cfg.as_ref(), self.results.get(self.selected))
        else {
            return;
        };
        // Rebuild the winning machine and decrypt for display.
        let names: Vec<&str> = cand.order.iter().map(String::as_str).collect();
        let rings: String = cfg.rings.iter().map(|&p| pos_to_char(p)).collect();
        let positions_str: String = cand.positions.iter().map(|&p| pos_to_char(p)).collect();
        let plugs: String = cfg
            .plugs
            .iter()
            .map(|&(a, b)| format!("{}{}", pos_to_char(a), pos_to_char(b)))
            .collect::<Vec<_>>()
            .join(" ");
        let etw_name = match cfg.etw {
            enigma_core::config::EtwKind::Identity => "identity",
            enigma_core::config::EtwKind::Qwertz => "qwertz",
            enigma_core::config::EtwKind::Custom(_) => "identity",
        };
        let reflector_name = format!("{:?}", cfg.reflector);
        // Custom ETW wirings can't round-trip through names; fall back to identity
        // (historic searches always use named wheels anyway).
        if let Ok(mcfg) = enigma_core::config::MachineConfig::from_strings(
            &names,
            &rings,
            &positions_str,
            &reflector_name,
            &plugs,
            etw_name,
        ) {
            if let Ok(mut m) = mcfg.build_machine() {
                // Full plaintext is kept for "save winner"; display truncates.
                self.preview = cfg
                    .cipher
                    .iter()
                    .map(|&c| pos_to_char(m.encipher_char(c)))
                    .collect();
            }
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.poll(ui);
        egui::Panel::left("crib_form").show(ui, |ui| {
            ui.heading("Step 1 — Describe the prey");
            if ui
                .button("Fill demo 🦕")
                .on_hover_text("Loads a German message + crib that always cracks")
                .clicked()
            {
                self.cipher = DEMO_CIPHER.trim().into();
                self.crib = DEMO_CRIB.into();
                self.pool = "I II III".into();
                self.lang = "de".into();
                self.error = None;
            }
            ui.horizontal(|ui| {
                ui.label("Rotor pool");
                ui.text_edit_singleline(&mut self.pool)
                    .on_hover_text("Wheels to try — every order gets tested, taken 3 at a time");
            });
            egui::ComboBox::from_label("M4 fourth")
                .selected_text(if self.fourth.is_empty() {
                    "—"
                } else {
                    &self.fourth
                })
                .show_ui(ui, |ui| {
                    for f in FOURTHS {
                        let label = if f.is_empty() { "—" } else { f };
                        if ui
                            .selectable_value(&mut self.fourth, f.to_string(), label)
                            .clicked()
                        {}
                    }
                });
            ui.horizontal(|ui| {
                ui.label("Rings");
                ui.text_edit_singleline(&mut self.rings);
            });
            egui::ComboBox::from_label("Reflector")
                .selected_text(&self.reflector)
                .show_ui(ui, |ui| {
                    for r in REFLECTORS {
                        ui.selectable_value(&mut self.reflector, r.to_string(), r);
                    }
                });
            ui.horizontal(|ui| {
                ui.label("Known plugs");
                ui.text_edit_singleline(&mut self.plugs);
            });
            egui::ComboBox::from_label("Entry wheel")
                .selected_text(&self.etw)
                .show_ui(ui, |ui| {
                    for e in ETWS {
                        ui.selectable_value(&mut self.etw, e.to_string(), e);
                    }
                });
            egui::ComboBox::from_label("Language")
                .selected_text(&self.lang)
                .show_ui(ui, |ui| {
                    for l in LANGS {
                        ui.selectable_value(&mut self.lang, l.to_string(), l);
                    }
                });
            ui.label("Ciphertext")
                .on_hover_text("The intercepted message — paste it here");
            ui.add(
                egui::TextEdit::multiline(&mut self.cipher)
                    .hint_text("Paste the gibberish here…")
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                ui.label("Crib");
                ui.text_edit_singleline(&mut self.crib)
                    .on_hover_text("A word you guess is hiding in the message");
            });
            ui.horizontal(|ui| {
                ui.label("Offset");
                ui.text_edit_singleline(&mut self.offset)
                    .on_hover_text("Empty scans every offset");
            });
            ui.horizontal(|ui| {
                ui.label("Winners");
                ui.text_edit_singleline(&mut self.top);
            });
            if ui.button("Start search").clicked() {
                self.start();
            }
            if ui.button("Load config…").clicked() {
                if let Some(path) = crate::dialogs::pick_toml() {
                    match enigma_config::AppConfig::load(&path) {
                        Ok(cfg) => self.apply_machine_section(&cfg.machine),
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            }
            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::RED, e);
            }
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let ratio = (self.done as f32 / self.total.max(1) as f32).clamp(0.0, 1.0);
            ui.add(egui::ProgressBar::new(ratio).show_percentage());
            ui.label(format!(
                "{}/{} rotor orders{}",
                self.done,
                self.total,
                if self.finished { " • done" } else { "" }
            ));
            ui.separator();
            let crib_len = self.last_cfg.as_ref().map(|c| c.crib.len()).unwrap_or(0);
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    for i in 0..self.results.len() {
                        let label = {
                            let cand = &self.results[i];
                            format!(
                                "#{} {} pos {} matches {}/{} score {:.1}",
                                i + 1,
                                cand.order.join(" "),
                                cand.positions
                                    .iter()
                                    .map(|&p| pos_to_char(p))
                                    .collect::<String>(),
                                cand.matches,
                                crib_len,
                                cand.score
                            )
                        };
                        if ui.selectable_value(&mut self.selected, i, label).clicked() {
                            self.update_preview();
                        }
                    }
                    if self.results.is_empty() {
                        ui.weak(if self.finished {
                            "No candidates — try a longer crib."
                        } else if self.running.is_some() {
                            "Searching…"
                        } else {
                            "Fill the form and press Start search."
                        });
                    }
                });
            if !self.preview.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new("Decrypt preview").strong());
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        let shown: String = self.preview.chars().take(600).collect();
                        let ellipsis = if self.preview.chars().count() > 600 {
                            "…"
                        } else {
                            ""
                        };
                        ui.monospace(format!("{shown}{ellipsis}"));
                    });
                if ui.button("Save winner…").clicked() {
                    if let Some(path) = crate::dialogs::save_txt("crib_result.txt") {
                        if let Err(e) = crate::dialogs::write_text(&path, &self.preview) {
                            self.error = Some(e);
                        }
                    }
                }
            }
        });
    }
}
