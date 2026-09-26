//! Native file dialogs (open/save) plus tiny read/write wrappers.
//!
//! Dialogs only run from button handlers — never in tests.

use std::path::PathBuf;

/// Pick an existing `.toml` config file.
pub fn pick_toml() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("TOML config", &["toml"])
        .pick_file()
}

/// Choose where to write a `.toml` config file.
pub fn save_toml() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("TOML config", &["toml"])
        .set_file_name("machine.toml")
        .save_file()
}

/// Choose where to write a `.txt` message file.
pub fn save_txt(default_name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Text file", &["txt"])
        .set_file_name(default_name)
        .save_file()
}

/// Write text, mapping I/O errors to display strings.
pub fn write_text(path: &PathBuf, content: &str) -> Result<(), String> {
    std::fs::write(path, content).map_err(|e| e.to_string())
}

/// Split a config section's rotors into `(fourth, pool)` for solver forms: a
/// leading Beta/Gamma becomes the M4 fourth, everything else is the pool.
pub fn split_rotors(rotors: &[String]) -> (String, String) {
    match rotors {
        [first, rest @ ..] if first == "Beta" || first == "Gamma" => {
            (first.clone(), rest.join(" "))
        }
        _ => (String::new(), rotors.join(" ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fourth_splits_off() {
        let (fourth, pool) = split_rotors(&["Beta".into(), "I".into(), "II".into()]);
        assert_eq!(fourth, "Beta");
        assert_eq!(pool, "I II");
        let (fourth, pool) = split_rotors(&["I".into(), "II".into()]);
        assert_eq!(fourth, "");
        assert_eq!(pool, "I II");
    }
}
