//! Starter levels, offered on "My levels" beside a blank one: a small
//! stage to learn the editor on, and versus maps for two and four drawn
//! with the mirror ([`Symmetry`]), which the bench keeps painting with.

use super::brush::SPAWNER_PERIOD;
use super::symmetry::{self, Symmetry};
use crate::app::i18n::{Tr, fill};
use crate::sim::{Board, CrabKind, Direction, Handedness, Level, LevelKind, Spawner, TileKind};

/// A level "My levels" starts a new one from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Template {
    /// Fresh sand, the bench's own size and kind.
    Blank,
    /// A small stage with a crab, a castle and a rock in the way.
    Puzzle,
    /// A versus map for this many players, mirrored so every seat plays
    /// the same beach: two across a line down the middle, four in the
    /// corners.
    Beach(u8),
}

impl Template {
    /// The list's order: the blank first, as it always was.
    pub(super) const ALL: [Template; 4] = [
        Template::Blank,
        Template::Puzzle,
        Template::Beach(2),
        Template::Beach(4),
    ];

    /// What the list calls it.
    pub(super) fn label(self, tr: &Tr) -> String {
        match self {
            Template::Blank => tr.ed_levels_new.to_string(),
            Template::Puzzle => tr.ed_template_puzzle.to_string(),
            Template::Beach(seats) => fill(tr.ed_template_beach, &[("n", &seats.to_string())]),
        }
    }

    /// The mirror the bench paints with once it is opened: the one the
    /// map was drawn with, so an edit to it stays fair.
    pub(super) fn symmetry(self) -> Symmetry {
        match self {
            Template::Blank | Template::Puzzle => Symmetry::Off,
            Template::Beach(2) => Symmetry::LeftRight,
            Template::Beach(_) => Symmetry::Both,
        }
    }

    /// The level it opens as, `None` for the blank, which is fresh sand of
    /// the bench's own.
    pub(super) fn level(self) -> Option<Level> {
        let hole = |dir| {
            TileKind::Spawner(Spawner {
                dir,
                period: SPAWNER_PERIOD,
            })
        };
        match self {
            Template::Blank => None,
            Template::Puzzle => {
                // The crab walks right into the rock and turns away from
                // the castle: one signpost sends it up the column instead.
                let mut board = Board::new(9, 7, 0x5EED);
                board.set_tile(7, 1, TileKind::Castle(0));
                board.set_tile(7, 5, TileKind::Rock);
                board.set_tile(4, 2, TileKind::Rock);
                board.set_tile(4, 3, TileKind::Kelp);
                board.spawn_crab(1, 5, Direction::Right, Handedness::Right, CrabKind::Common);
                Some(Level::from_board("Starter", 2, board).with_kind(LevelKind::Puzzle))
            }
            Template::Beach(seats) => {
                let symmetry = self.symmetry();
                let mut board = Board::new(12, 9, 0xBEAC);
                // One corner's worth, its copies made by the mirror: a
                // castle, a hole on the near edge pointing in, and rocks
                // to route round.
                let paint = |board: &mut Board, (x, y): (u8, u8), kind: TileKind| {
                    board.set_tile(x, y, kind);
                    symmetry::mirror_tile(board, symmetry, true, (x, y));
                };
                match seats {
                    2 => {
                        paint(&mut board, (1, 4), TileKind::Castle(0));
                        paint(&mut board, (5, 0), hole(Direction::Down));
                        paint(&mut board, (5, 8), hole(Direction::Up));
                        paint(&mut board, (3, 2), TileKind::Rock);
                        paint(&mut board, (3, 6), TileKind::Rock);
                        paint(&mut board, (2, 4), TileKind::Kelp);
                    }
                    _ => {
                        paint(&mut board, (1, 1), TileKind::Castle(0));
                        paint(&mut board, (5, 0), hole(Direction::Down));
                        paint(&mut board, (3, 3), TileKind::Rock);
                        paint(&mut board, (1, 4), TileKind::Kelp);
                    }
                }
                board.set_wall(2, 1, Direction::Down, true);
                symmetry::mirror_wall(&mut board, symmetry, (2, 1), Direction::Down);
                Some(Level::from_board("Versus", 3, board).with_kind(LevelKind::Arena))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{SolveOutcome, solve};

    /// The starter stage can be won, and not without a signpost.
    #[test]
    fn the_starter_puzzle_is_winnable_with_a_signpost() {
        let level = Template::Puzzle.level().expect("a level");
        match solve(&level) {
            SolveOutcome::Found(placements) => assert!(!placements.is_empty()),
            other @ (SolveOutcome::Unsolvable | SolveOutcome::GaveUp) => {
                panic!("the starter cannot be won: {other:?}")
            }
        }
        assert_eq!(level.kind, LevelKind::Puzzle);
    }

    /// Each starter map seats the players it says, plays (every castle
    /// reached from every hole, none boxed in) and is even for every seat.
    #[test]
    fn the_starter_maps_seat_their_players_and_are_fair() {
        for template in Template::ALL {
            let Template::Beach(seats) = template else {
                continue;
            };
            let level = template.level().expect("a level");
            let board = level.board();
            assert_eq!(board.table_seats(), Some(seats), "{template:?}");
            assert_eq!(crate::sim::unplayable(&board), vec![], "{template:?}");
            assert!(
                matches!(
                    super::super::validate::balance(&board),
                    super::super::validate::Balance::Even { .. }
                ),
                "{template:?}"
            );
            assert_eq!(level.kind, LevelKind::Arena);
        }
    }
}
