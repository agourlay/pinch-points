//! The crabs a Crab Monopoly sweep carries home, glowing where they were
//! picked up and then thrown over the beach into the banker's castle.

use super::{Tween, claw_color, mirrored, shade_of, shell_color, smoothing_alpha, sprite_side};
use crate::app::art::Art;
use crate::app::layout;
use crate::app::palette;
use crate::app::{Sim, effects};
use bevy::color::Mix;
use bevy::prelude::*;

/// A crab Crab Monopoly is carrying home, by id: drawn from the sim's own
/// flight ([`crate::sim::Sweeping`]), since it banks on the tick it lands.
#[derive(Component)]
pub struct SweepSprite {
    id: u32,
    /// The keep it is flying to: its owner's castle, as last seen.
    keep: Vec2,
    /// Where it was headed when a Castle Swap moved its owner's keep in
    /// mid-air, and when, in ticks: it turns toward the new keep from
    /// there rather than jumping onto a new arc.
    veer: Option<(Vec2, f32)>,
}

impl SweepSprite {
    pub(crate) fn new(id: u32, keep: Vec2) -> SweepSprite {
        SweepSprite {
            id,
            keep,
            veer: None,
        }
    }

    /// Where the crab is flying to at `now`, with its owner's castle at
    /// `castle`.
    ///
    /// The sim banks a swept crab into its owner's score wherever that
    /// castle stands, and a Castle Swap moves it. Read afresh every frame,
    /// every crab in the air snapped onto a new arc at the swap; flown to
    /// the old keep, it landed in a castle that now flies someone else's
    /// flag while its owner's score rose across the beach. So it turns: from
    /// where it was headed toward the new keep, arriving there exactly as
    /// it lands, which is where its bank is told (`SimEvent::CrabBanked`'s
    /// `keep`). One
    /// still on the sand has shown nobody where it was going, and simply
    /// goes to the new one.
    fn heading(&mut self, castle: Vec2, now: f32, launch: f32, lands: f32) -> Vec2 {
        if castle != self.keep {
            self.veer = (now > launch).then(|| (self.aim(now, lands), now));
            self.keep = castle;
        }
        self.aim(now, lands)
    }

    fn aim(&self, now: f32, lands: f32) -> Vec2 {
        match self.veer {
            None => self.keep,
            Some((was, at)) => {
                let turned = ((now - at) / (lands - at).max(1.0)).clamp(0.0, 1.0);
                was.lerp(self.keep, turned)
            }
        }
    }
}

/// Draw the crabs a Crab Monopoly sweep is carrying home: glowing gold
/// where they were picked up until they set off, then thrown over the
/// beach into the banker's castle, each landing on the tick it banks.
pub fn draw_sweeps(
    mut commands: Commands,
    sim: Res<Sim>,
    art: Res<Art>,
    settings: Res<crate::app::settings::GameSettings>,
    mut tween: Tween,
    mut sprites: Query<(Entity, &mut SweepSprite, &mut Transform, &mut Sprite)>,
) {
    let board = &sim.0;
    let flights = board.sweeping();
    if flights.is_empty() && sprites.is_empty() {
        return;
    }
    let time = &tween.time;
    let alpha = smoothing_alpha(
        board.ticks(),
        &mut tween.watch,
        time.delta_secs(),
        &tween.fixed_time,
    );
    // The last tick run, and how far into the next one this frame is.
    let now = board.ticks() as f32 - 1.0 + alpha;
    // A steady glow for reduced motion: the sweep is the rule, so the
    // crabs are still drawn, only not pulsing.
    let glow = if settings.reduced_motion {
        0.6
    } else {
        0.5 + 0.3 * (time.elapsed_secs() * 9.0).sin()
    };
    let mut drawn: Vec<u32> = Vec::with_capacity(flights.len());
    for (entity, mut sweep, mut transform, mut sprite) in &mut sprites {
        let Some(flight) = flights.iter().find(|f| f.crab.id == sweep.id) else {
            commands.entity(entity).despawn();
            continue;
        };
        drawn.push(sweep.id);
        let crab = &flight.crab;
        let from = layout::creature_pos(board, crab.tile, crab.dir, crab.progress);
        let launch = flight.launch as f32;
        let lands = flight.lands.max(flight.launch + 1) as f32;
        let keep = sweep.heading(keep_of(board, flight, from), now, launch, lands);
        let shell = shell_color(crab.kind, shade_of(crab.id));
        if now < launch {
            transform.translation = from.extend(effects::SWEEP_WAIT_Z);
            transform.scale = Vec3::ONE;
            sprite.color = shell.mix(&palette::GOLD, glow);
            continue;
        }
        let progress = ((now - launch) / (lands - launch)).clamp(0.0, 1.0);
        let (at, scale) = effects::sweep_arc(from, keep, progress);
        transform.translation = at;
        transform.scale = Vec3::splat(scale);
        sprite.color = shell;
    }
    for flight in flights {
        let crab = &flight.crab;
        if drawn.contains(&crab.id) {
            continue;
        }
        let pos = layout::creature_pos(board, crab.tile, crab.dir, crab.progress);
        let side = Vec2::splat(sprite_side(crab.kind));
        let flip_y = mirrored(crab.handed);
        commands
            .spawn((
                SweepSprite::new(crab.id, keep_of(board, flight, pos)),
                Sprite {
                    image: art.crab.clone(),
                    color: shell_color(crab.kind, shade_of(crab.id)).mix(&palette::GOLD, glow),
                    custom_size: Some(side),
                    flip_y,
                    ..default()
                },
                Transform::from_translation(pos.extend(effects::SWEEP_WAIT_Z))
                    .with_rotation(layout::dir_rotation(crab.dir)),
            ))
            .with_child((
                Sprite {
                    image: art.claw.clone(),
                    color: claw_color(crab.handed),
                    custom_size: Some(side),
                    flip_y,
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.1)),
            ));
    }
}

/// The middle of the castle a swept crab's owner holds now, or `from`
/// for an owner with none, which the sim never sweeps for.
fn keep_of(board: &crate::sim::Board, flight: &crate::sim::Sweeping, from: Vec2) -> Vec2 {
    board
        .castle_of(flight.owner)
        .map_or(from, |(x, y)| layout::tile_center(board, x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Castle Swap in mid-sweep moves the keep a crab in the air is
    /// flying to. It turns toward the new one from where it was headed,
    /// with no jump on the frame of the swap, and lands in the new keep
    /// on its tick, where the bank is told; one still waiting on the sand
    /// just goes to the new keep.
    #[test]
    fn a_crab_in_the_air_turns_toward_a_swapped_keep() {
        let (old, new) = (Vec2::new(100.0, 0.0), Vec2::new(-100.0, 50.0));
        let (launch, lands) = (10.0, 40.0);
        let mut sweep = SweepSprite::new(1, old);
        assert_eq!(sweep.heading(old, 20.0, launch, lands), old);
        assert_eq!(
            sweep.heading(new, 25.0, launch, lands),
            old,
            "no jump on the frame of the swap"
        );
        let halfway = sweep.heading(new, 32.5, launch, lands);
        assert!(
            halfway.distance(old.lerp(new, 0.5)) < 1e-3,
            "turning: {halfway:?}"
        );
        assert_eq!(sweep.heading(new, lands, launch, lands), new, "lands home");

        let mut waiting = SweepSprite::new(2, old);
        assert_eq!(
            waiting.heading(new, 5.0, launch, lands),
            new,
            "nobody saw where it was going"
        );
    }
}
