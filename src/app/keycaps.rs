//! What the letter keys say on *this* keyboard.
//!
//! Every binding is a Bevy [`KeyCode`], which is a physical position: the
//! key under the QWERTY "W" is `KeyW` on an AZERTY board too, where its cap
//! reads "Z". So the stock WASD already moves an AZERTY player on ZQSD -
//! but every legend on screen spelled the QWERTY caps, and the controls
//! screen answered a press of "Z" with "W", which reads as the game
//! ignoring the keyboard in front of it.
//!
//! The keys the game reads by their *letter* - M for mute, H for the hint,
//! N/P/R, C/V, T - are mnemonics, and a mnemonic has to sit on the cap that
//! says so: "M: mute" on an AZERTY board means the key that reads M, which
//! is `Semicolon` to Bevy. Those reads go through [`KeyCaps::key_for`].
//!
//! Nothing lets a program ask winit for the keymap up front, so the caps
//! are learned: each key press carries both the physical code and the
//! layout-aware character, and the pair is remembered (see
//! [`learn_keycaps`]). The table lives in the settings file so the second
//! launch reads right from its first menu.

use std::collections::BTreeMap;

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

use crate::app::binds;
use crate::app::settings::GameSettings;

/// Physical key -> the character on its cap, for the keys whose cap
/// differs from Bevy's QWERTY spelling. Empty on a QWERTY board.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct KeyCaps(BTreeMap<KeyCode, char>);

/// The two four-key blocks the legends name by their QWERTY caps, and the
/// physical keys they stand for.
const BLOCKS: [(&str, [KeyCode; 4]); 2] = [
    (
        "WASD",
        [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD],
    ),
    (
        "IJKL",
        [KeyCode::KeyI, KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL],
    ),
];

/// The two layouts common enough to finish from one press: a board that
/// says "Z" where QWERTY says "W" is AZERTY, and its Q/A moved with it
/// (and M, to the key QWERTY calls Semicolon); one that swaps Y and Z is
/// QWERTZ. Any key actually pressed still overrides the presumption.
const FAMILIES: &[&[(KeyCode, char)]] = &[
    &[
        (KeyCode::KeyW, 'Z'),
        (KeyCode::KeyZ, 'W'),
        (KeyCode::KeyA, 'Q'),
        (KeyCode::KeyQ, 'A'),
        (KeyCode::Semicolon, 'M'),
    ],
    &[(KeyCode::KeyY, 'Z'), (KeyCode::KeyZ, 'Y')],
];

impl KeyCaps {
    /// What the cap says, if it differs from the QWERTY spelling.
    pub fn cap(&self, key: KeyCode) -> Option<char> {
        self.0.get(&key).copied()
    }

    /// How a key reads on screen on this keyboard: the learned cap, or
    /// [`binds::key_label`]'s spelling when the key is not a letter or has
    /// never been pressed.
    pub fn label(&self, key: KeyCode) -> String {
        self.cap(key)
            .map_or_else(|| binds::key_label(key), |c| c.to_string())
    }

    /// The physical key whose cap says `letter`: the one learned to say
    /// so, else the key QWERTY spells that way. This is how a mnemonic
    /// stays on its letter across layouts.
    pub fn key_for(&self, letter: char) -> KeyCode {
        let letter = letter.to_ascii_uppercase();
        self.0
            .iter()
            .find(|(_, c)| **c == letter)
            .map(|(k, _)| *k)
            .or_else(|| binds::key_from_name(&format!("Key{letter}")))
            .or_else(|| binds::global_key_from_name(&format!("Key{letter}")))
            .unwrap_or_else(|| panic!("no key spells {letter}"))
    }

    /// Whether the cap that says `letter` was just pressed.
    pub fn just_pressed(&self, keys: &ButtonInput<KeyCode>, letter: char) -> bool {
        keys.just_pressed(self.key_for(letter))
    }

    /// Whether `key` is one of the [`binds::GLOBAL_KEYS`] on this
    /// keyboard - read by its letter whatever the bindings say, so not a
    /// key to hand a seat. The stock check is by position; this one is by
    /// cap, which is what the game reads.
    pub fn is_global(&self, key: KeyCode) -> bool {
        binds::GLOBAL_LETTERS
            .iter()
            .any(|&l| self.key_for(l) == key)
    }

    /// A legend with its `WASD` / `IJKL` block names respelled in this
    /// keyboard's caps: "WASD move" becomes "ZQSD move" on AZERTY. The
    /// legends across every language name the blocks by these exact four
    /// letters, so one respelling serves them all.
    pub fn legend(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (spelling, keys) in BLOCKS {
            if !out.contains(spelling) {
                continue;
            }
            let caps: String = keys.iter().map(|&k| self.label(k)).collect();
            out = out.replace(spelling, &caps);
        }
        out
    }

