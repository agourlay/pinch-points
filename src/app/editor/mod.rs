//! Driftwood, the level editor (spec §5.4): author walls, castles, spawners,
//! rocks, crabs, and gulls on a live board; playtest in place; validate
//! solvability by brute-force against the headless sim; save to disk in the
//! text level format.

mod brush;
mod naming;
mod palette;
mod share;
mod shelf;
mod validate;

pub use brush::Brush;
pub use naming::editor_naming;
pub use palette::{EditorUi, spawn_editor_ui, update_editor_palette};
pub use shelf::{custom_dir, legacy_save_path};
pub use validate::poll_solver;

use brush::paint;
use naming::type_a_name;
use validate::{Validation, arena_report, start_validation};

use crate::app::cursor::Cursor;
use crate::app::i18n::fill;
use crate::app::session::BoardSprites;
use crate::app::settings::GameSettings;
use crate::app::{Screen, Sim};
use crate::sim::{Board, Direction, Goal, Level, LevelKind};
use bevy::prelude::*;

/// The arrow keys and the edge each names: a wall toggled on that side of
/// the cursor's tile, or, in a playtest, the way an arrow points.
const ARROWS: [(KeyCode, Direction); 4] = [
    (KeyCode::ArrowUp, Direction::Up),
    (KeyCode::ArrowDown, Direction::Down),
    (KeyCode::ArrowLeft, Direction::Left),
    (KeyCode::ArrowRight, Direction::Right),
];

/// The editor's keys that follow P1's bindings, read by the editor and
/// named by its prompts from this one answer.
pub(crate) struct EditorKeys {
    /// P1's movement keys, which walk the editor's cursor whatever they
    /// are bound to (see `cursor::move_cursor`).
    pub moves: [KeyCode; 4],
    /// The wall toggles: the arrow keys, or P1's place keys while the
    /// arrows are P1's movement keys, which would leave no key to build a
    /// wall with.
    pub walls: [(KeyCode, Direction); 4],
    /// A playtest's placements and its removal: P1's, as a puzzle reads
    /// them.
    pub place: [(KeyCode, Direction); 4],
    pub remove: KeyCode,
}

impl EditorKeys {
    pub(crate) fn of(settings: &GameSettings) -> Self {
        let map = crate::app::cursor::keymap(settings, 0, settings.commit);
        let moves = map.move_keys();
        let walls = match ARROWS.iter().any(|(key, _)| moves.contains(key)) {
            true => map.places,
            false => ARROWS,
        };
        Self {
            moves,
            walls,
            place: map.places,
            remove: map.remove,
        }
    }

    /// A press the editor's own keys may answer: just pressed, and neither one
    /// of P1's movement keys nor a wall key.
    ///
    /// The cursor walks on P1's bindings, and the brushes and commands are
    /// keys of their own, so a binding moved onto one did two things at once:
    /// rebound to ESDF, E walked the cursor up and painted kelp on every step.
    /// Movement wins, being the binding the player chose and the one the
    /// prompt names, and so do the walls when they have moved onto P1's place
    /// keys; a brush they shadow is still reached with Tab and Space.
    fn command_press<'a>(
        &'a self,
        keys: &'a ButtonInput<KeyCode>,
    ) -> impl Fn(KeyCode) -> bool + 'a {
        let Self { moves, walls, .. } = self;
        move |key| {
            keys.just_pressed(key)
                && !moves.contains(&key)
                && !walls.iter().any(|(wall, _)| *wall == key)
        }
    }
}

/// Four keys as a prompt names them, in the order "WASD" spells a block
/// (up, left, down, right): run together when each cap is one character,
/// "ESDF", else "Up/Left/Down/Right".
pub(crate) fn spell_block(caps: &crate::app::keycaps::KeyCaps, keys: [KeyCode; 4]) -> String {
    let labels = keys.map(|key| caps.label(key));
    match labels.iter().all(|label| label.chars().count() == 1) {
        true => labels.concat(),
        false => labels.join("/"),
    }
}

/// A group of four keys in the order "WASD" spells a block (up, left,
/// down, right), as [`spell_block`] takes them.
fn block_order(group: [(KeyCode, Direction); 4]) -> [KeyCode; 4] {
    [
        Direction::Up,
        Direction::Left,
        Direction::Down,
        Direction::Right,
    ]
    .map(|dir| {
        group
            .iter()
            .find(|(_, d)| *d == dir)
            .map(|(key, _)| *key)
            .expect("a key for every way")
    })
}

/// `text` with the editor's `{moves}`, `{walls}`, `{place}` and
/// `{remove}` keys filled in: the caps of the keys [`EditorKeys`] reads,
/// or the language's own words while the walls and the playtest's keys
/// are the stock arrow keys and Space.
///
/// Run after [`crate::app::keycaps::KeyCaps::legend`]: the keys are
/// already in this keyboard's caps, and caps that happen to read "WASD"
/// would be respelled again as the QWERTY keys the legend means by it.
pub(crate) fn spell_editor_keys(
    text: &str,
    tr: &crate::app::i18n::Tr,
    caps: &crate::app::keycaps::KeyCaps,
    settings: &GameSettings,
) -> String {
    let keys = EditorKeys::of(settings);
    let block = |group: [(KeyCode, Direction); 4]| -> String {
        match group == ARROWS {
            true => tr.ed_arrow_keys.to_string(),
            false => spell_block(caps, block_order(group)),
        }
    };
    let [up, down, left, right] = keys.moves;
    let moves = spell_block(caps, [up, left, down, right]);
    let remove = if keys.remove == KeyCode::Space {
        tr.ed_space_key.to_string()
    } else {
        caps.label(keys.remove)
    };
    fill(
        text,
        &[
            ("moves", &moves),
            ("walls", &block(keys.walls)),
            ("place", &block(keys.place)),
            ("remove", &remove),
        ],
    )
}

