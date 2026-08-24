//! Names and lines of chat, as they travel.
//!
//! Fixed-width and NUL-padded, because [`NetMsg`] is `Copy` (the host
//! relays what it is told by copying it) and because a length prefix is
//! one more thing a stranger on the LAN could lie about. Everything read
//! back off the wire goes through here, so a name is tidied in exactly one
//! place no matter which message carried it.

/// A player name on the wire: UTF-8, NUL-padded, truncated to fit at a
/// character boundary. 24 bytes carries the full 12-char name cap for
/// every Latin-alphabet name; scripts wider than two bytes a character
/// lose tail characters, not validity. Fixed-size so [`NetMsg`] stays
/// `Copy`: the host relays messages by copying them.
pub const WIRE_NAME: usize = 24;
pub type WireName = [u8; WIRE_NAME];

/// A name into its wire form.
pub fn wire_name(name: &str) -> WireName {
    let mut out = [0u8; WIRE_NAME];
    let mut len = 0;
    for ch in name.chars() {
        let next = len + ch.len_utf8();
        if next > WIRE_NAME {
            break;
        }
        ch.encode_utf8(&mut out[len..next]);
        len = next;
    }
    out
}

/// A table of names into its wire form: one slot per seat, the slots
/// past the end of `names` left empty. A lobby table may be shorter than
/// the seats it will fill, and a per-seat one is exactly as long.
pub fn wire_table(names: &[String]) -> [WireName; crate::sim::MAX_PLAYERS] {
    std::array::from_fn(|i| {
        names
            .get(i)
            .map_or([0u8; WIRE_NAME], |name| wire_name(name))
    })
}

/// A wire table back into text, every slot through [`name_from_wire`]:
/// an empty slot is an empty name, which every screen reads as a seat
/// label.
pub fn table_from_wire(
    names: &[WireName; crate::sim::MAX_PLAYERS],
) -> [String; crate::sim::MAX_PLAYERS] {
    std::array::from_fn(|i| name_from_wire(&names[i]))
}

/// A line of lobby chat on the wire, in the same fixed-size, NUL-padded
/// form as a name and for the same reason: [`NetMsg`] stays `Copy`, which
/// is how the host relays one by copying it. 96 bytes carries the
/// full [`CHAT_CHARS`] cap for Latin text; wider scripts lose tail
/// characters, not validity.
pub const WIRE_CHAT: usize = 96;
pub type WireChat = [u8; WIRE_CHAT];

/// The most characters a line of chat keeps. Short on purpose: it is a
/// lobby, not a messaging app, and a line that fits the row is a line
/// everyone can read at a glance.
pub const CHAT_CHARS: usize = 48;

/// A line of chat into its wire form, truncated at a character boundary.
pub fn wire_chat(line: &str) -> WireChat {
    let mut out = [0u8; WIRE_CHAT];
    let mut len = 0;
    for ch in line.chars().take(CHAT_CHARS) {
        let next = len + ch.len_utf8();
        if next > WIRE_CHAT {
            break;
        }
        ch.encode_utf8(&mut out[len..next]);
        len = next;
    }
    out
}

