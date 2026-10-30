//! Checking a level before it is saved: the solver on a background thread
//! for a puzzle, a castle count for a beach, and the warning for a level
//! that would land on neither list.

use super::{EditorState, checked_level, level_here};
use crate::app::i18n::fill;
use crate::app::settings::GameSettings;
use crate::sim::{
    ArenaProblem, Board, Effort, Goal, Level, LevelKind, SolveOutcome, TileKind, solve, solve_from,
};
use bevy::prelude::*;
use std::sync::{Arc, Mutex};

/// What a check came back with: the solver's answer for the signposts the
/// stage grants, and, when those cannot win it, the fewest that can.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Verdict {
    pub(super) outcome: SolveOutcome,
    /// The fewest signposts that win the stage, found by a second search
    /// past the grant, up to the dial's top ([`super::setting::MAX_POSTS`]);
    /// `None` when there was no need to look, or that search found none
    /// within its budget.
    pub(super) needs: Option<usize>,
    /// On a co-op stage that can be won, whether it needs both players:
    /// `Some(true)` when neither player wins it alone with every arrow in
    /// their hand ([`crate::sim::needs_both`]), `Some(false)` when one of
    /// them can, and `None` on any other stage or when that search gave up.
    pub(super) shared: Option<bool>,
    /// Whether the stage checked is a co-op one, whose found arrows are
    /// each named by the hand that puts it down, however the search for
    /// [`Verdict::shared`] went.
    pub(super) coop: bool,
}

/// Slot a background validation thread drops its verdict into. `None`
/// means still running.
pub(super) type SolverSlot = Arc<Mutex<Option<Verdict>>>;

/// The second search's budget, past the grant: a quarter of the first's,
/// so a check of a stage that cannot be won still comes back in bounded
/// time, the deeper search costing the most per answer.
const WIDER_BUDGET: u32 = crate::sim::DEFAULT_NODE_BUDGET / 4;

/// A check in flight: where its answer will land.
///
/// The answer quotes the signposts granted ("NOT solvable - arrows
/// granted: 3"), read off the bench when it comes back. That is the count
/// the search was handed because turning the dial is an edit
/// ([`EditorState::edited`]) and drops the check: kept beside the request
/// instead, a dial turned mid-search left the old search running and V
/// saying "already validating" about a count no longer on the dial.
///
/// It also keeps the level it was asked about ([`checked_level`]'s text),
/// which the bench remembers as checked once the answer comes back.
pub(super) struct Validation {
    pub(super) slot: SolverSlot,
    pub(super) level: String,
    /// The save waiting on the answer, if F2 started or joined the check:
    /// the file and the name the level went into it under. A way through
    /// found is written into that file as its `solution:`.
    pub(super) save: Option<(std::path::PathBuf, String)>,
}

impl Validation {
    /// The save this check was finishing, carried on without the bench: a
    /// check let go of while a save waits on it (the bench edited, or the
    /// editor left) still owes the file its `solution:`. `None` for a check
    /// no save waits on, or a file that cannot be read.
    pub(super) fn orphan(self) -> Option<OrphanedSave> {
        let (path, _) = self.save?;
        let written = std::fs::read_to_string(&path).ok()?;
        Some(OrphanedSave {
            slot: self.slot,
            path,
            written,
        })
    }

    /// A check that never answers, for a test to drop or wait on.
    #[cfg(test)]
    pub(super) fn pending() -> Validation {
        Validation {
            slot: Arc::new(Mutex::new(None)),
            level: String::new(),
            save: None,
        }
    }
}

/// A save's check that outlived the bench it was started on
/// ([`Validation::orphan`]): its answer, when it comes, is written into
/// the file as it was saved, wherever the player is by then.
pub struct OrphanedSave {
    pub(super) slot: SolverSlot,
    path: std::path::PathBuf,
    /// The file as the save left it. The way through is written in only
    /// while the file still reads so: one saved over since, or deleted,
    /// is no longer the level the check was about.
    written: String,
}

impl OrphanedSave {
    /// Write the answer in, if it is back: true once there is nothing more
    /// to wait for.
    fn settle(&self) -> bool {
        let Some(verdict) = self.slot.lock().unwrap().take() else {
            return false; // still searching
        };
        let SolveOutcome::Found(solution) = verdict.outcome else {
            return true;
        };
        let still = std::fs::read_to_string(&self.path).is_ok_and(|now| now == self.written);
        if let (true, Ok(mut level)) = (still, Level::parse(&self.written)) {
            level.solution = solution;
            if let Err(e) = crate::app::paths::write_atomic(&self.path, level.to_text()) {
                warn!(
                    "could not write the solution into {}: {e}",
                    self.path.display()
                );
            }
        }
        true
    }
}

