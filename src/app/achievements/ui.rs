//! The unlock toasts and the achievements browser screen.

use super::{ACHIEVEMENTS, Stats, Unlocked};
use crate::app::i18n::fill;
use crate::app::menu_ui;
use crate::app::palette;
use crate::app::settings::GameSettings;
use bevy::prelude::*;

// --- toasts ----------------------------------------------------------------

#[derive(Component)]
pub struct Toast {
    age: f32,
}

const TOAST_LIFE: f32 = 4.0;

/// Where the first toast sits, and the air between one and the next.
const TOAST_TOP: f32 = 52.0;
const TOAST_GAP: f32 = 6.0;

/// What a toast is assumed to be tall on the frame it appears, before the
/// layout has measured it. Only ever used for one frame, and only to keep
/// the toast under it from starting in the same place.
const TOAST_GUESS: f32 = 64.0;

pub(super) fn spawn_toast(commands: &mut Commands, name: &str, desc: &str) {
    commands
        .spawn((
            Toast { age: 0.0 },
            GlobalZIndex(30),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(52.0),
                right: Val::Px(12.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(palette::TOAST_FILL),
        ))
        .with_children(|toast| {
            toast.spawn((
                Text::new(format!("* {name}")),
                TextFont {
                    font_size: FontSize::Px(menu_ui::type_scale::ROW),
                    ..default()
                },
                TextColor(palette::GOLD),
            ));
            toast.spawn((
                Text::new(desc.to_string()),
                TextFont {
                    font_size: FontSize::Px(menu_ui::type_scale::BODY),
                    ..default()
                },
                TextColor(palette::PARCHMENT.with_alpha(0.9)),
            ));
        });
}

/// Stack visible toasts under the header and expire them.
///
/// Each one is placed under the measured height of the one before it, not
/// under a constant. A toast is as tall as its two lines make it, and the
/// constant that used to space them cleared the real thing by one pixel: a
/// longer trophy name, a bigger font or a language with taller metrics and
/// they would have overlapped, with nothing in the code to say why.
pub fn update_toasts(
    time: Res<Time>,
    mut commands: Commands,
    mut toasts: Query<(Entity, &mut Toast, &mut Node, &ComputedNode)>,
) {
    let mut top = TOAST_TOP;
    for (entity, mut toast, mut node, computed) in &mut toasts {
        toast.age += time.delta_secs();
        if toast.age >= TOAST_LIFE {
            commands.entity(entity).despawn();
            continue;
        }
        let target = Val::Px(top);
        if node.top != target {
            node.top = target;
        }
        // Physical pixels out of the layout, logical pixels into `Val::Px`.
        let height = computed.size().y * computed.inverse_scale_factor();
        top += if height > 0.0 { height } else { TOAST_GUESS } + TOAST_GAP;
    }
}

// --- the browser screen ----------------------------------------------------

#[derive(Component)]
pub struct AchievementsUi;

/// The unlock mark: a filled gold disc when earned, an empty ring when not.
///
/// A disc rather than the star sprite, whose arms come out thinner than a
/// pixel at the 22 the list row allows, and it needs no asset at all.
/// The medal's ink, by what the trophy is for: banking gold, versus red,
/// puzzles green, the daily sky-blue, the editor violet, the gulls white.
/// A wall of fifty gold rows read as one blur; the inks group them
/// the way the difficulty inks group the stage grid.
fn category_ink(id: &str) -> Color {
    match id {
        // Versus: the beach fought over, and who you fought there.
        _ if id.starts_with("win_") || id.starts_with("series_") => palette::INK_RAID,
        _ if id.starts_with("online_") || id.starts_with("rounds_") => palette::INK_RAID,
        _ if id.starts_with("crowd_") || id.starts_with("hosted_") => palette::INK_RAID,
        "dry_castle" | "raids_25" | "all_seats" => palette::INK_RAID,
        // The campaign, and how it was walked rather than that it was.
        _ if id.starts_with("puzzle_") => palette::INK_SURGE,
        "campaign_done" | "beach_done" => palette::INK_SURGE,
        _ if id.starts_with("under_par") || id.starts_with("clean_") => palette::INK_SURGE,
        _ if id.starts_with("deep_") => palette::INK_SURGE,
        _ if id.starts_with("daily_") => palette::INK_TIDE,
        // The editor, and both ends of a level passed around as a code.
        _ if id.starts_with("built_") => palette::INK_LURE,
        "shared_1" | "taken_1" => palette::INK_LURE,
        "fed_25" => palette::PARCHMENT,
        _ => palette::GOLD,
    }
}

fn unlock_mark(done: bool, id: &str) -> (Node, BackgroundColor, BorderColor) {
    let ink = category_ink(id);
    (
        Node {
            width: Val::Px(14.0),
            height: Val::Px(14.0),
            flex_shrink: 0.0,
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(7.0)),
            ..default()
        },
        BackgroundColor(if done { ink } else { Color::NONE }),
        BorderColor::all(if done { ink } else { ink.with_alpha(0.35) }),
    )
}

