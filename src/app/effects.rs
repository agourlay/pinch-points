//! Render-side particles and floating score text: the game's moments made
//! visible. Everything here is cosmetic: spawned from *observed* sim state,
//! driven by frame time, and drawing on its own PRNG so the deterministic
//! sim stream is never touched.

use crate::app::art::Art;
use crate::app::layout::{self, TILE};
use crate::app::palette;
use crate::app::sim_events::SimEvent;
use bevy::prelude::*;

/// Which claw a crab put down last. A print is offset to one side of the
/// stride and the sides alternate; a `bool` called `left` in a tuple was
/// the kind of thing that reads the other way round at the second site.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Foot {
    Left,
    Right,
}

impl Foot {
    fn other(self) -> Foot {
        match self {
            Foot::Left => Foot::Right,
            Foot::Right => Foot::Left,
        }
    }

    /// How far to the side of the stride the print lands, signed.
    fn side(self) -> f32 {
        match self {
            Foot::Left => 5.0,
            Foot::Right => -5.0,
        }
    }
}

/// Where a crab last left a print, and with which foot.
#[derive(Clone, Copy, Debug)]
pub struct Footfall {
    last: Vec2,
    foot: Foot,
}

/// A cheap LCG for visual variety only. Never seed gameplay from this.
#[derive(Resource)]
pub struct VisualRng(u32);

impl Default for VisualRng {
    fn default() -> Self {
        VisualRng(0x9E37_79B9)
    }
}

impl VisualRng {
    pub(crate) fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }

    /// Uniform-ish float in `lo..hi`.
    pub(crate) fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (self.next() >> 8) as f32 / 16_777_216.0 * (hi - lo)
    }
}

/// A short-lived drifting sprite (or floating text, when spawned with one).
#[derive(Component)]
pub struct Particle {
    velocity: Vec2,
    /// Radians per second.
    spin: f32,
    /// Scale multiplier per second (1.0 = constant size).
    grow: f32,
    age: f32,
    life: f32,
}

/// Spawn a burst of `count` copies of `image` at `pos`, scattering outward.
#[allow(clippy::too_many_arguments)]
pub fn burst(
    commands: &mut Commands,
    rng: &mut VisualRng,
    image: &Handle<Image>,
    pos: Vec2,
    color: Color,
    count: usize,
    size: f32,
    speed: f32,
) {
    for _ in 0..count {
        let angle = rng.range(0.0, std::f32::consts::TAU);
        let velocity = Vec2::from_angle(angle) * rng.range(speed * 0.4, speed);
        commands.spawn((
            Particle {
                velocity,
                spin: rng.range(-4.0, 4.0),
                grow: rng.range(0.2, 0.7),
                age: 0.0,
                life: rng.range(0.35, 0.6),
            },
            Sprite {
                image: image.clone(),
                color,
                custom_size: Some(Vec2::splat(size * rng.range(0.7, 1.3))),
                ..default()
            },
            Transform::from_translation(pos.extend(layout::z::PARTICLE)),
        ));
    }
}

/// A single glint star that twinkles out in place.
pub fn glint(commands: &mut Commands, rng: &mut VisualRng, art: &Art, pos: Vec2, color: Color) {
    let offset = Vec2::new(rng.range(-14.0, 14.0), rng.range(-14.0, 14.0));
    commands.spawn((
        Particle {
            velocity: Vec2::new(0.0, rng.range(4.0, 10.0)),
            spin: rng.range(1.0, 3.0),
            grow: -0.8,
            age: 0.0,
            life: rng.range(0.4, 0.7),
        },
        Sprite {
            image: art.star.clone(),
            color,
            custom_size: Some(Vec2::splat(rng.range(8.0, 15.0))),
            ..default()
        },
        Transform::from_translation((pos + offset).extend(layout::z::PARTICLE)),
    ));
}

/// Floating score text ("+3", "-12") rising from a board position.
pub fn score_pip(commands: &mut Commands, text: String, pos: Vec2, color: Color) {
    commands.spawn((
        Particle {
            velocity: Vec2::new(0.0, 30.0),
            spin: 0.0,
            grow: 0.0,
            age: 0.0,
            life: 1.2,
        },
        Text2d::new(text),
        TextFont {
            font_size: FontSize::Px(22.0),
            ..default()
        },
        TextColor(color),
        Transform::from_translation(pos.extend(layout::z::PIP)),
    ));
}

/// Confetti for the winner: tinted stars raining over the board.
pub fn confetti(commands: &mut Commands, rng: &mut VisualRng, art: &Art, color: Color) {
    for _ in 0..46 {
        let x = rng.range(-6.5 * TILE, 6.5 * TILE);
        let y = rng.range(3.0 * TILE, 6.5 * TILE);
        commands.spawn((
            Particle {
                velocity: Vec2::new(rng.range(-18.0, 18.0), rng.range(-130.0, -70.0)),
                spin: rng.range(-6.0, 6.0),
                grow: 0.0,
                age: 0.0,
                life: rng.range(1.6, 3.0),
            },
            Sprite {
                image: art.star.clone(),
                color: color.lighter(rng.range(0.0, 0.2)),
                custom_size: Some(Vec2::splat(rng.range(8.0, 16.0))),
                ..default()
            },
            Transform::from_translation(Vec3::new(x, y, layout::z::CONFETTI)),
        ));
    }
}

