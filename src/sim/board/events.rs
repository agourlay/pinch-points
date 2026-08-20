//! Tide events: the sparkling crab's roulette, and the event and mania
//! types it draws from.

use super::*;

/// Spawn-mania flavours (tide events).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tempo {
    /// Everything on the beach moves at double speed.
    Fast,
    /// And at half.
    Slow,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mania {
    Crab,
    Gull,
}

/// The sparkling crab's roulette (the original's "?"-mouse events, re-themed
/// for the beach).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TideEvent {
    /// Gulls washed away; spawners flood crabs for a while.
    CrabMania,
    /// Spawners emit gulls for a while.
    GullMania,
    /// Half the loose crabs scuttle straight into the banker's castle.
    Monopoly,
    /// A gull lands beside every rival castle.
    GullAttack,
    SpeedUp,
    SlowDown,
    /// Every signpost on the beach washes away.
    FreshSand,
    /// Castles trade owners (rockets swap places!).
    CastleSwap,
}

impl TideEvent {
    /// Every event, in the order the roulette and the string tables use.
    pub const ALL: [TideEvent; 8] = [
        TideEvent::CrabMania,
        TideEvent::GullMania,
        TideEvent::Monopoly,
        TideEvent::GullAttack,
        TideEvent::SpeedUp,
        TideEvent::SlowDown,
        TideEvent::FreshSand,
        TideEvent::CastleSwap,
    ];

    /// Position in [`TideEvent::ALL`]: the index of this event's name in the
    /// string tables, and the bit that records having seen it.
    pub fn index(self) -> usize {
        TideEvent::ALL
            .iter()
            .position(|&event| event == self)
            .expect("every event is in ALL")
    }
}

impl Board {
    /// The sparkling crab's roulette (spec: the original's "?"-mouse random
    /// events, re-themed). Deterministic: one PRNG draw picks the event, and
    /// every effect operates in fixed order.
    pub(super) fn spin_tide_event(&mut self, banker: PlayerId) {
        if !self.events_enabled {
            return;
        }
        // One draw indexes ALL, so the roulette's order *is* ALL's order:
        // the same one the string tables and the snapshot use.
        let event = TideEvent::ALL[(self.rng.next_u32() % TideEvent::ALL.len() as u32) as usize];
        self.apply_tide_event(self.surge_safe(event), banker);
    }

    /// The surge already doubles the flock, so the roulette keeps off the
    /// gull events for the last 30 seconds. Observed once live: Gull Mania
    /// on top of the surge left fifteen gulls and almost no crabs with half
    /// a minute to play, which is the tensest stretch of a round with
    /// nothing left to route. Swapped rather than re-rolled, so the draw
    /// count stays fixed.
    fn surge_safe(&self, event: TideEvent) -> TideEvent {
        if !self.in_surge() {
            return event;
        }
        match event {
            TideEvent::GullMania => TideEvent::CrabMania,
            TideEvent::GullAttack => TideEvent::SpeedUp,
            kept @ (TideEvent::CrabMania
            | TideEvent::Monopoly
            | TideEvent::SpeedUp
            | TideEvent::SlowDown
            | TideEvent::FreshSand
            | TideEvent::CastleSwap) => kept,
        }
    }

    /// Apply one tide event's effects (split from the roulette so each event
    /// is unit-testable in isolation).
    /// Start a lure for `owner`, as banking a molting crab does.
    ///
    /// Same reason as [`Self::force_tide_event`]: the dev hook and the
    /// tests need one on demand, and waiting for a molt to turn up and be
    /// banked is not a thing a screenshot can do.
    pub fn force_lure(&mut self, owner: PlayerId) {
        self.lure = Some((owner, crate::sim::LURE_TICKS));
    }

    /// Fire a named tide event outright. The roulette is the only caller
    /// in play; this exists for the dev hook that has to show one on
    /// demand, and for the tests, which cannot wait for a sparkling crab.
    pub fn force_tide_event(&mut self, event: TideEvent, banker: PlayerId) {
        self.apply_tide_event(event, banker);
    }

    pub(super) fn apply_tide_event(&mut self, event: TideEvent, banker: PlayerId) {
        self.last_event = Some((event, self.tick));
        match event {
            TideEvent::CrabMania => {
                self.gulls.clear();
                self.mania = Some((Mania::Crab, EVENT_TICKS));
            }
            TideEvent::GullMania => {
                self.mania = Some((Mania::Gull, EVENT_TICKS));
            }
            TideEvent::Monopoly => {
                // Half the loose crabs (front of the line) scuttle straight
                // into the banker's castle.
                let take = self.crabs.len() / 2;
                for crab in self.crabs.drain(..take) {
                    self.scores[banker as usize] += crab.kind.value();
                    self.crabs_banked += 1;
                    if crab.kind == CrabKind::Golden {
                        self.golden_banked += 1;
                    }
                }
            }
            TideEvent::GullAttack => {
                // A gull lands beside every rival castle, facing it.
                let targets: Vec<(u16, PlayerId)> = self
                    .tiles
                    .iter()
                    .enumerate()
                    .filter_map(|(t, tile)| match tile {
                        TileKind::Castle(owner) if *owner != banker => Some((t as u16, *owner)),
                        TileKind::Castle(_)
                        | TileKind::Empty
                        | TileKind::Rock
                        | TileKind::Spawner(_)
                        | TileKind::Turnstile { .. }
                        | TileKind::Kelp
                        | TileKind::Pool => None,
                    })
                    .collect();
                for (castle, _) in targets {
                    let (cx, cy) = self.coords(castle);
                    // The first open edge-adjacent spot; the gull faces
                    // back toward the castle it besieges.
                    let ring = self.ring_openings(cx, cy, &CASTLE_RING[..4]);
                    if let Some(&(nx, ny, ox, oy)) = ring.first() {
                        let dir = Direction::toward(ox, oy).reverse();
                        self.spawn_gull(nx as u8, ny as u8, dir);
                    }
                }
            }
            TideEvent::SpeedUp => self.tempo = Some((Tempo::Fast, EVENT_TICKS)),
            TideEvent::SlowDown => self.tempo = Some((Tempo::Slow, EVENT_TICKS)),
            TideEvent::FreshSand => self.signposts.fill(None),
            TideEvent::CastleSwap => {
                // Rockets swap places: every castle passes to the next
                // participating owner, in a fixed rotation.
                let mut owners: Vec<PlayerId> = Vec::new();
                for tile in &self.tiles {
                    if let TileKind::Castle(owner) = tile
                        && !owners.contains(owner)
                    {
                        owners.push(*owner);
                    }
                }
                if owners.len() > 1 {
                    for tile in &mut self.tiles {
                        if let TileKind::Castle(owner) = tile {
                            let at = owners.iter().position(|o| o == owner).unwrap_or(0);
                            *owner = owners[(at + 1) % owners.len()];
                        }
                    }
                }
            }
        }
    }
}
