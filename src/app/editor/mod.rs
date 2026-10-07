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
use validate::{Validation, arena_report, orphan_warning, start_validation};

use crate::app::cursor::Cursor;
use crate::app::i18n::fill;
use crate::app::session::BoardSprites;
use crate::app::settings::GameSettings;
use crate::app::{Screen, Sim};
use crate::sim::{Board, Direction, Level, LevelKind};
use bevy::prelude::*;

/// The arrow keys and the edge each names: a wall toggled on that side of
/// the cursor's tile, or, in a playtest, the way an arrow points.
const ARROWS: [(KeyCode, Direction); 4] = [
    (KeyCode::ArrowUp, Direction::Up),
    (KeyCode::ArrowDown, Direction::Down),
    (KeyCode::ArrowLeft, Direction::Left),
    (KeyCode::ArrowRight, Direction::Right),
];

const EDITOR_BOARD: (u8, u8) = (12, 9);

/// Beach sizes the editor offers, smallest first. The same set versus plays
/// on, so a level built here fits any of the arenas the game already
/// draws.
const EDITOR_SIZES: [(u8, u8); 4] = [(9, 7), (12, 9), (16, 11), (20, 13)];
/// The flock dial's stops, K stepping through them in order.
const GULL_PERIODS: [u32; 4] = [0, 480, 240, 120];

/// Where the flock dial goes from `period`. Read off the board each press
/// rather than kept beside it: a board swapped in by F5 or a paste brings
/// its own period, and a remembered stop stepped from wherever the last
/// board was. A period off the dial, as a pasted level may carry, steps
/// to the first stop after off.
fn next_gull_period(period: u32) -> u32 {
    let at = GULL_PERIODS.iter().position(|&p| p == period).unwrap_or(0);
    GULL_PERIODS[(at + 1) % GULL_PERIODS.len()]
}

/// A key that throws the level on the board away, pressed once and waiting
/// to be pressed again. One at a time: pressing anything else, the other
/// one included, takes the offer back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Offer {
    /// F5: a fresh beach of the next size.
    Resize,
    /// F4: the level on the clipboard in place of this one.
    Paste,
}

impl Offer {
    fn key(self) -> KeyCode {
        match self {
            Offer::Resize => KeyCode::F5,
            Offer::Paste => KeyCode::F4,
        }
    }
}

/// Where the editor used to put its one and only level. Still read, so a
/// level saved by an older build is still there; nothing writes it now.
pub fn legacy_save_path() -> std::path::PathBuf {
    crate::app::paths::data_dir().join("levels/custom.txt")
}

/// The shelf every saved level goes on, one file each.
pub fn custom_dir() -> std::path::PathBuf {
    crate::app::paths::data_dir().join("levels/custom")
}

/// Whether anything has been built on this board yet.
///
/// Asked by comparing the level here against the one a fresh board of the
/// same size would give, which is cheaper to be sure of than a list of
/// every way a beach can be marked: a wall, a rock, a pool, a crab, a
/// castle, a gull, the flock, the wrap. A blank beach is worth nothing, so
/// F5 cycles straight past it and only stops to ask once there is a level.
fn has_work(state: &EditorState, board: &Board) -> bool {
    // The same seed, so the comparison is about what has been built and
    // not about where the board came from: a level's text carries its
    // seed, and a blank board with a different one reads as a difference.
    let blank = Board::new(board.width(), board.height(), board.seed());
    level_here(state, board, "x").to_text() != level_here(state, &blank, "x").to_text()
}

/// Whether no level is filed under this path yet. Read before a write,
/// which would make it true either way.
fn is_new_here(path: &std::path::Path) -> bool {
    !path.exists()
}

/// Write a level out, and say whether that brought it into being.
///
/// The answer is read before the write, which would make it true either
/// way. The trophies over this count levels, and they say so: "save 10
/// levels in the editor". Counting every F2 counted keystrokes instead,
/// so ten presses on one level took all three of them.
fn write_level(path: &std::path::Path, text: &str) -> std::io::Result<bool> {
    let is_new = !path.exists();
    crate::app::paths::write_atomic(path, text)?;
    Ok(is_new)
}

