//! Brute-force puzzle validation (spec §5.3): search signpost placements
//! against the headless sim until one meets the level's goal.
//!
//! The search is depth-first over placement counts 0..=inventory. Candidate
//! tiles are pruned to tiles crabs actually cross in a run under the current
//! placements, since a signpost nobody walks over cannot change the
//! outcome, and the candidate set is recomputed at each depth so placements
//! that open new paths are still found.

use crate::sim::board::{Board, PlayerId, TileKind};
use crate::sim::crab::Handedness;
use crate::sim::direction::Direction;
use crate::sim::level::{Level, PuzzleOutcome};

/// A signpost as an instruction: where it goes, which way it points, and
/// whose hand puts it down (seat 0 on every stage but a co-op one, where
/// seat 1's arrows are the right claw's: see `Rules::claw_hands`).
///
/// The same four are a solution step, a hint, and a line in a level file,
/// so they are one name rather than three spellings.
pub type Placement = (u8, u8, Direction, PlayerId);

/// The seats whose arrows a search on `board` places: both hands under
/// claw-keyed arrows, else the one seat every puzzle is played for.
fn placing_seats(board: &Board) -> &'static [PlayerId] {
    match board.rules().claw_hands {
        Some(_) => &[0, 1],
        None => &[0],
    }
}

/// Boards a budgeted search may simulate before it gives up.
///
/// Counted in simulations rather than seconds, so a board costs the same
/// budget on every machine and in every build profile. Each simulation is
/// itself bounded by the board's [`Level::deadline`], which makes this a
/// ceiling on total work rather than merely on visits.
///
/// Sized from both ends, and both ends were measured.
///
/// The ceiling is there for the boards nobody vetted: an author's own
/// level, which can be any size, any shape, and unsolvable in ways that
/// take far longer to prove than to draw. A sealed 12x9 board with six
/// crabs ran past ten minutes unbudgeted; at this ceiling it gives up in 48
/// seconds of a release build (2026-08-15), on the editor's background
/// thread, which is a wait rather than a freeze.
///
/// The floor is the campaign.
/// `no_campaign_level_grants_a_post_it_does_not_need` proves a level's
/// inventory minimal under this number, and proving no *three*-post answer
/// exists costs roughly the cube of the tiles the crabs cross. At 50,000
/// that proof consumed the whole budget on any board big enough to need
/// four posts: of 140 boards built to need one, 101 gave up. Gulls eating
/// the crabs they meet makes a placement fail later and deeper, and four
/// gull levels needed a million (six to twelve seconds each).
pub const DEFAULT_NODE_BUDGET: u32 = 1_000_000;

/// How hard a search may work before it admits defeat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effort {
    /// Give up after this many boards simulated.
    Budget(u32),
    /// Search until the answer is certain, however long that takes. For
    /// callers that can wait and need the truth: level authoring and CI.
    Exhaustive,
}

/// What a search found, and when it found nothing, whether that is a proof.
///
/// The distinction is what a budget is for. "No solution" is a claim about
/// the level; "gave up" is a claim about the search, and an editor that
/// prints the first when it means the second tells an author their level is
/// broken when it may be fine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SolveOutcome {
    /// Placements meeting the level's goal, using as few posts as possible.
    Found(Vec<Placement>),
    /// Every placement within the inventory was tried, and none wins.
    Unsolvable,
    /// The budget ran out first. Says nothing about whether a solution exists.
    GaveUp,
}

/// Search for a signpost set (within the level's inventory) that meets the
/// level's goal, under [`DEFAULT_NODE_BUDGET`]. Prefers fewer posts.
pub fn solve(level: &Level) -> SolveOutcome {
    solve_with(level, Effort::Budget(DEFAULT_NODE_BUDGET))
}

/// [`solve`], with the ceiling named by the caller.
pub fn solve_with(level: &Level, effort: Effort) -> SolveOutcome {
    solve_from(level, effort, 0)
}

