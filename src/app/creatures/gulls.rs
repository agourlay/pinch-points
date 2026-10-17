//! Gull sprites: their arrival, their walk, and the hop drawn as a
//! flight with its shadow left on the sand.

use super::{CreatureShadow, Tween, smoothing_alpha, tweened_pos, unturned};
use crate::app::art::Art;
use crate::app::effects;
use crate::app::layout::{self, TILE};
use crate::app::sim_events::{BoardMemory, BoardStamp, CurrentBoard};
use crate::sim::{Gull, GullState};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

#[derive(Component)]
pub struct GullSprite(pub u32);

/// Spawn sprites for new gulls, despawn sprites for departed ones.
pub fn sync_gull_sprites(
    current: CurrentBoard,
    fx: effects::Fx,
    mut live: Local<HashMap<u32, Gull>>,
    // The gulls that were on the board last frame, and which board that
    // was. A *sprite* being new is not a gull arriving: a level with gulls
    // written into it, a retry, a resumed round and an editor test run all
    // wipe the sprites and rebuild them.
    mut before: Local<(BoardStamp, Vec<u32>)>,
    existing: Query<(Entity, &GullSprite)>,
) {
    let effects::Fx {
        mut commands,
        art,
        mut rng,
        settings,
    } = fx;
    let board = &current.sim.0;
    // A different board, whose gull ids start again at zero, or a board at
    // tick zero that has not run at all: nothing on either has "arrived",
    // the birds on it are the ones it was written with, or (a round pasted
    // in mid-clock, a watcher handed the round as it stands, the first time
    // or again, since every board laid out counts as new) the ones already
    // there. So the memory is reseeded from those birds rather than
    // emptied, which otherwise puffs sand under every gull on the beach.
    let (last_board, seen) = &mut *before;
    let new_board = last_board.is_new(&current);
    if new_board || board.ticks() == 0 {
        seen.clear();
        seen.extend(board.gulls().iter().map(|g| g.id));
    }
    live.clear();
    live.extend(board.gulls().iter().map(|g| (g.id, *g)));
    for (entity, sprite) in &existing {
        if live.remove(&sprite.0).is_none() {
            commands.entity(entity).despawn();
        }
    }
    for (&id, gull) in live.iter() {
        let pos = layout::creature_pos(board, gull.tile, gull.dir, gull.progress);
        // A gull arriving is the worst news on the beach: it lands in a
        // puff of sand, never in silence at the edge.
        if !settings.reduced_motion && !seen.contains(&id) {
            kick_up(&mut commands, &mut rng, &art, pos);
        }
        commands
            .spawn((
                GullSprite(id),
                layout::image_sprite(&art.gull, Color::WHITE, Vec2::splat(TILE * 0.78)),
                Transform::from_translation(pos.extend(layout::z::CREATURE + 0.2))
                    .with_rotation(layout::dir_rotation(gull.dir)),
            ))
            .with_children(|parent| {
                parent.spawn((
                    CreatureShadow,
                    layout::image_sprite(
                        &art.shadow,
                        Color::WHITE,
                        Vec2::new(TILE * 0.72, TILE * 0.54),
                    ),
                    Transform::from_translation(Vec3::new(0.0, 0.0, -0.5)),
                ));
            });
    }
    seen.clear();
    seen.extend(board.gulls().iter().map(|gull| gull.id));
}

/// A gull's sprite, as the interpolation moves and dresses it.
type GullBody = (
    &'static GullSprite,
    &'static mut Transform,
    &'static mut Sprite,
    &'static Children,
);

/// The gulls on screen and the shadows under them.
#[derive(bevy::ecs::system::SystemParam)]
pub struct GullSprites<'w, 's> {
    bodies: Query<'w, 's, GullBody, Without<CreatureShadow>>,
    shadows: Query<'w, 's, (&'static mut Transform, &'static mut Sprite), With<CreatureShadow>>,
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
/// back under it as it drops: a tile from landing, its shadow is already
/// on the tile it will land on.
///
/// The flight scale is kept modest. Walls draw above creatures, but a
/// border plank is thirteen pixels of a tile-sized frame, so a gull scaled
/// far past its tile hangs over the edge of the board with nothing to
/// occlude it.
pub fn interpolate_gulls(
    current: CurrentBoard,
    fx: effects::Fx,
    tween: Tween,
    mut by_id: Local<HashMap<u32, Gull>>,
    // Gulls currently in the air. Presence is the flight: a gull that
    // turns up here without an entry has just taken off, and one whose
    // entry outlives its flight has just landed.
    mut aloft: Local<BoardMemory<u32, Flight>>,
    gulls: GullSprites,
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
        round,
        round_clock,
    } = tween;
    let GullSprites {
        bodies: mut sprites,
        mut shadows,
    } = gulls;
    let board = &current.sim.0;
    let alpha = smoothing_alpha(board.ticks(), &mut watch, time.delta_secs(), &fixed_time);
    let stopped = !round.moving(board);
    by_id.clear();
    by_id.extend(board.gulls().iter().map(|g| (g.id, *g)));
    // Kept per board: gull ids start again at zero on every one, and
    // pruned by id alone a recycled id inherits a latched descent and a
    // frozen altitude, so the bird slides flat over the sand with its
    // shadow pinned under it. Nothing is aloft on a board at tick zero.
    let aloft = aloft.of(&current);
    if board.ticks() == 0 {
        aloft.clear();
    }
    aloft.retain(|id, _| by_id.contains_key(id));
    // The wing beat and the circling shadow run with the round, so a gull
    // caught aloft by a pause or the results holds its pose.
    let t = round_clock.0;
    for (sprite, mut transform, mut image, children) in &mut sprites {
        let Some(gull) = by_id.get(&sprite.0) else {
            continue;
        };
        let prev = layout::pose_pos(board, gull.prev);
        let curr = layout::pose_pos(board, gull.pose());
        let pos = tweened_pos(prev, curr, alpha);
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
        // Spread-wing art in flight, and it flaps: the upstroke frame for
        // the rising half of the same wing-beat the scale pulses with
        // below, so the two agree (write-on-change).
        let beat = (t * 16.0 + phase).sin();
        let wanted = match (flying, beat > 0.35) {
            (true, true) => &art.gull_fly_b,
            (true, false) => &art.gull_fly,
            (false, _) => &art.gull,
        };
        if image.image != *wanted {
            image.image = wanted.clone();
        }
        let (scale, waddle) = if flying {
            (1.0 + 0.22 * altitude + beat * 0.05, 0.0)
        } else if prev == curr || stopped {
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
            // is, with a slow circling drift on top of the drop: the cue
            // that it is airborne. A walking gull's shadow sits at its
            // feet, at the offset everything else on the beach casts.
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
    /// The load-bearing part is that the last airborne tile is the one with
    /// `remaining == 1`, not zero. The sim lands a gull on the tick its
    /// counter would reach zero, so `Flying { remaining: 0 }` is a state no
    /// frame observes: reading the descent off it holds the bird at full
    /// height and then drops it in one frame.
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

    /// The last tile a gull flies is the one with `remaining == 1`, because
    /// the sim lands it on the tick the counter would reach zero. Reading
    /// the descent off zero holds the bird at full height and then drops it
    /// in a single frame.
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
}
