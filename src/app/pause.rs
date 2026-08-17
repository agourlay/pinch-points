//! The pause overlay: Esc (or Start on a pad) during play opens a card with
//! Continue / Put the round down / Back to menu / Quit.
//!
//! Offline it simply freezes the sim. Online it runs the lockstep pause
//! protocol (see `sim::net`): the peers agree on a frame, everyone's commits
//! stop there, and the match halts on the same frame on every screen. A peer
//! that pauses opens the card on the others too, or their beach would
//! freeze with no explanation.

use crate::app::i18n::fill;
use crate::app::menu_ui;
use crate::app::net::Online;
use crate::app::palette;
use crate::app::settings::GameSettings;
use crate::app::suspend;
use crate::app::{Paused, Phase, Screen};
use bevy::prelude::*;

/// One pause-card action; navigation and Enter both match on this.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PauseAction {
    Continue,
    /// Save the round mid-play and leave; the menu offers it back.
    PutDown,
    ToMenu,
    Quit,
}

impl PauseAction {
    pub const ALL: [PauseAction; 4] = [
        PauseAction::Continue,
        PauseAction::PutDown,
        PauseAction::ToMenu,
        PauseAction::Quit,
    ];
}

/// Which rows are offered. Putting a round down is a versus, offline thing:
/// a puzzle is a fixed level that retries instantly and has nothing worth
/// carrying, and an online round belongs to every peer in it: one player
/// cannot pocket it.
fn live_rows(screen: Screen, online: &Online) -> [bool; PauseAction::ALL.len()] {
    let can_put_down = screen == Screen::Versus && online.0.is_none();
    std::array::from_fn(|row| PauseAction::ALL[row] != PauseAction::PutDown || can_put_down)
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
    sim: Res<crate::app::Sim>,
    seats: Res<crate::app::Seats>,
    bots: Res<crate::app::Bots>,
    mut notice: ResMut<crate::app::RoundNotice>,
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
    let live = live_rows(*screen.get(), &online);
    menu.selected = menu_ui::nav_live(&keys, menu.selected, &live);
    menu.selected = menu_ui::nav_live_steps(pad_up, pad_down, menu.selected, &live);
    if keys.just_pressed(KeyCode::Enter) || pad_accept {
        match PauseAction::ALL[menu.selected] {
            PauseAction::Continue => {
                if let Some(session) = online.0.as_mut() {
                    session.request_resume();
                }
                close(&mut commands, &mut menu, &mut paused, &ui);
            }
            PauseAction::PutDown => {
                let round = suspend::Suspended {
                    seats: seats.0,
                    bots: bots.0,
                    board: sim.0.clone(),
                };
                notice.0 = match suspend::put_down(&round) {
                    Ok(()) => settings.tr().round_put_down.to_string(),
                    Err(e) => fill(settings.tr().round_put_down_failed, &[("e", &e)]),
                };
                close(&mut commands, &mut menu, &mut paused, &ui);
                next_screen.set(Screen::Menu);
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
    screen: Res<State<Screen>>,
    online: Res<Online>,
    mut rows: Query<(&PauseRow, &mut Text, &mut TextColor)>,
) {
    let tr = settings.tr();
    let labels = [
        tr.pause_continue,
        tr.pause_put_down,
        tr.pause_to_menu,
        tr.pause_quit,
    ];
    let live = live_rows(*screen.get(), &online);
    for (row, mut text, mut color) in &mut rows {
        // A row that cannot be taken is left blank rather than greyed out:
        // the card is four lines, and an option explained away is more
        // clutter than the gap it fills.
        let label = if live[row.0] { labels[row.0] } else { "" };
        menu_ui::paint_row(row.0 == menu.selected, label, &mut text, &mut color);
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
        // The card can put a round down, so it reads the round.
        app.insert_resource(crate::app::Sim(crate::sim::Board::new(4, 4, 0)));
        app.init_resource::<crate::app::Seats>();
        app.init_resource::<crate::app::Bots>();
        app.init_resource::<crate::app::RoundNotice>();
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

        // Down three times lands on "quit", up once steps back to "back to
        // menu". Offline in versus, every row is live, so the walk is the
        // plain one.
        tap(&mut app, KeyCode::KeyS);
        tap(&mut app, KeyCode::KeyS);
        tap(&mut app, KeyCode::KeyS);
        assert_eq!(
            PauseAction::ALL[app.world().resource::<PauseMenu>().selected],
            PauseAction::Quit
        );
        tap(&mut app, KeyCode::KeyW);
        assert_eq!(
            PauseAction::ALL[app.world().resource::<PauseMenu>().selected],
            PauseAction::ToMenu
        );

        tap(&mut app, KeyCode::Enter);
        app.update(); // let the state transition apply
        assert!(!app.world().resource::<PauseMenu>().open, "the card closed");
        assert!(!app.world().resource::<Paused>().0, "and unfroze");
        assert_eq!(*app.world().resource::<State<Screen>>().get(), Screen::Menu);
    }
}
