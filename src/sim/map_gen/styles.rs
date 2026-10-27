//! Generator styles: the terrain step of [`generate_styled`]. A style takes
//! the frame today's generator builds (castles dealt to the seats from the
//! seed, the spawner holes for the table, the versus rules), clears the
//! classic terrain off it, and lays its own in groups of four mirror
//! images, so the four corners stay as interchangeable as on any generated
//! beach. Never on a castle spot, the tiles beside one, a hole or the tile
//! in front of it.
//!
//! [`generate_styled`]: super::generate_styled

use super::{QUAD_FLIPS, castle_spots, mirror_dir, mirrored_tile, quad};
use crate::sim::board::{Board, TileKind};
use crate::sim::direction::Direction;
use crate::sim::playable::unplayable;
use crate::sim::rng::Pcg32;

/// The terrain a generated beach is laid with.
///
/// Measured by bots over 3000 rounds a table size (`examples/balance`):
/// every style is fair per seat at every table, and by castle spot at two
/// to four seats as today's beaches are. At six seats Maze, Driftwood,
/// Islands and Lagoon even out the long-edge castles' lead that today's
/// terrain gives them; Lanes keeps it, so the Surprise card deals Lanes
/// only to tables of four or fewer ([`surprise_style`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BeachStyle {
    /// Today's generated beach: rocks, walls, a pool, kelp and a ring of
    /// logs round the middle. Every seed builds the beach it always has.
    #[default]
    Classic,
    /// Pool channels cut the beach into islands crossed by sand bridges:
    /// wading is half speed, so the bridges are the roads.
    Islands,
    /// Long sea walls split the beach into lanes with a gap or two each.
    Lanes,
    /// A big shallow pool in the middle with a kelp shore: the short way
    /// across is slow, and walking gulls cannot cross the weed.
    Lagoon,
    /// A braided maze: a spanning tree over one quarter of the beach, most
    /// of its walls knocked through, mirrored into the other three.
    Maze,
    /// Logs strewn over the beach, each dealing its crabs left and right
    /// in turn.
    Driftwood,
}

impl BeachStyle {
    /// Every style, in the order its token and the wire read them.
    pub const ALL: [BeachStyle; 6] = [
        BeachStyle::Classic,
        BeachStyle::Islands,
        BeachStyle::Lanes,
        BeachStyle::Lagoon,
        BeachStyle::Maze,
        BeachStyle::Driftwood,
    ];

    /// The style's name in a bot command line and a test's message.
    pub fn token(self) -> &'static str {
        match self {
            BeachStyle::Classic => "classic",
            BeachStyle::Islands => "islands",
            BeachStyle::Lanes => "lanes",
            BeachStyle::Lagoon => "lagoon",
            BeachStyle::Maze => "maze",
            BeachStyle::Driftwood => "driftwood",
        }
    }

    pub fn from_token(token: &str) -> Option<BeachStyle> {
        BeachStyle::ALL
            .into_iter()
            .find(|style| style.token() == token)
    }

    /// The style's own random stream, apart from the frame's, so laying it
    /// cannot move a castle or a hole.
    fn stream(self) -> u64 {
        match self {
            BeachStyle::Classic => 0x0a_2e4a,
            BeachStyle::Islands => 0x57_0000,
            BeachStyle::Lanes => 0x57_0001,
            BeachStyle::Lagoon => 0x57_0002,
            BeachStyle::Maze => 0x57_0004,
            BeachStyle::Driftwood => 0x57_0005,
        }
    }
}

/// The biggest table the Surprise card deals Lanes to.
pub const SURPRISE_LANES_MOST: u8 = 4;

/// The styles the Surprise card deals from: Maze, Driftwood, Islands and
/// Lagoon at any table, and Lanes at four or fewer, where it measured as
/// fair as the rest (at five and six its edge castles won 20-21% against
/// the corners' 13-15%).
pub fn surprise_styles(seats: u8) -> Vec<BeachStyle> {
    let mut styles = vec![
        BeachStyle::Maze,
        BeachStyle::Driftwood,
        BeachStyle::Islands,
        BeachStyle::Lagoon,
    ];
    if seats <= SURPRISE_LANES_MOST {
        styles.push(BeachStyle::Lanes);
    }
    styles
}

