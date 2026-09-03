//! The versus/puzzle session driver: board boot and teardown, the level
//! reload path, the fixed-tick sim driver, and the round-outcome checks.
//! Split from `mod.rs` so the module keeps only types and wiring.

use super::*;
use std::sync::{Arc, OnceLock};

pub(super) fn send_first_load(mut load: MessageWriter<LoadLevel>) {
    load.write(LoadLevel { keep_posts: false });
}

/// Which seats the AI holds this round, and at what level. The AI always
/// takes the top seats, so the humans keep P1 downward.
pub(super) fn bot_seats(config: &match_setup::MatchConfig) -> [Option<BotLevel>; MAX_PLAYERS] {
    let mut bots = [None; MAX_PLAYERS];
    if config.armed {
        for seat in (config.seats - config.bots)..config.seats {
            bots[seat as usize] = Some(config.bot_levels[seat as usize]);
        }
    }
    bots
}

/// How many seats this round has, from whichever source knows.
///
/// Four ways into a versus round and they each answer differently: a replay
/// only knows what its recorded board shows, an online session agreed its
/// count at the lobby, a configured match was told, and a dev hook falls
/// back to the keyboard's two seats plus whatever pads are plugged in.
pub(super) fn seat_count(
    config: &match_setup::MatchConfig,
    playback: bool,
    online: Option<u8>,
    top_castle_owner: Option<u8>,
    pads: u8,
) -> u8 {
    let asked = if playback {
        // A replay carries no seat count; the castles on the recorded board
        // are the record of who played.
        top_castle_owner.unwrap_or(1).saturating_add(1)
    } else if let Some(seats) = online {
        seats
    } else if config.armed {
        config.seats
    } else {
        pads.max(2)
    };
    // Clamped on the way out, because two of these sources are not the
    // game's to trust: `online` is whatever number a host wrote into a
    // datagram, and `pads` is however many gamepads happen to be plugged
    // in. `Seats` indexes the per-seat arrays, which are `MAX_PLAYERS`
    // long, and a seventh seat ran off the end of the scores in
    // `leading_seats` and took the round down with it.
    let seats = asked.clamp(2, MAX_PLAYERS as u8);
    debug_assert!(
        (2..=MAX_PLAYERS as u8).contains(&seats),
        "the clamp let {seats} through"
    );
    seats
}

/// Where a versus round's board comes from. Bundled because there are six
/// answers and a Bevy system takes sixteen parameters in total.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct RoundSource<'w> {
    sandbox: Res<'w, Sandbox>,
    online: Res<'w, net::Online>,
    playback: Res<'w, Playback>,
    config: Res<'w, match_setup::MatchConfig>,
    beaches: Res<'w, match_setup::CustomBeaches>,
    daily: Res<'w, Daily>,
    resuming: ResMut<'w, crate::app::Resuming>,
}

