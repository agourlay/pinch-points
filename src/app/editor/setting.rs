//! The level's own settings, the rows of the editor's settings card: the
//! signposts a stage grants, its goal, its time, the flock, the holes'
//! rate and the wrap. One list says which rows a kind of level has, which
//! row "+" and "-" turn, and how each one steps, so the card, the keys and
//! the pad's triggers are three views of it.
//!
//! One row is the bench's rather than the level's: the mirror the author
//! paints with ([`super::Symmetry`]), dialled like the rest and saved
//! with none of them.

use super::EditorState;
use super::brush::SPAWNER_PERIOD;
use crate::sim::{Board, Goal, LevelKind, Spawner, TICKS_PER_SECOND, TileKind};

/// A setting the card shows and "+"/"-" turn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Setting {
    /// The signposts a stage grants its player: P1's hand on a co-op
    /// stage.
    #[default]
    Signposts,
    /// Who plays the stage: one player, or two with a hand each, and how
    /// many arrows P2's hand holds ([`EditorState::second_hand`]).
    Players,
    /// What the stage asks for ([`Goal`]).
    Goal,
    /// How long the stage runs before the tide ends it.
    Time,
    /// How often a gull flies in.
    Gulls,
    /// How often every hole lets a crab out.
    Holes,
    /// Whether the edges wrap round to the other side.
    Wrap,
    /// The copies each edit is made on as well: the bench's, not the
    /// level's ([`super::Symmetry`]).
    Mirror,
}

/// The stops the signpost grant runs between.
pub(super) const MAX_POSTS: u8 = 9;

/// The flock's stops, in ticks between gulls, off first.
pub(super) const GULL_PERIODS: [u32; 4] = [0, 480, 240, 120];

/// The holes' stops, in ticks between crabs.
pub(super) const HOLE_PERIODS: [u32; 5] = [30, SPAWNER_PERIOD, 90, 120, 180];

/// A stage's time stops, in seconds. The stage with no time of its own
/// plays [`crate::sim::PUZZLE_TICK_LIMIT`], which is the stop that reads as
/// that limit and writes no `round:` line.
pub(super) const TIME_STOPS: [u32; 6] = [30, 45, 60, 90, 120, 180];

/// The goals the dial offers: every goal the level format has, banking a
/// number of crabs at a few counts.
pub(super) const GOALS: [Goal; 13] = [
    Goal::AllCrabs,
    Goal::Survive,
    Goal::Golden,
    Goal::Bank(1),
    Goal::Bank(2),
    Goal::Bank(3),
    Goal::Bank(4),
    Goal::Bank(5),
    Goal::Bank(6),
    Goal::Bank(8),
    Goal::Bank(10),
    Goal::Bank(15),
    Goal::Bank(20),
];

impl Setting {
    /// The rows a kind of level has, in the card's order. A beach plays
    /// under the table's own terms for its goal, time, flock and signposts
    /// (`match_setup::apply_terms`), so the dials for those would turn
    /// numbers no versus round reads: its holes and its wrap are its own.
    pub(super) fn rows(kind: LevelKind) -> &'static [Setting] {
        match kind {
            LevelKind::Puzzle => &[
                Setting::Signposts,
                Setting::Players,
                Setting::Goal,
                Setting::Time,
                Setting::Gulls,
                Setting::Holes,
                Setting::Wrap,
                Setting::Mirror,
            ],
            LevelKind::Arena => &[Setting::Holes, Setting::Wrap, Setting::Mirror],
        }
    }

    /// The row the bench's "+"/"-" turn: the one chosen, or the kind's
    /// first while the one chosen is not a row of this kind.
    pub(super) fn chosen(state: &EditorState) -> Setting {
        let rows = Setting::rows(state.kind);
        match rows.contains(&state.setting) {
            true => state.setting,
            false => rows[0],
        }
    }

    /// The row after (or before) the chosen one, wrapping.
    pub(super) fn neighbour(state: &EditorState, forward: bool) -> Setting {
        let rows = Setting::rows(state.kind);
        let at = rows
            .iter()
            .position(|row| *row == Setting::chosen(state))
            .unwrap_or(0);
        let len = rows.len();
        rows[(at + if forward { 1 } else { len - 1 }) % len]
    }
}

/// The stop after (or before) `now` on `stops`, wrapping. A value off the
/// dial, as a pasted level may carry, steps to the stop beside it the way
/// the dial is turned, by `rank`, which the dial's stops climb in order:
/// "+" on 75 seconds goes to 90, not round to the first stop at 30. Past
/// either end it wraps as the dial does.
pub(super) fn step_stop_by<T: PartialEq + Copy, R: Ord>(
    stops: &[T],
    now: T,
    forward: bool,
    rank: impl Fn(T) -> R,
) -> T {
    let len = stops.len();
    match (stops.iter().position(|stop| *stop == now), forward) {
        (Some(at), true) => stops[(at + 1) % len],
        (Some(at), false) => stops[(at + len - 1) % len],
        (None, true) => stops
            .iter()
            .copied()
            .find(|&stop| rank(stop) > rank(now))
            .unwrap_or(stops[0]),
        (None, false) => stops
            .iter()
            .rev()
            .copied()
            .find(|&stop| rank(stop) < rank(now))
            .unwrap_or(stops[len - 1]),
    }
}

