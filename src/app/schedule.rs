//! The frame, in order: what the app is built from, what each screen puts
//! up and takes down, and which systems run when.
//!
//! Split from `mod.rs`, which keeps the types the whole shell shares. The
//! two answer different questions, what a thing *is* and when it *runs*,
//! and only this half changes when a system moves.

use super::*;

pub fn run() {
    let mut app = App::new();
    embedded::register(&mut app);
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Pinch Points".into(),
                    resolution: dev::window_size().map_or_else(default, |(w, h)| {
                        bevy::window::WindowResolution::new(w as u32, h as u32)
                    }),
                    ..default()
                }),
                ..default()
            })
            .set(bevy::log::LogPlugin {
                // Japanese has no word spaces, so a line of it is broken
                // with a dictionary - and the one baked into the text
                // stack's segmenter does not carry the CJK model. The
                // lines still lay out (they break between characters,
                // which is how Japanese wraps anyway); what it does is
                // warn, from deep inside a layout pass, once per line
                // per frame. Sixty of those a second is not a report of
                // anything, so this one target is turned off.
                filter: format!(
                    "{},icu_provider=off",
                    bevy::log::LogPlugin::default().filter
                ),
                ..bevy::log::LogPlugin::default()
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                    // Debug builds default to the Vulkan validation
                    // layer, which floods the log with a known
                    // wgpu-internal swapchain finding
                    // (VUID-VkPresentInfoKHR-pImageIndices /
                    // acquire-semaphore) we cannot act on. Run with
                    // WGPU_VALIDATION=1 to opt back in when debugging
                    // rendering.
                    instance_flags: InstanceFlags::empty().with_env(),
                    ..WgpuSettings::default()
                })),
                ..RenderPlugin::default()
            }),
    );
    insert_resources(&mut app);
    add_startup(&mut app);
    add_screen_transitions(&mut app);
    add_phase_transitions(&mut app);
    add_frame_systems(&mut app);
    // Dev hook: `PINCH_ST_EXEC=1` swaps every main-world schedule to the
    // single-threaded executor, for the backlog's CPU measurement: the
    // engine spends more coordinating this game's hundred small systems
    // than the systems spend working. The render sub-app keeps its own
    // schedules and its parallelism. `new()` rather than `default()`: the
    // derived default leaves `apply_final_deferred` false, and the app
    // dies on the first frame missing every command-queued resource.
    if dev::single_threaded_executor() {
        use bevy::ecs::schedule::{Schedules, SingleThreadedExecutor};
        let mut schedules = app.world_mut().resource_mut::<Schedules>();
        let swapped = schedules
            .iter_mut()
            .map(|(_, schedule)| schedule.set_executor(SingleThreadedExecutor::new()))
            .count();
        info!("single-threaded executor on {swapped} main-world schedules");
    }
    app.run();
}

