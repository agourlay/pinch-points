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
mod orient;
mod playable;
mod pose;
mod replay;
mod rng;
mod solve;

pub use board::{
    ArenaProblem, Board, CRAB_CAP_TILES_PER_CRAB, CapPolicy, EVENT_COOLDOWN, EVENT_TICKS,
    GOLDEN_CALLED_VALUE, GOLDEN_NOTICE, GULL_CAP, Grid, LURE_COOLDOWN, LURE_COOLDOWN_SECONDS,
    LURE_SECONDS, LURE_TICKS, MAX_PLAYERS, MAX_SIGNPOSTS_PER_PLAYER, Mania, PlayerAction, PlayerId,
    Refusal, Rules, SIGNPOST_LIFETIME, SPILL_CAP, SUBUNITS_PER_TILE, SURGE_TICKS, SWEEP_FLIGHT,
    SWEEP_HOLD, SWEEP_MEMORY, SWEEP_STAGGER, Signpost, SignpostHealth, Spawner, Sweeping,
    TICKS_PER_SECOND, TIER_FLOORS, Tempo, TideEvent, TileKind, castle_tier, claw_of, raid_loss,
    seat_of_claw,
};
pub(crate) use board::{bits_to_hex, sides_text, tile_token};
pub use bot::{
    BotLevel, FAIR_LIFT, FAIR_TICKS_PER_TILE, Hand, bot_action, bot_action_with, fair_walk,
    hand_steps,
};
pub use campaign::{
    COOP, VERSUS_MAPS, campaign_levels, coop_levels, coop_progress_key, versus_map,
};
pub use crab::{Crab, CrabKind, Handedness};
pub use direction::Direction;
pub use gull::{GULL_LEAD, Gull, GullState, IncomingGull};
pub use level::{Filing, Goal, Level, LevelKind, PUZZLE_TICK_LIMIT, PuzzleOutcome};
pub use map_gen::{
    BeachStyle, SURPRISE_LANES_MOST, castle_spots, classic_arena, classic_arena_seeded,
    generate_arena, generate_beach, generate_styled, set_versus_rules, surprise_size,
    surprise_style, surprise_styles,
};
pub(crate) use map_gen::{mirror_dir, mirrored_tile};
pub use net::{
    DEFAULT_DELAY, HASH_INTERVAL, InputMsg, Lockstep, MAX_COMMIT_LEAD, MAX_DELAY, RECORD_SPAN,
    RESEND_LEAD, decode_action, delay_for_round_trip, encode_action,
};
pub use orient::{Orientation, deal_pairs};
pub use playable::{Unplayable, seat_walks, unplayable};
pub use pose::Pose;
pub use replay::{NEEDS_NEWER, Replay, SeatKind, seat_name};
pub use rng::Pcg32;
pub use solve::{
    DEFAULT_NODE_BUDGET, Deal, Effort, Placement, SolveOutcome, needs_both, solve, solve_from,
    solve_with, validate, winning_sets,
};
