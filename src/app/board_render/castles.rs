//! Castles: their tier-scaled keep, the pennant on top, and the shudder
//! and floating points a bank sets off.

use super::{OnTile, WALL_COLOR, clear_stale, image_sprite, on_board, z};
use crate::app::Sim;
use crate::app::art::Art;
use crate::app::layout::{self, TILE};
use crate::app::palette;
use crate::sim::{TileKind, castle_tier};
use bevy::color::Mix;
use bevy::prelude::*;

/// Marker for a castle's sprite tree; carries the tier it was built at so a
/// score crossing a threshold rebuilds it (spec §3.4: the castle grows with
/// score and doubles as the scoreboard).
#[derive(Component)]
pub struct CastleSprite {
    x: u8,
    y: u8,
    tier: u8,
    /// Whose it is. Kept because the castle wears its owner's colour and a
    /// tide event can hand it to somebody else without its tier moving: on
    /// tier alone the sprite stands in the old owner's colour until the new
    /// one scores across a threshold.
    owner: u8,
}

impl OnTile for CastleSprite {
    fn tile(&self) -> (u8, u8) {
        (self.x, self.y)
    }
}

pub fn sync_castles(
    mut commands: Commands,
    sim: Res<Sim>,
    art: Res<Art>,
    mut covered: Local<Vec<bool>>,
    existing: Query<(Entity, &CastleSprite)>,
) {
    let board = &sim.0;
    clear_stale(
        &mut commands,
        board,
        &mut covered,
        existing,
        |sprite| match board.tile_at(sprite.x, sprite.y) {
            TileKind::Castle(owner) => {
                owner == sprite.owner && castle_tier(board.scores()[owner as usize]) == sprite.tier
            }
            TileKind::Empty
            | TileKind::Rock
            | TileKind::Spawner(_)
            | TileKind::Turnstile { .. }
            | TileKind::Kelp
            | TileKind::Pool => false,
        },
    );
    for (x, y, kind) in board.tiles() {
        let idx = usize::from(board.index_of(x, y));
        if covered[idx] {
            continue;
        }
        let TileKind::Castle(owner) = kind else {
            continue;
        };
        let tier = castle_tier(board.scores()[owner as usize]);
        let color = palette::player_color(owner);
        let pos = layout::tile_center(board, x, y);
        commands
            .spawn((
                CastleSprite { x, y, tier, owner },
                CastleKick(0.0),
                Transform::from_translation(pos.extend(z::TILE_FEATURE + 0.2)),
                Visibility::default(),
            ))
            .with_children(|parent| build_castle(parent, &art, tier, color));
    }
}

/// The sand a castle is built of before its owner's colour goes in.
const SAND: Color = Color::srgb(0.925, 0.84, 0.66);

/// A castle's walls: sand, dyed well toward its owner's colour.
///
/// It was the owner's colour outright, a flat coloured square on the
/// beach. Dyed sand still says whose it is from across the board, and
/// leaves the owner's full colour for the flags and the banner, which
/// is what they are for.
pub fn castle_body(owner: Color) -> Color {
    SAND.mix(&owner, 0.62)
}

/// One of a castle's two shadows, and where it rests under the castle at
/// its own size. The castle swells when a crab banks and when it flies in
/// a swap, and its shadows, being its children, swelled and lunged out
/// along the sun with it; [`steady_castle_shadows`] holds them to the
/// ground.
#[derive(Component)]
pub struct CastleShadow {
    at: Vec3,
}

/// A castle whose scale may have moved this frame.
type SwollenCastle = (With<CastleSprite>, Changed<Transform>);

/// Keep a castle's shadows where they lie and at their size however the
/// castle swells above them: their offset and size divided by its scale,
/// as a gull's are (a post's offset is too, though not yet its size).
/// After the kick and the flight, which are what scale it.
pub fn steady_castle_shadows(
    castles: Query<(&Transform, &Children), SwollenCastle>,
    mut shadows: Query<(&CastleShadow, &mut Transform), Without<CastleSprite>>,
) {
    for (castle, children) in &castles {
        let scale = castle.scale.x.max(f32::EPSILON);
        for child in children {
            if let Ok((shadow, mut place)) = shadows.get_mut(*child) {
                place.translation = (shadow.at.truncate() / scale).extend(shadow.at.z);
                place.scale = Vec3::splat(1.0 / scale);
            }
        }
    }
}

/// One piece of a castle: which sprite, how big and where, in tiles from
/// the tile's centre, and how far in front of the keep it is drawn.
struct Piece {
    size: f32,
    at: Vec2,
    z: f32,
}

/// Where a tier's keep stands and how big it is. It rises a little as the
/// castle grows, and moves back a step once there is a wall in front.
fn keep(tier: u8) -> Piece {
    let (size, lift) = match tier {
        0 => (0.66, 0.02),
        1 => (0.62, 0.08),
        2 => (0.64, 0.08),
        _ => (0.72, 0.10),
    };
    Piece {
        size,
        at: Vec2::new(0.0, lift),
        z: 0.0,
    }
}

