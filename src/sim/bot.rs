//! A basic signpost-placing opponent. Deterministic and pure: the action is
//! a function of the board alone, so bot matches replay bit-for-bit and the
//! same bot could later run host-side in online games.
//!
//! Heuristic, in priority order:
//! 1. Defend: a walking gull near the castle gets a signpost slammed in
//!    front of it, pointing back where it came from.
//! 2. Recruit: the most valuable crab nearby that is not already heading
//!    castle-ward gets a signpost on the tile ahead of it, pointing home.
//!
//! Fierce additionally reads the terrain (see [`BotLevel::reads_terrain`]):
//! it shoves gulls into kelp they cannot enter, walks its crabs around tide
//! pools instead of through them, and never spends a signpost whose turn a
//! turnstile would immediately undo.
//!
//! The bot acts on a fixed cadence (staggered per seat so bots do not move
//! in lockstep, and rotated per cycle so no seat is permanently quickest;
//! see [`BotLevel::acts_on`]) and simply retries next window if a placement
//! was rejected.

use crate::sim::board::{
    Board, LURE_COOLDOWN, LURE_TICKS, PlayerAction, PlayerId, SUBUNITS_PER_TILE, TICKS_PER_SECOND,
    TileKind,
};
use crate::sim::crab::{CrabKind, Handedness};
use crate::sim::direction::Direction;
use crate::sim::gull::GullState;

/// Bot difficulty. Levels differ in reaction cadence and search radii;
/// Hard additionally plays offense, steering gulls at the leading rival's
/// castle. All levels stay pure functions of the board.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BotLevel {
    Easy,
    #[default]
    Normal,
    Hard,
}

impl BotLevel {
    /// The level's word in a save file or a replay: the English name the
    /// match setup shows, so a file read by hand says what was played.
    pub fn token(self) -> &'static str {
        match self {
            BotLevel::Easy => "easy",
            BotLevel::Normal => "normal",
            BotLevel::Hard => "fierce",
        }
    }

    /// The level a [`token`](Self::token) names, if it names one.
    pub fn from_token(token: &str) -> Option<BotLevel> {
        match token {
            "easy" => Some(BotLevel::Easy),
            "normal" => Some(BotLevel::Normal),
            "fierce" => Some(BotLevel::Hard),
            _ => None,
        }
    }

    /// Ticks between decisions, staggered per seat.
    fn cadence(self) -> u64 {
        match self {
            BotLevel::Easy => 40,
            BotLevel::Normal => 20,
            BotLevel::Hard => 12,
        }
    }

    fn defend_radius(self) -> i32 {
        match self {
            BotLevel::Easy => 2,
            BotLevel::Normal => 4,
            BotLevel::Hard => 6,
        }
    }

    /// How far the bot will walk to attack rather than play its own corner.
    /// Only the fierce bot attacks at all, and only when the gull is on its
    /// way past.
    fn reach(self) -> i32 {
        match self {
            BotLevel::Easy | BotLevel::Normal => 0,
            BotLevel::Hard => 5,
        }
    }

    /// How far the bot will go for a jackpot: a golden crab, or a molting
    /// one and the lure that follows it home. Worth crossing the board for,
    /// where chasing a rival is not. Fierce only; the other levels see no
    /// further than their own corner.
    fn jackpot_reach(self) -> i32 {
        match self {
            BotLevel::Easy | BotLevel::Normal => 0,
            BotLevel::Hard => 14,
        }
    }

    /// One placement in this many is aimed the wrong way: the easy bot's
    /// hand slips. Zero means it never blunders.
    ///
    /// Derived from the tick and the seat rather than a random draw, so the
    /// bot stays a pure function of the board and every peer derives the
    /// same move for an AI seat.
    fn blunder_every(self) -> u64 {
        match self {
            BotLevel::Easy => 4,
            BotLevel::Normal => 8,
            BotLevel::Hard => 0,
        }
    }

    /// Whether the bot picks the crab that is *worth* the most (a molting
    /// crab and its lure, a golden jackpot) or just the nearest one. The
    /// easy bot cannot tell a jackpot from a common crab.
    fn values_the_catch(self) -> bool {
        !matches!(self, BotLevel::Easy)
    }

    fn recruit_radius(self) -> i32 {
        match self {
            BotLevel::Easy => 4,
            BotLevel::Normal => 7,
            BotLevel::Hard => 10,
        }
    }

    /// Whether this level plays the terrain (kelp, pools, turnstiles) or
    /// treats the beach as flat sand. Fierce only.
    fn reads_terrain(self) -> bool {
        matches!(self, BotLevel::Hard)
    }

    /// Whether this seat gets to think on this tick.
    ///
    /// Every seat decides once per cadence window (an unequal rate simply
    /// lets the faster thinker play more) at a moment drawn from the window
    /// and the seat. Drawn rather than a fixed grid of slots because the
    /// spawner holes fire on even ticks, so a seat parked on an odd slot
    /// meets every new crab a beat before a seat on an even one.
    ///
    /// The draw takes the board's seed as well, as [`slips`] does: drawn
    /// from the window and the seat alone, each seat sat on one schedule
    /// in every round ever played, and the one P2 (seat 1) was dealt kept it 2%
    /// to 4% behind on the four-player classic beach whichever castle it
    /// held, 4.5 standard errors over 10,000 rounds. Fixed for a board's
    /// life and carried in its snapshot, so every peer still agrees.
    ///
    /// Ties inside a single tick are the sim's business, not the bot's:
    /// see [`Board::action_order`](Board).
    fn acts_on(self, player: PlayerId, ticks: u64, seed: u64) -> bool {
        let cadence = self.cadence();
        let window = ticks / cadence;
        let z = avalanche(
            window.wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ u64::from(player).wrapping_mul(0xBF58_476D_1CE4_E5B9)
                ^ seed,
        );
        ticks % cadence == z % cadence
    }
}

