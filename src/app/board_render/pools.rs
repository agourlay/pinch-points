//! Rock pools: the ponds drawn over pool tiles, and their ripples.

use super::image_sprite;
use super::statics::BoardStatic;
use super::z;
use crate::app::art::Art;
use crate::app::layout::{self, TILE};
use crate::sim::{Board, TileKind};
use bevy::prelude::*;

/// A pool's layers, bottom up: what each is drawn with, its size on a
/// tile, and its size bridging two tiles (along the pair, across it).
#[derive(Clone, Copy)]
pub(crate) enum PoolLayer {
    /// The wet sand round the water, a plain tinted sprite.
    Shore(Color),
    /// The shallow rim and the deep middle, drawn by the water shader.
    Shallow,
    Deep,
}

pub(crate) const POOL_LAYERS: [(PoolLayer, f32, Vec2); 3] = [
    (
        PoolLayer::Shore(Color::srgb(0.77, 0.66, 0.48)),
        1.1,
        Vec2::new(1.2, 1.0),
    ),
    (PoolLayer::Shallow, 0.98, Vec2::new(1.1, 0.86)),
    (PoolLayer::Deep, 0.62, Vec2::new(1.0, 0.58)),
];

/// Spawn one layer of a pond, `extent` across and placed at `place`, with
/// `marker` on it: the wet-sand shore as a tinted puddle, the water as
/// the water shader cut to the puddle's shape. The board's pools and the
/// postcard's are drawn by this one function, so the two match.
pub(crate) fn spawn_layer(
    commands: &mut Commands,
    art: &Art,
    layer: PoolLayer,
    place: Transform,
    extent: Vec2,
    marker: impl Bundle,
) {
    match layer {
        PoolLayer::Shore(tint) => {
            commands.spawn((marker, image_sprite(&art.puddle, tint, extent), place));
        }
        PoolLayer::Shallow => {
            let water = &art.shaded.pool_shallow;
            commands.spawn((marker, art.shaded.quad(water, place, extent)));
        }
        PoolLayer::Deep => {
            let water = &art.shaded.pool_deep;
            commands.spawn((marker, art.shaded.quad(water, place, extent)));
        }
    }
}

/// The sun's glint on a pond `across` wide centred at `at`, with `marker`
/// on it: up toward the light, the opposite side to the ground shadows,
/// whichever way the puddle is turned. The board's pools and the
/// postcard's both draw theirs here.
pub(crate) fn spawn_glint(
    commands: &mut Commands,
    art: &Art,
    at: Vec3,
    across: Vec2,
    marker: impl Bundle,
) {
    let sun = -layout::SUN.normalize() * across * 0.13;
    let place = Transform::from_translation(at + sun.extend(0.0))
        .with_rotation(Quat::from_rotation_z(0.35));
    let size = Vec2::new(across.x * 0.26, across.y * 0.1);
    commands.spawn((marker, art.shaded.quad(&art.shaded.glint, place, size)));
}

/// A pool's slow swell: a ripple rising out of the water and fading.
#[derive(Component)]
pub struct PoolRipple {
    /// Where in its cycle this ripple starts, 0..1, so a pond's ripples
    /// never swell in step.
    phase: f32,
}

/// Seconds a ripple takes to swell and fade: slow enough to read as
/// water breathing, not as something happening.
const RIPPLE_PERIOD: f32 = 4.2;

/// A small, fixed number per tile, for the variety that must not change
/// between frames: which way a puddle is turned, when its ripple starts.
fn tile_hash(x: u8, y: u8) -> u32 {
    let h = u32::from(x).wrapping_mul(0x9E37_79B1) ^ u32::from(y).wrapping_mul(0x85EB_CA77);
    h ^ (h >> 15)
}

