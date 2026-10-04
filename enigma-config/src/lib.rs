//! TOML configuration files for Enigma setups.
//!
//! Machine settings get complicated fast (rotors, rings, positions,
//! reflector, plugs, entry wheel — times every daily key), so they live in
//! versionable files instead of long command lines:
//!
//! ```toml
//! [machine]                       # base setup (also the fallback)
//! rotors = ["I", "II", "III"]
//! rings = "AAA"
//! positions = "AAA"
//! reflector = "B"
//! plugs = "AV BS CG"
//! etw = "identity"
//!
//! [solver]                        # defaults for solve commands
//! lang = "de"
//! top = 5
//!
//! [profiles.naval]                # named overlay; inherits unset fields
//! rotors = ["Beta", "I", "II", "III"]
//! rings = "AAAA"
//! positions = "AAAA"
//! reflector = "Thin-B"
//! ```
//!
//! Precedence everywhere: CLI flags > `--profile` overlay > `[machine]`.
//! Anything still missing after merging is an error naming the field.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use enigma_core::{config::MachineConfig, EnigmaError};
use serde::{Deserialize, Serialize};

/// One machine setup; every field optional so profiles inherit from base.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MachineSection {
    /// Rotor names left -> right, e.g. `["I", "II", "III"]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotors: Option<Vec<String>>,
    /// Ring letters, one per rotor, e.g. `"AAA"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rings: Option<String>,
    /// Start window letters, e.g. `"KDO"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub positions: Option<String>,
    /// `B`, `C`, `Thin-B`, `Thin-C`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reflector: Option<String>,
    /// Plug pairs, e.g. `"AV BS CG"` (empty = unpatched).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugs: Option<String>,
    /// `identity` (Wehrmacht/Naval) or `qwertz` (D/K/Railway).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etw: Option<String>,
}

impl MachineSection {
    /// Overlay `over` on top of `self`: set fields win, `None` inherits.
    pub fn merged(&self, over: &MachineSection) -> MachineSection {
        let pick = |a: &Option<String>, b: &Option<String>| b.clone().or_else(|| a.clone());
        MachineSection {
            rotors: over.rotors.clone().or_else(|| self.rotors.clone()),
            rings: pick(&self.rings, &over.rings),
            positions: pick(&self.positions, &over.positions),
            reflector: pick(&self.reflector, &over.reflector),
            plugs: pick(&self.plugs, &over.plugs),
            etw: pick(&self.etw, &over.etw),
        }
    }

    /// Require the four structural fields (plugs/etw default when absent).
    fn require(&self) -> Result<ResolvedMachine, ConfigError> {
        let need = |v: &Option<String>, field: &'static str| {
            v.clone().ok_or(ConfigError::MissingField(field))
        };
        Ok(ResolvedMachine {
            rotors: self
                .rotors
                .clone()
                .ok_or(ConfigError::MissingField("rotors"))?,
            rings: need(&self.rings, "rings")?,
            positions: need(&self.positions, "positions")?,
            reflector: need(&self.reflector, "reflector")?,
            plugs: self.plugs.clone().unwrap_or_default(),
            etw: self.etw.clone().unwrap_or_else(|| "identity".into()),
        })
    }
}

/// Solver defaults for solve commands; everything optional.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SolverSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_plugs: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_positions: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restarts: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_matches: Option<usize>,
}

impl SolverSection {
    /// Overlay `over` on top of `self`: set fields win, `None` inherits.
    pub fn merged(&self, over: &SolverSection) -> SolverSection {
        let pick_u = |a: Option<usize>, b: Option<usize>| b.or(a);
        SolverSection {
            lang: over.lang.clone().or_else(|| self.lang.clone()),
            top: pick_u(self.top, over.top),
            max_plugs: pick_u(self.max_plugs, over.max_plugs),
            top_positions: pick_u(self.top_positions, over.top_positions),
            restarts: pick_u(self.restarts, over.restarts),
            seed: over.seed.or(self.seed),
            min_matches: pick_u(self.min_matches, over.min_matches),
        }
    }
}

/// A user-defined rotor (Railway/commercial variants, experiments).
///
/// ```toml
/// [custom_rotors.coastal]
/// wiring = "QWERTYUIOPASDFGHJKLZXCVBNM"
/// notches = ["A"]
/// steps = true
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomRotorSpec {
    /// 26-letter permutation wiring.
    pub wiring: String,
    /// Turnover window letters, e.g. `["Q"]` or `["Z", "M"]`.
    #[serde(default)]
    pub notches: Vec<String>,
    /// Whether the pawl can step it (false = Beta/Gamma-style fixed 4th).
    #[serde(default = "default_true")]
    pub steps: bool,
}

