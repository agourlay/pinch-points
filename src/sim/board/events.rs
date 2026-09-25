//! Tide events: the sparkling crab's roulette, and the event and mania
//! types it draws from.

use super::*;

/// The tide roulette's state: whether it spins on this board, how long
/// until it may spin again, and what it has set running. One struct
/// because every field is the roulette's alone, read by the event code and
/// the spawners and nothing else.
#[derive(Clone, Debug, Default)]
pub(crate) struct Tide {
    /// Tide events fire only where enabled (versus arenas, the attract
    /// beach), never in puzzles or goal-checked challenges.
    pub(crate) enabled: bool,
    /// Ticks until the roulette may spin again; set when any event fires.
    /// See [`EVENT_COOLDOWN`].
    pub(crate) cooldown: u32,
    /// Active spawn mania: spawners flood crabs or emit gulls instead.
    pub(crate) mania: Option<(Mania, u32)>,
    /// Active tempo shift and the ticks left of it.
    pub(crate) tempo: Option<(Tempo, u32)>,
    /// Ticks left of a Right Claws event, zero when none is running.
    ///
    /// A bare count rather than an `Option`, because unlike a mania or a
    /// tempo shift there is nothing to say but how long is left: the claw
    /// it favours never changes.
    pub(crate) claw_call: u32,
    /// The most recent tide event and the tick it fired (HUD banner).
    pub(crate) last: Option<(TideEvent, u64)>,
    /// Sparkling banks noticed during crab movement; the roulette spins
    /// after the movement pass so events may safely mutate the crab list.
    /// Always drained within the same tick (never hashed).
    pub(crate) queue: Vec<PlayerId>,
}

impl Tide {
    /// [`Board::copy_from`]'s share: every field, the queue's allocation
    /// kept.
    pub(crate) fn copy_from(&mut self, other: &Self) {
        let Self {
            enabled,
            cooldown,
            mania,
            tempo,
            claw_call,
            last,
            queue,
        } = other;
        self.enabled = *enabled;
        self.cooldown = *cooldown;
        self.mania = *mania;
        self.tempo = *tempo;
        self.claw_call = *claw_call;
        self.last = *last;
        self.queue.clear();
        self.queue.extend_from_slice(queue);
    }
}

/// Tempo shifts (tide events): the beach's clock run faster or slower.
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
    /// The tide calls a claw: for a while, a right-clawed crab banks for
    /// double and a left-clawed one costs what it was worth.
    ///
    /// Always the right claw, never drawn: the rule falls on the whole
    /// beach at once, so a fixed one is as fair as a coin and is learned
    /// once instead of read off the banner every time.
    RightClaws,
}

impl TideEvent {
    /// Every event, in the order the roulette and the string tables use.
    pub const ALL: [TideEvent; 9] = [
        TideEvent::CrabMania,
        TideEvent::GullMania,
        TideEvent::Monopoly,
        TideEvent::GullAttack,
        TideEvent::SpeedUp,
        TideEvent::SlowDown,
        TideEvent::FreshSand,
        TideEvent::CastleSwap,
        // Appended, never inserted: the index is the string table's row and
        // the bit in the seen-it mask, so an older save keeps its meaning.
        TideEvent::RightClaws,
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
        if !self.tide.enabled {
            return;
        }
        // Not while the last event is still running. The wheel is spun by
        // banking a Sparkling crab, and several of the faces it lands on
        // put more crabs on the beach, so without this the events feed
        // themselves: three Crab Manias inside fourteen seconds, measured.
        // The crab still banks and still scores.
        if self.tide.cooldown > 0 {
            return;
        }
        // One draw indexes ALL, so the roulette's order *is* ALL's order:
        // the same one the string tables and the snapshot use.
        let event = TideEvent::ALL[(self.rng.next_u32() % TideEvent::ALL.len() as u32) as usize];
        self.apply_tide_event(self.surge_safe(event), banker);
    }

