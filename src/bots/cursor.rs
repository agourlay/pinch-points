//! The fair cursor: a seat that is not a person still has a hand, and the
//! hand moves at a person's speed.
//!
//! The sim has no cursor at all, so without this a bot places anywhere
//! instantly, and a strategy that plants in opposite corners on consecutive
//! ticks is one no person can copy. Under the rule a reply names a target
//! and the seat's cursor walks there before the action lands, by
//! [`fair_walk`]: the first tile at once, the second after a held key's
//! lift, then one every [`FAIR_TICKS_PER_TILE`] ticks, diagonally when it
//! needs to, as a person holding two arrow keys does. The game's AI walks
//! by the same rule (`sim::bot`), read off the board instead of kept here.
//!
//! The cursor lives in the seat layer, outside the sim: a replay records
//! the action that was committed, as it does for a person, and the state
//! hash never sees a cursor.

use super::lookahead::sim_action;
use super::protocol::Act;
use crate::sim::{Board, PlayerAction, PlayerId, fair_walk, hand_steps};
pub use crate::sim::{FAIR_LIFT, FAIR_TICKS_PER_TILE};

/// One seat's hand.
///
/// The hand's pace belongs to the walk, not to the order: a walk goes on,
/// lift and all, for as long as the hand keeps moving, whatever it is
/// walking to. A new order mid-walk only changes where it goes, so asking
/// for the next tile every tick is a held key, not a fresh first step
/// every tick. A hand still for a key's repeat has let go, and its next
/// walk starts afresh with a free first tile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SeatCursor {
    pub at: (u8, u8),
    /// The order being walked to, if one is.
    order: Option<Act>,
    /// Ticks since this walk began.
    walked: u32,
    /// Tiles the hand has moved since this walk began.
    steps: u32,
    /// Ticks since the hand last had somewhere to go.
    still: u32,
}

impl SeatCursor {
    /// A hand resting on the seat's castle, where a person's cursor starts.
    pub fn home(board: &Board, seat: PlayerId) -> SeatCursor {
        let at = board
            .castle_of(seat)
            .unwrap_or((board.width() / 2, board.height() / 2));
        SeatCursor {
            at,
            order: None,
            walked: 0,
            steps: 0,
            still: FAIR_TICKS_PER_TILE,
        }
    }

    /// The order still being walked to.
    pub fn order(&self) -> Option<Act> {
        self.order
    }

    /// One tick: take this tick's new act (`Act::None` for none) and say
    /// what the seat does, and which act that carried out. Without the rule
    /// (`fair` false) the hand is wherever the act says, at once.
    ///
    /// Under the rule, `place`, `remove` and `move` set a new order that
    /// replaces any still walking; `none` never cancels one, so a bot need
    /// not repeat itself every tick; `clear` needs no walk, as the
    /// clear-all key needs none for a person, and the walk goes on under
    /// it (an order it brings home lands on the next tick, one action a
    /// tick being all a seat has).
    pub fn step(
        &mut self,
        board: &Board,
        seat: PlayerId,
        act: Act,
        fair: bool,
    ) -> (PlayerAction, Option<Act>) {
        if !fair {
            if let Some(target) = act.target() {
                self.at = clamp(board, target);
            }
            return (
                sim_action(board, seat, act),
                (act != Act::None).then_some(act),
            );
        }
        match act {
            Act::None | Act::Clear => {}
            Act::Place { .. } | Act::Remove { .. } | Act::Move { .. } => self.order = Some(act),
        }
        let target = self
            .order
            .map(|order| clamp(board, order.target().unwrap_or(self.at)));
        self.walk(target);
        if act == Act::Clear {
            return (sim_action(board, seat, act), Some(act));
        }
        match self.order {
            Some(order) if target == Some(self.at) => {
                self.order = None;
                (sim_action(board, seat, order), Some(order))
            }
            _ => (PlayerAction::None, None),
        }
    }

    /// One tick of the hand toward `target`, as far as the walk's pace
    /// allows. A hand held back by the pace is still holding the key; only
    /// one with nowhere to go has let it go.
    fn walk(&mut self, target: Option<(u8, u8)>) {
        let walking = match target {
            Some(target) if target != self.at => {
                if self.still >= FAIR_TICKS_PER_TILE {
                    self.walked = 0;
                    self.steps = 0;
                }
                // The most tiles a held key has covered by now.
                let mut allowed = 1;
                while fair_walk(allowed + 1) <= self.walked {
                    allowed += 1;
                }
                let n = allowed
                    .saturating_sub(self.steps)
                    .min(hand_steps(self.at, target));
                self.at = along(self.at, target, n);
                self.steps += n;
                true
            }
            _ => false,
        };
        self.still = if walking {
            0
        } else {
            self.still.saturating_add(1)
        };
        self.walked = self.walked.saturating_add(1);
    }
}

