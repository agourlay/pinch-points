//! The versus/puzzle session driver: board boot and teardown, the level
//! reload path, the fixed-tick sim driver, and the round-outcome checks.
//! Split from the app's `mod.rs` so that keeps only types and wiring: a
//! versus round's life, a puzzle run's, and the tick they share are each
//! a file here, with what both draw from beside them.

use super::*;

mod puzzle;
mod tick;
mod versus;

pub(super) use puzzle::*;
pub(crate) use tick::*;
pub(super) use versus::*;

/// Where a seat's cursor starts a round: fanned out near its own castle,
/// two tiles in from the corner so the first press is not into a wall.
///
/// Clamped to the board, not to the two-tile inset: a custom arena can be
/// as small as the level format allows, and asking `clamp` for an inset
/// the board cannot hold took a 3x3 beach down on its first frame. On a
/// board too small for the inset the cursor sits as far in as there is.
fn cursor_home(board: &Board, player: u8) -> (u8, u8) {
    let (w, h) = (board.width(), board.height());
    // The board is asked first, and the spot table is only the fallback:
    // which seat owns which castle is drawn with the beach (see
    // `seat_spots`), and a handmade beach was never in the table at all.
    let spots = castle_spots(w, h);
    let (cx, cy) = board
        .castle_of(player)
        .unwrap_or_else(|| spots[usize::from(player).min(spots.len() - 1)]);
    let inset = |len: u8| 2.min(len.saturating_sub(1) / 2);
    (
        cx.clamp(inset(w), w - 1 - inset(w)),
        cy.clamp(inset(h), h - 1 - inset(h)),
    )
}

/// The board in play, the actions queued for its next tick, and whether
/// it is held: what loading a round resets and ticking one advances.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Play<'w> {
    sim: ResMut<'w, Sim>,
    pending: ResMut<'w, PendingActions>,
    paused: ResMut<'w, Paused>,
}

/// What laying a fresh board out takes: the commands and art to draw its
/// statics with, and the cursors to set down on it. Both ways into a board
/// (a versus round, a puzzle level) draw it the same way and home the
/// cursors their own way.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct BoardStage<'w, 's> {
    commands: Commands<'w, 's>,
    art: Res<'w, art::Art>,
    cursors: Query<'w, 's, (&'static mut cursor::Cursor, &'static mut Transform)>,
    generation: ResMut<'w, sim_events::BoardGeneration>,
}

impl BoardStage<'_, '_> {
    /// Draw everything about `board` that does not move: the beach, the
    /// sand and the walls. And count it a new board, which a board laid
    /// out afresh is whatever its clock says.
    fn lay_out(&mut self, board: &Board) {
        self.generation.laid_out(board);
        board_render::spawn_static_board(&mut self.commands, board, &self.art);
    }
}

/// Every category of entity rendered from board state, bundled so the two
/// teardown paths (screen exit and level reload) cannot drift apart: a
/// missed category left turnstile sprites probing a smaller board and
/// panicking in `tile_at`.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct BoardSprites<'w, 's> {
    statics: Query<'w, 's, Entity, With<board_render::BoardStatic>>,
    posts: Query<'w, 's, Entity, With<board_render::SignpostSprite>>,
    castles: Query<'w, 's, Entity, With<board_render::CastleSprite>>,
    logs: Query<'w, 's, Entity, With<board_render::TurnstileSprite>>,
    crabs: Query<'w, 's, Entity, With<creatures::CrabSprite>>,
    gulls: Query<'w, 's, Entity, With<creatures::GullSprite>>,
    /// Gulls flying in to the old beach, on their way to tiles it had.
    arrivals: Query<'w, 's, Entity, With<creatures::GullArrival>>,
    /// Crab Monopoly's crabs in the air. Only their own system takes them
    /// down otherwise, and it runs on the board screens alone: a round
    /// quit inside a sweep left them glowing over the menu.
    sweeps: Query<'w, 's, Entity, With<creatures::SweepSprite>>,
    /// The old beach's dust, splashes, footprints and crabs hopping into
    /// its castles: drawn where things stood on it, and on a reload or
    /// an editor swap on the same screen they went on over the new one.
    particles: Query<'w, 's, Entity, With<effects::Particle>>,
    hops: Query<'w, 's, Entity, With<effects::Hop>>,
}

impl BoardSprites<'_, '_> {
    pub(super) fn despawn_all(&self, commands: &mut Commands) {
        for entity in self
            .statics
            .iter()
            .chain(self.posts.iter())
            .chain(self.castles.iter())
            .chain(self.logs.iter())
            .chain(self.crabs.iter())
            .chain(self.gulls.iter())
            .chain(self.arrivals.iter())
            .chain(self.sweeps.iter())
            .chain(self.particles.iter())
            .chain(self.hops.iter())
        {
            commands.entity(entity).despawn();
        }
    }
}

/// Despawn everything rendered from board state, in every category.
pub(super) fn despawn_board_sprites(mut commands: Commands, sprites: BoardSprites) {
    sprites.despawn_all(&mut commands);
}

