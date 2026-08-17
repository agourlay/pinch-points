//! Keyboard bindings (spec §8.4 full remapping): which key does what, for
//! each of the two keyboard seats.
//!
//! The bindings are a flat array indexed by [`Action`], so the controls
//! screen, the settings file, and the input layer all walk the same list and
//! cannot drift apart. Keys are stored under their Bevy names (`KeyW`,
//! `ArrowUp`, `Numpad0`) and shown prettier than that.

use bevy::prelude::KeyCode;

/// Seats that answer to a keyboard; the seats past these two (P3 up to
/// P6, see [`crate::sim::MAX_PLAYERS`]) are gamepad-only.
pub const BOUND_SEATS: usize = 2;

/// One thing a seat can do with a key press.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    PlaceUp,
    PlaceDown,
    PlaceLeft,
    PlaceRight,
    Remove,
    ClearAll,
}

impl Action {
    pub const ALL: [Action; 10] = [
        Action::MoveUp,
        Action::MoveDown,
        Action::MoveLeft,
        Action::MoveRight,
        Action::PlaceUp,
        Action::PlaceDown,
        Action::PlaceLeft,
        Action::PlaceRight,
        Action::Remove,
        Action::ClearAll,
    ];

    /// Slot in a [`SeatBinds`] key list.
    pub fn index(self) -> usize {
        Action::ALL
            .iter()
            .position(|&a| a == self)
            .expect("every action is in ALL")
    }
}

pub const ACTIONS: usize = Action::ALL.len();

/// One seat's keys, in [`Action::ALL`] order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SeatBinds(pub [KeyCode; ACTIONS]);

impl SeatBinds {
    pub fn key(&self, action: Action) -> KeyCode {
        self.0[action.index()]
    }

    pub fn set(&mut self, action: Action, key: KeyCode) {
        self.0[action.index()] = key;
    }

    /// The stock layout for a seat (spec §8.2): seat 1 drives WASD plus the
    /// arrow keys, seat 2 the IJKL block plus the numpad.
    pub fn default_for(seat: usize) -> SeatBinds {
        if seat == 0 {
            SeatBinds([
                KeyCode::KeyW,
                KeyCode::KeyS,
                KeyCode::KeyA,
                KeyCode::KeyD,
                KeyCode::ArrowUp,
                KeyCode::ArrowDown,
                KeyCode::ArrowLeft,
                KeyCode::ArrowRight,
                KeyCode::Space,
                KeyCode::ShiftLeft,
            ])
        } else {
            SeatBinds([
                KeyCode::KeyI,
                KeyCode::KeyK,
                KeyCode::KeyJ,
                KeyCode::KeyL,
                KeyCode::Numpad8,
                KeyCode::Numpad5,
                KeyCode::Numpad4,
                KeyCode::Numpad6,
                KeyCode::Numpad0,
                KeyCode::NumpadEnter,
            ])
        }
    }
}

/// Every seat's bindings at their defaults.
pub fn default_binds() -> [SeatBinds; BOUND_SEATS] {
    std::array::from_fn(SeatBinds::default_for)
}

/// Which seat and action a key is already bound to, if any. Two seats share
/// one keyboard, so a clash across seats is just as broken as one within a
/// seat, so this checks the whole board.
pub fn conflict(binds: &[SeatBinds; BOUND_SEATS], key: KeyCode) -> Option<(usize, Action)> {
    for (seat, seat_binds) in binds.iter().enumerate() {
        for action in Action::ALL {
            if seat_binds.key(action) == key {
                return Some((seat, action));
            }
        }
    }
    None
}

/// Whether every action across every seat has a key to itself.
pub fn all_distinct(binds: &[SeatBinds; BOUND_SEATS]) -> bool {
    let mut names: Vec<String> = binds
        .iter()
        .flat_map(|seat| seat.0.iter().copied().map(key_name))
        .collect();
    let total = names.len();
    names.sort();
    names.dedup();
    names.len() == total
}

