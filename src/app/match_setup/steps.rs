//! The three setup steps as a model and their keys: step 1's rows, step
//! 2's map cards, step 3's seats, the cursor each step remembers, and the
//! launch.
//!
//! Split from the drawing ([`ui`]) so every rule a row or a card
//! states can be asked here, by the drawing and by the tests alike, and
//! the line on screen reads the same function the key does.

use super::*;
use crate::app::gamepad::{LastDevice, PadSeats, SeatPad};
use crate::app::menu_ui;
use crate::app::settings::{GameSettings, SeatInput};

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

// --- step 1 ----------------------------------------------------------------

/// Turn step 1's `row` one notch. Scoring is the settings' team mode,
/// the one Settings turns too; everything else is the match's own.
pub fn turn_game_row(
    row: GameRow,
    turn: Turn,
    config: &mut MatchConfig,
    settings: &mut GameSettings,
    beaches: &CustomBeaches,
) {
    match row {
        GameRow::Players => {
            let seats = crate::app::cycle::dial(config.seats, turn, 1, SEATS_RANGE);
            reseat(config, seats, beaches);
        }
        // The level of every AI seat: a row that said "for every AI seat"
        // and left the seats where they were would be a row that lies.
        GameRow::AiLevel => {
            config.ai_level = config.ai_level.cycled(turn);
            let level = config.ai_level;
            for controller in &mut config.controllers {
                if controller.ai().is_some() {
                    *controller = SeatController::Ai(level);
                }
            }
        }
        GameRow::Round => config.round = config.round.cycled(turn),
        GameRow::Posts => {
            config.posts = crate::app::cycle::dial(config.posts, turn, 1, POSTS_RANGE);
        }
        GameRow::Gulls => config.gulls = config.gulls.cycled(turn),
        // Every mode, whether the table fits it yet or not, as the lobby's
        // teams dial turns: the row says when it starts.
        GameRow::Scoring => settings.team_mode = settings.team_mode.cycled(turn),
        GameRow::Series => config.series = config.series.cycled(turn),
    }
}

/// Step 1's row as it reads: its name, its value with the dial's `< >`,
/// and the quiet aside after the value, if it has one.
pub fn game_row_text(
    tr: &crate::app::i18n::Tr,
    row: GameRow,
    config: &MatchConfig,
    settings: &GameSettings,
) -> (&'static str, String, &'static str) {
    let dial = |value: &str| format!("< {value} >");
    match row {
        GameRow::Players => (
            tr.match_players,
            dial(&config.seats.to_string()),
            tr.setup_players_aside,
        ),
        GameRow::AiLevel => (
            tr.match_ai_level,
            dial(tr.bot_levels[config.ai_level.index()]),
            tr.setup_ai_aside,
        ),
        GameRow::Round => (
            tr.match_round,
            dial(tr.round_names[config.round.index()]),
            "",
        ),
        GameRow::Posts => (tr.match_posts, dial(&config.posts.to_string()), ""),
        GameRow::Gulls => (
            tr.match_gulls,
            dial(tr.gull_names[config.gulls.index()]),
            "",
        ),
        GameRow::Scoring => (
            tr.setup_scoring,
            dial(&crate::app::teams::row_value(
                tr,
                settings.team_mode,
                config.seats,
                true,
            )),
            "",
        ),
        GameRow::Series => (
            tr.setup_series,
            dial(tr.mode_names[config.series.index()]),
            "",
        ),
    }
}

/// What step 1's `row` is for, said under the rows while the cursor is
/// on it. The players range is filled from [`SEATS_RANGE`].
pub fn game_row_note(tr: &crate::app::i18n::Tr, row: GameRow) -> String {
    let index = GameRow::ALL.iter().position(|r| *r == row).unwrap_or(0);
    fill(
        tr.setup_game_notes[index],
        &[
            ("a", &SEATS_RANGE.start().to_string()),
            ("b", &SEATS_RANGE.end().to_string()),
        ],
    )
}

// --- step 2 ----------------------------------------------------------------

/// One of step 2's cards: a built-in map, a beach off the player's shelf
/// (by its place on the whole shelf), or Random.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Card {
    Map(MapChoice),
    Shelf(usize),
    Random,
}

/// Step 2's three sections, top to bottom, each a heading over a grid of
/// cards: the maps that ship with the game, the player's own, and the two
/// cards that pick the map for the player.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapSection {
    /// The handmade maps and the generated sizes.
    BuiltIn,
    /// Every beach on the player's shelf, or the card saying how to make
    /// one while there is none.
    Yours,
    /// Surprise and Random.
    Chance,
}

impl MapSection {
    pub const ALL: [MapSection; 3] = [MapSection::BuiltIn, MapSection::Yours, MapSection::Chance];

    pub fn index(self) -> usize {
        match self {
            MapSection::BuiltIn => 0,
            MapSection::Yours => 1,
            MapSection::Chance => 2,
        }
    }

    /// The section a map's card sits in, which is also its stretch of the
    /// dial ([`MapChoice::DIAL`]).
    pub fn of(map: MapChoice) -> MapSection {
        match map {
            MapChoice::Classic
            | MapChoice::GenSmall
            | MapChoice::GenClassic
            | MapChoice::GenLarge
            | MapChoice::GenXl
            | MapChoice::GenOcean
            | MapChoice::TheGates
            | MapChoice::TheMoat
            | MapChoice::LogJam
            | MapChoice::ThreeLanes
            | MapChoice::RoundTable
            | MapChoice::RoundTheWorld
            | MapChoice::Duel
            | MapChoice::TheFountain
            | MapChoice::FourDoors
            | MapChoice::GullRoad
            | MapChoice::BackToBack => MapSection::BuiltIn,
            MapChoice::Custom => MapSection::Yours,
            MapChoice::Surprise => MapSection::Chance,
        }
    }
}

/// One place in step 2's grid: a card, or the card standing in for the
/// player's own maps while the shelf is empty, which says how to make one.
/// That one is not a map: the cursor may rest on it so its line can
/// explain, but it is never picked and Enter does not go on from it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Card(Card),
    MakeOne,
}

