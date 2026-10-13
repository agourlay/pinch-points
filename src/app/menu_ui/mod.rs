//! Shared list-menu plumbing: one implementation of row navigation,
//! left/right detection, row spawning, and selected-row painting for the
//! landing menu, settings, match setup, and the pause card.
//!
//! The navigation keys live in [`nav`] and the card dressing (driftwood
//! frame, tide line) in [`frame`]; both are re-exported here, so every
//! screen reaches them as `menu_ui::x`.

mod frame;
mod nav;

pub use frame::{FOAM_DEPTH, Framed, ShoreCard, dress_cards, scale_card_frames, tide_line};
pub use nav::{
    Buttons, CLOSERS, NUMBER_KEYS, Nav, enter, left_right, nav, nav_live, number_pressed, spend,
    step,
};

use crate::app::palette;
use bevy::prelude::*;

/// Text in the display face at `px`: the wordmark, the header, clocks,
/// scores, banners and headings. See [`crate::app::boot::DISPLAY_FONT`]
/// for why it is those and not the rows.
pub fn display_font(px: f32) -> TextFont {
    TextFont {
        font: FontSource::Handle(crate::app::boot::DISPLAY_FONT),
        font_size: FontSize::Px(px),
        ..default()
    }
}

/// The UI's type scale, largest to smallest: the wordmark, screen titles,
/// section headings, list rows, body copy, and the fine print. Sizes that
/// answer to something else (per-seat score arrays, row constants tied to a
/// width budget, computed fits) stay where they are.
pub mod type_scale {
    /// The menu wordmark.
    pub const TITLE: f32 = 54.0;
    /// Screen titles: the HUD clock line, the tournament header.
    pub const DISPLAY: f32 = 34.0;
    /// Section headings.
    pub const HEADING: f32 = 24.0;
    /// List rows and prompts.
    pub const ROW: f32 = 19.0;
    /// Body copy on cards.
    pub const BODY: f32 = 17.0;
    /// The fine print: blurbs, keys, footnotes.
    pub const FINE: f32 = 15.0;
}

/// What sits over what, when two of these are on screen at once.
///
/// One list rather than a number spelled into each of the five files that
/// spawn one, because the order is the only thing about these numbers
/// that means anything, and the order is exactly what cannot be seen from
/// any one of them. A tide event's banner was above the card the crowd
/// picks tide events from, and above the pause card, both of which
/// somebody had opened and was in the middle of reading.
pub mod layer {
    /// The scoreboard at the end of a round. Under everything, being the
    /// thing the rest is shown against.
    pub const RESULTS: i32 = 10;
    /// An event announcing itself in the middle of the screen. Transient,
    /// and nobody's to work, so it goes under the cards that are: it has
    /// the middle to itself unless somebody wants that space more.
    pub const BANNER: i32 = 15;
    /// A card somebody opened and is reading or choosing from. Above a
    /// banner, because they are using this one.
    pub const CARD: i32 = 20;
    /// A toast in its own corner. Over everything, and overlapping
    /// nothing, which is what a corner is for.
    pub const TOAST: i32 = 30;

    /// The order is the whole of what these numbers are for, so it is
    /// checked where it cannot be run past: a card somebody opened is not
    /// covered by an announcement that is only passing through, an
    /// announcement still clears the board behind it, and the corner is
    /// over everything and in nobody's way.
    ///
    /// Found by opening the spectators' event list while a tide event was
    /// announcing itself: the banner held the middle, and the list nobody
    /// could read was underneath it. The pause card sat under one too.
    const _: () = assert!(RESULTS < BANNER && BANNER < CARD && CARD < TOAST);
}

/// A full-window node whose only job is to centre its child, so a card can
/// size itself to its contents instead of doing absolute-position maths.
/// Override the extras with struct update syntax:
/// `Node { row_gap: Val::Px(12.0), ..menu_ui::centred_overlay() }`.
pub fn centred_overlay() -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        ..default()
    }
}

