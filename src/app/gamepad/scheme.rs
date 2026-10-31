//! What every pad control does, one table per face of the game.
//!
//! A face is a screen in one of its states: the lobby browsing, the lobby
//! hosting, the editor with Select held, a round being played. For each
//! there is one table of [`Bind`]s, each a pad control, what it does in
//! the game's words, and how it does it: a key the pad bridge presses for
//! it, or nothing pressed at all because the screen reads the pad itself.
//!
//! Three readers share each table and nothing else: the bridge, which
//! presses the keys ([`super::pad_menu_bridge`]); the systems that read a
//! pad directly, which ask the table which button means what
//! ([`pressed`]); and the controller prompt, which is written from it
//! ([`legend`]). A prompt cannot name a button the bridge does not press,
//! or miss one it does, because the two are the same rows.

use crate::app::campaign::WonDoor;
use crate::app::editor::Command;
use crate::app::i18n::{Tr, fill};
use crate::app::menu_ui::{NUMBER_KEYS, Nav};
use crate::app::play_input::AfterRound;
use crate::sim::Direction;
use bevy::prelude::*;

/// Which hands the player last reached with: the keyboard, or a pad and
/// whose labels it wears.
///
/// The prompts follow it, naming the buttons of whichever was touched
/// last, so a Steam Deck player sees A and B where a desk player sees
/// Enter and Esc, and a DualSense player sees ✕ and ○. One resource for
/// the whole game, since one person at a time reads a prompt line.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LastDevice {
    #[default]
    Keyboard,
    Pad(PadFamily),
}

impl LastDevice {
    /// A pad with Xbox's labels, the stock one the tests read prompts on.
    #[cfg(test)]
    pub const PAD: LastDevice = LastDevice::Pad(PadFamily::Xbox);

    /// Whether the hands are on a pad, of whatever family.
    pub fn is_pad(self) -> bool {
        matches!(self, LastDevice::Pad(_))
    }

    /// Whose button names a prompt uses: the pad's, and Xbox's at the
    /// keyboard, where no button is named.
    pub fn family(self) -> PadFamily {
        match self {
            LastDevice::Pad(family) => family,
            LastDevice::Keyboard => PadFamily::Xbox,
        }
    }
}

/// Whose labels a pad wears, which is what its buttons are called. The
/// game reads every pad by position (the bottom face button is the one
/// that plays, whatever is printed on it), so a family changes the names
/// in the prompts and nothing else. Told by the maker's USB vendor id:
/// every pad that is neither Sony's nor Nintendo's (Steam's own, the
/// Deck's, Xbox pads and their many lookalikes) wears Xbox's.
///
/// Carried by [`LastDevice`]: the family of the pad last reached with
/// ([`super::LastPad`]), whose hands read the prompt line.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PadFamily {
    #[default]
    Xbox,
    /// A DualShock and older: ✕○□△, L1/R1, Options and Share.
    PlayStation,
    /// A DualSense, whose Share button was renamed Create.
    DualSense,
    /// Nintendo's own, and pads in Switch mode: A and B swapped from where
    /// Xbox has them, and X and Y too.
    Nintendo,
}

impl PadFamily {
    /// Every family, in the order a line spelled for each is kept in.
    pub const ALL: [PadFamily; 4] = [
        PadFamily::Xbox,
        PadFamily::PlayStation,
        PadFamily::DualSense,
        PadFamily::Nintendo,
    ];

    /// The family of `pad`, by the ids it reported when it was plugged in.
    pub fn of(pad: &Gamepad) -> PadFamily {
        PadFamily::from_ids(pad.vendor_id(), pad.product_id())
    }

    /// The family a USB vendor and product id say: Sony's 0x054C,
    /// Nintendo's 0x057E, and the DualSense and DualSense Edge among
    /// Sony's by their product ids.
    pub fn from_ids(vendor: Option<u16>, product: Option<u16>) -> PadFamily {
        match (vendor, product) {
            (Some(0x054C), Some(0x0CE6 | 0x0DF2)) => PadFamily::DualSense,
            (Some(0x054C), _) => PadFamily::PlayStation,
            (Some(0x057E), _) => PadFamily::Nintendo,
            _ => PadFamily::Xbox,
        }
    }
}

/// One control on an Xbox-layout pad, which is how Steam presents the
/// Deck's own controls and nearly every pad besides. The face buttons
/// are by position (A the bottom one); [`Pad::name`] says what the pad
/// in hand has printed there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pad {
    A,
    B,
    X,
    Y,
    LB,
    RB,
    LT,
    RT,
    Start,
    Select,
    /// The right stick pressed in.
    RS,
    Up,
    Down,
    Left,
    Right,
    /// The d-pad or the left stick held, which the board cursor reads for
    /// itself (`gamepad::pad_move_cursor`).
    Move,
    /// The right stick pushed toward one side.
    Flick(Direction),
}

impl Pad {
    /// The pad button behind this control, for the ones that are buttons.
    pub fn button(self) -> Option<GamepadButton> {
        Some(match self {
            A => GamepadButton::South,
            B => GamepadButton::East,
            X => GamepadButton::West,
            Y => GamepadButton::North,
            LB => GamepadButton::LeftTrigger,
            RB => GamepadButton::RightTrigger,
            LT => GamepadButton::LeftTrigger2,
            RT => GamepadButton::RightTrigger2,
            Start => GamepadButton::Start,
            Select => GamepadButton::Select,
            RS => GamepadButton::RightThumb,
            Pad::Up => GamepadButton::DPadUp,
            Pad::Down => GamepadButton::DPadDown,
            Pad::Left => GamepadButton::DPadLeft,
            Pad::Right => GamepadButton::DPadRight,
            Pad::Move | Pad::Flick(_) => return None,
        })
    }

