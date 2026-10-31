//! How wide a line draws in the interface's face, and the one cut a
//! name too long for its place is given.

/// The width DejaVu Sans Mono, the rows' face, draws each character it
/// carries at, in ems.
const MONO_EM: f32 = 0.602;

/// How wide the rows' face draws `ch`, in ems: [`MONO_EM`] for what
/// DejaVu carries, and a full em for the kana and kanji the Japanese face
/// draws behind it (`boot::teach_the_kanji_fallback`). Both faces are
/// monospace, so a line's width is the sum. `a_long_beach_name_is_cut_to_its_card`
/// holds both numbers to the shipped fonts' own bytes.
fn mono_em(ch: char) -> f32 {
    let wide = matches!(
        u32::from(ch),
        0x1100..=0x115F
            | 0x2E80..=0x303F
            | 0x3040..=0x30FF
            | 0x3100..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
    );
    match wide {
        true => 1.0,
        false => MONO_EM,
    }
}

/// How wide `text` draws at `font_px` in the interface's face, Japanese
/// falling back to its own as it does on screen ([`mono_em`]).
pub(crate) fn line_px(text: &str, font_px: f32) -> f32 {
    text.chars().map(mono_em).sum::<f32>() * font_px
}

/// `text` as it fits `room` pixels at `font_px` in the interface's face:
/// whole when it fits, else cut short with "…" where it would run past,
/// spaces before the "…" let go. The one cut a name too long for its
/// place is given, a beach's on its card or a player's on the line-up.
pub(crate) fn cut_to(text: &str, room: f32, font_px: f32) -> String {
    let room = room / font_px;
    let width: f32 = text.chars().map(mono_em).sum();
    if width <= room {
        return text.to_string();
    }
    let mut used = mono_em('…');
    let mut cut: String = text
        .chars()
        .take_while(|&ch| {
            used += mono_em(ch);
            used <= room
        })
        .collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}
