//! The release notes as the page shows them: GitHub's markdown flattened
//! to plain lines, wrapped to the card, and cut to what the frame has
//! room for.

/// The release notes block: how many characters across it wraps at, and
/// how many lines it runs to before "…". Sixty-four monospaced characters
/// at the notes' size is about the width the menu's own card runs to.
pub const NOTES_COLS: usize = 64;
pub const NOTES_MAX_LINES: usize = 12;

/// The page's height in UI pixels apart from the notes: the two lines
/// over them, the question and two rows under them, the card's padding
/// and the note beneath the card. What is left of the frame between the
/// bars is the notes' to fill, [`NOTES_LINE_H`] per line.
const PAGE_FIXED_H: f32 = 300.0;
const NOTES_LINE_H: f32 = 15.0;

/// How many note lines fit a frame this many UI pixels tall: the cap,
/// less on a short frame or a big UI scale, never fewer than two, so the
/// question and its answers stay off the header and the prompt.
pub fn notes_budget(frame_h: f32) -> usize {
    let room = ((frame_h - PAGE_FIXED_H) / NOTES_LINE_H).floor();
    if room.is_nan() {
        return 2;
    }
    // A negative room floors to nothing and a boundless one saturates;
    // the clamp answers for both.
    (room.max(0.0) as usize).clamp(2, NOTES_MAX_LINES)
}

/// The release notes as the page shows them: the markdown flattened to
/// plain lines, wrapped to [`NOTES_COLS`], and cut at `max_lines` (see
/// [`notes_budget`]) with an ellipsis. Empty notes read as one empty
/// list, and the page says so in words instead.
pub fn notes_lines(notes: &str, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in notes.lines() {
        let line = flatten_markdown(raw);
        // Collapse runs of blank lines, and never open on one.
        if line.is_empty() {
            if lines.last().is_some_and(|last| !last.is_empty()) {
                lines.push(String::new());
            }
            continue;
        }
        for wrapped in wrap(&line, NOTES_COLS) {
            lines.push(wrapped);
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines.saturating_sub(1));
        lines.push("…".to_string());
    }
    lines
}

/// One line of markdown as plain text: headings lose their hashes,
/// bullets become bullets, emphasis and code marks go, links keep their
/// words, HTML tags go (GitHub's own notes open with a comment and like a
/// `<details>`), and a rule is nothing at all.
fn flatten_markdown(raw: &str) -> String {
    let untagged = strip_tags(raw);
    let line = untagged.trim_end();
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    if trimmed.chars().all(|c| c == '-' || c == '*' || c == '=') {
        return String::new();
    }
    let (lead, body) = if let Some(rest) = trimmed.strip_prefix('#') {
        (String::new(), rest.trim_start_matches('#').trim_start())
    } else if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        // Nested bullets keep their indent, halved: two spaces per level
        // is how the source is usually written and four is a lot of card.
        (format!("{}• ", " ".repeat(indent / 2)), rest)
    } else {
        (String::new(), trimmed)
    };
    let mut out = lead;
    let mut rest = body;
    // `[words](url)` keeps the words, `![alt](url)` its alt. Found from
    // the `](` and back to the nearest `[`, so `[#12] and [docs](url)`
    // keeps its first bracket. Anything else with a bracket in it goes
    // through as written.
    while let Some(join) = rest.find("](") {
        let Some(open) = rest[..join].rfind('[') else {
            out.push_str(&rest[..join + 2]);
            rest = &rest[join + 2..];
            continue;
        };
        let Some(close) = rest[join + 2..].find(')') else {
            break;
        };
        out.push_str(rest[..open].strip_suffix('!').unwrap_or(&rest[..open]));
        out.push_str(&rest[open + 1..join]);
        rest = &rest[join + 2 + close + 1..];
    }
    out.push_str(rest);
    out.replace("**", "").replace('`', "")
}

