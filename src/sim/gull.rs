use crate::sim::crab::Handedness;
use crate::sim::direction::Direction;
use crate::sim::pose::Pose;

/// Gull behaviour state (spec §3.5). Walking is the default and the whole
/// offensive layer: a walking gull is steerable with signposts. Flight is a
/// brief, occasional hop that exists so no corner can be permanently safe.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GullState {
    Walking,
    /// Hopping `remaining` more tiles in the current direction, ignoring
    /// walls and signposts, eating nothing.
    Flying {
        remaining: u8,
    },
}

/// One gull. Moves on the grid exactly like a crab while walking (walls,
/// signposts, handedness), slower than a common crab.
#[derive(Clone, Copy, Debug)]
pub struct Gull {
    /// Stable identity for the render layer; unique within a match.
    pub id: u32,
    pub tile: u16,
    pub dir: Direction,
    pub progress: u16,
    /// Last tick's position, for the render layer's interpolation only.
    pub prev: Pose,
    pub handed: Handedness,
    pub state: GullState,
    /// Ticks of walking left until the next takeoff.
    pub takeoff_in: u32,
}

impl Gull {
    /// The current position as one value, so a tick can save it into
    /// `prev` in a single assignment.
    pub fn pose(&self) -> Pose {
        Pose {
            tile: self.tile,
            dir: self.dir,
            progress: self.progress,
        }
    }
}

/// Walking speed in subunits per tick (spec §4.2: slower than a common crab).
pub const GULL_WALK_SPEED: u16 = 8;
/// Flying speed in subunits per tick.
pub const GULL_FLY_SPEED: u16 = 16;
/// Gull-eats-crab range: Manhattan distance between sub-tile offsets on the
/// same tile, in subunits (spec §4.3 suggests 48).
pub const EAT_RANGE: u16 = 48;
/// Takeoff timer range in ticks (5–12 s at 30 Hz): keeps gulls walking, and
/// therefore steerable, roughly 85% of the time (spec §3.5).
pub const TAKEOFF_MIN: u32 = 150;
pub const TAKEOFF_MAX: u32 = 360;
/// Flight length range in tiles (spec §3.5: 2–4).
pub const FLIGHT_MIN: u8 = 2;
pub const FLIGHT_MAX: u8 = 4;
