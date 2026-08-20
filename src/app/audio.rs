//! Sound effects, driven by the [`SimEvent`] stream (one observer diffs the
//! sim; this module just maps events to one-shots and rumble). Win/lose
//! stingers hook the puzzle phase transitions; denied placements arrive on
//! their own message from the input layer.
//!
//! Anything that happens *somewhere* on the beach is panned to that side of
//! the stereo field ([`pan_pos`]); banners and stingers, which belong to the
//! whole round rather than a tile, stay centred.

use crate::app::sim_events::SimEvent;
use crate::app::{PlacementDenied, Screen, Sim};
use bevy::audio::{SpatialListener, SpatialScale, Volume};
use bevy::input::gamepad::{GamepadRumbleIntensity, GamepadRumbleRequest};
use bevy::prelude::*;
use std::time::Duration;

/// Marker for the looping background theme entity.
#[derive(Component)]
pub struct Music;

#[derive(Resource)]
pub struct Sounds {
    place: Handle<AudioSource>,
    remove: Handle<AudioSource>,
    bank: Handle<AudioSource>,
    eat: Handle<AudioSource>,
    raid: Handle<AudioSource>,
    win: Handle<AudioSource>,
    lose: Handle<AudioSource>,
    takeoff: Handle<AudioSource>,
    screech: Handle<AudioSource>,
    event: Handle<AudioSource>,
    golden: Handle<AudioSource>,
    tier: Handle<AudioSource>,
    surge: Handle<AudioSource>,
    horn: Handle<AudioSource>,
    denied: Handle<AudioSource>,
}

/// The rotating background playlist; a track spawns, plays once, despawns,
/// and `rotate_music` starts the next.
#[derive(Resource)]
pub struct MusicPlaylist {
    tracks: Vec<Handle<AudioSource>>,
    next: usize,
}

/// How far, in listener units, the edge of the board sits from the centre
/// of the stereo field. Small on purpose: rodio only attenuates by distance
/// beyond one unit, so keeping the whole beach inside that radius leaves
/// panning as the only thing that varies.
const PAN_WIDTH: f32 = 0.5;
/// Distance between the listener's ears, same units.
const EAR_GAP: f32 = 0.3;
/// A panned one-shot's two channels sum to less than a centred one's, so
/// positional sounds get a lift to sit level with the stingers.
const SPATIAL_GAIN: f32 = 1.4;

/// Where a board position lands in the listener's little stereo field.
///
/// The x is mirrored: rodio's spatial mixer boosts the ear *further* from
/// the emitter, so without the flip a raid on the left of the beach would
/// come out of the right speaker.
pub(crate) fn pan_pos(half_width: f32, pos: Vec2) -> Vec3 {
    let x = (pos.x / half_width.max(1.0)).clamp(-1.0, 1.0);
    Vec3::new(-x * PAN_WIDTH, 0.0, 0.0)
}

pub fn load_sounds(mut commands: Commands, assets: Res<AssetServer>) {
    // One listener, parked at the origin; emitters are placed relative to it
    // by `pan_pos` rather than at their true board positions, so the camera
    // zooming or sliding never changes how the beach sounds.
    commands.spawn(SpatialListener::new(EAR_GAP));
    commands.insert_resource(MusicPlaylist {
        tracks: vec![
            // theme.wav predates the sound generator and has no source to
            // re-encode from; the generated loops ship as OGG.
            assets.load("sounds/theme.wav"),
            assets.load("sounds/theme_b.ogg"),
            assets.load("sounds/theme_c.ogg"),
            assets.load("sounds/theme_d.ogg"),
            assets.load("sounds/theme_e.ogg"),
            assets.load("sounds/theme_f.ogg"),
            assets.load("sounds/theme_g.ogg"),
        ],
        next: 0,
    });
    commands.insert_resource(Sounds {
        place: assets.load("sounds/place.wav"),
        remove: assets.load("sounds/remove.wav"),
        bank: assets.load("sounds/bank.wav"),
        eat: assets.load("sounds/eat.wav"),
        raid: assets.load("sounds/raid.wav"),
        win: assets.load("sounds/win.wav"),
        lose: assets.load("sounds/lose.wav"),
        takeoff: assets.load("sounds/takeoff.wav"),
        screech: assets.load("sounds/screech.wav"),
        event: assets.load("sounds/event.wav"),
        golden: assets.load("sounds/golden.wav"),
        tier: assets.load("sounds/tier.wav"),
        surge: assets.load("sounds/surge.wav"),
        horn: assets.load("sounds/horn.wav"),
        denied: assets.load("sounds/denied.wav"),
    });
}

