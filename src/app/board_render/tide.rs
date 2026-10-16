//! The tide coming in: the round's ambient clock (spec §3.7), told by the
//! sea along the bottom of the view climbing the beach as the round runs
//! out, heaving in the closing stretch, and taking a mania's colour while
//! one runs.
//!
//! It was a ring of water widening round the board. The ring is gone; the
//! sea carries what it said.

use crate::sim::{Board, TideEvent};
use bevy::prelude::*;

/// How far the sea climbs over a whole round, in interface pixels.
pub(super) const RISE: f32 = 56.0;

/// How far the closing stretch's swell carries the sea up and back, in
/// interface pixels.
pub(super) const SWELL: f32 = 5.0;

/// Whether the sea should be heaving: the closing stretch of the round,
/// measured the way the clock measures it.
///
/// Not [`Board::in_surge`], which is a flat 30 s: no level file asks for a
/// round longer than that, so every timed board would heave from its first
/// frame and the swell would mean nothing. The sim's own surge is left
/// alone, since it drives gull spawns and tide events and moving it would
/// move every recorded round.
///
/// Still true once the clock reaches zero: the swell's own clock stops
/// with the round (see [`climb`]), so the sea holds where the last wave
/// left it rather than dropping flat in one frame.
fn heaving(board: &Board, remaining: u64) -> bool {
    board.rules().round_length.is_some()
        && remaining
            <= crate::app::hud::urgency_band(board.rules().round_length, crate::sim::SURGE_TICKS)
}

/// How far through its climb the sea is, 0 at the start of a round and 1
/// at its end, and the swell on top in the closing stretch, in interface
/// pixels, `swell_clock` seconds into the swell's own clock, which the
/// caller only runs while the round is moving: a paused or finished round's
/// sea holds where it was, and takes up again from there. None if the
/// player has asked for things to hold still. A board with no clock has no
/// tide to come in.
pub(super) fn climb(board: &Board, swell_clock: f32, still: bool) -> (f32, f32) {
    let Some(remaining) = board.remaining_ticks() else {
        return (0.0, 0.0);
    };
    let total = board.ticks() + remaining;
    let elapsed = 1.0 - remaining as f32 / total.max(1) as f32;
    let swell = if heaving(board, remaining) && !still {
        (swell_clock * 6.0).sin() * SWELL
    } else {
        0.0
    };
    (elapsed, swell)
}

/// The sea's colour while a mania runs, for as long as the sim says it
/// runs: its colour, and in the alpha how far the water takes it on. The
/// caller asks only of a round that has not ended: a mania the end of the
/// round froze mid-run is not one to keep showing under the results. A
/// pause is not an end, and the mania keeps its colour through one.
///
/// Asked of the running event and not of the last one fired: a
/// spectators' call landing during a mania is the last event, and read
/// that way the water went back to blue with the mania still on.
pub(super) fn mania_tint(board: &Board) -> Option<Color> {
    match board.running_event()? {
        (TideEvent::CrabMania, _) => Some(Color::srgba(0.35, 0.72, 0.55, 0.65)),
        (TideEvent::GullMania, _) => Some(Color::srgba(0.35, 0.38, 0.48, 0.7)),
        (
            TideEvent::Monopoly
            | TideEvent::GullAttack
            | TideEvent::SpeedUp
            | TideEvent::SlowDown
            | TideEvent::FreshSand
            | TideEvent::CastleSwap
            | TideEvent::RightClaws,
            _,
        ) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The water stays tinted for as long as the mania runs, a call landing
    /// on top of it included.
    #[test]
    fn a_call_landing_in_a_mania_keeps_the_sea_tinted() {
        let mut board = Board::new(9, 7, 1);
        assert_eq!(mania_tint(&board), None, "calm water before anything");
        board.force_tide_event(TideEvent::CrabMania, 0);
        let tinted = mania_tint(&board);
        assert!(tinted.is_some(), "a crab mania tints the water");
        for _ in 0..30 {
            board.tick_idle();
        }
        board.force_tide_event(TideEvent::SpeedUp, 0);
        assert_eq!(
            board.last_event().map(|(event, _)| event),
            Some(TideEvent::SpeedUp),
            "the call is the last event now"
        );
        assert_eq!(mania_tint(&board), tinted, "and the mania still runs");
        while board.running_event().is_some() {
            board.tick_idle();
        }
        assert_eq!(mania_tint(&board), None, "blue again once both are done");
    }

    fn timed(round: u32, elapsed: u32) -> Board {
        let mut board = Board::new(9, 7, 1);
        board.rules_mut().round_length = Some(round);
        for _ in 0..elapsed {
            board.tick_idle();
        }
        board
    }

    /// A board with no clock has no tide: every untimed puzzle is one.
    #[test]
    fn a_board_with_no_clock_has_no_tide() {
        let board = Board::new(9, 7, 1);
        assert_eq!(climb(&board, 3.0, false), (0.0, 0.0));
    }

    /// The sea climbs as the round runs down, from nothing to its full
    /// rise, and only heaves in the closing stretch.
    #[test]
    fn the_sea_climbs_over_the_round_and_heaves_at_the_end() {
        let early = timed(3600, 30);
        let (rise, swell) = climb(&early, 0.3, false);
        assert!(rise < 0.05, "barely up at the start: {rise}");
        assert_eq!(swell, 0.0, "calm early on");
        let late = timed(3600, 3590);
        let (rise, swell) = climb(&late, 0.3, false);
        assert!(rise > 0.95, "nearly all the way at the end: {rise}");
        assert!(swell != 0.0, "and heaving");
        assert_eq!(climb(&late, 0.3, true).1, 0.0, "but still when asked");
        // A stopped round stops the swell's clock, and the swell holds
        // where that clock left it rather than dropping flat.
        assert_eq!(
            climb(&late, 0.3, false),
            (rise, swell),
            "held where it got to"
        );
    }
}
