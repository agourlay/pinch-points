//! Crabs: the spawner holes, the walking pass, and what happens when one
//! arrives somewhere - a castle, a turnstile, or a lure calling it home.

use super::*;

impl Board {
    /// Place a crab directly (puzzle setups and tests; spawner tiles handle
    /// the normal case). The crab immediately wall-resolves so it never starts
    /// a tick facing a wall.
    pub fn spawn_crab(&mut self, x: u8, y: u8, dir: Direction, handed: Handedness, kind: CrabKind) {
        assert!(
            self.in_bounds(i32::from(x), i32::from(y)),
            "crab off the board"
        );
        let tile = self.index(i32::from(x), i32::from(y));
        assert!(
            self.grid.tiles[tile as usize] != TileKind::Rock,
            "crab on a rock"
        );
        let id = self.next_crab_id;
        self.next_crab_id += 1;
        let mut crab = Crab {
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
            kind,
        };
        self.resolve_walls(&mut crab);
        self.crabs.push(crab);
    }

    pub(super) fn run_spawners(&mut self) {
        for t in 0..self.grid.tiles.len() {
            let TileKind::Spawner(s) = self.grid.tiles[t] else {
                continue;
            };
            // Manias override the cadence: floods every 8 ticks.
            let period = match self.mania {
                Some((Mania::Crab | Mania::Gull, _)) => 8,
                None => u64::from(s.period),
            };
            if !self.tick.is_multiple_of(period) {
                continue;
            }
            if let Some((Mania::Gull, _)) = self.mania {
                // Balance: the mania flood is dramatic but bounded. Beyond
                // three flocks' worth the beach becomes unplayable for the
                // rest of the round (mania gulls only leave by raiding).
                if self.gulls.len() < GULL_CAP * 3 {
                    let (x, y) = self.coords(t as u16);
                    self.spawn_gull(x as u8, y as u8, s.dir);
                }
                continue;
            }
            // The beach fills to the cap and then waits for it to clear.
            // Crab Mania floods past it, which is the event, but only to
            // twice the cap, the same way Gull Mania stops at three flocks:
            // unbounded, it buried the board under two crabs a tile.
            let ceiling = match self.mania {
                Some((Mania::Crab, _)) => self.crab_cap() * 2,
                _ => self.crab_cap(),
            };
            if self.crabs.len() >= ceiling {
                continue;
            }
            let handed = self.roll_handedness();
            // Weighted kind mix so live boards show the whole population:
            // mostly commons, a scattering of juveniles, the odd giant or
            // molting crab, and once in a blue tide a golden jackpot.
            //
            // Molting was 4% and is 3%; the point went to the commons. It
            // is the only kind whose effect outlives the banking, so its
            // rate and `LURE_COOLDOWN` set lure uptime together, and one
            // in twenty-five crabs arriving with a lure attached kept the
            // beach under one for a third of a round.
            let kind = match self.rng.next_u32() % 100 {
                0..=69 => CrabKind::Common,
                70..=84 => CrabKind::Juvenile,
                85..=92 => CrabKind::Giant,
                93..=95 => CrabKind::Molting,
                96..=97 => CrabKind::Golden,
                98.. => CrabKind::Sparkling,
            };
            let id = self.next_crab_id;
            self.next_crab_id += 1;
            let mut crab = Crab {
                id,
                tile: t as u16,
                dir: s.dir,
                progress: 0,
                prev: Pose {
                    tile: t as u16,
                    dir: s.dir,
                    progress: 0,
                },
                handed,
                kind,
            };
            self.resolve_walls(&mut crab);
            self.crabs.push(crab);
        }
    }

    /// How many live crabs the ambient spawners will fill the beach to.
    /// Proportional to the board, so an XL arena still feels busy and the
    /// smallest puzzle board is not starved.
    pub(super) fn crab_cap(&self) -> usize {
        (self.grid.tiles.len() / CRAB_CAP_TILES_PER_CRAB).max(8)
    }

