//! Driftwood, the level editor (spec §5.4): author walls, castles, spawners,
//! rocks, crabs, and gulls on a live board; playtest in place; validate
//! solvability by brute-force against the headless sim; save to disk in the
//! text level format.

use crate::app::cursor::Cursor;
use crate::app::i18n::fill;
use crate::app::session::BoardSprites;
use crate::app::settings::GameSettings;
use crate::app::{Screen, Sim};
use crate::sim::MAX_PLAYERS;
use crate::sim::{
    Board, CrabKind, Direction, Handedness, Level, LevelKind, SolveOutcome, Spawner, TileKind,
    solve,
};
use bevy::prelude::*;
use std::sync::{Arc, Mutex};

/// Slot a background validation thread drops its result into. `None` means
/// still running; the [`SolveOutcome`] inside says which of the three answers
/// it came back with.
type SolverSlot = Arc<Mutex<Option<SolveOutcome>>>;

const EDITOR_BOARD: (u8, u8) = (12, 9);

/// Beach sizes the editor offers, smallest first. The same set versus
/// plays on, so a level built here fits any of the arenas the game already
/// draws. Before this the editor had exactly one size, which made "a big
/// beach" something you could play on but not build.
const EDITOR_SIZES: [(u8, u8); 4] = [(9, 7), (12, 9), (16, 11), (20, 13)];
const SPAWNER_PERIOD: u32 = 60;
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

/// What the brush paints. One name for each thing the sand can hold, so
/// the palette, the key that selects it and the edit it makes are three
/// views of one list rather than three lists that have to agree.
///
/// Before this the editor had nine letters and no way to find out what any
/// of them did except to press it and watch: no palette, no selection, and
/// a prompt line reading `R C H L W P B G O tiles`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Brush {
    /// Bare sand, which is also the eraser: the only way to clear a tile
    /// without knowing what is already on it.
    #[default]
    Sand,
    Rock,
    Castle,
    Hole,
    Log,
    Weed,
    Pool,
    Crab,
    Gull,
}

impl Brush {
    pub const ALL: [Brush; 9] = [
        Brush::Sand,
        Brush::Rock,
        Brush::Castle,
        Brush::Hole,
        Brush::Log,
        Brush::Weed,
        Brush::Pool,
        Brush::Crab,
        Brush::Gull,
    ];

    /// The key that loads this brush, which is also the shortcut that
    /// paints it outright. Kept as they were where they could be, so a
    /// hand that already knows the editor does not have to learn it again.
    ///
    /// None of them may be W, A, S or D: those walk the cursor, and a key
    /// that does both walks the cursor *and* changes the brush. Weed was
    /// on W, so moving up quietly armed the kelp. `no_brush_takes_a_
    /// movement_key` holds the line.
    pub fn key(self) -> KeyCode {
        match self {
            Brush::Sand => KeyCode::KeyX,
            Brush::Rock => KeyCode::KeyR,
            Brush::Castle => KeyCode::KeyC,
            Brush::Hole => KeyCode::KeyH,
            Brush::Log => KeyCode::KeyL,
            Brush::Weed => KeyCode::KeyE,
            Brush::Pool => KeyCode::KeyP,
            Brush::Crab => KeyCode::KeyB,
            Brush::Gull => KeyCode::KeyG,
        }
    }

