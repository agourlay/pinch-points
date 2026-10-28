//! What the letter keys say on *this* keyboard.
//!
//! Every binding is a Bevy [`KeyCode`], which is a physical position: the
//! key under the QWERTY "W" is `KeyW` on an AZERTY board too, where its cap
//! reads "Z". So the stock WASD already moves an AZERTY player on ZQSD,
//! but a legend that spells the QWERTY caps answers a press of "Z" with
//! "W", which reads as the game ignoring the keyboard in front of it.
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
//!    these live in the settings file, as one table.
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
use crate::app::cursor::LiveSeatKeys;
use crate::app::settings::GameSettings;

/// What this keyboard's caps say, where they differ from Bevy's QWERTY
/// spelling: what presses have shown, over what the language presumes.
///
/// A resource of its own rather than a field of [`GameSettings`]: it is
/// per-machine learned state, not a preference, and it changes on ordinary
/// keypresses, which inside the settings re-ran every settings reader. It
/// still travels in the same settings.txt, as the `keycaps:` line.
#[derive(Resource, Clone, PartialEq, Eq, Debug, Default)]
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
    /// whole answer for as long as it is set, rather than a cap over the
    /// top of the others: "this is an AZERTY board" is a statement about
    /// the whole board.
    ///
    /// Saved as its own setting rather than in the caps text, since it is a
    /// preference and the rest is evidence. The evidence goes on being
    /// collected underneath, so Auto comes back up to date.
    forced: Option<Layout>,
}

/// The key groups the legends name by their QWERTY caps, and the physical
/// keys they stand for, letter for letter.
///
/// The move blocks are the obvious pair, but "W/S" and "A/D" are read most
/// often: every menu, settings and setup prompt opens with one, so
/// unrespelled they make the first line on screen name the wrong keys.
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

/// The keyboard as the game reads it: the keys down this frame, and the
/// caps that say which key carries which letter. Read together by every
/// system that answers a mnemonic.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Keyboard<'w> {
    pub keys: Res<'w, ButtonInput<KeyCode>>,
    pub caps: Res<'w, KeyCaps>,
    /// What a global letter yields to: see [`KeyCaps::global_pressed`].
    pub live: Res<'w, LiveSeatKeys>,
}

impl Keyboard<'_> {
    /// Whether the global key whose cap says `letter` was just pressed,
    /// unless a seat playing here holds it: see [`KeyCaps::global_pressed`].
    pub fn global(&self, letter: char) -> bool {
        self.caps.global_pressed(&self.keys, letter, &self.live)
    }

    /// Whether the cap that says `letter` was just pressed: see
    /// [`KeyCaps::just_pressed`].
    pub fn just(&self, letter: char) -> bool {
        self.caps.just_pressed(&self.keys, letter)
    }
}

/// [`Keyboard`] for a system that also spends the press it answers (see
/// `menu_ui::spend`).
#[derive(bevy::ecs::system::SystemParam)]
pub struct KeyboardMut<'w> {
    pub keys: ResMut<'w, ButtonInput<KeyCode>>,
    pub caps: Res<'w, KeyCaps>,
    /// What a global letter yields to: see [`KeyCaps::global_pressed`].
    pub live: Res<'w, LiveSeatKeys>,
}

