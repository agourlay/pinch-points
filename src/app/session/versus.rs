//! A versus round from its first frame to its last: where it comes from
//! and what it is played on, the table's names, the tide coming in and
//! the highlight reel, and the teardown on the way out.

use super::*;
use std::sync::{Arc, OnceLock};

/// Which seats the AI holds this round, and at what level. The AI always
/// takes the top seats, so the humans keep P1 downward.
pub(in crate::app) fn bot_seats(
    config: &match_setup::MatchConfig,
) -> [Option<BotLevel>; MAX_PLAYERS] {
    let mut bots = [None; MAX_PLAYERS];
    if config.armed {
        for seat in (config.seats - config.bots)..config.seats {
            bots[seat as usize] = Some(config.bot_levels[seat as usize]);
        }
    }
    bots
}

/// Where a versus round comes from, which decides its board, its table
/// and whether it is recorded. Listed in the order they win: a round
/// picked back up is already a beach mid-play, a recording is watched as
/// it was, an online round is the lobby's agreement, and a match set up
/// here is this machine's own.
pub(super) enum RoundOrigin<'a> {
    /// Resumed from a save or a pasted code: its own beach and table.
    Resumed(Box<crate::app::suspend::Suspended>),
    /// A recording being watched.
    Replay(&'a Replay),
    /// A round at an online table, from its first frame or caught up
    /// with mid-round.
    Online(&'a net::OnlineSession),
    /// A match set up on this machine, or the daily's own table.
    Configured(&'a match_setup::MatchConfig),
    /// Straight in from a dev hook: two keyboard seats plus pads.
    Unconfigured,
}

impl RoundOrigin<'_> {
    /// The beach the round is played on.
    pub(super) fn board(
        &self,
        daily: bool,
        beaches: &match_setup::CustomBeaches,
        sandbox: bool,
        pads: u8,
    ) -> Board {
        match self {
            RoundOrigin::Resumed(round) => round.board.clone(),
            RoundOrigin::Replay(replay) => replay.level.board(),
            RoundOrigin::Online(session) => match &session.catch_up.board {
                // A watcher who arrived mid-round starts where the host's
                // board was when it was sent (`net::catch_up`).
                Some(board) => board.clone(),
                // Every peer builds from the terms the lobby agreed (map,
                // gull pressure, round length and seed) so the beach is
                // identical without anyone trusting their own menu. A
                // handmade beach cannot be described that way, so it
                // travelled whole and is used as it arrived.
                None => match_setup::board_from(&session.terms, session.seats, &session.beach),
            },
            // A configured local match: handcrafted classic or a generated
            // arena at the chosen size, with gull pressure and round length
            // overrides. Fresh seed per round (recorded via the board's
            // seed, so replays are exact).
            RoundOrigin::Configured(config) => {
                let seed = if daily {
                    Daily::seed()
                } else {
                    clock::fresh_seed()
                };
                let (w, h) = config.map.size();
                let mut board = if config.map == match_setup::MapChoice::Custom {
                    // A beach somebody built. Locally there is nobody to
                    // send it to, so it is read off the shelf.
                    beaches
                        .fitting(config.seats)
                        .get(config.custom)
                        .map_or_else(
                            || generate_arena(seed, config.seats, w, h),
                            |beach| beach.level.board(),
                        )
                } else if config.map == match_setup::MapChoice::Classic {
                    // Same handcrafted layout, fresh random stream: one
                    // seed's luck (gull entry points, spawn kinds) should
                    // not colour every round. Online keeps the canonical
                    // seed so peers agree.
                    classic_arena_seeded(seed, false, config.seats)
                } else {
                    generate_arena(seed, config.seats, w, h)
                };
                board.set_gull_period(config.gulls.period());
                board.set_round_length(Some(config.round.ticks()));
                board
            }
            RoundOrigin::Unconfigured => classic_arena(sandbox, pads.max(2)),
        }
    }

    /// Which seats an AI holds, and how many seats there are.
    ///
    /// Online, the AI seats are part of the agreed terms; a resumed round
    /// brings its own table; everywhere else they come from this machine's
    /// setup screen, which leaves a replay or a dev hook botting nobody
    /// unless a match was set up. `board` is the one [`Self::board`] built.
    pub(super) fn table(
        &self,
        config: &match_setup::MatchConfig,
        board: &Board,
        pads: u8,
    ) -> ([Option<BotLevel>; MAX_PLAYERS], u8) {
        let bots = match self {
            RoundOrigin::Resumed(round) => return (round.bots, round.seats),
            RoundOrigin::Online(session) => {
                match_setup::bot_seats_from(&session.terms, session.seats)
            }
            RoundOrigin::Replay(_) | RoundOrigin::Configured(_) | RoundOrigin::Unconfigured => {
                bot_seats(config)
            }
        };
        (bots, clamp_seats(self.asked_seats(board, pads)))
    }

    /// How many seats this way into a round asks for, before the clamp.
    pub(super) fn asked_seats(&self, board: &Board, pads: u8) -> u8 {
        match self {
            RoundOrigin::Resumed(round) => round.seats,
            // A replay carries no seat count; the castles on the recorded
            // board are the record of who played.
            RoundOrigin::Replay(_) => board.castle_owners().max().unwrap_or(1).saturating_add(1),
            RoundOrigin::Online(session) => session.seats,
            RoundOrigin::Configured(config) => config.seats,
            RoundOrigin::Unconfigured => pads.max(2),
        }
    }

    /// Whether the round is recorded as it is played. A replay is a
    /// recording already, and a round picked back up or caught up with
    /// mid-round has no first tick to record from: the board arrives as it
    /// stood, not the inputs that got it there. Every reader of the
    /// recorder allows for there being none.
    pub(super) fn recorded(&self) -> bool {
        match self {
            RoundOrigin::Resumed(_) | RoundOrigin::Replay(_) => false,
            RoundOrigin::Online(session) => session.catch_up.board.is_none(),
            RoundOrigin::Configured(_) | RoundOrigin::Unconfigured => true,
        }
    }
}

/// A seat count the per-seat arrays can hold. Two of the sources are not
/// the game's to trust: an online count is whatever number a host wrote
/// into a datagram, and a dev hook's is however many gamepads are plugged
/// in. `Seats` indexes the per-seat arrays, and a seventh seat ran off the
/// end of the scores in `leading_seats`.
pub(in crate::app) fn clamp_seats(asked: u8) -> u8 {
    asked.clamp(2, MAX_PLAYERS as u8)
}

/// Where a versus round's board comes from. Bundled because there are six
/// answers and a Bevy system takes sixteen parameters in total.
#[derive(bevy::ecs::system::SystemParam)]
pub(in crate::app) struct RoundSource<'w> {
    sandbox: Res<'w, Sandbox>,
    online: Res<'w, net::Online>,
    playback: Res<'w, Playback>,
    config: Res<'w, match_setup::MatchConfig>,
    beaches: Res<'w, match_setup::CustomBeaches>,
    daily: Res<'w, Daily>,
    resuming: ResMut<'w, Resuming>,
}

