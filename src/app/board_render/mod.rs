//! Everything drawn from board state, split by what it draws:
//!
//! - [`statics`]: the sand, the walls, the terrain, and the sprites that
//!   track tile contents (signposts, turnstiles, spawner holes).
//! - [`castles`]: the keeps, their pennants, and the shudder a bank sets
//!   off.
//! - [`water`]: the tide rising around the sand, and the foam on its edge.
//!
//! The shared bits, the sprite helper and the wall colour, live here, and
//! every module's public items are re-exported so the schedule and the
//! teardown code see one `board_render`.

mod castles;
mod statics;
mod water;

pub use castles::{
    CastleFlight, CastleSprite, fly_castles, kick_castles, sync_castles, wave_pennants,
};
pub use statics::{
    BoardStatic, SignpostSprite, TurnstileSprite, animate_turnstiles, pulse_spawners,
    spawn_static_board, sync_signposts, sync_turnstiles,
};
pub use water::{
    WaterFoam, Waterline, spawn_water_foam, spawn_waterline, update_water_foam, update_waterline,
};

use bevy::prelude::*;

/// A tinted sprite at a fixed size: the shape almost every board sprite
/// takes.
fn image_sprite(image: &Handle<Image>, tint: Color, size: Vec2) -> Sprite {
    Sprite {
        image: image.clone(),
        color: tint,
        custom_size: Some(size),
        ..default()
    }
}

/// Whether a sprite's remembered tile exists on this board.
///
/// The sync systems diff sprites against the sim by the coordinates each
/// sprite was built at, and the sim's `tile_at` and `signpost_at` assert on
/// coordinates off the board. A sprite left over from a bigger board (a
/// swap that skipped the teardown) is therefore a crash rather than a
/// stale picture, so every probe checks here first and treats a stranded
/// sprite as one to despawn.
fn on_board(board: &crate::sim::Board, x: u8, y: u8) -> bool {
    x < board.width() && y < board.height()
}

const WALL_COLOR: Color = Color::srgb(0.35, 0.28, 0.22);
const WATER: Color = Color::srgba(0.30, 0.58, 0.82, 0.65);
/// How wide the water grows over the whole round, in pixels, out from the
/// sand's edge: the bars swell seaward and never cover the board.
const WATER_MAX: f32 = 42.0;

pub(super) use crate::app::layout::z;
