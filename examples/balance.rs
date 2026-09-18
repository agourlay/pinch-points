//! Fairness harness: identical bots on every seat. Any consistent seat
//! advantage is map/rules bias, not skill. Run: cargo run --example balance

use indicatif::{ProgressBar, ProgressStyle};
use pinch_points::sim::{
    Board, BotLevel, MAX_PLAYERS, PlayerAction, bot_action, classic_arena, classic_arena_seeded,
    generate_arena,
};

/// A bar for one sweep of `n` games, labelled with what is being swept. The
/// big generated-map sweeps are thousands of full rounds and a minute of
/// otherwise-silent play; a bar is how the harness shows it is still going.
fn sweep_bar(label: &str, n: u64) -> ProgressBar {
    let bar = ProgressBar::new(n);
    bar.set_style(
        ProgressStyle::with_template("{msg:26} [{elapsed_precise}] {bar:30} {pos}/{len}")
            .expect("progress template")
            .progress_chars("=> "),
    );
    bar.set_message(label.to_owned());
    bar
}

/// Play a round out with identical bots on every seat, calling `each_tick`
/// after every step.
fn play_out(mut board: Board, seats: u8, mut each_tick: impl FnMut(&Board)) -> Board {
    while !board.round_over() {
        let mut actions = [PlayerAction::None; MAX_PLAYERS];
        for seat in 0..seats {
            actions[seat as usize] = bot_action(&board, seat, BotLevel::Normal);
        }
        board.tick(&actions);
        each_tick(&board);
    }
    board
}

fn play(board: Board, seats: u8) -> [u32; MAX_PLAYERS] {
    play_after(board, seats, 0)
}

/// The same round, started `warmup` idle ticks in: the spawners and the gull
/// timer are on a fixed schedule, so the offset varies the round without
/// varying the map.
fn play_after(mut board: Board, seats: u8, warmup: u32) -> [u32; MAX_PLAYERS] {
    for _ in 0..warmup {
        board.tick_idle();
    }
    *play_out(board, seats, |_| {}).scores()
}

fn probe(board: Board, seats: u8) {
    let mut raids = 0u32;
    let mut prev = *board.scores();
    let board = play_out(board, seats, |board| {
        let now = *board.scores();
        raids += now.iter().zip(prev).filter(|(n, p)| **n < *p).count() as u32;
        prev = now;
    });
    println!(
        "  {seats}p: scores {:?} spawned {} banked {} eaten {} raids {} gulls_now {}",
        board.scores(),
        board.crabs_spawned(),
        board.crabs_banked(),
        board
            .crabs_spawned()
            .saturating_sub(board.crabs_banked() + board.crabs().len() as u32),
        raids,
        board.gulls().len(),
    );
}

