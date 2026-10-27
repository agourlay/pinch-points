//! The replay library: every round the player kept, under a name you can
//! find it by, and the screen for picking one. A round is kept from its
//! results card ([`keep`]) or pasted in as a code.
//!
//! Rounds are filed by the day and time they were played plus who won, which
//! is what anyone remembers about a match ("the one Bo took on the twelfth").
//! Each row adds what the file name cannot say, read once from the round's
//! own head when the shelf is listed: what the winner won with, how many
//! played, and whether the game's AI or a bot took it.
//! Watching one, its speed and its transport bar, is [`playback`].

use crate::app::cycle::Cycle;
use crate::app::gamepad::LastDevice;
use crate::app::i18n::fill;
use crate::app::menu_ui::scroll_bar;
use crate::app::settings::GameSettings;
use crate::app::{Playback, Screen, menu_ui, palette};
use crate::sim::{BotLevel, Replay, SeatKind};
use bevy::prelude::*;

mod keep;
mod playback;
pub use keep::{
    KEEP_CAP, KeepKey, KeepOffer, RoundToKeep, Unkept, keep_input, keep_words, kept_lead,
    let_the_round_go,
};
pub use playback::{
    PlaybackSpeed, ReplayBar, ReplayProgress, ReplayReadout, playback_pause_input,
    playback_speed_input, tend_replay_bar, update_replay_bar,
};

/// Where the library lives. `last.txt` sits beside it, the newest round
/// whether it was kept or not, because the `PINCH_REPLAY` dev hook reaches
/// for it by name.
pub fn library_dir() -> std::path::PathBuf {
    crate::app::paths::data_dir().join("replays")
}

/// One kept round: the file, and what to call it on screen.
pub struct Kept {
    pub path: std::path::PathBuf,
    /// When it was played, `dd/mm hh:mm`; for a file not named the way the
    /// shelf names one, the file's own name.
    pub when: String,
    /// Who took it, as the file is named: empty for a draw, whose word is
    /// the player's language's and is added as the row is drawn, and for a
    /// file not named the shelf's way.
    pub winner: String,
    /// Nobody took it. A flag rather than a word in the label, so a player
    /// who calls themselves "draw" is still a winner.
    pub draw: bool,
    /// How many played, read off the round's beach; `None` for a file that
    /// does not read as a round.
    pub seats: Option<u8>,
    /// What took it, when that was not a person: see [`won_by`].
    pub won_by: Option<WonBy>,
    /// What it was won with ([`crate::app::teams::top_total`]), or the
    /// score a draw tied on, from the final scores the recording keeps;
    /// `None` for a recording made before scores were kept, and for a
    /// file not named the shelf's way, whose winner the row cannot say.
    pub points: Option<u32>,
}

/// What held the seat, or every seat of the team, that took a kept round,
/// when it was not a person.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WonBy {
    /// The game's AI, at the level the recording names, or `None` for a
    /// recording made before levels were kept (or a team whose AI seats
    /// played at different levels).
    Ai(Option<BotLevel>),
    /// A bot over the protocol.
    Bot,
}

/// The library screen's state: what is on the shelf, and where the cursor is.
#[derive(Resource)]
pub struct Library {
    /// The folder the shelf is read from and deleted in: [`library_dir`],
    /// or a test's own scratch folder.
    pub dir: std::path::PathBuf,
    pub kept: Vec<Kept>,
    pub selected: usize,
    /// First row shown: the shelf can hold more rounds than the card has
    /// rows (see [`ROWS`]), so the window slides to keep the cursor in it.
    pub scroll: usize,
    /// Set when a pick fails to parse, so the screen can say so.
    pub feedback: String,
    /// A delete asked for and waiting for its second press, about the
    /// round it named.
    pub delete: menu_ui::SecondPress<std::path::PathBuf>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            dir: library_dir(),
            kept: Vec::new(),
            selected: 0,
            scroll: 0,
            feedback: String::new(),
            delete: menu_ui::SecondPress::default(),
        }
    }
}

impl Library {
    /// Keep the cursor on the shelf and the window on the cursor. Called
    /// after every move and every re-read: the shelf can shrink under the
    /// cursor (a pasted code that failed, a re-entry after pruning) as
    /// well as grow past the rows.
    pub fn settle(&mut self) {
        let Self {
            dir: _,
            kept,
            selected,
            scroll,
            feedback: _,
            delete: _,
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

impl scroll_bar::Scrolls for Library {
    /// The rows [`update_library`] paints: [`ROWS`] of them from `scroll`.
    fn window(&self) -> scroll_bar::ScrollWindow {
        scroll_bar::ScrollWindow::rows(self.scroll, ROWS, self.kept.len())
    }
}

/// The file name for a round finished now, by who won it, or `None` for
/// a draw.
///
/// Named from the wall clock rather than a counter: a counter needs the
/// directory read to know what is next, and two copies of the game would
/// disagree about it. Seconds since the epoch sort correctly as text for the
/// next few hundred years.
///
/// A draw is the name with no winner part at all, which no winner's name
/// can produce: even an empty name leaves its `_`. It used to be filed under
/// the word "draw", and a player who had called themselves that was read
/// back as nobody.
pub fn file_name(stamp: u64, winner: Option<&str>) -> String {
    match winner {
        Some(winner) => format!("round_{stamp:010}_{}.txt", tidy(winner)),
        None => format!("round_{stamp:010}.txt"),
    }
}

/// A winner's name as a file name spells it: letters and digits kept,
/// anything else `_`, and no longer than twelve. What [`won_by`] matches
/// a seat's name against.
fn tidy(winner: &str) -> String {
    winner
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .take(12)
        .collect()
}

/// Where in `dir` to file a round finished at `stamp`: [`file_name`], or,
/// when that is taken, the same at the next free second.
///
/// Two rounds with one winner in one second are one file name, and the
/// second write replaced the first: two codes pasted in quick succession,
/// say. A second later on the label costs nothing, since the shelf shows
/// minutes, and the order stays the order they were filed in.
pub fn free_path(dir: &std::path::Path, stamp: u64, winner: Option<&str>) -> std::path::PathBuf {
    (stamp..)
        .map(|at| dir.join(file_name(at, winner)))
        .find(|path| !path.exists())
        .unwrap_or_else(|| dir.join(file_name(stamp, winner)))
}

/// Drop the oldest rounds until at most `cap` are kept.
///
/// Called after a round is filed, the one moment the shelf can have grown.
/// Best-effort like every other write here: a file that will not delete
/// stays on the shelf, and the next round tries again.
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
    for old in listed_in(dir).iter().skip(usize::from(cap)) {
        if let Err(e) = std::fs::remove_file(old) {
            warn!("could not drop the oldest round: {e}");
        }
    }
}

/// The "Rounds kept" setting turned from `was` to `now`, applied to the
/// shelf in `dir` when the setting is saved rather than at the next round
/// filed: a player who asked for fewer sees fewer. Only a lower cap
/// prunes, newest kept as [`prune_in`] keeps them; a raised one deletes
/// nothing, even on a shelf already over it (rounds copied in by hand).
pub fn cap_changed_in(dir: &std::path::Path, was: u8, now: u8) {
    if now < was {
        prune_in(dir, now);
    }
}

/// Delete the round at `path` from the shelf in `dir`.
///
/// Only a round on the shelf goes: the path is looked up in [`shelf_in`]
/// first, so `last.txt`, the highlight reel or anything else sharing the
/// folder cannot be removed through here, whatever path is handed in.
pub fn delete_in(dir: &std::path::Path, path: &std::path::Path) -> std::io::Result<()> {
    if !listed_in(dir).iter().any(|kept| kept == path) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "not a kept round",
        ));
    }
    std::fs::remove_file(path)
}

/// Everything on the shelf in `dir`, newest first. The directory is named
/// outright, for the same reason as [`prune_in`]: the data directory comes
/// from the process environment, and a test that changes that races every
/// other test in the binary.
///
/// Each round's file is read here, once, for its head (see [`kept`]), and
/// the rows are drawn from what that left in [`Kept`]: the screen redraws
/// every frame, and a shelf holds up to 99 rounds.
pub fn shelf_in(dir: &std::path::Path) -> Vec<Kept> {
    shelf_read(dir, |path| std::fs::read_to_string(path).ok())
}

/// [`shelf_in`] with the reading handed in, so a test can count it.
fn shelf_read(
    dir: &std::path::Path,
    mut read: impl FnMut(&std::path::Path) -> Option<String>,
) -> Vec<Kept> {
    listed_in(dir)
        .into_iter()
        .map(|path| {
            let text = read(&path);
            kept(path, text.as_deref())
        })
        .collect()
}

