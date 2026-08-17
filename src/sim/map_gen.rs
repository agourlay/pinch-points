//! Seeded skirmish-arena generation: every seed is a fresh, fair-ish
//! battleground at a configurable size. Deterministic (one `Pcg32` stream),
//! so a generated arena replays and stays lockstep-safe if shared by seed.

use crate::sim::board::{Board, MAX_PLAYERS, PlayerId, Spawner, TICKS_PER_SECOND, TileKind};
use crate::sim::crab::{CrabKind, Handedness};
use crate::sim::direction::Direction;
use crate::sim::rng::Pcg32;

/// A castle spot per seat for a board of the given size.
///
/// The first four are the corners, each the 180-degree image of the one
/// before it, which makes them interchangeable. Seats five and six
/// sit at the centre of the long edges as another such pair, not the left
/// and right mid-edges, which is where the side spawner holes point, and a
/// castle in front of a hole is fed crabs for free.
///
/// The long-edge pair takes the *centre* column, which is why generated
/// six-seat arenas are widened to an odd width (see [`generate_arena`]). An
/// even width has no centre, so the pair leans one tile nearer two corners
/// than the other two: the largest seat bias this board has had. Centred,
/// the whole castle set mirrors both ways.
///
/// Corners and edges cannot be made the same job, and it is worth saying why
/// rather than trying again: a rectangle's symmetry group has four elements,
/// so its positions fall into orbits of at most four, and six equivalent
/// spots do not exist on one. Four corners and two edge-centres is the
/// closest it comes; the few percent the edges keep is the price of the
/// shape. Handicapping the edge castles to even it up overshoots every way
/// it has been tried.
pub fn castle_spots(width: u8, height: u8) -> [(u8, u8); MAX_PLAYERS] {
    let mid = (width - 1) / 2;
    [
        (1, 1),
        (width - 2, height - 2),
        (width - 2, 1),
        (1, height - 2),
        (mid, 1),
        (width - 1 - mid, height - 2),
    ]
}

/// Whether a board this wide can seat the long-edge castles evenly. Only an
/// odd width has a centre column for them to share.
fn seats_the_long_edges_evenly(width: u8) -> bool {
    !width.is_multiple_of(2)
}

/// The two spawner holes on the side walls, facing in. Self-mirroring: the
/// middle row is the board's own axis (every generated height is odd), and
/// the pair swaps under a left-right flip.
fn side_spawners(width: u8, height: u8) -> [(u8, u8, Direction); 2] {
    [
        (0, height / 2, Direction::Right),
        (width - 1, height / 2, Direction::Left),
    ]
}

/// Four holes in the top and bottom walls, one full mirror group.
fn end_spawners(width: u8, height: u8) -> [(u8, u8, Direction); 4] {
    let col = width / 4;
    [
        (col, 0, Direction::Down),
        (width - 1 - col, 0, Direction::Down),
        (col, height - 1, Direction::Up),
        (width - 1 - col, height - 1, Direction::Up),
    ]
}

/// The four images of a tile under the arena's two mirror symmetries,
/// left-right and top-bottom.
///
/// Every generated feature is placed on all four at once. That is what
/// makes the four corner castles interchangeable: 180-degree symmetry alone
/// only pairs seat 1 with seat 2 and seat 3 with seat 4, and left the other
/// diagonal measurably better off.
fn quad(width: u8, height: u8, x: u8, y: u8) -> [(u8, u8); 4] {
    [
        (x, y),
        (width - 1 - x, y),
        (x, height - 1 - y),
        (width - 1 - x, height - 1 - y),
    ]
}

/// Which flips produce each entry of [`quad`], in order: none, left-right,
/// top-bottom, both (which is the 180-degree rotation).
const QUAD_FLIPS: [(bool, bool); 4] = [(false, false), (true, false), (false, true), (true, true)];

/// A direction seen in a mirror.
fn mirror_dir(dir: Direction, flip_x: bool, flip_y: bool) -> Direction {
    let horizontal = matches!(dir, Direction::Left | Direction::Right);
    if (flip_x && horizontal) || (flip_y && !horizontal) {
        dir.reverse()
    } else {
        dir
    }
}

