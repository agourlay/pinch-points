//! The world around the board: the beach and the sea behind it, the
//! scenery scattered over the margin, and the light over the sand.

use super::image_sprite;
use super::statics::BoardStatic;
use super::z;
use crate::app::Sim;
use crate::app::art::Art;
use crate::app::layout::{self, TILE};
use crate::app::materials::{WaterMaterial, WaterMode, WaterUniform};
use crate::sim::Board;
use bevy::prelude::*;

/// How far past the board a drifting cloud shadow turns round, in world
/// units: clear of any window the camera's clamped zoom can show. Not the
/// planes' [`REACH`], which is how far the world is *built*; a cloud
/// turning round that far out would be gone for a minute at a time.
const CLOUD_REACH: f32 = 2400.0;

/// How far past the board the beach and the sea are built, in world units.
///
/// Sized from the world rather than from the board: `boot::fit_camera`
/// fits a board to the tighter of the window's two sides and clamps the
/// zoom at `layout::MAX_ZOOM`, so a board leaves a great deal of beach
/// visible beside it, and a plane cut to the board's size ends in hard
/// edges with nothing beyond them.
const REACH: f32 = 9000.0;

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
/// edge so the beach never runs out of weather. They hold still with the
/// round, under a pause card and on the results.
pub fn drift_cloud_shadows(
    sim: Res<Sim>,
    time: Res<Time>,
    settings: Res<crate::app::settings::GameSettings>,
    round: crate::app::round_state::RoundState,
    mut clouds: Query<(&CloudShadow, &mut Transform)>,
) {
    if settings.reduced_motion || !round.moving(&sim.0) {
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

/// The world behind the board: dry sand to every edge of the window, drawn
/// by the sand shader, and the sea along the bottom of the view (see
/// [`lay_the_sea`]), so a round is played on the same beach the menu
/// promised instead of floating in a void. Sized from the board rather than
/// the window, because the camera zooms to fit the board and world units
/// are the only ones that hold still while it does. The sea alone follows
/// the view: sized from the board, it sat off the window on most boards.
pub(super) fn spawn_beach(commands: &mut Commands, board: &Board, art: &Art) {
    commands.spawn((
        BoardStatic,
        art.shaded.quad(
            &art.shaded.sand,
            Transform::from_translation(Vec3::new(0.0, 0.0, z::SAND - 10.0)),
            Vec2::splat(REACH),
        ),
    ));
    commands.spawn((
        BoardStatic,
        Sea,
        // Over the scenery, which is scattered without knowing where the
        // view's bottom edge will fall: the sea covers what lands in it.
        // Sized by `lay_the_sea`.
        art.shaded.quad(
            &art.shaded.sea,
            Transform::from_translation(Vec3::new(0.0, 0.0, SEA_Z)),
            Vec2::ONE,
        ),
        Visibility::Hidden,
    ));
    furnish_the_margin(commands, board, art);
}

/// The sea along the bottom of the view.
#[derive(Component)]
pub struct Sea;

/// Over the margin's scenery and under the board's own sand.
const SEA_Z: f32 = z::SAND - 9.0;

/// How far below the sand the sea's top keeps, world units: past the
/// wooden frame and its shadow, with a strip of beach to spare.
const FRAME_CLEAR: f32 = 22.0;

/// How much of the view's height the sea takes along the bottom at the
/// start of a round, in interface pixels: under the prompt line. Scaled
/// with the interface, as the prompt and the legend are. It climbs from
/// here as the round runs out (see [`super::tide`]).
const SEA_VIEW: f32 = 64.0;

/// How tall the band of wet sand the waves run up is, world units, and
/// over what depth of water the sea goes from shallow to deep.
const SEA_WET: f32 = 18.0;
const SEA_DEEPENS: f32 = 110.0;

const SEA_SHALLOW: Color = Color::srgba(0.47, 0.82, 0.80, 0.92);
const SEA_DEEP: Color = Color::srgba(0.12, 0.42, 0.60, 1.0);

/// The sea's look: its colours, its wet sand and how fast it deepens. Its
/// height the shader measures for itself, and its mania tint
/// [`lay_the_sea`] writes as manias come and go.
pub fn sea_uniform() -> WaterUniform {
    let mut water = WaterUniform::new(WaterMode::Sea, SEA_SHALLOW, SEA_DEEP);
    water.wet = SEA_WET;
    water.depth_span = SEA_DEEPENS;
    water
}

/// Where the sea's quad goes for a view `bottom` (world y of the window's
/// bottom edge), `scale` world units a logical pixel and `ui` logical
/// pixels an interface pixel, round a board whose sand ends at
/// `board_bottom`: its top edge and its height, or `None` when the view
/// leaves no room for it below the board.
///
/// `progress` is how far through its climb the tide is, 0 to 1, and
/// `swell` the closing stretch's heave in interface pixels. The climb is
/// [`super::tide::RISE`] where the window has room for it and whatever
/// room there is where it does not, less the swell's reach, so the sea
/// reaches the top of its climb as the round ends on any window with its
/// heave still whole. Where a small window (or the interface turned
/// down) leaves less room than the full rise, the sea starts lower, still
/// clear of the view's foot, so the clock moves at all. Nothing takes
/// the top past the board's frame: a short window loses sea, never board.
/// Whether there is room is asked without the swell, so a sea near the
/// limit does not blink in and out on every wave. The quad runs on past
/// the view's bottom so a shake never shows its edge.
fn sea_span(
    bottom: f32,
    scale: f32,
    ui: f32,
    board_bottom: f32,
    progress: f32,
    swell: f32,
) -> Option<(f32, f32)> {
    let px = ui * scale;
    let cap = board_bottom - FRAME_CLEAR;
    let rise = super::tide::RISE * px;
    let reach = super::tide::SWELL * px;
    let floor = bottom + SEA_WET + 2.0 * px;
    let start = (bottom + SEA_VIEW * px).min(cap - rise - reach).max(floor);
    let climb = rise.min((cap - reach - start).max(0.0));
    let resting = (start + climb * progress).min(cap);
    if resting <= bottom + SEA_WET {
        return None;
    }
    let top = (resting + swell * px).min(cap);
    Some((top, top - (bottom - 40.0 * scale)))
}

/// What [`lay_the_sea`] moves: where the sea's quad is, and whether there
/// is room for it at all.
type SeaQuad = (&'static mut Transform, &'static mut Visibility);

/// What the sea is laid against: the window, the camera resting on the
/// board, and the interface's scale.
#[derive(bevy::ecs::system::SystemParam)]
pub struct SeaView<'w, 's> {
    ui_scale: Res<'w, UiScale>,
    windows: Query<'w, 's, &'static Window>,
    cameras: Query<'w, 's, (&'static Projection, &'static Transform), With<Camera2d>>,
}

/// What the sea's climb reads: the round's state, the round's clock the
/// swell heaves on (so a paused or finished sea holds where it was), and
/// whether the player asked for things to hold still.
#[derive(bevy::ecs::system::SystemParam)]
pub struct SeaRound<'w> {
    settings: Res<'w, crate::app::settings::GameSettings>,
    round: crate::app::round_state::RoundState<'w>,
    clock: Res<'w, crate::app::round_state::RoundClock>,
}

/// Keep the sea along the bottom of the view, as wide as the window, as
/// the camera fits the board, climbing the beach and heaving as the round
/// runs out and coloured by a running mania. Between the fit and the
/// shake, so it sits where the camera rests and shakes with the rest of
/// the world.
pub fn lay_the_sea(
    sim: Res<Sim>,
    art: Res<Art>,
    view: SeaView,
    round: SeaRound,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut sea: Query<SeaQuad, (With<Sea>, Without<Camera2d>)>,
) {
    let Ok((mut transform, mut visibility)) = sea.single_mut() else {
        return;
    };
    let (Ok(window), Ok((Projection::Orthographic(ortho), camera))) =
        (view.windows.single(), view.cameras.single())
    else {
        return;
    };
    let (ui, scale) = (view.ui_scale.0, ortho.scale);
    let bottom = camera.translation.y - window.height() / 2.0 * scale;
    let board_bottom = -layout::board_extent(&sim.0).y / 2.0;
    let (progress, swell) = super::tide::climb(
        &sim.0,
        round.round.countdown(&sim.0),
        round.clock.0,
        round.settings.reduced_motion,
    );
    let Some((top, height)) = sea_span(bottom, scale, ui, board_bottom, progress, swell) else {
        if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden;
        }
        return;
    };
    if *visibility != Visibility::Inherited {
        *visibility = Visibility::Inherited;
    }
    let width = (window.width() + 200.0) * scale;
    let at = Vec3::new(camera.translation.x, top - height / 2.0, SEA_Z);
    let size = Vec3::new(width, height, 1.0);
    if transform.translation != at || transform.scale != size {
        transform.translation = at;
        transform.scale = size;
    }
    // The shader measures the quad's height itself, so the material is
    // only touched when a mania starts or ends: it is uploaded again
    // whenever it is.
    let tint = (!round.round.ended(&sim.0))
        .then(|| super::tide::mania_tint(&sim.0))
        .flatten()
        .map_or(LinearRgba::NONE, LinearRgba::from);
    let stale = materials
        .get(&art.shaded.sea)
        .is_some_and(|m| m.water.tint != tint);
    if stale && let Some(mut material) = materials.get_mut(&art.shaded.sea) {
        material.water.tint = tint;
    }
}