/// A one-shot at the given gain, centred in the stereo field. A gain of
/// zero (the slider at the bottom) spawns nothing at all.
fn play(commands: &mut Commands, sound: &Handle<AudioSource>, gain: f32) {
    if gain <= 0.0 {
        return;
    }
    commands.spawn((
        AudioPlayer::new(sound.clone()),
        PlaybackSettings {
            volume: Volume::Linear(gain),
            ..PlaybackSettings::DESPAWN
        },
    ));
}

/// A one-shot panned to a spot in the listener's field (see [`pan_pos`]).
fn play_at(commands: &mut Commands, sound: &Handle<AudioSource>, gain: f32, at: Vec3) {
    if gain <= 0.0 {
        return;
    }
    commands.spawn((
        AudioPlayer::new(sound.clone()),
        PlaybackSettings {
            volume: Volume::Linear(gain * SPATIAL_GAIN),
            spatial: true,
            spatial_scale: Some(SpatialScale::new_2d(1.0)),
            ..PlaybackSettings::DESPAWN
        },
        Transform::from_translation(at),
    ));
}

/// Map each sim event to its one-shot (and rumble where it matters).
#[allow(clippy::too_many_arguments)]
pub fn play_events(
    mut commands: Commands,
    mut events: MessageReader<SimEvent>,
    sounds: Res<Sounds>,
    screen: Res<State<Screen>>,
    sim: Res<Sim>,
    settings: Res<crate::app::settings::GameSettings>,
    pads: Query<Entity, With<Gamepad>>,
    mut rumble: MessageWriter<GamepadRumbleRequest>,
) {
    let mut buzz = |ms: u64, strength: f32| {
        if !settings.rumble {
            return;
        }
        for gamepad in &pads {
            rumble.write(GamepadRumbleRequest::Add {
                duration: Duration::from_millis(ms),
                intensity: GamepadRumbleIntensity::strong_motor(strength),
                gamepad,
            });
        }
    };
    let gain = settings.sfx_gain();
    let half_width = f32::from(sim.0.width()) * crate::app::layout::TILE / 2.0;
    let pan = |pos: &Vec2| pan_pos(half_width, *pos);
    for event in events.read() {
        match event {
            SimEvent::CrabBanked { kind, pos, .. } => {
                play_at(&mut commands, &sounds.bank, gain, pan(pos));
                if *kind == crate::sim::CrabKind::Golden {
                    play_at(&mut commands, &sounds.golden, gain, pan(pos));
                }
            }
            SimEvent::CrabEaten { pos } => play_at(&mut commands, &sounds.eat, gain, pan(pos)),
            SimEvent::CastleRaided { pos, .. } => {
                play_at(&mut commands, &sounds.raid, gain, pan(pos));
                buzz(400, 0.7);
            }
            SimEvent::GullArrived => play(&mut commands, &sounds.screech, gain),
            SimEvent::GullTookOff => play(&mut commands, &sounds.takeoff, gain),
            SimEvent::SignpostsChanged { delta } => {
                play(
                    &mut commands,
                    if *delta > 0 {
                        &sounds.place
                    } else {
                        &sounds.remove
                    },
                    gain,
                );
            }
            SimEvent::SignpostEvicted { pos, .. } => {
                play_at(&mut commands, &sounds.denied, gain, pan(pos));
            }
            SimEvent::TierUp { .. } => play(&mut commands, &sounds.tier, gain),
            SimEvent::TideEventFired { .. } => play(&mut commands, &sounds.event, gain),
            SimEvent::SurgeStarted => {
                play(&mut commands, &sounds.surge, gain);
                buzz(300, 0.4);
            }
            SimEvent::RoundEnded => {
                // The horn calls the tide in versus; puzzle endings have
                // their own win/lose stingers.
                if *screen.get() == Screen::Versus {
                    play(&mut commands, &sounds.horn, gain);
                    buzz(500, 0.5);
                }
            }
            SimEvent::CrabSpawned { .. } | SimEvent::GullLanded { .. } => {}
        }
    }
}

