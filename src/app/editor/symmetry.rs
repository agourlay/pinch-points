//! Mirror painting: whatever the author does to a tile, a wall or a
//! creature is done to its images too, so a fair beach is drawn once
//! rather than four times by hand. A castle's copy goes to the seat that
//! plays that side of the beach, the way the versus deal pairs them
//! (castles 0/1, 2/3 and 4/5 across from each other; see
//! [`crate::sim::deal_pairs`]).

use crate::sim::{Board, Direction, Handedness, Orientation, TileKind, mirror_dir, mirrored_tile};

/// Which images of a tile the bench paints along with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Symmetry {
    /// Only the tile painted.
    #[default]
    Off,
    /// The tile's image across the line down the middle.
    LeftRight,
    /// The tile's image across the line across the middle.
    TopBottom,
    /// Both mirrors and the half turn they make together: four copies.
    Both,
    /// The tile's image turned half way round the middle.
    HalfTurn,
}

impl Symmetry {
    /// The dial's stops, off first.
    pub(super) const ALL: [Symmetry; 5] = [
        Symmetry::Off,
        Symmetry::LeftRight,
        Symmetry::TopBottom,
        Symmetry::Both,
        Symmetry::HalfTurn,
    ];

    /// The mirrors that make each copy past the tile itself, as
    /// `(flip_x, flip_y)`: both flipped is the half turn.
    pub(super) fn flips(self) -> &'static [(bool, bool)] {
        match self {
            Symmetry::Off => &[],
            Symmetry::LeftRight => &[(true, false)],
            Symmetry::TopBottom => &[(false, true)],
            Symmetry::Both => &[(true, false), (false, true), (true, true)],
            Symmetry::HalfTurn => &[(true, true)],
        }
    }

    /// Whether a resize that moves the beach's middle half a tile across
    /// (`odd_x`, an odd change of width) or down (`odd_y`) moves it off a
    /// line this mirror paints across.
    pub(super) fn moves_off_centre(self, odd_x: bool, odd_y: bool) -> bool {
        self.flips()
            .iter()
            .any(|&(flip_x, flip_y)| (flip_x && odd_x) || (flip_y && odd_y))
    }

    /// Its place on the dial, which is its name's place in
    /// `Tr::ed_mirrors`.
    pub(super) fn index(self) -> usize {
        match self {
            Symmetry::Off => 0,
            Symmetry::LeftRight => 1,
            Symmetry::TopBottom => 2,
            Symmetry::Both => 3,
            Symmetry::HalfTurn => 4,
        }
    }

    /// The seat whose castle stands in the copy `flips` makes of a castle
    /// of `owner`'s.
    ///
    /// Two copies pair the seats as the deal does, 0 with 1, 2 with 3 and
    /// 4 with 5, so a castle painted for P1 is P2's across the line. Four
    /// copies give the first four seats a corner each, in the order the
    /// generated beaches put them ([`crate::sim::castle_spots`]: P1 top
    /// left, P2 bottom right, P3 top right, P4 bottom left), and seats 5
    /// and 6 the pair along the long edges, top and bottom. Each is a
    /// group action: a copy of a copy is the copy the two mirrors make
    /// together, so painting on any copy gives the same beach.
    pub(super) fn seat(self, (flip_x, flip_y): (bool, bool), owner: u8) -> u8 {
        let pair = |owner: u8| match usize::from(owner) < crate::sim::MAX_PLAYERS {
            true => owner ^ 1,
            false => owner,
        };
        match self {
            Symmetry::Off => owner,
            Symmetry::LeftRight | Symmetry::TopBottom | Symmetry::HalfTurn => pair(owner),
            Symmetry::Both => match owner {
                0..=3 => owner ^ ((u8::from(flip_x) * 2) ^ (u8::from(flip_y) * 3)),
                _ if flip_y => pair(owner),
                _ => owner,
            },
        }
    }

    /// Where the copies of `(x, y)` stand, each with the mirrors that make
    /// it: the tile itself left out, and a tile two copies land on given
    /// to the first of them, so a tile on a mirror's line is painted once.
    fn images(self, board: &Board, at: (u8, u8)) -> Vec<((u8, u8), (bool, bool))> {
        let mut out: Vec<((u8, u8), (bool, bool))> = Vec::new();
        for &flips in self.flips() {
            let image = place(board, flips, at);
            if image != at && out.iter().all(|(seen, _)| *seen != image) {
                out.push((image, flips));
            }
        }
        out
    }
}

