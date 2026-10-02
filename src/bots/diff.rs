//! What one tick did, read off the board before and after it: which crabs
//! banked and for whom, which were eaten, which castles were raided, which
//! signposts wore or went. The standings count it and `simulate` reports it.
//!
//! Read from the outside rather than recorded by the sim, which keeps no
//! such log: the sim's state is exactly what its hash covers, and a record
//! nothing in the sim reads would be a field for nothing.

use crate::sim::{Board, Direction, MAX_PLAYERS, PlayerId, SignpostHealth, TileKind};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Happened {
    Banked {
        crab: u32,
        owner: PlayerId,
        x: u8,
        y: u8,
    },
    Eaten {
        crab: u32,
        x: u8,
        y: u8,
    },
    Raided {
        owner: PlayerId,
        lost: u32,
    },
    PostWorn {
        x: u8,
        y: u8,
        owner: PlayerId,
    },
    PostGone {
        x: u8,
        y: u8,
        owner: PlayerId,
    },
}

/// The parts of a board a diff needs, taken before a tick.
pub struct Before {
    crabs: HashMap<u32, (u16, u8, u8)>,
    posts: Vec<(u8, u8, PlayerId, SignpostHealth, u64)>,
    scores: [u32; MAX_PLAYERS],
    claw_call: bool,
}

impl Before {
    pub fn of(board: &Board) -> Before {
        Before {
            crabs: board
                .crabs()
                .iter()
                .map(|c| {
                    let (x, y) = board.coords_u8(c.tile);
                    (c.id, (c.tile, x, y))
                })
                .collect(),
            posts: board
                .signposts()
                .map(|(x, y, p)| (x, y, p.owner, p.health, p.placed))
                .collect(),
            scores: *board.scores(),
            claw_call: board.in_claw_call(),
        }
    }

    /// What happened between this reading and `after`.
    pub fn diff(&self, after: &Board, out: &mut Vec<Happened>) {
        let alive: std::collections::HashSet<u32> = after.crabs().iter().map(|c| c.id).collect();
        let mut gone: Vec<(&u32, &(u16, u8, u8))> = self
            .crabs
            .iter()
            .filter(|(id, _)| !alive.contains(id))
            .collect();
        gone.sort_by_key(|(id, _)| **id);
        for (&crab, &(tile, x, y)) in gone {
            let owner = after
                .swept_home(crab)
                .or_else(|| castle_beside(after, tile, &self.scores));
            out.push(match owner {
                Some(owner) => {
                    let (x, y) = after.castle_of(owner).unwrap_or((x, y));
                    Happened::Banked { crab, owner, x, y }
                }
                None => Happened::Eaten { crab, x, y },
            });
        }
        if !self.claw_call {
            for (seat, (&was, &now)) in self.scores.iter().zip(after.scores()).enumerate() {
                if now < was {
                    out.push(Happened::Raided {
                        owner: seat as PlayerId,
                        lost: was - now,
                    });
                }
            }
        }
        for &(x, y, owner, health, placed) in &self.posts {
            match after.signpost_at(x, y) {
                Some(post) if post.owner == owner && post.placed == placed => {
                    if health == SignpostHealth::Full && post.health == SignpostHealth::Worn {
                        out.push(Happened::PostWorn { x, y, owner });
                    }
                }
                // Re-pointed by its owner: still standing, freshly.
                Some(post) if post.owner == owner => {}
                _ => out.push(Happened::PostGone { x, y, owner }),
            }
        }
    }
}

/// The castle a crab that vanished from `tile` walked into: on it or next
/// to it, and holding a seat whose score went up.
fn castle_beside(after: &Board, tile: u16, before: &[u32; MAX_PLAYERS]) -> Option<PlayerId> {
    let (x, y) = after.coords_u8(tile);
    let mut spots = vec![(x, y)];
    for dir in Direction::ALL {
        let (dx, dy) = dir.offset();
        let (nx, ny) = (i32::from(x) + dx, i32::from(y) + dy);
        if (0..i32::from(after.width())).contains(&nx)
            && (0..i32::from(after.height())).contains(&ny)
        {
            spots.push((nx as u8, ny as u8));
        }
    }
    spots
        .into_iter()
        .find_map(|(x, y)| match after.tile_at(x, y) {
            TileKind::Castle(owner)
                if after.scores()[usize::from(owner)] != before[usize::from(owner)] =>
            {
                Some(owner)
            }
            TileKind::Castle(_)
            | TileKind::Empty
            | TileKind::Rock
            | TileKind::Spawner(_)
            | TileKind::Turnstile { .. }
            | TileKind::Kelp
            | TileKind::Pool => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{CrabKind, Handedness};

    #[test]
    fn a_crab_walking_home_is_a_bank_for_that_castle() {
        let mut board = Board::new(5, 1, 1);
        board.set_tile(4, 0, TileKind::Castle(1));
        board.spawn_crab(2, 0, Direction::Right, Handedness::Left, CrabKind::Common);
        let mut seen = Vec::new();
        for _ in 0..200 {
            let before = Before::of(&board);
            board.tick_idle();
            before.diff(&board, &mut seen);
        }
        assert!(
            matches!(
                seen.as_slice(),
                [Happened::Banked {
                    owner: 1,
                    x: 4,
                    y: 0,
                    ..
                }]
            ),
            "{seen:?}"
        );
    }
}
