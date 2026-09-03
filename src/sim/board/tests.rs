//! The board's unit tests, spec section by spec section.

use super::*;
use crate::sim::direction::Direction::*;

fn common(board: &mut Board, x: u8, y: u8, dir: Direction, handed: Handedness) {
    board.spawn_crab(x, y, dir, handed, CrabKind::Common);
}

/// Ticks needed for a crab of `speed` to travel `tiles` tile-lengths.
fn ticks_to_cross(tiles: u32, speed: u32) -> u32 {
    (tiles * u32::from(SUBUNITS_PER_TILE)).div_ceil(speed)
}

#[test]
fn crab_crosses_a_tile_at_spec_speed() {
    let mut board = Board::new(5, 1, 0);
    common(&mut board, 0, 0, Right, Handedness::Left);
    // 256 subunits at 12/tick: still on tile 0 after 21 ticks (252).
    for _ in 0..21 {
        board.tick_idle();
    }
    assert_eq!(board.crabs()[0].tile, 0);
    assert_eq!(board.crabs()[0].progress, 252);
    // Tick 22 crosses: snap to tile 1, carry remainder 264 - 256 = 8.
    board.tick_idle();
    assert_eq!(board.crabs()[0].tile, 1);
    assert_eq!(board.crabs()[0].progress, 8);
}

#[test]
fn prev_fields_track_last_tick() {
    let mut board = Board::new(5, 1, 0);
    common(&mut board, 0, 0, Right, Handedness::Left);
    board.tick_idle();
    let crab = board.crabs()[0];
    assert_eq!(crab.prev.progress, 0);
    assert_eq!(crab.progress, 12);
    board.tick_idle();
    assert_eq!(board.crabs()[0].prev.progress, 12);
}

#[test]
fn handedness_splits_at_a_wall() {
    // Two crabs walk up the same corridor into the top border; the
    // left-clawed one turns left, the right-clawed one turns right.
    for (handed, expected) in [(Handedness::Left, Left), (Handedness::Right, Right)] {
        let mut board = Board::new(3, 3, 0);
        common(&mut board, 1, 1, Up, handed);
        for _ in 0..ticks_to_cross(1, 12) {
            board.tick_idle();
        }
        let crab = board.crabs()[0];
        assert_eq!((crab.tile, crab.dir), (1, expected), "{handed:?}");
    }
}

#[test]
fn blocked_preferred_side_takes_other_side() {
    // Left-clawed crab arrives at the top-left corner moving up: forward
    // and left are border walls, so it takes its off-side, right.
    let mut board = Board::new(3, 3, 0);
    common(&mut board, 0, 1, Up, Handedness::Left);
    for _ in 0..ticks_to_cross(1, 12) {
        board.tick_idle();
    }
    assert_eq!(board.crabs()[0].dir, Right);
}

#[test]
fn dead_end_reverses() {
    let mut board = Board::new(1, 3, 0);
    common(&mut board, 0, 1, Down, Handedness::Left);
    for _ in 0..ticks_to_cross(1, 12) {
        board.tick_idle();
    }
    let crab = board.crabs()[0];
    assert_eq!((crab.tile, crab.dir), (u16::from(board.width()) * 2, Up));
}

#[test]
fn fully_enclosed_crab_waits_without_panicking() {
    let mut board = Board::new(1, 1, 0);
    common(&mut board, 0, 0, Right, Handedness::Left);
    for _ in 0..100 {
        board.tick_idle();
    }
    assert_eq!(board.crabs()[0].tile, 0);
    assert_eq!(board.crabs()[0].progress, 0);
}

#[test]
fn rocks_are_impassable() {
    // Corridor with a rock in the middle: the crab treats it like a wall
    // and reverses (1×3 board leaves no side exits).
    let mut board = Board::new(3, 1, 0);
    board.set_tile(1, 0, TileKind::Rock);
    common(&mut board, 0, 0, Right, Handedness::Left);
    board.tick_idle();
    // spawn_crab wall-resolved immediately: right is blocked by the rock,
    // both sides are borders, so the crab reversed to Left on spawn.
    assert_eq!(board.crabs()[0].dir, Left);
}

#[test]
fn signpost_redirects_crabs() {
    let mut board = Board::new(5, 5, 0);
    assert!(board.place_signpost(0, 2, 2, Up));
    common(&mut board, 0, 2, Right, Handedness::Left);
    for _ in 0..ticks_to_cross(2, 12) {
        board.tick_idle();
    }
    let crab = board.crabs()[0];
    assert_eq!(crab.dir, Up);
    // One more tile-crossing later it has moved up to (2, 1).
    for _ in 0..ticks_to_cross(1, 12) {
        board.tick_idle();
    }
    assert_eq!(board.crabs()[0].tile, u16::from(board.width()) + 2);
}

#[test]
fn signpost_into_wall_is_followed_then_resolved() {
    // Frozen §9#2 behaviour: the signpost turns the crab into the wall,
    // then normal wall resolution applies from the signpost's direction.
    for (handed, expected) in [(Handedness::Left, Left), (Handedness::Right, Right)] {
        let mut board = Board::new(5, 5, 0);
        assert!(board.place_signpost(0, 2, 0, Up)); // top row, points at border
        common(&mut board, 0, 0, Right, handed);
        for _ in 0..ticks_to_cross(2, 12) {
            board.tick_idle();
        }
        let crab = board.crabs()[0];
        assert_eq!((crab.tile, crab.dir), (2, expected), "{handed:?}");
    }
}

#[test]
fn banking_awards_value_and_despawns() {
    let mut board = Board::new(5, 1, 0);
    board.set_tile(4, 0, TileKind::Castle(2));
    common(&mut board, 0, 0, Right, Handedness::Left);
    board.spawn_crab(1, 0, Right, Handedness::Right, CrabKind::Giant);
    // Giant (speed 7) needs 3 tiles: ceil(768 / 7) = 110 ticks, the
    // slowest arrival. Common banks earlier.
    for _ in 0..ticks_to_cross(3, 7) {
        board.tick_idle();
    }
    assert!(board.crabs().is_empty());
    assert_eq!(board.scores()[2], 1 + 10);
    assert_eq!(board.scores()[0], 0);
}

#[test]
fn juvenile_is_faster() {
    let mut board = Board::new(5, 1, 0);
    board.spawn_crab(0, 0, Right, Handedness::Left, CrabKind::Juvenile);
    common(&mut board, 0, 0, Right, Handedness::Left);
    for _ in 0..ticks_to_cross(1, 18) {
        board.tick_idle();
    }
    assert_eq!(board.crabs()[0].tile, 1); // juvenile crossed
    assert_eq!(board.crabs()[1].tile, 0); // common has not
}

#[test]
fn signpost_cap_evicts_oldest() {
    let mut board = Board::new(6, 1, 0);
    for x in 0..4 {
        assert!(board.place_signpost(0, x, 0, Up));
    }
    assert!(board.signpost_at(0, 0).is_none(), "oldest evicted");
    for x in 1..4 {
        assert!(board.signpost_at(x, 0).is_some());
    }
}

#[test]
fn repointing_refreshes_health_and_age() {
    let mut board = Board::new(8, 1, 0);
    for x in 0..3 {
        assert!(board.place_signpost(0, x, 0, Up));
    }
    // Re-point the oldest post: it becomes the newest...
    assert!(board.place_signpost(0, 0, 0, Down));
    assert_eq!(board.signpost_count(0), 3);
    // ...so the next placement over the cap evicts (1,0), not (0,0).
    assert!(board.place_signpost(0, 4, 0, Up));
    assert!(board.signpost_at(0, 0).is_some());
    assert!(board.signpost_at(1, 0).is_none());
}