/// Where the mirrors `flips` put the tile at `at`.
fn place(board: &Board, (flip_x, flip_y): (bool, bool), at: (u8, u8)) -> (u8, u8) {
    Orientation {
        flip_x,
        flip_y,
        roll: 0,
    }
    .place(at, board.width(), board.height())
}

/// Why a beach's tile cannot be mirrored fairly ([`unfair`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Unfair {
    /// A castle or a hole on a mirror's line: the tile is its own copy,
    /// and the castle would have to be two seats', the hole face two ways.
    Line,
    /// A log, or a crab or gull facing across, on a mirror's line: its own
    /// copy, the log would have to deal both ways first and the creature
    /// walk or fly both ways.
    Across,
    /// A castle whose copies give one seat two castles: P5 and P6 under
    /// both mirrors, anywhere but the middle column.
    SeatTwice,
}

/// What on a tile [`unfair`] weighs: the change being made decides, so a
/// turn of a crab is not refused for the hole it stands on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Layer {
    /// The ground and whatever stands on it: what a brush paints.
    Both,
    /// The ground alone: a hole turned.
    Ground,
    /// The crabs and gulls alone: a creature turned.
    Creatures,
}

/// Whether the tile at `at` of a beach stands so that its copies cannot be
/// fair to every seat, weighing `layer` of it: a castle every copy of
/// which must be one seat's own, a hole or a log every copy of which must
/// face or deal its own mirrored way, or a crab or gull every copy of
/// which must walk its own mirrored way. A tile that is its own copy has
/// to be the copy too (a castle the seat the copy is given, a hole, a log
/// or a creature turned the mirrored way), and two copies of a castle must
/// be two seats'. Painted anyway, the mirror made a beach one seat played
/// with a castle more, or a hole, a log or a crab that sent crabs to one
/// side.
pub(super) fn unfair(
    board: &Board,
    symmetry: Symmetry,
    at: (u8, u8),
    layer: Layer,
) -> Option<Unfair> {
    let ground = || unfair_ground(board, symmetry, at);
    let creatures = || unfair_creatures(board, symmetry, at);
    match layer {
        Layer::Both => ground().or_else(creatures),
        Layer::Ground => ground(),
        Layer::Creatures => creatures(),
    }
}

/// Each copy of `at` with the mirrors that make it, the tile itself first
/// (no mirror).
fn copies(
    board: &Board,
    symmetry: Symmetry,
    at: (u8, u8),
) -> impl Iterator<Item = ((bool, bool), (u8, u8))> + '_ {
    std::iter::once((false, false))
        .chain(symmetry.flips().iter().copied())
        .map(move |flips| (flips, place(board, flips, at)))
}

/// [`unfair`] of the ground at `at`.
fn unfair_ground(board: &Board, symmetry: Symmetry, at: (u8, u8)) -> Option<Unfair> {
    let (x, y) = at;
    let ground = board.tile_at(x, y);
    // A tile that is its own copy under a mirror that changes it.
    let turned_on_line = || {
        copies(board, symmetry, at)
            .any(|((fx, fy), image)| image == at && mirrored_tile(ground, fx, fy) != ground)
    };
    match ground {
        TileKind::Castle(owner) => {
            let seated: Vec<((u8, u8), u8)> = copies(board, symmetry, at)
                .map(|(flips, image)| {
                    let seat = match flips {
                        (false, false) => owner,
                        flips => symmetry.seat(flips, owner),
                    };
                    (image, seat)
                })
                .collect();
            let line = seated
                .iter()
                .any(|&(image, seat)| image == at && seat != owner);
            let twice = seated.iter().enumerate().any(|(i, &(a, seat))| {
                seated[..i]
                    .iter()
                    .any(|&(b, other)| a != b && seat == other)
            });
            match (line, twice) {
                (true, _) => Some(Unfair::Line),
                (false, true) => Some(Unfair::SeatTwice),
                (false, false) => None,
            }
        }
        TileKind::Spawner(_) => turned_on_line().then_some(Unfair::Line),
        TileKind::Turnstile { .. } => turned_on_line().then_some(Unfair::Across),
        TileKind::Empty | TileKind::Rock | TileKind::Kelp | TileKind::Pool => None,
    }
}

