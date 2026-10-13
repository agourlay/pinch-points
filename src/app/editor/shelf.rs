//! The shelf of saved levels: where each one is filed, and F2, which
//! files the level on the bench there.

use super::validate::orphan_warning;
use super::{EditorState, Offer, level_here};
use crate::app::i18n::fill;
use crate::sim::Board;
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

/// Whether the level filed at `path` goes by `name`, which makes it this
/// level and not another: levels are known by their name, progress and
/// all. A level pasted back in and saved again is its own file's owner,
/// and asking twice before writing it reads as somebody else's.
fn files_level_named(path: &std::path::Path, name: &str) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| {
        text.lines()
            .find_map(|line| line.strip_prefix("name:"))
            .is_some_and(|filed| filed.trim() == name)
    })
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

/// F2: file the level on the bench under its name.
///
/// Over a file of somebody else's it asks first (see the `Offer` it
/// leaves), and a level an older build filed under a cut-down name moves
/// to its whole one.
pub(super) fn save_level(
    state: &mut EditorState,
    board: &Board,
    tr: &crate::app::i18n::Tr,
    saved: &mut MessageWriter<crate::app::LevelSaved>,
) {
    let level = level_here(state, board, &state.name);
    let path = save_path(&state.name);
    let stranger = !is_new_here(&path)
        && !state.mine.contains(&state.name)
        && !files_level_named(&path, &state.name);
    // A file this session did not write is somebody else's level until
    // the author says otherwise: the first press only says whose it
    // is, and the second replaces it. Written at once, the note that
    // it had been replaced came after the level was already gone.
    if stranger && state.offer != Some(Offer::Overwrite) {
        state.offer = Some(Offer::Overwrite);
        state.feedback = tr.ed_save_taken.into();
    } else {
        state.offer = None;
        state.feedback = match write_level(&path, &level.to_text()) {
            Ok(is_new) => {
                // The same level under the name an older build cut it
                // to: one level, so one file, the new one. Only when
                // that file really holds a level of this name, as
                // `level.txt` may well hold another.
                let legacy = ascii_era_path(&state.name);
                let moved = legacy != path && files_level_named(&legacy, &state.name);
                if moved {
                    let _ = std::fs::remove_file(&legacy);
                }
                if is_new && !moved {
                    saved.write(crate::app::LevelSaved);
                }
                let named = state.name.clone();
                state.mine.insert(named);
                let filed = fill(tr.ed_saved_to, &[("path", &path.display().to_string())]);
                let orphans = orphan_warning(state, &level, tr);
                let notes = [stranger.then_some(tr.ed_saved_over), orphans.as_deref()];
                notes
                    .iter()
                    .flatten()
                    .fold(filed, |line, note| format!("{line} - {note}"))
            }
            Err(e) => fill(tr.ed_save_failed, &[("e", &e.to_string())]),
        };
    }
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
        let dir = std::env::temp_dir().join(format!("pinch-built-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory to save into");
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
