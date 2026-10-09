//! The match-setup screen: the rows, the keys that move them, and the seat
//! names typed into them.
//!
//! Split from the model the way settings is, so the dials and their wire
//! form stay readable without the UI wrapped around them.

use super::*;
use crate::app::SeatController;
use crate::app::i18n::fill;
use crate::app::menu_ui;
use crate::app::settings::GameSettings;

#[derive(Resource, Default)]
pub struct MatchMenu {
    pub selected: usize,
    /// The seat whose name is being typed, if any. While this is set the
    /// keyboard belongs to the name: navigation and launching wait.
    pub naming: Option<u8>,
    /// Whether a second player has shown up at the keyboard, by pressing
    /// one of P2's own keys. The pads say so by pressing Start
    /// ([`crate::app::gamepad::PadSeats`]); the keyboard's second seat
    /// had no way to, so nothing could tell two people sharing it from
    /// one person who set the table for two.
    pub p2_here: bool,
    /// The `PINCH_LAUNCH` hook has pressed its Enter.
    pub launched: bool,
}

/// Whether player 1 would have anybody to play against: an AI in the
/// match, or a second person who has joined, by a pad's Start or by
/// pressing P2's keys. Whoever is working this menu is player 1.
///
/// A two-seat table with no AI used to start with nobody else there, and
/// the round played out against an empty chair.
pub(super) fn has_an_opponent(config: &MatchConfig, pads_joined: usize, p2_here: bool) -> bool {
    config.bots > 0 || pads_joined > 0 || p2_here
}

/// Whether this frame's keys include one of P2's own: a key bound to the
/// second keyboard seat that neither player 1 nor the menu also uses, so
/// the person working the menu cannot join as P2 by accident.
pub(super) fn p2_pressed(keys: &ButtonInput<KeyCode>, settings: &GameSettings) -> bool {
    use crate::app::cursor::{KeyMap, keymap};
    let all = |map: KeyMap| {
        let KeyMap {
            moves,
            places,
            remove,
            clear_all,
        } = map;
        moves
            .into_iter()
            .map(|(key, ..)| key)
            .chain(places.into_iter().map(|(key, _)| key))
            .chain([remove, clear_all])
            .collect::<Vec<KeyCode>>()
    };
    let menu = [
        KeyCode::Enter,
        KeyCode::NumpadEnter,
        KeyCode::Escape,
        KeyCode::Tab,
        KeyCode::Backspace,
    ];
    let p1 = all(keymap(settings, 0, settings.commit));
    all(keymap(settings, 1, settings.commit))
        .into_iter()
        .filter(|key| !p1.contains(key) && !menu.contains(key))
        .any(|key| keys.just_pressed(key))
}

/// A row of the card, by its index. What the row *is* rides on its cells:
/// the row node itself only ever folds away or takes the cursor's band.
#[derive(Component)]
pub struct MatchRow(pub usize);

/// One half of a row's text: the dial's name, or what it is set to.
#[derive(Component)]
pub struct MatchCell(pub usize, pub Row, pub menu_ui::Half);

#[derive(Component)]
pub struct MatchUi;

/// The controller-join footer: hint plus the joined list.
#[derive(Component)]
pub struct MatchPadInfo(pub bool);

/// The line under the card saying why the map dial is offering none of the
/// player's own beaches. Empty the rest of the time, and spawned either
/// way, so the footer below it does not shift when it appears.
#[derive(Component)]
pub struct MatchBeachNote;

