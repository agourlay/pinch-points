use crate::sim::crab::{Crab, CrabKind, Handedness};
use crate::sim::direction::Direction;
use crate::sim::gull::{
    EAT_RANGE, FLIGHT_MAX, FLIGHT_MIN, GULL_FLY_SPEED, GULL_LEAD, GULL_WALK_SPEED, Gull, GullState,
    IncomingGull, TAKEOFF_MAX, TAKEOFF_MIN,
};
use crate::sim::hash::Fnv;
use crate::sim::pose::Pose;
use crate::sim::rng::Pcg32;

/// Castle tier thresholds by banked score (spec §3.4).
pub const TIER_FLOORS: [u32; 4] = [0, 10, 25, 50];
/// Most live crabs a single castle hit can spill back onto the sand. The
/// score still drops a full tier (spec §3.4's legible loss); crabs beyond the
/// cap are lost to the flock rather than flooding the board.
pub const SPILL_CAP: u32 = 8;
/// Molting-crab lure duration: 5 s at 30 Hz (spec §3.2).
///
/// Balance: at 10 s the banker took 19-34 points from one lure, 15-33% of
/// everything banked came in during the banker's own lure, and a fifth to
/// nearly a third of the lead changes that decided a round were lures. At
/// 5 s (with the [`LURE_COOLDOWN`] below) that is 9-16% of the points and
/// 8-17% of the deciding lead changes over 1500 bot rounds a table size,
/// and the owner still wins well above a fair share: 39% at four seats.
pub const LURE_TICKS: u32 = 150;
/// [`LURE_TICKS`] in whole seconds, rounded up: the one figure every line
/// that tells a player how long a lure runs reads.
pub const LURE_SECONDS: u32 = LURE_TICKS.div_ceil(TICKS_PER_SECOND);
/// Quiet spell after a lure ends before another may start: 30 s.
///
/// Balance: the lure pulls *every* loose crab to one castle, and the crabs it
/// delivers include the next molting crab, so an unguarded lure re-arms
/// itself. Non-stacking plus a cooldown stops the first molt from deciding
/// the round.
///
/// At 10 s, matching the lure, a measured six-player round ran lures 22.9,
/// 25.8, 27.2 and 23.3 s apart with one castle holding 50 of the 60 lured
/// seconds. With the 5 s lure, 30 s rather than 20 s took the lure share of
/// the classic arena's points from 18% to 16%, and of a three-seat table's
/// from 17% to 15%, for about one lure in eight fewer.
pub const LURE_COOLDOWN: u32 = 900;
/// [`LURE_COOLDOWN`] in whole seconds, rounded up, for the lines that tell
/// a player how long the quiet spell lasts.
pub const LURE_COOLDOWN_SECONDS: u32 = LURE_COOLDOWN.div_ceil(TICKS_PER_SECOND);
/// Ticks after any tide event before the roulette may spin again.
///
/// The roulette fires when a Sparkling crab banks, and Sparkling is 4% of
/// what the *spawners* produce, so the events that raise the crab supply
/// raise their own rate: Crab Mania floods the spawners and Speed Up banks
/// everything faster, and both spin the wheel that rolled them. Unvalved,
/// the seven kept rounds ran from nought events in three minutes to twelve,
/// the worst putting six inside nineteen seconds.
///
/// One [`EVENT_TICKS`] is exactly "no second event until the first has
/// finished", the same valve [`LURE_COOLDOWN`] puts on the other
/// self-feeding mechanic. Re-run through it (`cargo run --example
/// round_events`), no event in any kept round lands inside another's
/// window.
pub const EVENT_COOLDOWN: u32 = EVENT_TICKS;
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
///
/// Measured from the end, so a round no longer than this is in the surge
/// from its first tick. Every timed level in the game is one: the timed
/// campaign puzzles run less than 900 ticks, so for them the "final"
/// scramble is the whole round and its doubled rate is their baseline,
/// tuned as such. Only the versus round
/// lengths are long enough to have a calm stretch before it.
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
/// What a called golden crab banks for (see [`Rules::golden_every`]).
pub const GOLDEN_CALLED_VALUE: u32 = 30;
/// Ticks a called golden crab is announced before it comes out.
pub const GOLDEN_NOTICE: u32 = 5 * TICKS_PER_SECOND;

/// A Crab Monopoly sweep under [`Rules::sweep_flight`], in ticks: how long
/// the picked crabs wait where they stand, how far apart they set off, and
/// how long each is in the air before it lands and banks.
///
/// The wait is the event's banner, which covers the middle of the board
/// for its first two seconds: the crabs leave as it starts to fade.
pub const SWEEP_HOLD: u64 = 66;
pub const SWEEP_STAGGER: u64 = 4;
pub const SWEEP_FLIGHT: u64 = 30;
/// Ticks between called golden crabs in a versus round: three minutes,
/// so one a standard round and two in a long one.
pub const VERSUS_GOLDEN_EVERY: u32 = 3 * 60 * TICKS_PER_SECOND;
/// Spec §3.3: placing a fourth signpost removes that player's oldest.
pub const MAX_SIGNPOSTS_PER_PLAYER: usize = 3;
/// Balance: the ambient gull spawner pauses while this many gulls are on
/// the beach, unless the board's rules say otherwise ([`Rules::gull_cap`]).
/// Gull Mania floods past it on purpose, up to three times the cap.
pub const GULL_CAP: usize = 6;
/// Balance: the ambient crab spawners pause once live crabs reach this
/// fraction of the board's tiles. Past it the beach is a carpet rather than
/// a puzzle. Crab Mania floods past the cap up to twice it, the way Gull
/// Mania floods to three times [`GULL_CAP`].
pub const CRAB_CAP_TILES_PER_CRAB: usize = 3;
/// The most crabs and gulls a board read from text may carry, counting
/// crabs in the air: the level format and the snapshot both refuse more.
/// A beach the editor builds is at most 20x13, and a round saved mid-play
/// carries what was on it then, both far inside this. Unbounded, a pasted
/// code could carry a hundred thousand crab lines, and reading one through
/// to its result (as a pasted round is, to file it) froze the game.
pub const MAX_CREATURES: usize = 2048;
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
/// wire packs this into 3 bytes, 8 with the seat and frame of the
/// [`crate::sim::InputMsg`] around it; see [`crate::sim::encode_action`].
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
    /// The spectators called a tide event, relayed by the host.
    ///
    /// An action rather than a message of its own, and that is the whole
    /// point: a peer cannot simulate a frame without every seat's input
    /// for it, so an event carried this way arrives with the frame it
    /// belongs to or the frame does not run. A side channel would have
    /// been echoed and hoped for, and a peer that missed every echo would
    /// have played a different round without ever knowing.
    CallEvent(TideEvent),
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

