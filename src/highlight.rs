//! The highlight reel: the last fifteen seconds of a finished round, zoomed
//! on the castle that decided it, written out as an animated GIF.
//!
//! It is built by re-simulating the round's [`Replay`] rather than by
//! capturing the screen: determinism means the recording *is* the round, so
//! the reel needs no render pipeline, no window, and no timing luck. It is
//! also why this lives beside the sim rather than in the Bevy shell: a
//! headless little rasterizer over board state (see [`crate::gif`] for the
//! encoder underneath).

use crate::gif::Gif;
use crate::sim::{
    Board, CrabKind, Direction, GullState, MAX_PLAYERS, Replay, TICKS_PER_SECOND, TileKind,
};

/// How much of the ending the reel keeps.
pub const REEL_SECONDS: u32 = 15;
/// Frames a second in the finished GIF.
const FPS: u32 = 10;
/// Sim ticks between captured frames.
const TICKS_PER_FRAME: u32 = TICKS_PER_SECOND / FPS;
/// Pixels per board tile.
const TILE: u32 = 16;
/// The zoom: how many tiles of board the reel frames.
const WINDOW_W: u8 = 9;
const WINDOW_H: u8 = 7;

/// Palette slots. Kept small and named so the drawing code reads as
/// colours rather than as magic numbers.
mod ink {
    pub const SAND: u8 = 0;
    pub const SAND_DARK: u8 = 1;
    pub const ROCK: u8 = 2;
    pub const WALL: u8 = 3;
    pub const KELP: u8 = 4;
    pub const POOL: u8 = 5;
    pub const LOG: u8 = 6;
    pub const HOLE: u8 = 7;
    pub const GULL: u8 = 8;
    pub const CRAB: u8 = 9;
    pub const GOLDEN: u8 = 10;
    pub const GIANT: u8 = 11;
    /// Seat colours occupy the last slots, one per seat. They restate
    /// `app::palette`'s flag colours as raw bytes, because this module is
    /// engine-free and that one speaks `bevy::Color`; a test holds the two
    /// in step.
    pub const SEAT: u8 = 12;
}

const PALETTE: [[u8; 3]; 18] = [
    [222, 196, 148], // sand
    [206, 180, 132], // sand, the darker checker
    [96, 96, 104],   // rock
    [82, 62, 44],    // wall
    [46, 132, 72],   // kelp
    [96, 168, 208],  // pool
    [122, 88, 54],   // driftwood log
    [56, 40, 30],    // spawner hole
    [244, 244, 250], // gull
    [214, 92, 60],   // crab
    [240, 206, 74],  // golden crab
    [150, 52, 44],   // giant crab
    [214, 68, 68],   // seat 1
    [64, 120, 202],  // seat 2
    [76, 176, 79],   // seat 3
    [227, 186, 58],  // seat 4
    [178, 102, 209], // seat 5
    [242, 140, 51],  // seat 6
];

/// The highest seat with a colour of its own, for clamping.
const MAX_SEAT: u8 = MAX_PLAYERS as u8 - 1;

/// A single frame's worth of palette indices.
struct Frame {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    /// Board tile at the frame's top-left corner.
    origin: (u8, u8),
}

impl Frame {
    fn new(origin: (u8, u8)) -> Frame {
        let (width, height) = (u32::from(WINDOW_W) * TILE, u32::from(WINDOW_H) * TILE);
        Frame {
            pixels: vec![ink::SAND; (width * height) as usize],
            width,
            height,
            origin,
        }
    }

    fn set(&mut self, x: u32, y: u32, color: u8) {
        if x < self.width && y < self.height {
            self.pixels[(y * self.width + x) as usize] = color;
        }
    }

    fn rect(&mut self, x: i64, y: i64, w: u32, h: u32, color: u8) {
        for dy in 0..h {
            for dx in 0..w {
                let (px, py) = (x + i64::from(dx), y + i64::from(dy));
                if px >= 0 && py >= 0 {
                    self.set(px as u32, py as u32, color);
                }
            }
        }
    }

    /// Where a board position in subunits lands in the frame, in pixels.
    /// Positions outside the window come back anyway (negative or large);
    /// [`Frame::rect`] clips them.
    fn at(&self, tile_x: f32, tile_y: f32) -> (i64, i64) {
        let x = (tile_x - f32::from(self.origin.0)) * TILE as f32;
        let y = (tile_y - f32::from(self.origin.1)) * TILE as f32;
        (x as i64, y as i64)
    }
}

/// Which seat's castle the reel should frame: the winner, or, if the round
/// ended level, whoever banked the most while the reel was running.
fn deciding_seat(final_scores: &[u32; MAX_PLAYERS], gains: &[u32; MAX_PLAYERS]) -> u8 {
    let best = final_scores.iter().max().copied().unwrap_or(0);
    let leaders: Vec<usize> = (0..MAX_PLAYERS)
        .filter(|&s| final_scores[s] == best)
        .collect();
    let pick = match leaders.as_slice() {
        [only] => *only,
        _ => leaders
            .iter()
            .copied()
            .max_by_key(|&seat| gains[seat])
            .unwrap_or(0),
    };
    pick as u8
}