/// How far from a rival castle a Hard bot will weaponize a passing gull.
const ATTACK_RADIUS: i32 = 6;

/// The fair cursor's lift: ticks before a held key starts repeating, the
/// human default of 0.28 s (`GameSettings::repeat_delay`).
pub const FAIR_LIFT: u32 = 8;
/// The fair cursor's pace once a key repeats: a tile every three ticks, the
/// human default of 0.09 s (`GameSettings::repeat_interval`).
pub const FAIR_TICKS_PER_TILE: u32 = 3;

/// How long a person's hand takes to cross `steps` tiles, holding the keys
/// down: the first tap moves a tile at once, the next waits out the lift,
/// and each after that the repeat.
///
/// The fair cursor's one statement (`docs/bot-seats.md`): every seat that
/// is not a person walks by it, the game's AI here and a bot's virtual
/// cursor in `bots::cursor`, so neither out-walks the other or a person.
pub fn fair_walk(steps: u32) -> u32 {
    match steps {
        0 | 1 => 0,
        far => FAIR_LIFT + (far - 2) * FAIR_TICKS_PER_TILE,
    }
}

/// Tiles between two, as a hand walks them: diagonally, one step a tile in
/// both directions at once, the way two held arrow keys move a cursor.
pub fn hand_steps((ax, ay): (u8, u8), (bx, by): (u8, u8)) -> u32 {
    u32::from(ax.abs_diff(bx).max(ay.abs_diff(by)))
}

/// How a seat's hand reaches a tile.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Hand {
    /// At a person's pace ([`fair_walk`]). Every match with a person in it.
    #[default]
    Fair,
    /// Anywhere at once: an all-bot match with the fair cursor rule off,
    /// where every seat is equally instant and nobody is there to be fair to.
    Instant,
}

/// Why the bot wants this tile. A gull bearing down on your castle is worth
/// spending your last signpost on, since it carries off a third of the bank,
/// where a wayward crab is not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Intent {
    Defend,
    Recruit,
    Attack,
}

/// What the bot wants to do this tick, and whether its hand could get there
/// in time to do it.
///
/// A bot's cursor is wherever it last placed a signpost, read off the board
/// rather than stored, so the bot stays a pure function of the state and
/// every peer of an online match derives the same move for it. It walks
/// under the fair cursor rule, which every match with a person in it plays
/// by.
pub fn bot_action(board: &Board, player: PlayerId, level: BotLevel) -> PlayerAction {
    bot_action_with(board, player, level, Hand::Fair)
}

