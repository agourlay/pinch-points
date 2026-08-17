use crate::sim::crab::{Crab, CrabKind, Handedness};
use crate::sim::direction::Direction;
use crate::sim::gull::{
    EAT_RANGE, FLIGHT_MAX, FLIGHT_MIN, GULL_FLY_SPEED, GULL_WALK_SPEED, Gull, GullState,
    TAKEOFF_MAX, TAKEOFF_MIN,
};
use crate::sim::hash::Fnv;
use crate::sim::rng::Pcg32;

/// Castle tier thresholds by banked score (spec §3.4).
pub const TIER_FLOORS: [u32; 4] = [0, 10, 25, 50];
/// Most live crabs a single castle hit can spill back onto the sand. The
/// score still drops a full tier (spec §3.4's legible loss); crabs beyond the
/// cap are lost to the flock rather than flooding the board.
pub const SPILL_CAP: u32 = 8;
/// Molting-crab lure duration: 10 s at 30 Hz (spec §3.2).
pub const LURE_TICKS: u32 = 300;
/// Quiet spell after a lure ends before another may start, same 10 s.
///
/// Balance: the lure pulls *every* loose crab to one castle, and the crabs it
/// delivers include the next molting crab, so an unguarded lure re-arms
/// itself. Non-stacking plus a cooldown caps lure uptime at half a round and
/// stops the first molt from deciding it.
pub const LURE_COOLDOWN: u32 = 300;
/// Versus signposts fade away after this many ticks (10 s, the original's
/// balance valve against stale fortifications). Puzzle-rule boards
/// (CapPolicy::Reject) keep posts forever: a fixed inventory implies
/// permanence.
pub const SIGNPOST_LIFETIME: u32 = 300;
/// The canonical simulation rate. Everything that converts ticks to
/// seconds (round lengths, clocks, speed settings) goes through this.
pub const TICKS_PER_SECOND: u32 = 30;
/// Tide-event durations (manias, tempo shifts): 10 s.
pub const EVENT_TICKS: u32 = 300;
/// The final-scramble threshold: with a round timer set, gull spawning
/// doubles in rate when this many ticks remain (spec §3.6: 30 s).
pub const SURGE_TICKS: u32 = 900;

/// Castle tier (0–3) for a banked-crab score.
pub fn castle_tier(score: u32) -> u8 {
    match score {
        0..=9 => 0,
        10..=24 => 1,
        25..=49 => 2,
        _ => 3,
    }
}

/// A creature's sub-tile offset from its tile centre, in subunits, as signed
/// screen-space (x right, y down) components.
fn sub_offset(dir: Direction, progress: u16) -> (i32, i32) {
    let (dx, dy) = dir.offset();
    (dx * i32::from(progress), dy * i32::from(progress))
}

pub type PlayerId = u8;

/// A player id as a seat index, if it names a real seat: the one spelling
/// of the bounds check every reader of an untrusted seat goes through.
pub fn seat(player: PlayerId) -> Option<usize> {
    let seat = usize::from(player);
    (seat < MAX_PLAYERS).then_some(seat)
}

/// The ring around a castle, nearest first: the four edge-adjacent tiles
/// in fixed order, then the diagonals. The fixed order keeps every ring
/// walk deterministic, spilled crabs and besieging gulls alike.
pub(crate) const CASTLE_RING: [(i32, i32); 8] = [
    (0, -1),
    (1, 0),
    (0, 1),
    (-1, 0),
    (1, -1),
    (1, 1),
    (-1, 1),
    (-1, -1),
];