/// The corner towers a tier has: the front pair from tier 2, the back pair
/// from tier 3. Back towers stand behind the keep and front ones before
/// the wall, which is what makes it read as a courtyard.
fn turrets(tier: u8) -> Vec<Piece> {
    let mut towers = Vec::new();
    for (from, y, z) in [(3, 0.30, -0.15), (2, -0.30, 0.2)] {
        if tier >= from {
            for side in [-1.0, 1.0] {
                towers.push(Piece {
                    size: 0.36,
                    at: Vec2::new(side * 0.40, y),
                    z,
                });
            }
        }
    }
    towers
}

/// How far above its centre a tower's top stands, as a fraction of its
/// sprite: the top merlons in `tools/gen_sprites.py`.
const TOWER_TOP: f32 = 0.27;

/// Everything standing on a castle's tile at `tier`: its shadow on the
/// sand, and its [`castle_parts`].
fn build_castle(parent: &mut ChildSpawnerCommands, art: &Art, tier: u8, color: Color) {
    let footprint = if tier == 0 { 0.8 } else { 1.15 };
    // It sits on the sand rather than in it.
    parent.spawn((
        CastleShadow {
            at: layout::SUN.extend(-0.4),
        },
        image_sprite(
            &art.shadow,
            Color::srgba(1.0, 1.0, 1.0, 0.7),
            Vec2::splat(TILE * footprint),
        ),
        Transform::from_translation(layout::SUN.extend(-0.4)),
    ));
    // And the shadow its height throws, long along the sun: the castle is
    // the tallest thing on the beach, and grows taller with each tier.
    let (sprite, place) = super::cast_shadow(
        art,
        Vec2::ZERO,
        TILE * footprint,
        TILE * (0.14 + 0.05 * f32::from(tier)),
        0.45,
        -0.41,
    );
    parent.spawn((
        CastleShadow {
            at: place.translation,
        },
        sprite,
        place,
    ));
    for part in castle_parts(art, tier, color) {
        let at = (part.at * TILE).extend(part.z);
        let size = part.size * TILE;
        let sprite = match &part.image {
            Some(image) => image_sprite(image, part.tint, size),
            None => Sprite::from_color(part.tint, size),
        };
        let mut piece = parent.spawn((sprite, Transform::from_translation(at)));
        if part.pennant {
            piece.insert(Pennant);
        }
    }
}

/// One piece of a castle, wherever it is drawn: on its tile, or on the
/// results card that shows off the winner's.
pub(crate) struct CastlePart {
    /// The sprite, or `None` for a plain block of `tint`: a flag's pole or
    /// its pennant.
    pub image: Option<Handle<Image>>,
    pub tint: Color,
    /// Width and height, in tiles.
    pub size: Vec2,
    /// Its centre, in tiles from the castle's, y up.
    pub at: Vec2,
    /// Depth among the castle's own pieces: higher is nearer.
    pub z: f32,
    /// Whether it is a pennant, which waves.
    pub pennant: bool,
}

/// Every piece of a castle at `tier` in its owner's `color`, back to front
/// (spec §3.4: the castle grows with its owner's score and is the
/// scoreboard):
///
/// | tier | points | adds |
/// |---|---|---|
/// | 0 | 0-9 | the keep and its flag |
/// | 1 | 10-24 | a curtain wall, gate at the front |
/// | 2 | 25-49 | two front towers, flying pennants |
/// | 3 | 50+ | two back towers, a taller keep, a moat |
///
/// Each tier adds pieces and none replaces one, so a castle only ever
/// gets bigger. Before, the keep itself grew until at tier 3 it covered
/// the wall it was meant to stand inside, and the tiers were hard to
/// tell apart at a glance. Everything but the sand shadow under it, which
/// belongs to the beach rather than to the castle.
pub(crate) fn castle_parts(art: &Art, tier: u8, color: Color) -> Vec<CastlePart> {
    let body = castle_body(color);
    let mut parts = Vec::new();
    let mut add = |image: &Handle<Image>, tint: Color, piece: &Piece| {
        parts.push(CastlePart {
            image: Some(image.clone()),
            tint,
            size: Vec2::splat(piece.size),
            at: piece.at,
            z: piece.z,
            pennant: false,
        });
    };
    let whole = |z| Piece {
        size: 0.96,
        at: Vec2::ZERO,
        z,
    };
    if tier >= 3 {
        // The moat, dug outermost: water, open in the middle.
        add(
            &art.moat,
            Color::WHITE,
            &Piece {
                size: 1.22,
                at: Vec2::ZERO,
                z: -0.3,
            },
        );
    }
    if tier >= 1 {
        add(&art.wall_back, body, &whole(-0.2));
    }
    let keep = keep(tier);
    add(&art.castle, body, &keep);
    // The owner's banner on the keep, and its door's arch, in full colour.
    add(
        &art.castle_trim,
        color.lighter(0.12),
        &Piece { z: 0.05, ..keep },
    );
    if tier >= 1 {
        add(&art.wall_front, body, &whole(0.1));
    }
    let towers = turrets(tier);
    for tower in &towers {
        add(&art.turret, body, tower);
    }
    for tower in &towers {
        let top = tower.at + Vec2::new(0.0, tower.size * TOWER_TOP);
        flag(&mut parts, color, top, 0.16, 0.6, tower.z + 0.05);
    }
    // The flag (spec §3.4: flying their colour flag): a driftwood pole
    // with a bright owner-coloured pennant, sticking up past the keep so
    // it reads at a glance.
    flag(
        &mut parts,
        color,
        keep.at + Vec2::new(-0.06, keep.size * TOWER_TOP),
        KEEP_POLE,
        1.3,
        0.3,
    );
    parts.sort_by(|a, b| a.z.total_cmp(&b.z));
    parts
}