/// The padding round a card's contents: the driftwood frame drawn over
/// the card's edge ([`FRAME_PX`]) and clear room inside it. At 16 the top
/// row sat three pixels under the wood and the sides nine.
pub const CARD_PAD: f32 = frame::FRAME_PX + 12.0;

/// The shell's card: a deep fill behind a gold hairline, the shape every
/// list screen is drawn on. Menu, stage list, trophies, settings, key
/// bindings, the shelf of kept rounds and the lobby all wear it, so a
/// player crossing between them is looking at one game.
pub fn screen_card() -> (ShoreCard, Node, BackgroundColor, BorderColor, BoxShadow) {
    (
        ShoreCard,
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(ROW_GAP),
            // Free to shrink into the space between the bars. Left to its
            // automatic minimum, a card is held to the height its rows
            // measure while it is sizing itself, and a `card_row` measures
            // taller there than it lays out (by 13px a row, measured): the
            // key bindings card came out 702px for 546 of rows and ran
            // under the header and over the prompt, and the replay shelf
            // and match setup carried the same blank band, only with room
            // to spare. A card whose rows really do not fit is no better
            // off either way.
            min_height: Val::Px(0.0),
            padding: UiRect::all(Val::Px(CARD_PAD)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(16.0)),
            ..default()
        },
        BackgroundColor(palette::CARD_FILL),
        BorderColor::all(palette::CARD_EDGE),
        card_shadow(),
    )
}

/// How tall the HUD's header strip is. Lives here rather than in `hud`,
/// beside [`BAR_H`], which is derived from it: the bar and the space kept
/// clear for it are one measurement, and as two numbers in two files a
/// header grown to 56 tucks itself under every card on every list screen.
pub const HEADER_H: f32 = 42.0;

/// The first row clear of the header, for the in-round chrome that hangs
/// from it: the hint line and the two sidebars. Derived, like [`BAR_H`],
/// so a taller header moves them down rather than over them.
pub const UNDER_HEADER: f32 = HEADER_H + 4.0;

/// The air between a card and the chrome at either end of it.
const BAR_AIR: f32 = 10.0;

/// What [`between_bars`] keeps clear at each end.
///
/// One inset, used top and bottom, so a card sits centred in what is left.
/// Above it that is the header exactly, plus the air; below it the prompt
/// pill is shorter, so the card clears it comfortably.
pub const BAR_H: f32 = HEADER_H + BAR_AIR;

/// The frame a card is centred in: everything between the header bar and
/// the prompt line, and nothing outside it.
pub fn between_bars() -> Node {
    Node {
        position_type: PositionType::Absolute,
        top: Val::Px(BAR_H),
        bottom: Val::Px(BAR_H),
        left: Val::Px(0.0),
        right: Val::Px(0.0),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        row_gap: Val::Px(10.0),
        ..default()
    }
}

/// A heading over a group of rows inside a card, in the display face like
/// every other heading: the settings card's were the last ones still in
/// the rows' face.
pub fn heading(text: &str, first: bool) -> impl Bundle {
    (
        Text::new(text.to_string()),
        display_font(type_scale::BODY),
        TextColor(palette::GOLD.with_alpha(0.55)),
        Node {
            margin: UiRect::top(Val::Px(if first { 0.0 } else { 10.0 }))
                .with_bottom(Val::Px(3.0))
                .with_left(Val::Px(10.0)),
            align_self: AlignSelf::FlexStart,
            ..default()
        },
    )
}

/// The band behind the row the cursor is on. A band rather than a caret:
/// the rows are two columns wide and a marker at the far left is a long
/// way from the words it points at.
pub fn band(picked: bool) -> Color {
    if picked {
        palette::PICKED_WASH
    } else {
        Color::NONE
    }
}

/// A card row's height and the air between two of them. Public because a
/// card that hides rows cannot size itself to its contents and has to do
/// the arithmetic, and its own guess at these numbers left the match setup
/// card an inch too short for a full table.
pub const ROW_H: f32 = 25.0;
pub const ROW_GAP: f32 = 2.0;
/// The air above and below a row's text, inside the row.
const ROW_PAD_Y: f32 = 3.0;

