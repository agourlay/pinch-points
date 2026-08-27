//! Grid → world-space mapping. The board is centred on the origin; sim `y`
//! grows downward (row 0 on top) while Bevy's world `y` grows upward.

use crate::sim::{Board, Direction, Pose};
use bevy::prelude::*;

pub const TILE: f32 = 64.0;

/// Where everything standing on this beach drops its shadow, in world
/// pixels.
///
/// One direction for the whole board: the fence, the rocks, the kelp, the
/// keeps, the posts and the creatures all read it. They used to carry four
/// slightly different offsets between them - a rock at (2, -3), the fence
/// at (2.5, -3.5), a post at (3, -3) and a castle straight down - and a
/// rock beside a post casting at two different angles is the first thing
/// an eye picks out of a flat-lit scene.
pub const SUN: Vec2 = Vec2::new(2.5, -3.5);

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
    use crate::sim::Direction::{Down, Left, Right, Up};

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

    /// cursor is clamped by this and nothing else.
    #[test]
    fn a_point_off_the_board_clamps_onto_it() {
        let board = Board::new(9, 7, 1);
        let far = 10_000.0;
        assert_eq!(nearest_tile(&board, Vec2::new(-far, far)), (0, 0));
        assert_eq!(nearest_tile(&board, Vec2::new(far, -far)), (8, 6));
        assert_eq!(nearest_tile(&board, Vec2::new(far, far)), (8, 0));
    }

    /// looks plausible until a crab walks the wrong way.
    #[test]
    fn a_full_stride_arrives_on_the_neighbour_and_flips_the_axis() {
        let board = Board::new(9, 7, 1);
        let tile = board.index_of(4, 3);
        let full = crate::sim::SUBUNITS_PER_TILE;
        let step = |dir| creature_pos(&board, tile, dir, full);
        assert_eq!(step(Right), tile_center(&board, 5, 3));
        assert_eq!(step(Left), tile_center(&board, 3, 3));
        // Sim `Down` is row 4, which is *lower* on the screen.
        assert_eq!(step(Down), tile_center(&board, 4, 4));
        assert!(step(Down).y < tile_center(&board, 4, 3).y, "down is down");
        assert!(step(Up).y > tile_center(&board, 4, 3).y, "and up is up");
    }

    /// sitting a pixel off the ones of the other.
    #[test]
    fn a_pose_reads_the_same_as_its_parts() {
        let board = Board::new(12, 9, 1);
        for progress in [0, 17, 64, crate::sim::SUBUNITS_PER_TILE - 1] {
            for dir in [Up, Down, Left, Right] {
                let tile = board.index_of(6, 4);
                let pose = crate::sim::Pose {
                    tile,
                    dir,
                    progress,
                };
                assert_eq!(
                    pose_pos(&board, pose),
                    creature_pos(&board, tile, dir, progress)
                );
            }
        }
    }

    /// the wrong way round reads as a crab walking backwards.
    #[test]
    fn a_heading_turns_the_art_from_facing_right() {
        let ahead = Vec3::X;
        let turn = |dir| (dir_rotation(dir) * ahead).truncate();
        let close = |a: Vec2, b: Vec2| a.distance(b) < 1e-5;
        assert!(close(turn(Right), Vec2::X), "{:?}", turn(Right));
        assert!(close(turn(Left), -Vec2::X), "{:?}", turn(Left));
        assert!(close(turn(Up), Vec2::Y), "{:?}", turn(Up));
        assert!(close(turn(Down), -Vec2::Y), "{:?}", turn(Down));
    }

    /// constant was introduced to remove.
    #[test]
    fn the_sun_casts_down_and_to_the_right() {
        // A `const` block rather than a plain assertion: the value is
        // known at compile time, so this refuses to *build* with the sun
        // in the wrong quarter rather than merely failing a run.
        const { assert!(SUN.x > 0.0, "shadows fall to the right") };
        const { assert!(SUN.y < 0.0, "and downward, in world space") };
        // And the pair is a real diagonal, not a straight drop: a shadow
        // directly under its caster is a shadow nobody ever sees, which is
        // how the creatures' went unnoticed for the whole of their life.
        assert!(
            SUN.x.abs() > 1.0 && SUN.y.abs() > 1.0,
            "{SUN:?} is too flat"
        );
    }
}
