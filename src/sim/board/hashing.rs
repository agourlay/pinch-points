//! The determinism fingerprint (spec §7.5).

use super::*;

impl Board {
    /// Fingerprint of the complete simulation state, in a fixed field order.
    /// Two boards fed the same seed and inputs must agree on this after every
    /// tick, on every platform: the determinism contract of spec §7.5.
    pub fn state_hash(&self) -> u64 {
        // A census of every field, exhaustive on purpose and with no rest
        // pattern, so that adding one to `Board` stops compiling until it
        // is either hashed below or named here as deliberately left out. A
        // field that silently escapes the fingerprint is a desync no peer
        // can see: both sides play differently and both report agreement.
        // `lure_cooldown` escaped exactly this way, and only the two names
        // under "outside" below have any business doing so.
        let Self {
            // hash_terrain
            grid:
                Grid {
                    width: _,
                    height: _,
                    h_walls: _,
                    v_walls: _,
                    tiles: _,
                },
            signposts: _,
            // hash_creatures
            crabs: _,
            scores: _,
            rng: _,
            gulls: _,
            next_gull_id: _,
            // hash_round
            tick: _,
            signpost_seq: _,
            next_crab_id: _,
            rules:
                Rules {
                    signpost_cap: _,
                    cap_policy: _,
                    gull_period: _,
                    round_length: _,
                    castle_raids: _,
                },
            lure: _,
            lure_cooldown: _,
            event_cooldown: _,
            crabs_banked: _,
            golden_banked: _,
            events_enabled: _,
            mania: _,
            tempo: _,
            last_event: _,
            wrap: _,
            // Outside the fingerprint, each for a reason of its own: the
            // construction seed is dead once the PRNG state (which *is*
            // hashed) has been derived from it, and the event queue is
            // filled and drained inside a single tick, so it is always
            // empty by the time anyone hashes.
            seed: _,
            event_queue: _,
        } = self;
        let mut h = Fnv::new();
        self.hash_terrain(&mut h);
        self.hash_creatures(&mut h);
        self.hash_round(&mut h);
        h.finish()
    }

    /// The board itself: size, walls, tiles, signposts. Field order is part
    /// of the contract: never reorder these, only append.
    fn hash_terrain(&self, h: &mut Fnv) {
        h.u8(self.grid.width);
        h.u8(self.grid.height);
        for &wall in &self.grid.h_walls {
            h.bool(wall);
        }
        for &wall in &self.grid.v_walls {
            h.bool(wall);
        }
        for tile in &self.grid.tiles {
            match *tile {
                TileKind::Empty => h.u8(0),
                TileKind::Rock => h.u8(1),
                TileKind::Castle(owner) => {
                    h.u8(2);
                    h.u8(owner);
                }
                TileKind::Spawner(s) => {
                    h.u8(3);
                    h.u8(s.dir.id());
                    h.u32(s.period);
                }
                TileKind::Turnstile { next_right } => {
                    h.u8(4);
                    h.bool(next_right);
                }
                TileKind::Kelp => h.u8(5),
                TileKind::Pool => h.u8(6),
            }
        }
        for slot in &self.signposts {
            match slot {
                None => h.u8(0),
                Some(sp) => {
                    h.u8(1);
                    h.u8(sp.dir.id());
                    h.u8(sp.owner);
                    h.u8(match sp.health {
                        SignpostHealth::Full => 0,
                        SignpostHealth::Worn => 1,
                    });
                    h.u64(sp.seq);
                    h.u64(sp.placed);
                }
            }
        }
    }

