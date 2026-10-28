//! Gamepad input (spec §8.1). The face buttons form a diamond of four
//! directions, so placement maps them directly: top places Up, bottom Down,
//! left Left, right Right. D-pad or left stick moves the cursor, LB removes,
//! RB clears own signposts, Start pauses. What every other button does, on
//! every screen, is in [`scheme`]'s tables.
//!
//! Pads fill the human seats **from P2 upwards, P1 last**, so every
//! two-player setup works without configuration: one pad drives P2 beside
//! a P1 keyboard, two pads drive P2 and P1, and no pads leaves both
//! keyboard layouts. A setup worked with a pad makes that pad P1's, so a
//! Steam Deck alone plays P1 against the AI. On the setup screen a pad's
//! Start claims the next seat above P1 and keeps it (see [`PadSeats`]). The keyboard always
//! stays usable for seats 1-2; both feed the identical commit paths as
//! §8.2.

mod bridge;
pub mod face;
mod rumble;
pub mod scheme;
mod seats;

pub use bridge::{Bridged, pad_bridge_release, pad_menu_bridge};
pub use face::{
    LastPad, PadFace, hands_at_the_menu, pad_prompts_from_env, read_face, track_last_device,
};
pub use rumble::rumble_on_raid;
pub use scheme::{Face, LastDevice, PadPick};
pub use seats::{
    PadSeats, SeatPad, follow_the_setup_hands, forget_the_pilot, keep_pad_order,
    let_a_lone_players_claims_go, let_claims_go, pad_claim_seats,
};

use crate::app::cursor::{Cursor, HoldRepeat};
use crate::app::play_input::{SeatCommand, apply_setup, queue_versus};
use crate::app::settings::GameSettings;
use crate::app::{PendingActions, Phase, PlacementDenied, Sim};
use bevy::prelude::*;

