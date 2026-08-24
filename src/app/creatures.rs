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

fn body_size(kind: CrabKind) -> Vec2 {
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
                },
                Sprite {
                    image: art.crab.clone(),
                    color: body_color(crab.kind),
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
    mut commands: Commands,
    mut rng: ResMut<effects::VisualRng>,
    mut glint_clock: Local<f32>,
    mut watch: Local<(u64, f32)>,
    mut by_id: Local<HashMap<u32, Crab>>,
    mut sprites: Query<(&CrabSprite, &mut Transform, &mut Sprite)>,
) {
    let board = &sim.0;
    let alpha = smoothing_alpha(board.ticks(), &mut watch, time.delta_secs(), &fixed_time);
    by_id.clear();
    by_id.extend(board.crabs().iter().map(|c| (c.id, *c)));
    // Precious crabs glint on a shared clock (staggered per crab id).
    *glint_clock += time.delta_secs();
    let glint_now = *glint_clock >= 0.22;
    if glint_now {
        *glint_clock = 0.0;
    }
    // During a molting lure every loose crab ignores arrows; tint them all
    // toward the luring player's colour so the takeover is visible.
    let lure_tint = board.lure().map(|(owner, _)| palette::player_color(owner));
    for (sprite, mut transform, mut body) in &mut sprites {
        let Some(crab) = by_id.get(&sprite.id) else {
            continue; // despawns this frame
        };
        let base = body_color(crab.kind);
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
        // Sparkling and golden crabs actually sparkle.
        if glint_now {
            match sprite.kind {
                CrabKind::Sparkling => effects::glint(
                    &mut commands,
                    &mut rng,
                    &art,
                    curr,
                    Color::srgba(0.8, 0.95, 1.0, 0.95),
                ),
                CrabKind::Golden => effects::glint(
                    &mut commands,
                    &mut rng,
                    &art,
                    curr,
                    Color::srgba(1.0, 0.9, 0.3, 0.95),
                ),
                CrabKind::Common | CrabKind::Juvenile | CrabKind::Giant | CrabKind::Molting => {}
            }
        }
    }
}

/// Spawn sprites for new gulls, despawn sprites for departed ones.
pub fn sync_gull_sprites(
    mut commands: Commands,
    sim: Res<Sim>,
    art: Res<Art>,
    mut live: Local<HashMap<u32, Gull>>,
    existing: Query<(Entity, &GullSprite)>,
) {
    let board = &sim.0;
    live.clear();
    live.extend(board.gulls().iter().map(|g| (g.id, *g)));
    for (entity, sprite) in &existing {
        if live.remove(&sprite.0).is_none() {
            commands.entity(entity).despawn();
        }
    }
    for (&id, gull) in live.iter() {
        let pos = layout::creature_pos(board, gull.tile, gull.dir, gull.progress);
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
                        custom_size: Some(Vec2::new(TILE * 0.62, TILE * 0.46)),
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(0.0, 0.0, -0.5)),
                ));
            });
    }
}

/// Interpolate gull sprites; a flying gull is drawn larger, as if nearer the
/// camera, with a wing-beat pulse, and its shadow shrinks and slips behind:
/// the altitude illusion. Walking gulls waddle slightly.
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
    mut watch: Local<(u64, f32)>,
    mut by_id: Local<HashMap<u32, Gull>>,
    mut sprites: Query<
        (&GullSprite, &mut Transform, &mut Sprite, &Children),
        Without<CreatureShadow>,
    >,
    mut shadows: Query<(&mut Transform, &mut Sprite), With<CreatureShadow>>,
) {
    let board = &sim.0;
    let alpha = smoothing_alpha(board.ticks(), &mut watch, time.delta_secs(), &fixed_time);
    by_id.clear();
    by_id.extend(board.gulls().iter().map(|g| (g.id, *g)));
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
        let flying = matches!(gull.state, GullState::Flying { .. });
        // Dedicated spread-wing art in flight (write-on-change).
        let wanted = if flying { &art.gull_fly } else { &art.gull };
        if image.image != *wanted {
            image.image = wanted.clone();
        }
        let (scale, waddle) = if flying {
            (1.18 + (t * 16.0 + phase).sin() * 0.05, 0.0)
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
            if flying {
                // Compensate the parent's scale so the ground shadow shrinks
                // while the bird grows, and let it trail behind with a slow
                // circling drift - the kinetic cue that the bird is airborne.
                let drift = Vec2::new((t * 2.1 + phase).cos() * 2.5, (t * 1.7 + phase).sin() * 2.5);
                shadow_tf.translation = Vec3::new(-8.0 + drift.x, -8.0 + drift.y, -0.5);
                shadow_tf.scale = Vec3::splat(0.55 / scale);
                shadow_sprite.color = Color::srgba(1.0, 1.0, 1.0, 0.55);
            } else {
                shadow_tf.translation = Vec3::new(0.0, 0.0, -0.5);
                shadow_tf.scale = Vec3::ONE;
                shadow_sprite.color = Color::WHITE;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
