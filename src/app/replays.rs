//! The replay library: every finished round kept under a name you can find
//! it by, and the screen for picking one.
//!
//! Rounds are filed by the day and time they were played plus who won, which
//! is what anyone remembers about a match ("the one Bo took on the twelfth").

use crate::app::i18n::fill;
use crate::app::settings::GameSettings;
use crate::app::{Playback, Screen, menu_ui};
use crate::sim::Replay;
use bevy::prelude::*;

/// Where the library lives. `last.txt` still sits beside it, because the
/// dev hooks and the menu's Replay entry reach for the newest round by name.
pub fn library_dir() -> std::path::PathBuf {
    crate::app::paths::data_dir().join("replays")
}

/// One kept round: the file, and what to call it on screen.
pub struct Kept {
    pub path: std::path::PathBuf,
    pub label: String,
}

/// The library screen's state: what is on the shelf, and where the cursor is.
#[derive(Resource, Default)]
pub struct Library {
    pub kept: Vec<Kept>,
    pub selected: usize,
    /// First row shown: the shelf can hold more rounds than the card has
    /// rows (see [`ROWS`]), so the window slides to keep the cursor in it.
    pub scroll: usize,
    /// Set when a pick fails to parse, so the screen can say so.
    pub feedback: String,
}

impl Library {
    /// Keep the cursor on the shelf and the window on the cursor. Called
    /// after every move and every re-read: the shelf can shrink under the
    /// cursor (a pasted code that failed, a re-entry after pruning) as
    /// well as grow past the rows.
    pub fn settle(&mut self) {
        let Self {
            kept,
            selected,
            scroll,
            feedback: _,
        } = self;
        *selected = (*selected).min(kept.len().saturating_sub(1));
        // Only as far as it must: down when the cursor falls off the
        // bottom, up when it climbs off the top, and never past a shelf
        // that got shorter.
        *scroll = (*scroll)
            .min(*selected)
            .max(selected.saturating_sub(ROWS - 1))
            .min(kept.len().saturating_sub(ROWS));
    }
}

/// The file name for a round finished now, by who won it.
///
/// Named from the wall clock rather than a counter: a counter needs the
/// directory read to know what is next, and two copies of the game would
/// disagree about it. Seconds since the epoch sort correctly as text for the
/// next few hundred years.
pub fn file_name(stamp: u64, winner: &str) -> String {
    let tidy: String = winner
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .take(12)
        .collect();
    format!("round_{stamp:010}_{tidy}.txt")
}

/// Drop the oldest rounds until at most `cap` are kept.
///
/// Called after a round is filed, so the shelf is trimmed at the one moment
/// it can have grown. Best-effort like every other write here: a file that
/// will not delete stays on the shelf rather than stopping the game, and
/// the next round tries again.
///
/// Only rounds are counted and only rounds are removed. `last.txt` and the
/// highlight reel live in the same directory and belong to the menu, not to
/// the shelf.
pub fn prune(cap: u8) {
    prune_in(&library_dir(), cap);
}

/// [`prune`] on a directory named outright, so a test can hand it a
/// scratch folder instead of the player's library.
pub fn prune_in(dir: &std::path::Path, cap: u8) {
    let kept = shelf_in(dir);
    for old in kept.iter().skip(usize::from(cap)) {
        if let Err(e) = std::fs::remove_file(&old.path) {
            warn!("could not drop the oldest round: {e}");
        }
    }
}

/// Everything on the shelf, newest first.
pub fn shelf() -> Vec<Kept> {
    shelf_in(&library_dir())
}

/// [`shelf`] on a directory named outright, for the same reason as
/// [`prune_in`]: the data directory comes from the process environment,
/// and a test that changes that races every other test in the binary.
pub fn shelf_in(dir: &std::path::Path) -> Vec<Kept> {
    let Ok(dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut kept: Vec<Kept> = dir
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "txt")
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("round_"))
        })
        .map(|path| {
            let label = label_of(&path);
            Kept { path, label }
        })
        .collect();
    // Newest first: the names sort by timestamp, so this is just a reverse.
    kept.sort_by(|a, b| b.path.cmp(&a.path));
    kept
}

