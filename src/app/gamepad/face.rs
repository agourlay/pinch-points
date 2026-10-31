//! Which face of the game is up, worked out once a frame, and which hands
//! the player last reached with.
//!
//! Everything that reads a pad table asks [`PadFace`] which one is in
//! force, so the bridge pressing keys, a screen reading the pad itself and
//! the prompt naming the buttons are always reading the same table.

use super::scheme::{Face, LastDevice, PadFamily, PickList, Picks, TextBox};
use crate::app::bot_seats::BotSeats;
use crate::app::editor::EditorState;
use crate::app::lobby::{HostCard, LobbyState, Standing};
use crate::app::match_setup::MatchMenu;
use crate::app::net::Online;
use crate::app::pause::PauseMenu;
use crate::app::seating::Viewer;
use crate::app::settings::GameSettings;
use crate::app::spectators::{PickCard, SpectatorCard, SpectatorChat};
use crate::app::{Campaign, Phase, Playback, Screen, Sim, VersusPhase};
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;

/// The face of the game that is up: see [`Face`].
#[derive(Resource, Clone, PartialEq, Eq, Debug, Default)]
pub struct PadFace(pub Face);

/// Where the game stands on the screens with modes of their own.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Modes<'w> {
    screen: Res<'w, State<Screen>>,
    phase: Res<'w, State<Phase>>,
    vphase: Res<'w, State<VersusPhase>>,
    pause: Res<'w, PauseMenu>,
    lobby: Res<'w, LobbyState>,
    match_menu: Res<'w, MatchMenu>,
    editor: Res<'w, EditorState>,
    bots: Res<'w, BotSeats>,
    quit: Res<'w, crate::app::menu_scene::QuitCard>,
    stages: Res<'w, crate::app::stage_select::StageList>,
    level_card: Res<'w, crate::app::menu_scene::LevelCard>,
    settings_menu: Res<'w, crate::app::settings::screen::SettingsMenu>,
}

/// What a round on the board is, and who is looking at it.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Board<'w> {
    online: Res<'w, Online>,
    playback: Res<'w, Playback>,
    chat: Res<'w, SpectatorChat>,
    vote_card: Res<'w, SpectatorCard>,
    pick_card: Res<'w, PickCard>,
    campaign: Res<'w, Campaign>,
    /// Whether two are seated at the stage list, which says where a won
    /// card's A goes ([`Campaign::won_door`]).
    coop: Res<'w, crate::app::Coop>,
    sim: Res<'w, Sim>,
    hints: Res<'w, crate::app::hint::Hints>,
    tournament: Res<'w, crate::app::tournament::Tournament>,
    table: Res<'w, crate::app::rematch::LocalTable>,
    /// Whether the results card has a round to keep.
    keep: Res<'w, crate::app::replays::RoundToKeep>,
}