impl KeyboardMut<'_> {
    /// Whether the global key whose cap says `letter` was just pressed,
    /// unless a seat playing here holds it: see [`KeyCaps::global_pressed`].
    pub fn global(&self, letter: char) -> bool {
        self.caps.global_pressed(&self.keys, letter, &self.live)
    }

    /// Whether the cap that says `letter` was just pressed: see
    /// [`KeyCaps::just_pressed`].
    pub fn just(&self, letter: char) -> bool {
        self.caps.just_pressed(&self.keys, letter)
    }
}

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
    ///
    /// The QWERTY key is only a guess, and not one to make once its cap is
    /// known to say something else: on Dvorak `KeyP` says L, and P sent
    /// there would make the L key load the previous stage. Such a letter
    /// has no key until a press (or the platform) shows where it is.
    pub fn key_for(&self, letter: char) -> Option<KeyCode> {
        let letter = letter.to_ascii_uppercase();
        let named = self.forced.map_or_else(
            || {
                self.learned
                    .iter()
                    .find(|(_, c)| **c == letter)
                    .map(|(k, _)| *k)
                    .or_else(|| self.presumed.and_then(|layout| layout.key_for(letter)))
            },
            |forced| forced.key_for(letter),
        );
        named.or_else(|| {
            let name = format!("Key{letter}");
            binds::any_key_from_name(&name)
                .filter(|&key| self.cap(key).is_none_or(|cap| cap == letter))
        })
    }

    /// Whether the cap that says `letter` was just pressed. Not for the
    /// [`binds::GLOBAL_LETTERS`], which every reader takes through
    /// [`Self::global_pressed`].
    pub fn just_pressed(&self, keys: &ButtonInput<KeyCode>, letter: char) -> bool {
        debug_assert!(
            !binds::GLOBAL_LETTERS.contains(&letter.to_ascii_uppercase()),
            "{letter} is global: read it through Globals"
        );
        self.key_for(letter)
            .is_some_and(|key| keys.just_pressed(key))
    }

    /// Whether the global key whose cap says `letter` was just pressed,
    /// unless that physical key is bound to a seat playing here, in which
    /// case the press is the seat's.
    ///
    /// The globals are read by their cap, so which physical key they are
    /// moves with the layout: `;` bound to a seat on QWERTY is M on AZERTY,
    /// and without this every press of that seat key would mute the music
    /// too. The capture refuses a key that is global on the layout of the
    /// moment; this covers a binding made on another one. The binding is
    /// left where the player put it, and M stays on every other M.
    pub fn global_pressed(
        &self,
        keys: &ButtonInput<KeyCode>,
        letter: char,
        live: &LiveSeatKeys,
    ) -> bool {
        self.key_for(letter)
            .is_some_and(|key| keys.just_pressed(key) && !live.holds(key))
    }

    /// The keys just pressed whose cap says `cap`, for a symbol that
    /// moves between layouts: QWERTZ carries "+" beside P and "-" where
    /// QWERTY has "/", and AZERTY's "-" is on the 6 of its digit row.
    /// Only what this keyboard is known or presumed to say, so a QWERTY
    /// board answers nothing here and the caller names its QWERTY keys
    /// itself.
    pub fn pressed_caps<'a>(
        &'a self,
        keys: &'a ButtonInput<KeyCode>,
        cap: char,
    ) -> impl Iterator<Item = KeyCode> + 'a {
        keys.get_just_pressed()
            .copied()
            .filter(move |&key| self.cap(key) == Some(cap) || self.digit_sign(key) == Some(cap))
    }

    /// The sign on a digit-row key, for a layout that puts one there and
    /// that this keyboard reads as (its tells all say so). Never learned,
    /// since a press of the row is still read as its digit everywhere else.
    fn digit_sign(&self, key: KeyCode) -> Option<char> {
        Layout::ALL
            .into_iter()
            .filter(|layout| {
                layout
                    .tells()
                    .iter()
                    .all(|&(tell, said)| self.cap(tell) == Some(said))
            })
            .flat_map(Layout::digit_signs)
            .find(|(k, _)| *k == key)
            .map(|(_, sign)| *sign)
    }

    /// Whether `key` is one of the [`binds::GLOBAL_KEYS`] on this keyboard,
    /// read by its letter whatever the bindings say, so not a key to hand a
    /// seat. The stock check ([`binds::bindable`]) is by position; this one
    /// is by cap. A seat is offered only what passes both: Dvorak's R key
    /// says P and fails both, its J says H and fails this one, and its P
    /// (which says L) fails the stock one, since bindings are kept by
    /// position and on QWERTY that key is P again.
    pub fn is_global(&self, key: KeyCode) -> bool {
        binds::GLOBAL_LETTERS
            .iter()
            .any(|&l| self.key_for(l) == Some(key))
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
    /// Each pair goes through [`Self::learn`], a keymap read from the
    /// platform being the same kind of fact as a press: it overwrites a
    /// stale cap, forgets one back to its QWERTY spelling, and retires a
    /// presumption it disagrees with. Whether anything changed.
    ///
    /// The keys the platform would not answer for (a dead key, a cap
    /// outside ASCII) are absent from `keymap`, so what the presses and the
    /// language said about them still stands.
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
    /// Detection is right almost always, and "almost" covers remote
    /// desktops, KVMs, a borrowed machine and the odd locale nobody thought
    /// of. A player who can see the wrong letters must be able to say so.
    pub fn force(&mut self, layout: Option<Layout>) {
        self.forced = layout;
    }

    /// Take `layout` as this keyboard, until a press says otherwise.
    ///
    /// Called with the language's [`Layout::of`] whenever the language is
    /// set, which is also every load. A guess disproved in an earlier
    /// session is not made again: the press that disproved it is in
    /// `learned`, and any learned cap that disagrees refuses it.
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
    /// Whatever the key, the numpad's aside, the cap is taken off any
    /// other key that held it (see [`Self::evict`]).
    ///
    /// The exception is a press that disproves the presumed layout. That
    /// one is written down even though it agrees with QWERTY, being the
    /// only record that the guess was wrong: without it the language makes
    /// the same guess at the next launch.
    pub fn learn(&mut self, key: KeyCode, cap: char) -> bool {
        if !cap.is_ascii_graphic() {
            return false;
        }
        let cap = cap.to_ascii_uppercase();
        let before = self.clone();
        if is_numpad(key) {
            // The numpad repeats the main block's characters: its "-" and
            // "+" are on QWERTZ's Slash and BracketRight too, and its "*"
            // on AZERTY's Backslash, so a press here says nothing about
            // which main-block key carries them.
            return false;
        }
        if !learnable(key) {
            // Not a cap to write down (the digit row, a key no binding
            // names), but still the key that says it now: AZERTY's 6
            // typing "-" is the only word the board gives that QWERTZ's
            // Slash no longer does.
            self.evict(key, cap);
            return *self != before;
        }
        let disproves = self
            .presumed
            .and_then(|layout| layout.cap(key))
            .is_some_and(|presumed| presumed != cap);
        if disproves {
            self.presumed = None;
        }
        if !disproves && binds::key_label(key) == cap.to_string() {
            self.learned.remove(&key);
            self.evict(key, cap);
        } else {
            self.claim(key, cap);
        }
        for layout in Layout::ALL {
            if layout.tells().contains(&(key, cap)) {
                for &(k, c) in layout.caps() {
                    if !self.learned.contains_key(&k) {
                        self.claim(k, c);
                    }
                }
            }
        }
        *self != before
    }

    /// Write down that `key` says `cap`, and that no other key does.
    fn claim(&mut self, key: KeyCode, cap: char) {
        self.evict(key, cap);
        self.learned.insert(key, cap);
    }

    /// Forget `cap` on every key but `key`. One cap sits on one key, so
    /// another key still holding it is from a layout since switched away
    /// from, and [`Self::key_for`], which asks the learned table first,
    /// would go on sending a mnemonic there: Dvorak's N on `KeyL`
    /// outliving a switch back to QWERTY, so that moving right went on
    /// loading the next stage while the key that now says N did nothing.
    fn evict(&mut self, key: KeyCode, cap: char) {
        self.learned.retain(|&k, &mut c| k == key || c != cap);
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
            let key = binds::any_key_from_name(name);
            if let (Some(key), Some(c), None) = (key, chars.next(), chars.next())
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
/// ones ever shown by name) and the [`binds::GLOBAL_KEYS`], less the digit
/// rows, whose names are the numbers whatever the unshifted cap says.
///
/// The global positions are learned although no seat may bind them, since
/// a letter's cap can sit there on another layout: Dvorak and Colemak both
/// put P on QWERTY's R, and a cap never written down is a mnemonic
/// [`KeyCaps::key_for`] cannot find.
pub(super) fn learnable(key: KeyCode) -> bool {
    let name = binds::key_name(key);
    (binds::bindable(key) || binds::GLOBAL_KEYS.contains(&key))
        && key != KeyCode::Space
        && !name.starts_with("Digit")
        && !is_numpad(key)
}

/// A key on the numpad, which only ever repeats a character the main block
/// has: never a cap to learn, nor one to take off another key.
fn is_numpad(key: KeyCode) -> bool {
    binds::key_name(key).starts_with("Numpad")
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
    unasked: Res<crate::app::language::LanguageUnasked>,
    settings: Res<GameSettings>,
    mut caps: ResMut<KeyCaps>,
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
        // `ResMut` marks it changed, and everything that re-reads the
        // caps on a change would run for every keypress.
        let mut learned = caps.clone();
        if learned.learn(input.key_code, cap) {
            *caps = learned;
            changed = true;
        }
    }
    // Not before a language is taken: see [`crate::app::language::LanguageUnasked::may_save`].
    if changed && unasked.may_save() {
        settings.save(&caps);
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
        assert_eq!(caps.key_for('M'), Some(KeyCode::Semicolon));
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
        assert_eq!(german.key_for('M'), Some(KeyCode::KeyM));

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
        assert_eq!(
            caps.key_for('M'),
            Some(KeyCode::KeyM),
            "the mnemonics come home"
        );
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

    /// What the learner is told from one press of `key` producing `text`,
    /// with `held` down and `repeat` set as the window reports them.
    fn learned_from(key: KeyCode, text: &str, held: Option<KeyCode>, repeat: bool) -> String {
        use bevy::input::ButtonState;
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<crate::app::Screen>();
        app.insert_resource(State::new(crate::app::Screen::Settings));
        app.insert_resource(GameSettings::default());
        app.init_resource::<crate::app::language::LanguageUnasked>();
        app.init_resource::<KeyCaps>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.add_message::<KeyboardInput>();
        app.add_systems(Update, learn_keycaps);
        if let Some(held) = held {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(held);
        }
        app.world_mut().write_message(KeyboardInput {
            key_code: key,
            logical_key: Key::Character(text.into()),
            state: ButtonState::Pressed,
            text: Some(text.into()),
            repeat,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        app.world().resource::<KeyCaps>().label(key)
    }

    /// A plain press is a cap read off the keyboard. A held one repeating,
    /// one under Shift or AltGr (which is "!" or "€", not the cap), and a
    /// key that types more than one character teach nothing.
    #[test]
    fn only_a_plain_press_of_one_character_teaches_a_cap() {
        assert_eq!(learned_from(KeyCode::KeyW, "z", None, false), "Z");
        assert_eq!(
            learned_from(KeyCode::KeyW, "z", None, true),
            "W",
            "a repeat"
        );
        assert_eq!(
            learned_from(KeyCode::KeyW, "Z", Some(KeyCode::ShiftLeft), false),
            "W",
            "under Shift"
        );
        assert_eq!(
            learned_from(KeyCode::KeyE, "€", Some(KeyCode::AltRight), false),
            "E",
            "under AltGr"
        );
        assert_eq!(
            learned_from(KeyCode::KeyW, "zz", None, false),
            "W",
            "two characters"
        );
    }

    /// The mnemonics follow the cap: on AZERTY "M" is the key QWERTY calls
    /// Semicolon, and the key at QWERTY's M (which says ",") is not mute.
    #[test]
    fn mnemonics_follow_the_cap() {
        let caps = KeyCaps::default();
        assert_eq!(caps.key_for('M'), Some(KeyCode::KeyM));
        assert_eq!(caps.key_for('h'), Some(KeyCode::KeyH));
        assert!(caps.is_global(KeyCode::KeyM));
        assert!(!caps.is_global(KeyCode::Semicolon));

        let mut azerty = KeyCaps::default();
        azerty.learn(KeyCode::KeyW, 'z');
        assert_eq!(azerty.key_for('M'), Some(KeyCode::Semicolon));
        assert_eq!(azerty.key_for('A'), Some(KeyCode::KeyQ));
        assert_eq!(azerty.key_for('H'), Some(KeyCode::KeyH));
        assert!(azerty.is_global(KeyCode::Semicolon));
        // Its own position still refuses a seat: `bindable` says so.
        assert!(!binds::bindable(KeyCode::KeyM));

        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Semicolon);
        let none = LiveSeatKeys::default();
        assert!(azerty.global_pressed(&keys, 'M', &none));
        assert!(!caps.global_pressed(&keys, 'M', &none));
    }

    /// A seat key bound on QWERTY that AZERTY's caps read as a global
    /// letter (`;` is M) is the seat's while that seat plays: pressing it
    /// does not mute. The M cap nobody holds still does, and on the menus,
    /// where no seat is live, so does `;`.
    #[test]
    fn a_seat_key_that_is_a_global_cap_here_stays_the_seats() {
        use crate::app::Screen;
        use binds::Action;
        let mut settings = GameSettings::default();
        settings.binds[0].set(Action::ClearAll, KeyCode::Semicolon);
        let mut azerty = KeyCaps::default();
        azerty.presume(Some(Layout::Azerty));
        assert_eq!(azerty.key_for('M'), Some(KeyCode::Semicolon));
        let live = |screen| LiveSeatKeys::of(&settings, screen, false, []);

        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Semicolon);
        assert!(!azerty.global_pressed(&keys, 'M', &live(Screen::Puzzle)));
        assert!(!azerty.global_pressed(&keys, 'M', &live(Screen::Versus)));
        assert!(azerty.global_pressed(&keys, 'M', &live(Screen::Menu)));

        // Bound nowhere: the M cap mutes, seats or not.
        let stock = LiveSeatKeys::of(&GameSettings::default(), Screen::Versus, false, []);
        assert!(azerty.global_pressed(&keys, 'M', &stock));

        // Placing on IJKL, P1 reads keys its bindings do not name: a board
        // whose J says H gives a puzzle's hint nothing for that press.
        settings.commit = crate::app::settings::CommitScheme::Ijkl;
        let mut dvorak = KeyCaps::default();
        dvorak.learn(KeyCode::KeyJ, 'h');
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyJ);
        let puzzle = LiveSeatKeys::of(&settings, Screen::Puzzle, false, []);
        assert!(!dvorak.global_pressed(&keys, 'H', &puzzle));
    }

    /// In co-op the partner's cursor plays on the second seat's keys, so a
    /// global cap among them is the partner's: on Dvorak the L key reads
    /// N, and moving right must not load the next stage. The partner also
    /// stands the one-hand preset down, so P1's commit keys are the
    /// arrows again and IJKL is not read twice.
    #[test]
    fn a_coop_partners_keys_are_live_on_a_puzzle() {
        use crate::app::Screen;
        use crate::app::cursor::Cursor;
        let mut settings = GameSettings::default();
        settings.commit = crate::app::settings::CommitScheme::Ijkl;
        let mut dvorak = KeyCaps::default();
        dvorak.learn(KeyCode::KeyL, 'n');
        assert_eq!(dvorak.key_for('N'), Some(KeyCode::KeyL));
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyL);

        let pair = [Cursor::seated(0), Cursor::seated(1)];
        let coop = LiveSeatKeys::of(&settings, Screen::Puzzle, false, &pair);
        assert!(!dvorak.global_pressed(&keys, 'N', &coop));
        assert!(coop.holds(KeyCode::ArrowUp), "P1 commits on the arrows");

        // Alone on IJKL, L is P1's commit key: still not the global.
        let solo = LiveSeatKeys::of(&settings, Screen::Puzzle, false, &pair[..1]);
        assert!(!dvorak.global_pressed(&keys, 'N', &solo));
        assert!(!solo.holds(KeyCode::ArrowUp));
        // On the arrows with nobody at P2's keys, L is free for N.
        settings.commit = crate::app::settings::CommitScheme::Arrows;
        let solo = LiveSeatKeys::of(&settings, Screen::Puzzle, false, &pair[..1]);
        assert!(dvorak.global_pressed(&keys, 'N', &solo));
    }

    /// What the platform answers for the letter keys on a board that
    /// types `rows` (QWERTY's three letter rows, in their order), as
    /// [`KeyCaps::adopt`] takes it.
    fn letter_board(rows: [&str; 3]) -> Vec<(KeyCode, char)> {
        let qwerty = ["qwertyuiop", "asdfghjkl;", "zxcvbnm,./"];
        let mut board = Vec::new();
        for (spelled, typed) in qwerty.iter().zip(rows) {
            for (q, c) in spelled.chars().zip(typed.chars()) {
                let name = match q {
                    ';' => "Semicolon".to_string(),
                    ',' => "Comma".to_string(),
                    '.' => "Period".to_string(),
                    '/' => "Slash".to_string(),
                    q => format!("Key{}", q.to_ascii_uppercase()),
                };
                let key = binds::any_key_from_name(&name).unwrap();
                board.push((key, c));
            }
        }
        board
    }

    /// Dvorak and Colemak both put P on QWERTY's R, a global position
    /// itself: the P-labelled key is the previous stage, and the key at
    /// QWERTY's P (L on Dvorak, ";" on Colemak) is not. Every global
    /// letter, and every other mnemonic, is found by its cap however the
    /// global positions are shuffled, from the platform or from presses.
    #[test]
    fn global_letters_are_found_by_cap_on_dvorak_and_colemak() {
        use binds::GLOBAL_LETTERS;
        let dvorak = letter_board(["',.pyfgcrl", "aoeuidhtns", ";qjkxbmwvz"]);
        let colemak = letter_board(["qwfpgjluy;", "arstdhneio", "zxcvbkm,./"]);
        for (name, board) in [("dvorak", &dvorak), ("colemak", &colemak)] {
            let mut caps = KeyCaps::default();
            caps.adopt(board);
            // Each cap on the board answers for its own key, the global
            // letters among them.
            for &(key, cap) in board.iter() {
                let cap = cap.to_ascii_uppercase();
                if cap.is_ascii_alphabetic() {
                    assert_eq!(caps.key_for(cap), Some(key), "{name}: {cap}");
                }
                let global = GLOBAL_LETTERS.contains(&cap);
                assert_eq!(caps.is_global(key), global, "{name}: {key:?}");
            }
            assert_eq!(caps.key_for('P'), Some(KeyCode::KeyR), "{name}");
            let mut keys = ButtonInput::<KeyCode>::default();
            keys.press(KeyCode::KeyP);
            let none = LiveSeatKeys::default();
            assert!(!caps.global_pressed(&keys, 'P', &none), "{name}");
            keys.press(KeyCode::KeyR);
            assert!(caps.global_pressed(&keys, 'P', &none), "{name}");
            // Saved and loaded, the global positions' caps come back too.
            assert_eq!(KeyCaps::parse(&caps.to_text()), caps, "{name}");

            // Learned from presses the same: the P key pressed once.
            let mut pressed = KeyCaps::default();
            pressed.learn(KeyCode::KeyR, 'p');
            assert_eq!(pressed.key_for('P'), Some(KeyCode::KeyR), "{name}");
            assert!(!pressed.is_global(KeyCode::KeyP), "{name}");
        }

        // Only the key at QWERTY's P has been pressed so far: it is known
        // not to say P, so no key answers for P until one says it does.
        let mut partial = KeyCaps::default();
        partial.learn(KeyCode::KeyP, 'l');
        assert_eq!(partial.key_for('P'), None);
        assert_eq!(partial.key_for('L'), Some(KeyCode::KeyP));
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyP);
        assert!(!partial.global_pressed(&keys, 'P', &LiveSeatKeys::default()));

        // And back on QWERTY, one press of R or P takes P off QWERTY's R.
        for (key, typed) in [(KeyCode::KeyR, 'r'), (KeyCode::KeyP, 'p')] {
            let mut caps = KeyCaps::default();
            caps.adopt(&dvorak);
            caps.learn(key, typed);
            assert_eq!(caps.key_for(typed), Some(key));
        }
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

    /// A cap sits on one key: learning it somewhere takes it off the key
    /// that said it before, or the mnemonic stays on a layout the player
    /// has switched away from. Pressed on a key the table never keeps (a
    /// global letter), or on its own QWERTY key, it still moves.
    #[test]
    fn a_cap_learned_on_one_key_leaves_the_key_that_had_it() {
        let mut caps = KeyCaps::default();
        caps.learn(KeyCode::KeyL, 'n');
        assert_eq!(caps.key_for('N'), Some(KeyCode::KeyL));
        // Back on QWERTY: the press of N itself is not written down, being
        // what QWERTY spells that key.
        assert!(caps.learn(KeyCode::KeyN, 'n'));
        assert_eq!(caps.key_for('N'), Some(KeyCode::KeyN));
        assert_eq!(caps.label(KeyCode::KeyL), "L");

        // QWERTZ, then QWERTY: Z pressed on KeyZ frees KeyY of it.
        let mut caps = KeyCaps::default();
        caps.learn(KeyCode::KeyZ, 'y');
        assert_eq!(caps.key_for('Z'), Some(KeyCode::KeyY));
        caps.learn(KeyCode::KeyZ, 'z');
        assert_eq!(caps.key_for('Z'), Some(KeyCode::KeyZ));

        // And a cap moved between two kept keys is on the new one only.
        let mut caps = KeyCaps::default();
        caps.learn(KeyCode::KeyL, 'n');
        caps.learn(KeyCode::KeyB, 'n');
        assert_eq!(caps.key_for('N'), Some(KeyCode::KeyB));
        assert_eq!(caps.to_text(), "KeyB=N");
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

    /// The numpad's signs are the main block's again, so a press of one
    /// leaves the main-block key that carries the same sign alone: on
    /// QWERTZ the editor's dial reads "-" and "+" off Slash and
    /// BracketRight whatever the numpad is used for.
    #[test]
    fn a_numpad_press_leaves_the_main_blocks_caps_alone() {
        let mut qwertz = KeyCaps::default();
        qwertz.learn(KeyCode::KeyZ, 'y');
        assert!(!qwertz.learn(KeyCode::NumpadSubtract, '-'));
        assert!(!qwertz.learn(KeyCode::NumpadAdd, '+'));
        assert_eq!(qwertz.label(KeyCode::Slash), "-");
        assert_eq!(qwertz.label(KeyCode::BracketRight), "+");

        let mut azerty = KeyCaps::default();
        azerty.learn(KeyCode::KeyW, 'z');
        assert!(!azerty.learn(KeyCode::NumpadMultiply, '*'));
        assert!(!azerty.learn(KeyCode::NumpadDivide, '/'));
        assert_eq!(azerty.label(KeyCode::Backslash), "*");

        // The digit row still moves a sign: AZERTY's "-" on the 6 is the
        // only "-" once the board is switched away from QWERTZ.
        let mut switched = KeyCaps::default();
        switched.learn(KeyCode::KeyZ, 'y');
        assert!(switched.learn(KeyCode::Digit6, '-'));
        assert_eq!(switched.label(KeyCode::Slash), "/");
    }

    /// [`KeyCaps::legend`] respells the literal block names, so every
    /// language's legends have to name the blocks by those four letters
    /// exactly: "W-A-S-D" or "ZQSD" slips past it and teaches the wrong
    /// keys.
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
                tr.prompt_setup_swapped,
                tr.prompt_versus_short_swapped,
                tr.prompt_versus_local_swapped,
                tr.prompt_setup_coop_swapped,
                tr.ed_test_setup_coop_swapped,
                tr.val_wasd,
                tr.ed_prompt,
                lang.level_hint(crate::app::campaign::KEY_LESSON_LEVEL)
                    .unwrap(),
            ] {
                assert!(legend.contains("WASD"), "{lang:?}: {legend:?}");
                let respelled = azerty.legend(legend);
                assert!(respelled.contains("ZQSD"), "{lang:?}: {respelled:?}");
                assert!(!respelled.contains("WASD"), "{lang:?}: {respelled:?}");
            }
            assert!(tr.prompt_versus_local.contains("IJKL"), "{lang:?}");
            assert!(tr.val_ijkl.contains("IJKL"), "{lang:?}");
            assert!(tr.prompt_versus_local_swapped.contains("IJKL"), "{lang:?}");
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
    /// every language, and a translation that spelled one differently would
    /// keep the QWERTY letters on an AZERTY board. All eight are pinned.
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
            for prompt in [tr.prompt_settings, tr.prompt_setup_game] {
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
            Some(KeyCode::Semicolon),
            "mute moves to its cap"
        );
        assert!(caps.is_global(KeyCode::Semicolon));
        assert_eq!(caps.label(KeyCode::Slash), "!");
        // `Equal` says "=" on both boards, so it is not worth writing
        // down. `KeyM` is, though no seat may bind it: it is not M here.
        assert!(!caps.to_text().contains("Equal"), "{}", caps.to_text());
        assert!(caps.to_text().contains("KeyM=,"), "{}", caps.to_text());
        assert!(!caps.is_global(KeyCode::KeyM));

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
        assert_eq!(caps.key_for('M'), Some(KeyCode::KeyM));
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
        assert_eq!(caps.key_for('M'), Some(KeyCode::KeyM));
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
        assert_eq!(caps.key_for('M'), Some(KeyCode::Semicolon));
    }

    #[test]
    fn text_round_trips_and_parses_leniently() {
        let mut caps = KeyCaps::default();
        caps.learn(KeyCode::KeyW, 'z');
        // Taking ":" off Period, which the AZERTY fill gave it.
        caps.learn(KeyCode::Slash, ':');
        let text = caps.to_text();
        assert_eq!(KeyCaps::parse(&text), caps);
        assert_eq!(
            text,
            "Backslash=* BracketLeft=^ BracketRight=$ Comma=; KeyA=Q \
             KeyM=, KeyQ=A KeyW=Z KeyZ=W Minus=) Semicolon=M Slash=:"
        );
        let lenient = KeyCaps::parse("nonsense KeyW=Z NoSuchKey=Q KeyA=QQ Digit1=& KeyE=");
        let mut expected = KeyCaps::default();
        expected.learned.insert(KeyCode::KeyW, 'Z');
        assert_eq!(lenient, expected);
    }
}
