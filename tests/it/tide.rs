//! The tide roulette, played out at length.
//!
//! The wheel is spun by banking a Sparkling crab and several of the faces
//! it lands on put more crabs on the beach, so the mechanic feeds itself:
//! the interesting properties are all about *cadence*, and cadence only
//! shows over a whole round. These play real rounds rather than poking the
//! board, for the same reason the campaign proofs do.

use pinch_points::sim::{
    Board, BotLevel, EVENT_COOLDOWN, MAX_PLAYERS, Pcg32, PlayerAction, TideEvent, TileKind,
    bot_action, classic_arena_seeded,
};

/// Ticks per round: two minutes, the shortest Turf War that ships, which
/// is long enough for the roulette to be spun many times over.
const TICKS: u64 = 3600;

/// A four-seat beach with the roulette live. `classic_arena_seeded` turns
/// events on itself, as every arena the game ships does.
fn beach(seed: u64) -> Board {
    let mut board = classic_arena_seeded(seed, false, 4);
    assert!(
        board.events_enabled(),
        "an arena plays with the roulette on"
    );
    board.set_round_length(Some(TICKS as u32));
    board
}

/// One tick of four bots playing. The wheel is spun by *banking* a
/// Sparkling crab, so a round nobody routes never spins it at all: idle
/// seats make a quiet beach, not a representative one.
fn played(board: &Board) -> [PlayerAction; MAX_PLAYERS] {
    let mut actions = [PlayerAction::None; MAX_PLAYERS];
    for seat in 0..4u8 {
        actions[seat as usize] = bot_action(board, seat, BotLevel::Hard);
    }
    actions
}

/// Play a round out and answer every tide event and the tick it fired on.
fn events_of(seed: u64) -> Vec<(u64, TideEvent)> {
    let mut board = beach(seed);
    let mut seen: Vec<(u64, TideEvent)> = Vec::new();
    let mut last: Option<u64> = None;
    for _ in 0..TICKS {
        let actions = played(&board);
        board.tick(&actions);
        if let Some((event, at)) = board.last_event()
            && last != Some(at)
        {
            last = Some(at);
            seen.push((at, event));
        }
    }
    seen
}

/// The property the cooldown exists for, over forty rounds rather than
/// the one contrived board its unit test uses.
///
/// An event's own effect runs for `EVENT_TICKS`, and the cooldown is one
/// of those, so two events can never be watched at once: no mania on top
/// of a mania, and no banner cut off by the next banner. Before it, one
/// measured round put six inside nineteen seconds, three of them Crab
/// Mania.
#[test]
fn no_two_tide_events_land_inside_one_another() {
    let mut rounds_with_events = 0;
    for seed in 0..40u64 {
        let events = events_of(seed);
        rounds_with_events += usize::from(!events.is_empty());
        for pair in events.windows(2) {
            let gap = pair[1].0 - pair[0].0;
            assert!(
                gap >= u64::from(EVENT_COOLDOWN),
                "seed {seed}: {:?} at {} landed {gap} ticks after {:?}, inside the cooldown",
                pair[1].1,
                pair[1].0,
                pair[0].1
            );
        }
    }
    // And the sweep has to have been eventful enough to mean anything: a
    // roulette that never spun would pass the loop above trivially.
    assert!(
        rounds_with_events >= 20,
        "only {rounds_with_events} of forty rounds saw an event at all"
    );
}

/// Every face of the wheel is reachable. One draw indexes the whole list,
/// so an off-by-one in the modulo or a mis-ordered table would quietly
/// retire an event - and the one that went missing would be the last in
/// the list, which nobody would notice for a very long time.
#[test]
fn the_roulette_reaches_every_face() {
    let mut seen: Vec<TideEvent> = Vec::new();
    for seed in 0..60u64 {
        for (_, event) in events_of(seed) {
            if !seen.contains(&event) {
                seen.push(event);
            }
        }
        if seen.len() == TideEvent::ALL.len() {
            break;
        }
    }
    for event in TideEvent::ALL {
        assert!(
            seen.contains(&event),
            "{event:?} never came up in sixty rounds"
        );
    }
}

/// A beach with the roulette switched off keeps it off for the whole
/// round, however many Sparkling crabs bank. Every campaign level is one:
/// a puzzle whose board rearranged itself halfway through would be a
/// puzzle with no solution.
#[test]
fn a_beach_with_events_off_stays_quiet() {
    for seed in 0..12u64 {
        let mut board = classic_arena_seeded(seed, false, 4);
        board.set_round_length(Some(TICKS as u32));
        board.set_events_enabled(false);
        for tick in 0..TICKS {
            let actions = played(&board);
            board.tick(&actions);
            assert!(
                board.last_event().is_none(),
                "seed {seed}: an event fired at tick {tick} on a quiet beach"
            );
        }
    }
}

