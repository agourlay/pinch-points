//! The Bevy shell: windowing, rendering, input, and the fixed-timestep bridge
//! into the headless simulation. Nothing in `crate::sim` may depend on this.
//!
//! This file is the shell's shared vocabulary: the resources, the states,
//! the messages. When each of them runs is [`schedule`]'s business.

mod achievements;
pub(crate) mod announce;
mod art;
mod audio;
mod awards;
mod binds;
mod board_render;
mod boot;
pub mod campaign;
pub(crate) mod clock;
mod codes;
/// Whether `screen` stands on the shared beach postcard. One list, asked
/// by the run condition and by the backdrop tender, so a new screen
/// decides once where it stands.
pub(crate) fn postcard_screen(screen: Screen) -> bool {
    match screen {
        Screen::Menu
        | Screen::StageSelect
        | Screen::Settings
        | Screen::Controls
        | Screen::MatchSetup
        | Screen::Achievements
        | Screen::Replays
        | Screen::Lobby
        | Screen::Language
        | Screen::Interlude
        | Screen::NewVersion => true,
        Screen::Puzzle | Screen::Versus | Screen::Editor => false,
    }
}

mod conditions;
mod controls;
mod countdown;
mod creatures;
mod cursor;
mod cycle;
mod daily;
mod dev;
mod editor;
mod effects;
mod embedded;
mod gamepad;
mod hint;
mod hud;
pub(crate) mod i18n;
mod keycaps;
mod keymap;
mod language;
pub mod layout;
mod lobby;
mod play_input;
// Public for the integration tests: a launched round's board is built
// here, and the wire tests have to build it the way the game does.
pub mod match_setup;
mod menu_scene;
mod menu_ui;
pub mod net;
mod open;
pub mod palette;
pub(crate) mod paths;
mod pause;
pub mod progress;
pub mod replays;
mod results;
mod schedule;
mod session;
mod settings;
mod side_panels;
mod sim_events;
mod spectators;
mod stage_select;
mod suspend;
mod teams;
mod tournament;
mod typing;
pub mod update;
mod watch;

pub use campaign::{Campaign, CampaignKind, Coop};
pub use daily::Daily;
pub use schedule::run;

/// Open the game on one replay file (`pinch-points watch FILE`).
pub fn watch_file(replay: Replay) {
    schedule::run_with(watch::file(replay));
}

/// Open the game on an arena's games as they are played (`arena --watch`).
pub fn watch_live(feed: std::sync::mpsc::Receiver<crate::bots::game::Feed>) {
    schedule::run_with(watch::live(feed));
}

use crate::app::i18n::fill;
use crate::sim::{
    Board, BotLevel, Level, MAX_PLAYERS, PlayerAction, PuzzleOutcome, Replay, bot_action,
    castle_spots, classic_arena, classic_arena_seeded, generate_arena,
};
use bevy::app::{TaskPoolOptions, TaskPoolPlugin, TaskPoolThreadAssignmentPolicy};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::{InstanceFlags, RenderCreation, WgpuSettings};

/// Where the last finished versus round's replay is written (spec §7.7).
pub fn replay_path() -> std::path::PathBuf {
    replays::library_dir().join("last.txt")
}
/// Where that round's highlight reel lands (see [`crate::highlight`]).
pub fn highlight_path() -> std::path::PathBuf {
    replays::library_dir().join("highlight.gif")
}

/// Where the finished round's highlight reel was written, so the results
/// card can say so. Cleared when a round has no replay to build one from.
#[derive(Resource, Default)]
pub struct Highlight(pub Option<String>);

/// The authoritative simulation, wrapped for Bevy. Systems read it freely;
/// mutation happens in `advance_sim` (ticks) and, during puzzle setup only,
/// in the placement input system.
#[derive(Resource)]
pub struct Sim(pub Board);

/// Player actions accumulated from input since the last fixed tick, in the
/// shape rollback netcode will feed. Taken (and reset) by `advance_sim`
/// each tick.
#[derive(Resource, Default)]
pub struct PendingActions(pub [PlayerAction; MAX_PLAYERS]);