/// `out_of_signposts` agrees with the rule it explains, and never fires
/// where a fourth placement would simply take the oldest.
#[test]
fn out_of_signposts_names_the_inventory_refusal_only() {
    let mut board = Board::new(6, 2, 0);
    board.set_signpost_rule(2, CapPolicy::Reject);
    board.set_tile(5, 0, TileKind::Rock);
    assert!(!board.out_of_signposts(0, 0, 0), "nothing placed yet");
    assert!(board.place_signpost(0, 0, 0, Up));
    assert!(board.place_signpost(0, 1, 0, Up));
    // Spent: an empty tile now refuses, and that is the inventory talking.
    assert!(!board.can_place_signpost(0, 2, 0));
    assert!(board.out_of_signposts(0, 2, 0));
    // A rock refuses too, but aiming elsewhere is the answer there.
    assert!(!board.can_place_signpost(0, 5, 0));
    assert!(
        !board.out_of_signposts(0, 5, 0),
        "the tile, not the inventory"
    );
    // Re-pointing one you already own is never refused.
    assert!(!board.out_of_signposts(0, 0, 0));
    assert!(board.place_signpost(0, 0, 0, Down));
    // The other seat has its own inventory.
    assert!(!board.out_of_signposts(1, 2, 0));

    // Under the versus rule the fourth is taken in trade, so there is no
    // refusal to explain.
    let mut versus = Board::new(6, 2, 0);
    for x in 0..3 {
        assert!(versus.place_signpost(0, x, 0, Up));
    }
    assert!(versus.can_place_signpost(0, 4, 0));
    assert!(!versus.out_of_signposts(0, 4, 0), "evicting, not refusing");
}

#[test]
fn signpost_cap_is_per_player() {
    let mut board = Board::new(8, 1, 0);
    for x in 0..3 {
        assert!(board.place_signpost(0, x, 0, Up));
    }
    assert!(board.place_signpost(1, 3, 0, Up));
    // Player 1's placement must not evict player 0's oldest.
    for x in 0..4 {
        assert!(board.signpost_at(x, 0).is_some());
    }
}

#[test]
fn signpost_placement_rules() {
    let mut board = Board::new(5, 5, 0);
    board.set_tile(0, 0, TileKind::Rock);
    board.set_tile(1, 0, TileKind::Castle(0));
    board.set_tile(
        2,
        0,
        TileKind::Spawner(Spawner {
            dir: Down,
            period: 30,
        }),
    );
    assert!(!board.place_signpost(0, 0, 0, Up));
    assert!(!board.place_signpost(0, 1, 0, Up));
    assert!(!board.place_signpost(0, 2, 0, Up));
    assert!(board.place_signpost(0, 3, 0, Up));
    // A rival's signpost blocks the tile...
    assert!(!board.place_signpost(1, 3, 0, Down));
    // ...but your own is re-pointed in place, no Remove needed.
    assert!(board.place_signpost(0, 3, 0, Down));
    assert_eq!(board.signpost_at(3, 0).unwrap().dir, Down);
    assert_eq!(board.signpost_count(0), 1, "re-pointing is not a new post");
    // Out of bounds.
    assert!(!board.place_signpost(0, 200, 0, Up));
}

#[test]
fn only_the_owner_removes_a_signpost() {
    let mut board = Board::new(5, 5, 0);
    assert!(board.place_signpost(0, 2, 2, Up));
    assert!(!board.remove_signpost(1, 2, 2));
    assert!(board.signpost_at(2, 2).is_some());
    assert!(board.remove_signpost(0, 2, 2));
    assert!(board.signpost_at(2, 2).is_none());
}

/// Two seats reaching for one tile on one tick: whoever the tick's
/// [`Board::action_order`] reaches first takes it, and the other's placement
/// fails the occupied-tile check. On a board with no castles there is one
/// seat in play, so the order does not rotate and the lower seat leads.
#[test]
fn a_same_tile_conflict_goes_to_whoever_the_order_reaches_first() {
    let mut board = Board::new(5, 5, 0);
    let mut actions = [PlayerAction::None; MAX_PLAYERS];
    actions[1] = PlayerAction::Place {
        x: 2,
        y: 2,
        dir: Up,
    };
    actions[2] = PlayerAction::Place {
        x: 2,
        y: 2,
        dir: Down,
    };
    board.tick(&actions);
    let sp = board.signpost_at(2, 2).expect("someone placed");
    assert_eq!(sp.owner, 1);
    assert_eq!(sp.dir, Up);
}

/// And on a seated board the lead really does move, so no seat wins every
/// tie: the same conflict on tick 2 of a four-castle beach goes the other
/// way. This is the bias fix `action_order` exists for.
#[test]
fn the_lead_rotates_so_one_seat_does_not_win_every_tie() {
    let mut board = Board::new(5, 5, 0);
    for (seat, &(x, y)) in [(0u8, 0u8), (4, 4), (4, 0), (0, 4)].iter().enumerate() {
        board.set_tile(x, y, TileKind::Castle(seat as u8));
    }
    assert_eq!(board.seats_in_play(), 4);
    board.tick_idle();
    board.tick_idle();
    assert_eq!(board.ticks(), 2);

    let mut actions = [PlayerAction::None; MAX_PLAYERS];
    actions[1] = PlayerAction::Place {
        x: 2,
        y: 2,
        dir: Up,
    };
    actions[2] = PlayerAction::Place {
        x: 2,
        y: 2,
        dir: Down,
    };
    board.tick(&actions);
    let sp = board.signpost_at(2, 2).expect("someone placed");
    assert_eq!(sp.owner, 2, "seat 2 leads this tick, so it takes the tile");
    assert_eq!(sp.dir, Down);
}

#[test]
fn spawner_emits_on_schedule_with_seeded_handedness() {
    let mut board = Board::new(3, 3, 7);
    board.set_tile(
        1,
        1,
        TileKind::Spawner(Spawner {
            dir: Right,
            period: 10,
        }),
    );
    board.tick_idle(); // tick 0 spawns immediately
    assert_eq!(board.crabs().len(), 1);
    for _ in 0..20 {
        board.tick_idle(); // ticks 10 and 20 spawn
    }
    assert_eq!(board.crabs().len(), 3);

    // Same seed reproduces the same handedness sequence.
    let mut again = Board::new(3, 3, 7);
    again.set_tile(
        1,
        1,
        TileKind::Spawner(Spawner {
            dir: Right,
            period: 10,
        }),
    );
    for _ in 0..21 {
        again.tick_idle();
    }
    let handed: Vec<_> = board.crabs().iter().map(|c| c.handed).collect();
    let handed_again: Vec<_> = again.crabs().iter().map(|c| c.handed).collect();
    assert_eq!(handed, handed_again);
}

#[test]
fn walls_are_shared_between_neighbours() {
    let mut board = Board::new(3, 3, 0);
    board.set_wall(1, 1, Right, true);
    // The same edge seen from the other side.
    assert!(board.wall_at(2, 1, Left));
    board.set_wall(2, 1, Left, false);
    assert!(!board.wall_at(1, 1, Right));
}

/// A forced event arms the cooldown too.
///
/// The clock is set in `apply_tide_event` rather than in the roulette on
/// purpose: what holds the wheel is "an event is running", not "the wheel
/// was spun". The dev hook and the tests are the only other way an event
/// starts, and one fired that way must not leave the beach open to a
/// second on the same tick.
#[test]
fn an_event_fired_by_hand_holds_the_wheel_as_well() {
    let mut board = Board::new(9, 7, 2);
    board.set_events_enabled(true);
    board.force_tide_event(TideEvent::FreshSand, 0);
    let first = board.last_event().expect("the forced event landed");
    board.spin_tide_event(0);
    assert_eq!(board.last_event(), Some(first), "the wheel was still held");
}

/// A refused spin must not touch the PRNG.
///
/// The draw that picks the face comes *after* the cooldown check, and it
/// has to: the stream is the round. Moving the check below the draw would
/// consume one per refused Sparkling crab, so two peers that disagreed by
/// a single held spin would then disagree about every crab, gull and
/// event for the rest of the match - and every kept replay would play
/// back as a different round.
#[test]
fn a_held_spin_does_not_spend_a_draw() {
    let mut board = Board::new(9, 7, 2);
    board.set_events_enabled(true);
    board.force_tide_event(TideEvent::SpeedUp, 0);
    let untouched = board.state_hash();
    for _ in 0..25 {
        board.spin_tide_event(0);
    }
    assert_eq!(
        board.state_hash(),
        untouched,
        "a refused spin moved the board: the draw is being spent"
    );
}

