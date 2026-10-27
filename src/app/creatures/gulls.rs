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
        spawn_gull(&mut commands, &art, board, gull);
    }
    seen.clear();
    seen.extend(board.gulls().iter().map(|gull| gull.id));
}

/// `gull` where the board has it, with its shadow: the one gull a round
/// draws ([`sync_gull_sprites`], which adds the puff of its arrival), and
/// a map card's preview too ([`super::spawn_creatures`]).
pub(crate) fn spawn_gull(
    commands: &mut Commands,
    art: &Art,
    board: &crate::sim::Board,
    gull: &Gull,
) -> Entity {
    let pos = layout::creature_pos(board, gull.tile, gull.dir, gull.progress);
    commands
        .spawn((
            GullSprite(gull.id),
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
        })
        .id()
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
            layout::tint(
                &mut shadow_sprite,
                Color::srgba(1.0, 1.0, 1.0, 1.0 - 0.15 * altitude),
            );
        }
    }
}

/// One piece of a gull flying in to land (see [`fly_in_gulls`]), keyed by
/// the arrival it draws: the tick it lands on and its tile.
#[derive(Component)]
pub struct GullArrival {
    at: u64,
    tile: u16,
    part: ArrivalPart,
}

/// The three sprites a gull flying in is drawn with.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArrivalPart {
    /// The bird, in the air.
    Bird,
    /// Its spread wings' shadow on the sand while it is high.
    Wings,
    /// The round shadow every gull on the beach casts, which the wings'
    /// fold into as it comes down.
    Oval,
}

/// How much of a gull's lead its shadow takes to sweep in over the edge
/// onto its landing tile. Under half: for the rest of the lead the shadow
/// lies on that tile while the bird comes down onto it.
const SHADOW_SWEEP: f32 = 0.45;

/// Where a gull's shadow starts its sweep, in tiles from the landing tile:
/// this far back beyond the edge, and this far along it, so it glides in
/// at a slant, as a bird coming down does.
const SHADOW_BEHIND: f32 = 1.3;
const SHADOW_ALONG: f32 = 1.0;

/// How far the bird trails its shadow back along the way it came, in
/// tiles at the top of its flight. It closes to nothing as it lands.
const BIRD_TRAIL: f32 = 1.6;

/// The highest a gull flying in is, in the units of [`Flight`]'s altitude.
const ARRIVAL_HEIGHT: f32 = 2.0;

/// A gull flying in, `progress` of the way through its lead: where its
/// shadow lies (on the ground, the sun's offset included, without the
/// wing beat's drift), where the bird is, how high, and which way it flies.
#[derive(Clone, Copy, Debug)]
struct ArrivalPose {
    shadow: Vec2,
    bird: Vec2,
    altitude: f32,
    heading: f32,
}

/// Where a gull flying in to `land` (a tile on the beach's edge, which it
/// lands on facing `facing`, into the beach) is drawn `progress` of the way
/// through its lead.
///
/// The shadow is the warning, so the shadow leads and the bird follows. It
/// sweeps in over the edge onto the landing tile in the first
/// [`SHADOW_SWEEP`] of the lead and lies there for the rest, where it
/// would lie under a standing gull: whichever edge the gull comes in over,
/// a player sees the tile it comes down on for most of the lead. The bird
/// is placed from its shadow rather than the shadow from the bird: high,
/// it trails back along its line of flight, and it closes onto the shadow
/// as it comes down, landing on it. Not by the sun's throw, the offset
/// `interpolate_gulls` drops a hopping gull's shadow by: over the bottom
/// edge that throw points back out of the beach, and the two cancel, so
/// the bird sat on its shadow from the start. Which way along
/// the edge it comes from is the tile's parity, so two landing side by
/// side do not fly the same line.
fn arrival_pose(progress: f32, land: Vec2, facing: Vec2, tile: u16) -> ArrivalPose {
    let along = if tile.is_multiple_of(2) { 1.0 } else { -1.0 };
    let rest = land + layout::SUN;
    let entry = rest - facing * TILE * SHADOW_BEHIND + facing.perp() * along * TILE * SHADOW_ALONG;
    let shadow = entry.lerp(rest, effects::smoothstep(progress / SHADOW_SWEEP));
    let altitude = ARRIVAL_HEIGHT * (1.0 - effects::smoothstep(progress)).powf(1.1);
    let back = (entry - rest).normalize_or_zero();
    let high = altitude / ARRIVAL_HEIGHT;
    let bird = shadow - layout::SUN + back * TILE * BIRD_TRAIL * high;
    ArrivalPose {
        shadow,
        bird,
        altitude,
        heading: (-back).to_angle(),
    }
}