/// Boot the versus arena: fresh board, sprites, running phase. Online
/// sessions and replay playback use the same mode with a different board
/// source and input path.
#[allow(clippy::too_many_arguments)]
pub(in crate::app) fn load_versus(
    mut commands: Commands,
    art: Res<art::Art>,
    mut source: RoundSource,
    mut bots: ResMut<Bots>,
    pads: Query<&Gamepad>,
    mut seats: ResMut<Seats>,
    mut recorder: ResMut<Recorder>,
    mut sim: ResMut<Sim>,
    mut next_vphase: ResMut<NextState<VersusPhase>>,
    mut paused: ResMut<Paused>,
    mut pending: ResMut<PendingActions>,
    mut cursors: Query<(&mut cursor::Cursor, &mut Transform)>,
) {
    let RoundSource {
        sandbox,
        online,
        playback,
        config,
        beaches,
        daily,
        resuming,
    } = &mut source;
    // The daily plays on a table of its own rather than the player's.
    let daily_config = match_setup::MatchConfig::daily();
    let config: &match_setup::MatchConfig = if daily.active { &daily_config } else { config };
    let pad_count = pads.iter().count() as u8;
    let origin = if let Some(round) = resuming.0.take() {
        RoundOrigin::Resumed(Box::new(round))
    } else if let Some((replay, _)) = &playback.0 {
        RoundOrigin::Replay(replay)
    } else if let Some(session) = &online.0 {
        RoundOrigin::Online(session)
    } else if config.armed {
        RoundOrigin::Configured(config)
    } else {
        RoundOrigin::Unconfigured
    };
    sim.0 = origin.board(daily.active, beaches, sandbox.0, pad_count);
    (bots.0, seats.0) = origin.table(config, &sim.0, pad_count);
    // Every per-seat array is indexed by this, and every table needs two.
    debug_assert!(
        (2..=MAX_PLAYERS as u8).contains(&seats.0),
        "a table of {} seats",
        seats.0
    );
    recorder.0 = origin
        .recorded()
        .then(|| Replay::new(Level::from_board("Turf War", 3, sim.0.clone())));
    board_render::spawn_static_board(&mut commands, &sim.0, &art);
    board_render::spawn_waterline(&mut commands);
    board_render::spawn_water_foam(&mut commands, &art);
    pending.0 = [PlayerAction::None; MAX_PLAYERS];
    for (mut cur, mut transform) in &mut cursors {
        (cur.x, cur.y) = cursor_home(&sim.0, cur.player);
        transform.translation = layout::tile_center(&sim.0, cur.x, cur.y).extend(layout::z::CURSOR);
    }
    paused.0 = false;
    next_vphase.set(VersusPhase::Running);
}

