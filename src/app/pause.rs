//! The pause overlay: Esc (or Start on a pad) during play opens a card with
//! Continue / Back to menu / Quit, and Restart round over a couch round.
//!
//! Offline it simply freezes the sim. Online it runs the lockstep pause
//! protocol (see `sim::net`): the peers agree on a frame, everyone's commits
//! stop there, and the match halts on the same frame on every screen. A peer
//! that pauses opens the card on the others too, or their beach would
//! freeze with no explanation.

use crate::app::gamepad::scheme;
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
    /// The round from its countdown, on the same table: a couch round
    /// only ([`crate::app::rematch::restart_offered`]).
    Restart,
    ToMenu,
    Quit,
}

impl PauseAction {
    /// The card's rows, top to bottom. A puzzle has no restart row: R
    /// (Y on a pad) resets a running puzzle already, and the card is the
    /// way out of one.
    pub fn rows(restart: bool) -> &'static [PauseAction] {
        match restart {
            true => &[
                PauseAction::Continue,
                PauseAction::Restart,
                PauseAction::ToMenu,
                PauseAction::Quit,
            ],
            false => &[
                PauseAction::Continue,
                PauseAction::ToMenu,
                PauseAction::Quit,
            ],
        }
    }

    fn label(self, tr: &crate::app::i18n::Tr) -> &'static str {
        match self {
            PauseAction::Continue => tr.pause_continue,
            PauseAction::Restart => tr.pause_restart,
            PauseAction::ToMenu => tr.pause_to_menu,
            PauseAction::Quit => tr.pause_quit,
        }
    }
}

/// The pause card, if it is up.
#[derive(Resource, Default)]
pub struct PauseMenu {
    card: Option<OpenCard>,
}

/// What an open pause card knows, none of which means anything once it is
/// put away.
#[derive(Clone, Copy, Debug)]
struct OpenCard {
    /// Whether the round was already stopped when the card was raised.
    ///
    /// A replay can be paused from its own transport before the card is
    /// opened, and a card that handed the round back running whatever it
    /// found would start the recording again with no key pressed.
    held: bool,
    /// Whether the card was raised for this player alone: a spectator's,
    /// which stops nothing, since a watcher does not get to freeze the
    /// match. The table's pause did not raise it, so the table playing on
    /// does not put it away either.
    own: bool,
    /// Whether the card offers to restart the round, decided as it opens.
    restart: bool,
    /// The row under the cursor.
    selected: usize,
}

impl OpenCard {
    fn rows(&self) -> &'static [PauseAction] {
        PauseAction::rows(self.restart)
    }

    /// The action under the cursor.
    fn chosen(&self) -> PauseAction {
        self.rows()[self.selected.min(self.rows().len() - 1)]
    }
}

impl PauseMenu {
    /// Whether the card is up.
    pub fn is_open(&self) -> bool {
        self.card.is_some()
    }

    /// Whether the card is up and the round is stopped under it, which is
    /// what the count and the music wait on. (The prompt line asks only
    /// whether a card is up: any card has the keyboard.) A
    /// spectator's own card stops nothing: the table plays on behind it,
    /// and a countdown held under it left the spectator behind the match
    /// until the host forgot them as gone.
    pub fn stops_the_round(&self) -> bool {
        self.card.is_some_and(|card| !card.own)
    }

    /// The table's card, up over a round that was running: what a test
    /// outside this module raises to see what waits on it.
    #[cfg(test)]
    pub(crate) fn raised() -> PauseMenu {
        PauseMenu {
            card: Some(OpenCard {
                held: false,
                own: false,
                restart: false,
                selected: 0,
            }),
        }
    }
}

#[derive(Component)]
pub struct PauseUi;

#[derive(Component)]
pub struct PauseRow(usize);

/// The card, titled for what it is: "paused" when the round stopped, and
/// a plain menu for a spectator, whose match plays on behind it.
fn spawn_card(commands: &mut Commands, settings: &GameSettings, own: bool, rows: usize) {
    let tr = settings.tr();
    let title = match own {
        true => tr.pause_menu_title,
        false => tr.pause_title,
    };
    commands
        .spawn((
            PauseUi,
            GlobalZIndex(menu_ui::layer::CARD),
            menu_ui::centred_overlay(),
        ))
        .with_children(|wrap| {
            wrap.spawn(menu_ui::screen_card()).with_children(|card| {
                card.spawn((
                    Text::new(title),
                    menu_ui::display_font(30.0),
                    TextColor(palette::GOLD),
                ));
                card.spawn(menu_ui::spacer(6.0));
                menu_ui::spawn_rows(card, rows, 24.0, PauseRow);
            });
        });
}

