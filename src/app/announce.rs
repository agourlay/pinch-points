//! Centre-screen announcements for the moments that change the round.
//!
//! The side feed reports everything, which makes it good for reading back
//! and poor for noticing: a lure or a tide event lands in a 14px line in the
//! corner while four players are watching the middle of the board. The
//! original put its roulette result across the centre of the screen, and
//! that is what this does: big, brief, and colour-matched to the feed line
//! that records it.
//!
//! Render-only, and deliberately so: it never touches the sim or the clock,
//! so a peer that is a frame ahead still simulates the same round. Two
//! events landing together queue rather than overlap.

use crate::app::i18n::fill;
use crate::app::menu_ui;
use crate::app::palette;
use crate::app::settings::GameSettings;
use crate::app::sim_events::SimEvent;
use crate::sim::{CrabKind, PlayerId, TideEvent};
use bevy::prelude::*;
use std::collections::VecDeque;

/// How long one announcement holds the middle of the screen at full
/// strength, and the ramps at each end.
///
/// Two seconds of hold rather than the one and a half it began with. The
/// banner carries a headline *and* a line explaining what the event does,
/// and a second and a half is enough to read the first and not the second.
/// It could be spent now that the roulette has a cooldown on it
/// ([`crate::sim::EVENT_COOLDOWN`]): events used to arrive in waves that
/// kept the queue below saturated, and lengthening the banner then would
/// have made the pile-up worse rather than the reading better.
const HOLD: f32 = 2.0;
const FADE_IN: f32 = 0.15;
const FADE_OUT: f32 = 0.45;
/// The whole span, ramps included. Derived rather than written down, so a
/// change to either fade moves the total and leaves the *hold* - the part
/// that is actually read - exactly as long as it says it is.
const LIFE: f32 = FADE_IN + HOLD + FADE_OUT;

/// Never let a pile-up of events keep the middle of the screen busy for
/// the better part of ten seconds; the feed still has all of them.
///
/// The arithmetic is one banner more than it looks: `drive_announcements`
/// pops the queue *before* spawning, so one is on screen while this many
/// still wait behind it. The ceiling is therefore `(MAX_QUEUED + 1) *
/// LIFE`, and they queue rather than overlap, so that is genuinely how
/// long the middle of the board can stay covered. At three, and with the
/// hold lengthened to two seconds, that came to 10.4 s - past the line
/// this constant exists to hold. Two keeps it at 7.8 s.
const MAX_QUEUED: usize = 2;

/// Something worth the centre of the screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Announcement {
    Tide(TideEvent),
    /// A molting crab banked: every loose crab now runs for this seat.
    Lure(PlayerId),
    /// The final scramble.
    Surge,
}

impl Announcement {
    /// Headline, the line under it, and the ink they share.
    ///
    /// Pure, so what the player reads at the loudest moment of the round is
    /// testable without a window. Takes the settings rather than the string
    /// table because a lure names a seat, and a seat may have been given a
    /// name of its own.
    pub fn lines(
        self,
        settings: &GameSettings,
        names: &crate::app::SeatNames,
    ) -> (String, String, Color) {
        let tr = settings.tr();
        match self {
            Announcement::Tide(event) => (
                tr.events[event.index()].to_string(),
                tr.event_blurbs[event.index()].to_string(),
                palette::INK_TIDE,
            ),
            Announcement::Lure(owner) => (
                tr.ann_lure.to_string(),
                fill(tr.ann_lure_sub, &[("p", &names.label(tr, owner))]),
                palette::INK_LURE,
            ),
            Announcement::Surge => (
                tr.ann_surge.to_string(),
                tr.ann_surge_sub.to_string(),
                palette::INK_SURGE,
            ),
        }
    }
}

/// The announcement being shown and the ones waiting their turn.
#[derive(Resource, Default)]
pub struct Announcer {
    queue: VecDeque<Announcement>,
    /// Seconds the current banner has been up, once one is spawned.
    age: f32,
}

impl Announcer {
    /// Queue an announcement, unless the same one is already waiting (a
    /// second gull surge cannot happen, but a repeated tide event can).
    pub fn push(&mut self, announcement: Announcement) {
        if self.queue.contains(&announcement) || self.queue.len() >= MAX_QUEUED {
            return;
        }
        self.queue.push_back(announcement);
    }

    #[cfg(test)]
    pub fn pending(&self) -> usize {
        self.queue.len()
    }
}

