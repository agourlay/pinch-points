//! The keyboard layouts the game knows by name, as the caps each one puts
//! where QWERTY puts others, and which language presumes which.

use bevy::prelude::*;

use crate::app::i18n::Lang;

/// A keyboard layout the game knows by name, as the caps it puts where
/// QWERTY puts others.
///
/// Only the two layouts that move *letters* are here. The other languages
/// the game speaks are typed on boards whose letters sit exactly where
/// QWERTY puts them - Spanish, Italian and Dutch are QWERTY with their own
/// punctuation and accents, a Japanese JIS board carries the same Latin
/// letters, and a Russian ЙЦУКЕН board is dual-legend with QWERTY beneath
/// the Cyrillic - so for them there is nothing to presume and nothing that
/// would read wrong.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// The one every other table here is written against, and so the one
    /// with nothing of its own to say. Worth naming all the same: a
    /// player has to be able to point at it on the settings card and stop
    /// the game guessing.
    Qwerty,
    /// France and Belgium.
    Azerty,
    /// Germany, Austria and Switzerland.
    Qwertz,
}

/// What the keyboard row steps through: `None` is the game working it
/// out, and each layout after that is the player saying so outright.
impl crate::app::cycle::Cycle for Option<Layout> {
    const VARIANTS: &'static [Self] = &[
        None,
        Some(Layout::Qwerty),
        Some(Layout::Azerty),
        Some(Layout::Qwertz),
    ];
}

impl Layout {
    pub const ALL: [Layout; 3] = [Layout::Qwerty, Layout::Azerty, Layout::Qwertz];