/// A line of chat back into text, distrusting the sender: invalid UTF-8 is
/// dropped rather than replaced, and control characters are stripped,
/// since they are not text a child typed and a newline or an escape in a
/// UI label is nobody's idea of a good time.
///
/// Unlike a name this keeps `|` and `:`, which no save file will ever see
/// and which people actually type.
pub fn chat_from_wire(bytes: &WireChat) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(WIRE_CHAT);
    let Ok(text) = std::str::from_utf8(&bytes[..end]) else {
        return String::new();
    };
    text.chars()
        .filter(|ch| !ch.is_control())
        .take(CHAT_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// The most characters a name keeps coming off the wire: the same cap the
/// settings screen puts on typed names (`settings::NAME_MAX`), stated here
/// rather than imported because this layer knows nothing about menus.
const NAME_CHARS: usize = 12;

/// A wire name back into text, distrusting the sender: invalid UTF-8 is
/// dropped rather than replaced, control characters and the save-file
/// separators are stripped, and the length cap is the same one the
/// settings screen enforces on typed names.
pub fn name_from_wire(bytes: &WireName) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(WIRE_NAME);
    let Ok(text) = std::str::from_utf8(&bytes[..end]) else {
        return String::new();
    };
    text.chars()
        .filter(|&c| !c.is_control() && c != '|' && c != ':')
        .take(NAME_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod name_tests {
    use super::*;

    /// A name goes onto the wire in 24 bytes and comes back off it from a
    /// stranger. Wide scripts are cut at a character boundary so what
    /// comes back is always valid UTF-8; a name that fills all 24 bytes
    /// has no NUL terminator and still reads back whole; the save-file
    /// separators are stripped *before* the 12-character cap so a name
    /// padded with them cannot smuggle an empty one past it; and a name
    /// with nothing left after trimming is the empty string, which every
    /// caller treats as "no name given" (the roster stops at it, a table
    /// slot keeps what it had). The default name is the caller's business,
    /// not this layer's.
    #[test]
    fn a_wide_name_is_cut_at_a_character_and_read_back() {
        // Twelve three-byte characters is 36 bytes: only eight fit.
        let wide = "あ".repeat(12);
        let bytes = wire_name(&wide);
        assert!(!bytes.contains(&0), "eight fill the 24 bytes exactly");
        assert_eq!(name_from_wire(&bytes), "あ".repeat(8));
        // A two-byte character straddling the end is dropped whole.
        let bytes = wire_name(&format!("{}é", "a".repeat(23)));
        assert_eq!(bytes[23], 0, "no half of an 'é' on the wire");
        assert_eq!(name_from_wire(&bytes), "a".repeat(12), "then capped");
    }

    #[test]
    fn a_name_that_fills_the_wire_has_no_terminator_and_still_reads_back() {
        let full = "abcdefghijklmnopqrstuvwx";
        let bytes = wire_name(full);
        assert!(bytes.iter().all(|&b| b != 0), "every byte is a letter");
        assert_eq!(name_from_wire(&bytes), &full[..NAME_CHARS]);
    }

    #[test]
    fn separators_are_stripped_before_the_cap_and_nothing_left_is_empty() {
        assert_eq!(name_from_wire(&wire_name("|||||||||||||Alice")), "Alice");
        assert_eq!(name_from_wire(&wire_name("a:b|c")), "abc");
        assert_eq!(name_from_wire(&wire_name("  Bob  ")), "Bob");
        assert_eq!(name_from_wire(&wire_name("Bob\n\u{1b}")), "Bob");
        for blank in ["", "   ", "|||", ":::", "\t\n", " | : "] {
            assert_eq!(name_from_wire(&wire_name(blank)), "", "{blank:?}");
        }
        // Nonsense bytes are declined rather than replaced.
        let mut junk = [0xFFu8; WIRE_NAME];
        junk[WIRE_NAME - 1] = 0;
        assert_eq!(name_from_wire(&junk), "");
    }
}

#[cfg(test)]
mod chat_tests {
    use super::*;

    /// A line of chat comes off the wire from a child on the next machine,
    /// or, put plainly, from a stranger: it is capped, trimmed, and stripped
    /// of anything that is not text a person typed.
    #[test]
    fn a_line_of_chat_survives_the_trip_and_is_tidied_on_the_way() {
        for line in ["ready?", "wait for me!", "Anna: 3 > 2 | ok", "héllo"] {
            assert_eq!(chat_from_wire(&wire_chat(line)), line, "{line}");
        }
        // Newlines and escapes are not chat; they are ways to make a mess
        // of a UI label.
        assert_eq!(chat_from_wire(&wire_chat("one\ntwo\u{1b}[0m")), "onetwo[0m");
        // Trimmed at both ends, and empty is empty.
        assert_eq!(chat_from_wire(&wire_chat("   ")), "");
        assert_eq!(chat_from_wire(&wire_chat("  hi  ")), "hi");
        // Capped, at a character boundary, with no panic on the way.
        let long = "é".repeat(400);
        let short = chat_from_wire(&wire_chat(&long));
        assert!(short.chars().count() <= CHAT_CHARS, "{}", short.len());
        // And nonsense bytes are declined rather than replaced.
        let mut junk = [0xFFu8; WIRE_CHAT];
        junk[WIRE_CHAT - 1] = 0;
        assert_eq!(chat_from_wire(&junk), "");
    }
}
