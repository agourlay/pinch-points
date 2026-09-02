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
///
/// Wear and age are deliberately *not* part of that key. They used to be:
/// a quantized fade bucket meant a post was despawned and respawned six
/// times over its life, which is cheap enough but leaves the sprite with
/// no memory - and a post that is rebuilt every second cannot be given an
/// animation, because the animation restarts with it. They are written in
/// place by [`dress_signposts`] instead, and the sprite now lives from the
/// moment it is planted to the moment it is pulled.
#[derive(Component)]
pub struct SignpostSprite {
    x: u8,
    y: u8,
    dir: Direction,
    owner: u8,
    /// Seconds since it was planted. Drives the plant pop, and nothing else.
    age: f32,
    /// The tick the sim says this post was planted or re-pointed on.
    ///
    /// Pressing the same direction again on your own fading post is normal
    /// versus play: the sim stamps it `Full` with a fresh `placed`, and
    /// nothing else changes - not the tile, not the heading, not the
    /// owner. So the diff key sees no change, the differ raises no
    /// `SignpostPlaced`, and there is no ring and no knock. Without this
    /// the post simply snapped from worn and faint to fresh and bright
    /// between two frames with nothing marking it.
    planted: u64,
}

/// The soft dark copy of the arrow lying under it.
///
/// A child rather than baked into the art, because the arrow is tinted to
/// its owner's colour at spawn and a tint takes the whole sprite with it:
/// a baked shadow would come out red for one seat and blue for the next.
#[derive(Component)]
pub struct SignpostShadow;

/// How long a post takes to settle into the sand after it is planted.
const PLANT_POP: f32 = 0.17;

/// How big the plant draws a post, `age` seconds after it went in.
///
/// Driven in, overshooting, settling. `cos` through one and a half turns
/// lands exactly on rest, so the post finishes the size the board expects
/// and holds there rather than a hair off it for ever.
fn plant_pop(age: f32) -> f32 {
    if age >= PLANT_POP {
        return 1.0;
    }
    let t = age / PLANT_POP;
    1.0 + 0.35 * (1.0 - t) * (t * std::f32::consts::PI * 1.5).cos()
}

/// How strongly a post draws with `fade` of its life left.
///
/// Floored well short of transparent. Wear used to be spelled with alpha
/// alone and a worn, aged post came out at an eighth of full strength on
/// bright sand - as the one thing on the board the player steers with.
fn post_alpha(fade: f32) -> f32 {
    0.55 + 0.45 * fade
}

