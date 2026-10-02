//! The fair cursor: a seat that is not a person still has a hand, and the
//! hand moves at a person's speed.
//!
//! The sim has no cursor at all, so without this a bot places anywhere
//! instantly, and a strategy that plants in opposite corners on consecutive
//! ticks is one no person can copy. Under the rule a reply names a target
//! and the seat's cursor walks there, a tile every `pace` ticks, before the
//! action lands. It walks diagonally, as a person holding two arrow keys
//! does (`app::cursor::move_cursor` adds both axes on one step).
//!
//! The cursor lives here, in the seat layer, outside the sim: a replay
//! records the action that was committed, as it does for a person, and the
//! state hash never sees a cursor.

use super::lookahead::sim_action;
use super::protocol::Act;
use crate::sim::{Board, PlayerAction, PlayerId};

/// Ticks a fair cursor takes to cross a tile: about the 0.09 s a held key
/// repeats at (`settings::GameSettings::repeat_interval`).
pub const TICKS_PER_TILE: u32 = 3;

/// One seat's hand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SeatCursor {
    pub at: (u8, u8),
    /// The order being walked to, if one is.
    order: Option<Act>,
    /// Ticks spent on the current tile of the walk.
    walked: u32,
}

impl SeatCursor {
    /// A hand resting on the seat's castle, where a person's cursor starts.
    pub fn home(board: &Board, seat: PlayerId) -> SeatCursor {
        SeatCursor {
            at: board
                .castle_of(seat)
                .unwrap_or((board.width() / 2, board.height() / 2)),
            order: None,
            walked: 0,
        }
    }

    /// The order still being walked to.
    pub fn order(&self) -> Option<Act> {
        self.order
    }

    /// One tick: take this tick's new act (`Act::None` for none) and say
    /// what the seat does. `pace` is the rule: `None` and the hand is
    /// wherever the act says, at once.
    ///
    /// Under the rule, `place`, `remove` and `move` set a new order that
    /// replaces any still walking; `none` never cancels one, so a bot need
    /// not repeat itself every tick; `clear` needs no walk, as the
    /// clear-all key needs none for a person.
    pub fn step(
        &mut self,
        board: &Board,
        seat: PlayerId,
        act: Act,
        pace: Option<u32>,
    ) -> (PlayerAction, Option<Act>) {
        let Some(pace) = pace else {
            if let Some(target) = act.target() {
                self.at = clamp(board, target);
            }
            return (
                sim_action(board, seat, act),
                (act != Act::None).then_some(act),
            );
        };
        match act {
            Act::None => {}
            Act::Clear => return (sim_action(board, seat, act), Some(act)),
            Act::Place { .. } | Act::Remove { .. } | Act::Move { .. } => self.order = Some(act),
        }
        let Some(order) = self.order else {
            return (PlayerAction::None, None);
        };
        let target = clamp(board, order.target().unwrap_or(self.at));
        if self.at != target {
            self.walked += 1;
            if self.walked >= pace.max(1) {
                self.walked = 0;
                self.at = (toward(self.at.0, target.0), toward(self.at.1, target.1));
            }
        }
        if self.at != target {
            return (PlayerAction::None, None);
        }
        self.order = None;
        self.walked = 0;
        (sim_action(board, seat, order), Some(order))
    }
}

/// One step from `from` toward `to` along one axis.
fn toward(from: u8, to: u8) -> u8 {
    match from.cmp(&to) {
        std::cmp::Ordering::Less => from + 1,
        std::cmp::Ordering::Greater => from - 1,
        std::cmp::Ordering::Equal => from,
    }
}

/// A cursor stays on the beach, as a person's does: an order aimed off it
/// walks to the edge and is refused there by the sim.
fn clamp(board: &Board, (x, y): (u8, u8)) -> (u8, u8) {
    (x.min(board.width() - 1), y.min(board.height() - 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Direction, classic_arena};

    #[test]
    fn without_the_rule_the_hand_is_where_the_act_says() {
        let board = classic_arena(false, 2);
        let mut hand = SeatCursor::home(&board, 0);
        let act = Act::Place {
            x: 9,
            y: 7,
            dir: Direction::Up,
        };
        let (action, done) = hand.step(&board, 0, act, None);
        assert_eq!(
            action,
            PlayerAction::Place {
                x: 9,
                y: 7,
                dir: Direction::Up
            }
        );
        assert_eq!(done, Some(act));
        assert_eq!(hand.at, (9, 7));
    }

    #[test]
    fn under_the_rule_a_placement_waits_for_the_walk_and_none_never_cancels() {
        let board = classic_arena(false, 2);
        let mut hand = SeatCursor::home(&board, 0);
        let start = hand.at;
        let (tx, ty) = (start.0 + 3, start.1 + 2);
        let act = Act::Place {
            x: tx,
            y: ty,
            dir: Direction::Up,
        };
        let mut ticks = 0;
        let mut next = act;
        let landed = loop {
            ticks += 1;
            let (action, _) = hand.step(&board, 0, next, Some(TICKS_PER_TILE));
            next = Act::None;
            if action != PlayerAction::None {
                break action;
            }
            assert!(ticks < 100, "the walk never ended");
        };
        assert_eq!(landed, sim_action(&board, 0, act));
        // Diagonal: three tiles away on the long axis is three steps.
        assert_eq!(ticks, 3 * TICKS_PER_TILE);
        assert_eq!(hand.at, (tx, ty));
    }

    #[test]
    fn a_new_order_replaces_the_one_walking_and_clear_needs_no_walk() {
        let board = classic_arena(false, 2);
        let mut hand = SeatCursor::home(&board, 0);
        hand.step(&board, 0, Act::Move { x: 11, y: 8 }, Some(3));
        assert_eq!(hand.order(), Some(Act::Move { x: 11, y: 8 }));
        let (_, done) = hand.step(&board, 0, Act::Clear, Some(3));
        assert_eq!(done, Some(Act::Clear));
        assert_eq!(
            hand.order(),
            Some(Act::Move { x: 11, y: 8 }),
            "clear leaves the walk alone"
        );
        hand.step(&board, 0, Act::Move { x: 0, y: 0 }, Some(3));
        assert_eq!(hand.order(), Some(Act::Move { x: 0, y: 0 }));
    }
}
