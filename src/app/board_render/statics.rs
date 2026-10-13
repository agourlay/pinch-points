//! The unchanging beach and the things drawn straight from tile state:
//! sand, rocks, walls, terrain, spawner holes, and the pivoting driftwood
//! turnstiles.

use super::pools::spawn_pools;
use super::surround::{spawn_dusk_shore, spawn_weather};
use super::{Side, image_sprite, on_board, z};
use crate::app::Sim;
use crate::app::art::Art;
use crate::app::layout::{self, TILE};
use crate::sim::{Board, Direction, TileKind};
use bevy::prelude::*;

/// Everything spawned by [`spawn_static_board`]; despawned wholesale when a
/// different board loads.
#[derive(Component)]
pub struct BoardStatic;

/// The sand under one tile. The texture is rotated by a hash of the
/// coordinates so the checker does not visibly repeat; arbitrary, but
/// stable, so a board looks the same every time it loads.
fn spawn_sand(commands: &mut Commands, art: &Art, pos: Vec2, x: u8, y: u8) {
    let sand = if (x + y).is_multiple_of(2) {
        &art.sand_a
    } else {
        &art.sand_b
    };
    let quarter_turns = f32::from((x.wrapping_mul(7).wrapping_add(y.wrapping_mul(13))) % 4);
    commands.spawn((
        BoardStatic,
        image_sprite(sand, Color::WHITE, Vec2::splat(TILE)),
        Transform::from_translation(pos.extend(z::SAND)).with_rotation(Quat::from_rotation_z(
            quarter_turns * std::f32::consts::FRAC_PI_2,
        )),
    ));
}

/// The soft blob a thing standing on the sand casts.
///
/// Without one, rocks, kelp and the fence float, and a beach lit from
/// nowhere reads as a diagram. One sprite per feature is the cheapest
/// depth there is.
fn ground_shadow(commands: &mut Commands, art: &Art, pos: Vec2, size: f32) {
    commands.spawn((
        BoardStatic,
        image_sprite(
            &art.shadow,
            Color::srgba(1.0, 1.0, 1.0, 0.75),
            Vec2::new(size, size * 0.82),
        ),
        Transform::from_translation((pos + layout::SUN).extend(z::TILE_FEATURE - 0.05)),
    ));
}

/// Whatever sits on one tile, if it is a thing that never changes. Castles
/// and turnstile logs are drawn live elsewhere: a castle's look follows its
/// score tier, and a log's tilt flips on every crossing.
fn spawn_tile_feature(
    commands: &mut Commands,
    art: &Art,
    (x, y): (u8, u8),
    pos: Vec2,
    kind: TileKind,
) {
    match kind {
        TileKind::Empty => {}
        TileKind::Rock => {
            ground_shadow(commands, art, pos, TILE * 0.98);
            commands.spawn((
                BoardStatic,
                image_sprite(&art.rock, Color::WHITE, Vec2::splat(TILE * 0.94)),
                Transform::from_translation(pos.extend(z::TILE_FEATURE)),
            ));
        }
        // Castles are drawn by `sync_castles`: their look depends on
        // the live score tier (spec §3.4: the castle IS the
        // scoreboard), so they cannot be static.
        TileKind::Castle(_) => {}
        TileKind::Spawner(spawner) => {
            commands.spawn((
                BoardStatic,
                SpawnerSprite {
                    period: spawner.period,
                    at: (x, y),
                },
                image_sprite(&art.hole, Color::WHITE, Vec2::splat(TILE * 0.78)),
                Transform::from_translation(pos.extend(z::TILE_FEATURE)),
            ));
        }
        TileKind::Kelp => {
            ground_shadow(
                commands,
                art,
                pos + Vec2::new(0.0, -TILE * 0.22),
                TILE * 0.7,
            );
            commands.spawn((
                BoardStatic,
                image_sprite(&art.kelp, Color::WHITE, Vec2::splat(TILE * 0.94)),
                Transform::from_translation(pos.extend(z::TILE_FEATURE)),
            ));
        }
        // Drawn by `spawn_pools`: a pool's shore depends on its
        // neighbours, which this tile alone cannot see.
        TileKind::Pool => {}
        // The log is dynamic (its tilt flips per crossing); only a
        // shadow pad is static. Cast the way every other shadow is: it
        // sat dead under the log, the one thing on the beach lit from
        // straight above.
        TileKind::Turnstile { .. } => {
            commands.spawn((
                BoardStatic,
                image_sprite(&art.shadow, Color::WHITE, Vec2::splat(TILE * 0.8)),
                Transform::from_translation((pos + layout::SUN).extend(z::POOL)),
            ));
        }
    }
}