    /// The surge already doubles the flock, so the roulette keeps off the
    /// gull events for the last 30 seconds: Gull Mania on top of the surge
    /// left fifteen gulls and almost no crabs with half a minute to play.
    /// Swapped rather than re-rolled, so the draw count stays fixed.
    pub(super) fn surge_safe(&self, event: TideEvent) -> TideEvent {
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
            | TideEvent::CastleSwap
            // Kept through the surge: it puts nothing on the beach, and a
            // scramble over which claw is worth banking is exactly what the
            // last thirty seconds are for.
            | TideEvent::RightClaws) => kept,
        }
    }

    /// Start a lure for `owner`, as banking a molting crab does.
    ///
    /// Same reason as [`Self::force_tide_event`]: the dev hook and the
    /// tests need one on demand, and a screenshot cannot wait for a molt to
    /// turn up and be banked.
    pub fn force_lure(&mut self, owner: PlayerId) {
        self.lure = Some((owner, LURE_TICKS));
    }

    /// Fire a named tide event outright. The roulette is the only caller
    /// in play; this exists for the dev hook that has to show one on
    /// demand, and for the tests, which cannot wait for a sparkling crab.
    pub fn force_tide_event(&mut self, event: TideEvent, banker: PlayerId) {
        self.apply_tide_event(event, banker);
    }

    /// Apply one tide event's effects (split from the roulette so each event
    /// is unit-testable in isolation).
    pub(super) fn apply_tide_event(&mut self, event: TideEvent, banker: PlayerId) {
        self.tide.last = Some((event, self.tick));
        // Set here rather than in the roulette so a forced event starts
        // the clock too: the point is "an event is running", not "the
        // wheel was spun".
        self.tide.cooldown = EVENT_COOLDOWN;
        match event {
            TideEvent::CrabMania => {
                self.gulls.clear();
                self.tide.mania = Some((Mania::Crab, EVENT_TICKS));
            }
            TideEvent::GullMania => {
                self.tide.mania = Some((Mania::Gull, EVENT_TICKS));
            }
            TideEvent::Monopoly => {
                // Half the loose crabs (front of the line) scuttle straight
                // into the banker's castle.
                let take = self.crabs.len() / 2;
                let at = self.tick;
                // Drained first: `credit_bank` takes `&mut self`, and the
                // sweep is a fixed-order loop over a list that is being
                // emptied anyway.
                let swept: Vec<Crab> = self.crabs.drain(..take).collect();
                for crab in swept {
                    self.credit_bank(banker, &crab);
                    self.crabs_banked += 1;
                    if crab.kind == CrabKind::Golden {
                        self.golden_banked += 1;
                    }
                    // Written down because it cannot be worked out later:
                    // the crab is gone from a tile that is not a castle,
                    // which from the outside is exactly what being eaten
                    // looks like. See `Board::swept_home`.
                    self.swept_home.push(Swept {
                        crab: crab.id,
                        owner: banker,
                        at,
                    });
                }
            }
            TideEvent::GullAttack => {
                // A gull lands beside every rival castle, facing it.
                let targets: Vec<(u16, PlayerId)> = self
                    .grid
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
            TideEvent::RightClaws => self.tide.claw_call = EVENT_TICKS,
            TideEvent::SpeedUp => self.tide.tempo = Some((Tempo::Fast, EVENT_TICKS)),
            TideEvent::SlowDown => self.tide.tempo = Some((Tempo::Slow, EVENT_TICKS)),
            TideEvent::FreshSand => self.signposts.fill(None),
            TideEvent::CastleSwap => {
                // Rockets swap places: every castle passes to the next
                // participating owner, in a fixed rotation.
                let mut owners: Vec<PlayerId> = Vec::new();
                for tile in &self.grid.tiles {
                    if let TileKind::Castle(owner) = tile
                        && !owners.contains(owner)
                    {
                        owners.push(*owner);
                    }
                }
                if owners.len() > 1 {
                    for tile in &mut self.grid.tiles {
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