/// Boot the versus arena: fresh board, sprites, running phase. Online
/// sessions and replay playback use the same mode with a different board
/// source and input path.
#[allow(clippy::too_many_arguments)]
pub(super) fn load_versus(
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
    // A round picked back up beats every other source: it is already a
    // beach, mid-play, and nothing here should build it a fresh one.
    let resumed = resuming.0.take();
    sim.0 = if let Some(round) = &resumed {
        round.board.clone()
    } else if let Some((replay, _)) = &playback.0 {
        replay.level.board()
    } else if let Some(session) = &online.0 {
        // Every peer builds from the terms the lobby agreed (map, gull
        // pressure, round length and seed) so the beach is identical
        // without anyone trusting their own menu. A handmade beach cannot
        // be described that way, so it travelled whole and is used as it
        // arrived.
        match_setup::board_from(&session.terms, session.seats, &session.beach)
    } else if config.armed {
        // A configured local match: handcrafted classic or a generated
        // arena at the chosen size, with gull pressure and round length
        // overrides. Fresh seed per round (recorded via the board's seed,
        // so replays are exact).
        let seed = if daily.active {
            Daily::seed()
        } else {
            crate::app::clock::fresh_seed()
        };
        let (w, h) = config.map.size();
        let mut board = if config.map == match_setup::MapChoice::Custom {
            // A beach somebody built. Locally there is nobody to send it
            // to, so it is read off the shelf; the dial offers it either
            // way, and a match that quietly generated a random arena
            // instead would be the dial lying.
            beaches
                .fitting(config.seats)
                .get(config.custom)
                .map_or_else(
                    || generate_arena(seed, config.seats, w, h),
                    |beach| beach.level.board(),
                )
        } else if config.map == match_setup::MapChoice::Classic {
            // Same handcrafted layout, fresh random stream: one seed's luck
            // (gull entry points, spawn kinds) should not colour every
            // round. Online keeps the canonical seed so peers agree.
            classic_arena_seeded(seed, false, config.seats)
        } else {
            generate_arena(seed, config.seats, w, h)
        };
        board.set_gull_period(config.gulls.period());
        board.set_round_length(Some(config.round.ticks()));
        board
    } else {
        // Unconfigured entry (dev hooks): two keyboard seats plus pads.
        classic_arena(sandbox.0, (pads.iter().count() as u8).max(2))
    };
    // Online, the AI seats are part of the agreed terms; locally they come
    // from this machine's setup screen, except for a resumed round, which
    // brings its own table with it.
    (bots.0, seats.0) = match (&resumed, &online.0) {
        (Some(round), _) => (round.bots, round.seats),
        (None, session) => {
            let seated = session.as_ref().map(|s| s.seats);
            let bots = match session {
                Some(s) => match_setup::bot_seats_from(&s.terms, s.seats),
                None => bot_seats(config),
            };
            (
                bots,
                seat_count(
                    config,
                    playback.0.is_some(),
                    seated,
                    sim.0.castle_owners().max(),
                    pads.iter().count() as u8,
                ),
            )
        }
    };
    // A replay is the round from its first tick, and a round started from
    // a pasted code has no first tick to hand: the code carries the board
    // as it stood when it was copied, not the inputs that got it there. A
    // recording that opened on the mid-round board would replay as a
    // different round from the one that was played, so a pasted round is
    // not recorded at all. Nothing downstream minds: every reader of the
    // recorder is already an `if let`.
    recorder.0 = if playback.0.is_none() && resumed.is_none() {
        Some(Replay::new(Level::from_board("Turf War", 3, sim.0.clone())))
    } else {
        None
    };
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

/// Leaving a puzzle mid-run leaves `Phase` where it was, and the next
/// puzzle entered would run its first frame under it: `sim_should_run`
/// says yes to `Running`, and the stale board ticks once before the load
/// swaps it. `end_versus` puts `VersusPhase` back for the same reason.
pub(super) fn reset_puzzle_phase(mut next_phase: ResMut<NextState<Phase>>) {
    next_phase.set(Phase::Setup);
}

/// Where a seat's cursor starts a round: fanned out near its own castle,
/// two tiles in from the corner so the first press is not into a wall.
///
/// Clamped to the board, not to the two-tile inset: a custom arena can be
/// as small as the level format allows, and asking `clamp` for an inset
/// the board cannot hold was how a 3x3 beach off the shelf, or pasted as
/// a round code, took the game down on its first frame. On a board too
/// small for the inset the cursor sits as far in as there is.
fn cursor_home(board: &crate::sim::Board, player: u8) -> (u8, u8) {
    let (w, h) = (board.width(), board.height());
    let spots = castle_spots(w, h);
    let (cx, cy) = spots[usize::from(player).min(spots.len() - 1)];
    let inset = |len: u8| 2.min(len.saturating_sub(1) / 2);
    (
        cx.clamp(inset(w), w - 1 - inset(w)),
        cy.clamp(inset(h), h - 1 - inset(h)),
    )
}

/// Every category of entity rendered from board state, bundled so the two
/// teardown paths (screen exit and level reload) can never drift apart
/// again: a missed category here once left turnstile sprites probing a
/// smaller board and panicking in `tile_at`.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct BoardSprites<'w, 's> {
    statics: Query<'w, 's, Entity, With<board_render::BoardStatic>>,
    posts: Query<'w, 's, Entity, With<board_render::SignpostSprite>>,
    castles: Query<'w, 's, Entity, With<board_render::CastleSprite>>,
    water: Query<'w, 's, Entity, With<board_render::Waterline>>,
    foam: Query<'w, 's, Entity, With<board_render::WaterFoam>>,
    logs: Query<'w, 's, Entity, With<board_render::TurnstileSprite>>,
    crabs: Query<'w, 's, Entity, With<creatures::CrabSprite>>,
    gulls: Query<'w, 's, Entity, With<creatures::GullSprite>>,
}

