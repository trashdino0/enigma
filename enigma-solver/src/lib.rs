//! Enigma M3/M4 solvers.
//!
//! - [`score`]: language scoring (German/English quadgrams, index of coincidence).
//! - [`crib`]: known-plaintext (Bombe-style) search over rotor orders x positions.
//! - [`hillclimb`]: ciphertext-only attack (position scan + plugboard hill-climb).
//!
//! The quadgram tables are embedded with `include_str!`, so solver binaries
//! stay single-file. Parsing them into the flat score array happens once per
//! [`score::QuadgramScorer`] construction — never in the search hot loop.

pub mod crib;
pub mod hillclimb;
pub mod score;
