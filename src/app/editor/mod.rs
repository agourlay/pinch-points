//! Driftwood, the level editor (spec §5.3): author walls, castles, spawners,
//! rocks, crabs, and gulls on a live board; playtest in place; validate
//! solvability by brute-force against the headless sim; save to disk in the
//! text level format.

mod brush;
mod card;
mod help;
mod history;
mod levels;
mod naming;
mod palette;
mod playtest;
mod resize;
mod setting;
mod share;
mod shelf;
mod symmetry;
mod templates;
mod validate;

pub use brush::Brush;
pub use card::update_settings_card;
pub use help::{editor_card_input, show_editor_card};
pub use levels::{LevelList, editor_levels_input, show_level_list};
pub use palette::{CARD_MARGIN, CARD_W, EditorUi, spawn_editor_ui, update_editor_palette};
pub use playtest::{
    Playtest, TestPhase, editor_test_input, seat_test_bots, seat_test_partner, update_test_banner,
};
pub(crate) use playtest::{prompt as test_prompt, status as test_status};
pub use setting::Setting;
pub use share::{level_from_code, taken_from_elsewhere};
pub use shelf::{Shelved, custom_dir, legacy_save_path, shelve};
pub use symmetry::Symmetry;
pub use validate::{SaveChecks, finish_orphaned_saves, poll_solver};

use brush::{paint, stroke_onto};
use naming::type_a_name;
use symmetry::{Layer, Unfair};
use validate::{Validation, arena_report, start_check};

use crate::app::cursor::Cursor;
use crate::app::gamepad::LastDevice;
use crate::app::i18n::{Tr, fill};
use crate::app::session::BoardSprites;
use crate::app::settings::GameSettings;
use crate::app::{Screen, Sim};
use crate::sim::{Board, Direction, Goal, Level, LevelKind, Placement};
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
        // The editor seats nobody else at the keyboard: P1's scheme as set.
        let map = crate::app::cursor::keymap(settings, 0, settings.commit_in_play(false));
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

    /// P1's movement keys as the prompts name them: "arrow keys" when they
    /// are the arrows (the mirrored placing scheme puts them there), else
    /// the four caps in the order "WASD" spells a block.
    pub(crate) fn spell_moves(&self, tr: &Tr, caps: &crate::app::keycaps::KeyCaps) -> String {
        let [up, down, left, right] = self.moves;
        match self.moves == ARROWS.map(|(key, _)| key) {
            true => tr.ed_arrow_keys.to_string(),
            false => spell_block(caps, [up, left, down, right]),
        }
    }

    /// Whether `key` is one of P1's movement keys or a wall key, which win
    /// it from the editor's own keys.
    ///
    /// The cursor walks on P1's bindings, and the brushes and commands are
    /// keys of their own, so a binding moved onto one did two things at once:
    /// rebound to ESDF, E walked the cursor up and painted kelp on every step.
    /// Movement wins, being the binding the player chose and the one the
    /// prompt names, and so do the walls when they have moved onto P1's place
    /// keys. A brush they shadow is still reached with Tab and Space, and a
    /// command moves to its function key ([`EditorKeys::command`]).
    fn shadows(&self, key: KeyCode) -> bool {
        self.moves.contains(&key) || self.walls.iter().any(|(wall, _)| *wall == key)
    }

    /// A press the editor's own keys may answer: just pressed, and not one
    /// P1's movement or wall keys have taken ([`EditorKeys::shadows`]).
    fn command_press<'a>(
        &'a self,
        keys: &'a ButtonInput<KeyCode>,
    ) -> impl Fn(KeyCode) -> bool + 'a {
        move |key| keys.just_pressed(key) && !self.shadows(key)
    }

    /// The key that loads `brush`, unless P1's movement or wall keys have
    /// taken it: a brush has no stand-in, and is reached with Tab and
    /// Space then. Read by the brush's press and by the palette and help
    /// card that name it, so a taken letter is never offered.
    pub(crate) fn brush(&self, brush: Brush) -> Option<KeyCode> {
        Some(brush.key()).filter(|&key| !self.shadows(key))
    }

    /// The key `command` is on: its letter, or its function key while P1's
    /// movement or wall keys have taken the letter. Read by the command
    /// itself, by the prompt that names it and by the pad's button for it,
    /// so the three cannot part.
    pub(crate) fn command(&self, command: Command) -> KeyCode {
        match self.shadows(command.letter()) {
            true => command.stand_in(),
            false => command.letter(),
        }
    }
}

/// The editor's commands on letter keys, besides the brushes: each has a
/// letter it is known by and a function key it moves to when P1's keys
/// take the letter. Bound to the IJKL scheme, the walls moved onto IJKL
/// and K, the gulls' key, toggled a wall instead while the prompt went on
/// saying "K gulls".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    /// The solver for a stage, the castle count for a beach.
    Check,
    /// The flock dial.
    Gulls,
    /// The open-ocean toggle.
    Wrap,
    /// The creature or hole under the cursor turned to face the next way.
    Turn,
    /// The last edit taken back, and with Shift made again.
    Undo,
}

impl Command {
    pub(crate) const ALL: [Command; 5] = [
        Command::Check,
        Command::Gulls,
        Command::Wrap,
        Command::Turn,
        Command::Undo,
    ];

    /// The letter the command is known by.
    fn letter(self) -> KeyCode {
        match self {
            Command::Check => KeyCode::KeyV,
            Command::Gulls => KeyCode::KeyK,
            Command::Wrap => KeyCode::KeyO,
            Command::Turn => KeyCode::KeyT,
            Command::Undo => KeyCode::KeyZ,
        }
    }

    /// Where the command goes when its letter is taken. No binding can
    /// take a function key (`binds::bindable`), so this one is always the
    /// command's own.
    fn stand_in(self) -> KeyCode {
        match self {
            Command::Check => KeyCode::F9,
            Command::Gulls => KeyCode::F7,
            Command::Wrap => KeyCode::F8,
            Command::Turn => KeyCode::F10,
            Command::Undo => KeyCode::F12,
        }
    }

    /// The prompt's name for the slot the key goes in.
    fn slot(self) -> &'static str {
        match self {
            Command::Check => "check",
            Command::Gulls => "gulls",
            Command::Wrap => "wrap",
            Command::Turn => "turn",
            Command::Undo => "undo",
        }
    }
}

/// The key that, with Shift, puts up the card of every key and button
/// ([`EditorCard::Help`]): F1 alone names the level. Function keys are
/// the editor's own, which no binding can take, and every one is spoken
/// for, so help shares F1 the way the sizes share F5.
pub(crate) const HELP_KEY: KeyCode = KeyCode::F1;

/// The key that moves the settings card's choice on to its next row, and
/// back with Shift. N is one of the game's own letters that no binding may
/// take (`binds::GLOBAL_KEYS`), so unlike the commands it never has to
/// move off its letter.
pub(crate) const NEXT_SETTING: KeyCode = KeyCode::KeyN;

/// The key `command` is on under these settings ([`EditorKeys::command`]):
/// what the pad's button for it presses.
pub(crate) fn command_key(settings: &GameSettings, command: Command) -> KeyCode {
    EditorKeys::of(settings).command(command)
}

