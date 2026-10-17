//! Where the player's files live: the XDG base directories, so a packaged
//! build writes into the user's home rather than beside its executable.
//!
//! Data (progress, achievements, replays, custom levels, the suspended
//! round) goes under `$XDG_DATA_HOME/pinch-points`; configuration (the
//! settings file) under `$XDG_CONFIG_HOME/pinch-points`. Per the spec, a
//! relative value in either variable must be ignored, and the defaults are
//! `~/.local/share` and `~/.config`.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The game's data directory, created lazily by whoever writes into it.
pub fn data_dir() -> PathBuf {
    #[cfg(test)]
    return test_home().join("data");
    #[cfg(not(test))]
    resolve(std::env::var_os("XDG_DATA_HOME"), &[".local", "share"])
}

/// A record in the data directory that a save merges with what is on disk
/// (progress, trophies), rather than writing over it.
///
/// Under test, one folder per test thread, which libtest names after the
/// test: tests run side by side in one process, and a save that reads the
/// file back in picked up the counts another test had just written,
/// so a test of "this round counts nothing" saw another's round.
pub fn merged_record(name: &str) -> PathBuf {
    #[cfg(test)]
    {
        let thread = std::thread::current();
        let folder: String = thread
            .name()
            .unwrap_or("unnamed")
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        data_dir().join(folder).join(name)
    }
    #[cfg(not(test))]
    data_dir().join(name)
}

/// The game's configuration directory.
pub fn config_dir() -> PathBuf {
    #[cfg(test)]
    return test_home().join("config");
    #[cfg(not(test))]
    resolve(std::env::var_os("XDG_CONFIG_HOME"), &[".config"])
}

/// Where a unit test's saves land instead: a folder of its own in the
/// temp directory, one per test process.
///
/// Without it every test that walked through a save wrote the developer's
/// own files. The lobby's naming tests call `GameSettings::save` on their
/// way through, and each `cargo test` put the real settings file back to
/// defaults with a player called Bob in it.
#[cfg(test)]
fn test_home() -> PathBuf {
    std::env::temp_dir().join(format!("pinch-points-test-{}", std::process::id()))
}

/// A test's own scratch folder in the temp directory, named for `name`
/// and this process, and empty: gone and made again, so nothing an
/// earlier run left in it is read as this one's.
#[cfg(test)]
pub fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pinch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir
}

/// A player's own words made safe to use as a file name: the name of a
/// level they built.
///
/// A safety rule, not a formatting one: a name like `Crab/../etc` must not
/// become a path, and a name is typed by whoever is at the keyboard. The
/// replay shelf is the one exception, with a rule of its own in
/// `replays::file_name`: the winner is read back out of that file name for
/// the shelf's label, so it keeps its case and only trades what is not a
/// letter or digit for `_`, which is just as safe.
///
/// Letters and digits of any script are kept, lowercased: kept to ASCII,
/// every Russian or Japanese name came down to its digits or to nothing,
/// so "Моя карта" and "わたしのマップ" both saved to the fallback and
/// "Карта 2" and "Пляж 2" both to `2`, each overwriting the other.
///
/// `keep` bounds the result so a long name cannot make an absurd path, and
/// `fallback` covers a name with nothing usable left in it, which is a
/// thing a player can type.
pub fn safe_stem(name: &str, keep: usize, fallback: &str) -> String {
    let mut out = String::new();
    for ch in name.chars().take(keep) {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    let stem = if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    };
    // Windows keeps these names for devices, whatever the extension:
    // `con.txt` is the console, and a level called Con never reached the
    // disk. The `_` cannot come out of a name, so the stem stays its own.
    match is_reserved_on_windows(&stem) {
        true => format!("{stem}_"),
        false => stem,
    }
}

/// Whether Windows reserves `stem` for a device: CON, PRN, AUX, NUL and
/// COM1-9 and LPT1-9, in any case.
fn is_reserved_on_windows(stem: &str) -> bool {
    let stem = stem.to_ascii_lowercase();
    if matches!(stem.as_str(), "con" | "prn" | "aux" | "nul") {
        return true;
    }
    ["com", "lpt"].iter().any(|port| {
        stem.strip_prefix(port).is_some_and(|n| {
            // The superscript digits too: Windows reserves COM¹ as well.
            matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
                || matches!(n, "\u{b9}" | "\u{b2}" | "\u{b3}")
        })
    })
}

