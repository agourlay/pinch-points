//! The music: two sets of tracks, calm and battle, shuffled with rests
//! between them, faded across when the screen changes sets, and held
//! down by whichever of the three silences is on (see [`music_audible`]).

use super::Muted;
use crate::app::Screen;
use bevy::audio::Volume;
use bevy::prelude::*;

/// The background theme entity, and which set it was drawn from.
#[derive(Component)]
pub struct Music(Mood);

/// Which of the two sets of tracks a screen plays.
///
/// Players loved the dance tracks and wanted them where the game is fast:
/// a versus round gets the drums, and everywhere a player sits and thinks
/// (the menus, the puzzles, the editor) keeps the calmer tracks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    Calm,
    Battle,
}

impl Mood {
    /// The set this screen plays. The interlude between the rounds of a
    /// series is still the battle, so a track is not cut off by it.
    pub fn of(screen: Screen) -> Mood {
        match screen {
            Screen::Versus | Screen::Interlude => Mood::Battle,
            Screen::Menu
            | Screen::Puzzle
            | Screen::Editor
            | Screen::Lobby
            | Screen::Settings
            | Screen::Controls
            | Screen::MatchSetup
            | Screen::Achievements
            | Screen::StageSelect
            | Screen::Replays
            | Screen::Language
            | Screen::NewVersion => Mood::Calm,
        }
    }

    /// How long the beach goes quiet between two tracks, seconds.
    ///
    /// Calm rests are longer than most of the tracks themselves, on
    /// purpose: the rest is what lets a short playlist last a session
    /// without wearing thin. A battle only takes a breath: a round is a
    /// few minutes long, and a minute of it in silence is a round without
    /// its music.
    fn rest_secs(self) -> std::ops::Range<f32> {
        match self {
            Mood::Calm => 30.0..90.0,
            Mood::Battle => 3.0..6.0,
        }
    }
}

/// A track on its way out because the screen changed sets: it fades over
/// [`FADE_SECS`] under the new one and then goes. Seconds left.
#[derive(Component)]
pub struct FadingOut(f32);

/// How long a track takes to fade out when the set changes under it.
const FADE_SECS: f32 = 0.8;

/// The three voices [`music_audible`] hears.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Loudness<'w> {
    settings: Res<'w, crate::app::settings::GameSettings>,
    muted: Res<'w, Muted>,
    menu: Res<'w, crate::app::pause::PauseMenu>,
}

/// Whether the theme should be audible this frame. The pause card is the
/// third voice here, and the one that is not a player preference: it holds
/// the music down for as long as it is up and gives back exactly that.
///
/// Derived from all three every frame rather than pushed at the sink, so
/// nothing has to remember whether a silence is its to lift.
fn music_audible(
    settings: &crate::app::settings::GameSettings,
    muted: &Muted,
    card_open: bool,
) -> bool {
    settings.music_gain() > 0.0 && !muted.0 && !card_open
}

/// The background playlist; a track spawns, plays once, despawns, and
/// after a rest `rotate_music` starts another from the set the screen asks
/// for ([`Mood`]).
///
/// Players called the music repetitive, and it was: seven short loops,
/// two and a half minutes in all, back to back in a fixed order that
/// opened every launch on the same track. So each set is a shuffle bag
/// (every track once before any comes round again, never the same one
/// twice running, a different one first each launch), between tracks the
/// beach goes quiet for a while, and longer tracks joined them.
#[derive(Resource)]
pub struct MusicPlaylist {
    calm: Shelf,
    battle: Shelf,
    rest: Rest,
    rng: crate::app::effects::VisualRng,
}

/// One set's tracks and its shuffle bag.
struct Shelf {
    tracks: Vec<Handle<AudioSource>>,
    /// What is left to play this time round, drawn from the back.
    bag: Vec<usize>,
    /// The track drawn last, which the next refill must not open with.
    last: Option<usize>,
}

