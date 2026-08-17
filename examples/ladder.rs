//! Difficulty harness: does a level actually beat the level below it?
//!
//! Two of each side, seats mirrored across a second run so the map's own
//! corner bias cancels out, and the whole thing averaged over seeds. Run:
//! `cargo run --release --example ladder` (ROUNDS=n to change the sample).
//!
//! Also reports what each level does with its turns, because "how often it
//! thinks" and "how well it plays" are not the same number: a bot that
//! places a fourth signpost inside ten seconds evicts its own oldest, so
//! more decisions can mean less board control.
use indicatif::{ProgressBar, ProgressStyle};
use pinch_points::sim::{
    BotLevel, MAX_PLAYERS, PlayerAction, bot_action, classic_arena_seeded, generate_arena,
};

#[derive(Default)]
struct Tally {
    score: f64,
    placed: f64,
    evicted: f64,
}

/// One round of `levels`, reporting each seat's score, placements, and how
/// many of those placements destroyed a live signpost of its own.
///
/// An eviction is a placement the board accepts on a bare tile while the
/// seat already stands at its cap: re-pointing a post of its own does not
/// count (the count is unchanged, nothing is lost) and neither does a
/// placement the board turns down. Whether the board accepted it is read
/// after the tick, from the post standing where the bot aimed.
fn round(
    mut board: pinch_points::sim::Board,
    levels: [BotLevel; MAX_PLAYERS],
) -> [Tally; MAX_PLAYERS] {
    let mut out: [Tally; MAX_PLAYERS] = Default::default();
    let (cap, _) = board.signpost_rule();
    while !board.round_over() {
        let mut actions = [PlayerAction::None; MAX_PLAYERS];
        let mut would_evict = [None; MAX_PLAYERS];
        for seat in 0..MAX_PLAYERS {
            let action = bot_action(&board, seat as u8, levels[seat]);
            if let PlayerAction::Place { x, y, .. } = action {
                out[seat].placed += 1.0;
                let at_cap = board.signpost_count(seat as u8) >= usize::from(cap);
                if at_cap && board.signpost_at(x, y).is_none() {
                    would_evict[seat] = Some((x, y));
                }
            }
            actions[seat] = action;
        }
        board.tick(&actions);
        for (seat, aimed) in would_evict.into_iter().enumerate() {
            let landed = aimed.is_some_and(|(x, y)| {
                board
                    .signpost_at(x, y)
                    .is_some_and(|sp| usize::from(sp.owner) == seat)
            });
            if landed {
                out[seat].evicted += 1.0;
            }
        }
    }
    for (seat, tally) in out.iter_mut().enumerate() {
        tally.score = f64::from(board.scores()[seat]);
    }
    out
}

/// `a` on two seats against `b` on the other two, both ways round. `bar` is
/// ticked once per board played out, so one bar spans every pairing.
fn duel(a: BotLevel, b: BotLevel, rounds: u64, bar: &ProgressBar) -> (f64, f64, [Tally; 2]) {
    let (mut a_total, mut b_total) = (0.0, 0.0);
    let mut habits: [Tally; 2] = Default::default();
    for seed in 0..rounds {
        for swapped in [false, true] {
            let (lo, hi) = if swapped { (b, a) } else { (a, b) };
            let mut levels = [lo; MAX_PLAYERS];
            levels[2] = hi;
            levels[3] = hi;
            for board in [
                classic_arena_seeded(0xB0A7 ^ seed, false, 4),
                generate_arena(0xF00D ^ seed, 4, 12, 9),
            ] {
                let tally = round(board, levels);
                bar.inc(1);
                let low: f64 = tally[0].score + tally[1].score;
                let high: f64 = tally[2].score + tally[3].score;
                if swapped {
                    a_total += high;
                    b_total += low;
                } else {
                    a_total += low;
                    b_total += high;
                }
                // Habits are recorded for `a` and `b` from whichever pair
                // they held this run.
                let (a_pair, b_pair) = if swapped {
                    ([2, 3], [0, 1])
                } else {
                    ([0, 1], [2, 3])
                };
                for (slot, seats) in [(0usize, a_pair), (1, b_pair)] {
                    for seat in seats {
                        habits[slot].placed += tally[seat].placed;
                        habits[slot].evicted += tally[seat].evicted;
                    }
                }
            }
        }
    }
    (a_total, b_total, habits)
}

fn main() {
    let rounds: u64 = std::env::var("ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);
    let name = |l: BotLevel| match l {
        BotLevel::Easy => "easy",
        BotLevel::Normal => "normal",
        BotLevel::Hard => "fierce",
    };
    println!(
        "Head to head, seats mirrored, {} rounds a pairing.\n",
        rounds * 4
    );
    let pairings = [
        (BotLevel::Easy, BotLevel::Normal),
        (BotLevel::Normal, BotLevel::Hard),
        (BotLevel::Easy, BotLevel::Hard),
    ];
    // Every pairing plays `rounds * 4` boards (two swap orders, two maps),
    // and each is a full round out; a bar over the lot is how a run at a
    // large ROUNDS shows it is still going rather than hung. Result lines
    // print above it as each pairing finishes.
    let bar = ProgressBar::new(pairings.len() as u64 * rounds * 4);
    bar.set_style(
        ProgressStyle::with_template(
            "{spinner} [{elapsed_precise}] {bar:40} {pos}/{len} rounds  ETA {eta}",
        )
        .expect("progress template")
        .progress_chars("=> "),
    );
    for (a, b) in pairings {
        let (lo, hi, habits) = duel(a, b, rounds, &bar);
        let per = (rounds * 8) as f64; // seats-worth of rounds per side
        // Through `suspend` onto real stdout: it lifts the bar first on a
        // terminal, and just prints when the output is redirected, where
        // `bar.println` would drop the line into a hidden draw target -
        // these result lines are the whole point of the run and must survive
        // a `> ladder.txt`.
        bar.suspend(|| {
            println!(
                "{:>6} vs {:<6} {:5.1}% / {:5.1}%   placements {:5.0} / {:5.0}   \
                 self-evictions {:5.0} / {:5.0}",
                name(a),
                name(b),
                lo / (lo + hi) * 100.0,
                hi / (lo + hi) * 100.0,
                habits[0].placed / per,
                habits[1].placed / per,
                habits[0].evicted / per,
                habits[1].evicted / per,
            )
        });
    }
    bar.finish_and_clear();
}
