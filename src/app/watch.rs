//! Watching a game that was not played here: a replay file handed to
//! `pinch-points watch`, or an arena game as it is played (`arena
//! --watch`), tick by tick from the arena's thread.
//!
//! Both go through the replay player the shelf already uses: a live game
//! is a recording whose inputs keep arriving, and the player simply finds
//! the next one there when it asks.

use crate::app::{Playback, Screen, VersusPhase};
use crate::bots::game::Feed;
use crate::sim::Replay;
use bevy::prelude::*;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;

/// What this run of the game was opened to watch, if anything.
#[derive(Resource, Default)]
pub struct Watch {
    /// A replay file, played from the first frame.
    file: Option<Replay>,
    /// An arena's games as they are played.
    live: Option<Mutex<Receiver<Feed>>>,
    /// The arena's next games, oldest first, held until the one on screen
    /// is over. Ticks go to the newest: that is the game being played.
    next: VecDeque<Replay>,
}

/// Open the game on `replay` and nothing else.
pub fn file(replay: Replay) -> Watch {
    Watch {
        file: Some(replay),
        ..Watch::default()
    }
}

/// Open the game on an arena's feed.
pub fn live(feed: Receiver<Feed>) -> Watch {
    Watch {
        live: Some(Mutex::new(feed)),
        ..Watch::default()
    }
}

/// At startup: a file goes straight on screen.
pub fn begin(
    mut watch: ResMut<Watch>,
    mut playback: ResMut<Playback>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    if let Some(replay) = watch.file.take() {
        playback.0 = Some((replay, 0));
        next_screen.set(Screen::Versus);
    }
}

/// Every frame: take what the arena has played since the last one. A new
/// game waits for the one on screen to finish and its card to be read.
/// One that starts while nothing is on screen (the viewer went to the
/// menu) goes straight on, and the games that were waiting are let go:
/// the viewer walked away from them, and the arena has moved on.
pub fn pump(
    mut watch: ResMut<Watch>,
    mut playback: ResMut<Playback>,
    screen: Res<State<Screen>>,
    vphase: Res<State<VersusPhase>>,
    time: Res<Time>,
    mut over_for: Local<f32>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    let Watch { live, next, .. } = &mut *watch;
    let Some(live) = live else {
        return;
    };
    let rx = live
        .get_mut()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    while let Ok(feed) = rx.try_recv() {
        match feed {
            Feed::Start(replay) => {
                if playback.0.is_none() && *screen.get() != Screen::Versus {
                    next.clear();
                    playback.0 = Some((*replay, 0));
                    next_screen.set(Screen::Versus);
                } else {
                    next.push_back(*replay);
                }
            }
            Feed::Tick(actions) => match (next.back_mut(), playback.0.as_mut()) {
                (Some(queued), _) => queued.record(actions),
                (None, Some((replay, _))) => replay.record(actions),
                (None, None) => {}
            },
        }
    }
    // The card stays up long enough to read, then the next game starts.
    if *screen.get() == Screen::Versus && *vphase.get() == VersusPhase::Over && !next.is_empty() {
        *over_for += time.delta_secs();
        if *over_for > 4.0 {
            *over_for = 0.0;
            next_screen.set(Screen::Versus);
        }
    } else {
        *over_for = 0.0;
    }
}

/// On entering the arena: the next game, if one is waiting, becomes the
/// one on screen. Before anything reads the playback.
pub fn install(mut watch: ResMut<Watch>, mut playback: ResMut<Playback>) {
    if let Some(replay) = watch.next.pop_front() {
        playback.0 = Some((replay, 0));
    }
}

/// A replay file dropped on the window, from the menu or the replay shelf:
/// watched at once. The way in for a player who never opens a terminal,
/// handed the replay of a cup's final or a friend's arena game.
pub fn dropped_replays(
    mut drops: MessageReader<FileDragAndDrop>,
    settings: Res<crate::app::settings::GameSettings>,
    mut playback: ResMut<Playback>,
    mut library: ResMut<crate::app::replays::Library>,
    mut notice: ResMut<crate::app::RoundNotice>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    for drop in drops.read() {
        let FileDragAndDrop::DroppedFile { path_buf, .. } = drop else {
            continue;
        };
        let read = std::fs::read_to_string(path_buf)
            .map_err(|e| e.to_string())
            .and_then(|text| Replay::parse(&text));
        match read {
            Ok(replay) => {
                playback.0 = Some((replay, 0));
                next_screen.set(Screen::Versus);
                return;
            }
            Err(e) => {
                let why = crate::app::i18n::fill(settings.tr().replay_drop_bad, &[("e", &e)]);
                library.feedback.clone_from(&why);
                notice.0 = why;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Level, MAX_PLAYERS, PlayerAction, classic_arena};
    use std::sync::mpsc::{self, Sender};

    fn game(name: &str) -> Box<Replay> {
        let mut replay = Replay::new(Level::from_board("Arena", 3, classic_arena(false, 2)));
        replay.names[0] = name.into();
        Box::new(replay)
    }

    fn arena_window() -> (App, Sender<Feed>) {
        let (tx, rx) = mpsc::channel();
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_state::<VersusPhase>();
        app.insert_resource(Time::<()>::default());
        app.insert_resource(live(rx));
        app.insert_resource(Playback::default());
        app.add_systems(Update, pump);
        app.add_systems(OnEnter(Screen::Versus), install);
        // What leaving the arena does to the playback (`end_versus`).
        app.add_systems(OnExit(Screen::Versus), |mut playback: ResMut<Playback>| {
            playback.0 = None;
        });
        (app, tx)
    }

    fn on_screen(app: &App) -> Option<(String, usize)> {
        app.world()
            .resource::<Playback>()
            .0
            .as_ref()
            .map(|(replay, _)| (replay.names[0].clone(), replay.inputs.len()))
    }

    /// A viewer who leaves for the menu while a game waits its turn, and
    /// comes back on the arena's next game, watches that game: not the one
    /// that waited, and not one made of the two games' ticks together.
    #[test]
    fn a_game_started_after_leaving_is_the_one_watched() {
        let (mut app, feed) = arena_window();
        let tick = [PlayerAction::None; MAX_PLAYERS];
        feed.send(Feed::Start(game("g1"))).expect("feed");
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Versus
        );
        // Game 2 starts while game 1 is still on screen, and waits.
        feed.send(Feed::Start(game("g2"))).expect("feed");
        feed.send(Feed::Tick(tick)).expect("feed");
        app.update();
        assert_eq!(on_screen(&app), Some(("g1".into(), 0)));
        // The viewer leaves for the menu.
        app.world_mut()
            .resource_mut::<NextState<Screen>>()
            .set(Screen::Menu);
        app.update();
        assert_eq!(on_screen(&app), None);
        // Game 3 starts, and is put on.
        feed.send(Feed::Start(game("g3"))).expect("feed");
        for _ in 0..3 {
            feed.send(Feed::Tick(tick)).expect("feed");
        }
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Versus
        );
        assert_eq!(on_screen(&app), Some(("g3".into(), 3)));
        feed.send(Feed::Tick(tick)).expect("feed");
        app.update();
        assert_eq!(on_screen(&app), Some(("g3".into(), 4)));
    }
}
