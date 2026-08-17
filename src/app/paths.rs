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
    resolve(std::env::var_os("XDG_DATA_HOME"), &[".local", "share"])
}

/// The game's configuration directory.
pub fn config_dir() -> PathBuf {
    resolve(std::env::var_os("XDG_CONFIG_HOME"), &[".config"])
}

/// A player's own words made safe to use as a file name: the level they
/// named, the winner a replay is filed under.
///
/// One rule in one place because it is a safety rule, not a formatting
/// one. Both callers wrote their own, and the reason either exists is that
/// a name like `Crab/../etc` must not become a path: a level name and a
/// player name are both typed by whoever is at the keyboard.
///
/// `keep` bounds the result so a long name cannot make an absurd path, and
/// `fallback` covers a name with nothing usable left in it, which is a
/// thing a player can type.
pub fn safe_stem(name: &str, keep: usize, fallback: &str) -> String {
    let mut out = String::new();
    for ch in name.chars().take(keep) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
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
/// failed write. Without it the rename can reach the disk while the bytes
/// it published are still in the page cache, and a power loss then leaves
/// exactly the truncated file the whole dance is here to avoid. A failed
/// write takes its half-finished `.tmp` with it rather than leaving it to
/// sit beside the save forever.
pub fn write_atomic(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    ensure_parent(path);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let landed = spill(&tmp, contents.as_ref()).and_then(|()| std::fs::rename(&tmp, path));
    if landed.is_err() {
        // Nothing half-written is left to be found later, whichever step
        // gave out.
        let _ = std::fs::remove_file(&tmp);
    }
    landed
}

/// The bytes onto the disk itself, not merely into the page cache.
fn spill(tmp: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(tmp)?;
    file.write_all(contents)?;
    file.sync_all()
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
        assert_eq!(safe_stem("Café *3*", 40, "level"), "caf-3");
        // Never empty, however little is left of it.
        assert_eq!(safe_stem("***", 40, "level"), "level");
        assert_eq!(safe_stem("", 40, "level"), "level");
        // Long names are cut rather than making a silly path.
        assert!(safe_stem(&"x".repeat(200), 40, "level").len() <= 40);
        assert!(safe_stem(&"x".repeat(200), 12, "round").len() <= 12);
    }

    use super::*;

    /// The whole point of the rename dance: a rewrite replaces the file in
    /// one step, the parent chain appears on demand (privately, on unix),
    /// and no `.tmp` sibling is left lying around.
    #[test]
    fn atomic_writes_replace_whole_files_and_leave_no_droppings() {
        let dir = std::env::temp_dir().join(format!("pinch-paths-test-{}", std::process::id()));
        let path = dir.join("nested").join("save.txt");
        write_atomic(&path, "first").expect("writes");
        write_atomic(&path, "second").expect("rewrites");
        assert_eq!(std::fs::read_to_string(&path).expect("reads"), "second");
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        assert!(!Path::new(&tmp).exists());
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
        let dir = std::env::temp_dir().join(format!("pinch-paths-fail-{}", std::process::id()));
        let path = dir.join("save.txt");
        write_atomic(&path, "the good one").expect("writes");
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(&tmp);
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
