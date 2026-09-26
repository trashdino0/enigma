//! Blind solver tab: pool form, stage progress, winners with plaintext.

use std::sync::mpsc;

use eframe::egui;
use enigma_core::pos_to_char;
use enigma_solver::{
    hillclimb::{solve_blind_pool, BlindCandidate, BlindPoolConfig, BlindProgress},
    score::{Lang, QuadgramScorer},
};

const REFLECTORS: [&str; 4] = ["B", "C", "Thin-B", "Thin-C"];
const ETWS: [&str; 2] = ["identity", "qwertz"];
const LANGS: [&str; 2] = ["de", "en"];
const FOURTHS: [&str; 3] = ["", "Beta", "Gamma"];

enum BlindMsg {
    Progress(BlindProgress),
    Done(Vec<BlindCandidate>),
    Failed(String),
}

pub struct BlindTab {
    pool: String,
    fourth: String,
    rings: String,
    reflector: String,
    etw: String,
    lang: String,
    cipher: String,
    max_plugs: String,
    top_positions: String,
    restarts: String,
    seed: String,
    top: String,
    error: Option<String>,
    running: Option<mpsc::Receiver<BlindMsg>>,
    scan: (usize, usize),
    climb: (usize, usize),
    orders: (usize, usize),
    finished: bool,
    winners: Vec<BlindCandidate>,
    selected: usize,
}

impl Default for BlindTab {
    fn default() -> Self {
        Self {
            pool: "I II III".into(),
            fourth: String::new(),
            rings: "AAA".into(),
            reflector: "B".into(),
            etw: "identity".into(),
            lang: "de".into(),
            cipher: String::new(),
            max_plugs: "6".into(),
            top_positions: "10".into(),
            restarts: "3".into(),
            seed: "1".into(),
            top: "3".into(),
            error: None,
            running: None,
            scan: (0, 1),
            climb: (0, 1),
            orders: (0, 1),
            finished: false,
            winners: Vec::new(),
            selected: 0,
        }
    }
}

impl BlindTab {
    fn number(&self, raw: &str, field: &str) -> Result<usize, String> {
        raw.trim()
            .parse::<usize>()
            .map_err(|_| format!("{field} must be a whole number, got {raw:?}"))
    }

