//! Sound effects, driven by the [`SimEvent`] stream (one observer diffs the
//! sim; this module just maps events to one-shots and rumble). Win/lose
//! stingers hook the puzzle phase transitions; denied placements arrive on
//! their own message from the input layer.

use super::{Muted, sfx_gain};
use crate::app::sim_events::SimEvent;
use crate::app::{PlacementDenied, Screen, Sim};
use bevy::audio::{SpatialScale, Volume};
use bevy::input::gamepad::{GamepadRumbleIntensity, GamepadRumbleRequest};
use bevy::prelude::*;
use std::time::Duration;

#[derive(Resource, Default)]
pub struct Sounds {
    pub(super) place: Handle<AudioSource>,
    pub(super) remove: Handle<AudioSource>,
    pub(super) bank: Handle<AudioSource>,
    pub(super) eat: Handle<AudioSource>,
    pub(super) raid: Handle<AudioSource>,
    pub(super) win: Handle<AudioSource>,
    pub(super) lose: Handle<AudioSource>,
    pub(super) takeoff: Handle<AudioSource>,
    pub(super) screech: Handle<AudioSource>,
    pub(super) event: Handle<AudioSource>,
    pub(super) golden: Handle<AudioSource>,
    pub(super) tier: Handle<AudioSource>,
    pub(super) surge: Handle<AudioSource>,
    pub(super) horn: Handle<AudioSource>,
    pub(super) denied: Handle<AudioSource>,
    pub(super) evict: Handle<AudioSource>,
}

/// How far, in listener units, the edge of the board sits from the centre
/// of the stereo field. Small on purpose: rodio only attenuates by distance
/// beyond one unit, so keeping the whole beach inside that radius leaves
/// panning as the only thing that varies.
const PAN_WIDTH: f32 = 0.5;
/// Distance between the listener's ears, same units.
pub(super) const EAR_GAP: f32 = 0.3;
/// A panned one-shot's two channels sum to less than a centred one's, so
/// positional sounds get a lift to sit level with the stingers.
const SPATIAL_GAIN: f32 = 1.4;

/// Whether a post going down at `owner`'s hand is one to be heard here.
///
/// `seated_here` reads correctly wherever this machine answers for a seat:
/// alone in a puzzle it is your seat, at a table every human seat sharing
/// the speaker, online your seat and nobody else's.
///
/// Where this machine answers for *no* seat it says "nobody" and means it.
/// An online spectator and a replay both spawn no cursor at all (see
/// `cursor::spawn_versus_cursors`), so every post on the beach fell silent
/// while the banks, the gulls and the horn played on. A machine holding no
/// seat is watching, and a watcher hears the whole beach.
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
/// crabs at once. Sample-aligned copies do not read as several things
/// happening, they read as one louder, dirtier version of it, and four at
/// full gain clip.
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

/// What playing a one-shot takes: somewhere to spawn it, the sounds, and
/// how loud the player wants them (see [`sfx_gain`]).
#[derive(bevy::ecs::system::SystemParam)]
pub struct Speaker<'w, 's> {
    commands: Commands<'w, 's>,
    sounds: Res<'w, Sounds>,
    settings: Res<'w, crate::app::settings::GameSettings>,
    muted: Res<'w, Muted>,
}

impl Speaker<'_, '_> {
    /// One of the sounds, centred, at the player's effects gain.
    fn centred(&mut self, pick: impl FnOnce(&Sounds) -> &Handle<AudioSource>) {
        let gain = sfx_gain(&self.settings, &self.muted);
        play(&mut self.commands, pick(&self.sounds), gain);
    }
}

/// The pads plugged in, and the requests that shake them.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Rumble<'w, 's> {
    pads: Query<'w, 's, Entity, With<Gamepad>>,
    requests: MessageWriter<'w, GamepadRumbleRequest>,
}