/// The kept rounds' files in `dir`, newest first, by their names alone:
/// what pruning and deleting go by, which need no file opened.
fn listed_in(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Ok(dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut kept: Vec<std::path::PathBuf> = dir
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "txt")
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("round_"))
        })
        .collect();
    // Newest first: the names sort by timestamp, so this is just a reverse.
    kept.sort_by(|a, b| b.cmp(a));
    kept
}

/// A round on the shelf, from its file's name and `text`, the file's
/// contents, or `None` when it would not read.
///
/// Only the head of the recording is parsed ([`Replay::head`]): who sat
/// where and on which beach. The inputs are never decoded and the round
/// never played, so a full shelf lists in a blink.
fn kept(path: std::path::PathBuf, text: Option<&str>) -> Kept {
    let named = name_of(&path);
    let head = text.and_then(|text| Replay::head(text).ok());
    let seats = head
        .as_ref()
        .map(|head| crate::app::session::replay_seats(&head.level.board()));
    let won_by = match (&head, seats, &named.filed) {
        (Some(head), Some(seats), Some(filed)) => won_by(head, seats, filed),
        _ => None,
    };
    let points = match (&head, seats) {
        (Some(head), Some(seats)) if named.filed.is_some() || named.draw => {
            head.scores.map(|scores| {
                let mode = crate::app::teams::recorded(head).played_by(seats);
                crate::app::teams::top_total(&scores, seats, mode)
            })
        }
        _ => None,
    };
    Kept {
        path,
        when: named.when,
        winner: named.filed.as_deref().map(shown).unwrap_or_default(),
        draw: named.draw,
        seats,
        won_by,
        points,
    }
}

/// What a kept round's file name says.
struct Named {
    /// When, or the file's own name when it is not named the shelf's way.
    when: String,
    /// The winner's part of the name, as [`tidy`] spelled it.
    filed: Option<String>,
    draw: bool,
}

/// `round_0000012345_Anna.txt` reads as "Anna, 12345", and
/// `round_0000012345.txt`, with no winner part, as a draw at 12345 (see
/// [`file_name`]). The timestamp is shown as a date only if the clock
/// agrees it is one; a file copied from another machine is still listed,
/// just plainly.
fn name_of(path: &std::path::Path) -> Named {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("round")
        .to_string();
    let mut parts = stem.splitn(3, '_');
    let (_, stamp, winner) = (parts.next(), parts.next(), parts.next());
    match (stamp.and_then(|s| s.parse::<u64>().ok()), winner) {
        (Some(stamp), Some(winner)) => Named {
            when: clock(stamp),
            filed: Some(winner.to_string()),
            draw: false,
        },
        (Some(stamp), None) => Named {
            when: clock(stamp),
            filed: None,
            draw: true,
        },
        (None, _) => Named {
            when: stem,
            filed: None,
            draw: false,
        },
    }
}

/// A winner's filed name as the shelf shows it: the file name's `_` back
/// to spaces.
fn shown(filed: &str) -> String {
    filed.replace('_', " ")
}

/// What took the round, when it was not a person, from the round's head
/// and the winner's name in its file name, `filed`.
///
/// The file names the winner as the results card did: a seat's name or its
/// "P{n}" label, or under a team mode the team's joined, in the language of
/// the machine that filed it. So every seat, or every team, is spelled in
/// every language, and the one whose spelling is the file's is the winner.
/// A name two of them spell (two seats called alike) settles only when
/// both were held alike; otherwise the row says nothing rather than guess.
/// The round is never played through to find out: that costs a whole
/// round's ticks for every file on the shelf.
///
/// A seat is marked by what held it ([`Replay::kinds`]); a team only when
/// all its seats were held alike, so a person paired with the AI is a
/// person's win.
fn won_by(head: &Replay, seats: u8, filed: &str) -> Option<WonBy> {
    let mode = crate::app::teams::recorded(head).played_by(seats);
    let mut found = None;
    for team in 0..mode.teams(seats) {
        let spelled = crate::app::i18n::ALL_LANGS.iter().any(|lang| {
            let label = crate::app::teams::team_label(mode, team, seats, |seat| {
                crate::app::name_or_label(&head.names, lang.tr(), seat)
            });
            tidy(&label) == filed
        });
        if !spelled {
            continue;
        }
        let held = held_by(head, &mode.seats_of(team, seats));
        match found {
            None => found = Some(held),
            Some(other) if other != held => return None,
            Some(_) => {}
        }
    }
    found.flatten()
}

/// What held every one of `seats`, when that was not a person: the AI, at
/// its level if they all played at one, or a bot.
fn held_by(head: &Replay, seats: &[u8]) -> Option<WonBy> {
    let kinds: Vec<SeatKind> = seats.iter().map(|&s| head.kinds[usize::from(s)]).collect();
    let levels: Vec<Option<BotLevel>> = seats.iter().map(|&s| head.ai[usize::from(s)]).collect();
    if kinds.iter().all(|&kind| kind == SeatKind::Bot) {
        return Some(WonBy::Bot);
    }
    if !kinds.iter().all(|&kind| kind == SeatKind::Ai) {
        return None;
    }
    let level = levels[0].filter(|_| levels.iter().all(|&l| l == levels[0]));
    Some(WonBy::Ai(level))
}

/// A stamp as `dd/mm hh:mm`, on the clock in the room. No date library and
/// no need for one: days since the epoch convert with the same arithmetic
/// the daily challenge already uses, and the offset to add first comes from
/// the system zone (see [`crate::app::clock::local_offset`]).
///
/// The offset is taken at the stamp rather than now, so a round is shown on
/// the clock that was on the wall when it was played.
fn clock(stamp: u64) -> String {
    stamp_text(stamp, crate::app::clock::local_offset(stamp))
}

/// The same, with the offset handed in, so the format can be tested on a
/// machine in any zone.
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

/// One of a shelf row's columns, on the cell that shows it.
#[derive(Component)]
pub struct LibraryCell(usize, Column);

/// The shelf's columns: when a round was played, who took it, what with,
/// how many played. Columns rather than one line, so the winners and their
/// points line up under one another whatever the date or the name beside
/// them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Column {
    When,
    Who,
    Points,
    Players,
}

impl Column {
    const ALL: [Column; 4] = [Column::When, Column::Who, Column::Points, Column::Players];

    /// The column's width, measured for its widest text in the wordiest
    /// language by `every_column_fits_in_every_language`.
    fn width(self) -> f32 {
        match self {
            Column::When => WHEN_W,
            Column::Who => WHO_W,
            Column::Points => POINTS_W,
            Column::Players => PLAYERS_W,
        }
    }

    /// What the column says about `kept`, in `tr`'s words.
    fn text(self, kept: &Kept, tr: &crate::app::i18n::Tr) -> String {
        match self {
            Column::When => kept.when.clone(),
            Column::Who => who(kept, tr),
            Column::Points => points(kept, tr),
            Column::Players => players(kept, tr),
        }
    }
}

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
    library.kept = shelf_in(&library.dir);
    library.delete = menu_ui::SecondPress::default();
    library.settle();
    let tr = settings.tr();
    // Any replay file at all can be watched, from a cup's folder or a
    // friend's message: the status line says how, until it has news.
    library.feedback = tr.replay_drop_hint.to_string();
    commands
        .spawn((LibraryUi, menu_ui::between_bars()))
        .with_children(|wrap| {
            wrap.spawn(menu_ui::screen_card()).with_children(|card| {
                menu_ui::heading_row(card, tr.replays_heading, None);
                // The empty-shelf line, which the rows cannot carry: a row
                // cell is one shelf-wide column that does not wrap, and
                // this sentence is wider than it. Its own line, free to
                // wrap, keeps it inside, and it goes the moment there is a
                // round to show.
                card.spawn((
                    EmptyShelfNote,
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(menu_ui::type_scale::ROW),
                        ..default()
                    },
                    TextColor(palette::PARCHMENT.with_alpha(0.6)),
                    // No `no_wrap` here, unlike the row cells: the width is
                    // fixed to the shelf and the line is free to take a
                    // second row rather than run off the card.
                    Node {
                        width: Val::Px(ROW_W),
                        margin: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                        ..default()
                    },
                ));
                // A shelf can hold far more rounds than these rows, and
                // the bar beside them is what says so.
                let bar = scroll_bar::BarOf::<Library>::default();
                scroll_bar::beside(card, menu_ui::ROW_GAP, bar, |rows| {
                    for row in 0..ROWS {
                        rows.spawn((LibraryRow(row), menu_ui::card_row()))
                            .with_children(|line| {
                                for column in Column::ALL {
                                    line.spawn((
                                        LibraryCell(row, column),
                                        menu_ui::cell(column.width(), ROW_FONT),
                                    ));
                                }
                            });
                    }
                });
            });
        });
}

