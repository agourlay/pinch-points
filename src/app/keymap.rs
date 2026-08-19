//! What the keys say, asked of the operating system.
//!
//! [`crate::app::keycaps`] learns the caps from presses, because that is
//! all a `KeyboardInput` event offers and winit publishes no keymap of its
//! own - it parses one internally on every platform and keeps it private.
//! But the same tables the toolkit reads are a few calls away, and asking
//! for them directly is what every other game does: the whole board, at
//! start-up, before a key is touched and including the punctuation no
//! player ever presses.
//!
//! The asking lives in [`pinch_keymap`], which is a crate of its own
//! because two of the three platforms answer only through FFI and this
//! one forbids unsafe code. It answers in W3C `code` names - "KeyW",
//! "Semicolon" - which is the vocabulary Bevy's [`KeyCode`] is spelled
//! in, so [`binds::key_from_name`] is the whole of the translation.
//!
//! What it cannot answer - a platform with no path, an X server that will
//! not talk to us, a cap that is a dead key or is not ASCII - falls
//! through to the presses and the language, which stay exactly as they
//! were. This is a better first answer, not a replacement.
//!
//! A layout switched *during* a session is not noticed here; the next
//! press on a moved key is, and corrects the table itself.

use bevy::prelude::*;

use crate::app::binds;
use crate::app::settings::GameSettings;

/// Take the keyboard's word for its own caps, at start-up.
///
/// Saved only once a language has been taken: on a first run this lands
/// while the language picker is still up, and the picker is chosen on the
/// settings file not existing yet. See
/// [`crate::app::language::may_save`].
///
/// An exclusive system for one reason: macOS asks for the keyboard type
/// through a Carbon call that belongs on the main thread, and an
/// exclusive system is the one kind Bevy promises to run there.
pub fn read_keymap(world: &mut World) {
    let caps = ask();
    if caps.is_empty() {
        return;
    }
    // Read before the settings are borrowed, and false when there is no
    // screen at all: not writing is always the safe answer.
    let may_save = world
        .get_resource::<State<crate::app::Screen>>()
        .is_some_and(|screen| crate::app::language::may_save(screen.get()));
    let mut settings = world.resource_mut::<GameSettings>();
    let mut keycaps = settings.keycaps.clone();
    if keycaps.adopt(&caps) {
        settings.keycaps = keycaps;
        if may_save {
            settings.save();
        }
    }
}

/// The platform's answer, in this game's key type. A name it does not
/// know is dropped rather than fussed over: the crate reports the whole
/// keyboard, and the game binds a part of it.
fn ask() -> Vec<(KeyCode, char)> {
    pinch_keymap::query()
        .into_iter()
        .filter_map(|(name, cap)| {
            let key = binds::key_from_name(name).or_else(|| binds::global_key_from_name(name))?;
            Some((key, cap))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two crates agree on the spelling of a key, or every answer is
    /// silently dropped and the query does nothing at all. Bevy spells
    /// its [`KeyCode`]s the same way the W3C does, and this is what says
    /// so out loud.
    #[test]
    fn every_key_the_platform_names_is_one_the_game_knows() {
        let named = pinch_keymap::query();
        for (name, _) in &named {
            assert!(
                binds::key_from_name(name).is_some() || binds::global_key_from_name(name).is_some(),
                "{name} is not a key this game can name"
            );
        }
        assert_eq!(
            ask().len(),
            named.len(),
            "an answer was dropped in translation"
        );
    }

    /// Whatever this machine's keyboard is, the caps it reports have to be
    /// ones [`crate::app::keycaps`] would take from a press.
    #[test]
    fn the_answer_is_one_the_caps_table_would_take() {
        for (key, cap) in ask() {
            assert!(cap.is_ascii_graphic(), "{key:?} said {cap:?}");
        }
    }
}
