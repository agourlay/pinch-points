//! The determinism contract (spec §7.5): the same seed and input list must
//! reproduce bit-identical state on every run and every platform. CI runs this
//! on Linux; `EXPECTED_HASH` is the fixed anchor a run on any other platform
//! is held to (the release workflow builds, but does not test, on Windows and
//! macOS).

use pinch_points::sim::{
    Board, CrabKind, Direction, Handedness, MAX_PLAYERS, PlayerAction, Spawner, TileKind,
};

/// A busy 12×9 board exercising every M1 feature: walls, rocks, castles,
/// spawners, mixed crab kinds, and both handednesses.
fn build_board(seed: u64) -> Board {
    let mut board = Board::new(12, 9, seed);
    board.set_tile(
        0,
        0,
        TileKind::Spawner(Spawner {
            dir: Direction::Right,
            period: 7,
        }),
    );
    board.set_tile(
        11,
        8,
        TileKind::Spawner(Spawner {
            dir: Direction::Left,
            period: 11,
        }),
    );
    board.set_tile(11, 0, TileKind::Castle(0));
    board.set_tile(0, 8, TileKind::Castle(1));
    board.set_tile(5, 4, TileKind::Rock);
    board.set_tile(6, 4, TileKind::Rock);
    board.set_wall(3, 3, Direction::Right, true);
    board.set_wall(3, 4, Direction::Right, true);
    board.set_wall(8, 2, Direction::Down, true);
    board.set_wall(2, 6, Direction::Up, true);
    board.spawn_crab(4, 4, Direction::Up, Handedness::Left, CrabKind::Giant);
    board.spawn_crab(7, 4, Direction::Up, Handedness::Right, CrabKind::Giant);
    board.spawn_crab(6, 2, Direction::Left, Handedness::Left, CrabKind::Juvenile);
    board.spawn_crab(6, 6, Direction::Right, Handedness::Right, CrabKind::Molting);
    // M4 mechanics: gulls (walking, flying, eating, castle raids), periodic
    // edge spawning with the tide surge, and a timed round that freezes the
    // final 1000 ticks of the run.
    board.spawn_gull(3, 0, Direction::Down);
    board.set_gull_period(450);
    board.set_round_length(Some(9_000));
    board
}

/// Deterministic scripted inputs: players place, re-place past the cap, and
/// remove signposts throughout the run.
fn actions_for(tick: u64) -> [PlayerAction; MAX_PLAYERS] {
    let mut actions = [PlayerAction::None; MAX_PLAYERS];
    let dirs = [
        Direction::Up,
        Direction::Right,
        Direction::Down,
        Direction::Left,
    ];
    if tick.is_multiple_of(37) {
        let n = tick / 37;
        actions[0] = PlayerAction::Place {
            x: (n % 12) as u8,
            y: ((n * 5) % 9) as u8,
            dir: dirs[(n % 4) as usize],
        };
    }
    if tick.is_multiple_of(53) {
        let n = tick / 53;
        actions[1] = PlayerAction::Place {
            x: ((n * 3) % 12) as u8,
            y: (n % 9) as u8,
            dir: dirs[((n + 2) % 4) as usize],
        };
    }
    if tick.is_multiple_of(111) {
        // Remove one of player 1's own placements (same coordinate formula
        // as the tick-53 Place above), so owned-removal is genuinely
        // exercised; player 2's attempts on foreign posts stay no-ops.
        let n = tick / 111;
        let m = n * 2; // an earlier Place index of player 1
        actions[1] = PlayerAction::Remove {
            x: ((m * 3) % 12) as u8,
            y: (m % 9) as u8,
        };
        actions[2] = PlayerAction::Remove {
            x: (n % 12) as u8,
            y: ((n * 5) % 9) as u8,
        };
    }
    actions
}

const TICKS: u64 = 10_000;

/// Anchor value for cross-platform comparison. If a deliberate rule change
/// shifts it, rerun and update; an unexplained shift is a determinism bug.
///
/// Last re-derived 2026-08-27, when the tide roulette was given the same
/// kind of room the lure got. The wheel is spun by banking a Sparkling
/// crab, and several of the faces it lands on put more crabs on the beach,
/// so the events raised their own rate: measured across the kept rounds it
/// ranged from nought events in three minutes to twelve, and the worst put
/// six inside nineteen seconds. `EVENT_COOLDOWN` holds it to one event per
/// ten seconds - one `EVENT_TICKS`, so a second cannot start on top of the
/// first. That moves this round (the wheel is spun on it) and the new
/// `event_cooldown` field joins the fingerprint besides, for the reason
/// `lure_cooldown` did: it is live state that decides what happens next.
///
/// Before that, 2026-08-22, when the lure was given room: the quiet
/// spell after one ends went from 10 s to 20 s and the molting crab's share
/// of the spawn mix from 4% to 3%. Both move this round - a lure decides
/// where every loose crab walks, and the mix bands shifted so crabs above
/// the molting band draw a different kind, and the kinds move at different
/// speeds.
///
/// Before that, 2026-08-21, when castle raids became a board switch so
/// puzzles could turn them off. `castle_raids` joined the fingerprint
/// because it decides whether a gull reaching a castle takes anything, and
/// two boards that disagreed on it would report the same hash and then
/// play apart. The board here is an arena, with raids on as ever, so the
/// round it plays is unchanged - only the hash's reach grew.
///
/// Before that, 2026-08-16, when gulls started catching crabs they had
/// been walking through. Contact was tested tile by tile, and two creatures
/// approaching head-on cross between two tile centres while still filed
/// under different tiles, so the collision was never looked at. Every gull
/// on this board now eats what it meets, which moves the round.
///
/// Before that, 2026-08-13, when `lure_cooldown` joined the fingerprint.
/// It was live state all along, since it decides whether banking a molt
/// starts a lure, but it had never been hashed, so two boards could hold
/// different cooldowns, report the same hash, and then play differently.
/// The rules did not move; what the hash can see did.
///
/// Before that, 2026-07-30 for the widening to six seats: every board
/// hashes six per-seat slots now, so the anchor moved even where the rules
/// did not. (The rules moved that day too: the lure stopped stacking, the
/// roulette stopped rolling gull events into the surge, and the spawners
/// took a crab cap.)
const EXPECTED_HASH: u64 = 0x509a_9e6e_cf97_32c5;

