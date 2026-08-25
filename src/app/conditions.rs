//! The named run conditions the schedule is written in.
//!
//! Every one is a `fn(..) -> bool` over the screen and phase states (and
//! the odd resource), and every one is read by `schedule.rs` alone. They
//! were split between there and `session.rs`; here the schedule reads as
//! prose against one list, and `session.rs` is left with what its own doc
//! says it is: boot, teardown, reload, tick, outcome.

use super::{Phase, Playback, Screen, VersusPhase, editor, lobby, match_setup};
use bevy::prelude::*;

/// Screens that render a live board (settings and the lobby are clean).
pub(super) fn board_screens(screen: Res<State<Screen>>) -> bool {
    match screen.get() {
        Screen::Menu => false,
        Screen::Puzzle | Screen::Versus | Screen::Editor => true,
        Screen::Lobby
        | Screen::Settings
        | Screen::Controls
        | Screen::MatchSetup
        | Screen::Achievements
        | Screen::StageSelect
        | Screen::Replays
        | Screen::Interlude
        | Screen::Language
        | Screen::NewVersion => false,
    }
}

/// Screens where a round is being played, a puzzle or a versus match, as
/// one named condition, so the schedule reads as prose.
/// The screens that stand on the postcard beach: the menu at full
/// daylight, every browsing screen behind the scrim. Board screens are
/// out: a round is played over its own sand, not over a second beach.
pub(super) fn postcard_screens(screen: Res<State<Screen>>) -> bool {
    super::postcard_screen(*screen.get())
}

pub(super) fn play_screens(screen: Res<State<Screen>>) -> bool {
    matches!(screen.get(), Screen::Puzzle | Screen::Versus)
}

/// Whether a player is typing words rather than pressing keys: a seat's
/// name, a beach's name, an address, a line of chat.
///
/// The global letter keys are mnemonics, and a mnemonic has to stand down
/// when the letter is one the player meant to write - M is in "Emma" and
/// in most sentences worth sending. It only started to matter when M
/// became the master mute: silencing the whole game halfway through
/// typing a name is a good deal louder than the theme stopping.
pub(super) fn text_entry_open(
    lobby: Res<lobby::LobbyState>,
    setup: Res<match_setup::MatchMenu>,
    editor: Res<editor::EditorState>,
) -> bool {
    lobby.typing.is_some() || setup.naming.is_some() || editor.is_naming()
}

/// Whether the round on screen is being played rather than watched: a
/// replay rewatched is not a round won again, so nothing that counts
/// deeds (stats, trophies) runs while a recording is being fed.
pub(super) fn not_watching(playback: Res<Playback>) -> bool {
    playback.0.is_none()
}

/// The sim ticks while a puzzle runs or a versus round is live.
pub(super) fn sim_should_run(
    screen: Res<State<Screen>>,
    phase: Res<State<Phase>>,
    vphase: Res<State<VersusPhase>>,
    editor: Res<editor::EditorState>,
) -> bool {
    match screen.get() {
        // The menu is a still postcard; its little animations are not sim.
        Screen::Menu => false,
        Screen::Lobby
        | Screen::Settings
        | Screen::Controls
        | Screen::MatchSetup
        | Screen::Achievements
        | Screen::StageSelect
        | Screen::Replays
        | Screen::Interlude
        | Screen::Language
        | Screen::NewVersion => false,
        Screen::Puzzle => *phase.get() == Phase::Running,
        Screen::Versus => *vphase.get() == VersusPhase::Running,
        Screen::Editor => editor.is_testing(),
    }
}

/// The pause card is up, on any screen that can raise one.
pub(super) fn versus_running(screen: Res<State<Screen>>, phase: Res<State<VersusPhase>>) -> bool {
    *screen.get() == Screen::Versus && *phase.get() == VersusPhase::Running
}

pub(super) fn versus_over(screen: Res<State<Screen>>, phase: Res<State<VersusPhase>>) -> bool {
    *screen.get() == Screen::Versus && *phase.get() == VersusPhase::Over
}

pub(super) fn puzzle_setup(screen: Res<State<Screen>>, phase: Res<State<Phase>>) -> bool {
    *screen.get() == Screen::Puzzle && *phase.get() == Phase::Setup
}

pub(super) fn puzzle_running(screen: Res<State<Screen>>, phase: Res<State<Phase>>) -> bool {
    *screen.get() == Screen::Puzzle && *phase.get() == Phase::Running
}

pub(super) fn puzzle_done(screen: Res<State<Screen>>, phase: Res<State<Phase>>) -> bool {
    *screen.get() == Screen::Puzzle && matches!(*phase.get(), Phase::Won | Phase::Lost)
}