/// The saves whose checks outlived their bench, finished on any screen.
#[derive(Resource, Default)]
pub struct SaveChecks(pub(super) Vec<OrphanedSave>);

/// Take in the saves the bench let go of, and write each one's answer in
/// as it comes back. Runs everywhere: the editor left is the usual way a
/// check is orphaned.
pub fn finish_orphaned_saves(mut state: ResMut<EditorState>, mut checks: ResMut<SaveChecks>) {
    if !state.orphans.is_empty() {
        checks.0.append(&mut state.orphans);
    }
    if !checks.0.is_empty() {
        checks.0.retain(|check| !check.settle());
    }
}

/// How even a beach is for its seats: the walks from its holes to each
/// seat's castles ([`crate::sim::seat_walks`]), added up per seat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Balance {
    /// Fewer than two seats, or no hole: nothing to weigh yet.
    Unknown,
    /// Every seat walks as far: `steps` each.
    Even { steps: u32 },
    /// The seat with the shortest walk and the one with the longest, as
    /// `(seat, steps)`.
    Uneven { near: (u8, u32), far: (u8, u32) },
    /// A seat some hole's crabs cannot walk to.
    CutOff(u8),
}

/// Weigh `board` for its seats ([`Balance`]). The walks are added up over
/// every hole, so a seat close to one hole and far from the rest counts
/// as far as it is, and a beach drawn alike for every seat (the mirror's
/// work) comes out even.
pub(super) fn balance(board: &Board) -> Balance {
    let walks = crate::sim::seat_walks(board);
    if walks.len() < 2 || walks.iter().any(|(_, holes)| holes.is_empty()) {
        return Balance::Unknown;
    }
    let mut totals = Vec::new();
    for (seat, holes) in walks {
        match holes.into_iter().sum::<Option<u32>>() {
            Some(steps) => totals.push((seat, steps)),
            None => return Balance::CutOff(seat),
        }
    }
    let near = totals.iter().copied().min_by_key(|&(_, steps)| steps);
    let far = totals.iter().copied().max_by_key(|&(_, steps)| steps);
    match (near, far) {
        (Some(near), Some(far)) if near.1 != far.1 => Balance::Uneven { near, far },
        (Some((_, steps)), _) => Balance::Even { steps },
        (None, _) => Balance::Unknown,
    }
}

/// What the check says about a beach, which is not a thing the solver can
/// answer: a match is not won or lost, it is only playable or not, and
/// fair or not. Two castles is the floor, crabs have to come from
/// somewhere, and a seat further from the holes than another, or one some
/// hole cannot reach at all, is named ([`balance`]).
pub(super) fn arena_report(board: &Board, tr: &crate::app::i18n::Tr) -> String {
    if let Some(problem) = board.arena_problem() {
        return arena_complaint(problem, tr);
    }
    if !board.feeds_crabs() {
        return tr.ed_arena_no_crabs.to_string();
    }
    let seat = |seat: u8| crate::app::seat_label(tr, seat);
    match balance(board) {
        Balance::Uneven { near, far } => {
            return fill(
                tr.ed_fair_uneven,
                &[
                    ("p", &seat(near.0)),
                    ("n", &near.1.to_string()),
                    ("q", &seat(far.0)),
                    ("m", &far.1.to_string()),
                ],
            );
        }
        Balance::CutOff(cut) => return fill(tr.ed_fair_cut_off, &[("p", &seat(cut))]),
        Balance::Unknown | Balance::Even { .. } => {}
    }
    let seats = board.castle_seats();
    let holes = board
        .tiles()
        .filter(|(_, _, kind)| matches!(kind, TileKind::Spawner(_)))
        .count();
    fill(
        tr.ed_arena_ok,
        &[("n", &seats.to_string()), ("h", &holes.to_string())],
    )
}

