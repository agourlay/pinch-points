//! Shared list-menu plumbing: one implementation of row navigation,
//! left/right detection, row spawning, and selected-row painting for the
//! landing menu, settings, match setup, and the pause card.

use crate::app::cycle::Turn;
use crate::app::palette;
use bevy::color::Mix;
use bevy::prelude::*;
use bevy::ui::widget::NodeImageMode;

/// The UI's type scale, largest to smallest: the wordmark, screen titles,
/// section headings, list rows, body copy, and the fine print. Sizes that
/// answer to something else (per-seat score arrays, row constants tied to a
/// width budget, computed fits) stay where they are.
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

/// The number keys, one to nine, in the order a list numbers its rows.
pub const NUMBER_KEYS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

/// Which of the first `count` rows a number key picked this frame, as a
/// 0-based index: the menu's modes, the beaches on the air, the tide's
/// events, the seats to call.
pub fn number_pressed(keys: &ButtonInput<KeyCode>, count: usize) -> Option<usize> {
    NUMBER_KEYS
        .iter()
        .take(count)
        .position(|&key| keys.just_pressed(key))
}

/// W/S (or arrow) navigation over `len` rows, wrapping at the ends.
pub fn nav(keys: &ButtonInput<KeyCode>, selected: usize, len: usize) -> usize {
    let mut at = selected;
    if keys.just_pressed(KeyCode::KeyW) || keys.just_pressed(KeyCode::ArrowUp) {
        at = (at + len - 1) % len;
    }
    if keys.just_pressed(KeyCode::KeyS) || keys.just_pressed(KeyCode::ArrowDown) {
        at = (at + 1) % len;
    }
    at
}

/// One frame's worth of vertical menu movement. Its own type rather than
/// the `up: bool, down: bool` pair it replaces: two bools claim four states
/// where a cursor has three, and every caller had to know that up wins.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nav {
    Up,
    Down,
    Stay,
}

/// Navigation over a list where some rows are hidden (a setting that does
/// not apply right now): the cursor steps over them instead of landing on
/// a blank line. With nothing live it stays put.
pub fn nav_live(keys: &ButtonInput<KeyCode>, selected: usize, live: &[bool]) -> usize {
    let nav = if keys.just_pressed(KeyCode::KeyW) || keys.just_pressed(KeyCode::ArrowUp) {
        Nav::Up
    } else if keys.just_pressed(KeyCode::KeyS) || keys.just_pressed(KeyCode::ArrowDown) {
        Nav::Down
    } else {
        Nav::Stay
    };
    nav_live_steps(nav, selected, live)
}

/// Plain movement over `len` rows from an explicit [`Nav`], for input that
/// cannot ride the keyboard resource: the pause card runs during play, where
/// bridging the d-pad into synthetic key presses would also steer the round.
pub fn step(nav: Nav, selected: usize, len: usize) -> usize {
    match nav {
        Nav::Up => (selected + len - 1) % len,
        Nav::Down => (selected + 1) % len,
        Nav::Stay => selected,
    }
}

/// The same walk as [`nav_live`], from an explicit [`Nav`] rather than the
/// keyboard.
fn nav_live_steps(nav: Nav, selected: usize, live: &[bool]) -> usize {
    let len = live.len();
    let step = match nav {
        Nav::Up => len - 1, // one backwards, modulo len
        Nav::Down => 1,
        // Not moving, but the row under the cursor may have just gone dark.
        Nav::Stay => return first_live(selected, live).unwrap_or(selected),
    };
    let mut at = selected;
    for _ in 0..len {
        at = (at + step) % len;
        if live[at] {
            return at;
        }
    }
    selected
}

/// `from` if it is live, else the next live row after it.
fn first_live(from: usize, live: &[bool]) -> Option<usize> {
    (0..live.len())
        .map(|offset| (from + offset) % live.len())
        .find(|&row| live[row])
}

/// Enter, from either key that says it: the main one or the numpad's. One
/// question rather than a per-screen pair of `just_pressed` checks, which
/// disagreed on whether the numpad counted.
pub fn enter(keys: &ButtonInput<KeyCode>) -> bool {
    keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter)
}

