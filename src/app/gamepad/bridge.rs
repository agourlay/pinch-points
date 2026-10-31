//! The pad as a keyboard on the menus: each face's table
//! ([`scheme::binds`]) names the key a pad control stands for, and
//! the bridge presses it into the keyboard input resource and lets go of
//! it again, so every screen keeps a single input path.

use super::face::PadFace;
use super::scheme::{self, Key, Pad, PadPick};
use crate::app::settings::GameSettings;
use crate::sim::Direction;
use bevy::prelude::*;

/// Where each pad's right stick pointed last frame, for its flicks.
type Flicks = Vec<(Entity, Option<Direction>)>;

/// Press the keys the face's table names for this frame's pad presses.
///
/// Runs on every screen, and the table decides: a face whose rows are all
/// read off the pad by the screen itself (a round, the pause card) has no
/// key to press, so a pad in a round is never read as a seat's keyboard.
/// The exceptions the menus need are rows of their tables, each with its
/// reason beside it: B on the landing menu is not Escape, which there
/// leaves the game; the lobby's d-pad is the arrows, since W is its watch
/// toggle; the results cards over a board leave the d-pad alone, since the
/// board cursor reads W/A/S/D as P1's keys; and on match setup Select is
/// Enter while Start sits a friend down.
///
/// A results card that comes up over a round being played presses nothing
/// for its first moments ([`CardGrace`]).
pub fn pad_menu_bridge(
    pads: Query<(Entity, &Gamepad)>,
    (face, time): (Res<PadFace>, Res<Time>),
    (caps, settings): (Res<crate::app::keycaps::KeyCaps>, Res<GameSettings>),
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut bridged: ResMut<Bridged>,
    mut pick: ResMut<PadPick>,
    (mut flicked, mut grace): (Local<Flicks>, Local<CardGrace>),
) {
    let face = &face.0;
    let rows = scheme::binds(face);
    let waiting = grace.waiting(face, time.elapsed_secs());
    let deadzone = settings.deadzone();
    for (entity, pad) in &pads {
        // The stick has no press of its own: a flick is a push this frame
        // from a stick at rest last frame. Pushed one way and rolled round
        // to the next without coming back to the middle, it is still the
        // one flick: read as a new push each side it rolled through, an
        // undo flicked left through up-left resized the beach.
        let now = scheme::flick(pad, deadzone);
        let was = match flicked.iter_mut().find(|(e, _)| *e == entity) {
            Some((_, was)) => std::mem::replace(was, now),
            None => {
                flicked.push((entity, now));
                None
            }
        };
        if waiting {
            continue;
        }
        for row in &rows {
            let just = if let Pad::Flick(dir) = row.pad {
                now == Some(dir) && was.is_none()
            } else {
                // The cursor's Move has no button, and is never pressed.
                row.pad
                    .button()
                    .is_some_and(|button| pad.just_pressed(button))
            };
            if !just {
                continue;
            }
            if let Key::Step(nav) = row.key {
                if let Some(picks) = scheme::picks(face) {
                    pick.step(picks, nav);
                }
                continue;
            }
            for key in keys_for(row.key, face, &pick, &caps, &settings) {
                keys.press(key);
                if !bridged.0.contains(&(key, row.pad)) {
                    bridged.0.push((key, row.pad));
                }
            }
        }
    }
    flicked.retain(|(entity, _)| pads.contains(*entity));
}

/// How long a results card that came up over a round being played lets a
/// pad's presses go by, in seconds.
const CARD_GRACE: f32 = 0.5;

/// The pause a results card takes before a pad's buttons mean anything
/// on it, when it came up over a round being played.
///
/// A, B, X and Y place arrows in a round, and the tide comes in mid-rush:
/// taps already on their way landed on the card's first frames as its
/// keys, A walking out of the card (and out of an online series) before
/// anyone had read it, Y starting a single round over, and X keeping the
/// round. The keyboard has no such clash, since no seat key means anything
/// on the card, and is not held back. A card brought up any other way (a
/// spectator's, a recording's) needs no pause.
///
/// The pause card over a round being played still counts as playing: a
/// pause called in the round's last moments is dropped as the tide comes
/// in (`drop_a_late_pause`), and the card follows it with no round frame
/// between.
#[derive(Default)]
pub struct CardGrace {
    /// The face last frame was a round being played, or the pause card
    /// over one.
    playing: bool,
    /// Until when the card's pause lasts, in elapsed seconds.
    until: f32,
}

