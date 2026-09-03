//! Crab sprite lifecycle and render interpolation.
//!
//! One sprite entity per live crab, keyed by the sim's stable crab id. Every
//! frame, transforms are interpolated between the crab's previous and current
//! sub-tile positions using the fixed-timestep overstep fraction (spec §7.4).

use crate::app::art::Art;
use crate::app::layout::{self, TILE};
use crate::app::palette;
use crate::app::{Sim, effects};
use crate::sim::{Crab, CrabKind, Gull, GullState, Handedness};
use bevy::color::Mix;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

#[derive(Component)]
pub struct CrabSprite {
    pub id: u32,
    pub kind: CrabKind,
    /// A per-crab lift or drop on the shell colour, drawn once from the
    /// id. Twenty crabs of one kind used to be twenty copies of one
    /// sprite; this is what turns a queue of them into a crowd. Small
    /// enough that a kind is still read by its colour.
    pub(crate) shade: f32,
}

/// The widest a shell strays from its kind's colour, either way.
const SHADE_SPREAD: f32 = 0.05;

/// One crab's shell: its kind's colour, nudged by its own shade.
///
/// Only the kinds a beach is *crowded* with. The shade is there to stop
/// twenty commons reading as twenty copies of one sprite, and twenty is
/// never how many golden crabs are on the sand: those three carry their
/// worth in their colour, and a golden lightened by a twentieth lands
/// nearer a juvenile's pale orange than its own yellow - fifty points
/// wearing the face of two.
pub(crate) fn shell_color(kind: CrabKind, shade: f32) -> Color {
    let base = body_color(kind);
    match kind {
        CrabKind::Molting | CrabKind::Golden | CrabKind::Sparkling => base,
        CrabKind::Common | CrabKind::Juvenile | CrabKind::Giant => {
            if shade >= 0.0 {
                base.lighter(shade)
            } else {
                base.darker(-shade)
            }
        }
    }
}

/// The shade a crab is born with. A hash of the id rather than a draw from
/// [`effects::VisualRng`], so a crab keeps its shell for as long as it
/// lives however many times the sprite is rebuilt around it.
pub(crate) fn shade_of(id: u32) -> f32 {
    let mixed = id.wrapping_mul(2_654_435_761) >> 16;
    (mixed % 1000) as f32 / 1000.0 * SHADE_SPREAD * 2.0 - SHADE_SPREAD
}

#[derive(Component)]
pub struct GullSprite(pub u32);

/// The soft blob under a creature. Gull shadows detach in flight.
#[derive(Component)]
pub struct CreatureShadow;

const CLAW_LEFT: Color = Color::srgb(0.18, 0.49, 0.82); // blue = left-clawed
const CLAW_RIGHT: Color = Color::srgb(0.97, 0.82, 0.17); // yellow = right-clawed

pub(crate) fn body_color(kind: CrabKind) -> Color {
    match kind {
        CrabKind::Common => Color::srgb(0.91, 0.50, 0.29),
        CrabKind::Juvenile => Color::srgb(0.96, 0.76, 0.42),
        CrabKind::Giant => Color::srgb(0.70, 0.25, 0.17),
        CrabKind::Molting => Color::srgb(0.61, 0.48, 0.82),
        CrabKind::Golden => Color::srgb(1.0, 0.88, 0.20),
        CrabKind::Sparkling => Color::srgb(0.75, 0.95, 1.0),
    }
}

pub(crate) fn body_size(kind: CrabKind) -> Vec2 {
    match kind {
        CrabKind::Giant => Vec2::new(TILE * 0.80, TILE * 0.62),
        CrabKind::Juvenile => Vec2::new(TILE * 0.42, TILE * 0.34),
        CrabKind::Common | CrabKind::Molting | CrabKind::Golden | CrabKind::Sparkling => {
            Vec2::new(TILE * 0.56, TILE * 0.44)
        }
    }
}