pub fn enter_match_setup(
    mut commands: Commands,
    settings: Res<GameSettings>,
    mut menu: ResMut<MatchMenu>,
    mut config: ResMut<MatchConfig>,
    beaches: Res<CustomBeaches>,
) {
    menu.selected = 0;
    menu.naming = None;
    menu.p2_here = false;
    // The shelf may have changed since the last visit (a beach deleted, or
    // resaved with fewer castles): a config still pointing at it is moved
    // on, so the dial reads what will be played.
    settle_map(&mut config, &beaches);
    let tr = settings.tr();
    commands
        .spawn((
            MatchUi,
            Node {
                row_gap: Val::Px(FOOTER_GAP),
                ..menu_ui::between_bars()
            },
        ))
        .with_children(|wrap| {
            // A fixed height, because the rows that come and go fold away
            // rather than blanking: a card that shrinks around them jumps
            // while you turn the dial that adds them.
            let (mark, mut node, fill, edge, shadow) = menu_ui::screen_card();
            node.row_gap = Val::Px(ROW_GAP);
            node.height = Val::Px(card_height());
            node.justify_content = JustifyContent::FlexStart;
            wrap.spawn(Node {
                // A column, so the card's height is the axis it shrinks
                // along, and free to shrink itself: held to its content,
                // this line kept a full table's card at full height on a
                // short window and ran it under the header.
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                min_height: Val::Px(0.0),
                ..default()
            })
            .with_children(|line| {
                line.spawn((mark, node, fill, edge, shadow))
                    .with_children(|card| {
                        menu_ui::heading_row(card, tr.match_heading, None);
                        for row in 0..ROWS {
                            card.spawn((MatchRow(row), menu_ui::card_row()))
                                .with_children(|line| {
                                    for (half, width) in [
                                        (menu_ui::Half::Label, LABEL_W),
                                        (menu_ui::Half::Value, VALUE_W),
                                    ] {
                                        line.spawn((
                                            MatchCell(row, Row::ALL[row], half),
                                            menu_ui::cell(width, ROW_FONT),
                                        ));
                                    }
                                });
                        }
                    });
            });
            // The beach note sits first, right under the card and so under
            // the map row it is about, and in the parchment the rows use
            // rather than the controller footer's grey: it is about the
            // choice being made, not a standing hint.
            wrap.spawn((
                MatchBeachNote,
                // A row of its own height whether or not it has anything to
                // say: an empty text node collapses, and the controller
                // footer under it would step up the screen every time the
                // seat count crossed what the beaches can hold.
                Node {
                    height: Val::Px(20.0),
                    ..default()
                },
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(menu_ui::type_scale::BODY),
                    ..default()
                },
                TextLayout::no_wrap(),
                TextColor(palette::PARCHMENT.with_alpha(0.65)),
            ));
            for is_list in [false, true] {
                wrap.spawn((
                    MatchPadInfo(is_list),
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(menu_ui::type_scale::BODY),
                        ..default()
                    },
                    TextLayout::no_wrap(),
                    TextColor(footer_ink()),
                    // Clear, until the line is the warning that the match
                    // cannot start: that one has to be read, and faint
                    // parchment on bright sand is not.
                    Node {
                        padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
                        border_radius: BorderRadius::all(Val::Px(9.0)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ));
            }
        });
}

/// Tall enough for every row at once, each at the height it lays out at.
///
/// A full table of six with five of them AI shows every row there is. On a
/// window too short for that the card shrinks to the room between the bars
/// (see [`menu_ui::screen_card`]) and its rows close up, down to
/// [`menu_ui::ROW_H`]; on one with room it keeps them at their own height.
/// Sized for rows at their least instead, a full table ran its last row
/// onto the frame on every screen, the room unused.
pub(super) fn card_height() -> f32 {
    2.0 * menu_ui::CARD_PAD
        + menu_ui::HEADING_H
        + ROWS as f32 * (menu_ui::row_pitch(ROW_FONT) + ROW_GAP)
}

/// Label gutter and value gutter, the value including the `< >` the dial
/// wears. Two cells, the way every other card in the shell does it, rather
/// than one cell holding a label padded out with spaces.
///
/// Padding as `{:<15}` counts the width in `char`s, and two scripts break
/// that. The Latin ones overrun it ("Niveau van de AI S1" is nineteen
/// characters) and push the dial right on those rows alone. Japanese
/// undershoots it and still comes out wider: the shipped CJK face draws a
/// full em where DejaVu Sans Mono draws 0.602, and no number of spaces
/// closes a gap that is 1.66 of one.
///
/// Wide enough for the longest label and the longest built-in value in any
/// language, measured against those two faces. What the player types - a
/// seat name, a handmade beach's name - is not bounded by anything here and
/// clips, which is the same contract the settings card keeps.
pub(super) const LABEL_W: f32 = 220.0;
pub(super) const VALUE_W: f32 = 428.0;
/// Tighter than the shell's gap: a full table of six shows every row the
/// card has, and at the shell's gap they reach into the header bar once
/// the arrows dial joined them. Only the cursor's band shows the seam.
pub(super) const ROW_GAP: f32 = 0.0;

/// The gap between the card and each line under it, closer than the
/// shell's ten: a full table's card at 720p needs the room for its own
/// padding inside the wood.
pub(super) const FOOTER_GAP: f32 = 4.0;
/// The scale's row size, named rather than repeated: it happened to be
/// the same number already, which is not the same as saying so.
pub(super) const ROW_FONT: f32 = menu_ui::type_scale::ROW;

