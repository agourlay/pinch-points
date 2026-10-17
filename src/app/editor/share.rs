//! F3 and F4: the level on the bench as a share code, and a code back
//! onto the bench.

use super::naming::tidy_name;
use super::validate::{arena_report, start_validation};
use super::{BoardSwap, EditorState, Offer, ask_first, has_work, level_here, replace_board};
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
fn level_from(
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

/// Take on what a pasted level says about itself: its signpost grant, its
/// kind, its goal and its name.
///
/// The goal, because [`level_here`] rebuilds the level from the board and
/// a board does not carry one: a "bank 5" stage came back from F2 and F3 as
/// an every-crab stage, checked on the way in under the goal it no longer
/// had.
///
/// The kind, because opening somebody's beach and saving it back as a
/// stage is not what either of you meant. The name, because the one on the
/// bench belongs to the level the paste replaced, and F2 filed the pasted
/// board over the author's own file under it, without a word when this
/// session had saved it. And that name is taken back out of the ones this
/// session has saved under, so saving over a file already called that
/// says so.
fn take_level(state: &mut EditorState, level: &Level, tr: &crate::app::i18n::Tr) {
    state.posts = level.posts;
    state.kind = level.kind;
    state.goal = level.goal;
    state.name = tidy_name(&level.name, tr);
    state.mine.remove(&state.name);
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

/// F4: the level on the clipboard in place of the one on the bench.
///
/// A paste throws the board away as surely as F5 does, so once there is
/// a level here it asks twice the same way.
pub(super) fn paste_level(
    state: &mut EditorState,
    sim: &mut Sim,
    clipboard: &mut Clipboard,
    tr: &crate::app::i18n::Tr,
    taken: &mut MessageWriter<crate::app::CodeTaken>,
    swap: &mut BoardSwap,
) {
    match level_from(crate::app::codes::paste(clipboard), tr) {
        Ok(level) => {
            let at_stake = has_work(state, &sim.0);
            if !ask_first(state, Offer::Paste, at_stake, tr.ed_paste_confirm.into()) {
                return;
            }
            let bench = level_here(state, &sim.0, &state.name);
            let shelf = crate::app::campaign::load_custom_levels();
            if is_from_elsewhere(&level, &bench, &shelf) {
                taken.write(crate::app::CodeTaken);
            }
            take_level(state, &level, tr);
            // A pasted level is any size, so it is a board swap in
            // full, sprites and cursor included.
            replace_board(level.board(), sim, state, swap);
            // A pasted level is the one board here that nobody vetted:
            // authored elsewhere, carried through a chat message, and
            // dropped straight onto the sand. Checked on the way in,
            // which for a beach is a count of its castles.
            if level.kind == LevelKind::Arena {
                state.feedback = arena_report(&sim.0, tr);
            } else {
                start_validation(state, level);
                state.feedback = tr.code_level_checking.into();
            }
        }
        Err(complaint) => {
            state.offer = None;
            state.feedback = complaint;
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
            take_level(&mut state, &level, tr);
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
    /// and not over the level it replaced, and saving over a file already
    /// called that says so even when this session saved one under it.
    #[test]
    fn a_pasted_level_brings_its_name() {
        let mut state = EditorState {
            posts: 3,
            name: "My Beach".into(),
            ..EditorState::default()
        };
        state.mine.insert("My Beach".into());
        state.mine.insert("Gull Alley".into());
        let pasted = Level::from_board("Gull Alley", 5, sand()).with_kind(LevelKind::Arena);
        take_level(&mut state, &pasted, &crate::app::i18n::EN);
        assert_eq!(state.name, "Gull Alley");
        assert_eq!((state.posts, state.kind), (5, LevelKind::Arena));
        assert!(!state.mine.contains("Gull Alley"), "a save over it is said");
        assert!(state.mine.contains("My Beach"), "the rest are left alone");
    }

    /// A pasted Beach Day stage keeps its goal through the bench: what F2
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
        take_level(&mut state, &pasted, &crate::app::i18n::EN);
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
        let back = level_from(crate::share::decode(&code), &crate::app::i18n::EN)
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
            level_from(None, tr).err(),
            Some(tr.code_none_pasted.to_string())
        );
        // A real code, for a round rather than a level.
        let round = crate::share::encode(crate::share::Kind::Round, b"replay-v1\n");
        let complaint = level_from(crate::share::decode(&round), tr).expect_err("not a level");
        assert!(
            complaint.contains("a round") && complaint.contains("a level"),
            "{complaint}"
        );
        // A level code carrying something that is not a level.
        let junk = crate::share::encode(crate::share::Kind::Level, b"not a level at all");
        assert!(level_from(crate::share::decode(&junk), tr).is_err());
    }
}