/// `round_0000012345_Anna.txt` reads as "Anna, 12345". The timestamp is
/// shown as a date only if the clock agrees it is one; a file copied from
/// another machine is still listed, just plainly.
fn label_of(path: &std::path::Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("round")
        .to_string();
    let mut parts = stem.splitn(3, '_');
    let (_, stamp, winner) = (parts.next(), parts.next(), parts.next());
    let winner = winner.unwrap_or("").replace('_', " ");
    match stamp.and_then(|s| s.parse::<u64>().ok()) {
        // The separator is not decoration: "15/08 09:49  Mon" reads as a
        // weekday, and Mon is a player.
        Some(stamp) => format!("{}  -  {winner}", clock(stamp)),
        None => stem,
    }
}

/// A stamp as `dd/mm hh:mm`, on the clock in the room. No date library and
/// no need for one: days since the epoch convert with the same arithmetic
/// the daily challenge already uses, and the offset to add first comes from
/// the system zone (see [`crate::app::clock::local_offset`]).
///
/// The offset is taken at the stamp rather than now, so a round kept in
/// August and one kept in January are each shown on the clock that was on
/// the wall when they were played, not on today's.
fn clock(stamp: u64) -> String {
    stamp_text(stamp, crate::app::clock::local_offset(stamp))
}

/// The same, with the offset handed in: what the shelf renders is a
/// function of the stamp and the zone, and only the zone needs a file to
/// answer it. Split so the format can be tested on a machine in any zone,
/// which is every machine.
fn stamp_text(stamp: u64, offset: i64) -> String {
    // A clock set before 1970 already reads as zero (`clock::now_secs`);
    // west of Greenwich the offset can push such a stamp below it, and the
    // arithmetic below is unsigned. It stays at the epoch.
    let local = (stamp as i64 + offset).max(0) as u64;
    let secs = local % 86_400;
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    let (day, month) = crate::app::clock::civil_date((local / 86_400) as u32);
    format!("{day:02}/{month:02} {h:02}:{m:02}")
}

#[derive(Component)]
pub struct LibraryUi;

#[derive(Component)]
pub struct LibraryRow(usize);

/// The "nothing kept yet" line, shown in place of the rows on an empty shelf.
#[derive(Component)]
pub struct EmptyShelfNote;

/// How many rows the shelf shows at once.
const ROWS: usize = 12;

pub fn enter_library(
    mut commands: Commands,
    settings: Res<GameSettings>,
    mut library: ResMut<Library>,
) {
    library.kept = shelf();
    library.settle();
    library.feedback.clear();
    let tr = settings.tr();
    commands
        .spawn((LibraryUi, menu_ui::between_bars()))
        .with_children(|wrap| {
            wrap.spawn(menu_ui::screen_card()).with_children(|card| {
                menu_ui::heading_row(card, tr.replays_heading, None);
                // The empty-shelf line, which the rows cannot carry: a row
                // cell is one shelf-wide column that does not wrap, and this
                // sentence is wider than it, so squeezed in there it ran off
                // the side of the card. Its own line, the shelf's width and
                // free to wrap, keeps it inside. Blank, and out of the way,
                // the moment there is a round to show.
                card.spawn((
                    EmptyShelfNote,
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(19.0),
                        ..default()
                    },
                    TextColor(crate::app::palette::PARCHMENT.with_alpha(0.6)),
                    // No `no_wrap` here, unlike the row cells: the width is
                    // fixed to the shelf and the line is free to take a
                    // second row rather than run off the card.
                    Node {
                        width: Val::Px(ROW_W),
                        margin: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                        ..default()
                    },
                ));
                for row in 0..ROWS {
                    card.spawn((LibraryRow(row), menu_ui::card_row()))
                        .with_children(|line| {
                            line.spawn((LibraryRow(row), menu_ui::cell(ROW_W, 19.0)));
                        });
                }
            });
        });
}

/// The shelf is one column, as wide as a stamp and a winner's name.
const ROW_W: f32 = 420.0;