/// The rows' type size, which the column widths are measured at.
const ROW_FONT: f32 = menu_ui::type_scale::ROW;
/// A stamp, `dd/mm hh:mm`, and a gap after it.
const WHEN_W: f32 = 150.0;
/// A winner's filed name, twelve characters at most, or a team's, and
/// the AI's tag with the longest level after it. Twelve kana are the
/// widest such name, a full em each: a bot names itself over the protocol
/// in whatever script it likes.
const WHO_W: f32 = 410.0;
/// Four digits of points in the wordiest language: more than a five
/// minute round of six seats has ever banked.
const POINTS_W: f32 = 120.0;
/// "6 players" in the wordiest language.
const PLAYERS_W: f32 = 140.0;
/// The whole row: the empty-shelf note is as wide.
const ROW_W: f32 = WHEN_W + WHO_W + POINTS_W + PLAYERS_W;

pub fn update_library(
    library: Res<Library>,
    settings: Res<GameSettings>,
    mut cells: Query<(&LibraryCell, &mut Text, &mut TextColor), Without<EmptyShelfNote>>,
    mut rows: Query<(&LibraryRow, &mut BackgroundColor, &mut Node), Without<EmptyShelfNote>>,
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
    for (&LibraryCell(row, column), mut text, mut color) in &mut cells {
        let line = library
            .kept
            .get(at(row))
            .map_or_else(String::new, |kept| column.text(kept, tr));
        let picked = at(row) == library.selected && !library.kept.is_empty();
        menu_ui::set_text(&mut text, &line);
        menu_ui::set_color(
            &mut color,
            match (picked, library.kept.get(at(row)).is_some()) {
                (true, _) => Color::WHITE,
                (false, true) => palette::PARCHMENT.with_alpha(0.75),
                (false, false) => palette::PARCHMENT.with_alpha(0.45),
            },
        );
    }
    for (row, mut fill, mut node) in &mut rows {
        let ground = menu_ui::band(at(row.0) == library.selected && !library.kept.is_empty());
        menu_ui::set_bg(&mut fill, ground);
        // A slot with nothing in it still holds its height, so pasting a
        // round does not resize the card under the cursor. An empty shelf
        // has no cursor and nothing to keep still, and twelve blank slots
        // under one sentence is the first thing a new player sees here, so
        // there the note really does stand in place of the rows.
        menu_ui::set_shown(&mut node, !empty);
    }
}

/// A kept round as the shelf names it: when, who took it, and what with.
/// The question a delete asks reads it, from the same columns the row
/// shows, so the round named is the round shown.
fn shelf_line(kept: &Kept, tr: &crate::app::i18n::Tr) -> String {
    // The separator is not decoration: "15/08 09:49  Mon" reads as a
    // weekday, and Mon is a player.
    [kept.when.clone(), who(kept, tr), points(kept, tr)]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("  -  ")
}

/// Who took a kept round, as its row says: the name it was filed under
/// and, when the game's AI or a bot took it, which ([`tag`]). A draw is
/// filed as no winner at all, and said here in the player's language.
fn who(kept: &Kept, tr: &crate::app::i18n::Tr) -> String {
    match kept.draw {
        true => tr.replay_draw.to_string(),
        false => format!("{}{}", kept.winner, tag(kept.won_by, tr)),
    }
}

/// What follows a winner's name: " (AI fierce)", " (AI)" for a recording
/// that kept no level, " (bot)", and nothing for a person.
fn tag(won_by: Option<WonBy>, tr: &crate::app::i18n::Tr) -> String {
    match won_by {
        None => String::new(),
        Some(WonBy::Ai(None)) => tr.tag_ai.to_string(),
        Some(WonBy::Ai(Some(level))) => fill(
            tr.replay_tag_ai_level,
            &[("level", tr.bot_levels[level.index()])],
        ),
        Some(WonBy::Bot) => tr.replay_tag_bot.to_string(),
    }
}

/// What a kept round was won with, or nothing for a recording that did
/// not keep its scores.
fn points(kept: &Kept, tr: &crate::app::i18n::Tr) -> String {
    kept.points.map_or_else(String::new, |points| {
        fill(tr.replay_points, &[("n", &points.to_string())])
    })
}

/// How many played a kept round, or nothing for a file that would not
/// read as one.
fn players(kept: &Kept, tr: &crate::app::i18n::Tr) -> String {
    kept.seats.map_or_else(String::new, |seats| {
        fill(tr.replay_players, &[("n", &seats.to_string())])
    })
}

/// The letter whose key deletes the round under the cursor, as the
/// prompt names it. Not one of the global letters, and not a key the
/// shelf walks with.
pub(crate) const DELETE_CAP: char = 'D';

/// What deletes a round, as the player's hands know it: the key at a
/// keyboard, the replays table's button on a pad.
fn delete_control(tr: &crate::app::i18n::Tr, device: LastDevice) -> String {
    crate::app::gamepad::scheme::spelled(
        tr,
        device,
        &crate::app::gamepad::Face::Replays,
        crate::app::gamepad::scheme::Does::Delete,
        &DELETE_CAP.to_string(),
    )
}

/// Arrows pick, Enter watches, C copies a share code, V pastes one, D
/// deletes one when pressed twice, Esc leaves.
pub fn library_input(
    keyboard: crate::app::keycaps::Keyboard,
    (settings, device, time): (Res<GameSettings>, Res<LastDevice>, Res<Time>),
    mut clipboard: ResMut<Clipboard>,
    mut library: ResMut<Library>,
    mut playback: ResMut<Playback>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    let keys = &keyboard.keys;
    let tr = settings.tr();
    let now = time.elapsed_secs();
    // A delete asked and not taken up in time is dropped, and its
    // question with it.
    if library.delete.lapse(now) {
        library.feedback = tr.replay_drop_hint.to_string();
    }
    if keys.just_pressed(KeyCode::Escape) {
        next_screen.set(Screen::Menu);
        return;
    }
    if keyboard.just(DELETE_CAP) {
        library.feedback = ask_or_delete(&mut library, tr, *device, now);
        return;
    }
    // Anything else pressed takes the question back: moving the cursor
    // most of all, since the question named the round under it.
    if keys.get_just_pressed().next().is_some() && library.delete.take_back() {
        library.feedback = tr.replay_drop_hint.to_string();
    }
    // The cursor walks the whole shelf, not just the rows on show; the
    // window follows it.
    let shown = library.kept.len();
    if shown > 0 {
        library.selected = menu_ui::nav(keys, library.selected, shown);
        library.settle();
    }
    if keyboard.global('C') {
        library.feedback = copy_selected(&mut clipboard, tr, &library);
    }
    if keyboard.just('V') {
        library.feedback = keep_pasted(&mut clipboard, tr, &library.dir, settings.replay_cap);
        // Whatever happened, the shelf may have grown; re-read it so the new
        // round is there to pick rather than waiting for the screen to be
        // left and come back to.
        library.kept = shelf_in(&library.dir);
        library.settle();
    }
    if !menu_ui::enter(keys) {
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
        Err(e) => library.feedback = why_not(tr, &e),
    }
}

/// Why a recording will not play, as the player is told: a later build's
/// recording ([`crate::sim::NEEDS_NEWER`]) in their language, anything else
/// as the reader put it.
pub fn why_not(tr: &crate::app::i18n::Tr, e: &str) -> String {
    match e == crate::sim::NEEDS_NEWER {
        true => tr.replay_newer.to_string(),
        false => e.to_string(),
    }
}

/// D on the round under the cursor: the first press asks, naming the
/// round and the control that answers; a second on the same round soon
/// after ([`menu_ui::SecondPress`]) deletes it. What to say either way.
///
/// After a delete the shelf is read again and the cursor stays where it
/// was, on the round that moved up into the gap, or on the new last one
/// when the last went.
fn ask_or_delete(
    library: &mut Library,
    tr: &crate::app::i18n::Tr,
    device: LastDevice,
    now: f32,
) -> String {
    let Some(kept) = library.kept.get(library.selected) else {
        return tr.replays_empty.to_string();
    };
    let (path, line) = (kept.path.clone(), shelf_line(kept, tr));
    if !library.delete.press(now, path.clone()) {
        let key = delete_control(tr, device);
        return fill(tr.replay_delete_confirm, &[("k", &key), ("r", &line)]);
    }
    let deleted = delete_in(&library.dir, &path);
    library.kept = shelf_in(&library.dir);
    library.settle();
    match deleted {
        Ok(()) => fill(tr.replay_deleted, &[("r", &line)]),
        Err(e) => fill(
            tr.replay_delete_failed,
            &[("r", &line), ("e", &e.to_string())],
        ),
    }
}

