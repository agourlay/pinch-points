//! The tide: the water bars swelling outward from the sand's edge as the
//! round runs out, and the foam lip riding their outer, seaward edge.
//!
//! The sand is never covered: each bar keeps its inner edge on the board's
//! own edge and grows away from it, so the sea rises around the beach and
//! the board itself is untouched until the clock says otherwise.

use super::{SWELL, Side, WATER, WATER_MAX, WATER_MIN, image_sprite, z};
use crate::app::Sim;
use crate::app::art::Art;
use crate::app::layout;
use crate::sim::TideEvent;
use bevy::prelude::*;

/// Whether the water should be heaving: the closing stretch of the round,
/// measured the way the clock measures it.
///
/// Not [`crate::sim::Board::in_surge`], which is a flat 30 s: no level file
/// asks for a round longer than that, so every timed board heaves from its
/// first frame and the swell means nothing. The sim's own surge is left
/// alone, since it drives gull spawns and tide events and moving it would
/// move every recorded round.
fn heaving(board: &crate::sim::Board, remaining: u64) -> bool {
    // A board with no clock has no closing stretch to be in. Both callers
    // check that first, so this changes nothing today, but the question is
    // "is the tide coming in" and with no tide the answer is no.
    board.round_length().is_some()
        && !board.round_over()
        && remaining <= crate::app::hud::urgency_band(board.round_length(), crate::sim::SURGE_TICKS)
}

/// How far the water reaches out from the sand's edge, in pixels.
///
/// The one place the tide's width is worked out: the bars and the foam lip
/// riding them computed it separately from the same three inputs, which is
/// two chances for the lip to come adrift of the water it sits on.
fn tide_depth(board: &crate::sim::Board, remaining: u64, seconds: f32) -> f32 {
    let total = board.ticks() + remaining;
    let elapsed = 1.0 - remaining as f32 / total.max(1) as f32;
    let mut depth = WATER_MIN + elapsed * WATER_MAX;
    if heaving(board, remaining) {
        // The swell, in the closing stretch only.
        depth += (seconds * 6.0).sin() * SWELL;
    }
    depth
}

/// The tide's ambient clock (spec §3.6): four water bars around the board,
/// one a side, that widen outward from the sand's edge over the round.
#[derive(Component)]
pub struct Waterline(pub Side);

pub fn spawn_waterline(commands: &mut Commands) {
    for side in Side::ALL {
        commands.spawn((
            Waterline(side),
            Sprite::from_color(WATER, Vec2::ZERO),
            Transform::from_translation(Vec3::new(0.0, 0.0, z::WATER)),
        ));
    }
}

/// Grow the water bars with elapsed round time, each from the sand line
/// outward; they pulse during the final surge. Boards without a round timer
/// show no water.
pub fn update_waterline(
    sim: Res<Sim>,
    time: Res<Time>,
    mut bars: Query<(&Waterline, &mut Sprite, &mut Transform)>,
) {
    let board = &sim.0;
    let Some(remaining) = board.remaining_ticks() else {
        // Only when it is not already nothing, as the foam does below:
        // writing the same zero every frame is four sprites for
        // `bevy_render` to extract again.
        for (_, mut sprite, _) in &mut bars {
            if sprite.custom_size != Some(Vec2::ZERO) {
                sprite.custom_size = Some(Vec2::ZERO);
            }
        }
        return;
    };
    let depth = tide_depth(board, remaining, time.elapsed_secs());
    let extent = layout::board_extent(board);
    for (bar, mut sprite, mut transform) in &mut bars {
        let (along, across) = bar.0.span(extent);
        // The top and bottom bars run on over the corners; the side bars
        // stop at the sand.
        let size = match bar.0 {
            Side::Top | Side::Bottom => Vec2::new(along + depth * 2.0, depth),
            Side::Left | Side::Right => Vec2::new(depth, along),
        };
        let pos = bar.0.normal() * ((across + depth) / 2.0);
        sprite.custom_size = Some(size);
        transform.translation = pos.extend(z::WATER);
    }
}

/// The foam lip riding the waterline's outer, seaward edge.
#[derive(Component)]
pub struct WaterFoam(pub Side);

pub fn spawn_water_foam(commands: &mut Commands, art: &Art) {
    for side in Side::ALL {
        commands.spawn((
            WaterFoam(side),
            image_sprite(&art.foam, Color::WHITE, Vec2::ZERO),
            Transform::from_translation(Vec3::new(0.0, 0.0, z::FOAM)),
        ));
    }
}

