//! A whole 4-bot round runs to completion: the closest thing to an
//! integration test of the versus loop the sim can do headless.

use pinch_points::sim::{
    BotLevel, MAX_PLAYERS, MAX_SIGNPOSTS_PER_PLAYER, PlayerAction, bot_action,
    classic_arena_seeded, generate_arena,
};

#[test]
fn four_hard_bots_finish_a_round_with_a_result() {
    let mut board = classic_arena_seeded(0xB07_5EED, false, 4);
    let mut safety = 0u32;
    while !board.round_over() {
        let mut actions = [PlayerAction::None; MAX_PLAYERS];
        for seat in 0..4u8 {
            actions[seat as usize] = bot_action(&board, seat, BotLevel::Hard);
        }
        board.tick(&actions);
        safety += 1;
        assert!(safety < 20_000, "round never ended");
    }
    assert!(
        board.crabs_banked() > 0,
        "a full round of fierce bots banked nothing"
    );
    let total: u32 = board.scores().iter().sum();
    assert!(total > 0, "no seat scored in a full round");
}

/// A bot must never propose something the board would refuse.
///
/// The sim simply drops an illegal action, so a bot that produced them
/// would not crash - it would quietly play a worse game than its level
/// claims, on some maps and not others, and nothing would ever say so.
#[test]
fn a_bot_never_proposes_a_placement_the_board_refuses() {
    for (seats, seed) in [(2u8, 1u64), (4, 2), (6, 3)] {
        for level in [BotLevel::Easy, BotLevel::Normal, BotLevel::Hard] {
            // The classic arena seats four; six seats want a generated
            // beach, or the last two bots play with no castle of their own
            // and the six-seat leg tests nothing the four-seat one did not.
            let mut board = if seats <= 4 {
                classic_arena_seeded(seed, false, seats)
            } else {
                generate_arena(seed, seats, 20, 13)
            };
            for tick in 0..1500u32 {
                let mut actions = [PlayerAction::None; MAX_PLAYERS];
                for seat in 0..seats {
                    let action = bot_action(&board, seat, level);
                    if let PlayerAction::Place { x, y, .. } = action {
                        assert!(
                            board.can_place_signpost(seat, x, y),
                            "{level:?} bot on seat {seat} aimed at ({x},{y}) at tick {tick}, \
                             which the board refuses"
                        );
                    }
                    actions[seat as usize] = action;
                }
                board.tick(&actions);
            }
        }
    }
}

/// And never holds more posts than the rules allow it.
///
/// The cap is the whole of the versus economy: a seat that could quietly
/// keep a fourth post would be playing a different game from the players
/// beside it, and the board enforces the cap rather than the bot, so the
/// bot asking for too many would show up only as a bot that wastes turns.
#[test]
fn a_bot_never_holds_more_posts_than_the_rules_allow() {
    let mut board = classic_arena_seeded(0xB0_7CA9, false, 4);
    let cap = MAX_SIGNPOSTS_PER_PLAYER;
    for _ in 0..3000u32 {
        let mut actions = [PlayerAction::None; MAX_PLAYERS];
        for seat in 0..4u8 {
            actions[seat as usize] = bot_action(&board, seat, BotLevel::Hard);
        }
        board.tick(&actions);
        for seat in 0..4u8 {
            assert!(
                board.signpost_count(seat) <= cap,
                "seat {seat} is holding {} posts, over the cap of {cap}",
                board.signpost_count(seat)
            );
        }
    }
}

/// Every beach the game ships plays out under bots and reaches the wave.
///
/// A map that stalled - no route from a spawner to any castle, a seat
/// walled off from the sand - would look fine in the editor and be a dead
/// round in the living room. The bots are the only thing that walks every
/// shipped arena end to end.
#[test]
fn every_shipped_arena_plays_out_under_bots() {
    for seats in [2u8, 3, 4] {
        let mut board = classic_arena_seeded(u64::from(seats), false, seats);
        board.set_round_length(Some(1800));
        let mut safety = 0u32;
        while !board.round_over() {
            let mut actions = [PlayerAction::None; MAX_PLAYERS];
            for seat in 0..seats {
                actions[seat as usize] = bot_action(&board, seat, BotLevel::Normal);
            }
            board.tick(&actions);
            safety += 1;
            assert!(safety < 20_000, "{seats} seats: the round never ended");
        }
        assert!(
            board.crabs_banked() > 0,
            "{seats} seats: a whole round banked nothing, so the beach has no route home"
        );
    }
}
