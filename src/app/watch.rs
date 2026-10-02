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
use std::sync::Mutex;
use std::sync::mpsc::Receiver;

/// What this run of the game was opened to watch, if anything.
#[derive(Resource, Default)]
pub struct Watch {
    /// A replay file, played from the first frame.
    file: Option<Replay>,
    /// An arena's games as they are played.
    live: Option<Mutex<Receiver<Feed>>>,
    /// The arena's next game, held until the one on screen is over.
    next: Option<Replay>,
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
                    playback.0 = Some((*replay, 0));
                    next_screen.set(Screen::Versus);
                } else {
                    *next = Some(*replay);
                }
            }
            Feed::Tick(actions) => match (next.as_mut(), playback.0.as_mut()) {
                (Some(queued), _) => queued.record(actions),
                (None, Some((replay, _))) => replay.record(actions),
                (None, None) => {}
            },
        }
    }
    // The card stays up long enough to read, then the next game starts.
    if *screen.get() == Screen::Versus && *vphase.get() == VersusPhase::Over && next.is_some() {
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
    if let Some(replay) = watch.next.take() {
        playback.0 = Some((replay, 0));
    }
}