    /// What the prompt calls it on a pad of `family`: the one naming rule
    /// every prompt and legend goes through. The buttons go by the names
    /// printed on them, the same in every language; the d-pad and the
    /// stick are words.
    ///
    /// By position: the bottom face button is A on an Xbox pad, ✕ on a
    /// PlayStation and B on a Nintendo pad. Told "X: paste" with Xbox's
    /// letters, a DualSense player pressed ✕, the bottom button, and
    /// played the row instead; a Switch player told "A: play" pressed the
    /// right-hand button, which is Back. The symbols are DejaVu Sans
    /// Mono's, the face every line asks for first in every language.
    pub fn name(self, tr: &Tr, family: PadFamily) -> &'static str {
        use PadFamily::{DualSense, Nintendo, PlayStation, Xbox};
        match (self, family) {
            (Pad::Up | Pad::Down | Pad::Left | Pad::Right | Pad::Move, _) => tr.pad.dpad,
            (Pad::Flick(_), _) => tr.pad.right_stick,
            (A, Xbox) => "A",
            (B, Xbox) => "B",
            (X, Xbox) => "X",
            (Y, Xbox) => "Y",
            (LB, Xbox) => "LB",
            (RB, Xbox) => "RB",
            (LT, Xbox) => "LT",
            (RT, Xbox) => "RT",
            (Start, Xbox) => "Start",
            (Select, Xbox) => "Select",
            (RS, Xbox) => "RS",
            (A, PlayStation | DualSense) => "✕",
            (B, PlayStation | DualSense) => "○",
            (X, PlayStation | DualSense) => "□",
            (Y, PlayStation | DualSense) => "△",
            (LB, PlayStation | DualSense) => "L1",
            (RB, PlayStation | DualSense) => "R1",
            (LT, PlayStation | DualSense) => "L2",
            (RT, PlayStation | DualSense) => "R2",
            (Start, PlayStation | DualSense) => "Options",
            (Select, PlayStation) => "Share",
            (Select, DualSense) => "Create",
            (RS, PlayStation | DualSense) => "R3",
            (A, Nintendo) => "B",
            (B, Nintendo) => "A",
            (X, Nintendo) => "Y",
            (Y, Nintendo) => "X",
            (LB, Nintendo) => "L",
            (RB, Nintendo) => "R",
            (LT, Nintendo) => "ZL",
            (RT, Nintendo) => "ZR",
            (Start, Nintendo) => "+",
            (Select, Nintendo) => "−",
            (RS, Nintendo) => "RS",
        }
    }

    /// Whether the control is down on `pad`: the button held, or the
    /// right stick pushed past `deadzone` toward its side, along the axis
    /// it is pushed furthest on. [`Pad::Move`] is the cursor's, and never
    /// counts here.
    pub fn held(self, pad: &Gamepad, deadzone: f32) -> bool {
        if let Pad::Flick(dir) = self {
            return flick(pad, deadzone) == Some(dir);
        }
        // The cursor's Move has no button, and never counts.
        self.button().is_some_and(|button| pad.pressed(button))
    }
}

/// Which way the right stick is pushed, if past `deadzone`: the axis it
/// is pushed further along decides, so a push a little off true reads as
/// the side it is nearest.
pub fn flick(pad: &Gamepad, deadzone: f32) -> Option<Direction> {
    let x = pad.get(GamepadAxis::RightStickX).unwrap_or(0.0);
    let y = pad.get(GamepadAxis::RightStickY).unwrap_or(0.0);
    if x.abs().max(y.abs()) <= deadzone {
        return None;
    }
    // Stick Y is up-positive.
    Some(match x.abs() > y.abs() {
        true if x > 0.0 => Direction::Right,
        true => Direction::Left,
        false if y > 0.0 => Direction::Up,
        false => Direction::Down,
    })
}

/// How a control does what it does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    /// The bridge presses this key for it.
    Code(KeyCode),
    /// This key with Shift held: the way back through a list Tab walks
    /// forward.
    Shifted(KeyCode),
    /// The key whose cap says this letter on this keyboard, which is the
    /// key a mnemonic is read by (`KeyCaps::key_for`).
    Cap(char),
    /// The editor's wall toggle for this side, which follows P1's
    /// bindings (`editor::EditorKeys`).
    Wall(Direction),
    /// The key an editor command is on, which moves off its letter when
    /// P1's keys take it (`editor::EditorKeys::command`).
    Command(Command),
    /// The same key with Shift held: the command run the other way, as
    /// Shift+Z redoes what Z undid.
    ShiftedCommand(Command),
    /// The number key of the row the pad's pick is on ([`PadPick`]).
    Pick,
    /// Moves the pad's pick one row, and presses nothing.
    Step(Nav),
    /// Read off the pad by the screen's own system, never pressed as a
    /// key: in a round a pressed key would be read as a seat's.
    Direct,
    /// Held to show the face's second layer, which the face itself reads.
    Layer,
}

/// What a control does, in the words the prompt uses for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Does {
    Choose,
    ChooseAdjust,
    Confirm,
    /// On to the next step of match setup.
    Next,
    Play,
    Back,
    Menu,
    SaveBack,
    PasteRound,
    Mute,
    Rebind,
    Defaults,
    StartMatch,
    Join,
    /// Step 3's partner pick under a team mode.
    Partners,
    Rename,
    Coop,
    Watch,
    CopyCode,
    PasteCode,
    /// Deleting a kept round, asked twice ([`crate::app::menu_ui::SecondPress`]).
    Delete,
    Scroll,
    NotNow,
    GoOn,
    Cancel,
    NextPhrase,
    Copy,
    LanOpen,
    LanClose,
    Pick,
    AsBot,
    Host,
    ByAddress,
    PlayInstead,
    SetRound,
    Talk,
    AskToLeave,
    More,
    NeverMind,
    Move,
    /// Placing an arrow pointing this way.
    Place(Direction),
    Remove,
    Clear,
    Start,
    Run,
    PrevLevel,
    NextLevel,
    Hint,
    Reset,
    /// Opening the pause card over a round being played.
    Pause,
    /// Opening the pause card where it is the way to the menu: a puzzle's
    /// setup, a replay, a watched round. Its own word for the card, apart
    /// from [`Does::Menu`], which a results card's A also does and which
    /// must not open the card under it.
    OpenMenu,
    /// Stopping and starting a recording.
    PlayPause,
    NextStage,
    TryAgain,
    Stages,
    BackToMenu,
    Speed,
    Vote,
    PickWinner,
    Close,
    Lobby,
    NextRound,
    /// The online host's map page for the next round of a series.
    PickMap,
    /// Playing the round just over again, or its whole series.
    Rematch(crate::app::rematch::Rematch),
    /// Putting the round just over on the replay shelf.
    KeepReplay,
    Paint,
    PrevBrush,
    NextBrush,
    Walls,
    /// The settings card's chosen row turned down and up.
    SettingDown,
    SettingUp,
    /// The creature or hole under the cursor turned.
    Turn,
    /// The last edit taken back, and made again.
    Undo,
    Redo,
    /// Opening the level chosen on the editor's list.
    Open,
    /// A level of the player's own opened in the editor.
    Edit,
    Check,
    Test,
    HoldMore,
    Save,
    Name,
    ShareLevel,
    LoadLevel,
    Size,
    Kind,
    /// The settings card's choice moved to the row before and after.
    PrevSetting,
    NextSetting,
    EndTest,
    /// The card of every key and button the editor answers to.
    Help,
    /// Leaving the game: Start raising the main menu's quit card, and A
    /// on the card answering it.
    Quit,
}

