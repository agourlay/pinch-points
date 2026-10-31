//! Boot-time scaffolding: the camera, the UI font, the zoom that keeps
//! whatever board is loaded inside the window's chrome, and the shove that
//! zoom takes when the beach is hit hard enough.

use crate::app::{Screen, Sim, layout};
use bevy::asset::uuid_handle;
use bevy::prelude::*;
use bevy::text::FontCx;
use parlance::Script;

/// The one camera, with the light graded over the world it draws.
///
/// HDR so the grade runs as its own pass over everything at once: without
/// it Bevy tonemaps inside each sprite's shader, and the water and sand
/// shaders, which do not, would be graded differently from the sprites on
/// them. `Linear` keeps every colour the art was drawn in and only lets
/// the grade through; a filmic curve dulled the sand and the player
/// colours. The interface is drawn after the grade and never touched by
/// it, so a colour in palette.rs is the colour on screen.
///
/// No bloom: nothing on the beach is brighter than white, so a threshold
/// low enough to catch the foam and the gulls let through a glow too faint
/// to change a pixel by one level, and its passes were the most expensive
/// thing in the frame on an integrated chip.
pub(super) fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        bevy::camera::Hdr,
        bevy::core_pipeline::tonemapping::Tonemapping::Linear,
        grade(),
        // Spawned with it rather than left to the engine, so the settings'
        // anti-aliasing switch (`apply_antialiasing`) always has one to set,
        // at the setting's default.
        Msaa::Off,
    ));
}

/// Smooth the beach's edges, or not, as the settings say: Bevy's 4x
/// multisampling, or none. Off by default: every board piece is a sprite
/// whose own texels fade out before its quad's edge, so multisampling only
/// ever touched a row of pixels along a plank, while every layer blended
/// over the beach was written four times over in a wide colour format.
///
/// The game's own camera only: a map card's preview camera draws with
/// none, whatever the setting, into a picture shown at half its size.
pub(super) fn apply_antialiasing(
    settings: Res<crate::app::settings::GameSettings>,
    mut cameras: Query<&mut Msaa, crate::app::match_setup::GameCamera>,
) {
    let want = if crate::app::dev::msaa_override().unwrap_or(settings.antialiasing) {
        Msaa::Sample4
    } else {
        Msaa::Off
    };
    for mut msaa in &mut cameras {
        msaa.set_if_neq(want);
    }
}

/// A warm afternoon: a little more colour, a touch of contrast in the
/// midtones, the white point nudged toward the sun.
pub(crate) fn grade() -> bevy::render::view::ColorGrading {
    use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};
    ColorGrading {
        global: ColorGradingGlobal {
            temperature: 0.01,
            post_saturation: 1.06,
            ..default()
        },
        midtones: ColorGradingSection {
            contrast: 1.04,
            ..default()
        },
        ..default()
    }
}

/// The slanted face of the UI font, for the few lines that are the game
/// speaking rather than a player: the lobby's arrivals and departures.
///
/// A fixed handle rather than a resource, because a `TextFont` is written
/// from systems all over the shell and none of them should have to carry a
/// resource for one italic line. There is no synthetic italic to fall back
/// on.
pub const ITALIC_FONT: Handle<Font> = uuid_handle!("9b451db2-8b49-4ad5-9794-ac1ce2ac943e");

/// The Japanese face, which DejaVu cannot stand in for: it has no kana
/// and no kanji at all. Held under its own handle because nothing asks
/// for it by hand - [`teach_the_kanji_fallback`] hands it to the text
/// stack, which reaches for it a run at a time.
pub const JP_FONT: Handle<Font> = uuid_handle!("6c2f0e33-3d4b-4d2e-9a58-6f2b2c9d4d71");

