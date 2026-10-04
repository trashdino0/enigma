//! CLI integration tests: known vectors + solver subcommands end-to-end.

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;

fn enigma() -> Command {
    Command::cargo_bin("enigma").expect("enigma binary builds")
}

/// Absolute path to a file in the workspace root (tests run with the
/// crate dir as CWD, so `examples/...` would not resolve).
fn workspace_file(rel: &str) -> String {
    format!("{}/../{}", env!("CARGO_MANIFEST_DIR"), rel)
}

#[test]
fn encrypt_aaaaa_to_bdzgo() {
    enigma()
        .args([
            "encrypt",
            "--rotors",
            "I",
            "II",
            "III",
            "--rings",
            "AAA",
            "--pos",
            "AAA",
            "--reflector",
            "B",
            "--text",
            "AAAAA",
        ])
        .assert()
        .success()
        .stdout("BDZGO");
}

#[test]
fn decrypt_reverses_encrypt() {
    let mut enc = enigma();
    let cipher = enc
        .args([
            "encrypt",
            "--rotors",
            "IV",
            "II",
            "V",
            "--rings",
            "BQU",
            "--pos",
            "WZA",
            "--reflector",
            "C",
            "--plugs",
            "AV BS CG",
            "--text",
            "HELLOWORLD",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let cipher = String::from_utf8(cipher).unwrap();

    enigma()
        .args([
            "decrypt",
            "--rotors",
            "IV",
            "II",
            "V",
            "--rings",
            "BQU",
            "--pos",
            "WZA",
            "--reflector",
            "C",
            "--plugs",
            "AV BS CG",
            "--text",
            &cipher,
        ])
        .assert()
        .success()
        .stdout("HELLOWORLD");
}

#[test]
fn bad_config_fails_with_message() {
    enigma()
        .args(["encrypt", "--rotors", "I", "I", "II", "--text", "A"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("repeat"));
}

const EN_250: &str = "THEWEATHERREPORTFORTHENEXTFEWDAYSFORECASTSSUNSHINEANDWARM\
TEMPERATURESWITHAGENTLEBREEZEFROMTHEWESTINTHEAFTERNOONANDCLEARSKIESOVERNIGHT\
THEOUTLOOKFORTHEWEEKENDREMAINSPLEASANTWITHLITTLECHANCEOFRAINANDTHEHARVESTEXPE";

fn encrypt_stdout(rotors: &[&str], pos: &str, plugs: &str, text: &str) -> String {
    encrypt_stdout_ring(rotors, "AAA", pos, plugs, text)
}

fn encrypt_stdout_ring(rotors: &[&str], rings: &str, pos: &str, plugs: &str, text: &str) -> String {
    let mut args = vec!["encrypt".to_string(), "--rotors".to_string()];
    args.extend(rotors.iter().map(|s| s.to_string()));
    for s in [
        "--rings",
        rings,
        "--pos",
        pos,
        "--reflector",
        "B",
        "--plugs",
        plugs,
        "--text",
        text,
    ] {
        args.push(s.to_string());
    }
    let bytes = enigma()
        .args(&args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn solve_crib_recovers_key() {
    let cipher = encrypt_stdout(&["I", "II", "III"], "KDO", "", EN_250);
    enigma()
        .args([
            "solve-crib",
            "--rotors",
            "I",
            "II",
            "III",
            "--crib",
            "PLEASANTWITHLITTLE",
            "--cipher",
            cipher.trim(),
            "--lang",
            "en",
            "--top",
            "3",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("pos KDO"));
}

#[test]
fn solve_blind_positions_only_no_plugs() {
    let cipher = encrypt_stdout(&["I", "II", "III"], "MKL", "", EN_250);
    enigma()
        .args([
            "solve-blind",
            "--rotors",
            "I",
            "II",
            "III",
            "--cipher",
            cipher.trim(),
            "--lang",
            "en",
            "--max-plugs",
            "0",
            "--top-positions",
            "5",
            "--top",
            "1",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("pos MKL"));
}

fn temp_json(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("enigma_cli_test_{name}.json"))
}

#[test]
fn solve_crib_output_json() {
    let path = temp_json("crib");
    let _ = std::fs::remove_file(&path);
    let cipher = encrypt_stdout(&["I", "II", "III"], "KDO", "", EN_250);
    enigma()
        .args([
            "solve-crib",
            "--rotors",
            "I",
            "II",
            "III",
            "--crib",
            "PLEASANTWITHLITTLE",
            "--cipher",
            cipher.trim(),
            "--lang",
            "en",
            "--top",
            "2",
            "--output-json",
            path.to_str().unwrap(),
        ])
        .assert()
        .success();
    let json = std::fs::read_to_string(&path).unwrap();
    assert!(json.contains("\"matches\": 18"), "{json}");
    assert!(json.contains("\"II\""), "{json}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn config_file_drives_encrypt() {
    let config = workspace_file("examples/day.toml");
    enigma()
        .args(["encrypt", "--config", &config, "--text", "AAAAA"])
        .assert()
        .success()
        .stdout("BDZGO");
}

#[test]
fn config_profile_and_flag_override() {
    let config = workspace_file("examples/day.toml");
    // Naval profile from the file.
    enigma()
        .args([
            "encrypt",
            "--config",
            &config,
            "--profile",
            "naval",
            "--text",
            "HELLOWORLD",
        ])
        .assert()
        .success()
        .stdout("ILBDAAMTAZ");
    // Flag overrides the file's positions (AAA -> AAB changes output).
    enigma()
        .args([
            "encrypt", "--config", &config, "--pos", "AAB", "--text", "AAAAA",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("BDZGO").not());
}

#[test]
fn unknown_profile_fails_clearly() {
    let config = workspace_file("examples/day.toml");
    enigma()
        .args([
            "encrypt",
            "--config",
            &config,
            "--profile",
            "desert",
            "--text",
            "A",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("naval"));
}

#[test]
fn solve_blind_pool_finds_order() {
    let cipher = encrypt_stdout(&["II", "I", "III"], "BNK", "", EN_250);
    enigma()
        .args([
            "solve-blind",
            "--pool",
            "I",
            "II",
            "III",
            "--cipher",
            cipher.trim(),
            "--lang",
            "en",
            "--max-plugs",
            "0",
            "--top-positions",
            "3",
            "--top",
            "1",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("II I III"));
}

#[test]
fn transliterate_encrypts_umlauts() {
    // Grüße -> GRUESSE before enciphering; decrypt must round-trip it.
    let config = workspace_file("examples/day.toml");
    let cipher = enigma()
        .args([
            "encrypt",
            "--config",
            &config,
            "--transliterate",
            "--text",
            "Grüße",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let cipher = String::from_utf8(cipher).unwrap();
    enigma()
        .args(["decrypt", "--config", &config, "--text", cipher.trim()])
        .assert()
        .success()
        .stdout("GRUESSE");
}

#[test]
fn ring_scan_finds_key_with_wrong_base_rings() {
    // True rings DAA; CLI assumes AAA but scans slot 0: full crib must match.
    let cipher = encrypt_stdout_ring(&["I", "II", "III"], "DAA", "KDO", "", EN_250);
    enigma()
        .args([
            "solve-crib",
            "--rotors",
            "I",
            "II",
            "III",
            "--crib",
            "PLEASANTWITHLITTLE",
            "--cipher",
            cipher.trim(),
            "--lang",
            "en",
            "--ring-scan",
            "0",
            "--top",
            "3",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("matches 18/18"));
}

#[test]
fn solver_preset_overrides_defaults() {
    // Preset accepted: search succeeds and finds the key.
    let config = workspace_file("examples/day.toml");
    let cipher = encrypt_stdout(&["I", "II", "III"], "KDO", "", EN_250);
    enigma()
        .args([
            "solve-crib",
            "--config",
            &config,
            "--solver-preset",
            "quick",
            "--rotors",
            "I",
            "II",
            "III",
            "--crib",
            "PLEASANTWITHLITTLE",
            "--cipher",
            cipher.trim(),
            "--lang",
            "en",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("pos KDO"));
    // Unknown preset fails naming the known one.
    enigma()
        .args([
            "solve-crib",
            "--config",
            &config,
            "--solver-preset",
            "bogus",
            "--rotors",
            "I",
            "II",
            "III",
            "--crib",
            "PLEASANTWITHLITTLE",
            "--cipher",
            cipher.trim(),
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("quick"));
}

#[test]
fn custom_rotor_from_config_enciphers() {
    // MY1 is wired exactly like rotor I: same known vector.
    let dir = std::env::temp_dir();
    let path = dir.join("enigma_custom_test.toml");
    std::fs::write(
        &path,
        "[machine]\nrotors = [\"MY1\", \"II\", \"III\"]\nrings = \"AAA\"\n\
         positions = \"AAA\"\nreflector = \"B\"\n\n[custom_rotors.MY1]\n\
         wiring = \"EKMFLGDQVZNTOWYHXUSPAIBRCJ\"\nnotches = [\"Q\"]\n",
    )
    .unwrap();
    enigma()
        .args([
            "encrypt",
            "--config",
            path.to_str().unwrap(),
            "--text",
            "AAAAA",
        ])
        .assert()
        .success()
        .stdout("BDZGO");
    let _ = std::fs::remove_file(&path);
}