/// The selected round as a share code on the clipboard.
///
/// A round is thirty kilobytes of text and compresses to about eight
/// thousand characters, so the message says how long it is and the player
/// decides whether that goes in a chat window or a file.
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
/// Parsed before anything is written, so what lands in the library is a
/// round this build can play back rather than a file that fails when
/// somebody presses Enter on it. Takes the pasted code rather than the
/// clipboard so it can be tested without touching the real one.
///
/// The text filed is the recording with the final scores of the
/// playthrough that crowned its winner ([`ended`]), whatever scores the
/// code itself carried: the shelf believes the line without playing the
/// round, so it is this build's own.
fn round_from(
    pasted: Option<(crate::share::Kind, Vec<u8>)>,
    tr: &crate::app::i18n::Tr,
) -> Result<(Option<String>, String), String> {
    let text =
        crate::app::codes::payload_text(pasted, tr, crate::share::Kind::Round, tr.code_round_bad)?;
    let mut replay =
        Replay::parse(&text).map_err(|e| fill(tr.code_round_bad, &[("e", &why_not(tr, &e))]))?;
    let winner = ended(&mut replay, tr);
    Ok((winner, replay.to_text()))
}

/// Play a pasted round through to its last tick, once: keep its final
/// scores in it, and say who took it, for the shelf to file it under like
/// every other round, or `None` for a draw. The code may carry the inputs
/// and not the result, so the round is played, and crowned under the
/// scoring mode it records, a team as the card names it
/// ([`crate::app::teams::winner_label`]). Named as the recording named the
/// seat, or by its number.
fn ended(replay: &mut Replay, tr: &crate::app::i18n::Tr) -> Option<String> {
    let board = replay.playback();
    replay.scores = Some(*board.scores());
    let seats = crate::app::session::replay_seats(&board);
    let mode = crate::app::teams::recorded(replay).played_by(seats);
    crate::app::teams::winner_label(board.scores(), seats, mode, |seat| {
        crate::app::name_or_label(&replay.names, tr, seat)
    })
}

/// A round off the clipboard, onto the shelf in `dir`.
fn keep_pasted(
    clipboard: &mut Clipboard,
    tr: &crate::app::i18n::Tr,
    dir: &std::path::Path,
    cap: u8,
) -> String {
    keep(crate::app::codes::paste(clipboard), tr, dir, cap)
}