/// The display face: Nunito ExtraBold (SIL OFL, `assets/fonts/Nunito-LICENSE`),
/// for the text that is read from across a room rather than studied: the
/// wordmark, the header, the clocks, the scores, banners and headings.
///
/// DejaVu Sans Mono stays on everything else, and on purpose: the cards
/// lay their rows out in columns measured as a monospace sum
/// (`i18n::metrics`), and a proportional face there would make every
/// width guard a guess. The display face is the friendlier voice on top.
///
/// A static instance at weight 800 cut from Google's variable Nunito,
/// whose default instance is ExtraLight. Nunito declares no Reserved Font
/// Name, so the instance may keep the name. Japanese falls back to the
/// subset exactly as it does under DejaVu: the fallback is by script, not
/// by face.
pub const DISPLAY_FONT: Handle<Font> = uuid_handle!("6cf5591f-337a-4b2e-bb6a-697aece4ab2b");

/// The family name the subset carries in its `name` table, and the three
/// scripts it is the answer for: kana of both kinds, and the kanji.
const JP_FAMILY: &str = "Noto Sans Mono CJK JP";
const JP_SCRIPTS: [Script; 3] = [
    Script::from_bytes(*b"Hira"),
    Script::from_bytes(*b"Kana"),
    Script::from_bytes(*b"Hani"),
];

/// The family name DejaVu Sans Mono carries, and the other scripts the
/// tables are written in. A symbol takes the script of the words around
/// it, so these and the three above are every script a line here is in.
const UI_FAMILY: &str = "DejaVu Sans Mono";
const UI_SCRIPTS: [Script; 2] = [Script::from_bytes(*b"Latn"), Script::from_bytes(*b"Cyrl")];

/// Replace Bevy's built-in default font (ASCII-only) with an embedded font
/// that covers the glyphs seven of the eight languages need - the accented
/// Latin ones, as far out as Spanish's inverted marks and Dutch's
/// diaeresis, and the Cyrillic of the Russian table. Installing it under
/// the default handle localizes every `TextFont::default()` at once.
///
/// The eighth is Japanese, whose face goes in under [`JP_FONT`] and is
/// reached for by script rather than by asking for it: see
/// [`teach_the_kanji_fallback`].
///
/// The slanted face goes in beside them under [`ITALIC_FONT`]. All three
/// are `include_bytes!` rather than loaded through the asset server: the
/// font is wanted on the first frame text is drawn, and a load that is
/// still in flight draws that frame in Bevy's fallback face.
pub(super) fn install_ui_font(mut fonts: ResMut<Assets<Font>>) {
    let upright = include_bytes!("../../assets/fonts/DejaVuSansMono.ttf");
    if fonts
        .insert(
            &Handle::<Font>::default(),
            Font::from_bytes(upright.to_vec()),
        )
        .is_err()
    {
        warn!("UI font failed to install; accented glyphs will be missing");
    }
    let slanted = include_bytes!("../../assets/fonts/DejaVuSansMono-Oblique.ttf");
    if fonts
        .insert(&ITALIC_FONT, Font::from_bytes(slanted.to_vec()))
        .is_err()
    {
        warn!("italic UI font failed to install; notices will read upright");
    }
    let display = include_bytes!("../../assets/fonts/Nunito-ExtraBold.ttf");
    if fonts
        .insert(&DISPLAY_FONT, Font::from_bytes(display.to_vec()))
        .is_err()
    {
        warn!("display font failed to install; headings will read in the UI face");
    }
    let kanji = include_bytes!("../../assets/fonts/NotoSansMonoCJKjp-Subset.otf");
    if fonts
        .insert(&JP_FONT, Font::from_bytes(kanji.to_vec()))
        .is_err()
    {
        warn!("Japanese font failed to install; Japanese will draw blank");
    }
}