impl Does {
    /// The words the prompt puts after the button. Controls that share
    /// words share a slot in the prompt: "LT/RT: level".
    pub fn label(self, tr: &Tr) -> &'static str {
        let p = &tr.pad;
        match self {
            Does::Choose => p.choose,
            Does::ChooseAdjust => p.choose_adjust,
            Does::Confirm => p.confirm,
            Does::Next => p.next,
            Does::Play => p.play,
            Does::Back => p.back,
            Does::Menu | Does::OpenMenu => p.menu,
            Does::SaveBack => p.save_back,
            Does::PasteRound => p.paste_round,
            Does::Mute => p.mute,
            Does::Rebind => p.rebind,
            Does::Defaults => p.defaults,
            Does::StartMatch => p.start_match,
            Does::Join => p.join,
            Does::Partners => p.partners,
            Does::Rename => p.rename,
            Does::Coop => p.coop,
            Does::Watch => p.watch,
            Does::CopyCode | Does::ShareLevel => p.copy_code,
            Does::PasteCode => p.paste_code,
            Does::LoadLevel => p.my_levels,
            Does::Delete => p.delete,
            Does::Scroll => p.scroll,
            Does::NotNow => p.not_now,
            Does::GoOn => p.go_on,
            Does::Cancel => p.cancel,
            Does::NextPhrase => p.next_phrase,
            Does::Copy => p.copy,
            Does::LanOpen => p.lan_open,
            Does::LanClose => p.lan_close,
            Does::Pick => p.pick,
            Does::AsBot => p.as_bot,
            Does::Host => p.host,
            Does::ByAddress => p.by_address,
            Does::PlayInstead => p.play_instead,
            Does::SetRound => p.set_round,
            Does::Talk => p.talk,
            Does::AskToLeave => p.ask_to_leave,
            Does::More => p.more,
            Does::NeverMind => p.never_mind,
            Does::Move => p.moves,
            Does::Place(_) => p.place,
            Does::Remove => p.remove,
            Does::Clear => p.clear,
            Does::Start => p.start,
            Does::Run => p.run,
            Does::PrevLevel | Does::NextLevel => p.level,
            Does::Hint => p.hint,
            Does::Reset => p.reset,
            Does::Pause | Does::PlayPause => p.pause,
            Does::NextStage => p.next_level,
            Does::TryAgain => p.try_again,
            Does::Stages => p.stages,
            Does::BackToMenu => p.back_to_menu,
            Does::Speed => p.speed,
            Does::Vote => p.vote,
            Does::PickWinner => p.pick_winner,
            Does::Close => p.close,
            Does::Lobby => p.lobby,
            Does::NextRound => p.next_round,
            Does::PickMap => p.pick_map,
            Does::Rematch(crate::app::rematch::Rematch::Round) => p.rematch,
            Does::Rematch(crate::app::rematch::Rematch::Series) => p.rematch_series,
            Does::Rematch(crate::app::rematch::Rematch::SeriesAsked) => p.rematch_series_confirm,
            Does::KeepReplay => p.keep_replay,
            Does::Paint => p.paint,
            Does::PrevBrush | Does::NextBrush => p.brush,
            Does::Walls => p.walls,
            Does::SettingDown | Does::SettingUp => p.setting,
            Does::Turn => p.turn,
            Does::Undo | Does::Redo => p.undo,
            Does::Open => p.open,
            Does::Edit => p.edit,
            Does::Check => p.check,
            Does::Test => p.test,
            Does::HoldMore => p.hold_more,
            Does::Save => p.save,
            Does::Name => p.name,
            Does::Size => p.size,
            Does::Kind => p.kind,
            Does::PrevSetting | Does::NextSetting => p.pick_setting,
            Does::EndTest => p.end_test,
            Does::Help => p.help,
            Does::Quit => p.quit,
        }
    }
}

/// One row of a face's table: a control, how it acts, and what it does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bind {
    pub pad: Pad,
    pub key: Key,
    pub does: Does,
}

const fn bind(pad: Pad, key: Key, does: Does) -> Bind {
    Bind { pad, key, does }
}

/// A list the pad picks from with a cursor of its own, where the keyboard
/// presses a number: the rows and the number key each one answers to.
///
/// Numbers rather than a cursor suit a keyboard, one press for any row,
/// but a pad has no number keys, and mapping buttons to numbers leaves
/// nobody able to read the prompt. So the pad walks the list and A
/// presses the number of the row it stands on.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Picks {
    /// Which list this is, so the pick starts again at the top when
    /// another list opens.
    pub list: PickList,
    /// Each row's number key (1 to 9) and what the row is called.
    pub rows: Vec<(u8, String)>,
}

/// The lists a pad picks from: see [`Picks`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickList {
    /// The host's "ask who to leave" (`lobby::kick_prompt`).
    Kick,
    /// A spectator's card of tide events to call.
    Vote,
    /// A spectator's card of seats to call the winner from.
    Winner,
    /// The host's seats to pick partners for (`lobby::partners`).
    Partners,
}

/// Where the pad's pick stands on the list a face offers ([`Picks`]).
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PadPick {
    list: Option<PickList>,
    at: usize,
}

impl PadPick {
    /// The row the pick stands on in `picks`: the first when the list is
    /// new, and never past the end of one that has shrunk.
    pub fn row(&self, picks: &Picks) -> usize {
        match self.list == Some(picks.list) {
            true => self.at.min(picks.rows.len().saturating_sub(1)),
            false => 0,
        }
    }

    /// Stand on the row of `picks` whose number is `number`, if it has
    /// one: where a row moved to once a pick reordered the list.
    pub fn stand_on(&mut self, picks: &Picks, number: u8) {
        if let Some(at) = picks.rows.iter().position(|(row, _)| *row == number) {
            self.list = Some(picks.list);
            self.at = at;
        }
    }

    /// One row up or down `picks`, wrapping at the ends as every list in
    /// the game does.
    pub fn step(&mut self, picks: &Picks, nav: Nav) {
        let at = self.row(picks);
        let len = picks.rows.len().max(1);
        self.list = Some(picks.list);
        self.at = crate::app::menu_ui::step(nav, at, len);
    }
}

/// How a text box ends, which is what its prompt offers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextBox {
    /// A line of lobby chat: Y walks the quick phrases, B drops it.
    Chat,
    /// A box Esc throws away.
    Cancels,
    /// A box every way out of keeps, so only confirming is offered.
    Keeps,
}