/// Every pool on the board, as ponds rather than squares.
///
/// Each tile is an irregular puddle in three
/// flat layers, turned a different way per tile, and every pair of
/// neighbouring pool tiles gets a stretched puddle between them, in the
/// same three layers. Layers of one flat colour merge where they overlap,
/// so a lane of pool tiles reads as one body of water with one shoreline,
/// which is what the campaign's pool-filled lanes are. A two-by-two block
/// gets a puddle over its shared corner too, or its middle is dry.
///
/// The two water layers are the water shader, which colours a pixel by
/// where it is in the world and not by which puddle it belongs to, so the
/// overlapping pieces still merge into one body with one moving light.
///
/// All of it sits under creatures and signposts, which wade over it.
pub(super) fn spawn_pools(commands: &mut Commands, board: &Board, art: &Art) {
    let pieces = pond_pieces(board);
    for (layer, (kind, size, bridge)) in POOL_LAYERS.into_iter().enumerate() {
        let z = z::POOL - 0.03 + 0.01 * layer as f32;
        for piece in &pieces {
            let (at, turn, extent) = match *piece {
                PondPiece::Tile { at, turn } | PondPiece::Corner { at, turn } => {
                    (at, turn, Vec2::splat(size))
                }
                PondPiece::Bridge { at, turn } => (at, turn, bridge),
            };
            let place = Transform::from_translation(at.extend(z))
                .with_rotation(Quat::from_rotation_z(turn));
            spawn_layer(commands, art, kind, place, extent * TILE, BoardStatic);
        }
    }
    for (x, y, kind) in board.tiles() {
        if kind != TileKind::Pool {
            continue;
        }
        let hash = tile_hash(x, y);
        let wander = Vec2::new(
            ((hash >> 8) % 100) as f32 / 100.0 - 0.5,
            ((hash >> 16) % 100) as f32 / 100.0 - 0.5,
        ) * TILE
            * 0.12;
        commands.spawn((
            BoardStatic,
            PoolRipple {
                phase: ((hash >> 4) % 1000) as f32 / 1000.0,
            },
            image_sprite(&art.ripple, Color::NONE, Vec2::splat(TILE)),
            Transform::from_translation(
                (layout::tile_center(board, x, y) + wander).extend(z::POOL + 0.005),
            )
            .with_rotation(Quat::from_rotation_z(pool_turn(hash))),
        ));
        let at = (layout::tile_center(board, x, y) + wander).extend(z::POOL + 0.004);
        spawn_glint(commands, art, at, Vec2::splat(TILE), BoardStatic);
    }
}

/// One stroke of a pond, in no layer in particular: [`spawn_pools`] draws
/// every piece once per layer.
#[derive(Clone, Copy, Debug, PartialEq)]
enum PondPiece {
    /// A puddle over one pool tile.
    Tile { at: Vec2, turn: f32 },
    /// A stretched puddle between two neighbouring pool tiles.
    Bridge { at: Vec2, turn: f32 },
    /// A puddle over the corner four pool tiles share.
    Corner { at: Vec2, turn: f32 },
}

/// Which way a pool tile's puddle is turned, so no two neighbours are the
/// same shape.
fn pool_turn(hash: u32) -> f32 {
    (hash % 628) as f32 / 100.0
}

/// Every piece of every pond on the board, in world space.
fn pond_pieces(board: &Board) -> Vec<PondPiece> {
    let pool = |x: u8, y: u8| {
        x < board.width() && y < board.height() && board.tile_at(x, y) == TileKind::Pool
    };
    let mut pieces = Vec::new();
    for (x, y, kind) in board.tiles() {
        if kind != TileKind::Pool {
            continue;
        }
        let at = layout::tile_center(board, x, y);
        let turn = pool_turn(tile_hash(x, y));
        pieces.push(PondPiece::Tile { at, turn });
        // Right and down only, so each pair is bridged once.
        for (dx, dy, turn) in [(1, 0, 0.0), (0, 1, std::f32::consts::FRAC_PI_2)] {
            if pool(x + dx, y + dy) {
                let next = layout::tile_center(board, x + dx, y + dy);
                pieces.push(PondPiece::Bridge {
                    at: (at + next) / 2.0,
                    turn,
                });
            }
        }
        if pool(x + 1, y) && pool(x, y + 1) && pool(x + 1, y + 1) {
            pieces.push(PondPiece::Corner {
                at: at + Vec2::new(TILE / 2.0, -TILE / 2.0),
                turn: turn + 1.0,
            });
        }
    }
    pieces
}