/// Keep the foam lip on the water's outer edge, `depth` out from the sand
/// and lapping gently; tint the water with the active mania so events read
/// on the board itself.
pub fn update_water_foam(
    sim: Res<Sim>,
    time: Res<Time>,
    mut bars: Query<(&Waterline, &mut Sprite), Without<WaterFoam>>,
    mut foam: Query<(&WaterFoam, &mut Sprite, &mut Transform)>,
) {
    let board = &sim.0;
    let mania_tint = match board.last_event() {
        Some((TideEvent::CrabMania, at)) if board.ticks().saturating_sub(at) < 300 => {
            Some(Color::srgba(0.35, 0.72, 0.55, 0.65))
        }
        Some((TideEvent::GullMania, at)) if board.ticks().saturating_sub(at) < 300 => {
            Some(Color::srgba(0.35, 0.38, 0.48, 0.7))
        }
        _ => None,
    };
    for (_, mut sprite) in &mut bars {
        let target = mania_tint.unwrap_or(WATER);
        if sprite.color != target {
            sprite.color = target;
        }
    }
    let Some(remaining) = board.remaining_ticks() else {
        for (_, mut sprite, _) in &mut foam {
            if sprite.custom_size != Some(Vec2::ZERO) {
                sprite.custom_size = Some(Vec2::ZERO);
            }
        }
        return;
    };
    let depth = tide_depth(board, remaining, time.elapsed_secs());
    let lap = (time.elapsed_secs() * 1.8).sin() * 1.5;
    let extent = layout::board_extent(board);
    const FOAM: f32 = 7.0;
    for (bar, mut sprite, mut transform) in &mut foam {
        let side = bar.0;
        let (along, across) = side.span(extent);
        // Over the corners along the top and bottom, as the bars run.
        let length = match side {
            Side::Top | Side::Bottom => along + depth * 2.0,
            Side::Left | Side::Right => along,
        };
        let size = Vec2::new(length, FOAM);
        let pos = side.normal() * (across / 2.0 + depth - lap);
        // Not `Side::rotation`: the lip keeps the turns it has always been
        // drawn with, its up pointing out of the board at the top and the
        // bottom and into it at the two sides.
        let rot = match side {
            Side::Top => 0.0,
            Side::Bottom => std::f32::consts::PI,
            Side::Left => -std::f32::consts::FRAC_PI_2,
            Side::Right => std::f32::consts::FRAC_PI_2,
        };
        sprite.custom_size = Some(size);
        transform.translation = pos.extend(z::FOAM);
        transform.rotation = Quat::from_rotation_z(rot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Board;

    fn timed(round: u32, elapsed: u32) -> Board {
        let mut board = Board::new(9, 7, 1);
        board.set_round_length(Some(round));
        for _ in 0..elapsed {
            board.tick_idle();
        }
        board
    }

    /// A board with no clock has no tide. Every puzzle in the campaign
    /// without a round line is one, and water creeping in on a level with
    /// no deadline would be promising a wave that never comes.
    #[test]
    fn a_board_with_no_clock_never_heaves() {
        let board = Board::new(9, 7, 1);
        assert_eq!(board.remaining_ticks(), None);
        assert!(!heaving(&board, 0), "no clock, no swell");
    }

    /// The swell belongs to the closing stretch and nowhere else. It used
    /// to be read off the sim's flat 30-second surge, which on a short
    /// level meant a storm from the first frame.
    #[test]
    fn the_swell_is_only_for_the_closing_stretch() {
        let board = timed(3600, 30);
        let remaining = board.remaining_ticks().expect("a clock");
        assert!(!heaving(&board, remaining), "early on it is calm");
        assert!(heaving(&board, 1), "and heaving at the end");
    }

    /// The water widens as the round runs down, and never reaches the
    /// sand: each bar keeps its inner edge on the board and grows away
    /// from it, so the beach is playable until the wave itself.
    #[test]
    fn the_tide_widens_over_the_round_and_stays_off_the_sand() {
        let board = timed(3600, 1);
        let full = board.remaining_ticks().expect("a clock");
        let early = tide_depth(&board, full, 0.0);
        let late = tide_depth(&board, full / 8, 0.0);
        assert!(early > 0.0, "there is water from the start: {early}");
        assert!(late > early, "and more of it later: {early} then {late}");
        assert!(
            late <= WATER_MAX + WATER_MIN,
            "the tide never runs past its own ceiling: {late}"
        );
    }
}
