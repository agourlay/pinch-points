//! A whole 4-bot round runs to completion: the closest thing to an
//! integration test of the versus loop the sim can do headless.

use pinch_points::sim::{BotLevel, MAX_PLAYERS, PlayerAction, bot_action, classic_arena_seeded};

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
