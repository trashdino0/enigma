//! Hot-loop baselines (measure before/after every optimization, release mode):
//! - `encipher_1k`: full M3 encrypt of 1,000 chars (stepping + signal path).
//! - `score_1k`: quadgram fitness over 1,000 pre-encoded bytes.
//!
//! Run with `cargo bench -p enigma-solver`. Numbers are only meaningful from
//! release builds; `cargo bench` builds benches with optimizations on.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use enigma_core::{config::MachineConfig, machine::EnigmaMachine};
use enigma_solver::score::{Lang, QuadgramScorer};

fn machine_aaa() -> EnigmaMachine {
    MachineConfig::from_strings(&["I", "II", "III"], "AAA", "AAA", "B", "", "identity")
        .expect("bench machine valid")
        .build_machine()
        .expect("bench machine builds")
}

fn bench_encipher_1k(c: &mut Criterion) {
    let text = "A".repeat(1000);
    let mut machine = machine_aaa();
    c.bench_function("encipher_1k", |b| {
        b.iter(|| {
            machine.set_positions(&[0, 0, 0]).expect("reset");
            black_box(machine.encipher_str(&text))
        });
    });
}

fn bench_score_1k(c: &mut Criterion) {
    let scorer = QuadgramScorer::new(Lang::En);
    // Deterministic pseudo-English bytes (repeating pangram fragment).
    let frag: Vec<u8> = b"THEQUICKBROWNFOXJUMPSOVERTHELAZYDOG"
        .iter()
        .map(|b| b - b'A')
        .collect();
    let bytes: Vec<u8> = frag.iter().cycle().take(1000).copied().collect();
    c.bench_function("score_1k", |b| {
        b.iter(|| black_box(scorer.score_bytes(&bytes)));
    });
}

criterion_group!(benches, bench_encipher_1k, bench_score_1k);
criterion_main!(benches);