/// The key that toggles the wall on `dir`'s side of the cursor's tile,
/// as [`EditorKeys`] reads it: what the pad's right stick presses.
pub(crate) fn wall_key(settings: &GameSettings, dir: Direction) -> Option<KeyCode> {
    EditorKeys::of(settings)
        .walls
        .iter()
        .find(|(_, side)| *side == dir)
        .map(|(key, _)| *key)
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

/// `text` with the editor's `{moves}`, `{walls}`, `{place}`, `{remove}`,
/// `{help}` and command keys (`{check}`, `{gulls}`, `{wrap}`) filled in: the caps
/// of the keys [`EditorKeys`] reads,
/// or the language's own words while the walls and the playtest's keys
/// are the stock arrow keys and Space.
///
/// Run after [`crate::app::keycaps::KeyCaps::legend`]: the keys are
/// already in this keyboard's caps, and caps that happen to read "WASD"
/// would be respelled again as the QWERTY keys the legend means by it.
pub(crate) fn spell_editor_keys(
    text: &str,
    tr: &Tr,
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
    let moves = keys.spell_moves(tr, caps);
    let remove = if keys.remove == KeyCode::Space {
        tr.ed_space_key.to_string()
    } else {
        caps.label(keys.remove)
    };
    let commands = Command::ALL.map(|command| (command.slot(), caps.label(keys.command(command))));
    let mut slots = vec![
        ("moves", moves),
        ("walls", block(keys.walls)),
        ("place", block(keys.place)),
        ("remove", remove),
        ("help", shifted(caps, HELP_KEY)),
    ];
    slots.extend(commands);
    let slots: Vec<(&str, &str)> = slots.iter().map(|(k, v)| (*k, v.as_str())).collect();
    fill(text, &slots)
}

/// A key held with Shift, as a prompt names it: "Shift+F1".
pub(crate) fn shifted(caps: &crate::app::keycaps::KeyCaps, key: KeyCode) -> String {
    format!("Shift+{}", caps.label(key))
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

/// A key that throws the level on the board away, pressed once and waiting
/// to be pressed again. One at a time: pressing anything else, the other
/// one included, takes the offer back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Offer {
    /// Enter on "My levels": the level chosen there in place of this one.
    Open,
    /// F2: this level saved over a file this bench did not write.
    Overwrite,
    /// Esc: back to the menu, with what has not been saved left behind.
    Leave,
}

impl Offer {
    /// What the first press says is about to go, naming the key that takes
    /// the offer up: for a pad's player, the pad's button for it, from the
    /// editor's table the bridge presses that key by.
    fn warning(self, tr: &Tr, device: LastDevice) -> String {
        use crate::app::gamepad::scheme::{Does, button_name};
        let (keys, pad, layer, does) = match self {
            Offer::Open => (tr.ed_open_confirm, tr.pad.open_confirm, None, Does::Open),
            Offer::Overwrite => (tr.ed_save_taken, tr.pad.save_taken, Some(true), Does::Save),
            Offer::Leave => (
                tr.ed_leave_confirm,
                tr.pad.leave_confirm,
                Some(false),
                Does::Menu,
            ),
        };
        let face = match layer {
            Some(layer) => crate::app::gamepad::Face::Editor { layer },
            None => crate::app::gamepad::Face::LevelList,
        };
        match (device, button_name(&face, does, tr, device.family())) {
            (LastDevice::Pad(_), Some(button)) => fill(pad, &[("b", &button)]),
            _ => keys.to_string(),
        }
    }

    fn key(self) -> KeyCode {
        match self {
            Offer::Open => KeyCode::Enter,
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
/// castle, a gull, the flock, the wrap. A blank beach is worth nothing.
fn has_work(state: &EditorState, board: &Board) -> bool {
    // The same seed, so the comparison is about what has been built and
    // not about where the board came from: a level's text carries its
    // seed, and a blank board with a different one reads as a difference.
    let blank = Board::new(board.width(), board.height(), board.seed());
    level_here(state, board, "x").to_text() != level_here(state, &blank, "x").to_text()
}

/// Whether throwing the bench away now would lose work: something is
/// built here, and it is not the level F2 last wrote. Esc and F4 each
/// ask first on it: a blank beach is worth nothing, and a level just saved
/// is on the shelf, so over either they go at once, where the old
/// question said "this one goes" of a level that was safe.
fn unsaved_work(state: &EditorState, board: &Board) -> bool {
    has_work(state, board) && state.filed.as_deref() != Some(filed_text(state, board).as_str())
}

/// The level on the bench as [`EditorState::filed`] remembers it: under
/// its name and without the way through it carries. A way through found
/// after a save is the check's work, not the author's, and with it the
/// level just saved read as unsaved.
pub(super) fn filed_text(state: &EditorState, board: &Board) -> String {
    let mut level = level_here(state, board, &state.name);
    level.solution.clear();
    level.to_text()
}

#[derive(Resource, Default)]
pub struct EditorState {
    /// The signposts a stage grants: on a co-op stage, P1's hand.
    pub posts: u8,
    /// A co-op stage's second hand, P2's arrows (`hands:`): `None` for a
    /// stage played alone. Like the grant, the author's setting, which a
    /// fresh map keeps.
    pub second_hand: Option<u8>,
    /// What is being built: a stage for the Tide Pool list, or a beach for
    /// the versus map dial. The author says which rather than the game
    /// counting castles and deciding for them.
    pub kind: LevelKind,
    /// What the stage asks of the player. The editor has no key to change
    /// it, but a pasted code can bring a goal of its own, and the bench
    /// keeps it so that saving, sharing, playtesting and the check all hand
    /// on the level that was pasted rather than an every-crab stage. The
    /// status line shows it, and a fresh map or a turn of F6 puts the
    /// every-crab goal back.
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
    /// Saves whose check the bench let go of while they waited on it,
    /// handed on to [`validate::SaveChecks`] to be finished off the bench.
    orphans: Vec<validate::OrphanedSave>,
    /// The level, as [`checked_level`] reads it, that the last check came
    /// back about: the bench is checked while it still reads the same.
    checked: Option<String>,
    /// The tables a pasted beach names (`seats:`) and the strip it rolls by
    /// (`roll:`), handed on to its file. A board does not carry them, and
    /// rebuilt from the board a beach for four and six lost the line that
    /// keeps a table of five off it. Dropped where the board no longer
    /// fits them ([`level_here`]).
    tables: Vec<u8>,
    roll: u8,
    /// The winning placements a pasted stage carried or the last check
    /// found, handed on to its file (`solution:`) and drawn on the sand
    /// until an edit changes the board they win.
    solution: Vec<Placement>,
    /// The files this bench has written this session.
    ///
    /// Saving onto any other file that is already there asks first: the
    /// save path is the name, and a level saved in another session, a
    /// friend's code under the same name or a fresh map left on the default
    /// name all land on a file that holds somebody's level. A file written
    /// from here holds this level's earlier self, and saving over it is
    /// the common case, so it goes ahead.
    mine: std::collections::HashSet<std::path::PathBuf>,
    /// The file the level on the bench was opened from, and the name it
    /// went by there: F2 saves it back into that file while it keeps that
    /// name ([`shelf::save_target`]). Its file is not always the one its
    /// name would make (an older build's `custom.txt`, a file named by
    /// hand, the second of two namesakes), and saving under the name wrote
    /// a second copy of the level beside the first.
    opened: Option<(std::path::PathBuf, String)>,
    /// A board-replacing key pressed once and waiting to be taken up.
    offer: Option<Offer>,
    /// The level's text as F2 last wrote it, or as a starter opened it, so
    /// Esc can tell work already on the shelf, or no work of the author's
    /// at all, from work that would be lost (see [`unsaved_work`]).
    filed: Option<String>,
    /// The settings card's row "+" and "-" turn ([`Setting::chosen`]).
    pub setting: Setting,
    /// The copies every edit is made on as well ([`Symmetry`]): how the
    /// level on the bench is being drawn. Off for every level brought onto
    /// the bench (a new one, a saved one, a pasted code), on as drawn for
    /// a starter; not saved with the level, and not carried to the next.
    pub symmetry: Symmetry,
    /// The rate new holes are dug at, which the card's holes row turns
    /// for every hole on the bench at once; `None` for the stock rate
    /// ([`setting::hole_period`]).
    hole_period: Option<u32>,
    /// A brush being dragged across the sand: see [`Stroke`].
    stroke: Option<Stroke>,
    /// The edits Z takes back and Shift+Z makes again.
    history: history::History,
    /// Statics need a rebuild after a tile/wall edit.
    dirty: bool,
}

/// A brush held down and dragged: the tile the stroke began on, which
/// every tile the cursor then crosses is made like
/// ([`stroke_onto`]), and the last tile it touched.
#[derive(Clone, Debug)]
struct Stroke {
    from: (u8, u8),
    last: (u8, u8),
    /// Every tile the stroke has made, its own and its mirror's copies.
    done: Vec<(u8, u8)>,
    /// Whether the stroke has an edit in the history yet, which its later
    /// steps carry on. A press that changed nothing has none, and the
    /// first tile its drag changes is its own step back: merged into the
    /// edit before it, one Z took back the drag and an unrelated edit.
    recorded: bool,
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
    JustClosed,
    /// "My levels" is up over the bench ([`LevelList`]), and has the keys.
    Levels(Box<LevelList>),
    /// A card of words over the bench, which any key puts away
    /// ([`EditorCard`]).
    Card(EditorCard),
    /// A playtest is under way ([`Playtest`]), with the board to restore
    /// when it ends. Boxed: a board is most of a kilobyte and the other
    /// modes are nothing.
    Testing(Box<Playtest>),
}

/// The cards the editor puts over the bench to be read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditorCard {
    /// Every key and pad button the editor answers to, Shift+F1 or
    /// Select+Start (`help::sections`).
    Help,
    /// A few words for a first visit, shown once ever (`help::TIPS`).
    Tips,
}

impl Mode {
    /// A stage's playtest of `bench`, running: for a test elsewhere that
    /// needs the editor in one.
    #[cfg(test)]
    pub fn testing(bench: Board) -> Mode {
        let level = Level::from_board("Test", 3, bench.clone());
        let mut test = Playtest::of(bench, level);
        test.phase = TestPhase::Running;
        Mode::Testing(Box::new(test))
    }
}

impl EditorState {
    pub fn is_naming(&self) -> bool {
        matches!(self.mode, Mode::Naming { .. })
    }

    /// Whether a box over the bench has the keyboard, the name being
    /// typed, "My levels" or a card: the board's keys and the cursor wait.
    pub fn has_keys(&self) -> bool {
        matches!(
            self.mode,
            Mode::Naming { .. } | Mode::Levels(_) | Mode::Card(_)
        )
    }

    /// The card up over the bench, if one is.
    pub fn card(&self) -> Option<EditorCard> {
        match self.mode {
            Mode::Card(card) => Some(card),
            Mode::Painting
            | Mode::Naming { .. }
            | Mode::JustClosed
            | Mode::Levels(_)
            | Mode::Testing(_) => None,
        }
    }

    pub fn is_testing(&self) -> bool {
        matches!(self.mode, Mode::Testing(_))
    }

    /// The playtest under way, if one is.
    pub fn test(&self) -> Option<&Playtest> {
        match &self.mode {
            Mode::Testing(test) => Some(test),
            Mode::Painting
            | Mode::Naming { .. }
            | Mode::JustClosed
            | Mode::Levels(_)
            | Mode::Card(_) => None,
        }
    }

    /// Whether a playtest's board is ticking ([`TestPhase::runs`]).
    pub fn test_running(&self) -> bool {
        self.test().is_some_and(|test| test.phase.runs())
    }

    /// The level changed under the author's hand: the statics are redrawn,
    /// and a check still running is dropped, because it was handed the
    /// board as it stood and would answer "solvable" or not over this one
    /// (and V would say it is still checking until it ran out). Its
    /// "checking..." goes with it, and so does a carried solution, which
    /// won the board before the edit. The creatures are turned the way
    /// the file will read them ([`settle_facings`]), and the level as it
    /// stood goes into the history Z walks back through. Every author edit
    /// goes through here, so none can forget any of it.
    ///
    /// An edit starts a held stroke ([`Stroke`]) over from the tile it is
    /// on: a wall, a turn or a dial while a brush is held is an edit of its
    /// own, and the drag after it is a step back of its own too, not that
    /// edit carried on; and it paints on, where ended it painted nothing
    /// more for the rest of the hold with no word why. From the tile the
    /// stroke is on, which a turn may have just changed, when the stroke
    /// made that tile; from the tile it began on when the mirror would not
    /// let it. Says whether the level changed.
    fn edited(&mut self, board: &mut Board) -> bool {
        if let Some(stroke) = &mut self.stroke {
            stroke.restart();
        }
        self.edit(board, false)
    }

    /// [`EditorState::edited`] for a step of a stroke, `merge` when it
    /// carries on a step already in the history. A press that changed
    /// nothing (a brush on a tile it already made, a dial with nothing to
    /// turn) is no edit: the level, its check and its way through stand,
    /// and the history is not written.
    ///
    /// An edit that lets a way through go lets the check that found it go
    /// too: the bench reads as checked by its level alone, and an edit
    /// taken back by hand (a rock painted and erased) brought the checked
    /// level back without its way through, which F2 then saved with no
    /// `solution:` and no check.
    fn edit(&mut self, board: &mut Board, merge: bool) -> bool {
        self.dirty = true;
        settle_facings(self, board);
        if self.history.is_now(&history::snapshot(self, board)) {
            return false;
        }
        if !self.solution.is_empty() {
            self.solution.clear();
            self.checked = None;
        }
        self.drop_check();
        let after = history::snapshot(self, board);
        self.history.record(after, merge);
        true
    }

    /// Begin the history afresh at the level on `board`: a level brought
    /// onto the bench is a level of its own, and Z does not take it back
    /// to the one it replaced.
    fn begin_history(&mut self, board: &Board) {
        self.history = history::History::starting_at(history::snapshot(self, board));
    }

    /// Make the copies of the tile at `at` what it now is, as the bench's
    /// mirror paints them ([`symmetry::mirror_tile`]): a beach's castles
    /// go to their copies' seats, a stage's all bank alike. Called by each
    /// key that changes a tile before the one [`EditorState::edited`] of
    /// its press, so a mirrored press is one edit and one undo. Says the
    /// tiles it wrote, or, on a beach whose copies of the tile could not
    /// be fair ([`symmetry::unfair`], weighing `layer` of it), why, having
    /// written nothing.
    fn mirror_tile(
        &self,
        board: &mut Board,
        at: (u8, u8),
        layer: Layer,
    ) -> Result<Vec<(u8, u8)>, Unfair> {
        let seats = self.kind == LevelKind::Arena;
        if seats && let Some(why) = symmetry::unfair(board, self.symmetry, at, layer) {
            return Err(why);
        }
        Ok(symmetry::mirror_tile(board, self.symmetry, seats, at))
    }

    /// Let a running check go, with its "checking..." line: it was asked
    /// about a board that is going.
    fn drop_check(&mut self) {
        if let Some(check) = self.solver.take() {
            self.feedback.clear();
            self.orphans.extend(check.orphan());
        }
    }

    /// Clear the bench for a new level: a name no saved level has, and
    /// nothing of the level before (its goal, tables, roll and solution,
    /// its holes' rate, its check, the files it was saved to, what F2 last
    /// wrote and the mirror it was drawn with). The kind, the signpost
    /// grant, the brush and the card's chosen row stay, being the author's
    /// own settings rather than the level's.
    ///
    /// What entering the editor and a new level both start from, `board`
    /// being the fresh sand. A fresh map that kept the name and the files
    /// had the next F2 write it over the level just saved without a word.
    fn start_afresh(&mut self, tr: &Tr, board: &Board) {
        let Self {
            posts: _,
            second_hand: _,
            kind: _,
            goal,
            name,
            mode: _,
            brush: _,
            feedback: _,
            solver: _,
            orphans: _,
            checked,
            tables,
            roll,
            solution,
            mine,
            opened,
            offer: _,
            filed,
            setting: _,
            symmetry,
            hole_period,
            stroke,
            history: _,
            dirty: _,
        } = self;
        *name = shelf::fresh_name(tr);
        *goal = Goal::default();
        *hole_period = None;
        *stroke = None;
        *checked = None;
        tables.clear();
        *roll = 0;
        solution.clear();
        mine.clear();
        *opened = None;
        *filed = None;
        *symmetry = Symmetry::Off;
        self.begin_history(board);
    }
}

/// Turn the bench's crabs and gulls the way its file will read them back
/// ([`Board::face_as`]): a wall built in front of a crab turns it, by the
/// rule the level reader turns it by, the gulls by the claw their file
/// rolls them. Left facing the wall, the editor drew one facing, the file
/// played another, and the bench read as a different level from its own
/// code.
fn settle_facings(state: &EditorState, board: &mut Board) {
    let settled = level_here(state, board, &state.name).board();
    board.face_as(&settled);
}

/// A level for the editor to open on, left by the screen that sends the
/// player there: a custom stage's Edit on the stage list, or a level code
/// pasted on the menu, to edit or to play.
#[derive(Resource, Default)]
pub struct EditorOpening(pub Option<Opening>);

/// See [`EditorOpening`].
#[derive(Clone, Debug)]
pub struct Opening {
    pub level: Level,
    /// The file it was read from, which becomes the bench's own (see
    /// `levels::open_saved`); `None` for a pasted code.
    pub path: Option<std::path::PathBuf>,
    /// Straight into a playtest of it, as the menu's Play does for a beach.
    pub test: bool,
}

/// The editor opening on the level filed at `path`, read fresh from the
/// file rather than taken from a list that may have renamed it to tell it
/// from a namesake (`campaign::disambiguate`). `None` when the file is
/// gone or no longer reads.
pub fn opening_from_file(path: &std::path::Path) -> Option<Opening> {
    let text = std::fs::read_to_string(path).ok()?;
    let level = Level::parse(&text).ok()?;
    Some(Opening {
        level,
        path: Some(path.to_path_buf()),
        test: false,
    })
}

pub fn enter_editor(
    mut sim: ResMut<Sim>,
    settings: Res<GameSettings>,
    mut state: ResMut<EditorState>,
    mut opening: ResMut<EditorOpening>,
) {
    let tr = settings.tr();
    *state = EditorState {
        posts: 3,
        feedback: tr.ed_fresh_sand.into(),
        dirty: true, // draws the initial statics
        ..default()
    };
    sim.0 = Board::new(EDITOR_BOARD.0, EDITOR_BOARD.1, 0xED17);
    state.start_afresh(tr, &sim.0);
    // A first visit is met with the tips, until they are put away once
    // (`help::editor_card_input`); a playtest opened from the menu goes
    // straight in, and the tips wait for the next visit.
    if settings.editor_tips {
        state.mode = Mode::Card(EditorCard::Tips);
    }
    // `PINCH_EDITOR_LEVEL=<path>`: that level file on the bench.
    let dev = crate::app::dev::editor_level().and_then(|path| {
        let opened = opening_from_file(&path);
        if opened.is_none() {
            warn!("PINCH_EDITOR_LEVEL: {} does not read", path.display());
        }
        opened
    });
    let Some(Opening { level, path, test }) = opening.0.take().or(dev) else {
        return;
    };
    sim.0 = level.board();
    match path {
        Some(path) => levels::open_saved(&mut state, &level, path, &sim.0, tr),
        None => {
            share::take_level(&mut state, &level, &sim.0, tr);
            state.feedback = fill(tr.ed_opened, &[("name", &state.name)]);
        }
    }
    if test {
        playtest::begin(&mut sim, &mut state);
    }
}

/// The bench is cleared on the way out, and so is the AI a beach's
/// playtest sat at its table.
pub fn exit_editor(
    mut state: ResMut<EditorState>,
    mut controllers: ResMut<crate::app::Controllers>,
    mut checks: ResMut<validate::SaveChecks>,
) {
    // A save still waiting on its check gets its answer all the same.
    let waiting = state.solver.take().and_then(validate::Validation::orphan);
    checks.0.extend(waiting);
    checks.0.append(&mut state.orphans);
    *state = EditorState::default();
    *controllers = crate::app::Controllers::default();
}

/// Whether a box over the bench has the keyboard
/// ([`EditorState::has_keys`]). A run condition rather than an early
/// return, because the cursor is walked by a system of its own: typing
/// "Wade" otherwise walks the cursor up and then left across the beach.
pub fn editor_has_keys(state: Res<EditorState>) -> bool {
    state.has_keys()
}

/// Whether a card is up over the bench. (Run-condition helper.)
pub fn editor_on_card(state: Res<EditorState>) -> bool {
    state.card().is_some()
}

/// Whether "My levels" is up. (Run-condition helper.)
pub fn editor_on_levels(state: Res<EditorState>) -> bool {
    matches!(state.mode, Mode::Levels(_))
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
    // Asked about the board that went, even when the one that came reads
    // the same and so is no edit.
    state.drop_check();
    state.edited(&mut sim.0);
    crate::app::cursor::center_on(&sim.0, &mut swap.cursors);
}

/// Put the level an undo or redo went to (`text`, a [`history::snapshot`])
/// back on the bench: its board, sprites and all, since its creatures are
/// read back from text and numbered afresh ([`swap_board`]), and the lines
/// of its own an edit can change (signposts, kind, goal, solution and the
/// holes' rate). Not the tables and the roll: those the bench carries
/// whole from the level it took on, and a snapshot holds them only as far
/// as its board fits them ([`level_here`]), so put back from one, a table
/// of four left out while the fourth castle was gone was gone for good.
/// Not an edit itself, or it would go into the history it came out of; a
/// check of the level it replaced is let go all the same.
fn put_back(text: &str, sim: &mut Sim, state: &mut EditorState, swap: &mut BoardSwap) {
    let Ok(level) = Level::parse(text) else {
        return;
    };
    swap_board(level.board(), sim, state, &mut swap.commands, &swap.sprites);
    state.posts = level.hands().map_or(level.posts, |[p1, _]| p1);
    state.second_hand = level.hands().map(|[_, p2]| p2);
    state.kind = level.kind;
    state.goal = level.goal;
    state.solution.clone_from(&level.solution);
    if let Some(period) = setting::first_hole_period(&sim.0) {
        state.hole_period = Some(period);
    }
    state.drop_check();
    move_cursors(&sim.0, &mut swap.cursors, (0, 0));
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
    // A stroke's first tile was on the board that is going.
    state.stroke = None;
    sprites.despawn_all(commands);
    std::mem::replace(&mut sim.0, board)
}

/// Whether either Shift key is held: the key that walks a dial, the
/// brushes, the settings card's rows, the sizes and the history the other
/// way.
fn shift_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)
}

/// Move the cursors by `shift` with what they stood on, kept on `board`:
/// after a resize they stay over the same tile of the level, which moved.
fn move_cursors(
    board: &Board,
    cursors: &mut Query<(&mut Cursor, &mut Transform)>,
    (dx, dy): (i16, i16),
) {
    let clamp = |at: u8, by: i16, size: u8| -> u8 {
        u8::try_from((i16::from(at) + by).clamp(0, i16::from(size) - 1)).unwrap_or(0)
    };
    for (mut cursor, mut transform) in cursors {
        cursor.x = clamp(cursor.x, dx, board.width());
        cursor.y = clamp(cursor.y, dy, board.height());
        transform.translation = crate::app::layout::tile_center(board, cursor.x, cursor.y)
            .extend(crate::app::layout::z::CURSOR);
    }
}

/// Tile and creature painting under the cursor: walls, terrain, crabs,
/// and gulls. The editor's command keys live in [`editor_commands`].
pub fn editor_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    settings: Res<GameSettings>,
    device: Res<LastDevice>,
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
    // "My levels" and a card have the keys while they are up
    // (`levels::editor_levels_input`, `help::editor_card_input`). The
    // card is read before this system, so the frame its key put it away
    // is sat out too: that key was the card's, not a brush's.
    if matches!(
        state.mode,
        Mode::Levels(_) | Mode::Card(_) | Mode::JustClosed
    ) {
        return;
    }
    let editor_keys = EditorKeys::of(&settings);
    let pressed = editor_keys.command_press(&keys);
    // Anything else said this frame takes an offer back with it, F4's or
    // F2's, the way stepping off the settings row takes back the reset.
    //
    // Esc is the exception, being answered by the commands: over F4's or
    // F2's question it takes the question back and nothing else (see
    // [`editor_commands`]), where taken back here it went on to leave.
    if let Some(offer) = state.offer
        && keys
            .get_just_pressed()
            .any(|&k| k != offer.key() && k != KeyCode::Escape)
    {
        state.offer = None;
    }
    // Beach size, on a function key: the next size up, or down with
    // Shift. On `[` and `]`, which are *physical* keys, the prompt named
    // keys a non-US keyboard does not have; F5 is F5 everywhere.
    //
    // What is built stays, centred on the new beach ([`resize::resized`]).
    // A resize used to start a fresh beach and so asked twice over a level;
    // now it is an edit like any other, and Z brings back whatever a
    // smaller beach cut off.
    if pressed(KeyCode::F5) {
        let board = &sim.0;
        let was = (board.width(), board.height());
        let (w, h) = resize::next_size(was, !shift_held(&keys));
        let resize::Resized { board, shift, cut } = resize::resized(board, (w, h));
        // A side grown or shrunk by an odd count moves the beach's middle
        // half a tile against what is built, and a mirror across it would
        // paint each copy half a tile off the level's own: it goes off,
        // and the author is told why.
        let moved = state
            .symmetry
            .moves_off_centre((w ^ was.0) & 1 == 1, (h ^ was.1) & 1 == 1);
        if moved {
            state.symmetry = Symmetry::Off;
        }
        swap_board(
            board,
            &mut sim,
            &mut state,
            &mut swap.commands,
            &swap.sprites,
        );
        state.edited(&mut sim.0);
        move_cursors(&sim.0, &mut swap.cursors, shift);
        let tr = settings.tr();
        let said = match cut {
            true => tr.ed_resized_cut,
            false => tr.ed_resized,
        };
        state.feedback = fill(said, &[("w", &w.to_string()), ("h", &h.to_string())]);
        if moved {
            state.feedback = format!("{} - {}", tr.ed_mirror_moved, state.feedback);
        }
        return;
    }
    if pressed(HELP_KEY) && shift_held(&keys) {
        state.mode = Mode::Card(EditorCard::Help);
        state.offer = None;
        return;
    }
    if pressed(KeyCode::F1) {
        state.mode = Mode::Naming {
            was: state.name.clone(),
        };
        // A pad's player has no keys to name it with until Steam's own
        // keyboard is up, so that is what they are told.
        state.feedback = match *device {
            LastDevice::Pad(_) => settings.tr().pad.steam_keyboard.to_string(),
            LastDevice::Keyboard => settings.tr().ed_naming.to_string(),
        };
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
            symmetry::mirror_wall(board, state.symmetry, (x, y), dir);
            state.edited(board);
        }
    }

    // The creature or hole under the cursor, turned to face the next way
    // it can.
    // On the mirror's line it goes on round past the ways it cannot face
    // there, judged by what turns: a crab is not held back by the hole
    // under it.
    let tr = settings.tr();
    if pressed(editor_keys.command(Command::Turn)) && turn_here(&state, &mut board.clone(), x, y) {
        let layer = match brush::creature_at(board, x, y) {
            true => Layer::Creatures,
            false => Layer::Ground,
        };
        let mut tries = 0;
        let turned = paint_fairly(&mut state, board, (x, y), layer, tr, |state, trial| {
            tries += 1;
            turn_here(state, trial, x, y) && tries < Direction::ALL.len()
        });
        if turned.is_some() {
            state.edited(board);
        }
    }

    // Pick a brush and paint with it. The letters do both, so a hand that
    // knows the old editor keeps working and the palette simply shows it
    // what it just chose; Tab walks the palette for a hand that does not.
    // Either press begins a stroke, which goes on while the key is held.
    let period = setting::hole_period(&state);
    for brush in Brush::ALL {
        if editor_keys.brush(brush).is_some_and(&pressed) {
            state.brush = brush;
            press(&mut state, board, (x, y), period, tr);
        }
    }
    if pressed(KeyCode::Tab) {
        // A stroke goes on with the brush it began with or not at all.
        state.stroke = None;
        let at = Brush::ALL
            .iter()
            .position(|b| *b == state.brush)
            .unwrap_or(0);
        let step = if shift_held(&keys) {
            Brush::ALL.len() - 1
        } else {
            1
        };
        state.brush = Brush::ALL[(at + step) % Brush::ALL.len()];
    }
    if pressed(KeyCode::Space) {
        press(&mut state, board, (x, y), period, tr);
    }
    drag(&mut state, board, &editor_keys, &keys, (x, y), tr);
}

