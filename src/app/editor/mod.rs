//! Driftwood, the level editor (spec §5.4): author walls, castles, spawners,
//! rocks, crabs, and gulls on a live board; playtest in place; validate
//! solvability by brute-force against the headless sim; save to disk in the
//! text level format.

mod brush;
mod palette;
mod validate;

pub use brush::Brush;
pub use palette::{EditorUi, spawn_editor_ui, update_editor_palette};
pub use validate::poll_solver;

use brush::paint;
use validate::{SolverSlot, arena_report, orphan_warning, start_validation};

use crate::app::cursor::Cursor;
use crate::app::i18n::fill;
use crate::app::session::BoardSprites;
use crate::app::settings::GameSettings;
use crate::app::{Screen, Sim};
use crate::sim::{Board, Direction, Level, LevelKind};
use bevy::prelude::*;

const EDITOR_BOARD: (u8, u8) = (12, 9);

/// Beach sizes the editor offers, smallest first. The same set versus
/// plays on, so a level built here fits any of the arenas the game already
/// draws. Before this the editor had exactly one size, which made "a big
/// beach" something you could play on but not build.
const EDITOR_SIZES: [(u8, u8); 4] = [(9, 7), (12, 9), (16, 11), (20, 13)];
const GULL_PERIODS: [u32; 4] = [0, 480, 240, 120];
/// The editor's save slot, under the XDG data directory beside the
/// player's dropped-in levels.
/// Where the editor used to put its one and only level. Still read, so a
/// level saved by an older build is still there; nothing writes it now.
pub fn legacy_save_path() -> std::path::PathBuf {
    crate::app::paths::data_dir().join("levels/custom.txt")
}

/// The shelf every saved level goes on, one file each.
pub fn custom_dir() -> std::path::PathBuf {
    crate::app::paths::data_dir().join("levels/custom")
}

/// Where a level called `name` is filed. One file per name, so saving is
/// keeping rather than replacing: the editor wrote to a single fixed path
/// before this, and a second level overwrote the first.
///
/// The name also has to survive being a file name, and every level in the
/// campaign is identified by its name (that is what progress is keyed on),
/// so two levels sharing one would share their gold star.
pub fn save_path(name: &str) -> std::path::PathBuf {
    custom_dir().join(format!(
        "{}.txt",
        crate::app::paths::safe_stem(name, 40, "level")
    ))
}

#[derive(Resource, Default)]
pub struct EditorState {
    pub posts: u8,
    /// What is being built: a stage for the Tide Pool list, or a beach for
    /// the versus map dial. The author says which rather than the game
    /// counting castles and deciding for them.
    pub kind: LevelKind,
    /// What the level is called. It is the file name, and it is what the
    /// stage list shows and what progress is filed under, so two levels
    /// wanting their own gold star need two names.
    pub name: String,
    /// What the keys mean right now: see [`Mode`].
    pub mode: Mode,
    /// What the brush is loaded with. Painting is now a thing you choose
    /// and then do, rather than nine separate verbs.
    pub brush: Brush,
    pub gull_period_idx: usize,
    pub feedback: String,
    pub(super) solver: Option<SolverSlot>,
    /// Statics need a rebuild after a tile/wall edit.
    dirty: bool,
}

/// What the editor is doing, which is what its keys mean. One state
/// rather than a `naming` flag, a `named` flag and a `testing` snapshot,
/// three of which could once be set at the same time and none of which
/// were meant to be.
#[derive(Default, Debug)]
pub enum Mode {
    /// The board is being edited; keys are brushes and commands.
    #[default]
    Painting,
    /// The name is being typed; every letter is a letter, not a brush.
    Naming,
    /// The one frame after the name is committed. The Enter or Escape that
    /// ended it is still just-pressed when [`editor_commands`] runs later
    /// that frame, where it reads as "playtest" or "leave"; the schedule's
    /// naming gate cannot see this, since the mode is no longer `Naming`
    /// by then, so the commands sit that frame out on this instead.
    JustNamed,
    /// A playtest is running, on a copy; the board to restore when it ends.
    /// Boxed: a board is most of a kilobyte and the other modes are nothing.
    Testing(Box<Board>),
}

