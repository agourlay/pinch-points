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
/// A wall of thirty-two gold rows read as one blur; the inks group them
/// the way the difficulty inks group the stage grid.
fn category_ink(id: &str) -> Color {
    match id {
        _ if id.starts_with("win_") || id.starts_with("series_") => palette::INK_RAID,
        "dry_castle" | "online_1" | "rounds_100" => palette::INK_RAID,
        _ if id.starts_with("puzzle_") => palette::INK_SURGE,
        "campaign_done" | "beach_done" => palette::INK_SURGE,
        _ if id.starts_with("daily_") => palette::INK_TIDE,
        _ if id.starts_with("built_") => palette::INK_LURE,
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

/// Trophies per column. Thirty-two in one column ran off the bottom of the
/// screen; two columns of sixteen fit between the header and the prompt
/// with room for the count and the lifetime line. Two columns is also why
/// there are thirty-two rather than thirty-one.
const COLS: usize = 2;

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
            // Between the bars and centred in what is left, the same frame
            // the stage list sits in.
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(50.0),
                bottom: Val::Px(50.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|wrap| {
            wrap.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(8.0),
                    padding: UiRect::axes(Val::Px(20.0), Val::Px(14.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(16.0)),
                    ..default()
                },
                BackgroundColor(palette::CARD_FILL),
                crate::app::menu_ui::ShoreCard,
                BorderColor::all(palette::CARD_EDGE),
            ))
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
                card.spawn(Node {
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
                        .with_children(|column| {
                            let first = chunk * per_column;
                            let last = (first + per_column).min(ACHIEVEMENTS.len());
                            for (index, achievement) in
                                ACHIEVEMENTS.iter().enumerate().take(last).skip(first)
                            {
                                let done = unlocked.0.contains(achievement.id);
                                let (now, goal) = achievement.progress(&stats);
                                spawn_trophy(column, tr, index, done, now, goal);
                            }
                        });
                    }
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
) {
    if keys.just_pressed(KeyCode::Escape) || crate::app::menu_ui::enter(&keys) {
        next_screen.set(crate::app::Screen::Menu);
    }
}
