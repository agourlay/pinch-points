//! Gulls (spec §3.5): spawning, walking, flight, castle raids, and the
//! crab-eating collision pass.

use super::*;

/// Tiles (in either direction) a gull must be from every castle to be sent
/// off when the flock turns over.
const TURNOVER_CLEARANCE: i32 = 2;

impl Board {
    /// Whether a gull can be put down on `(x, y)`: sand, not rock and not
    /// kelp, which a gull's feet cannot reach through. The one rule every
    /// placer asks (`spawn_gull` asserts it) so a level file, a pasted
    /// code and the editor's brush cannot disagree with the sim about
    /// where a bird may stand.
    pub fn gull_may_stand(&self, x: u8, y: u8) -> bool {
        self.in_bounds(i32::from(x), i32::from(y))
            && !matches!(self.tile_at(x, y), TileKind::Rock | TileKind::Kelp)
    }

    /// Drop a gull onto the board, walking. Used by level setup and the
    /// periodic edge spawner.
    pub fn spawn_gull(&mut self, x: u8, y: u8, dir: Direction) {
        assert!(
            self.in_bounds(i32::from(x), i32::from(y)),
            "gull off the board"
        );
        assert!(self.gull_may_stand(x, y), "gull on a rock or in kelp");
        let tile = self.index(i32::from(x), i32::from(y));
        let id = self.next_gull_id;
        self.next_gull_id += 1;
        let handed = self.roll_handedness();
        let takeoff_in = self.roll_takeoff();
        let mut gull = Gull {
            id,
            tile,
            dir,
            progress: 0,
            prev: Pose {
                tile,
                dir,
                progress: 0,
            },
            handed,
            state: GullState::Walking,
            takeoff_in,
        };
        self.resolve_walls_for(tile, &mut gull.dir, handed, Walker::Gull);
        self.gulls.push(gull);
    }

    /// The ambient flock's edge spawner: every `gull_period` ticks (double
    /// rate during the final-scramble surge) it sends for a gull, which
    /// lands [`GULL_LEAD`] ticks later on a PRNG perimeter tile, facing
    /// into the board.
    ///
    /// The tile is picked when the gull is sent for, not when it lands, so
    /// the board can show where it comes down for the whole of its flight
    /// in (see [`IncomingGull`]). Everything else is decided at the pick
    /// too: the flock cap counts the gulls on their way as well as the ones
    /// on the sand, and a full flock turning over sends its leaver off as
    /// the newcomer is sent for. What is left to the landing is what the
    /// bird itself rolls, its claw and its first takeoff.
    ///
    /// The other ways a gull comes onto the beach land at once: Gull
    /// Mania's come up out of the crab holes and Gull Attack's are set
    /// down beside the castles it names, both announced by the event's own
    /// banner, and a level's gulls are on the sand when it starts.
    pub(super) fn run_gull_spawner(&mut self) {
        // First, whatever was sent for earlier: the arrivals keep their
        // tick whatever happens to the spawner, a lookahead that switched
        // it off included, since the bird is already in the air.
        self.land_incoming_gulls();
        if self.rules.gull_period == 0 {
            return;
        }
        let period = if self.in_surge() {
            (self.rules.gull_period / 2).max(1)
        } else {
            self.rules.gull_period
        };
        if !self.tick.is_multiple_of(u64::from(period)) {
            return;
        }
        // A gull that would land after the tide comes in is never sent
        // for: the round is frozen by then and it would hang in the sky.
        let at = self.tick + u64::from(GULL_LEAD);
        if self
            .rules
            .round_length
            .is_some_and(|len| at >= u64::from(len))
        {
            return;
        }
        // Balance: the ambient flock is capped so the late round stays
        // playable (raiders leaving keeps the population cycling). Tide
        // events (GullMania, GullAttack) deliberately bypass the cap.
        let full = self.gulls.len() + self.incoming.len() >= self.flock_cap();
        if full {
            // With turnover on, a full flock makes room rather than shutting
            // the spawner out: see `Rules::gull_turnover`. Under the old
            // order the room is made before the newcomer's tile is known.
            if !self.rules.gull_turnover || (!self.rules.whole_flock && !self.send_a_gull_off()) {
                return;
            }
        }
        let (w, h) = (u32::from(self.grid.width), u32::from(self.grid.height));
        let perimeter = if w > 1 && h > 1 {
            2 * w + 2 * h - 4
        } else {
            w * h
        };
        let k = self.rng.next_u32() % perimeter;
        let (x, y, dir) = if k < w {
            (k, 0, Direction::Down) // top edge
        } else if k < 2 * w {
            (k - w, h - 1, Direction::Up) // bottom edge
        } else if k < 2 * w + (h - 2) {
            (0, k - 2 * w + 1, Direction::Right) // left edge
        } else {
            (w - 1, k - 2 * w - (h - 2) + 1, Direction::Left) // right edge
        };
        if !self.gull_may_stand(x as u8, y as u8) {
            return; // unlucky roll; the flock circles and tries again later
        }
        // Room made only for a newcomer that can land, so an unlucky roll
        // leaves the flock as it was rather than a gull short: see
        // `Rules::whole_flock`.
        if full && self.rules.whole_flock && !self.send_a_gull_off() {
            return;
        }
        let tile = self.index(x as i32, y as i32);
        self.incoming.push(IncomingGull { at, tile, dir });
    }

