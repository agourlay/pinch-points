//! Particles and the one-shot effects built from them: bursts, crumbs,
//! feathers, rings, glints and confetti, on the render layer's own PRNG.

use super::*;

/// Smoothstep: 0 below 0, 1 above 1, and an S between that leaves and
/// arrives at rest. The easing every flourish on the beach starts and
/// stops by.
pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
    /// Seeded from the wall clock, for variety that should differ from one
    /// launch to the next rather than open the same way every time.
    pub(crate) fn from_clock() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.subsec_nanos() ^ since.as_secs() as u32);
        VisualRng(nanos | 1)
    }

    /// A fixed seed: the same draws every run, for tests and for scenery
    /// that should look the same every time its board is played.
    pub(crate) fn seeded(seed: u32) -> Self {
        VisualRng(seed)
    }

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
    pub(super) velocity: Vec2,
    /// Pixels per second squared. What makes a kicked grain of sand fall
    /// back to the beach instead of sailing off the board.
    pub(super) gravity: Vec2,
    /// Fraction of speed shed per second; 0.0 coasts for ever.
    pub(super) drag: f32,
    /// Radians per second.
    pub(super) spin: f32,
    /// Scale multiplier per second (1.0 = constant size).
    pub(super) grow: f32,
    /// The scale it is drawn at before growing. 1.0 for everything but
    /// text, which is rasterized larger than it is shown (see
    /// [`score_pip`]).
    pub(super) size: f32,
    /// A colour to drift to over life, and the one drifted from. `None`
    /// keeps the spawn colour and moves only its alpha.
    pub(super) ramp: Option<(Color, Color)>,
    /// Fraction of life spent fading in. Nothing should arrive at full
    /// strength on the frame it is born: at 60 fps that is a pop.
    pub(super) fade_in: f32,
    /// The alpha this was spawned at, read out of the sprite on the first
    /// frame and kept.
    ///
    /// It is the ceiling: a piece asked for at 60% never draws stronger
    /// than 60%, so a caller that wants full strength asks for 1.0. See
    /// [`Particle::shade`] for why it is read once and kept.
    pub(super) peak: Option<f32>,
    /// The height it comes to rest at, falling: a grain of sand off a
    /// castle lies on the beach at the castle's foot rather than falling
    /// through it. `None` falls for ever.
    pub(super) floor: Option<f32>,
    /// Fraction of life held at full strength before the fade begins, for
    /// a piece that lies a moment before it goes. Zero fades all the way.
    pub(super) hold: f32,
    pub(super) age: f32,
    pub(super) life: f32,
}

impl Default for Particle {
    fn default() -> Self {
        Particle {
            velocity: Vec2::ZERO,
            gravity: Vec2::ZERO,
            drag: 0.0,
            spin: 0.0,
            grow: 0.0,
            size: 1.0,
            ramp: None,
            fade_in: 0.09,
            peak: None,
            floor: None,
            hold: 0.0,
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
        let held = self.hold * self.life;
        if held > 0.0 {
            return match self.age < held {
                true => 1.0,
                false => 1.0 - (self.age - held) / (self.life - held).max(f32::EPSILON),
            };
        }
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
    /// The peak is taken out of the sprite exactly once and kept, which is
    /// the whole point of this existing. The alpha written back is already
    /// a fraction of the peak, so reading it out again next frame is a
    /// fraction of a fraction: the fade compounds, and every particle goes
    /// out several times faster than its life says, while each frame on
    /// its own looks right.
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
pub(super) const SAND_DRY: Color = Color::srgba(0.9, 0.82, 0.62, 0.75);
pub(super) const SAND_DAMP: Color = Color::srgba(0.62, 0.55, 0.40, 0.75);

/// A gull's feather, and the grey of the wing it came out of.
const FEATHER_LIT: Color = Color::srgba(1.0, 1.0, 1.0, 0.95);
const FEATHER_GREY: Color = Color::srgba(0.72, 0.76, 0.82, 0.95);

/// A scatter of pieces thrown out of one point: what a bank, a raid, a
/// spawn and a gull's meal all look like, differing only in these.
///
/// A struct rather than eight positional arguments, six of them floats.
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
            layout::image_sprite(
                &spec.image,
                spec.color,
                Vec2::splat(spec.size * rng.range(0.7, 1.3)),
            ),
            Transform::from_translation(spec.pos.extend(layout::z::PARTICLE)),
        ));
    }
}

/// A handful of grains knocked off a raided castle (see [`crumbs`]).
#[derive(Clone, Copy)]
pub(crate) struct Crumbs {
    /// Where they fall from: the keep's rim, over its middle.
    pub rim: Vec2,
    /// The height of the castle's foot, where they come to rest.
    pub foot: f32,
    /// The castle's own sand, which they are a little darker than.
    pub body: Color,
    pub count: usize,
    /// Seconds from falling to gone.
    pub life: f32,
}

