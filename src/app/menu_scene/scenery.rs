//! The postcard behind the menu: a beach at rest.
//!
//! Bottom to top: sand furnished with the props the game is played with,
//! the waterline, a sea with a rolling swell, and sky. Crabs scuttle the
//! sand, gulls glide, clouds drift and little boats cross. None of it is
//! sim: the menu runs no board, and every animation here is decoration
//! driven by frame time.

use super::{MenuArt, VisualRng};
use crate::app::art;
use crate::app::palette;
use crate::sim::CrabKind;
use bevy::prelude::*;

/// What kind of ambient traveller is crossing the scene.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CritterKind {
    Crab,
    Gull,
    Cloud,
    Boat,
}

/// An ambient traveller: signed horizontal speed, an animation clock, its
/// kind, and the lane it bobs around.
#[derive(Component)]
pub struct MenuCritter {
    speed: f32,
    frame_clock: f32,
    kind: CritterKind,
    base_y: f32,
}

/// One short segment of a wave crest. A whole crest is a row of these, each
/// riding the same travelling sine a little further along, so the line
/// undulates instead of merely sliding.
#[derive(Component)]
pub struct WaveSegment {
    /// Resting position; `x` doubles as the phase along the crest.
    base: Vec2,
    /// Height of the swell here, in pixels.
    amp: f32,
    /// Radians per pixel along the shore: how tight the swell is.
    freq: f32,
    /// Radians per second: how fast it rolls.
    speed: f32,
    /// This crest's offset, so lanes do not rise in unison.
    phase: f32,
    /// Peak whiteness of the crest.
    brightness: f32,
}

/// The foam lip at the waterline, which breathes with the tide.
#[derive(Component)]
pub struct ShoreFoam {
    base_y: f32,
}

/// Crests between the horizon and the shore. Near ones are taller,
/// brighter and quicker; far ones flatten out toward the horizon, which is
/// the whole of the perspective trick.
const WAVE_LANES: usize = 7;

/// Segments per crest. Fine enough that the step between neighbours is
/// smaller than a segment is tall, so the row of them reads as one
/// continuous curve.
const WAVE_SEGMENTS: usize = 110;

/// The sand strip's vertical extent, from the bottom of the window.
const SHORE_H: f32 = 208.0;

/// The horizon, as a fraction of window height from the bottom: the sea
/// runs from the shore to here, sky above.
const HORIZON: f32 = 0.62;

/// Marker for the shore backdrop pieces, so a resize can rebuild them
/// without touching the rest of the menu.
#[derive(Component)]
pub struct MenuShore;

/// The whole postcard, bottom to top: sand with shells, a wet line, the
/// open sea up to the horizon, and sky above. World-space sprites behind
/// the UI (the menu camera is 1:1 with the window).
fn spawn_shore(commands: &mut Commands, art: &art::Art, rng: &mut VisualRng, window: &Window) {
    let (w, h) = (window.width(), window.height());
    let bottom = -h / 2.0;
    let horizon = bottom + h * HORIZON;
    let band = |y: f32, height: f32, color: Color, z: f32| {
        (
            MenuArt,
            MenuShore,
            Sprite::from_color(color, Vec2::new(w + 40.0, height)),
            Transform::from_translation(Vec3::new(0.0, y, z)),
        )
    };
    // Sky: a deeper azure up top fading to a pale band at the horizon.
    let sky_h = h / 2.0 - horizon;
    commands.spawn(band(
        horizon + sky_h * 0.667,
        sky_h * 0.667 * 2.0,
        Color::srgb(0.42, 0.65, 0.85),
        0.0,
    ));
    commands.spawn(band(
        horizon + sky_h * 0.167,
        sky_h * 0.334,
        Color::srgb(0.62, 0.79, 0.90),
        0.05,
    ));
    // Sea: deep at the horizon, brighter toward the shore, and a hazy
    // horizon line where they meet.
    let sea_top = SHORE_H + 18.0;
    let sea_h = (horizon - bottom) - sea_top;
    commands.spawn(band(
        horizon - sea_h * 0.25,
        sea_h * 0.5,
        Color::srgb(0.16, 0.36, 0.52),
        0.0,
    ));
    commands.spawn(band(
        horizon - sea_h * 0.75,
        sea_h * 0.5,
        Color::srgb(0.24, 0.47, 0.60),
        0.0,
    ));
    commands.spawn(band(horizon, 3.0, Color::srgb(0.72, 0.84, 0.92), 0.1));
    // The beach: dry sand, a wet line where the last wave reached.
    commands.spawn(band(
        bottom + SHORE_H / 2.0,
        SHORE_H,
        Color::srgb(0.86, 0.77, 0.57),
        0.0,
    ));
    commands.spawn(band(
        bottom + SHORE_H + 9.0,
        18.0,
        Color::srgb(0.72, 0.64, 0.47),
        0.0,
    ));
    // The foam lip where the sea meets the sand; it laps in and out.
    let foam_y = bottom + SHORE_H + 20.0;
    commands.spawn((
        MenuArt,
        MenuShore,
        ShoreFoam { base_y: foam_y },
        Sprite {
            image: art.foam.clone(),
            color: Color::srgba(1.0, 1.0, 1.0, 0.65),
            custom_size: Some(Vec2::new(w + 40.0, 12.0)),
            ..default()
        },
        Transform::from_translation(Vec3::new(0.0, foam_y, 0.1)),
    ));
    spawn_swell(commands, rng, w, sea_top, sea_h, bottom);
    seed_travellers(commands, art, rng, w, h);
    spawn_beach_props(commands, art, rng, w, bottom);
}