/// Where a level called `name` is filed. One file per name, so saving is
/// keeping rather than replacing.
///
/// The name also has to survive being a file name, and every level is
/// identified by its name (that is what progress is keyed on), so two
/// levels sharing one would share their gold star.
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
    pub feedback: String,
    solver: Option<Validation>,
    /// Names this session has already saved under.
    ///
    /// The save path is the name, so F2 writes over whatever is there, and
    /// the editor opens on the same default name every time: two levels
    /// both left unnamed become one, with nothing said. Re-saving your own
    /// work is an overwrite too, and the common one, so the notice is kept
    /// for a name this session did not put there.
    mine: std::collections::HashSet<String>,
    /// A board-replacing key pressed once and waiting to be taken up.
    offer: Option<Offer>,
    /// Statics need a rebuild after a tile/wall edit.
    dirty: bool,
}

/// What the editor is doing, which is what its keys mean. One state rather
/// than a `naming` flag, a `named` flag and a `testing` snapshot, which
/// could all be set at once.
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
        state.name = tidy_name(&state.name, tr);
        state.mode = Mode::JustNamed;
        state.feedback.clear();
    }
}

/// How long a level's name may be.
const NAME_MAX: usize = 28;

/// A name as the editor keeps one, however it arrived: trimmed, no longer
/// than [`NAME_MAX`], and the default name when nothing is left.
///
/// Typing bounds a name key by key, but a pasted level brings whatever its
/// text says, and the level format takes an empty name as readily as a
/// long one. An empty one is a level called nothing, and a long one went
/// past the cap to where saving cuts a file name short, so two long names
/// alike up to there shared one file.
fn tidy_name(raw: &str, tr: &crate::app::i18n::Tr) -> String {
    let capped: String = raw.trim().chars().take(NAME_MAX).collect();
    match capped.trim_end() {
        "" => tr.ed_default_name.to_string(),
        name => name.to_string(),
    }
}

/// True while the editor is spelling out a name, which is when the board
/// keys mean letters. A run condition rather than an early return, because
/// the cursor is walked by a system of its own: typing "Wade" otherwise
/// walks the cursor up and then left across the beach.
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

/// The beach being edited, and everything the editor knows about it.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Workbench<'w> {
    sim: ResMut<'w, Sim>,
    state: ResMut<'w, EditorState>,
}

/// What putting a different board on the sand takes (see
/// [`replace_board`]): the commands, every sprite the old board drew, and
/// the cursors to bring back to the middle.
#[derive(bevy::ecs::system::SystemParam)]
pub struct BoardSwap<'w, 's> {
    commands: Commands<'w, 's>,
    sprites: BoardSprites<'w, 's>,
    cursors: Query<'w, 's, (&'static mut Cursor, &'static mut Transform)>,
}

/// The news an edit makes for the trophies: a level saved, a code handed
/// out, a code taken in.
#[derive(bevy::ecs::system::SystemParam)]
pub struct EditorNews<'w> {
    saved: MessageWriter<'w, crate::app::LevelSaved>,
    shared: MessageWriter<'w, crate::app::CodeShared>,
    taken: MessageWriter<'w, crate::app::CodeTaken>,
}

/// Put a different board on the sand: a resize or a pasted level.
///
/// The cursor comes back to the middle, for the reason the sprites go (see
/// [`swap_board`]): movement only clamps on a step, so on a smaller beach
/// it would stand off the board until it was moved. And a check still
/// running is dropped, because it was handed the board that went and would
/// answer "solvable" or not over this one.
fn replace_board(
    board: Board,
    sim: &mut Sim,
    state: &mut EditorState,
    commands: &mut Commands,
    sprites: &BoardSprites,
    cursors: &mut Query<(&mut Cursor, &mut Transform)>,
) {
    swap_board(board, sim, state, commands, sprites);
    state.solver = None;
    crate::app::cursor::center_on(&sim.0, cursors);
}