/// [`step_stop_by`] on a dial no value can be off, or whose stops climb
/// by their own order.
pub(super) fn step_stop<T: PartialEq + Copy + Ord>(stops: &[T], now: T, forward: bool) -> T {
    step_stop_by(stops, now, forward, |stop| stop)
}

/// The players dial's next stop: one player, then two with P2 holding one
/// arrow, two, and so on to [`MAX_POSTS`], then round to one player.
fn step_players(now: Option<u8>, forward: bool) -> Option<u8> {
    match (now, forward) {
        (None, true) => Some(1),
        (None, false) => Some(MAX_POSTS),
        (Some(n), true) if n >= MAX_POSTS => None,
        (Some(n), true) => Some(n + 1),
        (Some(n), false) if n <= 1 => None,
        (Some(n), false) => Some(n - 1),
    }
}

/// Where a goal stands on [`GOALS`], which the dial climbs: the three of
/// their own first, then banking by the count.
fn goal_rank(goal: Goal) -> (u8, u32) {
    match goal {
        Goal::AllCrabs => (0, 0),
        Goal::Survive => (1, 0),
        Goal::Golden => (2, 0),
        Goal::Bank(n) => (3, n),
    }
}

/// Where a flock stands on [`GULL_PERIODS`]: off first, then more often,
/// which is fewer ticks between gulls.
fn gull_rank(period: u32) -> (bool, std::cmp::Reverse<u32>) {
    (period != 0, std::cmp::Reverse(period))
}

/// The holes' rate on the bench: the one new holes are dug at and the
/// dial shows.
pub(super) fn hole_period(state: &EditorState) -> u32 {
    state.hole_period.unwrap_or(SPAWNER_PERIOD)
}

/// Every hole on `board`, where it is and what it is.
pub(super) fn holes(board: &Board) -> Vec<(u8, u8, Spawner)> {
    board
        .tiles()
        .filter_map(|(x, y, kind)| {
            if let TileKind::Spawner(s) = kind {
                Some((x, y, s))
            } else {
                None
            }
        })
        .collect()
}

/// The rate of the first hole on `board`, which a level brought onto the
/// bench sets the dial to.
pub(super) fn first_hole_period(board: &Board) -> Option<u32> {
    holes(board).first().map(|(_, _, s)| s.period)
}

/// A stage's time in seconds, as the dial reads it: its own round, or
/// the limit a stage without one is held to.
pub(super) fn time_secs(board: &Board) -> u32 {
    let ticks = crate::sim::Level::deadline(board);
    u32::try_from(ticks / u64::from(TICKS_PER_SECOND)).unwrap_or(u32::MAX)
}