/// The resources, states and message types the whole shell shares.
fn insert_resources(app: &mut App) {
    app.insert_resource(Time::<Fixed>::from_hz(f64::from(
        crate::sim::TICKS_PER_SECOND,
    )));
    app.insert_resource(Sandbox(dev::sandbox()));
    let (levels, builtins) = campaign::tide_pool_levels();
    app.insert_resource(Campaign {
        kind: CampaignKind::TidePool,
        levels,
        index: 0,
        builtins,
    });
    // Replaced on mode entry; a 1x1 board just keeps the resource non-null.
    app.insert_resource(Sim(Board::new(1, 1, 0)));
    app.init_resource::<PendingActions>();
    app.init_resource::<Paused>();
    app.init_resource::<editor::EditorState>();
    app.init_resource::<Recorder>();
    app.init_resource::<Highlight>();
    app.init_resource::<ReelThread>();
    app.init_resource::<Bots>();
    app.init_resource::<art::Art>();
    app.init_resource::<match_setup::MatchConfig>();
    app.init_resource::<match_setup::CustomBeaches>();
    app.init_resource::<board_render::CastleFlight>();
    app.init_resource::<match_setup::MatchMenu>();
    app.init_resource::<Playback>();
    app.init_resource::<net::Online>();
    app.init_resource::<lobby::LobbyState>();
    app.init_resource::<lobby::Homecoming>();
    app.init_resource::<Seats>();
    app.init_resource::<Resuming>();
    app.init_resource::<RoundNotice>();
    app.init_resource::<menu_scene::MenuList>();
    app.init_resource::<stage_select::StageList>();
    app.init_resource::<hint::Hints>();
    app.init_resource::<hint::DeniedNote>();
    app.init_resource::<replays::Library>();
    app.init_resource::<replays::PlaybackSpeed>();
    app.init_resource::<gamepad::PadSeats>();
    app.init_resource::<effects::VisualRng>();
    app.init_resource::<effects::Trauma>();
    app.init_resource::<pause::PauseMenu>();
    app.init_resource::<audio::Muted>();
    app.init_resource::<Daily>();
    app.init_resource::<SeatNames>();
    app.init_resource::<tournament::Tournament>();
    app.init_resource::<side_panels::EventLog>();
    app.init_resource::<announce::Announcer>();
    app.init_resource::<update::UpdateCheck>();
    // A first run has no settings file, and opens on the language picker
    // rather than on a menu written in a language nobody chose. One read
    // answers both questions: what the settings are, and whether there
    // were any. A dev hook still wins - `dev::kickoff` sets NextState in
    // Startup, which lands before the first state transition.
    let saved = settings::GameSettings::load_saved();
    let opens_on = language::opening_screen(saved.is_some());
    // Two resources off one read: the preferences, and the learned caps
    // table that shares their file (see [`keycaps::KeyCaps`]).
    let (settings, caps) = saved.unwrap_or_default();
    app.insert_resource(settings);
    app.insert_resource(caps);
    app.init_resource::<settings::screen::SettingsMenu>();
    app.init_resource::<controls::ControlsMenu>();
    app.insert_state(opens_on);
    app.init_state::<Phase>();
    app.init_state::<VersusPhase>();
    app.add_message::<LoadLevel>();
    app.add_message::<PlacementDenied>();
    app.add_message::<LevelSaved>();
    app.add_message::<CodeShared>();
    app.add_message::<CodeTaken>();
    app.add_message::<sim_events::SimEvent>();
    app.init_resource::<achievements::PuzzleAttempt>();
}

/// One-time setup: camera, font, HUD, sounds, saved stats, dev hooks.
fn add_startup(app: &mut App) {
    app.add_systems(
        Startup,
        (
            boot::setup_camera,
            boot::install_ui_font,
            hud::spawn_hud,
            audio::load_sounds,
            achievements::load,
            progress::load,
            dev::kickoff,
            // Off-thread; the menu polls for the answer.
            update::start_check,
        ),
    );
}

