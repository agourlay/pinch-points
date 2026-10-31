//! Signposts: the driftwood boards players plant, built from the sim's
//! posts and dressed every frame with how settled, worn and spent they
//! are, and which one the next placement will take.

use super::{OnTile, clear_stale, image_sprite, on_board, z};
use crate::app::Sim;
use crate::app::art::Art;
use crate::app::layout::{self, SIGN_SIZE};
use crate::app::palette;
use crate::sim::{Direction, SignpostHealth};
use bevy::prelude::*;

/// Marker for a signpost sprite; carries the state it was built from so the
/// sync system can tell when the board's signpost changed under it.
///
/// Wear and age are deliberately *not* part of that key. As a quantized
/// fade bucket they had a post despawned and respawned six times over its
/// life, which leaves the sprite with no memory and no animation that can
/// outlive a rebuild. They are written in place by [`dress_signposts`]
/// instead, and the sprite lives from planting to pulling.
#[derive(Component)]
pub struct SignpostSprite {
    x: u8,
    y: u8,
    dir: Direction,
    owner: u8,
    /// Seconds since it was planted. Drives the plant pop, and nothing else.
    age: f32,
    /// The tick the sim says this post was planted or re-pointed on.
    ///
    /// Pressing the same direction again on your own fading post is normal
    /// versus play: the sim stamps it `Full` with a fresh `placed` and
    /// nothing else changes, so the diff key sees nothing, the differ
    /// raises no `SignpostPlaced`, and there is no ring and no knock.
    /// Without this the post snaps from worn to fresh between two frames.
    planted: u64,
}

impl OnTile for SignpostSprite {
    fn tile(&self) -> (u8, u8) {
        (self.x, self.y)
    }
}

/// The soft dark copy of the arrow lying under it.
///
/// A child rather than baked into the art, because the arrow is tinted to
/// its owner's colour at spawn and a tint takes the whole sprite with it:
/// a baked shadow would come out red for one seat and blue for the next.
#[derive(Component)]
pub struct SignpostShadow;

/// The arrow painted on a signpost's board: the one part in its owner's
/// colour. A child of the board, so it turns and settles with it.
#[derive(Component)]
pub struct SignpostPaint;

/// A gold glow behind the post the next placement will take: the same
/// silhouette as the shadow, a size up, shown only on that one post.
#[derive(Component)]
pub struct SignpostHalo;

/// How far the halo reaches past the post's own outline.
const HALO_SCALE: f32 = 1.4;

/// How far the next-to-go post rocks either way, in radians: a loose post
/// in the sand, not a spinning one.
const WOBBLE: f32 = 0.12;

/// How long a post takes to settle into the sand after it is planted.
const PLANT_POP: f32 = 0.17;

/// How big the plant draws a post, `age` seconds after it went in.
///
/// Driven in, overshooting, settling. `cos` through one and a half turns
/// lands exactly on rest, so the post finishes the size the board expects
/// and holds there rather than a hair off it for ever.
fn plant_pop(age: f32) -> f32 {
    if age >= PLANT_POP {
        return 1.0;
    }
    let t = age / PLANT_POP;
    1.0 + 0.35 * (1.0 - t) * (t * std::f32::consts::PI * 1.5).cos()
}

/// How strongly a post's paint draws with `fade` of its life left.
///
/// Floored well short of transparent: spelled with alpha alone, a worn,
/// aged post comes out at an eighth of full strength on bright sand, as
/// the one thing on the board the player steers with.
///
/// Only the paint: the board it is on stays at full strength, since the
/// wood against the sand is what finds a post at all, and a faded board is
/// sand-coloured wood fading into sand.
fn post_alpha(fade: f32) -> f32 {
    // Higher than the 0.55 a whole post once faded to: over wood that no
    // longer fades, an aged yellow at 0.55 was cream on tan.
    0.7 + 0.3 * fade
}

