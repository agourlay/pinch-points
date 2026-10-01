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
/// Wear and age are deliberately *not* part of that key. As a quantized
/// fade bucket they had a post despawned and respawned six times over its
/// life, which leaves the sprite with no memory and no animation that can
/// outlive a rebuild. They are written in place by [`dress_signposts`]
/// instead, and the sprite lives from planting to pulling.
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
    /// versus play: the sim stamps it `Full` with a fresh `placed` and
    /// nothing else changes, so the diff key sees nothing, the differ
    /// raises no `SignpostPlaced`, and there is no ring and no knock.
    /// Without this the post snaps from worn to fresh between two frames.
    planted: u64,
}

/// The soft dark copy of the arrow lying under it.
///
/// A child rather than baked into the art, because the arrow is tinted to
/// its owner's colour at spawn and a tint takes the whole sprite with it:
/// a baked shadow would come out red for one seat and blue for the next.
#[derive(Component)]
pub struct SignpostShadow;

/// The arrow painted on a signpost's board: the one part in its owner's
/// colour. A child of the board, so it turns and settles with it.
#[derive(Component)]
pub struct SignpostPaint;

/// A gold glow behind the post the next placement will take: the same
/// silhouette as the shadow, a size up, shown only on that one post.
#[derive(Component)]
pub struct SignpostHalo;

/// How far the halo reaches past the post's own outline.
const HALO_SCALE: f32 = 1.4;

/// How far the next-to-go post rocks either way, in radians: a loose post
/// in the sand, not a spinning one.
const WOBBLE: f32 = 0.12;

/// The board's size on its tile: a touch bigger than the flat arrow was,
/// since the paint on it is what has to read and it sits inside the wood.
const SIGN_SIZE: f32 = TILE * 0.94;

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

/// How strongly a post's paint draws with `fade` of its life left.
///
/// Floored well short of transparent: spelled with alpha alone, a worn,
/// aged post comes out at an eighth of full strength on bright sand, as
/// the one thing on the board the player steers with.
///
/// Only the paint: the board it is on stays at full strength, since the
/// wood against the sand is what finds a post at all, and a faded board is
/// sand-coloured wood fading into sand.
fn post_alpha(fade: f32) -> f32 {
    // Higher than the 0.55 a whole post once faded to: over wood that no
    // longer fades, an aged yellow at 0.55 was cream on tan.
    0.7 + 0.3 * fade
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
        spawn_tile_feature(commands, art, pos, kind);
    }
    spawn_pools(commands, board, art);
    spawn_walls(commands, board, art);
    spawn_weather(commands, board, art);
}