/// What a beach prop is made of: one sprite, sometimes a shadow under it.
struct Prop {
    image: Handle<Image>,
    tint: Color,
    size: Vec2,
    /// Ground shadow width, or zero for flat things like shells.
    shadow: f32,
    rotation: f32,
}

/// The swell: a stack of crests rolling shoreward. Each is a row of
/// segments sampling one travelling sine, so a crest rises and falls along
/// its length instead of sliding across rigidly.
fn spawn_swell(
    commands: &mut Commands,
    rng: &mut VisualRng,
    w: f32,
    sea_top: f32,
    sea_h: f32,
    bottom: f32,
) {
    let seg_w = (w + 40.0) / WAVE_SEGMENTS as f32;
    for lane in 0..WAVE_LANES {
        // 0 at the horizon, 1 at the beach.
        let near = (lane as f32 + 0.5) / WAVE_LANES as f32;
        let y = bottom + sea_top + sea_h * (1.0 - near) * 0.98;
        let amp = 1.5 + near * near * 9.0;
        // Shorter wavelength close in: a swell you can see the shape of,
        // not a straight streak with a hint of a bend.
        let freq = 0.020 + near * 0.055;
        let speed = 0.6 + near * 2.2;
        let phase = rng.range(0.0, std::f32::consts::TAU);
        let brightness = 0.10 + near * 0.42;
        for segment in 0..WAVE_SEGMENTS {
            let x = -w / 2.0 - 20.0 + seg_w * (segment as f32 + 0.5);
            let base = Vec2::new(x, y);
            commands.spawn((
                MenuArt,
                MenuShore,
                WaveSegment {
                    base,
                    amp,
                    freq,
                    speed,
                    phase,
                    brightness,
                },
                Sprite::from_color(
                    Color::srgba(1.0, 1.0, 1.0, 0.0),
                    // Overlap slightly: a crest should be a line, not a
                    // dotted one.
                    Vec2::new(seg_w + 1.5, 2.0 + near * 3.0),
                ),
                Transform::from_translation(base.extend(0.2)),
            ));
        }
    }
}

/// Pre-seed the sky and sea so the postcard never starts empty: the ambient
/// travellers are slow and would otherwise take minutes to arrive from the
/// edges.
fn seed_travellers(commands: &mut Commands, art: &art::Art, rng: &mut VisualRng, w: f32, h: f32) {
    for _ in 0..3 {
        let x = rng.range(-w * 0.45, w * 0.45);
        spawn_critter(commands, art, rng, w, h, CritterKind::Cloud, Some(x));
    }
    let boat_x = rng.range(-w * 0.35, w * 0.35);
    spawn_critter(commands, art, rng, w, h, CritterKind::Boat, Some(boat_x));
    let crab_x = rng.range(-w * 0.4, w * 0.4);
    spawn_critter(commands, art, rng, w, h, CritterKind::Crab, Some(crab_x));
}

