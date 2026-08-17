//! What the bot does, and what it must never do: the decision ladder,
//! the hand it has to walk, the schedule every seat shares, and the
//! purity an online AI seat depends on.

use super::*;
use crate::sim::{CrabKind, Handedness, MAX_PLAYERS};

fn arena() -> Board {
    let mut board = Board::new(9, 7, 5);
    board.set_tile(4, 3, TileKind::Castle(1));
    board
}

/// The easy hand slips on a fixed cadence rather than at random, so an
/// online AI seat blunders the same way on every peer. A blunder only
/// mis-aims: it never moves the post to another tile.
#[test]
fn the_easy_hand_slips_predictably() {
    let straight = PlayerAction::Place {
        x: 3,
        y: 3,
        dir: Direction::Up,
    };
    let slipped = PlayerAction::Place {
        x: 3,
        y: 3,
        dir: Direction::Right,
    };
    let slips = |level: BotLevel, seat: PlayerId| {
        (0..40)
            .filter(|tick| fumble(straight, seat, level, *tick) == slipped)
            .count()
    };
    assert_eq!(slips(BotLevel::Easy, 0), 10, "one placement in four");
    assert_eq!(slips(BotLevel::Normal, 0), 5, "one in eight");
    assert_eq!(slips(BotLevel::Hard, 0), 0, "fierce does not slip");
    // Every seat slips, and none of them in step with another - a shared
    // rhythm would make two easy bots blunder together.
    let counts: Vec<usize> = (0..MAX_PLAYERS as u8)
        .map(|seat| slips(BotLevel::Easy, seat))
        .collect();
    assert!(counts.iter().all(|&n| n == 10), "{counts:?}");
    let first: Vec<u64> = (0..MAX_PLAYERS as u8)
        .map(|seat| {
            (0..40)
                .find(|tick| fumble(straight, seat, BotLevel::Easy, *tick) == slipped)
                .unwrap_or(99)
        })
        .collect();
    assert!(
        first
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            > 1,
        "seats slip on the same tick: {first:?}"
    );
    // And nothing else is touched.
    assert_eq!(
        fumble(PlayerAction::None, 0, BotLevel::Easy, 0),
        PlayerAction::None
    );
}

/// Only the fierce bot crosses the beach for a jackpot: a golden crab
/// eleven tiles from its castle is invisible to the other two, and out of
/// even fierce's recruiting reach, so the placement can only be the chase.
#[test]
fn only_fierce_chases_a_jackpot_across_the_board() {
    // The first placement each level makes, if any, inside a second of
    // play. Every level acts on its own slot of its own cadence, so the
    // answer has to be looked for over ticks rather than on tick zero.
    let first_place = |level: BotLevel| {
        // A board wide enough that "across the beach" is further than any
        // level's recruiting horizon (fierce's is ten): eleven steps from
        // castle to crab, inside fierce's fourteen-tile jackpot reach.
        let mut board = Board::new(15, 11, 5);
        board.set_tile(7, 5, TileKind::Castle(1));
        // Eleven steps out, with room ahead of it: a crab flat against a
        // wall has no tile in front to post on.
        board.spawn_crab(14, 1, Direction::Up, Handedness::Left, CrabKind::Golden);
        (0..40).find_map(|_| {
            let action = bot_action(&board, 1, level);
            board.tick_idle();
            matches!(action, PlayerAction::Place { .. }).then_some(action)
        })
    };
    let fierce = first_place(BotLevel::Hard);
    assert!(
        matches!(fierce, Some(PlayerAction::Place { x: 14, y: 0, .. })),
        "fierce goes for the gold: {fierce:?}"
    );
    for level in [BotLevel::Easy, BotLevel::Normal] {
        assert_eq!(
            first_place(level),
            None,
            "{level:?} should not see a crab eleven tiles away"
        );
    }
}

/// The bot's hand has to walk. A placement far from its last one has to
/// wait out the trip; the tile it is already standing on is free; and the
/// walk is charged the way a player's cursor actually moves.
#[test]
fn a_bot_pays_for_the_walk_to_the_tile() {
    let mut board = arena();
    // Its hand is at (1,1) as of tick 0.
    assert!(board.place_signpost(1, 1, 1, Direction::Up));
    let level = BotLevel::Normal;

    // Right next door is one keypress: free, like a player's first tap.
    assert!(hand_arrived(&board, 1, level, 2, 1));
    assert!(hand_arrived(&board, 1, level, 1, 1), "already there");

    // The far corner is twelve steps away: 8 + 11*3 = 41 ticks.
    assert!(!hand_arrived(&board, 1, level, 8, 6), "no teleporting");
    for _ in 0..40 {
        board.tick_idle();
    }
    assert!(!hand_arrived(&board, 1, level, 8, 6), "still walking");
    board.tick_idle();
    assert!(hand_arrived(&board, 1, level, 8, 6), "arrived at last");

    // A fiercer bot's hand is quicker over the same ground.
    let mut fresh = arena();
    assert!(fresh.place_signpost(1, 1, 1, Direction::Up));
    for _ in 0..30 {
        fresh.tick_idle();
    }
    assert!(hand_arrived(&fresh, 1, BotLevel::Hard, 8, 6));
    assert!(!hand_arrived(&fresh, 1, BotLevel::Easy, 8, 6));

    // With nothing of its own standing, it has been idle long enough to
    // be anywhere.
    let empty = arena();
    assert!(hand_arrived(&empty, 1, BotLevel::Easy, 8, 6));
}

