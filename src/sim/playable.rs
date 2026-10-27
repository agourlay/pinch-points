//! Whether a versus beach can be played: every castle reachable on foot
//! from every spawner hole, no sand cut off from all of them, and no castle
//! boxed in. The generator's styles retry a layout that fails it, and the
//! tests hold every shipped map and every style to it.

use crate::sim::board::{Board, PlayerId, TileKind};
use crate::sim::direction::Direction;

/// One way a beach cannot be played.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unplayable {
    /// No spawner hole: no crab ever arrives.
    NoHoles,
    /// A castle no crab from this hole can walk to, so no arrow can bring
    /// one home from it.
    CastleUnreachable {
        owner: PlayerId,
        at: (u8, u8),
        hole: (u8, u8),
    },
    /// Sand no hole's crabs can reach: a pocket where a gull may land and
    /// nothing else happens.
    CutOff { tiles: u32 },
    /// A castle with fewer than two open sides: crabs reach it down one
    /// lane only, and a single arrow shuts it.
    BoxedIn {
        owner: PlayerId,
        at: (u8, u8),
        sides: u32,
    },
}

impl std::fmt::Display for Unplayable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unplayable::NoHoles => write!(f, "no spawner holes"),
            Unplayable::CastleUnreachable { owner, at, hole } => {
                write!(f, "castle {owner} at {at:?} unreachable from hole {hole:?}")
            }
            Unplayable::CutOff { tiles } => write!(f, "{tiles} tiles cut off from every hole"),
            Unplayable::BoxedIn { owner, at, sides } => {
                write!(f, "castle {owner} at {at:?} has {sides} open side(s)")
            }
        }
    }
}

/// The tile a walker steps onto leaving `(x, y)` toward `dir`, if it can:
/// no wall on the edge, a tile beyond it (across the seam on an open
/// beach), and not a rock. Kelp, pools and logs are walked through.
fn neighbour(board: &Board, x: u8, y: u8, dir: Direction) -> Option<(u8, u8)> {
    if board.wall_at(x, y, dir) {
        return None;
    }
    let next = board.step(board.index_of(x, y), dir)?;
    let (nx, ny) = board.coords_u8(next);
    // An edge is a wall from either side.
    if board.wall_at(nx, ny, dir.reverse()) {
        return None;
    }
    (board.tile_at(nx, ny) != TileKind::Rock).then_some((nx, ny))
}

/// Every tile a walker reaches from `from`.
fn reach(board: &Board, from: &[(u8, u8)]) -> Vec<bool> {
    let w = usize::from(board.width());
    let mut seen = vec![false; w * usize::from(board.height())];
    let mut queue: Vec<(u8, u8)> = from.to_vec();
    for &(x, y) in from {
        seen[usize::from(y) * w + usize::from(x)] = true;
    }
    while let Some((x, y)) = queue.pop() {
        for dir in Direction::ALL {
            if let Some((nx, ny)) = neighbour(board, x, y, dir) {
                let i = usize::from(ny) * w + usize::from(nx);
                if !seen[i] {
                    seen[i] = true;
                    queue.push((nx, ny));
                }
            }
        }
    }
    seen
}

/// How many steps a crab out of the hole at `from`, facing `facing`, takes
/// to each tile, `None` for a tile it never reaches: breadth first, so
/// each count is the shortest.
///
/// Walked as the sim walks a crab, as far as a count of steps can: it
/// leaves the hole the way the hole faces when that way is open (turned
/// only by a wall, as the sim turns it), and a castle is where a walk
/// ends, any seat's, since a crab walking into one is banked there and
/// goes no further.
fn steps_from(board: &Board, from: (u8, u8), facing: Direction) -> Vec<Option<u32>> {
    let w = usize::from(board.width());
    let index = |(x, y): (u8, u8)| usize::from(y) * w + usize::from(x);
    let mut steps = vec![None; w * usize::from(board.height())];
    let mut queue = std::collections::VecDeque::new();
    // Out the way it faces, from where it may come back over the hole; or
    // turned by a wall, any way open.
    match neighbour(board, from.0, from.1, facing) {
        Some(first) => {
            steps[index(first)] = Some(1);
            queue.push_back((first, 1u32));
        }
        None => {
            steps[index(from)] = Some(0);
            queue.push_back((from, 0u32));
        }
    }
    while let Some(((x, y), walked)) = queue.pop_front() {
        if matches!(board.tile_at(x, y), TileKind::Castle(_)) {
            continue;
        }
        for dir in Direction::ALL {
            if let Some(next) = neighbour(board, x, y, dir) {
                let i = index(next);
                if steps[i].is_none() {
                    steps[i] = Some(walked + 1);
                    queue.push_back((next, walked + 1));
                }
            }
        }
    }
    steps
}