fn default_true() -> bool {
    true
}

/// Whole configuration file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// Base machine setup (fallback for every profile).
    #[serde(default)]
    pub machine: MachineSection,
    /// Named overlays inheriting unset fields from `[machine]`.
    #[serde(default)]
    pub profiles: HashMap<String, MachineSection>,
    /// Solver flag defaults.
    #[serde(default)]
    pub solver: SolverSection,
    /// Named solver overlays (`--solver-preset`), merged over `[solver]`.
    #[serde(default)]
    pub solver_presets: HashMap<String, SolverSection>,
    /// User-defined rotors usable anywhere a rotor name goes.
    #[serde(default)]
    pub custom_rotors: HashMap<String, CustomRotorSpec>,
}

impl AppConfig {
    /// Parse a TOML file.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(ConfigError::Io)?;
        toml::from_str(&text).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Serialize back to TOML (GUI "save config").
    pub fn to_toml_string(&self) -> Result<String, ConfigError> {
        toml::to_string_pretty(self).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Resolve `[machine]` + optional `--profile` overlay into a concrete setup.
    pub fn machine_for(&self, profile: Option<&str>) -> Result<ResolvedMachine, ConfigError> {
        let merged = match profile {
            Some(name) => {
                let overlay = self.profiles.get(name).ok_or_else(|| {
                    let mut known: Vec<String> =
                        self.profiles.keys().map(|k| k.to_string()).collect();
                    known.sort();
                    ConfigError::UnknownProfile {
                        name: name.to_string(),
                        known,
                    }
                })?;
                self.machine.merged(overlay)
            }
            None => self.machine.clone(),
        };
        merged.require()
    }

    /// Resolve `[solver]` + optional `--solver-preset` overlay.
    pub fn solver_for(&self, preset: Option<&str>) -> Result<SolverSection, ConfigError> {
        match preset {
            Some(name) => {
                let overlay = self.solver_presets.get(name).ok_or_else(|| {
                    let mut known: Vec<String> =
                        self.solver_presets.keys().map(|k| k.to_string()).collect();
                    known.sort();
                    ConfigError::UnknownProfile {
                        name: name.to_string(),
                        known,
                    }
                })?;
                Ok(self.solver.merged(overlay))
            }
            None => Ok(self.solver.clone()),
        }
    }

    /// Build rotors for an order, resolving historic names first and
    /// `[custom_rotors]` second. `rings`/`positions` align left -> right.
    pub fn build_rotors(
        &self,
        names: &[String],
        rings: &[u8],
        positions: &[u8],
    ) -> Result<Vec<enigma_core::rotor::Rotor>, ConfigError> {
        use enigma_core::rotor::Rotor;
        if names.len() != rings.len() || names.len() != positions.len() {
            return Err(ConfigError::Parse(format!(
                "rotors/rings/positions length mismatch ({} vs {} vs {})",
                names.len(),
                rings.len(),
                positions.len()
            )));
        }
        // Distinctness (historic equality can't see customs — compare names).
        {
            let mut seen = std::collections::HashSet::new();
            for name in names {
                if !seen.insert(name.to_ascii_uppercase()) {
                    return Err(ConfigError::Parse(format!(
                        "duplicate rotor {name:?} in order"
                    )));
                }
            }
        }
        let mut out = Vec::with_capacity(names.len());
        for ((name, &ring), &pos) in names.iter().zip(rings).zip(positions) {
            // Historic names always win; customs fill the gaps.
            if let Ok(kind) = enigma_core::config::parse_rotor_name(name) {
                out.push(Rotor::historical(kind, ring, pos)?);
            } else if let Some(spec) = lookup_custom(&self.custom_rotors, name) {
                let mut notches = Vec::with_capacity(spec.notches.len());
                for letter in &spec.notches {
                    let upper = letter.to_ascii_uppercase();
                    let bytes = upper.as_bytes();
                    if bytes.len() != 1 || !bytes[0].is_ascii_uppercase() {
                        return Err(ConfigError::Parse(format!(
                            "custom rotor {name:?}: bad notch {letter:?}"
                        )));
                    }
                    notches.push(bytes[0] - b'A');
                }
                out.push(Rotor::from_wiring_str(
                    &spec.wiring,
                    &notches,
                    ring,
                    pos,
                    spec.steps,
                )?);
            } else {
                return Err(ConfigError::Parse(format!("unknown rotor {name:?}")));
            }
        }
        Ok(out)
    }

    /// Full machine from a resolved setup, supporting custom rotors.
    /// Historic-only setups behave exactly like `MachineConfig`.
    pub fn build_machine(
        &self,
        resolved: &ResolvedMachine,
    ) -> Result<enigma_core::machine::EnigmaMachine, ConfigError> {
        use enigma_core::config::{EtwKind, ReflectorKind};
        use enigma_core::{machine::EnigmaMachine, plugboard::Plugboard};
        let rings = parse_letter_string(&resolved.rings, "rings")?;
        let positions = parse_letter_string(&resolved.positions, "positions")?;
        let rotors = self.build_rotors(&resolved.rotors, &rings, &positions)?;
        let reflector = match ReflectorKind::parse(&resolved.reflector) {
            Ok(ReflectorKind::A) => enigma_core::reflector::Reflector::a(),
            Ok(ReflectorKind::B) => enigma_core::reflector::Reflector::b(),
            Ok(ReflectorKind::C) => enigma_core::reflector::Reflector::c(),
            Ok(ReflectorKind::ThinB) => enigma_core::reflector::Reflector::thin_b(),
            Ok(ReflectorKind::ThinC) => enigma_core::reflector::Reflector::thin_c(),
            Err(e) => return Err(ConfigError::Parse(e.to_string())),
        }?;
        let etw = match EtwKind::parse(&resolved.etw) {
            Ok(EtwKind::Identity) => enigma_core::etw::EntryWheel::identity(),
            Ok(EtwKind::Qwertz) => enigma_core::etw::EntryWheel::qwertz(),
            Ok(EtwKind::Custom(w)) => enigma_core::etw::EntryWheel::custom(&w)?,
            Err(e) => return Err(ConfigError::Parse(e.to_string())),
        };
        let plugboard = Plugboard::from_wiring(&resolved.plugs)?;
        Ok(EnigmaMachine::new(etw, rotors, reflector, plugboard)?)
    }
}

/// Case-insensitive custom rotor lookup.
fn lookup_custom<'a>(
    customs: &'a HashMap<String, CustomRotorSpec>,
    name: &str,
) -> Option<&'a CustomRotorSpec> {
    customs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
}