/// The style a Surprise round at `seed` is laid in: drawn from the seed on
/// a stream of its own, so every peer and every replay of the round draws
/// the same one, and the next round, on the next seed, likely another.
pub fn surprise_style(seed: u64, seats: u8) -> BeachStyle {
    let styles = surprise_styles(seats);
    let mut rng = Pcg32::new(seed, 0x5029_175e);
    styles[rng.next_u32() as usize % styles.len()]
}

/// The size a Surprise beach is built at for a table of `seats`: the
/// classic 12x9 up to four seats and 16x11 (widened to 17 by the frame, so
/// the long-edge castles share a centre column) for five and six, the
/// sizes the styles were measured at.
pub fn surprise_size(seats: u8) -> (u8, u8) {
    match seats {
        0..=4 => (12, 9),
        _ => (16, 11),
    }
}

/// splitmix64 of `seed` and `salt`: a fresh seed for each attempt at a
/// layout, sharing nothing with its neighbours.
fn mix(seed: u64, salt: u64) -> u64 {
    let mut z = seed.wrapping_add(salt).wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// How many layouts a style tries before it settles for the bare frame.
/// Over 1000 boards a style (seeds 0-199 at two to six seats) none needed
/// the frame; the bound keeps the generator from looping on a seed that
/// would.
const ATTEMPTS: u64 = 24;

/// Lay `style` over the beach `classic` (what the generator built at
/// `seed`): its terrain cleared, and the style's laid until a layout
/// passes [`unplayable`].
pub(super) fn restyle(classic: Board, style: BeachStyle, seed: u64) -> Board {
    let frame = cleared(classic);
    for attempt in 0..ATTEMPTS {
        let mut board = frame.clone();
        let mut rng = Pcg32::new(
            mix(seed, attempt.wrapping_mul(0x1_0000_0001)),
            style.stream(),
        );
        let t = Terrain::new(&board);
        match style {
            BeachStyle::Classic => return board,
            BeachStyle::Islands => islands(&mut board, &t, &mut rng),
            BeachStyle::Lanes => lanes(&mut board, &t, &mut rng),
            BeachStyle::Lagoon => lagoon(&mut board, &t, &mut rng),
            BeachStyle::Maze => maze(&mut board, &t, &mut rng),
            BeachStyle::Driftwood => driftwood(&mut board, &t, &mut rng),
        }
        if unplayable(&board).is_empty() {
            return board;
        }
    }
    frame
}

/// The beach with its terrain taken off: castles, holes, rules and rim
/// stay, rocks, kelp, pools, logs and inner walls go.
fn cleared(mut board: Board) -> Board {
    let (w, h) = (board.width(), board.height());
    for y in 0..h {
        for x in 0..w {
            match board.tile_at(x, y) {
                TileKind::Rock | TileKind::Kelp | TileKind::Pool | TileKind::Turnstile { .. } => {
                    board.set_tile(x, y, TileKind::Empty);
                }
                TileKind::Empty | TileKind::Castle(_) | TileKind::Spawner(_) => {}
            }
            if x + 1 < w {
                board.set_wall(x, y, Direction::Right, false);
            }
            if y + 1 < h {
                board.set_wall(x, y, Direction::Down, false);
            }
        }
    }
    board
}

/// What a style may not build on: every spot a castle could stand on
/// (the corners always, so a style's terrain does not move with the seat
/// count; the long-edge pair on a table that uses it), the tiles beside
/// them, the holes and the tile in front of each.
struct Terrain {
    w: u8,
    h: u8,
    keep: Vec<bool>,
}

impl Terrain {
    fn new(board: &Board) -> Terrain {
        let (w, h) = (board.width(), board.height());
        let mut t = Terrain {
            w,
            h,
            keep: vec![false; usize::from(w) * usize::from(h)],
        };
        let spots = castle_spots(w, h);
        let edges_used = spots[4..]
            .iter()
            .any(|&(x, y)| matches!(board.tile_at(x, y), TileKind::Castle(_)));
        for (i, &(x, y)) in spots.iter().enumerate() {
            if i < 4 || edges_used {
                t.keep_around(x, y);
            }
        }
        for (x, y, kind) in board.tiles() {
            if let TileKind::Spawner(spawner) = kind {
                t.mark(i32::from(x), i32::from(y));
                let (dx, dy) = spawner.dir.offset();
                t.mark(i32::from(x) + dx, i32::from(y) + dy);
            }
        }
        t
    }

    fn mark(&mut self, x: i32, y: i32) {
        if (0..i32::from(self.w)).contains(&x) && (0..i32::from(self.h)).contains(&y) {
            self.keep[y as usize * usize::from(self.w) + x as usize] = true;
        }
    }

    fn keep_around(&mut self, x: u8, y: u8) {
        self.mark(i32::from(x), i32::from(y));
        for dir in Direction::ALL {
            let (dx, dy) = dir.offset();
            self.mark(i32::from(x) + dx, i32::from(y) + dy);
        }
    }

    fn kept(&self, x: u8, y: u8) -> bool {
        self.keep[usize::from(y) * usize::from(self.w) + usize::from(x)]
    }

    /// `kind` on all four images of `(x, y)` when every one is free sand
    /// and not kept.
    fn put(&self, board: &mut Board, x: u8, y: u8, kind: TileKind) -> bool {
        if x >= self.w || y >= self.h {
            return false;
        }
        let images = quad(self.w, self.h, x, y);
        if images
            .iter()
            .any(|&(ix, iy)| self.kept(ix, iy) || board.tile_at(ix, iy) != TileKind::Empty)
        {
            return false;
        }
        for (&(ix, iy), (flip_x, flip_y)) in images.iter().zip(QUAD_FLIPS) {
            board.set_tile(ix, iy, mirrored_tile(kind, flip_x, flip_y));
        }
        true
    }

    /// A wall on the `dir` side of `(x, y)` and its three images, unless
    /// one would stand between two kept tiles or against a castle.
    fn wall(&self, board: &mut Board, x: u8, y: u8, dir: Direction) -> bool {
        let (dx, dy) = dir.offset();
        let (nx, ny) = (i32::from(x) + dx, i32::from(y) + dy);
        if nx < 0 || ny < 0 || nx >= i32::from(self.w) || ny >= i32::from(self.h) {
            return false;
        }
        let images = quad(self.w, self.h, x, y);
        let sides = images.iter().zip(QUAD_FLIPS).map(|(&(ix, iy), (fx, fy))| {
            let d = mirror_dir(dir, fx, fy);
            let (ddx, ddy) = d.offset();
            let (jx, jy) = ((i32::from(ix) + ddx) as u8, (i32::from(iy) + ddy) as u8);
            (ix, iy, jx, jy, d)
        });
        let sides: Vec<(u8, u8, u8, u8, Direction)> = sides.collect();
        for &(ix, iy, jx, jy, _) in &sides {
            if self.kept(ix, iy) && self.kept(jx, jy) {
                return false;
            }
            if matches!(board.tile_at(ix, iy), TileKind::Castle(_))
                || matches!(board.tile_at(jx, jy), TileKind::Castle(_))
            {
                return false;
            }
        }
        for (ix, iy, _, _, d) in sides {
            board.set_wall(ix, iy, d, true);
        }
        true
    }
}

/// A draw in `0..n` (`n` of zero reads as one).
fn roll(rng: &mut Pcg32, n: u32) -> u32 {
    rng.next_u32() % n.max(1)
}

/// A few tiles of `kind` in mirror groups, none beside anything already
/// laid.
fn scatter(board: &mut Board, t: &Terrain, rng: &mut Pcg32, kind: TileKind, groups: u32) {
    let mut placed = 0;
    for _ in 0..60 {
        if placed >= groups {
            break;
        }
        let x = 2 + roll(rng, u32::from(t.w - 4)) as u8;
        let y = 1 + roll(rng, u32::from(t.h - 2)) as u8;
        let crowded = Direction::ALL.iter().any(|d| {
            let (dx, dy) = d.offset();
            let (nx, ny) = (i32::from(x) + dx, i32::from(y) + dy);
            nx >= 0
                && ny >= 0
                && nx < i32::from(t.w)
                && ny < i32::from(t.h)
                && board.tile_at(nx as u8, ny as u8) != TileKind::Empty
        });
        if !crowded && t.put(board, x, y, kind) {
            placed += 1;
        }
    }
}

/// Islands: pool channels down the middle (or a mirrored pair a third of
/// the way in) and along the middle row, each with a sand bridge.
fn islands(board: &mut Board, t: &Terrain, rng: &mut Pcg32) {
    let (w, h) = (t.w, t.h);
    let cols: Vec<u8> = if roll(rng, 2) == 0 {
        vec![(w - 1) / 2]
    } else {
        vec![w / 3 - u8::from(w > 12)]
    };
    let row = (h - 1) / 2;
    let vbridge = 1 + roll(rng, u32::from(row.max(2) - 1)) as u8;
    let hbridge = 1 + roll(rng, u32::from((w - 1) / 2 - 1)) as u8;
    for &c in &cols {
        for y in 0..=row {
            if y != vbridge {
                t.put(board, c, y, TileKind::Pool);
            }
        }
    }
    for x in 0..=(w - 1) / 2 {
        if x != hbridge && !cols.contains(&x) {
            t.put(board, x, row, TileKind::Pool);
        }
    }
    let n = 1 + roll(rng, 2);
    scatter(board, t, rng, TileKind::Rock, n);
}

/// Lanes: a sea wall a third of the way down (two on a tall beach), with a
/// gap or two each, and a baffle or two inside the lanes.
fn lanes(board: &mut Board, t: &Terrain, rng: &mut Pcg32) {
    let (w, h) = (t.w, t.h);
    let mut rows = vec![h / 3 - 1];
    if h >= 11 && roll(rng, 2) == 0 {
        rows = vec![1, h / 3 + 1];
    }
    let half = (w - 1) / 2;
    for &r in &rows {
        let gaps = 1 + roll(rng, 2) as u8;
        let mut gap_at: Vec<u8> = Vec::new();
        for _ in 0..gaps {
            gap_at.push(1 + roll(rng, u32::from(half)) as u8);
        }
        for x in 1..=half {
            if !gap_at.contains(&x) {
                t.wall(board, x, r, Direction::Down);
            }
        }
    }
    for _ in 0..1 + roll(rng, 2) {
        let x = 2 + roll(rng, u32::from(half - 2)) as u8;
        let y = 1 + roll(rng, u32::from(h / 2 - 1)) as u8;
        t.wall(board, x, y, Direction::Right);
    }
    let n = roll(rng, 2);
    scatter(board, t, rng, TileKind::Rock, n);
}

/// Lagoon: an elliptical pool in the middle, a kelp shore round some of
/// it, and sometimes a rock islet. The ellipse is tested in whole numbers
/// (doubled coordinates, radii in hundredths of the beach), so every
/// platform lays the same shore.
fn lagoon(board: &mut Board, t: &Terrain, rng: &mut Pcg32) {
    let (w, h) = (t.w, t.h);
    // Radii: the beach's width and height times 0.18, 0.22 or 0.26.
    let rx = i64::from(w) * (18 + 4 * i64::from(roll(rng, 3)));
    let ry = i64::from(h) * (18 + 4 * i64::from(roll(rng, 3)));
    let kelp_ring = roll(rng, 3) as u8;
    // ((x - cx) / rx')^2 + ((y - cy) / ry')^2 with rx' = rx / 100 and
    // cx = (w - 1) / 2, over the common denominator: dx2 = 2x - (w - 1).
    let (rx2, ry2) = (rx * rx, ry * ry);
    let reach = |x: u8, y: u8| {
        let dx2 = 2 * i64::from(x) - i64::from(w - 1);
        let dy2 = 2 * i64::from(y) - i64::from(h - 1);
        2500 * (dx2 * dx2 * ry2 + dy2 * dy2 * rx2)
    };
    let whole = rx2 * ry2;
    for y in 0..=(h - 1) / 2 {
        for x in 0..=(w - 1) / 2 {
            let d = reach(x, y);
            if d <= whole {
                t.put(board, x, y, TileKind::Pool);
            } else if 10 * d <= 19 * whole && (x + y) % 3 < kelp_ring {
                t.put(board, x, y, TileKind::Kelp);
            }
        }
    }
    if roll(rng, 2) == 0 {
        let (mx, my) = ((w - 1) / 2, (h - 1) / 2);
        for (x, y) in quad(w, h, mx, my) {
            board.set_tile(x, y, TileKind::Rock);
        }
    }
    let n = roll(rng, 2);
    scatter(board, t, rng, TileKind::Rock, n);
}

/// Maze: a spanning tree over one quarter of the beach, about a third of
/// the walls it leaves standing kept, mirrored into the other three.
fn maze(board: &mut Board, t: &Terrain, rng: &mut Pcg32) {
    let (w, h) = (t.w, t.h);
    let (qw, qh) = (w.div_ceil(2), h.div_ceil(2));
    let idx = |x: u8, y: u8| usize::from(y) * usize::from(qw) + usize::from(x);
    let mut visited = vec![false; usize::from(qw) * usize::from(qh)];
    // The tree's open edges, each as (x, y, Right or Down).
    let mut open: Vec<(u8, u8, Direction)> = Vec::new();
    let start = (
        roll(rng, u32::from(qw)) as u8,
        roll(rng, u32::from(qh)) as u8,
    );
    let mut stack = vec![start];
    visited[idx(start.0, start.1)] = true;
    while let Some(&(x, y)) = stack.last() {
        let mut next = Vec::new();
        for dir in Direction::ALL {
            let (dx, dy) = dir.offset();
            let (nx, ny) = (i32::from(x) + dx, i32::from(y) + dy);
            if nx >= 0
                && ny >= 0
                && nx < i32::from(qw)
                && ny < i32::from(qh)
                && !visited[idx(nx as u8, ny as u8)]
            {
                next.push((nx as u8, ny as u8, dir));
            }
        }
        if next.is_empty() {
            stack.pop();
            continue;
        }
        let (nx, ny, dir) = next[roll(rng, next.len() as u32) as usize];
        visited[idx(nx, ny)] = true;
        open.push(match dir {
            Direction::Right | Direction::Down => (x, y, dir),
            Direction::Left => (nx, ny, Direction::Right),
            Direction::Up => (nx, ny, Direction::Down),
        });
        stack.push((nx, ny));
    }
    let keep = 30 + roll(rng, 15);
    for y in 0..qh {
        for x in 0..qw {
            for dir in [Direction::Right, Direction::Down] {
                // An edge on a mirror axis stays open: its image is the
                // other side of the same edge.
                let on_axis = match dir {
                    Direction::Right => x + 1 >= qw,
                    Direction::Down | Direction::Left | Direction::Up => y + 1 >= qh,
                };
                if on_axis || open.contains(&(x, y, dir)) {
                    continue;
                }
                if roll(rng, 100) < keep {
                    t.wall(board, x, y, dir);
                }
            }
        }
    }
}

/// Driftwood: logs off both mirror axes (a log on an axis is its own
/// opposite, and a handed thing cannot be), never two side by side.
fn driftwood(board: &mut Board, t: &Terrain, rng: &mut Pcg32) {
    let (w, h) = (t.w, t.h);
    let groups = 2 + roll(rng, 2) + u32::from(w) * u32::from(h) / 150;
    let mut placed = 0;
    for _ in 0..80 {
        if placed >= groups {
            break;
        }
        let x = 1 + roll(rng, u32::from((w - 1) / 2)) as u8;
        let y = 1 + roll(rng, u32::from((h - 1) / 2)) as u8;
        if (w % 2 == 1 && x == (w - 1) / 2) || (h % 2 == 1 && y == (h - 1) / 2) {
            continue;
        }
        let near = quad(w, h, x, y).iter().any(|&(ix, iy)| {
            Direction::ALL.iter().any(|d| {
                let (dx, dy) = d.offset();
                let (nx, ny) = (i32::from(ix) + dx, i32::from(iy) + dy);
                nx >= 0
                    && ny >= 0
                    && nx < i32::from(w)
                    && ny < i32::from(h)
                    && matches!(
                        board.tile_at(nx as u8, ny as u8),
                        TileKind::Turnstile { .. }
                    )
            })
        });
        if near {
            continue;
        }
        let kind = TileKind::Turnstile {
            next_right: roll(rng, 2) == 0,
        };
        if t.put(board, x, y, kind) {
            placed += 1;
        }
    }
    let n = 1 + roll(rng, 2);
    scatter(board, t, rng, TileKind::Rock, n);
}
