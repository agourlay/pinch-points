//! Whether the round on screen is moving, asked in one place.
//!
//! The sea, the creatures, the clocks' blink and the controls line each
//! worked this out for themselves, from different flags, and disagreed:
//! an online pause sets no `Paused` (the lockstep holds instead), so the
//! sea heaved under an online pause card; and a creature read a late
//! network input, the ordinary rhythm of lockstep, as the round stopping
//! and twitched into its rest pose. This is the one answer.

use crate::app::net::Online;
use crate::app::pause::PauseMenu;
use crate::app::{Paused, Phase, Screen, VersusPhase};
use crate::sim::{Board, Level};
use bevy::prelude::*;

/// How far a round's clock has run, in ticks: what is left of it, and how
/// long it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Countdown {
    pub remaining: u64,
    pub limit: u64,
}

impl Countdown {
    /// How much of the round has gone, 0 at its start and 1 at its end.
    pub fn elapsed(self) -> f32 {
        1.0 - self.remaining as f32 / self.limit.max(1) as f32
    }
}

/// The clock a round on `board` runs down, or `None` for one with none.
/// A puzzle always has one: its own round when it carries one, else the
/// campaign tick limit [`Level::deadline`] fails it at. The HUD's clock
/// counts it down and the sea climbs with it, so the two cannot part: the
/// sea once read only the board's own round, and over an untimed puzzle
/// it lay flat while the clock ran red. Anywhere else the board's round
/// is the only clock.
pub fn countdown(board: &Board, puzzle: bool) -> Option<Countdown> {
    let limit = if puzzle {
        Level::deadline(board)
    } else {
        u64::from(board.rules().round_length?)
    };
    Some(Countdown {
        remaining: limit.saturating_sub(board.ticks()),
        limit,
    })
}

/// Seconds the round on screen has been moving, run by
/// [`tick_round_clock`]: the time the round's animations that more than one
/// system draws are drawn at, so they hold still under a pause card and on
/// the results with the rest, and still agree with one another (the
/// next-to-go post on the beach and its dot on the score chip pulse
/// together). On the editor's bench, where nothing pauses, it simply runs.
#[derive(Resource, Default)]
pub struct RoundClock(pub f32);

/// Run the [`RoundClock`] on by this frame, while the round moves.
pub fn tick_round_clock(
    time: Res<Time>,
    sim: Res<crate::app::Sim>,
    round: RoundState,
    mut clock: ResMut<RoundClock>,
) {
    round.run_clock(&sim.0, &mut clock.0, time.delta_secs());
}

/// Where the round on screen stands: what [`RoundState::moving`] and
/// [`RoundState::ended`] read.
#[derive(bevy::ecs::system::SystemParam)]
pub struct RoundState<'w> {
    screen: Res<'w, State<Screen>>,
    paused: Res<'w, Paused>,
    pause_menu: Res<'w, PauseMenu>,
    online: Res<'w, Online>,
    phase: Option<Res<'w, State<Phase>>>,
    versus: Option<Res<'w, State<VersusPhase>>>,
}

impl RoundState<'_> {
    /// Whether the round is over: a puzzle won or lost, a versus round at
    /// its results, or the board's own clock run out. What ends with a
    /// round (a mania's colour on the sea) stops showing here.
    pub fn ended(&self, board: &Board) -> bool {
        let settled = self.phase.as_ref().is_some_and(|phase| {
            crate::app::conditions::puzzle_settled(self.screen.get(), phase.get())
        });
        let over = *self.screen.get() == Screen::Versus
            && self
                .versus
                .as_ref()
                .is_some_and(|v| *v.get() == VersusPhase::Over);
        settled || over || board.round_over()
    }

    /// Whether the round is moving: not ended, and not held by any pause,
    /// offline (a replay's included), online, or the table's pause card.
    /// A lockstep wait for a late input is not a pause: the round is still
    /// moving, a frame behind.
    pub fn moving(&self, board: &Board) -> bool {
        !self.held() && !self.ended(board)
    }

    /// Whether a pause holds the round, offline, online or the table's
    /// card: [`Self::moving`] without the round's end. What finishes after
    /// the horn, a crab's hop into its castle, still stops for this.
    pub fn held(&self) -> bool {
        let held_online = self
            .online
            .0
            .as_ref()
            .is_some_and(|session| session.session.paused());
        self.paused.0 || held_online || self.pause_menu.stops_the_round()
    }

    /// Whether the screen is a versus round past its countdown and short
    /// of its results, paused or not.
    pub fn versus_running(&self) -> bool {
        *self.screen.get() == Screen::Versus
            && self
                .versus
                .as_ref()
                .is_some_and(|v| *v.get() == VersusPhase::Running)
    }

    /// The clock the round on screen runs down: [`countdown`], read for
    /// a puzzle on the puzzle screen.
    pub fn countdown(&self, board: &Board) -> Option<Countdown> {
        countdown(board, *self.screen.get() == Screen::Puzzle)
    }

    /// Run `clock` on by `dt` while the round is moving, and read it: the
    /// time an animation that belongs to the round is drawn at, so it holds
    /// still under a pause card and on the results with everything else.
    pub fn run_clock(&self, board: &Board, clock: &mut f32, dt: f32) -> f32 {
        if self.moving(board) {
            *clock += dt;
        }
        *clock
    }
}

/// An app holding what [`RoundState`] reads: a versus round running,
/// unpaused, with a time to advance. For a test of a system that holds
/// still under a pause card.
#[cfg(test)]
pub fn held_test_app() -> App {
    let mut app = App::new();
    app.add_plugins(bevy::state::app::StatesPlugin);
    app.insert_state(Screen::Versus);
    app.insert_state(VersusPhase::Running);
    app.init_resource::<Time>();
    app.init_resource::<Paused>();
    app.init_resource::<PauseMenu>();
    app.init_resource::<Online>();
    app
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The round's clock runs with the round and holds under the pause
    /// card, so what is drawn on it (the next-to-go wobble, a sweep's
    /// glow) freezes with the beach instead of running on the wall clock.
    #[test]
    fn the_round_clock_holds_under_the_pause_card() {
        let mut app = held_test_app();
        app.init_resource::<RoundClock>();
        app.insert_resource(crate::app::Sim(Board::new(9, 7, 1)));
        app.add_systems(Update, tick_round_clock);
        let step = |app: &mut App| {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_millis(100));
            app.update();
            app.world().resource::<RoundClock>().0
        };
        let running = step(&mut app);
        assert!(running > 0.0, "it runs with the round");
        app.insert_resource(PauseMenu::raised());
        assert_eq!(step(&mut app), running, "and holds under the card");
        app.insert_resource(PauseMenu::default());
        assert!(step(&mut app) > running, "and runs on after it");
    }
}