/// Where the playlist is between two tracks, and for which set.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Rest {
    /// Nothing has played yet: the first track starts without a wait.
    Due,
    /// A track is alive, playing or held down.
    Playing(Mood),
    /// Seconds of quiet left before the next track.
    Waiting(Mood, f32),
}

impl Shelf {
    fn new(tracks: Vec<Handle<AudioSource>>) -> Self {
        Shelf {
            tracks,
            bag: Vec::new(),
            last: None,
        }
    }

    /// The next track to play, from a bag refilled and shuffled whenever it
    /// runs dry.
    fn draw(&mut self, rng: &mut crate::app::effects::VisualRng) -> usize {
        if self.bag.is_empty() {
            self.bag = (0..self.tracks.len()).collect();
            for i in (1..self.bag.len()).rev() {
                // The high bits: an LCG's low bits cycle with short periods.
                let j = (rng.next() >> 16) as usize % (i + 1);
                self.bag.swap(i, j);
            }
            // The last of one round and the first of the next would
            // otherwise be the same song twice, one rest apart.
            if self.bag.len() > 1 && self.bag.last() == self.last.as_ref() {
                let end = self.bag.len() - 1;
                self.bag.swap(0, end);
            }
        }
        let track = self.bag.pop().unwrap_or(0);
        self.last = Some(track);
        track
    }
}

impl MusicPlaylist {
    pub(super) fn new(
        calm: Vec<Handle<AudioSource>>,
        battle: Vec<Handle<AudioSource>>,
        rng: crate::app::effects::VisualRng,
    ) -> Self {
        MusicPlaylist {
            calm: Shelf::new(calm),
            battle: Shelf::new(battle),
            rest: Rest::Due,
            rng,
        }
    }

    /// The next track from `mood`'s set.
    fn draw(&mut self, mood: Mood) -> Handle<AudioSource> {
        let MusicPlaylist {
            calm, battle, rng, ..
        } = self;
        let shelf = match mood {
            Mood::Calm => calm,
            Mood::Battle => battle,
        };
        let index = shelf.draw(rng);
        shelf.tracks[index].clone()
    }
}

/// Which set of music this moment wants, if any.
///
/// None once a versus round is over: the results card is where the win
/// fanfare plays and the awards are read out, and a dance track carrying
/// on under them buried both. The next round of a series brings it back.
fn wanted(screen: Screen, phase: crate::app::VersusPhase) -> Option<Mood> {
    match (screen, phase) {
        (Screen::Versus, crate::app::VersusPhase::Over) => None,
        (screen, _) => Some(Mood::of(screen)),
    }
}

/// Keep the playlist rolling: when a track ends, rest, then start the next
/// from the set this screen plays.
///
/// A track that is merely down - muted, paused, switched off - is still
/// alive and still here, so it is the same song that comes back, from
/// where it left off. Nothing new is started while none of it would be
/// audible, which is what keeps a silenced game from decoding a playlist
/// nobody is listening to, and the rest only counts down while the music
/// could be heard: a pause card does not eat it.
///
/// When the screen changes sets, the track from the other one fades out
/// and one from this set starts at once, rest or no rest: a round opening
/// on a lull is a round opening without its music.
///
/// When a versus round is over the playing track fades and nothing takes
/// its place (see [`wanted`]).
pub fn rotate_music(
    mut commands: Commands,
    mut playlist: ResMut<MusicPlaylist>,
    loudness: Loudness,
    screen: Res<State<Screen>>,
    phase: Res<State<crate::app::VersusPhase>>,
    time: Res<Time>,
    playing: Query<(Entity, &Music), Without<FadingOut>>,
) {
    let Loudness {
        settings,
        muted,
        menu,
    } = loudness;
    let mood = wanted(*screen.get(), *phase.get());
    let mut live = false;
    for (entity, music) in &playing {
        if Some(music.0) == mood {
            live = true;
        } else {
            commands.entity(entity).try_insert(FadingOut(FADE_SECS));
        }
    }
    let Some(mood) = mood else {
        // Quiet until something wants music again, which then starts at
        // once rather than after a rest: the next round of a series should
        // open on its music, as the first one did.
        playlist.rest = Rest::Due;
        return;
    };
    if live {
        playlist.rest = Rest::Playing(mood);
        return;
    }
    if !music_audible(&settings, &muted, menu.stops_the_round()) {
        return;
    }
    match playlist.rest {
        Rest::Playing(was) if was == mood => {
            let rest = mood.rest_secs();
            let secs = playlist.rng.range(rest.start, rest.end);
            playlist.rest = Rest::Waiting(mood, secs);
            return;
        }
        Rest::Waiting(was, left) if was == mood => {
            let left = left - time.delta_secs();
            if left > 0.0 {
                playlist.rest = Rest::Waiting(mood, left);
                return;
            }
        }
        // Nothing yet, or the other set was playing or resting.
        Rest::Due | Rest::Playing(_) | Rest::Waiting(..) => {}
    }
    let track = playlist.draw(mood);
    playlist.rest = Rest::Playing(mood);
    commands.spawn((
        Music(mood),
        AudioPlayer::new(track),
        PlaybackSettings {
            volume: Volume::Linear(settings.music_gain()),
            ..PlaybackSettings::DESPAWN
        },
    ));
}