/// A line with its `<tags>` taken out: `<b>`, `</details>`, `<!-- -->`.
///
/// Only what reads as a tag. A closing `</…>` or a comment `<!…>` always
/// is; an opening one is a lowercase name standing on its own - after a
/// space or at the start, never glued to a word - and closed on the line.
/// So `Res<Time>` and `Vec<usize>` keep their arguments (this is a Rust
/// project's release notes), `a < b` keeps its sign, and an autolink
/// `<https://…>` keeps its address.
fn strip_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    // What stood just before `rest` in the line: `>` once a tag has been
    // taken out, so `</h2><br/>` reads as two tags, not one glued word.
    let mut before: Option<char> = None;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let glued = rest[..open]
            .chars()
            .last()
            .or(before)
            .is_some_and(char::is_alphanumeric);
        rest = &rest[open..];
        let Some(close) = rest.find('>') else {
            break;
        };
        let inner = &rest[1..close];
        let name = inner.split([' ', '\t', '/']).next().unwrap_or("");
        let tag_like = inner.starts_with('/')
            || inner.starts_with('!')
            || (!glued
                && !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-' || c.is_ascii_digit()));
        let autolink =
            (inner.starts_with("https://") || inner.starts_with("http://")) && !inner.contains(' ');
        if autolink {
            out.push_str(inner);
        } else if !tag_like {
            out.push('<');
            out.push_str(inner);
            out.push('>');
        }
        before = Some('>');
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

/// Word-wrap one line at `cols` characters, breaking a longer word where
/// it must. Continuation lines carry the bullet's indent.
fn wrap(line: &str, cols: usize) -> Vec<String> {
    let lead: String = line.chars().take_while(|&c| c == ' ' || c == '•').collect();
    let indent = " ".repeat(lead.chars().count());
    let mut out = Vec::new();
    let mut current = lead.clone();
    // Nothing but the lead on this line yet.
    let mut fresh = true;
    for mut word in line[lead.len()..].split(' ') {
        while !word.is_empty() {
            let used = current.chars().count() + usize::from(!fresh);
            if used + word.chars().count() <= cols {
                if !fresh {
                    current.push(' ');
                }
                current.push_str(word);
                fresh = false;
                break;
            }
            if !fresh {
                out.push(std::mem::replace(&mut current, indent.clone()));
                fresh = true;
                continue;
            }
            // Wider than the line on its own: cut it.
            let room = cols.saturating_sub(current.chars().count()).max(1);
            let cut = word.char_indices().nth(room).map_or(word.len(), |(i, _)| i);
            current.push_str(&word[..cut]);
            out.push(std::mem::replace(&mut current, indent.clone()));
            word = &word[cut..];
        }
    }
    if !fresh {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The notes read as plain lines: headings, bullets, emphasis, links
    /// and rules all come out as words, blank runs collapse, and the
    /// block ends with an ellipsis when there is more.
    #[test]
    fn release_notes_flatten_to_plain_lines() {
        let notes = "<!-- Release notes generated using ... -->\r\n## What's new\r\n\r\n\r\n- A **big** one, see [the docs](https://x/y).\r\n  - nested `code`\r\n\r\n---\r\n<details><summary>more</summary>\r\n**Full Changelog**: https://x/compare/a...b\r\n</details>\r\n\r\n";
        assert_eq!(
            notes_lines(notes, 12),
            vec![
                "What's new",
                "",
                "• A big one, see the docs.",
                " • nested code",
                "",
                "more",
                "Full Changelog: https://x/compare/a...b",
            ]
        );
        // What is not a tag stays: a less-than, a type argument (this is
        // a Rust project's notes), an autolink's address.
        assert_eq!(notes_lines("a < b and c > d", 12), vec!["a < b and c > d"]);
        assert_eq!(
            notes_lines("Res<Time>, Vec<usize> and Option<Vec<u8>>", 12),
            vec!["Res<Time>, Vec<usize> and Option<Vec<u8>>"]
        );
        assert_eq!(notes_lines("<h2>Big</h2><br/>text", 12), vec!["Bigtext"]);
        assert_eq!(
            notes_lines("<b>bold</b> <https://x/y>", 12),
            vec!["bold https://x/y"]
        );
        // Brackets before a link are not the link.
        assert_eq!(
            notes_lines("see [#12] and [the docs](https://x) ![img](https://y)", 12),
            vec!["see [#12] and the docs img"]
        );
        assert_eq!(notes_lines("", 12), Vec::<String>::new());
        assert_eq!(notes_lines("\n\n---\n\n", 12), Vec::<String>::new());

        let long: String = (0..40).map(|i| format!("- line {i}\n")).collect();
        let lines = notes_lines(&long, NOTES_MAX_LINES);
        assert_eq!(lines.len(), NOTES_MAX_LINES);
        assert_eq!(lines.last().map(String::as_str), Some("…"));
        assert_eq!(lines[0], "• line 0");
        // A short frame gets fewer lines, never fewer than two.
        assert_eq!(notes_lines(&long, 4).len(), 4);
        assert_eq!(notes_lines(&long, 2), vec!["• line 0", "…"]);
    }

    /// The notes give way before the question does: a tall frame gets
    /// the full cap, a short one (a small window, a big UI scale) fewer,
    /// and nothing sillier than two.
    #[test]
    fn the_notes_fit_the_frame() {
        assert_eq!(notes_budget(720.0 - 104.0), NOTES_MAX_LINES);
        // 1280x720 at 150% UI scale: 480 tall, 376 between the bars.
        assert!(notes_budget(376.0) < NOTES_MAX_LINES);
        assert!(notes_budget(376.0) >= 2);
        assert_eq!(notes_budget(0.0), 2);
        assert_eq!(notes_budget(f32::NAN), 2);
        assert_eq!(notes_budget(f32::INFINITY), NOTES_MAX_LINES);
    }

    /// Long lines wrap at the column, on spaces where there are any,
    /// continuation lines under a bullet keep its indent, and a word wider
    /// than the card is cut rather than run off it.
    #[test]
    fn long_lines_wrap_to_the_card() {
        let words = "word ".repeat(30);
        for line in notes_lines(&words, 12) {
            assert!(line.chars().count() <= NOTES_COLS, "{line:?}");
            assert!(!line.ends_with(' '));
        }
        let bullet = format!("- {}", "crab ".repeat(20).trim());
        let lines = notes_lines(&bullet, 12);
        assert!(lines.len() >= 2);
        assert!(lines[0].starts_with("• crab"));
        assert!(lines[1].starts_with("  crab"), "{:?}", lines[1]);
        let wall = "x".repeat(150);
        let lines = notes_lines(&wall, 12);
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|l| l.chars().count() <= NOTES_COLS));
        assert_eq!(lines.concat(), wall);
        // Wide characters count as one column each, not one byte.
        let accents = "é".repeat(70);
        let lines = notes_lines(&accents, 12);
        assert_eq!(lines[0].chars().count(), NOTES_COLS);
        assert_eq!(lines.concat(), accents);
    }
}
