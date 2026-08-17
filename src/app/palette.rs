//! The game's colours: the per-seat identity palette (with its
//! colour-vision-safe alternative) and the shared UI inks.
//!
//! Split out of `layout`, which is grid arithmetic and has no business
//! knowing what red is.

use bevy::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};

/// Unselected menu-row text: a muted sand-yellow that reads as "idle"
/// against the highlighted row.
pub const IDLE_ROW: Color = Color::srgb(0.66, 0.54, 0.17);
/// The highlighted row in every list menu.
pub const SELECTED_ROW: Color = Color::srgb(0.95, 0.9, 0.6);
/// Dark card/panel background used by every overlay.
pub const CARD_BG: Color = Color::srgb(0.07, 0.08, 0.11);
/// Trophy gold: titles, crowns, champions, the daily best.
pub const GOLD: Color = Color::srgb(0.96, 0.83, 0.35);
/// Warm off-white body text on dark cards.
pub const PARCHMENT: Color = Color::srgb(0.92, 0.89, 0.78);
/// The landing-menu card: a deep-sea fill the menu and the stage list
/// share, so the two cards age together.
///
/// Nearly opaque, and it has to be. The menu card hangs over a bright sea
/// with clouds and crabs moving behind it, and at 0.88 the sea came
/// through as a shifting wash that pale text sat on badly.
pub const CARD_FILL: Color = Color::srgba(0.04, 0.07, 0.12, 0.95);
/// Its hairline border: [`GOLD`] at a hairline's strength (0.30 alpha),
/// spelled out because `with_alpha` is not const.
pub const CARD_EDGE: Color = Color::srgba(0.96, 0.83, 0.35, 0.30);

/// Round-event inks, shared by the side feed and the centre-screen
/// announcements so one event is one colour wherever it is reported.
pub const INK_TIDE: Color = Color::srgb(0.55, 0.8, 0.95);
pub const INK_LURE: Color = Color::srgb(0.75, 0.6, 0.95);
pub const INK_SURGE: Color = Color::srgb(0.5, 0.9, 0.7);
pub const INK_RAID: Color = Color::srgb(0.96, 0.45, 0.38);

/// Which player palette is in force. Colours are read from ~20 places
/// across the shell (castles, cursors, chips, confetti, medals) none of
/// which have any other reason to know about settings, so the choice lives
/// here as one flag that [`set_colorblind`] pushes on change. Render-only:
/// the sim never sees a colour.
static COLORBLIND: AtomicBool = AtomicBool::new(false);

/// Point [`player_color`] at the colour-vision-safe palette (or back).
pub fn set_colorblind(on: bool) {
    COLORBLIND.store(on, Ordering::Relaxed);
}

pub fn player_color(player: u8) -> Color {
    if COLORBLIND.load(Ordering::Relaxed) {
        safe_color(player)
    } else {
        classic_color(player)
    }
}

/// The default flag colours. `pub(crate)` so the highlight reel, which is
/// engine-free and therefore carries its own copy of these as raw bytes, can
/// be tested against them.
pub(crate) fn classic_color(player: u8) -> Color {
    match player {
        0 => Color::srgb(0.84, 0.27, 0.27), // red
        1 => Color::srgb(0.25, 0.47, 0.79), // blue
        2 => Color::srgb(0.30, 0.69, 0.31), // green
        3 => Color::srgb(0.89, 0.73, 0.23), // yellow
        4 => Color::srgb(0.70, 0.40, 0.82), // violet
        _ => Color::srgb(0.95, 0.55, 0.20), // orange
    }
}

/// The colour-vision-safe palette, from the Okabe-Ito set. No green at all:
/// the red/green pair is the classic trap, and it is exactly the P1/P3
/// pairing of the default flags. These stay apart for protanopia and
/// deuteranopia both by hue and by brightness.
///
/// Six is where hue alone runs out, so the sixth is a slate that separates
/// on brightness instead. The six were picked by searching the Okabe-Ito set
/// for the combination with the widest worst-case separation under the
/// dichromat model the test uses (0.28, against a bar of 0.2).
fn safe_color(player: u8) -> Color {
    match player {
        0 => Color::srgb(0.80, 0.40, 0.00), // vermillion
        1 => Color::srgb(0.35, 0.70, 0.90), // sky blue
        2 => Color::srgb(0.95, 0.90, 0.25), // yellow
        3 => Color::srgb(0.80, 0.60, 0.70), // reddish purple
        4 => Color::srgb(0.00, 0.45, 0.70), // blue
        _ => Color::srgb(0.35, 0.35, 0.38), // slate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_colors_are_distinct() {
        for palette in [classic_color, safe_color] {
            let colors: Vec<_> = (0..crate::sim::MAX_PLAYERS as u8).map(palette).collect();
            for i in 0..colors.len() {
                for j in (i + 1)..colors.len() {
                    assert_ne!(colors[i], colors[j], "seats {i} and {j}");
                }
            }
        }
    }

    /// How a red-blind (protanope) or green-blind (deuteranope) eye reads a
    /// colour: the red and green cones collapse into one brightness signal,
    /// leaving brightness and a blue-yellow axis to tell colours apart.
    fn dichromat_view(color: Color) -> (f32, f32) {
        let c = color.to_srgba();
        let brightness = 0.35 * c.red + 0.55 * c.green + 0.10 * c.blue;
        let blue_yellow = c.blue - (c.red + c.green) / 2.0;
        (brightness, blue_yellow)
    }

    /// The point of the accessible palette: every pair of seats stays
    /// telling-apart-able without red/green discrimination. The default
    /// flags fail this, P1 red and P3 green being the classic trap, which
    /// is why the option exists.
    #[test]
    fn the_safe_palette_survives_color_blind_vision() {
        let separation = |palette: fn(u8) -> Color| {
            let mut worst = f32::INFINITY;
            for a in 0..crate::sim::MAX_PLAYERS as u8 {
                for b in (a + 1)..crate::sim::MAX_PLAYERS as u8 {
                    let (ba, ya) = dichromat_view(palette(a));
                    let (bb, yb) = dichromat_view(palette(b));
                    worst = worst.min((ba - bb).abs().max((ya - yb).abs()));
                }
            }
            worst
        };
        let safe = separation(safe_color);
        assert!(safe > 0.2, "closest safe pair is only {safe} apart");
        assert!(
            separation(classic_color) < safe,
            "the accessible palette must beat the default it replaces"
        );
    }
}
