//! Playing the same table again: R on a couch round's results card, and
//! "Restart round" on its pause card.
//!
//! Only a match set up at this machine's match setup has a table to play
//! again. An online round has the lobby and the host's call for that, a
//! replay is a recording, a round picked back up from a code brings a
//! table that is nobody's setup, and the daily is the day's one beach,
//! entered from the menu. Every reader asks [`rematch`], so the prompt,
//! the pad's row and the key all agree on whether there is one.

use crate::app::match_setup::{MapChoice, MatchConfig};
use crate::app::net::Online;
use crate::app::tournament::Tournament;
use crate::app::{Screen, VersusPhase};
use bevy::prelude::*;

/// What this machine knows about the round on the sand that a rematch
/// needs: whether it was set up here, and where its series began.
#[derive(Resource, Default, Debug)]
pub struct LocalTable {
    /// The round was built from this machine's match setup (not the
    /// daily's own table, a replay, an online table, a resumed round or a
    /// dev hook's). Set by `load_versus` as each round is built.
    set_here: bool,
    /// The map and handmade beach the series opened on. A series steps
    /// the map dial each round, and a series played again opens where the
    /// first one did.
    opening: Option<(MapChoice, usize)>,
    /// The arena is being entered again on the same table. Read on the
    /// way out of the arena, which keeps the match armed for the way back
    /// in (`end_versus`), and spent when the round is built.
    again: bool,
    /// R pressed once on the results card of a series still being played,
    /// waiting for the second press that throws the series away
    /// ([`asks_first`]). Taken back when the next round is built.
    restart: crate::app::menu_ui::SecondPress,
}

impl LocalTable {
    /// Note how the round being built came to be. `set_here` for a match
    /// set up on this machine; the map is kept as the series' opening
    /// when this is its first round (or a single round).
    pub fn note_round(&mut self, set_here: bool, config: &MatchConfig, tournament: &Tournament) {
        self.set_here = set_here;
        self.again = false;
        self.restart = crate::app::menu_ui::SecondPress::default();
        if set_here && (!tournament.in_series() || tournament.round <= 1) {
            self.opening = Some((config.map, config.custom));
        }
    }

    /// Whether the round on the sand was set up on this machine.
    pub fn set_here(&self) -> bool {
        self.set_here
    }

    /// Whether the arena is being entered again on the same table, which
    /// keeps the match armed through the arena's own teardown.
    pub fn keeps_the_table(&self) -> bool {
        self.again
    }
}

/// What R on the results card plays again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rematch {
    /// One round, set up the same way, on a fresh seed.
    Round,
    /// The whole series from its first round: the tally back to nothing
    /// and the map back to the one it opened on. A series is a match, and
    /// its last round alone is not one to play again.
    Series,
    /// [`Rematch::Series`] with R pressed once already: the series is still
    /// being played ([`asks_first`]), and R again restarts it.
    SeriesAsked,
}

impl Rematch {
    /// What is played again once the asking is done: a series asked about
    /// is the series.
    pub fn settled(self) -> Rematch {
        match self {
            Rematch::Round => Rematch::Round,
            Rematch::Series | Rematch::SeriesAsked => Rematch::Series,
        }
    }
}

/// Whether R on the results card asks before it plays again: restarting
/// a series that is still being played throws its tally away, so the
/// first press only asks and a second inside the window
/// ([`crate::app::menu_ui::SecondPress`]) restarts it. A single round, or
/// a series already decided, has nothing to lose and plays again at once.
pub fn asks_first(rematch: Rematch, tournament: &Tournament) -> bool {
    match rematch.settled() {
        Rematch::Round => false,
        Rematch::Series | Rematch::SeriesAsked => tournament.is_running(),
    }
}