impl EditorState {
    pub fn is_naming(&self) -> bool {
        matches!(self.mode, Mode::Naming)
    }

    pub fn is_testing(&self) -> bool {
        matches!(self.mode, Mode::Testing(_))
    }
}

/// Type the level's name. Reads the text a keystroke produces rather than
/// the key code, so the player's own layout and their shift key decide
/// what a keystroke means.
fn type_a_name(
    typed: &mut MessageReader<bevy::input::keyboard::KeyboardInput>,
    keys: &ButtonInput<KeyCode>,
    state: &mut EditorState,
    tr: &crate::app::i18n::Tr,
) {
    use crate::app::typing::{Keystroke, keystrokes};
    let ends = [KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Escape];
    for stroke in keystrokes(typed, &ends) {
        match stroke {
            Keystroke::Erase => {
                state.name.pop();
            }
            // A name is one line and has to fit a file name and a stage
            // caption, so it is bounded here rather than at save.
            Keystroke::Char(ch) if state.name.chars().count() < NAME_MAX => state.name.push(ch),
            // The finish is decided below from just_pressed, as one branch
            // for keyboard and pad alike.
            Keystroke::Char(_) | Keystroke::Done(_) => {}
        }
    }
    if crate::app::menu_ui::enter(keys) || keys.just_pressed(KeyCode::Escape) {
        let tidy = state.name.trim().to_string();
        state.name = if tidy.is_empty() {
            tr.ed_default_name.to_string()
        } else {
            tidy
        };
        state.mode = Mode::JustNamed;
        state.feedback.clear();
    }
}

/// How long a level's name may be.
const NAME_MAX: usize = 28;

/// True while the editor is spelling out a name, which is when the board
/// keys mean letters. A run condition rather than an early return, because
/// the cursor is walked by a system of its own: without this, typing
/// "Wade" walked the cursor up and then left across the beach.
pub fn editor_naming(state: Res<EditorState>) -> bool {
    state.is_naming()
}

pub fn enter_editor(
    mut sim: ResMut<Sim>,
    settings: Res<GameSettings>,
    mut state: ResMut<EditorState>,
) {
    *state = EditorState {
        posts: 3,
        name: settings.tr().ed_default_name.to_string(),
        feedback: settings.tr().ed_fresh_sand.into(),
        dirty: true, // draws the initial statics
        ..default()
    };
    sim.0 = Board::new(EDITOR_BOARD.0, EDITOR_BOARD.1, 0xED17);
}

pub fn exit_editor(mut state: ResMut<EditorState>) {
    *state = EditorState::default();
}

/// Is the editor currently playtesting? (Run-condition helper.)
pub fn editor_testing(state: Res<EditorState>) -> bool {
    state.is_testing()
}

/// Put a different board on the sand: a resize or a pasted level.
///
/// The load path's rule applies here too (see `BoardSprites` in
/// `session.rs`): every sprite drawn from the old board goes, because the
/// sync systems probe the new board at each sprite's remembered tile, and
/// a crab, castle or log left over from a bigger beach probes a tile the
/// smaller one does not have. Only the statics were being cleared, so a
/// 20x13 sand shrunk to 9x7 with a castle in its far corner went down in
/// `tile_at`. The cursor comes back to the middle for the same reason:
/// movement only clamps on a step, so it would otherwise stand off the
/// board until it was moved, and be painted on there.
fn replace_board(
    board: Board,
    sim: &mut Sim,
    state: &mut EditorState,
    commands: &mut Commands,
    sprites: BoardSprites,
    cursors: &mut Query<(&mut Cursor, &mut Transform)>,
) {
    sim.0 = board;
    state.dirty = true;
    crate::app::session::despawn_board_sprites(commands.reborrow(), sprites);
    crate::app::cursor::center_on(&sim.0, cursors);
}