/// Columns the shelf is dealt into. Two, because a 580px row twice over
/// plus the card's padding is 1216 of the window's 1280: a third column
/// does not fit sideways, and a narrower row would have to give up either
/// the description or the bar.
const COLS: usize = 2;

/// A trophy tile's own height, and the `row_gap` the column puts under it.
/// Named rather than inlined because the viewport height below is derived
/// from them, and a row that changed shape without them would silently
/// start cutting the bottom row in half.
const TILE_HEIGHT: f32 = 32.0;
const TILE_GAP: f32 = 2.0;

/// A trophy row and the gap under it. One press of a scroll key moves
/// exactly this, so the shelf comes to rest on row boundaries instead of
/// drifting half a row out of step with itself.
const ROW_PITCH: f32 = TILE_HEIGHT + TILE_GAP;

/// Rows the viewport shows at once. Fifteen is what fits between the header
/// and the prompt with the count above and the lifetime line below. It is
/// no longer a cap on how many trophies there may be, only on how many are
/// in front of you at once.
const SHELF_ROWS: f32 = 15.0;

/// The viewport height: whole rows, so the shelf never cuts one in half.
/// The last row has no gap under it, hence the one gap taken back off.
const SHELF_HEIGHT: f32 = SHELF_ROWS * ROW_PITCH - TILE_GAP;

/// The scrolling viewport, so the input system can find it.
#[derive(Component)]
pub struct Shelf;

/// The moving part of the scrollbar, sized and placed from where the shelf
/// has got to.
#[derive(Component)]
pub struct ShelfThumb;

/// The scrollbar's width, and the air between it and the rows. Slim: it is
/// there to say the list goes on, not to be dragged.
const BAR_WIDTH: f32 = 6.0;
const BAR_GAP: f32 = 8.0;

/// The shortest the thumb is allowed to get. A list long enough would
/// otherwise shrink it to a dot, which reads as a speck of dirt rather than
/// as a position.
const MIN_THUMB: f32 = 24.0;