/// A screen in one of its states, as far as the pad cares: which table of
/// [`Bind`]s is in force. Worked out once a frame from the game's state
/// by [`super::face::read_face`], the one place that decides it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum Face {
    #[default]
    Menu,
    /// The main menu's quit card is up ([`crate::app::menu_scene::QuitCard`]).
    Quitting,
    /// The settings screen: `dial` whether the cursor is on a dial, where
    /// A is done here as B is, rather than on a door A works (the key
    /// bindings, the reset).
    Settings {
        dial: bool,
    },
    Controls,
    /// One of the three steps of match setup. `join`: whether the Start
    /// of the pad last reached with would seat somebody
    /// ([`super::PadSeats::start_joins_here`]), which is when step 3's
    /// legend offers it; else that Start starts the match there. Reading
    /// a press of it (`pad_claim_seats`) asks with it on. `partners`:
    /// whether the table plays a team mode, where step 3 picks partners
    /// ([`crate::app::match_setup::setup_teams`]).
    MatchSetup {
        step: crate::app::match_setup::Step,
        join: bool,
        partners: bool,
    },
    /// The stage list: `custom` whether the stage chosen is one of the
    /// player's own, which can be edited and its code copied.
    StageSelect {
        custom: bool,
    },
    Replays,
    Achievements,
    Language,
    NewVersion,
    /// The pause between rounds of a series, and whether X has the round
    /// just played to keep ([`crate::app::replays::RoundToKeep::offer`]).
    Interlude {
        keep: crate::app::replays::KeepOffer,
    },
    /// A bot's doorway card, over match setup or the lobby. `lan`: the
    /// card is showing the LAN's strings, which Y takes back.
    Door {
        lan: bool,
    },
    /// A text box has the keyboard.
    Typing(TextBox),
    /// The lobby's beach list: `hosts` whether there are any, `watching`
    /// whether W is armed.
    Browse {
        hosts: bool,
        watching: bool,
    },
    /// The lobby, hosting. `partners`: whether its table plays a team mode,
    /// where the host picks partners ([`crate::app::lobby::partners_open`]).
    Hosting {
        partners: bool,
    },
    /// The host's map page over the lobby, once it has started the match:
    /// a card picked starts it, B goes back to the lobby card. `pick`
    /// whether the cursor is on a card A picks
    /// ([`crate::app::match_setup::card_to_pick`]).
    HostMap {
        pick: bool,
    },
    /// The host's map page between the rounds of a series: a card picked
    /// calls the next round on it, B goes back to the results card.
    NextMap {
        pick: bool,
    },
    /// The host picking whom to ask to leave; `pages` whether there is
    /// more than one page of them to turn.
    Kicking {
        picks: Picks,
        pages: bool,
    },
    /// The lobby, at somebody else's beach.
    AtBeach,
    /// A puzzle being set up: `posts` whether the level hands out any to
    /// place, `hint` whether a hint is on offer.
    PuzzleSetup {
        posts: bool,
        hint: bool,
    },
    PuzzleRunning,
    /// A puzzle's won card: `door` where its A goes.
    PuzzleWon {
        door: crate::app::campaign::WonDoor,
    },
    PuzzleLost {
        hint: bool,
    },
    /// The pause card is up.
    Paused,
    /// A versus round being played by a seat here.
    Round,
    /// A recording playing.
    Replay,
    /// Someone else's round, watched: `vote` whether a tide event can be
    /// called, `pick` whether the winner can.
    Watching {
        vote: bool,
        pick: bool,
    },
    /// A spectator's card of calls is up.
    Card(Picks),
    /// A versus round's results: where A goes, whether this player is a
    /// spectator, who can talk here, what Y plays again at a table set up
    /// here ([`crate::app::rematch::rematch`]), and whether X has a round
    /// to keep ([`crate::app::replays::RoundToKeep::offer`]).
    Results {
        door: AfterRound,
        talk: bool,
        rematch: Option<crate::app::rematch::Rematch>,
        keep: crate::app::replays::KeepOffer,
    },
    /// The editor: `layer` whether Select is held for its second layer.
    Editor {
        layer: bool,
    },
    /// The editor's "My levels" list is up.
    LevelList,
    /// A card of words over the editor's bench: its keys, or the tips.
    EditorCard,
    /// A level code pasted on the main menu, on its card.
    LevelCard,
    /// The editor's playtest, where it stands.
    Playtest(crate::app::editor::TestPhase),
}

use Pad::{A, B, LB, LT, RB, RS, RT, Select, Start, X, Y};

/// The letter whose key picks partners on step 3, which no seat can be
/// bound to ([`crate::app::binds::GLOBAL_LETTERS`]), so no player's key
/// is ever also the pick.
pub const PARTNER_CAP: char = 'P';

/// X keeping the round just played, while there is one to keep: on every
/// results card, a spectator's included, and on the interlude after it.
/// Once it is kept the line says so instead.
fn keep_bind(keep: crate::app::replays::KeepOffer) -> Option<Bind> {
    match keep {
        crate::app::replays::KeepOffer::Offered => Some(bind(
            X,
            Key::Cap(crate::app::replays::KEEP_CAP),
            Does::KeepReplay,
        )),
        crate::app::replays::KeepOffer::Nothing | crate::app::replays::KeepOffer::Kept => None,
    }
}

/// The d-pad walking a vertical list as W and S.
const UP_DOWN: [Bind; 2] = [
    bind(Pad::Up, Key::Code(KeyCode::KeyW), Does::Choose),
    bind(Pad::Down, Key::Code(KeyCode::KeyS), Does::Choose),
];

/// The d-pad as W, S, A and D, for a list of dials.
const DIALS: [Bind; 4] = [
    bind(Pad::Up, Key::Code(KeyCode::KeyW), Does::ChooseAdjust),
    bind(Pad::Down, Key::Code(KeyCode::KeyS), Does::ChooseAdjust),
    bind(Pad::Left, Key::Code(KeyCode::KeyA), Does::ChooseAdjust),
    bind(Pad::Right, Key::Code(KeyCode::KeyD), Does::ChooseAdjust),
];

/// The four placements, on the face buttons as a diamond (spec §8.1): the
/// top button places Up, the bottom Down, the left Left, the right Right.
pub const PLACES: [Bind; 4] = [
    bind(A, Key::Direct, Does::Place(Direction::Down)),
    bind(B, Key::Direct, Does::Place(Direction::Right)),
    bind(X, Key::Direct, Does::Place(Direction::Left)),
    bind(Y, Key::Direct, Does::Place(Direction::Up)),
];

/// The cursor, which the pad moves itself on every board.
const MOVE: Bind = bind(Pad::Move, Key::Direct, Does::Move);

/// A board's own controls, read off the pad and never pressed as keys:
/// move, the four placements, LB removes, RB clears.
const BOARD: [Bind; 7] = [
    MOVE,
    PLACES[0],
    PLACES[1],
    PLACES[2],
    PLACES[3],
    bind(LB, Key::Direct, Does::Remove),
    bind(RB, Key::Direct, Does::Clear),
];

/// The table in force on `face`.
/// A host's map page: the d-pad walks the cards, A does what a pick does
/// there when the cursor is on a card it picks (`pick`), and B goes back.
/// On a greyed card, or the card saying how to make a map, Enter does
/// nothing, so A is not offered.
fn map_page_binds(pick: bool, picked: Does) -> Vec<Bind> {
    let mut out: Vec<Bind> = DIALS
        .map(|row| Bind {
            does: Does::Choose,
            ..row
        })
        .to_vec();
    if pick {
        out.push(bind(A, Key::Code(KeyCode::Enter), picked));
    }
    out.push(bind(B, Key::Code(KeyCode::Escape), Does::Back));
    out
}