/// Turn `setting` one stop forward or back, and say whether the level
/// changed. The signposts stop at nought and nine rather than wrap: a
/// grant running from nine to nought on one press throws a stage's whole
/// inventory away. The rest wrap, as the flock dial always has. The
/// mirror is no change to the level, and is not an edit: only its lines
/// on the sand are redrawn.
pub(super) fn step(
    state: &mut EditorState,
    board: &mut Board,
    setting: Setting,
    forward: bool,
) -> bool {
    match setting {
        Setting::Signposts => {
            let was = state.posts;
            // Up to nine by the dial; a pasted level may carry more, which
            // "+" leaves alone rather than lowering (or, at 255,
            // overflowing).
            if forward && state.posts < MAX_POSTS {
                state.posts += 1;
            } else if !forward {
                state.posts = state.posts.saturating_sub(1);
            }
            state.posts != was
        }
        Setting::Players => {
            state.second_hand = step_players(state.second_hand, forward);
            true
        }
        Setting::Goal => {
            state.goal = step_stop_by(&GOALS, state.goal, forward, goal_rank);
            true
        }
        Setting::Time => {
            let secs = step_stop(&TIME_STOPS, time_secs(board), forward);
            let limit = crate::sim::PUZZLE_TICK_LIMIT / u64::from(TICKS_PER_SECOND);
            board.rules_mut().round_length =
                (u64::from(secs) != limit).then_some(secs * TICKS_PER_SECOND);
            true
        }
        Setting::Gulls => {
            let rules = board.rules_mut();
            rules.gull_period = step_stop_by(&GULL_PERIODS, rules.gull_period, forward, gull_rank);
            true
        }
        Setting::Holes => {
            let period = step_stop(&HOLE_PERIODS, hole_period(state), forward);
            state.hole_period = Some(period);
            for (x, y, s) in holes(board) {
                board.set_tile(x, y, TileKind::Spawner(Spawner { period, ..s }));
            }
            true
        }
        Setting::Wrap => {
            board.set_wrap(!board.wrap());
            true
        }
        Setting::Mirror => {
            state.symmetry = step_stop_by(
                &super::Symmetry::ALL,
                state.symmetry,
                forward,
                super::Symmetry::index,
            );
            state.dirty = true;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every stop is reached forward and back, and a value off the dial
    /// steps onto it.
    #[test]
    fn a_dial_wraps_both_ways_and_catches_a_value_off_it() {
        let gulls = |now, forward| step_stop_by(&GULL_PERIODS, now, forward, gull_rank);
        assert_eq!(gulls(0, true), 480);
        assert_eq!(gulls(120, true), 0);
        assert_eq!(gulls(0, false), 120);
        // Off the dial, to the stop beside it the way it is turned.
        assert_eq!(gulls(333, true), 240);
        assert_eq!(gulls(333, false), 480);
        assert_eq!(gulls(60, true), 0, "past the end, round");
        assert_eq!(step_stop(&TIME_STOPS, 75, true), 90);
        assert_eq!(step_stop(&TIME_STOPS, 75, false), 60);
        let goal = |now, forward| step_stop_by(&GOALS, now, forward, goal_rank);
        assert_eq!(goal(Goal::Bank(7), true), Goal::Bank(8));
        assert_eq!(goal(Goal::Bank(7), false), Goal::Bank(6));
        assert_eq!(goal(Goal::Bank(30), true), Goal::AllCrabs);
        assert_eq!(super::super::resize::next_size((14, 10), false), (12, 9));
    }

    /// The time dial's stop at the stage limit writes no `round:` line,
    /// so a stage left at it reads as one with no time of its own.
    #[test]
    fn the_time_dial_at_the_limit_is_no_round_line() {
        let mut state = EditorState::default();
        let mut board = Board::new(6, 5, 1);
        assert_eq!(time_secs(&board), 60);
        step(&mut state, &mut board, Setting::Time, true);
        assert_eq!(time_secs(&board), 90);
        assert_eq!(board.rules().round_length, Some(90 * TICKS_PER_SECOND));
        step(&mut state, &mut board, Setting::Time, false);
        assert_eq!(board.rules().round_length, None, "back to the limit");
        step(&mut state, &mut board, Setting::Time, false);
        assert_eq!(time_secs(&board), 45);
    }

    /// The players dial runs from one player through P2's hand and round:
    /// a stage dialled to two is a co-op stage granting both hands, and
    /// back to one it is a solo stage again.
    #[test]
    fn the_players_dial_makes_a_coop_stage() {
        let mut state = EditorState {
            posts: 2,
            ..EditorState::default()
        };
        let mut board = Board::new(6, 5, 1);
        board.set_tile(5, 2, TileKind::Castle(0));
        assert!(step(&mut state, &mut board, Setting::Players, true));
        assert_eq!(state.second_hand, Some(1));
        let level = super::super::level_here(&state, &board, "Duo");
        assert_eq!((level.hands(), level.posts), (Some([2, 1]), 3));
        step(&mut state, &mut board, Setting::Players, false);
        assert_eq!(state.second_hand, None);
        let level = super::super::level_here(&state, &board, "Solo");
        assert_eq!((level.hands(), level.posts), (None, 2));
        assert_eq!(step_players(None, false), Some(MAX_POSTS));
        assert_eq!(step_players(Some(MAX_POSTS), true), None);
    }

    /// The signposts stop at the ends rather than wrap.
    #[test]
    fn the_signposts_stop_at_nought_and_nine() {
        let mut state = EditorState::default();
        let mut board = Board::new(6, 5, 1);
        assert!(!step(&mut state, &mut board, Setting::Signposts, false));
        state.posts = MAX_POSTS;
        assert!(!step(&mut state, &mut board, Setting::Signposts, true));
        assert_eq!(state.posts, MAX_POSTS);
    }

    /// The holes' dial turns every hole on the bench, and the row a beach
    /// does not have is never the one "+" turns there.
    #[test]
    fn the_holes_dial_turns_every_hole_and_rows_follow_the_kind() {
        let mut state = EditorState::default();
        let mut board = Board::new(6, 5, 1);
        for x in [1, 3] {
            let hole = Spawner {
                dir: crate::sim::Direction::Right,
                period: SPAWNER_PERIOD,
            };
            board.set_tile(x, 1, TileKind::Spawner(hole));
        }
        step(&mut state, &mut board, Setting::Holes, true);
        let periods: Vec<u32> = holes(&board).iter().map(|(_, _, s)| s.period).collect();
        assert_eq!(periods, [90, 90]);
        state.setting = Setting::Goal;
        state.kind = LevelKind::Arena;
        assert_eq!(Setting::chosen(&state), Setting::Holes);
        assert_eq!(Setting::neighbour(&state, true), Setting::Wrap);
        assert_eq!(Setting::neighbour(&state, false), Setting::Mirror);
    }
}
