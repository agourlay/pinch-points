//! The tide clock: mm:ss counting down a round, calm until the closing
//! stretch, then red, then blinking. The big clock over a puzzle is
//! drawn here, and the versus sidebar's goes through the same helpers,
//! so the two always say the same thing.

use crate::app::palette::{CLOCK_CALM, CLOCK_RED, CLOCK_RED_BRIGHT};
use crate::app::settings::GameSettings;
use crate::app::{Phase, Screen, Sim, menu_ui};
use crate::sim::TICKS_PER_SECOND;
use bevy::prelude::*;

/// The big versus tide clock (top-centre).
#[derive(Component)]
pub struct TideClock;

/// mm:ss for a remaining-tick count, written into a buffer the caller
/// keeps: the clocks repaint every frame and change once a second.
pub(crate) fn clock_into(out: &mut String, ticks: u64) {
    use std::fmt::Write;
    let secs = ticks / u64::from(TICKS_PER_SECOND);
    out.clear();
    let _ = write!(out, "{}:{:02}", secs / 60, secs % 60);
}

/// mm:ss for a remaining-tick count.
#[cfg(test)]
pub(crate) fn clock_text(ticks: u64) -> String {
    let mut out = String::new();
    clock_into(&mut out, ticks);
    out
}

/// How much of a round the screen draws as its closing emergency.
///
/// `band` is what that stretch is worth on a round with a middle to it:
/// 30 s of red, the last 10 s of it blinking. A round shorter than the
/// band has no middle, and the fixed figure then covers the whole of it:
/// the longest round any level file asks for is 900 ticks, exactly
/// [`crate::sim::SURGE_TICKS`], so no shipped level ever left the red, and
/// Dry Feet ran all eight of its seconds under a blinking clock.
///
/// Below the band the last third stands in for it. Versus rounds are two
/// minutes and up and keep the fixed 30 s.
///
/// This is the *drawn* scramble only. [`crate::sim::Board::in_surge`] is
/// the sim's own 30 s rule, which doubles the gull spawn rate and gates
/// tide events, and it stays where it is: moving it would move every
/// replay.
pub(crate) fn urgency_band(round: Option<u32>, band: u32) -> u64 {
    match round {
        Some(round) if round <= band => u64::from(round) / 3,
        _ => u64::from(band),
    }
}

/// The clock colour ramp: calm, red inside the closing band, blinking
/// half-seconds for the last third of that. `blink` is off under reduced
/// motion, where the final stretch simply holds the bright red instead of
/// flashing. `round` is the board's round length, absent on an untimed
/// puzzle counting down the campaign tick limit.
pub(crate) fn clock_color(ticks: u64, round: Option<u32>, elapsed: f32, blink: bool) -> Color {
    const BLINK_TICKS: u64 = 10 * TICKS_PER_SECOND as u64;
    let red = urgency_band(round, crate::sim::SURGE_TICKS);
    // The blink is the last third of the red, and never more than the ten
    // seconds it is worth on a long round: the two have to stay in that
    // order, or a short round starts blinking the instant it turns red.
    if ticks <= BLINK_TICKS.min(red / 3) {
        if !blink || ((elapsed * 2.0) as u32).is_multiple_of(2) {
            CLOCK_RED
        } else {
            CLOCK_RED_BRIGHT
        }
    } else if ticks <= red {
        CLOCK_RED
    } else {
        CLOCK_CALM
    }
}