/// The window's top-left tile: centred on `focus`, nudged back inside the
/// board so the reel never frames empty space off the edge.
fn window_origin(board: &Board, focus: (u8, u8)) -> (u8, u8) {
    let clamp = |value: u8, window: u8, size: u8| {
        let half = window / 2;
        if size <= window {
            0
        } else {
            value.saturating_sub(half).min(size - window)
        }
    };
    (
        clamp(focus.0, WINDOW_W, board.width()),
        clamp(focus.1, WINDOW_H, board.height()),
    )
}

/// Draw one frame of board state.
fn draw(board: &Board, origin: (u8, u8)) -> Frame {
    let mut frame = Frame::new(origin);
    for row in 0..WINDOW_H {
        for col in 0..WINDOW_W {
            let (x, y) = (origin.0 + col, origin.1 + row);
            if x >= board.width() || y >= board.height() {
                continue;
            }
            let (px, py) = (u32::from(col) * TILE, u32::from(row) * TILE);
            let checker = if (x + y) % 2 == 0 {
                ink::SAND
            } else {
                ink::SAND_DARK
            };
            let (fill, inset) = match board.tile_at(x, y) {
                TileKind::Empty => (checker, 0),
                TileKind::Rock => (ink::ROCK, 2),
                TileKind::Kelp => (ink::KELP, 2),
                TileKind::Pool => (ink::POOL, 0),
                TileKind::Turnstile { .. } => (ink::LOG, 5),
                TileKind::Spawner(_) => (ink::HOLE, 3),
                TileKind::Castle(owner) => (ink::SEAT + owner.min(MAX_SEAT), 1),
            };
            if inset > 0 {
                frame.rect(i64::from(px), i64::from(py), TILE, TILE, checker);
            }
            frame.rect(
                i64::from(px + inset),
                i64::from(py + inset),
                TILE - 2 * inset,
                TILE - 2 * inset,
                fill,
            );
            // Walls ride the tile's edges, two pixels thick.
            for (dir, wx, wy, w, h) in [
                (Direction::Up, px, py, TILE, 2),
                (Direction::Down, px, py + TILE - 2, TILE, 2),
                (Direction::Left, px, py, 2, TILE),
                (Direction::Right, px + TILE - 2, py, 2, TILE),
            ] {
                if board.wall_at(x, y, dir) {
                    frame.rect(i64::from(wx), i64::from(wy), w, h, ink::WALL);
                }
            }
            // A signpost is a small chip in its owner's colour.
            if let Some(post) = board.signpost_at(x, y) {
                frame.rect(
                    i64::from(px + 5),
                    i64::from(py + 5),
                    6,
                    6,
                    ink::SEAT + post.owner.min(MAX_SEAT),
                );
            }
        }
    }
    for crab in board.crabs() {
        let color = match crab.kind {
            CrabKind::Golden => ink::GOLDEN,
            CrabKind::Giant => ink::GIANT,
            CrabKind::Common | CrabKind::Juvenile | CrabKind::Molting | CrabKind::Sparkling => {
                ink::CRAB
            }
        };
        let (x, y) = creature_tile_pos(board, crab.tile, crab.dir, crab.progress);
        let (px, py) = frame.at(x, y);
        frame.rect(px + 4, py + 4, 8, 8, color);
    }
    for gull in board.gulls() {
        let (x, y) = creature_tile_pos(board, gull.tile, gull.dir, gull.progress);
        let (px, py) = frame.at(x, y);
        let size = if matches!(gull.state, GullState::Flying { .. }) {
            12
        } else {
            10
        };
        frame.rect(px + 3, py + 3, size, size, ink::GULL);
    }
    frame
}

/// A creature's position in tile units, its part-way progress included.
fn creature_tile_pos(board: &Board, tile: u16, dir: Direction, progress: u16) -> (f32, f32) {
    let (x, y) = board.coords_u8(tile);
    let (dx, dy) = dir.offset();
    let t = f32::from(progress) / f32::from(crate::sim::SUBUNITS_PER_TILE);
    (f32::from(x) + dx as f32 * t, f32::from(y) + dy as f32 * t)
}