/// The wheel comes back the moment the cooldown runs out, and not a tick
/// before. Both ends matter: a cooldown that expired early would let two
/// events overlap after all, and one that never expired would be a mute
/// button on the whole mechanic.
#[test]
fn the_wheel_comes_back_on_the_tick_the_cooldown_ends() {
    let mut board = Board::new(9, 7, 2);
    board.set_events_enabled(true);
    board.force_tide_event(TideEvent::SlowDown, 0);
    let held = board.last_event().expect("the forced event landed");
    // One tick short: still held.
    for _ in 0..(EVENT_COOLDOWN - 1) {
        board.tick_idle();
    }
    board.spin_tide_event(0);
    assert_eq!(board.last_event(), Some(held), "it opened a tick early");
    board.tick_idle();
    board.spin_tide_event(0);
    assert_ne!(board.last_event(), Some(held), "and never opened at all");
}

/// The roulette holds off while an event is still running.
///
/// It is spun by banking a Sparkling crab, and several faces of the wheel
/// (Crab Mania, Speed Up) put more crabs on the beach - so without a valve
/// the events raise their own rate and arrive in waves. This is the valve:
/// one event, then nothing for `EVENT_COOLDOWN`, then the wheel is live
/// again. The crab still banks either way; only the spin is held.
#[test]
fn the_roulette_will_not_spin_on_top_of_a_running_event() {
    let mut board = Board::new(9, 7, 2);
    board.set_events_enabled(true);
    board.force_tide_event(TideEvent::SpeedUp, 0);
    let first = board.last_event().expect("the forced event landed");
    assert_eq!(first.0, TideEvent::SpeedUp);

    // Every spin inside the cooldown is refused, however many crabs bank.
    for _ in 0..40 {
        board.spin_tide_event(0);
        assert_eq!(
            board.last_event(),
            Some(first),
            "a second event started on top of the first"
        );
    }
    // And the wheel comes back once the first event has run its course.
    for _ in 0..EVENT_COOLDOWN {
        board.tick_idle();
    }
    board.spin_tide_event(0);
    assert_ne!(
        board.last_event(),
        Some(first),
        "the wheel never came back: it is a mute button, not a cooldown"
    );
}

/// both cap policies, including the at-cap and rival-post cases.
/// `can_place_signpost` must agree with `place_signpost` on every tile, in
/// both cap policies, including the at-cap and rival-post cases.
#[test]
fn can_place_mirrors_place() {
    for policy in [CapPolicy::Reject, CapPolicy::Evict] {
        let mut board = Board::new(6, 5, 3);
        board.set_signpost_rule(2, policy);
        board.set_tile(4, 0, TileKind::Rock);
        assert!(board.place_signpost(1, 5, 0, Down)); // rival post
        assert!(board.place_signpost(0, 0, 0, Right));
        assert!(board.place_signpost(0, 1, 0, Right)); // player 0 at cap
        for y in 0..board.height() {
            for x in 0..board.width() {
                let predicted = board.can_place_signpost(0, x, y);
                let mut probe = board.clone();
                assert_eq!(
                    probe.place_signpost(0, x, y, Up),
                    predicted,
                    "({x},{y}) under {policy:?}"
                );
            }
        }
    }
}

#[test]
fn signpost_fade_is_full_under_puzzle_rules_and_decays_under_evict() {
    let mut board = Board::new(5, 5, 0);
    // Puzzle rules (Reject): permanent, always full, however old. Set
    // before the post goes down: a fresh board is under Evict, and a
    // fresh post reads full under either rule, so a test that read it at
    // age zero was not reading the Reject branch at all.
    board.set_signpost_rule(3, CapPolicy::Reject);
    assert!(board.place_signpost(0, 2, 2, Right));
    let sp = board.signpost_at(2, 2).unwrap();
    for _ in 0..SIGNPOST_LIFETIME {
        board.tick_idle();
    }
    assert_eq!(board.signpost_fade(&sp), 1.0, "a puzzle post never fades");
    assert!(board.signpost_at(2, 2).is_some(), "nor washes away");

    // Versus rules (Evict): full when fresh, half at half-life, gone at end.
    let mut board = Board::new(5, 5, 0);
    board.set_signpost_rule(3, CapPolicy::Evict);
    assert!(board.place_signpost(0, 2, 2, Right));
    let sp = board.signpost_at(2, 2).unwrap();
    assert_eq!(board.signpost_fade(&sp), 1.0);
    for _ in 0..SIGNPOST_LIFETIME / 2 {
        board.tick_idle();
    }
    let sp = board.signpost_at(2, 2).unwrap();
    let half = board.signpost_fade(&sp);
    assert!((half - 0.5).abs() < 0.01, "half-life fade was {half}");
    for _ in 0..SIGNPOST_LIFETIME / 2 + 1 {
        board.tick_idle();
    }
    assert!(
        board.signpost_at(2, 2).is_none(),
        "post washed away at end of life"
    );
}

#[test]
fn first_signpost_of_scans_in_reading_order() {
    let mut board = Board::new(6, 6, 0);
    assert_eq!(board.first_signpost_of(0), None);
    assert!(board.place_signpost(0, 4, 3, Left));
    assert!(board.place_signpost(0, 1, 3, Up));
    assert!(board.place_signpost(0, 3, 1, Down));
    // Row-major: (3,1) beats both y=3 posts; other players see nothing.
    assert_eq!(board.first_signpost_of(0), Some((3, 1)));
    assert_eq!(board.first_signpost_of(1), None);
}

/// The eat check is a hard boundary at [`EAT_RANGE`] subunits (spec §4.3):
/// exactly in range is eaten, one subunit past is not.
#[test]
fn gulls_eat_exactly_at_the_range_boundary() {
    for (crab_progress, expect_eaten) in [(EAT_RANGE - 8, true), (EAT_RANGE - 7, false)] {
        let mut board = Board::new(5, 5, 0);
        common(&mut board, 2, 2, Down, Handedness::Left);
        board.spawn_gull(2, 2, Right);
        board.crabs[0].progress = crab_progress;
        board.gulls[0].progress = 8;
        // Offsets: gull (8, 0), crab (0, p) -> Manhattan distance 8 + p.
        board.gulls_eat();
        assert_eq!(
            board.crabs.is_empty(),
            expect_eaten,
            "distance {}",
            8 + crab_progress
        );
    }
}

/// Two creatures meeting head-on through the seam of a wrapping arena are
/// a tile apart on the sand and a whole board apart in raw coordinates.
/// The eat check has to measure the sand, or a gull and a crab walk
/// through each other there; the same board without the seam keeps them
/// apart, since there is a wall between them.
#[test]
fn gulls_eat_across_the_seam_of_a_wrapping_board() {
    for wrap in [false, true] {
        let mut board = Board::new(5, 3, 0);
        board.set_wrap(wrap);
        common(&mut board, 4, 1, Right, Handedness::Left);
        board.spawn_gull(0, 1, Left);
        // Gull at x = -120, crab at x = 4 * 256 + 120: sixteen subunits
        // apart through the seam, twelve hundred and more the other way.
        board.crabs[0].progress = 120;
        board.gulls[0].progress = 120;
        board.gulls_eat();
        assert_eq!(board.crabs.is_empty(), wrap, "wrap {wrap}");
    }
}

/// The fold is per axis: the top and bottom edges are a seam too.
#[test]
fn gulls_eat_across_the_top_and_bottom_seam_as_well() {
    for wrap in [false, true] {
        let mut board = Board::new(3, 5, 0);
        board.set_wrap(wrap);
        common(&mut board, 1, 4, Down, Handedness::Left);
        board.spawn_gull(1, 0, Up);
        board.crabs[0].progress = 120;
        board.gulls[0].progress = 120;
        board.gulls_eat();
        assert_eq!(board.crabs.is_empty(), wrap, "wrap {wrap}");
    }
}

/// A lured crab takes the short way to the castle, which on a wrapping
/// board can be through the seam.
#[test]
fn a_lure_pulls_through_the_seam_when_that_is_the_short_way() {
    for (wrap, expect) in [(false, Right), (true, Left)] {
        let mut board = Board::new(5, 3, 0);
        board.set_wrap(wrap);
        board.set_tile(4, 1, TileKind::Castle(0));
        board.force_lure(0);
        let target = board.lure_target();
        assert_eq!(
            board.lure_step(board.index_of(0, 1), target),
            Some(expect),
            "wrap {wrap}"
        );
    }
}

