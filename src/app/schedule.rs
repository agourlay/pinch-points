//! The frame, in order: what the app is built from, what each screen puts
//! up and takes down, and which systems run when.
//!
//! Split from `mod.rs`, which keeps the types the whole shell shares: only
//! this half changes when a system moves.

use super::*;

pub fn run() {
    run_with(watch::Watch::default());
}

/// The game, opened on whatever `watching` says to watch (usually nothing).
pub(super) fn run_with(watching: watch::Watch) {
    let mut app = App::new();
    app.insert_resource(watching);
    embedded::register(&mut app);
    // Read before the window is built, so a fullscreen player never sees a
    // window open and then grow. The same read seeds the resources below.
    let saved = settings::GameSettings::load_saved();
    let mode = settings::window_mode(
        saved
            .as_ref()
            .map_or(&settings::GameSettings::default(), |(settings, _)| settings),
    );
    app.add_plugins(trim_unused_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Pinch Points".into(),
                    resolution: dev::window_size().map_or_else(default, |(w, h)| {
                        bevy::window::WindowResolution::new(w as u32, h as u32)
                    }),
                    mode,
                    window_level: if dev::on_top() {
                        bevy::window::WindowLevel::AlwaysOnTop
                    } else {
                        default()
                    },
                    present_mode: dev::present_mode().unwrap_or_default(),
                    ..default()
                }),
                ..default()
            })
            .set(bevy::log::LogPlugin {
                // Japanese has no word spaces, so a line of it is broken
                // with a dictionary, and the one baked into the text
                // stack's segmenter does not carry the CJK model. The lines
                // still lay out (they break between characters, which is
                // how Japanese wraps anyway); what it does is warn once per
                // line per frame, so this one target is turned off.
                filter: format!(
                    "{},icu_provider=off",
                    bevy::log::LogPlugin::default().filter
                ),
                ..bevy::log::LogPlugin::default()
            })
            .set(TaskPoolPlugin {
                task_pool_options: TaskPoolOptions {
                    // Bevy caps the io and async-compute pools at four
                    // threads each and hands the compute pool everything
                    // left over, uncapped. On a 24-core machine that is
                    // sixteen workers for this game's hundred small
                    // systems, and a `perf` profile of a six-seat XL round
                    // is then mostly the workers finding each other: queue
                    // pops, work stealing and a contended mutex come to
                    // about 18% of samples.
                    //
                    // Swept on that machine with `PINCH_THREADS`, 30 s of
                    // that round each on the parallel executor, CPU
                    // seconds (user + sys):
                    //
                    //     cap  1     8.8     cap  4    13.3
                    //     cap  2     9.2     cap  6    15.9
                    //     cap  3    10.8     uncapped  21.2
                    //
                    // Nothing else moves: every one of those runs holds the
                    // fixed tick exactly, and the round clock reads 2:30 in
                    // all of them.
                    //
                    // One, now that the main world runs single-threaded
                    // (below) and the pool works only for the render world
                    // and the engine's parallel queries. CPU time a frame
                    // and the uncapped frame time, cap 2 against cap 1:
                    //
                    //                          cap 2          cap 1
                    //   title screen, vsync    3.2 ms         2.5 ms
                    //   six-seat XL, vsync     4.7 ms         3.5 ms
                    //   six-seat XL, uncapped  2.26 ms frame  2.19 ms
                    //   same, four slow cores  2.90 ms frame  2.90 ms
                    //   lobby, four slow cores 2.86 ms frame  2.51 ms
                    //
                    // The second worker was kept for a slow laptop's heavy
                    // frame; on four of this laptop's 3.3 GHz cores pinned
                    // with `taskset` (a Steam Deck's count) it shortens no
                    // frame, the slow ones included (95th percentile 3.35
                    // and 3.36 ms on the XL round).
                    compute: TaskPoolThreadAssignmentPolicy {
                        // `PINCH_THREADS=n` sweeps the cap, and `=0` puts
                        // Bevy's own default back, which is how the table
                        // above was measured and how to re-measure it on a
                        // machine that is not this one.
                        max_threads: dev::compute_threads().unwrap_or(1),
                        ..TaskPoolOptions::default().compute
                    },
                    ..TaskPoolOptions::default()
                },
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
                // A pipeline is built the first time something needs it,
                // and built in the background by default, drawing nothing
                // meanwhile: the title screen opened on flat beige where
                // the sea goes, the first board on bare clear colour round
                // it, and turning anti-aliasing over could do it again on
                // the settings screen. Waiting the frame out instead costs
                // a hitch on that first use. (A shader file still loading
                // at launch is drawn with nothing either way: this is the
                // building, not the loading.)
                synchronous_pipeline_compilation: true,
                ..RenderPlugin::default()
            }),
    ));
    materials::install(&mut app);
    dev::install_profiling(&mut app);
    insert_resources(&mut app, saved);
    failsafe::install(&mut app);
    add_startup(&mut app);
    add_screen_transitions(&mut app);
    add_phase_transitions(&mut app);
    add_frame_systems(&mut app);
    // Every main-world schedule on the single-threaded executor; the render
    // sub-app keeps its own schedules and its parallelism. This game's main
    // world is a hundred-odd small systems, and handing each to a worker
    // costs more than the system does: a `perf` profile of the parallel
    // executor is mostly its queues, atomics and wake-ups. With pipelined
    // rendering the main world already runs beside the render world, so the
    // parallel one bought nothing back. Measured on release builds, CPU
    // time a frame (all threads) and the uncapped frame time:
    //
    //                            parallel       single-threaded
    //   title screen, vsync      3.6 ms         2.8 ms
    //   six-seat XL, vsync       5.2 ms         4.1 ms
    //   six-seat XL, uncapped    2.31 ms frame  2.12 ms frame
    //   same, on four slow cores 3.35 ms frame  2.90 ms (95th: 3.83, 3.32)
    //   same, on two slow cores  4.97 ms frame  3.27 ms (95th: 5.48, 4.43)
    //
    // The slow cores are this laptop's 3.3 GHz efficiency cores pinned with
    // `taskset`, four standing in for a Steam Deck: the frames a slow
    // machine takes are shorter, not longer, and so are the slow ones.
    //
    // `PINCH_MT_EXEC=1` puts the parallel executor back, to measure it
    // again. `new()` rather than `default()`: the derived default leaves
    // `apply_final_deferred` false, and the app dies on the first frame
    // missing every command-queued resource.
    if !dev::multi_threaded_executor() {
        use bevy::ecs::schedule::{Schedules, SingleThreadedExecutor};
        let mut schedules = app.world_mut().resource_mut::<Schedules>();
        for (_, schedule) in schedules.iter_mut() {
            schedule.set_executor(SingleThreadedExecutor::new());
        }
    }
    // Dev hook: `PINCH_SHUFFLE=<seed>` (with `--features shuffle`) shuffles
    // the unordered systems of every main-world schedule. See
    // `dev::shuffle_seed`.
    #[cfg(feature = "shuffle")]
    if let Some(seed) = dev::shuffle_seed() {
        use bevy::ecs::schedule::Schedules;
        let mut schedules = app.world_mut().resource_mut::<Schedules>();
        let shuffled = schedules
            .iter_mut()
            .map(|(_, schedule)| {
                let mut settings = schedule.get_build_settings();
                settings.shuffle_seed = Some(seed);
                schedule.set_build_settings(settings);
            })
            .count();
        info!("PINCH_SHUFFLE: {shuffled} main-world schedules shuffled with seed {seed}");
    }
    app.run();
}

