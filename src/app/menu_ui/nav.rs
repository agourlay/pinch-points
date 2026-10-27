//! Moving through a list: the keys that step a cursor up and down it,
//! over the rows that are not there to pick, the number keys, Enter,
//! and the left/right turn of a dial.

use crate::app::cycle::Turn;
use bevy::prelude::*;

/// The number keys, one to nine, in the order a list numbers its rows.
pub const NUMBER_KEYS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

/// Which of the first `count` rows a number key picked this frame, as a
/// 0-based index: the menu's modes, the beaches on the air, the tide's
/// events, the seats to call.
pub fn number_pressed(keys: &ButtonInput<KeyCode>, count: usize) -> Option<usize> {
    NUMBER_KEYS
        .iter()
        .take(count)
        .position(|&key| keys.just_pressed(key))
}

/// W/S (or arrow) navigation over `len` rows, wrapping at the ends.
pub fn nav(keys: &ButtonInput<KeyCode>, selected: usize, len: usize) -> usize {
    let mut at = selected;
    if keys.just_pressed(KeyCode::KeyW) || keys.just_pressed(KeyCode::ArrowUp) {
        at = (at + len - 1) % len;
    }
    if keys.just_pressed(KeyCode::KeyS) || keys.just_pressed(KeyCode::ArrowDown) {
        at = (at + 1) % len;
    }
    at
}

/// One frame's worth of vertical menu movement. Its own type rather than
/// the `up: bool, down: bool` pair it replaces: two bools claim four states
/// where a cursor has three, and every caller had to know that up wins.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nav {
    Up,
    Down,
    Stay,
}

/// Plain movement over `len` rows from an explicit [`Nav`], for input that
/// cannot ride the keyboard resource: the pause card runs during play, where
/// bridging the d-pad into synthetic key presses would also steer the round.
pub fn step(nav: Nav, selected: usize, len: usize) -> usize {
    match nav {
        Nav::Up => (selected + len - 1) % len,
        Nav::Down => (selected + 1) % len,
        Nav::Stay => selected,
    }
}

/// Whether something earlier this frame has already said where the game
/// goes next. A way into the arena that comes with its board (a pasted
/// code, a dropped replay) gives way to it rather than overwriting it:
/// the round would otherwise be asked for and another screen entered, or
/// another round's board seated under the menu's choice.
pub fn screen_chosen(next: &NextState<crate::app::Screen>) -> bool {
    !matches!(next, NextState::Unchanged)
}

/// Enter, from either key that says it: the main one or the numpad's. One
/// question rather than a per-screen pair of `just_pressed` checks, which
/// disagreed on whether the numpad counted.
pub fn enter(keys: &ButtonInput<KeyCode>) -> bool {
    keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter)
}

/// Use up the keys that just closed something, so nothing later in the
/// frame reads them again.
///
/// A card or a line of chat closes on Enter or Escape, and the systems
/// behind it run later in the same frame and see the same press: the
/// Enter that sent a spectator's line also left the results card, and
/// the Escape that dropped it opened the pause card. Only the press is
/// spent; the key is still held, and lets go as it always would.
pub fn spend(keys: &mut ButtonInput<KeyCode>, codes: &[KeyCode]) {
    for &code in codes {
        keys.clear_just_pressed(code);
    }
}

/// The keys that end a line or put a card away.
pub const CLOSERS: [KeyCode; 3] = [KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Escape];

/// A/D (or arrow) adjustment, as the turn of a dial.
pub fn left_right(keys: &ButtonInput<KeyCode>) -> Option<Turn> {
    if keys.just_pressed(KeyCode::KeyD) || keys.just_pressed(KeyCode::ArrowRight) {
        Some(Turn::Right)
    } else if keys.just_pressed(KeyCode::KeyA) || keys.just_pressed(KeyCode::ArrowLeft) {
        Some(Turn::Left)
    } else {
        None
    }
}

/// Every button a menu answers to: the keyboard, and the pads with the
/// face whose table says what their buttons do.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Buttons<'w, 's> {
    pub keys: Res<'w, ButtonInput<KeyCode>>,
    pub pads: Query<'w, 's, &'static Gamepad>,
    pub face: Res<'w, crate::app::gamepad::PadFace>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nav_wraps_both_ways() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        assert_eq!(nav(&keys, 0, 5), 4, "up from the top wraps to the bottom");
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyS);
        assert_eq!(nav(&keys, 4, 5), 0, "down from the bottom wraps to the top");
    }

    /// The plain walk, for a list with no dark rows in it at all: up and
    /// down by one, wrapping at both ends.
    #[test]
    fn the_plain_walk_wraps_at_both_ends() {
        assert_eq!(step(Nav::Down, 0, 3), 1);
        assert_eq!(step(Nav::Down, 2, 3), 0, "off the bottom");
        assert_eq!(step(Nav::Up, 0, 3), 2, "off the top");
        assert_eq!(step(Nav::Stay, 1, 3), 1);
        assert_eq!(step(Nav::Down, 0, 1), 0, "a list of one goes nowhere");
    }
}
