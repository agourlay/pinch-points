//! Checking a level before it is saved: the solver on a background thread
//! for a puzzle, a castle count for a beach, and the warning for a level
//! that would land on neither list.

use super::EditorState;
use crate::app::i18n::fill;
use crate::app::settings::GameSettings;
use crate::sim::{Board, Level, LevelKind, SolveOutcome, TileKind, solve};
use bevy::prelude::*;
use std::sync::{Arc, Mutex};

/// Slot a background validation thread drops its result into. `None` means
/// still running; the [`SolveOutcome`] inside says which of the three answers
/// it came back with.
pub(super) type SolverSlot = Arc<Mutex<Option<SolveOutcome>>>;

/// What the check says about a beach, which is not a thing the solver can
/// answer: a match is not won or lost, it is only playable or not. Two
/// castles is the floor, and crabs have to come from somewhere.
pub(super) fn arena_report(board: &Board, tr: &crate::app::i18n::Tr) -> String {
    let seats = board.castle_seats();
    let holes = board
        .tiles()
        .filter(|(_, _, kind)| matches!(kind, TileKind::Spawner(_)))
        .count();
    if seats < 2 {
        return fill(tr.ed_arena_needs_seats, &[("n", &seats.to_string())]);
    }
    if holes == 0 && board.crabs().is_empty() {
        return tr.ed_arena_no_crabs.to_string();
    }
    fill(
        tr.ed_arena_ok,
        &[("n", &seats.to_string()), ("h", &holes.to_string())],
    )
}

/// Why the level, as it stands, will turn up on no list at all - or `None`
/// when it will.
///
/// Both lists have a floor: the stage list takes a puzzle with a crab to
/// route, the map dial takes a beach with a castle for every seat at the
/// table. A file that clears neither is written to disk and then never seen
/// again, and being saved is exactly the moment an author stops looking for
/// it. Before the kind was the author's to choose nothing could go missing
/// this way: every level with a crab was a stage.
pub(super) fn orphan_warning(
    state: &EditorState,
    level: &Level,
    tr: &crate::app::i18n::Tr,
) -> Option<String> {
    match state.kind {
        LevelKind::Arena if level.seats() < 2 => Some(fill(
            tr.ed_arena_needs_seats,
            &[("n", &level.seats().to_string())],
        )),
        LevelKind::Puzzle if level.crab_count() == 0 => Some(tr.ed_puzzle_no_crabs.to_string()),
        LevelKind::Arena | LevelKind::Puzzle => None,
    }
}

/// Hand a level to the solver on a background thread, superseding whatever
/// was running.
///
/// Off the frame thread because even a budgeted search is seconds of work
/// (`DEFAULT_NODE_BUDGET`), and the editor has to keep drawing while it
/// happens. Superseding rather than queueing because only the newest board
/// matters: an answer about a board the author has already replaced is worse
/// than no answer at all. The displaced thread runs on to its budget and
/// writes into a slot nobody holds any more, and its verdict is dropped.
pub(super) fn start_validation(state: &mut EditorState, level: Level) {
    let slot: SolverSlot = Arc::new(Mutex::new(None));
    let thread_slot = Arc::clone(&slot);
    std::thread::spawn(move || {
        *thread_slot.lock().unwrap() = Some(solve(&level));
    });
    state.solver = Some(slot);
}

/// Collect a finished background validation.
pub fn poll_solver(settings: Res<GameSettings>, mut state: ResMut<EditorState>) {
    let tr = settings.tr();
    let Some(slot) = &state.solver else {
        return;
    };
    let result = slot.lock().unwrap().take();
    match result {
        None => return, // still searching
        Some(SolveOutcome::Found(solution)) => {
            let described: Vec<String> = solution
                .iter()
                .map(|(x, y, dir)| format!("({x},{y} {dir:?})"))
                .collect();
            state.feedback = if solution.is_empty() {
                tr.ed_solvable_free.into()
            } else {
                fill(tr.ed_solvable, &[("placements", &described.join(" "))])
            };
        }
        Some(SolveOutcome::Unsolvable) => {
            state.feedback = fill(tr.ed_not_solvable, &[("n", &state.posts.to_string())]);
        }
        // The one answer the old editor could not give. It used to search
        // without a ceiling, so a board it could not crack simply never came
        // back, and "still validating..." forever reads as a hung editor.
        Some(SolveOutcome::GaveUp) => {
            state.feedback = tr.ed_solver_gave_up.into();
        }
    }
    state.solver = None;
}

