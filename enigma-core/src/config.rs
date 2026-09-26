//! Validated machine configuration: parse strings once, build machines often.
//!
//! [`MachineConfig`] is the boundary between user input (CLI/TUI/solver) and
//! the hot loop. All parsing and cross-field validation (rotor distinctness,
//! M4 thin-rotor placement, plug limits) happens here, so
//! [`crate::machine::EnigmaMachine`] construction from a valid config cannot
//! fail on shape — only on genuinely inconsistent state.
//!
//! Deliberately dependency-free (no `serde` yet): JSON checkpoint
//! serialization for long M4 solver runs arrives with the solver CLI story
//! and will derive on top of these types without changing their shape.

use std::fmt;
use std::str::FromStr;

use crate::{
    etw::EntryWheel,
    machine::EnigmaMachine,
    plugboard::Plugboard,
    reflector::Reflector,
    rotor::{HistoricalRotor, Rotor},
    EnigmaError,
};

/// Which reflector to fit. Thin variants pair with the M4 Beta/Gamma rotor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReflectorKind {
    A,
    B,
    C,
    ThinB,
    ThinC,
}

impl ReflectorKind {
    /// Parse `B`, `UKW-B`, `Thin-C`, `thinb`, ... (case-insensitive).
    pub fn parse(s: &str) -> Result<Self, EnigmaError> {
        let key: String = s
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        match key.as_str() {
            "A" | "UKWA" => Ok(Self::A),
            "B" | "UKWB" => Ok(Self::B),
            "C" | "UKWC" => Ok(Self::C),
            "THINB" | "UKWBTHIN" | "BTHIN" | "BDUNN" | "DUNNB" => Ok(Self::ThinB),
            "THINC" | "UKWCTHIN" | "CTHIN" | "CDUNN" | "DUNNC" => Ok(Self::ThinC),
            _ => Err(EnigmaError::InvalidReflectorName(s.to_string())),
        }
    }

    /// Build the validated reflector.
    pub fn build(self) -> Result<Reflector, EnigmaError> {
        match self {
            Self::A => Reflector::a(),
            Self::B => Reflector::b(),
            Self::C => Reflector::c(),
            Self::ThinB => Reflector::thin_b(),
            Self::ThinC => Reflector::thin_c(),
        }
    }
}

impl FromStr for ReflectorKind {
    type Err = EnigmaError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Which entry wheel to fit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EtwKind {
    /// Wehrmacht/Naval identity (`A -> A`).
    Identity,
    /// Commercial/Railway QWERTZ.
    Qwertz,
    /// 26-letter custom mapping.
    Custom(String),
}

impl EtwKind {
    /// Parse `identity`, `qwertz`/`commercial`/`railway`, or a 26-letter wiring.
    pub fn parse(s: &str) -> Result<Self, EnigmaError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "identity" | "abc" | "etw" | "wehrmacht" | "naval" => Ok(Self::Identity),
            "qwertz" | "commercial" | "railway" | "d" | "k" => Ok(Self::Qwertz),
            _ => {
                let upper = s.trim().to_ascii_uppercase();
                // Validate eagerly so typos fail at parse time, not build time.
                EntryWheel::custom(&upper)?;
                Ok(Self::Custom(upper))
            }
        }
    }

    /// Build the validated entry wheel.
    pub fn build(&self) -> Result<EntryWheel, EnigmaError> {
        match self {
            Self::Identity => Ok(EntryWheel::identity()),
            Self::Qwertz => Ok(EntryWheel::qwertz()),
            Self::Custom(w) => EntryWheel::custom(w),
        }
    }
}