/// Use up the keys that just closed something, so nothing later in the
/// frame reads them again.
///
/// A card or a line of chat closes on Enter or Escape, and the systems
/// behind it run later in the same frame and see the same press: the
/// Enter that sent a spectator's line also left the results card, and
/// the Escape that dropped it opened the pause card. Only the press is
/// spent; the key is still held, and lets go as it always would.
pub fn spend(keys: &mut ButtonInput<KeyCode>, codes: &[KeyCode]) {
    for &code in codes {
        keys.clear_just_pressed(code);
    }
}

/// The keys that end a line or put a card away.
pub const CLOSERS: [KeyCode; 3] = [KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Escape];

/// A/D (or arrow) adjustment, as the turn of a dial.
pub fn left_right(keys: &ButtonInput<KeyCode>) -> Option<Turn> {
    if keys.just_pressed(KeyCode::KeyD) || keys.just_pressed(KeyCode::ArrowRight) {
        Some(Turn::Right)
    } else if keys.just_pressed(KeyCode::KeyA) || keys.just_pressed(KeyCode::ArrowLeft) {
        Some(Turn::Left)
    } else {
        None
    }
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
pub const CARD_PAD: f32 = FRAME_PX + 12.0;

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

/// A card the tide dresses: [`dress_cards`] gives every one of these a
/// foam line at its foot the frame it appears, so a screen cannot forget
/// the dressing and no screen has to carry the art handle for it.
#[derive(Component)]
pub struct ShoreCard;

/// The driftwood frame round a card (`card_frame.png`): nine-sliced, so its
/// corners draw as they are and its sides stretch along the card.
#[derive(Component)]
pub struct CardFrame;

/// A card that wears the driftwood frame but not the tide line: the menu's
/// own sign and list, and the lobby's panels, which draw their foam
/// themselves or have none. Every [`ShoreCard`] is framed too.
#[derive(Component)]
pub struct Framed;

/// The corner a framed card is rounded to: the frame's own, so no corner of
/// the card's fill pokes out past the wood.
const FRAMED_RADIUS: f32 = 16.0;

/// How wide the frame's band of wood is, in the sprite's own pixels
/// (tools/gen_sprites.py draws it 32 of 192).
const FRAME_BAND: f32 = 32.0;

/// Where the sprite is cut, in its own pixels: past the band, far enough
/// in that each corner slice holds its whole rounded corner. Cut at the
/// band, the curves ran over into the side slices and were stretched along
/// them, as dark smears into the card.
const FRAME_SLICE: f32 = 56.0;
const _: () = assert!(
    FRAME_SLICE > FRAME_BAND,
    "the cut must hold each corner whole"
);

/// How thick the frame is drawn, in interface pixels.
const FRAME_PX: f32 = 13.0;

/// The slice's corner scale for a frame [`FRAME_PX`] thick at this scale.
///
/// Bevy draws a slice's border at the sprite's own pixels times this cap,
/// in *physical* pixels, whatever the interface's scale: left fixed, a
/// frame drawn 10 px thick at 720p came out 7 at 1080p, as the cards grew
/// round it.
fn frame_corner_scale(ui_scale: f32, scale_factor: f32) -> f32 {
    FRAME_PX / FRAME_BAND * ui_scale * scale_factor
}

/// A card's frame: driftwood, or driftwood stained with the card's own
/// edge colour where a screen gave it one (the winner's, a loss's red),
/// so the frame carries what the hairline under it used to.
fn frame_bundle(art: &crate::app::art::Art, edge: Option<Color>, corner: f32) -> impl Bundle {
    let tint = frame_tint(edge);
    (
        CardFrame,
        ImageNode {
            image: art.card_frame.clone(),
            color: tint,
            image_mode: NodeImageMode::Sliced(TextureSlicer {
                border: BorderRect::all(FRAME_SLICE),
                max_corner_scale: corner,
                ..default()
            }),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            // Over the card's own edge, a pixel out, so none of the
            // hairline shows round it.
            left: Val::Px(-1.0),
            right: Val::Px(-1.0),
            top: Val::Px(-1.0),
            bottom: Val::Px(-1.0),
            ..default()
        },
        Pickable::IGNORE,
    )
}

/// The tint a card's frame takes from the card's edge: bare wood for the
/// plain gold edge every card is born with, or none; the wood stained
/// toward any other, at full strength whatever the edge's own alpha.
fn frame_tint(edge: Option<Color>) -> Color {
    match edge {
        Some(color) if color != palette::CARD_EDGE => {
            Color::WHITE.mix(&color.with_alpha(1.0), 0.55)
        }
        Some(_) | None => Color::WHITE,
    }
}

/// Keep every frame [`FRAME_PX`] thick in interface pixels as the
/// interface's scale moves (see [`frame_corner_scale`]).
pub fn scale_card_frames(
    ui_scale: Res<UiScale>,
    windows: Query<&Window>,
    mut frames: Query<&mut ImageNode, With<CardFrame>>,
    added: Query<(), Added<CardFrame>>,
) {
    if !ui_scale.is_changed() && added.is_empty() {
        return;
    }
    let factor = windows.iter().next().map_or(1.0, Window::scale_factor);
    let corner = frame_corner_scale(ui_scale.0, factor);
    for mut frame in &mut frames {
        if let NodeImageMode::Sliced(slicer) = &mut frame.image_mode
            && (slicer.max_corner_scale - corner).abs() > f32::EPSILON
        {
            slicer.max_corner_scale = corner;
        }
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
}

/// Every button a menu answers to: the keyboard, and the pads.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Buttons<'w, 's> {
    pub keys: Res<'w, ButtonInput<KeyCode>>,
    pub pads: Query<'w, 's, &'static Gamepad>,
}

impl Buttons<'_, '_> {
    /// Whether any pad pressed `button` this frame.
    pub fn pad(&self, button: GamepadButton) -> bool {
        self.pads.iter().any(|pad| pad.just_pressed(button))
    }
}

/// A card just made: what dressing it needs to know.
type NewCard = (Entity, &'static mut Node, Option<&'static BorderColor>);

/// Give a newborn card its tide line and its driftwood frame.
pub fn dress_cards(
    mut commands: Commands,
    art: Res<crate::app::art::Art>,
    viewport: Viewport,
    mut cards: Query<NewCard, Added<ShoreCard>>,
    mut framed: Query<NewCard, (Added<Framed>, Without<ShoreCard>)>,
) {
    let corner = frame_corner_scale(viewport.ui_scale(), viewport.scale_factor());
    for (card, mut node, edge) in &mut framed {
        node.border_radius = BorderRadius::all(Val::Px(FRAMED_RADIUS));
        clear_the_wood(&mut node);
        let frame = commands
            .spawn(frame_bundle(&art, edge.map(|edge| edge.top), corner))
            .id();
        commands.entity(card).insert_children(0, &[frame]);
    }
    for (card, mut node, edge) in &mut cards {
        node.border_radius = BorderRadius::all(Val::Px(FRAMED_RADIUS));
        clear_the_wood(&mut node);
        // The last row keeps its feet dry whatever padding the card chose.
        let dry = Val::Px(FOAM_DEPTH + 4.0);
        if px_of(node.padding.bottom).unwrap_or(0.0) < FOAM_DEPTH {
            node.padding.bottom = dry;
        }
        // Prepended, not appended: UI siblings stack in the order they are
        // added, and the foam belongs behind the rows, not over the last
        // one, which is the promise tide_line's doc makes.
        let tide = commands.spawn(tide_bundle(&art.foam)).id();
        // Over the foam, so the tide breaks inside the wood rather than
        // across it, and under the rows.
        let frame = commands
            .spawn(frame_bundle(&art, edge.map(|edge| edge.top), corner))
            .id();
        commands.entity(card).insert_children(0, &[tide, frame]);
    }
}

/// Hold a framed card's contents [`CARD_PAD`] in from its edge at the top
/// and the sides, whatever padding the card chose: the frame is drawn over
/// the card, so anything nearer runs up against the wood. The foot is the
/// tide's to set.
fn clear_the_wood(node: &mut Node) {
    for side in [
        &mut node.padding.top,
        &mut node.padding.left,
        &mut node.padding.right,
    ] {
        if px_of(*side).is_some_and(|px| px < CARD_PAD) {
            *side = Val::Px(CARD_PAD);
        }
    }
}

fn px_of(v: Val) -> Option<f32> {
    match v {
        Val::Px(px) => Some(px),
        Val::Auto | Val::Percent(_) | Val::Vw(_) | Val::Vh(_) | Val::VMin(_) | Val::VMax(_) => None,
    }
}

/// How deep the tide laps into a card's foot.
pub const FOAM_DEPTH: f32 = 20.0;

/// The tide at the foot of a card: the foam strip, drawn across it and
/// turned over so it breaks upward into the panel. It makes a card a
/// stretch of beach rather than a box; spawn it before the rows so UI
/// sibling order keeps it behind them, and leave the card room at the
/// foot ([`FOAM_DEPTH`]) so the last row stays dry.
pub fn tide_line(
    panel: &mut bevy::ecs::relationship::RelatedSpawnerCommands<ChildOf>,
    foam: &Handle<Image>,
) {
    panel.spawn(tide_bundle(foam));
}

/// The pieces of one tide line, for a spawner that is not a child scope.
fn tide_bundle(foam: &Handle<Image>) -> (ImageNode, Node) {
    (
        ImageNode {
            image: foam.clone(),
            color: palette::FOAM_LINE,
            flip_y: true,
            // Stretched, not tiled: tiling scales the sprite down to the
            // strip's height first, and six scallops inside twenty pixels
            // come out as a dotted line.
            image_mode: NodeImageMode::Stretch,
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            bottom: Val::Px(0.0),
            height: Val::Px(FOAM_DEPTH),
            // Kept off the card's rounded corners, which a square strip
            // would otherwise fill back in.
            border_radius: BorderRadius::bottom(Val::Px(10.0)),
            ..default()
        },
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

    /// A plain card's frame is bare wood; a card a screen gave its own edge
    /// (the winner's colour, a loss's red) stains its wood with it, at full
    /// strength however faint that edge was drawn.
    #[test]
    fn a_frame_takes_the_cards_own_colour_and_only_that() {
        assert_eq!(frame_tint(None), Color::WHITE);
        assert_eq!(frame_tint(Some(palette::CARD_EDGE)), Color::WHITE);
        let red = frame_tint(Some(palette::INK_RAID.with_alpha(0.6)));
        assert_ne!(red, Color::WHITE, "stained");
        assert_eq!(red.alpha(), 1.0, "at full strength");
        assert_eq!(
            red,
            frame_tint(Some(palette::INK_RAID)),
            "whatever the edge's alpha"
        );
    }

    /// The frame is the same thickness in interface pixels at every scale:
    /// its slice cap follows the interface and the display, so a card grown
    /// to 1080p keeps its frame in proportion, and the default is the
    /// frame's own width over the band's.
    #[test]
    fn a_frame_keeps_its_thickness_at_every_scale() {
        let at = |ui: f32, factor: f32| FRAME_BAND * frame_corner_scale(ui, factor) / (ui * factor);
        for (ui, factor) in [(1.0, 1.0), (1.5, 1.0), (0.8, 1.0), (1.5, 2.0)] {
            assert!((at(ui, factor) - FRAME_PX).abs() < 1e-4, "{ui} x {factor}");
        }
    }

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

    /// Hidden rows are stepped over in both directions, and a cursor left
    /// standing on a row that just went dark slides to the next live one.
    #[test]
    fn nav_live_skips_hidden_rows() {
        let live = [true, false, false, true, false];
        let mut down = ButtonInput::<KeyCode>::default();
        down.press(KeyCode::KeyS);
        assert_eq!(nav_live(&down, 0, &live), 3);
        assert_eq!(nav_live(&down, 3, &live), 0, "wraps past the dark tail");
        let mut up = ButtonInput::<KeyCode>::default();
        up.press(KeyCode::KeyW);
        assert_eq!(nav_live(&up, 3, &live), 0);
        assert_eq!(nav_live(&up, 0, &live), 3);
        // Standing still on a row that went dark: slide to the next live one.
        let idle = ButtonInput::<KeyCode>::default();
        assert_eq!(nav_live(&idle, 1, &live), 3);
        assert_eq!(nav_live(&idle, 3, &live), 3);
        // Nothing live at all: stay put rather than spin.
        assert_eq!(nav_live(&down, 2, &[false, false, false]), 2);
    }

    #[test]
    fn nav_wraps_both_ways() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        assert_eq!(nav(&keys, 0, 5), 4, "up from the top wraps to the bottom");
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyS);
        assert_eq!(nav(&keys, 4, 5), 0, "down from the bottom wraps to the top");
    }
}