/// Create the directory a file is about to be written into, private to the
/// player (0700) as the XDG spec asks of directories created on its behalf.
/// Best-effort, like the save files themselves: a disk that will not take it
/// surfaces at the write, not here.
pub fn ensure_parent(path: &Path) {
    if let Some(dir) = path.parent() {
        let mut dirs = std::fs::DirBuilder::new();
        dirs.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut dirs, 0o700);
        let _ = dirs.create(dir);
    }
}

/// Write a player file whole or not at all: the bytes land in a `.tmp`
/// sibling, are flushed to the disk itself, and only then does a rename
/// give them the real name. A crash or a full disk mid-write leaves the old
/// file standing, where a plain write would leave a truncated one, which
/// the lenient parsers would read as a fresh start.
///
/// The `sync_all` is what extends that to a *crash* rather than merely a
/// failed write: without it the rename can reach the disk while the bytes
/// it published are still in the page cache, and a power loss leaves the
/// truncated file this exists to avoid. A failed write takes its
/// half-finished `.tmp` with it.
pub fn write_atomic(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    ensure_parent(path);
    let tmp = tmp_sibling(path);
    let landed = spill(&tmp, contents.as_ref()).and_then(|()| std::fs::rename(&tmp, path));
    if landed.is_err() {
        // Nothing half-written is left to be found later, whichever step
        // gave out.
        let _ = std::fs::remove_file(&tmp);
    }
    landed
}

/// Where [`write_atomic`] stages a file: beside it, named for this
/// process. Two copies of the game saving the same file at once each
/// stage their own, where one shared `.tmp` let the second truncate the
/// first's between its flush and its rename, publishing half a file.
fn tmp_sibling(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}.tmp", std::process::id()));
    PathBuf::from(tmp)
}

/// The bytes onto the disk itself, not merely into the page cache.
fn spill(tmp: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(tmp)?;
    file.write_all(contents)?;
    file.sync_all()
}

/// What a save file said that this build cannot read, kept word for word
/// so the next save writes it back: a line a newer build added. Read and
/// dropped, one game in an older build cost the player it for good.
///
/// In the order first met, each once: a file that had picked up a line
/// twice is not made to grow it again on every save.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct ForeignLines(Vec<String>);

impl ForeignLines {
    /// Keep `line` (its trailing blanks trimmed) unless it is kept already.
    pub fn keep(&mut self, line: &str) {
        let line = line.trim_end();
        if !self.0.iter().any(|kept| kept == line) {
            self.0.push(line.to_string());
        }
    }

    /// Keep every line `other` kept.
    pub fn extend(&mut self, other: ForeignLines) {
        for line in other.0 {
            self.keep(&line);
        }
    }

    /// Forget them all.
    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    /// Write each kept line onto the end of `text`, a line apiece.
    pub fn append_to(&self, text: &mut String) {
        for line in &self.0 {
            text.push_str(line);
            text.push('\n');
        }
    }
}

/// The XDG rule, injected values so it can be tested without touching the
/// process environment: an absolute `$XDG_*` wins, anything else falls back
/// to the conventional spot under home.
fn resolve(var: Option<OsString>, home_default: &[&str]) -> PathBuf {
    var.map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .unwrap_or_else(|| {
            let mut dir = home();
            for part in home_default {
                dir.push(part);
            }
            dir
        })
        .join("pinch-points")
}

