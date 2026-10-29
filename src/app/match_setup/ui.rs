//! The setup steps on screen: one card per step under the step indicator,
//! built when its step comes up and painted every frame from the model in
//! [`steps`].

use super::*;
use crate::app::gamepad::LastDevice;
use crate::app::menu_ui;
use crate::app::settings::GameSettings;

/// Everything match setup puts on screen, under one root.
#[derive(Component)]
pub struct MatchUi;

/// The step the screen's card was built for, and on step 3 the order its
/// rows were built in ([`seat_rows`]). A step change builds the next one
/// in its place, and so does a change of order: a team mode coming into
/// play, or a table that fits another.
#[derive(Component)]
pub struct BuiltFor(Step, Vec<u8>);

/// The rows step 3 is built in for `config` under `settings`: team by
/// team under a team mode ([`seat_rows`]). Empty on the other steps.
fn built_rows(step: Step, config: &MatchConfig, settings: &GameSettings) -> Vec<u8> {
    match step {
        Step::Players => seat_rows(setup_teams(settings, config), config.seats),
        Step::Game | Step::Map => Vec::new(),
    }
}

/// Label and value gutters of step 1's rows, the value including the
/// `< >` the dial wears and the aside after it. Two cells, the way every
/// other card in the shell does it, rather than one cell holding a label
/// padded out with spaces: padding counts `char`s, and neither the Latin
/// labels that overrun it nor the Japanese that draws a full em a
/// character line up that way.
pub(super) const LABEL_W: f32 = 240.0;
/// The least air between a step 1 label and its value, which the label's
/// cell keeps clear: a label filling its cell sat against the dial's `<`.
pub(super) const LABEL_GAP: f32 = ASIDE_GAP;
pub(super) const VALUE_W: f32 = 428.0;
/// The type size of the rows, the scale's own.
pub(super) const ROW_FONT: f32 = menu_ui::type_scale::ROW;
/// The aside after a step 1 value, and the air before it.
pub(super) const ASIDE_FONT: f32 = menu_ui::type_scale::FINE;
pub(super) const ASIDE_GAP: f32 = 18.0;
/// The line under the rows of steps 1 and 2.
pub(super) const NOTE_FONT: f32 = 16.0;

/// Step 2's card and its tiles.
pub(super) const MAP_CARD_W: f32 = 1200.0;
/// The air between two cards, across and down.
pub(super) const TILE_GAP: f32 = 14.0;
const TILE_PAD: f32 = 8.0;
/// The room the grid's scroll bar keeps beside it, and as much again on
/// the other side so the grid stays in the middle of the card
/// ([`menu_ui::scroll_bar::beside`]).
const BAR_ROOM: f32 = 2.0 * (menu_ui::scroll_bar::BAR_GAP + menu_ui::scroll_bar::BAR_W);
/// A tile's width: the card's room inside its padding, its 1px edge and the
/// scroll bar's room, shared by the columns and the gaps between them,
/// rounded down so four always fit on a row.
pub(super) const TILE_W: f32 = ((MAP_CARD_W
    - 2.0 * menu_ui::CARD_PAD
    - 2.0
    - BAR_ROOM
    - (CARD_COLUMNS as f32 - 1.0) * TILE_GAP)
    / CARD_COLUMNS as f32) as u32 as f32;
pub(super) const THUMB_W: f32 = TILE_W - 2.0 * TILE_PAD;
/// A card's picture's height: [`THUMB_W`] over it is the shape the
/// pictures are drawn in ([`preview::PREVIEW_PX`]), so a board is
/// not stretched.
pub(super) const THUMB_H: f32 = 148.0;
/// A card's whole height: the picture, its padding and edge, the name
/// under it and the size line. Set rather than left to the layout, so the
/// page's model ([`MapPage`]) knows where every row sits.
pub(super) const TILE_H: f32 = THUMB_H
    + 2.0 * TILE_PAD
    + 2.0 * 2.0
    + NAME_GAP
    + (CARD_NAME_FONT + CARD_SUB_FONT) * menu_ui::LINE_HEIGHT
    + 2.0;
/// The air between a card's picture and its name.
const NAME_GAP: f32 = 8.0;
/// A section's heading, with the air under it before its first row.
pub(super) const HEADING_H: f32 = 26.0;
pub(super) const HEADING_FONT: f32 = menu_ui::type_scale::BODY;
/// The air between a section's last row and the next section's heading:
/// a row's gap, so stepping a row down a section boundary scrolls by
/// whole rows and headings.
pub(super) const SECTION_GAP: f32 = TILE_GAP;
/// The window step 2's page is seen through: two rows, each under its
/// section's heading, so a section of one row shows whole beside the next.
pub(super) const VIEW_H: f32 = 2.0 * (HEADING_H + TILE_H) + TILE_GAP;
pub(super) const CARD_NAME_FONT: f32 = 18.0;
pub(super) const CARD_SUB_FONT: f32 = 14.0;
pub(super) const BADGE_FONT: f32 = 13.0;
pub(super) const PILL_FONT: f32 = 16.0;

/// Step 3's card and the columns of its seats.
pub(super) const SEATS_CARD_W: f32 = 960.0;
const CHIP_W: f32 = 92.0;
pub(super) const NAME_W: f32 = 210.0;
const ICON_W: f32 = 30.0;
pub(super) const BY_W: f32 = 440.0;
pub(super) const STATUS_W: f32 = 130.0;
pub(super) const STATUS_FONT: f32 = 16.0;
/// An open seat's call to a friend, a size down from the rows so the
/// longest still fits its cell.
pub(super) const OPEN_FONT: f32 = 17.0;
pub(super) const SEAT_HINT_FONT: f32 = menu_ui::type_scale::FINE;
/// A team's heading on step 3, the line-up's size.
pub(super) const TEAM_HEADING_FONT: f32 = crate::app::lineup::SUB_FONT;
/// The pill buttons' words.
pub(super) const BUTTON_FONT: f32 = 20.0;
/// Step 3's Start match button, wide enough for its words and its key in
/// every language ([`start_button_label`]).
pub(super) const START_BUTTON_W: f32 = 340.0;
const SEAT_PITCH: f32 = 50.0;

/// One of step 1's rows, by index into [`GameRow::ALL`].
#[derive(Component)]
pub struct GameRowUi(usize);

/// The three texts of a step 1 row.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum GamePart {
    Label,
    Value,
    Aside,
}

#[derive(Component)]
pub struct GameCell(usize, GamePart);

/// The line under the rows, saying what the row (or card) under the
/// cursor is for.
#[derive(Component)]
pub struct StepNote;

/// One of step 2's tiles, by index into [`map_slots`].
#[derive(Component)]
pub struct TileUi(usize);

impl TileUi {
    /// The tile's place, an index into [`map_slots`].
    pub fn index(&self) -> usize {
        self.0
    }
}

/// One of step 2's section headings.
#[derive(Component)]
pub struct SectionHeading(MapSection);

impl SectionHeading {
    pub fn section(&self) -> MapSection {
        self.0
    }
}

/// Step 2's page inside its window, moved up by the scroll.
#[derive(Component)]
pub struct MapPageUi;

/// Step 2's scroll bar, beside the page: the window's share of the page
/// and how far down it is ([`MapPage::window`]).
#[derive(Component)]
pub struct MapBar;

/// The parts of a tile that change.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum TilePart {
    Thumb,
    Badge,
    Name,
    Sub,
    /// The "seats N, not M" pill over a greyed tile, and its words.
    Pill,
    PillText,
    /// One quarter of the Random card's mosaic.
    Quarter(usize),
    /// One quarter of the Surprise card's: a beach in one of its styles.
    Sample(usize),
}

#[derive(Component)]
pub struct TileCell(usize, TilePart);

/// One of step 3's seats.
#[derive(Component)]
pub struct SeatRowUi(u8);

/// The parts of a seat row that change.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum SeatPart {
    Name,
    By,
    Status,
    KeysIcon,
    PadIcon,
    RobotIcon,
}

#[derive(Component)]
pub struct SeatCell(u8, SeatPart);

/// The line under step 3's seats saying the seat count moved the map
/// since step 2 ([`moved_note`]), on a line of its own, up only while it
/// did. Said in the hint's place, it hid what A/D does for the whole visit.
#[derive(Component)]
pub struct SeatMoved;

/// The line under that: what A/D does to the seat under the cursor.
#[derive(Component)]
pub struct SeatHint;

/// The line under it: the open seats, or how a friend joins.
#[derive(Component)]
pub struct SeatsLine;

/// A team's heading over its rows on step 3, by team number.
#[derive(Component)]
pub struct TeamHeadingUi(u8);

/// The line under step 3's hint about teams ([`partner_line`]).
#[derive(Component)]
pub struct PartnerLine;

/// The words on step 3's Start match button, which name its key.
#[derive(Component)]
pub struct StartButtonText;

