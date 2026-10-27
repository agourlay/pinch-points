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
/// Not the browsing screens' card, which is [`CARD_FILL`]: that one hangs
/// over the bright, moving postcard and needs both the blue-black and the
/// 0.95 to sit still against it. One fill for each thing a card stands
/// on.
pub const CARD_BG: Color = Color::srgb(0.07, 0.08, 0.11);
/// The sand a crab icon stands on, off the board: the board's own, a
/// touch dimmed so the coins do not outshine the words beside them. The
/// crab's sprite is drawn for sand, and on a dark card its outline
/// vanishes.
pub const LEGEND_SAND: Color = Color::srgb(0.84, 0.77, 0.62);
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
/// The track of a list's scroll bar: [`GOLD`] fainter than the card's
/// hairline, so it reads as part of the frame and not as a second edge.
pub const SCROLL_TRACK: Color = Color::srgba(0.96, 0.83, 0.35, 0.10);
/// The thumb on that track: the same gold, firm enough to find at a
/// glance and still under the rows' text.
pub const SCROLL_THUMB: Color = Color::srgba(0.96, 0.83, 0.35, 0.55);
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
/// asks for it.
///
/// A bar rather than a marker character: with a dozen beaches on screen the
/// eye wants a block, and the number stays where it is instead of shuffling
/// sideways to make room for a caret.
pub const PICKED_WASH: Color = Color::srgba(0.96, 0.83, 0.35, 0.16);

// --- white-on-dark hairlines and washes -------------------------------------

/// Hairline edge on the sidebar cards.
pub const HAIRLINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);
/// The unlock-mark ring on a trophy not yet earned.
pub const UNLIT_RING: Color = Color::srgba(1.0, 1.0, 1.0, 0.22);
/// The unfilled track behind the gold progress bars (achievements screen,
/// stage list).
pub const BAR_TRACK: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);
/// How strongly a seat's name is inked on its chip, a touch under the
/// score beside it, which is the thing read across a room ([`name_ink_on`]).
pub const CHIP_NAME_ALPHA: f32 = 0.92;
/// The brightest text in the round chrome: the header's labels.
pub const HUD_INK: Color = Color::WHITE;
/// Text on a light fill: near black, warm rather than blue.
pub const DARK_INK: Color = Color::srgb(0.06, 0.05, 0.04);
/// The least contrast text keeps against the fill it is written on: WCAG's
/// AA for body text, which the seat names on the chips are.
pub const TEXT_CONTRAST: f32 = 4.5;

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

/// The colour a seat paints its posts: its own, a touch lifted so it reads
/// bright on the wood. The same paint marks everything that stands for the
/// seat's posts: its cursor brackets and its score chip's edge, so the eye
/// ties a post on the beach to the chip it scores for.
pub fn seat_paint(seat: u8) -> Color {
    player_color(seat).lighter(0.08)
}

/// A seat's colour as text on the dark cards (the results, the lobby's
/// table, the event feed): the colour itself where it already reaches
/// [`TEXT_CONTRAST`] on both dark fills ([`CARD_BG`] and [`CARD_FILL`]),
/// else lifted in lightness at its own hue just far enough that it does.
///
/// The classic blue and the safe palette's slate and blue are mid-tones
/// that read at under 3:1 on those cards as they are. Kept once per seat
/// and palette, as the chips are.
pub fn seat_text(seat: u8) -> Color {
    use std::sync::OnceLock;
    const SEATS: usize = crate::sim::MAX_PLAYERS;
    static KEPT: [[OnceLock<Color>; SEATS]; 2] = [const { [const { OnceLock::new() }; SEATS] }; 2];
    let colorblind = COLORBLIND.load(Ordering::Relaxed);
    let work = || {
        text_on_dark(match colorblind {
            true => safe_color(seat),
            false => classic_color(seat),
        })
    };
    match KEPT[usize::from(colorblind)].get(usize::from(seat)) {
        Some(kept) => *kept.get_or_init(work),
        None => work(),
    }
}

/// [`seat_text`] for a seat in `color`.
fn text_on_dark(color: Color) -> Color {
    let reads = |ink: Color| {
        [CARD_BG, CARD_FILL.with_alpha(1.0)]
            .iter()
            .all(|&fill| contrast(ink, fill) >= TEXT_CONTRAST)
    };
    let lch = Oklcha::from(color);
    let mut lightness = lch.lightness;
    let mut ink = srgb_of(lch);
    while !reads(ink) && lightness < 1.0 {
        lightness = (lightness + 0.005).min(1.0);
        ink = in_gamut(lch.with_lightness(lightness));
    }
    ink
}