/// Keys the game reads whatever the bindings say, on screens where the
/// bindings are live: M toggles the music anywhere; on a puzzle H asks for
/// the hint, N and P step between stages and R restarts the run; in a
/// versus round C copies the round code. Bound to a seat, such a key would
/// do both jobs on one press - place a signpost *and* skip the stage - so
/// none of them is offered. Escape is kept out the same way (it is how a
/// capture is cancelled) but never sat in the list to begin with.
///
/// Not here, on purpose: W and S, which walk the menus but are stock
/// bindings, and only ever read as menu keys on screens where the seats
/// are not playing (the menus, the lobby, a paused round's card); S also
/// steps a replay's speed, while nobody is playing; V pastes a code on the
/// menu and the replay shelf and validates in the editor, T opens the
/// lobby's chat - none of them a play screen either.
pub const GLOBAL_KEYS: [KeyCode; 6] = [
    KeyCode::KeyM,
    KeyCode::KeyH,
    KeyCode::KeyN,
    KeyCode::KeyP,
    KeyCode::KeyR,
    KeyCode::KeyC,
];

/// Keys a player may bind. Anything outside this list is ignored during
/// capture, which keeps Escape (cancel), the [`GLOBAL_KEYS`] and the odd
/// media key from being swallowed by a rebind.
pub fn bindable(key: KeyCode) -> bool {
    #[allow(clippy::enum_glob_use)]
    use KeyCode::*;
    !GLOBAL_KEYS.contains(&key)
        && matches!(
            key,
            KeyA | KeyB
                | KeyC
                | KeyD
                | KeyE
                | KeyF
                | KeyG
                | KeyH
                | KeyI
                | KeyJ
                | KeyK
                | KeyL
                | KeyM
                | KeyN
                | KeyO
                | KeyP
                | KeyQ
                | KeyR
                | KeyS
                | KeyT
                | KeyU
                | KeyV
                | KeyW
                | KeyX
                | KeyY
                | KeyZ
                | Digit0
                | Digit1
                | Digit2
                | Digit3
                | Digit4
                | Digit5
                | Digit6
                | Digit7
                | Digit8
                | Digit9
                | ArrowUp
                | ArrowDown
                | ArrowLeft
                | ArrowRight
                | Numpad0
                | Numpad1
                | Numpad2
                | Numpad3
                | Numpad4
                | Numpad5
                | Numpad6
                | Numpad7
                | Numpad8
                | Numpad9
                | NumpadEnter
                | NumpadAdd
                | NumpadSubtract
                | NumpadMultiply
                | NumpadDivide
                | NumpadDecimal
                | Space
                | Tab
                | Backspace
                | Insert
                | Delete
                | Home
                | End
                | PageUp
                | PageDown
                | ShiftLeft
                | ShiftRight
                | ControlLeft
                | ControlRight
                | AltLeft
                | AltRight
                | Comma
                | Period
                | Slash
                | Semicolon
                | Quote
                | BracketLeft
                | BracketRight
                | Backslash
                | Minus
                | Equal
                | Backquote
        )
}

/// The list of bindable keys, used to read a name back into a [`KeyCode`].
/// Built from [`bindable`] so the two can never disagree.
fn all_bindable() -> impl Iterator<Item = KeyCode> {
    // KeyCode has no iterator, but every variant we accept is reachable
    // from this catalogue of candidates.
    CANDIDATES.iter().copied().filter(|&key| bindable(key))
}