/// Everything about a board that never moves: the sand, what sits on each
/// tile, and the driftwood fence around and through it.
pub fn spawn_static_board(commands: &mut Commands, board: &Board, art: &Art) {
    spawn_dusk_shore(commands, board, art);
    for (x, y, kind) in board.tiles() {
        let pos = layout::tile_center(board, x, y);
        spawn_sand(commands, art, pos, x, y);
        spawn_tile_feature(commands, art, (x, y), pos, kind);
    }
    spawn_pools(commands, board, art);
    spawn_walls(commands, board, art);
    spawn_weather(commands, board, art);
}

/// Iterates every tile's Up and Left edges plus the far borders, so each
/// edge is drawn exactly once.
fn spawn_walls(commands: &mut Commands, board: &Board, art: &Art) {
    let plank_size = Vec2::new(TILE + 8.0, 13.0);
    let plank = |commands: &mut Commands, center: Vec2, vertical: bool| {
        let rotation = if vertical {
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)
        } else {
            Quat::IDENTITY
        };
        // Every plank drops one, the frame and the interior runs alike, so
        // the fence stands in the sand at the same height wherever it is.
        // The offset is in world space and the rotation is applied after
        // it, so the shadow is its own entity rather than a child.
        commands.spawn((
            BoardStatic,
            image_sprite(&art.plank, Color::srgba(0.10, 0.07, 0.05, 0.34), plank_size),
            // Below the tile features, where every other ground shadow
            // sits. At the fence's own depth it drew *over* creatures -
            // the plank occludes them on purpose, its shadow should not.
            Transform::from_translation((center + layout::SUN).extend(z::TILE_FEATURE - 0.06))
                .with_rotation(rotation),
        ));
        commands.spawn((
            BoardStatic,
            image_sprite(&art.plank, Color::WHITE, plank_size),
            Transform::from_translation(center.extend(z::WALL)).with_rotation(rotation),
        ));
    };
    // Corner posts where interior wall segments end or meet tie the fence
    // together visually. Corner (cx, cy) sits at the top-left of tile
    // (cx, cy); borders are skipped (the frame covers them).
    let mut corner_hits = vec![0u8; (board.width() as usize + 1) * (board.height() as usize + 1)];
    let corner_index = |cx: u16, cy: u16| cy as usize * (board.width() as usize + 1) + cx as usize;
    for (x, y, _) in board.tiles() {
        let pos = layout::tile_center(board, x, y);
        if board.wall_at(x, y, Direction::Up) {
            plank(commands, pos + Vec2::new(0.0, TILE / 2.0), false);
            corner_hits[corner_index(u16::from(x), u16::from(y))] += 1;
            corner_hits[corner_index(u16::from(x) + 1, u16::from(y))] += 1;
        }
        if board.wall_at(x, y, Direction::Left) {
            plank(commands, pos + Vec2::new(-TILE / 2.0, 0.0), true);
            corner_hits[corner_index(u16::from(x), u16::from(y))] += 1;
            corner_hits[corner_index(u16::from(x), u16::from(y) + 1)] += 1;
        }
        if y == board.height() - 1 && board.wall_at(x, y, Direction::Down) {
            plank(commands, pos + Vec2::new(0.0, -TILE / 2.0), false);
        }
        if x == board.width() - 1 && board.wall_at(x, y, Direction::Right) {
            plank(commands, pos + Vec2::new(TILE / 2.0, 0.0), true);
        }
    }
    let extent = layout::board_extent(board);
    let (w, h) = (extent.x, extent.y);
    for cy in 1..u16::from(board.height()) {
        for cx in 1..u16::from(board.width()) {
            if corner_hits[corner_index(cx, cy)] == 0 {
                continue;
            }
            let corner = Vec2::new(
                f32::from(cx) * TILE - w / 2.0,
                h / 2.0 - f32::from(cy) * TILE,
            );
            commands.spawn((
                BoardStatic,
                image_sprite(&art.post, Color::WHITE, Vec2::splat(15.0)),
                Transform::from_translation(corner.extend(z::WALL + 0.1)),
            ));
        }
    }

    // Wet-sand fringe: a damp gradient just outside the sand, fading toward
    // the water. The wet.png strip fades downward; turned per side.
    for side in Side::ALL {
        let (along, across) = side.span(extent);
        let offset = side.normal() * (across / 2.0 + 9.0);
        commands.spawn((
            BoardStatic,
            image_sprite(&art.wet, Color::WHITE, Vec2::new(along + 36.0, 18.0)),
            Transform::from_translation(offset.extend(z::WET))
                .with_rotation(Quat::from_rotation_z(side.rotation())),
        ));
    }
}