#[test]
fn turnstile_alternates_deflections() {
    let mut board = Board::new(5, 5, 0);
    board.set_tile(2, 2, TileKind::Turnstile { next_right: true });
    // Two crabs cross the log one after the other, both heading Right.
    common(&mut board, 0, 2, Right, Handedness::Left);
    for _ in 0..ticks_to_cross(2, 12) {
        board.tick_idle();
    }
    // First crossing deflects right (screen Down for a Right-walker).
    assert_eq!(board.crabs()[0].dir, Down);
    common(&mut board, 0, 2, Right, Handedness::Left);
    for _ in 0..ticks_to_cross(2, 12) {
        board.tick_idle();
    }
    // Second crossing deflects left (screen Up).
    assert_eq!(board.crabs()[1].dir, Up);
}

#[test]
fn kelp_blocks_walking_gulls_but_not_crabs() {
    // A 3x1 corridor with kelp in the middle: the crab passes through; the
    // gull treats it like a wall (separate boards so nobody gets eaten).
    let mut crabs = Board::new(3, 1, 0);
    crabs.set_tile(1, 0, TileKind::Kelp);
    common(&mut crabs, 0, 0, Right, Handedness::Left);
    for _ in 0..ticks_to_cross(2, 12) + 1 {
        crabs.tick_idle();
    }
    assert_eq!(crabs.crabs()[0].tile, 2, "crab crossed the kelp corridor");

    let mut gulls = Board::new(3, 1, 0);
    gulls.set_tile(1, 0, TileKind::Kelp);
    gulls.spawn_gull(2, 0, Left);
    gulls.tick_idle();
    let gull = gulls.gulls()[0];
    assert_eq!(gull.dir, Right, "gull reversed off the kelp");
    assert_eq!(gull.tile, 2, "and stays put in the dead end");
}

#[test]
fn pools_halve_wading_speed() {
    let mut dry = Board::new(5, 1, 0);
    let mut wet = Board::new(5, 1, 0);
    wet.set_tile(0, 0, TileKind::Pool);
    common(&mut dry, 0, 0, Right, Handedness::Left);
    common(&mut wet, 0, 0, Right, Handedness::Left);
    dry.tick_idle();
    wet.tick_idle();
    assert_eq!(dry.crabs()[0].progress, 12);
    assert_eq!(wet.crabs()[0].progress, 6, "wading is half speed");
}

/// Turnstiles deflect walking gulls exactly like crabs, flipping per
/// crossing - a defensive tool against steered raids.
#[test]
fn turnstile_deflects_walking_gulls_too() {
    let mut board = Board::new(5, 5, 0);
    board.set_tile(2, 2, TileKind::Turnstile { next_right: true });
    board.spawn_gull(0, 2, Right);
    for _ in 0..ticks_to_cross(2, u32::from(GULL_WALK_SPEED)) {
        board.tick_idle();
    }
    assert_eq!(board.gulls()[0].dir, Down, "first crossing deflects right");
    assert_eq!(
        board.tile_at(2, 2),
        TileKind::Turnstile { next_right: false },
        "the log flipped"
    );
}

#[test]
fn gulls_wade_pools_at_half_speed() {
    let mut board = Board::new(5, 1, 0);
    board.set_tile(0, 0, TileKind::Pool);
    board.spawn_gull(0, 0, Right);
    board.tick_idle();
    assert_eq!(board.gulls()[0].progress, GULL_WALK_SPEED / 2);
}

/// A flying gull cannot land in kelp: it glides one more tile, exactly as
/// over a rock.
#[test]
fn flying_gulls_glide_past_kelp() {
    let mut board = Board::new(5, 1, 0);
    board.set_tile(2, 0, TileKind::Kelp);
    board.spawn_gull(1, 0, Right);
    // The walking spawn wall-resolved away from the kelp; flight ignores
    // terrain, so point it back before takeoff.
    board.gulls[0].dir = Right;
    board.gulls[0].state = GullState::Flying { remaining: 1 };
    // One flight hop per arrival: tick until it lands.
    while matches!(board.gulls()[0].state, GullState::Flying { .. }) {
        board.tick_idle();
    }
    assert_eq!(board.gulls()[0].tile, 3, "landed past the kelp, not in it");
}

// --- M4: gulls, castles, molting, tide -------------------------------

#[test]
fn gull_obeys_and_degrades_signposts() {
    let mut board = Board::new(7, 3, 0);
    assert!(board.place_signpost(0, 3, 1, Up));
    board.spawn_gull(0, 1, Right);
    // Gull walks at 8 subunits/tick: 3 tiles in 96 ticks, under the
    // 150-tick takeoff minimum.
    for _ in 0..ticks_to_cross(3, 8) {
        board.tick_idle();
    }
    assert_eq!(board.gulls()[0].dir, Up, "gull follows the signpost");
    assert_eq!(
        board.signpost_at(3, 1).unwrap().health,
        SignpostHealth::Worn
    );
    // A second gull crossing destroys it.
    board.spawn_gull(2, 1, Right);
    for _ in 0..ticks_to_cross(1, 8) {
        board.tick_idle();
    }
    assert!(board.signpost_at(3, 1).is_none());
}

#[test]
fn walking_gull_eats_crab_on_shared_tile() {
    let mut board = Board::new(5, 3, 0);
    common(&mut board, 2, 1, Right, Handedness::Left);
    board.spawn_gull(2, 1, Right);
    board.tick_idle();
    assert!(board.crabs().is_empty(), "crab within eat range is gone");
    assert_eq!(board.gulls().len(), 1);
}

#[test]
fn distant_crab_survives_the_gull() {
    let mut board = Board::new(9, 1, 0);
    common(&mut board, 8, 0, Left, Handedness::Left);
    board.spawn_gull(0, 0, Right);
    board.tick_idle();
    assert_eq!(board.crabs().len(), 1);
}

#[test]
fn gull_raid_halves_the_castle() {
    let mut board = Board::new(7, 5, 0);
    board.set_tile(4, 2, TileKind::Castle(1));
    board.scores[1] = 30;
    board.spawn_gull(0, 2, Right);
    for _ in 0..ticks_to_cross(4, 8) {
        board.tick_idle();
    }
    assert_eq!(board.scores()[1], 15, "half the banked crabs carried off");
    assert_eq!(
        board.crabs().len(),
        SPILL_CAP as usize,
        "the spill cap of live crabs lands on the sand"
    );
    assert!(
        board.gulls().is_empty(),
        "the raider departs the beach with its loot"
    );
}

/// With raids off a gull still walks into the castle and still leaves the
/// beach - it just takes nothing with it.
///
/// The puzzle rule. A raid halves a score the puzzle header never shows and
/// spills banked crabs back onto the sand, and each spilled crab counts as
/// newly spawned - so the "Saved a/b" denominator climbed every time a gull
/// touched the castle, and the level's own target ran away from the player.
/// Nine shipped campaign levels did it while playing the authored solution.
///
/// The departure has to survive: a castle is the one tile that takes a gull
/// off the sand, and the levels lean on it whether or not anyone meant them
/// to. Sealing the castle instead left Two Giants unsolvable.
#[test]
fn a_raid_with_raids_off_takes_nothing_and_still_ends_the_visit() {
    let mut board = Board::new(7, 5, 0);
    board.set_tile(4, 2, TileKind::Castle(1));
    board.set_castle_raids(false);
    board.scores[1] = 30;
    board.spawn_gull(0, 2, Right);
    let spawned_before = board.crabs_spawned();
    for _ in 0..ticks_to_cross(4, 8) {
        board.tick_idle();
    }
    assert_eq!(board.scores()[1], 30, "the bank is not robbed");
    assert_eq!(
        board.crabs_spawned(),
        spawned_before,
        "nothing spills, so the level's target stays where it was"
    );
    assert!(
        board.gulls().is_empty(),
        "the visitor still leaves the beach, which is what the levels rely on"
    );

    // A crab walks the same lane straight in and banks, as ever.
    let mut board = Board::new(7, 5, 0);
    board.set_tile(4, 2, TileKind::Castle(1));
    board.set_castle_raids(false);
    board.spawn_crab(0, 2, Right, Handedness::Left, CrabKind::Common);
    for _ in 0..ticks_to_cross(4, 12) {
        board.tick_idle();
    }
    assert_eq!(board.crabs_banked(), 1, "the castle is still the goal");
}