/// Take out the default plugins this game draws nothing with: no 3D
/// (meshes, lights, glTF, scenes, animation), no gizmos, no post-process
/// or screen-space anti-aliasing passes (the only anti-aliasing here is
/// MSAA, which is the core pipeline's), and no ready-made UI widgets (every
/// card here is plain nodes read from the keyboard and pads).
///
/// Each one still adds its systems to every frame, in the main world and in
/// the render world, and a system that finds nothing to do still costs the
/// executor a slot: they came to a quarter of the CPU time a frame took on
/// the title screen and a fifth on a six-seat round. `PINCH_ALL_PLUGINS=1`
/// puts them back, for measuring what they cost.
fn trim_unused_plugins(plugins: bevy::app::PluginGroupBuilder) -> bevy::app::PluginGroupBuilder {
    if dev::all_plugins() {
        return plugins;
    }
    use bevy::ui_widgets::{
        ButtonPlugin, CheckboxPlugin, DialogPlugin, ListBoxPlugin, MenuPlugin, ModalDialogPlugin,
        RadioGroupPlugin, ScrollAreaPlugin, ScrollbarPlugin, SliderPlugin, TabPlugin,
        TextInputPlugin,
    };
    plugins
        .disable::<bevy::pbr::PbrPlugin>()
        .disable::<bevy::gltf::GltfPlugin>()
        .disable::<bevy::scene::ScenePlugin>()
        .disable::<bevy::world_serialization::WorldSerializationPlugin>()
        .disable::<AnimationPlugin>()
        .disable::<bevy::gizmos::GizmoPlugin>()
        .disable::<bevy::gizmos_render::GizmoRenderPlugin>()
        .disable::<bevy::anti_alias::AntiAliasPlugin>()
        .disable::<bevy::post_process::PostProcessPlugin>()
        .disable::<bevy::light::LightPlugin>()
        .disable::<ButtonPlugin>()
        .disable::<CheckboxPlugin>()
        .disable::<TextInputPlugin>()
        .disable::<ListBoxPlugin>()
        .disable::<MenuPlugin>()
        .disable::<DialogPlugin>()
        .disable::<ModalDialogPlugin>()
        .disable::<RadioGroupPlugin>()
        .disable::<ScrollAreaPlugin>()
        .disable::<ScrollbarPlugin>()
        .disable::<SliderPlugin>()
        .disable::<TabPlugin>()
}

