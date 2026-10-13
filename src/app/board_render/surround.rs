//! The world around the board: the dusk shore and sea behind it, the
//! scenery scattered over the margin, and the light over the sand.

use super::image_sprite;
use super::statics::BoardStatic;
use super::z;
use crate::app::art::Art;
use crate::app::layout::{self, TILE};
use crate::sim::Board;
use bevy::prelude::*;

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
pub(super) fn spawn_weather(commands: &mut Commands, board: &Board, art: &Art) {
    let Vec2 { x: w, y: h } = layout::board_extent(board);
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

/// The dry sand the board is staked out on: a shade darker and duller than
/// the board's own sand, so the play stays the brightest thing on screen.
/// It was a dim olive, which read as nowhere at all rather than as beach.
const SURROUND_SAND: Color = Color::srgb(0.70, 0.61, 0.45);

/// The world behind the board: dusk sand to every edge of the window and
/// the sea lying along the top, so a round is played on the same beach the
/// menu promised instead of floating in a grey void. Sized from the board
/// rather than the window, because the camera zooms to fit the board and
/// world units are the only ones that hold still while it does.
pub(super) fn spawn_dusk_shore(commands: &mut Commands, board: &Board, art: &Art) {
    // Far larger than any zoomed-out view can reach: the wave's reach.
    const REACH: f32 = super::wash::REACH;
    let top = layout::board_extent(board).y / 2.0 + 26.0;
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
        image_sprite(
            &art.foam,
            Color::srgba(1.0, 1.0, 1.0, 0.45),
            Vec2::new(REACH, 12.0),
        ),
        Transform::from_translation(Vec3::new(0.0, shore + 2.0, z::SAND - 9.6)),
    ));
    // Crests further out, fainter, thinner and more broken toward the
    // horizon, so the strip of blue is a sea coming in rather than a band
    // of colour. Short lengths with gaps between: one crest the width of
    // the world was a ruled line.
    let mut rng = crate::app::effects::VisualRng::seeded(board.seed() as u32 ^ 0x5EA);
    let span = layout::board_extent(board).x / 2.0 + MARGIN_TILES * TILE;
    for (lift, alpha, depth) in [(30.0, 0.34, 9.0), (62.0, 0.24, 7.0), (104.0, 0.15, 5.0)] {
        let mut x = -span;
        while x < span {
            let length = rng.range(70.0, 190.0);
            commands.spawn((
                BoardStatic,
                image_sprite(
                    &art.foam,
                    Color::srgba(1.0, 1.0, 1.0, alpha),
                    Vec2::new(length, depth),
                ),
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
    let size = layout::board_extent(board);
    let mut rng = crate::app::effects::VisualRng::seeded(margin_seed(board));
    let spot = |rng: &mut crate::app::effects::VisualRng| margin_spot(rng, size, shore);
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