/// Drive the big clock: mm:ss during a versus round, red inside the last
/// 30 seconds, pulsing for the final 10. Colour-only pulsing: it never
/// re-shapes the text.
pub fn update_tide_clock(
    sim: Res<Sim>,
    screen: Res<State<Screen>>,
    phase: Res<State<Phase>>,
    time: Res<Time>,
    settings: Res<GameSettings>,
    mut clocks: Query<(&mut Text, &mut TextColor), With<TideClock>>,
    mut line: Local<String>,
) {
    let remaining = match screen.get() {
        // Versus shows its clock in the right sidebar instead.
        Screen::Versus => None,
        // Every running puzzle counts down: its own round if it has one,
        // otherwise the campaign-wide tick limit that would otherwise fail
        // the level invisibly.
        Screen::Puzzle if *phase.get() == Phase::Running => sim.0.remaining_ticks().or(Some(
            crate::sim::PUZZLE_TICK_LIMIT.saturating_sub(sim.0.ticks()),
        )),
        // A timed level shows its deadline while the signposts are still
        // going down: Dry Feet is decided in eight seconds, with the player
        // choosing where to spend their one post. Only levels that carry a
        // round, since the campaign tick limit is a backstop rather than a
        // deadline.
        Screen::Puzzle if *phase.get() == Phase::Setup => sim.0.remaining_ticks(),
        Screen::Puzzle
        | Screen::Menu
        | Screen::Editor
        | Screen::Lobby
        | Screen::Settings
        | Screen::Controls
        | Screen::MatchSetup
        | Screen::Achievements
        | Screen::StageSelect
        | Screen::Replays
        | Screen::Interlude
        | Screen::Language
        | Screen::NewVersion => None,
    };
    for (mut text, mut color) in &mut clocks {
        let Some(ticks) = remaining else {
            if !text.0.is_empty() {
                text.0.clear();
            }
            continue;
        };
        clock_into(&mut line, ticks);
        menu_ui::set_text(&mut text, &line);
        let target = clock_color(
            ticks,
            sim.0.round_length(),
            time.elapsed_secs(),
            !settings.reduced_motion,
        );
        menu_ui::set_color(&mut color, target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_formats_minutes_and_seconds() {
        let tps = u64::from(TICKS_PER_SECOND);
        assert_eq!(clock_text(0), "0:00");
        assert_eq!(clock_text(29 * tps), "0:29");
        assert_eq!(clock_text(90 * tps), "1:30");
        assert_eq!(clock_text(600 * tps), "10:00");
    }

    /// The clock reddens inside the last 30 seconds and blinks for the last
    /// 10, unless the player asked for less motion, where it holds. The
    /// flat figures are for a long round (versus, or the untimed puzzle
    /// backstop) and are unchanged on one.
    #[test]
    fn the_clock_reddens_then_blinks() {
        let tps = u64::from(TICKS_PER_SECOND);
        let long = Some(3 * 60 * TICKS_PER_SECOND);
        for round in [None, long] {
            assert_eq!(clock_color(60 * tps, round, 0.0, true), CLOCK_CALM);
            assert_eq!(clock_color(20 * tps, round, 0.0, true), CLOCK_RED);
            // Inside ten seconds the colour alternates with the wall clock.
            assert_eq!(clock_color(5 * tps, round, 0.0, true), CLOCK_RED);
            assert_eq!(clock_color(5 * tps, round, 0.5, true), CLOCK_RED_BRIGHT);
            // Reduced motion: red, but steady.
            assert_eq!(clock_color(5 * tps, round, 0.5, false), CLOCK_RED);
        }
    }

    /// A round shorter than the flat band still has a calm stretch.
    ///
    /// Every timed level in the game is shorter than [`SURGE_TICKS`] (the
    /// longest `round:` any file asks for is 900, that figure exactly), so
    /// under a flat rule not one of them shows a calm clock: Dry Feet, at
    /// 240 ticks, is red and blinking from its first frame to its last.
    #[test]
    fn a_short_round_is_not_one_long_emergency() {
        let dry_feet = Some(240);
        // Two thirds of the way in, still calm; the last third reddens and
        // the very end of that blinks.
        assert_eq!(clock_color(200, dry_feet, 0.0, true), CLOCK_CALM);
        assert_eq!(clock_color(100, dry_feet, 0.0, true), CLOCK_CALM);
        assert_eq!(clock_color(60, dry_feet, 0.0, true), CLOCK_RED);
        assert_eq!(clock_color(20, dry_feet, 0.5, true), CLOCK_RED_BRIGHT);
        // The red always arrives before the blink, on any round length.
        for round in [240u32, 300, 900, 1800, 3600] {
            let red = urgency_band(Some(round), crate::sim::SURGE_TICKS);
            let blink = (0..=round)
                .rev()
                .map(u64::from)
                .find(|&t| clock_color(t, Some(round), 0.5, true) == CLOCK_RED_BRIGHT);
            assert!(
                blink.is_some_and(|blink| blink < red),
                "round {round}: reddens at {red}, blinks at {blink:?}"
            );
        }
    }

    /// The flat band is what a round long enough to have a middle gets;
    /// a shorter one gets its last third instead.
    #[test]
    fn the_urgency_band_shrinks_only_for_short_rounds() {
        assert_eq!(urgency_band(None, 900), 900);
        assert_eq!(urgency_band(Some(3600), 900), 900);
        assert_eq!(urgency_band(Some(901), 900), 900);
        // At and below the band, the last third of the round.
        assert_eq!(urgency_band(Some(900), 900), 300);
        assert_eq!(urgency_band(Some(240), 900), 80);
    }
}