/// Alpha over one announcement's life: a quick ramp in, a hold, a slower
/// fade out. Colour-only, so it never re-shapes the text under it.
fn envelope(age: f32) -> f32 {
    if age < FADE_IN {
        (age / FADE_IN).clamp(0.0, 1.0)
    } else if age > LIFE - FADE_OUT {
        ((LIFE - age) / FADE_OUT).clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// The whole banner, so it can be despawned in one call.
#[derive(Component)]
pub struct Banner;

/// A piece of the banner and the colours it fades from: text and border take
/// `ink`, the card's panel takes `fill`.
#[derive(Component)]
pub struct BannerPart {
    ink: Color,
    fill: Option<Color>,
}

/// Fold the loud sim events into the queue. Everything else stays in the
/// feed: an announcement that fires every few seconds is wallpaper.
pub fn collect_announcements(
    mut events: MessageReader<SimEvent>,
    mut announcer: ResMut<Announcer>,
) {
    for event in events.read() {
        match event {
            SimEvent::TideEventFired { event } => announcer.push(Announcement::Tide(*event)),
            SimEvent::CrabBanked {
                owner,
                kind: CrabKind::Molting,
                ..
            } => announcer.push(Announcement::Lure(*owner)),
            SimEvent::SurgeStarted => announcer.push(Announcement::Surge),
            SimEvent::CrabBanked { .. }
            | SimEvent::CrabEaten { .. }
            | SimEvent::CrabSpawned { .. }
            | SimEvent::CastleRaided { .. }
            | SimEvent::GullArrived
            | SimEvent::GullTookOff
            | SimEvent::GullLanded { .. }
            | SimEvent::SignpostPlaced { .. }
            | SimEvent::SignpostRemoved { .. }
            | SimEvent::SignpostEvicted { .. }
            | SimEvent::TierUp { .. }
            | SimEvent::RoundEnded => {}
        }
    }
}

/// The three colours a banner piece can wear: its text, its edge, its panel.
/// A piece has whichever of them it was spawned with.
type BannerColors<'w> = (
    &'w BannerPart,
    Option<&'w mut TextColor>,
    Option<&'w mut BorderColor>,
    Option<&'w mut BackgroundColor>,
);

/// Show the queue one at a time: age the banner that is up, fade it, retire
/// it, then raise the next one.
pub fn drive_announcements(
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<GameSettings>,
    names: Res<crate::app::SeatNames>,
    mut announcer: ResMut<Announcer>,
    banners: Query<Entity, With<Banner>>,
    mut parts: Query<BannerColors>,
) {
    if let Ok(banner) = banners.single() {
        announcer.age += time.delta_secs();
        if announcer.age >= LIFE {
            commands.entity(banner).despawn();
            announcer.age = 0.0;
            return;
        }
        let alpha = envelope(announcer.age);
        for (part, text, border, fill) in &mut parts {
            let ink = part.ink.with_alpha(part.ink.alpha() * alpha);
            if let Some(mut color) = text {
                crate::app::menu_ui::set_color(&mut color, ink);
            }
            if let Some(mut color) = border
                && color.top != ink
            {
                *color = BorderColor::all(ink);
            }
            if let (Some(mut color), Some(base)) = (fill, part.fill) {
                crate::app::menu_ui::set_bg(&mut color, base.with_alpha(base.alpha() * alpha));
            }
        }
        return;
    }
    let Some(next) = announcer.queue.pop_front() else {
        return;
    };
    announcer.age = 0.0;
    spawn_banner(&mut commands, next, &settings, &names);
}

fn spawn_banner(
    commands: &mut Commands,
    announcement: Announcement,
    settings: &GameSettings,
    names: &crate::app::SeatNames,
) {
    let (headline, blurb, ink) = announcement.lines(settings, names);
    // Opaque, like the pause card: a translucent panel over a busy board
    // showed the crabs through the letters, which is the opposite of the
    // point. It only holds the middle for two seconds.
    let card_fill = palette::CARD_BG;
    commands
        .spawn((Banner, GlobalZIndex(25), menu_ui::centred_overlay()))
        .with_children(|wrap| {
            wrap.spawn((
                BannerPart {
                    ink: ink.with_alpha(0.9),
                    fill: Some(card_fill),
                },
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(4.0),
                    padding: UiRect::axes(Val::Px(40.0), Val::Px(22.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(18.0)),
                    ..default()
                },
                BackgroundColor(card_fill.with_alpha(0.0)),
                BorderColor::all(ink.with_alpha(0.0)),
            ))
            .with_children(|card| {
                card.spawn((
                    BannerPart { ink, fill: None },
                    Text::new(headline),
                    TextFont {
                        font_size: FontSize::Px(46.0),
                        ..default()
                    },
                    TextColor(ink.with_alpha(0.0)),
                ));
                card.spawn((
                    BannerPart {
                        ink: palette::PARCHMENT,
                        fill: None,
                    },
                    Text::new(blurb),
                    TextFont {
                        font_size: FontSize::Px(menu_ui::type_scale::ROW),
                        ..default()
                    },
                    TextColor(palette::PARCHMENT.with_alpha(0.0)),
                ));
            });
        });
}

/// Leaving the round takes the banner and anything queued behind it.
pub fn clear_announcements(
    mut commands: Commands,
    mut announcer: ResMut<Announcer>,
    banners: Query<Entity, With<Banner>>,
) {
    for banner in &banners {
        commands.entity(banner).despawn();
    }
    announcer.queue.clear();
    announcer.age = 0.0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::i18n::{ALL_LANGS, EN};

    /// The envelope ramps in, holds, and reaches zero at the end. A banner
    /// that never quite disappears would sit on the board forever.
    #[test]
    fn the_envelope_opens_and_closes() {
        assert_eq!(envelope(0.0), 0.0);
        assert_eq!(envelope(FADE_IN), 1.0);
        assert_eq!(
            envelope(FADE_IN + HOLD),
            1.0,
            "full strength for the whole of the hold, to its last instant"
        );
        assert_eq!(envelope(LIFE / 2.0), 1.0, "holds through the middle");
        assert_eq!(envelope(LIFE), 0.0);
        assert!(envelope(LIFE - FADE_OUT / 2.0) < 1.0, "on the way out");
        assert_eq!(envelope(LIFE + 5.0), 0.0, "never negative");
    }

    /// The queue serializes announcements and drops a duplicate of one
    /// already waiting, so a double event cannot double the banner.
    #[test]
    fn the_queue_serializes_and_dedupes() {
        let mut announcer = Announcer::default();
        announcer.push(Announcement::Surge);
        announcer.push(Announcement::Surge);
        assert_eq!(announcer.pending(), 1);
        announcer.push(Announcement::Tide(TideEvent::CrabMania));
        announcer.push(Announcement::Lure(2));
        // Counted off the cap rather than written as a number, so moving
        // the cap moves the test with it instead of pinning the old value.
        assert_eq!(announcer.pending(), MAX_QUEUED, "everything up to the cap");
        announcer.push(Announcement::Tide(TideEvent::FreshSand));
        assert_eq!(announcer.pending(), MAX_QUEUED, "the pile-up is capped");
    }

    /// The wiring, end to end in a headless App: a tide event raises a
    /// banner that says what happened, one at a time, and it retires itself.
    #[test]
    fn an_event_raises_a_banner_that_retires_itself() {
        use crate::app::sim_events::SimEvent;
        use std::time::Duration;

        let mut app = App::new();
        app.init_resource::<Time>();
        app.add_message::<SimEvent>();
        app.init_resource::<Announcer>();
        app.insert_resource(GameSettings::default());
        app.init_resource::<crate::app::SeatNames>();
        app.add_systems(Update, (collect_announcements, drive_announcements).chain());

        app.world_mut().write_message(SimEvent::TideEventFired {
            event: TideEvent::CastleSwap,
        });
        app.world_mut().write_message(SimEvent::SurgeStarted);
        app.update();

        let headlines = |app: &mut App| {
            app.world_mut()
                .query::<&Text>()
                .iter(app.world())
                .map(|text| text.0.clone())
                .collect::<Vec<_>>()
        };
        let shown = headlines(&mut app);
        assert!(
            shown.iter().any(|line| line == EN.events[7]),
            "the first event took the screen: {shown:?}"
        );
        assert!(
            !shown.iter().any(|line| line == EN.ann_surge),
            "the second one waits its turn: {shown:?}"
        );
        assert_eq!(app.world().resource::<Announcer>().pending(), 1);

        // Run past its life: it retires, then the queued one takes over on
        // the frame after (the despawn is a command, and the next banner
        // waits for it to land).
        //
        // Counted off `LIFE` rather than written as a number of frames.
        // Four 800 ms steps was exactly enough at the 2.1 s this banner
        // used to live, and lengthening the hold turned it into a test
        // that watched the first banner and never saw the second.
        let step = Duration::from_millis(200);
        let frames = (LIFE / step.as_secs_f32()).ceil() as u32 + 2;
        for _ in 0..frames {
            app.world_mut().resource_mut::<Time>().advance_by(step);
            app.update();
        }
        let shown = headlines(&mut app);
        assert!(
            shown.iter().any(|line| line == EN.ann_surge),
            "the surge got its turn: {shown:?}"
        );
        assert_eq!(app.world().resource::<Announcer>().pending(), 0);
    }

    /// Every announcement has a headline and a line explaining it, in every
    /// language: a "Fresh Sand" banner that does not say your signposts just
    /// washed away is a colour, not information.
    #[test]
    fn every_announcement_says_what_happened() {
        let mut all = vec![Announcement::Surge, Announcement::Lure(0)];
        all.extend(TideEvent::ALL.map(Announcement::Tide));
        for lang in ALL_LANGS {
            for announcement in &all {
                let settings = GameSettings {
                    language: lang,
                    ..GameSettings::default()
                };
                let (head, blurb, _) =
                    announcement.lines(&settings, &crate::app::SeatNames(settings.names.clone()));
                assert!(!head.is_empty(), "{announcement:?} in {lang:?}");
                assert!(blurb.len() > head.len(), "{announcement:?} in {lang:?}");
            }
        }
        // The lure names the seat that pulled it off.
        let (_, blurb, ink) = Announcement::Lure(3)
            .lines(&GameSettings::default(), &crate::app::SeatNames::default());
        assert!(blurb.contains('4'), "{blurb}");
        assert_eq!(ink, palette::INK_LURE, "the feed line's colour");
    }
}