impl FromStr for EtwKind {
    type Err = EnigmaError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Parse a historic rotor name (`I`-`VIII`, `Beta`, `Gamma`; case-insensitive).
pub fn parse_rotor_name(s: &str) -> Result<HistoricalRotor, EnigmaError> {
    match s.trim().to_ascii_uppercase().as_str() {
        "I" => Ok(HistoricalRotor::I),
        "II" => Ok(HistoricalRotor::II),
        "III" => Ok(HistoricalRotor::III),
        "IV" => Ok(HistoricalRotor::IV),
        "V" => Ok(HistoricalRotor::V),
        "VI" => Ok(HistoricalRotor::VI),
        "VII" => Ok(HistoricalRotor::VII),
        "VIII" => Ok(HistoricalRotor::VIII),
        "BETA" => Ok(HistoricalRotor::Beta),
        "GAMMA" => Ok(HistoricalRotor::Gamma),
        _ => Err(EnigmaError::InvalidRotorName(s.to_string())),
    }
}

/// Parse a letter string like `"AAA"` or `"QWE"` into `0..26` positions.
pub fn parse_letters(s: &str) -> Result<Vec<u8>, EnigmaError> {
    let bytes = s.trim().to_ascii_uppercase().into_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    for (i, b) in bytes.iter().enumerate() {
        if !b.is_ascii_uppercase() {
            return Err(EnigmaError::InvalidWiringChar { index: i, byte: *b });
        }
        out.push(b - b'A');
    }
    Ok(out)
}

/// A fully validated Enigma setup. Build once via [`MachineConfig::new`] or
/// [`MachineConfig::from_strings`], then call [`MachineConfig::build_machine`]
/// as often as needed (solvers rebuild per candidate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineConfig {
    /// Left -> right rotor identities.
    pub rotors: Vec<HistoricalRotor>,
    /// Ring settings, one per rotor.
    pub rings: Vec<u8>,
    /// Start positions, one per rotor.
    pub positions: Vec<u8>,
    /// Reflector choice.
    pub reflector: ReflectorKind,
    /// Plugboard pairs as `0..26` tuples.
    pub plugs: Vec<(u8, u8)>,
    /// Entry wheel choice.
    pub etw: EtwKind,
}

impl MachineConfig {
    /// Validate cross-field rules and store. All `u8` values must be `< 26`.
    pub fn new(
        rotors: Vec<HistoricalRotor>,
        rings: Vec<u8>,
        positions: Vec<u8>,
        reflector: ReflectorKind,
        plugs: Vec<(u8, u8)>,
        etw: EtwKind,
    ) -> Result<Self, EnigmaError> {
        if rotors.len() != 3 && rotors.len() != 4 {
            return Err(EnigmaError::InvalidRotorCount(rotors.len()));
        }
        for (field, values) in [("rings", &rings), ("positions", &positions)] {
            if values.len() != rotors.len() {
                return Err(EnigmaError::ConfigLengthMismatch {
                    field,
                    expected: rotors.len(),
                    got: values.len(),
                });
            }
            if values.iter().any(|&v| v >= 26) {
                let bad = values.iter().find(|&&v| v >= 26).copied().unwrap_or(26);
                if field == "rings" {
                    return Err(EnigmaError::InvalidRing(bad));
                }
                return Err(EnigmaError::InvalidPosition(bad));
            }
        }
        // Stepping rotors must be distinct; thin rotors only as M4 4th.
        let mut seen_steps = [false; 8];
        for (i, &r) in rotors.iter().enumerate() {
            let steps = r.spec().steps;
            if steps {
                let idx = stepping_index(r).expect("stepping rotor has index");
                if seen_steps[idx] {
                    return Err(EnigmaError::DuplicateRotor);
                }
                seen_steps[idx] = true;
                if rotors.len() == 4 && i == 0 {
                    return Err(EnigmaError::FourthRotorMustBeNonStepping);
                }
            } else {
                // Beta/Gamma.
                if rotors.len() != 4 || i != 0 {
                    return Err(EnigmaError::NonSteppingRotorNotAllowedHere);
                }
            }
        }
        // Plugboard limits (self-stecker, reuse, >10) checked here so solver
        // candidates with bad plugs fail before machine construction.
        Plugboard::from_pairs(&plugs)?;
        // Reflector/ETW wirings are historic constants or pre-validated.
        Ok(Self {
            rotors,
            rings,
            positions,
            reflector,
            plugs,
            etw,
        })
    }

