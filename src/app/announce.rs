//! Centre-screen announcements for the moments that change the round.
//!
//! The side feed reports everything, which makes it good for reading back
//! and poor for noticing: a lure or a tide event lands in a 14px line in
//! the corner while four players are watching the middle of the board. So
//! the roulette result goes across the centre of the screen, as the
//! original put it: big, brief, and colour-matched to the feed line that
//! records it.
//!
//! Render-only, and deliberately so: it never touches the sim or the clock,
//! so a peer that is a frame ahead still simulates the same round. Two
//! events landing together queue rather than overlap.

use crate::app::i18n::fill;
use crate::app::menu_ui;
use crate::app::palette;
use crate::app::settings::GameSettings;
use crate::app::sim_events::SimEvent;
use crate::sim::{PlayerId, TideEvent};
use bevy::prelude::*;
use std::collections::VecDeque;

/// How long one announcement holds the middle of the screen at full
/// strength, and the ramps at each end.
///
/// Two seconds of hold, not the one and a half it began with: the banner
/// carries a headline *and* a line explaining what the event does, and a
/// second and a half is enough to read the first and not the second. It can
/// be spent now the roulette has a cooldown ([`crate::sim::EVENT_COOLDOWN`])
/// and events no longer arrive in waves.
const HOLD: f32 = 2.0;
const FADE_IN: f32 = 0.15;
/// When a banner starts to fade, and the board under it can be seen again.
const HOLD_ENDS: f32 = FADE_IN + HOLD;
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
/// wait behind it, and the ceiling is `(MAX_QUEUED + 1) * LIFE`. At three,
/// with the hold at two seconds, that is 10.4 s; two keeps it at 7.8 s.
const MAX_QUEUED: usize = 2;

/// Something worth the centre of the screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Announcement {
    /// A tide event: `by` is the seat whose sparkling crab spun it (`None`
    /// for a called one), `keeper` the seat whose castle sat out a Castle
    /// Swap ([`crate::sim::Board::castle_swap_keeper`]).
    Tide {
        event: TideEvent,
        by: Option<PlayerId>,
        keeper: Option<PlayerId>,
    },
    /// A molting crab banked: every loose crab now runs for this seat.
    Lure(PlayerId),
    /// The final scramble.
    Surge,
    /// A golden crab is about to come out of a spawner hole.
    Golden,
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
            Announcement::Tide { event, keeper, .. } => (
                tr.events[event.index()].to_string(),
                match keeper {
                    // Only a swap has a keeper, and then the blurb has to
                    // say whose castle stayed put, or it tells the spinner
                    // they lost theirs.
                    Some(seat) => fill(tr.event_blurb_swap_kept, &[("p", &names.label(tr, seat))]),
                    None => tr.event_blurbs[event.index()].to_string(),
                },
                palette::INK_TIDE,
            ),
            Announcement::Lure(owner) => (
                tr.ann_lure.to_string(),
                fill(
                    tr.ann_lure_sub,
                    &[
                        ("p", &names.label(tr, owner)),
                        ("n", &crate::sim::LURE_SECONDS.to_string()),
                    ],
                ),
                palette::INK_LURE,
            ),
            Announcement::Surge => (
                tr.ann_surge.to_string(),
                tr.ann_surge_sub.to_string(),
                palette::INK_SURGE,
            ),
            Announcement::Golden => (
                tr.ann_golden.to_string(),
                fill(
                    tr.ann_golden_sub,
                    &[("n", &crate::sim::GOLDEN_CALLED_VALUE.to_string())],
                ),
                palette::GOLD,
            ),
        }
    }

    /// The small line over the headline, in its own ink: whose sparkling
    /// crab spun a tide event, in that seat's colour, as the feed line
    /// that records it is. Nothing for a called event, which nobody spun,
    /// or for any other announcement.
    pub fn kicker(
        self,
        settings: &GameSettings,
        names: &crate::app::SeatNames,
    ) -> Option<(String, Color)> {
        let tr = settings.tr();
        match self {
            Announcement::Tide { by: Some(seat), .. } => Some((
                fill(tr.ann_spun_by, &[("p", &names.label(tr, seat))]),
                palette::seat_text(seat),
            )),
            Announcement::Tide { by: None, .. }
            | Announcement::Lure(_)
            | Announcement::Surge
            | Announcement::Golden => None,
        }
    }
}

/// The announcement being shown and the ones waiting their turn.
#[derive(Resource, Default)]
pub struct Announcer {
    queue: VecDeque<Announcement>,
    /// Seconds the current banner has been up, once one is spawned.
    age: f32,
    /// Whether a banner is on screen.
    showing: bool,
}