/// Tell the text stack which face to reach for when a line is in kana or
/// kanji.
///
/// Every `TextFont::default()` in the game asks for DejaVu, and a glyph it
/// has not got draws as nothing at all, Bevy loading no system fonts.
/// Registering the subset as the fallback for the three Japanese scripts is
/// what makes a Japanese line legible without every `Text` in the shell
/// knowing which language it is in, and a line that is half Japanese (most
/// prompts, with their WASD and their Enter) draws each half in the face
/// that has it.
///
/// DejaVu goes behind every script last, for the display face: Nunito
/// has no ✕, ○, □ or △, which a PlayStation pad's prompt names its face
/// buttons by, and DejaVu has all four. A character the face lacks is
/// looked for in the fallbacks of the script it is set in.
///
/// Runs until it lands rather than once: Bevy registers a font asset with
/// the collection in its own system, and the family is not there to be
/// named until it has.
pub(super) fn teach_the_kanji_fallback(
    mut fonts: ResMut<FontCx>,
    mut lines: Query<&mut TextFont>,
    mut taught: Local<bool>,
) {
    if *taught {
        return;
    }
    let (Some(family), Some(ui)) = (
        fonts.collection.family_id(JP_FAMILY),
        fonts.collection.family_id(UI_FAMILY),
    ) else {
        return;
    };
    for script in JP_SCRIPTS {
        fonts
            .collection
            .append_fallbacks(script, [family, ui].into_iter());
    }
    for script in UI_SCRIPTS {
        fonts
            .collection
            .append_fallbacks(script, std::iter::once(ui));
    }
    // Anything already on screen was shaped before the fallback existed
    // and holds a layout with holes in it - the header and the prompt are
    // spawned with the HUD, before the first frame. Touching every
    // `TextFont` is how Bevy's own font loader asks for a re-shape.
    for mut line in &mut lines {
        line.set_changed();
    }
    *taught = true;
}

/// Where the camera rests over a board: its zoom, in world units per
/// logical pixel, and how far the board's centre sits from the window's,
/// in logical pixels. One answer for the camera and for the interface that
/// lines up with the board's edges.
struct View {
    scale: f32,
    offset: Vec2,
}

/// The [`View`] a `screen` showing `board` rests at in `window`, with the
/// interface at `ui` times its design size.
fn resting_view(window: &Window, ui: f32, screen: Screen, board: &crate::sim::Board) -> View {
    // Every postcard screen, not only the menu: the postcard is sized to
    // the window in world units, and scaling the camera to whatever board
    // `Sim` last held left it short of the window's edges on the list
    // screens after a round, by a strip of clear colour at the bottom.
    let menu = crate::app::postcard_screen(screen);
    let chrome = screen.chrome();
    // The chrome is UI, so it takes whatever the global UI scale makes of
    // it; the board does not, and the unscaled width would slide a big grid
    // under the sidebars as soon as the interface grew. Read the applied
    // scale rather than the setting: the interface also shrinks to fit a
    // small window.
    let (chrome_w, chrome_top, chrome_bottom) =
        (chrome.width * ui, chrome.top * ui, chrome.bottom * ui);
    let chrome_h = chrome_top + chrome_bottom;
    let Vec2 {
        x: board_w,
        y: board_h,
    } = layout::board_extent(board);
    // The chrome (sidebars, header, prompt line) is fixed-size UI, so the
    // board must fit the window *minus* it; adding it to the board before
    // dividing would under-reserve and let a big grid slide under the
    // sidebars. Guard the subtraction so tiny windows zoom out rather
    // than divide by nothing.
    let fit_w = (window.width() - chrome_w).max(layout::TILE);
    let fit_h = (window.height() - chrome_h).max(layout::TILE);
    // The menu has no board: its decoration is laid out 1:1 with the
    // window.
    // The postcard is laid out in interface units (`scenery::postcard_size`)
    // and zoomed by the interface's scale, so its sand, sea and props grow
    // with the cards standing on them rather than shrinking beside them on
    // a big window.
    let scale = if menu {
        1.0 / ui.max(f32::EPSILON)
    } else {
        board_scale(
            Vec2::new(board_w, board_h),
            Vec2::new(fit_w, fit_h),
            window.scale_factor(),
        )
    };
    // Centre the board on the gap between the bars, not on the window. The
    // camera's y maps straight to screen y, so half the difference between
    // the two reserves lifts the board clear of the taller one.
    let offset = Vec2::new(
        0.0,
        if menu {
            0.0
        } else {
            (chrome_top - chrome_bottom) / 2.0
        },
    );
    View { scale, offset }
}

