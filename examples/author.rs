//! Level-authoring helper: for each file argument, parse the level, report
//! what the solver finds within the post inventory, and replay the unposted
//! board so the necessity rule is visible while designing.
//!
//! Run: `cargo run --release --example author -- levels/21_log_ride.txt`.
//! Release, and not by preference: a debug solver is tens of times slower,
//! and the search is exponential in the post count.
//!
//! Searches under [`DEFAULT_NODE_BUDGET`] rather than exhaustively, because
//! that ceiling *is* the shipping constraint: `tests/it/campaign.rs` proves
//! minimality under it, and the editor gives up there too. A level the
//! budget cannot answer is a level that cannot ship, so hearing "gave up"
//! while authoring is the useful answer and not a limitation. Pass
//! `--exhaustive` to wait for the truth instead.
//!
//! Files are searched in parallel: a batch of candidates is the everyday
//! way this is used and each one is independent. Half the cores by
//! default, because this is an authoring tool and not a batch job: it
//! runs for tens of minutes at a time and the machine it runs on belongs
//! to somebody who is also using it. `--lanes N` (or `--lanes=N`) sets it
//! explicitly, and `--lanes 0` means every core.
//!
//! Results print as they land rather than in the order given, above a
//! progress bar that counts the files off: a batch of a few hundred boards
//! can run for half an hour, and a bar is how you tell "still solving" from
//! "wedged" while the next slow level holds its lane. Each report is written
//! in one `println` on the bar, so two lanes cannot interleave halfway
//! through a level and the bar is never left corrupted by a stray line.

use indicatif::{ProgressBar, ProgressStyle};
use pinch_points::sim::{
    DEFAULT_NODE_BUDGET, Effort, Level, PuzzleOutcome, SolveOutcome, solve_with,
};

fn unposted_outcome(level: &Level) -> (PuzzleOutcome, u64) {
    let mut board = level.board();
    loop {
        board.tick_idle();
        match level.outcome(&board) {
            PuzzleOutcome::Running => {}
            done @ (PuzzleOutcome::Won | PuzzleOutcome::Lost) => return (done, board.ticks()),
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let effort = match args.iter().any(|a| a == "--exhaustive") {
        true => Effort::Exhaustive,
        false => Effort::Budget(DEFAULT_NODE_BUDGET),
    };
    // `--lanes` takes its number either glued on with `=` or as the next
    // argument; in the second form that argument is the lane count and not
    // a level file, so it is spoken for before the paths are gathered.
    let mut asked: Option<usize> = None;
    let mut paths: Vec<&String> = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if let Some(n) = arg.strip_prefix("--lanes=") {
            asked = n.parse().ok();
        } else if arg == "--lanes" {
            asked = rest.next().and_then(|n| n.parse().ok());
        } else if !arg.starts_with("--") {
            paths.push(arg);
        }
    }
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let lanes = match asked {
        Some(0) => cores,
        Some(n) => n.min(cores),
        None => (cores / 2).max(1),
    };
    eprintln!("{} boards on {lanes} of {cores} cores", paths.len());
    // The bar owns the terminal's bottom line; every report goes out through
    // `bar.println`, which lifts the bar, writes the lines, and redraws it,
    // so the lanes cannot interleave and the bar is never scribbled over.
    let bar = ProgressBar::new(paths.len() as u64);
    bar.set_style(
        ProgressStyle::with_template(
            "{spinner} [{elapsed_precise}] {bar:40} {pos}/{len} boards  ETA {eta}",
        )
        .expect("progress template")
        .progress_chars("=> "),
    );
    std::thread::scope(|scope| {
        for lane in 0..lanes {
            let mine: Vec<&String> = paths.iter().copied().skip(lane).step_by(lanes).collect();
            let bar = &bar;
            scope.spawn(move || {
                for path in mine {
                    let line = report(path, effort);
                    // `suspend` prints to real stdout - lifting the bar first
                    // on a terminal, and simply running the closure when the
                    // output is redirected, where `bar.println` would drop
                    // the line into a hidden draw target and lose the report
                    // entirely. It also serialises the lanes, so two reports
                    // cannot interleave.
                    bar.suspend(|| {
                        use std::io::Write;
                        print!("{line}");
                        let _ = std::io::stdout().flush();
                    });
                    bar.inc(1);
                }
            });
        }
    });
    bar.finish_and_clear();
}

/// What one level file has to say for itself, as the lines to print.
///
/// Returned rather than printed so the lanes cannot interleave halfway
/// through a level's two lines.
fn report(path: &str, effort: Effort) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let text = std::fs::read_to_string(path).expect("readable level file");
    let level = match Level::parse(&text) {
        Ok(level) => level,
        Err(e) => {
            let _ = writeln!(out, "{path}: PARSE ERROR: {e}");
            return out;
        }
    };
    // Written out and read back before anything else is said about it. A
    // level that does not survive that is a level whose stored crabs are
    // not the crabs it starts with, and a recorded round stores its
    // starting board *as a level*, so the replay would diverge. It caught
    // `Castle On High`, whose crab faced a rock: the sim turns it on the
    // first tick, so the file said Right and the board meant Up.
    match Level::parse(&level.to_text()) {
        Ok(back) if back.board().state_hash() == level.board().state_hash() => {}
        Ok(_) => {
            let _ = writeln!(out, "{path}: ROUND TRIP: rebuilds a different board");
        }
        Err(e) => {
            let _ = writeln!(out, "{path}: ROUND TRIP: will not parse back: {e}");
        }
    }
    let (outcome, ticks) = unposted_outcome(&level);
    let _ = writeln!(out, "{path}: unposted -> {outcome:?} after {ticks} ticks");
    match solve_with(&level, effort) {
        SolveOutcome::Found(placements) => {
            let text: Vec<String> = placements
                .iter()
                .map(|(x, y, d)| format!("{x},{y} {d:?}"))
                .collect();
            let _ = writeln!(
                out,
                "  solver ({} posts): {}",
                placements.len(),
                text.join(" | ")
            );
        }
        SolveOutcome::Unsolvable => {
            let _ = writeln!(out, "  solver: NO SOLUTION within {} posts", level.posts);
        }
        // Not a failure of the tool. A board the shipped ceiling cannot
        // answer is one the minimality guard would fail on, so this is the
        // level saying it is too big rather than the search saying it is
        // too slow.
        SolveOutcome::GaveUp => {
            let _ = writeln!(
                out,
                "  solver: gave up at {DEFAULT_NODE_BUDGET} simulations, too big to ship"
            );
        }
    }
    out
}