/// [`bot_action`] with the hand named: [`Hand::Instant`] is for all-bot
/// matches played with the fair cursor rule off.
pub fn bot_action_with(
    board: &Board,
    player: PlayerId,
    level: BotLevel,
    hand: Hand,
) -> PlayerAction {
    let (wanted, intent) = decide(board, player, level);
    let wanted = fumble(wanted, player, level, board.ticks(), board.seed());
    // Only a placement has a tile to walk to, or a post to cost; nothing
    // else waits or weighs.
    if let PlayerAction::Place { x, y, .. } = wanted
        && ((hand == Hand::Fair && !hand_arrived(board, player, x, y))
            || !worth_the_walk(board, player, level, x, y, intent))
    {
        return PlayerAction::None;
    }
    wanted
}

/// splitmix64's finaliser: the shifts and multiply that fold a struck-together
/// key's high bits down over its low ones. Shared by the two draws this file
/// makes from state that is not itself random.
///
/// It does not make every bit as good as every other: a multiply carries
/// upwards only, so the lowest bits stay the least mixed and the reader has
/// to know which end to read from. See [`slips`].
fn avalanche(mut z: u64) -> u64 {
    z ^= z >> 31;
    z = z.wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 29;
    z
}

/// The bot's hand slips: one placement in [`BotLevel::blunder_every`] comes
/// out pointing the wrong way. A legible difficulty knob, one you can watch
/// happen, where "thinks less often" is invisible.
fn fumble(
    action: PlayerAction,
    player: PlayerId,
    level: BotLevel,
    ticks: u64,
    seed: u64,
) -> PlayerAction {
    let every = level.blunder_every();
    if every == 0 {
        return action;
    }
    // Only a placement can come out crooked; the tile is still the right one.
    if let PlayerAction::Place { x, y, dir } = action
        && slips(ticks, player, seed, every)
    {
        return PlayerAction::Place {
            x,
            y,
            dir: dir.right(),
        };
    }
    action
}

/// Whether the hand slips on a placement made by `player` at `ticks` on the
/// board `seed` built: one in `every`, and the same one on every peer.
///
/// Written `(ticks ^ player).is_multiple_of(every)`, each seat sat on a
/// residue of its own mod `every`, while [`BotLevel::acts_on`] only offers
/// a seat the chance to place every `cadence` ticks; 20 and 8 share a
/// factor, so the residues were not sampled evenly and the rates came out
/// 8.9% to 13.7% against an intended 12.5%, worth about 4 sigma of seat
/// drift on a 3000-game sweep. As a function of the tick alone it was the
/// same handicap in every round ever played, so averaging could not find
/// it.
///
/// The seed is what varies it from board to board: fixed for the life of a
/// board, consuming no PRNG state, and travelling in the snapshot, so peers
/// and pasted rounds still agree to the tick.
fn slips(ticks: u64, player: PlayerId, seed: u64, every: u64) -> bool {
    let z = avalanche(
        ticks.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ u64::from(player).wrapping_mul(0xBF58_476D_1CE4_E5B9)
            ^ seed,
    );
    // The draw is `z` scaled into `0..every`, and a slip is the first
    // bucket. Scaled from the top of the hash rather than taken as
    // `z % every`, because every blunder rate here is a power of two and
    // `%` on one of those reads the lowest bits, which a multiplying hash
    // never carries into: at Easy's one-in-four that gives 23.7% to 26.3%
    // where the top bits give 24.9% to 25.2%.
    (u128::from(z) * u128::from(every)) >> 64 == 0
}

/// Whether this placement is worth the trip.
///
/// Defending is always worth it: a gull reaching a castle carries off a
/// third of the bank. Chasing a rival across the board is not: the walk is dead time
/// that costs more than the raid pays. Offence has to be on the way.
///
/// The walk is measured on the flat grid even on an open beach: it is the
/// hand's walk, and a cursor does not wrap at the screen's edge the way a
/// creature does at the seam.
///
/// Deliberately not a rule about evicting its own posts: a crab walks a
/// tile every twenty ticks, so a post aimed where one was four seconds ago
/// is aimed at nothing and the churn helps.
fn worth_the_walk(
    board: &Board,
    player: PlayerId,
    level: BotLevel,
    x: u8,
    y: u8,
    intent: Intent,
) -> bool {
    if intent != Intent::Attack {
        return true;
    }
    let Some((from_x, from_y, _)) = board.last_placement_of(player) else {
        return true;
    };
    let steps = i32::from(x.abs_diff(from_x)) + i32::from(y.abs_diff(from_y));
    steps <= level.reach()
}