/// A pole standing on `base` (tiles from the castle's centre), `height`
/// tiles tall, with a pennant of `scale` times the keep's first one.
fn flag(parts: &mut Vec<CastlePart>, color: Color, base: Vec2, height: f32, scale: f32, z: f32) {
    parts.push(CastlePart {
        image: None,
        tint: WALL_COLOR,
        size: Vec2::new(2.5 / TILE, height),
        at: Vec2::new(base.x, base.y + height / 2.0),
        z,
        pennant: false,
    });
    let size = Vec2::new(0.20, 0.12) * scale;
    parts.push(CastlePart {
        image: None,
        // The owner's colour as it is: the castle is dyed sand now, and a
        // lightened pennant, which stood out against a castle already in
        // that colour, washed out to white on a yellow one.
        tint: color,
        size,
        at: Vec2::new(base.x + size.x / 2.0, base.y + height - size.y / 2.0),
        z,
        pennant: true,
    });
}

/// How high over a castle's centre its leader's crown floats, in tiles:
/// clear above the top of the keep's flagpole, which rises with the tier.
fn crown_lift(tier: u8) -> f32 {
    let keep = keep(tier);
    keep.at.y + keep.size * TOWER_TOP + KEEP_POLE + CROWN_SIZE * 0.32
}

/// How tall the keep's flagpole stands, in tiles.
const KEEP_POLE: f32 = 0.36;

/// How big the leader's crown is drawn, in tiles (the sprite is square,
/// the crown in it a little smaller).
const CROWN_SIZE: f32 = 0.5;

/// The crown floating over a leading castle, by the tile the castle
/// stands on, and how long it has been up, for its arrival.
#[derive(Component)]
pub struct CastleCrown {
    x: u8,
    y: u8,
    age: f32,
}

/// What `crown_the_leader` reads: who sits at the table, and how.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Table<'w> {
    screen: Res<'w, State<crate::app::Screen>>,
    seats: Res<'w, crate::app::Seats>,
    online: Res<'w, crate::app::net::Online>,
    settings: Res<'w, crate::app::settings::GameSettings>,
    time: Res<'w, Time>,
}

/// Float a crown over the castle of whoever leads, so the board itself
/// says who is winning: a judge watching the beach could not tell without
/// looking across at the scores.
///
/// The leader is the score panel's own (`seating::leading_seats`): the one
/// top scorer, or every castle of the leading team, and nobody at zero or
/// in a tie, so the board's crown and the panel's never disagree. Versus
/// only: a puzzle has no one to lead. A new leader's crown drops in with a
/// little bounce, held still under reduced motion; it follows its castle
/// through a swap's flight.
pub fn crown_the_leader(
    mut commands: Commands,
    sim: Res<Sim>,
    art: Res<Art>,
    table: Table,
    castles: Query<(&CastleSprite, &Transform), Without<CastleCrown>>,
    mut crowns: Query<(Entity, &mut CastleCrown, &mut Transform), Without<CastleSprite>>,
) {
    let board = &sim.0;
    let versus = *table.screen.get() == crate::app::Screen::Versus;
    let mode = crate::app::teams::in_play(&table.settings, &table.online, table.seats.0);
    let leaders = crate::app::seating::leading_seats(board.scores(), table.seats.0, mode);
    let leading = |c: &CastleSprite| versus && leaders[usize::from(c.owner)];
    // A crown whose castle no longer leads, or is gone, goes.
    for (entity, crown, _) in &crowns {
        let kept = castles
            .iter()
            .any(|(c, _)| c.x == crown.x && c.y == crown.y && leading(c));
        if !kept {
            commands.entity(entity).despawn();
        }
    }
    let calm = table.settings.reduced_motion;
    let dt = table.time.delta_secs();
    let now = table.time.elapsed_secs();
    for (castle, place) in &castles {
        if !leading(castle) {
            continue;
        }
        let over = place.translation.truncate()
            + Vec2::new(0.0, crown_lift(castle.tier) * TILE * place.scale.y);
        let found = crowns
            .iter_mut()
            .find(|(_, crown, _)| crown.x == castle.x && crown.y == castle.y);
        let Some((_, mut crown, mut at)) = found else {
            commands.spawn((
                super::BoardStatic,
                CastleCrown {
                    x: castle.x,
                    y: castle.y,
                    age: 0.0,
                },
                image_sprite(&art.crown, Color::WHITE, Vec2::splat(CROWN_SIZE * TILE)),
                Transform::from_translation(over.extend(z::TILE_FEATURE + 0.6))
                    .with_scale(Vec3::splat(if calm { 1.0 } else { 0.0 })),
            ));
            continue;
        };
        crown.age += dt;
        let (drop, grow) = if calm { (0.0, 1.0) } else { arrival(crown.age) };
        // A slow breath once it has landed, so it reads as floating.
        let bob = if calm { 0.0 } else { (now * 2.2).sin() * 1.5 };
        at.translation = (over + Vec2::new(0.0, drop * TILE + bob)).extend(z::TILE_FEATURE + 0.6);
        at.scale = Vec3::splat(grow);
    }
}