/// A press of the loaded brush at `at`, mirrored, which begins a stroke.
/// The two brushes that cycle step on past what the mirror cannot copy
/// fairly there, a castle to the next seat that it can and a hole to the
/// next way, and so to nothing when none can; a press that had to skip
/// begins no stroke, since what it left is not what its brush paints.
///
/// A press ends the stroke before it, whatever comes of it: a letter the
/// mirror refused left the old stroke going with the new brush, which
/// made each tile crossed like a first tile the new brush never painted.
fn press(state: &mut EditorState, board: &mut Board, at: (u8, u8), period: u32, tr: &Tr) {
    state.stroke = None;
    let (fair, refused) = press_fairly(state, board, at, period);
    if let Some(why) = refused {
        state.feedback = unfair_line(why, tr);
    }
    if let Some(Fair { written, skipped }) = fair {
        let recorded = state.edited(board);
        if !skipped {
            state.stroke = Some(Stroke::at(at, written, recorded));
        }
    }
}

/// What a press of the loaded brush at `at` makes of `board` ([`press`]),
/// and the last way it was refused: the one answer the press and the
/// readout's "next:" both read (`palette::under_and_next`), so the line
/// cannot promise a castle the mirror will not let the key paint.
///
/// A crab or gull the mirror will not have facing across its line is
/// turned on round to a way it will, as a hole is: set down facing right,
/// the brush could otherwise never put one on the middle column.
fn press_fairly(
    state: &EditorState,
    board: &mut Board,
    at: (u8, u8),
    period: u32,
) -> (Option<Fair>, Option<Unfair>) {
    let brush = state.brush;
    let tries = match brush {
        Brush::Hole => Direction::ALL.len() + 1,
        Brush::Castle => crate::sim::MAX_PLAYERS + 1,
        Brush::Crab | Brush::Gull => Direction::ALL.len(),
        Brush::Sand | Brush::Rock | Brush::Log | Brush::Weed | Brush::Pool => 1,
    };
    let creature = matches!(brush, Brush::Crab | Brush::Gull);
    let mut tried = 0;
    try_fairly(state, board, at, Layer::Both, |_, trial| {
        tried += 1;
        match (creature, tried) {
            (true, 2..) => turn_round(trial, at),
            (_, _) => paint(trial, at.0, at.1, brush, period),
        }
        tried < tries
    })
}

/// Turn what [`brush::facing`] reads on the tile at `at` a quarter round
/// clockwise.
fn turn_round(board: &mut Board, (x, y): (u8, u8)) {
    if let Some(dir) = brush::facing(board, x, y) {
        brush::face(board, x, y, dir.right());
    }
}

/// A change [`paint_fairly`] kept.
struct Fair {
    /// The tiles the mirror wrote.
    written: Vec<(u8, u8)>,
    /// Whether a try before the one kept was refused (and said so).
    skipped: bool,
}

/// Make a change to the tile at `at` (`change`, on a copy of the board)
/// and its copies with the mirror, and keep it; or, when the copies could
/// not be fair to every seat (weighing `layer` of the tile), leave the
/// board as it was. `change` is asked again while it says there is another
/// way to try, each try going on from the last. A refusal is said on the
/// status line, which is why a castle or a hole would not go where it was
/// put.
fn paint_fairly(
    state: &mut EditorState,
    board: &mut Board,
    at: (u8, u8),
    layer: Layer,
    tr: &Tr,
    change: impl FnMut(&EditorState, &mut Board) -> bool,
) -> Option<Fair> {
    let (fair, refused) = try_fairly(state, board, at, layer, change);
    if let Some(why) = refused {
        state.feedback = unfair_line(why, tr);
    }
    fair
}

/// [`paint_fairly`] without the saying: the change kept, if one was, and
/// the last refusal on the way.
fn try_fairly(
    state: &EditorState,
    board: &mut Board,
    at: (u8, u8),
    layer: Layer,
    mut change: impl FnMut(&EditorState, &mut Board) -> bool,
) -> (Option<Fair>, Option<Unfair>) {
    let mut trial = board.clone();
    let mut refused = None;
    loop {
        let again = change(state, &mut trial);
        match state.mirror_tile(&mut trial, at, layer) {
            Ok(written) => {
                *board = trial;
                let skipped = refused.is_some();
                return (Some(Fair { written, skipped }), refused);
            }
            Err(why) => {
                refused = Some(why);
                if !again {
                    return (None, refused);
                }
            }
        }
    }
}

/// What the status line says of a tile the mirror could not copy fairly.
fn unfair_line(why: Unfair, tr: &Tr) -> String {
    match why {
        Unfair::Line => tr.ed_mirror_line.to_string(),
        Unfair::Across => tr.ed_mirror_across.to_string(),
        Unfair::SeatTwice => fill(
            tr.ed_mirror_seats,
            &[
                ("p", &crate::app::seat_label(tr, 4)),
                ("q", &crate::app::seat_label(tr, 5)),
            ],
        ),
    }
}

impl Stroke {
    /// A stroke begun on `tile` by a press that wrote its copies at
    /// `copies`, `recorded` if the press changed the level.
    fn at(tile: (u8, u8), copies: Vec<(u8, u8)>, recorded: bool) -> Self {
        let mut done = copies;
        done.push(tile);
        Stroke {
            from: tile,
            last: tile,
            done,
            recorded,
        }
    }

    /// Carry the stroke on as a new one from the tile it is on, if it made
    /// that tile ([`EditorState::edited`]), with no step in the history yet.
    fn restart(&mut self) {
        if self.done.contains(&self.last) {
            self.from = self.last;
        }
        self.recorded = false;
    }
}

