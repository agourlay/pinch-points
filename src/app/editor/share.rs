//! F3 and "Paste a code": the level on the bench as a share code, and a
//! code back onto the bench.

use super::naming::tidy_name;
use super::validate::{arena_report, start_check};
use super::{BoardSwap, EditorState, Offer, ask_first, level_here, replace_board, unsaved_work};
use crate::app::Sim;
use crate::app::i18n::fill;
use crate::sim::{Board, Level, LevelKind};
use bevy::prelude::*;

/// A level out of what was on the clipboard, or what to say about why not.
///
/// Takes the pasted code rather than the clipboard itself, so the three
/// ways this fails (nothing readable there, a code of the wrong sort, a
/// level this build cannot parse) each get their own answer and their own
/// test.
pub fn level_from_code(
    pasted: Option<(crate::share::Kind, Vec<u8>)>,
    tr: &crate::app::i18n::Tr,
) -> Result<Level, String> {
    let text =
        crate::app::codes::payload_text(pasted, tr, crate::share::Kind::Level, tr.code_level_bad)?;
    Level::parse(&text).map_err(|e| fill(tr.code_level_bad, &[("e", &e)]))
}

/// Whether a pasted level came from somebody else, which is what the
/// trophy for taking a code is about: "open a level from someone else's
/// code". Not when it is the level already on the bench, which is F3 and
/// then F4, and not when it is one of the player's own saved levels coming
/// back to them. Compared as the text a file would hold, name and all, so
/// a friend's edit of your level, or your level under their name, is
/// theirs.
fn is_from_elsewhere(pasted: &Level, bench: &Level, shelf: &[Level]) -> bool {
    let text = pasted.to_text();
    text != bench.to_text() && shelf.iter().all(|own| own.to_text() != text)
}

/// Whether a level pasted on the menu came from somebody else: by the
/// rule [`is_from_elsewhere`] holds a paste onto the bench to, with no
/// bench to be the level already there. The menu's level card counts a
/// code taken by it, as "Paste a code" does.
pub fn taken_from_elsewhere(pasted: &Level) -> bool {
    let shelf = crate::app::campaign::load_custom_levels();
    shelf.iter().all(|own| own.to_text() != pasted.to_text())
}

/// Take on what a level brought onto the bench says about itself: its
/// signpost grant, kind, goal, name, tables, roll, solution and the rate
/// its holes let crabs out at. Called once
/// its board is on the sand, since putting a board there is an edit and an
/// edit lets a solution go.
///
/// The goal, tables, roll and solution, because [`level_here`] rebuilds
/// the level from the board and a board carries none of them: a "bank 5"
/// stage came back from F2 and F3 as an every-crab stage, and a beach for
/// four and six lost the line that keeps a table of five off it.
///
/// The kind, because opening somebody's beach and saving it back as a
/// stage is not what either of you meant. The name, because the one on the
/// bench belongs to the level this one replaced. And it is a different
/// level from the one this bench saved, so none of the files the bench
/// wrote are its own: F2 asks before it writes over any of them, the
/// one under its name included. Its history begins with it, `board` being
/// the level on the sand. And the mirror goes off, as on every level
/// brought onto the bench: painted on with the last level's mirror, a
/// level not drawn with one had copies stamped on it unasked.
pub(super) fn take_level(
    state: &mut EditorState,
    level: &Level,
    board: &Board,
    tr: &crate::app::i18n::Tr,
) {
    state.posts = level.hands().map_or(level.posts, |[p1, _]| p1);
    state.second_hand = level.hands().map(|[_, p2]| p2);
    state.kind = level.kind;
    state.goal = level.goal;
    state.name = tidy_name(&level.name, tr);
    state.tables.clone_from(&level.tables);
    state.roll = level.roll;
    state.solution.clone_from(&level.solution);
    state.hole_period = super::setting::first_hole_period(&level.board());
    state.mine.clear();
    state.opened = None;
    state.filed = None;
    state.symmetry = super::Symmetry::Off;
    state.checked = None;
    state.begin_history(board);
}

/// F3: the level on the bench, as a code on the clipboard. A level is a
/// couple of hundred characters, which is a thing you can put in a
/// message, where "save it and send them the file" was never really
/// sharing.
pub(super) fn copy_level(
    state: &mut EditorState,
    board: &Board,
    clipboard: &mut Clipboard,
    tr: &crate::app::i18n::Tr,
    shared: &mut MessageWriter<crate::app::CodeShared>,
) {
    let level = level_here(state, board, &state.name);
    let (feedback, took) = crate::app::codes::copy_counted(
        clipboard,
        tr,
        crate::share::Kind::Level,
        level.to_text().as_bytes(),
        tr.code_copied,
    );
    state.feedback = feedback;
    if took {
        shared.write(crate::app::CodeShared);
    }
}