/// Spawn sprites for new crabs, despawn sprites for banked ones.
pub fn sync_crab_sprites(
    mut commands: Commands,
    sim: Res<Sim>,
    art: Res<Art>,
    mut live: Local<HashMap<u32, Crab>>,
    existing: Query<(Entity, &CrabSprite)>,
) {
    let board = &sim.0;
    // Reused buffer: rebuilt each frame without reallocating. Bank/eaten
    // storytelling happens in sim_events + effects; this system only keeps
    // sprites in step with the population.
    live.clear();
    live.extend(board.crabs().iter().map(|c| (c.id, *c)));
    for (entity, sprite) in &existing {
        if live.remove(&sprite.id).is_none() {
            commands.entity(entity).despawn();
        }
    }
    // Remaining entries are crabs without a sprite yet.
    for (&id, crab) in live.iter() {
        let pos = layout::creature_pos(board, crab.tile, crab.dir, crab.progress);
        let size = body_size(crab.kind);
        let claw = match crab.handed {
            Handedness::Left => CLAW_LEFT,
            Handedness::Right => CLAW_RIGHT,
        };
        // The claw sits at the crab's front, offset to its handed side.
        // Sprite is authored facing +X; sim-left is -Y in local space *before*
        // the world-y flip, so left-handed offset is +Y here.
        let claw_y = match crab.handed {
            Handedness::Left => size.y * 0.45,
            Handedness::Right => -size.y * 0.45,
        };
        commands
            .spawn((
                CrabSprite {
                    id,
                    kind: crab.kind,
                    shade: shade_of(id),
                },
                Sprite {
                    image: art.crab.clone(),
                    color: shell_color(crab.kind, shade_of(id)),
                    custom_size: Some(size * 1.25),
                    ..default()
                },
                Transform::from_translation(pos.extend(layout::z::CREATURE))
                    .with_rotation(layout::dir_rotation(crab.dir)),
            ))
            .with_children(|parent| {
                parent.spawn((
                    CreatureShadow,
                    Sprite {
                        image: art.shadow.clone(),
                        custom_size: Some(Vec2::new(size.x * 1.35, size.y * 1.15)),
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(0.0, 0.0, -0.5)),
                ));
                parent.spawn((
                    Sprite {
                        image: art.claw.clone(),
                        color: claw,
                        custom_size: Some(Vec2::splat(size.y * 0.62)),
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(size.x * 0.5, claw_y, 0.1)),
                ));
            });
    }
}

/// Smoothing factor for creature sprites. Normally the fixed-clock overstep
/// fraction; when the sim has been frozen for clearly longer than one fixed
/// period (pause, results, round over), hold at the final position instead of
/// oscillating. The 1.5-period threshold is the load-bearing part: at 60+ fps
/// most render frames legitimately fall *between* ticks, and treating those
/// as frozen snaps sprites forward on alternating frames (visible chop).
fn smoothing_alpha(
    ticks: u64,
    watch: &mut (u64, f32),
    delta_secs: f32,
    fixed_time: &Time<Fixed>,
) -> f32 {
    let (last_tick, frozen_for) = watch;
    if ticks == *last_tick {
        *frozen_for += delta_secs;
    } else {
        *last_tick = ticks;
        *frozen_for = 0.0;
    }
    if *frozen_for > fixed_time.timestep().as_secs_f32() * 1.5 {
        1.0
    } else {
        fixed_time.overstep_fraction()
    }
}