/// How much a post shrinks as it runs out.
///
/// The alpha floor above leaves the fade little range to speak in, so an
/// expiring post withers as well as dimming. Campaign posts
/// never fade, so this never touches a puzzle.
fn wither(fade: f32) -> f32 {
    0.86 + 0.14 * fade
}

/// Diff the signpost sprites against the sim every frame: despawn sprites
/// whose signpost is gone or was replaced, spawn sprites for signposts
/// without one. Wear and fade are [`dress_signposts`]'s to write.
pub fn sync_signposts(
    mut commands: Commands,
    sim: Res<Sim>,
    art: Res<Art>,
    mut covered: Local<Vec<bool>>,
    existing: Query<(Entity, &SignpostSprite)>,
) {
    let board = &sim.0;
    clear_stale(&mut commands, board, &mut covered, existing, |sprite| {
        // A different heading or a different owner on the same tile is a
        // different post, and gets its own plant.
        board
            .signpost_at(sprite.x, sprite.y)
            .is_some_and(|sp| sp.dir == sprite.dir && sp.owner == sprite.owner)
    });
    for (x, y, _) in board.tiles() {
        let idx = usize::from(board.index_of(x, y));
        if covered[idx] {
            continue;
        }
        let Some(sp) = board.signpost_at(x, y) else {
            continue;
        };
        spawn_signpost(&mut commands, &art, board, (x, y), sp);
    }
}

/// The post `sp` planted on tile `(x, y)`: the one post a round draws
/// ([`sync_signposts`], dressed after by [`dress_signposts`]), and a map
/// card's preview too ([`super::spawn_board_pieces`]).
pub(crate) fn spawn_signpost(
    commands: &mut Commands,
    art: &Art,
    board: &crate::sim::Board,
    (x, y): (u8, u8),
    sp: crate::sim::Signpost,
) -> Entity {
    let pos = layout::tile_center(board, x, y);
    // A driftwood board, cut to a point, with an arrow painted on it
    // in the owner's colour, over a shadow of itself so it stands in
    // the sand rather than being painted on it. The board is its own
    // colour and only the paint is tinted. The art and the colours are
    // written on the first frame by `dress_signposts`.
    commands
        .spawn((
            SignpostSprite {
                x,
                y,
                dir: sp.dir,
                owner: sp.owner,
                age: 0.0,
                planted: sp.placed,
            },
            image_sprite(&art.sign_board, Color::WHITE, Vec2::splat(SIGN_SIZE)),
            Transform::from_translation(pos.extend(z::SIGNPOST))
                .with_rotation(layout::dir_rotation(sp.dir)),
        ))
        .with_children(|parent| {
            parent.spawn((
                SignpostHalo,
                image_sprite(&art.sign_shape, palette::GOLD, Vec2::splat(SIGN_SIZE)),
                Transform::from_translation(Vec3::new(0.0, 0.0, -0.04))
                    .with_scale(Vec3::splat(HALO_SCALE)),
                Visibility::Hidden,
            ));
            parent.spawn((
                SignpostShadow,
                image_sprite(&art.sign_shape, SIGN_SHADOW, Vec2::splat(SIGN_SIZE)),
                // Down-and-right in the board's own frame would swing
                // with its heading; the offset is applied in world
                // space by `dress_signposts` for that reason.
                Transform::from_translation(Vec3::new(0.0, 0.0, -0.05)),
            ));
            parent.spawn((
                SignpostPaint,
                image_sprite(
                    &art.sign_paint,
                    palette::seat_paint(sp.owner),
                    Vec2::splat(SIGN_SIZE),
                ),
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.01)),
            ));
            // On a co-op stage the arrow also wears the claw it turns: a
            // small crab in the corner with its big claw on that side and
            // in that claw's colour, the same tell the crabs it steers
            // carry, so the seat colour is not the only cue.
            if board.rules().claw_hands.is_some()
                && let Some(handed) = crate::sim::claw_of(sp.owner)
            {
                parent.spawn(claw_badge(art, sp.dir, handed));
            }
        })
        .id()
}

