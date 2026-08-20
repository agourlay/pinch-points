//! The tide: the water bars swelling outward from the sand's edge as the
//! round runs out, and the foam lip riding their outer, seaward edge.
//!
//! The sand is never covered. Each bar keeps its inner edge on the board's
//! own edge and grows away from it, so the sea reads as rising around the
//! beach: wider water, farther foam, and the board itself untouched until
//! the clock says otherwise.

use super::{WATER, WATER_MAX, image_sprite, z};
use crate::app::Sim;
use crate::app::art::Art;
use crate::app::layout::TILE;
use crate::sim::TideEvent;
use bevy::prelude::*;

/// Whether the water should be heaving: the closing stretch of the round,
/// measured the way the clock measures it.
///
/// Not [`crate::sim::Board::in_surge`], which is a flat 30 s. No level file
/// asks for a round longer than that, so every timed board heaved from its
/// first frame and the swell stopped meaning anything - Dry Feet is eight
/// seconds long and spent all of them in a storm. The sim's own surge is
/// left alone: it drives gull spawns and tide events, and moving it would
/// move every recorded round.
fn heaving(board: &crate::sim::Board, remaining: u64) -> bool {
    !board.round_over()
        && remaining <= crate::app::hud::urgency_band(board.round_length(), crate::sim::SURGE_TICKS)
}

/// The tide's ambient clock (spec §3.6): four water bars around the board
/// that widen outward from the sand's edge over the round. `0..4` = top,
/// bottom, left, right.
#[derive(Component)]
pub struct Waterline(pub u8);

pub fn spawn_waterline(commands: &mut Commands) {
    for side in 0..4u8 {
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
        for (_, mut sprite, _) in &mut bars {
            sprite.custom_size = Some(Vec2::ZERO);
        }
        return;
    };
    let total = board.ticks() + remaining;
    let elapsed = 1.0 - remaining as f32 / total.max(1) as f32;
    let mut depth = 6.0 + elapsed * WATER_MAX;
    if heaving(board, remaining) {
        depth += (time.elapsed_secs() * 6.0).sin() * 3.0;
    }
    let w = f32::from(board.width()) * TILE;
    let h = f32::from(board.height()) * TILE;
    for (bar, mut sprite, mut transform) in &mut bars {
        let (size, pos) = match bar.0 {
            0 => (
                Vec2::new(w + depth * 2.0, depth),
                Vec2::new(0.0, (h + depth) / 2.0),
            ),
            1 => (
                Vec2::new(w + depth * 2.0, depth),
                Vec2::new(0.0, -(h + depth) / 2.0),
            ),
            2 => (Vec2::new(depth, h), Vec2::new(-(w + depth) / 2.0, 0.0)),
            _ => (Vec2::new(depth, h), Vec2::new((w + depth) / 2.0, 0.0)),
        };
        sprite.custom_size = Some(size);
        transform.translation = pos.extend(z::WATER);
    }
}

/// The foam lip riding the waterline's outer, seaward edge.
#[derive(Component)]
pub struct WaterFoam(pub u8);

pub fn spawn_water_foam(commands: &mut Commands, art: &Art) {
    for side in 0..4u8 {
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
    let total = board.ticks() + remaining;
    let elapsed = 1.0 - remaining as f32 / total.max(1) as f32;
    let mut depth = 6.0 + elapsed * WATER_MAX;
    if heaving(board, remaining) {
        depth += (time.elapsed_secs() * 6.0).sin() * 3.0;
    }
    let lap = (time.elapsed_secs() * 1.8).sin() * 1.5;
    let w = f32::from(board.width()) * TILE;
    let h = f32::from(board.height()) * TILE;
    const FOAM: f32 = 7.0;
    for (bar, mut sprite, mut transform) in &mut foam {
        let (size, pos, rot) = match bar.0 {
            0 => (
                Vec2::new(w + depth * 2.0, FOAM),
                Vec2::new(0.0, h / 2.0 + depth - lap),
                0.0,
            ),
            1 => (
                Vec2::new(w + depth * 2.0, FOAM),
                Vec2::new(0.0, -h / 2.0 - depth + lap),
                std::f32::consts::PI,
            ),
            2 => (
                Vec2::new(h, FOAM),
                Vec2::new(-w / 2.0 - depth + lap, 0.0),
                -std::f32::consts::FRAC_PI_2,
            ),
            _ => (
                Vec2::new(h, FOAM),
                Vec2::new(w / 2.0 + depth - lap, 0.0),
                std::f32::consts::FRAC_PI_2,
            ),
        };
        sprite.custom_size = Some(size);
        transform.translation = pos.extend(z::FOAM);
        transform.rotation = Quat::from_rotation_z(rot);
    }
}
