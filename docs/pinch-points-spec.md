# Pinch Points: Design and Technical Specification

**Version:** 0.1 (pre-release; describes shipped behaviour)
**Engine:** Rust + Bevy 0.19
**Genre:** Real-time grid routing puzzler / party game
**Players:** 1 (puzzle), 2–6 (versus, local + online)

---

## 1. Elevator Pitch

The tide is out. Hundreds of crabs wander the drying sand in straight lines.
You cannot touch them. All you can do is jam **driftwood signposts** into the
sand and let the crabs walk into them. Route them into your sandcastle before
your rivals route them into theirs, and before the gulls arrive.

---

## 2. Lineage & Design Intent

An original game built on the mechanical skeleton of **ChuChu Rocket!**
(Sonic Team, Dreamcast, 1999), which it owes and credits. What that skeleton
is, and what this game makes of it:

| The shape it starts from | What it is here |
|---|---|
| Autonomous agents walking a tile grid | Crabs, not mice |
| Wall-collision turn-preference rule | Extended with per-crab handedness |
| Player-placed directional tiles | Driftwood signposts |
| Limited simultaneous placements | Same, retuned |
| Predator that degrades placements | Gulls, with a flight state |
| Goal tiles that bank agents | Sandcastles that visibly grow |
| Penalty for predator reaching goal | Turret collapse + crab spill |
| Puzzle / versus / editor modes | Same structure, original levels |

Every asset, level, and line of code here is this project's own. **No original
level layouts are reproduced** - every stage is authored fresh - and no art,
audio, or text is taken from the original.

**Design intent:** ChuChu Rocket works because it has exactly one verb.
Every addition taxes that clarity and must earn its place.

**The one big mechanical divergence: handedness.** The original's
turn-preference rule is global; here it is per-crab. **Left-clawed** crabs try
left before right, **right-clawed** the reverse, and the oversized,
colour-tinted claw is the tell. A one-bit change that turns herding puzzles
into **sorting** puzzles: the same corridor routes two crabs to two
destinations.

---

## 3. Core Concepts

### 3.1 The Board

- Tile grid, default **12 × 9**.
- **Walls live on edges between tiles, never on tiles.** A "wall tile" model
  is wrong and causes level-authoring bugs.
- Board edges are walled by default; per-level `wrap: on` opens them,
  creatures leaving one side re-enter on the opposite (§9 #8: the original
  wraps, and classic puzzles hinge on wrap-timing dodges).
- A tile is exactly one kind: nothing, a signpost, a sandcastle, a crab hole
  (spawner), a rock (impassable), or one of the three terrain kinds in §3.8.
  Rock is a tile a creature may not enter rather than four walls, so
  authoring cannot desync rock and wall data (§7.3).

### 3.2 Crabs

Crabs walk forward at constant speed and never stop; the player has no direct
control. Each carries a **handedness** bit assigned at spawn.

| Type | Speed | Value | Notes |
|---|---|---|---|
| **Common crab** | 1.0× | 1 | The bulk of the population |
| **Juvenile** | 1.5× | 2 | Fast, hard to route |
| **Giant crab** | 0.6× | 10 | Slow, valuable, a liability to protect |
| **Molting crab** | 1.0× | 5 + effect | On banking: for 5 s all loose crabs path toward the banker's castle. Lures do not stack, and a further 30 s must pass after one ends before another may start |
| **Golden crab** | 1.25× | 50 | The jackpot: rare and quick |
| **Sparkling crab** | 1.0× | 1 + effect | On banking: spins the tide-event roulette (§3.6) |

Distinct values mean what is walking toward a castle reads as a score
forecast. Spawner mix: 68% common, 15% juvenile, 8% giant, 3% molting,
2% golden, 4% sparkling, deterministic per seed.

Versus has since moved on from this table in one place: its golden crab
is never rolled. One is announced and comes out of a chosen hole once
every three minutes, worth 30 (see `docs/guide.md`).

### 3.3 Signposts (the player's only verb)

A plank pointing in one of four directions; any creature entering the tile
has its direction set to it. Signposts affect **all** creatures, including
crabs you didn't intend and gulls you didn't invite.

- **3 simultaneous per player**; a fourth evicts your oldest.
- Two health states (`Full` → `Worn` → destroyed); a walking gull degrades
  one state per crossing.
- Placeable only on empty sand, never on a **rival's** signpost. Placing on
  your **own** re-points it: health and lifetime reset, eviction order
  refreshed.