/// Tile and creature painting under the cursor: walls, terrain, crabs,
/// and gulls. The editor's command keys live in [`editor_commands`].
#[allow(clippy::too_many_arguments)]
pub fn editor_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    settings: Res<GameSettings>,
    mut sim: ResMut<Sim>,
    mut state: ResMut<EditorState>,
    sprites: BoardSprites,
    mut cursors: Query<(&mut Cursor, &mut Transform)>,
) {
    // Naming swallows the keyboard: every letter is a letter, not a brush.
    if state.is_naming() {
        type_a_name(&mut typed, &keys, &mut state, settings.tr());
        return;
    }
    typed.clear();
    // Beach size, on a function key. It was on `[` and `]`, which are
    // *physical* keys: on a keyboard that is not US the two beside P are
    // not those brackets at all, so the prompt named keys the player did
    // not have. F5 is F5 everywhere, and one key that wraps is enough for
    // four sizes.
    //
    // Resizing starts a fresh beach: there is no honest way to keep a
    // 20-wide layout when it becomes 9 wide, and half a level silently
    // thrown away is worse than a clean sheet you asked for.
    for (key, forward) in [(KeyCode::F5, true)] {
        if keys.just_pressed(key) {
            let board = &sim.0;
            let now = (board.width(), board.height());
            let at = EDITOR_SIZES.iter().position(|&s| s == now).unwrap_or(1);
            let n = EDITOR_SIZES.len();
            let (w, h) = EDITOR_SIZES[(at + if forward { 1 } else { n - 1 }) % n];
            replace_board(
                Board::new(w, h, 0xED17),
                &mut sim,
                &mut state,
                &mut commands,
                sprites,
                &mut cursors,
            );
            state.feedback = fill(
                settings.tr().ed_resized,
                &[("w", &w.to_string()), ("h", &h.to_string())],
            );
            return;
        }
    }
    if keys.just_pressed(KeyCode::F1) {
        state.mode = Mode::Naming;
        state.feedback = settings.tr().ed_naming.to_string();
        return;
    }
    let Some((cursor, _)) = cursors.iter().next() else {
        return;
    };
    let (x, y) = (cursor.x, cursor.y);
    let board = &mut sim.0;

    // Walls on the cursor tile's edges.
    for (key, dir) in [
        (KeyCode::ArrowUp, Direction::Up),
        (KeyCode::ArrowDown, Direction::Down),
        (KeyCode::ArrowLeft, Direction::Left),
        (KeyCode::ArrowRight, Direction::Right),
    ] {
        if keys.just_pressed(key) {
            let present = board.wall_at(x, y, dir);
            board.set_wall(x, y, dir, !present);
            state.dirty = true;
        }
    }

    // Pick a brush and paint with it. The letters do both, so a hand that
    // knows the old editor keeps working and the palette simply shows it
    // what it just chose; Tab walks the palette for a hand that does not.
    for brush in Brush::ALL {
        if keys.just_pressed(brush.key()) {
            state.brush = brush;
            paint(board, x, y, brush);
            state.dirty = true;
        }
    }
    if keys.just_pressed(KeyCode::Tab) {
        let at = Brush::ALL
            .iter()
            .position(|b| *b == state.brush)
            .unwrap_or(0);
        let step = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
            Brush::ALL.len() - 1
        } else {
            1
        };
        state.brush = Brush::ALL[(at + step) % Brush::ALL.len()];
    }
    if keys.just_pressed(KeyCode::Space) {
        paint(board, x, y, state.brush);
        state.dirty = true;
    }
}