/// How tall a row of `font`-sized text lays out when nothing squeezes it:
/// its line and the row's padding. Taller than [`ROW_H`], which is only
/// the least a row keeps; a row squeezed down to that spills its text into
/// the next, and the last one onto the card's frame.
pub fn row_pitch(font: f32) -> f32 {
    (font * LINE_HEIGHT + 2.0 * ROW_PAD_Y).max(ROW_H)
}
/// A group heading with the margin under it: the heading's line, at the
/// line height Bevy gives text by default, and the air under it. Derived,
/// because a guess of 20 left the match setup card four pixels short, and
/// with every row of a full table at its minimum that put the last one on
/// the card's frame.
pub const HEADING_H: f32 = HEADING_FONT * LINE_HEIGHT + HEADING_MARGIN;
/// The heading's type size, and the air between it and the first row.
const HEADING_FONT: f32 = 15.0;
const HEADING_MARGIN: f32 = 6.0;
/// Bevy's default line height, as a multiple of the font size.
pub const LINE_HEIGHT: f32 = 1.2;

/// The shape of a row inside a card: its own padding, its own corner, and
/// a background the cursor fills in.
pub fn card_row() -> (Node, BackgroundColor) {
    (
        Node {
            align_items: AlignItems::Center,
            // A row keeps its height when it has nothing to say. A shelf
            // of twelve slots holding two rounds is still twelve slots
            // tall, so pasting a third does not resize the card under it.
            min_height: Val::Px(ROW_H),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(ROW_PAD_Y)),
            border_radius: BorderRadius::all(Val::Px(7.0)),
            ..default()
        },
        BackgroundColor(Color::NONE),
    )
}

/// Which side of a two-column card row a cell is: what the row sets, and
/// what it is set to.
///
/// One enum rather than one per screen: the settings card and the key
/// bindings card each declared an identical `Half { Label, Value }`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Half {
    Label,
    Value,
}

/// A two-column row's ink: the label quiet and the value clear, both lit
/// on the row the cursor is on. The settings and the key bindings are the
/// same card, and read alike.
pub fn cell_ink(half: Half, picked: bool) -> Color {
    match (half, picked) {
        (Half::Label, true) => Color::WHITE,
        (Half::Label, false) => palette::PARCHMENT.with_alpha(0.62),
        (Half::Value, true) => palette::GOLD,
        (Half::Value, false) => palette::PARCHMENT.with_alpha(0.92),
    }
}

/// A fixed-width, clipped, single-line cell inside a card row. Every list
/// on every screen lines its columns up this way, and none of them can be
/// resized by what is written in them.
pub fn cell(width: f32, font_px: f32) -> (Node, Text, TextFont, TextLayout, TextColor) {
    (
        Node {
            width: Val::Px(width),
            flex_shrink: 0.0,
            overflow: Overflow::clip_x(),
            ..default()
        },
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(font_px),
            ..default()
        },
        TextLayout::no_wrap(),
        TextColor(palette::IDLE_ROW),
    )
}

/// The lift every card in the shell stands on: a soft deep-water shadow,
/// straight down, no spread. Shared so the cards on one screen do not
/// float at a different height from the cards on the next.
pub fn card_shadow() -> BoxShadow {
    BoxShadow::from(ShadowStyle {
        color: palette::CARD_SHADOW,
        x_offset: Val::Px(0.0),
        y_offset: Val::Px(6.0),
        spread_radius: Val::Px(0.0),
        blur_radius: Val::Px(18.0),
    })
}

/// Despawn everything a screen tagged with `M`. Usable straight as a system:
/// `add_systems(OnExit(Screen::Foo), despawn_marked::<FooUi>)`.
pub fn despawn_marked<M: Component>(mut commands: Commands, ui: Query<Entity, With<M>>) {
    for entity in &ui {
        commands.entity(entity).despawn();
    }
}