/// Swell each pool's ripple out of the water and fade it, over and over.
///
/// On the wall clock rather than the sim's: it is weather, and it keeps
/// breathing through a pause the way the clouds keep drifting.
///
/// Not at all for a player who asked for less motion: a ripple is nothing
/// but motion, so it stays the clear water it is spawned as, and one
/// caught mid-swell when the setting is turned on fades out with it.
pub fn ripple_pools(
    time: Res<Time>,
    settings: Res<crate::app::settings::GameSettings>,
    mut ripples: Query<(&PoolRipple, &mut Transform, &mut Sprite)>,
) {
    if settings.reduced_motion {
        for (_, _, mut sprite) in &mut ripples {
            if sprite.color != Color::NONE {
                sprite.color = Color::NONE;
            }
        }
        return;
    }
    let now = time.elapsed_secs();
    for (ripple, mut transform, mut sprite) in &mut ripples {
        let t = (now / RIPPLE_PERIOD + ripple.phase).fract();
        // From a point to most of the deep water's width, never onto the
        // shallows' edge.
        transform.scale = Vec3::splat(0.12 + 0.5 * t);
        let strength = (t * std::f32::consts::PI).sin();
        sprite.color = Color::srgba(0.88, 0.95, 0.98, 0.42 * strength);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Neighbouring pool tiles turn their puddles their own ways, so a pond
    /// is not a row of one shape, and every turn is within a full circle.
    #[test]
    fn neighbouring_puddles_turn_their_own_ways() {
        for y in 0..6u8 {
            for x in 0..6u8 {
                let turn = pool_turn(tile_hash(x, y));
                assert!((0.0..std::f32::consts::TAU).contains(&turn), "{turn}");
                assert_ne!(
                    tile_hash(x, y),
                    tile_hash(x + 1, y),
                    "({x}, {y}) and its right"
                );
                assert_ne!(tile_hash(x, y), tile_hash(x, y + 1), "({x}, {y}) and below");
            }
        }
    }

    /// With less motion asked for, a pool's ripple stays the clear water
    /// it is spawned as, and one already showing fades out.
    #[test]
    fn pools_hold_still_for_less_motion() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.insert_resource(crate::app::settings::GameSettings {
            reduced_motion: true,
            ..Default::default()
        });
        app.add_systems(Update, ripple_pools);
        let ripple = app
            .world_mut()
            .spawn((
                PoolRipple { phase: 0.5 },
                Sprite::from_color(Color::WHITE, Vec2::ONE),
                Transform::default(),
            ))
            .id();
        app.update();
        let sprite = app.world().get::<Sprite>(ripple).expect("a ripple");
        assert_eq!(sprite.color, Color::NONE);
    }

    fn pond(width: u8, height: u8, pools: &[(u8, u8)]) -> Vec<PondPiece> {
        let mut board = Board::new(width, height, 1);
        for &(x, y) in pools {
            board.set_tile(x, y, TileKind::Pool);
        }
        pond_pieces(&board)
    }

    fn count(pieces: &[PondPiece]) -> (usize, usize, usize) {
        let tiles = pieces
            .iter()
            .filter(|p| matches!(p, PondPiece::Tile { .. }));
        let bridges = pieces
            .iter()
            .filter(|p| matches!(p, PondPiece::Bridge { .. }));
        let corners = pieces
            .iter()
            .filter(|p| matches!(p, PondPiece::Corner { .. }));
        (tiles.count(), bridges.count(), corners.count())
    }

    /// A lone pool is one puddle, with nothing to bridge to.
    #[test]
    fn a_lone_pool_is_one_puddle() {
        assert_eq!(count(&pond(5, 5, &[(2, 2)])), (1, 0, 0));
    }

    /// A lane is one pond: every neighbouring pair bridged, once, halfway
    /// between the two, and turned along the lane.
    #[test]
    fn a_lane_is_bridged_between_every_pair() {
        let across = pond(6, 3, &[(1, 1), (2, 1), (3, 1)]);
        assert_eq!(count(&across), (3, 2, 0));
        let down = pond(3, 6, &[(1, 1), (1, 2), (1, 3)]);
        assert_eq!(count(&down), (3, 2, 0));
        for piece in down {
            if let PondPiece::Bridge { at, turn } = piece {
                assert_eq!(at.x, 0.0, "down the middle column");
                assert_eq!(at.y % TILE, 0.0, "on the edge between two rows: {at}");
                assert_eq!(turn, std::f32::consts::FRAC_PI_2, "along the lane");
            }
        }
    }

    /// Four pool tiles in a square bridge on all four sides and fill the
    /// corner they share, or the middle of the pond is dry sand.
    #[test]
    fn a_square_of_pools_has_no_dry_middle() {
        let square = pond(4, 4, &[(1, 1), (2, 1), (1, 2), (2, 2)]);
        assert_eq!(count(&square), (4, 4, 1));
        let corner = square.iter().find_map(|p| {
            if let PondPiece::Corner { at, .. } = p {
                Some(*at)
            } else {
                None
            }
        });
        assert_eq!(corner, Some(Vec2::ZERO), "the square's middle, dead centre");
    }

    /// Diagonal neighbours touch only at a corner, which is not a shore
    /// the water can cross: two puddles, no bridge.
    #[test]
    fn diagonal_pools_stay_apart() {
        assert_eq!(count(&pond(4, 4, &[(1, 1), (2, 2)])), (2, 0, 0));
    }
}