    /// The letter the palette shows beside the brush: the one on the key
    /// that loads it, always. The two lists once disagreed - Weed moved
    /// off W to E and the palette went on saying W - and a palette that
    /// names the wrong key is worse than one that names none, so
    /// `letters_name_their_keys` now holds them together.
    pub fn letter(self) -> &'static str {
        match self {
            Brush::Sand => "X",
            Brush::Rock => "R",
            Brush::Castle => "C",
            Brush::Hole => "H",
            Brush::Log => "L",
            Brush::Weed => "E",
            Brush::Pool => "P",
            Brush::Crab => "B",
            Brush::Gull => "G",
        }
    }

    pub fn label(self, tr: &crate::app::i18n::Tr) -> &'static str {
        tr.ed_brushes[Brush::ALL.iter().position(|b| *b == self).unwrap_or(0)]
    }

    pub fn icon(self, art: &crate::app::art::Art) -> Handle<Image> {
        match self {
            Brush::Sand => art.sand_a.clone(),
            Brush::Rock => art.rock.clone(),
            Brush::Castle => art.castle.clone(),
            Brush::Hole => art.hole.clone(),
            Brush::Log => art.log.clone(),
            Brush::Weed => art.kelp.clone(),
            Brush::Pool => art.pool.clone(),
            Brush::Crab => art.crab.clone(),
            Brush::Gull => art.gull.clone(),
        }
    }
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
    /// True while the name is being typed rather than the board edited.
    pub naming: bool,
    /// True for the one frame after the name is committed. The Enter or
    /// Escape that ended it is still just-pressed when [`editor_commands`]
    /// runs later that frame, where it reads as "playtest" or "leave"; the
    /// schedule's naming gate cannot see this, since `naming` is already
    /// false by then, so the commands sit that frame out on this instead.
    pub named: bool,
    /// What the brush is loaded with. Painting is now a thing you choose
    /// and then do, rather than nine separate verbs.
    pub brush: Brush,
    pub gull_period_idx: usize,
    /// Pre-playtest snapshot to restore when the test ends.
    pub testing: Option<Board>,
    pub feedback: String,
    solver: Option<SolverSlot>,
    /// Statics need a rebuild after a tile/wall edit.
    dirty: bool,
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
    for event in typed.read() {
        if !event.state.is_pressed() {
            continue;
        }
        let done = matches!(
            event.key_code,
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Escape
        );
        if matches!(event.key_code, KeyCode::Backspace | KeyCode::Delete) {
            state.name.pop();
        } else if !done {
            for ch in event.text.iter().flat_map(|text| text.chars()) {
                // A name is one line and has to fit a file name and a
                // stage caption, so it is bounded here rather than at save.
                if !ch.is_control() && state.name.chars().count() < NAME_MAX {
                    state.name.push(ch);
                }
            }
        }
    }
    if keys.just_pressed(KeyCode::Enter)
        || keys.just_pressed(KeyCode::NumpadEnter)
        || keys.just_pressed(KeyCode::Escape)
    {
        let tidy = state.name.trim().to_string();
        state.name = if tidy.is_empty() {
            tr.ed_default_name.to_string()
        } else {
            tidy
        };
        state.naming = false;
        state.named = true;
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
    state.naming
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
    state.testing.is_some()
}