/// Tear down per-round versus resources when leaving the mode.
#[allow(clippy::too_many_arguments)]
pub(in crate::app) fn end_versus(
    mut online: ResMut<net::Online>,
    mut playback: ResMut<Playback>,
    mut recorder: ResMut<Recorder>,
    mut reel_thread: ResMut<ReelThread>,
    mut pending: ResMut<PendingActions>,
    mut config: ResMut<match_setup::MatchConfig>,
    mut bots: ResMut<Bots>,
    mut next_vphase: ResMut<NextState<VersusPhase>>,
) {
    // A session armed for another round outlives the screen: the interlude
    // is a doorway between rounds, not the end of the match. Everything
    // else is torn down and rebuilt as it is between local rounds.
    if !online.0.as_ref().is_some_and(|session| session.next_round) {
        online.0 = None;
    }
    playback.0 = None;
    recorder.0 = None;
    // A reel still being written finishes on disk; its news is for a card
    // that is gone.
    reel_thread.0 = None;
    config.armed = false;
    bots.0 = [None; MAX_PLAYERS];
    // Never leak a queued action or a stale Over phase into the next round
    // or another mode.
    pending.0 = [PlayerAction::None; MAX_PLAYERS];
    next_vphase.set(VersusPhase::Running);
}

/// Resolve what each seat is called this round; chained right after
/// `load_versus`. Online, the handshake's table is the only truth, since
/// the local couch names must never label a rival; offline they are just
/// what the player typed for their table.
pub(in crate::app) fn resolve_seat_names(
    online: Res<net::Online>,
    playback: Res<Playback>,
    settings: Res<settings::GameSettings>,
    mut recorder: ResMut<Recorder>,
    mut names: ResMut<SeatNames>,
) {
    names.0 = match (&playback.0, &online.0) {
        // A replay is watched, not played: the names belong to the round on
        // screen, not to whoever is sitting here now.
        (Some((replay, _)), _) => replay.names.clone(),
        (None, Some(session)) => session.names.clone(),
        (None, None) => settings.names.clone(),
    };
    // And stamp them onto the round being recorded, created one system
    // earlier before anybody knew who was playing. Now rather than at save
    // time, by when an online round's names have gone with its session.
    if let Some(replay) = &mut recorder.0 {
        replay.names = names.0.clone();
    }
}