    /// Parse the human forms: `["I","II","III"]`, `"AAA"`, `"AAA"`, `"B"`,
    /// `"AV BS"`, `"identity"`.
    pub fn from_strings(
        rotor_names: &[&str],
        rings: &str,
        positions: &str,
        reflector: &str,
        plugs: &str,
        etw: &str,
    ) -> Result<Self, EnigmaError> {
        let mut rotors = Vec::with_capacity(rotor_names.len());
        for name in rotor_names {
            rotors.push(parse_rotor_name(name)?);
        }
        let rings = parse_letters(rings)?;
        let positions = parse_letters(positions)?;
        let reflector = ReflectorKind::parse(reflector)?;
        let plugboard = Plugboard::from_wiring(plugs)?;
        // Recover validated pairs from the plugboard map (canonical form).
        let mut plugs_vec = Vec::new();
        let mut seen = [false; 26];
        for a in 0..26u8 {
            let b = plugboard.swap(a);
            if b != a && !seen[a as usize] {
                plugs_vec.push((a, b));
                seen[a as usize] = true;
                seen[b as usize] = true;
            }
        }
        let etw = EtwKind::parse(etw)?;
        Self::new(rotors, rings, positions, reflector, plugs_vec, etw)
    }

    /// Assemble a ready-to-encipher machine at the configured start positions.
    pub fn build_machine(&self) -> Result<EnigmaMachine, EnigmaError> {
        let mut rotors = Vec::with_capacity(self.rotors.len());
        for ((&kind, &ring), &pos) in self.rotors.iter().zip(&self.rings).zip(&self.positions) {
            rotors.push(Rotor::historical(kind, ring, pos)?);
        }
        EnigmaMachine::new(
            self.etw.build()?,
            rotors,
            self.reflector.build()?,
            Plugboard::from_pairs(&self.plugs)?,
        )
    }
}

impl fmt::Display for MachineConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.rotors.iter().map(rotor_short_name).collect();
        let rings: String = self.rings.iter().map(|&p| crate::pos_to_char(p)).collect();
        let pos: String = self
            .positions
            .iter()
            .map(|&p| crate::pos_to_char(p))
            .collect();
        write!(
            f,
            "rotors {} rings {rings} pos {pos} reflector {:?} plugs {} etw {:?}",
            names.join(" "),
            self.reflector,
            plugs_to_string(&self.plugs),
            self.etw
        )
    }
}

fn stepping_index(r: HistoricalRotor) -> Option<usize> {
    match r {
        HistoricalRotor::I => Some(0),
        HistoricalRotor::II => Some(1),
        HistoricalRotor::III => Some(2),
        HistoricalRotor::IV => Some(3),
        HistoricalRotor::V => Some(4),
        HistoricalRotor::VI => Some(5),
        HistoricalRotor::VII => Some(6),
        HistoricalRotor::VIII => Some(7),
        HistoricalRotor::Beta | HistoricalRotor::Gamma => None,
    }
}

fn rotor_short_name(r: &HistoricalRotor) -> &'static str {
    match r {
        HistoricalRotor::I => "I",
        HistoricalRotor::II => "II",
        HistoricalRotor::III => "III",
        HistoricalRotor::IV => "IV",
        HistoricalRotor::V => "V",
        HistoricalRotor::VI => "VI",
        HistoricalRotor::VII => "VII",
        HistoricalRotor::VIII => "VIII",
        HistoricalRotor::Beta => "Beta",
        HistoricalRotor::Gamma => "Gamma",
    }
}