/// Put `board` on the sand in place of the one there, which comes back.
/// Every way a board changes other than an edit goes through here: a
/// resize, a paste, and a playtest starting or ending.
///
/// The load path's rule applies (see `BoardSprites` in `session.rs`):
/// every sprite drawn from the old board goes. The sync systems probe the
/// new board at each sprite's remembered tile, and a crab, castle or log
/// left over from a bigger beach probes a tile the smaller one does not
/// have. And they match creatures to sprites by id, with a crab's size,
/// claws and shine set once at its spawn: the playtest's board is read
/// back from text, which numbers crabs afresh from nought, so once a crab
/// had been erased every crab after it was drawn as its neighbour, and a
/// crab a spawner made during the test lent its look to the edited crab
/// that came back on Escape under the same number.
fn swap_board(
    board: Board,
    sim: &mut Sim,
    state: &mut EditorState,
    commands: &mut Commands,
    sprites: &BoardSprites,
) -> Board {
    state.dirty = true;
    sprites.despawn_all(commands);
    std::mem::replace(&mut sim.0, board)
}

/// Tile and creature painting under the cursor: walls, terrain, crabs,
/// and gulls. The editor's command keys live in [`editor_commands`].
pub fn editor_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    settings: Res<GameSettings>,
    bench: Workbench,
    swap: BoardSwap,
) {
    let Workbench { mut sim, mut state } = bench;
    let BoardSwap {
        mut commands,
        sprites,
        mut cursors,
    } = swap;
    // Naming swallows the keyboard: every letter is a letter, not a brush.
    if state.is_naming() {
        type_a_name(&mut typed, &keys, &mut state, settings.tr());
        return;
    }
    typed.clear();
    // Beach size, on a function key. On `[` and `]`, which are *physical*
    // keys, the prompt named keys a non-US keyboard does not have; F5 is F5
    // everywhere, and one key that wraps is enough for four sizes.
    //
    // Anything else said this frame takes an offer back with it, F5's or
    // F4's, the way stepping off the settings row takes back the reset.
    if let Some(offer) = state.offer
        && keys.get_just_pressed().any(|&k| k != offer.key())
    {
        state.offer = None;
    }
    // Resizing starts a fresh beach: there is no honest way to keep a
    // 20-wide layout when it becomes 9 wide.
    //
    // Which is why it asks twice once there is a beach to lose. One press
    // of a key the prompt calls "size" threw away an unsaved level with no
    // warning and nothing to undo it with, while wiping a campaign in
    // Settings, which at least only costs stars, takes two.
    if keys.just_pressed(KeyCode::F5) {
        let board = &sim.0;
        let now = (board.width(), board.height());
        let at = EDITOR_SIZES.iter().position(|&s| s == now).unwrap_or(1);
        let (w, h) = EDITOR_SIZES[(at + 1) % EDITOR_SIZES.len()];
        let tr = settings.tr();
        if state.offer != Some(Offer::Resize) && has_work(&state, board) {
            state.offer = Some(Offer::Resize);
            state.feedback = fill(
                tr.ed_resize_confirm,
                &[("w", &w.to_string()), ("h", &h.to_string())],
            );
            return;
        }
        state.offer = None;
        replace_board(
            Board::new(w, h, 0xED17),
            &mut sim,
            &mut state,
            &mut commands,
            &sprites,
            &mut cursors,
        );
        state.feedback = fill(
            tr.ed_resized,
            &[("w", &w.to_string()), ("h", &h.to_string())],
        );
        return;
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

    // Walls on the cursor tile's edges. The rim of an open-ocean beach is
    // the seam creatures cross and takes no wall (see `Board::set_wall`),
    // which is said rather than the key quietly doing nothing.
    for (key, dir) in ARROWS {
        if keys.just_pressed(key) {
            if board.on_open_rim(x, y, dir) {
                state.feedback = settings.tr().ed_wrap_on.to_string();
                continue;
            }
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
/// Takes the pasted code rather than the clipboard itself, so the three
/// ways this fails (nothing readable there, a code of the wrong sort, a
/// level this build cannot parse) each get their own answer and their own
/// test.
fn level_from(
    pasted: Option<(crate::share::Kind, Vec<u8>)>,
    tr: &crate::app::i18n::Tr,
) -> Result<Level, String> {
    let text =
        crate::app::codes::payload_text(pasted, tr, crate::share::Kind::Level, tr.code_level_bad)?;
    Level::parse(&text).map_err(|e| fill(tr.code_level_bad, &[("e", &e)]))
}

/// Whether a pasted level came from somebody else, which is what the
/// trophy for taking a code is about: "open a level from someone else's
/// code". Not when it is the level already on the bench, which is F3 and
/// then F4, and not when it is one of the player's own saved levels coming
/// back to them. Compared as the text a file would hold, name and all, so
/// a friend's edit of your level, or your level under their name, is
/// theirs.
fn is_from_elsewhere(pasted: &Level, bench: &Level, shelf: &[Level]) -> bool {
    let text = pasted.to_text();
    text != bench.to_text() && shelf.iter().all(|own| own.to_text() != text)
}

/// Take on what a pasted level says about itself: its signpost grant, its
/// kind and its name.
///
/// The kind, because opening somebody's beach and saving it back as a
/// stage is not what either of you meant. The name, because the one on the
/// bench belongs to the level the paste replaced, and F2 filed the pasted
/// board over the author's own file under it, without a word when this
/// session had saved it. And that name is taken back out of the ones this
/// session has saved under, so saving over a file already called that
/// says so.
fn take_level(state: &mut EditorState, level: &Level, tr: &crate::app::i18n::Tr) {
    state.posts = level.posts;
    state.kind = level.kind;
    state.name = tidy_name(&level.name, tr);
    state.mine.remove(&state.name);
}

/// The level as it stands on the sand, under the name and the kind the
/// author has chosen. Every command that turns the board into a level goes
/// through here, so saving, sharing and checking cannot disagree about what
/// is being built.
///
/// The rule is where the two kinds part. A puzzle is played with the
/// inventory it grants and no evictions; an arena keeps the board's own
/// rule, which is the one every generated beach plays under. Stamping the
/// puzzle rule on a handmade beach gives a versus table three signposts
/// each and no way to replace them.
///
/// A puzzle also plays without castle raids, as every campaign puzzle
/// does: its castle is the finish line, and a gull leaving through it
/// would move the target.
///
/// What comes back is read from the text that will be saved, not the board
/// on screen. The two differ when gulls were placed and erased: each
/// placement draws from the board's PRNG, the text carries only the seed,
/// and the survivors read back with a different hand and takeoff, so a
/// "solvable" certified on the live board is about a beach nobody plays.
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
/// every key is a letter, or an "o" in the name flips wrap and Escape
/// leaves for the menu. The frame the name is committed is sat out here
/// (see `EditorState::named`), because the gate lifts within that frame.
pub fn editor_commands(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<GameSettings>,
    bench: Workbench,
    mut clipboard: ResMut<Clipboard>,
    news: EditorNews,
    mut next_screen: ResMut<NextState<Screen>>,
    swap: BoardSwap,
) {
    let Workbench { mut sim, mut state } = bench;
    let EditorNews {
        mut saved,
        mut shared,
        mut taken,
    } = news;
    let BoardSwap {
        mut commands,
        sprites,
        mut cursors,
    } = swap;
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
    // A paste throws the board away as surely as F5 does, so once there is
    // a level here it asks twice the same way.
    if keys.just_pressed(KeyCode::F4) {
        match level_from(crate::app::codes::paste(&mut clipboard), tr) {
            Ok(_) if state.offer != Some(Offer::Paste) && has_work(&state, &sim.0) => {
                state.offer = Some(Offer::Paste);
                state.feedback = tr.ed_paste_confirm.into();
            }
            Ok(level) => {
                state.offer = None;
                let bench = level_here(&state, &sim.0, &state.name);
                let shelf = crate::app::campaign::load_custom_levels();
                if is_from_elsewhere(&level, &bench, &shelf) {
                    taken.write(crate::app::CodeTaken);
                }
                take_level(&mut state, &level, tr);
                // A pasted level is any size, so it is a board swap in
                // full, sprites and cursor included.
                replace_board(
                    level.board(),
                    &mut sim,
                    &mut state,
                    &mut commands,
                    &sprites,
                    &mut cursors,
                );
                // A pasted level is the one board here that nobody vetted:
                // authored elsewhere, carried through a chat message, and
                // dropped straight onto the sand. Checked on the way in,
                // which for a beach is a count of its castles.
                if level.kind == LevelKind::Arena {
                    state.feedback = arena_report(&sim.0, tr);
                } else {
                    start_validation(&mut state, level);
                    state.feedback = tr.code_level_checking.into();
                }
            }
            Err(complaint) => {
                state.offer = None;
                state.feedback = complaint;
            }
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
        let period = next_gull_period(board.gull_period());
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
        let stranger = !is_new_here(&path) && !state.mine.contains(&state.name);
        state.feedback = match write_level(&path, &level.to_text()) {
            Ok(is_new) => {
                if is_new {
                    saved.write(crate::app::LevelSaved);
                }
                let named = state.name.clone();
                state.mine.insert(named);
                let filed = fill(tr.ed_saved_to, &[("path", &path.display().to_string())]);
                let orphans = orphan_warning(&state, &level, tr);
                let notes = [stranger.then_some(tr.ed_saved_over), orphans.as_deref()];
                notes
                    .iter()
                    .flatten()
                    .fold(filed, |line, note| format!("{line} - {note}"))
            }
            Err(e) => fill(tr.ed_save_failed, &[("e", &e.to_string())]),
        };
    }
    if crate::app::menu_ui::enter(&keys) {
        // The board being edited is put aside whole, to come back on Esc,
        // and the playtest runs on the level the file will hold: the same
        // board saving, sharing and the solver are handed. That is the
        // rule it will be played under (a stage's granted inventory and no
        // raids, a beach's own versus rule) and the gulls as the file's
        // seed rolls them, which is not how they stand on a board that
        // placed and erased a few.
        let played = level_here(&state, board, &state.name).board();
        let snapshot = swap_board(played, &mut sim, &mut state, &mut commands, &sprites);
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
    swap: BoardSwap,
) {
    let BoardSwap {
        mut commands,
        sprites,
        cursors,
    } = swap;
    let Some((cursor, _)) = cursors.iter().next() else {
        return;
    };
    for (key, dir) in ARROWS {
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
        swap_board(*snapshot, &mut sim, &mut state, &mut commands, &sprites);
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
    /// with the survivors rolled afresh.
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

    /// A beach keeps its raids: the finish-line reasoning that turns them
    /// off for a stage does not apply where the goal is a score, and the
    /// versus beaches all play with them on.
    #[test]
    fn a_beach_from_the_editor_keeps_its_raids() {
        let mut board = sand();
        board.set_tile(0, 0, TileKind::Castle(0));
        board.set_tile(5, 4, TileKind::Castle(1));
        let state = EditorState {
            posts: 3,
            kind: LevelKind::Arena,
            ..EditorState::default()
        };
        let beach = level_here(&state, &board, "Beach");
        assert!(beach.board().castle_raids());
        assert!(!beach.to_text().contains("raids:"));
    }

    /// The toggle decides two things at once: which list the saved level
    /// joins, and which signpost rule it is played under. A beach saved
    /// under the puzzle rule hands a versus table three posts each and no
    /// way to replace them.
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

    /// A level counts once, when it comes into being. The trophies over
    /// this say "save 10 levels", and every F2 used to raise one, so ten
    /// presses on a single level took all three of them.
    #[test]
    fn saving_the_same_level_again_does_not_build_a_second_one() {
        let dir = std::env::temp_dir().join(format!("pinch-built-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory to save into");
        let path = dir.join("my-beach.txt");

        assert!(write_level(&path, "one").expect("wrote"), "a new level");
        assert!(
            !write_level(&path, "two").expect("wrote"),
            "the same level again"
        );
        assert!(
            !write_level(&path, "three").expect("wrote"),
            "and again, however long the key is held"
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("reads"),
            "three",
            "every save still lands; only the counting stops"
        );

        // A level deleted and rebuilt is a level built again.
        std::fs::remove_file(&path).expect("removed");
        assert!(write_level(&path, "four").expect("wrote"), "built afresh");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// F5 throws the beach away, so once there is one it asks twice. One
    /// press of a key the prompt calls "size" used to take an unsaved
    /// level with it, and the editor has no undo and no load.
    #[test]
    fn resizing_a_beach_with_a_level_on_it_asks_twice() {
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
        app.add_message::<bevy::input::keyboard::KeyboardInput>();
        // F5 is read by the painting system, beside the cursor it moves,
        // not by the command keys.
        app.add_systems(Update, editor_input);

        let tap = |app: &mut App, key: KeyCode| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(key);
            app.update();
        };
        let size = |app: &mut App| {
            let board = &app.world().resource::<Sim>().0;
            (board.width(), board.height())
        };

        // A blank beach is worth nothing, so the dial just turns.
        let first = size(&mut app);
        tap(&mut app, KeyCode::F5);
        assert_ne!(size(&mut app), first, "an empty beach cycles straight");

        // Put something on it: now the first press only offers.
        app.world_mut().resource_mut::<Sim>().0.set_wrap(true);
        let built = size(&mut app);
        tap(&mut app, KeyCode::F5);
        assert_eq!(size(&mut app), built, "the level is still there");
        assert_eq!(
            app.world().resource::<EditorState>().offer,
            Some(Offer::Resize)
        );
        tap(&mut app, KeyCode::F5);
        assert_ne!(size(&mut app), built, "and the second press takes it");
        assert_eq!(app.world().resource::<EditorState>().offer, None);

        // An offer not taken up goes away when anything else is pressed.
        app.world_mut().resource_mut::<Sim>().0.set_wrap(true);
        let built = size(&mut app);
        tap(&mut app, KeyCode::F5);
        assert_eq!(
            app.world().resource::<EditorState>().offer,
            Some(Offer::Resize)
        );
        tap(&mut app, KeyCode::KeyO);
        assert!(
            app.world().resource::<EditorState>().offer.is_none(),
            "another key takes the offer back"
        );
        tap(&mut app, KeyCode::F5);
        assert_eq!(size(&mut app), built, "so this press only offers again");
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

    /// The playtest runs the level the file will hold, not the board as it
    /// stands: a stage plays without raids, and gulls placed and erased
    /// are rolled from the seed the file carries. Escape puts the board
    /// being edited back exactly as it was, stray PRNG draws and all.
    #[test]
    fn the_playtest_plays_the_level_the_file_will_hold() {
        let mut board = sand();
        board.set_tile(0, 0, TileKind::Castle(0));
        board.spawn_crab(2, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        board.spawn_gull(3, 3, Direction::Right);
        board.remove_gulls_at(3, 3);
        board.spawn_gull(1, 3, Direction::Right);
        let editing = board.state_hash();

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.add_message::<crate::app::LevelSaved>();
        app.add_message::<crate::app::CodeShared>();
        app.add_message::<crate::app::CodeTaken>();
        app.insert_resource(Sim(board.clone()));
        app.insert_resource(EditorState {
            posts: 3,
            name: "Stage".into(),
            ..EditorState::default()
        });
        app.init_resource::<GameSettings>();
        app.init_resource::<Clipboard>();
        app.init_resource::<ButtonInput<KeyCode>>();
        // The test's own keys need a cursor to stand on, as in the editor.
        app.world_mut()
            .spawn((Cursor::seated(0), Transform::default()));
        app.add_systems(
            Update,
            (
                editor_commands.run_if(|state: Res<EditorState>| !state.is_testing()),
                editor_test_input.run_if(editor_testing),
            )
                .chain(),
        );
        let tap = |app: &mut App, key: KeyCode| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(key);
            app.update();
        };

        tap(&mut app, KeyCode::Enter);
        let state = app.world().resource::<EditorState>();
        assert!(state.is_testing());
        let played = &app.world().resource::<Sim>().0;
        let filed = level_here(state, &board, "Stage").board();
        assert_eq!(played.state_hash(), filed.state_hash(), "the file's level");
        assert!(!played.castle_raids(), "a stage plays without raids");
        assert_ne!(played.state_hash(), editing, "not the board as it stood");

        tap(&mut app, KeyCode::Escape);
        assert!(!app.world().resource::<EditorState>().is_testing());
        assert_eq!(
            app.world().resource::<Sim>().0.state_hash(),
            editing,
            "the board being edited comes back as it was"
        );
    }

    /// Starting and ending a playtest swaps boards, so both take the old
    /// board's creature sprites with them. The playtest's crabs are
    /// numbered afresh, and a sprite kept by number wore another crab's
    /// size and claws.
    #[test]
    fn a_playtest_redraws_its_creatures_both_ways() {
        let mut board = sand();
        board.set_tile(0, 0, TileKind::Castle(0));
        board.spawn_crab(2, 2, Direction::Right, Handedness::Left, CrabKind::Giant);
        board.spawn_crab(3, 2, Direction::Right, Handedness::Right, CrabKind::Common);
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.add_message::<crate::app::LevelSaved>();
        app.add_message::<crate::app::CodeShared>();
        app.add_message::<crate::app::CodeTaken>();
        app.insert_resource(Sim(board));
        app.insert_resource(EditorState {
            posts: 3,
            name: "Stage".into(),
            ..EditorState::default()
        });
        app.init_resource::<GameSettings>();
        app.init_resource::<Clipboard>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.world_mut()
            .spawn((Cursor::seated(0), Transform::default()));
        app.add_systems(
            Update,
            (
                editor_commands.run_if(|state: Res<EditorState>| !state.is_testing()),
                editor_test_input.run_if(editor_testing),
            )
                .chain(),
        );
        let tap = |app: &mut App, key: KeyCode| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(key);
            app.update();
        };
        let draw = |app: &mut App| {
            let world = app.world_mut();
            let crab = crate::app::creatures::CrabSprite {
                id: 1,
                kind: CrabKind::Common,
                shade: 0.0,
            };
            world.spawn(crab);
            world.spawn(crate::app::creatures::GullSprite(0));
        };
        let drawn = |app: &mut App| {
            let world = app.world_mut();
            let crabs = world
                .query::<&crate::app::creatures::CrabSprite>()
                .iter(world)
                .count();
            let gulls = world
                .query::<&crate::app::creatures::GullSprite>()
                .iter(world)
                .count();
            crabs + gulls
        };

        draw(&mut app);
        tap(&mut app, KeyCode::Enter);
        assert!(app.world().resource::<EditorState>().is_testing());
        assert_eq!(drawn(&mut app), 0, "the edited board's sprites went");

        draw(&mut app);
        tap(&mut app, KeyCode::Escape);
        assert!(!app.world().resource::<EditorState>().is_testing());
        assert_eq!(drawn(&mut app), 0, "and so did the playtest's");
    }

    /// A check still running was handed the board a resize threw away, so
    /// the resize drops it rather than let it answer for the fresh beach.
    #[test]
    fn a_resize_drops_the_check_on_the_old_beach() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(Sim(sand()));
        app.insert_resource(EditorState {
            solver: Some(Validation {
                slot: std::sync::Arc::new(std::sync::Mutex::new(None)),
                posts: 3,
            }),
            ..EditorState::default()
        });
        app.init_resource::<GameSettings>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.add_message::<bevy::input::keyboard::KeyboardInput>();
        app.add_systems(Update, editor_input);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F5);
        app.update();
        let width = app.world().resource::<Sim>().0.width();
        assert_ne!(width, sand().width(), "the blank beach was resized");
        assert!(app.world().resource::<EditorState>().solver.is_none());
    }

    /// A pasted name is kept the way a typed one is: trimmed, capped, and
    /// the default when there is nothing to it.
    #[test]
    fn a_pasted_name_is_tidied_like_a_typed_one() {
        let tr = &crate::app::i18n::EN;
        let named = |name: &str| {
            let mut state = EditorState::default();
            let mut level = Level::from_board("x", 3, sand());
            level.name = name.into();
            take_level(&mut state, &level, tr);
            state.name
        };
        assert_eq!(named("  Gull Alley "), "Gull Alley");
        assert_eq!(named(""), tr.ed_default_name);
        assert_eq!(named("   "), tr.ed_default_name);
        let long = named(&"\u{e9}".repeat(NAME_MAX + 12));
        assert_eq!(long.chars().count(), NAME_MAX, "cut on characters");
        // A cut landing on a space leaves no trailing space behind it.
        let spaced = format!("{} tail", "a".repeat(NAME_MAX - 1));
        assert_eq!(named(&spaced), "a".repeat(NAME_MAX - 1));
    }

    /// The flock dial steps from whatever board is on the sand, so a board
    /// a paste or a resize swapped in starts the dial from its own period
    /// rather than from where the last board's left it.
    #[test]
    fn the_flock_dial_steps_from_the_board_it_is_on() {
        assert_eq!(next_gull_period(0), GULL_PERIODS[1]);
        assert_eq!(next_gull_period(GULL_PERIODS[1]), GULL_PERIODS[2]);
        assert_eq!(next_gull_period(GULL_PERIODS[3]), 0, "and round to off");
        assert_eq!(next_gull_period(333), GULL_PERIODS[1], "off the dial");
    }

    /// The trophy for taking a code is about somebody else's level, so the
    /// one already on the bench (F3 then F4) and the player's own saved
    /// levels coming back to them do not count. An edit of theirs does.
    #[test]
    fn only_a_level_from_elsewhere_counts_as_taken() {
        let mut board = sand();
        board.set_tile(0, 0, TileKind::Castle(0));
        let mine = Level::from_board("Mine", 3, board.clone());
        board.set_tile(2, 2, TileKind::Rock);
        let bench = Level::from_board("Bench", 3, board.clone());
        let shelf = [mine.clone()];
        assert!(!is_from_elsewhere(&bench, &bench, &shelf), "F3 then F4");
        assert!(!is_from_elsewhere(&mine, &bench, &shelf), "my own, back");
        board.set_tile(3, 3, TileKind::Rock);
        let theirs = Level::from_board("Mine", 3, board);
        assert!(
            is_from_elsewhere(&theirs, &bench, &shelf),
            "their edit of mine"
        );
    }

    /// A pasted level brings its name, so the next F2 files it under that
    /// and not over the level it replaced, and saving over a file already
    /// called that says so even when this session saved one under it.
    #[test]
    fn a_pasted_level_brings_its_name() {
        let mut state = EditorState {
            posts: 3,
            name: "My Beach".into(),
            ..EditorState::default()
        };
        state.mine.insert("My Beach".into());
        state.mine.insert("Gull Alley".into());
        let pasted = Level::from_board("Gull Alley", 5, sand()).with_kind(LevelKind::Arena);
        take_level(&mut state, &pasted, &crate::app::i18n::EN);
        assert_eq!(state.name, "Gull Alley");
        assert_eq!((state.posts, state.kind), (5, LevelKind::Arena));
        assert!(!state.mine.contains("Gull Alley"), "a save over it is said");
        assert!(state.mine.contains("My Beach"), "the rest are left alone");
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