pub fn binds(face: &Face) -> Vec<Bind> {
    let code = Key::Code;
    let esc = code(KeyCode::Escape);
    let enter = code(KeyCode::Enter);
    let mut out = Vec::new();
    match face {
        // B is not Escape here: the menu's Escape leaves the game, and a
        // player reaching for "back" must not close it. Start asks first,
        // on a card of its own (`menu_scene::menu_input`).
        Face::Menu => {
            out.extend(UP_DOWN);
            out.extend([
                bind(A, enter, Does::Play),
                bind(X, Key::Cap('V'), Does::PasteRound),
                bind(Y, Key::Cap('M'), Does::Mute),
                bind(Start, Key::Direct, Does::Quit),
            ]);
        }
        // The quit card reads the pad itself: A leaves the game, and B or
        // Start again, the button that raised it, puts it away.
        Face::Quitting => out.extend([
            bind(A, Key::Direct, Does::Quit),
            bind(B, Key::Direct, Does::Back),
            bind(Start, Key::Direct, Does::Back),
        ]),
        Face::Settings { dial } => {
            out.extend(DIALS);
            let a = if *dial { Does::SaveBack } else { Does::Confirm };
            out.extend([bind(A, enter, a), bind(B, esc, Does::SaveBack)]);
        }
        Face::Controls => {
            out.extend(DIALS);
            out.extend([
                bind(A, enter, Does::Rebind),
                bind(X, code(KeyCode::Backspace), Does::Defaults),
                bind(B, esc, Does::Back),
            ]);
        }
        // Steps 1 and 2: nobody joins there, so A goes on, as Enter does.
        // A pad's Start is still heard on them (`pad_claim_seats` reads
        // step 3's table), and the friend is seated when step 3 opens.
        Face::MatchSetup {
            step: crate::app::match_setup::Step::Game,
            ..
        } => {
            out.extend(DIALS);
            out.extend([bind(A, enter, Does::Next), bind(B, esc, Does::Back)]);
        }
        Face::MatchSetup {
            step: crate::app::match_setup::Step::Map,
            ..
        } => {
            out.extend(DIALS.map(|row| Bind {
                does: Does::Choose,
                ..row
            }));
            out.extend([bind(A, enter, Does::Next), bind(B, esc, Does::Back)]);
        }
        // Step 3: Start is how a friend sits down (`pad_claim_seats`), and
        // on a pad already at the table it starts the match, as A does,
        // as on every other screen. Start first when it starts, so the
        // button names it.
        Face::MatchSetup {
            step: crate::app::match_setup::Step::Players,
            join,
            partners,
        } => {
            out.extend(DIALS);
            if !*join {
                out.push(bind(Start, Key::Direct, Does::StartMatch));
            }
            out.extend([
                bind(A, enter, Does::StartMatch),
                bind(X, code(KeyCode::Tab), Does::Rename),
            ]);
            if *partners {
                out.push(bind(Y, Key::Cap(PARTNER_CAP), Does::Partners));
            }
            if *join {
                out.push(bind(Start, Key::Direct, Does::Join));
            }
            out.push(bind(B, esc, Does::Back));
        }
        Face::StageSelect { custom } => {
            out.extend(DIALS.map(|row| Bind {
                does: Does::Choose,
                ..row
            }));
            out.push(bind(A, enter, Does::Play));
            if *custom {
                out.extend([
                    bind(X, Key::Cap('E'), Does::Edit),
                    bind(Select, code(KeyCode::F3), Does::CopyCode),
                ]);
            }
            out.extend([bind(Y, Key::Cap('C'), Does::Coop), bind(B, esc, Does::Menu)]);
        }
        // Delete is Select: the four face buttons are taken, and moving
        // copy or paste off X and Y would unteach the main menu's X. Out
        // of the thumb's way from A is right for the one press here that
        // cannot be undone, which asks twice besides.
        Face::Replays => {
            out.extend(UP_DOWN);
            out.extend([
                bind(A, enter, Does::Watch),
                bind(X, Key::Cap('C'), Does::CopyCode),
                bind(Y, Key::Cap('V'), Does::PasteCode),
                bind(Select, Key::Cap('D'), Does::Delete),
                bind(B, esc, Does::Menu),
            ]);
        }
        Face::Achievements => {
            out.extend(UP_DOWN.map(|row| Bind {
                does: Does::Scroll,
                ..row
            }));
            out.push(bind(B, esc, Does::Menu));
        }
        // No way back: there is nowhere behind the first screen anybody
        // sees (`language::language_input`).
        Face::Language => {
            out.extend(UP_DOWN);
            out.push(bind(A, enter, Does::Confirm));
        }
        Face::NewVersion => {
            out.extend(UP_DOWN);
            out.extend([bind(A, enter, Does::Confirm), bind(B, esc, Does::NotNow)]);
        }
        Face::Interlude { keep } => {
            out.push(bind(A, enter, Does::GoOn));
            out.extend(keep_bind(*keep));
        }
        Face::Door { lan } => out.extend([
            bind(X, Key::Cap('C'), Does::Copy),
            bind(
                Y,
                Key::Cap('L'),
                match lan {
                    true => Does::LanClose,
                    false => Does::LanOpen,
                },
            ),
            bind(B, esc, Does::Back),
        ]),
        Face::Typing(ends) => {
            out.push(bind(A, enter, Does::Confirm));
            match ends {
                // The lobby reads North itself, on an open line and a
                // closed one alike (`lobby::lobby_input`).
                TextBox::Chat => out.extend([
                    bind(Y, Key::Direct, Does::NextPhrase),
                    bind(B, esc, Does::Cancel),
                ]),
                TextBox::Cancels => out.push(bind(B, esc, Does::Cancel)),
                TextBox::Keeps => {}
            }
        }
        // Arrows, not W and S: W is the watch toggle here.
        Face::Browse { hosts, watching } => {
            if *hosts {
                out.extend([
                    bind(Pad::Up, code(KeyCode::ArrowUp), Does::Pick),
                    bind(Pad::Down, code(KeyCode::ArrowDown), Does::Pick),
                ]);
            }
            match watching {
                // A watches the beach under the cursor, so only with one
                // on the list; Select dials one, as J does, either way.
                true => {
                    if *hosts {
                        out.push(bind(A, enter, Does::Watch));
                    }
                    out.extend([
                        bind(Select, Key::Cap('J'), Does::ByAddress),
                        bind(RB, Key::Cap('W'), Does::PlayInstead),
                    ]);
                }
                false => {
                    if *hosts {
                        out.extend([
                            bind(A, enter, Does::Join),
                            bind(X, Key::Cap('B'), Does::AsBot),
                        ]);
                    }
                    out.extend([
                        bind(Y, Key::Cap('H'), Does::Host),
                        bind(Select, Key::Cap('J'), Does::ByAddress),
                        bind(RB, Key::Cap('W'), Does::Watch),
                    ]);
                }
            }
            out.push(bind(B, esc, Does::Menu));
        }
        Face::Hosting { partners } => {
            out.extend([
                bind(Pad::Up, code(KeyCode::ArrowUp), Does::SetRound),
                bind(Pad::Down, code(KeyCode::ArrowDown), Does::SetRound),
                bind(Pad::Left, code(KeyCode::ArrowLeft), Does::SetRound),
                bind(Pad::Right, code(KeyCode::ArrowRight), Does::SetRound),
                bind(A, enter, Does::StartMatch),
                bind(Y, Key::Direct, Does::Talk),
                bind(X, Key::Cap('K'), Does::AskToLeave),
            ]);
            if *partners {
                out.push(bind(Select, Key::Cap(PARTNER_CAP), Does::Partners));
            }
            out.push(bind(B, esc, Does::Menu));
        }
        Face::HostMap { pick } => out.extend(map_page_binds(*pick, Does::StartMatch)),
        Face::NextMap { pick } => out.extend(map_page_binds(*pick, Does::NextRound)),
        Face::Kicking { pages, .. } => {
            out.extend([
                bind(Pad::Up, Key::Step(Nav::Up), Does::Choose),
                bind(Pad::Down, Key::Step(Nav::Down), Does::Choose),
                bind(A, Key::Pick, Does::AskToLeave),
            ]);
            if *pages {
                out.push(bind(Select, code(KeyCode::Tab), Does::More));
            }
            out.push(bind(B, esc, Does::NeverMind));
        }
        Face::AtBeach => out.extend([bind(Y, Key::Direct, Does::Talk), bind(B, esc, Does::Menu)]),
        // Start begins the run here, as Enter does, so the pause card is
        // Select's.
        Face::PuzzleSetup { posts, hint } => {
            match posts {
                true => out.extend(BOARD),
                false => out.push(MOVE),
            }
            out.push(bind(
                Start,
                Key::Direct,
                match posts {
                    true => Does::Start,
                    false => Does::Run,
                },
            ));
            out.extend([
                bind(LT, Key::Direct, Does::PrevLevel),
                bind(RT, Key::Direct, Does::NextLevel),
            ]);
            if *hint {
                out.push(bind(RS, Key::Direct, Does::Hint));
            }
            out.push(bind(Select, Key::Direct, Does::OpenMenu));
        }
        Face::PuzzleRunning => out.extend([
            bind(Y, Key::Direct, Does::Reset),
            bind(Start, Key::Direct, Does::Pause),
        ]),
        // The results cards over a board never press the d-pad's keys:
        // nothing on them reads W/A/S/D, but the board cursor does, as
        // P1's keys.
        Face::PuzzleWon { door } => out.extend([
            match door {
                WonDoor::Menu => bind(A, enter, Does::BackToMenu),
                WonDoor::Stages => bind(A, enter, Does::Stages),
                WonDoor::Next => bind(A, enter, Does::NextStage),
            },
            bind(Y, Key::Cap('R'), Does::Reset),
            bind(B, esc, Does::Stages),
        ]),
        Face::PuzzleLost { hint } => {
            out.push(bind(A, enter, Does::TryAgain));
            if *hint {
                out.push(bind(RS, Key::Direct, Does::Hint));
            }
            out.push(bind(B, esc, Does::Stages));
        }
        // The card reads the pad itself (`pause::pause_input`): it is up
        // over a round, where a pressed key is a seat's.
        Face::Paused => out.extend([
            bind(Pad::Up, Key::Direct, Does::Choose),
            bind(Pad::Down, Key::Direct, Does::Choose),
            bind(A, Key::Direct, Does::Confirm),
            bind(Start, Key::Direct, Does::GoOn),
            bind(B, Key::Direct, Does::GoOn),
        ]),
        Face::Round => {
            out.extend(BOARD);
            out.push(bind(Start, Key::Direct, Does::Pause));
        }
        Face::Replay => out.extend([
            bind(A, Key::Direct, Does::PlayPause),
            bind(X, Key::Direct, Does::Speed),
            bind(Start, Key::Direct, Does::OpenMenu),
        ]),
        // A spectator holds no seat, so nothing here can steer one: the
        // keys are the spectator's own (`spectators`).
        Face::Watching { vote, pick } => {
            out.push(bind(Y, Key::Cap('T'), Does::Talk));
            if *vote {
                out.push(bind(X, Key::Cap('E'), Does::Vote));
            }
            if *pick {
                out.push(bind(RB, Key::Cap('P'), Does::PickWinner));
            }
            out.push(bind(Start, Key::Direct, Does::OpenMenu));
        }
        Face::Card(picks) => out.extend([
            bind(Pad::Up, Key::Step(Nav::Up), Does::Choose),
            bind(Pad::Down, Key::Step(Nav::Down), Does::Choose),
            bind(
                A,
                Key::Pick,
                match picks.list {
                    PickList::Winner => Does::PickWinner,
                    PickList::Vote | PickList::Kick => Does::Vote,
                    PickList::Partners => Does::Partners,
                },
            ),
            bind(B, esc, Does::Close),
        ]),
        Face::Results {
            door,
            talk,
            rematch,
            keep,
        } => {
            out.push(bind(
                A,
                enter,
                match door {
                    AfterRound::NextRound => Does::NextRound,
                    AfterRound::NextMap => Does::PickMap,
                    AfterRound::Lobby => Does::Lobby,
                    AfterRound::Menu => Does::Menu,
                },
            ));
            // A spectator is at somebody else's table, which has no
            // rematch here: Y is the one or the other.
            if *talk {
                out.push(bind(Y, Key::Cap('T'), Does::Talk));
            } else if let Some(again) = rematch {
                out.push(bind(Y, Key::Cap('R'), Does::Rematch(*again)));
            }
            out.extend(keep_bind(*keep));
        }
        // The cursor walks on the pad by itself, so the d-pad is never a
        // key here: pressed as W/A/S/D it would step twice.
        Face::Editor { layer: false } => out.extend([
            MOVE,
            bind(
                Pad::Flick(Direction::Up),
                Key::Wall(Direction::Up),
                Does::Walls,
            ),
            bind(
                Pad::Flick(Direction::Down),
                Key::Wall(Direction::Down),
                Does::Walls,
            ),
            bind(
                Pad::Flick(Direction::Left),
                Key::Wall(Direction::Left),
                Does::Walls,
            ),
            bind(
                Pad::Flick(Direction::Right),
                Key::Wall(Direction::Right),
                Does::Walls,
            ),
            bind(A, code(KeyCode::Space), Does::Paint),
            bind(LB, Key::Shifted(KeyCode::Tab), Does::PrevBrush),
            bind(RB, code(KeyCode::Tab), Does::NextBrush),
            bind(LT, code(KeyCode::Minus), Does::SettingDown),
            bind(RT, code(KeyCode::Equal), Does::SettingUp),
            bind(Y, Key::Command(Command::Turn), Does::Turn),
            bind(X, Key::Command(Command::Check), Does::Check),
            bind(Start, enter, Does::Test),
            bind(B, esc, Does::Menu),
            bind(Select, Key::Layer, Does::HoldMore),
        ]),
        // The right stick is the walls' on the base layer; held under
        // Select it walks the history, left back and right forward, and
        // the beach's sizes, up a size and down one (F5 and Shift+F5).
        Face::Editor { layer: true } => out.extend([
            bind(A, code(KeyCode::F2), Does::Save),
            bind(Y, code(KeyCode::F1), Does::Name),
            bind(X, code(KeyCode::F3), Does::ShareLevel),
            bind(B, code(KeyCode::F4), Does::LoadLevel),
            bind(RB, code(KeyCode::F6), Does::Kind),
            bind(
                LT,
                Key::Shifted(crate::app::editor::NEXT_SETTING),
                Does::PrevSetting,
            ),
            bind(
                RT,
                code(crate::app::editor::NEXT_SETTING),
                Does::NextSetting,
            ),
            bind(
                Pad::Flick(Direction::Left),
                Key::Command(Command::Undo),
                Does::Undo,
            ),
            bind(
                Pad::Flick(Direction::Right),
                Key::ShiftedCommand(Command::Undo),
                Does::Redo,
            ),
            bind(Pad::Flick(Direction::Up), code(KeyCode::F5), Does::Size),
            bind(
                Pad::Flick(Direction::Down),
                Key::Shifted(KeyCode::F5),
                Does::Size,
            ),
            bind(
                Start,
                Key::Shifted(crate::app::editor::HELP_KEY),
                Does::Help,
            ),
        ]),
        // Any key puts a card away (`editor::editor_card_input`).
        Face::EditorCard => out.extend([bind(A, enter, Does::Close), bind(B, esc, Does::Close)]),
        // The keys the card's own line names: Enter, E, S and Esc.
        Face::LevelCard => out.extend([
            bind(A, enter, Does::Play),
            bind(X, Key::Cap('E'), Does::Edit),
            bind(Y, Key::Cap('S'), Does::Save),
            bind(B, esc, Does::Back),
        ]),
        // As the replay library's list: Select deletes, out of the thumb's
        // way from A for the one press that cannot be undone.
        Face::LevelList => {
            out.extend(UP_DOWN);
            out.extend([
                bind(A, enter, Does::Open),
                bind(X, Key::Cap('C'), Does::CopyCode),
                bind(Select, Key::Cap('D'), Does::Delete),
                bind(B, esc, Does::Close),
            ]);
        }
        // Select ends the test in every phase, Start being a stage's run
        // as it is in a stage's setup; Y retries a stage as on its cards,
        // and RB restarts a beach, whose face buttons place.
        Face::Playtest(phase) => {
            use crate::app::editor::TestPhase;
            match phase {
                TestPhase::Setup => {
                    out.extend(BOARD[..6].iter().copied());
                    out.push(bind(Start, Key::Direct, Does::Start));
                }
                TestPhase::Running | TestPhase::Won | TestPhase::Lost => {
                    out.push(bind(Y, Key::Direct, Does::Reset));
                }
                TestPhase::Match => {
                    out.extend(BOARD[..6].iter().copied());
                    out.push(bind(RB, Key::Direct, Does::Reset));
                }
                TestPhase::MatchOver => out.push(bind(RB, Key::Direct, Does::Reset)),
            }
            out.push(bind(Select, Key::Direct, Does::EndTest));
        }
    }
    out
}