fn close(
    commands: &mut Commands,
    menu: &mut PauseMenu,
    paused: &mut Paused,
    ui: &Query<Entity, With<PauseUi>>,
) {
    // Give the round back exactly as the card found it; with no card up,
    // unfrozen.
    paused.0 = menu.card.take().is_some_and(|card| card.held);
    for entity in ui {
        commands.entity(entity).despawn();
    }
}

/// The pause card: whether it is up and where its cursor is, the freeze
/// it holds, and the commands and entities that draw it.
#[derive(bevy::ecs::system::SystemParam)]
pub struct PauseCard<'w, 's> {
    commands: Commands<'w, 's>,
    menu: ResMut<'w, PauseMenu>,
    paused: ResMut<'w, Paused>,
    ui: Query<'w, 's, Entity, With<PauseUi>>,
}

/// The ways out of a round from the card: the menu, the desktop, and the
/// same round again from its countdown.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Leaving<'w> {
    next_screen: ResMut<'w, NextState<Screen>>,
    exit: MessageWriter<'w, AppExit>,
    again: crate::app::rematch::PlayAgain<'w>,
    tournament: ResMut<'w, crate::app::tournament::Tournament>,
    vphase: Res<'w, State<crate::app::VersusPhase>>,
}

/// Open, navigate, and act on the pause card. Runs on the play screens.
pub fn pause_input(
    card: PauseCard,
    buttons: menu_ui::Buttons,
    settings: Res<GameSettings>,
    screen: Res<State<Screen>>,
    phase: Res<State<Phase>>,
    mut online: ResMut<Online>,
    leaving: Leaving,
) {
    let PauseCard {
        mut commands,
        mut menu,
        mut paused,
        ui,
    } = card;
    let keys = &*buttons.keys;
    let Leaving {
        mut next_screen,
        mut exit,
        mut again,
        mut tournament,
        vphase,
    } = leaving;
    // Pads: whatever the face's table opens the card with. Start over a
    // round, Select in a puzzle's setup where Start begins the run, and
    // nothing on a results card, whose A to the menu is `Does::Menu` and
    // not the card's. Once the card is up its own table walks it, and
    // Start or B puts it away.
    let face = &buttons.face.0;
    let pad = |does| scheme::pressed(face, does, buttons.pads.iter());
    let pad_as = |control, does| scheme::pressed_as(face, control, does, buttons.pads.iter());
    let pad_start = pad(scheme::Does::Pause) || pad(scheme::Does::OpenMenu);
    // Nor on a puzzle's won or lost card, where Esc is the way back to
    // the stage list and a pause card would open under it for a frame.
    let done = crate::app::conditions::puzzle_settled(screen.get(), phase.get());
    let escape = !done && keys.just_pressed(KeyCode::Escape);
    let pad_up = pad_as(scheme::Pad::Up, scheme::Does::Choose);
    let pad_down = pad_as(scheme::Pad::Down, scheme::Does::Choose);
    let pad_accept = pad(scheme::Does::Confirm);
    let pad_close = pad(scheme::Does::GoOn);

    // A peer's pause opens the card here too, so a frozen beach is never
    // unexplained. Their Escape is as good as ours.
    let peer_paused = online.0.as_ref().is_some_and(|s| s.session.paused());
    let Some(mut open) = menu.card else {
        if escape || pad_start || peer_paused {
            // What to hand back on the way out: a replay stopped from its
            // own transport must still be stopped when the card closes.
            let held = paused.0;
            let own = match online.0.as_mut() {
                // Online: the peers agree on a frame to stop at, and the sim
                // halts there by itself. Freezing the local ticker instead
                // would stop the network pump that carries the resume.
                Some(session) if !peer_paused => {
                    let own = session.watching();
                    session.request_pause();
                    own
                }
                Some(_) => false,
                None => {
                    paused.0 = true;
                    false
                }
            };
            let restart = *screen.get() == Screen::Versus
                && crate::app::rematch::restart_offered(
                    &again.table,
                    &online,
                    &tournament,
                    *vphase.get(),
                );
            let card = OpenCard {
                held,
                own,
                restart,
                selected: 0,
            };
            menu.card = Some(card);
            spawn_card(&mut commands, &settings, own, card.rows().len());
        }
        return;
    };

    if keys.just_pressed(KeyCode::Escape) || pad_close {
        resume(&mut online);
        close(&mut commands, &mut menu, &mut paused, &ui);
        return;
    }
    // A peer resumed: everyone plays on. A spectator's own card was never
    // the table's, and stays up until they close it. It used to be shut
    // on the very next frame, so "Esc: menu" could not reach the menu.
    if online.0.is_some() && !peer_paused && !open.own {
        close(&mut commands, &mut menu, &mut paused, &ui);
        return;
    }
    let options = open.rows().len();
    let before = open.selected;
    open.selected = menu_ui::nav(keys, open.selected, options);
    // Up wins over a same-frame down, as it always has. Read only when the
    // keys did not move the cursor already, so a pad press that ever
    // arrives as W/S as well (the menu bridge once sent the d-pad over a
    // results card) moves one row, not two: Down from Continue landed on
    // Quit.
    let pad_nav = if open.selected != before {
        menu_ui::Nav::Stay
    } else if pad_up {
        menu_ui::Nav::Up
    } else if pad_down {
        menu_ui::Nav::Down
    } else {
        menu_ui::Nav::Stay
    };
    open.selected = menu_ui::step(pad_nav, open.selected, options);
    menu.card = Some(open);
    if menu_ui::enter(keys) || pad_accept {
        match open.chosen() {
            PauseAction::Continue => {
                resume(&mut online);
                close(&mut commands, &mut menu, &mut paused, &ui);
            }
            PauseAction::Restart => {
                // Offered offline only, so there is no table to resume.
                close(&mut commands, &mut menu, &mut paused, &ui);
                let restart = crate::app::rematch::Again::Restart;
                again.ask(restart, &mut tournament, &mut next_screen);
            }
            PauseAction::ToMenu => {
                // Leaving drops the session anyway, but resume first so the
                // peers are not left frozen waiting for a player who quit.
                resume(&mut online);
                close(&mut commands, &mut menu, &mut paused, &ui);
                next_screen.set(Screen::Menu);
            }
            PauseAction::Quit => {
                // As the way to the menu does: a pause this player called
                // is lifted on the way out. The goodbye that follows hands
                // the castle over at once, but not while the round is held
                // still on purpose, so a player who paused and quit left
                // the table frozen until somebody pressed Continue.
                resume(&mut online);
                exit.write(AppExit::Success);
            }
        }
    }
}

