//! Enter: the level on the bench played as its player will play it. A
//! stage is set up, run, won or lost, and retried with R from the arrows
//! it was run with; a beach is played live against the AI in every other
//! seat, under the terms a match on it gets by default. Esc goes back to
//! the bench as it was.

use super::{BoardSwap, EditorState, Mode, level_here, swap_board};
use crate::app::gamepad::scheme::Does;
use crate::app::i18n::{Tr, fill};
use crate::app::settings::{GameSettings, KeyLegend};
use crate::app::{Controllers, SeatController, Sim};
use crate::sim::{Board, BotLevel, Level, LevelKind, PuzzleOutcome, TICKS_PER_SECOND};
use bevy::prelude::*;

/// Where a playtest stands, which is what its keys mean.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TestPhase {
    /// A stage being set up: its arrows go down on a still board.
    Setup,
    /// A stage running.
    Running,
    /// A stage won and lost, held still where it was decided.
    Won,
    Lost,
    /// A beach being played against the AI, arrows going down live.
    Match,
    /// A beach whose round is over.
    MatchOver,
}

impl TestPhase {
    pub const ALL: [TestPhase; 6] = [
        TestPhase::Setup,
        TestPhase::Running,
        TestPhase::Won,
        TestPhase::Lost,
        TestPhase::Match,
        TestPhase::MatchOver,
    ];

    /// Whether the board ticks.
    pub fn runs(self) -> bool {
        match self {
            TestPhase::Running | TestPhase::Match => true,
            TestPhase::Setup | TestPhase::Won | TestPhase::Lost | TestPhase::MatchOver => false,
        }
    }
}

/// A playtest under way.
#[derive(Debug)]
pub struct Playtest {
    /// The board being edited, back on the sand when the test ends.
    bench: Board,
    /// The level played: the one the file will hold.
    pub(super) level: Level,
    /// The board the run began from: a stage's with its arrows set up, a
    /// beach's as the round opened. R puts it back.
    start: Board,
    pub phase: TestPhase,
    /// The seats at the beach's table; every one but P1's is the AI's.
    seats: u8,
    /// Set on the frame the test began, whose Enter is still just pressed
    /// when the test's keys are read and would run the stage at once.
    fresh: bool,
    /// Opened from the menu's level card (a pasted beach played there):
    /// ending it goes back to the menu, where the player came from, rather
    /// than onto a bench they never asked for and that then asked about
    /// unsaved work. The test's own, so it goes with the test however the
    /// test ends: kept on the bench, a test a fault ended left it set, and
    /// the next test begun from the bench went to the menu on Esc and
    /// threw the bench away without asking.
    from_menu: bool,
}

/// The AI's level in a playtest: the one a match sets up by default.
const TEST_BOT: BotLevel = BotLevel::Normal;

impl Playtest {
    /// The test of `level`, with `bench` put aside: a stage in its setup,
    /// a beach under a match's default terms at the largest table it
    /// seats ([`Level::fits`]: one its `seats:` line names, if it names
    /// any), already under way. A beach for four and six with castles for
    /// six and no table of six named sits four, as the map dial sits it;
    /// tested at six, it was a beach nobody plays.
    pub(super) fn of(bench: Board, level: Level) -> Playtest {
        let (start, seats, phase) = match level.kind {
            LevelKind::Puzzle => (level.board(), 1, TestPhase::Setup),
            LevelKind::Arena => {
                let castles = level.table_seats().unwrap_or(1);
                let config = crate::app::match_setup::MatchConfig::default();
                let table = (2..=castles).rev().find(|&seats| level.fits(seats));
                let (mut board, seats) = match table {
                    Some(seats) => (level.table_board(level.board().seed(), seats), seats),
                    None => (level.board(), castles),
                };
                crate::app::match_setup::apply_terms(
                    &mut board,
                    config.gulls,
                    config.round,
                    config.posts,
                );
                (board, seats, TestPhase::Match)
            }
        };
        Playtest {
            bench,
            level,
            start,
            phase,
            seats,
            fresh: true,
            from_menu: false,
        }
    }