/// Build the reel for a finished round: re-simulate the replay, keep the
/// last [`REEL_SECONDS`], and encode it. `None` if the replay is too short
/// to have a board or an ending worth showing.
pub fn reel(replay: &Replay) -> Option<Vec<u8>> {
    if replay.inputs.is_empty() {
        return None;
    }
    let total = replay.inputs.len();
    let window = (REEL_SECONDS * TICKS_PER_SECOND) as usize;
    let start = total.saturating_sub(window);

    // First pass: play it through for the final scores and for how much
    // each seat banked inside the window, the measure of "deciding".
    let mut board = replay.level.board();
    let mut at_start = [0u32; MAX_PLAYERS];
    for (tick, actions) in replay.inputs.iter().enumerate() {
        if tick == start {
            at_start = *board.scores();
        }
        board.tick(actions);
    }
    let finals = *board.scores();
    let gains = std::array::from_fn(|s| finals[s].saturating_sub(at_start[s]));
    let seat = deciding_seat(&finals, &gains);

    // Second pass: the same round again, drawing every few ticks once the
    // window opens.
    let mut board = replay.level.board();
    let focus = board
        .castle_of(seat)
        .unwrap_or((board.width() / 2, board.height() / 2));
    let origin = window_origin(&board, focus);
    let (width, height) = (u32::from(WINDOW_W) * TILE, u32::from(WINDOW_H) * TILE);
    let mut gif = Gif::new(width as u16, height as u16, &PALETTE);
    let delay = (100 / FPS) as u16;
    let mut frames = 0;
    for (tick, actions) in replay.inputs.iter().enumerate() {
        if tick >= start && (tick - start).is_multiple_of(TICKS_PER_FRAME as usize) {
            gif.add_frame(&draw(&board, origin).pixels, delay);
            frames += 1;
        }
        board.tick(actions);
    }
    // Hold on the final board for a beat, the way a highlight should end.
    gif.add_frame(&draw(&board, origin).pixels, delay * 15);
    frames += 1;
    (frames > 1).then(|| gif.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Level, PlayerAction, classic_arena_seeded};

    /// The reel wears the same flags the match did. It cannot share the
    /// palette outright, since that one is `bevy::Color` and this module is
    /// engine-free, so the two copies are held together here instead of by
    /// hoping nobody restyles one of them.
    #[test]
    fn the_reel_flies_the_games_own_flags() {
        for seat in 0..MAX_PLAYERS as u8 {
            let want = crate::app::palette::classic_color(seat).to_srgba();
            let want = [want.red, want.green, want.blue].map(|c| (c * 255.0).round() as i32);
            let got = PALETTE[(ink::SEAT + seat) as usize].map(i32::from);
            for channel in 0..3 {
                assert!(
                    (want[channel] - got[channel]).abs() <= 1,
                    "seat {seat} channel {channel}: reel {got:?} against game {want:?}"
                );
            }
        }
    }

    fn recorded_round(ticks: usize) -> Replay {
        let board = classic_arena_seeded(0x21EE1, false, 4);
        let mut replay = Replay::new(Level::from_board("Reel", 3, board.clone()));
        let mut board = board;
        for tick in 0..ticks {
            let mut actions = [PlayerAction::None; MAX_PLAYERS];
            if tick == 40 {
                actions[0] = PlayerAction::Place {
                    x: 4,
                    y: 4,
                    dir: Direction::Up,
                };
            }
            board.tick(&actions);
            replay.record(actions);
        }
        replay
    }

    #[test]
    fn a_reel_is_a_playable_gif() {
        let replay = recorded_round(900);
        let bytes = reel(&replay).expect("a round of that length has a reel");
        assert_eq!(&bytes[..6], b"GIF89a");
        assert_eq!(*bytes.last().expect("trailer"), 0x3B);
        // One frame per FPS-th of a second across the window, plus the hold.
        let frames = bytes.iter().filter(|&&b| b == 0x2C).count();
        assert!(
            frames >= (REEL_SECONDS * FPS) as usize,
            "expected a full window of frames, got {frames}"
        );
    }

    /// A round shorter than the window still produces a reel, of whatever
    /// there was.
    #[test]
    fn short_rounds_still_make_a_reel() {
        assert!(reel(&recorded_round(60)).is_some());
        assert!(reel(&recorded_round(0)).is_none(), "nothing to show");
    }

    /// The reel frames the winner's castle, and stays on the board.
    #[test]
    fn the_window_follows_the_deciding_castle() {
        let board = classic_arena_seeded(1, false, 4);
        // Seat 2's castle is at the bottom-right on the classic arena.
        let corner = board.castle_of(1).expect("seat 2 has a castle");
        let origin = window_origin(&board, corner);
        assert!(
            origin.0 + WINDOW_W <= board.width() && origin.1 + WINDOW_H <= board.height(),
            "the window stayed on the board: {origin:?}"
        );
        // A castle in the middle of a big board is centred exactly.
        let mut wide = Board::new(20, 13, 1);
        wide.set_tile(10, 6, TileKind::Castle(0));
        assert_eq!(window_origin(&wide, (10, 6)), (6, 3));
    }

    #[test]
    fn the_deciding_seat_is_the_winner_then_the_late_surge() {
        assert_eq!(deciding_seat(&[10, 40, 5, 0, 0, 0], &[0; MAX_PLAYERS]), 1);
        // A dead heat goes to whoever earned it in the closing seconds.
        assert_eq!(deciding_seat(&[40, 40, 0, 0, 0, 0], &[3, 9, 0, 0, 0, 0]), 1);
        assert_eq!(deciding_seat(&[40, 40, 0, 0, 0, 0], &[9, 3, 0, 0, 0, 0]), 0);
    }
}