/// The gate only ever holds a placement back, and it holds the same way
/// on every copy of the board, so an online AI seat can be derived rather
/// than sent.
#[test]
fn the_hand_gate_is_a_pure_function_of_the_board() {
    let mut board = arena();
    board.spawn_gull(4, 1, Direction::Down);
    let mut twin = board.clone();
    for frame in 0..300 {
        let mine = bot_action(&board, 1, BotLevel::Hard);
        let theirs = bot_action(&twin, 1, BotLevel::Hard);
        assert_eq!(mine, theirs, "two copies disagreed at frame {frame}");
        let mut frame_actions = [PlayerAction::None; MAX_PLAYERS];
        frame_actions[1] = mine;
        board.tick(&frame_actions);
        let mut twin_actions = [PlayerAction::None; MAX_PLAYERS];
        twin_actions[1] = theirs;
        twin.tick(&twin_actions);
    }
}

#[test]
fn bot_blocks_an_incoming_gull() {
    let mut board = arena();
    board.spawn_gull(4, 1, Direction::Down); // two tiles above the castle
    // Advance to the bot's cadence window.
    let action = loop {
        match bot_action(&board, 1, BotLevel::Normal) {
            PlayerAction::None => board.tick_idle(),
            act @ (PlayerAction::Place { .. } | PlayerAction::Remove { .. }) => break act,
        }
    };
    let PlayerAction::Place { y, dir, .. } = action else {
        panic!("expected a defensive placement, got {action:?}");
    };
    assert_eq!(
        dir,
        Direction::Up,
        "points the gull back where it came from"
    );
    assert!(y <= 3, "placed on the gull's approach");
}

#[test]
fn bot_recruits_crabs_and_banks_them() {
    let mut board = arena();
    board.spawn_crab(1, 1, Direction::Up, Handedness::Left, CrabKind::Common);
    board.spawn_crab(7, 5, Direction::Down, Handedness::Right, CrabKind::Giant);
    for _ in 0..900 {
        let mut actions = [PlayerAction::None; MAX_PLAYERS];
        actions[1] = bot_action(&board, 1, BotLevel::Normal);
        board.tick(&actions);
    }
    assert!(
        board.scores()[1] > 0,
        "the bot routed at least one crab home (score {:?})",
        board.scores()
    );
}

/// A gull five tiles out: Normal (radius 4) ignores it, Hard (6) reacts.
#[test]
fn hard_defends_farther_than_normal() {
    let mut probe = arena(); // castle at (4,3)
    probe.spawn_gull(0, 2, Direction::Right); // manhattan |4-0|+|3-2| = 5
    let normal = first_window_action(&probe, BotLevel::Normal);
    let hard = first_window_action(&probe, BotLevel::Hard);
    assert_eq!(normal, PlayerAction::None, "out of Normal's defend radius");
    assert!(
        matches!(hard, PlayerAction::Place { .. }),
        "within Hard's defend radius, got {hard:?}"
    );
}

/// Easy thinks half as often as Normal, and every seat thinks exactly as
/// often as every other - the rate is the promise, not the moment.
#[test]
fn a_level_thinks_at_its_own_rate_and_no_seat_thinks_more() {
    let cycles = 600u64;
    for (level, cadence) in [
        (BotLevel::Easy, 40u64),
        (BotLevel::Normal, 20),
        (BotLevel::Hard, 12),
    ] {
        for seat in 0..MAX_PLAYERS as u8 {
            let turns = (0..cycles * cadence)
                .filter(|tick| level.acts_on(seat, *tick))
                .count() as u64;
            assert_eq!(
                turns, cycles,
                "{level:?} seat {seat} took {turns} turns in {cycles} windows"
            );
        }
    }
    // And a slower level really is slower over the same stretch.
    let count = |level: BotLevel| (0..1200u64).filter(|tick| level.acts_on(1, *tick)).count();
    assert_eq!(count(BotLevel::Easy) * 2, count(BotLevel::Normal));
}

