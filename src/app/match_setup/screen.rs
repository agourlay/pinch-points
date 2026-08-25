//! The match-setup screen: the rows, the keys that move them, and the seat
//! names typed into them.
//!
//! Split from the model the way settings is, so the dials and their wire
//! form stay readable without the UI wrapped around them.

use super::*;
use crate::app::company;
use crate::app::i18n::fill;
use crate::app::menu_ui;
use crate::app::settings::GameSettings;

#[derive(Resource, Default)]
pub struct MatchMenu {
    pub selected: usize,
    /// The seat whose name is being typed, if any. While this is set the
    /// keyboard belongs to the name: navigation and launching wait.
    pub naming: Option<u8>,
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

/// The company this card keeps: hung wide, because the match card is more
/// than twice the width of the language picker's and its shoulders reach
/// out further again. Gulls in the sky above, crabs on the sand below,
/// none of them over the card.
pub(super) const FLOCK: [company::Perch; 4] = [
    crate::app::company::Perch::gull(0.025, 0.06, 60.0),
    crate::app::company::Perch::gull(0.885, 0.10, 54.0),
    crate::app::company::Perch::crab(0.02, 0.72, 64.0),
    crate::app::company::Perch::crab(0.89, 0.78, 58.0),
];

pub fn enter_match_setup(
    mut commands: Commands,
    settings: Res<GameSettings>,
    art: Res<crate::app::art::Art>,
    mut menu: ResMut<MatchMenu>,
    mut config: ResMut<MatchConfig>,
    beaches: Res<CustomBeaches>,
) {
    menu.selected = 0;
    menu.naming = None;
    // The shelf may have changed since the last visit (a beach deleted, or
    // resaved with fewer castles): a config still pointing at it is moved
    // on, so the dial reads what will be played.
    crate::app::match_setup::settle_map(&mut config, &beaches);
    let tr = settings.tr();
    commands
        .spawn((MatchUi, menu_ui::between_bars()))
        .with_children(|wrap| {
            // The flock first, so it sits behind the card rather than on it.
            company::flock(wrap, &art, &FLOCK);
            // A fixed height, because the rows that come and go fold away
            // rather than blanking: four AI-level rows in the middle of the
            // card would be four holes if they merely emptied, and a card
            // that shrinks around them jumps while you turn the dial that
            // adds them.
            let (mut node, fill, edge, shadow) = menu_ui::screen_card();
            node.height = Val::Px(
                2.0 * menu_ui::CARD_PAD_Y
                    + menu_ui::HEADING_H
                    + ROWS as f32 * (menu_ui::ROW_H + menu_ui::ROW_GAP),
            );
            node.justify_content = JustifyContent::FlexStart;
            // The card between the crab and the gull, so it stays centred
            // on its own rows rather than on the crab.
            wrap.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(company::CRITTER_GAP),
                ..default()
            })
            .with_children(|line| {
                company::shoulder(line, &art, crate::app::company::Company::Crab, 0.0);
                line.spawn((node, fill, edge, shadow))
                    .with_children(|card| {
                        card.spawn(menu_ui::heading(tr.match_heading, true));
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
                company::shoulder(line, &art, crate::app::company::Company::Gull, 1.7);
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
                    font_size: FontSize::Px(15.0),
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
                        font_size: FontSize::Px(15.0),
                        ..default()
                    },
                    TextLayout::no_wrap(),
                    TextColor(palette::PARCHMENT.with_alpha(0.40)),
                ));
            }
        });
}

/// Label gutter and value gutter, the value including the `< >` the dial
/// wears. Two cells, the way every other card in the shell does it, rather
/// than one cell holding a label padded out with spaces.
///
/// Padding was `{:<15}`, and Rust counts that width in `char`s. Two scripts
/// break that. The Latin ones overrun it - "Niveau van de AI S1" is
/// nineteen characters - and pushed the dial right on those rows alone.
/// Japanese undershoots it and still comes out wider: the shipped CJK face
/// draws a full em where DejaVu Sans Mono draws 0.602, so a two-character
/// label padded to fifteen is 17 columns wide and a six-character one is
/// 21, and no number of spaces closes a gap that is 1.66 of one. A cell of
/// a fixed pixel width has no opinion about either.
///
/// Wide enough for the longest label and the longest built-in value in any
/// language, measured against those two faces. What the player types - a
/// seat name, a handmade beach's name - is not bounded by anything here and
/// clips, which is the same contract the settings card keeps.
pub(super) const LABEL_W: f32 = 220.0;
pub(super) const VALUE_W: f32 = 428.0;
pub(super) const ROW_FONT: f32 = 19.0;

