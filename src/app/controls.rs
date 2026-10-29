//! The key-binding screen (spec §8.4 full remapping): one seat's ten
//! actions at a time, each rebound by selecting it and pressing the key you
//! want. Reached from the Settings row of the same name.
//!
//! Capture is deliberately narrow: only [`binds::bindable`] keys count, so
//! Escape always means "never mind" and the keys the game reads on its own
//! during play ([`binds::GLOBAL_KEYS`]) stay theirs. A key already doing
//! another job is refused rather than silently stolen: the two keyboard
//! seats share one keyboard, and a shadowed action is a seat that cannot
//! move.

use crate::app::Screen;
use crate::app::binds::{self, Action, ActionGroup, BOUND_SEATS, SeatBinds};
use crate::app::cursor::{KeyDoing, key_doing, key_for};
use crate::app::cycle::Turn;
use crate::app::i18n::fill;
use crate::app::menu_ui::{self, Half};
use crate::app::palette;
use crate::app::settings::{CommitScheme, GameSettings};
use bevy::prelude::*;

/// Rows: the seat picker, then one per action, then reset-to-defaults.
const ROWS: usize = 2 + binds::ACTIONS;

/// What a row of the list is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ControlsLine {
    /// The seat picker, first.
    Seat,
    /// One action's key, in [`Action::ALL`] order.
    Bind(Action),
    /// Reset-to-defaults, last.
    Reset,
}

impl ControlsLine {
    /// The line at list index `row`.
    fn at(row: usize) -> ControlsLine {
        match row {
            0 => ControlsLine::Seat,
            row if row <= binds::ACTIONS => ControlsLine::Bind(Action::ALL[row - 1]),
            _ => ControlsLine::Reset,
        }
    }
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
    /// A reset asked for and waiting for its second press. Every binding
    /// of both seats goes at once, and X on a pad is one thumb away from
    /// A, so one press only asks.
    pub reset: menu_ui::SecondPress,
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
/// on the left, its "press a key" prompt on the right. Held to by
/// `every_row_fits_its_cell_in_every_language`, since the cells clip
/// silently: at 268 and 196 seven strings did.
const LABEL_W: f32 = 304.0;
const VALUE_W: f32 = 280.0;
/// The row font, which the measurement has to be taken at.
const ROW_FONT: f32 = 19.0;

/// Where a heading falls, and which one: before the seat picker, and
/// before the first action of each group (the movement keys, the placement
/// keys, and the two that clear up after them). Read off the actions'
/// groups, so a key added to a group lands under its heading.
fn heading_before(row: usize, tr: &crate::app::i18n::Tr) -> Option<&'static str> {
    let group = match ControlsLine::at(row) {
        ControlsLine::Seat => return Some(tr.ctl_group_seat),
        ControlsLine::Reset => return None,
        ControlsLine::Bind(action) => action.group(),
    };
    let continues = matches!(
        ControlsLine::at(row - 1),
        ControlsLine::Bind(above) if above.group() == group
    );
    if continues {
        return None;
    }
    Some(match group {
        ActionGroup::Move => tr.ctl_group_move,
        ActionGroup::Place => tr.ctl_group_place,
        ActionGroup::Posts => tr.ctl_group_posts,
    })
}

