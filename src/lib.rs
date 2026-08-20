//! Pinch Points: a real-time grid routing puzzler.
//!
//! [`sim`] is the headless simulation core and [`app`] the Bevy shell around
//! it. The simulation is deliberately free of any engine types: plain Rust,
//! integer-only, and deterministic, so it can be unit-tested, replayed, and
//! driven over the wire. See `docs/pinch-points-spec.md`.
//!
//! There is almost no `unsafe` here and there is not going to be more: the
//! game is a grid of integers, and the one thing it does with bytes a
//! stranger wrote, decoding them, is the last place to want manual memory
//! handling.
//!
//! The one exception is [`app::keymap`], where asking Windows and macOS
//! what the keys say means calling their keyboard APIs: a dozen lines of
//! FFI with a reason written over each of them. That module allows itself
//! what this line denies, which is the only way in - `deny` rather than
//! `forbid` for exactly that reason, and a `#[allow(unsafe_code)]`
//! anywhere else is a change to argue with, not to wave through.
#![deny(unsafe_code)]

pub mod app;
pub mod gif;
pub mod highlight;
pub mod lzw;
pub mod share;
pub mod sim;
pub mod transport;