/// A signpost's claw mark ([`spawn_signpost`]): drawn upright in a corner
/// of the tile whichever way the post points, so it is placed and turned
/// against the post's own rotation.
fn claw_badge(art: &Art, dir: Direction, handed: crate::sim::Handedness) -> impl Bundle {
    let undo = layout::dir_rotation(dir).inverse();
    let side = Vec2::splat(SIGN_SIZE * CLAW_BADGE);
    let corner = Vec3::new(SIGN_SIZE * 0.34, -SIGN_SIZE * 0.30, 0.03);
    let face_up = layout::dir_rotation(Direction::Up);
    let mirrored = crate::app::creatures::mirrored(handed);
    (
        SignpostClaw,
        Sprite {
            image: art.crab.clone(),
            color: palette::PARCHMENT,
            custom_size: Some(side),
            flip_y: mirrored,
            ..default()
        },
        Transform::from_translation(undo * corner).with_rotation(undo * face_up),
        Children::spawn_one((
            Sprite {
                image: art.claw.clone(),
                color: crate::app::creatures::claw_color(handed, true),
                custom_size: Some(side),
                flip_y: mirrored,
                ..default()
            },
            Transform::from_translation(Vec3::new(0.0, 0.0, 0.01)),
        )),
    )
}

/// The claw mark on a co-op stage's signpost: which crabs it turns.
#[derive(Component)]
pub struct SignpostClaw;

/// The claw mark's size, of a signpost's.
const CLAW_BADGE: f32 = 0.5;

/// The shadow a signpost drops, at full strength.
const SIGN_SHADOW: Color = Color::srgba(0.14, 0.10, 0.06, 0.34);

/// A post's shadow, told apart from the post and its other children so all
/// of them can be written in the same pass.
type ShadowOnly = (
    With<SignpostShadow>,
    Without<SignpostSprite>,
    Without<SignpostPaint>,
    Without<SignpostHalo>,
);
/// A post's paint, likewise.
type PaintOnly = (
    With<SignpostPaint>,
    Without<SignpostSprite>,
    Without<SignpostShadow>,
    Without<SignpostHalo>,
);
/// A post's halo, likewise.
type HaloOnly = (
    With<SignpostHalo>,
    Without<SignpostSprite>,
    Without<SignpostShadow>,
    Without<SignpostPaint>,
);

/// The children a post is drawn with besides its board.
#[derive(bevy::ecs::system::SystemParam)]
pub struct PostParts<'w, 's> {
    shadows: Query<'w, 's, (&'static mut Sprite, &'static mut Transform), ShadowOnly>,
    paints: Query<'w, 's, &'static mut Sprite, PaintOnly>,
    halos: Query<'w, 's, (&'static mut Sprite, &'static mut Visibility), HaloOnly>,
}

/// The two clocks a post is drawn on: the wall's for the settle after a
/// plant, stopped under a pause card (`state`), and the round's for the
/// next-to-go mark, which holds under the pause card and pulses with its
/// dot on the chip.
#[derive(bevy::ecs::system::SystemParam)]
pub struct PostClocks<'w> {
    time: Res<'w, Time>,
    round: Res<'w, crate::app::round_state::RoundClock>,
    state: crate::app::round_state::RoundState<'w>,
}

