//! Gulls (spec §3.5): spawning, walking, flight, castle raids, and the
//! crab-eating collision pass.

use super::*;

impl Board {
    /// Drop a gull onto the board, walking. Used by level setup and the
    /// periodic edge spawner.
    pub fn spawn_gull(&mut self, x: u8, y: u8, dir: Direction) {
        assert!(
            self.in_bounds(i32::from(x), i32::from(y)),
            "gull off the board"
        );
        let tile = self.index(i32::from(x), i32::from(y));
        assert!(
            !matches!(self.tiles[tile as usize], TileKind::Rock | TileKind::Kelp),
            "gull on a rock or in kelp"
        );
        let id = self.next_gull_id;
        self.next_gull_id += 1;
        let handed = self.roll_handedness();
        let takeoff_in = self.roll_takeoff();
        let mut gull = Gull {
            id,
            tile,
            dir,
            progress: 0,
            prev_tile: tile,
            prev_progress: 0,
            prev_dir: dir,
            handed,
            state: GullState::Walking,
            takeoff_in,
        };
        self.resolve_walls_for(tile, &mut gull.dir, handed, Walker::Gull);
        self.gulls.push(gull);
    }

    /// Auto-spawn a gull at the edge every `gull_period` ticks (double rate
    /// during the final-scramble surge), at a PRNG perimeter tile, facing
    /// into the board.
    pub(super) fn run_gull_spawner(&mut self) {
        if self.gull_period == 0 {
            return;
        }
        let period = if self.in_surge() {
            (self.gull_period / 2).max(1)
        } else {
            self.gull_period
        };
        if !self.tick.is_multiple_of(u64::from(period)) {
            return;
        }
        // Balance: the ambient flock is capped so the late round stays
        // playable (raiders leaving keeps the population cycling). Tide
        // events (GullMania, GullAttack) deliberately bypass the cap.
        if self.gulls.len() >= GULL_CAP {
            return;
        }
        let (w, h) = (u32::from(self.width), u32::from(self.height));
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
        let tile = self.index(x as i32, y as i32);
        if matches!(self.tiles[tile as usize], TileKind::Rock | TileKind::Kelp) {
            return; // unlucky roll; the flock circles and tries again later
        }
        self.spawn_gull(x as u8, y as u8, dir);
    }

    fn roll_takeoff(&mut self) -> u32 {
        TAKEOFF_MIN + self.rng.next_u32() % (TAKEOFF_MAX - TAKEOFF_MIN + 1)
    }

    pub(super) fn move_gulls(&mut self) {
        let mut departed: Vec<usize> = Vec::new();
        for i in 0..self.gulls.len() {
            let mut gull = self.gulls[i];
            gull.prev_tile = gull.tile;
            gull.prev_progress = gull.progress;
            gull.prev_dir = gull.dir;
            let raided = match gull.state {
                GullState::Walking => self.walk_gull(&mut gull),
                GullState::Flying { .. } => self.fly_gull(&mut gull),
            };
            if raided {
                // Balance: a successful raider hauls its loot back to the
                // flock and leaves the beach, so gull pressure regulates
                // itself instead of compounding all round.
                departed.push(i);
            } else {
                self.gulls[i] = gull;
            }
        }
        for &i in departed.iter().rev() {
            self.gulls.remove(i);
        }
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
        if let TileKind::Castle(owner) = self.tiles[t] {
            if self.castle_raids {
                self.damage_castle(owner, gull.tile);
            }
            return true;
        }
        if self.turnstile_deflect(gull.tile, &mut gull.dir, gull.handed, Walker::Gull) {
            return false;
        }
        if let Some(mut sp) = self.signposts[t] {
            gull.dir = sp.dir;
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
                    // 1×1 board: nowhere to fly. Land where we are.
                    gull.state = GullState::Walking;
                    gull.takeoff_in = self.roll_takeoff();
                    return false;
                }
            }
            gull.tile = self.neighbor(gull.tile, gull.dir);
            remaining = remaining.saturating_sub(1);
            if remaining == 0 {
                if matches!(
                    self.tiles[gull.tile as usize],
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
                    if let TileKind::Castle(owner) = self.tiles[gull.tile as usize] {
                        if self.castle_raids {
                            self.damage_castle(owner, gull.tile);
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

    /// Spec §3.4: a gull reaching a castle carries off **half** the banked
    /// crabs (score halves, rounding the loss up). A raid on a fat castle is
    /// devastating and legible from across the room, and the tier drop falls
    /// out of the score loss. At most [`SPILL_CAP`] of the lost crabs respawn
    /// as live crabs in the surrounding tiles; the rest are carried off by
    /// the flock.
    pub(super) fn damage_castle(&mut self, owner: PlayerId, castle_tile: u16) {
        let score = self.scores[owner as usize];
        let target = score / 2;
        let spill = (score - target).min(SPILL_CAP);
        self.scores[owner as usize] = target;

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
    }

    /// Where a creature actually stands, in board subunits: the tile it is
    /// filed under, plus how far it has walked out of that tile's centre.
    ///
    /// Measuring against the tile alone is what let a gull and a crab walk
    /// through each other. Two creatures approaching head-on cross the gap
    /// between two tile centres while still filed under *different* tiles,
    /// so a same-tile test never sees the contact, and by the time they do
    /// share a tile they have passed and their offsets point apart.
    fn sub_position(&self, tile: u16, dir: Direction, progress: u16) -> (i32, i32) {
        let (x, y) = self.coords(tile);
        let (dx, dy) = sub_offset(dir, progress);
        (
            x * i32::from(SUBUNITS_PER_TILE) + dx,
            y * i32::from(SUBUNITS_PER_TILE) + dy,
        )
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
            let (gx, gy) = self.sub_position(gull.tile, gull.dir, gull.progress);
            let mut c = 0;
            while c < self.crabs.len() {
                let crab = self.crabs[c];
                let (cx, cy) = self.sub_position(crab.tile, crab.dir, crab.progress);
                if (gx - cx).unsigned_abs() + (gy - cy).unsigned_abs() <= u32::from(EAT_RANGE) {
                    self.crabs.remove(c);
                    continue;
                }
                c += 1;
            }
        }
    }
}