    /// Land every gull due this tick, in the order they were sent for.
    ///
    /// A landing tile that can no longer take a gull (nothing in play turns
    /// sand to rock today, but a snapshot is text) loses its bird rather
    /// than tripping `spawn_gull`'s assertion.
    pub(super) fn land_incoming_gulls(&mut self) {
        while let Some(&next) = self.incoming.first()
            && next.at <= self.tick
        {
            self.incoming.remove(0);
            let (x, y) = self.coords_u8(next.tile);
            if self.gull_may_stand(x, y) {
                self.spawn_gull(x, y, next.dir);
            }
        }
    }

    /// How many ambient gulls the spawner allows right now: the board's
    /// cap, raised by half for the final rush where the flock turns over.
    fn flock_cap(&self) -> usize {
        let cap = usize::from(self.rules.gull_cap);
        match self.rules.gull_turnover && self.in_surge() {
            true => cap + cap / 2,
            false => cap,
        }
    }

    /// The oldest walking gull more than [`TURNOVER_CLEARANCE`] tiles from
    /// every castle leaves the beach. False when none qualifies, and the
    /// flock stays as it is.
    ///
    /// Oldest by id, which the board hands out in order. Walking, because
    /// a bird in the air is already going somewhere a player can see. Clear
    /// of the castles, because a gull somebody walked up to a rival's keep
    /// is that player's attack, and pulling it away a tile short of the
    /// raid is the game cheating them.
    ///
    /// Every castle, a seat's second one too, and measured the way the
    /// gull would walk it: across the seam on an open beach, where a bird
    /// two tiles from a keep through the edge is as close as any other.
    fn send_a_gull_off(&mut self) -> bool {
        let castles: Vec<u16> = (0..self.grid.tiles.len())
            .filter(|&t| matches!(self.grid.tiles[t], TileKind::Castle(_)))
            .map(|t| t as u16)
            .collect();
        let leaving = self
            .gulls
            .iter()
            .filter(|gull| matches!(gull.state, GullState::Walking))
            .filter(|gull| {
                castles.iter().all(|&castle| {
                    let (dx, dy) = self.displacement(gull.tile, castle);
                    dx.abs().max(dy.abs()) > TURNOVER_CLEARANCE
                })
            })
            .map(|gull| gull.id)
            .min();
        let Some(id) = leaving else {
            return false;
        };
        self.drop_gulls(|gull| gull.id == id);
        true
    }

    pub(super) fn roll_takeoff(&mut self) -> u32 {
        TAKEOFF_MIN + self.rng.next_u32() % (TAKEOFF_MAX - TAKEOFF_MIN + 1)
    }

    pub(super) fn move_gulls(&mut self) {
        let mut departed: Vec<u32> = Vec::new();
        for i in 0..self.gulls.len() {
            let mut gull = self.gulls[i];
            gull.prev = gull.pose();
            let raided = match gull.state {
                GullState::Walking => self.walk_gull(&mut gull),
                GullState::Flying { .. } => self.fly_gull(&mut gull),
            };
            if raided {
                // Balance: a successful raider hauls its loot back to the
                // flock and leaves the beach, so gull pressure regulates
                // itself instead of compounding all round.
                departed.push(gull.id);
            } else {
                self.gulls[i] = gull;
            }
        }
        if !departed.is_empty() {
            self.drop_gulls(|gull| departed.contains(&gull.id));
        }
    }