/// `count` is the number of games the iterator will yield, so the bar has a
/// length before the first slow round is played; it is trusted, not checked
/// against the iterator.
///
/// Returns the worst seat deviation seen, in units of standard error, so
/// the caller can hold it to a budget. Printed and dropped, it let a seat
/// handicap in the bot's blunder draw sit in every round ever played.
fn tally(
    label: &str,
    count: u64,
    games: impl Iterator<Item = [u32; MAX_PLAYERS]>,
    seats: usize,
) -> f64 {
    let mut wins = [0u32; MAX_PLAYERS];
    let mut ties = 0u32;
    let mut totals = [0f64; MAX_PLAYERS];
    let mut squares = [0f64; MAX_PLAYERS];
    let mut n = 0u32;
    let bar = sweep_bar(label, count);
    for scores in games {
        bar.inc(1);
        n += 1;
        let best = scores[..seats].iter().max().copied().unwrap_or(0);
        let top: Vec<usize> = (0..seats).filter(|&s| scores[s] == best).collect();
        if top.len() == 1 {
            wins[top[0]] += 1;
        } else {
            ties += 1;
        }
        for s in 0..seats {
            let score = f64::from(scores[s]);
            totals[s] += score;
            squares[s] += score * score;
        }
    }
    // The bar has served its purpose the moment the sweep is done; clear it
    // so the result line below owns the terminal.
    bar.finish_and_clear();
    // Round scores scatter hugely from seed to seed, so a seat average is
    // only worth reading beside its standard error: a five percent gap on a
    // few hundred games is usually nothing. Sigma is how far each seat sits
    // from the table average in units of that error, and anything past
    // about two is worth investigating.
    let n = f64::from(n.max(1));
    let mean = totals.iter().take(seats).sum::<f64>() / seats as f64 / n;
    print!("{label}: {n} games, ties {ties} | ");
    let mut worst: f64 = 0.0;
    for s in 0..seats {
        let avg = totals[s] / n;
        let error = ((squares[s] / n - avg * avg).max(0.0) / n).sqrt();
        let sigma = if error > 0.0 {
            (avg - mean) / error
        } else {
            0.0
        };
        worst = worst.max(sigma.abs());
        print!(
            "P{}: {} wins, avg {avg:.1} ({:+.1}%, {sigma:+.1}s) | ",
            s + 1,
            wins[s],
            100.0 * (avg - mean) / mean.max(1.0),
        );
    }
    println!("worst {worst:.1}s");
    worst
}

/// One sweep's result, and the drift it is allowed before the run fails.
struct Sweep {
    label: &'static str,
    worst: f64,
    /// `None` for the `classic` sweeps, which are reported and never gated:
    /// they play a single handmade board a hundred times with only the
    /// warm-up offset or the seed varying, so their sigmas swing several
    /// points between runs that change nothing they measure.
    budget: Option<f64>,
}

/// Run one sweep and record it against its budget.
fn sweep(
    out: &mut Vec<Sweep>,
    label: &'static str,
    budget: Option<f64>,
    count: u64,
    games: impl Iterator<Item = [u32; MAX_PLAYERS]>,
    seats: usize,
) {
    let worst = tally(label, count, games, seats);
    out.push(Sweep {
        label,
        worst,
        budget,
    });
}

/// Fail the run if any gated sweep drifted past what it is allowed.
///
/// The point of running the harness unattended: read by hand on 2026-08-11
/// and not again until 2026-08-22, every figure had moved and one had gone
/// from 3.2 sigma to 5.9.
fn verdict(sweeps: &[Sweep]) {
    let over: Vec<(&str, f64, f64)> = sweeps
        .iter()
        .filter_map(|s| {
            s.budget
                .filter(|budget| s.worst > *budget)
                .map(|budget| (s.label, s.worst, budget))
        })
        .collect();
    if over.is_empty() {
        let gated = sweeps.iter().filter(|s| s.budget.is_some()).count();
        println!("\n{gated} gated sweep(s), all inside budget");
        return;
    }
    eprintln!("\n{} sweep(s) past the seat-drift budget:", over.len());
    for (label, worst, budget) in &over {
        eprintln!("  {label}: worst {worst:.1}s, budget {budget:.1}s");
    }
    std::process::exit(1);
}