/// Into match setup: the table checked against the shelf, P2's keys
/// forgotten, P1 the pad the hands last used were on if they were on one
/// ([`crate::app::gamepad::PadSeats::setup_opened`]), and the seed the
/// map cards are drawn with drawn. The step it opens on is the one it was
/// left on reset to the first ([`leave_match_setup`]), or the one a dev
/// hook asked for.
pub fn enter_match_setup(
    mut menu: ResMut<MatchMenu>,
    mut config: ResMut<MatchConfig>,
    beaches: Res<CustomBeaches>,
    mut pads: ResMut<crate::app::gamepad::PadSeats>,
    hands: (Res<LastDevice>, Res<crate::app::gamepad::LastPad>),
) {
    pads.setup_opened(crate::app::gamepad::hands_at_the_menu(*hands.0, *hands.1));
    menu.naming = None;
    menu.online = false;
    menu.seed = crate::app::clock::fresh_seed();
    settle(&mut config, &beaches);
    if menu.step == Step::Map {
        menu.scroll = 0.0;
        put_the_cursor_on_the_pick(&mut menu, &config, &beaches);
    }
}

/// Out of match setup: the next visit opens on step 1.
pub fn leave_match_setup(mut menu: ResMut<MatchMenu>) {
    menu.step = Step::Game;
    menu.naming = None;
}

/// What the cards are painted from.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Paint<'w> {
    menu: Res<'w, MatchMenu>,
    config: Res<'w, MatchConfig>,
    settings: Res<'w, GameSettings>,
    device: Res<'w, LastDevice>,
    beaches: Res<'w, CustomBeaches>,
    art: Res<'w, crate::app::art::Art>,
}

/// Build the step's card when the step changes, and the first time.
pub fn update_match_ui(
    mut commands: Commands,
    paint: Paint,
    built: Query<(Entity, &BuiltFor), With<MatchUi>>,
) {
    let step = paint.menu.step;
    let rows = built_rows(step, &paint.config, &paint.settings);
    let mut up_to_date = false;
    for (root, built) in &built {
        if built.0 == step && built.1 == rows {
            up_to_date = true;
        } else {
            commands.entity(root).despawn();
        }
    }
    if up_to_date {
        return;
    }
    spawn_step_card(&mut commands, &paint, step);
}

/// The lobby host's map page ([`MatchMenu::online`]): step 2's card, up
/// while the host picks and gone once it is not, over the lobby. The
/// same card as the couch's, built by the same function.
pub fn update_host_map_ui(
    mut commands: Commands,
    paint: Paint,
    built: Query<Entity, With<MatchUi>>,
    mut titles: Query<&mut Text, With<HostMapTitle>>,
) {
    let up = paint.menu.online && paint.menu.step == Step::Map;
    match (up, built.is_empty()) {
        (true, true) => spawn_step_card(&mut commands, &paint, Step::Map),
        (false, false) => {
            for root in &built {
                commands.entity(root).despawn();
            }
        }
        (true, false) | (false, true) => {}
    }
    // The table it picks for can shrink under it, between rounds.
    if up {
        let title = host_map_title(paint.settings.tr(), paint.config.seats);
        for mut text in &mut titles {
            if text.0 != title {
                text.0.clone_from(&title);
            }
        }
    }
}

/// The host's map page's heading, rewritten when the table it names
/// changes under it.
#[derive(Component)]
pub struct HostMapTitle;

/// One step's card, under the step indicator; the lobby host's map page
/// has a heading naming the table instead, there being no other steps.
fn spawn_step_card(commands: &mut Commands, paint: &Paint, step: Step) {
    let tr = paint.settings.tr();
    let mut root = commands.spawn((
        MatchUi,
        BuiltFor(step, built_rows(step, &paint.config, &paint.settings)),
        Node {
            row_gap: Val::Px(4.0),
            ..menu_ui::between_bars()
        },
    ));
    // The host's page stands over whatever it is opened from: the lobby,
    // or the round's results card between the rounds of a series.
    if paint.menu.online {
        root.insert(GlobalZIndex(menu_ui::layer::CARD));
    }
    root.with_children(|wrap| {
        let (mark, mut node, fill, edge, shadow) = menu_ui::screen_card();
        node.align_items = AlignItems::Stretch;
        node.width = Val::Px(match step {
            Step::Game => LABEL_W + VALUE_W + 20.0 + 2.0 * menu_ui::CARD_PAD,
            Step::Map => MAP_CARD_W,
            Step::Players => SEATS_CARD_W,
        });
        wrap.spawn((mark, node, fill, edge, shadow))
            .with_children(|card| {
                match paint.menu.online {
                    true => host_map_heading(card, tr, paint.config.seats),
                    false => stepper(card, tr, step),
                }
                match step {
                    Step::Game => game_card(card, tr),
                    Step::Map => map_card(card, paint),
                    Step::Players => seats_card(card, tr, paint),
                }
            });
    });
}

/// The lobby host's map page's heading, where the couch has its step
/// indicator: the table the map is picked for.
fn host_map_heading(card: &mut ChildSpawnerCommands, tr: &crate::app::i18n::Tr, seats: u8) {
    card.spawn((
        HostMapTitle,
        Text::new(host_map_title(tr, seats)),
        menu_ui::display_font(HOST_MAP_TITLE_FONT),
        TextColor(palette::GOLD),
        Node {
            margin: UiRect::new(Val::Px(6.0), Val::Px(4.0), Val::Px(0.0), Val::Px(10.0)),
            ..default()
        },
    ));
}

/// The size of the lobby host's map page heading, the step names' own.
pub(super) const HOST_MAP_TITLE_FONT: f32 = 17.0;

/// The lobby host's map page heading's words: the table it picks for.
pub fn host_map_title(tr: &crate::app::i18n::Tr, seats: u8) -> String {
    fill(tr.host_map_title, &[("n", &seats.to_string())])
}

/// Text in the rows' face at `px`, on one line.
fn mono(text: impl Into<String>, px: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(px),
            ..default()
        },
        TextLayout::no_wrap(),
        TextColor(color),
    )
}

/// A gold hairline, `alpha` strong, as wide as its row.
fn hairline(alpha: f32) -> impl Bundle {
    (
        Node {
            height: Val::Px(1.0),
            margin: UiRect::vertical(Val::Px(8.0)),
            ..default()
        },
        BackgroundColor(palette::GOLD.with_alpha(alpha)),
    )
}

/// The card's heading row grown into the step indicator: a medal disc and
/// a name per step, joined by the heading's gold hairline. The step being
/// set is gold and underlined, the ones done parchment, the ones to come
/// driftwood.
fn stepper(card: &mut ChildSpawnerCommands, tr: &crate::app::i18n::Tr, at: Step) {
    card.spawn(Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        margin: UiRect::new(Val::Px(4.0), Val::Px(4.0), Val::Px(0.0), Val::Px(10.0)),
        ..default()
    })
    .with_children(|row| {
        for (index, step) in Step::ALL.into_iter().enumerate() {
            if index > 0 {
                row.spawn((
                    Node {
                        flex_grow: 1.0,
                        height: Val::Px(1.0),
                        margin: UiRect::horizontal(Val::Px(12.0)),
                        ..default()
                    },
                    BackgroundColor(palette::CARD_EDGE),
                ));
            }
            let (disc, digit, ink) = match step.index().cmp(&at.index()) {
                std::cmp::Ordering::Less => (
                    palette::PARCHMENT,
                    palette::MEDAL_DIGIT,
                    palette::PARCHMENT.with_alpha(0.75),
                ),
                std::cmp::Ordering::Equal => {
                    (palette::MEDAL_GOLD, palette::MEDAL_DIGIT, palette::GOLD)
                }
                std::cmp::Ordering::Greater => (
                    palette::MEDAL_DRIFTWOOD,
                    Color::srgb(0.84, 0.84, 0.86),
                    palette::IDLE_ROW.with_alpha(0.85),
                ),
            };
            let current = step == at;
            row.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                padding: UiRect::bottom(Val::Px(4.0)),
                border: UiRect::bottom(Val::Px(if current { 1.5 } else { 0.0 })),
                ..default()
            })
            .insert(BorderColor::all(palette::GOLD.with_alpha(0.85)))
            .with_children(|name| {
                medal(name, disc, digit, &(index + 1).to_string(), 22.0);
                name.spawn((
                    Text::new(tr.setup_steps[index]),
                    menu_ui::display_font(17.0),
                    TextColor(ink),
                ));
            });
        }
    });
}

/// A medal disc `size` across in `fill`, with `digit` on it.
fn medal(parent: &mut ChildSpawnerCommands, fill: Color, ink: Color, digit: &str, size: f32) {
    parent
        .spawn((
            Node {
                width: Val::Px(size),
                height: Val::Px(size),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(size / 2.0)),
                ..default()
            },
            BackgroundColor(fill),
        ))
        .with_children(|disc| {
            disc.spawn((
                Text::new(digit),
                menu_ui::display_font(size * 0.64),
                TextColor(ink),
            ));
        });
}

