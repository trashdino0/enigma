//! In-TUI configuration form: every machine/solver setting edited as text.
//!
//! No CLI flags are required anymore — flags only prefill these fields.
//! Each [`FormMode`] shows its relevant subset; `Start` validates everything
//! through the same builders the CLI uses, so the TUI can never launch an
//! inconsistent configuration.

use enigma_core::{
    config::{parse_letters, parse_rotor_name, MachineConfig},
    rotor::HistoricalRotor,
    EnigmaError,
};
use enigma_solver::{
    crib::{build_crib_config, CribConfig},
    hillclimb::BlindPoolConfig,
    score::Lang,
};

use crate::app::App;

/// A validated mode input, ready to hand to its screen.
#[derive(Debug)]
pub enum Ready {
    Type(App),
    Crib(CribConfig, Lang, String),
    Blind(BlindPoolConfig, Lang, String),
}

/// Which screen the form feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormMode {
    /// Interactive enciphering.
    Type,
    /// Known-plaintext search (pool always permuted).
    Crib,
    /// Ciphertext-only pool search.
    Blind,
}

impl FormMode {
    fn title(self) -> &'static str {
        match self {
            Self::Type => "Type — machine settings",
            Self::Crib => "Solve (crib) — search settings",
            Self::Blind => "Solve (blind) — search settings",
        }
    }
}

/// One editable text field.
#[derive(Debug, Clone)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub value: String,
}

/// Optional CLI prefills; anything missing falls back to safe defaults.
#[derive(Debug, Clone, Default)]
pub struct Prefill {
    pub rotors: Vec<String>,
    pub rings: Option<String>,
    pub positions: Option<String>,
    pub reflector: Option<String>,
    pub plugs: Option<String>,
    pub etw: Option<String>,
    pub lang: Option<String>,
}

impl Prefill {
    fn rotors_joined(&self, fallback: &str) -> String {
        if self.rotors.is_empty() {
            fallback.to_string()
        } else {
            self.rotors.join(" ")
        }
    }
}

/// Append-only text form with a field cursor.
#[derive(Debug)]
pub struct ConfigForm {
    pub mode: FormMode,
    pub fields: Vec<Field>,
    pub cursor: usize,
    pub editing: bool,
    pub error: Option<String>,
}