/// R on the results card, pressed at `now`: what to play again, or `None`
/// while it only asks. The one rule the key follows, and the reason
/// [`rematch`] reports [`Rematch::SeriesAsked`] to the prompt and the pad.
pub fn press_rematch(
    table: &mut LocalTable,
    rematch: Rematch,
    tournament: &Tournament,
    now: f32,
) -> Option<Rematch> {
    let go = !asks_first(rematch, tournament) || table.restart.press(now, ());
    go.then(|| rematch.settled())
}

/// A key other than R pressed on the results card, or the window run out
/// at `now`: the question R asked is taken back. Touches the table only
/// when there is one, so its change marks a change of question.
pub fn settle_rematch_ask(table: &mut ResMut<LocalTable>, other_key: bool, now: f32) {
    if !table.restart.asked() {
        return;
    }
    let lapsed = table.bypass_change_detection().restart.lapse(now);
    if lapsed {
        table.set_changed();
    } else if other_key {
        table.restart.take_back();
    }
}

/// Whether the round just played can be played again from here, and what
/// that replays: the one answer the results card's key, its prompt and
/// the pad's row all read.
pub fn rematch(table: &LocalTable, online: &Online, tournament: &Tournament) -> Option<Rematch> {
    if !table.set_here || online.0.is_some() {
        return None;
    }
    Some(match (tournament.in_series(), table.restart.asked()) {
        (true, true) => Rematch::SeriesAsked,
        (true, false) => Rematch::Series,
        (false, _) => Rematch::Round,
    })
}

/// Whether the pause card offers to restart the round under it: a table
/// that could be played again, while the round is still being played.
/// Restarting replays this round only, its place in a series and the
/// tally kept, since the round was never counted.
pub fn restart_offered(
    table: &LocalTable,
    online: &Online,
    tournament: &Tournament,
    vphase: VersusPhase,
) -> bool {
    let playing = match vphase {
        VersusPhase::Countdown | VersusPhase::Running => true,
        VersusPhase::Over => false,
    };
    playing && rematch(table, online, tournament).is_some()
}

/// The prompt's words for R: what it plays again.
pub fn rematch_key(tr: &crate::app::i18n::Tr, rematch: Rematch) -> &'static str {
    match rematch {
        Rematch::Round => tr.prompt_rematch,
        Rematch::Series => tr.prompt_rematch_series,
        Rematch::SeriesAsked => tr.prompt_rematch_series_confirm,
    }
}

/// The results card's line of keys: where Enter goes, at a table that
/// can play again what R does, and the round's offer to the replay shelf
/// or its word that the round is kept ([`crate::app::replays::keep_words`]).
pub fn over_prompt(
    tr: &crate::app::i18n::Tr,
    online: &Online,
    series_on: bool,
    rematch: Option<Rematch>,
    keep: crate::app::replays::KeepOffer,
) -> String {
    let door = crate::app::play_input::enter_door(tr, online, series_on);
    let again = rematch.map(|again| rematch_key(tr, again));
    let keep = crate::app::replays::keep_words(tr, keep);
    std::iter::once(door)
        .chain(again)
        .chain(keep)
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Which way the table is played again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Again {
    /// From the results card: see [`Rematch`].
    Rematch(Rematch),
    /// From the pause card: this round from its countdown.
    Restart,
}

/// The table, and the dials it was set up on: what playing it again
/// changes.
#[derive(bevy::ecs::system::SystemParam)]
pub struct PlayAgain<'w> {
    pub table: ResMut<'w, LocalTable>,
    pub config: ResMut<'w, MatchConfig>,
    pub next_vphase: ResMut<'w, NextState<VersusPhase>>,
    /// The shelf a Random map is drawn from again; with none read yet, the
    /// built-in maps alone.
    pub beaches: Option<Res<'w, crate::app::match_setup::CustomBeaches>>,
}

