//! What a shipped build does when one of its systems panics: says so in
//! the log, and stops the round it was in, rather than closing the game.
//!
//! At a party one bad system used to take a player out of the match and
//! off the beach. Bevy 0.20 hands a panic in a system, a command or an
//! observer to the fallback error handler instead of unwinding through the
//! app, and this is that handler, installed in release builds only: a
//! debug build still panics, so a bug is found where it happens.
//!
//! A round is not played on after one. The handler cannot tell which
//! system failed (Bevy keeps system names only in its `debug` builds), and
//! a panic in the middle of a tick leaves the board half moved: online the
//! hash check would end the round at the next comparison anyway, and
//! offline nothing would. So the round is stopped cleanly and the player
//! is told, and outside a round the game simply carries on. A round at a
//! table formed in the lobby goes back to the lobby, the way a round whose
//! host vanished does; one in the editor stops its playtest and stays at
//! the bench, the level being built still on it.
//!
//! What trips it is any error whose severity is [`Severity::Panic`], and
//! that is not only a caught unwind: it is also the severity Bevy gives an
//! error a system returns without naming one. Either way something did
//! not do what it was written to, and the round is stopped the same way.
//! A panic in the render sub-app is beyond its reach: that world has its
//! own error handler, and this one is installed on the main app only.

use super::*;
use bevy::ecs::error::{BevyError, ErrorContext, Severity, match_severity};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// A panic was caught since the last frame looked.
static TRIPPED: AtomicBool = AtomicBool::new(false);

/// Each panic's text, once: a system that panics every frame would
/// otherwise write its line sixty times a second.
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The fallback error handler for a shipped build: a panic is logged and
/// noted, and every other error goes where Bevy would send it.
pub(super) fn keep_going(error: BevyError, ctx: ErrorContext) {
    if error.severity() != Severity::Panic {
        match_severity(error, ctx);
        return;
    }
    TRIPPED.store(true, Ordering::Relaxed);
    let line = format!("{ctx}: {error}");
    let mut seen = SEEN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !seen.contains(&line) {
        error!("caught a panic and carried on: {line}");
        seen.push(line);
    }
}

/// Install [`keep_going`] in a release build. A debug build keeps Bevy's
/// own handler, which panics.
pub(super) fn install(app: &mut App) {
    if cfg!(not(debug_assertions)) {
        app.insert_resource(bevy::ecs::error::FallbackErrorHandler(keep_going));
    }
}

/// What a broken round puts back: the editor's bench, or the online table.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Wreckage<'w, 's> {
    online: Option<ResMut<'w, net::Online>>,
    sim: Option<ResMut<'w, Sim>>,
    editor: Option<ResMut<'w, editor::EditorState>>,
    commands: Commands<'w, 's>,
    sprites: session::BoardSprites<'w, 's>,
}