/// The resources, states and message types the whole shell shares.
fn insert_resources(app: &mut App, saved: Option<(settings::GameSettings, keycaps::KeyCaps)>) {
    app.insert_resource(Time::<Fixed>::from_hz(f64::from(
        crate::sim::TICKS_PER_SECOND,
    )));
    app.insert_resource(Sandbox(dev::sandbox()));
    app.insert_resource(Coop(dev::coop()));
    app.insert_resource(campaign::stage_list());
    // Replaced on mode entry; a 1x1 board just keeps the resource non-null.
    app.insert_resource(Sim(Board::new(1, 1, 0)));
    app.init_resource::<PendingActions>();
    app.init_resource::<Paused>();
    app.init_resource::<editor::EditorState>();
    app.init_resource::<editor::EditorOpening>();
    app.init_resource::<Recorder>();
    app.init_resource::<awards::RoundTally>();
    app.init_resource::<Highlight>();
    app.init_resource::<ReelThread>();
    app.init_resource::<Controllers>();
    app.init_resource::<bot_seats::BotSeats>();
    app.init_resource::<art::Art>();
    app.init_resource::<match_setup::MatchConfig>();
    app.init_resource::<match_setup::CustomBeaches>();
    app.init_resource::<match_setup::BeachGone>();
    app.init_resource::<editor::SaveChecks>();
    app.init_resource::<board_render::CastleFlight>();
    app.init_resource::<sim_events::BoardGeneration>();
    app.init_resource::<match_setup::MatchMenu>();
    app.init_resource::<next_map::NextMap>();
    app.init_resource::<match_setup::MapPreviews>();
    match_setup::watch_preview_pipelines(app);
    app.init_resource::<Playback>();
    app.init_resource::<net::Online>();
    app.init_resource::<lobby::LobbyState>();
    app.init_resource::<lobby::Homecoming>();
    app.init_resource::<Seats>();
    app.init_resource::<Resuming>();
    app.init_resource::<RoundNotice>();
    app.init_resource::<menu_scene::MenuList>();
    app.init_resource::<menu_scene::QuitCard>();
    app.init_resource::<menu_scene::LevelCard>();
    app.init_resource::<stage_select::StageList>();
    app.init_resource::<hint::Hints>();
    app.init_resource::<hint::DeniedNote>();
    app.init_resource::<replays::Library>();
    app.init_resource::<replays::RoundToKeep>();
    app.init_resource::<replays::PlaybackSpeed>();
    app.init_resource::<round_state::RoundClock>();
    app.init_resource::<gamepad::PadSeats>();
    app.init_resource::<gamepad::Bridged>();
    app.init_resource::<gamepad::PadFace>();
    app.init_resource::<gamepad::LastDevice>();
    app.init_resource::<gamepad::LastPad>();
    app.init_resource::<gamepad::PadPick>();
    app.add_systems(Startup, gamepad::pad_prompts_from_env);
    app.init_resource::<effects::VisualRng>();
    app.init_resource::<effects::Trauma>();
    app.init_resource::<pause::PauseMenu>();
    app.init_resource::<countdown::Countdown>();
    app.init_resource::<spectators::SpectatorChat>();
    app.init_resource::<spectators::SpectatorCard>();
    app.init_resource::<spectators::PickCard>();
    app.init_resource::<audio::Muted>();
    app.init_resource::<Daily>();
    app.init_resource::<SeatNames>();
    app.init_resource::<SeatKinds>();
    app.init_resource::<tournament::Tournament>();
    app.init_resource::<rematch::LocalTable>();
    app.init_resource::<side_panels::EventLog>();
    app.init_resource::<announce::Announcer>();
    app.init_resource::<update::UpdateCheck>();
    // A first run has no settings file, and opens on the language picker
    // rather than on a menu written in a language nobody chose. One read
    // answers both questions: what the settings are, and whether there
    // were any. A dev hook still wins - `dev::kickoff` sets NextState in
    // Startup, which lands before the first state transition.
    let opens_on = language::opening_screen(saved.is_some());
    // The same answer kept as its own fact: a `watch` boot skips the
    // picker, and must not write the file that would answer it.
    app.insert_resource(language::LanguageUnasked(saved.is_none()));
    // Two resources off one read: the preferences, and the learned caps
    // table that shares their file (see [`keycaps::KeyCaps`]).
    let (settings, caps) = saved.unwrap_or_default();
    app.insert_resource(settings);
    app.insert_resource(caps);
    app.init_resource::<cursor::LiveSeatKeys>();
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
    app.add_message::<countdown::CountBeat>();
    app.init_resource::<achievements::PuzzleAttempt>();
    app.init_resource::<achievements::StagesRun>();
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
            dev::slow_motion,
            watch::begin,
            // Off-thread; the menu polls for the answer.
            update::start_check,
        ),
    );
}

