//! Render-side particles and floating score text: the game's moments made
//! visible. Everything here is cosmetic: spawned from *observed* sim state,
//! driven by frame time, and drawing on its own PRNG so the deterministic
//! sim stream is never touched.

use crate::app::art::Art;
use crate::app::layout::{self, TILE};
use crate::app::materials::PRINT;
use crate::app::palette;
use crate::app::sim_events::{BoardMemory, CurrentBoard, SimEvent};
use crate::sim::TileKind;
use bevy::color::Mix;
use bevy::prelude::*;

mod hops;
mod particle;
mod water;
pub use hops::*;
pub use particle::*;
pub use water::*;

/// Which claw a crab put down last. A print is offset to one side of the
/// stride and the sides alternate.
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

/// What putting a flourish on screen takes: somewhere to spawn it, the
/// art, the dice it is thrown with, and the settings that say whether the
/// player asked for less motion. Every system that decorates the beach
/// reads these four together.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Fx<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub art: Res<'w, Art>,
    pub rng: ResMut<'w, VisualRng>,
    pub settings: Res<'w, crate::app::settings::GameSettings>,
}

/// Whether `event` still draws anything under reduced motion. The news
/// stays and the fireworks go: a raid's number, a gull seen leaving (or it
/// went *in*, which is a raid), a banked crab fading at the gate rather
/// than vanishing, and the ghost of a post traded away, which is how a
/// player sees which arrow they lost.
fn kept_when_calm(event: &SimEvent) -> bool {
    matches!(
        event,
        SimEvent::CastleRaided { .. }
            | SimEvent::GullShooed { .. }
            | SimEvent::GullFlewOff { .. }
            | SimEvent::CrabBanked { .. }
            | SimEvent::SignpostEvicted { .. }
    )
}