/// Wet sand, shallow water, deep water: a pool's layers, bottom up, each
/// with its tint, its size on a tile, and its size bridging two tiles
/// (along the pair, across it).
const POOL_LAYERS: [(Color, f32, Vec2); 3] = [
    (Color::srgb(0.77, 0.66, 0.48), 1.1, Vec2::new(1.2, 1.0)),
    (Color::srgb(0.44, 0.69, 0.80), 0.98, Vec2::new(1.1, 0.86)),
    (Color::srgb(0.33, 0.57, 0.72), 0.62, Vec2::new(1.0, 0.58)),
];

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
/// It was one tile-sized rounded square per pool tile, which read as a
/// button lying on the sand. Now each tile is an irregular puddle in three
/// flat layers, turned a different way per tile, and every pair of
/// neighbouring pool tiles gets a stretched puddle between them, in the
/// same three layers. Layers of one flat colour merge where they overlap,
/// so a lane of pool tiles reads as one body of water with one shoreline,
/// which is what the campaign's pool-filled lanes are. A two-by-two block
/// gets a puddle over its shared corner too, or its middle is dry.
///
/// All of it sits under creatures and signposts, which wade over it.
fn spawn_pools(commands: &mut Commands, board: &Board, art: &Art) {
    let pieces = pond_pieces(board);
    for (layer, (tint, size, bridge)) in POOL_LAYERS.into_iter().enumerate() {
        let z = z::POOL - 0.03 + 0.01 * layer as f32;
        for piece in &pieces {
            let (at, turn, extent) = match *piece {
                PondPiece::Tile { at, turn } | PondPiece::Corner { at, turn } => {
                    (at, turn, Vec2::splat(size))
                }
                PondPiece::Bridge { at, turn } => (at, turn, bridge),
            };
            commands.spawn((
                BoardStatic,
                image_sprite(&art.puddle, tint, extent * TILE),
                Transform::from_translation(at.extend(z))
                    .with_rotation(Quat::from_rotation_z(turn)),
            ));
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
pub fn ripple_pools(
    time: Res<Time>,
    mut ripples: Query<(&PoolRipple, &mut Transform, &mut Sprite)>,
) {
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

/// How far past the board a drifting cloud shadow turns round, in world
/// units: clear of any window the camera's clamped zoom can show. Not the
/// planes' `wash::REACH`, which is how far the world is *built*; a cloud
/// turning round that far out would be gone for a minute at a time.
const CLOUD_REACH: f32 = 2400.0;

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
/// Both sit between the sand and everything that stands on it: a vignette
/// over the whole scene would dim the crabs and posts in the corners too,
/// and those are the pieces a player reads under time pressure.
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
    // They turn round well outside the window, not just outside the board:
    // `boot::fit_camera` fits the board to the tighter of the window's two
    // sides and stops zooming at `layout::MAX_ZOOM`, so a board sits in a
    // wider visible beach, and a shadow wrapping at the board's edge
    // vanishes in plain sight mid-sand.
    let edge = w / 2.0 + CLOUD_REACH;
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
            // there leaves two of the three so far off screen that the
            // beach spends most of a round under a single shadow.
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
/// The dry sand the board is staked out on: a shade darker and duller than
/// the board's own sand, so the play stays the brightest thing on screen.
/// It was a dim olive, which read as nowhere at all rather than as beach.
const SURROUND_SAND: Color = Color::srgb(0.70, 0.61, 0.45);

fn spawn_dusk_shore(commands: &mut Commands, board: &Board, art: &Art) {
    // Far larger than any zoomed-out view can reach: the wave's reach.
    const REACH: f32 = super::wash::REACH;
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
        SURROUND_SAND,
    ));
    // The sea along the top of the beach, deepening away from the shore.
    let shore = top + TILE * 0.55;
    commands.spawn(plane(
        Vec2::new(REACH, REACH / 2.0),
        Vec2::new(0.0, shore + REACH / 4.0),
        z::SAND - 9.8,
        Color::srgb(0.13, 0.30, 0.42),
    ));
    commands.spawn(plane(
        Vec2::new(REACH, 26.0),
        Vec2::new(0.0, shore + 13.0),
        z::SAND - 9.7,
        Color::srgb(0.20, 0.42, 0.55),
    ));
    // The wet line where the last wave reached, and its foam lip.
    commands.spawn(plane(
        Vec2::new(REACH, 18.0),
        Vec2::new(0.0, shore - 9.0),
        z::SAND - 9.7,
        Color::srgb(0.58, 0.50, 0.37),
    ));
    commands.spawn((
        BoardStatic,
        Sprite {
            image: art.foam.clone(),
            color: Color::srgba(1.0, 1.0, 1.0, 0.45),
            custom_size: Some(Vec2::new(REACH, 12.0)),
            ..default()
        },
        Transform::from_translation(Vec3::new(0.0, shore + 2.0, z::SAND - 9.6)),
    ));
    // Crests further out, fainter, thinner and more broken toward the
    // horizon, so the strip of blue is a sea coming in rather than a band
    // of colour. Short lengths with gaps between: one crest the width of
    // the world was a ruled line.
    let mut rng = crate::app::effects::VisualRng::seeded(board.seed() as u32 ^ 0x5EA);
    let span = f32::from(board.width()) * TILE / 2.0 + MARGIN_TILES * TILE;
    for (lift, alpha, depth) in [(30.0, 0.34, 9.0), (62.0, 0.24, 7.0), (104.0, 0.15, 5.0)] {
        let mut x = -span;
        while x < span {
            let length = rng.range(70.0, 190.0);
            commands.spawn((
                BoardStatic,
                Sprite {
                    image: art.foam.clone(),
                    color: Color::srgba(1.0, 1.0, 1.0, alpha),
                    custom_size: Some(Vec2::new(length, depth)),
                    ..default()
                },
                Transform::from_translation(Vec3::new(
                    x + length / 2.0,
                    shore + lift + rng.range(-5.0, 5.0),
                    z::SAND - 9.65,
                )),
            ));
            x += length + rng.range(30.0, 110.0);
        }
    }
    furnish_the_margin(commands, board, art, shore);
}

/// How far out from the board the margin is furnished, in tiles: past
/// anything a window can show beside a zoomed-in board, and the scenery
/// beyond it would never be seen.
const MARGIN_TILES: f32 = 14.0;

/// A place in the margin round a board `size` wide and high, or `None` if
/// the draw landed where nothing may go: on the board, its frame or the
/// tide that widens past it, or in the sea above `shore`.
fn margin_spot(rng: &mut crate::app::effects::VisualRng, size: Vec2, shore: f32) -> Option<Vec2> {
    let clear = size / 2.0 + Vec2::splat(super::RIM + 14.0);
    let reach = size / 2.0 + Vec2::splat(MARGIN_TILES * TILE);
    let at = Vec2::new(
        rng.range(-reach.x, reach.x),
        rng.range(-reach.y, shore - 16.0),
    );
    (at.x.abs() > clear.x || at.y.abs() > clear.y).then_some(at)
}

/// The seed a board's margin is scattered from: its own seed and its size,
/// so a level looks the same every time it is played and two levels do
/// not share a beach.
fn margin_seed(board: &Board) -> u32 {
    (board.seed() as u32)
        ^ u32::from(board.width()).wrapping_mul(0x9E37_79B1)
        ^ u32::from(board.height()).wrapping_mul(0x85EB_CA77)
}

/// Scatter the beach around the board: sand grain, dune grass, driftwood,
/// pebbles and the odd starfish.
///
/// Only things that are not in the game: a rock, a pool or a hole out
/// here would read as a piece of the board that had wandered off. Kept
/// clear of the board, its frame and the tide that widens round it
/// (`RIM`), and of the sea. Drawn from the board's own size and seed, so a
/// level looks the same every time it is played, and a little dimmer than
/// the same things on the board.
fn furnish_the_margin(commands: &mut Commands, board: &Board, art: &Art, shore: f32) {
    let (w, h) = (
        f32::from(board.width()) * TILE,
        f32::from(board.height()) * TILE,
    );
    let mut rng = crate::app::effects::VisualRng::seeded(margin_seed(board));
    let spot = |rng: &mut crate::app::effects::VisualRng| margin_spot(rng, Vec2::new(w, h), shore);
    let z = z::SAND - 9.5;
    // Grain first, the most of it and the least of each.
    for _ in 0..900 {
        let Some(at) = spot(&mut rng) else { continue };
        let pale = rng.next().is_multiple_of(3);
        let tint = if pale {
            Color::srgba(1.0, 0.96, 0.86, 0.22)
        } else {
            Color::srgba(0.45, 0.37, 0.25, 0.20)
        };
        commands.spawn((
            BoardStatic,
            Sprite::from_color(tint, Vec2::splat(rng.range(2.0, 5.5))),
            Transform::from_translation(at.extend(z)),
        ));
    }
    for _ in 0..140 {
        let Some(at) = spot(&mut rng) else { continue };
        let (image, tint, size, turn) = match rng.next() % 10 {
            0..=3 => (
                &art.kelp,
                Color::srgba(0.62, 0.70, 0.45, 0.80),
                Vec2::splat(TILE * rng.range(0.38, 0.62)),
                rng.range(-0.25, 0.25),
            ),
            4..=5 => (
                &art.log,
                Color::srgba(0.92, 0.86, 0.78, 0.90),
                Vec2::splat(TILE * rng.range(0.55, 0.85)),
                rng.range(0.0, std::f32::consts::TAU),
            ),
            6..=8 => (
                &art.shadow,
                Color::srgba(0.42, 0.40, 0.37, 0.55),
                Vec2::new(TILE * 0.16, TILE * 0.11) * rng.range(0.7, 1.4),
                rng.range(0.0, std::f32::consts::TAU),
            ),
            _ => (
                &art.star,
                Color::srgba(0.93, 0.56, 0.42, 0.85),
                Vec2::splat(TILE * rng.range(0.20, 0.28)),
                rng.range(0.0, std::f32::consts::TAU),
            ),
        };
        commands.spawn((
            BoardStatic,
            image_sprite(&art.shadow, Color::srgba(1.0, 1.0, 1.0, 0.35), size * 0.9),
            Transform::from_translation((at + layout::SUN).extend(z + 0.01)),
        ));
        commands.spawn((
            BoardStatic,
            image_sprite(image, tint, size),
            Transform::from_translation(at.extend(z + 0.02))
                .with_rotation(Quat::from_rotation_z(turn)),
        ));
    }
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
        let idx = usize::from(board.index_of(sprite.x, sprite.y));
        match board.signpost_at(sprite.x, sprite.y) {
            Some(sp) if sp.dir == sprite.dir && sp.owner == sprite.owner => covered[idx] = true,
            // A different heading or a different owner on the same tile is
            // a different post, and gets its own plant.
            Some(_) | None => commands.entity(entity).despawn(),
        }
    }
    for (x, y, _) in board.tiles() {
        let idx = usize::from(board.index_of(x, y));
        if covered[idx] {
            continue;
        }
        let Some(sp) = board.signpost_at(x, y) else {
            continue;
        };
        let pos = layout::tile_center(board, x, y);
        // A driftwood board, cut to a point, with an arrow painted on it
        // in the owner's colour, over a shadow of itself so it stands in
        // the sand rather than being painted on it. The board is its own
        // colour and only the paint is tinted. The art and the colours are
        // written on the first frame by `dress_signposts`.
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
                image_sprite(&art.sign_board, Color::WHITE, Vec2::splat(SIGN_SIZE)),
                Transform::from_translation(pos.extend(z::SIGNPOST))
                    .with_rotation(layout::dir_rotation(sp.dir)),
            ))
            .with_children(|parent| {
                parent.spawn((
                    SignpostHalo,
                    image_sprite(&art.sign_shape, palette::GOLD, Vec2::splat(SIGN_SIZE)),
                    Transform::from_translation(Vec3::new(0.0, 0.0, -0.04))
                        .with_scale(Vec3::splat(HALO_SCALE)),
                    Visibility::Hidden,
                ));
                parent.spawn((
                    SignpostShadow,
                    image_sprite(&art.sign_shape, SIGN_SHADOW, Vec2::splat(SIGN_SIZE)),
                    // Down-and-right in the board's own frame would swing
                    // with its heading; the offset is applied in world
                    // space by `dress_signposts` for that reason.
                    Transform::from_translation(Vec3::new(0.0, 0.0, -0.05)),
                ));
                parent.spawn((
                    SignpostPaint,
                    image_sprite(
                        &art.sign_paint,
                        paint_color(sp.owner),
                        Vec2::splat(SIGN_SIZE),
                    ),
                    Transform::from_translation(Vec3::new(0.0, 0.0, 0.01)),
                ));
            });
    }
}

