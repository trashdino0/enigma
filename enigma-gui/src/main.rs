//! `enigma-gui`: desktop Enigma workbench (machine + crib + blind tabs).

mod blind_tab;
mod crib_tab;
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
        egui::Panel::top("tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Machine, "Machine");
                ui.selectable_value(&mut self.tab, Tab::Crib, "Crib solver");
                ui.selectable_value(&mut self.tab, Tab::Blind, "Blind solver");
            });
        });
        match self.tab {
            Tab::Machine => self.machine.show(ui),
            Tab::Crib => self.crib.show(ui),
            Tab::Blind => self.blind.show(ui),
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Enigma M3/M4",
        options,
        Box::new(|_cc| Ok(Box::new(GuiApp::default()))),
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
