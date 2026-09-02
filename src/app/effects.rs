//! Render-side particles and floating score text: the game's moments made
//! visible. Everything here is cosmetic: spawned from *observed* sim state,
//! driven by frame time, and drawing on its own PRNG so the deterministic
//! sim stream is never touched.

use crate::app::art::Art;
use crate::app::layout::{self, TILE};
use crate::app::palette;
use crate::app::sim_events::SimEvent;
use crate::sim::TileKind;
use bevy::color::Mix;
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

/// How hard the camera is being shaken, 0 (still) to 1 (a raid).
///
/// A single pool rather than a queue of shakes: two events landing together
/// should not add up to a camera that leaves the window, and the square in
/// [`Trauma::offset`] is what makes a small knock read as a tap and a big
/// one as a blow. Written by the moment systems, read once a frame by
/// `boot::shake_camera` after the fit has placed the camera.
#[derive(Resource, Default)]
pub struct Trauma(f32);

impl Trauma {
    /// Ceiling on the pool: past this the shake stops growing, so a pile-up
    /// of raids in one frame is loud, not unwatchable.
    const MAX: f32 = 1.0;
    /// How much of the pool drains per second.
    const DECAY: f32 = 1.9;
    /// The worst displacement, in screen pixels, at full trauma.
    const THROW: f32 = 7.0;

    /// A knock. Clamped, so the pool cannot be filled past [`Trauma::MAX`].
    pub fn add(&mut self, amount: f32) {
        self.0 = (self.0 + amount).min(Self::MAX);
    }

    /// Drain the pool and answer where the camera should sit this frame,
    /// in screen pixels off its resting place.
    ///
    /// Two incommensurate sines per axis rather than a fresh random offset
    /// each frame: random jitter strobes at high frame rates and reads as a
    /// broken sprite, where this reads as a camera being shoved.
    pub fn offset(&mut self, dt: f32, elapsed: f32) -> Vec2 {
        self.0 = (self.0 - dt * Self::DECAY).max(0.0);
        if self.0 <= 0.0 {
            return Vec2::ZERO;
        }
        let throw = Self::THROW * self.0 * self.0;
        Vec2::new(
            (elapsed * 37.0).sin() * (elapsed * 17.0).cos(),
            (elapsed * 29.0).sin() * (elapsed * 23.0).cos(),
        ) * throw
    }
}

/// A short-lived drifting sprite (or floating text, when spawned with one).
#[derive(Component)]
pub struct Particle {
    velocity: Vec2,
    /// Pixels per second squared. What makes a kicked grain of sand fall
    /// back to the beach instead of sailing off the board.
    gravity: Vec2,
    /// Fraction of speed shed per second; 0.0 coasts for ever.
    drag: f32,
    /// Radians per second.
    spin: f32,
    /// Scale multiplier per second (1.0 = constant size).
    grow: f32,
    /// A colour to drift to over life, and the one drifted from. `None`
    /// keeps the spawn colour and moves only its alpha.
    ramp: Option<(Color, Color)>,
    /// Fraction of life spent fading in. Nothing should arrive at full
    /// strength on the frame it is born: at 60 fps that is a pop.
    fade_in: f32,
    /// The alpha this was spawned at, read out of the sprite on the first
    /// frame and kept.
    ///
    /// It is the ceiling: a piece asked for at 60% never draws stronger
    /// than 60%. That is new - the alpha in a spawn colour used to be
    /// thrown away and everything drew at full strength - so a caller that
    /// wants what it always got asks for 1.0. See [`Particle::shade`] for
    /// why the ceiling is read once and kept.
    peak: Option<f32>,
    age: f32,
    life: f32,
}

impl Default for Particle {
    fn default() -> Self {
        Particle {
            velocity: Vec2::ZERO,
            gravity: Vec2::ZERO,
            drag: 0.0,
            spin: 0.0,
            grow: 0.0,
            ramp: None,
            fade_in: 0.09,
            peak: None,
            age: 0.0,
            life: 0.5,
        }
    }
}

