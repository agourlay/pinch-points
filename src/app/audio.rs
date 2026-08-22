//! Sound effects, driven by the [`SimEvent`] stream (one observer diffs the
//! sim; this module just maps events to one-shots and rumble). Win/lose
//! stingers hook the puzzle phase transitions; denied placements arrive on
//! their own message from the input layer.
//!
//! Anything that happens *somewhere* on the beach is panned to that side of
//! the stereo field ([`pan_pos`]); banners and stingers, which belong to the
//! whole round rather than a tile, stay centred.
//!
//! Three things can silence the game, and they compose rather than fight:
//! the two settings switches (music and effects, each with its own volume
//! underneath), the master mute on M ([`Muted`]), and the pause card, which
//! puts the music down for as long as it is up. Everything reads them
//! through [`sfx_gain`] and [`music_audible`], so there is one answer to
//! "should this be heard" and no sink is left holding an opinion of its
//! own.

use crate::app::sim_events::SimEvent;
use crate::app::{PlacementDenied, Screen, Sim};
use bevy::audio::{SpatialListener, SpatialScale, Volume};
use bevy::input::gamepad::{GamepadRumbleIntensity, GamepadRumbleRequest};
use bevy::prelude::*;
use std::time::Duration;

/// Marker for the looping background theme entity.
#[derive(Component)]
pub struct Music;

/// The master mute on M: everything off, now, without touching what the
/// player has set.
///
/// Deliberately not a setting. The two switches on the settings card are
/// preferences and are written to disk; this is the key you hit when
/// somebody walks in, and the game comes back the way you left it. Which
/// also settles what M means when the music is already switched off in
/// settings: unmuting restores the settings, and they still say off.
#[derive(Resource, Default)]
pub struct Muted(pub bool);

/// Effects gain right now: the settings switch and slider, with the master
/// mute able to veto. Zero means nothing is spawned at all.
pub(crate) fn sfx_gain(settings: &crate::app::settings::GameSettings, muted: &Muted) -> f32 {
    if muted.0 { 0.0 } else { settings.sfx_gain() }
}

/// Whether the theme should be audible this frame. The pause card is the
/// third voice here, and the one that is not a player preference: it holds
/// the music down for as long as it is up and gives back exactly that.
///
/// Deriving the sink's state from all three every frame is what lets the
/// answer be this short. The card used to *do* something to the sink and
/// so had to remember whether the silence was its to lift - M could have
/// got there first, and a muted round must not come back singing. Nothing
/// pushes at the sink any more, so there is nothing to remember.
fn music_audible(
    settings: &crate::app::settings::GameSettings,
    muted: &Muted,
    card_open: bool,
) -> bool {
    settings.music_gain() > 0.0 && !muted.0 && !card_open
}

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
    evict: Handle<AudioSource>,
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
        evict: assets.load("sounds/evict.wav"),
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