/// `steps` tiles from `from` toward `to`, both axes at once while both have
/// ground to cover.
fn along(from: (u8, u8), to: (u8, u8), steps: u32) -> (u8, u8) {
    let axis = |a: u8, b: u8| {
        let moved = u32::from(a.abs_diff(b)).min(steps) as u8;
        if b >= a { a + moved } else { a - moved }
    };
    (axis(from.0, to.0), axis(from.1, to.1))
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
        let (action, done) = hand.step(&board, 0, act, false);
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
        let (tx, ty) = (start.0 + 4, start.1 + 2);
        let act = Act::Place {
            x: tx,
            y: ty,
            dir: Direction::Up,
        };
        let mut ticks = 0;
        let mut next = act;
        let landed = loop {
            let (action, _) = hand.step(&board, 0, next, true);
            next = Act::None;
            if action != PlayerAction::None {
                break action;
            }
            ticks += 1;
            assert!(ticks < 100, "the walk never ended");
        };
        assert_eq!(landed, sim_action(&board, 0, act));
        // Four tiles on the long axis, diagonally: the lift and two repeats.
        assert_eq!(ticks, fair_walk(4));
        assert_eq!(ticks, FAIR_LIFT + 2 * FAIR_TICKS_PER_TILE);
        assert_eq!(hand.at, (tx, ty));
    }

    #[test]
    fn a_neighbouring_tile_is_one_keypress_away() {
        let board = classic_arena(false, 2);
        let mut hand = SeatCursor::home(&board, 0);
        let (x, y) = hand.at;
        let act = Act::Place {
            x: x + 1,
            y: y + 1,
            dir: Direction::Left,
        };
        let (action, done) = hand.step(&board, 0, act, true);
        assert_eq!(done, Some(act), "landed on the tick it was asked for");
        assert_ne!(action, PlayerAction::None);
    }

    #[test]
    fn asking_again_for_the_tile_being_walked_to_keeps_walking() {
        let board = classic_arena(false, 2);
        let mut hand = SeatCursor::home(&board, 0);
        let (x, y) = hand.at;
        let far = Act::Place {
            x: x + 6,
            y: y + 6,
            dir: Direction::Up,
        };
        let mut ticks = 0;
        loop {
            // Repeated every few ticks, as a bot that re-decides would.
            let act = if ticks % 4 == 0 { far } else { Act::None };
            let (action, _) = hand.step(&board, 0, act, true);
            if action != PlayerAction::None {
                break;
            }
            ticks += 1;
            assert!(ticks < 100, "a repeated order never landed");
        }
        assert_eq!(ticks, fair_walk(6));
    }

    #[test]
    fn a_new_order_replaces_the_one_walking_and_clear_needs_no_walk() {
        let board = classic_arena(false, 2);
        let mut hand = SeatCursor::home(&board, 0);
        hand.step(&board, 0, Act::Move { x: 11, y: 8 }, true);
        assert_eq!(hand.order(), Some(Act::Move { x: 11, y: 8 }));
        let (_, done) = hand.step(&board, 0, Act::Clear, true);
        assert_eq!(done, Some(Act::Clear));
        assert_eq!(
            hand.order(),
            Some(Act::Move { x: 11, y: 8 }),
            "clear leaves the walk alone"
        );
        hand.step(&board, 0, Act::Move { x: 0, y: 0 }, true);
        assert_eq!(hand.order(), Some(Act::Move { x: 0, y: 0 }));
    }

    /// Clearing is the clear-all key, pressed while the other hand keeps
    /// walking: the walk does not stop for it.
    #[test]
    fn clearing_every_tick_does_not_freeze_the_walk() {
        let board = classic_arena(false, 2);
        let mut hand = SeatCursor::home(&board, 0);
        let (x, y) = hand.at;
        let far = Act::Move { x: x + 6, y };
        hand.step(&board, 0, far, true);
        for _ in 0..fair_walk(6) + 5 {
            hand.step(&board, 0, Act::Clear, true);
        }
        assert_eq!(hand.at, (x + 6, y), "the hand walked on under the clears");
    }

    /// A tile at a time, asked for every tick, is a held key and walks at a
    /// held key's pace, lift and all: not a tile a tick.
    #[test]
    fn a_step_at_a_time_walks_no_faster_than_a_held_key() {
        let board = classic_arena(false, 2);
        let mut hand = SeatCursor::home(&board, 0);
        let start = hand.at;
        let ticks = 20;
        for _ in 0..ticks {
            let (x, y) = hand.at;
            hand.step(&board, 0, Act::Move { x: x + 1, y }, true);
        }
        let moved = hand_steps(start, hand.at);
        let held = (0..=ticks)
            .filter(|&s| fair_walk(s) < ticks)
            .max()
            .unwrap_or(0);
        assert!(
            moved <= held,
            "{moved} tiles in {ticks} ticks, where a held key goes {held}"
        );
    }
}