/// Take a track that is leaving down to nothing, then away.
///
/// Counted without a sink too: a track faded before its audio had started
/// has no sink to turn down, and must still go.
pub fn fade_music(
    mut commands: Commands,
    settings: Res<crate::app::settings::GameSettings>,
    time: Res<Time>,
    mut fading: Query<(Entity, &mut FadingOut, Option<&mut AudioSink>)>,
) {
    for (entity, mut fade, sink) in &mut fading {
        fade.0 -= time.delta_secs();
        if fade.0 <= 0.0 {
            commands.entity(entity).try_despawn();
        } else if let Some(mut sink) = sink {
            sink.set_volume(Volume::Linear(settings.music_gain() * fade.0 / FADE_SECS));
        }
    }
}

/// M mutes the game: the cap that says M, on whatever keyboard this is.
///
/// The master mute: the effects go quiet with the music, since the beach
/// is noisier than the theme is. Works while the pause card is up, the two
/// no longer touching the same sink, and is held off only while a name or
/// a chat line is being typed, where M is a letter.
pub fn toggle_mute(
    keys: Res<ButtonInput<KeyCode>>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    live: Res<crate::app::keycaps::LiveSeatKeys>,
    mut muted: ResMut<Muted>,
) {
    if caps.global_pressed(&keys, 'M', &live) {
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
pub fn drive_music(loudness: Loudness, sinks: Query<&AudioSink, With<Music>>) {
    let Loudness {
        settings,
        muted,
        menu,
    } = loudness;
    let audible = music_audible(&settings, &muted, menu.stops_the_round());
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
    use crate::app::effects::VisualRng;
    use crate::app::settings::GameSettings;
    use std::time::Duration;

    /// Between the rounds of a series the music plays on: only the results
    /// card is quiet, and the interlude that follows it is the battle set,
    /// whatever phase the round it came from was left in.
    #[test]
    fn the_series_interlude_keeps_its_music() {
        use crate::app::VersusPhase;
        for phase in [VersusPhase::Running, VersusPhase::Over] {
            assert_eq!(
                wanted(Screen::Interlude, phase),
                Some(Mood::Battle),
                "{phase:?}"
            );
        }
    }

    /// The results card is quiet: once a versus round is over, the track
    /// that played it out fades and no other starts, while every other
    /// moment, the round itself included, still wants its set.
    #[test]
    fn the_music_stops_when_a_versus_round_is_over() {
        use crate::app::VersusPhase;
        assert_eq!(wanted(Screen::Versus, VersusPhase::Over), None);
        assert_eq!(
            wanted(Screen::Versus, VersusPhase::Running),
            Some(Mood::Battle)
        );
        // A phase left over from the last round means nothing off the beach.
        assert_eq!(wanted(Screen::Menu, VersusPhase::Over), Some(Mood::Calm));

        let mut app = App::new();
        app.insert_resource(MusicPlaylist::new(
            vec![Handle::default()],
            vec![Handle::default()],
            VisualRng::default(),
        ));
        app.insert_resource(GameSettings::default());
        app.init_resource::<Muted>();
        app.init_resource::<crate::app::pause::PauseMenu>();
        app.insert_resource(State::new(Screen::Versus));
        app.insert_resource(State::new(VersusPhase::Over));
        app.init_resource::<Time>();
        app.add_systems(Update, rotate_music);
        let track = app.world_mut().spawn(Music(Mood::Battle)).id();
        app.update();
        assert!(
            app.world().get::<FadingOut>(track).is_some(),
            "the round's track fades out"
        );
        let mut music = app.world_mut().query::<&Music>();
        assert_eq!(
            music.iter(app.world()).count(),
            1,
            "and nothing starts in its place"
        );
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

    /// Every track once before any comes round again, and never the same
    /// one twice running, across as many rounds of the bag as it takes.
    #[test]
    fn the_shuffle_plays_everything_and_never_repeats_back_to_back() {
        let tracks = vec![Handle::default(); 7];
        for seed in 0..50 {
            let mut rng = VisualRng::seeded(seed);
            let mut shelf = Shelf::new(tracks.clone());
            let mut previous = None;
            for _ in 0..20 {
                let mut round: Vec<usize> = (0..7).map(|_| shelf.draw(&mut rng)).collect();
                for &track in &round {
                    assert_ne!(Some(track), previous, "seed {seed}: {track} twice running");
                    previous = Some(track);
                }
                round.sort_unstable();
                assert_eq!(
                    round,
                    (0..7).collect::<Vec<_>>(),
                    "seed {seed}: one of each"
                );
            }
        }
        // And launches do not all open on the same track.
        let openers: std::collections::HashSet<usize> = (0..50)
            .map(|seed| Shelf::new(tracks.clone()).draw(&mut VisualRng::seeded(seed)))
            .collect();
        assert!(openers.len() > 1, "every launch opened on {openers:?}");
    }

    /// The versus screens play the battle set, and every other one the
    /// calm set.
    #[test]
    fn only_a_round_plays_the_battle_set() {
        for screen in Screen::ALL {
            let battle = matches!(screen, Screen::Versus | Screen::Interlude);
            let want = if battle { Mood::Battle } else { Mood::Calm };
            assert_eq!(Mood::of(screen), want, "{screen:?}");
        }
    }

    /// A world with the playlist's systems: two calm tracks, two battle
    /// tracks, on the menu, with the clock under the test's hand.
    fn radio() -> App {
        let mut app = App::new();
        app.insert_resource(GameSettings::default());
        app.insert_resource(Muted(false));
        app.insert_resource(crate::app::pause::PauseMenu::default());
        app.insert_resource(State::new(Screen::Menu));
        app.insert_resource(State::new(crate::app::VersusPhase::Running));
        app.insert_resource(Time::<()>::default());
        app.insert_resource(MusicPlaylist::new(
            vec![Handle::default(), Handle::default()],
            vec![Handle::default(), Handle::default()],
            VisualRng::seeded(7),
        ));
        app.add_systems(Update, (rotate_music, fade_music).chain());
        app
    }

    /// The tracks alive, by set, with whether each is on its way out.
    fn on_air(app: &mut App) -> Vec<(Mood, bool)> {
        app.world_mut()
            .query::<(&Music, Has<FadingOut>)>()
            .iter(app.world())
            .map(|(music, fading)| (music.0, fading))
            .collect()
    }

    fn wait(app: &mut App, secs: f32) {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(secs));
        app.update();
    }

    fn end_it(app: &mut App) {
        let live: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<Music>>()
            .iter(app.world())
            .collect();
        for entity in live {
            app.world_mut().entity_mut(entity).despawn();
        }
    }

    /// The playlist rests between tracks, and it only moves when somebody
    /// could hear it.
    ///
    /// A track that is merely down (muted, paused, switched off) is still
    /// alive and still the same song, so nothing new starts over the top of
    /// it, and nothing starts at all while none of it would be audible.
    #[test]
    fn the_playlist_rests_between_tracks_and_only_while_somebody_could_hear_it() {
        let mut app = radio();
        let rest = Mood::Calm.rest_secs();
        let playing = |app: &mut App| on_air(app).len();

        app.update();
        assert_eq!(playing(&mut app), 1, "the first track starts at once");

        // The one that is playing is the one that stays: a second is not
        // stacked on top of it, however many frames go by.
        app.update();
        wait(&mut app, 200.0);
        assert_eq!(playing(&mut app), 1, "still the one song");

        // It ends, and the beach goes quiet for the whole of the shortest
        // rest before anything else starts.
        end_it(&mut app);
        app.update();
        wait(&mut app, rest.start - 1.0);
        assert_eq!(playing(&mut app), 0, "resting");

        // Silenced, the rest holds where it is, however long it lasts:
        // not by the master mute, the switch, the slider, the pause card.
        for silence in 0..4 {
            {
                let world = app.world_mut();
                let mut settings = world.resource_mut::<GameSettings>();
                settings.music_on = silence != 1;
                settings.music_volume = if silence == 2 { 0 } else { 45 };
                world.resource_mut::<Muted>().0 = silence == 0;
                *world.resource_mut::<crate::app::pause::PauseMenu>() = match silence {
                    3 => crate::app::pause::PauseMenu::raised(),
                    _ => crate::app::pause::PauseMenu::default(),
                };
            }
            wait(&mut app, 500.0);
            assert_eq!(playing(&mut app), 0, "silence {silence} starts nothing");
        }
        *app.world_mut().resource_mut::<GameSettings>() = GameSettings::default();
        *app.world_mut()
            .resource_mut::<crate::app::pause::PauseMenu>() =
            crate::app::pause::PauseMenu::default();
        wait(&mut app, 0.5);
        assert_eq!(playing(&mut app), 0, "the rest was held, not spent");

        // Past the longest rest, the next track is on.
        wait(&mut app, rest.end);
        assert_eq!(playing(&mut app), 1, "and then the next one");
    }

    /// Walking into a round swaps the calm track for a battle one at once,
    /// the old one fading under it, and walking out swaps them back, even
    /// in the middle of a rest.
    #[test]
    fn a_round_changes_the_set_without_waiting_for_a_rest() {
        let mut app = radio();
        app.update();
        assert_eq!(on_air(&mut app), [(Mood::Calm, false)], "the menu is calm");

        app.insert_resource(State::new(Screen::Versus));
        wait(&mut app, 0.1);
        app.update();
        let mut air = on_air(&mut app);
        air.sort_by_key(|&(mood, _)| mood == Mood::Battle);
        assert_eq!(
            air,
            [(Mood::Calm, true), (Mood::Battle, false)],
            "the calm one fades under the battle one"
        );
        wait(&mut app, FADE_SECS);
        assert_eq!(on_air(&mut app), [(Mood::Battle, false)], "and is gone");

        // A battle track ends and the round takes only a breath.
        end_it(&mut app);
        app.update();
        wait(&mut app, Mood::Battle.rest_secs().end);
        assert_eq!(on_air(&mut app), [(Mood::Battle, false)], "the next one");

        // Out of the round mid-rest: the calm set does not wait out the
        // battle's breath, nor start one of its own.
        end_it(&mut app);
        app.update();
        app.insert_resource(State::new(Screen::Menu));
        wait(&mut app, 0.1);
        assert_eq!(on_air(&mut app), [(Mood::Calm, false)], "straight back");
    }
}