/// How far down the window the foot of the board's fence lands at rest, in
/// interface pixels (logical pixels over the interface's scale), so a card
/// can reach down to it whatever the board and the window. Read from the
/// camera's resting place rather than where it is: a shake would otherwise
/// shake the card's height too.
pub(super) fn board_foot(
    window: &Window,
    ui: f32,
    screen: Screen,
    board: &crate::sim::Board,
) -> f32 {
    let View { scale, offset } = resting_view(window, ui, screen, board);
    let below_centre =
        layout::board_extent(board).y / 2.0 + crate::app::board_render::FENCE_DEPTH / 2.0;
    (window.height() / 2.0 + offset.y + below_centre / scale.max(f32::EPSILON))
        / ui.max(f32::EPSILON)
}

/// Zoom out just enough that any board fits between the header bar and the
/// prompt line; standard boards stay at 1:1. On the menu the attract beach
/// instead frames small in the lower right, clear of the title banner and
/// the mode panel.
pub(super) fn fit_camera(
    sim: Res<Sim>,
    screen: Res<State<Screen>>,
    ui_scale: Res<UiScale>,
    windows: Query<&Window>,
    mut cameras: Query<(&mut Projection, &mut Transform), crate::app::match_setup::GameCamera>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let View { scale, offset } = resting_view(window, ui_scale.0, *screen.get(), &sim.0);
    for (mut projection, mut transform) in &mut cameras {
        if let Projection::Orthographic(ortho) = &mut *projection
            && (ortho.scale - scale).abs() > 0.001
        {
            ortho.scale = scale;
        }
        // The resting place, written whenever the camera is anywhere else.
        // Exact rather than within a deadband: `shake_camera` adds its
        // offset to what this leaves, and a deadband let every shake
        // smaller than it stay, so a run of small knocks walked the camera
        // off centre for good. Compared before writing, so a camera at
        // rest is not marked changed every frame.
        let target = Vec2::new(-offset.x, offset.y) * scale;
        if transform.translation.truncate() != target {
            transform.translation.x = target.x;
            transform.translation.y = target.y;
        }
    }
}

/// The camera scale that fits a board of `board` world units into `fit`
/// logical pixels, on a display that scales by `scale_factor`: world units
/// per logical pixel, so below 1 is zoomed in.
///
/// A board fills the room it has, up to the size its art was drawn at: a
/// tile of [`layout::SPRITE_PX`] physical pixels, which on a display the
/// system scales up is fewer logical ones. It used to stop at a quarter
/// over life size, 80 pixels a tile, which was the 96-pixel sprites' limit
/// and left a board on a 1080p screen filling under half of it.
///
/// A board smaller than the classic arena grows no further than the
/// classic arena would ([`layout::SMALLEST_FIT`]), with one floor under
/// that: it may always be drawn at [`layout::SMALL_BOARD_ZOOM`], the
/// quarter over life size small boards have always had. In a 720p window
/// the classic arena sits at 1:1, and without the floor the first puzzle
/// would have shrunk from 80 pixels a tile to 64.
///
/// The grid is what is fitted, but the board is drawn past it: the wooden
/// frame and the strip of beach round it, out to [`board_render::RIM`].
/// The interface's margins leave room for that band at 1:1, and zoomed in
/// it grows with everything else: at twice the size the edge of the board
/// ran under the prompt line. So the grid and its rim together get
/// the room they have at 1:1. That changes nothing for a board that is
/// zoomed out, next to nothing for one near 1:1, and holds the rim to its
/// 1:1 width however far in the board goes.
///
/// [`board_render::RIM`]: crate::app::board_render::RIM
fn board_scale(board: Vec2, fit: Vec2, scale_factor: f32) -> f32 {
    let fitted = |board: Vec2| {
        let rim = Vec2::splat(2.0 * crate::app::board_render::RIM);
        let grid = board / fit;
        let framed = (board + rim) / (fit + rim);
        grid.max_element()
            .max(framed.max_element())
            .max(scale_factor / layout::MAX_ZOOM)
    };
    // For a board at least the classic arena's size on both sides the
    // second term is never the larger, so only small boards are held back.
    let small = fitted(layout::SMALLEST_FIT).min(1.0 / layout::SMALL_BOARD_ZOOM);
    fitted(board).max(small)
}