/// A new leader's crown arriving `age` seconds in: how far above its
/// resting place it is, in tiles, and how big, dropping in over a third of
/// a second and settling with a small bounce.
fn arrival(age: f32) -> (f32, f32) {
    const DROP: f32 = 0.35;
    let t = (age / DROP).clamp(0.0, 1.0);
    // Ease-out-back: overshoots a touch, then settles.
    let back = 1.0 + 2.7 * (t - 1.0).powi(3) + 1.7 * (t - 1.0).powi(2);
    ((1.0 - t) * 0.4, back)
}

/// A castle's flag; waves gently in the sea breeze.
#[derive(Component)]
pub struct Pennant;

/// A scale wobble on a castle when a crab banks there (the original's
/// rocket shudder). Set to 1.0 on a bank, decays to rest.
#[derive(Component)]
pub struct CastleKick(pub f32);

/// How long the castles are in the air when the tide swaps them.
const FLIGHT: f32 = 1.1;

/// How much bigger a castle looks at the top of its flight. The path is a
/// straight line, so this is the only thing saying it is off the ground
/// rather than sliding along the sand.
const RISE: f32 = 0.30;

/// The swap animation: how long is left, and where each castle is flying
/// from.
///
/// A resource and not a component because a castle changing hands is
/// rebuilt by [`sync_castles`] mid-flight (new owner, new colour, often a
/// new tier), and a component would go down with the entity it was on.
#[derive(Resource, Default)]
pub struct CastleFlight {
    left: f32,
    /// Where each castle in the air set off from. Empty when none is.
    from: Vec<Flight>,
}

/// One castle's journey: the tile it is landing on, and where it left.
pub struct Flight {
    tile: (u8, u8),
    from: Vec2,
}

/// The swap detector's memory: the castles as they stood last frame, and
/// which board they stood on.
///
/// The board is what tells a new layout from a swap. Without it a match
/// that ended on a CastleSwap leaves the swapped layout here, and the next
/// round on the same map reads as a swap on its first frame; a round
/// pasted in mid-clock did the same, since its clock had not rolled back.
#[derive(Default)]
pub struct Detector {
    /// The [`BoardGeneration`] the castles in `held` were read off.
    ///
    /// [`BoardGeneration`]: crate::app::sim_events::BoardGeneration
    board: u64,
    held: Vec<Held>,
}

/// A castle as the swap detector sees it: whose it is, and where.
///
/// Named because the comparison that spots a swap is about which of these
/// changed and which did not, and in `(u8, u8, u8)` the two coordinates and
/// the owner are the same type.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Held {
    x: u8,
    y: u8,
    owner: u8,
}

/// Where a castle is, and how big it looks, `progress` of the way from
/// `from` to `home`.
///
/// Split out because it has to start exactly on one tile and finish exactly
/// on the other, or a castle lands off its own sand.
fn hop(from: Vec2, home: Vec2, progress: f32) -> (Vec2, f32) {
    // A straight line between the two tiles, eased out of the launch and
    // into the landing so it reads as a thing setting off and arriving
    // rather than a thing being dragged.
    let eased = progress * progress * (3.0 - 2.0 * progress);
    // Up off the sand and back down onto it, in scale alone: zero at both
    // ends, so the castle finishes exactly the size it started.
    let height = (progress * std::f32::consts::PI).sin();
    (from.lerp(home, eased), 1.0 + RISE * height)
}

/// Fly the castles to their new places when the tide swaps them round.
///
/// The sim hands each castle to the next owner where it stands: nothing
/// moves, the colours change. That is right for the round and wrong for
/// the eye, so the render layer says it the other way about: your castle
/// *travels* to where the next one was, over the sand, and lands wearing
/// the colour it took off in.
///
/// The swap is spotted by watching the board rather than by listening for
/// the tide event. The event is raised by the snapshot comparison a tick
/// behind the board it describes, so by the time it arrives a castle asked
/// where it came from answers "here"; watching for the same castles on the
/// same tiles under different owners cannot be mistimed.
///
/// Runs after [`kick_castles`], which also writes scale: a bank landing in
/// the same frame as a swap should not fight the flight for the transform.
///
/// A new board (see [`BoardGeneration`]) empties both the memory and any
/// flight still in the air, so a round that ends mid-swap leaves nothing
/// to land on the next beach. Done here rather than on screen exit because
/// a rematch on the same map is a board swap with no screen change in it.
///
/// [`BoardGeneration`]: crate::app::sim_events::BoardGeneration
pub fn fly_castles(
    time: Res<Time>,
    current: crate::app::sim_events::CurrentBoard,
    settings: Res<crate::app::settings::GameSettings>,
    mut flight: ResMut<CastleFlight>,
    mut before: Local<Detector>,
    // Swapped with the last reading rather than built fresh each frame.
    mut now: Local<Vec<Held>>,
    mut castles: Query<(&CastleSprite, &mut Transform)>,
) {
    let board = &current.sim.0;
    if before.board != current.generation() {
        before.held.clear();
        flight.from.clear();
        flight.left = 0.0;
    }
    before.board = current.generation();
    now.clear();
    now.extend(board.tiles().filter_map(|(x, y, kind)| match kind {
        TileKind::Castle(owner) => Some(Held { x, y, owner }),
        TileKind::Empty
        | TileKind::Rock
        | TileKind::Spawner(_)
        | TileKind::Turnstile { .. }
        | TileKind::Kelp
        | TileKind::Pool => None,
    }));
    if swapped(&before.held, &now) && !settings.reduced_motion {
        flight.from.clear();
        let mut old = before.held.clone();
        for held in now.iter() {
            // An owner with two castles has them paired in board order,
            // which is arbitrary but consistent, and nothing crosses.
            if let Some(at) = old.iter().position(|was| was.owner == held.owner) {
                let was = old.remove(at);
                flight.from.push(Flight {
                    tile: (held.x, held.y),
                    from: layout::tile_center(board, was.x, was.y),
                });
            }
        }
        flight.left = FLIGHT;
    }
    std::mem::swap(&mut before.held, &mut now);
    if flight.left <= 0.0 {
        return;
    }
    flight.left = (flight.left - time.delta_secs()).max(0.0);
    let progress = 1.0 - flight.left / FLIGHT;
    for (sprite, mut transform) in &mut castles {
        let home = layout::tile_center(board, sprite.x, sprite.y);
        let from = flight
            .from
            .iter()
            .find(|flight| flight.tile == (sprite.x, sprite.y))
            .map_or(home, |flight| flight.from);
        let (at, scale) = hop(from, home, progress);
        transform.translation.x = at.x;
        transform.translation.y = at.y;
        transform.scale = Vec3::splat(scale);
    }
    if flight.left == 0.0 {
        // Landed: hand the transform back exactly as it was found, so
        // nothing downstream inherits a fraction of a scale.
        flight.from.clear();
        for (sprite, mut transform) in &mut castles {
            let home = layout::tile_center(board, sprite.x, sprite.y);
            transform.translation = home.extend(transform.translation.z);
            transform.scale = Vec3::ONE;
        }
    }
}

