//! Pinch Points: a real-time grid routing puzzler.
//!
//! [`sim`] is the headless simulation core and [`app`] the Bevy shell around
//! it. The simulation is deliberately free of any engine types: plain Rust,
//! integer-only, and deterministic, so it can be unit-tested, replayed, and
//! driven over the wire. See `docs/pinch-points-spec.md`.
//!
//! There is no `unsafe` here and there is not going to be: the game is a
//! grid of integers, and the one thing it does with bytes a stranger wrote,
//! decoding them, is the last place to want manual memory handling.
//! Forbidden rather than merely denied, so it cannot be turned back on
//! locally without saying so here.
#![forbid(unsafe_code)]

pub mod app;
pub mod gif;
pub mod highlight;
pub mod lzw;
pub mod share;
pub mod sim;
pub mod transport;