/// What to say about a beach no table can sit at. A skipped seat is one no
/// map dial offers at any size of table ([`Level::fits`]):
/// seat `n` plays castle `n`, and the skipped seat would have nothing to
/// bank in.
fn arena_complaint(problem: ArenaProblem, tr: &crate::app::i18n::Tr) -> String {
    match problem {
        ArenaProblem::SkippedSeat(seat) => fill(
            tr.ed_arena_skips_seat,
            &[("n", &(u32::from(seat) + 1).to_string())],
        ),
        ArenaProblem::TooFew(seats) => fill(tr.ed_arena_needs_seats, &[("n", &seats.to_string())]),
    }
}

/// What saving the level should warn about, if anything: the first of the
/// reasons it will not be listed, not be playable or not be known to be
/// winnable, in that order. Being saved is the moment an author stops
/// looking at a level, so a file that lands on no list, or on one where
/// nobody can win it, says so then.
///
/// One at a time, the gravest first: the status line has room for one,
/// and a level with no crabs is not made worse by being unchecked too.
pub(super) fn save_warning(
    state: &EditorState,
    level: &Level,
    tr: &crate::app::i18n::Tr,
) -> Option<String> {
    match state.kind {
        LevelKind::Arena => arena_seat_warning(level, tr).or_else(|| arena_crab_warning(level, tr)),
        LevelKind::Puzzle => {
            puzzle_crab_warning(level, tr).or_else(|| puzzle_castle_warning(level, tr))
        }
    }
}

/// A beach no table can sit at: castles for fewer than two seats, or
/// castles that skip one. The map dial offers it nowhere.
fn arena_seat_warning(level: &Level, tr: &crate::app::i18n::Tr) -> Option<String> {
    level
        .arena_problem()
        .map(|problem| arena_complaint(problem, tr))
}

/// A beach with no crab on it and no hole to bring one: nothing to fight
/// over, so the map dial leaves it out ([`Board::feeds_crabs`]).
fn arena_crab_warning(level: &Level, tr: &crate::app::i18n::Tr) -> Option<String> {
    (!level.board().feeds_crabs()).then(|| tr.ed_arena_no_crabs.to_string())
}

/// A stage with no crab to route, which no stage list takes.
fn puzzle_crab_warning(level: &Level, tr: &crate::app::i18n::Tr) -> Option<String> {
    (level.crab_count() == 0).then(|| tr.ed_puzzle_no_crabs.to_string())
}

/// A stage whose goal is to bank crabs and that has no castle to bank
/// them in: it is listed, and nobody can win it.
fn puzzle_castle_warning(level: &Level, tr: &crate::app::i18n::Tr) -> Option<String> {
    let banks = match level.goal {
        Goal::AllCrabs | Goal::Bank(_) | Goal::Golden => true,
        Goal::Survive => false,
    };
    let castles = level.board().castle_owners().next().is_some();
    (banks && !castles).then(|| tr.ed_puzzle_no_castle.to_string())
}

/// Check the level on the bench: hand it to the solver on a background
/// thread, superseding whatever was running. When the signposts granted
/// cannot win it, the thread searches on past the grant for the fewest
/// that can ([`Verdict::needs`]), so the answer says what the stage is
/// short of rather than only that it is.
///
/// Off the frame thread because even a budgeted search is seconds of work
/// (`DEFAULT_NODE_BUDGET`) and the editor has to keep drawing. Superseding
/// rather than queueing because only the newest board matters: the
/// displaced thread runs on to its budget, writes into a slot nobody holds
/// any more, and its verdict is dropped.
pub(super) fn start_check(state: &mut EditorState, board: &Board) {
    let level = checked_level(state, board);
    // The same level granting the dial's top, which the bench is put at
    // for as long as it takes to read it off and no longer.
    let granted = state.posts;
    // A co-op stage's grant is two hands, and more arrows in P1's alone
    // would answer a question nobody asked of it.
    let solo = state.second_hand.is_none();
    let wider = (solo && granted < super::setting::MAX_POSTS).then(|| {
        state.posts = super::setting::MAX_POSTS;
        let wider = checked_level(state, board);
        state.posts = granted;
        wider
    });
    start_validation(state, level, wider);
}