/// The rules a board is built with: the signpost cap and what happens at
/// it, the ambient gull rate, the tide, and whether gulls may raid. Set
/// by the level or the lobby before the first tick and, apart from the
/// surge doubling of the gull rate, untouched by play. Hashed and saved
/// like the rest of the board: peers on different rules would desync.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rules {
    pub signpost_cap: u8,
    pub cap_policy: CapPolicy,
    /// Auto-spawn a gull at a PRNG edge tile every this many ticks; 0 = off.
    /// Halved (the rate doubled) during the final-scramble surge of a
    /// timed round, which on a round no longer than [`SURGE_TICKS`] is the
    /// whole round: there the period a level states is never the one it
    /// plays at.
    pub gull_period: u32,
    /// How many gulls the ambient spawner allows on the beach at once.
    /// [`GULL_CAP`] unless a versus beach bigger than the classic one asked
    /// for more (`set_versus_rules`), or a level file or a snapshot names
    /// one of its own (`gull_cap:`).
    pub gull_cap: u8,
    /// Whether the flock turns over: at the spawner's turn with the flock
    /// full, the oldest gull clear of the castles flies off and a fresh
    /// one comes in, and the final rush raises the cap by half.
    ///
    /// Without it a gull leaves only by raiding, and a full flock that has
    /// settled into loops away from the castles holds the beach for the
    /// rest of the round: the spawner is shut out, and the final rush's
    /// doubled rate doubles nothing. In bot rounds on the classic size the
    /// flock sat at its cap for four fifths of the final minute, and one
    /// round in five saw no gull leave in its last sixty seconds. On for
    /// versus only: a puzzle's flock is part of what its solution was
    /// proven against, and a file that does not say so keeps the old rule.
    pub gull_turnover: bool,
    /// Ticks between called golden crabs; 0 leaves the golden crab in the
    /// spawn mix at its full worth, as every board before versus took it.
    ///
    /// With it set, the mix never rolls a golden crab. Instead, once every
    /// this many ticks the board calls one ([`Board::golden_call`]): a tick
    /// a third to two thirds of the way into the stretch and a spawner
    /// hole, both drawn when the stretch begins, so the call is on the
    /// board for everyone to see before the crab is, and the golden crab
    /// is worth [`GOLDEN_CALLED_VALUE`]. Four silent fifty-point crabs a
    /// round, each worth a third of a good score, decided rounds for
    /// whoever had an arrow in the right place; one a round, called, is a
    /// race the whole table can see coming.
    pub golden_every: u32,
    /// Whether the event wheel locks only while a timed event runs.
    ///
    /// Every event used to lock the wheel for [`EVENT_COOLDOWN`], the ones
    /// over in a tick (Crab Monopoly, Fresh Sand, Castle Swap, Gull Attack)
    /// as much as the ones that last. The lock is there so a Crab Mania's
    /// flood of crabs cannot spin the wheel again while it runs; after an
    /// event that is already over it only swallowed the next player's
    /// sparkling crab, with nothing on the beach to say why. On for versus
    /// only, and a file that does not say so keeps the old lock.
    pub timed_lock: bool,
    /// Whether Crab Monopoly carries its crabs home rather than banking
    /// them on the spot.
    ///
    /// Banked on the spot, half the beach vanished in one tick and the
    /// scores jumped before anything could be seen to happen; whatever the
    /// screen drew afterwards was decoration on a done deal. With this the
    /// picked crabs leave the beach (no gull can take them, no arrow turns
    /// them) and wait [`SWEEP_HOLD`], then fly to the banker's castle one
    /// after another, and each banks the tick it lands ([`Board::sweeping`]).
    /// On for versus only, and a file that does not say so keeps the
    /// instant sweep.
    pub sweep_flight: bool,
    /// Whether the flock's turnover keeps the flock whole: the spawner
    /// rolls the newcomer's edge tile first and sends a gull off only for
    /// one that can land.
    ///
    /// The first turnover sent the old gull off before the roll, and a
    /// roll onto rock or kelp spawns nothing, so a beach with a rocky rim
    /// lost a gull each time and the full flock it was meant to keep
    /// whole thinned out instead. On for versus only, and a file that
    /// does not say so keeps the old order.
    pub whole_flock: bool,
    /// Whether a crab Crab Monopoly carries home banks what it was worth
    /// when the sweep fired, rather than what it is worth when it lands.
    ///
    /// The instant sweep banked every crab on the tick it fired, so a
    /// Right Claws starting later could not touch it. In flight, a
    /// timed lock leaves the wheel free to spin Right Claws while the
    /// crabs are still in the air, and each left-clawed one then cost its
    /// banker the points it was meant to add. The claw call in force at
    /// the sweep is written on each crab ([`Sweeping::claw_call`]). On for
    /// versus only, and a file that does not say so keeps the old reading.
    pub sweep_keeps_worth: bool,
    /// Whether the spawner holes that open on the same tick take turns
    /// at the beach's crab cap.
    ///
    /// Tried in reading order, the top-left hole always spawned and, once
    /// the beach was at its cap, the ones read last went without. Six
    /// holes on a beach at the cap all round gave the top-left one 14%
    /// more crabs than the bottom-right one, and its castle the extra
    /// points. With this on, a tick that leaves a hole out tries them
    /// from one drawn at random. On for versus only: a level file that
    /// does not say so (`fair_spawns:`) keeps reading order.
    pub fair_spawns: bool,
    /// Round length in ticks (the tide, spec §3.6). None = untimed. When it
    /// reaches zero the sim freezes: scores are locked at the wave.
    pub round_length: Option<u32>,
    /// Whether a gull reaching a castle carries off a third of its bank
    /// (spec §3.4). True on a versus beach, where robbing a rival is the
    /// point.
    ///
    /// False in a puzzle, where a raid has nothing to take and one bad
    /// side effect: the spill puts banked crabs back on the sand, each
    /// counting as newly spawned, which is the denominator of "Saved a/b".
    /// Nine shipped campaign levels grew their own target that way while
    /// playing their authored solution.
    ///
    /// The gull still walks in and still leaves the beach with the flock.
    /// That departure is load-bearing: a castle is the one tile that takes
    /// a gull off the sand, and shutting the door instead left Two Giants
    /// with no two-post solution at all.
    pub castle_raids: bool,
    /// Claw-keyed arrows, the co-op stage's rule: `Some([left, right])`
    /// gives P1 (seat 0) `left` arrows that turn only left-clawed crabs and
    /// P2 (seat 1) `right` arrows that turn only right-clawed ones (see
    /// [`claw_of`]). Every other seat has none, and a gull turns on any
    /// arrow, whoever placed it.
    ///
    /// Each half of the beach's crowd then answers to one player, and a
    /// tile still holds one arrow: the two plans meet on the sand, which is
    /// what makes a co-op stage need both of them rather than a second
    /// pair of hands on a solo puzzle. `None` everywhere else, where every
    /// arrow turns every crab and [`Rules::signpost_cap`] is each seat's.
    pub claw_hands: Option<[u8; 2]>,
    /// Who plays for whom: each seat's team, when the round is scored in
    /// teams, and `None` when every seat plays for itself.
    ///
    /// Read by the rules that pick on rivals, so a seat never picks on its
    /// own partner: Gull Attack lands no gull beside a castle of the
    /// spinner's team, and the AI steers no gull at one. Scored by team,
    /// one of the three raids a 2v2 spin set up used to hit the spinner's
    /// own total. On the board rather than read off a setting, so every
    /// peer of an online round plays by the same split: the host's terms
    /// put it here on every peer alike (`match_setup::board_from`), and a
    /// snapshot and a recording carry it.
    pub teams: Option<[u8; MAX_PLAYERS]>,
}