/// A level out of what was on the clipboard, or what to say about why not.
///
/// Takes the pasted code rather than the clipboard itself, so the three ways
/// this fails (nothing readable there, a code of the wrong sort, a level
/// this build cannot parse) each get their own answer and their own test,
/// without a test ever touching the player's real clipboard.
fn level_from(
    pasted: Option<(crate::share::Kind, Vec<u8>)>,
    tr: &crate::app::i18n::Tr,
) -> Result<Level, String> {
    let text =
        crate::app::codes::payload_text(pasted, tr, crate::share::Kind::Level, tr.code_level_bad)?;
    Level::parse(&text).map_err(|e| fill(tr.code_level_bad, &[("e", &e)]))
}

/// The level as it stands on the sand, under the name and the kind the
/// author has chosen. Every command that turns the board into a level goes
/// through here, so saving, sharing and checking cannot disagree about what
/// is being built.
///
/// The rule is where the two kinds part. A puzzle is played with the
/// inventory it grants and no evictions; an arena keeps the board's own
/// rule, which is the one every generated beach plays under. Stamping the
/// puzzle rule on a handmade beach gave a versus table three signposts each
/// and no way to replace them, which is not the game the other beaches play.
///
/// A puzzle also plays without castle raids, as every campaign puzzle
/// does: its castle is the finish line, and a gull leaving through it
/// would move the target.
///
/// What comes back is read from the text that will be saved, not the
/// board on screen. The two differ when gulls were placed and erased:
/// each placement draws from the board's PRNG, the text carries only the
/// seed, and the survivors read back with a different hand and takeoff.
/// A "solvable" certified on the live board was then a claim about a
/// beach nobody would ever play.
pub(super) fn level_here(state: &EditorState, board: &Board, name: &str) -> Level {
    let mut snapshot = board.clone();
    if state.kind == LevelKind::Puzzle {
        snapshot.set_signpost_rule(state.posts, crate::sim::CapPolicy::Reject);
        snapshot.set_castle_raids(false);
    }
    let level = Level::from_board(name, state.posts, snapshot).with_kind(state.kind);
    Level::parse(&level.to_text()).unwrap_or(level)
}