/// Walking crabs scuff the sand: tiny alternating footprints that linger
/// and fade, plus the occasional kicked-up grain. Footfalls are paced by
/// distance walked, tracked per crab id. Pure decoration, sim untouched.
pub fn crab_trails(
    mut commands: Commands,
    sim: Res<crate::app::Sim>,
    art: Res<Art>,
    settings: Res<crate::app::settings::GameSettings>,
    mut rng: ResMut<VisualRng>,
    mut footfalls: Local<bevy::platform::collections::HashMap<u32, Footfall>>,
    mut seen: Local<Vec<u32>>,
) {
    use crate::sim::TileKind;
    if settings.reduced_motion {
        return;
    }
    const STRIDE: f32 = 20.0;
    let board = &sim.0;
    seen.clear();
    for crab in board.crabs() {
        seen.push(crab.id);
        let pos = layout::creature_pos(board, crab.tile, crab.dir, crab.progress);
        let Some(fall) = footfalls.get_mut(&crab.id) else {
            footfalls.insert(
                crab.id,
                Footfall {
                    last: pos,
                    foot: Foot::Right,
                },
            );
            continue;
        };
        let step = pos - fall.last;
        if step.length() < STRIDE {
            continue;
        }
        let side = step.normalize_or_zero().perp() * fall.foot.side();
        fall.last = pos;
        fall.foot = fall.foot.other();
        let (x, y) = board.coords_u8(crab.tile);
        if board.tile_at(x, y) != TileKind::Empty {
            continue; // prints only show on dry sand
        }
        commands.spawn((
            Particle {
                velocity: Vec2::ZERO,
                spin: 0.0,
                grow: 0.0,
                age: 0.0,
                life: rng.range(1.4, 2.2),
            },
            Sprite::from_color(
                Color::srgba(0.45, 0.36, 0.24, 0.38),
                Vec2::splat(rng.range(3.0, 4.2)),
            ),
            Transform::from_translation((pos + side).extend(layout::z::WET + 0.05)),
        ));
        // Now and then a grain of sand kicks up behind the crab.
        if rng.range(0.0, 1.0) < 0.3 {
            commands.spawn((
                Particle {
                    velocity: -step.normalize_or_zero() * rng.range(12.0, 26.0),
                    spin: rng.range(-3.0, 3.0),
                    grow: 0.5,
                    age: 0.0,
                    life: rng.range(0.25, 0.4),
                },
                Sprite {
                    image: art.puff.clone(),
                    color: Color::srgba(0.9, 0.82, 0.62, 0.55),
                    custom_size: Some(Vec2::splat(rng.range(5.0, 8.0))),
                    ..default()
                },
                Transform::from_translation(pos.extend(layout::z::PARTICLE)),
            ));
        }
    }
    footfalls.retain(|id, _| seen.contains(id));
}

/// Advance and expire particles: drift, spin, grow, fade out over life.
#[allow(clippy::type_complexity)]
pub fn update_particles(
    time: Res<Time>,
    mut commands: Commands,
    mut particles: Query<(
        Entity,
        &mut Particle,
        &mut Transform,
        Option<&mut Sprite>,
        Option<&mut TextColor>,
    )>,
) {
    let dt = time.delta_secs();
    for (entity, mut particle, mut transform, sprite, text_color) in &mut particles {
        particle.age += dt;
        if particle.age >= particle.life {
            commands.entity(entity).despawn();
            continue;
        }
        let fade = 1.0 - particle.age / particle.life;
        transform.translation.x += particle.velocity.x * dt;
        transform.translation.y += particle.velocity.y * dt;
        transform.rotation *= Quat::from_rotation_z(particle.spin * dt);
        let scale = (1.0 + particle.grow * particle.age).max(0.05);
        transform.scale = Vec3::splat(scale);
        if let Some(mut sprite) = sprite {
            sprite.color = sprite.color.with_alpha(fade);
        }
        if let Some(mut color) = text_color {
            color.0 = color.0.with_alpha(fade);
        }
    }
}