fn plugs_to_string(plugs: &[(u8, u8)]) -> String {
    if plugs.is_empty() {
        return "-".to_string();
    }
    plugs
        .iter()
        .map(|&(a, b)| format!("{}{}", crate::pos_to_char(a), crate::pos_to_char(b)))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m3() -> MachineConfig {
        MachineConfig::from_strings(&["I", "II", "III"], "AAA", "AAA", "B", "", "identity")
            .expect("valid M3 config")
    }

    #[test]
    fn config_builds_machine_passing_bdzgo() {
        let mut m = m3().build_machine().expect("buildable");
        assert_eq!(m.encipher_str("AAAAA"), "BDZGO");
    }

    #[test]
    fn m4_beta_config_builds_and_holds_fourth_fixed() {
        let cfg = MachineConfig::from_strings(
            &["Beta", "I", "II", "III"],
            "AAAA",
            "AAAA",
            "Thin-B",
            "",
            "identity",
        )
        .expect("valid M4 config");
        let mut m = cfg.build_machine().expect("buildable");
        assert_eq!(m.rotor_count(), 4);
        m.encipher_str(&"A".repeat(50));
        assert_eq!(m.positions()[0], 0);
    }

    #[test]
    fn parsing_helpers() {
        assert_eq!(parse_rotor_name("iii").unwrap(), HistoricalRotor::III);
        assert_eq!(parse_rotor_name("Beta").unwrap(), HistoricalRotor::Beta);
        assert!(parse_rotor_name("IX").is_err());
        assert_eq!(parse_letters("AaZ").unwrap(), vec![0, 0, 25]);
        assert_eq!(ReflectorKind::parse("ukw-b").unwrap(), ReflectorKind::B);
        assert_eq!(
            ReflectorKind::parse("THIN-C").unwrap(),
            ReflectorKind::ThinC
        );
        assert!(ReflectorKind::parse("D").is_err());
        assert_eq!(EtwKind::parse("QWERTZ").unwrap(), EtwKind::Qwertz);
        assert!(matches!(
            EtwKind::parse("QWERTZUIOASDFGHJKPYXCVBNML").unwrap(),
            EtwKind::Qwertz | EtwKind::Custom(_)
        ));
        assert!(EtwKind::parse("not-an-etw").is_err());
    }

    #[test]
    fn invalid_configs_rejected() {
        // Duplicate stepping rotor.
        assert!(
            MachineConfig::from_strings(&["I", "I", "II"], "AAA", "AAA", "B", "", "identity")
                .is_err()
        );
        // Beta outside the M4 4th slot.
        assert!(MachineConfig::from_strings(
            &["Beta", "II", "III"],
            "AAA",
            "AAA",
            "B",
            "",
            "identity"
        )
        .is_err());
        assert!(MachineConfig::from_strings(
            &["I", "Beta", "III"],
            "AAA",
            "AAA",
            "B",
            "",
            "identity"
        )
        .is_err());
        // Stepping rotor as M4 4th.
        assert!(MachineConfig::from_strings(
            &["I", "II", "III", "IV"],
            "AAAA",
            "AAAA",
            "Thin-B",
            "",
            "identity"
        )
        .is_err());
        // Length mismatches.
        assert!(
            MachineConfig::from_strings(&["I", "II", "III"], "AA", "AAA", "B", "", "identity")
                .is_err()
        );
        // Bad plugs / rings / reflector.
        assert!(MachineConfig::from_strings(
            &["I", "II", "III"],
            "AAA",
            "AAA",
            "B",
            "AA",
            "identity"
        )
        .is_err());
        assert!(
            MachineConfig::from_strings(&["I", "II"], "AA", "AA", "B", "", "identity").is_err()
        );
        assert!(MachineConfig::from_strings(
            &["I", "II", "III"],
            "AAA",
            "AAA",
            "Z",
            "",
            "identity"
        )
        .is_err());
    }

    #[test]
    fn display_is_human_readable() {
        let s = m3().to_string();
        assert!(s.contains("I II III"), "{s}");
        assert!(s.contains("B"), "{s}");
    }
}
