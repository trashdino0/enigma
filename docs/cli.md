# Command-line reference (`enigma`)

Rotor order is left → right (last = fast). M4 takes a non-stepping
Beta/Gamma 4th rotor first plus a thin reflector. Letters are uppercased;
anything else passes through **without** stepping the rotors.

## Encrypt / decrypt

The same operation with the same settings (reflector symmetry).

```sh
# Known-answer vector: I II III, AAA/AAA, UKW-B, no plugs
enigma encrypt --rotors I II III --rings AAA --pos AAA --reflector B --text AAAAA
# BDZGO
enigma decrypt --rotors I II III --rings AAA --pos AAA --reflector B --text BDZGO
# AAAAA

# M4 Naval (4th rotor never steps)
enigma encrypt --rotors Beta I II III --rings AAAA --pos AAAA \
  --reflector Thin-B --text HELLOWORLD
# ILBDAAMTAZ

# Files and pipes: --text beats --input beats stdin; --output writes a file
enigma encrypt --rotors I II III --pos KDO --reflector B \
  --plugs "AV BS CG" --input msg.txt --output cipher.txt
echo HELLO | enigma encrypt --rotors I II III --pos AAA --reflector B
```

Flags: `--rotors` (3, or 4 with Beta/Gamma first), `--rings`, `--pos`,
`--reflector` (`B`, `C`, `Thin-B`, `Thin-C`), `--plugs` (`"AV BS CG"`,
empty = unpatched), `--etw` (`identity` for Wehrmacht/Naval, `qwertz` for
D/K/Railway). Umlauts are not transliterated — write `AE/OE/UE/SS` yourself.

## Crib search

Known-plaintext attack over rotor orders × positions (Bombe-style). Rings,
reflector, plugs, and entry wheel are fixed inputs — unless scanned or
inferred (below).

```sh
enigma solve-crib --rotors I II III --crib MORGENGRAUEN \
  --input examples/crib_cipher.txt --lang de --top 3
# #1 order I II III pos KDO rings AAA matches 12/12 plugs - score -543.0

enigma solve-crib --rotors I II III IV V --crib WETTER --crib-offset 4 \
  --input cipher.txt --checkpoint-file crib.json --output-json winners.json
```

M4 example (double-notch naval wheels, thin reflector, plugs):

```sh
enigma solve-crib --rotors VI VII VIII --fourth Beta --rings NORD \
  --reflector Thin-C --plugs "AO IU" --crib GELEITZUG \
  --input examples/naval_cipher.txt --lang de
# #1 order Beta VI VII VIII pos GSTQ rings NORD matches 9/9 plugs AO IU
```

Flags: `--rotors` pool (taken 3 per order), `--fourth`, `--rings`,
`--reflector`, `--plugs` (assumed known), `--etw`, `--crib` (required),
`--crib-offset` (default: scan every offset), `--min-matches` (default: full
crib length, ignored when inferring), `--top`, `--lang de|en`,
`--checkpoint-file` (JSON resume per finished unit),
`--output-json` (pretty winners file).

- `--ring-scan 0,1`: exhaustively try ring slots (0-indexed, max 676
  combos). Units become order × rings; checkpoint keys look like
  `"I II III@DAA"`. Ring/position pairs form equivalence classes — any
  full-match key decrypts the message.
- `--infer-plugs`: recover unknown plugs by crib-anchored hill-climbing
  (greedy add/drop/re-terminate/swap on crib matches, starting from
  `--plugs`). `--plug-top` bounds positions attempted (default 50),
  `--max-plugs` caps pairs (default 10). Called "guided recovery" rather
  than Bombe because it climbs instead of chaining menus — same answers on
  cooperative texts, less machinery.

## Blind search

Ciphertext-only: unplugged position scan, then plugboard hill-climb
(add/drop/re-terminate/swap moves, deterministic seed) per surviving
position. Needs ~150+ characters.

```sh
enigma solve-blind --rotors I II III --input examples/blind_cipher.txt \
  --lang en --max-plugs 6 --top-positions 8 --restarts 3 --top 2
# #1 order I II III pos MKL plugs AV BS CG score -2791.1
#   THEWEATHERREPORT...

# Order search over a pool instead of a fixed order (M4 via --fourth Beta)
enigma solve-blind --pool I II III --input examples/blind_cipher.txt --lang en \
  --max-plugs 0 --top-positions 3 --top 1 --restarts 0
```

Flags: `--rotors` fixed order **or** `--pool` (+ optional `--fourth`),
`--rings`, `--reflector`, `--etw`, `--lang`, `--max-plugs` (`0` = positions
only), `--top-positions`, `--restarts`, `--seed`, `--top`, `--output-json`,
`--ring-scan 0,1` (same semantics as crib), `--checkpoint-file` (JSON resume
per finished pool order).
Progress prints live (`scan`/`climb`/`orders` counters on stderr).

Failures exit non-zero with a one-line `enigma: ...` message (duplicate
rotors, 11 plug pairs, crib longer than cipher, …).

## Configuration files

Machine setups live in TOML files instead of long command lines
(`examples/day.toml` is a working example). Precedence everywhere:
**flags > `--profile` > `[machine]`**.

```toml
[machine]                       # base setup
rotors = ["I", "II", "III"]
rings = "AAA"
positions = "AAA"
reflector = "B"
plugs = ""
etw = "identity"

[solver]                        # solve-command defaults
lang = "de"
top = 5

[profiles.naval]                # named overlay, inherits unset fields
rotors = ["Beta", "I", "II", "III"]
rings = "AAAA"
positions = "AAAA"
reflector = "Thin-B"
```

```sh
enigma encrypt --config examples/day.toml --text AAAAA
# BDZGO
enigma encrypt --config examples/day.toml --profile naval --text HELLOWORLD
# ILBDAAMTAZ
enigma encrypt --config examples/day.toml --pos AAB --text AAAAA
# flags win: different output than BDZGO
```

Solver flags (`--lang`, `--top`, `--max-plugs`, …) fall back to `[solver]`
the same way. Unknown profiles fail listing the known ones; anything still
missing after merging names the field.

```toml
[custom_rotors.coastal]
wiring = "QWERTYUIOPASDFGHJKLZXCVBNML"
notches = ["A"]
steps = true

[solver_presets.thorough]
max_plugs = 10
restarts = 8
```

- `[custom_rotors.NAME]` defines extra wheels usable anywhere a rotor name
  goes (historic names always win ties). Wiring must be a 26-letter
  permutation; notches are window letters; `steps = false` makes a
  Beta-style fixed wheel. CLI-only: the GUI/TUI forms accept historic names.
- `[solver_presets.NAME]` overlays `[solver]`; select with
  `--solver-preset thorough`.

Global text flag: `--transliterate` maps German umlauts before encrypting
(`ä→ae`, `ö→oe`, `ü→ue`, `ß→ss`), for encrypt/decrypt and solvers alike.