/// Whether the bot's cursor has had time to reach `(x, y)` since its last
/// placement, walking under the fair cursor rule. Before its first one its
/// hand is at its castle, where a person's cursor and a bot's start, as of
/// the round's first tick: the opening placement walks like anyone's,
/// rather than landing anywhere at once. After it, the hand is where it
/// last placed whether or not that post still stands.
///
/// The hand is read off the board, never kept, so every peer derives the
/// same one. Time spent idle counts toward the walk, as it does for a
/// person who moved their cursor ahead while waiting.
///
/// Every level walks at the same pace: the rule is a person's speed, and a
/// fiercer bot is fiercer in what it sees and chooses, never in a hand no
/// person at the table could match.
fn hand_arrived(board: &Board, player: PlayerId, x: u8, y: u8) -> bool {
    let (from_x, from_y, since) = board.last_placement_of(player).unwrap_or_else(|| {
        let (x, y) = board
            .castle_of(player)
            .unwrap_or((board.width() / 2, board.height() / 2));
        (x, y, 0)
    });
    let walk = fair_walk(hand_steps((from_x, from_y), (x, y)));
    board.ticks().saturating_sub(since) >= u64::from(walk)
}

/// What the bot wants to do this tick.
///
/// Four strategies in order of what a round is won and lost on: a gull about
/// to raid you costs a third of your bank, a jackpot is worth crossing the beach
/// for, a wayward crab is bread and butter, and shoving a gull at the leader
/// is the luxury you buy last. Each answers `None` when it has nothing to say.
fn decide(board: &Board, player: PlayerId, level: BotLevel) -> (PlayerAction, Intent) {
    let nothing = (PlayerAction::None, Intent::Recruit);
    if !level.acts_on(player, board.ticks(), board.seed()) {
        return nothing;
    }
    let Some(castle) = castle_of(board, player) else {
        return nothing;
    };
    defend(board, player, level, castle)
        .or_else(|| chase_jackpot(board, player, level, castle))
        .or_else(|| recruit(board, player, level, castle))
        .or_else(|| attack(board, player, level))
        .unwrap_or(nothing)
}

/// Turn back the nearest gull threatening our castle.
fn defend(
    board: &Board,
    player: PlayerId,
    level: BotLevel,
    castle: u16,
) -> Option<(PlayerAction, Intent)> {
    let mut best: Option<(i32, u16, Direction)> = None;
    for gull in board.gulls() {
        if gull.state != GullState::Walking {
            continue;
        }
        let d = manhattan(board, gull.tile, castle);
        if d > level.defend_radius() {
            continue;
        }
        // A gull walking away is no threat, and a post that turns it round
        // would make one: only a bird whose next step closes on the castle
        // is worth a signpost.
        let closing = board
            .step(gull.tile, gull.dir)
            .is_some_and(|next| manhattan(board, next, castle) < d);
        if closing && best.is_none_or(|(bd, ..)| d < bd) {
            best = Some((d, gull.tile, gull.dir));
        }
    }
    let (_, tile, dir) = best?;
    // Fierce shoves the gull into kelp beside its next tile when that is
    // safe: a walking gull cannot enter kelp, so the sim turns it along the
    // weed instead, and the shove is only taken when neither turn brings
    // the bird closer than sending it back the way it came. Otherwise, and
    // for the terrain-blind levels, reverse it.
    let out = board
        .step(tile, dir)
        .filter(|_| level.reads_terrain())
        .and_then(|target| safe_kelp_shove(board, target, tile, dir, castle))
        .unwrap_or_else(|| dir.reverse());
    let action = place_ahead(board, player, tile, dir, out, level)?;
    Some((action, Intent::Defend))
}