    /// Take every gull `leaves` picks off the beach, and its steerer with
    /// it. The one way a gull leaves the board (a raider flying home, the
    /// flock turning over, Crab Mania, the editor), so the steering list
    /// never names a gull that is gone: a snapshot carrying one would not
    /// parse back.
    pub(super) fn drop_gulls(&mut self, mut leaves: impl FnMut(&Gull) -> bool) {
        self.gulls.retain(|gull| !leaves(gull));
        self.steering.keep_only(&self.gulls);
    }

    /// Returns true if the gull raided a castle (it departs with the loot).
    fn walk_gull(&mut self, gull: &mut Gull) -> bool {
        // Takeoff timer runs only while walking (spec §3.5: per-gull timer).
        if gull.takeoff_in == 0 {
            let distance =
                FLIGHT_MIN + (self.rng.next_u32() % u32::from(FLIGHT_MAX - FLIGHT_MIN + 1)) as u8;
            gull.state = GullState::Flying {
                remaining: distance,
            };
            return self.fly_gull(gull);
        }
        gull.takeoff_in -= 1;
        if !self.passable_for(gull.tile, gull.dir, Walker::Gull) {
            return false; // sealed in; waits like a crab would
        }
        gull.progress += self.walk_step(gull.tile, GULL_WALK_SPEED);
        while gull.progress >= SUBUNITS_PER_TILE {
            gull.progress -= SUBUNITS_PER_TILE;
            gull.tile = self.neighbor(gull.tile, gull.dir);
            if self.gull_arrival(gull) {
                return true;
            }
        }
        false
    }

    /// Walking-gull arrival: raid a castle it reaches (returning true, and
    /// the raider then leaves the beach with its loot), obey and degrade any
    /// signpost it crosses, wall-resolve.
    fn gull_arrival(&mut self, gull: &mut Gull) -> bool {
        let t = gull.tile as usize;
        if let TileKind::Castle(owner) = self.grid.tiles[t] {
            if self.rules.castle_raids {
                let lost = self.damage_castle(owner, gull.tile);
                self.steering.credit(gull.id, owner, lost);
            }
            return true;
        }
        if self.turnstile_deflect(gull.tile, &mut gull.dir, gull.handed, Walker::Gull) {
            return false;
        }
        if let Some(mut sp) = self.signposts[t] {
            gull.dir = sp.dir;
            self.steering.steer(gull.id, sp.owner);
            self.signposts[t] = match sp.health {
                SignpostHealth::Full => {
                    sp.health = SignpostHealth::Worn;
                    Some(sp)
                }
                SignpostHealth::Worn => None,
            };
        }
        self.resolve_walls_for(gull.tile, &mut gull.dir, gull.handed, Walker::Gull);
        false
    }

    /// Flying: hop tile-by-tile in the current direction, ignoring walls and
    /// signposts, bouncing off the board edge, landing only on a tile a
    /// creature can stand on (spec §3.5). Returns true on a landing raid.
    fn fly_gull(&mut self, gull: &mut Gull) -> bool {
        let GullState::Flying { mut remaining } = gull.state else {
            return false;
        };
        let mut landed = false;
        gull.progress += self.tempo_speed(GULL_FLY_SPEED);
        while gull.progress >= SUBUNITS_PER_TILE {
            gull.progress -= SUBUNITS_PER_TILE;
            let (x, y) = self.coords(gull.tile);
            let (dx, dy) = gull.dir.offset();
            if !self.wrap && !self.in_bounds(x + dx, y + dy) {
                gull.dir = gull.dir.reverse();
                let (rx, ry) = gull.dir.offset();
                if !self.in_bounds(x + rx, y + ry) {
                    // A board one tile long on this axis: nowhere to fly.
                    // Land where we are.
                    gull.state = GullState::Walking;
                    gull.takeoff_in = self.roll_takeoff();
                    return false;
                }
            }
            gull.tile = self.neighbor(gull.tile, gull.dir);
            remaining = remaining.saturating_sub(1);
            if remaining == 0 {
                if matches!(
                    self.grid.tiles[gull.tile as usize],
                    TileKind::Rock | TileKind::Kelp
                ) {
                    remaining = 1; // glide one more tile to somewhere landable
                } else {
                    landed = true;
                    gull.state = GullState::Walking;
                    gull.takeoff_in = self.roll_takeoff();
                    // Landing is not an arrival: no signpost effect (§3.5),
                    // but never leave the gull facing a wall.
                    self.resolve_walls_for(gull.tile, &mut gull.dir, gull.handed, Walker::Gull);
                    // A gull that lands on a castle raids it on the spot and
                    // departs with the loot, like a walking raid.
                    if let TileKind::Castle(owner) = self.grid.tiles[gull.tile as usize] {
                        if self.rules.castle_raids {
                            let lost = self.damage_castle(owner, gull.tile);
                            self.steering.credit(gull.id, owner, lost);
                        }
                        return true;
                    }
                    break;
                }
            }
        }
        if !landed && let GullState::Flying { remaining: r } = &mut gull.state {
            *r = remaining;
        }
        false
    }