/// [`unfair`] of the crabs and gulls at `at`.
fn unfair_creatures(board: &Board, symmetry: Symmetry, at: (u8, u8)) -> Option<Unfair> {
    let tile = board.index_of(at.0, at.1);
    let crabs = board
        .crabs()
        .iter()
        .filter(|c| c.tile == tile)
        .map(|c| c.dir);
    let gulls = board
        .gulls()
        .iter()
        .filter(|g| g.tile == tile)
        .map(|g| g.dir);
    let mut facings = crabs.chain(gulls);
    facings
        .any(|dir| {
            copies(board, symmetry, at)
                .any(|((fx, fy), image)| image == at && mirror_dir(dir, fx, fy) != dir)
        })
        .then_some(Unfair::Across)
}

/// Make every copy of the tile at `at` what the tile now is: its ground
/// mirrored (a hole facing the mirrored way, a log dealing the other way
/// first), a castle given to its copy's seat when `seats` (a beach's
/// castles belong to seats; a stage's all bank alike), and its crab and
/// gull set down facing the mirrored way, a crab on the other claw where a
/// single mirror swaps its hands. The walls round the tile are left to
/// [`mirror_wall`], since a brush never changes them. Says which tiles it
/// wrote.
pub(super) fn mirror_tile(
    board: &mut Board,
    symmetry: Symmetry,
    seats: bool,
    at: (u8, u8),
) -> Vec<(u8, u8)> {
    let (x, y) = at;
    let tile = board.index_of(x, y);
    let ground = board.tile_at(x, y);
    let crabs: Vec<_> = board
        .crabs()
        .iter()
        .filter(|c| c.tile == tile)
        .copied()
        .collect();
    let gulls: Vec<Direction> = board
        .gulls()
        .iter()
        .filter(|g| g.tile == tile)
        .map(|g| g.dir)
        .collect();
    let images = symmetry.images(board, at);
    for &((ix, iy), (flip_x, flip_y)) in &images {
        let kind = match mirrored_tile(ground, flip_x, flip_y) {
            TileKind::Castle(owner) if seats => {
                TileKind::Castle(symmetry.seat((flip_x, flip_y), owner))
            }
            kind @ (TileKind::Castle(_)
            | TileKind::Empty
            | TileKind::Rock
            | TileKind::Spawner(_)
            | TileKind::Turnstile { .. }
            | TileKind::Kelp
            | TileKind::Pool) => kind,
        };
        board.remove_crabs_at(ix, iy);
        board.remove_gulls_at(ix, iy);
        board.set_tile(ix, iy, kind);
        for crab in &crabs {
            let handed = match (flip_x ^ flip_y, crab.handed) {
                (false, handed) => handed,
                (true, Handedness::Left) => Handedness::Right,
                (true, Handedness::Right) => Handedness::Left,
            };
            let dir = mirror_dir(crab.dir, flip_x, flip_y);
            board.spawn_crab(ix, iy, dir, handed, crab.kind);
        }
        for &dir in &gulls {
            if board.gull_may_stand(ix, iy) {
                board.spawn_gull(ix, iy, mirror_dir(dir, flip_x, flip_y));
            }
        }
    }
    images.into_iter().map(|(image, _)| image).collect()
}