/// [`solve_with`], trying no fewer than `fewest` signposts: for a caller
/// that already knows fewer cannot win, such as the editor's second
/// search past a grant the first proved short. Each count is a search of
/// its own (the memo starts afresh at each), so the counts skipped are
/// work saved and change nothing about the counts searched; the first
/// that wins is still the fewest from `fewest` up.
pub fn solve_from(level: &Level, effort: Effort, fewest: u8) -> SolveOutcome {
    let mut search = Search::new(level, effort);
    // No deeper than the board can hold. Every post goes down before the
    // first tick, so past the cap a placement either is refused (Reject)
    // or evicts the oldest (Evict), and either way the beach plays out
    // with at most `cap` posts standing. Each such beach that wins is
    // already reached at its own size: its posts, taken in the order the
    // run first arrives at them, are each a candidate on the board the
    // earlier ones make, and a post nothing arrives at changes nothing.
    //
    // And the search deeper than that would be wrong, not just idle. The
    // memo keys a node on its placements as a set, which holds only while
    // order cannot matter, and under Evict past the cap the order decides
    // which post goes; backing out of a placement that evicted one would
    // not put the evicted one back either.
    //
    // Saturating: two hands pasted into the editor may hold more between
    // them than a `u8` counts, and the grant is a `u8` anyway.
    let room = placing_seats(&search.sim)
        .iter()
        .map(|&seat| search.sim.signpost_cap_of(seat))
        .fold(0u8, u8::saturating_add);
    for depth in fewest..=level.posts.min(room) {
        let mut board = level.board();
        if let Some(mut placements) = search.search_at(&mut board, depth) {
            placements.reverse();
            return SolveOutcome::Found(placements);
        }
        if search.gave_up {
            return SolveOutcome::GaveUp;
        }
    }
    SolveOutcome::Unsolvable
}

/// One search, and the fuel it has left.
struct Search<'a> {
    level: &'a Level,
    /// Simulations remaining, or `None` when the caller asked for certainty.
    fuel: Option<u32>,
    /// Set the moment fuel runs out, so "found nothing" can be told apart
    /// from "proved there is nothing".
    gave_up: bool,
    /// Signpost sets already explored and found wanting, each held in a
    /// fixed order so that the same set reached by a different route is
    /// recognised as the same set.
    ///
    /// Signposts are a *set*: placing A then B leaves exactly the board
    /// that placing B then A leaves, as long as neither is placed past the
    /// cap, which [`solve_with`] never searches. The search walks orderings, so without
    /// this it explores all `depth!` of them: six wasted subtrees out of
    /// seven at three posts, twenty-three out of twenty-four at four.
    ///
    /// Ordering the candidates instead would be cheaper and is not sound
    /// here: the candidate set is recomputed at each depth from the board as
    /// it now stands, so a tile can become worth trying only *because* an
    /// earlier signpost sent a crab across it.
    ///
    /// Cleared between depths, and that is not housekeeping: the search runs
    /// once per inventory size, and a set that failed with one post left to
    /// spend says nothing about the same set with two. Carrying the memo
    /// across those runs made `Both Lanes` unsolvable.
    ///
    /// Keyed by `(x, y, Direction::id, seat)` so the set can be sorted and
    /// hashed without asking a sim type to grow orderings it has no other
    /// use for.
    seen: std::collections::HashSet<Vec<(u8, u8, u8, u8)>>,
    /// The signposts standing right now, in placement order.
    placed: Vec<Placement>,
    /// The board every simulation is played on, kept from node to node.
    ///
    /// Neither [`Search::wins`] nor [`Search::visited_placeable_tiles`] may
    /// play the node's own board: that board is the position the search has
    /// to be able to back out to. A `clone` apiece is an allocation per
    /// `Vec` on it, twice per node; [`Board::copy_from`] writes the same
    /// copy into this one and keeps its buffers.
    ///
    /// Never live across a recursive call: each user fills it, reads its
    /// answer out, and is finished with it before recursing, so one board
    /// serves the whole search rather than one per depth.
    sim: Board,
    /// Scratch for [`Search::visited_placeable_tiles`]: per tile, which
    /// walkers arrived there ([`LEFT_CLAW`], [`RIGHT_CLAW`], [`GULL`]),
    /// resized to the board and refilled per call.
    visited: Vec<u8>,
    /// Candidate-tile buffers, handed back here as each level unwinds.
    ///
    /// This one does need a buffer per depth: `run` reads its candidates
    /// for as long as it is looping over them, and that loop spans its
    /// recursive calls. The depth is the level's inventory, so the pool
    /// settles at a handful of buffers and stops growing.
    tile_pool: Vec<Vec<(u8, u8, PlayerId)>>,
    /// Scratch for the memo key, so that a node the memo already holds
    /// costs a lookup and nothing else. Only a node that is new to the memo
    /// allocates, and that allocation is the memo's own entry.
    key: Vec<(u8, u8, u8, u8)>,
}

