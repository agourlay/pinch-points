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

/// Navigation over a list where some rows are hidden (a setting that does
/// not apply right now): the cursor steps over them instead of landing on
/// a blank line. With nothing live it stays put.
pub fn nav_live(keys: &ButtonInput<KeyCode>, selected: usize, live: &[bool]) -> usize {
    let nav = if keys.just_pressed(KeyCode::KeyW) || keys.just_pressed(KeyCode::ArrowUp) {
        Nav::Up
    } else if keys.just_pressed(KeyCode::KeyS) || keys.just_pressed(KeyCode::ArrowDown) {
        Nav::Down
    } else {
        Nav::Stay
    };
    nav_live_steps(nav, selected, live)
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

/// The same walk as [`nav_live`], from an explicit [`Nav`] rather than the
/// keyboard.
fn nav_live_steps(nav: Nav, selected: usize, live: &[bool]) -> usize {
    let len = live.len();
    let step = match nav {
        Nav::Up => len - 1, // one backwards, modulo len
        Nav::Down => 1,
        // Not moving, but the row under the cursor may have just gone dark.
        Nav::Stay => return first_live(selected, live).unwrap_or(selected),
    };
    let mut at = selected;
    for _ in 0..len {
        at = (at + step) % len;
        if live[at] {
            return at;
        }
    }
    selected
}

/// `from` if it is live, else the next live row after it.
fn first_live(from: usize, live: &[bool]) -> Option<usize> {
    (0..live.len())
        .map(|offset| (from + offset) % live.len())
        .find(|&row| live[row])
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

/// Every button a menu answers to: the keyboard, and the pads.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Buttons<'w, 's> {
    pub keys: Res<'w, ButtonInput<KeyCode>>,
    pub pads: Query<'w, 's, &'static Gamepad>,
}

impl Buttons<'_, '_> {
    /// Whether any pad pressed `button` this frame.
    pub fn pad(&self, button: GamepadButton) -> bool {
        self.pads.iter().any(|pad| pad.just_pressed(button))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hidden rows are stepped over in both directions, and a cursor left
    /// standing on a row that just went dark slides to the next live one.
    #[test]
    fn nav_live_skips_hidden_rows() {
        let live = [true, false, false, true, false];
        let mut down = ButtonInput::<KeyCode>::default();
        down.press(KeyCode::KeyS);
        assert_eq!(nav_live(&down, 0, &live), 3);
        assert_eq!(nav_live(&down, 3, &live), 0, "wraps past the dark tail");
        let mut up = ButtonInput::<KeyCode>::default();
        up.press(KeyCode::KeyW);
        assert_eq!(nav_live(&up, 3, &live), 0);
        assert_eq!(nav_live(&up, 0, &live), 3);
        // Standing still on a row that went dark: slide to the next live one.
        let idle = ButtonInput::<KeyCode>::default();
        assert_eq!(nav_live(&idle, 1, &live), 3);
        assert_eq!(nav_live(&idle, 3, &live), 3);
        // Nothing live at all: stay put rather than spin.
        assert_eq!(nav_live(&down, 2, &[false, false, false]), 2);
    }

    #[test]
    fn nav_wraps_both_ways() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        assert_eq!(nav(&keys, 0, 5), 4, "up from the top wraps to the bottom");
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyS);
        assert_eq!(nav(&keys, 4, 5), 0, "down from the bottom wraps to the top");
    }

    /// A row that is not there to pick is stepped over, not landed on.
    ///
    /// Every list with rows that come and go reads this: the settings that
    /// only apply in one mode, the stage list's locked levels, the match
    /// dials that a chosen map has no use for. Landing on one puts the
    /// cursor on a blank line where Enter does nothing.
    #[test]
    fn the_cursor_steps_over_a_row_that_is_not_there_to_pick() {
        let live = [true, false, false, true, true];
        assert_eq!(
            nav_live_steps(Nav::Down, 0, &live),
            3,
            "over the two dark ones"
        );
        assert_eq!(nav_live_steps(Nav::Up, 3, &live), 0, "and back over them");
        assert_eq!(nav_live_steps(Nav::Down, 3, &live), 4, "the next one along");
    }

    /// The ends of the list join up, which is the only way to reach the
    /// last row from the first without walking the whole list.
    #[test]
    fn the_ends_of_the_list_join_up() {
        let all = [true, true, true];
        assert_eq!(
            nav_live_steps(Nav::Up, 0, &all),
            2,
            "off the top to the bottom"
        );
        assert_eq!(
            nav_live_steps(Nav::Down, 2, &all),
            0,
            "and off the bottom to the top"
        );

        // With dark rows at the seam, the wrap keeps walking past them.
        let live = [true, false, false];
        assert_eq!(
            nav_live_steps(Nav::Down, 0, &live),
            0,
            "the only row there is"
        );
        assert_eq!(nav_live_steps(Nav::Up, 0, &live), 0);
    }

    /// A list with nothing live leaves the cursor exactly where it was.
    /// The walk gives up after one lap rather than spinning, which is what
    /// an empty custom-beach shelf looks like from here.
    #[test]
    fn a_list_with_nothing_to_pick_leaves_the_cursor_alone() {
        let none = [false, false, false];
        for nav in [Nav::Up, Nav::Down, Nav::Stay] {
            assert_eq!(
                nav_live_steps(nav, 1, &none),
                1,
                "the cursor moved to nowhere"
            );
        }
    }

    /// Standing still still moves, when the row underneath has just gone
    /// dark: a dial answered somewhere else can take the row the cursor is
    /// on out of the list, and the cursor has to come off it without
    /// anybody pressing anything.
    #[test]
    fn standing_still_comes_off_a_row_that_has_just_gone_dark() {
        let live = [false, false, true, true];
        assert_eq!(
            nav_live_steps(Nav::Stay, 0, &live),
            2,
            "the next one that is there"
        );
        assert_eq!(
            nav_live_steps(Nav::Stay, 2, &live),
            2,
            "and a live row stays put"
        );
        // Past the end, the search wraps like the walk does.
        let live = [true, false, false];
        assert_eq!(nav_live_steps(Nav::Stay, 2, &live), 0);
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