/// Every face leaves a board that still plays. The events reach in and
/// rewrite live state - Crab Mania empties the flock, Monopoly drains half
/// the crab list mid-round, Castle Swap deals the castles out again - and
/// a board left inconsistent by one of them would fail somewhere else
/// entirely, ticks later.
#[test]
fn every_tide_event_leaves_a_board_that_still_plays() {
    for event in TideEvent::ALL {
        let mut board = beach(7);
        // Let the beach fill first, so the events have something to act on.
        for _ in 0..600 {
            let actions = played(&board);
            board.tick(&actions);
        }
        board.force_tide_event(event, 0);
        let mut rng = Pcg32::new(0x5EED_0000 + event.index() as u64, 0xA5A5);
        for tick in 0..900 {
            // A little live input as well, so the board is not merely
            // coasting while it is examined.
            let mut actions = played(&board);
            if rng.next_u32().is_multiple_of(8) {
                actions[0] = PlayerAction::Place {
                    x: (rng.next_u32() % u32::from(board.width())) as u8,
                    y: (rng.next_u32() % u32::from(board.height())) as u8,
                    dir: pinch_points::sim::Direction::Up,
                };
            }
            board.tick(&actions);
            for crab in board.crabs() {
                let (x, y) = board.coords_u8(crab.tile);
                assert!(
                    x < board.width() && y < board.height(),
                    "{event:?}: a crab left the board at tick {tick}"
                );
            }
            for gull in board.gulls() {
                let (x, y) = board.coords_u8(gull.tile);
                assert!(
                    x < board.width() && y < board.height(),
                    "{event:?}: a gull left the board at tick {tick}"
                );
            }
        }
        // And the board is still describable, which is what the replay
        // line, the wire and the save file all rest on.
        let text = board.to_snapshot();
        assert!(
            Board::parse_snapshot(&text).is_ok(),
            "{event:?}: left a board its own format cannot write"
        );
    }
}

/// The map dial hands out beaches that are actually playable.
///
/// A generated arena is built by an algorithm rather than a person, and
/// nobody looks at one before it is played: the dial rolls it and six
/// people sit down in front of it. So the things a human author would
/// never get wrong are exactly the ones worth asserting - a seat with no
/// castle to run for, a crab standing inside a rock, a board that seats
/// fewer than it was asked to.
#[test]
fn every_generated_beach_is_one_that_can_be_played() {
    for seed in 0..40u64 {
        for seats in 2..=MAX_PLAYERS as u8 {
            let board = pinch_points::sim::generate_arena(seed, seats, 12, 9);
            let where_ = format!("seed {seed}, {seats} seats");

            let mut owners: Vec<u8> = board
                .tiles()
                // Named rather than wildcarded, so a new kind of tile has
                // to say here whether a seat can bank into it.
                .filter_map(|(_, _, kind)| match kind {
                    TileKind::Castle(owner) => Some(owner),
                    TileKind::Empty
                    | TileKind::Rock
                    | TileKind::Spawner(_)
                    | TileKind::Turnstile { .. }
                    | TileKind::Kelp
                    | TileKind::Pool => None,
                })
                .collect();
            owners.sort_unstable();
            owners.dedup();
            assert_eq!(
                owners.len(),
                seats as usize,
                "{where_}: {} seats got a castle, not {seats}",
                owners.len()
            );

            for crab in board.crabs() {
                let (x, y) = board.coords_u8(crab.tile);
                assert_ne!(
                    board.tile_at(x, y),
                    TileKind::Rock,
                    "{where_}: a crab starts inside a rock at ({x},{y})"
                );
            }
        }
    }
}

/// And a different beach for a different seed. A generator that ignored
/// its seed would pass every legality check above and quietly ship one
/// board for ever, which is the failure nobody would report as a bug -
/// only as the maps feeling samey.
#[test]
fn the_map_dial_is_not_one_beach_wearing_hats() {
    let hashes: Vec<u64> = (0..24u64)
        .map(|seed| pinch_points::sim::generate_arena(seed, 4, 12, 9).state_hash())
        .collect();
    let mut distinct = hashes.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert!(
        distinct.len() >= hashes.len() - 1,
        "twenty-four seeds produced only {} beaches",
        distinct.len()
    );
}