    pub(super) fn move_crabs(&mut self) {
        // One board scan per tick, not one per crab arrival (refreshed on
        // banks below, which is when a lure can start mid-tick).
        let mut lure_target = self.lure_target();
        let mut banked: Vec<usize> = Vec::new();
        for i in 0..self.crabs.len() {
            let mut crab = self.crabs[i];
            crab.prev = crab.pose();
            // Arrival resolution guarantees the exit direction is passable
            // except for a crab sealed in on all four sides; that crab waits.
            if !self.passable(crab.tile, crab.dir) {
                self.crabs[i] = crab;
                continue;
            }
            crab.progress += self.walk_step(crab.tile, crab.kind.speed());
            let mut was_banked = false;
            while crab.progress >= SUBUNITS_PER_TILE {
                crab.progress -= SUBUNITS_PER_TILE;
                crab.tile = self.neighbor(crab.tile, crab.dir);
                if self.resolve_arrival(&mut crab, lure_target) {
                    was_banked = true;
                    // A molting bank starts a lure that later arrivals this
                    // same tick must already obey, as they always did.
                    lure_target = self.lure_target();
                    break;
                }
            }
            if was_banked {
                banked.push(i);
            } else {
                self.crabs[i] = crab;
            }
        }
        // Remove back-to-front so earlier indices stay valid and the stable
        // creature order (our fixed resolution order) is preserved.
        for &i in banked.iter().rev() {
            self.crabs.remove(i);
        }
        // Sparkling banks spin the roulette only now: events like Monopoly
        // drain the crab list, which must not happen mid-iteration.
        let queued = std::mem::take(&mut self.event_queue);
        for banker in queued {
            self.spin_tide_event(banker);
        }
    }

    /// Spec §4.1 resolution on arriving at a tile centre. Returns `true` if
    /// the crab banked and must despawn.
    ///
    /// Frozen decision for spec §9 open question 2: a signpost pointing into
    /// a wall is *followed*, and wall resolution then applies from the
    /// signpost's direction.
    ///
    /// While a molting lure is active (spec §3.2), loose crabs ignore
    /// signposts entirely and greedily head for the luring player's castle;
    /// wall resolution still applies.
    pub(super) fn resolve_arrival(&mut self, crab: &mut Crab, lure_target: Option<u16>) -> bool {
        let t = crab.tile as usize;
        if let TileKind::Castle(owner) = self.grid.tiles[t] {
            self.scores[owner as usize] += crab.kind.value();
            self.crabs_banked += 1;
            match crab.kind {
                // A molt banked during a lure (anyone's) or in the quiet
                // spell after one banks for its points and nothing more.
                CrabKind::Molting => {
                    if self.lure.is_none() && self.lure_cooldown == 0 {
                        self.lure = Some((owner, LURE_TICKS));
                    }
                }
                CrabKind::Golden => self.golden_banked += 1,
                CrabKind::Sparkling => self.event_queue.push(owner),
                CrabKind::Common | CrabKind::Juvenile | CrabKind::Giant => {}
            }
            return true;
        }
        if self.turnstile_deflect(crab.tile, &mut crab.dir, crab.handed, Walker::Crab) {
            return false;
        }
        if let Some(dir) = self.lure_step(crab.tile, lure_target) {
            crab.dir = dir;
        } else if let Some(sp) = self.signposts[t] {
            crab.dir = sp.dir;
        }
        self.resolve_walls(crab);
        false
    }

    /// One fair coin flip of the sim's PRNG stream.
    pub(super) fn roll_handedness(&mut self) -> Handedness {
        if self.rng.next_u32() & 1 == 0 {
            Handedness::Left
        } else {
            Handedness::Right
        }
    }

    /// The luring player's castle tile, if a lure is active and that player
    /// still has a castle. Computed once per tick and threaded through
    /// arrivals, so the board scan is not repeated per crab.
    pub(super) fn lure_target(&self) -> Option<u16> {
        let (owner, _) = self.lure?;
        self.grid
            .tiles
            .iter()
            .position(|t| *t == TileKind::Castle(owner))
            .map(|t| t as u16)
    }

    /// Greedy step direction from `from` toward the cached lure target.
    pub(super) fn lure_step(&self, from: u16, lure_target: Option<u16>) -> Option<Direction> {
        let castle = lure_target?;
        let (fx, fy) = self.coords(from);
        let (cx, cy) = self.coords(castle);
        let (dx, dy) = (cx - fx, cy - fy);
        if dx == 0 && dy == 0 {
            return None;
        }
        Some(Direction::toward(dx, dy))
    }
}
