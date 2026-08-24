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
//! Nothing lets a program ask *winit* for the keymap up front, so the caps
//! come from three places, in this order:
//!
//! 1. **Read from the platform.** [`crate::app::keymap`] asks the
//!    operating system directly at start-up - X11, `user32`, Carbon - and
//!    hands over the whole board through [`KeyCaps::adopt`]. This is the
//!    real answer where it can be had, which is most machines.
//! 2. **Learned from presses.** Each key press carries both the physical
//!    code and the layout-aware character, and the pair is remembered
//!    (see [`learn_keycaps`]). This is what catches a layout switched
//!    mid-session, and what answers on a platform with no query. Both of
//!    these live in the settings file, as one table: a fact is a fact
//!    whichever asked for it.
//! 3. **Presumed from the language.** A player who reads the game in
//!    French is typing on AZERTY, and one who reads it in German on
//!    QWERTZ - so a first run is spelled right even before the query
//!    lands. See [`Layout::of`]. Never saved: it is re-derived from the
//!    language at every load.
//!
//! A presumption is a guess, and the first press that disagrees with it
//! retires it (see [`KeyCaps::learn`]) - one press of W on a Québécois or
//! Swiss board and the game stops believing in AZERTY.

mod layout;

pub use layout::Layout;

use std::collections::BTreeMap;

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

use crate::app::binds;
use crate::app::settings::GameSettings;

/// What this keyboard's caps say, where they differ from Bevy's QWERTY
/// spelling: what presses have shown, over what the language presumes.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct KeyCaps {
    /// Physical key -> the character on its cap, from presses. Empty on a
    /// QWERTY board that has never been guessed at. Saved.
    learned: BTreeMap<KeyCode, char>,
    /// The language's layout, until a press says otherwise. Not saved.
    ///
    /// While this is `Some`, `learned` never contradicts it: the press
    /// that would have is the press that clears it.
    presumed: Option<Layout>,
    /// The layout the player named on the settings card, which is the
    /// whole answer for as long as it is set - not a cap over the top of
    /// the others but instead of them, because "this is an AZERTY board"
    /// is a statement about the whole board.
    ///
    /// Saved as its own setting rather than in the caps text, since it is
    /// a preference and the rest is evidence. The evidence goes on being
    /// collected underneath: switch back to Auto and it is all still
    /// there, up to date.
    forced: Option<Layout>,
}

/// The key groups the legends name by their QWERTY caps, and the physical
/// keys they stand for, letter for letter.
///
/// The move blocks are the obvious pair, but "W/S" and "A/D" are the ones
/// most players read most often: every menu, settings and setup prompt in
/// every language opens with one, so a board they are not respelled on is
/// a board where the very first line on screen names the wrong keys.
///
/// The punctuation inside a spelling is kept as it is written; only the
/// letters are looked up. Longest first, so a shorter group cannot eat
/// part of a longer one.
const BLOCKS: &[(&str, &[KeyCode])] = &[
    (
        "WASD",
        &[KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD],
    ),
    (
        "IJKL",
        &[KeyCode::KeyI, KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL],
    ),
    ("W/S", &[KeyCode::KeyW, KeyCode::KeyS]),
    ("A/D", &[KeyCode::KeyA, KeyCode::KeyD]),
];

impl KeyCaps {
    /// What the cap says, if it differs from the QWERTY spelling.
    pub fn cap(&self, key: KeyCode) -> Option<char> {
        if let Some(forced) = self.forced {
            return forced.cap(key);
        }
        self.learned
            .get(&key)
            .copied()
            .or_else(|| self.presumed.and_then(|layout| layout.cap(key)))
    }

    /// How a key reads on screen on this keyboard: the learned cap, or
    /// [`binds::key_label`]'s spelling when the key is not a letter or has
    /// never been pressed.
    pub fn label(&self, key: KeyCode) -> String {
        self.cap(key)
            .map_or_else(|| binds::key_label(key), |c| c.to_string())
    }

