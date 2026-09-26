# Solvers

Two attacks, matching the two historical situations: a guessed plaintext
fragment exists (crib), or only ciphertext exists (blind). Both live in the
`enigma-solver` crate and run on rayon; scoring uses quadgram
log-probabilities from tables embedded in the binary (`--lang de|en`,
German default).

## Crib search (`crib.rs`)

For every rotor order from the pool (M3: taken 3; M4: fixed `--fourth` plus
taken 3) and every start position (26³ = 17,576 for M3, 26⁴ for M4), decrypt
and count crib matches at the offset (or every offset). Candidates at or
above `--min-matches` are shortlisted; quadgram fitness breaks ties. Orders
run sequentially so `--checkpoint-file` can resume per finished order, while
positions within an order use all cores.

Assumes rings, reflector, plugs, and entry wheel known. On this hardware the
`examples/crib_cipher.txt` demo (6 orders) takes ≈ 0.15 s in release.

## Blind search (`hillclimb.rs`)

1. **Scan.** Every position decrypted plugless and quadgram-scored; the top
   `--top-positions` survive.
2. **Climb.** Per surviving position, greedy plugboard search from an
   unplugged start plus `--restarts` seeded random starts (xorshift64, no
   `rand` dependency). Moves: add/drop a pair, re-terminate one end, swap two
   ends; capped at `--max-plugs`; stops with no improvement (200-iteration
   backstop).
3. **Merge.** Winners ranked by `(score desc, positions asc, plugs asc)` —
   deterministic for a fixed `--seed`; identical restarts deduped.

`--pool` (+ `--fourth`) repeats this per order with per-order seeds and
merges globally. The demo (`I II III`, `MKL`, `AV BS CG`) takes ≈ 0.25 s in
release. Debug builds are ~10× slower — always benchmark and demo with
`--release`.

## Limits (honest)

- Rings are always assumed known — no ring search exists yet.
- Blind mode wants ~150+ characters and a fixed order or small pool; M4-scale
  blind pools are out of reach without a crib (as in 1942).
- Shortlisted crib candidates below full matches only make sense with
  `--min-matches` lowered deliberately.

## Scoring (`score.rs`)

Quadgram tables parse once into a flat 26⁴ `f32` array: one indexed load per
character, no hashing, no heap, `Sync` for rayon. Unseen quads score
`ln(0.01/total)` (Lyons' floor). Non-A-Z lines (e.g. German umlaut quads) are
skipped — they can't occur in the machine's 26-letter domain. Index of
coincidence is provided as a cheap pre-filter (≈ 0.038 random vs ≈ 0.076
language).
