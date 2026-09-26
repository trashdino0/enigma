# Development

Requires stable Rust (developed on 1.98).

| Crate          | What it is                                              |
|----------------|---------------------------------------------------------|
| `enigma-core`  | Zero-dependency library: rotors, reflector, ETW, plugboard, machine, config |
| `enigma-solver`| Crib + blind solvers, quadgram scoring (`rayon`, `serde`) |
| `enigma-cli`   | `enigma` binary: encrypt/decrypt/solve-crib/solve-blind |
| `enigma-tui`   | `enigma-tui` binary: live typing + signal path, solver progress |
| `enigma-gui`   | `enigma-gui` binary: desktop workbench (machine + crib + blind tabs) |
| `examples/`    | Demo plaintext/cipher pairs                              |

```sh
cargo build --release       # optimized binaries (solvers need this)
cargo test --workspace      # 63 tests: unit + CLI/TUI integration
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo bench -p enigma-solver   # release hot-loop baselines
cargo doc -p enigma-core --no-deps --open
```

Baselines on the dev machine (release): `encipher_1k` ≈ 42 µs
(~24M chars/s), `score_1k` ≈ 1.8 µs. Measure before/after every
optimization; revert neutral or negative results.

Design rules: zero heap in `encipher_char`/scoring (fixed `[u8; 26]`
lookups); fallible parsing only at construction; historically exact stepping
(double-stepping, ring-independent turnover, fixed M4 4th rotor).

## Releases

Pushing a tag `v*` runs `.github/workflows/release.yml`, which builds the
Windows release binaries and attaches `enigma-gui.exe`, `enigma.exe`, and
`enigma-tui.exe` to a GitHub Release. The root README's download option
points there.

## Data

Quadgram tables in `enigma-solver/data/` mirror
[torognes/enigma](https://github.com/torognes/enigma), originally from James
Lyons' [Practical Cryptography](https://practicalcryptography.com/cryptanalysis/letter-frequencies-various-languages/).
Embedded via `include_str!`, parsed once into a flat 26⁴ array.
