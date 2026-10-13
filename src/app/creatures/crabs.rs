//! Crab sprites: their shells and claws, the crowding that fans a pile
//! out into lanes, and the walk drawn between ticks.

use super::{CreatureShadow, Tween, smoothing_alpha, tweened_pos, unturned};
use crate::app::art::Art;
use crate::app::layout::{self, TILE};
use crate::app::palette;
use crate::app::{Sim, effects};
use crate::sim::{Crab, CrabKind, Handedness};
use bevy::color::Mix;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

#[derive(Component)]
pub struct CrabSprite {
    pub id: u32,
    pub kind: CrabKind,
    /// A per-crab lift or drop on the shell colour, drawn once from the id,
    /// so twenty crabs of one kind are a crowd rather than twenty copies of
    /// one sprite. Small enough that a kind is still read by its colour.
    pub(crate) shade: f32,
}

/// How far a crab has been nudged out of a crowd, and how much smaller it
/// is drawn there, eased toward what [`crowd_target`] asks each frame so a
/// pile forming or breaking up is a drift and not a jump.
#[derive(Component, Default)]
pub struct Crowding {
    spread: Vec2,
    shrink: f32,
}

/// How close another crab has to be, centre to centre, to count as company.
const CROWD_RADIUS: f32 = TILE * 0.45;

/// How far across its heading a crab in a thick crowd is moved: the outer
/// of its five lanes, in world units.
const CROWD_SPREAD: f32 = TILE * 0.32;

/// How much smaller a crab in a thick crowd is drawn.
const CROWD_SHRINK: f32 = 0.28;

/// How crowded a crab is, 0 to 1, from how many others are within
/// [`CROWD_RADIUS`]: none is 0, three or more is a pile.
fn crowd_level(neighbours: usize) -> f32 {
    (neighbours as f32 / 3.0).min(1.0)
}

/// Which of five lanes across its heading a crab takes in a crowd, -1 to
/// 1: fixed by its id, so the same crab always moves the same way and a
/// stream fans out into a band instead of shuffling.
fn crowd_lane(id: u32) -> f32 {
    let lane = (id.wrapping_mul(2_654_435_761) >> 16) % 5;
    lane as f32 / 2.0 - 1.0
}

/// Where a crab should be drawn relative to its true place, and how much
/// smaller, given how crowded it is and which way it faces.
///
/// A crowd is where crabs are drawn on top of each other: a stream out of
/// a spawner walks single file, each half over the one ahead, and a lure
/// or Crab Mania heaps them into a pile of legs nobody can count. Moved
/// across their heading into lanes and drawn a little smaller, they read
/// as that many crabs. A crab on its own is drawn exactly where it is:
/// the offset is render-side only and never reaches the sim.
fn crowd_target(neighbours: usize, id: u32, heading: Vec2) -> (Vec2, f32) {
    let level = crowd_level(neighbours);
    let across = Vec2::new(-heading.y, heading.x);
    (
        across * crowd_lane(id) * CROWD_SPREAD * level,
        CROWD_SHRINK * level,
    )
}

/// How many other crabs are within [`CROWD_RADIUS`] of each crab, by id.
/// Bucketed by radius-sized cells, so each crab checks its own cell and the
/// eight round it rather than the whole beach.
fn count_neighbours(board: &crate::sim::Board, into: &mut HashMap<u32, usize>) {
    into.clear();
    let at: Vec<(u32, Vec2)> = board
        .crabs()
        .iter()
        .map(|crab| (crab.id, layout::pose_pos(board, crab.pose())))
        .collect();
    let cell = |p: Vec2| {
        (
            (p.x / CROWD_RADIUS).floor() as i32,
            (p.y / CROWD_RADIUS).floor() as i32,
        )
    };
    let mut cells: HashMap<(i32, i32), Vec<Vec2>> = HashMap::default();
    for &(_, p) in &at {
        cells.entry(cell(p)).or_default().push(p);
    }
    for &(id, p) in &at {
        let (cx, cy) = cell(p);
        let mut near = 0;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(others) = cells.get(&(cx + dx, cy + dy)) {
                    near += others
                        .iter()
                        .filter(|o| o.distance_squared(p) < CROWD_RADIUS * CROWD_RADIUS)
                        .count();
                }
            }
        }
        // Itself, counted once in its own cell.
        into.insert(id, near.saturating_sub(1));
    }
}

/// The widest a shell strays from its kind's colour, either way.
const SHADE_SPREAD: f32 = 0.05;

/// One crab's shell: its kind's colour, nudged by its own shade.
///
/// Only the kinds a beach is *crowded* with. Twenty is never how many
/// golden crabs are on the sand, and a golden lightened by a twentieth
/// lands nearer a juvenile's pale orange than its own yellow: fifty points
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

const CLAW_LEFT: Color = Color::srgb(0.18, 0.49, 0.82); // blue = left-clawed
const CLAW_RIGHT: Color = Color::srgb(0.97, 0.82, 0.17); // yellow = right-clawed

