//! Fairness harness: identical bots on every seat. Any consistent seat
//! advantage is map/rules bias, not skill. Run: cargo run --example balance

mod common;

use pinch_points::app::match_setup::{
    GullPressure, MapChoice, RoundLength, apply_terms, holds, versus_board,
};
use pinch_points::sim::{
    Board, BotLevel, MAX_PLAYERS, MAX_SIGNPOSTS_PER_PLAYER, Orientation, PlayerAction, bot_action,
    castle_spots, classic_arena, surprise_styles, versus_map,
};

/// One round played out: the scores, and the castle spot each seat played
/// from, as it stands on the board ([`spots_of`]) and as it stands from the
/// empty spot ([`from_the_gap`]).
#[derive(Clone, Copy)]
struct Game {
    scores: [u32; MAX_PLAYERS],
    spots: [usize; MAX_PLAYERS],
    from_gap: Option<[usize; MAX_PLAYERS]>,
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

fn play(board: Board, seats: u8) -> Game {
    play_after(board, seats, 0)
}

/// The same round, started `warmup` idle ticks in: the spawners and the gull
/// timer are on a fixed schedule, so the offset varies the round without
/// varying the map.
fn play_after(mut board: Board, seats: u8, warmup: u32) -> Game {
    let spots = spots_of(&board, seats);
    let from_gap = from_the_gap(&board, seats, spots);
    for _ in 0..warmup {
        board.tick_idle();
    }
    Game {
        scores: *play_out(board, seats, |_| {}).scores(),
        spots,
        from_gap,
    }
}

/// The castle spot each seat plays from, as an index into
/// [`castle_spots`].
///
/// By spot as well as by seat, because which seat sits where is drawn with
/// the beach: a seat's figure is every spot's averaged, and reads the same
/// however unfair one of them is.
fn spots_of(board: &Board, seats: u8) -> [usize; MAX_PLAYERS] {
    let all = castle_spots(board.width(), board.height());
    let mut spots = [usize::MAX; MAX_PLAYERS];
    for (seat, spot) in spots.iter_mut().enumerate().take(usize::from(seats)) {
        let at = board
            .castle_of(seat as u8)
            .expect("every seat has a castle");
        *spot = all
            .iter()
            .position(|&spot| spot == at)
            .expect("a castle on a castle spot");
    }
    spots
}

/// [`spots_of`] mirrored so the one empty spot of an odd table is always
/// the first corner, or the top edge's centre: `None` when the table
/// leaves no spot, or more than one, empty.
///
/// Which spot is left empty is drawn too, and the generated beaches mirror
/// both ways, so counted where it stands on the board, a castle is beside
/// the gap as often as any other, and a seat the gap favours evens out
/// just the same. Seen from the gap, "the corner beside it on its own
/// side" is one figure.
fn from_the_gap(
    board: &Board,
    seats: u8,
    spots: [usize; MAX_PLAYERS],
) -> Option<[usize; MAX_PLAYERS]> {
    let (width, height) = (board.width(), board.height());
    let all = castle_spots(width, height);
    let seated = &spots[..usize::from(seats)];
    // Four corners at four seats or fewer, the long edges' two besides.
    let in_play = if seats <= 4 { 4 } else { MAX_PLAYERS };
    let mut empty = (0..in_play).filter(|spot| !seated.contains(spot));
    let (Some(empty), None) = (empty.next(), empty.next()) else {
        return None;
    };
    let (ex, ey) = all[empty];
    let (tx, ty) = if empty < 4 { all[0] } else { all[4] };
    let (flip_x, flip_y) = (ex != tx, ey != ty);
    let mut mirrored = spots;
    for spot in &mut mirrored[..usize::from(seats)] {
        let (x, y) = all[*spot];
        let x = if flip_x { width - 1 - x } else { x };
        let y = if flip_y { height - 1 - y } else { y };
        *spot = all
            .iter()
            .position(|&spot| spot == (x, y))
            .expect("a castle spot's mirror is one");
    }
    Some(mirrored)
}

/// Play `count` rounds over every core, round `seed` built by `round`, in
/// seed order: the same games a single thread plays, a core's worth of
/// times sooner.
fn par(count: u64, round: impl Fn(u64) -> Game + Sync) -> std::vec::IntoIter<Game> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()) as u64;
    let next = std::sync::atomic::AtomicU64::new(0);
    let mut games: Vec<(u64, Game)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let seed = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if seed >= count {
                            return out;
                        }
                        out.push((seed, round(seed)));
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("a sweep thread"))
            .collect()
    });
    games.sort_by_key(|(seed, _)| *seed);
    games
        .into_iter()
        .map(|(_, game)| game)
        .collect::<Vec<_>>()
        .into_iter()
}

