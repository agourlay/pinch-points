//! Ask the operating system what each key on the keyboard prints.
//!
//! A window toolkit gives you a key press as a *position* - the key under
//! QWERTY's W is `KeyW` on an AZERTY board too, where its cap reads "Z".
//! That is the right thing to bind to and the wrong thing to print on
//! screen, and a game that prints it anyway tells an AZERTY player to
//! press W when their W is somewhere else entirely. What is missing is
//! the other half: what does this key *say*?
//!
//! Every platform knows. X11 has had `GetKeyboardMapping` since the
//! beginning, Windows answers `MapVirtualKeyEx`, and macOS will run a key
//! through `UCKeyTranslate` against the current input source. This crate
//! asks all three the same question and answers in one vocabulary.
//!
//! ```no_run
//! for (key, cap) in pinch_keymap::query() {
//!     println!("{key} says {cap}");   // "KeyW says z" on AZERTY
//! }
//! ```
//!
//! Keys are named by their W3C UI Events `code`: `"KeyW"`, `"Semicolon"`.
//! That is the same vocabulary as a DOM `KeyboardEvent.code`, as winit's
//! `KeyCode` and as Bevy's, so the answer maps onto a toolkit's own key
//! type by its name alone.
//!
//! # Why this crate exists
//!
//! It carries the `unsafe` that two of those three answers need, so that
//! the program asking the question does not have to. Its sibling
//! [`pinch-points`] forbids unsafe code outright and calls in here
//! instead.
//!
//! [`pinch-points`]: https://crates.io/crates/pinch-points

#![deny(missing_docs)]
// The whole point of the crate: every unsafe block says why it is sound.
#![deny(clippy::undocumented_unsafe_blocks)]

/// Every key whose cap can move between layouts, and the number each
/// platform knows it by.
///
/// The middle column does two jobs: over this stretch of the keyboard the
/// PS/2 set 1 scancodes Windows reports and the evdev codes Linux uses are
/// the same numbers, and an X11 keycode is that number plus 8 (keycodes
/// below that are reserved). The last column is the Carbon virtual
/// keycode, which follows the shape of the original Apple keyboard and so
/// follows nothing else.
///
/// The digit row is deliberately absent. An AZERTY board prints `&é"'`
/// along the top and the keys are still called 1 to 4 by everyone
/// including the player, so reporting a cap there would be answering a
/// question nobody asked.
const KEYS: [(&str, u8, u8); 37] = [
    ("KeyA", 30, 0x00),
    ("KeyB", 48, 0x0B),
    ("KeyC", 46, 0x08),
    ("KeyD", 32, 0x02),
    ("KeyE", 18, 0x0E),
    ("KeyF", 33, 0x03),
    ("KeyG", 34, 0x05),
    ("KeyH", 35, 0x04),
    ("KeyI", 23, 0x22),
    ("KeyJ", 36, 0x26),
    ("KeyK", 37, 0x28),
    ("KeyL", 38, 0x25),
    ("KeyM", 50, 0x2E),
    ("KeyN", 49, 0x2D),
    ("KeyO", 24, 0x1F),
    ("KeyP", 25, 0x23),
    ("KeyQ", 16, 0x0C),
    ("KeyR", 19, 0x0F),
    ("KeyS", 31, 0x01),
    ("KeyT", 20, 0x11),
    ("KeyU", 22, 0x20),
    ("KeyV", 47, 0x09),
    ("KeyW", 17, 0x0D),
    ("KeyX", 45, 0x07),
    ("KeyY", 21, 0x10),
    ("KeyZ", 44, 0x06),
    ("Comma", 51, 0x2B),
    ("Period", 52, 0x2F),
    ("Slash", 53, 0x2C),
    ("Semicolon", 39, 0x29),
    ("Quote", 40, 0x27),
    ("BracketLeft", 26, 0x21),
    ("BracketRight", 27, 0x1E),
    ("Backslash", 43, 0x2A),
    ("Minus", 12, 0x1B),
    ("Equal", 13, 0x18),
    ("Backquote", 41, 0x32),
];

/// What the keyboard prints, unshifted, for every key this crate knows
/// and the platform will answer for.
///
/// Each pair is a W3C `code` and the single printable ASCII character on
/// that key's cap. A key is simply absent when there is nothing useful to
/// say about it: a dead key (AZERTY's `^`), a cap outside ASCII (`ö`,
/// `ù`), a platform with no way to ask, or a display server that will not
/// talk to us. So an empty answer means "no idea", never "a blank
/// keyboard", and the caller keeps whatever it believed before.
///
/// Only ASCII, on purpose. A Cyrillic or kana board carries the Latin
/// caps alongside its own and its players read those, so reporting `ц`
/// for `KeyW` would replace a legend everyone can read with one only some
/// can.
///
/// # Platform notes
///
/// - **Linux**: reads the X11 keyboard mapping over a connection of its
///   own. A Wayland session answers through XWayland; one without it
///   answers nothing.
/// - **macOS**: call this from the main thread. It asks for the keyboard
///   type through Carbon, which expects to be there.
/// - A layout switched after this returns is not noticed. Ask again.
#[must_use]
pub fn query() -> Vec<(&'static str, char)> {
    platform::query().unwrap_or_default()
}

