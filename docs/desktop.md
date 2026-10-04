# Desktop app (`enigma-desktop` / EnigmaSaurus)

Tauri shell (Rust backend in `src-tauri/`) around a vanilla HTML/CSS/JS
frontend (`ui/` — no bundler, no Node). Same engine as everything else.

```sh
cargo install tauri-cli --locked   # once
cargo tauri dev                    # hot-reload while tweaking the UI
cargo tauri build --no-bundle      # exe in target/release/enigma-desktop.exe
```

## Tabs

- **Machine**: dropdowns for rotors/reflector/entry wheel, rings/positions/
  plugs fields, live typing with rotor windows and per-stage signal path.
  🎲 rolls random start positions; Copy/Save output export the text.
- **Crib hunt / Blind hunt**: forms, background workers with live progress,
  clickable winners with decrypt previews, Save winner export. **Fill demo**
  loads a guaranteed hunt. `Ctrl+Enter` starts a hunt; short ciphers get an
  amber overfitting warning.

First launch shows a 3-step tour (remembered in localStorage); the moon
button toggles jungle night / savanna day (persisted too).

## Backend notes

- Commands live in `src-tauri/src/commands.rs` (14 total): typing session,
  config/text files, native dialogs (paths only — bytes move through
  commands), crib/blind workers emitting progress events.
- The frontend talks through `window.__TAURI__`, enabled by
  `withGlobalTauri: true` in `tauri.conf.json` (Tauri v2 omits it by
  default — without it every button silently does nothing).
- `Load config` fills machine fields *and* `[solver]` fields (language,
  counts, seed). Named `[solver_presets]` stay CLI-only (`--solver-preset`).
- Icons come from `cargo tauri icon <png>`; the committed set was generated
  from a drawn PNG, no external assets.