/// After a caught panic, leave the round being played and say why. Back
/// to the lobby when the table came from there, as a round whose host
/// went is (`net::leave_a_hostless_round`), else to the menu; the editor
/// stays the editor (see [`editor::recover`]). Outside a round there is
/// nothing to leave.
pub(super) fn leave_a_broken_round(
    screen: Res<State<Screen>>,
    settings: Res<settings::GameSettings>,
    mut notice: ResMut<RoundNotice>,
    mut next_screen: ResMut<NextState<Screen>>,
    wreckage: Wreckage,
) {
    if !TRIPPED.swap(false, Ordering::Relaxed) {
        return;
    }
    let Wreckage {
        online,
        sim,
        editor,
        mut commands,
        sprites,
    } = wreckage;
    let tr = settings.tr();
    match screen.get() {
        Screen::Editor => {
            if let (Some(mut sim), Some(mut editor)) = (sim, editor) {
                editor::recover(&mut sim, &mut editor, &mut commands, &sprites, tr);
            }
        }
        Screen::Puzzle | Screen::Versus | Screen::Interlude => {
            notice.0 = tr.round_failed.to_string();
            let mut lobby = false;
            if let Some(mut online) = online
                && let Some(session) = online.0.as_mut()
            {
                lobby = session.home.from_lobby;
                // Not on to another round of it: leaving the arena lets
                // the session go (`end_versus`) only when nothing armed
                // it, and between two rounds the arena is already left.
                session.next_round = false;
                if *screen.get() == Screen::Interlude {
                    online.0 = None;
                }
            }
            next_screen.set(match lobby {
                true => Screen::Lobby,
                false => Screen::Menu,
            });
        }
        // Outside a round the game simply carries on.
        Screen::Menu
        | Screen::Lobby
        | Screen::Settings
        | Screen::Controls
        | Screen::MatchSetup
        | Screen::Achievements
        | Screen::StageSelect
        | Screen::Replays
        | Screen::Language
        | Screen::NewVersion => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A system that panics under this handler leaves the app running, and
    /// the round it was in is left with a word about it.
    fn a_system_with_a_bug() {
        panic!("a system with a bug (expected by this test)");
    }

    /// A world on `screen` whose next update panics in a system and then
    /// runs the failsafe. Every case below runs in one test, one after the
    /// other, because the flag a panic raises is one for the process.
    fn broken(screen: Screen) -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.insert_state(screen);
        app.insert_resource(settings::GameSettings::default());
        app.init_resource::<RoundNotice>();
        app.insert_resource(bevy::ecs::error::FallbackErrorHandler(keep_going));
        app.add_systems(Update, (a_system_with_a_bug, leave_a_broken_round).chain());
        app
    }

    fn online_from(from_lobby: bool) -> net::Online {
        let mut session = net::OnlineSession::new(
            crate::transport::UdpTransport::host(0).expect("game socket"),
            crate::sim::Lockstep::new(0, vec![0, 1], 0),
            2,
            crate::transport::MatchTerms::default(),
        );
        session.home.from_lobby = from_lobby;
        net::Online(Some(session))
    }

    #[test]
    fn a_panic_ends_the_round_and_not_the_game() {
        // The panics' messages are printed by the default hook: they are
        // this test's, and expected. The hook is not swapped out to hide
        // them, because it is process-wide and would hide other tests'
        // failures.
        let mut app = broken(Screen::Versus);
        app.update();
        app.update();
        assert_eq!(*app.world().resource::<State<Screen>>().get(), Screen::Menu);
        assert_eq!(
            app.world().resource::<RoundNotice>().0,
            i18n::EN.round_failed
        );

        // A table formed in the lobby goes back to the lobby, as one whose
        // host vanished does; a pair wired up by hand has no lobby.
        for (from_lobby, home) in [(true, Screen::Lobby), (false, Screen::Menu)] {
            let mut app = broken(Screen::Versus);
            app.insert_resource(online_from(from_lobby));
            app.update();
            app.update();
            assert_eq!(*app.world().resource::<State<Screen>>().get(), home);
            let online = app.world().resource::<net::Online>();
            assert!(
                online.0.as_ref().is_none_or(|session| !session.next_round),
                "nothing carries the session on to another round"
            );
        }

        // The editor stays the editor: the playtest stops, and the level
        // being built is back on the sand rather than thrown away.
        let mut app = broken(Screen::Editor);
        let built = crate::sim::Board::new(9, 7, 5);
        let mut playing = crate::sim::Board::new(12, 9, 5);
        playing.set_wrap(true);
        app.insert_resource(Sim(playing));
        let mut state = editor::EditorState::default();
        state.mode = editor::Mode::Testing(Box::new(built.clone()));
        app.insert_resource(state);
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Editor
        );
        let editor = app.world().resource::<editor::EditorState>();
        assert!(!editor.is_testing(), "the playtest is over");
        assert_eq!(editor.feedback, i18n::EN.ed_failed);
        assert_eq!(
            app.world().resource::<Sim>().0.state_hash(),
            built.state_hash(),
            "the level being built is back"
        );
    }
}