/// [`start_check`] on levels already in hand: `level` as it is granted,
/// and `wider` the same with more signposts, searched only if `level`
/// cannot be won.
pub(super) fn start_validation(state: &mut EditorState, level: Level, wider: Option<Level>) {
    let slot: SolverSlot = Arc::new(Mutex::new(None));
    let thread_slot = Arc::clone(&slot);
    let text = level.to_text();
    std::thread::spawn(move || {
        let outcome = solve(&level);
        let needs = match (&outcome, wider) {
            // The first search proved no count up to the grant wins, so
            // the second starts past it: searched again from none, the
            // counts already proved short cost it many times the first.
            (SolveOutcome::Unsolvable, Some(wider)) => {
                let past = level.posts.saturating_add(1);
                match solve_from(&wider, Effort::Budget(WIDER_BUDGET), past) {
                    SolveOutcome::Found(placements) => Some(placements.len()),
                    SolveOutcome::Unsolvable | SolveOutcome::GaveUp => None,
                }
            }
            _ => None,
        };
        // A co-op stage that can be won is also tried with every arrow in
        // one hand: one that either player wins alone is a solo puzzle
        // with a quota, which the author is told.
        let shared = match (&outcome, level.is_coop()) {
            (SolveOutcome::Found(_), true) => {
                match crate::sim::needs_both(&level, Effort::Budget(WIDER_BUDGET)) {
                    SolveOutcome::Unsolvable => Some(true),
                    SolveOutcome::Found(_) => Some(false),
                    SolveOutcome::GaveUp => None,
                }
            }
            _ => None,
        };
        *thread_slot.lock().unwrap() = Some(Verdict {
            outcome,
            needs,
            shared,
            coop: level.is_coop(),
        });
    });
    state.solver = Some(Validation {
        slot,
        level: text,
        save: None,
    });
}

/// The way something on the bench faces, as an arrow, in the readout of
/// what the cursor stands on: read in all eight languages, where "Up" and
/// "Left" would be English to every one of them. The UI face the cards
/// are set in draws these four, the Japanese text included, since it is
/// the face that draws first. The header's display face does not, which
/// is why the check's answer there names its tiles and leaves the ways to
/// the signposts drawn on the board.
pub(super) fn arrow(dir: crate::sim::Direction) -> char {
    use crate::sim::Direction;
    match dir {
        Direction::Up => '↑',
        Direction::Down => '↓',
        Direction::Left => '←',
        Direction::Right => '→',
    }
}

/// What a check's verdict says, granted `posts` signposts.
pub(super) fn verdict_line(verdict: &Verdict, posts: u8, tr: &crate::app::i18n::Tr) -> String {
    match &verdict.outcome {
        SolveOutcome::Found(solution) if solution.is_empty() => tr.ed_solvable_free.into(),
        SolveOutcome::Found(solution) => {
            // On a co-op stage each arrow is named by the hand that puts it
            // down, which is half of the answer.
            let described: Vec<String> = solution
                .iter()
                .map(|&(x, y, _, seat)| match verdict.coop {
                    true => format!("{}({x},{y})", crate::app::seat_label(tr, seat)),
                    false => format!("({x},{y})"),
                })
                .collect();
            let said = fill(
                tr.ed_solvable,
                &[
                    ("n", &solution.len().to_string()),
                    ("g", &posts.to_string()),
                    ("placements", &described.join(" ")),
                ],
            );
            // The co-op verdict leads, as a save's warnings do: what the
            // status line clips past the header is the arrows' tiles, which
            // the bench draws in each hand's colour anyway.
            match verdict.shared {
                Some(true) => format!("{} - {said}", tr.ed_needs_both),
                Some(false) => format!("{} - {said}", tr.ed_one_alone),
                None => said,
            }
        }
        SolveOutcome::Unsolvable => match verdict.needs {
            Some(needs) => fill(
                tr.ed_not_solvable_needs,
                &[("n", &posts.to_string()), ("m", &needs.to_string())],
            ),
            None => fill(tr.ed_not_solvable, &[("n", &posts.to_string())]),
        },
        // The one answer the old editor could not give. It used to search
        // without a ceiling, so a board it could not crack simply never came
        // back, and "still validating..." forever reads as a hung editor.
        SolveOutcome::GaveUp => tr.ed_solver_gave_up.into(),
    }
}