/// One prop, rolled from the beach's furniture list. `scale` shrinks the
/// far ones; the weights are the mix that makes a beach read as a beach:
/// mostly rocks and kelp, a castle or two, the odd landmark.
fn roll_prop(art: &art::Art, rng: &mut VisualRng, scale: f32) -> Prop {
    match rng.next() % 12 {
        0 | 1 => Prop {
            // A castle from some earlier game, still standing.
            image: art.castle.clone(),
            tint: palette::player_color((rng.next() % 4) as u8),
            size: Vec2::splat(52.0 * scale),
            shadow: 44.0 * scale,
            rotation: 0.0,
        },
        2 => Prop {
            // Driftwood: the turnstile log, washed up.
            image: art.log.clone(),
            tint: Color::srgb(1.15, 1.12, 1.05),
            size: Vec2::splat(50.0 * scale),
            shadow: 40.0 * scale,
            rotation: if rng.next().is_multiple_of(2) {
                0.0
            } else {
                std::f32::consts::FRAC_PI_2
            },
        },
        3 | 4 => Prop {
            image: art.kelp.clone(),
            tint: Color::srgba(1.0, 1.0, 1.0, 0.92),
            size: Vec2::splat(rng.range(26.0, 38.0) * scale),
            shadow: 0.0,
            rotation: rng.range(-0.25, 0.25),
        },
        5 => Prop {
            // A tide pool left behind by the water.
            image: art.pool.clone(),
            tint: Color::srgba(1.0, 1.0, 1.0, 0.9),
            size: Vec2::splat(rng.range(46.0, 62.0) * scale),
            shadow: 0.0,
            rotation: 0.0,
        },
        6 => Prop {
            // A crab burrow.
            image: art.hole.clone(),
            tint: Color::WHITE,
            size: Vec2::splat(rng.range(26.0, 34.0) * scale),
            shadow: 0.0,
            rotation: 0.0,
        },
        7 => Prop {
            // A signpost nobody picked up, still pointing somewhere.
            image: art.arrow.clone(),
            tint: palette::player_color((rng.next() % 4) as u8).lighter(0.12),
            size: Vec2::splat(rng.range(30.0, 38.0) * scale),
            shadow: 24.0 * scale,
            rotation: (rng.next() % 4) as f32 * std::f32::consts::FRAC_PI_2,
        },
        8..=10 => Prop {
            image: art.rock.clone(),
            tint: Color::WHITE,
            size: Vec2::splat(rng.range(22.0, 38.0) * scale),
            shadow: 22.0 * scale,
            rotation: rng.range(0.0, std::f32::consts::TAU),
        },
        _ => Prop {
            // A shell in the sand.
            image: art.star.clone(),
            tint: Color::srgba(0.95, 0.78, 0.6, 0.85),
            size: Vec2::splat(rng.range(9.0, 15.0) * scale),
            shadow: 0.0,
            rotation: rng.range(0.0, std::f32::consts::TAU),
        },
    }
}

/// The beach's furniture: the same props the game is played with, so the
/// postcard reads as a board at rest rather than as an empty strip:
/// sandcastles, driftwood turnstiles, kelp, a tide pool, rocks, burrows,
/// and the odd signpost still standing from the last round.
fn spawn_beach_props(
    commands: &mut Commands,
    art: &art::Art,
    rng: &mut VisualRng,
    w: f32,
    bottom: f32,
) {
    // Props are laid along the strip in jittered slots rather than at
    // random: a purely random scatter clumps, and a beach reads better
    // with things spread out and a few deliberate landmarks.
    let slots = ((w / 58.0) as usize).clamp(10, 26);
    for slot in 0..slots {
        let lane = (slot as f32 + 0.5) / slots as f32;
        let x = (lane - 0.5) * w + rng.range(-38.0, 38.0);
        // Up the beach (higher y) is further away: smaller and dimmer.
        // The band starts above the legend card, which owns the very
        // bottom of the window.
        let depth = rng.range(0.0, 1.0);
        let y = bottom + 88.0 + depth * (SHORE_H - 104.0);
        let scale = 1.0 - depth * 0.35;
        let prop = roll_prop(art, rng, scale);
        // Nearer props draw in front, so the strip has depth.
        let z = 0.6 - depth * 0.25;
        if prop.shadow > 0.0 {
            commands.spawn((
                MenuArt,
                MenuShore,
                Sprite {
                    image: art.shadow.clone(),
                    color: Color::srgba(0.36, 0.28, 0.18, 0.28),
                    custom_size: Some(Vec2::new(prop.shadow, prop.shadow * 0.42)),
                    ..default()
                },
                Transform::from_translation(Vec3::new(x, y - prop.size.y * 0.36, z - 0.01)),
            ));
        }
        commands.spawn((
            MenuArt,
            MenuShore,
            Sprite {
                image: prop.image,
                color: prop.tint,
                custom_size: Some(prop.size),
                ..default()
            },
            Transform::from_translation(Vec3::new(x, y, z))
                .with_rotation(Quat::from_rotation_z(prop.rotation)),
        ));
    }
    // Grain: the flat sand band reads as paint without a little speckle,
    // the same way the board's sand tiles do.
    for _ in 0..90 {
        let pale = rng.next().is_multiple_of(3);
        let tint = if pale {
            Color::srgba(1.0, 0.97, 0.88, 0.30)
        } else {
            Color::srgba(0.55, 0.46, 0.31, 0.22)
        };
        let size = rng.range(2.5, 7.0);
        commands.spawn((
            MenuArt,
            MenuShore,
            Sprite::from_color(tint, Vec2::splat(size)),
            Transform::from_translation(Vec3::new(
                rng.range(-w / 2.0, w / 2.0),
                rng.range(bottom + 4.0, bottom + SHORE_H - 6.0),
                0.12,
            )),
        ));
    }
    // Damp patches where the last waves reached: darker sand, faint, and
    // only just below the waterline.
    for _ in 0..5 {
        commands.spawn((
            MenuArt,
            MenuShore,
            Sprite {
                image: art.foam.clone(),
                color: Color::srgba(0.55, 0.47, 0.34, 0.22),
                custom_size: Some(Vec2::new(rng.range(120.0, 240.0), rng.range(10.0, 18.0))),
                ..default()
            },
            Transform::from_translation(Vec3::new(
                rng.range(-w / 2.0, w / 2.0),
                bottom + SHORE_H - rng.range(6.0, 34.0),
                0.15,
            )),
        ));
    }
}

