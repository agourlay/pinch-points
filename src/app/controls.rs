//! The key-binding screen (spec §8.4 full remapping): one seat's ten
//! actions at a time, each rebound by selecting it and pressing the key you
//! want. Reached from the Settings row of the same name.
//!
//! Capture is deliberately narrow: only [`binds::bindable`] keys count, so
//! Escape always means "never mind" and the keys the game reads on its own
//! during play ([`binds::GLOBAL_KEYS`]: music, hint, stage skip, restart,
//! round code) stay theirs; and a key already doing another job is refused
//! rather than silently stolen: the two keyboard seats share one keyboard,
//! and a shadowed action is a seat that cannot move.

use crate::app::Screen;
use crate::app::binds::{self, Action, BOUND_SEATS, SeatBinds};
use crate::app::cycle::Turn;
use crate::app::i18n::fill;
use crate::app::menu_ui::{self, Half};
use crate::app::palette;
use crate::app::settings::GameSettings;
use bevy::prelude::*;

/// Rows: the seat picker, then one per action, then reset-to-defaults.
const ROWS: usize = 2 + binds::ACTIONS;
const SEAT_ROW: usize = 0;
const RESET_ROW: usize = ROWS - 1;

/// Which row a list index is.
fn action_at(row: usize) -> Option<Action> {
    (row > SEAT_ROW && row < RESET_ROW).then(|| Action::ALL[row - 1])
}

#[derive(Resource, Default)]
pub struct ControlsMenu {
    pub selected: usize,
    /// Which keyboard seat is being edited.
    pub seat: usize,
    /// The action waiting for a key press, if a capture is running.
    pub capturing: Option<Action>,
    /// Transient line under the list: "press a key", "that key is taken".
    pub feedback: String,
}

#[derive(Component)]
pub struct ControlsUi;

#[derive(Component)]
pub struct ControlsRow(usize);

#[derive(Component)]
pub struct ControlsFeedback;

/// One half of one row.
#[derive(Component)]
pub struct ControlsCell(usize, Half);

/// Action gutter and key gutter, sized in pixels for the widest thing
/// either ever holds in any language: the French "remove all your arrows"
/// on the left, its "press a key" prompt on the right. Measured, and held
/// to, by `every_row_fits_its_cell_in_every_language`; the cells clip
/// silently, and at the old 268 and 196 seven strings did.
const LABEL_W: f32 = 304.0;
const VALUE_W: f32 = 280.0;
/// The row font, which the measurement has to be taken at.
const ROW_FONT: f32 = 19.0;

/// Where a heading falls, and which one: before the seat picker, before
/// the movement keys, before the placement keys, and before the two that
/// clear up after them.
fn heading_before(row: usize, tr: &crate::app::i18n::Tr) -> Option<&'static str> {
    match row {
        SEAT_ROW => Some(tr.ctl_group_seat),
        1 => Some(tr.ctl_group_move),
        5 => Some(tr.ctl_group_place),
        9 => Some(tr.ctl_group_posts),
        _ => None,
    }
}

pub fn enter_controls(
    mut commands: Commands,
    settings: Res<GameSettings>,
    mut menu: ResMut<ControlsMenu>,
) {
    menu.selected = SEAT_ROW;
    menu.capturing = None;
    menu.feedback.clear();
    let tr = settings.tr();
    commands
        .spawn((ControlsUi, menu_ui::between_bars()))
        .with_children(|wrap| {
            wrap.spawn(menu_ui::screen_card()).with_children(|card| {
                for row in 0..ROWS {
                    if let Some(heading) = heading_before(row, tr) {
                        card.spawn(menu_ui::heading(heading, row == SEAT_ROW));
                    }
                    card.spawn((ControlsRow(row), menu_ui::card_row()))
                        .with_children(|line| {
                            line.spawn((
                                ControlsCell(row, Half::Label),
                                menu_ui::cell(LABEL_W, ROW_FONT),
                            ));
                            line.spawn((
                                ControlsCell(row, Half::Value),
                                menu_ui::cell(VALUE_W, ROW_FONT),
                            ));
                        });
                }
            });
            wrap.spawn((
                ControlsFeedback,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(17.0),
                    ..default()
                },
                TextLayout::no_wrap(),
                TextColor(palette::GOLD),
            ));
        });
}

