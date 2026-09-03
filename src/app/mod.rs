//! The Bevy shell: windowing, rendering, input, and the fixed-timestep bridge
//! into the headless simulation. Nothing in `crate::sim` may depend on this.
//!
//! This file is the shell's shared vocabulary: the resources, the states,
//! the messages. When each of them runs is [`schedule`]'s business.

mod achievements;
pub(crate) mod announce;
mod art;
mod audio;
mod binds;
mod board_render;
mod boot;
pub mod campaign;
mod clock;
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
mod paths;
mod pause;
pub mod progress;
pub mod replays;
mod results;
mod schedule;
mod session;
mod settings;
mod side_panels;
mod sim_events;
mod stage_select;
mod suspend;
mod teams;
mod tournament;
mod typing;
pub mod update;

pub use campaign::{Campaign, CampaignKind};
pub use daily::Daily;
pub use schedule::run;

use crate::app::i18n::fill;
use crate::sim::{
    Board, BotLevel, Level, MAX_PLAYERS, PlayerAction, PuzzleOutcome, Replay, bot_action,
    castle_spots, classic_arena, classic_arena_seeded, generate_arena,
};
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

/// Which seats are bot-driven this round, and at what difficulty.
#[derive(Resource, Default)]
pub struct Bots(pub [Option<BotLevel>; MAX_PLAYERS]);

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
/// Three floats in a row, two of them the same unit and one of them not,
/// is a thing to get the wrong way round: the camera reads `top` and
/// `bottom` to centre the board on the gap between the bars, and swapping
/// them moves every board a few pixels the wrong way on every screen.
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
    /// is one line and the prompt runs to two in the wordier languages. A
    /// single height would fit the board and then centre it on the window
    /// rather than on the gap, which is how the editor's bottom wall rail
    /// ended up under the prompt.
    ///
    /// Lives here rather than in the camera system because it is a fact
    /// about the screen, and because a new screen should have to answer
    /// this question at the point it is declared.
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

/// Versus round flow: play until the tide, then results.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VersusPhase {
    #[default]
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
    /// other refusal is answered by aiming somewhere else, and this one is
    /// answered by picking a signpost back up. They looked identical - the
    /// same flash, the same knock - so "you have none left" read as "not
    /// there".
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

impl SeatNames {
    /// The name to show for `seat`, or the localized "P{n}" fallback.
    pub fn label(&self, tr: &i18n::Tr, seat: u8) -> String {
        match self.0.get(usize::from(seat)) {
            Some(name) if !name.is_empty() => name.clone(),
            _ => seat_label(tr, seat),
        }
    }
}

/// The localized "P{n}" label for a seat, off-by-one included: seats count
/// from 0, players from 1. The screens that talk about a seat with no name
/// to consult (bindings, match setup) say it this way too.
pub fn seat_label(tr: &i18n::Tr, seat: u8) -> String {
    fill(tr.player_label, &[("p", &(seat + 1).to_string())])
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
