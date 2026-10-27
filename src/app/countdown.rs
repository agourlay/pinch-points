//! The three seconds before a round: the beach is laid out and holding
//! still, so everyone can find their castle and read the map before the
//! first crab moves, and a count in the middle of the screen says when.
//!
//! A phase of its own ([`VersusPhase::Countdown`]) rather than a flag, so
//! everything that waits for a round to be live (the sim, the placements,
//! the AI, the tide clock, the round-over check, the net's patience with a
//! silent seat) already waits for it, without being told.
//!
//! Online, every peer counts on its own clock from the moment its `Start`
//! lands, so the counts end as far apart as the `Start`s arrived, which is
//! the same skew a round has always opened with. Nothing is sent while a
//! peer counts: the lockstep keeps resending every commit a peer could be
//! missing, so the frames the first one out commits reach the others when
//! they come out too, and the round-holding checks only run once the round
//! is, so three seconds of quiet costs nobody their seat.

use crate::app::i18n::{Tr, fill};
use crate::app::settings::GameSettings;
use crate::app::{Sim, VersusPhase, menu_ui, palette};
use crate::sim::{Board, CapPolicy, CrabKind, LURE_SECONDS, SIGNPOST_LIFETIME, TICKS_PER_SECOND};
use bevy::prelude::*;

/// How long the beach holds still before the round starts.
pub const COUNTDOWN_SECS: f32 = 3.0;

/// How long the word that starts the round stays up, fading as it goes.
const GO_SECS: f32 = 0.7;

/// The count's size: a documented literal rather than one of the type
/// scale's, because nothing else on screen is meant to be read from across
/// a room. Drawn at one size and popped by scale, which keeps it to one
/// glyph atlas (see the score chips' pop for the cost of the other way).
const COUNT_PX: f32 = 160.0;

/// How far over its size a number lands before settling.
const POP: f32 = 0.45;

/// The widest the tip under the count is drawn, in design pixels: the
/// board's own width between the two sidebars at the design size, less a
/// margin, so the line never runs under a scoreboard.
pub const TIP_W: f32 = 700.0;

/// The tip pill's padding either side of its line, as the prompt pill has.
const TIP_PAD_X: f32 = 12.0;

/// The castle the raid tip robs, for a figure a player can picture.
const RAID_EXAMPLE: u32 = 40;

/// The rules tips that hold on `board`'s table, each a line for the count
/// before a round: a rule switched off on this table (raids, the event
/// wheel, the fading arrows of the versus rule, the called golden crab)
/// has no tip. Every number is the rule's own, read from the constant or
/// the board that plays it.
pub fn tips(tr: &Tr, board: &Board) -> Vec<String> {
    let rules = board.rules();
    let secs = |ticks: u32| ticks.div_ceil(TICKS_PER_SECOND).to_string();
    let mut out = Vec::new();
    if rules.castle_raids && rules.gull_period > 0 {
        let taken = crate::sim::raid_loss(RAID_EXAMPLE);
        out.push(fill(
            tr.tip_raid,
            &[("a", &RAID_EXAMPLE.to_string()), ("b", &taken.to_string())],
        ));
    }
    if rules.cap_policy == CapPolicy::Evict {
        out.push(fill(tr.tip_fade, &[("n", &secs(SIGNPOST_LIFETIME))]));
        out.push(fill(tr.tip_cap, &[("n", &rules.signpost_cap.to_string())]));
    }
    out.push(fill(tr.tip_lure, &[("n", &LURE_SECONDS.to_string())]));
    if rules.golden_every > 0 {
        let worth = board.crab_value(CrabKind::Golden);
        out.push(fill(tr.tip_golden, &[("n", &worth.to_string())]));
    }
    if board.events_enabled() {
        out.push(tr.tip_events.to_string());
    }
    out
}

