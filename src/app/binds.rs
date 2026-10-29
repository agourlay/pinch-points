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

    /// What the action is for, which is the group the controls screen
    /// lists it under.
    pub fn group(self) -> ActionGroup {
        match self {
            Action::MoveUp | Action::MoveDown | Action::MoveLeft | Action::MoveRight => {
                ActionGroup::Move
            }
            Action::PlaceUp | Action::PlaceDown | Action::PlaceLeft | Action::PlaceRight => {
                ActionGroup::Place
            }
            Action::Remove | Action::ClearAll => ActionGroup::Posts,
        }
    }

    /// The action in the other of the move and place groups that faces
    /// the same way: move-up for place-up and back. It is its own inverse,
    /// and the two clearing-up actions have no partner, so they stay.
    pub fn mirrored(self) -> Action {
        match self {
            Action::MoveUp => Action::PlaceUp,
            Action::MoveDown => Action::PlaceDown,
            Action::MoveLeft => Action::PlaceLeft,
            Action::MoveRight => Action::PlaceRight,
            Action::PlaceUp => Action::MoveUp,
            Action::PlaceDown => Action::MoveDown,
            Action::PlaceLeft => Action::MoveLeft,
            Action::PlaceRight => Action::MoveRight,
            Action::Remove => Action::Remove,
            Action::ClearAll => Action::ClearAll,
        }
    }
}

/// The kinds of thing an action does: walk the cursor, put an arrow down,
/// or clear up after them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActionGroup {
    Move,
    Place,
    Posts,
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
                // Beside Num 0, which removes. Not Num Enter, which it was:
                // that is Enter to every menu and card, so P2 clearing
                // their arrows in the final rush could press straight
                // through the results card the frame it opened.
                KeyCode::NumpadDecimal,
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
/// do both jobs on one press, so none is offered. Escape is kept out the
/// same way, being how a capture is cancelled. The letters move with the
/// layout, so a key bound on one keyboard can be a global's cap on
/// another: there the global yields to the seat (see
/// [`crate::app::keycaps::KeyCaps::global_pressed`]).
///
/// Not here, on purpose: W and S, which walk the menus but are stock
/// bindings and only read as menu keys where the seats are not playing (the
/// menus, the lobby, a paused round's card); S also steps a replay's speed;
/// V pastes a code on the menu and the replay shelf and validates in the
/// editor; T opens the lobby's chat.
pub const GLOBAL_KEYS: [KeyCode; 6] = [
    KeyCode::KeyM,
    KeyCode::KeyH,
    KeyCode::KeyN,
    KeyCode::KeyP,
    KeyCode::KeyR,
    KeyCode::KeyC,
];

/// The same six as the letters on their caps: what the game actually
/// listens for, through [`crate::app::keycaps::KeyCaps::key_for`], since a
/// mnemonic belongs to its letter and not to a position. [`GLOBAL_KEYS`]
/// is where those letters sit on QWERTY.
pub const GLOBAL_LETTERS: [char; 6] = ['M', 'H', 'N', 'P', 'R', 'C'];

/// [`key_from_name`] for the global keys, which are not bindable and so
/// not in its catalogue.
pub fn global_key_from_name(name: &str) -> Option<KeyCode> {
    GLOBAL_KEYS.into_iter().find(|&key| key_name(key) == name)
}

/// The key `name` spells, bindable or global: every letter key a keyboard
/// layout is read over.
pub fn any_key_from_name(name: &str) -> Option<KeyCode> {
    key_from_name(name).or_else(|| global_key_from_name(name))
}

/// Keys a player may bind. Anything else is ignored during capture, which
/// keeps Escape (cancel), both Enters, the function keys, the
/// [`GLOBAL_KEYS`] and the odd media key from being swallowed by a rebind.
/// The menus read Num Enter as Enter (`menu_ui::enter`), so a seat's key
/// there would also confirm whatever card opened under it.
pub fn bindable(key: KeyCode) -> bool {
    !GLOBAL_KEYS.contains(&key) && BINDABLE.contains(&key)
}

/// The bindable keys in the order the controls screen offers them, used
/// to read a name back into a [`KeyCode`]. The one list [`bindable`]
/// answers from too, so the two can never disagree.
pub(crate) fn all_bindable() -> impl Iterator<Item = KeyCode> {
    BINDABLE
        .iter()
        .copied()
        .filter(|key| !GLOBAL_KEYS.contains(key))
}