#[test]
fn empty_castle_hit_is_harmless() {
    let mut board = Board::new(7, 5, 0);
    board.set_tile(4, 2, TileKind::Castle(1));
    board.spawn_gull(0, 2, Right);
    for _ in 0..ticks_to_cross(4, 8) {
        board.tick_idle();
    }
    assert_eq!(board.scores()[1], 0);
    assert!(board.crabs().is_empty());
}

#[test]
fn flight_crosses_walls_and_lands_on_sand() {
    let mut board = Board::new(9, 1, 7);
    board.set_wall(0, 0, Right, true); // walking would bounce off this
    board.spawn_gull(0, 0, Right);
    board.gulls[0].takeoff_in = 0; // force takeoff next tick
    board.gulls[0].dir = Right; // spawn wall-resolve reversed it; fly right
    for _ in 0..80 {
        board.tick_idle();
        if board.gulls[0].state == GullState::Walking && board.gulls[0].tile > 0 {
            break;
        }
    }
    let gull = board.gulls()[0];
    assert_eq!(gull.state, GullState::Walking, "flight ended");
    assert!(
        (2..=4).contains(&gull.tile),
        "landed 2-4 tiles out, past the wall (tile {})",
        gull.tile
    );
}

#[test]
fn molting_bank_lures_loose_crabs_then_expires() {
    let mut board = Board::new(7, 5, 0);
    board.set_tile(6, 2, TileKind::Castle(2));
    board.spawn_crab(5, 2, Right, Handedness::Left, CrabKind::Molting);
    common(&mut board, 0, 2, Up, Handedness::Left);
    let bank_tick = ticks_to_cross(1, 12);
    for _ in 0..bank_tick {
        board.tick_idle();
    }
    let (owner, _) = board.lure().expect("molting bank starts the lure");
    assert_eq!(owner, 2);
    // The loose crab was walking Up; its arrival under the lure turns it
    // toward the castle at (6,2): dx dominates, so Right.
    assert_eq!(board.crabs()[0].dir, Right);
    for _ in 0..LURE_TICKS {
        board.tick_idle();
    }
    assert!(board.lure().is_none(), "lure expires after 10 s");
}

/// The lure does not stack and does not restart on its own heels: a molt
/// banked while one is running, or during the quiet spell after, banks for
/// its points and nothing more. Without this the lure feeds itself, because
/// the crabs it delivers include the next molting crab.
#[test]
fn a_lure_neither_stacks_nor_restarts_at_once() {
    let mut board = Board::new(7, 5, 0);
    board.set_tile(6, 2, TileKind::Castle(0));
    board.set_tile(6, 4, TileKind::Castle(1));
    // Two molts, one for each castle, arriving a few ticks apart.
    board.spawn_crab(5, 2, Right, Handedness::Left, CrabKind::Molting);
    board.spawn_crab(4, 4, Right, Handedness::Left, CrabKind::Molting);
    while board.lure().is_none() {
        board.tick_idle();
    }
    let (owner, started_with) = board.lure().expect("the first molt lures");
    assert_eq!(owner, 0);
    // The second molt banks during the lure: it must not take it over, and
    // must not top the clock back up.
    let mut lowest = started_with;
    for _ in 0..LURE_TICKS - 1 {
        board.tick_idle();
        if let Some((who, left)) = board.lure() {
            assert_eq!(who, 0, "a molt banked mid-lure must not steal it");
            assert!(left <= lowest, "the clock only ever runs down");
            lowest = left;
        }
    }
    board.tick_idle();
    assert!(board.lure().is_none(), "and it ends on time");

    // In the quiet spell a fresh molt banks for points only.
    board.spawn_crab(5, 2, Right, Handedness::Left, CrabKind::Molting);
    let before = board.scores()[0];
    for _ in 0..LURE_COOLDOWN / 2 {
        board.tick_idle();
    }
    assert!(board.scores()[0] > before, "it still banked");
    assert!(board.lure().is_none(), "but started nothing");

    // Once the spell is over, the next molt lures again.
    for _ in 0..LURE_COOLDOWN {
        board.tick_idle();
    }
    board.spawn_crab(5, 2, Right, Handedness::Left, CrabKind::Molting);
    for _ in 0..ticks_to_cross(1, 12) + 2 {
        board.tick_idle();
    }
    assert!(board.lure().is_some(), "the cooldown has passed");
}

/// The ambient spawners fill the beach to a cap and then wait. Crab Mania
/// floods past it, but only to twice the cap.
#[test]
fn the_beach_fills_to_a_cap_and_mania_to_twice_it() {
    let mut board = Board::new(9, 9, 5); // 81 tiles: cap 27, mania 54
    for (x, y, dir) in [(0u8, 4u8, Right), (8, 4, Left)] {
        board.set_tile(x, y, TileKind::Spawner(Spawner { dir, period: 2 }));
    }
    // A castle in the middle of the lane, so the population cycles and the
    // cap is a standing level rather than a one-way fill.
    board.set_tile(4, 4, TileKind::Castle(0));
    board.set_events_enabled(true);
    for _ in 0..1200 {
        board.tick_idle();
    }
    let cap = 81 / 3;
    assert!(
        board.crabs().len() <= cap + 2,
        "ambient spawning stops at the cap, saw {}",
        board.crabs().len()
    );
    assert!(
        board.crabs_spawned() > 100,
        "and it kept spawning as the banked ones made room, saw {}",
        board.crabs_spawned()
    );

    board.apply_tide_event(TideEvent::CrabMania, 0);
    // Watched across the whole flood rather than read at the end of it. The
    // mania lasts `EVENT_TICKS` and the beach drains back to the cap once it
    // lifts, so a single count taken 600 ticks later measures the drain and
    // not the flood: this asserted on the tail, and held only because the
    // tail happened to land above the line.
    let mut peak = board.crabs().len();
    for _ in 0..600 {
        board.tick_idle();
        peak = peak.max(board.crabs().len());
    }
    assert!(peak > cap + 2, "the mania floods past the cap, saw {peak}");
    assert!(peak <= cap * 2 + 4, "but not without bound, saw {peak}");
}

/// The surge already doubles the flock, so the roulette swaps its two gull
/// events for something the players can still route.
#[test]
fn the_roulette_keeps_gulls_out_of_the_surge() {
    let mut board = party_board();
    board.set_round_length(Some(1000));
    assert!(!board.in_surge());
    board.apply_tide_event(TideEvent::GullMania, 0);
    assert_eq!(
        board.last_event().map(|(e, _)| e),
        Some(TideEvent::GullMania),
        "outside the surge it stands"
    );

    // Into the last 30 seconds, where a spin lands on the gull events.
    let mut surging = party_board();
    surging.set_round_length(Some(200));
    surging.set_score(0, 1); // a sparkling bank needs a castle owner
    for _ in 0..100 {
        surging.tick_idle();
    }
    assert!(surging.in_surge());
    // The spin's filter, asked outright: the wheel itself draws from the
    // PRNG, and which face it lands on is not the point.
    assert_eq!(
        surging.surge_safe(TideEvent::GullMania),
        TideEvent::CrabMania
    );
    assert_eq!(
        surging.surge_safe(TideEvent::GullAttack),
        TideEvent::SpeedUp
    );
    for kept in [
        TideEvent::CrabMania,
        TideEvent::Monopoly,
        TideEvent::SpeedUp,
        TideEvent::SlowDown,
        TideEvent::FreshSand,
        TideEvent::CastleSwap,
    ] {
        assert_eq!(surging.surge_safe(kept), kept, "{kept:?} stands");
    }
    assert_eq!(
        board.surge_safe(TideEvent::GullMania),
        TideEvent::GullMania,
        "outside the surge"
    );
    // Applying one directly is still honoured; only the spin is filtered.
    surging.apply_tide_event(TideEvent::GullMania, 0);
    assert_eq!(
        surging.last_event().map(|(e, _)| e),
        Some(TideEvent::GullMania)
    );
}

