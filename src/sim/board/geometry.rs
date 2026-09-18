//! The board's geometry: walls, passability, wrapping, and the arithmetic
//! that turns a tile index into coordinates and back. Shared by every
//! creature pass.
//!
//! [`Grid`] holds the beach and answers what needs only the beach: sizes,
//! indices, which edges carry a wall. The rest stays on [`Board`], because
//! passability and neighbours also depend on the wrap switch, and a
//! turnstile crossing mutates a tile.

use super::*;

/// The beach: its size, its walls, and what stands on each tile. Set by
/// level authoring, mutated in play only by turnstile flips and castle
/// swaps.
#[derive(Clone, Debug)]
pub struct Grid {
    pub(super) width: u8,
    pub(super) height: u8,
    /// Horizontal wall segments, `(height + 1)` rows × `width` columns.
    /// `h_walls[y * width + x]` is the edge *above* tile `(x, y)`.
    pub(super) h_walls: Vec<bool>,
    /// Vertical wall segments, `height` rows × `(width + 1)` columns.
    /// `v_walls[y * (width + 1) + x]` is the edge *left of* tile `(x, y)`.
    pub(super) v_walls: Vec<bool>,
    pub(super) tiles: Vec<TileKind>,
}

/// Which creature is asking to move. Only one tile tells them apart (a
/// walking gull cannot enter kelp and a crab slips through), but that rule
/// reaches every wall resolution and `true` at a call site says nothing
/// about which way round it goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Walker {
    Crab,
    Gull,
}

impl Grid {
    pub fn width(&self) -> u8 {
        self.width
    }

    pub fn height(&self) -> u8 {
        self.height
    }

    /// The slot in the wall bitmaps for the edge leaving `(x, y)` toward
    /// `dir`: `true` picks `h_walls`. The one statement of the edge
    /// arithmetic, so a read and a write can never disagree about which
    /// edge a wall stands on.
    fn wall_edge(&self, x: usize, y: usize, dir: Direction) -> (bool, usize) {
        let w = self.width as usize;
        match dir {
            Direction::Up => (true, y * w + x),
            Direction::Down => (true, (y + 1) * w + x),
            Direction::Left => (false, y * (w + 1) + x),
            Direction::Right => (false, y * (w + 1) + x + 1),
        }
    }

    /// Whether a wall stands on the edge leaving `(x, y)` toward `dir`.
    pub(super) fn edge_blocked(&self, x: usize, y: usize, dir: Direction) -> bool {
        let (horizontal, i) = self.wall_edge(x, y, dir);
        if horizontal {
            self.h_walls[i]
        } else {
            self.v_walls[i]
        }
    }

    /// Put a wall on that edge, or take it away.
    pub(super) fn set_edge(&mut self, x: usize, y: usize, dir: Direction, present: bool) {
        let (horizontal, i) = self.wall_edge(x, y, dir);
        if horizontal {
            self.h_walls[i] = present;
        } else {
            self.v_walls[i] = present;
        }
    }

    /// Coordinates folded back onto the beach, for a wrapping arena.
    pub(super) fn wrap_coords(&self, x: i32, y: i32) -> (i32, i32) {
        let w = i32::from(self.width);
        let h = i32::from(self.height);
        ((x % w + w) % w, (y % h + h) % h)
    }

    /// Tile index to `(x, y)`, in the `i32` the movement arithmetic speaks.
    /// This is the only copy of the arithmetic: the bot, the solver, and
    /// the renderers all ask rather than re-derive.
    pub fn coords(&self, tile: u16) -> (i32, i32) {
        (
            i32::from(tile % u16::from(self.width)),
            i32::from(tile / u16::from(self.width)),
        )
    }

    /// `(x, y)` back to the tile index, the inverse of [`Board::coords_u8`].
    pub fn index_of(&self, x: u8, y: u8) -> u16 {
        self.index(i32::from(x), i32::from(y))
    }

    pub(super) fn index(&self, x: i32, y: i32) -> u16 {
        debug_assert!(self.in_bounds(x, y));
        (y * i32::from(self.width) + x) as u16
    }