/// Whether the castles changed hands: the same tiles holding the same
/// owners between them, dealt out differently.
///
/// Deliberately narrow: a castle built, a castle lost or a score crossing a
/// tier all change this list too, and only a permutation of who holds what
/// is a swap.
fn swapped(before: &[Held], now: &[Held]) -> bool {
    if before.len() < 2 || before.len() != now.len() || before == now {
        return false;
    }
    let tiles = |list: &[Held]| {
        let mut out: Vec<(u8, u8)> = list.iter().map(|held| (held.x, held.y)).collect();
        out.sort_unstable();
        out
    };
    let owners = |list: &[Held]| {
        let mut out: Vec<u8> = list.iter().map(|held| held.owner).collect();
        out.sort_unstable();
        out
    };
    tiles(before) == tiles(now) && owners(before) == owners(now)
}

/// What each keep banked over one frame's events, added up, as the keep's
/// centre and its total.
///
/// The sim reports a bank per crab, and a castle can take several on one
/// tick: eight, measured over a set of six-seat rounds. [`score_pip`] has
/// no jitter in it, so a pip per crab puts eight identical `+1`s pixel on
/// pixel and the player who banked eight reads one `+1` drawn too dark.
///
/// Summed instead: `+8` is the number that happened, and this is the only
/// place it is ever shown.
///
/// Per keep and not per seat: a seat can hold two castles (some of the
/// shipped puzzles give the first seat two), and summed by owner both
/// of them bounced and floated the pair's total, so a crab walking into
/// one castle was shown arriving at both, at twice its worth.
///
/// [`score_pip`]: crate::app::effects::score_pip
fn gains<'a>(
    events: impl IntoIterator<Item = &'a crate::app::sim_events::SimEvent>,
) -> Vec<(Vec2, i32)> {
    use crate::app::sim_events::SimEvent;
    let mut gained: Vec<(Vec2, i32)> = Vec::new();
    for event in events {
        let SimEvent::CrabBanked { keep, points, .. } = event else {
            continue;
        };
        match gained.iter_mut().find(|(at, _)| same_keep(*at, *keep)) {
            Some((_, total)) => *total = total.saturating_add(*points),
            None => gained.push((*keep, *points)),
        }
    }
    gained
}

/// Whether two board positions name the same keep: a bank's `keep` and a
/// castle sprite's tile centre come out of the same `tile_center`, so
/// anything within a fraction of a tile is the one castle.
fn same_keep(a: Vec2, b: Vec2) -> bool {
    a.distance_squared(b) < (TILE * 0.25).powi(2)
}

/// What the keep at `at` banked this frame, out of [`gains`].
fn gained_at(gained: &[(Vec2, i32)], at: Vec2) -> i32 {
    gained
        .iter()
        .find(|(keep, _)| same_keep(*keep, at))
        .map_or(0, |(_, points)| *points)
}

