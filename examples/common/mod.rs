//! What the authoring and balance harnesses share: how `--lanes` is read,
//! and the bar that shows a long batch is still moving.
//!
//! Two of them once carried their own copy of each, and the copies had
//! drifted: one tool defaulted to half the cores and the other to all of
//! them, and an unreadable `--lanes=abc` meant one thing here and another
//! there. One reading now, with the default the caller's business.

#![allow(dead_code)]

use indicatif::{ProgressBar, ProgressStyle};

/// The thread count a command line asks for: `--lanes N` or `--lanes=N`,
/// where `0` means every core. Absent, or unreadable, it is `default`,
/// given the core count so a tool can say "half of them" or "all".
/// Never more lanes than cores: a lane with no core is a queue.
pub fn lanes(args: &[String], default: impl Fn(usize) -> usize) -> usize {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut rest = args.iter();
    let mut asked: Option<Option<usize>> = None;
    while let Some(arg) = rest.next() {
        if let Some(n) = arg.strip_prefix("--lanes=") {
            asked = Some(n.parse().ok());
        } else if arg == "--lanes" {
            asked = Some(rest.next().and_then(|n| n.parse().ok()));
        }
    }
    match asked {
        Some(Some(0)) => cores,
        Some(Some(n)) => n.min(cores),
        Some(None) | None => default(cores).clamp(1, cores),
    }
}

/// The arguments that are not flags: the files a tool was pointed at.
/// `--lanes N` spends its number, so that never comes back as a path.
pub fn paths(args: &[String]) -> Vec<&String> {
    let mut rest = args.iter();
    let mut paths = Vec::new();
    while let Some(arg) = rest.next() {
        if arg == "--lanes" {
            rest.next();
        } else if !arg.starts_with("--") {
            paths.push(arg);
        }
    }
    paths
}

/// A bar counting `total` of `unit` off, with an ETA. The bar owns the
/// terminal's bottom line; a report goes out through `bar.println` or
/// `bar.suspend`, which lift it, write, and redraw, so parallel lanes
/// cannot interleave and the bar is never scribbled over.
pub fn bar(total: u64, unit: &str) -> ProgressBar {
    let bar = ProgressBar::new(total);
    bar.set_style(
        ProgressStyle::with_template(&format!(
            "{{spinner}} [{{elapsed_precise}}] {{bar:40}} {{pos}}/{{len}} {unit}  ETA {{eta}}"
        ))
        .expect("progress template")
        .progress_chars("=> "),
    );
    bar
}
