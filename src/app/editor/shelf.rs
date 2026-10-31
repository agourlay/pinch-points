//! The shelf of saved levels: where each one is filed, and F2, which
//! files the level on the bench there.

use super::validate::{save_warning, start_check};
use super::{EditorState, is_checked, level_here};
use crate::app::gamepad::LastDevice;
use crate::app::i18n::{Tr, fill};
use crate::sim::{Board, LevelKind};
use bevy::prelude::*;

/// What the shelf remembers of the files this bench has written, and
/// what was last filed from it.
#[derive(Default)]
pub(super) struct Filing {
    /// The files this bench has written this session.
    ///
    /// Saving onto any other file that is already there asks first: the
    /// save path is the name, and a level saved in another session, a
    /// friend's code under the same name or a fresh map left on the default
    /// name all land on a file that holds somebody's level. A file written
    /// from here holds this level's earlier self, and saving over it is
    /// the common case, so it goes ahead.
    pub(super) mine: std::collections::HashSet<std::path::PathBuf>,
    /// The file the level on the bench was opened from, and the name it
    /// went by there: F2 saves it back into that file while it keeps that
    /// name ([`save_target`]). Its file is not always the one its name
    /// would make (an older build's `custom.txt`, a file named by hand,
    /// the second of two namesakes), and saving under the name wrote a
    /// second copy of the level beside the first.
    pub(super) opened: Option<(std::path::PathBuf, String)>,
    /// The level's text as F2 last wrote it, or as a starter opened it, so
    /// Esc can tell work already on the shelf, or no work of the author's
    /// at all, from work that would be lost (see [`super::unsaved_work`]).
    pub(super) filed: Option<String>,
}

impl Filing {
    /// Forget every file: the level on the bench now is a level of its
    /// own, and the next F2 asks before writing over any of them.
    pub(super) fn forget(&mut self) {
        let Self {
            mine,
            opened,
            filed,
        } = self;
        mine.clear();
        *opened = None;
        *filed = None;
    }
}

/// Where the editor used to put its one and only level. Still read, so a
/// level saved by an older build is still there; nothing writes it now.
pub fn legacy_save_path() -> std::path::PathBuf {
    crate::app::paths::data_dir().join("levels/custom.txt")
}

/// The shelf every saved level goes on, one file each.
pub fn custom_dir() -> std::path::PathBuf {
    crate::app::paths::data_dir().join("levels/custom")
}

/// Whether no level is filed under this path yet. Read before a write,
/// which would make it true either way.
fn is_new_here(path: &std::path::Path) -> bool {
    !path.exists()
}

/// The name the level filed at `path` goes by, read off its `name:` line.
fn filed_name(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix("name:"))
        .map(|name| name.trim().to_string())
}

/// Whether an older build's file at `legacy` holds the very level being
/// saved as `text`: the bench was opened from it, or it reads back as the
/// same level. Only then is it tidied away. A file of the same name is not
/// enough: every Cyrillic or kana name went to `level.txt`, and a fresh
/// bench on the language's default name met the old level filed there.
fn holds_this_level(state: &EditorState, legacy: &std::path::Path, text: &str) -> bool {
    let opened_there = state
        .filing
        .opened
        .as_ref()
        .is_some_and(|(path, _)| path == legacy);
    opened_there
        || std::fs::read_to_string(legacy)
            .ok()
            .and_then(|old| crate::sim::Level::parse(&old).ok())
            .is_some_and(|old| old.to_text() == text)
}