/// Step 2's places in the order they are laid out, each with its section:
/// the built-in maps in the dial's order ([`MapChoice::DIAL`]: the
/// handmade maps, then the generated sizes), every beach on the player's
/// shelf (fitting the table or not, so a beach too small is greyed rather
/// than missing) or, with none, the card saying how to make one, then
/// Surprise and Random.
pub fn map_slots(beaches: &CustomBeaches) -> Vec<(MapSection, Slot)> {
    let maps_in = |section: MapSection| {
        MapChoice::DIAL
            .into_iter()
            .filter(move |&map| map != MapChoice::Custom && MapSection::of(map) == section)
            .map(move |map| (section, Slot::Card(Card::Map(map))))
    };
    let yours: Vec<(MapSection, Slot)> = match beaches.0.is_empty() {
        true => vec![(MapSection::Yours, Slot::MakeOne)],
        false => (0..beaches.0.len())
            .map(|shelf| (MapSection::Yours, Slot::Card(Card::Shelf(shelf))))
            .collect(),
    };
    maps_in(MapSection::BuiltIn)
        .chain(yours)
        .chain(maps_in(MapSection::Chance))
        .chain(std::iter::once((
            MapSection::Chance,
            Slot::Card(Card::Random),
        )))
        .collect()
}

/// Step 2's cards, in the order they are laid out ([`map_slots`]), without
/// the card saying how to make a map.
pub fn cards(beaches: &CustomBeaches) -> Vec<Card> {
    map_slots(beaches)
        .into_iter()
        .filter_map(|(_, slot)| match slot {
            Slot::Card(card) => Some(card),
            Slot::MakeOne => None,
        })
        .collect()
}

/// How many players `card` seats, least and most, or `None` for a
/// handmade beach whose castles skip a number and so seat no table. Read
/// off [`card_tables`], so the label, the picture and the greying all go
/// by the one rule.
pub fn card_seats(card: Card, beaches: &CustomBeaches) -> Option<(u8, u8)> {
    let tables = card_tables(card, beaches);
    Some((*tables.first()?, *tables.last()?))
}

/// Every table `card` can be played by, smallest first: the counts in
/// [`SEATS_RANGE`] that [`card_fits`] takes, which for a beach off the
/// shelf is what [`crate::sim::Level::fits`] says, its `seats:` line
/// included. Counted off the castles alone, a `seats: 4` beach was
/// labelled "2-4" while greyed for two, and drawn for two, which it does
/// not seat, so its card had no picture.
pub fn card_tables(card: Card, beaches: &CustomBeaches) -> Vec<u8> {
    SEATS_RANGE
        .filter(|&seats| card_fits(card, seats, beaches))
        .collect()
}

/// Whether `card` names the tables it is built for, a handmade map's own
/// list or a shelf beach's `seats:` line, which its card says as that
/// list ("2/4") rather than as a range with an odd one inside.
pub fn card_lists_its_tables(card: Card, beaches: &CustomBeaches) -> bool {
    match card {
        Card::Map(map) => map.tables().is_some(),
        Card::Shelf(shelf) => beaches
            .0
            .get(shelf)
            .is_some_and(|beach| !beach.level.tables.is_empty()),
        Card::Random => false,
    }
}

/// Whether `card` can be played by a table of `seats`. A card that cannot
/// is greyed: the cursor may rest on it, so its line can say why, but it
/// is not picked and Enter does not go on from it.
pub fn card_fits(card: Card, seats: u8, beaches: &CustomBeaches) -> bool {
    match card {
        Card::Map(map) => holds(map, seats),
        Card::Shelf(shelf) => beaches.among_fitting(seats, shelf).is_some(),
        Card::Random => true,
    }
}

/// Whether `card` can be sent to every peer at an online table: a beach
/// off the shelf too big for an invitation plays on this machine only
/// ([`Beach::too_big_to_send`]).
pub fn card_travels(card: Card, beaches: &CustomBeaches) -> bool {
    match card {
        Card::Shelf(shelf) => beaches
            .0
            .get(shelf)
            .is_some_and(|beach| !beach.too_big_to_send()),
        Card::Map(_) | Card::Random => true,
    }
}

/// Whether `card` can be picked for a table of `seats`: it seats the table
/// ([`card_fits`]) and, `online`, it travels ([`card_travels`]). The one
/// rule the greying, the pick and the line under the cards read.
pub fn card_open(card: Card, seats: u8, beaches: &CustomBeaches, online: bool) -> bool {
    card_fits(card, seats, beaches) && (!online || card_travels(card, beaches))
}

/// The card step 2 lights as picked: Random under the Random card, else
/// the card of the map in force, which is the player's pick unless the
/// seat count moved it ([`MatchConfig::pick`]).
pub fn picked_card(config: &MatchConfig, beaches: &CustomBeaches) -> Card {
    if config.random {
        return Card::Random;
    }
    match config.map {
        MapChoice::Custom => beaches
            .on_shelf(config.seats, config.custom)
            .map_or(Card::Map(MapChoice::GenXl), Card::Shelf),
        map @ (MapChoice::Classic
        | MapChoice::GenSmall
        | MapChoice::GenClassic
        | MapChoice::GenLarge
        | MapChoice::GenXl
        | MapChoice::GenOcean
        | MapChoice::TheGates
        | MapChoice::TheMoat
        | MapChoice::LogJam
        | MapChoice::ThreeLanes
        | MapChoice::RoundTable
        | MapChoice::RoundTheWorld
        | MapChoice::Duel
        | MapChoice::TheFountain
        | MapChoice::FourDoors
        | MapChoice::GullRoad
        | MapChoice::BackToBack
        | MapChoice::Surprise) => Card::Map(map),
    }
}

/// Pick `card`, when the table fits it: the map in force is then the
/// pick itself, and nothing is said to have moved.
pub fn pick_card(config: &mut MatchConfig, card: Card, beaches: &CustomBeaches) {
    if !card_fits(card, config.seats, beaches) {
        return;
    }
    match card {
        Card::Map(map) => {
            config.map = map;
            config.random = false;
            config.pick = None;
        }
        Card::Shelf(shelf) => {
            if let Some(custom) = beaches.among_fitting(config.seats, shelf) {
                config.map = MapChoice::Custom;
                config.custom = custom;
                config.random = false;
                config.pick = None;
            }
        }
        Card::Random => config.random = true,
    }
}

