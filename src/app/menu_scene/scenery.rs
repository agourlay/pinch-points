//! The postcard behind the menu: a beach at rest.
//!
//! Bottom to top: sand furnished with the props the game is played with,
//! the waterline, a sea with a rolling swell, and sky. Crabs scuttle the
//! sand, gulls glide, clouds drift and little boats cross. None of it is
//! sim: the menu runs no board, and every animation here is decoration
//! driven by frame time.

use super::VisualRng;
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

/// The sand strip's vertical extent, from the bottom of the window.
const SHORE_H: f32 = 208.0;

/// The horizon, as a fraction of window height from the bottom: the sea
/// runs from the shore to here, sky above.
const HORIZON: f32 = 0.62;

/// Marker for the shore backdrop pieces, so a resize can rebuild them
/// without touching the rest of the menu.
#[derive(Component)]
pub struct MenuShore;

/// The dusk over the postcard everywhere but the menu: one translucent
/// sheet between the scene and the cards, so the same beach can stand
/// behind every screen without shouting over the rows in front of it.
#[derive(Component)]
pub struct Scrim;

/// How big the scrim is for a postcard of `size`: the whole window and a
/// margin past it, so no edge of daylight shows round the dusk. One rule
/// for the spawn and for [`refit_shore`], which resizes it beside the
/// shore.
fn scrim_size(size: Vec2) -> Vec2 {
    size + Vec2::splat(80.0)
}

/// Keep the postcard on the screens that want it, and only there.
///
/// The shore itself is spawned and resized by [`refit_shore`]; this system
/// owns the two transitions refit cannot see: leaving the postcard set for
/// a board screen, and the menu against every other postcard screen (the
/// menu gets the scene at full daylight, the rest behind the scrim).
pub fn tend_backdrop(
    mut commands: Commands,
    screen: Res<State<crate::app::Screen>>,
    viewport: crate::app::menu_ui::Viewport,
    shore: Query<Entity, With<MenuShore>>,
    critters: Query<Entity, With<MenuCritter>>,
    scrim: Query<Entity, With<Scrim>>,
) {
    // Reconciled every frame rather than on the transition: the window can
    // be missing on the exact frame the state flips, and a scrim lost to
    // that race would leave a whole screen at full daylight.
    let on_postcard = crate::app::postcard_screen(*screen.get());
    if !on_postcard {
        for entity in shore.iter().chain(critters.iter()).chain(scrim.iter()) {
            commands.entity(entity).despawn();
        }
        return;
    }
    let wants_scrim = *screen.get() != crate::app::Screen::Menu;
    if wants_scrim && scrim.is_empty() {
        let Some(size) = viewport.size() else {
            return;
        };
        commands.spawn((
            Scrim,
            Sprite::from_color(Color::srgba(0.07, 0.09, 0.12, 0.55), scrim_size(size)),
            Transform::from_translation(Vec3::new(0.0, 0.0, 5.0)),
        ));
    } else if !wants_scrim {
        for entity in &scrim {
            commands.entity(entity).despawn();
        }
    }
}

// The postcard is laid out in the window's size in interface units
// ([`crate::app::menu_ui::Viewport::size`]). The camera over it zooms by
// the interface's scale (`boot::fit_camera`), so a postcard that size fills
// the window and every pixel measure in this file is one of the
// interface's own. Laid out in window pixels, the scenery stayed its 720p
// size while the cards grew with the window, and on a big screen the props
// shrank to specks beside them.