/// A pasted code onto the shelf in `dir`, which then keeps at most `cap`
/// rounds as it does after a round is played: a shelf filled by pasting is
/// still the shelf the setting promises. Takes the code rather than the
/// clipboard, as [`round_from`] does, so a test can file one.
fn keep(
    pasted: Option<(crate::share::Kind, Vec<u8>)>,
    tr: &crate::app::i18n::Tr,
    dir: &std::path::Path,
    cap: u8,
) -> String {
    let (winner, text) = match round_from(pasted, tr) {
        Ok(round) => round,
        Err(complaint) => return complaint,
    };
    let path = free_path(dir, crate::app::clock::now_secs(), winner.as_deref());
    match crate::app::paths::write_atomic(&path, &text) {
        Ok(()) => {
            prune_in(dir, cap);
            tr.code_round_saved.to_string()
        }
        Err(e) => fill(tr.code_round_bad, &[("e", &e.to_string())]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Who a round's playthrough crowns, the recording left as it was.
    fn winner_of(replay: &Replay, tr: &crate::app::i18n::Tr) -> Option<String> {
        ended(&mut replay.clone(), tr)
    }

    /// A shelf longer than the card scrolls: the window follows the cursor
    /// down past the last row and back up, and never opens past the end.
    #[test]
    fn the_shelf_window_follows_the_cursor() {
        let mut library = Library {
            kept: (0..ROWS + 8)
                .map(|n| kept(format!("round_{n}.txt").into(), None))
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
        let name = |stamp, winner| file_name(stamp, Some(winner));
        assert_eq!(name(12_345, "Anna"), "round_0000012345_Anna.txt");
        // A name with punctuation, or none at all, still makes a file name.
        assert_eq!(name(7, "Bo/../etc"), "round_0000000007_Bo____etc.txt");
        assert_eq!(name(7, ""), "round_0000000007_.txt");
        // A draw has no winner part, which no name can spell.
        assert_eq!(file_name(7, None), "round_0000000007.txt");
        // Zero-padded so plain text sorting is chronological.
        assert!(name(2, "a") < name(10, "a"));
        // And over-long winners are cut rather than making a silly path.
        assert!(name(1, &"x".repeat(40)).len() < 40);
    }

    #[test]
    fn labels_read_as_a_time_and_a_winner() {
        let tr = &crate::app::i18n::EN;
        let round = kept("replays/round_1751328000_Anna_B.txt".into(), None);
        assert_eq!(round.winner, "Anna B");
        assert!(
            round.when.contains('/') && round.when.contains(':'),
            "{}",
            round.when
        );
        assert!(!round.draw);
        assert_eq!(shelf_line(&round, tr), format!("{}  -  Anna B", round.when));
        // A file from somewhere else is listed under its own name.
        let odd = kept("replays/round_handmade.txt".into(), None);
        assert_eq!(
            (odd.when.as_str(), odd.winner.as_str()),
            ("round_handmade", "")
        );
        assert!(!odd.draw);
        assert_eq!(shelf_line(&odd, tr), "round_handmade");
    }

    /// A draw is a flag, not a word in the label. The word used to be
    /// swapped for its translation wherever it appeared, so a winner called
    /// "drawbridge" came out half translated, and one called "draw" was
    /// shown as a tie.
    #[test]
    fn a_draw_is_told_from_a_winner_called_draw() {
        let tr = &crate::app::i18n::EN;
        let read = |winner| kept(file_name(1_751_328_000, winner).into(), None);
        let tie = read(None);
        assert!(tie.draw);
        assert!(tie.winner.is_empty(), "the word is the shelf's to add");
        assert_eq!(who(&tie, tr), "draw");
        for name in ["draw", "drawbridge"] {
            let round = read(Some(name));
            assert!(!round.draw, "{name} won");
            assert_eq!(who(&round, tr), name);
        }
    }

    /// The head of a round of `seats` on a generated beach, with `kinds`
    /// and `ai` for its first seats, as a file would hold it.
    fn round_text(seats: u8, kinds: &[SeatKind], ai: &[Option<BotLevel>]) -> String {
        let board = crate::sim::generate_arena(7, seats, 16, 12);
        let mut replay = Replay::new(crate::sim::Level::from_board("Versus", 3, board));
        replay.kinds[..kinds.len()].copy_from_slice(kinds);
        replay.ai[..ai.len()].copy_from_slice(ai);
        replay.record([crate::sim::PlayerAction::None; crate::sim::MAX_PLAYERS]);
        replay.to_text()
    }

    /// The row a file named for `winner` and holding `text` gets.
    fn row(winner: Option<&str>, text: &str) -> Kept {
        kept(file_name(1_751_328_000, winner).into(), Some(text))
    }

    /// Each row says who took the round and what they were: a person
    /// plainly, the game's AI with its level, the AI of a recording that
    /// kept none without one, a bot as a bot; and how many played.
    #[test]
    fn a_row_says_what_took_the_round_and_how_many_played() {
        use SeatKind::{Ai, Bot, Human};
        let tr = &crate::app::i18n::EN;
        let text = round_text(
            4,
            &[Human, Ai, Bot, Ai],
            &[None, Some(BotLevel::Hard), None, Some(BotLevel::Easy)],
        );
        let cases = [
            (Some("P1"), "P1"),
            (Some("P2"), "P2 (AI fierce)"),
            (Some("P3"), "P3 (bot)"),
            (Some("P4"), "P4 (AI easy)"),
            (None, "draw"),
        ];
        for (winner, said) in cases {
            let round = row(winner, &text);
            assert_eq!(who(&round, tr), said);
            assert_eq!(players(&round, tr), "4 players");
        }
        // A seat is found under its label in the language that filed it.
        let de = crate::app::i18n::Lang::De.tr();
        assert_eq!(who(&row(Some("S2"), &text), de), "S2 (KI wild)");
        // And under its own name, which a file name tidies.
        let named = text.replacen("replay-v3\n", "replay-v3\nnames: Anna|Deep Blue 2000\n", 1);
        assert_eq!(
            who(&row(Some("Deep Blue 2000"), &named), tr),
            "Deep Blue 20 (AI fierce)"
        );
        // A name nobody at the table answers to is shown as filed.
        assert_eq!(who(&row(Some("Zed"), &text), tr), "Zed");
    }

    /// A recording made before levels were kept has its AI seats without
    /// one, and says the AI took it without guessing the level; one made
    /// before kinds were kept says nothing at all, as a person's win.
    #[test]
    fn an_older_recording_says_what_it_can() {
        use SeatKind::{Ai, Human};
        let tr = &crate::app::i18n::EN;
        let text = round_text(3, &[Human, Ai, Ai], &[None, Some(BotLevel::Normal)]);
        let no_levels: String = text
            .lines()
            .filter(|line| !line.starts_with("ai:"))
            .map(|line| format!("{line}\n"))
            .collect();
        assert_ne!(no_levels, text, "the line was there to take out");
        assert_eq!(who(&row(Some("P2"), &no_levels), tr), "P2 (AI)");
        assert_eq!(players(&row(Some("P2"), &no_levels), tr), "3 players");
        let no_kinds: String = no_levels
            .lines()
            .filter(|line| !line.starts_with("kinds:"))
            .map(|line| format!("{line}\n"))
            .collect();
        assert_eq!(who(&row(Some("P2"), &no_kinds), tr), "P2");
        // And a file that is no round at all is listed, with nothing to say
        // about who played.
        let junk = row(Some("P2"), "not a replay");
        assert_eq!(
            (who(&junk, tr).as_str(), players(&junk, tr).as_str()),
            ("P2", "")
        );
    }

    /// A team is marked only when every seat of it was held alike: two AI
    /// at one level, two at different levels, a person and the AI.
    #[test]
    fn a_team_is_marked_only_when_held_alike() {
        use SeatKind::{Ai, Human};
        let tr = &crate::app::i18n::EN;
        let with_teams = |kinds: &[SeatKind], ai: &[Option<BotLevel>]| {
            let board = crate::sim::generate_arena(7, 4, 16, 12);
            let mut replay = Replay::new(crate::sim::Level::from_board("Versus", 3, board));
            replay.kinds[..kinds.len()].copy_from_slice(kinds);
            replay.ai[..ai.len()].copy_from_slice(ai);
            replay.teams = crate::app::teams::TeamMode::Pairs.index() as u8;
            replay.to_text()
        };
        let hard = Some(BotLevel::Hard);
        let level_pair = with_teams(&[Human, Human, Ai, Ai], &[None, None, hard, hard]);
        assert_eq!(
            who(&row(Some("P3+P4"), &level_pair), tr),
            "P3 P4 (AI fierce)"
        );
        assert_eq!(who(&row(Some("P1+P2"), &level_pair), tr), "P1 P2");
        let mixed_levels = with_teams(&[Human, Human, Ai, Ai], &[None, None, hard, None]);
        assert_eq!(who(&row(Some("P3+P4"), &mixed_levels), tr), "P3 P4 (AI)");
        let mixed = with_teams(&[Human, Human, Human, Ai], &[None, None, None, hard]);
        assert_eq!(who(&row(Some("P3+P4"), &mixed), tr), "P3 P4");
        assert_eq!(players(&row(Some("P3+P4"), &mixed), tr), "4 players");
    }

    /// Two seats a file name could mean, held differently, mark nothing:
    /// the row would be guessing.
    #[test]
    fn a_name_two_seats_answer_to_marks_nothing() {
        use SeatKind::{Ai, Human};
        let tr = &crate::app::i18n::EN;
        let text = round_text(2, &[Human, Ai], &[None, Some(BotLevel::Hard)]).replacen(
            "replay-v3\n",
            "replay-v3\nnames: Bo|Bo\n",
            1,
        );
        assert_eq!(who(&row(Some("Bo"), &text), tr), "Bo");
        // A person who calls themselves after an empty seat's label, the
        // same: P2 could be either.
        let text = round_text(2, &[Human, Ai], &[None, Some(BotLevel::Hard)]).replacen(
            "replay-v3\n",
            "replay-v3\nnames: P2|\n",
            1,
        );
        assert_eq!(who(&row(Some("P2"), &text), tr), "P2");
    }

    /// `text` with its final scores kept, as a round is filed now.
    fn with_scores(text: &str, scores: [u32; crate::sim::MAX_PLAYERS]) -> String {
        let mut head = Replay::parse(text).expect("a round");
        head.scores = Some(scores);
        head.to_text()
    }

    /// A row says what the round was won with: the winner's score, a
    /// team's total as the results card sums it, the score a draw tied on,
    /// and nothing for a recording that kept no scores. The delete's
    /// question names the round as the row shows it, points included.
    #[test]
    fn a_row_says_what_the_round_was_won_with() {
        use SeatKind::{Ai, Human};
        let tr = &crate::app::i18n::EN;
        let plain = round_text(4, &[Human, Ai], &[None, Some(BotLevel::Hard)]);
        let text = with_scores(&plain, [12, 87, 30, 42, 0, 0]);
        let won = row(Some("P2"), &text);
        assert_eq!(won.points, Some(87));
        assert_eq!(
            (who(&won, tr).as_str(), points(&won, tr).as_str()),
            ("P2 (AI fierce)", "87 pts")
        );
        assert_eq!(
            shelf_line(&won, tr),
            format!("{}  -  P2 (AI fierce)  -  87 pts", won.when)
        );
        // In pairs the winning pair's total: P1+P2's 12 + 87 against
        // P3+P4's 30 + 42.
        let mut paired = Replay::parse(&text).expect("a round");
        paired.teams = crate::app::teams::TeamMode::Pairs.index() as u8;
        let paired = row(Some("P1+P2"), &paired.to_text());
        assert_eq!(points(&paired, tr), "99 pts");
        // A draw: the score the leaders tied on.
        let tie = row(None, &with_scores(&plain, [5, 9, 9, 1, 0, 0]));
        assert_eq!(
            (who(&tie, tr).as_str(), points(&tie, tr).as_str()),
            ("draw", "9 pts")
        );
        assert_eq!(
            shelf_line(&tie, tr),
            format!("{}  -  draw  -  9 pts", tie.when)
        );
        // Kept before scores were: no points, and the question as it was.
        let old = row(Some("P2"), &plain);
        assert_eq!(old.points, None);
        assert_eq!(points(&old, tr), "");
        assert_eq!(
            shelf_line(&old, tr),
            format!("{}  -  P2 (AI fierce)", old.when)
        );
        // And a file not named the shelf's way has no winner to credit.
        let odd = kept("replays/round_handmade.txt".into(), Some(&text));
        assert_eq!(odd.points, None);
        // In every language the column says the number the line holds.
        for lang in crate::app::i18n::ALL_LANGS {
            assert!(points(&won, lang.tr()).contains("87"), "{lang:?}");
        }
    }

    /// A hand-edited recording claiming the most points a score holds, in
    /// teams, is listed rather than a panic: a team's total stops at the
    /// most a total can hold, as the results card's does.
    #[test]
    fn a_row_with_absurd_scores_is_listed() {
        let plain = round_text(4, &[], &[]);
        let mut paired = Replay::parse(&with_scores(&plain, [u32::MAX; crate::sim::MAX_PLAYERS]))
            .expect("a round");
        paired.teams = crate::app::teams::TeamMode::Pairs.index() as u8;
        let round = row(Some("P1+P2"), &paired.to_text());
        assert_eq!(round.points, Some(u32::MAX));
        let tr = &crate::app::i18n::EN;
        assert!(points(&round, tr).contains(&u32::MAX.to_string()));
    }

    /// The shelf never plays a round: its rows come from the head alone. A
    /// file whose inputs would not even parse, let alone replay, is listed
    /// with everything its head says, the points included.
    #[test]
    fn the_shelf_never_plays_a_round() {
        let text = with_scores(&round_text(3, &[], &[]), [4, 11, 2, 0, 0, 0]);
        let broken = format!("{text}not an input line\n");
        assert!(Replay::parse(&broken).is_err(), "it would not replay");
        let dir = crate::app::paths::test_dir("never-plays");
        std::fs::write(dir.join(file_name(1_751_328_000, Some("P2"))), &broken).expect("written");
        let shelf = shelf_in(&dir);
        assert_eq!(shelf.len(), 1);
        assert_eq!((shelf[0].points, shelf[0].seats), (Some(11), Some(3)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The count comes off the beach, two to six.
    #[test]
    fn a_row_counts_the_seats_off_the_beach() {
        let tr = &crate::app::i18n::Lang::Ru.tr();
        for seats in 2..=crate::sim::MAX_PLAYERS as u8 {
            let round = row(Some("P1"), &round_text(seats, &[], &[]));
            assert_eq!(round.seats, Some(seats));
            assert_eq!(players(&round, tr), format!("игроков: {seats}"));
        }
    }

    /// Listing the shelf opens each round's file once, and nothing else
    /// in the folder; the rows are drawn from what that read left.
    #[test]
    fn listing_the_shelf_reads_each_round_once() {
        let dir = stocked("reads-once", 5);
        let mut reads: Vec<std::path::PathBuf> = Vec::new();
        let shelf = shelf_read(&dir, |path| {
            reads.push(path.to_path_buf());
            std::fs::read_to_string(path).ok()
        });
        assert_eq!(shelf.len(), 5);
        let mut listed: Vec<_> = shelf.iter().map(|kept| kept.path.clone()).collect();
        reads.sort();
        listed.sort();
        assert_eq!(reads, listed, "each round once, and only rounds");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every column holds its widest text in every language: the stamp,
    /// a twelve-character winner, Latin or kana, with the longest tag, six
    /// players.
    #[test]
    fn every_column_fits_in_every_language() {
        use crate::app::i18n::metrics::text_px;
        let fits = |lang, column: Column, text: &str| {
            let px = text_px(text, ROW_FONT);
            assert!(
                px <= column.width() - 8.0,
                "{lang:?} {column:?}: {text:?} is {px:.1}px of {}",
                column.width()
            );
        };
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            fits(lang, Column::When, &stamp_text(1_751_328_000, 0));
            let mut tags: Vec<String> = [None, Some(WonBy::Ai(None)), Some(WonBy::Bot)]
                .into_iter()
                .map(|won_by| tag(won_by, tr))
                .collect();
            tags.extend(
                crate::app::match_setup::BOT_LEVELS
                    .iter()
                    .map(|&level| tag(Some(WonBy::Ai(Some(level))), tr)),
            );
            // The widest twelve characters a filed name keeps: Latin at
            // the UI face's advance, and kana, which a bot's name over the
            // protocol may be all of, at the subset's full em.
            for tag in tags {
                for name in ["W".repeat(12), "あ".repeat(12)] {
                    fits(lang, Column::Who, &format!("{name}{tag}"));
                }
            }
            fits(lang, Column::Who, tr.replay_draw);
            fits(
                lang,
                Column::Points,
                &fill(tr.replay_points, &[("n", "9999")]),
            );
            let six = crate::sim::MAX_PLAYERS.to_string();
            fits(
                lang,
                Column::Players,
                &fill(tr.replay_players, &[("n", &six)]),
            );
        }
    }

    /// The AI is called what the rest of the game calls it, and a bot
    /// what the match setup calls one, in every language.
    #[test]
    fn the_tags_use_the_games_own_words() {
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            let ai = tr.tag_ai.trim_end_matches(')');
            assert!(
                tr.replay_tag_ai_level.starts_with(&format!("{ai} ")),
                "{lang:?}"
            );
            assert!(tr.replay_tag_ai_level.contains("{level}"), "{lang:?}");
            assert_eq!(
                tr.replay_tag_bot,
                format!(" ({})", tr.match_bot),
                "{lang:?}"
            );
            assert!(tr.replay_players.contains("{n}"), "{lang:?}");
            assert!(tr.replay_points.contains("{n}"), "{lang:?}");
        }
    }

    /// Two codes for rounds the same seat won, pasted within one second,
    /// are two rounds on the shelf: they used to share a file name and the
    /// second replaced the first. And pasting keeps the shelf to its cap,
    /// as finishing a round does.
    #[test]
    fn pasted_rounds_keep_their_own_files_and_the_cap() {
        let tr = &crate::app::i18n::EN;
        let dir = crate::app::paths::test_dir("pasted-rounds");
        let code = || {
            let text = banked_round().to_text();
            crate::share::decode(&crate::share::encode(
                crate::share::Kind::Round,
                text.as_bytes(),
            ))
        };
        assert_eq!(keep(code(), tr, &dir, 10), tr.code_round_saved);
        assert_eq!(keep(code(), tr, &dir, 10), tr.code_round_saved);
        assert_eq!(shelf_in(&dir).len(), 2, "two pastes, two rounds");
        // The second-later rule itself, on a clock held still.
        std::fs::write(dir.join(file_name(5, Some("P1"))), "x").expect("written");
        assert_eq!(
            free_path(&dir, 5, Some("P1")),
            dir.join(file_name(6, Some("P1"))),
            "a taken name is passed over for the next second"
        );
        assert_eq!(
            free_path(&dir, 5, None),
            dir.join(file_name(5, None)),
            "and a free one is kept"
        );
        for _ in 0..4 {
            keep(code(), tr, &dir, 3);
        }
        assert_eq!(shelf_in(&dir).len(), 3, "and the cap holds");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A shelf of `rounds` rounds in a scratch folder, a minute apart and
    /// won by `P0`, `P1`... in the order played, with `last.txt` and the
    /// highlight reel beside them as the menu keeps them.
    fn stocked(name: &str, rounds: u64) -> std::path::PathBuf {
        let dir = crate::app::paths::test_dir(name);
        for n in 0..rounds {
            let path = dir.join(file_name(1_751_328_000 + 60 * n, Some(&format!("P{n}"))));
            std::fs::write(path, "x").expect("written");
        }
        std::fs::write(dir.join("last.txt"), "x").expect("written");
        std::fs::write(dir.join("highlight.gif"), "x").expect("written");
        dir
    }

    /// The shelf laid out by Bevy's own layout in a 1280x720 window: its
    /// rows centred in the card, under the heading, the scroll bar's room
    /// balanced on the other side, and the card inside the window with
    /// the widest winner column.
    #[test]
    fn the_rows_sit_in_the_middle_of_a_card_that_fits() {
        use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
        let (width, height) = (1280u32, 720u32);
        let dir = stocked("centred", 3);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            AssetPlugin::default(),
            ImagePlugin::default(),
            bevy::text::TextPlugin,
            bevy::ui::UiPlugin,
        ));
        app.add_plugins((
            bevy::input::InputPlugin,
            WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            },
            bevy::a11y::AccessibilityPlugin,
            DefaultPickingPlugins,
        ));
        app.init_asset::<TextureAtlasLayout>();
        let settings = GameSettings::default();
        app.insert_resource(UiScale(crate::app::settings::applied_ui_ratio(
            settings.ui_ratio(),
            width as f32,
            height as f32,
        )));
        app.insert_resource(settings);
        app.insert_resource(Library {
            dir: dir.clone(),
            ..Library::default()
        });
        app.world_mut().spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(width, height),
                        scale_factor: 1.0,
                    }),
                    ..default()
                },
                viewport: Some(Viewport {
                    physical_size: UVec2::new(width, height),
                    ..default()
                }),
                ..default()
            },
        ));
        app.add_systems(Startup, enter_library);
        app.add_systems(Update, update_library);
        for _ in 0..4 {
            app.update();
        }
        let world = app.world_mut();
        let span = |world: &World, entity: Entity| {
            let node = world.get::<ComputedNode>(entity).expect("laid out");
            let at = world.get::<UiGlobalTransform>(entity).expect("placed");
            (
                at.translation.x - node.size.x / 2.0,
                at.translation.x + node.size.x / 2.0,
            )
        };
        let mut cards = world.query_filtered::<Entity, With<menu_ui::ShoreCard>>();
        let card = cards.single(world).expect("one card");
        let mut rows = world.query::<(Entity, &LibraryRow)>();
        let row = rows
            .iter(world)
            .find(|(_, row)| row.0 == 0)
            .map(|(entity, _)| entity)
            .expect("a row");
        let (card_left, card_right) = span(world, card);
        let (row_left, row_right) = span(world, row);
        let middle = |(left, right): (f32, f32)| (left + right) / 2.0;
        assert!(
            (middle((row_left, row_right)) - middle((card_left, card_right))).abs() < 1.0,
            "row {row_left}..{row_right} in card {card_left}..{card_right}"
        );
        assert!(
            card_left >= 0.0 && card_right <= width as f32,
            "card {card_left}..{card_right} in a {width}px window"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The library screen over `dir`, with a pad plugged in and the bridge
    /// in front of it, as the game runs it.
    fn library_app(dir: &std::path::Path) -> (App, Entity) {
        use crate::app::gamepad::{Bridged, Face, PadFace, PadPick};
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::Replays));
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<Bridged>();
        app.init_resource::<PadPick>();
        app.init_resource::<Time>();
        app.init_resource::<LastDevice>();
        app.init_resource::<Clipboard>();
        app.init_resource::<Playback>();
        app.init_resource::<crate::app::cursor::LiveSeatKeys>();
        app.insert_resource(PadFace(Face::Replays));
        app.init_resource::<GameSettings>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        let mut library = Library {
            dir: dir.to_path_buf(),
            ..Library::default()
        };
        library.kept = shelf_in(dir);
        app.insert_resource(library);
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        app.add_systems(
            Update,
            (
                crate::app::gamepad::pad_menu_bridge,
                library_input,
                crate::app::gamepad::pad_bridge_release,
            )
                .chain(),
        );
        (app, pad)
    }

    fn tap(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.update();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(key);
        keys.clear();
    }

    fn wait(app: &mut App, secs: f32) {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(secs));
    }

    /// The shelf's scroll bar shows the window the rows are painted from:
    /// hidden while every round fits, at the top on entry, at the bottom
    /// once the cursor has walked to the last round, and gone again once
    /// deleting has brought the shelf back inside the card.
    #[test]
    fn the_bar_follows_the_shelf_window() {
        use bevy::ecs::system::RunSystemOnce;
        use scroll_bar::{BarOf, reading};
        let dir = stocked("scroll-bar", (ROWS + 6) as u64);
        let (mut app, _) = library_app(&dir);
        app.add_systems(
            Update,
            (update_library, scroll_bar::follow::<Library>)
                .chain()
                .after(library_input),
        );
        app.world_mut()
            .run_system_once(enter_library)
            .expect("the shelf is built");
        app.update();
        let bar = |app: &mut App| reading::<BarOf<Library>>(app.world_mut());
        let share = ROWS as f32 / (ROWS + 6) as f32;
        assert_eq!(bar(&mut app), Some([0.0, share, 1.0 - share]), "at the top");

        // Walk to the last round: the thumb reaches the bottom of its track.
        for _ in 0..ROWS + 5 {
            tap(&mut app, KeyCode::ArrowDown);
        }
        assert_eq!(app.world().resource::<Library>().selected, ROWS + 5);
        assert_eq!(
            bar(&mut app),
            Some([1.0 - share, share, 0.0]),
            "at the bottom"
        );

        // Delete from the bottom: the thumb grows as the shelf shortens and
        // stays at the bottom, then the bar goes once all of it fits.
        for left in (ROWS..ROWS + 6).rev() {
            tap(&mut app, KeyCode::KeyD);
            wait(&mut app, 0.5);
            tap(&mut app, KeyCode::KeyD);
            assert_eq!(app.world().resource::<Library>().kept.len(), left);
            let share = ROWS as f32 / left as f32;
            let want = (left > ROWS).then_some([1.0 - share, share, 0.0]);
            assert_eq!(bar(&mut app), want, "{left} rounds left");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What the shelf holds, by winner, newest first.
    fn winners(dir: &std::path::Path) -> Vec<String> {
        shelf_in(dir).into_iter().map(|kept| kept.winner).collect()
    }

    /// D only asks, naming the round as the shelf shows it; D again soon
    /// after deletes it, and the cursor stays on its row, now the next
    /// round down. Another key between the two takes the question back,
    /// and a question left standing lapses.
    #[test]
    fn a_delete_asks_first_and_takes_the_round_named() {
        let tr = &crate::app::i18n::EN;
        let dir = stocked("delete-asks", 4);
        let (mut app, _) = library_app(&dir);
        let library = |app: &App| -> (usize, String) {
            let library = app.world().resource::<Library>();
            (library.selected, library.feedback.clone())
        };
        let named = shelf_line(&shelf_in(&dir)[0], tr);

        tap(&mut app, KeyCode::KeyD);
        assert_eq!(
            winners(&dir),
            ["P3", "P2", "P1", "P0"],
            "one press only asks"
        );
        assert_eq!(
            library(&app).1,
            fill(tr.replay_delete_confirm, &[("k", "D"), ("r", &named)])
        );
        wait(&mut app, 1.0);
        tap(&mut app, KeyCode::KeyD);
        assert_eq!(winners(&dir), ["P2", "P1", "P0"], "the second deletes");
        assert_eq!(
            library(&app),
            (0, fill(tr.replay_deleted, &[("r", &named)]))
        );
        assert_eq!(app.world().resource::<Library>().kept.len(), 3, "re-read");

        // Moving the cursor between the two takes the question back.
        tap(&mut app, KeyCode::KeyD);
        tap(&mut app, KeyCode::KeyS);
        assert_eq!(library(&app), (1, tr.replay_drop_hint.to_string()));
        tap(&mut app, KeyCode::KeyD);
        assert_eq!(
            winners(&dir),
            ["P2", "P1", "P0"],
            "so this press asks again"
        );

        // So does any other key, one that does nothing here included.
        tap(&mut app, KeyCode::KeyZ);
        tap(&mut app, KeyCode::KeyD);
        assert_eq!(winners(&dir).len(), 3, "and so does this one");

        // Left standing, the question lapses, and a late D asks afresh.
        wait(&mut app, menu_ui::SECOND_PRESS_SECS + 1.0);
        app.update();
        assert_eq!(library(&app).1, tr.replay_drop_hint);
        tap(&mut app, KeyCode::KeyD);
        assert_eq!(winners(&dir).len(), 3, "a late second press only asks");

        // Taken up now: the second row goes, the cursor stays on it.
        tap(&mut app, KeyCode::KeyD);
        assert_eq!(winners(&dir), ["P2", "P0"]);
        assert_eq!(library(&app).0, 1);
        // The last row going leaves the cursor on the row above.
        tap(&mut app, KeyCode::KeyD);
        tap(&mut app, KeyCode::KeyD);
        assert_eq!(winners(&dir), ["P2"]);
        assert_eq!(library(&app).0, 0);
        // And the last round going leaves an empty shelf, which the screen
        // shows as its note, and nothing else in the folder touched.
        tap(&mut app, KeyCode::KeyD);
        tap(&mut app, KeyCode::KeyD);
        assert!(app.world().resource::<Library>().kept.is_empty());
        assert!(winners(&dir).is_empty());
        assert!(dir.join("last.txt").exists() && dir.join("highlight.gif").exists());
        tap(&mut app, KeyCode::KeyD);
        assert_eq!(library(&app), (0, tr.replays_empty.to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The pad's delete, through the bridge, asks and deletes the same
    /// way, and the question names the pad's button.
    #[test]
    fn a_pad_deletes_through_the_bridge_and_is_named_its_button() {
        let tr = &crate::app::i18n::EN;
        let dir = stocked("delete-pad", 2);
        let (mut app, pad) = library_app(&dir);
        app.insert_resource(LastDevice::Pad);
        let button = crate::app::gamepad::scheme::Pad::Select
            .button()
            .expect("a button");
        let press = |app: &mut App| {
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("pad");
            gamepad.digital_mut().press(button);
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("pad");
            gamepad.digital_mut().release(button);
            gamepad.digital_mut().clear();
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
        };
        let named = shelf_line(&shelf_in(&dir)[0], tr);
        press(&mut app);
        assert_eq!(winners(&dir), ["P1", "P0"], "Select only asks");
        assert_eq!(
            app.world().resource::<Library>().feedback,
            fill(tr.replay_delete_confirm, &[("k", "Select"), ("r", &named)])
        );
        press(&mut app);
        assert_eq!(winners(&dir), ["P0"], "and Select again deletes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The prompt names the key the handler reads, in every language, and
    /// the question names the hands' own control: the key at a keyboard,
    /// the replays table's button on a pad, which its legend names too.
    #[test]
    fn the_delete_is_named_by_the_control_that_does_it() {
        use crate::app::gamepad::{Face, scheme};
        for lang in crate::app::i18n::ALL_LANGS {
            let prompt = lang.tr().prompt_replays;
            assert!(
                prompt.contains(&format!("{DELETE_CAP}:"))
                    || prompt.contains(&format!("{DELETE_CAP} :")),
                "{lang:?}: {prompt}"
            );
        }
        let tr = &crate::app::i18n::EN;
        assert_eq!(delete_control(tr, LastDevice::Keyboard), "D");
        assert_eq!(delete_control(tr, LastDevice::Pad), "Select");
        assert!(scheme::legend(tr, &Face::Replays).contains("Select: delete"));
        // D is nobody else's on this screen: not a global letter, which
        // every screen answers, and not one the shelf walks with.
        assert!(!crate::app::binds::GLOBAL_LETTERS.contains(&DELETE_CAP));
    }

    /// Only a round on the shelf can be deleted: `last.txt`, the
    /// highlight reel and a round from another folder all stay.
    #[test]
    fn only_shelf_rounds_are_deletable() {
        let dir = stocked("delete-shelf-only", 1);
        let elsewhere = stocked("delete-elsewhere", 1);
        for path in [
            dir.join("last.txt"),
            dir.join("highlight.gif"),
            shelf_in(&elsewhere)[0].path.clone(),
        ] {
            assert!(delete_in(&dir, &path).is_err(), "{path:?}");
            assert!(path.exists(), "{path:?} kept");
        }
        // A round that is on the shelf goes, and a second try says why not.
        let round = shelf_in(&dir)[0].path.clone();
        delete_in(&dir, &round).expect("deleted");
        assert!(!round.exists());
        assert!(delete_in(&dir, &round).is_err());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&elsewhere);
    }

    /// A file that will not delete is said on the feedback line rather
    /// than panicked on, and the round stays on the shelf.
    #[test]
    fn a_round_that_will_not_delete_says_so() {
        let tr = &crate::app::i18n::EN;
        let dir = stocked("delete-fails", 1);
        // A directory with a round's name is listed, and will not go as a
        // file does.
        std::fs::create_dir(dir.join(file_name(1_751_400_000, Some("Stuck")))).expect("made");
        let mut library = Library {
            dir: dir.clone(),
            kept: shelf_in(&dir),
            ..Library::default()
        };
        let named = shelf_line(&library.kept[0], tr);
        ask_or_delete(&mut library, tr, LastDevice::Keyboard, 0.0);
        let said = ask_or_delete(&mut library, tr, LastDevice::Keyboard, 1.0);
        assert!(
            said.starts_with(&fill(tr.replay_delete_failed, &[("r", &named), ("e", "")])),
            "{said}"
        );
        assert_eq!(library.kept.len(), 2, "still on the shelf");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Lowering "Rounds kept" trims the shelf at once to the newest; raising
    /// it deletes nothing, even on a shelf already over the new cap.
    #[test]
    fn a_lowered_cap_prunes_at_once_and_a_raised_one_never_does() {
        let dir = stocked("cap-changed", 12);
        cap_changed_in(&dir, 5, 10);
        assert_eq!(winners(&dir).len(), 12, "raised: nothing goes");
        cap_changed_in(&dir, 20, 20);
        assert_eq!(winners(&dir).len(), 12, "unchanged: nothing goes");
        cap_changed_in(&dir, 20, 5);
        assert_eq!(
            winners(&dir),
            ["P11", "P10", "P9", "P8", "P7"],
            "newest kept"
        );
        assert!(dir.join("last.txt").exists() && dir.join("highlight.gif").exists());
        let _ = std::fs::remove_dir_all(&dir);
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
    /// same text, filed under whoever took it, which for forty ticks of
    /// nobody banking anything is nobody. The clipboard itself is not
    /// touched: with `system_clipboard` on it is the real one.
    #[test]
    fn a_round_travels_as_a_code_and_comes_back() {
        let tr = &crate::app::i18n::EN;
        let mut replay = Replay::new(crate::sim::Level::from_board(
            "Turf War",
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
        let mut ended = replay.clone();
        ended.scores = Some(*replay.playback().scores());
        assert_eq!(back, ended.to_text(), "what went in, with how it ended");
        assert_eq!(
            winner, None,
            "filed under the result (a draw), not the beach"
        );
    }

    /// A pasted round is filed under whoever took it, worked out by
    /// playing it through: named as the recording named the seat, or by
    /// its number when it did not.
    fn banked_round() -> Replay {
        let level = crate::sim::Level::parse(
            "name: T\nposts: 1\ncrab: 0,0 R R common\nmap:\n\
             +-+-+\n|. 0|\n+ + +\n|. .|\n+-+-+\n",
        )
        .expect("level");
        let mut replay = Replay::new(level);
        for _ in 0..60 {
            replay.record([crate::sim::PlayerAction::None; crate::sim::MAX_PLAYERS]);
        }
        assert!(replay.playback().scores()[0] > 0, "the crab walked home");
        replay
    }

    #[test]
    fn a_pasted_round_is_filed_under_the_name_of_whoever_took_it() {
        let tr = &crate::app::i18n::EN;
        let mut replay = banked_round();
        replay.names[0] = "Anna".into();
        assert_eq!(winner_of(&replay, tr).as_deref(), Some("Anna"));
    }

    /// A share code carries the recording's text whole, so a pasted round
    /// keeps the level its AI played at, and its row says so.
    #[test]
    fn a_pasted_round_keeps_its_ai_level() {
        let tr = &crate::app::i18n::EN;
        let dir = crate::app::paths::test_dir("pasted-ai");
        let mut replay = banked_round();
        replay.kinds[0] = SeatKind::Ai;
        replay.ai[0] = Some(BotLevel::Hard);
        // A code that claims a result is filed with the playthrough's.
        replay.scores = Some([999; crate::sim::MAX_PLAYERS]);
        let text = replay.to_text();
        let code = crate::share::encode(crate::share::Kind::Round, text.as_bytes());
        assert_eq!(
            keep(crate::share::decode(&code), tr, &dir, 10),
            tr.code_round_saved
        );
        let shelf = shelf_in(&dir);
        let filed = Replay::parse(&std::fs::read_to_string(&shelf[0].path).expect("read"))
            .expect("a round");
        let board = replay.playback();
        assert_eq!(filed.scores, Some(*board.scores()), "the playthrough's");
        assert_eq!(filed.ai, replay.ai);
        assert_eq!(filed.inputs.len(), replay.inputs.len());
        let mode = crate::app::teams::recorded(&replay).played_by(2);
        assert_eq!(
            shelf[0].points,
            Some(crate::app::teams::top_total(board.scores(), 2, mode))
        );
        assert!(shelf[0].points > Some(0), "the crab walked home");
        assert_eq!(who(&shelf[0], tr), "P1 (AI fierce)");
        assert_eq!(players(&shelf[0], tr), "2 players");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pasted_round_with_no_names_is_filed_under_the_seat() {
        let tr = &crate::app::i18n::EN;
        assert_eq!(winner_of(&banked_round(), tr).as_deref(), Some("P1"));
    }

    /// A round played in pairs is filed under the pair, as its card named
    /// it, read off the mode the recording carries through its text.
    #[test]
    fn a_pasted_round_in_pairs_is_filed_under_the_pair() {
        let tr = &crate::app::i18n::EN;
        let level = crate::sim::Level::parse(
            "name: T\nposts: 1\ncrab: 0,0 R R common\nmap:\n\
             +-+-+-+-+-+\n|. 0 1 2 3|\n+ + + + + +\n|. . . . .|\n+-+-+-+-+-+\n",
        )
        .expect("level");
        let mut replay = Replay::new(level);
        for _ in 0..60 {
            replay.record([crate::sim::PlayerAction::None; crate::sim::MAX_PLAYERS]);
        }
        assert_eq!(winner_of(&replay, tr).as_deref(), Some("P1"));
        replay.teams = crate::app::teams::TeamMode::Pairs.index() as u8;
        let back = Replay::parse(&replay.to_text()).expect("reads back");
        assert_eq!(winner_of(&back, tr).as_deref(), Some("P1+P2"));
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

    /// A round recorded by a later build is refused in the player's own
    /// language, pasted, opened from the shelf or dropped on the window,
    /// in every language: never the reader's English.
    #[test]
    fn a_later_builds_round_is_refused_in_the_players_language() {
        let later = round_text(2, &[], &[]).replacen("replay-v3", "replay-v99", 1);
        let code = crate::share::encode(crate::share::Kind::Round, later.as_bytes());
        let refused = Replay::parse(&later).expect_err("a later build's");
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            let pasted = round_from(crate::share::decode(&code), tr).expect_err("refused");
            assert_eq!(
                pasted,
                fill(tr.code_round_bad, &[("e", tr.replay_newer)]),
                "{lang:?}"
            );
            assert_eq!(why_not(tr, &refused), tr.replay_newer, "{lang:?}");
            if lang != crate::app::i18n::Lang::En {
                assert_ne!(tr.replay_newer, crate::sim::NEEDS_NEWER, "{lang:?}");
            }
        }
        // Anything else is said as the reader said it.
        let tr = &crate::app::i18n::EN;
        assert_eq!(why_not(tr, "not a replay"), "not a replay");
    }
}
