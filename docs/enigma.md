# The Enigma: history and how it works

## A one-minute history

The Enigma began as a **commercial product**: German engineer Arthur
Scherbius patented a rotor cipher machine in 1918 and sold it through the
1920s to businesses that wanted private correspondence. It was a commercial
flop — but several militaries noticed. Germany's army and navy adopted
upgraded versions from the late 1920s onward, adding the plugboard and more
rotors over time.

To send a message, both stations set their machines **identically** each day
according to secret key sheets: which rotors, in which order, ring settings,
start positions, and plugboard cables. The receiving operator set up the same
key and typed in the gibberish to get the plaintext back.

The machine's reputation for unbreakability did not survive contact with
mathematicians. Poland's Cipher Bureau — chiefly **Marian Rejewski** — broke
the system as early as 1932 by exploiting a procedural flaw (operators
transmitted each message key twice). When war loomed, Poland shared
everything with Britain and France. At **Bletchley Park**, Alan Turing and
Gordon Welchman industrialized the attack with the **Bombe**, an
electromechanical machine that tested rotor positions against a guessed
plaintext fragment ("crib") — exactly the attack this project's crib solver
reproduces in software. The 1942 introduction of the 4-rotor naval Enigma
(M4, for U-boat traffic) blacked out Allied codebreakers for months until new
methods and captured material closed the gap.

## How a keypress travels

Pressing a key sends current on a journey through the machine and back.
In order:

1. **Keyboard → plugboard.** Cables on the front panel swap up to 10 letter
   pairs (e.g. A↔V). Unplugged letters pass through unchanged.
2. **Entry wheel.** A fixed wiring that connects keys to the rotor stack
   (identity on military machines; QWERTZ order on commercial ones).
3. **Rotors, right to left.** Each rotor scrambles the signal through its
   internal wiring, shifted by its current position and ring setting.
4. **Reflector.** Sends the current back through the rotors by a *different*
   route. Because of it, encryption and decryption are the same operation —
   and no letter ever encrypts to itself (a famous flaw the codebreakers
   exploited).
5. **Back out** through rotors, entry wheel, and plugboard to light the lamp.

Crucially, the **rightmost rotor steps with every keypress**, occasionally
kicking its neighbor — including the famous *double-stepping* quirk where
the middle rotor moves twice in a row. So the scrambling changes with every
letter: typing `AAAAA` gives five different lamps (`BDZGO` on the classic
test setup).

## What each setting does

These are the pieces of the daily key — the same settings this app asks for:

| Setting | Historical name | What it does |
|---------|-----------------|--------------|
| Rotor order | *Walzenlage* | Which scrambling wheels are fitted, left to right (e.g. `I II III`). Naval M4 adds a thin 4th wheel (`Beta`/`Gamma`) that never steps. |
| Ring settings | *Ringstellung* | Rotates each wheel's wiring against its letter ring — a fine offset applied once, then left alone. |
| Start positions | *Grundstellung* | The letters in the little windows where typing begins. Changed per message, not just per day. |
| Reflector | *Umkehrwalze* | The turnaround wiring (`B`/`C`, thin versions for M4). Chosen rarely; `B` was standard for years. |
| Plugboard pairs | *Stecker* | The front-panel cables. Chosen daily, up to 10 pairs — the largest part of the keyspace. |
| Entry wheel | *Eintrittswalze* | Fixed per machine type; effectively never changed in the field. |
| Language | — | Not a machine setting, but solvers need to know what plaintext *looks like*: German was the historical default. |

A full M3 key (5 rotors to choose 3 from, rings, positions, 10 plugs) has on
the order of 10²⁰ possibilities — which is why brute force was hopeless and
the historical attacks, like the crib method, worked by *sidestepping* most
of that keyspace instead of searching it.