#[cfg(test)]
mod tests {
    use super::super::brush::SPAWNER_PERIOD;
    use super::super::level_here;
    use super::*;
    use crate::sim::{CrabKind, Direction, Handedness, Spawner};

    fn sand() -> Board {
        Board::new(6, 5, 3)
    }

    /// Saving a level that will appear on neither list says so. Before the
    /// author picked the kind, nothing could go missing this way: every
    /// level with a crab on it was a stage.
    #[test]
    fn a_save_that_lands_nowhere_says_so() {
        use crate::app::i18n::EN;
        let mut board = sand();
        let state = |kind| EditorState {
            posts: 3,
            kind,
            ..EditorState::default()
        };

        // A beach with one castle is on no dial: two seats is the floor.
        board.set_tile(0, 0, TileKind::Castle(0));
        let arena = state(LevelKind::Arena);
        let level = level_here(&arena, &board, "Half a Beach");
        let complaint = orphan_warning(&arena, &level, &EN).expect("nowhere to go");
        assert!(complaint.contains('1'), "{complaint}");

        // The same board as a puzzle has no crab to route, which is the
        // other floor.
        let puzzle = state(LevelKind::Puzzle);
        let level = level_here(&puzzle, &board, "Empty Stage");
        assert_eq!(
            orphan_warning(&puzzle, &level, &EN).as_deref(),
            Some(EN.ed_puzzle_no_crabs)
        );

        // Give each what it wants and the complaint goes away.
        board.spawn_crab(2, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        let level = level_here(&puzzle, &board, "A Stage");
        assert_eq!(orphan_warning(&puzzle, &level, &EN), None);
        board.set_tile(5, 4, TileKind::Castle(1));
        let level = level_here(&arena, &board, "A Beach");
        assert_eq!(orphan_warning(&arena, &level, &EN), None);
    }

    /// Checking a beach counts its castles instead of searching for a
    /// route, and says which of the two things is missing.
    #[test]
    fn checking_a_beach_reads_its_castles() {
        use crate::app::i18n::EN;
        let mut board = sand();
        assert!(arena_report(&board, &EN).contains('0'), "bare sand seats 0");
        board.set_tile(0, 0, TileKind::Castle(0));
        let one = arena_report(&board, &EN);
        assert!(one.contains('1'), "one castle is not a match: {one}");

        board.set_tile(5, 4, TileKind::Castle(1));
        assert_eq!(
            arena_report(&board, &EN),
            EN.ed_arena_no_crabs,
            "castles but nothing to fight over"
        );

        board.set_tile(
            2,
            2,
            TileKind::Spawner(Spawner {
                dir: Direction::Right,
                period: SPAWNER_PERIOD,
            }),
        );
        let ok = arena_report(&board, &EN);
        assert!(ok.contains('2'), "two seats: {ok}");
        assert!(!ok.contains("wants"), "no complaint left: {ok}");
    }

    /// A validation started here really does come back with a verdict: the
    /// thread runs, the slot fills, and `poll_solver` has something to
    /// collect. Both answers are checked, because the interesting failure is
    /// a search that reports one board's verdict for another's.
    #[test]
    fn a_started_validation_leaves_a_verdict_in_the_slot() {
        fn verdict(level: Level) -> SolveOutcome {
            let mut state = EditorState::default();
            start_validation(&mut state, level);
            let slot = state.solver.expect("a validation is running");
            // The solver is on its own thread; wait for it rather than
            // sleeping a fixed guess.
            loop {
                if let Some(outcome) = slot.lock().unwrap().take() {
                    return outcome;
                }
                std::thread::yield_now();
            }
        }
        // A crab one tile left of a castle, walking into it: solvable with
        // nothing placed at all.
        let mut easy = sand();
        easy.set_tile(4, 2, TileKind::Castle(0));
        easy.spawn_crab(1, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        assert_eq!(
            verdict(Level::from_board("Custom", 1, easy)),
            SolveOutcome::Found(Vec::new())
        );

        // The same crab, with the castle walled off on all four sides.
        let mut sealed = sand();
        sealed.set_tile(4, 2, TileKind::Castle(0));
        sealed.spawn_crab(1, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        for dir in Direction::ALL {
            sealed.set_wall(4, 2, dir, true);
        }
        assert_eq!(
            verdict(Level::from_board("Custom", 1, sealed)),
            SolveOutcome::Unsolvable
        );
    }
}