/// The config a card's match is played on, for drawing its preview: the
/// config with the card's map picked. `None` for Random and Surprise,
/// which have no one beach to show, and for a card that seats no table. A card the table
/// does not fit is drawn for the least table it does fit, whatever the
/// table's count, so the count moving does not redraw it.
pub fn card_config(
    config: &MatchConfig,
    card: Card,
    beaches: &CustomBeaches,
) -> Option<MatchConfig> {
    let seats = match card_fits(card, config.seats, beaches) {
        true => config.seats,
        false => card_seats(card, beaches)?.0,
    };
    let mut shown = MatchConfig {
        seats,
        random: false,
        ..config.clone()
    };
    match card {
        // A new beach every round: its card has art of its own rather
        // than one round's picture.
        Card::Map(MapChoice::Surprise) => return None,
        Card::Map(map) => shown.map = map,
        Card::Shelf(shelf) => {
            shown.map = MapChoice::Custom;
            shown.custom = beaches.among_fitting(seats, shelf)?;
        }
        Card::Random => return None,
    }
    Some(shown)
}

// --- step 3 ----------------------------------------------------------------

/// The team mode step 3's table plays: the one set on step 1 where the
/// table fits it, else free-for-all ([`TeamMode::played_by`]). Partners
/// are picked on step 3 under it, and the match is scored by it.
///
/// [`TeamMode::played_by`]: crate::app::teams::TeamMode::played_by
pub fn setup_teams(settings: &GameSettings, config: &MatchConfig) -> crate::app::teams::TeamMode {
    settings.team_mode.played_by(config.seats)
}

/// What a seat can be set to on step 3, in the order A/D steps through
/// them: a person, the AI at each level, a bot.
pub const SEAT_KINDS: [SeatController; 5] = [
    SeatController::Local,
    SeatController::Ai(BotLevel::Easy),
    SeatController::Ai(BotLevel::Normal),
    SeatController::Ai(BotLevel::Hard),
    SeatController::Bot,
];

/// Whether A/D turns who plays `seat`: every seat but player 1's, which
/// stays a person, whoever is working the menu being player 1. What
/// [`turn_seat`] does, and what step 3's hint and prompt say.
pub fn seat_turns(seat: u8) -> bool {
    seat != 0
}

/// Step who plays `seat` one notch, where [`seat_turns`] says it turns.
pub fn turn_seat(config: &mut MatchConfig, seat: u8, turn: Turn) {
    if !seat_turns(seat) || seat >= config.seats {
        return;
    }
    let now = config.controller(seat);
    let at = SEAT_KINDS.iter().position(|c| *c == now).unwrap_or(0);
    let next = crate::app::cycle::dial(at as u8, turn, 1, 0..=SEAT_KINDS.len() as u8 - 1);
    config.controllers[usize::from(seat)] = SEAT_KINDS[usize::from(next)];
}

/// Who plays a seat, as step 3 says it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlayedBy {
    /// A keyboard seat's keys: P1's, or P2's once one was pressed.
    Keys(u8),
    /// Controller `number`: the pad the seat rule gives the seat
    /// ([`PadSeats`]). The round reads the same rule, so this pad is the
    /// one that plays it. `joined` when its player pressed Start for the
    /// seat ([`SeatPad::joined`]), not when the rule only dealt it.
    Pad {
        number: u8,
        joined: bool,
    },
    /// A person's seat nobody has sat down in. `keys`: P2's keys would
    /// take it too. The match fills it with the AI ([`open_seats`]).
    Open {
        keys: bool,
    },
    Ai(BotLevel),
    Bot,
}

/// Whether P2's keyboard keys can play the second seat: not when P2's
/// settings row names a controller.
fn p2_keys(settings: &GameSettings) -> bool {
    !matches!(settings.seat_input[1], SeatInput::Pad(_))
}

/// Who plays `seat`, with `pads` the pads seated by the one seat rule
/// ([`PadSeats::seat_pads`]) and `p2_here` whether P2 pressed one of
/// their own keys ([`PadSeats::p2_at_keys`]).
pub fn played_by(
    config: &MatchConfig,
    seat: u8,
    pads: &[SeatPad],
    p2_here: bool,
    settings: &GameSettings,
) -> PlayedBy {
    match config.controller(seat) {
        SeatController::Ai(level) => return PlayedBy::Ai(level),
        SeatController::Bot => return PlayedBy::Bot,
        SeatController::Local | SeatController::Remote => {}
    }
    // A seat the rule gives a pad is that pad's, claimed or dealt: the
    // round drives it with that pad, so the AI never sits there. Player 1
    // is at the menu, on the keys unless the rule gives P1 a pad.
    if let Some(pad) = pads.iter().find(|pad| pad.seat == seat) {
        return PlayedBy::Pad {
            number: pad.number,
            joined: pad.joined,
        };
    }
    if seat == 0 {
        return PlayedBy::Keys(0);
    }
    let keys = seat == 1 && p2_keys(settings);
    match keys && p2_here {
        true => PlayedBy::Keys(1),
        false => PlayedBy::Open { keys },
    }
}

/// The person's seats nobody has sat down in, which the match is started
/// with the AI in at step 1's level.
pub fn open_seats(
    config: &MatchConfig,
    pads: &[SeatPad],
    p2_here: bool,
    settings: &GameSettings,
) -> Vec<u8> {
    (0..config.seats)
        .filter(|&seat| {
            matches!(
                played_by(config, seat, pads, p2_here, settings),
                PlayedBy::Open { .. }
            )
        })
        .collect()
}

/// What step 3's "played by" cell says.
pub fn played_by_text(tr: &crate::app::i18n::Tr, by: PlayedBy) -> String {
    match by {
        PlayedBy::Keys(seat) => fill(
            tr.setup_by_keys,
            &[("p", &crate::app::seat_label(tr, seat))],
        ),
        PlayedBy::Pad { number, .. } => fill(tr.setup_by_pad, &[("n", &number.to_string())]),
        PlayedBy::Open { keys: false } => tr.setup_by_open.to_string(),
        PlayedBy::Open { keys: true } => crate::app::name_the_pair(tr, tr.setup_by_open_keys),
        PlayedBy::Ai(level) => fill(tr.setup_by_ai, &[("l", tr.bot_levels[level.index()])]),
        PlayedBy::Bot => tr.setup_by_bot.to_string(),
    }
}