/// The one tip this round shows, picked by the board's seed: every peer at
/// an online table builds the same board and reads the same tip, and a
/// fresh seed each round turns the tips over.
pub fn round_tip(tr: &Tr, board: &Board) -> Option<String> {
    let mut tips = tips(tr, board);
    let at = (board.seed() % tips.len().max(1) as u64) as usize;
    (at < tips.len()).then(|| tips.swap_remove(at))
}

/// The tip line under the count.
#[derive(Component)]
pub struct CountdownTip;

/// Seconds left on the count.
#[derive(Resource, Default)]
pub struct Countdown {
    left: f32,
    /// The number last said out loud ([`CountBeat`]), so each is said
    /// once, on the frame it comes up.
    said: Option<u32>,
    /// Seconds left of the line-up before the count begins
    /// ([`crate::app::lineup`]): the first round of an online match opens
    /// on who is playing and on what, every peer for the same fixed time
    /// from its own `Start`, as the count itself is run.
    lineup: f32,
}

/// A beat of the count, for the ear: a number coming up, or the word that
/// starts the round. Written once each by [`run_countdown`], whatever the
/// frame rate, and only on a count that runs: a recording or a round
/// joined part way opens without one, and says nothing.
#[derive(Message, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CountBeat {
    Number(u32),
    Go,
}

impl Countdown {
    /// Seconds left of the line-up, none once the count has begun.
    pub fn lineup_left(&self) -> f32 {
        self.lineup.max(0.0)
    }

    /// The line-up at `secs` left, as a round opening on it sets it.
    #[cfg(test)]
    pub fn set_lineup_for_tests(&mut self, secs: f32) {
        self.lineup = secs;
    }

    /// The number on screen: 3, 2, then 1, each for its own second.
    fn shown(&self) -> u32 {
        self.left.ceil().max(1.0) as u32
    }

    /// How far into its second the number on screen is, 0 to 1.
    fn settled(&self) -> f32 {
        (self.left.ceil() - self.left).clamp(0.0, 1.0)
    }
}

/// How a versus round opens: on the count, or at once (a recording, a
/// round joined or resumed part way, which is already moving).
#[derive(bevy::ecs::system::SystemParam)]
pub struct Opening<'w> {
    countdown: ResMut<'w, Countdown>,
    next_vphase: ResMut<'w, NextState<VersusPhase>>,
    first_step: ResMut<'w, FirstStep>,
}

impl Opening<'_> {
    /// Open the round, on the count or at once, and when it counts in and
    /// `lineup`, on the line-up first.
    pub fn open(&mut self, count_in: bool, lineup: bool) {
        self.countdown.left = match count_in {
            true => COUNTDOWN_SECS,
            false => 0.0,
        };
        self.countdown.lineup = match count_in && lineup {
            true => crate::app::lineup::LINEUP_SECS,
            false => 0.0,
        };
        self.first_step.waiting = !count_in;
        self.countdown.said = None;
        self.next_vphase.set(match count_in {
            true => VersusPhase::Countdown,
            false => VersusPhase::Running,
        });
    }
}

/// A round that opens at once (a recording, a round resumed from a code
/// or caught up with part way) and has not had its first fixed step yet.
///
/// The frame that builds the board is a long one, and the fixed clock
/// banks all of it: the first frame the round runs on then played that
/// time as sand time, one or two ticks in a release build and up to the
/// clock's cap of seven in a debug one, before the beach had been on
/// screen for any of it. A round that counts in spends that frame under
/// the count instead. So the first fixed step of an opening without one
/// plays no tick and lets the banked time go ([`hold_the_opening_step`]),
/// and the round starts from its saved tick on the next.
#[derive(Resource, Default, Debug)]
pub struct FirstStep {
    /// Opened at once and not yet run.
    waiting: bool,
    /// This fixed step is that first one, and plays no tick.
    holding: bool,
}

impl FirstStep {
    /// A round just opened at once, as [`Opening::open`] leaves it.
    #[cfg(test)]
    pub(crate) fn waiting() -> FirstStep {
        FirstStep {
            waiting: true,
            holding: false,
        }
    }
}

