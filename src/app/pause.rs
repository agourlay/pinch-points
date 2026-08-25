//! The pause overlay: Esc (or Start on a pad) during play opens a card with
//! Continue / Back to menu / Quit.
//!
//! Offline it simply freezes the sim. Online it runs the lockstep pause
//! protocol (see `sim::net`): the peers agree on a frame, everyone's commits
//! stop there, and the match halts on the same frame on every screen. A peer
//! that pauses opens the card on the others too, or their beach would
//! freeze with no explanation.

use crate::app::menu_ui;
use crate::app::net::Online;
use crate::app::palette;
use crate::app::settings::GameSettings;
use crate::app::{Paused, Phase, Screen};
use bevy::prelude::*;

/// One pause-card action; navigation and Enter both match on this.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PauseAction {
    Continue,
    ToMenu,
    Quit,
}

impl PauseAction {
    pub const ALL: [PauseAction; 3] = [
        PauseAction::Continue,
        PauseAction::ToMenu,
        PauseAction::Quit,
    ];
}

const OPTIONS: usize = PauseAction::ALL.len();

/// Whether the pause card is open, and which row is selected.
#[derive(Resource, Default)]
pub struct PauseMenu {
    pub open: bool,
    selected: usize,
}

#[derive(Component)]
pub struct PauseUi;

#[derive(Component)]
pub struct PauseRow(usize);

fn spawn_card(commands: &mut Commands, settings: &GameSettings) {
    let tr = settings.tr();
    commands
        .spawn((PauseUi, GlobalZIndex(20), menu_ui::centred_overlay()))
        .with_children(|wrap| {
            wrap.spawn(menu_ui::screen_card()).with_children(|card| {
                card.spawn((
                    Text::new(tr.pause_title),
                    TextFont {
                        font_size: FontSize::Px(30.0),
                        ..default()
                    },
                    TextColor(palette::GOLD),
                ));
                card.spawn(Node {
                    height: Val::Px(6.0),
                    ..default()
                });
                menu_ui::spawn_rows(card, OPTIONS, 24.0, PauseRow);
            });
        });
}

fn close(
    commands: &mut Commands,
    menu: &mut PauseMenu,
    paused: &mut Paused,
    ui: &Query<Entity, With<PauseUi>>,
) {
    menu.open = false;
    paused.0 = false;
    for entity in ui {
        commands.entity(entity).despawn();
    }
}