/// Collect a finished background validation.
///
/// A check a save is waiting on ([`Validation::save`]) finishes the save:
/// a way through found goes into the file as its `solution:`, and the line
/// gives the verdict and then the file.
pub fn poll_solver(
    settings: Res<GameSettings>,
    sim: Res<crate::app::Sim>,
    mut state: ResMut<EditorState>,
) {
    let tr = settings.tr();
    // A playtest has the sand and the status line: the answer waits in
    // its slot until the bench it is about is back.
    if state.is_testing() {
        return;
    }
    let Some(check) = &state.solver else {
        return;
    };
    let Some(verdict) = check.slot.lock().unwrap().take() else {
        return; // still searching
    };
    let Some(Validation { level, save, .. }) = state.solver.take() else {
        return;
    };
    // Whatever the answer, the level has been checked.
    state.checked = Some(level);
    if let SolveOutcome::Found(solution) = &verdict.outcome {
        // The way through, kept as the level's own (`solution:`) and drawn
        // on the sand until the next edit, which lets it go: the check was
        // dropped by any edit since it began, so it won the bench as it
        // stands.
        state.solution.clone_from(solution);
        state.dirty = true;
        let now = super::history::snapshot(&state, &sim.0);
        state.history.amend(now);
    }
    let granted = state.posts.saturating_add(state.second_hand.unwrap_or(0));
    let said = verdict_line(&verdict, granted, tr);
    let filed =
        save.and_then(|(path, name)| finish_save(&mut state, &sim.0, &path, &name, &verdict, tr));
    state.feedback = match filed {
        Some(filed) => format!("{said} - {filed}"),
        None => said,
    };
}

/// A save's last step once its check is back: the solution found written
/// into the file, which the bench has not changed since (an edit would
/// have dropped the check). Says what the save line says of the file.
///
/// Only into a file the bench still holds as its own ([`EditorState::mine`]):
/// one deleted on "My levels" while the check ran is gone, and writing the
/// solution into it brought the level back. `None` then, the save having
/// nothing left to say.
fn finish_save(
    state: &mut EditorState,
    board: &Board,
    path: &std::path::Path,
    name: &str,
    verdict: &Verdict,
    tr: &crate::app::i18n::Tr,
) -> Option<String> {
    if !state.mine.contains(path) {
        return None;
    }
    let file = super::shelf::file_name(path);
    if matches!(verdict.outcome, SolveOutcome::Found(_)) {
        let text = level_here(state, board, name).to_text();
        if let Err(e) = crate::app::paths::write_atomic(path, &text) {
            return Some(fill(tr.ed_save_failed, &[("e", &e.to_string())]));
        }
        if state.name == name {
            state.filed = Some(super::filed_text(state, board));
        }
    }
    Some(fill(tr.ed_saved_to, &[("file", &file)]))
}

#[cfg(test)]
mod tests {
    /// The solver's answer names directions with arrows, so the face
    /// every line is drawn in has to carry them: a missing glyph draws as
    /// nothing at all.
    #[test]
    fn the_ui_face_draws_the_solution_arrows() {
        let face = crate::app::i18n::metrics::characters_in_font(include_bytes!(
            "../../../assets/fonts/DejaVuSansMono.ttf"
        ));
        for dir in Direction::ALL {
            let glyph = arrow(dir);
            assert!(face.contains(&glyph), "{dir:?} draws as {glyph:?}");
        }
    }

    use super::super::brush::SPAWNER_PERIOD;
    use super::super::level_here;
    use super::*;
    use crate::sim::{CrabKind, Direction, Handedness, Spawner};

    fn sand() -> Board {
        Board::new(6, 5, 3)
    }

    /// What the editor says once the background search hands back `outcome`.
    fn verdict(outcome: Option<SolveOutcome>) -> (String, bool) {
        said_of(outcome.map(|outcome| Verdict {
            outcome,
            needs: None,
            shared: None,
            coop: false,
        }))
    }

    /// The same, for a whole verdict.
    fn said_of(verdict: Option<Verdict>) -> (String, bool) {
        let mut app = App::new();
        app.insert_resource(GameSettings::default());
        app.insert_resource(crate::app::Sim(sand()));
        app.insert_resource(EditorState {
            // The dial cannot turn under a running search (it drops it),
            // so the count on the bench is the one the search was handed.
            posts: 2,
            solver: Some(Validation {
                slot: Arc::new(Mutex::new(verdict)),
                level: String::new(),
                save: None,
            }),
            ..EditorState::default()
        });
        app.add_systems(Update, poll_solver);
        app.update();
        let state = app.world().resource::<EditorState>();
        (state.feedback.clone(), state.solver.is_some())
    }