/// A tile as it looks in a mirror: a spawner faces the other way, and a
/// turnstile's next deflection swaps sides, since a reflection exchanges
/// left and right and a turnstile is the one tile with a handedness.
pub(crate) fn mirrored_tile(kind: TileKind, flip_x: bool, flip_y: bool) -> TileKind {
    let flipped = flip_x ^ flip_y; // two flips are a rotation: chirality survives
    match kind {
        TileKind::Spawner(spawner) => TileKind::Spawner(Spawner {
            dir: mirror_dir(spawner.dir, flip_x, flip_y),
            period: spawner.period,
        }),
        TileKind::Turnstile { next_right } => TileKind::Turnstile {
            next_right: next_right ^ flipped,
        },
        TileKind::Empty
        | TileKind::Rock
        | TileKind::Castle(_)
        | TileKind::Kelp
        | TileKind::Pool => kind,
    }
}

/// Put `kind` on all four images of a tile, each mirrored to suit its
/// corner, but only if every one of them is free. A half-placed group
/// would tilt the board toward a corner, so the whole group is skipped
/// instead.
fn place_quad(board: &mut Board, x: u8, y: u8, kind: TileKind) -> bool {
    let spots = quad(board.width(), board.height(), x, y);
    if spots
        .iter()
        .any(|&(qx, qy)| board.tile_at(qx, qy) != TileKind::Empty)
    {
        return false;
    }
    for (&(qx, qy), (flip_x, flip_y)) in spots.iter().zip(QUAD_FLIPS) {
        board.set_tile(qx, qy, mirrored_tile(kind, flip_x, flip_y));
    }
    true
}

/// The four images of a *wall edge*. An edge is named by the tile on one
/// side of it, so mirroring it means naming the tile on the other side:
/// the right-hand wall of column `x` is the right-hand wall of column
/// `width - 2 - x` once flipped.
fn wall_quad(width: u8, height: u8, x: u8, y: u8, dir: Direction) -> [(u8, u8); 4] {
    let (mx, my) = match dir {
        Direction::Down | Direction::Up => (width - 1 - x, height.saturating_sub(2) - y),
        Direction::Left | Direction::Right => (width.saturating_sub(2) - x, height - 1 - y),
    };
    [(x, y), (mx, y), (x, my), (mx, my)]
}

fn place_wall_quad(board: &mut Board, x: u8, y: u8, dir: Direction) {
    for (wx, wy) in wall_quad(board.width(), board.height(), x, y, dir) {
        board.set_wall(wx, wy, dir, true);
    }
}

/// The handcrafted Turf War arena for 2-4 seats: a corner castle per player,
/// two side spawners feeding the middle, rocks to route around, gulls on a
/// timer, and a 3-minute tide. `preload_scores` stocks two castles for
/// sandbox screenshots.
pub fn classic_arena(preload_scores: bool, seats: u8) -> Board {
    classic_arena_seeded(0x5EA51DE, preload_scores, seats)
}