#[test]
fn ten_thousand_ticks_reproduce_exactly() {
    let mut a = build_board(0xDECA_FBAD);
    let mut b = build_board(0xDECA_FBAD);
    let mut ever_banked = false;
    for t in 0..TICKS {
        let actions = actions_for(t);
        a.tick(&actions);
        b.tick(&actions);
        assert_eq!(a.state_hash(), b.state_hash(), "diverged at tick {t}");
        ever_banked |= a.scores().iter().any(|&s| s > 0);
    }
    // The run must also have been eventful enough to mean something. (Gull
    // raids can knock a tier-0 score back to zero, so sample during the run.)
    assert!(ever_banked, "no crab ever banked");
    assert!(!a.crabs().is_empty(), "no crabs left on the board");
    assert!(!a.gulls().is_empty(), "no gulls on the board");
    assert!(a.round_over(), "the tide never came in");

    let final_hash = a.state_hash();
    assert_eq!(
        final_hash, EXPECTED_HASH,
        "state hash after {TICKS} ticks was {final_hash:#018x}, expected {EXPECTED_HASH:#018x}"
    );
}

#[test]
fn different_seeds_diverge() {
    let mut a = build_board(1);
    let mut b = build_board(2);
    for t in 0..200 {
        let actions = actions_for(t);
        a.tick(&actions);
        b.tick(&actions);
    }
    assert_ne!(a.state_hash(), b.state_hash());
}

/// The daily challenge is the same beach for everybody, and a different
/// one tomorrow.
///
/// The whole premise is in the strapline: "the same beach for everyone".
/// Nobody can check that from inside one machine except by pinning the
/// only thing that varies - the day - and proving the beach follows it and
/// nothing else. A seed that drifted would hand two friends different
/// boards and the same scoreboard.
#[test]
fn the_daily_beach_is_one_beach_a_day() {
    use pinch_points::app::Daily;
    // The same day is the same beach, however many times it is asked.
    for day in [0u32, 1, 19_000, 20_321, u32::MAX] {
        let seed = Daily::seed_for(day);
        assert_eq!(seed, Daily::seed_for(day), "day {day} is not stable");
        let a = pinch_points::sim::generate_arena(seed, 4, 12, 9);
        let b = pinch_points::sim::generate_arena(Daily::seed_for(day), 4, 12, 9);
        assert_eq!(
            a.state_hash(),
            b.state_hash(),
            "day {day} built two different beaches from one seed"
        );
    }
    // And consecutive days are different beaches: a seed that ignored the
    // day would pass everything above and ship one board for ever.
    let week: Vec<u64> = (19_000..19_007).map(Daily::seed_for).collect();
    for (i, seed) in week.iter().enumerate() {
        for (j, other) in week.iter().enumerate().skip(i + 1) {
            assert_ne!(seed, other, "days {i} and {j} share a seed");
        }
    }
}

/// A recorded round survives the trip to text and back and still plays to
/// the same last frame.
///
/// `replays_round_trip_through_text` proves the *file* comes back equal.
/// This is the harder half: that the round it describes, replayed from
/// the reloaded copy, ends on the board the original ended on. A field
/// rounded off in the format would pass the first and fail here, days
/// later, as a kept round that plays out differently from the one that
/// was played.
#[test]
fn a_replay_reloaded_from_text_ends_on_the_same_board() {
    use pinch_points::sim::{BotLevel, Level, Replay, bot_action, classic_arena_seeded};
    for seed in [1u64, 2, 3] {
        let mut board = classic_arena_seeded(seed, false, 4);
        board.set_round_length(Some(900));
        let mut replay = Replay::new(Level::from_board("Turf War", 3, board.clone()));
        while !board.round_over() {
            let mut actions = [PlayerAction::None; MAX_PLAYERS];
            for seat in 0..4u8 {
                actions[seat as usize] = bot_action(&board, seat, BotLevel::Normal);
            }
            board.tick(&actions);
            replay.record(actions);
        }
        let played = board.state_hash();
        assert_eq!(
            replay.playback().state_hash(),
            played,
            "seed {seed}: the recording is wrong"
        );

        let text = replay.to_text();
        let reloaded = Replay::parse(&text).expect("a kept round reloads");
        assert_eq!(
            reloaded.playback().state_hash(),
            played,
            "seed {seed}: the round came back off the shelf and played out differently"
        );
    }
}