pub fn update_library(
    library: Res<Library>,
    settings: Res<GameSettings>,
    mut cells: Query<(&LibraryRow, &mut Text, &mut TextColor), Without<EmptyShelfNote>>,
    mut rows: Query<(&LibraryRow, &mut BackgroundColor)>,
    mut empty_note: Query<(&mut Text, &mut Node), With<EmptyShelfNote>>,
) {
    let tr = settings.tr();
    // The empty-shelf line carries the "nothing yet" message now, so the
    // rows never do. On a shelf with rounds on it, it is taken out of the
    // layout entirely (not merely blanked), or its empty line would sit
    // between the heading and the first round as a gap.
    let empty = library.kept.is_empty();
    for (mut note, mut node) in &mut empty_note {
        menu_ui::set_text(&mut note, if empty { tr.replays_empty } else { "" });
        menu_ui::set_shown(&mut node, empty);
    }
    // Row `r` of the card shows shelf entry `scroll + r`.
    let at = |row: usize| library.scroll + row;
    for (row, mut text, mut color) in &mut cells {
        let line = match library.kept.get(at(row.0)) {
            // "draw" is filed in the file name, where it has to stay the
            // same word on every machine; it is read out translated.
            Some(kept) => kept
                .label
                .replace("  -  draw", &format!("  -  {}", tr.replay_draw)),
            None => String::new(),
        };
        let picked = at(row.0) == library.selected && !library.kept.is_empty();
        menu_ui::set_text(&mut text, &line);
        menu_ui::set_color(
            &mut color,
            match (picked, library.kept.get(at(row.0)).is_some()) {
                (true, _) => Color::WHITE,
                (false, true) => crate::app::palette::PARCHMENT.with_alpha(0.75),
                (false, false) => crate::app::palette::PARCHMENT.with_alpha(0.45),
            },
        );
    }
    for (row, mut fill) in &mut rows {
        let ground = menu_ui::band(at(row.0) == library.selected && !library.kept.is_empty());
        menu_ui::set_bg(&mut fill, ground);
    }
}

/// Arrows pick, Enter watches, C copies a share code, V pastes one, Esc
/// leaves.
pub fn library_input(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<GameSettings>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    mut clipboard: ResMut<Clipboard>,
    mut library: ResMut<Library>,
    mut playback: ResMut<Playback>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    let tr = settings.tr();
    if keys.just_pressed(KeyCode::Escape) {
        next_screen.set(Screen::Menu);
        return;
    }
    // The cursor walks the whole shelf, not just the rows on show; the
    // window follows it.
    let shown = library.kept.len();
    if shown > 0 {
        library.selected = menu_ui::nav(&keys, library.selected, shown);
        library.settle();
    }
    if caps.just_pressed(&keys, 'C') {
        library.feedback = copy_selected(&mut clipboard, tr, &library);
    }
    if caps.just_pressed(&keys, 'V') {
        library.feedback = keep_pasted(&mut clipboard, tr);
        // Whatever happened, the shelf may have grown; re-read it so the new
        // round is there to pick rather than waiting for the screen to be
        // left and come back to.
        library.kept = shelf();
        library.settle();
    }
    if !menu_ui::enter(&keys) {
        return;
    }
    let Some(kept) = library.kept.get(library.selected) else {
        return;
    };
    match std::fs::read_to_string(&kept.path)
        .map_err(|e| e.to_string())
        .and_then(|text| Replay::parse(&text))
    {
        Ok(replay) => {
            playback.0 = Some((replay, 0));
            next_screen.set(Screen::Versus);
        }
        Err(e) => library.feedback = e,
    }
}

/// The selected round as a share code on the clipboard.
///
/// A round is thirty kilobytes of text and compresses to about eight
/// thousand characters, which is a paste rather than a thing anyone types,
/// so the message says how long it is and the player decides whether that
/// goes in a chat window or a file.
fn copy_selected(
    clipboard: &mut Clipboard,
    tr: &crate::app::i18n::Tr,
    library: &Library,
) -> String {
    let Some(kept) = library.kept.get(library.selected) else {
        return tr.replays_empty.to_string();
    };
    let text = match std::fs::read_to_string(&kept.path) {
        Ok(text) => text,
        Err(e) => return fill(tr.code_round_bad, &[("e", &e.to_string())]),
    };
    crate::app::codes::copy_feedback(
        clipboard,
        tr,
        crate::share::Kind::Round,
        text.as_bytes(),
        tr.code_copied,
    )
}