/// The editor's command keys: post inventory, wrap, gull cadence, the
/// solver, saving, playtesting, and leaving.
///
/// Gated in the schedule on not naming (F1): while a name is being typed
/// every key is a letter, and before the gate an "o" in the name flipped
/// wrap, a "v" started the solver, Enter began a playtest and Escape left
/// for the menu. The frame the name is committed is sat out here (see
/// `EditorState::named`), because the gate lifts within that frame.
#[allow(clippy::too_many_arguments)]
pub fn editor_commands(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut sim: ResMut<Sim>,
    settings: Res<GameSettings>,
    mut clipboard: ResMut<Clipboard>,
    mut state: ResMut<EditorState>,
    mut saved: MessageWriter<crate::app::LevelSaved>,
    mut shared: MessageWriter<crate::app::CodeShared>,
    mut taken: MessageWriter<crate::app::CodeTaken>,
    mut next_screen: ResMut<NextState<Screen>>,
    sprites: BoardSprites,
    mut cursors: Query<(&mut Cursor, &mut Transform)>,
) {
    if matches!(state.mode, Mode::JustNamed) {
        state.mode = Mode::Painting;
        return;
    }
    let tr = settings.tr();
    // F3/F4: the level as a share code, and back. A level is a couple of
    // hundred characters, which is a thing you can put in a message - where
    // "save it and send them the file" was never really sharing.
    if keys.just_pressed(KeyCode::F3) {
        let level = level_here(&state, &sim.0, &state.name);
        let (feedback, took) = crate::app::codes::copy_counted(
            &mut clipboard,
            tr,
            crate::share::Kind::Level,
            level.to_text().as_bytes(),
            tr.code_copied,
        );
        state.feedback = feedback;
        if took {
            shared.write(crate::app::CodeShared);
        }
    }
    if keys.just_pressed(KeyCode::F4) {
        match level_from(crate::app::codes::paste(&mut clipboard), tr) {
            Ok(level) => {
                taken.write(crate::app::CodeTaken);
                state.posts = level.posts;
                // A pasted level brings its own kind: opening somebody's
                // beach and saving it back as a stage is not what either of
                // you meant.
                state.kind = level.kind;
                // A pasted level is any size, so it is a board swap in
                // full, sprites and cursor included.
                replace_board(
                    level.board(),
                    &mut sim,
                    &mut state,
                    &mut commands,
                    sprites,
                    &mut cursors,
                );
                // A pasted level is the one board here that nobody vetted:
                // authored elsewhere, carried through a chat message, and
                // dropped straight onto the sand. Check it on the way in
                // rather than waiting to be asked - which for a beach is a
                // count of its castles, not a search for a route.
                if level.kind == LevelKind::Arena {
                    state.feedback = arena_report(&sim.0, tr);
                } else {
                    start_validation(&mut state, level);
                    state.feedback = tr.code_level_checking.into();
                }
            }
            Err(complaint) => state.feedback = complaint,
        }
    }
    let board = &mut sim.0;
    // The signpost inventory is a puzzle's rule and nothing else: on a
    // beach the versus rule governs, so the dial would be turning a number
    // the match never reads. It keeps its value across a trip through arena
    // mode rather than being zeroed.
    if state.kind == LevelKind::Puzzle {
        if keys.just_pressed(KeyCode::Equal) {
            state.posts = (state.posts + 1).min(9);
        }
        if keys.just_pressed(KeyCode::Minus) {
            state.posts = state.posts.saturating_sub(1);
        }
    }
    if keys.just_pressed(KeyCode::KeyO) {
        let wrap = !board.wrap();
        board.set_wrap(wrap);
        state.dirty = true;
        state.feedback = if wrap {
            tr.ed_wrap_on.into()
        } else {
            tr.ed_wrap_off.into()
        };
    }
    if keys.just_pressed(KeyCode::KeyK) {
        state.gull_period_idx = (state.gull_period_idx + 1) % GULL_PERIODS.len();
        let period = GULL_PERIODS[state.gull_period_idx];
        board.set_gull_period(period);
        state.feedback = if period == 0 {
            tr.ed_gulls_off.into()
        } else {
            fill(tr.ed_gulls_every, &[("period", &period.to_string())])
        };
    }

    if keys.just_pressed(KeyCode::KeyV) {
        if state.kind == LevelKind::Arena {
            // Nothing to solve: a beach is playable or it is short of
            // castles, and that is an answer this frame rather than a
            // search on another thread.
            state.feedback = arena_report(board, tr);
        } else if state.solver.is_some() {
            state.feedback = tr.ed_already_validating.into();
        } else {
            let level = level_here(&state, board, "Custom");
            start_validation(&mut state, level);
            state.feedback = tr.ed_validating.into();
        }
    }
    if keys.just_pressed(KeyCode::F6) {
        state.kind = match state.kind {
            LevelKind::Puzzle => LevelKind::Arena,
            LevelKind::Arena => LevelKind::Puzzle,
        };
        state.feedback = match state.kind {
            LevelKind::Puzzle => tr.ed_now_puzzle.into(),
            LevelKind::Arena => tr.ed_now_arena.into(),
        };
    }
    if keys.just_pressed(KeyCode::F2) {
        let level = level_here(&state, board, &state.name);
        let path = save_path(&state.name);
        state.feedback = match crate::app::paths::write_atomic(&path, level.to_text()) {
            Ok(()) => {
                saved.write(crate::app::LevelSaved);
                let filed = fill(tr.ed_saved_to, &[("path", &path.display().to_string())]);
                match orphan_warning(&state, &level, tr) {
                    Some(complaint) => format!("{filed} - {complaint}"),
                    None => filed,
                }
            }
            Err(e) => fill(tr.ed_save_failed, &[("e", &e.to_string())]),
        };
    }
    if crate::app::menu_ui::enter(&keys) {
        let snapshot = board.clone();
        // Test it under the rule it will be played under: the granted
        // inventory for a stage, the versus rule the board already holds
        // for a beach.
        if state.kind == LevelKind::Puzzle {
            board.set_signpost_rule(state.posts, crate::sim::CapPolicy::Reject);
        }
        state.mode = Mode::Testing(Box::new(snapshot));
        state.feedback = tr.ed_playtest_prompt.into();
    }
    if keys.just_pressed(KeyCode::Escape) {
        next_screen.set(Screen::Menu);
    }
}