/// Keep the controller footer current: the join hint, and who joined -
/// and the note about beaches this table has grown too big for.
pub fn update_match_pad_info(
    seats: Res<crate::app::gamepad::PadSeats>,
    config: Res<MatchConfig>,
    settings: Res<GameSettings>,
    beaches: Res<CustomBeaches>,
    mut rows: Query<(&MatchPadInfo, &mut Text)>,
    mut note: Query<&mut Text, (With<MatchBeachNote>, Without<MatchPadInfo>)>,
) {
    let tr = settings.tr();
    if let Ok(mut text) = note.single_mut() {
        let line = crate::app::match_setup::beaches_note(&config, tr, &beaches).unwrap_or_default();
        menu_ui::set_text(&mut text, &line);
    }
    let humans = config.seats - config.bots;
    for (info, mut text) in &mut rows {
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

/// Step the difficulty of the AI seat behind `slot`. A dead slot (the row
/// is hidden) changes nothing.
pub(super) fn cycle_ai_level(config: &mut MatchConfig, slot: u8, turn: Turn) {
    if let Some(seat) = ai_seat(config, slot) {
        config.bot_levels[seat as usize] = config.bot_levels[seat as usize].cycled(turn);
    }
}

/// Which rows are showing right now: the AI-level rows appear one per AI
/// seat, so a two-human match has none of them.
pub(super) fn live_rows(config: &MatchConfig) -> [bool; ROWS] {
    std::array::from_fn(|row| match Row::ALL[row] {
        Row::BotLevel(slot) => ai_seat(config, slot).is_some(),
        Row::Name(seat) => seat < config.seats,
        Row::Players | Row::Bots | Row::Map | Row::Gulls | Row::Round | Row::Mode => true,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn match_setup_input(
    keys: Res<ButtonInput<KeyCode>>,
    beaches: Res<CustomBeaches>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut menu: ResMut<MatchMenu>,
    mut config: ResMut<MatchConfig>,
    mut settings: ResMut<GameSettings>,
    mut tournament: ResMut<crate::app::tournament::Tournament>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    if let Some(seat) = menu.naming {
        type_a_name(seat, &mut typed, &keys, &mut settings, &mut menu);
        return;
    }
    // Nothing typed while not naming, but the reader must still be drained
    // or the first keystroke of a rename arrives with a backlog.
    typed.clear();
    if keys.just_pressed(KeyCode::Escape) {
        next_screen.set(Screen::Menu);
        return;
    }
    // Tab opens a name for typing, not Enter. Enter used to do it, and on
    // a name row that meant Enter could never do the other thing it says
    // it does: the name rows are last on the list, so a player who had
    // just named everyone pressed Enter to start and got the name box
    // again, and again.
    if keys.just_pressed(KeyCode::Tab)
        && let Row::Name(seat) = Row::ALL[menu.selected]
    {
        menu.naming = Some(seat);
        return;
    }
    if menu_ui::enter(&keys) {
        config.armed = true;
        *tournament = if config.series.is_series() {
            crate::app::tournament::Tournament::start(config.series)
        } else {
            crate::app::tournament::Tournament::default()
        };
        next_screen.set(Screen::Versus);
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
            // Five and six castles need a beach with room for them, and a
            // handmade beach only seats as many as it has castles: the map
            // follows the table.
            crate::app::match_setup::settle_map(&mut config, &beaches);
        }
        Row::Bots => {
            config.bots = crate::app::cycle::dial(config.bots, turn, 1, 0..=config.seats - 1);
        }
        Row::BotLevel(slot) => cycle_ai_level(&mut config, slot, turn),
        Row::Map => {
            crate::app::match_setup::cycle_map(&mut config, turn, &beaches);
            // Stepping down to a small beach drops the seats it cannot hold
            // rather than starting a match six players cannot all sit at.
            if config.map.size().0 < WIDE_ENOUGH {
                config.seats = config.seats.min(CLASSIC_SEATS);
                config.bots = config.bots.min(config.seats - 1);
            }
        }
        Row::Gulls => config.gulls = config.gulls.cycled(turn),
        Row::Round => config.round = config.round.cycled(turn),
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
            (
                format!("{} {who}", tr.match_ai_level),
                dial(tr.bot_levels[config.bot_levels[seat as usize].index()]),
            )
        }
        // What the dial is *not* offering rides under the card, not here:
        // the value is one fixed-width cell, and the sentence runs off the
        // end of it in every language.
        Row::Map => (
            tr.match_map.to_string(),
            dial(&crate::app::match_setup::map_label(config, tr, beaches)),
        ),
        Row::Gulls => (
            tr.match_gulls.to_string(),
            dial(tr.gull_names[config.gulls.index()]),
        ),
        Row::Round => (
            tr.match_round.to_string(),
            dial(tr.round_names[config.round.index()]),
        ),
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
