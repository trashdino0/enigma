//! CLI integration tests: known vectors + solver subcommands end-to-end.

use assert_cmd::Command;

fn enigma() -> Command {
    Command::cargo_bin("enigma").expect("enigma binary builds")
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
    let mut args = vec!["encrypt".to_string(), "--rotors".to_string()];
    args.extend(rotors.iter().map(|s| s.to_string()));
    for s in [
        "--rings",
        "AAA",
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