/// The same, for the screens that edit settings: write them out on the way
/// past. Match setup counts as one of those, because the seat names typed
/// there are a setting and outlive the match.
pub fn save_and_despawn<M: Component>(
    mut commands: Commands,
    settings: Res<crate::app::settings::GameSettings>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    ui: Query<Entity, With<M>>,
) {
    settings.save(&caps);
    for entity in &ui {
        commands.entity(entity).despawn();
    }
}

/// Spawn `count` empty text rows tagged by `make(row)`, in display order.
///
/// The rows share one left edge, in a column the card centres as a whole.
/// Centred one by one, as a card lays out its children, labels of
/// different lengths each started somewhere else and the `>` marker
/// wandered from row to row.
pub fn spawn_rows<M: Component>(
    parent: &mut ChildSpawnerCommands,
    count: usize,
    font_px: f32,
    make: impl Fn(usize) -> M,
) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexStart,
            row_gap: Val::Px(ROW_GAP),
            ..default()
        })
        .with_children(|column| {
            for row in 0..count {
                column.spawn((
                    make(row),
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(font_px),
                        ..default()
                    },
                    TextColor(Color::WHITE),
                ));
            }
        });
}

/// Write text only when it changed: an unchanged `Text` write still
/// re-shapes and re-rasterizes the glyphs, which once cost this game 4.4%
/// of its CPU (see the guard note in `side_panels`). A one-liner so the
/// guard cannot be forgotten at a call site.
///
/// Takes the query's `Mut` rather than a plain `&mut Text`, and that is the
/// whole trick: `Mut`'s `DerefMut` flags the component changed the instant
/// it is taken, so a helper reached through one announces a change before
/// looking at whether there was one. Reading through `Deref` to compare is
/// free; only the write inside the branch reaches for `DerefMut`.
///
/// Generic over the string-shaped text components, because a [`TextSpan`],
/// half of a line that is two colours, costs as much to write blindly as
/// the [`Text`] it hangs off.
pub fn set_text<T: std::ops::DerefMut<Target = String>>(text: &mut Mut<T>, value: &str) {
    if text.as_str() != value {
        value.clone_into(&mut ***text);
    }
}

/// The same guard for a text colour, `Mut` and all.
pub fn set_color(color: &mut Mut<TextColor>, target: Color) {
    if color.0 != target {
        color.0 = target;
    }
}

/// And for a background fill.
pub fn set_bg(bg: &mut Mut<BackgroundColor>, target: Color) {
    if bg.0 != target {
        bg.0 = target;
    }
}

/// Show or hide a node, with the same only-write-on-change guard: a `Node`
/// written blindly re-runs layout for the whole tree it sits in. Shown is
/// `Display::Flex`, which is what every hidden-and-shown node here uses.
pub fn set_shown(node: &mut Mut<Node>, shown: bool) {
    let want = if shown { Display::Flex } else { Display::None };
    if node.display != want {
        node.display = want;
    }
}

/// The window as the interface sees it: its size and pixel density, and
/// the interface's own scale over both.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Viewport<'w, 's> {
    windows: Query<'w, 's, &'static Window>,
    ui_scale: Res<'w, UiScale>,
}

impl Viewport<'_, '_> {
    /// The window's size in interface units, which is what anything laid
    /// out against the interface measures in: its pixels over the
    /// interface's scale. `None` on a frame with no window.
    pub fn size(&self) -> Option<Vec2> {
        let window = self.windows.single().ok()?;
        Some(Vec2::new(window.width(), window.height()) / self.ui_scale.0.max(f32::EPSILON))
    }

    /// The interface's scale.
    pub fn ui_scale(&self) -> f32 {
        self.ui_scale.0
    }

    /// Physical pixels per logical one, 1 with no window to ask.
    pub fn scale_factor(&self) -> f32 {
        self.windows.iter().next().map_or(1.0, Window::scale_factor)
    }

    /// Whether the interface's scale moved since this system last ran.
    pub fn ui_scale_changed(&self) -> bool {
        self.ui_scale.is_changed()
    }
}

/// Empty room `px` tall between two lines of a card.
pub fn spacer(px: f32) -> Node {
    Node {
        height: Val::Px(px),
        ..default()
    }
}