/// Turn sim events into their on-board moments: pips, puffs, flashes.
pub fn moment_effects(
    mut commands: Commands,
    mut events: MessageReader<SimEvent>,
    art: Res<Art>,
    settings: Res<crate::app::settings::GameSettings>,
    mut rng: ResMut<VisualRng>,
) {
    // Reduced motion keeps the news and drops the fireworks: the score pip
    // for a raid still floats up (it is the only place that number appears),
    // but the puffs, the scatter, and the full-tile white flash do not.
    let calm = settings.reduced_motion;
    for event in events.read() {
        if calm && !matches!(event, SimEvent::CastleRaided { .. }) {
            continue;
        }
        match event {
            SimEvent::CrabBanked { pos, .. } => {
                burst(
                    &mut commands,
                    &mut rng,
                    &art.puff,
                    *pos,
                    Color::srgba(0.95, 0.88, 0.7, 0.8),
                    4,
                    14.0,
                    40.0,
                );
            }
            SimEvent::CrabEaten { pos } => burst(
                &mut commands,
                &mut rng,
                &art.puff,
                *pos,
                Color::srgba(0.85, 0.92, 0.98, 0.8),
                5,
                12.0,
                55.0,
            ),
            // The ghost of the signpost that just got traded away, still
            // pointing where it pointed, swelling as it fades. Under the
            // versus cap a fourth placement takes your oldest, and it used
            // to vanish in silence somewhere else on the beach: the player
            // sees the arrow they lost, and where they lost it.
            SimEvent::SignpostEvicted { owner, pos, dir } => {
                commands.spawn((
                    Particle {
                        velocity: Vec2::ZERO,
                        spin: 0.0,
                        grow: 1.1,
                        age: 0.0,
                        life: 0.5,
                    },
                    Sprite {
                        image: art.arrow.clone(),
                        color: palette::player_color(*owner).lighter(0.12),
                        custom_size: Some(Vec2::splat(layout::TILE * 0.88)),
                        ..default()
                    },
                    Transform::from_translation(pos.extend(layout::z::PARTICLE))
                        .with_rotation(layout::dir_rotation(*dir)),
                ));
            }
            SimEvent::CrabSpawned { pos } => burst(
                &mut commands,
                &mut rng,
                &art.puff,
                *pos,
                Color::srgba(0.9, 0.8, 0.6, 0.85),
                3,
                12.0,
                35.0,
            ),
            SimEvent::CastleRaided { pos, lost, .. } => {
                if calm {
                    score_pip(
                        &mut commands,
                        format!("-{lost}"),
                        *pos + Vec2::new(0.0, 10.0),
                        Color::srgb(0.96, 0.3, 0.25),
                    );
                    continue;
                }
                commands.spawn((
                    Particle {
                        velocity: Vec2::ZERO,
                        spin: 0.0,
                        grow: 1.6,
                        age: 0.0,
                        life: 0.3,
                    },
                    Sprite {
                        image: art.puff.clone(),
                        color: Color::srgba(1.0, 1.0, 1.0, 0.9),
                        custom_size: Some(Vec2::splat(TILE * 1.1)),
                        ..default()
                    },
                    Transform::from_translation(pos.extend(layout::z::FLASH)),
                ));
                burst(
                    &mut commands,
                    &mut rng,
                    &art.puff,
                    *pos,
                    Color::srgba(0.9, 0.8, 0.6, 0.85),
                    8,
                    16.0,
                    70.0,
                );
                score_pip(
                    &mut commands,
                    format!("-{lost}"),
                    *pos + Vec2::new(0.0, 10.0),
                    Color::srgb(0.96, 0.3, 0.25),
                );
            }
            SimEvent::GullLanded { pos } => burst(
                &mut commands,
                &mut rng,
                &art.puff,
                *pos,
                Color::srgba(0.92, 0.85, 0.68, 0.8),
                4,
                13.0,
                45.0,
            ),
            SimEvent::GullArrived
            | SimEvent::GullTookOff
            | SimEvent::SignpostPlaced { .. }
            | SimEvent::SignpostRemoved { .. }
            | SimEvent::TierUp { .. }
            | SimEvent::TideEventFired { .. }
            | SimEvent::SurgeStarted
            | SimEvent::RoundEnded => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `range` feeds particle lifetimes, sizes and spins straight into
    /// sprite maths, and a draw that lands on or past `hi` (a 24-bit
    /// mantissa scaled a hair too far) is a zero-length life or a
    /// negative size nobody will trace back here. Ten thousand draws stay
    /// in the half-open interval, for a plain range and a negative one.
    #[test]
    fn a_visual_draw_never_leaves_the_half_open_range() {
        let mut rng = VisualRng::default();
        for (lo, hi) in [(0.35f32, 0.6f32), (-4.0, 4.0), (0.0, std::f32::consts::TAU)] {
            let (mut min, mut max) = (f32::MAX, f32::MIN);
            for _ in 0..10_000 {
                let draw = rng.range(lo, hi);
                assert!((lo..hi).contains(&draw), "{draw} is outside {lo}..{hi}");
                min = min.min(draw);
                max = max.max(draw);
            }
            // And it actually spreads across the range rather than sitting
            // on one end of it.
            assert!(
                min < lo + (hi - lo) * 0.05,
                "min {min} never came near {lo}"
            );
            assert!(
                max > hi - (hi - lo) * 0.05,
                "max {max} never came near {hi}"
            );
        }
    }
}
