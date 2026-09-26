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
reflector, plugs, and entry wheel are fixed inputs.

```sh
enigma solve-crib --rotors I II III --crib MORGENGRAUEN \
  --input examples/crib_cipher.txt --lang de --top 3
# #1 order I II III pos KDO matches 12/12 score -543.0

enigma solve-crib --rotors I II III IV V --crib WETTER --crib-offset 4 \
  --input cipher.txt --checkpoint-file crib.json --output-json winners.json
```

Flags: `--rotors` pool (taken 3 per order), `--fourth` (`Beta`/`Gamma` for
M4), `--rings`, `--reflector`, `--plugs` (assumed known), `--etw`,
`--crib` (required), `--crib-offset` (default: scan every offset),
`--min-matches` (default: full crib length), `--top`, `--lang de|en`,
`--checkpoint-file` (JSON `{done_orders, best}`, written per finished order —
restart the same command to resume), `--output-json` (pretty winners file).

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
only), `--top-positions`, `--restarts`, `--seed`, `--top`, `--output-json`.
Progress prints live (`scan`/`climb`/`orders` counters on stderr).

Failures exit non-zero with a one-line `enigma: ...` message (duplicate
rotors, 11 plug pairs, crib longer than cipher, …).