/// Bind `key` to `action` for `seat`, or say who already has it. Pure, so
/// the refusal rule is testable without a keyboard.
pub(crate) fn rebind(
    binds: &mut [SeatBinds; BOUND_SEATS],
    seat: usize,
    action: Action,
    key: KeyCode,
) -> Result<(), (usize, Action)> {
    match binds::conflict(binds, key) {
        // Re-pressing the key an action already has is a no-op, not a clash.
        Some((s, a)) if (s, a) != (seat, action) => Err((s, a)),
        _ => {
            binds[seat].set(action, key);
            Ok(())
        }
    }
}

pub fn controls_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<ControlsMenu>,
    mut settings: ResMut<GameSettings>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    let tr = settings.tr();
    // A running capture swallows everything: the next bindable key lands on
    // the selected action, and Escape backs out.
    if let Some(action) = menu.capturing {
        if keys.just_pressed(KeyCode::Escape) {
            menu.capturing = None;
            menu.feedback.clear();
            return;
        }
        let Some(&key) = keys
            .get_just_pressed()
            .find(|&&k| binds::bindable(k) && !caps.is_global(k))
        else {
            return;
        };
        let seat = menu.seat;
        menu.feedback = match rebind(&mut settings.binds, seat, action, key) {
            Ok(()) => String::new(),
            Err((_, taken_by)) => fill(
                tr.ctl_taken,
                &[
                    ("k", &caps.label(key)),
                    ("a", tr.bind_actions[taken_by.index()]),
                ],
            ),
        };
        menu.capturing = None;
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        next_screen.set(Screen::Settings);
        return;
    }
    if keys.just_pressed(KeyCode::Backspace) {
        settings.binds = binds::default_binds();
        menu.feedback.clear();
        return;
    }
    if menu_ui::enter(&keys) {
        match menu.selected {
            RESET_ROW => {
                settings.binds = binds::default_binds();
                menu.feedback.clear();
            }
            SEAT_ROW => menu.seat = (menu.seat + 1) % BOUND_SEATS,
            row => {
                menu.capturing = action_at(row);
                menu.feedback = tr.ctl_press_key.to_string();
            }
        }
        return;
    }
    menu.selected = menu_ui::nav(&keys, menu.selected, ROWS);
    if let Some(turn) = menu_ui::left_right(&keys)
        && menu.selected == SEAT_ROW
    {
        let step = match turn {
            Turn::Right => 1,
            Turn::Left => BOUND_SEATS - 1, // one backwards, modulo the seats
        };
        menu.seat = (menu.seat + step) % BOUND_SEATS;
    }
}