    fn start(&mut self) {
        self.error = None;
        let pool: Vec<String> = self.pool.split_whitespace().map(str::to_string).collect();
        let lang = match Lang::parse(&self.lang) {
            Ok(l) => l,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let parsed = (|| -> Result<BlindPoolConfig, String> {
            use enigma_core::config::{parse_letters, parse_rotor_name};
            let mut rotor_pool = Vec::with_capacity(pool.len());
            for name in &pool {
                rotor_pool.push(parse_rotor_name(name).map_err(|e| e.to_string())?);
            }
            let fourth = if self.fourth.trim().is_empty() {
                None
            } else {
                Some(parse_rotor_name(self.fourth.trim()).map_err(|e| e.to_string())?)
            };
            Ok(BlindPoolConfig {
                cipher: enigma_solver::crib::encode_text(&self.cipher),
                rotor_pool,
                fourth,
                rings: parse_letters(&self.rings).map_err(|e| e.to_string())?,
                reflector: self
                    .reflector
                    .parse()
                    .map_err(|e: enigma_core::EnigmaError| e.to_string())?,
                etw: self
                    .etw
                    .parse()
                    .map_err(|e: enigma_core::EnigmaError| e.to_string())?,
                max_plugs: self.number(&self.max_plugs, "max plugs")?,
                top_positions: self.number(&self.top_positions, "top positions")?,
                restarts: self.number(&self.restarts, "restarts")?,
                seed: self
                    .seed
                    .trim()
                    .parse::<u64>()
                    .map_err(|_| format!("seed must be a whole number, got {:?}", self.seed))?,
                per_order_top: 1,
                top_n: self.number(&self.top, "winners")?,
            })
        })();
        let cfg = match parsed {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        self.winners.clear();
        self.selected = 0;
        self.finished = false;
        self.scan = (0, 1);
        self.climb = (0, 1);
        self.orders = (0, 1);
        let (tx, rx) = mpsc::channel::<BlindMsg>();
        self.running = Some(rx);
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
    }

    fn poll(&mut self, ui: &egui::Ui) {
        let mut done_flag = false;
        if let Some(rx) = &self.running {
            for msg in rx.try_iter() {
                match msg {
                    BlindMsg::Progress(p) => match p {
                        BlindProgress::Scan { done, total } => {
                            self.scan = (done, total.max(1));
                        }
                        BlindProgress::Climb { done, total } => {
                            self.climb = (done, total.max(1));
                        }
                        BlindProgress::Order { done, total } => {
                            self.orders = (done, total.max(1));
                        }
                    },
                    BlindMsg::Done(winners) => {
                        done_flag = true;
                        self.finished = true;
                        self.winners = winners;
                    }
                    BlindMsg::Failed(e) => {
                        done_flag = true;
                        self.finished = true;
                        self.error = Some(e);
                    }
                }
            }
            ui.ctx().request_repaint();
        }
        if done_flag {
            self.running = None;
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.poll(ui);
        egui::Panel::left("blind_form").show(ui, |ui| {
            ui.heading("Blind search");
            ui.horizontal(|ui| {
                ui.label("Rotor pool");
                ui.text_edit_singleline(&mut self.pool)
                    .on_hover_text("Permuted, taken 3 per order");
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
                        let _ = ui.selectable_value(&mut self.fourth, f.to_string(), label);
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
            ui.label("Ciphertext");
            ui.add(
                egui::TextEdit::multiline(&mut self.cipher)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
            for (label, field) in [
                ("Max plugs", &mut self.max_plugs),
                ("Top positions", &mut self.top_positions),
                ("Restarts", &mut self.restarts),
                ("Seed", &mut self.seed),
                ("Winners", &mut self.top),
            ] {
                ui.horizontal(|ui| {
                    ui.label(label);
                    ui.text_edit_singleline(field);
                });
            }
            if ui.button("Start search").clicked() {
                self.start();
            }
            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::RED, e);
            }
        });

        egui::CentralPanel::default().show(ui, |ui| {
            for (label, (done, total)) in [
                ("scan", self.scan),
                ("climb", self.climb),
                ("orders", self.orders),
            ] {
                let ratio = (done as f32 / total.max(1) as f32).clamp(0.0, 1.0);
                ui.horizontal(|ui| {
                    ui.label(format!("{label:<7}"));
                    ui.add(egui::ProgressBar::new(ratio).show_percentage());
                    ui.label(format!("{done}/{total}"));
                });
            }
            if self.finished {
                ui.label("done");
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    for (i, cand) in self.winners.iter().enumerate() {
                        let label = format!(
                            "#{} {} pos {} plugs {} score {:.1}",
                            i + 1,
                            cand.order.join(" "),
                            cand.positions
                                .iter()
                                .map(|&p| pos_to_char(p))
                                .collect::<String>(),
                            if cand.plugs.is_empty() {
                                "-".to_string()
                            } else {
                                cand.plugs
                                    .iter()
                                    .map(|&(a, b)| format!("{}{}", pos_to_char(a), pos_to_char(b)))
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            },
                            cand.score
                        );
                        ui.selectable_value(&mut self.selected, i, label);
                    }
                    if self.winners.is_empty() {
                        ui.weak(if self.finished {
                            "No winners."
                        } else if self.running.is_some() {
                            "Searching…"
                        } else {
                            "Fill the form and press Start search."
                        });
                    }
                });
            if let Some(cand) = self.winners.get(self.selected) {
                ui.separator();
                ui.label(egui::RichText::new("Decrypt preview").strong());
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        ui.monospace(cand.plaintext.chars().take(600).collect::<String>());
                    });
            }
        });
    }
}