/// What each screen builds on entry and tears down on exit.
fn add_screen_transitions(app: &mut App) {
    app.add_systems(
        OnEnter(Screen::Puzzle),
        (
            // Before the first load, whose message this stage's retry count
            // is about to be measured against.
            achievements::reset_puzzle_attempt,
            cursor::spawn_puzzle_cursor,
            send_first_load,
        )
            .chain(),
    );
    app.add_systems(
        OnEnter(Screen::Achievements),
        achievements::enter_achievements,
    );
    app.add_systems(
        OnEnter(Screen::StageSelect),
        stage_select::enter_stage_select,
    );
    app.add_systems(
        OnExit(Screen::StageSelect),
        menu_ui::despawn_marked::<stage_select::StageSelectUi>,
    );
    app.add_systems(OnEnter(Screen::Language), language::enter_language);
    app.add_systems(OnEnter(Screen::NewVersion), update::enter_new_version);
    app.add_systems(OnExit(Screen::NewVersion), update::exit_new_version);
    app.add_systems(
        OnExit(Screen::Language),
        menu_ui::despawn_marked::<language::LanguageUi>,
    );
    app.add_systems(OnEnter(Screen::Replays), replays::enter_library);
    app.add_systems(
        OnExit(Screen::Replays),
        menu_ui::despawn_marked::<replays::LibraryUi>,
    );
    app.add_systems(
        OnExit(Screen::Achievements),
        menu_ui::despawn_marked::<achievements::AchievementsUi>,
    );
    app.add_systems(
        OnEnter(Screen::Versus),
        // Chained so the cursor spawn commands apply before load_versus
        // positions the cursors, and the side panels see the seat count.
        (
            // A local match on a handmade beach reads it off the shelf, so
            // the shelf has to be current before the board is built.
            match_setup::refresh_custom_beaches,
            cursor::spawn_versus_cursors,
            load_versus,
            resolve_seat_names,
            side_panels::spawn_side_panels,
            achievements::reset_round_scratch,
        )
            .chain(),
    );
    app.add_systems(
        OnEnter(Screen::Editor),
        (
            editor::enter_editor,
            editor::spawn_editor_ui,
            cursor::spawn_puzzle_cursor,
            cursor::center_cursors,
        )
            .chain(),
    );
    app.add_systems(
        OnEnter(Screen::Menu),
        (
            menu_scene::enter_menu,
            tournament::reset_on_menu,
            // Whatever kept a session alive between rounds, the menu is the
            // end of it: the socket closes and the beach comes off the air.
            |mut online: ResMut<net::Online>| online.0 = None,
        ),
    );
    app.add_systems(
        OnExit(Screen::Menu),
        (
            menu_ui::despawn_marked::<menu_scene::MenuArt>,
            despawn_board_sprites,
            // The notice is news about one round; it is read on the menu and
            // does not follow the player into the next thing they pick.
            |mut notice: ResMut<RoundNotice>| notice.0.clear(),
        ),
    );
    app.add_systems(
        OnEnter(Screen::Lobby),
        (match_setup::refresh_custom_beaches, lobby::enter_lobby),
    );
    app.add_systems(
        OnExit(Screen::Lobby),
        (lobby::exit_lobby, menu_ui::despawn_marked::<lobby::LobbyUi>),
    );
    app.add_systems(OnEnter(Screen::Settings), settings::screen::enter_settings);
    app.add_systems(
        OnExit(Screen::Settings),
        menu_ui::save_and_despawn::<settings::screen::SettingsUi>,
    );
    app.add_systems(OnEnter(Screen::Controls), controls::enter_controls);
    app.add_systems(
        OnExit(Screen::Controls),
        menu_ui::save_and_despawn::<controls::ControlsUi>,
    );
    app.add_systems(
        OnEnter(Screen::MatchSetup),
        (
            match_setup::refresh_custom_beaches,
            match_setup::enter_match_setup,
        ),
    );
    app.add_systems(
        OnExit(Screen::MatchSetup),
        menu_ui::save_and_despawn::<match_setup::MatchUi>,
    );
    app.add_systems(OnEnter(Screen::Interlude), tournament::enter_interlude);
    app.add_systems(
        OnExit(Screen::Interlude),
        menu_ui::despawn_marked::<tournament::InterludeUi>,
    );
    app.add_systems(
        OnExit(Screen::Editor),
        (
            menu_ui::despawn_marked::<cursor::Cursor>,
            menu_ui::despawn_marked::<cursor::PostGhost>,
            menu_ui::despawn_marked::<effects::Particle>,
            menu_ui::despawn_marked::<effects::Hop>,
            menu_ui::despawn_marked::<editor::EditorUi>,
            despawn_board_sprites,
            editor::exit_editor,
        ),
    );
    app.add_systems(
        OnExit(Screen::Puzzle),
        (
            menu_ui::despawn_marked::<cursor::Cursor>,
            menu_ui::despawn_marked::<cursor::PostGhost>,
            despawn_board_sprites,
            menu_ui::despawn_marked::<results::ResultsPanel>,
            menu_ui::despawn_marked::<effects::Particle>,
            menu_ui::despawn_marked::<effects::Hop>,
            announce::clear_announcements,
            hint::clear_hint_ghosts,
            pause::reset_pause,
            achievements::save_now,
        ),
    );
    app.add_systems(
        OnExit(Screen::Versus),
        (
            menu_ui::despawn_marked::<cursor::Cursor>,
            menu_ui::despawn_marked::<cursor::PostGhost>,
            despawn_board_sprites,
            end_versus,
            menu_ui::despawn_marked::<results::ResultsPanel>,
            menu_ui::despawn_marked::<side_panels::SidePanelRoot>,
            menu_ui::despawn_marked::<effects::Particle>,
            menu_ui::despawn_marked::<effects::Hop>,
            announce::clear_announcements,
            pause::reset_pause,
            achievements::save_now,
            |mut daily: ResMut<Daily>| daily.active = false,
        ),
    );
}

