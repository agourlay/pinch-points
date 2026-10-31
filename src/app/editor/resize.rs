//! F5 and Shift+F5: the beach a size up or down, with what is built on it
//! kept where it stands against the middle.

use crate::sim::{Board, Direction, TileKind};

/// Beach sizes the editor offers, smallest first. The same set versus plays
/// on, so a level built here fits any of the arenas the game already
/// draws.
pub(super) const SIZES: [(u8, u8); 4] = [(9, 7), (12, 9), (16, 11), (20, 13)];

/// The size after (or before) `now` on [`SIZES`], wrapping. A size off the
/// list, as a pasted level may have, steps to the size beside it the way
/// it is stepped.
pub(super) fn next_size(now: (u8, u8), up: bool) -> (u8, u8) {
    super::setting::step_stop(&SIZES, now, up)
}

/// A board resized, and what came of it.
pub(super) struct Resized {
    pub board: Board,
    /// How far everything moved: the new tile of what stood at `(x, y)`
    /// is `(x + dx, y + dy)`.
    pub shift: (i16, i16),
    /// Whether anything built fell off a side that shrank.
    pub cut: bool,
}

/// `board` at `size`, everything on it kept centred: a side that grows
/// gains sand evenly at both ends, and one that shrinks loses it evenly
/// (the odd tile at the right or bottom), so a level built in the middle of
/// the beach keeps its shape and its place.
///
/// The rim is the new board's own. The old rim's walls were the fence, and
/// carried inward they would wall the level in on a larger beach; a wall
/// inside the old beach that lands on the new rim is the fence already, or
/// the open seam of a wrapping beach, which takes none. Everything else
/// the level's text reads off the board comes along unchanged: the rules,
/// the wrap, the seed, the tide events and the scores.
pub(super) fn resized(board: &Board, (w, h): (u8, u8)) -> Resized {
    let dx = (i16::from(w) - i16::from(board.width())) / 2;
    let dy = (i16::from(h) - i16::from(board.height())) / 2;
    let to = |x: u8, y: u8| -> Option<(u8, u8)> {
        let nx = u8::try_from(i16::from(x) + dx).ok().filter(|&nx| nx < w)?;
        let ny = u8::try_from(i16::from(y) + dy).ok().filter(|&ny| ny < h)?;
        Some((nx, ny))
    };
    let mut out = Board::new(w, h, board.seed());
    *out.rules_mut() = *board.rules();
    out.set_wrap(board.wrap());
    out.set_events_enabled(board.events_enabled());
    for (player, &score) in board.scores().iter().enumerate() {
        out.set_score(player as u8, score);
    }
    let mut cut = false;
    for (x, y, kind) in board.tiles() {
        match to(x, y) {
            Some((nx, ny)) => out.set_tile(nx, ny, kind),
            None => cut |= kind != TileKind::Empty,
        }
    }
    // Each edge inside the old beach once, from the tile on its left or
    // above, and kept from whichever of its two tiles survives.
    for (x, y, _) in board.tiles() {
        for dir in [Direction::Right, Direction::Down] {
            let (ox, oy) = dir.offset();
            let (Ok(bx), Ok(by)) = (
                u8::try_from(i32::from(x) + ox),
                u8::try_from(i32::from(y) + oy),
            ) else {
                continue;
            };
            if bx >= board.width() || by >= board.height() || !board.wall_at(x, y, dir) {
                continue;
            }
            match (to(x, y), to(bx, by)) {
                (Some((nx, ny)), _) => out.set_wall(nx, ny, dir, true),
                (None, Some((nx, ny))) => out.set_wall(nx, ny, dir.right().right(), true),
                (None, None) => cut = true,
            }
        }
    }
    for crab in board.crabs() {
        let (x, y) = board.coords_u8(crab.tile);
        match to(x, y) {
            Some((nx, ny)) => out.spawn_crab(nx, ny, crab.dir, crab.handed, crab.kind),
            None => cut = true,
        }
    }
    for gull in board.gulls() {
        let (x, y) = board.coords_u8(gull.tile);
        match to(x, y) {
            Some((nx, ny)) => out.spawn_gull(nx, ny, gull.dir),
            None => cut = true,
        }
    }
    Resized {
        board: out,
        shift: (dx, dy),
        cut,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{CrabKind, Handedness};

    /// Grown and shrunk back, a level built in the middle comes back as it
    /// was: tiles, walls, crabs and gulls, and the fence is the new board's
    /// own rather than the old one carried inward.
    #[test]
    fn a_resize_keeps_the_level_centred() {
        let mut board = Board::new(9, 7, 5);
        board.set_tile(4, 3, TileKind::Castle(0));
        board.set_tile(2, 2, TileKind::Rock);
        board.set_wall(4, 3, Direction::Left, true);
        board.spawn_crab(6, 3, Direction::Left, Handedness::Right, CrabKind::Giant);
        board.spawn_gull(3, 5, Direction::Up);
        let grown = resized(&board, (12, 9));
        assert!(!grown.cut);
        assert_eq!(grown.shift, (1, 1));
        let big = &grown.board;
        assert_eq!(big.tile_at(5, 4), TileKind::Castle(0));
        assert_eq!(big.tile_at(3, 3), TileKind::Rock);
        assert!(big.wall_at(5, 4, Direction::Left));
        assert!(
            !big.wall_at(1, 4, Direction::Left),
            "the old fence stays out"
        );
        assert!(big.wall_at(0, 4, Direction::Left), "the new fence is up");
        assert_eq!(big.coords_u8(big.crabs()[0].tile), (7, 4));
        assert_eq!(big.crabs()[0].kind, CrabKind::Giant);
        assert_eq!(big.coords_u8(big.gulls()[0].tile), (4, 6));
        let back = resized(big, (9, 7));
        assert!(!back.cut);
        assert_eq!(back.board.tile_at(4, 3), TileKind::Castle(0));
        assert!(back.board.wall_at(4, 3, Direction::Left));
    }

    /// What the level's text reads off the board besides what stands on
    /// it comes along: `events: on` was dropped, and F2, F3 and the
    /// playtest handed on a level without tide events.
    #[test]
    fn a_resize_keeps_what_the_board_says_of_the_level() {
        let mut board = Board::new(9, 7, 5);
        board.set_events_enabled(true);
        board.set_wrap(true);
        board.set_score(1, 7);
        board.rules_mut().golden_every = 40;
        let level = |board: &Board| {
            crate::sim::Level::from_board("R", 1, board.clone())
                .to_text()
                .lines()
                .take_while(|&line| line != "map:")
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let grown = resized(&board, (12, 9)).board;
        assert!(grown.events_enabled());
        assert!(level(&board).iter().any(|line| line == "events: on"));
        assert_eq!(level(&grown), level(&board));
    }

    /// Shrinking past what is built says so, which is the author's cue
    /// that Z brings it back.
    #[test]
    fn a_shrink_that_cuts_says_so() {
        let mut board = Board::new(20, 13, 5);
        board.set_tile(0, 0, TileKind::Rock);
        assert!(resized(&board, (9, 7)).cut);
        let mut walled = Board::new(20, 13, 5);
        walled.set_wall(0, 0, Direction::Right, true);
        assert!(resized(&walled, (9, 7)).cut);
        assert!(!resized(&Board::new(20, 13, 5), (9, 7)).cut);
    }

    /// The sizes step both ways and wrap, and a size off the list joins it
    /// at the size beside it the way it is stepped.
    #[test]
    fn the_sizes_step_both_ways() {
        assert_eq!(next_size((9, 7), true), (12, 9));
        assert_eq!(next_size((9, 7), false), (20, 13));
        assert_eq!(next_size((20, 13), true), (9, 7));
        assert_eq!(next_size((10, 10), true), (12, 9));
        assert_eq!(next_size((10, 10), false), (9, 7));
        assert_eq!(
            next_size((6, 5), false),
            (20, 13),
            "below the smallest, round"
        );
    }
}
