//! The editor's side panel: the kind toggle, the brush palette with its
//! sprites and letters, and the line saying what the cursor stands on.

use super::EditorState;
use super::brush::{Brush, standing_on};
use crate::app::Sim;
use crate::app::cursor::Cursor;
use crate::app::i18n::fill;
use crate::app::menu_ui;
use crate::app::settings::GameSettings;
use crate::sim::LevelKind;
use bevy::prelude::*;

#[derive(Component)]
pub struct EditorUi;

/// One row of the palette, by its place in [`Brush::ALL`].
#[derive(Component)]
pub struct PaletteRow(usize);

#[derive(Component)]
pub struct PaletteLabel(usize);

/// One of the two rows of the kind toggle, by what it selects.
#[derive(Component)]
pub struct KindRow(LevelKind);

/// What the cursor is standing on, which is the other half of knowing what
/// a keypress is about to do.
#[derive(Component)]
pub struct UnderCursor;

/// The brush palette: every paintable thing, its sprite, and the letter
/// that loads it.
///
/// The editor used to say `R C H L W P B G O tiles` along the bottom and
/// leave the rest to memory, which is a poor deal for a screen whose whole
/// job is making things.
pub fn spawn_editor_ui(
    mut commands: Commands,
    art: Res<crate::app::art::Art>,
    settings: Res<GameSettings>,
) {
    let tr = settings.tr();
    commands
        .spawn((
            EditorUi,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(70.0),
                right: Val::Px(16.0),
                width: Val::Px(190.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::all(Val::Px(10.0)),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(12.0)),
                ..default()
            },
            BorderColor::all(crate::app::palette::CARD_EDGE),
            BackgroundColor(crate::app::palette::CARD_FILL),
            crate::app::menu_ui::card_shadow(),
        ))
        .with_children(|panel| {
            // What is being built, above the brushes and lit like them: the
            // choice decides which list the level joins, and a toggle you
            // cannot see the state of is a toggle you press to find out.
            panel.spawn((
                Text::new(tr.ed_kind_row.to_string()),
                TextFont {
                    font_size: FontSize::Px(menu_ui::type_scale::FINE),
                    ..default()
                },
                TextColor(crate::app::palette::PARCHMENT.with_alpha(0.55)),
            ));
            for kind in [LevelKind::Puzzle, LevelKind::Arena] {
                panel
                    .spawn((
                        KindRow(kind),
                        Node {
                            align_items: AlignItems::Center,
                            padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                            border_radius: BorderRadius::all(Val::Px(6.0)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .with_children(|row| {
                        row.spawn((
                            Text::new(match kind {
                                LevelKind::Puzzle => tr.ed_kind_puzzle.to_string(),
                                LevelKind::Arena => tr.ed_kind_arena.to_string(),
                            }),
                            TextFont {
                                font_size: FontSize::Px(menu_ui::type_scale::BODY),
                                ..default()
                            },
                            TextColor(crate::app::palette::PARCHMENT),
                        ));
                    });
            }
            panel.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(1.0),
                    margin: UiRect::axes(Val::Px(0.0), Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(crate::app::palette::PARCHMENT.with_alpha(0.14)),
            ));
            for (i, brush) in Brush::ALL.iter().enumerate() {
                panel
                    .spawn((
                        PaletteRow(i),
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(8.0),
                            padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                            border_radius: BorderRadius::all(Val::Px(6.0)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .with_children(|row| {
                        row.spawn((
                            ImageNode::new(brush.icon(&art)),
                            Node {
                                width: Val::Px(22.0),
                                height: Val::Px(22.0),
                                ..default()
                            },
                        ));
                        row.spawn((
                            PaletteLabel(i),
                            Text::new(format!("{}  {}", brush.letter(), brush.label(tr))),
                            TextFont {
                                font_size: FontSize::Px(menu_ui::type_scale::BODY),
                                ..default()
                            },
                            TextColor(crate::app::palette::PARCHMENT),
                        ));
                    });
            }
            panel.spawn((
                UnderCursor,
                Text::new(String::new()),
                TextFont {
                    font_size: FontSize::Px(menu_ui::type_scale::BODY),
                    ..default()
                },
                TextColor(crate::app::palette::IDLE_ROW),
                Node {
                    margin: UiRect::top(Val::Px(8.0)),
                    ..default()
                },
            ));
        });
}

/// Light the loaded brush and the chosen kind, and say what the cursor is
/// standing on.
#[allow(clippy::too_many_arguments)]
pub fn update_editor_palette(
    state: Res<EditorState>,
    sim: Res<Sim>,
    settings: Res<GameSettings>,
    cursors: Query<&Cursor>,
    mut rows: Query<(&PaletteRow, &mut BackgroundColor)>,
    mut kinds: Query<(&KindRow, &mut BackgroundColor), Without<PaletteRow>>,
    mut labels: Query<(&PaletteLabel, &mut TextColor)>,
    mut under: Query<&mut Text, With<UnderCursor>>,
) {
    let tr = settings.tr();
    for (row, mut fill) in &mut kinds {
        let want = match row.0 == state.kind {
            true => crate::app::palette::SELECTED_ROW.with_alpha(0.22),
            false => Color::NONE,
        };
        crate::app::menu_ui::set_bg(&mut fill, want);
    }
    let at = Brush::ALL.iter().position(|b| *b == state.brush);
    for (row, mut fill) in &mut rows {
        let want = match Some(row.0) == at {
            true => crate::app::palette::SELECTED_ROW.with_alpha(0.22),
            false => Color::NONE,
        };
        crate::app::menu_ui::set_bg(&mut fill, want);
    }
    for (label, mut color) in &mut labels {
        let want = match Some(label.0) == at {
            true => crate::app::palette::SELECTED_ROW,
            false => crate::app::palette::PARCHMENT,
        };
        crate::app::menu_ui::set_color(&mut color, want);
    }
    let Some(cursor) = cursors.iter().next() else {
        return;
    };
    let board = &sim.0;
    let standing = standing_on(board, cursor.x, cursor.y).label(tr);
    for mut text in &mut under {
        crate::app::menu_ui::set_text(&mut text, &fill(tr.ed_under, &[("t", standing)]));
    }
}
