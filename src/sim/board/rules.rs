//! The rules a board is built with, and the seat-side readings of them:
//! teams, and the claw each seat turns under claw-keyed arrows.

use super::*;

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