/// During a playtest the author plays: place and remove signposts live while
/// the sim runs. Esc ends the test and restores the snapshot.
pub fn editor_test_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim: ResMut<Sim>,
    settings: Res<GameSettings>,
    mut state: ResMut<EditorState>,
    cursors: Query<&Cursor>,
) {
    let Some(cursor) = cursors.iter().next() else {
        return;
    };
    for (key, dir) in [
        (KeyCode::ArrowUp, Direction::Up),
        (KeyCode::ArrowDown, Direction::Down),
        (KeyCode::ArrowLeft, Direction::Left),
        (KeyCode::ArrowRight, Direction::Right),
    ] {
        if keys.just_pressed(key) {
            let _ = sim.0.place_signpost(0, cursor.x, cursor.y, dir);
        }
    }
    if keys.just_pressed(KeyCode::Space) {
        let _ = sim.0.remove_signpost(0, cursor.x, cursor.y);
    }
    // Only Escape ends the test: Enter started it this very frame, and both
    // systems see the same just_pressed set.
    if keys.just_pressed(KeyCode::Escape)
        && let Mode::Testing(snapshot) = std::mem::take(&mut state.mode)
    {
        sim.0 = *snapshot;
        state.dirty = true;
        state.feedback = settings.tr().ed_back.into();
    }
}