    /// Spec §3.4: a gull reaching a castle carries off a **third** of the
    /// banked crabs, rounding the loss up ([`raid_loss`]). A raid on a fat
    /// castle still hurts and reads from across the room, and the tier drop
    /// falls out of the score loss. At most [`SPILL_CAP`] of the lost crabs respawn
    /// as live crabs in the surrounding tiles; the rest are carried off by
    /// the flock.
    ///
    /// Returns the points taken, for the raid's credit.
    pub(super) fn damage_castle(&mut self, owner: PlayerId, castle_tile: u16) -> u32 {
        let score = self.scores[owner as usize];
        let lost = raid_loss(score);
        let spill = lost.min(SPILL_CAP);
        self.scores[owner as usize] = score - lost;

        let (cx, cy) = self.coords(castle_tile);
        for (nx, ny, ox, oy) in self
            .ring_openings(cx, cy, &CASTLE_RING)
            .into_iter()
            .take(spill as usize)
        {
            // Scatter away from the castle: dominant axis of the offset.
            let dir = Direction::toward(ox, oy);
            let handed = self.roll_handedness();
            self.spawn_crab(nx as u8, ny as u8, dir, handed, CrabKind::Common);
        }
        lost
    }

    /// Where a creature actually stands, in board subunits: the tile it is
    /// filed under, plus how far it has walked out of that tile's centre.
    ///
    /// Measured against the tile alone, a gull and a crab walk through each
    /// other: two creatures approaching head-on cross the gap between two
    /// tile centres while still filed under *different* tiles, and by the
    /// time they share one they have passed.
    fn sub_position(&self, tile: u16, dir: Direction, progress: u16) -> (i32, i32) {
        let (x, y) = self.coords(tile);
        let (dx, dy) = sub_offset(dir, progress);
        (
            x * i32::from(SUBUNITS_PER_TILE) + dx,
            y * i32::from(SUBUNITS_PER_TILE) + dy,
        )
    }

    /// Manhattan distance between two sub-positions, in subunits. On a
    /// wrapping board the short way round counts: two creatures meeting
    /// head-on through the seam are a tile apart on the sand and a whole
    /// board apart in raw coordinates.
    fn sub_distance(&self, a: (i32, i32), b: (i32, i32)) -> u32 {
        let (mut dx, mut dy) = ((a.0 - b.0).unsigned_abs(), (a.1 - b.1).unsigned_abs());
        if self.wrap {
            let sub = u32::from(SUBUNITS_PER_TILE);
            let (w, h) = (
                u32::from(self.width()) * sub,
                u32::from(self.height()) * sub,
            );
            dx %= w;
            dy %= h;
            dx = dx.min(w - dx);
            dy = dy.min(h - dy);
        }
        dx + dy
    }

    /// Fixed-order collision pass: each gull, in index order, eats every crab
    /// within [`EAT_RANGE`] subunits of it (Manhattan distance across the
    /// board, spec §4.3). Flying gulls eat nothing.
    pub(super) fn gulls_eat(&mut self) {
        for g in 0..self.gulls.len() {
            let gull = self.gulls[g];
            if gull.state != GullState::Walking {
                continue;
            }
            let at = self.sub_position(gull.tile, gull.dir, gull.progress);
            let mut c = 0;
            while c < self.crabs.len() {
                let crab = self.crabs[c];
                let crab_at = self.sub_position(crab.tile, crab.dir, crab.progress);
                if self.sub_distance(at, crab_at) <= u32::from(EAT_RANGE) {
                    self.crabs.remove(c);
                    continue;
                }
                c += 1;
            }
        }
    }
}
