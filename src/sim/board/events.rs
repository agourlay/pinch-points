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
    /// beach), never in puzzles.
    pub(crate) enabled: bool,
    /// Ticks until the roulette may spin again; set when an event fires,
    /// or under [`Rules::timed_lock`] only when one that lasts does. See
    /// [`EVENT_COOLDOWN`].
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
    /// Whose sparkling crab spun the wheel for `last`, or `None` when it
    /// was called rather than spun (a spectators' vote, a dev hook). For
    /// the trophies and nothing in the rules, so never hashed or saved.
    pub(crate) spun_by: Option<PlayerId>,
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
            spun_by,
            queue,
        } = other;
        self.enabled = *enabled;
        self.cooldown = *cooldown;
        self.mania = *mania;
        self.tempo = *tempo;
        self.claw_call = *claw_call;
        self.last = *last;
        self.spun_by = *spun_by;
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
    /// A gull lands beside every rival castle: every one but the
    /// banker's, and its partners' in a round played in teams.
    GullAttack,
    /// Every crab and gull moves at double speed, and the holes open
    /// twice as often.
    SpeedUp,
    SlowDown,
    /// Every signpost on the beach washes away.
    FreshSand,
    /// Castles trade owners (rockets swap places!). When three seats or
    /// more hold castles, the spinner's own castles sit the trade out; the
    /// count is of seats, so two seats with two castles each rotate both.
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

    /// Whether the event runs on for [`EVENT_TICKS`] rather than being over
    /// in the tick it fires: the ones a [`Rules::timed_lock`] wheel waits
    /// out.
    pub fn lasts(self) -> bool {
        match self {
            TideEvent::CrabMania
            | TideEvent::GullMania
            | TideEvent::SpeedUp
            | TideEvent::SlowDown
            | TideEvent::RightClaws => true,
            TideEvent::Monopoly
            | TideEvent::GullAttack
            | TideEvent::FreshSand
            | TideEvent::CastleSwap => false,
        }
    }

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
        self.fire_tide_event(self.surge_safe(event), banker, true);
    }

    /// The surge already doubles the flock, so the roulette keeps off the
    /// gull events for the last 30 seconds: Gull Mania on top of the surge
    /// left fifteen gulls and almost no crabs with half a minute to play.
    /// Swapped rather than re-rolled, so the draw count stays fixed.
    pub fn surge_safe(&self, event: TideEvent) -> TideEvent {
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

    /// Every seat that owns a castle, once each, in reading order of
    /// their first one: the order Castle Swap rotates them in.
    fn swap_owners(&self) -> Vec<PlayerId> {
        let mut owners: Vec<PlayerId> = Vec::new();
        for owner in self.castle_owners() {
            if !owners.contains(&owner) {
                owners.push(owner);
            }
        }
        owners
    }

    /// The seat whose castle sits out a Castle Swap spun by `spinner`
    /// (`None` for a called one), if any: the spinner, when three seats or
    /// more hold castles. Seats and not castles: the rotation runs over
    /// owners ([`Self::swap_owners`]), and between two owners there is
    /// nobody else to trade with however many castles each has. The swap
    /// and the banner that explains it both read this.
    ///
    /// Swept along with the rest, the spinner's castle went to a rival
    /// with every arrow still pointing at it, which made the swap the one
    /// face of the wheel that cost the seat that spun it (2 to 6 points of
    /// win rate at three seats and up, measured); kept out, it is neutral.
    /// Two owners have nobody else to trade with, and a call has no
    /// spinner: both rotate everyone.
    pub fn castle_swap_keeper(&self, spinner: Option<PlayerId>) -> Option<PlayerId> {
        let spinner = spinner?;
        let owners = self.swap_owners();
        (owners.len() > 2 && owners.contains(&spinner)).then_some(spinner)
    }

    /// Fire a named tide event outright, as called rather than spun: the
    /// way a spectators' vote lands, with nobody's sparkling crab behind
    /// it. Also the dev hook's and the tests', which cannot wait for one.
    pub fn force_tide_event(&mut self, event: TideEvent, banker: PlayerId) {
        self.fire_tide_event(event, banker, false);
    }

    /// Fire a named tide event as if `spinner`'s sparkling crab had spun
    /// it, for the dev hook and the tests: the one way to reach a rule that
    /// depends on who spun (Castle Swap) without waiting for the wheel.
    pub fn force_spun_tide_event(&mut self, event: TideEvent, spinner: PlayerId) {
        self.fire_tide_event(event, spinner, true);
    }

    /// Apply one tide event's effects (split from the roulette so each event
    /// is unit-testable in isolation). `banker` is the seat the event
    /// favours; `spun` says the seat's own sparkling crab spun the wheel,
    /// as against a call, which only Castle Swap tells apart.
    fn fire_tide_event(&mut self, event: TideEvent, banker: PlayerId, spun: bool) {
        self.tide.last = Some((event, self.tick));
        self.tide.spun_by = spun.then_some(banker);
        // Set here rather than in the roulette so a forced event starts
        // the clock too: the point is "an event is running", not "the
        // wheel was spun". Under a timed lock an event that is already
        // over leaves the wheel as it found it, which may be still locked
        // by a timed one a vote or a dev hook landed on top of.
        if !self.rules.timed_lock || event.lasts() {
            self.tide.cooldown = EVENT_COOLDOWN;
        }
        match event {
            TideEvent::CrabMania => {
                // The sky is cleared with the sand: the birds still on
                // their way in are called off too.
                self.drop_gulls(|_| true);
                self.incoming.clear();
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
                // Drained first: `bank_swept` takes `&mut self`, and the
                // sweep is a fixed-order loop over a list that is being
                // emptied anyway.
                let swept: Vec<Crab> = self.crabs.drain(..take).collect();
                let claw_call = self.in_claw_call();
                if self.rules.sweep_flight {
                    // Carried home instead, front of the line first, each
                    // with the claw call it was swept under.
                    for (n, crab) in swept.into_iter().enumerate() {
                        let launch = at + SWEEP_HOLD + SWEEP_STAGGER * n as u64;
                        self.sweeps.launch(Sweeping {
                            crab,
                            owner: banker,
                            launch,
                            lands: launch + SWEEP_FLIGHT,
                            claw_call,
                        });
                    }
                    return;
                }
                for crab in swept {
                    self.bank_swept(banker, &crab, claw_call);
                }
            }
            TideEvent::GullAttack => {
                // A gull lands beside every rival castle, facing it: every
                // castle but the banker's, and in a round played in teams
                // ([`Rules::teams`]) but its partners' too, whose raids
                // came off the banker's own team total.
                let rules = self.rules;
                let targets: Vec<(u16, PlayerId)> = self
                    .grid
                    .tiles
                    .iter()
                    .enumerate()
                    .filter_map(|(t, tile)| match tile {
                        TileKind::Castle(owner) if !rules.allies(*owner, banker) => {
                            Some((t as u16, *owner))
                        }
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
                // participating owner, in a fixed rotation, bar the one
                // that sits it out.
                let keeper = self.castle_swap_keeper(spun.then_some(banker));
                let mut owners = self.swap_owners();
                owners.retain(|&owner| Some(owner) != keeper);
                if owners.len() > 1 {
                    for tile in &mut self.grid.tiles {
                        // A castle whose owner sat out keeps that owner.
                        if let TileKind::Castle(owner) = tile
                            && let Some(at) = owners.iter().position(|o| o == owner)
                        {
                            *owner = owners[(at + 1) % owners.len()];
                        }
                    }
                }
            }
        }
    }
}
