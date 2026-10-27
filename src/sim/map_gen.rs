//! Seeded skirmish-arena generation: every seed is a fresh, fair-ish
//! battleground at a configurable size. Deterministic (one `Pcg32` stream),
//! so a generated arena replays and stays lockstep-safe if shared by seed.

use crate::sim::board::{
    Board, GULL_CAP, MAX_PLAYERS, PlayerId, Spawner, TICKS_PER_SECOND, TileKind,
    VERSUS_GOLDEN_EVERY,
};
use crate::sim::crab::{CrabKind, Handedness};
use crate::sim::direction::Direction;
use crate::sim::rng::Pcg32;

mod styles;

pub use styles::{BeachStyle, SURPRISE_LANES_MOST, surprise_size, surprise_style, surprise_styles};

/// Tiles on the classic 12x9 beach, the size the gull pressures and the
/// crab flow were tuned on.
const CLASSIC_AREA: u32 = 12 * 9;

/// The rules of a versus round on a beach of this size, from the gull
/// period a pressure dial names for the classic 12x9 beach: the flock
/// turns over ([`Rules::gull_turnover`]), the golden crab is called
/// rather than rolled ([`Rules::golden_every`]), the event wheel locks
/// only while a timed event runs ([`Rules::timed_lock`]), Crab Monopoly
/// carries its crabs home ([`Rules::sweep_flight`]) at what they were
/// worth when it fired ([`Rules::sweep_keeps_worth`]), the turnover keeps
/// the flock whole ([`Rules::whole_flock`]), and a big beach gets more
/// gulls. The rules it does not name (the signpost rule, the round's
/// length, raids) are left as the board had them.
///
/// [`Rules::gull_turnover`]: crate::sim::board::Rules::gull_turnover
/// [`Rules::golden_every`]: crate::sim::board::Rules::golden_every
/// [`Rules::timed_lock`]: crate::sim::board::Rules::timed_lock
/// [`Rules::sweep_flight`]: crate::sim::board::Rules::sweep_flight
/// [`Rules::sweep_keeps_worth`]: crate::sim::board::Rules::sweep_keeps_worth
/// [`Rules::whole_flock`]: crate::sim::board::Rules::whole_flock
///
/// The ambient spawner sends one gull a period and stops at a fixed cap,
/// whatever the size, so the bigger beaches spread the same half-dozen
/// birds over up to two and a half times the sand, and their raids fell to
/// well under the classic rate. A beach bigger than the classic one gets a
/// cap and a rate scaled by its area, keeping gulls per tile level.
/// Smaller beaches keep the classic numbers: a crowded 9x7 is the point of
/// choosing one.
pub fn set_versus_rules(board: &mut Board, period: u32) {
    let area = u32::from(board.width()) * u32::from(board.height());
    let area = area.max(CLASSIC_AREA);
    let rules = board.rules_mut();
    rules.gull_period = (period * CLASSIC_AREA / area).max(1);
    rules.gull_cap = (GULL_CAP as u32 * area / CLASSIC_AREA).min(u32::from(u8::MAX)) as u8;
    rules.gull_turnover = true;
    rules.golden_every = VERSUS_GOLDEN_EVERY;
    rules.timed_lock = true;
    rules.sweep_flight = true;
    rules.whole_flock = true;
    rules.sweep_keeps_worth = true;
    rules.fair_spawns = true;
}

/// A castle spot per seat for a board of the given size.
///
/// The first four are the corners, each the 180-degree image of the one
/// before it, which makes them interchangeable. Seats five and six sit at
/// the centre of the long edges as another such pair, not the left and
/// right mid-edges, which is where the side spawner holes point, and a
/// castle in front of a hole is fed crabs for free.
///
/// A five-seat table leaves one spot out, which one depending on the beach
/// (see `five_spots`).
///
/// The long-edge pair takes the *centre* column, which is why generated
/// six-seat arenas are widened to an odd width (see [`generate_arena`]): an
/// even width has no centre, so the pair leans one tile nearer two corners
/// than the other two.
///
/// Corners and edges cannot be made the same job, and it is worth saying
/// why rather than trying again: a rectangle's symmetry group has four
/// elements, so its positions fall into orbits of at most four, and six
/// equivalent spots do not exist on one. The few percent the edges keep is
/// the price of the shape, and handicapping them overshoots every way it
/// has been tried.
///
/// Every generated board is at least 9x7, so the spots are one tile in from
/// the walls. A custom arena can be any size the level format allows, down
/// to 1x1, and this is asked about those too (`load_versus` starts each
/// cursor near its castle): the arithmetic saturates so a tiny board gets
/// spots that are merely wrong, never a subtraction past zero.
pub fn castle_spots(width: u8, height: u8) -> [(u8, u8); MAX_PLAYERS] {
    let mid = width.saturating_sub(1) / 2;
    let (far_x, far_y) = (width.saturating_sub(2), height.saturating_sub(2));
    [
        (1, 1),
        (far_x, far_y),
        (far_x, 1),
        (1, far_y),
        (mid, 1),
        (width.saturating_sub(1 + mid), far_y),
    ]
}