pub fn enter_controls(
    mut commands: Commands,
    settings: Res<GameSettings>,
    mut menu: ResMut<ControlsMenu>,
) {
    menu.selected = 0;
    menu.capturing = None;
    menu.feedback.clear();
    menu.reset = menu_ui::SecondPress::default();
    let tr = settings.tr();
    commands
        .spawn((ControlsUi, menu_ui::between_bars()))
        .with_children(|wrap| {
            wrap.spawn(menu_ui::screen_card()).with_children(|card| {
                for row in 0..ROWS {
                    if let Some(heading) = heading_before(row, tr) {
                        card.spawn(menu_ui::heading(heading, row == 0));
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

/// The scheme the screen lists and rebinds under: the one set in
/// Settings, as a shared keyboard reads it ([`GameSettings::commit_in_play`]).
/// The mirrored scheme is the same either way. The one-hand preset only
/// holds with nobody on the second seat's keys, and the place keys it
/// stands down to in versus and co-op are P1's own, which nothing else
/// lists or rebinds; so the place rows are those, and the preset's IJKL
/// is named beside them ([`alone_note`]). One rule for the rows, the
/// capture and the refusal, and the HUD's "your keys" line points here.
fn listed_scheme(settings: &GameSettings) -> CommitScheme {
    settings.commit_in_play(true)
}

/// Which slot of `seat`'s keys does the job `action` names under
/// `commit`: the rule the cursor reads ([`key_doing`]), so a row lists
/// and rebinds the key that really does its job.
fn job_slot(commit: CommitScheme, seat: usize, action: Action) -> KeyDoing {
    key_doing(seat as u8, commit, action)
}

/// The key a row shows: the one that does its job ([`job_slot`]) under
/// the listed scheme ([`listed_scheme`]).
fn shown_key(settings: &GameSettings, seat: usize, action: Action) -> KeyCode {
    key_for(settings, seat as u8, listed_scheme(settings), action)
}

/// The key the one-hand preset gives `action` for `seat` when P1 plays
/// alone, where the row shows another: the line under the list while the
/// row is picked, so the screen names both keys that place.
fn alone_note(
    settings: &GameSettings,
    caps: &crate::app::keycaps::KeyCaps,
    seat: usize,
    action: Action,
) -> Option<String> {
    let KeyDoing::Preset(key) = key_doing(seat as u8, settings.commit, action) else {
        return None;
    };
    Some(alone_line(settings.tr(), caps, key, action))
}

/// What `key` does for P1 alone under the one-hand preset: `action`.
fn alone_line(
    tr: &crate::app::i18n::Tr,
    caps: &crate::app::keycaps::KeyCaps,
    key: KeyCode,
    action: Action,
) -> String {
    fill(
        tr.ctl_preset,
        &[
            ("k", &caps.label(key)),
            ("a", tr.bind_actions[action.index()]),
            ("s", tr.set_commit_keys),
        ],
    )
}

/// The refusal of `key` on `seat`'s page, held by `held_by` for `job`:
/// the seat is named when it is not the page's, whose rows do not list
/// the key.
fn taken_line(
    tr: &crate::app::i18n::Tr,
    caps: &crate::app::keycaps::KeyCaps,
    seat: usize,
    held_by: usize,
    key: KeyCode,
    job: Action,
) -> String {
    let (k, a) = (caps.label(key), tr.bind_actions[job.index()]);
    if held_by == seat {
        fill(tr.ctl_taken, &[("k", &k), ("a", a)])
    } else {
        let p = crate::app::seat_label(tr, held_by as u8);
        fill(tr.ctl_taken_by, &[("k", &k), ("a", a), ("p", &p)])
    }
}

/// Why a key was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Taken {
    /// That seat's key for that job.
    Bound(usize, Action),
    /// The one-hand preset's key for that job of P1's, when P1 plays
    /// alone ([`crate::app::cursor::preset_clash`]).
    Alone(Action),
}

/// Bind `key` to the job `action` for `seat`, with `set` the scheme set
/// in Settings: into the slot that does that job under the listed scheme
/// ([`listed_scheme`], [`job_slot`]), and a refusal names the job the
/// clashing key does. The mirrored scheme's trade is its own inverse, so
/// the slot holding a key and the job it does read through the same
/// rule. A key the one-hand preset places with is refused to a slot P1
/// still reads alone: the one press would do both jobs.
pub(crate) fn rebind_job(
    binds: &mut [SeatBinds; BOUND_SEATS],
    set: CommitScheme,
    seat: usize,
    action: Action,
    key: KeyCode,
) -> Result<(), Taken> {
    let commit = set.in_play(true);
    let job = |seat: usize, slot: Action| match job_slot(commit, seat, slot) {
        KeyDoing::Bound(job) => job,
        // The listed scheme sets no preset; a slot names itself.
        KeyDoing::Preset(_) => slot,
    };
    match job_slot(commit, seat, action) {
        KeyDoing::Bound(slot) => {
            if crate::app::cursor::slot_read(seat as u8, set, slot)
                && let Some(placed) = crate::app::cursor::preset_job(seat as u8, set, key)
            {
                return Err(Taken::Alone(placed));
            }
            rebind(binds, seat, slot, key).map_err(|(s, taken)| Taken::Bound(s, job(s, taken)))
        }
        // The listed scheme sets no preset; nothing to bind.
        KeyDoing::Preset(_) => Ok(()),
    }
}

/// The line a first reset press puts up: the key that takes it up, or
/// for a pad's player the controls table's button for it.
fn reset_question(tr: &crate::app::i18n::Tr, device: crate::app::gamepad::LastDevice) -> String {
    fill(tr.ctl_reset_confirm, &[("k", &reset_control(tr, device))])
}

/// What resets every binding, as the player's hands know it: the key at
/// a keyboard, the controls table's button on a pad. The reset row and
/// the question it asks both name it.
fn reset_control(tr: &crate::app::i18n::Tr, device: crate::app::gamepad::LastDevice) -> String {
    use crate::app::gamepad::{Face, scheme};
    scheme::spelled(
        tr,
        device,
        &Face::Controls,
        scheme::Does::Defaults,
        tr.ctl_reset_key,
    )
}

pub fn controls_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<ControlsMenu>,
    mut settings: ResMut<GameSettings>,
    (caps, device, time): (
        Res<crate::app::keycaps::KeyCaps>,
        Res<crate::app::gamepad::LastDevice>,
        Res<Time>,
    ),
    bridged: Res<crate::app::gamepad::Bridged>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    let tr = settings.tr();
    let now = time.elapsed_secs();
    // A reset asked and not taken up in time is dropped, and its question
    // with it.
    if menu.reset.lapse(now) {
        menu.feedback.clear();
    }
    // A running capture swallows everything: the next bindable key lands on
    // the selected action, and Escape backs out. A key the pad bridge
    // pressed is not one: the d-pad comes through as W/S/A/D, and a pad
    // player who touched it bound a key they never pressed.
    if let Some(action) = menu.capturing {
        if keys.just_pressed(KeyCode::Escape) {
            menu.capturing = None;
            menu.feedback.clear();
            return;
        }
        let Some(&key) = keys
            .get_just_pressed()
            .find(|&&k| binds::bindable(k) && !caps.is_global(k) && !bridged.holds(k))
        else {
            return;
        };
        let seat = menu.seat;
        let set = settings.commit;
        menu.feedback = match rebind_job(&mut settings.binds, set, seat, action, key) {
            Ok(()) => String::new(),
            Err(Taken::Alone(placed)) => alone_line(tr, &caps, key, placed),
            Err(Taken::Bound(held_by, taken_by)) => {
                taken_line(tr, &caps, seat, held_by, key, taken_by)
            }
        };
        menu.capturing = None;
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        next_screen.set(Screen::Settings);
        return;
    }
    // Backspace anywhere, or Enter on the reset row: asked once, done on
    // a second press inside the window.
    let resets = keys.just_pressed(KeyCode::Backspace)
        || (menu_ui::enter(&keys) && ControlsLine::at(menu.selected) == ControlsLine::Reset);
    if resets {
        match menu.reset.press(now, ()) {
            true => {
                settings.binds = binds::default_binds();
                menu.feedback.clear();
            }
            false => menu.feedback = reset_question(tr, *device),
        }
        return;
    }
    // Anything else pressed takes the question back.
    if keys.get_just_pressed().next().is_some() && menu.reset.take_back() {
        menu.feedback.clear();
    }
    if menu_ui::enter(&keys) {
        match ControlsLine::at(menu.selected) {
            // Taken above, as the second of two presses.
            ControlsLine::Reset => {}
            ControlsLine::Seat => menu.seat = (menu.seat + 1) % BOUND_SEATS,
            ControlsLine::Bind(action) => {
                menu.capturing = Some(action);
                menu.feedback = tr.ctl_press_key.to_string();
            }
        }
        return;
    }
    menu.selected = menu_ui::nav(&keys, menu.selected, ROWS);
    if let Some(turn) = menu_ui::left_right(&keys)
        && ControlsLine::at(menu.selected) == ControlsLine::Seat
    {
        let step = match turn {
            Turn::Right => 1,
            Turn::Left => BOUND_SEATS - 1, // one backwards, modulo the seats
        };
        menu.seat = (menu.seat + step) % BOUND_SEATS;
    }
}

pub fn update_controls_ui(
    (settings, device): (Res<GameSettings>, Res<crate::app::gamepad::LastDevice>),
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
        let (label, value) = match ControlsLine::at(cell.0) {
            ControlsLine::Seat => (
                tr.ctl_seat.to_string(),
                if picked {
                    format!("< {seat_label} >")
                } else {
                    seat_label.clone()
                },
            ),
            // The key that resets, or for a pad's player the controls
            // table's button for it: what the question it asks names.
            ControlsLine::Reset => (tr.ctl_reset.to_string(), reset_control(tr, *device)),
            ControlsLine::Bind(action) => {
                let key = shown_key(&settings, menu.seat, action);
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
        let target = menu_ui::cell_ink(cell.1, picked);
        menu_ui::set_text(&mut text, &line);
        menu_ui::set_color(&mut color, target);
    }
    for (row, mut fill) in &mut rows {
        let ground = menu_ui::band(row.0 == menu.selected);
        menu_ui::set_bg(&mut fill, ground);
    }
    // Nothing to say, on a row the one-hand preset also places from:
    // the key that does it when P1 plays alone.
    let note = match ControlsLine::at(menu.selected) {
        ControlsLine::Bind(action) if menu.feedback.is_empty() && menu.capturing.is_none() => {
            alone_note(&settings, &caps, menu.seat, action)
        }
        ControlsLine::Bind(_) | ControlsLine::Seat | ControlsLine::Reset => None,
    };
    let line = note.as_deref().unwrap_or(&menu.feedback);
    for mut text in &mut feedback {
        menu_ui::set_text(&mut text, line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pad player listening for a key who touches the d-pad binds
    /// nothing: the bridge's W/S/A/D are the pad's, not a key pressed.
    #[test]
    fn a_capture_ignores_the_pads_bridged_keys() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::Controls));
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::gamepad::Bridged>();
        app.init_resource::<crate::app::gamepad::PadPick>();
        app.init_resource::<crate::app::gamepad::LastDevice>();
        app.init_resource::<Time>();
        app.insert_resource(crate::app::gamepad::PadFace(
            crate::app::gamepad::Face::Controls,
        ));
        app.init_resource::<GameSettings>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.insert_resource(ControlsMenu {
            capturing: Some(Action::MoveUp),
            ..ControlsMenu::default()
        });
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        app.add_systems(
            Update,
            (crate::app::gamepad::pad_menu_bridge, controls_input).chain(),
        );
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("pad");
        gamepad.digital_mut().press(GamepadButton::DPadLeft);
        app.update();
        let menu = app.world().resource::<ControlsMenu>();
        assert_eq!(menu.capturing, Some(Action::MoveUp), "still listening");
        assert_eq!(
            app.world().resource::<GameSettings>().binds,
            binds::default_binds(),
            "nothing bound"
        );
    }

    /// Backspace, or a pad's X through the bridge, only asks: the second
    /// press inside the window resets every binding, one after it asks
    /// again, and the question names the hands' own control.
    #[test]
    fn a_reset_asks_before_it_wipes_the_bindings() {
        use crate::app::gamepad::{Bridged, Face, LastDevice, PadFace, PadPick};
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::Controls));
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<Bridged>();
        app.init_resource::<PadPick>();
        app.init_resource::<Time>();
        app.init_resource::<LastDevice>();
        app.insert_resource(PadFace(Face::Controls));
        app.init_resource::<GameSettings>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<ControlsMenu>();
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        app.add_systems(
            Update,
            (
                crate::app::gamepad::pad_menu_bridge,
                controls_input,
                crate::app::gamepad::pad_bridge_release,
            )
                .chain(),
        );
        let rebound = |app: &mut App| {
            let mut settings = app.world_mut().resource_mut::<GameSettings>();
            settings.binds[0].set(Action::MoveUp, KeyCode::KeyT);
        };
        let wiped =
            |app: &App| app.world().resource::<GameSettings>().binds == binds::default_binds();
        let feedback = |app: &App| app.world().resource::<ControlsMenu>().feedback.clone();
        let tap = |app: &mut App, key: KeyCode| {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            app.update();
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(key);
            keys.clear();
        };
        let wait = |app: &mut App, secs: f32| {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(secs));
        };

        rebound(&mut app);
        tap(&mut app, KeyCode::Backspace);
        assert!(!wiped(&app), "one press only asks");
        assert_eq!(
            feedback(&app),
            fill(
                crate::app::i18n::EN.ctl_reset_confirm,
                &[("k", "Backspace")]
            )
        );
        wait(&mut app, 1.0);
        tap(&mut app, KeyCode::Backspace);
        assert!(wiped(&app), "the second, soon after, resets");
        assert!(feedback(&app).is_empty());

        rebound(&mut app);
        tap(&mut app, KeyCode::Backspace);
        wait(&mut app, menu_ui::SECOND_PRESS_SECS + 1.0);
        app.update();
        assert!(feedback(&app).is_empty(), "the question lapses");
        tap(&mut app, KeyCode::Backspace);
        assert!(!wiped(&app), "a late second press only asks again");

        // Anything else between the two takes the question back.
        tap(&mut app, KeyCode::KeyS);
        tap(&mut app, KeyCode::Backspace);
        assert!(!wiped(&app));

        // A pad's X, through the bridge, the same way; named as X.
        tap(&mut app, KeyCode::KeyW);
        app.insert_resource(LastDevice::PAD);
        let press_x = |app: &mut App| {
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("pad");
            gamepad.digital_mut().press(GamepadButton::West);
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("pad");
            gamepad.digital_mut().release(GamepadButton::West);
            gamepad.digital_mut().clear();
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
        };
        press_x(&mut app);
        assert!(!wiped(&app), "X only asks");
        assert_eq!(
            feedback(&app),
            fill(crate::app::i18n::EN.ctl_reset_confirm, &[("k", "X")])
        );
        press_x(&mut app);
        assert!(wiped(&app), "and X again resets");
    }

    #[test]
    fn rows_line_up_with_the_actions() {
        assert_eq!(ControlsLine::at(0), ControlsLine::Seat);
        assert_eq!(ControlsLine::at(ROWS - 1), ControlsLine::Reset);
        assert_eq!(ControlsLine::at(1), ControlsLine::Bind(Action::MoveUp));
        assert_eq!(
            ControlsLine::at(ROWS - 2),
            ControlsLine::Bind(Action::ClearAll)
        );
    }

    /// A heading over the seat picker and over the first key of each
    /// group, and nowhere else.
    #[test]
    fn each_group_is_headed_once() {
        let tr = &crate::app::i18n::EN;
        let headed: Vec<(usize, &str)> = (0..ROWS)
            .filter_map(|row| heading_before(row, tr).map(|heading| (row, heading)))
            .collect();
        assert_eq!(
            headed,
            vec![
                (0, tr.ctl_group_seat),
                (1, tr.ctl_group_move),
                (5, tr.ctl_group_place),
                (9, tr.ctl_group_posts),
            ]
        );
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

    /// Under the mirrored scheme the screen lists what each key does: the
    /// "Move up" row shows the up arrow, and rebinding "Place up" moves
    /// the key that places up, as the cursor reads it.
    #[test]
    fn the_mirrored_scheme_lists_and_rebinds_each_job() {
        use crate::app::cursor::keymap;
        use crate::sim::Direction;
        let mut settings = GameSettings {
            commit: CommitScheme::Wasd,
            ..GameSettings::default()
        };
        let caps = crate::app::keycaps::KeyCaps::default();
        assert_eq!(
            caps.label(shown_key(&settings, 0, Action::MoveUp)),
            caps.label(KeyCode::ArrowUp)
        );
        assert_eq!(shown_key(&settings, 0, Action::PlaceUp), KeyCode::KeyW);
        // Seat 2 has no scheme.
        assert_eq!(shown_key(&settings, 1, Action::MoveUp), KeyCode::KeyI);

        assert_eq!(
            rebind_job(
                &mut settings.binds,
                CommitScheme::Wasd,
                0,
                Action::PlaceUp,
                KeyCode::KeyT
            ),
            Ok(())
        );
        let map = keymap(&settings, 0, CommitScheme::Wasd);
        assert_eq!(map.places[0], (KeyCode::KeyT, Direction::Up), "T places up");
        assert_eq!(map.moves[0].0, KeyCode::ArrowUp, "the up arrow still moves");
        assert_eq!(shown_key(&settings, 0, Action::PlaceUp), KeyCode::KeyT);

        // A clash names the job the key does, not the slot it sits in.
        assert_eq!(
            rebind_job(
                &mut settings.binds,
                CommitScheme::Wasd,
                0,
                Action::PlaceDown,
                KeyCode::ArrowLeft
            ),
            Err(Taken::Bound(0, Action::MoveLeft))
        );
    }

    /// Under the one-hand preset the place rows list and rebind P1's own
    /// place keys, which are what places once the preset stands down in
    /// versus and co-op; a refusal names the job the key does there, and
    /// the line under a picked place row names the preset's key.
    #[test]
    fn the_one_hand_preset_rows_list_the_keys_a_shared_keyboard_places_with() {
        use crate::app::cursor::keymap;
        use crate::sim::Direction;
        let mut settings = GameSettings {
            commit: CommitScheme::Ijkl,
            ..GameSettings::default()
        };
        assert_eq!(shown_key(&settings, 0, Action::PlaceUp), KeyCode::ArrowUp);
        assert_eq!(shown_key(&settings, 0, Action::MoveUp), KeyCode::KeyW);

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::Controls));
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::gamepad::Bridged>();
        app.init_resource::<crate::app::gamepad::LastDevice>();
        app.init_resource::<Time>();
        app.insert_resource(settings.clone());
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.insert_resource(ControlsMenu {
            selected: 1 + Action::PlaceUp.index(),
            ..ControlsMenu::default()
        });
        app.add_systems(Update, controls_input);
        let press = |app: &mut App, key: KeyCode| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release_all();
            keys.clear();
            keys.press(key);
            app.update();
        };
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.world().resource::<ControlsMenu>().capturing,
            Some(Action::PlaceUp)
        );
        press(&mut app, KeyCode::KeyT);
        let bound = app.world().resource::<GameSettings>();
        let map = keymap(bound, 0, bound.commit_in_play(true));
        assert_eq!(map.places[0], (KeyCode::KeyT, Direction::Up), "T places up");
        assert_eq!(shown_key(bound, 0, Action::PlaceUp), KeyCode::KeyT);

        // The up arrow is free again, and the move rows may take it; the
        // left arrow still places left, which is what its row reads.
        settings.binds = bound.binds;
        assert_eq!(listed_scheme(&settings), CommitScheme::Arrows);
        assert_eq!(
            rebind_job(
                &mut settings.binds,
                CommitScheme::Ijkl,
                0,
                Action::MoveUp,
                KeyCode::ArrowLeft
            ),
            Err(Taken::Bound(0, Action::PlaceLeft))
        );
        assert_eq!(
            shown_key(&settings, 0, Action::PlaceLeft),
            KeyCode::ArrowLeft
        );

        let caps = crate::app::keycaps::KeyCaps::default();
        let tr = &crate::app::i18n::EN;
        assert_eq!(
            alone_note(&settings, &caps, 0, Action::PlaceUp),
            Some(fill(
                tr.ctl_preset,
                &[
                    ("k", &caps.label(KeyCode::KeyI)),
                    ("a", tr.bind_actions[Action::PlaceUp.index()]),
                    ("s", tr.set_commit_keys),
                ]
            ))
        );
        assert_eq!(alone_note(&settings, &caps, 0, Action::MoveUp), None);
        assert_eq!(alone_note(&settings, &caps, 1, Action::PlaceUp), None);
    }

    /// Under the one-hand preset P1's I/J/K/L place when P1 plays alone,
    /// so a P1 key read alone may not take one even once P2 lets it go:
    /// the one press would do both jobs. A place row may, since the
    /// preset reads none alone, and P2 is refused nothing new.
    #[test]
    fn the_one_hand_presets_keys_take_no_second_job() {
        let mut table = binds::default_binds();
        let ijkl = CommitScheme::Ijkl;
        assert_eq!(
            rebind_job(&mut table, ijkl, 1, Action::MoveUp, KeyCode::KeyU),
            Ok(())
        );
        for action in [Action::Remove, Action::MoveUp] {
            assert_eq!(
                rebind_job(&mut table, ijkl, 0, action, KeyCode::KeyI),
                Err(Taken::Alone(Action::PlaceUp)),
                "{action:?}"
            );
        }
        assert_eq!(table[0], binds::SeatBinds::default_for(0), "unchanged");
        assert!(!crate::app::cursor::preset_clash(&table, ijkl));
        assert_eq!(
            rebind_job(&mut table, ijkl, 0, Action::PlaceUp, KeyCode::KeyI),
            Ok(())
        );
        assert!(!crate::app::cursor::preset_clash(&table, ijkl));
        assert_eq!(
            rebind_job(&mut table, ijkl, 1, Action::MoveUp, KeyCode::KeyO),
            Ok(())
        );
        // Under the stock arrows I is free; the preset then clashes.
        let mut table = binds::default_binds();
        table[1].set(Action::MoveUp, KeyCode::KeyU);
        assert_eq!(
            rebind_job(
                &mut table,
                CommitScheme::Arrows,
                0,
                Action::Remove,
                KeyCode::KeyI
            ),
            Ok(())
        );
        assert!(crate::app::cursor::preset_clash(&table, ijkl));
    }

    /// A refusal names the seat holding the key when it is not the
    /// page's: on P1's page, I is P2's "Move up", not the W the page's
    /// own "Move up" row shows.
    #[test]
    fn a_refusal_names_the_other_seat() {
        let tr = &crate::app::i18n::EN;
        let caps = crate::app::keycaps::KeyCaps::default();
        let mut table = binds::default_binds();
        let Err(Taken::Bound(held_by, job)) = rebind_job(
            &mut table,
            CommitScheme::Arrows,
            0,
            Action::MoveDown,
            KeyCode::KeyI,
        ) else {
            panic!("I is P2's");
        };
        assert_eq!((held_by, job), (1, Action::MoveUp));
        assert_eq!(
            taken_line(tr, &caps, 0, held_by, KeyCode::KeyI, job),
            "I already does \"Move up\" for P2"
        );
        assert_eq!(
            taken_line(tr, &caps, 1, held_by, KeyCode::KeyI, job),
            "I already does \"Move up\""
        );
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
