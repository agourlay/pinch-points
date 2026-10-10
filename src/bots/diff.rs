//! What one tick did, read off the board before and after it: which crabs
//! banked and for whom, which were eaten, which castles were raided, which
//! signposts wore or went. The standings count it and `simulate` reports it.
//!
//! Read from the outside rather than recorded by the sim, which keeps no
//! such log: the sim's state is exactly what its hash covers, and a record
//! nothing in the sim reads would be a field for nothing.

use crate::sim::{
    Board, CrabKind, Direction, Handedness, MAX_PLAYERS, PlayerId, SignpostHealth, TileKind,
};

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

/// A crab as a diff needs it.
#[derive(Clone, Copy)]
struct Seen {
    id: u32,
    tile: u16,
    dir: Direction,
    progress: u16,
    handed: Handedness,
    kind: CrabKind,
}

/// The parts of a board a diff needs, taken before a tick.
pub struct Before {
    /// In the board's own order, which is the order the tick moves them.
    crabs: Vec<Seen>,
    posts: Vec<(u8, u8, PlayerId, SignpostHealth, u64)>,
    scores: [u32; MAX_PLAYERS],
    banked: u32,
    claw_call: bool,
}

impl Before {
    pub fn of(board: &Board) -> Before {
        Before {
            crabs: board
                .crabs()
                .iter()
                .map(|c| Seen {
                    id: c.id,
                    tile: c.tile,
                    dir: c.dir,
                    progress: c.progress,
                    handed: c.handed,
                    kind: c.kind,
                })
                .collect(),
            posts: board
                .signposts()
                .map(|(x, y, p)| (x, y, p.owner, p.health, p.placed))
                .collect(),
            scores: *board.scores(),
            banked: board.crabs_banked(),
            claw_call: board.in_claw_call(),
        }
    }