/// A file's name without its folder: what the status line says a level was
/// saved as. The whole path ran off the header.
pub(super) fn file_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The level a save to `path` would replace, by the name it goes by (its
/// file's name when it has none to read), or `None` when the save
/// replaces nothing of anybody's: no file is there yet, or this bench
/// wrote it this session.
///
/// Any other file holds a level somebody saved: in another session, under
/// the same default name, or as the friend's code just pasted in under
/// its own. Asking only about a file holding a level of another name let
/// all three be written over without a word, and with the file went the
/// cleared tick filed under it, which the new level took over.
fn replaced_level(state: &EditorState, path: &std::path::Path) -> Option<String> {
    if is_new_here(path) || state.filing.mine.contains(path) {
        return None;
    }
    Some(filed_name(path).unwrap_or_else(|| file_name(path)))
}

/// A name for a fresh level that no saved level has: the language's
/// default name, or the first "My Map 2", "My Map 3"... whose file is free.
/// Every fresh bench used to be "My Map", so the second level left
/// unnamed was saved over the first.
pub(super) fn fresh_name(tr: &crate::app::i18n::Tr) -> String {
    let base = tr.ed_default_name;
    std::iter::once(base.to_string())
        .chain((2..).map(|n| format!("{base} {n}")))
        .take(1000)
        .find(|name| is_new_here(&save_path(name)))
        .unwrap_or_else(|| base.to_string())
}

/// Write a level out, and say whether that brought it into being.
///
/// The answer is read before the write, which would make it true either
/// way. The trophies over this count levels, and they say so: "save 10
/// levels in the editor". Counting every F2 counted keystrokes instead,
/// so ten presses on one level took all three of them.
fn write_level(path: &std::path::Path, text: &str) -> std::io::Result<bool> {
    let is_new = is_new_here(path);
    crate::app::paths::write_atomic(path, text)?;
    Ok(is_new)
}

/// Where a build before 0.8.1 filed a level called `name`: its file names
/// kept only ASCII letters and digits, so "Café" went to `caf.txt` and a
/// Russian name to `level.txt`. Read only to tidy such a file away once
/// the level is saved under its whole name (see [`save_level`]), or the shelf
/// would list the level twice.
pub(super) fn ascii_era_path(name: &str) -> std::path::PathBuf {
    let mut stem = String::new();
    for ch in name.chars().take(40) {
        if ch.is_ascii_alphanumeric() {
            stem.push(ch.to_ascii_lowercase());
        } else if !stem.ends_with('-') {
            stem.push('-');
        }
    }
    let stem = match stem.trim_matches('-') {
        "" => "level",
        trimmed => trimmed,
    };
    custom_dir().join(format!("{stem}.txt"))
}

/// Where a level called `name` is filed. One file per name, so saving is
/// keeping rather than replacing.
///
/// The name also has to survive being a file name, and every level is
/// identified by its name (that is what progress is keyed on), so two
/// levels sharing one would share their gold star.
pub(super) fn save_path(name: &str) -> std::path::PathBuf {
    custom_dir().join(format!(
        "{}.txt",
        crate::app::paths::safe_stem(name, 40, "level")
    ))
}

/// Where F2 files the level on the bench: back into the file it was
/// opened from while it goes by the name it had there, else the file its
/// name makes ([`save_path`]). Renamed, it is saved as a level of its own
/// beside the one it was opened from.
pub(super) fn save_target(state: &EditorState) -> std::path::PathBuf {
    match &state.filing.opened {
        Some((path, name)) if *name == state.name => path.clone(),
        Some(_) | None => save_path(&state.name),
    }
}

/// What [`shelve`] did with a level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shelved {
    /// Filed, in the file of this name.
    Saved(String),
    /// Not filed: a level by this name is in its file already, and it is
    /// asked about first.
    Taken(String),
}

/// A level from somewhere else (a code pasted on the menu) put on the
/// shelf under its own name, tidied as the bench tidies one, where the
/// stage list and the map dial find it. Over a file already there it asks
/// first, as F2 does over a file the bench did not write: `replace` is the
/// second press.
pub fn shelve(
    level: &crate::sim::Level,
    replace: bool,
    tr: &crate::app::i18n::Tr,
) -> Result<Shelved, String> {
    let mut level = level.clone();
    level.name = super::naming::tidy_name(&level.name, tr);
    let path = save_path(&level.name);
    if !replace && !is_new_here(&path) {
        return Ok(Shelved::Taken(
            filed_name(&path).unwrap_or_else(|| file_name(&path)),
        ));
    }
    match crate::app::paths::write_atomic(&path, level.to_text()) {
        Ok(()) => Ok(Shelved::Saved(file_name(&path))),
        Err(e) => Err(fill(tr.ed_save_failed, &[("e", &e.to_string())])),
    }
}

