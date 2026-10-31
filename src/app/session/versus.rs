//! A versus round from its first frame to its last: where it comes from
//! and what it is played on, the table's names, the tide coming in and
//! the highlight reel, and the teardown on the way out.

use super::*;
use std::sync::{Arc, OnceLock};

/// Who decides for each seat this round, by this machine's setup: seat by
/// seat, as step 3 of the setup left it.
pub(in crate::app) fn bot_seats(config: &match_setup::MatchConfig) -> Controllers {
    let mut table = Controllers::default();
    if config.armed {
        for seat in 0..config.seats {
            table.0[usize::from(seat)] = config.controller(seat);
        }
    }
    table
}

/// Where a versus round comes from, which decides its board, its table
/// and whether it is recorded. Listed in the order they win: a round
/// picked back up is already a beach mid-play, a recording is watched as
/// it was, an online round is the lobby's agreement, and a match set up
/// here is this machine's own.
pub(super) enum RoundOrigin<'a> {
    /// Resumed from a save or a pasted code: its own beach and table.
    Resumed(Box<suspend::Suspended>),
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
        daily: Option<u32>,
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
            // A configured local match, on the seed its map card was drawn
            // with when it was launched from the setup steps, else a fresh
            // one per round (recorded via the board's seed, so replays are
            // exact). Online keeps the canonical seed so peers agree.
            RoundOrigin::Configured(config) => {
                let seed = daily.map_or_else(
                    || config.seed.unwrap_or_else(clock::fresh_seed),
                    Daily::seed_for,
                );
                match_setup::local_board(config, beaches, seed)
            }
            RoundOrigin::Unconfigured => classic_arena(sandbox, pads.max(2)),
        }
    }

    /// Who decides for each seat, and how many seats there are.
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
    ) -> (Controllers, u8) {
        let controllers = match self {
            RoundOrigin::Resumed(round) => {
                return (Controllers::from_levels(round.bots), round.seats);
            }
            // The AI seats are the terms'; this peer plays its own seat and
            // every other one is somebody's down the wire.
            RoundOrigin::Online(session) => online_controllers(session),
            RoundOrigin::Replay(_) | RoundOrigin::Configured(_) | RoundOrigin::Unconfigured => {
                bot_seats(config)
            }
        };
        (controllers, clamp_seats(self.asked_seats(board, pads)))
    }

    /// How many seats this way into a round asks for, before the clamp.
    pub(super) fn asked_seats(&self, board: &Board, pads: u8) -> u8 {
        match self {
            RoundOrigin::Resumed(round) => round.seats,
            RoundOrigin::Replay(_) => replay_seats(board),
            RoundOrigin::Online(session) => session.seats,
            RoundOrigin::Configured(config) => config.seats,
            RoundOrigin::Unconfigured => pads.max(2),
        }
    }

    /// Whether the round opens on a countdown: one played from its first
    /// tick does, the same rounds that are recorded. A recording starts
    /// at once, and so does a round joined or resumed part way, which is
    /// already moving.
    pub(super) fn counts_in(&self) -> bool {
        self.recorded()
    }

    /// Whether the round opens on the line-up before its count, `set_here`
    /// for a table set up on this machine (not the daily's). Every online
    /// round does: the map was picked for it, the first by the lobby's
    /// page and every one after by the host's between rounds, so every
    /// peer is shown it. So does every round of a table set up here, where
    /// the friends on the couch find their seats, colours and teams before
    /// the count. A round that does not count in opens on neither.
    pub(super) fn opens_on_lineup(&self, set_here: bool) -> bool {
        let lineup = match self {
            RoundOrigin::Online(_) => true,
            RoundOrigin::Configured(_) => set_here,
            RoundOrigin::Resumed(_) | RoundOrigin::Replay(_) | RoundOrigin::Unconfigured => false,
        };
        lineup && self.counts_in()
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

/// Who decides for each seat of an online round, from this peer's side.
///
/// The AI seats are the terms'. This peer plays its own seat, unless it
/// joined as a bot (route 1), and the bots that came straight to this host
/// (route 2) play theirs. A seat a bot drives is a bot's, not this
/// machine's person's: it earns nobody's trophies and is not "(you)".
/// Every other seat is somebody's down the wire.
pub(super) fn online_controllers(session: &net::OnlineSession) -> Controllers {
    let ai = match_setup::bot_seats_from(&session.terms, session.seats);
    let mine = session.session.seat().map(usize::from);
    let driven = |seat: usize| {
        (Some(seat) == mine && session.bot.is_some())
            || session
                .bots_here
                .iter()
                .any(|bot| usize::from(bot.seat) == seat)
    };
    Controllers(std::array::from_fn(|seat| match ai[seat] {
        Some(level) => SeatController::Ai(level),
        None if driven(seat) => SeatController::Bot,
        None if Some(seat) == mine => SeatController::Local,
        None => SeatController::Remote,
    }))
}

/// The seats a recorded round had, read off its board: a replay carries
/// no seat count, and the castles are the record of who played. A
/// handmade beach's spare castles were put away before it was recorded
/// ([`Level::table_board`]), so they count only the seats the
/// round had. Watching a replay and filing a pasted one both ask this.
pub(in crate::app) fn replay_seats(board: &Board) -> u8 {
    clamp_seats(board.seats_in_play())
}

/// A seat count the per-seat arrays can hold. Two of the sources are not
/// the game's to trust: an online count is whatever number a host wrote
/// into a datagram, and a dev hook's is however many gamepads are plugged
/// in. `Seats` indexes the per-seat arrays, and a seventh seat ran off the
/// end of the scores in `leading_seats`.
pub(in crate::app) fn clamp_seats(asked: u8) -> u8 {
    asked.clamp(2, MAX_PLAYERS as u8)
}

/// Where a versus round's board comes from: six answers, and the pads
/// plugged in, which seat a round nobody set up. Read by the cursor spawn
/// too, one system earlier, since the same answer says how many cursors
/// the round is played with.
#[derive(bevy::ecs::system::SystemParam)]
pub(in crate::app) struct RoundSource<'w, 's> {
    pub(in crate::app) pads: Query<'w, 's, &'static Gamepad>,
    pub(in crate::app) sandbox: Res<'w, Sandbox>,
    pub(in crate::app) online: Res<'w, net::Online>,
    pub(in crate::app) playback: Res<'w, Playback>,
    pub(in crate::app) config: Res<'w, match_setup::MatchConfig>,
    pub(in crate::app) beaches: Res<'w, match_setup::CustomBeaches>,
    pub(in crate::app) daily: ResMut<'w, Daily>,
    pub(in crate::app) resuming: ResMut<'w, Resuming>,
    /// Whether the round is one set up here, noted for a rematch, and
    /// the series it belongs to.
    pub(in crate::app) table: ResMut<'w, rematch::LocalTable>,
    pub(in crate::app) tournament: Res<'w, tournament::Tournament>,
    /// This machine's own say in how a round set up here is scored.
    pub(in crate::app) settings: Res<'w, crate::app::settings::GameSettings>,
}

/// Boot the versus arena: fresh board, sprites, running phase. Online
/// sessions and replay playback use the same mode with a different board
/// source and input path.
pub(in crate::app) fn load_versus(
    mut stage: BoardStage,
    mut source: RoundSource,
    mut controllers: ResMut<Controllers>,
    mut seats: ResMut<Seats>,
    mut recorder: ResMut<Recorder>,
    play: Play,
    mut opening: countdown::Opening,
) {
    let Play {
        mut sim,
        mut pending,
        mut paused,
    } = play;
    let RoundSource {
        pads,
        sandbox,
        online,
        playback,
        config,
        beaches,
        daily,
        resuming,
        table,
        tournament,
        settings,
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
    // The daily's day is read once, here, and kept: the seed is built
    // from it and the trophies count the round under it when it ends.
    let daily_day =
        (daily.active && matches!(origin, RoundOrigin::Configured(_))).then(Daily::today);
    if let Some(day) = daily_day {
        daily.day = day;
    }
    // A table set up here can be played again; the daily's cannot.
    let set_here = matches!(origin, RoundOrigin::Configured(_)) && daily_day.is_none();
    table.note_round(set_here, config, tournament);
    sim.0 = origin.board(daily_day, beaches, sandbox.0, pad_count);
    (*controllers, seats.0) = origin.table(config, &sim.0, pad_count);
    // Every per-seat array is indexed by this, and every table needs two.
    debug_assert!(
        (2..=MAX_PLAYERS as u8).contains(&seats.0),
        "a table of {} seats",
        seats.0
    );
    // Who plays for whom, on the board, where the AI and Gull Attack read
    // it: a round set up here is scored as [`teams::local`] says. Every
    // other board brought its own: the host's terms put it there on every
    // peer (`match_setup::board_from`), and a snapshot or a recording
    // carries it.
    if matches!(
        origin,
        RoundOrigin::Configured(_) | RoundOrigin::Unconfigured
    ) {
        sim.0.rules_mut().teams = teams::local(settings, daily).played_by(seats.0).sides();
    }
    recorder.0 = origin
        .recorded()
        .then(|| Replay::new(Level::from_board("Versus", 3, sim.0.clone())));
    stage.lay_out(&sim.0);
    pending.0 = [PlayerAction::None; MAX_PLAYERS];
    for (mut cur, mut transform) in &mut stage.cursors {
        (cur.x, cur.y) = cursor_home(&sim.0, cur.player);
        transform.translation = layout::tile_center(&sim.0, cur.x, cur.y).extend(layout::z::CURSOR);
    }
    paused.0 = false;
    opening.open(origin.counts_in(), origin.opens_on_lineup(set_here));
}

/// Forget what was asked of a round that was never entered.
///
/// A pasted code ([`Resuming`]), a replay to watch ([`Playback`]) and the
/// daily are each set beside a request for the arena, and read by
/// [`load_versus`] when it is entered. When another screen was entered
/// instead (the request overwritten that frame, by the menu or a key the
/// same frame), they stayed behind, and the next way into the arena of
/// any kind took them up: an online launch played the pasted board
/// (a desync at the first hash), and a daily was credited on a stale day.
/// So any screen but the arena entered clears them. Runs in the
/// transition itself, after every `OnEnter`, so a code pasted on the
/// frame a screen is reached is the next frame's, and kept.
pub(in crate::app) fn forget_a_round_not_entered(
    mut transitions: MessageReader<StateTransitionEvent<Screen>>,
    mut resuming: ResMut<Resuming>,
    mut playback: ResMut<Playback>,
    mut daily: ResMut<Daily>,
    mut again: rematch::PlayAgain,
) {
    let Some(entered) = transitions.read().last().map(|t| t.entered) else {
        return;
    };
    if entered != Some(Screen::Versus) {
        resuming.0 = None;
        playback.0 = None;
        daily.active = false;
        again.let_go();
    }
}

/// What a round leaves behind: its recording, the highlight reel being
/// cut from it, and the offer to keep it on the replay shelf.
#[derive(bevy::ecs::system::SystemParam)]
pub(in crate::app) struct RoundLog<'w> {
    recorder: ResMut<'w, Recorder>,
    reel_thread: ResMut<'w, ReelThread>,
    keep: ResMut<'w, replays::RoundToKeep>,
}

/// Tear down per-round versus resources when leaving the mode.
pub(in crate::app) fn end_versus(
    screen: Res<State<Screen>>,
    mut online: ResMut<net::Online>,
    mut playback: ResMut<Playback>,
    log: RoundLog,
    mut pending: ResMut<PendingActions>,
    mut controllers: ResMut<Controllers>,
    again: rematch::PlayAgain,
) {
    let RoundLog {
        mut recorder,
        mut reel_thread,
        mut keep,
    } = log;
    let rematch::PlayAgain {
        table,
        mut config,
        mut next_vphase,
        beaches: _,
    } = again;
    // A session armed for another round outlives the screen: the interlude
    // is a doorway between rounds, not the end of the match. Everything
    // else is torn down and rebuilt as it is between local rounds.
    if !online.0.as_ref().is_some_and(|session| session.next_round) {
        online.0 = None;
    }
    playback.0 = None;
    recorder.0 = None;
    // The results card is left: a round not kept from it is let go
    // (`replays::keep`), unless the way on is the interlude before the
    // next round of a series, which holds the offer until that round
    // starts. Read off the screen entered, which the state already is
    // as this one is left.
    if *screen.get() != Screen::Interlude {
        *keep = replays::RoundToKeep::Nothing;
    }
    // A reel still being written finishes on disk; its news is for a card
    // that is gone.
    reel_thread.0 = None;
    // Disarmed, unless the same table is being played again: the arena's
    // build reads the match it is armed with as it comes back in.
    config.armed = table.keeps_the_table();
    *controllers = Controllers::default();
    // Never leak a queued action or a stale Over phase into the next round
    // or another mode. The phase goes back to where a round is entered.
    pending.0 = [PlayerAction::None; MAX_PLAYERS];
    next_vphase.set(VersusPhase::Countdown);
}

/// Resolve what each seat is called this round; chained right after
/// `load_versus`. Online, the handshake's table is the only truth, since
/// the local couch names must never label a rival; offline they are just
/// what the player typed for their table, each seat named after the person
/// partner picks put there ([`match_setup::MatchConfig::seat_names`]),
/// except at the daily's own table.
pub(in crate::app) fn resolve_seat_names(
    scoring: teams::Scoring,
    config: Res<match_setup::MatchConfig>,
    controllers: Res<Controllers>,
    seats: Res<Seats>,
    mut recorder: ResMut<Recorder>,
    mut names: ResMut<SeatNames>,
    mut kinds: ResMut<SeatKinds>,
) {
    let teams::Scoring {
        local,
        online,
        playback,
        sim: _,
    } = &scoring;
    let settings = &local.settings;
    let couch;
    let given = match (&playback.0, &online.0) {
        // A replay is watched, not played: the names belong to the round on
        // screen, not to whoever is sitting here now.
        (Some((replay, _)), _) => &replay.names,
        (None, Some(session)) => &session.names,
        (None, None) if local.daily.active => &settings.names,
        (None, None) => {
            couch = config.seat_names(&settings.names);
            &couch
        }
    };
    // Spelled as the recording spells them ([`crate::sim::seat_name`]), so
    // the card, the file the round is kept under and the shelf reading that
    // file back name a seat alike by construction: one spelling in one
    // place, rather than a promise that every way a name arrives (typed,
    // read from the settings file, sent over the wire, read off a replay)
    // was tidied the same way before it got here.
    names.0 = given.each_ref().map(|name| crate::sim::seat_name(name));
    // And stamp them onto the round being recorded, created one system
    // earlier before anybody knew who was playing. Now rather than at save
    // time, by when an online round's names have gone with its session.
    // What held each seat, by the same rule: the recording's own, else
    // the table this round is played at.
    *kinds = match (&playback.0, &online.0) {
        (Some((replay, _)), _) => SeatKinds(replay.kinds),
        // The table's word, which knows a rival's bot from a rival.
        (None, Some(session)) => SeatKinds(session.kinds),
        (None, None) => SeatKinds::of(&controllers),
    };
    if let Some(replay) = &mut recorder.0 {
        replay.names = names.0.clone();
        replay.kinds = kinds.0;
        // The level each AI seat plays at, from whoever decides for it
        // here: the couch's dials, the online terms' level, the daily's
        // fierce AI. Every peer of an online round computes the AI, so
        // every peer's recording says the same.
        replay.ai = controllers.0.map(SeatController::ai);
        // And how it is scored, so the round is crowned the same when it
        // is watched again, whatever the watcher's own setting by then.
        replay.teams = scoring.mode(seats.0).index() as u8;
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

/// Who to file a finished round under, as the results card names them
/// ([`teams::winner_label`]), or `None` for a draw (see
/// `replays::file_name`).
pub(super) fn winner_name(
    sim: &Sim,
    seats: &Seats,
    mode: teams::TeamMode,
    names: &SeatNames,
    tr: &i18n::Tr,
) -> Option<String> {
    teams::winner_label(sim.0.scores(), seats.0, mode, |seat| names.label(tr, seat))
}

pub(in crate::app) fn check_versus_over(
    sim: Res<Sim>,
    seating: seating::Seating,
    scoring: teams::Scoring,
    log: RoundLog,
    mut highlight: ResMut<Highlight>,
    mut next_vphase: ResMut<NextState<VersusPhase>>,
) {
    let seating::Seating {
        seats,
        names: seat_names,
        ..
    } = seating;
    let settings = &scoring.local.settings;
    let RoundLog {
        mut recorder,
        mut reel_thread,
        mut keep,
    } = log;
    if sim.0.round_over() {
        // Spec §7.7: a finished round is a shareable replay.
        highlight.0 = None;
        reel_thread.0 = None;
        if let Some(mut replay) = recorder.0.take() {
            // The board the results card shows, so the shelf can say what
            // the round was won with without playing it through.
            replay.scores = Some(*sim.0.scores());
            let text = replay.to_text();
            // `last.txt` is the newest round, kept or not, for the
            // `PINCH_REPLAY` dev hook.
            let last = replay_path();
            match paths::write_atomic(&last, &text) {
                Ok(()) => info!("replay saved to {}", last.display()),
                Err(e) => warn!("could not save replay: {e}"),
            }
            // The shelf holds only the rounds a player keeps: the results
            // card offers this one, filed under when it ended and who
            // took it, if the key is pressed before the card is left.
            let winner = winner_name(
                &sim,
                &seats,
                scoring.mode(seats.0),
                &seat_names,
                settings.tr(),
            );
            *keep = replays::RoundToKeep::Offered(Box::new(replays::Unkept {
                text,
                stamp: clock::now_secs(),
                winner,
            }));
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

/// Walk a joiner straight on to the next round when the host's call for it
/// lands before this board has finished.
///
/// A peer a frame or two behind the host is still playing the last frames
/// of the round when the host, on its results card, calls the next one,
/// and a late watcher can be handed the next round as it stands the same
/// way. The session is the new round's from then on (`take_up`), so the
/// old board has nothing left to play: the frames it was missing belong to
/// a lockstep that is gone, and the pump stops ticking it. Left on the
/// beach it stood there for good, the tide never came in, and the results
/// card that would have followed the call never appeared. The round it was
/// in is over for everyone else already, so the doorway is the way on.
///
/// A board that did finish shows its results card first, as ever:
/// `versus_over_input` follows the call from there.
///
/// The round it leaves never reaches its results card here, and with it
/// the trophies' count of it: `record_called_round` counts it instead,
/// on the host's word for who won ([`walked_on`] is the one question both
/// ask).
pub(in crate::app) fn follow_the_called_round(
    sim: Res<Sim>,
    online: Res<net::Online>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    if walked_on(&sim.0, &online) {
        next_screen.set(Screen::Interlude);
    }
}

/// Whether this joiner is being walked on to the next round before its
/// own board has finished: see [`follow_the_called_round`].
pub(in crate::app) fn walked_on(board: &Board, online: &net::Online) -> bool {
    let called = online
        .0
        .as_ref()
        .is_some_and(|session| session.next_round && !session.is_host());
    called && !board.round_over()
}

/// Host, as the tide comes in: write down whom the results card crowns,
/// for the call for the next round to carry (`Invitation::crowned`). A
/// joiner walked on to that round before its own board had finished never
/// sees the card, and learns from this whether it won the round it played.
pub(in crate::app) fn note_the_crowned(
    sim: Res<Sim>,
    seats: Res<Seats>,
    local: teams::LocalScoring,
    mut online: ResMut<net::Online>,
) {
    // The pure rule rather than `Scoring`, which reads the session this
    // writes; never a replay's, which crowns nobody's next call.
    let mode = teams::in_play(local.mode(), &online, None, seats.0);
    let Some(session) = online.0.as_mut().filter(|session| session.is_host()) else {
        return;
    };
    let winners = seating::leading_seats(sim.0.scores(), seats.0, mode);
    session.crowned = crate::transport::crowned_bits(&winners);
}