/// Round outcomes: the puzzle win/lose cards and the versus results.
fn add_phase_transitions(app: &mut App) {
    app.add_systems(
        OnEnter(Phase::Won),
        (
            audio::play_win.run_if(in_state(Screen::Puzzle)),
            results::spawn_puzzle_won.run_if(in_state(Screen::Puzzle)),
            progress::record_cleared.run_if(in_state(Screen::Puzzle)),
            // After the clear is recorded: the campaign-finished trophy asks
            // whether every stage is done, and the last one is this one.
            achievements::record_puzzle
                .run_if(in_state(Screen::Puzzle))
                .after(progress::record_cleared),
        ),
    );
    app.add_systems(
        OnExit(Phase::Won),
        menu_ui::despawn_marked::<results::ResultsPanel>,
    );
    app.add_systems(
        OnEnter(Phase::Lost),
        (
            audio::play_lose.run_if(in_state(Screen::Puzzle)),
            results::spawn_puzzle_lost.run_if(in_state(Screen::Puzzle)),
            hint::record_loss.run_if(in_state(Screen::Puzzle)),
        ),
    );
    app.add_systems(
        OnExit(Phase::Lost),
        menu_ui::despawn_marked::<results::ResultsPanel>,
    );
    // A banner raised in the last second of a round would otherwise sit on
    // top of the results card; the round is over, the news can go.
    app.add_systems(OnEnter(Phase::Won), announce::clear_announcements);
    app.add_systems(OnEnter(Phase::Lost), announce::clear_announcements);
    app.add_systems(OnEnter(VersusPhase::Over), announce::clear_announcements);
    app.add_systems(
        OnEnter(VersusPhase::Over),
        (
            tournament::record_series_round.run_if(in_state(Screen::Versus)),
            // The stats before the card: the card quotes today's daily
            // best, and it should be the best with this round counted.
            // A replay being watched counts for nothing.
            achievements::record_round.run_if(in_state(Screen::Versus).and_then(not_watching)),
            results::spawn_versus_results.run_if(in_state(Screen::Versus)),
        )
            .chain(),
    );
    app.add_systems(
        OnExit(VersusPhase::Over),
        menu_ui::despawn_marked::<results::ResultsPanel>,
    );
}

/// The frame, in order. Every `Update` system belongs to exactly one of
/// these, and the sets are chained, so the order is declared here instead
/// of being implied by a system's position in a hundred-line tuple.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Frame {
    /// Menus and the screens that only read input.
    Ui,
    /// Settings pushed out into Bevy: sink volume, UI scale, tick rate.
    Apply,
    /// Play input, level loading, and the round-outcome checks.
    Play,
    /// Sprites and particles catching up with the sim.
    Render,
    /// Consumers of the observed sim-event stream. After `Render` on
    /// purpose: the effects read the previous frame's events, as they
    /// always have.
    Events,
    /// HUD text, sidebars, results chrome.
    Chrome,
    /// Camera, audio, dev hooks: the last word on the frame.
    Finish,
}

/// The per-frame schedule: the fixed-timestep sim driver, and the `Update`
/// systems grouped into the [`Frame`] sets.
fn add_frame_systems(app: &mut App) {
    app.add_systems(FixedUpdate, advance_sim.run_if(sim_should_run));
    // A hosted beach keeps its beacon up for the whole match, not just the
    // lobby, so a player arriving late sees a game in progress with a chair
    // free rather than an empty network. In `Update` rather than beside the
    // sim, because it must go on saying so while the round is paused.
    app.add_systems(
        Update,
        (|time: Res<Time>, mut online: ResMut<net::Online>| {
            if let Some(session) = &mut online.0 {
                session.keep_announcing(time.delta_secs());
            }
        })
        .in_set(Frame::Play),
    );
    // Learns what the keys say before anything this frame spells one out:
    // the controls screen labelling a rebind, the prompt line naming the
    // move keys. After Bevy's own input pass, so the modifier check sees
    // this frame's Shift, not last frame's.
    app.add_systems(
        PreUpdate,
        keycaps::learn_keycaps.after(bevy::input::InputSystems),
    );
    // Asked once, before the first frame draws a legend: the platform
    // knows the whole board, including the punctuation nobody will press.
    // What it will not answer, the presses above go on answering.
    app.add_systems(Startup, keymap::read_keymap);
    app.configure_sets(
        Update,
        (
            Frame::Ui,
            Frame::Apply,
            Frame::Play,
            Frame::Render,
            Frame::Events,
            Frame::Chrome,
            Frame::Finish,
        )
            .chain(),
    );
    add_ui_systems(app);
    add_apply_systems(app);
    add_play_systems(app);
    add_render_systems(app);
    add_event_systems(app);
    add_chrome_systems(app);
    add_finish_systems(app);
}

