//! Brute-force puzzle validation (spec §5.4): search signpost placements
//! against the headless sim until one meets the level's goal.
//!
//! The search is depth-first over placement counts 0..=inventory. Candidate
//! tiles are pruned to tiles crabs actually cross in a run under the current
//! placements, since a signpost nobody walks over cannot change the
//! outcome, and the candidate set is recomputed at each depth so placements
//! that open new paths are still found.

use crate::sim::board::{Board, TileKind};
use crate::sim::direction::Direction;
use crate::sim::level::{Level, PuzzleOutcome};

/// A signpost as an instruction: where it goes and which way it points.
///
/// The same triple is a solution step, a hint, and a line in a level file,
/// so it is one name rather than three spellings.
pub type Placement = (u8, u8, Direction);

/// Boards a budgeted search may simulate before it gives up.
///
/// Counted in simulations rather than seconds, so a board costs the same
/// budget on every machine and in every build profile. Each simulation is
/// itself bounded by the board's [`Level::deadline`], which makes this a
/// ceiling on total work rather than merely on visits.
///
/// Sized from both ends, and both ends were measured.
///
/// The ceiling is there for the boards nobody vetted: an author's own level,
/// which can be any size, any shape, and unsolvable in ways that take far
/// longer to prove than to draw. A 12x9 board with six crabs, sealed so that
/// no solution exists, ran past ten minutes unbudgeted without an answer. At
/// this ceiling that same board gives up in 48 seconds of a release build,
/// measured 2026-08-15. The editor validates on a background thread and says
/// it is working, so that is a wait rather than a freeze.
///
/// Raised from 50,000 the same day, and the reason is the campaign rather
/// than the editor. `no_campaign_level_grants_a_post_it_does_not_need`
/// proves a level's inventory minimal under this number, and proving that no
/// *three*-post answer exists costs roughly the cube of the tiles the crabs
/// cross. At 50,000 that proof consumed the whole budget on any board big
/// enough to need four posts, so four-post levels could not be shown minimal
/// and therefore could not ship: of 140 boards built to need one, 101 gave
/// up and none came back with four. The old ceiling was not protecting the
/// editor from slow boards so much as capping how hard a level was allowed
/// to be.
///
/// Raised again to 1,000,000 on 2026-08-16, and again the campaign asked
/// for it. Gulls started eating the crabs they met, which makes a placement
/// fail later and deeper, and four gull levels stopped fitting: 400,000 for
/// The Long Shelf, 600,000 for Quick Feet, a round million for Slow And
/// Sure and The Far Corner. Six to twelve seconds each on this machine, on
/// the editor's background thread, which is a wait on a button you press
/// deliberately.
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
/// The distinction is what a budget is for. "No solution" is a claim
/// about the level; "gave up" is a claim about the search, and an editor that
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
    let mut search = Search::new(level, effort);
    for depth in 0..=level.posts {
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
    /// that placing B then A leaves. The search walks orderings, so without
    /// this it explores every one of them, and there are `depth!` of those.
    /// Six wasted subtrees out of seven at three posts, twenty-three out of
    /// twenty-four at four. That factorial is what kept four-post levels out
    /// of reach of any budget worth having.
    ///
    /// Ordering the candidates instead would be cheaper still and is not
    /// sound here: the candidate set is recomputed at each depth from the
    /// board as it now stands, so a tile can become worth trying only
    /// *because* an earlier signpost sent a crab across it. Refusing to go
    /// back would lose those. Remembering where we have been loses nothing.
    ///
    /// Cleared between depths, and that is not housekeeping. The search
    /// runs once per inventory size, and a set that failed with one post
    /// left to spend says nothing about the same set with two. Carrying the
    /// memo across those runs made `Both Lanes` unsolvable, which is what
    /// the minimality guard is for.
    /// Keyed by `(x, y, Direction::id)` so the set can be sorted and
    /// hashed without asking a sim type to grow orderings it has no other
    /// use for.
    seen: std::collections::HashSet<Vec<(u8, u8, u8)>>,
    /// The signposts standing right now, in placement order.
    placed: Vec<Placement>,
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
    /// than a copy of the all-crabs rule: a Beach Day stage asks for a number
    /// banked, or for nobody eaten, and a solver that only knows how to bank
    /// every crab answers a harder question than it was asked, reporting "no
    /// solution" for stages that are perfectly beatable.
    fn wins(&mut self, board: &Board) -> bool {
        if !self.charge() {
            return false;
        }
        let mut sim = board.clone();
        self.level.play_out(&mut sim).0 == PuzzleOutcome::Won
    }

    /// Tiles any creature arrives at during a run of the current board: the
    /// only places a new signpost could matter. Gulls count too: a solution
    /// may hinge on steering a gull away from the crabs. Restricted to empty,
    /// signpost-free tiles (the only legal placements).
    fn visited_placeable_tiles(&mut self, board: &Board) -> Vec<(u8, u8)> {
        if !self.charge() {
            return Vec::new();
        }
        let mut sim = board.clone();
        let mut seen = vec![false; sim.width() as usize * sim.height() as usize];
        for _ in 0..Level::deadline(&sim) {
            sim.tick_idle();
            for crab in sim.crabs() {
                seen[crab.tile as usize] = true;
            }
            for gull in sim.gulls() {
                seen[gull.tile as usize] = true;
            }
            if sim.crabs().is_empty() {
                break;
            }
        }
        let mut tiles = Vec::new();
        for (x, y, kind) in board.tiles() {
            if seen[usize::from(board.index_of(x, y))]
                && kind == TileKind::Empty
                && board.signpost_at(x, y).is_none()
            {
                tiles.push((x, y));
            }
        }
        tiles
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
        if depth == 0 {
            return self.wins(board).then(Vec::new);
        }
        // Somewhere we have already been, by another road. Nothing about the
        // board depends on how it was reached, so neither does the answer.
        let mut key: Vec<(u8, u8, u8)> = self
            .placed
            .iter()
            .map(|(x, y, dir)| (*x, *y, dir.id()))
            .collect();
        key.sort_unstable();
        if !self.seen.insert(key) {
            return None;
        }
        for (x, y) in self.visited_placeable_tiles(board) {
            for dir in Direction::ALL {
                if self.gave_up {
                    return None;
                }
                if !board.place_signpost(0, x, y, dir) {
                    continue;
                }
                self.placed.push((x, y, dir));
                if let Some(mut placements) = self.run(board, depth - 1) {
                    placements.push((x, y, dir));
                    return Some(placements);
                }
                self.placed.pop();
                board.remove_signpost(0, x, y);
            }
        }
        None
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
        let solvable = crate::sim::campaign_levels().remove(1);
        assert!(validate(&solvable).is_ok());
        // A castle sealed on all four sides cannot be solved.
        let text = "name: No\nposts: 1\ncrab: 0,0 R L common\nmap:\n\
+-+-+-+\n|. . .|\n+ +-+ +\n|.|0|.|\n+ +-+ +\n|. . .|\n+-+-+-+\n";
        let level = crate::sim::Level::parse(text).expect("parses");
        let err = validate(&level).unwrap_err();
        assert!(err.contains("no solution"), "{err}");
    }
    use crate::sim::campaign_levels;
    use crate::sim::level::PuzzleOutcome;

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
        for &(x, y, dir) in &solution {
            assert!(board.place_signpost(0, x, y, dir));
        }
        let (outcome, _) = level.play_out(&mut board);
        assert_eq!(
            outcome,
            PuzzleOutcome::Won,
            "solver's answer must actually win"
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