#[derive(Resource, Default)]
pub struct Paused(pub bool);

/// Records the running versus round for the replay file (spec §7.7).
#[derive(Resource, Default)]
pub struct Recorder(pub Option<Replay>);

/// What decides for a seat: the one description of who sits where, which
/// the fixed tick reads to fill each seat's action (`docs/bot-seats.md`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SeatController {
    /// A person at this machine, on the keyboard or a pad, through a cursor.
    #[default]
    Local,
    /// A seat another peer owns: its inputs arrive over the wire, and this
    /// machine never decides for it.
    Remote,
    /// The game's AI at a difficulty. A pure function of the board, so
    /// online every peer computes it and its moves never cross the wire.
    Ai(BotLevel),
    /// A bot connected to this machine over the bot protocol. Unlike the
    /// AI it is owned by the one game it connected to, as a person's seat
    /// is, and its actions travel like a keyboard's.
    Bot,
}

impl SeatController {
    /// The AI's level, when the AI holds the seat.
    pub fn ai(self) -> Option<BotLevel> {
        match self {
            SeatController::Ai(level) => Some(level),
            SeatController::Local | SeatController::Remote | SeatController::Bot => None,
        }
    }
}

/// Who decides for every seat this round.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Controllers(pub [SeatController; MAX_PLAYERS]);

impl Controllers {
    /// The AI's level at `seat`, if the AI holds it.
    pub fn ai(&self, seat: usize) -> Option<BotLevel> {
        self.0.get(seat).copied().and_then(SeatController::ai)
    }

    /// How many seats the AI holds.
    pub fn ai_count(&self) -> usize {
        self.0.iter().filter(|c| c.ai().is_some()).count()
    }

    /// The AI seats and nothing else, the shape a saved round keeps.
    pub fn levels(&self) -> [Option<BotLevel>; MAX_PLAYERS] {
        self.0.map(SeatController::ai)
    }

    /// A table from the AI seats alone: everyone else plays at this machine.
    pub fn from_levels(levels: [Option<BotLevel>; MAX_PLAYERS]) -> Controllers {
        Controllers(levels.map(|level| level.map_or(SeatController::Local, SeatController::Ai)))
    }
}

/// A loaded replay being watched, and the next input index to feed.
#[derive(Resource, Default)]
pub struct Playback(pub Option<(Replay, usize)>);

/// Top-level mode select.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Screen {
    #[default]
    Menu,
    /// Tide Pool (spec §5.1).
    Puzzle,
    /// Turf War (spec §5.3).
    Versus,
    /// Driftwood (spec §5.4).
    Editor,
    /// LAN matchmaking for online Turf War.
    Lobby,
    Settings,
    /// Per-key rebinding, reached from Settings.
    Controls,
    /// Local versus configuration (players, bots, map, ...).
    MatchSetup,
    /// Lifetime stats and trophies.
    Achievements,
    /// The stage list a puzzle campaign is entered through.
    StageSelect,
    /// The kept rounds, and which one to watch.
    Replays,
    /// Between-rounds breather in a series.
    Interlude,
    /// The very first screen of the very first run: which language the
    /// game should speak. Never reached again once a settings file
    /// exists, since the dial in Settings takes over from there.
    Language,
    /// A newer release is out: its notes and one question. Reached from
    /// the menu when the start-up check comes back with one, and left
    /// for the menu either way.
    NewVersion,
}

/// Fixed interface a board must not slide under, in unscaled pixels.
///
/// Three floats in a row, two of them the same unit and one not, is a thing
/// to get the wrong way round: the camera reads `top` and `bottom` to
/// centre the board on the gap between the bars.
#[derive(Clone, Copy)]
pub struct Chrome {
    /// The sidebars, both of them together.
    pub width: f32,
    /// The header bar.
    pub top: f32,
    /// The prompt line, which runs to two rows in the wordier languages,
    /// and the crab legend above it.
    pub bottom: f32,
}

impl Chrome {
    const fn of(width: f32, top: f32, bottom: f32) -> Chrome {
        Chrome { width, top, bottom }
    }
}

