//! Measuring a line the way the shell draws it: which of the two shipped
//! faces takes each character, and how wide that face draws it.
//!
//! Test-only, and it exists because the cards lay their columns out in
//! pixels while the obvious way to guard one is to count characters. Those
//! are not the same question. The Japanese face draws a full em where
//! DejaVu Sans Mono draws 0.602, so a fifteen-character Japanese label is
//! not fifteen columns of anything, and the ratio between them - 1.66 - is
//! not a whole number of spaces either. A width guard written in `chars()`
//! passes a row that runs off the end of its cell and fails one that does
//! not.
//!
//! Which face takes a character is settled by what DejaVu has: every
//! `TextFont` in the game asks for DejaVu, and `boot::teach_the_kanji_fallback`
//! hangs the subset behind it for the Japanese scripts. So DejaVu draws
//! what DejaVu has, and the subset draws the rest.
//!
//! Both faces are monospace, which is what makes this a sum rather than a
//! shaping run. The advances are read out of the very bytes that ship
//! rather than written down here, so re-subsetting either one cannot leave
//! this quietly wrong.

use std::collections::BTreeSet;
use std::sync::LazyLock;

const UI_FACE: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSansMono.ttf");
const JP_FACE: &[u8] = include_bytes!("../../../assets/fonts/NotoSansMonoCJKjp-Subset.otf");

fn be16(font: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([font[at], font[at + 1]])
}

fn be32(font: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([font[at], font[at + 1], font[at + 2], font[at + 3]])
}

/// Where a table sits in the sfnt directory. Both a TrueType outline font
/// and a CFF one are laid out this way, which is why one reader serves the
/// `.ttf` and the `.otf` alike.
fn table(font: &[u8], tag: &[u8; 4]) -> usize {
    (0..be16(font, 4) as usize)
        .map(|index| 12 + index * 16)
        .find(|&record| &font[record..record + 4] == tag)
        .map(|record| be32(font, record + 8) as usize)
        .unwrap_or_else(|| panic!("no {} table", String::from_utf8_lossy(tag)))
}

/// Every character the face can draw, from its format 4 cmap.
///
/// Only what the shipped faces actually carry is parsed: the table
/// directory, and the format 4 subtable. A font with neither panics, which
/// is the right answer for one this cannot vouch for.
pub fn characters_in_font(font: &[u8]) -> BTreeSet<char> {
    let cmap = table(font, b"cmap");
    let subtable = (0..be16(font, cmap + 2) as usize)
        .map(|index| cmap + 4 + index * 8)
        .map(|record| cmap + be32(font, record + 4) as usize)
        .find(|&at| be16(font, at) == 4)
        .expect("the face has no format 4 cmap");
    let segments = be16(font, subtable + 6) as usize / 2;
    let ends = subtable + 14;
    let starts = ends + segments * 2 + 2;
    let deltas = starts + segments * 2;
    let ranges = deltas + segments * 2;
    let mut out = BTreeSet::new();
    for segment in 0..segments {
        let at = segment * 2;
        let (first, last) = (be16(font, starts + at), be16(font, ends + at));
        // The last segment is the 0xffff terminator, not a character.
        for code in first..=last.min(0xfffe) {
            let range_offset = be16(font, ranges + at);
            let glyph = if range_offset == 0 {
                code.wrapping_add(be16(font, deltas + at))
            } else {
                let index = ranges + at + range_offset as usize + (code - first) as usize * 2;
                match be16(font, index) {
                    0 => 0,
                    glyph => glyph.wrapping_add(be16(font, deltas + at)),
                }
            };
            if glyph != 0
                && let Some(ch) = char::from_u32(u32::from(code))
            {
                out.insert(ch);
            }
        }
    }
    out
}

/// The face's advance, as a fraction of the em.
///
/// `hmtx` stops early and every glyph past its end inherits the last entry,
/// so that entry is what all but a handful of reserved slots are drawn at.
/// Asserting it against `advanceWidthMax` is the monospace check: on a
/// proportional face the two differ and this measure would be a guess.
fn em_advance(font: &[u8]) -> f32 {
    let hhea = table(font, b"hhea");
    let widest = be16(font, hhea + 10);
    let metrics = be16(font, hhea + 34) as usize;
    let last = be16(font, table(font, b"hmtx") + (metrics - 1) * 4);
    assert_eq!(last, widest, "the face is not monospace");
    f32::from(widest) / f32::from(be16(font, table(font, b"head") + 18))
}

struct Face {
    covers: BTreeSet<char>,
    advance: f32,
}

static UI: LazyLock<Face> = LazyLock::new(|| Face {
    covers: characters_in_font(UI_FACE),
    advance: em_advance(UI_FACE),
});

static JP: LazyLock<Face> = LazyLock::new(|| Face {
    advance: em_advance(JP_FACE),
    covers: characters_in_font(JP_FACE),
});

/// How wide `line` draws at `font_px`, in the faces the shell installs.
///
/// A character neither face has draws as nothing at all - Bevy loads no
/// system fonts, so there is no font of last resort - and that is a hole in
/// the screen, not a width. It is counted at the UI face's advance so the
/// caller measuring a cell gets an answer, and
/// `the_japanese_font_carries_every_character_the_tables_use` is what
/// actually catches it.
pub fn text_px(line: &str, font_px: f32) -> f32 {
    line.chars()
        .map(|ch| {
            if UI.covers.contains(&ch) || !JP.covers.contains(&ch) {
                UI.advance
            } else {
                JP.advance
            }
        })
        .sum::<f32>()
        * font_px
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two numbers every width guard in the shell rests on. Written out
    /// here so that re-subsetting a face to a different em fails one small
    /// test with the reason in it, rather than a handful of card widths
    /// with no clue why.
    #[test]
    fn the_shipped_faces_measure_what_they_always_have() {
        assert!(
            (UI.advance - 1233.0 / 2048.0).abs() < 0.0001,
            "DejaVu now advances {}",
            UI.advance
        );
        assert!(
            (JP.advance - 1.0).abs() < 0.0001,
            "the Japanese subset now advances {}",
            JP.advance
        );
        // And the gap between them is why none of this can be done in
        // characters: it is not two, and it is not a whole number.
        let ratio = JP.advance / UI.advance;
        assert!((1.66..1.67).contains(&ratio), "ratio is {ratio}");
    }

    /// A line of Latin measures out of the UI face, a line of kana out of
    /// the subset, and a line of both out of one each.
    #[test]
    fn each_half_of_a_mixed_line_is_measured_in_its_own_face() {
        let px = |s| text_px(s, 100.0);
        assert!((px("ab") - 2.0 * 100.0 * 1233.0 / 2048.0).abs() < 0.01);
        assert!((px("かな") - 200.0).abs() < 0.01);
        assert!((px("あa") - (100.0 + 100.0 * 1233.0 / 2048.0)).abs() < 0.01);
        assert_eq!(px(""), 0.0);
    }

    /// The curly quotes German and Dutch write are above the Japanese
    /// tables' `\u{4ff}` cut but are drawn by DejaVu all the same. Measuring
    /// them at a full em would overstate two languages' rows by a third of
    /// a character each, which is exactly the kind of quiet wrongness a
    /// codepoint threshold buys.
    #[test]
    fn a_curly_quote_is_measured_in_the_face_that_draws_it() {
        for quote in ['\u{201c}', '\u{201d}', '\u{201e}'] {
            assert!(UI.covers.contains(&quote), "{quote:?}");
            assert_eq!(text_px(&quote.to_string(), 100.0), text_px("x", 100.0));
        }
    }
}