/// Step the castle on a tile through the four seats and then away again,
/// so one key paints every owner. Anything else on the tile is replaced
/// (and its creatures swept) on the way in.
fn cycle_castle(board: &mut Board, x: u8, y: u8) {
    let kind = match board.tile_at(x, y) {
        TileKind::Castle(owner) if usize::from(owner) + 1 < MAX_PLAYERS => {
            TileKind::Castle(owner + 1)
        }
        TileKind::Castle(_) => TileKind::Empty,
        TileKind::Empty
        | TileKind::Rock
        | TileKind::Spawner(_)
        | TileKind::Turnstile { .. }
        | TileKind::Kelp
        | TileKind::Pool => {
            board.remove_crabs_at(x, y);
            board.remove_gulls_at(x, y);
            TileKind::Castle(0)
        }
    };
    board.set_tile(x, y, kind);
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
    if state.naming {
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
        state.naming = true;
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

/// Put `brush` on the tile under the cursor.
///
/// Every brush replaces whatever was there, rather than toggling it off
/// again: a palette that paints on the first press and erases on the second
/// is a palette that cannot be dragged across a row. `Sand` is the eraser,
/// and it is the only way to clear a tile without first knowing what is on
/// it.
fn paint(board: &mut Board, x: u8, y: u8, brush: Brush) {
    // Two of them cycle rather than replace, because they carry a value the
    // player has to be able to reach: which seat a castle belongs to, and
    // which way a hole faces.
    match brush {
        Brush::Castle => return cycle_castle(board, x, y),
        Brush::Hole => {
            let kind = match board.tile_at(x, y) {
                TileKind::Spawner(s) => match next_dir(s.dir) {
                    Some(dir) => TileKind::Spawner(Spawner {
                        dir,
                        period: SPAWNER_PERIOD,
                    }),
                    None => TileKind::Empty,
                },
                TileKind::Empty
                | TileKind::Rock
                | TileKind::Castle(_)
                | TileKind::Turnstile { .. }
                | TileKind::Kelp
                | TileKind::Pool => {
                    board.remove_crabs_at(x, y);
                    board.remove_gulls_at(x, y);
                    TileKind::Spawner(Spawner {
                        dir: Direction::Right,
                        period: SPAWNER_PERIOD,
                    })
                }
            };
            return board.set_tile(x, y, kind);
        }
        Brush::Crab => return cycle_crab(board, x, y),
        Brush::Gull => {
            let tile = board.index_of(x, y);
            if board.gulls().iter().any(|g| g.tile == tile) {
                board.remove_gulls_at(x, y);
            } else if board.tile_at(x, y) != TileKind::Rock {
                board.spawn_gull(x, y, Direction::Right);
            }
            return;
        }
        Brush::Sand | Brush::Rock | Brush::Log | Brush::Weed | Brush::Pool => {}
    }
    // The rest are plain ground. Anything standing on the tile goes with
    // it, since a crab inside a rock is not a thing the sim has a rule for.
    board.remove_crabs_at(x, y);
    board.remove_gulls_at(x, y);
    board.set_tile(
        x,
        y,
        match brush {
            Brush::Rock => TileKind::Rock,
            Brush::Log => TileKind::Turnstile { next_right: true },
            Brush::Weed => TileKind::Kelp,
            Brush::Pool => TileKind::Pool,
            // The four that return above never reach here; sand is the
            // eraser and clears the tile.
            Brush::Sand | Brush::Castle | Brush::Hole | Brush::Crab | Brush::Gull => {
                TileKind::Empty
            }
        },
    );
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
fn level_here(state: &EditorState, board: &Board, name: &str) -> Level {
    let mut snapshot = board.clone();
    if state.kind == LevelKind::Puzzle {
        snapshot.set_signpost_rule(state.posts, crate::sim::CapPolicy::Reject);
    }
    Level::from_board(name, state.posts, snapshot).with_kind(state.kind)
}

/// What the check says about a beach, which is not a thing the solver can
/// answer: a match is not won or lost, it is only playable or not. Two
/// castles is the floor, and crabs have to come from somewhere.
fn arena_report(board: &Board, tr: &crate::app::i18n::Tr) -> String {
    let seats = board.castle_seats();
    let holes = board
        .tiles()
        .filter(|(_, _, kind)| matches!(kind, TileKind::Spawner(_)))
        .count();
    if seats < 2 {
        return fill(tr.ed_arena_needs_seats, &[("n", &seats.to_string())]);
    }
    if holes == 0 && board.crabs().is_empty() {
        return tr.ed_arena_no_crabs.to_string();
    }
    fill(
        tr.ed_arena_ok,
        &[("n", &seats.to_string()), ("h", &holes.to_string())],
    )
}

/// Why the level, as it stands, will turn up on no list at all - or `None`
/// when it will.
///
/// Both lists have a floor: the stage list takes a puzzle with a crab to
/// route, the map dial takes a beach with a castle for every seat at the
/// table. A file that clears neither is written to disk and then never seen
/// again, and being saved is exactly the moment an author stops looking for
/// it. Before the kind was the author's to choose nothing could go missing
/// this way: every level with a crab was a stage.
fn orphan_warning(state: &EditorState, level: &Level, tr: &crate::app::i18n::Tr) -> Option<String> {
    match state.kind {
        LevelKind::Arena if level.seats() < 2 => Some(fill(
            tr.ed_arena_needs_seats,
            &[("n", &level.seats().to_string())],
        )),
        LevelKind::Puzzle if level.crab_count() == 0 => Some(tr.ed_puzzle_no_crabs.to_string()),
        LevelKind::Arena | LevelKind::Puzzle => None,
    }
}

/// Hand a level to the solver on a background thread, superseding whatever
/// was running.
///
/// Off the frame thread because even a budgeted search is seconds of work
/// (`DEFAULT_NODE_BUDGET`), and the editor has to keep drawing while it
/// happens. Superseding rather than queueing because only the newest board
/// matters: an answer about a board the author has already replaced is worse
/// than no answer at all. The displaced thread runs on to its budget and
/// writes into a slot nobody holds any more, and its verdict is dropped.
fn start_validation(state: &mut EditorState, level: Level) {
    let slot: SolverSlot = Arc::new(Mutex::new(None));
    let thread_slot = Arc::clone(&slot);
    std::thread::spawn(move || {
        *thread_slot.lock().unwrap() = Some(solve(&level));
    });
    state.solver = Some(slot);
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
    mut next_screen: ResMut<NextState<Screen>>,
    sprites: BoardSprites,
    mut cursors: Query<(&mut Cursor, &mut Transform)>,
) {
    if std::mem::take(&mut state.named) {
        return;
    }
    let tr = settings.tr();
    // F3/F4: the level as a share code, and back. A level is a couple of
    // hundred characters, which is a thing you can put in a message - where
    // "save it and send them the file" was never really sharing.
    if keys.just_pressed(KeyCode::F3) {
        let level = level_here(&state, &sim.0, &state.name);
        state.feedback = crate::app::codes::copy_feedback(
            &mut clipboard,
            tr,
            crate::share::Kind::Level,
            level.to_text().as_bytes(),
            tr.code_copied,
        );
    }
    if keys.just_pressed(KeyCode::F4) {
        match level_from(crate::app::codes::paste(&mut clipboard), tr) {
            Ok(level) => {
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
    if keys.just_pressed(KeyCode::Enter) {
        let snapshot = board.clone();
        // Test it under the rule it will be played under: the granted
        // inventory for a stage, the versus rule the board already holds
        // for a beach.
        if state.kind == LevelKind::Puzzle {
            board.set_signpost_rule(state.posts, crate::sim::CapPolicy::Reject);
        }
        state.testing = Some(snapshot);
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
        && let Some(snapshot) = state.testing.take()
    {
        sim.0 = snapshot;
        state.dirty = true;
        state.feedback = settings.tr().ed_back.into();
    }
}

/// Collect a finished background validation.
pub fn poll_solver(settings: Res<GameSettings>, mut state: ResMut<EditorState>) {
    let tr = settings.tr();
    let Some(slot) = &state.solver else {
        return;
    };
    let result = slot.lock().unwrap().take();
    match result {
        None => return, // still searching
        Some(SolveOutcome::Found(solution)) => {
            let described: Vec<String> = solution
                .iter()
                .map(|(x, y, dir)| format!("({x},{y} {dir:?})"))
                .collect();
            state.feedback = if solution.is_empty() {
                tr.ed_solvable_free.into()
            } else {
                fill(tr.ed_solvable, &[("placements", &described.join(" "))])
            };
        }
        Some(SolveOutcome::Unsolvable) => {
            state.feedback = fill(tr.ed_not_solvable, &[("n", &state.posts.to_string())]);
        }
        // The one answer the old editor could not give. It used to search
        // without a ceiling, so a board it could not crack simply never came
        // back, and "still validating..." forever reads as a hung editor.
        Some(SolveOutcome::GaveUp) => {
            state.feedback = tr.ed_solver_gave_up.into();
        }
    }
    state.solver = None;
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

fn next_dir(dir: Direction) -> Option<Direction> {
    match dir {
        Direction::Right => Some(Direction::Down),
        Direction::Down => Some(Direction::Left),
        Direction::Left => Some(Direction::Up),
        Direction::Up => None,
    }
}

/// Cycle the crab on a tile through kind/handedness combinations, ending at
/// none. Spawned crabs wall-resolve, so the cycle keys off identity fields.
fn cycle_crab(board: &mut Board, x: u8, y: u8) {
    const CYCLE: [(CrabKind, Handedness); 12] = [
        (CrabKind::Common, Handedness::Left),
        (CrabKind::Common, Handedness::Right),
        (CrabKind::Juvenile, Handedness::Left),
        (CrabKind::Juvenile, Handedness::Right),
        (CrabKind::Giant, Handedness::Left),
        (CrabKind::Giant, Handedness::Right),
        (CrabKind::Molting, Handedness::Left),
        (CrabKind::Molting, Handedness::Right),
        (CrabKind::Golden, Handedness::Left),
        (CrabKind::Golden, Handedness::Right),
        (CrabKind::Sparkling, Handedness::Left),
        (CrabKind::Sparkling, Handedness::Right),
    ];
    if board.tile_at(x, y) != TileKind::Empty {
        return; // crabs start on open sand only
    }
    let tile = board.index_of(x, y);
    let current = board
        .crabs()
        .iter()
        .find(|c| c.tile == tile)
        .map(|c| (c.kind, c.handed));
    board.remove_crabs_at(x, y);
    let next = match current {
        None => Some(CYCLE[0]),
        Some(cur) => CYCLE
            .iter()
            .position(|&e| e == cur)
            .and_then(|i| CYCLE.get(i + 1))
            .copied(),
    };
    if let Some((kind, handed)) = next {
        board.spawn_crab(x, y, Direction::Right, handed, kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Board;

    /// The editor moves on WASD and paints with letters, so a brush on one
    /// of those four does two things at once. It also must not collide
    /// with the editor's own keys, which are just as invisible a clash.
    #[test]
    fn no_brush_takes_a_movement_key() {
        use bevy::prelude::KeyCode;
        let movement = [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD];
        let controls = [
            KeyCode::KeyK,   // gull period
            KeyCode::KeyO,   // wrapping edges
            KeyCode::KeyV,   // validate
            KeyCode::Tab,    // next brush
            KeyCode::Space,  // paint
            KeyCode::Enter,  // playtest
            KeyCode::Escape, // leave
            KeyCode::F6,     // puzzle or beach
        ];
        for brush in Brush::ALL {
            let key = brush.key();
            assert!(!movement.contains(&key), "{brush:?} is on a movement key");
            assert!(!controls.contains(&key), "{brush:?} is on a control key");
        }
        // And no two brushes share one.
        let mut keys: Vec<_> = Brush::ALL
            .iter()
            .map(|b| format!("{:?}", b.key()))
            .collect();
        keys.sort();
        let before = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), before, "two brushes on one key");
    }

    /// The palette names the key that loads each brush, so the letter it
    /// shows has to be the key's own: it once said "W" for kelp after the
    /// key had moved to E.
    #[test]
    fn letters_name_their_keys() {
        for brush in Brush::ALL {
            assert_eq!(
                format!("{:?}", brush.key()),
                format!("Key{}", brush.letter()),
                "{brush:?} shows a letter that is not its key"
            );
        }
    }

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

    /// Saving a level that will appear on neither list says so. Before the
    /// author picked the kind, nothing could go missing this way: every
    /// level with a crab on it was a stage.
    #[test]
    fn a_save_that_lands_nowhere_says_so() {
        use crate::app::i18n::EN;
        let mut board = sand();
        let state = |kind| EditorState {
            posts: 3,
            kind,
            ..EditorState::default()
        };

        // A beach with one castle is on no dial: two seats is the floor.
        board.set_tile(0, 0, TileKind::Castle(0));
        let arena = state(LevelKind::Arena);
        let level = level_here(&arena, &board, "Half a Beach");
        let complaint = orphan_warning(&arena, &level, &EN).expect("nowhere to go");
        assert!(complaint.contains('1'), "{complaint}");

        // The same board as a puzzle has no crab to route, which is the
        // other floor.
        let puzzle = state(LevelKind::Puzzle);
        let level = level_here(&puzzle, &board, "Empty Stage");
        assert_eq!(
            orphan_warning(&puzzle, &level, &EN).as_deref(),
            Some(EN.ed_puzzle_no_crabs)
        );

        // Give each what it wants and the complaint goes away.
        board.spawn_crab(2, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        let level = level_here(&puzzle, &board, "A Stage");
        assert_eq!(orphan_warning(&puzzle, &level, &EN), None);
        board.set_tile(5, 4, TileKind::Castle(1));
        let level = level_here(&arena, &board, "A Beach");
        assert_eq!(orphan_warning(&arena, &level, &EN), None);
    }

    /// The posts dial is a puzzle's rule, so it is inert on a beach - and
    /// the value waits there rather than being lost on the way through.
    #[test]
    fn the_posts_dial_is_inert_on_a_beach() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.add_message::<crate::app::LevelSaved>();
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
        app.insert_resource(Sim(sand()));
        app.init_resource::<EditorState>();
        app.init_resource::<GameSettings>();
        app.init_resource::<Clipboard>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.add_systems(Update, editor_commands);
        app.world_mut().resource_mut::<EditorState>().named = true;

        let press = |app: &mut App| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::Enter);
            app.update();
        };
        press(&mut app);
        let state = app.world().resource::<EditorState>();
        assert!(state.testing.is_none(), "the naming Enter began a playtest");
        assert!(!state.named, "the flag is for one frame");
        press(&mut app);
        assert!(
            app.world().resource::<EditorState>().testing.is_some(),
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

    /// Checking a beach counts its castles instead of searching for a
    /// route, and says which of the two things is missing.
    #[test]
    fn checking_a_beach_reads_its_castles() {
        use crate::app::i18n::EN;
        let mut board = sand();
        assert!(arena_report(&board, &EN).contains('0'), "bare sand seats 0");
        board.set_tile(0, 0, TileKind::Castle(0));
        let one = arena_report(&board, &EN);
        assert!(one.contains('1'), "one castle is not a match: {one}");

        board.set_tile(5, 4, TileKind::Castle(1));
        assert_eq!(
            arena_report(&board, &EN),
            EN.ed_arena_no_crabs,
            "castles but nothing to fight over"
        );

        board.set_tile(
            2,
            2,
            TileKind::Spawner(Spawner {
                dir: Direction::Right,
                period: SPAWNER_PERIOD,
            }),
        );
        let ok = arena_report(&board, &EN);
        assert!(ok.contains('2'), "two seats: {ok}");
        assert!(!ok.contains("wants"), "no complaint left: {ok}");
    }

    /// One key paints all four owners and then clears the tile, and a
    /// castle never lands on top of a creature.
    #[test]
    fn the_castle_key_walks_the_seats_then_clears() {
        let mut board = sand();
        board.spawn_crab(2, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        for owner in 0..MAX_PLAYERS as u8 {
            cycle_castle(&mut board, 2, 2);
            assert_eq!(board.tile_at(2, 2), TileKind::Castle(owner));
        }
        assert!(board.crabs().is_empty(), "the crab was swept aside");
        cycle_castle(&mut board, 2, 2);
        assert_eq!(board.tile_at(2, 2), TileKind::Empty, "and round to nothing");
    }

    /// A brush replaces what is on the tile, and `Sand` is what takes it
    /// away.
    ///
    /// It used to toggle: pressing the same key twice put the tile back to
    /// bare sand. That reads well with one key per terrain and badly with a
    /// palette, where dragging a brush along a row would rub out every
    /// other tile it crossed. `Sand` is now the eraser, and it is the only
    /// one that does not need to know what it is erasing.
    #[test]
    fn a_brush_replaces_and_sand_erases() {
        let mut board = sand();
        paint(&mut board, 1, 1, Brush::Weed);
        assert_eq!(board.tile_at(1, 1), TileKind::Kelp);
        paint(&mut board, 1, 1, Brush::Weed);
        assert_eq!(board.tile_at(1, 1), TileKind::Kelp, "and again is the same");
        paint(&mut board, 1, 1, Brush::Pool);
        assert_eq!(board.tile_at(1, 1), TileKind::Pool, "another replaces it");
        paint(&mut board, 1, 1, Brush::Sand);
        assert_eq!(board.tile_at(1, 1), TileKind::Empty, "and sand clears it");

        // Whatever was standing there goes with the ground: a crab inside a
        // rock is not a thing the sim has a rule for.
        board.spawn_crab(3, 3, Direction::Right, Handedness::Left, CrabKind::Common);
        paint(&mut board, 3, 3, Brush::Rock);
        assert_eq!(board.tile_at(3, 3), TileKind::Rock);
        assert!(board.crabs().is_empty());
    }

    /// Every brush has its own letter, and the palette, the keys and the
    /// labels are all the one list.
    #[test]
    fn the_palette_is_one_list() {
        use std::collections::HashSet;
        let keys: HashSet<KeyCode> = Brush::ALL.iter().map(|b| b.key()).collect();
        assert_eq!(keys.len(), Brush::ALL.len(), "two brushes share a key");
        let letters: HashSet<&str> = Brush::ALL.iter().map(|b| b.letter()).collect();
        assert_eq!(
            letters.len(),
            Brush::ALL.len(),
            "two brushes share a letter"
        );
        assert_eq!(
            crate::app::i18n::EN.ed_brushes.len(),
            Brush::ALL.len(),
            "the palette and its labels have drifted apart"
        );
    }

    /// The spawner key turns its hole through the four directions and then
    /// removes it, so the whole cycle is reachable from one key.
    #[test]
    fn spawner_directions_run_out_after_four() {
        let dirs: Vec<Direction> =
            std::iter::successors(Some(Direction::Right), |d| next_dir(*d)).collect();
        assert_eq!(
            dirs,
            [
                Direction::Right,
                Direction::Down,
                Direction::Left,
                Direction::Up
            ]
        );
        assert_eq!(next_dir(Direction::Up), None, "the cycle ends, and removes");
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

    /// A validation started here really does come back with a verdict: the
    /// thread runs, the slot fills, and `poll_solver` has something to
    /// collect. Both answers are checked, because the interesting failure is
    /// a search that reports one board's verdict for another's.
    #[test]
    fn a_started_validation_leaves_a_verdict_in_the_slot() {
        fn verdict(level: Level) -> SolveOutcome {
            let mut state = EditorState::default();
            start_validation(&mut state, level);
            let slot = state.solver.expect("a validation is running");
            // The solver is on its own thread; wait for it rather than
            // sleeping a fixed guess.
            loop {
                if let Some(outcome) = slot.lock().unwrap().take() {
                    return outcome;
                }
                std::thread::yield_now();
            }
        }
        // A crab one tile left of a castle, walking into it: solvable with
        // nothing placed at all.
        let mut easy = sand();
        easy.set_tile(4, 2, TileKind::Castle(0));
        easy.spawn_crab(1, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        assert_eq!(
            verdict(Level::from_board("Custom", 1, easy)),
            SolveOutcome::Found(Vec::new())
        );

        // The same crab, with the castle walled off on all four sides.
        let mut sealed = sand();
        sealed.set_tile(4, 2, TileKind::Castle(0));
        sealed.spawn_crab(1, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        for dir in Direction::ALL {
            sealed.set_wall(4, 2, dir, true);
        }
        assert_eq!(
            verdict(Level::from_board("Custom", 1, sealed)),
            SolveOutcome::Unsolvable
        );
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

    /// The crab key walks every kind in both handednesses and then leaves
    /// the tile bare; crabs refuse to stand on anything but open sand.
    #[test]
    fn the_crab_key_cycles_twelve_then_clears() {
        let mut board = sand();
        let mut seen = Vec::new();
        for _ in 0..12 {
            cycle_crab(&mut board, 3, 3);
            let crab = board.crabs().first().copied().expect("a crab is standing");
            seen.push((crab.kind, crab.handed));
        }
        seen.dedup();
        assert_eq!(seen.len(), 12, "every kind, both hands: {seen:?}");
        cycle_crab(&mut board, 3, 3);
        assert!(board.crabs().is_empty(), "the thirteenth press clears it");

        // Not on a rock.
        board.set_tile(4, 3, TileKind::Rock);
        cycle_crab(&mut board, 4, 3);
        assert!(board.crabs().is_empty(), "crabs start on open sand only");
    }
}

// --- the palette --------------------------------------------------------

#[derive(Component)]
pub struct EditorUi;

/// One row of the palette, by its place in [`Brush::ALL`].
#[derive(Component)]
pub struct PaletteRow(usize);

#[derive(Component)]
pub struct PaletteLabel(usize);

/// One of the two rows of the kind toggle, by what it selects.
#[derive(Component)]
pub struct KindRow(LevelKind);

/// What the cursor is standing on, which is the other half of knowing what
/// a keypress is about to do.
#[derive(Component)]
pub struct UnderCursor;

/// The brush palette: every paintable thing, its sprite, and the letter
/// that loads it.
///
/// The editor used to say `R C H L W P B G O tiles` along the bottom and
/// leave the rest to memory, which is a poor deal for a screen whose whole
/// job is making things.
pub fn spawn_editor_ui(
    mut commands: Commands,
    art: Res<crate::app::art::Art>,
    settings: Res<GameSettings>,
) {
    let tr = settings.tr();
    commands
        .spawn((
            EditorUi,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(70.0),
                right: Val::Px(16.0),
                width: Val::Px(190.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::all(Val::Px(10.0)),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(12.0)),
                ..default()
            },
            BorderColor::all(crate::app::palette::CARD_EDGE),
            BackgroundColor(crate::app::palette::CARD_FILL),
            crate::app::menu_ui::card_shadow(),
        ))
        .with_children(|panel| {
            // What is being built, above the brushes and lit like them: the
            // choice decides which list the level joins, and a toggle you
            // cannot see the state of is a toggle you press to find out.
            panel.spawn((
                Text::new(tr.ed_kind_row.to_string()),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(crate::app::palette::PARCHMENT.with_alpha(0.55)),
            ));
            for kind in [LevelKind::Puzzle, LevelKind::Arena] {
                panel
                    .spawn((
                        KindRow(kind),
                        Node {
                            align_items: AlignItems::Center,
                            padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                            border_radius: BorderRadius::all(Val::Px(6.0)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .with_children(|row| {
                        row.spawn((
                            Text::new(match kind {
                                LevelKind::Puzzle => tr.ed_kind_puzzle.to_string(),
                                LevelKind::Arena => tr.ed_kind_arena.to_string(),
                            }),
                            TextFont {
                                font_size: FontSize::Px(15.0),
                                ..default()
                            },
                            TextColor(crate::app::palette::PARCHMENT),
                        ));
                    });
            }
            panel.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(1.0),
                    margin: UiRect::axes(Val::Px(0.0), Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(crate::app::palette::PARCHMENT.with_alpha(0.14)),
            ));
            for (i, brush) in Brush::ALL.iter().enumerate() {
                panel
                    .spawn((
                        PaletteRow(i),
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(8.0),
                            padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                            border_radius: BorderRadius::all(Val::Px(6.0)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .with_children(|row| {
                        row.spawn((
                            ImageNode::new(brush.icon(&art)),
                            Node {
                                width: Val::Px(22.0),
                                height: Val::Px(22.0),
                                ..default()
                            },
                        ));
                        row.spawn((
                            PaletteLabel(i),
                            Text::new(format!("{}  {}", brush.letter(), brush.label(tr))),
                            TextFont {
                                font_size: FontSize::Px(16.0),
                                ..default()
                            },
                            TextColor(crate::app::palette::PARCHMENT),
                        ));
                    });
            }
            panel.spawn((
                UnderCursor,
                Text::new(String::new()),
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(crate::app::palette::IDLE_ROW),
                Node {
                    margin: UiRect::top(Val::Px(8.0)),
                    ..default()
                },
            ));
        });
}

/// Light the loaded brush and the chosen kind, and say what the cursor is
/// standing on.
#[allow(clippy::too_many_arguments)]
pub fn update_editor_palette(
    state: Res<EditorState>,
    sim: Res<Sim>,
    settings: Res<GameSettings>,
    cursors: Query<&Cursor>,
    mut rows: Query<(&PaletteRow, &mut BackgroundColor)>,
    mut kinds: Query<(&KindRow, &mut BackgroundColor), Without<PaletteRow>>,
    mut labels: Query<(&PaletteLabel, &mut TextColor)>,
    mut under: Query<&mut Text, With<UnderCursor>>,
) {
    let tr = settings.tr();
    for (row, mut fill) in &mut kinds {
        let want = match row.0 == state.kind {
            true => crate::app::palette::SELECTED_ROW.with_alpha(0.22),
            false => Color::NONE,
        };
        crate::app::menu_ui::set_bg(&mut fill, want);
    }
    let at = Brush::ALL.iter().position(|b| *b == state.brush);
    for (row, mut fill) in &mut rows {
        let want = match Some(row.0) == at {
            true => crate::app::palette::SELECTED_ROW.with_alpha(0.22),
            false => Color::NONE,
        };
        crate::app::menu_ui::set_bg(&mut fill, want);
    }
    for (label, mut color) in &mut labels {
        let want = match Some(label.0) == at {
            true => crate::app::palette::SELECTED_ROW,
            false => crate::app::palette::PARCHMENT,
        };
        crate::app::menu_ui::set_color(&mut color, want);
    }
    let Some(cursor) = cursors.iter().next() else {
        return;
    };
    let board = &sim.0;
    let standing = standing_on(board, cursor.x, cursor.y).label(tr);
    for mut text in &mut under {
        crate::app::menu_ui::set_text(&mut text, &fill(tr.ed_under, &[("t", standing)]));
    }
}

/// Which brush would have drawn what is on this tile. Creatures win over
/// the ground they stand on, because that is what the player put there
/// last and what another press would take away.
fn standing_on(board: &Board, x: u8, y: u8) -> Brush {
    let tile = board.index_of(x, y);
    if board.gulls().iter().any(|g| g.tile == tile) {
        return Brush::Gull;
    }
    if board.crabs().iter().any(|c| c.tile == tile) {
        return Brush::Crab;
    }
    match board.tile_at(x, y) {
        TileKind::Rock => Brush::Rock,
        TileKind::Castle(_) => Brush::Castle,
        TileKind::Spawner(_) => Brush::Hole,
        TileKind::Turnstile { .. } => Brush::Log,
        TileKind::Kelp => Brush::Weed,
        TileKind::Pool => Brush::Pool,
        TileKind::Empty => Brush::Sand,
    }
}
