//! One reading of the keyboard as text.
//!
//! Three screens take typed input: a level's name, a seat's name, a line of
//! chat. As a copy of the ladder apiece (skip releases, Backspace and
//! Delete erase, a handful of keys finish, everything else is the text the
//! keystroke produces) they had grown three ideas of "finished". Here the
//! ladder is written once and the finishing keys are the caller's list.

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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, NativeKey};

    /// What one stroke came out as, in a form a test can compare.
    #[derive(PartialEq, Eq, Debug)]
    enum Says {
        Ch(char),
        Erase,
        Done(KeyCode),
    }

    /// Feed the window's own report of some keys and see what the text box
    /// is told they mean.
    fn read(keys: &[(KeyCode, Option<&str>, ButtonState)], done: &[KeyCode]) -> Vec<Says> {
        let mut app = App::new();
        app.add_message::<KeyboardInput>();
        for &(key_code, text, state) in keys {
            app.world_mut().write_message(KeyboardInput {
                key_code,
                logical_key: text.map_or(Key::Unidentified(NativeKey::Unidentified), |t| {
                    Key::Character(t.into())
                }),
                state,
                text: text.map(Into::into),
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
        }
        let done = done.to_vec();
        app.world_mut()
            .run_system_once(move |mut typed: MessageReader<KeyboardInput>| {
                keystrokes(&mut typed, &done)
                    .into_iter()
                    .map(|stroke| match stroke {
                        Keystroke::Char(ch) => Says::Ch(ch),
                        Keystroke::Erase => Says::Erase,
                        Keystroke::Done(key) => Says::Done(key),
                    })
                    .collect::<Vec<_>>()
            })
            .expect("the reader ran")
    }

    /// The three things a keystroke can mean to a text box, in the order
    /// they were typed. One ladder for the level's name, the seat's name
    /// and a line of chat, which had grown three ideas of what finishes.
    #[test]
    fn a_stroke_is_text_an_erasure_or_the_key_that_finished_it() {
        let said = read(
            &[
                (KeyCode::KeyH, Some("h"), ButtonState::Pressed),
                (KeyCode::KeyI, Some("i"), ButtonState::Pressed),
                (KeyCode::Backspace, None, ButtonState::Pressed),
                (KeyCode::Delete, None, ButtonState::Pressed),
                (KeyCode::Enter, Some("\r"), ButtonState::Pressed),
            ],
            &[KeyCode::Enter, KeyCode::Escape],
        );
        assert_eq!(
            said,
            [
                Says::Ch('h'),
                Says::Ch('i'),
                Says::Erase,
                Says::Erase,
                Says::Done(KeyCode::Enter),
            ]
        );
    }

    /// Which finishing key it was travels with it, because Enter commonly
    /// commits where Escape abandons, and a caller that could not tell
    /// them apart would save the line somebody pressed Escape on.
    #[test]
    fn the_key_that_finished_says_which_one_it_was() {
        let said = read(
            &[(KeyCode::Escape, None, ButtonState::Pressed)],
            &[KeyCode::Enter, KeyCode::Escape],
        );
        assert_eq!(said, [Says::Done(KeyCode::Escape)]);

        // A key nobody named as finishing is just a key, and one with no
        // text to it types nothing at all.
        let said = read(&[(KeyCode::Escape, None, ButtonState::Pressed)], &[]);
        assert_eq!(said, [], "Escape typed a character");
    }

    /// Letting a key go is not typing it again. Read from the raw stream,
    /// every character in a name arrived twice.
    #[test]
    fn letting_a_key_go_types_nothing() {
        let said = read(
            &[
                (KeyCode::KeyA, Some("a"), ButtonState::Pressed),
                (KeyCode::KeyA, Some("a"), ButtonState::Released),
                (KeyCode::Backspace, None, ButtonState::Released),
                (KeyCode::Enter, None, ButtonState::Released),
            ],
            &[KeyCode::Enter],
        );
        assert_eq!(said, [Says::Ch('a')]);
    }

    /// A control character is not text, whatever the window calls it.
    /// Enter reports itself as "\r" and Tab as "\t", and a box that took
    /// them at their word had a carriage return in the middle of a name.
    #[test]
    fn a_control_character_is_not_something_to_type() {
        let said = read(
            &[
                (KeyCode::Enter, Some("\r"), ButtonState::Pressed),
                (KeyCode::Tab, Some("\t"), ButtonState::Pressed),
                (KeyCode::Space, Some(" "), ButtonState::Pressed),
            ],
            &[],
        );
        assert_eq!(said, [Says::Ch(' ')], "only the space is text");
    }
}
