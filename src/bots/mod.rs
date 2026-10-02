//! Bot seats: bots people write, in any language, playing over TCP.
//!
//! The one rule is that the game never runs a bot. A bot is a network
//! client that connects to the game, registers, and answers each tick with
//! an action; the game only listens, sends the board and reads moves back.
//! The design is `docs/bot-seats.md` and the contract bot authors write
//! against is `docs/bot-protocol.md`.
//!
//! Nothing here starts Bevy: the arena and a cup are the sim, the seat
//! controllers and the protocol, and run as fast as the bots answer. The
//! one exception is `arena --watch`, which hands the game to a window.

pub mod arena;
pub mod cli;
pub mod connstr;
pub mod cup;
pub mod cursor;
pub mod diff;
pub mod draw;
pub mod game;
pub mod listener;
pub mod lookahead;
pub mod observe;
pub mod protocol;
pub mod seat;
#[cfg(test)]
mod tests;

use std::ffi::OsString;

/// The subcommands, if the command line names one: `arena`, `cup` and
/// `watch`. `None` for a plain launch, which opens the game.
///
/// The arguments are the OS's own: a file name need not be UTF-8, so
/// `watch` takes its path as given, and the flags of `arena` and `cup`,
/// which are text, are refused with a reason when they are not.
pub fn subcommand(args: &[OsString]) -> Option<std::process::ExitCode> {
    let (command, rest) = args.split_first()?;
    let command = command.to_str()?;
    let outcome = match command {
        "arena" => text(rest).and_then(arena::run),
        "cup" => text(rest).and_then(cup::run),
        "watch" => watch(rest),
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        _ => return None,
    };
    Some(match outcome {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pinch-points {command}: {e}");
            std::process::ExitCode::FAILURE
        }
    })
}

/// Flags as text, or which one is not.
fn text(args: &[OsString]) -> Result<Vec<String>, String> {
    args.iter()
        .map(|arg| {
            arg.to_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{} is not UTF-8 text", arg.to_string_lossy()))
        })
        .collect()
}

const USAGE: &str = "\
pinch-points                 open the game
pinch-points arena ...       play bots against the game's AI or each other
pinch-points cup serve ...   run a competition between bots
pinch-points watch FILE      watch a replay in the game's window

`pinch-points <command> --help` says more. Writing a bot: docs/bot-protocol.md
";

/// `pinch-points watch FILE`: the real game, playing a replay at full
/// fidelity, with the seat names and kinds on screen.
fn watch(args: &[OsString]) -> Result<(), String> {
    let [path] = args else {
        return Err("watch takes one replay file".into());
    };
    let replay = read_replay(std::path::Path::new(path))?;
    crate::app::watch_file(replay);
    Ok(())
}

/// The replay at `path`, or why not.
fn read_replay(path: &std::path::Path) -> Result<crate::sim::Replay, String> {
    let shown = path.display();
    let text = std::fs::read_to_string(path).map_err(|e| format!("{shown}: {e}"))?;
    crate::sim::Replay::parse(&text).map_err(|e| format!("{shown}: {e}"))
}

#[cfg(all(test, unix))]
mod args_tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;

    fn not_utf8(prefix: &str) -> OsString {
        let mut bytes = prefix.as_bytes().to_vec();
        bytes.extend_from_slice(b"\xff\xfe");
        OsString::from_vec(bytes)
    }

    /// An argument that is not UTF-8 is no panic: a plain launch with one
    /// opens the game, and a flag that is not text is refused by name.
    #[test]
    fn arguments_that_are_not_utf8_are_taken_as_given() {
        assert!(subcommand(&[not_utf8("x")]).is_none());
        assert!(text(&[OsString::from("--games"), not_utf8("3")]).is_err());
        assert_eq!(
            text(&[OsString::from("--games"), OsString::from("3")]),
            Ok(vec!["--games".to_string(), "3".to_string()])
        );
    }

    /// A replay whose file name is not UTF-8 is read all the same.
    #[test]
    fn a_replay_with_a_name_that_is_not_utf8_is_read() {
        let level = crate::sim::Level::from_board("Arena", 3, crate::sim::classic_arena(false, 2));
        let replay = crate::sim::Replay::new(level);
        let mut name = std::env::temp_dir().into_os_string();
        name.push("/");
        name.push(not_utf8(&format!("pinch-watch-{}-", std::process::id())));
        let path = std::path::PathBuf::from(name);
        std::fs::write(&path, replay.to_text()).expect("write");
        let read = read_replay(&path);
        let _ = std::fs::remove_file(&path);
        read.expect("a replay");
    }
}