    /// The physical key whose cap says `letter`: the one learned to say
    /// so, else the one the presumed layout puts it on, else the key
    /// QWERTY spells that way. This is how a mnemonic stays on its letter
    /// across layouts.
    pub fn key_for(&self, letter: char) -> KeyCode {
        let letter = letter.to_ascii_uppercase();
        self.forced
            .map_or_else(
                || {
                    self.learned
                        .iter()
                        .find(|(_, c)| **c == letter)
                        .map(|(k, _)| *k)
                        .or_else(|| self.presumed.and_then(|layout| layout.key_for(letter)))
                },
                |forced| forced.key_for(letter),
            )
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

    /// A legend with its [`BLOCKS`] respelled in this keyboard's caps:
    /// "WASD move" becomes "ZQSD move" on AZERTY, and "W/S: choose"
    /// becomes "Z/S: choose". The legends across every language name the
    /// groups by these exact letters, so one respelling serves them all.
    pub fn legend(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (spelling, keys) in BLOCKS {
            if !out.contains(spelling) {
                continue;
            }
            let mut keys = keys.iter();
            let caps: String = spelling
                .chars()
                .map(|c| {
                    if c.is_ascii_alphabetic() {
                        keys.next().map_or_else(String::new, |&key| self.label(key))
                    } else {
                        c.to_string()
                    }
                })
                .collect();
            out = out.replace(spelling, &caps);
        }
        out
    }

    /// Take the operating system's word for the caps, key by key.
    ///
    /// Each pair goes through [`Self::learn`], because a keymap read from
    /// the platform is the same kind of fact as a press and deserves the
    /// same treatment: it overwrites a stale cap, forgets one that is
    /// back to its QWERTY spelling, and retires a presumption it
    /// disagrees with. Whether anything changed.
    ///
    /// The keys the platform would not answer for - a dead key, a cap
    /// outside ASCII - are simply absent from `keymap`, so whatever the
    /// presses and the language had to say about them still stands.
    pub fn adopt(&mut self, keymap: &[(KeyCode, char)]) -> bool {
        let mut changed = false;
        for &(key, cap) in keymap {
            changed |= self.learn(key, cap);
        }
        changed
    }

    /// Read the whole board as `layout`, whatever the keyboard, the
    /// presses and the language say - or, with `None`, go back to working
    /// it out from all three.
    ///
    /// The escape hatch every game ships, and the reason it is worth
    /// shipping: detection is right almost always, and "almost" covers
    /// remote desktops, KVMs, a borrowed machine and the odd locale
    /// nobody thought of. A player who can see the wrong letters on the
    /// card must be able to say so.
    pub fn force(&mut self, layout: Option<Layout>) {
        self.forced = layout;
    }

    /// Take `layout` as this keyboard, until a press says otherwise.
    ///
    /// Called with the language's [`Layout::of`] whenever the language is
    /// set, which is also every load - so a guess disproved in an earlier
    /// session is not made again: the press that disproved it is in
    /// `learned`, and a presumption is refused outright by any learned cap
    /// that spells one of its keys differently.
    pub fn presume(&mut self, layout: Option<Layout>) {
        self.presumed = layout.filter(|layout| {
            layout
                .caps()
                .iter()
                .all(|&(key, cap)| self.learned.get(&key).is_none_or(|&said| said == cap))
        });
    }

    /// Remember what `key`'s cap says. Whether anything changed.
    ///
    /// Only a printable ASCII character is taken as a cap: the letters and
    /// punctuation are what move between layouts, while a digit row that
    /// yields `&é"'` unshifted (AZERTY) is still called 1234, and a
    /// Cyrillic or kana board carries the Latin caps as well and its
    /// players read WASD. Storing only the caps that differ from the
    /// spelling keeps a QWERTY file empty and lets a board switched back
    /// to QWERTY unlearn itself.
    ///
    /// The exception is a press that disproves the presumed layout. That
    /// one is written down even though it agrees with QWERTY, because it
    /// is the only record that the guess was wrong - without it the
    /// language would make the same guess at the next launch, and the
    /// player would spend every session's first keypress taking it back.
    pub fn learn(&mut self, key: KeyCode, cap: char) -> bool {
        if !cap.is_ascii_graphic() || !learnable(key) {
            return false;
        }
        let cap = cap.to_ascii_uppercase();
        let before = self.clone();
        let disproves = self
            .presumed
            .and_then(|layout| layout.cap(key))
            .is_some_and(|presumed| presumed != cap);
        if disproves {
            self.presumed = None;
        }
        if !disproves && binds::key_label(key) == cap.to_string() {
            self.learned.remove(&key);
        } else {
            self.learned.insert(key, cap);
        }
        for layout in Layout::ALL {
            if layout.tells().contains(&(key, cap)) {
                for &(k, c) in layout.caps() {
                    self.learned.entry(k).or_insert(c);
                }
            }
        }
        *self != before
    }

    /// Settings-file text: `KeyW=Z KeyA=Q`, empty on QWERTY. Presses only:
    /// what the language presumes is presumed again on the way back in.
    pub fn to_text(&self) -> String {
        self.learned
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
                caps.learned.insert(key, c.to_ascii_uppercase());
            }
        }
        caps
    }
}