/// Before the sim driver, every fixed step: whether this is the first step
/// of a round that opened at once ([`FirstStep`]), and if so, the time the
/// clock banked while the round was being built is let go.
pub fn hold_the_opening_step(
    screen: Res<State<crate::app::Screen>>,
    vphase: Res<State<VersusPhase>>,
    mut step: ResMut<FirstStep>,
    mut fixed: ResMut<Time<Fixed>>,
) {
    let running =
        *screen.get() == crate::app::Screen::Versus && *vphase.get() == VersusPhase::Running;
    step.holding = running && step.waiting;
    if step.holding {
        step.waiting = false;
        let banked = fixed.overstep();
        fixed.discard_overstep(banked);
    }
}

/// The sim driver's condition: not the held first step of a round that
/// opened at once.
pub fn past_the_opening_step(step: Res<FirstStep>) -> bool {
    !step.holding
}

/// The count in the middle of the screen, and then the word that starts
/// the round. `go` is how long that word has been up.
#[derive(Component)]
pub struct CountdownText {
    go: Option<f32>,
}

/// Put the count on screen, for a round that opens on one, with the
/// round's rules tip under it. Chained after `load_versus`, which winds it
/// and builds the board the tip is read from.
pub fn show_countdown(
    mut commands: Commands,
    countdown: Res<Countdown>,
    sim: Res<Sim>,
    settings: Res<GameSettings>,
) {
    if countdown.left <= 0.0 {
        return;
    }
    let tip = round_tip(settings.tr(), &sim.0);
    commands
        .spawn((
            CountdownText { go: None },
            GlobalZIndex(menu_ui::layer::BANNER),
            // The number, and the tip under it.
            Node {
                flex_direction: FlexDirection::Column,
                ..menu_ui::centred_overlay()
            },
        ))
        .with_children(|wrap| {
            wrap.spawn((
                Text::new(countdown.shown().to_string()),
                menu_ui::display_font(COUNT_PX),
                TextColor(palette::GOLD),
                TextShadow {
                    offset: Vec2::new(4.0, 5.0),
                    color: Color::BLACK.with_alpha(0.6),
                },
                UiTransform::IDENTITY,
            ));
            if let Some(tip) = tip {
                wrap.spawn((
                    Text::new(tip),
                    TextFont {
                        font_size: FontSize::Px(menu_ui::type_scale::BODY),
                        ..default()
                    },
                    TextLayout::justify(Justify::Center),
                    TextColor(palette::PARCHMENT),
                    // A pill, as the prompt is: bare text over the sand
                    // and the board's clutter does not read.
                    Node {
                        max_width: Val::Px(TIP_W),
                        margin: UiRect::top(Val::Px(8.0)),
                        padding: UiRect::axes(Val::Px(TIP_PAD_X), Val::Px(5.0)),
                        border_radius: BorderRadius::all(Val::Px(11.0)),
                        ..default()
                    },
                    BackgroundColor(palette::PILL_FILL),
                    CountdownTip,
                ));
            }
        });
}

/// Run the count down, and start the round when it is out. It holds while
/// the pause card is up: the card is a promise that nothing moves. Each
/// number is said as it comes up, and the word as the count runs out
/// ([`CountBeat`]).
pub fn run_countdown(
    time: Res<Time>,
    menu: Res<crate::app::pause::PauseMenu>,
    mut countdown: ResMut<Countdown>,
    mut next_vphase: ResMut<NextState<VersusPhase>>,
    mut beats: MessageWriter<CountBeat>,
) {
    if menu.stops_the_round() {
        return;
    }
    // The line-up first, said nothing over: the count is not up yet.
    if countdown.lineup > 0.0 {
        countdown.lineup -= time.delta_secs();
        return;
    }
    // The number on screen as this frame begins, said if it has not been:
    // the first frame of the count says its first number.
    let shown = countdown.shown();
    if countdown.left > 0.0 && countdown.said != Some(shown) {
        countdown.said = Some(shown);
        beats.write(CountBeat::Number(shown));
    }
    countdown.left -= time.delta_secs();
    if countdown.left <= 0.0 {
        if countdown.said.is_some() {
            beats.write(CountBeat::Go);
        }
        countdown.said = None;
        next_vphase.set(VersusPhase::Running);
    }
}