/// Letter string like `"AAA"` into `0..26` positions.
fn parse_letter_string(s: &str, field: &str) -> Result<Vec<u8>, ConfigError> {
    let upper = s.trim().to_ascii_uppercase();
    let mut out = Vec::with_capacity(upper.len());
    for (i, b) in upper.bytes().enumerate() {
        if !b.is_ascii_uppercase() {
            return Err(ConfigError::Parse(format!("{field} index {i} is not A-Z")));
        }
        out.push(b - b'A');
    }
    Ok(out)
}

/// Fully concrete machine setup: every field present.
#[derive(Debug, Clone)]
pub struct ResolvedMachine {
    pub rotors: Vec<String>,
    pub rings: String,
    pub positions: String,
    pub reflector: String,
    pub plugs: String,
    pub etw: String,
}

impl ResolvedMachine {
    /// Validate and build a ready machine (same checks as the CLI path).
    pub fn to_machine_config(&self) -> Result<MachineConfig, EnigmaError> {
        let names: Vec<&str> = self.rotors.iter().map(String::as_str).collect();
        MachineConfig::from_strings(
            &names,
            &self.rings,
            &self.positions,
            &self.reflector,
            &self.plugs,
            &self.etw,
        )
    }
}

/// Configuration failure modes.
#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(String),
    UnknownProfile { name: String, known: Vec<String> },
    MissingField(&'static str),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Parse(e) => write!(f, "config parse error: {e}"),
            Self::UnknownProfile { name, known } => {
                if known.is_empty() {
                    write!(
                        f,
                        "config has no profiles, but --profile {name:?} was given"
                    )
                } else {
                    write!(f, "unknown profile {name:?} (known: {})", known.join(", "))
                }
            }
            Self::MissingField(field) => write!(
                f,
                "config is missing {field:?} (add it to [machine], the profile, or pass the flag)"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<EnigmaError> for ConfigError {
    fn from(e: EnigmaError) -> Self {
        Self::Parse(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
[machine]
rotors = ["I", "II", "III"]
rings = "AAA"
positions = "AAA"
reflector = "B"
plugs = ""
etw = "identity"

[solver]
lang = "de"
top = 5

[profiles.naval]
rotors = ["Beta", "I", "II", "III"]
rings = "AAAA"
positions = "AAAA"
reflector = "Thin-B"
"#;

    fn example() -> AppConfig {
        toml::from_str(EXAMPLE).expect("example parses")
    }

    #[test]
    fn base_resolves_and_enciphers() {
        let cfg = example().machine_for(None).expect("base resolves");
        assert_eq!(cfg.rotors, vec!["I", "II", "III"]);
        let mut m = cfg
            .to_machine_config()
            .expect("valid")
            .build_machine()
            .unwrap();
        assert_eq!(m.encipher_str("AAAAA"), "BDZGO");
    }

    #[test]
    fn profile_inherits_unset_fields() {
        let cfg = example()
            .machine_for(Some("naval"))
            .expect("naval resolves");
        assert_eq!(cfg.rotors, vec!["Beta", "I", "II", "III"]);
        assert_eq!(cfg.rings, "AAAA");
        // Inherited from [machine], not repeated in the profile.
        assert_eq!(cfg.plugs, "");
        assert_eq!(cfg.etw, "identity");
        let mut m = cfg
            .to_machine_config()
            .expect("valid")
            .build_machine()
            .unwrap();
        assert_eq!(m.encipher_str("HELLOWORLD"), "ILBDAAMTAZ");
    }

    #[test]
    fn unknown_profile_lists_known() {
        let err = example().machine_for(Some("desert")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("naval"), "{msg}");
    }

    #[test]
    fn missing_fields_named() {
        let cfg: AppConfig = toml::from_str("[machine]\nrotors = [\"I\"]\n").unwrap();
        let err = cfg.machine_for(None).unwrap_err();
        assert!(matches!(err, ConfigError::MissingField(_)), "{err:?}");
    }

    #[test]
    fn merge_prefers_overlay() {
        let base = MachineSection {
            rings: Some("AAA".into()),
            positions: Some("AAA".into()),
            ..MachineSection::default()
        };
        let over = MachineSection {
            positions: Some("KDO".into()),
            ..MachineSection::default()
        };
        let merged = base.merged(&over);
        assert_eq!(merged.rings.as_deref(), Some("AAA"));
        assert_eq!(merged.positions.as_deref(), Some("KDO"));
    }

    #[test]
    fn save_roundtrip() {
        let cfg = example();
        let text = cfg.to_toml_string().expect("serializes");
        let back: AppConfig = toml::from_str(&text).expect("reparses");
        assert_eq!(back.profiles.len(), 1);
        assert_eq!(back.solver.top, Some(5));
    }

    const CUSTOM: &str = r#"
[machine]
rotors = ["MY1", "II", "III"]
rings = "AAA"
positions = "AAA"
reflector = "B"

[custom_rotors.MY1]
wiring = "EKMFLGDQVZNTOWYHXUSPAIBRCJ"
notches = ["Q"]
steps = true

[solver]
lang = "en"

[solver_presets.thorough]
max_plugs = 10
restarts = 8
"#;

    #[test]
    fn custom_rotor_matches_historic_twin() {
        // MY1 is wired exactly like rotor I: same ciphertext expected.
        let cfg: AppConfig = toml::from_str(CUSTOM).unwrap();
        let resolved = cfg.machine_for(None).unwrap();
        let mut m = cfg.build_machine(&resolved).unwrap();
        assert_eq!(m.encipher_str("AAAAA"), "BDZGO");
    }

    #[test]
    fn custom_rotor_rejects_bad_wiring_and_unknown_names() {
        let mut cfg: AppConfig = toml::from_str(CUSTOM).unwrap();
        cfg.custom_rotors.get_mut("MY1").unwrap().wiring = "ABC".into();
        let resolved = cfg.machine_for(None).unwrap();
        assert!(cfg.build_machine(&resolved).is_err());
        cfg.custom_rotors.get_mut("MY1").unwrap().wiring = "EKMFLGDQVZNTOWYHXUSPAIBRCJ".into();
        let resolved2 = ResolvedMachine {
            rotors: vec!["NOPE".into(), "II".into(), "III".into()],
            rings: "AAA".into(),
            positions: "AAA".into(),
            reflector: "B".into(),
            plugs: "".into(),
            etw: "identity".into(),
        };
        assert!(cfg.build_machine(&resolved2).is_err());
    }

    #[test]
    fn solver_preset_merges_over_base() {
        let cfg: AppConfig = toml::from_str(CUSTOM).unwrap();
        let merged = cfg.solver_for(Some("thorough")).unwrap();
        assert_eq!(merged.lang.as_deref(), Some("en"));
        assert_eq!(merged.max_plugs, Some(10));
        assert_eq!(merged.restarts, Some(8));
        assert!(cfg.solver_for(Some("missing")).is_err());
    }
}