/// A left-clawed crab arrived at the tile ([`Search::visited`]).
const LEFT_CLAW: u8 = 1;
/// A right-clawed crab arrived at the tile.
const RIGHT_CLAW: u8 = 2;
/// A gull arrived at the tile, which every arrow turns.
const GULL: u8 = 4;

/// The walkers an arrow of `seat`'s turns on `board`, as [`Search::visited`]
/// flags: everyone, unless claw-keyed arrows give it one claw's crabs and
/// the gulls.
fn turned_by(board: &Board, seat: PlayerId) -> u8 {
    match (board.rules().claw_hands, crate::sim::board::claw_of(seat)) {
        (Some(_), Some(Handedness::Left)) => LEFT_CLAW | GULL,
        (Some(_), Some(Handedness::Right)) => RIGHT_CLAW | GULL,
        (Some(_), None) => 0,
        (None, _) => LEFT_CLAW | RIGHT_CLAW | GULL,
    }
}

impl<'a> Search<'a> {
    fn new(level: &'a Level, effort: Effort) -> Self {
        Search {
            level,
            fuel: match effort {
                Effort::Budget(nodes) => Some(nodes),
                Effort::Exhaustive => None,
            },
            gave_up: false,
            seen: std::collections::HashSet::new(),
            placed: Vec::new(),
            sim: level.board(),
            visited: Vec::new(),
            tile_pool: Vec::new(),
            key: Vec::new(),
        }
    }

    /// Charge one simulation to the budget. Once it returns false it returns
    /// false forever, and every caller unwinds without simulating again: a
    /// spent search must stop promptly, not merely stop eventually.
    fn charge(&mut self) -> bool {
        if self.gave_up {
            return false;
        }
        if let Some(fuel) = self.fuel.as_mut() {
            match fuel.checked_sub(1) {
                Some(left) => *fuel = left,
                None => {
                    self.gave_up = true;
                    return false;
                }
            }
        }
        true
    }

    /// Does this exact board win on its own, by the level's own reckoning?
    ///
    /// The judge is [`Level::outcome`], the same one the game uses, rather
    /// than a copy of the all-crabs rule: a stage may ask for a number
    /// banked, or for nobody eaten, and a solver that only knows how
    /// to bank every crab reports "no solution" for beatable stages.
    fn wins(&mut self, board: &Board) -> bool {
        if !self.charge() {
            return false;
        }
        self.sim.copy_from(board);
        self.level.play_out(&mut self.sim).0 == PuzzleOutcome::Won
    }

