//! Step 1 on screen: the game card and its rows.

use super::*;

pub(super) fn game_card(card: &mut ChildSpawnerCommands, tr: &crate::app::i18n::Tr) {
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
