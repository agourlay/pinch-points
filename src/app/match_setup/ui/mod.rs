//! The setup steps on screen: one card per step under the step indicator,
//! built when its step comes up and painted every frame from the model in
//! [`steps`].

use super::*;
use crate::app::gamepad::LastDevice;
use crate::app::menu_ui;
use crate::app::menu_ui::cut_to;
use crate::app::settings::GameSettings;

mod game;
mod map;
mod map_words;
mod seats;
pub use game::*;
pub use map::*;
pub use map_words::*;
pub use seats::*;

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
    let up = paint.menu.host_page_up();
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

/// A row's ink: white under the cursor, parchment elsewhere.
fn row_ink(picked: bool) -> Color {
    match picked {
        true => Color::WHITE,
        false => palette::PARCHMENT.with_alpha(0.80),
    }
}
