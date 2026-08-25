//! Share codes at the keyboard: put one on the clipboard, take one off it.
//!
//! The codec is [`crate::share`]; this is the half that touches the machine.
//! It exists so the editor and the replay library ask the same question the
//! same way, and so the one awkward detail (that a clipboard read is a
//! promise on the web and an answer everywhere else) is written down once.

use crate::app::i18n::fill;
use crate::share::{self, Kind};
use bevy::prelude::*;

/// Put a payload on the clipboard as a share code. Returns the code, so a
/// screen can say how long it is: a level is a couple of lines and a round
/// is eight thousand characters, and knowing which you just copied is the
/// difference between pasting it into a message and pasting it into a file.
pub fn copy(clipboard: &mut Clipboard, kind: Kind, payload: &[u8]) -> Result<String, String> {
    let code = share::encode(kind, payload);
    clipboard
        .set_text(code.clone())
        .map(|()| code)
        .map_err(|e| e.to_string())
}

/// Copy a payload as a share code and say how it went: `ok` (with its `{n}`
/// slot filled by the code's length) on success, the shared failure
/// sentence otherwise. The one feedback for the one gesture: three screens
/// had each written their own copy of this match.
pub fn copy_feedback(
    clipboard: &mut Clipboard,
    tr: &crate::app::i18n::Tr,
    kind: Kind,
    payload: &[u8],
    ok: &str,
) -> String {
    copy_counted(clipboard, tr, kind, payload, ok).0
}

/// [`copy_feedback`], and whether the clipboard actually took it, for a
/// caller that wants to count the gesture and not merely report it. A
/// clipboard that refused is not a level shared, whatever the player
/// pressed.
pub fn copy_counted(
    clipboard: &mut Clipboard,
    tr: &crate::app::i18n::Tr,
    kind: Kind,
    payload: &[u8],
    ok: &str,
) -> (String, bool) {
    match copy(clipboard, kind, payload) {
        Ok(code) => (fill(ok, &[("n", &code.len().to_string())]), true),
        Err(e) => (fill(tr.code_copy_failed, &[("e", &e)]), false),
    }
}

/// Take a share code off the clipboard, if what is on it is one.
///
/// The read is answered immediately on every platform this game ships to;
/// on the web it would be a promise, and this returns `None` rather than
/// pretending to wait for it.
pub fn paste(clipboard: &mut Clipboard) -> Option<(Kind, Vec<u8>)> {
    let text = clipboard.fetch_text().poll_result()?.ok()?;
    share::decode(&text)
}

/// What to say when a paste did not work out, as one of the three things
/// that can go wrong: nothing readable on the clipboard, a code of the wrong
/// sort, or a code that will not load.
///
/// Kept apart from the screens because "that is a level, not a round" is the
/// message people actually need, and it is easy to collapse all three into
/// "bad code" and leave someone guessing.
pub fn wrong_kind(tr: &crate::app::i18n::Tr, wanted: Kind, got: Kind) -> String {
    let name = |kind: Kind| match kind {
        Kind::Beach => tr.code_kind_beach,
        Kind::Level => tr.code_kind_level,
        Kind::Round => tr.code_kind_round,
    };
    fill(
        tr.code_wrong_kind,
        &[("want", name(wanted)), ("got", name(got))],
    )
}

/// A pasted code's payload as text, if it is a code of the wanted sort, or
/// what to say about why not. The first three failure steps every paste
/// walks (nothing pasted, wrong kind, not text), written once; `bad` is the
/// screen's own "this build cannot read it" sentence, whose `{e}` slot (if
/// it has one) gets the detail. The screen keeps only its final parse.
pub fn payload_text(
    pasted: Option<(Kind, Vec<u8>)>,
    tr: &crate::app::i18n::Tr,
    want: Kind,
    bad: &str,
) -> Result<String, String> {
    let (kind, payload) = pasted.ok_or_else(|| tr.code_none_pasted.to_string())?;
    if kind != want {
        return Err(wrong_kind(tr, want, kind));
    }
    String::from_utf8(payload).map_err(|e| fill(bad, &[("e", &e.to_string())]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing here opens a [`Clipboard`]. With `system_clipboard` on, that
    /// resource is the *real* one, so a test that read and wrote it would
    /// sit on whatever the person running the suite had just copied, and
    /// two such tests would hand each other their payloads. The codec is tested
    /// in [`crate::share`]; what is worth testing here is the sentence a
    /// player gets when a paste is not what the screen wanted.
    #[test]
    fn the_wrong_sort_of_code_says_which_sort_it_is() {
        let complaint = wrong_kind(&crate::app::i18n::EN, Kind::Round, Kind::Level);
        assert!(complaint.contains("a level"), "{complaint}");
        assert!(complaint.contains("a round"), "{complaint}");
        assert!(!complaint.contains('{'), "every slot filled: {complaint}");
    }
}