/// Carry a stroke on to the cursor's tile while its key is held: Space,
/// or the letter of the brush it is painting with (A on a pad, which holds
/// Space down for as long as it is). Each tile crossed is made like the
/// stroke's first ([`stroke_onto`]); letting go ends it.
///
/// A tile the stroke has already made, itself or as a mirror's copy, is
/// left as it is: made again from the stroke's first tile, a copy across
/// the line was painted the unmirrored way, and its own copy, written back
/// over the first tile, turned the stroke's castle into the other seat's.
fn drag(
    state: &mut EditorState,
    board: &mut Board,
    editor_keys: &EditorKeys,
    keys: &ButtonInput<KeyCode>,
    here: (u8, u8),
    tr: &Tr,
) {
    let held = keys.pressed(KeyCode::Space)
        || editor_keys
            .brush(state.brush)
            .is_some_and(|letter| keys.pressed(letter));
    let Some(mut stroke) = state.stroke.take().filter(|_| held) else {
        return;
    };
    if stroke.last == here {
        state.stroke = Some(stroke);
        return;
    }
    stroke.last = here;
    if !stroke.done.contains(&here) {
        let brush = state.brush;
        let from = stroke.from;
        let written = paint_fairly(state, board, here, Layer::Both, tr, |_, trial| {
            stroke_onto(trial, from, here, brush);
            false
        });
        if let Some(Fair { written, .. }) = written {
            stroke.done.push(here);
            stroke.done.extend(written);
            stroke.recorded |= state.edit(board, stroke.recorded);
        }
    }
    state.stroke = Some(stroke);
}

/// Turn the creature or hole on a tile to the next way round, clockwise,
/// that it will still face once settled ([`settle_facings`]): a way a wall
/// would turn it straight back from is skipped. Says whether it turned.
fn turn_here(state: &EditorState, board: &mut Board, x: u8, y: u8) -> bool {
    let Some(mut dir) = brush::facing(board, x, y) else {
        return false;
    };
    for _ in 0..3 {
        dir = dir.right();
        let mut trial = board.clone();
        brush::face(&mut trial, x, y, dir);
        settle_facings(state, &mut trial);
        if brush::facing(&trial, x, y) == Some(dir) {
            *board = trial;
            return true;
        }
    }
    false
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
    // A co-op stage grants its two hands together, P1's being the grant
    // dial's and P2's the players dial's; any other level has no hands.
    let hands = (state.kind == LevelKind::Puzzle)
        .then(|| state.second_hand.map(|p2| [state.posts, p2]))
        .flatten();
    snapshot.rules_mut().claw_hands = hands;
    let posts = hands.map_or(state.posts, |[p1, p2]| p1.saturating_add(p2));
    if state.kind == LevelKind::Puzzle {
        snapshot.set_signpost_rule(posts, crate::sim::CapPolicy::Reject);
        snapshot.rules_mut().castle_raids = false;
    } else {
        let stock = crate::sim::Rules::default();
        snapshot.set_signpost_rule(stock.signpost_cap, stock.cap_policy);
        snapshot.rules_mut().castle_raids = stock.castle_raids;
    }
    // What a pasted level said about itself that its board does not: its
    // tables and roll while the board still fits them (a table of four
    // goes with the fourth castle, a roll with the wrap), and its solution
    // until an edit (see [`EditorState::edited`]).
    let tables = state
        .tables
        .iter()
        .copied()
        .filter(|&seats| Level::table_fits(&snapshot, seats))
        .collect();
    let roll = match Level::roll_fits(&snapshot, state.roll) {
        true => state.roll,
        false => 0,
    };
    let mut level = Level::from_board(name, posts, snapshot).with_kind(state.kind);
    level.goal = state.goal;
    level.tables = tables;
    level.roll = roll;
    level.solution.clone_from(&state.solution);
    Level::parse(&level.to_text()).unwrap_or(level)
}

/// The level a check is asked about, and remembered by
/// ([`EditorState::checked`]): [`level_here`] under one name and without
/// the solution it carries, so that naming the level or storing a
/// solution does not make a checked bench read as unchecked.
pub(super) fn checked_level(state: &EditorState, board: &Board) -> Level {
    let mut level = level_here(state, board, "Custom");
    level.solution.clear();
    level
}

/// Whether a check has come back about the level on the bench as it now
/// stands.
pub(super) fn is_checked(state: &EditorState, board: &Board) -> bool {
    state.checked.as_deref() == Some(checked_level(state, board).to_text().as_str())
}

/// The settings card's keys: N (Shift+N back) to choose a row, "+" and
/// "-" to turn the one chosen, and the two rows with letters of their own,
/// K for the flock and O for the wrap, which choose their row and turn it
/// at once. A turn is an edit like any other: a check running on the old
/// setting is dropped, not left to answer for it.
///
/// K is a stage's: a beach plays under the table's flock
/// (`match_setup::apply_terms`), and has no row for it.
fn turn_settings(
    state: &mut EditorState,
    board: &mut Board,
    editor_keys: &EditorKeys,
    pressed: &impl Fn(KeyCode) -> bool,
    (keys, caps): (&ButtonInput<KeyCode>, &crate::app::keycaps::KeyCaps),
) {
    let shift = shift_held(keys);
    if pressed(NEXT_SETTING) {
        state.setting = Setting::neighbour(state, !shift);
    }
    let mut turns = Vec::new();
    if dial_press(pressed, keys, caps, true) {
        turns.push((Setting::chosen(state), true));
    }
    if dial_press(pressed, keys, caps, false) {
        turns.push((Setting::chosen(state), false));
    }
    let rows = Setting::rows(state.kind);
    for (command, row) in [
        (Command::Gulls, Setting::Gulls),
        (Command::Wrap, Setting::Wrap),
    ] {
        if pressed(editor_keys.command(command)) && rows.contains(&row) {
            state.setting = row;
            turns.push((row, !shift));
        }
    }
    for (row, forward) in turns {
        if setting::step(state, board, row, forward) {
            state.edited(board);
        }
    }
}

/// The editor's command keys: the settings card, the solver, saving,
/// playtesting, and leaving.
///
/// Gated in the schedule on not naming (F1): while a name is being typed
/// every key is a letter, or an "o" in the name flips wrap and Escape
/// leaves for the menu. The frame the name is committed is sat out here
/// (see `EditorState::named`), because the gate lifts within that frame.
pub fn editor_commands(
    keyboard: crate::app::keycaps::Keyboard,
    (settings, device): (Res<GameSettings>, Res<LastDevice>),
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
        taken: _,
    } = news;
    if matches!(state.mode, Mode::JustClosed) {
        state.mode = Mode::Painting;
        return;
    }
    let tr = settings.tr();
    let editor_keys = EditorKeys::of(&settings);
    let pressed = editor_keys.command_press(&keys);
    if pressed(KeyCode::F3) {
        share::copy_level(&mut state, &sim.0, &mut clipboard, tr, &mut shared);
    }
    // "My levels": a new level, a code, or one off the shelf, on a list
    // that has the keys until it is put away.
    if pressed(KeyCode::F4) {
        state.mode = Mode::Levels(Box::new(LevelList::read()));
        state.offer = None;
        state.feedback.clear();
        return;
    }
    if pressed(editor_keys.command(Command::Undo)) {
        let redo = shift_held(&keys);
        let back = match redo {
            true => state.history.redo(),
            false => state.history.undo(),
        };
        state.feedback = match (back, redo) {
            (Some(text), _) => {
                put_back(&text, &mut sim, &mut state, &mut swap);
                match redo {
                    true => tr.ed_redone,
                    false => tr.ed_undone,
                }
            }
            (None, true) => tr.ed_no_redo,
            (None, false) => tr.ed_no_undo,
        }
        .into();
    }
    let board = &mut sim.0;
    turn_settings(&mut state, board, &editor_keys, &pressed, (&keys, &caps));
    if pressed(editor_keys.command(Command::Check)) {
        if state.kind == LevelKind::Arena {
            // Nothing to solve: a beach is playable or it is short of
            // castles, and that is an answer this frame rather than a
            // search on another thread.
            state.feedback = arena_report(board, tr);
        } else if state.solver.is_some() {
            state.feedback = tr.ed_already_validating.into();
        } else {
            start_check(&mut state, board);
            state.feedback = tr.ed_validating.into();
        }
    }
    if pressed(KeyCode::F6) {
        // A puzzle's verdict over a beach, or a search a beach never asked
        // for, would land on the other kind. The goal is a stage's, and a
        // beach has none to keep: it goes back to every crab, which is
        // where a stage made from a beach starts.
        state.kind = match state.kind {
            LevelKind::Puzzle => LevelKind::Arena,
            LevelKind::Arena => LevelKind::Puzzle,
        };
        state.goal = Goal::default();
        state.edited(board);
        state.feedback = match state.kind {
            LevelKind::Puzzle => tr.ed_now_puzzle.into(),
            LevelKind::Arena => tr.ed_now_arena.into(),
        };
    }
    if pressed(KeyCode::F2) {
        shelf::save_level(&mut state, board, (tr, *device), &mut saved);
    }
    if crate::app::menu_ui::enter(&keys) {
        playtest::start(&mut sim, &mut state, &mut swap);
    }
    // Leaving resets the bench, and the editor has no load: the level goes
    // with it unless it is on the shelf, so Esc asks first, as F5 does.
    //
    // Over another key's question, Esc takes that question back and stays.
    if pressed(KeyCode::Escape) && state.offer.is_some_and(|offer| offer != Offer::Leave) {
        state.offer = None;
        state.feedback.clear();
    } else if pressed(KeyCode::Escape) {
        let at_stake = unsaved_work(&state, &sim.0);
        if ask_first(
            &mut state,
            Offer::Leave,
            at_stake,
            Offer::Leave.warning(tr, *device),
        ) {
            next_screen.set(Screen::Menu);
        }
    }
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
    tr: &Tr,
) {
    playtest::end_playtest(sim, state, commands, sprites);
    // Whatever was mid-way (an offer, a check on another thread) was asked
    // of the moment that failed, and is let go with it.
    state.offer = None;
    state.drop_check();
    state.edited(&mut sim.0);
    state.feedback = tr.ed_failed.into();
}

/// Rebuild the static board sprites after an edit, with the bench's own
/// marks over them ([`spawn_bench_marks`]).
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
    spawn_bench_marks(&mut commands, &sim.0, &state, &art);
}

/// A solution's signposts, drawn on the bench where they stand: faint, as
/// the hint draws its own, and only while the board is being edited, a
/// playtest being the author's own go at it.
#[derive(Component)]
pub struct SolutionGhost;

/// What the bench draws over the board and no level does: the column and
/// row numbers round its rim, which are the numbers a check's answer and a
/// level file name tiles by, and the solution it carries as faint
/// signposts. Static, so redrawn with the board after every edit, which is
/// also what takes the solution's away once an edit lets it go.
fn spawn_bench_marks(
    commands: &mut Commands,
    board: &Board,
    state: &EditorState,
    art: &crate::app::art::Art,
) {
    use crate::app::board_render::{BoardStatic, FENCE_DEPTH};
    use crate::app::layout::{self, TILE};
    // Drawn larger and shown smaller, as the score pips are, so the
    // numbers stay sharp at the closest the camera comes.
    let oversample = layout::MAX_ZOOM;
    let ink = crate::app::palette::DARK_INK.with_alpha(0.6);
    let label = |commands: &mut Commands, text: String, at: Vec2| {
        commands.spawn((
            BoardStatic,
            Text2d::new(text),
            crate::app::menu_ui::display_font(18.0 * oversample),
            TextColor(ink),
            Transform::from_translation(at.extend(layout::z::WALL))
                .with_scale(Vec3::splat(1.0 / oversample)),
        ));
    };
    let rim = TILE / 2.0 + FENCE_DEPTH + 12.0;
    for x in 0..board.width() {
        let top = layout::tile_center(board, x, 0);
        label(commands, x.to_string(), top + Vec2::new(0.0, rim));
    }
    for y in 0..board.height() {
        let left = layout::tile_center(board, 0, y);
        label(commands, y.to_string(), left - Vec2::new(rim, 0.0));
    }
    if state.is_testing() {
        return;
    }
    spawn_mirror_lines(commands, board, state.symmetry);
    let size = Some(Vec2::splat(layout::SIGN_SIZE));
    for &(x, y, dir, seat) in &state.solution {
        if x >= board.width() || y >= board.height() {
            continue;
        }
        let color = crate::app::palette::player_color(seat).lighter(0.2);
        commands
            .spawn((
                BoardStatic,
                SolutionGhost,
                Sprite {
                    image: art.sign_shape.clone(),
                    color: color.with_alpha(0.22),
                    custom_size: size,
                    ..default()
                },
                Transform::from_translation(
                    layout::tile_center(board, x, y).extend(layout::z::SIGNPOST - 0.1),
                )
                .with_rotation(layout::dir_rotation(dir)),
            ))
            .with_child((
                Sprite {
                    image: art.sign_paint.clone(),
                    color: color.with_alpha(0.5),
                    custom_size: size,
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.01)),
            ));
    }
}

/// The lines the bench's mirror paints across ([`Symmetry`]), faint over
/// the sand while it is on: down the middle, across it, both, or a mark
/// at the centre a half turn goes round.
#[derive(Component)]
pub struct MirrorLine;