/// `$HOME`, or `%USERPROFILE%` where that is what exists. An environment
/// with neither gets `.`, which is the old beside-the-game behaviour.
fn home() -> PathBuf {
    ["HOME", "USERPROFILE"]
        .into_iter()
        .find_map(|var| std::env::var_os(var).filter(|v| !v.is_empty()))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    /// The rule exists so a name typed by a player cannot become a path.
    /// Everything else about it is presentation.
    #[test]
    fn a_name_becomes_a_file_name() {
        use super::safe_stem;
        assert_eq!(safe_stem("Gull Alley", 40, "level"), "gull-alley");
        assert_eq!(safe_stem("  spaced  out  ", 40, "level"), "spaced-out");
        assert_eq!(safe_stem("Crab/../etc", 40, "level"), "crab-etc");
        // Never empty, however little is left of it.
        assert_eq!(safe_stem("***", 40, "level"), "level");
        assert_eq!(safe_stem("", 40, "level"), "level");
        // Long names are cut rather than making a silly path.
        assert!(safe_stem(&"x".repeat(200), 40, "level").len() <= 40);
        assert!(safe_stem(&"x".repeat(200), 12, "round").len() <= 12);
    }

    /// Names in any script keep their letters, so two of them only share
    /// a file when they would read the same.
    #[test]
    fn a_name_in_any_script_keeps_its_letters() {
        use super::safe_stem;
        assert_eq!(safe_stem("Моя карта", 40, "level"), "моя-карта");
        assert_eq!(safe_stem("わたしのマップ", 40, "level"), "わたしのマップ");
        assert_ne!(
            safe_stem("Карта 2", 40, "level"),
            safe_stem("Пляж 2", 40, "level")
        );
        assert_eq!(safe_stem("Café *3*", 40, "level"), "café-3");
        // Bounded in characters, and still far inside any file system's
        // limit in bytes.
        assert!(safe_stem(&"я".repeat(200), 40, "level").len() <= 255);
    }

    /// A level called Con, Nul or Com1 is not a Windows device.
    #[test]
    fn a_device_name_is_not_a_file_name() {
        use super::safe_stem;
        for name in ["Con", "PRN", "aux", "Nul", "COM1", "lpt9", "Com\u{b9}"] {
            let stem = safe_stem(name, 40, "level");
            assert!(stem.ends_with('_'), "{name} -> {stem}");
        }
        for name in ["Cone", "Com", "Com0", "Com10", "Lpt 1", "Console"] {
            let stem = safe_stem(name, 40, "level");
            assert!(!stem.ends_with('_'), "{name} -> {stem}");
        }
    }

    use super::*;

    /// The whole point of the rename dance: a rewrite replaces the file in
    /// one step, the parent chain appears on demand (privately, on unix),
    /// and no `.tmp` sibling is left lying around.
    #[test]
    fn atomic_writes_replace_whole_files_and_leave_no_droppings() {
        let dir = crate::app::paths::test_dir("paths-test");
        let path = dir.join("nested").join("save.txt");
        write_atomic(&path, "first").expect("writes");
        write_atomic(&path, "second").expect("rewrites");
        assert_eq!(std::fs::read_to_string(&path).expect("reads"), "second");
        assert!(!tmp_sibling(&path).exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path.parent().expect("has a parent"))
                .expect("stats")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A write that cannot finish leaves nothing of itself behind: the old
    /// save stands, and no `.tmp` sibling lingers for the next run to find.
    /// The unwritable case here is a temp path that cannot be created
    /// because a *directory* already sits on it.
    #[test]
    fn a_failed_write_leaves_the_old_file_and_no_droppings() {
        let dir = crate::app::paths::test_dir("paths-fail");
        let path = dir.join("save.txt");
        write_atomic(&path, "the good one").expect("writes");
        let tmp = tmp_sibling(&path);
        std::fs::create_dir_all(&tmp).expect("a directory in the temp file's way");
        assert!(write_atomic(&path, "the doomed one").is_err());
        assert_eq!(
            std::fs::read_to_string(&path).expect("reads"),
            "the good one",
            "the save that was already there is untouched"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn absolute_xdg_wins_and_relative_is_ignored() {
        let set = |v: &str| Some(OsString::from(v));
        assert_eq!(
            resolve(set("/somewhere/share"), &[".local", "share"]),
            PathBuf::from("/somewhere/share/pinch-points")
        );
        // The spec: a relative value must be ignored, not joined.
        let fallback = resolve(None, &[".config"]);
        assert_eq!(resolve(set("relative/dir"), &[".config"]), fallback);
        assert_eq!(resolve(set(""), &[".config"]), fallback);
        assert!(fallback.ends_with(".config/pinch-points"));
    }
}