/// The shadow a signpost drops, at full strength.
const SIGN_SHADOW: Color = Color::srgba(0.14, 0.10, 0.06, 0.34);

/// The colour a seat paints its arrows: its own, a touch lifted so it
/// reads bright on the wood.
fn paint_color(owner: u8) -> Color {
    palette::player_color(owner).lighter(0.08)
}

/// A post's shadow, told apart from the post and its other children so all
/// of them can be written in the same pass.
type ShadowOnly = (
    With<SignpostShadow>,
    Without<SignpostSprite>,
    Without<SignpostPaint>,
    Without<SignpostHalo>,
);
/// A post's paint, likewise.
type PaintOnly = (
    With<SignpostPaint>,
    Without<SignpostSprite>,
    Without<SignpostShadow>,
    Without<SignpostHalo>,
);
/// A post's halo, likewise.
type HaloOnly = (
    With<SignpostHalo>,
    Without<SignpostSprite>,
    Without<SignpostShadow>,
    Without<SignpostPaint>,
);

/// The children a post is drawn with besides its board.
#[derive(bevy::ecs::system::SystemParam)]
pub struct PostParts<'w, 's> {
    shadows: Query<'w, 's, (&'static mut Sprite, &'static mut Transform), ShadowOnly>,
    paints: Query<'w, 's, &'static mut Sprite, PaintOnly>,
    halos: Query<'w, 's, (&'static mut Sprite, &'static mut Visibility), HaloOnly>,
}