    /// A found solution is spelled out placement by placement, and one
    /// that needs no arrows at all says that instead of an empty list.
    #[test]
    fn a_solution_is_spelled_out_or_called_free() {
        use crate::app::i18n::EN;
        let (said, pending) = verdict(Some(SolveOutcome::Found(vec![(1, 2, Direction::Up, 0)])));
        assert!(!pending, "the answer is taken");
        assert_eq!(
            said,
            fill(
                EN.ed_solvable,
                &[("n", "1"), ("g", "2"), ("placements", "(1,2)")]
            )
        );
        let (said, _) = verdict(Some(SolveOutcome::Found(Vec::new())));
        assert_eq!(said, EN.ed_solvable_free);
    }

    /// A co-op stage's arrows are named by their hands even when the
    /// search for whether it needs both players gave up: the hands are half
    /// of the answer, and that search is a second, separate one.
    #[test]
    fn a_coop_solution_names_its_hands_whatever_the_second_search_said() {
        use crate::app::i18n::EN;
        let solution = vec![(3, 2, Direction::Up, 0), (5, 2, Direction::Down, 1)];
        let said = verdict_line(
            &Verdict {
                outcome: SolveOutcome::Found(solution),
                needs: None,
                shared: None,
                coop: true,
            },
            2,
            &EN,
        );
        assert!(said.contains("P1(3,2) P2(5,2)"), "{said}");
    }

    /// No solution names the arrows it was tried with, a search that ran
    /// out of budget says so rather than claiming either answer, and one
    /// still running leaves the slot where it is.
    #[test]
    fn no_solution_and_no_answer_are_told_apart() {
        use crate::app::i18n::EN;
        let (said, _) = verdict(Some(SolveOutcome::Unsolvable));
        assert_eq!(said, EN.ed_not_solvable.replace("{n}", "2"));
        let (said, _) = said_of(Some(Verdict {
            outcome: SolveOutcome::Unsolvable,
            needs: Some(4),
            shared: None,
            coop: false,
        }));
        assert_eq!(
            said,
            fill(EN.ed_not_solvable_needs, &[("n", "2"), ("m", "4")]),
            "and the fewest that would"
        );
        let (said, _) = verdict(Some(SolveOutcome::GaveUp));
        assert_eq!(said, EN.ed_solver_gave_up);
        let (said, pending) = verdict(None);
        assert!(pending, "still searching");
        assert!(said.is_empty(), "and nothing said yet: {said}");
    }