    /// Who plays each seat: P1 here, and on a beach the AI everywhere
    /// else at the table.
    pub fn controllers(&self) -> Controllers {
        let mut table = Controllers::default();
        if self.level.kind == LevelKind::Arena {
            for seat in 1..usize::from(self.seats) {
                if let Some(slot) = table.0.get_mut(seat) {
                    *slot = SeatController::Ai(TEST_BOT);
                }
            }
        }
        table
    }

    /// Whether the board is still undecided, as a stage judges it: the
    /// tick that decides it is the last one played (see
    /// `conditions::puzzle_undecided`).
    pub fn undecided(&self, board: &Board) -> bool {
        self.level.kind == LevelKind::Arena || self.level.outcome(board) == PuzzleOutcome::Running
    }
}

/// Begin a playtest of the bench: the board being edited is put aside
/// whole, to come back on Esc, and the test runs on the level the file
/// will hold, the same one saving, sharing and the solver are handed.
/// That is the rule it will be played under (a stage's granted inventory
/// and no raids, a beach's versus rule) and the gulls as the file's seed
/// rolls them, which is not how they stand on a board that placed and
/// erased a few.
///
/// A check still running goes on, and its answer waits for the test to
/// end (`validate::poll_solver`): it is about the bench, which the test
/// leaves as it was, and a save may be waiting on it to file its way
/// through.
pub(super) fn start(sim: &mut Sim, state: &mut EditorState, swap: &mut BoardSwap) {
    let level = level_here(state, &sim.0, &state.name);
    let test = Playtest::of(sim.0.clone(), level);
    swap_board(
        test.start.clone(),
        sim,
        state,
        &mut swap.commands,
        &swap.sprites,
    );
    state.feedback.clear();
    state.mode = Mode::Testing(Box::new(test));
}

/// [`start`] as the editor opens from the menu's level card, before
/// anything is drawn: no sprite of the bench's is on the sand yet to take
/// away, and ending the test goes back to the menu.
pub(super) fn begin(sim: &mut Sim, state: &mut EditorState) {
    let level = level_here(state, &sim.0, &state.name);
    let mut test = Playtest::of(sim.0.clone(), level);
    test.from_menu = true;
    sim.0 = test.start.clone();
    state.dirty = true;
    state.mode = Mode::Testing(Box::new(test));
}

/// The key that runs a stage set up, besides Num Enter
/// (`menu_ui::enter`).
pub(super) const RUN_KEY: KeyCode = KeyCode::Enter;
/// The cap that sends the test back to the start of its run, as a stage's
/// own R does.
pub(super) const RETRY_CAP: char = 'R';
/// The key that ends the test.
pub(super) const END_KEY: KeyCode = KeyCode::Escape;