/// Seats a board can hold. Six is the current cut: four corners and two
/// long-edge castles on a generated arena (see
/// [`castle_spots`](crate::sim::castle_spots)). The handcrafted classic
/// arena is a four-castle beach and stays one.
pub const MAX_PLAYERS: usize = 6;
/// Spec §3.3: placing a fourth signpost removes that player's oldest.
pub const MAX_SIGNPOSTS_PER_PLAYER: usize = 3;
/// Balance: the ambient gull spawner pauses while this many gulls are on
/// the beach. Tide events ignore the cap on purpose.
pub const GULL_CAP: usize = 6;
/// Balance: the ambient crab spawners pause once live crabs reach this
/// fraction of the board's tiles. Past it the beach is a carpet rather than
/// a puzzle. Crab Mania ignores the cap, the way Gull Mania ignores
/// [`GULL_CAP`].
pub const CRAB_CAP_TILES_PER_CRAB: usize = 3;
/// Spec §4.2: one tile is 256 subunits; all movement is integer arithmetic.
pub const SUBUNITS_PER_TILE: u16 = 256;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TileKind {
    Empty,
    /// Impassable. Modelled as a tile no creature may enter rather than as
    /// four walls, so level authoring can't desync rock and wall data.
    Rock,
    Castle(PlayerId),
    Spawner(Spawner),
    /// A pivoting driftwood log: deflects each crossing creature to its
    /// right or left alternately, flipping after every crossing. A
    /// deterministic 50/50 stream splitter with no PRNG draw.
    Turnstile {
        next_right: bool,
    },
    /// Seaweed: crabs slip through, but walking gulls are blocked and
    /// flying gulls cannot land here (they glide one more tile).
    Kelp,
    /// Shallow water: creatures standing in it move at half speed.
    Pool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Spawner {
    /// Direction emitted crabs initially face.
    pub dir: Direction,
    /// A crab spawns every `period` ticks, starting on tick 0.
    pub period: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignpostHealth {
    Full,
    /// One walking-gull crossing away from destruction.
    Worn,
}

#[derive(Clone, Copy, Debug)]
pub struct Signpost {
    pub dir: Direction,
    pub owner: PlayerId,
    pub health: SignpostHealth,
    /// Monotonic placement counter, used to find the owner's oldest signpost
    /// when the cap evicts one.
    seq: u64,
    /// Tick this signpost was placed (or re-pointed); drives expiry under
    /// versus rules and the render-side fade.
    pub placed: u64,
}

/// One player's input for one tick. Coordinates are the cursor's tile. The
/// wire packs this into 2 bytes (spec §7.6); see [`crate::transport`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PlayerAction {
    #[default]
    None,
    Place {
        x: u8,
        y: u8,
        dir: Direction,
    },
    Remove {
        x: u8,
        y: u8,
    },
}

/// What happens when a player places a signpost at their cap.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapPolicy {
    /// Versus rules (spec §3.3): the player's oldest signpost is removed.
    Evict,
    /// Puzzle rules (spec §5.1): the placement fails, fixed inventory.
    Reject,
}

impl CapPolicy {
    /// The level-format token; `from_token` is its inverse.
    pub fn token(self) -> &'static str {
        match self {
            CapPolicy::Evict => "evict",
            CapPolicy::Reject => "reject",
        }
    }

    pub fn from_token(token: &str) -> Option<CapPolicy> {
        match token {
            "evict" => Some(CapPolicy::Evict),
            "reject" => Some(CapPolicy::Reject),
            _ => None,
        }
    }
}