/// How far each seat's castles stand from the holes: for every seat with
/// a castle, in seat order, the shortest walk from each hole to the
/// nearest of that seat's castles, `None` where the hole's crabs cannot
/// reach any of them. Holes in the order [`Board::tiles`] reads them.
///
/// The measure of a beach's fairness the editor gives: a seat whose
/// castles are further from where the crabs come out has to route them
/// further, and on a beach drawn the same for every seat every seat's
/// walks are the same numbers.
pub fn seat_walks(board: &Board) -> Vec<(PlayerId, Vec<Option<u32>>)> {
    let w = usize::from(board.width());
    let holes: Vec<Vec<Option<u32>>> = board
        .tiles()
        .filter_map(|(x, y, kind)| match kind {
            TileKind::Spawner(hole) => Some(steps_from(board, (x, y), hole.dir)),
            TileKind::Empty
            | TileKind::Rock
            | TileKind::Castle(_)
            | TileKind::Turnstile { .. }
            | TileKind::Kelp
            | TileKind::Pool => None,
        })
        .collect();
    let mut seats: Vec<(PlayerId, Vec<usize>)> = Vec::new();
    for (x, y, kind) in board.tiles() {
        if let TileKind::Castle(owner) = kind {
            let at = usize::from(y) * w + usize::from(x);
            match seats.iter_mut().find(|(seat, _)| *seat == owner) {
                Some((_, castles)) => castles.push(at),
                None => seats.push((owner, vec![at])),
            }
        }
    }
    seats.sort_by_key(|(seat, _)| *seat);
    seats
        .into_iter()
        .map(|(seat, castles)| {
            let walks = holes
                .iter()
                .map(|steps| castles.iter().filter_map(|&at| steps[at]).min())
                .collect();
            (seat, walks)
        })
        .collect()
}

