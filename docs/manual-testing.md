# Manual testing walkthrough

Use the release binaries (`cargo build --release`). Every step below was
executed against them; outputs are exact.

1. `enigma encrypt --rotors I II III --rings AAA --pos AAA --reflector B --text AAAAA`
   → expect `BDZGO` (published historical vector).
2. Roundtrip: encrypt `examples/crib_plain.txt` with `--pos KDO`, decrypt the
   result with the same flags → expect the original text back.
3. M4: `HELLOWORLD` with `--rotors Beta I II III --rings AAAA --pos AAAA --reflector Thin-B`
   → expect `ILBDAAMTAZ`; decrypt it back.
4. Crib: `solve-crib --rotors I II III --crib MORGENGRAUEN --input examples/crib_cipher.txt --lang de`
   → expect `#1 order I II III pos KDO rings AAA matches 12/12` (≈ 0.15 s).
4b. Ring scan: encrypt `examples/blind_plain.txt` with `--rotors I II III
   --rings DAA --pos KDO`, then `solve-crib --rotors I II III
   --crib PLEASANTWITHLITTLE --ring-scan 0 --lang en` → expect `matches
   18/18`; decrypting with the reported key reproduces the file
   (ring/position equivalence classes — see `solvers.md`).
4c. Naval M4 crib: `solve-crib --rotors VI VII VIII --fourth Beta --rings NORD
   --reflector Thin-C --plugs "AO IU" --crib GELEITZUG --input examples/naval_cipher.txt --lang de`
   → expect `#1 order Beta VI VII VIII pos GSTQ rings NORD matches 9/9`.
4d. Plug inference: encrypt a ~60-char text at `I II III/AAA/MKL` with plugs
   `AV BS CG`, then `solve-crib --rotors I II III --crib <12-char fragment>
   --infer-plugs --plug-top 30 --max-plugs 6` → expect full matches with all
   three plugs recovered.
5. Blind: `solve-blind --rotors I II III --input examples/blind_cipher.txt --lang en --max-plugs 6 --top-positions 8 --restarts 3`
   → expect `#1 ... pos MKL plugs AV BS CG` and the full weather report (≈ 0.25 s).
6. Pool: `solve-blind --pool I II III --input examples/blind_cipher.txt --lang en --max-plugs 0 --top-positions 3 --top 1 --restarts 0`
   → expect `#1 order I II III pos MKL` (garbled text — plugs unknown).
7. Break it on purpose: duplicate rotors (`--rotors I I II`), 11 plug pairs,
   crib longer than cipher — each must fail with a one-line `enigma: ...` message.
8. GUI: `cargo run --release -p enigma-gui`, Machine tab, `Load machine`,
   type `AAAAA` → windows `A A F`, output `BDZGO`. Crib tab with the step-4
   inputs → `#1 order I II III pos KDO`.
9. TUI: `enigma-tui`, open Type, type `AAAAA` → same `BDZGO`; open Solve
   (crib), fill cipher + crib from step 4, `F5` → `#1 ... pos KDO`.
