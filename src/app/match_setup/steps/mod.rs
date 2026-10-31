//! The three setup steps as a model and their keys: step 1's rows, step
//! 2's map cards, step 3's seats, the cursor each step remembers, and the
//! launch.
//!
//! Split from the drawing ([`ui`]) so every rule a row or a card
//! states can be asked here, by the drawing and by the tests alike, and
//! the line on screen reads the same function the key does.

use super::*;
use crate::app::gamepad::scheme::Pad;
use crate::app::gamepad::{Bridged, LastDevice, PadFamily, PadSeats, SeatPad};
use crate::app::menu_ui;
use crate::app::settings::{GameSettings, SeatInput};

mod cards;
mod game;
mod launch;
mod map_page;
mod players;
pub use cards::*;
pub use game::*;
pub use launch::*;
pub use map_page::*;
pub use players::*;

/// The setup's three steps, in the order they are walked.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Step {
    /// The game's settings: players, AI level, round, arrows, gulls,
    /// scoring, series.
    #[default]
    Game,
    /// The map, from cards.
    Map,
    /// Who plays each seat.
    Players,
}

impl Step {
    pub const ALL: [Step; 3] = [Step::Game, Step::Map, Step::Players];

    pub fn index(self) -> usize {
        match self {
            Step::Game => 0,
            Step::Map => 1,
            Step::Players => 2,
        }
    }
}

/// One of step 1's rows, top to bottom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameRow {
    Players,
    AiLevel,
    Round,
    Posts,
    Gulls,
    Scoring,
    Series,
}

impl GameRow {
    pub const ALL: [GameRow; 7] = [
        GameRow::Players,
        GameRow::AiLevel,
        GameRow::Round,
        GameRow::Posts,
        GameRow::Gulls,
        GameRow::Scoring,
        GameRow::Series,
    ];
}

/// Where each step's cursor stands, which step is showing, and the
/// setup's other passing state. Each step keeps its own cursor, so going
/// back a step and forward again lands where it was left.
#[derive(Resource, Default)]
pub struct MatchMenu {
    pub step: Step,
    /// Step 1's row.
    pub game_row: usize,
    /// Step 2's place, an index into [`map_slots`].
    pub slot: usize,
    /// Step 3's seat.
    pub seat: u8,
    /// The seat whose name is being typed, if any. While this is set the
    /// keyboard belongs to the name: navigation and launching wait.
    pub naming: Option<u8>,
    /// The `PINCH_LAUNCH` hook has pressed its Enter.
    pub launched: bool,
    /// The seed step 2's cards are drawn with, and the one the match
    /// launched from them is played on. Drawn as match setup opens.
    pub seed: u64,
    /// How far down step 2's page the window is scrolled, in pixels
    /// ([`MapPage::scrolled`]): the cards in view have their previews
    /// drawn first.
    pub scroll: f32,
    /// Step 2's page is the lobby host's pick for an online table
    /// ([`crate::app::lobby`]) rather than the couch's: a beach that cannot
    /// travel is greyed too ([`card_open`]), and Enter starts the match on
    /// the card rather than going on to step 3.
    pub online: bool,
}

/// Every step's keys. Esc goes back a step (from step 1, to the menu),
/// Enter goes on (from step 3, starts the match); the rest are the step's
/// own.
pub fn match_setup_input(
    (keys, caps): (Res<ButtonInput<KeyCode>>, Res<crate::app::keycaps::KeyCaps>),
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut menu: ResMut<MatchMenu>,
    dials: Dials,
    mut settings: ResMut<GameSettings>,
    start: MatchStart,
    mut pads: SetupPads,
) {
    let Dials {
        mut config,
        beaches,
    } = dials;
    let mut start = start;
    // Taken every frame, so a Start heard under a card or a name box is
    // not kept for later.
    let pad_start = pads.seats.take_start();
    // A doorway card is up: its keys are its own, and the match goes on
    // the moment the last bot is in.
    if start.bots.door_for.card_up() {
        if start
            .bots
            .door
            .as_ref()
            .is_some_and(crate::app::bot_seats::Doorway::ready)
        {
            start.bots.door_for = crate::app::bot_seats::DoorFor::Nothing;
            start.bots.feedback.clear();
            launch_the_table(
                &mut config,
                &menu,
                &mut start,
                &mut pads,
                &settings,
                &beaches,
            );
        }
        typed.clear();
        return;
    }
    if let Some(seat) = menu.naming {
        // The name of whoever sits there, saved on their own row.
        let row = config.name_row(seat);
        type_a_name(row, &mut typed, &keys, &mut settings, &mut menu);
        return;
    }
    // Nothing typed while not naming, but the reader must still be drained
    // or the first keystroke of a rename arrives with a backlog.
    typed.clear();
    let p2 = p2_pressed(&keys, &pads.bridged, &settings);
    if p2 {
        pads.seats.p2_sat_down();
    }
    // The partner key: a letter no seat can be bound to, yet P2's own on
    // a layout whose caps move it there, where the press is P2's.
    let partner = !p2
        && caps
            .key_for(crate::app::gamepad::scheme::PARTNER_CAP)
            .is_some_and(|key| keys.just_pressed(key));
    // The dev hook's Enter, pressed once for an unattended run, from
    // whichever step the setup opened on.
    let hooked = crate::app::dev::launch() && !std::mem::replace(&mut menu.launched, true);
    if hooked {
        start_match(
            &mut config,
            &menu,
            &mut start,
            &mut pads,
            &settings,
            &beaches,
        );
        return;
    }
    match menu.step {
        Step::Game => game_keys(
            &keys,
            &mut menu,
            &mut config,
            &mut settings,
            &beaches,
            &mut start,
            &mut pads,
        ),
        Step::Map => map_keys(&keys, &mut menu, &mut config, &beaches),
        Step::Players => players_keys(
            (&keys, pad_start, partner),
            &mut menu,
            &mut config,
            &settings,
            &beaches,
            &mut start,
            &mut pads,
        ),
    }
}

/// Step 2, as it opens: the cursor on the card picked, and the page
/// scrolled to it.
fn open_map_step(menu: &mut MatchMenu, config: &MatchConfig, beaches: &CustomBeaches) {
    put_the_cursor_on_the_pick(menu, config, beaches);
    menu.step = Step::Map;
}

/// Step 2's cursor on the card picked ([`picked_card`]), scrolled from
/// where the page was left the least that brings it into view.
pub(super) fn put_the_cursor_on_the_pick(
    menu: &mut MatchMenu,
    config: &MatchConfig,
    beaches: &CustomBeaches,
) {
    let picked = Slot::Card(picked_card(config, beaches));
    let slots = map_slots(beaches);
    menu.slot = slots
        .iter()
        .position(|(_, slot)| *slot == picked)
        .unwrap_or(0);
    menu.scroll = MapPage::of(&slots).scrolled(menu.scroll, menu.slot);
}
