# Enigma M3/M4 — encrypt messages like it's 1941

[![CI](https://github.com/trashdino0/enigma/actions/workflows/ci.yml/badge.svg)](https://github.com/trashdino0/enigma/actions/workflows/ci.yml)

The Enigma was the cipher machine behind Germany's WWII secret radio
traffic. This app recreates it faithfully on your computer: set it up exactly
like an operator would, type a message, and watch it turn into gibberish that
only someone with the same settings can decode. It can also *break* messages —
give it a ciphertext and it will hunt down the settings for you.

No technical knowledge needed. Everything below happens in a normal desktop
window with buttons and text boxes.

## Get the app

**Option A — download (easiest, once a release exists):** open the
[Releases page](https://github.com/trashdino0/enigma/releases), download
`enigma-gui.exe`, and double-click it. No installation.

**Option B — run from source:** install Rust once from
[rustup.rs](https://rustup.rs), then in this folder run:

```sh
cargo run --release -p enigma-gui
```

## Take the tour (5 minutes)

**1. Encrypt your first message.** Open the **Machine** tab. Everything is
already set to a valid machine, so just press **Load machine**, type `AAAAA`
in the input box, and watch the output read `BDZGO` while the little rotor
windows step forward with every letter. That exact result is even a published
historical test — your machine agrees with the real thing.

**2. Scramble it properly.** Add plugboard pairs `AV BS CG`, change positions
to `KDO`, press **Load machine** again, and type a sentence. Spaces pass
through untouched; every letter comes out different — and no letter ever
encrypts to itself, just like the original.

**3. Decrypt it back.** Encryption and decryption are the *same* operation
here. Load the exact same settings on any machine and type in the gibberish:
your message comes back out.

**4. Break a message.** Open the **Crib solver** tab. A "crib" is a guess at
part of the message — operators often guessed words like weather reports.
Copy the text from `examples/crib_cipher.txt` into the ciphertext box, type
`MORGENGRAUEN` as the crib, press **Start search**, and watch it recover the
full settings (`I II III`, positions `KDO`) in under a second.

**5. Break one with no clues.** Open the **Blind solver** tab, paste
`examples/blind_cipher.txt`, set language to English, press **Start search**.
With zero knowledge of the settings it finds the rotors, positions *and* the
plugboard (`AV BS CG`) and prints the whole decrypted message.

## Concepts in plain words

- **Rotors** — the scrambling wheels inside the machine (named I–VIII, plus
  Beta/Gamma for naval messages). Their *order* is part of the key.
- **Positions** — the letters showing in the little windows; this is where
  the wheels start, and they step as you type.
- **Rings** — a fine adjustment to each wheel, set once per day.
- **Plugs** — cables on the front panel that swap letter pairs before and
  after scrambling (up to 10).
- **Reflector** — the part that sends the signal back through the wheels,
  which is why encrypting and decrypting are the same action.

New to all this? Read [the Enigma: history and how it works](docs/enigma.md)
— a short history plus what every setting on the machine actually does.

## Something wrong?

- **The solver finds nothing** — it needs enough text (about a paragraph for
  blind mode) and, for crib mode, a correct guess. Check the language setting
  (`de` for German messages, `en` for English).
- **Red error in the machine tab** — the settings contradict each other
  (e.g. the same rotor twice). Read the message, fix the field, load again.
- **The app window won't open** — the 3D interface needs working graphics
  drivers; updating them fixes it in nearly all cases.

## For the technically curious

Detailed documentation lives in [`docs/`](docs/README.md): the
[GUI guide](docs/gui.md), the [command-line reference](docs/cli.md), how the
[solvers](docs/solvers.md) work, [manual testing](docs/manual-testing.md),
and [development](docs/development.md).

Licensed under MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`).
