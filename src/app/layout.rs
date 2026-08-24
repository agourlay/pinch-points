//! Grid → world-space mapping. The board is centred on the origin; sim `y`
//! grows downward (row 0 on top) while Bevy's world `y` grows upward.

use crate::sim::{Board, Direction, Pose};
use bevy::prelude::*;

pub const TILE: f32 = 64.0;

/// Z-layers, back to front.
pub mod z {
    pub const SAND: f32 = 0.0;
    /// Damp fringe around the board, above sand.
    pub const WET: f32 = 0.2;
    /// Pool water sits under features and wading creatures.
    pub const POOL: f32 = 0.5;
    pub const TILE_FEATURE: f32 = 1.0;
    pub const SIGNPOST: f32 = 2.0;
    pub const CREATURE: f32 = 3.0;
    pub const WALL: f32 = 4.0;
    /// The tide water bars.
    pub const WATER: f32 = 4.5;
    /// The foam lip riding the water's inner edge.
    pub const FOAM: f32 = 4.6;
    pub const CURSOR: f32 = 5.0;
    /// Confetti under the flash/pip layers.
    pub const CONFETTI: f32 = 5.4;
    /// Raid flash overlay.
    pub const FLASH: f32 = 5.45;
    /// Burst/glint particles.
    pub const PARTICLE: f32 = 5.5;
    /// Floating score text tops everything on the board.
    pub const PIP: f32 = 5.7;
}

pub fn tile_center(board: &Board, x: u8, y: u8) -> Vec2 {
    let w = f32::from(board.width());
    let h = f32::from(board.height());
    Vec2::new(
        (f32::from(x) - (w - 1.0) / 2.0) * TILE,
        ((h - 1.0) / 2.0 - f32::from(y)) * TILE,
    )
}

/// The tile whose centre is closest to a world position (clamped in-board).
pub fn nearest_tile(board: &Board, pos: Vec2) -> (u8, u8) {
    let w = f32::from(board.width());
    let h = f32::from(board.height());
    let x = (pos.x / TILE + (w - 1.0) / 2.0).round().clamp(0.0, w - 1.0);
    let y = ((h - 1.0) / 2.0 - pos.y / TILE).round().clamp(0.0, h - 1.0);
    (x as u8, y as u8)
}

/// World position of a creature `progress` subunits past the centre of `tile`,
/// heading in `dir`.
pub fn creature_pos(board: &Board, tile: u16, dir: Direction, progress: u16) -> Vec2 {
    let (x, y) = board.coords_u8(tile);
    let (dx, dy) = dir.offset();
    let t = f32::from(progress) / f32::from(crate::sim::SUBUNITS_PER_TILE) * TILE;
    // Flip dy: sim y-down, world y-up.
    tile_center(board, x, y) + Vec2::new(dx as f32 * t, -dy as f32 * t)
}

/// World position of a creature at `pose`: the one path both the crab and
/// the gull interpolation take for last tick's and this tick's position.
pub fn pose_pos(board: &Board, pose: Pose) -> Vec2 {
    creature_pos(board, pose.tile, pose.dir, pose.progress)
}

/// Rotation aligning a sprite authored facing +X with a sim direction.
pub fn dir_rotation(dir: Direction) -> Quat {
    let angle = match dir {
        Direction::Right => 0.0,
        Direction::Up => std::f32::consts::FRAC_PI_2,
        Direction::Left => std::f32::consts::PI,
        Direction::Down => -std::f32::consts::FRAC_PI_2,
    };
    Quat::from_rotation_z(angle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_centers_round_trip_through_nearest_tile() {
        let board = crate::sim::Board::new(12, 9, 1);
        for y in 0..board.height() {
            for x in 0..board.width() {
                let pos = tile_center(&board, x, y);
                assert_eq!(nearest_tile(&board, pos), (x, y));
            }
        }
    }

    #[test]
    fn creature_pos_at_rest_is_the_tile_center() {
        let board = crate::sim::Board::new(7, 5, 1);
        for tile in 0..(7 * 5u16) {
            let (x, y) = board.coords_u8(tile);
            assert_eq!(
                creature_pos(&board, tile, crate::sim::Direction::Right, 0),
                tile_center(&board, x, y)
            );
        }
    }

    use crate::sim::Board;

    #[test]
    fn tile_centers_are_symmetric_about_the_board_middle() {
        let board = Board::new(5, 3, 0);
        let a = tile_center(&board, 0, 0);
        let b = tile_center(&board, 4, 2);
        assert_eq!(a, -b, "opposite corners mirror through the origin");
        assert_eq!(tile_center(&board, 2, 1), Vec2::ZERO, "centre tile is 0,0");
    }
}