/// A pill-shaped button: gold-edged and lit, the way Enter goes. `mark`
/// goes on its words, for a label painted afterwards.
fn button(
    parent: &mut ChildSpawnerCommands,
    label: String,
    width: f32,
    align: AlignSelf,
    mark: impl Bundle,
) {
    parent
        .spawn((
            Node {
                width: Val::Px(width),
                height: Val::Px(40.0),
                align_self: align,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(20.0)),
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(palette::GOLD.with_alpha(0.30)),
            BorderColor::all(palette::GOLD.with_alpha(0.9)),
        ))
        .with_children(|pill| {
            pill.spawn((
                Text::new(label),
                menu_ui::display_font(BUTTON_FONT),
                TextColor(palette::GOLD),
                mark,
            ));
        });
}

// --- step 1 ------------------------------------------------------------------

fn game_card(card: &mut ChildSpawnerCommands, tr: &crate::app::i18n::Tr) {
    for (index, _) in GameRow::ALL.iter().enumerate() {
        card.spawn((GameRowUi(index), menu_ui::card_row()))
            .with_children(|row| {
                // The label clips short of its column, so the air before
                // the value stays whatever the label says.
                let (mut node, text, font, layout, ink) =
                    menu_ui::cell(LABEL_W - LABEL_GAP, ROW_FONT);
                node.margin = UiRect::right(Val::Px(LABEL_GAP));
                row.spawn((
                    GameCell(index, GamePart::Label),
                    node,
                    text,
                    font,
                    layout,
                    ink,
                ));
                row.spawn(Node {
                    width: Val::Px(VALUE_W),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip_x(),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|value| {
                    value.spawn((
                        GameCell(index, GamePart::Value),
                        mono("", ROW_FONT, palette::PARCHMENT),
                    ));
                    value.spawn((
                        GameCell(index, GamePart::Aside),
                        mono("", ASIDE_FONT, palette::PARCHMENT.with_alpha(0.45)),
                        Node {
                            margin: UiRect::left(Val::Px(ASIDE_GAP)),
                            ..default()
                        },
                    ));
                });
            });
    }
    card.spawn(hairline(0.18));
    card.spawn((
        StepNote,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(NOTE_FONT),
            ..default()
        },
        TextColor(palette::PARCHMENT.with_alpha(0.65)),
        Node {
            // Two lines' room whichever row is picked, so the button
            // under it holds still.
            min_height: Val::Px(2.0 * NOTE_FONT * menu_ui::LINE_HEIGHT),
            max_width: Val::Px(LABEL_W + VALUE_W),
            margin: UiRect::horizontal(Val::Px(10.0)),
            ..default()
        },
    ));
    button(
        card,
        format!("{}  >", tr.setup_next),
        250.0,
        AlignSelf::FlexEnd,
        (),
    );
}

/// Paint step 1: each row's words, the cursor's band, and the note.
pub fn paint_game_step(
    paint: Paint,
    mut rows: Query<(&GameRowUi, &mut BackgroundColor)>,
    mut cells: Query<(&GameCell, &mut Text, &mut TextColor), Without<StepNote>>,
    mut note: Query<&mut Text, With<StepNote>>,
) {
    if paint.menu.step != Step::Game {
        return;
    }
    let tr = paint.settings.tr();
    let at = paint.menu.game_row;
    for (row, mut fill) in &mut rows {
        menu_ui::set_bg(&mut fill, menu_ui::band(row.0 == at));
    }
    for (cell, mut text, mut ink) in &mut cells {
        let (label, value, aside) =
            game_row_text(tr, GameRow::ALL[cell.0], &paint.config, &paint.settings);
        let picked = cell.0 == at;
        let (words, color) = match cell.1 {
            GamePart::Label => (label.to_string(), row_ink(picked)),
            GamePart::Value => (value, row_ink(picked)),
            GamePart::Aside => (aside.to_string(), palette::PARCHMENT.with_alpha(0.45)),
        };
        menu_ui::set_text(&mut text, &words);
        menu_ui::set_color(&mut ink, color);
    }
    if let Ok(mut text) = note.single_mut() {
        menu_ui::set_text(&mut text, &game_row_note(tr, GameRow::ALL[at]));
    }
}

/// A row's ink: white under the cursor, parchment elsewhere.
fn row_ink(picked: bool) -> Color {
    match picked {
        true => Color::WHITE,
        false => palette::PARCHMENT.with_alpha(0.80),
    }
}

// --- step 2 ------------------------------------------------------------------

fn map_card(card: &mut ChildSpawnerCommands, paint: &Paint) {
    let slots = map_slots(&paint.beaches);
    let tr = paint.settings.tr();
    menu_ui::scroll_bar::beside(card, 0.0, MapBar, |column| {
        // The window, a heading and two rows tall, and the page inside it,
        // moved up by the scroll ([`paint_map_step`]).
        column
            .spawn(Node {
                height: Val::Px(VIEW_H),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|window| {
                window
                    .spawn((
                        MapPageUi,
                        Node {
                            flex_direction: FlexDirection::Column,
                            flex_shrink: 0.0,
                            row_gap: Val::Px(SECTION_GAP),
                            ..default()
                        },
                    ))
                    .with_children(|page| {
                        for section in MapSection::ALL {
                            map_section(page, tr, section, &slots);
                        }
                    });
            });
    });
    card.spawn((
        StepNote,
        mono("", NOTE_FONT, palette::PARCHMENT.with_alpha(0.65)),
        Node {
            margin: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(12.0), Val::Px(0.0)),
            ..default()
        },
    ));
}

/// One of step 2's sections: its heading over its cards, [`CARD_COLUMNS`]
/// to a row.
fn map_section(
    page: &mut ChildSpawnerCommands,
    tr: &crate::app::i18n::Tr,
    section: MapSection,
    slots: &[(MapSection, Slot)],
) {
    page.spawn(Node {
        flex_direction: FlexDirection::Column,
        flex_shrink: 0.0,
        ..default()
    })
    .with_children(|column| {
        column
            .spawn((
                SectionHeading(section),
                Node {
                    height: Val::Px(HEADING_H),
                    flex_shrink: 0.0,
                    align_items: AlignItems::FlexStart,
                    padding: UiRect::left(Val::Px(4.0)),
                    ..default()
                },
            ))
            .with_children(|heading| {
                heading.spawn((
                    Text::new(tr.setup_sections[section.index()]),
                    menu_ui::display_font(HEADING_FONT),
                    TextColor(palette::GOLD.with_alpha(0.75)),
                ));
            });
        column
            .spawn(Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(TILE_GAP),
                row_gap: Val::Px(TILE_GAP),
                ..default()
            })
            .with_children(|grid| {
                for (index, (of, slot)) in slots.iter().enumerate() {
                    if *of == section {
                        tile(grid, index, *slot);
                    }
                }
            });
    });
}

/// One map card: its picture, badge, name and size line.
fn tile(grid: &mut ChildSpawnerCommands, index: usize, slot: Slot) {
    grid.spawn((
        TileUi(index),
        Node {
            width: Val::Px(TILE_W),
            height: Val::Px(TILE_H),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(TILE_PAD)),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(10.0)),
            ..default()
        },
        BackgroundColor(Color::WHITE.with_alpha(0.05)),
        BorderColor::all(Color::NONE),
    ))
    .with_children(|tile| {
        tile.spawn(Node {
            width: Val::Px(THUMB_W),
            height: Val::Px(THUMB_H),
            border_radius: BorderRadius::all(Val::Px(6.0)),
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|thumb| {
            match slot {
                Slot::MakeOne => make_one_art(thumb),
                Slot::Card(Card::Random) => {
                    mosaic_art(thumb, index, TilePart::Quarter, MosaicMedal::Random);
                }
                Slot::Card(Card::Map(MapChoice::Surprise)) => {
                    mosaic_art(thumb, index, TilePart::Sample, MosaicMedal::Surprise);
                }
                Slot::Card(Card::Map(_) | Card::Shelf(_)) => {
                    thumb.spawn((
                        TileCell(index, TilePart::Thumb),
                        ImageNode::default().with_color(crate::app::materials::SAND),
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                    ));
                }
            }
            // The badge, top left over the picture.
            thumb
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(6.0),
                        top: Val::Px(6.0),
                        padding: UiRect::axes(Val::Px(7.0), Val::Px(2.0)),
                        border_radius: BorderRadius::all(Val::Px(9.0)),
                        ..default()
                    },
                    BackgroundColor(palette::PILL_FILL),
                ))
                .with_children(|badge| {
                    badge.spawn((
                        TileCell(index, TilePart::Badge),
                        mono("", BADGE_FONT, palette::PARCHMENT.with_alpha(0.85)),
                    ));
                });
            // "Seats N, not M", over a greyed picture.
            thumb
                .spawn((
                    TileCell(index, TilePart::Pill),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        right: Val::Px(0.0),
                        top: Val::Px(0.0),
                        bottom: Val::Px(0.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        display: Display::None,
                        ..default()
                    },
                ))
                .with_children(|middle| {
                    middle
                        .spawn((
                            Node {
                                padding: UiRect::axes(Val::Px(12.0), Val::Px(4.0)),
                                border_radius: BorderRadius::all(Val::Px(12.0)),
                                ..default()
                            },
                            BackgroundColor(palette::PILL_FILL),
                        ))
                        .with_children(|pill| {
                            pill.spawn((
                                TileCell(index, TilePart::PillText),
                                Text::new(""),
                                menu_ui::display_font(PILL_FONT),
                                TextColor(palette::PARCHMENT.with_alpha(0.85)),
                            ));
                        });
                });
        });
        // Each line in a cell that clips it, as step 1's values are: a
        // text node does not clip its own words, and a long beach name ran
        // on into the next card.
        tile.spawn(clip_cell(UiRect::new(
            Val::Px(2.0),
            Val::Px(0.0),
            Val::Px(NAME_GAP),
            Val::Px(0.0),
        )))
        .with_children(|cell| {
            cell.spawn((
                TileCell(index, TilePart::Name),
                mono("", CARD_NAME_FONT, palette::PARCHMENT),
            ));
        });
        tile.spawn(clip_cell(UiRect::left(Val::Px(2.0))))
            .with_children(|cell| {
                cell.spawn((
                    TileCell(index, TilePart::Sub),
                    mono("", CARD_SUB_FONT, palette::PARCHMENT.with_alpha(0.6)),
                ));
            });
    });
}