/// Who decides each seat of an online round on this peer, as the round
/// reads it.
#[cfg(test)]
pub(crate) fn online_controllers_for_tests(session: &net::OnlineSession) -> Controllers {
    online_controllers(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{BotLevel, Level, Replay};
    use bevy::prelude::*;

    /// A bystander the teardown must not touch.
    #[derive(Component)]
    struct Marker;

    /// The smallest world `advance_sim` will run in: a board, the
    /// resources it reads and writes, and nothing else.
    fn sim_app() -> App {
        let mut app = App::new();
        app.insert_resource(Sim(classic_arena(false, 2)));
        app.init_resource::<PendingActions>();
        app.init_resource::<Paused>();
        app.init_resource::<net::Online>();
        app.init_resource::<Recorder>();
        app.init_resource::<Playback>();
        app.init_resource::<replays::PlaybackSpeed>();
        app.init_resource::<Controllers>();
        app.init_resource::<bot_seats::BotSeats>();
        app.init_resource::<awards::RoundTally>();
        app.add_systems(Update, advance_sim);
        app
    }

    /// A couch match set up here opens on the line-up, every round of it;
    /// the daily, a dev hook's table and a resumed round do not.
    #[test]
    fn a_couch_match_opens_on_the_lineup() {
        use super::versus::RoundOrigin;
        let config = crate::app::match_setup::MatchConfig::default();
        assert!(RoundOrigin::Configured(&config).opens_on_lineup(true));
        assert!(
            !RoundOrigin::Configured(&config).opens_on_lineup(false),
            "the daily"
        );
        assert!(!RoundOrigin::Unconfigured.opens_on_lineup(true));
    }

    /// What a finished round is filed under. Nothing else tests this, and
    /// it names every file in the replay library: a draw and a team win
    /// both have to answer, and a seat that was renamed keeps its name.
    #[test]
    fn a_kept_round_is_filed_under_whoever_took_it() {
        let mut board = Board::new(5, 5, 0);
        board.set_tile(0, 0, crate::sim::TileKind::Castle(0));
        board.set_tile(4, 4, crate::sim::TileKind::Castle(1));
        let seats = Seats(2);
        let online = net::Online::default();
        let named = |scores: [u32; MAX_PLAYERS], settings: &settings::GameSettings| {
            let mut board = board.clone();
            for (seat, score) in scores.iter().enumerate() {
                board.set_score(seat as u8, *score);
            }
            // Couch rounds resolve seat names from the settings table.
            let names = SeatNames(settings.names.clone());
            let mode = teams::in_play(settings.team_mode, &online, None, seats.0);
            winner_name(&Sim(board), &seats, mode, &names, settings.tr())
        };

        let plain = settings::GameSettings::default();
        assert_eq!(named([0, 7, 0, 0, 0, 0], &plain).as_deref(), Some("P2"));
        // Level scores are nobody's round, and the file has to say so
        // rather than crediting the lowest seat.
        assert_eq!(named([7, 7, 0, 0, 0, 0], &plain), None);
        assert_eq!(named([0, 0, 0, 0, 0, 0], &plain), None);

        // A renamed seat is filed under its name, since that is what the
        // player will look for on the shelf.
        let mut settings = settings::GameSettings::default();
        settings.names[1] = "Bo".to_string();
        assert_eq!(named([0, 7, 0, 0, 0, 0], &settings).as_deref(), Some("Bo"));

        // A round won in pairs is filed under the pair, as the card names
        // it, and a replay of one under the mode it was played in, not
        // the setting it is watched with.
        let four = Seats(4);
        let mut board = Board::new(5, 5, 0);
        for seat in 0..4 {
            board.set_tile(seat, 0, crate::sim::TileKind::Castle(seat));
            board.set_score(seat, [5, 5, 4, 5][usize::from(seat)]);
        }
        let pairs = settings::GameSettings {
            team_mode: teams::TeamMode::Pairs,
            ..settings::GameSettings::default()
        };
        let names = SeatNames(pairs.names.clone());
        let live_mode = teams::in_play(pairs.team_mode, &online, None, four.0);
        let live = winner_name(&Sim(board.clone()), &four, live_mode, &names, pairs.tr());
        assert_eq!(live.as_deref(), Some("P1+P2"));
        let mut replay = Replay::new(Level::from_board("T", 3, board.clone()));
        replay.teams = teams::TeamMode::Pairs.index() as u8;
        let watched_mode = teams::in_play(plain.team_mode, &online, Some(&replay), four.0);
        let watched = winner_name(&Sim(board), &four, watched_mode, &names, plain.tr());
        assert_eq!(watched.as_deref(), Some("P1+P2"));
    }

    fn armed(seats: u8, bots: u8) -> match_setup::MatchConfig {
        match_setup::MatchConfig {
            armed: true,
            ..match_setup::MatchConfig::with_ai(seats, bots)
        }
    }

    /// A round launched from the setup steps is played on the board its
    /// map card showed: the board the card's picture is drawn from (the
    /// card's config, the setup's seed) against the one the round builds
    /// from the config the launch leaves. With no seed kept (a series'
    /// next round, a rematch) every round draws its own.
    #[test]
    fn a_launched_round_is_played_on_the_previewed_seed() {
        use match_setup::{Card, MapChoice};
        let beaches = match_setup::CustomBeaches::default();
        let setup = armed(4, 2);
        let card = Card::Map(MapChoice::GenLarge);
        let shown = match_setup::card_config(&setup, card, &beaches).expect("a card");
        let pictured = match_setup::preview_board(&shown, &beaches, 31337);
        let mut launched = setup.clone();
        match_setup::pick_card(&mut launched, card, &beaches);
        match_setup::seat_the_match(&mut launched, &[], 31337, &beaches, 0);
        let mut board = RoundOrigin::Configured(&launched).board(None, &beaches, false, 0);
        assert_eq!(board.seed(), 31337);
        for _ in 0..match_setup::PREVIEW_TICKS {
            board.tick_idle();
        }
        assert_eq!(board.state_hash(), pictured.state_hash());
        let fresh = match_setup::MatchConfig {
            seed: None,
            ..launched
        };
        let board = RoundOrigin::Configured(&fresh).board(None, &beaches, false, 0);
        assert_ne!(board.seed(), 31337);
    }

    /// Step 3 puts the AI in any seat, not only the top ones: the round
    /// seats it where step 3 did, and a cursor on every other seat.
    #[test]
    fn the_ai_sits_where_step_three_put_it() {
        let mut config = armed(4, 0);
        config.controllers[1] = SeatController::Ai(BotLevel::Easy);
        let table = bot_seats(&config);
        assert_eq!(table.0[1], SeatController::Ai(BotLevel::Easy));
        assert_eq!(table.0[3], SeatController::Local);
        assert_eq!(config.human_seats(), vec![0, 2, 3]);
    }

    /// The open ocean has no edges on the couch either. The local launch
    /// built its own board and never opened them, so a couch round on it
    /// was played walled while the same map online wrapped.
    #[test]
    fn a_couch_round_on_the_open_ocean_wraps() {
        let beaches = match_setup::CustomBeaches::default();
        for (map, wraps) in [
            (match_setup::MapChoice::GenOcean, true),
            (match_setup::MapChoice::GenLarge, false),
            (match_setup::MapChoice::Classic, false),
        ] {
            let config = match_setup::MatchConfig { map, ..armed(2, 0) };
            let board = RoundOrigin::Configured(&config).board(None, &beaches, false, 0);
            assert_eq!(board.wrap(), wraps, "{map:?}");
        }
    }

    /// The daily's table is one human and three fierce rivals, built from
    /// its own config rather than the player's.
    #[test]
    fn the_daily_seats_one_human_against_three_hard_bots() {
        let daily = match_setup::MatchConfig::daily();
        assert!(daily.armed, "ready to launch as it is");
        let table = bot_seats(&daily);
        assert_eq!(table.0[0], SeatController::Local, "the player's chair");
        assert_eq!(
            &table.0[1..4],
            &[SeatController::Ai(BotLevel::Hard); 3],
            "three fierce rivals"
        );
        assert!(
            table.0[4..].iter().all(|c| *c == SeatController::Local),
            "and no more chairs"
        );
        let player = match_setup::MatchConfig::default();
        assert_ne!(player.seats, daily.seats, "the player's own is untouched");
    }

    /// A seat a bot drives online is a bot's, not this machine's person's:
    /// joined as a bot (route 1) or come straight to the host (route 2),
    /// it earns no trophies and is not tagged "(you)", both of which key
    /// on `Local`.
    #[test]
    fn a_seat_a_bot_drives_online_is_not_the_players() {
        let session = |seat: u8, bot: bool, bots_here: Vec<u8>| {
            let mut session = net::OnlineSession::new(
                crate::transport::UdpTransport::host(0).expect("socket"),
                crate::sim::Lockstep::new(seat, vec![0, 1, 2], crate::sim::DEFAULT_DELAY),
                3,
                crate::transport::MatchTerms::default(),
            );
            session.bot = bot.then(|| "Greedy (Ana)".to_string());
            session.bots_here = bots_here
                .into_iter()
                .map(|seat| net::HostedBot {
                    seat,
                    id: 0,
                    name: "Greedy".to_string(),
                })
                .collect();
            session
        };
        // A person at seat one: their own chair, the others down the wire.
        let table = online_controllers(&session(1, false, vec![]));
        assert_eq!(table.0[1], SeatController::Local);
        assert_eq!(table.0[0], SeatController::Remote);
        // Route 1: this peer's own seat is its bot's.
        let table = online_controllers(&session(1, true, vec![]));
        assert_eq!(
            table.0[1],
            SeatController::Bot,
            "the bot's, not the person's"
        );
        assert_eq!(table.0[0], SeatController::Remote);
        // Route 2: the host plays seat zero, its bot seat two.
        let table = online_controllers(&session(0, false, vec![2]));
        assert_eq!(table.0[0], SeatController::Local);
        assert_eq!(table.0[2], SeatController::Bot);
        assert_eq!(table.0[1], SeatController::Remote);
    }

    /// Boards from the shelf or a pasted code can be any size at all, and
    /// the cursor's opening spot has to be on every one of them: near the
    /// castle on a real arena, and simply on the board when there is no
    /// room for the two-tile inset.
    #[test]
    fn a_cursor_opens_on_the_board_whatever_its_size() {
        for (w, h) in [(1, 1), (2, 1), (3, 3), (4, 5), (5, 5), (12, 9), (20, 13)] {
            let board = Board::new(w, h, 0);
            for player in 0..MAX_PLAYERS as u8 {
                let (x, y) = cursor_home(&board, player);
                assert!(
                    x < w && y < h,
                    "seat {player} opened at ({x},{y}) on a {w}x{h} board"
                );
            }
        }
        let big = Board::new(12, 9, 0);
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
        config.controllers[3] = SeatController::Ai(BotLevel::Hard);
        config.controllers[2] = SeatController::Ai(BotLevel::Easy);
        assert_eq!(bot_seats(&config).levels(), {
            let mut want = [None; MAX_PLAYERS];
            want[2] = Some(BotLevel::Easy);
            want[3] = Some(BotLevel::Hard);
            want
        });
        // An unarmed config is a dev hook or a replay: nobody is botted.
        let mut idle = armed(4, 3);
        idle.armed = false;
        assert_eq!(bot_seats(&idle), Controllers::default());
    }

    /// A recording is watched at the table it was played at: its AI seats
    /// wear "(AI)" on the chips and the card as they did live, and C copies
    /// them as the AI (a protocol bot's as the stand-in AI), not as seats
    /// for people nobody here will sit in. An AI seat from before levels
    /// were kept is still the AI.
    #[test]
    fn a_replay_is_watched_at_its_own_table() {
        use crate::app::seating::{Viewer, seat_tag};
        use crate::sim::SeatKind;
        let level = Level::from_board("Turf War", 3, classic_arena(false, 4));
        let mut replay = Replay::new(level);
        replay.kinds[1] = SeatKind::Ai;
        replay.ai[1] = Some(BotLevel::Hard);
        replay.kinds[2] = SeatKind::Bot;
        replay.kinds[3] = SeatKind::Ai;
        let table = replay_controllers(&replay);
        let tr = crate::app::i18n::Lang::En.tr();
        assert_eq!(seat_tag(tr, &table, Viewer::Replay, 0), "");
        assert_eq!(seat_tag(tr, &table, Viewer::Replay, 1), tr.tag_ai);
        assert_eq!(seat_tag(tr, &table, Viewer::Replay, 3), tr.tag_ai);
        assert_eq!(table.0[2], SeatController::Bot);
        let copied = table.levels();
        assert_eq!(copied[0], None, "the person stays a person");
        assert_eq!(copied[1], Some(BotLevel::Hard));
        assert_eq!(copied[2], Some(bot_seats::STAND_IN));
        assert_eq!(copied[3], Some(BotLevel::default()));
        // And the arena builds it so, whatever the player's own dials say.
        let config = armed(4, 2);
        let board = classic_arena(false, 4);
        let origin = RoundOrigin::Replay(&replay);
        assert_eq!(origin.table(&config, &board, 0).0, table);
    }

    /// A round copied with a bot in it keeps that seat off the keyboard:
    /// pasted back, the bot is not there, so the AI that stands in for a
    /// bot holds it, rather than a person's keys or nobody at all.
    #[test]
    fn a_bots_seat_is_saved_as_the_ai_that_stands_in_for_it() {
        let mut config = armed(3, 2);
        config.controllers[2] = SeatController::Bot;
        config.controllers[1] = SeatController::Ai(BotLevel::Hard);
        let table = bot_seats(&config);
        assert_eq!(table.0[2], SeatController::Bot);
        let saved = table.levels();
        assert_eq!(saved[0], None, "the person stays a person");
        assert_eq!(saved[1], Some(BotLevel::Hard));
        assert_eq!(
            saved[2],
            Some(bot_seats::STAND_IN),
            "the bot's seat is the stand-in AI's"
        );
        let resumed = Controllers::from_levels(saved);
        assert_eq!(resumed.0[2], SeatController::Ai(bot_seats::STAND_IN));
    }

    /// A replay's board with castles for `seats` seats.
    fn recorded(seats: u8) -> Replay {
        Replay::new(Level::from_board(
            "Turf War",
            3,
            classic_arena(false, seats),
        ))
    }

    fn online_at(seats: u8) -> net::OnlineSession {
        net::OnlineSession::new(
            crate::transport::UdpTransport::host(0).expect("socket"),
            crate::sim::Lockstep::new(0, vec![0, 1], crate::sim::DEFAULT_DELAY),
            seats,
            crate::transport::MatchTerms::default(),
        )
    }

    /// Each way into a round has its own authority on the seat count.
    #[test]
    fn every_entry_path_knows_its_own_seat_count() {
        let config = armed(3, 1);
        // A replay: the highest castle owner on the recorded board.
        let replay = recorded(4);
        let board = replay.level.board();
        assert_eq!(RoundOrigin::Replay(&replay).table(&config, &board, 0).1, 4);
        let bare = Board::new(4, 4, 1);
        assert_eq!(
            RoundOrigin::Replay(&replay).table(&config, &bare, 0).1,
            2,
            "a castle-less recording still seats two"
        );
        // Online: the lobby agreed the count, whatever the local config says.
        let session = online_at(2);
        assert_eq!(RoundOrigin::Online(&session).table(&config, &bare, 0).1, 2);
        // A configured match is told.
        assert_eq!(
            RoundOrigin::Configured(&config).table(&config, &bare, 0).1,
            3
        );
        // A dev hook: the keyboard's two seats, plus a pad each beyond that.
        assert_eq!(RoundOrigin::Unconfigured.table(&config, &bare, 0).1, 2);
        assert_eq!(RoundOrigin::Unconfigured.table(&config, &bare, 3).1, 3);
    }

    /// The daily's beach is the one for the day handed in, which is the day
    /// `load_versus` keeps for the trophies, and not whatever the clock says
    /// by the time anyone asks again.
    #[test]
    fn the_daily_is_built_from_the_day_it_is_handed() {
        let daily = match_setup::MatchConfig::daily();
        let built = |day| {
            RoundOrigin::Configured(&daily)
                .board(Some(day), &Default::default(), false, 0)
                .state_hash()
        };
        assert_eq!(built(20_000), built(20_000), "one beach for everybody");
        assert_ne!(built(20_000), built(20_001), "and a fresh one tomorrow");
    }

    /// The arrows dial on the setup card is the rule the round is played
    /// under, on every kind of beach it builds, and an online round keeps
    /// the versus three whatever this machine's card says.
    #[test]
    fn the_arrows_dial_is_the_rule_the_round_plays() {
        use crate::app::match_setup::MapChoice;
        for map in [MapChoice::Classic, MapChoice::GenLarge] {
            for posts in match_setup::POSTS_RANGE {
                let config = match_setup::MatchConfig {
                    map,
                    posts,
                    ..armed(2, 1)
                };
                let board =
                    RoundOrigin::Configured(&config).board(None, &Default::default(), false, 0);
                assert_eq!(
                    board.signpost_rule(),
                    (posts, crate::sim::CapPolicy::Evict),
                    "{map:?}"
                );
            }
        }
        let board = RoundOrigin::Online(&online_at(2)).board(None, &Default::default(), false, 0);
        assert_eq!(
            board.signpost_rule().0,
            crate::sim::MAX_SIGNPOSTS_PER_PLAYER as u8
        );
    }

    /// A table of five on a four-castle beach, however it got there, is
    /// played on a beach with a castle for every seat.
    #[test]
    fn a_table_too_big_for_its_beach_still_gets_a_castle_each() {
        use crate::app::match_setup::MapChoice;
        for map in [
            MapChoice::Classic,
            MapChoice::GenSmall,
            MapChoice::GenClassic,
        ] {
            for seats in [5, 6] {
                let config = match_setup::MatchConfig {
                    map,
                    ..armed(seats, 0)
                };
                let board =
                    RoundOrigin::Configured(&config).board(None, &Default::default(), false, 0);
                let mut owners: Vec<u8> = board.castle_owners().collect();
                owners.sort_unstable();
                owners.dedup();
                assert_eq!(owners.len(), usize::from(seats), "{seats} on {map:?}");
            }
        }
    }

    /// Two of the sources are outside the game's control, and the count
    /// they give becomes the length of every per-seat loop: a seventh seat
    /// runs off the end of the `MAX_PLAYERS`-long scores in
    /// `leading_seats`.
    #[test]
    fn a_seat_count_never_leaves_the_table() {
        let config = armed(3, 1);
        let seated = MAX_PLAYERS as u8;
        let bare = Board::new(4, 4, 1);
        // A host can put any byte in a `Start`.
        assert_eq!(
            RoundOrigin::Online(&online_at(200))
                .table(&config, &bare, 0)
                .1,
            seated
        );
        assert_eq!(
            RoundOrigin::Online(&online_at(0))
                .table(&config, &bare, 0)
                .1,
            2
        );
        // And a player can plug in more gamepads than there are chairs.
        assert_eq!(RoundOrigin::Unconfigured.table(&config, &bare, 9).1, seated);
        // Whatever comes back is a count the per-seat arrays can hold.
        for asked in [0, 1, 3, 6, 7, 200, 255] {
            assert!((2..=seated).contains(&clamp_seats(asked)), "{asked}");
        }
    }

    /// Only a round played from its first tick is recorded: a replay is a
    /// recording already, and one resumed or caught up with mid-round has
    /// no first tick to record from.
    #[test]
    fn only_a_round_played_from_its_first_tick_is_recorded() {
        let config = armed(3, 1);
        let replay = recorded(2);
        let mut caught_up = online_at(2);
        assert!(RoundOrigin::Configured(&config).recorded());
        assert!(RoundOrigin::Unconfigured.recorded());
        assert!(RoundOrigin::Online(&caught_up).recorded());
        assert!(!RoundOrigin::Replay(&replay).recorded());
        caught_up.catch_up.board = Some(Board::new(4, 4, 1));
        assert!(!RoundOrigin::Online(&caught_up).recorded());
        let resumed = suspend::Suspended {
            seats: 3,
            bots: [None; MAX_PLAYERS],
            board: Board::new(4, 4, 1),
        };
        let origin = RoundOrigin::Resumed(&resumed);
        assert!(!origin.recorded());
        assert_eq!(
            origin.table(&config, &Board::new(4, 4, 1), 0).1,
            3,
            "its own table"
        );
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
        let mut bots = Controllers::default();
        bots.0[2] = SeatController::Ai(BotLevel::Normal);
        bots.0[3] = SeatController::Ai(BotLevel::Hard);

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
    /// A category left behind meant turnstile sprites surviving onto a
    /// smaller board and panicking in `tile_at`: a crash at level load,
    /// from a sprite nobody was looking at. The two teardown paths share
    /// this bundle, and this is the guard that the bundle is complete.
    #[test]
    fn the_teardown_leaves_no_board_sprite_behind() {
        use crate::app::{board_render, creatures};
        let mut app = App::new();
        // One of each marked category, plus a bystander that must survive:
        // a teardown that simply emptied the world would pass every
        // assertion below without it.
        let survivor = app.world_mut().spawn(Marker).id();
        app.world_mut().spawn(board_render::BoardStatic);
        app.world_mut().spawn(creatures::CrabSprite {
            id: 1,
            kind: crate::sim::CrabKind::Common,
            shade: 0.0,
        });
        app.world_mut().spawn(creatures::GullSprite(1));
        app.world_mut()
            .spawn(creatures::SweepSprite::new(2, Vec2::ZERO));
        // A footprint: on a reload it was left pressed into the new beach.
        app.world_mut().spawn(effects::Particle::default());
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
                .query_filtered::<Entity, With<creatures::CrabSprite>>()
                .iter(app.world())
                .count(),
            app.world_mut()
                .query_filtered::<Entity, With<creatures::GullSprite>>()
                .iter(app.world())
                .count(),
            app.world_mut()
                .query_filtered::<Entity, With<creatures::SweepSprite>>()
                .iter(app.world())
                .count(),
            app.world_mut()
                .query_filtered::<Entity, With<effects::Particle>>()
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

    /// A seat given up on mid-round turns AI on the frame the table
    /// agreed on, and not one frame sooner.
    ///
    /// Found by killing a real joiner: every peer desynced within a second
    /// of the drop. `Controllers` says which seats the AI holds and nothing about
    /// when it took them, so `fill_bot_actions` filled the chair from the
    /// instant the notice reached each shell. That instant is the host's
    /// own decision on the host and a datagram everywhere else, and a peer
    /// still working through frames it already had the departed player's
    /// inputs for wrote a bot's move over each of them. `PlayerAction::None`
    /// is what a player pressing nothing sends, so this was most of them.
    #[test]
    fn an_abandoned_seat_turns_ai_on_the_frame_the_table_agreed_on() {
        let bots = Controllers::from_levels([None, None, Some(BotLevel::Normal), None, None, None]);
        // Seat 1 walked out, and the round agreed to empty it from 300.
        let abandoned = [(1u8, 300u32)];
        let at = |seat, frame| ai_holding(&bots, &abandoned, BotLevel::Hard, seat, frame);

        assert_eq!(at(1, 299), None, "299 was still theirs to play");
        assert_eq!(at(1, 300), Some(BotLevel::Hard), "and 300 is the AI's");
        assert_eq!(at(1, 900), Some(BotLevel::Hard), "and every frame after");

        // A seat dealt to the AI at the launch was never anyone's, so the
        // frame has nothing to say about it, and a human seat stays human.
        assert_eq!(
            at(2, 0),
            Some(BotLevel::Normal),
            "the launch AI, from the start"
        );
        assert_eq!(
            at(2, 900),
            Some(BotLevel::Normal),
            "and all the way through"
        );
        assert_eq!(at(0, 900), None, "and a seat nobody left is left be");
    }

    /// A recorded round whose tide comes in on the first frame: see
    /// [`round_over`].
    fn finished_round(dir: &std::path::Path, online: net::Online) -> App {
        round_over(dir, online, true)
    }

    /// A round whose tide comes in on the first frame, on a shelf of its
    /// own, with the results card's keep key and the arena's teardown.
    /// `online` is the table it was played at, and `recorded` whether this
    /// machine has a recording of it.
    fn round_over(dir: &std::path::Path, online: net::Online, recorded: bool) -> App {
        round_over_with(dir, online, recorded, |_| {})
    }

    /// [`round_over`], with `prepare` let at the world before the round
    /// is played.
    fn round_over_with(
        dir: &std::path::Path,
        online: net::Online,
        recorded: bool,
        prepare: impl FnOnce(&mut App),
    ) -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.insert_state(Screen::Versus);
        app.insert_state(VersusPhase::Running);
        let mut board = classic_arena(false, 2);
        board.rules_mut().round_length = Some(0);
        board.set_score(1, 3);
        let mut replay = Replay::new(Level::from_board("Versus", 3, board.clone()));
        replay.ai[1] = Some(BotLevel::Hard);
        app.insert_resource(Sim(board));
        app.insert_resource(Recorder(recorded.then_some(replay)));
        app.insert_resource(Seats(2));
        app.init_resource::<Controllers>();
        let settings = settings::GameSettings::default();
        app.insert_resource(SeatNames(settings.names.clone()));
        app.insert_resource(settings);
        app.insert_resource(online);
        app.init_resource::<Playback>();
        app.init_resource::<replays::PlaybackSpeed>();
        app.init_resource::<Daily>();
        app.init_resource::<ReelThread>();
        app.init_resource::<Highlight>();
        app.init_resource::<replays::RoundToKeep>();
        app.insert_resource(replays::Library {
            dir: dir.to_path_buf(),
            ..replays::Library::default()
        });
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<keycaps::KeyCaps>();
        app.init_resource::<cursor::LiveSeatKeys>();
        app.init_resource::<pause::PauseMenu>();
        app.init_resource::<spectators::SpectatorChat>();
        app.init_resource::<PendingActions>();
        app.init_resource::<rematch::LocalTable>();
        app.init_resource::<match_setup::MatchConfig>();
        app.add_systems(
            Update,
            (
                check_versus_over.run_if(in_state(VersusPhase::Running)),
                replays::keep_input.run_if(in_state(VersusPhase::Over)),
                replays::keep_input.run_if(in_state(Screen::Interlude)),
            ),
        );
        app.add_systems(OnExit(Screen::Versus), end_versus);
        app.add_systems(OnExit(Screen::Interlude), replays::let_the_round_go);
        prepare(&mut app);
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<VersusPhase>>().get(),
            VersusPhase::Over,
            "the tide is in"
        );
        app
    }

    fn press_keep(app: &mut App) {
        let key = app
            .world()
            .resource::<keycaps::KeyCaps>()
            .key_for(replays::KEEP_CAP)
            .expect("a key");
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.clear();
        keys.press(key);
        app.update();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(key);
        keys.clear();
    }

    /// A shelf of its own, emptied first.
    fn scratch_shelf(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pinch-shelf-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A finished round goes on the shelf only when it is kept: nothing
    /// lands while the card is up, and one press files it once, under
    /// the name the round earned and with its whole recording, the AI's
    /// level line included.
    #[test]
    fn a_finished_round_is_shelved_only_when_kept() {
        let dir = scratch_shelf("kept");
        let mut app = finished_round(&dir, net::Online::default());
        for _ in 0..3 {
            app.update();
        }
        assert!(replays::shelf_in(&dir).is_empty(), "not by itself");
        assert_eq!(
            app.world().resource::<replays::RoundToKeep>().offer(),
            replays::KeepOffer::Offered
        );
        press_keep(&mut app);
        let shelf = replays::shelf_in(&dir);
        assert_eq!(shelf.len(), 1, "kept");
        assert_eq!(shelf[0].winner, "P2", "filed under who took it");
        let kept = Replay::parse(&std::fs::read_to_string(&shelf[0].path).expect("read"))
            .expect("a round");
        assert_eq!(kept.ai[1], Some(BotLevel::Hard), "with its AI's level");
        assert_eq!(kept.playback().scores()[1], 3);
        // And how it ended, as the results card showed it: the board the
        // tide came in on, and the row's points the card's first row.
        let card = *app.world().resource::<Sim>().0.scores();
        assert_eq!(kept.scores, Some(card), "the final board's scores");
        let mode = teams::TeamMode::Solo;
        assert_eq!(shelf[0].points, Some(teams::top_total(&card, 2, mode)));
        assert_eq!(shelf[0].points, Some(3));
        press_keep(&mut app);
        assert_eq!(replays::shelf_in(&dir).len(), 1, "and only once");
        assert_eq!(
            app.world().resource::<replays::RoundToKeep>().offer(),
            replays::KeepOffer::Kept
        );
    }

    /// A seat named with a trailing space, "Bo ", beside one named "Bo!",
    /// takes the round for the AI: it is kept under the name the shelf
    /// reads back from the recording, and the shelf tags the AI's row,
    /// not the person's whose name tidies to what an untrimmed one did.
    #[test]
    fn a_name_ending_in_a_space_is_kept_under_the_name_read_back() {
        let dir = scratch_shelf("spaced");
        let mut app = round_over_with(&dir, net::Online::default(), true, |app| {
            let names = ["Bo!", "Bo ", "", "", "", ""].map(String::from);
            app.world_mut()
                .resource_mut::<settings::GameSettings>()
                .names = names;
            app.world_mut().resource_mut::<Controllers>().0[1] = SeatController::Ai(BotLevel::Hard);
            app.init_resource::<SeatKinds>();
            // A table set up here, whose names are the saved rows'.
            app.world_mut()
                .resource_mut::<rematch::LocalTable>()
                .note_round(
                    true,
                    &match_setup::MatchConfig::default(),
                    &Default::default(),
                );
            app.add_systems(Startup, resolve_seat_names);
        });
        press_keep(&mut app);
        let shelf = replays::shelf_in(&dir);
        assert_eq!(shelf.len(), 1);
        assert_eq!(shelf[0].winner, "Bo", "filed as the recording names it");
        assert_eq!(
            shelf[0].won_by,
            Some(replays::WonBy::Ai(Some(BotLevel::Hard))),
            "and tagged as the AI that took it"
        );
    }

    /// Leaving the card to the menu, or straight back in for a rematch,
    /// lets the round go: nothing is shelved, then or after.
    #[test]
    fn leaving_the_card_lets_an_unkept_round_go() {
        for (way, to) in [("menu", Screen::Menu), ("rematch", Screen::Versus)] {
            let dir = scratch_shelf(way);
            let mut app = finished_round(&dir, net::Online::default());
            app.world_mut().resource_mut::<NextState<Screen>>().set(to);
            app.update();
            assert_eq!(
                app.world().resource::<replays::RoundToKeep>().offer(),
                replays::KeepOffer::Nothing,
                "{way}: let go"
            );
            press_keep(&mut app);
            assert!(replays::shelf_in(&dir).is_empty(), "{way}: nothing shelved");
        }
    }

    /// Leaving the card for the interlude before a series' next round
    /// keeps the offer standing there, and the key keeps it from there;
    /// the interlude left, for the next round or the menu, lets an unkept
    /// round go.
    #[test]
    fn the_interlude_holds_the_offer_until_it_is_left() {
        for (way, after) in [("next round", Screen::Versus), ("menu", Screen::Menu)] {
            let dir = scratch_shelf(&format!("interlude-{}", after == Screen::Menu));
            let mut app = finished_round(&dir, net::Online::default());
            let go = |app: &mut App, to: Screen| {
                app.world_mut().resource_mut::<NextState<Screen>>().set(to);
                app.update();
            };
            go(&mut app, Screen::Interlude);
            assert_eq!(
                app.world().resource::<replays::RoundToKeep>().offer(),
                replays::KeepOffer::Offered,
                "{way}: still on offer in the interlude"
            );
            go(&mut app, after);
            assert_eq!(
                app.world().resource::<replays::RoundToKeep>().offer(),
                replays::KeepOffer::Nothing,
                "{way}: let go as the interlude is left"
            );
            press_keep(&mut app);
            assert!(replays::shelf_in(&dir).is_empty(), "{way}: nothing shelved");
        }
    }

    /// Online, the host calls the next round of a series the moment its
    /// own card is up, which walks a joiner off its card on the spot. The
    /// joiner can still keep the round it just played, from the
    /// interlude: once, and that round rather than the one being called.
    #[test]
    fn a_joiner_walked_on_keeps_the_round_from_the_interlude() {
        use crate::app::tournament::{SeriesLength, Tournament};
        use crate::sim::{DEFAULT_DELAY, Lockstep};
        use crate::transport::{MatchTerms, UdpTransport};
        let dir = scratch_shelf("walked-on");
        let joiner = net::Online(Some(net::OnlineSession::new(
            UdpTransport::join(("127.0.0.1", 47998)).expect("socket"),
            Lockstep::new(1, vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        )));
        let mut app = round_over_with(&dir, joiner, true, |app| {
            app.insert_resource(Tournament::start(SeriesLength::BestOfThree));
            app.init_resource::<lobby::Homecoming>();
            app.init_resource::<Time>();
            app.init_resource::<crate::app::next_map::NextMap>();
            app.init_resource::<match_setup::MatchMenu>();
            app.add_systems(
                Update,
                play_input::versus_over_input.run_if(conditions::versus_over),
            );
        });
        let finished = match app.world().resource::<replays::RoundToKeep>() {
            replays::RoundToKeep::Offered(round) => round.text.clone(),
            other @ (replays::RoundToKeep::Nothing | replays::RoundToKeep::Kept) => {
                panic!("the round is offered: {other:?}")
            }
        };
        // The host's call lands while the joiner's card is up.
        app.world_mut()
            .resource_mut::<net::Online>()
            .0
            .as_mut()
            .expect("at the table")
            .next_round = true;
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Interlude,
            "walked on by the call"
        );
        press_keep(&mut app);
        press_keep(&mut app);
        let shelf = replays::shelf_in(&dir);
        assert_eq!(shelf.len(), 1, "kept, and only once");
        let kept = std::fs::read_to_string(&shelf[0].path).expect("read");
        assert_eq!(kept, finished, "the round just played");
        assert_eq!(
            app.world().resource::<replays::RoundToKeep>().offer(),
            replays::KeepOffer::Kept
        );
        // The next round starts: nothing of the last is left on offer.
        app.world_mut()
            .resource_mut::<NextState<Screen>>()
            .set(Screen::Versus);
        app.update();
        assert_eq!(
            app.world().resource::<replays::RoundToKeep>().offer(),
            replays::KeepOffer::Nothing
        );
        assert_eq!(replays::shelf_in(&dir).len(), 1);
    }

    /// Online each machine keeps the round for itself: the host keeping
    /// it shelves it on the host alone, and a joiner keeps its own copy
    /// or none, with nothing sent either way.
    #[test]
    fn host_and_joiner_each_keep_their_own() {
        use crate::sim::{DEFAULT_DELAY, Lockstep};
        use crate::transport::{MatchTerms, UdpTransport};
        let table = |seat: u8| {
            let transport = match seat {
                0 => UdpTransport::host(0).expect("socket"),
                _ => UdpTransport::join(("127.0.0.1", 47999)).expect("socket"),
            };
            net::Online(Some(net::OnlineSession::new(
                transport,
                Lockstep::new(seat, vec![0, 1], DEFAULT_DELAY),
                2,
                MatchTerms::default(),
            )))
        };
        let (host_dir, joiner_dir) = (scratch_shelf("host"), scratch_shelf("joiner"));
        let mut host = finished_round(&host_dir, table(0));
        let mut joiner = finished_round(&joiner_dir, table(1));
        assert!(
            host.world()
                .resource::<net::Online>()
                .0
                .as_ref()
                .expect("on")
                .is_host()
        );
        for app in [&host, &joiner] {
            assert_eq!(
                app.world().resource::<replays::RoundToKeep>().offer(),
                replays::KeepOffer::Offered,
                "both are offered it"
            );
        }
        press_keep(&mut host);
        joiner.update();
        assert_eq!(replays::shelf_in(&host_dir).len(), 1, "the host kept it");
        assert!(
            replays::shelf_in(&joiner_dir).is_empty(),
            "the joiner did not"
        );
        press_keep(&mut joiner);
        assert_eq!(replays::shelf_in(&joiner_dir).len(), 1, "until it does");
        assert_eq!(replays::shelf_in(&host_dir).len(), 1);
    }

    /// A round with no recording here, a replay watched or a round picked
    /// up from a code, has nothing to offer the shelf.
    #[test]
    fn a_round_with_no_recording_offers_nothing() {
        let dir = scratch_shelf("none");
        let mut app = round_over(&dir, net::Online::default(), false);
        assert_eq!(
            app.world().resource::<replays::RoundToKeep>().offer(),
            replays::KeepOffer::Nothing
        );
        press_keep(&mut app);
        assert!(replays::shelf_in(&dir).is_empty());
    }

    /// A round being recorded is stamped with what held each seat and,
    /// for the game's AI, at which level: the shelf names the AI that
    /// took a round by it.
    /// A couch round names each seat after the person partner picks put
    /// there, from the names saved in settings, which stay as typed; the
    /// daily's own table goes by the saved rows. A round picked up from a
    /// pasted code (or a dev hook's) is no table set up here, and its seats
    /// go by their labels, not by the last match's partner picks.
    #[test]
    fn a_couch_round_names_the_people_picked_into_each_seat() {
        let mut app = App::new();
        let mut saved = settings::GameSettings::default();
        saved.names[1] = "Bo".into();
        saved.names[2] = "Cleo".into();
        app.insert_resource(saved);
        let mut config = match_setup::MatchConfig::default();
        config.people.0.swap(1, 2);
        let mut table = rematch::LocalTable::default();
        table.note_round(true, &config, &Default::default());
        app.insert_resource(table);
        app.insert_resource(config);
        app.init_resource::<Daily>();
        app.init_resource::<net::Online>();
        app.init_resource::<Playback>();
        app.insert_resource(Sim(Board::new(5, 5, 0)));
        app.init_resource::<SeatNames>();
        app.init_resource::<SeatKinds>();
        app.insert_resource(Seats(4));
        app.init_resource::<Controllers>();
        app.insert_resource(Recorder(None));
        app.add_systems(Update, resolve_seat_names);
        app.update();
        assert_eq!(app.world().resource::<SeatNames>().0[1..3], ["Cleo", "Bo"]);
        app.world_mut().resource_mut::<Daily>().active = true;
        app.update();
        assert_eq!(app.world().resource::<SeatNames>().0[1..3], ["Bo", "Cleo"]);
        app.world_mut().resource_mut::<Daily>().active = false;
        let config = app.world().resource::<match_setup::MatchConfig>().clone();
        app.world_mut()
            .resource_mut::<rematch::LocalTable>()
            .note_round(false, &config, &Default::default());
        app.update();
        let names = app.world().resource::<SeatNames>();
        assert_eq!(names.0[1..3], ["", ""]);
        let tr = crate::app::i18n::Lang::En.tr();
        assert_eq!(names.label(tr, 1), "P2");
    }

    #[test]
    fn a_recorded_round_keeps_each_ai_level() {
        let mut app = App::new();
        app.init_resource::<settings::GameSettings>();
        app.init_resource::<rematch::LocalTable>();
        app.init_resource::<match_setup::MatchConfig>();
        app.init_resource::<Daily>();
        app.init_resource::<net::Online>();
        app.init_resource::<Playback>();
        app.insert_resource(Sim(Board::new(5, 5, 0)));
        app.init_resource::<SeatNames>();
        app.init_resource::<SeatKinds>();
        app.insert_resource(Seats(4));
        app.insert_resource(Controllers([
            SeatController::Local,
            SeatController::Ai(BotLevel::Hard),
            SeatController::Bot,
            SeatController::Ai(BotLevel::Easy),
            SeatController::Local,
            SeatController::Local,
        ]));
        app.insert_resource(Recorder(Some(Replay::new(Level::from_board(
            "Turf War",
            3,
            classic_arena(false, 4),
        )))));
        app.add_systems(Update, resolve_seat_names);
        app.update();
        let replay = app.world().resource::<Recorder>().0.clone().expect("kept");
        assert_eq!(
            replay.ai,
            [
                None,
                Some(BotLevel::Hard),
                None,
                Some(BotLevel::Easy),
                None,
                None
            ]
        );
        assert_eq!(replay.kinds[2], crate::sim::SeatKind::Bot);
        let back = Replay::parse(&replay.to_text()).expect("reads back");
        assert_eq!(back.ai, replay.ai, "and the file says so");
    }

    /// A round recorded as it is played holds one input a tick, and played
    /// back from its first board lands on the board the round left: the
    /// whole promise of the replay library, through the real tick system.
    #[test]
    fn a_recorded_round_plays_back_to_the_board_it_left() {
        let mut app = sim_app();
        app.world_mut().resource_mut::<Controllers>().0[0] = SeatController::Ai(BotLevel::Normal);
        app.world_mut().resource_mut::<Controllers>().0[1] = SeatController::Ai(BotLevel::Hard);
        let start = app.world().resource::<Sim>().0.clone();
        app.world_mut().resource_mut::<Recorder>().0 =
            Some(Replay::new(Level::from_board("Turf War", 3, start)));
        for _ in 0..40 {
            app.update();
        }
        let board = &app.world().resource::<Sim>().0;
        let replay = app
            .world()
            .resource::<Recorder>()
            .0
            .as_ref()
            .expect("recording");
        assert_eq!(board.ticks(), 40);
        assert_eq!(replay.inputs.len(), 40, "one input a tick");
        assert_eq!(
            replay.playback().state_hash(),
            board.state_hash(),
            "and played back, the same beach"
        );
    }

    /// The host's call for the next round can land on a joiner still a few
    /// frames short of the tide: the session is the new round's from then
    /// on, and the board is still the old one. Ticked on, the old board
    /// played the new round's first frames out, and the new board began at
    /// tick zero with the lockstep frames ahead of it: a desync for the
    /// whole of the next round. The board is left where it stood, nothing
    /// is committed against it, and the joiner is walked on to the round
    /// that was called.
    #[test]
    fn a_called_round_leaves_the_old_board_where_it_stood() {
        use crate::transport::{MatchTerms, UdpTransport};

        let mut app = sim_app();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::Versus));
        app.add_systems(Update, follow_the_called_round);
        // The last round, most of the way through on this machine.
        for _ in 0..50 {
            app.world_mut()
                .resource_mut::<Sim>()
                .0
                .tick(&[PlayerAction::None; MAX_PLAYERS]);
        }
        // And the next one taken up off the wire: a fresh lockstep, whose
        // first frames need nobody's input to run.
        let mut joiner = net::OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            crate::sim::Lockstep::new(1, vec![0, 1], crate::sim::DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        joiner.next_round = true;
        let commit = joiner.session.next_commit();
        app.insert_resource(net::Online(Some(joiner)));
        app.world_mut().resource_mut::<PendingActions>().0[1] = PlayerAction::Remove { x: 1, y: 1 };

        app.update();
        assert_eq!(
            app.world().resource::<Sim>().0.ticks(),
            50,
            "the old board played the new round's frames"
        );
        let online = app.world().resource::<net::Online>();
        let session = online.0.as_ref().expect("the session");
        assert_eq!(session.session.frame(), 0, "the new round is untouched");
        assert_eq!(
            session.session.next_commit(),
            commit,
            "a key meant for the old board went to the new one"
        );
        app.update();
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Interlude,
            "and on to the round that was called"
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

    /// The world `load_versus` runs in: every resource it reads or writes,
    /// a couch match of `seats` armed, scored as `team_mode` says.
    fn versus_app(seats: u8, team_mode: teams::TeamMode) -> App {
        use bevy::state::app::StatesPlugin;
        let mut app = App::new();
        app.add_plugins(StatesPlugin);
        app.init_state::<VersusPhase>();
        app.insert_resource(art::Art::blank());
        app.insert_resource(Sim(Board::new(4, 4, 0)));
        app.init_resource::<PendingActions>();
        app.init_resource::<Paused>();
        app.init_resource::<Seats>();
        app.init_resource::<Controllers>();
        app.init_resource::<Recorder>();
        app.init_resource::<countdown::Countdown>();
        app.init_resource::<countdown::FirstStep>();
        app.init_resource::<sim_events::BoardGeneration>();
        app.insert_resource(armed(seats, 0));
        app.init_resource::<match_setup::CustomBeaches>();
        app.init_resource::<Daily>();
        app.init_resource::<Resuming>();
        app.init_resource::<Playback>();
        app.init_resource::<net::Online>();
        app.init_resource::<rematch::LocalTable>();
        app.init_resource::<tournament::Tournament>();
        app.insert_resource(settings::GameSettings {
            team_mode,
            ..settings::GameSettings::default()
        });
        app.insert_resource(Sandbox(false));
        app
    }

    fn load(app: &mut App) {
        use bevy::ecs::system::RunSystemOnce;
        app.world_mut()
            .run_system_once(load_versus)
            .expect("loaded");
    }

    /// The phase `load_versus` asked for: the count, or straight to play.
    fn next_phase(app: &App) -> Option<VersusPhase> {
        match app.world().resource::<NextState<VersusPhase>>() {
            NextState::Pending(phase) | NextState::PendingIfDifferent(phase) => Some(*phase),
            NextState::Unchanged => None,
        }
    }

    /// `load_versus` is the one place a couch round learns its teams:
    /// scoring reads them off the board alone (`teams::on_board`), so a
    /// round set up here in pairs has to be stamped with them, the daily
    /// is everyone for themselves, and a board that brought its own (a
    /// pasted code) keeps them over this machine's setting.
    /// `teams::tests::a_pasted_round_is_scored_by_its_own_board` stamps
    /// its board by hand, so this is the guard on the stamp itself.
    #[test]
    fn a_round_is_stamped_with_its_teams_where_it_is_loaded() {
        use teams::TeamMode;
        // A couch match set up here in pairs plays in pairs, recorded and
        // counted in.
        let mut app = versus_app(4, TeamMode::Pairs);
        load(&mut app);
        assert_eq!(app.world().resource::<Seats>().0, 4);
        assert_eq!(
            app.world().resource::<Sim>().0.rules().teams,
            TeamMode::Pairs.sides(),
            "a couch round in pairs scored free for all"
        );
        assert!(app.world().resource::<Recorder>().0.is_some());
        assert_eq!(next_phase(&app), Some(VersusPhase::Countdown));

        // The daily is its own table, whatever the setting, and is played
        // on the day it was loaded.
        let mut app = versus_app(4, TeamMode::Pairs);
        app.world_mut().resource_mut::<Daily>().active = true;
        load(&mut app);
        assert_eq!(app.world().resource::<Sim>().0.rules().teams, None);
        assert_ne!(app.world().resource::<Daily>().day, 0, "the day kept");

        // A pasted round in pairs, picked up here with the setting on
        // solo: its own teams, no recording, and no count.
        let mut app = versus_app(4, TeamMode::Solo);
        let mut board = classic_arena(false, 4);
        board.rules_mut().teams = TeamMode::Pairs.sides();
        app.insert_resource(Resuming(Some(suspend::Suspended {
            seats: 4,
            bots: [None; MAX_PLAYERS],
            board,
        })));
        load(&mut app);
        assert_eq!(
            app.world().resource::<Sim>().0.rules().teams,
            TeamMode::Pairs.sides(),
            "the pasted board's teams were overwritten"
        );
        assert!(app.world().resource::<Recorder>().0.is_none());
        assert_eq!(next_phase(&app), Some(VersusPhase::Running));
        assert!(app.world().resource::<Resuming>().0.is_none(), "taken");

        // A recording being watched is not recorded again.
        let mut app = versus_app(4, TeamMode::Solo);
        app.insert_resource(Playback(Some((recorded(4), 0))));
        load(&mut app);
        assert!(app.world().resource::<Recorder>().0.is_none());
        assert_eq!(app.world().resource::<Seats>().0, 4);
    }

    /// The far end of a round hosted by [`online_hosted`]: seat 1's peer,
    /// a socket and a lockstep that say nothing until [`Joiner::answer`].
    struct Joiner {
        transport: crate::transport::UdpTransport,
        session: crate::sim::Lockstep,
    }

    impl Joiner {
        /// Take what the host sent, play every frame it can, commit up to
        /// its lead and send its tail back, as a joiner's tick does.
        fn answer(&mut self) {
            use crate::transport::{Batch, NetMsg};
            for (msg, _) in self.transport.recv_all() {
                if let NetMsg::Inputs(batch) = msg {
                    for input in batch.inputs {
                        self.session.receive(input);
                    }
                }
            }
            while self.session.advance().is_some() {}
            while self.session.commit_local(PlayerAction::None).is_some() {}
            let tail = Batch {
                inputs: self.session.recent_commits(),
                ..Batch::default()
            };
            self.transport.send_batch(&tail, |_| true);
        }
    }

    /// `advance_sim` hosting an online round at seat 0, on a beach whose
    /// wheel is on, against a joiner at seat 1 that has greeted and then
    /// says nothing. `tests/it/online.rs` drives a lockstep loop of its
    /// own; this is the game's.
    fn online_hosted() -> (App, Joiner) {
        use crate::transport::{NetMsg, UdpTransport};
        let mut app = sim_app();
        let mut board = classic_arena(false, 2);
        board.set_events_enabled(true);
        app.insert_resource(Sim(board));
        let session = online_at(2);
        let port = session.transport.local_addr().expect("bound").port();
        app.insert_resource(net::Online(Some(session)));
        let transport = UdpTransport::join(("127.0.0.1", port)).expect("join");
        transport.send(NetMsg::hello("Joiner"));
        let joiner = Joiner {
            transport,
            session: crate::sim::Lockstep::new(1, vec![0, 1], crate::sim::DEFAULT_DELAY),
        };
        (app, joiner)
    }

    fn hosted(app: &mut App) -> &mut net::OnlineSession {
        app.world_mut()
            .resource_mut::<net::Online>()
            .into_inner()
            .0
            .as_mut()
            .expect("hosting")
    }

    /// Tick the host until `done`, the joiner answering each tick, over
    /// a real socket that may take a moment.
    fn tick_until(app: &mut App, joiner: &mut Joiner, done: impl Fn(&mut App) -> bool) {
        for _ in 0..500 {
            joiner.answer();
            app.update();
            if done(app) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("never got there");
    }

    /// Tick the host, its joiner silent, until it has committed as far
    /// ahead as the lockstep lets it and waits on the table.
    fn stall(app: &mut App) {
        let mut last = None;
        for _ in 0..100 {
            app.update();
            let next = hosted(app).session.next_commit();
            if last == Some(next) {
                return;
            }
            last = Some(next);
        }
        panic!("never reached the commit lead");
    }

    /// A press made while the table waits on a slow peer is kept and sent
    /// when the round moves again, and one made during a pause is dropped:
    /// at the commit lead the press was thrown away and the player's arrow
    /// never landed, and a press queued under a pause fired on its resume.
    #[test]
    fn a_press_waits_out_a_stall_but_not_a_pause() {
        let (mut app, _joiner) = online_hosted();
        stall(&mut app);
        let place = PlayerAction::Place {
            x: 3,
            y: 3,
            dir: crate::sim::Direction::Up,
        };
        app.world_mut().resource_mut::<PendingActions>().0[0] = place;
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<PendingActions>().0[0],
            place,
            "a press dropped at the commit lead"
        );
        hosted(&mut app).session.request_pause();
        app.update();
        assert_eq!(
            app.world().resource::<PendingActions>().0[0],
            PlayerAction::None,
            "a press kept through a pause"
        );
    }

    /// The crowd's call waits out a stalled frame rather than being lost
    /// on it, leaves once it is committed, and the host tells the room it
    /// ran when its frame is played.
    #[test]
    fn a_crowd_call_waits_for_its_frame_and_is_announced() {
        use crate::sim::TideEvent;
        use crate::transport::Notice;
        let (mut app, mut joiner) = online_hosted();
        stall(&mut app);
        hosted(&mut app).stands.tide.pending_call = Some(TideEvent::FreshSand);
        app.update();
        assert_eq!(
            hosted(&mut app).stands.tide.pending_call,
            Some(TideEvent::FreshSand),
            "a call lost on a stalled frame"
        );
        assert!(hosted(&mut app).notices.is_empty());
        tick_until(&mut app, &mut joiner, |app| {
            hosted(app).stands.tide.pending_call.is_none()
        });
        tick_until(&mut app, &mut joiner, |app| !hosted(app).notices.is_empty());
        assert!(
            matches!(hosted(&mut app).notices[..], [Notice::Called { .. }]),
            "{:?}",
            hosted(&mut app).notices
        );
        assert!(
            app.world().resource::<Sim>().0.ticks() > u64::from(crate::sim::DEFAULT_DELAY),
            "and played on"
        );
    }

    /// A 4x3 puzzle stage with two arrows to spend, `goal` its goal.
    fn stage(goal: &str) -> Level {
        Level::parse(&format!(
            "name: Retry\nposts: 2\nround: 2\ngoal: {goal}\nmap:\n\
             +-+-+-+-+\n|. . . .|\n+ + + + +\n|. . . .|\n+ + + + +\n|. . . .|\n+-+-+-+-+\n"
        ))
        .expect("level")
    }

    /// The puzzle screen's reload with what it reads: the stage, the
    /// attempt kept on it, and two pairs of hands.
    fn puzzle_app() -> App {
        use bevy::state::app::StatesPlugin;
        let mut app = App::new();
        app.add_plugins(StatesPlugin);
        app.init_state::<Phase>();
        app.insert_resource(art::Art::blank());
        let level = stage("survive");
        app.insert_resource(Sim(level.board()));
        app.insert_resource(Campaign::tide_pool_of(vec![level]));
        app.init_resource::<PendingActions>();
        app.init_resource::<Paused>();
        app.init_resource::<sim_events::BoardGeneration>();
        app.init_resource::<achievements::PuzzleAttempt>();
        app.init_resource::<achievements::StagesRun>();
        app.init_resource::<progress::Progress>();
        app.add_message::<LoadLevel>();
        app.add_systems(
            Update,
            (achievements::track_puzzle_attempt, handle_load_level).chain(),
        );
        for seat in 0..2 {
            app.world_mut()
                .spawn((cursor::Cursor::seated(seat), Transform::default()));
        }
        app
    }

    fn reload(app: &mut App, keep_posts: bool) {
        app.world_mut().write_message(LoadLevel { keep_posts });
        app.update();
    }

    fn posts(app: &App) -> Vec<(u8, u8)> {
        let board = &app.world().resource::<Sim>().0;
        board
            .tiles()
            .filter(|&(x, y, _)| board.signpost_at(x, y).is_some())
            .map(|(x, y, _)| (x, y))
            .collect()
    }

    /// A retry puts back the arrows the run began with, a gull having
    /// finished one off mid-run or not; with no run noted it keeps what
    /// stands; a fresh load keeps nothing. And the hands are set down in
    /// the middle, the second a tile to the right of the first.
    #[test]
    fn a_retry_gives_back_the_arrows_the_run_began_with() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = puzzle_app();
        reload(&mut app, false);
        let began = [(0, 0), (3, 2)];
        {
            let mut sim = app.world_mut().resource_mut::<Sim>();
            for (x, y) in began {
                assert!(sim.0.place_signpost(0, x, y, crate::sim::Direction::Right));
            }
        }
        app.world_mut()
            .run_system_once(achievements::note_posts_spent)
            .expect("the run begins");
        // A gull takes one of them mid-run.
        assert!(
            app.world_mut()
                .resource_mut::<Sim>()
                .0
                .remove_signpost(0, 3, 2)
        );
        reload(&mut app, true);
        assert_eq!(posts(&app), began, "a retry short of an arrow");
        assert!(matches!(
            app.world().resource::<NextState<Phase>>(),
            NextState::Pending(Phase::Setup)
        ));

        reload(&mut app, false);
        assert_eq!(posts(&app), [], "a fresh load kept arrows");

        // No run noted on this stage: what stands is carried over.
        app.insert_resource(achievements::PuzzleAttempt::default());
        assert!(app.world_mut().resource_mut::<Sim>().0.place_signpost(
            0,
            1,
            1,
            crate::sim::Direction::Up
        ));
        reload(&mut app, true);
        assert_eq!(posts(&app), [(1, 1)]);

        let mut hands: Vec<(u8, u8, u8)> = app
            .world_mut()
            .query::<&cursor::Cursor>()
            .iter(app.world())
            .map(|cursor| (cursor.player, cursor.x, cursor.y))
            .collect();
        hands.sort_unstable();
        assert_eq!(hands, [(0, 2, 1), (1, 3, 1)]);
    }

    /// The run is called as the stage's goal reads the board: a survive
    /// stage that sees its round out is won, a bank stage that banks
    /// nothing in it is lost, and one still going is left to run.
    #[test]
    fn a_run_is_won_or_lost_as_its_goal_reads() {
        use bevy::ecs::system::RunSystemOnce;
        use bevy::state::app::StatesPlugin;
        for (goal, ticks, phase) in [
            ("survive", 0, None),
            ("survive", 2, Some(Phase::Won)),
            ("bank 1", 2, Some(Phase::Lost)),
        ] {
            let mut app = App::new();
            app.add_plugins(StatesPlugin);
            app.init_state::<Phase>();
            let level = stage(goal);
            let mut board = level.board();
            for _ in 0..ticks {
                board.tick_idle();
            }
            app.insert_resource(Sim(board));
            app.insert_resource(Campaign::tide_pool_of(vec![level]));
            app.world_mut()
                .run_system_once(check_outcome)
                .expect("checked");
            let next = match app.world().resource::<NextState<Phase>>() {
                NextState::Pending(phase) => Some(*phase),
                NextState::PendingIfDifferent(_) | NextState::Unchanged => None,
            };
            assert_eq!(next, phase, "{goal} after {ticks}");
        }
    }

    /// The host writes down whom the results card crowns, by the table's
    /// teams: a pairs round with scores 10, 0, 5 and 9 is the second
    /// pair's, both of its seats, and the same scores alone are seat 0's.
    #[test]
    fn the_host_notes_the_crowned_by_the_tables_teams() {
        use crate::transport::{MatchTerms, crowned_bits};
        use bevy::ecs::system::RunSystemOnce;
        for (mode, crowned) in [
            (
                teams::TeamMode::Pairs,
                [false, false, true, true, false, false],
            ),
            (
                teams::TeamMode::Solo,
                [true, false, false, false, false, false],
            ),
        ] {
            let mut app = App::new();
            let mut board = classic_arena(false, 4);
            for (seat, score) in [10, 0, 5, 9].into_iter().enumerate() {
                board.set_score(seat as u8, score);
            }
            app.insert_resource(Sim(board));
            app.insert_resource(Seats(4));
            app.init_resource::<settings::GameSettings>();
            app.init_resource::<Daily>();
            let session = net::OnlineSession::new(
                crate::transport::UdpTransport::host(0).expect("socket"),
                crate::sim::Lockstep::new(0, vec![0, 1, 2, 3], crate::sim::DEFAULT_DELAY),
                4,
                MatchTerms {
                    teams: mode.index() as u8,
                    ..MatchTerms::default()
                },
            );
            app.insert_resource(net::Online(Some(session)));
            app.world_mut()
                .run_system_once(note_the_crowned)
                .expect("noted");
            assert_eq!(hosted(&mut app).crowned, crowned_bits(&crowned), "{mode:?}");
        }
    }

    /// A recording is fed one frame per tick, or as many as the transport
    /// asks for. The speed is the whole of the fast-forward: there is no
    /// separate scrubbing path.
    #[test]
    fn the_transport_speed_is_how_many_frames_a_replay_eats() {
        for speed in [1u8, 2, 4] {
            let mut app = sim_app();
            let level = Level::from_board("Turf War", 3, classic_arena(false, 2));
            let mut replay = Replay::new(level);
            for _ in 0..40 {
                replay.record([PlayerAction::None; MAX_PLAYERS]);
            }
            app.world_mut().insert_resource(Playback(Some((replay, 0))));
            app.world_mut()
                .insert_resource(replays::PlaybackSpeed(speed));
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
        let level = Level::from_board("Turf War", 3, classic_arena(false, 2));
        let mut replay = Replay::new(level);
        replay.record([PlayerAction::None; MAX_PLAYERS]);
        app.world_mut().insert_resource(Playback(Some((replay, 0))));
        app.world_mut().insert_resource(replays::PlaybackSpeed(4));
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