/// How much a post shrinks as it runs out.
///
/// The alpha floor above costs the fade most of the range it used to speak
/// in, so an expiring post withers as well as dimming. Campaign posts
/// never fade, so this never touches a puzzle.
fn wither(fade: f32) -> f32 {
    0.86 + 0.14 * fade
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

/// The soft blob a thing standing on the sand casts.
///
/// Rocks, kelp and the fence used to float: nothing on a tile cast
/// anything, and a beach lit from nowhere reads as a diagram. One sprite
/// per feature is the cheapest depth there is.
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
fn spawn_tile_feature(commands: &mut Commands, art: &Art, pos: Vec2, kind: TileKind) {
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
    spawn_weather(commands, board, art);
}

/// How far past the board a drifting thing turns round, in world units:
/// clear of any window the camera's clamped zoom can show.
const REACH: f32 = 2400.0;

/// A shadow of a cloud nobody can see, crossing the beach.
#[derive(Component)]
pub struct CloudShadow {
    /// Pixels per second, signed: half of them come the other way.
    speed: f32,
    /// How far out the shadow turns round, in world x.
    edge: f32,
}

/// The light over the board: a vignette that sinks the corners, and the
/// shadows of clouds crossing the sand.
///
/// Both sit between the sand and everything that stands on it. That is the
/// load-bearing choice: a vignette over the whole scene would dim the
/// crabs and posts in the corners too, and those are the pieces a player
/// is reading under time pressure. The ground gets a centre; the pieces
/// stay exactly as bright as they were.
fn spawn_weather(commands: &mut Commands, board: &Board, art: &Art) {
    let w = f32::from(board.width()) * TILE;
    let h = f32::from(board.height()) * TILE;
    commands.spawn((
        BoardStatic,
        image_sprite(&art.vignette, Color::WHITE, Vec2::new(w + 40.0, h + 40.0)),
        Transform::from_translation(Vec3::new(0.0, 0.0, z::POOL - 0.05)),
    ));
    // Three of them, at different heights, sizes and speeds, so the beach
    // is never quite evenly lit twice.
    //
    // They turn round well outside the window, not just outside the board.
    // `boot::fit_camera` stops zooming in at 0.8, so a small board sits in
    // a much larger visible beach, and a shadow wrapping at the board's
    // edge vanished and reappeared in plain sight mid-sand. The closing
    // wave carries the same reasoning as `wash::REACH`.
    let edge = w / 2.0 + REACH;
    for (i, (span, speed, at)) in [(3.4, 9.0, -0.28), (5.0, -6.0, 0.12), (2.6, 13.0, 0.38)]
        .into_iter()
        .enumerate()
    {
        commands.spawn((
            BoardStatic,
            CloudShadow { speed, edge },
            image_sprite(
                &art.cloud,
                Color::srgba(0.10, 0.13, 0.20, 0.10),
                Vec2::new(span * TILE, span * TILE * 0.62),
            ),
            // Spread across the *board*, not across the wrap distance:
            // `edge` is how far out they turn round, and starting them
            // there put two of the three so far off screen that one took
            // two minutes to drift into view and the beach spent nearly a
            // whole round under a single shadow.
            Transform::from_translation(Vec3::new(
                -w / 2.0 + (i as f32 + 0.5) / 3.0 * w,
                at * h,
                z::POOL - 0.08,
            )),
        ));
    }
}

/// Drift the cloud shadows across the sand, turning them round at the far
/// edge so the beach never runs out of weather.
pub fn drift_cloud_shadows(
    time: Res<Time>,
    settings: Res<crate::app::settings::GameSettings>,
    mut clouds: Query<(&CloudShadow, &mut Transform)>,
) {
    if settings.reduced_motion {
        return;
    }
    let dt = time.delta_secs();
    for (cloud, mut transform) in &mut clouds {
        transform.translation.x += cloud.speed * dt;
        if transform.translation.x > cloud.edge {
            transform.translation.x = -cloud.edge;
        } else if transform.translation.x < -cloud.edge {
            transform.translation.x = cloud.edge;
        }
    }
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
        // Every plank drops one, the frame and the interior runs alike, so
        // the fence reads as standing in the sand at the same height
        // wherever it is. The offset is in world space and the rotation is
        // applied after it, which is why the shadow is its own entity
        // rather than a child of the plank.
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

/// Diff the signpost sprites against the sim every frame: despawn sprites
/// whose signpost is gone or was replaced, spawn sprites for signposts
/// without one. Wear and fade are [`dress_signposts`]'s to write.
pub fn sync_signposts(
    mut commands: Commands,
    sim: Res<Sim>,
    art: Res<Art>,
    mut covered: Local<Vec<bool>>,
    existing: Query<(Entity, &SignpostSprite)>,
) {
    let board = &sim.0;
    covered.clear();
    covered.resize(board.width() as usize * board.height() as usize, false);
    for (entity, sprite) in &existing {
        if !on_board(board, sprite.x, sprite.y) {
            commands.entity(entity).despawn();
            continue;
        }
        let idx = sprite.y as usize * board.width() as usize + sprite.x as usize;
        match board.signpost_at(sprite.x, sprite.y) {
            Some(sp) if sp.dir == sprite.dir && sp.owner == sprite.owner => covered[idx] = true,
            // A different heading or a different owner on the same tile is
            // a different post, and gets its own plant.
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
        let pos = layout::tile_center(board, x, y);
        // One clear owner-coloured arrow, over a shadow of itself so it
        // reads as standing in the sand rather than painted on it. The
        // colour and the art are written on the first frame by
        // `dress_signposts`; what is set here is only what it starts as.
        commands
            .spawn((
                SignpostSprite {
                    x,
                    y,
                    dir: sp.dir,
                    owner: sp.owner,
                    age: 0.0,
                    planted: sp.placed,
                },
                image_sprite(
                    &art.arrow,
                    palette::player_color(sp.owner).lighter(0.12),
                    Vec2::splat(TILE * 0.88),
                ),
                Transform::from_translation(pos.extend(z::SIGNPOST))
                    .with_rotation(layout::dir_rotation(sp.dir)),
            ))
            .with_children(|parent| {
                parent.spawn((
                    SignpostShadow,
                    image_sprite(
                        &art.arrow,
                        Color::srgba(0.14, 0.10, 0.06, 0.34),
                        Vec2::splat(TILE * 0.88),
                    ),
                    // Down-and-right in the arrow's own frame would swing
                    // with its heading; the offset is applied in world
                    // space by `dress_signposts` for that reason.
                    Transform::from_translation(Vec3::new(0.0, 0.0, -0.05)),
                ));
            });
    }
}

/// Write what a post looks like *now*: how far it has settled after being
/// planted, how worn it is, and how much life it has left.
///
/// Wear keeps a post's ink and takes its edges instead of dimming it away
/// (see [`post_alpha`]): [`crate::app::art::Art::arrow_worn`] is the same
/// arrow with splinters bitten out of it.
#[allow(clippy::type_complexity)]
pub fn dress_signposts(
    time: Res<Time>,
    sim: Res<Sim>,
    art: Res<Art>,
    settings: Res<crate::app::settings::GameSettings>,
    mut posts: Query<(&mut SignpostSprite, &mut Sprite, &mut Transform, &Children)>,
    mut shadows: Query<
        (&mut Sprite, &mut Transform),
        (With<SignpostShadow>, Without<SignpostSprite>),
    >,
) {
    let board = &sim.0;
    let dt = time.delta_secs();
    for (mut post, mut sprite, mut transform, children) in &mut posts {
        post.age += dt;
        if !on_board(board, post.x, post.y) {
            continue; // `sync_signposts` takes it away this frame
        }
        let Some(sp) = board.signpost_at(post.x, post.y) else {
            continue;
        };
        // Driven in again: the same post, restamped. Replay the plant.
        if sp.placed != post.planted {
            post.planted = sp.placed;
            post.age = 0.0;
        }
        let worn = sp.health == SignpostHealth::Worn;
        let wanted = if worn { &art.arrow_worn } else { &art.arrow };
        if sprite.image != *wanted {
            sprite.image = wanted.clone();
        }
        // Versus posts age out; a puzzle's stand until pulled. What is
        // left of a post's life dims it, but never past the point where it
        // stops reading against the sand.
        let fade = board.signpost_fade(&sp);
        let alpha = post_alpha(fade);
        let mut color = palette::player_color(post.owner).lighter(0.12);
        if worn {
            color = color.darker(0.06);
        }
        sprite.color = color.with_alpha(alpha);
        let pop = if settings.reduced_motion {
            1.0
        } else {
            plant_pop(post.age)
        };
        transform.scale = Vec3::splat(pop * wither(fade));
        for child in children {
            let Ok((mut shadow, mut shadow_tf)) = shadows.get_mut(*child) else {
                continue;
            };
            shadow.color = Color::srgba(0.14, 0.10, 0.06, 0.34 * alpha);
            if shadow.image != *wanted {
                shadow.image = wanted.clone();
            }
            // The sun is one direction for the whole beach, so the offset
            // is undone out of the arrow's rotation: a post pointing left
            // and a post pointing up drop their shadow the same way.
            // Divided by the pop, for the reason the creatures' shadows
            // are divided by theirs: a child's translation is in the
            // parent's frame, so the plant's 1.35x overshoot would fling
            // the shadow out and drag it back in again.
            let scale = transform.scale.x.max(f32::EPSILON);
            let local = transform.rotation.inverse() * (layout::SUN / scale).extend(0.0);
            shadow_tf.translation = Vec3::new(local.x, local.y, -0.05);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The plant has to hand over to rest without a step in it.
    ///
    /// Past `PLANT_POP` the size is a flat 1.0, so the curve has to arrive
    /// there on its own: if it ends anywhere else, every post on the beach
    /// visibly jumps on the frame the animation stops. Asserting the value
    /// *at* the boundary proves nothing - that is the flat branch - so
    /// this reads the last moment of the curve itself.
    #[test]
    fn the_plant_hands_over_to_rest_without_a_step() {
        assert!(plant_pop(0.0) > 1.3, "driven in big: {}", plant_pop(0.0));
        let last = plant_pop(PLANT_POP - 1e-4);
        assert!(
            (last - 1.0).abs() < 0.01,
            "the curve ends at {last}, and then snaps to 1.0"
        );
        assert_eq!(plant_pop(PLANT_POP), 1.0, "which is where rest begins");
        assert_eq!(plant_pop(PLANT_POP * 4.0), 1.0, "and stays");
    }

    /// It squashes on the way down rather than easing straight in - that
    /// dip under 1.0 is what makes it read as driven into sand rather than
    /// scaled up - but it must never invert or vanish.
    #[test]
    fn the_plant_squashes_without_ever_turning_inside_out() {
        let mut dipped = false;
        for step in 0..=40 {
            let pop = plant_pop(PLANT_POP * step as f32 / 40.0);
            assert!(pop > 0.5, "collapsed to {pop} at step {step}");
            assert!(pop < 1.4, "blew up to {pop} at step {step}");
            dipped |= pop < 0.98;
        }
        assert!(dipped, "no squash at all: it only ever grows");
    }

    /// Wear is spelled in the art now, not in transparency. Whatever is
    /// left of a post's life, it stays well clear of invisible: it is the
    /// one thing on the board the player is steering with, on bright sand.
    #[test]
    fn a_post_never_fades_past_reading() {
        for step in 0..=20 {
            let alpha = post_alpha(step as f32 / 20.0);
            assert!((0.55..=1.0).contains(&alpha), "{alpha} is off the scale");
        }
        assert_eq!(post_alpha(1.0), 1.0, "a fresh post is at full strength");
        assert!(post_alpha(0.0) >= 0.55, "and a spent one is still legible");
    }

    /// The alpha floor costs the fade most of the range it used to speak
    /// in, so the size carries the rest of the message: a post about to go
    /// is visibly smaller, and never small enough to be mistaken for
    /// somebody else's.
    #[test]
    fn an_expiring_post_withers_but_does_not_shrivel() {
        assert_eq!(wither(1.0), 1.0, "a fresh post is full size");
        assert!(wither(0.0) >= 0.86, "and a spent one is only a little less");
        let mut last = f32::MAX;
        for step in 0..=20 {
            let size = wither(1.0 - step as f32 / 20.0);
            assert!(size <= last, "it grew back at step {step}");
            last = size;
        }
    }
}