/// The pads by the seats they drive, on the face their buttons are read
/// by: for a system outside this module that places for each cursor with
/// the pad that moves it ([`pad_move_cursor`]), as [`pad_setup_input`]
/// does.
#[derive(bevy::ecs::system::SystemParam)]
pub struct SeatPads<'w, 's> {
    pads: Query<'w, 's, (Entity, &'static Gamepad)>,
    face: Res<'w, PadFace>,
    seats: Res<'w, PadSeats>,
}

impl SeatPads<'_, '_> {
    /// Whether the pad `player`'s cursor answers to, among the seats with
    /// a cursor (`players`), pressed the button that does `does` on this
    /// face.
    pub fn pressed(
        &self,
        settings: &GameSettings,
        players: &[u8],
        player: u8,
        does: scheme::Does,
    ) -> bool {
        let players = seats::seated(players.iter().copied());
        self.seats
            .pad_for(&self.pads, settings, &players, player)
            .is_some_and(|pad| scheme::pressed_on(&self.face.0, does, pad))
    }
}

/// Per-cursor hold-to-repeat for held pad directions, and whether a
/// direction was held last frame, which makes this frame's a fresh press
/// or not: the stick has no `just_pressed` of its own.
#[derive(Component, Default)]
pub struct PadRepeat {
    repeat: HoldRepeat,
    held: bool,
}

const DPAD: [GamepadButton; 4] = [
    GamepadButton::DPadUp,
    GamepadButton::DPadDown,
    GamepadButton::DPadLeft,
    GamepadButton::DPadRight,
];

/// Whether this frame's direction is a fresh press: one from rest, or a
/// d-pad button just pressed while another direction was held. The
/// second is what a second move key does on the keyboard, stepping at
/// once, and rocking the d-pad across is one too. Not the stick's second
/// axis coming on, which a thumb held off-axis near the deadzone does
/// over and over: each would step at once and restart the delay.
fn fresh_press(was_held: bool, held: bool, dpad_just: bool) -> bool {
    held && (!was_held || dpad_just)
}

/// The pad's fresh presses on `face` that place and remove, which mean
/// the same in setup and in versus: the rows of its table that do. Clear
/// and start are read by each mode.
fn pad_commands<'a>(face: &'a Face, pad: &'a Gamepad) -> impl Iterator<Item = SeatCommand> + 'a {
    scheme::binds(face)
        .into_iter()
        .filter(|row| row.pad.button().is_some_and(|b| pad.just_pressed(b)))
        .filter_map(|row| {
            if let scheme::Does::Place(dir) = row.does {
                return Some(SeatCommand::Place(dir));
            }
            (row.does == scheme::Does::Remove).then_some(SeatCommand::Remove)
        })
}

fn held_direction(pad: &Gamepad, deadzone: f32) -> (i16, i16) {
    let mut dx = 0i16;
    let mut dy = 0i16;
    if pad.pressed(GamepadButton::DPadUp) {
        dy -= 1;
    }
    if pad.pressed(GamepadButton::DPadDown) {
        dy += 1;
    }
    if pad.pressed(GamepadButton::DPadLeft) {
        dx -= 1;
    }
    if pad.pressed(GamepadButton::DPadRight) {
        dx += 1;
    }
    let stick_x = pad.get(GamepadAxis::LeftStickX).unwrap_or(0.0);
    let stick_y = pad.get(GamepadAxis::LeftStickY).unwrap_or(0.0);
    if stick_x < -deadzone {
        dx -= 1;
    }
    if stick_x > deadzone {
        dx += 1;
    }
    // Stick Y is up-positive; board y grows downward.
    if stick_y > deadzone {
        dy -= 1;
    }
    if stick_y < -deadzone {
        dy += 1;
    }
    (dx.signum(), dy.signum())
}

/// Move cursors from pads (any screen with cursors). Gamepad N → player N.
pub fn pad_move_cursor(
    pads: Query<(Entity, &Gamepad)>,
    seats: Res<PadSeats>,
    time: Res<Time>,
    sim: Res<Sim>,
    settings: Res<GameSettings>,
    mut commands: Commands,
    mut cursors: Query<(Entity, &mut Cursor, Option<&mut PadRepeat>)>,
) {
    let board = &sim.0;
    let deadzone = settings.deadzone();
    let players = seats::seated(cursors.iter().map(|(_, c, ..)| c.player));
    for (entity, mut cursor, repeat) in &mut cursors {
        let Some(pad) = seats.pad_for(&pads, &settings, &players, cursor.player) else {
            continue;
        };
        let (dx, dy) = held_direction(pad, deadzone);
        let Some(mut repeat) = repeat else {
            commands.entity(entity).insert(PadRepeat::default());
            continue;
        };
        let held = dx != 0 || dy != 0;
        let dpad_just = DPAD.iter().any(|&button| pad.just_pressed(button));
        let just = fresh_press(repeat.held, held, dpad_just);
        repeat.held = held;
        if repeat.repeat.advance(
            just,
            held,
            time.delta(),
            settings.repeat_delay,
            settings.repeat_interval,
        ) {
            cursor.step(board, dx, dy);
        }
    }
}

/// Versus commits from pads, through the same one-action-per-tick queue the
/// keyboard uses.
pub fn pad_versus_input(
    pads: Query<(Entity, &Gamepad)>,
    (face, seats): (Res<PadFace>, Res<PadSeats>),
    settings: Res<GameSettings>,
    sim: Res<Sim>,
    mut pending: ResMut<PendingActions>,
    mut denied: MessageWriter<PlacementDenied>,
    mut cursors: Query<&mut Cursor>,
) {
    let board = &sim.0;
    let players = seats::seated(cursors.iter().map(|c| c.player));
    for mut cursor in &mut cursors {
        let Some(pad) = seats.pad_for(&pads, &settings, &players, cursor.player) else {
            continue;
        };
        // RB clears held, as the keyboard's clear-all does: one removal
        // a tick for as long as it is down.
        let commands = pad_commands(&face.0, pad).chain(
            scheme::held_on(&face.0, scheme::Does::Clear, pad).then_some(SeatCommand::ClearAll),
        );
        for command in commands {
            queue_versus(command, &mut cursor, board, &mut pending, &mut denied);
        }
    }
}

/// Puzzle setup commits from P1's pad: place/remove directly, Start begins
/// the run (mirrors the keyboard Enter), as the setup's table says. P1's pad is whichever one
/// [`PadSeats::pad_for`] hands the seat (the one it named in settings, or the
/// first pad plugged in), the same answer `pad_move_cursor` gives, so the
/// pad that moves the cursor is the pad that places.
pub fn pad_setup_input(
    pads: Query<(Entity, &Gamepad)>,
    (face, seats): (Res<PadFace>, Res<PadSeats>),
    settings: Res<GameSettings>,
    mut sim: ResMut<Sim>,
    mut next_phase: ResMut<NextState<Phase>>,
    mut denied: MessageWriter<PlacementDenied>,
    mut cursors: Query<&mut Cursor>,
) {
    let players = seats::seated(cursors.iter().map(|c| c.player));
    // Each cursor answers to its own pad, and in co-op both place the one
    // seat's arrows (see `Coop`).
    for mut cursor in &mut cursors {
        let Some(pad) = seats.pad_for(&pads, &settings, &players, cursor.player) else {
            continue;
        };
        // The board is still, so RB clears on the press, all at once.
        let face = &face.0;
        let on = |does| scheme::pressed_on(face, does, pad);
        let commands = pad_commands(face, pad)
            .chain(on(scheme::Does::Clear).then_some(SeatCommand::ClearAll))
            .chain(
                (on(scheme::Does::Start) || on(scheme::Does::Run)).then_some(SeatCommand::Start),
            );
        for command in commands {
            apply_setup(command, &mut cursor, &mut sim, &mut next_phase, &mut denied);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A held d-pad steps on the press, again once the delay is out, and
    /// then at the faster interval. The pad's hold once stalled at the
    /// first-step delay for good, every step waiting as long as the first.
    #[test]
    fn a_held_pad_direction_gets_past_the_first_delay() {
        let mut app = App::new();
        app.insert_resource(Sim(crate::sim::Board::new(12, 9, 1)));
        app.insert_resource(GameSettings {
            repeat_delay: 0.28,
            repeat_interval: 0.09,
            ..GameSettings::default()
        });
        app.init_resource::<PadSeats>();
        app.init_resource::<Time>();
        app.world_mut().spawn(Cursor::seated(0));
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        app.add_systems(Update, pad_move_cursor);
        let frame = |app: &mut App, secs: f32| {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_secs_f32(secs));
            app.update();
            // What the input plugin does between frames: a press is just
            // pressed for the one frame it lands in.
            app.world_mut()
                .get_mut::<Gamepad>(pad)
                .expect("the pad")
                .digital_mut()
                .clear();
            let world = app.world_mut();
            world
                .query::<&Cursor>()
                .single(world)
                .expect("one cursor")
                .x
        };
        let dpad = |app: &mut App, down: bool| {
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("the pad");
            match down {
                true => gamepad.digital_mut().press(GamepadButton::DPadRight),
                false => gamepad.digital_mut().release(GamepadButton::DPadRight),
            }
        };
        // The first frame only fits the cursor with its repeat state.
        assert_eq!(frame(&mut app, 0.0), 0);
        dpad(&mut app, true);
        assert_eq!(frame(&mut app, 0.01), 1, "the press steps at once");
        assert_eq!(frame(&mut app, 0.1), 1, "then waits out the delay");
        assert_eq!(frame(&mut app, 0.2), 2, "and steps when it is out");
        assert_eq!(frame(&mut app, 0.1), 3, "then at the interval");
        assert_eq!(frame(&mut app, 0.1), 4);
        dpad(&mut app, false);
        assert_eq!(frame(&mut app, 0.1), 4, "let go, it stops");
        dpad(&mut app, true);
        assert_eq!(frame(&mut app, 0.01), 5, "and a new press steps at once");
    }

    /// The d-pad moves P1's cursor one tile and places nothing, under
    /// every placing scheme. A board's table reads the d-pad off the pad
    /// and presses no key for it, so the mirrored scheme, whose place keys
    /// are W/A/S/D, never turns a pad's move into a placement.
    #[test]
    fn the_d_pad_moves_under_every_placing_scheme() {
        use crate::app::Screen;
        use crate::app::cycle::Cycle;
        use crate::app::settings::CommitScheme;
        let levels = crate::sim::campaign_levels();
        let level = levels
            .iter()
            .find(|level| level.posts >= 1)
            .expect("a stage with an arrow");
        let board = level.board();
        // A tile that takes an arrow with one above that does too: had a
        // bridged W placed after the step, the board would show it.
        let (x, y) = board
            .tiles()
            .map(|(x, y, _)| (x, y))
            .find(|&(x, y)| {
                y > 0 && board.can_place_signpost(0, x, y) && board.can_place_signpost(0, x, y - 1)
            })
            .expect("two open tiles, one above the other");
        for &commit in CommitScheme::VARIANTS {
            let mut app = App::new();
            app.add_plugins(bevy::state::app::StatesPlugin);
            app.init_state::<Screen>();
            app.insert_resource(State::new(Screen::Puzzle));
            app.init_state::<Phase>();
            app.insert_resource(Sim(board.clone()));
            app.insert_resource(GameSettings {
                commit,
                ..GameSettings::default()
            });
            app.insert_resource(PadFace(Face::PuzzleSetup {
                posts: true,
                hint: false,
            }));
            app.init_resource::<PadSeats>();
            app.init_resource::<PadPick>();
            app.init_resource::<Bridged>();
            app.init_resource::<crate::app::keycaps::KeyCaps>();
            app.init_resource::<crate::app::net::Online>();
            app.init_resource::<ButtonInput<KeyCode>>();
            app.init_resource::<Time>();
            app.add_message::<PlacementDenied>();
            app.add_systems(
                Update,
                (
                    pad_menu_bridge,
                    pad_move_cursor,
                    crate::app::cursor::move_cursor,
                    crate::app::play_input::setup_input,
                    pad_setup_input,
                    pad_bridge_release,
                )
                    .chain(),
            );
            let mut cursor = Cursor::seated(0);
            (cursor.x, cursor.y) = (x, y);
            app.world_mut().spawn(cursor);
            let pad = app.world_mut().spawn(Gamepad::default()).id();
            // The first frame only fits the cursor with its repeat state.
            app.update();
            app.world_mut()
                .get_mut::<Gamepad>(pad)
                .expect("the pad")
                .digital_mut()
                .press(GamepadButton::DPadUp);
            app.update();
            let world = app.world_mut();
            let cursor = world.query::<&Cursor>().single(world).expect("one cursor");
            assert_eq!((cursor.x, cursor.y), (x, y - 1), "{commit:?}: one step up");
            let board = &app.world().resource::<Sim>().0;
            assert_eq!(board.signpost_count(0), 0, "{commit:?}: and no arrow");
        }
    }

    /// A d-pad button pressed while another direction is held is a press
    /// of its own, as a second move key is on the keyboard. The stick's
    /// second axis flickering on near the deadzone is not.
    #[test]
    fn a_new_direction_on_the_pad_is_a_press() {
        assert!(fresh_press(false, true, false), "from rest");
        assert!(fresh_press(true, true, true), "a d-pad direction added");
        assert!(!fresh_press(true, true, false), "held, the stick wobbling");
        assert!(!fresh_press(true, false, false), "let go");
        assert!(!fresh_press(false, false, false), "at rest");
    }
}