#[test]
fn tide_freezes_the_sim_and_surges_gulls() {
    let mut board = Board::new(7, 5, 3);
    board.set_gull_period(500);
    board.set_round_length(Some(2000));
    common(&mut board, 0, 0, Right, Handedness::Left);
    for _ in 0..1099 {
        board.tick_idle();
    }
    assert!(!board.in_surge(), "901 ticks left: not yet the scramble");
    board.tick_idle();
    assert!(board.in_surge(), "900 ticks left: final scramble");
    for _ in 0..900 {
        board.tick_idle();
    }
    assert!(board.round_over());
    let frozen = board.state_hash();
    for _ in 0..10 {
        board.tick_idle();
    }
    assert_eq!(board.state_hash(), frozen, "scores locked at the wave");
}

#[test]
fn versus_signposts_fade_away() {
    let mut board = Board::new(5, 3, 0); // default Evict rules
    assert!(board.place_signpost(0, 2, 1, Up));
    for _ in 0..SIGNPOST_LIFETIME {
        board.tick_idle();
    }
    assert!(
        board.signpost_at(2, 1).is_some(),
        "alive through its lifetime"
    );
    board.tick_idle();
    assert!(board.signpost_at(2, 1).is_none(), "faded one tick after");
}

#[test]
fn puzzle_signposts_are_permanent() {
    let mut board = Board::new(5, 3, 0);
    board.set_signpost_rule(3, CapPolicy::Reject);
    assert!(board.place_signpost(0, 2, 1, Up));
    for _ in 0..SIGNPOST_LIFETIME * 2 {
        board.tick_idle();
    }
    assert!(board.signpost_at(2, 1).is_some());
}

#[test]
fn wrapped_edges_carry_crabs_across() {
    let mut board = Board::new(3, 3, 0);
    board.set_wrap(true);
    common(&mut board, 2, 1, Right, Handedness::Left);
    for _ in 0..ticks_to_cross(1, 12) {
        board.tick_idle();
    }
    let crab = board.crabs()[0];
    assert_eq!(crab.tile, 3, "walked off the east edge onto (0,1)");
    assert_eq!(crab.dir, Right);
}

/// The public step the bot plans with agrees with the beach the crabs walk:
/// off the edge is nowhere on a plain board, and the far side on a wrapping
/// one. The bot used to stop at the seam while the crabs crossed it.
#[test]
fn a_step_crosses_the_seam_exactly_when_the_beach_wraps() {
    let mut board = Board::new(6, 5, 0);
    let corner = 0u16; // (0,0)
    assert_eq!(board.step(corner, Left), None);
    assert_eq!(board.step(corner, Up), None);
    assert_eq!(board.step(corner, Right), Some(1));
    assert_eq!(board.step(corner, Down), Some(6));
    board.set_wrap(true);
    assert_eq!(board.step(corner, Left), Some(5), "west seam to (5,0)");
    assert_eq!(board.step(corner, Up), Some(24), "north seam to (0,4)");
}

#[test]
fn sparkling_bank_spins_events_only_when_enabled() {
    let mut quiet = Board::new(5, 3, 9);
    quiet.set_tile(4, 1, TileKind::Castle(0));
    quiet.spawn_crab(3, 1, Right, Handedness::Left, CrabKind::Sparkling);
    for _ in 0..ticks_to_cross(1, 12) {
        quiet.tick_idle();
    }
    assert!(quiet.last_event().is_none(), "events off by default");

    let mut party = Board::new(5, 3, 9);
    party.set_events_enabled(true);
    party.set_tile(4, 1, TileKind::Castle(0));
    party.spawn_crab(3, 1, Right, Handedness::Left, CrabKind::Sparkling);
    for _ in 0..ticks_to_cross(1, 12) {
        party.tick_idle();
    }
    let (event, at) = party.last_event().expect("roulette spun");
    assert!(at < party.ticks());
    // The event actually did something observable for the effects that
    // leave immediate state (others set timers).
    match event {
        TideEvent::SpeedUp | TideEvent::SlowDown => {
            assert!(party.state_hash() != quiet.state_hash());
        }
        TideEvent::CrabMania
        | TideEvent::GullMania
        | TideEvent::Monopoly
        | TideEvent::GullAttack
        | TideEvent::FreshSand
        | TideEvent::CastleSwap => {}
    }
}

// --- tide events, one by one -------------------------------------

fn party_board() -> Board {
    let mut board = Board::new(7, 5, 11);
    board.set_events_enabled(true);
    board.set_tile(1, 1, TileKind::Castle(0));
    board.set_tile(5, 3, TileKind::Castle(1));
    board
}

#[test]
fn event_crab_mania_clears_gulls_and_floods() {
    let mut board = party_board();
    board.set_tile(
        0,
        2,
        TileKind::Spawner(Spawner {
            dir: Right,
            period: 100,
        }),
    );
    board.spawn_gull(3, 2, Right);
    board.apply_tide_event(TideEvent::CrabMania, 0);
    assert!(board.gulls().is_empty(), "gulls washed away");
    let before = board.crabs().len();
    for _ in 0..64 {
        board.tick_idle();
    }
    assert!(
        board.crabs().len() >= before + 8,
        "flood cadence: 8 spawns in 64 ticks"
    );
}

#[test]
fn event_gull_mania_spawns_gulls_from_holes() {
    let mut board = party_board();
    board.set_tile(
        0,
        2,
        TileKind::Spawner(Spawner {
            dir: Right,
            period: 100,
        }),
    );
    board.apply_tide_event(TideEvent::GullMania, 0);
    for _ in 0..64 {
        board.tick_idle();
    }
    assert!(
        board.gulls().len() >= 8,
        "holes emit gulls during the mania"
    );
}

#[test]
fn event_monopoly_banks_half_the_loose_crabs() {
    let mut board = party_board();
    for x in 0..6 {
        common(&mut board, x, 4, Right, Handedness::Left);
    }
    board.apply_tide_event(TideEvent::Monopoly, 1);
    assert_eq!(board.crabs().len(), 3);
    assert_eq!(board.scores()[1], 3);
    assert_eq!(board.crabs_banked(), 3);
}

/// A crab the tide banks goes from a tile that is not a castle, which
/// from outside the sim is exactly what being eaten looks like. So the sim
/// writes down what it did, because nothing else can work it out
/// afterwards.
#[test]
fn a_swept_crab_is_written_down_against_the_seat_that_got_it() {
    let mut board = party_board();
    for x in 0..6 {
        common(&mut board, x, 4, Right, Handedness::Left);
    }
    let taken: Vec<u32> = board.crabs()[..3].iter().map(|crab| crab.id).collect();
    let left: Vec<u32> = board.crabs()[3..].iter().map(|crab| crab.id).collect();
    board.apply_tide_event(TideEvent::Monopoly, 1);
    for crab in taken {
        assert_eq!(
            board.swept_home(crab),
            Some(1),
            "crab {crab} went to seat 1"
        );
    }
    for crab in left {
        assert_eq!(board.swept_home(crab), None, "crab {crab} is still walking");
    }
    // And a crab nobody has ever heard of is nobody's.
    assert_eq!(board.swept_home(9999), None);
}

/// The record is news about one tick, and it is read between frames - a
/// frame that ran long can cover several ticks, and the frame straight
/// after a sweep is the busiest one the renderer ever draws. So it
/// outlives its own tick, and then it goes.
#[test]
fn a_swept_crab_is_remembered_a_few_ticks_and_then_forgotten() {
    let mut board = party_board();
    for x in 0..6 {
        common(&mut board, x, 4, Right, Handedness::Left);
    }
    let crab = board.crabs()[0].id;
    board.apply_tide_event(TideEvent::Monopoly, 1);
    assert_eq!(board.swept_home(crab), Some(1), "the tick it happened on");
    board.tick_idle();
    assert_eq!(board.swept_home(crab), Some(1), "and the frame after it");
    for _ in 0..super::SWEEP_MEMORY {
        board.tick_idle();
    }
    assert_eq!(board.swept_home(crab), None, "but not for the round");
}