    /// Remember what `key`'s cap says. Whether the table changed.
    ///
    /// Only a printable ASCII character is taken as a cap: the letters and
    /// punctuation are what move between layouts, while a digit row that
    /// yields `&é"'` unshifted (AZERTY) is still called 1234, and a
    /// Cyrillic or kana board carries the Latin caps as well and its
    /// players read WASD. Storing only the caps that differ from the
    /// spelling keeps a QWERTY file empty and lets a board switched back
    /// to QWERTY unlearn itself.
    pub fn learn(&mut self, key: KeyCode, cap: char) -> bool {
        if !cap.is_ascii_graphic() || !learnable(key) {
            return false;
        }
        let cap = cap.to_ascii_uppercase();
        let before = self.0.clone();
        if binds::key_label(key) == cap.to_string() {
            self.0.remove(&key);
        } else {
            self.0.insert(key, cap);
        }
        for family in FAMILIES {
            if family.contains(&(key, cap)) {
                for &(k, c) in *family {
                    self.0.entry(k).or_insert(c);
                }
            }
        }
        self.0 != before
    }

    /// Settings-file text: `KeyW=Z KeyA=Q`, empty on QWERTY.
    pub fn to_text(&self) -> String {
        self.0
            .iter()
            .map(|(k, c)| format!("{}={c}", binds::key_name(*k)))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Lenient inverse of [`Self::to_text`]: a token that does not name a
    /// key or a cap is skipped, not fatal.
    pub fn parse(text: &str) -> KeyCaps {
        let mut caps = KeyCaps::default();
        for token in text.split_whitespace() {
            let Some((name, cap)) = token.split_once('=') else {
                continue;
            };
            let mut chars = cap.chars();
            if let (Some(key), Some(c), None) =
                (binds::key_from_name(name), chars.next(), chars.next())
                && learnable(key)
                && c.is_ascii_graphic()
            {
                caps.0.insert(key, c.to_ascii_uppercase());
            }
        }
        caps
    }
}

/// The keys whose caps are worth learning: the bindable ones (the only
/// ones ever shown by name), less the digit rows, whose names are the
/// numbers whatever the unshifted cap says.
fn learnable(key: KeyCode) -> bool {
    let name = binds::key_name(key);
    binds::bindable(key)
        && key != KeyCode::Space
        && !name.starts_with("Digit")
        && !name.starts_with("Numpad")
}

/// Watch every key press for what its cap says, and keep the table on disk.
///
/// A press under a modifier is skipped: Shift turns "," into "<" and AltGr
/// turns "E" into "€", neither of which is what the cap says at a glance.
/// Repeats are skipped for being the same press again.
pub fn learn_keycaps(
    mut typed: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<GameSettings>,
) {
    const MODIFIERS: [KeyCode; 8] = [
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ];
    let mut changed = false;
    for input in typed.read() {
        if !input.state.is_pressed() || input.repeat || keys.any_pressed(MODIFIERS) {
            continue;
        }
        let Key::Character(text) = &input.logical_key else {
            continue;
        };
        let mut chars = text.chars();
        let (Some(cap), None) = (chars.next(), chars.next()) else {
            continue;
        };
        // Learned into a copy first: touching the resource through
        // `ResMut` marks it changed, and everything that reapplies
        // settings on a change would run for every keypress.
        let mut caps = settings.keycaps.clone();
        if caps.learn(input.key_code, cap) {
            settings.keycaps = caps;
            changed = true;
        }
    }
    if changed {
        settings.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_qwerty_board_learns_nothing() {
        let mut caps = KeyCaps::default();
        assert!(!caps.learn(KeyCode::KeyW, 'w'));
        assert!(!caps.learn(KeyCode::Comma, ','));
        assert_eq!(caps, KeyCaps::default());
        assert_eq!(caps.legend("WASD move | IJKL"), "WASD move | IJKL");
        assert_eq!(caps.label(KeyCode::KeyW), "W");
        assert_eq!(caps.to_text(), "");
    }

    /// One press of the key under QWERTY's W says "Z": AZERTY, and the
    /// whole family follows so the first legend is already right.
    #[test]
    fn azerty_is_presumed_from_one_press_and_respells_the_legends() {
        let mut caps = KeyCaps::default();
        assert!(caps.learn(KeyCode::KeyW, 'z'));
        assert_eq!(caps.label(KeyCode::KeyW), "Z");
        assert_eq!(caps.label(KeyCode::KeyA), "Q");
        assert_eq!(caps.label(KeyCode::Semicolon), "M");
        assert_eq!(
            caps.legend("WASD bouger | flèches placer | IJKL"),
            "ZQSD bouger | flèches placer | IJKL"
        );
        // A second press of the same key is nothing new.
        assert!(!caps.learn(KeyCode::KeyW, 'z'));
    }

    /// The mnemonics follow the cap: on AZERTY "M" is the key QWERTY calls
    /// Semicolon, and the key at QWERTY's M (which says ",") is not mute.
    #[test]
    fn mnemonics_follow_the_cap() {
        let caps = KeyCaps::default();
        assert_eq!(caps.key_for('M'), KeyCode::KeyM);
        assert_eq!(caps.key_for('h'), KeyCode::KeyH);
        assert!(caps.is_global(KeyCode::KeyM));
        assert!(!caps.is_global(KeyCode::Semicolon));

        let mut azerty = KeyCaps::default();
        azerty.learn(KeyCode::KeyW, 'z');
        assert_eq!(azerty.key_for('M'), KeyCode::Semicolon);
        assert_eq!(azerty.key_for('A'), KeyCode::KeyQ);
        assert_eq!(azerty.key_for('H'), KeyCode::KeyH);
        assert!(azerty.is_global(KeyCode::Semicolon));
        // Its own position still refuses a seat: `bindable` says so.
        assert!(!binds::bindable(KeyCode::KeyM));

        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Semicolon);
        assert!(azerty.just_pressed(&keys, 'M'));
        assert!(!caps.just_pressed(&keys, 'M'));
    }

    #[test]
    fn qwertz_swaps_y_and_z() {
        let mut caps = KeyCaps::default();
        assert!(caps.learn(KeyCode::KeyZ, 'y'));
        assert_eq!(caps.label(KeyCode::KeyY), "Z");
        assert_eq!(caps.label(KeyCode::KeyZ), "Y");
        // The WASD block is untouched on QWERTZ.
        assert_eq!(caps.legend("WASD"), "WASD");
    }

    /// A press beats a presumption, and a cap that matches the spelling
    /// again forgets the key: the board was switched back.
    #[test]
    fn a_press_overrides_a_presumption_and_can_unlearn() {
        let mut caps = KeyCaps::default();
        caps.learn(KeyCode::KeyW, 'z');
        assert!(caps.learn(KeyCode::Semicolon, ';'));
        assert_eq!(caps.label(KeyCode::Semicolon), ";");
        assert!(caps.learn(KeyCode::KeyW, 'w'));
        assert_eq!(caps.label(KeyCode::KeyW), "W");
    }

    /// Digits, the numpad, and anything outside printable ASCII are not
    /// caps to learn.
    #[test]
    fn digits_and_non_ascii_are_ignored() {
        let mut caps = KeyCaps::default();
        assert!(!caps.learn(KeyCode::Digit1, '&'));
        assert!(!caps.learn(KeyCode::Numpad1, '1'));
        assert!(!caps.learn(KeyCode::KeyW, 'ц'));
        assert!(!caps.learn(KeyCode::KeyE, '€'));
        assert!(!caps.learn(KeyCode::Space, ' '));
        assert_eq!(caps, KeyCaps::default());
    }

    /// [`KeyCaps::legend`] respells the literal block names, so every
    /// language's legends have to name the blocks by those four letters
    /// exactly - a translation that wrote "W-A-S-D" or "ZQSD" would slip
    /// past it and teach the wrong keys.
    #[test]
    fn every_language_names_the_blocks_by_their_qwerty_caps() {
        let mut azerty = KeyCaps::default();
        azerty.learn(KeyCode::KeyW, 'z');
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            for legend in [
                tr.prompt_setup,
                tr.prompt_versus_short,
                tr.prompt_versus_local,
                tr.ed_prompt,
                lang.level_hint(crate::app::hud::KEY_LESSON_LEVEL).unwrap(),
            ] {
                assert!(legend.contains("WASD"), "{lang:?}: {legend:?}");
                let respelled = azerty.legend(legend);
                assert!(respelled.contains("ZQSD"), "{lang:?}: {respelled:?}");
                assert!(!respelled.contains("WASD"), "{lang:?}: {respelled:?}");
            }
            assert!(tr.prompt_versus_local.contains("IJKL"), "{lang:?}");
            assert!(tr.val_ijkl.contains("IJKL"), "{lang:?}");
        }
    }

    #[test]
    fn text_round_trips_and_parses_leniently() {
        let mut caps = KeyCaps::default();
        caps.learn(KeyCode::KeyW, 'z');
        caps.learn(KeyCode::Slash, ':');
        let text = caps.to_text();
        assert_eq!(KeyCaps::parse(&text), caps);
        assert_eq!(text, "KeyA=Q KeyQ=A KeyW=Z KeyZ=W Semicolon=M Slash=:");
        let lenient = KeyCaps::parse("nonsense KeyW=Z NoSuchKey=Q KeyA=QQ Digit1=& KeyE=");
        let mut expected = KeyCaps::default();
        expected.0.insert(KeyCode::KeyW, 'Z');
        assert_eq!(lenient, expected);
    }
}