- **Versus posts fade after 10 seconds** (the original's valve against stale
  fortifications), rendering dimmer as they age. Puzzle boards keep posts
  forever: a fixed inventory implies permanence.
- Only the owner may remove one (frozen). Rivals interfere by routing, not
  deleting.
- Placement is instant; the cap is the only constraint.

### 3.4 Sandcastles (the goal)

One castle per player, one tile, their flag. Crabs entering are banked.
Castles **grow with score** and double as the scoreboard: the HUD is a
redundancy:

| Banked crabs | Appearance |
|---|---|
| 0–9 | Bare mound, one flag |
| 10–24 | Outer wall |
| 25–49 | Wall + two turrets |
| 50+ | Full keep, moat, pennants |

**Damage.** A gull reaching a castle **carries off a third of the banked
crabs** (rounded up), **spills some back as live crabs** in the surrounding
tiles (up to a spill cap), then **leaves the beach with its loot**: raids
drain the flock rather than compounding it (balance freeze: without
departure, four-player rounds collapse to zero under repeated raids). A
third rather than half keeps a wild gull nobody steered from deciding most
rounds. The spill turns a private penalty into a scramble everyone joins.

### 3.5 Gulls (the predator)

Gulls eat crabs on contact, degrade signposts they walk over, and raid
castles. **Gulls are steerable**: they are the entire offensive layer.

**Walking (default, ~85% of the time):** moves like a crab, obeying walls and
signposts, but slower; degrades posts, eats crabs it meets, fully
weaponizable.

**Flying (brief, occasional):** on a jittered per-gull timer, hops 2–4 tiles
in its current direction, ignoring walls and signposts, eating nothing, then
lands and walks. Exists so nobody can wall off a corner and stop thinking
about gulls; kept short and rare because unsteerable gulls would cost the
offensive layer.

**Tuning dials:** takeoff frequency, flight distance, walking speed. If the
game becomes all gull-aiming, raise takeoff frequency.

**Population (balance freeze):** the ambient edge spawner keeps the flock
at a cap, counting the gulls on their way in: 6 on a puzzle and on a versus
beach no bigger than the classic 12x9; a bigger versus beach scales the cap
and the spawner's rate by its area, so gulls per tile stay level. Tide
events bypass the cap on purpose. With raiders departing, gull pressure
cycles instead of accumulating.

In versus the full flock turns over rather than pausing the spawner: once
the newcomer has a tile to land on, the oldest walking gull more than two
tiles from every castle (measured across the seam on an open beach) flies
off as it is sent for. A gull that close to a castle is somebody's attack
and is never the one sent off. In the final 30 seconds the cap rises by
half, so the doubled rate adds birds rather than only swapping them.

### 3.6 Tide events (the sparkling crab)

The original's "?"-roulette, re-themed. A sparkling crab banks for 1 and
spins the roulette on boards with events enabled (versus arenas; never
puzzles):

| Event | Effect |
|---|---|
| Crab Mania | Gulls washed away; spawners flood crabs for 10 s |
| Gull Mania | Spawners emit gulls for 10 s |
| Crab Monopoly | Half the loose crabs scuttle straight into the banker's castle |
| Gull Attack | A gull lands beside every rival castle, facing it: every castle but the spinner's, and in a round played in teams but its partners' too |
| Speed Up | Everything moves at double speed, and the spawner holes open twice as often, for 10 s |
| Slow Down | Everything moves at half speed for 10 s |
| Fresh Sand | Every signpost washes away |
| Castle Swap | Castles trade owners in rotation ("rockets swap places"). When three seats or more hold castles, the spinner's castles sit out and the other seats' rotate; two seats holding castles (however many castles each), or a swap the spectators call (nobody spun it), rotate everyone. Banks stay with their seats |
| Right Claws | For 10 s a right-clawed crab banks for double and a left-clawed one costs what it was worth |

The roulette spins one event at a time: after a timed one (the manias,
Speed Up, Slow Down, Right Claws) it locks until the event ends, and a
sparkling crab banked meanwhile scores but spins nothing. The instant ones
(Crab Monopoly, Gull Attack, Fresh Sand, Castle Swap) leave it free.

A spectators' call is not a spin, and fires whatever the lock says, so
timed events can overlap. The manias, the speed pair and Right Claws each
keep their own clock: a called event lands on top of a running one from
another of the three, and replaces one from its own (a mania for a mania,
a speed for a speed). The feed card shows every timed event running.

