//! Shared list-menu plumbing: one implementation of row navigation,
//! left/right detection, row spawning, and selected-row painting for the
//! landing menu, settings, match setup, and the pause card.

use crate::app::cycle::Turn;
use crate::app::palette;
use bevy::prelude::*;

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

/// Enter, from either key that says it: the main one or the numpad's.
/// One question rather than a per-screen pair of `just_pressed` checks,
/// because the screens disagreed on whether the numpad counted, and a
/// player whose Enter is on the numpad should be heard everywhere.
pub fn enter(keys: &ButtonInput<KeyCode>) -> bool {
    keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter)
}

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

/// The padding above and below a card's contents.
pub const CARD_PAD_Y: f32 = 16.0;

/// The shell's card: a deep fill behind a gold hairline, the shape every
/// list screen is drawn on. Menu, stage list, trophies, settings, key
/// bindings, the shelf of kept rounds and the lobby all wear it, so a
/// player crossing between them is looking at one game.
pub fn screen_card() -> (Node, BackgroundColor, BorderColor, BoxShadow) {
    (
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(ROW_GAP),
            padding: UiRect::axes(Val::Px(22.0), Val::Px(CARD_PAD_Y)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(16.0)),
            ..default()
        },
        BackgroundColor(palette::CARD_FILL),
        BorderColor::all(palette::CARD_EDGE),
        card_shadow(),
    )
}

/// What [`between_bars`] keeps clear at each end: the header bar above,
/// the prompt line below.
pub const BAR_H: f32 = 52.0;

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

/// A heading over a group of rows inside a card.
pub fn heading(text: &str, first: bool) -> impl Bundle {
    (
        Text::new(text.to_string()),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
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
        palette::GOLD.with_alpha(0.16)
    } else {
        Color::NONE
    }
}

/// A card row's height and the air between two of them. Public because a
/// card that hides rows cannot size itself to its contents and has to do
/// the arithmetic: doing it with its own guess at these numbers is how the
/// match setup card came out an inch too short for a full table.
pub const ROW_H: f32 = 25.0;
pub const ROW_GAP: f32 = 2.0;
/// A group heading with the margin under it.
pub const HEADING_H: f32 = 20.0;

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
            padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
            border_radius: BorderRadius::all(Val::Px(7.0)),
            ..default()
        },
        BackgroundColor(Color::NONE),
    )
}

/// Which side of a two-column card row a cell is: what the row sets, and
/// what it is set to.
///
/// One enum rather than one per screen. The settings card and the key
/// bindings card each declared an identical `Half { Label, Value }`, which
/// is two names for one idea and two places to change it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Half {
    Label,
    Value,
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
        color: Color::srgba(0.0, 0.05, 0.12, 0.45),
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
pub fn spawn_rows<M: Component>(
    parent: &mut ChildSpawnerCommands,
    count: usize,
    font_px: f32,
    make: impl Fn(usize) -> M,
) {
    for row in 0..count {
        parent.spawn((
            make(row),
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(font_px),
                ..default()
            },
            TextColor(Color::WHITE),
        ));
    }
}

/// Write text only when it changed: an unchanged `Text` write still
/// re-shapes and re-rasterizes the glyphs, which once cost this game 4.4%
/// of its CPU (see the guard note in `side_panels`). A one-liner so the
/// guard cannot be forgotten at a call site.
///
/// Takes the query's `Mut` rather than a plain `&mut Text`, and that is the
/// whole trick: `Mut`'s `DerefMut` flags the component changed the instant
/// it is taken, so a helper reached through one would announce a change
/// before it had looked at whether there was one, and `bevy_ui` would
/// re-measure the text anyway. Reading through `Deref` to compare is free;
/// only the write inside the branch reaches for `DerefMut`.
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

    /// The guard has to survive the trip through `Mut`. Bevy flags a
    /// component the moment `DerefMut` is taken, so a helper that accepted
    /// a plain `&mut Text` announced every write as a change before it had
    /// compared anything, and `bevy_ui` re-measures and re-rasterizes on
    /// `Changed<Text>`, which is the cost the guard exists to dodge.
    /// The check is on the flag, not the string: both spellings write the
    /// same bytes and only one of them is free.
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
