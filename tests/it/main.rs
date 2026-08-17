//! Every integration test, in one binary.
//!
//! One file per subject as before, but as modules of a single test target
//! rather than six of them. Cargo builds a separate binary for each root
//! under `tests/`, and each one statically links the whole of Bevy: six
//! roots meant six near-identical multi-gigabyte links, which is most of
//! what the test build spent its time and its disk on. It is also why CI
//! ran out of disk mid-link.
//!
//! Splitting by file is still the right way to read these; it just does
//! not need to be a split by *crate*. See matklad's "Delete Cargo
//! Integration Tests" for the longer argument.
//!
//! Note for anyone reaching for a single suite: the target is named `it`,
//! so it is `cargo test --test it campaign` rather than `--test campaign`.

mod bots;
mod campaign;
mod determinism;
mod format;
mod invariants;
mod online;
