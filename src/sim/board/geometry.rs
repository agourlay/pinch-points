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
/// walking gull cannot enter kelp and a crab slips through) but that one
/// rule reaches every wall resolution, and `true` at a call site says
/// nothing about which way round it goes.
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
    /// arrival is settled. One body for crabs and gulls, so the deflection rule cannot
    /// drift between them.
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