/// [`Rules::teams`] as the `sides:` line of a snapshot or a level file
/// writes it: each seat's team, in seat order.
pub fn sides_text(teams: &[u8; MAX_PLAYERS]) -> String {
    teams.map(|team| team.to_string()).join(" ")
}

/// A `sides:` line's value read back: a team for every seat, each one a
/// seat number at most, as a team is numbered by its seats.
pub fn parse_sides(value: &str) -> Result<[u8; MAX_PLAYERS], String> {
    let mut words = value.split_whitespace();
    let mut teams = [0u8; MAX_PLAYERS];
    for (seat, slot) in teams.iter_mut().enumerate() {
        let word = words
            .next()
            .ok_or_else(|| format!("sides: no team for seat {seat}"))?;
        *slot = word
            .parse::<u8>()
            .ok()
            .filter(|&team| usize::from(team) < MAX_PLAYERS)
            .ok_or_else(|| format!("sides: bad team {word:?}"))?;
    }
    match words.next() {
        Some(extra) => Err(format!("sides: more teams than seats at {extra:?}")),
        None => Ok(teams),
    }
}

impl Rules {
    /// The team `seat` plays for: its own seat number when every seat is
    /// for itself ([`Rules::teams`]).
    pub fn team_of(&self, seat: PlayerId) -> u8 {
        match self.teams {
            Some(teams) => teams.get(usize::from(seat)).copied().unwrap_or(seat),
            None => seat,
        }
    }

    /// Whether two seats play for the same side: a seat and itself always,
    /// two seats only when they share a team.
    pub fn allies(&self, a: PlayerId, b: PlayerId) -> bool {
        a == b || self.team_of(a) == self.team_of(b)
    }
}

/// The claw a seat's arrows turn under claw-keyed arrows
/// ([`Rules::claw_hands`]): P1's the left, P2's the right, the left player
/// owning the left claw. `None` for a seat that places none there.
pub fn claw_of(player: PlayerId) -> Option<Handedness> {
    match player {
        0 => Some(Handedness::Left),
        1 => Some(Handedness::Right),
        _ => None,
    }
}

/// The seat whose arrows turn crabs of `claw` under claw-keyed arrows:
/// [`claw_of`] read backwards.
pub fn seat_of_claw(claw: Handedness) -> PlayerId {
    match claw {
        Handedness::Left => 0,
        Handedness::Right => 1,
    }
}

/// The rules of a board nobody has set any on: the versus signpost rule,
/// no ambient gulls, an untimed round, gulls free to raid, and every rule
/// that came in later off, which is what a file that does not name it
/// keeps.
impl Default for Rules {
    fn default() -> Rules {
        Rules {
            signpost_cap: MAX_SIGNPOSTS_PER_PLAYER as u8,
            cap_policy: CapPolicy::Evict,
            gull_period: 0,
            gull_cap: GULL_CAP as u8,
            gull_turnover: false,
            golden_every: 0,
            timed_lock: false,
            sweep_flight: false,
            whole_flock: false,
            sweep_keeps_worth: false,
            fair_spawns: false,
            round_length: None,
            castle_raids: true,
            claw_hands: None,
            teams: None,
        }
    }
}

/// Why no table can sit at a beach ([`Board::arena_problem`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArenaProblem {
    /// The castles skip this seat, counted from 0, while a later one has a
    /// castle. Seat `n` plays castle `n`, so the skipped seat would have
    /// nothing to bank in and the later castle would bank for nobody.
    SkippedSeat(PlayerId),
    /// Castles for only this many seats, under the two a table needs.
    TooFew(u8),
}

