//! Pinch Points: the game, or one of its bot commands (`arena`, `cup`,
//! `watch`) when the command line names one.

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = pinch_points::bots::subcommand(&args) {
        return code;
    }
    pinch_points::app::run();
    std::process::ExitCode::SUCCESS
}