#[cfg(test)]
mod nav_tests {
    use super::*;

    /// A row that is not there to pick is stepped over, not landed on.
    ///
    /// Every list with rows that come and go reads this: the settings that
    /// only apply in one mode, the stage list's locked levels, the match
    /// dials that a chosen map has no use for. Landing on one puts the
    /// cursor on a blank line where Enter does nothing.
    #[test]
    fn the_cursor_steps_over_a_row_that_is_not_there_to_pick() {
        let live = [true, false, false, true, true];
        assert_eq!(
            nav_live_steps(Nav::Down, 0, &live),
            3,
            "over the two dark ones"
        );
        assert_eq!(nav_live_steps(Nav::Up, 3, &live), 0, "and back over them");
        assert_eq!(nav_live_steps(Nav::Down, 3, &live), 4, "the next one along");
    }

    /// The ends of the list join up, which is the only way to reach the
    /// last row from the first without walking the whole list.
    #[test]
    fn the_ends_of_the_list_join_up() {
        let all = [true, true, true];
        assert_eq!(
            nav_live_steps(Nav::Up, 0, &all),
            2,
            "off the top to the bottom"
        );
        assert_eq!(
            nav_live_steps(Nav::Down, 2, &all),
            0,
            "and off the bottom to the top"
        );

        // With dark rows at the seam, the wrap keeps walking past them.
        let live = [true, false, false];
        assert_eq!(
            nav_live_steps(Nav::Down, 0, &live),
            0,
            "the only row there is"
        );
        assert_eq!(nav_live_steps(Nav::Up, 0, &live), 0);
    }