/// The controller footer's usual ink: a quiet hint under the card.
fn footer_ink() -> Color {
    palette::PARCHMENT.with_alpha(0.40)
}

/// A footer line under the card: its words, their ink, and the pill that
/// turns the warning gold.
type FooterLine = (
    &'static MatchPadInfo,
    &'static mut Text,
    &'static mut TextColor,
    &'static mut BackgroundColor,
);

/// Keep the controller footer current: the join hint, and who joined -
/// and the note about beaches this table has grown too big for.
pub fn update_match_pad_info(
    seats: Res<crate::app::gamepad::PadSeats>,
    menu: Res<MatchMenu>,
    config: Res<MatchConfig>,
    settings: Res<GameSettings>,
    beaches: Res<CustomBeaches>,
    mut rows: Query<FooterLine>,
    mut note: Query<&mut Text, (With<MatchBeachNote>, Without<MatchPadInfo>)>,
) {
    let tr = settings.tr();
    if let Ok(mut text) = note.single_mut() {
        let line = beaches_note(&config, tr, &beaches).unwrap_or_default();
        menu_ui::set_text(&mut text, &line);
    }
    let humans = config.seats - config.bots;
    let alone = !has_an_opponent(&config, seats.0.len(), menu.p2_here);
    for (info, mut text, mut ink, mut pill) in &mut rows {
        let warning = !info.0 && alone;
        let (want_ink, want_pill) = if warning {
            (palette::GOLD, palette::PILL_FILL)
        } else {
            (footer_ink(), Color::NONE)
        };
        menu_ui::set_color(&mut ink, want_ink);
        menu_ui::set_bg(&mut pill, want_pill);
        let line = if info.0 {
            if seats.0.is_empty() {
                String::new()
            } else {
                // Pad claim i drives the i-th human seat from the top.
                let list: Vec<String> = (0..seats.0.len())
                    .map(|i| {
                        let seat = humans.saturating_sub(1 + i as u8);
                        crate::app::seat_label(tr, seat)
                    })
                    .collect();
                fill(tr.match_pad_joined, &[("list", &list.join(", "))])
            }
        } else if warning {
            // In the join hint's place: it is the same news, a seat
            // waiting for somebody, with the other way to fill it.
            tr.match_needs_opponent.to_string()
        } else {
            tr.match_pad_hint.to_string()
        };
        menu_ui::set_text(&mut text, &line);
    }
}

/// The seat the `slot`-th AI row configures: the AI fills the top seats, so
/// slot 0 is the highest one. `None` once the slot is past the AI count.
pub(super) fn ai_seat(config: &MatchConfig, slot: u8) -> Option<u8> {
    (slot < config.bots).then(|| config.seats - 1 - slot)
}

/// What a seat that is not played here can be, in the order its dial
/// steps through them: the AI at each difficulty, then a bot.
const OPPONENTS: [SeatController; 4] = [
    SeatController::Ai(BotLevel::Easy),
    SeatController::Ai(BotLevel::Normal),
    SeatController::Ai(BotLevel::Hard),
    SeatController::Bot,
];

/// Step what holds the seat behind `slot`: easier or fiercer AI, or a bot
/// connected over the network. A dead slot (the row is hidden) changes
/// nothing.
pub(super) fn cycle_ai_level(config: &mut MatchConfig, slot: u8, turn: Turn) {
    if let Some(seat) = ai_seat(config, slot) {
        let now = config.controllers[usize::from(seat)];
        let at = OPPONENTS.iter().position(|c| *c == now).unwrap_or(1);
        let next = crate::app::cycle::dial(at as u8, turn, 1, 0..=OPPONENTS.len() as u8 - 1);
        config.controllers[usize::from(seat)] = OPPONENTS[usize::from(next)];
    }
}

/// The seats a bot holds in the match as set up.
pub fn bot_seat_list(config: &MatchConfig) -> Vec<u8> {
    (0..config.seats)
        .filter(|&seat| config.controller(seat) == SeatController::Bot)
        .collect()
}

/// Which rows are showing right now: the AI-level rows appear one per AI
/// seat, so a two-human match has none of them.
pub(super) fn live_rows(config: &MatchConfig) -> [bool; ROWS] {
    std::array::from_fn(|row| match Row::ALL[row] {
        Row::BotLevel(slot) => ai_seat(config, slot).is_some(),
        Row::Name(seat) => seat < config.seats,
        Row::Players | Row::Bots | Row::Map | Row::Gulls | Row::Round | Row::Posts | Row::Mode => {
            true
        }
    })
}

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
        use crate::app::bot_seats::{Doorway, Waiting};
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
        self.bots.waiting = Some(Waiting::Couch);
        false
    }
}