/// A stage saved with no check of it as it stands is checked on the way:
/// the check already running joins the save, or one starts, and its
/// answer finishes the save (`validate::poll_solver`). Saved unchecked, a
/// stage joined the list with nobody knowing whether it could be won, and
/// the way through the author had found by hand went nowhere.
fn check_for_save(state: &mut EditorState, board: &Board, path: std::path::PathBuf, name: String) {
    if state.check.solver.is_none() {
        start_check(state, board);
    }
    if let Some(check) = state.check.solver.as_mut() {
        check.join(path, name);
    }
}

/// F2: file the level on the bench under its name.
///
/// Over a file this bench did not write it asks first, naming the level it
/// would replace (see the `Offer` it leaves), and a level an older build
/// filed under a cut-down name moves to its whole one.
pub(super) fn save_level(
    state: &mut EditorState,
    board: &Board,
    (tr, device): (&crate::app::i18n::Tr, crate::app::gamepad::LastDevice),
    saved: &mut MessageWriter<crate::app::LevelSaved>,
) {
    let level = level_here(state, board, &state.name);
    let path = save_target(state);
    let replaced = replaced_level(state, &path);
    // The first press only says whose level is there, and the second
    // replaces it. Written at once, the note that it had been replaced
    // came after the level was already gone.
    let prompt = fill(
        &Offer::Overwrite.warning(tr, device),
        &[("name", replaced.as_deref().unwrap_or_default())],
    );
    if !ask_first(state, Offer::Overwrite, replaced.is_some(), prompt) {
        return;
    }
    let text = level.to_text();
    state.feedback = match write_level(&path, &text) {
        Ok(is_new) => {
            // The same level under the name an older build cut it to:
            // one level, so one file, the new one. Only when that file
            // really holds this level, as `level.txt` may well hold
            // another of the same name.
            let legacy = ascii_era_path(&state.name);
            let moved = legacy != path && holds_this_level(state, &legacy, &text);
            if moved {
                let _ = std::fs::remove_file(&legacy);
            }
            if is_new && !moved {
                saved.write(crate::app::LevelSaved);
            }
            state.filing.mine.insert(path.clone());
            state.filing.filed = Some(super::filed_text(state, board));
            // A warning leads: the line clips at the header's end, and the
            // file's name is the part the author can do without. The level
            // it replaced was named by the question before it.
            let filed = fill(tr.ed_saved_to, &[("file", &file_name(&path))]);
            match save_warning(state, &level, tr) {
                Some(warning) => format!("{warning} - {filed}"),
                None if state.kind == LevelKind::Puzzle && !is_checked(state, board) => {
                    check_for_save(state, board, path.clone(), state.name.clone());
                    format!("{} - {filed}", tr.ed_validating)
                }
                None => filed,
            }
        }
        Err(e) => fill(tr.ed_save_failed, &[("e", &e.to_string())]),
    };
}

/// A key that throws the level on the board away, pressed once and waiting
/// to be pressed again. One at a time: pressing anything else, the other
/// one included, takes the offer back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Offer {
    /// Enter on "My levels": the level chosen there in place of this one.
    Open,
    /// F2: this level saved over a file this bench did not write.
    Overwrite,
    /// Esc: back to the menu, with what has not been saved left behind.
    Leave,
}

impl Offer {
    /// What the first press says is about to go, naming the key that takes
    /// the offer up: for a pad's player, the pad's button for it, from the
    /// editor's table the bridge presses that key by.
    pub(super) fn warning(self, tr: &Tr, device: LastDevice) -> String {
        use crate::app::gamepad::Face;
        use crate::app::gamepad::scheme::{Does, button_name};
        // The face the bridge presses the key from: the list's for an open,
        // the bench under Select for a save, its base layer for leaving.
        let (keys, pad, face, does) = match self {
            Offer::Open => (
                tr.ed_open_confirm,
                tr.pad.open_confirm,
                Face::LevelList,
                Does::Open,
            ),
            Offer::Overwrite => (
                tr.ed_save_taken,
                tr.pad.save_taken,
                Face::Editor { layer: true },
                Does::Save,
            ),
            Offer::Leave => (
                tr.ed_leave_confirm,
                tr.pad.leave_confirm,
                Face::Editor { layer: false },
                Does::Menu,
            ),
        };
        match (device, button_name(&face, does, tr, device.family())) {
            (LastDevice::Pad(_), Some(button)) => fill(pad, &[("b", &button)]),
            _ => keys.to_string(),
        }
    }