/// The face's list to pick from, if it has one.
pub fn picks(face: &Face) -> Option<&Picks> {
    if let Face::Kicking { picks, .. } | Face::Card(picks) = face {
        return Some(picks);
    }
    None
}

/// Whether any pad pressed the control that does `does` on `face` this
/// frame: how a system that reads the pad itself asks, so its button is
/// the one the prompt names.
pub fn pressed<'a>(face: &Face, does: Does, pads: impl IntoIterator<Item = &'a Gamepad>) -> bool {
    let buttons: Vec<GamepadButton> = binds(face)
        .iter()
        .filter(|row| row.does == does)
        .filter_map(|row| row.pad.button())
        .collect();
    pads.into_iter()
        .any(|pad| buttons.iter().any(|&button| pad.just_pressed(button)))
}

/// Whether any pad pressed `control` this frame, as long as `face` has it
/// doing `does`: for a list read off the pad, whose two directions share
/// their words.
pub fn pressed_as<'a>(
    face: &Face,
    control: Pad,
    does: Does,
    pads: impl IntoIterator<Item = &'a Gamepad>,
) -> bool {
    let bound = binds(face)
        .iter()
        .any(|row| row.pad == control && row.does == does);
    let Some(button) = control.button().filter(|_| bound) else {
        return false;
    };
    pads.into_iter().any(|pad| pad.just_pressed(button))
}