/// Which castle spot each seat takes on a board built from `seed`.
///
/// The spots are not equal work: `examples/balance` over 3000 six-seat
/// rounds puts the edge pair about six percent ahead of the worst corner,
/// a 13% spread between the best seat and the worst. That gap is the shape
/// of a rectangle and cannot be designed away (see [`castle_spots`]), so
/// what moves instead is *who* gets it, and over a series the advantage
/// cancels rather than compounding on whoever joined third.
///
/// An even table is shuffled in pairs, never seat by seat, and that is
/// load-bearing.
/// [`castle_spots`] lists 180-degree opposite pairs, `(0,1) (2,3) (4,5)`,
/// and both team modes are built on it: Pairs gives a team a whole pair,
/// Trios gives each team one seat from every pair. Permuting whole pairs
/// and flipping within them keeps seat `2k` opposite seat `2k+1`, where
/// shuffling the six spots freely would hand one team two corners and the
/// other two edges.
///
/// Only the pairs in use are touched: at four seats or fewer the two edge
/// spots stay out of it, which is what keeps a four-seat board off the
/// long edges and the same size it asked for.
fn seat_spots(seed: u64, seats: u8, wraps: bool) -> [usize; MAX_PLAYERS] {
    // A stream of its own, so drawing it cannot shift the arena's own rolls
    // and change the beach that a seed has always built.
    let mut rng = Pcg32::new(seed, 0x0c_a571);
    let pairs = (seats.clamp(2, MAX_PLAYERS as u8) as usize).div_ceil(2);
    let mut order: Vec<usize> = (0..pairs).collect();
    // Fisher-Yates over the pairs in use.
    for i in (1..pairs).rev() {
        let j = (rng.next_u32() as usize) % (i + 1);
        order.swap(i, j);
    }
    let mut spots = [0usize; MAX_PLAYERS];
    for (seat_pair, &spot_pair) in order.iter().enumerate() {
        let flip = usize::from(rng.next_u32().is_multiple_of(2));
        spots[seat_pair * 2] = spot_pair * 2 + flip;
        spots[seat_pair * 2 + 1] = spot_pair * 2 + (1 - flip);
    }
    // Pairs the seat count never reaches keep their own spots, so the array
    // is always a permutation whatever it is asked about.
    for pair in pairs..MAX_PLAYERS / 2 {
        spots[pair * 2] = pair * 2;
        spots[pair * 2 + 1] = pair * 2 + 1;
    }
    // An odd table is dealt afresh. The pair draw leaves the seat on the
    // last, half-used pair facing an empty castle that its two neighbours
    // share between them, and over 1500 bot rounds that one seat won 22-26%
    // at three and 15% at five. Team modes need an even table, so nothing
    // leans on the pairing here.
    let seated = usize::from(seats.clamp(2, MAX_PLAYERS as u8));
    if seated % 2 == 1 {
        if seated == 5 {
            spots = five_spots(&mut rng, wraps);
        }
        // Whatever spots are in use, which seat sits on which is a fresh
        // draw, so every seat is as likely as any other to get each one.
        for i in (1..seated).rev() {
            let j = (rng.next_u32() as usize) % (i + 1);
            spots.swap(i, j);
        }
    }
    spots
}