/// A cell as wide as a card's picture, past `margin`, that clips the line
/// inside it.
fn clip_cell(margin: UiRect) -> Node {
    Node {
        width: Val::Px(THUMB_W - NAME_INSET),
        margin,
        overflow: Overflow::clip_x(),
        ..default()
    }
}

/// How far a card's lines sit in from its picture's left edge.
pub(super) const NAME_INSET: f32 = 2.0;

/// The width DejaVu Sans Mono, the rows' face, draws each character it
/// carries at, in ems.
const MONO_EM: f32 = 0.602;

/// How wide the rows' face draws `ch`, in ems: [`MONO_EM`] for what
/// DejaVu carries, and a full em for the kana and kanji the Japanese face
/// draws behind it (`boot::teach_the_kanji_fallback`). Both faces are
/// monospace, so a line's width is the sum. `a_long_beach_name_is_cut_to_its_card`
/// holds both numbers to the shipped fonts' own bytes.
fn mono_em(ch: char) -> f32 {
    let wide = matches!(
        u32::from(ch),
        0x1100..=0x115F
            | 0x2E80..=0x303F
            | 0x3040..=0x30FF
            | 0x3100..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
    );
    match wide {
        true => 1.0,
        false => MONO_EM,
    }
}

/// A handmade beach's name as its card says it: whole when it fits the
/// card's picture, else cut short with "…" where it would run past it.
/// The one name a card, its note and step 3's line give a beach.
pub(super) fn card_name(name: &str) -> String {
    cut_to(name, THUMB_W - NAME_INSET, CARD_NAME_FONT)
}

/// How wide `text` draws at `font_px` in the interface's face, Japanese
/// falling back to its own as it does on screen ([`mono_em`]).
pub(crate) fn line_px(text: &str, font_px: f32) -> f32 {
    text.chars().map(mono_em).sum::<f32>() * font_px
}

/// `text` as it fits `room` pixels at `font_px` in the interface's face:
/// whole when it fits, else cut short with "…" where it would run past,
/// spaces before the "…" let go. The one cut a name too long for its
/// place is given, a beach's on its card or a player's on the line-up.
pub(crate) fn cut_to(text: &str, room: f32, font_px: f32) -> String {
    let room = room / font_px;
    let width: f32 = text.chars().map(mono_em).sum();
    if width <= room {
        return text.to_string();
    }
    let mut used = mono_em('…');
    let mut cut: String = text
        .chars()
        .take_while(|&ch| {
            used += mono_em(ch);
            used <= room
        })
        .collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

/// The picture of the card saying how to make a map: bare sand under a
/// driftwood medal with a plus on it, a beach still to be built.
fn make_one_art(thumb: &mut ChildSpawnerCommands) {
    thumb
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(crate::app::materials::SAND.darker(0.35)),
        ))
        .with_children(|middle| {
            medal(
                middle,
                palette::MEDAL_DRIFTWOOD,
                palette::MEDAL_DIGIT,
                "+",
                72.0,
            );
        });
}

/// The medal over a mosaic card's picture.
#[derive(Clone, Copy)]
enum MosaicMedal {
    /// Random's gold question mark: one of the maps on show.
    Random,
    /// Surprise's silver exclamation mark: a beach nobody has seen yet.
    Surprise,
}

/// A mosaic card's own picture: four pictures, darkened, under a medal.
/// Random borrows four of the other cards' pictures ([`MOSAIC`]), Surprise
/// shows a beach in each of four of its styles ([`SURPRISE_SAMPLES`]).
fn mosaic_art(
    thumb: &mut ChildSpawnerCommands,
    index: usize,
    part: fn(usize) -> TilePart,
    medal_of: MosaicMedal,
) {
    thumb
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            ..default()
        })
        .with_children(|mosaic| {
            for quarter in 0..4 {
                mosaic.spawn((
                    TileCell(index, part(quarter)),
                    ImageNode::default().with_color(crate::app::materials::SAND.darker(0.3)),
                    Node {
                        width: Val::Percent(50.0),
                        height: Val::Percent(50.0),
                        ..default()
                    },
                ));
            }
        });
    thumb
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|middle| {
            middle
                .spawn((
                    Node {
                        padding: UiRect::all(Val::Px(4.0)),
                        border: UiRect::all(Val::Px(2.0)),
                        border_radius: BorderRadius::all(Val::Px(50.0)),
                        ..default()
                    },
                    BorderColor::all(match medal_of {
                        MosaicMedal::Random => palette::GOLD.with_alpha(0.55),
                        MosaicMedal::Surprise => palette::MEDAL_SILVER.with_alpha(0.55),
                    }),
                ))
                .with_children(|ring| {
                    let (fill, mark) = match medal_of {
                        MosaicMedal::Random => (palette::MEDAL_GOLD, "?"),
                        MosaicMedal::Surprise => (palette::MEDAL_SILVER, "!"),
                    };
                    medal(ring, fill, palette::MEDAL_DIGIT, mark, 84.0)
                });
        });
}

/// What a tile reads.
pub(super) struct TileWords {
    pub badge: &'static str,
    pub name: String,
    pub sub: String,
    /// The "seats N, not M" pill, over a card the table cannot play.
    pub pill: Option<String>,
}

/// The words on `card`'s tile at a table of `seats`, `online` the lobby
/// host's pick, where a beach that cannot travel is greyed too.
pub(super) fn tile_words(
    tr: &crate::app::i18n::Tr,
    card: Card,
    seats: u8,
    beaches: &CustomBeaches,
    online: bool,
) -> TileWords {
    let range = card_seats(card, beaches);
    let span = |sub: &str, (w, h): (u8, u8)| {
        let (least, most) = range.unwrap_or((0, 0));
        fill(
            sub,
            &[
                ("w", &w.to_string()),
                ("h", &h.to_string()),
                ("a", &least.to_string()),
                ("b", &most.to_string()),
                ("n", &most.to_string()),
            ],
        )
    };
    let pill = match (card_fits(card, seats, beaches), range) {
        (true, _) if card_open(card, seats, beaches, online) => None,
        (true, _) => Some(tr.setup_local_only.to_string()),
        (false, Some(_)) => Some(fill(
            tr.setup_seats_not,
            &[("s", &seats_said(card, beaches)), ("n", &seats.to_string())],
        )),
        (false, None) => Some(tr.setup_seats_none.to_string()),
    };
    match card {
        Card::Map(MapChoice::Surprise) => TileWords {
            badge: tr.setup_badges[1],
            name: map_title(tr, MapChoice::Surprise),
            sub: tr.setup_surprise_sub.to_string(),
            pill,
        },
        Card::Map(map) => TileWords {
            badge: match map {
                MapChoice::Classic
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
                | MapChoice::BackToBack => tr.setup_badges[0],
                MapChoice::GenSmall
                | MapChoice::GenClassic
                | MapChoice::GenLarge
                | MapChoice::GenXl
                | MapChoice::GenOcean
                | MapChoice::Custom
                | MapChoice::Surprise => tr.setup_badges[1],
            },
            name: map_title(tr, map),
            // A map with no edges says so whatever its tables: crabs and
            // gulls walking off one side come back on the other, and
            // nothing else on the page says it.
            sub: match map.tables() {
                // The tables it is built for, as the pill and the note
                // say them: "2/4", never a range with an odd one inside.
                Some(_) => fill(
                    match map.wraps() {
                        true => tr.setup_map_sub_exact_wrap,
                        false => tr.setup_map_sub_exact,
                    },
                    &[
                        ("w", &map.size().0.to_string()),
                        ("h", &map.size().1.to_string()),
                        ("n", &seats_said(card, beaches)),
                    ],
                ),
                None => span(
                    match map.wraps() {
                        true => tr.setup_map_sub_wrap,
                        false => tr.setup_map_sub,
                    },
                    map.size(),
                ),
            },
            pill,
        },
        Card::Shelf(shelf) => {
            let beach = &beaches.0[shelf];
            let size = (beach.level.board().width(), beach.level.board().height());
            let mut sub = match range {
                Some(_) if card_lists_its_tables(card, beaches) => fill(
                    tr.setup_map_sub_exact,
                    &[
                        ("w", &size.0.to_string()),
                        ("h", &size.1.to_string()),
                        ("n", &seats_said(card, beaches)),
                    ],
                ),
                Some((least, most)) if least == most => span(tr.setup_map_sub_exact, size),
                Some(_) => span(tr.setup_map_sub, size),
                None => format!("{}x{}", size.0, size.1),
            };
            if beach.too_big_to_send() {
                sub = format!("{sub} · {}", tr.setup_local_only);
            }
            TileWords {
                badge: tr.setup_badges[2],
                name: card_name(&beach.level.name),
                sub,
                pill,
            }
        }
        Card::Random => TileWords {
            badge: tr.setup_badges[3],
            name: tr.setup_map_names[MapChoice::Custom.index()].to_string(),
            sub: fill(tr.setup_random_sub, &[("n", &seats.to_string())]),
            pill,
        },
    }
}