/// Every key a binding may use, the [`GLOBAL_KEYS`] among them since they
/// sit in the alphabet; [`bindable`] takes those back out. KeyCode has no
/// iterator of its own, so the list is written down.
const BINDABLE: [KeyCode; 81] = [
    KeyCode::KeyA,
    KeyCode::KeyB,
    KeyCode::KeyC,
    KeyCode::KeyD,
    KeyCode::KeyE,
    KeyCode::KeyF,
    KeyCode::KeyG,
    KeyCode::KeyH,
    KeyCode::KeyI,
    KeyCode::KeyJ,
    KeyCode::KeyK,
    KeyCode::KeyL,
    KeyCode::KeyM,
    KeyCode::KeyN,
    KeyCode::KeyO,
    KeyCode::KeyP,
    KeyCode::KeyQ,
    KeyCode::KeyR,
    KeyCode::KeyS,
    KeyCode::KeyT,
    KeyCode::KeyU,
    KeyCode::KeyV,
    KeyCode::KeyW,
    KeyCode::KeyX,
    KeyCode::KeyY,
    KeyCode::KeyZ,
    KeyCode::Digit0,
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::ArrowUp,
    KeyCode::ArrowDown,
    KeyCode::ArrowLeft,
    KeyCode::ArrowRight,
    KeyCode::Numpad0,
    KeyCode::Numpad1,
    KeyCode::Numpad2,
    KeyCode::Numpad3,
    KeyCode::Numpad4,
    KeyCode::Numpad5,
    KeyCode::Numpad6,
    KeyCode::Numpad7,
    KeyCode::Numpad8,
    KeyCode::Numpad9,
    KeyCode::NumpadAdd,
    KeyCode::NumpadSubtract,
    KeyCode::NumpadMultiply,
    KeyCode::NumpadDivide,
    KeyCode::NumpadDecimal,
    KeyCode::Space,
    KeyCode::Tab,
    KeyCode::Backspace,
    KeyCode::Insert,
    KeyCode::Delete,
    KeyCode::Home,
    KeyCode::End,
    KeyCode::PageUp,
    KeyCode::PageDown,
    KeyCode::ShiftLeft,
    KeyCode::ShiftRight,
    KeyCode::ControlLeft,
    KeyCode::ControlRight,
    KeyCode::AltLeft,
    KeyCode::AltRight,
    KeyCode::Comma,
    KeyCode::Period,
    KeyCode::Slash,
    KeyCode::Semicolon,
    KeyCode::Quote,
    KeyCode::BracketLeft,
    KeyCode::BracketRight,
    KeyCode::Backslash,
    KeyCode::Minus,
    KeyCode::Equal,
    KeyCode::Backquote,
];

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

/// Keys that were once bindable and are not any more, with the key a
/// saved binding to one moves to. Num Enter was P2's stock clear-all;
/// read as unknown, it would have put a player's whole P2 layout back to
/// stock for the one key.
const RETIRED: [(&str, KeyCode); 1] = [("NumpadEnter", KeyCode::NumpadDecimal)];

/// Read one `keys_pN` line. A short line, an unknown key name, or a
/// duplicate leaves that seat on its defaults rather than half-bound.
/// The settings file reads through [`parse_seat_moved`], which also says
/// what it moved; this is that without the list, for the tests.
#[cfg(test)]
pub fn parse_seat(value: &str) -> Option<SeatBinds> {
    parse_seat_moved(value).map(|(seat, _)| seat)
}

/// [`parse_seat`], and which slots held a retired key and were moved.
///
/// A key moved to stands where the player never put it, so it gives way:
/// to the first key the seat leaves free here, and across the table in
/// [`rehome`], which only the reader of both seats can do.
pub fn parse_seat_moved(value: &str) -> Option<(SeatBinds, Vec<usize>)> {
    let names: Vec<&str> = value.split_whitespace().collect();
    if names.len() != ACTIONS {
        return None;
    }
    let mut keys = [KeyCode::Space; ACTIONS];
    let mut moved = Vec::new();
    for (slot, name) in names.iter().enumerate() {
        keys[slot] = match key_from_name(name) {
            Some(key) => key,
            None => {
                moved.push(slot);
                RETIRED
                    .iter()
                    .find(|(retired, _)| retired == name)
                    .map(|&(_, key)| key)?
            }
        };
    }
    for &slot in &moved {
        let taken = |key: KeyCode, keys: &[KeyCode; ACTIONS]| {
            keys.iter()
                .enumerate()
                .any(|(at, &k)| at != slot && k == key)
        };
        if taken(keys[slot], &keys) {
            keys[slot] = BINDABLE
                .into_iter()
                .find(|&key| !taken(key, &keys))
                .expect("more bindable keys than actions");
        }
    }
    let mut seen: Vec<String> = keys.iter().copied().map(key_name).collect();
    seen.sort();
    seen.dedup();
    (seen.len() == ACTIONS).then_some((SeatBinds(keys), moved))
}