/// Bounce a castle on every gain and float the points its banks made (or
/// cost) over the keep, in the owner's colour.
pub fn kick_castles(
    mut commands: Commands,
    mut events: MessageReader<crate::app::sim_events::SimEvent>,
    sim: Res<Sim>,
    time: Res<Time>,
    settings: Res<crate::app::settings::GameSettings>,
    mut castles: Query<(&CastleSprite, &mut CastleKick, &mut Transform)>,
) {
    let board = &sim.0;
    // Reduced motion keeps the floating points and drops the bounce.
    let calm = settings.reduced_motion;
    let gained = gains(events.read());
    for (sprite, mut kick, _) in &mut castles {
        // A stranded sprite (off this board) is `sync_castles`'s to
        // remove; it has no tile to read.
        if !on_board(board, sprite.x, sprite.y) {
            continue;
        }
        let TileKind::Castle(owner) = board.tile_at(sprite.x, sprite.y) else {
            continue;
        };
        let at = layout::tile_center(board, sprite.x, sprite.y);
        let points = gained_at(&gained, at);
        if points == 0 {
            continue;
        }
        // A bounce is a celebration; a castle that just lost points to a
        // left claw shows the loss and stays put.
        if points > 0 {
            kick.0 = if calm { 0.0 } else { 1.0 };
        }
        crate::app::effects::score_pip(
            &mut commands,
            // Signed: a left claw banked under a Right Claws call costs.
            format!("{points:+}"),
            at + Vec2::new(0.0, TILE * 0.35),
            palette::player_color(owner).lighter(0.15),
        );
    }
    let dt = time.delta_secs();
    for (_, mut kick, mut transform) in &mut castles {
        if kick.0 <= 0.0 {
            continue;
        }
        kick.0 = (kick.0 - dt * 3.2).max(0.0);
        // A quick squash-and-settle: overshoot then ease back.
        let wobble = 1.0 + 0.20 * kick.0 * (kick.0 * std::f32::consts::PI * 2.0).sin().abs();
        transform.scale = Vec3::splat(wobble);
    }
}

/// A castle that just grew says so.
///
/// [`castle_tier`] is the whole scoreboard (spec §3.4), and without this a
/// castle crossing a threshold changes shape between one frame and the next
/// with nothing to mark it.
///
/// Runs after [`sync_castles`], and must: the sprite being cheered is the
/// one built at the *new* tier, and the events reaching this frame are the
/// previous frame's (see the `Frame` sets), so it is already standing.
pub fn cheer_tier_ups(
    fx: crate::app::effects::Fx,
    mut events: MessageReader<crate::app::sim_events::SimEvent>,
    sim: Res<Sim>,
    mut trauma: ResMut<crate::app::effects::Trauma>,
    mut castles: Query<(&CastleSprite, &mut CastleKick)>,
) {
    use crate::app::effects::{Burst, Fx, burst, ring};
    let Fx {
        mut commands,
        art,
        mut rng,
        settings,
    } = fx;
    use crate::app::sim_events::SimEvent;
    let board = &sim.0;
    for event in events.read() {
        let SimEvent::TierUp { owner } = event else {
            continue;
        };
        // The growth itself is the news and it survives reduced motion;
        // only the fireworks over it stand down.
        if settings.reduced_motion {
            continue;
        }
        for (sprite, mut kick) in &mut castles {
            if sprite.owner != *owner || !on_board(board, sprite.x, sprite.y) {
                continue;
            }
            let at = layout::tile_center(board, sprite.x, sprite.y);
            let color = palette::player_color(*owner);
            ring(
                &mut commands,
                &art,
                at,
                color.lighter(0.25),
                TILE * 0.8,
                2.6,
                0.55,
            );
            burst(
                &mut commands,
                &mut rng,
                &Burst {
                    image: art.puff.clone(),
                    pos: at,
                    color: Color::srgba(0.95, 0.9, 0.76, 0.9),
                    count: 9,
                    size: 15.0,
                    speed: 62.0,
                    gravity: 55.0,
                },
            );
            // Grains of the new work trickling off its walls: small, quick
            // to fall, in the castle's own dyed sand, out of its top.
            burst(
                &mut commands,
                &mut rng,
                &Burst {
                    image: art.puff.clone(),
                    pos: at + Vec2::new(0.0, TILE * 0.25),
                    color: castle_body(color),
                    count: 12,
                    size: 5.0,
                    speed: 70.0,
                    gravity: 170.0,
                },
            );
            for _ in 0..4 {
                crate::app::effects::glint(&mut commands, &mut rng, &art, at, color.lighter(0.4));
            }
            kick.0 = 1.0;
        }
        trauma.add(0.2);
    }
}

/// Flutter every pennant: a small shear-like x-scale ripple plus a slight
/// tilt, phased per entity so flags do not wave in lockstep.
pub fn wave_pennants(time: Res<Time>, mut flags: Query<(Entity, &mut Transform), With<Pennant>>) {
    let t = time.elapsed_secs();
    for (entity, mut transform) in &mut flags {
        let phase = f32::from(entity.to_bits() as u16 % 17);
        transform.scale.x = 1.0 + (t * 5.0 + phase).sin() * 0.12;
        transform.rotation = Quat::from_rotation_z((t * 3.0 + phase).sin() * 0.08);
    }
}

#[cfg(test)]
mod tests {
    use super::{castle_body, castle_parts, gained_at, gains, hop, keep, turrets};