/// Menu, lobby, settings, controls and match setup: the screens that only
/// read input and repaint their own rows.
fn add_ui_systems(app: &mut App) {
    app.add_systems(
        Update,
        (
            gamepad::pad_menu_bridge.run_if(
                in_state(Screen::Menu)
                    .or_else(in_state(Screen::Settings))
                    .or_else(in_state(Screen::Controls))
                    .or_else(in_state(Screen::MatchSetup))
                    .or_else(in_state(Screen::Lobby))
                    .or_else(in_state(Screen::StageSelect))
                    .or_else(in_state(Screen::Language))
                    .or_else(in_state(Screen::NewVersion))
                    .or_else(versus_over)
                    .or_else(
                        in_state(Screen::Puzzle)
                            .and_then(in_state(Phase::Won).or_else(in_state(Phase::Lost))),
                    ),
            ),
            // The check's answer is read on the menu, and a newer release
            // takes the menu to the page. Before the menu's own input, so
            // the two never both set the screen from one keypress.
            (
                update::poll_check,
                menu_scene::menu_input,
                menu_scene::update_menu_rows,
            )
                .chain()
                .run_if(in_state(Screen::Menu)),
            (update::new_version_input, update::update_new_version_rows)
                .chain()
                .run_if(in_state(Screen::NewVersion)),
            (
                lobby::discover,
                lobby::host_tick,
                lobby::join_tick,
                lobby::lobby_input,
                lobby::update_lobby_list,
                lobby::update_lobby_view,
                lobby::update_lobby_players,
                lobby::update_lobby_terms,
                lobby::update_lobby_beach_note,
                lobby::update_lobby_chat,
            )
                .chain()
                .run_if(in_state(Screen::Lobby)),
            (
                settings::screen::settings_input,
                settings::screen::update_settings_ui,
            )
                .chain()
                .run_if(in_state(Screen::Settings)),
            (controls::controls_input, controls::update_controls_ui)
                .chain()
                .run_if(in_state(Screen::Controls)),
            (
                stage_select::stage_select_input,
                stage_select::update_stage_tiles,
            )
                .chain()
                .run_if(in_state(Screen::StageSelect)),
            (replays::library_input, replays::update_library)
                .chain()
                .run_if(in_state(Screen::Replays)),
            (language::language_input, language::update_language_ui)
                .chain()
                .run_if(in_state(Screen::Language)),
            (replays::playback_speed_input, replays::playback_pause_input)
                .run_if(in_state(Screen::Versus)),
            (
                gamepad::pad_claim_seats,
                match_setup::match_setup_input,
                match_setup::update_match_ui,
                match_setup::update_match_pad_info,
            )
                .chain()
                .run_if(in_state(Screen::MatchSetup)),
        )
            .chain()
            .in_set(Frame::Ui),
    );
}

/// Settings reaching the engine, and the two small input reactions.
fn add_apply_systems(app: &mut App) {
    app.add_systems(
        Update,
        (
            cursor::flash_cursors,
            audio::play_denied,
            settings::apply_music_volume,
            settings::apply_accessibility,
            settings::apply_sim_speed,
        )
            .chain()
            .in_set(Frame::Apply),
    );
}