/// Every key the game could conceivably offer; `bindable` picks from it.
const CANDIDATES: [KeyCode; 89] = {
    #[allow(clippy::enum_glob_use)]
    use KeyCode::*;
    [
        KeyA,
        KeyB,
        KeyC,
        KeyD,
        KeyE,
        KeyF,
        KeyG,
        KeyH,
        KeyI,
        KeyJ,
        KeyK,
        KeyL,
        KeyM,
        KeyN,
        KeyO,
        KeyP,
        KeyQ,
        KeyR,
        KeyS,
        KeyT,
        KeyU,
        KeyV,
        KeyW,
        KeyX,
        KeyY,
        KeyZ,
        Digit0,
        Digit1,
        Digit2,
        Digit3,
        Digit4,
        Digit5,
        Digit6,
        Digit7,
        Digit8,
        Digit9,
        ArrowUp,
        ArrowDown,
        ArrowLeft,
        ArrowRight,
        Numpad0,
        Numpad1,
        Numpad2,
        Numpad3,
        Numpad4,
        Numpad5,
        Numpad6,
        Numpad7,
        Numpad8,
        Numpad9,
        NumpadEnter,
        NumpadAdd,
        NumpadSubtract,
        NumpadMultiply,
        NumpadDivide,
        NumpadDecimal,
        Space,
        Tab,
        Backspace,
        Insert,
        Delete,
        Home,
        End,
        PageUp,
        PageDown,
        ShiftLeft,
        ShiftRight,
        ControlLeft,
        ControlRight,
        AltLeft,
        AltRight,
        Comma,
        Period,
        Slash,
        Semicolon,
        Quote,
        BracketLeft,
        BracketRight,
        Backslash,
        Minus,
        Equal,
        Backquote,
        Escape,
        Enter,
        F1,
        F2,
        F3,
        F4,
        F5,
    ]
};

/// The settings-file name for a key: Bevy's own spelling, which is stable
/// and unambiguous. [`key_from_name`] is its inverse.
pub fn key_name(key: KeyCode) -> String {
    format!("{key:?}")
}

pub fn key_from_name(name: &str) -> Option<KeyCode> {
    all_bindable().find(|&key| key_name(key) == name)
}

/// How a key reads on screen: "KeyW" is a spelling, "W" is a keycap.
pub fn key_label(key: KeyCode) -> String {
    let name = key_name(key);
    // The punctuation keys are printed on the cap, so print them here too.
    // Spelling one out ("Backquote") asks the player to translate.
    for (spelling, cap) in [
        ("Comma", ","),
        ("Period", "."),
        ("Slash", "/"),
        ("Backslash", "\\"),
        ("Semicolon", ";"),
        ("Quote", "'"),
        ("BracketLeft", "["),
        ("BracketRight", "]"),
        ("Minus", "-"),
        ("Equal", "="),
        ("Backquote", "`"),
    ] {
        if name == spelling {
            return cap.to_string();
        }
    }
    // Keycaps do not say "Key" or "Arrow"; the numpad keys do want saying.
    for (prefix, cap) in [
        ("Key", ""),
        ("Digit", ""),
        ("Arrow", ""),
        ("Numpad", "Num "),
    ] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return format!("{cap}{rest}");
        }
    }
    // Bevy names the modifiers side-last ("ShiftLeft"); a keyboard names
    // them side-first, and the default clear-all binding puts one of these
    // on the controls screen for every player to read.
    for side in ["Left", "Right"] {
        if let Some(rest) = name.strip_suffix(side) {
            return format!("{side} {rest}");
        }
    }
    // Two-word names run together the same way.
    for (run_on, spaced) in [("PageUp", "Page Up"), ("PageDown", "Page Down")] {
        if name == run_on {
            return spaced.to_string();
        }
    }
    name
}

/// The whole binding table as settings-file text, one line per seat.
pub fn to_text(binds: &[SeatBinds; BOUND_SEATS]) -> String {
    let mut out = String::new();
    for (seat, seat_binds) in binds.iter().enumerate() {
        let names: Vec<String> = seat_binds.0.iter().copied().map(key_name).collect();
        out.push_str(&format!("keys_p{}: {}\n", seat + 1, names.join(" ")));
    }
    out
}