impl PlayAgain<'_> {
    /// A rematch was asked for and another screen was entered instead:
    /// the table is not kept for some later way into the arena.
    pub fn let_go(&mut self) {
        if std::mem::take(&mut self.table.again) {
            self.config.armed = false;
        }
    }

    /// Enter the arena again on the same seats, settings and pad claims.
    /// The board is built anew as every local round is, on a fresh seed:
    /// a generated beach is a new one, the classic and a handmade beach
    /// are the same sand with new luck. A rematch under step 2's Random
    /// card draws its map again, as each round of a series does; a
    /// restart is the same round, on the map it was on.
    pub fn ask(
        &mut self,
        again: Again,
        tournament: &mut Tournament,
        next_screen: &mut NextState<Screen>,
    ) {
        if again == Again::Rematch(Rematch::Series) {
            *tournament = Tournament::start(self.config.series);
            if let Some((map, custom)) = self.table.opening {
                self.config.map = map;
                self.config.custom = custom;
            }
        }
        if self.config.random && matches!(again, Again::Rematch(_)) {
            let none = crate::app::match_setup::CustomBeaches::default();
            crate::app::match_setup::draw_map(
                &mut self.config,
                self.beaches.as_deref().unwrap_or(&none),
                crate::app::clock::fresh_seed(),
            );
        }
        // Every round after the first is on a seed of its own.
        self.config.seed = None;
        self.table.again = true;
        self.config.armed = true;
        // The same screen again: its teardown and its build both run.
        next_screen.set(Screen::Versus);
        // And the count asked for in the same transition. The arena's own
        // ask for it, made while the screen changes, lands a frame later,
        // and a round restarted mid-play would run that frame on the new
        // beach before the count began.
        self.next_vphase.set(VersusPhase::Countdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tournament::SeriesLength;
    use bevy::ecs::system::RunSystemOnce;

    fn here() -> LocalTable {
        let mut table = LocalTable::default();
        table.note_round(true, &MatchConfig::default(), &Tournament::default());
        table
    }

    /// Only a table set up here is played again, and only offline.
    #[test]
    fn a_rematch_is_a_couch_tables_alone() {
        let alone = Online::default();
        let single = Tournament::default();
        assert_eq!(rematch(&here(), &alone, &single), Some(Rematch::Round));
        let series = Tournament::start(SeriesLength::BestOfThree);
        assert_eq!(rematch(&here(), &alone, &series), Some(Rematch::Series));
        // The daily, a replay, a resumed round: not set up here.
        let mut elsewhere = here();
        elsewhere.note_round(false, &MatchConfig::default(), &single);
        assert_eq!(rematch(&elsewhere, &alone, &single), None);
        assert!(!restart_offered(
            &elsewhere,
            &alone,
            &single,
            VersusPhase::Running
        ));
        assert!(restart_offered(
            &here(),
            &alone,
            &single,
            VersusPhase::Countdown
        ));
        assert!(!restart_offered(
            &here(),
            &alone,
            &single,
            VersusPhase::Over
        ));
    }

    /// R on the card of a series still being played asks first: the first
    /// press only asks, and the prompt says R again restarts it; a second
    /// inside the window restarts it; one after the window asks afresh. A
    /// single round plays again on the first press.
    #[test]
    fn restarting_a_series_midway_asks_first() {
        use crate::app::menu_ui::SECOND_PRESS_SECS;
        let alone = Online::default();
        let tr = crate::app::i18n::Lang::En.tr();
        let single = Tournament::default();
        let mut table = here();
        assert_eq!(
            press_rematch(&mut table, Rematch::Round, &single, 0.0),
            Some(Rematch::Round)
        );
        let series = Tournament::start(SeriesLength::BestOfThree);
        let mut table = here();
        let first = rematch(&table, &alone, &series).expect("a series to restart");
        assert_eq!(rematch_key(tr, first), tr.prompt_rematch_series);
        assert_eq!(press_rematch(&mut table, first, &series, 0.0), None);
        let asked = rematch(&table, &alone, &series).expect("still one");
        assert_eq!(asked, Rematch::SeriesAsked);
        assert_eq!(rematch_key(tr, asked), tr.prompt_rematch_series_confirm);
        assert_eq!(
            press_rematch(&mut table, asked, &series, 1.0),
            Some(Rematch::Series),
            "R again restarts it"
        );
        // Too late: the question lapsed, and R asks again.
        let mut table = here();
        assert_eq!(press_rematch(&mut table, first, &series, 0.0), None);
        let mut world = World::new();
        world.insert_resource(table);
        let _ = world.run_system_once(move |mut table: ResMut<LocalTable>| {
            settle_rematch_ask(&mut table, false, SECOND_PRESS_SECS + 0.5);
        });
        let mut table = world.remove_resource::<LocalTable>().expect("the table");
        assert_eq!(rematch(&table, &alone, &series), Some(Rematch::Series));
        assert_eq!(press_rematch(&mut table, first, &series, 5.0), None);
        // Another key takes the question back.
        let mut world = World::new();
        world.insert_resource(table);
        let _ = world.run_system_once(|mut table: ResMut<LocalTable>| {
            settle_rematch_ask(&mut table, true, 5.5);
        });
        let table = world.remove_resource::<LocalTable>().expect("the table");
        assert_eq!(rematch(&table, &alone, &series), Some(Rematch::Series));
    }

    /// A series played again opens on its first map with a clean tally;
    /// a restarted round keeps its map and the series where it stands.
    #[test]
    fn a_series_rematch_starts_over_and_a_restart_does_not() {
        let mut world = World::new();
        world.init_resource::<NextState<Screen>>();
        world.init_resource::<NextState<VersusPhase>>();
        world.init_resource::<crate::app::match_setup::CustomBeaches>();
        let config = MatchConfig {
            series: SeriesLength::BestOfThree,
            ..MatchConfig::default()
        };
        let mut tournament = Tournament::start(SeriesLength::BestOfThree);
        let mut table = LocalTable::default();
        table.note_round(true, &config, &tournament);
        world.insert_resource(table);
        world.insert_resource(config);
        // Round two, on the next map, with a win in.
        tournament.round = 2;
        tournament.wins[1] = 1;
        world.resource_mut::<MatchConfig>().map = MapChoice::GenSmall;
        world.resource_mut::<MatchConfig>().armed = false;
        let ask = |world: &mut World, again: Again, tournament: &mut Tournament| {
            let mut state: bevy::ecs::system::SystemState<(PlayAgain, ResMut<NextState<Screen>>)> =
                bevy::ecs::system::SystemState::new(world);
            let (mut play, mut next) = state.get_mut(world).expect("the params");
            play.ask(again, tournament, &mut next);
        };
        ask(&mut world, Again::Restart, &mut tournament);
        assert_eq!(tournament.round, 2, "a restart keeps the series' place");
        assert_eq!(tournament.wins[1], 1);
        assert_eq!(world.resource::<MatchConfig>().map, MapChoice::GenSmall);
        assert!(world.resource::<MatchConfig>().armed);
        assert!(world.resource::<LocalTable>().keeps_the_table());

        ask(&mut world, Again::Rematch(Rematch::Series), &mut tournament);
        assert_eq!(tournament.round, 1, "the series starts over");
        assert_eq!(tournament.wins, [0; crate::sim::MAX_PLAYERS]);
        assert!(tournament.is_running());
        assert_eq!(world.resource::<MatchConfig>().map, MapChoice::Classic);
    }

    /// Under step 2's Random card a rematch draws its map again, among the
    /// maps that seat the table, and on a seed of its own; a restart is the
    /// same round and keeps its map.
    #[test]
    fn a_random_rematch_draws_again() {
        let mut world = World::new();
        world.init_resource::<NextState<Screen>>();
        world.init_resource::<NextState<VersusPhase>>();
        world.init_resource::<crate::app::match_setup::CustomBeaches>();
        world.init_resource::<LocalTable>();
        world.insert_resource(MatchConfig {
            seats: 5,
            random: true,
            map: MapChoice::GenOcean,
            seed: Some(5),
            ..MatchConfig::default()
        });
        let ask = |world: &mut World, again: Again| {
            let mut state: bevy::ecs::system::SystemState<(PlayAgain, ResMut<NextState<Screen>>)> =
                bevy::ecs::system::SystemState::new(world);
            let (mut play, mut next) = state.get_mut(world).expect("the params");
            play.ask(again, &mut Tournament::default(), &mut next);
        };
        ask(&mut world, Again::Restart);
        assert_eq!(world.resource::<MatchConfig>().map, MapChoice::GenOcean);
        assert_eq!(world.resource::<MatchConfig>().seed, None, "a fresh seed");
        let mut drawn = std::collections::BTreeSet::new();
        for _ in 0..200 {
            ask(&mut world, Again::Rematch(Rematch::Round));
            let config = world.resource::<MatchConfig>();
            assert!(crate::app::match_setup::holds(config.map, 5));
            assert!(config.random);
            drawn.insert(format!("{:?}", config.map));
        }
        assert!(drawn.len() > 1, "drawn again, not kept: {drawn:?}");
    }

    /// The arena re-entered on the same table: its teardown, which
    /// disarms every other way out, leaves the match armed for its build,
    /// which then reads the couch's table rather than nobody's, and the
    /// round comes back in on its count. Any other screen entered lets the
    /// table go.
    #[test]
    fn a_rematch_comes_back_in_armed_and_on_the_count() {
        use crate::app::session::{end_versus, forget_a_round_not_entered};
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.insert_state(Screen::Versus);
        app.insert_state(VersusPhase::Running);
        app.init_resource::<Online>();
        app.init_resource::<crate::app::Playback>();
        app.init_resource::<crate::app::Recorder>();
        app.init_resource::<crate::app::session::ReelThread>();
        app.init_resource::<crate::app::replays::RoundToKeep>();
        app.init_resource::<crate::app::PendingActions>();
        app.init_resource::<crate::app::Controllers>();
        app.init_resource::<crate::app::Resuming>();
        app.init_resource::<crate::app::Daily>();
        app.init_resource::<Tournament>();
        // Spent by the build of the round being played.
        app.init_resource::<MatchConfig>();
        app.init_resource::<crate::app::match_setup::CustomBeaches>();
        let mut table = LocalTable::default();
        table.note_round(true, &MatchConfig::default(), &Tournament::default());
        app.insert_resource(table);
        #[derive(Resource, Default)]
        struct Entered(Vec<bool>);
        app.init_resource::<Entered>();
        app.add_systems(OnExit(Screen::Versus), end_versus);
        app.add_systems(
            OnEnter(Screen::Versus),
            |config: Res<MatchConfig>, mut entered: ResMut<Entered>| entered.0.push(config.armed),
        );
        app.add_systems(
            StateTransition,
            forget_a_round_not_entered
                .after(bevy::state::state::StateTransitionSystems::EnterSchedules),
        );
        app.update();
        let ask = |app: &mut App, again: Again| {
            app.world_mut()
                .run_system_cached_with(
                    |In(again): In<Again>,
                     mut play: PlayAgain,
                     mut tournament: ResMut<Tournament>,
                     mut next: ResMut<NextState<Screen>>| {
                        play.ask(again, &mut tournament, &mut next);
                    },
                    again,
                )
                .expect("asked");
        };
        ask(&mut app, Again::Restart);
        app.update();
        assert_eq!(
            app.world().resource::<Entered>().0,
            [false, true],
            "armed on the way back in"
        );
        assert_eq!(
            *app.world().resource::<State<VersusPhase>>().get(),
            VersusPhase::Countdown,
            "and on the count in the same frame"
        );

        // Asked, and the menu entered instead: nothing is left armed.
        ask(&mut app, Again::Rematch(Rematch::Round));
        app.world_mut()
            .resource_mut::<NextState<Screen>>()
            .set(Screen::Menu);
        app.update();
        assert!(!app.world().resource::<MatchConfig>().armed);
        assert!(!app.world().resource::<LocalTable>().keeps_the_table());
    }
}