/// The entire game state. No engine types, no floats, no hash maps: every
/// tick is a pure function of prior state plus one action per seat, so the
/// same seed and input list replays bit-identically on any platform.
#[derive(Clone, Debug)]
pub struct Board {
    /// The beach itself: size, walls, tiles.
    grid: Grid,
    /// Construction seed, kept for serialization (`Level::to_text`). Not
    /// hashed: the live PRNG state, which is hashed, derives from it.
    seed: u64,
    signposts: Vec<Option<Signpost>>,
    /// The rules this board plays by, set at construction.
    rules: Rules,
    crabs: Vec<Crab>,
    scores: [u32; MAX_PLAYERS],
    rng: Pcg32,
    tick: u64,
    signpost_seq: u64,
    next_crab_id: u32,
    gulls: Vec<Gull>,
    next_gull_id: u32,
    /// Gulls the edge spawner has sent for and that have not landed yet,
    /// in the order they land.
    incoming: Vec<IncomingGull>,
    /// Active molting-crab lure: all loose crabs path toward this player's
    /// castle for the remaining ticks (spec §3.2).
    lure: Option<(PlayerId, u32)>,
    /// Ticks until another lure may start; set when one ends.
    lure_cooldown: u32,
    /// Crabs banked since the start, all players combined. With
    /// `next_crab_id` (crabs ever spawned) this distinguishes "every crab is
    /// safe" from "the gulls got some" (spec §5.1 win condition).
    crabs_banked: u32,
    /// Golden crabs banked (the `golden` goal).
    golden_banked: u32,
    /// The golden crab called and not yet out, as the tick and the
    /// spawner tile (see [`Rules::golden_every`]).
    golden_call: Option<(u64, u16)>,
    /// Open edges: creatures walking (or flying) off one side re-enter on
    /// the opposite side (spec §3.1 wrap-around, settled by research: the
    /// original wraps).
    wrap: bool,
    /// The tide roulette and what it has set running.
    pub(crate) tide: events::Tide,
    /// Crab Monopoly's crabs, in the air and just landed.
    sweeps: Sweeps,
    /// The molts banked lately that started no lure, and why.
    ///
    /// The render layer reads them between frames, and a frame can span
    /// several ticks: a lure that ends inside one leaves its end state
    /// saying "quiet spell" for a molt banked while it still ran. So the
    /// reason is written down on the bank tick, kept for as long as the
    /// swept-home record ([`SWEEP_MEMORY`]) and for the same reason. Never
    /// read back by the sim, so it stays out of the fingerprint.
    refused_lures: Vec<RefusedLure>,
    /// The crabs that walked into a castle lately: which seat banked each,
    /// and on which tile.
    ///
    /// The render layer's answer to "banked or eaten?" for a crab gone
    /// since the last frame. Read off the map instead (a crab on or facing
    /// a castle), a gull taking a crab one tile short of a castle was a
    /// bank, sound, points and all. Kept as long as the swept-home record
    /// ([`SWEEP_MEMORY`]), since a frame can span several ticks; never
    /// read back by the sim, so it stays out of the fingerprint. The
    /// tide's banks are in the swept-home record ([`Board::swept_home`]).
    walked_home: Vec<WalkedHome>,
    /// Where each seat last put a signpost down, as the tile and the tick:
    /// the anchor an AI seat's hand walks from to its next placement (see
    /// [`Board::last_placement_of`]). Kept apart from the posts, because a
    /// post can go (Fresh Sand, a gull wearing it out, its owner picking it
    /// up) while the hand that put it there stays where it was.
    hands: [Option<(u16, u64)>; MAX_PLAYERS],
    /// Who steered each gull, and what the steered gulls took: see
    /// [`steering::Steering`].
    steering: steering::Steering,
}

/// A molting crab banked while no lure could start ([`Board::lure_ready`]
/// false), for the render layer to find.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct RefusedLure {
    crab: u32,
    /// Whether a lure was running, rather than the quiet spell after one.
    running: bool,
    /// The tick it banked on, for pruning.
    at: u64,
}

/// A crab that walked into a castle, for the render layer to find (see
/// [`Board::walked_home`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct WalkedHome {
    crab: u32,
    /// The seat the castle belongs to.
    owner: PlayerId,
    /// The castle's tile.
    tile: u16,
    /// What it paid, before the floor at nothing (see
    /// [`Board::credit_bank`]).
    points: i32,
    /// The tick it banked on, for pruning.
    at: u64,
}

/// Replace `dst`'s contents with `src`'s, keeping `dst`'s buffer. The one
/// move [`Board::copy_from`] is made of.
fn refill<T: Copy>(dst: &mut Vec<T>, src: &[T]) {
    dst.clear();
    dst.extend_from_slice(src);
}

impl Board {
    /// An empty all-sand board with walled borders (spec §3.1; wrap-around
    /// edges are a later, flag-gated variant).
    pub fn new(width: u8, height: u8, seed: u64) -> Board {
        assert!(width > 0 && height > 0, "board must be at least 1×1");
        let (w, h) = (width as usize, height as usize);
        let mut board = Board {
            grid: Grid {
                width,
                height,
                h_walls: vec![false; (h + 1) * w],
                v_walls: vec![false; h * (w + 1)],
                tiles: vec![TileKind::Empty; w * h],
            },
            seed,
            signposts: vec![None; w * h],
            rules: Rules::default(),
            crabs: Vec::new(),
            scores: [0; MAX_PLAYERS],
            rng: Pcg32::new(seed, 0x0005_eaba_55ed),
            tick: 0,
            signpost_seq: 0,
            next_crab_id: 0,
            gulls: Vec::new(),
            next_gull_id: 0,
            incoming: Vec::new(),
            lure: None,
            lure_cooldown: 0,
            crabs_banked: 0,
            golden_banked: 0,
            golden_call: None,
            wrap: false,
            tide: events::Tide::default(),
            sweeps: Sweeps::default(),
            refused_lures: Vec::new(),
            walked_home: Vec::new(),
            hands: [None; MAX_PLAYERS],
            steering: steering::Steering::default(),
        };
        // The border walls are the wrap rule with wrap off; the border
        // indices are stated once, in set_wrap.
        board.set_wrap(false);
        board
    }