impl Screen {
    /// Every screen, so anything that must cover all of them can iterate
    /// rather than be remembered.
    pub const ALL: [Screen; 14] = [
        Screen::Menu,
        Screen::Puzzle,
        Screen::Versus,
        Screen::Editor,
        Screen::Lobby,
        Screen::Settings,
        Screen::Controls,
        Screen::MatchSetup,
        Screen::Achievements,
        Screen::StageSelect,
        Screen::Replays,
        Screen::Interlude,
        Screen::Language,
        Screen::NewVersion,
    ];

    /// The fixed interface this screen puts around a board.
    ///
    /// Top and bottom are separate because they are not equal: the header
    /// is one line and the prompt runs to two in the wordier languages, and
    /// a single height centres the board on the window rather than on the
    /// gap, which put the editor's bottom wall rail under the prompt.
    ///
    /// Lives here rather than in the camera system so a new screen has to
    /// answer the question where it is declared.
    fn chrome(self) -> Chrome {
        match self {
            // The menu is a full-bleed postcard laid out 1:1.
            Screen::Menu => Chrome::of(0.0, 0.0, 0.0),
            // Versus flanks the board with the two score panels.
            Screen::Versus => Chrome::of(2.0 * side_panels::SIDEBAR_W + 60.0, ABOVE_BOARD, 104.0),
            Screen::Puzzle
            | Screen::Editor
            | Screen::Lobby
            | Screen::Settings
            | Screen::Controls
            | Screen::MatchSetup
            | Screen::Achievements
            | Screen::StageSelect
            | Screen::Replays
            | Screen::Interlude
            | Screen::Language
            | Screen::NewVersion => Chrome::of(40.0, ABOVE_BOARD, 104.0),
        }
    }
}

/// What the header keeps clear above the board: the header itself and
/// the air the board's top edge wants under it. Derived, so a taller
/// header pushes the board down rather than sitting on it.
const ABOVE_BOARD: f32 = menu_ui::HEADER_H + 18.0;

/// Puzzle-mode round phases (spec §5.1: place, run, win or lose, retry).
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Phase {
    #[default]
    Setup,
    Running,
    Won,
    Lost,
}

/// Versus round flow: a count in, play until the tide, then results.
///
/// A round is entered in `Countdown`, which is where the phase rests
/// between rounds, and `load_versus` moves it on to `Running` for a round
/// that does not count in. Resting anywhere else let a round's first frame
/// run before the count began: a phase asked for while the screen changes
/// lands a frame later.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VersusPhase {
    /// The beach laid out and holding still while a count runs down, so
    /// the table can read the map and find its castles (see `countdown`).
    #[default]
    Countdown,
    Running,
    Over,
}

/// Dev sandbox (`PINCH_SANDBOX=1`): skips the menu straight into a versus
/// arena with preloaded castle tiers.
#[derive(Resource)]
pub struct Sandbox(pub bool);

/// Rebuild the board from the campaign's current level. With `keep_posts`,
/// signposts standing on the old board are re-placed: the fast retry loop.
#[derive(Message)]
pub struct LoadLevel {
    pub keep_posts: bool,
}

/// A player's placement was rejected (occupied tile, rival post, or spent
/// inventory): drives the denied sound and cursor flash.
#[derive(Message)]
pub struct PlacementDenied {
    pub player: u8,
    /// The inventory said no, not the tile. Worth a line of its own: every
    /// other refusal is answered by aiming somewhere else, and this one by
    /// picking a signpost back up, and with the same flash and knock "you
    /// have none left" reads as "not there".
    pub out_of_signposts: bool,
}

/// The editor wrote a level to disk. A message rather than a direct call so
/// the editor need not know that anyone is keeping score.
#[derive(Message)]
pub struct LevelSaved;

/// A level went out of the editor as a share code, and one came in as one.
/// Their own messages rather than lines in the editor, for the reason
/// [`LevelSaved`] is one: the editor stays ignorant of achievements.
#[derive(Message)]
pub struct CodeShared;

