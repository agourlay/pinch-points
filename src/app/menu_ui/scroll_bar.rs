//! The scroll bar every list that can hold more than its card shows wears
//! down its right inside edge: a slim track, and a thumb whose length is
//! the share of the list in view and whose place is how far down it the
//! view has got.
//!
//! It is there to say "there is more below", which a list that stops on a
//! clean row boundary otherwise never says. It is not something to drag.
//!
//! The bar computes no window of its own. Each list hands it the window it
//! paints its rows from ([`ScrollWindow`]), so the thumb cannot disagree
//! with the rows beside it. The track is laid out by flex shares rather
//! than pixels, so a resized window or a new interface scale moves it with
//! the rows and nothing has to be told.
//!
//! The thumb jumps rather than glides, so there is nothing for reduced
//! motion to turn off.

use super::{row_pitch, type_scale};
use crate::app::palette;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use std::marker::PhantomData;

/// The track's width: slim, because it is a sign rather than a handle.
pub const BAR_W: f32 = 6.0;
/// The air between the rows and the track.
pub const BAR_GAP: f32 = 8.0;

/// The shortest the thumb gets: one list row. A list long enough would
/// otherwise shrink it to a speck that reads as dirt on the card rather
/// than as a place in the list.
fn min_thumb() -> f32 {
    row_pitch(type_scale::ROW)
}

/// What a list has in view: the first row (or pixel) showing, how many it
/// shows, and how many there are. Rows for a list that slides by whole
/// rows, pixels for one that scrolls a viewport.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ScrollWindow {
    pub first: f32,
    pub shown: f32,
    pub total: f32,
}

impl ScrollWindow {
    /// A list of `total` rows showing `shown` of them from row `first`.
    pub fn rows(first: usize, shown: usize, total: usize) -> Self {
        Self {
            first: first as f32,
            shown: shown as f32,
            total: total as f32,
        }
    }

    /// Nothing to scroll: what a list that is not on screen reports.
    pub const NONE: Self = Self {
        first: 0.0,
        shown: 0.0,
        total: 0.0,
    };

    /// The thumb for this window, or `None` when the whole list is in view
    /// and the bar should not be drawn at all.
    ///
    /// Its length is `shown / total` of the track; its place is
    /// `first / (total - shown)` of the way along what the thumb can
    /// travel. A window opened past the end (a list that shrank under it
    /// and kept its blank rows) reads as the bottom.
    pub fn thumb(self) -> Option<Thumb> {
        let Self {
            first,
            shown,
            total,
        } = self;
        if shown <= 0.0 || total <= shown {
            return None;
        }
        Some(Thumb {
            size: shown / total,
            at: (first / (total - shown)).clamp(0.0, 1.0),
        })
    }
}

/// Where the thumb sits, in fractions: `size` of the track long, `at` of
/// the way down the travel left over (0 at the top, 1 at the bottom).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Thumb {
    pub size: f32,
    pub at: f32,
}

impl Thumb {
    /// The flex shares of the gap above the thumb, the thumb, and the gap
    /// below it. Shares rather than pixels: when the thumb's floor
    /// ([`min_thumb`]) holds it longer than its share, the layout takes the
    /// difference from the two gaps in proportion, which keeps the place
    /// right.
    fn grows(self) -> [f32; 3] {
        let Self { size, at } = self;
        let travel = 1.0 - size;
        [travel * at, size, travel * (1.0 - at)]
    }
}

/// The track, on the node that hides when the list fits.
#[derive(Component)]
pub struct ScrollTrack;

/// The three parts the track is laid out in, top to bottom.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScrollPart {
    Above,
    Thumb,
    Below,
}

impl ScrollPart {
    const ALL: [ScrollPart; 3] = [ScrollPart::Above, ScrollPart::Thumb, ScrollPart::Below];

    fn index(self) -> usize {
        match self {
            ScrollPart::Above => 0,
            ScrollPart::Thumb => 1,
            ScrollPart::Below => 2,
        }
    }
}

/// The marker on the bar of a list kept in resource `S`, which
/// [`follow`] paints from `S`'s own window.
#[derive(Component)]
pub struct BarOf<S: Send + Sync + 'static>(PhantomData<fn() -> S>);

impl<S: Send + Sync + 'static> Default for BarOf<S> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

/// A list kept in a resource, which can say what it has in view.
pub trait Scrolls: Resource {
    /// The window the list paints its rows from. The same numbers, read
    /// from the same fields: the bar is a picture of this and nothing
    /// else.
    fn window(&self) -> ScrollWindow;
}

