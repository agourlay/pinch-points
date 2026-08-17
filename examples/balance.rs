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
fn tally(label: &str, count: u64, games: impl Iterator<Item = [u32; MAX_PLAYERS]>, seats: usize) {
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
    // only worth reading beside its standard error: a five percent gap on
    // a few hundred games is usually nothing. Sigma is how far each seat
    // sits from the table average in units of that error; anything past
    // about two is worth investigating, anything under is noise.
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
}

fn main() {
    probe(classic_arena(false, 2), 2);
    probe(classic_arena(false, 4), 4);
    tally(
        "classic 2p x100 warmups",
        100,
        (0..100).map(|k| play_after(classic_arena(false, 2), 2, k)),
        2,
    );
    tally(
        "classic 4p x100 warmups",
        100,
        (0..100).map(|k| play_after(classic_arena(false, 4), 4, k)),
        4,
    );
    tally(
        "classic 2p x100 seeds",
        100,
        (0..100u64).map(|seed| play(classic_arena_seeded(seed, false, 2), 2)),
        2,
    );
    tally(
        "classic 4p x100 seeds",
        100,
        (0..100u64).map(|seed| play(classic_arena_seeded(seed, false, 4), 4)),
        4,
    );
    if std::env::var("BALANCE_FULL").is_err() {
        return;
    }
    println!("classic 2p: {:?}", play(classic_arena(false, 2), 2));
    println!("classic 4p: {:?}", play(classic_arena(false, 4), 4));
    // The generated-map seat spread is the one balance number still moving,
    // so it gets the sample size that makes it readable (see `tally`).
    tally(
        "generated 4p 12x9",
        3000,
        (0..3000u64).map(|seed| play(generate_arena(seed, 4, 12, 9), 4)),
        4,
    );
    tally(
        "generated 2p 12x9",
        300,
        (0..300u64).map(|seed| play(generate_arena(seed, 2, 12, 9), 2)),
        2,
    );
    tally(
        "generated 4p 16x11",
        200,
        (0..200u64).map(|seed| play(generate_arena(seed, 4, 16, 11), 4)),
        4,
    );
    // Six seats is four corners and two long-edge castles, which are not the
    // same job: this sweep is what says whether that difference shows up in
    // the score. The XL beach is the one a six-player match is played on, and
    // it generates one column wider than it is asked for so the edge castles
    // have a centre to share (see `castle_spots`).
    tally(
        "generated 6p 21x13",
        3000,
        (0..3000u64).map(|seed| play(generate_arena(seed, 6, 20, 13), 6)),
        6,
    );
}