#[derive(Message)]
pub struct CodeTaken;

/// How many players a versus round seats (2-4). Drives castles, cursors,
/// and HUD chips.
#[derive(Resource, Default)]
pub struct Seats(pub u8);

/// What each seat is called this round, resolved once at round load:
/// online, the handshake's agreed table (never the local couch names,
/// which would label rivals with leftovers); offline, the settings names.
/// Empty entries fall back to the localized seat label.
#[derive(Resource, Default)]
pub struct SeatNames(pub [String; MAX_PLAYERS]);

/// What held each seat this round: a person, the game's AI, or a bot. Read
/// wherever a name is drawn, so a bot always wears the robot (see
/// [`crate::app::art::Art::robot`]). Resolved with the names: a replay's
/// own, the table's online, this machine's controllers otherwise.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SeatKinds(pub [crate::sim::SeatKind; MAX_PLAYERS]);

impl SeatKinds {
    /// Whether a bot holds `seat`.
    pub fn bot(&self, seat: u8) -> bool {
        self.0.get(usize::from(seat)) == Some(&crate::sim::SeatKind::Bot)
    }

    /// The kinds a table of controllers makes.
    pub fn of(controllers: &Controllers) -> SeatKinds {
        SeatKinds(controllers.0.map(|c| match c {
            SeatController::Local | SeatController::Remote => crate::sim::SeatKind::Human,
            SeatController::Ai(_) => crate::sim::SeatKind::Ai,
            SeatController::Bot => crate::sim::SeatKind::Bot,
        }))
    }
}

impl SeatNames {
    /// The name to show for `seat`, or the localized "P{n}" fallback.
    pub fn label(&self, tr: &i18n::Tr, seat: u8) -> String {
        name_or_label(&self.0, tr, seat)
    }
}

/// The localized "P{n}" label for a seat, off-by-one included: seats count
/// from 0, players from 1. The screens that talk about a seat with no name
/// to consult (bindings, match setup) say it this way too.
pub fn seat_label(tr: &i18n::Tr, seat: u8) -> String {
    fill(tr.player_label, &[("p", &(seat + 1).to_string())])
}

/// What to call a seat: its name, or the "P{n}" label when it has none.
/// One rule for every table of names, the couch's, a round's and an
/// online session's, so an unnamed seat reads the same on every screen.
pub fn name_or_label(names: &[String], tr: &i18n::Tr, seat: u8) -> String {
    match names.get(usize::from(seat)) {
        Some(name) if !name.is_empty() => name.clone(),
        _ => seat_label(tr, seat),
    }
}

/// A round from a pasted code, waiting for [`Screen::Versus`] to seat it.
/// Taken by `load_versus`, which is the one place that decides what board a
/// round starts from.
#[derive(Resource, Default)]
pub struct Resuming(pub Option<suspend::Suspended>);

/// What the menu has to say about the round you just copied or pasted, or
/// failed to. Shown in the menu's status slot, so a code that will not read
/// says so rather than doing nothing.
#[derive(Resource, Default)]
pub struct RoundNotice(pub String);

#[cfg(test)]
mod chrome_tests {
    use super::*;

    /// The header is drawn over the board's world, so every screen with a
    /// board keeps at least the header's height clear above it, and the
    /// menu, which has no board, reserves nothing.
    #[test]
    fn the_header_never_sits_on_the_board() {
        assert_eq!(Screen::Menu.chrome().top, 0.0);
        for screen in [
            Screen::Versus,
            Screen::Puzzle,
            Screen::Editor,
            Screen::Lobby,
            Screen::Settings,
            Screen::Controls,
            Screen::MatchSetup,
            Screen::Achievements,
            Screen::StageSelect,
            Screen::Replays,
            Screen::Interlude,
            Screen::Language,
            Screen::NewVersion,
        ] {
            assert!(
                screen.chrome().top >= menu_ui::HEADER_H,
                "{screen:?} reserves {} under a header of {}",
                screen.chrome().top,
                menu_ui::HEADER_H
            );
        }
    }
}