/// A versus round as a couch match deals it: the map's board for `seed`,
/// turned for the round ([`versus_board`]), under the default terms
/// (normal gulls, a three-minute round, three arrows evicting).
fn dealt(map: MapChoice, seed: u64, seats: u8) -> Board {
    let mut board = versus_board(map, seed, seats);
    apply_terms(
        &mut board,
        GullPressure::Normal,
        RoundLength::Standard,
        MAX_SIGNPOSTS_PER_PLAYER as u8,
    );
    board
}

/// A round on a handmade map, by castle as the map draws it: each seat's
/// spot is the number its castle has in the level file, found by turning
/// the drawn castles the way the round turned them. The deal and the
/// mirror make the seats fair by construction; the castles as drawn are
/// what a lopsided map would show in.
fn play_drawn(map: MapChoice, seed: u64, seats: u8) -> Game {
    let board = dealt(map, seed, seats);
    let level = versus_map(map.handmade().expect("a handmade map"));
    let drawn = level.table_board(seed, seats);
    let turn = Orientation::for_round(seed, drawn.width(), level.roll);
    let (w, h) = (drawn.width(), drawn.height());
    let mut spots = [usize::MAX; MAX_PLAYERS];
    for (seat, spot) in spots.iter_mut().enumerate().take(usize::from(seats)) {
        let at = board
            .castle_of(seat as u8)
            .expect("every seat has a castle");
        *spot = (0..seats)
            .position(|castle| {
                drawn
                    .castle_of(castle)
                    .is_some_and(|home| turn.place(home, w, h) == at)
            })
            .expect("a castle the map draws");
    }
    Game {
        scores: *play_out(board, seats, |_| {}).scores(),
        spots,
        from_gap: None,
    }
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

/// The running figures of one seat, or one spot.
#[derive(Clone, Copy, Default)]
struct Column {
    rounds: f64,
    total: f64,
    squares: f64,
    wins: u32,
}

impl Column {
    fn add(&mut self, score: u32, won: bool) {
        let score = f64::from(score);
        self.rounds += 1.0;
        self.total += score;
        self.squares += score * score;
        self.wins += u32::from(won);
    }
}

/// Print one row of columns against the table average, and return the
/// worst deviation, in units of standard error.
///
/// Round scores scatter hugely from seed to seed, so an average is only
/// worth reading beside its standard error: a five percent gap on a few
/// hundred games is usually nothing. Sigma is how far each column sits
/// from the table average in units of that error, and anything past about
/// two is worth investigating.
fn spread(name: &str, columns: &[Column]) -> f64 {
    let rounds: f64 = columns.iter().map(|c| c.rounds).sum();
    let mean = columns.iter().map(|c| c.total).sum::<f64>() / rounds.max(1.0);
    let mut worst: f64 = 0.0;
    for (i, column) in columns.iter().enumerate() {
        if column.rounds == 0.0 {
            continue;
        }
        let n = column.rounds;
        let avg = column.total / n;
        let error = ((column.squares / n - avg * avg).max(0.0) / n).sqrt();
        let sigma = if error > 0.0 {
            (avg - mean) / error
        } else {
            0.0
        };
        worst = worst.max(sigma.abs());
        print!(
            "{name}{}: {:.0}% wins, avg {avg:.1} ({:+.1}%, {sigma:+.1}s) | ",
            i + 1,
            100.0 * f64::from(column.wins) / n,
            100.0 * (avg - mean) / mean.max(1.0),
        );
    }
    println!("worst {worst:.1}s");
    worst
}

/// The worst drift a sweep showed, in units of standard error: by seat,
/// by castle spot, and by castle spot seen from the empty one (zero for a
/// table that leaves none).
#[derive(Clone, Copy)]
struct Drift {
    seat: f64,
    spot: f64,
    gap: f64,
}

/// `count` is the number of games the iterator will yield, so the bar has a
/// length before the first slow round is played; it is trusted, not checked
/// against the iterator.
///
/// Returns the worst deviations seen, so the caller can hold them to a
/// budget. Printed and dropped, the seat figure let a seat handicap in the
/// bot's blunder draw sit in every round ever played. All three are kept:
/// a seat's figure catches whatever is keyed to the seat (the AI's draws),
/// a spot's catches a castle the beach favours wherever the gap is, and
/// the gap's catches a castle favoured for where it stands beside it,
/// neither of which the seat figure can see once the seats are drawn.
fn tally(label: &str, count: u64, games: impl Iterator<Item = Game>, seats: usize) -> Drift {
    let mut by_seat = [Column::default(); MAX_PLAYERS];
    let mut by_spot = [Column::default(); MAX_PLAYERS];
    let mut by_gap = [Column::default(); MAX_PLAYERS];
    let mut ties = 0u32;
    let mut n = 0u32;
    // The big generated-map sweeps are thousands of full rounds and a
    // minute of otherwise-silent play; the bar, labelled with what is
    // being swept, is how the harness shows it is still going.
    let bar = common::bar(count, label);
    for Game {
        scores,
        spots,
        from_gap,
    } in games
    {
        bar.inc(1);
        n += 1;
        let best = scores[..seats].iter().max().copied().unwrap_or(0);
        let top = (0..seats).filter(|&s| scores[s] == best).count();
        ties += u32::from(top > 1);
        for s in 0..seats {
            let won = top == 1 && scores[s] == best;
            by_seat[s].add(scores[s], won);
            by_spot[spots[s]].add(scores[s], won);
            if let Some(from_gap) = from_gap {
                by_gap[from_gap[s]].add(scores[s], won);
            }
        }
    }
    // The bar has served its purpose the moment the sweep is done; clear it
    // so the result lines below own the terminal.
    bar.finish_and_clear();
    println!("{label}: {n} games, ties {ties}");
    print!("  by seat | ");
    let seat = spread("P", &by_seat[..seats]);
    print!("  by spot | ");
    let spot = spread("S", &by_spot);
    let gap = match by_gap.iter().any(|c| c.rounds > 0.0) {
        true => {
            print!("  by gap  | ");
            spread("G", &by_gap)
        }
        false => 0.0,
    };
    Drift { seat, spot, gap }
}

/// One sweep's result, and the drift it is allowed before the run fails.
struct Sweep {
    label: &'static str,
    drift: Drift,
    /// `None` for the `classic` two- and four-seat sweeps, which are
    /// reported and never gated: they play a single handmade board a
    /// hundred times with only the warm-up offset or the seed varying, so
    /// their sigmas swing several points between runs that change nothing
    /// they measure.
    budget: Option<Drift>,
}

/// Run one sweep and record it against its budget.
fn sweep(
    out: &mut Vec<Sweep>,
    label: &'static str,
    budget: Option<Drift>,
    count: u64,
    games: impl Iterator<Item = Game>,
    seats: usize,
) {
    let drift = tally(label, count, games, seats);
    out.push(Sweep {
        label,
        drift,
        budget,
    });
}

/// Fail the run if any gated sweep drifted past what it is allowed, by
/// seat or by spot.
///
/// The point of running the harness unattended: read by hand on 2026-08-11
/// and not again until 2026-08-22, every figure had moved and one had gone
/// from 3.2 sigma to 5.9.
fn verdict(sweeps: &[Sweep]) {
    let mut over: Vec<(&str, &str, f64, f64)> = Vec::new();
    for s in sweeps {
        let Some(budget) = s.budget else { continue };
        if s.drift.seat > budget.seat {
            over.push((s.label, "seat", s.drift.seat, budget.seat));
        }
        if s.drift.spot > budget.spot {
            over.push((s.label, "spot", s.drift.spot, budget.spot));
        }
        if s.drift.gap > budget.gap {
            over.push((s.label, "spot from the gap", s.drift.gap, budget.gap));
        }
    }
    if over.is_empty() {
        let gated = sweeps.iter().filter(|s| s.budget.is_some()).count();
        println!("\n{gated} gated sweep(s), all inside budget");
        return;
    }
    eprintln!("\n{} figure(s) past the drift budget:", over.len());
    for (label, by, worst, budget) in &over {
        eprintln!("  {label}, by {by}: worst {worst:.1}s, budget {budget:.1}s");
    }
    std::process::exit(1);
}

/// Drift budgets, in units of standard error, one per gated sweep: the
/// seat figure's history first, the spot figures' after it.
///
/// The sweeps are not comparable to each other, which is why there is no
/// single number.
///
/// Seat figures measured, budgeted:
///
/// | sweep | 2026-08-22 | 2026-09-17 | 2026-09-18 | 2026-10-02 | 2026-10-13 | budget |
/// |---|---|---|---|---|---|---|
/// | generated 2p 12x9  | 0.3 | 0.7 | 1.2 | 0.2 | 0.3 | 2.0 |
/// | generated 4p 12x9  | 1.3 | 1.0 | 1.0 | 0.7 | 0.7 | 2.5 |
/// | generated 4p 16x11 | 1.2 | 1.3 | 1.0 | 1.4 | 1.1 | 3.0 |
/// | generated 6p 21x13 | 2.5 | 2.7 | 1.4 | 1.2 | 1.2 | 3.5 |
///
/// The 2026-09-18 column is a different experiment, not a better reading
/// of the old one: Right Claws put a ninth face on the roulette, so every
/// draw moved and all 3000 rounds of each sweep are different rounds. Read
/// it as a re-roll. What it says is that nothing about that change biased
/// a seat, and that the six-seat figure which had been sitting at 2.7 has
/// come back to the middle, which is what a run of noise does and what a
/// real asymmetry would not.
///
/// The 2026-10-02 column is the AI walking under the fair cursor rule
/// (`sim::fair_walk`): every level at a person's pace, diagonals allowed,
/// where each used to walk at a pace of its own. Another re-roll, measured
/// beside the build before it on the same night (1.2, 1.0, 1.0 and 1.4),
/// and nothing about it biased a seat. Later that night the AI's hand
/// started each round at its castle rather than anywhere at once, and
/// the sweeps read 0.9, 1.5, 1.5 and 1.2: inside every budget, and no
/// seat favoured.
///
/// The 2026-10-13 column is three changes measured together: a table of
/// four or more always dealt a spawner hole for all but one player, the
/// holes that open together drawing which the crab cap leaves out
/// (`fair_spawns`), and the AI's moment to think in each window taking
/// the board's seed. The last is the one aimed at seats: drawn from the
/// window and the seat alone, every seat kept one schedule in every round
/// ever played, and the second seat's held it 2% to 4% behind on the
/// four-player classic beach whichever castle it sat at (4.5 sigma over
/// 10,000 rounds), which the classic sweeps here, at 100 rounds, are too
/// small to see. Nothing in it biased a seat on the generated beaches.
///
/// The six-seat budget is still the uncomfortable one and its headroom is
/// the thinnest on purpose: `tally` calls anything past about two worth
/// investigating. It is set to catch that figure getting worse, not to
/// bless where it stands, and one run at 1.4 does not retire it.
const BUDGET_2P_12X9: Drift = Drift {
    seat: 2.0,
    spot: 2.0,
    gap: 0.0,
};
const BUDGET_4P_12X9: Drift = Drift {
    seat: 2.5,
    spot: 2.5,
    gap: 0.0,
};
/// 200 games against the others' 3000, so the noisiest of the four.
const BUDGET_4P_16X11: Drift = Drift {
    seat: 3.0,
    spot: 3.0,
    gap: 0.0,
};
/// The spot figure is the long-edge pair's lead over the corners, which
/// the shape of a rectangle keeps (see `castle_spots`): 6.4 on
/// 2026-10-22, budgeted to catch it growing.
const BUDGET_6P_21X13: Drift = Drift {
    seat: 3.5,
    spot: 8.5,
    gap: 0.0,
};
/// The odd tables, first measured 2026-10-22 with the seats drawn: classic
/// 3p 0.6, generated 3p 12x9 1.2, generated 5p 17x11 0.6. Before the draw
/// the last seat always faced the empty spot, and 1500 rounds had the
/// classic arena's first seat winning 69% at three and the generated
/// beaches' third seat 22-26%; these sweeps read 22.8, 10.6 and 3.7 on
/// that build.
///
/// With the seats drawn the seat figure reads fair however unfair a spot
/// is, so the spots carry the gate. Measured the same night, and against
/// the layouts the odd tables moved away from, each built for the test:
///
/// | sweep | by spot | from the gap | the old layout |
/// |---|---|---|---|
/// | classic 3p x1000    | 1.7 | 6.4  | side holes: 1.6, 20.4 |
/// | generated 3p 12x9   | 1.4 | 8.8  | side holes: 1.7, 15.2 |
/// | generated 5p 17x11  | 6.3 | 22.8 | a corner empty: 13.4, 27.4 |
/// | open ocean 5p 17x11 | 3.2 | 29.8 | an edge empty: 37.6, 53.0 |
///
/// The seat figure is blind to every one of those layouts, and the spot
/// figure to the side holes, which favour a castle for where it stands
/// beside the gap, not on the beach. The figures from the gap are large
/// even as dealt: at five, the two corners beside an empty edge win 28%
/// and 33% and the two across from it 10% and 11%. The draw spreads that
/// over the seats rather than removing it, and the budgets are set to
/// catch it growing.
const BUDGET_3P_CLASSIC: Drift = Drift {
    seat: 3.0,
    spot: 3.0,
    gap: 10.0,
};
const BUDGET_3P_12X9: Drift = Drift {
    seat: 3.0,
    spot: 3.0,
    gap: 12.0,
};
/// 3000 rounds, where the three-seat sweeps play 1000: the old layout's
/// spot figure sits only seven sigma above the dealt one at that, and
/// would sit four at 1000.
const BUDGET_5P_17X11: Drift = Drift {
    seat: 3.0,
    spot: 9.5,
    gap: 26.0,
};
const BUDGET_5P_OCEAN: Drift = Drift {
    seat: 3.0,
    spot: 6.0,
    gap: 34.0,
};

/// The handmade maps, at each table they seat, by castle as drawn. First
/// measured by the prototype (6000 rounds each, 10,000 for Round Table)
/// with the per-round mirror, by spot, in sigma of the average score: Log
/// Jam 0.0 and 1.7, The Gates 0.2 and 1.6, The Moat 0.1 and 0.1, Three
/// Lanes 0.6 and 1.2 (2p and 4p), Round Table 0.3, 0.7 and 2.7, Round the
/// World 0.1, 2.4 and 1.7 (2p, 4p, 6p), Duel 0.6; no seat past 2.4. The
/// budget is noise for a fair map at 3000 rounds with room for one bad
/// night, and well under the 4-7 a lopsided castle showed (Three Lanes
/// unmirrored: 4.3; Round the World before the strip roll: 4.9). The
/// first run here (2026-10-23, 3000 rounds each) read 0.0-2.1 by seat and
/// 0.1-3.5 by spot, Log Jam at four the 3.5. Round Table at six read 9.1
/// by spot once a raid took a third rather than half: its corners had
/// paid for their richer holes in raids, and Fierce bots, which defend,
/// had never paid it. With its top and bottom holes a tile nearer the
/// middle and a wall behind each corner it read 1.1, 0.4 and 2.9 (2p,
/// 4p, 6p), and within a point of a fair win share at both bot levels.
///
/// The four maps added on 2026-10-26 are held to the same budget at each
/// of their tables, the odd ones included: Four Doors at three and Gull
/// Road at five are identical strips with a castle in each, so no spot
/// is left empty and there is no figure from the gap to take. Their first
/// run read 0.3-2.2 by seat and 0.2-1.2 by spot (The Fountain 2p and 4p,
/// Four Doors 3p, Gull Road 5p, Back to Back 2p, 4p and 6p).
const BUDGET_HANDMADE: Drift = Drift {
    seat: 3.0,
    spot: 4.0,
    gap: 0.0,
};
/// The Surprise styles at two to four seats and at six, by castle spot:
/// measured by the prototype at 3000 rounds, 0.1-2.4 at two to four and
/// 1.9-3.6 at six (Islands the highest), against today's terrain's 7.2 at
/// six. Lanes is dealt to four seats at most and swept only there.
const BUDGET_STYLE: Drift = Drift {
    seat: 3.0,
    spot: 4.5,
    gap: 0.0,
};
/// Three seats, which leave a corner empty: by spot 1.8-2.4 in the
/// prototype, and from the gap 9.4-17.8 on the first run here (3000
/// rounds), the lean today's terrain shows too (`BUDGET_3P_12X9`: 8.8 at
/// a third of the rounds, which is about 15 at these).
const BUDGET_STYLE_3P: Drift = Drift {
    seat: 3.0,
    spot: 4.0,
    gap: 22.0,
};
/// Five seats keep the frame's lean against the long-edge castles in every
/// style, as today's terrain does (`BUDGET_5P_17X11`): 4.9-12.9 by spot in
/// the prototype, worst on Lagoon and Islands; 8.4-14.2 by spot and
/// 16.3-22.8 from the gap on the first run here. Budgeted to catch it
/// growing, not to bless it.
const BUDGET_STYLE_5P: Drift = Drift {
    seat: 3.0,
    spot: 18.0,
    gap: 30.0,
};

/// The handmade maps' and the Surprise styles' sweeps, each table they are
/// played at, 3000 rounds each.
fn map_sweeps(sweeps: &mut Vec<Sweep>) {
    const ROUNDS: u64 = 3000;
    for map in MapChoice::DIAL
        .into_iter()
        .filter(|map| map.handmade().is_some())
    {
        for &seats in map.tables().expect("a handmade map's tables") {
            let label: &'static str =
                Box::leak(format!("{} {seats}p", map.token()).into_boxed_str());
            sweep(
                sweeps,
                label,
                Some(BUDGET_HANDMADE),
                ROUNDS,
                par(ROUNDS, |seed| play_drawn(map, seed, seats)),
                usize::from(seats),
            );
        }
    }
    // Surprise draws its style from the seed; swept a style at a time, so
    // each is held to its own figure. Built the way a Surprise round is,
    // with the style named rather than drawn.
    for seats in 2..=MAX_PLAYERS as u8 {
        assert!(holds(MapChoice::Surprise, seats));
        for style in surprise_styles(seats) {
            let label: &'static str =
                Box::leak(format!("surprise {} {seats}p", style.token()).into_boxed_str());
            let budget = match seats {
                3 => BUDGET_STYLE_3P,
                5 => BUDGET_STYLE_5P,
                _ => BUDGET_STYLE,
            };
            sweep(
                sweeps,
                label,
                Some(budget),
                ROUNDS,
                par(ROUNDS, |seed| {
                    // The first seed past this one that draws the style,
                    // so every round is a real Surprise round.
                    let seed = (seed * 64..)
                        .find(|&s| pinch_points::sim::surprise_style(s, seats) == style)
                        .expect("every style is drawn");
                    play(dealt(MapChoice::Surprise, seed, seats), seats)
                }),
                usize::from(seats),
            );
        }
    }
}

fn main() {
    probe(classic_arena(false, 2), 2);
    probe(classic_arena(false, 4), 4);

    // Budgets are per sweep because the sweeps are not comparable: two
    // seats sit a third of a sigma apart and six sit two and a half, so one
    // global number would miss real drift at two seats or fail every night
    // at six.
    let mut sweeps: Vec<Sweep> = Vec::new();

    // The new maps alone, for a quicker read of them.
    if std::env::var("BALANCE_MAPS").is_ok() {
        map_sweeps(&mut sweeps);
        verdict(&sweeps);
        return;
    }

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
        par(100, |seed| {
            play(versus_board(MapChoice::Classic, seed, 2), 2)
        }),
        2,
    );
    sweep(
        &mut sweeps,
        "classic 4p x100 seeds",
        None,
        100,
        par(100, |seed| {
            play(versus_board(MapChoice::Classic, seed, 4), 4)
        }),
        4,
    );
    if std::env::var("BALANCE_FULL").is_err() {
        // The generated sweeps are the gated ones, so a short run has
        // nothing to rule on. Say so rather than printing a pass.
        println!("\nclassic sweeps only; set BALANCE_FULL=1 for the gated ones");
        return;
    }
    println!("classic 2p: {:?}", play(classic_arena(false, 2), 2).scores);
    println!("classic 4p: {:?}", play(classic_arena(false, 4), 4).scores);
    // The generated-map seat spread is the one balance number still moving,
    // so it gets the sample size that makes it readable (see `tally`).
    sweep(
        &mut sweeps,
        "generated 4p 12x9",
        Some(BUDGET_4P_12X9),
        3000,
        par(3000, |seed| {
            play(versus_board(MapChoice::GenClassic, seed, 4), 4)
        }),
        4,
    );
    // Odd tables leave a spot empty, and which seat sits beside it is drawn
    // with the beach (see `seat_spots` in map_gen): the seat figures say
    // whether the draw holds, and the spot figures whether the layout
    // around the gap does. The classic arena seats three by the same draw,
    // so unlike its two- and four-seat sweeps this one varies the seats and
    // is gated.
    sweep(
        &mut sweeps,
        "classic 3p x1000 seeds",
        Some(BUDGET_3P_CLASSIC),
        1000,
        par(1000, |seed| {
            play(versus_board(MapChoice::Classic, seed, 3), 3)
        }),
        3,
    );
    sweep(
        &mut sweeps,
        "generated 3p 12x9",
        Some(BUDGET_3P_12X9),
        1000,
        par(1000, |seed| {
            play(versus_board(MapChoice::GenClassic, seed, 3), 3)
        }),
        3,
    );
    sweep(
        &mut sweeps,
        "generated 5p 17x11",
        Some(BUDGET_5P_17X11),
        3000,
        par(3000, |seed| {
            play(versus_board(MapChoice::GenLarge, seed, 5), 5)
        }),
        5,
    );
    // The open ocean seats five its own way, a corner left empty rather
    // than an edge (see `five_spots`), so it is swept on its own.
    sweep(
        &mut sweeps,
        "open ocean 5p 17x11",
        Some(BUDGET_5P_OCEAN),
        3000,
        par(3000, |seed| {
            play(versus_board(MapChoice::GenOcean, seed, 5), 5)
        }),
        5,
    );
    sweep(
        &mut sweeps,
        "generated 2p 12x9",
        Some(BUDGET_2P_12X9),
        300,
        par(300, |seed| {
            play(versus_board(MapChoice::GenClassic, seed, 2), 2)
        }),
        2,
    );
    sweep(
        &mut sweeps,
        "generated 4p 16x11",
        Some(BUDGET_4P_16X11),
        200,
        par(200, |seed| {
            play(versus_board(MapChoice::GenLarge, seed, 4), 4)
        }),
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
        par(3000, |seed| {
            play(versus_board(MapChoice::GenXl, seed, 6), 6)
        }),
        6,
    );

    map_sweeps(&mut sweeps);

    verdict(&sweeps);
}
