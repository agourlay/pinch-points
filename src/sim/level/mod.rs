//! Text level format and puzzle rules (spec §5.1).
//!
//! A level file has key/value header lines, then a `map:` section, e.g.:
//!
//! ```text
//! name: First Steps
//! posts: 1
//! crab: 0,1 R L common
//! solution: 3,1 U
//! map:
//! +-+-+-+-+
//! |. . . 0|
//! + +-+ + +
//! |. .|. .|
//! +-+-+-+-+
//! ```
//!
//! The map is a half-resolution lattice: tile `(x, y)` sits at lattice
//! `(2x+1, 2y+1)`; the character between two tiles is their shared wall
//! (`-`/`|` wall, anything else open). Tile characters: `.` sand, `#` rock,
//! `0`–`3` castle of that player. Crabs and spawners are header lines, not
//! map characters, because they carry more data than one character holds:
//! `crab: x,y DIR HAND KIND` and `spawner: x,y DIR period`.
//!
//! Puzzles are played with a fixed signpost inventory (`posts:`) under
//! `CapPolicy::Reject`, and won when every crab has banked. `solution:` lines
//! are machine-checked by tests, so a level that ships is a level that is
//! solvable.
//!
//! `kind: puzzle|arena` says which of the two a level is (see [`LevelKind`]),
//! and so which list it joins. Files written before the editor had that
//! toggle leave it out, and are read off their castles.

mod format;

use crate::sim::board::{Board, CapPolicy};
use crate::sim::solve::Placement;

/// Sim ticks a puzzle may run before it counts as failed (60 s at 30 Hz).
/// Generous: it exists to catch crabs orbiting forever, not to add pressure.
pub const PUZZLE_TICK_LIMIT: u64 = 60 * crate::sim::TICKS_PER_SECOND as u64;

#[derive(Clone, Debug)]
pub struct Level {
    pub name: String,
    /// Signpost inventory for the puzzle.
    pub posts: u8,
    /// One known solution; validated by tests.
    pub solution: Vec<Placement>,
    pub goal: Goal,
    /// What the level is for: a stage to solve alone, or a beach to fight
    /// over. The author chooses it in the editor and it decides which of
    /// the two lists the level joins.
    pub kind: LevelKind,
    board: Board,
    crab_count: u32,
    /// True when the level text carried an explicit `rule:` line (versus
    /// replays); otherwise `board()` applies puzzle rules from `posts`.
    explicit_rule: bool,
}

/// What a level was built to be.
///
/// The two want opposite things of a beach: a puzzle wants one bank and a
/// route to it, an arena wants a castle per seat and crabs arriving forever.
/// A file that does not say which it is has to be guessed at, and guessing
/// put a versus beach in the middle of the campaign and kept a spawner-fed
/// arena off the map dial for want of a starting crab.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LevelKind {
    /// A stage for one player: route every crab home with the signposts the
    /// level grants. Joins the Tide Pool list.
    #[default]
    Puzzle,
    /// A beach for a match: a castle for each seat. Joins the map dial in
    /// Turf War and the lobby.
    Arena,
}

impl LevelKind {
    pub fn token(self) -> &'static str {
        match self {
            LevelKind::Puzzle => "puzzle",
            LevelKind::Arena => "arena",
        }
    }

    pub fn from_token(token: &str) -> Option<LevelKind> {
        match token {
            "puzzle" => Some(LevelKind::Puzzle),
            "arena" => Some(LevelKind::Arena),
            _ => None,
        }
    }
}

/// What a stage asks of the player. `AllCrabs` is classic puzzle mode; the
/// rest are Beach Day challenge goals (the original's Stage Challenge,
/// re-themed).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Goal {
    /// Every crab that ever exists must bank (spec §5.1).
    AllCrabs,
    /// Bank at least this many crabs before the tide.
    Bank(u32),
    /// No crab may be eaten before the tide comes in.
    Survive,
    /// Bank a golden crab before the tide.
    Golden,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PuzzleOutcome {
    Running,
    Won,
    /// Some crab is still loose at the tick limit.
    Lost,
}