/// Grains of sand knocked off a raided castle's rim: they drop from it,
/// scattered a little either side and tossed up, fall to the castle's
/// foot, lie there a moment and sink away, each a piece of the castle's
/// own sand.
pub(crate) fn crumbs(commands: &mut Commands, rng: &mut VisualRng, art: &Art, spec: &Crumbs) {
    let Crumbs {
        rim,
        foot,
        body,
        count,
        life,
    } = *spec;
    let body = body.to_srgba();
    for _ in 0..count {
        let side = if rng.range(0.0, 1.0) < 0.5 { -1.0 } else { 1.0 };
        let from = rim + Vec2::new(side * rng.range(4.3, 13.9), -rng.range(0.0, 5.3));
        let dark = rng.range(0.70, 0.86);
        commands.spawn((
            Particle {
                velocity: Vec2::new(side * rng.range(10.7, 32.0), rng.range(10.7, 37.3)),
                gravity: Vec2::new(0.0, -363.0),
                spin: rng.range(-3.0, 3.0),
                fade_in: 0.0,
                floor: Some(foot + rng.range(-8.5, 3.2)),
                hold: 0.55,
                life,
                ..default()
            },
            layout::image_sprite(
                &art.puff,
                Color::srgb(body.red * dark, body.green * dark, body.blue * dark),
                Vec2::splat(rng.range(4.5, 6.6)),
            ),
            // On the castle's tile, in front of it, under whatever walks
            // past.
            Transform::from_translation(from.extend(layout::z::TILE_FEATURE + 0.7)),
        ));
    }
}

/// What a gull leaves of a crab: feathers, lit as they fly and the grey of
/// the wing they came out of by the time they land.
///
/// The one place a burst's pieces change colour on the way, and what
/// [`Particle::ramp`] is for.
pub(super) fn feathers(commands: &mut Commands, rng: &mut VisualRng, art: &Art, pos: Vec2) {
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
            layout::image_sprite(
                &art.feather,
                FEATHER_LIT,
                Vec2::splat(rng.range(11.0, 18.0)),
            ),
            Transform::from_translation(pos.extend(layout::z::PARTICLE)),
        ));
    }
}

/// A gull a castle turned away, beating off the beach the way it faces
/// from the keep: a pale knock on the wall, and the bird climbing out,
/// swelling as it rises toward the camera and fading as it goes.
///
/// `calm` keeps the bird and drops the flight. It still fades where it
/// stood, which is the news: vanishing on the spot is what a raid does.
pub(super) fn shooed(commands: &mut Commands, art: &Art, pos: Vec2, castle: Vec2, calm: bool) {
    let away = (pos - castle).try_normalize().unwrap_or(Vec2::Y);
    // Up the screen as well as out, so a bird leaving downward still
    // reads as taking off rather than as walking on.
    let heading = (away + Vec2::Y * 0.6).normalize();
    if !calm {
        ring(
            commands,
            art,
            castle,
            Color::srgba(0.95, 0.97, 1.0, 0.55),
            TILE * 0.55,
            2.0,
            0.3,
        );
    }
    flying_away(commands, art, pos, heading, calm);
}

/// A gull in the air leaving the beach from `pos` along `heading`, or,
/// with reduced motion, fading where it stood.
pub(super) fn flying_away(
    commands: &mut Commands,
    art: &Art,
    pos: Vec2,
    heading: Vec2,
    calm: bool,
) {
    commands.spawn((
        Particle {
            velocity: if calm { Vec2::ZERO } else { heading * 150.0 },
            grow: if calm { 0.0 } else { 0.6 },
            fade_in: 0.0,
            life: 0.75,
            ..default()
        },
        layout::image_sprite(&art.gull_fly, Color::WHITE, Vec2::splat(TILE * 0.78)),
        Transform::from_translation(pos.extend(layout::z::PARTICLE))
            .with_rotation(Quat::from_rotation_z(heading.to_angle())),
    ));
}

/// A shockwave out of one tile: a bright circle swelling and thinning.
///
/// The shape every "something happened *here*" gets, so a tier-up, a post
/// going in and a gull touching down read as one kind of news at different
/// volumes.
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
            layout::image_sprite(
                &art.star,
                Color::srgba(1.0, 0.92, 0.45, 0.9),
                Vec2::splat(rng.range(6.0, 11.0)),
            ),
            Transform::from_translation((pos + spread).extend(layout::z::PARTICLE)),
        ));
    }
}