/// (Re)build the shore whenever the window size changes or the menu is
/// freshly entered: the backdrop is world-space and sized to the window,
/// so maximizing must stretch it.
pub fn refit_shore(
    mut commands: Commands,
    art: Res<art::Art>,
    mut rng: ResMut<VisualRng>,
    windows: Query<&Window>,
    mut last: Local<Vec2>,
    shore: Query<Entity, With<MenuShore>>,
    critters: Query<Entity, With<MenuCritter>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = Vec2::new(window.width(), window.height());
    if *last == size && !shore.is_empty() {
        return;
    }
    *last = size;
    // Travellers carry lane positions from the old window size; sweep
    // them with the backdrop and let the pre-seeding repopulate.
    for entity in shore.iter().chain(critters.iter()) {
        commands.entity(entity).despawn();
    }
    spawn_shore(&mut commands, &art, &mut rng, window);
}

/// Spawn one ambient traveller. `at_x` places it mid-scene (used to
/// pre-seed clouds and a boat when the postcard is built); otherwise it
/// enters from just past the upwind edge.
fn spawn_critter(
    commands: &mut Commands,
    art: &art::Art,
    rng: &mut VisualRng,
    w: f32,
    h: f32,
    kind: CritterKind,
    at_x: Option<f32>,
) {
    let bottom = -h / 2.0;
    let horizon = bottom + h * HORIZON;
    let rightward = rng.next().is_multiple_of(2);
    let (y, speed, image, size, tint) = match kind {
        CritterKind::Gull => (
            rng.range(horizon + 30.0, h / 2.0 - 40.0),
            rng.range(130.0, 190.0),
            art.gull_fly.clone(),
            Vec2::splat(46.0),
            Color::WHITE,
        ),
        CritterKind::Cloud => (
            rng.range(horizon + h * 0.08, h / 2.0 - 30.0),
            rng.range(9.0, 20.0),
            art.cloud.clone(),
            Vec2::new(rng.range(110.0, 190.0), rng.range(55.0, 95.0)),
            Color::srgba(1.0, 1.0, 1.0, rng.range(0.75, 0.95)),
        ),
        CritterKind::Boat => (
            rng.range(horizon - 60.0, horizon - 14.0),
            rng.range(18.0, 30.0),
            art.boat.clone(),
            Vec2::splat(rng.range(38.0, 54.0)),
            Color::WHITE,
        ),
        CritterKind::Crab => {
            const KINDS: [CrabKind; 6] = [
                CrabKind::Common,
                CrabKind::Common,
                CrabKind::Juvenile,
                CrabKind::Giant,
                CrabKind::Molting,
                CrabKind::Golden,
            ];
            let crab = KINDS[(rng.next() % 6) as usize];
            (
                rng.range(bottom + 18.0, bottom + SHORE_H - 40.0),
                rng.range(28.0, 55.0),
                art.crab.clone(),
                Vec2::splat(30.0),
                crate::app::creatures::body_color(crab),
            )
        }
    };
    let speed = if rightward { speed } else { -speed };
    // Crabs and gulls face their travel. Their sprites are authored facing
    // +X (`tools/gen_sprites.py`), the same convention the board uses, so
    // this is the board's own rotation for Right or Left rather than a
    // quarter turn: a quarter turn had them crossing the postcard
    // sideways. Clouds and boats stay level, boats flipping to sail
    // forward.
    let rotation = match kind {
        CritterKind::Crab | CritterKind::Gull => crate::app::layout::dir_rotation(if rightward {
            crate::sim::Direction::Right
        } else {
            crate::sim::Direction::Left
        }),
        CritterKind::Cloud | CritterKind::Boat => Quat::IDENTITY,
    };
    let z = match kind {
        CritterKind::Cloud => 1.5,
        CritterKind::Gull => 2.0,
        CritterKind::Boat => 0.3,
        CritterKind::Crab => 0.5,
    };
    let x = at_x.unwrap_or(-speed.signum() * (w / 2.0 + 110.0));
    commands.spawn((
        MenuArt,
        MenuCritter {
            speed,
            frame_clock: rng.range(0.0, 3.0),
            kind,
            base_y: y,
        },
        Sprite {
            image,
            color: tint,
            custom_size: Some(size),
            flip_x: kind == CritterKind::Boat && !rightward,
            ..default()
        },
        Transform::from_translation(Vec3::new(x, y, z)).with_rotation(rotation),
    ));
}

