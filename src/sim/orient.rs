//! The per-round transform of a versus beach: mirrored left-right and/or
//! top-bottom at random from the round's seed, an open beach built of
//! strips rolled round by a whole number of them, and a handmade beach's
//! castles dealt to the seats in opposite pairs.
//!
//! Why every versus round is mirrored: the sim and the AI have a hand. On a
//! beach that is exactly mirror-symmetric both ways one diagonal pair of
//! castles still won about three points more than the other over 8000 bot
//! rounds (Three Lanes as drawn: 26.6/25.4% against 23.6/22.7%). Mirrored
//! from the seed, whatever the lean is falls on every castle in turn rather
//! than on the same one, and every handmade map read within noise.
//!
//! Every peer builds the round's board from the same seed, so they mirror
//! it alike, and a replay records the board as it was played.

use crate::sim::board::{Board, MAX_PLAYERS, TileKind};
use crate::sim::crab::Handedness;
use crate::sim::direction::Direction;
use crate::sim::map_gen::{mirror_dir, mirrored_tile};
use crate::sim::rng::Pcg32;

/// How a round's beach is turned from the one its map draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Orientation {
    /// Mirrored left-right.
    pub flip_x: bool,
    /// Mirrored top-bottom.
    pub flip_y: bool,
    /// Columns an open beach is rolled round by before the mirror, a whole
    /// number of its strips; 0 on a beach with edges.
    pub roll: u8,
}

impl Orientation {
    /// The orientation of the round played on `seed`, for a beach `width`
    /// wide whose strips are `strip` columns (0 for a beach that does not
    /// roll). Drawn on a stream of its own, so drawing it moves nothing the
    /// seed builds.
    pub fn for_round(seed: u64, width: u8, strip: u8) -> Orientation {
        let mut rng = Pcg32::new(seed, 0x0f11);
        let flip_x = rng.next_u32() % 2 == 1;
        let flip_y = rng.next_u32() % 2 == 1;
        let roll = match strip {
            0 => 0,
            strip => (rng.next_u32() % u32::from((width / strip).max(1))) as u8 * strip,
        };
        Orientation {
            flip_x,
            flip_y,
            roll,
        }
    }

    /// Whether the beach is played as drawn.
    pub fn is_identity(self) -> bool {
        self == Orientation::default()
    }

    /// Where the tile at `(x, y)` lands on a `width` x `height` beach.
    pub fn place(self, (x, y): (u8, u8), width: u8, height: u8) -> (u8, u8) {
        let x = ((u16::from(x) + u16::from(self.roll)) % u16::from(width)) as u8;
        (
            if self.flip_x { width - 1 - x } else { x },
            if self.flip_y { height - 1 - y } else { y },
        )
    }

    /// `board`, a beach at its first tick, turned: every tile, wall,
    /// creature and score moved to its image, each spawner facing, log and
    /// crab's claw mirrored to suit. Rules, seed and edges are the beach's
    /// own.
    pub fn apply(self, board: &Board) -> Board {
        if self.is_identity() {
            return board.clone();
        }
        debug_assert_eq!(board.ticks(), 0, "only a beach at its first tick is turned");
        debug_assert!(
            self.roll == 0 || board.wrap(),
            "only an open beach rolls round"
        );
        let (w, h) = (board.width(), board.height());
        let (fx, fy) = (self.flip_x, self.flip_y);
        let mut out = Board::new(w, h, board.seed());
        out.set_wrap(board.wrap());
        *out.rules_mut() = *board.rules();
        out.set_events_enabled(board.events_enabled());
        for (x, y, kind) in board.tiles() {
            let (ix, iy) = self.place((x, y), w, h);
            out.set_tile(ix, iy, mirrored_tile(kind, fx, fy));
        }
        for (x, y, _) in board.tiles() {
            for dir in Direction::ALL {
                let (dx, dy) = dir.offset();
                let (nx, ny) = (i32::from(x) + dx, i32::from(y) + dy);
                let inner = (0..i32::from(w)).contains(&nx) && (0..i32::from(h)).contains(&ny);
                if inner && board.wall_at(x, y, dir) {
                    let (ix, iy) = self.place((x, y), w, h);
                    let turned = mirror_dir(dir, fx, fy);
                    debug_assert!(
                        !out.on_open_rim(ix, iy, turned),
                        "a wall rolled onto the seam of {w}x{h}"
                    );
                    out.set_wall(ix, iy, turned, true);
                }
            }
        }
        // A mirror swaps left and right, so a crab's claw swaps with it;
        // two mirrors are a half turn, which keeps it.
        let swap_claws = fx ^ fy;
        for crab in board.crabs() {
            let (ix, iy) = self.place(board.coords_u8(crab.tile), w, h);
            let handed = match (swap_claws, crab.handed) {
                (false, handed) => handed,
                (true, Handedness::Left) => Handedness::Right,
                (true, Handedness::Right) => Handedness::Left,
            };
            out.spawn_crab(ix, iy, mirror_dir(crab.dir, fx, fy), handed, crab.kind);
        }
        for gull in board.gulls() {
            let (ix, iy) = self.place(board.coords_u8(gull.tile), w, h);
            out.spawn_gull(ix, iy, mirror_dir(gull.dir, fx, fy));
        }
        for (seat, &score) in board.scores().iter().enumerate() {
            if score > 0 {
                out.set_score(seat as u8, score);
            }
        }
        out
    }
}