/// Floating score text ("+3", "-12") rising from a board position.
///
/// Text on the board is rasterized at its font size in screen pixels and
/// then magnified with the board, so a pip on a zoomed-in beach came out a
/// blur. It is drawn [`layout::MAX_ZOOM`] times larger and shown at that
/// fraction of the scale: sharp at the closest the camera ever gets, and
/// merely downsampled at every other zoom.
pub fn score_pip(commands: &mut Commands, text: String, pos: Vec2, color: Color) {
    let oversample = layout::MAX_ZOOM;
    commands.spawn((
        Particle {
            velocity: Vec2::new(0.0, 46.0),
            // Thrown up and slowing: the number arrives, hangs, and is
            // gone, rather than sliding at one speed off the top.
            gravity: Vec2::new(0.0, -34.0),
            life: 1.2,
            fade_in: 0.0,
            size: 1.0 / oversample,
            ..default()
        },
        Text2d::new(text),
        crate::app::menu_ui::display_font(22.0 * oversample),
        TextColor(color),
        Transform::from_translation(pos.extend(layout::z::PIP))
            .with_scale(Vec3::splat(1.0 / oversample)),
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
            layout::image_sprite(
                &art.star,
                color.lighter(rng.range(0.0, 0.2)),
                Vec2::splat(rng.range(8.0, 16.0)),
            ),
            Transform::from_translation(Vec3::new(x, y, layout::z::CONFETTI)),
        ));
    }
}

/// A particle and whichever of the three ways it is drawn: a sprite, a
/// floating number, or a quad of a shared fading colour
/// ([`crate::app::materials::FadeMaterial`]) whose strength is its tag.
type ParticleParts = (
    Entity,
    &'static mut Particle,
    &'static mut Transform,
    Option<&'static mut Sprite>,
    Option<&'static mut TextColor>,
    Option<&'static mut bevy::mesh::MeshTag>,
);