impl CardGrace {
    /// Whether the pad's presses on `face` are let go by this frame.
    fn waiting(&mut self, face: &scheme::Face, now: f32) -> bool {
        let card = matches!(face, scheme::Face::Results { .. });
        if card && self.playing {
            self.until = now + CARD_GRACE;
        }
        self.playing =
            *face == scheme::Face::Round || (*face == scheme::Face::Paused && self.playing);
        card && now < self.until
    }
}

/// The keys a row of `face`'s table presses, as this keyboard and these
/// settings spell them: none for a row the screen reads off the pad, or
/// one that only walks a pick. What the bridge presses, and what it keeps
/// holding while the face still has the row.
fn keys_for(
    key: Key,
    face: &scheme::Face,
    pick: &PadPick,
    caps: &crate::app::keycaps::KeyCaps,
    settings: &GameSettings,
) -> Vec<KeyCode> {
    match key {
        Key::Direct | Key::Layer | Key::Step(_) => vec![],
        Key::Pick => scheme::picks(face)
            .and_then(|picks| scheme::pick_key(picks, pick))
            .into_iter()
            .collect(),
        Key::Code(key) => vec![key],
        Key::Shifted(key) => vec![KeyCode::ShiftLeft, key],
        Key::Cap(letter) => caps.key_for(letter).into_iter().collect(),
        Key::Wall(dir) => crate::app::editor::wall_key(settings, dir)
            .into_iter()
            .collect(),
        Key::Command(command) => vec![crate::app::editor::command_key(settings, command)],
        Key::ShiftedCommand(command) => vec![
            KeyCode::ShiftLeft,
            crate::app::editor::command_key(settings, command),
        ],
    }
}

/// The keys the bridge is holding down for a pad, each with the control
/// holding it, to let go of when the control comes up.
#[derive(Resource, Default)]
pub struct Bridged(Vec<(KeyCode, Pad)>);

impl Bridged {
    /// Whether `key` is down because a pad's button is, not a key. What a
    /// reader that wants the keyboard itself (a key capture) leaves out.
    pub fn holds(&self, key: KeyCode) -> bool {
        self.0.iter().any(|&(held, _)| held == key)
    }
}