pub fn update_controls_ui(
    settings: Res<GameSettings>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    menu: Res<ControlsMenu>,
    mut cells: Query<(&ControlsCell, &mut Text, &mut TextColor)>,
    mut rows: Query<(&ControlsRow, &mut BackgroundColor)>,
    mut feedback: Query<&mut Text, (With<ControlsFeedback>, Without<ControlsCell>)>,
) {
    let tr = settings.tr();
    let seat_label = crate::app::seat_label(tr, menu.seat as u8);
    for (cell, mut text, mut color) in &mut cells {
        let picked = cell.0 == menu.selected;
        let (label, value) = match cell.0 {
            SEAT_ROW => (
                tr.ctl_seat.to_string(),
                if picked {
                    format!("< {seat_label} >")
                } else {
                    seat_label.clone()
                },
            ),
            RESET_ROW => (tr.ctl_reset.to_string(), tr.ctl_reset_key.to_string()),
            index => {
                let action = Action::ALL[index - 1];
                let key = settings.binds[menu.seat].key(action);
                // The row being captured shows the prompt in place of its
                // key, so it is obvious which one is listening.
                let value = if menu.capturing == Some(action) {
                    tr.ctl_listening.to_string()
                } else {
                    caps.label(key)
                };
                (tr.bind_actions[action.index()].to_string(), value)
            }
        };
        let line = match cell.1 {
            Half::Label => label,
            Half::Value => value,
        };
        let target = match (cell.1, picked) {
            (Half::Label, true) => Color::WHITE,
            (Half::Label, false) => palette::PARCHMENT.with_alpha(0.62),
            (Half::Value, true) => palette::GOLD,
            (Half::Value, false) => palette::PARCHMENT.with_alpha(0.92),
        };
        menu_ui::set_text(&mut text, &line);
        menu_ui::set_color(&mut color, target);
    }
    for (row, mut fill) in &mut rows {
        let ground = menu_ui::band(row.0 == menu.selected);
        menu_ui::set_bg(&mut fill, ground);
    }
    for mut text in &mut feedback {
        menu_ui::set_text(&mut text, &menu.feedback);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_line_up_with_the_actions() {
        assert_eq!(action_at(SEAT_ROW), None);
        assert_eq!(action_at(RESET_ROW), None);
        assert_eq!(action_at(1), Some(Action::MoveUp));
        assert_eq!(action_at(ROWS - 2), Some(Action::ClearAll));
    }

    /// Every string a cell can hold, at the row font, in every language,
    /// against the cell that holds it. The cells clip silently, so this is
    /// the only thing that says when a translation has outgrown its gutter:
    /// the labels are the action names, the reset row and the seat row;
    /// the values are every key a seat can be bound to as its cap reads
    /// (the widest is a spelled-out key on a bare keyboard), the listening
    /// prompt, the reset key's name, and the seat picker with its arrows.
    #[test]
    fn every_row_fits_its_cell_in_every_language() {
        use crate::app::i18n::metrics::text_px;
        let caps = crate::app::keycaps::KeyCaps::default();
        let keys: Vec<String> = binds::all_bindable().map(|key| caps.label(key)).collect();
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            let mut labels: Vec<String> = tr.bind_actions.iter().map(|s| s.to_string()).collect();
            labels.push(tr.ctl_reset.to_string());
            labels.push(tr.ctl_seat.to_string());
            for label in labels {
                let w = text_px(&label, ROW_FONT);
                assert!(
                    w <= LABEL_W,
                    "{lang:?}: label {label:?} is {w:.1}px, and the cell holds {LABEL_W}"
                );
            }
            let mut values = keys.clone();
            values.push(tr.ctl_listening.to_string());
            values.push(tr.ctl_reset_key.to_string());
            let seat = crate::app::seat_label(tr, BOUND_SEATS as u8 - 1);
            values.push(format!("< {seat} >"));
            for value in values {
                let w = text_px(&value, ROW_FONT);
                assert!(
                    w <= VALUE_W,
                    "{lang:?}: value {value:?} is {w:.1}px, and the cell holds {VALUE_W}"
                );
            }
        }
    }

    /// Rebinding refuses a key another action already owns, including one
    /// owned by the *other* seat, since they share a keyboard, but happily
    /// re-confirms a key on the action that already has it.
    #[test]
    fn rebinding_refuses_to_shadow_another_action() {
        let mut table = binds::default_binds();
        // Seat 1's move-up is W; seat 2 may not take it.
        assert_eq!(
            rebind(&mut table, 1, Action::Remove, KeyCode::KeyW),
            Err((0, Action::MoveUp))
        );
        assert_eq!(table[1].key(Action::Remove), KeyCode::Numpad0, "unchanged");
        // A free key lands.
        assert_eq!(
            rebind(&mut table, 1, Action::Remove, KeyCode::Backquote),
            Ok(())
        );
        assert_eq!(table[1].key(Action::Remove), KeyCode::Backquote);
        // Re-pressing an action's own key is not a clash with itself.
        assert_eq!(
            rebind(&mut table, 1, Action::Remove, KeyCode::Backquote),
            Ok(())
        );
        assert!(binds::all_distinct(&table));
    }
}