/// Read one `keys_pN` line. A short line, an unknown key name, or a
/// duplicate leaves that seat on its defaults rather than half-bound.
pub fn parse_seat(value: &str) -> Option<SeatBinds> {
    let names: Vec<&str> = value.split_whitespace().collect();
    if names.len() != ACTIONS {
        return None;
    }
    let mut keys = [KeyCode::Space; ACTIONS];
    for (slot, name) in keys.iter_mut().zip(&names) {
        *slot = key_from_name(name)?;
    }
    let mut seen: Vec<String> = keys.iter().copied().map(key_name).collect();
    seen.sort();
    seen.dedup();
    (seen.len() == ACTIONS).then_some(SeatBinds(keys))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names_round_trip() {
        for key in all_bindable() {
            assert_eq!(key_from_name(&key_name(key)), Some(key), "{key:?}");
        }
        assert_eq!(key_from_name("NoSuchKey"), None);
        // Escape stays unbindable: it is how you back out of a capture.
        assert!(!bindable(KeyCode::Escape));
        assert_eq!(key_from_name("Escape"), None);
    }

    /// A key the game reads on its own during play cannot also be a
    /// seat's, or one press would do two things.
    #[test]
    fn global_keys_are_not_bindable() {
        for key in GLOBAL_KEYS {
            assert!(!bindable(key), "{key:?} is read whatever the bindings say");
            assert_eq!(key_from_name(&key_name(key)), None, "{key:?}");
        }
        assert!(all_bindable().all(|key| !GLOBAL_KEYS.contains(&key)));
        // And the stock layouts never used one, or the game would refuse
        // its own defaults.
        for seat in default_binds() {
            for action in Action::ALL {
                assert!(bindable(seat.key(action)), "{action:?}");
            }
        }
    }

    #[test]
    fn labels_read_like_keycaps() {
        assert_eq!(key_label(KeyCode::KeyW), "W");
        assert_eq!(key_label(KeyCode::Digit4), "4");
        assert_eq!(key_label(KeyCode::ArrowUp), "Up");
        assert_eq!(key_label(KeyCode::Numpad8), "Num 8");
        assert_eq!(key_label(KeyCode::ShiftLeft), "Left Shift");
        assert_eq!(key_label(KeyCode::ControlRight), "Right Control");
        assert_eq!(key_label(KeyCode::Backquote), "`");
        assert_eq!(key_label(KeyCode::PageUp), "Page Up");
    }

    /// The two keyboard seats share a keyboard, so no key may do two jobs.
    #[test]
    fn the_default_layout_has_no_clashes() {
        let binds = default_binds();
        for seat in 0..BOUND_SEATS {
            for action in Action::ALL {
                assert_eq!(
                    conflict(&binds, binds[seat].key(action)),
                    Some((seat, action)),
                    "{seat} {action:?} is claimed by someone else"
                );
            }
        }
        assert_eq!(conflict(&binds, KeyCode::F5), None);
    }

    #[test]
    fn bindings_round_trip_through_text() {
        let mut binds = default_binds();
        binds[0].set(Action::Remove, KeyCode::Backquote);
        binds[1].set(Action::MoveUp, KeyCode::Home);
        let text = to_text(&binds);
        for (seat, line) in text.lines().enumerate() {
            let (_, value) = line.split_once(':').expect("key: value");
            assert_eq!(parse_seat(value), Some(binds[seat]));
        }
    }

    /// A half-written or self-contradicting line is refused outright, so a
    /// hand-edited file can never leave a seat unable to move.
    #[test]
    fn broken_binding_lines_are_refused() {
        assert_eq!(parse_seat("KeyW KeyS"), None, "too few keys");
        assert_eq!(parse_seat("Bogus ".repeat(10).trim()), None, "unknown key");
        let doubled = "KeyW KeyW KeyA KeyD ArrowUp ArrowDown ArrowLeft ArrowRight Space ShiftLeft";
        assert_eq!(parse_seat(doubled), None, "one key, two jobs");
    }
}