/// How far out from the board the margin is furnished, in tiles: past
/// anything a window can show beside a zoomed-in board, and the scenery
/// beyond it would never be seen.
const MARGIN_TILES: f32 = 14.0;

/// A place in the margin round a board `size` wide and high, or `None` if
/// the draw landed where nothing may go: on the board or inside the band
/// the camera keeps clear round it (`RIM`). The sea needs no keeping out
/// of: it is drawn over the scenery, wherever the view puts it.
fn margin_spot(rng: &mut crate::app::effects::VisualRng, size: Vec2) -> Option<Vec2> {
    let clear = size / 2.0 + Vec2::splat(super::RIM + 14.0);
    let reach = size / 2.0 + Vec2::splat(MARGIN_TILES * TILE);
    let at = Vec2::new(rng.range(-reach.x, reach.x), rng.range(-reach.y, reach.y));
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

/// Scatter the beach around the board: dune grass, driftwood, pebbles and
/// the odd starfish. The grain is the sand shader's.
///
/// Only things that are not in the game: a rock, a pool or a hole out
/// here would read as a piece of the board that had wandered off. Kept
/// clear of the board and the band the camera keeps clear round it, its
/// frame included (`RIM`). Drawn from the board's own size and seed, so a
/// level looks the same every time it is played, and a little dimmer than
/// the same things on the board.
fn furnish_the_margin(commands: &mut Commands, board: &Board, art: &Art) {
    let size = layout::board_extent(board);
    let mut rng = crate::app::effects::VisualRng::seeded(margin_seed(board));
    let spot = |rng: &mut crate::app::effects::VisualRng| margin_spot(rng, size);
    let z = z::SAND - 9.5;
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

    /// The sea takes the bottom of the view and never comes up onto the
    /// board or its frame: on a short window the sea shrinks or goes, the
    /// board does not.
    #[test]
    fn the_sea_keeps_to_the_bottom_of_the_view_and_off_the_board() {
        let board_bottom = -300.0;
        let clear = board_bottom - FRAME_CLEAR;
        // Plenty of room below the board: the sea is its usual height.
        let (top, height) = sea_span(-700.0, 1.0, 1.0, board_bottom, 0.0, 0.0).expect("room");
        assert_eq!(top, -700.0 + SEA_VIEW);
        assert!(height > SEA_VIEW, "runs on past the bottom: {height}");
        // A tight window: the top stops short of the frame.
        let (top, _) = sea_span(-380.0, 1.0, 1.0, board_bottom, 0.0, 0.0).expect("a little");
        assert!(top <= clear, "{top} reaches the frame at {clear}");
        // No room at all: no sea.
        assert_eq!(sea_span(-320.0, 1.0, 1.0, board_bottom, 0.0, 0.0), None);
        // Zoomed out, the same strip of window is more world.
        let (top, _) = sea_span(-2000.0, 2.0, 1.0, board_bottom, 0.0, 0.0).expect("room");
        assert_eq!(top, -2000.0 + SEA_VIEW * 2.0);
        // A bigger interface, a taller strip, under a taller prompt line.
        let (top, _) = sea_span(-2000.0, 1.0, 1.5, board_bottom, 0.0, 0.0).expect("room");
        assert_eq!(top, -2000.0 + SEA_VIEW * 1.5);
        // With room, the tide climbs its full rise by the end of the round.
        let rise = super::super::tide::RISE;
        let (top, _) = sea_span(-2000.0, 1.0, 1.0, board_bottom, 1.0, 0.0).expect("room");
        assert_eq!(top, -2000.0 + SEA_VIEW + rise);
        // And a swell never carries it onto the frame.
        let (top, _) = sea_span(-380.0, 1.0, 1.0, board_bottom, 1.0, 50.0).expect("room");
        assert!(top <= clear, "{top} swelled onto the frame at {clear}");
    }

    /// Short of room for the full rise, the tide climbs what room there is,
    /// all the way to the end of the round, and still leaves its swell room
    /// to rise as far as it falls: held to the full rise it hit the frame
    /// halfway through, and with no room left over the closing heave lost
    /// its crest against the frame.
    #[test]
    fn a_tight_window_still_tells_the_whole_round() {
        let board_bottom = -300.0;
        let cap = board_bottom - FRAME_CLEAR;
        let reach = super::super::tide::SWELL;
        let bottom = cap - SEA_VIEW - 20.0;
        let at = |progress, swell| {
            sea_span(bottom, 1.0, 1.0, board_bottom, progress, swell)
                .unwrap()
                .0
        };
        assert!(
            at(0.5, 0.0) < at(0.9, 0.0),
            "still climbing late in the round"
        );
        assert!(at(0.9, 0.0) < at(1.0, 0.0), "and right up to its end");
        assert!(
            (at(1.0, 0.0) - (cap - reach)).abs() < 1e-3,
            "short of the frame by a swell"
        );
        let (up, down) = (
            at(1.0, reach) - at(1.0, 0.0),
            at(1.0, 0.0) - at(1.0, -reach),
        );
        assert!(
            (up - down).abs() < 1e-3,
            "a whole swell at the end: {up} up, {down} down"
        );
    }

    /// Where the room under the board is less than the whole rise, the sea
    /// starts lower so the clock still moves, and it never starts so low it
    /// cannot be seen.
    #[test]
    fn a_crowded_window_still_moves_the_clock() {
        let board_bottom = -300.0;
        let cap = board_bottom - FRAME_CLEAR;
        // Room under the frame for the starting strip and nothing more.
        let bottom = cap - SEA_VIEW;
        let at = |progress| {
            sea_span(bottom, 1.0, 1.0, board_bottom, progress, 0.0)
                .unwrap()
                .0
        };
        assert!(
            at(1.0) - at(0.0) > 10.0,
            "it climbs: {} to {}",
            at(0.0),
            at(1.0)
        );
        assert!(
            at(0.0) > bottom + SEA_WET,
            "and starts where it can be seen"
        );
    }

    /// Whether the sea shows is asked of its resting height, not of where
    /// a wave has carried it, so a sea near the limit does not blink in and
    /// out on every swell.
    #[test]
    fn a_swell_never_hides_the_sea() {
        let board_bottom = -300.0;
        let cap = board_bottom - FRAME_CLEAR;
        let reach = super::super::tide::SWELL;
        let bottom = cap - SEA_WET - 3.0;
        let shown = sea_span(bottom, 1.0, 1.0, board_bottom, 0.0, 0.0).is_some();
        for swell in [-reach, -reach / 2.0, reach / 2.0, reach] {
            assert_eq!(
                sea_span(bottom, 1.0, 1.0, board_bottom, 0.0, swell).is_some(),
                shown,
                "a swell of {swell} changed whether the sea shows"
            );
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

    /// The scenery keeps off the play: nothing lands on the board or in
    /// the band round it, its frame included.
    #[test]
    fn the_margin_keeps_off_the_board() {
        let size = Vec2::new(12.0, 9.0) * TILE;
        let mut rng = crate::app::effects::VisualRng::seeded(7);
        let mut placed = 0;
        for _ in 0..5000 {
            let Some(at) = margin_spot(&mut rng, size) else {
                continue;
            };
            placed += 1;
            let off_board = at.x.abs() > size.x / 2.0 + super::super::RIM
                || at.y.abs() > size.y / 2.0 + super::super::RIM;
            assert!(off_board, "{at} is on the board or its rim");
        }
        assert!(placed > 1000, "only {placed} of 5000 found a place");
    }
}