/// A seat's score chip when it is not the leader: its own colour a
/// quarter darker, kept in hue, and as [`legible`] as a chip has to be.
///
/// Darkened by scaling the perceived lightness rather than subtracting a
/// fixed amount: `darker(0.22)` took the same step off every colour, which
/// left green and yellow mid-tones and pushed the red and blue seats, darker
/// to begin with, to almost black.
pub fn seat_chip(seat: u8) -> Color {
    chips(COLORBLIND.load(Ordering::Relaxed), seat).0
}

/// A seat's two chips, trailing and leading, in the colour-vision-safe
/// palette or the classic one.
///
/// Worked out once per seat and palette and kept: each is a search over
/// lightness and chroma, a few hundred colour conversions, and the side
/// panels ask for every seat's chip every frame (on a six-seat round that
/// search was the busiest thing the game's own code did).
fn chips(colorblind: bool, seat: u8) -> (Color, Color) {
    use std::sync::OnceLock;
    type Pair = OnceLock<(Color, Color)>;
    const SEATS: usize = crate::sim::MAX_PLAYERS;
    static KEPT: [[Pair; SEATS]; 2] = [const { [const { OnceLock::new() }; SEATS] }; 2];
    let work = || {
        let color = match colorblind {
            true => safe_color(seat),
            false => classic_color(seat),
        };
        (trailing_chip(color), legible(color))
    };
    match KEPT[usize::from(colorblind)].get(usize::from(seat)) {
        Some(kept) => *kept.get_or_init(work),
        None => work(),
    }
}

/// [`seat_chip`] for a seat in `color`.
fn trailing_chip(color: Color) -> Color {
    legible(chip_shade(color))
}

/// The leader's score chip, and anything else filled with a seat's own
/// colour and written on: the colour, [`legible`].
pub fn leader_chip(seat: u8) -> Color {
    chips(COLORBLIND.load(Ordering::Relaxed), seat).1
}

/// [`seat_chip`]'s shading of any seat colour: a quarter of its lightness
/// off at its own hue, in the screen's gamut.
fn chip_shade(color: Color) -> Color {
    let lch = Oklcha::from(color);
    in_gamut(lch.with_lightness(lch.lightness * CHIP_SHADE))
}

/// `lch` as a colour the screen can show: at its own lightness and hue,
/// with as much of its chroma as fits in sRGB.
///
/// Scaling a saturated yellow's lightness down at its chroma asks for a
/// colour no screen has (a negative blue), which the conversion then
/// clips channel by channel into a different hue and lightness.
fn in_gamut(lch: Oklcha) -> Color {
    let fits = |lch: Oklcha| {
        let rgb = Color::from(lch).to_srgba();
        [rgb.red, rgb.green, rgb.blue]
            .iter()
            .all(|c| (-1e-4..=1.0 + 1e-4).contains(c))
    };
    if fits(lch) {
        return srgb_of(lch);
    }
    let (mut inside, mut outside) = (0.0, lch.chroma);
    for _ in 0..24 {
        let mid = (inside + outside) / 2.0;
        match fits(lch.with_chroma(mid)) {
            true => inside = mid,
            false => outside = mid,
        }
    }
    srgb_of(lch.with_chroma(inside))
}

/// `lch` in sRGB, each channel held to the screen's range against the
/// last bit of rounding.
fn srgb_of(lch: Oklcha) -> Color {
    let rgb = Color::from(lch).to_srgba();
    Color::srgb(
        rgb.red.clamp(0.0, 1.0),
        rgb.green.clamp(0.0, 1.0),
        rgb.blue.clamp(0.0, 1.0),
    )
}

/// The ink text takes on `fill`: white, or [`DARK_INK`] on a fill light
/// enough that dark reads better. The one choice every chip, medal and
/// seat badge makes, for its name, its number and its digit.
pub fn ink_on(fill: Color) -> Color {
    match contrast(DARK_INK, fill) > contrast(Color::WHITE, fill) {
        true => DARK_INK,
        false => Color::WHITE,
    }
}

/// A seat's name on `fill`: [`ink_on`] it, a touch lighter.
pub fn name_ink_on(fill: Color) -> Color {
    ink_on(fill).with_alpha(CHIP_NAME_ALPHA)
}

