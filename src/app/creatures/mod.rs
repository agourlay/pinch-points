//! Creature sprites: one entity per live crab and gull, keyed by the
//! sim's stable ids, and the crabs a Crab Monopoly sweep is carrying.
//!
//! Every frame, transforms are interpolated between a creature's previous
//! and current sub-tile positions using the fixed-timestep overstep
//! fraction (spec §7.4). The crabs are in [`crabs`], the gulls in
//! [`gulls`], and the sweep in [`sweeps`]; the clocks they are drawn
//! between ticks by, and the shadow every one of them casts, are here.

mod crabs;
mod gulls;
mod sweeps;

pub use crabs::{CrabSprite, interpolate_crabs, sync_crab_sprites};
pub(crate) use crabs::{body_color, claw_color, mirrored, shade_of, shell_color, sprite_side};
pub use gulls::{GullSprite, interpolate_gulls, sync_gull_sprites};
pub use sweeps::{SweepSprite, draw_sweeps};

use crate::app::layout::TILE;
use bevy::prelude::*;

/// The soft blob under a creature. Gull shadows detach in flight.
#[derive(Component)]
pub struct CreatureShadow;

/// Smoothing factor for creature sprites. Normally the fixed-clock overstep
/// fraction; when the sim has been frozen for clearly longer than one fixed
/// period (pause, results, round over), hold at the final position instead of
/// oscillating. The 1.5-period threshold is the load-bearing part: at 60+ fps
/// most render frames legitimately fall *between* ticks, and treating those
/// as frozen snaps sprites forward on alternating frames (visible chop).
fn smoothing_alpha(
    ticks: u64,
    watch: &mut (u64, f32),
    delta_secs: f32,
    fixed_time: &Time<Fixed>,
) -> f32 {
    let (last_tick, frozen_for) = watch;
    if ticks == *last_tick {
        *frozen_for += delta_secs;
    } else {
        *last_tick = ticks;
        *frozen_for = 0.0;
    }
    if *frozen_for > fixed_time.timestep().as_secs_f32() * 1.5 {
        1.0
    } else {
        fixed_time.overstep_fraction()
    }
}

/// The clocks a creature is drawn between ticks by: the frame's, the
/// sim's fixed one, and the watch [`smoothing_alpha`] keeps on the tick.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Tween<'w, 's> {
    time: Res<'w, Time>,
    fixed_time: Res<'w, Time<Fixed>>,
    watch: Local<'s, (u64, f32)>,
}

/// Where a creature is drawn `alpha` of the way from its last tick's place
/// to this one's. A wrap-around crossing is a teleport, not a walk: it
/// snaps to the far side rather than sliding across the whole board.
fn tweened_pos(prev: Vec2, curr: Vec2, alpha: f32) -> Vec2 {
    if prev.distance(curr) > TILE * 2.0 {
        curr
    } else {
        prev.lerp(curr, alpha)
    }
}

/// A world-space offset expressed in the local space of a sprite that has
/// been turned to face its heading.
///
/// Creature sprites are rotated to their direction and their shadows are
/// children of them, so a shadow given a plain local offset swings round
/// the creature as it turns corners. Undoing the parent's rotation is what
/// keeps the sun still while the crab walks under it.
fn unturned(parent: &Transform, world: Vec2, z: f32) -> Vec3 {
    let local = parent.rotation.inverse() * world.extend(0.0);
    Vec3::new(local.x, local.y, z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::layout;

    /// The sun stays put while the creature turns: a shadow given a plain
    /// local offset swings round its owner at every corner, and `unturned`
    /// has to undo that exactly, for every heading.
    #[test]
    fn a_shadow_holds_its_bearing_through_every_turn() {
        for dir in [
            crate::sim::Direction::Up,
            crate::sim::Direction::Down,
            crate::sim::Direction::Left,
            crate::sim::Direction::Right,
        ] {
            let parent = Transform::from_rotation(layout::dir_rotation(dir));
            let local = unturned(&parent, layout::SUN, -0.5);
            // Put back through the parent's own rotation, it is the world
            // offset again: that is the whole contract.
            let world = parent.rotation * local;
            assert!(
                world.truncate().distance(layout::SUN) < 1e-4,
                "{dir:?} bent the sun to {:?}",
                world.truncate()
            );
            assert_eq!(local.z, -0.5, "and the layer is left alone");
        }
    }

    /// A 30 Hz fixed clock with `overstep` of a period already accumulated,
    /// so the "between ticks" answer is a fraction the "frozen" answer
    /// cannot be mistaken for.
    fn fixed_clock(overstep: f32) -> Time<Fixed> {
        let mut fixed = Time::<Fixed>::from_hz(30.0);
        fixed.accumulate_overstep(fixed.timestep().mul_f32(overstep));
        fixed
    }

    /// The 1.5-period threshold is the load-bearing part: a render frame
    /// that falls between ticks at 60+ fps is not a frozen sim, and calling
    /// it one snaps sprites forward on alternating frames. So a sim that
    /// has not ticked for 1.2 periods still interpolates, 1.6 periods holds
    /// at the final position, and the first new tick starts the count over.
    #[test]
    fn the_sim_counts_as_frozen_only_past_one_and_a_half_periods() {
        let fixed = fixed_clock(0.4);
        let period = fixed.timestep().as_secs_f32();
        let between = fixed.overstep_fraction();
        assert!((0.0..1.0).contains(&between), "a fraction, got {between}");
        let mut watch = (5u64, 0.0f32);
        // Three frames of 0.4 periods: 1.2 periods without a tick.
        let mut alpha = 0.0;
        for _ in 0..3 {
            alpha = smoothing_alpha(5, &mut watch, period * 0.4, &fixed);
        }
        assert_eq!(
            alpha, between,
            "1.2 periods without a tick still interpolates"
        );
        // One more frame makes 1.6 periods: frozen, hold the final position.
        let alpha = smoothing_alpha(5, &mut watch, period * 0.4, &fixed);
        assert_eq!(alpha, 1.0, "1.6 periods without a tick holds still");
        // The sim ticks: the frozen count resets and interpolation resumes.
        let alpha = smoothing_alpha(6, &mut watch, period * 0.4, &fixed);
        assert_eq!(alpha, between, "a fresh tick resets the frozen count");
        assert_eq!(watch, (6, 0.0), "the watch follows the new tick");
    }
}
