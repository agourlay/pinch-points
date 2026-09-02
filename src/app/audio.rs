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

#[derive(Resource, Default)]
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

/// Whether a post going down at `owner`'s hand is one to be heard here.
///
/// The seat rule and the whole of its exception. `seated_here` reads
/// correctly wherever this machine answers for a seat: alone in a puzzle
/// it is your seat, at a table it is every human seat sharing the speaker,
/// online it is your seat and nobody else's.
///
/// Where it does not read correctly is where this machine answers for *no*
/// seat, because then it says "nobody" and means it. An online spectator
/// and a replay both spawn no cursor at all (see
/// `cursor::spawn_versus_cursors`: there is no seat to place from), so
/// every post on the beach fell silent while the banks, the gulls and the
/// horn played on - a replay of your own round with all the arrows taken
/// out of it. A machine holding no seat is watching, and a watcher is
/// there to hear the whole beach.
pub(crate) fn post_is_heard(cursors: &Query<&crate::app::cursor::Cursor>, owner: u8) -> bool {
    cursors.is_empty() || crate::app::cursor::seated_here(cursors, owner)
}

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
/// The sim reports per tile and per crab, so one frame can hold a great
/// many of a kind: every seat may act on the same tick, posts placed
/// together wear out together, and a castle can take a whole stream of
/// crabs at once. Sample-aligned copies of one sound do not read as
/// several things happening; they read as one louder, dirtier version of
/// it, and four of them at full gain clip. This is the rule the denial
/// knock has always used, applied where the volume actually is.
///
/// What `used` counts is the caller's to choose, and it is the axis a
/// listener could tell apart: one flag for all the posts, since they knock
/// alike wherever they are, and one per seat for banks, since each seat's
/// crabs arrive at its own castle and pan to its own side of the beach.
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
    cursors: Query<&crate::app::cursor::Cursor>,
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
    // Banks are the busiest thing on the beach, and the one cue the sim
    // raises per crab rather than per act. Twelve six-seat rounds of bots,
    // counted through the differ itself: eight into one castle on a single
    // tick, and two to four of them a couple of hundred times over sixty
    // thousand ticks. Sample-aligned copies at the spatial gain are not
    // eight sounds, they are one loud dirty one and eight sinks to mix it
    // from.
    //
    // Per seat rather than one flag for the lot, because unlike posts these
    // are distinguishable: a seat's crabs walk into that seat's castle, so
    // each flag stands for one place on the beach and six castles filling
    // at once still read as six.
    let mut banked = [false; crate::sim::MAX_PLAYERS];
    let mut goldened = [false; crate::sim::MAX_PLAYERS];
    for event in events.read() {
        match event {
            SimEvent::CrabBanked {
                owner, kind, pos, ..
            } => {
                let seat = usize::from(*owner);
                let at = pan(pos);
                if let Some(used) = banked.get_mut(seat) {
                    once(&mut commands, used, &sounds.bank, gain, at);
                }
                if *kind == crate::sim::CrabKind::Golden
                    && let Some(used) = goldened.get_mut(seat)
                {
                    once(&mut commands, used, &sounds.golden, gain, at);
                }
            }
            SimEvent::CrabEaten { pos } => play_at(&mut commands, &sounds.eat, gain, pan(pos)),
            // No buzz here: a raid happens to *one* seat, and
            // `gamepad::rumble_on_raid` puts it in that seat's own hands.
            // Buzzing every pad in the room told five other players that
            // something had happened to them when nothing had.
            SimEvent::CastleRaided { pos, .. } => {
                play_at(&mut commands, &sounds.raid, gain, pan(pos));
            }
            SimEvent::GullArrived => play(&mut commands, &sounds.screech, gain),
            SimEvent::GullTookOff => play(&mut commands, &sounds.takeoff, gain),
            // Only your own posts knock.
            //
            // The sound was right when a beach held one or two people and
            // wrong the moment it held six: the bots place far more often
            // than the players do, and every one of them was a click. And
            // online it was announcing arrows that went down in somebody
            // else's room.
            //
            // Unless nobody here holds one, which is what watching is.
            // [`post_is_heard`] is the whole rule, and it reads correctly
            // in every mode without being told which one it is in.
            SimEvent::SignpostPlaced { owner, pos } => {
                if post_is_heard(&cursors, *owner) {
                    once(&mut commands, &mut placed, &sounds.place, gain, pan(pos));
                }
            }
            // The other end of the same rule. In versus a post wears out
            // on its own, so leaving this ungated let every bot post knock
            // on the way out and the beach kept its click track.
            SimEvent::SignpostRemoved { owner, pos } => {
                if post_is_heard(&cursors, *owner) {
                    once(&mut commands, &mut removed, &sounds.remove, gain, pan(pos));
                }
            }
            // Its own sample, not the denial knock: a fourth post at the
            // cap is a placement that *worked*, and answering it with the
            // sound of a refusal told the player the opposite of what
            // happened. The placement sounds too, from the new tile.
            SimEvent::SignpostEvicted { owner, pos, .. } => {
                // The third and last end of the same rule. `CapPolicy`
                // defaults to `Evict` in versus, so every bot placement
                // past the cap raises one of these: gating the placement
                // and the removal but not this left five bots knocking
                // almost continuously on a six-seat beach.
                if post_is_heard(&cursors, *owner) {
                    once(&mut commands, &mut evicted, &sounds.evict, gain, pan(pos));
                }
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

/// The puzzle's win stinger. Versus ends on the horn instead, which is why
/// this is gated at all; the gate is the schedule's `in_state`, as it is
/// for every other system in the same phase transition.
pub fn play_win(
    mut commands: Commands,
    sounds: Res<Sounds>,
    settings: Res<crate::app::settings::GameSettings>,
    muted: Res<Muted>,
) {
    play(&mut commands, &sounds.win, sfx_gain(&settings, &muted));
}

/// The puzzle's lose stinger; gated like [`play_win`].
pub fn play_lose(
    mut commands: Commands,
    sounds: Res<Sounds>,
    settings: Res<crate::app::settings::GameSettings>,
    muted: Res<Muted>,
) {
    play(&mut commands, &sounds.lose, sfx_gain(&settings, &muted));
}

/// One dull knock per frame with at least one rejected placement.
pub fn play_denied(
    mut commands: Commands,
    mut denials: MessageReader<PlacementDenied>,
    sounds: Res<Sounds>,
    settings: Res<crate::app::settings::GameSettings>,
    muted: Res<Muted>,
) {
    let any = denials.read().next().is_some();
    denials.clear();
    if any {
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

/// How much faster the theme runs by the time the wave lands.
const SURGE_RAMP: f32 = 0.35;

/// How fast the theme runs with `remaining` ticks left, which is 1.0
/// everywhere but inside the final scramble.
///
/// Pure and apart from the system, because the only other way to read the
/// ramp is off a live `AudioSink`, and a curve nobody can check is a curve
/// that quietly stops matching the thing it is supposed to be racing.
pub(crate) fn surge_ramp(remaining: Option<u64>) -> f32 {
    let window = u64::from(crate::sim::SURGE_TICKS);
    match remaining {
        Some(ticks) if ticks <= window => 1.0 + SURGE_RAMP * (1.0 - ticks as f32 / window as f32),
        _ => 1.0,
    }
}

/// The tide nudge: across the final scramble of a versus round the music
/// speeds up, ramping to [`SURGE_RAMP`] at the wave. Everywhere else it
/// plays straight. Speed-only (pitch rises with it, which is the point).
///
/// The window is [`crate::sim::SURGE_TICKS`] and is not a number of its
/// own here. It was, twice over, and it was the only reader that spelled
/// it out: the gulls double, the water reddens, the HUD turns its clock
/// red and the surge stinger sounds, all off the sim's constant, while the
/// music ramped off a copy. Move the scramble and the copy would have gone
/// on describing the old one, with nothing failing to compile to say so.
pub fn surge_tempo(
    sim: Res<crate::app::Sim>,
    screen: Res<State<Screen>>,
    sinks: Query<&AudioSink, With<Music>>,
) {
    let ramp = match *screen.get() == Screen::Versus {
        true => surge_ramp(sim.0.remaining_ticks()),
        false => 1.0,
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
    caps: Res<crate::app::keycaps::KeyCaps>,
    mut muted: ResMut<Muted>,
) {
    if caps.just_pressed(&keys, 'M') {
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
    use crate::app::cursor::Cursor;
    use crate::app::settings::GameSettings;
    use crate::sim::classic_arena;

    /// A beach with `humans` seats answered for at this machine. Bots and
    /// rivals down a wire are exactly the seats with no cursor, which is
    /// how they are spelled here: seats `humans..` have none.
    fn table(humans: u8) -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::Versus));
        app.insert_resource(Sim(classic_arena(false, 6)));
        app.insert_resource(GameSettings::default());
        app.insert_resource(Muted(false));
        app.insert_resource(Sounds::default());
        app.add_message::<SimEvent>();
        app.add_message::<GamepadRumbleRequest>();
        for seat in 0..humans {
            app.world_mut().spawn(Cursor::seated(seat));
        }
        app.add_systems(Update, play_events);
        app
    }

    /// Everything `events` sets off on one frame, counted, with the stage
    /// swept clear for the next call.
    ///
    /// Every test below is some version of "how many sounds did that make",
    /// and each used to carry its own copy of this: write, update, count,
    /// despawn. One of them is enough, and a test that says only what it is
    /// about is a test that can be read.
    fn heard(app: &mut App, events: impl IntoIterator<Item = SimEvent>) -> usize {
        for event in events {
            app.world_mut().write_message(event);
        }
        app.update();
        let live: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<AudioPlayer>>()
            .iter(app.world())
            .collect();
        let played = live.len();
        for entity in live {
            app.world_mut().entity_mut(entity).despawn();
        }
        played
    }

    fn placed(owner: u8) -> SimEvent {
        SimEvent::SignpostPlaced {
            owner,
            pos: Vec2::ZERO,
        }
    }

    /// A crab of `kind` walking into `owner`'s castle.
    fn banked(owner: u8, kind: crate::sim::CrabKind) -> SimEvent {
        SimEvent::CrabBanked {
            id: 0,
            owner,
            pos: Vec2::ZERO,
            keep: Vec2::ZERO,
            value: 1,
            kind,
        }
    }

    /// The rule the whole beach turns on: a post knocks when the seat that
    /// put it down is one this machine answers for, and stays quiet
    /// otherwise. One human of six is the shape that matters - five bots
    /// placing all round used to be five clicks a second.
    #[test]
    fn only_the_seats_at_this_machine_are_heard() {
        let mut app = table(1);
        assert_eq!(heard(&mut app, [placed(0)]), 1, "your own post knocks");
        for bot in 1..6 {
            let played = heard(&mut app, [placed(bot)]);
            assert_eq!(played, 0, "seat {bot} is not at this table");
        }
    }

    /// Both ends of a post's life follow the same rule. A post wearing
    /// out is the half that used to leak: in versus every one of them
    /// expires on its own, so gating the placement and not the removal
    /// left the click track running at nearly its old rate.
    #[test]
    fn a_post_leaving_is_as_quiet_as_a_post_arriving() {
        let mut app = table(1);
        let gone = |owner| SimEvent::SignpostRemoved {
            owner,
            pos: Vec2::ZERO,
        };
        let played = heard(&mut app, [gone(0)]);
        assert_eq!(played, 1, "your own post going is worth hearing");
        assert_eq!(heard(&mut app, [gone(3)]), 0, "a bot's expiring is not");
    }

    /// And the third end: a post traded away at the cap. Versus defaults
    /// to the evicting rule, so this is the one a six-seat beach raises
    /// most of all - every bot placement past its third post.
    #[test]
    fn a_post_traded_away_is_as_quiet_as_the_other_two() {
        let mut app = table(1);
        let traded = |owner| SimEvent::SignpostEvicted {
            owner,
            pos: Vec2::ZERO,
            dir: crate::sim::Direction::Up,
        };
        let played = heard(&mut app, [traded(0)]);
        assert_eq!(played, 1, "losing your own oldest is worth hearing");
        let played = heard(&mut app, [traded(4)]);
        assert_eq!(played, 0, "a bot trading one of its own is not");
    }

    /// Two people on one couch share a speaker, so both are heard; the
    /// four bots beside them are not.
    #[test]
    fn a_shared_couch_hears_everybody_sitting_at_it() {
        let mut app = table(2);
        assert_eq!(heard(&mut app, [placed(0)]), 1);
        let played = heard(&mut app, [placed(1)]);
        assert_eq!(played, 1, "the seat beside you is in the room");
        assert_eq!(heard(&mut app, [placed(2)]), 0, "the bot is not");
    }

    /// A machine that answers for no seat is watching - an online
    /// spectator, or a replay - and hears the whole beach rather than
    /// none of it. The seat rule read "nobody" there, which took every
    /// post out of a replay of your own round while the banks, the gulls
    /// and the horn played on.
    #[test]
    fn a_watcher_hears_every_seat_because_it_holds_none() {
        let mut app = table(0);
        assert_eq!(heard(&mut app, [placed(0)]), 1, "somebody's post knocks");
        let played = heard(&mut app, [placed(4)]);
        assert_eq!(played, 1, "and so does a seat right across the beach");
    }

    /// Banks are the one cue the sim raises per crab, and a castle can take
    /// a stream of them on one tick: twelve six-seat rounds of bots put
    /// eight into one castle on a single tick. Sample-aligned copies are
    /// not eight sounds, so a seat's castle filling is one sound.
    ///
    /// Per seat, though, and not one for the lot: six castles filling at
    /// once is six things happening in six places, and the beach should say
    /// so. This is the axis the flag is kept on.
    #[test]
    fn a_castle_taking_a_stream_of_crabs_banks_once_for_it() {
        use crate::sim::CrabKind;
        let mut app = table(1);
        let common = |owner| banked(owner, CrabKind::Common);
        assert_eq!(heard(&mut app, [common(0)]), 1, "one crab, one sound");
        let played = heard(&mut app, (0..40).map(|_| common(0)));
        assert_eq!(played, 1, "and forty into the one castle is still one");
        // Unlike a post, a bank is not gated by the seat: a rival banking
        // is news, and hearing where it happened is the point of the pan.
        assert_eq!(heard(&mut app, [common(3)]), 1, "a rival's castle too");
        let played = heard(&mut app, (0..6).map(common));
        assert_eq!(played, 6, "six castles at once are six things");
        let played = heard(&mut app, (0..60).map(|i| common(i % 6)));
        assert_eq!(played, 6, "however many crabs each of them took");
    }

    /// The golden chime rides along with the bank and is held to the same
    /// seat: a castle taking two golden crabs on one tick is one arrival
    /// worth announcing, not two of them stacked on top of each other.
    #[test]
    fn a_golden_arrival_chimes_once_a_castle_too() {
        use crate::sim::CrabKind;
        let mut app = table(1);
        let golden = |owner| banked(owner, CrabKind::Golden);
        let played = heard(&mut app, [golden(0)]);
        assert_eq!(played, 2, "the bank it is, and the chime it is worth");
        let played = heard(&mut app, [golden(0), golden(0)]);
        assert_eq!(played, 2, "two into one castle are still one of each");
        let played = heard(&mut app, [golden(0), golden(1)]);
        assert_eq!(played, 4, "two castles are two of each");
    }

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

    /// The playlist walks, and it only walks when somebody could hear it.
    ///
    /// A track that is merely down - muted, paused, switched off - is
    /// still alive and still the same song, so nothing new starts over the
    /// top of it. And nothing starts at all while none of it would be
    /// audible: a silenced game that kept the playlist rolling would sit
    /// there decoding one theme after another for nobody.
    #[test]
    fn the_playlist_walks_only_while_somebody_could_hear_it() {
        let mut app = App::new();
        app.insert_resource(GameSettings::default());
        app.insert_resource(Muted(false));
        app.insert_resource(crate::app::pause::PauseMenu::default());
        app.insert_resource(MusicPlaylist {
            tracks: vec![Handle::default(), Handle::default(), Handle::default()],
            next: 0,
        });
        app.add_systems(Update, rotate_music);
        let playing = |app: &mut App| {
            app.world_mut()
                .query_filtered::<Entity, With<Music>>()
                .iter(app.world())
                .count()
        };
        let next = |app: &App| app.world().resource::<MusicPlaylist>().next;

        app.update();
        assert_eq!(playing(&mut app), 1, "a track starts");
        assert_eq!(next(&app), 1, "and the playlist moves on");

        // The one that is playing is the one that stays: a second is not
        // stacked on top of it, however many frames go by.
        app.update();
        app.update();
        assert_eq!(playing(&mut app), 1, "still the one song");
        assert_eq!(next(&app), 1, "and the playlist did not move");

        // It ends. The next one takes its place, and the list comes round
        // to the top rather than running off the end of itself.
        let end_it = |app: &mut App| {
            let live: Vec<Entity> = app
                .world_mut()
                .query_filtered::<Entity, With<Music>>()
                .iter(app.world())
                .collect();
            for entity in live {
                app.world_mut().entity_mut(entity).despawn();
            }
        };
        end_it(&mut app);
        app.update();
        assert_eq!(next(&app), 2);
        end_it(&mut app);
        app.update();
        assert_eq!(next(&app), 0, "three tracks and back to the first");

        // Silenced, nothing new is started: not by the master mute, not by
        // the switch, not by the slider, not by the pause card.
        for silence in 0..4 {
            end_it(&mut app);
            let at = next(&app);
            {
                let world = app.world_mut();
                let mut settings = world.resource_mut::<GameSettings>();
                settings.music_on = silence != 1;
                settings.music_volume = if silence == 2 { 0 } else { 45 };
                world.resource_mut::<Muted>().0 = silence == 0;
                world.resource_mut::<crate::app::pause::PauseMenu>().open = silence == 3;
            }
            app.update();
            assert_eq!(playing(&mut app), 0, "silence {silence} starts nothing");
            assert_eq!(next(&app), at, "and does not walk the list either");
        }
    }

    /// The tide nudge, end to end. The theme races the round it is played
    /// under, so the curve has to start where the scramble starts and
    /// arrive at the top exactly when the wave does.
    ///
    /// Read off the sim's own window rather than a number of its own: this
    /// is the check that the two stay married, and it is the one thing a
    /// live `AudioSink` cannot be asked.
    #[test]
    fn the_tide_nudge_ramps_across_the_scramble_and_nowhere_else() {
        let window = u64::from(crate::sim::SURGE_TICKS);
        let top = 1.0 + SURGE_RAMP;
        assert_eq!(surge_ramp(None), 1.0, "an untimed round never hurries");
        assert_eq!(surge_ramp(Some(window * 4)), 1.0, "nor the early minutes");
        assert_eq!(surge_ramp(Some(window + 1)), 1.0, "nor a tick before it");
        assert_eq!(
            surge_ramp(Some(window)),
            1.0,
            "the scramble opens at a walk"
        );
        assert!(
            (surge_ramp(Some(0)) - top).abs() < 1e-6,
            "and the wave lands at the top"
        );
        assert!(
            (surge_ramp(Some(window / 2)) - (1.0 + SURGE_RAMP / 2.0)).abs() < 1e-3,
            "halfway through is halfway up"
        );
        // Never backwards, and never past the top: a round the players are
        // living through should not hear the music hesitate, and a sink
        // asked for a speed outside the ramp is a sink asked for a pitch
        // nobody designed.
        let mut last = 1.0;
        for left in (0..=window).rev() {
            let now = surge_ramp(Some(left));
            assert!(now >= last - 1e-6, "{left} ticks left went backwards");
            assert!(now <= top + 1e-6, "{left} ticks left overshot to {now}");
            last = now;
        }
        assert!((last - top).abs() < 1e-6, "and it finishes at the top");
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