/// Play: level loading, cursors, the editor, pauses, and the checks that
/// end a round.
fn add_play_systems(app: &mut App) {
    app.add_systems(
        Update,
        (
            handle_load_level,
            // Nested: the two that a fresh level starts, and the outer
            // tuple is at Bevy's arity limit.
            (
                hint::reset_on_level,
                achievements::track_puzzle_attempt.run_if(in_state(Screen::Puzzle)),
            )
                .run_if(on_message::<LoadLevel>),
            cursor::move_cursor
                .run_if(not(in_state(Screen::Menu)).and_then(not(editor::editor_naming))),
            play_input::setup_input.run_if(puzzle_setup),
            dev::debug_autoplay.run_if(puzzle_setup),
            hint::hint_input.run_if(puzzle_setup.or_else(puzzle_done)),
            // One nested group so the tuple stays inside Bevy's arity limit
            // of 20, which this list was already sitting on. The chain still
            // runs them in written order, and that order matters: the note
            // is aged before it is raised, so a denial arriving this frame
            // survives to be drawn this frame.
            (
                hint::draw_hint.run_if(in_state(Screen::Puzzle)),
                hint::tick_denied_note,
                hint::note_denials,
            ),
            play_input::running_input.run_if(puzzle_running),
            play_input::done_input.run_if(puzzle_done),
            check_outcome.run_if(puzzle_running),
            play_input::versus_input.run_if(versus_running),
            suspend::copy_round_code.run_if(versus_running),
            dev::debug_net_probe.run_if(versus_running),
            (
                dev::debug_banner.run_if(versus_running.or_else(puzzle_running)),
                dev::debug_moments,
            ),
            // Paired into one slot: the list is at Bevy's twenty-element
            // limit. Both only do anything with their variable set.
            (dev::debug_tide, dev::debug_lure).run_if(versus_running.or_else(puzzle_running)),
            // A player who stops sending holds the whole table on one
            // frame. The host gives up on them after a while and hands the
            // seat to an AI; every peer does it on the host's word, so they
            // all do it on the same frame and stay in step. And when it is
            // the *host* that has gone, no word is coming and no seat can
            // be filled: the round is over and the joiners are told so
            // rather than left watching a still beach. Paired into tuple
            // slots because the list around it is at Bevy's limit.
            (
                check_versus_over,
                net::abandon_the_departed,
                net::leave_a_hostless_round,
            )
                .run_if(versus_running),
            // The sim is stopped on the results card, so the session has to
            // be drained by hand: this is when a latecomer greets to queue
            // and when the host's next-round invitation arrives. Chained
            // ahead of the input, so an invitation that lands this frame is
            // acted on this frame.
            // The host can vanish while the card is up as easily as during
            // a round, and the card is where a table sits longest. Last in
            // the chain, so its notice is the one that survives a frame
            // where the player was pressing Enter anyway.
            (
                net::poll_between_rounds,
                play_input::versus_over_input,
                net::leave_a_hostless_round,
            )
                .chain()
                .run_if(versus_over),
            // `editor_input` is what types the name, so it runs while naming;
            // the command keys do not, or the letters of a name would flip
            // wrap and start the solver, and Enter would begin a playtest.
            (
                editor::editor_input,
                editor::editor_commands.run_if(not(editor::editor_naming)),
            )
                .chain()
                .run_if(in_state(Screen::Editor).and_then(not(editor::editor_testing))),
            editor::editor_test_input
                .run_if(in_state(Screen::Editor).and_then(editor::editor_testing)),
            // Paired into one slot because the list around it is at Bevy's
            // twenty-element limit.
            (
                editor::poll_solver,
                editor::update_editor_palette,
                editor::rebuild_statics,
            )
                .run_if(in_state(Screen::Editor)),
        )
            .chain()
            .in_set(Frame::Play),
    );
    app.add_systems(
        Update,
        (
            gamepad::pad_move_cursor
                .run_if(not(in_state(Screen::Menu)).and_then(not(editor::editor_naming))),
            gamepad::pad_setup_input.run_if(puzzle_setup),
            gamepad::pad_versus_input.run_if(versus_running),
            (pause::pause_input, pause::update_pause_rows)
                .chain()
                .run_if(play_screens),
        )
            .chain()
            .in_set(Frame::Play)
            .after(editor::rebuild_statics),
    );
}