    /// What happened between this reading and `after`.
    ///
    /// A crab that is gone banked if the tide swept it home, or if the
    /// castle is the tile it stood on or the one it was walking into: a
    /// crab moves at most a tile a tick, and banks by stepping into a
    /// castle. Being next to a castle whose score went up is not that, and
    /// a score is no witness anyway (Right Claws can bank a crab for
    /// nothing). If more crabs were walking in than the board says banked,
    /// the ones furthest along made it and the rest were eaten short of
    /// the door.
    pub fn diff(&self, after: &Board, out: &mut Vec<Happened>) {
        let alive: std::collections::HashSet<u32> = after.crabs().iter().map(|c| c.id).collect();
        let gone: Vec<&Seen> = self
            .crabs
            .iter()
            .filter(|c| !alive.contains(&c.id))
            .collect();
        let swept = gone
            .iter()
            .filter(|c| after.swept_home(c.id).is_some())
            .count() as u32;
        let walked_in = after
            .crabs_banked()
            .saturating_sub(self.banked)
            .saturating_sub(swept) as usize;
        let mut walking: Vec<(&Seen, PlayerId, (u8, u8))> = gone
            .iter()
            .filter(|c| after.swept_home(c.id).is_none())
            .filter_map(|c| castle_entered(after, c).map(|(owner, at)| (*c, owner, at)))
            .collect();
        if walking.len() > walked_in {
            walking.sort_by_key(|(c, ..)| std::cmp::Reverse(c.progress));
            walking.truncate(walked_in);
        }
        // What the banks alone would leave each score at, credited in the
        // order the tick credits them: the walkers as they arrive, then
        // the tide's sweep.
        let mut expected = self.scores;
        let mut credit = |owner: PlayerId, crab: &Seen| {
            if let Some(score) = expected.get_mut(usize::from(owner)) {
                *score = credited(*score, crab, after.crab_value(crab.kind), self.claw_call);
            }
        };
        let mut reports: Vec<(u32, Happened)> = Vec::new();
        for crab in &gone {
            let (x, y) = after.coords_u8(crab.tile);
            if let Some(&(_, owner, (x, y))) = walking.iter().find(|(c, ..)| c.id == crab.id) {
                credit(owner, crab);
                reports.push((
                    crab.id,
                    Happened::Banked {
                        crab: crab.id,
                        owner,
                        x,
                        y,
                    },
                ));
            } else if after.swept_home(crab.id).is_none() {
                reports.push((
                    crab.id,
                    Happened::Eaten {
                        crab: crab.id,
                        x,
                        y,
                    },
                ));
            }
        }
        for crab in &gone {
            if let Some(owner) = after.swept_home(crab.id) {
                credit(owner, crab);
                let (x, y) = after
                    .castle_of(owner)
                    .unwrap_or_else(|| after.coords_u8(crab.tile));
                reports.push((
                    crab.id,
                    Happened::Banked {
                        crab: crab.id,
                        owner,
                        x,
                        y,
                    },
                ));
            }
        }
        reports.sort_by_key(|(id, _)| *id);
        out.extend(reports.into_iter().map(|(_, what)| what));
        // Whatever the banks do not account for, a gull carried off.
        for (seat, (&was, &now)) in expected.iter().zip(after.scores()).enumerate() {
            if now < was {
                out.push(Happened::Raided {
                    owner: seat as PlayerId,
                    lost: was - now,
                });
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

/// The castle a crab that vanished walked into, and where: the tile it
/// stood on, or the one it was heading into (across the seam on a wrapping
/// beach).
fn castle_entered(after: &Board, crab: &Seen) -> Option<(PlayerId, (u8, u8))> {
    [Some(crab.tile), after.step(crab.tile, crab.dir)]
        .into_iter()
        .flatten()
        .find_map(|tile| {
            let (x, y) = after.coords_u8(tile);
            match after.tile_at(x, y) {
                TileKind::Castle(owner) => Some((owner, (x, y))),
                TileKind::Empty
                | TileKind::Rock
                | TileKind::Spawner(_)
                | TileKind::Turnstile { .. }
                | TileKind::Kelp
                | TileKind::Pool => None,
            }
        })
}

/// A score after banking `crab`, worth `value` on this board, into it, by
/// the sim's own rule: Right Claws doubles a right-clawed crab and charges
/// for a left-clawed one, down to nothing.
fn credited(score: u32, crab: &Seen, value: u32, claw_call: bool) -> u32 {
    match (claw_call, crab.handed) {
        (false, _) => score + value,
        (true, Handedness::Right) => score + value * 2,
        (true, Handedness::Left) => score.saturating_sub(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn run(board: &mut Board, ticks: u32, seen: &mut Vec<Happened>) {
        for _ in 0..ticks {
            let before = Before::of(board);
            board.tick_idle();
            before.diff(board, seen);
        }
    }

    /// A crab eaten beside a castle on the tick another walks into it is
    /// eaten, not banked: standing next to a castle whose score went up is
    /// not walking into it.
    #[test]
    fn a_crab_eaten_beside_a_castle_that_banked_is_eaten() {
        let mut board = Board::new(6, 4, 1);
        board.set_tile(4, 0, TileKind::Castle(1));
        board.spawn_crab(3, 0, Direction::Right, Handedness::Left, CrabKind::Common);
        let mut seen = Vec::new();
        run(&mut board, 11, &mut seen);
        board.spawn_crab(4, 1, Direction::Down, Handedness::Left, CrabKind::Common);
        board.spawn_gull(4, 2, Direction::Up);
        run(&mut board, 40, &mut seen);
        let banked = seen
            .iter()
            .filter(|h| matches!(h, Happened::Banked { .. }))
            .count();
        let eaten = seen
            .iter()
            .filter(|h| matches!(h, Happened::Eaten { .. }))
            .count();
        assert_eq!((banked, eaten), (1, 1), "{seen:?}");
    }

    /// Right Claws turns a left-clawed bank into a cost, which is no raid;
    /// but a gull raiding a castle while it runs is still a raid.
    #[test]
    fn a_raid_during_right_claws_is_still_a_raid() {
        let mut board = Board::new(6, 1, 1);
        board.set_tile(0, 0, TileKind::Castle(0));
        board.set_tile(5, 0, TileKind::Castle(1));
        board.set_score(1, 20);
        board.spawn_gull(3, 0, Direction::Right);
        let text = format!("{}claw_call: 200\n", board.to_snapshot());
        let mut board = Board::parse_snapshot(&text).expect("a board with Right Claws running");
        assert!(board.in_claw_call());
        let mut seen = Vec::new();
        run(&mut board, 100, &mut seen);
        assert!(
            seen.iter()
                .any(|h| matches!(h, Happened::Raided { owner: 1, .. })),
            "{seen:?}"
        );
    }

    /// A left-clawed crab banked at nothing during Right Claws leaves the
    /// score where it was, and is still a bank.
    #[test]
    fn a_bank_that_scores_nothing_is_still_a_bank() {
        let mut board = Board::new(5, 1, 1);
        board.set_tile(4, 0, TileKind::Castle(1));
        board.spawn_crab(2, 0, Direction::Right, Handedness::Left, CrabKind::Common);
        let text = format!("{}claw_call: 200\n", board.to_snapshot());
        let mut board = Board::parse_snapshot(&text).expect("a board with Right Claws running");
        let mut seen = Vec::new();
        run(&mut board, 100, &mut seen);
        assert_eq!(board.scores()[1], 0);
        assert!(
            matches!(seen.as_slice(), [Happened::Banked { owner: 1, .. }]),
            "{seen:?}"
        );
    }
}