/// The playtest's keys. Each cursor's place and remove keys, as a puzzle
/// reads them, while a stage is set up or a beach is played; Enter runs a
/// stage; R (by its cap, as a stage reads it) goes back to the start of
/// the run; Esc ends the test. A pad does the same by the playtest's
/// table, read off the pad: a round, where a pressed key would be a
/// seat's.
pub fn editor_test_input(
    keyboard: crate::app::keycaps::Keyboard,
    (pads, seat_pads): (
        crate::app::gamepad::face::PadsOnFace,
        crate::app::gamepad::SeatPads,
    ),
    mut sim: ResMut<Sim>,
    settings: Res<GameSettings>,
    mut state: ResMut<EditorState>,
    swap: BoardSwap,
    mut next_screen: ResMut<NextState<crate::app::Screen>>,
) {
    let BoardSwap {
        mut commands,
        sprites,
        cursors,
    } = swap;
    let keys = &keyboard.keys;
    if !cursors.iter().any(|(c, _)| c.player == 0) {
        return;
    }
    if keys.just_pressed(END_KEY) || pads.pressed(Does::EndTest) {
        if let Some(from_menu) = end_playtest(&mut sim, &mut state, &mut commands, &sprites) {
            state.feedback = settings.tr().ed_back.into();
            if from_menu {
                next_screen.set(crate::app::Screen::Menu);
            }
        }
        return;
    }
    let Mode::Testing(test) = &mut state.mode else {
        return;
    };
    if std::mem::take(&mut test.fresh) {
        return;
    }
    // The run decided since the last frame: a stage won or lost, a
    // beach's round over.
    test.phase = match test.phase {
        TestPhase::Running => match test.level.outcome(&sim.0) {
            PuzzleOutcome::Running => TestPhase::Running,
            PuzzleOutcome::Won => TestPhase::Won,
            PuzzleOutcome::Lost => TestPhase::Lost,
        },
        TestPhase::Match if sim.0.round_over() => TestPhase::MatchOver,
        phase @ (TestPhase::Setup
        | TestPhase::Won
        | TestPhase::Lost
        | TestPhase::Match
        | TestPhase::MatchOver) => phase,
    };
    let placing = matches!(test.phase, TestPhase::Setup | TestPhase::Match);
    if placing {
        // Each cursor on its own seat's keys and pad, as a stage's setup
        // reads them (`play_input::setup_input`, `gamepad::pad_setup_input`):
        // with a co-op stage's partner seated ([`seat_test_partner`]) IJKL
        // is P2's and the one-hand preset stands down, and the pad that
        // moves a cursor is the pad that places for it.
        let hands: Vec<&crate::app::cursor::Cursor> = cursors.iter().map(|(c, _)| c).collect();
        let commit =
            settings.commit_in_play(crate::app::cursor::has_partner(hands.iter().copied()));
        let players: Vec<u8> = hands.iter().map(|c| c.player).collect();
        for hand in hands.iter().filter(|c| c.player < 2) {
            let seat = crate::app::cursor::stage_seat(&sim.0, hand);
            let map = crate::app::cursor::keymap(&settings, hand.player, commit);
            let pad = |does| seat_pads.pressed(&settings, &players, hand.player, does);
            for (key, dir) in map.places {
                if keys.just_pressed(key) || pad(Does::Place(dir)) {
                    let _ = sim.0.place_signpost(seat, hand.x, hand.y, dir);
                }
            }
            if keys.just_pressed(map.remove) || pad(Does::Remove) {
                let _ = sim.0.remove_signpost(seat, hand.x, hand.y);
            }
        }
    }
    let run = crate::app::menu_ui::enter(keys) || pads.pressed(Does::Start);
    if test.phase == TestPhase::Setup && run {
        test.start = sim.0.clone();
        test.phase = TestPhase::Running;
        return;
    }
    let retry = keyboard.global(RETRY_CAP) || pads.pressed(Does::Reset);
    if retry && test.phase != TestPhase::Setup {
        test.phase = match test.level.kind {
            LevelKind::Puzzle => TestPhase::Setup,
            LevelKind::Arena => TestPhase::Match,
        };
        let start = test.start.clone();
        swap_board(start, &mut sim, &mut state, &mut commands, &sprites);
    }
}

/// End a playtest, if one is running, with the board it was started from
/// back on the sand; and say whether one was, and if so whether it was
/// opened from the menu ([`Playtest::from_menu`]).
///
/// Asked before the mode is taken, not after: taking it puts the default
/// in its place, so a recovery in the middle of naming would otherwise
/// end the naming too.
pub(super) fn end_playtest(
    sim: &mut Sim,
    state: &mut EditorState,
    commands: &mut Commands,
    sprites: &crate::app::session::BoardSprites,
) -> Option<bool> {
    if !state.is_testing() {
        return None;
    }
    let Mode::Testing(test) = std::mem::take(&mut state.mode) else {
        return None;
    };
    swap_board(test.bench, sim, state, commands, sprites);
    Some(test.from_menu)
}