impl Particle {
    /// Strength at the current age: up over the fade-in, then down to
    /// nothing at the end of life. With `fade_in` at zero this is the
    /// straight `1 - age/life` ramp it has always been.
    fn strength(&self) -> f32 {
        let rise = self.fade_in * self.life;
        if self.age < rise {
            self.age / rise.max(f32::EPSILON)
        } else {
            1.0 - (self.age - rise) / (self.life - rise).max(f32::EPSILON)
        }
    }

    /// How far through life, 0..1: where a colour ramp is read.
    fn progress(&self) -> f32 {
        (self.age / self.life.max(f32::EPSILON)).clamp(0.0, 1.0)
    }

    /// The colour to write this frame, given whatever the sprite is
    /// holding.
    ///
    /// The peak is taken out of the sprite exactly once and kept, and that
    /// is the whole point of this existing. The alpha written back is
    /// already a fraction of the peak, so a peak read out of the sprite
    /// again next frame is a fraction of a fraction: the fade compounds,
    /// and every particle in the game goes out several times faster than
    /// its life says. It is not subtle when it happens - the sand behind a
    /// walking crab stops appearing at all - but it is invisible in the
    /// code, because each frame on its own looks right.
    fn shade(&mut self, current: Color) -> Color {
        let peak = *self.peak.get_or_insert(current.alpha());
        let progress = self.progress();
        let base = self
            .ramp
            .map_or(current, |(from, to)| from.mix(&to, progress));
        base.with_alpha(peak * self.strength())
    }
}

/// Sand as it leaves the ground, and as it looks by the time it lands.
/// A grain thrown up off dry sand comes down on damp, and a burst that
/// held one colour all the way through read as a puff of smoke.
const SAND_DRY: Color = Color::srgba(0.9, 0.82, 0.62, 0.75);
const SAND_DAMP: Color = Color::srgba(0.62, 0.55, 0.40, 0.75);

/// A gull's feather, and the grey of the wing it came out of.
const FEATHER_LIT: Color = Color::srgba(1.0, 1.0, 1.0, 0.95);
const FEATHER_GREY: Color = Color::srgba(0.72, 0.76, 0.82, 0.95);

/// A scatter of pieces thrown out of one point: what a bank, a raid, a
/// spawn and a gull's meal all look like, differing only in these.
///
/// A struct rather than eight positional arguments, because six of them
/// are floats and the two that matter most (`size` and `speed`) read the
/// same way round.
pub struct Burst {
    pub image: Handle<Image>,
    pub pos: Vec2,
    pub color: Color,
    pub count: usize,
    pub size: f32,
    /// Pixels per second, scattered outward; each piece draws between 40%
    /// and 100% of it.
    pub speed: f32,
    /// Downward pull. Sand and feathers fall back; spray and foam do not.
    pub gravity: f32,
}

/// Throw a [`Burst`] of pieces.
pub fn burst(commands: &mut Commands, rng: &mut VisualRng, spec: &Burst) {
    for _ in 0..spec.count {
        let angle = rng.range(0.0, std::f32::consts::TAU);
        let velocity = Vec2::from_angle(angle) * rng.range(spec.speed * 0.4, spec.speed);
        commands.spawn((
            Particle {
                velocity,
                gravity: Vec2::new(0.0, -spec.gravity),
                drag: 1.1,
                spin: rng.range(-4.0, 4.0),
                grow: rng.range(0.2, 0.7),
                life: rng.range(0.35, 0.6),
                ..default()
            },
            Sprite {
                image: spec.image.clone(),
                color: spec.color,
                custom_size: Some(Vec2::splat(spec.size * rng.range(0.7, 1.3))),
                ..default()
            },
            Transform::from_translation(spec.pos.extend(layout::z::PARTICLE)),
        ));
    }
}

/// What a gull leaves of a crab: feathers, lit as they fly and the grey of
/// the wing they came out of by the time they land.
///
/// The one place a burst's pieces change colour on the way, and the reason
/// [`Particle::ramp`] exists.
fn feathers(commands: &mut Commands, rng: &mut VisualRng, art: &Art, pos: Vec2) {
    for _ in 0..5 {
        let angle = rng.range(0.0, std::f32::consts::TAU);
        commands.spawn((
            Particle {
                velocity: Vec2::from_angle(angle) * rng.range(18.0, 46.0),
                gravity: Vec2::new(0.0, -90.0),
                drag: 1.6,
                spin: rng.range(-5.0, 5.0),
                ramp: Some((FEATHER_LIT, FEATHER_GREY)),
                life: rng.range(0.45, 0.8),
                ..default()
            },
            Sprite {
                image: art.feather.clone(),
                color: FEATHER_LIT,
                custom_size: Some(Vec2::splat(rng.range(11.0, 18.0))),
                ..default()
            },
            Transform::from_translation(pos.extend(layout::z::PARTICLE)),
        ));
    }
}

