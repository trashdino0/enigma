# GUI guide (`enigma-gui` / EnigmaSaurus)

Launch with `cargo run --release -p enigma-gui` (or the `enigma-gui.exe`
from the Releases page). Jungle-themed workbench, three tabs. Hover any
field for a plain-language explanation.

## Machine tab

Left panel: rotor slots 1–3 (`I`–`VIII`), optional 4th (`Beta`/`Gamma` for M4,
needs a thin reflector), reflector and entry-wheel dropdowns, rings,
positions, and plugs (`AV BS CG`, empty = unpatched). **Load machine**
validates everything — contradictions appear as a red inline error.

Typing area: input box, output box, big rotor-window letters, and the
per-stage signal path (`IN → plug → ETW → rotors → reflector → back → out`)
for the last keypress. Typing one letter enciphers once; deletes, pastes, and
mid-text edits rewind to the start positions and replay (rotor stepping is
one-way, like the real machine). **Clear** empties the buffers, **Copy**
copies the output, **🎲** rolls random start positions.

## Crib solver tab

Form: rotor pool (permuted, taken 3 per order), M4 fourth, rings, reflector,
assumed-known plugs, entry wheel, language, ciphertext and crib boxes, crib
offset (empty scans every offset), winner count. **Start search** runs on a
background thread: progress bar counts finished rotor orders, the results
table fills live, and clicking a row shows a decrypt preview. **Fill demo
🦕** loads a German message + crib that always cracks. See
[solvers](solvers.md) for what the settings mean.

## Blind solver tab

Form: pool, fourth, rings, reflector, entry wheel, language, ciphertext, plug
cap (`0` = positions only), top positions per order, restarts, seed, winner
count. **Fill demo 🦕** loads an English hunt with known-good settings.
Progress shows scan/climb/order counters; winners list order,
positions, plugs, score, and a plaintext preview. Ciphertexts shorter than 25
letters are rejected; ~150+ characters give reliable results. Short ciphers
get an amber warning: few letters plus a high plug cap invents plugboards
that outscore the truth.

Quitting the app ends any running search. Resume long crib searches through
the CLI `--checkpoint-file` instead.

## Files: configs and messages

- **Load config…** (machine tab, both solver tabs) reads a TOML setup file
  into the form — see [CLI configuration files](cli.md#configuration-files)
  for the format. Solver tabs split a leading Beta/Gamma into the M4 fourth.
- **Save config…** (machine tab) writes the current form as `[machine]` TOML.
- **Save output…** (machine tab) writes the output pane to a `.txt` file;
  **Save winner…** (solver tabs) writes the selected winner's full plaintext.