/// Seat-drift budgets, in units of standard error, one per gated sweep.
///
/// The sweeps are not comparable to each other, which is why there is no
/// single number.
///
/// Measured, budgeted:
///
/// | sweep | 2026-08-22 | 2026-09-17 | 2026-09-18 | budget |
/// |---|---|---|---|---|
/// | generated 2p 12x9  | 0.3 | 0.7 | 1.2 | 2.0 |
/// | generated 4p 12x9  | 1.3 | 1.0 | 1.0 | 2.5 |
/// | generated 4p 16x11 | 1.2 | 1.3 | 1.0 | 3.0 |
/// | generated 6p 21x13 | 2.5 | 2.7 | 1.4 | 3.5 |
///
/// The 2026-09-18 column is a different experiment, not a better reading
/// of the old one: Right Claws put a ninth face on the roulette, so every
/// draw moved and all 3000 rounds of each sweep are different rounds. Read
/// it as a re-roll. What it says is that nothing about that change biased
/// a seat, and that the six-seat figure which had been sitting at 2.7 has
/// come back to the middle, which is what a run of noise does and what a
/// real asymmetry would not.
///
/// The six-seat budget is still the uncomfortable one and its headroom is
/// the thinnest on purpose: `tally` calls anything past about two worth
/// investigating. It is set to catch that figure getting worse, not to
/// bless where it stands, and one run at 1.4 does not retire it.
const BUDGET_2P_12X9: f64 = 2.0;
const BUDGET_4P_12X9: f64 = 2.5;
/// 200 games against the others' 3000, so the noisiest of the four.
const BUDGET_4P_16X11: f64 = 3.0;
const BUDGET_6P_21X13: f64 = 3.5;

fn main() {
    probe(classic_arena(false, 2), 2);
    probe(classic_arena(false, 4), 4);

    // Budgets are per sweep because the sweeps are not comparable: two
    // seats sit a third of a sigma apart and six sit two and a half, so one
    // global number would miss real drift at two seats or fail every night
    // at six.
    let mut sweeps: Vec<Sweep> = Vec::new();

    sweep(
        &mut sweeps,
        "classic 2p x100 warmups",
        None,
        100,
        (0..100).map(|k| play_after(classic_arena(false, 2), 2, k)),
        2,
    );
    sweep(
        &mut sweeps,
        "classic 4p x100 warmups",
        None,
        100,
        (0..100).map(|k| play_after(classic_arena(false, 4), 4, k)),
        4,
    );
    sweep(
        &mut sweeps,
        "classic 2p x100 seeds",
        None,
        100,
        (0..100u64).map(|seed| play(classic_arena_seeded(seed, false, 2), 2)),
        2,
    );
    sweep(
        &mut sweeps,
        "classic 4p x100 seeds",
        None,
        100,
        (0..100u64).map(|seed| play(classic_arena_seeded(seed, false, 4), 4)),
        4,
    );
    if std::env::var("BALANCE_FULL").is_err() {
        // The generated sweeps are the gated ones, so a short run has
        // nothing to rule on. Say so rather than printing a pass.
        println!("\nclassic sweeps only; set BALANCE_FULL=1 for the gated ones");
        return;
    }
    println!("classic 2p: {:?}", play(classic_arena(false, 2), 2));
    println!("classic 4p: {:?}", play(classic_arena(false, 4), 4));
    // The generated-map seat spread is the one balance number still moving,
    // so it gets the sample size that makes it readable (see `tally`).
    sweep(
        &mut sweeps,
        "generated 4p 12x9",
        Some(BUDGET_4P_12X9),
        3000,
        (0..3000u64).map(|seed| play(generate_arena(seed, 4, 12, 9), 4)),
        4,
    );
    sweep(
        &mut sweeps,
        "generated 2p 12x9",
        Some(BUDGET_2P_12X9),
        300,
        (0..300u64).map(|seed| play(generate_arena(seed, 2, 12, 9), 2)),
        2,
    );
    sweep(
        &mut sweeps,
        "generated 4p 16x11",
        Some(BUDGET_4P_16X11),
        200,
        (0..200u64).map(|seed| play(generate_arena(seed, 4, 16, 11), 4)),
        4,
    );
    // Six seats is four corners and two long-edge castles, which are not
    // the same job, and this sweep says whether that shows up in the score.
    // The XL beach generates one column wider than it is asked for so the
    // edge castles have a centre to share (see `castle_spots`).
    sweep(
        &mut sweeps,
        "generated 6p 21x13",
        Some(BUDGET_6P_21X13),
        3000,
        (0..3000u64).map(|seed| play(generate_arena(seed, 6, 20, 13), 6)),
        6,
    );

    verdict(&sweeps);
}