/// "Paste a code" on "My levels": the level on the clipboard (`pasted`, as
/// `codes::paste` read it) in place of the one on the bench. Says whether
/// it took the bench's place.
///
/// A paste throws the board away, so over unsaved work it asks twice, as
/// every way of opening a level does.
pub(super) fn paste_level(
    state: &mut EditorState,
    sim: &mut Sim,
    pasted: Option<(crate::share::Kind, Vec<u8>)>,
    (tr, device): (&crate::app::i18n::Tr, crate::app::gamepad::LastDevice),
    taken: &mut MessageWriter<crate::app::CodeTaken>,
    swap: &mut BoardSwap,
) -> bool {
    match level_from_code(pasted, tr) {
        Ok(level) => {
            let at_stake = unsaved_work(state, &sim.0);
            if !ask_first(
                state,
                Offer::Open,
                at_stake,
                Offer::Open.warning(tr, device),
            ) {
                return false;
            }
            let bench = level_here(state, &sim.0, &state.name);
            let shelf = crate::app::campaign::load_custom_levels();
            if is_from_elsewhere(&level, &bench, &shelf) {
                taken.write(crate::app::CodeTaken);
            }
            // A pasted level is any size, so it is a board swap in
            // full, sprites and cursor included.
            replace_board(level.board(), sim, state, swap);
            take_level(state, &level, &sim.0, tr);
            // A pasted level is the one board here that nobody vetted:
            // authored elsewhere, carried through a chat message, and
            // dropped straight onto the sand. Checked on the way in,
            // which for a beach is a count of its castles, and checked as
            // the bench will save it: a stage's rule stamped on, which
            // the code itself may not carry.
            if level.kind == LevelKind::Arena {
                state.feedback = arena_report(&sim.0, tr);
            } else {
                start_check(state, &sim.0);
                state.feedback = tr.code_level_checking.into();
            }
            true
        }
        Err(complaint) => {
            state.offer = None;
            state.feedback = complaint;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::naming::NAME_MAX;
    use super::*;
    use crate::sim::{Direction, TileKind};

    fn sand() -> Board {
        Board::new(6, 5, 3)
    }

    /// A pasted name is kept the way a typed one is: trimmed, capped, and
    /// the default when there is nothing to it.
    #[test]
    fn a_pasted_name_is_tidied_like_a_typed_one() {
        let tr = &crate::app::i18n::EN;
        let named = |name: &str| {
            let mut state = EditorState::default();
            let mut level = Level::from_board("x", 3, sand());
            level.name = name.into();
            take_level(&mut state, &level, &level.board(), tr);
            state.name
        };
        assert_eq!(named("  Gull Alley "), "Gull Alley");
        assert_eq!(named(""), tr.ed_default_name);
        assert_eq!(named("   "), tr.ed_default_name);
        let long = named(&"\u{e9}".repeat(NAME_MAX + 12));
        assert_eq!(long.chars().count(), NAME_MAX, "cut on characters");
        // A cut landing on a space leaves no trailing space behind it.
        let spaced = format!("{} tail", "a".repeat(NAME_MAX - 1));
        assert_eq!(named(&spaced), "a".repeat(NAME_MAX - 1));
    }

    /// The trophy for taking a code is about somebody else's level, so the
    /// one already on the bench (F3 then F4) and the player's own saved
    /// levels coming back to them do not count. An edit of theirs does.
    #[test]
    fn only_a_level_from_elsewhere_counts_as_taken() {
        let mut board = sand();
        board.set_tile(0, 0, TileKind::Castle(0));
        let mine = Level::from_board("Mine", 3, board.clone());
        board.set_tile(2, 2, TileKind::Rock);
        let bench = Level::from_board("Bench", 3, board.clone());
        let shelf = [mine.clone()];
        assert!(!is_from_elsewhere(&bench, &bench, &shelf), "F3 then F4");
        assert!(!is_from_elsewhere(&mine, &bench, &shelf), "my own, back");
        board.set_tile(3, 3, TileKind::Rock);
        let theirs = Level::from_board("Mine", 3, board);
        assert!(
            is_from_elsewhere(&theirs, &bench, &shelf),
            "their edit of mine"
        );
    }

    /// A pasted level brings its name, so the next F2 files it under that
    /// and not over the level it replaced; and none of the files this
    /// bench wrote are its own, so F2 asks before writing over any of them.
    #[test]
    fn a_pasted_level_brings_its_name() {
        let mut state = EditorState {
            posts: 3,
            name: "My Beach".into(),
            ..EditorState::default()
        };
        state
            .mine
            .insert(super::super::shelf::save_path("My Beach"));
        state
            .mine
            .insert(super::super::shelf::save_path("Gull Alley"));
        let pasted = Level::from_board("Gull Alley", 5, sand()).with_kind(LevelKind::Arena);
        take_level(&mut state, &pasted, &pasted.board(), &crate::app::i18n::EN);
        assert_eq!(state.name, "Gull Alley");
        assert_eq!((state.posts, state.kind), (5, LevelKind::Arena));
        assert!(state.mine.is_empty(), "a save over any of them is asked");
    }

    /// A pasted beach keeps the lines its board cannot carry: the tables
    /// it names and the strip it rolls by, through F2 and F3 alike; and a
    /// pasted stage keeps its solution until an edit changes its board.
    #[test]
    fn a_pasted_level_keeps_its_tables_roll_and_solution() {
        let text = "name: Four Six\nposts: 3\nkind: arena\nwrap: on\nseats: 4 6\nroll: 3\n\
            spawner: 1,1 R 60\nmap:\n\
            + + + + + + +\n 0 1 2 3 4 5 \n+ + + + + + +\n . . . . . . \n+ + + + + + +\n";
        let pasted = Level::parse(text).expect("a beach");
        assert_eq!((pasted.tables.as_slice(), pasted.roll), (&[4, 6][..], 3));
        let mut state = EditorState::default();
        take_level(&mut state, &pasted, &pasted.board(), &crate::app::i18n::EN);
        let here = level_here(&state, &pasted.board(), &state.name);
        assert_eq!(here.tables, [4, 6]);
        assert_eq!(here.roll, 3);
        // A table the castles no longer seat goes, and so does the roll
        // once the beach stops wrapping.
        let mut board = pasted.board();
        board.set_tile(5, 0, TileKind::Empty);
        board.set_wrap(false);
        let here = level_here(&state, &board, &state.name);
        assert_eq!((here.tables.as_slice(), here.roll), (&[4][..], 0));
        assert!(Level::parse(&here.to_text()).is_ok(), "{}", here.to_text());

        let mut stage = Level::from_board("Solved", 2, sand());
        stage.solution = vec![(1, 1, Direction::Up, 0)];
        take_level(&mut state, &stage, &stage.board(), &crate::app::i18n::EN);
        let here = level_here(&state, &stage.board(), &state.name);
        assert_eq!(here.solution, [(1, 1, Direction::Up, 0)]);
    }

    /// A pasted stage keeps its own goal through the bench: what F2
    /// saves, F3 shares, the playtest plays and the check certifies is the
    /// "bank 2" stage that was pasted, not an every-crab one.
    #[test]
    fn a_pasted_goal_survives_the_bench() {
        let mut board = sand();
        board.set_tile(4, 3, TileKind::Castle(0));
        let mut pasted = Level::from_board("Bank Two", 2, board.clone());
        pasted.goal = crate::sim::Goal::Bank(2);
        let pasted = Level::parse(&pasted.to_text()).expect("reads back");
        assert_eq!(pasted.goal, crate::sim::Goal::Bank(2));
        let mut state = EditorState::default();
        take_level(&mut state, &pasted, &pasted.board(), &crate::app::i18n::EN);
        let here = level_here(&state, &pasted.board(), &state.name);
        assert_eq!(here.goal, crate::sim::Goal::Bank(2));
        assert!(
            here.to_text().contains("goal: bank 2"),
            "{}",
            here.to_text()
        );
    }

    /// A level authored here survives being carried as a share code: out
    /// through the level format, through the codec, and back to a board with
    /// the same walls, tiles and signpost budget on it.
    #[test]
    fn a_level_travels_as_a_code_and_comes_back() {
        let mut board = sand();
        board.set_tile(2, 2, TileKind::Rock);
        board.set_tile(4, 3, TileKind::Castle(0));
        board.set_wall(1, 1, Direction::Right, true);
        board.set_wrap(true);
        let level = Level::from_board("Custom", 5, board.clone());

        let code = crate::share::encode(crate::share::Kind::Level, level.to_text().as_bytes());
        let back = level_from_code(crate::share::decode(&code), &crate::app::i18n::EN)
            .expect("our own level reads back");

        assert_eq!(back.posts, 5, "the signpost budget travels with it");
        let rebuilt = back.board();
        assert_eq!(rebuilt.tile_at(2, 2), TileKind::Rock);
        assert_eq!(rebuilt.tile_at(4, 3), TileKind::Castle(0));
        assert!(rebuilt.wall_at(1, 1, Direction::Right));
        assert!(rebuilt.wrap(), "an open-ocean level stays open");
    }

    /// The three ways a paste is not a level, each answered its own way.
    #[test]
    fn what_is_not_a_level_says_which_way_it_is_not() {
        let tr = &crate::app::i18n::EN;
        assert_eq!(
            level_from_code(None, tr).err(),
            Some(tr.code_none_pasted.to_string())
        );
        // A real code, for a round rather than a level.
        let round = crate::share::encode(crate::share::Kind::Round, b"replay-v1\n");
        let complaint = level_from_code(crate::share::decode(&round), tr).expect_err("not a level");
        assert!(
            complaint.contains("a round") && complaint.contains("a level"),
            "{complaint}"
        );
        // A level code carrying something that is not a level.
        let junk = crate::share::encode(crate::share::Kind::Level, b"not a level at all");
        assert!(level_from_code(crate::share::decode(&junk), tr).is_err());
    }
}