/// Ask the table to play on, if this is an online round; offline the card
/// gives the round back itself as it closes.
fn resume(online: &mut Online) {
    if let Some(session) = online.0.as_mut() {
        session.request_resume();
    }
}

/// Style the option rows around the selection.
pub fn update_pause_rows(
    menu: Res<PauseMenu>,
    settings: Res<GameSettings>,
    mut rows: Query<(&PauseRow, &mut Text, &mut TextColor)>,
) {
    let tr = settings.tr();
    let Some(card) = menu.card else {
        return;
    };
    for (row, mut text, mut color) in &mut rows {
        let Some(action) = card.rows().get(row.0) else {
            continue;
        };
        menu_ui::paint_row(
            row.0 == card.selected,
            action.label(tr),
            &mut text,
            &mut color,
        );
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

    /// What the card asks to decide on a restart row, at their plainest:
    /// no table set up here, so no row.
    fn add_restart_sources(app: &mut App) {
        app.init_resource::<crate::app::rematch::LocalTable>();
        app.init_resource::<crate::app::match_setup::MatchConfig>();
        app.init_resource::<crate::app::tournament::Tournament>();
        if !app
            .world()
            .contains_resource::<State<crate::app::VersusPhase>>()
        {
            app.init_state::<crate::app::VersusPhase>();
        }
    }

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
        app.init_resource::<crate::app::gamepad::PadFace>();
        app.init_resource::<crate::app::gamepad::LastDevice>();
        app.init_resource::<PauseMenu>();
        app.init_resource::<Paused>();
        app.init_resource::<Online>();
        app.insert_resource(GameSettings::default());
        app.add_message::<AppExit>();
        add_restart_sources(&mut app);
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
        assert!(
            app.world().resource::<PauseMenu>().is_open(),
            "the card opened"
        );
        assert!(
            app.world().resource::<Paused>().0,
            "an offline round freezes with it"
        );

        // The card is three rows: down twice reaches the last of them.
        let row = |app: &App| {
            app.world()
                .resource::<PauseMenu>()
                .card
                .expect("the card is up")
                .chosen()
        };
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
        assert!(
            !app.world().resource::<PauseMenu>().is_open(),
            "the card closed"
        );
        assert!(!app.world().resource::<Paused>().0, "and unfroze");
        assert_eq!(*app.world().resource::<State<Screen>>().get(), Screen::Menu);
    }

    /// A pad opens and works the card by the face's table: Start over a
    /// round, Select in a puzzle's setup, where Start begins the run; then
    /// the d-pad walks it, and B puts it away as Start does.
    #[test]
    fn a_pad_works_the_card_by_its_table() {
        use crate::app::gamepad::{Face, PadFace};
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_state::<Phase>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<PadFace>();
        app.init_resource::<PauseMenu>();
        app.init_resource::<Paused>();
        app.init_resource::<Online>();
        app.insert_resource(GameSettings::default());
        app.add_message::<AppExit>();
        add_restart_sources(&mut app);
        // The face follows the card, as `read_face` has it.
        let face = |menu: Res<PauseMenu>, mut face: ResMut<PadFace>, mut under: Local<Face>| {
            if !menu.is_open() {
                *under = face.0.clone();
            }
            face.0 = match menu.is_open() {
                true => Face::Paused,
                false => under.clone(),
            };
        };
        app.add_systems(Update, (face, pause_input).chain());
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        let tap = |app: &mut App, button: GamepadButton| {
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("the pad");
            gamepad.digital_mut().clear();
            gamepad.digital_mut().press(button);
            app.update();
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("the pad");
            gamepad.digital_mut().release(button);
            gamepad.digital_mut().clear();
            app.update();
        };
        let open = |app: &App| app.world().resource::<PauseMenu>().is_open();

        app.insert_resource(State::new(Screen::Puzzle));
        app.insert_resource(PadFace(Face::PuzzleSetup {
            posts: true,
            hint: false,
        }));
        tap(&mut app, GamepadButton::Start);
        assert!(!open(&app), "Start begins the run in setup");
        tap(&mut app, GamepadButton::Select);
        assert!(open(&app), "Select opens the card there");
        tap(&mut app, GamepadButton::DPadDown);
        let row = app
            .world()
            .resource::<PauseMenu>()
            .card
            .expect("up")
            .chosen();
        assert_eq!(row, PauseAction::ToMenu);
        tap(&mut app, GamepadButton::East);
        assert!(!open(&app), "B puts it away");

        app.insert_resource(State::new(Screen::Versus));
        app.insert_resource(PadFace(Face::Round));
        tap(&mut app, GamepadButton::Start);
        assert!(open(&app), "Start opens it over a round");
        tap(&mut app, GamepadButton::Start);
        assert!(!open(&app), "and puts it away");

        // A results card's A takes its door, the menu included, and opens
        // no card under it.
        app.insert_resource(PadFace(Face::Results {
            door: crate::app::play_input::AfterRound::Menu,
            talk: false,
            rematch: None,
            keep: crate::app::replays::KeepOffer::Nothing,
        }));
        tap(&mut app, GamepadButton::South);
        assert!(!open(&app), "A on the results card is not a pause");
        // A replay's Start is the way to the menu, by the card.
        app.insert_resource(PadFace(Face::Replay));
        tap(&mut app, GamepadButton::Start);
        assert!(open(&app), "Start opens it over a replay");
    }

    /// A spectator's Escape opens a card of their own. The lockstep names
    /// no frame for a watcher, so the table never pauses, and the card used
    /// to read that as "a peer resumed" and shut itself the next frame:
    /// "Esc: menu" on the spectator's prompt led nowhere.
    #[test]
    fn a_spectators_card_stays_up() {
        use crate::app::net::OnlineSession;
        use crate::sim::{DEFAULT_DELAY, Lockstep};
        use crate::transport::{MatchTerms, UdpTransport};

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_state::<Phase>();
        app.insert_resource(State::new(Screen::Versus));
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::gamepad::PadFace>();
        app.init_resource::<crate::app::gamepad::LastDevice>();
        app.init_resource::<PauseMenu>();
        app.init_resource::<Paused>();
        app.insert_resource(GameSettings::default());
        app.add_message::<AppExit>();
        add_restart_sources(&mut app);
        let watcher = OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            Lockstep::observer(vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        assert!(watcher.watching());
        app.insert_resource(Online(Some(watcher)));
        app.add_systems(Update, pause_input);

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().resource::<PauseMenu>().is_open(), "still up");
    }

    /// The card takes the keyboard while it is up.
    ///
    /// It is an overlay and not a `Screen`, and it freezes the sim through
    /// a resource of its own, so every input system's own run condition
    /// stays true while it is open. Before `keys_are_free` gated them, the
    /// same W that walked these rows moved the board cursor behind the
    /// card, and an arrow key queued a placement that fired on resume.
    #[test]
    fn the_card_takes_the_keyboard_while_it_is_up() {
        use crate::app::conditions::keys_are_free;
        use crate::app::cursor::{Cursor, move_cursor};
        use crate::app::play_input::versus_input;
        use crate::app::{PendingActions, PlacementDenied, Sim, VersusPhase};
        use crate::sim::classic_arena;

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_state::<Phase>();
        app.init_state::<VersusPhase>();
        app.insert_resource(State::new(Screen::Versus));
        app.insert_resource(Sim(classic_arena(false, 2)));
        app.insert_resource(GameSettings::default());
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::gamepad::PadFace>();
        app.init_resource::<crate::app::gamepad::LastDevice>();
        app.init_resource::<Time>();
        app.init_resource::<PendingActions>();
        app.init_resource::<PauseMenu>();
        app.init_resource::<Paused>();
        app.init_resource::<Online>();
        app.add_message::<PlacementDenied>();
        app.add_message::<AppExit>();
        add_restart_sources(&mut app);
        app.world_mut()
            .spawn((Cursor::seated(0), Transform::default()));
        // The schedule's own conditions, so this cannot pass while the real
        // ones let the keys through.
        app.add_systems(
            Update,
            (
                pause_input,
                (move_cursor, versus_input).run_if(keys_are_free),
            )
                .chain(),
        );

        let tap = |app: &mut App, key: KeyCode| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(key);
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
        };
        let cursor = |app: &mut App| {
            let mut q = app.world_mut().query::<&Cursor>();
            let c = q.iter(app.world()).next().expect("one cursor");
            (c.x, c.y)
        };

        tap(&mut app, KeyCode::Escape);
        assert!(
            app.world().resource::<PauseMenu>().is_open(),
            "the card opened"
        );
        let parked = cursor(&mut app);

        // S walks the card's rows and nothing else.
        tap(&mut app, KeyCode::KeyS);
        assert_eq!(
            app.world()
                .resource::<PauseMenu>()
                .card
                .expect("the card is up")
                .chosen(),
            PauseAction::ToMenu
        );
        assert_eq!(cursor(&mut app), parked, "the cursor stayed put");

        // And an arrow key queues nothing for the resume to play out.
        tap(&mut app, KeyCode::ArrowUp);
        assert_eq!(
            app.world().resource::<PendingActions>().0[0],
            crate::sim::PlayerAction::None,
            "no placement waiting behind the card"
        );

        // Closing it hands the keys back.
        tap(&mut app, KeyCode::Escape);
        assert!(
            !app.world().resource::<PauseMenu>().is_open(),
            "the card closed"
        );
        tap(&mut app, KeyCode::KeyS);
        assert_ne!(cursor(&mut app), parked, "the cursor moves again");
    }

    /// On a puzzle's won or lost card Escape is the way back to the stage
    /// list, read by the card's own input; the pause card stays out of it
    /// rather than opening underneath for a frame.
    #[test]
    fn the_card_stays_shut_on_a_finished_puzzle() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_state::<Phase>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::gamepad::PadFace>();
        app.init_resource::<crate::app::gamepad::LastDevice>();
        app.init_resource::<PauseMenu>();
        app.init_resource::<Paused>();
        app.init_resource::<Online>();
        app.insert_resource(GameSettings::default());
        app.add_message::<AppExit>();
        add_restart_sources(&mut app);
        app.add_systems(Update, pause_input);
        app.insert_resource(State::new(Screen::Puzzle));
        for phase in [Phase::Won, Phase::Lost] {
            app.insert_resource(State::new(phase));
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::Escape);
            app.update();
            assert!(!app.world().resource::<PauseMenu>().is_open(), "{phase:?}");
            assert!(!app.world().resource::<Paused>().0, "{phase:?}");
        }
        // And in the running phase the same press opens it.
        app.insert_resource(State::new(Phase::Running));
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::Escape);
        app.update();
        assert!(app.world().resource::<PauseMenu>().is_open());
    }

    /// A running puzzle, with the running-phase input beside the card as
    /// the schedule has it. Read by both, the phase input froze the round
    /// and the card took that frozen state as what to hand back on
    /// Continue. One reader now; Continue unfreezes.
    #[test]
    fn continue_unfreezes_a_running_puzzle() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_state::<Phase>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::gamepad::PadFace>();
        app.init_resource::<crate::app::gamepad::LastDevice>();
        app.init_resource::<PauseMenu>();
        app.init_resource::<Paused>();
        app.init_resource::<Online>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<crate::app::cursor::LiveSeatKeys>();
        app.insert_resource(GameSettings::default());
        app.add_message::<AppExit>();
        add_restart_sources(&mut app);
        app.add_message::<crate::app::LoadLevel>();
        let levels = crate::sim::campaign_levels();
        app.insert_resource(crate::app::Sim(levels[0].board()));
        app.insert_resource(crate::app::Campaign::tide_pool_of(levels));
        app.add_systems(
            Update,
            (crate::app::play_input::running_input, pause_input).chain(),
        );
        app.insert_resource(State::new(Screen::Puzzle));
        app.insert_resource(State::new(Phase::Running));

        let tap = |app: &mut App, key: KeyCode| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(key);
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
        };
        tap(&mut app, KeyCode::Escape);
        assert!(
            app.world().resource::<PauseMenu>().is_open(),
            "the card opened"
        );
        assert!(app.world().resource::<Paused>().0, "and froze the round");
        tap(&mut app, KeyCode::Enter);
        assert!(
            !app.world().resource::<PauseMenu>().is_open(),
            "Continue closed it"
        );
        assert!(
            !app.world().resource::<Paused>().0,
            "and the round runs again"
        );
        // Escape again closes it the same way.
        tap(&mut app, KeyCode::Escape);
        assert!(app.world().resource::<PauseMenu>().is_open());
        tap(&mut app, KeyCode::Escape);
        assert!(!app.world().resource::<PauseMenu>().is_open());
        assert!(
            !app.world().resource::<Paused>().0,
            "Escape out is Continue too"
        );
    }

    /// Over a round set up at this machine the card offers to restart it,
    /// and the row does: the arena is entered again with the match armed.
    /// The daily's round, or any round not set up here, has no such row.
    #[test]
    fn a_couch_round_can_be_restarted_from_the_card() {
        for set_here in [true, false] {
            let mut app = App::new();
            app.add_plugins(bevy::state::app::StatesPlugin);
            app.init_state::<Screen>();
            app.init_state::<Phase>();
            app.insert_resource(State::new(Screen::Versus));
            app.init_resource::<ButtonInput<KeyCode>>();
            app.init_resource::<crate::app::gamepad::PadFace>();
            app.init_resource::<crate::app::gamepad::LastDevice>();
            app.init_resource::<PauseMenu>();
            app.init_resource::<Paused>();
            app.init_resource::<Online>();
            app.insert_resource(GameSettings::default());
            app.add_message::<AppExit>();
            add_restart_sources(&mut app);
            let config = crate::app::match_setup::MatchConfig::default();
            app.world_mut()
                .resource_mut::<crate::app::rematch::LocalTable>()
                .note_round(set_here, &config, &Default::default());
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
            tap(&mut app, KeyCode::Escape);
            let card = app.world().resource::<PauseMenu>().card.expect("up");
            assert_eq!(card.rows().contains(&PauseAction::Restart), set_here);
            if !set_here {
                continue;
            }
            tap(&mut app, KeyCode::KeyS);
            let card = app.world().resource::<PauseMenu>().card.expect("up");
            assert_eq!(card.chosen(), PauseAction::Restart);
            tap(&mut app, KeyCode::Enter);
            assert!(!app.world().resource::<PauseMenu>().is_open(), "closed");
            assert!(!app.world().resource::<Paused>().0, "and let go");
            assert!(
                app.world()
                    .resource::<crate::app::match_setup::MatchConfig>()
                    .armed
            );
            assert!(
                app.world()
                    .resource::<crate::app::rematch::LocalTable>()
                    .keeps_the_table()
            );
            assert!(matches!(
                *app.world().resource::<NextState<Screen>>(),
                NextState::Pending(Screen::Versus)
            ));
        }
    }
}