/// A footnote under a card, on a pill of its own: below a card these lines
/// sit on open sand, where pale text on bright sand is the one place this
/// interface goes illegible, and on a small window partly under the card,
/// where bare dim text disappears.
pub fn footnote(text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(type_scale::BODY),
            ..default()
        },
        TextLayout::no_wrap(),
        TextColor(palette::PARCHMENT.with_alpha(0.75)),
        Node {
            padding: UiRect::axes(Val::Px(12.0), Val::Px(3.0)),
            border_radius: BorderRadius::all(Val::Px(11.0)),
            ..default()
        },
        BackgroundColor(palette::PILL_FILL),
    )
}

/// The heading of a card: an optional sprite, its name, and a rule
/// running off to the card's edge in the same gold hairline the card is
/// drawn in, so a panel of empty rows still reads as a made thing.
pub fn heading_row(
    panel: &mut bevy::ecs::relationship::RelatedSpawnerCommands<ChildOf>,
    heading: &str,
    icon: Option<&Handle<Image>>,
) {
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            margin: UiRect::bottom(Val::Px(HEADING_MARGIN)),
            width: Val::Percent(100.0),
            ..default()
        })
        .with_children(|head| {
            if let Some(icon) = icon {
                head.spawn((
                    // 22px, the size the field guide draws a crab at: the
                    // sprites are 96px of smooth edges, and below about
                    // twenty they stop being a shape and become a smudge.
                    ImageNode::new(icon.clone()).with_color(palette::HEADING_ICON),
                    Node {
                        width: Val::Px(22.0),
                        height: Val::Px(22.0),
                        ..default()
                    },
                ));
            }
            head.spawn((
                Text::new(heading),
                display_font(HEADING_FONT),
                TextColor(palette::IDLE_ROW),
            ));
            head.spawn((
                Node {
                    flex_grow: 1.0,
                    height: Val::Px(1.0),
                    margin: UiRect::left(Val::Px(8.0)),
                    ..default()
                },
                BackgroundColor(palette::CARD_EDGE),
            ));
        });
}

/// Write one row's text and colour with the shared selection style,
/// guarding both writes so unchanged rows never re-shape.
pub fn paint_row(selected: bool, line: &str, text: &mut Mut<Text>, color: &mut Mut<TextColor>) {
    set_text(
        text,
        &format!("{} {line}", if selected { ">" } else { " " }),
    );
    let target = if selected {
        palette::SELECTED_ROW
    } else {
        palette::IDLE_ROW
    };
    set_color(color, target);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The guard has to survive the trip through `Mut`: Bevy flags a
    /// component the moment `DerefMut` is taken, and `bevy_ui` re-measures
    /// and re-rasterizes on `Changed<Text>`, which is the cost the guard
    /// exists to dodge. The check is on the flag, not the string: both
    /// spellings write the same bytes and only one is free.
    #[test]
    fn rewriting_the_same_text_flags_nothing() {
        let mut world = World::new();
        let id = world
            .spawn((Text::new("same"), TextColor(palette::IDLE_ROW)))
            .id();
        let flagged = |world: &mut World, write: &dyn Fn(&mut World)| {
            world.clear_trackers();
            let before = world.change_tick();
            // A write stamps the world's current tick, so the clock has to
            // move for "changed since `before`" to mean anything.
            world.increment_change_tick();
            write(world);
            world
                .entity(id)
                .get_ref::<Text>()
                .expect("spawned with text")
                .last_changed()
                .is_newer_than(before, world.change_tick())
        };
        assert!(
            !flagged(&mut world, &|world| {
                let mut text = world.get_mut::<Text>(id).expect("spawned with text");
                set_text(&mut text, "same");
            }),
            "an identical string must not mark the text changed"
        );
        assert!(
            flagged(&mut world, &|world| {
                let mut text = world.get_mut::<Text>(id).expect("spawned with text");
                set_text(&mut text, "different");
            }),
            "a real edit still has to mark it changed"
        );
        assert_eq!(world.entity(id).get::<Text>().expect("text").0, "different");
    }
}
