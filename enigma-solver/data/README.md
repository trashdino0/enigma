# Solver n-gram data

Both tables are mirrored from [torognes/enigma](https://github.com/torognes/enigma),
which in turn obtained them from James Lyons'
[Practical Cryptography](https://practicalcryptography.com/cryptanalysis/letter-frequencies-various-languages/)
frequency tables. Format: `QUAD COUNT` per line (e.g. `TION 13168375`).

- `english_quadgrams.txt` (3.3 MB) — English letter-quadgram counts.
- `german_quadgrams.txt` (3.0 MB) — German letter-quadgram counts
  (primary historical Enigma language).

They are embedded into solver binaries with `include_str!` (selected at
runtime via `--lang de|en`) and parsed once into a flat `26^4` log-probability
array — no hashing, no heap in the scoring hot loop.