/// `color` as a fill a seat's name can be read on: the colour itself where
/// its [`name_ink_on`] already reaches [`TEXT_CONTRAST`], else moved in
/// lightness, at its own hue, just far enough that one of the two inks
/// does: darker under white or lighter under [`DARK_INK`], whichever is
/// the smaller move, so the colour stays as near itself as it can.
///
/// A mid-tone red or blue reads under neither ink as it is: white on the
/// classic red reached 3.9:1, and near black about the same.
pub fn legible(color: Color) -> Color {
    let fill: Color = color.to_srgba().with_alpha(1.0).into();
    let reads = |fill: Color| contrast(name_ink_on(fill), fill) >= TEXT_CONTRAST;
    if reads(fill) {
        return fill;
    }
    let lch = Oklcha::from(fill);
    // The first lightness each way at which the ink that way reads.
    let moved = |toward: f32, ink: Color| {
        let mut lightness = lch.lightness;
        loop {
            lightness = (lightness + toward).clamp(0.0, 1.0);
            let nudged = in_gamut(lch.with_lightness(lightness));
            let done = contrast(ink.with_alpha(CHIP_NAME_ALPHA), nudged) >= TEXT_CONTRAST;
            if done || lightness <= 0.0 || lightness >= 1.0 {
                return (lightness, nudged);
            }
        }
    };
    let (darker, under_white) = moved(-0.005, Color::WHITE);
    let (lighter, under_dark) = moved(0.005, DARK_INK);
    match lch.lightness - darker <= lighter - lch.lightness {
        true => under_white,
        false => under_dark,
    }
}

/// The WCAG contrast ratio between text in `ink`, laid over `fill` at the
/// ink's alpha, and the opaque `fill`: from 1 (none) to 21 (black on white).
///
/// A translucent ink is blended with the fill under it, and whether that
/// happens on light (linear) values or on the screen's encoded ones
/// depends on the target drawn to; the two differ by a few hundredths
/// near the bar, so this answers the lower of them.
pub fn contrast(ink: Color, fill: Color) -> f32 {
    let luminance = |c: LinearRgba| 0.2126 * c.red + 0.7152 * c.green + 0.0722 * c.blue;
    let ratio = |x: f32, y: f32| (x.max(y) + 0.05) / (x.min(y) + 0.05);
    let a = ink.alpha();
    let under = luminance(fill.to_linear());
    let (over, base) = (ink.to_linear(), fill.to_linear());
    let on_light = LinearRgba::rgb(
        over.red * a + base.red * (1.0 - a),
        over.green * a + base.green * (1.0 - a),
        over.blue * a + base.blue * (1.0 - a),
    );
    let (over, base) = (ink.to_srgba(), fill.to_srgba());
    let on_screen = Srgba::rgb(
        over.red * a + base.red * (1.0 - a),
        over.green * a + base.green * (1.0 - a),
        over.blue * a + base.blue * (1.0 - a),
    );
    ratio(luminance(on_light), under).min(ratio(luminance(on_screen.into()), under))
}