/// Move a key a retired one was moved to off a key the other seat holds,
/// onto the first one nobody holds: two seats share one keyboard, and a
/// clash puts the whole table back to stock, the very loss the move was
/// for.
pub fn rehome(binds: &mut [SeatBinds; BOUND_SEATS], seat: usize, slot: usize) {
    let key = binds[seat].0[slot];
    let held = |key: KeyCode, binds: &[SeatBinds; BOUND_SEATS]| {
        binds.iter().enumerate().any(|(s, seat_binds)| {
            seat_binds
                .0
                .iter()
                .enumerate()
                .any(|(at, &k)| (s, at) != (seat, slot) && k == key)
        })
    };
    if held(key, binds) {
        let free = BINDABLE
            .into_iter()
            .find(|&key| !held(key, binds))
            .expect("more bindable keys than actions");
        binds[seat].0[slot] = free;
    }
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

    /// No key a menu reads as Enter or Escape is a seat's: pressed in
    /// play it would also confirm or close whatever card opened under it.
    #[test]
    fn menu_keys_are_not_bindable() {
        for key in crate::app::menu_ui::CLOSERS {
            assert!(!bindable(key), "{key:?}");
            assert_eq!(key_from_name(&key_name(key)), None, "{key:?}");
        }
    }

    /// A layout saved when Num Enter was P2's clear-all still loads, with
    /// that one key moved to the new stock one and the rest kept.
    #[test]
    fn a_saved_num_enter_moves_to_the_new_stock_key() {
        let old = "KeyU KeyK KeyJ KeyL Numpad8 Numpad5 Numpad4 Numpad6 Numpad0 NumpadEnter";
        let seat = parse_seat(old).expect("still loads");
        assert_eq!(seat.key(Action::ClearAll), KeyCode::NumpadDecimal);
        assert_eq!(seat.key(Action::MoveUp), KeyCode::KeyU, "the rest is kept");
        assert_eq!(
            default_binds()[1].key(Action::ClearAll),
            KeyCode::NumpadDecimal
        );
    }

    /// Num . was bindable before it was P2's stock clear-all: a player who
    /// had it on another action, on either seat, keeps their layout, and
    /// the moved key goes to one nobody holds.
    #[test]
    fn a_moved_num_enter_gives_way_to_a_key_already_held() {
        // On the same seat.
        let old = "KeyU KeyK KeyJ KeyL Numpad8 Numpad5 Numpad4 Numpad6 NumpadDecimal NumpadEnter";
        let (seat, moved) = parse_seat_moved(old).expect("still loads");
        assert_eq!(moved, vec![9]);
        assert_eq!(seat.key(Action::Remove), KeyCode::NumpadDecimal, "kept");
        assert_ne!(seat.key(Action::ClearAll), KeyCode::NumpadDecimal);
        // On the other seat.
        let old = "KeyU KeyK KeyJ KeyL Numpad8 Numpad5 Numpad4 Numpad6 Numpad0 NumpadEnter";
        let (p2, moved) = parse_seat_moved(old).expect("still loads");
        let mut p1 = default_binds()[0];
        p1.0[0] = KeyCode::NumpadDecimal;
        let mut binds = [p1, p2];
        assert!(!all_distinct(&binds), "the clash the move made");
        for slot in moved {
            rehome(&mut binds, 1, slot);
        }
        assert!(all_distinct(&binds), "{binds:?}");
        assert_eq!(binds[0].0[0], KeyCode::NumpadDecimal, "P1 keeps its key");
        assert_eq!(binds[1].key(Action::MoveUp), KeyCode::KeyU, "P2 its others");
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