    pub(super) fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < i32::from(self.width) && y < i32::from(self.height)
    }

    /// Become an exact copy of `other`, keeping the buffers already held.
    /// The grid half of [`Board::copy_from`], which explains why.
    pub(super) fn copy_from(&mut self, other: &Self) {
        let Self {
            width,
            height,
            h_walls,
            v_walls,
            tiles,
        } = other;
        self.width = *width;
        self.height = *height;
        refill(&mut self.h_walls, h_walls);
        refill(&mut self.v_walls, v_walls);
        refill(&mut self.tiles, tiles);
    }
}

impl Board {
    /// Wall resolution for any creature: forward, else preferred side by
    /// handedness, else other side, else reverse.
    pub(super) fn resolve_walls_for(
        &self,
        tile: u16,
        dir: &mut Direction,
        handed: Handedness,
        who: Walker,
    ) {
        let d = *dir;
        if self.passable_for(tile, d, who) {
            return;
        }
        let (first, second) = match handed {
            Handedness::Left => (d.left(), d.right()),
            Handedness::Right => (d.right(), d.left()),
        };
        if self.passable_for(tile, first, who) {
            *dir = first;
        } else if self.passable_for(tile, second, who) {
            *dir = second;
        } else {
            *dir = d.reverse();
        }
    }

    /// Spec §4.1 step 3: forward, else preferred side (the crab's big claw),
    /// else other side, else reverse.
    pub(super) fn resolve_walls(&self, crab: &mut Crab) {
        self.resolve_walls_for(crab.tile, &mut crab.dir, crab.handed, Walker::Crab);
    }

    /// A turnstile physically deflects whoever crosses it, alternating
    /// sides; it overrides lures and signposts. `true` if the tile was one,
    /// in which case the walker's exit is already wall-resolved and its
    /// arrival settled. One body for crabs and gulls, so the deflection
    /// rule cannot drift between them.
    pub(super) fn turnstile_deflect(
        &mut self,
        tile: u16,
        dir: &mut Direction,
        handed: Handedness,
        who: Walker,
    ) -> bool {
        let t = tile as usize;
        let TileKind::Turnstile { next_right } = self.grid.tiles[t] else {
            return false;
        };
        *dir = if next_right { dir.right() } else { dir.left() };
        self.grid.tiles[t] = TileKind::Turnstile {
            next_right: !next_right,
        };
        self.resolve_walls_for(tile, dir, handed, who);
        true
    }

    /// Can a creature step from `tile` in `dir`: no wall on that edge, the
    /// neighbour exists, and the neighbour is not a rock.
    pub(super) fn passable(&self, tile: u16, dir: Direction) -> bool {
        self.passable_for(tile, dir, Walker::Crab)
    }

    pub(super) fn edge_blocked(&self, x: usize, y: usize, dir: Direction) -> bool {
        self.grid.edge_blocked(x, y, dir)
    }

    pub(super) fn set_edge(&mut self, x: usize, y: usize, dir: Direction, present: bool) {
        self.grid.set_edge(x, y, dir, present);
    }

    /// Whether a creature at `tile` may exit toward `dir`. Kelp lets crabs
    /// slip through but blocks walking gulls.
    pub(super) fn passable_for(&self, tile: u16, dir: Direction, who: Walker) -> bool {
        let (x, y) = self.coords(tile);
        if self.edge_blocked(x as usize, y as usize, dir) {
            return false;
        }
        let (dx, dy) = dir.offset();
        let (nx, ny) = (x + dx, y + dy);
        let dest = if !self.in_bounds(nx, ny) {
            if !self.wrap {
                return false;
            }
            let (wx, wy) = self.wrap_coords(nx, ny);
            self.grid.tiles[self.index(wx, wy) as usize]
        } else {
            self.grid.tiles[self.index(nx, ny) as usize]
        };
        match dest {
            TileKind::Rock => false,
            TileKind::Kelp => who == Walker::Crab,
            TileKind::Empty
            | TileKind::Castle(_)
            | TileKind::Spawner(_)
            | TileKind::Turnstile { .. }
            | TileKind::Pool => true,
        }
    }

