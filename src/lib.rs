//! Pinch Points: a real-time grid routing puzzler.
//!
//! [`sim`] is the headless simulation core and [`app`] the Bevy shell around
//! it. The simulation is deliberately free of any engine types: plain Rust,
//! integer-only, and deterministic, so it can be unit-tested, replayed, and
//! driven over the wire. See `docs/pinch-points-spec.md`.
//!
//! There is almost no `unsafe` here and there is not going to be more: the
//! game is a grid of integers, and the one thing it does with bytes a
//! stranger wrote is decode them.
//!
//! The one exception is `app::keymap::os`, where asking Windows and macOS
//! what the keys say means calling their keyboard APIs: a dozen lines of
//! FFI with a reason written over each of them. That module allows itself
//! what this line denies, which is why it is `deny` rather than `forbid`;
//! an `#[allow(unsafe_code)]` anywhere else is a change to argue with.
#![deny(unsafe_code)]
// A public item pointing at a private one is the useful direction in this
// crate: these docs are read by whoever is working on the game, in an
// editor where the link resolves, and almost everything worth pointing at
// is internal. Demoting a dozen correct cross-references to plain text
// would cost the reader something to satisfy a rendering this crate does
// not publish.
//
// A link that resolves to *nothing* is the rot this lint gets confused
// with, and it is a different thing: `app::keymap` pointed at a module
// that had moved and said so in a warning nobody was reading, because
// `cargo doc` is not part of the build and its output was thirteen lines
// long. That one is denied now, so the next stale link fails `cargo doc`
// instead of hiding in the noise of the ones that are fine.
#![allow(rustdoc::private_intra_doc_links)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod app;
pub mod gif;
pub mod highlight;
pub mod lzw;
pub mod share;
pub mod sim;
pub mod transport;
