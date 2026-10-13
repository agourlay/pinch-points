//! F1: typing the level's name, and the rule a name is kept by however
//! it arrived.

use super::{EditorState, Mode};
use bevy::prelude::*;

/// Type the level's name. Reads the text a keystroke produces rather than
/// the key code, so the player's own layout and their shift key decide
/// what a keystroke means.
pub(super) fn type_a_name(
    typed: &mut MessageReader<bevy::input::keyboard::KeyboardInput>,
    keys: &ButtonInput<KeyCode>,
    state: &mut EditorState,
    tr: &crate::app::i18n::Tr,
) {
    use crate::app::typing::{Keystroke, keystrokes};
    let ends = [KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Escape];
    for stroke in keystrokes(typed, &ends) {
        match stroke {
            Keystroke::Erase => {
                state.name.pop();
            }
            // A name is one line and has to fit a file name and a stage
            // caption, so it is bounded here rather than at save.
            Keystroke::Char(ch) if state.name.chars().count() < NAME_MAX => state.name.push(ch),
            // The finish is decided below from just_pressed, as one branch
            // for keyboard and pad alike.
            Keystroke::Char(_) | Keystroke::Done(_) => {}
        }
    }
    if crate::app::menu_ui::enter(keys) || keys.just_pressed(KeyCode::Escape) {
        state.name = tidy_name(&state.name, tr);
        state.mode = Mode::JustNamed;
        state.feedback.clear();
    }
}

/// How long a level's name may be.
pub(super) const NAME_MAX: usize = 28;

/// A name as the editor keeps one, however it arrived: trimmed, no longer
/// than [`NAME_MAX`], and the default name when nothing is left.
///
/// Typing bounds a name key by key, but a pasted level brings whatever its
/// text says, and the level format takes an empty name as readily as a
/// long one. An empty one is a level called nothing, and a long one went
/// past the cap to where saving cuts a file name short, so two long names
/// alike up to there shared one file.
pub(super) fn tidy_name(raw: &str, tr: &crate::app::i18n::Tr) -> String {
    let capped: String = raw.trim().chars().take(NAME_MAX).collect();
    match capped.trim_end() {
        "" => tr.ed_default_name.to_string(),
        name => name.to_string(),
    }
}

/// True while the editor is spelling out a name, which is when the board
/// keys mean letters. A run condition rather than an early return, because
/// the cursor is walked by a system of its own: typing "Wade" otherwise
/// walks the cursor up and then left across the beach.
pub fn editor_naming(state: Res<EditorState>) -> bool {
    state.is_naming()
}