impl Level {
    /// A fresh board for this level. Puzzle levels get fixed-inventory rules
    /// from `posts`; levels carrying an explicit `rule:` (recorded versus
    /// matches) keep the rule the match was played under.
    ///
    /// An arena that states no rule keeps the board's own, which is the
    /// versus one. Reading `posts:` as a rule there gave a table three
    /// signposts each and no way to replace them - the file says how many
    /// the *author* had to hand, not how a match is played on it.
    pub fn board(&self) -> Board {
        let mut board = self.board.clone();
        if !self.explicit_rule && self.kind == LevelKind::Puzzle {
            board.set_signpost_rule(self.posts, CapPolicy::Reject);
            // A campaign puzzle's castle is the finish line, not a bank to
            // be robbed: see `Board::castle_raids`. Beach Day states `rule:`
            // and keeps its raids - there the goal is a number banked and a
            // robbery is drama, not a target that moves.
            board.set_castle_raids(false);
        }
        board
    }

    pub fn crab_count(&self) -> u32 {
        self.crab_count
    }

    /// Say what the level is for. The editor's toggle is authoritative: a
    /// level saved as a puzzle stays one however many castles it grew.
    pub fn with_kind(mut self, kind: LevelKind) -> Level {
        self.kind = kind;
        self
    }

    /// How many seats the beach could hold: one castle each, counted by
    /// owner. What the map dial checks a handmade beach against, and what
    /// tells a level with no `kind:` line of its own what it must be.
    pub fn seats(&self) -> u8 {
        self.board.castle_seats()
    }

    /// Win/loss state of a board created by [`Level::board`], judged by the
    /// level's goal. All goals use banked-count accounting (spec §5.1: a
    /// crab a gull ate is a loss for AllCrabs/Survive, never a quiet
    /// disappearance).
    /// Run `board` idle until the goal is decided, and say when it was.
    ///
    /// Every caller that wants to know how a board ends wants exactly this
    /// loop, and ten of them once carried their own copy. [`Level::outcome`]
    /// is what makes it terminate: every goal loses or wins by
    /// `PUZZLE_TICK_LIMIT`, so the loop needs no guard of its own, and if
    /// that ever moves there is one place to put it.
    pub fn play_out(&self, board: &mut Board) -> (PuzzleOutcome, u64) {
        loop {
            board.tick_idle();
            match self.outcome(board) {
                PuzzleOutcome::Running => {}
                done @ (PuzzleOutcome::Won | PuzzleOutcome::Lost) => return (done, board.ticks()),
            }
        }
    }

    /// Place the authored solution on `board` for seat 0. `Err` names the
    /// first placement the board refused, which on a shipped level is a
    /// broken level file and never something to shrug off.
    pub fn place_solution(&self, board: &mut Board) -> Result<(), Placement> {
        for &(x, y, dir) in &self.solution {
            if !board.place_signpost(0, x, y, dir) {
                return Err((x, y, dir));
            }
        }
        Ok(())
    }

    /// A fresh board with the authored solution standing on it.
    pub fn solved_board(&self) -> Result<Board, Placement> {
        let mut board = self.board();
        self.place_solution(&mut board)?;
        Ok(board)
    }