/// The record is written for the render layer and never read back by the
/// sim, so it has no business in the fingerprint. Two peers playing the
/// same round have to agree on their hashes, and a field that moved one of
/// them would be a desync neither could see.
///
/// Put in by hand rather than by playing a round into it: the question is
/// whether the fingerprint can feel it at all, and the fingerprint is the
/// only thing being asked.
#[test]
fn a_sweep_leaves_the_fingerprint_alone() {
    let mut board = party_board();
    for x in 0..6 {
        common(&mut board, x, 4, Right, Handedness::Left);
    }
    let before = board.state_hash();
    board.swept_home.push(super::Swept {
        crab: 7,
        owner: 1,
        at: 0,
    });
    assert_eq!(board.swept_home(7), Some(1), "the record is really there");
    assert_eq!(
        board.state_hash(),
        before,
        "and the fingerprint cannot feel it"
    );
}

#[test]
fn event_gull_attack_targets_rival_castles_only() {
    let mut board = party_board();
    board.apply_tide_event(TideEvent::GullAttack, 0);
    assert_eq!(board.gulls().len(), 1, "one gull, at the rival's castle");
    let (gx, gy) = (board.gulls()[0].tile % 7, board.gulls()[0].tile / 7);
    let dist = (i32::from(gx) - 5).abs() + (i32::from(gy) - 3).abs();
    assert_eq!(dist, 1, "landed adjacent to the rival castle");
}

#[test]
fn event_tempo_doubles_and_halves_movement() {
    let mut fast = party_board();
    common(&mut fast, 0, 4, Right, Handedness::Left);
    fast.apply_tide_event(TideEvent::SpeedUp, 0);
    for _ in 0..11 {
        fast.tick_idle();
    }
    assert_eq!(fast.crabs()[0].tile, 4 * 7 + 1, "24/tick crosses in 11");

    let mut slow = party_board();
    common(&mut slow, 0, 4, Right, Handedness::Left);
    slow.apply_tide_event(TideEvent::SlowDown, 0);
    for _ in 0..42 {
        slow.tick_idle();
    }
    assert_eq!(slow.crabs()[0].tile, 4 * 7, "6/tick still mid-tile at 42");
    slow.tick_idle();
    assert_eq!(slow.crabs()[0].tile, 4 * 7 + 1);
}

#[test]
fn event_fresh_sand_washes_all_signposts() {
    let mut board = party_board();
    assert!(board.place_signpost(0, 3, 2, Up));
    assert!(board.place_signpost(1, 4, 2, Down));
    board.apply_tide_event(TideEvent::FreshSand, 0);
    assert_eq!(board.signpost_count(0) + board.signpost_count(1), 0);
}

#[test]
fn event_castle_swap_rotates_owners() {
    let mut board = party_board();
    board.apply_tide_event(TideEvent::CastleSwap, 0);
    assert_eq!(board.tile_at(1, 1), TileKind::Castle(1));
    assert_eq!(board.tile_at(5, 3), TileKind::Castle(0));
}

#[test]
fn event_timers_expire() {
    let mut board = party_board();
    board.apply_tide_event(TideEvent::SpeedUp, 0);
    common(&mut board, 0, 4, Right, Handedness::Left);
    for _ in 0..EVENT_TICKS + 1 {
        board.tick_idle();
    }
    // Back to normal speed: a fresh crab crosses in 22 ticks again.
    common(&mut board, 0, 2, Right, Handedness::Left);
    let start = board.crabs().last().unwrap().tile;
    for _ in 0..21 {
        board.tick_idle();
    }
    assert_eq!(board.crabs().last().unwrap().tile, start, "12/tick again");
}

#[test]
fn repointing_restarts_the_fade_clock() {
    let mut board = Board::new(5, 3, 0);
    assert!(board.place_signpost(0, 2, 1, Up));
    for _ in 0..200 {
        board.tick_idle();
    }
    assert!(board.place_signpost(0, 2, 1, Down), "re-point in place");
    for _ in 0..200 {
        board.tick_idle();
    }
    assert!(
        board.signpost_at(2, 1).is_some(),
        "clock restarted at re-point"
    );
    for _ in 0..SIGNPOST_LIFETIME {
        board.tick_idle();
    }
    assert!(board.signpost_at(2, 1).is_none());
}

#[test]
fn wrapped_flight_crosses_the_edge() {
    let mut board = Board::new(5, 1, 3);
    board.set_wrap(true);
    board.spawn_gull(4, 0, Right);
    board.gulls[0].takeoff_in = 0;
    board.gulls[0].dir = Right;
    for _ in 0..80 {
        board.tick_idle();
        if board.gulls[0].state == GullState::Walking {
            break;
        }
    }
    assert!(
        board.gulls[0].tile < 4,
        "flew across the open edge (tile {})",
        board.gulls[0].tile
    );
}

#[test]
fn castle_tier_boundaries() {
    for (score, tier) in [
        (0, 0),
        (9, 0),
        (10, 1),
        (24, 1),
        (25, 2),
        (49, 2),
        (50, 3),
        (5000, 3),
    ] {
        assert_eq!(castle_tier(score), tier, "score {score}");
    }
}

#[test]
fn surge_doubles_gull_spawn_rate() {
    // Surged board: surge covers remaining <= 900, i.e. from tick 100 on,
    // halving the period to 50 (spawns at 0, 100, 150, 200, 250 -> capped).
    let mut board = Board::new(7, 5, 3);
    board.set_gull_period(100);
    board.set_round_length(Some(1000));
    // Control at the normal rate.
    let mut calm = Board::new(7, 5, 3);
    calm.set_gull_period(100);
    for _ in 0..320 {
        board.tick_idle();
        calm.tick_idle();
    }
    assert_eq!(
        board.gulls().len(),
        GULL_CAP,
        "surged spawns hit the ambient flock cap"
    );
    assert_eq!(calm.gulls().len(), 4, "un-surged control spawns at period");
    // The cap holds from here on: no ambient spawn can exceed it.
    for _ in 0..300 {
        board.tick_idle();
    }
    assert!(board.gulls().len() <= GULL_CAP);
}

/// Tide events bypass the ambient flock cap on purpose: GullMania floods the
/// beach through the crab spawners.
#[test]
fn gull_mania_ignores_the_flock_cap() {
    let mut board = Board::new(9, 7, 5);
    board.set_tile(
        0,
        3,
        TileKind::Spawner(Spawner {
            dir: Right,
            period: 40,
        }),
    );
    board.set_events_enabled(true);
    board.apply_tide_event(TideEvent::GullMania, 0);
    for _ in 0..200 {
        board.tick_idle();
    }
    assert!(
        board.gulls().len() > GULL_CAP,
        "mania floods past the cap, got {}",
        board.gulls().len()
    );
}

/// Spec §3.4 says a raid halves the score "rounding the loss up": the
/// castle keeps `score / 2` and the odd crab goes with the gull. A score of
/// 7 keeps 3 and loses 4, all four spilling back onto the sand (under the
/// spill cap); 3 keeps 1 and spills 2. Pinned so a later "round to nearest"
/// cannot creep in and quietly change how devastating a raid on a small
/// castle is.
#[test]
fn a_raid_on_an_odd_score_rounds_the_loss_up_and_spills_it() {
    for (score, kept, spilled) in [(7, 3, 4), (3, 1, 2)] {
        let mut board = Board::new(7, 5, 0);
        board.set_tile(3, 2, TileKind::Castle(0));
        let castle = board.index_of(3, 2);
        board.scores[0] = score;
        board.damage_castle(0, castle);
        assert_eq!(board.scores()[0], kept, "{score} halves to {kept}");
        assert_eq!(
            board.crabs().len(),
            spilled,
            "{score}: the lost crabs spill"
        );
    }
}