/// Map each sim event to its one-shot (and rumble where it matters).
pub fn play_events(
    speaker: Speaker,
    mut events: MessageReader<SimEvent>,
    screen: Res<State<Screen>>,
    sim: Res<Sim>,
    cursors: Query<&crate::app::cursor::Cursor>,
    rumble: Rumble,
) {
    let Speaker {
        mut commands,
        sounds,
        settings,
        muted,
    } = speaker;
    let Rumble {
        pads,
        requests: mut rumble,
    } = rumble;
    // The whole table's knocks, felt by whoever holds a pad here, which
    // is nobody while a replay or someone else's match is watched: no
    // cursor, no hands on the beach.
    let hands_here = !cursors.is_empty();
    let mut buzz = |ms: u64, strength: f32| {
        if !settings.rumble || !hands_here {
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
    // raises per crab rather than per act: twelve six-seat rounds of bots
    // put eight into one castle on a single tick.
    //
    // Per seat rather than one flag for the lot, because unlike posts these
    // are distinguishable: a seat's crabs walk into that seat's castle, so
    // six castles filling at once still read as six.
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
            // The wingbeat of a bird leaving, from where it leaves: not the
            // raid's crash, which is the sound of losing something.
            SimEvent::GullShooed { pos, .. } | SimEvent::GullFlewOff { pos } => {
                play_at(&mut commands, &sounds.takeoff, gain, pan(pos));
            }
            // Only your own posts knock: the bots place far more often
            // than the players do, and online this was announcing arrows
            // that went down in somebody else's room. Unless nobody here
            // holds a seat, which is what watching is: see `post_is_heard`.
            SimEvent::SignpostPlaced { owner, pos } => {
                if post_is_heard(&cursors, *owner) {
                    once(&mut commands, &mut placed, &sounds.place, gain, pan(pos));
                }
            }
            // The other end of the same rule. In versus a post wears out
            // on its own, so ungated every bot post knocks on the way out.
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
                // The third end of the same rule. `CapPolicy` defaults to
                // `Evict` in versus, so every bot placement past the cap
                // raises one of these.
                if post_is_heard(&cursors, *owner) {
                    once(&mut commands, &mut evicted, &sounds.evict, gain, pan(pos));
                }
            }
            SimEvent::TierUp { .. } => play(&mut commands, &sounds.tier, gain),
            SimEvent::TideEventFired { .. } => play(&mut commands, &sounds.event, gain),
            SimEvent::LureStarted { .. } => {}
            // The chime a golden crab banks with, from the hole it will
            // come out of: the sound of the prize, before it is anyone's.
            SimEvent::GoldenCalled { pos } => {
                play_at(&mut commands, &sounds.golden, gain, pan(pos));
            }
            // And again as it comes out, with the event sting under it so
            // the arrival is not mistaken for another call.
            SimEvent::GoldenOut { pos } => {
                play_at(&mut commands, &sounds.golden, gain, pan(pos));
                play_at(&mut commands, &sounds.event, gain * 0.6, pan(pos));
            }
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
            SimEvent::CrabSpawned { .. }
            | SimEvent::GullLanded { .. }
            | SimEvent::WheelLocked { .. } => {}
        }
    }
}

/// The puzzle's win stinger. Versus ends on the horn instead, which is why
/// this is gated at all; the gate is the schedule's `in_state`, as it is
/// for every other system in the same phase transition.
pub fn play_win(mut speaker: Speaker) {
    speaker.centred(|sounds| &sounds.win);
}

/// The puzzle's lose stinger; gated like [`play_win`].
pub fn play_lose(mut speaker: Speaker) {
    speaker.centred(|sounds| &sounds.lose);
}

/// One dull knock per frame with at least one rejected placement.
pub fn play_denied(mut speaker: Speaker, mut denials: MessageReader<PlacementDenied>) {
    let any = denials.read().next().is_some();
    denials.clear();
    if any {
        speaker.centred(|sounds| &sounds.denied);
    }
}

/// A bright unlock chime (the tier fanfare doing double duty).
pub fn play_chime(commands: &mut Commands, sounds: &Sounds, gain: f32) {
    play(commands, &sounds.tier, gain);
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
    /// Every test below is some version of "how many sounds did that
    /// make": write, update, count, despawn.
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
            points: 1,
            kind,
            handed: crate::sim::Handedness::Right,
        }
    }

    /// The rule the whole beach turns on: a post knocks when the seat that
    /// put it down is one this machine answers for. One human of six is the
    /// shape that matters: five bots placing was five clicks a second.
    #[test]
    fn only_the_seats_at_this_machine_are_heard() {
        let mut app = table(1);
        assert_eq!(heard(&mut app, [placed(0)]), 1, "your own post knocks");
        for bot in 1..6 {
            let played = heard(&mut app, [placed(bot)]);
            assert_eq!(played, 0, "seat {bot} is not at this table");
        }
    }

    /// Both ends of a post's life follow the same rule. In versus every
    /// post expires on its own, so gating the placement and not the removal
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

    /// A machine that answers for no seat is watching, an online spectator
    /// or a replay, and hears the whole beach rather than none of it: the
    /// seat rule read "nobody" there and took every post out of a replay.
    #[test]
    fn a_watcher_hears_every_seat_because_it_holds_none() {
        let mut app = table(0);
        assert_eq!(heard(&mut app, [placed(0)]), 1, "somebody's post knocks");
        let played = heard(&mut app, [placed(4)]);
        assert_eq!(played, 1, "and so does a seat right across the beach");
    }

    /// Banks are the one cue the sim raises per crab, and a castle can take
    /// a stream of them on one tick, so a seat's castle filling is one
    /// sound. Per seat, though, and not one for the lot: six castles
    /// filling at once is six things happening in six places.
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
