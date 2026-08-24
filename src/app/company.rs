//! The company a card keeps: a crab and a gull at its shoulders, and a
//! pale flock of both scattered behind it.
//!
//! A list of rows on a dark card is a form, and a form is a poor face for
//! a game about crabs and the birds that chase them. These say what the
//! game is about without a word - and they are the game's own sprites, at
//! the game's own colours, not decoration drawn for one screen.
//!
//! Lifted out of the language picker, which had them first, when the two
//! mode screens wanted the same treatment. What is shared is the critter
//! and how it moves; where they *stand* is not, and each screen hangs its
//! own flock. A card 279 pixels wide and one 712 wide leave very different
//! margins, and one scatter that suited both would suit neither: it would
//! either crowd the narrow card or huddle in the middle of the wide one.

use crate::app::art::Art;
use bevy::prelude::*;

/// Which creature keeps a card company: the two read differently on the
/// screen and are drawn from different art, so a perch says which it
/// holds rather than carrying a flag whose meaning is only in the name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Company {
    Crab,
    Gull,
}

/// One crab or gull keeping a card company.
#[derive(Component)]
pub struct Critter {
    /// The gulls flap; the crabs scuttle.
    who: Company,
    /// Radians per second of the bob, and how far it carries.
    rate: f32,
    travel: f32,
    /// Where in the bob this one starts, so a flock does not rise and fall
    /// as one body.
    phase: f32,
    /// Seconds per animation frame: the flap, and the scuttle.
    frame: f32,
}

/// How big the two at the card's shoulders are, and how much air they keep
/// between themselves and it: enough that neither crowds the rows, close
/// enough that they read as keeping the card company.
pub const CRITTER: f32 = 84.0;
pub const CRITTER_GAP: f32 = 40.0;

/// The width the pair adds to a row, both sides counted: what a screen has
/// to have spare beside its card before it can seat them.
///
/// Nothing at run time reads it - the row is laid out by flexbox from
/// [`CRITTER`] and [`CRITTER_GAP`] themselves - so it is a fact the fit
/// checks need and the game does not.
#[cfg(test)]
pub const SHOULDERS: f32 = 2.0 * (CRITTER + CRITTER_GAP);

/// How far the flock is faded back. Company, not a crowd: at full strength
/// it would compete with the rows that matter.
const FLOCK_ALPHA: f32 = 0.30;

/// One place in a flock: where on the frame it hangs as a fraction of each
/// axis, how big it is drawn, and who sits there.
#[derive(Clone, Copy, Debug)]
pub struct Perch {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub who: Company,
}

impl Perch {
    pub const fn gull(x: f32, y: f32, size: f32) -> Perch {
        Perch {
            x,
            y,
            size,
            who: Company::Gull,
        }
    }

    pub const fn crab(x: f32, y: f32, size: f32) -> Perch {
        Perch {
            x,
            y,
            size,
            who: Company::Crab,
        }
    }
}

/// The pair at the card's shoulders. Spawn the crab before the card and
/// the gull after it, so the row reads crab-card-gull and the card stays
/// centred on its own rows rather than on the crab.
pub fn shoulder(parent: &mut ChildSpawnerCommands, art: &Art, who: Company, phase: f32) {
    spawn_critter(parent, art, who, CRITTER, phase, None);
}

/// The pale scatter behind the card. Pinned rather than laid out, so it
/// sits behind without moving anything off centre, and spawned before the
/// card so it stays behind it.
pub fn flock(parent: &mut ChildSpawnerCommands, art: &Art, perches: &[Perch]) {
    for (index, perch) in perches.iter().enumerate() {
        spawn_critter(
            parent,
            art,
            perch.who,
            perch.size,
            index as f32 * 0.9,
            Some((perch.x, perch.y)),
        );
    }
}

/// One crab or gull. `size` is its side in pixels, `at` the fraction of the
/// frame it is pinned to (the flock) or `None` for the two that stand in
/// the row with the card.
///
/// The crabs keep a common crab's colour and the gulls the white they are
/// drawn in, so both read as the creatures they are on the board rather
/// than as shapes chosen to fill the margins.
fn spawn_critter(
    parent: &mut ChildSpawnerCommands,
    art: &Art,
    who: Company,
    size: f32,
    phase: f32,
    at: Option<(f32, f32)>,
) {
    let gull = who == Company::Gull;
    let (image, tint) = match who {
        Company::Gull => (art.gull.clone(), Color::WHITE),
        Company::Crab => (
            art.crab.clone(),
            crate::app::creatures::body_color(crate::sim::CrabKind::Common),
        ),
    };
    let node = match at {
        Some((x, y)) => Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(x * 100.0),
            top: Val::Percent(y * 100.0),
            width: Val::Px(size),
            height: Val::Px(size),
            ..default()
        },
        None => Node {
            width: Val::Px(size),
            height: Val::Px(size),
            flex_shrink: 0.0,
            ..default()
        },
    };
    parent.spawn((
        Critter {
            who,
            // The gulls ride a long slow hover; the crabs a shorter,
            // busier bob, which is what a crab looks like standing still.
            rate: if gull { 1.5 } else { 3.4 },
            travel: if gull { 7.0 } else { 3.0 },
            phase,
            frame: if gull { 0.30 } else { 0.16 },
        },
        ImageNode {
            image,
            color: tint.with_alpha(if at.is_some() { FLOCK_ALPHA } else { 1.0 }),
            // Every sprite is drawn facing right, so the ones standing to
            // the right of the card are turned round: the whole company
            // looks inward, at the rows, rather than half of it off the
            // edge of the screen.
            flip_x: at.map_or(gull, |(x, _)| x > 0.5),
            ..default()
        },
        node,
    ));
}

