//! Headless game simulation. No engine types allowed in this module tree.

mod board;
mod bot;
mod campaign;
mod crab;
mod direction;
mod gull;
mod hash;
mod level;
mod map_gen;
mod net;
mod pose;
mod replay;
mod rng;
mod solve;

pub use board::{
    Board, CapPolicy, EVENT_TICKS, LURE_TICKS, MAX_PLAYERS, MAX_SIGNPOSTS_PER_PLAYER, PlayerAction,
    PlayerId, SIGNPOST_LIFETIME, SPILL_CAP, SUBUNITS_PER_TILE, SURGE_TICKS, Signpost,
    SignpostHealth, Spawner, TICKS_PER_SECOND, TIER_FLOORS, TideEvent, TileKind, castle_tier,
};
pub use bot::{BotLevel, bot_action};
pub use campaign::{campaign_levels, challenge_levels};
pub use crab::{Crab, CrabKind, Handedness};
pub use direction::Direction;
pub use gull::{Gull, GullState};
pub use level::{Goal, Level, LevelKind, PUZZLE_TICK_LIMIT, PuzzleOutcome};
pub use map_gen::{castle_spots, classic_arena, classic_arena_seeded, generate_arena};
pub use net::{
    DEFAULT_DELAY, HASH_INTERVAL, INPUT_BYTES, InputMsg, Lockstep, decode_action, encode_action,
};
pub use pose::Pose;
pub use replay::Replay;
pub use rng::Pcg32;
pub use solve::{
    DEFAULT_NODE_BUDGET, Effort, Placement, SolveOutcome, solve, solve_with, validate,
};