/// Interpolate each crab sprite between its previous and current sim position.
#[allow(clippy::too_many_arguments)]
pub fn interpolate_crabs(
    sim: Res<Sim>,
    art: Res<Art>,
    fixed_time: Res<Time<Fixed>>,
    time: Res<Time>,
    settings: Res<crate::app::settings::GameSettings>,
    mut commands: Commands,
    mut rng: ResMut<effects::VisualRng>,
    mut glint_clock: Local<f32>,
    mut watch: Local<(u64, f32)>,
    mut by_id: Local<HashMap<u32, Crab>>,
    mut sprites: Query<
        (&CrabSprite, &mut Transform, &mut Sprite, &Children),
        Without<CreatureShadow>,
    >,
    mut shadows: Query<&mut Transform, (With<CreatureShadow>, Without<CrabSprite>)>,
) {
    let board = &sim.0;
    let alpha = smoothing_alpha(board.ticks(), &mut watch, time.delta_secs(), &fixed_time);
    by_id.clear();
    by_id.extend(board.crabs().iter().map(|c| (c.id, *c)));
    // Precious crabs glint on one clock: they twinkle together, and what
    // keeps that from reading as a metronome is the scatter `glint` puts
    // on each star's own position. A clock apiece would be a timer per
    // crab for a twinkle nobody is timing.
    *glint_clock += time.delta_secs();
    let glint_now = *glint_clock >= 0.22;
    if glint_now {
        *glint_clock = 0.0;
    }
    // During a molting lure every loose crab ignores arrows; tint them all
    // toward the luring player's colour so the takeover is visible.
    let lure_tint = board.lure().map(|(owner, _)| palette::player_color(owner));
    for (sprite, mut transform, mut body, children) in &mut sprites {
        let Some(crab) = by_id.get(&sprite.id) else {
            continue; // despawns this frame
        };
        let base = shell_color(crab.kind, sprite.shade);
        body.color = match lure_tint {
            Some(tint) => base.mix(&tint, 0.55),
            None => base,
        };
        let prev = layout::pose_pos(board, crab.prev);
        let curr = layout::pose_pos(board, crab.pose());
        // A wrap-around crossing is a teleport, not a walk: snap, don't slide.
        let pos = if prev.distance(curr) > TILE * 2.0 {
            curr
        } else {
            prev.lerp(curr, alpha)
        };
        transform.translation = pos.extend(layout::z::CREATURE);
        // Scuttle: a small heading wiggle while actually moving, phased per
        // crab so a crowd shimmers instead of marching.
        let moving = prev != curr;
        let wobble = if moving {
            (time.elapsed_secs() * 11.0 + f32::from(sprite.id as u16 % 32)).sin() * 0.06
        } else {
            0.0
        };
        transform.rotation = layout::dir_rotation(crab.dir) * Quat::from_rotation_z(wobble);
        // Two-frame leg cycle while walking.
        let frame_b = moving
            && ((time.elapsed_secs() * 7.0 + f32::from(sprite.id as u16 % 8)) as u32)
                .is_multiple_of(2);
        let wanted = if frame_b { &art.crab_b } else { &art.crab };
        if body.image != *wanted {
            body.image = wanted.clone();
        }
        // Molting crabs pulse gently: they are worth chasing.
        if sprite.kind == CrabKind::Molting {
            let pulse = 1.0 + (time.elapsed_secs() * 5.0).sin() * 0.06;
            transform.scale = Vec3::splat(pulse);
        }
        // The shadow: pushed out from under the body, the same way as
        // every other thing standing on this sand. It used to sit exactly
        // beneath the crab, which is to say it was never once seen.
        //
        // After the pulse, and divided by it: a child's translation is in
        // the parent's frame, so a molting crab would otherwise swing its
        // shadow in time with its own breathing.
        let body_scale = transform.scale.x.max(f32::EPSILON);
        for child in children {
            if let Ok(mut shadow) = shadows.get_mut(*child) {
                shadow.translation = unturned(&transform, layout::SUN / body_scale, -0.5);
            }
        }
        // Sparkling and golden crabs actually sparkle. The golden one is
        // fifty points walking and is worth more than a twinkle: it draws
        // a ribbon of sparks behind it, so a table of four can see which
        // way the jackpot is heading from across the room.
        if glint_now {
            match sprite.kind {
                CrabKind::Sparkling => effects::glint(
                    &mut commands,
                    &mut rng,
                    &art,
                    curr,
                    Color::srgba(0.8, 0.95, 1.0, 0.95),
                ),
                CrabKind::Golden => {
                    effects::glint(
                        &mut commands,
                        &mut rng,
                        &art,
                        curr,
                        Color::srgba(1.0, 0.9, 0.3, 0.95),
                    );
                    if moving && !settings.reduced_motion {
                        let (dx, dy) = crab.dir.offset();
                        // Sim y runs down, world y runs up.
                        let heading = Vec2::new(dx as f32, -(dy as f32));
                        effects::spark_trail(
                            &mut commands,
                            &mut rng,
                            &art,
                            curr - heading * TILE * 0.3,
                            -heading,
                        );
                    }
                }
                CrabKind::Common | CrabKind::Juvenile | CrabKind::Giant | CrabKind::Molting => {}
            }
        }
    }
}