/// The round a pasted code carries, and the text to file it under, or what
/// to say about why not.
///
/// Parsed before anything is written, so what lands in the library is a round
/// this build can actually play back: a file that only fails when someone
/// presses Enter on it is worse than no file. Takes the pasted code rather
/// than the clipboard so it can be tested without touching the real one.
fn round_from(
    pasted: Option<(crate::share::Kind, Vec<u8>)>,
    tr: &crate::app::i18n::Tr,
) -> Result<(String, String), String> {
    let text =
        crate::app::codes::payload_text(pasted, tr, crate::share::Kind::Round, tr.code_round_bad)?;
    let replay = Replay::parse(&text).map_err(|e| fill(tr.code_round_bad, &[("e", &e)]))?;
    Ok((replay.level.name.clone(), text))
}

/// A round off the clipboard, onto the shelf.
fn keep_pasted(clipboard: &mut Clipboard, tr: &crate::app::i18n::Tr) -> String {
    let (winner, text) = match round_from(crate::app::codes::paste(clipboard), tr) {
        Ok(round) => round,
        Err(complaint) => return complaint,
    };
    let stamp = crate::app::clock::now_secs();
    let path = library_dir().join(file_name(stamp, &winner));
    match crate::app::paths::write_atomic(&path, &text) {
        Ok(()) => tr.code_round_saved.to_string(),
        Err(e) => fill(tr.code_round_bad, &[("e", &e.to_string())]),
    }
}

/// Playback speed while watching, so a three-minute round can be skimmed.
#[derive(Resource)]
pub struct PlaybackSpeed(pub u8);

impl Default for PlaybackSpeed {
    fn default() -> Self {
        PlaybackSpeed(1)
    }
}

impl PlaybackSpeed {
    /// The speeds the key steps through.
    pub const STEPS: [u8; 3] = [1, 2, 4];

    fn stepped(self) -> PlaybackSpeed {
        let next = Self::STEPS
            .iter()
            .position(|&s| s == self.0)
            .map_or(0, |at| (at + 1) % Self::STEPS.len());
        PlaybackSpeed(Self::STEPS[next])
    }
}