    /// The open spots, in bounds and on empty sand, along a ring of offsets
    /// around `(cx, cy)`, in ring order, each with the offset it sits at.
    /// The shared half of every castle-ring walk.
    pub(super) fn ring_openings(
        &self,
        cx: i32,
        cy: i32,
        ring: &[(i32, i32)],
    ) -> Vec<(i32, i32, i32, i32)> {
        ring.iter()
            .map(|&(ox, oy)| (cx + ox, cy + oy, ox, oy))
            .filter(|&(nx, ny, _, _)| {
                self.in_bounds(nx, ny)
                    && self.grid.tiles[self.index(nx, ny) as usize] == TileKind::Empty
            })
            .collect()
    }

    pub(super) fn wrap_coords(&self, x: i32, y: i32) -> (i32, i32) {
        self.grid.wrap_coords(x, y)
    }

    pub(super) fn neighbor(&self, tile: u16, dir: Direction) -> u16 {
        let (x, y) = self.coords(tile);
        let (dx, dy) = dir.offset();
        if self.wrap {
            let (wx, wy) = self.wrap_coords(x + dx, y + dy);
            return self.index(wx, wy);
        }
        self.index(x + dx, y + dy)
    }

    /// The tile one step away in `dir`, if there is one: across the seam on
    /// a wrapping arena, `None` off the edge otherwise. The public face of
    /// [`Board::neighbor`] for callers outside the board: the bot plans on
    /// the same beach the crabs walk, seam included.
    pub fn step(&self, tile: u16, dir: Direction) -> Option<u16> {
        let (x, y) = self.coords(tile);
        let (dx, dy) = dir.offset();
        if self.wrap {
            let (wx, wy) = self.wrap_coords(x + dx, y + dy);
            return Some(self.index(wx, wy));
        }
        self.in_bounds(x + dx, y + dy)
            .then(|| self.index(x + dx, y + dy))
    }

    /// Tile index to `(x, y)`, in the `i32` the movement arithmetic speaks;
    /// [`Board::coords_u8`] is the byte-sized form. Forwards to the grid,
    /// which owns the arithmetic.
    pub fn coords(&self, tile: u16) -> (i32, i32) {
        self.grid.coords(tile)
    }

    /// `(x, y)` back to the tile index, the inverse of [`Board::coords_u8`].
    pub fn index_of(&self, x: u8, y: u8) -> u16 {
        self.grid.index_of(x, y)
    }

    pub(super) fn index(&self, x: i32, y: i32) -> u16 {
        self.grid.index(x, y)
    }