    pub(super) fn key(self) -> KeyCode {
        match self {
            Offer::Open => KeyCode::Enter,
            Offer::Overwrite => KeyCode::F2,
            Offer::Leave => KeyCode::Escape,
        }
    }
}

/// Whether to go ahead with what `offer`'s key throws away.
///
/// With nothing `at_stake` it goes ahead at once. With something, the
/// first press only says what is about to go (`prompt`) and leaves the
/// offer standing, and a second press of the same key takes it up. Going
/// ahead always lets the offer go.
pub(super) fn ask_first(
    state: &mut EditorState,
    offer: Offer,
    at_stake: bool,
    prompt: String,
) -> bool {
    if at_stake && state.offer != Some(offer) {
        state.offer = Some(offer);
        state.asked = Asked {
            line: prompt.clone(),
            cursors: None,
        };
        state.feedback = prompt;
        return false;
    }
    state.offer = None;
    true
}

/// What stood when the offer was made: the question's own line, and where
/// the cursors were (noted the first frame after it was asked). The offer
/// is only good while both still stand ([`settle_offer`]).
#[derive(Default)]
pub(super) struct Asked {
    line: String,
    cursors: Option<Vec<(u8, u8)>>,
}

/// Take the standing offer back, and its question with it while the line
/// still asks it: a question left on screen with no offer behind it
/// promised a second press that would only ask again.
pub(super) fn take_back_offer(state: &mut EditorState) {
    if state.offer.take().is_some() && state.feedback == state.asked.line {
        state.feedback.clear();
    }
}

/// Let the standing offer go once the question is no longer what the
/// author is looking at: its line replaced by another (a check's verdict
/// landing between the two presses), or a cursor moved without a key (a
/// pad's stick). Either way the second press would have gone ahead with
/// nothing on screen saying it would.
pub(super) fn settle_offer(state: &mut EditorState, cursors: impl Iterator<Item = (u8, u8)>) {
    if state.offer.is_none() {
        return;
    }
    if state.feedback != state.asked.line {
        state.offer = None;
        return;
    }
    let now: Vec<_> = cursors.collect();
    match &state.asked.cursors {
        Some(was) if *was != now => take_back_offer(state),
        Some(_) => {}
        None => state.asked.cursors = Some(now),
    }
}

/// Whether anything has been built on this board yet.
///
/// Asked by comparing the level here against the one a fresh board of the
/// same size would give, which is cheaper to be sure of than a list of
/// every way a beach can be marked: a wall, a rock, a pool, a crab, a
/// castle, a gull, the flock, the wrap. A blank beach is worth nothing.
fn has_work(state: &EditorState, board: &Board) -> bool {
    // The same seed, so the comparison is about what has been built and
    // not about where the board came from: a level's text carries its
    // seed, and a blank board with a different one reads as a difference.
    let blank = Board::new(board.width(), board.height(), board.seed());
    level_here(state, board, "x").to_text() != level_here(state, &blank, "x").to_text()
}

/// Whether throwing the bench away now would lose work: something is
/// built here, and it is not the level F2 last wrote. Esc and F4 each
/// ask first on it: a blank beach is worth nothing, and a level just saved
/// is on the shelf, so over either they go at once, where the old
/// question said "this one goes" of a level that was safe.
pub(super) fn unsaved_work(state: &EditorState, board: &Board) -> bool {
    has_work(state, board)
        && state.filing.filed.as_deref() != Some(filed_text(state, board).as_str())
}

/// The level on the bench as [`shelf::Filing::filed`] remembers it: under
/// its name and without the way through it carries. A way through found
/// after a save is the check's work, not the author's, and with it the
/// level just saved read as unsaved.
pub(super) fn filed_text(state: &EditorState, board: &Board) -> String {
    let mut level = level_here(state, board, &state.name);
    level.solution.clear();
    level.to_text()
}

#[cfg(test)]
mod tests {
    use super::super::tests::{bench_app, tap, tap_and_settle};
    use super::super::*;
    use super::*;
    use crate::sim::{CrabKind, Handedness, TileKind};