/// A turnstile's log sprite; `right` is the tilt it was drawn with.
#[derive(Component)]
pub struct TurnstileSprite {
    x: u8,
    y: u8,
    right: bool,
}

/// Diff turnstile logs against the board: the log leans toward the side it
/// will deflect to next; `animate_turnstiles` swings it there smoothly.
pub fn sync_turnstiles(
    mut commands: Commands,
    sim: Res<Sim>,
    art: Res<Art>,
    mut existing: Query<(Entity, &mut TurnstileSprite)>,
) {
    let board = &sim.0;
    let mut covered: Vec<(u8, u8)> = Vec::new();
    for (entity, mut sprite) in &mut existing {
        if !on_board(board, sprite.x, sprite.y) {
            commands.entity(entity).despawn();
            continue;
        }
        match board.tile_at(sprite.x, sprite.y) {
            TileKind::Turnstile { next_right } => {
                // Retarget in place; the animation system swings the log.
                if next_right != sprite.right {
                    sprite.right = next_right;
                }
                covered.push((sprite.x, sprite.y));
            }
            TileKind::Empty
            | TileKind::Rock
            | TileKind::Castle(_)
            | TileKind::Spawner(_)
            | TileKind::Kelp
            | TileKind::Pool => commands.entity(entity).despawn(),
        }
    }
    for (x, y, kind) in board.tiles() {
        let TileKind::Turnstile { next_right } = kind else {
            continue;
        };
        if covered.contains(&(x, y)) {
            continue;
        }
        let tilt = if next_right { -0.6 } else { 0.6 };
        commands.spawn((
            TurnstileSprite {
                x,
                y,
                right: next_right,
            },
            // Square, as the art is drawn: the log's own shape is in the
            // picture. Squeezed to a strip, the drawing inside came out a
            // sliver a few pixels thick.
            image_sprite(&art.log, Color::WHITE, Vec2::splat(TILE * 0.98)),
            Transform::from_translation(
                layout::tile_center(board, x, y).extend(z::TILE_FEATURE + 0.1),
            )
            .with_rotation(Quat::from_rotation_z(tilt)),
        ));
    }
}

/// Marker for spawner holes; carries the cadence so the hole can swell just
/// before it emits (the telegraph).
#[derive(Component)]
pub struct SpawnerSprite {
    period: u32,
    /// Its tile, for telling the hole a golden crab is called to.
    at: (u8, u8),
}

/// Swell each spawner hole in the last quarter of its cadence, and the hole
/// a golden crab is called to for the whole of its notice
/// (`Board::golden_call`), with `effects::golden_call_rings` calling
/// attention to it.
pub fn pulse_spawners(sim: Res<Sim>, mut holes: Query<(&SpawnerSprite, &mut Transform)>) {
    let tick = sim.0.ticks();
    let called = sim.0.golden_call().map(|(_, x, y)| (x, y));
    for (hole, mut transform) in &mut holes {
        if called == Some(hole.at) {
            transform.scale = Vec3::splat(1.3);
            continue;
        }
        if hole.period == 0 {
            continue;
        }
        let phase = (tick % u64::from(hole.period)) as f32 / hole.period as f32;
        let swell = if phase > 0.75 {
            1.0 + (phase - 0.75) * 0.6
        } else {
            1.0
        };
        transform.scale = Vec3::splat(swell);
    }
}

/// Swing each log toward its target tilt instead of snapping.
pub fn animate_turnstiles(time: Res<Time>, mut logs: Query<(&TurnstileSprite, &mut Transform)>) {
    for (sprite, mut transform) in &mut logs {
        let target = if sprite.right { -0.6 } else { 0.6 };
        let (_, _, current) = transform.rotation.to_euler(EulerRot::XYZ);
        let next = current + (target - current) * (time.delta_secs() * 14.0).min(1.0);
        if (next - current).abs() > 0.0005 {
            transform.rotation = Quat::from_rotation_z(next);
        }
    }
}
