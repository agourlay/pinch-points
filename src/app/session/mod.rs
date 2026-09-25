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

/// Every category of entity rendered from board state, bundled so the two
/// teardown paths (screen exit and level reload) cannot drift apart: a
/// missed category left turnstile sprites probing a smaller board and
/// panicking in `tile_at`.
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
        app.init_resource::<Bots>();
        app.init_resource::<awards::RoundTally>();
        app.add_systems(Update, advance_sim);
        app
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
            winner_name(&Sim(board), &seats, settings, &online, &names)
        };

        let plain = settings::GameSettings::default();
        assert_eq!(named([0, 7, 0, 0, 0, 0], &plain), "P2");
        // Level scores are nobody's round, and the file has to say so
        // rather than crediting the lowest seat.
        assert_eq!(named([7, 7, 0, 0, 0, 0], &plain), "draw");
        assert_eq!(named([0, 0, 0, 0, 0, 0], &plain), "draw");

        // A renamed seat is filed under its name, since that is what the
        // player will look for on the shelf.
        let mut settings = settings::GameSettings::default();
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

    /// The daily's table is one human and three fierce rivals, built from
    /// its own config rather than the player's.
    #[test]
    fn the_daily_seats_one_human_against_three_hard_bots() {
        let daily = match_setup::MatchConfig::daily();
        assert!(daily.armed, "ready to launch as it is");
        let bots = bot_seats(&daily);
        assert_eq!(bots[0], None, "the player's chair");
        assert_eq!(
            &bots[1..4],
            &[Some(BotLevel::Hard); 3],
            "three fierce rivals"
        );
        assert!(bots[4..].iter().all(Option::is_none), "and no more chairs");
        let player = match_setup::MatchConfig::default();
        assert_ne!(player.seats, daily.seats, "the player's own is untouched");
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
        let resumed = crate::app::suspend::Suspended {
            seats: 3,
            bots: [None; MAX_PLAYERS],
            board: Board::new(4, 4, 1),
        };
        let origin = RoundOrigin::Resumed(Box::new(resumed));
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

    /// A seat given up on mid-round turns AI on the frame the table
    /// agreed on, and not one frame sooner.
    ///
    /// Found by killing a real joiner: every peer desynced within a second
    /// of the drop. `Bots` says which seats the AI holds and nothing about
    /// when it took them, so `fill_bot_actions` filled the chair from the
    /// instant the notice reached each shell. That instant is the host's
    /// own decision on the host and a datagram everywhere else, and a peer
    /// still working through frames it already had the departed player's
    /// inputs for wrote a bot's move over each of them. `PlayerAction::None`
    /// is what a player pressing nothing sends, so this was most of them.
    #[test]
    fn an_abandoned_seat_turns_ai_on_the_frame_the_table_agreed_on() {
        let bots = Bots([None, None, Some(BotLevel::Normal), None, None, None]);
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

    /// A round recorded as it is played holds one input a tick, and played
    /// back from its first board lands on the board the round left: the
    /// whole promise of the replay library, through the real tick system.
    #[test]
    fn a_recorded_round_plays_back_to_the_board_it_left() {
        let mut app = sim_app();
        app.world_mut().resource_mut::<Bots>().0[0] = Some(BotLevel::Normal);
        app.world_mut().resource_mut::<Bots>().0[1] = Some(BotLevel::Hard);
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