/// A board one person plays alone follows the first pad, not a claim a
/// friend made for a couch match: the puzzles (co-op's two cursors share
/// one seat's arrows), the editor, and a versus round with one cursor
/// here, the daily among them. Read before the round's cursors are
/// spawned, while the source they are spawned from is still whole.
pub(super) fn add_claim_release(app: &mut App) {
    app.add_systems(OnEnter(Screen::Puzzle), gamepad::let_claims_go);
    app.add_systems(OnEnter(Screen::Editor), gamepad::let_claims_go);
    app.add_systems(
        OnEnter(Screen::Versus),
        gamepad::let_a_lone_players_claims_go.before(cursor::spawn_versus_cursors),
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
            hint::reset_on_entry,
            cursor::spawn_stage_cursors,
            send_first_load,
        )
            .chain(),
    );
    add_claim_release(app);
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
        (
            menu_ui::despawn_marked::<stage_select::StageSelectUi>,
            stage_select::exit_stage_select,
        ),
    );
    app.add_systems(OnEnter(Screen::Language), language::enter_language);
    app.add_systems(OnEnter(Screen::NewVersion), update::enter_new_version);
    app.add_systems(OnExit(Screen::NewVersion), update::exit_new_version);
    app.add_systems(
        OnExit(Screen::Language),
        menu_ui::despawn_marked::<language::LanguageUi>,
    );
    app.add_systems(
        OnEnter(Screen::Replays),
        (replays::enter_library, dev::debug_library_cursor).chain(),
    );
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
            // An arena being watched puts its next game on first, so the
            // cursors and the board both see it.
            watch::install,
            cursor::spawn_versus_cursors,
            load_versus,
            countdown::show_countdown,
            resolve_seat_names,
            // Bots' seats are named for their bots, over what was resolved.
            bot_seats::begin_round,
            side_panels::spawn_side_panels,
            match_setup::say_beach_gone,
            achievements::reset_round_scratch,
            awards::RoundTally::forget,
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
            bot_seats::close_door,
            gamepad::forget_the_pilot,
            // Whatever kept a session alive between rounds, the menu is the
            // end of it: the socket closes and the beach comes off the air.
            |mut online: ResMut<net::Online>| online.0 = None,
        ),
    );
    app.add_systems(
        OnExit(Screen::Menu),
        (
            menu_ui::despawn_marked::<menu_scene::MenuArt>,
            menu_scene::close_quit_card,
            menu_scene::close_level_card,
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
        (
            lobby::exit_lobby,
            menu_ui::despawn_marked::<lobby::LobbyUi>,
            menu_ui::despawn_marked::<match_setup::MatchUi>,
            match_setup::release_previews,
        ),
    );
    app.add_systems(
        OnEnter(Screen::Settings),
        (
            settings::screen::note_replay_cap,
            settings::screen::enter_settings,
        ),
    );
    app.add_systems(
        OnExit(Screen::Settings),
        (
            menu_ui::save_and_despawn::<settings::screen::SettingsUi>,
            settings::screen::apply_replay_cap,
        ),
    );
    app.add_systems(OnEnter(Screen::Controls), controls::enter_controls);
    app.add_systems(
        OnExit(Screen::Controls),
        (
            menu_ui::save_and_despawn::<controls::ControlsUi>,
            settings::screen::apply_replay_cap,
        ),
    );
    app.add_systems(
        OnEnter(Screen::MatchSetup),
        (
            match_setup::refresh_custom_beaches,
            match_setup::enter_match_setup,
        )
            .chain(),
    );
    app.add_systems(
        OnExit(Screen::MatchSetup),
        (
            menu_ui::save_and_despawn::<match_setup::MatchUi>,
            match_setup::leave_match_setup,
            match_setup::release_previews,
        ),
    );
    app.add_systems(OnEnter(Screen::Interlude), tournament::enter_interlude);
    app.add_systems(
        OnExit(Screen::Interlude),
        (
            menu_ui::despawn_marked::<tournament::InterludeUi>,
            replays::let_the_round_go,
        ),
    );
    app.add_systems(
        OnExit(Screen::Editor),
        (
            menu_ui::despawn_marked::<cursor::Cursor>,
            menu_ui::despawn_marked::<cursor::PostGhost>,
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
            announce::clear_announcements,
            hint::clear_hint_ghosts,
            pause::reset_pause,
            reset_puzzle_phase,
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
            bot_seats::end_round,
            menu_ui::despawn_marked::<results::ResultsPanel>,
            menu_ui::despawn_marked::<side_panels::SidePanelRoot>,
            menu_ui::despawn_marked::<countdown::CountdownText>,
            menu_ui::despawn_marked::<lineup::LineupUi>,
            next_map::put_the_page_away,
            menu_ui::despawn_marked::<match_setup::MatchUi>,
            match_setup::release_previews,
            announce::clear_announcements,
            pause::reset_pause,
            spectators::forget_spectating,
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
        OnEnter(Phase::Running),
        achievements::note_posts_spent.run_if(in_state(Screen::Puzzle)),
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
    clear_banners_between_runs(app);
    app.add_systems(
        OnEnter(VersusPhase::Over),
        (
            // Before the card, which a pause in flight would cover.
            net::drop_a_late_pause.run_if(in_state(Screen::Versus)),
            tournament::record_series_round.run_if(in_state(Screen::Versus)),
            // For the host's call for the next round to carry.
            note_the_crowned.run_if(in_state(Screen::Versus)),
            // The stats before the card: the card quotes today's daily
            // best, and it should be the best with this round counted.
            // A replay being watched counts for nothing.
            achievements::record_round.run_if(in_state(Screen::Versus).and_then(not_watching)),
            // A spectator's call, scored before the card that says how it went.
            spectators::score_the_call.run_if(in_state(Screen::Versus)),
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

/// When the banners go before the board does.
fn clear_banners_between_runs(app: &mut App) {
    // A banner raised in the last second of a round would otherwise sit on
    // top of the results card; the round is over, the news can go.
    app.add_systems(OnEnter(Phase::Won), announce::clear_announcements);
    app.add_systems(OnEnter(Phase::Lost), announce::clear_announcements);
    app.add_systems(OnEnter(VersusPhase::Over), announce::clear_announcements);
    // And a stage reloaded mid-run (R, N or P, `handle_load_level`) is
    // back at its setup on a fresh beach: news of the old run went on
    // fading in over the new one.
    app.add_systems(OnEnter(Phase::Setup), announce::clear_announcements);
}

/// The fixed-timestep sim driver: a tick while the game is playing, and
/// none past the one that decides a puzzle (`puzzle_undecided`).
fn add_sim_driver(app: &mut App) {
    app.init_resource::<countdown::FirstStep>();
    app.add_systems(
        FixedUpdate,
        (
            countdown::hold_the_opening_step,
            advance_sim.run_if(
                sim_should_run
                    .and_then(puzzle_undecided)
                    .and_then(countdown::past_the_opening_step),
            ),
            // An online round's network, from the host's call to its
            // first frame, when no sim tick carries it.
            net::listen_before_the_round.run_if(versus_before_the_round),
        )
            .chain(),
    );
}

/// What was asked of a round never entered is forgotten in the transition
/// that enters something else: see `forget_a_round_not_entered`.
fn add_round_forgetting(app: &mut App) {
    app.add_systems(
        StateTransition,
        forget_a_round_not_entered
            .after(bevy::state::state::StateTransitionSystems::EnterSchedules),
    );
}

/// The per-frame schedule: the fixed-timestep sim driver, and the `Update`
/// systems grouped into the [`Frame`] sets.
fn add_frame_systems(app: &mut App) {
    add_sim_driver(app);
    add_round_forgetting(app);
    app.add_systems(Update, watch::pump);
    app.add_systems(
        Update,
        dev::debug_delete
            .run_if(in_state(Screen::Replays))
            .before(replays::library_input),
    );
    app.add_systems(
        Update,
        dev::debug_lobby_cursor
            .run_if(in_state(Screen::Lobby))
            .after(lobby::lobby_input)
            .before(lobby::update_lobby_list),
    );
    app.add_systems(
        Update,
        dev::debug_keep.run_if(versus_over).before(replays::KeepKey),
    );
    app.add_systems(
        Update,
        dev::debug_editor_press
            .run_if(in_state(Screen::Editor))
            .before(editor::editor_input),
    );
    app.add_systems(
        Update,
        watch::dropped_replays
            .run_if(in_state(Screen::Menu).or_else(in_state(Screen::Replays)))
            // After the screens' own input, which it gives way to.
            .after(menu_scene::paste_round)
            .after(replays::library_input),
    );
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
    // Before any global letter is read: which seat keys it yields to. In
    // `Update`, after the frame's state transition, so it answers for the
    // screen and the cursors the readers see: from `PreUpdate` it was a
    // frame behind a screen change, and the first frame of a puzzle read a
    // seat key whose cap is a global letter as the global.
    app.add_systems(Update, cursor::track_live_seat_keys.before(Frame::Ui));
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
            (
                gamepad::keep_pad_order,
                // The face first: the bridge presses what its table says,
                // and the hands last used are not counted mid-round.
                gamepad::read_face,
                gamepad::track_last_device,
                // On every screen: the face's table decides what it presses,
                // which in a round is nothing.
                gamepad::pad_menu_bridge,
                // On every screen: a press lets go wherever its finger comes
                // off, which is often a screen the bridge does not run on.
                gamepad::pad_bridge_release,
            )
                .chain(),
            // The check's answer is read on the menu, and a newer release
            // takes the menu to the page. Before the menu's own input, so
            // the two never both set the screen from one keypress.
            (
                update::poll_check,
                menu_scene::menu_input,
                // After the menu's own input, which stands down while the
                // card is up, so its Esc is the card's and not a quit.
                menu_scene::answer_level_card,
                // After the menu's own choice, which it gives way to.
                menu_scene::paste_round,
                menu_scene::update_menu_rows,
                menu_scene::show_quit_card,
                menu_scene::show_level_card,
            )
                .chain()
                .run_if(in_state(Screen::Menu)),
            (update::new_version_input, update::update_new_version_rows)
                .chain()
                .run_if(in_state(Screen::NewVersion)),
            (
                lobby::fit_lobby_list,
                lobby::discover,
                lobby::host_tick,
                lobby::join_tick,
                lobby::lobby_input,
                lobby::update_lobby_list,
                menu_ui::scroll_bar::follow::<lobby::LobbyState>,
                lobby::update_lobby_view,
                lobby::update_lobby_players,
                lobby::update_lobby_robots,
                lobby::update_lobby_terms,
                lobby::update_lobby_beach_note,
                lobby::update_lobby_chat,
                // The host's map page, over the lobby while it picks.
                match_setup::update_host_map_ui,
                (match_setup::paint_map_step, match_setup::follow_map_bar),
                match_setup::retire_previews,
                match_setup::render_previews,
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
            (
                replays::library_input,
                replays::update_library,
                menu_ui::scroll_bar::follow::<replays::Library>,
            )
                .chain()
                .run_if(in_state(Screen::Replays)),
            (language::language_input, language::update_language_ui)
                .chain()
                .run_if(in_state(Screen::Language)),
            (replays::playback_speed_input, replays::playback_pause_input)
                .run_if(in_state(Screen::Versus).and_then(keys_are_free)),
            (
                spectators::spectator_chat_input.run_if(chat_keys_free),
                (
                    spectators::spectator_vote_input,
                    spectators::spectator_pick_input,
                    spectators::settle_spectator_vote,
                )
                    .chain()
                    .run_if(keys_are_free),
            )
                .chain()
                .run_if(in_state(Screen::Versus)),
            bot_seats::announce.run_if(in_state(Screen::Versus)),
            (
                gamepad::follow_the_setup_hands,
                gamepad::pad_claim_seats,
                match_setup::match_setup_input,
                match_setup::update_match_ui,
                (
                    match_setup::paint_game_step,
                    match_setup::paint_map_step,
                    match_setup::follow_map_bar,
                    match_setup::paint_players_step,
                    match_setup::paint_start_button,
                    match_setup::paint_teams,
                ),
                match_setup::retire_previews,
                match_setup::render_previews,
            )
                .chain()
                .run_if(in_state(Screen::MatchSetup)),
            // After the screens' own input, which stands down while a
            // doorway card is up, so the card's Esc is read once.
            (bot_seats::door_input, bot_seats::door_card)
                .chain()
                .run_if(in_state(Screen::MatchSetup).or_else(in_state(Screen::Lobby))),
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
            settings::toggle_fullscreen,
            settings::apply_window_mode,
        )
            .chain()
            .in_set(Frame::Apply),
    );
}

/// Play: level loading, cursors, the editor, pauses, and the checks that
/// end a round.
fn add_play_systems(app: &mut App) {
    // Off the bench too: a save whose check outlived the editor is
    // finished wherever the player went.
    app.add_systems(Update, editor::finish_orphaned_saves);
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
                .run_if(on_message::<LoadLevel>)
                .after(handle_load_level),
            cursor::move_cursor.run_if(board_cursor_free),
            (play_input::setup_input, play_input::browse_levels)
                .chain()
                .run_if(puzzle_setup.and_then(keys_are_free)),
            dev::debug_autoplay.run_if(puzzle_setup),
            hint::hint_input.run_if(puzzle_setup.or_else(puzzle_done).and_then(keys_are_free)),
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
            play_input::running_input.run_if(puzzle_running.and_then(keys_are_free)),
            // Under the pause card a pad's Start can open on the card,
            // and the card's South is the card's, not "next stage".
            play_input::done_input.run_if(puzzle_done.and_then(keys_are_free)),
            check_outcome.run_if(puzzle_running),
            play_input::versus_input.run_if(versus_running.and_then(keys_are_free)),
            // C is a letter in a spectator's line of chat.
            suspend::copy_round_code.run_if(
                versus_running
                    .and_then(keys_are_free)
                    .and_then(not(text_entry_open)),
            ),
            (
                dev::debug_net_probe,
                dev::debug_autopilot,
                dev::debug_spectator,
            )
                .run_if(versus_running),
            (
                dev::debug_banner.run_if(versus_running.or_else(puzzle_running)),
                dev::debug_moments,
            ),
            // Paired into one slot: the list is at Bevy's twenty-element
            // limit. Both only do anything with their variable set.
            (dev::debug_tide, dev::debug_lure).run_if(versus_running.or_else(puzzle_running)),
            // A player who stops sending holds the whole table on one
            // frame. The host gives up on them and hands the seat to an AI;
            // every peer does it on the host's word, so they all do it on
            // the same frame. When it is the *host* that has gone, no word
            // is coming and no seat can be filled, so the joiners are told
            // rather than left watching a still beach. And a joiner the
            // host's next round reaches before its own board is over goes
            // on to it. Paired into tuple slots because the list around it
            // is at Bevy's limit.
            (
                check_versus_over,
                // The round left behind counted before it is left.
                (achievements::record_called_round, follow_the_called_round).chain(),
                net::abandon_the_departed,
                net::leave_a_hostless_round,
                net::catch_up_again,
            )
                .run_if(versus_running),
            // The sim is stopped on the results card, so the session has to
            // be drained by hand: this is when a latecomer greets to queue
            // and when the host's next-round invitation arrives. Chained
            // ahead of the input, so an invitation that lands this frame is
            // acted on this frame.
            //
            // The host can vanish while the card is up as easily as during
            // a round, and the card is where a table sits longest, so that
            // watch goes last in the chain.
            (
                net::poll_between_rounds,
                // Before the card's door, so a keep and an Enter on one
                // frame keep the round on the way out.
                replays::keep_input.in_set(replays::KeepKey),
                play_input::versus_over_input,
                net::leave_a_hostless_round,
            )
                .chain()
                .run_if(versus_over),
            // `editor_input` is what types the name, so it runs while naming;
            // the command keys do not, or the letters of a name would flip
            // wrap and start the solver, and Enter would begin a playtest.
            // "My levels" has the keys the same way while it is up, and so
            // does a card, read first so the key that puts it away is not
            // also read as a brush (`editor::editor_card_input`).
            (
                editor::editor_card_input.run_if(editor::editor_on_card),
                editor::editor_input,
                editor::editor_levels_input.run_if(editor::editor_on_levels),
                editor::editor_commands.run_if(not(editor::editor_has_keys)),
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
                editor::update_settings_card,
                editor::rebuild_statics,
                editor::seat_test_bots,
                editor::seat_test_partner,
                editor::update_test_banner,
                editor::show_level_list,
                editor::show_editor_card,
            )
                .run_if(in_state(Screen::Editor)),
        )
            .chain()
            .in_set(Frame::Play),
    );
    app.add_systems(
        Update,
        (
            // The pad walks the card's rows with the same stick the board
            // cursor answers to, so it needs the gate as much as the keys.
            gamepad::pad_move_cursor.run_if(board_cursor_free),
            gamepad::pad_setup_input.run_if(puzzle_setup.and_then(keys_are_free)),
            gamepad::pad_versus_input.run_if(versus_running.and_then(keys_are_free)),
            pause_card_systems(),
        )
            .chain()
            .in_set(Frame::Play)
            .after(editor::rebuild_statics),
    );
    // A kick or a host gone, heard from the host's call to the first frame,
    // walks the player out then rather than once the round runs. After the
    // interlude's own tick, whose step into the round would win otherwise.
    app.add_systems(
        Update,
        net::leave_a_hostless_round
            .run_if(versus_before_the_round)
            .after(tournament::interlude_tick)
            .in_set(Frame::Events),
    );
}

/// The pause card's keys and rows, on the play screens, after the play
/// keys in the frame. Not over the host's map page between rounds, whose
/// Esc is the page's own: the page spends the key that puts it away
/// (`versus_over_input`), since by the time this condition is read the
/// page is down. Its own function so a test runs it as the game does.
pub(super) fn pause_card_systems()
-> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    (pause::pause_input, pause::update_pause_rows)
        .chain()
        .run_if(play_screens.and_then(next_map::no_page_up))
}

/// Everything that redraws the board from sim state.
fn add_render_systems(app: &mut App) {
    app.add_systems(
        Update,
        (
            // First, and paired for the twenty-element limit: every drawing
            // below that remembers last frame's board asks this which
            // board it is now.
            (
                sim_events::track_board,
                round_state::tick_round_clock,
                board_render::sync_signposts,
            )
                .chain(),
            board_render::dress_signposts,
            board_render::sync_turnstiles,
            board_render::sync_castles,
            board_render::kick_castles,
            board_render::cheer_tier_ups,
            // After the kick, which also writes scale: a bank landing in
            // the same frame as a swap must not fight the flight.
            (
                board_render::fly_castles,
                board_render::flinch_castles,
                board_render::steady_castle_shadows,
                board_render::crown_the_leader,
            )
                .chain(),
            board_render::wave_pennants,
            // Paired: the list is at Bevy's twenty-element limit.
            (creatures::sync_crab_sprites, creatures::draw_sweeps),
            creatures::sync_gull_sprites,
            creatures::interpolate_crabs,
            // Paired for the twenty-element limit: the gulls on the sand
            // and the ones flying in to it.
            (creatures::interpolate_gulls, creatures::fly_in_gulls),
            board_render::pulse_spawners,
            // Paired for the same twenty-element limit: pools and logs are
            // the terrain that moves.
            (board_render::animate_turnstiles, board_render::ripple_pools),
            board_render::drift_cloud_shadows,
            (cursor::glide_cursors, cursor::ghost_pending_posts).chain(),
            effects::moment_effects,
            // Paired for the twenty-element limit: both are the beach
            // answering what walks on it.
            (
                effects::crab_trails,
                effects::splash_ponds,
                effects::golden_call_rings,
            ),
        )
            .chain()
            .run_if(board_screens)
            .in_set(Frame::Render),
    );
    // On every screen: the sprites load while the menu is up, and the menu
    // draws them too.
    app.add_systems(Update, art::mipmap_sprites);
    // On every screen too: it is also what puts the prompt back to its own
    // size and wrapping once there is no legend under it.
    app.add_systems(Update, hud::fit_prompt);
}

/// The sim observer and its consumers, plus the ambient screens.
fn add_event_systems(app: &mut App) {
    app.add_systems(
        Update,
        (
            sim_events::observe_sim.run_if(board_screens),
            audio::play_events,
            (
                audio::play_count,
                audio::play_final_seconds.run_if(play_screens),
            ),
            gamepad::rumble_on_raid.run_if(play_screens.and_then(not_watching)),
            achievements::track_events.run_if(play_screens.and_then(not_watching)),
            (
                announce::collect_announcements,
                announce::drive_announcements,
            )
                .chain()
                .run_if(play_screens),
            achievements::record_level_built.run_if(in_state(Screen::Editor)),
            // On every screen: codes are copied from the stage list and
            // taken in on the menu's level card as well as the editor, and
            // a message no reader runs for is gone two frames later.
            achievements::record_codes,
            achievements::update_toasts,
            (
                achievements::achievements_input,
                achievements::update_shelf_scrollbar,
            )
                .chain()
                .run_if(in_state(Screen::Achievements)),
            // The round before the interlude can still be kept there.
            (
                replays::keep_input.in_set(replays::KeepKey),
                tournament::interlude_tick,
            )
                .chain()
                .run_if(in_state(Screen::Interlude)),
            menu_scene::menu_ambience.run_if(postcard_screens),
            menu_scene::refit_shore.run_if(postcard_screens),
            menu_scene::tend_backdrop,
            (menu_ui::dress_cards, art::fit_card_frame_mips),
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
            // Again, so a face that changed this frame is named this frame.
            (gamepad::read_face, hud::update_hud).chain(),
            (hud::header_backdrop, hud::fade_prompt),
            hud::field_guide_visibility,
            hud::update_field_guide,
            side_panels::update_side_panels,
            side_panels::update_arrow_dots.run_if(any_with_component::<side_panels::ArrowDot>),
            side_panels::update_side_clock.run_if(in_state(Screen::Versus)),
            (side_panels::collect_log, side_panels::collect_chat)
                .chain()
                .run_if(in_state(Screen::Versus)),
            (side_panels::update_log, side_panels::update_event_timer),
            side_panels::reach_the_board,
            hud::update_tide_clock,
            hud::update_hint,
            (
                countdown::run_countdown
                    .run_if(in_state(Screen::Versus).and_then(in_state(VersusPhase::Countdown))),
                countdown::draw_countdown.run_if(any_with_component::<countdown::CountdownText>),
                // The line-up before an online match's count, and its
                // picture of the map, drawn as the map cards' are.
                // And the host's map page between the rounds of a series,
                // drawn as the lobby's is.
                (
                    lineup::show_lineup,
                    lineup::lineup_picture,
                    match_setup::update_host_map_ui,
                    (match_setup::paint_map_step, match_setup::follow_map_bar),
                    match_setup::retire_previews,
                    match_setup::render_previews,
                )
                    .chain()
                    .run_if(in_state(Screen::Versus)),
            )
                .chain(),
            (replays::tend_replay_bar, replays::update_replay_bar).chain(),
            // The reel lands after the card is up; the card's line waits.
            (
                (poll_reel, results::update_highlight_line)
                    .chain()
                    .run_if(versus_over),
                (results::refresh_over_keys, results::swap_card_keys).chain(),
            ),
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
            boot::apply_antialiasing,
            boot::fit_camera,
            // The sea follows the view's bottom edge: after the fit, before
            // the shake, so it sits where the camera rests and shakes with
            // the world.
            board_render::lay_the_sea,
            // After the fit, which writes the camera's resting place: the
            // shake is an offset from there.
            boot::shake_camera,
            audio::toggle_mute.run_if(not(text_entry_open)),
            audio::drive_music,
            audio::rotate_music,
            audio::fade_music,
            dev::debug_screenshot,
        )
            .chain()
            .in_set(Frame::Finish),
    );
    app.add_systems(Update, dev::frame_times);
    // Last in the frame, so a panic caught anywhere in it is seen in it.
    app.add_systems(Last, failsafe::leave_a_broken_round);
    // Last, so an exit asked for anywhere in the frame is seen in it: the
    // app stops once the frame is over.
    app.add_systems(Last, achievements::save_on_exit);
}

use conditions::*;
use session::*;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every schedule the shell adds builds: no system ordered against one
    /// scheduled twice, no cycle. A build error is a panic on the first
    /// frame of the real game, which no test of a system on its own sees.
    #[test]
    fn every_schedule_builds() {
        use bevy::ecs::schedule::Schedules;
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::state::app::StatesPlugin,
            bevy::input::InputPlugin,
        ));
        add_startup(&mut app);
        add_screen_transitions(&mut app);
        add_phase_transitions(&mut app);
        add_frame_systems(&mut app);
        let world = app.world_mut();
        let mut schedules = world.remove_resource::<Schedules>().expect("schedules");
        for (label, schedule) in schedules.iter_mut() {
            if let Err(e) = schedule.initialize(world) {
                panic!("{label:?}: {e}");
            }
        }
    }

    /// A pasted code, a replay to watch and the daily wait for the arena
    /// beside the request for it. When another screen is entered instead,
    /// they go: left behind, the next way into the arena of any kind (an
    /// online launch among them) played the pasted board.
    #[test]
    fn a_round_asked_for_and_not_entered_is_forgotten() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_resource::<Resuming>();
        app.init_resource::<Playback>();
        app.init_resource::<Daily>();
        app.init_resource::<rematch::LocalTable>();
        app.init_resource::<match_setup::MatchConfig>();
        app.init_state::<VersusPhase>();
        add_round_forgetting(&mut app);
        app.update();
        let ask = |app: &mut App, screen: Screen| {
            let world = app.world_mut();
            world.resource_mut::<Resuming>().0 = Some(suspend::Suspended {
                seats: 2,
                bots: [None; MAX_PLAYERS],
                board: classic_arena(false, 2),
            });
            let replay = Replay::new(Level::from_board("T", 3, classic_arena(false, 2)));
            world.resource_mut::<Playback>().0 = Some((replay, 0));
            world.resource_mut::<Daily>().active = true;
            world.resource_mut::<NextState<Screen>>().set(screen);
            app.update();
            let world = app.world();
            (
                world.resource::<Resuming>().0.is_some(),
                world.resource::<Playback>().0.is_some(),
                world.resource::<Daily>().active,
            )
        };
        // The menu's own choice won the frame: the paste is dropped.
        assert_eq!(ask(&mut app, Screen::MatchSetup), (false, false, false));
        // Into the arena, they are kept for `load_versus` to take.
        assert_eq!(ask(&mut app, Screen::Versus), (true, true, true));
    }

    /// The sim driver stops on the tick that decides an untimed stage,
    /// whatever the frame covers: run as the fixed clock runs it, a step
    /// on a decided board plays no tick, and one on a board still open
    /// plays its tick. `a_decided_puzzle_takes_no_more_ticks` asks the
    /// condition; this asks the schedule that is meant to read it.
    #[test]
    fn the_sim_driver_plays_no_tick_past_a_decided_puzzle() {
        use crate::sim::{Level, PUZZLE_TICK_LIMIT};
        let level = Level::parse(
            "name: T\nposts: 0\ngoal: survive\nmap:\n+-+-+\n|. .|\n+ + +\n|. .|\n+-+-+\n",
        )
        .expect("level");
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.insert_state(Screen::Puzzle);
        app.insert_state(Phase::Running);
        app.init_state::<VersusPhase>();
        app.init_resource::<editor::EditorState>();
        app.insert_resource(Campaign {
            levels: vec![level.clone()],
            index: 0,
            builtins: 1,
            coop: 0,
        });
        app.init_resource::<PendingActions>();
        app.init_resource::<Paused>();
        app.init_resource::<net::Online>();
        app.init_resource::<Recorder>();
        app.init_resource::<Playback>();
        app.init_resource::<replays::PlaybackSpeed>();
        app.init_resource::<Controllers>();
        app.init_resource::<bot_seats::BotSeats>();
        app.init_resource::<awards::RoundTally>();
        app.init_resource::<Time<Fixed>>();
        add_sim_driver(&mut app);
        app.update();
        let mut board = level.board();
        for _ in 1..PUZZLE_TICK_LIMIT {
            board.tick_idle();
        }
        let ticks = |app: &App| app.world().resource::<Sim>().0.ticks();
        app.insert_resource(Sim(board));
        app.world_mut().run_schedule(FixedUpdate);
        assert_eq!(
            ticks(&app),
            PUZZLE_TICK_LIMIT,
            "the deciding tick is played"
        );
        app.world_mut().run_schedule(FixedUpdate);
        assert_eq!(ticks(&app), PUZZLE_TICK_LIMIT, "and nothing past it");
    }

    /// A round resumed from a code opens at once, on its saved tick: the
    /// first fixed step plays no tick and lets go of the time the clock
    /// banked while the board was built, and the next plays the saved
    /// tick. A round played on from there ticks every step.
    #[test]
    fn a_resumed_round_starts_at_its_saved_tick() {
        use std::time::Duration;
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.insert_state(Screen::Versus);
        app.insert_state(VersusPhase::Running);
        app.init_state::<Phase>();
        app.init_resource::<editor::EditorState>();
        app.insert_resource(Campaign {
            levels: Vec::new(),
            index: 0,
            builtins: 0,
            coop: 0,
        });
        app.init_resource::<PendingActions>();
        app.init_resource::<Paused>();
        app.init_resource::<net::Online>();
        app.init_resource::<Recorder>();
        app.init_resource::<Playback>();
        app.init_resource::<replays::PlaybackSpeed>();
        app.init_resource::<Controllers>();
        app.init_resource::<bot_seats::BotSeats>();
        app.init_resource::<awards::RoundTally>();
        app.init_resource::<Time<Fixed>>();
        add_sim_driver(&mut app);
        app.update();
        let mut board = classic_arena(false, 2);
        for _ in 0..300 {
            board.tick_idle();
        }
        app.insert_resource(Sim(board));
        app.insert_resource(countdown::FirstStep::waiting());
        // The long frame that built the board, banked by the fixed clock.
        app.world_mut()
            .resource_mut::<Time<Fixed>>()
            .accumulate_overstep(Duration::from_millis(70));
        let ticks = |app: &App| app.world().resource::<Sim>().0.ticks();
        app.world_mut().run_schedule(FixedUpdate);
        assert_eq!(ticks(&app), 300, "the first step plays no tick");
        assert_eq!(
            app.world().resource::<Time<Fixed>>().overstep(),
            Duration::ZERO,
            "and the build's time is let go"
        );
        app.world_mut().run_schedule(FixedUpdate);
        assert_eq!(ticks(&app), 301, "the saved tick is the next one played");
        app.world_mut().run_schedule(FixedUpdate);
        assert_eq!(ticks(&app), 302);
    }

    /// A stage reloaded mid-run is back at its setup, on a fresh beach,
    /// and the old run's news goes with the old beach.
    #[test]
    fn a_reload_mid_run_takes_the_banners_down() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Phase>();
        app.init_state::<VersusPhase>();
        app.init_resource::<announce::Announcer>();
        clear_banners_between_runs(&mut app);
        app.world_mut()
            .resource_mut::<NextState<Phase>>()
            .set(Phase::Running);
        app.update();
        app.world_mut()
            .resource_mut::<announce::Announcer>()
            .push(announce::Announcement::Surge);
        // `handle_load_level`'s way back to the setup.
        app.world_mut()
            .resource_mut::<NextState<Phase>>()
            .set(Phase::Setup);
        app.update();
        assert_eq!(app.world().resource::<announce::Announcer>().pending(), 0);
    }
}