/// Spawn sprites for new gulls, despawn sprites for departed ones.
#[allow(clippy::too_many_arguments)]
pub fn sync_gull_sprites(
    mut commands: Commands,
    sim: Res<Sim>,
    art: Res<Art>,
    settings: Res<crate::app::settings::GameSettings>,
    mut rng: ResMut<effects::VisualRng>,
    mut live: Local<HashMap<u32, Gull>>,
    // The gulls that were on the board last frame, and the clock they were
    // on. A *sprite* being new is not a gull arriving: a level with gulls
    // written into it, a retry, a resumed round and an editor test run all
    // wipe the sprites and rebuild them, and keying the arrival cue off
    // that puffed sand under every bird on the beach at once, every time.
    mut before: Local<(Option<u64>, Vec<u32>)>,
    existing: Query<(Entity, &GullSprite)>,
) {
    let board = &sim.0;
    // A clock that has run backwards is a different board, whose gull ids
    // start again at zero, and a board at tick zero has not run at all:
    // nothing on either has "arrived", the birds on it are the ones it was
    // written with. So is a board this has never seen. The memory is
    // reseeded from those birds rather than emptied, which was what puffed
    // sand under every pre-placed gull on load and on every retry.
    let (last_ticks, seen) = &mut *before;
    let fresh = board.ticks() == 0 || last_ticks.is_none_or(|last| board.ticks() < last);
    if fresh {
        seen.clear();
        seen.extend(board.gulls().iter().map(|g| g.id));
    }
    *last_ticks = Some(board.ticks());
    live.clear();
    live.extend(board.gulls().iter().map(|g| (g.id, *g)));
    for (entity, sprite) in &existing {
        if live.remove(&sprite.0).is_none() {
            commands.entity(entity).despawn();
        }
    }
    for (&id, gull) in live.iter() {
        let pos = layout::creature_pos(board, gull.tile, gull.dir, gull.progress);
        // A gull arriving is the worst news on the beach and used to
        // appear in silence at the edge of the sand.
        if !settings.reduced_motion && !seen.contains(&id) {
            kick_up(&mut commands, &mut rng, &art, pos);
        }
        commands
            .spawn((
                GullSprite(id),
                Sprite {
                    image: art.gull.clone(),
                    custom_size: Some(Vec2::splat(TILE * 0.78)),
                    ..default()
                },
                Transform::from_translation(pos.extend(layout::z::CREATURE + 0.2))
                    .with_rotation(layout::dir_rotation(gull.dir)),
            ))
            .with_children(|parent| {
                parent.spawn((
                    CreatureShadow,
                    Sprite {
                        image: art.shadow.clone(),
                        custom_size: Some(Vec2::new(TILE * 0.72, TILE * 0.54)),
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(0.0, 0.0, -0.5)),
                ));
            });
    }
    seen.clear();
    seen.extend(board.gulls().iter().map(|gull| gull.id));
}