/// The entire game state. No engine types, no floats, no hash maps: every
/// tick is a pure function of prior state plus one action per seat, so the
/// same seed and input list replays bit-identically on any platform.
#[derive(Clone, Debug)]
pub struct Board {
    width: u8,
    height: u8,
    /// Construction seed, kept for serialization (`Level::to_text`). Not
    /// hashed: the live PRNG state, which is hashed, derives from it.
    seed: u64,
    /// Horizontal wall segments, `(height + 1)` rows × `width` columns.
    /// `h_walls[y * width + x]` is the edge *above* tile `(x, y)`.
    h_walls: Vec<bool>,
    /// Vertical wall segments, `height` rows × `(width + 1)` columns.
    /// `v_walls[y * (width + 1) + x]` is the edge *left of* tile `(x, y)`.
    v_walls: Vec<bool>,
    tiles: Vec<TileKind>,
    signposts: Vec<Option<Signpost>>,
    crabs: Vec<Crab>,
    scores: [u32; MAX_PLAYERS],
    rng: Pcg32,
    tick: u64,
    signpost_seq: u64,
    next_crab_id: u32,
    signpost_cap: u8,
    cap_policy: CapPolicy,
    gulls: Vec<Gull>,
    next_gull_id: u32,
    /// Auto-spawn a gull at a PRNG edge tile every this many ticks; 0 = off.
    gull_period: u32,
    /// Round length in ticks (the tide, spec §3.6). None = untimed. When it
    /// reaches zero the sim freezes: scores are locked at the wave.
    round_length: Option<u32>,
    /// Active molting-crab lure: all loose crabs path toward this player's
    /// castle for the remaining ticks (spec §3.2).
    lure: Option<(PlayerId, u32)>,
    /// Ticks until another lure may start; set when one ends.
    lure_cooldown: u32,
    /// Crabs banked since the start, all players combined. With
    /// `next_crab_id` (crabs ever spawned) this distinguishes "every crab is
    /// safe" from "the gulls got some" (spec §5.1 win condition).
    crabs_banked: u32,
    /// Golden crabs banked (challenge goals).
    golden_banked: u32,
    /// Tide events fire only where enabled (versus arenas, the attract
    /// beach), never in puzzles or goal-checked challenges.
    events_enabled: bool,
    /// Active spawn mania: spawners flood crabs or emit gulls instead.
    mania: Option<(Mania, u32)>,
    /// Active tempo shift and the ticks left of it.
    tempo: Option<(Tempo, u32)>,
    /// The most recent tide event and the tick it fired (HUD banner).
    last_event: Option<(TideEvent, u64)>,
    /// Open edges: creatures walking (or flying) off one side re-enter on
    /// the opposite side (spec §3.1 wrap-around, settled by research: the
    /// original wraps).
    wrap: bool,
    /// Sparkling banks noticed during crab movement; the roulette spins
    /// after the movement pass so events may safely mutate the crab list.
    /// Always drained within the same tick (never hashed).
    event_queue: Vec<PlayerId>,
}

impl Board {
    /// An empty all-sand board with walled borders (spec §3.1; wrap-around
    /// edges are a later, flag-gated variant).
    pub fn new(width: u8, height: u8, seed: u64) -> Board {
        assert!(width > 0 && height > 0, "board must be at least 1×1");
        let (w, h) = (width as usize, height as usize);
        let mut board = Board {
            width,
            height,
            seed,
            h_walls: vec![false; (h + 1) * w],
            v_walls: vec![false; h * (w + 1)],
            tiles: vec![TileKind::Empty; w * h],
            signposts: vec![None; w * h],
            crabs: Vec::new(),
            scores: [0; MAX_PLAYERS],
            rng: Pcg32::new(seed, 0x0005_eaba_55ed),
            tick: 0,
            signpost_seq: 0,
            next_crab_id: 0,
            signpost_cap: MAX_SIGNPOSTS_PER_PLAYER as u8,
            cap_policy: CapPolicy::Evict,
            gulls: Vec::new(),
            next_gull_id: 0,
            gull_period: 0,
            round_length: None,
            lure: None,
            lure_cooldown: 0,
            crabs_banked: 0,
            golden_banked: 0,
            events_enabled: false,
            mania: None,
            tempo: None,
            last_event: None,
            wrap: false,
            event_queue: Vec::new(),
        };
        // The border walls are the wrap rule with wrap off; the border
        // indices are stated once, in set_wrap.
        board.set_wrap(false);
        board
    }

    // --- level authoring -------------------------------------------------

    /// Add or remove the wall on the `dir` side of tile `(x, y)`. Walls are
    /// stored per-edge, so the neighbouring tile sees the same wall and the
    /// two tiles cannot disagree (spec §7.3).
    pub fn set_wall(&mut self, x: u8, y: u8, dir: Direction, present: bool) {
        assert!(
            self.in_bounds(i32::from(x), i32::from(y)),
            "wall off the board"
        );
        self.set_edge(x as usize, y as usize, dir, present);
    }

    pub fn set_tile(&mut self, x: u8, y: u8, kind: TileKind) {
        assert!(
            self.in_bounds(i32::from(x), i32::from(y)),
            "tile off the board"
        );
        if let TileKind::Castle(owner) = kind {
            assert!(seat(owner).is_some(), "invalid castle owner");
        }
        if let TileKind::Spawner(s) = kind {
            assert!(s.period > 0, "spawner period must be at least 1 tick");
        }
        let t = self.index(i32::from(x), i32::from(y));
        self.tiles[t as usize] = kind;
    }