impl ConfigForm {
    /// Build the per-mode field list, prefilled from CLI flags or defaults.
    pub fn new(mode: FormMode, pre: &Prefill) -> Self {
        let mut f = Vec::new();
        let mut add =
            |key: &'static str, label: &'static str, hint: &'static str, value: String| {
                f.push(Field {
                    key,
                    label,
                    hint,
                    value,
                });
            };
        match mode {
            FormMode::Type => {
                add(
                    "rotors",
                    "Rotors",
                    "I II III, or Beta I II III",
                    pre.rotors_joined("I II III"),
                );
                add(
                    "rings",
                    "Rings",
                    "one letter per rotor",
                    pre.rings.clone().unwrap_or_else(|| "AAA".into()),
                );
                add(
                    "positions",
                    "Positions",
                    "start windows",
                    pre.positions.clone().unwrap_or_else(|| "AAA".into()),
                );
                add(
                    "reflector",
                    "Reflector",
                    "B C Thin-B Thin-C",
                    pre.reflector.clone().unwrap_or_else(|| "B".into()),
                );
                add(
                    "plugs",
                    "Plugs",
                    "AV BS CG, empty = none",
                    pre.plugs.clone().unwrap_or_default(),
                );
                add(
                    "etw",
                    "Entry wheel",
                    "identity / qwertz",
                    pre.etw.clone().unwrap_or_else(|| "identity".into()),
                );
            }
            FormMode::Crib => {
                add(
                    "rotors",
                    "Rotor pool",
                    "permuted, taken 3",
                    pre.rotors_joined("I II III"),
                );
                add(
                    "fourth",
                    "M4 fourth",
                    "empty, or Beta / Gamma",
                    String::new(),
                );
                add(
                    "rings",
                    "Rings",
                    "3 letters, 4 with fourth",
                    pre.rings.clone().unwrap_or_else(|| "AAA".into()),
                );
                add(
                    "reflector",
                    "Reflector",
                    "B C Thin-B Thin-C",
                    pre.reflector.clone().unwrap_or_else(|| "B".into()),
                );
                add(
                    "plugs",
                    "Known plugs",
                    "assumed correct",
                    pre.plugs.clone().unwrap_or_default(),
                );
                add(
                    "etw",
                    "Entry wheel",
                    "identity / qwertz",
                    pre.etw.clone().unwrap_or_else(|| "identity".into()),
                );
                add(
                    "lang",
                    "Language",
                    "de / en",
                    pre.lang.clone().unwrap_or_else(|| "de".into()),
                );
                add(
                    "cipher",
                    "Ciphertext",
                    "the message to break",
                    String::new(),
                );
                add("crib", "Crib", "known fragment", String::new());
                add(
                    "crib_offset",
                    "Crib offset",
                    "empty = scan all",
                    String::new(),
                );
                add(
                    "ring_scan",
                    "Ring scan",
                    "empty, or slots 0,1",
                    String::new(),
                );
                add("top", "Winners", "how many to show", "5".into());
            }
            FormMode::Blind => {
                add(
                    "rotors",
                    "Rotor pool",
                    "permuted, taken 3",
                    pre.rotors_joined("I II III"),
                );
                add(
                    "fourth",
                    "M4 fourth",
                    "empty, or Beta / Gamma",
                    String::new(),
                );
                add(
                    "rings",
                    "Rings",
                    "3 letters, 4 with fourth",
                    pre.rings.clone().unwrap_or_else(|| "AAA".into()),
                );
                add(
                    "reflector",
                    "Reflector",
                    "B C Thin-B Thin-C",
                    pre.reflector.clone().unwrap_or_else(|| "B".into()),
                );
                add(
                    "etw",
                    "Entry wheel",
                    "identity / qwertz",
                    pre.etw.clone().unwrap_or_else(|| "identity".into()),
                );
                add(
                    "lang",
                    "Language",
                    "de / en",
                    pre.lang.clone().unwrap_or_else(|| "de".into()),
                );
                add(
                    "cipher",
                    "Ciphertext",
                    "150+ chars works best",
                    String::new(),
                );
                add(
                    "ring_scan",
                    "Ring scan",
                    "empty, or slots 0,1",
                    String::new(),
                );
                add("max_plugs", "Max plugs", "0 = positions only", "6".into());
                add(
                    "top_positions",
                    "Top positions",
                    "per order into climb",
                    "10".into(),
                );
                add("restarts", "Restarts", "random, per position", "3".into());
                add("seed", "Seed", "deterministic runs", "1".into());
                add("top", "Winners", "how many to show", "3".into());
            }
        }
        Self {
            mode,
            fields: f,
            cursor: 0,
            editing: false,
            error: None,
        }
    }

    pub fn title(&self) -> &'static str {
        self.mode.title()
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < self.fields.len() {
            self.cursor += 1;
        }
    }

    pub fn toggle_edit(&mut self) {
        self.editing = !self.editing;
    }

    pub fn push_char(&mut self, c: char) {
        if self.fields[self.cursor].key == "cipher" || self.fields[self.cursor].key == "crib" {
            // Cipher material: keep letters, drop whitespace noise.
            if c.is_ascii_alphabetic() {
                self.fields[self.cursor].value.push(c.to_ascii_uppercase());
            }
        } else if !c.is_control() {
            self.fields[self.cursor].value.push(c);
        }
    }

    pub fn backspace(&mut self) {
        self.fields[self.cursor].value.pop();
    }

    pub fn get(&self, key: &str) -> &str {
        self.fields
            .iter()
            .find(|f| f.key == key)
            .map(|f| f.value.as_str())
            .unwrap_or("")
    }

    fn number(&self, key: &str) -> Result<usize, String> {
        self.get(key).trim().parse::<usize>().map_err(|_| {
            format!(
                "field '{key}' must be a whole number, got {:?}",
                self.get(key)
            )
        })
    }

    fn ring_slots(&self) -> Result<Vec<usize>, String> {
        let raw = self.get("ring_scan").trim();
        if raw.is_empty() {
            return Ok(Vec::new());
        }
        raw.split(',')
            .map(|part| {
                part.trim()
                    .parse::<usize>()
                    .map_err(|_| format!("ring scan slot {part:?} is not a number (try \"0,1\")"))
            })
            .collect()
    }

    fn pool(&self) -> Result<Vec<String>, EnigmaError> {
        let pool: Vec<String> = self
            .get("rotors")
            .split_whitespace()
            .map(str::to_string)
            .collect();
        for name in &pool {
            parse_rotor_name(name)?;
        }
        Ok(pool)
    }

    fn fourth(&self) -> Result<Option<HistoricalRotor>, EnigmaError> {
        let raw = self.get("fourth").trim();
        if raw.is_empty() {
            Ok(None)
        } else {
            parse_rotor_name(raw).map(Some)
        }
    }

    fn lang(&self) -> Result<Lang, String> {
        Lang::parse(self.get("lang"))
    }

    /// Type mode: full machine config plus echoable rotor names.
    pub fn build_type(&self) -> Result<(MachineConfig, Vec<String>), String> {
        let names: Vec<&str> = self.get("rotors").split_whitespace().collect();
        MachineConfig::from_strings(
            &names,
            self.get("rings"),
            self.get("positions"),
            self.get("reflector"),
            self.get("plugs"),
            self.get("etw"),
        )
        .map(|cfg| {
            let echo: Vec<String> = names.iter().map(|s| s.to_ascii_uppercase()).collect();
            (cfg, echo)
        })
        .map_err(|e| e.to_string())
    }

    /// Crib mode: solver config plus language, crib length, and a subtitle.
    pub fn build_crib(&self) -> Result<(CribConfig, Lang, String), String> {
        let pool = self.pool().map_err(|e| e.to_string())?;
        let fourth = self
            .fourth()
            .map_err(|e| e.to_string())?
            .map(|r| format!("{r:?}"));
        let lang = self.lang()?;
        let crib_offset = if self.get("crib_offset").trim().is_empty() {
            None
        } else {
            Some(self.number("crib_offset")?)
        };
        let top = self.number("top")?;
        let scan_rings = self.ring_slots()?;
        let cfg = build_crib_config(
            &pool,
            fourth.as_deref(),
            self.get("rings"),
            self.get("reflector"),
            self.get("plugs"),
            self.get("etw"),
            self.get("cipher"),
            self.get("crib"),
            crib_offset,
            top,
            None,
            &scan_rings,
            false,
            50,
            10,
        )
        .map_err(|e| e.to_string())?;
        let subtitle = format!(
            "pool {} • crib {} chars • lang {}",
            pool.join(" "),
            cfg.crib.len(),
            self.get("lang").trim(),
        );
        Ok((cfg, lang, subtitle))
    }

    /// Blind mode: pool config plus language and a subtitle.
    pub fn build_blind(&self) -> Result<(BlindPoolConfig, Lang, String), String> {
        let pool_names = self.pool().map_err(|e| e.to_string())?;
        let mut pool = Vec::with_capacity(pool_names.len());
        for name in &pool_names {
            pool.push(parse_rotor_name(name).map_err(|e| e.to_string())?);
        }
        let fourth = self.fourth().map_err(|e| e.to_string())?;
        let rings = parse_letters(self.get("rings")).map_err(|e| e.to_string())?;
        let lang = self.lang()?;
        let cipher = enigma_solver::crib::encode_text(self.get("cipher"));
        let cfg = BlindPoolConfig {
            cipher,
            rotor_pool: pool.clone(),
            fourth,
            rings,
            reflector: self
                .get("reflector")
                .parse()
                .map_err(|e: EnigmaError| e.to_string())?,
            etw: self
                .get("etw")
                .parse()
                .map_err(|e: EnigmaError| e.to_string())?,
            max_plugs: self.number("max_plugs")?,
            top_positions: self.number("top_positions")?,
            restarts: self.number("restarts")?,
            seed: self.get("seed").trim().parse::<u64>().map_err(|_| {
                format!(
                    "field 'seed' must be a whole number, got {:?}",
                    self.get("seed")
                )
            })?,
            scan_rings: self.ring_slots()?,
            per_order_top: 1,
            top_n: self.number("top")?,
            checkpoint: None,
        };
        let subtitle = format!(
            "pool {} • {} cipher chars • lang {}",
            pool_names.join(" "),
            cfg.cipher.len(),
            self.get("lang").trim(),
        );
        Ok((cfg, lang, subtitle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefill() -> Prefill {
        Prefill::default()
    }

    #[test]
    fn type_defaults_build() {
        let form = ConfigForm::new(FormMode::Type, &prefill());
        let (cfg, echo) = form.build_type().expect("defaults valid");
        assert_eq!(echo, vec!["I", "II", "III"]);
        let mut m = cfg.build_machine().expect("builds");
        assert_eq!(m.encipher_str("AAAAA"), "BDZGO");
    }

    #[test]
    fn type_rejects_bad_rotors() {
        let mut form = ConfigForm::new(FormMode::Type, &prefill());
        form.fields
            .iter_mut()
            .find(|f| f.key == "rotors")
            .unwrap()
            .value = "I I II".into();
        assert!(form.build_type().is_err());
    }

    #[test]
    fn crib_defaults_parse_pool_and_lang() {
        let mut form = ConfigForm::new(FormMode::Crib, &prefill());
        for (key, val) in [("cipher", "ABCDEF"), ("crib", "ABC")] {
            form.fields.iter_mut().find(|f| f.key == key).unwrap().value = val.into();
        }
        let (cfg, lang, subtitle) = form.build_crib().expect("parses");
        assert_eq!(lang, Lang::De);
        assert_eq!(cfg.rotor_pool.len(), 3);
        assert!(subtitle.contains("crib 3 chars"));
    }

    #[test]
    fn blind_rejects_bad_numbers() {
        let mut form = ConfigForm::new(FormMode::Blind, &prefill());
        form.fields
            .iter_mut()
            .find(|f| f.key == "max_plugs")
            .unwrap()
            .value = "many".into();
        assert!(form.build_blind().is_err());
    }

    #[test]
    fn form_navigation_and_editing() {
        let mut form = ConfigForm::new(FormMode::Type, &prefill());
        assert_eq!(form.cursor, 0);
        form.move_down();
        assert_eq!(form.cursor, 1);
        form.move_up();
        assert_eq!(form.cursor, 0);
        form.move_up();
        assert_eq!(form.cursor, 0, "clamped at top");
        form.toggle_edit();
        form.push_char('X');
        assert!(form.fields[0].value.ends_with('X'));
        form.backspace();
        assert!(!form.fields[0].value.ends_with('X'));
    }
}