/// Interpolate gull sprites, and fly the flying ones.
///
/// A gull's hop is two to four tiles in a straight line (spec §3.5), and
/// the sim says nothing about height: it is on a tile or it is not. The
/// altitude here is invented whole, out of how much of the hop is left,
/// and it is what turns a bird sliding over the sand into a bird that
/// takes off, crosses, and comes down.
///
/// It also buys the telegraph for free. The shadow is pinned to the
/// ground, so it slides out from under the bird as it climbs and comes
/// back under it as it drops: by the time a gull is a tile from landing,
/// its shadow is already on the tile it is going to land on, which is the
/// warning the beach never used to give.
///
/// The flight scale is kept modest on purpose. Walls draw above creatures,
/// but a border plank is thirteen pixels of a tile-sized frame, so a gull
/// scaled far past its tile hangs out over the edge of the board with
/// nothing to occlude it and reads as a sprite that has come loose.
#[allow(clippy::too_many_arguments)]
pub fn interpolate_gulls(
    sim: Res<Sim>,
    art: Res<Art>,
    fixed_time: Res<Time<Fixed>>,
    time: Res<Time>,
    settings: Res<crate::app::settings::GameSettings>,
    mut commands: Commands,
    mut rng: ResMut<effects::VisualRng>,
    mut watch: Local<(u64, f32)>,
    mut by_id: Local<HashMap<u32, Gull>>,
    // Gulls currently in the air. Presence is the flight: a gull that
    // turns up here without an entry has just taken off, and one whose
    // entry outlives its flight has just landed.
    mut aloft: Local<HashMap<u32, Flight>>,
    mut sprites: Query<
        (&GullSprite, &mut Transform, &mut Sprite, &Children),
        Without<CreatureShadow>,
    >,
    mut shadows: Query<(&mut Transform, &mut Sprite), With<CreatureShadow>>,
) {
    let board = &sim.0;
    // Read before `smoothing_alpha`, which writes this frame's tick into
    // the watch: after it, the two always agree and the test below is dead.
    let fresh = board.ticks() < watch.0 || board.ticks() == 0;
    let alpha = smoothing_alpha(board.ticks(), &mut watch, time.delta_secs(), &fixed_time);
    by_id.clear();
    by_id.extend(board.gulls().iter().map(|g| (g.id, *g)));
    // A clock that has run backwards is a different board, and gull ids
    // start again at zero on every one of them. Pruning by id alone let a
    // recycled id inherit a latched descent and a frozen altitude, so the
    // bird slid flat over the sand with its shadow pinned under it - the
    // landing telegraph off, silently, for the rest of that hop.
    if fresh {
        aloft.clear();
    }
    aloft.retain(|id, _| by_id.contains_key(id));
    let t = time.elapsed_secs();
    for (sprite, mut transform, mut image, children) in &mut sprites {
        let Some(gull) = by_id.get(&sprite.0) else {
            continue;
        };
        let prev = layout::pose_pos(board, gull.prev);
        let curr = layout::pose_pos(board, gull.pose());
        let pos = if prev.distance(curr) > TILE * 2.0 {
            curr
        } else {
            prev.lerp(curr, alpha)
        };
        transform.translation = pos.extend(layout::z::CREATURE + 0.2);
        let phase = f32::from(sprite.0 as u16 % 32);
        let altitude = match gull.state {
            GullState::Walking => {
                aloft.remove(&sprite.0);
                0.0
            }
            GullState::Flying { remaining } => {
                let calm = settings.reduced_motion;
                let flight = aloft.entry(gull.id).or_insert_with(|| {
                    // Just off the ground: the sand it pushed away.
                    if !calm {
                        kick_up(&mut commands, &mut rng, &art, pos);
                    }
                    Flight {
                        start: remaining,
                        altitude: 0.0,
                        descending: false,
                    }
                });
                let across = f32::from(gull.progress) / f32::from(crate::sim::SUBUNITS_PER_TILE);
                flight.height(remaining, across)
            }
        };
        let flying = altitude > 0.0 || matches!(gull.state, GullState::Flying { .. });
        // Dedicated spread-wing art in flight (write-on-change).
        let wanted = if flying { &art.gull_fly } else { &art.gull };
        if image.image != *wanted {
            image.image = wanted.clone();
        }
        let (scale, waddle) = if flying {
            (1.0 + 0.22 * altitude + (t * 16.0 + phase).sin() * 0.05, 0.0)
        } else if prev == curr {
            (1.0, 0.0)
        } else {
            (1.0, (t * 9.0 + phase).sin() * 0.05)
        };
        transform.rotation = layout::dir_rotation(gull.dir) * Quat::from_rotation_z(waddle);
        transform.scale = Vec3::splat(scale);
        for child in children {
            let Ok((mut shadow_tf, mut shadow_sprite)) = shadows.get_mut(*child) else {
                continue;
            };
            // Left behind on the ground, further out the higher the bird
            // is, with a slow circling drift on top of the drop: the
            // kinetic cue that it is airborne. A walking gull's shadow
            // sits at its feet, where the same offset every other thing on
            // the beach casts puts it.
            let drift = Vec2::new((t * 2.1 + phase).cos() * 2.5, (t * 1.7 + phase).sin() * 2.5);
            let drop = 26.0 * altitude;
            let world = layout::SUN + Vec2::new(-drop * 0.5, -drop) + drift * altitude;
            // Divided by the parent's scale for the same reason the
            // shadow's own scale is: a child's translation is in the
            // parent's frame, so a bird pulsing at its wing-beat would
            // otherwise drag its shadow back and forth under it.
            shadow_tf.translation = unturned(&transform, world / scale, -0.5);
            // Compensating the parent's scale keeps the shadow the size of
            // the bird's footprint rather than of the bird.
            shadow_tf.scale = Vec3::splat((1.0 - 0.25 * altitude) / scale);
            // It stays dark as the bird climbs: the shadow is the warning,
            // and a warning that fades as the danger nears is no warning.
            shadow_sprite.color = Color::srgba(1.0, 1.0, 1.0, 1.0 - 0.15 * altitude);
        }
    }
}