    pub fn outcome(&self, board: &Board) -> PuzzleOutcome {
        let alive = board.crabs().len() as u32;
        let eaten = board.crabs_banked() + alive < board.crabs_spawned();
        let timed_out = board.round_over() || board.ticks() >= PUZZLE_TICK_LIMIT;
        // What each goal counts as already lost, and as already won. Losing
        // is checked first: a goal met on the same tick a crab was eaten does
        // not save an AllCrabs or Survive stage.
        let (lost, won) = match self.goal {
            Goal::AllCrabs => (
                eaten,
                alive == 0 && board.crabs_banked() == board.crabs_spawned(),
            ),
            Goal::Bank(n) => (false, board.crabs_banked() >= n),
            // Survive is the one goal the clock is *for*: running it out is
            // the win. The tick limit stands in when a stage has no timer,
            // which would otherwise never end.
            Goal::Survive => (eaten, timed_out),
            Goal::Golden => (false, board.golden_banked() >= 1),
        };
        if lost {
            PuzzleOutcome::Lost
        } else if won {
            PuzzleOutcome::Won
        } else if timed_out {
            // Every other goal loses when the tide beats it to the finish.
            PuzzleOutcome::Lost
        } else {
            PuzzleOutcome::Running
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::direction::Direction;

    #[test]
    fn parse_rejects_bad_tile_chars() {
        let text = "name: Bad\nposts: 1\nmap:\n+-+\n|X|\n+-+\n";
        let err = super::Level::parse(text).unwrap_err();
        assert!(err.contains("bad tile char"), "{err}");
    }

    #[test]
    fn parse_rejects_unknown_keys() {
        let text = "name: Bad\nwibble: 3\nmap:\n+-+\n|.|\n+-+\n";
        let err = super::Level::parse(text).unwrap_err();
        assert!(err.contains("unknown key"), "{err}");
    }

    #[test]
    fn parse_rejects_malformed_solutions_and_directions() {
        let text = "name: Bad\nposts: 1\nsolution: 0,0 Q\nmap:\n+-+\n|.|\n+-+\n";
        assert!(super::Level::parse(text).is_err());
        let text = "name: Bad\nposts: 1\nsolution: zero,0 U\nmap:\n+-+\n|.|\n+-+\n";
        assert!(super::Level::parse(text).is_err());
    }

    /// A degenerate lattice is refused, not asserted against. `parse` is
    /// fallible because it reads text a stranger wrote (a pasted level
    /// code, a hand-edited custom level) and both callers handle the
    /// error. Odd-sized alone let a lone border line through as a
    /// zero-sized board, which `Board::new` met with a panic.
    #[test]
    fn parse_rejects_a_map_with_no_tiles() {
        for text in [
            "name: Bad\nposts: 1\nmap:\n+-+-+\n",   // one row of border
            "name: Bad\nposts: 1\nmap:\n+\n|\n+\n", // one column of border
            "name: Bad\nposts: 1\nmap:\n+\n",
        ] {
            let err = super::Level::parse(text).unwrap_err();
            assert!(err.contains("at least one tile"), "{text:?} gave {err}");
        }
    }

    /// A board dimension is a `u8`, and the cast to one used to wrap: a
    /// 300-tile lattice loaded as a 44-tile beach, silently not the level
    /// the text described.
    #[test]
    fn parse_rejects_a_map_too_wide_to_name() {
        let border: String = "+-".repeat(300) + "+";
        let row: String = "|.".repeat(300) + "|";
        let text = format!("name: Vast\nposts: 1\nmap:\n{border}\n{row}\n{border}\n");
        let err = super::Level::parse(&text).unwrap_err();
        assert!(err.contains("300"), "{err}");
        // And the widest board that *does* fit still parses.
        let border: String = "+-".repeat(255) + "+";
        let row: String = "|.".repeat(255) + "|";
        let text = format!("name: Wide\nposts: 1\nmap:\n{border}\n{row}\n{border}\n");
        let level = super::Level::parse(&text).expect("255 tiles is nameable");
        assert_eq!(level.board.width(), 255);
    }

    /// Short rows pad with spaces by design: editors trim trailing
    /// whitespace, and wrap maps end rows with spaces (open edges), so a
    /// strict width check would reject to_text's own trimmed output.
    #[test]
    fn parse_pads_short_map_rows() {
        let text = "name: Trimmed\nposts: 1\nmap:\n+-+-+\n|.\n+-+-+\n";
        let level = super::Level::parse(text).expect("pads, never panics");
        assert_eq!(level.board.width(), 2);
    }

    use super::*;
    use crate::sim::board::TileKind;

    /// A turnstile's pivot survives the format: a replay stores its starting
    /// board as a level, and generated arenas mirror their logs, so a dropped
    /// pivot would flip half of them on every recorded round.
    #[test]
    fn a_turnstiles_pivot_survives_the_format() {
        for next_right in [true, false] {
            let mut board = Board::new(4, 3, 1);
            board.set_tile(1, 1, TileKind::Turnstile { next_right });
            let text = Level::from_board("Pivot", 1, board).to_text();
            let parsed = Level::parse(&text).expect("own output").board();
            assert_eq!(
                parsed.tile_at(1, 1),
                TileKind::Turnstile { next_right },
                "next_right {next_right} was not preserved:\n{text}"
            );
        }
    }

    const TINY: &str = "\
name: Tiny
posts: 1
crab: 0,0 R L common
solution: 2,0 D
map:
+-+-+-+
|. . .|
+ +-+ +
|. . 0|
+-+-+-+
";

    #[test]
    fn terrain_tiles_round_trip() {
        let mut board = Board::new(4, 3, 7);
        board.set_tile(1, 1, TileKind::Turnstile { next_right: true });
        board.set_tile(2, 1, TileKind::Kelp);
        board.set_tile(3, 1, TileKind::Pool);
        let text = Level::from_board("Terrain", 2, board).to_text();
        let level = Level::parse(&text).expect("parses");
        assert_eq!(
            level.board.tile_at(1, 1),
            TileKind::Turnstile { next_right: true }
        );
        assert_eq!(level.board.tile_at(2, 1), TileKind::Kelp);
        assert_eq!(level.board.tile_at(3, 1), TileKind::Pool);
        assert_eq!(level.to_text(), text, "text form is stable");
    }

    #[test]
    fn parses_dimensions_walls_and_tiles() {
        let level = Level::parse(TINY).expect("parses");
        assert_eq!(level.name, "Tiny");
        assert_eq!(level.posts, 1);
        assert_eq!(level.crab_count(), 1);
        let board = level.board();
        assert_eq!((board.width(), board.height()), (3, 2));
        assert_eq!(board.tile_at(2, 1), TileKind::Castle(0));
        assert!(board.wall_at(1, 0, Direction::Down));
        assert!(!board.wall_at(0, 0, Direction::Down));
        assert_eq!(board.crabs().len(), 1);
    }

    #[test]
    fn solution_wins_the_puzzle() {
        let level = Level::parse(TINY).expect("parses");
        let mut board = level.solved_board().expect("every placement lands");
        assert_eq!(level.play_out(&mut board).0, PuzzleOutcome::Won);
    }

    #[test]
    fn serialization_round_trips() {
        let level = Level::parse(TINY).expect("parses");
        let text = level.to_text();
        let again = Level::parse(&text).unwrap_or_else(|e| panic!("reparse: {e}\n{text}"));
        assert_eq!(level.name, again.name);
        assert_eq!(level.posts, again.posts);
        assert_eq!(level.solution, again.solution);
        assert_eq!(
            level.board().state_hash(),
            again.board().state_hash(),
            "round-tripped board must be bit-identical"
        );
    }

    #[test]
    fn timed_level_loses_at_the_wave() {
        // A `round:` shorter than the crab's walk: the tide freezes the sim
        // and the puzzle must resolve to Lost, not sit Running forever.
        let text = TINY.replace("posts: 1", "posts: 1\nround: 10");
        let level = Level::parse(&text).expect("parses");
        let mut board = level.board();
        for _ in 0..12 {
            board.tick_idle();
        }
        assert!(board.round_over());
        assert_eq!(level.outcome(&board), PuzzleOutcome::Lost);
    }

    #[test]
    fn goal_wrap_and_new_kinds_round_trip() {
        let text = "\
name: Fancy
posts: 2
rule: evict 3
round: 900
wrap: on
goal: bank 30
crab: 0,0 R L golden
crab: 2,1 L R sparkling
map:
+-+-+-+-+
|. . . .|
+ + + + +
|. 0 . .|
+-+-+-+-+
";
        let level = Level::parse(text).expect("parses");
        assert_eq!(level.goal, Goal::Bank(30));
        assert!(level.board().wrap());
        let again = Level::parse(&level.to_text()).expect("reparses");
        assert_eq!(again.goal, level.goal);
        assert_eq!(
            again.board().state_hash(),
            level.board().state_hash(),
            "wrap/goal/kind round-trip must be bit-identical"
        );
    }

    #[test]
    fn goal_outcomes_judge_correctly() {
        // Bank goal: reached mid-round.
        let bank = Level::parse(
            "name: B\nposts: 0\nrule: evict 3\nround: 900\ngoal: bank 1\n\
             crab: 0,1 R L common\nmap:\n+-+-+-+\n|. . .|\n+ + + +\n|. . 0|\n+-+-+-+\n",
        )
        .expect("parses");
        let mut board = bank.board();
        assert_eq!(bank.outcome(&board), PuzzleOutcome::Running);
        for _ in 0..200 {
            board.tick_idle();
        }
        assert_eq!(bank.outcome(&board), PuzzleOutcome::Won);

        // Survive goal: an eaten crab is an instant loss. The gull starts on
        // the crab's tile, so the very first collision pass eats it.
        let survive = Level::parse(
            "name: S\nposts: 0\nrule: evict 3\nround: 900\ngoal: survive\n\
             crab: 1,0 R L common\ngull: 1,0 R\nmap:\n+-+-+-+-+-+-+-+-+-+\n\
             |. . . . . . . . .|\n+-+-+-+-+-+-+-+-+-+\n",
        )
        .expect("parses");
        let mut board = survive.board();
        board.tick_idle();
        assert_eq!(
            survive.outcome(&board),
            PuzzleOutcome::Lost,
            "the gull got someone"
        );
    }

    /// The kind survives the format both ways round, and is the author's
    /// answer rather than the board's: a puzzle with four castles saved as
    /// a puzzle comes back a puzzle.
    #[test]
    fn the_kind_round_trips_and_outranks_the_castles() {
        let level = Level::parse(TINY).expect("parses");
        assert_eq!(level.kind, LevelKind::Puzzle, "one castle, one player");
        for kind in [LevelKind::Puzzle, LevelKind::Arena] {
            let text = level.clone().with_kind(kind).to_text();
            assert_eq!(Level::parse(&text).expect("reparses").kind, kind, "{text}");
        }
        // Four castles and the author still says puzzle.
        let four = "\
name: Four
posts: 1
kind: puzzle
crab: 0,0 R L common
map:
+-+-+-+-+
|0 1 2 3|
+-+-+-+-+
";
        let level = Level::parse(four).expect("parses");
        assert_eq!(level.seats(), 4);
        assert_eq!(level.kind, LevelKind::Puzzle, "the line beats the board");
        assert!(Level::parse(&four.replace("kind: puzzle", "kind: mud")).is_err());
    }

    /// A file written before the editor had a toggle says nothing about
    /// what it is, so the castles answer for it: one bank is a stage,
    /// castles for two players is a beach nobody plays alone.
    #[test]
    fn a_file_with_no_kind_is_read_off_its_castles() {
        assert_eq!(Level::parse(TINY).expect("parses").kind, LevelKind::Puzzle);
        let two = "\
name: Old Arena
posts: 3
crab: 0,0 R L common
map:
+-+-+-+
|0 . 1|
+-+-+-+
";
        let level = Level::parse(two).expect("parses");
        assert_eq!(level.seats(), 2);
        assert_eq!(level.kind, LevelKind::Arena);
        // And once read, it is written down: the guess happens once.
        assert!(
            level.to_text().contains("kind: arena"),
            "{}",
            level.to_text()
        );
    }

    #[test]
    fn inventory_is_enforced() {
        let level = Level::parse(TINY).expect("parses");
        let mut board = level.board();
        assert!(board.place_signpost(0, 0, 1, Direction::Up));
        // posts: 1, so the second placement must be rejected, not evict.
        assert!(!board.place_signpost(0, 1, 1, Direction::Up));
        assert_eq!(board.signpost_count(0), 1);
    }
}