/// Lay `rows` out beside a scroll bar tagged `tag`: the rows in a column
/// that takes the width, the bar down its right side as tall as they are.
///
/// The bar keeps its room when it is hidden, so a list that grows past its
/// card does not widen the card under the cursor. As much room again is
/// kept on the left, so the rows sit in the middle of the card, under its
/// heading, as a list without a bar does: the bar's room alone set them
/// half a track left of centre.
pub fn beside(
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    row_gap: f32,
    tag: impl Bundle,
    rows: impl FnOnce(&mut RelatedSpawnerCommands<ChildOf>),
) {
    parent
        .spawn(Node {
            column_gap: Val::Px(BAR_GAP),
            padding: UiRect::left(Val::Px(BAR_GAP + BAR_W)),
            ..default()
        })
        .with_children(|band| {
            band.spawn(Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                row_gap: Val::Px(row_gap),
                ..default()
            })
            .with_children(rows);
            spawn(band, tag);
        });
}

/// Lay `rows` out like [`beside`], in a band that takes whatever height its
/// card has left rather than the height of the rows, and clips them to it.
///
/// For a list whose card is sized by the window rather than by its rows:
/// the band's height is then the room the list has, which
/// [`rows_that_fit`] turns into how many rows to paint. It never depends
/// on how many rows there are, so measuring it cannot feed back into it.
/// The column the rows sit in wears `room`, the marker it is measured by.
/// The bar's room is balanced on the left as [`beside`] balances it, so the
/// rows sit in the middle of their card.
pub fn filling(
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    row_gap: f32,
    tag: impl Bundle,
    room: impl Bundle,
    rows: impl FnOnce(&mut RelatedSpawnerCommands<ChildOf>),
) {
    parent
        .spawn(Node {
            column_gap: Val::Px(BAR_GAP),
            padding: UiRect::left(Val::Px(BAR_GAP + BAR_W)),
            flex_grow: 1.0,
            flex_basis: Val::Px(0.0),
            min_height: Val::Px(0.0),
            ..default()
        })
        .with_children(|band| {
            band.spawn((
                room,
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    row_gap: Val::Px(row_gap),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ))
            .with_children(rows);
            spawn(band, tag);
        });
}

/// How many rows `row` tall, `gap` apart, fit whole in `room`: the number
/// a list sized by its card shows. A row that would run past the foot is
/// not counted, since half a row cut off by the frame reads as a glitch.
/// Half a pixel of slack absorbs the layout's rounding.
pub fn rows_that_fit(room: f32, row: f32, gap: f32) -> usize {
    if row <= 0.0 || room < row {
        return 0;
    }
    ((room + gap + 0.5) / (row + gap)).floor() as usize
}

/// The track and its thumb on their own, for a list whose rows are laid
/// out some other way. It stretches to the height of the row it sits in.
pub fn spawn(parent: &mut RelatedSpawnerCommands<ChildOf>, tag: impl Bundle) {
    let round = BorderRadius::all(Val::Px(BAR_W / 2.0));
    parent
        .spawn((
            tag,
            ScrollTrack,
            Node {
                width: Val::Px(BAR_W),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                border_radius: round,
                ..default()
            },
            BackgroundColor(palette::SCROLL_TRACK),
            // Hidden until a list says it has more than it shows.
            Visibility::Hidden,
        ))
        .with_children(|track| {
            for part in ScrollPart::ALL {
                let thumb = part == ScrollPart::Thumb;
                track.spawn((
                    part,
                    Node {
                        flex_basis: Val::Px(0.0),
                        flex_grow: Thumb { size: 1.0, at: 0.0 }.grows()[part.index()],
                        min_height: Val::Px(if thumb { min_thumb() } else { 0.0 }),
                        border_radius: round,
                        ..default()
                    },
                    BackgroundColor(if thumb {
                        palette::SCROLL_THUMB
                    } else {
                        Color::NONE
                    }),
                ));
            }
        });
}

/// A track tagged `M`: whether it is drawn, and the parts it holds.
type Tracks<'w, 's, M> =
    Query<'w, 's, (&'static mut Visibility, &'static Children), (With<ScrollTrack>, With<M>)>;

/// The bars tagged `M`, and the parts they are laid out in.
#[derive(SystemParam)]
pub struct ScrollBars<'w, 's, M: Component> {
    tracks: Tracks<'w, 's, M>,
    parts: Query<'w, 's, (&'static ScrollPart, &'static mut Node)>,
}