/// The sprites a gull flying in is drawn with: all three in flight, and
/// under reduced motion only the round shadow fading in on its tile.
fn arrival_parts(calm: bool) -> &'static [ArrivalPart] {
    match calm {
        true => &[ArrivalPart::Oval],
        false => &[ArrivalPart::Bird, ArrivalPart::Wings, ArrivalPart::Oval],
    }
}

/// Draw each gull the edge spawner has sent for as it flies in.
///
/// The sim picks a gull's landing tile [`crate::sim::GULL_LEAD`] ticks
/// before it lands, so for that long the board knows where a bird will
/// come down that is not on the sand yet. That lead is drawn as the bird
/// gliding in from beyond the beach, high and large at first, behind its
/// shadow, a soft spread-wing silhouette that sweeps in over the edge and
/// settles on the landing tile well before the bird ([`arrival_pose`]):
/// that is the warning. As the bird comes down it closes onto the shadow,
/// which folds into the round one a standing gull casts, and on the
/// landing tick the bird is handed over to [`sync_gull_sprites`] on the
/// very spot, in its puff of sand.
///
/// Everything is read off the board each frame, the flight's progress
/// included (the sim's tick between frames, like every creature), so it
/// holds still under a pause and a board laid out mid-flight draws its
/// gulls already on their way. The sprites follow the setting each frame
/// too: turning reduced motion on or off mid-flight takes away or brings
/// back the parts it draws. Once the round is over, which stops the board
/// (a stage won on its goal stops it mid-lead), a gull still on its
/// way never lands, so it is not drawn hanging in the air.
///
/// Reduced motion keeps the warning and drops the flight: the round
/// shadow fades in on the landing tile and the gull simply appears.
pub fn fly_in_gulls(
    current: CurrentBoard,
    fx: effects::Fx,
    tween: Tween,
    mut parts: Query<(Entity, &GullArrival, &mut Transform, &mut Sprite)>,
) {
    let effects::Fx {
        mut commands,
        art,
        rng: _,
        settings,
    } = fx;
    let Tween {
        time,
        fixed_time,
        mut watch,
        round,
        round_clock,
    } = tween;
    let board = &current.sim.0;
    let over = round.ended(board);
    let incoming = if over {
        &[][..]
    } else {
        board.incoming_gulls()
    };
    let wanted = arrival_parts(settings.reduced_motion);
    for (entity, part, _, _) in &parts {
        let flying = incoming
            .iter()
            .any(|gull| gull.at == part.at && gull.tile == part.tile);
        if !flying || !wanted.contains(&part.part) {
            commands.entity(entity).despawn();
        }
    }
    for gull in incoming {
        for &part in wanted {
            let drawn = parts
                .iter()
                .any(|(_, p, _, _)| p.at == gull.at && p.tile == gull.tile && p.part == part);
            if drawn {
                continue;
            }
            let (image, size) = match part {
                ArrivalPart::Bird => (&art.gull_fly, Vec2::splat(TILE * 0.78)),
                ArrivalPart::Wings => (&art.gull_shadow, Vec2::splat(TILE * 0.78 * 1.05)),
                ArrivalPart::Oval => (&art.shadow, Vec2::new(TILE * 0.72, TILE * 0.54)),
            };
            commands.spawn((
                GullArrival {
                    at: gull.at,
                    tile: gull.tile,
                    part,
                },
                layout::image_sprite(image, Color::NONE, size),
                // Placed by the pass below on the frame it shows.
                Transform::from_translation(Vec3::new(0.0, 0.0, layout::z::CREATURE)),
            ));
        }
    }
    let alpha = smoothing_alpha(board.ticks(), &mut watch, time.delta_secs(), &fixed_time);
    // The moment drawn, in ticks: the creatures are drawn `alpha` of the
    // way from the last tick's poses to this one's, and a gull that lands
    // on tick `at` is first drawn standing at `at` exactly.
    let now = board.ticks() as f32 - 1.0 + alpha;
    let lead = crate::sim::GULL_LEAD as f32;
    let t = round_clock.0;
    for (_, part, mut transform, mut sprite) in &mut parts {
        let Some(gull) = incoming
            .iter()
            .find(|gull| gull.at == part.at && gull.tile == part.tile)
        else {
            continue;
        };
        let progress = ((now - (gull.at as f32 - lead)) / lead).clamp(0.0, 1.0);
        let land = layout::creature_pos(board, gull.tile, gull.dir, 0);
        let facing = (layout::dir_rotation(gull.dir) * Vec3::X).truncate();
        let landed_heading = facing.to_angle();
        if settings.reduced_motion {
            transform.translation = (land + layout::SUN).extend(layout::z::CREATURE - 0.4);
            transform.rotation = Quat::from_rotation_z(landed_heading);
            transform.scale = Vec3::ONE;
            layout::tint(
                &mut sprite,
                Color::srgba(1.0, 1.0, 1.0, effects::smoothstep(progress)),
            );
            continue;
        }
        let pose = arrival_pose(progress, land, facing, gull.tile);
        let altitude = pose.altitude;
        let phase = f32::from(gull.tile % 32);
        let beat = (t * 16.0 + phase).sin();
        let upstroke = beat > 0.35;
        let scale = 1.0 + 0.22 * altitude + beat * 0.05;
        // Nose along its line of flight, turning to face into the beach
        // over the last quarter, the short way round.
        let turn = effects::smoothstep((progress - 0.75) / 0.25);
        let swing = (landed_heading - pose.heading + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        let heading = Quat::from_rotation_z(pose.heading + swing * turn);
        // The same slow circling drift a hopping gull's shadow has.
        let drift = Vec2::new((t * 2.1 + phase).cos() * 2.5, (t * 1.7 + phase).sin() * 2.5);
        let shadow = pose.shadow + drift * altitude;
        let wings = effects::smoothstep(altitude / 0.7);
        match part.part {
            ArrivalPart::Bird => {
                transform.translation = pose.bird.extend(layout::z::ARRIVING_GULL);
                transform.scale = Vec3::splat(scale);
                layout::tint(&mut sprite, Color::WHITE);
                let wanted = if upstroke {
                    &art.gull_fly_b
                } else {
                    &art.gull_fly
                };
                if sprite.image != *wanted {
                    sprite.image = wanted.clone();
                }
            }
            ArrivalPart::Wings => {
                transform.translation = shadow.extend(layout::z::CREATURE - 0.45);
                transform.scale = Vec3::splat(scale);
                layout::tint(&mut sprite, Color::srgba(1.0, 1.0, 1.0, 0.85 * wings));
                let wanted = if upstroke {
                    &art.gull_shadow_b
                } else {
                    &art.gull_shadow
                };
                if sprite.image != *wanted {
                    sprite.image = wanted.clone();
                }
            }
            ArrivalPart::Oval => {
                transform.translation = shadow.extend(layout::z::CREATURE - 0.4);
                transform.scale = Vec3::splat(1.0 - 0.25 * altitude);
                let oval = Color::srgba(1.0, 1.0, 1.0, (1.0 - 0.15 * altitude) * (1.0 - wings));
                layout::tint(&mut sprite, oval);
            }
        }
        transform.rotation = heading;
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

    /// The shadow of a gull flying in is the warning, so it is on the sand
    /// before the bird, whichever edge it comes in over: for at least half
    /// the lead it lies inside the beach within a tile of the tile the
    /// gull lands on, and it is there, settled, while the bird is still up
    /// and away. Both end on the landing spot together.
    #[test]
    fn a_gull_flying_in_casts_its_shadow_on_its_tile_first() {
        use crate::sim::Direction;
        let land = Vec2::new(100.0, -40.0);
        for dir in [
            Direction::Down,
            Direction::Up,
            Direction::Left,
            Direction::Right,
        ] {
            let facing = (layout::dir_rotation(dir) * Vec3::X).truncate();
            for tile in [6u16, 7] {
                let steps = 100;
                let warned = (0..=steps)
                    .filter(|&step| {
                        let pose = arrival_pose(step as f32 / steps as f32, land, facing, tile);
                        let off = pose.shadow - land;
                        // Not out past the beach's edge, which is half a
                        // tile behind the landing tile's centre.
                        off.dot(facing) >= -TILE * 0.5 && off.length() <= TILE
                    })
                    .count();
                assert!(
                    warned * 2 >= steps,
                    "{dir:?} tile {tile}: on the sand for {warned} of {steps}"
                );
                let half = arrival_pose(0.5, land, facing, tile);
                assert!(
                    half.shadow.distance(land + layout::SUN) < 1.0,
                    "{dir:?}: settled on its tile by half way"
                );
                assert!(
                    half.bird.distance(land) > TILE * 0.5,
                    "{dir:?}: while the bird is still on its way"
                );
                let landed = arrival_pose(1.0, land, facing, tile);
                assert!(
                    landed.bird.distance(land) < 1e-3,
                    "{dir:?}: lands on its tile"
                );
                assert!(landed.shadow.distance(land + layout::SUN) < 1e-3);
                assert_eq!(landed.altitude, 0.0);
            }
        }
    }

    /// A stage that runs [`fly_in_gulls`] on a board with one gull on its
    /// way in, mid versus round.
    fn arriving(calm: bool) -> App {
        use crate::app::{Screen, VersusPhase};
        let mut app = App::new();
        let mut board = crate::sim::Board::new(9, 7, 3);
        board.rules_mut().gull_period = 500;
        board.tick_idle();
        assert_eq!(board.incoming_gulls().len(), 1, "one sent for");
        app.insert_resource(crate::app::Sim(board));
        app.init_resource::<crate::app::sim_events::BoardGeneration>();
        app.insert_resource(Art::blank());
        app.init_resource::<effects::VisualRng>();
        app.insert_resource(crate::app::settings::GameSettings {
            reduced_motion: calm,
            ..default()
        });
        app.init_resource::<Time>();
        app.init_resource::<Time<Fixed>>();
        app.init_resource::<crate::app::round_state::RoundClock>();
        app.insert_resource(State::new(Screen::Versus));
        app.insert_resource(State::new(VersusPhase::Running));
        app.init_resource::<crate::app::Paused>();
        app.init_resource::<crate::app::pause::PauseMenu>();
        app.init_resource::<crate::app::net::Online>();
        app.add_systems(Update, fly_in_gulls);
        app
    }

    /// The parts of the gull flying in on screen, and the oval's scale.
    fn drawn_parts(app: &mut App) -> (Vec<bool>, Option<Vec3>) {
        let world = app.world_mut();
        let mut parts = [false; 3];
        let mut oval = None;
        for (arrival, place) in world.query::<(&GullArrival, &Transform)>().iter(world) {
            let index = match arrival.part {
                ArrivalPart::Bird => 0,
                ArrivalPart::Wings => 1,
                ArrivalPart::Oval => {
                    oval = Some(place.scale);
                    2
                }
            };
            parts[index] = true;
        }
        (parts.to_vec(), oval)
    }

    fn set_calm(app: &mut App, calm: bool) {
        app.world_mut()
            .resource_mut::<crate::app::settings::GameSettings>()
            .reduced_motion = calm;
    }

    /// Reduced motion turned on or off while a gull is flying in redraws it
    /// as the setting now asks: the bird and its wings go and the oval
    /// lies at its full size, or the bird and its wings come back.
    #[test]
    fn a_gull_flying_in_follows_the_motion_setting_mid_flight() {
        let mut app = arriving(false);
        app.update();
        app.update();
        assert_eq!(drawn_parts(&mut app).0, [true, true, true], "in flight");
        set_calm(&mut app, true);
        app.update();
        app.update();
        let (parts, oval) = drawn_parts(&mut app);
        assert_eq!(parts, [false, false, true], "only the oval");
        assert_eq!(oval, Some(Vec3::ONE), "at its full size");
        set_calm(&mut app, false);
        app.update();
        app.update();
        assert_eq!(drawn_parts(&mut app).0, [true, true, true], "flying again");
    }

    /// A round over (a stage won on its goal stops the board) never
    /// lands a gull still on its way in, so none is left hanging in the air.
    #[test]
    fn a_gull_flying_in_when_the_round_ends_is_not_left_in_the_air() {
        let mut app = arriving(false);
        app.update();
        app.update();
        assert_eq!(drawn_parts(&mut app).0, [true, true, true], "in flight");
        app.insert_resource(State::new(crate::app::VersusPhase::Over));
        app.update();
        app.update();
        assert_eq!(drawn_parts(&mut app).0, [false, false, false], "gone");
    }
}
