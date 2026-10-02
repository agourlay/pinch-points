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

/// The subcommands, if the command line names one: `arena`, `cup` and
/// `watch`. `None` for a plain launch, which opens the game.
pub fn subcommand(args: &[String]) -> Option<std::process::ExitCode> {
    let (command, rest) = args.split_first()?;
    let rest = rest.to_vec();
    let outcome = match command.as_str() {
        "arena" => arena::run(rest),
        "cup" => cup::run(rest),
        "watch" => watch(&rest),
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

const USAGE: &str = "\
pinch-points                 open the game
pinch-points arena ...       play bots against the game's AI or each other
pinch-points cup serve ...   run a competition between bots
pinch-points watch FILE      watch a replay in the game's window

`pinch-points <command> --help` says more. Writing a bot: docs/bot-protocol.md
";

/// `pinch-points watch FILE`: the real game, playing a replay at full
/// fidelity, with the seat names and kinds on screen.
fn watch(args: &[String]) -> Result<(), String> {
    let [path] = args else {
        return Err("watch takes one replay file".into());
    };
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let replay = crate::sim::Replay::parse(&text).map_err(|e| format!("{path}: {e}"))?;
    crate::app::watch_file(replay);
    Ok(())
}