/// Shake the camera by whatever trauma is in the pool.
///
/// After [`fit_camera`], and it has to be (the two are chained): the fit
/// puts the camera back at its exact resting place every frame it is
/// anywhere else, so the shake is an offset from there and nothing
/// accumulates, however small the shake.
///
/// The throw is in screen pixels, so it is multiplied by the projection's
/// scale: a zoomed-out board would otherwise be shaken by a fraction of
/// what a 1:1 one is.
pub(super) fn shake_camera(
    time: Res<Time>,
    round: crate::app::round_state::RoundState,
    settings: Res<crate::app::settings::GameSettings>,
    mut trauma: ResMut<crate::app::effects::Trauma>,
    mut cameras: Query<(&Projection, &mut Transform), crate::app::match_setup::GameCamera>,
) {
    // Under a pause card the camera rests and the shake waits, kept for
    // when the beach moves again.
    if round.held() {
        return;
    }
    let offset = trauma.offset(time.delta_secs(), time.elapsed_secs());
    // Drained either way, so nothing accumulates while the shake is off.
    if offset == Vec2::ZERO || settings.reduced_motion || crate::app::dev::screenshotting() {
        return;
    }
    for (projection, mut transform) in &mut cameras {
        let scale = match projection {
            Projection::Orthographic(ortho) => ortho.scale,
            Projection::Perspective(_) | Projection::Custom(_) => 1.0,
        };
        transform.translation.x += offset.x * scale;
        transform.translation.y += offset.y * scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::board_render::RIM;

    /// The anti-aliasing switch turns the game's camera, and leaves a map
    /// card's preview camera drawing without.
    #[test]
    fn antialiasing_leaves_the_preview_cameras_alone() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(crate::app::settings::GameSettings {
            antialiasing: true,
            ..crate::app::settings::GameSettings::default()
        });
        let game = world.spawn((Camera2d, Msaa::Off)).id();
        let preview = world
            .spawn((Camera2d, crate::app::match_setup::PreviewCamera, Msaa::Off))
            .id();
        world.run_system_once(apply_antialiasing).expect("runs");
        assert_eq!(world.get::<Msaa>(game), Some(&Msaa::Sample4));
        assert_eq!(world.get::<Msaa>(preview), Some(&Msaa::Off));
    }

    /// What the fit was before the zoom was lifted: the grid alone, and
    /// never closer than 0.8.
    fn old_scale(board: Vec2, fit: Vec2) -> f32 {
        (board / fit).max_element().max(0.8)
    }

    /// In a 720p window nothing moves: the XL beach is zoomed out and
    /// fits exactly as it did, and the classic one, which sits a hair
    /// inside 1:1, gives its rim back a fraction of a percent.
    #[test]
    fn a_720p_window_fits_as_it_did() {
        let fit = Vec2::new(1280.0 - 490.0, 720.0 - 137.0);
        let xl = Vec2::new(20.0, 13.0) * layout::TILE;
        assert!(old_scale(xl, fit) > 1.0, "the XL beach is zoomed out");
        assert_eq!(board_scale(xl, fit, 1.0), old_scale(xl, fit));

        let classic = Vec2::new(12.0, 9.0) * layout::TILE;
        let (new, old) = (board_scale(classic, fit, 1.0), old_scale(classic, fit));
        assert!((new / old - 1.0).abs() < 0.01, "{new} against {old}");
    }

    /// A shake is an offset from the resting place and never a step away
    /// from it: once the trauma has drained, the camera is exactly where it
    /// was before the first knock. With a half-pixel deadband in the fit,
    /// shakes smaller than that stayed, and a run of small knocks (a crab
    /// eaten is a tenth of a raid) left the board resting off centre.
    #[test]
    fn the_camera_comes_to_rest_where_it_started_after_a_shake() {
        use crate::app::effects::Trauma;
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.insert_state(Screen::Versus);
        app.init_resource::<crate::app::Paused>();
        app.init_resource::<crate::app::pause::PauseMenu>();
        app.init_resource::<crate::app::net::Online>();
        app.insert_resource(Sim(crate::sim::classic_arena(false, 2)));
        app.insert_resource(UiScale(1.0));
        app.insert_resource(crate::app::settings::GameSettings::default());
        app.init_resource::<Trauma>();
        app.init_resource::<Time>();
        app.world_mut().spawn(Window::default());
        let camera = app.world_mut().spawn(Camera2d).id();
        app.add_systems(Update, (fit_camera, shake_camera).chain());
        let rest = |app: &App| {
            app.world()
                .get::<Transform>(camera)
                .expect("the camera has a transform")
                .translation
        };
        let frame = |app: &mut App| {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_millis(16));
            app.update();
        };
        frame(&mut app);
        let home = rest(&app);
        // Knock after knock, each too small to move the camera half a
        // pixel, then long enough for the pool to drain.
        for _ in 0..40 {
            app.world_mut().resource_mut::<Trauma>().add(0.08);
            frame(&mut app);
        }
        for _ in 0..120 {
            frame(&mut app);
        }
        assert_eq!(rest(&app), home, "back exactly where it rested");
    }

    /// In a 720p window a small board is drawn a quarter over life size,
    /// as it always was, though the classic arena there is at 1:1.
    #[test]
    fn a_small_board_keeps_its_old_zoom_in_a_small_window() {
        let fit = Vec2::new(1280.0 - 490.0, 720.0 - 137.0);
        let first = Vec2::new(5.0, 3.0) * layout::TILE;
        assert_eq!(board_scale(first, fit, 1.0), old_scale(first, fit));
    }

    /// A board smaller than the classic arena is drawn at the classic
    /// arena's tile size, not blown up to fill the window: the five-by-
    /// three first puzzle and a narrow corridor both.
    #[test]
    fn a_small_board_grows_no_further_than_the_classic_one() {
        let fit = Vec2::new(1920.0 - 490.0, 1152.0 - 137.0);
        let classic = board_scale(Vec2::new(12.0, 9.0) * layout::TILE, fit, 1.0);
        for tiles in [
            Vec2::new(5.0, 3.0),
            Vec2::new(8.0, 6.0),
            Vec2::new(12.0, 5.0),
        ] {
            let scale = board_scale(tiles * layout::TILE, fit, 1.0);
            assert_eq!(scale, classic, "{tiles}");
        }
        // Wider than the classic arena on one side is fitted on that side.
        let long = board_scale(Vec2::new(20.0, 5.0) * layout::TILE, fit, 1.0);
        assert!(long > classic, "a 20-wide board zooms out further: {long}");
    }

    /// A tile never grows past the pixels its sprite was drawn with, on a
    /// plain display or a scaled one, however big the window.
    #[test]
    fn a_tile_stops_at_its_sprite() {
        let board = Vec2::new(12.0, 9.0) * layout::TILE;
        let fit = Vec2::new(6000.0, 4000.0);
        for scale_factor in [1.0, 1.5, 2.0] {
            let scale = board_scale(board, fit, scale_factor);
            let physical = layout::TILE / scale * scale_factor;
            assert!(
                (physical - layout::SPRITE_PX).abs() < 0.01,
                "{physical} px at {scale_factor}x"
            );
        }
    }

    /// Zoomed in, the frame and the beach round it take no more of the
    /// screen than they do at 1:1, so they stay out from under the
    /// interface.
    #[test]
    fn a_zoomed_board_keeps_its_rim_in_the_room_it_had() {
        let board = Vec2::new(12.0, 9.0) * layout::TILE;
        let fit = Vec2::new(1920.0 - 490.0, 1152.0 - 137.0);
        let scale = board_scale(board, fit, 1.0);
        assert!(scale < 0.8, "a 1080p-class window zooms past the old cap");
        let drawn = (board + Vec2::splat(2.0 * RIM)) / scale;
        let room = fit + Vec2::splat(2.0 * RIM);
        assert!(
            drawn.x <= room.x + 0.01 && drawn.y <= room.y + 0.01,
            "{drawn} in {room}"
        );
    }
}