/// One gull's hop, as the render layer sees it.
///
/// The sim never says how high a gull is - it is on a tile or it is not -
/// so the height is invented here out of how much of the hop is left, and
/// carried between frames, because a bird that has started coming down
/// must not go back up.
pub struct Flight {
    /// Tiles left when it took off.
    start: u8,
    /// Where it was last frame.
    altitude: f32,
    /// Whether it has begun its descent. Latched, never cleared: it is
    /// only ever true for the rest of one hop.
    descending: bool,
}

impl Flight {
    /// How high the bird is, `across` of the way over a tile with
    /// `remaining` tiles of hop still to go.
    ///
    /// The load-bearing part is that the last airborne tile is the one
    /// with `remaining == 1`, not zero. The sim lands a gull on the tick
    /// its counter would reach zero, so `Flying { remaining: 0 }` is a
    /// state no frame ever observes: reading the descent off it left the
    /// bird at full height until the instant it became a walking gull and
    /// then dropped it in one frame, which is the opposite of a telegraph.
    ///
    /// A gull whose landing tile turns out to be rock or kelp is given one
    /// more tile to glide, and its `across` starts over: without the latch
    /// it would bob back to full height for that tile.
    fn height(&mut self, remaining: u8, across: f32) -> f32 {
        if remaining <= 1 {
            self.descending = true;
        }
        let climb = if remaining >= self.start { across } else { 1.0 };
        let descend = if remaining <= 1 { 1.0 - across } else { 1.0 };
        let wanted = climb.min(descend).clamp(0.0, 1.0);
        self.altitude = if self.descending {
            wanted.min(self.altitude)
        } else {
            wanted
        };
        self.altitude
    }
}

/// A world-space offset expressed in the local space of a sprite that has
/// been turned to face its heading.
///
/// Creature sprites are rotated to their direction and their shadows are
/// children of them, so a shadow given a plain local offset swings round
/// the creature as it turns corners. Undoing the parent's rotation is what
/// keeps the sun still while the crab walks under it.
fn unturned(parent: &Transform, world: Vec2, z: f32) -> Vec3 {
    let local = parent.rotation.inverse() * world.extend(0.0);
    Vec3::new(local.x, local.y, z)
}