/// The highlight reel being written off-thread for the round that just
/// ended: its answer is where the GIF landed, or nothing when there was
/// no reel to write. Emptied once read, and when the arena is left.
#[derive(Resource, Default)]
pub struct ReelThread(Option<Arc<OnceLock<Option<String>>>>);

/// Carry the reel thread's answer over to [`Highlight`] once it is in, so
/// the results card claims a saved reel only after the save happened.
pub(in crate::app) fn poll_reel(
    mut reel_thread: ResMut<ReelThread>,
    mut highlight: ResMut<Highlight>,
) {
    let Some(answer) = reel_thread.0.as_ref().and_then(|slot| slot.get().cloned()) else {
        return;
    };
    reel_thread.0 = None;
    if answer.is_some() {
        highlight.0 = answer;
    }
}

/// Who to file a finished round under: the leading seat's name, or nobody.
pub(super) fn winner_name(
    sim: &Sim,
    seats: &Seats,
    settings: &settings::GameSettings,
    online: &net::Online,
    names: &SeatNames,
) -> String {
    let mode = teams::in_play(settings, online, seats.0);
    let leaders = side_panels::leading_seats(sim.0.scores(), seats.0, mode);
    match leaders.iter().position(|&led| led) {
        Some(seat) => names.label(settings.tr(), seat as u8),
        None => "draw".to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(in crate::app) fn check_versus_over(
    sim: Res<Sim>,
    seats: Res<Seats>,
    settings: Res<settings::GameSettings>,
    online: Res<net::Online>,
    seat_names: Res<SeatNames>,
    mut recorder: ResMut<Recorder>,
    mut highlight: ResMut<Highlight>,
    mut reel_thread: ResMut<ReelThread>,
    mut next_vphase: ResMut<NextState<VersusPhase>>,
) {
    if sim.0.round_over() {
        // Spec §7.7: a finished round is a shareable replay.
        highlight.0 = None;
        reel_thread.0 = None;
        if let Some(replay) = recorder.0.take() {
            let library = replays::library_dir();
            let text = replay.to_text();
            // `last.txt` is still the newest round, for the menu's Replay
            // entry and the dev hook; the library keeps every round beside
            // it under a name that says when it was and who took it.
            let last = replay_path();
            match paths::write_atomic(&last, &text) {
                Ok(()) => info!("replay saved to {}", last.display()),
                Err(e) => warn!("could not save replay: {e}"),
            }
            let stamp = clock::now_secs();
            let winner = winner_name(&sim, &seats, &settings, &online, &seat_names);
            let kept = library.join(replays::file_name(stamp, &winner));
            if let Err(e) = paths::write_atomic(&kept, &text) {
                warn!("could not file the replay: {e}");
            }
            // Trimmed here, the one moment the shelf can have grown.
            replays::prune(settings.replay_cap);
            // The reel re-simulates the whole round twice and encodes 150
            // frames, so it goes on its own thread: the results card should
            // appear the instant the tide comes in, not after the GIF. The
            // thread answers with the path once the GIF is written, and
            // nothing if the round was too short or the write failed.
            let reel = highlight_path();
            let answer = Arc::new(OnceLock::new());
            reel_thread.0 = Some(Arc::clone(&answer));
            std::thread::spawn(move || {
                let saved = match crate::highlight::reel(&replay) {
                    Some(bytes) => match paths::write_atomic(&reel, bytes) {
                        Ok(()) => {
                            info!("highlight reel saved to {}", reel.display());
                            Some(reel.display().to_string())
                        }
                        Err(e) => {
                            warn!("could not save the highlight reel: {e}");
                            None
                        }
                    },
                    None => {
                        warn!("the round was too short for a highlight reel");
                        None
                    }
                };
                let _ = answer.set(saved);
            });
        }
        next_vphase.set(VersusPhase::Over);
    }
}