impl<M: Component> ScrollBars<'_, '_, M> {
    /// Show `window` on every bar tagged `M`: hidden when it all fits,
    /// otherwise the thumb sized and placed from it. Written only on a
    /// change, because a `Node` touched re-runs the layout.
    pub fn show(&mut self, window: ScrollWindow) {
        let thumb = window.thumb();
        for (mut shown, children) in &mut self.tracks {
            let want = match thumb {
                Some(_) => Visibility::Inherited,
                None => Visibility::Hidden,
            };
            if *shown != want {
                *shown = want;
            }
            let Some(thumb) = thumb else {
                continue;
            };
            let grows = thumb.grows();
            for &child in children {
                if let Ok((part, mut node)) = self.parts.get_mut(child) {
                    let grow = grows[part.index()];
                    if node.flex_grow != grow {
                        node.flex_grow = grow;
                    }
                }
            }
        }
    }
}

/// Keep the bar of the list in `S` on that list's window. Registered after
/// the list's input, so a press shows on the frame it moved the rows.
pub fn follow<S: Scrolls>(list: Res<S>, mut bars: ScrollBars<BarOf<S>>) {
    bars.show(list.window());
}

/// What a bar is showing, for tests: `None` when hidden, else the shares
/// of the gap above, the thumb and the gap below.
#[cfg(test)]
pub fn reading<M: Component>(world: &mut World) -> Option<[f32; 3]> {
    let mut tracks =
        world.query_filtered::<(&Visibility, &Children), (With<ScrollTrack>, With<M>)>();
    let (shown, children) = tracks.single(world).expect("one bar");
    if *shown == Visibility::Hidden {
        return None;
    }
    let children: Vec<Entity> = children.iter().collect();
    let mut grows = [0.0; 3];
    for child in children {
        let entity = world.entity(child);
        let part = entity
            .get::<ScrollPart>()
            .expect("a track holds only parts");
        grows[part.index()] = entity.get::<Node>().expect("a part is a node").flex_grow;
    }
    Some(grows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thumb(first: usize, shown: usize, total: usize) -> Option<Thumb> {
        ScrollWindow::rows(first, shown, total).thumb()
    }

    /// A list that fits has no bar at all: not a full-length thumb, which
    /// would still say "scroll bar" to someone looking for more.
    #[test]
    fn a_list_that_fits_has_no_bar() {
        assert_eq!(thumb(0, 12, 0), None, "empty");
        assert_eq!(thumb(0, 12, 1), None, "one row");
        assert_eq!(thumb(0, 12, 11), None, "short");
        assert_eq!(thumb(0, 12, 12), None, "an exact fit");
        assert_eq!(ScrollWindow::NONE.thumb(), None, "not on screen");
        assert!(thumb(0, 12, 13).is_some(), "one row more than fits");
    }

    /// The thumb is as long as the share in view and sits as far down its
    /// travel as the window is down the list: top, middle, bottom.
    #[test]
    fn the_thumb_is_the_share_in_view_at_the_place_reached() {
        let top = thumb(0, 12, 48).expect("overflows");
        assert_eq!(
            top,
            Thumb {
                size: 0.25,
                at: 0.0
            }
        );
        assert_eq!(top.grows(), [0.0, 0.25, 0.75]);

        let middle = thumb(18, 12, 48).expect("overflows");
        assert_eq!(
            middle,
            Thumb {
                size: 0.25,
                at: 0.5
            }
        );
        assert_eq!(middle.grows(), [0.375, 0.25, 0.375]);

        let bottom = thumb(36, 12, 48).expect("overflows");
        assert_eq!(
            bottom,
            Thumb {
                size: 0.25,
                at: 1.0
            }
        );
        assert_eq!(bottom.grows(), [0.75, 0.25, 0.0]);

        // One row more than fits: a long thumb with one step to take.
        assert_eq!(thumb(1, 12, 13).expect("overflows").at, 1.0);
    }

    /// The shares always fill the track, so the gaps and the thumb between
    /// them add up to its length whatever the window.
    #[test]
    fn the_parts_fill_the_track() {
        for total in 13..=99 {
            for first in 0..=total - 12 {
                let grows = thumb(first, 12, total).expect("overflows").grows();
                let sum: f32 = grows.iter().sum();
                assert!((sum - 1.0).abs() < 1e-5, "{first}/{total}: {grows:?}");
                assert!(
                    grows.iter().all(|g| *g >= 0.0),
                    "{first}/{total}: {grows:?}"
                );
            }
        }
    }

    /// A window left past the end, as a list that lost rows under it can
    /// be for a moment, reads as the bottom rather than off the track.
    #[test]
    fn a_window_past_the_end_reads_as_the_bottom() {
        assert_eq!(thumb(30, 14, 20).expect("overflows").at, 1.0);
    }

    /// Whole rows only: one that would end a pixel past the foot is not
    /// counted, one that ends on it (or within the layout's rounding) is.
    #[test]
    fn rows_that_fit_count_whole_rows() {
        // Twelve 34px rows with 4px between take 12 * 34 + 11 * 4 = 452.
        assert_eq!(rows_that_fit(452.0, 34.0, 4.0), 12);
        assert_eq!(rows_that_fit(451.6, 34.0, 4.0), 12, "rounding");
        assert_eq!(rows_that_fit(451.0, 34.0, 4.0), 11);
        assert_eq!(rows_that_fit(489.0, 34.0, 4.0), 12, "not a 13th");
        assert_eq!(rows_that_fit(490.0, 34.0, 4.0), 13);
        assert_eq!(rows_that_fit(20.0, 34.0, 4.0), 0, "not even one");
        assert_eq!(rows_that_fit(400.0, 0.0, 4.0), 0, "not laid out yet");
    }

    /// Both ways of laying rows out beside a bar put them in the middle of
    /// the card they are in, laid out by Bevy's own layout: the bar's room
    /// on the right is kept as much again on the left.
    #[test]
    fn the_rows_sit_in_the_middle_of_their_card() {
        use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
        #[derive(Component)]
        struct Card;
        #[derive(Component)]
        struct Row;
        #[derive(Component)]
        struct Tag;
        #[derive(Component)]
        struct Room;
        let (width, height) = (800u32, 600u32);
        for filled in [false, true] {
            let mut app = App::new();
            app.add_plugins((
                MinimalPlugins,
                TransformPlugin,
                AssetPlugin::default(),
                ImagePlugin::default(),
                bevy::text::TextPlugin,
                bevy::ui::UiPlugin,
            ));
            app.add_plugins((
                bevy::input::InputPlugin,
                WindowPlugin {
                    primary_window: None,
                    exit_condition: bevy::window::ExitCondition::DontExit,
                    ..default()
                },
                bevy::a11y::AccessibilityPlugin,
                DefaultPickingPlugins,
            ));
            app.init_asset::<TextureAtlasLayout>();
            app.world_mut().spawn((
                Camera2d,
                Camera {
                    computed: ComputedCameraValues {
                        target_info: Some(RenderTargetInfo {
                            physical_size: UVec2::new(width, height),
                            scale_factor: 1.0,
                        }),
                        ..default()
                    },
                    viewport: Some(Viewport {
                        physical_size: UVec2::new(width, height),
                        ..default()
                    }),
                    ..default()
                },
            ));
            let build = move |mut commands: Commands| {
                commands
                    .spawn((
                        Card,
                        Node {
                            width: Val::Px(400.0),
                            height: Val::Px(300.0),
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Stretch,
                            ..default()
                        },
                    ))
                    .with_children(|card| {
                        let rows = |rows: &mut RelatedSpawnerCommands<ChildOf>| {
                            rows.spawn((
                                Row,
                                Node {
                                    height: Val::Px(20.0),
                                    ..default()
                                },
                            ));
                        };
                        match filled {
                            true => filling(card, 4.0, Tag, Room, rows),
                            false => beside(card, 4.0, Tag, rows),
                        }
                    });
            };
            app.add_systems(Startup, build);
            for _ in 0..3 {
                app.update();
            }
            let world = app.world_mut();
            let middle = |world: &mut World, entity: Entity| {
                let node = world.get::<ComputedNode>(entity).expect("laid out");
                assert!(node.size.x > 0.0, "laid out");
                world
                    .get::<UiGlobalTransform>(entity)
                    .expect("placed")
                    .translation
                    .x
            };
            let card = world
                .query_filtered::<Entity, With<Card>>()
                .single(world)
                .expect("a card");
            let row = world
                .query_filtered::<Entity, With<Row>>()
                .single(world)
                .expect("a row");
            let (card, row) = (middle(world, card), middle(world, row));
            assert!(
                (card - row).abs() < 0.5,
                "filling {filled}: the row's middle {row}, the card's {card}"
            );
        }
    }

    /// The pixel form, for a viewport: the same arithmetic in pixels.
    #[test]
    fn a_viewport_reads_in_pixels() {
        let window = ScrollWindow {
            first: 50.0,
            shown: 100.0,
            total: 200.0,
        };
        assert_eq!(window.thumb(), Some(Thumb { size: 0.5, at: 0.5 }));
    }
}
