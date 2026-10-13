//! Everything drawn from board state, split by what it draws:
//!
//! - [`statics`]: the sand, the walls, the terrain, and the sprites that
//!   track tile contents (turnstiles, spawner holes).
//! - [`signposts`]: the posts players plant, and how worn and settled
//!   each one is drawn.
//! - [`pools`]: the ponds over pool tiles, and their ripples.
//! - [`surround`]: the shore, sea and scenery around the board, and the
//!   cloud shadows over it.
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
mod pools;
mod signposts;
mod statics;
mod surround;
mod wash;
mod water;

pub(crate) use castles::castle_parts;
pub use castles::{
    CastleFlight, CastleSprite, castle_body, cheer_tier_ups, fly_castles, kick_castles,
    sync_castles, wave_pennants,
};
pub use pools::ripple_pools;
pub use signposts::{SignpostSprite, dress_signposts, sync_signposts};
pub use statics::{
    BoardStatic, TurnstileSprite, animate_turnstiles, pulse_spawners, spawn_static_board,
    sync_turnstiles,
};
pub use surround::drift_cloud_shadows;
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

/// One side of the board, for the things drawn along all four: the tide's
/// bars and foam, and the wet sand fringe.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Top, Side::Bottom, Side::Left, Side::Right];

    /// Out of the board across this side, in world space (y up).
    fn normal(self) -> Vec2 {
        match self {
            Side::Top => Vec2::Y,
            Side::Bottom => Vec2::NEG_Y,
            Side::Left => Vec2::NEG_X,
            Side::Right => Vec2::X,
        }
    }

    /// The turn that points a strip's foot out of the board across this
    /// side: a strip drawn fading downward fades out toward the water.
    fn rotation(self) -> f32 {
        match self {
            Side::Top => std::f32::consts::PI,
            Side::Bottom => 0.0,
            Side::Left => -std::f32::consts::FRAC_PI_2,
            Side::Right => std::f32::consts::FRAC_PI_2,
        }
    }

    /// The board measured from this side, `extent` being its size: how long
    /// this side's edge is, and how far it is across the board to the
    /// opposite one.
    fn span(self, extent: Vec2) -> (f32, f32) {
        match self {
            Side::Top | Side::Bottom => (extent.x, extent.y),
            Side::Left | Side::Right => (extent.y, extent.x),
        }
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

/// A sprite drawn for one tile, which remembers the tile it was built at.
trait OnTile {
    fn tile(&self) -> (u8, u8);
}

/// The clearing half of a sync system: what the tile-tracking sprites
/// (castles, signposts, turnstiles) all do before spawning what is missing.
///
/// Every sprite whose tile is off this board is despawned first, before
/// anything asks the board about it: that is the crash guard (see
/// [`on_board`]). Then every one that `current` says no longer matches its
/// tile goes too. The tiles whose sprite stands are marked in `covered`,
/// indexed as [`crate::sim::Board::index_of`], for the spawning half to
/// skip. `current` may retarget a sprite in place as it answers.
fn clear_stale<S: OnTile, T: std::ops::Deref<Target = S>>(
    commands: &mut Commands,
    board: &crate::sim::Board,
    covered: &mut Vec<bool>,
    sprites: impl IntoIterator<Item = (Entity, T)>,
    mut current: impl FnMut(&mut T) -> bool,
) {
    covered.clear();
    covered.resize(board.width() as usize * board.height() as usize, false);
    for (entity, mut sprite) in sprites {
        let (x, y) = sprite.tile();
        if !on_board(board, x, y) {
            commands.entity(entity).despawn();
            continue;
        }
        if current(&mut sprite) {
            covered[usize::from(board.index_of(x, y))] = true;
        } else {
            commands.entity(entity).despawn();
        }
    }
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