    // --- the player's one verb -------------------------------------------

    /// Change the signpost cap and what happens at it. Versus keeps the
    /// default (3, evict-oldest); puzzle mode sets (inventory, reject).
    pub fn set_signpost_rule(&mut self, cap: u8, policy: CapPolicy) {
        self.signpost_cap = cap;
        self.cap_policy = policy;
    }

    /// Auto-spawn a gull every `period` ticks (0 disables). Doubled during
    /// the final-scramble surge of a timed round.
    pub fn set_gull_period(&mut self, period: u32) {
        self.gull_period = period;
    }

    pub fn set_round_length(&mut self, ticks: Option<u32>) {
        self.round_length = ticks;
    }

    /// Open (or close) the board edges. Opening removes the border walls so
    /// creatures walk and fly off one side and re-enter on the opposite one.
    pub fn set_wrap(&mut self, wrap: bool) {
        self.wrap = wrap;
        let (w, h) = (self.width as usize, self.height as usize);
        for x in 0..w {
            self.h_walls[x] = !wrap;
            self.h_walls[h * w + x] = !wrap;
        }
        for y in 0..h {
            self.v_walls[y * (w + 1)] = !wrap;
            self.v_walls[y * (w + 1) + w] = !wrap;
        }
    }

    pub fn wrap(&self) -> bool {
        self.wrap
    }

    /// Enable tide events (the sparkling crab's roulette). Off by default:
    /// puzzles and goal-checked challenges stay predictable.
    pub fn events_enabled(&self) -> bool {
        self.events_enabled
    }

    pub fn set_events_enabled(&mut self, enabled: bool) {
        self.events_enabled = enabled;
    }

    /// The most recent tide event and when it fired.
    pub fn last_event(&self) -> Option<(TideEvent, u64)> {
        self.last_event
    }

    pub fn golden_banked(&self) -> u32 {
        self.golden_banked
    }

    /// Preload a score (editor, sandboxes, tests). Not used by live play:
    /// scores otherwise change only through banking and gull raids. A seat
    /// that does not exist takes no score: the callers that parse untrusted
    /// text already refuse it with a message, and this is the backstop.
    pub fn set_score(&mut self, player: PlayerId, score: u32) {
        if let Some(seat) = seat(player) {
            self.scores[seat] = score;
        }
    }

    // --- simulation ------------------------------------------------------

    /// Advance one fixed 30 Hz step. Order within a tick is fixed and part of
    /// the ruleset: player actions (in the tick's rotating seat order; on a
    /// same-tile conflict, whoever that order reaches first wins), then
    /// spawners, then crab movement, then gull movement, then gulls eat, in
    /// stable creature order throughout. A signpost placed this tick affects
    /// crabs arriving this tick. Once the tide is in (`round_over`), the sim
    /// is frozen and ticks are no-ops: scores locked at the wave (spec §3.6).
    pub fn tick(&mut self, actions: &[PlayerAction; MAX_PLAYERS]) {
        if self.round_over() {
            return;
        }
        for player in self.action_order() {
            self.apply_action(player, actions[player as usize]);
        }
        self.expire_signposts();
        self.run_spawners();
        self.run_gull_spawner();
        self.move_crabs();
        self.move_gulls();
        self.gulls_eat();
        if let Some((_, ticks)) = &mut self.lure {
            *ticks -= 1;
            if *ticks == 0 {
                self.lure = None;
                self.lure_cooldown = LURE_COOLDOWN;
            }
        } else {
            self.lure_cooldown = self.lure_cooldown.saturating_sub(1);
        }
        if let Some((_, ticks)) = &mut self.mania {
            *ticks -= 1;
            if *ticks == 0 {
                self.mania = None;
            }
        }
        if let Some((_, ticks)) = &mut self.tempo {
            *ticks -= 1;
            if *ticks == 0 {
                self.tempo = None;
            }
        }
        self.tick += 1;
    }