/// A panned one-shot that sounds at most once a frame: `used` is the
/// caller's "already played this one" flag, and the first call through it
/// wins.
///
/// Signpost cues are per tile, so one frame can hold several of a kind -
/// every seat may act on the same tick, and posts placed together wear out
/// together. Sample-aligned copies of one sound do not read as several
/// things happening; they read as one louder, dirtier version of it, and
/// four of them at full gain clip. This is the rule the denial knock has
/// always used, applied where the volume actually is.
fn once(
    commands: &mut Commands,
    used: &mut bool,
    sound: &Handle<AudioSource>,
    gain: f32,
    at: Vec3,
) {
    if !std::mem::replace(used, true) {
        play_at(commands, sound, gain, at);
    }
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
    muted: Res<Muted>,
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
    let gain = sfx_gain(&settings, &muted);
    let half_width = f32::from(sim.0.width()) * crate::app::layout::TILE / 2.0;
    let pan = |pos: &Vec2| pan_pos(half_width, *pos);
    let (mut placed, mut removed, mut evicted) = (false, false, false);
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
            SimEvent::SignpostPlaced { pos } => {
                once(&mut commands, &mut placed, &sounds.place, gain, pan(pos));
            }
            SimEvent::SignpostRemoved { pos } => {
                once(&mut commands, &mut removed, &sounds.remove, gain, pan(pos));
            }
            // Its own sample, not the denial knock: a fourth post at the
            // cap is a placement that *worked*, and answering it with the
            // sound of a refusal told the player the opposite of what
            // happened. The placement sounds too, from the new tile.
            SimEvent::SignpostEvicted { pos, .. } => {
                once(&mut commands, &mut evicted, &sounds.evict, gain, pan(pos));
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
    muted: Res<Muted>,
) {
    if *screen.get() == Screen::Puzzle {
        play(&mut commands, &sounds.win, sfx_gain(&settings, &muted));
    }
}

pub fn play_lose(
    mut commands: Commands,
    sounds: Res<Sounds>,
    screen: Res<State<Screen>>,
    settings: Res<crate::app::settings::GameSettings>,
    muted: Res<Muted>,
) {
    if *screen.get() == Screen::Puzzle {
        play(&mut commands, &sounds.lose, sfx_gain(&settings, &muted));
    }
}

/// One dull knock per frame with at least one rejected placement.
pub fn play_denied(
    mut commands: Commands,
    mut denials: MessageReader<PlacementDenied>,
    sounds: Option<Res<Sounds>>,
    settings: Res<crate::app::settings::GameSettings>,
    muted: Res<Muted>,
) {
    let any = denials.read().next().is_some();
    denials.clear();
    if any && let Some(sounds) = sounds {
        play(&mut commands, &sounds.denied, sfx_gain(&settings, &muted));
    }
}

/// A bright unlock chime (the tier fanfare doing double duty).
pub fn play_chime(commands: &mut Commands, sounds: &Sounds, gain: f32) {
    play(commands, &sounds.tier, gain);
}

/// Keep the playlist rolling: whenever no track entity is alive and the
/// music can be heard, start the next one.
///
/// A track that is merely down - muted, paused, switched off - is still
/// alive and still here, so it is the same song that comes back, from
/// where it left off. Nothing new is started while none of it would be
/// audible, which is what keeps a silenced game from decoding a playlist
/// nobody is listening to.
pub fn rotate_music(
    mut commands: Commands,
    mut playlist: ResMut<MusicPlaylist>,
    settings: Res<crate::app::settings::GameSettings>,
    muted: Res<Muted>,
    menu: Res<crate::app::pause::PauseMenu>,
    playing: Query<(), With<Music>>,
) {
    if !playing.is_empty() || !music_audible(&settings, &muted, menu.open) {
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

/// M mutes the game: the cap that says M, on whatever keyboard this is.
///
/// It used to stop the music alone, which was never what a player reaching
/// for it wants - the beach is noisier than the theme is. Now it is the
/// master mute, and the effects go quiet with it.
///
/// Works while the pause card is up, which the music-only toggle could
/// not: the two no longer touch the same sink, so there is nothing to
/// fight over. Held off only while a name or a chat line is being typed,
/// where the M is a letter the player meant to write.
pub fn toggle_mute(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<crate::app::settings::GameSettings>,
    mut muted: ResMut<Muted>,
) {
    if settings.keycaps.just_pressed(&keys, 'M') {
        muted.0 = !muted.0;
    }
}

/// Hold the theme's sink to whatever [`music_audible`] says this frame.
///
/// The pause card is one of the three voices in that answer, rather than
/// something that reaches in and pauses on its own. It is also the signal
/// rather than [`crate::app::Paused`], because that flag is an offline
/// one: an online pause deliberately leaves the ticker running so the
/// network pump can carry the resume, and the beach there is held still by
/// the lockstep instead. The card is open in both.
pub fn drive_music(
    settings: Res<crate::app::settings::GameSettings>,
    muted: Res<Muted>,
    menu: Res<crate::app::pause::PauseMenu>,
    sinks: Query<&AudioSink, With<Music>>,
) {
    let audible = music_audible(&settings, &muted, menu.open);
    for sink in &sinks {
        // Only when the sink disagrees with the answer: this runs every
        // frame, and the theme is the one sound that is always there.
        if audible && sink.is_paused() {
            sink.play();
        } else if !audible && !sink.is_paused() {
            sink.pause();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::settings::GameSettings;

    /// The three ways to silence the theme, and how they compose. Each one
    /// is a veto, so the card gives back exactly what it took and no more:
    /// a game muted with M before the pause is still muted after it.
    #[test]
    fn every_silence_is_a_veto_and_none_of_them_lifts_another() {
        let settings = |on: bool, volume: u8| GameSettings {
            music_on: on,
            music_volume: volume,
            ..GameSettings::default()
        };
        let audible =
            |on, volume, mute, card| music_audible(&settings(on, volume), &Muted(mute), card);

        assert!(audible(true, 45, false, false), "on, up, unmuted, playing");
        assert!(!audible(false, 45, false, false), "the settings switch");
        assert!(!audible(true, 0, false, false), "the slider at the bottom");
        assert!(!audible(true, 45, true, false), "the master mute");
        assert!(!audible(true, 45, false, true), "the pause card");

        // Muted, then paused, then unpaused: still muted. The card is not
        // a way to get the music back, only a way to lose it for a while.
        assert!(!audible(true, 45, true, true), "both, on the way in");
        assert!(!audible(true, 45, true, false), "and M still holds it");
        // And unmuting under an open card does not play over the pause.
        assert!(!audible(true, 45, false, true), "the card still holds it");
    }

    /// The master mute takes the effects with it, which is the whole point
    /// of it: reaching for M because someone walked in and still hearing
    /// every gull on the beach is the bug it was named for.
    #[test]
    fn the_master_mute_silences_the_effects_too() {
        let settings = GameSettings {
            sfx_on: true,
            sfx_volume: 80,
            ..GameSettings::default()
        };
        assert!(sfx_gain(&settings, &Muted(false)) > 0.0, "heard by default");
        assert_eq!(sfx_gain(&settings, &Muted(true)), 0.0, "M silences them");

        // The switch on the settings card does it on its own, and keeps
        // the volume it was set to for when it comes back on.
        let off = GameSettings {
            sfx_on: false,
            ..settings.clone()
        };
        assert_eq!(sfx_gain(&off, &Muted(false)), 0.0, "switched off");
        assert_eq!(off.sfx_volume, 80, "the slider is where it was left");
    }

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