fn spawn_mirror_lines(commands: &mut Commands, board: &Board, symmetry: Symmetry) {
    use crate::app::board_render::BoardStatic;
    use crate::app::layout::{self, TILE};
    let first = layout::tile_center(board, 0, 0);
    let last = layout::tile_center(board, board.width() - 1, board.height() - 1);
    let centre = (first + last) / 2.0;
    let (w, h) = (
        f32::from(board.width()) * TILE,
        f32::from(board.height()) * TILE,
    );
    let ink = crate::app::palette::player_color(0).with_alpha(0.45);
    let mut line = |size: Vec2, turn: f32| {
        commands.spawn((
            BoardStatic,
            MirrorLine,
            Sprite {
                color: ink,
                custom_size: Some(size),
                ..default()
            },
            Transform::from_translation(centre.extend(layout::z::SIGNPOST - 0.2))
                .with_rotation(Quat::from_rotation_z(turn)),
        ));
    };
    let (down, across, mark) = match symmetry {
        Symmetry::Off => (false, false, false),
        Symmetry::LeftRight => (true, false, false),
        Symmetry::TopBottom => (false, true, false),
        Symmetry::Both => (true, true, false),
        Symmetry::HalfTurn => (false, false, true),
    };
    if down {
        line(Vec2::new(4.0, h), 0.0);
    }
    if across {
        line(Vec2::new(w, 4.0), 0.0);
    }
    if mark {
        line(Vec2::splat(TILE * 0.3), std::f32::consts::FRAC_PI_4);
    }
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
    pub(super) fn bench_app() -> App {
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
        app.init_resource::<crate::app::gamepad::PadFace>();
        app.init_resource::<crate::app::gamepad::PadSeats>();
        app.init_resource::<LastDevice>();
        app.init_resource::<crate::app::Controllers>();
        app.init_resource::<EditorOpening>();
        app.insert_resource(crate::app::Campaign::tide_pool());
        app.init_resource::<crate::app::progress::Progress>();
        app.init_resource::<SaveChecks>();
        app
    }

    /// `key` alone pressed for one frame, then let go for one, as a hand
    /// presses it: the game runs frames with no key down between any two
    /// presses, and a question taken back on such a frame passed every
    /// test that pressed twice with no frame between.
    pub(super) fn tap(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(key);
        app.update();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(key);
        keys.clear();
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

    /// F5 keeps what is built, centred on the next size up, and Shift+F5
    /// goes down; Z takes a resize back like any edit, a stroke goes back
    /// in one, and Shift+Z makes it again. A resize used to throw the
    /// level away and ask twice first.
    #[test]
    fn a_resize_keeps_the_level_and_z_takes_edits_back() {
        use crate::app::i18n::EN;
        let (mut app, cursor) = painting_app((1, 1));
        {
            let world = app.world_mut();
            let board = world.resource::<Sim>().0.clone();
            world.resource_mut::<EditorState>().begin_history(&board);
        }
        let tile = |app: &App, x, y| app.world().resource::<Sim>().0.tile_at(x, y);
        let size = |app: &App| {
            let board = &app.world().resource::<Sim>().0;
            (board.width(), board.height())
        };
        let feedback = |app: &App| app.world().resource::<EditorState>().feedback.clone();
        // A stroke of rock over three tiles.
        hold(&mut app, KeyCode::KeyR);
        step_to(&mut app, cursor, (2, 1));
        step_to(&mut app, cursor, (3, 1));
        tap(&mut app, KeyCode::Space);
        let sand = size(&app);
        tap(&mut app, KeyCode::F5);
        assert_eq!(
            size(&app),
            (9, 7),
            "up from 6x5, off the list, to its first"
        );
        let (dx, dy) = (1, 1);
        for x in 1..=3 {
            assert_eq!(tile(&app, x + dx, 1 + dy), TileKind::Rock, "x {x}");
        }
        let at = app.world().get::<Cursor>(cursor).expect("cursor");
        assert_eq!((at.x, at.y), (3 + dx, 1 + dy), "the cursor kept its tile");
        assert_eq!(
            feedback(&app),
            fill(EN.ed_resized, &[("w", "9"), ("h", "7")])
        );

        tap(&mut app, KeyCode::KeyZ);
        assert_eq!(size(&app), sand, "Z takes the resize back");
        assert_eq!(feedback(&app), EN.ed_undone);
        tap(&mut app, KeyCode::KeyZ);
        for x in 1..=3 {
            assert_eq!(tile(&app, x, 1), TileKind::Empty, "the stroke went whole");
        }
        tap(&mut app, KeyCode::KeyZ);
        assert_eq!(feedback(&app), EN.ed_no_undo);
        // Shift+Z makes them again, in order.
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::ShiftLeft);
        keys.press(KeyCode::KeyZ);
        app.update();
        assert_eq!(feedback(&app), EN.ed_redone);
        assert_eq!(tile(&app, 2, 1), TileKind::Rock);
        assert_eq!(size(&app), sand);

        // Down from the smallest wraps to the largest, and up from the
        // largest to the smallest cuts what fell off the edge, said so.
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::ShiftLeft);
        keys.press(KeyCode::F5);
        app.update();
        assert_eq!(size(&app), (20, 13));
        app.world_mut()
            .resource_mut::<Sim>()
            .0
            .set_tile(0, 0, TileKind::Rock);
        tap(&mut app, KeyCode::F5);
        assert_eq!(size(&app), (9, 7));
        assert_eq!(
            feedback(&app),
            fill(EN.ed_resized_cut, &[("w", "9"), ("h", "7")])
        );
    }

    /// Esc resets the bench on the way out, so over unsaved work it asks
    /// first, as F4 does; over a blank beach or a level just saved
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

    /// F2 onto a file this bench did not write asks first: the first
    /// press leaves the other level's file as it was and names the level
    /// in it, and only a second press replaces it. Saved at once, the note
    /// that a level had been replaced arrived with that level already
    /// gone.
    #[test]
    fn saving_over_another_levels_file_asks_twice() {
        let mut app = bench_app();
        app.add_systems(Update, (editor_input, editor_commands));
        // A name of this test's own: the shelf is shared by every test in
        // the process.
        let name = "Пляж Overwrite Probe";
        app.world_mut().resource_mut::<EditorState>().name = name.to_string();
        let path = save_path(name);
        crate::app::paths::write_atomic(&path, "name: Their Beach\n").expect("wrote");

        let on_disk = || std::fs::read_to_string(&path).expect("reads");
        tap(&mut app, KeyCode::F2);
        assert_eq!(on_disk(), "name: Their Beach\n", "the first press asks");
        assert_eq!(
            app.world().resource::<EditorState>().feedback,
            fill(
                crate::app::i18n::EN.ed_save_taken,
                &[("name", "Their Beach")]
            ),
            "naming the level it would replace"
        );
        // Anything else pressed in between takes the offer back.
        tap(&mut app, KeyCode::KeyO);
        tap(&mut app, KeyCode::F2);
        assert_eq!(on_disk(), "name: Their Beach\n", "asked again");
        tap(&mut app, KeyCode::F2);
        assert!(on_disk().contains(name), "the second press saves");
        // Now it is this bench's own file, and saves at once.
        app.world_mut().resource_mut::<Sim>().0.set_wrap(false);
        tap(&mut app, KeyCode::F2);
        assert!(
            app.world().resource::<EditorState>().offer.is_none(),
            "no question about its own file"
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

    /// A file holding a level of this very name is still somebody's level
    /// when this bench did not write it: one saved in another session, or
    /// the one a pasted code of the same name came from. Saved at once, a
    /// second "My Map" took the first one's file and its cleared tick.
    #[test]
    fn a_namesake_saved_elsewhere_is_asked_about() {
        let mut app = bench_app();
        app.add_systems(Update, (editor_input, editor_commands));
        let name = "Namesake Probe";
        app.world_mut().resource_mut::<EditorState>().name = name.to_string();
        let path = save_path(name);
        let theirs = format!("name: {name}\nold: true\n");
        crate::app::paths::write_atomic(&path, &theirs).expect("wrote");
        app.world_mut().resource_mut::<Sim>().0.set_wrap(true);

        tap(&mut app, KeyCode::F2);
        assert_eq!(std::fs::read_to_string(&path).expect("reads"), theirs);
        assert_eq!(
            app.world().resource::<EditorState>().offer,
            Some(Offer::Overwrite),
            "the first press asks"
        );
        tap(&mut app, KeyCode::F2);
        assert!(
            !std::fs::read_to_string(&path)
                .expect("reads")
                .contains("old: true")
        );
        let _ = std::fs::remove_file(&path);
    }

    /// The editor opens on a name no saved level has, and a fresh map
    /// takes another: every bench used to open on "My Map", so the second
    /// level left unnamed was saved over the first.
    #[test]
    fn a_fresh_bench_takes_a_name_no_saved_level_has() {
        // A language of the test's own, so its default name's files are
        // this test's alone on the shared shelf.
        let lang = crate::app::i18n::Lang::It;
        let tr = lang.tr();
        let settings = GameSettings {
            language: lang,
            ..GameSettings::default()
        };
        let mut app = bench_app();
        app.insert_resource(settings);
        app.insert_state(Screen::Editor);
        app.add_systems(OnEnter(Screen::Editor), enter_editor);
        app.add_systems(Update, (editor_input, editor_commands).chain());
        let base = tr.ed_default_name;
        let second = format!("{base} 2");
        let third = format!("{base} 3");
        for name in [base, &second, &third] {
            let _ = std::fs::remove_file(save_path(name));
        }
        crate::app::paths::write_atomic(&save_path(base), "name: x\n").expect("wrote");
        app.update();
        let name = |app: &App| app.world().resource::<EditorState>().name.clone();
        assert_eq!(name(&app), second, "the default is taken");

        // Saved, then a fresh map: a new level under a new name, which the
        // next F2 does not write over the one just saved.
        app.world_mut().resource_mut::<Sim>().0.set_wrap(true);
        tap(&mut app, KeyCode::F2);
        let saved = std::fs::read_to_string(save_path(&second)).expect("saved");
        {
            let world = app.world_mut();
            let board = world.resource::<Sim>().0.clone();
            world.resource_mut::<EditorState>().start_afresh(tr, &board);
        }
        assert_eq!(name(&app), third, "a fresh map, a fresh name");
        assert!(app.world().resource::<EditorState>().mine.is_empty());
        app.world_mut()
            .resource_mut::<Sim>()
            .0
            .set_tile(1, 1, TileKind::Rock);
        tap(&mut app, KeyCode::F2);
        assert_eq!(
            std::fs::read_to_string(save_path(&second)).expect("still there"),
            saved,
            "the level saved before the fresh map is left alone"
        );
        assert!(save_path(&third).exists());
        // Renamed onto the level saved before, the fresh map asks.
        app.world_mut().resource_mut::<EditorState>().name = second.clone();
        tap(&mut app, KeyCode::F2);
        assert_eq!(
            app.world().resource::<EditorState>().offer,
            Some(Offer::Overwrite)
        );
        for name in [base, &second, &third] {
            let _ = std::fs::remove_file(save_path(name));
        }
    }

    /// A fresh map and a turn of F6 put the every-crab goal back: a pasted
    /// stage's goal stuck to whatever was built after it, out of sight.
    #[test]
    fn a_fresh_map_or_a_kind_flip_forgets_a_pasted_goal() {
        let mut app = bench_app();
        app.add_systems(Update, (editor_input, editor_commands).chain());
        let goal = |app: &App| app.world().resource::<EditorState>().goal;
        app.world_mut().resource_mut::<EditorState>().goal = Goal::Bank(4);
        app.world_mut()
            .resource_mut::<EditorState>()
            .start_afresh(&crate::app::i18n::EN, &sand());
        assert_eq!(goal(&app), Goal::AllCrabs, "a fresh map");
        app.world_mut().resource_mut::<EditorState>().goal = Goal::Survive;
        tap(&mut app, KeyCode::F6);
        assert_eq!(goal(&app), Goal::AllCrabs, "F6");
    }

    /// F4 as the editor's commands answer it, with `code` standing in for
    /// the clipboard, which a test must not touch (see `codes`' tests).
    fn paste_on_f4(
        code: Res<TestCode>,
        keys: Res<ButtonInput<KeyCode>>,
        settings: Res<GameSettings>,
        bench: Workbench,
        mut news: EditorNews,
        mut swap: BoardSwap,
    ) {
        let Workbench { mut sim, mut state } = bench;
        if keys.just_pressed(KeyCode::F4) {
            share::paste_level(
                &mut state,
                &mut sim,
                crate::share::decode(&code.0),
                (settings.tr(), LastDevice::Keyboard),
                &mut news.taken,
                &mut swap,
            );
        }
    }

    #[derive(Resource)]
    struct TestCode(String);

    fn code_of(text: &str) -> TestCode {
        TestCode(crate::share::encode(
            crate::share::Kind::Level,
            text.as_bytes(),
        ))
    }

    /// A pasted beach saved with F2 keeps the tables and roll its code
    /// named, and a pasted stage its solution until an edit changes the
    /// board it wins.
    #[test]
    fn a_pasted_level_saves_with_its_own_lines() {
        let mut app = bench_app();
        app.add_systems(Update, (editor_input, editor_commands, paste_on_f4).chain());
        let beach = "name: Seats Probe\nposts: 3\nkind: arena\nwrap: on\nseats: 4 6\nroll: 3\n\
            spawner: 1,1 R 60\nmap:\n\
            + + + + + + +\n 0 1 2 3 4 5 \n+ + + + + + +\n . . . . . . \n+ + + + + + +\n";
        app.insert_resource(code_of(beach));
        tap(&mut app, KeyCode::F4);
        let path = save_path("Seats Probe");
        let _ = std::fs::remove_file(&path);
        tap(&mut app, KeyCode::F2);
        let filed = std::fs::read_to_string(&path).expect("saved");
        assert!(filed.contains("seats: 4 6"), "{filed}");
        assert!(filed.contains("roll: 3"), "{filed}");
        let _ = std::fs::remove_file(&path);

        let mut board = sand();
        board.set_tile(4, 2, TileKind::Castle(0));
        board.spawn_crab(1, 2, Direction::Down, Handedness::Left, CrabKind::Common);
        let mut stage = Level::from_board("Solution Probe", 1, board);
        stage.solution = vec![(1, 2, Direction::Right, 0)];
        app.insert_resource(code_of(&stage.to_text()));
        // The beach is saved, so this paste goes at once.
        tap(&mut app, KeyCode::F4);
        let path = save_path("Solution Probe");
        let _ = std::fs::remove_file(&path);
        tap(&mut app, KeyCode::F2);
        let filed = std::fs::read_to_string(&path).expect("saved");
        assert!(filed.contains("solution: 1,2 R"), "{filed}");
        tap(&mut app, KeyCode::KeyO);
        tap(&mut app, KeyCode::F2);
        let filed = std::fs::read_to_string(&path).expect("saved");
        assert!(!filed.contains("solution:"), "an edit lets it go: {filed}");
        let _ = std::fs::remove_file(&path);
    }

    /// The check a paste starts is handed the level the bench will save,
    /// a stage's rule stamped on, not the code as it came: a stage code
    /// that left raids on was certified with them and saved without.
    #[test]
    fn a_paste_checks_the_level_the_bench_will_save() {
        let mut app = bench_app();
        app.add_systems(Update, paste_on_f4);
        let mut board = sand();
        board.set_tile(4, 2, TileKind::Castle(0));
        board.spawn_crab(1, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        let code = Level::from_board("Raids Probe", 1, board).to_text();
        assert!(!code.contains("raids: off"), "the code leaves raids on");
        app.insert_resource(code_of(&code));
        tap(&mut app, KeyCode::F4);
        let world = app.world();
        let state = world.resource::<EditorState>();
        let checking = &state.solver.as_ref().expect("a check").level;
        let bench = checked_level(state, &world.resource::<Sim>().0).to_text();
        assert_eq!(checking, &bench);
        assert!(checking.contains("raids: off"), "{checking}");
    }

    /// A wall put up in front of a crab or a gull turns it, there and
    /// then, the way the level reader turns it: the bench, its file and
    /// the bench read back from its file are one level.
    #[test]
    fn a_wall_in_front_of_a_creature_turns_it_on_the_bench() {
        let mut board = sand();
        board.spawn_crab(2, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        board.spawn_gull(3, 3, Direction::Right);
        let mut app = bench_app();
        app.insert_resource(Sim(board));
        let mut cursor = Cursor::seated(0);
        (cursor.x, cursor.y) = (2, 2);
        let cursor = app.world_mut().spawn((cursor, Transform::default())).id();
        app.add_systems(Update, editor_input);
        tap(&mut app, KeyCode::ArrowRight);
        app.world_mut().get_mut::<Cursor>(cursor).expect("cursor").x = 3;
        app.world_mut().get_mut::<Cursor>(cursor).expect("cursor").y = 3;
        tap(&mut app, KeyCode::ArrowRight);

        let world = app.world();
        let (state, bench) = (world.resource::<EditorState>(), &world.resource::<Sim>().0);
        assert_ne!(bench.crabs()[0].dir, Direction::Right, "the crab turned");
        assert_ne!(bench.gulls()[0].dir, Direction::Right, "the gull turned");
        let filed = level_here(state, bench, "Facing");
        let read = filed.board();
        assert_eq!(read.crabs()[0].dir, bench.crabs()[0].dir);
        assert_eq!(read.gulls()[0].dir, bench.gulls()[0].dir);
        assert_eq!(
            level_here(state, &read, "Facing").to_text(),
            filed.to_text(),
            "read back, the same level"
        );
    }

    /// A save says what it saved as by the file's name alone, warns first
    /// about a level nobody can win, and checks a stage nobody has checked.
    #[test]
    fn a_save_names_its_file_and_warns() {
        use crate::app::i18n::EN;
        let mut app = bench_app();
        app.add_systems(Update, (editor_input, editor_commands, poll_solver).chain());
        let name = "Warning Probe";
        app.world_mut().resource_mut::<EditorState>().name = name.into();
        app.world_mut().resource_mut::<Sim>().0.spawn_crab(
            1,
            1,
            Direction::Right,
            Handedness::Left,
            CrabKind::Common,
        );
        let path = save_path(name);
        let _ = std::fs::remove_file(&path);
        tap(&mut app, KeyCode::F2);
        let said = app.world().resource::<EditorState>().feedback.clone();
        let file = path.file_name().expect("a file").to_string_lossy();
        let filed = fill(EN.ed_saved_to, &[("file", &file)]);
        assert_eq!(said, format!("{} - {filed}", EN.ed_puzzle_no_castle));
        assert!(!said.contains('/'), "no folders: {said}");

        app.world_mut()
            .resource_mut::<Sim>()
            .0
            .set_tile(4, 1, TileKind::Castle(0));
        tap(&mut app, KeyCode::F2);
        let said = app.world().resource::<EditorState>().feedback.clone();
        // The check may already be back within the frame that saved.
        let done = format!("{} - {filed}", EN.ed_solvable_free);
        assert!(
            said == format!("{} - {filed}", EN.ed_validating) || said == done,
            "{said}"
        );
        // Saved unchecked, a stage is checked on the way, and the answer
        // finishes the save: the verdict first, then the file.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        loop {
            app.update();
            if app.world().resource::<EditorState>().solver.is_none() {
                break;
            }
            std::thread::yield_now();
        }
        assert_eq!(
            app.world().resource::<EditorState>().feedback,
            format!("{} - {filed}", EN.ed_solvable_free)
        );
        // Checked, and saved again as it stands: nothing to warn about.
        tap(&mut app, KeyCode::F2);
        assert_eq!(app.world().resource::<EditorState>().feedback, filed);
        let _ = std::fs::remove_file(&path);
    }

    /// The way through a save's check finds goes into the file it saved,
    /// as the level's `solution:`.
    #[test]
    fn a_save_check_writes_the_solution_it_finds() {
        let mut app = bench_app();
        app.add_systems(Update, (editor_input, editor_commands, poll_solver).chain());
        let name = "Save Check Probe";
        {
            let mut state = app.world_mut().resource_mut::<EditorState>();
            state.name = name.into();
            state.posts = 1;
        }
        {
            let board = &mut app.world_mut().resource_mut::<Sim>().0;
            board.set_tile(3, 1, TileKind::Castle(0));
            for x in 0..6 {
                board.set_wall(x, 2, Direction::Down, true);
                if x != 3 {
                    board.set_wall(x, 2, Direction::Up, true);
                }
            }
            board.spawn_crab(1, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        }
        let path = save_path(name);
        let _ = std::fs::remove_file(&path);
        tap(&mut app, KeyCode::F2);
        assert!(
            !std::fs::read_to_string(&path)
                .expect("saved")
                .contains("solution:")
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        while app.world().resource::<EditorState>().solver.is_some() {
            app.update();
            std::thread::yield_now();
        }
        let text = std::fs::read_to_string(&path).expect("saved");
        assert!(text.contains("solution: 3,2 U"), "{text}");
        let state = app.world().resource::<EditorState>();
        let board = &app.world().resource::<Sim>().0;
        assert!(!unsaved_work(state, board), "and is filed");
        let _ = std::fs::remove_file(&path);
    }

    /// A save still waiting on its check when the editor is left gets its
    /// solution all the same, off the bench; and a file saved over in the
    /// meantime is not the level the check was about, and is left alone.
    #[test]
    fn a_save_check_outlives_the_editor() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = bench_app();
        // No poll here: the bench must not take the answer first.
        app.add_systems(Update, (editor_input, editor_commands).chain());
        let name = "Orphan Check Probe";
        {
            let mut state = app.world_mut().resource_mut::<EditorState>();
            state.name = name.into();
            state.posts = 1;
        }
        {
            let board = &mut app.world_mut().resource_mut::<Sim>().0;
            board.set_tile(3, 1, TileKind::Castle(0));
            for x in 0..6 {
                board.set_wall(x, 2, Direction::Down, true);
                if x != 3 {
                    board.set_wall(x, 2, Direction::Up, true);
                }
            }
            board.spawn_crab(1, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        }
        let path = save_path(name);
        let _ = std::fs::remove_file(&path);
        tap(&mut app, KeyCode::F2);
        assert!(app.world().resource::<EditorState>().solver.is_some());
        app.world_mut()
            .run_system_once(exit_editor)
            .expect("left the editor");
        let settle = |app: &mut App| {
            while !app.world().resource::<SaveChecks>().0.is_empty() {
                app.world_mut()
                    .run_system_once(finish_orphaned_saves)
                    .expect("polled");
                std::thread::yield_now();
            }
        };
        settle(&mut app);
        let text = std::fs::read_to_string(&path).expect("saved");
        assert!(text.contains("solution: 3,2 U"), "{text}");

        // Saved over while the check runs: the newer file stands.
        let mut check = validate::Validation::pending();
        check.save = Some((path.clone(), name.into()));
        let orphan = check.orphan().expect("the file reads");
        std::fs::write(&path, "name: Newer\n").expect("saved over");
        *orphan.slot.lock().unwrap() = Some(validate::Verdict {
            outcome: crate::sim::SolveOutcome::Found(vec![(3, 2, Direction::Up, 0)]),
            needs: None,
            shared: None,
            coop: false,
        });
        app.world_mut().resource_mut::<SaveChecks>().0.push(orphan);
        settle(&mut app);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "name: Newer\n"
        );
        let _ = std::fs::remove_file(&path);
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

    /// A check still running was handed the board before a resize, so the
    /// resize drops it rather than let it answer for the beach after.
    #[test]
    fn a_resize_drops_the_check_on_the_old_beach() {
        let mut app = bench_app();
        app.insert_resource(EditorState {
            solver: Some(Validation::pending()),
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
        use setting::GULL_PERIODS;
        let mut app = bench_app();
        app.add_systems(Update, editor_commands);
        let period = |app: &mut App, was: u32| {
            app.world_mut()
                .resource_mut::<Sim>()
                .0
                .rules_mut()
                .gull_period = was;
            tap(app, KeyCode::KeyK);
            app.world().resource::<Sim>().0.rules().gull_period
        };
        assert_eq!(period(&mut app, 0), GULL_PERIODS[1]);
        assert_eq!(period(&mut app, GULL_PERIODS[1]), GULL_PERIODS[2]);
        assert_eq!(period(&mut app, GULL_PERIODS[3]), 0, "and round to off");
        assert_eq!(
            period(&mut app, 333),
            GULL_PERIODS[2],
            "off the dial, beside it"
        );
    }

    /// The Enter that commits a name must not also start a playtest: the
    /// commands sit out the frame the name was committed on, and read the
    /// key normally from the next frame.
    #[test]
    fn committing_a_name_does_not_start_a_playtest() {
        let mut app = bench_app();
        app.add_systems(Update, editor_commands);
        app.world_mut().resource_mut::<EditorState>().mode = Mode::JustClosed;

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
            (editor_input, editor_commands.run_if(not(editor_has_keys))).chain(),
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
                state.solver = Some(Validation::pending());
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
        assert_eq!(
            test,
            "arrow keys place | Space remove | R restart | Esc end test"
        );
        let test = spell_editor_keys(EN.ed_playtest_prompt, &EN, &caps, &settings);
        assert_eq!(test, "WASD place | X remove | R restart | Esc end test");
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
        assert!(moves.starts_with("arrow keys move |"), "{moves}");

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

    /// Every command keeps a key of its own under any bindings: with P1
    /// moving on the arrows and placing on IJKL, the walls moved onto IJKL
    /// and K toggled a wall where the prompt said "K gulls". The command
    /// moves to its function key, and the line that names it names that
    /// key: the prompt for the check and the turn, the settings card's row
    /// for the flock and the wrap.
    #[test]
    fn every_command_has_a_key_in_every_binding_scheme() {
        use crate::app::binds::Action;
        use crate::app::i18n::EN;
        let caps = crate::app::keycaps::KeyCaps::default();
        let mut ijkl = GameSettings::default();
        for (action, key) in [
            (Action::MoveUp, KeyCode::ArrowUp),
            (Action::MoveDown, KeyCode::ArrowDown),
            (Action::MoveLeft, KeyCode::ArrowLeft),
            (Action::MoveRight, KeyCode::ArrowRight),
        ] {
            ijkl.binds[0].set(action, key);
        }
        ijkl.commit = crate::app::settings::CommitScheme::Ijkl;
        let mut letters = GameSettings::default();
        for (action, key) in [
            (Action::MoveUp, KeyCode::KeyV),
            (Action::MoveDown, KeyCode::KeyK),
            (Action::MoveLeft, KeyCode::KeyO),
            (Action::MoveRight, KeyCode::KeyT),
        ] {
            letters.binds[0].set(action, key);
        }
        let mut undo_taken = GameSettings::default();
        undo_taken.binds[0].set(Action::MoveUp, KeyCode::KeyZ);
        let wasd = GameSettings {
            commit: crate::app::settings::CommitScheme::Wasd,
            ..GameSettings::default()
        };
        for settings in [
            GameSettings::default(),
            ijkl.clone(),
            wasd,
            letters,
            undo_taken,
        ] {
            let keys = EditorKeys::of(&settings);
            let prompt = spell_editor_keys(EN.ed_prompt, &EN, &caps, &settings);
            for command in Command::ALL {
                let key = keys.command(command);
                assert!(!keys.shadows(key), "{command:?} on {key:?}");
                let row =
                    |setting| card::Row::Setting(setting).key(LastDevice::Keyboard, &keys, &caps);
                match command {
                    Command::Check | Command::Turn | Command::Undo => {
                        let named = format!("{} ", caps.label(key));
                        assert!(prompt.contains(&named), "{command:?}: {prompt}");
                    }
                    Command::Gulls => assert_eq!(row(Setting::Gulls), caps.label(key)),
                    Command::Wrap => assert_eq!(row(Setting::Wrap), caps.label(key)),
                }
            }
            assert!(!prompt.contains('{'), "every slot filled: {prompt}");
        }
        let stock = spell_editor_keys(EN.ed_prompt, &EN, &caps, &GameSettings::default());
        assert!(
            stock.contains("T turn") && stock.contains("V check"),
            "{stock}"
        );
        // N, which chooses the card's row, is a key no binding can take.
        assert!(!crate::app::binds::bindable(NEXT_SETTING));

        // Through the keys themselves: K is the down wall, F7 the gulls.
        let mut app = bench_app();
        app.insert_resource(ijkl);
        let mut cursor = Cursor::seated(0);
        (cursor.x, cursor.y) = (2, 2);
        app.world_mut().spawn((cursor, Transform::default()));
        app.add_systems(Update, (editor_input, editor_commands).chain());
        let period = |app: &App| app.world().resource::<Sim>().0.rules().gull_period;
        tap(&mut app, KeyCode::KeyK);
        assert_eq!(period(&app), 0, "K is a wall key here");
        assert!(
            app.world()
                .resource::<Sim>()
                .0
                .wall_at(2, 2, Direction::Down)
        );
        tap(&mut app, KeyCode::F7);
        assert_eq!(period(&app), setting::GULL_PERIODS[1], "F7 steps the gulls");
        tap(&mut app, KeyCode::F8);
        assert!(
            !app.world().resource::<Sim>().0.wrap(),
            "O is free, so F8 is not"
        );
        tap(&mut app, KeyCode::KeyO);
        assert!(app.world().resource::<Sim>().0.wrap(), "O wraps");
    }

    /// Esc over F4's or F2's question takes the question back and
    /// stays: the editor took the question back and then left, with the
    /// level the question was about.
    #[test]
    fn escape_over_a_question_only_takes_it_back() {
        let mut app = bench_app();
        app.insert_state(Screen::Editor);
        app.add_systems(Update, (editor_input, editor_commands).chain());
        let screen = |app: &App| *app.world().resource::<State<Screen>>().get();
        // A level just saved, so Esc on its own would leave at once.
        let name = "Escape Probe";
        app.world_mut().resource_mut::<EditorState>().name = name.into();
        let path = save_path(name);
        let _ = std::fs::remove_file(&path);
        app.world_mut().resource_mut::<Sim>().0.set_wrap(true);
        tap_and_settle(&mut app, KeyCode::F2);
        // F2 again under another name whose file is somebody's: a question.
        let other = "Escape Probe Other";
        crate::app::paths::write_atomic(&save_path(other), "name: Other\n").expect("wrote");
        app.world_mut().resource_mut::<EditorState>().name = other.into();
        app.world_mut().resource_mut::<EditorState>().filed = None;
        tap_and_settle(&mut app, KeyCode::F2);
        assert_eq!(
            app.world().resource::<EditorState>().offer,
            Some(Offer::Overwrite)
        );
        tap_and_settle(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Screen::Editor, "still at the bench");
        let state = app.world().resource::<EditorState>();
        assert_eq!(state.offer, None, "the question is taken back");
        assert!(state.feedback.is_empty(), "{}", state.feedback);
        let _ = std::fs::remove_file(save_path(other));
        let _ = std::fs::remove_file(&path);
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

    /// The bench with a cursor at `at`, and the painting keys answered.
    fn painting_app(at: (u8, u8)) -> (App, Entity) {
        let mut app = bench_app();
        let mut cursor = Cursor::seated(0);
        (cursor.x, cursor.y) = at;
        let cursor = app.world_mut().spawn((cursor, Transform::default())).id();
        app.add_systems(Update, (editor_input, editor_commands).chain());
        (app, cursor)
    }

    /// The cursor moved to `to` while whatever is held stays held, as a
    /// frame of a drag.
    fn step_to(app: &mut App, cursor: Entity, to: (u8, u8)) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.clear();
        let mut at = app.world_mut().get_mut::<Cursor>(cursor).expect("cursor");
        (at.x, at.y) = to;
        app.update();
    }

    /// `key` pressed and held from this frame on.
    fn hold(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(key);
        app.update();
    }

    /// A brush held down and moved paints every tile it crosses as it
    /// painted the first: the same seat's castle, not the next seat's on
    /// each tile; the same crab; and the eraser clears a row. Let go, the
    /// cursor moves without painting.
    #[test]
    fn holding_a_brush_down_paints_a_stroke() {
        let (mut app, cursor) = painting_app((1, 1));
        let tile = |app: &App, x, y| app.world().resource::<Sim>().0.tile_at(x, y);
        hold(&mut app, KeyCode::KeyC);
        assert_eq!(tile(&app, 1, 1), TileKind::Castle(0));
        step_to(&mut app, cursor, (2, 1));
        step_to(&mut app, cursor, (3, 1));
        for x in 1..=3 {
            assert_eq!(tile(&app, x, 1), TileKind::Castle(0), "x {x}");
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyC);
        step_to(&mut app, cursor, (4, 1));
        assert_eq!(tile(&app, 4, 1), TileKind::Empty, "let go");

        // Space drags the loaded brush: the eraser, back along the row.
        tap(&mut app, KeyCode::KeyX);
        hold(&mut app, KeyCode::Space);
        step_to(&mut app, cursor, (3, 1));
        step_to(&mut app, cursor, (2, 1));
        for x in 2..=4 {
            assert_eq!(tile(&app, x, 1), TileKind::Empty, "x {x}");
        }
        assert_eq!(tile(&app, 1, 1), TileKind::Castle(0), "short of the stroke");

        // A crab, copied kind, claw and all.
        step_to(&mut app, cursor, (1, 3));
        tap(&mut app, KeyCode::KeyB);
        hold(&mut app, KeyCode::KeyB);
        step_to(&mut app, cursor, (2, 3));
        let board = &app.world().resource::<Sim>().0;
        let crabs: Vec<_> = board.crabs().iter().map(|c| (c.kind, c.handed)).collect();
        assert_eq!(
            crabs,
            [
                (CrabKind::Common, Handedness::Right),
                (CrabKind::Common, Handedness::Right)
            ],
            "the second press's crab, twice"
        );
    }

    /// T turns the creature or hole under the cursor clockwise, past a way
    /// a wall would turn it straight back from, and the file reads it back
    /// facing the way the bench shows.
    #[test]
    fn t_turns_what_is_under_the_cursor() {
        let (mut app, cursor) = painting_app((2, 2));
        {
            let board = &mut app.world_mut().resource_mut::<Sim>().0;
            board.spawn_crab(2, 2, Direction::Right, Handedness::Left, CrabKind::Common);
            board.set_wall(2, 2, Direction::Down, true);
        }
        let facing = |app: &App, x, y| brush::facing(&app.world().resource::<Sim>().0, x, y);
        tap(&mut app, KeyCode::KeyT);
        assert_eq!(facing(&app, 2, 2), Some(Direction::Left), "past the wall");
        tap(&mut app, KeyCode::KeyT);
        assert_eq!(facing(&app, 2, 2), Some(Direction::Up));
        let world = app.world();
        let (state, bench) = (world.resource::<EditorState>(), &world.resource::<Sim>().0);
        let read = level_here(state, bench, "Turned").board();
        assert_eq!(read.crabs()[0].dir, Direction::Up, "the file agrees");

        // A hole turns and keeps its rate.
        step_to(&mut app, cursor, (4, 1));
        tap(&mut app, KeyCode::KeyH);
        tap(&mut app, KeyCode::KeyT);
        let tile = app.world().resource::<Sim>().0.tile_at(4, 1);
        let TileKind::Spawner(hole) = tile else {
            panic!("a hole: {tile:?}");
        };
        assert_eq!(
            (hole.dir, hole.period),
            (Direction::Down, brush::SPAWNER_PERIOD)
        );
        // Bare sand has nothing to turn.
        step_to(&mut app, cursor, (0, 4));
        tap(&mut app, KeyCode::KeyT);
        assert_eq!(
            app.world().resource::<Sim>().0.tile_at(0, 4),
            TileKind::Empty
        );
    }

    /// N chooses the settings card's row and "+"/"-" turn it: the goal,
    /// the time and the holes' rate are the level's, and the file says
    /// so. On a beach "+" turns the holes, and K, a stage's flock, does
    /// nothing.
    #[test]
    fn the_card_keys_choose_and_turn_the_settings() {
        let (mut app, cursor) = painting_app((1, 1));
        let chosen = |app: &App| Setting::chosen(app.world().resource::<EditorState>());
        assert_eq!(chosen(&app), Setting::Signposts);
        tap(&mut app, NEXT_SETTING);
        assert_eq!(chosen(&app), Setting::Players);
        tap(&mut app, NEXT_SETTING);
        assert_eq!(chosen(&app), Setting::Goal);
        tap(&mut app, KeyCode::Equal);
        assert_eq!(app.world().resource::<EditorState>().goal, Goal::Survive);
        tap(&mut app, NEXT_SETTING);
        tap(&mut app, KeyCode::Equal);
        assert_eq!(
            app.world().resource::<Sim>().0.rules().round_length,
            Some(90 * crate::sim::TICKS_PER_SECOND)
        );
        // Shift+N goes back a row, and round to the last.
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::ShiftLeft);
            keys.press(NEXT_SETTING);
        }
        app.update();
        assert_eq!(chosen(&app), Setting::Goal);
        for _ in 0..3 {
            tap(&mut app, NEXT_SETTING);
        }
        assert_eq!(chosen(&app), Setting::Holes);
        tap(&mut app, KeyCode::Equal);
        tap(&mut app, KeyCode::KeyH);
        let text = {
            let world = app.world();
            let state = world.resource::<EditorState>();
            level_here(state, &world.resource::<Sim>().0, "Card").to_text()
        };
        assert!(text.contains("goal: survive"), "{text}");
        assert!(text.contains("round: 2700"), "{text}");
        assert!(text.contains("spawner: 1,1 R 90"), "{text}");

        tap(&mut app, KeyCode::F6);
        tap(&mut app, KeyCode::Equal);
        let tile = app.world().resource::<Sim>().0.tile_at(1, 1);
        assert!(
            matches!(tile, TileKind::Spawner(s) if s.period == 120),
            "{tile:?}"
        );
        let gulls = app.world().resource::<Sim>().0.rules().gull_period;
        tap(&mut app, KeyCode::KeyK);
        assert_eq!(app.world().resource::<Sim>().0.rules().gull_period, gulls);
        let _ = cursor;
    }

    /// A check that finds a way through keeps it: drawn on the sand as
    /// faint signposts, saved as the level's `solution:`, and let go, from
    /// both, by the next edit.
    #[test]
    fn a_found_solution_is_drawn_saved_and_let_go_by_an_edit() {
        let mut board = sand();
        board.set_tile(4, 2, TileKind::Castle(0));
        board.spawn_crab(1, 2, Direction::Down, Handedness::Left, CrabKind::Common);
        let mut app = bench_app();
        app.insert_resource(Sim(board));
        app.insert_resource(crate::app::art::Art::blank());
        let name = "Ghost Probe";
        app.insert_resource(EditorState {
            posts: 1,
            name: name.into(),
            ..EditorState::default()
        });
        app.add_systems(
            Update,
            (editor_commands, poll_solver, rebuild_statics).chain(),
        );
        let ghosts = |app: &mut App| {
            let world = app.world_mut();
            world.query::<&SolutionGhost>().iter(world).count()
        };
        tap(&mut app, KeyCode::KeyV);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        while app.world().resource::<EditorState>().solver.is_some() {
            app.update();
            std::thread::yield_now();
        }
        app.update();
        let solution = app.world().resource::<EditorState>().solution.clone();
        assert!(!solution.is_empty(), "the check found one");
        assert_eq!(ghosts(&mut app), solution.len(), "drawn");

        let path = save_path(name);
        let _ = std::fs::remove_file(&path);
        tap(&mut app, KeyCode::F2);
        let filed = std::fs::read_to_string(&path).expect("saved");
        let (x, y, dir, _) = solution[0];
        let line = format!("solution: {x},{y} {}", dir.letter());
        assert!(filed.contains(&line), "{filed}");
        tap(&mut app, KeyCode::KeyO);
        app.update();
        assert!(app.world().resource::<EditorState>().solution.is_empty());
        assert_eq!(ghosts(&mut app), 0, "an edit takes them away");
        let _ = std::fs::remove_file(&path);
    }

    /// A check's way through is the level's own as the check left it: a
    /// press that changes nothing keeps it, Z back past a later edit
    /// brings it back, and it is no unsaved work.
    #[test]
    fn a_found_way_through_outlives_no_edits_and_comes_back_with_z() {
        let (mut app, _) = painting_app((0, 0));
        app.add_systems(Update, poll_solver.after(editor_commands));
        {
            let world = app.world_mut();
            let mut board = sand();
            board.set_tile(4, 2, TileKind::Castle(0));
            board.spawn_crab(1, 2, Direction::Down, Handedness::Left, CrabKind::Common);
            world.resource_mut::<Sim>().0 = board.clone();
            let mut state = world.resource_mut::<EditorState>();
            state.posts = 1;
            state.begin_history(&board);
        }
        let solution = |app: &App| app.world().resource::<EditorState>().solution.clone();
        tap(&mut app, KeyCode::KeyV);
        while app.world().resource::<EditorState>().solver.is_some() {
            app.update();
            std::thread::yield_now();
        }
        let found = solution(&app);
        assert!(!found.is_empty(), "the check found one");
        let filed = filed_text(
            app.world().resource::<EditorState>(),
            &app.world().resource::<Sim>().0,
        );
        app.world_mut().resource_mut::<EditorState>().filed = Some(filed);
        tap(&mut app, KeyCode::KeyX);
        assert_eq!(solution(&app), found, "the eraser on bare sand is no edit");
        {
            let world = app.world();
            let state = world.resource::<EditorState>();
            assert!(is_checked(state, &world.resource::<Sim>().0));
            assert!(
                !unsaved_work(state, &world.resource::<Sim>().0),
                "saved before the check"
            );
        }
        tap(&mut app, KeyCode::KeyR);
        assert!(solution(&app).is_empty(), "an edit lets it go");
        tap(&mut app, KeyCode::KeyZ);
        assert_eq!(solution(&app), found, "and Z brings it back");
    }

    /// An edit that lets a found way through go lets its check go too: a
    /// rock painted and erased by hand is the checked level again, but
    /// without the way through, and F2 saved it with no `solution:` and no
    /// check.
    #[test]
    fn an_edit_taken_back_by_hand_leaves_the_bench_unchecked() {
        let (mut app, _) = painting_app((0, 0));
        app.add_systems(Update, poll_solver.after(editor_commands));
        {
            let world = app.world_mut();
            let mut board = sand();
            board.set_tile(4, 2, TileKind::Castle(0));
            board.spawn_crab(1, 2, Direction::Down, Handedness::Left, CrabKind::Common);
            world.resource_mut::<Sim>().0 = board.clone();
            let mut state = world.resource_mut::<EditorState>();
            state.posts = 1;
            state.begin_history(&board);
        }
        tap(&mut app, KeyCode::KeyV);
        while app.world().resource::<EditorState>().solver.is_some() {
            app.update();
            std::thread::yield_now();
        }
        let checked = |app: &App| {
            let world = app.world();
            is_checked(world.resource::<EditorState>(), &world.resource::<Sim>().0)
        };
        assert!(!app.world().resource::<EditorState>().solution.is_empty());
        assert!(checked(&app));
        tap(&mut app, KeyCode::KeyR);
        tap(&mut app, KeyCode::KeyX);
        let state = app.world().resource::<EditorState>();
        assert!(state.solution.is_empty(), "the rock let it go");
        assert!(!checked(&app), "so a save checks again");
    }

    /// With the mirror on, a beach keeps fair: a castle will not go on the
    /// mirror's line (the status line says why), a hole there turns to a
    /// way its copy matches, a stroke across the line leaves its first
    /// tile and the copies it made as they were painted, and a resize that
    /// moves the middle half a tile turns the mirror off and says so.
    #[test]
    fn the_mirror_keeps_a_beach_fair() {
        let (mut app, cursor) = painting_app((4, 1));
        {
            let world = app.world_mut();
            world.resource_mut::<Sim>().0 = Board::new(9, 7, 3);
            let mut state = world.resource_mut::<EditorState>();
            state.kind = LevelKind::Arena;
            state.symmetry = Symmetry::LeftRight;
        }
        let tile = |app: &App, x, y| app.world().resource::<Sim>().0.tile_at(x, y);
        let said = |app: &App| app.world().resource::<EditorState>().feedback.clone();
        tap(&mut app, KeyCode::KeyC);
        assert_eq!(tile(&app, 4, 1), TileKind::Empty, "no castle on the line");
        assert_eq!(said(&app), crate::app::i18n::EN.ed_mirror_line);
        // And the readout does not promise the castle the key refused.
        let (_, next) = {
            let world = app.world();
            let state = world.resource::<EditorState>();
            palette::under_and_next(
                &world.resource::<Sim>().0,
                4,
                1,
                state,
                &crate::app::i18n::EN,
            )
        };
        assert_eq!(next, None, "nothing a press would change");
        tap(&mut app, KeyCode::KeyH);
        assert!(
            matches!(tile(&app, 4, 1), TileKind::Spawner(s) if s.dir == Direction::Down),
            "a hole on the line faces along it"
        );

        // A castle stroke from (3,3) over the line to (5,3), which is the
        // first tile's own copy: P1 stays P1, its copy P2.
        step_to(&mut app, cursor, (3, 3));
        hold(&mut app, KeyCode::KeyC);
        step_to(&mut app, cursor, (4, 3));
        step_to(&mut app, cursor, (5, 3));
        step_to(&mut app, cursor, (6, 3));
        assert_eq!(tile(&app, 3, 3), TileKind::Castle(0));
        assert_eq!(tile(&app, 5, 3), TileKind::Castle(1));
        assert_eq!(tile(&app, 6, 3), TileKind::Castle(0));
        assert_eq!(tile(&app, 2, 3), TileKind::Castle(1));
        assert_eq!(tile(&app, 4, 3), TileKind::Empty, "the line skipped");
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyC);
        step_to(&mut app, cursor, (6, 3));

        // 9 wide to 12: the middle moves half a tile.
        tap(&mut app, KeyCode::F5);
        let state = app.world().resource::<EditorState>();
        assert_eq!(state.symmetry, Symmetry::Off);
        assert!(
            state
                .feedback
                .starts_with(crate::app::i18n::EN.ed_mirror_moved)
        );
    }

    /// With the mirror on, a beach's crab, gull or log on the line is held
    /// to the line too: a log is refused, a crab or gull is set down
    /// facing along the line and T turns it only along it, the status line
    /// saying why; and T on a crab is judged by the crab, not by a hole
    /// under it that already faces across.
    #[test]
    fn the_mirror_keeps_creatures_and_logs_on_its_line_fair() {
        let (mut app, cursor) = painting_app((4, 1));
        {
            let world = app.world_mut();
            world.resource_mut::<Sim>().0 = Board::new(9, 7, 3);
            let mut state = world.resource_mut::<EditorState>();
            state.kind = LevelKind::Arena;
            state.symmetry = Symmetry::LeftRight;
        }
        let tile = |app: &App, x, y| app.world().resource::<Sim>().0.tile_at(x, y);
        let said = |app: &App| app.world().resource::<EditorState>().feedback.clone();
        let facing = |app: &App, x, y| brush::facing(&app.world().resource::<Sim>().0, x, y);
        let across = crate::app::i18n::EN.ed_mirror_across;
        tap(&mut app, KeyCode::KeyL);
        assert_eq!(tile(&app, 4, 1), TileKind::Empty, "no log on the line");
        assert_eq!(said(&app), across);

        step_to(&mut app, cursor, (4, 3));
        tap(&mut app, KeyCode::KeyB);
        assert_eq!(facing(&app, 4, 3), Some(Direction::Down), "along the line");
        assert_eq!(said(&app), across);
        tap(&mut app, KeyCode::KeyT);
        assert_eq!(facing(&app, 4, 3), Some(Direction::Up), "past Left");

        step_to(&mut app, cursor, (4, 5));
        tap(&mut app, KeyCode::KeyG);
        assert!(
            matches!(facing(&app, 4, 5), Some(Direction::Up | Direction::Down)),
            "{:?}",
            facing(&app, 4, 5)
        );

        // A hole facing across, put there with the mirror off, and a crab
        // on it: T turns the crab.
        {
            let world = app.world_mut();
            let mut sim = world.resource_mut::<Sim>();
            let hole = crate::sim::Spawner {
                dir: Direction::Right,
                period: 60,
            };
            sim.0.set_tile(4, 4, TileKind::Spawner(hole));
            sim.0
                .spawn_crab(4, 4, Direction::Down, Handedness::Left, CrabKind::Common);
        }
        assert_eq!(facing(&app, 4, 4), Some(Direction::Down));
        step_to(&mut app, cursor, (4, 4));
        tap(&mut app, KeyCode::KeyT);
        assert_eq!(facing(&app, 4, 4), Some(Direction::Up), "the crab turned");
    }

    /// The tables a pasted beach named come back with the castle they
    /// need, after an undo and a redo of its going.
    #[test]
    fn undo_and_redo_keep_a_pasted_beachs_tables() {
        let (mut app, _) = painting_app((5, 4));
        {
            let world = app.world_mut();
            let mut board = sand();
            for (seat, (x, y)) in [(0, 0), (5, 4), (5, 0), (0, 4)].into_iter().enumerate() {
                board.set_tile(x, y, TileKind::Castle(seat as u8));
            }
            world.resource_mut::<Sim>().0 = board.clone();
            let mut state = world.resource_mut::<EditorState>();
            state.kind = LevelKind::Arena;
            state.tables = vec![2, 4];
            state.begin_history(&board);
        }
        let tables = |app: &App| {
            let world = app.world();
            level_here(
                world.resource::<EditorState>(),
                &world.resource::<Sim>().0,
                "x",
            )
            .tables
        };
        assert_eq!(tables(&app), [2, 4]);
        app.world_mut()
            .resource_mut::<Sim>()
            .0
            .set_tile(0, 4, TileKind::Empty);
        {
            let world = app.world_mut();
            let mut board = world.resource::<Sim>().0.clone();
            world.resource_mut::<EditorState>().edited(&mut board);
            world.resource_mut::<Sim>().0 = board;
        }
        assert_eq!(tables(&app), [2]);
        tap(&mut app, KeyCode::KeyZ);
        assert_eq!(tables(&app), [2, 4]);
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::ShiftLeft);
            keys.press(KeyCode::KeyZ);
        }
        app.update();
        assert_eq!(tables(&app), [2], "redone: the castle is gone again");
        app.world_mut()
            .resource_mut::<Sim>()
            .0
            .set_tile(0, 4, TileKind::Castle(3));
        assert_eq!(tables(&app), [2, 4], "and back with it");
    }

    /// A stroke begun on a tile its brush had already made is its own
    /// step back, not the edit before it carried on; and a wall put up
    /// while a brush is held is a step of its own too, which ends the
    /// stroke.
    #[test]
    fn a_stroke_is_one_step_back_of_its_own() {
        let (mut app, cursor) = painting_app((1, 1));
        let world = app.world_mut();
        let board = world.resource::<Sim>().0.clone();
        world.resource_mut::<EditorState>().begin_history(&board);
        let tile = |app: &App, x, y| app.world().resource::<Sim>().0.tile_at(x, y);
        tap(&mut app, KeyCode::KeyR);
        assert_eq!(tile(&app, 1, 1), TileKind::Rock);
        hold(&mut app, KeyCode::KeyR);
        step_to(&mut app, cursor, (2, 1));
        step_to(&mut app, cursor, (3, 1));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyR);
        step_to(&mut app, cursor, (3, 1));
        tap(&mut app, KeyCode::KeyZ);
        assert_eq!(tile(&app, 2, 1), TileKind::Empty, "the drag undone");
        assert_eq!(tile(&app, 1, 1), TileKind::Rock, "the rock before it stays");

        hold(&mut app, KeyCode::KeyC);
        step_to(&mut app, cursor, (4, 1));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowRight);
        app.update();
        assert!(
            app.world()
                .resource::<Sim>()
                .0
                .wall_at(4, 1, Direction::Right)
        );
        tap(&mut app, KeyCode::KeyZ);
        assert!(
            !app.world()
                .resource::<Sim>()
                .0
                .wall_at(4, 1, Direction::Right)
        );
        assert_eq!(tile(&app, 4, 1), TileKind::Castle(0), "the stroke stays");
    }

    /// A wall put up while a brush is held does not end the painting: the
    /// drag goes on painting after it, as a step back of its own.
    #[test]
    fn a_held_brush_paints_on_past_a_wall() {
        let (mut app, cursor) = painting_app((1, 1));
        let world = app.world_mut();
        let board = world.resource::<Sim>().0.clone();
        world.resource_mut::<EditorState>().begin_history(&board);
        let tile = |app: &App, x, y| app.world().resource::<Sim>().0.tile_at(x, y);
        let wall = |app: &App| {
            app.world()
                .resource::<Sim>()
                .0
                .wall_at(2, 1, Direction::Right)
        };
        hold(&mut app, KeyCode::KeyR);
        step_to(&mut app, cursor, (2, 1));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowRight);
        app.update();
        assert!(wall(&app));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::ArrowRight);
        step_to(&mut app, cursor, (3, 1));
        step_to(&mut app, cursor, (4, 1));
        assert_eq!(tile(&app, 3, 1), TileKind::Rock, "still painting");
        assert_eq!(tile(&app, 4, 1), TileKind::Rock);
        tap(&mut app, KeyCode::KeyZ);
        assert_eq!(tile(&app, 3, 1), TileKind::Empty, "the drag after the wall");
        assert!(wall(&app), "is its own step back");
        assert_eq!(tile(&app, 2, 1), TileKind::Rock);
    }

    /// A brush letter the mirror refuses ends the stroke held before it:
    /// left going with the new brush, it went on painting with a brush the
    /// mirror had just refused, over the castles it crossed.
    #[test]
    fn a_refused_brush_letter_ends_the_stroke_before_it() {
        let (mut app, cursor) = painting_app((3, 3));
        {
            let world = app.world_mut();
            let mut board = Board::new(9, 7, 3);
            board.set_tile(6, 3, TileKind::Castle(0));
            board.set_tile(2, 3, TileKind::Castle(1));
            world.resource_mut::<Sim>().0 = board;
            let mut state = world.resource_mut::<EditorState>();
            state.kind = LevelKind::Arena;
            state.symmetry = Symmetry::LeftRight;
        }
        let tile = |app: &App, x, y| app.world().resource::<Sim>().0.tile_at(x, y);
        tap(&mut app, KeyCode::KeyR);
        hold(&mut app, KeyCode::Space);
        step_to(&mut app, cursor, (4, 3));
        assert_eq!(tile(&app, 4, 3), TileKind::Rock, "a rock on the line");
        // L on the line, Space still held: the mirror refuses the log.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyL);
        app.update();
        assert_eq!(tile(&app, 4, 3), TileKind::Rock, "refused");
        step_to(&mut app, cursor, (6, 3));
        assert_eq!(tile(&app, 6, 3), TileKind::Castle(0), "left alone");
        assert_eq!(tile(&app, 2, 3), TileKind::Castle(1));
    }

    /// With the mirror on, a beach's castle stroke and a wall land on
    /// every copy, each castle its copy's seat, and Z takes the whole
    /// mirrored stroke back in one press, as it would the stroke alone.
    #[test]
    fn a_mirrored_stroke_is_one_edit_on_every_copy() {
        let (mut app, cursor) = painting_app((0, 0));
        {
            let mut state = app.world_mut().resource_mut::<EditorState>();
            state.kind = LevelKind::Arena;
            state.symmetry = Symmetry::Both;
        }
        let begin = |app: &mut App| {
            let world = app.world_mut();
            let board = world.resource::<Sim>().0.clone();
            world.resource_mut::<EditorState>().begin_history(&board);
        };
        begin(&mut app);
        let tile = |app: &App, x, y| app.world().resource::<Sim>().0.tile_at(x, y);
        hold(&mut app, KeyCode::KeyC);
        step_to(&mut app, cursor, (1, 0));
        // The bench is 6x5.
        for (x, y, seat) in [
            (0, 0, 0),
            (1, 0, 0),
            (5, 0, 2),
            (4, 0, 2),
            (0, 4, 3),
            (5, 4, 1),
        ] {
            assert_eq!(tile(&app, x, y), TileKind::Castle(seat), "({x},{y})");
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyC);
        step_to(&mut app, cursor, (1, 1));
        tap(&mut app, KeyCode::ArrowRight);
        let walls = |app: &App| {
            let board = &app.world().resource::<Sim>().0;
            [
                board.wall_at(1, 1, Direction::Right),
                board.wall_at(4, 1, Direction::Left),
                board.wall_at(1, 3, Direction::Right),
                board.wall_at(4, 3, Direction::Left),
            ]
        };
        assert_eq!(walls(&app), [true; 4]);
        tap(&mut app, KeyCode::KeyZ);
        assert_eq!(walls(&app), [false; 4], "the wall and its copies in one");
        assert_eq!(tile(&app, 5, 4), TileKind::Castle(1));
        tap(&mut app, KeyCode::KeyZ);
        for (x, y) in [(0, 0), (1, 0), (5, 0), (4, 0), (0, 4), (5, 4)] {
            assert_eq!(tile(&app, x, y), TileKind::Empty, "({x},{y}) undone");
        }
    }
}