/// Deal a handmade beach's castles to a table of `seats` from the round's
/// `seed`, in opposite pairs: the beach draws castles 0/1, 2/3 and 4/5
/// across from each other, and the deal hands each pair of seats a whole
/// pair of castles, either way round. Pairs then still face each other,
/// which is what both team modes are built on (Pairs gives a team a whole
/// pair, Trios a seat from every pair), while who plays which castle moves
/// round to round.
///
/// Castles past the table are expected put away already
/// ([`crate::sim::Level::table_board`]); a seat left over at an odd table
/// keeps its castle, which costs it nothing on the only handmade beaches
/// that seat one: identical strips with a castle each, every castle a
/// copy of the others.
pub fn deal_pairs(board: &mut Board, seats: u8, seed: u64) {
    let pairs = usize::from(seats.min(MAX_PLAYERS as u8) / 2);
    let mut rng = Pcg32::new(seed, 0x0dea_1ca5);
    let mut order: Vec<usize> = (0..pairs).collect();
    for i in (1..pairs).rev() {
        let j = rng.next_u32() as usize % (i + 1);
        order.swap(i, j);
    }
    let mut seat_of: [u8; MAX_PLAYERS] = std::array::from_fn(|castle| castle as u8);
    for (seat_pair, &castle_pair) in order.iter().enumerate() {
        let flip = usize::from(rng.next_u32().is_multiple_of(2));
        seat_of[castle_pair * 2 + flip] = (seat_pair * 2) as u8;
        seat_of[castle_pair * 2 + 1 - flip] = (seat_pair * 2 + 1) as u8;
    }
    let castles: Vec<(u8, u8, u8)> = board
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
    for (x, y, owner) in castles {
        if let Some(&seat) = seat_of.get(usize::from(owner)) {
            board.set_tile(x, y, TileKind::Castle(seat));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::board::Spawner;
    use crate::sim::crab::CrabKind;

    /// A mirror moves each thing to its image and turns what has a facing
    /// or a hand: a hole faces the other way, a log deals the other way
    /// first, a crab walks the other way on the other claw; two mirrors
    /// are a half turn, which keeps every hand.
    #[test]
    fn a_mirror_turns_what_has_a_facing_or_a_hand() {
        let mut board = Board::new(5, 3, 9);
        board.set_tile(
            0,
            1,
            TileKind::Spawner(Spawner {
                dir: Direction::Right,
                period: 40,
            }),
        );
        board.set_tile(1, 0, TileKind::Turnstile { next_right: true });
        board.set_tile(4, 2, TileKind::Castle(0));
        board.set_wall(2, 1, Direction::Right, true);
        board.spawn_crab(2, 2, Direction::Right, Handedness::Left, CrabKind::Molting);
        let flipped = Orientation {
            flip_x: true,
            ..Orientation::default()
        }
        .apply(&board);
        assert_eq!(
            flipped.tile_at(4, 1),
            TileKind::Spawner(Spawner {
                dir: Direction::Left,
                period: 40
            })
        );
        assert_eq!(
            flipped.tile_at(3, 0),
            TileKind::Turnstile { next_right: false }
        );
        assert_eq!(flipped.tile_at(0, 2), TileKind::Castle(0));
        assert!(flipped.wall_at(2, 1, Direction::Left), "the wall's image");
        assert!(!flipped.wall_at(2, 1, Direction::Right));
        let crab = flipped.crabs()[0];
        assert_eq!(flipped.coords_u8(crab.tile), (2, 2));
        assert_eq!(
            (crab.dir, crab.handed),
            (Direction::Left, Handedness::Right)
        );

        let turned = Orientation {
            flip_x: true,
            flip_y: true,
            roll: 0,
        }
        .apply(&board);
        assert_eq!(
            turned.tile_at(3, 2),
            TileKind::Turnstile { next_right: true }
        );
        assert_eq!(
            turned.crabs()[0].handed,
            Handedness::Left,
            "a half turn keeps the claw"
        );
        assert_eq!(turned.tile_at(0, 0), TileKind::Castle(0));
    }

    /// The draw is the seed's, and over many seeds lands on all four
    /// mirrors and every whole strip.
    #[test]
    fn every_round_is_drawn_from_its_seed() {
        assert_eq!(
            Orientation::for_round(42, 21, 7),
            Orientation::for_round(42, 21, 7)
        );
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..200u64 {
            let o = Orientation::for_round(seed, 21, 7);
            assert_eq!(o.roll % 7, 0);
            assert!(o.roll < 21);
            seen.insert((o.flip_x, o.flip_y, o.roll));
        }
        assert_eq!(seen.len(), 12, "four mirrors by three strips: {seen:?}");
        assert!((0..50u64).all(|seed| Orientation::for_round(seed, 13, 0).roll == 0));
    }

    /// The deal keeps every pair of seats across from each other, and over
    /// many seeds gives every seat every castle.
    #[test]
    fn the_deal_keeps_opposite_pairs() {
        let mut seen = [[false; MAX_PLAYERS]; MAX_PLAYERS];
        for seed in 0..300u64 {
            let mut board = Board::new(6, 2, 1);
            for castle in 0..6u8 {
                board.set_tile(castle, castle % 2, TileKind::Castle(castle));
            }
            deal_pairs(&mut board, 6, seed);
            let spot = |seat: u8| board.castle_of(seat).expect("every seat dealt a castle");
            for pair in 0..3u8 {
                let (a, b) = (spot(pair * 2), spot(pair * 2 + 1));
                // Drawn castles 2k and 2k+1 stand in columns 2k and 2k+1.
                assert_eq!(
                    a.0 / 2,
                    b.0 / 2,
                    "seed {seed}: seats {} and {}",
                    pair * 2,
                    pair * 2 + 1
                );
            }
            for seat in 0..6u8 {
                seen[usize::from(seat)][usize::from(spot(seat).0)] = true;
            }
        }
        assert!(seen.iter().flatten().all(|&s| s), "{seen:?}");
    }
}
