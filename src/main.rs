//! Pinch Points: the game, or one of its bot commands (`arena`, `cup`,
//! `watch`) when the command line names one.

fn main() -> std::process::ExitCode {
    // As the OS gave them: `args()` panics on one that is not UTF-8, and
    // a replay's path need not be.
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if let Some(code) = pinch_points::bots::subcommand(&args) {
        return code;
    }
    pinch_points::app::run();
    std::process::ExitCode::SUCCESS
}