    /// Two levels with different names must not land on the same file:
    /// progress is filed under the name, so sharing a file means sharing a
    /// gold star. The character rule itself lives with `safe_stem`.
    #[test]
    fn different_names_are_different_files() {
        assert_ne!(save_path("First Beach"), save_path("Second Beach"));
        assert!(
            save_path("Gull Alley").ends_with("gull-alley.txt"),
            "{:?}",
            save_path("Gull Alley")
        );
    }

    /// A level counts once, when it comes into being. The trophies over
    /// this say "save 10 levels", and every F2 used to raise one, so ten
    /// presses on a single level took all three of them.
    #[test]
    fn saving_the_same_level_again_does_not_build_a_second_one() {
        let dir = crate::app::paths::test_dir("built");
        let path = dir.join("my-beach.txt");

        assert!(write_level(&path, "one").expect("wrote"), "a new level");
        assert!(
            !write_level(&path, "two").expect("wrote"),
            "the same level again"
        );
        assert!(
            !write_level(&path, "three").expect("wrote"),
            "and again, however long the key is held"
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("reads"),
            "three",
            "every save still lands; only the counting stops"
        );

        // A level deleted and rebuilt is a level built again.
        std::fs::remove_file(&path).expect("removed");
        assert!(write_level(&path, "four").expect("wrote"), "built afresh");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Esc resets the bench on the way out, so over unsaved work it asks
    /// first, as F4 does; over a blank beach or a level just saved
    /// it leaves at once.
    #[test]
    fn leaving_with_an_unsaved_level_asks_twice() {
        let mut app = bench_app();
        app.insert_state(Screen::Editor);
        app.add_systems(Update, (editor_input, editor_commands).chain());
        app.add_systems(OnExit(Screen::Editor), exit_editor);
        let name = "Leave Probe";
        app.world_mut().resource_mut::<EditorState>().name = name.to_string();

        let screen = |app: &App| *app.world().resource::<State<Screen>>().get();

        app.world_mut().resource_mut::<Sim>().0.set_wrap(true);
        tap_and_settle(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Screen::Editor, "the first press only asks");
        assert_eq!(
            app.world().resource::<EditorState>().feedback,
            crate::app::i18n::EN.ed_leave_confirm
        );
        assert!(app.world().resource::<Sim>().0.wrap(), "the level stays");
        // Another key takes the offer back, so Esc asks again.
        tap_and_settle(&mut app, KeyCode::F6);
        tap_and_settle(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Screen::Editor, "asked again");
        tap_and_settle(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Screen::Menu, "the second press leaves");

        // Saved as it stands, the level is not lost: one press leaves.
        app.insert_state(Screen::Editor);
        app.update();
        let mut state = app.world_mut().resource_mut::<EditorState>();
        state.name = name.to_string();
        app.world_mut().resource_mut::<Sim>().0.set_wrap(true);
        let path = save_path(name);
        let _ = std::fs::remove_file(&path);
        tap_and_settle(&mut app, KeyCode::F2);
        assert!(std::fs::read_to_string(&path).is_ok_and(|text| text.contains(name)));
        tap_and_settle(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Screen::Menu, "nothing unsaved to ask about");
        let _ = std::fs::remove_file(&path);
    }

    /// A question stands only while it is on screen and the cursor is
    /// where it was asked: a check's verdict landing on its line, or a
    /// pad's stick walking the cursor off, takes it back, and the next
    /// press asks again rather than leaving with no warning showing.
    #[test]
    fn a_question_goes_with_its_line_or_a_cursor_moved() {
        let mut app = bench_app();
        app.insert_state(Screen::Editor);
        let cursor = app
            .world_mut()
            .spawn((crate::app::cursor::Cursor::seated(0), Transform::default()))
            .id();
        app.add_systems(Update, (editor_input, editor_commands).chain());
        let screen = |app: &App| *app.world().resource::<State<Screen>>().get();
        let asked = |app: &App| app.world().resource::<EditorState>().offer;
        app.world_mut().resource_mut::<Sim>().0.set_wrap(true);

        tap_and_settle(&mut app, KeyCode::Escape);
        assert_eq!(asked(&app), Some(Offer::Leave));
        // The verdict of a check the author started takes the line.
        app.world_mut().resource_mut::<EditorState>().feedback = "solvable".into();
        app.update();
        assert_eq!(asked(&app), None, "the verdict took the question");
        assert_eq!(app.world().resource::<EditorState>().feedback, "solvable");
        tap_and_settle(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Screen::Editor, "asked again");
        assert_eq!(asked(&app), Some(Offer::Leave));

        // The stick moves the cursor with no key pressed.
        app.world_mut()
            .get_mut::<crate::app::cursor::Cursor>(cursor)
            .expect("cursor")
            .x += 1;
        app.update();
        assert_eq!(asked(&app), None, "the cursor walked off it");
        assert_eq!(
            app.world().resource::<EditorState>().feedback,
            "",
            "and its line went with it"
        );
        tap_and_settle(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Screen::Editor, "asked again");
        tap_and_settle(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Screen::Menu, "the second press leaves");
    }

    /// F2 onto a file this bench did not write asks first: the first
    /// press leaves the other level's file as it was and names the level
    /// in it, and only a second press replaces it. Saved at once, the note
    /// that a level had been replaced arrived with that level already
    /// gone.
    #[test]
    fn saving_over_another_levels_file_asks_twice() {
        let mut app = bench_app();
        app.add_systems(Update, (editor_input, editor_commands));
        // A name of this test's own: the shelf is shared by every test in
        // the process.
        let name = "Пляж Overwrite Probe";
        app.world_mut().resource_mut::<EditorState>().name = name.to_string();
        let path = save_path(name);
        crate::app::paths::write_atomic(&path, "name: Their Beach\n").expect("wrote");

        let on_disk = || std::fs::read_to_string(&path).expect("reads");
        tap(&mut app, KeyCode::F2);
        assert_eq!(on_disk(), "name: Their Beach\n", "the first press asks");
        assert_eq!(
            app.world().resource::<EditorState>().feedback,
            fill(
                crate::app::i18n::EN.ed_save_taken,
                &[("name", "Their Beach")]
            ),
            "naming the level it would replace"
        );
        // Anything else pressed in between takes the offer back.
        tap(&mut app, KeyCode::KeyO);
        tap(&mut app, KeyCode::F2);
        assert_eq!(on_disk(), "name: Their Beach\n", "asked again");
        tap(&mut app, KeyCode::F2);
        assert!(on_disk().contains(name), "the second press saves");
        // Now it is this bench's own file, and saves at once.
        app.world_mut().resource_mut::<Sim>().0.set_wrap(false);
        tap(&mut app, KeyCode::F2);
        assert!(
            app.world().resource::<EditorState>().offer.is_none(),
            "no question about its own file"
        );
        let _ = std::fs::remove_file(&path);

        // Filed by an older build under the ASCII it kept of the name: the
        // level moves to its whole name, and is not listed twice.
        let name = "Café Legacy Probe";
        let legacy = ascii_era_path(name);
        assert!(legacy.ends_with("caf-legacy-probe.txt"), "{legacy:?}");
        app.world_mut().resource_mut::<EditorState>().name = name.to_string();
        let bench = |app: &App| {
            let state = app.world().resource::<EditorState>();
            super::super::level_here(state, &app.world().resource::<Sim>().0, name).to_text()
        };
        crate::app::paths::write_atomic(&legacy, bench(&app)).expect("wrote");
        tap(&mut app, KeyCode::F2);
        assert!(save_path(name).exists(), "saved under its whole name");
        assert!(!legacy.exists(), "and the old file tidied away");
        let _ = std::fs::remove_file(save_path(name));
    }

    /// An older build filed every Cyrillic or kana name under the ASCII it
    /// kept, so one old file holds whatever level last went by the
    /// language's default name. A fresh bench saved under that name is
    /// another level: the old file stays.
    #[test]
    fn an_older_builds_file_of_another_level_is_left_alone() {
        let mut app = bench_app();
        app.add_systems(Update, (editor_input, editor_commands));
        for name in ["Моя карта Legacy Probe", "わたしのマップ Legacy Probe"] {
            let legacy = ascii_era_path(name);
            assert!(legacy.ends_with("legacy-probe.txt"), "{legacy:?}");
            let theirs = format!(
                "name: {name}\nposts: 0\ncrab: 0,0 R R common\nmap:\n+-+-+\n|. 0|\n+-+-+\n"
            );
            crate::app::paths::write_atomic(&legacy, &theirs).expect("wrote");
            let _ = std::fs::remove_file(save_path(name));
            app.world_mut().resource_mut::<EditorState>().name = name.to_string();
            tap(&mut app, KeyCode::F2);
            assert!(
                save_path(name).exists(),
                "{name}: saved under its whole name"
            );
            assert_eq!(
                std::fs::read_to_string(&legacy).ok().as_deref(),
                Some(theirs.as_str()),
                "{name}: the old level is still there"
            );
            let _ = std::fs::remove_file(save_path(name));
            let _ = std::fs::remove_file(&legacy);
        }
    }

    /// A file holding a level of this very name is still somebody's level
    /// when this bench did not write it: one saved in another session, or
    /// the one a pasted code of the same name came from. Saved at once, a
    /// second "My Map" took the first one's file and its cleared tick.
    #[test]
    fn a_namesake_saved_elsewhere_is_asked_about() {
        let mut app = bench_app();
        app.add_systems(Update, (editor_input, editor_commands));
        let name = "Namesake Probe";
        app.world_mut().resource_mut::<EditorState>().name = name.to_string();
        let path = save_path(name);
        let theirs = format!("name: {name}\nold: true\n");
        crate::app::paths::write_atomic(&path, &theirs).expect("wrote");
        app.world_mut().resource_mut::<Sim>().0.set_wrap(true);

        tap(&mut app, KeyCode::F2);
        assert_eq!(std::fs::read_to_string(&path).expect("reads"), theirs);
        assert_eq!(
            app.world().resource::<EditorState>().offer,
            Some(Offer::Overwrite),
            "the first press asks"
        );
        tap(&mut app, KeyCode::F2);
        assert!(
            !std::fs::read_to_string(&path)
                .expect("reads")
                .contains("old: true")
        );
        let _ = std::fs::remove_file(&path);
    }

    /// The editor opens on a name no saved level has, and a fresh map
    /// takes another: every bench used to open on "My Map", so the second
    /// level left unnamed was saved over the first.
    #[test]
    fn a_fresh_bench_takes_a_name_no_saved_level_has() {
        // A language of the test's own, so its default name's files are
        // this test's alone on the shared shelf.
        let lang = crate::app::i18n::Lang::It;
        let tr = lang.tr();
        let settings = GameSettings {
            language: lang,
            ..GameSettings::default()
        };
        let mut app = bench_app();
        app.insert_resource(settings);
        app.insert_state(Screen::Editor);
        app.add_systems(OnEnter(Screen::Editor), enter_editor);
        app.add_systems(Update, (editor_input, editor_commands).chain());
        let base = tr.ed_default_name;
        let second = format!("{base} 2");
        let third = format!("{base} 3");
        for name in [base, &second, &third] {
            let _ = std::fs::remove_file(save_path(name));
        }
        crate::app::paths::write_atomic(&save_path(base), "name: x\n").expect("wrote");
        app.update();
        let name = |app: &App| app.world().resource::<EditorState>().name.clone();
        assert_eq!(name(&app), second, "the default is taken");

        // Saved, then a fresh map: a new level under a new name, which the
        // next F2 does not write over the one just saved.
        app.world_mut().resource_mut::<Sim>().0.set_wrap(true);
        tap(&mut app, KeyCode::F2);
        let saved = std::fs::read_to_string(save_path(&second)).expect("saved");
        {
            let world = app.world_mut();
            let board = world.resource::<Sim>().0.clone();
            world.resource_mut::<EditorState>().start_afresh(tr, &board);
        }
        assert_eq!(name(&app), third, "a fresh map, a fresh name");
        assert!(app.world().resource::<EditorState>().filing.mine.is_empty());
        app.world_mut()
            .resource_mut::<Sim>()
            .0
            .set_tile(1, 1, TileKind::Rock);
        tap(&mut app, KeyCode::F2);
        assert_eq!(
            std::fs::read_to_string(save_path(&second)).expect("still there"),
            saved,
            "the level saved before the fresh map is left alone"
        );
        assert!(save_path(&third).exists());
        // Renamed onto the level saved before, the fresh map asks.
        app.world_mut().resource_mut::<EditorState>().name = second.clone();
        tap(&mut app, KeyCode::F2);
        assert_eq!(
            app.world().resource::<EditorState>().offer,
            Some(Offer::Overwrite)
        );
        for name in [base, &second, &third] {
            let _ = std::fs::remove_file(save_path(name));
        }
    }

    /// A save says what it saved as by the file's name alone, warns first
    /// about a level nobody can win, and checks a stage nobody has checked.
    #[test]
    fn a_save_names_its_file_and_warns() {
        use crate::app::i18n::EN;
        let mut app = bench_app();
        app.add_systems(Update, (editor_input, editor_commands, poll_solver).chain());
        let name = "Warning Probe";
        app.world_mut().resource_mut::<EditorState>().name = name.into();
        app.world_mut().resource_mut::<Sim>().0.spawn_crab(
            1,
            1,
            Direction::Right,
            Handedness::Left,
            CrabKind::Common,
        );
        let path = save_path(name);
        let _ = std::fs::remove_file(&path);
        tap(&mut app, KeyCode::F2);
        let said = app.world().resource::<EditorState>().feedback.clone();
        let file = path.file_name().expect("a file").to_string_lossy();
        let filed = fill(EN.ed_saved_to, &[("file", &file)]);
        assert_eq!(said, format!("{} - {filed}", EN.ed_puzzle_no_castle));
        assert!(!said.contains('/'), "no folders: {said}");

        app.world_mut()
            .resource_mut::<Sim>()
            .0
            .set_tile(4, 1, TileKind::Castle(0));
        tap(&mut app, KeyCode::F2);
        let said = app.world().resource::<EditorState>().feedback.clone();
        // The check may already be back within the frame that saved.
        let done = format!("{} - {filed}", EN.ed_solvable_free);
        assert!(
            said == format!("{} - {filed}", EN.ed_validating) || said == done,
            "{said}"
        );
        // Saved unchecked, a stage is checked on the way, and the answer
        // finishes the save: the verdict first, then the file.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        loop {
            app.update();
            if app.world().resource::<EditorState>().check.solver.is_none() {
                break;
            }
            std::thread::yield_now();
        }
        assert_eq!(
            app.world().resource::<EditorState>().feedback,
            format!("{} - {filed}", EN.ed_solvable_free)
        );
        // Checked, and saved again as it stands: nothing to warn about.
        tap(&mut app, KeyCode::F2);
        assert_eq!(app.world().resource::<EditorState>().feedback, filed);
        let _ = std::fs::remove_file(&path);
    }

