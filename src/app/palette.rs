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
/// The in-round chrome's fill: the sidebar cards, the event feed, the
/// announcement banner. Opaque, because what is behind it is the board.
///
/// Not the browsing screens' card - that is [`CARD_FILL`], which is a
/// different dark for a different reason: it hangs over the bright,
/// moving postcard and needs both the blue-black and the 0.95 to sit
/// still against it. Two fills, one for each thing a card can stand on.
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
/// The card's hairline border: [`GOLD`] at a hairline's strength (0.30
/// alpha), spelled out because `with_alpha` is not const.
pub const CARD_EDGE: Color = Color::srgba(0.96, 0.83, 0.35, 0.30);
/// The pill behind a prompt or legend line. Nearly opaque: at 0.72 it
/// blended into whatever stood behind it, and the same pill read dark on
/// one screen and sandy on the next.
pub const PILL_FILL: Color = Color::srgba(0.05, 0.07, 0.11, 0.92);
/// The tide foam at the foot of a card.
pub const FOAM_LINE: Color = Color::srgba(0.62, 0.82, 0.92, 0.16);
/// A card heading's little sprite: [`GOLD`], a shade off full strength.
/// Every heading in the game wears it, the lobby's included - which for a
/// while had a second constant of its own holding the same three numbers.
pub const HEADING_ICON: Color = Color::srgba(0.96, 0.83, 0.35, 0.85);

/// Round-event inks, shared by the side feed and the centre-screen
/// announcements so one event is one colour wherever it is reported.
pub const INK_TIDE: Color = Color::srgb(0.55, 0.8, 0.95);
pub const INK_LURE: Color = Color::srgb(0.75, 0.6, 0.95);
pub const INK_SURGE: Color = Color::srgb(0.5, 0.9, 0.7);
pub const INK_RAID: Color = Color::srgb(0.96, 0.45, 0.38);

// --- shared chrome ----------------------------------------------------------

/// The HUD header bar, on every screen but the menu (where it goes clear).
pub const HEADER_FILL: Color = Color::srgb(0.08, 0.09, 0.12);
/// The achievement toast's backing.
pub const TOAST_FILL: Color = Color::srgb(0.1, 0.12, 0.16);
/// The deep-water drop shadow under every card: `menu_ui::card_shadow`.
pub const CARD_SHADOW: Color = Color::srgba(0.0, 0.05, 0.12, 0.45);
/// The fill behind the row or box under the cursor, on every screen that
/// has one: [`GOLD`] at a wash's strength. `menu_ui::band` is how a screen
/// asks for it, and it lived under a lobby heading while four other
/// screens were quietly computing the same three numbers for themselves.
///
/// A bar rather than a marker character: with a dozen beaches on screen
/// the eye wants a block, and the number stays where it is instead of
/// shuffling sideways to make room for a caret.
pub const PICKED_WASH: Color = Color::srgba(0.96, 0.83, 0.35, 0.16);

// --- white-on-dark hairlines and washes -------------------------------------

/// Hairline edge on the sidebar cards.
pub const HAIRLINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);
/// The unlock-mark ring on a trophy not yet earned.
pub const UNLIT_RING: Color = Color::srgba(1.0, 1.0, 1.0, 0.22);
/// The unfilled track behind the gold progress bars (achievements screen,
/// stage list).
pub const BAR_TRACK: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);
/// A castle-tier pip on the score chips, lit and unlit.
pub const PIP_ON: Color = Color::srgba(1.0, 1.0, 1.0, 0.95);
pub const PIP_OFF: Color = Color::srgba(1.0, 1.0, 1.0, 0.25);
/// The seat name on a score chip.
pub const CHIP_NAME: Color = Color::srgba(1.0, 1.0, 1.0, 0.92);
/// The brightest text in the round chrome: the header's labels, and the
/// score on a chip.
///
/// Full white and meant to be, which is the whole reason it is written
/// down. A chip carries three whites - the name at 0.92, the tier pips at
/// 0.95, and the number, which is the thing being read across a room -
/// and the brightest of the three was the one nobody had named.
pub const HUD_INK: Color = Color::WHITE;

// --- the title sign ---------------------------------------------------------

/// The wordmark's ink and the shadow it drops on the sky.
pub const TITLE_INK: Color = Color::srgb(0.99, 0.85, 0.36);
pub const TITLE_SHADOW: Color = Color::srgba(0.05, 0.10, 0.20, 0.55);
/// The title sign hangs over the bright sky, where a cloud drifting behind
/// 12% of transparency shows through as a grey smudge across the letters,
/// so it gets a denser fill than the cards.
pub const SIGN_FILL: Color = Color::srgba(0.05, 0.09, 0.14, 0.96);

// --- the HUD clocks ---------------------------------------------------------

/// The header clock, calm and in its closing emergency (steady red, and the
/// brighter half of the final blink).
pub const CLOCK_CALM: Color = Color::srgb(0.95, 0.93, 0.84);
pub const CLOCK_RED: Color = Color::srgb(0.96, 0.25, 0.18);
pub const CLOCK_RED_BRIGHT: Color = Color::srgb(1.0, 0.55, 0.25);

// --- the stage grid ---------------------------------------------------------

/// Difficulty inks for the signpost counts without a round-event colour to
/// borrow: one post (green) and three (orange) on the stage grid and its key.
pub const INK_ONE_POST: Color = Color::srgb(0.52, 0.82, 0.55);
pub const INK_THREE_POSTS: Color = Color::srgb(0.96, 0.62, 0.30);
/// A cleared stage tile's number.
pub const TILE_CLEARED_INK: Color = Color::srgb(1.0, 0.96, 0.86);
/// An open stage tile's fill: parchment thinned to a wash.
pub const TILE_OPEN_FILL: Color = Color::srgba(0.95, 0.93, 0.84, 0.12);
/// A locked stage tile: sunk fill, faded number.
pub const TILE_LOCKED_FILL: Color = Color::srgba(0.06, 0.08, 0.11, 0.45);
pub const TILE_LOCKED_INK: Color = Color::srgba(0.95, 0.93, 0.84, 0.22);

// --- the sidebars -----------------------------------------------------------

/// Rank medal fills: gold, silver, bronze, then driftwood for the rest.
pub const MEDAL_GOLD: Color = Color::srgb(0.95, 0.78, 0.25);
pub const MEDAL_SILVER: Color = Color::srgb(0.76, 0.79, 0.83);
pub const MEDAL_BRONZE: Color = Color::srgb(0.75, 0.52, 0.33);
pub const MEDAL_DRIFTWOOD: Color = Color::srgb(0.42, 0.44, 0.5);
/// The rank digit on a medal disc: dark, the medals are bright.
pub const MEDAL_DIGIT: Color = Color::srgb(0.12, 0.11, 0.09);
/// The gull's line in the event feed.
pub const GULL_INK: Color = Color::srgba(0.85, 0.88, 0.92, 0.9);

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