    pub(super) fn in_bounds(&self, x: i32, y: i32) -> bool {
        self.grid.in_bounds(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tile index and a pair of coordinates are the same thing said two
    /// ways, and everything the sim does walks between them, so an
    /// off-by-one in either direction puts the whole board a tile out of
    /// step with itself.
    #[test]
    fn an_index_and_a_pair_of_coordinates_are_the_same_place() {
        for (w, h) in [(1u8, 1u8), (2, 3), (9, 7), (21, 13)] {
            let board = Board::new(w, h, 1);
            for y in 0..h {
                for x in 0..w {
                    let tile = board.index_of(x, y);
                    assert_eq!(
                        board.coords(tile),
                        (i32::from(x), i32::from(y)),
                        "{w}x{h}: ({x},{y}) did not survive the trip through {tile}"
                    );
                    assert!(
                        (tile as usize) < usize::from(w) * usize::from(h),
                        "{w}x{h}: ({x},{y}) indexed off the end at {tile}"
                    );
                }
            }
        }
    }

    /// A step off any edge lands where it would on a loop, however far
    /// out the arithmetic goes. The `%` on its own answers negatively for
    /// a negative left-hand side, which is a crab leaving the west side
    /// and arriving at tile minus one.
    #[test]
    fn a_step_off_any_edge_folds_back_onto_the_beach() {
        let board = Board::new(5, 3, 1);
        assert_eq!(board.wrap_coords(-1, -1), (4, 2), "off the top-left corner");
        assert_eq!(board.wrap_coords(5, 3), (0, 0), "and off the bottom-right");
        assert_eq!(board.wrap_coords(2, 1), (2, 1), "somewhere in the middle");
        // Several laps out, which the gull's flight arithmetic can reach.
        assert_eq!(board.wrap_coords(-11, -7), (4, 2));
        assert_eq!(board.wrap_coords(17, 10), (2, 1));
    }

    /// One wall, two tiles: the edge to the right of a tile is the edge to
    /// the left of its neighbour, and `wall_edge` is the only statement of
    /// which slot that is. Were the two to disagree, a crab would walk
    /// through a wall from one side and bounce off it from the other.
    #[test]
    fn a_wall_is_the_same_wall_from_either_side_of_it() {
        let mut board = Board::new(4, 4, 1);
        board.set_wall(1, 2, Direction::Right, true);
        assert!(board.edge_blocked(1, 2, Direction::Right));
        assert!(
            board.edge_blocked(2, 2, Direction::Left),
            "the neighbour is up against the same wall"
        );
        assert!(
            !board.edge_blocked(1, 2, Direction::Left),
            "and only that one"
        );

        board.set_wall(1, 2, Direction::Down, true);
        assert!(board.edge_blocked(1, 2, Direction::Down));
        assert!(
            board.edge_blocked(1, 3, Direction::Up),
            "the tile below is under the same wall"
        );

        // And taking it away from the far side takes away the only wall.
        board.set_wall(2, 2, Direction::Left, false);
        assert!(!board.edge_blocked(1, 2, Direction::Right));
    }

    /// Rock stops everybody; kelp is the one tile that tells a crab from a
    /// gull on foot, which is why `Walker` exists at all.
    #[test]
    fn rock_stops_everyone_and_kelp_stops_only_a_walking_gull() {
        let mut board = Board::new(3, 1, 1);
        board.set_tile(1, 0, TileKind::Rock);
        let from = board.index_of(0, 0);
        assert!(!board.passable_for(from, Direction::Right, Walker::Crab));
        assert!(!board.passable_for(from, Direction::Right, Walker::Gull));

        board.set_tile(1, 0, TileKind::Kelp);
        assert!(
            board.passable_for(from, Direction::Right, Walker::Crab),
            "a crab slips through the kelp"
        );
        assert!(
            !board.passable_for(from, Direction::Right, Walker::Gull),
            "and a gull on foot does not"
        );

        board.set_tile(1, 0, TileKind::Pool);
        assert!(board.passable_for(from, Direction::Right, Walker::Crab));
        assert!(board.passable_for(from, Direction::Right, Walker::Gull));
    }

    /// The edge of a closed beach is a wall; the edge of an open one is
    /// the far side. Both answers come from the same call, which is what
    /// keeps the bot planning on the beach the crabs actually walk.
    #[test]
    fn the_rim_is_a_wall_until_the_beach_wraps() {
        let mut board = Board::new(4, 3, 1);
        let west = board.index_of(0, 1);
        assert!(!board.passable(west, Direction::Left), "closed at the rim");
        assert_eq!(board.step(west, Direction::Left), None);

        board.set_wrap(true);
        assert!(
            board.passable(west, Direction::Left),
            "and open once it wraps"
        );
        assert_eq!(
            board.step(west, Direction::Left),
            Some(board.index_of(3, 1)),
            "straight across to the far side of the same row"
        );
        assert_eq!(
            board.step(board.index_of(1, 0), Direction::Up),
            Some(board.index_of(1, 2)),
            "and the same over the top"
        );
    }

    /// A turnstile turns whoever crosses it, and turns the other way for
    /// the next one. The flip is the whole of the thing: a turnstile that
    /// forgot to alternate would send every crab the same way for ever.
    #[test]
    fn a_turnstile_turns_the_other_way_for_the_next_walker() {
        let mut board = Board::new(3, 3, 1);
        board.set_tile(1, 1, TileKind::Turnstile { next_right: true });
        let tile = board.index_of(1, 1);

        let mut dir = Direction::Up;
        assert!(board.turnstile_deflect(tile, &mut dir, Handedness::Right, Walker::Crab));
        assert_eq!(dir, Direction::Up.right(), "the first one is sent right");

        let mut dir = Direction::Up;
        assert!(board.turnstile_deflect(tile, &mut dir, Handedness::Right, Walker::Crab));
        assert_eq!(dir, Direction::Up.left(), "and the next one the other way");

        // Any other tile is not a turnstile and deflects nobody.
        let mut dir = Direction::Up;
        assert!(!board.turnstile_deflect(
            board.index_of(0, 0),
            &mut dir,
            Handedness::Right,
            Walker::Crab
        ));
        assert_eq!(dir, Direction::Up, "left exactly as it was");
    }

    /// Spec 4.1 step 3, in order: straight on, then the big claw's side,
    /// then the other side, then back the way it came. Handedness only
    /// picks which side is tried first, so the mirror of a board sends a
    /// mirrored crab the mirrored way.
    #[test]
    fn a_blocked_crab_tries_its_big_claw_before_its_small_one() {
        let mut board = Board::new(3, 3, 1);
        let tile = board.index_of(1, 1);
        let mut dir = Direction::Up;
        board.resolve_walls_for(tile, &mut dir, Handedness::Right, Walker::Crab);
        assert_eq!(dir, Direction::Up, "nothing in the way, so straight on");

        board.set_wall(1, 1, Direction::Up, true);
        let mut right = Direction::Up;
        board.resolve_walls_for(tile, &mut right, Handedness::Right, Walker::Crab);
        assert_eq!(
            right,
            Direction::Up.right(),
            "the right-clawed one goes right"
        );
        let mut left = Direction::Up;
        board.resolve_walls_for(tile, &mut left, Handedness::Left, Walker::Crab);
        assert_eq!(left, Direction::Up.left(), "and the left-clawed one left");

        // Its own side shut too, so it takes the other.
        board.set_wall(1, 1, Direction::Up.right(), true);
        let mut dir = Direction::Up;
        board.resolve_walls_for(tile, &mut dir, Handedness::Right, Walker::Crab);
        assert_eq!(dir, Direction::Up.left(), "the small claw's side");

        // Boxed in on three sides: back the way it came.
        board.set_wall(1, 1, Direction::Up.left(), true);
        let mut dir = Direction::Up;
        board.resolve_walls_for(tile, &mut dir, Handedness::Right, Walker::Crab);
        assert_eq!(dir, Direction::Down, "nothing left but the way it came");
    }

    /// The castle-ring walk offers open sand and nothing else: not a tile
    /// off the board, and not one with anything standing on it. Every
    /// spill and every castle scatter is placed through this.
    #[test]
    fn a_ring_offers_only_the_open_sand_that_is_really_there() {
        let mut board = Board::new(3, 3, 1);
        board.set_tile(0, 0, TileKind::Rock);
        board.set_tile(2, 0, TileKind::Castle(0));
        let ring = [(-1, -1), (0, -1), (1, -1), (1, 0), (1, 1)];

        let open = board.ring_openings(1, 1, &ring);
        let places: Vec<(i32, i32)> = open.iter().map(|&(x, y, _, _)| (x, y)).collect();
        assert_eq!(
            places,
            [(1, 0), (2, 1), (2, 2)],
            "the rock and the castle are not sand, and nothing off the board is offered"
        );
        // The offset each one sits at travels with it, in ring order.
        assert_eq!(open[0], (1, 0, 0, -1));

        // From a corner, most of the ring is off the beach.
        let corner = board.ring_openings(0, 0, &ring);
        assert!(
            corner
                .iter()
                .all(|&(x, y, _, _)| (0..3).contains(&x) && (0..3).contains(&y)),
            "nothing off the board: {corner:?}"
        );
    }
}
