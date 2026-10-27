//! The shelf of saved levels: where each one is filed, and F2, which
//! files the level on the bench there.

use super::validate::{save_warning, start_check};
use super::{EditorState, Offer, ask_first, is_checked, level_here};
use crate::app::i18n::fill;
use crate::sim::{Board, LevelKind};
use bevy::prelude::*;

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

/// Whether the level filed at `path` goes by `name`.
fn files_level_named(path: &std::path::Path, name: &str) -> bool {
    filed_name(path).is_some_and(|filed| filed == name)
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
    if is_new_here(path) || state.mine.contains(path) {
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
    match &state.opened {
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
fn check_for_save(state: &mut EditorState, board: &Board, save: (std::path::PathBuf, String)) {
    if state.solver.is_none() {
        start_check(state, board);
    }
    if let Some(check) = state.solver.as_mut() {
        check.save = Some(save);
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
    state.feedback = match write_level(&path, &level.to_text()) {
        Ok(is_new) => {
            // The same level under the name an older build cut it to:
            // one level, so one file, the new one. Only when that file
            // really holds a level of this name, as `level.txt` may well
            // hold another.
            let legacy = ascii_era_path(&state.name);
            let moved = legacy != path && files_level_named(&legacy, &state.name);
            if moved {
                let _ = std::fs::remove_file(&legacy);
            }
            if is_new && !moved {
                saved.write(crate::app::LevelSaved);
            }
            state.mine.insert(path.clone());
            state.filed = Some(super::filed_text(state, board));
            // A warning leads: the line clips at the header's end, and the
            // file's name is the part the author can do without. The level
            // it replaced was named by the question before it.
            let filed = fill(tr.ed_saved_to, &[("file", &file_name(&path))]);
            match save_warning(state, &level, tr) {
                Some(warning) => format!("{warning} - {filed}"),
                None if state.kind == LevelKind::Puzzle && !is_checked(state, board) => {
                    check_for_save(state, board, (path.clone(), state.name.clone()));
                    format!("{} - {filed}", tr.ed_validating)
                }
                None => filed,
            }
        }
        Err(e) => fill(tr.ed_save_failed, &[("e", &e.to_string())]),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