/// All of them, moving: the crabs scuttling on the spot, the gulls
/// hovering with a slow flap.
///
/// These are still screens - no board behind them, no attract round - and
/// a still picture on one looks like a game that has stopped responding.
pub fn animate_company(
    time: Res<Time>,
    art: Res<Art>,
    mut critters: Query<(&Critter, &mut Node, &mut ImageNode)>,
) {
    let now = time.elapsed_secs();
    for (critter, mut node, mut image) in &mut critters {
        // The flock is pinned by a percentage top, so the bob is added as
        // a margin: writing `top` would tear each of them off the place it
        // was hung.
        let bob = Val::Px((now * critter.rate + critter.phase).sin() * critter.travel);
        if node.margin.top != bob {
            node.margin.top = bob;
        }
        // Two frames each, alternating: the crab's two-step, and the gull
        // beating a wing to hold its place. Off its own phase, so seven
        // wings do not beat on the same tick.
        let beat = ((now + critter.phase) / critter.frame) as u32 % 2 == 1;
        let want = match ((critter.who == Company::Gull), beat) {
            (true, true) => &art.gull_fly,
            (true, false) => &art.gull,
            (false, true) => &art.crab_b,
            (false, false) => &art.crab,
        };
        if image.image != *want {
            image.image = want.clone();
        }
    }
}

/// The fraction of the frame's width a centred card of `card_w` pixels
/// covers once its shoulders are counted.
///
/// What a flock has to keep out of. Worked out from the card's own width
/// rather than written down per screen, because the width is the thing
/// that changes when a row is added or a column widened - and a hand-hung
/// flock that silently starts overlapping is exactly what this is for.
#[cfg(test)]
pub fn keep_clear(card_w: f32) -> std::ops::Range<f32> {
    let taken = ((card_w + SHOULDERS) / crate::app::settings::DESIGN_W).min(1.0);
    (0.5 - taken / 2.0)..(0.5 + taken / 2.0)
}

/// Every rule a hand-hung flock has to keep, checked for one screen.
///
/// A hand can hang a critter behind the card, where the card's near-solid
/// fill shows it through as a smudge under a row, or half off the edge of
/// the frame where it is drawn cut in two. Both are judged on the sprite's
/// own footprint rather than on the corner it hangs from: a critter is
/// pinned by its top-left and reaches down and to the right by its size,
/// so a perch that clears the card by the corner can still cover it.
///
/// `clear_x` is the band the card and its shoulders occupy; `clear_y` is
/// the card's own vertical extent, since a flock above and below it is the
/// whole point.
#[cfg(test)]
pub fn flock_is_hung_clear(perches: &[Perch], clear_x: std::ops::Range<f32>, clear_y: (f32, f32)) {
    let frame_h = crate::app::settings::DESIGN_H - 2.0 * crate::app::menu_ui::BAR_H;
    for &Perch { x, y, size, who } in perches {
        assert!(size > 0.0, "a critter with no size is not drawn");
        let (wide, tall) = (size / crate::app::settings::DESIGN_W, size / frame_h);
        assert!(
            x >= 0.0 && x + wide <= 1.0,
            "the critter at {x} runs off the frame"
        );
        assert!(
            y >= 0.0 && y + tall <= 1.0,
            "the critter at {y} runs off the frame"
        );
        assert_eq!(
            who == Company::Gull,
            y < 0.5,
            "a gull on the sand, or a crab in the air"
        );
        let over_x = x + wide > clear_x.start && x < clear_x.end;
        let over_y = y + tall > clear_y.0 && y < clear_y.1;
        assert!(
            !(over_x && over_y),
            "the {size}px critter at {x},{y} covers the card, which spans \
             {clear_x:?} across and {clear_y:?} down"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The band a flock has to avoid grows with the card, and a card wide
    /// enough leaves only the two margins. The language picker's card is
    /// narrow and the stage grid's more than twice as wide; one hand-hung
    /// scatter shared by both is what this exists to prevent.
    #[test]
    fn a_wider_card_leaves_a_narrower_margin() {
        let narrow = keep_clear(279.0);
        let wide = keep_clear(630.0);
        assert!(
            wide.start < narrow.start && narrow.end < wide.end,
            "the wide card ({wide:?}) has to swallow the narrow one ({narrow:?})"
        );
        // Centred, both of them: what is left on one side is left on the
        // other, or a flock would be told to bunch up on one edge.
        for band in [&narrow, &wide] {
            assert!(
                (band.start - (1.0 - band.end)).abs() < 0.001,
                "{band:?} is not centred"
            );
        }
        // And a card that fills the design width leaves nothing at all,
        // rather than a band running off the end of the frame.
        assert_eq!(keep_clear(crate::app::settings::DESIGN_W), 0.0..1.0);
    }

    /// The footprint rule itself, which is the whole value of the check: a
    /// sprite hangs from its top-left corner, so one whose corner clears
    /// the card can still be drawn across it.
    #[test]
    fn a_critter_is_judged_by_its_footprint_not_its_corner() {
        let band = keep_clear(279.0);
        // A big gull whose corner sits left of the card and whose body
        // does not. The corner alone would pass this.
        let reaching = [Perch::gull(band.start - 0.01, 0.30, 60.0)];
        assert!(
            std::panic::catch_unwind(|| flock_is_hung_clear(&reaching, band.clone(), (0.15, 0.80)))
                .is_err(),
            "a critter reaching onto the card was allowed"
        );
        // The same gull, hung far enough out that its whole body clears.
        let clear = [Perch::gull(band.start - 0.10, 0.30, 60.0)];
        flock_is_hung_clear(&clear, band, (0.15, 0.80));
    }
}