/// The classic layout on an arbitrary PRNG seed. The layout is identical;
/// only the in-round random stream (spawn kinds, gull entry points, events)
/// differs. Online play uses the canonical seed so peers agree by
/// construction; local play may vary it for round-to-round freshness.
pub fn classic_arena_seeded(seed: u64, preload_scores: bool, seats: u8) -> Board {
    let mut board = Board::new(12, 9, seed);
    // The handcrafted beach is a four-castle board; six seats want a
    // generated arena with room for the long-edge castles.
    for (seat, &(x, y)) in castle_spots(12, 9)
        .iter()
        .enumerate()
        .take(seats.clamp(2, 4) as usize)
    {
        board.set_tile(x, y, TileKind::Castle(seat as PlayerId));
    }
    // Balance: strictly equal cadences. Even a 44/48 stagger measurably
    // tilted bot duels toward the faster side over a full round.
    board.set_tile(
        0,
        4,
        TileKind::Spawner(Spawner {
            dir: Direction::Right,
            period: 46,
        }),
    );
    board.set_tile(
        11,
        4,
        TileKind::Spawner(Spawner {
            dir: Direction::Left,
            period: 46,
        }),
    );
    board.set_tile(5, 4, TileKind::Rock);
    board.set_tile(6, 4, TileKind::Rock);
    // Interior walls, 180-degree rotationally symmetric so both players face
    // the same routing problems: a ledge shielding each castle's quadrant, a
    // spine off each spawner lane, and a baffle per flank.
    for (x, y, dir) in [
        (2u8, 2u8, Direction::Down),
        (3, 2, Direction::Down),
        (9, 5, Direction::Down),
        (8, 5, Direction::Down),
        (5, 1, Direction::Right),
        (5, 7, Direction::Right),
        (8, 3, Direction::Right),
        (2, 5, Direction::Right),
    ] {
        board.set_wall(x, y, dir, true);
    }
    // Balance: starters and opening gulls mirror under the same 180-degree
    // rotation as the walls, so neither half begins richer. (The old
    // Molting-vs-Giant pair fed P2's quadrant double the starting value,
    // and the lone top gull pressured only the upper castles.) Rotation
    // preserves chirality, so a strict mirror needs equal handedness.
    board.spawn_crab(3, 2, Direction::Right, Handedness::Left, CrabKind::Molting);
    board.spawn_crab(8, 6, Direction::Left, Handedness::Left, CrabKind::Molting);
    board.set_gull_period(240);
    board.set_round_length(Some(3 * 60 * TICKS_PER_SECOND));
    board.set_events_enabled(true);
    if preload_scores {
        board.set_score(0, 30);
        board.set_score(1, 12);
    }
    board
}