/// Everything [`read_face`] decides the face from.
#[derive(bevy::ecs::system::SystemParam)]
pub struct FaceSources<'w, 's> {
    modes: Modes<'w>,
    board: Board<'w>,
    settings: Res<'w, GameSettings>,
    pads: Query<'w, 's, (Entity, &'static Gamepad)>,
    seats: Res<'w, super::PadSeats>,
    config: Res<'w, crate::app::match_setup::MatchConfig>,
    beaches: Res<'w, crate::app::match_setup::CustomBeaches>,
    last_pad: Res<'w, LastPad>,
}

impl FaceSources<'_, '_> {
    /// The face of the game as it stands: the one function that says.
    fn face(&self) -> Face {
        let Self {
            modes,
            board,
            settings,
            pads,
            seats,
            config,
            beaches,
            last_pad,
        } = self;
        let tr = settings.tr();
        // The map page's cursor on a card a pick takes, which A is offered on.
        let map_pick =
            || crate::app::match_setup::card_to_pick(&modes.match_menu, config, beaches).is_some();
        let door = || {
            modes
                .bots
                .door
                .as_ref()
                .filter(|_| modes.bots.door_for.card_up())
                .map(|door| Face::Door {
                    lan: door.showing_lan,
                })
        };
        match modes.screen.get() {
            Screen::Menu => menu_face(&modes.quit, &modes.level_card),
            Screen::Settings => Face::Settings {
                dial: crate::app::settings::screen::Row::ALL[modes.settings_menu.selected]
                    .is_dial(),
            },
            Screen::Controls => Face::Controls,
            Screen::Achievements => Face::Achievements,
            Screen::Language => Face::Language,
            Screen::NewVersion => Face::NewVersion,
            Screen::Replays => Face::Replays,
            Screen::Interlude => Face::Interlude {
                keep: board.keep.offer(),
            },
            // A stage of the player's own can be edited and its code
            // copied, which a shipped one cannot.
            Screen::StageSelect => Face::StageSelect {
                custom: crate::app::stage_select::is_custom(&board.campaign, modes.stages.selected),
            },
            Screen::MatchSetup => door().unwrap_or_else(|| match modes.match_menu.naming {
                Some(_) => Face::Typing(TextBox::Keeps),
                None => Face::MatchSetup {
                    step: modes.match_menu.step,
                    join: {
                        let live: Vec<Entity> = pads.iter().map(|(entity, _)| entity).collect();
                        seats.start_joins_here(&live, settings, &config.human_seats(), last_pad.0)
                    },
                    partners: crate::app::match_setup::setup_teams(settings, config)
                        != crate::app::teams::TeamMode::Solo,
                },
            }),
            Screen::Lobby => {
                door().unwrap_or_else(|| lobby_face(&modes.lobby, &modes.bots, tr, map_pick()))
            }
            Screen::Editor => {
                use crate::app::editor::Mode;
                match &modes.editor.mode {
                    Mode::Naming { .. } => Face::Typing(TextBox::Cancels),
                    Mode::Levels(_) => Face::LevelList,
                    Mode::Card(_) => Face::EditorCard,
                    Mode::Testing(test) => Face::Playtest(test.phase),
                    Mode::Painting | Mode::JustClosed => Face::Editor {
                        layer: editor_layer(seats, pads, settings),
                    },
                }
            }
            Screen::Puzzle if modes.pause.is_open() => Face::Paused,
            Screen::Puzzle => {
                let level = board.campaign.current();
                let hint = board.hints.offered();
                match modes.phase.get() {
                    Phase::Setup => Face::PuzzleSetup {
                        posts: level.posts > 0,
                        hint,
                    },
                    Phase::Running => Face::PuzzleRunning,
                    Phase::Won => Face::PuzzleWon {
                        door: board.campaign.won_door(*board.coop),
                    },
                    Phase::Lost => Face::PuzzleLost { hint },
                }
            }
            // A spectator's line open under a peer's pause keeps the keys,
            // a pad's A included, until it is said or dropped.
            Screen::Versus if modes.pause.is_open() && !board.chat.open() => Face::Paused,
            // The host's map page for the next round of a series.
            Screen::Versus if modes.match_menu.host_page_up() => Face::NextMap { pick: map_pick() },
            Screen::Versus => versus_face(board, *modes.vphase.get(), tr),
        }
    }
}

/// Whether the editor's second layer is up: Select held on the pad that
/// drives the editor, the one its cursor answers to (P1's, as
/// `pad_move_cursor` deals it). Any pad's Select used to raise it, so a
/// friend's pad resting with Select down turned the author's A from paint
/// into save.
fn editor_layer(
    seats: &super::PadSeats,
    pads: &Query<(Entity, &Gamepad)>,
    settings: &GameSettings,
) -> bool {
    seats
        .pad_for(pads, settings, &[0], 0)
        .is_some_and(|pad| pad.pressed(GamepadButton::Select))
}

/// The main menu's face: its quit card's or a pasted level's card's while
/// one is up.
pub fn menu_face(
    quit: &crate::app::menu_scene::QuitCard,
    level_card: &crate::app::menu_scene::LevelCard,
) -> Face {
    match (quit.open, level_card.is_open()) {
        (true, _) => Face::Quitting,
        (false, true) => Face::LevelCard,
        (false, false) => Face::Menu,
    }
}

/// The lobby's face: a box being typed in, or where the player stands.
/// `map_pick` whether the map page's cursor is on a card a pick takes.
pub fn lobby_face(
    lobby: &LobbyState,
    bots: &BotSeats,
    tr: &crate::app::i18n::Tr,
    map_pick: bool,
) -> Face {
    if let Some(open) = &lobby.typing {
        return Face::Typing(match open.what {
            crate::app::lobby::Entry::Chat => TextBox::Chat,
            crate::app::lobby::Entry::PlayerName
            | crate::app::lobby::Entry::GameName
            | crate::app::lobby::Entry::Address => TextBox::Cancels,
        });
    }
    match lobby.standing() {
        Standing::Choosing { watching } => Face::Browse {
            hosts: !lobby.hosts.is_empty(),
            watching: *watching,
        },
        Standing::Hosting(hosted) => match hosted.card() {
            HostCard::Picking => Face::HostMap { pick: map_pick },
            HostCard::Kicking(_) => Face::Kicking {
                picks: Picks {
                    list: PickList::Kick,
                    rows: crate::app::lobby::kick_rows(hosted, bots, tr),
                },
                pages: crate::app::lobby::kick_pages(&hosted.peers) > 1,
            },
            HostCard::Partnering => Face::Card(Picks {
                list: PickList::Partners,
                rows: crate::app::lobby::partner_rows(lobby, tr),
            }),
            HostCard::Table => Face::Hosting {
                partners: crate::app::lobby::partners_open(lobby),
            },
        },
        Standing::Joining(_) => Face::AtBeach,
    }
}

/// A versus round's results card's face: where A goes, whether Y talks to
/// the table (a spectator's) or plays it again, and whether X keeps the
/// round. The one answer the bridge presses by and the card's own line of
/// keys is written from.
pub fn results_face(
    online: &Online,
    series_on: bool,
    rematch: Option<crate::app::rematch::Rematch>,
    keep: crate::app::replays::KeepOffer,
) -> Face {
    Face::Results {
        door: crate::app::play_input::after_round(online, series_on),
        talk: Viewer::of(online, false) == Viewer::Spectator,
        rematch,
        keep,
    }
}

/// A versus round's face: its results, a recording, a spectator's view of
/// somebody else's round, or a round being played.
fn versus_face(board: &Board, vphase: VersusPhase, tr: &crate::app::i18n::Tr) -> Face {
    let online = &*board.online;
    let spectating = Viewer::of(online, false) == Viewer::Spectator;
    if spectating && board.chat.open() {
        return Face::Typing(TextBox::Cancels);
    }
    if vphase == VersusPhase::Over {
        return results_face(
            online,
            board.tournament.is_running(),
            crate::app::rematch::rematch(&board.table, online, &board.tournament),
            board.keep.offer(),
        );
    }
    // The header calls a recording under a live session a replay, and so
    // does this (`hud::text::header_viewer`).
    if board.playback.0.is_some() {
        return Face::Replay;
    }
    if !spectating {
        return Face::Round;
    }
    let Some(session) = &online.0 else {
        return Face::Round;
    };
    let running = vphase == VersusPhase::Running;
    if board.vote_card.0 {
        return Face::Card(Picks {
            list: PickList::Vote,
            rows: crate::app::spectators::SPECTATOR_EVENTS
                .iter()
                .enumerate()
                .map(|(at, event)| (at as u8 + 1, tr.events[event.index()].to_string()))
                .collect(),
        });
    }
    if board.pick_card.0 {
        return Face::Card(Picks {
            list: PickList::Winner,
            rows: (0..session.seats)
                .map(|seat| {
                    (
                        seat + 1,
                        crate::app::name_or_label(&session.names, tr, seat),
                    )
                })
                .collect(),
        });
    }
    Face::Watching {
        vote: running && board.sim.0.events_enabled(),
        pick: running && session.stands.winner.picks.open > 0,
    }
}

/// The pads, and the face whose table says what their buttons do: what a
/// system that reads the pad itself asks which button means what.
#[derive(bevy::ecs::system::SystemParam)]
pub struct PadsOnFace<'w, 's> {
    pub face: Res<'w, PadFace>,
    pub pads: Query<'w, 's, &'static Gamepad>,
}

impl PadsOnFace<'_, '_> {
    /// Whether any pad pressed the button that does `does` on this face.
    pub fn pressed(&self, does: super::scheme::Does) -> bool {
        super::scheme::pressed(&self.face.0, does, self.pads.iter())
    }
}

/// Work out the face for this frame. Run before the bridge, which presses
/// its keys, and again before the prompt is written, so a face that
/// changed during the frame is named the frame it changed.
pub fn read_face(sources: FaceSources, mut face: ResMut<PadFace>) {
    let now = sources.face();
    if face.0 != now {
        face.0 = now;
    }
}

/// Note which hands the player last reached with: any key on the keyboard,
/// or any pad button, or a stick pushed out past the deadzone.
///
/// Only the keyboard's own presses count, read off its messages: the pad
/// bridge presses keys into the input resource and sends none, so a pad
/// on a menu stays a pad.
///
/// A stick counts on the frame it crosses the deadzone, as the bridge's
/// flick does, and not on every frame it is held out there: a stick that
/// drifts, or a thumb resting on it, held the prompts on the pad's
/// buttons however many keys were typed.
///
/// Not while a couch shares the board ([`a_couch_shares_the_board`]). A
/// table can have a keyboard and pads at once, and the prompt there would
/// flip with every press, coming back each time from the fade that takes
/// it off the beach six seconds in. The board keeps the hands the table
/// started it with.
pub fn track_last_device(
    mut keyboard: MessageReader<KeyboardInput>,
    pads: Query<(Entity, &Gamepad)>,
    settings: Res<GameSettings>,
    face: Res<PadFace>,
    cursors: Query<&crate::app::cursor::Cursor>,
    (mut last, mut last_pad): (ResMut<LastDevice>, ResMut<LastPad>),
    mut pushed: Local<Vec<Entity>>,
) {
    let typed = keyboard
        .read()
        .any(|press| press.state == ButtonState::Pressed);
    let deadzone = settings.deadzone();
    // Read every frame, the shared board included, so a stick already out
    // when the board is left does not count as a push the frame after.
    let mut flicked = false;
    let mut reached = None;
    for (entity, pad) in &pads {
        let out = [
            GamepadAxis::LeftStickX,
            GamepadAxis::LeftStickY,
            GamepadAxis::RightStickX,
            GamepadAxis::RightStickY,
        ]
        .iter()
        .any(|&axis| pad.get(axis).is_some_and(|v| v.abs() > deadzone));
        let was = pushed.contains(&entity);
        flicked |= out && !was;
        if (out && !was) || pad.get_just_pressed().next().is_some() {
            reached = Some(entity);
        }
        match (out, was) {
            (true, false) => pushed.push(entity),
            (false, true) => pushed.retain(|&held| held != entity),
            _ => {}
        }
    }
    pushed.retain(|&held| pads.contains(held));
    if a_couch_shares_the_board(&face.0, cursors.iter().count()) {
        return;
    }
    let pressed = flicked
        || pads
            .iter()
            .any(|(_, pad)| pad.get_just_pressed().next().is_some());
    // Named as the pad pressed has its buttons printed.
    let family = reached
        .and_then(|entity| pads.get(entity).ok())
        .map(|(_, pad)| PadFamily::of(pad))
        .unwrap_or_default();
    let now = match (typed, pressed) {
        (_, true) => LastDevice::Pad(family),
        (true, false) => LastDevice::Keyboard,
        (false, false) => return,
    };
    if *last != now {
        *last = now;
    }
    if reached.is_some() && last_pad.0 != reached {
        last_pad.0 = reached;
    }
}

/// Which pad the hands last reached with, when [`LastDevice`] says a pad:
/// counted by the same presses and frozen by the same rule
/// ([`track_last_device`]). Match setup opens with it as P1's pad
/// ([`crate::app::gamepad::PadSeats::setup_opened`]).
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LastPad(pub Option<Entity>);

/// Whose labels a line names the buttons by when it names them whatever
/// the hands are on (a reference card, read at a keyboard too): the pad's
/// in hand, else the pad's last reached with, else Xbox's. At the
/// keyboard [`LastDevice`] has no family to tell.
pub fn reference_family(last: LastDevice, pad: LastPad, pads: &Query<&Gamepad>) -> PadFamily {
    match last {
        LastDevice::Pad(family) => family,
        LastDevice::Keyboard => pad
            .0
            .and_then(|entity| pads.get(entity).ok())
            .map(PadFamily::of)
            .unwrap_or_default(),
    }
}

/// The pad working the setup as it opens: the pad last reached with when
/// the hands last used are a pad's, else none, the keyboard's.
pub fn hands_at_the_menu(last: LastDevice, pad: LastPad) -> Option<Entity> {
    match last {
        LastDevice::Pad(_) => pad.0,
        LastDevice::Keyboard => None,
    }
}

/// Whether more than one pair of hands here plays the board on `face`,
/// with `cursors` the cursors on it: a versus round, which may seat a
/// keyboard and pads together, or a puzzle played in co-op, where P2 is a
/// second cursor. The prompt there keeps the hands it began with.
fn a_couch_shares_the_board(face: &Face, cursors: usize) -> bool {
    match face {
        Face::Round => true,
        Face::PuzzleSetup { .. } | Face::PuzzleRunning => cursors > 1,
        Face::Menu
        | Face::Quitting
        | Face::Settings { .. }
        | Face::Controls
        | Face::MatchSetup { .. }
        | Face::StageSelect { .. }
        | Face::Replays
        | Face::Achievements
        | Face::Language
        | Face::NewVersion
        | Face::Interlude { .. }
        | Face::Door { .. }
        | Face::Typing(_)
        | Face::Browse { .. }
        | Face::Hosting { .. }
        | Face::HostMap { .. }
        | Face::NextMap { .. }
        | Face::Kicking { .. }
        | Face::AtBeach
        | Face::PuzzleWon { .. }
        | Face::PuzzleLost { .. }
        | Face::Paused
        | Face::Replay
        | Face::Watching { .. }
        | Face::Card(_)
        | Face::Results { .. }
        | Face::Editor { .. }
        | Face::LevelList
        | Face::EditorCard
        | Face::LevelCard
        | Face::Playtest(_) => false,
    }
}

/// `PINCH_PAD_PROMPTS`: start with the controller's prompts up, for a
/// screenshot of them; the first key pressed puts the keyboard's back.
/// `PINCH_PADS` plugs pads in, the first of them the hands last used
/// under `PINCH_PAD_PROMPTS`; `PINCH_QUIT_CARD` raises the menu's quit
/// card.
pub fn pad_prompts_from_env(
    mut commands: Commands,
    (mut last, mut last_pad): (ResMut<LastDevice>, ResMut<LastPad>),
    mut quit: ResMut<crate::app::menu_scene::QuitCard>,
) {
    let pads: Vec<Entity> = (0..crate::app::dev::virtual_pads())
        .map(|_| commands.spawn(Gamepad::default()).id())
        .collect();
    if crate::app::dev::pad_prompts() {
        *last = LastDevice::Pad(crate::app::dev::pad_family());
        last_pad.0 = pads.first().copied();
    }
    // `PINCH_QUIT_CARD`: the card a pad's Start raises, up from the start.
    quit.open = crate::app::dev::quit_card();
}

/// One of every face the game can show, for the tests that walk them all.
#[cfg(test)]
pub fn every_face() -> Vec<Face> {
    use crate::app::play_input::AfterRound;
    let picks = |list| Picks {
        list,
        rows: vec![(2, "Ann".to_string()), (7, "Bob".to_string())],
    };
    let mut faces = vec![
        Face::Menu,
        Face::Quitting,
        Face::Settings { dial: false },
        Face::Settings { dial: true },
        Face::Controls,
        Face::Replays,
        Face::Achievements,
        Face::Language,
        Face::NewVersion,
        Face::Hosting { partners: false },
        Face::Hosting { partners: true },
        Face::HostMap { pick: true },
        Face::HostMap { pick: false },
        Face::NextMap { pick: true },
        Face::NextMap { pick: false },
        Face::AtBeach,
        Face::PuzzleRunning,
        Face::Paused,
        Face::Round,
        Face::Replay,
        Face::StageSelect { custom: false },
        Face::StageSelect { custom: true },
        Face::LevelList,
        Face::EditorCard,
        Face::LevelCard,
    ];
    for join in [false, true] {
        for partners in [false, true] {
            faces.extend(
                crate::app::match_setup::Step::ALL.map(|step| Face::MatchSetup {
                    step,
                    join,
                    partners,
                }),
            );
        }
    }
    faces.extend(crate::app::editor::TestPhase::ALL.map(Face::Playtest));
    faces.extend(crate::app::campaign::WonDoor::ALL.map(|door| Face::PuzzleWon { door }));
    for on in [false, true] {
        faces.extend([
            Face::Door { lan: on },
            Face::PuzzleLost { hint: on },
            Face::Editor { layer: on },
            Face::Kicking {
                picks: picks(PickList::Kick),
                pages: on,
            },
        ]);
        for other in [false, true] {
            faces.extend([
                Face::Browse {
                    hosts: on,
                    watching: other,
                },
                Face::PuzzleSetup {
                    posts: on,
                    hint: other,
                },
                Face::Watching {
                    vote: on,
                    pick: other,
                },
            ]);
        }
    }
    for ends in [TextBox::Chat, TextBox::Cancels, TextBox::Keeps] {
        faces.push(Face::Typing(ends));
    }
    for list in [PickList::Vote, PickList::Winner] {
        faces.push(Face::Card(picks(list)));
    }
    use crate::app::rematch::Rematch;
    use crate::app::replays::KeepOffer;
    for keep in [KeepOffer::Nothing, KeepOffer::Offered, KeepOffer::Kept] {
        faces.push(Face::Interlude { keep });
    }
    for door in [
        AfterRound::NextRound,
        AfterRound::NextMap,
        AfterRound::Lobby,
        AfterRound::Menu,
    ] {
        for keep in [KeepOffer::Nothing, KeepOffer::Offered, KeepOffer::Kept] {
            for talk in [false, true] {
                faces.push(Face::Results {
                    door,
                    talk,
                    rematch: None,
                    keep,
                });
            }
            for again in [Rematch::Round, Rematch::Series, Rematch::SeriesAsked] {
                faces.push(Face::Results {
                    door,
                    talk: false,
                    rematch: Some(again),
                    keep,
                });
            }
        }
    }
    faces
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::cursor::Cursor;
    use bevy::input::keyboard::Key;

    fn hands() -> App {
        let mut app = App::new();
        app.init_resource::<PadFace>();
        app.init_resource::<LastDevice>();
        app.init_resource::<LastPad>();
        app.insert_resource(GameSettings::default());
        app.add_message::<KeyboardInput>();
        app.world_mut().spawn(Gamepad::default());
        app.add_systems(Update, track_last_device);
        app
    }

    fn type_a_key(app: &mut App) {
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyQ,
            logical_key: Key::Character("q".into()),
            state: ButtonState::Pressed,
            text: Some("q".into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }

    fn press_a_button(app: &mut App) {
        let pad = app
            .world_mut()
            .query_filtered::<Entity, With<Gamepad>>()
            .single(app.world())
            .expect("one pad");
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("the pad");
        gamepad.digital_mut().clear();
        gamepad.digital_mut().press(GamepadButton::South);
        app.update();
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("the pad");
        gamepad.digital_mut().release(GamepadButton::South);
        gamepad.digital_mut().clear();
    }

    /// The editor's second layer is up while Select is held on the pad
    /// that drives the editor, and not for another pad's Select: a
    /// friend's pad resting with Select down turned the author's A from
    /// paint into save.
    #[test]
    fn only_the_editors_pad_raises_its_second_layer() {
        #[derive(Resource, Default)]
        struct Layer(bool, Option<Entity>);
        fn read(
            seats: Res<super::super::PadSeats>,
            pads: Query<(Entity, &Gamepad)>,
            settings: Res<GameSettings>,
            mut layer: ResMut<Layer>,
        ) {
            layer.0 = editor_layer(&seats, &pads, &settings);
            layer.1 = seats.pad_entity_for(&pads, &settings, &[0], 0);
        }
        let mut app = App::new();
        app.insert_resource(GameSettings::default());
        app.init_resource::<super::super::PadSeats>();
        app.init_resource::<Layer>();
        let one = app.world_mut().spawn(Gamepad::default()).id();
        let two = app.world_mut().spawn(Gamepad::default()).id();
        app.add_systems(Update, read);
        app.update();
        let driver = app.world().resource::<Layer>().1.expect("a pad drives");
        let other = if driver == one { two } else { one };
        let hold = |app: &mut App, pad: Entity, down: bool| {
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("pad");
            match down {
                true => gamepad.digital_mut().press(GamepadButton::Select),
                false => gamepad.digital_mut().release(GamepadButton::Select),
            }
            app.update();
            app.world().resource::<Layer>().0
        };
        assert!(!hold(&mut app, other, true), "another pad's Select");
        assert!(hold(&mut app, driver, true), "the editor's own pad");
        assert!(!hold(&mut app, driver, false));
    }

    /// The map page's A says what it does where it is up: over the lobby
    /// it starts the match, and between the rounds of a series it calls
    /// the next round, as the keyboard's prompt there says of Enter. One
    /// face for both said "start the match" between rounds.
    #[test]
    fn the_map_pages_a_says_what_it_does_there() {
        use crate::app::gamepad::scheme::legend;
        use crate::app::i18n::EN;
        let between = legend(
            &EN,
            crate::app::gamepad::PadFamily::Xbox,
            &Face::NextMap { pick: true },
        );
        assert!(between.contains(EN.pad.next_round), "{between}");
        assert!(!between.contains(EN.pad.start_match), "{between}");
        assert!(between.contains(EN.pad.back), "{between}");
        let lobby = legend(
            &EN,
            crate::app::gamepad::PadFamily::Xbox,
            &Face::HostMap { pick: true },
        );
        assert!(lobby.contains(EN.pad.start_match), "{lobby}");
        // On a card A picks nothing from, A is not offered.
        for face in [Face::HostMap { pick: false }, Face::NextMap { pick: false }] {
            let said = legend(&EN, crate::app::gamepad::PadFamily::Xbox, &face);
            assert!(!said.contains(EN.pad.start_match), "{said}");
            assert!(!said.contains(EN.pad.next_round), "{said}");
            assert!(said.contains(EN.pad.back), "{said}");
        }
    }

    /// A key on the keyboard and a button on a pad each make their own
    /// the hands the prompts are written for, and nothing else does.
    #[test]
    fn the_last_hands_used_are_the_ones_named() {
        let mut app = hands();
        let last = |app: &App| *app.world().resource::<LastDevice>();
        app.update();
        assert_eq!(last(&app), LastDevice::Keyboard, "a keyboard to begin");
        press_a_button(&mut app);
        assert_eq!(last(&app), LastDevice::PAD);
        app.update();
        assert_eq!(last(&app), LastDevice::PAD, "and a pad until a key");
        type_a_key(&mut app);
        assert_eq!(last(&app), LastDevice::Keyboard);
    }

    /// The hands on a pad carry its maker's labels, told by the vendor id
    /// it was plugged in with: a DualSense's prompts name ✕, not A.
    #[test]
    fn a_pad_is_named_by_its_makers_labels() {
        use bevy::input::gamepad::{
            GamepadConnection, GamepadConnectionEvent, gamepad_connection_system,
        };
        let mut app = hands();
        app.add_message::<GamepadConnectionEvent>();
        app.add_systems(PreUpdate, gamepad_connection_system);
        let pad = app
            .world_mut()
            .query_filtered::<Entity, With<Gamepad>>()
            .single(app.world())
            .expect("one pad");
        app.world_mut().write_message(GamepadConnectionEvent::new(
            pad,
            GamepadConnection::Connected {
                name: "DualSense Wireless Controller".into(),
                vendor_id: Some(0x054C),
                product_id: Some(0x0CE6),
            },
        ));
        app.update();
        press_a_button(&mut app);
        let last = *app.world().resource::<LastDevice>();
        assert_eq!(last, LastDevice::Pad(PadFamily::DualSense));
        assert!(
            super::super::scheme::legend(&crate::app::i18n::EN, last.family(), &Face::Quitting)
                .starts_with("✕: quit"),
        );
    }

    /// A key the bridge pressed for a pad is not the keyboard: the bridge
    /// sends no keyboard message, and a pad on a menu stays a pad.
    #[test]
    fn a_bridged_key_is_not_the_keyboard() {
        let mut app = hands();
        app.init_resource::<ButtonInput<KeyCode>>();
        press_a_button(&mut app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert_eq!(*app.world().resource::<LastDevice>(), LastDevice::PAD);
    }

    /// A versus round keeps the hands it began with, so a couch with a
    /// keyboard and pads is not shown its controls line again at every
    /// press, flipping between the two.
    #[test]
    fn a_round_keeps_the_hands_it_began_with() {
        let mut app = hands();
        app.insert_resource(PadFace(Face::Round));
        press_a_button(&mut app);
        assert_eq!(*app.world().resource::<LastDevice>(), LastDevice::Keyboard);
        app.insert_resource(PadFace(Face::Paused));
        press_a_button(&mut app);
        assert_eq!(*app.world().resource::<LastDevice>(), LastDevice::PAD);
    }

    /// A puzzle in co-op is a shared board as a round is: P2 on a pad
    /// beside P1 at the keys does not flip the prompt. Alone at it, the
    /// last hands used still name the controls.
    #[test]
    fn a_coop_puzzle_keeps_the_hands_it_began_with() {
        let mut app = hands();
        let last = |app: &App| *app.world().resource::<LastDevice>();
        for face in [
            Face::PuzzleSetup {
                posts: true,
                hint: false,
            },
            Face::PuzzleRunning,
        ] {
            app.insert_resource(PadFace(face.clone()));
            app.insert_resource(LastDevice::Keyboard);
            let one = app.world_mut().spawn(Cursor::seated(0)).id();
            press_a_button(&mut app);
            assert_eq!(last(&app), LastDevice::PAD, "alone: {face:?}");
            app.insert_resource(LastDevice::Keyboard);
            let two = app.world_mut().spawn(Cursor::seated(1)).id();
            press_a_button(&mut app);
            assert_eq!(last(&app), LastDevice::Keyboard, "co-op: {face:?}");
            app.world_mut().despawn(one);
            app.world_mut().despawn(two);
        }
    }

    /// A stick counts when it is pushed out past the deadzone, not for as
    /// long as it stays there: one that drifts, or a thumb resting on it,
    /// leaves the next key typed its say.
    #[test]
    fn a_stick_held_out_counts_once() {
        let mut app = hands();
        let last = |app: &App| *app.world().resource::<LastDevice>();
        let pad = app
            .world_mut()
            .query_filtered::<Entity, With<Gamepad>>()
            .single(app.world())
            .expect("one pad");
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("the pad");
        gamepad.analog_mut().set(GamepadAxis::LeftStickX, 0.9);
        app.update();
        assert_eq!(last(&app), LastDevice::PAD, "the push counts");
        type_a_key(&mut app);
        assert_eq!(last(&app), LastDevice::Keyboard, "the key, over the drift");
        app.update();
        assert_eq!(last(&app), LastDevice::Keyboard, "and stays");
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("the pad");
        gamepad.analog_mut().set(GamepadAxis::LeftStickX, 0.0);
        app.update();
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("the pad");
        gamepad.analog_mut().set(GamepadAxis::LeftStickX, -0.9);
        app.update();
        assert_eq!(last(&app), LastDevice::PAD, "a fresh push counts again");
    }
}