/// The words on place `slot`'s tile: a card's ([`tile_words`]), or the
/// player's own badge over a name and a line that send them to the editor.
pub(super) fn slot_words(
    tr: &crate::app::i18n::Tr,
    slot: Slot,
    seats: u8,
    beaches: &CustomBeaches,
    online: bool,
) -> TileWords {
    match slot {
        Slot::Card(card) => tile_words(tr, card, seats, beaches, online),
        Slot::MakeOne => TileWords {
            badge: tr.setup_badges[2],
            name: tr.setup_make_one.to_string(),
            sub: tr.setup_make_one_sub.to_string(),
            pill: None,
        },
    }
}

/// The line under step 2's cards for place `slot`: a card's
/// ([`card_note`]), or how to make a map of one's own, naming the menu's
/// editor entry and the editor's list of levels as they are named there.
pub(super) fn slot_note(
    tr: &crate::app::i18n::Tr,
    slot: Slot,
    config: &MatchConfig,
    beaches: &CustomBeaches,
    online: bool,
) -> String {
    use crate::app::menu_scene::MenuEntry;
    match slot {
        Slot::Card(card) => card_note(tr, card, config, beaches, online),
        Slot::MakeOne => {
            let editor = MenuEntry::ALL
                .iter()
                .position(|entry| *entry == MenuEntry::Driftwood)
                .map_or("", |entry| tr.menu_names[entry]);
            fill(
                tr.setup_make_one_note,
                &[("e", editor), ("l", tr.ed_levels_title)],
            )
        }
    }
}

/// The tables `card` seats, as the pill, the size line and the note say
/// them: the list a handmade map or a shelf beach names ("2/4"), else the
/// most it seats.
pub(super) fn seats_said(card: Card, beaches: &CustomBeaches) -> String {
    if card_lists_its_tables(card, beaches) {
        let tables: Vec<String> = card_tables(card, beaches)
            .iter()
            .map(u8::to_string)
            .collect();
        return tables.join("/");
    }
    card_seats(card, beaches).map_or_else(String::new, |(_, most)| most.to_string())
}

/// The line saying the seat count moved the map off the player's pick
/// ([`map_moved`]): the pick, the count it does not seat, and the map
/// played instead. `None` while the map in force is the pick.
pub fn moved_note(
    tr: &crate::app::i18n::Tr,
    config: &MatchConfig,
    beaches: &CustomBeaches,
) -> Option<String> {
    let (pick, now) = map_moved(config, beaches)?;
    let name = |card: Card| tile_words(tr, card, config.seats, beaches, false).name;
    Some(fill(
        tr.setup_map_moved,
        &[
            ("old", &name(pick)),
            ("new", &name(now)),
            ("n", &config.seats.to_string()),
        ],
    ))
}

/// The line under step 2's cards for the card under the cursor: what the
/// map is and how many it seats, or why a greyed one cannot be played,
/// or, on the map in force while the seat count keeps it off the pick,
/// that it moved ([`moved_note`]). `online`, the lobby host's pick: the
/// table is whoever turned up, so a greyed card names the table rather
/// than sending the host to a Players row it does not have, and a beach
/// that cannot travel says so.
pub(crate) fn card_note(
    tr: &crate::app::i18n::Tr,
    card: Card,
    config: &MatchConfig,
    beaches: &CustomBeaches,
    online: bool,
) -> String {
    let name = |card: Card| tile_words(tr, card, config.seats, beaches, online).name;
    if !online
        && card == picked_card(config, beaches)
        && let Some(note) = moved_note(tr, config, beaches)
    {
        return note;
    }
    let range = card_seats(card, beaches);
    let listed = card_lists_its_tables(card, beaches);
    let table = config.seats.to_string();
    if !card_fits(card, config.seats, beaches) {
        return match (range, listed, online) {
            (Some(_), true, false) => fill(
                tr.setup_greyed_tables,
                &[("m", &name(card)), ("s", &seats_said(card, beaches))],
            ),
            (Some((_, most)), false, false) => fill(
                tr.setup_greyed_note,
                &[("m", &name(card)), ("s", &most.to_string())],
            ),
            (Some(_), _, true) => fill(
                tr.host_map_greyed,
                &[
                    ("m", &name(card)),
                    ("s", &seats_said(card, beaches)),
                    ("n", &table),
                ],
            ),
            (None, _, _) => format!("{}: {}", name(card), tr.map_skips_castle),
        };
    }
    if !card_open(card, config.seats, beaches, online) {
        return fill(tr.host_map_too_big, &[("m", &name(card))]);
    }
    let seats = match range {
        Some(_) if listed => fill(tr.setup_seats_exact, &[("n", &seats_said(card, beaches))]),
        Some((least, most)) if least == most => {
            fill(tr.setup_seats_exact, &[("n", &most.to_string())])
        }
        Some((least, most)) => fill(
            tr.setup_seats_range,
            &[("a", &least.to_string()), ("b", &most.to_string())],
        ),
        None => String::new(),
    };
    let about = match card {
        Card::Map(MapChoice::Surprise) => {
            return format!(
                "{}: {}",
                name(card),
                fill(
                    tr.setup_surprise_note,
                    &[("n", &crate::sim::SURPRISE_LANES_MOST.to_string())]
                )
            );
        }
        Card::Map(map) => map_about(tr, map).to_string(),
        Card::Shelf(shelf) => match beaches.0[shelf].too_big_to_send() {
            true => format!("{} {}", tr.setup_custom_about, tr.setup_local_note),
            false => tr.setup_custom_about.to_string(),
        },
        Card::Random => {
            return format!(
                "{}: {}",
                name(card),
                fill(tr.setup_random_note, &[("n", &config.seats.to_string())])
            );
        }
    };
    format!("{}: {about} {seats}", name(card))
}

/// What step 2's tiles are painted from.
type TilePaint = (
    &'static TileCell,
    Option<&'static mut Text>,
    Option<&'static mut TextColor>,
    Option<&'static mut ImageNode>,
    Option<&'static mut Node>,
);

/// Step 2's tiles, and nothing else.
type Tiles = (
    &'static TileUi,
    &'static mut BackgroundColor,
    &'static mut BorderColor,
    &'static mut Visibility,
);

/// Step 2's headings and page, and nothing else with a node.
type PageParts = (
    Option<&'static SectionHeading>,
    Option<&'static mut Visibility>,
    &'static mut Node,
);

/// Step 2's headings and the page they sit on, and no tile or its parts.
type PageOnly = (
    Or<(With<SectionHeading>, With<MapPageUi>)>,
    Without<TileUi>,
    Without<TileCell>,
);