/// Level one of group one is the unshifted cap, which is the one the
/// player reads. A Latin-1 keysym *is* its character, so the ASCII range
/// needs no table; anything else - a dead key's `dead_circumflex`, a
/// Cyrillic letter - falls out of the filter and is left unsaid.
#[cfg(target_os = "linux")]
mod platform {
    use super::KEYS;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt;

    /// X11 numbers the keys eight above evdev: 0 to 7 are reserved.
    const X11_OFFSET: u8 = 8;

    pub fn query() -> Option<Vec<(&'static str, char)>> {
        let (conn, _) = x11rb::connect(None).ok()?;
        let (first, last) = {
            let setup = conn.setup();
            (setup.min_keycode, setup.max_keycode)
        };
        let count = last.checked_sub(first)?.checked_add(1)?;
        let map = conn.get_keyboard_mapping(first, count).ok()?.reply().ok()?;
        let per_key = usize::from(map.keysyms_per_keycode);
        if per_key == 0 {
            return None;
        }
        let mut caps = Vec::new();
        for (key, evdev, _) in KEYS {
            let Some(row) = evdev
                .checked_add(X11_OFFSET)
                .filter(|code| (first..=last).contains(code))
                .and_then(|code| code.checked_sub(first))
                .map(usize::from)
            else {
                continue;
            };
            let Some(&keysym) = map.keysyms.get(row * per_key) else {
                continue;
            };
            if let Some(cap) = char::from_u32(keysym).filter(char::is_ascii_graphic) {
                caps.push((key, cap));
            }
        }
        (!caps.is_empty()).then_some(caps)
    }
}

/// Two calls into `user32`: the scancode becomes a virtual key under the
/// thread's current layout, and the virtual key becomes the character
/// printed on the cap. Windows raises the top bit of that answer for a
/// dead key, which is a cap with nothing to read on its own.
#[cfg(target_os = "windows")]
mod platform {
    use super::KEYS;

    /// `MAPVK_VSC_TO_VK_EX`, `MAPVK_VK_TO_CHAR`, and the flag the second
    /// raises on a dead key.
    const VSC_TO_VK_EX: u32 = 3;
    const VK_TO_CHAR: u32 = 2;
    const DEAD_KEY: u32 = 0x8000_0000;

    #[link(name = "user32")]
    unsafe extern "system" {
        /// Zero asks for the calling thread's own layout.
        #[link_name = "GetKeyboardLayout"]
        fn get_keyboard_layout(thread: u32) -> isize;
        #[link_name = "MapVirtualKeyExW"]
        fn map_virtual_key_ex_w(code: u32, translation: u32, layout: isize) -> u32;
    }

    pub fn query() -> Option<Vec<(&'static str, char)>> {
        // SAFETY: the one argument is a thread id, and zero is the
        // documented way to name the calling thread. Nothing is borrowed
        // and the handle is only ever passed back to `user32` below.
        let layout = unsafe { get_keyboard_layout(0) };
        if layout == 0 {
            return None;
        }
        let mut caps = Vec::new();
        for (key, scancode, _) in KEYS {
            // SAFETY: both arguments are plain integers and `layout` came
            // from the call above; the function reads no memory of ours
            // and returns a virtual key code or zero for "no such key".
            let virtual_key =
                unsafe { map_virtual_key_ex_w(u32::from(scancode), VSC_TO_VK_EX, layout) };
            if virtual_key == 0 {
                continue;
            }
            // SAFETY: as above, with a virtual key this time.
            let printed = unsafe { map_virtual_key_ex_w(virtual_key, VK_TO_CHAR, layout) };
            if printed & DEAD_KEY != 0 {
                continue;
            }
            if let Some(cap) = char::from_u32(printed).filter(char::is_ascii_graphic) {
                caps.push((key, cap));
            }
        }
        (!caps.is_empty()).then_some(caps)
    }
}

/// `UCKeyTranslate` against the current input source's layout data, which
/// is how macOS answers "what does this key display".
#[cfg(target_os = "macos")]
mod platform {
    use super::KEYS;
    use std::ffi::c_void;
    use std::os::raw::c_ulong;