On screen, the event's banner names it, says what it does and, above
the name, whose sparkling crab spun it, in that seat's colour; the feed
records it the same way. While a timed event runs, the top of the feed
card shows its name, the seconds left and a draining bar, read off the
board's own countdown.

### 3.7 The Tide

The round timer and its spectacle. A round lasts ~3 minutes with the
waterline visibly creeping in at the board edge: an ambient clock. At 30
seconds the gull spawn rate rises; at zero the sim freezes, scores locked at
the wave, results card over the held board. Only the freeze shipped; the
round-end spectacle and the *Rising Tide* variant are in `backlog.md`.

### 3.8 Terrain

Three passable tile kinds that shape a stream without spending a signpost.
None is in the original. Each is a `TileKind` like rock, so a tile is still
exactly one thing, and each is one character in a level file (`T`/`t`, `K`,
`~`).

**Turnstile log.** A pivoting log that deflects each creature crossing it to
its right, then its left, then its right again, flipping after every
crossing. A deterministic 50/50 splitter that takes no PRNG draw, so it
costs nothing in replay or lockstep and a level author can predict it
exactly. It resolves *instead of* the tile's signpost, not before it
(§4.1): a post standing on a turnstile is never consulted.

**Kelp.** Crabs slip through; a walking gull cannot enter at all, and a
flying gull that would land in kelp glides one more tile to find somewhere
it can stand. Kelp is how a supply lane is made gull-proof without walling
it, which would stop the crabs too.

**Tide pools.** Shallow water. A creature standing in one moves at half its
usual step, never below one subunit per tick, applied after the tide
event's tempo (§3.6) so the two compose. The only terrain that changes
speed rather than direction.

---

## 4. Movement & Turn Resolution

### 4.1 Resolution order on arriving at a tile centre

```
1. If tile contains a sandcastle    → bank the crab, despawn, award score. Stop.
                                      (a gull raids it instead, and departs)
2. If tile contains a turnstile     → deflect to the log's current side, flip
                                      the log, wall-resolve. Stop: steps 3
                                      and 4 do not run (§3.8).
3. If a lure is running             → step toward the luring castle. Crabs
                                      only, and it outranks the signpost
                                      (§3.2, the molting crab).
4. If tile contains a signpost      → set direction := signpost.direction
5. Wall resolution (from current direction):
     if forward is passable         → continue forward
     else if preferred side passable→ turn to preferred side
     else if other side passable    → turn to other side
     else                           → reverse
   where "preferred side" = left for left-clawed, right for right-clawed.
6. Resolve collisions on this tile (gull eats crab, etc.)
```

**Decision (frozen):** a signpost pointing into a wall is **followed**, then
wall-resolved from the signpost's direction. Left- and right-clawed crabs
therefore exit the same into-wall signpost on opposite sides: a deliberate
sorting tool. Encoded in `signpost_into_wall_is_followed_then_resolved`;
changing it invalidates authored levels.

### 4.2 Sub-tile position

Movement is **integer-only** for determinism. No floats in simulation
position: float drift costs bit-exact determinism, which costs rollback
netcode, replays, and headless validation.

- One tile = **256 subunits**.
- Common crab **12 subunits/tick**; juvenile **18**; giant **7** (0.6× of 12
  is 7.2, rounded down, frozen); walking gull **8**.
- On crossing 256: snap to the next tile, run resolution, carry the
  remainder.
- The step is the kind's speed adjusted by the tide event's tempo, then
  halved (never below 1) if the walker stands in a tide pool (§3.8). One
  statement of the rule serves crabs and gulls both.

### 4.3 Collision

A gull eats a crab when the two stand within 48 subunits of each other,
measured as Manhattan distance between their positions **on the board**:
tile centre plus how far each has walked out of it. Resolved in fixed
iteration order, never hash-map or query order, so every machine agrees.

Measuring within a tile is not enough, and shipped that way until it was
found in play: two creatures walking head-on close the gap while still filed
under *different* tiles, so a same-tile test never sees the contact, and by
the time they share a tile they have passed each other.

### 4.4 Tick order (frozen)

Within one 30 Hz tick:

1. **Player actions**, in the tick's seat order: one seat leads, the rest
   follow round the table, the lead rotating per tick as a pure function of
   the tick (so peers and replays agree). On a same-tile conflict the order
   decides; the rotation exists so this does not always favour the lowest
   seat.
