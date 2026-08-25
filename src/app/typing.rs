//! One reading of the keyboard as text.
//!
//! Three screens take typed input: a level's name, a seat's name, a line
//! of chat. Each drained the keystroke events itself, and each had its own
//! copy of the same ladder: skip releases, Backspace and Delete erase, a
//! handful of keys finish, everything else is the text the keystroke
//! produces, so the player's own layout and their shift key decide what a
//! key means. The copies had already grown three different ideas of
//! "finished"; here the ladder is written once and the finishing keys are
//! the caller's list.

use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;

/// What one keystroke means to a text box.
pub enum Keystroke {
    /// The text a key produced, one character at a time. Control
    /// characters are already dropped; any length cap is the caller's.
    Char(char),
    /// Backspace or Delete: take a character back.
    Erase,
    /// One of the caller's finishing keys, which one included, since Enter
    /// commonly commits where Escape abandons.
    Done(KeyCode),
}

/// Drain this frame's keystrokes into what each means to a text box.
/// `done` names the keys that finish the entry rather than typing.
pub fn keystrokes(typed: &mut MessageReader<KeyboardInput>, done: &[KeyCode]) -> Vec<Keystroke> {
    let mut strokes = Vec::new();
    for event in typed.read() {
        if !event.state.is_pressed() {
            continue;
        }
        if matches!(event.key_code, KeyCode::Backspace | KeyCode::Delete) {
            strokes.push(Keystroke::Erase);
        } else if done.contains(&event.key_code) {
            strokes.push(Keystroke::Done(event.key_code));
        } else {
            strokes.extend(
                event
                    .text
                    .iter()
                    .flat_map(|text| text.chars())
                    .filter(|ch| !ch.is_control())
                    .map(Keystroke::Char),
            );
        }
    }
    strokes
}