/// The keys whose caps are worth learning: the bindable ones (the only
/// ones ever shown by name), less the digit rows, whose names are the
/// numbers whatever the unshifted cap says.
pub(super) fn learnable(key: KeyCode) -> bool {
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
///
/// The table is kept in memory whatever screen this is, but written out
/// only once a language has been taken - the first-run picker is chosen
/// on the settings file not existing yet, so a cap learned in the picker
/// must not be what creates it.
pub fn learn_keycaps(
    mut typed: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    screen: Res<State<crate::app::Screen>>,
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
    // Not on the picker: see [`crate::app::language::may_save`].
    if changed && crate::app::language::may_save(screen.get()) {
        settings.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::i18n::Lang;

    /// French spells the legends and the mnemonics AZERTY before a key has
    /// been touched: the first menu of a first run reads right.
    #[test]
    fn the_language_presumes_its_countrys_keyboard() {
        let mut caps = KeyCaps::default();
        caps.presume(Layout::of(Lang::Fr));
        assert_eq!(caps.legend("WASD bouger | IJKL"), "ZQSD bouger | IJKL");
        assert_eq!(caps.key_for('M'), KeyCode::Semicolon);
        assert!(caps.is_global(KeyCode::Semicolon));
        // Presumed, not learned: nothing was seen, so nothing is written.
        assert_eq!(caps.to_text(), "");

        let mut german = KeyCaps::default();
        german.presume(Layout::of(Lang::De));
        assert_eq!(german.label(KeyCode::KeyY), "Z");
        assert_eq!(german.label(KeyCode::Slash), "-");
        assert_eq!(
            german.legend("WASD"),
            "WASD",
            "QWERTZ leaves the block alone"
        );
        assert_eq!(german.key_for('M'), KeyCode::KeyM);

        let mut english = KeyCaps::default();
        english.presume(Layout::of(Lang::En));
        assert_eq!(english, KeyCaps::default());
    }

    /// The French speaker on a QWERTY board takes it back with one press,
    /// and the press is kept so the next launch does not guess again.
    #[test]
    fn one_press_disproves_the_languages_keyboard() {
        let mut caps = KeyCaps::default();
        caps.presume(Layout::of(Lang::Fr));
        assert!(caps.learn(KeyCode::KeyW, 'w'));
        assert_eq!(caps.presumed, None);
        assert_eq!(caps.label(KeyCode::KeyW), "W");
        assert_eq!(caps.key_for('M'), KeyCode::KeyM, "the mnemonics come home");
        assert_eq!(caps.legend("WASD"), "WASD");

        // Saved, reloaded, and presumed at again: the record stands.
        let reloaded = KeyCaps::parse(&caps.to_text());
        assert_eq!(reloaded.to_text(), "KeyW=W");
        let mut reloaded = reloaded;
        reloaded.presume(Layout::of(Lang::Fr));
        assert_eq!(reloaded.presumed, None);
        assert_eq!(reloaded.legend("WASD"), "WASD");
    }

    /// A board that has already shown itself to be another layout is not
    /// talked out of it by the language: the Swiss French player types on
    /// QWERTZ, and said so.
    #[test]
    fn a_learned_board_refuses_the_languages_guess() {
        let mut caps = KeyCaps::default();
        caps.learn(KeyCode::KeyZ, 'y');
        caps.presume(Layout::of(Lang::Fr));
        assert_eq!(caps.presumed, None);
        assert_eq!(caps.label(KeyCode::KeyY), "Z");
        assert_eq!(caps.legend("WASD"), "WASD");
    }

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
    /// whole board follows so the first legend is already right - this is
    /// the path for an AZERTY player reading the game in English.
    #[test]
    fn azerty_is_learned_from_one_press_and_respells_the_legends() {
        let mut caps = KeyCaps::default();
        assert!(caps.learn(KeyCode::KeyW, 'z'));
        assert_eq!(caps.label(KeyCode::KeyW), "Z");
        assert_eq!(caps.label(KeyCode::KeyA), "Q");
        assert_eq!(caps.label(KeyCode::Semicolon), "M");
        assert_eq!(caps.label(KeyCode::Slash), "!");
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

    /// A block has to name exactly as many keys as it spells letters, or
    /// the respelling would run out of keys and drop a letter on the
    /// floor - silently, in the one line every player reads first.
    #[test]
    fn every_block_spells_as_many_letters_as_it_names_keys() {
        for (spelling, keys) in BLOCKS {
            let letters = spelling.chars().filter(char::is_ascii_alphabetic).count();
            assert_eq!(letters, keys.len(), "{spelling}");
        }
        // Longest first, so "W/S" cannot be respelled inside a "WASD"
        // that has not been dealt with yet.
        let mut lengths: Vec<usize> = BLOCKS.iter().map(|(s, _)| s.len()).collect();
        let sorted = {
            let mut sorted = lengths.clone();
            sorted.sort_unstable_by(|a, b| b.cmp(a));
            sorted
        };
        lengths.dedup();
        assert_eq!(
            BLOCKS.iter().map(|(s, _)| s.len()).collect::<Vec<_>>(),
            sorted,
            "the blocks are not longest-first"
        );
    }

    /// The menu, settings and setup prompts open with "W/S" or "A/D" in
    /// every language, and they are the first line a player reads on any
    /// screen. A translation that spelled one differently would keep the
    /// QWERTY letters on an AZERTY board, so this pins all eight.
    #[test]
    fn every_language_names_the_menu_keys_by_their_qwerty_caps() {
        let mut azerty = KeyCaps::default();
        azerty.presume(Some(Layout::Azerty));
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            for prompt in [
                tr.menu_prompt,
                tr.prompt_new_version,
                tr.prompt_pick_language,
                tr.prompt_replays,
                tr.prompt_controls,
            ] {
                assert!(prompt.contains("W/S"), "{lang:?}: {prompt:?}");
                let respelled = azerty.legend(prompt);
                assert!(respelled.contains("Z/S"), "{lang:?}: {respelled:?}");
                assert!(!respelled.contains("W/S"), "{lang:?}: {respelled:?}");
            }
            for prompt in [tr.prompt_settings, tr.prompt_match_setup] {
                assert!(prompt.contains("A/D"), "{lang:?}: {prompt:?}");
                let respelled = azerty.legend(prompt);
                assert!(respelled.contains("Q/D"), "{lang:?}: {respelled:?}");
                assert!(!respelled.contains("A/D"), "{lang:?}: {respelled:?}");
            }
        }
    }

    /// The whole path an AZERTY player takes, without an AZERTY keyboard
    /// to take it on: what [`crate::app::keymap`] reads off the platform,
    /// handed to [`KeyCaps::adopt`], has to move the legends and the
    /// mnemonics exactly as a board full of presses would.
    #[test]
    fn a_keymap_read_from_the_platform_moves_everything() {
        // What an X11 `GetKeyboardMapping` answers on a French board, in
        // the order the query walks the keys. The dead keys (`^`) and the
        // caps outside ASCII (`ù`) are absent, because the query drops
        // them rather than guessing.
        let french = [
            (KeyCode::KeyA, 'q'),
            (KeyCode::KeyM, ','),
            (KeyCode::KeyQ, 'a'),
            (KeyCode::KeyW, 'z'),
            (KeyCode::KeyZ, 'w'),
            (KeyCode::Comma, ';'),
            (KeyCode::Period, ':'),
            (KeyCode::Slash, '!'),
            (KeyCode::Semicolon, 'm'),
            (KeyCode::Backslash, '*'),
            (KeyCode::Minus, ')'),
            (KeyCode::Equal, '='),
        ];
        let mut caps = KeyCaps::default();
        assert!(caps.adopt(&french));
        assert_eq!(caps.legend("WASD move | IJKL"), "ZQSD move | IJKL");
        assert_eq!(
            caps.legend("W/S: choose | A/D: adjust"),
            "Z/S: choose | Q/D: adjust"
        );
        assert_eq!(
            caps.key_for('M'),
            KeyCode::Semicolon,
            "mute moves to its cap"
        );
        assert!(caps.is_global(KeyCode::Semicolon));
        assert_eq!(caps.label(KeyCode::Slash), "!");
        // `Equal` says "=" on both boards, so it is not worth writing
        // down, and `KeyM` is a global key the caps table will not take.
        assert!(!caps.to_text().contains("Equal"), "{}", caps.to_text());
        assert!(!caps.to_text().contains("KeyM="), "{}", caps.to_text());

        // Plugged into a QWERTY board next: the same call takes it all
        // back, rather than leaving half a French keyboard behind.
        let us: Vec<(KeyCode, char)> = french
            .iter()
            .map(|&(key, _)| (key, binds::key_label(key).chars().next().unwrap()))
            .collect();
        assert!(caps.adopt(&us));
        assert_eq!(
            caps.legend("WASD move | W/S: choose"),
            "WASD move | W/S: choose"
        );
        assert_eq!(caps.key_for('M'), KeyCode::KeyM);
    }

    /// The row on the card outranks everything the game worked out, and
    /// hands it all straight back when it goes to Auto.
    #[test]
    fn a_named_keyboard_outranks_what_the_game_worked_out() {
        // A board the platform read as AZERTY, in a French-speaking game.
        let mut caps = KeyCaps::default();
        caps.presume(Some(Layout::Azerty));
        caps.adopt(&[(KeyCode::KeyW, 'z'), (KeyCode::Semicolon, 'm')]);
        assert_eq!(caps.legend("WASD | W/S"), "ZQSD | Z/S");

        // The player says it is QWERTY after all - a remote desktop, a
        // borrowed machine - and every cap goes back, including the ones
        // no press touched.
        caps.force(Some(Layout::Qwerty));
        assert_eq!(caps.legend("WASD | W/S"), "WASD | W/S");
        assert_eq!(caps.label(KeyCode::Semicolon), ";");
        assert_eq!(caps.key_for('M'), KeyCode::KeyM);
        assert!(!caps.is_global(KeyCode::Semicolon));

        // Naming a layout works the other way just as well, on a board
        // that never said a word.
        let mut fresh = KeyCaps::default();
        fresh.force(Some(Layout::Qwertz));
        assert_eq!(fresh.label(KeyCode::KeyY), "Z");
        assert_eq!(fresh.legend("WASD"), "WASD");

        // Back to Auto: the evidence was kept underneath the whole time,
        // so nothing has to be learned twice.
        caps.force(None);
        assert_eq!(caps.legend("WASD | W/S"), "ZQSD | Z/S");
        assert_eq!(caps.key_for('M'), KeyCode::Semicolon);
    }

    #[test]
    fn text_round_trips_and_parses_leniently() {
        let mut caps = KeyCaps::default();
        caps.learn(KeyCode::KeyW, 'z');
        caps.learn(KeyCode::Slash, ':');
        let text = caps.to_text();
        assert_eq!(KeyCaps::parse(&text), caps);
        assert_eq!(
            text,
            "Backslash=* BracketLeft=^ BracketRight=$ Comma=; KeyA=Q \
             KeyQ=A KeyW=Z KeyZ=W Minus=) Period=: Semicolon=M Slash=:"
        );
        let lenient = KeyCaps::parse("nonsense KeyW=Z NoSuchKey=Q KeyA=QQ Digit1=& KeyE=");
        let mut expected = KeyCaps::default();
        expected.learned.insert(KeyCode::KeyW, 'Z');
        assert_eq!(lenient, expected);
    }
}