pub fn play_win(
    mut commands: Commands,
    sounds: Res<Sounds>,
    screen: Res<State<Screen>>,
    settings: Res<crate::app::settings::GameSettings>,
) {
    if *screen.get() == Screen::Puzzle {
        play(&mut commands, &sounds.win, settings.sfx_gain());
    }
}

pub fn play_lose(
    mut commands: Commands,
    sounds: Res<Sounds>,
    screen: Res<State<Screen>>,
    settings: Res<crate::app::settings::GameSettings>,
) {
    if *screen.get() == Screen::Puzzle {
        play(&mut commands, &sounds.lose, settings.sfx_gain());
    }
}

/// One dull knock per frame with at least one rejected placement.
pub fn play_denied(
    mut commands: Commands,
    mut denials: MessageReader<PlacementDenied>,
    sounds: Option<Res<Sounds>>,
    settings: Res<crate::app::settings::GameSettings>,
) {
    let any = denials.read().next().is_some();
    denials.clear();
    if any && let Some(sounds) = sounds {
        play(&mut commands, &sounds.denied, settings.sfx_gain());
    }
}

/// A bright unlock chime (the tier fanfare doing double duty).
pub fn play_chime(commands: &mut Commands, sounds: &Sounds, gain: f32) {
    play(commands, &sounds.tier, gain);
}

/// Keep the playlist rolling: whenever no track entity is alive, start the
/// next one. Tracks despawn when they finish; a paused track (M) persists,
/// so the toggle keeps working mid-song.
pub fn rotate_music(
    mut commands: Commands,
    mut playlist: ResMut<MusicPlaylist>,
    settings: Res<crate::app::settings::GameSettings>,
    playing: Query<(), With<Music>>,
) {
    if !playing.is_empty() {
        return;
    }
    let track = playlist.tracks[playlist.next].clone();
    playlist.next = (playlist.next + 1) % playlist.tracks.len();
    commands.spawn((
        Music,
        AudioPlayer::new(track),
        PlaybackSettings {
            volume: Volume::Linear(settings.music_gain()),
            ..PlaybackSettings::DESPAWN
        },
    ));
}

/// The tide nudge: over the last 30 seconds of a versus round the music
/// speeds up, ramping to +35% at the wave. Everywhere else it plays
/// straight. Speed-only (pitch rises with it, which is the point).
pub fn surge_tempo(
    sim: Res<crate::app::Sim>,
    screen: Res<State<Screen>>,
    sinks: Query<&AudioSink, With<Music>>,
) {
    let ramp = if *screen.get() == Screen::Versus {
        match sim.0.remaining_ticks() {
            Some(ticks) if ticks <= 900 => 1.0 + 0.35 * (1.0 - ticks as f32 / 900.0),
            _ => 1.0,
        }
    } else {
        1.0
    };
    for sink in &sinks {
        if (sink.speed() - ramp).abs() > 0.005 {
            sink.set_speed(ramp);
        }
    }
}

/// M toggles the background music anywhere: the cap that says M, on
/// whatever keyboard this is.
pub fn toggle_music(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<crate::app::settings::GameSettings>,
    sinks: Query<&AudioSink, With<Music>>,
) {
    if settings.keycaps.just_pressed(&keys, 'M') {
        for sink in &sinks {
            sink.toggle_playback();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stereo field mirrors the board (rodio boosts the far ear), tops
    /// out at the edges, and never reaches far enough from the listener for
    /// distance attenuation to make a far-off bank quieter than a near one.
    #[test]
    fn panning_mirrors_the_board_and_stays_inside_the_listener() {
        let half = 6.0 * crate::app::layout::TILE;
        assert_eq!(pan_pos(half, Vec2::ZERO).x, 0.0, "the middle is centred");
        let left = pan_pos(half, Vec2::new(-half, 0.0));
        let right = pan_pos(half, Vec2::new(half, 0.0));
        assert_eq!(left.x, PAN_WIDTH, "mirrored: left of the beach, left ear");
        assert_eq!(right.x, -left.x, "the two edges are symmetric");
        // Off-board positions clamp rather than sliding out of the field.
        assert_eq!(pan_pos(half, Vec2::new(-9.0 * half, 0.0)).x, PAN_WIDTH);
        // rodio attenuates beyond one unit; every ear-to-emitter distance
        // must stay inside that, or the pan would double as a volume drop.
        const { assert!(PAN_WIDTH + EAR_GAP / 2.0 < 1.0) };
    }
}