    /// The seat order this tick's actions are applied in: one seat leads,
    /// then round the table.
    ///
    /// Balance: two players reaching for the same tile on the same tick
    /// cannot both have it, so the lead rotates rather than always falling to
    /// the lowest seat. It rotates over the seats *in play*, not the width of
    /// the array, or the lead lands on seats that do not exist and the real
    /// ones keep their relative order.
    ///
    /// The lead is a small mix of the tick rather than `tick % seats`,
    /// because the things that act on a schedule act on multiples of four and
    /// a plain modulo would hand every bot decision to one seat. Still a pure
    /// function of the tick, so lockstep peers and replays agree.
    fn action_order(&self) -> [PlayerId; MAX_PLAYERS] {
        let seats = u64::from(self.seats_in_play()).max(1);
        let first = (self.tick ^ (self.tick >> 3)) % seats;
        std::array::from_fn(|i| {
            let i = i as u64;
            if i < seats {
                ((first + i) % seats) as PlayerId
            } else {
                i as PlayerId // absent seats, in any order: they act on nothing
            }
        })
    }

    /// How many seats this board seats: one past the highest castle owner, so
    /// a four-castle beach rotates ties among four however wide the arrays.
    pub fn seats_in_play(&self) -> u8 {
        self.castle_owners().max().map_or(0, |owner| owner + 1)
    }

    /// The tide has come in: the round is finished and the sim is frozen.
    pub fn round_over(&self) -> bool {
        self.round_length
            .is_some_and(|len| self.tick >= u64::from(len))
    }

    /// Ticks left before the wave, if a round timer is set.
    pub fn remaining_ticks(&self) -> Option<u64> {
        self.round_length
            .map(|len| u64::from(len).saturating_sub(self.tick))
    }

    /// The final scramble: the last 30 s of a timed round, when gulls spawn
    /// at double rate.
    pub fn in_surge(&self) -> bool {
        self.remaining_ticks()
            .is_some_and(|left| left <= u64::from(SURGE_TICKS))
    }

    /// Advance one step with no player input.
    pub fn tick_idle(&mut self) {
        self.tick(&[PlayerAction::None; MAX_PLAYERS]);
    }

    fn apply_action(&mut self, player: PlayerId, action: PlayerAction) {
        match action {
            PlayerAction::None => {}
            PlayerAction::Place { x, y, dir } => {
                let _ = self.place_signpost(player, x, y, dir);
            }
            PlayerAction::Remove { x, y } => {
                let _ = self.remove_signpost(player, x, y);
            }
        }
    }

    /// A walker's step this tick: the tempo-adjusted speed, halved (never
    /// to zero) while standing in a tide pool. One statement of the wading
    /// rule, for crabs and gulls both.
    fn walk_step(&self, tile: u16, base: u16) -> u16 {
        debug_assert!(
            usize::from(tile) < self.tiles.len(),
            "walking off the board: tile {tile} of {}",
            self.tiles.len()
        );
        let step = self.tempo_speed(base);
        if self.tiles[tile as usize] == TileKind::Pool {
            return (step / 2).max(1);
        }
        step
    }

    /// Tide-event tempo: doubled or halved speed for every creature.
    fn tempo_speed(&self, base: u16) -> u16 {
        match self.tempo {
            Some((Tempo::Fast, _)) => base * 2,
            Some((Tempo::Slow, _)) => (base / 2).max(1),
            None => base,
        }
    }

    // --- read access (render layer, modes, tests) ------------------------

    pub fn width(&self) -> u8 {
        self.width
    }

    pub fn height(&self) -> u8 {
        self.height
    }

    pub fn ticks(&self) -> u64 {
        self.tick
    }

    pub fn crabs(&self) -> &[Crab] {
        &self.crabs
    }

    pub fn gulls(&self) -> &[Gull] {
        &self.gulls
    }

    pub fn gull_period(&self) -> u32 {
        self.gull_period
    }

    pub fn round_length(&self) -> Option<u32> {
        self.round_length
    }