/// A castle in a corner with a rock on its diagonal has only two open ring
/// tiles, fewer than the crabs a raid wants to spill. The spill takes what
/// sand there is and the flock carries the rest off: nothing spawns off the
/// board or on the rock, and the score still drops by the full half.
#[test]
fn a_spill_into_a_cramped_corner_lands_only_on_the_open_sand() {
    let mut board = Board::new(3, 3, 0);
    board.set_tile(0, 0, TileKind::Castle(0));
    board.set_tile(1, 1, TileKind::Rock);
    let castle = board.index_of(0, 0);
    board.scores[0] = 7;
    board.damage_castle(0, castle);
    assert_eq!(board.scores()[0], 3, "the loss is the full half regardless");
    let landed: Vec<(u8, u8)> = board
        .crabs()
        .iter()
        .map(|c| board.coords_u8(c.tile))
        .collect();
    assert_eq!(
        landed.len(),
        2,
        "only two ring tiles are open sand: {landed:?}"
    );
    for &(x, y) in &landed {
        assert!(x < 3 && y < 3, "spilled crab off the board at ({x},{y})");
        assert!(
            matches!((x, y), (1, 0) | (0, 1)),
            "spilled crab on the castle or the rock at ({x},{y})"
        );
    }
}

/// Where the ambient flock comes from: every spawn is on the edge of the
/// board and walks inward, and over enough rolls every edge tile gets its
/// turn. The perimeter arithmetic has four branches (top, bottom, left,
/// right) and an off-by-one in any of them either skips tiles or, worse,
/// indexes past the board.
#[test]
fn the_gull_spawner_covers_the_whole_perimeter_and_faces_inward() {
    let mut board = Board::new(4, 3, 7);
    board.set_gull_period(1);
    let mut hit = std::collections::BTreeSet::new();
    for _ in 0..400 {
        board.run_gull_spawner();
        let gull = board.gulls()[0];
        let (x, y) = board.coords(gull.tile);
        assert!(
            x == 0 || y == 0 || x == 3 || y == 2,
            "spawned inside the board at ({x},{y})"
        );
        let (dx, dy) = gull.dir.offset();
        assert!(
            board.in_bounds(x + dx, y + dy),
            "spawned at ({x},{y}) facing {:?}, off the board",
            gull.dir
        );
        hit.insert((x, y));
        board.gulls.clear();
    }
    assert_eq!(
        hit.len(),
        10,
        "every perimeter tile of a 4x3 board: {hit:?}"
    );
}

/// A one-wide or one-tall board is all perimeter, and the spawner's
/// `w * h` branch has to say so: every tile gets a gull eventually, and
/// none of the four edge branches names a tile the strip does not have.
/// There is no "inward" on a strip; the spawn's wall resolution turns the
/// gull along it, and that is asserted as "facing a tile that exists".
#[test]
fn the_gull_spawner_walks_the_length_of_a_strip() {
    for (w, h) in [(1u8, 5u8), (5, 1)] {
        let mut board = Board::new(w, h, 3);
        board.set_gull_period(1);
        let mut hit = std::collections::BTreeSet::new();
        for _ in 0..200 {
            board.run_gull_spawner();
            let gull = board.gulls()[0];
            let (x, y) = board.coords(gull.tile);
            let (dx, dy) = gull.dir.offset();
            assert!(
                board.in_bounds(x + dx, y + dy),
                "{w}x{h}: spawned at ({x},{y}) facing {:?}, off the board",
                gull.dir
            );
            hit.insert((x, y));
            board.gulls.clear();
        }
        assert_eq!(hit.len(), 5, "{w}x{h}: every tile of the strip: {hit:?}");
    }
}

/// A gull in flight, with its `remaining` hops given.
fn flying(board: &Board, x: u8, y: u8, dir: Direction, remaining: u8) -> Gull {
    let tile = board.index_of(x, y);
    Gull {
        id: 99,
        tile,
        dir,
        progress: 0,
        prev: crate::sim::Pose {
            tile,
            dir,
            progress: 0,
        },
        handed: Handedness::Left,
        state: GullState::Flying { remaining },
        takeoff_in: 0,
    }
}

/// Spec §3.5: a flying gull bounces off the board edge and lands only on a
/// tile a creature can stand on. On a 4x1 strip with rocks on the far two
/// tiles, a gull flying right with two hops left runs out over the rocks,
/// glides one more tile, hits the edge, turns back and puts down on the
/// first sand it finds. It has to end walking, on sand, in a bounded number
/// of ticks: a gull that flies forever is a gull that eats nothing all
/// round.
#[test]
fn a_flying_gull_bounces_off_the_edge_and_lands_on_sand() {
    let mut board = Board::new(4, 1, 0);
    board.set_tile(2, 0, TileKind::Rock);
    board.set_tile(3, 0, TileKind::Rock);
    let gull = flying(&board, 0, 0, Right, 2);
    board.gulls.push(gull);
    // Five hops (1, 2, 3, back to 2, back to 1) at one tile per 16 ticks.
    let hops = 5 * ticks_to_cross(1, u32::from(GULL_FLY_SPEED));
    for _ in 0..hops {
        board.tick_idle();
        if board.gulls()[0].state == GullState::Walking {
            break;
        }
    }
    let gull = board.gulls()[0];
    assert_eq!(
        gull.state,
        GullState::Walking,
        "still airborne after {hops} ticks"
    );
    let (x, y) = board.coords_u8(gull.tile);
    assert_eq!(board.tile_at(x, y), TileKind::Empty, "landed on a rock");
    assert_eq!((x, y), (1, 0), "the first sand on the way back");
    assert!(gull.takeoff_in > 0, "a fresh takeoff timer on landing");
}

/// The one-tile board: a gull that takes off has nowhere to fly, forward
/// or back, and has to land where it stands rather than hop off the edge
/// into a tile that does not exist.
#[test]
fn a_gull_on_a_one_tile_board_has_nowhere_to_fly_and_lands_where_it_is() {
    let mut board = Board::new(1, 1, 0);
    let gull = flying(&board, 0, 0, Right, 3);
    board.gulls.push(gull);
    for _ in 0..ticks_to_cross(1, u32::from(GULL_FLY_SPEED)) {
        board.tick_idle();
    }
    let gull = board.gulls()[0];
    assert_eq!(gull.state, GullState::Walking, "landed on the only tile");
    assert_eq!(gull.tile, 0);
    assert!(gull.takeoff_in > 0, "a fresh takeoff timer on landing");
}

/// The bot's cursor anchor: a player's newest post is the one with the
/// highest sequence number, and re-pointing a post in place takes a fresh
/// number. Evicting the oldest at the cap and a rival's posts must both
/// leave the answer alone, or a bot pays for a walk it never made.
#[test]
fn the_newest_signpost_is_by_sequence_and_ignores_rivals_and_evictions() {
    let mut board = Board::new(6, 6, 0);
    assert_eq!(board.newest_signpost_of(0), None, "nothing placed yet");
    assert!(board.place_signpost(0, 1, 1, Right));
    board.tick_idle();
    assert!(board.place_signpost(0, 2, 2, Right));
    assert_eq!(board.newest_signpost_of(0), Some((2, 2, 1)));
    // Re-pointing the first post makes it the newest again, stamped now.
    board.tick_idle();
    assert!(board.place_signpost(0, 1, 1, Down));
    assert_eq!(board.newest_signpost_of(0), Some((1, 1, 2)));
    // A rival's post, however fresh, is not this player's.
    board.tick_idle();
    assert!(board.place_signpost(1, 4, 4, Left));
    assert_eq!(board.newest_signpost_of(0), Some((1, 1, 2)));
    assert_eq!(board.newest_signpost_of(1), Some((4, 4, 3)));
    // A third fills the cap; a fourth evicts the oldest, which is (2, 2)
    // now that (1, 1) was re-pointed, and the fourth is the newest.
    assert!(board.place_signpost(0, 3, 3, Up));
    assert!(board.place_signpost(0, 5, 5, Up));
    assert!(board.signpost_at(2, 2).is_none(), "the oldest was evicted");
    assert!(
        board.signpost_at(1, 1).is_some(),
        "the re-pointed post survived"
    );
    assert_eq!(board.newest_signpost_of(0), Some((5, 5, 3)));
    // Losing a rival's post changes nothing for this player.
    assert!(board.remove_signpost(1, 4, 4));
    assert_eq!(board.newest_signpost_of(0), Some((5, 5, 3)));
    assert_eq!(board.newest_signpost_of(1), None);
}