/// The status at the end of a seat's row: ready at the keys or on a pad
/// the rule dealt the seat, joined on a pad whose player pressed Start for
/// it, the AI to come in an open seat, and nothing for the AI's own.
pub fn status_text(tr: &crate::app::i18n::Tr, by: PlayedBy) -> &'static str {
    match by {
        PlayedBy::Keys(_) | PlayedBy::Pad { joined: false, .. } => tr.setup_status[0],
        PlayedBy::Pad { joined: true, .. } => tr.setup_status[1],
        PlayedBy::Open { .. } => tr.setup_status[2],
        PlayedBy::Ai(_) | PlayedBy::Bot => "",
    }
}

/// The line under step 3's seats: which seats are open and what fills
/// them, else how a friend joins when a pad here could, else nothing.
/// The second half says whether it is the warning.
pub fn seats_line(
    tr: &crate::app::i18n::Tr,
    open: &[u8],
    level: BotLevel,
    pad_to_join: bool,
) -> (String, bool) {
    if !open.is_empty() {
        let list: Vec<String> = open
            .iter()
            .map(|&seat| crate::app::seat_label(tr, seat))
            .collect();
        return (
            fill(
                tr.setup_open_line,
                &[
                    ("list", &list.join(", ")),
                    ("l", tr.bot_levels[level.index()]),
                ],
            ),
            true,
        );
    }
    match pad_to_join {
        true => (tr.setup_join_hint.to_string(), false),
        false => (String::new(), false),
    }
}

/// The hint under step 3's seats: what A/D does to the seat under the
/// cursor, by the keys or the pad's d-pad, as the step's own table names
/// it. Nothing on P1's row, which A/D leaves alone ([`turn_seat`]).
pub fn seat_hint(tr: &crate::app::i18n::Tr, device: LastDevice, seat: u8) -> String {
    if !seat_turns(seat) {
        return String::new();
    }
    let keys = match device {
        LastDevice::Keyboard => "A/D".to_string(),
        LastDevice::Pad => crate::app::gamepad::scheme::Pad::Left.name(tr).to_string(),
    };
    fill(tr.setup_seat_hint, &[("k", &keys)])
}

/// A seat with no name yet, saying the key that opens it for typing:
/// "Tab" in every language, which a pad's player is told as step 3's
/// button for it.
pub fn unnamed(tr: &crate::app::i18n::Tr, device: LastDevice) -> String {
    use crate::app::gamepad::scheme::{Does, button_name};
    let face = crate::app::gamepad::Face::MatchSetup {
        step: Step::Players,
        join: true,
        partners: false,
    };
    match (device, button_name(&face, Does::Rename, tr)) {
        (LastDevice::Pad, Some(button)) => tr.match_name_empty.replacen("Tab", &button, 1),
        _ => tr.match_name_empty.to_string(),
    }
}

/// Whether this frame's keys include one of P2's own: a key bound to the
/// second keyboard seat that neither player 1 nor the menu also uses, so
/// the person working the menu cannot join as P2 by accident.
///
/// Read under the commit rule the round itself plays with
/// ([`GameSettings::commit_in_play`], keyboard shared): a versus round puts
/// P1's placement on the arrows whatever the preset, so IJKL is P2's here
/// too, and the one-hand preset must not hide the keys P2 will play with.
pub fn p2_pressed(keys: &ButtonInput<KeyCode>, settings: &GameSettings) -> bool {
    p2_only_keys(settings)
        .into_iter()
        .any(|key| keys.just_pressed(key))
}

/// P2's own keys, as [`p2_pressed`] reads them: bound to the second
/// keyboard seat, and used by neither player 1 nor the menu. Every other
/// key is the hands of whoever works the setup
/// ([`crate::app::gamepad::follow_the_setup_hands`]).
pub fn p2_only_keys(settings: &GameSettings) -> Vec<KeyCode> {
    use crate::app::cursor::keymap;
    let menu = [
        KeyCode::Enter,
        KeyCode::NumpadEnter,
        KeyCode::Escape,
        KeyCode::Tab,
        KeyCode::Backspace,
    ];
    let commit = settings.commit_in_play(true);
    let p1: Vec<KeyCode> = keymap(settings, 0, commit).keys().collect();
    keymap(settings, 1, commit)
        .keys()
        .filter(|key| !p1.contains(key) && !menu.contains(key))
        .collect()
}

/// The seats a bot holds in the match as set up.
pub fn bot_seat_list(config: &MatchConfig) -> Vec<u8> {
    (0..config.seats)
        .filter(|&seat| config.controller(seat) == SeatController::Bot)
        .collect()
}

// --- the launch --------------------------------------------------------------

/// What starting the match sets going: the series, the screen, and the
/// doorway the table's bots come in by.
#[derive(bevy::ecs::system::SystemParam)]
pub struct MatchStart<'w> {
    tournament: ResMut<'w, crate::app::tournament::Tournament>,
    next_screen: ResMut<'w, NextState<Screen>>,
    bots: ResMut<'w, crate::app::bot_seats::BotSeats>,
}

impl MatchStart<'_> {
    /// Put the match on: armed, the series begun, the arena opened.
    fn launch(&mut self, config: &mut MatchConfig) {
        config.armed = true;
        *self.tournament = if config.series.is_series() {
            crate::app::tournament::Tournament::start(config.series)
        } else {
            crate::app::tournament::Tournament::default()
        };
        self.next_screen.set(Screen::Versus);
    }

    /// Whether the table's bots are all in, opening the doorway for any
    /// that are not. A table with no bot seat is ready at once.
    fn bots_in(&mut self, config: &MatchConfig, tr: &crate::app::i18n::Tr) -> bool {
        use crate::app::bot_seats::{DoorFor, Doorway};
        let seats = bot_seat_list(config);
        if seats.is_empty() {
            self.bots.door = None;
            return true;
        }
        if self.bots.door.is_none() {
            match Doorway::open() {
                Ok(door) => self.bots.door = Some(door),
                Err(e) => {
                    self.bots.feedback = fill(tr.door_could_not_listen, &[("e", &e.to_string())]);
                    return false;
                }
            }
        }
        let Some(door) = &mut self.bots.door else {
            return false;
        };
        door.seat_bots(&seats);
        door.poll();
        if door.ready() {
            return true;
        }
        self.bots.door_for = DoorFor::Couch;
        false
    }
}