    /// Become an exact copy of `other`, reusing the buffers this board
    /// already holds instead of allocating fresh ones.
    ///
    /// `*self = other.clone()` is the same value and costs one allocation
    /// per non-empty `Vec`, which is a poor bargain for a caller that makes
    /// a copy per node of a search and drops it a moment later
    /// (`sim::solve`). `Clone::clone_from` is not available: the derive does
    /// not forward it to the fields, so a derived one allocates as much as
    /// `clone`.
    ///
    /// Every field is named rather than waved through with `..`, so a new
    /// field on `Board` fails to compile here instead of quietly going
    /// uncopied and letting the copy play differently.
    pub(crate) fn copy_from(&mut self, other: &Self) {
        let Self {
            grid,
            seed,
            signposts,
            rules,
            crabs,
            scores,
            rng,
            tick,
            signpost_seq,
            next_crab_id,
            gulls,
            next_gull_id,
            incoming,
            lure,
            lure_cooldown,
            crabs_banked,
            golden_banked,
            golden_call,
            wrap,
            tide,
            sweeps,
            refused_lures,
            walked_home,
            hands,
            steering,
        } = other;
        self.grid.copy_from(grid);
        self.seed = *seed;
        refill(&mut self.signposts, signposts);
        self.rules = *rules;
        refill(&mut self.crabs, crabs);
        self.scores = *scores;
        self.rng.clone_from(rng);
        self.tick = *tick;
        self.signpost_seq = *signpost_seq;
        self.next_crab_id = *next_crab_id;
        refill(&mut self.gulls, gulls);
        self.next_gull_id = *next_gull_id;
        refill(&mut self.incoming, incoming);
        self.lure = *lure;
        self.lure_cooldown = *lure_cooldown;
        self.crabs_banked = *crabs_banked;
        self.golden_banked = *golden_banked;
        self.golden_call = *golden_call;
        self.wrap = *wrap;
        self.tide.copy_from(tide);
        self.sweeps.copy_from(sweeps);
        refill(&mut self.refused_lures, refused_lures);
        refill(&mut self.walked_home, walked_home);
        self.hands = *hands;
        self.steering.copy_from(steering);
    }

    // --- level authoring -------------------------------------------------

    /// Add or remove the wall on the `dir` side of tile `(x, y)`. Walls are
    /// stored per-edge, so the neighbouring tile sees the same wall and the
    /// two tiles cannot disagree (spec §7.3).
    ///
    /// Except at the rim of an open-ocean board, where nothing is stored:
    /// see [`Board::on_open_rim`].
    pub fn set_wall(&mut self, x: u8, y: u8, dir: Direction, present: bool) {
        assert!(
            self.in_bounds(i32::from(x), i32::from(y)),
            "wall off the board"
        );
        if self.on_open_rim(x, y, dir) {
            return;
        }
        self.set_edge(x as usize, y as usize, dir, present);
    }

    /// Whether the edge leaving `(x, y)` toward `dir` is the rim of a
    /// wrapping board, which takes no wall.
    ///
    /// The rim is the seam a creature crosses to the far side, and it is
    /// stored as two edges, one on each side of the beach. A wall on one of
    /// them was a one-way door: walked into from this side, open from the
    /// other. It also did not survive a save, since the level format opens
    /// the whole rim when it reads `wrap: on`. So the rim of an open beach
    /// is open, full stop: [`Board::set_wrap`] clears it, `set_wall` leaves
    /// it alone, and a snapshot holding a wall there is refused.
    pub fn on_open_rim(&self, x: u8, y: u8, dir: Direction) -> bool {
        let (dx, dy) = dir.offset();
        self.wrap && !self.in_bounds(i32::from(x) + dx, i32::from(y) + dy)
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
        self.grid.tiles[t as usize] = kind;
    }

    // --- the player's one verb -------------------------------------------

    /// The rules this board plays by.
    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    /// The rules to change, before the first tick: a level, a lobby or a
    /// test setting up the beach it plays on.
    pub fn rules_mut(&mut self) -> &mut Rules {
        &mut self.rules
    }

    /// How many signposts `player` may have standing: their hand's count
    /// under claw-keyed arrows ([`Rules::claw_hands`]), none for a seat
    /// past the two hands there, and [`Rules::signpost_cap`] elsewhere.
    pub fn signpost_cap_of(&self, player: PlayerId) -> u8 {
        match self.rules.claw_hands {
            Some(hands) => hands.get(usize::from(player)).copied().unwrap_or(0),
            None => self.rules.signpost_cap,
        }
    }

    /// Whether signpost `post` turns a crab of `handed` claw arriving on
    /// it: always, unless claw-keyed arrows are on and it is the other
    /// hand's ([`Rules::claw_hands`]). Gulls never ask: every arrow turns
    /// a gull.
    pub fn turns_crab(&self, post: &Signpost, handed: Handedness) -> bool {
        self.rules.claw_hands.is_none() || claw_of(post.owner) == Some(handed)
    }

    /// Change the signpost cap and what happens at it. Versus keeps the
    /// default (3, evict-oldest); puzzle mode sets (inventory, reject).
    pub fn set_signpost_rule(&mut self, cap: u8, policy: CapPolicy) {
        self.rules.signpost_cap = cap;
        self.rules.cap_policy = policy;
    }

    /// Open (or close) the board edges. Opening removes the border walls so
    /// creatures walk and fly off one side and re-enter on the opposite one.
    pub fn set_wrap(&mut self, wrap: bool) {
        self.wrap = wrap;
        self.grid.set_rim(!wrap);
    }

    pub fn wrap(&self) -> bool {
        self.wrap
    }

    /// Enable tide events (the sparkling crab's roulette). Off by default:
    /// puzzles stay predictable.
    pub fn events_enabled(&self) -> bool {
        self.tide.enabled
    }

    pub fn set_events_enabled(&mut self, enabled: bool) {
        self.tide.enabled = enabled;
    }

    /// The most recent tide event and when it fired.
    pub fn last_event(&self) -> Option<(TideEvent, u64)> {
        self.tide.last
    }