/// Let go of a bridged key once no pad is holding its control, or once
/// the face in force no longer has that control pressing that key, on
/// every screen.
///
/// The press is what the face decides, and so is how long it is held.
/// A press that changes the screen is let go of on the screen it opened,
/// often one whose table has no row for that control (A on the stage list
/// opens a puzzle in setup), and a release left to the button coming up
/// kept the key down there: the d-pad held through the stage list's W
/// steered P1's cursor across the puzzle, which reads W as P1's key for as
/// long as it is down. The bridge presses only on a button's own press,
/// so a control still held on the new face is not pressed again there.
///
/// Asked as "is any pad still holding it" rather than "did a button just
/// come up", because a pad unplugged mid-press never reports the release:
/// its held d-pad left W down, and the next round's cursor slid up on its
/// own. Only keys the bridge pressed are let go, so a keyboard player's
/// own held W is not, and a key two controls are holding stays down until
/// both are up.
pub fn pad_bridge_release(
    pads: Query<&Gamepad>,
    face: Res<PadFace>,
    (caps, settings): (Res<crate::app::keycaps::KeyCaps>, Res<GameSettings>),
    pick: Res<PadPick>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut bridged: ResMut<Bridged>,
) {
    if bridged.0.is_empty() {
        return;
    }
    let deadzone = settings.deadzone();
    let rows = scheme::binds(&face.0);
    let held = |control: Pad| pads.iter().any(|pad| control.held(pad, deadzone));
    let bound = |key: KeyCode, control: Pad| {
        rows.iter()
            .filter(|row| row.pad == control)
            .any(|row| keys_for(row.key, &face.0, &pick, &caps, &settings).contains(&key))
    };
    let kept: Vec<(KeyCode, Pad)> = bridged
        .0
        .iter()
        .copied()
        .filter(|&(key, control)| held(control) && bound(key, control))
        .collect();
    for &(key, _) in &bridged.0 {
        if !kept.iter().any(|&(still, _)| still == key) {
            keys.release(key);
        }
    }
    bridged.0 = kept;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::gamepad::face::every_face;
    use crate::app::gamepad::scheme::{Face, PickList, Picks, binds};
    use crate::app::play_input::AfterRound;

    /// One pad on `face`, with the bridge wired up and nothing else.
    fn bridged(face: Face) -> App {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<Bridged>();
        app.init_resource::<PadPick>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<Time>();
        app.insert_resource(GameSettings::default());
        app.insert_resource(PadFace(face));
        app.world_mut().spawn(Gamepad::default());
        app.add_systems(Update, (pad_menu_bridge, pad_bridge_release).chain());
        app
    }

    fn the_pad(app: &mut App) -> Entity {
        app.world_mut()
            .query_filtered::<Entity, With<Gamepad>>()
            .single(app.world())
            .expect("one pad")
    }

    /// Hold `control` down on the one pad (or let it go), and run a frame.
    /// Last frame's presses are spent first, as the input plugin spends
    /// them between frames.
    fn hold(app: &mut App, control: Pad, down: bool) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        let pad = the_pad(app);
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("the pad");
        gamepad.digital_mut().clear();
        if let Pad::Flick(dir) = control {
            let push = if down { 1.0 } else { 0.0 };
            let (axis, value) = match dir {
                Direction::Up => (GamepadAxis::RightStickY, push),
                Direction::Down => (GamepadAxis::RightStickY, -push),
                Direction::Left => (GamepadAxis::RightStickX, -push),
                Direction::Right => (GamepadAxis::RightStickX, push),
            };
            gamepad.analog_mut().set(axis, value);
        } else {
            let button = control.button().expect("a button");
            match down {
                true => gamepad.digital_mut().press(button),
                false => gamepad.digital_mut().release(button),
            }
        }
        app.update();
    }

    fn press(app: &mut App, control: Pad) {
        hold(app, control, true);
    }

    fn keys(app: &App) -> &ButtonInput<KeyCode> {
        app.world().resource::<ButtonInput<KeyCode>>()
    }

    /// Every control a pad has that the bridge could read.
    const CONTROLS: [Pad; 19] = [
        Pad::A,
        Pad::B,
        Pad::X,
        Pad::Y,
        Pad::LB,
        Pad::RB,
        Pad::LT,
        Pad::RT,
        Pad::Start,
        Pad::Select,
        Pad::RS,
        Pad::Up,
        Pad::Down,
        Pad::Left,
        Pad::Right,
        Pad::Flick(Direction::Up),
        Pad::Flick(Direction::Down),
        Pad::Flick(Direction::Left),
        Pad::Flick(Direction::Right),
    ];

    /// The keys pressing `control` on `face` presses, on a fresh pad.
    fn pressed_by(face: &Face, control: Pad) -> Vec<KeyCode> {
        let mut app = bridged(face.clone());
        press(&mut app, control);
        let mut keys: Vec<KeyCode> = keys(&app).get_just_pressed().copied().collect();
        keys.sort_by_key(|key| format!("{key:?}"));
        keys
    }

    /// The keys a row of the table stands for, as a fresh pad on `face`
    /// with the stock keyboard and settings would press them.
    fn keys_of(face: &Face, key: Key) -> Vec<KeyCode> {
        let caps = crate::app::keycaps::KeyCaps::default();
        match key {
            Key::Direct | Key::Layer | Key::Step(_) => vec![],
            Key::Code(code) => vec![code],
            Key::Shifted(code) => vec![KeyCode::ShiftLeft, code],
            Key::Cap(letter) => caps.key_for(letter).into_iter().collect(),
            Key::Wall(dir) => crate::app::editor::wall_key(&GameSettings::default(), dir)
                .into_iter()
                .collect(),
            Key::Command(command) => vec![crate::app::editor::command_key(
                &GameSettings::default(),
                command,
            )],
            Key::ShiftedCommand(command) => vec![
                KeyCode::ShiftLeft,
                crate::app::editor::command_key(&GameSettings::default(), command),
            ],
            Key::Pick => scheme::picks(face)
                .and_then(|picks| scheme::pick_key(picks, &PadPick::default()))
                .into_iter()
                .collect(),
        }
    }

    /// On every face, each control presses exactly the keys its row of
    /// the table names and nothing else: no key from a control the table
    /// leaves out, none from a row the screen reads off the pad itself,
    /// and so none at all in a round. And the prompt names every row: the
    /// legend is written from the same table, so a button it names is a
    /// button the bridge or the screen answers.
    #[test]
    fn every_face_presses_exactly_the_keys_its_table_names() {
        for face in every_face() {
            let rows = binds(&face);
            for control in CONTROLS {
                let mut wanted: Vec<KeyCode> = rows
                    .iter()
                    .filter(|row| row.pad == control)
                    .flat_map(|row| keys_of(&face, row.key))
                    .collect();
                wanted.sort_by_key(|key| format!("{key:?}"));
                assert_eq!(
                    pressed_by(&face, control),
                    wanted,
                    "{control:?} on {face:?}"
                );
            }
            let tr = &crate::app::i18n::EN;
            let legend = scheme::legend(tr, crate::app::gamepad::PadFamily::Xbox, &face);
            for row in &rows {
                assert!(
                    legend.contains(row.pad.name(tr, crate::app::gamepad::PadFamily::Xbox))
                        && legend.contains(row.does.label(tr)),
                    "{face:?}: the prompt {legend:?} leaves out {row:?}"
                );
            }
        }
    }

    /// Every keyboard action of every screen is reachable from a pad: the
    /// keys each screen's handlers read, by face, against the keys its
    /// table presses or the pad reads itself. Text entry is the one thing
    /// left to the keyboard, which on a Steam Deck is Steam's own.
    #[test]
    fn every_screen_action_has_a_pad_button() {
        use KeyCode::*;
        let reach = |faces: &[Face]| -> Vec<KeyCode> {
            faces
                .iter()
                .flat_map(|face| {
                    binds(face)
                        .into_iter()
                        .flat_map(move |row| keys_of(face, row.key))
                })
                .collect()
        };
        let editor = [Face::Editor { layer: false }, Face::Editor { layer: true }];
        let picks = |list| Picks {
            list,
            rows: vec![(3, "Cleo".to_string())],
        };
        let screens: Vec<(&str, Vec<Face>, Vec<KeyCode>)> = vec![
            (
                "menu",
                vec![Face::Menu],
                vec![KeyW, KeyS, Enter, KeyV, KeyM],
            ),
            (
                "settings",
                vec![Face::Settings { dial: false }],
                vec![KeyW, KeyS, KeyA, KeyD, Enter, Escape],
            ),
            (
                "controls",
                vec![Face::Controls],
                vec![KeyW, KeyS, KeyA, KeyD, Enter, Backspace, Escape],
            ),
            (
                "match setup",
                crate::app::match_setup::Step::ALL
                    .map(|step| Face::MatchSetup {
                        step,
                        join: true,
                        partners: false,
                    })
                    .to_vec(),
                vec![KeyW, KeyS, KeyA, KeyD, Tab, Enter, Escape],
            ),
            // Under a team mode step 3's P picks partners.
            (
                "match setup in teams",
                vec![Face::MatchSetup {
                    step: crate::app::match_setup::Step::Players,
                    join: true,
                    partners: true,
                }],
                vec![KeyW, KeyS, KeyA, KeyD, Tab, Enter, Escape, KeyP],
            ),
            (
                "stage select",
                vec![Face::StageSelect { custom: false }],
                vec![KeyW, KeyS, KeyA, KeyD, Enter, KeyC, Escape],
            ),
            (
                "replays",
                vec![Face::Replays],
                vec![KeyW, KeyS, Enter, KeyC, KeyV, KeyD, Escape],
            ),
            (
                "achievements",
                vec![Face::Achievements],
                vec![KeyW, KeyS, Escape],
            ),
            ("language", vec![Face::Language], vec![KeyW, KeyS, Enter]),
            (
                "new version",
                vec![Face::NewVersion],
                vec![KeyW, KeyS, Enter, Escape],
            ),
            (
                "interlude",
                vec![Face::Interlude {
                    keep: crate::app::replays::KeepOffer::Offered,
                }],
                vec![Enter, KeyG],
            ),
            (
                "bot door",
                vec![Face::Door { lan: false }],
                vec![KeyC, KeyL, Escape],
            ),
            (
                "lobby",
                vec![
                    Face::Browse {
                        hosts: true,
                        watching: false,
                    },
                    Face::Hosting { partners: true },
                    Face::Kicking {
                        picks: picks(PickList::Kick),
                        pages: true,
                    },
                    Face::AtBeach,
                    Face::Typing(scheme::TextBox::Chat),
                ],
                vec![
                    ArrowUp, ArrowDown, ArrowLeft, ArrowRight, Enter, KeyB, KeyH, KeyJ, KeyW, KeyK,
                    KeyP, Digit3, Tab, Escape,
                ],
            ),
            (
                "puzzle cards",
                vec![
                    Face::PuzzleWon {
                        door: crate::app::campaign::WonDoor::Next,
                    },
                    Face::PuzzleLost { hint: true },
                ],
                vec![Enter, KeyR, Escape],
            ),
            (
                "spectating",
                vec![
                    Face::Watching {
                        vote: true,
                        pick: true,
                    },
                    Face::Card(picks(PickList::Vote)),
                    Face::Typing(scheme::TextBox::Cancels),
                ],
                vec![KeyT, KeyE, KeyP, Digit3, Enter, Escape],
            ),
            (
                "results",
                vec![Face::Results {
                    door: AfterRound::Lobby,
                    talk: true,
                    rematch: None,
                    keep: crate::app::replays::KeepOffer::Offered,
                }],
                vec![Enter, KeyT, KeyG],
            ),
            (
                "couch results",
                vec![Face::Results {
                    door: AfterRound::Menu,
                    talk: false,
                    rematch: Some(crate::app::rematch::Rematch::Round),
                    keep: crate::app::replays::KeepOffer::Offered,
                }],
                vec![Enter, KeyR, KeyG],
            ),
            // K and O are the keyboard's short cuts to two of the settings
            // card's rows, which a pad reaches by choosing the row (N) and
            // turning it (- and =).
            (
                "editor",
                editor.to_vec(),
                vec![
                    Space, Tab, ShiftLeft, Minus, Equal, KeyV, KeyN, KeyT, KeyZ, Enter, Escape,
                    ArrowUp, ArrowDown, ArrowLeft, ArrowRight, F1, F2, F3, F4, F5, F6,
                ],
            ),
        ];
        for (screen, faces, wanted) in screens {
            let reached = reach(&faces);
            for key in wanted {
                assert!(
                    reached.contains(&key),
                    "{screen}: no pad button for {key:?}"
                );
            }
        }
        // The faces read off the pad by the screen itself: every action
        // the screen's keys have, the table has a row for.
        use scheme::Does;
        let direct: Vec<(Face, Vec<Does>)> = vec![
            (
                Face::PuzzleSetup {
                    posts: true,
                    hint: true,
                },
                vec![
                    Does::Move,
                    Does::Place(Direction::Up),
                    Does::Place(Direction::Down),
                    Does::Place(Direction::Left),
                    Does::Place(Direction::Right),
                    Does::Remove,
                    Does::Clear,
                    Does::Start,
                    Does::PrevLevel,
                    Does::NextLevel,
                    Does::Hint,
                    Does::OpenMenu,
                ],
            ),
            (Face::PuzzleRunning, vec![Does::Reset, Does::Pause]),
            (
                Face::Round,
                vec![Does::Move, Does::Remove, Does::Clear, Does::Pause],
            ),
            (Face::Paused, vec![Does::Choose, Does::Confirm, Does::GoOn]),
            (
                Face::Replay,
                vec![Does::PlayPause, Does::Speed, Does::OpenMenu],
            ),
            (
                Face::Playtest(crate::app::editor::TestPhase::Setup),
                vec![Does::Move, Does::Remove, Does::Start, Does::EndTest],
            ),
            (
                Face::Playtest(crate::app::editor::TestPhase::Running),
                vec![Does::Reset, Does::EndTest],
            ),
            (
                Face::Playtest(crate::app::editor::TestPhase::Match),
                vec![Does::Move, Does::Remove, Does::Reset, Does::EndTest],
            ),
            (Face::Hosting { partners: false }, vec![Does::Talk]),
            (
                Face::MatchSetup {
                    step: crate::app::match_setup::Step::Players,
                    join: true,
                    partners: false,
                },
                vec![Does::Join],
            ),
        ];
        for (face, wanted) in direct {
            let rows = binds(&face);
            for does in wanted {
                assert!(
                    rows.iter().any(|row| row.does == does),
                    "{face:?}: no pad button for {does:?}"
                );
            }
        }
    }

    /// On a results card over the board the d-pad is not W/A/S/D, which
    /// the board cursor reads as P1's keys; A is still Enter.
    #[test]
    fn the_dpad_does_not_steer_the_board_from_a_results_card() {
        let mut app = bridged(Face::Results {
            door: AfterRound::Menu,
            talk: false,
            rematch: None,
            keep: crate::app::replays::KeepOffer::Nothing,
        });
        press(&mut app, Pad::Up);
        press(&mut app, Pad::Left);
        assert!(!keys(&app).pressed(KeyCode::KeyW), "Up is not P1's W");
        assert!(!keys(&app).pressed(KeyCode::KeyA), "Left is not P1's A");
        press(&mut app, Pad::A);
        assert!(
            keys(&app).just_pressed(KeyCode::Enter),
            "A still takes the card's door"
        );
        let mut app = bridged(Face::PuzzleWon {
            door: crate::app::campaign::WonDoor::Next,
        });
        press(&mut app, Pad::Down);
        assert!(!keys(&app).pressed(KeyCode::KeyS), "nor on a puzzle's");
    }

    /// A results card that comes up over a round lets the pad's taps meant
    /// for arrows go by, and answers a press made once it has been read.
    #[test]
    fn a_card_over_a_round_lets_the_rush_taps_go_by() {
        let card = Face::Results {
            door: AfterRound::Menu,
            talk: false,
            rematch: Some(crate::app::rematch::Rematch::Round),
            keep: crate::app::replays::KeepOffer::Offered,
        };
        let mut app = bridged(Face::Round);
        app.update();
        app.insert_resource(PadFace(card));
        for control in [Pad::A, Pad::Y, Pad::X] {
            press(&mut app, control);
            assert!(
                keys(&app).get_just_pressed().next().is_none(),
                "{control:?} tapped as the tide came in is not the card's"
            );
            hold(&mut app, control, false);
        }
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(CARD_GRACE));
        press(&mut app, Pad::A);
        assert!(
            keys(&app).just_pressed(KeyCode::Enter),
            "a press once the card is read takes its door"
        );
    }

    /// A pause called as the tide comes in is dropped for the results
    /// card, which follows the pause card with no round frame between: a
    /// tap meant for an arrow still goes by.
    #[test]
    fn a_card_after_a_late_pause_lets_the_rush_taps_go_by() {
        let card = Face::Results {
            door: AfterRound::Menu,
            talk: false,
            rematch: Some(crate::app::rematch::Rematch::Round),
            keep: crate::app::replays::KeepOffer::Offered,
        };
        let mut app = bridged(Face::Round);
        app.update();
        app.insert_resource(PadFace(Face::Paused));
        app.update();
        app.insert_resource(PadFace(card));
        press(&mut app, Pad::A);
        assert!(
            keys(&app).get_just_pressed().next().is_none(),
            "A tapped as the pause dropped is not the card's"
        );
    }

    /// B on the landing menu must not reach Escape, because Escape there
    /// is `AppExit` and nothing asks first: a player reaching for "back"
    /// would close the game.
    #[test]
    fn b_does_not_quit_from_the_menu() {
        let mut app = bridged(Face::Menu);
        press(&mut app, Pad::B);
        assert!(
            !keys(&app).pressed(KeyCode::Escape),
            "B on the menu must not press the key that leaves the game"
        );
    }

    /// Everywhere else Escape means back, which is what B should do, so
    /// the menu's exception must not cost the pad its way out of a
    /// sub-screen.
    #[test]
    fn b_still_goes_back_from_a_sub_screen() {
        for face in [
            Face::Settings { dial: true },
            Face::Controls,
            Face::StageSelect { custom: false },
        ] {
            let mut app = bridged(face.clone());
            press(&mut app, Pad::B);
            assert!(
                keys(&app).pressed(KeyCode::Escape),
                "B is the way back from {face:?}"
            );
        }
    }

    /// Leaving a sub-screen with B lands on the menu, where B is nothing.
    /// Swallowing the *release* too would leave the pressed Escape held
    /// for ever, and `ButtonInput::press` only reports `just_pressed` for
    /// a key it was not already holding.
    #[test]
    fn a_press_that_changes_screen_still_releases_its_key() {
        let mut app = bridged(Face::Settings { dial: true });
        press(&mut app, Pad::B);
        assert!(keys(&app).just_pressed(KeyCode::Escape));
        // The screen it asked for arrives, and only then does the finger
        // come off the button.
        app.insert_resource(PadFace(Face::Menu));
        hold(&mut app, Pad::B, false);
        assert!(
            !keys(&app).pressed(KeyCode::Escape),
            "the pressed Escape must not be left held on the menu"
        );
    }

    /// The same when the face opened has nothing on that button at all: A
    /// on the stage list opens a puzzle in setup, and the Enter it left
    /// held there swallowed the next press of Enter on the won card.
    #[test]
    fn a_press_that_leaves_the_table_behind_still_releases_its_key() {
        let mut app = bridged(Face::StageSelect { custom: false });
        press(&mut app, Pad::A);
        assert!(keys(&app).just_pressed(KeyCode::Enter));
        app.insert_resource(PadFace(Face::PuzzleSetup {
            posts: true,
            hint: false,
        }));
        hold(&mut app, Pad::A, false);
        assert!(
            !keys(&app).pressed(KeyCode::Enter),
            "let go of on a face that presses nothing"
        );
    }

    /// The d-pad held from the stage list into a puzzle's setup lets go of
    /// the W it pressed there, though the thumb is still on it: the puzzle
    /// reads the d-pad off the pad, and W held down is P1's cursor walking
    /// up the board on its own. Still held, it is not pressed again; a
    /// face that binds the same control to the same key keeps it down.
    #[test]
    fn a_dpad_held_into_a_face_without_it_lets_go() {
        let mut app = bridged(Face::StageSelect { custom: false });
        press(&mut app, Pad::Up);
        assert!(keys(&app).pressed(KeyCode::KeyW));
        app.insert_resource(PadFace(Face::PuzzleSetup {
            posts: true,
            hint: false,
        }));
        hold(&mut app, Pad::Up, true);
        assert!(!keys(&app).pressed(KeyCode::KeyW), "let go in the puzzle");
        hold(&mut app, Pad::Up, true);
        assert!(!keys(&app).pressed(KeyCode::KeyW), "and not pressed again");

        let mut app = bridged(Face::Menu);
        press(&mut app, Pad::Down);
        app.insert_resource(PadFace(Face::Settings { dial: true }));
        hold(&mut app, Pad::Down, true);
        assert!(keys(&app).pressed(KeyCode::KeyS), "S is S on both faces");
    }

    /// A pad unplugged with a button down never reports the release, and
    /// the key the bridge held for it is let go all the same.
    #[test]
    fn an_unplugged_pad_lets_go_of_its_key() {
        let mut app = bridged(Face::StageSelect { custom: false });
        press(&mut app, Pad::A);
        let pad = the_pad(&mut app);
        app.world_mut().entity_mut(pad).remove::<Gamepad>();
        app.update();
        assert!(!keys(&app).pressed(KeyCode::Enter));
    }

    /// On step 3 of match setup A is Enter, which starts the match, as A
    /// goes on from steps 1 and 2; Start presses nothing, its join or start
    /// being read off the pad itself (`pad_claim_seats`), and the d-pad
    /// still walks the seats. Select is Enter nowhere in the menus.
    #[test]
    fn a_on_the_setup_screen_starts_the_match() {
        use crate::app::match_setup::Step;
        let setup = Face::MatchSetup {
            step: Step::Players,
            join: true,
            partners: false,
        };
        assert_eq!(pressed_by(&setup, Pad::A), vec![KeyCode::Enter]);
        assert_eq!(pressed_by(&setup, Pad::Select), vec![]);
        assert_eq!(pressed_by(&setup, Pad::Start), vec![], "Start is a seat's");
        assert_eq!(pressed_by(&setup, Pad::Down), vec![KeyCode::KeyS]);
        assert_eq!(pressed_by(&setup, Pad::X), vec![KeyCode::Tab]);
        assert_eq!(
            pressed_by(&Face::Settings { dial: true }, Pad::Select),
            vec![]
        );
        for step in [Step::Game, Step::Map] {
            let face = Face::MatchSetup {
                step,
                join: true,
                partners: false,
            };
            assert_eq!(pressed_by(&face, Pad::A), vec![KeyCode::Enter], "{step:?}");
            assert_eq!(pressed_by(&face, Pad::B), vec![KeyCode::Escape], "{step:?}");
            assert_eq!(pressed_by(&face, Pad::Start), vec![], "{step:?}");
            assert_eq!(
                pressed_by(&face, Pad::Right),
                vec![KeyCode::KeyD],
                "{step:?}"
            );
        }
    }

    /// In the lobby the d-pad is the arrows, which walk the beach list,
    /// and never W, which there turns a player into a spectator.
    #[test]
    fn the_lobby_dpad_is_the_arrows() {
        let browse = Face::Browse {
            hosts: true,
            watching: false,
        };
        assert_eq!(pressed_by(&browse, Pad::Up), vec![KeyCode::ArrowUp]);
        assert_eq!(pressed_by(&browse, Pad::Down), vec![KeyCode::ArrowDown]);
        for face in [browse, Face::Hosting { partners: true }] {
            for control in [Pad::Up, Pad::Down, Pad::Left, Pad::Right] {
                assert!(
                    !pressed_by(&face, control).contains(&KeyCode::KeyW),
                    "{control:?} must not toggle watching on {face:?}"
                );
            }
        }
    }

    /// Up pressed on the menu and let go in the lobby lets go of the
    /// menu's W, rather than of the lobby's arrow it never pressed.
    #[test]
    fn a_dpad_press_carried_into_the_lobby_is_released() {
        let mut app = bridged(Face::Menu);
        press(&mut app, Pad::Up);
        assert!(keys(&app).pressed(KeyCode::KeyW));
        app.insert_resource(PadFace(Face::Browse {
            hosts: true,
            watching: false,
        }));
        hold(&mut app, Pad::Up, false);
        assert!(
            !keys(&app).pressed(KeyCode::KeyW),
            "the W pressed on the menu must not be left held in the lobby"
        );
    }

    /// A list the keyboard picks from by number, the pad walks and picks
    /// with A: the number of the row the pick stands on.
    #[test]
    fn the_pad_picks_a_numbered_row() {
        let mut app = bridged(Face::Kicking {
            picks: Picks {
                list: PickList::Kick,
                rows: vec![(2, "Ann".to_string()), (7, "Bob".to_string())],
            },
            pages: false,
        });
        press(&mut app, Pad::Down);
        assert!(
            keys(&app).get_just_pressed().next().is_none(),
            "a step presses nothing"
        );
        hold(&mut app, Pad::Down, false);
        press(&mut app, Pad::A);
        assert!(keys(&app).just_pressed(KeyCode::Digit7), "Bob's number");
    }

    /// A stick rolled from one side round to the next without coming back
    /// to the middle is one flick, not one for each side: under Select, an
    /// undo flicked left that rolled through up-left resized the beach.
    #[test]
    fn a_stick_rolled_round_is_one_flick() {
        let mut app = bridged(Face::Editor { layer: true });
        let undo = crate::app::editor::command_key(
            &GameSettings::default(),
            crate::app::editor::Command::Undo,
        );
        let stick = |app: &mut App, x: f32, y: f32| {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
            let pad = the_pad(app);
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("the pad");
            gamepad.analog_mut().set(GamepadAxis::RightStickX, x);
            gamepad.analog_mut().set(GamepadAxis::RightStickY, y);
            app.update();
        };
        stick(&mut app, -1.0, 0.0);
        assert!(keys(&app).just_pressed(undo), "the flick left");
        stick(&mut app, -0.6, 0.8);
        assert!(
            !keys(&app).just_pressed(KeyCode::F5),
            "rolled to up: no resize"
        );
        stick(&mut app, 0.0, 0.0);
        stick(&mut app, 0.0, 1.0);
        assert!(
            keys(&app).just_pressed(KeyCode::F5),
            "from rest, it is a push up"
        );
    }

    /// The editor's walls are the right stick pushed toward the side, one
    /// toggle a push; Shift goes down with Tab for the brush before.
    #[test]
    fn the_editor_flicks_walls_and_steps_brushes_back() {
        let mut app = bridged(Face::Editor { layer: false });
        press(&mut app, Pad::Flick(Direction::Right));
        assert!(keys(&app).just_pressed(KeyCode::ArrowRight));
        // Held over, it is not pushed again.
        hold(&mut app, Pad::Flick(Direction::Right), true);
        assert!(!keys(&app).just_pressed(KeyCode::ArrowRight));
        hold(&mut app, Pad::Flick(Direction::Right), false);
        assert!(!keys(&app).pressed(KeyCode::ArrowRight), "let go");
        press(&mut app, Pad::LB);
        assert!(keys(&app).pressed(KeyCode::ShiftLeft) && keys(&app).just_pressed(KeyCode::Tab));
        hold(&mut app, Pad::LB, false);
        assert!(
            !keys(&app).pressed(KeyCode::ShiftLeft),
            "Shift goes with it"
        );
    }
}