/// Put the wall on `dir`'s side of `at`, or take it away, on every copy
/// as it now stands on the tile itself. A tile on a mirror's line is its
/// own copy, and its wall's copy is on the tile's other side.
pub(super) fn mirror_wall(board: &mut Board, symmetry: Symmetry, at: (u8, u8), dir: Direction) {
    let present = board.wall_at(at.0, at.1, dir);
    for &(flip_x, flip_y) in symmetry.flips() {
        let (ix, iy) = place(board, (flip_x, flip_y), at);
        let side = mirror_dir(dir, flip_x, flip_y);
        if ((ix, iy), side) != (at, dir) {
            board.set_wall(ix, iy, side, present);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{CrabKind, Spawner};

    /// Each mode's seat map is a group action: the copy a mirror makes of
    /// a copy is the copy the two mirrors make together, so a castle
    /// painted on any copy gives the same beach. And each copy of a seat
    /// is a seat of its own, never two seats on one.
    #[test]
    fn the_seat_maps_compose_like_the_mirrors() {
        for symmetry in Symmetry::ALL {
            for &a in symmetry.flips() {
                for owner in 0..crate::sim::MAX_PLAYERS as u8 {
                    let back = symmetry.seat(a, symmetry.seat(a, owner));
                    assert_eq!(back, owner, "{symmetry:?} {a:?} undoes itself");
                    for &b in symmetry.flips() {
                        let both = (a.0 ^ b.0, a.1 ^ b.1);
                        let composed = match both {
                            (false, false) => owner,
                            flips => symmetry.seat(flips, owner),
                        };
                        assert_eq!(
                            symmetry.seat(b, symmetry.seat(a, owner)),
                            composed,
                            "{symmetry:?} {a:?} then {b:?} on {owner}"
                        );
                    }
                }
            }
        }
        // Four copies of P1's corner castle seat the first four players.
        let mut seats: Vec<u8> = Symmetry::Both
            .flips()
            .iter()
            .map(|&flips| Symmetry::Both.seat(flips, 0))
            .collect();
        seats.sort_unstable();
        assert_eq!(seats, [1, 2, 3]);
    }

    /// A castle, a hole, a log and a crab painted at a corner land on
    /// every copy mirrored to suit: the castle another seat's, the hole
    /// facing the mirrored way, the crab walking it on the other claw.
    #[test]
    fn a_tile_lands_on_every_copy_mirrored() {
        let mut board = Board::new(6, 5, 1);
        board.set_tile(0, 0, TileKind::Castle(0));
        mirror_tile(&mut board, Symmetry::Both, true, (0, 0));
        assert_eq!(board.tile_at(5, 0), TileKind::Castle(2));
        assert_eq!(board.tile_at(0, 4), TileKind::Castle(3));
        assert_eq!(board.tile_at(5, 4), TileKind::Castle(1));

        let hole = |dir| TileKind::Spawner(Spawner { dir, period: 60 });
        board.set_tile(1, 1, hole(Direction::Right));
        mirror_tile(&mut board, Symmetry::LeftRight, true, (1, 1));
        assert_eq!(board.tile_at(4, 1), hole(Direction::Left));

        board.spawn_crab(2, 3, Direction::Up, Handedness::Left, CrabKind::Giant);
        mirror_tile(&mut board, Symmetry::TopBottom, true, (2, 3));
        let copy = board
            .crabs()
            .iter()
            .find(|c| c.tile == board.index_of(2, 1))
            .expect("the crab's copy");
        assert_eq!(
            (copy.dir, copy.handed, copy.kind),
            (Direction::Down, Handedness::Right, CrabKind::Giant)
        );

        // A stage's castles all bank alike: the copy keeps its seat.
        let mut stage = Board::new(6, 5, 1);
        stage.set_tile(0, 2, TileKind::Castle(0));
        mirror_tile(&mut stage, Symmetry::HalfTurn, false, (0, 2));
        assert_eq!(stage.tile_at(5, 2), TileKind::Castle(0));
    }

    /// A wall on a tile on the mirror's line is copied to the tile's other
    /// side, and a castle or a hole there that its copy would not match is
    /// unfair, as is a castle whose copies give one seat two.
    #[test]
    fn the_line_mirrors_walls_and_refuses_unfair_tiles() {
        let mut board = Board::new(9, 7, 1);
        board.set_wall(4, 2, Direction::Right, true);
        mirror_wall(&mut board, Symmetry::LeftRight, (4, 2), Direction::Right);
        assert!(board.wall_at(4, 2, Direction::Left), "the other side");
        board.set_wall(4, 3, Direction::Up, true);
        mirror_wall(&mut board, Symmetry::HalfTurn, (4, 3), Direction::Up);
        assert!(board.wall_at(4, 3, Direction::Down), "round the centre");

        board.set_tile(4, 1, TileKind::Castle(0));
        assert_eq!(
            unfair(&board, Symmetry::LeftRight, (4, 1), Layer::Both),
            Some(Unfair::Line)
        );
        assert_eq!(unfair(&board, Symmetry::Off, (4, 1), Layer::Both), None);
        board.set_tile(3, 1, TileKind::Castle(0));
        assert_eq!(
            unfair(&board, Symmetry::LeftRight, (3, 1), Layer::Both),
            None
        );
        // P5 belongs on the middle column under both mirrors, and nowhere
        // else; a beach with no middle column has no place for it.
        board.set_tile(4, 0, TileKind::Castle(4));
        assert_eq!(unfair(&board, Symmetry::Both, (4, 0), Layer::Both), None);
        board.set_tile(2, 0, TileKind::Castle(4));
        assert_eq!(
            unfair(&board, Symmetry::Both, (2, 0), Layer::Both),
            Some(Unfair::SeatTwice)
        );
        let mut even = Board::new(12, 9, 1);
        for x in 0..12 {
            even.set_tile(x, 0, TileKind::Castle(5));
            assert!(
                unfair(&even, Symmetry::Both, (x, 0), Layer::Both).is_some(),
                "x {x}"
            );
        }
        let hole = |dir| TileKind::Spawner(Spawner { dir, period: 60 });
        board.set_tile(4, 5, hole(Direction::Right));
        assert_eq!(
            unfair(&board, Symmetry::LeftRight, (4, 5), Layer::Both),
            Some(Unfair::Line)
        );
        board.set_tile(4, 5, hole(Direction::Down));
        assert_eq!(
            unfair(&board, Symmetry::LeftRight, (4, 5), Layer::Both),
            None
        );
        assert_eq!(
            unfair(&board, Symmetry::TopBottom, (4, 5), Layer::Both),
            None,
            "off its line"
        );
    }

    /// On the line, a log is unfair under a mirror (it would deal both
    /// ways first) and fair under the half turn, which keeps its hand; a
    /// crab or gull is unfair facing across the line and fair along it.
    /// A turn weighs only what it turns: a crab on a hole facing across is
    /// judged by the crab.
    #[test]
    fn the_line_refuses_logs_and_creatures_facing_across() {
        let mut board = Board::new(9, 7, 1);
        board.set_tile(4, 1, TileKind::Turnstile { next_right: true });
        assert_eq!(
            unfair(&board, Symmetry::LeftRight, (4, 1), Layer::Both),
            Some(Unfair::Across)
        );
        assert_eq!(
            unfair(&board, Symmetry::HalfTurn, (4, 3), Layer::Both),
            None
        );
        board.set_tile(4, 3, TileKind::Turnstile { next_right: true });
        assert_eq!(
            unfair(&board, Symmetry::HalfTurn, (4, 3), Layer::Both),
            None
        );
        assert_eq!(
            unfair(&board, Symmetry::LeftRight, (3, 1), Layer::Both),
            None
        );

        board.spawn_crab(4, 4, Direction::Right, Handedness::Left, CrabKind::Common);
        assert_eq!(
            unfair(&board, Symmetry::LeftRight, (4, 4), Layer::Both),
            Some(Unfair::Across)
        );
        assert_eq!(
            unfair(&board, Symmetry::TopBottom, (4, 4), Layer::Both),
            None
        );
        board.remove_crabs_at(4, 4);
        board.spawn_crab(4, 4, Direction::Down, Handedness::Left, CrabKind::Common);
        assert_eq!(
            unfair(&board, Symmetry::LeftRight, (4, 4), Layer::Both),
            None
        );
        board.spawn_gull(4, 5, Direction::Left);
        assert_eq!(
            unfair(&board, Symmetry::LeftRight, (4, 5), Layer::Both),
            Some(Unfair::Across)
        );

        // A crab on a hole facing across: the crab's turn weighs the crab.
        board.set_tile(
            4,
            6,
            TileKind::Spawner(Spawner {
                dir: Direction::Right,
                period: 60,
            }),
        );
        board.spawn_crab(4, 6, Direction::Up, Handedness::Left, CrabKind::Common);
        assert_eq!(
            unfair(&board, Symmetry::LeftRight, (4, 6), Layer::Both),
            Some(Unfair::Line)
        );
        assert_eq!(
            unfair(&board, Symmetry::LeftRight, (4, 6), Layer::Creatures),
            None
        );
    }

    /// A tile on a mirror's line is its own copy, and is left as painted;
    /// and a wall is put up, and taken down, on every copy.
    #[test]
    fn the_line_keeps_its_tile_and_walls_follow() {
        let mut board = Board::new(5, 5, 1);
        board.set_tile(2, 0, TileKind::Castle(4));
        mirror_tile(&mut board, Symmetry::Both, true, (2, 0));
        assert_eq!(board.tile_at(2, 0), TileKind::Castle(4), "its own copy");
        assert_eq!(board.tile_at(2, 4), TileKind::Castle(5));

        board.set_wall(0, 0, Direction::Right, true);
        mirror_wall(&mut board, Symmetry::Both, (0, 0), Direction::Right);
        assert!(board.wall_at(4, 0, Direction::Left));
        assert!(board.wall_at(0, 4, Direction::Right));
        assert!(board.wall_at(4, 4, Direction::Left));
        board.set_wall(0, 0, Direction::Right, false);
        mirror_wall(&mut board, Symmetry::Both, (0, 0), Direction::Right);
        assert!(!board.wall_at(4, 4, Direction::Left));
    }
}