/// Paint step 2: the page scrolled to the cursor, with only the rows and
/// headings whole in the window shown, and each tile's picture, words,
/// pick and greying, and the note.
pub fn paint_map_step(
    paint: Paint,
    previews: Res<MapPreviews>,
    mut tiles: Query<Tiles, Without<TileCell>>,
    mut page_parts: Query<PageParts, PageOnly>,
    mut cells: Query<TilePaint, (Without<TileUi>, Without<StepNote>)>,
    mut note: Query<&mut Text, (With<StepNote>, Without<TileCell>)>,
) {
    if paint.menu.step != Step::Map {
        return;
    }
    let tr = paint.settings.tr();
    let config = &*paint.config;
    let slots = map_slots(&paint.beaches);
    let page = MapPage::of(&slots);
    let at = paint.menu.slot.min(slots.len() - 1);
    let scroll = paint.menu.scroll;
    let picked = Slot::Card(picked_card(config, &paint.beaches));
    let online = paint.menu.online;
    let fits = |slot: Slot| match slot {
        Slot::Card(card) => card_open(card, config.seats, &paint.beaches, online),
        Slot::MakeOne => false,
    };
    for (heading, visibility, mut node) in &mut page_parts {
        match (heading, visibility) {
            (Some(heading), Some(mut visibility)) => {
                set_visible(&mut visibility, page.heading_in_view(scroll, heading.0));
            }
            (Some(_), None) | (None, _) => {
                let top = Val::Px(-scroll);
                if node.margin.top != top {
                    node.margin.top = top;
                }
            }
        }
    }
    for (tile, mut fill, mut edge, mut visibility) in &mut tiles {
        let slot = slots[tile.0].1;
        let is_picked = slot == picked;
        set_visible(&mut visibility, page.slot_in_view(scroll, tile.0));
        menu_ui::set_bg(
            &mut fill,
            match is_picked {
                true => palette::PICKED_WASH,
                false => Color::WHITE.with_alpha(0.05),
            },
        );
        // The pick wears the cards' gold hairline at full strength; the
        // cursor resting on a card it cannot pick, a parchment one.
        let want = match (is_picked, tile.0 == at && !fits(slot)) {
            (true, _) => palette::GOLD,
            (false, true) => palette::PARCHMENT.with_alpha(0.6),
            (false, false) => Color::NONE,
        };
        if *edge != BorderColor::all(want) {
            *edge = BorderColor::all(want);
        }
    }
    // Worked out once a card, when one of its cells first asks: a card is
    // a dozen cells, and the words are formatted strings.
    let mut said: Vec<Option<TileWords>> = slots.iter().map(|_| None).collect();
    for (cell, text, ink, image, node) in &mut cells {
        let slot = slots[cell.0].1;
        // The card saying how to make a map reads as one to walk onto, not
        // greyed: there is nothing wrong with it, only nothing to pick.
        let lit = fits(slot) || slot == Slot::MakeOne;
        let words =
            match cell.1 {
                TilePart::Thumb | TilePart::Quarter(_) | TilePart::Sample(_) => None,
                TilePart::Pill
                | TilePart::PillText
                | TilePart::Badge
                | TilePart::Name
                | TilePart::Sub => Some(&*said[cell.0].get_or_insert_with(|| {
                    slot_words(tr, slot, config.seats, &paint.beaches, online)
                })),
            };
        let is_picked = slot == picked;
        match cell.1 {
            TilePart::Thumb => {
                let Slot::Card(card) = slot else {
                    continue;
                };
                if let (Some(mut image), Some(drawn)) = (image, previews.image(Pic::Card(card)))
                    && image.image != drawn
                {
                    image.image = drawn;
                    image.color = Color::WHITE;
                }
            }
            TilePart::Quarter(quarter) | TilePart::Sample(quarter) => {
                let pic = match matches!(cell.1, TilePart::Sample(_)) {
                    true => Pic::Sample(quarter),
                    false => Pic::Card(Card::Map(MOSAIC[quarter])),
                };
                if let (Some(mut image), Some(drawn)) = (image, previews.image(pic))
                    && image.image != drawn
                {
                    image.image = drawn;
                    // Darkened, so the medal reads over it.
                    image.color = Color::srgb(0.5, 0.48, 0.45);
                }
            }
            TilePart::Pill => {
                if let (Some(mut node), Some(words)) = (node, words) {
                    menu_ui::set_shown(&mut node, words.pill.is_some());
                }
            }
            TilePart::PillText | TilePart::Badge | TilePart::Name | TilePart::Sub => {
                let (Some(mut text), Some(mut ink), Some(words)) = (text, ink, words) else {
                    continue;
                };
                let (line, color) = match cell.1 {
                    TilePart::PillText => (
                        words.pill.clone().unwrap_or_default(),
                        palette::PARCHMENT.with_alpha(0.85),
                    ),
                    TilePart::Badge => (
                        words.badge.to_string(),
                        match slot {
                            Slot::Card(Card::Shelf(_)) | Slot::MakeOne => palette::GOLD,
                            Slot::Card(Card::Map(_) | Card::Random) => {
                                palette::PARCHMENT.with_alpha(0.85)
                            }
                        },
                    ),
                    TilePart::Name => (
                        words.name.clone(),
                        match (lit, is_picked) {
                            (false, _) => palette::PARCHMENT.with_alpha(0.30),
                            (true, true) => Color::WHITE,
                            (true, false) => palette::PARCHMENT.with_alpha(0.85),
                        },
                    ),
                    TilePart::Sub => (
                        words.sub.clone(),
                        palette::PARCHMENT.with_alpha(if lit { 0.6 } else { 0.25 }),
                    ),
                    TilePart::Thumb
                    | TilePart::Pill
                    | TilePart::Quarter(_)
                    | TilePart::Sample(_) => continue,
                };
                menu_ui::set_text(&mut text, &line);
                menu_ui::set_color(&mut ink, color);
            }
        }
    }
    if let Ok(mut text) = note.single_mut() {
        let line = slot_note(tr, slots[at].1, config, &paint.beaches, online);
        menu_ui::set_text(&mut text, &line);
    }
}

/// Show or hide a part of step 2's page, keeping its place in the layout:
/// the page is laid out whole and moved, and a part hidden must not move
/// the rest.
fn set_visible(visibility: &mut Mut<Visibility>, shown: bool) {
    let want = match shown {
        true => Visibility::Inherited,
        false => Visibility::Hidden,
    };
    if **visibility != want {
        **visibility = want;
    }
}

/// Keep step 2's scroll bar on the window's place down the page.
pub fn follow_map_bar(
    menu: Res<MatchMenu>,
    beaches: Res<CustomBeaches>,
    mut bars: menu_ui::scroll_bar::ScrollBars<MapBar>,
) {
    if menu.step == Step::Map {
        let page = MapPage::of(&map_slots(&beaches));
        bars.show(page.window(menu.scroll));
    }
}

// --- step 3 ------------------------------------------------------------------

/// Step 3's card: the column heads, a row for every seat a table can have
/// (those past the table folded), team by team under a team mode with
/// each team's heading over it, the lines under them and the button.
fn seats_card(card: &mut ChildSpawnerCommands, tr: &crate::app::i18n::Tr, paint: &Paint) {
    let art = &*paint.art;
    let mode = setup_teams(&paint.settings, &paint.config);
    // The column heads.
    card.spawn(Node {
        flex_direction: FlexDirection::Row,
        padding: UiRect::horizontal(Val::Px(10.0)),
        margin: UiRect::bottom(Val::Px(2.0)),
        ..default()
    })
    .with_children(|heads| {
        for (index, width) in [CHIP_W + 10.0, NAME_W + 8.0, BY_W].into_iter().enumerate() {
            heads.spawn((
                Text::new(tr.setup_columns[index]),
                menu_ui::display_font(13.0),
                TextColor(palette::GOLD.with_alpha(0.55)),
                Node {
                    width: Val::Px(width),
                    ..default()
                },
            ));
        }
    });
    let heads: Vec<(u8, Option<u8>)> = match mode {
        crate::app::teams::TeamMode::Solo => {
            (0..MAX_PLAYERS as u8).map(|seat| (seat, None)).collect()
        }
        crate::app::teams::TeamMode::Pairs | crate::app::teams::TeamMode::Trios => {
            crate::app::lineup::seat_order(mode, paint.config.seats)
        }
    };
    for (seat, heads) in heads {
        if let Some(team) = heads {
            card.spawn((
                TeamHeadingUi(team),
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(TEAM_HEADING_FONT),
                    ..default()
                },
                TextLayout::no_wrap(),
                TextColor(palette::PARCHMENT),
                Node {
                    margin: UiRect::new(
                        Val::Px(12.0),
                        Val::Px(10.0),
                        Val::Px(if team > 0 { 4.0 } else { 0.0 }),
                        Val::Px(0.0),
                    ),
                    overflow: Overflow::clip_x(),
                    ..default()
                },
            ));
        }
        card.spawn((
            SeatRowUi(seat),
            Node {
                height: Val::Px(SEAT_PITCH - 4.0),
                margin: UiRect::vertical(Val::Px(2.0)),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(10.0)),
                border_radius: BorderRadius::all(Val::Px(7.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|row| seat_row(row, seat, tr, art));
    }
    card.spawn(hairline(0.18));
    card.spawn((
        SeatMoved,
        mono("", SEAT_HINT_FONT, palette::GOLD),
        Node {
            margin: UiRect::horizontal(Val::Px(10.0)),
            display: Display::None,
            ..default()
        },
    ));
    card.spawn((
        SeatHint,
        mono("", SEAT_HINT_FONT, palette::PARCHMENT.with_alpha(0.55)),
        Node {
            margin: UiRect::horizontal(Val::Px(10.0)),
            ..default()
        },
    ));
    card.spawn((
        PartnerLine,
        mono("", SEAT_HINT_FONT, palette::PARCHMENT.with_alpha(0.55)),
        Node {
            margin: UiRect::horizontal(Val::Px(10.0)),
            display: Display::None,
            ..default()
        },
    ));
    card.spawn((
        SeatsLine,
        mono("", ROW_FONT - 2.0, palette::GOLD),
        Node {
            align_self: AlignSelf::Center,
            margin: UiRect::top(Val::Px(8.0)),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
            border_radius: BorderRadius::all(Val::Px(9.0)),
            min_height: Val::Px(25.0),
            ..default()
        },
        BackgroundColor(Color::NONE),
    ));
    button(
        card,
        String::new(),
        START_BUTTON_W,
        AlignSelf::Center,
        StartButtonText,
    );
}

/// One seat's row: its chip, name, who plays it and its status.
fn seat_row(
    row: &mut ChildSpawnerCommands,
    seat: u8,
    tr: &crate::app::i18n::Tr,
    art: &crate::app::art::Art,
) {
    // The chip, after the side panels' score chips: the seat's colour as
    // the fill, its paint as the edge, a medal disc with its number.
    row.spawn((
        Node {
            width: Val::Px(CHIP_W),
            height: Val::Px(34.0),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            padding: UiRect::left(Val::Px(6.0)),
            margin: UiRect::right(Val::Px(10.0)),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(9.0)),
            ..default()
        },
        BackgroundColor(chip_fill(seat)),
        BorderColor::all(palette::seat_paint(seat)),
    ))
    .with_children(|chip| {
        let disc = palette::leader_chip(seat);
        medal(
            chip,
            disc,
            palette::ink_on(disc),
            &(seat + 1).to_string(),
            22.0,
        );
        chip.spawn((
            Text::new(crate::app::seat_label(tr, seat)),
            menu_ui::display_font(19.0),
            TextColor(palette::name_ink_on(chip_fill(seat))),
        ));
    });
    row.spawn((
        SeatCell(seat, SeatPart::Name),
        menu_ui::cell(NAME_W, ROW_FONT),
    ));
    row.spawn(Node {
        width: Val::Px(8.0),
        flex_shrink: 0.0,
        ..default()
    });
    // The icon: the keys, a pad, or the robot the bot tag wears.
    row.spawn(Node {
        width: Val::Px(ICON_W),
        height: Val::Px(26.0),
        flex_shrink: 0.0,
        margin: UiRect::right(Val::Px(10.0)),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        ..default()
    })
    .with_children(|icon| {
        keys_icon(icon, seat);
        pad_icon(icon, seat);
        icon.spawn((
            SeatCell(seat, SeatPart::RobotIcon),
            ImageNode::new(art.robot.clone()).with_color(palette::GOLD.with_alpha(0.85)),
            Node {
                width: Val::Px(26.0),
                height: Val::Px(26.0),
                display: Display::None,
                ..default()
            },
        ));
    });
    row.spawn((
        SeatCell(seat, SeatPart::By),
        mono("", ROW_FONT, palette::PARCHMENT),
        Node {
            width: Val::Px(BY_W - ICON_W - 10.0),
            flex_shrink: 0.0,
            overflow: Overflow::clip_x(),
            ..default()
        },
    ));
    row.spawn((
        SeatCell(seat, SeatPart::Status),
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(STATUS_FONT),
            ..default()
        },
        TextColor(palette::INK_ONE_POST),
        Node {
            width: Val::Px(STATUS_W),
            flex_grow: 1.0,
            justify_content: JustifyContent::FlexEnd,
            ..default()
        },
        TextLayout::new(Justify::Right, LineBreak::NoWrap),
    ));
}