impl BoardSprites<'_, '_> {
    fn despawn_all(&self, commands: &mut Commands) {
        for entity in self
            .statics
            .iter()
            .chain(self.posts.iter())
            .chain(self.castles.iter())
            .chain(self.water.iter())
            .chain(self.foam.iter())
            .chain(self.logs.iter())
            .chain(self.crabs.iter())
            .chain(self.gulls.iter())
        {
            commands.entity(entity).despawn();
        }
    }
}

/// Despawn everything rendered from board state, in every category.
pub(super) fn despawn_board_sprites(mut commands: Commands, sprites: BoardSprites) {
    sprites.despawn_all(&mut commands);
}

/// Swap the sim to the campaign's current level and rebuild everything that
/// renders from board identity (statics, signposts, crabs, cursor bounds).
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_load_level(
    mut commands: Commands,
    mut messages: MessageReader<LoadLevel>,
    campaign: Res<Campaign>,
    art: Res<art::Art>,
    mut pending: ResMut<PendingActions>,
    mut sim: ResMut<Sim>,
    mut next_phase: ResMut<NextState<Phase>>,
    mut paused: ResMut<Paused>,
    sprites: BoardSprites,
    mut cursors: Query<(&mut cursor::Cursor, &mut Transform)>,
) {
    let Some(message) = messages.read().last() else {
        return;
    };
    let mut board = campaign.current().board();
    if message.keep_posts {
        let old = &sim.0;
        for y in 0..old.height().min(board.height()) {
            for x in 0..old.width().min(board.width()) {
                if let Some(sp) = old.signpost_at(x, y) {
                    board.place_signpost(sp.owner, x, y, sp.dir);
                }
            }
        }
    }
    sim.0 = board;
    pending.0 = [PlayerAction::None; MAX_PLAYERS];

    sprites.despawn_all(&mut commands);
    board_render::spawn_static_board(&mut commands, &sim.0, &art);
    // Timed levels show the tide; update_waterline hides the bars when the
    // board has no round timer, so this is free for normal puzzles.
    board_render::spawn_waterline(&mut commands);
    board_render::spawn_water_foam(&mut commands, &art);
    if let Ok((mut cur, mut transform)) = cursors.single_mut() {
        cur.x = sim.0.width() / 2;
        cur.y = sim.0.height() / 2;
        transform.translation = layout::tile_center(&sim.0, cur.x, cur.y).extend(layout::z::CURSOR);
    }
    paused.0 = false;
    next_phase.set(Phase::Setup);
}