    /// A new leader's crown starts small and above its place and comes to
    /// rest at full size exactly on it, and it floats clear of the keep's
    /// flag at every tier.
    #[test]
    fn the_crown_lands_on_its_place_above_the_flag() {
        use super::{CROWN_SIZE, KEEP_POLE, TOWER_TOP, arrival, crown_lift};
        let (drop, grow) = arrival(0.0);
        assert!(
            drop > 0.0 && grow < 0.05,
            "arrives from above, small: {drop} {grow}"
        );
        assert_eq!(
            arrival(1.0),
            (0.0, 1.0),
            "and rests at full size on its place"
        );
        for tier in 0..=3 {
            let keep = keep(tier);
            let pole_top = keep.at.y + keep.size * TOWER_TOP + KEEP_POLE;
            let crown_foot = crown_lift(tier) - CROWN_SIZE * 0.5 * 0.7;
            assert!(
                crown_foot > pole_top - 0.02,
                "tier {tier}: crown on the flag"
            );
        }
    }

    /// A castle swelling with a bank leaves its shadows lying where they
    /// were, at their own size: the ground does not swell.
    #[test]
    fn a_swelling_castle_leaves_its_shadow_on_the_ground() {
        use super::{CastleShadow, CastleSprite, steady_castle_shadows};
        use bevy::prelude::*;
        let mut app = App::new();
        let at = Vec3::new(15.0, -21.0, -0.41);
        let castle = app
            .world_mut()
            .spawn((
                CastleSprite {
                    x: 0,
                    y: 0,
                    tier: 3,
                    owner: 0,
                },
                Transform::from_scale(Vec3::splat(1.25)),
            ))
            .id();
        let shadow = app
            .world_mut()
            .spawn((
                CastleShadow { at },
                Transform::from_translation(at),
                ChildOf(castle),
            ))
            .id();
        app.add_systems(Update, steady_castle_shadows);
        app.update();
        let place = app.world().get::<Transform>(shadow).expect("the shadow");
        let drawn = place.translation.truncate() * 1.25;
        assert!(
            (drawn - at.truncate()).length() < 1e-4,
            "it lunged to {drawn}"
        );
        assert!(
            (place.scale.x * 1.25 - 1.0).abs() < 1e-4,
            "it grew to {}",
            place.scale.x
        );
    }
    use crate::app::art::Art;
    use crate::app::palette;

    fn parts(tier: u8) -> Vec<super::CastlePart> {
        castle_parts(&Art::blank(), tier, palette::player_color(0))
    }

    /// Every tier draws more than the last: the scoreboard only grows.
    #[test]
    fn each_tier_draws_more_than_the_last() {
        let counts: Vec<usize> = (0..=3).map(|tier| parts(tier).len()).collect();
        assert!(counts.windows(2).all(|w| w[0] < w[1]), "{counts:?}");
    }

    /// The moat is the top tier's, and only the top tier's: it is the one
    /// piece drawn wider than the tile.
    #[test]
    fn only_the_top_tier_digs_a_moat() {
        let moats = |tier| parts(tier).iter().filter(|p| p.size.x > 1.0).count();
        assert_eq!([moats(0), moats(1), moats(2), moats(3)], [0, 0, 0, 1]);
    }

    /// Every tower flies a pennant of its own, and the keep flies one more.
    #[test]
    fn every_tower_flies_a_pennant() {
        for tier in 0..=3 {
            let pennants = parts(tier).iter().filter(|p| p.pennant).count();
            assert_eq!(pennants, turrets(tier).len() + 1, "tier {tier}");
        }
    }

    /// The parts come back to front: the order the results card draws them
    /// in, which UI takes from the order they are spawned.
    #[test]
    fn a_castle_comes_back_to_front() {
        for tier in 0..=3 {
            let depths: Vec<f32> = parts(tier).iter().map(|p| p.z).collect();
            assert!(
                depths.windows(2).all(|w| w[0] <= w[1]),
                "tier {tier}: {depths:?}"
            );
        }
    }

    /// A castle is sand dyed toward its owner, not the owner's colour
    /// outright, and the dye still keeps any two owners apart.
    #[test]
    fn a_castle_is_dyed_sand() {
        let bodies: Vec<Color> = (0..6)
            .map(|seat| castle_body(palette::player_color(seat)))
            .collect();
        for (seat, body) in bodies.iter().enumerate() {
            assert_ne!(
                *body,
                palette::player_color(seat as u8),
                "seat {seat} is raw colour"
            );
            for other in &bodies[seat + 1..] {
                assert_ne!(body, other, "two owners dye alike");
            }
        }
    }
    use crate::app::sim_events::SimEvent;
    use crate::sim::CrabKind;
    use bevy::prelude::*;

    /// A castle only ever gains pieces: every tier keeps the towers the
    /// last one had, going none, none, two, four, and the top tier's keep
    /// stands taller than a bare one. (Tiers 1 and 2 step the keep down a
    /// little, to fit it inside the wall they add.)
    #[test]
    fn every_tier_adds_to_the_castle() {
        let towers: Vec<usize> = (0..=3).map(|tier| turrets(tier).len()).collect();
        assert_eq!(towers, [0, 0, 2, 4]);
        for tier in 1..=3 {
            let before: Vec<_> = turrets(tier - 1).iter().map(|t| t.at).collect();
            let now: Vec<_> = turrets(tier).iter().map(|t| t.at).collect();
            assert!(
                before.iter().all(|at| now.contains(at)),
                "tier {tier} lost a tower"
            );
        }
        assert!(
            keep(3).size > keep(0).size,
            "the top tier's keep stands taller"
        );
    }