    /// Everything alive, the scores they are chasing, and the PRNG that
    /// drives them.
    fn hash_creatures(&self, h: &mut Fnv) {
        for crab in &self.crabs {
            h.u32(crab.id);
            h.u16(crab.tile);
            h.u8(crab.dir.id());
            h.u16(crab.progress);
            h.u16(crab.prev.tile);
            h.u16(crab.prev.progress);
            h.u8(crab.prev.dir.id());
            h.u8(crab.handed.id());
            h.u8(crab.kind.id());
        }
        for &score in &self.scores {
            h.u32(score);
        }
        let (state, inc) = self.rng.hash_state();
        h.u64(state);
        h.u64(inc);
        for gull in &self.gulls {
            h.u32(gull.id);
            h.u16(gull.tile);
            h.u8(gull.dir.id());
            h.u16(gull.progress);
            h.u16(gull.prev.tile);
            h.u16(gull.prev.progress);
            h.u8(gull.prev.dir.id());
            h.u8(gull.handed.id());
            match gull.state {
                GullState::Walking => h.u8(0),
                GullState::Flying { remaining } => {
                    h.u8(1);
                    h.u8(remaining);
                }
            }
            h.u32(gull.takeoff_in);
        }
        h.u32(self.next_gull_id);
    }

    /// The round's own state: the tide, the active tide effects, and the
    /// counters that outlive a single creature.
    fn hash_round(&self, h: &mut Fnv) {
        h.u32(self.rules.gull_period);
        match self.rules.round_length {
            None => h.u8(0),
            Some(len) => {
                h.u8(1);
                h.u32(len);
            }
        }
        match self.lure {
            None => h.u8(0),
            Some((p, t)) => {
                h.u8(1);
                h.u8(p);
                h.u32(t);
            }
        }
        h.u32(self.golden_banked);
        h.bool(self.rules.castle_raids);
        h.bool(self.events_enabled);
        h.bool(self.wrap);
        match self.mania {
            None => h.u8(0),
            Some((Mania::Crab, t)) => {
                h.u8(1);
                h.u32(t);
            }
            Some((Mania::Gull, t)) => {
                h.u8(2);
                h.u32(t);
            }
        }
        match self.tempo {
            None => h.u8(0),
            Some((tempo, t)) => {
                h.u8(match tempo {
                    Tempo::Fast => 1,
                    Tempo::Slow => 2,
                });
                h.u32(t);
            }
        }
        match self.last_event {
            None => h.u8(0),
            Some((event, tick)) => {
                // ALL's position, not the declaration discriminant: every
                // stable meaning of an event (roulette, snapshot, strings)
                // reads off ALL, and the hash follows the same order.
                h.u8(1 + event.index() as u8);
                h.u64(tick);
            }
        }
        h.u64(self.tick);
        h.u64(self.signpost_seq);
        h.u32(self.next_crab_id);
        h.u32(self.crabs_banked);
        h.u8(self.rules.signpost_cap);
        h.u8(match self.rules.cap_policy {
            CapPolicy::Evict => 0,
            CapPolicy::Reject => 1,
        });
        // The quiet spell after a lure. Appended here rather than slotted
        // in beside `lure`, where it belongs by meaning, because field
        // order is the format and only the tail is safe to grow.
        h.u32(self.lure_cooldown);
        h.u32(self.event_cooldown);
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::{Board, CapPolicy, CrabKind, Direction, Handedness, Spawner, TileKind};

    /// Every externally-reachable mutator must move the hash: a field that
    /// escapes `state_hash` is a silent online-desync blind spot.
    #[test]
    fn every_mutator_changes_the_hash() {
        let base = || Board::new(8, 6, 42);
        type Mutation = (&'static str, Box<dyn Fn(&mut Board)>);
        let mutations: Vec<Mutation> = vec![
            ("set_tile", Box::new(|b| b.set_tile(2, 2, TileKind::Rock))),
            (
                "set_spawner",
                Box::new(|b| {
                    b.set_tile(
                        3,
                        3,
                        TileKind::Spawner(Spawner {
                            dir: Direction::Right,
                            period: 40,
                        }),
                    );
                }),
            ),
            (
                "set_wall",
                Box::new(|b| b.set_wall(1, 1, Direction::Up, true)),
            ),
            ("set_wrap", Box::new(|b| b.set_wrap(true))),
            ("set_score", Box::new(|b| b.set_score(0, 7))),
            ("set_gull_period", Box::new(|b| b.set_gull_period(99))),
            (
                "set_round_length",
                Box::new(|b| b.set_round_length(Some(500))),
            ),
            (
                "set_events_enabled",
                Box::new(|b| b.set_events_enabled(true)),
            ),
            ("set_castle_raids", Box::new(|b| b.set_castle_raids(false))),
            (
                "set_signpost_rule",
                Box::new(|b| b.set_signpost_rule(5, CapPolicy::Reject)),
            ),
            (
                "place_signpost",
                Box::new(|b| {
                    b.place_signpost(0, 4, 4, Direction::Left);
                }),
            ),
            (
                "spawn_crab",
                Box::new(|b| {
                    b.spawn_crab(2, 3, Direction::Up, Handedness::Left, CrabKind::Common);
                }),
            ),
            (
                "spawn_gull",
                Box::new(|b| b.spawn_gull(5, 2, Direction::Down)),
            ),
            ("tick_idle", Box::new(Board::tick_idle)),
        ];
        let clean = base().state_hash();
        for (name, mutate) in mutations {
            let mut board = base();
            mutate(&mut board);
            assert_ne!(board.state_hash(), clean, "{name} did not move state_hash");
        }
    }

    /// State reachable only by ticking, which the mutator census above
    /// cannot see. The lure cooldown decides whether banking a molt starts
    /// a lure at all, and while it went unhashed two boards could sit at
    /// the same fingerprint and then play the round differently: the one
    /// failure the hash exists to catch.
    #[test]
    fn the_lure_cooldown_is_part_of_the_fingerprint() {
        use crate::sim::{MAX_PLAYERS, PlayerAction};
        let armed = |cooldown: u32| {
            let mut board = Board::new(6, 5, 1);
            board.set_tile(3, 2, TileKind::Castle(0));
            board.lure_cooldown = cooldown;
            board.spawn_crab(2, 2, Direction::Right, Handedness::Right, CrabKind::Molting);
            board
        };
        let (mut quiet, mut ready) = (armed(200), armed(0));
        assert_ne!(
            quiet.state_hash(),
            ready.state_hash(),
            "a cooldown that changes the round has to change the hash"
        );
        for board in [&mut quiet, &mut ready] {
            for _ in 0..40 {
                board.tick(&[PlayerAction::None; MAX_PLAYERS]);
            }
        }
        assert!(quiet.lure().is_none(), "the quiet spell swallows the lure");
        assert!(ready.lure().is_some(), "a clear board starts one");
    }

    /// The same trap, one field along. `event_cooldown` decides whether
    /// banking a Sparkling crab spins the roulette at all, so two boards
    /// holding different ones play the round apart - and the census above
    /// only forces a new field to be *named*, not hashed, which is exactly
    /// how `lure_cooldown` slipped through in the first place. Removing the
    /// `h.u32` for this field passes every other test in the suite.
    #[test]
    fn the_event_cooldown_is_part_of_the_fingerprint() {
        use crate::sim::{MAX_PLAYERS, PlayerAction};
        let armed = |cooldown: u32| {
            let mut board = Board::new(6, 5, 1);
            board.set_events_enabled(true);
            board.set_tile(3, 2, TileKind::Castle(0));
            board.event_cooldown = cooldown;
            board.spawn_crab(
                2,
                2,
                Direction::Right,
                Handedness::Right,
                CrabKind::Sparkling,
            );
            board
        };
        let (mut quiet, mut ready) = (armed(200), armed(0));
        assert_ne!(
            quiet.state_hash(),
            ready.state_hash(),
            "a cooldown that changes the round has to change the hash"
        );
        for board in [&mut quiet, &mut ready] {
            for _ in 0..40 {
                board.tick(&[PlayerAction::None; MAX_PLAYERS]);
            }
        }
        assert!(
            quiet.last_event().is_none(),
            "the cooldown swallows the spin"
        );
        assert!(
            ready.last_event().is_some(),
            "a clear board spins the wheel"
        );
    }
}