    /// A list with nothing live leaves the cursor exactly where it was.
    /// The walk gives up after one lap rather than spinning, which is what
    /// an empty custom-beach shelf looks like from here.
    #[test]
    fn a_list_with_nothing_to_pick_leaves_the_cursor_alone() {
        let none = [false, false, false];
        for nav in [Nav::Up, Nav::Down, Nav::Stay] {
            assert_eq!(
                nav_live_steps(nav, 1, &none),
                1,
                "the cursor moved to nowhere"
            );
        }
    }

    /// Standing still still moves, when the row underneath has just gone
    /// dark: a dial answered somewhere else can take the row the cursor is
    /// on out of the list, and the cursor has to come off it without
    /// anybody pressing anything.
    #[test]
    fn standing_still_comes_off_a_row_that_has_just_gone_dark() {
        let live = [false, false, true, true];
        assert_eq!(
            nav_live_steps(Nav::Stay, 0, &live),
            2,
            "the next one that is there"
        );
        assert_eq!(
            nav_live_steps(Nav::Stay, 2, &live),
            2,
            "and a live row stays put"
        );
        // Past the end, the search wraps like the walk does.
        let live = [true, false, false];
        assert_eq!(nav_live_steps(Nav::Stay, 2, &live), 0);
    }

    /// The plain walk, for a list with no dark rows in it at all: up and
    /// down by one, wrapping at both ends.
    #[test]
    fn the_plain_walk_wraps_at_both_ends() {
        assert_eq!(step(Nav::Down, 0, 3), 1);
        assert_eq!(step(Nav::Down, 2, 3), 0, "off the bottom");
        assert_eq!(step(Nav::Up, 0, 3), 2, "off the top");
        assert_eq!(step(Nav::Stay, 1, 3), 1);
        assert_eq!(step(Nav::Down, 0, 1), 0, "a list of one goes nowhere");
    }
}
