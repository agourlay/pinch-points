//! `simulate`: a lookahead the listener answers for every bot, so the edge
//! of running the sim forward is not something only a Rust bot has.
//!
//! The copy it runs is [`Board::lookahead`]'s: everything on the sand
//! exactly, the public schedules kept, and what the PRNG would decide
//! replaced by a draw nobody can steer. Rivals stand idle; the plan is the
//! asking seat's alone.

use super::diff::{Before, Happened};
use super::listener::View;
use super::protocol::{self, Act, Planned, You};
use crate::sim::{Board, MAX_PLAYERS, PlayerAction, PlayerId};
use serde_json::{Value, json};

/// The fixed draw a lookahead's dice come from. Fixed, so the same request
/// on the same board answers the same way every time it is asked.
pub const LOOKAHEAD_SEED: u64 = 0x10_0CA4_EAD5;

/// Lookahead ticks per decision under a fast-forward clock and a live one.
/// Live, the listener is the machine drawing a party's game.
pub const CAP_FAST: u32 = 600;
pub const CAP_LIVE: u32 = 60;

/// The sim's action for an act, on `board`, for `seat`.
pub fn sim_action(board: &Board, seat: PlayerId, act: Act) -> PlayerAction {
    match act {
        Act::None | Act::Move { .. } => PlayerAction::None,
        Act::Place { x, y, dir } => PlayerAction::Place { x, y, dir },
        Act::Remove { x, y } => PlayerAction::Remove { x, y },
        // The clear-all key: the seat's first signpost in reading order,
        // one a tick, as the held key does for a person.
        Act::Clear => board
            .first_signpost_of(seat)
            .map_or(PlayerAction::None, |(x, y)| PlayerAction::Remove { x, y }),
    }
}

/// What happened, as the protocol reports it.
pub fn happened_json(tick: u64, what: Happened) -> Value {
    match what {
        Happened::Banked { crab, owner, x, y } => {
            json!({"tick": tick, "what": "banked", "crab": crab, "owner": owner, "x": x, "y": y})
        }
        Happened::Eaten { crab, x, y } => {
            json!({"tick": tick, "what": "eaten", "crab": crab, "x": x, "y": y})
        }
        Happened::Raided { owner, lost } => {
            json!({"tick": tick, "what": "raided", "owner": owner, "lost": lost})
        }
        Happened::PostWorn { x, y, owner } => {
            json!({"tick": tick, "what": "post_worn", "x": x, "y": y, "owner": owner})
        }
        Happened::PostGone { x, y, owner } => {
            json!({"tick": tick, "what": "post_gone", "x": x, "y": y, "owner": owner})
        }
    }
}

/// Answer one `simulate` for `seat`, charging its budget for this tick.
pub fn answer(view: &mut View, game: u32, seat: PlayerId, ticks: u32, plan: &[Planned]) -> Value {
    let Some(budget) = view.budget.get_mut(usize::from(seat)) else {
        return json!({"type": "error", "message": "no such seat"});
    };
    if *budget == 0 {
        return json!({
            "type": "error", "game": game,
            "message": "lookahead budget for this tick is spent",
        });
    }
    let ticks = ticks.min(*budget);
    *budget -= ticks;
    let (end, events) = run(&view.board, seat, ticks, plan);
    // A lookahead stops where the round does.
    let ticks = end.ticks() - view.board.ticks();
    let mut state = protocol::tick(&end, game, &You { seat, last: None }, &view.cursors);
    // Crabs the copy spawned are a guess about their kind and claw: when
    // they come is public, what they are is not.
    let known = view.board.crabs_spawned();
    if let Some(crabs) = state["crabs"].as_array_mut() {
        for crab in crabs {
            if crab["id"].as_u64().is_some_and(|id| id >= u64::from(known)) {
                crab["predicted"] = json!(true);
            }
        }
    }
    json!({
        "type": "simulated", "game": game, "ticks": ticks,
        "state": state, "events": events,
    })
}

/// Run `board`'s lookahead copy `ticks` forward with `plan` for `seat`.
pub fn run(board: &Board, seat: PlayerId, ticks: u32, plan: &[Planned]) -> (Board, Vec<Value>) {
    let mut copy = board.lookahead(LOOKAHEAD_SEED);
    let mut events = Vec::new();
    let mut seen = Vec::new();
    for i in 0..ticks {
        if copy.round_over() {
            break;
        }
        let mut actions = [PlayerAction::None; MAX_PLAYERS];
        if let Some(step) = plan.iter().find(|p| p.at == i)
            && let Some(slot) = actions.get_mut(usize::from(seat))
        {
            *slot = sim_action(&copy, seat, step.act);
        }
        let before = Before::of(&copy);
        copy.tick(&actions);
        seen.clear();
        before.diff(&copy, &mut seen);
        events.extend(seen.iter().map(|&what| happened_json(copy.ticks(), what)));
    }
    (copy, events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::classic_arena;

    #[test]
    fn a_lookahead_is_capped_and_charged() {
        let board = classic_arena(false, 2);
        let mut view = View {
            board,
            cursors: vec![None, None],
            budget: vec![100, 100],
        };
        let msg = answer(&mut view, 1, 0, 1_000_000, &[]);
        assert_eq!(msg["ticks"], json!(100));
        assert_eq!(view.budget[0], 0);
        let again = answer(&mut view, 1, 0, 10, &[]);
        assert_eq!(again["type"], json!("error"));
        // The other seat's budget is its own.
        assert_eq!(answer(&mut view, 1, 1, 10, &[])["ticks"], json!(10));
    }

    #[test]
    fn a_plan_is_played_and_its_consequences_reported() {
        let mut board = Board::new(6, 1, 1);
        board.set_tile(5, 0, crate::sim::TileKind::Castle(1));
        board.set_tile(0, 0, crate::sim::TileKind::Castle(0));
        board.spawn_crab(
            2,
            0,
            crate::sim::Direction::Right,
            crate::sim::Handedness::Left,
            crate::sim::CrabKind::Common,
        );
        let (_, idle) = run(&board, 0, 200, &[]);
        assert!(
            idle.iter()
                .any(|e| e["what"] == json!("banked") && e["owner"] == json!(1)),
            "{idle:?}"
        );
        // A post turning it round sends it to seat 0 instead.
        let turn = [Planned {
            at: 0,
            act: Act::Place {
                x: 3,
                y: 0,
                dir: crate::sim::Direction::Left,
            },
        }];
        let (_, turned) = run(&board, 0, 200, &turn);
        assert!(
            turned
                .iter()
                .any(|e| e["what"] == json!("banked") && e["owner"] == json!(0)),
            "{turned:?}"
        );
        let board = classic_arena(false, 2);
        let plan = [Planned {
            at: 0,
            act: Act::Place {
                x: 1,
                y: 4,
                dir: crate::sim::Direction::Up,
            },
        }];
        let (end, _) = run(&board, 0, 5, &plan);
        assert!(end.signpost_at(1, 4).is_some());
    }
}