    /// The seed this board was constructed with.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Remove every crab standing on tile `(x, y)` (editor use).
    pub fn remove_crabs_at(&mut self, x: u8, y: u8) {
        let tile = self.index(i32::from(x), i32::from(y));
        self.crabs.retain(|c| c.tile != tile);
    }

    /// Remove every gull standing on tile `(x, y)` (editor use).
    pub fn remove_gulls_at(&mut self, x: u8, y: u8) {
        let tile = self.index(i32::from(x), i32::from(y));
        self.gulls.retain(|g| g.tile != tile);
    }

    /// Active molting lure, if any: (luring player, ticks left).
    pub fn lure(&self) -> Option<(PlayerId, u32)> {
        self.lure
    }

    /// Crabs banked since the start, all players combined.
    pub fn crabs_banked(&self) -> u32 {
        self.crabs_banked
    }

    /// Crabs ever spawned (initial, spawner-emitted, and castle-spilled).
    pub fn crabs_spawned(&self) -> u32 {
        self.next_crab_id
    }

    pub fn scores(&self) -> &[u32; MAX_PLAYERS] {
        &self.scores
    }

    pub fn tile_at(&self, x: u8, y: u8) -> TileKind {
        assert!(self.in_bounds(i32::from(x), i32::from(y)));
        self.tiles[self.index(i32::from(x), i32::from(y)) as usize]
    }

    /// Every tile with its coordinates, in the board's own row-major order.
    pub fn tiles(&self) -> impl Iterator<Item = (u8, u8, TileKind)> + '_ {
        let width = self.width;
        self.tiles.iter().enumerate().map(move |(index, &kind)| {
            (
                (index % width as usize) as u8,
                (index / width as usize) as u8,
                kind,
            )
        })
    }

    /// Where a seat's castle stands, if it has one.
    pub fn castle_of(&self, player: PlayerId) -> Option<(u8, u8)> {
        self.tiles()
            .find(|&(_, _, kind)| kind == TileKind::Castle(player))
            .map(|(x, y, _)| (x, y))
    }

    /// The highest seat number with a castle on the board: the seat count
    /// a recorded board implies.
    pub fn castle_owners(&self) -> impl Iterator<Item = PlayerId> + '_ {
        self.tiles().filter_map(|(_, _, kind)| match kind {
            TileKind::Castle(owner) => Some(owner),
            TileKind::Empty
            | TileKind::Rock
            | TileKind::Spawner(_)
            | TileKind::Turnstile { .. }
            | TileKind::Kelp
            | TileKind::Pool => None,
        })
    }

    /// How many seats the board has castles for, counted by owner rather
    /// than by castle: a beach seats as many players as there are banks to
    /// run for. What the map dial measures a handmade beach against.
    pub fn castle_seats(&self) -> u8 {
        let mut seen = [false; MAX_PLAYERS];
        for owner in self.castle_owners() {
            if let Some(slot) = seen.get_mut(usize::from(owner)) {
                *slot = true;
            }
        }
        seen.iter().filter(|held| **held).count() as u8
    }

    /// The first signpost owned by `player` in reading order (top-left to
    /// bottom-right), as tile coordinates. Drives the "clear one" input.
    pub fn first_signpost_of(&self, player: PlayerId) -> Option<(u8, u8)> {
        self.signposts
            .iter()
            .position(|post| post.is_some_and(|post| post.owner == player))
            .map(|index| self.coords_u8(index as u16))
    }

    /// Tile index back to coordinates. [`Board::coords`] speaks `i32` for
    /// the movement arithmetic; this is the public-facing form.
    pub fn coords_u8(&self, tile: u16) -> (u8, u8) {
        let (x, y) = self.coords(tile);
        (x as u8, y as u8)
    }

    pub fn wall_at(&self, x: u8, y: u8, dir: Direction) -> bool {
        assert!(self.in_bounds(i32::from(x), i32::from(y)));
        self.edge_blocked(x as usize, y as usize, dir)
    }
}

mod crabs;
mod events;
mod geometry;
mod gulls;
mod hashing;
mod signposts;
mod snapshot;
#[cfg(test)]
mod tests;

pub use events::{Mania, Tempo, TideEvent};
pub(crate) use geometry::Walker;