/// How much of its lightness a trailing seat's chip keeps.
const CHIP_SHADE: f32 = 0.78;

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
/// on brightness instead. The six have the widest worst-case separation in
/// the Okabe-Ito set under the dichromat model the test uses (0.28, against
/// a bar of 0.2).
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

    /// Every seat's colour in both palettes, by the palettes' own functions
    /// rather than through [`set_colorblind`], a switch every test running
    /// beside these reads too.
    fn every_seat() -> Vec<(bool, u8, Color)> {
        [
            (false, classic_color as fn(u8) -> Color),
            (true, safe_color),
        ]
        .into_iter()
        .flat_map(|(colorblind, palette)| {
            (0..crate::sim::MAX_PLAYERS as u8).map(move |seat| (colorblind, seat, palette(seat)))
        })
        .collect()
    }

    /// The angle between two hues, in degrees, the short way round.
    fn hue_gap(a: f32, b: f32) -> f32 {
        let gap = (a - b).rem_euclid(360.0);
        gap.min(360.0 - gap)
    }

    /// A trailing chip is a darker shade of its own colour on every seat,
    /// in both palettes, read as the screen shows it, in sRGB: a colour
    /// the screen has, never near black, the same hue, and about a quarter
    /// of its lightness off, moved a little either way where its name
    /// needs it, and always clearly darker than the leader's.
    #[test]
    fn a_trailing_chip_keeps_its_colour() {
        for (_, seat, full) in every_seat() {
            {
                let rgb = trailing_chip(full).to_srgba();
                assert!(
                    [rgb.red, rgb.green, rgb.blue]
                        .iter()
                        .all(|c| (0.0..=1.0).contains(c)),
                    "seat {seat}: {rgb:?} is off the screen"
                );
                let (full, shown) = (Oklcha::from(full), Oklcha::from(Color::from(rgb)));
                assert!(shown.lightness > 0.35, "seat {seat}: {shown:?}");
                let ratio = shown.lightness / full.lightness;
                assert!(
                    (0.68..=0.86).contains(&ratio),
                    "seat {seat} keeps {ratio} of its lightness"
                );
                if full.chroma > 0.03 {
                    assert!(
                        hue_gap(shown.hue, full.hue) < 2.0,
                        "seat {seat} keeps its hue: {} for {}",
                        shown.hue,
                        full.hue
                    );
                }
            }
        }
    }

    /// Every chip, trailing or leading, in both palettes, holds its name
    /// and its score at [`TEXT_CONTRAST`], the name at its own alpha: the
    /// numbers are worked out from sRGB, as WCAG works them.
    #[test]
    fn every_chip_reads() {
        for (colorblind, seat, color) in every_seat() {
            {
                for fill in [trailing_chip(color), legible(color)] {
                    let name = contrast(name_ink_on(fill), fill);
                    let score = contrast(ink_on(fill), fill);
                    assert!(
                        name >= TEXT_CONTRAST && score >= name,
                        "colour-blind {colorblind}, seat {seat} on {:?}: name {name:.2}, score {score:.2}",
                        fill.to_srgba()
                    );
                }
            }
        }
    }

    /// Whatever colour is made [`legible`], a name reads on it: the step 3
    /// badges and medals fill with colours of their own making.
    #[test]
    fn any_colour_made_legible_reads() {
        for r in 0..=8 {
            for g in 0..=8 {
                for b in 0..=8 {
                    let color = Color::srgb(r as f32 / 8.0, g as f32 / 8.0, b as f32 / 8.0);
                    let fill = legible(color);
                    let seen = contrast(name_ink_on(fill), fill);
                    assert!(seen >= TEXT_CONTRAST, "{color:?}: {seen:.2}");
                }
            }
        }
    }

    /// The leader's chip is its seat's own colour wherever that reads, and
    /// moved only as far as it must where it does not.
    #[test]
    fn a_leader_chip_stays_its_colour() {
        for (_, seat, full) in every_seat() {
            {
                let (full, chip) = (Oklcha::from(full), Oklcha::from(legible(full)));
                assert!(
                    (chip.lightness - full.lightness).abs() < 0.1,
                    "seat {seat}: {full:?} became {chip:?}"
                );
            }
        }
    }

    /// The contrast measure itself, on the pairs WCAG publishes, and on a
    /// translucent ink, which is the fill showing through it.
    #[test]
    fn contrast_is_measured_as_wcag_measures_it() {
        assert!((contrast(Color::BLACK, Color::WHITE) - 21.0).abs() < 1e-3);
        assert!((contrast(Color::WHITE, Color::WHITE) - 1.0).abs() < 1e-6);
        let grey = Color::srgb(0.4627, 0.4627, 0.4627);
        assert!((contrast(Color::WHITE, grey) - 4.54).abs() < 0.02);
        let half = contrast(Color::WHITE.with_alpha(0.5), Color::BLACK);
        assert!(half > 1.0 && half < 21.0, "{half}");
        assert_eq!(ink_on(Color::srgb(0.95, 0.9, 0.25)), DARK_INK);
        assert_eq!(ink_on(Color::srgb(0.1, 0.2, 0.5)), Color::WHITE);
    }

    /// The kept chips are the ones the search finds, seat by seat, in
    /// both palettes, and a seat past the table is still answered.
    #[test]
    fn kept_chips_are_the_searched_ones() {
        for seat in 0..=crate::sim::MAX_PLAYERS as u8 {
            for (colorblind, color) in [(false, classic_color(seat)), (true, safe_color(seat))] {
                let want = (trailing_chip(color), legible(color));
                assert_eq!(chips(colorblind, seat), want, "seat {seat}");
                assert_eq!(chips(colorblind, seat), want, "seat {seat}, kept");
            }
        }
    }

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
    /// distinguishable without red/green discrimination. The default flags
    /// fail this, P1 red and P3 green being the classic trap.
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

    /// Every seat's text colour reads on both dark cards, in both
    /// palettes, and keeps its seat's hue: the lift is in lightness only.
    #[test]
    fn seat_text_reads_on_the_dark_cards() {
        for palette in [classic_color, safe_color] {
            for seat in 0..crate::sim::MAX_PLAYERS as u8 {
                let color = palette(seat);
                let ink = text_on_dark(color);
                for fill in [CARD_BG, CARD_FILL.with_alpha(1.0)] {
                    let ratio = contrast(ink, fill);
                    assert!(
                        ratio >= TEXT_CONTRAST,
                        "seat {seat}: {ratio:.2} on {fill:?}"
                    );
                }
                let (was, now) = (Oklcha::from(color), Oklcha::from(ink));
                assert!(now.lightness >= was.lightness - 1e-3, "seat {seat}");
                if was.chroma > 0.05 {
                    let turn = (now.hue - was.hue).abs();
                    assert!(turn.min(360.0 - turn) < 6.0, "seat {seat} turned {turn}");
                }
            }
        }
    }
}