/// A seat chip's fill: the seat's colour, darkened for the white label on
/// it, and as far again as the label needs ([`palette::legible`]). Scaled
/// channel by channel, so every seat keeps its hue.
fn chip_fill(seat: u8) -> Color {
    let color = palette::player_color(seat).to_srgba();
    palette::legible(Color::srgb(
        color.red * 0.6,
        color.green * 0.6,
        color.blue * 0.6,
    ))
}

/// A keyboard drawn in the interface's own lines: a rounded case and two
/// rows of keys over a space bar.
fn keys_icon(icon: &mut ChildSpawnerCommands, seat: u8) {
    icon.spawn((
        SeatCell(seat, SeatPart::KeysIcon),
        Node {
            width: Val::Px(28.0),
            height: Val::Px(18.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: Val::Px(1.5),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            display: Display::None,
            ..default()
        },
        BorderColor::all(palette::GOLD.with_alpha(0.85)),
    ))
    .with_children(|case| {
        for row in 0..2 {
            case.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(1.5),
                ..default()
            })
            .with_children(|keys| {
                for _ in 0..(5 - row) {
                    keys.spawn((
                        Node {
                            width: Val::Px(2.5),
                            height: Val::Px(2.5),
                            ..default()
                        },
                        BackgroundColor(palette::GOLD.with_alpha(0.85)),
                    ));
                }
            });
        }
        case.spawn((
            Node {
                width: Val::Px(12.0),
                height: Val::Px(2.0),
                ..default()
            },
            BackgroundColor(palette::GOLD.with_alpha(0.85)),
        ));
    });
}

/// A controller in the same lines: a rounded body, a d-pad's cross and
/// four face buttons.
fn pad_icon(icon: &mut ChildSpawnerCommands, seat: u8) {
    let dot = |size: f32| {
        (
            Node {
                width: Val::Px(size),
                height: Val::Px(size),
                border_radius: BorderRadius::all(Val::Px(size / 2.0)),
                ..default()
            },
            BackgroundColor(palette::GOLD.with_alpha(0.85)),
        )
    };
    icon.spawn((
        SeatCell(seat, SeatPart::PadIcon),
        Node {
            width: Val::Px(30.0),
            height: Val::Px(18.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            padding: UiRect::horizontal(Val::Px(3.0)),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(8.0)),
            display: Display::None,
            ..default()
        },
        BorderColor::all(palette::GOLD.with_alpha(0.85)),
    ))
    .with_children(|body| {
        // The cross: a bar and a post through it.
        body.spawn(Node {
            width: Val::Px(7.0),
            height: Val::Px(7.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|cross| {
            cross.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(7.0),
                    height: Val::Px(2.0),
                    ..default()
                },
                BackgroundColor(palette::GOLD.with_alpha(0.85)),
            ));
            cross.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(2.0),
                    height: Val::Px(7.0),
                    ..default()
                },
                BackgroundColor(palette::GOLD.with_alpha(0.85)),
            ));
        });
        body.spawn(Node {
            width: Val::Px(8.0),
            height: Val::Px(8.0),
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            justify_content: JustifyContent::SpaceBetween,
            align_content: AlignContent::SpaceBetween,
            ..default()
        })
        .with_children(|buttons| {
            for _ in 0..4 {
                buttons.spawn(dot(3.0));
            }
        });
    });
}

/// What step 3 is painted from beyond the model: the pads seated.
#[derive(bevy::ecs::system::SystemParam)]
pub struct SeatsPaint<'w, 's> {
    pads: Res<'w, crate::app::gamepad::PadSeats>,
    live: Query<'w, 's, Entity, With<Gamepad>>,
}

/// What step 3's rows are painted with.
type SeatPaint = (
    &'static SeatCell,
    Option<&'static mut Text>,
    Option<&'static mut TextColor>,
    Option<&'static mut TextFont>,
    Option<&'static mut Node>,
    Option<&'static mut BorderColor>,
);

/// Step 3's hint, and nothing else that has text.
type HintLine = (With<SeatHint>, Without<SeatCell>, Without<SeatsLine>);

/// Step 3's moved-map line, and nothing else with text or a node.
type MovedLine = (
    With<SeatMoved>,
    Without<SeatHint>,
    Without<SeatCell>,
    Without<SeatsLine>,
    Without<SeatRowUi>,
);

/// The line under step 3's seats, and nothing else with a fill.
type SeatsLineOnly = (
    With<SeatsLine>,
    Without<SeatRowUi>,
    Without<SeatHint>,
    Without<SeatCell>,
);