    /// Tiles a creature an arrow would turn arrives at during a run of the
    /// current board, with the seat whose arrow it would be: the only
    /// places a new signpost could matter. Gulls count too: a solution may
    /// hinge on steering a gull away from the crabs. Under claw-keyed
    /// arrows a tile only left-clawed crabs cross is a candidate for P1's
    /// hand alone, since P2's arrow there turns nobody. Restricted to
    /// empty, signpost-free tiles (the only legal placements), and to seats
    /// with a hand to place.
    ///
    /// Answers into `tiles` rather than into a fresh `Vec`, so that the
    /// caller can lend the same buffer to node after node.
    fn visited_placeable_tiles(&mut self, board: &Board, tiles: &mut Vec<(u8, u8, PlayerId)>) {
        tiles.clear();
        if !self.charge() {
            return;
        }
        self.sim.copy_from(board);
        self.visited.clear();
        self.visited
            .resize(self.sim.width() as usize * self.sim.height() as usize, 0);
        for _ in 0..Level::deadline(&self.sim) {
            self.sim.tick_idle();
            for crab in self.sim.crabs() {
                self.visited[crab.tile as usize] |= match crab.handed {
                    Handedness::Left => LEFT_CLAW,
                    Handedness::Right => RIGHT_CLAW,
                };
            }
            for gull in self.sim.gulls() {
                self.visited[gull.tile as usize] |= GULL;
            }
            // Once the goal is decided, by the same judge `wins` plays to,
            // nothing later matters: a post only acts on what arrives at
            // it, so a tile first reached after the decision cannot change
            // it. A quiet beach is not a decision on a stage that plays on
            // (a hole spawns more, a stage banks a number or holds out to
            // the clock), and scanning on to the deadline
            // after a win or a loss is wasted ticks on every node.
            if self.level.outcome(&self.sim) != PuzzleOutcome::Running {
                break;
            }
        }
        for (x, y, kind) in board.tiles() {
            let arrived = self.visited[usize::from(board.index_of(x, y))];
            if arrived == 0 || kind != TileKind::Empty || board.signpost_at(x, y).is_some() {
                continue;
            }
            for &seat in placing_seats(board) {
                if arrived & turned_by(board, seat) != 0 && board.signpost_cap_of(seat) > 0 {
                    tiles.push((x, y, seat));
                }
            }
        }
    }

    /// One whole search at a fixed inventory size.
    ///
    /// Within a single call, "how many are placed" and "how many are left"
    /// add to a constant, so the placed set alone identifies a node and the
    /// memo is exact. Across calls it is not, so the memo starts empty.
    fn search_at(&mut self, board: &mut Board, depth: u8) -> Option<Vec<Placement>> {
        self.seen.clear();
        self.placed.clear();
        self.run(board, depth)
    }

    fn run(&mut self, board: &mut Board, depth: u8) -> Option<Vec<Placement>> {
        // Somewhere we have already been, by another road. Nothing about
        // the board depends on how it was reached, so neither does the
        // answer. The leaves are asked too, and they are the ones that
        // matter: a set of `d` posts has up to `d` parents, each playing the
        // full board out again.
        self.key.clear();
        self.key.extend(
            self.placed
                .iter()
                .map(|&(x, y, dir, seat)| (x, y, dir.id(), seat)),
        );
        self.key.sort_unstable();
        if self.seen.contains(&self.key) {
            return None;
        }
        self.seen.insert(self.key.clone());
        if depth == 0 {
            return self.wins(board).then(Vec::new);
        }
        // Borrowed for the whole loop, recursive calls included, and handed
        // back at the end: see `tile_pool`.
        let mut tiles = self.tile_pool.pop().unwrap_or_default();
        self.visited_placeable_tiles(board, &mut tiles);
        let mut found = None;
        'candidates: for &(x, y, seat) in &tiles {
            for dir in Direction::ALL {
                if self.gave_up {
                    break 'candidates;
                }
                if !board.place_signpost(seat, x, y, dir) {
                    continue;
                }
                self.placed.push((x, y, dir, seat));
                if let Some(mut placements) = self.run(board, depth - 1) {
                    placements.push((x, y, dir, seat));
                    found = Some(placements);
                    break 'candidates;
                }
                self.placed.pop();
                board.remove_signpost(seat, x, y);
            }
        }
        tiles.clear();
        self.tile_pool.push(tiles);
        found
    }
}

