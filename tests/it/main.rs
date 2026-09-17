//! Every integration test, in one binary.
//!
//! One file per subject, as modules of a single test target rather than six
//! of them. Cargo builds a separate binary for each root under `tests/`,
//! and each statically links the whole of Bevy: six roots meant six
//! near-identical multi-gigabyte links, and CI running out of disk
//! mid-link. See matklad's "Delete Cargo Integration Tests".
//!
//! Note for anyone reaching for a single suite: the target is named `it`,
//! so it is `cargo test --test it campaign` rather than `--test campaign`.

mod bots;
mod campaign;
mod determinism;
mod format;
mod invariants;
mod online;
mod shelves;
mod tide;
mod update_check;