/// The second player in a co-op stage's playtest: a cursor for P2 while
/// the test is of a co-op stage, and none otherwise. Asked every frame of
/// the editor, as the AI's seats are, so the cursor cannot outlive the
/// test whichever way it ends.
pub fn seat_test_partner(
    mut commands: Commands,
    state: Res<EditorState>,
    art: Res<crate::app::art::Art>,
    cursors: Query<(Entity, &crate::app::cursor::Cursor)>,
) {
    let wanted = match &state.mode {
        Mode::Testing(test) => test.level.is_coop(),
        Mode::Painting
        | Mode::Naming { .. }
        | Mode::JustClosed
        | Mode::Levels(_)
        | Mode::Card(_) => false,
    };
    let partner = cursors.iter().find(|(_, c)| c.player == 1);
    match (wanted, partner) {
        (true, None) => crate::app::cursor::cursor_sprite(&mut commands, &art, 1),
        (false, Some((entity, _))) => commands.entity(entity).despawn(),
        (true, Some(_)) | (false, None) => {}
    }
}

/// The AI in a beach's playtest: the table the test sets, and nobody's
/// otherwise. Asked every frame of the editor, so the AI cannot outlive
/// the test, whichever way it ends.
pub fn seat_test_bots(state: Res<EditorState>, mut controllers: ResMut<Controllers>) {
    let table = match &state.mode {
        Mode::Testing(test) => test.controllers(),
        Mode::Painting
        | Mode::Naming { .. }
        | Mode::JustClosed
        | Mode::Levels(_)
        | Mode::Card(_) => Controllers::default(),
    };
    if *controllers != table {
        *controllers = table;
    }
}

/// The header's status during a playtest: a stage's arrows and its goal,
/// as the stage itself shows them, or a beach's scores and time left.
pub(crate) fn status(tr: &Tr, test: &Playtest, board: &Board) -> String {
    match test.level.kind {
        LevelKind::Puzzle => crate::app::hud::puzzle_status(tr, &test.level, board),
        LevelKind::Arena => {
            let mut parts: Vec<String> = (0..test.seats)
                .map(|seat| {
                    format!(
                        "{} {}",
                        crate::app::seat_label(tr, seat),
                        board.scores()[usize::from(seat)]
                    )
                })
                .collect();
            if let Some(left) = board.remaining_ticks() {
                let secs = left.div_ceil(u64::from(TICKS_PER_SECOND));
                parts.push(fill(tr.ed_secs, &[("s", &secs.to_string())]));
            }
            parts.join(" | ")
        }
    }
}

/// The keyboard's prompt during a playtest, its keys still to be spelled
/// (`editor::spell_editor_keys`). A co-op stage's setup names both
/// players' keys, read as [`editor_test_input`] reads them: with the
/// partner seated ([`seat_test_partner`]) the one-hand preset stands down
/// ([`GameSettings::commit_in_play`]), so the stock names hold unless a
/// key is rebound or P1's keys are mirrored ([`GameSettings::key_legend`]).
pub(crate) fn prompt(tr: &Tr, test: &Playtest, settings: &GameSettings) -> String {
    match test.phase {
        TestPhase::Setup if test.level.is_coop() => match settings.key_legend(true) {
            KeyLegend::Stock => crate::app::name_the_pair(tr, tr.ed_test_setup_coop),
            KeyLegend::Swapped => crate::app::name_the_pair(tr, tr.ed_test_setup_coop_swapped),
            KeyLegend::Own => tr.ed_test_setup_coop_custom.to_string(),
        },
        phase @ (TestPhase::Setup
        | TestPhase::Running
        | TestPhase::Won
        | TestPhase::Lost
        | TestPhase::Match
        | TestPhase::MatchOver) => solo_prompt(tr, phase).to_string(),
    }
}

/// [`prompt`] on every stage but a co-op one's setup.
fn solo_prompt(tr: &Tr, phase: TestPhase) -> &'static str {
    match phase {
        TestPhase::Setup => tr.ed_test_setup,
        TestPhase::Match => tr.ed_playtest_prompt,
        TestPhase::Running | TestPhase::Won | TestPhase::Lost | TestPhase::MatchOver => {
            tr.ed_test_retry
        }
    }
}

