//! Castles: their tier-scaled keep, the pennant on top, and the shudder
//! and floating points a bank sets off.

use super::{WALL_COLOR, image_sprite, on_board, z};
use crate::app::Sim;
use crate::app::art::Art;
use crate::app::layout::{self, TILE};
use crate::app::palette;
use crate::sim::{TileKind, castle_tier};
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
    /// tier alone the sprite was left standing in the old owner's colour
    /// until the new one happened to score across a threshold.
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
        let idx = sprite.y as usize * board.width() as usize + sprite.x as usize;
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
        let idx = y as usize * board.width() as usize + x as usize;
        if covered[idx] {
            continue;
        }
        let TileKind::Castle(owner) = kind else {
            continue;
        };
        let tier = castle_tier(board.scores()[owner as usize]);
        let color = palette::player_color(owner);
        let pos = layout::tile_center(board, x, y);
        let base_size = TILE * (0.55 + 0.12 * f32::from(tier));
        commands
            .spawn((
                CastleSprite { x, y, tier, owner },
                CastleKick(0.0),
                image_sprite(&art.castle, color, Vec2::splat(base_size * 1.12)),
                Transform::from_translation(pos.extend(z::TILE_FEATURE + 0.2)),
            ))
            .with_children(|parent| {
                // The keep sits on the sand rather than in it.
                parent.spawn((
                    image_sprite(
                        &art.shadow,
                        Color::srgba(1.0, 1.0, 1.0, 0.7),
                        Vec2::splat(base_size * 1.3),
                    ),
                    Transform::from_translation(layout::SUN.extend(-0.3)),
                ));
                if tier >= 1 {
                    // The curtain wall the first tier throws up: a
                    // battlemented ring, hollow, with the keep inside it.
                    // It was a plain coloured square until the castles
                    // started being looked at, which is a shame for the
                    // one sprite on the board that *is* the scoreboard.
                    parent.spawn((
                        image_sprite(&art.keep_ring, color.darker(0.12), Vec2::splat(TILE * 0.99)),
                        Transform::from_translation(Vec3::new(0.0, 0.0, -0.1)),
                    ));
                }
                if tier >= 2 {
                    // Bucket-moulded corner towers on the front wall.
                    for side in [-1.0, 1.0] {
                        parent.spawn((
                            image_sprite(
                                &art.turret,
                                color.lighter(0.10),
                                Vec2::splat(TILE * 0.28),
                            ),
                            Transform::from_translation(Vec3::new(
                                side * base_size * 0.42,
                                base_size * 0.42,
                                0.1,
                            )),
                        ));
                    }
                }
                if tier >= 3 {
                    // The moat, dug outermost: water with its own ripples,
                    // open in the middle where the keep stands.
                    parent.spawn((
                        image_sprite(&art.moat, Color::WHITE, Vec2::splat(TILE * 1.06)),
                        Transform::from_translation(Vec3::new(0.0, 0.0, -0.2)),
                    ));
                }
                // Pennant on top; every tier flies the flag.
                // The flag (spec §3.4: flying their colour flag): a
                // driftwood pole with a bright owner-coloured pennant,
                // sticking up past the keep so it reads at a glance.
                let pole_height = TILE * 0.34;
                let pole_top = base_size * 0.5 + pole_height;
                parent.spawn((
                    Sprite::from_color(WALL_COLOR, Vec2::new(2.5, pole_height)),
                    Transform::from_translation(Vec3::new(
                        -TILE * 0.06,
                        base_size * 0.5 + pole_height / 2.0,
                        0.3,
                    )),
                ));
                parent.spawn((
                    Pennant,
                    Sprite::from_color(color.lighter(0.22), Vec2::new(TILE * 0.20, TILE * 0.12)),
                    Transform::from_translation(Vec3::new(
                        -TILE * 0.06 + TILE * 0.11,
                        pole_top - TILE * 0.05,
                        0.3,
                    )),
                ));
            });
    }
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
/// straight line, so this is the only thing saying it is off the ground,
/// and it is worth having: without it the castles read as sliding along
/// the sand rather than crossing over it.
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
/// The clock is what tells a new board from the old one. Without it the
/// memory outlived the round: a match that ended on a CastleSwap left the
/// swapped layout here, and the next round on the same map, whose castles
/// stand the other way about, read as a swap on its first frame and flew
/// them all from where the last round left them.
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
/// changed and which did not, and `(u8, u8, u8)` says none of that: the
/// two coordinates and the owner are the same type and read the same way
/// round.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Held {
    x: u8,
    y: u8,
    owner: u8,
}