/// Everything that redraws the board from sim state.
fn add_render_systems(app: &mut App) {
    app.add_systems(
        Update,
        (
            board_render::sync_signposts,
            board_render::dress_signposts,
            board_render::sync_turnstiles,
            board_render::sync_castles,
            board_render::kick_castles,
            board_render::cheer_tier_ups,
            // After the kick, which also writes scale: a bank landing in
            // the same frame as a swap must not fight the flight.
            board_render::fly_castles,
            board_render::wave_pennants,
            creatures::sync_crab_sprites,
            creatures::sync_gull_sprites,
            creatures::interpolate_crabs,
            creatures::interpolate_gulls,
            // Grouped: the tide's two systems in one slot, because the
            // list around them is at Bevy's twenty-element limit.
            (
                board_render::update_waterline,
                board_render::update_water_foam,
            )
                .chain(),
            board_render::pulse_spawners,
            board_render::animate_turnstiles,
            board_render::drift_cloud_shadows,
            (
                board_render::start_tide_wash,
                board_render::advance_tide_wash,
            )
                .chain(),
            (cursor::glide_cursors, cursor::ghost_pending_posts).chain(),
            effects::moment_effects,
            effects::crab_trails,
        )
            .chain()
            .run_if(board_screens)
            .in_set(Frame::Render),
    );
}

/// The sim observer and its consumers, plus the ambient screens.
fn add_event_systems(app: &mut App) {
    app.add_systems(
        Update,
        (
            sim_events::observe_sim.run_if(board_screens),
            audio::play_events,
            gamepad::rumble_on_raid.run_if(play_screens.and_then(not_watching)),
            achievements::track_events.run_if(play_screens.and_then(not_watching)),
            (
                announce::collect_announcements,
                announce::drive_announcements,
            )
                .chain()
                .run_if(play_screens),
            achievements::record_level_built.run_if(in_state(Screen::Editor)),
            achievements::record_codes.run_if(in_state(Screen::Editor)),
            achievements::update_toasts,
            (
                achievements::achievements_input,
                achievements::update_shelf_scrollbar,
            )
                .chain()
                .run_if(in_state(Screen::Achievements)),
            tournament::interlude_tick.run_if(in_state(Screen::Interlude)),
            menu_scene::menu_ambience.run_if(postcard_screens),
            menu_scene::refit_shore.run_if(postcard_screens),
            menu_scene::tend_backdrop,
            menu_ui::dress_cards,
        )
            .chain()
            .in_set(Frame::Events),
    );
}

/// The readouts: header, sidebars, clocks, hints.
fn add_chrome_systems(app: &mut App) {
    app.add_systems(
        Update,
        (
            hud::update_hud,
            hud::header_backdrop,
            hud::field_guide_visibility,
            hud::update_field_guide,
            side_panels::update_side_panels,
            side_panels::update_side_clock.run_if(in_state(Screen::Versus)),
            side_panels::collect_log.run_if(in_state(Screen::Versus)),
            side_panels::update_log,
            hud::update_tide_clock,
            hud::update_hint,
            (replays::tend_replay_bar, replays::update_replay_bar).chain(),
            // The reel lands after the card is up; the card's line waits.
            (poll_reel, results::update_highlight_line)
                .chain()
                .run_if(versus_over),
        )
            .chain()
            .in_set(Frame::Chrome),
    );
}

/// Particles, camera, music, and the screenshot hook.
fn add_finish_systems(app: &mut App) {
    app.add_systems(
        Update,
        (
            effects::update_particles,
            effects::advance_hops,
            // Until it lands: the font it names is registered by a system
            // of Bevy's own, and is not there to be named before that has
            // run once.
            boot::teach_the_kanji_fallback,
            boot::fit_camera,
            // After the fit, which writes the camera's resting place: the
            // shake is an offset from there.
            boot::shake_camera,
            audio::toggle_mute.run_if(not(text_entry_open)),
            audio::drive_music,
            audio::rotate_music,
            audio::surge_tempo,
            dev::debug_screenshot,
        )
            .chain()
            .in_set(Frame::Finish),
    );
}

use conditions::*;
use session::*;