/// The same question of one pad, held rather than pressed this frame.
pub fn held_on(face: &Face, does: Does, pad: &Gamepad) -> bool {
    binds(face)
        .iter()
        .filter(|row| row.does == does)
        .filter_map(|row| row.pad.button())
        .any(|button| pad.pressed(button))
}

/// The same question of one pad, pressed this frame.
pub fn pressed_on(face: &Face, does: Does, pad: &Gamepad) -> bool {
    pressed(face, does, [pad])
}

/// The control that does `does` on `face`, as the prompt names it: on the
/// editor's second layer with Select in front, since it is held for it.
/// `None` where the face has no control for it.
pub fn button_name(face: &Face, does: Does, tr: &Tr, family: PadFamily) -> Option<String> {
    let row = binds(face).into_iter().find(|row| row.does == does)?;
    Some(match *face == (Face::Editor { layer: true }) {
        true => format!("{}+{}", Select.name(tr, family), row.pad.name(tr, family)),
        false => row.pad.name(tr, family).to_string(),
    })
}

/// What a line names for the control that does `does` on `face`: the
/// keyboard's own word for it, `keys`, at a keyboard, and the pad's
/// button for a pad's player, so a sentence with a key in it reads true
/// in either hand.
pub fn spelled(tr: &Tr, device: LastDevice, face: &Face, does: Does, keys: &str) -> String {
    match (device, button_name(face, does, tr, device.family())) {
        (LastDevice::Pad(_), Some(button)) => button,
        _ => keys.to_string(),
    }
}