    /// Saving a level that will appear on neither list, or on one where
    /// nobody can win it or nobody knows if they can, says so: the gravest
    /// reason first, one at a time.
    #[test]
    fn a_save_that_lands_nowhere_says_so() {
        use crate::app::i18n::EN;
        let mut board = sand();
        let state = |kind| EditorState {
            posts: 3,
            kind,
            ..EditorState::default()
        };
        let warned = |state: &EditorState, board: &Board| {
            save_warning(state, &level_here(state, board, "Probe"), &EN)
        };

        // A beach with one castle is on no dial: two seats is the floor.
        board.set_tile(0, 0, TileKind::Castle(0));
        let arena = state(LevelKind::Arena);
        let complaint = warned(&arena, &board).expect("nowhere to go");
        assert!(complaint.contains('1'), "{complaint}");

        // The same board as a puzzle has no crab to route, which is the
        // other floor.
        let mut puzzle = state(LevelKind::Puzzle);
        assert_eq!(
            warned(&puzzle, &board).as_deref(),
            Some(EN.ed_puzzle_no_crabs)
        );

        // A crab with no castle to bank in is listed and cannot be won;
        // a goal of losing none needs no castle.
        let mut castleless = sand();
        castleless.spawn_crab(2, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        assert_eq!(
            warned(&puzzle, &castleless).as_deref(),
            Some(EN.ed_puzzle_no_castle)
        );
        puzzle.goal = Goal::Survive;
        assert_ne!(
            warned(&puzzle, &castleless).as_deref(),
            Some(EN.ed_puzzle_no_castle)
        );
        puzzle.goal = Goal::AllCrabs;

        // A stage that can be listed and won has nothing to warn about:
        // whether it is won is the check's to say, which a save starts.
        board.spawn_crab(2, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        assert_eq!(warned(&puzzle, &board), None);

        // Two castles and a crab make a beach.
        board.set_tile(5, 4, TileKind::Castle(1));
        assert_eq!(warned(&arena, &board), None);

        // Two castles and nothing to fight over do not.
        let mut bare = sand();
        bare.set_tile(0, 0, TileKind::Castle(0));
        bare.set_tile(5, 4, TileKind::Castle(1));
        assert_eq!(warned(&arena, &bare).as_deref(), Some(EN.ed_arena_no_crabs));
    }

    /// Castles that skip a player seat no table, and the check and the
    /// save both name the player skipped, counted from 1.
    #[test]
    fn a_beach_that_skips_a_player_says_which() {
        use crate::app::i18n::EN;
        let mut board = sand();
        board.set_tile(0, 0, TileKind::Castle(0));
        board.set_tile(5, 4, TileKind::Castle(2));
        let said = fill(EN.ed_arena_skips_seat, &[("n", "2")]);
        assert_eq!(arena_report(&board, &EN), said);
        let arena = EditorState {
            kind: LevelKind::Arena,
            ..EditorState::default()
        };
        let level = level_here(&arena, &board, "Gappy");
        assert_eq!(save_warning(&arena, &level, &EN), Some(said));
        board.set_tile(3, 3, TileKind::Castle(1));
        assert!(!arena_report(&board, &EN).contains("castle -"));
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

    /// A beach whose seats walk unequal ways from the hole is called
    /// uneven, naming the nearest seat and the furthest; mirrored it is
    /// even; and a seat the hole cannot reach is named.
    #[test]
    fn a_beach_is_weighed_for_its_seats() {
        use crate::app::i18n::EN;
        let mut board = Board::new(7, 3, 1);
        board.set_tile(
            3,
            1,
            TileKind::Spawner(Spawner {
                dir: Direction::Up,
                period: SPAWNER_PERIOD,
            }),
        );
        board.set_tile(0, 1, TileKind::Castle(0));
        board.set_tile(5, 1, TileKind::Castle(1));
        assert_eq!(
            balance(&board),
            Balance::Uneven {
                near: (1, 4),
                far: (0, 5)
            }
        );
        let said = fill(
            EN.ed_fair_uneven,
            &[("p", "P2"), ("n", "4"), ("q", "P1"), ("m", "5")],
        );
        assert_eq!(arena_report(&board, &EN), said);
        board.set_tile(5, 1, TileKind::Empty);
        board.set_tile(6, 1, TileKind::Castle(1));
        assert_eq!(balance(&board), Balance::Even { steps: 5 });
        assert!(!arena_report(&board, &EN).contains("P1"));
        for y in 0..3 {
            board.set_tile(5, y, TileKind::Rock);
        }
        assert_eq!(balance(&board), Balance::CutOff(1));
        assert_eq!(
            arena_report(&board, &EN),
            fill(EN.ed_fair_cut_off, &[("p", "P2")])
        );
    }

    /// A validation started here really does come back with a verdict: the
    /// thread runs, the slot fills, and `poll_solver` has something to
    /// collect. Both answers are checked, because the interesting failure is
    /// a search that reports one board's verdict for another's.
    #[test]
    fn a_started_validation_leaves_a_verdict_in_the_slot() {
        fn verdict(level: Level) -> SolveOutcome {
            needed(level, None).outcome
        }
        fn needed(level: Level, wider: Option<Level>) -> Verdict {
            let mut state = EditorState::default();
            start_validation(&mut state, level, wider);
            let slot = state.solver.expect("a validation is running").slot;
            // The solver is on its own thread; wait for it rather than
            // sleeping a fixed guess.
            loop {
                if let Some(verdict) = slot.lock().unwrap().take() {
                    return verdict;
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

        // A crab pacing a corridor past the one gap up to its castle: no
        // arrow, no way in; one, turning it up at the gap, and the second
        // search past the grant says so.
        let mut corridor = sand();
        corridor.set_tile(3, 1, TileKind::Castle(0));
        for x in 0..6 {
            corridor.set_wall(x, 2, Direction::Down, true);
            if x != 3 {
                corridor.set_wall(x, 2, Direction::Up, true);
            }
        }
        corridor.spawn_crab(1, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        let granted = Level::from_board("Corridor", 0, corridor.clone());
        let wider = Level::from_board("Corridor", 3, corridor);
        assert_eq!(
            needed(granted, Some(wider)),
            Verdict {
                outcome: SolveOutcome::Unsolvable,
                needs: Some(1),
                shared: None,
                coop: false,
            }
        );
    }
}