    /// Esc over F4's or F2's question takes the question back and
    /// stays: the editor took the question back and then left, with the
    /// level the question was about.
    #[test]
    fn escape_over_a_question_only_takes_it_back() {
        let mut app = bench_app();
        app.insert_state(Screen::Editor);
        app.add_systems(Update, (editor_input, editor_commands).chain());
        let screen = |app: &App| *app.world().resource::<State<Screen>>().get();
        // A level just saved, so Esc on its own would leave at once.
        let name = "Escape Probe";
        app.world_mut().resource_mut::<EditorState>().name = name.into();
        let path = save_path(name);
        let _ = std::fs::remove_file(&path);
        app.world_mut().resource_mut::<Sim>().0.set_wrap(true);
        tap_and_settle(&mut app, KeyCode::F2);
        // F2 again under another name whose file is somebody's: a question.
        let other = "Escape Probe Other";
        crate::app::paths::write_atomic(&save_path(other), "name: Other\n").expect("wrote");
        app.world_mut().resource_mut::<EditorState>().name = other.into();
        app.world_mut().resource_mut::<EditorState>().filing.filed = None;
        tap_and_settle(&mut app, KeyCode::F2);
        assert_eq!(
            app.world().resource::<EditorState>().offer,
            Some(Offer::Overwrite)
        );
        tap_and_settle(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Screen::Editor, "still at the bench");
        let state = app.world().resource::<EditorState>();
        assert_eq!(state.offer, None, "the question is taken back");
        assert!(state.feedback.is_empty(), "{}", state.feedback);
        let _ = std::fs::remove_file(save_path(other));
        let _ = std::fs::remove_file(&path);
    }
}