    /// The name it goes by, spelled off its own top row. The same in
    /// every language, so it is not in the string tables.
    pub fn name(self) -> &'static str {
        match self {
            Layout::Qwerty => "QWERTY",
            Layout::Azerty => "AZERTY",
            Layout::Qwertz => "QWERTZ",
        }
    }

    /// Stable settings-file key.
    pub fn key(self) -> &'static str {
        match self {
            Layout::Qwerty => "qwerty",
            Layout::Azerty => "azerty",
            Layout::Qwertz => "qwertz",
        }
    }

    /// What `key` names, or `None` for "work it out" - which is also
    /// where "auto" and anything unreadable land, since a hand-edited
    /// file naming no layout is asking for the default rather than for a
    /// broken start-up.
    pub fn from_key(key: &str) -> Option<Layout> {
        Layout::ALL.into_iter().find(|layout| layout.key() == key)
    }

    /// The layout the country behind a language types on, where it is not
    /// QWERTY. This is the whole of what the language tells us about the
    /// keyboard, and it is a majority rather than a rule: a French speaker
    /// in Montréal types on QWERTY, which is why one press is enough to
    /// take it back.
    pub fn of(lang: Lang) -> Option<Layout> {
        match lang {
            Lang::Fr => Some(Layout::Azerty),
            Lang::De => Some(Layout::Qwertz),
            Lang::En | Lang::Es | Lang::It | Lang::Nl | Lang::Ru | Lang::Ja => None,
        }
    }

    /// Every cap this layout spells differently from QWERTY, in printable
    /// ASCII. The accented caps are left out - `ù`, `ö`, `ß` and their
    /// kin are not stored for a learned board either, so they are not
    /// presumed for a guessed one.
    pub fn caps(self) -> &'static [(KeyCode, char)] {
        match self {
            // Nothing. Every cap here is a difference from QWERTY, so
            // QWERTY differs from itself in no place at all - which is
            // exactly what makes it worth naming: choosing it clears
            // every cap the game thought it knew.
            Layout::Qwerty => &[],
            // ² & é " ' ( - è _ ç à ) =  /  a z e r t y u i o p ^ $
            // q s d f g h j k l m ù *    /  w x c v b n , ; : !
            // (the "," at QWERTY's M is left out: `learnable` refuses the
            // global keys, so no press could ever confirm or deny it)
            Layout::Azerty => &[
                (KeyCode::Backslash, '*'),
                (KeyCode::BracketLeft, '^'),
                (KeyCode::BracketRight, '$'),
                (KeyCode::Comma, ';'),
                (KeyCode::KeyA, 'Q'),
                (KeyCode::KeyQ, 'A'),
                (KeyCode::KeyW, 'Z'),
                (KeyCode::KeyZ, 'W'),
                (KeyCode::Minus, ')'),
                (KeyCode::Period, ':'),
                (KeyCode::Semicolon, 'M'),
                (KeyCode::Slash, '!'),
            ],
            // ^ 1 2 3 4 5 6 7 8 9 0 ß ´  /  q w e r t z u i o p ü +
            // a s d f g h j k l ö ä #    /  y x c v b n m , . -
            Layout::Qwertz => &[
                (KeyCode::Backquote, '^'),
                (KeyCode::Backslash, '#'),
                (KeyCode::BracketRight, '+'),
                (KeyCode::KeyY, 'Z'),
                (KeyCode::KeyZ, 'Y'),
                (KeyCode::Slash, '-'),
            ],
        }
    }

    /// The caps that give this layout away: press one and the board is
    /// this one, so the rest of [`Self::caps`] can be taken as read.
    ///
    /// A tell has to be unmistakable, which is less than "differs from
    /// QWERTY": a UK board says "#" on the key beside Enter, exactly as
    /// QWERTZ does, so that one names no layout. The letters do.
    pub fn tells(self) -> &'static [(KeyCode, char)] {
        match self {
            // Nothing gives QWERTY away, because QWERTY is what a board
            // is when nothing has given anything away.
            Layout::Qwerty => &[],
            Layout::Azerty => &[
                (KeyCode::KeyA, 'Q'),
                (KeyCode::KeyQ, 'A'),
                (KeyCode::KeyW, 'Z'),
                (KeyCode::KeyZ, 'W'),
                (KeyCode::Semicolon, 'M'),
            ],
            Layout::Qwertz => &[(KeyCode::KeyY, 'Z'), (KeyCode::KeyZ, 'Y')],
        }
    }

    pub(super) fn cap(self, key: KeyCode) -> Option<char> {
        self.caps().iter().find(|(k, _)| *k == key).map(|(_, c)| *c)
    }

    pub(super) fn key_for(self, letter: char) -> Option<KeyCode> {
        self.caps()
            .iter()
            .find(|(_, c)| *c == letter)
            .map(|(k, _)| *k)
    }
}

#[cfg(test)]
mod tests {
    use super::super::learnable;
    use super::*;
    use crate::app::binds;

    /// The caps a layout claims have to be caps the rest of the module
    /// would accept from a press, or a presumed board and a learned one
    /// would not read alike.
    #[test]
    fn every_layout_table_is_learnable_and_says_something_new() {
        for layout in Layout::ALL {
            let mut seen = Vec::new();
            for &(key, cap) in layout.caps() {
                assert!(cap.is_ascii_graphic(), "{layout:?} {key:?}");
                assert!(learnable(key), "{layout:?} {key:?}");
                assert_ne!(binds::key_label(key), cap.to_string(), "{layout:?}");
                assert!(!seen.contains(&key), "{layout:?} spells {key:?} twice");
                seen.push(key);
            }
            for tell in layout.tells() {
                assert!(layout.caps().contains(tell), "{layout:?} {tell:?}");
            }
        }
    }

    /// Every language answers for its country's keyboard, and the six that
    /// answer "QWERTY" do so by name rather than by falling through.
    #[test]
    fn every_language_names_its_keyboard() {
        assert_eq!(Layout::of(Lang::Fr), Some(Layout::Azerty));
        assert_eq!(Layout::of(Lang::De), Some(Layout::Qwertz));
        for lang in crate::app::i18n::ALL_LANGS {
            if matches!(lang, Lang::Fr | Lang::De) {
                continue;
            }
            assert_eq!(Layout::of(lang), None, "{lang:?}");
        }
    }
}