/// Tear down per-round versus resources when leaving the mode.
#[allow(clippy::too_many_arguments)]
pub(super) fn end_versus(
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

/// Fill in the AI seats' moves for a frame.
///
/// Online this runs on every peer over the board they have just agreed on,
/// which is why AI seats are free: no bandwidth, and no chance of
/// divergence, because it is the same state through the same code. A human
/// seat's action is never overwritten, since theirs arrived over the wire.
pub(crate) fn fill_bot_actions(
    board: &Board,
    bots: &Bots,
    actions: &mut [PlayerAction; MAX_PLAYERS],
) {
    debug_assert_eq!(actions.len(), bots.0.len(), "a seat with no bot slot");
    for (seat, action) in actions.iter_mut().enumerate() {
        if let Some(level) = bots.0[seat]
            && matches!(action, PlayerAction::None)
        {
            *action = bot_action(board, seat as u8, level);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn advance_sim(
    mut sim: ResMut<Sim>,
    mut pending: ResMut<PendingActions>,
    paused: Res<Paused>,
    mut online: ResMut<net::Online>,
    mut recorder: ResMut<Recorder>,
    mut playback: ResMut<Playback>,
    speed: Res<crate::app::replays::PlaybackSpeed>,
    bots: Res<Bots>,
) {
    if paused.0 {
        return;
    }
    // Watching a replay: feed the recorded inputs, then stop. At 2x or 4x
    // the tick feeds that many frames, which is scrubbing without a
    // separate code path - the sim is the same either way.
    if let Some((replay, idx)) = &mut playback.0 {
        for _ in 0..speed.0.max(1) {
            let Some(actions) = replay.inputs.get(*idx).copied() else {
                break;
            };
            *idx += 1;
            sim.0.tick(&actions);
        }
        return;
    }
    // Online: local input goes through the lockstep session; frames simulate
    // only when every player's input is known.
    if let Some(session) = &mut online.0 {
        // A spectator has no seat: it commits nothing and simply simulates
        // the frames the players agree on.
        let local = session.session.seat().map(usize::from);
        let action = local.map_or(PlayerAction::None, |seat| pending.0[seat]);
        let sim = &mut sim.0;
        let recorder = &mut recorder.0;
        let bots = &*bots;
        let committed = session.pump(action, |net| {
            if let Some(mut frame_actions) = net.session.advance() {
                // The lockstep carries only the humans; the AI seats are
                // derived from the frame every peer has just agreed on.
                fill_bot_actions(sim, bots, &mut frame_actions);
                sim.tick(&frame_actions);
                if let Some(replay) = recorder {
                    replay.record(frame_actions);
                }
                net.after_frame(sim.state_hash());
            }
        });
        if let Some(seat) = local
            && (committed || session.session.paused())
        {
            // Only a committed action leaves the queue; at the commit lead
            // (a stalled peer) the press is retried next tick, not dropped.
            // Presses made *during* a pause are dropped instead of queued:
            // nobody expects the game to act on them when it unfreezes.
            pending.0[seat] = PlayerAction::None;
        }
        return;
    }
    let mut actions = std::mem::take(&mut pending.0);
    fill_bot_actions(&sim.0, &bots, &mut actions);
    sim.0.tick(&actions);
    if let Some(replay) = &mut recorder.0 {
        replay.record(actions);
    }
}

pub(super) fn check_outcome(
    sim: Res<Sim>,
    campaign: Res<Campaign>,
    mut next_phase: ResMut<NextState<Phase>>,
) {
    match campaign.current().outcome(&sim.0) {
        PuzzleOutcome::Running => {}
        PuzzleOutcome::Won => next_phase.set(Phase::Won),
        PuzzleOutcome::Lost => next_phase.set(Phase::Lost),
    }
}

/// Resolve what each seat is called this round; chained right after
/// `load_versus`. Online, the handshake's table is the only truth, since
/// the local couch names must never label a rival; offline they are just
/// what the player typed for their table.
pub(super) fn resolve_seat_names(
    online: Res<net::Online>,
    playback: Res<Playback>,
    settings: Res<crate::app::settings::GameSettings>,
    mut recorder: ResMut<Recorder>,
    mut names: ResMut<SeatNames>,
) {
    names.0 = match (&playback.0, &online.0) {
        // A replay is watched, not played: the names belong to the round on
        // screen, not to whoever is sitting here now. Without this, watching
        // somebody else's online match relabelled every crab with the local
        // couch names.
        (Some((replay, _)), _) => replay.names.clone(),
        (None, Some(session)) => session.names.clone(),
        (None, None) => settings.names.clone(),
    };
    // And stamp them onto the round being recorded, which was created one
    // system earlier, before anybody knew who was playing. Recorded now
    // rather than at save time because by then the session is gone and an
    // online round's names went with it.
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
pub(super) fn poll_reel(mut reel_thread: ResMut<ReelThread>, mut highlight: ResMut<Highlight>) {
    let Some(answer) = reel_thread.0.as_ref().and_then(|slot| slot.get().cloned()) else {
        return;
    };
    reel_thread.0 = None;
    if answer.is_some() {
        highlight.0 = answer;
    }
}

/// Who to file a finished round under: the leading seat's name, or nobody.
fn winner_name(
    sim: &Sim,
    seats: &Seats,
    settings: &crate::app::settings::GameSettings,
    online: &net::Online,
    names: &SeatNames,
) -> String {
    let mode = crate::app::teams::in_play(settings, online, seats.0);
    let leaders = crate::app::side_panels::leading_seats(sim.0.scores(), seats.0, mode);
    match leaders.iter().position(|&led| led) {
        Some(seat) => names.label(settings.tr(), seat as u8),
        None => "draw".to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn check_versus_over(
    sim: Res<Sim>,
    seats: Res<Seats>,
    settings: Res<crate::app::settings::GameSettings>,
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
            let library = crate::app::replays::library_dir();
            let text = replay.to_text();
            // `last.txt` is still the newest round, for the menu's Replay
            // entry and the dev hook; the library keeps every round beside
            // it under a name that says when it was and who took it.
            let last = crate::app::replay_path();
            match crate::app::paths::write_atomic(&last, &text) {
                Ok(()) => info!("replay saved to {}", last.display()),
                Err(e) => warn!("could not save replay: {e}"),
            }
            let stamp = crate::app::clock::now_secs();
            let winner = winner_name(&sim, &seats, &settings, &online, &seat_names);
            let kept = library.join(crate::app::replays::file_name(stamp, &winner));
            if let Err(e) = crate::app::paths::write_atomic(&kept, &text) {
                warn!("could not file the replay: {e}");
            }
            // Trimmed here, the one moment the shelf can have grown.
            crate::app::replays::prune(settings.replay_cap);
            // The reel re-simulates the whole round twice and encodes 150
            // frames, so it goes on its own thread: the results card should
            // appear the instant the tide comes in, not after the GIF.
            // The card only says the reel is there once it is: the thread
            // answers with the path when the GIF is written, and nothing
            // when the round was too short or the write failed, and
            // `poll_reel` carries the answer over to the card.
            let reel = crate::app::highlight_path();
            let answer = Arc::new(OnceLock::new());
            reel_thread.0 = Some(Arc::clone(&answer));
            std::thread::spawn(move || {
                let saved = match crate::highlight::reel(&replay) {
                    Some(bytes) => match crate::app::paths::write_atomic(&reel, bytes) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{BotLevel, Level, Replay};
    use bevy::prelude::*;

    /// A bystander the teardown must not touch.
    #[derive(Component)]
    struct Marker;

    /// The smallest world `advance_sim` will run in: a board, the four
    /// resources it reads, and nothing else.
    fn sim_app() -> App {
        let mut app = App::new();
        app.insert_resource(Sim(crate::sim::classic_arena(false, 2)));
        app.init_resource::<PendingActions>();
        app.init_resource::<Paused>();
        app.init_resource::<net::Online>();
        app.init_resource::<Recorder>();
        app.init_resource::<Playback>();
        app.init_resource::<crate::app::replays::PlaybackSpeed>();
        app.init_resource::<Bots>();
        app.add_systems(Update, advance_sim);
        app
    }

    /// What a finished round is filed under. Nothing else tests this, and
    /// it names every file in the replay library: a draw and a team win
    /// both have to answer, and a seat that was renamed keeps its name.
    #[test]
    fn a_kept_round_is_filed_under_whoever_took_it() {
        let mut board = crate::sim::Board::new(5, 5, 0);
        board.set_tile(0, 0, crate::sim::TileKind::Castle(0));
        board.set_tile(4, 4, crate::sim::TileKind::Castle(1));
        let seats = Seats(2);
        let online = net::Online::default();
        let named = |scores: [u32; MAX_PLAYERS], settings: &crate::app::settings::GameSettings| {
            let mut board = board.clone();
            for (seat, score) in scores.iter().enumerate() {
                board.set_score(seat as u8, *score);
            }
            // Couch rounds resolve seat names from the settings table.
            let names = SeatNames(settings.names.clone());
            winner_name(&Sim(board), &seats, settings, &online, &names)
        };

        let plain = crate::app::settings::GameSettings::default();
        assert_eq!(named([0, 7, 0, 0, 0, 0], &plain), "P2");
        // Level scores are nobody's round, and the file has to say so
        // rather than crediting the lowest seat.
        assert_eq!(named([7, 7, 0, 0, 0, 0], &plain), "draw");
        assert_eq!(named([0, 0, 0, 0, 0, 0], &plain), "draw");

        // A renamed seat is filed under its name, since that is what the
        // player will look for on the shelf.
        let mut settings = crate::app::settings::GameSettings::default();
        settings.names[1] = "Bo".to_string();
        assert_eq!(named([0, 7, 0, 0, 0, 0], &settings), "Bo");
    }

    fn armed(seats: u8, bots: u8) -> match_setup::MatchConfig {
        match_setup::MatchConfig {
            seats,
            bots,
            armed: true,
            ..match_setup::MatchConfig::default()
        }
    }

    /// The AI fills from the top seat down, each with its own level, and
    /// leaves the human seats alone.
    /// Boards from the shelf or a pasted code can be any size at all, and
    /// the cursor's opening spot has to be on every one of them: near the
    /// castle on a real arena, and simply on the board when there is no
    /// room for the two-tile inset.
    #[test]
    fn a_cursor_opens_on_the_board_whatever_its_size() {
        for (w, h) in [(1, 1), (2, 1), (3, 3), (4, 5), (5, 5), (12, 9), (20, 13)] {
            let board = crate::sim::Board::new(w, h, 0);
            for player in 0..MAX_PLAYERS as u8 {
                let (x, y) = cursor_home(&board, player);
                assert!(
                    x < w && y < h,
                    "seat {player} opened at ({x},{y}) on a {w}x{h} board"
                );
            }
        }
        let big = crate::sim::Board::new(12, 9, 0);
        assert_eq!(
            cursor_home(&big, 0),
            (2, 2),
            "two in from the host's corner"
        );
        assert_eq!(cursor_home(&big, 1), (9, 6), "and from the far one");
    }

    #[test]
    fn the_ai_takes_the_top_seats() {
        let mut config = armed(4, 2);
        config.bot_levels[3] = BotLevel::Hard;
        config.bot_levels[2] = BotLevel::Easy;
        assert_eq!(bot_seats(&config), {
            let mut want = [None; MAX_PLAYERS];
            want[2] = Some(BotLevel::Easy);
            want[3] = Some(BotLevel::Hard);
            want
        });
        // An unarmed config is a dev hook or a replay: nobody is botted.
        let mut idle = armed(4, 3);
        idle.armed = false;
        assert_eq!(bot_seats(&idle), [None; MAX_PLAYERS]);
    }

    /// Each way into a round has its own authority on the seat count, and
    /// they are consulted in a fixed order.
    #[test]
    fn every_entry_path_knows_its_own_seat_count() {
        let config = armed(3, 1);
        // A replay: the highest castle owner on the recorded board.
        assert_eq!(seat_count(&config, true, None, Some(3), 0), 4);
        assert_eq!(
            seat_count(&config, true, None, None, 0),
            2,
            "a castle-less recording still seats two"
        );
        // Online beats the local config: the lobby agreed the count.
        assert_eq!(seat_count(&config, false, Some(2), None, 0), 2);
        // A configured match is told.
        assert_eq!(seat_count(&config, false, None, None, 0), 3);
        // A dev hook: the keyboard's two seats, plus a pad each beyond that.
        let mut hook = armed(3, 1);
        hook.armed = false;
        assert_eq!(seat_count(&hook, false, None, None, 0), 2);
        assert_eq!(seat_count(&hook, false, None, None, 3), 3);
    }

    /// Two of the sources are outside the game's control, and the count
    /// they give becomes the length of every per-seat loop. A seventh seat
    /// used to run straight off the end of the `MAX_PLAYERS`-long scores in
    /// `leading_seats` and panic the round.
    #[test]
    fn a_seat_count_never_leaves_the_table() {
        let config = armed(3, 1);
        let seated = MAX_PLAYERS as u8;
        // A host can put any byte in a `Start`.
        assert_eq!(seat_count(&config, false, Some(200), None, 0), seated);
        assert_eq!(seat_count(&config, false, Some(0), None, 0), 2);
        // And a player can plug in more gamepads than there are chairs.
        let mut hook = armed(3, 1);
        hook.armed = false;
        assert_eq!(seat_count(&hook, false, None, None, 9), seated);
        // Whatever comes back is a count the per-seat arrays can hold.
        for online in [None, Some(0), Some(3), Some(255)] {
            for pads in [0, 2, 200] {
                let seats = seat_count(&hook, false, online, Some(255), pads);
                assert!((2..=seated).contains(&seats), "{online:?}/{pads} → {seats}");
            }
        }
    }

    /// The AI fill is why an online AI seat is safe at all: two peers holding
    /// the same board must produce the same moves for it, frame after frame,
    /// while the humans' moves ride over the wire untouched.
    #[test]
    fn the_ai_fill_is_identical_on_every_peer() {
        use crate::sim::{Direction, classic_arena_seeded};

        // Two peers, each with its own copy of the same beach.
        let mut here = classic_arena_seeded(0x51DE, false, 4);
        let mut there = classic_arena_seeded(0x51DE, false, 4);
        // Two humans in the low seats, two AI behind them: a 2v2 online match.
        let mut bots = Bots::default();
        bots.0[2] = Some(BotLevel::Normal);
        bots.0[3] = Some(BotLevel::Hard);

        let pressed = PlayerAction::Place {
            x: 4,
            y: 4,
            dir: Direction::Up,
        };
        for frame in 0..600 {
            // The wire delivers the humans' actions to both peers alike.
            let mut wire = [PlayerAction::None; MAX_PLAYERS];
            if frame == 30 {
                wire[1] = pressed;
            }
            let (mut mine, mut theirs) = (wire, wire);
            fill_bot_actions(&here, &bots, &mut mine);
            fill_bot_actions(&there, &bots, &mut theirs);
            assert_eq!(mine, theirs, "peers disagreed on an AI seat at {frame}");
            assert_eq!(mine[0], PlayerAction::None, "seat 1 has no AI to fill");
            if frame == 30 {
                assert_eq!(mine[1], pressed, "a human's press was overwritten");
            }
            here.tick(&mine);
            there.tick(&theirs);
            assert_eq!(
                here.state_hash(),
                there.state_hash(),
                "the boards diverged at frame {frame}"
            );
        }
        // And the AI seats actually did something with those 600 frames.
        assert!(
            here.scores()[2] > 0 || here.scores()[3] > 0,
            "no AI seat banked anything, so the fill proved nothing"
        );
    }

    /// The teardown has to sweep *every* category of board sprite.
    ///
    /// Its own comment records what a miss costs: a category left behind
    /// once meant turnstile sprites surviving onto a smaller board and
    /// panicking in `tile_at` - a crash at level load, from a sprite
    /// nobody was looking at. The two teardown paths share this bundle so
    /// they cannot drift apart, and this is the guard that the bundle
    /// itself is complete.
    #[test]
    fn the_teardown_leaves_no_board_sprite_behind() {
        use crate::app::{board_render, creatures};
        let mut app = App::new();
        // One of each marked category, plus a bystander that must survive:
        // a teardown that simply emptied the world would pass every
        // assertion below without it.
        let survivor = app.world_mut().spawn(Marker).id();
        app.world_mut().spawn(board_render::BoardStatic);
        app.world_mut().spawn(board_render::Waterline(0));
        app.world_mut().spawn(board_render::WaterFoam(0));
        app.world_mut().spawn(creatures::CrabSprite {
            id: 1,
            kind: crate::sim::CrabKind::Common,
            shade: 0.0,
        });
        app.world_mut().spawn(creatures::GullSprite(1));
        let before = app.world().entities().len();
        assert!(before > 5, "the fixture did not spawn");

        app.add_systems(Update, despawn_board_sprites);
        app.update();

        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<board_render::BoardStatic>>()
                .iter(app.world())
                .count(),
            0,
            "the static beach survived the teardown"
        );
        for left in [
            app.world_mut()
                .query_filtered::<Entity, With<board_render::Waterline>>()
                .iter(app.world())
                .count(),
            app.world_mut()
                .query_filtered::<Entity, With<board_render::WaterFoam>>()
                .iter(app.world())
                .count(),
            app.world_mut()
                .query_filtered::<Entity, With<creatures::CrabSprite>>()
                .iter(app.world())
                .count(),
            app.world_mut()
                .query_filtered::<Entity, With<creatures::GullSprite>>()
                .iter(app.world())
                .count(),
        ] {
            assert_eq!(left, 0, "a category of board sprite survived the teardown");
        }
        assert!(
            app.world().get_entity(survivor).is_ok(),
            "the teardown took something that was not a board sprite"
        );
    }

    /// A round nobody is watching still stops when it is paused.
    #[test]
    fn a_paused_round_does_not_advance() {
        let mut app = sim_app();
        app.world_mut().resource_mut::<Paused>().0 = true;
        let before = app.world().resource::<Sim>().0.ticks();
        app.update();
        assert_eq!(
            app.world().resource::<Sim>().0.ticks(),
            before,
            "a paused beach kept walking"
        );
        app.world_mut().resource_mut::<Paused>().0 = false;
        app.update();
        assert!(
            app.world().resource::<Sim>().0.ticks() > before,
            "and never started again"
        );
    }

    /// A recording is fed one frame per tick, or as many as the transport
    /// asks for. The speed is the whole of the fast-forward: there is no
    /// separate scrubbing path, so a speed that did not multiply here
    /// would be a transport button that did nothing.
    #[test]
    fn the_transport_speed_is_how_many_frames_a_replay_eats() {
        for speed in [1u8, 2, 4] {
            let mut app = sim_app();
            let level = Level::from_board("Turf War", 3, crate::sim::classic_arena(false, 2));
            let mut replay = Replay::new(level);
            for _ in 0..40 {
                replay.record([PlayerAction::None; MAX_PLAYERS]);
            }
            app.world_mut().insert_resource(Playback(Some((replay, 0))));
            app.world_mut()
                .insert_resource(crate::app::replays::PlaybackSpeed(speed));
            app.update();
            let (_, idx) = app
                .world()
                .resource::<Playback>()
                .0
                .as_ref()
                .expect("still watching");
            assert_eq!(
                *idx,
                usize::from(speed),
                "at {speed}x the recording moved {idx} frames in one tick"
            );
        }
    }

    /// And it stops at the end rather than running off the tape.
    #[test]
    fn a_replay_stops_when_the_recording_runs_out() {
        let mut app = sim_app();
        let level = Level::from_board("Turf War", 3, crate::sim::classic_arena(false, 2));
        let mut replay = Replay::new(level);
        replay.record([PlayerAction::None; MAX_PLAYERS]);
        app.world_mut().insert_resource(Playback(Some((replay, 0))));
        app.world_mut()
            .insert_resource(crate::app::replays::PlaybackSpeed(4));
        for _ in 0..5 {
            app.update();
        }
        let (replay, idx) = app
            .world()
            .resource::<Playback>()
            .0
            .as_ref()
            .expect("still watching");
        assert_eq!(*idx, replay.inputs.len(), "it read past the last frame");
    }
}