/// The five castle spots a five-seat table plays on, in [`castle_spots`]
/// order, and the empty one last.
///
/// On a beach with edges that is the four corners and one long-edge centre:
/// over 1500 bot rounds each, on the 17x11 and the 21x13, every spot then
/// won within two points of a fifth, where leaving a corner empty handed
/// the edge pair 27-31% and the corners as little as 11%. The open ocean is
/// the other way round: its lone edge castle won 7%, and with a corner
/// left empty every spot won 15-23%.
fn five_spots(rng: &mut Pcg32, wraps: bool) -> [usize; MAX_PLAYERS] {
    let empty = match wraps {
        true => rng.next_u32() as usize % 4,
        false => 4 + rng.next_u32() as usize % 2,
    };
    let mut spots = [empty; MAX_PLAYERS];
    for (slot, spot) in spots
        .iter_mut()
        .zip((0..MAX_PLAYERS).filter(|&spot| spot != empty))
    {
        *slot = spot;
    }
    spots
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

/// The holes a three-seat table is dealt, on any beach: the four end
/// holes, one by each corner whether or not a castle sits there.
///
/// A side hole feeds both corners on its side, and with three castles one
/// side has a corner empty: its hole is one seat's alone while the other
/// side's is split two ways. Over 1500 bot rounds of the classic arena
/// with the seats drawn, the seat beside the empty corner on its side won
/// 63% with the side holes in, and the seat facing the empty corner 16%;
/// with the end holes alone every seat's spot won 24-38%, on every size.
fn three_seat_holes(width: u8, height: u8) -> [(u8, u8, Direction); 4] {
    end_spawners(width, height)
}

/// The fewest spawner holes a table of `seats` should be dealt: one fewer
/// than the players. Two holes feeding four castles leave the crabs on two
/// lanes everybody fights over, and the round turns into a scrum at the
/// same two spots; a hole for nearly every player spreads them out.
fn holes_wanted(seats: u8) -> usize {
    usize::from(seats.saturating_sub(1))
}

/// The four images of a tile under the arena's two mirror symmetries,
/// left-right and top-bottom.
///
/// Every generated feature is placed on all four at once, which is what
/// makes the four corner castles interchangeable: 180-degree symmetry alone
/// pairs seat 1 with seat 2 and seat 3 with seat 4, and leaves the other
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
pub(crate) fn mirror_dir(dir: Direction, flip_x: bool, flip_y: bool) -> Direction {
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
/// differs. Every versus round, online or on the couch, builds it on that
/// round's seed (`match_setup::versus_board`): online, the seed travels in
/// the match terms, so every peer builds the same beach from it.
pub fn classic_arena_seeded(seed: u64, preload_scores: bool, seats: u8) -> Board {
    let mut board = Board::new(12, 9, seed);
    // The handcrafted beach is a four-castle board; six seats want a
    // generated arena with room for the long-edge castles.
    //
    // Three seats leave a corner empty, and its sand goes to the two castles
    // beside it. Who sits where is drawn from the seed as on a generated
    // beach ([`seat_spots`]), so that is nobody's seat in particular: drawn
    // the same way every round, the castle at (1,1) won 69%. Two and four
    // seats keep the beach as drawn.
    let spots = castle_spots(12, 9);
    let seated = seats.clamp(2, 4);
    let order: [usize; MAX_PLAYERS] = match seated % 2 == 1 {
        true => seat_spots(seed, seated, false),
        false => std::array::from_fn(|spot| spot),
    };
    for (seat, &spot) in order.iter().enumerate().take(usize::from(seated)) {
        let (x, y) = spots[spot];
        board.set_tile(x, y, TileKind::Castle(seat as PlayerId));
    }
    // Balance: strictly equal cadences. Even a 44/48 stagger measurably
    // tilted bot duels toward the faster side over a full round.
    //
    // The two side holes are the beach as it was drawn. A table bigger
    // than [`holes_wanted`] lets two holes feed also gets the generated
    // arenas' four end holes, one by each corner castle, and every hole
    // slows to keep the crab flow the beach was balanced on.
    //
    // Three seats get the four end holes alone (see [`three_seat_holes`]).
    let sides = side_spawners(12, 9);
    let ends = end_spawners(12, 9);
    let holes: Vec<(u8, u8, Direction)> = if seated == 3 {
        three_seat_holes(12, 9).to_vec()
    } else if holes_wanted(seats) > sides.len() {
        sides.iter().chain(ends.iter()).copied().collect()
    } else {
        sides.to_vec()
    };
    let period = 46 * holes.len() as u32 / 2;
    for (x, y, dir) in holes {
        board.set_tile(x, y, TileKind::Spawner(Spawner { dir, period }));
    }
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
    // rotation as the walls, so neither half begins richer. Rotation
    // preserves chirality, so a strict mirror needs equal handedness.
    board.spawn_crab(3, 2, Direction::Right, Handedness::Left, CrabKind::Molting);
    board.spawn_crab(8, 6, Direction::Left, Handedness::Left, CrabKind::Molting);
    board.rules_mut().gull_period = 240;
    board.rules_mut().round_length = Some(3 * 60 * TICKS_PER_SECOND);
    board.set_events_enabled(true);
    if preload_scores {
        board.set_score(0, 30);
        board.set_score(1, 12);
    }
    board
}

/// Tiles per extra mirror group of rocks.
const ROCK_AREA: u32 = 110;
/// Tiles per extra mirror group of interior walls.
const WALL_AREA: u32 = 120;

/// Generate a versus arena for `seats` players (2–[`MAX_PLAYERS`]) from a
/// seed, at any size from 9×7 up, with edges.
pub fn generate_arena(seed: u64, seats: u8, width: u8, height: u8) -> Board {
    generate_beach(seed, seats, width, height, false)
}

/// [`generate_arena`], with its edges open when `wraps`. The open ocean is
/// laid out like any other generated beach except where the castles of an
/// odd table go, which is measured separately for it (see [`five_spots`]),
/// so it is built here rather than by opening an edged arena's edges.
pub fn generate_beach(seed: u64, seats: u8, width: u8, height: u8, wraps: bool) -> Board {
    generate_styled(seed, seats, width, height, wraps, BeachStyle::Classic)
}

/// [`generate_beach`] laid in `style`. The frame is the same whatever the
/// style: castles dealt to the seats from the seed, the holes for the
/// table and the versus rules, all as [`BeachStyle::Classic`] builds them,
/// so a style changes the terrain between the castles and nothing else.
/// The classic terrain is the default, and every seed builds the beach it
/// always built (`tests/it/maps.rs` holds the generator to the text it
/// wrote before styles existed).
pub fn generate_styled(
    seed: u64,
    seats: u8,
    width: u8,
    height: u8,
    wraps: bool,
    style: BeachStyle,
) -> Board {
    let classic = classic_beach(seed, seats, width, height, wraps);
    match style {
        BeachStyle::Classic => classic,
        BeachStyle::Islands
        | BeachStyle::Lanes
        | BeachStyle::Lagoon
        | BeachStyle::Maze
        | BeachStyle::Driftwood => styles::restyle(classic, style, seed),
    }
}

/// The generated beach with today's terrain: the frame and the classic
/// rocks, walls, pool, kelp and logs, on one stream.
fn classic_beach(seed: u64, seats: u8, width: u8, height: u8, wraps: bool) -> Board {
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
    let spots = castle_spots(width, height);
    // Which chair is which castle is drawn with the board: see `seat_spots`.
    for (seat, spot) in seat_spots(seed, seats, wraps)
        .iter()
        .enumerate()
        .take(seats.clamp(2, MAX_PLAYERS as u8) as usize)
    {
        let (x, y) = spots[*spot];
        board.set_tile(x, y, TileKind::Castle(seat as PlayerId));
    }

    // Spawner holes, always in complete mirror groups: the two side holes,
    // the four end holes, or (on a big beach, which would otherwise feel
    // empty, or for a table [`holes_wanted`] says outnumbers the four) both.
    // Each hole then runs slower the more of them there are, so the crab
    // flow the balance work was tuned against is unchanged: more holes
    // spread the crabs out, they do not add to them.
    let area = u32::from(width) * u32::from(height);
    let roll = rng.next_u32() % 2;
    let sides = side_spawners(width, height);
    let ends = end_spawners(width, height);
    let wanted = holes_wanted(seats);
    let spots: Vec<(u8, u8, Direction)> = if seats == 3 {
        three_seat_holes(width, height).to_vec()
    } else if area >= 240 || wanted > ends.len() {
        sides.iter().chain(ends.iter()).copied().collect()
    } else if roll == 0 && wanted <= sides.len() {
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
    let groups = area / ROCK_AREA + 1 + rng.next_u32() % 2;
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

    // Interior walls, again in mirror groups, and more of them the bigger
    // the beach: walls are what make the choke points an arrow decides, and
    // a big beach with the classic beach's one or two groups is an open
    // field where the crabs find their own way home.
    let wall_groups = 1 + area / WALL_AREA + rng.next_u32() % 2;
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
    // the one tile with a handedness, and a handed thing on a mirror axis
    // is its own opposite. One tile in from each axis, the group is four
    // honest mirror images.
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
    set_versus_rules(&mut board, 200 + rng.next_u32() % 80);
    board.rules_mut().round_length = Some(3 * 60 * TICKS_PER_SECOND);
    board.set_events_enabled(true);
    board.set_wrap(wraps);
    board
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asked about a custom arena of any size the level format allows, the
    /// spots must come back rather than subtract past zero: a 2x1 beach
    /// off the shelf once took the round down on its first frame.
    #[test]
    fn castle_spots_never_subtract_past_zero() {
        for (w, h) in [(1u8, 1u8), (2, 1), (1, 2), (3, 3), (4, 2)] {
            let spots = castle_spots(w, h);
            assert_eq!(spots.len(), MAX_PLAYERS);
            for (x, y) in spots {
                assert!(x <= w && y <= h, "({x},{y}) on a {w}x{h} board");
            }
        }
        assert_eq!(
            castle_spots(9, 7)[1],
            (7, 5),
            "and a real board is unchanged"
        );
    }

    /// Two and four seats sit as the beach was drawn. Three are drawn from
    /// the seed (see `the_seat_facing_the_empty_spot_is_drawn_not_fixed`).
    #[test]
    fn classic_arena_seats_castles_in_join_order() {
        for seats in [2u8, 4] {
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

    /// The classic beach and the smaller one keep the gulls the pressure
    /// dial names; a bigger one takes more of them, and more often, in
    /// proportion to its area.
    #[test]
    fn only_a_beach_bigger_than_the_classic_one_takes_more_gulls() {
        for (w, h) in [(9u8, 7u8), (12, 9)] {
            let mut board = Board::new(w, h, 1);
            set_versus_rules(&mut board, 240);
            assert_eq!(board.rules().gull_period, 240, "{w}x{h}");
            assert_eq!(usize::from(board.rules().gull_cap), GULL_CAP, "{w}x{h}");
        }
        let mut xl = Board::new(20, 13, 1);
        set_versus_rules(&mut xl, 240);
        assert_eq!(xl.rules().gull_period, 240 * 108 / 260);
        assert_eq!(xl.rules().gull_cap, 14);
    }

    /// The bigger beaches carry more walls, and walls can box a tile in:
    /// every castle on every size must still be reachable on foot from a
    /// spawner hole, or a seat is out of the round before it starts.
    #[test]
    fn every_castle_can_be_walked_to_from_a_spawner() {
        for &(w, h) in &[(9u8, 7u8), (12, 9), (16, 11), (20, 13)] {
            for seats in [2u8, 4, 6] {
                for seed in 0..60u64 {
                    let board = generate_arena(seed, seats, w, h);
                    let (bw, bh) = (board.width(), board.height());
                    let index = |x: u8, y: u8| usize::from(y) * usize::from(bw) + usize::from(x);
                    let mut seen = vec![false; usize::from(bw) * usize::from(bh)];
                    let mut queue: Vec<(u8, u8)> = board
                        .tiles()
                        .filter(|(_, _, kind)| matches!(kind, TileKind::Spawner(_)))
                        .map(|(x, y, _)| (x, y))
                        .collect();
                    for &(x, y) in &queue {
                        seen[index(x, y)] = true;
                    }
                    while let Some((x, y)) = queue.pop() {
                        for dir in Direction::ALL {
                            if board.wall_at(x, y, dir) {
                                continue;
                            }
                            let (dx, dy) = dir.offset();
                            let (nx, ny) = (i32::from(x) + dx, i32::from(y) + dy);
                            if nx < 0 || ny < 0 || nx >= i32::from(bw) || ny >= i32::from(bh) {
                                continue;
                            }
                            let (nx, ny) = (nx as u8, ny as u8);
                            if seen[index(nx, ny)] || board.tile_at(nx, ny) == TileKind::Rock {
                                continue;
                            }
                            seen[index(nx, ny)] = true;
                            queue.push((nx, ny));
                        }
                    }
                    for player in board.castle_owners() {
                        let (x, y) = board.castle_of(player).expect("a castle");
                        assert!(
                            seen[index(x, y)],
                            "{w}x{h}, {seats} seats, seed {seed}: castle {player} walled off"
                        );
                    }
                }
            }
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

    /// Every beach a table can be dealt has a hole for all but one of its
    /// players, and the extra holes slow down so the crab flow is the one
    /// the beach was balanced on (two holes' worth at the classic cadence).
    #[test]
    fn a_table_gets_a_hole_for_all_but_one_player() {
        let holes = |board: &Board| -> Vec<u32> {
            let mut periods = Vec::new();
            for y in 0..board.height() {
                for x in 0..board.width() {
                    if let TileKind::Spawner(s) = board.tile_at(x, y) {
                        periods.push(s.period);
                    }
                }
            }
            periods
        };
        for seats in 2..=MAX_PLAYERS as u8 {
            let wanted = usize::from(seats - 1);
            if seats <= 4 {
                let classic = holes(&classic_arena(false, seats));
                assert!(classic.len() >= wanted, "classic, {seats} seats");
                let flow: f64 = classic.iter().map(|&p| 1.0 / f64::from(p)).sum();
                assert!((flow - 2.0 / 46.0).abs() < 1e-9, "classic, {seats} seats");
            }
            for (w, h) in [(9, 7), (12, 9), (16, 11), (20, 13)] {
                for seed in 0..12u64 {
                    let n = holes(&generate_arena(seed, seats, w, h)).len();
                    assert!(n >= wanted, "{w}x{h} seed {seed}, {seats} seats: {n} holes");
                }
            }
        }
    }

    /// The one thing the shuffle may never break. Both team modes read
    /// fairness off `castle_spots` pairing seat `2k` with seat `2k+1` as
    /// 180-degree opposites: Pairs hands a team a whole pair, Trios hands
    /// each team one seat out of every pair. Shuffle the six spots freely
    /// and a team quietly gets two corners while the other gets two edges.
    /// Only an even table has teams, and only an even table keeps its pairs:
    /// an odd one is dealt afresh (see `seat_spots`).
    #[test]
    fn every_seat_pair_still_sits_on_opposite_castles() {
        for seed in 0..200u64 {
            for seats in (2..=MAX_PLAYERS as u8).step_by(2) {
                let board = generate_arena(seed, seats, 12, 9);
                let (w, h) = (board.width(), board.height());
                for pair in 0..usize::from(seats) / 2 {
                    let a = board
                        .castle_of(pair as u8 * 2)
                        .expect("a seat in the count has a castle");
                    let b = board
                        .castle_of(pair as u8 * 2 + 1)
                        .expect("and so does its partner");
                    assert_eq!(
                        (w - 1 - a.0, h - 1 - a.1),
                        b,
                        "seed {seed}, {seats} seats, pair {pair}"
                    );
                }
            }
        }
    }

    /// Four seats or fewer keep off the long edges however the pairs fall,
    /// which is what lets a four-seat board stay the size it asked for.
    #[test]
    fn a_small_table_never_lands_on_a_long_edge() {
        for seed in 0..200u64 {
            for seats in 2..=4u8 {
                let board = generate_arena(seed, seats, 12, 9);
                let (w, h) = (board.width(), board.height());
                let corners = [(1, 1), (w - 2, h - 2), (w - 2, 1), (1, h - 2)];
                for seat in 0..seats {
                    let spot = board.castle_of(seat).expect("a castle");
                    assert!(
                        corners.contains(&spot),
                        "seed {seed}, {seats} seats: seat {seat} at {spot:?} is not a corner"
                    );
                }
            }
        }
    }

    /// And the shuffle actually shuffles: the long-edge pair is worth a few
    /// percent, so no seat may hold it every round. Seat zero is the host's
    /// chair online, and was stuck with a corner for a whole series.
    #[test]
    fn the_edge_castles_do_not_always_fall_to_the_same_seats() {
        let mut edge = 0;
        let seeds = 300u64;
        for seed in 0..seeds {
            let board = generate_arena(seed, 6, 20, 13);
            let (x, y) = board.castle_of(0).expect("the host has a castle");
            let (w, h) = (board.width(), board.height());
            // A corner sits one tile in from both walls; the long-edge
            // castles are the two that do not.
            let corner = (x == 1 || x == w - 2) && (y == 1 || y == h - 2);
            if !corner {
                edge += 1;
            }
        }
        // One pair in three is the edge pair, and a seat holds one of its
        // pair's two spots, so seat zero should take an edge about a third
        // of the time. Loose bounds: this is a shuffle, not a schedule.
        assert!(
            (seeds / 6..seeds / 2).contains(&edge),
            "seat zero took an edge castle {edge} times in {seeds}"
        );
    }

    /// How often each seat of an odd table sits facing the empty spot, the
    /// spot whose 180-degree image has no castle, over 300 seeds.
    fn times_facing_the_empty_spot(seats: u8, build: impl Fn(u64) -> Board) -> Vec<u32> {
        let mut facing = vec![0u32; usize::from(seats)];
        for seed in 0..300u64 {
            let board = build(seed);
            let (w, h) = (board.width(), board.height());
            let castles: Vec<(u8, u8)> = (0..seats)
                .map(|seat| board.castle_of(seat).expect("every seat has a castle"))
                .collect();
            for (seat, &(x, y)) in castles.iter().enumerate() {
                if !castles.contains(&(w - 1 - x, h - 1 - y)) {
                    facing[seat] += 1;
                }
            }
        }
        facing
    }

    /// An odd table leaves a spot empty, and the castle facing it is the
    /// worst seat at the table (its neighbours share the empty spot's sand).
    /// Which seat gets it is drawn with the beach, at three and at five, on
    /// the classic arena and on generated beaches alike, so it is no one
    /// seat's every round.
    #[test]
    fn the_seat_facing_the_empty_spot_is_drawn_not_fixed() {
        type Build = fn(u64) -> Board;
        let cases: [(&str, u8, Build); 4] = [
            ("classic", 3, |seed| classic_arena_seeded(seed, false, 3)),
            ("12x9", 3, |seed| generate_arena(seed, 3, 12, 9)),
            ("16x11", 5, |seed| generate_arena(seed, 5, 16, 11)),
            ("ocean", 5, |seed| generate_beach(seed, 5, 16, 11, true)),
        ];
        for (name, seats, build) in cases {
            let facing = times_facing_the_empty_spot(seats, build);
            // One castle in the table faces the empty spot each round, so a
            // seat should hold it 300 / seats times. Loose bounds: a draw.
            let fair = 300 / u32::from(seats);
            for (seat, &n) in facing.iter().enumerate() {
                assert!(
                    (fair / 2..fair * 3 / 2).contains(&n),
                    "{name}, {seats} seats: seat {seat} faced the empty spot {n} times in 300"
                );
            }
        }
    }

    /// Five seats on a beach with edges play the four corners and one
    /// long-edge centre; on the open ocean it is a corner that stays empty.
    /// Each is the layout whose spots won within a few points of each
    /// other in bot rounds (see `five_spots`).
    #[test]
    fn five_seats_leave_an_edge_empty_on_land_and_a_corner_at_sea() {
        for seed in 0..100u64 {
            for wraps in [false, true] {
                let board = generate_beach(seed, 5, 16, 11, wraps);
                let (w, h) = (board.width(), board.height());
                let corners = (0..5u8)
                    .filter_map(|seat| board.castle_of(seat))
                    .filter(|&(x, y)| (x == 1 || x == w - 2) && (y == 1 || y == h - 2))
                    .count();
                let expected = match wraps {
                    false => 4,
                    true => 3,
                };
                assert_eq!(corners, expected, "seed {seed}, wraps {wraps}");
                assert_eq!(board.wrap(), wraps, "seed {seed}");
            }
        }
    }

    /// A three-seat table plays on the four end holes and no side hole, on
    /// the classic arena and every generated size: a side hole beside the
    /// empty corner is one seat's alone (see `three_seat_holes`).
    #[test]
    fn three_seats_get_the_end_holes_only() {
        let holes = |board: &Board| -> Vec<(u8, u8)> {
            let mut at = Vec::new();
            for y in 0..board.height() {
                for x in 0..board.width() {
                    if let TileKind::Spawner(_) = board.tile_at(x, y) {
                        at.push((x, y));
                    }
                }
            }
            at
        };
        let ends = |w: u8, h: u8| -> Vec<(u8, u8)> {
            let mut at: Vec<(u8, u8)> =
                end_spawners(w, h).iter().map(|&(x, y, _)| (x, y)).collect();
            at.sort_unstable_by_key(|&(x, y)| (y, x));
            at
        };
        for seed in 0..20u64 {
            assert_eq!(
                holes(&classic_arena_seeded(seed, false, 3)),
                ends(12, 9),
                "classic seed {seed}"
            );
            for (w, h) in [(9, 7), (12, 9), (16, 11), (20, 13)] {
                assert_eq!(
                    holes(&generate_arena(seed, 3, w, h)),
                    ends(w, h),
                    "{w}x{h} seed {seed}"
                );
            }
        }
    }

    #[test]
    fn generated_arenas_mirror_both_ways() {
        // The fairness contract: the whole board reads the same after a
        // left-right flip and after a top-bottom flip. Those two flips
        // permute the four corner castles among themselves, so every seat
        // faces an identical routing problem.
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
    /// level 26, but had only ever run on handcrafted boards: a generated
    /// arena puts spawner holes *in* the border tiles a wrapping creature
    /// walks through.
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

    /// Whatever it is asked, the draw hands back a permutation: every spot
    /// used once and no spot twice. Two seats on one spot is two castles
    /// on one tile, and a spot left out is a seat with nowhere to sit.
    #[test]
    fn the_seat_draw_is_always_a_permutation_of_the_spots() {
        for seats in 0..=(MAX_PLAYERS as u8 + 2) {
            for seed in [0u64, 1, 7, 42, 0xDECAF, u64::MAX] {
                let spots = seat_spots(seed, seats, false);
                let mut sorted = spots;
                sorted.sort_unstable();
                assert_eq!(
                    sorted,
                    std::array::from_fn::<usize, MAX_PLAYERS, _>(|i| i),
                    "{seats} seats on seed {seed} drew {spots:?}"
                );
            }
        }
    }

    /// The same seed draws the same beach, every time and on every
    /// machine: it is what lets a host send a number instead of a board,
    /// and what a replay leans on to lay the arena out again.
    #[test]
    fn a_seed_draws_the_same_seats_every_time_and_a_different_one_does_not() {
        for seats in 2..=MAX_PLAYERS as u8 {
            assert_eq!(
                seat_spots(0xC0FFEE, seats, false),
                seat_spots(0xC0FFEE, seats, false),
                "{seats} seats moved between two draws of one seed"
            );
        }
        // Not every seed differs from every other at a two-seat table,
        // where there are only two arrangements, but over a spread of them
        // the draw has to be doing something.
        let drawn: std::collections::HashSet<_> =
            (0..64u64).map(|seed| seat_spots(seed, 6, false)).collect();
        assert!(drawn.len() > 1, "every seed drew the same six seats");
    }

    /// A mirror group is four copies of one spot, one per quadrant, and
    /// all of them on the board: the generator paints through this, so a
    /// corner that ran off the edge would paint off the end of the tiles.
    #[test]
    fn a_mirror_group_is_four_places_on_the_board() {
        for (w, h) in [(9u8, 7u8), (12, 9), (21, 13), (2, 2)] {
            for y in 0..h {
                for x in 0..w {
                    let four = quad(w, h, x, y);
                    assert!(
                        four.iter().all(|&(qx, qy)| qx < w && qy < h),
                        "{w}x{h} at ({x},{y}) left the board: {four:?}"
                    );
                    assert!(
                        four.contains(&(x, y)),
                        "the spot itself is in its own group"
                    );
                    // Mirroring twice is where you started, so the group of
                    // any member is the same group.
                    let mut a = four;
                    let mut b = quad(w, h, four[3].0, four[3].1);
                    a.sort_unstable();
                    b.sort_unstable();
                    assert_eq!(a, b, "{w}x{h} at ({x},{y}) mirrors to another group");
                }
            }
        }
    }
}
