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
//! is told, and outside a round the game simply carries on.

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

/// After a caught panic, leave the round being played for the menu, and
/// say why there. Outside a round there is nothing to leave.
pub(super) fn leave_a_broken_round(
    screen: Res<State<Screen>>,
    settings: Res<settings::GameSettings>,
    mut notice: ResMut<RoundNotice>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    if !TRIPPED.swap(false, Ordering::Relaxed) {
        return;
    }
    if matches!(
        screen.get(),
        Screen::Puzzle | Screen::Versus | Screen::Interlude | Screen::Editor
    ) {
        notice.0 = settings.tr().round_failed.to_string();
        next_screen.set(Screen::Menu);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A system that panics under this handler leaves the app running, and
    /// the round it was in is left for the menu with a word about it.
    fn a_system_with_a_bug() {
        panic!("a system with a bug (expected by this test)");
    }

    #[test]
    fn a_panic_ends_the_round_and_not_the_game() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.insert_state(Screen::Versus);
        app.insert_resource(settings::GameSettings::default());
        app.init_resource::<RoundNotice>();
        app.insert_resource(bevy::ecs::error::FallbackErrorHandler(keep_going));
        app.add_systems(Update, (a_system_with_a_bug, leave_a_broken_round).chain());
        // The panic's message is printed by the default hook: it is this
        // test's, and expected. The hook is not swapped out to hide it,
        // because it is process-wide and would hide other tests' failures.
        app.update();
        app.update();
        assert_eq!(*app.world().resource::<State<Screen>>().get(), Screen::Menu);
        assert_eq!(
            app.world().resource::<RoundNotice>().0,
            crate::app::i18n::EN.round_failed
        );
    }
}