    /// `kUCKeyActionDisplay`: what the cap shows, rather than what typing
    /// it would commit. With `kUCKeyTranslateNoDeadKeysMask`, so a dead
    /// key answers with nothing instead of arming itself.
    const ACTION_DISPLAY: u16 = 3;
    const NO_DEAD_KEYS: u32 = 1;

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        #[link_name = "kTISPropertyUnicodeKeyLayoutData"]
        static UNICODE_KEY_LAYOUT_DATA: *const c_void;
        #[link_name = "TISCopyCurrentKeyboardLayoutInputSource"]
        fn copy_current_keyboard_layout_input_source() -> *mut c_void;
        #[link_name = "TISGetInputSourceProperty"]
        fn get_input_source_property(source: *mut c_void, key: *const c_void) -> *mut c_void;
        #[link_name = "LMGetKbdType"]
        fn get_kbd_type() -> u8;
        #[link_name = "UCKeyTranslate"]
        fn key_translate(
            layout: *const c_void,
            virtual_key: u16,
            action: u16,
            modifiers: u32,
            keyboard_type: u32,
            options: u32,
            dead_key_state: *mut u32,
            max_len: c_ulong,
            len: *mut c_ulong,
            unicode: *mut u16,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        #[link_name = "CFDataGetBytePtr"]
        fn data_byte_ptr(data: *mut c_void) -> *const u8;
        #[link_name = "CFRelease"]
        fn release(item: *mut c_void);
    }

    pub fn query() -> Option<Vec<(&'static str, char)>> {
        // SAFETY: takes nothing. Named `Copy`, so the reference it returns
        // is ours to release, which every path below does exactly once.
        let source = unsafe { copy_current_keyboard_layout_input_source() };
        if source.is_null() {
            return None;
        }
        // SAFETY: `source` is non-null and alive until the release below,
        // and the key is the framework's own static. The result borrows
        // from `source` rather than being a reference of its own, so it
        // is not released separately.
        let data = unsafe { get_input_source_property(source, UNICODE_KEY_LAYOUT_DATA) };
        if data.is_null() {
            // SAFETY: `source` is the live reference from above, released
            // once, and never touched again.
            unsafe { release(source) };
            return None;
        }
        // SAFETY: `data` is the non-null CFData from the property. The
        // bytes belong to it, and it in turn to `source`, which outlives
        // every use of `layout` below.
        let layout = unsafe { data_byte_ptr(data) }.cast::<c_void>();
        // SAFETY: takes nothing and returns a small integer. Expects the
        // main thread, which `query`'s documentation asks the caller for.
        let keyboard_type = u32::from(unsafe { get_kbd_type() });
        let mut caps = Vec::new();
        for (key, _, virtual_key) in KEYS {
            let mut unicode = [0u16; 4];
            let mut len: c_ulong = 0;
            let mut dead_key_state: u32 = 0;
            // SAFETY: `layout` points at the live layout data. The two out
            // pointers are to locals, and `max_len` is the true length of
            // the buffer `unicode`, so nothing can be written past it.
            let status = unsafe {
                key_translate(
                    layout,
                    u16::from(virtual_key),
                    ACTION_DISPLAY,
                    0,
                    keyboard_type,
                    NO_DEAD_KEYS,
                    &raw mut dead_key_state,
                    unicode.len() as c_ulong,
                    &raw mut len,
                    unicode.as_mut_ptr(),
                )
            };
            // One character exactly: a cap that displays two is not one
            // anything on screen can stand in for.
            if status != 0 || len != 1 {
                continue;
            }
            if let Some(cap) = char::from_u32(u32::from(unicode[0])).filter(char::is_ascii_graphic)
            {
                caps.push((key, cap));
            }
        }
        // SAFETY: the live reference from the copy above, released once at
        // the end of the only scope that uses it.
        unsafe { release(source) };
        (!caps.is_empty()).then_some(caps)
    }
}

/// Everywhere else: nothing to ask, and the caller keeps what it had.
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
mod platform {
    pub fn query() -> Option<Vec<(&'static str, char)>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table is the contract with three platforms at once, and a
    /// number in the wrong row would put a cap on the wrong key with
    /// nothing to catch it.
    #[test]
    fn the_key_table_is_sound() {
        for column in 0..3 {
            let mut seen: Vec<String> = KEYS
                .iter()
                .map(|&(name, evdev, mac)| match column {
                    0 => name.to_string(),
                    1 => evdev.to_string(),
                    _ => mac.to_string(),
                })
                .collect();
            seen.sort();
            seen.dedup();
            assert_eq!(seen.len(), KEYS.len(), "column {column} names a key twice");
        }
        for (name, evdev, _) in KEYS {
            assert!(!name.is_empty() && name.is_ascii(), "{name}");
            assert!(evdev > 0, "{name} has no scancode");
        }
    }

    /// Whatever the platform answers has to keep the promises in
    /// [`query`]'s documentation, on whatever keyboard is running this.
    #[test]
    fn the_answer_keeps_its_promises() {
        for (key, cap) in query() {
            assert!(cap.is_ascii_graphic(), "{key} said {cap:?}");
            assert!(KEYS.iter().any(|&(k, ..)| k == key), "{key} is not a key");
        }
    }
}