/// Fierce only: go after a jackpot anywhere on the beach. A golden crab pays
/// what the board says (fifty rolled, thirty called) and a molt turns the
/// whole board toward home, so unlike chasing a rival this walk pays for
/// itself.
fn chase_jackpot(
    board: &Board,
    player: PlayerId,
    level: BotLevel,
    castle: u16,
) -> Option<(PlayerAction, Intent)> {
    if level.jackpot_reach() == 0 {
        return None;
    }
    let mut best: Option<(u32, i32, u16, Direction)> = None;
    for crab in board.crabs() {
        let worth = match crab.kind {
            CrabKind::Golden => board.crab_value(CrabKind::Golden),
            CrabKind::Molting => molt_worth(board, crab, castle),
            CrabKind::Giant => 10,
            CrabKind::Common | CrabKind::Juvenile | CrabKind::Sparkling => continue,
        };
        // A jackpot on the wrong claw is not a jackpot: under Right Claws
        // a left-clawed golden crab costs its whole worth to bank.
        if board.bank_worth(crab).is_none() {
            continue;
        }
        let d = manhattan(board, crab.tile, castle);
        if d == 0 || d > level.jackpot_reach() || crab.dir == toward(board, crab.tile, castle) {
            continue;
        }
        if best.is_none_or(|(bw, bd, ..)| worth > bw || (worth == bw && d < bd)) {
            best = Some((worth, d, crab.tile, crab.dir));
        }
    }
    let (_, _, tile, dir) = best?;
    turn_home(board, player, level, castle, (tile, dir))
}

/// What a lure is worth to the AI on top of the molt's own points, per
/// second it runs: 2.5, the 25 a 10 s lure was once counted at.
const LURE_POINTS_PER_SECOND_X2: u32 = 5;

/// What turning `crab`, a molt, home to `castle` is worth: its points, its
/// lure when one could start by the time it gets there, and what banking
/// it keeps from the rivals.
///
/// A molt banked while a lure runs or in the quiet spell after one starts
/// none (`Board::lure_ready`), so the AI's own lure counts only for a molt
/// that will arrive after that spell, and for as long as a lure lasts
/// ([`LURE_TICKS`]).
///
/// Banked now, the molt is gone; left on the beach, it waits for whoever
/// turns it home once a lure is ready again, since a molt does not leave
/// the beach of its own accord. Against one rival that is a wash: the lure
/// left behind is as likely to be the AI's own later. Each rival past the
/// first is one more seat that could bank it first, and its lure is counted
/// in full for each. That denial is what puts a molt ahead of a giant
/// during the rest from three seats, and ahead of a called golden crab
/// once it is ready from four.
fn molt_worth(board: &Board, crab: &crate::sim::Crab, castle: u16) -> u32 {
    let points = board.crab_value(CrabKind::Molting);
    let lure = LURE_TICKS * LURE_POINTS_PER_SECOND_X2 / (2 * TICKS_PER_SECOND);
    let resting_for = match board.lure() {
        Some((_, left)) => left + LURE_COOLDOWN,
        None => board.lure_cooldown(),
    };
    let ticks_per_tile = u32::from(SUBUNITS_PER_TILE).div_ceil(u32::from(crab.kind.speed()));
    let arrives_in = manhattan(board, crab.tile, castle).unsigned_abs() * ticks_per_tile;
    let ours = if resting_for > arrives_in { 0 } else { lure };
    let denied = lure * u32::from(board.seats_in_play().saturating_sub(2));
    points + ours + denied
}

/// Put an arrow in front of the crab at `tile` walking `dir`, pointing it
/// home: the move both a jackpot chase and a recruit end in.
fn turn_home(
    board: &Board,
    player: PlayerId,
    level: BotLevel,
    castle: u16,
    (tile, dir): (u16, Direction),
) -> Option<(PlayerAction, Intent)> {
    let ahead = board.step(tile, dir).unwrap_or(tile);
    let home = homeward(board, ahead, castle, level, TileKind::Pool);
    let action = place_ahead(board, player, tile, dir, home, level)?;
    Some((action, Intent::Recruit))
}

