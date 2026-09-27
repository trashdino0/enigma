//! `enigma-gui`: desktop Enigma workbench (machine + crib + blind tabs).

mod blind_tab;
mod crib_tab;
mod dialogs;
mod machine_tab;

use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Machine,
    Crib,
    Blind,
}

#[derive(Default)]
struct GuiApp {
    tab: Tab,
    machine: machine_tab::MachineTab,
    crib: crib_tab::CribTab,
    blind: blind_tab::BlindTab,
}

impl eframe::App for GuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("🦕 EnigmaSaurus")
                    .size(22.0)
                    .strong()
                    .color(AMBER),
            );
            ui.weak("the Enigma workbench");
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Machine, "🔤 Machine");
            ui.selectable_value(&mut self.tab, Tab::Crib, "🔍 Crib hunt");
            ui.selectable_value(&mut self.tab, Tab::Blind, "🌿 Blind hunt");
        });
        ui.separator();
        match self.tab {
            Tab::Machine => self.machine.show(ui),
            Tab::Crib => self.crib.show(ui),
            Tab::Blind => self.blind.show(ui),
        }
    }
}

/// Jungle night palette: deep greens, fern selection, amber highlights.
pub(crate) fn dino_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.window_fill = egui::Color32::from_rgb(16, 26, 18);
    v.panel_fill = egui::Color32::from_rgb(20, 32, 22);
    v.faint_bg_color = egui::Color32::from_rgb(28, 44, 30);
    v.extreme_bg_color = egui::Color32::from_rgb(12, 20, 14);
    v.selection.bg_fill = egui::Color32::from_rgb(62, 110, 66);
    v.hyperlink_color = AMBER;
    v.warn_fg_color = AMBER;
    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(40, 62, 42);
    v.widgets.active.bg_fill = egui::Color32::from_rgb(62, 110, 66);
    v
}

pub(crate) const AMBER: egui::Color32 = egui::Color32::from_rgb(232, 184, 90);

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "🦕 EnigmaSaurus — M3/M4 Workbench",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(dino_visuals());
            Ok(Box::new(GuiApp::default()))
        }),
    )
}

/// One-line `STAGE:letter` signal chain for the last keypress.
/// `R1` is the leftmost rotor, `R{n}` the fast rightmost.
pub(crate) fn trace_line(t: &enigma_core::machine::SignalTrace) -> String {
    use enigma_core::pos_to_char;
    let c = pos_to_char;
    let n = t.rotor_count;
    let mut parts = Vec::with_capacity(2 * n + 7);
    parts.push(format!("IN:{}", c(t.input)));
    parts.push(format!("STB:{}", c(t.plug_in)));
    parts.push(format!("ETW:{}", c(t.etw_in)));
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