/// The overlay's two kinds of line: the number (then the word), which is
/// rewritten and popped, and the tip, which only fades.
#[derive(bevy::ecs::system::SystemParam)]
pub struct CountLines<'w, 's> {
    texts: Query<
        'w,
        's,
        (
            &'static mut Text,
            &'static mut TextColor,
            &'static mut UiTransform,
        ),
        Without<CountdownTip>,
    >,
    tips: Query<'w, 's, (&'static mut TextColor, &'static mut BackgroundColor), With<CountdownTip>>,
}

/// Draw the count: the number for this second, landing big and settling,
/// and once the round is live the word that started it, fading out.
pub fn draw_countdown(
    mut commands: Commands,
    time: Res<Time>,
    countdown: Res<Countdown>,
    phase: Res<State<VersusPhase>>,
    settings: Res<GameSettings>,
    mut overlays: Query<(Entity, &mut CountdownText, &Children, &mut Visibility)>,
    lines: CountLines,
) {
    let CountLines {
        mut texts,
        mut tips,
    } = lines;
    let calm = settings.reduced_motion;
    for (entity, mut overlay, children, mut shown) in &mut overlays {
        // Under the line-up, the count waits unseen.
        let want = match countdown.lineup > 0.0 {
            true => Visibility::Hidden,
            false => Visibility::Inherited,
        };
        if *shown != want {
            *shown = want;
        }
        let (word, ink, settle) = match phase.get() {
            VersusPhase::Countdown => (
                countdown.shown().to_string(),
                palette::GOLD,
                countdown.settled(),
            ),
            VersusPhase::Running => {
                let up = overlay.go.unwrap_or(0.0) + time.delta_secs();
                overlay.go = Some(up);
                if up >= GO_SECS {
                    commands.entity(entity).despawn();
                    continue;
                }
                let fade = 1.0 - up / GO_SECS;
                (
                    settings.tr().countdown_go.to_string(),
                    palette::PARCHMENT.with_alpha(fade),
                    up / GO_SECS,
                )
            }
            // The tide came in under it, which a three-second count cannot
            // outlast, but nothing should be left standing on the card.
            VersusPhase::Over => {
                commands.entity(entity).despawn();
                continue;
            }
        };
        let scale = match calm {
            true => 1.0,
            // Lands over its size and eases down, the way a stamp does.
            false => 1.0 + POP * (1.0 - settle).powi(3),
        };
        for child in children {
            // The tip fades out with the word, and is never rewritten.
            if let Ok((mut color, mut fill)) = tips.get_mut(*child) {
                let alpha = match overlay.go {
                    Some(up) => (1.0 - up / GO_SECS).max(0.0),
                    None => 1.0,
                };
                menu_ui::set_color(&mut color, palette::PARCHMENT.with_alpha(alpha));
                let pill = palette::PILL_FILL.with_alpha(palette::PILL_FILL.alpha() * alpha);
                menu_ui::set_bg(&mut fill, pill);
                continue;
            }
            let Ok((mut text, mut color, mut transform)) = texts.get_mut(*child) else {
                continue;
            };
            menu_ui::set_text(&mut text, &word);
            menu_ui::set_color(&mut color, ink);
            let pop = Vec2::splat(scale);
            if transform.scale != pop {
                transform.scale = pop;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three, two, one, each for a second, and never a zero: the round
    /// starts on the word, not on a number nobody counts to.
    #[test]
    fn the_count_reads_three_two_one() {
        let at = |left: f32| {
            Countdown {
                left,
                said: None,
                lineup: 0.0,
            }
            .shown()
        };
        assert_eq!(at(3.0), 3);
        assert_eq!(at(2.01), 3);
        assert_eq!(at(2.0), 2);
        assert_eq!(at(1.5), 2);
        assert_eq!(at(0.5), 1);
        assert_eq!(at(0.0), 1);
        assert_eq!(at(-0.1), 1);
    }

    /// The beach holds still for the count, and only for the count: the
    /// sim does not tick, the round starts when it is out, and the pause
    /// card holds it.
    #[test]
    fn the_round_starts_when_the_count_is_out() {
        use bevy::state::app::StatesPlugin;
        let mut app = App::new();
        app.add_plugins(StatesPlugin);
        app.init_state::<VersusPhase>();
        app.insert_resource(State::new(VersusPhase::Countdown));
        app.init_resource::<Countdown>();
        app.init_resource::<crate::app::pause::PauseMenu>();
        app.insert_resource(Time::<()>::default());
        app.world_mut().resource_mut::<Countdown>().left = COUNTDOWN_SECS;
        app.add_message::<CountBeat>();
        #[derive(Resource, Default)]
        struct Heard(Vec<CountBeat>);
        app.init_resource::<Heard>();
        app.add_systems(
            Update,
            (
                run_countdown,
                |mut beats: MessageReader<CountBeat>, mut heard: ResMut<Heard>| {
                    heard.0.extend(beats.read().copied());
                },
            )
                .chain(),
        );
        let step = |app: &mut App, secs: f32| {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(secs));
            app.update();
        };
        let phase = |app: &App| *app.world().resource::<State<VersusPhase>>().get();

        step(&mut app, 1.0);
        step(&mut app, 1.0);
        assert_eq!(phase(&app), VersusPhase::Countdown, "two seconds in");
        app.insert_resource(crate::app::pause::PauseMenu::raised());
        step(&mut app, 5.0);
        assert_eq!(phase(&app), VersusPhase::Countdown, "held by the card");
        app.insert_resource(crate::app::pause::PauseMenu::default());
        step(&mut app, 1.1);
        // The state change lands on the next frame's transition.
        app.update();
        assert_eq!(phase(&app), VersusPhase::Running, "and out");
        // Each number said once as it came up, the card's five seconds
        // silent, and the word as it ran out.
        use CountBeat::{Go, Number};
        assert_eq!(
            app.world().resource::<Heard>().0,
            [Number(3), Number(2), Number(1), Go]
        );
    }

    /// A round opened on the line-up holds still for the line-up and then
    /// the whole count, saying nothing until the count begins: every peer
    /// runs the same fixed time from its own `Start`, so nothing moves the
    /// round's start against the others'.
    #[test]
    fn the_lineup_comes_before_the_count() {
        use bevy::ecs::system::RunSystemOnce;
        use bevy::state::app::StatesPlugin;
        let mut app = App::new();
        app.add_plugins(StatesPlugin);
        app.init_state::<VersusPhase>();
        app.insert_resource(State::new(VersusPhase::Countdown));
        app.init_resource::<Countdown>();
        app.init_resource::<FirstStep>();
        app.init_resource::<crate::app::pause::PauseMenu>();
        app.insert_resource(Time::<()>::default());
        app.add_message::<CountBeat>();
        app.add_systems(Update, run_countdown);
        app.world_mut()
            .run_system_once(|mut opening: Opening| opening.open(true, true))
            .expect("opened");
        let step = |app: &mut App, secs: f32| {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(secs));
            app.update();
        };
        let phase = |app: &App| *app.world().resource::<State<VersusPhase>>().get();
        let beats = |app: &App| app.world().resource::<Messages<CountBeat>>().len();
        let lineup = crate::app::lineup::LINEUP_SECS;
        step(&mut app, 0.0);
        step(&mut app, lineup - 0.5);
        assert!(app.world().resource::<Countdown>().lineup_left() > 0.0);
        assert_eq!(beats(&app), 0, "nothing said under the line-up");
        step(&mut app, 1.0);
        assert_eq!(app.world().resource::<Countdown>().lineup_left(), 0.0);
        step(&mut app, COUNTDOWN_SECS - 0.5);
        assert_eq!(phase(&app), VersusPhase::Countdown, "the whole count");
        step(&mut app, 1.0);
        app.update();
        assert_eq!(phase(&app), VersusPhase::Running);
        // Opened at once, or on the count alone, there is no line-up.
        for (count_in, lineup) in [(false, true), (true, false)] {
            app.world_mut()
                .run_system_once(move |mut opening: Opening| opening.open(count_in, lineup))
                .expect("opened");
            assert_eq!(app.world().resource::<Countdown>().lineup_left(), 0.0);
        }
    }

    /// A versus table as the match setup builds it: the classic arena on
    /// `seed`, with the setup's gull and round dials.
    fn table(seed: u64) -> Board {
        use crate::app::match_setup::{MapChoice, MatchConfig, apply_terms, versus_board};
        let config = MatchConfig::default();
        let mut board = versus_board(MapChoice::Classic, seed, 2);
        apply_terms(&mut board, config.gulls, config.round, config.posts);
        board
    }

    /// The tips quote the rules' own numbers, and leave out a rule the
    /// table does not play by.
    #[test]
    fn the_tips_quote_the_rules_they_teach() {
        use crate::app::i18n::EN;
        let board = table(7);
        let said = tips(&EN, &board).join("\n");
        let secs = |ticks: u32| ticks.div_ceil(TICKS_PER_SECOND);
        for figure in [
            format!(
                "a raid on {RAID_EXAMPLE} takes {}",
                crate::sim::raid_loss(RAID_EXAMPLE)
            ),
            format!("fade {} seconds", secs(SIGNPOST_LIFETIME)),
            format!(
                "for {} s",
                crate::sim::LURE_TICKS.div_ceil(TICKS_PER_SECOND)
            ),
            format!("more than {} arrows", board.rules().signpost_cap),
            format!("worth {} points", board.crab_value(CrabKind::Golden)),
            "random event".to_string(),
        ] {
            assert!(said.contains(&figure), "{figure:?} in {said}");
        }
        let mut quiet = board.clone();
        quiet.rules_mut().castle_raids = false;
        quiet.set_events_enabled(false);
        quiet.rules_mut().cap_policy = CapPolicy::Reject;
        let said = tips(&EN, &quiet);
        assert_eq!(said.len(), tips(&EN, &board).len() - 4, "{said:?}");
        assert!(
            said.iter()
                .all(|tip| !tip.contains("raid") && !tip.contains("event"))
        );
    }

    /// One tip a round, the same on every screen of a table (it is read
    /// off the board's seed), and a different one as the seeds change.
    #[test]
    fn a_tip_turns_over_with_the_rounds() {
        use crate::app::i18n::EN;
        assert_eq!(round_tip(&EN, &table(3)), round_tip(&EN, &table(3)));
        let seen: std::collections::BTreeSet<String> = (0..40)
            .filter_map(|seed| round_tip(&EN, &table(seed)))
            .collect();
        assert_eq!(seen.len(), tips(&EN, &table(0)).len(), "{seen:?}");
    }

    /// Every tip in every language fits one line under the count, between
    /// the sidebars, at 1280x720 and 1280x800 (where the interface is drawn
    /// at its design size either way).
    #[test]
    fn every_tip_fits_one_line_between_the_sidebars() {
        use crate::app::i18n::{ALL_LANGS, metrics::text_px};
        use crate::app::settings::applied_ui_ratio;
        let between = 1280.0 - 2.0 * crate::app::side_panels::SIDEBAR_W;
        assert!(TIP_W < between, "the tip's room is the board's");
        let board = table(1);
        for height in [720.0, 800.0] {
            let scale = applied_ui_ratio(1.0, 1280.0, height);
            for lang in ALL_LANGS {
                for tip in tips(lang.tr(), &board) {
                    let width = text_px(&tip, menu_ui::type_scale::BODY) * scale;
                    assert!(
                        width <= (TIP_W - 2.0 * TIP_PAD_X) * scale,
                        "{lang:?} at 1280x{height}: {width:.0}px > {TIP_W}: {tip}"
                    );
                }
            }
        }
    }
}