/// The moment inside a window is drawn, not fixed: no seat sits on the
/// same beat as the spawner holes, which fire on even ticks.
#[test]
fn decision_moments_do_not_settle_on_one_parity() {
    for seat in 0..MAX_PLAYERS as u8 {
        let odd = (0..400u64)
            .filter(|tick| BotLevel::Normal.acts_on(seat, *tick))
            .filter(|tick| !tick.is_multiple_of(2))
            .count();
        // Twenty windows in four hundred ticks; a fixed grid would put
        // every one of them on the same parity.
        assert!(
            (4..=16).contains(&odd),
            "seat {seat} landed on {odd} odd ticks out of 20 windows"
        );
    }
}

/// Hard weaponizes a gull loitering near the leading rival's castle. Its
/// own castle sits in the far corner, twelve tiles from the gull, so the
/// placement cannot be defense wearing offense's clothes.
#[test]
fn hard_steers_gulls_at_the_leader() {
    let mut board = Board::new(9, 7, 5);
    board.set_tile(8, 6, TileKind::Castle(1)); // ours, out of defend range
    board.set_tile(0, 0, TileKind::Castle(2));
    board.set_score(2, 20); // seat 2 leads
    board.spawn_gull(2, 0, Direction::Down); // 2 tiles from castle (0,0)
    let action = first_window_action(&board, BotLevel::Hard);
    let PlayerAction::Place { x, y, dir } = action else {
        panic!("expected an offensive placement, got {action:?}");
    };
    // Aimed from the gull's path toward the rival castle at (0,0).
    assert!(x <= 2 && y <= 2, "placed on the gull's approach ({x},{y})");
    assert!(
        dir == Direction::Left || dir == Direction::Up,
        "points toward the leader's castle, got {dir:?}"
    );
    // Normal never plays offense: the same gull by the same leader, with
    // nothing of its own to defend or recruit, gets no post from it.
    assert_eq!(
        first_window_action(&board, BotLevel::Normal),
        PlayerAction::None
    );
}

/// Advance a fresh clone to seat 1's next decision window and return the
/// bot's action there.
fn first_window_action(board: &Board, level: BotLevel) -> PlayerAction {
    let mut board = board.clone();
    loop {
        let action = bot_action(&board, 1, level);
        if !level.acts_on(1, board.ticks()) {
            board.tick_idle();
            continue;
        }
        return action;
    }
}

/// Offense skips gulls already heading at the target; ties for the
/// lead resolve to the lowest seat. The bot's own castle sits in a
/// corner so its defense radius cannot reach the probes.
#[test]
fn hard_offense_edge_cases() {
    // Gull already inbound toward the leader's castle: nothing to do.
    let mut board = Board::new(9, 7, 5);
    board.set_tile(0, 0, TileKind::Castle(1)); // ours, far away
    board.set_tile(8, 6, TileKind::Castle(2));
    board.set_score(2, 20);
    board.spawn_gull(8, 4, Direction::Down); // already heading castle-ward
    assert_eq!(
        first_window_action(&board, BotLevel::Hard),
        PlayerAction::None
    );

    // Two tied rivals: the lower seat is the target, so a gull loitering
    // by the higher seat's castle is ignored.
    let mut board = Board::new(9, 7, 5);
    board.set_tile(0, 0, TileKind::Castle(1));
    board.set_tile(8, 0, TileKind::Castle(0));
    board.set_tile(8, 6, TileKind::Castle(2));
    board.set_score(0, 20);
    board.set_score(2, 20);
    board.spawn_gull(6, 6, Direction::Left); // near seat 2's castle only
    assert_eq!(
        first_window_action(&board, BotLevel::Hard),
        PlayerAction::None,
        "the tied lead resolves to seat 0, far from this gull"
    );
}

/// Fierce reads the weed, and reads it honestly. Kelp is a wall to a
/// walking gull, so a post aimed into it does not park the bird: the sim
/// turns it along the weed by its handedness, and a right-handed gull
/// turned to its right may be looking straight at the castle. Fierce
/// shoves only when neither hand's turn ends closer than plain reversal;
/// otherwise it reverses, which is all Normal ever knows.
#[test]
fn fierce_shoves_gulls_into_kelp() {
    // Kelp beside the gull's next tile, castle straight ahead: a shove
    // to the right would turn a right-handed bird Down, onto the castle.
    // Fierce declines and reverses it instead.
    let mut board = arena(); // castle at (4,3)
    board.spawn_gull(4, 1, Direction::Down); // next tile is (4,2)
    board.set_tile(5, 2, TileKind::Kelp); // kelp to its right
    for level in [BotLevel::Hard, BotLevel::Normal] {
        assert_eq!(
            first_window_action(&board, level),
            PlayerAction::Place {
                x: 4,
                y: 2,
                dir: Direction::Up
            },
            "{level:?} sends the gull back rather than around the weed"
        );
    }

    // With the way ahead blocked, the only turn out of the kelp is back
    // up the gull's own line, whichever hand it has: safe, so Fierce
    // takes the shove where Normal still just reverses.
    let mut blocked = Board::new(9, 7, 5);
    blocked.set_tile(4, 4, TileKind::Castle(1));
    blocked.set_tile(4, 3, TileKind::Rock);
    blocked.spawn_gull(4, 1, Direction::Down); // next tile is (4,2)
    blocked.set_tile(5, 2, TileKind::Kelp);
    let PlayerAction::Place { x, y, dir } = first_window_action(&blocked, BotLevel::Hard) else {
        panic!("expected a defensive placement");
    };
    assert_eq!((x, y), (4, 2), "the post lands in front of the gull");
    assert_eq!(dir, Direction::Right, "pointed into the kelp");
    assert_eq!(
        first_window_action(&blocked, BotLevel::Normal),
        PlayerAction::Place {
            x: 4,
            y: 2,
            dir: Direction::Up
        }
    );
}