    /// A crab worth `points` walking into the keep at `keep`.
    fn bank(owner: u8, keep: Vec2, points: i32) -> SimEvent {
        SimEvent::CrabBanked {
            id: 0,
            owner,
            pos: Vec2::ZERO,
            keep,
            points,
            kind: CrabKind::Common,
            handed: crate::sim::Handedness::Right,
        }
    }

    /// The floating number is the only place a bank's worth is ever shown,
    /// so on a frame that banked eight it has to say eight.
    ///
    /// Said as `+1` eight times it lands in the same place at the same
    /// speed, since `score_pip` has no jitter: the copies rise as one and
    /// the number is simply wrong, drawn a little too dark.
    #[test]
    fn a_frame_of_banks_floats_one_number_and_it_is_the_total() {
        let keep = Vec2::new(64.0, -32.0);
        assert!(gains([]).is_empty(), "a quiet frame gains nobody");
        assert_eq!(gained_at(&gains(&[bank(1, keep, 3)]), keep), 3);
        let stream: Vec<SimEvent> = (0..8).map(|_| bank(1, keep, 1)).collect();
        assert_eq!(
            gained_at(&gains(&stream), keep),
            8,
            "eight into one keep is eight, not eight ones"
        );
        // Crabs are not all worth the same, so this is a sum and not a
        // count: a golden one is fifty walking.
        let golden = gains(&[bank(2, keep, 50), bank(2, keep, 1)]);
        assert_eq!(gained_at(&golden, keep), 51);
    }

    /// Keeps are added up apart, the two of one seat's included. A puzzle
    /// that gives the first seat two castles floated the pair's total over
    /// both of them, as if every crab had walked into each.
    #[test]
    fn every_keep_is_counted_on_its_own_even_two_of_one_seat() {
        let (left, right, far) = (
            Vec2::new(-200.0, 0.0),
            Vec2::new(200.0, 0.0),
            Vec2::new(0.0, 150.0),
        );
        let frame = vec![
            bank(0, left, 1),
            bank(0, right, 2),
            bank(0, left, 4),
            bank(3, far, 1),
        ];
        let gained = gains(&frame);
        assert_eq!(gained_at(&gained, left), 5, "two crabs home on the left");
        assert_eq!(gained_at(&gained, right), 2, "and one on the right");
        assert_eq!(gained_at(&gained, far), 1, "a rival's keep is its own");
        assert_eq!(
            gained_at(&gained, Vec2::new(0.0, -150.0)),
            0,
            "and a keep that banked nothing floats nothing"
        );
        let total: i32 = gained.iter().map(|(_, points)| points).sum();
        assert_eq!(total, 8, "nothing counted twice");
    }

    /// The two ends are the point: a castle that lands a few pixels off
    /// its tile stays there until something else redraws it.
    #[test]
    fn a_hop_starts_and_finishes_on_the_sand() {
        let from = Vec2::new(-100.0, 40.0);
        let home = Vec2::new(220.0, -60.0);
        let (start, start_scale) = hop(from, home, 0.0);
        assert_eq!(start, from);
        assert!((start_scale - 1.0).abs() < 1e-6);
        let (end, end_scale) = hop(from, home, 1.0);
        assert!(end.distance(home) < 1e-3, "{end:?} vs {home:?}");
        assert!((end_scale - 1.0).abs() < 1e-6);
    }

    /// Only a permutation is a swap. A castle built, lost, or crossing a
    /// tier changes the list too, and animating those as flights would
    /// have castles sliding in from wherever the last one happened to be.
    #[test]
    fn only_a_reshuffle_counts_as_a_swap() {
        let held = |x, y, owner| super::Held { x, y, owner };
        let a = vec![held(1, 1, 0), held(5, 1, 1)];
        let swapped_pair = vec![held(1, 1, 1), held(5, 1, 0)];
        assert!(super::swapped(&a, &swapped_pair));
        assert!(!super::swapped(&a, &a), "nothing moved");
        assert!(!super::swapped(&[], &a), "the first frame is not a swap");
        assert!(
            !super::swapped(&a, &[held(1, 1, 0), held(5, 1, 1), held(9, 1, 2)]),
            "a castle was built"
        );
        assert!(
            !super::swapped(&a, &[held(1, 1, 0), held(7, 1, 1)]),
            "a castle moved tile, which the sim never does"
        );
        assert!(
            !super::swapped(&a, &[held(1, 1, 0), held(5, 1, 2)]),
            "a different owner turned up"
        );
        assert!(
            !super::swapped(&[held(1, 1, 0)], &[held(1, 1, 1)]),
            "one castle cannot swap with itself"
        );
    }

    /// Straight between the two tiles, never off the line: a castle that
    /// bowed upwards would fly over the wall on a corner-to-corner swap.
    #[test]
    fn a_hop_travels_in_a_straight_line() {
        let from = Vec2::new(0.0, 0.0);
        let home = Vec2::new(400.0, 200.0);
        for step in 0..=10 {
            let progress = step as f32 / 10.0;
            let (at, _) = hop(from, home, progress);
            // Every point is on the segment: y is exactly half of x, which
            // is what the two ends say it should be.
            assert!((at.y - at.x * 0.5).abs() < 1e-3, "{at:?} at {progress}");
        }
        // And it is off the ground in between, which is the only thing
        // saying it is flying rather than sliding.
        assert!(hop(from, home, 0.5).1 > 1.0);
    }
}