/// The big claw's colour: the handedness tell.
pub(crate) fn claw_color(handed: Handedness) -> Color {
    match handed {
        Handedness::Left => CLAW_LEFT,
        Handedness::Right => CLAW_RIGHT,
    }
}

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

/// The side of the square a crab's sprites are drawn in: the body, and the
/// big claw laid over it in the same frame. The sprites are square with
/// the legs reaching toward the corners, so the side is wider than the
/// shell's footprint ([`body_size`]), which is what the shadow is cut to.
pub(crate) fn sprite_side(kind: CrabKind) -> f32 {
    body_size(kind).x * 1.45
}

/// Which way up a crab's sprites are drawn. They are authored right-
/// handed, big claw on the crab's right; a left-handed crab is the same
/// crab mirrored across its own heading.
pub(crate) fn mirrored(handed: Handedness) -> bool {
    match handed {
        Handedness::Left => true,
        Handedness::Right => false,
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

/// A crab's body in `color`, at its kind's size and mirrored for its hand:
/// the sprite every crab on the beach is drawn as, walking, swept or
/// hopping into a keep.
pub(crate) fn crab_body(art: &Art, kind: CrabKind, handed: Handedness, color: Color) -> Sprite {
    Sprite {
        image: art.crab.clone(),
        color,
        custom_size: Some(Vec2::splat(sprite_side(kind))),
        flip_y: mirrored(handed),
        ..default()
    }
}

/// How far over its body a crab's big claw is drawn, as the body's child.
///
/// The smallest step that keeps the claw over its own body. A tenth, as
/// the walking and swept crabs once had it, put a hopping crab's claw at
/// the gulls' own depth, where which drew on top was left to chance.
const CLAW_LIFT: f32 = 0.01;

/// The big claw: the handedness tell, and the one thing a player reads a
/// crab by. A child of the body, `side` square like it, so it sits where
/// the body's arm reaches whatever the crab's size, and tinted by hand
/// rather than by kind.
pub(crate) fn crab_claw(art: &Art, handed: Handedness, side: Vec2) -> (Sprite, Transform) {
    (
        Sprite {
            image: art.claw.clone(),
            color: claw_color(handed),
            custom_size: Some(side),
            flip_y: mirrored(handed),
            ..default()
        },
        Transform::from_translation(Vec3::new(0.0, 0.0, CLAW_LIFT)),
    )
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
        let side = Vec2::splat(sprite_side(crab.kind));
        commands
            .spawn((
                CrabSprite {
                    id,
                    kind: crab.kind,
                    shade: shade_of(id),
                },
                Crowding::default(),
                crab_body(
                    &art,
                    crab.kind,
                    crab.handed,
                    shell_color(crab.kind, shade_of(id)),
                ),
                Transform::from_translation(pos.extend(layout::z::CREATURE))
                    .with_rotation(layout::dir_rotation(crab.dir)),
            ))
            .with_children(|parent| {
                parent.spawn((
                    CreatureShadow,
                    layout::image_sprite(
                        &art.shadow,
                        Color::WHITE,
                        Vec2::new(size.x * 1.35, size.y * 1.15),
                    ),
                    Transform::from_translation(Vec3::new(0.0, 0.0, -0.5)),
                ));
                parent.spawn(crab_claw(&art, crab.handed, side));
            });
    }
}

/// A crab's sprite, as the interpolation moves and dresses it.
type CrabBody = (
    &'static CrabSprite,
    &'static mut Crowding,
    &'static mut Transform,
    &'static mut Sprite,
    &'static Children,
);

/// The crabs on screen and the shadows under them.
#[derive(bevy::ecs::system::SystemParam)]
pub struct CrabSprites<'w, 's> {
    bodies: Query<'w, 's, CrabBody, Without<CreatureShadow>>,
    shadows: Query<'w, 's, &'static mut Transform, (With<CreatureShadow>, Without<CrabSprite>)>,
}

/// Interpolate each crab sprite between its previous and current sim position.
pub fn interpolate_crabs(
    sim: Res<Sim>,
    fx: effects::Fx,
    tween: Tween,
    mut glint_clock: Local<f32>,
    mut by_id: Local<HashMap<u32, Crab>>,
    mut crowds: Local<HashMap<u32, usize>>,
    crabs: CrabSprites,
) {
    let effects::Fx {
        mut commands,
        art,
        mut rng,
        settings,
    } = fx;
    let Tween {
        time,
        fixed_time,
        mut watch,
    } = tween;
    let CrabSprites {
        bodies: mut sprites,
        mut shadows,
    } = crabs;
    let board = &sim.0;
    let alpha = smoothing_alpha(board.ticks(), &mut watch, time.delta_secs(), &fixed_time);
    by_id.clear();
    by_id.extend(board.crabs().iter().map(|c| (c.id, *c)));
    // Precious crabs glint on one clock: they twinkle together, and the
    // scatter `glint` puts on each star's position keeps that from reading
    // as a metronome.
    *glint_clock += time.delta_secs();
    let glint_now = *glint_clock >= 0.22;
    if glint_now {
        *glint_clock = 0.0;
    }
    // During a molting lure every loose crab ignores arrows; tint them all
    // toward the luring player's colour so the takeover is visible.
    let lure_tint = board.lure().map(|(owner, _)| palette::player_color(owner));
    count_neighbours(board, &mut crowds);
    // The same easing whatever the frame rate: about a third of a second
    // to settle into, or out of, a crowd.
    let ease = 1.0 - (-time.delta_secs() * 8.0).exp();
    for (sprite, mut crowding, mut transform, mut body, children) in &mut sprites {
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
        let pos = tweened_pos(prev, curr, alpha);
        let (dx, dy) = crab.dir.offset();
        // Sim y runs down, world y runs up.
        let heading = Vec2::new(dx as f32, -(dy as f32));
        let neighbours = crowds.get(&sprite.id).copied().unwrap_or(0);
        let (spread, shrink) = crowd_target(neighbours, sprite.id, heading);
        let (was_spread, was_shrink) = (crowding.spread, crowding.shrink);
        crowding.spread = was_spread + (spread - was_spread) * ease;
        crowding.shrink = was_shrink + (shrink - was_shrink) * ease;
        transform.translation = (pos + crowding.spread).extend(layout::z::CREATURE);
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
        // Drawn a little smaller in a crowd, and molting crabs pulse
        // gently on top of that: they are worth chasing.
        let pulse = if sprite.kind == CrabKind::Molting {
            1.0 + (time.elapsed_secs() * 5.0).sin() * 0.06
        } else {
            1.0
        };
        transform.scale = Vec3::splat(pulse * (1.0 - crowding.shrink));
        // The shadow: pushed out from under the body, the same way as
        // every other thing standing on this sand, since one sitting
        // exactly beneath the crab is never seen.
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
        // fifty points walking, so it draws a ribbon of sparks behind it
        // and a table of four can see which way the jackpot is heading.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A left-handed crab is the right-handed drawing mirrored, and the two
    /// hands wear different claws: the tell a player reads a crab by.
    #[test]
    fn a_left_handed_crab_is_the_other_one_mirrored() {
        assert!(mirrored(Handedness::Left));
        assert!(!mirrored(Handedness::Right));
        assert_ne!(claw_color(Handedness::Left), claw_color(Handedness::Right));
    }

    /// A crab's square is bigger than its shell, legs out to the corners,
    /// and the kinds keep their sizes: a giant over a common over a young.
    #[test]
    fn a_crab_is_drawn_bigger_than_its_shell_and_in_its_size() {
        for kind in CrabKind::ALL {
            assert!(sprite_side(kind) > body_size(kind).x, "{kind:?}");
        }
        let side = sprite_side;
        assert!(side(CrabKind::Giant) > side(CrabKind::Common));
        assert!(side(CrabKind::Common) > side(CrabKind::Juvenile));
    }

    /// How crowded a crab is climbs with its company and stops at full:
    /// nothing alone, a third with one neighbour, full at three, no further.
    #[test]
    fn a_crowd_fills_up_at_three_neighbours() {
        assert_eq!(crowd_level(0), 0.0);
        assert!((crowd_level(1) - 1.0 / 3.0).abs() < 1e-6);
        assert_eq!(crowd_level(3), 1.0);
        assert_eq!(crowd_level(20), 1.0);
    }

    /// A crab on its own is drawn exactly where it is; in a crowd it moves
    /// across its heading, never along it, by at most the outer lane, and
    /// always to the same side for the same crab.
    #[test]
    fn a_crowd_fans_out_across_its_heading_and_a_lone_crab_stays_put() {
        let east = Vec2::X;
        for id in 0..40 {
            assert_eq!(
                crowd_target(0, id, east),
                (Vec2::ZERO, 0.0),
                "crab {id} alone"
            );
            let (spread, shrink) = crowd_target(5, id, east);
            assert_eq!(spread.x, 0.0, "crab {id} moved along its heading");
            assert!(spread.y.abs() <= CROWD_SPREAD + 1e-4, "{spread}");
            assert_eq!(shrink, CROWD_SHRINK, "a pile is a full crowd");
            assert_eq!(
                crowd_target(5, id, east),
                (spread, shrink),
                "same crab, same lane"
            );
        }
        // One neighbour is a third of a crowd.
        let (_, some) = crowd_target(1, 3, east);
        assert!((some - CROWD_SHRINK / 3.0).abs() < 1e-6);
        // The five lanes are all in use across a crowd of crabs.
        let lanes: std::collections::BTreeSet<i32> =
            (0..40).map(|id| (crowd_lane(id) * 2.0) as i32).collect();
        assert_eq!(lanes.len(), 5, "{lanes:?}");
    }

    /// A crab keeps its shell for as long as it lives. The shade is a hash
    /// of the id rather than a draw, so a sprite rebuilt around a crab, as
    /// they constantly are, does not hand it a new colour.
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
    /// The kinds are told apart by colour on a crowded beach, and what that
    /// needs is not a small *number* (the nudge is a perceptual lighten, so
    /// a saturated gold moves a long way in blue for little visible change)
    /// but a guarantee about neighbours: whatever the shade does, the shell
    /// must still be nearer its own kind than any other.
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
}