2. **Signpost expiry**, where posts expire (versus).
3. **The golden call**: a called golden crab comes out of its hole on its
   tick, past the crab cap; at the start of each stretch the next call is
   drawn.
4. **Spawners**, in tile-index order. On a versus beach (fair spawns), a
   tick where the room left under the crab cap is less than the holes
   opening draws, from the board's PRNG, which opening hole starts, and
   the rest follow round in tile-index order; a beach that never fills
   plays in plain tile-index order.
5. **The gull spawner**: gulls sent for earlier land, then the edge
   spawner sends for the next (see §3.5, population).
6. **Crab movement**, in stable spawn order; banking removes a crab
   without disturbing the survivors' order.
7. **Gull movement**, in stable spawn order; a raider leaves the beach.
8. **The eat pass**: each walking gull, in order, eats every crab within
   range (§4.3).
9. **Timers** count down: the lure, then the mania, Right Claws and the
   speed events.
10. **Crab Monopoly's crabs** in flight land and bank.

The event cooldown counts down before any of it, so an event spun this
tick gets its whole cooldown.

A signpost placed this tick affects creatures arriving this tick. A crab
sealed on all four sides waits in place.

---

## 5. Game Modes

### 5.1 Tide Pool (puzzle)

Fixed signpost inventory, no score: place, press start, watch it run.
The classic goal, and most of the campaign, is **all crabs**: every crab
home is a win, one escapee is a loss. A handful of stages carry a goal of
their own instead (`goal:` in the level file, `round:` for a tide of its
own): **bank N** crabs, **survive** with none eaten until the tide, or
bank a **golden** crab. Every stage ends inside a sixty-second tick limit
even without a tide, which a survive stage runs out to win and every other
goal loses to. Deterministic and short, so failures are readable and retries
instant, and it exercises the whole core sim while postponing caps,
degradation, tiers and networking.

### 5.2 Turf War (versus)

2–6 players, local or online: the full ruleset. Five and six seats need a
generated arena wide enough for the two long-edge castles, or a handmade
map built for the table.

**Maps** (the map byte of the match terms, appended in this order and
never reordered): the handcrafted classic arena (12x9, 2-4); generated
beaches at 9x7, 12x9, 16x11 and 20x13; the open ocean (generated, 16x11,
no edges); a beach the host built, which travels whole; seven handmade
maps, embedded level files under `levels/versus/` that state the tables
they seat (`seats:`): The Gates, The Moat, Log Jam, Three Lanes (13x9 or
15x9, 2 and 4), Round Table (15x11, 2, 4 and 6), Round the World (21x12,
no edges, 2, 4 and 6, `roll: 7`) and Duel (11x7, 2); **Surprise**; and
four more handmade maps, appended after it: The Fountain (13x11, 2 and
4: every hole sits on a rock in a pond in the middle), Four Doors (21x11,
no edges, 3, `roll: 7`: each castle walled in with a door on each side),
Gull Road (25x13, no edges, 5, `roll: 5`: kelp hedges keep the walking
gulls on a road along the top and bottom rows, with two gates to each
castle) and Back to Back (21x12, no edges, 2, 4 and 6, `roll: 7`: the
castles stand in pairs back to back, a wall between them).

An odd table gets a handmade map only on an open beach of identical
strips, one castle a strip and none put away, whose seam (the first and
last columns) is kelp or rock from the second row to the second last, so
the edge spawner lands gulls on the top and bottom rows of every strip
alike. Every castle is then a copy of every other. On a fixed layout
with a castle put away, the one across from the gap wins 45-58% at
three, which is why the other handmade maps leave odd tables to a
generated beach.

