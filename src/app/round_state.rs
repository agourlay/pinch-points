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
use crate::sim::Board;
use bevy::prelude::*;

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
        let settled = *self.screen.get() == Screen::Puzzle
            && self
                .phase
                .as_ref()
                .is_some_and(|p| matches!(p.get(), Phase::Won | Phase::Lost));
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
        let held_online = self
            .online
            .0
            .as_ref()
            .is_some_and(|session| session.session.paused());
        !self.paused.0 && !held_online && !self.pause_menu.stops_the_round() && !self.ended(board)
    }
}