pub fn match_setup_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut menu: ResMut<MatchMenu>,
    dials: Dials,
    mut settings: ResMut<GameSettings>,
    start: MatchStart,
    pads: Res<crate::app::gamepad::PadSeats>,
) {
    let Dials {
        mut config,
        beaches,
    } = dials;
    let mut start = start;
    // A doorway card is up: its keys are its own, and the match goes on
    // the moment the last bot is in.
    if start.bots.waiting.is_some() {
        if start
            .bots
            .door
            .as_ref()
            .is_some_and(crate::app::bot_seats::Doorway::ready)
        {
            start.bots.waiting = None;
            start.bots.feedback.clear();
            start.launch(&mut config);
        }
        typed.clear();
        return;
    }
    if let Some(seat) = menu.naming {
        type_a_name(seat, &mut typed, &keys, &mut settings, &mut menu);
        return;
    }
    // Nothing typed while not naming, but the reader must still be drained
    // or the first keystroke of a rename arrives with a backlog.
    typed.clear();
    if keys.just_pressed(KeyCode::Escape) {
        start.next_screen.set(Screen::Menu);
        return;
    }
    // Tab opens a name for typing, not Enter: on a name row Enter could
    // then never do the other thing it says it does, and the name rows are
    // last on the list, so a player who had just named everyone would press
    // Enter to start and get the name box again.
    if keys.just_pressed(KeyCode::Tab)
        && let Row::Name(seat) = Row::ALL[menu.selected]
    {
        menu.naming = Some(seat);
        return;
    }
    if p2_pressed(&keys, &settings) {
        menu.p2_here = true;
    }
    // The dev hook's Enter, pressed once for an unattended run.
    let hooked = crate::app::dev::launch() && !std::mem::replace(&mut menu.launched, true);
    if menu_ui::enter(&keys) || hooked {
        // Nobody to play: the footer already says what to do about it.
        if !has_an_opponent(&config, pads.0.len(), menu.p2_here) {
            return;
        }
        // Bots first: a match never starts with a seat nothing is driving.
        if start.bots_in(&config, settings.tr()) {
            start.launch(&mut config);
        }
        return;
    }
    menu.selected = menu_ui::nav_live(&keys, menu.selected, &live_rows(&config));
    let Some(turn) = menu_ui::left_right(&keys) else {
        return;
    };
    match Row::ALL[menu.selected] {
        Row::Players => {
            config.seats = crate::app::cycle::dial(config.seats, turn, 1, 2..=MAX_PLAYERS as u8);
            config.bots = config.bots.min(config.seats - 1);
            // A generated beach follows the table, so two players do not
            // rattle around a beach sized for six. Five and six castles need
            // a beach with room for them, and a handmade beach only seats as
            // many as it has castles.
            if config.map.is_a_plain_size() {
                config.map = sized_for(config.seats);
            }
            settle_map(&mut config, &beaches);
        }
        Row::Bots => {
            config.bots = crate::app::cycle::dial(config.bots, turn, 1, 0..=config.seats - 1);
        }
        Row::BotLevel(slot) => cycle_ai_level(&mut config, slot, turn),
        Row::Map => {
            cycle_map(&mut config, turn, &beaches);
            // Stepping down to a small beach drops the seats it cannot hold
            // rather than starting a match six players cannot all sit at.
            if config.map.size().0 < WIDE_ENOUGH {
                config.seats = config.seats.min(CLASSIC_SEATS);
                config.bots = config.bots.min(config.seats - 1);
            }
        }
        Row::Gulls => config.gulls = config.gulls.cycled(turn),
        Row::Round => config.round = config.round.cycled(turn),
        Row::Posts => config.posts = crate::app::cycle::dial(config.posts, turn, 1, POSTS_RANGE),
        Row::Mode => config.series = config.series.cycled(turn),
        // A name is typed, not stepped through.
        Row::Name(_) => {}
    }
}

/// The keyboard belongs to one seat's name: characters land in it, Backspace
/// rubs one out, Enter or Esc hands the keyboard back. Bevy's `text` field
/// is used rather than the key codes so the player's own layout, and their
/// shift key, decide what a keystroke means.
fn type_a_name(
    seat: u8,
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
            Keystroke::Erase => settings.pop_name_char(seat),
            Keystroke::Char(ch) => settings.push_name_char(seat, ch),
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
        settings.tidy_name(seat);
        menu.naming = None;
    }
}