/// Make the table the match is played at, as Start match does: every open
/// seat to the AI at step 1's level for this match (marked, so match
/// setup opens with it open again: [`open_the_seats_filled_at_start`]),
/// and the map the match is played on, drawn now under the Random card,
/// else on the seed its card was drawn with.
pub fn seat_the_match(
    config: &mut MatchConfig,
    open: &[u8],
    preview_seed: u64,
    beaches: &CustomBeaches,
    roll: u64,
) {
    for &seat in open {
        config.controllers[usize::from(seat)] = SeatController::Ai(config.ai_level);
        config.filled_at_start[usize::from(seat)] = true;
    }
    match config.random {
        true => {
            draw_map(config, beaches, roll);
            config.seed = None;
        }
        false => config.seed = Some(preview_seed),
    }
}

/// The pads plugged in, and who they seat.
#[derive(bevy::ecs::system::SystemParam)]
pub struct SetupPads<'w, 's> {
    pub seats: ResMut<'w, PadSeats>,
    pub live: Query<'w, 's, Entity, With<Gamepad>>,
}

impl SetupPads<'_, '_> {
    /// The pads seated at `config`'s person's seats, by the seat rule.
    pub fn seated(&self, config: &MatchConfig, settings: &GameSettings) -> Vec<SeatPad> {
        let live: Vec<Entity> = self.live.iter().collect();
        self.seats.seat_pads(&live, settings, &config.human_seats())
    }
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
    let p2 = p2_pressed(&keys, &settings);
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

/// Step 1: W/S choose a row, A/D turn it, Enter on to the map, Esc back
/// to the menu.
fn game_keys(
    keys: &ButtonInput<KeyCode>,
    menu: &mut MatchMenu,
    config: &mut MatchConfig,
    settings: &mut GameSettings,
    beaches: &CustomBeaches,
    start: &mut MatchStart,
    pads: &mut SetupPads,
) {
    if keys.just_pressed(KeyCode::Escape) {
        start.next_screen.set(Screen::Menu);
        return;
    }
    if menu_ui::enter(keys) {
        open_map_step(menu, config, beaches);
        return;
    }
    menu.game_row = menu_ui::nav(keys, menu.game_row, GameRow::ALL.len());
    let Some(turn) = menu_ui::left_right(keys) else {
        return;
    };
    let row = GameRow::ALL[menu.game_row];
    turn_game_row(row, turn, config, settings, beaches);
    if row == GameRow::Players {
        // The people and the AI are now what the dials say.
        pads.seats.table_set_by_hand();
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
pub fn put_the_cursor_on_the_pick(
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

/// Step 2's grid is this many cards across, in every section.
pub const CARD_COLUMNS: usize = 4;

/// The cards the Random card's mosaic shows: four of the built-in maps,
/// whose pictures it borrows.
pub const MOSAIC: [MapChoice; 4] = [
    MapChoice::Classic,
    MapChoice::GenLarge,
    MapChoice::GenOcean,
    MapChoice::GenXl,
];

/// One row of step 2's page: the places on it ([`map_slots`] indices),
/// its section, and how far down the page its top sits, in pixels.
#[derive(Clone, PartialEq, Debug)]
pub struct PageRow {
    pub section: MapSection,
    pub slots: std::ops::Range<usize>,
    pub top: f32,
    /// The section's first row, the one its heading sits on.
    pub first: bool,
}

/// Step 2's page laid out as the screen lays it out: each section a
/// heading over rows of [`CARD_COLUMNS`] cards, one under the other. The
/// sizes are the screen's own ([`ui::TILE_H`] and the rest), so what this
/// says is in view is what the window shows, and the scroll bar, the
/// cards hidden and the pictures drawn first all read it.
#[derive(Clone, PartialEq, Debug)]
pub struct MapPage {
    pub rows: Vec<PageRow>,
    /// Where each section's heading sits, in [`MapSection::ALL`] order.
    pub headings: [f32; 3],
    /// The whole page's height.
    pub height: f32,
}

impl MapPage {
    /// The page `slots` ([`map_slots`]) make. Each section's places are
    /// one run of them, in section order.
    pub fn of(slots: &[(MapSection, Slot)]) -> MapPage {
        use super::ui::{HEADING_H, SECTION_GAP, TILE_GAP, TILE_H};
        let mut rows = Vec::new();
        let mut headings = [0.0; 3];
        let mut y = 0.0;
        for section in MapSection::ALL {
            let places: Vec<usize> = slots
                .iter()
                .enumerate()
                .filter(|(_, (of, _))| *of == section)
                .map(|(index, _)| index)
                .collect();
            if section.index() > 0 {
                y += SECTION_GAP;
            }
            headings[section.index()] = y;
            y += HEADING_H;
            for (n, row) in places.chunks(CARD_COLUMNS).enumerate() {
                if n > 0 {
                    y += TILE_GAP;
                }
                rows.push(PageRow {
                    section,
                    slots: row[0]..row[row.len() - 1] + 1,
                    top: y,
                    first: n == 0,
                });
                y += TILE_H;
            }
        }
        MapPage {
            rows,
            headings,
            height: y,
        }
    }

    /// How many places the page has.
    pub fn places(&self) -> usize {
        self.rows.last().map_or(0, |row| row.slots.end)
    }

    /// The row place `slot` is on.
    pub fn row_of(&self, slot: usize) -> usize {
        self.rows
            .iter()
            .position(|row| row.slots.contains(&slot))
            .unwrap_or(self.rows.len().saturating_sub(1))
    }

    /// The scroll from `scroll` the least that brings `slot`'s row whole
    /// into view, its section's heading with it on the section's first
    /// row. The window's top is always a heading's top or a row's, so the
    /// window never opens on the empty bottom of a card it hides.
    pub fn scrolled(&self, scroll: f32, slot: usize) -> f32 {
        use super::ui::{TILE_H, VIEW_H};
        let Some(row) = self.rows.get(self.row_of(slot)) else {
            return 0.0;
        };
        let top = match row.first {
            true => self.headings[row.section.index()],
            false => row.top,
        };
        let bottom = row.top + TILE_H;
        if top < scroll {
            return top;
        }
        if bottom <= scroll + VIEW_H + 0.5 {
            return scroll;
        }
        // The first top far enough down to show the row's foot: there is
        // one, since the row's own reach is shorter than the window.
        self.headings
            .iter()
            .copied()
            .chain(self.rows.iter().filter(|row| !row.first).map(|row| row.top))
            .filter(|&line| line + VIEW_H + 0.5 >= bottom)
            .fold(top, f32::min)
    }

    /// Whether a thing `height` tall at `top` is whole in the window
    /// scrolled to `scroll`. Only whole ones are drawn: a card or a
    /// heading cut by the window's edge reads as a glitch.
    pub fn whole_in_view(scroll: f32, top: f32, height: f32) -> bool {
        top >= scroll - 0.5 && top + height <= scroll + super::ui::VIEW_H + 0.5
    }

    /// Whether place `slot`'s row is whole in view at `scroll`.
    pub fn slot_in_view(&self, scroll: f32, slot: usize) -> bool {
        self.rows
            .get(self.row_of(slot))
            .is_some_and(|row| Self::whole_in_view(scroll, row.top, super::ui::TILE_H))
    }

    /// Whether `section`'s heading is whole in view at `scroll`.
    pub fn heading_in_view(&self, scroll: f32, section: MapSection) -> bool {
        Self::whole_in_view(scroll, self.headings[section.index()], super::ui::HEADING_H)
    }

    /// The page as its scroll bar sees it, in pixels: the window's height
    /// at `scroll` down a page this tall.
    pub fn window(&self, scroll: f32) -> menu_ui::scroll_bar::ScrollWindow {
        menu_ui::scroll_bar::ScrollWindow {
            first: scroll,
            shown: super::ui::VIEW_H,
            total: self.height,
        }
    }

    /// The cursor on place `at` moved by `(dx, dy)`. A/D walk the places
    /// in order, from one row's end onto the next row and from one section
    /// into the next; W/S go a row up or down, across a section's edge
    /// too, to the same column, or the row's last card when it is shorter.
    /// Past the page's first or last place nothing moves.
    pub fn step(&self, at: usize, (dx, dy): (i32, i32)) -> usize {
        let mut at = at.min(self.places().saturating_sub(1));
        let moved = at as i64 + i64::from(dx);
        if dx != 0 && (0..self.places() as i64).contains(&moved) {
            at = moved as usize;
        }
        if dy != 0 {
            let row = self.row_of(at);
            let column = at - self.rows[row].slots.start;
            let target = usize::try_from(row as i64 + i64::from(dy))
                .ok()
                .and_then(|target| self.rows.get(target));
            if let Some(target) = target {
                at = (target.slots.start + column).min(target.slots.end - 1);
            }
        }
        at
    }
}

/// What a press on the map page asks for, beyond walking the cards.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapPress {
    Nothing,
    /// Esc: back where the page was opened from.
    Back,
    /// Enter on a card the table can play, which is now picked.
    Chosen(Card),
}

/// The map page's keys, wherever it is up: WASD walk the cards, picking
/// each one the table can play ([`card_open`]), Enter on one of those is
/// [`MapPress::Chosen`], Esc is [`MapPress::Back`]. The card saying how to
/// make a map is walked onto like the rest, and neither picked nor chosen.
/// What Back and Chosen lead to is the caller's: step 1 and step 3 on the
/// couch, the lobby and the match online.
/// The card Enter picks on the map page: the one under the cursor, when
/// the table can play it. `None` on the card saying how to make a map and
/// on a greyed card, where Enter does nothing, which the page's prompt
/// and a pad's A read before offering the key.
pub fn card_to_pick(
    menu: &MatchMenu,
    config: &MatchConfig,
    beaches: &CustomBeaches,
) -> Option<Card> {
    let slots = map_slots(beaches);
    match slots[menu.slot.min(slots.len() - 1)].1 {
        Slot::Card(card) if card_open(card, config.seats, beaches, menu.online) => Some(card),
        Slot::Card(_) | Slot::MakeOne => None,
    }
}

pub fn map_page_keys(
    keys: &ButtonInput<KeyCode>,
    menu: &mut MatchMenu,
    config: &mut MatchConfig,
    beaches: &CustomBeaches,
) -> MapPress {
    if keys.just_pressed(KeyCode::Escape) {
        return MapPress::Back;
    }
    let slots = map_slots(beaches);
    let at = menu.slot.min(slots.len() - 1);
    let open = |card: Card| card_open(card, config.seats, beaches, menu.online);
    if menu_ui::enter(keys) {
        return match card_to_pick(menu, config, beaches) {
            Some(card) => {
                pick_card(config, card, beaches);
                MapPress::Chosen(card)
            }
            None => MapPress::Nothing,
        };
    }
    let pressed = |codes: [KeyCode; 2]| codes.iter().any(|&code| keys.just_pressed(code));
    let dx = i32::from(pressed([KeyCode::KeyD, KeyCode::ArrowRight]))
        - i32::from(pressed([KeyCode::KeyA, KeyCode::ArrowLeft]));
    let dy = i32::from(pressed([KeyCode::KeyS, KeyCode::ArrowDown]))
        - i32::from(pressed([KeyCode::KeyW, KeyCode::ArrowUp]));
    if (dx, dy) == (0, 0) {
        return MapPress::Nothing;
    }
    let page = MapPage::of(&slots);
    menu.slot = page.step(at, (dx, dy));
    menu.scroll = page.scrolled(menu.scroll, menu.slot);
    if let Slot::Card(card) = slots[menu.slot].1
        && open(card)
    {
        pick_card(config, card, beaches);
    }
    MapPress::Nothing
}

/// Step 2: the map page's keys ([`map_page_keys`]), Enter on to the
/// players from a card the table fits, Esc back to step 1.
fn map_keys(
    keys: &ButtonInput<KeyCode>,
    menu: &mut MatchMenu,
    config: &mut MatchConfig,
    beaches: &CustomBeaches,
) {
    match map_page_keys(keys, menu, config, beaches) {
        MapPress::Back => menu.step = Step::Game,
        MapPress::Chosen(_) => menu.step = Step::Players,
        MapPress::Nothing => {}
    }
}

/// Step 3: W/S choose a seat, in the order the rows are shown
/// ([`seat_rows`]), A/D change who plays it, P picks its partners under a
/// team mode (`partner`), Tab names it, Enter starts the match, and so
/// does `pad_start`, a seated pad's Start
/// ([`crate::app::gamepad::pad_claim_seats`]); Esc back to the map.
fn players_keys(
    (keys, pad_start, partner): (&ButtonInput<KeyCode>, bool, bool),
    menu: &mut MatchMenu,
    config: &mut MatchConfig,
    settings: &GameSettings,
    beaches: &CustomBeaches,
    start: &mut MatchStart,
    pads: &mut SetupPads,
) {
    if keys.just_pressed(KeyCode::Escape) {
        open_map_step(menu, config, beaches);
        return;
    }
    menu.seat = menu.seat.min(config.seats - 1);
    // Tab opens a name for typing, not Enter, which starts the match.
    if keys.just_pressed(KeyCode::Tab) {
        menu.naming = Some(menu.seat);
        return;
    }
    if menu_ui::enter(keys) || pad_start {
        start_match(config, menu, start, pads, settings, beaches);
        return;
    }
    if partner {
        pick_partners(menu, config, settings, pads);
        return;
    }
    let rows: Vec<u8> = seat_rows(setup_teams(settings, config), config.seats)
        .into_iter()
        .filter(|&seat| seat < config.seats)
        .collect();
    let at = rows.iter().position(|&seat| seat == menu.seat).unwrap_or(0);
    menu.seat = rows[menu_ui::nav(keys, at, rows.len())];
    if let Some(turn) = menu_ui::left_right(keys) {
        turn_seat(config, menu.seat, turn);
        pads.seats.table_set_by_hand();
    }
}

/// Step 3's rows in the order they are shown, every seat a table can
/// have: under a team mode team by team, as the line-up lists them
/// ([`crate::app::lineup::seat_order`]), else in seat order.
pub fn seat_rows(mode: crate::app::teams::TeamMode, seats: u8) -> Vec<u8> {
    match mode {
        crate::app::teams::TeamMode::Solo => (0..MAX_PLAYERS as u8).collect(),
        crate::app::teams::TeamMode::Pairs | crate::app::teams::TeamMode::Trios => {
            crate::app::lineup::seat_order(mode, seats)
                .into_iter()
                .map(|(seat, _)| seat)
                .collect()
        }
    }
}

/// Why P2's seat stays where it is when partners are picked: the seat is
/// wired to the keyboard's second keys, or to the controller settings name
/// for P2, so a person moved into it would be driven by those, not by
/// their own pad. Whoever plays it (P2, the AI, nobody yet) stays.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum P2Pin {
    /// P2 plays it at the keyboard.
    Keys,
    /// P2 plays it on controller `n`, counted from 0 as settings count
    /// them.
    Pad(u8),
    /// Nobody plays it at the keys yet (the AI, or nobody), and it is
    /// kept for them.
    KeptForKeys,
    /// Controller `n` does not play it (the AI does, or the pad is not
    /// plugged in), and it is kept for that pad.
    KeptForPad(u8),
}

/// P2's pin at `config`'s table ([`P2Pin`]), read off step 3's own row
/// for the seat ([`played_by`], with `pads` and `p2_here` as it takes
/// them), so the line and the rows agree. A row naming the keys or a
/// controller pins the seat whoever plays it. A row left to whichever pad
/// pins it once P2's keys were pressed, and while the seat is a person's
/// nobody sits in: the keyboard's second player takes it with one key,
/// and would push a friend picked into it out.
pub fn p2_pin(
    config: &MatchConfig,
    settings: &GameSettings,
    pads: &[SeatPad],
    p2_here: bool,
) -> Option<P2Pin> {
    if config.seats < 2 {
        return None;
    }
    let by = played_by(config, 1, pads, p2_here, settings);
    let seated = matches!(by, PlayedBy::Keys(_) | PlayedBy::Pad { .. });
    let keys = match seated {
        true => P2Pin::Keys,
        false => P2Pin::KeptForKeys,
    };
    match settings.seat_input[1] {
        SeatInput::Keys => Some(keys),
        SeatInput::Pad(n) if seated => Some(P2Pin::Pad(n)),
        SeatInput::Pad(n) => Some(P2Pin::KeptForPad(n)),
        SeatInput::Auto if p2_here => Some(keys),
        SeatInput::Auto => (by == PlayedBy::Open { keys: true }).then_some(P2Pin::KeptForKeys),
    }
}

/// The seats a couch pick never moves, besides P1's: P2's when pinned.
fn pinned_seats(pin: Option<P2Pin>) -> Vec<u8> {
    pin.map(|_| 1).into_iter().collect()
}

/// The partner pick for the seat under step 3's cursor: the next team it
/// can be given ([`crate::app::partners::next_pick`]), made by swapping
/// seats ([`swap_seats`]). The cursor follows the picker when the pick
/// moves them to a seat beside a partner who cannot move.
fn pick_partners(
    menu: &mut MatchMenu,
    config: &mut MatchConfig,
    settings: &GameSettings,
    pads: &mut SetupPads,
) {
    let mode = setup_teams(settings, config);
    let pin = p2_pin(
        config,
        settings,
        &pads.seated(config, settings),
        pads.seats.p2_at_keys(),
    );
    let pinned = pinned_seats(pin);
    let Some(pick) = crate::app::partners::next_pick(
        mode,
        config.seats,
        menu.seat,
        (&[0], &pinned),
        &config.people.0,
    ) else {
        return;
    };
    for &pair in &pick.swaps {
        swap_seats(config, settings, pads, pair);
    }
    crate::app::partners::reseated(&mut config.people.0, &pick);
    menu.seat = pick.moved(menu.seat);
}

/// The people in seats `a` and `b` trade places: who plays each (a
/// person, the AI, a bot), the name it goes by ([`MatchConfig::people`]),
/// and the pad that drives it ([`PadSeats::swap_seats`]).
pub fn swap_seats(
    config: &mut MatchConfig,
    settings: &GameSettings,
    pads: &mut SetupPads,
    (a, b): (u8, u8),
) {
    let live: Vec<Entity> = pads.live.iter().collect();
    pads.seats
        .swap_seats(&live, settings, &config.human_seats(), (a, b));
    let (a, b) = (usize::from(a), usize::from(b));
    config.controllers.swap(a, b);
    config.filled_at_start.swap(a, b);
}

/// The line under step 3's seats about teams, for the seat under the
/// cursor, if there is one to say: under a team mode the partner key and
/// whose partners it picks, or why P2 cannot be moved when that holds a
/// pick back; under a mode the table does not fit, the scoring row's own
/// words for it ([`crate::app::teams::row_value`]).
pub fn partner_line(
    tr: &crate::app::i18n::Tr,
    device: LastDevice,
    (config, settings): (&MatchConfig, &GameSettings),
    seat: u8,
    pin: Option<P2Pin>,
) -> Option<String> {
    use crate::app::teams::TeamMode;
    let set = settings.team_mode;
    let mode = setup_teams(settings, config);
    let names = config.seat_names(&settings.names);
    let name = |seat: u8| crate::app::name_or_label(&names, tr, seat);
    if mode == TeamMode::Solo {
        return (set != TeamMode::Solo)
            .then(|| crate::app::teams::row_value(tr, set, config.seats, true));
    }
    let (_, held) = crate::app::partners::picks(mode, config.seats, seat, &[0], &pinned_seats(pin));
    if held && let Some(pin) = pin {
        let n = |n: u8| (n + 1).to_string();
        return Some(match pin {
            P2Pin::Keys => fill(tr.setup_pinned_keys, &[("p", &name(1))]),
            P2Pin::Pad(pad) => fill(tr.setup_pinned_pad, &[("p", &name(1)), ("n", &n(pad))]),
            P2Pin::KeptForKeys => fill(tr.setup_kept_keys, &[("p", &name(1))]),
            P2Pin::KeptForPad(pad) => fill(tr.setup_kept_pad, &[("p", &name(1)), ("n", &n(pad))]),
        });
    }
    let face = crate::app::gamepad::Face::MatchSetup {
        step: Step::Players,
        join: false,
        partners: true,
    };
    let key = crate::app::gamepad::scheme::spelled(
        tr,
        device,
        &face,
        crate::app::gamepad::scheme::Does::Partners,
        &crate::app::gamepad::scheme::PARTNER_CAP.to_string(),
    );
    let hint = match mode {
        TeamMode::Pairs => tr.setup_partner_hint,
        TeamMode::Solo | TeamMode::Trios => tr.setup_teammates_hint,
    };
    Some(fill(hint, &[("k", &key), ("p", &name(seat))]))
}

/// Start match: once the bots are in, the table launched
/// ([`launch_the_table`]). Bots first: a match never starts with a seat
/// nothing is driving. Nothing is set for the match until it does start:
/// a doorway card backed out of leaves the open seats open and the Random
/// card undrawn, as step 3 showed them.
fn start_match(
    config: &mut MatchConfig,
    menu: &MatchMenu,
    start: &mut MatchStart,
    pads: &mut SetupPads,
    settings: &GameSettings,
    beaches: &CustomBeaches,
) {
    if start.bots_in(config, settings.tr()) {
        launch_the_table(config, menu, start, pads, settings, beaches);
    }
}

/// The match goes on: the open seats to the AI, the map drawn or the
/// previewed seed kept ([`seat_the_match`]), and the round. Where Start
/// match lands with the bots in, and where the doorway card does once the
/// last one comes in.
fn launch_the_table(
    config: &mut MatchConfig,
    menu: &MatchMenu,
    start: &mut MatchStart,
    pads: &mut SetupPads,
    settings: &GameSettings,
    beaches: &CustomBeaches,
) {
    let open = open_seats(
        config,
        &pads.seated(config, settings),
        pads.seats.p2_at_keys(),
        settings,
    );
    seat_the_match(
        config,
        &open,
        menu.seed,
        beaches,
        crate::app::clock::fresh_seed(),
    );
    if !open.is_empty() {
        pads.seats.table_set_by_hand();
    }
    start.launch(config);
}

/// The keyboard belongs to one name, the one saved on settings row `row`: characters land in it, Backspace
/// rubs one out, Enter or Esc hands the keyboard back. Bevy's `text` field
/// is used rather than the key codes so the player's own layout, and their
/// shift key, decide what a keystroke means.
fn type_a_name(
    row: u8,
    typed: &mut MessageReader<bevy::input::keyboard::KeyboardInput>,
    keys: &ButtonInput<KeyCode>,
    settings: &mut GameSettings,
    menu: &mut MatchMenu,
) {
    use crate::app::typing::{Keystroke, keystrokes};
    let ends = [
        KeyCode::Enter,
        KeyCode::NumpadEnter,
        KeyCode::Escape,
        KeyCode::Tab,
    ];
    for stroke in keystrokes(typed, &ends) {
        match stroke {
            Keystroke::Erase => settings.pop_name_char(row),
            Keystroke::Char(ch) => settings.push_name_char(row, ch),
            // The finish is decided below from just_pressed, one branch
            // for every way out.
            Keystroke::Done(_) => {}
        }
    }
    // Every way out of the name box puts it away and nothing else: Enter
    // does not also start the match here, or a player finishing a name
    // would find the round already running.
    let done = menu_ui::enter(keys)
        || keys.just_pressed(KeyCode::Escape)
        || keys.just_pressed(KeyCode::Tab);
    if done {
        settings.tidy_name(row);
        menu.naming = None;
    }
}