/// Whether "+" (`up`) or "-" was pressed for the posts dial. The prompt
/// says "+/-" in every language, so the keys are the ones that say so:
/// the US pair beside Backspace, the keypad's, and whichever key this
/// keyboard's caps put the sign on.
fn dial_press(
    pressed: &impl Fn(KeyCode) -> bool,
    keys: &ButtonInput<KeyCode>,
    caps: &crate::app::keycaps::KeyCaps,
    up: bool,
) -> bool {
    let (us, pad, sign) = if up {
        (KeyCode::Equal, KeyCode::NumpadAdd, '+')
    } else {
        (KeyCode::Minus, KeyCode::NumpadSubtract, '-')
    };
    pressed(us) || pressed(pad) || caps.pressed_caps(keys, sign).any(pressed)
}

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
    /// F2: this level saved over the file of a different one.
    Overwrite,
    /// Esc: back to the menu, with what has not been saved left behind.
    Leave,
}

impl Offer {
    fn key(self) -> KeyCode {
        match self {
            Offer::Resize => KeyCode::F5,
            Offer::Paste => KeyCode::F4,
            Offer::Overwrite => KeyCode::F2,
            Offer::Leave => KeyCode::Escape,
        }
    }
}

/// Whether to go ahead with what `offer`'s key throws away.
///
/// With nothing `at_stake` it goes ahead at once. With something, the
/// first press only says what is about to go (`prompt`) and leaves the
/// offer standing, and a second press of the same key takes it up. Going
/// ahead always lets the offer go.
fn ask_first(state: &mut EditorState, offer: Offer, at_stake: bool, prompt: String) -> bool {
    if at_stake && state.offer != Some(offer) {
        state.offer = Some(offer);
        state.feedback = prompt;
        return false;
    }
    state.offer = None;
    true
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

/// Whether leaving now would lose work: something is built here, and it
/// is not the level F2 last wrote. Esc asks first on it, as F5 and F4 do
/// on [`has_work`], but a level just saved is not lost by leaving.
fn unsaved_work(state: &EditorState, board: &Board) -> bool {
    let here = level_here(state, board, &state.name).to_text();
    has_work(state, board) && state.filed.as_deref() != Some(here.as_str())
}

#[derive(Resource, Default)]
pub struct EditorState {
    pub posts: u8,
    /// What is being built: a stage for the Tide Pool list, or a beach for
    /// the versus map dial. The author says which rather than the game
    /// counting castles and deciding for them.
    pub kind: LevelKind,
    /// What the stage asks of the player. The editor has no key to change
    /// it, but a pasted code can bring a Beach Day goal, and the bench
    /// keeps it so that saving, sharing, playtesting and the check all hand
    /// on the level that was pasted rather than an every-crab stage.
    pub goal: Goal,
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
    /// The level's text as F2 last wrote it, so Esc can tell work already
    /// on the shelf from work that would be lost (see [`unsaved_work`]).
    filed: Option<String>,
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
    /// `was` is the name F1 found, which Escape puts back.
    Naming { was: String },
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
        matches!(self.mode, Mode::Naming { .. })
    }

    pub fn is_testing(&self) -> bool {
        matches!(self.mode, Mode::Testing(_))
    }

    /// The level changed under the author's hand: the statics are redrawn,
    /// and a check still running is dropped, because it was handed the
    /// board as it stood and would answer "solvable" or not over this one
    /// (and V would say it is still checking until it ran out). Its
    /// "checking..." goes with it. Every author edit goes through here, so
    /// none can forget the search.
    fn edited(&mut self) {
        self.dirty = true;
        if self.solver.take().is_some() {
            self.feedback.clear();
        }
    }
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
/// it would stand off the board until it was moved. And it is an edit
/// (see [`EditorState::edited`]): a check still running was handed the
/// board that went.
fn replace_board(board: Board, sim: &mut Sim, state: &mut EditorState, swap: &mut BoardSwap) {
    swap_board(board, sim, state, &mut swap.commands, &swap.sprites);
    state.edited();
    crate::app::cursor::center_on(&sim.0, &mut swap.cursors);
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
    mut swap: BoardSwap,
) {
    let Workbench { mut sim, mut state } = bench;
    // Naming swallows the keyboard: every letter is a letter, not a brush.
    if state.is_naming() {
        type_a_name(&mut typed, &keys, &mut state, settings.tr());
        return;
    }
    typed.clear();
    let editor_keys = EditorKeys::of(&settings);
    let pressed = editor_keys.command_press(&keys);
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
    if pressed(KeyCode::F5) {
        let board = &sim.0;
        let now = (board.width(), board.height());
        let at = EDITOR_SIZES.iter().position(|&s| s == now).unwrap_or(1);
        let (w, h) = EDITOR_SIZES[(at + 1) % EDITOR_SIZES.len()];
        let tr = settings.tr();
        let at_stake = has_work(&state, board);
        let prompt = fill(
            tr.ed_resize_confirm,
            &[("w", &w.to_string()), ("h", &h.to_string())],
        );
        if !ask_first(&mut state, Offer::Resize, at_stake, prompt) {
            return;
        }
        replace_board(Board::new(w, h, 0xED17), &mut sim, &mut state, &mut swap);
        state.feedback = fill(
            tr.ed_resized,
            &[("w", &w.to_string()), ("h", &h.to_string())],
        );
        return;
    }
    if pressed(KeyCode::F1) {
        state.mode = Mode::Naming {
            was: state.name.clone(),
        };
        state.feedback = settings.tr().ed_naming.to_string();
        return;
    }
    let Some((cursor, _)) = swap.cursors.iter().next() else {
        return;
    };
    let (x, y) = (cursor.x, cursor.y);
    let board = &mut sim.0;

    // Walls on the cursor tile's edges. The rim of an open-ocean beach is
    // the seam creatures cross and takes no wall (see `Board::set_wall`),
    // which is said rather than the key quietly doing nothing.
    for (key, dir) in editor_keys.walls {
        if keys.just_pressed(key) {
            if board.on_open_rim(x, y, dir) {
                state.feedback = settings.tr().ed_wrap_on.to_string();
                continue;
            }
            let present = board.wall_at(x, y, dir);
            board.set_wall(x, y, dir, !present);
            state.edited();
        }
    }

    // Pick a brush and paint with it. The letters do both, so a hand that
    // knows the old editor keeps working and the palette simply shows it
    // what it just chose; Tab walks the palette for a hand that does not.
    for brush in Brush::ALL {
        if pressed(brush.key()) {
            state.brush = brush;
            paint(board, x, y, brush);
            state.edited();
        }
    }
    if pressed(KeyCode::Tab) {
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
    if pressed(KeyCode::Space) {
        paint(board, x, y, state.brush);
        state.edited();
    }
}

/// The level as it stands on the sand, under the name and the kind the
/// author has chosen. Every command that turns the board into a level goes
/// through here, so saving, sharing and checking cannot disagree about what
/// is being built.
///
/// The rule is where the two kinds part. A puzzle is played with the
/// inventory it grants and no evictions; an arena gets the stock rule,
/// which is the one every generated beach plays under. Stamping the
/// puzzle rule on a handmade beach gives a versus table three signposts
/// each and no way to replace them.
///
/// A puzzle also plays without castle raids, as every campaign puzzle
/// does: its castle is the finish line, and a gull leaving through it
/// would move the target. An arena plays with them, as every versus beach
/// does, and versus never turns them back on. Stamped both ways because
/// the board on the bench can carry the other kind's: a puzzle code
/// pasted in and switched to an arena with F6 brought `raids: off` and
/// its granted rule along, and saved a beach that never raided.
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
        snapshot.rules_mut().castle_raids = false;
    } else {
        let stock = crate::sim::Rules::default();
        snapshot.set_signpost_rule(stock.signpost_cap, stock.cap_policy);
        snapshot.rules_mut().castle_raids = stock.castle_raids;
    }
    let mut level = Level::from_board(name, state.posts, snapshot).with_kind(state.kind);
    level.goal = state.goal;
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
    keyboard: crate::app::keycaps::Keyboard,
    settings: Res<GameSettings>,
    bench: Workbench,
    mut clipboard: ResMut<Clipboard>,
    news: EditorNews,
    mut next_screen: ResMut<NextState<Screen>>,
    mut swap: BoardSwap,
) {
    let Workbench { mut sim, mut state } = bench;
    let crate::app::keycaps::Keyboard { keys, caps, .. } = keyboard;
    let EditorNews {
        mut saved,
        mut shared,
        mut taken,
    } = news;
    if matches!(state.mode, Mode::JustNamed) {
        state.mode = Mode::Painting;
        return;
    }
    let tr = settings.tr();
    let editor_keys = EditorKeys::of(&settings);
    let pressed = editor_keys.command_press(&keys);
    if pressed(KeyCode::F3) {
        share::copy_level(&mut state, &sim.0, &mut clipboard, tr, &mut shared);
    }
    if pressed(KeyCode::F4) {
        share::paste_level(
            &mut state,
            &mut sim,
            &mut clipboard,
            tr,
            &mut taken,
            &mut swap,
        );
    }
    let board = &mut sim.0;
    // The signpost inventory is a puzzle's rule and nothing else: on a
    // beach the versus rule governs, so the dial would be turning a number
    // the match never reads. It keeps its value across a trip through arena
    // mode rather than being zeroed.
    if state.kind == LevelKind::Puzzle {
        // Up to nine by the dial; a pasted level may carry more, which
        // "+" leaves alone rather than lowering (or, at 255, overflowing).
        // A turn that moves the count is an edit like any other: a check
        // running on the old count is dropped, not left to answer for it.
        let was = state.posts;
        if dial_press(&pressed, &keys, &caps, true) && state.posts < 9 {
            state.posts += 1;
        }
        if dial_press(&pressed, &keys, &caps, false) {
            state.posts = state.posts.saturating_sub(1);
        }
        if state.posts != was {
            state.edited();
        }
    }
    if pressed(KeyCode::KeyO) {
        let wrap = !board.wrap();
        board.set_wrap(wrap);
        state.edited();
        state.feedback = if wrap {
            tr.ed_wrap_on.into()
        } else {
            tr.ed_wrap_off.into()
        };
    }
    if pressed(KeyCode::KeyK) {
        let period = next_gull_period(board.rules().gull_period);
        board.rules_mut().gull_period = period;
        state.edited();
        state.feedback = if period == 0 {
            tr.ed_gulls_off.into()
        } else {
            // In seconds, as a player reads time: the period is kept in
            // ticks, which mean nothing off the board.
            let secs = period as f32 / crate::sim::TICKS_PER_SECOND as f32;
            fill(tr.ed_gulls_every, &[("s", &format!("{secs}"))])
        };
    }

    if pressed(KeyCode::KeyV) {
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
    if pressed(KeyCode::F6) {
        // A puzzle's verdict over a beach, or a search a beach never asked
        // for, would land on the other kind.
        state.edited();
        state.kind = match state.kind {
            LevelKind::Puzzle => LevelKind::Arena,
            LevelKind::Arena => LevelKind::Puzzle,
        };
        state.feedback = match state.kind {
            LevelKind::Puzzle => tr.ed_now_puzzle.into(),
            LevelKind::Arena => tr.ed_now_arena.into(),
        };
    }
    if pressed(KeyCode::F2) {
        shelf::save_level(&mut state, board, tr, &mut saved);
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
        let snapshot = swap_board(
            played,
            &mut sim,
            &mut state,
            &mut swap.commands,
            &swap.sprites,
        );
        state.mode = Mode::Testing(Box::new(snapshot));
        state.feedback = spell_editor_keys(tr.ed_playtest_prompt, tr, &caps, &settings);
    }
    // Leaving resets the bench, and the editor has no load: the level goes
    // with it unless it is on the shelf, so Esc asks first, as F5 does.
    if pressed(KeyCode::Escape) {
        let at_stake = unsaved_work(&state, &sim.0);
        if ask_first(
            &mut state,
            Offer::Leave,
            at_stake,
            tr.ed_leave_confirm.into(),
        ) {
            next_screen.set(Screen::Menu);
        }
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
    // P1's place and remove keys, as a puzzle reads them: the arrows were
    // fixed here, and unreachable once P1 moved on them.
    let EditorKeys { place, remove, .. } = EditorKeys::of(&settings);
    for (key, dir) in place {
        if keys.just_pressed(key) {
            let _ = sim.0.place_signpost(0, cursor.x, cursor.y, dir);
        }
    }
    if keys.just_pressed(remove) {
        let _ = sim.0.remove_signpost(0, cursor.x, cursor.y);
    }
    // Only Escape ends the test: Enter started it this very frame, and both
    // systems see the same just_pressed set.
    if keys.just_pressed(KeyCode::Escape)
        && end_playtest(&mut sim, &mut state, &mut commands, &sprites)
    {
        state.feedback = settings.tr().ed_back.into();
    }
}

/// End a playtest, if one is running, with the board it was started from
/// back on the sand; and say whether one was.
///
/// Asked before the mode is taken, not after: taking it puts the default
/// in its place, so a recovery in the middle of naming would otherwise
/// end the naming too.
fn end_playtest(
    sim: &mut Sim,
    state: &mut EditorState,
    commands: &mut Commands,
    sprites: &BoardSprites,
) -> bool {
    if !state.is_testing() {
        return false;
    }
    let Mode::Testing(snapshot) = std::mem::take(&mut state.mode) else {
        return false;
    };
    swap_board(*snapshot, sim, state, commands, sprites);
    true
}

/// The editor after something in it failed and was caught (see
/// `failsafe`): a playtest stops, with the board it was started from back
/// on the sand, and the author stays at the bench with the level they were
/// building. Sent to the menu instead, they lost every unsaved tile of it
/// for a fault that was the game's.
pub(super) fn recover(
    sim: &mut Sim,
    state: &mut EditorState,
    commands: &mut Commands,
    sprites: &BoardSprites,
    tr: &crate::app::i18n::Tr,
) {
    end_playtest(sim, state, commands, sprites);
    // Whatever was mid-way (an offer, a check on another thread) was asked
    // of the moment that failed, and is let go with it.
    state.offer = None;
    state.edited();
    state.feedback = tr.ed_failed.into();
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
    use super::shelf::{ascii_era_path, save_path};
    use super::*;
    use crate::sim::{CrabKind, Direction, Handedness, TileKind};

    fn sand() -> Board {
        Board::new(6, 5, 3)
    }

    /// An app with what the editor's systems read, on a blank beach and
    /// the default settings; a test inserts its own over these and adds
    /// the systems it drives.
    fn bench_app() -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.add_message::<crate::app::LevelSaved>();
        app.add_message::<crate::app::CodeShared>();
        app.add_message::<crate::app::CodeTaken>();
        app.add_message::<bevy::input::keyboard::KeyboardInput>();
        app.insert_resource(Sim(sand()));
        app.init_resource::<EditorState>();
        app.init_resource::<GameSettings>();
        app.init_resource::<Clipboard>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<crate::app::cursor::LiveSeatKeys>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app
    }

    /// `key` alone pressed for one frame.
    fn tap(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(key);
        app.update();
    }

    /// [`tap`], then a frame with nothing pressed, on which a screen
    /// change it asked for lands.
    fn tap_and_settle(app: &mut App, key: KeyCode) {
        tap(app, key);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
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
        assert!(
            !level.board().rules().castle_raids,
            "a stage plays without raids"
        );
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
        assert!(beach.board().rules().castle_raids);
        assert!(!beach.to_text().contains("raids:"));
    }

    /// A stage turned into a beach with F6 leaves the stage's rule behind:
    /// a puzzle code pasted in carries `raids: off` and its granted
    /// inventory on the board itself, and the beach saved from it never
    /// raided, since versus does not turn raids back on.
    #[test]
    fn a_stage_turned_beach_plays_with_raids() {
        let mut board = sand();
        board.set_tile(0, 0, TileKind::Castle(0));
        board.set_tile(5, 4, TileKind::Castle(1));
        let puzzle = EditorState {
            posts: 3,
            kind: LevelKind::Puzzle,
            ..EditorState::default()
        };
        // The stage as its code carries it, read back onto the bench.
        let pasted = Level::parse(&level_here(&puzzle, &board, "Stage").to_text())
            .expect("reads back")
            .board();
        assert!(!pasted.rules().castle_raids, "the stage's own rule");
        let arena = EditorState {
            kind: LevelKind::Arena,
            ..puzzle
        };
        let beach = level_here(&arena, &pasted, "Beach");
        let rules = *beach.board().rules();
        let stock = crate::sim::Rules::default();
        assert!(rules.castle_raids, "a beach raids");
        assert_eq!(
            (rules.signpost_cap, rules.cap_policy),
            (stock.signpost_cap, stock.cap_policy),
            "and plays the stock rule, not the stage's grant"
        );
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

    /// F5 throws the beach away, so once there is one it asks twice. One
    /// press of a key the prompt calls "size" used to take an unsaved
    /// level with it, and the editor has no undo and no load.
    #[test]
    fn resizing_a_beach_with_a_level_on_it_asks_twice() {
        let mut app = bench_app();
        // F5 is read by the painting system, beside the cursor it moves,
        // not by the command keys.
        app.add_systems(Update, editor_input);

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

    /// Esc resets the bench on the way out, so over unsaved work it asks
    /// first, as F5 and F4 do; over a blank beach or a level just saved
    /// it leaves at once.
    #[test]
    fn leaving_with_an_unsaved_level_asks_twice() {
        let mut app = bench_app();
        app.insert_state(Screen::Editor);
        app.add_systems(Update, (editor_input, editor_commands).chain());
        app.add_systems(OnExit(Screen::Editor), exit_editor);
        let name = "Leave Probe";
        app.world_mut().resource_mut::<EditorState>().name = name.to_string();

        let screen = |app: &App| *app.world().resource::<State<Screen>>().get();

        app.world_mut().resource_mut::<Sim>().0.set_wrap(true);
        tap_and_settle(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Screen::Editor, "the first press only asks");
        assert_eq!(
            app.world().resource::<EditorState>().feedback,
            crate::app::i18n::EN.ed_leave_confirm
        );
        assert!(app.world().resource::<Sim>().0.wrap(), "the level stays");
        // Another key takes the offer back, so Esc asks again.
        tap_and_settle(&mut app, KeyCode::F6);
        tap_and_settle(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Screen::Editor, "asked again");
        tap_and_settle(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Screen::Menu, "the second press leaves");

        // Saved as it stands, the level is not lost: one press leaves.
        app.insert_state(Screen::Editor);
        app.update();
        let mut state = app.world_mut().resource_mut::<EditorState>();
        state.name = name.to_string();
        app.world_mut().resource_mut::<Sim>().0.set_wrap(true);
        let path = save_path(name);
        let _ = std::fs::remove_file(&path);
        tap_and_settle(&mut app, KeyCode::F2);
        assert!(std::fs::read_to_string(&path).is_ok_and(|text| text.contains(name)));
        tap_and_settle(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Screen::Menu, "nothing unsaved to ask about");
        let _ = std::fs::remove_file(&path);
    }

    /// F2 onto a file this session did not write asks first: the first
    /// press leaves the other level's file as it was and says so, and only
    /// a second press replaces it. Saved at once, the note that a level had
    /// been replaced arrived with that level already gone.
    #[test]
    fn saving_over_another_levels_file_asks_twice() {
        let mut app = bench_app();
        app.add_systems(Update, (editor_input, editor_commands));
        // A name of this test's own: the shelf is shared by every test in
        // the process.
        let name = "Пляж Overwrite Probe";
        app.world_mut().resource_mut::<EditorState>().name = name.to_string();
        let path = save_path(name);
        crate::app::paths::write_atomic(&path, "somebody else's level").expect("wrote");

        let on_disk = || std::fs::read_to_string(&path).expect("reads");
        tap(&mut app, KeyCode::F2);
        assert_eq!(on_disk(), "somebody else's level", "the first press asks");
        assert_eq!(
            app.world().resource::<EditorState>().feedback,
            crate::app::i18n::EN.ed_save_taken
        );
        // Anything else pressed in between takes the offer back.
        tap(&mut app, KeyCode::KeyO);
        tap(&mut app, KeyCode::F2);
        assert_eq!(on_disk(), "somebody else's level", "asked again");
        tap(&mut app, KeyCode::F2);
        assert!(on_disk().contains(name), "the second press saves");
        // Now it is this session's own file, and saves at once.
        app.world_mut().resource_mut::<Sim>().0.set_wrap(false);
        tap(&mut app, KeyCode::F2);
        assert!(
            app.world().resource::<EditorState>().offer.is_none(),
            "no question about its own file"
        );
        // A file holding a level by this very name is this level, written
        // in another session or pasted back in: saved at once too.
        app.world_mut().resource_mut::<EditorState>().mine.clear();
        crate::app::paths::write_atomic(&path, format!("name: {name}\nold: true\n"))
            .expect("wrote");
        tap(&mut app, KeyCode::F2);
        assert!(
            !on_disk().contains("old: true"),
            "saved over its own earlier self on the first press"
        );
        let _ = std::fs::remove_file(&path);

        // Filed by an older build under the ASCII it kept of the name: the
        // level moves to its whole name, and is not listed twice.
        let name = "Café Legacy Probe";
        let legacy = ascii_era_path(name);
        assert!(legacy.ends_with("caf-legacy-probe.txt"), "{legacy:?}");
        crate::app::paths::write_atomic(&legacy, format!("name: {name}\n")).expect("wrote");
        app.world_mut().resource_mut::<EditorState>().name = name.to_string();
        tap(&mut app, KeyCode::F2);
        assert!(save_path(name).exists(), "saved under its whole name");
        assert!(!legacy.exists(), "and the old file tidied away");
        let _ = std::fs::remove_file(save_path(name));
    }

    /// The posts dial is a puzzle's rule, so it is inert on a beach - and
    /// the value waits there rather than being lost on the way through.
    #[test]
    fn the_posts_dial_is_inert_on_a_beach() {
        let mut app = bench_app();
        app.add_systems(Update, editor_commands);
        app.world_mut().resource_mut::<EditorState>().posts = 3;

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

        let mut app = bench_app();
        app.insert_resource(Sim(board.clone()));
        app.insert_resource(EditorState {
            posts: 3,
            name: "Stage".into(),
            ..EditorState::default()
        });
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

        tap(&mut app, KeyCode::Enter);
        let state = app.world().resource::<EditorState>();
        assert!(state.is_testing());
        let played = &app.world().resource::<Sim>().0;
        let filed = level_here(state, &board, "Stage").board();
        assert_eq!(played.state_hash(), filed.state_hash(), "the file's level");
        assert!(!played.rules().castle_raids, "a stage plays without raids");
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
        let mut app = bench_app();
        app.insert_resource(Sim(board));
        app.insert_resource(EditorState {
            posts: 3,
            name: "Stage".into(),
            ..EditorState::default()
        });
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
        let mut app = bench_app();
        app.insert_resource(EditorState {
            solver: Some(Validation {
                slot: std::sync::Arc::new(std::sync::Mutex::new(None)),
            }),
            ..EditorState::default()
        });
        app.add_systems(Update, editor_input);
        tap(&mut app, KeyCode::F5);
        let width = app.world().resource::<Sim>().0.width();
        assert_ne!(width, sand().width(), "the blank beach was resized");
        assert!(app.world().resource::<EditorState>().solver.is_none());
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

    /// The Enter that commits a name must not also start a playtest: the
    /// commands sit out the frame the name was committed on, and read the
    /// key normally from the next frame.
    #[test]
    fn committing_a_name_does_not_start_a_playtest() {
        let mut app = bench_app();
        app.add_systems(Update, editor_commands);
        app.world_mut().resource_mut::<EditorState>().mode = Mode::JustNamed;

        let press = |app: &mut App| tap(app, KeyCode::Enter);
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

    /// Escape in the name box abandons the name, back to the one F1
    /// found, where Enter commits; and that Escape is the name box's, not
    /// a press of Esc to leave the editor or to start asking about it.
    #[test]
    fn escape_in_the_name_box_puts_the_old_name_back() {
        let mut app = bench_app();
        app.insert_state(Screen::Editor);
        // As the schedule runs them.
        app.add_systems(
            Update,
            (editor_input, editor_commands.run_if(not(editor_naming))).chain(),
        );
        app.world_mut().resource_mut::<EditorState>().name = "Rock Pool".into();
        // Work on the bench, so a leaking Esc would at least raise the offer.
        app.world_mut().resource_mut::<Sim>().0.set_wrap(true);

        tap_and_settle(&mut app, KeyCode::F1);
        assert!(app.world().resource::<EditorState>().is_naming());
        app.world_mut().resource_mut::<EditorState>().name = "Rock Po".into();
        tap_and_settle(&mut app, KeyCode::Escape);
        let state = app.world().resource::<EditorState>();
        assert_eq!(state.name, "Rock Pool", "Escape abandons the edit");
        assert!(matches!(state.mode, Mode::Painting));
        assert_eq!(state.offer, None, "and is not also a press to leave");
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Editor
        );

        tap_and_settle(&mut app, KeyCode::F1);
        app.world_mut().resource_mut::<EditorState>().name = " Tide Line ".into();
        tap_and_settle(&mut app, KeyCode::Enter);
        let state = app.world().resource::<EditorState>();
        assert_eq!(state.name, "Tide Line", "Enter commits, tidied");
        assert!(!state.is_testing(), "and is not also a playtest");
    }

    /// An edit drops a check still running: it was handed the board as it
    /// stood, and its verdict would land on one that no longer exists,
    /// with V answering "already checking" until it ran out.
    #[test]
    fn an_edit_drops_a_check_of_the_board_before_it() {
        let mut app = bench_app();
        app.insert_state(Screen::Editor);
        app.add_systems(Update, editor_commands);
        // O wraps the beach, K steps the gulls, + and - turn the signpost
        // grant, F6 makes it the other kind: each one a different level
        // from the one being checked.
        let keys = [
            KeyCode::KeyO,
            KeyCode::KeyK,
            KeyCode::Equal,
            KeyCode::Minus,
            KeyCode::F6,
        ];
        for key in keys {
            {
                let mut state = app.world_mut().resource_mut::<EditorState>();
                // A search that never answers, so only the edit can end it.
                state.solver = Some(validate::Validation {
                    slot: std::sync::Arc::new(std::sync::Mutex::new(None)),
                });
            }
            tap(&mut app, key);
            let state = app.world().resource::<EditorState>();
            assert!(state.solver.is_none(), "{key:?} kept the stale check");
        }
    }

    /// The prompt says "+/-" in every language, so the dial answers the
    /// keys that say so: the keypad's pair, and on a QWERTZ board the
    /// caps beside P and where QWERTY has "/", besides the US pair.
    #[test]
    fn the_posts_dial_answers_the_keys_that_say_plus_and_minus() {
        let mut app = bench_app();
        app.add_systems(Update, editor_commands);
        app.world_mut().resource_mut::<EditorState>().posts = 3;
        let posts_after = |app: &mut App, key: KeyCode| {
            tap(app, key);
            app.world().resource::<EditorState>().posts
        };
        assert_eq!(posts_after(&mut app, KeyCode::NumpadAdd), 4);
        assert_eq!(posts_after(&mut app, KeyCode::NumpadSubtract), 3);
        assert_eq!(
            posts_after(&mut app, KeyCode::Equal),
            4,
            "the US pair still works"
        );
        assert_eq!(posts_after(&mut app, KeyCode::Minus), 3);
        assert_eq!(
            posts_after(&mut app, KeyCode::BracketRight),
            3,
            "QWERTY's ]"
        );
        app.world_mut()
            .resource_mut::<crate::app::keycaps::KeyCaps>()
            .force(Some(crate::app::keycaps::Layout::Qwertz));
        assert_eq!(
            posts_after(&mut app, KeyCode::BracketRight),
            4,
            "QWERTZ's +"
        );
        assert_eq!(posts_after(&mut app, KeyCode::Slash), 3, "QWERTZ's -");
        assert_eq!(posts_after(&mut app, KeyCode::Digit6), 3, "a 6 on QWERTZ");
        // AZERTY's "-" is on the 6, forced or learned from a press.
        app.world_mut()
            .resource_mut::<crate::app::keycaps::KeyCaps>()
            .force(Some(crate::app::keycaps::Layout::Azerty));
        assert_eq!(posts_after(&mut app, KeyCode::Digit6), 2, "AZERTY's -");
        let mut learned = crate::app::keycaps::KeyCaps::default();
        learned.learn(KeyCode::KeyW, 'z');
        app.insert_resource(learned);
        assert_eq!(
            posts_after(&mut app, KeyCode::Digit6),
            1,
            "AZERTY's -, learned"
        );
        assert_eq!(
            posts_after(&mut app, KeyCode::Digit5),
            1,
            "the rest of the row"
        );
        app.world_mut().resource_mut::<EditorState>().posts = 3;
        app.world_mut()
            .resource_mut::<crate::app::keycaps::KeyCaps>()
            .force(Some(crate::app::keycaps::Layout::Qwertz));
        // The cap at nine still holds on every key that says "+".
        app.world_mut().resource_mut::<EditorState>().posts = 9;
        assert_eq!(posts_after(&mut app, KeyCode::BracketRight), 9);
        assert_eq!(posts_after(&mut app, KeyCode::NumpadAdd), 9);
    }

    /// The cursor walks on P1's bindings and the brushes are keys of their
    /// own. Rebound to ESDF, E walked the cursor up and painted kelp with
    /// the same press; movement keeps the key, and the brush is still on
    /// Tab and Space.
    #[test]
    fn a_rebound_movement_key_does_not_also_paint() {
        use crate::app::binds::Action;
        let mut settings = GameSettings::default();
        for (action, key) in [
            (Action::MoveUp, KeyCode::KeyE),
            (Action::MoveLeft, KeyCode::KeyS),
            (Action::MoveDown, KeyCode::KeyD),
            (Action::MoveRight, KeyCode::KeyF),
        ] {
            settings.binds[0].set(action, key);
        }
        let mut app = bench_app();
        app.insert_resource(settings);
        app.world_mut()
            .spawn((Cursor::seated(0), Transform::default()));
        app.add_systems(Update, editor_input);
        tap(&mut app, KeyCode::KeyE);
        assert_eq!(
            app.world().resource::<Sim>().0.tile_at(0, 0),
            TileKind::Empty
        );
        assert_ne!(app.world().resource::<EditorState>().brush, Brush::Weed);
        // A brush letter the bindings left alone still paints.
        tap(&mut app, KeyCode::KeyR);
        assert_eq!(
            app.world().resource::<Sim>().0.tile_at(0, 0),
            TileKind::Rock
        );
    }

    /// P1 moving on the arrow keys left the editor no wall key and the
    /// playtest no place key, both being the arrows. The walls move onto
    /// P1's place keys and a playtest reads P1's place and remove keys, as a
    /// puzzle does; the prompts name the keys that work.
    #[test]
    fn arrows_bound_to_movement_leave_walls_and_playtest_on_the_place_keys() {
        use crate::app::binds::Action;
        let mut settings = GameSettings::default();
        let swapped = [
            (Action::MoveUp, KeyCode::ArrowUp),
            (Action::MoveDown, KeyCode::ArrowDown),
            (Action::MoveLeft, KeyCode::ArrowLeft),
            (Action::MoveRight, KeyCode::ArrowRight),
            (Action::PlaceUp, KeyCode::KeyW),
            (Action::PlaceDown, KeyCode::KeyS),
            (Action::PlaceLeft, KeyCode::KeyA),
            (Action::PlaceRight, KeyCode::KeyD),
            (Action::Remove, KeyCode::KeyX),
        ];
        for (action, key) in swapped {
            settings.binds[0].set(action, key);
        }
        let caps = crate::app::keycaps::KeyCaps::default();
        let stock = GameSettings::default();
        use crate::app::i18n::EN;
        let walls = spell_editor_keys(EN.ed_prompt, &EN, &caps, &stock);
        assert!(walls.contains("arrow keys walls"), "{walls}");
        let walls = spell_editor_keys(EN.ed_prompt, &EN, &caps, &settings);
        assert!(walls.contains("| WASD walls |"), "{walls}");
        let test = spell_editor_keys(EN.ed_playtest_prompt, &EN, &caps, &stock);
        assert_eq!(test, "arrow keys place | Space remove | Esc end test");
        let test = spell_editor_keys(EN.ed_playtest_prompt, &EN, &caps, &settings);
        assert_eq!(test, "WASD place | X remove | Esc end test");
        // The move keys too, in the order "WASD" spells a block: the stock
        // keys read as the language's own legend did, rebound keys by
        // their caps.
        let painting = EN.ed_prompt.replacen("WASD", "{moves}", 1);
        let moves = spell_editor_keys(&painting, &EN, &caps, &stock);
        assert!(
            moves.starts_with("WASD move | arrow keys walls |"),
            "{moves}"
        );
        let moves = spell_editor_keys(&painting, &EN, &caps, &settings);
        assert!(moves.starts_with("Up/Left/Down/Right move |"), "{moves}");

        let mut app = bench_app();
        let mut board = sand();
        board.set_tile(0, 0, TileKind::Castle(0));
        app.insert_resource(Sim(board));
        app.insert_resource(EditorState {
            posts: 3,
            name: "Stage".into(),
            ..EditorState::default()
        });
        app.insert_resource(settings);
        let mut cursor = Cursor::seated(0);
        (cursor.x, cursor.y) = (2, 2);
        app.world_mut().spawn((cursor, Transform::default()));
        let editing = |state: Res<EditorState>| !state.is_testing();
        app.add_systems(
            Update,
            (
                (editor_input, editor_commands).chain().run_if(editing),
                editor_test_input.run_if(editor_testing),
            )
                .chain(),
        );
        let wall = |app: &App| app.world().resource::<Sim>().0.wall_at(2, 2, Direction::Up);
        tap(&mut app, KeyCode::ArrowUp);
        assert!(!wall(&app), "the arrow is P1's move key");
        tap(&mut app, KeyCode::KeyW);
        assert!(wall(&app), "P1's place key builds the wall");
        tap(&mut app, KeyCode::KeyW);
        assert!(!wall(&app), "and takes it down");

        tap(&mut app, KeyCode::Enter);
        assert!(app.world().resource::<EditorState>().is_testing());
        let posts = |app: &App| app.world().resource::<Sim>().0.signpost_count(0);
        tap(&mut app, KeyCode::ArrowLeft);
        assert_eq!(posts(&app), 0, "the arrow is P1's move key");
        tap(&mut app, KeyCode::KeyA);
        assert_eq!(posts(&app), 1, "P1's place key");
        tap(&mut app, KeyCode::Space);
        assert_eq!(posts(&app), 1, "Space is not P1's remove key here");
        tap(&mut app, KeyCode::KeyX);
        assert_eq!(posts(&app), 0, "P1's remove key");
    }

    /// The key itself, through the system that reads it: F6 flips the kind
    /// and says where the level now lands.
    #[test]
    fn f6_flips_the_kind() {
        let mut app = bench_app();
        app.add_systems(Update, editor_commands);

        let press = |app: &mut App| tap(app, KeyCode::F6);
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
}