/// Turn the best wayward crab in range toward home.
fn recruit(
    board: &Board,
    player: PlayerId,
    level: BotLevel,
    castle: u16,
) -> Option<(PlayerAction, Intent)> {
    let mut best: Option<(u32, i32, u16, Direction)> = None;
    for crab in board.crabs() {
        let d = manhattan(board, crab.tile, castle);
        if d == 0 || d > level.recruit_radius() {
            continue;
        }
        if crab.dir == toward(board, crab.tile, castle) {
            continue; // already coming to us
        }
        // The easy bot cannot tell a jackpot from a common crab and simply
        // grabs at whatever is closest. It cannot read the tide either, so
        // under Right Claws it goes on fetching crabs that cost it points,
        // which is the sort of mistake the easy bot is for.
        let value = if level.values_the_catch() {
            match board.bank_worth(crab) {
                Some(worth) => worth,
                // Costs points to bank: leave it where it is.
                None => continue,
            }
        } else {
            1
        };
        if best.is_none_or(|(bv, bd, ..)| value > bv || (value == bv && d < bd)) {
            best = Some((value, d, crab.tile, crab.dir));
        }
    }
    let (_, _, tile, dir) = best?;
    turn_home(board, player, level, castle, (tile, dir))
}

/// Fierce only: steer a gull that is already near the leading rival's castle
/// the rest of the way in.
fn attack(board: &Board, player: PlayerId, level: BotLevel) -> Option<(PlayerAction, Intent)> {
    if level != BotLevel::Hard {
        return None;
    }
    let target = leading_rival_castle(board, player)?;
    for gull in board.gulls() {
        if gull.state != GullState::Walking {
            continue;
        }
        let d = manhattan(board, gull.tile, target);
        if d == 0 || d > ATTACK_RADIUS || gull.dir == toward(board, gull.tile, target) {
            continue;
        }
        let ahead = board.step(gull.tile, gull.dir).unwrap_or(gull.tile);
        let aim = homeward(board, ahead, target, level, TileKind::Kelp);
        if let Some(action) = place_ahead(board, player, gull.tile, gull.dir, aim, level) {
            return Some((action, Intent::Attack));
        }
    }
    None
}

/// The castle of the highest-scoring rival (ties to the lowest seat).
fn leading_rival_castle(board: &Board, player: PlayerId) -> Option<u16> {
    let scores = board.scores();
    let mut best: Option<(u32, PlayerId)> = None;
    for seat in 0..crate::sim::MAX_PLAYERS as PlayerId {
        if seat == player {
            continue;
        }
        let Some(_) = castle_of(board, seat) else {
            continue;
        };
        if best.is_none_or(|(s, _)| scores[seat as usize] > s) {
            best = Some((scores[seat as usize], seat));
        }
    }
    best.and_then(|(_, seat)| castle_of(board, seat))
}

/// A seat's castle as a tile index, the form the bot's distance and step
/// helpers speak.
fn castle_of(board: &Board, player: PlayerId) -> Option<u16> {
    board.castle_of(player).map(|(x, y)| board.index_of(x, y))
}

/// Tiles a creature walks between `a` and `b`, the short way: through the
/// seam on an open beach ([`Board::displacement`]). Measured across the
/// flat grid there, a crab two tiles from home over the seam read as the
/// width of the beach away, and the AI left the castles by the seam to
/// fend for themselves (on Round the World the middle strip's castles won
/// 14% at six seats against the 17.5% of the others).
fn manhattan(board: &Board, a: u16, b: u16) -> i32 {
    let (dx, dy) = board.displacement(a, b);
    dx.abs() + dy.abs()
}

/// Greedy direction from `from` toward `to` (the sim's lure tiebreak),
/// through the seam when that is the short way, as the lure goes.
fn toward(board: &Board, from: u16, to: u16) -> Direction {
    let (dx, dy) = board.displacement(from, to);
    Direction::toward(dx, dy)
}

/// The other axis's direction toward `to`, when that axis has ground left
/// to cover. Straight lines have no second option.
fn cross_toward(board: &Board, from: u16, to: u16) -> Option<Direction> {
    let (dx, dy) = board.displacement(from, to);
    match toward(board, from, to) {
        Direction::Left | Direction::Right => (dy != 0).then(|| Direction::toward(0, dy)),
        Direction::Up | Direction::Down => (dx != 0).then(|| Direction::toward(dx, 0)),
    }
}

/// The kind of the tile one step from `tile` in `dir`, if it is on the board.
fn kind_ahead(board: &Board, tile: u16, dir: Direction) -> Option<TileKind> {
    let target = board.step(tile, dir)?;
    let (x, y) = board.coords(target);
    Some(board.tile_at(x as u8, y as u8))
}

