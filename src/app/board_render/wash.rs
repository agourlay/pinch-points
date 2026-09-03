//! The tide coming in on a finished round.
//!
//! At zero the sim stops and the scores lock, and until now that was the
//! whole of it: the board froze, the card came up, and a round that had
//! been four players shouting at each other ended on an accountant's note.
//! The original washes the beach flat, and this is that - one wave off the
//! sea, over the sand, and away again, with everything the round built
//! under it.
//!
//! Render-only and frozen-safe. Nothing here reads or writes the sim, the
//! clock or the phase machine: the wave plays over whatever the board was
//! left holding, and the results card comes up on its own schedule as it
//! always did.

use super::{BoardStatic, z};
use crate::app::Sim;
use crate::app::art::Art;
use crate::app::layout::TILE;
use crate::app::sim_events::SimEvent;
use bevy::prelude::*;

/// A piece of the closing wave, with the wave's own clock on it. Both the
/// body of water and the foam on its front carry one, and both are placed
/// from the same age, so the lip can never come adrift of the water.
#[derive(Component)]
pub struct TideWash {
    age: f32,
}

/// The scalloped lip on the front of the wave.
#[derive(Component)]
pub struct WashFoam;

/// How long the wave takes to cross the beach.
const SWEEP: f32 = 0.85;
/// How long it lies over it, everything the round built underneath.
const HOLD: f32 = 0.3;
/// How long it takes to drain away, leaving the card standing.
const DRAIN: f32 = 0.7;

/// How far the wave is built past the board, in world units.
///
/// Sized from the world rather than from the board, and it has to be:
/// `boot::fit_camera` clamps the zoom at 0.8, so a board smaller than the
/// window leaves a great deal of beach visible around it, and a wave cut
/// to the board's own width ends in two hard vertical edges with dry sand
/// beyond them. `spawn_dusk_shore` reaches this far for the same reason,
/// and reads it from here.
pub(super) const REACH: f32 = 9000.0;

/// Send the wave in when the round ends.
///
/// Guarded against a second wave: `RoundEnded` is raised once, but the
/// differ that raises it works from a snapshot, and a round left and
/// re-entered inside the same board would otherwise stack two waves at
/// different ages on top of each other.
pub fn start_tide_wash(
    mut commands: Commands,
    mut events: MessageReader<SimEvent>,
    art: Res<Art>,
    settings: Res<crate::app::settings::GameSettings>,
    mut trauma: ResMut<crate::app::effects::Trauma>,
    running: Query<Entity, With<TideWash>>,
) {
    let arriving = events.read().any(|e| matches!(e, SimEvent::RoundEnded));
    if !arriving || settings.reduced_motion || !running.is_empty() {
        return;
    }
    // Placed properly on the first frame of `advance_tide_wash`; what
    // matters here is only that both pieces start at the same age.
    commands.spawn((
        BoardStatic,
        TideWash { age: 0.0 },
        Sprite {
            // A ramp rather than a flat fill: solid at the front and
            // thinning away behind it, so the sea reads as a wave with a
            // shoulder rather than as a slab of colour being slid down the
            // window. `ramp` is opaque along its top, so it is turned over.
            image: art.ramp.clone(),
            color: Color::srgba(0.21, 0.46, 0.67, 0.88),
            custom_size: Some(Vec2::ZERO),
            ..default()
        },
        Transform::from_translation(Vec3::new(0.0, 0.0, z::PIP + 0.1))
            .with_rotation(Quat::from_rotation_z(std::f32::consts::PI)),
    ));
    commands.spawn((
        BoardStatic,
        TideWash { age: 0.0 },
        WashFoam,
        Sprite {
            image: art.foam.clone(),
            color: Color::srgba(1.0, 1.0, 1.0, 0.92),
            custom_size: Some(Vec2::ZERO),
            ..default()
        },
        Transform::from_translation(Vec3::new(0.0, 0.0, z::PIP + 0.12)),
    ));
    // The sea arriving, felt rather than seen.
    trauma.add(0.45);
}