/// The whole postcard, bottom to top: sand with shells, a wet line, the
/// open sea up to the horizon, and sky above. World-space sprites behind
/// the UI, in interface units ([`crate::app::menu_ui::Viewport::size`]).
fn spawn_shore(commands: &mut Commands, art: &art::Art, rng: &mut VisualRng, size: Vec2) {
    let (w, h) = (size.x, size.y);
    let bottom = -h / 2.0;
    let horizon = bottom + h * HORIZON;
    let band = |y: f32, height: f32, color: Color, z: f32| {
        (
            MenuShore,
            Sprite::from_color(color, Vec2::new(w + 40.0, height)),
            Transform::from_translation(Vec3::new(0.0, y, z)),
        )
    };
    // Sky: pale at the horizon, deepening all the way up: a band of the
    // pale with the deep washed over it, opaque at the top (`ramp`'s
    // opaque edge) and gone at the horizon, so there is no hard step.
    let sky_h = h / 2.0 - horizon;
    let sky_mid = horizon + sky_h / 2.0;
    commands.spawn(band(sky_mid, sky_h, Color::srgb(0.62, 0.79, 0.90), 0.0));
    commands.spawn((
        MenuShore,
        Sprite {
            image: art.ramp.clone(),
            color: Color::srgb(0.42, 0.65, 0.85),
            custom_size: Some(Vec2::new(w + 40.0, sky_h)),
            ..default()
        },
        Transform::from_translation(Vec3::new(0.0, sky_mid, 0.05)),
    ));
    // The beach and the open sea, drawn by the same two shaders as the
    // beach round a board, so the postcard is the place the rounds are
    // played: dry rippled sand, and the sea from its waterline, where the
    // waves run up and back, out to the horizon. It was flat bands under
    // seven rows of stepped crest segments, which read as pixel art beside
    // the drawn sprites.
    let sand_top = bottom + SHORE_H + 40.0;
    commands.spawn((
        MenuShore,
        Mesh2d(art.water.quad.clone()),
        MeshMaterial2d(art.water.sand.clone()),
        Transform::from_translation(Vec3::new(0.0, (bottom - 20.0 + sand_top) / 2.0, 0.0))
            .with_scale(Vec3::new(w + 40.0, sand_top - bottom + 20.0, 1.0)),
    ));
    // Its foot a little below the old waterline: the shader's first stretch
    // up from there is the wet sand the waves run up.
    let sea_foot = bottom + SHORE_H - 10.0;
    commands.spawn((
        MenuShore,
        Mesh2d(art.water.quad.clone()),
        MeshMaterial2d(art.water.ocean.clone()),
        Transform::from_translation(Vec3::new(0.0, (sea_foot + horizon) / 2.0, 0.02))
            .with_scale(Vec3::new(w + 40.0, horizon - sea_foot, 1.0)),
    ));
    commands.spawn(band(horizon, 3.0, Color::srgb(0.72, 0.84, 0.92), 0.1));
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
    /// A second layer over the first, tinted: a signpost's paint on its
    /// board.
    paint: Option<(Handle<Image>, Color)>,
    /// A tide pool: drawn as the board draws one (see `spawn_pond`), not
    /// as `image`.
    pond: bool,
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
            tint: crate::app::board_render::castle_body(palette::player_color(
                (rng.next() % 4) as u8,
            )),
            size: Vec2::splat(52.0 * scale),
            shadow: 44.0 * scale,
            rotation: 0.0,
            paint: None,
            pond: false,
        },
        2 => Prop {
            // Driftwood: the turnstile log, washed up. Not tinted past
            // white: under the HDR camera a tint over 1 is light, and the
            // log haloed in the bloom brighter than the foam.
            image: art.log.clone(),
            tint: Color::srgb(1.0, 0.98, 0.93),
            size: Vec2::splat(50.0 * scale),
            shadow: 40.0 * scale,
            rotation: if rng.next().is_multiple_of(2) {
                0.0
            } else {
                std::f32::consts::FRAC_PI_2
            },
            paint: None,
            pond: false,
        },
        3 | 4 => Prop {
            image: art.kelp.clone(),
            tint: Color::srgba(1.0, 1.0, 1.0, 0.92),
            size: Vec2::splat(rng.range(26.0, 38.0) * scale),
            shadow: 0.0,
            rotation: rng.range(-0.25, 0.25),
            paint: None,
            pond: false,
        },
        5 => Prop {
            // A tide pool left behind by the water, in the board's layers:
            // no picture of its own.
            image: Handle::default(),
            tint: Color::WHITE,
            size: Vec2::splat(rng.range(46.0, 62.0) * scale),
            shadow: 0.0,
            rotation: 0.0,
            paint: None,
            pond: true,
        },
        6 => Prop {
            // A crab burrow.
            image: art.hole.clone(),
            tint: Color::WHITE,
            size: Vec2::splat(rng.range(26.0, 34.0) * scale),
            shadow: 0.0,
            rotation: 0.0,
            paint: None,
            pond: false,
        },
        7 => Prop {
            // A signpost nobody picked up, still pointing somewhere.
            image: art.sign_board.clone(),
            tint: Color::WHITE,
            paint: Some((
                art.sign_paint.clone(),
                palette::seat_paint((rng.next() % 4) as u8),
            )),
            size: Vec2::splat(rng.range(30.0, 38.0) * scale),
            shadow: 24.0 * scale,
            rotation: (rng.next() % 4) as f32 * std::f32::consts::FRAC_PI_2,
            pond: false,
        },
        8..=10 => Prop {
            image: art.rock.clone(),
            tint: Color::WHITE,
            size: Vec2::splat(rng.range(22.0, 38.0) * scale),
            shadow: 22.0 * scale,
            rotation: rng.range(0.0, std::f32::consts::TAU),
            paint: None,
            pond: false,
        },
        _ => Prop {
            // A shell in the sand.
            image: art.star.clone(),
            tint: Color::srgba(0.95, 0.78, 0.6, 0.85),
            size: Vec2::splat(rng.range(9.0, 15.0) * scale),
            shadow: 0.0,
            rotation: rng.range(0.0, std::f32::consts::TAU),
            paint: None,
            pond: false,
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
                MenuShore,
                crate::app::layout::image_sprite(
                    &art.shadow,
                    Color::srgba(0.36, 0.28, 0.18, 0.28),
                    Vec2::new(prop.shadow, prop.shadow * 0.42),
                ),
                // Leaning the way the board's shadows fall (`layout::SUN`):
                // the sun is over the same shoulder on every screen.
                Transform::from_translation(Vec3::new(
                    x + prop.shadow * 0.12,
                    y - prop.size.y * 0.36,
                    z - 0.01,
                )),
            ));
        }
        // A tide pool is drawn as the board draws one, so the two match: a
        // lip of wet sand under the water shader's shallows and deep, and
        // the sun's glint. The baked pool sprite read as a soap bubble
        // beside the board's pools.
        if prop.pond {
            spawn_pond(commands, art, Vec3::new(x, y, z), prop.size);
            continue;
        }
        let mut piece = commands.spawn((
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
        if let Some((image, color)) = prop.paint {
            piece.with_child((
                Sprite {
                    image,
                    color,
                    custom_size: Some(prop.size),
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.001)),
            ));
        }
    }
    // No grain or damp patches of its own: the sand shader draws both.
}