/// Generate a versus arena for `seats` players (2–[`MAX_PLAYERS`]) from a
/// seed, at any size from 9×7 up.
pub fn generate_arena(seed: u64, seats: u8, width: u8, height: u8) -> Board {
    let (width, height) = (width.max(9), height.max(7));
    // Five seats or more put castles on the long edges, and those want the
    // centre column an even width does not have; one tile wider and the six
    // castles mirror both ways. Four seats never touch the long edges, so
    // their boards keep the size they asked for.
    let width = if seats >= 5 && !seats_the_long_edges_evenly(width) {
        width + 1
    } else {
        width
    };
    let mut rng = Pcg32::new(seed, 0x0a_2e4a);
    let mut board = Board::new(width, height, seed);
    for (seat, &(x, y)) in castle_spots(width, height)
        .iter()
        .enumerate()
        .take(seats.clamp(2, MAX_PLAYERS as u8) as usize)
    {
        board.set_tile(x, y, TileKind::Castle(seat as PlayerId));
    }

    // Spawner holes, always in complete mirror groups: the two side holes,
    // the four end holes, or (on a big beach, which would otherwise feel
    // empty) both. Each hole then runs slower the more of them there are,
    // so the crab flow the balance work was tuned against is unchanged.
    let area = u32::from(width) * u32::from(height);
    let roll = rng.next_u32() % 2;
    let sides = side_spawners(width, height);
    let ends = end_spawners(width, height);
    let spots: Vec<(u8, u8, Direction)> = if area >= 240 {
        sides.iter().chain(ends.iter()).copied().collect()
    } else if roll == 0 {
        sides.to_vec()
    } else {
        ends.to_vec()
    };
    let period = (34 + rng.next_u32() % 26) * spots.len() as u32 / 2;
    for (x, y, dir) in spots {
        board.set_tile(x, y, TileKind::Spawner(Spawner { dir, period }));
    }

    // Interior rocks in mirror groups of four (scaled to area), kept apart
    // from each other and away from castles and spawners so nothing gets
    // walled in.
    let groups = area / 160 + 1 + rng.next_u32() % 2;
    let (rock_w, rock_h) = (u32::from(width) - 6, u32::from(height) - 4);
    let mut placed: Vec<(i32, i32)> = Vec::new();
    let mut attempts = 0;
    while (placed.len() as u32) < groups && attempts < 80 {
        attempts += 1;
        let x = 3 + (rng.next_u32() % rock_w) as i32;
        let y = 2 + (rng.next_u32() % rock_h) as i32;
        let images = quad(width, height, x as u8, y as u8);
        if images.iter().any(|&(ix, iy)| {
            placed
                .iter()
                .any(|&(px, py)| (px - i32::from(ix)).abs() + (py - i32::from(iy)).abs() <= 2)
        }) {
            continue;
        }
        if !place_quad(&mut board, x as u8, y as u8, TileKind::Rock) {
            continue;
        }
        placed.extend(images.map(|(ix, iy)| (i32::from(ix), i32::from(iy))));
    }

    // Interior walls, again in mirror groups.
    let wall_groups = 1 + rng.next_u32() % 2;
    for _ in 0..wall_groups {
        let x = 1 + (rng.next_u32() % u32::from(width - 3)) as u8;
        let y = 1 + (rng.next_u32() % u32::from(height - 3)) as u8;
        let dir = if rng.next_u32().is_multiple_of(2) {
            Direction::Down
        } else {
            Direction::Right
        };
        place_wall_quad(&mut board, x, y, dir);
    }

    // Terrain: a pool group and a kelp group. Bounded attempts keep
    // generation deterministic.
    for kind in [TileKind::Pool, TileKind::Kelp] {
        for _ in 0..8 {
            let x = 2 + (rng.next_u32() % u32::from(width - 4)) as u8;
            let y = 2 + (rng.next_u32() % u32::from(height - 4)) as u8;
            if place_quad(&mut board, x, y, kind) {
                break;
            }
        }
    }
    // Turnstiles ring the middle rather than sitting on it: a turnstile is
    // the one tile with a handedness, and a handed thing standing on a
    // mirror axis is its own opposite: the one shape this board cannot
    // make fair. One tile in from each axis, the group is four honest
    // mirror images.
    place_quad(
        &mut board,
        (width - 1) / 2 - 1,
        (height - 1) / 2 - 1,
        TileKind::Turnstile { next_right: true },
    );

    // No opening gull: a lone bird has to enter somewhere, and wherever it
    // entered it leaned on the nearest two castles. The ambient spawner
    // picks uniformly around the perimeter, so waiting for it costs a few
    // seconds and buys an unbiased start.
    board.set_gull_period(200 + rng.next_u32() % 80);
    board.set_round_length(Some(3 * 60 * TICKS_PER_SECOND));
    board.set_events_enabled(true);
    board
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_arena_seats_castles_in_join_order() {
        for seats in 2..=4u8 {
            let board = classic_arena(false, seats);
            for seat in 0..4u8 {
                let &(x, y) = &castle_spots(12, 9)[seat as usize];
                let expect = if seat < seats {
                    TileKind::Castle(seat)
                } else {
                    TileKind::Empty
                };
                assert_eq!(board.tile_at(x, y), expect, "{seats} seats, seat {seat}");
            }
        }
        // Deterministic: two builds agree, and scores start empty unless
        // preloaded for the sandbox.
        assert_eq!(
            classic_arena(false, 2).state_hash(),
            classic_arena(false, 2).state_hash()
        );
        assert_eq!(classic_arena(false, 2).scores()[0], 0);
        assert_eq!(classic_arena(true, 2).scores()[0], 30);
    }

    /// With six castles down, the whole set has to survive both mirrors, not
    /// just the 180-degree turn, which needs the centre column an odd width
    /// has, or the long-edge pair leans a tile toward two of the corners.
    #[test]
    fn six_castles_mirror_both_ways() {
        for &(w, h) in &[(9u8, 7u8), (12, 9), (16, 11), (20, 13)] {
            let board = generate_arena(7, 6, w, h);
            let (w, h) = (board.width(), board.height());
            assert!(!w.is_multiple_of(2), "{w}x{h} has a centre column");
            let spots = castle_spots(w, h);

            // Every spot's image under each mirror is also a spot, which is
            // what makes the four corners one job and the two edges another.
            let set: Vec<(u8, u8)> = spots.to_vec();
            for &(x, y) in &spots {
                assert!(set.contains(&(w - 1 - x, y)), "({x},{y}) flipped across");
                assert!(set.contains(&(x, h - 1 - y)), "({x},{y}) flipped down");
            }
            // And the pairs the team split is built from are still opposites.
            for pair in 0..3usize {
                let (x0, y0) = spots[pair * 2];
                assert_eq!((w - 1 - x0, h - 1 - y0), spots[pair * 2 + 1], "{pair}");
            }
            // The edge castles share the centre column rather than leaning.
            assert_eq!(spots[4].0, spots[5].0, "{w}x{h} edge castles centred");
        }
    }

    #[test]
    fn same_seed_same_arena() {
        let a = generate_arena(1234, 4, 12, 9);
        let b = generate_arena(1234, 4, 12, 9);
        assert_eq!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn different_seeds_differ() {
        assert_ne!(
            generate_arena(1, 4, 12, 9).state_hash(),
            generate_arena(2, 4, 12, 9).state_hash()
        );
    }

    #[test]
    fn every_size_generates_sane_arenas() {
        for &(w, h) in &[(9u8, 7u8), (12, 9), (16, 11), (20, 13)] {
            for seed in 0..10u64 {
                let board = generate_arena(seed, 4, w, h);
                assert_eq!(board.width(), w);
                assert_eq!(board.height(), h);
                let mut castles = 0;
                for y in 0..h {
                    for x in 0..w {
                        if let TileKind::Castle(_) = board.tile_at(x, y) {
                            castles += 1;
                        }
                    }
                }
                assert_eq!(castles, 4, "{w}x{h} seed {seed}");
            }
        }
    }

    #[test]
    fn arenas_have_the_requested_castles_and_some_spawners() {
        for seed in 0..20u64 {
            let board = generate_arena(seed, 4, 12, 9);
            let mut castles = 0;
            let mut spawners = 0;
            for y in 0..board.height() {
                for x in 0..board.width() {
                    match board.tile_at(x, y) {
                        TileKind::Castle(_) => castles += 1,
                        TileKind::Spawner(_) => spawners += 1,
                        TileKind::Empty
                        | TileKind::Rock
                        | TileKind::Turnstile { .. }
                        | TileKind::Kelp
                        | TileKind::Pool => {}
                    }
                }
            }
            assert_eq!(castles, 4, "seed {seed}");
            assert!(spawners >= 2, "seed {seed}");
            assert!(board.remaining_ticks().is_some());
        }
    }

    /// The fairness contract for generated arenas: the whole board reads
    /// the same after a left-right flip and after a top-bottom flip. Since
    /// those two flips permute the four corner castles among themselves,
    /// every seat faces an identical routing problem, and no seat spread
    /// can come from the map itself.
    #[test]
    fn generated_arenas_mirror_both_ways() {
        for &(w, h) in &[(9u8, 7u8), (12, 9), (16, 11), (20, 13)] {
            for seed in 0..25u64 {
                let board = generate_arena(seed, 4, w, h);
                for y in 0..h {
                    for x in 0..w {
                        let here = board.tile_at(x, y);
                        // Castles differ by owner; what matters is that a
                        // castle sits at each image, which the fixed corner
                        // spots guarantee.
                        let same = |other: TileKind, flip_x: bool, flip_y: bool| match (
                            mirrored_tile(here, flip_x, flip_y),
                            other,
                        ) {
                            (TileKind::Castle(_), TileKind::Castle(_)) => true,
                            (a, b) => a == b,
                        };
                        assert!(
                            same(board.tile_at(w - 1 - x, y), true, false),
                            "{w}x{h} seed {seed}: ({x},{y}) breaks the left-right mirror"
                        );
                        assert!(
                            same(board.tile_at(x, h - 1 - y), false, true),
                            "{w}x{h} seed {seed}: ({x},{y}) breaks the top-bottom mirror"
                        );
                        // Walls are edges, so their mirrors are named from
                        // the tile on the other side of the flip.
                        assert_eq!(
                            board.wall_at(x, y, Direction::Down),
                            board.wall_at(w - 1 - x, y, Direction::Down),
                            "{w}x{h} seed {seed}: wall under ({x},{y})"
                        );
                        assert_eq!(
                            board.wall_at(x, y, Direction::Right),
                            board.wall_at(x, h - 1 - y, Direction::Right),
                            "{w}x{h} seed {seed}: wall right of ({x},{y})"
                        );
                        // The other two relations: a wall below a tile
                        // mirrors top-bottom onto the wall below the tile
                        // one row *before* the mirrored row, and likewise
                        // for a wall to the right under a left-right flip.
                        if y + 2 <= h {
                            assert_eq!(
                                board.wall_at(x, y, Direction::Down),
                                board.wall_at(x, h - 2 - y, Direction::Down),
                                "{w}x{h} seed {seed}: wall under ({x},{y}), flipped"
                            );
                        }
                        if x + 2 <= w {
                            assert_eq!(
                                board.wall_at(x, y, Direction::Right),
                                board.wall_at(w - 2 - x, y, Direction::Right),
                                "{w}x{h} seed {seed}: wall right of ({x},{y}), flipped"
                            );
                        }
                    }
                }
                // Spawners mirror as a set, cadence included.
                let holes: Vec<(u8, u8, Spawner)> = (0..h)
                    .flat_map(|y| (0..w).map(move |x| (x, y)))
                    .filter_map(|(x, y)| match board.tile_at(x, y) {
                        TileKind::Spawner(s) => Some((x, y, s)),
                        TileKind::Empty
                        | TileKind::Rock
                        | TileKind::Castle(_)
                        | TileKind::Turnstile { .. }
                        | TileKind::Kelp
                        | TileKind::Pool => None,
                    })
                    .collect();
                assert!(holes.len() >= 2, "{w}x{h} seed {seed}: too few spawners");
                for &(x, y, hole) in &holes {
                    let twin = |flip_x: bool, flip_y: bool| Spawner {
                        dir: mirror_dir(hole.dir, flip_x, flip_y),
                        period: hole.period,
                    };
                    assert!(
                        holes.contains(&(w - 1 - x, y, twin(true, false)))
                            && holes.contains(&(x, h - 1 - y, twin(false, true))),
                        "{w}x{h} seed {seed}: spawner ({x},{y}) has no mirror twin"
                    );
                }
            }
        }
    }

    /// A generated arena must actually play: with the four seats botted, a
    /// couple of simulated minutes produce banked crabs.
    #[test]
    fn generated_arenas_are_alive_with_bots() {
        use crate::sim::{BotLevel, MAX_PLAYERS, PlayerAction, bot_action};
        let mut board = generate_arena(777, 4, 12, 9);
        for _ in 0..4000 {
            let mut actions = [PlayerAction::None; MAX_PLAYERS];
            for seat in 0..4u8 {
                actions[seat as usize] = bot_action(&board, seat, BotLevel::Normal);
            }
            board.tick(&actions);
        }
        assert!(
            board.crabs_spawned() > 40,
            "spawners ran ({} spawned)",
            board.crabs_spawned()
        );
        assert!(
            board.crabs_banked() > 0,
            "bots routed crabs home (scores {:?})",
            board.scores()
        );
    }
    /// A generated beach with no edges. Wrapping is as old as the campaign's
    /// level 26, but it had only ever run on handcrafted boards - a generated
    /// arena puts spawner holes *in* the border tiles a wrapping creature
    /// walks through, which is a combination nothing had played before.
    #[test]
    fn an_open_ocean_arena_plays_a_round() {
        use crate::sim::{BotLevel, MAX_PLAYERS, PlayerAction, bot_action};
        for seed in 0..6u64 {
            let mut board = generate_arena(seed, 4, 16, 11);
            board.set_wrap(true);
            for _ in 0..4000 {
                let mut actions = [PlayerAction::None; MAX_PLAYERS];
                for seat in 0..4u8 {
                    actions[seat as usize] = bot_action(&board, seat, BotLevel::Normal);
                }
                board.tick(&actions);
            }
            assert!(board.crabs_spawned() > 40, "seed {seed} spawners ran");
            assert!(
                board.crabs_banked() > 0,
                "seed {seed} bots routed crabs home across an open edge (scores {:?})",
                board.scores()
            );
            // Nothing walked off the world: every creature is on a real tile.
            let tiles = u16::from(board.width()) * u16::from(board.height());
            assert!(board.gulls().iter().all(|g| g.tile < tiles), "seed {seed}");
        }
    }
}