/// Where a castle is, and how big it looks, `progress` of the way from
/// `from` to `home`.
///
/// Split out because it is the whole animation and the only part of it
/// worth testing: it has to start exactly on one tile and finish exactly
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
/// the eye, which sees the castles blink into other colours and has to
/// work out what happened. So the render layer says it the other way
/// about: your castle *travels* to where the next one was, over the sand,
/// and lands wearing the colour it took off in.
///
/// The swap is spotted by watching the board rather than by listening for
/// the tide event, and that is the whole trick. The event is raised by the
/// snapshot comparison a tick behind the board it describes, so by the
/// time it arrives the layout it was announcing is already the current
/// one, and a castle asked where it came from answers "here". Watching for
/// the same castles on the same tiles under different owners cannot be
/// mistimed, because it *is* the thing being animated.
///
/// Runs after [`kick_castles`], which also writes scale: a bank landing in
/// the same frame as a swap should not fight the flight for the transform.
///
/// A fresh board (see [`Detector::is_fresh`]) empties both the memory and
/// any flight still in the air, so a round that ends mid-swap, or a screen
/// left mid-swap, leaves nothing to land on the next beach. Done here
/// rather than on screen exit because every board swap must do it, not
/// only the ones that pass through a screen change (a rematch on the
/// same map does not).
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
/// Deliberately narrow. A castle built, a castle lost or a score crossing
/// a tier all change this list too, and none of them is a swap; only a
/// permutation of who holds what is.
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

/// Bounce the owner's castle on every bank and float the points gained
/// over the keep, in the owner's colour.
pub fn kick_castles(
    mut commands: Commands,
    mut events: MessageReader<crate::app::sim_events::SimEvent>,
    sim: Res<Sim>,
    time: Res<Time>,
    settings: Res<crate::app::settings::GameSettings>,
    mut castles: Query<(&CastleSprite, &mut CastleKick, &mut Transform)>,
) {
    use crate::app::sim_events::SimEvent;
    let board = &sim.0;
    // Reduced motion keeps the floating points and drops the bounce.
    let calm = settings.reduced_motion;
    for event in events.read() {
        let SimEvent::CrabBanked { owner, value, .. } = event else {
            continue;
        };
        for (sprite, mut kick, _) in &mut castles {
            // A stranded sprite (off this board) is `sync_castles`'s to
            // remove; it has no tile to read.
            if on_board(board, sprite.x, sprite.y)
                && board.tile_at(sprite.x, sprite.y) == TileKind::Castle(*owner)
            {
                kick.0 = if calm { 0.0 } else { 1.0 };
                crate::app::effects::score_pip(
                    &mut commands,
                    format!("+{value}"),
                    layout::tile_center(board, sprite.x, sprite.y) + Vec2::new(0.0, TILE * 0.35),
                    palette::player_color(*owner).lighter(0.15),
                );
            }
        }
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
/// [`crate::sim::castle_tier`] is the whole scoreboard (spec §3.4), and
/// until this existed a castle crossing a threshold changed shape between
/// one frame and the next with nothing to mark it: the loudest good news a
/// player gets, delivered by a sprite swap. `TierUp` was one of the events
/// the effects layer read and threw away.
///
/// Runs after [`sync_castles`], and must: the sprite being cheered is the
/// one built at the *new* tier, and the events reaching this frame are the
/// previous frame's (see the `Frame` sets), so it is already standing.
#[allow(clippy::too_many_arguments)]
pub fn cheer_tier_ups(
    mut commands: Commands,
    mut events: MessageReader<crate::app::sim_events::SimEvent>,
    sim: Res<Sim>,
    art: Res<Art>,
    settings: Res<crate::app::settings::GameSettings>,
    mut rng: ResMut<crate::app::effects::VisualRng>,
    mut trauma: ResMut<crate::app::effects::Trauma>,
    mut castles: Query<(&CastleSprite, &mut CastleKick)>,
) {
    use crate::app::effects::{Burst, burst, ring};
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
    use super::hop;
    use bevy::prelude::*;

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
