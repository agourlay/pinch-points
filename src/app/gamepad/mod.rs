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

use crate::app::cursor::Cursor;
use crate::app::settings::GameSettings;
use crate::app::{PendingActions, Phase, PlacementDenied, Sim};
use crate::sim::{Direction, PlayerAction};
use bevy::prelude::*;

/// Per-cursor repeat timer for held pad directions.
#[derive(Component)]
pub struct PadRepeat(Timer);

/// The four commit directions in face-button diamond order.
const PLACES: [(GamepadButton, Direction); 4] = [
    (GamepadButton::North, Direction::Up),
    (GamepadButton::South, Direction::Down),
    (GamepadButton::West, Direction::Left),
    (GamepadButton::East, Direction::Right),
];

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
            commands
                .entity(entity)
                .insert(PadRepeat(Timer::from_seconds(0.0, TimerMode::Once)));
            continue;
        };
        if dx == 0 && dy == 0 {
            // Released: next press steps immediately.
            repeat.0 = Timer::from_seconds(0.0, TimerMode::Once);
            continue;
        }
        repeat.0.tick(time.delta());
        if !repeat.0.is_finished() {
            continue;
        }
        // The zero-length timer is the one a fresh press starts on. Asking
        // whether elapsed equals duration says yes for every finished timer,
        // since Bevy clamps a finished one, so the hold never gets past the
        // first-step delay.
        let first_step = repeat.0.duration().is_zero();
        repeat.0 = Timer::from_seconds(
            if first_step {
                settings.repeat_delay
            } else {
                settings.repeat_interval
            },
            TimerMode::Once,
        );
        let nx = (i16::from(cursor.x) + dx).clamp(0, i16::from(board.width()) - 1) as u8;
        let ny = (i16::from(cursor.y) + dy).clamp(0, i16::from(board.height()) - 1) as u8;
        cursor.x = nx;
        cursor.y = ny;
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
        let p = cursor.player as usize;
        for (button, dir) in PLACES {
            if pad.just_pressed(button) {
                if !board.can_place_signpost(cursor.player, cursor.x, cursor.y) {
                    cursor.flash = 0.25;
                    denied.write(PlacementDenied {
                        player: cursor.player,
                        out_of_signposts: board.out_of_signposts(cursor.player, cursor.x, cursor.y),
                    });
                    continue;
                }
                pending.0[p] = PlayerAction::Place {
                    x: cursor.x,
                    y: cursor.y,
                    dir,
                };
            }
        }
        if pad.just_pressed(GamepadButton::LeftTrigger) {
            pending.0[p] = PlayerAction::Remove {
                x: cursor.x,
                y: cursor.y,
            };
        }
        if pad.pressed(GamepadButton::RightTrigger)
            && let Some((x, y)) = board.first_signpost_of(cursor.player)
        {
            pending.0[p] = PlayerAction::Remove { x, y };
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
        for (button, dir) in PLACES {
            if pad.just_pressed(button) {
                let spent = sim.0.out_of_signposts(0, cursor.x, cursor.y);
                if !sim.0.place_signpost(0, cursor.x, cursor.y, dir) {
                    cursor.flash = 0.25;
                    denied.write(PlacementDenied {
                        player: 0,
                        out_of_signposts: spent,
                    });
                }
            }
        }
        if pad.just_pressed(GamepadButton::LeftTrigger) {
            let _ = sim.0.remove_signpost(0, cursor.x, cursor.y);
        }
        if pad.just_pressed(GamepadButton::RightTrigger) {
            crate::app::play_input::clear_setup(&mut sim);
        }
        if pad.just_pressed(GamepadButton::Start) {
            next_phase.set(Phase::Running);
        }
    }
}