/// The probe behind the rule above: play the kelp layout out with a
/// fierce defender against gulls of either hand, and the castle is never
/// raided. Handedness is rolled from the board's seed, so several seeds
/// are played to be sure a right-handed bird was among them.
#[test]
fn a_fierce_kelp_shove_never_hands_a_gull_the_castle() {
    let mut saw_right_hand = false;
    for seed in 0..8 {
        let mut board = Board::new(9, 7, seed);
        board.set_tile(4, 3, TileKind::Castle(1));
        board.set_score(1, 40);
        board.spawn_gull(4, 1, Direction::Down);
        board.set_tile(5, 2, TileKind::Kelp);
        saw_right_hand |= board.gulls()[0].handed == Handedness::Right;
        for _ in 0..400 {
            let mut actions = [PlayerAction::None; MAX_PLAYERS];
            actions[1] = bot_action(&board, 1, BotLevel::Hard);
            board.tick(&actions);
        }
        assert_eq!(
            board.scores()[1],
            40,
            "seed {seed}: the castle was raided ({:?})",
            board.gulls()[0].handed
        );
    }
    assert!(saw_right_hand, "no seed rolled a right-handed gull");
}

/// Fierce walks its crabs around a tide pool (half speed) when the other
/// axis closes the gap just as well.
#[test]
fn fierce_routes_crabs_around_pools() {
    let mut board = arena(); // castle at (4,3)
    // A crab at (1,1) walking away from home; the post lands at (0,1),
    // from where home is Right (dx 4, dy 2), but that step is a pool.
    board.spawn_crab(1, 1, Direction::Left, Handedness::Left, CrabKind::Common);
    board.set_tile(1, 1, TileKind::Pool);
    let PlayerAction::Place { x, y, dir } = first_window_action(&board, BotLevel::Hard) else {
        panic!("expected a recruiting placement");
    };
    assert_eq!((x, y), (0, 1));
    assert_eq!(dir, Direction::Down, "dry route home, around the pool");
    // Normal wades straight through.
    assert_eq!(
        first_window_action(&board, BotLevel::Normal),
        PlayerAction::Place {
            x: 0,
            y: 1,
            dir: Direction::Right
        }
    );
}

/// A signpost pointing straight into a turnstile buys a turn the log
/// takes back on the next tile, so Fierce declines to spend one there.
#[test]
fn fierce_never_feeds_a_turnstile() {
    let mut board = arena(); // castle at (4,3)
    board.spawn_gull(4, 1, Direction::Down); // post would land at (4,2)
    // The gull is walking off a turnstile, so reversing it at (4,2)
    // would send it Up onto the log, which would just spin it again.
    board.set_tile(4, 1, TileKind::Turnstile { next_right: true });
    assert_eq!(
        first_window_action(&board, BotLevel::Hard),
        PlayerAction::None
    );
    assert!(matches!(
        first_window_action(&board, BotLevel::Normal),
        PlayerAction::Place { .. }
    ));
}

/// A seat with no castle has nothing to defend and nowhere to send a
/// crab, so it stays idle through every one of its decision windows, not
/// merely on tick zero where the cadence gate would answer for it.
#[test]
fn bot_without_a_castle_stays_idle() {
    let mut board = Board::new(5, 5, 0);
    board.spawn_gull(2, 0, Direction::Down);
    board.spawn_crab(0, 2, Direction::Right, Handedness::Left, CrabKind::Golden);
    let mut windows = 0;
    for _ in 0..200 {
        if BotLevel::Normal.acts_on(2, board.ticks()) {
            windows += 1;
            assert_eq!(
                bot_action(&board, 2, BotLevel::Normal),
                PlayerAction::None,
                "tick {}",
                board.ticks()
            );
        }
        board.tick_idle();
    }
    assert_eq!(windows, 10, "every window was checked");
}