**Surprise** generates a beach every round in a style drawn from the
round's seed: Maze, Driftwood (logs), Islands, Lagoon, and Lanes at four
seats or fewer, at 12x9 up to four seats and 16x11 (17 wide) at five and
six. A style is a terrain step over the generated frame (castles, holes
and rules as the classic terrain's), laid in mirror groups of four and
retried until every castle is reachable from every hole, no sand is cut
off and no castle is boxed in; the classic terrain is the default, and
every seed builds the beach it always built.

**The round's turn.** Every versus board but a player's own beach is
mirrored left-right and/or top-bottom from the round's seed, and an
edgeless handmade map is rolled round by a whole number of its strips
(`roll:`, 7 columns on Round the World, Four Doors and Back to Back, 5 on
Gull Road); a handmade map first deals its castles to the seats in
opposite pairs (0/1, 2/3, 4/5 stay across from each other, which both
team modes rely on), and at an odd table the seat left over keeps the
last castle, a copy of the others on the only beaches that seat one. The sim and the AI lean a few points toward one diagonal pair on a
beach that is mirror-symmetric both ways; the mirror spreads the lean
over every castle. Every peer turns the board alike from the seed, and a
replay records the board as played. The AI measures distances through
the seam of an open beach (its hand's walk stays on the flat grid, since
a cursor does not wrap).

### 5.3 Driftwood (editor)

Walls, castles, spawners, rocks, crab mix, gull count; ships with the game.
Puzzle levels are **auto-validated** by brute-forcing placements against the
headless sim: a direct payoff of determinism.

---

## 6. Title

**Pinch Points.** A *pinch point* is a routing term, the bottleneck a flow
must squeeze through, which is what the player builds; and crabs pinch.

**Known drawback:** it is also a heavily-used industrial safety term with a
faint injury connotation, so search visibility will be an uphill fight and
store art has to say "cheerful crab puzzler" immediately.

---

## 7. Technical Architecture

### 7.1 Stack

**Rust + Bevy 0.19.** Third-party Bevy crates trail each release by weeks or
months, so check compatibility before depending on one, netcode especially
(§9 risk 6).

### 7.2 The simulation lives outside the ECS

The Bevy instinct, one entity per crab, fights this game three ways:

1. Every rule is "what else is on this tile": a spatial-index query the ECS
   gives you nothing for.
2. Query iteration order is not guaranteed stable, and resolution order
   genuinely affects outcomes, which is fatal for rollback netcode.
3. Crab floods push hundreds of near-identical entities through the
   archetype machinery for no benefit.

Instead:

```rust
/// The entire game state. No Bevy types. Testable with `cargo test`.
pub struct Board {
    width: u8,
    height: u8,
    h_walls: Vec<bool>,      // per-edge storage, see §7.3
    v_walls: Vec<bool>,
    tiles: Vec<TileKind>,    // Empty | Rock | Castle(PlayerId) | Spawner
    signposts: Vec<Option<Signpost>>,
    crabs: Vec<Crab>,
    gulls: Vec<Gull>,
    scores: [u32; MAX_PLAYERS], // six seats
    rng: Pcg32,              // seeded, ticked deterministically
    tick: u64,
}

pub struct Crab {
    tile: u16,
    dir: Direction,
    progress: u16,           // 0..256 subunits
    handed: Handedness,
    kind: CrabKind,
}

impl Board {
    pub fn tick(&mut self, actions: &[PlayerAction; MAX_PLAYERS]) { /* ... */ }
}
```

Wrap it in a Bevy `Resource`; spawn one sprite entity per creature with a
`CrabId(u32)` and sync transforms in a render system. The sim never knows
Bevy exists. **Payoffs:** unit-testable without an `App`, trivially
serialisable, and deterministic by construction.

### 7.3 Wall storage

**Decision (frozen): two arrays**, horizontals `w × (h+1)`, verticals
`(w+1) × h`. Each edge stored exactly once, so the two tiles sharing it
cannot disagree (the desync class a per-tile side mask invites, via authoring
tools). Rocks are impassable *tiles*, not four walls, for the same
single-source-of-truth reason.

### 7.4 Timestep

Simulation in `FixedUpdate` at **30 Hz**; rendering interpolates between
`prev_progress` and `progress` using the overstep fraction. Decoupling sim
rate from frame rate up front is far cheaper than retrofitting it.

### 7.5 Determinism checklist

Non-negotiable if netcode is in scope:

- [x] Integer-only simulation arithmetic. No `f32` anywhere in `Board`.
- [x] Fixed iteration order everywhere. No `HashMap`/`HashSet` iteration.
- [x] Seeded PRNG owned by `Board`, advanced only inside `tick()` (inline PCG32, no dependency).
- [x] Spawns derived from the seeded PRNG, never wall-clock time.
- [x] No dependence on Bevy query order, entity IDs, or system execution order.
- [x] Cross-platform test: 10,000 ticks from a fixed seed, hash the state,
      compare against a fixed anchor (`tests/it/determinism.rs`). CI runs it
      on Linux; the release workflow builds, but does not test, on Windows
      and macOS, so the anchor is what a run there would be held to.

### 7.6 Networking

**Shipped: deterministic lockstep**, 3-frame input delay (100 ms at 30 Hz)
on a quick network and up to 6 on a slow one (the host picks it per round
from its players' round trips), UDP host-relay star. A frame simulates
only once every seat's action is known, and a player's own commits pace
it to one a tick; the host keeps the players' clocks level with its own.
Peers exchange state hashes every 30 frames to catch desync loudly.

Each action packs into **3 bytes**:

```rust
// byte 0: cursor column
// byte 1: cursor row
// byte 2: bits 0-1 op (none/place/remove), bits 2-3 direction
```

(A full byte per axis: the nibble this spec first called for caps boards at
16 wide, and the XL beach is 20. 3 bytes × 6 players × 30 Hz is trivial.)

**Rollback remains available**, unbuilt for want of a need rather than a
dependency: the delay it would give back is three frames on a link whose
round trip is a millisecond. The packed input and `Board` snapshots keep the
swap contained if that changes; see `backlog.md`, which also says why the
Bevy wrappers are the wrong way in.

### 7.7 Replays

A replay is the starting level plus the per-tick input list; replaying
through `Board::tick` reproduces the round bit-for-bit. A finished round the
player keeps from its results card (or the interlude after it) goes into a
library, and any round can travel as a share code.

---

## 8. Input

Two-stage model throughout: **move a cursor, then commit a direction.** The
cursor is per-player, tinted to the flag colour.

### 8.1 Controller (primary target)

The face buttons form a diamond, so four directions. Natural mapping, default:

| Control | Action |
|---|---|
| D-pad / left stick | Move tile cursor (with hold-to-repeat) |
| **Y / △** (top) | Place signpost pointing **up** |
| **A / ✕** (bottom) | Place signpost pointing **down** |
| **X / □** (left) | Place signpost pointing **left** |
| **B / ○** (right) | Place signpost pointing **right** |
| L1 / LB | Remove signpost under cursor |
| R1 / RB | Clear all own signposts |
| Start | Pause / ready-up |

### 8.2 Keyboard

| Control | Action |
|---|---|
| `WASD` | Move tile cursor |
| `↑ ↓ ← →` | Place signpost in that direction |
| `Space` | Remove signpost under cursor |
| `Shift` | Clear all own signposts |
| `Esc` | Pause |

Left hand navigates, right hand commits: the controller's stick/face split,
no modal state. A single-hand preset (`WASD` moves, `IJKL` commits) ships in
settings.

### 8.3 Local multiplayer

Up to 6 seats, hot-plug: pads fill from the highest seat down so the
keyboard's two stay usable. Cursor colours match castle flags and pair with
distinct cursor **shapes**, so seats stay tellable-apart under colour-vision
deficiencies.

### 8.4 Accessibility

Full remapping (keyboard and pad), adjustable cursor repeat rate and delay,
high-contrast cursor toggle, optional sim slow-down in single-player, and a
colour-blind-safe palette for flags, claws, and cursors.

---

## 9. Open Questions & Risks

How each design risk was settled. Remaining ideas live in `backlog.md`.

| # | Item | Settled |
|---|---|---|
| 1 | Turn-preference handedness in the original (left-first vs right-first) | Moot: handedness is per-crab here (§2), so neither global rule applies |
| 2 | Signpost pointing into a wall | Signpost is followed, then wall-resolved (§4.1) |
| 3 | Original's roulette trigger conditions | Not reproduced; the tide-event set and its tuning are our own (§3.6) |
| 4 | Gull steerability tuning | Playtested to the dials in §3.5; steering gulls is the offensive game, not the whole game |
| 5 | Handedness + directional castle gates | Gates dropped. Handedness alone carries the sorting puzzles, and the pair was unreadable in the player's head |
| 6 | Bevy 0.19 ecosystem maturity for netcode | Overtaken (re-checked 2026-09-17): `bevy_ggrs` 0.22 takes 0.19, and raw `ggrs` never wanted an engine at all. Online ships as deterministic lockstep because a 100 ms delay over a 1 ms LAN does not need prediction, not because rollback is blocked (§7.6, `backlog.md`) |
| 7 | Title SEO collision | Accepted; revisit before a store page |
| 8 | Board wrap-around edges | The original wraps. Per-level flag, taught at campaign level 26, and the open-ocean and Round the World versus maps turn it on |