/// Turn sim events into their on-board moments: pips, puffs, flashes, and
/// the shove the camera takes for the loud ones.
pub fn moment_effects(
    fx: Fx,
    mut events: MessageReader<SimEvent>,
    cursors: Query<&crate::app::cursor::Cursor>,
    hops: Query<&Hop>,
    mut trauma: ResMut<Trauma>,
    sim: Res<crate::app::Sim>,
) {
    let Fx {
        mut commands,
        art,
        mut rng,
        settings,
    } = fx;
    // Reduced motion keeps the news and drops the fireworks: the score pip
    // for a raid still floats up (it is the only place that number appears),
    // but the puffs, the scatter, the shake and the full-tile white flash
    // do not. A gull turned away at a puzzle's castle still fades where it
    // stood, or it vanished into the keep, which is what a raid looks like.
    // What survives is [`kept_when_calm`].
    let calm = settings.reduced_motion;
    // Crabs swept home so far this frame, so each sets off after the last.
    let mut sweeps = 0usize;
    // The keeps with a crab still waiting at the door, from an earlier
    // frame or this one, and how long until the last of them sets off.
    let mut doorsteps: Vec<(Vec2, f32)> = Vec::new();
    for hop in &hops {
        if hop.path != HopPath::OverWall || hop.age >= 0.0 {
            continue;
        }
        match doorsteps
            .iter_mut()
            .find(|(at, _)| at.distance(hop.to) < 1.0)
        {
            Some((_, wait)) => *wait = wait.max(-hop.age),
            None => doorsteps.push((hop.to, -hop.age)),
        }
    }
    for event in events.read() {
        if calm && !kept_when_calm(event) {
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
                handed,
                points,
                owner: _,
            } => {
                let way = homeward(*pos, *keep, *points, &mut sweeps, &mut doorsteps);
                let crab = BankedCrab {
                    id: *id,
                    from: *pos,
                    to: *keep,
                    kind: *kind,
                    handed: *handed,
                    coop: crate::app::creatures::coop_claws(&sim.0),
                    points: *points,
                };
                if calm {
                    bank_fade(&mut commands, &art, &crab);
                } else {
                    bank_hop(&mut commands, &art, &crab, way);
                }
            }
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
            // sees the arrow they lost, and where they lost it. Calm, it
            // fades where it stood without swelling: still the news.
            //
            // Only your own (or every one, watching), by the rule the
            // trade's sound asks: a bot past its cap trades a post every
            // few seconds, and a ghost for each was a stream of arrows
            // nobody here had lost.
            SimEvent::SignpostEvicted { owner, pos, dir }
                if crate::app::cursor::post_noticed_here(&cursors, *owner) =>
            {
                commands.spawn((
                    Particle {
                        grow: if calm { 0.0 } else { 1.1 },
                        life: 0.5,
                        fade_in: 0.0,
                        ..default()
                    },
                    // The arrow off the lost post, in its owner's paint.
                    layout::image_sprite(
                        &art.sign_paint,
                        palette::seat_paint(*owner),
                        Vec2::splat(layout::SIGN_SIZE),
                    ),
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
                    layout::image_sprite(
                        &art.puff,
                        Color::srgba(1.0, 1.0, 1.0, 1.0),
                        Vec2::splat(TILE * 1.1),
                    ),
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
            // Turned away at a puzzle's castle. The bird has to be seen
            // leaving, or it went *in*, and in versus that is a raid.
            SimEvent::GullShooed { pos, castle } => {
                shooed(&mut commands, &art, *pos, *castle, calm);
            }
            // Made room in a full flock: up and away from where it stood,
            // with no ring, since nothing here turned it away.
            SimEvent::GullFlewOff { pos } => {
                flying_away(&mut commands, &art, *pos, Vec2::Y, calm);
            }
            // A post going in gets a ring under it; the pop of the post
            // itself belongs to the sprite, which `board_render` owns.
            //
            // Only your own (or every one, watching), for the reason the
            // knock is only your own: on a six-seat beach five bots place
            // and lose a post every few seconds each, and a ring for every
            // one of them is a stream of particles over tiles nobody is
            // watching. The picture and the sound ask the one rule, so they
            // agree about whose beach this is, a watcher's included.
            SimEvent::SignpostPlaced { owner, pos }
                if crate::app::cursor::post_noticed_here(&cursors, *owner) =>
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
                // Driven in: the sand the stake shoved aside, thrown out
                // low and falling straight back.
                burst(
                    &mut commands,
                    &mut rng,
                    &Burst {
                        image: art.puff.clone(),
                        pos: *pos,
                        color: SAND_KICK,
                        count: 6,
                        size: 7.0,
                        speed: 48.0,
                        gravity: 95.0,
                    },
                );
            }
            SimEvent::SignpostRemoved { owner, pos }
                if crate::app::cursor::post_noticed_here(&cursors, *owner) =>
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
            SimEvent::SignpostPlaced { .. }
            | SimEvent::SignpostRemoved { .. }
            | SimEvent::SignpostEvicted { .. } => {}
            // The final scramble arrives with the sea behind it.
            SimEvent::SurgeStarted => trauma.add(0.7),
            // Out at last: what the rings have been pointing at. Gold
            // thrown up out of the hole, and a ring wide enough to catch an
            // eye watching another corner of the board.
            SimEvent::GoldenOut { pos } => {
                burst(
                    &mut commands,
                    &mut rng,
                    &Burst {
                        image: art.star.clone(),
                        pos: *pos,
                        color: palette::GOLD,
                        count: 14,
                        size: 14.0,
                        speed: 120.0,
                        gravity: 90.0,
                    },
                );
                ring(
                    &mut commands,
                    &art,
                    *pos,
                    palette::GOLD,
                    TILE * 0.7,
                    4.5,
                    0.8,
                );
                ring(
                    &mut commands,
                    &art,
                    *pos,
                    Color::srgba(1.0, 1.0, 0.85, 0.9),
                    TILE * 0.4,
                    3.0,
                    0.5,
                );
                trauma.add(0.25);
            }
            SimEvent::GoldenCalled { pos } => {
                ring(
                    &mut commands,
                    &art,
                    *pos,
                    palette::GOLD,
                    TILE * 0.6,
                    3.0,
                    0.6,
                );
            }
            SimEvent::GullIncoming { .. }
            | SimEvent::GullArrived
            | SimEvent::GullTookOff
            | SimEvent::TierUp { .. }
            | SimEvent::TideEventFired { .. }
            | SimEvent::LureStarted { .. }
            | SimEvent::WheelLocked { .. }
            | SimEvent::LureRefused { .. }
            | SimEvent::RoundEnded => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reduced motion drops the fireworks and keeps the news. A banked crab
    /// and a post traded away are news: dropped, the crab vanished on the
    /// doorstep and the lost arrow went without a trace. Their calm shape
    /// stands still and only fades.
    #[test]
    fn reduced_motion_keeps_a_bank_and_a_lost_post_without_moving_them() {
        let banked = SimEvent::CrabBanked {
            id: 0,
            owner: 0,
            pos: Vec2::ZERO,
            keep: Vec2::X,
            points: 1,
            kind: crate::sim::CrabKind::Common,
            handed: crate::sim::Handedness::Right,
        };
        let evicted = SimEvent::SignpostEvicted {
            owner: 0,
            pos: Vec2::ZERO,
            dir: crate::sim::Direction::Up,
        };
        assert!(kept_when_calm(&banked), "a bank still shows");
        assert!(kept_when_calm(&evicted), "and so does a lost post");
        assert!(
            !kept_when_calm(&SimEvent::CrabEaten { pos: Vec2::ZERO }),
            "the feathers of an eaten crab are fireworks"
        );
        let calm = standing_fade();
        assert_eq!(calm.velocity, Vec2::ZERO, "it does not drift");
        assert_eq!(calm.gravity, Vec2::ZERO, "or fall");
        assert_eq!(calm.grow, 0.0, "or swell");
        assert_eq!(calm.spin, 0.0, "or turn");
        assert!(calm.life > 0.0, "it is there for a moment, then gone");
    }

    /// A beach for the effect systems to draw on: `seats` cursors at this
    /// machine (seats past them are bots, none at all is watching), and
    /// reduced motion when `calm`.
    pub(super) fn stage(seats: u8, calm: bool) -> App {
        let mut app = App::new();
        app.insert_resource(Art::blank());
        app.init_resource::<VisualRng>();
        app.init_resource::<Trauma>();
        app.insert_resource(crate::app::settings::GameSettings {
            reduced_motion: calm,
            ..default()
        });
        app.init_resource::<crate::app::sim_events::BoardGeneration>();
        app.add_message::<SimEvent>();
        app.insert_resource(State::new(crate::app::Screen::Versus));
        app.init_resource::<crate::app::Paused>();
        app.init_resource::<crate::app::pause::PauseMenu>();
        app.init_resource::<crate::app::net::Online>();
        app.insert_resource(crate::app::Sim(crate::sim::Board::new(4, 4, 0)));
        for seat in 0..seats {
            app.world_mut()
                .spawn(crate::app::cursor::Cursor::seated(seat));
        }
        app
    }

    /// The particles drawn since the last call, swept away for the next.
    pub(super) fn drawn(app: &mut App) -> usize {
        let world = app.world_mut();
        let live: Vec<Entity> = world
            .query_filtered::<Entity, With<Particle>>()
            .iter(world)
            .collect();
        for &entity in &live {
            world.entity_mut(entity).despawn();
        }
        live.len()
    }

    /// The ghost of a post traded away asks the rule its sound asks: your
    /// own lost arrow, or every one when watching. A bot past its cap
    /// trades a post every few seconds, and drew a ghost for each, calm or
    /// not, while the speaker kept quiet about them.
    #[test]
    fn a_lost_post_is_drawn_where_it_is_heard() {
        let traded = |owner| SimEvent::SignpostEvicted {
            owner,
            pos: Vec2::ZERO,
            dir: crate::sim::Direction::Up,
        };
        for calm in [false, true] {
            let mut app = stage(1, calm);
            app.add_systems(Update, moment_effects);
            app.world_mut().write_message(traded(0));
            app.update();
            assert_eq!(drawn(&mut app), 1, "your own lost arrow (calm: {calm})");
            app.world_mut().write_message(traded(4));
            app.update();
            assert_eq!(drawn(&mut app), 0, "not a bot's (calm: {calm})");
        }
        let mut app = stage(0, false);
        app.add_systems(Update, moment_effects);
        app.world_mut().write_message(traded(4));
        app.update();
        assert_eq!(drawn(&mut app), 1, "a watcher sees every seat's");
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
}