/// Write what a post looks like *now*: how far it has settled after being
/// planted, how worn it is, how much life it has left, and whether it is
/// the one the next placement will take.
///
/// That last is marked only for seats played at this screen (see
/// [`crate::app::side_panels::played_here`]): it glows and rocks in the
/// sand, in time with its dot on the owner's score chip.
///
/// Wear keeps a post's ink and takes its edges instead of dimming it away
/// (see [`post_alpha`]): a worn board is split and chipped
/// ([`crate::app::art::Art::sign_board_worn`]) and its paint flaking.
pub fn dress_signposts(
    time: Res<Time>,
    sim: Res<Sim>,
    art: Res<Art>,
    settings: Res<crate::app::settings::GameSettings>,
    seating: crate::app::side_panels::Seating,
    mut posts: Query<(&mut SignpostSprite, &mut Sprite, &mut Transform, &Children)>,
    parts: PostParts,
) {
    use crate::app::side_panels::next_to_go_pulse;
    let PostParts {
        mut shadows,
        mut paints,
        mut halos,
    } = parts;
    let board = &sim.0;
    let dt = time.delta_secs();
    let next: [Option<(u8, u8)>; crate::sim::MAX_PLAYERS] = std::array::from_fn(|seat| {
        seating
            .played_here(seat as u8)
            .then(|| board.next_to_go(seat as u8))
            .flatten()
    });
    let secs = time.elapsed_secs();
    let pulse = next_to_go_pulse(secs, settings.reduced_motion);
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
        let (board_art, paint_art) = if worn {
            (&art.sign_board_worn, &art.sign_paint_worn)
        } else {
            (&art.sign_board, &art.sign_paint)
        };
        if sprite.image != *board_art {
            sprite.image = board_art.clone();
        }
        // Versus posts age out; a puzzle's stand until pulled. What is
        // left of a post's life dims it, but never past the point where it
        // stops reading against the sand.
        let fade = board.signpost_fade(&sp);
        let alpha = post_alpha(fade);
        if sprite.color != Color::WHITE {
            sprite.color = Color::WHITE;
        }
        let mut paint = paint_color(post.owner);
        if worn {
            paint = paint.darker(0.06);
        }
        let pop = if settings.reduced_motion {
            1.0
        } else {
            plant_pop(post.age)
        };
        transform.scale = Vec3::splat(pop * wither(fade));
        let marked = next.get(usize::from(post.owner)).copied().flatten() == Some((post.x, post.y));
        // Rocked about its heading, so the shadow below is worked out from
        // the rotation it is actually drawn at.
        let rock = match marked && !settings.reduced_motion {
            true => WOBBLE * (secs * std::f32::consts::TAU * 2.4).sin(),
            false => 0.0,
        };
        let rotation = layout::dir_rotation(post.dir) * Quat::from_rotation_z(rock);
        if transform.rotation != rotation {
            transform.rotation = rotation;
        }
        for child in children {
            if let Ok((mut halo, mut shown)) = halos.get_mut(*child) {
                let want = match marked {
                    true => Visibility::Inherited,
                    false => Visibility::Hidden,
                };
                if *shown != want {
                    *shown = want;
                }
                if marked {
                    halo.color = palette::GOLD.with_alpha(pulse);
                }
                continue;
            }
            if let Ok(mut coat) = paints.get_mut(*child) {
                if coat.image != *paint_art {
                    coat.image = paint_art.clone();
                }
                coat.color = paint.with_alpha(alpha);
                continue;
            }
            let Ok((mut shadow, mut shadow_tf)) = shadows.get_mut(*child) else {
                continue;
            };
            shadow.color = SIGN_SHADOW;
            // The sun is one direction for the whole beach, so the offset
            // is undone out of the arrow's rotation: a post pointing left
            // and one pointing up drop their shadow the same way. Divided
            // by the pop, since a child's translation is in the parent's
            // frame and the 1.35x overshoot would fling the shadow out.
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

    /// The margin is the same every time a level is played, and a
    /// different beach for a different level: same seed and size, same
    /// scatter; another size or seed, another.
    #[test]
    fn a_level_keeps_its_own_beach() {
        let seed = |w, h, s| margin_seed(&Board::new(w, h, s));
        assert_eq!(seed(12, 9, 7), seed(12, 9, 7));
        assert_ne!(seed(12, 9, 7), seed(12, 9, 8), "another seed");
        assert_ne!(seed(12, 9, 7), seed(9, 12, 7), "the same area turned round");
    }

    /// An aged post's paint never fades past the point its wood needs: the
    /// board stays solid, and the paint is what says whose post it is.
    #[test]
    fn a_posts_paint_never_fades_past_seventy_percent() {
        for step in 0..=10 {
            let alpha = post_alpha(step as f32 / 10.0);
            assert!(alpha >= 0.7 - 1e-6, "{alpha} at {step}");
        }
    }

    /// The scenery keeps off the play: nothing lands on the board, its
    /// frame, or the tide that widens round it by the end of a round, and
    /// nothing lands in the sea.
    #[test]
    fn the_margin_keeps_off_the_board_and_out_of_the_sea() {
        let size = Vec2::new(12.0, 9.0) * TILE;
        let shore = size.y / 2.0 + 26.0 + TILE * 0.55;
        let mut rng = crate::app::effects::VisualRng::seeded(7);
        let mut placed = 0;
        for _ in 0..5000 {
            let Some(at) = margin_spot(&mut rng, size, shore) else {
                continue;
            };
            placed += 1;
            let off_board = at.x.abs() > size.x / 2.0 + super::super::RIM
                || at.y.abs() > size.y / 2.0 + super::super::RIM;
            assert!(off_board, "{at} is on the board or its tide");
            assert!(at.y < shore, "{at} is in the sea");
        }
        assert!(placed > 1000, "only {placed} of 5000 found a place");
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

    /// The plant has to hand over to rest without a step in it.
    ///
    /// Past `PLANT_POP` the size is a flat 1.0, so the curve has to arrive
    /// there on its own, or every post jumps on the frame the animation
    /// stops. Asserting the value *at* the boundary reads the flat branch,
    /// so this reads the last moment of the curve itself.
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

    /// The alpha floor costs the fade most of its range, so the size
    /// carries the rest of the message: a post about to go is visibly
    /// smaller, and never small enough to be mistaken for somebody
    /// else's.
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
