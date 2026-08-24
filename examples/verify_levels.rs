//! Campaign minimality proof: no shipped level grants a signpost it does
//! not need. For every campaign level, the cheapest solution the solver
//! finds within the post inventory must use *every* post the level hands
//! out — a level beatable with fewer was mis-tuned, and one of its posts is
//! scenery the player is invited to waste.
//!
//! Run: `cargo run --release --example verify_levels`. **Release, and not by
//! preference**: this is an exhaustive-per-level search and a debug solver
//! is tens of times slower (the whole run is `Board::tick`, ~86% by perf) —
//! a minute in release across every core, far more unoptimised. It lived as
//! `#[ignore]`d test `no_campaign_level_grants_a_post_it_does_not_need`, run
//! in debug, which is the cost this binary exists to shed.
//!
//! It is deliberately not in CI: a minute of solving on every push is too
//! much for a check on data that changes far less often than code. Run it by
//! hand after editing a level file; that is the moment its answer can change.
//!
//! The proof is under [`DEFAULT_NODE_BUDGET`] rather than exhaustive,
//! because that ceiling *is* the shipping constraint: the editor gives up
//! there too, so a level the budget cannot answer is a level that cannot
//! ship. `GaveUp` is therefore a failure here, not an inconclusive result.
//! It doubles as the calibration check for the budget: nothing else searches
//! a real level under the shipped ceiling.
//!
//! It reads no arguments but `--lanes`: the levels are independent and share
//! no state, so the run is split across the cores the machine admits to
//! (a batch job, unlike `author`, which leaves half the machine for the
//! person authoring on it). `--lanes N` (or `--lanes=N`) pins the count;
//! `--lanes 0` means every core, which is the default here anyway.
//!
//! A progress bar counts the levels as they clear, so a run of a minute or
//! more is not a silent terminal; a level that fails prints above the bar as
//! it lands. Exits non-zero, and names every offending level, if any post
//! proves unnecessary or any level slips past the budget.

mod common;

use pinch_points::sim::{
    DEFAULT_NODE_BUDGET, Effort, Level, SolveOutcome, campaign_levels, solve_with,
};
use std::sync::Mutex;

/// Level 1 is the controls tutorial, and its blurb reads "place a signpost
/// with the arrow keys". The crab walks home unaided on purpose: the post is
/// there to be practised with on a board you cannot lose, which is the one
/// good reason to hand over a post the level does not need.
const TUTORIAL: &str = "Welcome Ashore";

/// Prove one level, or say why it failed. `None` is a pass.
fn complaint(index: usize, level: &Level) -> Option<String> {
    let n = index + 1;
    // Budgeted rather than exhaustive, and it still proves minimality: a
    // search reports GaveUp before it advances a depth, so a Found at depth
    // d means depths 0..d really were searched to the end. Running it under
    // the shipped ceiling costs nothing extra and pins that ceiling too.
    match solve_with(level, Effort::Budget(DEFAULT_NODE_BUDGET)) {
        SolveOutcome::Found(cheapest) if cheapest.len() == level.posts as usize => None,
        SolveOutcome::Found(cheapest) => Some(format!(
            "level {n} {:?} grants {} posts but falls to {}: {cheapest:?}",
            level.name,
            level.posts,
            cheapest.len(),
        )),
        SolveOutcome::Unsolvable => Some(format!(
            "level {n} {:?}: no signpost set within its inventory beats it",
            level.name
        )),
        SolveOutcome::GaveUp => Some(format!(
            "level {n} {:?} no longer fits DEFAULT_NODE_BUDGET \
             ({DEFAULT_NODE_BUDGET} simulations), so the editor would give up \
             on a shipped level",
            level.name
        )),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Every core by default: a batch job, run when the machine is spare.
    let lanes = common::lanes(&args, |cores| cores);
    let levels = campaign_levels();
    // The tutorial is exempt (its granted post is deliberate scenery), and a
    // one-post level has nothing to prove minimal: zero posts is already
    // pinned to lose by `granted_signposts_are_necessary`.
    let work: Vec<(usize, &Level)> = levels
        .iter()
        .enumerate()
        .filter(|(_, level)| level.name != TUTORIAL && level.posts >= 2)
        .collect();
    let total = work.len() as u64;
    let skipped = levels.len() - work.len();
    println!(
        "verifying {total} of {} campaign levels across {lanes} lane(s)",
        levels.len()
    );
    // The rest need no search: the tutorial grants its post on purpose, and
    // a level with zero or one post is already pinned by the fast tests -
    // `granted_signposts_are_necessary` proves zero posts lose and
    // `every_campaign_level_is_solvable_with_its_solution` proves one wins,
    // so its minimality is settled without an exhaustive search here.
    println!(
        "  ({skipped} skipped: the tutorial, and every 0- and 1-post level, \
         are covered by the fast tests)"
    );

    // One bar for the whole run, ticked as each level clears. It counts
    // levels, not solver nodes: the levels are wildly uneven (a four-post
    // board is minutes, a two-post one is instant), so the bar is honest
    // about how many are done and no more - the ETA it shows is indicatif's,
    // and it wanders while a slow level holds the count still. A failing
    // level prints its complaint above the bar via `println`, so the bar is
    // not corrupted by a stray line and the failures are visible as they land.
    let bar = common::bar(total, "levels");

    let complaints: Mutex<Vec<String>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for lane in 0..lanes {
            // Every lane strides the work list, so a run of hard levels sitting
            // together does not fall on one lane while the others go idle.
            let mine: Vec<_> = work.iter().skip(lane).step_by(lanes).copied().collect();
            let complaints = &complaints;
            let bar = &bar;
            scope.spawn(move || {
                for (i, level) in mine {
                    if let Some(line) = complaint(i, level) {
                        let mut out = complaints.lock().expect("complaint lock");
                        // Through `suspend` so a failure shows the moment it
                        // lands even when the run is redirected to a file
                        // (`bar.println` would drop it into a hidden target);
                        // the same line is also kept for the final roll-up
                        // below, which is what the exit code and a piped run
                        // ultimately rely on.
                        bar.suspend(|| eprintln!("FAIL  {line}"));
                        out.push(line);
                    }
                    bar.inc(1);
                }
            });
        }
    });
    bar.finish_and_clear();

    let mut complaints = complaints.into_inner().expect("complaint lock");
    complaints.sort();
    if complaints.is_empty() {
        println!("all {total} levels grant exactly the posts they need");
    } else {
        eprintln!(
            "\n{} level(s) grant a post they do not need:",
            complaints.len()
        );
        for line in &complaints {
            eprintln!("  {line}");
        }
        std::process::exit(1);
    }
}
