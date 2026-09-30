//! Everything drawn from board state, split by what it draws:
//!
//! - [`statics`]: the sand, the walls, the terrain, and the sprites that
//!   track tile contents (signposts, turnstiles, spawner holes).
//! - [`castles`]: the keeps, their pennants, and the shudder a bank sets
//!   off.
//! - [`water`]: the tide rising around the sand, and the foam on its edge.
//! - [`wash`]: the wave that comes in over the whole beach when the round
//!   is over.
//!
//! The shared bits, the sprite helper and the wall colour, live here, and
//! every module's public items are re-exported so the schedule and the
//! teardown code see one `board_render`.

mod castles;
mod statics;
mod wash;
mod water;

pub(crate) use castles::castle_parts;
pub use castles::{
    CastleFlight, CastleSprite, castle_body, cheer_tier_ups, fly_castles, kick_castles,
    sync_castles, wave_pennants,
};
pub use statics::{
    BoardStatic, SignpostSprite, TurnstileSprite, animate_turnstiles, dress_signposts,
    drift_cloud_shadows, pulse_spawners, ripple_pools, spawn_static_board, sync_signposts,
    sync_turnstiles,
};
pub use wash::{advance_tide_wash, start_tide_wash};
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
/// coordinates off the board, so a sprite left over from a bigger board is
/// a crash rather than a stale picture. Every probe checks here first and
/// despawns a stranded sprite.
fn on_board(board: &crate::sim::Board, x: u8, y: u8) -> bool {
    x < board.width() && y < board.height()
}

const WALL_COLOR: Color = Color::srgb(0.35, 0.28, 0.22);
const WATER: Color = Color::srgba(0.30, 0.58, 0.82, 0.65);
/// How wide the water grows over the whole round, in pixels, out from the
/// sand's edge: the bars swell seaward and never cover the board.
const WATER_MAX: f32 = 42.0;
/// The water's width before the round has run at all.
const WATER_MIN: f32 = 6.0;
/// How far the closing stretch's swell carries the water past its width.
const SWELL: f32 = 3.0;

/// How far past the sand the board is ever drawn, in world units: the tide
/// at its widest, which reaches past the wooden frame. `boot::fit_camera`
/// keeps this band inside the screen room it has at 1:1, so a zoomed-in
/// board does not push its tide under the interface.
pub(crate) const RIM: f32 = WATER_MIN + WATER_MAX + SWELL;

pub(super) use crate::app::layout::z;