/// `S` cycles 1x, 2x, 4x while a replay is playing.
pub fn playback_speed_input(
    keys: Res<ButtonInput<KeyCode>>,
    playback: Res<Playback>,
    mut speed: ResMut<PlaybackSpeed>,
) {
    if playback.0.is_some() && keys.just_pressed(KeyCode::KeyS) {
        *speed = PlaybackSpeed(speed.0).stepped();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shelf longer than the card scrolls: the window follows the cursor
    /// down past the last row and back up, and never opens past the end.
    #[test]
    fn the_shelf_window_follows_the_cursor() {
        let mut library = Library {
            kept: (0..ROWS + 8)
                .map(|n| Kept {
                    path: format!("{n}.txt").into(),
                    label: n.to_string(),
                })
                .collect(),
            ..Library::default()
        };
        library.settle();
        assert_eq!((library.selected, library.scroll), (0, 0));
        // Down to the last shown row: still no scroll.
        library.selected = ROWS - 1;
        library.settle();
        assert_eq!(library.scroll, 0);
        // One further: the window slides by one.
        library.selected = ROWS;
        library.settle();
        assert_eq!(library.scroll, 1);
        // The last entry is reachable, and the window shows a full card.
        library.selected = ROWS + 7;
        library.settle();
        assert_eq!(library.scroll, 8);
        // Back to the top: the window climbs with the cursor.
        library.selected = 3;
        library.settle();
        assert_eq!(library.scroll, 3);
        // A shelf that shrinks pulls the cursor and the window back in.
        library.kept.truncate(4);
        library.selected = 10;
        library.settle();
        assert_eq!((library.selected, library.scroll), (3, 0));
        // An empty shelf is a cursor at nought and no window at all.
        library.kept.clear();
        library.settle();
        assert_eq!((library.selected, library.scroll), (0, 0));
    }

    #[test]
    fn round_names_sort_newest_last_and_survive_odd_winners() {
        assert_eq!(file_name(12_345, "Anna"), "round_0000012345_Anna.txt");
        // A name with punctuation, or none at all, still makes a file name.
        assert_eq!(file_name(7, "Bo/../etc"), "round_0000000007_Bo____etc.txt");
        assert_eq!(file_name(7, ""), "round_0000000007_.txt");
        // Zero-padded so plain text sorting is chronological.
        assert!(file_name(2, "a") < file_name(10, "a"));
        // And over-long winners are cut rather than making a silly path.
        assert!(file_name(1, &"x".repeat(40)).len() < 40);
    }

    #[test]
    fn labels_read_as_a_time_and_a_winner() {
        let label = label_of(std::path::Path::new("replays/round_1751328000_Anna.txt"));
        assert!(label.contains("Anna"), "{label}");
        assert!(label.contains('/') && label.contains(':'), "{label}");
        // A file from somewhere else is listed under its own name.
        let odd = label_of(std::path::Path::new("replays/round_handmade.txt"));
        assert_eq!(odd, "round_handmade");
    }

    /// A stamp reads on the clock that was on the wall, which is not the
    /// one in Greenwich. The offset is handed in here because the machine
    /// running this is in whatever zone it is in.
    #[test]
    fn a_stamp_reads_on_the_local_clock() {
        // 2025-07-01 00:00:00 UTC, day 20270.
        const NOON_LESS_TWELVE: u64 = 1_751_328_000;
        assert_eq!(stamp_text(NOON_LESS_TWELVE, 0), "01/07 00:00", "Greenwich");
        assert_eq!(stamp_text(NOON_LESS_TWELVE, 7200), "01/07 02:00", "Paris");
        // West of Greenwich the same instant is still the day before, which
        // is the case a bare `stamp % 86_400` gets wrong.
        assert_eq!(
            stamp_text(NOON_LESS_TWELVE, -4 * 3600),
            "30/06 20:00",
            "Halifax"
        );
        // And a stamp so early the offset would take it below the epoch
        // stays at the epoch rather than wrapping to the far future.
        assert_eq!(stamp_text(60, -4 * 3600), "01/01 00:00");
    }

    /// A round survives being carried as a share code and comes back as the
    /// same text, filed under the same winner. The clipboard itself is not
    /// touched: with `system_clipboard` on it is the real one, and a test
    /// has no business sitting on what someone just copied.
    #[test]
    fn a_round_travels_as_a_code_and_comes_back() {
        let tr = &crate::app::i18n::EN;
        let mut replay = Replay::new(crate::sim::Level::from_board(
            "Anna",
            3,
            crate::sim::classic_arena(false, 2),
        ));
        for tick in 0..40u8 {
            let mut actions = [crate::sim::PlayerAction::None; crate::sim::MAX_PLAYERS];
            actions[0] = crate::sim::PlayerAction::Place {
                x: tick % 10,
                y: 3,
                dir: crate::sim::Direction::Down,
            };
            replay.record(actions);
        }
        let text = replay.to_text();
        let code = crate::share::encode(crate::share::Kind::Round, text.as_bytes());
        let (winner, back) = round_from(crate::share::decode(&code), tr).expect("a round");
        assert_eq!(back, text, "what went in is what comes out");
        assert!(!winner.is_empty(), "and it knows what to file it under");
    }

    /// The three ways a paste is not a round, each with its own answer. One
    /// shrug for all three leaves a player guessing which mistake they made.
    #[test]
    fn what_is_not_a_round_says_which_way_it_is_not() {
        let tr = &crate::app::i18n::EN;
        // Nothing readable on the clipboard at all.
        assert_eq!(round_from(None, tr), Err(tr.code_none_pasted.to_string()));
        // A real code, but for a level - the commonest mix-up, since both
        // are copied with the same gesture from different screens.
        let level = crate::share::encode(crate::share::Kind::Level, b"name: X\nposts: 3\n");
        let complaint = round_from(crate::share::decode(&level), tr).expect_err("not a round");
        assert!(
            complaint.contains("a level") && complaint.contains("a round"),
            "{complaint}"
        );
        // A round code carrying something that is not a round.
        let junk = crate::share::encode(crate::share::Kind::Round, b"not a replay at all");
        assert!(round_from(crate::share::decode(&junk), tr).is_err());
    }

    #[test]
    fn the_speed_key_cycles_the_steps() {
        let mut speed = PlaybackSpeed::default();
        assert_eq!(speed.0, 1);
        for want in [2, 4, 1, 2] {
            speed = PlaybackSpeed(speed.0).stepped();
            assert_eq!(speed.0, want);
        }
    }
}
