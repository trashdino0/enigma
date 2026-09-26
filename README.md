# Enigma M3/M4 — machine, solvers, CLI and TUI

[![CI](https://github.com/trashdino0/enigma/actions/workflows/ci.yml/badge.svg)](https://github.com/trashdino0/enigma/actions/workflows/ci.yml)

A historically exact Enigma machine (Wehrmacht M3 + Naval M4) in Rust, with
known-plaintext (crib) and ciphertext-only (blind) solvers, a CLI, and an
interactive terminal. Zero heap allocations in the encipher/score hot loops;
search parallelized with rayon.

- **Exact stepping**: double-stepping anomaly, `Ringstellung` wiring offset with
  ring-independent turnover, reciprocal plugboard (≤ 10 pairs).
- **Full coverage**: rotors I–VIII + Beta/Gamma, reflectors B/C + Thin-B/C,
  identity (Wehrmacht/Naval) and QWERTZ (D/K/Railway) entry wheels, custom
  rotor/reflector/ETW definitions.
- **Solvers**: crib search over rotor orders × positions with JSON checkpoint
  resume; blind attack via position scan + plugboard hill-climb; German
  (default) and English quadgram scoring, embedded in the binary.

## Layout

| Crate          | What it is                                              |
|----------------|---------------------------------------------------------|
| `enigma-core`  | Zero-dependency library: rotors, reflector, ETW, plugboard, machine, config |
| `enigma-solver`| Crib + blind solvers, quadgram scoring (`rayon`, `serde`) |
| `enigma-cli`   | `enigma` binary: encrypt/decrypt/solve-crib/solve-blind |
| `enigma-tui`   | `enigma-tui` binary: live typing + signal path, crib progress view |
| `examples/`    | Demo plaintext/cipher pairs used below                   |

## Build

Requires stable Rust (developed on 1.98). Solvers are CPU-heavy: use
`--release` for anything beyond the unit tests.

```sh
cargo build                 # debug binaries in target/debug/
cargo build --release       # optimized binaries in target/release/
cargo test --workspace      # 60 tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo bench -p enigma-solver   # hot-loop baselines (release)
```

## Machine usage

Rotor order is left → right (last = fast). M4 takes a non-stepping
Beta/Gamma 4th rotor first plus a thin reflector.

```sh
# Known-answer vector: I II III, AAA/AAA, UKW-B, no plugs
enigma encrypt --rotors I II III --rings AAA --pos AAA --reflector B --text AAAAA
# BDZGO

# Decrypting is the same operation with the same settings
enigma decrypt --rotors I II III --rings AAA --pos AAA --reflector B --text BDZGO
# AAAAA

# M4 Naval example (4th rotor never steps)
enigma encrypt --rotors Beta I II III --rings AAAA --pos AAAA \
  --reflector Thin-B --text HELLOWORLD
# ILBDAAMTAZ

# Files and pipes: --text beats --input beats stdin; --output writes a file
enigma encrypt --rotors I II III --pos KDO --reflector B \
  --plugs "AV BS CG" --input msg.txt --output cipher.txt
echo HELLO | enigma encrypt --rotors I II III --pos AAA --reflector B
```

Letters are uppercased; anything else passes through **without** stepping the
rotors. Umlauts are not transliterated — write `AE/OE/UE/SS` yourself.

## Solver usage

Rings, reflector, plugs (crib), and entry wheel are fixed inputs; the search
recovers orders and positions (plus plugs in blind mode). `--lang de|en`
selects the scoring table (German default — the historical language).

```sh
# Crib: known plaintext fragment, scans every offset by default
enigma solve-crib --rotors I II III --crib MORGENGRAUEN \
  --input examples/crib_cipher.txt --lang de --top 3
# #1 order I II III pos KDO matches 12/12 score -543.0

# ...with a fixed crib offset, JSON output, and resume support for long runs
enigma solve-crib --rotors I II III IV V --crib WETTER --crib-offset 4 \
  --input cipher.txt --checkpoint-file crib.json --output-json winners.json

# Blind: fixed order, recovers positions AND plugs (needs ~150+ chars)
enigma solve-blind --rotors I II III --input examples/blind_cipher.txt \
  --lang en --max-plugs 6 --top-positions 8 --restarts 3 --top 2
# #1 order I II III pos MKL plugs AV BS CG score -2791.1
#   THEWEATHERREPORT...

# Blind over a rotor pool instead of a fixed order (M4 via --fourth Beta)
enigma solve-blind --pool I II III --input examples/blind_cipher.txt --lang en \
  --max-plugs 0 --top-positions 3 --top 1 --restarts 0
# #1 order I II III pos MKL ...   (plugs unknown here, so text stays garbled)
```

Measured on a modern desktop (release): the crib demo above ≈ 0.15 s, the
blind demo ≈ 0.25 s. Debug builds are an order of magnitude slower — always
time and demo with `--release`.

## TUI

Menu-driven — no flags required; everything is configured in forms. CLI
flags only prefill the forms.

```sh
enigma-tui
enigma-tui --rotors I II III --reflector B --lang de   # prefills
```

The menu offers three modes:

1. **Type** — encipher interactively with live rotor windows and a per-stage
   signal path. Form: rotors, rings, positions, reflector, plugs, entry wheel.
   Keys: type `A-Z`, `Backspace` undoes, `Ctrl-R` resets, `Esc` back to menu.
2. **Solve (crib)** — known-plaintext search with progress gauge and live top
   candidates. Form adds: rotor pool (always permuted), M4 fourth, language,
   ciphertext, crib, crib offset, winner count.
3. **Solve (blind)** — pool search with scan/climb/order stage counters and
   live winners. Form adds plug cap, top positions, restarts, seed.

Form keys: `↑↓`/`Tab` move, `Enter` edits, `Esc` goes back, `F5` validates and
starts (validation errors stay on the form). `Q`/`Esc` in a solve screen ends
the search and returns to the menu — resume long searches through the CLI
`--checkpoint-file`.

## Manual testing walkthrough

1. `cargo build --release`
2. `enigma encrypt --rotors I II III --rings AAA --pos AAA --reflector B --text AAAAA` → expect `BDZGO`.
3. Roundtrip: encrypt `examples/crib_plain.txt` with `--pos KDO`, decrypt the
   result with the same flags → expect the original text back.
4. M4: `HELLOWORLD` with `--rotors Beta I II III --rings AAAA --pos AAAA --reflector Thin-B`
   → expect `ILBDAAMTAZ`; decrypt it back.
5. Crib: `solve-crib --rotors I II III --crib MORGENGRAUEN --input examples/crib_cipher.txt --lang de`
   → expect `#1 order I II III pos KDO matches 12/12`.
6. Blind: `solve-blind --rotors I II III --input examples/blind_cipher.txt --lang en --max-plugs 6 --top-positions 8 --restarts 3`
   → expect `#1 ... pos MKL plugs AV BS CG` and the full weather report.
7. Break it on purpose: duplicate rotors (`--rotors I I II`), 11 plug pairs,
   crib longer than cipher — each must fail with a one-line `enigma: ...` message.
8. TUI: launch `enigma-tui`, open Type, type `AAAAA`, watch windows show
   `A A F` and output `BDZGO`; open Solve (crib), fill cipher + crib from
   step 5, `F5`, watch `#1 order I II III pos KDO` appear.

## Development

- `cargo test --workspace` — unit + integration tests (55).
- `cargo bench -p enigma-solver` — baselines: `encipher_1k` ≈ 42 µs,
  `score_1k` ≈ 1.8 µs (release). Measure before/after every optimization.
- `cargo doc -p enigma-core --no-deps --open` — API docs.

Known limits: rings are always assumed known (no ring search); blind mode
wants 150+ characters and a fixed order or small pool; M4-scale blind pools
are computationally out of reach without a crib — historically honest, the
Bombe needed cribs too.

## Data + license

Quadgram tables in `enigma-solver/data/` mirror
[torognes/enigma](https://github.com/torognes/enigma), originally from James
Lyons' [Practical Cryptography](https://practicalcryptography.com/cryptanalysis/letter-frequencies-various-languages/).
See `enigma-solver/data/README.md`.

Licensed under MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`).