/// Advance and expire particles: drift, fall, spin, grow, fade out.
///
/// Held, not run, under a pause card, like everything else the round
/// draws: a puff, a crumb or a floating score stands still over a frozen
/// beach and carries on when it starts again.
pub fn update_particles(
    time: Res<Time>,
    round: crate::app::round_state::RoundState,
    mut commands: Commands,
    mut particles: Query<ParticleParts>,
) {
    if round.held() {
        return;
    }
    let dt = time.delta_secs();
    for (entity, mut particle, mut transform, sprite, text_color, tag) in &mut particles {
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
        transform.translation.x += particle.velocity.x * dt;
        transform.translation.y += particle.velocity.y * dt;
        transform.rotation *= Quat::from_rotation_z(particle.spin * dt);
        // Come to rest on the floor, falling onto it: it stays where it
        // lands, and only fades from there.
        if let Some(floor) = particle.floor
            && particle.velocity.y < 0.0
            && transform.translation.y <= floor
        {
            transform.translation.y = floor;
            particle.velocity = Vec2::ZERO;
            particle.gravity = Vec2::ZERO;
            particle.spin = 0.0;
        }
        let scale = (1.0 + particle.grow * particle.age).max(0.05);
        transform.scale = Vec3::splat(particle.size * scale);
        if let Some(mut sprite) = sprite {
            // The colour spawned with is the peak, alpha included: a piece
            // asked for at 60% never draws stronger than 60%.
            let shaded = particle.shade(sprite.color);
            layout::tint(&mut sprite, shaded);
        }
        if let Some(mut color) = text_color {
            // Through the same ceiling the sprites go through. It read the
            // alpha straight off the strength before, so a number asked
            // for at 60% arrived at full: the rule [`Particle::peak`]
            // states is a rule about particles, and a floating score is
            // one. No caller asks for less than full today, which is the
            // only reason nobody has seen it.
            color.0 = particle.shade(color.0);
        }
        if let Some(mut tag) = tag {
            // The colour is the material's; only the strength is the
            // quad's, and its peak was set at spawn.
            let alpha = particle.shade(Color::WHITE).alpha();
            tag.set_if_neq(crate::app::materials::fade_tag(alpha));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::stage;
    use super::*;

    /// A grain falling onto a floor stops there and lies at full strength
    /// for its hold, then fades from where it lies.
    #[test]
    fn a_grain_comes_to_rest_on_its_floor_and_lies_there() {
        let mut app = stage(0, false);
        app.init_resource::<Time>();
        app.add_systems(Update, update_particles);
        let grain = app
            .world_mut()
            .spawn((
                Particle {
                    velocity: Vec2::new(10.0, 20.0),
                    gravity: Vec2::new(0.0, -400.0),
                    fade_in: 0.0,
                    floor: Some(-10.0),
                    hold: 0.5,
                    life: 2.0,
                    ..default()
                },
                Sprite::default(),
                Transform::default(),
            ))
            .id();
        let mut landed = None;
        for _ in 0..60 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(1.0 / 60.0));
            app.update();
            let at = app
                .world()
                .get::<Transform>(grain)
                .expect("alive")
                .translation;
            assert!(at.y >= -10.0, "fell through the floor to {}", at.y);
            if at.y == -10.0 {
                match landed {
                    None => landed = Some(at.x),
                    Some(x) => assert_eq!(at.x, x, "it slid once it landed"),
                }
            }
        }
        assert!(landed.is_some(), "it never landed");
        let held = app.world().get::<Particle>(grain).expect("alive");
        assert_eq!(held.strength(), 1.0, "still lying at full strength");
        let faded = Particle { age: 1.5, ..*held };
        assert!((faded.strength() - 0.5).abs() < 1e-5, "halfway out");
    }

    /// A footprint drawn as a fading quad shows, frame by frame, the
    /// alpha the same particle drawn as a sprite is given, in its tag,
    /// while the sprite's own colour, and so its material, would change.
    #[test]
    fn a_fading_quad_carries_the_alpha_a_sprite_would_wear() {
        let mut app = stage(0, false);
        app.init_resource::<Time>();
        app.add_systems(Update, update_particles);
        let like = || Particle {
            life: 1.0,
            fade_in: 0.05,
            peak: Some(PRINT.alpha()),
            ..default()
        };
        let quad = app
            .world_mut()
            .spawn((
                like(),
                crate::app::materials::fade_tag(PRINT.alpha()),
                Transform::default(),
            ))
            .id();
        let sprite = app
            .world_mut()
            .spawn((
                like(),
                Sprite::from_color(PRINT, Vec2::ONE),
                Transform::default(),
            ))
            .id();
        for _ in 0..8 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.1));
            app.update();
            let tag = app
                .world()
                .get::<bevy::mesh::MeshTag>(quad)
                .expect("alive")
                .value;
            let worn = app.world().get::<Sprite>(sprite).expect("alive").color;
            assert_eq!(f32::from_bits(tag), worn.alpha());
            assert!(worn.alpha() < PRINT.alpha(), "fading");
        }
    }

    /// Under a pause a particle holds where it is, as old as it was, and
    /// carries on from there once the round does.
    #[test]
    fn a_particle_holds_still_under_a_pause() {
        let mut app = stage(0, false);
        app.init_resource::<Time>();
        app.add_systems(Update, update_particles);
        let puff = app
            .world_mut()
            .spawn((
                Particle {
                    velocity: Vec2::new(30.0, 40.0),
                    life: 2.0,
                    ..default()
                },
                Sprite::default(),
                Transform::default(),
            ))
            .id();
        let step = |app: &mut App| {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.1));
            app.update();
            let age = app.world().get::<Particle>(puff).expect("alive").age;
            let at = app
                .world()
                .get::<Transform>(puff)
                .expect("alive")
                .translation;
            (age, at)
        };
        step(&mut app);
        app.world_mut().resource_mut::<crate::app::Paused>().0 = true;
        let before = step(&mut app);
        assert_eq!(step(&mut app), before, "it moved under the pause");
        app.world_mut().resource_mut::<crate::app::Paused>().0 = false;
        assert!(step(&mut app).0 > before.0, "and carries on after it");
    }

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
    /// alpha compounded every frame and crabs left no prints at all.
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

    /// Floating text goes through the same ceiling the sprites do: taken
    /// straight off the strength, a number asked for at 60% arrives at
    /// full.
    #[test]
    fn floating_text_is_held_to_the_alpha_it_asked_for() {
        let mut particle = Particle {
            life: 1.0,
            fade_in: 0.0,
            ..default()
        };
        let asked = Color::srgba(1.0, 1.0, 1.0, 0.6);
        // Freshly born, at the top of its arc, and it is still only 60%.
        let opening = particle.shade(asked);
        assert!(
            (opening.alpha() - 0.6).abs() < 1e-5,
            "arrived at {} rather than the 0.6 it asked for",
            opening.alpha()
        );
        // And it fades from there rather than to there.
        particle.age = 0.5;
        let midway = particle.shade(opening);
        assert!((midway.alpha() - 0.3).abs() < 1e-5, "{}", midway.alpha());
        particle.age = 1.0;
        assert!(particle.shade(midway).alpha() < 1e-5, "gone by the end");
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

    /// A kicked grain of sand has to come back down: gravity is what
    /// separates a puff of sand from a puff of smoke.
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