/// What a co-op stage was tried under, beside its own two hands: each the
/// same beach with its arrows dealt another way ([`Level::with_hands`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deal {
    /// One arrow fewer in this seat's hand: the hand is minimal when this
    /// loses.
    Short(PlayerId),
    /// Every arrow in this seat's hand and none in the other's: one
    /// player alone. The stage needs both when this loses for both seats.
    Alone(PlayerId),
    /// As many arrows, each turning every crab: the stage as a solo puzzle
    /// with a quota. It needs the claw split when this loses.
    Ordinary,
}

impl Deal {
    /// Every way a co-op stage of `hands` is tried, in the order the
    /// guards ask: the hands short first, then one player alone, then
    /// ordinary arrows. A hand already empty has no shorter version.
    pub fn all(hands: [u8; 2]) -> Vec<Deal> {
        let mut deals: Vec<Deal> = (0..2)
            .filter(|&seat| hands[usize::from(seat)] > 0)
            .map(Deal::Short)
            .collect();
        deals.extend([Deal::Alone(0), Deal::Alone(1), Deal::Ordinary]);
        deals
    }

    /// `level`, a co-op stage, with its arrows dealt this way.
    pub fn level(self, level: &Level) -> Level {
        let hands = level.hands().unwrap_or([level.posts, 0]);
        let total = hands[0].saturating_add(hands[1]);
        match self {
            Deal::Short(seat) => {
                let mut short = hands;
                short[usize::from(seat)] = short[usize::from(seat)].saturating_sub(1);
                level.with_hands(Some(short), 0)
            }
            Deal::Alone(0) => level.with_hands(Some([total, 0]), 0),
            Deal::Alone(_) => level.with_hands(Some([0, total]), 0),
            Deal::Ordinary => level.with_hands(None, total),
        }
    }
}

/// Whether a co-op stage needs both players: tried with every arrow in
/// P1's hand and then in P2's, and won by neither. `Found` names a deal
/// one player wins alone, with its placements; `Unsolvable` is the proof
/// that the stage needs two; `GaveUp` says the budget ran out first.
pub fn needs_both(level: &Level, effort: Effort) -> SolveOutcome {
    let mut gave_up = false;
    for seat in [0, 1] {
        match solve_with(&Deal::Alone(seat).level(level), effort) {
            found @ SolveOutcome::Found(_) => return found,
            SolveOutcome::Unsolvable => {}
            SolveOutcome::GaveUp => gave_up = true,
        }
    }
    match gave_up {
        true => SolveOutcome::GaveUp,
        false => SolveOutcome::Unsolvable,
    }
}

