//! The window's close button (or Alt+F4) at the editor: unsaved work is
//! asked about first, the way Esc asks, and a second request closes.

use super::EditorState;
use super::playtest::end_playtest;
use super::shelf::{Offer, ask_first, unsaved_work};
use crate::app::Screen;
use crate::app::Sim;
use crate::app::gamepad::LastDevice;
use crate::app::session::BoardSprites;
use crate::app::settings::GameSettings;
use bevy::prelude::*;
use bevy::window::{ClosingWindow, WindowCloseRequested};

/// The bench a close request reaches, and what says how to ask.
#[derive(bevy::ecs::system::SystemParam)]
pub struct CloseAsker<'w, 's> {
    screen: Option<Res<'w, State<Screen>>>,
    state: Option<ResMut<'w, EditorState>>,
    sim: Option<ResMut<'w, Sim>>,
    settings: Res<'w, GameSettings>,
    device: Res<'w, LastDevice>,
    sprites: BoardSprites<'w, 's>,
}

impl CloseAsker<'_, '_> {
    /// Whether a request to close the window is held back, to ask first:
    /// at the editor with work that is not on the shelf, and not already
    /// asked. A playtest ends first, as Esc ends it, so the question shows
    /// on the bench it is about; one begun from the menu's level card has
    /// no bench of the player's to lose.
    fn holds(&mut self, commands: &mut Commands) -> bool {
        let Self {
            screen,
            state,
            sim,
            settings,
            device,
            sprites,
        } = self;
        let (Some(screen), Some(state), Some(sim)) = (screen, state, sim) else {
            return false;
        };
        if *screen.get() != Screen::Editor {
            return false;
        }
        let tr = settings.tr();
        if end_playtest(sim, state, commands, sprites) == Some(true) {
            return false;
        }
        let at_stake = unsaved_work(state, &sim.0);
        !ask_first(
            state,
            Offer::Leave,
            at_stake,
            Offer::Leave.warning(tr, **device),
        )
    }
}

/// Bevy's own `close_when_requested`, which the window plugin is told to
/// leave out, with the editor's question in front of it: a window asked
/// to close is marked closing, and despawned on the next frame, unless the
/// editor holds it back to ask ([`CloseAsker::holds`]). The second request
/// goes through, as the second Esc leaves.
pub fn close_when_asked(
    mut commands: Commands,
    mut requests: MessageReader<WindowCloseRequested>,
    closing: Query<Entity, With<ClosingWindow>>,
    mut asker: CloseAsker,
) {
    // Marked on the last frame, so gone on this one.
    for window in &closing {
        commands.entity(window).despawn();
    }
    // One request a frame is answered: a close button pressed twice in
    // one frame is one press, not the question and its answer.
    if let Some(request) = requests.read().last()
        && !asker.holds(&mut commands)
    {
        // `try_`: a request repeated for a window just despawned above.
        commands.entity(request.window).try_insert(ClosingWindow);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::bench_app;
    use super::super::{editor_commands, editor_input};
    use super::*;

    /// A close request at the editor with unsaved work asks, as Esc does,
    /// and the second one closes. Off the editor it closes at once.
    #[test]
    fn closing_the_window_asks_about_unsaved_work() {
        let mut app = bench_app();
        app.add_message::<WindowCloseRequested>();
        app.insert_state(Screen::Editor);
        app.add_systems(Update, (editor_input, editor_commands).chain());
        app.add_systems(Last, close_when_asked);
        let window = app.world_mut().spawn_empty().id();
        let request = |app: &mut App| {
            app.world_mut()
                .write_message(WindowCloseRequested { window });
            app.update();
        };
        let closing = |app: &App| app.world().get::<ClosingWindow>(window).is_some();

        app.world_mut().resource_mut::<Sim>().0.set_wrap(true);
        request(&mut app);
        assert!(!closing(&app), "the first request only asks");
        let state = app.world().resource::<EditorState>();
        assert_eq!(state.offer, Some(Offer::Leave));
        assert_eq!(state.feedback, crate::app::i18n::EN.ed_leave_confirm);
        // A frame between, as a person takes, and the question stands.
        app.update();
        request(&mut app);
        assert!(closing(&app), "the second one closes");
        app.update();
        assert!(
            app.world().get_entity(window).is_err(),
            "and the window goes"
        );

        // Off the editor, nothing is asked.
        let window = app.world_mut().spawn_empty().id();
        app.insert_state(Screen::Menu);
        app.update();
        app.world_mut()
            .write_message(WindowCloseRequested { window });
        app.update();
        assert!(app.world().get::<ClosingWindow>(window).is_some());
    }
}