/// Paint step 3: each seat's name, who plays it and its status, the rows
/// past the table folded, the cursor's band, and the lines under them.
pub fn paint_players_step(
    paint: Paint,
    seated: SeatsPaint,
    mut rows: Query<(&SeatRowUi, &mut BackgroundColor, &mut Node), Without<SeatCell>>,
    mut cells: Query<SeatPaint, (Without<SeatRowUi>, Without<SeatsLine>)>,
    mut hint: Query<(&mut Text, &mut TextColor), HintLine>,
    mut moved: Query<(&mut Text, &mut Node), MovedLine>,
    mut line: Query<(&mut Text, &mut BackgroundColor), SeatsLineOnly>,
) {
    let menu = &*paint.menu;
    if menu.step != Step::Players {
        return;
    }
    let tr = paint.settings.tr();
    let config = &*paint.config;
    let live: Vec<Entity> = seated.live.iter().collect();
    let pads = seated
        .pads
        .seat_pads(&live, &paint.settings, &config.human_seats());
    let at = menu.seat.min(config.seats - 1);
    for (row, mut fill, mut node) in &mut rows {
        menu_ui::set_shown(&mut node, row.0 < config.seats);
        menu_ui::set_bg(&mut fill, menu_ui::band(row.0 == at));
    }
    for (cell, text, ink, font, node, edge) in &mut cells {
        let seat = cell.0;
        if seat >= config.seats {
            continue;
        }
        let by = played_by(
            config,
            seat,
            &pads,
            seated.pads.p2_at_keys(),
            &paint.settings,
        );
        let picked = seat == at;
        match cell.1 {
            SeatPart::KeysIcon | SeatPart::PadIcon | SeatPart::RobotIcon => {
                let icon = match by {
                    PlayedBy::Keys(_) => SeatPart::KeysIcon,
                    PlayedBy::Pad { .. } | PlayedBy::Open { .. } => SeatPart::PadIcon,
                    PlayedBy::Ai(_) | PlayedBy::Bot => SeatPart::RobotIcon,
                };
                let shown = cell.1 == icon;
                if let Some(mut node) = node {
                    menu_ui::set_shown(&mut node, shown);
                }
                // A pad waiting for a friend is drawn faint.
                if let Some(mut edge) = edge {
                    let want = BorderColor::all(match by {
                        PlayedBy::Open { .. } => palette::PARCHMENT.with_alpha(0.35),
                        PlayedBy::Keys(_)
                        | PlayedBy::Pad { .. }
                        | PlayedBy::Ai(_)
                        | PlayedBy::Bot => palette::GOLD.with_alpha(0.85),
                    });
                    if *edge != want {
                        *edge = want;
                    }
                }
            }
            SeatPart::Name | SeatPart::By | SeatPart::Status => {
                let (Some(mut text), Some(mut ink)) = (text, ink) else {
                    continue;
                };
                let (words, color, px) = match cell.1 {
                    SeatPart::Name => seat_name_cell(tr, &paint, seat, by, picked),
                    SeatPart::By => {
                        let words = played_by_text(tr, by);
                        match (by, picked && seat > 0) {
                            (PlayedBy::Open { .. }, _) => (words, palette::GOLD, OPEN_FONT),
                            // The dial's `< >` under the cursor, on every
                            // seat A/D turns.
                            (
                                PlayedBy::Keys(_)
                                | PlayedBy::Pad { .. }
                                | PlayedBy::Ai(_)
                                | PlayedBy::Bot,
                                true,
                            ) => (format!("< {words} >"), Color::WHITE, ROW_FONT),
                            (
                                PlayedBy::Keys(_)
                                | PlayedBy::Pad { .. }
                                | PlayedBy::Ai(_)
                                | PlayedBy::Bot,
                                false,
                            ) => (words, row_ink(picked), ROW_FONT),
                        }
                    }
                    SeatPart::Status
                    | SeatPart::KeysIcon
                    | SeatPart::PadIcon
                    | SeatPart::RobotIcon => (
                        status_text(tr, by).to_string(),
                        match by {
                            PlayedBy::Keys(_) | PlayedBy::Pad { .. } => {
                                palette::INK_ONE_POST.with_alpha(0.95)
                            }
                            PlayedBy::Open { .. } | PlayedBy::Ai(_) | PlayedBy::Bot => {
                                palette::PARCHMENT.with_alpha(0.45)
                            }
                        },
                        STATUS_FONT,
                    ),
                };
                menu_ui::set_text(&mut text, &words);
                menu_ui::set_color(&mut ink, color);
                if let Some(mut font) = font
                    && font.font_size != FontSize::Px(px)
                {
                    font.font_size = FontSize::Px(px);
                }
            }
        }
    }
    // A pad's Start that added a seat can move the map after step 2
    // showed it: the match is played on the one named here.
    if let Ok((mut text, mut node)) = moved.single_mut() {
        let note = moved_note(tr, config, &paint.beaches);
        menu_ui::set_text(&mut text, note.as_deref().unwrap_or(""));
        menu_ui::set_shown(&mut node, note.is_some());
    }
    if let Ok((mut text, mut ink)) = hint.single_mut() {
        menu_ui::set_text(&mut text, &seat_hint(tr, *paint.device, at));
        menu_ui::set_color(&mut ink, palette::PARCHMENT.with_alpha(0.55));
    }
    if let Ok((mut text, mut pill)) = line.single_mut() {
        let open = open_seats(config, &pads, seated.pads.p2_at_keys(), &paint.settings);
        let pad_to_join =
            seated
                .pads
                .a_start_would_seat_someone(&live, &paint.settings, &config.human_seats());
        let (words, warns) = seats_line(tr, &open, config.ai_level, pad_to_join);
        menu_ui::set_text(&mut text, &words);
        menu_ui::set_bg(
            &mut pill,
            match warns {
                true => palette::PILL_FILL,
                false => Color::NONE,
            },
        );
    }
}

/// A seat's name cell: the name, the box being typed into, or for a seat
/// somebody plays with no name yet, the key that names it.
fn seat_name_cell(
    tr: &crate::app::i18n::Tr,
    paint: &Paint,
    seat: u8,
    by: PlayedBy,
    picked: bool,
) -> (String, Color, f32) {
    let given = &paint.settings.names[usize::from(paint.config.name_row(seat))];
    if paint.menu.naming == Some(seat) {
        // A caret says the keyboard is being typed into rather than
        // navigating; the row is its own instructions.
        let keep = crate::app::gamepad::scheme::spelled(
            tr,
            *paint.device,
            &crate::app::gamepad::Face::Typing(crate::app::gamepad::scheme::TextBox::Keeps),
            crate::app::gamepad::scheme::Does::Confirm,
            tr.key_enter,
        );
        return (
            format!("{given}_ {}", fill(tr.match_name_typing, &[("k", &keep)])),
            Color::WHITE,
            STATUS_FONT,
        );
    }
    if !given.is_empty() {
        return (given.clone(), row_ink(picked), ROW_FONT);
    }
    match by {
        PlayedBy::Keys(_) | PlayedBy::Pad { .. } => (
            unnamed(tr, *paint.device),
            palette::PARCHMENT.with_alpha(0.45),
            STATUS_FONT,
        ),
        PlayedBy::Open { .. } | PlayedBy::Ai(_) | PlayedBy::Bot => (
            "-".to_string(),
            palette::PARCHMENT.with_alpha(0.35),
            ROW_FONT,
        ),
    }
}

/// Step 3's Start match button, naming the key that starts the match in
/// the hands last used: Enter at the keyboard, and on a pad the first
/// button the step's own table starts it with
/// ([`crate::app::gamepad::scheme::binds`]), Start once that pad's Start
/// seats nobody new, else A.
pub fn start_button_label(tr: &crate::app::i18n::Tr, device: LastDevice, join: bool) -> String {
    use crate::app::gamepad::scheme::{Does, spelled};
    let face = crate::app::gamepad::Face::MatchSetup {
        step: Step::Players,
        join,
        partners: false,
    };
    let key = spelled(tr, device, &face, Does::StartMatch, tr.key_enter);
    fill(tr.setup_start, &[("k", &key)])
}

/// Paint step 3's Start match button's words ([`start_button_label`]).
pub fn paint_start_button(
    paint: Paint,
    seated: SeatsPaint,
    last_pad: Res<crate::app::gamepad::LastPad>,
    mut words: Query<&mut Text, With<StartButtonText>>,
) {
    if paint.menu.step != Step::Players {
        return;
    }
    let live: Vec<Entity> = seated.live.iter().collect();
    let join = seated.pads.start_joins_here(
        &live,
        &paint.settings,
        &paint.config.human_seats(),
        last_pad.0,
    );
    let label = start_button_label(paint.settings.tr(), *paint.device, join);
    for mut text in &mut words {
        menu_ui::set_text(&mut text, &label);
    }
}

/// Step 3's line about teams, and no team heading.
type PartnerLineOnly = (With<PartnerLine>, Without<TeamHeadingUi>);

/// Paint step 3's team headings, each its seats' names joined in the
/// colour of the seat standing for it, as the line-up heads a team
/// ([`crate::app::lineup::team_heading`]), and the line about teams under
/// the hint ([`partner_line`]).
pub fn paint_teams(
    paint: Paint,
    seated: SeatsPaint,
    mut headings: Query<(&TeamHeadingUi, &mut Text, &mut TextColor), Without<PartnerLine>>,
    mut line: Query<(&mut Text, &mut Node), PartnerLineOnly>,
) {
    if paint.menu.step != Step::Players {
        return;
    }
    let tr = paint.settings.tr();
    let config = &*paint.config;
    let mode = setup_teams(&paint.settings, config);
    let names = crate::app::SeatNames(config.seat_names(&paint.settings.names));
    for (heading, mut text, mut ink) in &mut headings {
        let (words, color) =
            crate::app::lineup::team_heading(tr, &names, mode, heading.0, config.seats);
        menu_ui::set_text(&mut text, &words);
        menu_ui::set_color(&mut ink, color);
    }
    let at = paint.menu.seat.min(config.seats - 1);
    let live: Vec<Entity> = seated.live.iter().collect();
    let pads = seated
        .pads
        .seat_pads(&live, &paint.settings, &config.human_seats());
    let pin = p2_pin(config, &paint.settings, &pads, seated.pads.p2_at_keys());
    let said = partner_line(tr, *paint.device, (config, &paint.settings), at, pin);
    for (mut text, mut node) in &mut line {
        menu_ui::set_text(&mut text, said.as_deref().unwrap_or(""));
        menu_ui::set_shown(&mut node, said.is_some());
    }
}