/// Convenience: validate a level exhaustively, returning the solution found
/// (also checks an authored level's claim that it is solvable).
pub fn validate(level: &Level) -> Result<Vec<Placement>, String> {
    match solve_with(level, Effort::Exhaustive) {
        SolveOutcome::Found(placements) => Ok(placements),
        SolveOutcome::Unsolvable => Err(format!("no solution within {} signposts", level.posts)),
        // Unreachable by construction: an exhaustive search has no budget to
        // run out of. Spelled out rather than waved through, so that adding a
        // second way to stop early cannot silently become "no solution".
        SolveOutcome::GaveUp => Err("search gave up".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_reports_solvability_either_way() {
        let solvable = campaign_levels().remove(1);
        assert!(validate(&solvable).is_ok());
        // A castle sealed on all four sides cannot be solved.
        let text = "name: No\nposts: 1\ncrab: 0,0 R L common\nmap:\n\
+-+-+-+\n|. . .|\n+ +-+ +\n|.|0|.|\n+ +-+ +\n|. . .|\n+-+-+-+\n";
        let level = Level::parse(text).expect("parses");
        let err = validate(&level).unwrap_err();
        assert!(err.contains("no solution"), "{err}");
    }
    use crate::sim::campaign_levels;
    use crate::sim::level::PuzzleOutcome;

    /// A search that starts past the counts already proved short finds
    /// the same fewest signposts for a fraction of the work: the editor's
    /// second search past a grant, which repeated the first search's
    /// counts and cost it many times over.
    #[test]
    fn a_search_from_a_count_skips_the_counts_below_it() {
        let level = campaign_levels()
            .into_iter()
            .find(|level| level.posts >= 2)
            .expect("a campaign stage of two signposts or more");
        let budget = DEFAULT_NODE_BUDGET * 4;
        let spent = |fewest: u8| {
            let mut search = Search::new(&level, Effort::Budget(budget));
            let mut found = None;
            for depth in fewest..=level.posts {
                let mut board = level.board();
                if let Some(placements) = search.search_at(&mut board, depth) {
                    found = Some(placements.len());
                    break;
                }
            }
            (found, budget - search.fuel.unwrap_or(0))
        };
        let (all, all_cost) = spent(0);
        let (past, past_cost) = spent(level.posts);
        assert_eq!(all, Some(usize::from(level.posts)), "{}", level.name);
        assert_eq!(past, all);
        assert!(past_cost < all_cost, "{past_cost} vs {all_cost}");
        let SolveOutcome::Found(placements) =
            solve_from(&level, Effort::Budget(budget), level.posts)
        else {
            panic!("{} wins at its grant", level.name);
        };
        assert_eq!(placements.len(), usize::from(level.posts));
    }

    #[test]
    fn solver_cracks_an_early_campaign_level() {
        // Level 2, "First Turn": one post, one crab.
        let levels = campaign_levels();
        let level = &levels[1];
        let SolveOutcome::Found(solution) = solve(level) else {
            panic!("level 2 is solvable");
        };
        assert!(solution.len() <= level.posts as usize);
        // Replay the found solution to be sure.
        let mut board = level.board();
        for &(x, y, dir, seat) in &solution {
            assert!(board.place_signpost(seat, x, y, dir));
        }
        let (outcome, _) = level.play_out(&mut board);
        assert_eq!(
            outcome,
            PuzzleOutcome::Won,
            "solver's answer must actually win"
        );
    }

    /// Under Evict an inventory larger than the cap buys nothing before the
    /// first tick: the posts past the cap only push out the oldest. The
    /// search stops at the cap, where its memo of placement sets is still
    /// exact, and what it finds there replays to a win.
    #[test]
    fn an_inventory_past_an_evicting_cap_is_searched_only_to_the_cap() {
        let text = "\
name: Evicting
posts: 3
rule: evict 1
crab: 0,3 R L common
map:
+-+-+-+-+-+
|. . . . .|
+ + + + + +
|. . 0 . .|
+ + + + + +
|. . . . .|
+ + + + + +
|. . . . .|
+-+-+-+-+-+
";
        let level = Level::parse(text).expect("parses");
        let SolveOutcome::Found(solution) = solve_with(&level, Effort::Exhaustive) else {
            panic!("one post turns the crab home");
        };
        assert_eq!(solution.len(), 1, "{solution:?}");
        let mut board = level.board();
        for &(x, y, dir, seat) in &solution {
            assert!(board.place_signpost(seat, x, y, dir));
        }
        assert_eq!(level.play_out(&mut board).0, PuzzleOutcome::Won);

        // And a hopeless one says so without walking every ordering of
        // three posts that a cap of one would have thinned to the last.
        let sealed = text.replace("|. . 0 . .|", "|. .|0|. .|").replace(
            "+ + + + + +\n|. .|0|. .|\n+ + + + + +",
            "+ + +-+ + +\n|. .|0|. .|\n+ + +-+ + +",
        );
        let level = Level::parse(&sealed).expect("parses");
        assert_eq!(
            solve_with(&level, Effort::Exhaustive),
            SolveOutcome::Unsolvable
        );
    }

    #[test]
    fn unsolvable_level_returns_none() {
        // A crab orbiting the border with zero posts and an unreachable
        // interior castle: provably unsolvable.
        let text = "\
name: Hopeless
posts: 0
crab: 0,0 R L common
map:
+-+-+-+-+-+
|. . . . .|
+ +-+-+-+ +
|. .|0|. .|
+ +-+-+-+ +
|. . . . .|
+-+-+-+-+-+
";
        let level = Level::parse(text).expect("parses");
        assert_eq!(solve(&level), SolveOutcome::Unsolvable);
    }

    /// The distinction the budget exists to make: the same hopeless board
    /// answers "no solution" when the search is allowed to finish, and "gave
    /// up" when it is not. Both are honest; only the first is a claim about
    /// the level.
    #[test]
    fn a_spent_budget_gives_up_rather_than_claiming_unsolvable() {
        let text = "\
name: Hopeless
posts: 2
crab: 0,0 R L common
map:
+-+-+-+-+-+
|. . . . .|
+ +-+-+-+ +
|. .|0|. .|
+ +-+-+-+ +
|. . . . .|
+-+-+-+-+-+
";
        let level = Level::parse(text).expect("parses");
        assert_eq!(
            solve_with(&level, Effort::Exhaustive),
            SolveOutcome::Unsolvable
        );
        assert_eq!(solve_with(&level, Effort::Budget(4)), SolveOutcome::GaveUp);
    }

    /// Two hands holding more between them than a `u8` counts, as a
    /// pasted level can hand the editor's check, are searched up to the
    /// grant rather than overflowing the count of room on the board.
    #[test]
    fn two_hands_past_a_u8_do_not_overflow_the_room() {
        let text = "\
name: Big Hands
posts: 1
crab: 0,0 R L common
map:
+-+-+-+
|. . 0|
+-+-+-+
";
        let level = Level::parse(text)
            .expect("parses")
            .with_hands(Some([250, 9]), 0);
        assert!(matches!(
            solve_with(&level, Effort::Budget(64)),
            SolveOutcome::Found(_)
        ));
    }

    /// A beach can fall quiet and fill again: on a stage that banks a
    /// number, the tiles the next crab out of the hole walks are still
    /// places a post could matter. Here the turnstile sends the first crab
    /// home and the second down to (1,2), after a spell with no crab on the
    /// beach at all, where the scan used to stop.
    #[test]
    fn a_quiet_spell_does_not_end_the_scan_on_a_stage_that_plays_on() {
        let text = "\
name: Second Wave
posts: 1
rule: evict 3
round: 600
goal: bank 5
spawner: 0,1 R 150
map:
+-+-+-+
|. 0 .|
+ + + +
|. t .|
+ + + +
|. . .|
+-+-+-+
";
        let level = Level::parse(text).expect("parses");
        let mut search = Search::new(&level, Effort::Exhaustive);
        let mut tiles = Vec::new();
        search.visited_placeable_tiles(&level.board(), &mut tiles);
        assert!(
            tiles.contains(&(1, 2, 0)),
            "the second crab's tile: {tiles:?}"
        );
    }

    /// The scan ends where the goal is decided, not at the deadline: once
    /// the first crab banks a stage asking for one, the second crab's walk
    /// down to (1,2) cannot change anything, and playing it out costs every
    /// node of the editor's search the rest of the round.
    #[test]
    fn the_scan_stops_once_the_goal_is_decided() {
        let text = "\
name: First Wave
posts: 1
rule: evict 3
round: 600
goal: bank 1
spawner: 0,1 R 150
map:
+-+-+-+
|. 0 .|
+ + + +
|. t .|
+ + + +
|. . .|
+-+-+-+
";
        let level = Level::parse(text).expect("parses");
        let mut search = Search::new(&level, Effort::Exhaustive);
        let mut tiles = Vec::new();
        search.visited_placeable_tiles(&level.board(), &mut tiles);
        assert!(!tiles.contains(&(1, 2, 0)), "past the decision: {tiles:?}");
        assert!(search.sim.ticks() < Level::deadline(&search.sim));
    }

    /// A budget must not cost correctness on a board it can afford: one
    /// simulation is enough to see that a level already won needs nothing.
    #[test]
    fn a_budget_large_enough_still_finds_the_answer() {
        let level = &campaign_levels()[1];
        assert_eq!(
            solve_with(level, Effort::Budget(DEFAULT_NODE_BUDGET)),
            solve_with(level, Effort::Exhaustive),
        );
    }
}