/// What the banner over a decided test says: the stage's own won and lost
/// headings, or who took the beach.
pub(crate) fn banner(tr: &Tr, test: &Playtest, board: &Board) -> Option<String> {
    let every_crab = test.level.goal.saves_every_crab();
    match test.phase {
        TestPhase::Setup | TestPhase::Running | TestPhase::Match => None,
        TestPhase::Won => Some(
            match every_crab {
                true => tr.all_safe,
                false => tr.goal_met,
            }
            .to_string(),
        ),
        TestPhase::Lost => Some(
            crate::app::results::loss_words(tr, test.level.goal)
                .0
                .into(),
        ),
        TestPhase::MatchOver => {
            let scores = &board.scores()[..usize::from(test.seats)];
            let best = scores.iter().copied().max().unwrap_or(0);
            let leaders: Vec<usize> = (0..scores.len()).filter(|&s| scores[s] == best).collect();
            Some(match leaders.as_slice() {
                [seat] => fill(tr.wins, &[("p", &crate::app::seat_label(tr, *seat as u8))]),
                _ => tr.replay_draw.to_uppercase(),
            })
        }
    }
}

/// The banner over a decided playtest.
#[derive(Component)]
pub struct TestBanner;

/// The banner's node, hidden until a test is decided. Part of the editor's
/// interface, spawned and torn down with it.
pub(super) fn spawn_banner(commands: &mut Commands) {
    commands
        .spawn((
            super::EditorUi,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(84.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|wrap| {
            wrap.spawn((
                TestBanner,
                Text::new(String::new()),
                crate::app::menu_ui::display_font(34.0),
                TextColor(crate::app::palette::GOLD),
                Node {
                    display: Display::None,
                    padding: UiRect::axes(Val::Px(22.0), Val::Px(8.0)),
                    border_radius: BorderRadius::all(Val::Px(14.0)),
                    ..default()
                },
                BackgroundColor(crate::app::palette::PILL_FILL),
            ));
        });
}

/// Say on the banner how a decided test went, and hide it otherwise.
pub fn update_test_banner(
    state: Res<EditorState>,
    sim: Res<Sim>,
    settings: Res<GameSettings>,
    mut banners: Query<(&mut Text, &mut Node), With<TestBanner>>,
) {
    let said = match &state.mode {
        Mode::Testing(test) => banner(settings.tr(), test, &sim.0),
        Mode::Painting
        | Mode::Naming { .. }
        | Mode::JustClosed
        | Mode::Levels(_)
        | Mode::Card(_) => None,
    };
    for (mut text, mut node) in &mut banners {
        crate::app::menu_ui::set_shown(&mut node, said.is_some());
        crate::app::menu_ui::set_text(&mut text, said.as_deref().unwrap_or_default());
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{bench_app, tap};
    use super::super::{Cursor, editor_commands, editor_input, editor_testing};
    use super::*;
    use crate::sim::{CrabKind, Direction, Handedness, TileKind};

    /// The editor with a cursor at `at` and its systems as the game runs
    /// them: the bench's keys, or the test's while one is under way.
    fn test_app(board: Board, at: (u8, u8)) -> App {
        let mut app = bench_app();
        app.insert_resource(Sim(board));
        app.world_mut().resource_mut::<EditorState>().posts = 2;
        let mut cursor = Cursor::seated(0);
        (cursor.x, cursor.y) = at;
        app.world_mut().spawn((cursor, Transform::default()));
        let editing = |state: Res<EditorState>| !state.is_testing();
        app.add_systems(
            Update,
            (
                (editor_input, editor_commands).chain().run_if(editing),
                editor_test_input.run_if(editor_testing),
                seat_test_bots,
            )
                .chain(),
        );
        app
    }

    fn phase(app: &App) -> Option<TestPhase> {
        app.world()
            .resource::<EditorState>()
            .test()
            .map(|test| test.phase)
    }

    /// A co-op stage's playtest places as the stage does: each cursor on
    /// its own seat's keys, with the one-hand preset set aside for the
    /// partner, so IJKL is P2's and never puts down one of P1's arrows.
    /// Its setup line names both players' keys.
    #[test]
    fn a_coop_playtest_places_each_hand_on_its_own_keys() {
        let mut board = Board::new(6, 5, 3);
        board.set_tile(4, 2, TileKind::Castle(0));
        board.spawn_crab(1, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        let mut app = test_app(board, (2, 2));
        app.world_mut().resource_mut::<GameSettings>().commit =
            crate::app::settings::CommitScheme::Ijkl;
        {
            let mut state = app.world_mut().resource_mut::<EditorState>();
            state.posts = 1;
            state.second_hand = Some(1);
        }
        let mut partner = Cursor::seated(1);
        (partner.x, partner.y) = (3, 2);
        app.world_mut().spawn((partner, Transform::default()));
        tap(&mut app, KeyCode::Enter);
        assert_eq!(phase(&app), Some(TestPhase::Setup));
        let posts = |app: &App, seat| app.world().resource::<Sim>().0.signpost_count(seat);
        tap(&mut app, KeyCode::KeyI);
        assert_eq!(posts(&app, 0), 0, "I is P2's key, not P1's place key");
        tap(&mut app, KeyCode::ArrowUp);
        assert_eq!(posts(&app, 0), 1, "P1 places on the arrows");
        tap(&mut app, KeyCode::Numpad8);
        assert_eq!(posts(&app, 1), 1, "P2 on the numpad, for P2");
        let state = app.world().resource::<EditorState>();
        let test = state.test().expect("a test");
        let tr = &crate::app::i18n::EN;
        let stock = GameSettings::default();
        assert_eq!(
            prompt(tr, test, &stock),
            crate::app::name_the_pair(tr, tr.ed_test_setup_coop)
        );
        let one_hand = app.world().resource::<GameSettings>();
        assert_eq!(
            prompt(tr, test, one_hand),
            crate::app::name_the_pair(tr, tr.ed_test_setup_coop),
            "the partner sets the one-hand preset aside"
        );
        let mut rebound = GameSettings::default();
        rebound.binds[1].set(crate::app::binds::Action::PlaceUp, KeyCode::Digit9);
        assert_eq!(prompt(tr, test, &rebound), tr.ed_test_setup_coop_custom);
    }

    /// A stage's playtest is the stage: set up on a still board, run with
    /// Enter, won where the stage would be, and R takes it back to its
    /// setup with the arrows it ran with; Esc gives the bench back.
    #[test]
    fn a_stage_is_set_up_run_won_and_retried() {
        let mut board = Board::new(6, 5, 3);
        board.set_tile(4, 2, TileKind::Castle(0));
        board.spawn_crab(1, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        let mut app = test_app(board, (2, 2));
        tap(&mut app, KeyCode::Enter);
        assert_eq!(phase(&app), Some(TestPhase::Setup), "not run at once");
        tap(&mut app, KeyCode::ArrowRight);
        let posts = |app: &App| app.world().resource::<Sim>().0.signpost_count(0);
        assert_eq!(posts(&app), 1, "an arrow set up");
        tap(&mut app, KeyCode::Enter);
        assert_eq!(phase(&app), Some(TestPhase::Running));
        assert!(app.world().resource::<EditorState>().test_running());
        // The fixed tick's part, played by hand.
        for _ in 0..600 {
            let mut sim = app.world_mut().resource_mut::<Sim>();
            let test_level = Level::from_board("x", 2, sim.0.clone());
            if test_level.outcome(&sim.0) != PuzzleOutcome::Running {
                break;
            }
            sim.0.tick_idle();
        }
        app.update();
        assert_eq!(phase(&app), Some(TestPhase::Won));
        assert!(!app.world().resource::<EditorState>().test_running());
        let banner = {
            let state = app.world().resource::<EditorState>();
            banner(
                &crate::app::i18n::EN,
                state.test().expect("a test"),
                &Board::new(1, 1, 0),
            )
        };
        assert_eq!(banner.as_deref(), Some(crate::app::i18n::EN.all_safe));
        tap(&mut app, KeyCode::KeyR);
        assert_eq!(phase(&app), Some(TestPhase::Setup), "back to the setup");
        assert_eq!(posts(&app), 1, "with the arrow it ran with");
        tap(&mut app, KeyCode::Escape);
        assert_eq!(phase(&app), None);
        assert_eq!(posts(&app), 0, "the bench as it was");
    }

    /// A check running when a playtest starts goes on, and its answer
    /// waits for the bench to be back: it is about the bench, and a save
    /// may be waiting on it. Dropped, the save it finished lost its
    /// solution; let in, its verdict took the test's line.
    #[test]
    fn a_playtest_holds_a_running_checks_answer() {
        use super::super::validate::{Validation, Verdict, poll_solver};
        let mut board = Board::new(6, 5, 3);
        board.set_tile(4, 2, TileKind::Castle(0));
        board.spawn_crab(1, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        let mut app = test_app(board, (2, 2));
        app.add_systems(Update, poll_solver);
        let found = vec![(2, 2, Direction::Right, 0)];
        app.world_mut().resource_mut::<EditorState>().solver = Some(Validation::pending());
        tap(&mut app, KeyCode::Enter);
        assert_eq!(phase(&app), Some(TestPhase::Setup));
        let answer = Verdict {
            outcome: crate::sim::SolveOutcome::Found(found.clone()),
            needs: None,
            shared: None,
            coop: false,
        };
        {
            let state = app.world().resource::<EditorState>();
            let check = state.solver.as_ref().expect("still running");
            *check.slot.lock().unwrap() = Some(answer);
        }
        app.update();
        let state = app.world().resource::<EditorState>();
        assert!(state.solver.is_some(), "the answer waits");
        assert!(state.solution.is_empty());
        tap(&mut app, KeyCode::Escape);
        let state = app.world().resource::<EditorState>();
        assert!(state.solver.is_none(), "taken once the bench is back");
        assert_eq!(state.solution, found);
    }

    /// A beach played from the menu's level card ends back at the menu;
    /// one tested from the bench ends at the bench.
    #[test]
    fn a_test_from_the_menu_ends_at_the_menu() {
        use crate::app::Screen;
        use bevy::ecs::system::RunSystemOnce;
        let mut board = Board::new(9, 7, 3);
        board.set_tile(0, 0, TileKind::Castle(0));
        board.set_tile(8, 6, TileKind::Castle(1));
        let level = Level::from_board("Menu Beach", 3, board.clone()).with_kind(LevelKind::Arena);
        let mut app = test_app(board, (2, 2));
        let screen = |app: &App| *app.world().resource::<State<Screen>>().get();
        for test in [true, false] {
            app.world_mut()
                .resource_mut::<NextState<Screen>>()
                .set(Screen::Editor);
            app.update();
            assert_eq!(screen(&app), Screen::Editor);
            app.world_mut()
                .resource_mut::<super::super::EditorOpening>()
                .0 = Some(super::super::Opening {
                level: level.clone(),
                path: None,
                test,
            });
            let _ = app.world_mut().run_system_once(super::super::enter_editor);
            if !test {
                tap(&mut app, KeyCode::Enter);
            }
            assert!(app.world().resource::<EditorState>().is_testing());
            tap(&mut app, KeyCode::Escape);
            let back = match test {
                true => Screen::Menu,
                false => Screen::Editor,
            };
            assert_eq!(screen(&app), back, "opened to test: {test}");
        }
    }

    /// A test from the menu that a fault ends leaves the author at the
    /// bench, and the next test begun there ends at the bench too, not at
    /// the menu with the bench thrown away.
    #[test]
    fn a_menu_test_a_fault_ends_does_not_send_the_next_to_the_menu() {
        use crate::app::Screen;
        use bevy::ecs::system::RunSystemOnce;
        let mut board = Board::new(9, 7, 3);
        board.set_tile(0, 0, TileKind::Castle(0));
        board.set_tile(8, 6, TileKind::Castle(1));
        let level = Level::from_board("Menu Beach", 3, board.clone()).with_kind(LevelKind::Arena);
        let mut app = test_app(board, (2, 2));
        let screen = |app: &App| *app.world().resource::<State<Screen>>().get();
        app.world_mut()
            .resource_mut::<NextState<Screen>>()
            .set(Screen::Editor);
        app.update();
        app.world_mut()
            .resource_mut::<super::super::EditorOpening>()
            .0 = Some(super::super::Opening {
            level,
            path: None,
            test: true,
        });
        let _ = app.world_mut().run_system_once(super::super::enter_editor);
        assert!(app.world().resource::<EditorState>().is_testing());
        let _ = app.world_mut().run_system_once(
            |mut sim: ResMut<Sim>,
             mut state: ResMut<EditorState>,
             mut commands: Commands,
             sprites: crate::app::session::BoardSprites| {
                super::super::recover(
                    &mut sim,
                    &mut state,
                    &mut commands,
                    &sprites,
                    &crate::app::i18n::EN,
                );
            },
        );
        assert!(!app.world().resource::<EditorState>().is_testing());
        tap(&mut app, KeyCode::Enter);
        assert!(app.world().resource::<EditorState>().is_testing());
        tap(&mut app, KeyCode::Escape);
        app.update();
        assert_eq!(screen(&app), Screen::Editor, "back at the bench");
    }

    /// A beach whose `seats:` line leaves out the table its castles would
    /// seat is tested at the largest table it names, as the map dial
    /// seats it.
    #[test]
    fn a_beach_is_tested_at_the_largest_table_it_names() {
        let mut board = Board::new(9, 7, 3);
        for (seat, (x, y)) in [(0, 0), (8, 6), (8, 0), (0, 6), (4, 0), (4, 6)]
            .into_iter()
            .enumerate()
        {
            board.set_tile(x, y, TileKind::Castle(seat as u8));
        }
        let mut level = Level::from_board("Six", 3, board.clone()).with_kind(LevelKind::Arena);
        level.tables = vec![2, 4];
        assert!(!level.board().events_enabled(), "the file says nothing");
        let test = Playtest::of(board, level);
        assert!(
            test.start.events_enabled(),
            "the wheel turns, as in a real round"
        );
        assert_eq!(test.seats, 4);
        assert_eq!(
            test.start.table_seats(),
            Some(4),
            "castles 5 and 6 put away"
        );
    }

    /// A beach's playtest is a match under way at once, every seat but
    /// P1's the AI's, on a clock; the AI goes when the test does.
    #[test]
    fn a_beach_is_played_against_the_ai() {
        let mut board = Board::new(9, 7, 3);
        board.set_tile(0, 0, TileKind::Castle(0));
        board.set_tile(8, 6, TileKind::Castle(1));
        board.set_tile(8, 0, TileKind::Castle(2));
        board.spawn_crab(4, 3, Direction::Right, Handedness::Left, CrabKind::Common);
        let mut app = test_app(board, (2, 2));
        app.world_mut().resource_mut::<EditorState>().kind = LevelKind::Arena;
        tap(&mut app, KeyCode::Enter);
        assert_eq!(phase(&app), Some(TestPhase::Match));
        let controllers = *app.world().resource::<Controllers>();
        assert_eq!(controllers.0[0], SeatController::Local);
        assert_eq!(controllers.ai_count(), 2, "{controllers:?}");
        assert!(app.world().resource::<Sim>().0.remaining_ticks().is_some());
        tap(&mut app, KeyCode::Escape);
        assert_eq!(
            *app.world().resource::<Controllers>(),
            Controllers::default()
        );
    }
}