/// Rebuild the static board sprites after an edit.
pub fn rebuild_statics(
    mut commands: Commands,
    sim: Res<Sim>,
    art: Res<crate::app::art::Art>,
    mut state: ResMut<EditorState>,
    statics: Query<Entity, With<crate::app::board_render::BoardStatic>>,
) {
    if !state.dirty {
        return;
    }
    state.dirty = false;
    for entity in &statics {
        commands.entity(entity).despawn();
    }
    crate::app::board_render::spawn_static_board(&mut commands, &sim.0, &art);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{CrabKind, Direction, Handedness, TileKind};

    fn sand() -> Board {
        Board::new(6, 5, 3)
    }

    /// Two levels with different names must not land on the same file:
    /// progress is filed under the name, so sharing a file means sharing a
    /// gold star. The character rule itself lives with `safe_stem`.
    #[test]
    fn different_names_are_different_files() {
        assert_ne!(save_path("First Beach"), save_path("Second Beach"));
        assert!(
            save_path("Gull Alley").ends_with("gull-alley.txt"),
            "{:?}",
            save_path("Gull Alley")
        );
    }

    /// The editor used to name every level "Custom", so two of them shared
    /// one progress entry and clearing either lit both on the stage list.
    #[test]
    fn a_saved_level_carries_the_name_it_was_given() {
        let level = Level::from_board("Gull Alley", 3, sand());
        assert_eq!(level.name, "Gull Alley");
        let back = Level::parse(&level.to_text()).expect("round trip");
        assert_eq!(back.name, "Gull Alley");
    }

    /// What the solver and the playtest are handed is what the file will
    /// say. Placing a gull draws from the board's PRNG and the text carries
    /// only the seed, so a board that placed and erased gulls reads back
    /// with the survivors rolled afresh; a "solvable" certified on the
    /// live board was a claim about a beach nobody would play.
    #[test]
    fn the_level_certified_is_the_level_saved() {
        let mut board = sand();
        board.set_tile(0, 0, TileKind::Castle(0));
        board.spawn_crab(2, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        // Two birds placed and taken back move the stream past where the
        // file's seed will start it.
        board.spawn_gull(3, 3, Direction::Right);
        board.remove_gulls_at(3, 3);
        board.spawn_gull(4, 4, Direction::Right);
        board.remove_gulls_at(4, 4);
        board.spawn_gull(1, 3, Direction::Right);
        let state = EditorState {
            posts: 3,
            kind: LevelKind::Puzzle,
            ..EditorState::default()
        };
        let level = level_here(&state, &board, "Stage");
        let saved = Level::parse(&level.to_text()).expect("round trip");
        assert_eq!(
            level.board().state_hash(),
            saved.board().state_hash(),
            "the board certified is the board saved"
        );
        assert!(!level.board().castle_raids(), "a stage plays without raids");
        assert!(level.to_text().contains("raids: off"));
    }

    /// The toggle decides two things at once: which list the saved level
    /// joins, and which signpost rule it is played under. A beach saved
    /// under the puzzle rule handed a versus table three posts each and no
    /// way to replace them, which is not the game the other beaches play.
    #[test]
    fn the_toggle_sets_the_kind_and_the_rule() {
        use crate::sim::CapPolicy;
        let mut board = sand();
        board.set_tile(0, 0, TileKind::Castle(0));
        board.set_tile(5, 4, TileKind::Castle(1));
        let state = |kind| EditorState {
            posts: 3,
            kind,
            ..EditorState::default()
        };

        let puzzle = level_here(&state(LevelKind::Puzzle), &board, "Stage");
        assert_eq!(puzzle.kind, LevelKind::Puzzle, "two castles, still a stage");
        assert_eq!(
            puzzle.board().signpost_rule(),
            (3, CapPolicy::Reject),
            "a stage grants what it grants"
        );

        let arena = level_here(&state(LevelKind::Arena), &board, "Beach");
        assert_eq!(arena.kind, LevelKind::Arena);
        assert_eq!(
            arena.board().signpost_rule(),
            board.signpost_rule(),
            "a beach keeps the versus rule"
        );
        assert_eq!(arena.board().signpost_rule().1, CapPolicy::Evict);
        // And it survives the file it is written to.
        let back = Level::parse(&arena.to_text()).expect("round trip");
        assert_eq!(back.kind, LevelKind::Arena);
        assert_eq!(back.board().signpost_rule(), arena.board().signpost_rule());
    }

    /// The posts dial is a puzzle's rule, so it is inert on a beach - and
    /// the value waits there rather than being lost on the way through.
    #[test]
    fn the_posts_dial_is_inert_on_a_beach() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.add_message::<crate::app::LevelSaved>();
        app.add_message::<crate::app::CodeShared>();
        app.add_message::<crate::app::CodeTaken>();
        app.insert_resource(Sim(sand()));
        app.init_resource::<EditorState>();
        app.init_resource::<GameSettings>();
        app.init_resource::<Clipboard>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.add_systems(Update, editor_commands);
        app.world_mut().resource_mut::<EditorState>().posts = 3;

        let tap = |app: &mut App, key: KeyCode| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(key);
            app.update();
        };
        tap(&mut app, KeyCode::Equal);
        assert_eq!(app.world().resource::<EditorState>().posts, 4, "a stage");
        tap(&mut app, KeyCode::F6); // now a beach
        tap(&mut app, KeyCode::Equal);
        tap(&mut app, KeyCode::Minus);
        assert_eq!(app.world().resource::<EditorState>().posts, 4, "untouched");
        tap(&mut app, KeyCode::F6); // and back
        tap(&mut app, KeyCode::Equal);
        assert_eq!(app.world().resource::<EditorState>().posts, 5, "waiting");
    }

    /// The Enter that commits a name must not also start a playtest: the
    /// commands sit out the frame the name was committed on, and read the
    /// key normally from the next frame.
    #[test]
    fn committing_a_name_does_not_start_a_playtest() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.add_message::<crate::app::LevelSaved>();
        app.add_message::<crate::app::CodeShared>();
        app.add_message::<crate::app::CodeTaken>();
        app.insert_resource(Sim(sand()));
        app.init_resource::<EditorState>();
        app.init_resource::<GameSettings>();
        app.init_resource::<Clipboard>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.add_systems(Update, editor_commands);
        app.world_mut().resource_mut::<EditorState>().mode = Mode::JustNamed;

        let press = |app: &mut App| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::Enter);
            app.update();
        };
        press(&mut app);
        let state = app.world().resource::<EditorState>();
        assert!(!state.is_testing(), "the naming Enter began a playtest");
        assert!(
            matches!(state.mode, Mode::Painting),
            "the latch is for one frame"
        );
        press(&mut app);
        assert!(
            app.world().resource::<EditorState>().is_testing(),
            "the next Enter is a real one"
        );
    }

    /// The key itself, through the system that reads it: F6 flips the kind
    /// and says where the level now lands.
    #[test]
    fn f6_flips_the_kind() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.add_message::<crate::app::LevelSaved>();
        app.add_message::<crate::app::CodeShared>();
        app.add_message::<crate::app::CodeTaken>();
        app.insert_resource(Sim(sand()));
        app.init_resource::<EditorState>();
        app.init_resource::<GameSettings>();
        app.init_resource::<Clipboard>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.add_systems(Update, editor_commands);

        let press = |app: &mut App| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::F6);
            app.update();
        };
        assert_eq!(
            app.world().resource::<EditorState>().kind,
            LevelKind::Puzzle
        );
        press(&mut app);
        let state = app.world().resource::<EditorState>();
        assert_eq!(state.kind, LevelKind::Arena);
        assert_eq!(state.feedback, crate::app::i18n::EN.ed_now_arena);
        press(&mut app);
        assert_eq!(
            app.world().resource::<EditorState>().kind,
            LevelKind::Puzzle
        );
    }

    /// A level authored here survives being carried as a share code: out
    /// through the level format, through the codec, and back to a board with
    /// the same walls, tiles and signpost budget on it.
    #[test]
    fn a_level_travels_as_a_code_and_comes_back() {
        let mut board = sand();
        board.set_tile(2, 2, TileKind::Rock);
        board.set_tile(4, 3, TileKind::Castle(0));
        board.set_wall(1, 1, Direction::Right, true);
        board.set_wrap(true);
        let level = Level::from_board("Custom", 5, board.clone());

        let code = crate::share::encode(crate::share::Kind::Level, level.to_text().as_bytes());
        let back = level_from(crate::share::decode(&code), &crate::app::i18n::EN)
            .expect("our own level reads back");

        assert_eq!(back.posts, 5, "the signpost budget travels with it");
        let rebuilt = back.board();
        assert_eq!(rebuilt.tile_at(2, 2), TileKind::Rock);
        assert_eq!(rebuilt.tile_at(4, 3), TileKind::Castle(0));
        assert!(rebuilt.wall_at(1, 1, Direction::Right));
        assert!(rebuilt.wrap(), "an open-ocean level stays open");
    }

    /// The three ways a paste is not a level, each answered its own way.
    #[test]
    fn what_is_not_a_level_says_which_way_it_is_not() {
        let tr = &crate::app::i18n::EN;
        assert_eq!(
            level_from(None, tr).err(),
            Some(tr.code_none_pasted.to_string())
        );
        // A real code, for a round rather than a level.
        let round = crate::share::encode(crate::share::Kind::Round, b"replay-v1\n");
        let complaint = level_from(crate::share::decode(&round), tr).expect_err("not a level");
        assert!(
            complaint.contains("a round") && complaint.contains("a level"),
            "{complaint}"
        );
        // A level code carrying something that is not a level.
        let junk = crate::share::encode(crate::share::Kind::Level, b"not a level at all");
        assert!(level_from(crate::share::decode(&junk), tr).is_err());
    }
}