    /// Whose sparkling crab spun the most recent tide event, `None` when
    /// it was called instead (a spectators' vote).
    pub fn event_spun_by(&self) -> Option<PlayerId> {
        self.tide.spun_by
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
        // Before anything that could spin the roulette, so an event
        // firing this tick gets its whole cooldown rather than one tick
        // less than it.
        self.tide.cooldown = self.tide.cooldown.saturating_sub(1);
        // And before anything that could add to it. Kept for a few ticks
        // rather than cleared outright: the only reader runs between
        // frames, and a frame can cover more than one tick.
        self.sweeps.forget_before(self.tick);
        let tick = self.tick;
        self.refused_lures
            .retain(|refused| refused.at + SWEEP_MEMORY > tick);
        self.walked_home
            .retain(|walked| walked.at + SWEEP_MEMORY > tick);
        for player in self.action_order() {
            self.apply_action(player, actions[player as usize]);
        }
        self.expire_signposts();
        self.run_golden_call();
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
        if let Some((_, ticks)) = &mut self.tide.mania {
            *ticks -= 1;
            if *ticks == 0 {
                self.tide.mania = None;
            }
        }
        self.tide.claw_call = self.tide.claw_call.saturating_sub(1);
        if let Some((_, ticks)) = &mut self.tide.tempo {
            *ticks -= 1;
            if *ticks == 0 {
                self.tide.tempo = None;
            }
        }
        self.land_sweeps();
        self.tick += 1;
        #[cfg(debug_assertions)]
        self.check_invariants();
    }

    /// What every tick must leave true, checked after each one in a debug
    /// build (which is what the tests run). Each is something a later
    /// reader relies on without looking:
    ///
    /// - a running timer has at least a tick left, because an expired one
    ///   is cleared the tick it reaches zero, and the snapshot parser
    ///   refuses a zero, so a board holding one could not be resumed or
    ///   sent to a watcher catching up;
    /// - a signpost stands on sand, since crabs turn on it and only sand
    ///   takes one, and its sequence number is behind the counter, or the
    ///   cap would evict the wrong post first;
    /// - no seat holds more posts than the cap;
    /// - every creature is on the board, which the tile indexing assumes,
    ///   and so is every gull on its way in, due on a tick still to come
    ///   and no further off than the spawner sends for one;
    /// - the golden crabs banked are some of the crabs banked;
    /// - an open-ocean board has no wall on its rim (see
    ///   [`Board::on_open_rim`]).
    ///
    /// The snapshot parser refuses a board breaking any of these, since a
    /// share code or a save file is text anybody can edit, and a board
    /// read from one must not trip an assertion a tick later.
    #[cfg(debug_assertions)]
    fn check_invariants(&self) {
        for (what, left) in [
            ("lure", self.lure.map(|(_, t)| t)),
            ("mania", self.tide.mania.map(|(_, t)| t)),
            ("tempo", self.tide.tempo.map(|(_, t)| t)),
        ] {
            debug_assert!(left != Some(0), "a {what} left running with no ticks");
        }
        let tiles = self.grid.tiles.len();
        let mut held = [0usize; MAX_PLAYERS];
        for (t, post) in self.signposts.iter().enumerate() {
            let Some(post) = post else {
                continue;
            };
            debug_assert_eq!(
                self.grid.tiles[t],
                TileKind::Empty,
                "a signpost stands on a {:?} at tile {t}",
                self.grid.tiles[t]
            );
            debug_assert!(post.seq < self.signpost_seq, "a signpost from the future");
            if let Some(slot) = held.get_mut(usize::from(post.owner)) {
                *slot += 1;
            }
        }
        debug_assert!(
            held.iter()
                .all(|&n| n <= usize::from(self.rules.signpost_cap)),
            "posts over the cap of {}: {held:?}",
            self.rules.signpost_cap
        );
        debug_assert!(
            self.crabs.iter().all(|crab| usize::from(crab.tile) < tiles),
            "a crab off the board"
        );
        debug_assert!(
            self.gulls.iter().all(|gull| usize::from(gull.tile) < tiles),
            "a gull off the board"
        );
        debug_assert!(
            self.incoming
                .iter()
                .all(|gull| usize::from(gull.tile) < tiles
                    && gull.at >= self.tick
                    && gull.at <= self.tick + u64::from(GULL_LEAD)),
            "a gull on its way in off the board or off the clock"
        );
        debug_assert!(self.golden_banked <= self.crabs_banked);
        debug_assert!(
            !(self.wrap && self.grid.rim_walled()),
            "a wall on the rim of an open beach"
        );
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
        self.rules
            .round_length
            .is_some_and(|len| self.tick >= u64::from(len))
    }

    /// Ticks left before the wave, if a round timer is set.
    pub fn remaining_ticks(&self) -> Option<u64> {
        self.rules
            .round_length
            .map(|len| u64::from(len).saturating_sub(self.tick))
    }