/// The sand a gull throws down when it beats its way off the beach.
fn kick_up(commands: &mut Commands, rng: &mut effects::VisualRng, art: &Art, pos: Vec2) {
    effects::burst(
        commands,
        rng,
        &effects::Burst {
            image: art.puff.clone(),
            pos,
            color: Color::srgba(0.93, 0.87, 0.7, 0.75),
            count: 5,
            size: 13.0,
            speed: 52.0,
            gravity: 80.0,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A crab keeps its shell for as long as it lives. The shade is a
    /// hash of the id rather than a draw, precisely so that a sprite
    /// rebuilt around a crab - and they are rebuilt constantly - does not
    /// hand it a new colour each time.
    #[test]
    fn a_crab_keeps_the_shell_it_was_born_with() {
        for id in [0u32, 1, 7, 64, 1_000, 65_535, u32::MAX] {
            assert_eq!(shade_of(id), shade_of(id), "id {id} drifted");
            let shade = shade_of(id);
            assert!(
                (-SHADE_SPREAD..=SHADE_SPREAD).contains(&shade),
                "id {id} strayed off the band: {shade}"
            );
        }
    }

    /// And the crowd has to actually be a crowd: a hash that answered the
    /// same for every id would compile, pass the test above, and leave
    /// twenty identical crabs on the sand.
    #[test]
    fn a_crowd_is_not_twenty_copies_of_one_crab() {
        let shades: Vec<f32> = (0..40).map(shade_of).collect();
        let spread = shades.iter().fold(f32::MIN, |a, &b| a.max(b))
            - shades.iter().fold(f32::MAX, |a, &b| a.min(b));
        assert!(
            spread > SHADE_SPREAD,
            "forty crabs covered only {spread} of the band"
        );
    }

    /// A shell is its kind's colour, nudged - never another kind's.
    ///
    /// The kinds are told apart by colour on a crowded beach, and what
    /// that needs is not a small *number* (the nudge is a perceptual
    /// lighten, so a saturated gold moves a long way in blue for very
    /// little visible change) but a guarantee about neighbours: whatever
    /// the shade does, the shell must still be nearer its own kind than
    /// any other. A spread wide enough to blur a juvenile into a common
    /// would cost the read.
    #[test]
    fn a_shade_never_carries_a_shell_into_another_kind() {
        const KINDS: [CrabKind; 6] = [
            CrabKind::Common,
            CrabKind::Juvenile,
            CrabKind::Giant,
            CrabKind::Molting,
            CrabKind::Golden,
            CrabKind::Sparkling,
        ];
        let apart = |a: Color, b: Color| {
            let (a, b) = (a.to_srgba(), b.to_srgba());
            ((a.red - b.red).powi(2) + (a.green - b.green).powi(2) + (a.blue - b.blue).powi(2))
                .sqrt()
        };
        for kind in KINDS {
            for id in 0..40u32 {
                let shell = shell_color(kind, shade_of(id));
                let own = apart(shell, body_color(kind));
                for other in KINDS.into_iter().filter(|&k| k != kind) {
                    assert!(
                        own < apart(shell, body_color(other)),
                        "a shaded {kind:?} (id {id}) is nearer {other:?} than itself"
                    );
                }
                let srgba = shell.to_srgba();
                for channel in [srgba.red, srgba.green, srgba.blue] {
                    assert!((0.0..=1.0).contains(&channel), "{kind:?} left the channel");
                }
            }
        }
    }

    /// The sun stays put while the creature turns. A shadow given a plain
    /// local offset swings round its owner at every corner, which is the
    /// artefact `unturned` exists to undo - so undoing it has to be exact,
    /// for every heading.
    #[test]
    fn a_shadow_holds_its_bearing_through_every_turn() {
        for dir in [
            crate::sim::Direction::Up,
            crate::sim::Direction::Down,
            crate::sim::Direction::Left,
            crate::sim::Direction::Right,
        ] {
            let parent = Transform::from_rotation(layout::dir_rotation(dir));
            let local = unturned(&parent, layout::SUN, -0.5);
            // Put back through the parent's own rotation, it is the world
            // offset again: that is the whole contract.
            let world = parent.rotation * local;
            assert!(
                world.truncate().distance(layout::SUN) < 1e-4,
                "{dir:?} bent the sun to {:?}",
                world.truncate()
            );
            assert_eq!(local.z, -0.5, "and the layer is left alone");
        }
    }

    /// The gull's invented altitude has one subtle contract in it: the
    /// last tile a gull flies is the one with `remaining == 1`, because
    /// the sim lands it on the tick the counter would reach zero. Reading
    /// the descent off zero instead held the bird at full height and then
    /// dropped it in a single frame.
    #[test]
    fn a_gull_climbs_once_and_only_ever_comes_down() {
        let mut flight = Flight {
            start: 3,
            altitude: 0.0,
            descending: false,
        };
        assert_eq!(flight.height(3, 0.0), 0.0, "leaves the sand at nothing");
        assert!((flight.height(3, 0.5) - 0.5).abs() < 1e-6, "climbing");
        assert_eq!(flight.height(2, 0.5), 1.0, "level across the middle");
        assert!(
            (flight.height(1, 0.25) - 0.75).abs() < 1e-6,
            "and coming down over the last tile, which is `remaining == 1`"
        );
        let low = flight.height(1, 0.9);
        assert!(low < 0.15, "all but on the sand: {low}");
        // A rock or kelp under the landing tile buys one more tile, and
        // its `across` starts over. The bird glides on; it does not bob.
        assert!(flight.height(1, 0.0) <= low, "went back up to {low}");
    }

    /// A 30 Hz fixed clock with `overstep` of a period already accumulated,
    /// so the "between ticks" answer is a fraction the "frozen" answer
    /// cannot be mistaken for.
    fn fixed_clock(overstep: f32) -> Time<Fixed> {
        let mut fixed = Time::<Fixed>::from_hz(30.0);
        fixed.accumulate_overstep(fixed.timestep().mul_f32(overstep));
        fixed
    }

    /// The 1.5-period threshold is the load-bearing part: a render frame
    /// that falls between ticks at 60+ fps is not a frozen sim, and calling
    /// it one snaps sprites forward on alternating frames. So a sim that
    /// has not ticked for 1.2 periods still interpolates, 1.6 periods holds
    /// at the final position, and the first new tick starts the count over.
    #[test]
    fn the_sim_counts_as_frozen_only_past_one_and_a_half_periods() {
        let fixed = fixed_clock(0.4);
        let period = fixed.timestep().as_secs_f32();
        let between = fixed.overstep_fraction();
        assert!((0.0..1.0).contains(&between), "a fraction, got {between}");
        let mut watch = (5u64, 0.0f32);
        // Three frames of 0.4 periods: 1.2 periods without a tick.
        let mut alpha = 0.0;
        for _ in 0..3 {
            alpha = smoothing_alpha(5, &mut watch, period * 0.4, &fixed);
        }
        assert_eq!(
            alpha, between,
            "1.2 periods without a tick still interpolates"
        );
        // One more frame makes 1.6 periods: frozen, hold the final position.
        let alpha = smoothing_alpha(5, &mut watch, period * 0.4, &fixed);
        assert_eq!(alpha, 1.0, "1.6 periods without a tick holds still");
        // The sim ticks: the frozen count resets and interpolation resumes.
        let alpha = smoothing_alpha(6, &mut watch, period * 0.4, &fixed);
        assert_eq!(alpha, between, "a fresh tick resets the frozen count");
        assert_eq!(watch, (6, 0.0), "the watch follows the new tick");
    }
}