/// Whether a walking gull standing on `tile` may step toward `dir`: the
/// bot's reading of the sim's passability (no wall on the edge, a tile on
/// the far side, and neither rock nor kelp there), so it can predict what
/// the wall resolution will do with a post it is about to plant.
fn gull_passable(board: &Board, tile: u16, dir: Direction) -> bool {
    let (x, y) = board.coords_u8(tile);
    !board.wall_at(x, y, dir)
        && !matches!(
            kind_ahead(board, tile, dir),
            None | Some(TileKind::Rock | TileKind::Kelp)
        )
}

/// Where the sim sends a gull of `handed` hand that a signpost on `tile`
/// has aimed at kelp: its preferred side, else the other, else back. The
/// same ladder as the board's wall resolution, which the bot cannot call.
fn kelp_turn(board: &Board, tile: u16, into_kelp: Direction, handed: Handedness) -> Direction {
    let (first, second) = match handed {
        Handedness::Left => (into_kelp.left(), into_kelp.right()),
        Handedness::Right => (into_kelp.right(), into_kelp.left()),
    };
    if gull_passable(board, tile, first) {
        first
    } else if gull_passable(board, tile, second) {
        second
    } else {
        into_kelp.reverse()
    }
}

/// A direction out of `target` (the tile ahead of a gull on `tile` walking
/// `travel`) that aims straight into kelp, when the shove is safe. Kelp is
/// a wall to a walking gull, so the sim turns the bird along the weed by
/// its handedness, and a right-handed gull turned to its right may be
/// facing the castle. The shove is only offered when, whichever hand the
/// gull has, the turn leaves it no closer than plain reversal would (which
/// sends it back onto `tile`). Kelp straight ahead does not count: that is
/// the gull's own line, and a post there changes nothing.
fn safe_kelp_shove(
    board: &Board,
    target: u16,
    tile: u16,
    travel: Direction,
    castle: u16,
) -> Option<Direction> {
    let reversed = manhattan(board, tile, castle);
    Direction::ALL.into_iter().find(|&dir| {
        dir != travel
            && kind_ahead(board, target, dir) == Some(TileKind::Kelp)
            && [Handedness::Left, Handedness::Right]
                .into_iter()
                .all(|handed| {
                    let turned = kelp_turn(board, target, dir, handed);
                    board
                        .step(target, turned)
                        .is_none_or(|next| manhattan(board, next, castle) >= reversed)
                })
    })
}

/// Which way to send a walker standing on `from` to reach `to`. Fierce
/// steps around `hazard`, the terrain that stops this walker (tide pools
/// halve a crab's speed, kelp walls out a gull), when the other axis makes
/// progress too; the other levels always take the greedy step.
fn homeward(board: &Board, from: u16, to: u16, level: BotLevel, hazard: TileKind) -> Direction {
    let greedy = toward(board, from, to);
    if !level.reads_terrain() || kind_ahead(board, from, greedy) != Some(hazard) {
        return greedy;
    }
    match cross_toward(board, from, to) {
        Some(other) if kind_ahead(board, from, other) != Some(hazard) => other,
        _ => greedy,
    }
}

/// Place a signpost on the tile ahead of a creature, pointing `dir_out`, if
/// that tile can take one of ours.
fn place_ahead(
    board: &Board,
    player: PlayerId,
    creature_tile: u16,
    creature_dir: Direction,
    dir_out: Direction,
    level: BotLevel,
) -> Option<PlayerAction> {
    let target = board.step(creature_tile, creature_dir)?;
    let (x, y) = board.coords(target);
    let (x, y) = (x as u8, y as u8);
    if board.tile_at(x, y) != TileKind::Empty {
        return None;
    }
    // A turnstile deflects whatever crosses it, so a post aimed straight
    // into one buys a turn the log immediately takes back. Fierce spends
    // the signpost somewhere it survives instead.
    if level.reads_terrain()
        && matches!(
            kind_ahead(board, target, dir_out),
            Some(TileKind::Turnstile { .. })
        )
    {
        return None;
    }
    match board.signpost_at(x, y) {
        Some(sp) if sp.owner != player => None,
        Some(sp) if sp.dir == dir_out => None, // already doing its job
        _ => Some(PlayerAction::Place { x, y, dir: dir_out }),
    }
}

#[cfg(test)]
mod tests;