/// One trophy's row: the mark, its name, what it asks for, how far along,
/// and a bar saying the same thing at a glance. The bar is the point of the
/// restyle: "292/1000" is a fact you have to read, and a bar a third full
/// is one you can see.
fn spawn_trophy(
    column: &mut ChildSpawnerCommands,
    tr: &crate::app::i18n::Tr,
    index: usize,
    done: bool,
    now: u32,
    goal: u32,
) {
    // A locked trophy is a rumour, not a deed: it drops back further than
    // the earned rows so the shelf reads at a glance.
    let ink = if done {
        palette::SELECTED_ROW
    } else {
        palette::IDLE_ROW.with_alpha(0.55)
    };
    column
        .spawn((
            Node {
                width: Val::Px(580.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            // Earned rows carry a wash of gold behind them, so the screen
            // reads as a shelf with gaps rather than a list with markers.
            BackgroundColor(if done {
                palette::GOLD.with_alpha(0.10)
            } else {
                Color::NONE
            }),
        ))
        .with_children(|tile| {
            tile.spawn(Node {
                column_gap: Val::Px(8.0),
                align_items: AlignItems::Center,
                width: Val::Percent(100.0),
                ..default()
            })
            .with_children(|line| {
                line.spawn(unlock_mark(
                    done,
                    crate::app::achievements::ACHIEVEMENTS[index].id,
                ));
                // A fixed name column, so every description starts at the
                // same x and the eye can run down the list of what to do.
                // Wide enough for the longest name in any of the three
                // languages (French, at twenty-two characters).
                line.spawn((
                    Node {
                        width: Val::Px(204.0),
                        flex_shrink: 0.0,
                        overflow: Overflow::clip_x(),
                        ..default()
                    },
                    children![(
                        Text::new(tr.ach_names[index].to_string()),
                        TextFont {
                            font_size: FontSize::Px(menu_ui::type_scale::BODY),
                            ..default()
                        },
                        TextLayout::no_wrap(),
                        TextColor(ink),
                    )],
                ));
                // `min_width: 0` on purpose: a flex item will not shrink
                // below its content by default, so the longest description
                // would push the count off the end of the row instead of
                // being clipped itself.
                line.spawn((
                    Node {
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        min_width: Val::Px(0.0),
                        overflow: Overflow::clip_x(),
                        ..default()
                    },
                    children![(
                        Text::new(tr.ach_descs[index].to_string()),
                        TextFont {
                            font_size: FontSize::Px(menu_ui::type_scale::FINE),
                            ..default()
                        },
                        TextLayout::no_wrap(),
                        TextColor(palette::PARCHMENT.with_alpha(if done { 0.7 } else { 0.45 })),
                    )],
                ));
                line.spawn((
                    Node {
                        flex_shrink: 0.0,
                        ..default()
                    },
                    children![(
                        Text::new(format!("{now}/{goal}")),
                        TextFont {
                            font_size: FontSize::Px(menu_ui::type_scale::FINE),
                            ..default()
                        },
                        TextLayout::no_wrap(),
                        TextColor(palette::PARCHMENT.with_alpha(if done { 0.7 } else { 0.4 })),
                    )],
                ));
            });
            // The track, and the part of it filled in. `goal` is never zero
            // (a threshold of nothing is not an achievement), but the sim
            // does not enforce that, so the division guards itself.
            let filled = if goal == 0 {
                100.0
            } else {
                (now.min(goal) as f32 / goal as f32) * 100.0
            };
            tile.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(3.0),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(palette::BAR_TRACK),
                children![(
                    Node {
                        width: Val::Percent(filled),
                        height: Val::Percent(100.0),
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(if done {
                        palette::GOLD
                    } else {
                        palette::GOLD.with_alpha(0.45)
                    }),
                )],
            ));
        });
}

pub fn enter_achievements(
    mut commands: Commands,
    settings: Res<GameSettings>,
    stats: Res<Stats>,
    unlocked: Res<Unlocked>,
) {
    let tr = settings.tr();
    let per_column = ACHIEVEMENTS.len().div_ceil(COLS);
    commands
        .spawn((
            AchievementsUi,
            // Between the bars and centred in what is left, the same
            // frame the stage list sits in - which it now literally is.
            // It said so while sitting two pixels inside it.
            crate::app::menu_ui::between_bars(),
        ))
        .with_children(|wrap| {
            // The shared card, with room between the count, the shelf and
            // the key. Built by hand before, with its own padding and no
            // shadow: the same card, sitting differently.
            // `bg` and not `fill`: this file's `fill` is the one that puts
            // words into a translated string, and it is used all through
            // the card below.
            let (mark, mut node, bg, edge, shadow) = crate::app::menu_ui::screen_card();
            node.row_gap = Val::Px(8.0);
            wrap.spawn((mark, node, bg, edge, shadow))
                .with_children(|card| {
                    let earned = ACHIEVEMENTS
                        .iter()
                        .filter(|a| unlocked.0.contains(a.id))
                        .count();
                    card.spawn((
                        Text::new(fill(
                            tr.ach_progress,
                            &[
                                ("a", &earned.to_string()),
                                ("b", &ACHIEVEMENTS.len().to_string()),
                            ],
                        )),
                        TextFont {
                            font_size: FontSize::Px(menu_ui::type_scale::ROW),
                            ..default()
                        },
                        TextColor(palette::PARCHMENT.with_alpha(0.75)),
                    ));
                    // The shelf and its scrollbar ride together: a list that
                    // stops at a clean row boundary looks finished, and the
                    // prompt in the corner is not where anyone is looking.
                    card.spawn(Node {
                        column_gap: Val::Px(BAR_GAP),
                        align_items: AlignItems::FlexStart,
                        ..default()
                    })
                    .with_children(|band| {
                        band.spawn((
                            Shelf,
                            Node {
                                // The viewport. Its child is as tall as the
                                // trophies make it and slides behind this.
                                max_height: Val::Px(SHELF_HEIGHT),
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                            ScrollPosition::default(),
                        ))
                        .with_children(|shelf| {
                            shelf
                                .spawn(Node {
                                    column_gap: Val::Px(16.0),
                                    ..default()
                                })
                                .with_children(|grid| {
                                    for chunk in 0..COLS {
                                        grid.spawn(Node {
                                            flex_direction: FlexDirection::Column,
                                            row_gap: Val::Px(2.0),
                                            ..default()
                                        })
                                        .with_children(
                                            |column| {
                                                let first = chunk * per_column;
                                                let last =
                                                    (first + per_column).min(ACHIEVEMENTS.len());
                                                for (index, achievement) in ACHIEVEMENTS
                                                    .iter()
                                                    .enumerate()
                                                    .take(last)
                                                    .skip(first)
                                                {
                                                    let done = unlocked.0.contains(achievement.id);
                                                    let (now, goal) = achievement.progress(&stats);
                                                    spawn_trophy(
                                                        column, tr, index, done, now, goal,
                                                    );
                                                }
                                            },
                                        );
                                    }
                                });
                        });
                        // The track, and the thumb that says how far down the
                        // shelf you are. Placed by `update_shelf_scrollbar`,
                        // which is the only thing that knows how tall the rows
                        // turned out to be.
                        band.spawn((
                            Node {
                                width: Val::Px(BAR_WIDTH),
                                height: Val::Px(SHELF_HEIGHT),
                                flex_shrink: 0.0,
                                border_radius: BorderRadius::all(Val::Px(BAR_WIDTH / 2.0)),
                                ..default()
                            },
                            BackgroundColor(palette::PARCHMENT.with_alpha(0.12)),
                        ))
                        .with_children(|track| {
                            track.spawn((
                                ShelfThumb,
                                Node {
                                    position_type: PositionType::Absolute,
                                    width: Val::Px(BAR_WIDTH),
                                    height: Val::Px(SHELF_HEIGHT),
                                    border_radius: BorderRadius::all(Val::Px(BAR_WIDTH / 2.0)),
                                    ..default()
                                },
                                BackgroundColor(palette::GOLD.with_alpha(0.7)),
                            ));
                        });
                    });
                    let footer = fill(
                        tr.stats_footer,
                        &[
                            ("banked", &stats.banked.to_string()),
                            ("golden", &stats.golden.to_string()),
                            ("wins", &stats.wins.to_string()),
                            ("rounds", &stats.rounds.to_string()),
                            ("puzzles", &stats.puzzles.to_string()),
                        ],
                    );
                    card.spawn((
                        Text::new(footer),
                        TextFont {
                            font_size: FontSize::Px(menu_ui::type_scale::BODY),
                            ..default()
                        },
                        TextColor(palette::PARCHMENT.with_alpha(0.6)),
                    ));
                });
        });
}

pub fn achievements_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut next_screen: ResMut<NextState<crate::app::Screen>>,
    mut shelf: Query<(&mut ScrollPosition, &ComputedNode), With<Shelf>>,
) {
    if keys.just_pressed(KeyCode::Escape) || crate::app::menu_ui::enter(&keys) {
        next_screen.set(crate::app::Screen::Menu);
        return;
    }
    let pressed = |a, b| keys.just_pressed(a) || keys.just_pressed(b);
    let mut step = 0.0;
    if pressed(KeyCode::KeyS, KeyCode::ArrowDown) {
        step += ROW_PITCH;
    }
    if pressed(KeyCode::KeyW, KeyCode::ArrowUp) {
        step -= ROW_PITCH;
    }
    if step == 0.0 {
        return;
    }
    for (mut position, computed) in &mut shelf {
        let scale = computed.inverse_scale_factor();
        position.0.y = scrolled(
            position.0.y,
            step,
            computed.size().y * scale,
            computed.content_size().y * scale,
        );
    }
}

/// Size and place the thumb from where the shelf has got to.
///
/// Runs every frame the screen is up rather than on a scroll message: the
/// rows are measured by the layout a frame after they are spawned, so the
/// first honest answer is not available at the moment the shelf is built.
pub fn update_shelf_scrollbar(
    shelf: Query<(&ScrollPosition, &ComputedNode), With<Shelf>>,
    mut thumb: Query<&mut Node, With<ShelfThumb>>,
) {
    let Ok((position, computed)) = shelf.single() else {
        return;
    };
    let scale = computed.inverse_scale_factor();
    let (height, top) = thumb_bounds(
        computed.size().y * scale,
        computed.content_size().y * scale,
        position.0.y,
    );
    for mut node in &mut thumb {
        let (height, top) = (Val::Px(height), Val::Px(top));
        // Written only on a change: `Node` is change-detected and the
        // layout runs again for anything that touches it, every frame.
        if node.height != height {
            node.height = height;
        }
        if node.top != top {
            node.top = top;
        }
    }
}

/// How tall the thumb is and how far down the track it sits.
///
/// Its height is the fraction of the shelf you can see, which is the part
/// that says there is more; its position is how far through you are. A
/// shelf shorter than its viewport fills the track, which is the truth: all
/// of it is in front of you.
fn thumb_bounds(viewport: f32, content: f32, scroll: f32) -> (f32, f32) {
    if content <= viewport || viewport <= 0.0 {
        return (SHELF_HEIGHT, 0.0);
    }
    let height = (SHELF_HEIGHT * (viewport / content)).max(MIN_THUMB);
    let travel = (SHELF_HEIGHT - height).max(0.0);
    let progress = (scroll / (content - viewport)).clamp(0.0, 1.0);
    (height, travel * progress)
}

/// Where a scroll of `step` lands, given what the viewport shows and how
/// tall the shelf behind it is.
///
/// Clamped here rather than left to the layout, which clamps what it
/// *draws* and leaves the component alone. An unclamped component runs on
/// past the end of the list, and then the first several presses back the
/// other way move nothing at all, which reads as a stuck key.
fn scrolled(from: f32, step: f32, viewport: f32, content: f32) -> f32 {
    (from + step).clamp(0.0, (content - viewport).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shelf stops at both ends, and stops *at* them: a component left
    /// to run past the bottom would take several presses to start moving
    /// back up, which feels like a dropped key rather than a limit.
    #[test]
    fn the_shelf_stops_at_both_ends() {
        // Ten rows of content behind a viewport showing four of them.
        let viewport = 4.0 * ROW_PITCH;
        let content = 10.0 * ROW_PITCH;
        let bottom = content - viewport;

        assert_eq!(scrolled(0.0, -ROW_PITCH, viewport, content), 0.0, "at rest");
        assert_eq!(scrolled(0.0, ROW_PITCH, viewport, content), ROW_PITCH);
        assert_eq!(
            scrolled(bottom, ROW_PITCH, viewport, content),
            bottom,
            "the last row is the last row"
        );
        // And one press back from the bottom moves exactly one row, which
        // is what an unclamped component would get wrong.
        assert_eq!(
            scrolled(bottom, -ROW_PITCH, viewport, content),
            bottom - ROW_PITCH
        );

        // A list shorter than its viewport does not scroll at all.
        assert_eq!(scrolled(0.0, ROW_PITCH, viewport, viewport - 10.0), 0.0);
    }

    /// The thumb is the affordance: its height says how much of the shelf
    /// is in front of you, which is the part that says there is more, and
    /// its position says how far through you are.
    #[test]
    fn the_thumb_reports_how_much_is_hidden() {
        // Twice as much shelf as viewport: half a track of thumb.
        let (height, top) = thumb_bounds(100.0, 200.0, 0.0);
        assert_eq!(height, SHELF_HEIGHT / 2.0);
        assert_eq!(top, 0.0, "at the top");

        // Scrolled to the end, the thumb sits at the end of its travel.
        let (height, top) = thumb_bounds(100.0, 200.0, 100.0);
        assert_eq!(top, SHELF_HEIGHT - height, "flush with the bottom");

        // Halfway is halfway.
        let (_, top) = thumb_bounds(100.0, 200.0, 50.0);
        assert_eq!(top, (SHELF_HEIGHT - SHELF_HEIGHT / 2.0) / 2.0);

        // A shelf that fits fills the track: all of it is in front of you.
        assert_eq!(thumb_bounds(100.0, 80.0, 0.0), (SHELF_HEIGHT, 0.0));

        // And a very long shelf keeps a thumb you can still see, without
        // letting it run off the end of the track.
        let (height, top) = thumb_bounds(100.0, 100_000.0, 100_000.0);
        assert_eq!(height, MIN_THUMB);
        assert!(top + height <= SHELF_HEIGHT, "{top} + {height}");

        // Before the layout has measured anything, nothing is claimed.
        assert_eq!(thumb_bounds(0.0, 0.0, 0.0), (SHELF_HEIGHT, 0.0));
    }

    /// The viewport is whole rows, so the shelf never rests showing half of
    /// one. Fifteen tiles and the fourteen gaps between them.
    #[test]
    fn the_viewport_is_whole_rows() {
        assert_eq!(
            SHELF_HEIGHT,
            SHELF_ROWS * TILE_HEIGHT + (SHELF_ROWS - 1.0) * TILE_GAP
        );
    }
}