/// The two halves of a row: the dial's name, and what it is set to.
///
/// Pure, and separate from the system below, so every language's rows can
/// be measured against the cells that hold them without a `World`. The
/// value half carries its own `< >`, since the one row that is typed into
/// rather than stepped through wears neither.
pub(super) fn row_text(
    tr: &crate::app::i18n::Tr,
    config: &MatchConfig,
    settings: &GameSettings,
    beaches: &CustomBeaches,
    naming: Option<u8>,
    row: Row,
) -> (String, String) {
    let dial = |value: &str| format!("< {value} >");
    match row {
        Row::Players => (
            tr.match_players.to_string(),
            dial(&config.seats.to_string()),
        ),
        Row::Bots => {
            let humans = config.seats - config.bots;
            let rest = if humans == 1 {
                tr.human_one.to_string()
            } else {
                fill(tr.human_many, &[("n", &humans.to_string())])
            };
            (
                tr.match_ai.to_string(),
                format!("{}   ({rest})", dial(&config.bots.to_string())),
            )
        }
        Row::BotLevel(slot) => {
            let seat = ai_seat(config, slot).unwrap_or(0);
            let who = crate::app::seat_label(tr, seat);
            let value = match config.controller(seat) {
                SeatController::Bot => tr.match_bot,
                SeatController::Ai(_) | SeatController::Local | SeatController::Remote => {
                    tr.bot_levels[config.level(seat).index()]
                }
            };
            (format!("{} {who}", tr.match_ai_level), dial(value))
        }
        // What the dial is *not* offering rides under the card, not here:
        // the value is one fixed-width cell, and the sentence runs off the
        // end of it in every language.
        Row::Map => (
            tr.match_map.to_string(),
            dial(&map_label(config, tr, beaches)),
        ),
        Row::Gulls => (
            tr.match_gulls.to_string(),
            dial(tr.gull_names[config.gulls.index()]),
        ),
        Row::Round => (
            tr.match_round.to_string(),
            dial(tr.round_names[config.round.index()]),
        ),
        Row::Posts => (tr.match_posts.to_string(), dial(&config.posts.to_string())),
        Row::Mode => (
            tr.match_mode.to_string(),
            dial(tr.mode_names[config.series.index()]),
        ),
        Row::Name(seat) => {
            let who = crate::app::seat_label(tr, seat);
            let given = settings.names[usize::from(seat)].clone();
            let value = if naming == Some(seat) {
                // A caret says the keyboard is being typed into rather than
                // navigating; the row is its own instructions.
                format!("{given}_   {}", tr.match_name_typing)
            } else if given.is_empty() {
                tr.match_name_empty.to_string()
            } else {
                given
            };
            (format!("{} {who}", tr.match_name), value)
        }
    }
}

pub fn update_match_ui(
    config: Res<MatchConfig>,
    menu: Res<MatchMenu>,
    settings: Res<GameSettings>,
    beaches: Res<CustomBeaches>,
    mut cells: Query<(&MatchCell, &mut Text, &mut TextColor)>,
    mut rows: Query<(&MatchRow, &mut BackgroundColor, &mut Node)>,
) {
    let tr = settings.tr();
    // One source of truth for which rows apply right now, shared with the
    // navigation: a row the cursor cannot reach must not be on screen.
    let live = live_rows(&config);
    for (cell, mut text, mut color) in &mut cells {
        if !live[cell.0] {
            continue;
        }
        let (label, value) = row_text(tr, &config, &settings, &beaches, menu.naming, cell.1);
        let half = match cell.2 {
            menu_ui::Half::Label => label,
            menu_ui::Half::Value => value,
        };
        menu_ui::set_text(&mut text, &half);
        menu_ui::set_color(
            &mut color,
            if cell.0 == menu.selected {
                Color::WHITE
            } else {
                palette::PARCHMENT.with_alpha(0.80)
            },
        );
    }
    for (row, mut fill, mut node) in &mut rows {
        // A row that does not apply leaves no trace: it folds out of the
        // column entirely. The card is a fixed height, so the ones that
        // remain do not move when it does.
        menu_ui::set_shown(&mut node, live[row.0]);
        let ground = menu_ui::band(row.0 == menu.selected && live[row.0]);
        menu_ui::set_bg(&mut fill, ground);
    }
}
