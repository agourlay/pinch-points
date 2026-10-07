//! Castles: their tier-scaled keep, the pennant on top, and the shudder
//! and floating points a bank sets off.

use super::{WALL_COLOR, image_sprite, on_board, z};
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

pub fn sync_castles(
    mut commands: Commands,
    sim: Res<Sim>,
    art: Res<Art>,
    mut covered: Local<Vec<bool>>,
    existing: Query<(Entity, &CastleSprite)>,
) {
    let board = &sim.0;
    covered.clear();
    covered.resize(board.width() as usize * board.height() as usize, false);
    for (entity, sprite) in &existing {
        if !on_board(board, sprite.x, sprite.y) {
            commands.entity(entity).despawn();
            continue;
        }
        let idx = usize::from(board.index_of(sprite.x, sprite.y));
        match board.tile_at(sprite.x, sprite.y) {
            TileKind::Castle(owner)
                if owner == sprite.owner
                    && castle_tier(board.scores()[owner as usize]) == sprite.tier =>
            {
                covered[idx] = true;
            }
            TileKind::Castle(_)
            | TileKind::Empty
            | TileKind::Rock
            | TileKind::Spawner(_)
            | TileKind::Turnstile { .. }
            | TileKind::Kelp
            | TileKind::Pool => {
                commands.entity(entity).despawn();
            }
        }
    }
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
        image_sprite(
            &art.shadow,
            Color::srgba(1.0, 1.0, 1.0, 0.7),
            Vec2::splat(TILE * footprint),
        ),
        Transform::from_translation(layout::SUN.extend(-0.4)),
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
        0.36,
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
/// the board clock they stood on.
///
/// The clock is what tells a new board from the old one. Without it a match
/// that ended on a CastleSwap leaves the swapped layout here, and the next
/// round on the same map reads as a swap on its first frame.
#[derive(Default)]
pub struct Detector {
    ticks: u64,
    held: Vec<Held>,
}

impl Detector {
    /// Whether `board` is a different board from the one remembered: the
    /// clock rolled back (a fresh round or level, whose clock starts at 0)
    /// or the beach changed size under it. Either way what is remembered
    /// says nothing about this board.
    fn is_fresh(&self, board: &crate::sim::Board) -> bool {
        board.ticks() < self.ticks
            || self
                .held
                .iter()
                .any(|held| !on_board(board, held.x, held.y))
    }
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
/// A fresh board (see [`Detector::is_fresh`]) empties both the memory and
/// any flight still in the air, so a round that ends mid-swap leaves
/// nothing to land on the next beach. Done here rather than on screen exit
/// because a rematch on the same map is a board swap with no screen change
/// in it.
pub fn fly_castles(
    time: Res<Time>,
    sim: Res<Sim>,
    settings: Res<crate::app::settings::GameSettings>,
    mut flight: ResMut<CastleFlight>,
    mut before: Local<Detector>,
    // Swapped with the last reading rather than built fresh each frame.
    mut now: Local<Vec<Held>>,
    mut castles: Query<(&CastleSprite, &mut Transform)>,
) {
    let board = &sim.0;
    if before.is_fresh(board) {
        before.held.clear();
        flight.from.clear();
        flight.left = 0.0;
    }
    before.ticks = board.ticks();
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

/// What each seat banked over one frame's events, added up.
///
/// The sim reports a bank per crab, and a castle can take several on one
/// tick: eight, measured over a set of six-seat rounds. [`score_pip`] has
/// no jitter in it, so a pip per crab puts eight identical `+1`s pixel on
/// pixel and the player who banked eight reads one `+1` drawn too dark.
///
/// Summed instead: `+8` is the number that happened, and this is the only
/// place it is ever shown.
///
/// [`score_pip`]: crate::app::effects::score_pip
fn gains<'a>(
    events: impl IntoIterator<Item = &'a crate::app::sim_events::SimEvent>,
) -> [i32; crate::sim::MAX_PLAYERS] {
    use crate::app::sim_events::SimEvent;
    let mut gained = [0i32; crate::sim::MAX_PLAYERS];
    for event in events {
        if let SimEvent::CrabBanked { owner, points, .. } = event
            && let Some(total) = gained.get_mut(usize::from(*owner))
        {
            *total = total.saturating_add(*points);
        }
    }
    gained
}

/// Bounce the owner's castle on every gain and float the points the banks
/// made (or cost) over the keep, in the owner's colour.
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
        let Some(&points) = gained.get(usize::from(owner)) else {
            continue;
        };
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
            layout::tile_center(board, sprite.x, sprite.y) + Vec2::new(0.0, TILE * 0.35),
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
    use super::{castle_body, castle_parts, gains, hop, keep, turrets};
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
    use crate::sim::{CrabKind, MAX_PLAYERS};
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

    /// A crab worth `points` walking into `owner`'s keep.
    fn bank(owner: u8, points: i32) -> SimEvent {
        SimEvent::CrabBanked {
            id: 0,
            owner,
            pos: Vec2::ZERO,
            keep: Vec2::ZERO,
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
        assert_eq!(gains([]), [0; MAX_PLAYERS], "a quiet frame gains nobody");
        assert_eq!(gains(&[bank(1, 3)])[1], 3, "one crab is worth its own");
        let stream: Vec<SimEvent> = (0..8).map(|_| bank(1, 1)).collect();
        assert_eq!(
            gains(&stream)[1],
            8,
            "eight into one keep is eight, not eight ones"
        );
        // Crabs are not all worth the same, so this is a sum and not a
        // count: a golden one is fifty walking.
        assert_eq!(gains(&[bank(2, 50), bank(2, 1)])[2], 51);
    }

    /// Seats are added up apart. Six castles filling on one tick is six
    /// numbers over six keeps, and each of them is that keep's own.
    #[test]
    fn every_seat_is_counted_over_its_own_keep() {
        let frame: Vec<SimEvent> = vec![bank(0, 1), bank(3, 2), bank(0, 4), bank(3, 1)];
        let gained = gains(&frame);
        assert_eq!(gained[0], 5, "two crabs home");
        assert_eq!(gained[3], 3);
        assert_eq!(
            gained[1], 0,
            "and a seat that banked nothing floats nothing"
        );
        assert_eq!(gained.iter().sum::<i32>(), 8, "nothing counted twice");
        // A seat past the end of the table is dropped rather than indexed
        // off it: the owner rides in on an observed event, and every other
        // reader of one is careful about that.
        let stray = gains(&[bank(MAX_PLAYERS as u8, 9), bank(u8::MAX, 9)]);
        assert_eq!(stray, [0; MAX_PLAYERS], "no seat, no number");
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

    /// A round that ends on a swap must not fly the next round's castles
    /// from where the last one left them: a board whose clock rolled
    /// back, or that shrank under the remembered castles, is a fresh one.
    #[test]
    fn a_new_round_forgets_the_last_layout() {
        let held = |x, y, owner| super::Held { x, y, owner };
        let mut board = crate::sim::Board::new(12, 9, 1);
        for _ in 0..50 {
            board.tick_idle();
        }
        let detector = super::Detector {
            ticks: board.ticks(),
            held: vec![held(1, 1, 0), held(10, 7, 1)],
        };
        assert!(!detector.is_fresh(&board), "the same board, ticking on");
        let next_round = crate::sim::Board::new(12, 9, 2);
        assert!(detector.is_fresh(&next_round), "the clock rolled back");
        let mut smaller = crate::sim::Board::new(9, 7, 1);
        for _ in 0..80 {
            smaller.tick_idle();
        }
        assert!(
            detector.is_fresh(&smaller),
            "a castle remembered off this board says the board changed"
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
