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
}