/// Open, navigate, and act on the pause card. Runs on the play screens.
#[allow(clippy::too_many_arguments)]
pub fn pause_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    settings: Res<GameSettings>,
    screen: Res<State<Screen>>,
    phase: Res<State<Phase>>,
    mut menu: ResMut<PauseMenu>,
    mut paused: ResMut<Paused>,
    mut online: ResMut<Online>,
    mut next_screen: ResMut<NextState<Screen>>,
    mut exit: MessageWriter<AppExit>,
    ui: Query<Entity, With<PauseUi>>,
) {
    // Pads: Start toggles the card, but never during puzzle setup where
    // Start means "begin the run".
    let start_ok = !(*screen.get() == Screen::Puzzle && *phase.get() == Phase::Setup);
    let pad_start = start_ok && pads.iter().any(|p| p.just_pressed(GamepadButton::Start));
    let pad_up = pads.iter().any(|p| p.just_pressed(GamepadButton::DPadUp));
    let pad_down = pads.iter().any(|p| p.just_pressed(GamepadButton::DPadDown));
    let pad_accept = pads.iter().any(|p| p.just_pressed(GamepadButton::South));

    // A peer's pause opens the card here too, so a frozen beach is never
    // unexplained. Their Escape is as good as ours.
    let peer_paused = online.0.as_ref().is_some_and(|s| s.session.paused());
    if !menu.open {
        if keys.just_pressed(KeyCode::Escape) || pad_start || peer_paused {
            menu.open = true;
            menu.selected = 0;
            match online.0.as_mut() {
                // Online: the peers agree on a frame to stop at, and the sim
                // halts there by itself. Freezing the local ticker instead
                // would stop the network pump that carries the resume.
                Some(session) if !peer_paused => session.request_pause(),
                Some(_) => {}
                None => paused.0 = true,
            }
            spawn_card(&mut commands, &settings);
        }
        return;
    }

    if keys.just_pressed(KeyCode::Escape) || pad_start {
        if let Some(session) = online.0.as_mut() {
            session.request_resume();
        }
        close(&mut commands, &mut menu, &mut paused, &ui);
        return;
    }
    // A peer resumed: everyone plays on.
    if online.0.is_some() && !peer_paused {
        close(&mut commands, &mut menu, &mut paused, &ui);
        return;
    }
    menu.selected = menu_ui::nav(&keys, menu.selected, OPTIONS);
    // Up wins over a same-frame down, as it always has.
    let pad_nav = if pad_up {
        menu_ui::Nav::Up
    } else if pad_down {
        menu_ui::Nav::Down
    } else {
        menu_ui::Nav::Stay
    };
    menu.selected = menu_ui::step(pad_nav, menu.selected, OPTIONS);
    if menu_ui::enter(&keys) || pad_accept {
        match PauseAction::ALL[menu.selected] {
            PauseAction::Continue => {
                if let Some(session) = online.0.as_mut() {
                    session.request_resume();
                }
                close(&mut commands, &mut menu, &mut paused, &ui);
            }
            PauseAction::ToMenu => {
                // Leaving drops the session anyway, but resume first so the
                // peers are not left frozen waiting for a player who quit.
                if let Some(session) = online.0.as_mut() {
                    session.request_resume();
                }
                close(&mut commands, &mut menu, &mut paused, &ui);
                next_screen.set(Screen::Menu);
            }
            PauseAction::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
}

/// Style the option rows around the selection.
pub fn update_pause_rows(
    menu: Res<PauseMenu>,
    settings: Res<GameSettings>,
    mut rows: Query<(&PauseRow, &mut Text, &mut TextColor)>,
) {
    let tr = settings.tr();
    let labels = [tr.pause_continue, tr.pause_to_menu, tr.pause_quit];
    for (row, mut text, mut color) in &mut rows {
        menu_ui::paint_row(row.0 == menu.selected, labels[row.0], &mut text, &mut color);
    }
}

/// Leaving a play screen always clears the card and unfreezes.
pub fn reset_pause(
    mut commands: Commands,
    mut menu: ResMut<PauseMenu>,
    mut paused: ResMut<Paused>,
    ui: Query<Entity, With<PauseUi>>,
) {
    close(&mut commands, &mut menu, &mut paused, &ui);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Screen;

    /// The card's own behaviour, which the pause *protocol* tests never
    /// touch: Escape opens it and freezes an offline round, the selection
    /// walks the options, and picking "back to the beach" leaves.
    #[test]
    fn escape_opens_the_card_and_the_menu_row_leaves() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_state::<Phase>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<PauseMenu>();
        app.init_resource::<Paused>();
        app.init_resource::<crate::app::net::Online>();
        app.insert_resource(GameSettings::default());
        app.add_message::<AppExit>();
        app.add_systems(Update, pause_input);

        let tap = |app: &mut App, key: KeyCode| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(key);
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
        };

        // A versus round, running.
        app.insert_resource(State::new(Screen::Versus));
        tap(&mut app, KeyCode::Escape);
        assert!(app.world().resource::<PauseMenu>().open, "the card opened");
        assert!(
            app.world().resource::<Paused>().0,
            "an offline round freezes with it"
        );

        // The card is three rows: down twice reaches the last of them.
        let row = |app: &App| PauseAction::ALL[app.world().resource::<PauseMenu>().selected];
        tap(&mut app, KeyCode::KeyS);
        tap(&mut app, KeyCode::KeyS);
        assert_eq!(row(&app), PauseAction::Quit);
        // A third wraps to the top rather than falling off the end.
        tap(&mut app, KeyCode::KeyS);
        assert_eq!(row(&app), PauseAction::Continue);
        // And back the other way, onto the row this test then takes.
        tap(&mut app, KeyCode::KeyW);
        assert_eq!(row(&app), PauseAction::Quit);
        tap(&mut app, KeyCode::KeyW);
        assert_eq!(row(&app), PauseAction::ToMenu);

        tap(&mut app, KeyCode::Enter);
        app.update(); // let the state transition apply
        assert!(!app.world().resource::<PauseMenu>().open, "the card closed");
        assert!(!app.world().resource::<Paused>().0, "and unfroze");
        assert_eq!(*app.world().resource::<State<Screen>>().get(), Screen::Menu);
    }
}