/// Everything that keeps `board` from being a fair beach to play, empty
/// when nothing does.
pub fn unplayable(board: &Board) -> Vec<Unplayable> {
    let w = usize::from(board.width());
    let at = |x: u8, y: u8| usize::from(y) * w + usize::from(x);
    let holes: Vec<(u8, u8)> = board
        .tiles()
        .filter(|(_, _, kind)| matches!(kind, TileKind::Spawner(_)))
        .map(|(x, y, _)| (x, y))
        .collect();
    if holes.is_empty() {
        return vec![Unplayable::NoHoles];
    }
    let castles: Vec<(u8, u8, PlayerId)> = board
        .tiles()
        .filter_map(|(x, y, kind)| match kind {
            TileKind::Castle(owner) => Some((x, y, owner)),
            TileKind::Empty
            | TileKind::Rock
            | TileKind::Spawner(_)
            | TileKind::Turnstile { .. }
            | TileKind::Kelp
            | TileKind::Pool => None,
        })
        .collect();
    let mut problems = Vec::new();
    // Every castle from every hole: an arrow can route a crab anywhere it
    // can walk, and nowhere it cannot.
    for &hole in &holes {
        let seen = reach(board, &[hole]);
        for &(x, y, owner) in &castles {
            if !seen[at(x, y)] {
                problems.push(Unplayable::CastleUnreachable {
                    owner,
                    at: (x, y),
                    hole,
                });
            }
        }
    }
    let seen = reach(board, &holes);
    let cut_off = board
        .tiles()
        .filter(|&(x, y, kind)| kind != TileKind::Rock && !seen[at(x, y)])
        .count() as u32;
    if cut_off > 0 {
        problems.push(Unplayable::CutOff { tiles: cut_off });
    }
    for &(x, y, owner) in &castles {
        let sides = Direction::ALL
            .into_iter()
            .filter(|&dir| neighbour(board, x, y, dir).is_some())
            .count() as u32;
        if sides < 2 {
            problems.push(Unplayable::BoxedIn {
                owner,
                at: (x, y),
                sides,
            });
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each seat's walk from each hole is the shortest way to its nearest
    /// castle, round a wall rather than through it, and a hole that cannot
    /// reach a seat says so.
    #[test]
    fn the_walks_go_round_walls_to_the_nearest_castle() {
        let mut board = Board::new(7, 3, 1);
        let hole = TileKind::Spawner(crate::sim::Spawner {
            dir: Direction::Left,
            period: 40,
        });
        board.set_tile(3, 1, hole);
        board.set_tile(0, 1, TileKind::Castle(0));
        board.set_tile(6, 0, TileKind::Castle(1));
        board.set_tile(6, 2, TileKind::Castle(1));
        assert_eq!(
            seat_walks(&board),
            vec![(0, vec![Some(3)]), (1, vec![Some(6)])]
        );
        // A wall straight left of the hole sends the walk round it.
        board.set_wall(3, 1, Direction::Left, true);
        assert_eq!(seat_walks(&board)[0], (0, vec![Some(5)]));
        // Rocks round P2's two castles shut it out.
        for (x, y) in [(5, 0), (5, 1), (5, 2)] {
            board.set_tile(x, y, TileKind::Rock);
        }
        assert_eq!(seat_walks(&board)[1], (1, vec![None]));
    }

    /// A walk does not pass through another seat's castle, where the sim
    /// banks the crab, and it leaves a hole the way the hole faces.
    #[test]
    fn the_walks_end_at_castles_and_leave_holes_facing_their_way() {
        let mut board = Board::new(7, 1, 1);
        let hole = |dir| TileKind::Spawner(crate::sim::Spawner { dir, period: 40 });
        board.set_tile(0, 0, hole(Direction::Right));
        board.set_tile(3, 0, TileKind::Castle(0));
        board.set_tile(6, 0, TileKind::Castle(1));
        assert_eq!(
            seat_walks(&board),
            vec![(0, vec![Some(3)]), (1, vec![None])]
        );
        // A hole in the middle facing right sends its crabs right first.
        let mut board = Board::new(7, 1, 1);
        board.set_tile(3, 0, hole(Direction::Right));
        board.set_tile(0, 0, TileKind::Castle(0));
        board.set_tile(6, 0, TileKind::Castle(1));
        assert_eq!(
            seat_walks(&board),
            vec![(0, vec![Some(5)]), (1, vec![Some(3)])]
        );
    }

    /// Each problem is seen, and a plain beach has none.
    #[test]
    fn each_problem_is_seen() {
        let mut board = Board::new(5, 3, 1);
        assert_eq!(unplayable(&board), vec![Unplayable::NoHoles]);
        board.set_tile(
            0,
            1,
            TileKind::Spawner(crate::sim::Spawner {
                dir: Direction::Right,
                period: 40,
            }),
        );
        board.set_tile(4, 1, TileKind::Castle(0));
        assert!(unplayable(&board).is_empty(), "{:?}", unplayable(&board));
        // A rock wall down column 3 cuts the castle off, and the sand
        // behind it with it.
        for y in 0..3 {
            board.set_tile(3, y, TileKind::Rock);
        }
        let problems = unplayable(&board);
        assert!(
            problems
                .iter()
                .any(|p| matches!(p, Unplayable::CastleUnreachable { .. }))
        );
        assert!(
            problems.contains(&Unplayable::CutOff { tiles: 3 }),
            "{problems:?}"
        );
        // Opened again, but walled on three sides: one lane in.
        board.set_tile(3, 1, TileKind::Empty);
        board.set_tile(3, 0, TileKind::Empty);
        board.set_tile(3, 2, TileKind::Empty);
        board.set_wall(4, 1, Direction::Up, true);
        board.set_wall(4, 1, Direction::Down, true);
        assert_eq!(
            unplayable(&board),
            vec![Unplayable::BoxedIn {
                owner: 0,
                at: (4, 1),
                sides: 1
            }]
        );
    }
}