/// The controls of `face`, as the prompt line says them on a pad of
/// `family`: "D-pad: choose | A: play". Rows that share their words share
/// a slot, "LT/RT: level", and the d-pad's directions are one "D-pad".
pub fn legend(tr: &Tr, family: PadFamily, face: &Face) -> String {
    let mut slots: Vec<(Vec<&'static str>, &'static str)> = Vec::new();
    for row in binds(face) {
        let label = row.does.label(tr);
        let name = row.pad.name(tr, family);
        match slots.iter_mut().find(|(_, said)| *said == label) {
            Some((names, _)) => {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            None => slots.push((vec![name], label)),
        }
    }
    slots
        .iter()
        .map(|(names, label)| fill(tr.pad.bind, &[("b", &names.join("/")), ("a", label)]))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The number key the pick's row answers to.
pub fn pick_key(picks: &Picks, pick: &PadPick) -> Option<KeyCode> {
    let (number, _) = picks.rows.get(pick.row(picks))?;
    NUMBER_KEYS
        .get(usize::from(*number).checked_sub(1)?)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::i18n::EN;

    /// On a settings dial A is done here, as B is, and the legend says so
    /// rather than offering a "confirm" that leaves the screen; on a door
    /// A is the door's.
    #[test]
    fn the_settings_legend_names_what_a_does_on_the_row() {
        let xbox = crate::app::gamepad::PadFamily::Xbox;
        let dial = legend(&EN, xbox, &Face::Settings { dial: true });
        assert!(dial.contains("A/B: "), "{dial}");
        assert!(!dial.contains(EN.pad.confirm), "{dial}");
        let door = legend(&EN, xbox, &Face::Settings { dial: false });
        assert!(door.contains(&format!("A: {}", EN.pad.confirm)), "{door}");
    }

    /// Watching armed in the lobby: A watches the beach under the cursor,
    /// so with no beach on the list it is not offered, and Select (J, an
    /// address) is the way to one.
    #[test]
    fn a_watches_only_with_a_beach_on_the_list() {
        for hosts in [false, true] {
            let rows = binds(&Face::Browse {
                hosts,
                watching: true,
            });
            let a = rows.iter().any(|row| row.does == Does::Watch);
            assert_eq!(a, hosts, "hosts: {hosts}");
            assert!(rows.iter().any(|row| row.does == Does::ByAddress));
        }
    }

    /// The legend joins a face's rows into slots by their words, so the
    /// d-pad reads once and a pair of bumpers shares one.
    #[test]
    fn the_legend_shares_a_slot_between_rows_with_the_same_words() {
        assert_eq!(
            legend(&EN, crate::app::gamepad::PadFamily::Xbox, &Face::Menu),
            "D-pad: choose | A: play | X: paste a code | Y: mute | Start: quit"
        );
        assert_eq!(
            legend(&EN, crate::app::gamepad::PadFamily::Xbox, &Face::Quitting),
            "A: quit | B/Start: back"
        );
        assert_eq!(
            legend(
                &EN,
                crate::app::gamepad::PadFamily::Xbox,
                &Face::PuzzleSetup {
                    posts: true,
                    hint: false
                }
            ),
            "D-pad: move | A/B/X/Y: place | LB: remove | RB: clear | Start: start \
             | LT/RT: level | Select: menu"
        );
    }

    /// A pad is told apart by its maker's USB vendor id, and every pad not
    /// Sony's or Nintendo's wears Xbox's labels.
    #[test]
    fn a_pads_family_is_its_makers() {
        let of = PadFamily::from_ids;
        assert_eq!(of(Some(0x054C), Some(0x09CC)), PadFamily::PlayStation);
        assert_eq!(of(Some(0x054C), Some(0x0CE6)), PadFamily::DualSense);
        assert_eq!(of(Some(0x057E), Some(0x2009)), PadFamily::Nintendo);
        assert_eq!(of(Some(0x045E), Some(0x0B12)), PadFamily::Xbox);
        assert_eq!(of(Some(0x28DE), Some(0x1205)), PadFamily::Xbox, "a Deck");
        assert_eq!(of(None, None), PadFamily::Xbox);
    }

    /// The buttons go by position and are named by what is printed there:
    /// the bottom face button plays, ✕ on a PlayStation pad and B on a
    /// Nintendo one, whose A is on the right, where Xbox's B is.
    #[test]
    fn each_maker_names_the_same_positions_its_own_way() {
        assert_eq!(
            legend(&EN, PadFamily::PlayStation, &Face::Menu),
            "D-pad: choose | ✕: play | □: paste a code | △: mute | Options: quit"
        );
        assert_eq!(
            legend(&EN, PadFamily::Nintendo, &Face::Menu),
            "D-pad: choose | B: play | Y: paste a code | X: mute | +: quit"
        );
        assert_eq!(
            legend(&EN, PadFamily::Nintendo, &Face::Quitting),
            "B: quit | A/+: back"
        );
        let layer = Face::Editor { layer: true };
        let save = |family| button_name(&layer, Does::Save, &EN, family);
        assert_eq!(save(PadFamily::Xbox).as_deref(), Some("Select+A"));
        assert_eq!(save(PadFamily::PlayStation).as_deref(), Some("Share+✕"));
        assert_eq!(save(PadFamily::DualSense).as_deref(), Some("Create+✕"));
        assert_eq!(save(PadFamily::Nintendo).as_deref(), Some("−+B"));
        // At the keyboard a line names its key, whatever pad is about.
        assert_eq!(
            spelled(&EN, LastDevice::Keyboard, &Face::Menu, Does::Mute, "M"),
            "M"
        );
        assert_eq!(
            spelled(
                &EN,
                LastDevice::Pad(PadFamily::PlayStation),
                &Face::Menu,
                Does::Mute,
                "M"
            ),
            "△"
        );
    }

    /// Every name a pad's button goes by is drawn: by DejaVu, which every
    /// card row asks for, and by the display face the prompt line is set
    /// in, or else by DejaVu behind it (`boot::teach_the_kanji_fallback`).
    #[test]
    fn every_button_name_is_in_the_shipped_faces() {
        let ui = crate::app::i18n::metrics::characters_in_font(include_bytes!(
            "../../../assets/fonts/DejaVuSansMono.ttf"
        ));
        for family in PadFamily::ALL {
            for face in super::super::face::every_face() {
                for row in binds(&face) {
                    for ch in row.pad.name(&EN, family).chars() {
                        assert!(ui.contains(&ch), "{family:?}: {ch:?}");
                    }
                }
            }
        }
    }

    /// A round's table is all the pad's own: nothing in it is a key, so a
    /// pad in a round can never be read as a seat's keyboard.
    #[test]
    fn a_round_presses_no_keys() {
        for face in [
            Face::Round,
            Face::PuzzleSetup {
                posts: true,
                hint: true,
            },
            Face::PuzzleRunning,
            Face::Paused,
            Face::Replay,
            Face::Playtest(crate::app::editor::TestPhase::Match),
            Face::Playtest(crate::app::editor::TestPhase::Setup),
        ] {
            for row in binds(&face) {
                assert_eq!(row.key, Key::Direct, "{face:?}: {row:?}");
            }
        }
    }

    /// No face gives one control two jobs.
    #[test]
    fn no_control_does_two_things_on_one_face() {
        for face in super::super::face::every_face() {
            let rows = binds(&face);
            for (i, a) in rows.iter().enumerate() {
                for b in &rows[i + 1..] {
                    assert_ne!(a.pad, b.pad, "{face:?}: {a:?} and {b:?}");
                }
            }
        }
    }

    /// The pick walks its list and wraps, starts at the top of a list it
    /// has not seen, and never stands past the end of one that shrank.
    #[test]
    fn the_pick_walks_its_list() {
        let list = |n: u8, kind| Picks {
            list: kind,
            rows: (1..=n).map(|i| (i, i.to_string())).collect(),
        };
        let votes = list(3, PickList::Vote);
        let mut pick = PadPick::default();
        assert_eq!(pick_key(&votes, &pick), Some(KeyCode::Digit1));
        pick.step(&votes, Nav::Up);
        assert_eq!(pick_key(&votes, &pick), Some(KeyCode::Digit3), "wraps");
        pick.step(&votes, Nav::Down);
        assert_eq!(pick.row(&votes), 0);
        pick.step(&votes, Nav::Down);
        pick.step(&votes, Nav::Down);
        assert_eq!(pick.row(&list(2, PickList::Vote)), 1, "held to the end");
        assert_eq!(pick.row(&list(3, PickList::Winner)), 0, "a new list");
    }
}