impl Announcer {
    /// Whether a banner covers the middle of the board, or one is waiting
    /// to: what is happening there would be missed. Clear once the last
    /// one starts to fade.
    pub fn holds_the_board(&self) -> bool {
        !self.queue.is_empty() || (self.showing && self.age < HOLD_ENDS)
    }

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
    sim: Res<crate::app::Sim>,
    mut announcer: ResMut<Announcer>,
) {
    for event in events.read() {
        match event {
            SimEvent::TideEventFired { event, by } => {
                // Read off the board after the swap: a rotation keeps the
                // same owners, so the count that decided it still holds.
                let keeper = match event {
                    TideEvent::CastleSwap => sim.0.castle_swap_keeper(*by),
                    TideEvent::CrabMania
                    | TideEvent::GullMania
                    | TideEvent::Monopoly
                    | TideEvent::GullAttack
                    | TideEvent::SpeedUp
                    | TideEvent::SlowDown
                    | TideEvent::FreshSand
                    | TideEvent::RightClaws => None,
                };
                announcer.push(Announcement::Tide {
                    event: *event,
                    by: *by,
                    keeper,
                });
            }
            SimEvent::LureStarted { owner } => announcer.push(Announcement::Lure(*owner)),
            SimEvent::SurgeStarted => announcer.push(Announcement::Surge),
            SimEvent::GoldenCalled { .. } => announcer.push(Announcement::Golden),
            SimEvent::CrabBanked { .. }
            | SimEvent::CrabEaten { .. }
            | SimEvent::CrabSpawned { .. }
            | SimEvent::CastleRaided { .. }
            | SimEvent::GullIncoming { .. }
            | SimEvent::GullArrived
            | SimEvent::GullTookOff
            | SimEvent::GullLanded { .. }
            | SimEvent::GullShooed { .. }
            | SimEvent::GullFlewOff { .. }
            | SimEvent::SignpostPlaced { .. }
            | SimEvent::SignpostRemoved { .. }
            | SimEvent::SignpostEvicted { .. }
            | SimEvent::TierUp { .. }
            | SimEvent::WheelLocked { .. }
            | SimEvent::LureRefused { .. }
            | SimEvent::GoldenOut { .. }
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

/// What a banner is timed by: the frame's time, while the round moves on.
#[derive(bevy::ecs::system::SystemParam)]
pub struct BannerClock<'w> {
    time: Res<'w, Time>,
    round: crate::app::round_state::RoundState<'w>,
}

/// Show the queue one at a time: age the banner that is up, fade it, retire
/// it, then raise the next one.
///
/// All of it holds while a pause holds the round, as the round's own
/// effects do: the news is the round's, and a banner that went on fading
/// under the pause card, or the next one raised behind it, was gone, read
/// by nobody, by the time the round came back.
pub fn drive_announcements(
    mut commands: Commands,
    clock: BannerClock,
    settings: Res<GameSettings>,
    names: Res<crate::app::SeatNames>,
    mut announcer: ResMut<Announcer>,
    banners: Query<Entity, With<Banner>>,
    mut parts: Query<BannerColors>,
) {
    if clock.round.held() {
        return;
    }
    if let Ok(banner) = banners.single() {
        announcer.age += clock.time.delta_secs();
        if announcer.age >= LIFE {
            commands.entity(banner).despawn();
            announcer.age = 0.0;
            announcer.showing = false;
            return;
        }
        let alpha = envelope(announcer.age);
        for (part, text, border, fill) in &mut parts {
            let ink = part.ink.with_alpha(part.ink.alpha() * alpha);
            if let Some(mut color) = text {
                menu_ui::set_color(&mut color, ink);
            }
            if let Some(mut color) = border
                && color.top != ink
            {
                *color = BorderColor::all(ink);
            }
            if let (Some(mut color), Some(base)) = (fill, part.fill) {
                menu_ui::set_bg(&mut color, base.with_alpha(base.alpha() * alpha));
            }
        }
        return;
    }
    let Some(next) = announcer.queue.pop_front() else {
        return;
    };
    announcer.age = 0.0;
    announcer.showing = true;
    spawn_banner(&mut commands, next, &settings, &names);
}

/// How much of the board an event banner's panel hides at its fullest.
const BANNER_FILL_ALPHA: f32 = 0.9;

fn spawn_banner(
    commands: &mut Commands,
    announcement: Announcement,
    settings: &GameSettings,
    names: &crate::app::SeatNames,
) {
    let (headline, blurb, ink) = announcement.lines(settings, names);
    let kicker = announcement.kicker(settings, names);
    // Mostly opaque: the board shows through enough that the round reads
    // as still going on under the news, and not so much that the crabs
    // run through the letters, which a lighter panel once let them do.
    let card_fill = palette::CARD_BG.with_alpha(BANNER_FILL_ALPHA);
    commands
        .spawn((
            Banner,
            GlobalZIndex(menu_ui::layer::BANNER),
            menu_ui::centred_overlay(),
        ))
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
                if let Some((line, seat_ink)) = kicker {
                    card.spawn((
                        BannerPart {
                            ink: seat_ink,
                            fill: None,
                        },
                        Text::new(line),
                        TextFont {
                            font_size: FontSize::Px(menu_ui::type_scale::ROW),
                            ..default()
                        },
                        TextColor(seat_ink.with_alpha(0.0)),
                    ));
                }
                card.spawn((
                    BannerPart { ink, fill: None },
                    Text::new(headline),
                    menu_ui::display_font(46.0),
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
    announcer.showing = false;
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
        announcer.push(Announcement::Tide {
            event: TideEvent::CrabMania,
            by: None,
            keeper: None,
        });
        announcer.push(Announcement::Lure(2));
        // Counted off the cap rather than written as a number, so moving
        // the cap moves the test with it instead of pinning the old value.
        assert_eq!(announcer.pending(), MAX_QUEUED, "everything up to the cap");
        announcer.push(Announcement::Tide {
            event: TideEvent::FreshSand,
            by: Some(1),
            keeper: None,
        });
        assert_eq!(announcer.pending(), MAX_QUEUED, "the pile-up is capped");
    }

    /// A headless App the banners run in, on a versus round under way.
    fn banner_app() -> App {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.add_message::<SimEvent>();
        app.init_resource::<Announcer>();
        app.insert_resource(GameSettings::default());
        app.init_resource::<crate::app::SeatNames>();
        app.insert_resource(State::new(crate::app::Screen::Versus));
        app.init_resource::<crate::app::Paused>();
        app.init_resource::<crate::app::pause::PauseMenu>();
        app.init_resource::<crate::app::net::Online>();
        app.insert_resource(crate::app::Sim(crate::sim::Board::new(7, 5, 1)));
        app.add_systems(Update, (collect_announcements, drive_announcements).chain());
        app
    }

    /// A pause holds the banner that is up where it stands, and the one
    /// waiting behind it: under the pause card they went on fading and
    /// were gone, unread, by the time the round came back.
    #[test]
    fn a_pause_holds_the_banners() {
        use crate::app::sim_events::SimEvent;
        use std::time::Duration;

        let mut app = banner_app();
        app.world_mut().write_message(SimEvent::SurgeStarted);
        app.world_mut()
            .write_message(SimEvent::LureStarted { owner: 1 });
        app.update();
        let step = Duration::from_millis(200);
        app.world_mut().resource_mut::<Time>().advance_by(step);
        app.update();
        let age = app.world().resource::<Announcer>().age;
        assert!(age > 0.0, "the banner is up and ageing");

        app.world_mut().resource_mut::<crate::app::Paused>().0 = true;
        let frames = (LIFE / step.as_secs_f32()).ceil() as u32 * 2;
        for _ in 0..frames {
            app.world_mut().resource_mut::<Time>().advance_by(step);
            app.update();
        }
        let announcer = app.world().resource::<Announcer>();
        assert_eq!(announcer.age, age, "held where it stood");
        assert_eq!(announcer.pending(), 1, "and the next one held back");

        app.world_mut().resource_mut::<crate::app::Paused>().0 = false;
        for _ in 0..frames {
            app.world_mut().resource_mut::<Time>().advance_by(step);
            app.update();
        }
        assert_eq!(app.world().resource::<Announcer>().pending(), 0, "and on");
    }

    /// The wiring, end to end in a headless App: a tide event raises a
    /// banner that says what happened, one at a time, and it retires itself.
    #[test]
    fn an_event_raises_a_banner_that_retires_itself() {
        use crate::app::sim_events::SimEvent;
        use std::time::Duration;

        let mut app = banner_app();

        app.world_mut().write_message(SimEvent::TideEventFired {
            event: TideEvent::CastleSwap,
            by: None,
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
        // Counted off `LIFE` rather than written as a number of frames:
        // four 800 ms steps was exactly enough at 2.1 s, so lengthening the
        // hold left a test that never saw the second banner.
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

    /// The board is held while any banner covers it or waits to, through a
    /// queue of two, and let go as the last one starts to fade: what a
    /// Crab Monopoly's sweep waits for before its crabs fly.
    #[test]
    fn the_board_is_held_until_the_last_banner_fades() {
        use crate::app::sim_events::SimEvent;
        use std::time::Duration;

        let mut app = banner_app();
        let held = |app: &App| app.world().resource::<Announcer>().holds_the_board();
        assert!(!held(&app), "nothing to hold for");

        app.world_mut()
            .write_message(SimEvent::LureStarted { owner: 1 });
        app.world_mut().write_message(SimEvent::TideEventFired {
            event: TideEvent::Monopoly,
            by: Some(0),
        });
        let step = Duration::from_millis(50);
        let mut seconds = 0.0;
        let mut released = None;
        while seconds < 3.0 * LIFE {
            app.update();
            if !held(&app) && released.is_none() {
                released = Some(seconds);
            }
            app.world_mut().resource_mut::<Time>().advance_by(step);
            seconds += step.as_secs_f32();
        }
        let released = released.expect("the board was never let go");
        // The lure's whole life, a frame for its despawn to land, and the
        // Monopoly banner's hold.
        assert!(
            (LIFE + HOLD_ENDS..=LIFE + HOLD_ENDS + 0.2).contains(&released),
            "let go at {released} s"
        );
    }

    /// Every announcement has a headline and a line explaining it, in every
    /// language: a "Fresh Sand" banner that does not say your signposts just
    /// washed away is a colour, not information.
    #[test]
    fn every_announcement_says_what_happened() {
        let mut all = vec![Announcement::Surge, Announcement::Lure(0)];
        all.extend(TideEvent::ALL.map(|event| Announcement::Tide {
            event,
            by: Some(0),
            keeper: None,
        }));
        all.push(Announcement::Tide {
            event: TideEvent::CastleSwap,
            by: Some(0),
            keeper: Some(0),
        });
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

    /// A spun event's banner names the seat that spun it, in that seat's
    /// colour, in every language; a called one names nobody.
    #[test]
    fn a_spun_event_names_its_spinner_in_their_colour() {
        for lang in ALL_LANGS {
            let settings = GameSettings {
                language: lang,
                ..GameSettings::default()
            };
            let names = crate::app::SeatNames(settings.names.clone());
            let label = names.label(settings.tr(), 2);
            let spun = Announcement::Tide {
                event: TideEvent::SpeedUp,
                by: Some(2),
                keeper: None,
            };
            let (line, ink) = spun.kicker(&settings, &names).expect("a spinner");
            assert!(line.contains(&label), "{lang:?}: {line}");
            assert!(!line.contains('{'), "{lang:?} left a placeholder: {line}");
            assert_eq!(ink, palette::seat_text(2));
            let called = Announcement::Tide {
                event: TideEvent::SpeedUp,
                by: None,
                keeper: None,
            };
            assert_eq!(called.kicker(&settings, &names), None);
        }
    }

    /// Castle Swap's banner says whose castle stayed put when one did, read
    /// off the same rule the swap used: three castle owners keep the
    /// spinner's, two rotate both, and the blurb for a full rotation is the plain one.
    #[test]
    fn a_castle_swap_banner_says_whose_castle_stayed() {
        use crate::app::sim_events::SimEvent;
        use crate::sim::TileKind;

        let keeper_at = |castles: u8| {
            let mut app = banner_app();
            {
                let mut sim = app.world_mut().resource_mut::<crate::app::Sim>();
                for seat in 0..castles {
                    sim.0.set_tile(seat, 0, TileKind::Castle(seat));
                }
            }
            app.world_mut().write_message(SimEvent::TideEventFired {
                event: TideEvent::CastleSwap,
                by: Some(1),
            });
            app.update();
            let texts: Vec<String> = app
                .world_mut()
                .query::<&Text>()
                .iter(app.world())
                .map(|text| text.0.clone())
                .collect();
            texts
        };
        let names = crate::app::SeatNames::default();
        let p2 = names.label(&EN, 1);
        let kept = fill(EN.event_blurb_swap_kept, &[("p", &p2)]);
        let three = keeper_at(3);
        assert!(three.contains(&kept), "{three:?}");
        let two = keeper_at(2);
        assert!(two.iter().any(|t| t == EN.event_blurbs[7]), "{two:?}");
        assert!(!two.contains(&kept), "{two:?}");
    }

    /// The lure banner says how long the lure runs, and in every language
    /// the figure is filled from the rule, so the two cannot disagree.
    #[test]
    fn the_lure_banner_states_the_lure_length_from_the_rule() {
        let seconds = crate::sim::LURE_SECONDS.to_string();
        for lang in ALL_LANGS {
            let settings = GameSettings {
                language: lang,
                ..GameSettings::default()
            };
            let (_, blurb, _) = Announcement::Lure(0)
                .lines(&settings, &crate::app::SeatNames(settings.names.clone()));
            assert!(blurb.contains(&seconds), "{lang:?}: {blurb}");
            assert!(!blurb.contains('{'), "{lang:?} left a placeholder: {blurb}");
        }
    }
}
