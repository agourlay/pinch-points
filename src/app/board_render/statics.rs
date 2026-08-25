//! The unchanging beach and the things drawn straight from tile state:
//! sand, rocks, walls, terrain, spawner holes, signposts, and the pivoting
//! driftwood turnstiles.

use super::{image_sprite, on_board, z};
use crate::app::Sim;
use crate::app::art::Art;
use crate::app::layout::{self, TILE};
use crate::app::palette;
use crate::sim::{Board, Direction, SignpostHealth, TileKind};
use bevy::prelude::*;

/// Marker for a signpost sprite; carries the state it was built from so the
/// sync system can tell when the board's signpost changed under it.
#[derive(Component)]
pub struct SignpostSprite {
    x: u8,
    y: u8,
    dir: Direction,
    owner: u8,
    health: SignpostHealth,
    /// Quantized remaining lifetime (versus posts fade out); part of the
    /// diff key so the sprite rebuilds as it ages.
    fade_bucket: u8,
}

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

/// Whatever sits on one tile, if it is a thing that never changes. Castles
/// and turnstile logs are drawn live elsewhere: a castle's look follows its
/// score tier, and a log's tilt flips on every crossing.
fn spawn_tile_feature(commands: &mut Commands, art: &Art, pos: Vec2, kind: TileKind) {
    match kind {
        TileKind::Empty => {}
        TileKind::Rock => {
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
                },
                image_sprite(&art.hole, Color::WHITE, Vec2::splat(TILE * 0.78)),
                Transform::from_translation(pos.extend(z::TILE_FEATURE)),
            ));
        }
        TileKind::Kelp => {
            commands.spawn((
                BoardStatic,
                image_sprite(&art.kelp, Color::WHITE, Vec2::splat(TILE * 0.94)),
                Transform::from_translation(pos.extend(z::TILE_FEATURE)),
            ));
        }
        TileKind::Pool => {
            commands.spawn((
                BoardStatic,
                image_sprite(&art.pool, Color::WHITE, Vec2::splat(TILE * 0.98)),
                // Under creatures and signposts: they wade over it.
                Transform::from_translation(pos.extend(z::POOL)),
            ));
        }
        // The log is dynamic (its tilt flips per crossing); only a
        // shadow pad is static.
        TileKind::Turnstile { .. } => {
            commands.spawn((
                BoardStatic,
                image_sprite(&art.shadow, Color::WHITE, Vec2::splat(TILE * 0.8)),
                Transform::from_translation(pos.extend(z::POOL)),
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
        spawn_tile_feature(commands, art, pos, kind);
    }
    spawn_walls(commands, board, art);
}

/// The world behind the board: dusk sand to every edge of the window and
/// the sea lying along the top, so a round is played on the same beach the
/// menu promised instead of floating in a grey void. Sized from the board
/// rather than the window, because the camera zooms to fit the board and
/// world units are the only ones that hold still while it does.
fn spawn_dusk_shore(commands: &mut Commands, board: &Board, art: &Art) {
    // Far larger than any zoomed-out view can reach.
    const REACH: f32 = 9000.0;
    let top = f32::from(board.height()) * TILE / 2.0 + 26.0;
    let plane = |size: Vec2, at: Vec2, z: f32, color: Color| {
        (
            BoardStatic,
            Sprite::from_color(color, size),
            Transform::from_translation(at.extend(z)),
        )
    };
    // Sand under and around everything.
    commands.spawn(plane(
        Vec2::splat(REACH),
        Vec2::ZERO,
        z::SAND - 10.0,
        Color::srgb(0.38, 0.34, 0.25),
    ));
    // The sea along the top of the beach, deepening away from the shore.
    let shore = top + TILE * 0.55;
    commands.spawn(plane(
        Vec2::new(REACH, REACH / 2.0),
        Vec2::new(0.0, shore + REACH / 4.0),
        z::SAND - 9.8,
        Color::srgb(0.13, 0.24, 0.33),
    ));
    commands.spawn(plane(
        Vec2::new(REACH, 26.0),
        Vec2::new(0.0, shore + 13.0),
        z::SAND - 9.7,
        Color::srgb(0.18, 0.32, 0.42),
    ));
    // The wet line where the last wave reached, and its foam lip.
    commands.spawn(plane(
        Vec2::new(REACH, 18.0),
        Vec2::new(0.0, shore - 9.0),
        z::SAND - 9.7,
        Color::srgb(0.31, 0.29, 0.22),
    ));
    commands.spawn((
        BoardStatic,
        Sprite {
            image: art.foam.clone(),
            color: Color::srgba(1.0, 1.0, 1.0, 0.28),
            custom_size: Some(Vec2::new(REACH, 12.0)),
            ..default()
        },
        Transform::from_translation(Vec3::new(0.0, shore + 2.0, z::SAND - 9.6)),
    ));
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
    let (w, h) = (
        f32::from(board.width()) * TILE,
        f32::from(board.height()) * TILE,
    );
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
    // the water. The wet.png strip fades downward; rotate per side.
    for (size, offset, rot) in [
        (
            Vec2::new(w + 36.0, 18.0),
            Vec2::new(0.0, h / 2.0 + 9.0),
            std::f32::consts::PI,
        ),
        (
            Vec2::new(w + 36.0, 18.0),
            Vec2::new(0.0, -h / 2.0 - 9.0),
            0.0,
        ),
        (
            Vec2::new(h + 36.0, 18.0),
            Vec2::new(-w / 2.0 - 9.0, 0.0),
            -std::f32::consts::FRAC_PI_2,
        ),
        (
            Vec2::new(h + 36.0, 18.0),
            Vec2::new(w / 2.0 + 9.0, 0.0),
            std::f32::consts::FRAC_PI_2,
        ),
    ] {
        commands.spawn((
            BoardStatic,
            image_sprite(&art.wet, Color::WHITE, size),
            Transform::from_translation(offset.extend(z::WET))
                .with_rotation(Quat::from_rotation_z(rot)),
        ));
    }
}

/// Diff the signpost sprites against the sim every frame: despawn sprites whose
/// signpost is gone or changed, spawn sprites for signposts without one.
pub fn sync_signposts(
    mut commands: Commands,
    sim: Res<Sim>,
    art: Res<Art>,
    mut covered: Local<Vec<bool>>,
    existing: Query<(Entity, &SignpostSprite)>,
) {
    let board = &sim.0;
    let bucket = |fade: f32| (fade * 6.0).ceil().clamp(0.0, 6.0) as u8;
    covered.clear();
    covered.resize(board.width() as usize * board.height() as usize, false);
    for (entity, sprite) in &existing {
        if !on_board(board, sprite.x, sprite.y) {
            commands.entity(entity).despawn();
            continue;
        }
        let idx = sprite.y as usize * board.width() as usize + sprite.x as usize;
        match board.signpost_at(sprite.x, sprite.y) {
            Some(sp)
                if sp.dir == sprite.dir
                    && sp.owner == sprite.owner
                    && sp.health == sprite.health
                    && bucket(board.signpost_fade(&sp)) == sprite.fade_bucket =>
            {
                covered[idx] = true;
            }
            Some(_) | None => commands.entity(entity).despawn(),
        }
    }
    for (x, y, _) in board.tiles() {
        let idx = y as usize * board.width() as usize + x as usize;
        if covered[idx] {
            continue;
        }
        let Some(sp) = board.signpost_at(x, y) else {
            continue;
        };
        let fade = board.signpost_fade(&sp);
        let alpha = match sp.health {
            SignpostHealth::Full => 1.0,
            SignpostHealth::Worn => 0.5,
        } * (0.25 + 0.75 * fade);
        let pos = layout::tile_center(board, x, y);
        // One clear owner-coloured arrow, fading as it ages and wears.
        commands.spawn((
            SignpostSprite {
                x,
                y,
                dir: sp.dir,
                owner: sp.owner,
                health: sp.health,
                fade_bucket: bucket(fade),
            },
            image_sprite(
                &art.arrow,
                palette::player_color(sp.owner)
                    .lighter(0.12)
                    .with_alpha(alpha),
                Vec2::splat(TILE * 0.88),
            ),
            Transform::from_translation(pos.extend(z::SIGNPOST))
                .with_rotation(layout::dir_rotation(sp.dir)),
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
            image_sprite(&art.log, Color::WHITE, Vec2::new(TILE * 0.94, TILE * 0.3)),
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
}

/// Swell each spawner hole in the last quarter of its cadence.
pub fn pulse_spawners(sim: Res<Sim>, mut holes: Query<(&SpawnerSprite, &mut Transform)>) {
    let tick = sim.0.ticks();
    for (hole, mut transform) in &mut holes {
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