/// Write what a post looks like *now*: how far it has settled after being
/// planted, how worn it is, how much life it has left, and whether it is
/// the one the next placement will take.
///
/// That last is marked only for seats played at this screen (see
/// [`crate::app::seating::played_here`]): it glows and rocks in the
/// sand, in time with its dot on the owner's score chip.
///
/// Wear keeps a post's ink and takes its edges instead of dimming it away
/// (see [`post_alpha`]): a worn board is split and chipped
/// ([`crate::app::art::Art::sign_board_worn`]) and its paint flaking.
pub fn dress_signposts(
    clocks: PostClocks,
    sim: Res<Sim>,
    art: Res<Art>,
    settings: Res<crate::app::settings::GameSettings>,
    seating: crate::app::seating::Seating,
    mut posts: Query<(&mut SignpostSprite, &mut Sprite, &mut Transform, &Children)>,
    parts: PostParts,
) {
    use crate::app::side_panels::next_to_go_pulse;
    let PostParts {
        mut shadows,
        mut paints,
        mut halos,
    } = parts;
    let board = &sim.0;
    let PostClocks { time, round, state } = clocks;
    // The plant's pop holds under a pause card with the beach.
    let dt = match state.held() {
        true => 0.0,
        false => time.delta_secs(),
    };
    let next: [Option<(u8, u8)>; crate::sim::MAX_PLAYERS] = std::array::from_fn(|seat| {
        seating
            .played_here(seat as u8)
            .then(|| board.next_to_go(seat as u8))
            .flatten()
    });
    // The round's clock, not the wall's: the wobble and the pulse hold
    // still under the pause card, and the chip's dot reads the same one.
    let secs = round.0;
    let pulse = next_to_go_pulse(secs, settings.reduced_motion);
    for (mut post, mut sprite, mut transform, children) in &mut posts {
        post.age += dt;
        if !on_board(board, post.x, post.y) {
            continue; // `sync_signposts` takes it away this frame
        }
        let Some(sp) = board.signpost_at(post.x, post.y) else {
            continue;
        };
        // Driven in again: the same post, restamped. Replay the plant.
        if sp.placed != post.planted {
            post.planted = sp.placed;
            post.age = 0.0;
        }
        let worn = sp.health == SignpostHealth::Worn;
        let (board_art, paint_art) = if worn {
            (&art.sign_board_worn, &art.sign_paint_worn)
        } else {
            (&art.sign_board, &art.sign_paint)
        };
        if sprite.image != *board_art {
            sprite.image = board_art.clone();
        }
        // Versus posts age out; a puzzle's stand until pulled. What is
        // left of a post's life dims it, but never past the point where it
        // stops reading against the sand.
        let fade = board.signpost_fade(&sp);
        let alpha = post_alpha(fade);
        if sprite.color != Color::WHITE {
            sprite.color = Color::WHITE;
        }
        let mut paint = palette::seat_paint(post.owner);
        if worn {
            paint = paint.darker(0.06);
        }
        let pop = if settings.reduced_motion {
            1.0
        } else {
            plant_pop(post.age)
        };
        transform.scale = Vec3::splat(pop * wither(fade));
        let marked = next.get(usize::from(post.owner)).copied().flatten() == Some((post.x, post.y));
        // Rocked about its heading, so the shadow below is worked out from
        // the rotation it is actually drawn at.
        let rock = match marked && !settings.reduced_motion {
            true => WOBBLE * (secs * std::f32::consts::TAU * 2.4).sin(),
            false => 0.0,
        };
        let rotation = layout::dir_rotation(post.dir) * Quat::from_rotation_z(rock);
        if transform.rotation != rotation {
            transform.rotation = rotation;
        }
        for child in children {
            if let Ok((mut halo, mut shown)) = halos.get_mut(*child) {
                let want = match marked {
                    true => Visibility::Inherited,
                    false => Visibility::Hidden,
                };
                shown.set_if_neq(want);
                if marked {
                    layout::tint(&mut halo, palette::GOLD.with_alpha(pulse));
                }
                continue;
            }
            if let Ok(mut coat) = paints.get_mut(*child) {
                if coat.image != *paint_art {
                    coat.image = paint_art.clone();
                }
                layout::tint(&mut coat, paint.with_alpha(alpha));
                continue;
            }
            let Ok((mut shadow, mut shadow_tf)) = shadows.get_mut(*child) else {
                continue;
            };
            layout::tint(&mut shadow, SIGN_SHADOW);
            // The sun is one direction for the whole beach, so the offset
            // is undone out of the arrow's rotation: a post pointing left
            // and one pointing up drop their shadow the same way. Divided
            // by the pop, since a child's translation is in the parent's
            // frame and the 1.35x overshoot would fling the shadow out.
            let scale = transform.scale.x.max(f32::EPSILON);
            let local = transform.rotation.inverse() * (layout::SUN / scale).extend(0.0);
            shadow_tf.translation = Vec3::new(local.x, local.y, -0.05);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An aged post's paint never fades past the point its wood needs: the
    /// board stays solid, and the paint is what says whose post it is.
    #[test]
    fn a_posts_paint_never_fades_past_seventy_percent() {
        for step in 0..=10 {
            let alpha = post_alpha(step as f32 / 10.0);
            assert!(alpha >= 0.7 - 1e-6, "{alpha} at {step}");
        }
    }

    /// The plant has to hand over to rest without a step in it.
    ///
    /// Past `PLANT_POP` the size is a flat 1.0, so the curve has to arrive
    /// there on its own, or every post jumps on the frame the animation
    /// stops. Asserting the value *at* the boundary reads the flat branch,
    /// so this reads the last moment of the curve itself.
    #[test]
    fn the_plant_hands_over_to_rest_without_a_step() {
        assert!(plant_pop(0.0) > 1.3, "driven in big: {}", plant_pop(0.0));
        let last = plant_pop(PLANT_POP - 1e-4);
        assert!(
            (last - 1.0).abs() < 0.01,
            "the curve ends at {last}, and then snaps to 1.0"
        );
        assert_eq!(plant_pop(PLANT_POP), 1.0, "which is where rest begins");
        assert_eq!(plant_pop(PLANT_POP * 4.0), 1.0, "and stays");
    }

    /// It squashes on the way down rather than easing straight in - that
    /// dip under 1.0 is what makes it read as driven into sand rather than
    /// scaled up - but it must never invert or vanish.
    #[test]
    fn the_plant_squashes_without_ever_turning_inside_out() {
        let mut dipped = false;
        for step in 0..=40 {
            let pop = plant_pop(PLANT_POP * step as f32 / 40.0);
            assert!(pop > 0.5, "collapsed to {pop} at step {step}");
            assert!(pop < 1.4, "blew up to {pop} at step {step}");
            dipped |= pop < 0.98;
        }
        assert!(dipped, "no squash at all: it only ever grows");
    }

    /// Wear is spelled in the art now, not in transparency. Whatever is
    /// left of a post's life, it stays well clear of invisible: it is the
    /// one thing on the board the player is steering with, on bright sand.
    #[test]
    fn a_post_never_fades_past_reading() {
        for step in 0..=20 {
            let alpha = post_alpha(step as f32 / 20.0);
            assert!((0.55..=1.0).contains(&alpha), "{alpha} is off the scale");
        }
        assert_eq!(post_alpha(1.0), 1.0, "a fresh post is at full strength");
        assert!(post_alpha(0.0) >= 0.55, "and a spent one is still legible");
    }

    /// The alpha floor costs the fade most of its range, so the size
    /// carries the rest of the message: a post about to go is visibly
    /// smaller, and never small enough to be mistaken for somebody
    /// else's.
    #[test]
    fn an_expiring_post_withers_but_does_not_shrivel() {
        assert_eq!(wither(1.0), 1.0, "a fresh post is full size");
        assert!(wither(0.0) >= 0.86, "and a spent one is only a little less");
        let mut last = f32::MAX;
        for step in 0..=20 {
            let size = wither(1.0 - step as f32 / 20.0);
            assert!(size <= last, "it grew back at step {step}");
            last = size;
        }
    }
}
