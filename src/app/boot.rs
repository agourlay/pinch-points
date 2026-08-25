//! Boot-time scaffolding: the camera, the UI font, and the zoom that keeps
//! whatever board is loaded inside the window's chrome.

use crate::app::{Screen, Sim, layout};
use bevy::asset::uuid_handle;
use bevy::prelude::*;
use bevy::text::FontCx;
use parlance::Script;

pub(super) fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// The slanted face of the UI font, for the few lines that are the game
/// speaking rather than a player: the lobby's arrivals and departures.
///
/// A fixed handle rather than a resource because a `TextFont` is written
/// from systems all over the shell, and asking each of them to carry a
/// resource for one italic line is a lot of plumbing for one font. Bevy
/// resolves a handle to the exact face, so asking for this one is asking
/// for the slant. There is no synthetic italic to fall back on.
pub const ITALIC_FONT: Handle<Font> = uuid_handle!("9b451db2-8b49-4ad5-9794-ac1ce2ac943e");

/// The Japanese face, which DejaVu cannot stand in for: it has no kana
/// and no kanji at all. Held under its own handle because nothing asks
/// for it by hand - [`teach_the_kanji_fallback`] hands it to the text
/// stack, which reaches for it a run at a time.
pub const JP_FONT: Handle<Font> = uuid_handle!("6c2f0e33-3d4b-4d2e-9a58-6f2b2c9d4d71");

/// The family name the subset carries in its `name` table, and the three
/// scripts it is the answer for: kana of both kinds, and the kanji.
const JP_FAMILY: &str = "Noto Sans Mono CJK JP";
const JP_SCRIPTS: [&str; 3] = ["Hira", "Kana", "Hani"];

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
/// has not got draws as nothing whatever - Bevy loads no system fonts, so
/// there is no font of last resort behind it. Registering the subset as
/// the fallback for the three Japanese scripts is what makes a Japanese
/// line legible without every `Text` in the shell having to know which
/// language it is in. It also means a line that is half Japanese and half
/// not - which most of the prompts are, with their WASD and their Enter -
/// draws each half in the face that has it.
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
    let Some(family) = fonts.collection.family_id(JP_FAMILY) else {
        return;
    };
    for script in JP_SCRIPTS {
        fonts
            .collection
            .append_fallbacks(Script::from_str_unchecked(script), std::iter::once(family));
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

/// Zoom out just enough that any board fits between the header bar and the
/// prompt line; standard boards stay at 1:1. On the menu the attract beach
/// instead frames small in the lower right, clear of the title banner and
/// the mode panel.
pub(super) fn fit_camera(
    sim: Res<Sim>,
    screen: Res<State<Screen>>,
    ui_scale: Res<UiScale>,
    windows: Query<&Window>,
    mut cameras: Query<(&mut Projection, &mut Transform), With<Camera2d>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let menu = matches!(screen.get(), Screen::Menu);
    let chrome = screen.get().chrome();
    // The chrome is UI, so it takes whatever the global UI scale makes of
    // it; the board does not. Reserving the unscaled width here would slide
    // a big grid under the sidebars as soon as the interface grew. Read the
    // applied scale rather than the setting: the interface also shrinks to
    // fit a small window, and the reserve has to shrink with it.
    let ui = ui_scale.0;
    let (chrome_w, chrome_top, chrome_bottom) =
        (chrome.width * ui, chrome.top * ui, chrome.bottom * ui);
    let chrome_h = chrome_top + chrome_bottom;
    let board_w = f32::from(sim.0.width()) * layout::TILE;
    let board_h = f32::from(sim.0.height()) * layout::TILE;
    // The chrome (sidebars, header, prompt line) is fixed-size UI, so the
    // board must fit the window *minus* it; adding it to the board before
    // dividing would under-reserve and let a big grid slide under the
    // sidebars. Guard the subtraction so tiny windows zoom out rather
    // than divide by nothing.
    let fit_w = (window.width() - chrome_w).max(layout::TILE);
    let fit_h = (window.height() - chrome_h).max(layout::TILE);
    // The menu has no board: its decoration is laid out 1:1 with the window.
    // Small boards zoom in a little rather than floating in their margins:
    // a five-row puzzle at 1:1 fills a third of the window and reads as
    // lost. 0.8 is a quarter over life size, gentle enough that the flat
    // sprite art stays crisp.
    let scale = if menu {
        1.0
    } else {
        (board_w / fit_w).max(board_h / fit_h).max(0.8)
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
    for (mut projection, mut transform) in &mut cameras {
        if let Projection::Orthographic(ortho) = &mut *projection
            && (ortho.scale - scale).abs() > 0.001
        {
            ortho.scale = scale;
        }
        let target = Vec2::new(-offset.x, offset.y) * scale;
        if transform.translation.truncate().distance(target) > 0.5 {
            transform.translation.x = target.x;
            transform.translation.y = target.y;
        }
    }
}
