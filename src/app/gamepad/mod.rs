//! Gamepad input (spec §8.1). The face buttons form a diamond of four
//! directions, so placement maps them directly: top places Up, bottom Down,
//! left Left, right Right. D-pad or left stick moves the cursor, L1 removes,
//! R1 clears own signposts, Start pauses.
//!
//! Pads fill the human seats **from the highest seat downward**, so every
//! two-player setup works without configuration: one pad drives P2 beside
//! a P1 keyboard, two pads drive P2 and P1, and no pads leaves both
//! keyboard layouts. With four humans, pads land on P4 and P3; with six,
//! on P6 down to P3. The keyboard always stays usable for seats 1-2; both
//! feed the identical commit paths as §8.2.

mod bridge;
mod rumble;
mod seats;

pub use bridge::{Bridged, pad_bridge_release, pad_menu_bridge};
pub use rumble::rumble_on_raid;
pub use seats::{PadSeats, keep_pad_order, pad_claim_seats};

use crate::app::cursor::{Cursor, HoldRepeat};
use crate::app::play_input::{SeatCommand, apply_setup, queue_versus};
use crate::app::settings::GameSettings;
use crate::app::{PendingActions, Phase, PlacementDenied, Sim};
use crate::sim::Direction;
use bevy::prelude::*;

/// Per-cursor hold-to-repeat for held pad directions, and whether a
/// direction was held last frame, which makes this frame's a fresh press
/// or not: the stick has no `just_pressed` of its own.
#[derive(Component, Default)]
pub struct PadRepeat {
    repeat: HoldRepeat,
    held: bool,
}

/// The four commit directions in face-button diamond order.
const PLACES: [(GamepadButton, Direction); 4] = [
    (GamepadButton::North, Direction::Up),
    (GamepadButton::South, Direction::Down),
    (GamepadButton::West, Direction::Left),
    (GamepadButton::East, Direction::Right),
];

/// The pad's fresh presses that mean the same in setup and in versus: a
/// face button places, L1 removes. R1 and Start are read by each mode.
fn pad_commands(pad: &Gamepad) -> impl Iterator<Item = SeatCommand> + '_ {
    let places = PLACES
        .into_iter()
        .filter(|(button, _)| pad.just_pressed(*button));
    places.map(|(_, dir)| SeatCommand::Place(dir)).chain(
        pad.just_pressed(GamepadButton::LeftTrigger)
            .then_some(SeatCommand::Remove),
    )
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
        let just = held && !repeat.held;
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
    seats: Res<PadSeats>,
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
        // R1 clears held, as the keyboard's clear-all does: one removal
        // a tick for as long as it is down.
        let commands = pad_commands(pad).chain(
            pad.pressed(GamepadButton::RightTrigger)
                .then_some(SeatCommand::ClearAll),
        );
        for command in commands {
            queue_versus(command, &mut cursor, board, &mut pending, &mut denied);
        }
    }
}

/// Puzzle setup commits from P1's pad: place/remove directly, Start begins
/// the run (mirrors the keyboard Enter). P1's pad is whichever one
/// [`PadSeats::pad_for`] hands the seat - the one it named in settings, or the
/// first unclaimed one - the same answer `pad_move_cursor` gives, so the
/// pad that moves the cursor is the pad that places.
pub fn pad_setup_input(
    pads: Query<(Entity, &Gamepad)>,
    seats: Res<PadSeats>,
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
        // The board is still, so R1 clears on the press, all at once.
        let commands = pad_commands(pad)
            .chain(
                pad.just_pressed(GamepadButton::RightTrigger)
                    .then_some(SeatCommand::ClearAll),
            )
            .chain(
                pad.just_pressed(GamepadButton::Start)
                    .then_some(SeatCommand::Start),
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
}