/// A shockwave out of one tile: a bright circle swelling and thinning.
///
/// The shape every "something happened *here*" gets, so a tier-up, a post
/// going in and a gull touching down are read as the same kind of news at
/// different volumes.
pub fn ring(
    commands: &mut Commands,
    art: &Art,
    pos: Vec2,
    color: Color,
    size: f32,
    grow: f32,
    life: f32,
) {
    commands.spawn((
        Particle {
            grow,
            life,
            fade_in: 0.0,
            ..default()
        },
        Sprite {
            image: art.ring.clone(),
            color,
            custom_size: Some(Vec2::splat(size)),
            ..default()
        },
        Transform::from_translation(pos.extend(layout::z::PARTICLE)),
    ));
}

/// A single glint star that twinkles out in place.
pub fn glint(commands: &mut Commands, rng: &mut VisualRng, art: &Art, pos: Vec2, color: Color) {
    let offset = Vec2::new(rng.range(-14.0, 14.0), rng.range(-14.0, 14.0));
    commands.spawn((
        Particle {
            velocity: Vec2::new(0.0, rng.range(4.0, 10.0)),
            spin: rng.range(1.0, 3.0),
            grow: -0.8,
            life: rng.range(0.4, 0.7),
            ..default()
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

/// Two sparks shed behind something worth following.
///
/// `back` points away from where it is going, so the ribbon lies along the
/// path it came by rather than scattering round it: at a glance the trail
/// is a heading, not a halo.
pub fn spark_trail(commands: &mut Commands, rng: &mut VisualRng, art: &Art, pos: Vec2, back: Vec2) {
    for _ in 0..2 {
        let spread = back.perp() * rng.range(-7.0, 7.0);
        commands.spawn((
            Particle {
                velocity: back * rng.range(10.0, 26.0),
                drag: 2.4,
                spin: rng.range(-5.0, 5.0),
                grow: -0.9,
                life: rng.range(0.35, 0.6),
                ..default()
            },
            Sprite {
                image: art.star.clone(),
                color: Color::srgba(1.0, 0.92, 0.45, 0.9),
                custom_size: Some(Vec2::splat(rng.range(6.0, 11.0))),
                ..default()
            },
            Transform::from_translation((pos + spread).extend(layout::z::PARTICLE)),
        ));
    }
}

/// Floating score text ("+3", "-12") rising from a board position.
pub fn score_pip(commands: &mut Commands, text: String, pos: Vec2, color: Color) {
    commands.spawn((
        Particle {
            velocity: Vec2::new(0.0, 46.0),
            // Thrown up and slowing: the number arrives, hangs, and is
            // gone, rather than sliding at one speed off the top.
            gravity: Vec2::new(0.0, -34.0),
            life: 1.2,
            fade_in: 0.0,
            ..default()
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
                velocity: Vec2::new(rng.range(-18.0, 18.0), rng.range(-90.0, -40.0)),
                // Falling under its own weight against a little air, so
                // the pieces spread as they come down rather than dropping
                // on parallel rails.
                gravity: Vec2::new(0.0, -55.0),
                drag: 0.55,
                spin: rng.range(-6.0, 6.0),
                life: rng.range(1.6, 3.0),
                ..default()
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

/// A crab on its last quarter-second: the sim has stopped counting it, and
/// the render layer walks it the rest of the way into the keep.
///
/// The bank is the game's most-repeated reward and it used to be a
/// disappearance with a puff over it. Here the crab is seen to arrive.
#[derive(Component)]
pub struct Hop {
    from: Vec2,
    to: Vec2,
    age: f32,
}

/// How long a crab is in the air on its way into the keep. Short: the
/// score has already moved, and a slow hop would read as lag.
const HOP: f32 = 0.19;

/// Send a banked crab home over the wall.
pub fn bank_hop(
    commands: &mut Commands,
    art: &Art,
    id: u32,
    from: Vec2,
    to: Vec2,
    kind: crate::sim::CrabKind,
) {
    let size = crate::app::creatures::body_size(kind);
    commands.spawn((
        Hop { from, to, age: 0.0 },
        Sprite {
            image: art.crab.clone(),
            color: crate::app::creatures::shell_color(kind, crate::app::creatures::shade_of(id)),
            custom_size: Some(size * 1.25),
            ..default()
        },
        Transform::from_translation(from.extend(layout::z::CREATURE + 0.1)),
    ));
}

/// Where a banking crab is, and how big it looks, `progress` of the way
/// from the tile it vanished on to the keep it vanished into.
///
/// Split out and tested for the same reason the castles' own hop is: the
/// two ends are the whole of it. A crab that starts anywhere but where it
/// was last drawn jumps on its first frame, and one that finishes anywhere
/// but at the gate is last seen standing on the wall.
fn arc(from: Vec2, to: Vec2, progress: f32) -> (Vec2, f32) {
    // Eased both ends, so it sets off and arrives rather than being
    // dragged across.
    let eased = progress * progress * (3.0 - 2.0 * progress);
    // Over the wall: zero lift at both ends, so the path meets the sand at
    // one end and the gate at the other.
    let lift = (progress * std::f32::consts::PI).sin() * 12.0;
    // Shrinking into the gate: the keep is a hole in the picture as far as
    // the crab is concerned.
    (
        from.lerp(to, eased) + Vec2::new(0.0, lift),
        1.0 - 0.7 * eased,
    )
}

/// Carry the hopping crabs over the wall and puff them out of sight.
pub fn advance_hops(
    time: Res<Time>,
    mut commands: Commands,
    art: Res<Art>,
    mut rng: ResMut<VisualRng>,
    mut hops: Query<(Entity, &mut Hop, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut hop, mut transform) in &mut hops {
        hop.age += dt;
        let progress = (hop.age / HOP).clamp(0.0, 1.0);
        if progress >= 1.0 {
            commands.entity(entity).despawn();
            // The sand it kicks up going over the wall, landing where it
            // landed rather than where it left.
            burst(
                &mut commands,
                &mut rng,
                &Burst {
                    image: art.puff.clone(),
                    pos: hop.to,
                    color: Color::srgba(0.95, 0.88, 0.7, 1.0),
                    count: 4,
                    size: 14.0,
                    speed: 40.0,
                    gravity: 40.0,
                },
            );
            continue;
        }
        let (at, scale) = arc(hop.from, hop.to, progress);
        transform.translation.x = at.x;
        transform.translation.y = at.y;
        transform.scale = Vec3::splat(scale);
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
    mut seen: Local<bevy::platform::collections::HashSet<u32>>,
) {
    if settings.reduced_motion {
        return;
    }
    const STRIDE: f32 = 20.0;
    let board = &sim.0;
    seen.clear();
    for crab in board.crabs() {
        seen.insert(crab.id);
        let pos = layout::creature_pos(board, crab.tile, crab.dir, crab.progress);
        // A crab seen for the first time lands on a zero-length step
        // below, which the stride check skips.
        let fall = footfalls.entry(crab.id).or_insert(Footfall {
            last: pos,
            foot: Foot::Right,
        });
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
                life: rng.range(1.4, 2.2),
                // A print is pressed into the sand, not dropped on it: it
                // arrives over a moment as the weight goes on.
                fade_in: 0.05,
                ..default()
            },
            Sprite::from_color(
                // Near-opaque, not the 0.38 this was written with. The
                // spawn alpha used to be discarded, so a print rendered at
                // full strength whatever it asked for; now that the alpha
                // is honoured (see `Particle::shade`) asking for 0.38
                // would quietly dim every print in the game against the
                // beach it shipped with.
                Color::srgba(0.45, 0.36, 0.24, 0.9),
                Vec2::splat(rng.range(3.0, 4.2)),
            ),
            Transform::from_translation((pos + side).extend(layout::z::WET + 0.05)),
        ));
        // Now and then a grain of sand kicks up behind the crab.
        if rng.range(0.0, 1.0) < 0.3 {
            commands.spawn((
                Particle {
                    velocity: -step.normalize_or_zero() * rng.range(12.0, 26.0),
                    gravity: Vec2::new(0.0, -60.0),
                    spin: rng.range(-3.0, 3.0),
                    grow: 0.5,
                    ramp: Some((SAND_DRY, SAND_DAMP)),
                    life: rng.range(0.25, 0.4),
                    ..default()
                },
                Sprite {
                    image: art.puff.clone(),
                    color: SAND_DRY,
                    custom_size: Some(Vec2::splat(rng.range(5.0, 8.0))),
                    ..default()
                },
                Transform::from_translation(pos.extend(layout::z::PARTICLE)),
            ));
        }
    }
    // A set, not a list. This sweeps one entry per crab and asks after
    // each, so a linear scan makes it quadratic on the busiest boards -
    // and a busy six-seat beach carries well over a hundred crabs, every
    // frame, purely to take out the ones that have gone.
    footfalls.retain(|id, _| seen.contains(id));
}

/// Advance and expire particles: drift, fall, spin, grow, fade out.
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
        // Read out first: `Mut` cannot hand out a shared and a unique
        // borrow of the same particle in one expression.
        let (gravity, drag) = (particle.gravity, particle.drag);
        particle.velocity += gravity * dt;
        if drag > 0.0 {
            particle.velocity *= (1.0 - drag * dt).clamp(0.0, 1.0);
        }
        let strength = particle.strength();
        transform.translation.x += particle.velocity.x * dt;
        transform.translation.y += particle.velocity.y * dt;
        transform.rotation *= Quat::from_rotation_z(particle.spin * dt);
        let scale = (1.0 + particle.grow * particle.age).max(0.05);
        transform.scale = Vec3::splat(scale);
        if let Some(mut sprite) = sprite {
            // The colour spawned with is the peak, alpha included: a piece
            // asked for at 60% never draws stronger than 60%.
            sprite.color = particle.shade(sprite.color);
        }
        if let Some(mut color) = text_color {
            color.0 = color.0.with_alpha(strength);
        }
    }
}

/// Turn sim events into their on-board moments: pips, puffs, flashes, and
/// the shove the camera takes for the loud ones.
#[allow(clippy::too_many_arguments)]
pub fn moment_effects(
    mut commands: Commands,
    mut events: MessageReader<SimEvent>,
    art: Res<Art>,
    cursors: Query<&crate::app::cursor::Cursor>,
    settings: Res<crate::app::settings::GameSettings>,
    mut rng: ResMut<VisualRng>,
    mut trauma: ResMut<Trauma>,
) {
    // Reduced motion keeps the news and drops the fireworks: the score pip
    // for a raid still floats up (it is the only place that number appears),
    // but the puffs, the scatter, the shake and the full-tile white flash
    // do not.
    let calm = settings.reduced_motion;
    for event in events.read() {
        if calm && !matches!(event, SimEvent::CastleRaided { .. }) {
            continue;
        }
        match event {
            // Walked home over the wall, and the puff waits for the
            // landing; a crab that vanished at the threshold was the one
            // moment the game repeated most and showed least.
            SimEvent::CrabBanked {
                id,
                pos,
                keep,
                kind,
                ..
            } => bank_hop(&mut commands, &art, *id, *pos, *keep, *kind),
            // A gull got one: sand, and the feathers that say what kind of
            // ending this was.
            SimEvent::CrabEaten { pos } => {
                burst(
                    &mut commands,
                    &mut rng,
                    &Burst {
                        image: art.puff.clone(),
                        pos: *pos,
                        color: Color::srgba(0.85, 0.92, 0.98, 1.0),
                        count: 5,
                        size: 12.0,
                        speed: 55.0,
                        gravity: 0.0,
                    },
                );
                feathers(&mut commands, &mut rng, &art, *pos);
                trauma.add(0.10);
            }
            // The ghost of the signpost that just got traded away, still
            // pointing where it pointed, swelling as it fades. Under the
            // versus cap a fourth placement takes your oldest, and it used
            // to vanish in silence somewhere else on the beach: the player
            // sees the arrow they lost, and where they lost it.
            SimEvent::SignpostEvicted { owner, pos, dir } => {
                commands.spawn((
                    Particle {
                        grow: 1.1,
                        life: 0.5,
                        fade_in: 0.0,
                        ..default()
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
                &Burst {
                    image: art.puff.clone(),
                    pos: *pos,
                    color: Color::srgba(0.9, 0.8, 0.6, 1.0),
                    count: 3,
                    size: 12.0,
                    speed: 35.0,
                    gravity: 50.0,
                },
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
                        grow: 1.6,
                        life: 0.3,
                        fade_in: 0.0,
                        ..default()
                    },
                    Sprite {
                        image: art.puff.clone(),
                        color: Color::srgba(1.0, 1.0, 1.0, 1.0),
                        custom_size: Some(Vec2::splat(TILE * 1.1)),
                        ..default()
                    },
                    Transform::from_translation(pos.extend(layout::z::FLASH)),
                ));
                burst(
                    &mut commands,
                    &mut rng,
                    &Burst {
                        image: art.puff.clone(),
                        pos: *pos,
                        color: Color::srgba(0.9, 0.8, 0.6, 1.0),
                        count: 8,
                        size: 16.0,
                        speed: 70.0,
                        gravity: 60.0,
                    },
                );
                ring(
                    &mut commands,
                    &art,
                    *pos,
                    Color::srgba(1.0, 0.5, 0.4, 0.85),
                    TILE * 0.7,
                    3.4,
                    0.45,
                );
                score_pip(
                    &mut commands,
                    format!("-{lost}"),
                    *pos + Vec2::new(0.0, 10.0),
                    Color::srgb(0.96, 0.3, 0.25),
                );
                // The loudest thing that happens to a player who is not
                // looking at their own castle.
                trauma.add(0.55);
            }
            SimEvent::GullLanded { pos } => {
                burst(
                    &mut commands,
                    &mut rng,
                    &Burst {
                        image: art.puff.clone(),
                        pos: *pos,
                        color: Color::srgba(0.92, 0.85, 0.68, 1.0),
                        count: 4,
                        size: 13.0,
                        speed: 45.0,
                        gravity: 70.0,
                    },
                );
                ring(
                    &mut commands,
                    &art,
                    *pos,
                    Color::srgba(0.95, 0.92, 0.85, 0.5),
                    TILE * 0.5,
                    2.2,
                    0.35,
                );
                trauma.add(0.14);
            }
            // A post going in gets a ring under it; the pop of the post
            // itself belongs to the sprite, which `board_render` owns.
            //
            // Only your own, for the reason the knock is only your own: on
            // a six-seat beach five bots place and lose a post every few
            // seconds each, and a ring for every one of them is a stream
            // of particles over tiles nobody is watching. The picture and
            // the sound have to agree about whose beach this is.
            SimEvent::SignpostPlaced { owner, pos }
                if crate::app::cursor::seated_here(&cursors, *owner) =>
            {
                ring(
                    &mut commands,
                    &art,
                    *pos,
                    Color::srgba(1.0, 0.98, 0.9, 0.5),
                    TILE * 0.45,
                    1.9,
                    0.3,
                );
            }
            SimEvent::SignpostRemoved { owner, pos }
                if crate::app::cursor::seated_here(&cursors, *owner) =>
            {
                burst(
                    &mut commands,
                    &mut rng,
                    &Burst {
                        image: art.puff.clone(),
                        pos: *pos,
                        color: Color::srgba(0.92, 0.86, 0.72, 1.0),
                        count: 3,
                        size: 11.0,
                        speed: 30.0,
                        gravity: 50.0,
                    },
                );
            }
            // Somebody else's post: the board already shows it appearing
            // and going, which is all a rival's arrow owes anybody.
            SimEvent::SignpostPlaced { .. } | SimEvent::SignpostRemoved { .. } => {}
            // The final scramble arrives with the sea behind it.
            SimEvent::SurgeStarted => trauma.add(0.7),
            SimEvent::GullArrived
            | SimEvent::GullTookOff
            | SimEvent::TierUp { .. }
            | SimEvent::TideEventFired { .. }
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

    /// A particle's strength has to start at nothing, reach exactly full
    /// at the top of the fade-in, and reach exactly nothing at the end of
    /// life. The ends are what a reader sees: a particle that starts at
    /// half strength pops, and one that ends at half strength blinks out.
    #[test]
    fn a_particle_fades_in_and_out_of_its_whole_life() {
        let at = |age: f32, fade_in: f32| {
            Particle {
                age,
                life: 1.0,
                fade_in,
                ..Default::default()
            }
            .strength()
        };
        assert_eq!(at(0.0, 0.2), 0.0, "born at nothing");
        assert!((at(0.2, 0.2) - 1.0).abs() < 1e-6, "full at the top");
        assert!((at(0.6, 0.2) - 0.5).abs() < 1e-6, "half way down");
        assert!(at(1.0, 0.2).abs() < 1e-6, "gone at the end");
        // With no fade-in it is the plain ramp it has always been.
        assert!((at(0.0, 0.0) - 1.0).abs() < 1e-6);
        assert!((at(0.25, 0.0) - 0.75).abs() < 1e-6);
    }

    /// The fade must be a fraction of the colour a particle was *spawned*
    /// with, never of the colour it is currently wearing. Shading twice at
    /// one age has to land twice in the same place; when it did not, the
    /// alpha compounded every frame and the whole particle layer - sand,
    /// footprints, glints, confetti - went out too fast to be seen. The
    /// symptom was a beach where crabs left no prints.
    #[test]
    fn shading_a_particle_twice_lands_in_the_same_place() {
        let mut particle = Particle {
            life: 1.0,
            fade_in: 0.0,
            age: 0.25,
            ..Default::default()
        };
        let spawn = Color::srgba(1.0, 0.9, 0.7, 0.8);
        let once = particle.shade(spawn);
        assert!(
            (once.alpha() - 0.6).abs() < 1e-6,
            "a peak of 0.8 at three-quarter strength: {}",
            once.alpha()
        );
        let twice = particle.shade(once);
        assert!(
            (twice.alpha() - once.alpha()).abs() < 1e-6,
            "the peak is the spawn alpha for ever, not what was written last: {} then {}",
            once.alpha(),
            twice.alpha()
        );
    }

    /// A ramped particle takes its colour from the ramp and its strength
    /// from the peak, so a feather thrown white and landing grey still
    /// fades out rather than holding at the grey.
    #[test]
    fn a_ramp_moves_the_colour_without_touching_the_fade() {
        let mut particle = Particle {
            life: 1.0,
            fade_in: 0.0,
            age: 1.0,
            ramp: Some((FEATHER_LIT, FEATHER_GREY)),
            ..Default::default()
        };
        let end = particle.shade(FEATHER_LIT);
        let grey = FEATHER_GREY.to_srgba();
        assert!(
            (end.to_srgba().blue - grey.blue).abs() < 1e-3,
            "arrived grey"
        );
        assert!(end.alpha() < 1e-6, "and gone at the end of life");
    }

    /// A kicked grain of sand has to come back down. Gravity is what
    /// separates a puff of sand from a puff of smoke, and it was not there
    /// at all until the particle layer grew one.
    #[test]
    fn gravity_brings_a_particle_back_down() {
        let mut particle = Particle {
            velocity: Vec2::new(0.0, 40.0),
            gravity: Vec2::new(0.0, -90.0),
            life: 2.0,
            ..Default::default()
        };
        let mut height = 0.0;
        let mut peak: f32 = 0.0;
        for _ in 0..60 {
            particle.velocity += particle.gravity * (1.0 / 60.0);
            height += particle.velocity.y * (1.0 / 60.0);
            peak = peak.max(height);
        }
        assert!(peak > 0.0, "it went up first: {peak}");
        assert!(height < peak, "and came back down: {height} under {peak}");
    }

    /// Drag slows a piece without ever turning it round. A frame long
    /// enough to drive the multiplier negative would send sand flying back
    /// into the crab that kicked it, which is why the clamp is there.
    #[test]
    fn drag_slows_a_particle_without_reversing_it() {
        for drag in [0.5f32, 1.1, 2.4, 40.0] {
            for dt in [1.0f32 / 240.0, 1.0 / 60.0, 0.5, 2.0] {
                let mut velocity = Vec2::new(30.0, -12.0);
                let before = velocity;
                velocity *= (1.0 - drag * dt).clamp(0.0, 1.0);
                assert!(
                    velocity.length() <= before.length() + 1e-6,
                    "drag {drag} at dt {dt} sped it up"
                );
                assert!(
                    velocity.x >= 0.0 && velocity.y <= 0.0,
                    "drag {drag} at dt {dt} turned it round: {velocity:?}"
                );
            }
        }
    }

    /// A print is put down with alternating claws, and the sides have to
    /// be opposite: both on one side and a crab walks a single rut instead
    /// of leaving a trail.
    #[test]
    fn the_claws_alternate_and_fall_on_opposite_sides() {
        assert_eq!(Foot::Left.other(), Foot::Right);
        assert_eq!(Foot::Right.other(), Foot::Left);
        assert_eq!(
            Foot::Left.other().other(),
            Foot::Left,
            "two steps is a cycle"
        );
        assert!(
            Foot::Left.side() * Foot::Right.side() < 0.0,
            "the two claws fall on the same side of the stride"
        );
    }

    /// A banking crab has to leave from exactly where it was last drawn
    /// and arrive at exactly the gate: anything else is a jump on the
    /// first frame or a crab left standing on the wall on the last. And it
    /// has to be off the ground in between, which is the only thing
    /// saying it went *over* rather than *through*.
    #[test]
    fn a_bank_starts_on_the_sand_and_finishes_in_the_gate() {
        let (from, to) = (Vec2::new(-120.0, 30.0), Vec2::new(64.0, 64.0));
        let (start, start_scale) = arc(from, to, 0.0);
        assert_eq!(start, from);
        assert!((start_scale - 1.0).abs() < 1e-6, "full size on the sand");
        let (end, end_scale) = arc(from, to, 1.0);
        assert!(end.distance(to) < 1e-3, "{end:?} vs {to:?}");
        assert!(end_scale < 0.35, "and all but gone: {end_scale}");
        let (middle, _) = arc(from, to, 0.5);
        assert!(
            middle.y > from.lerp(to, 0.5).y,
            "off the ground in between: {middle:?}"
        );
    }

    /// A banking crab travels straight to the gate, never bowing sideways
    /// off it. The castles' own hop is asserted the same way and for the
    /// same reason: a path that bowed would carry the crab over a wall on
    /// a corner-to-corner bank, through terrain it never touched.
    #[test]
    fn a_bank_travels_in_a_straight_line() {
        let (from, to) = (Vec2::new(0.0, 0.0), Vec2::new(400.0, 200.0));
        for step in 0..=10 {
            let (at, _) = arc(from, to, step as f32 / 10.0);
            // Every point is on the segment once the lift is taken back
            // off: y is exactly half of x, which is what the two ends say.
            let lift = (step as f32 / 10.0 * std::f32::consts::PI).sin() * 12.0;
            assert!(
                (at.y - lift - at.x * 0.5).abs() < 1e-3,
                "bowed off the line at {step}: {at:?}"
            );
        }
    }

    /// Trauma is a pool, not a queue: four raids in one frame must not
    /// throw the camera four times as far, and it has to drain to exactly
    /// zero rather than to a fraction that shakes for ever.
    #[test]
    fn trauma_is_capped_and_drains_away() {
        let mut trauma = Trauma::default();
        for _ in 0..8 {
            trauma.add(0.55);
        }
        assert_eq!(trauma.0, Trauma::MAX, "the pool has a ceiling");
        let thrown = trauma.offset(0.016, 0.4);
        assert!(thrown.length() > 0.0, "a full pool moves the camera");
        assert!(
            thrown.length() <= Trauma::THROW * std::f32::consts::SQRT_2,
            "and never further than the throw allows: {thrown:?}"
        );
        for _ in 0..200 {
            trauma.offset(0.016, 0.4);
        }
        assert_eq!(trauma.0, 0.0, "and it comes to rest");
        assert_eq!(trauma.offset(0.016, 0.4), Vec2::ZERO);
    }
}