    /// The final scramble: the last 30 s of a timed round, when gulls spawn
    /// at double rate. True from tick 0 on a round no longer than
    /// [`SURGE_TICKS`], which is every timed level (see there).
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
            // The spectators', not this seat's: the host carries it because
            // the host is the one every peer waits on anyway. It fires
            // whatever the wheel's own cooldown says, because their call is
            // not a spin of the wheel and should not be refused by one.
            //
            // Through `surge_safe` all the same. The swap it makes is not
            // the wheel's bookkeeping but a rule about the beach, measured:
            // Gull Mania on top of the surge leaves fifteen gulls and
            // almost no crabs with half a minute to play. A crowd calling
            // it down is the surge's worst case, not its exception.
            //
            // Only on a beach whose wheel turns at all, though: never on a
            // puzzle, or a versus round replayed from before every round
            // turned it, where the wheel is off. The `enabled` flag says where events fire,
            // and a call is an event firing.
            PlayerAction::CallEvent(event) => {
                if let Some(runs) = self.call_runs(event) {
                    self.force_tide_event(runs, player);
                }
            }
        }
    }

    /// Credit a banked crab to `owner`, under whatever the tide is asking
    /// for right now.
    ///
    /// Every bank goes through here, the ordinary walk into a castle and
    /// Crab Monopoly's sweep alike: the two used to add the crab's value
    /// each in their own line, and a rule that arrived later would have
    /// landed on one of them. So does the count of crabs banked, golden
    /// ones and all, which three sites used to bump by hand.
    ///
    /// Right Claws is that rule. While it runs, a right-clawed crab is
    /// worth twice what it says and a left-clawed one costs the same
    /// again, down to nothing and no further: a score that has gone
    /// negative is a player who has stopped playing, and this game is for
    /// children.
    pub(super) fn credit_bank(&mut self, owner: PlayerId, crab: &Crab) -> i32 {
        self.credit_bank_under(owner, crab, self.in_claw_call())
    }

    /// [`Self::credit_bank`] under a claw call given rather than read off
    /// the clock: a swept crab's, written down when the sweep fired.
    /// Returns what the crab was worth, before the floor at nothing.
    fn credit_bank_under(&mut self, owner: PlayerId, crab: &Crab, claw_call: bool) -> i32 {
        let points = crab.bank_points(self.crab_value(crab.kind), claw_call);
        let score = &mut self.scores[owner as usize];
        *score = score.saturating_add_signed(points);
        self.crabs_banked += 1;
        if crab.kind == CrabKind::Golden {
            self.golden_banked += 1;
        }
        points
    }

    /// A walker's step this tick: the tempo-adjusted speed, halved (never
    /// to zero) while standing in a tide pool. One statement of the wading
    /// rule, for crabs and gulls both.
    fn walk_step(&self, tile: u16, base: u16) -> u16 {
        debug_assert!(
            usize::from(tile) < self.grid.tiles.len(),
            "walking off the board: tile {tile} of {}",
            self.grid.tiles.len()
        );
        let step = self.tempo_speed(base);
        if self.grid.tiles[tile as usize] == TileKind::Pool {
            return (step / 2).max(1);
        }
        step
    }

    /// Tide-event tempo: doubled or halved speed for every creature.
    fn tempo_speed(&self, base: u16) -> u16 {
        match self.tide.tempo {
            Some((Tempo::Fast, _)) => base * 2,
            Some((Tempo::Slow, _)) => (base / 2).max(1),
            None => base,
        }
    }

    // --- read access (render layer, modes, tests) ------------------------

    pub fn width(&self) -> u8 {
        self.grid.width
    }

    pub fn height(&self) -> u8 {
        self.grid.height
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

    /// The gulls flying in, which land in this order: each one's tick, the
    /// tile it comes down on and the way it faces (see [`IncomingGull`]).
    pub fn incoming_gulls(&self) -> &[IncomingGull] {
        &self.incoming
    }

    /// The golden crab the board has called, once the call is public: the
    /// tick it comes out on and the spawner hole it comes out of, from
    /// [`GOLDEN_NOTICE`] ticks before it does.
    ///
    /// Drawn a minute or more ahead, but told to nobody until the notice:
    /// a bot reads the board and a person reads the screen, and a minute's
    /// warning for one and five seconds for the other is not one race.
    pub fn golden_call(&self) -> Option<(u64, u8, u8)> {
        self.golden_call
            .filter(|&(at, _)| at.saturating_sub(self.tick) <= u64::from(GOLDEN_NOTICE))
            .map(|(at, tile)| {
                let (x, y) = self.coords_u8(tile);
                (at, x, y)
            })
    }

    /// What banking a crab of this kind is worth on this board, before any
    /// Right Claws call: its kind's worth, except a called golden crab's.
    pub fn crab_value(&self, kind: CrabKind) -> u32 {
        match (kind, self.rules.golden_every) {
            (CrabKind::Golden, 1..) => GOLDEN_CALLED_VALUE,
            _ => kind.value(),
        }
    }

    /// The seed this board was constructed with.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Remove every crab standing on tile `(x, y)` (editor use).
    pub fn remove_crabs_at(&mut self, x: u8, y: u8) {
        assert!(
            self.in_bounds(i32::from(x), i32::from(y)),
            "crab off the board"
        );
        let tile = self.index(i32::from(x), i32::from(y));
        self.crabs.retain(|c| c.tile != tile);
    }

    /// Remove every gull standing on tile `(x, y)` (editor use).
    pub fn remove_gulls_at(&mut self, x: u8, y: u8) {
        assert!(
            self.in_bounds(i32::from(x), i32::from(y)),
            "gull off the board"
        );
        let tile = self.index(i32::from(x), i32::from(y));
        self.drop_gulls(|gull| gull.tile == tile);
    }

    /// Turn every crab and gull to face the way its counterpart on
    /// `settled` does, matched by order (editor use).
    ///
    /// A creature turns away from a wall when it is put down, so one put
    /// down first and walled in afterwards faces the wall until its level
    /// is read back, where it turns on the spot. The editor settles its
    /// bench against that reading after every edit, so the facing drawn,
    /// the one saved and the one played are one facing.
    pub fn face_as(&mut self, settled: &Board) {
        for (crab, twin) in self.crabs.iter_mut().zip(&settled.crabs) {
            crab.dir = twin.dir;
            crab.prev.dir = twin.dir;
        }
        for (gull, twin) in self.gulls.iter_mut().zip(&settled.gulls) {
            gull.dir = twin.dir;
            gull.prev.dir = twin.dir;
        }
    }

    /// Whether crabs come onto this beach at all: one standing on it at the
    /// start, or a hole to come out of. A versus beach without either has
    /// nothing to fight over, so the map dial leaves it out and the editor
    /// says so on its check and its save.
    pub fn feeds_crabs(&self) -> bool {
        !self.crabs.is_empty()
            || self
                .grid
                .tiles
                .iter()
                .any(|tile| matches!(tile, TileKind::Spawner(_)))
    }

    /// Active molting lure, if any: (luring player, ticks left).
    pub fn lure(&self) -> Option<(PlayerId, u32)> {
        self.lure
    }

    /// Ticks left in the quiet spell after a lure, when a molt banked
    /// starts none. Public: anyone who watched the lure end can count it.
    pub fn lure_cooldown(&self) -> u32 {
        self.lure_cooldown
    }

    /// Whether a molting crab banked now would start a lure: no lure is
    /// running and the quiet spell after the last one is over. The one
    /// rule the bank reads, and what the board's dimmed molts and the AI's
    /// worth of a molt read too.
    pub fn lure_ready(&self) -> bool {
        self.lure.is_none() && self.lure_cooldown == 0
    }

    /// For a molting crab banked lately that started no lure, whether a
    /// lure was running on the tick it banked (`false`: the quiet spell
    /// after one). `None` for any other crab, a molt the tide swept home
    /// among them, since that path never starts a lure on any rule.
    pub fn lure_refused(&self, crab: u32) -> Option<bool> {
        self.refused_lures
            .iter()
            .find(|refused| refused.crab == crab)
            .map(|refused| refused.running)
    }

    /// For a crab that walked into a castle lately, the seat it banked for,
    /// the castle's tile and what it paid, before the floor at nothing.
    /// `None` for any other crab: one a gull took, however close to a
    /// castle, and one the tide swept home (see [`Self::swept_home`]).
    pub fn walked_home(&self, crab: u32) -> Option<(PlayerId, u16, i32)> {
        self.walked_home
            .iter()
            .find(|walked| walked.crab == crab)
            .map(|walked| (walked.owner, walked.tile, walked.points))
    }

    /// Whether Right Claws is running: a right-clawed crab banks double
    /// and a left-clawed one costs. Read by the bots, who would otherwise
    /// go on herding whatever is nearest and hand their points back.
    pub fn in_claw_call(&self) -> bool {
        self.tide.claw_call > 0
    }

    /// What a crab is worth to whoever banks it right now, which is not
    /// always what its kind says. `None` for one that would cost.
    pub fn bank_worth(&self, crab: &Crab) -> Option<u32> {
        u32::try_from(crab.bank_points(self.crab_value(crab.kind), self.in_claw_call())).ok()
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
        self.grid.tiles[self.index(i32::from(x), i32::from(y)) as usize]
    }

    /// Every tile with its coordinates, in the board's own row-major order.
    pub fn tiles(&self) -> impl Iterator<Item = (u8, u8, TileKind)> + '_ {
        let width = self.grid.width;
        self.grid
            .tiles
            .iter()
            .enumerate()
            .map(move |(index, &kind)| {
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

    /// The owner of every castle on the board, in reading order, once per
    /// castle: a seat with two castles is named twice.
    pub fn castle_owners(&self) -> impl Iterator<Item = PlayerId> + '_ {
        // Over the tiles themselves rather than `tiles()`, which pays a
        // divide and a remainder per tile to name coordinates this throws
        // away. `seats_in_play` runs this on every tick, and the solver
        // ticks a board up to `PUZZLE_TICK_LIMIT` times per node.
        self.grid.tiles.iter().filter_map(|kind| match kind {
            TileKind::Castle(owner) => Some(*owner),
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
        self.owners_held().iter().filter(|held| **held).count() as u8
    }

    /// How big a table this beach can seat: its castles' owners when they
    /// run 0, 1, 2 and on with none skipped, `None` when one is.
    ///
    /// Seat `n` plays castle `n`. Castles marked 0 and 2 counted as two
    /// seats by owner ([`Board::castle_seats`]), and a table of two sat at
    /// them with seat 1 owning nothing to bank in all round, while the
    /// castle marked 2 banked for a seat nobody played. Such a beach seats
    /// no table at all.
    pub fn table_seats(&self) -> Option<u8> {
        let held = self.owners_held();
        let run = held.iter().take_while(|held| **held).count();
        held[run..].iter().all(|held| !held).then_some(run as u8)
    }

    /// Why no table can sit at this beach, `None` when one can: its
    /// castles skip a seat ([`Board::table_seats`]), or there are castles
    /// for fewer than the two seats a table needs. The one question the
    /// level loader and the editor's check both ask.
    pub fn arena_problem(&self) -> Option<ArenaProblem> {
        let held = self.owners_held();
        let run = held.iter().take_while(|held| **held).count();
        match held[run..].iter().any(|held| *held) {
            true => Some(ArenaProblem::SkippedSeat(run as PlayerId)),
            false if run < 2 => Some(ArenaProblem::TooFew(run as u8)),
            false => None,
        }
    }

    /// Which seats own at least one castle, by seat number. An owner past
    /// [`MAX_PLAYERS`] seats nobody and is left out.
    fn owners_held(&self) -> [bool; MAX_PLAYERS] {
        let mut held = [false; MAX_PLAYERS];
        for owner in self.castle_owners() {
            if let Some(slot) = held.get_mut(usize::from(owner)) {
                *slot = true;
            }
        }
        held
    }

    /// Put away the castles of seats a table of `seats` does not have,
    /// leaving open sand where they stood.
    ///
    /// A handmade beach can have castles for more players than sit down at
    /// it. Left standing, the castles past the table banked crabs for
    /// seats nobody played: points on the board, a replay that counted the
    /// phantom seats back in from the castles and could crown one of them,
    /// and bots told scores for players who were never there. Every board a
    /// table plays on a handmade beach is built through
    /// [`crate::sim::Level::table_board`], which calls this, so every peer
    /// and the recording start from the same sand.
    pub fn retire_castles_from(&mut self, seats: u8) {
        for tile in &mut self.grid.tiles {
            if let TileKind::Castle(owner) = *tile
                && owner >= seats
            {
                *tile = TileKind::Empty;
            }
        }
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
mod lookahead;
mod signposts;
mod snapshot;
mod steering;
mod sweep;
#[cfg(test)]
mod tests;

pub use events::{Mania, Tempo, TideEvent};
pub use signposts::Refusal;
pub use steering::raid_loss;
use sweep::Sweeps;
pub use sweep::{SWEEP_MEMORY, Sweeping};

pub use geometry::Grid;
pub(crate) use geometry::Walker;
pub(crate) use snapshot::{bits_to_hex, golden_every, tile_token};