/// A tide pool on the postcard, `size` across, centred at `at`: the
/// board's layers and glint, from the board's own table and helpers, at
/// the size of the prop rather than of a tile.
fn spawn_pond(commands: &mut Commands, art: &art::Art, at: Vec3, size: Vec2) {
    use crate::app::board_render::{POOL_LAYERS, glint_place, spawn_layer};
    // Squashed a little: the postcard's beach is seen at a slant.
    let size = size * Vec2::new(1.0, 0.7);
    for (layer, (kind, share, _)) in POOL_LAYERS.into_iter().enumerate() {
        let place = Transform::from_translation(at + Vec3::new(0.0, 0.0, 0.001 * layer as f32));
        spawn_layer(commands, art, kind, place, size * share, MenuShore);
    }
    commands.spawn((
        MenuShore,
        Mesh2d(art.water.quad.clone()),
        MeshMaterial2d(art.water.glint.clone()),
        glint_place(at + Vec3::new(0.0, 0.0, 0.003), size),
    ));
}

/// Everything a rebuild of the postcard sweeps away: the shore and the
/// travellers crossing it.
type PostcardPieces = Or<(With<MenuShore>, With<MenuCritter>)>;

/// (Re)build the shore whenever the window size changes or the menu is
/// freshly entered: the backdrop is world-space and sized to the window,
/// so maximizing must stretch it.
pub fn refit_shore(
    mut commands: Commands,
    art: Res<art::Art>,
    mut rng: ResMut<VisualRng>,
    viewport: crate::app::menu_ui::Viewport,
    mut last: Local<Vec2>,
    scene: Query<(Entity, Has<MenuShore>), PostcardPieces>,
    mut scrim: Query<&mut Sprite, With<Scrim>>,
) {
    // In interface units, so turning the UI scale rebuilds it as surely as
    // resizing the window does.
    let Some(size) = viewport.size() else {
        return;
    };
    if *last == size && scene.iter().any(|(_, shore)| shore) {
        return;
    }
    *last = size;
    // Travellers carry lane positions from the old window size; sweep
    // them with the backdrop and let the pre-seeding repopulate.
    for (entity, _) in &scene {
        commands.entity(entity).despawn();
    }
    spawn_shore(&mut commands, &art, &mut rng, size);
    // The dusk over it is sized to the window too. It is not part of the
    // shore, and outlives a rebuild, so it is stretched here rather than
    // left at the size of the window it was spawned in: a window grown
    // past it showed a frame of full daylight round the edges.
    for mut sprite in &mut scrim {
        sprite.custom_size = Some(scrim_size(size));
    }
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
    // quarter turn, which had them crossing the postcard sideways. Clouds
    // and boats stay level, boats flipping to sail forward.
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
    // A crab carries its big claw, on whichever side it was born with, as
    // it does on the board: the body alone is a crab with one small claw.
    let handed = if rng.next().is_multiple_of(2) {
        crate::sim::Handedness::Left
    } else {
        crate::sim::Handedness::Right
    };
    let flip_y = kind == CritterKind::Crab && crate::app::creatures::mirrored(handed);
    let mut critter = commands.spawn((
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
            flip_y,
            ..default()
        },
        Transform::from_translation(Vec3::new(x, y, z)).with_rotation(rotation),
    ));
    if kind == CritterKind::Crab {
        critter.with_child(crate::app::creatures::crab_claw(art, handed, size));
    }
}

/// A traveller on the postcard, and what moving it touches.
type Critter = (
    Entity,
    &'static mut MenuCritter,
    &'static mut Transform,
    &'static mut Sprite,
);

/// Every so often something traverses the postcard: crabs scuttle the
/// sand, gulls glide the sky, clouds drift high, and little sailboats
/// cross the sea with a gentle bob. The sea itself moves in its shader.
pub fn menu_ambience(
    mut commands: Commands,
    time: Res<Time>,
    art: Res<art::Art>,
    mut rng: ResMut<VisualRng>,
    mut countdown: Local<f32>,
    viewport: crate::app::menu_ui::Viewport,
    mut critters: Query<Critter>,
) {
    let Some(size) = viewport.size() else {
        return;
    };
    let (w, h) = (size.x, size.y);
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
                use crate::app::creatures::{BOB, WALK_FPS};
                let step = critter.frame_clock * WALK_FPS;
                let want = &art.crab_walk[step as usize % art.crab_walk.len()];
                // The same bob as on the board.
                let bob = 1.0 + (step * std::f32::consts::PI).sin().abs() * BOB;
                transform.scale = Vec3::splat(bob);
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
}