/// Where the wave's front stands, `age` seconds in, how deep the water
/// behind it is, and how strongly it is drawn.
///
/// Split out because the two ends are the whole of it: the front has to
/// start clear of the top of the board and finish with the whole beach
/// under water, or the wave is seen to begin, or to give up, in the middle
/// of the sand.
///
/// The body is the board's own height and a bit, not a slab reaching off
/// the top of the world. A wave wants a shoulder that thins away behind
/// the crest, and a shoulder taller than the window is a shoulder nobody
/// can see: it drew as a flat sheet of colour sliding down the glass.
fn wave(age: f32, half_height: f32) -> (f32, f32, f32) {
    // Every one of the three measured against the *view*, not the board.
    // The shoulder was, and the two ends were not, which left the sea
    // materialising over visible sand at the top of a tall window and
    // stopping short of the bottom with dry sand still under it: the
    // camera stops zooming in at 0.8, so a small board sits in a much
    // larger visible beach (see `REACH`).
    let reach = half_height.max(REACH / 4.0);
    let body = half_height * 2.0 + REACH / 2.0;
    let sky = reach + TILE;
    let deep = -reach - 3.0 * TILE;
    let sweep = (age / SWEEP).clamp(0.0, 1.0);
    // Eased out: the sea arrives quickly and settles, rather than sliding
    // down the board at one speed.
    let eased = 1.0 - (1.0 - sweep) * (1.0 - sweep);
    let front = sky + (deep - sky) * eased;
    let drained = ((age - SWEEP - HOLD) / DRAIN).clamp(0.0, 1.0);
    (front, body, 1.0 - drained)
}

/// Carry the wave down the beach, hold it there, and drain it away.
pub fn advance_tide_wash(
    time: Res<Time>,
    sim: Res<Sim>,
    mut commands: Commands,
    mut pieces: Query<(
        Entity,
        &mut TideWash,
        &mut Sprite,
        &mut Transform,
        Option<&WashFoam>,
    )>,
) {
    let board = &sim.0;
    let dt = time.delta_secs();
    let half_h = f32::from(board.height()) * TILE / 2.0;
    let span = REACH;
    for (entity, mut wash, mut sprite, mut transform, foam) in &mut pieces {
        wash.age += dt;
        if wash.age >= SWEEP + HOLD + DRAIN {
            commands.entity(entity).despawn();
            continue;
        }
        let (front, body, strength) = wave(wash.age, half_h);
        // A small heave along the front, so the sea does not arrive as a
        // ruled line.
        let front = front + (wash.age * 9.0).sin() * 3.0;
        let peak = base_alpha(foam);
        sprite.color.set_alpha(strength * peak);
        transform.translation.x = 0.0;
        if foam.is_some() {
            sprite.custom_size = Some(Vec2::new(span, 24.0));
            transform.translation.y = front;
            continue;
        }
        // The body of water behind the lip, thinning away up its back.
        sprite.custom_size = Some(Vec2::new(span, body));
        transform.translation.y = front + body / 2.0;
    }
}

/// How strong each piece is at full flood: the foam reads brighter than
/// the water it rides on.
fn base_alpha(foam: Option<&WashFoam>) -> f32 {
    if foam.is_some() { 0.92 } else { 0.88 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wave has to arrive from off the beach and leave the whole of it
    /// under water: a front that starts on the sand is a sheet appearing
    /// out of nothing, and one that stops short leaves a dry strip along
    /// the top row with the round's last castle standing in it.
    #[test]
    fn the_wave_comes_in_off_the_sea_and_covers_the_beach() {
        let half_height = 5.0 * TILE;
        let (front, body, strength) = wave(0.0, half_height);
        assert!(front > half_height, "starts clear of the top: {front}");
        assert!(
            front + body > half_height.max(REACH / 4.0),
            "with its shoulder clear of the top of the view"
        );
        assert_eq!(strength, 1.0, "at full flood the moment it is spawned");
        let (front, body, _) = wave(SWEEP, half_height);
        let window_bottom = -half_height.max(REACH / 4.0);
        assert!(
            front < window_bottom,
            "the front passes the bottom of the view, not just of the board: {front}"
        );
        // Against the *window*, not the board. The camera stops zooming in
        // at 0.8, so a small board sits in a much larger visible beach and
        // a wave measured against the board alone ends on screen.
        let window_top = half_height.max(REACH / 4.0);
        assert!(
            front + body > window_top,
            "and the water still covers the top of the view: {front} + {body}"
        );
    }

    /// It drains to exactly nothing, and only after it has held. A wave
    /// that started fading on its way in never covers anything.
    #[test]
    fn the_wave_holds_before_it_drains() {
        let half_height = 5.0 * TILE;
        // A hair under one: the age is a sum of three constants and the
        // subtraction inside `wave` does not land on zero exactly.
        assert!(
            wave(SWEEP + HOLD, half_height).2 > 0.999,
            "still at flood when the hold ends"
        );
        let (_, _, strength) = wave(SWEEP + HOLD + DRAIN / 2.0, half_height);
        assert!(
            (0.1..0.9).contains(&strength),
            "half drained, not all or nothing: {strength}"
        );
        assert_eq!(wave(SWEEP + HOLD + DRAIN, half_height).2, 0.0, "and gone");
    }
}