/// Every so often something traverses the postcard: crabs scuttle the
/// sand, gulls glide the sky, clouds drift high, and little sailboats
/// cross the sea with a gentle bob. Waves roll shoreward on a loop.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn menu_ambience(
    mut commands: Commands,
    time: Res<Time>,
    art: Res<art::Art>,
    mut rng: ResMut<VisualRng>,
    mut countdown: Local<f32>,
    windows: Query<&Window>,
    mut critters: Query<(Entity, &mut MenuCritter, &mut Transform, &mut Sprite)>,
    mut waves: Query<(&WaveSegment, &mut Transform, &mut Sprite), Without<MenuCritter>>,
    mut foam: Query<
        (&ShoreFoam, &mut Transform, &mut Sprite),
        (Without<MenuCritter>, Without<WaveSegment>),
    >,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let (w, h) = (window.width(), window.height());
    let dt = time.delta_secs();

    *countdown -= dt;
    if *countdown <= 0.0 {
        *countdown = rng.range(2.0, 5.0);
        let kind = match rng.next() % 10 {
            0..=2 => CritterKind::Gull,
            3..=4 => CritterKind::Cloud,
            5 => CritterKind::Boat,
            _ => CritterKind::Crab,
        };
        spawn_critter(&mut commands, &art, &mut rng, w, h, kind, None);
    }

    for (entity, mut critter, mut transform, mut sprite) in &mut critters {
        transform.translation.x += critter.speed * dt;
        if transform.translation.x.abs() > w / 2.0 + 130.0 {
            commands.entity(entity).despawn();
            continue;
        }
        critter.frame_clock += dt;
        match critter.kind {
            CritterKind::Crab => {
                let frame_b = (critter.frame_clock / 0.16) as u32 % 2 == 1;
                let want = if frame_b { &art.crab_b } else { &art.crab };
                if sprite.image != *want {
                    sprite.image = want.clone();
                }
            }
            // Boats ride a slow swell.
            CritterKind::Boat => {
                transform.translation.y = critter.base_y + (critter.frame_clock * 1.6).sin() * 2.5;
            }
            CritterKind::Gull | CritterKind::Cloud => {}
        }
    }

    // The swell. Each segment samples one travelling sine at its own place
    // along the crest, so the line rolls: the crest rides up, the trough
    // sinks, and the two chase each other shoreward. Whiteness follows the
    // same wave: foam gathers at the top of a swell.
    let now = time.elapsed_secs();
    for (wave, mut transform, mut sprite) in &mut waves {
        let angle = wave.base.x * wave.freq - now * wave.speed + wave.phase;
        // Two sines at unrelated wavelengths: one alone is a corrugated
        // roof, two make a sea.
        let long = (angle * 0.37 - now * wave.speed * 0.55).sin();
        let swell = (angle.sin() + long * 0.55) / 1.55;
        transform.translation.y = wave.base.y + swell * wave.amp;
        // Sharpen the crest: foam should be a line along the top of the
        // swell, not an even glow over the whole thing.
        let crest = ((swell + 1.0) * 0.5).powf(2.2);
        sprite.color.set_alpha(wave.brightness * crest);
    }
    // The waterline breathes: the foam lip creeps up the sand and back.
    for (shore, mut transform, mut sprite) in &mut foam {
        let breath = (now * 0.42).sin();
        transform.translation.y = shore.base_y + breath * 5.0;
        sprite.color.set_alpha(0.5 + 0.22 * (breath * 0.5 + 0.5));
    }
}
