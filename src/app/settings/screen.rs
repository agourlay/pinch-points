//! The settings screen: the row ladder, its input, and its renderer.
//!
//! Split from the settings *model* next door, which the whole game reads
//! and which has no reason to depend on Bevy UI.

use super::{
    CommitScheme, DEADZONE_RANGE, GameSettings, REPEAT_DELAY_RANGE, REPEAT_INTERVAL_RANGE,
    SeatInput, UI_SCALE_MAX, UI_SCALE_MIN, VOLUME_RANGE, clamp_in,
};
use crate::app::Screen;
use crate::app::cycle::{Cycle, dial};
use crate::app::i18n::fill;
use crate::app::menu_ui::{self, Half};
use crate::app::palette;
use bevy::prelude::*;

/// One settings row. Both the input ladder and the renderer match on
/// this, so inserting a row can never desynchronize them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Row {
    /// What drives P1, and P2: keyboard, a named controller, or whatever
    /// turns up.
    InputP1,
    InputP2,
    CommitKeys,
    /// Which keyboard the caps are read off, or Auto.
    Keyboard,
    /// Opens the per-key rebinding screen.
    KeyBindings,
    RepeatDelay,
    RepeatRate,
    /// Music and effects each switch off on their own, above the slider
    /// that says how loud they are when they are on.
    MusicOn,
    Music,
    SfxOn,
    Sfx,
    Speed,
    VersusMode,
    Rumble,
    Deadzone,
    Palette,
    /// Borderless fullscreen or a window; F11 flips it anywhere else.
    Fullscreen,
    UiScale,
    ReducedMotion,
    /// Smoothing the edges of the beach, which a weak graphics chip pays
    /// for every frame.
    Antialiasing,
    /// How many finished rounds the shelf keeps.
    ReplayCap,
    Language,
    /// Whether start-up asks GitHub for a newer release.
    UpdateCheck,
    /// Forgets every cleared stage. Last, and behind a confirmation.
    ResetProgress,
}

impl Row {
    /// Whether Enter on the row is done here, as Escape is: every dial.
    /// The two doors, the key bindings screen and the reset, are the rows
    /// it works instead. The pad's legend names A by this.
    pub fn is_dial(self) -> bool {
        !matches!(self, Row::KeyBindings | Row::ResetProgress)
    }

    /// Display and navigation order, which is [`SECTIONS`] flattened. The
    /// rows used to run in the order they were added, which put the pad
    /// deadzone six rows below the keys it belongs with.
    pub const ALL: [Row; 24] = [
        // Controls
        Row::InputP1,
        Row::InputP2,
        Row::CommitKeys,
        // Before the rebinding door: it is what every key on the far side
        // of that door is labelled with.
        Row::Keyboard,
        Row::KeyBindings,
        Row::RepeatDelay,
        Row::RepeatRate,
        Row::Rumble,
        Row::Deadzone,
        // Sound
        Row::MusicOn,
        Row::Music,
        Row::SfxOn,
        Row::Sfx,
        // The round
        Row::Speed,
        Row::VersusMode,
        Row::ReplayCap,
        // Presentation
        Row::Palette,
        Row::Fullscreen,
        Row::UiScale,
        Row::ReducedMotion,
        Row::Antialiasing,
        Row::Language,
        // The game itself
        Row::UpdateCheck,
        // And the one that throws something away
        Row::ResetProgress,
    ];
}

/// A heading on the settings card. Every dial in one ladder (there are
/// `Row::ALL.len()` of them) is a list to be read from the top every time;
/// a handful of named groups is a place to look things up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Controls,
    Sound,
    Round,
    Look,
    /// The game as a program: the one setting about the copy on disk
    /// rather than about playing it.
    Game,
    Danger,
}

impl Group {
    fn label(self, tr: &crate::app::i18n::Tr) -> &'static str {
        match self {
            Group::Controls => tr.set_group_controls,
            Group::Sound => tr.set_group_sound,
            Group::Round => tr.set_group_round,
            Group::Look => tr.set_group_look,
            Group::Game => tr.set_group_game,
            Group::Danger => tr.set_group_danger,
        }
    }
}

/// The card, left column then right. Flattening this must give
/// [`Row::ALL`]; a test says so.
pub const SECTIONS: [&[(Group, &[Row])]; 2] = [
    &[
        (
            Group::Controls,
            &[
                Row::InputP1,
                Row::InputP2,
                Row::CommitKeys,
                Row::Keyboard,
                Row::KeyBindings,
                Row::RepeatDelay,
                Row::RepeatRate,
                Row::Rumble,
                Row::Deadzone,
            ],
        ),
        (
            Group::Sound,
            &[Row::MusicOn, Row::Music, Row::SfxOn, Row::Sfx],
        ),
    ],
    &[
        (Group::Round, &[Row::Speed, Row::VersusMode, Row::ReplayCap]),
        (
            Group::Look,
            &[
                Row::Palette,
                Row::Fullscreen,
                Row::UiScale,
                Row::ReducedMotion,
                Row::Antialiasing,
                Row::Language,
            ],
        ),
        (Group::Game, &[Row::UpdateCheck]),
        (Group::Danger, &[Row::ResetProgress]),
    ],
];

const ROWS: usize = Row::ALL.len();

/// How far through the reset the player is. Wiping a campaign on one
/// keypress, on a row the cursor passes over on its way to the language, is
/// not a thing to do to somebody.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ResetPrompt {
    /// Offering to reset.
    #[default]
    Idle,
    /// Enter pressed once; a second press does it.
    Armed,
    /// Done, and saying so until the cursor moves on.
    Done,
}

/// What Enter on the reset row leads to. Pure, so the two-press rule is
/// testable without a window, or a test writing over the real save file.
fn next_reset_prompt(current: ResetPrompt) -> ResetPrompt {
    match current {
        ResetPrompt::Armed => ResetPrompt::Done,
        ResetPrompt::Idle | ResetPrompt::Done => ResetPrompt::Armed,
    }
}

/// Which row the settings cursor is on.
#[derive(Resource, Default)]
pub struct SettingsMenu {
    pub selected: usize,
    /// State of the reset row; cleared whenever the cursor leaves it.
    pub reset: ResetPrompt,
    /// "Rounds kept" as the settings flow was entered, so leaving it knows
    /// whether the cap went down ([`apply_replay_cap`]).
    pub replay_cap_was: u8,
}

#[derive(Component)]
pub struct SettingsRow(pub usize);

/// One half of one row.
#[derive(Component)]
pub struct SettingsCell(pub usize, pub Half);

#[derive(Component)]
pub struct SettingsUi;

/// Label gutter and value gutter, the value including the `< >` the
/// selected row wears. Two columns of these have to fit in
/// [`DESIGN_W`](crate::app::settings::DESIGN_W), which
/// `the_card_fits_the_window_it_was_drawn_for` holds them to.
///
/// Both cells clip what overruns rather than wrapping it, so a row that
/// does not fit loses its tail with no warning;
/// `every_row_fits_its_cell_in_every_language` says so instead. That check
/// measures pixels rather than characters, because two faces draw these
/// rows: DejaVu Sans Mono at 0.602 em and the Japanese subset at a full
/// one.
const LABEL_W: f32 = 252.0;
const VALUE_W: f32 = 322.0;
/// A point under [`menu_ui::type_scale::ROW`], and the only list card in
/// the game that is.
///
/// Not drift: it is the size at which two columns of rows fit a 1280-wide
/// window in every language. At the scale's 19 the German duos value runs
/// 332px into a 322px cell, and the ten pixels the cell would need are
/// twenty on the card, which has eighteen to give.
const ROW_FONT: f32 = 18.0;

/// The language row's flag chip: 3:2, the ratio the art is drawn at, and
/// short enough to sit inside an 18px line rather than growing the row.
const FLAG_W: f32 = 21.0;
const FLAG_H: f32 = 14.0;
const FLAG_GAP: f32 = 7.0;

/// The flag chip on the language row, so the dial can repaint it without
/// finding it by position among the cells.
#[derive(Component)]
pub struct LanguageFlag;

pub fn enter_settings(
    mut commands: Commands,
    settings: Res<GameSettings>,
    art: Res<crate::app::art::Art>,
    mut menu: ResMut<SettingsMenu>,
    hands: (
        Res<crate::app::gamepad::LastDevice>,
        Res<crate::app::gamepad::LastPad>,
        Query<&Gamepad>,
    ),
) {
    let tr = settings.tr();
    let family = crate::app::gamepad::reference_family(*hands.0, *hands.1, &hands.2);
    menu.selected = 0;
    menu.reset = ResetPrompt::Idle;
    let mut index = 0usize;
    commands
        .spawn((
            SettingsUi,
            // Centred between the bars, like every other list screen -
            // through the frame that says so, rather than a copy of its
            // numbers.
            menu_ui::between_bars(),
        ))
        .with_children(|wrap| {
            // The shared card, laid out sideways: this one holds columns
            // of rows rather than a single run of them.
            let (mark, mut node, fill, edge, shadow) = menu_ui::screen_card();
            node.flex_direction = FlexDirection::Row;
            node.align_items = AlignItems::FlexStart;
            node.column_gap = Val::Px(28.0);
            wrap.spawn((mark, node, fill, edge, shadow))
                .with_children(|card| {
                    for column in SECTIONS {
                        card.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(2.0),
                            ..default()
                        })
                        .with_children(|side| {
                            // `at`, not `index`: the running index counts
                            // rows across the whole card, and whether a
                            // heading wants air above it is a question
                            // about its own column. Asked of the row
                            // counter, the right column's first heading
                            // took the 10px meant to separate two groups.
                            for (at, (group, rows)) in column.iter().enumerate() {
                                side.spawn(menu_ui::heading(group.label(tr), at == 0));
                                for row in *rows {
                                    let flag = (*row == Row::Language)
                                        .then(|| art.flag(settings.language));
                                    spawn_setting_row(side, index, flag);
                                    index += 1;
                                }
                            }
                        });
                    }
                });
            // Read-only controller reference, under the card: a round's
            // controls and the menus', written from their own tables.
            use crate::app::gamepad::{Face, scheme};
            for (lead, face) in [
                (tr.pad.help_round, Face::Round),
                (tr.pad.help_menus, Face::Settings { dial: false }),
            ] {
                let line = format!("{lead} {}", scheme::legend(tr, family, &face));
                wrap.spawn(menu_ui::footnote(line));
            }
        });
}

/// The placing scheme the dial turns to from the one set: the next one
/// along, passing over any that would give a bound key a second job
/// ([`crate::app::cursor::preset_clash`]). The stock arrows never do.
fn next_commit(settings: &GameSettings, turn: crate::app::cycle::Turn) -> CommitScheme {
    let mut next = settings.commit.cycled(turn);
    while crate::app::cursor::preset_clash(&settings.binds, next) {
        next = next.cycled(turn);
    }
    next
}

/// One dial: its name in a fixed gutter and its value in another, so every
/// value on the card starts at the same x and the column can be read down.
///
/// The language row is the one exception: its value carries a flag chip in
/// front of the name, which pushes that one value right by the width of the
/// chip. A blank slot on every other row would straighten the column and
/// cost every one of them that width, so the indent stands.
fn spawn_setting_row(side: &mut ChildSpawnerCommands, index: usize, flag: Option<Handle<Image>>) {
    side.spawn((
        SettingsRow(index),
        Node {
            align_items: AlignItems::Center,
            padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
            border_radius: BorderRadius::all(Val::Px(7.0)),
            ..default()
        },
        BackgroundColor(Color::NONE),
    ))
    .with_children(|line| {
        for (half, width) in [(Half::Label, LABEL_W), (Half::Value, VALUE_W)] {
            line.spawn(Node {
                width: Val::Px(width),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                column_gap: Val::Px(FLAG_GAP),
                overflow: Overflow::clip_x(),
                ..default()
            })
            .with_children(|cell| {
                if let (Half::Value, Some(flag)) = (half, flag.clone()) {
                    cell.spawn((
                        LanguageFlag,
                        ImageNode::new(flag),
                        Node {
                            width: Val::Px(FLAG_W),
                            height: Val::Px(FLAG_H),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                }
                cell.spawn((
                    SettingsCell(index, half),
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(ROW_FONT),
                        ..default()
                    },
                    TextLayout::no_wrap(),
                    TextColor(palette::IDLE_ROW),
                ));
            });
        }
    });
}

pub fn settings_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<SettingsMenu>,
    mut settings: ResMut<GameSettings>,
    mut caps: ResMut<crate::app::keycaps::KeyCaps>,
    mut progress: ResMut<crate::app::progress::Progress>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    // Enter works the doors, the key-binding screen and the reset; on every
    // dial it means the same as Escape: done here.
    if menu_ui::enter(&keys) {
        match Row::ALL[menu.selected] {
            Row::KeyBindings => next_screen.set(Screen::Controls),
            Row::ResetProgress => {
                menu.reset = next_reset_prompt(menu.reset);
                if menu.reset == ResetPrompt::Done {
                    progress.clear_all();
                    crate::app::progress::save_reset(&progress);
                }
            }
            Row::InputP1
            | Row::InputP2
            | Row::CommitKeys
            | Row::Keyboard
            | Row::RepeatDelay
            | Row::RepeatRate
            | Row::MusicOn
            | Row::Music
            | Row::SfxOn
            | Row::Sfx
            | Row::Speed
            | Row::VersusMode
            | Row::Rumble
            | Row::Deadzone
            | Row::Palette
            | Row::Fullscreen
            | Row::UiScale
            | Row::ReducedMotion
            | Row::Antialiasing
            | Row::ReplayCap
            | Row::Language
            | Row::UpdateCheck => next_screen.set(Screen::Menu),
        }
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        next_screen.set(Screen::Menu);
        return;
    }
    let was = menu.selected;
    menu.selected = menu_ui::nav(&keys, menu.selected, ROWS);
    if menu.selected != was {
        // Stepping off the row takes the offer back with you.
        menu.reset = ResetPrompt::Idle;
    }
    let Some(turn) = menu_ui::left_right(&keys) else {
        return;
    };
    let step = turn.signum() as f32;
    match Row::ALL[menu.selected] {
        Row::InputP1 | Row::InputP2 => {
            let seat = usize::from(Row::ALL[menu.selected] == Row::InputP2);
            settings.seat_input[seat] = settings.seat_input[seat].cycled(turn);
        }
        Row::CommitKeys => settings.commit = next_commit(&settings, turn),
        Row::RepeatDelay => {
            settings.repeat_delay =
                clamp_in(settings.repeat_delay + step * 0.05, &REPEAT_DELAY_RANGE);
        }
        Row::RepeatRate => {
            settings.repeat_interval = clamp_in(
                settings.repeat_interval + step * 0.02,
                &REPEAT_INTERVAL_RANGE,
            );
        }
        Row::MusicOn => settings.music_on = !settings.music_on,
        Row::SfxOn => settings.sfx_on = !settings.sfx_on,
        Row::Music => settings.music_volume = dial(settings.music_volume, turn, 10, VOLUME_RANGE),
        Row::Sfx => settings.sfx_volume = dial(settings.sfx_volume, turn, 10, VOLUME_RANGE),
        Row::VersusMode => settings.team_mode = settings.team_mode.cycled(turn),
        Row::Speed => {
            settings.puzzle_speed = settings.puzzle_speed.stepped(turn);
        }
        Row::Rumble => settings.rumble = !settings.rumble,
        Row::Deadzone => {
            settings.pad_deadzone = dial(settings.pad_deadzone, turn, 10, DEADZONE_RANGE);
        }
        Row::Palette => settings.colorblind = !settings.colorblind,
        Row::Fullscreen => settings.fullscreen = !settings.fullscreen,
        Row::UiScale => {
            settings.ui_scale = dial(settings.ui_scale, turn, 10, UI_SCALE_MIN..=UI_SCALE_MAX);
        }
        Row::ReducedMotion => settings.reduced_motion = !settings.reduced_motion,
        Row::Antialiasing => settings.antialiasing = !settings.antialiasing,
        Row::ReplayCap => {
            settings.replay_cap = dial(
                settings.replay_cap,
                turn,
                5,
                crate::app::settings::REPLAY_CAP_MIN..=crate::app::settings::REPLAY_CAP_MAX,
            );
        }
        // Doors: worked with Enter, not adjusted with A/D.
        Row::KeyBindings | Row::ResetProgress => {}
        Row::Language => {
            let next = settings.language.cycled(turn);
            settings.set_language(next, &mut caps);
        }
        Row::Keyboard => {
            let next = settings.keyboard.cycled(turn);
            settings.set_keyboard(next, &mut caps);
        }
        Row::UpdateCheck => settings.check_updates = !settings.check_updates,
    }
}

/// Note "Rounds kept" as the settings flow is entered, for
/// [`apply_replay_cap`] to measure a lowered dial from. Back from the key
/// bindings is the same visit, and keeps the cap it began with.
pub fn note_replay_cap(
    settings: Res<GameSettings>,
    mut menu: ResMut<SettingsMenu>,
    previous: Option<Res<PreviousState<Screen>>>,
) {
    if !previous.is_some_and(|previous| in_settings_flow(**previous)) {
        menu.replay_cap_was = settings.replay_cap;
    }
}

/// Whether `screen` is part of the settings flow: Settings itself, and the
/// key bindings it opens a door to, which hand back to it.
pub fn in_settings_flow(screen: Screen) -> bool {
    match screen {
        Screen::Settings | Screen::Controls => true,
        Screen::Menu
        | Screen::Puzzle
        | Screen::Versus
        | Screen::Editor
        | Screen::Lobby
        | Screen::MatchSetup
        | Screen::Achievements
        | Screen::StageSelect
        | Screen::Replays
        | Screen::Interlude
        | Screen::Language
        | Screen::NewVersion => false,
    }
}

/// "Rounds kept" lowered on this visit is applied to the shelf as the
/// settings flow is left, with the rest of the settings saved. At the dial
/// it would be applied on every step, and a player stepping 20 down past
/// 10 on the way to 15 would lose five rounds to the overshoot. A visit to
/// the key bindings and back is still the same visit: run as Settings or
/// Controls is left, it does nothing while the screen entered is the
/// other one ([`in_settings_flow`]).
pub fn apply_replay_cap(
    screen: Res<State<Screen>>,
    settings: Res<GameSettings>,
    menu: Res<SettingsMenu>,
    library: Res<crate::app::replays::Library>,
) {
    if in_settings_flow(*screen.get()) {
        return;
    }
    crate::app::replays::cap_changed_in(&library.dir, menu.replay_cap_was, settings.replay_cap);
}

/// Whether the cursor puts `< >` around this row's value. Doors do not step
/// through anything, so they wear none, and the widest a value ever draws
/// depends on the answer: one rule rather than one in the screen and
/// another in the test that measures it.
fn has_arrows(row: Row) -> bool {
    !matches!(row, Row::KeyBindings | Row::ResetProgress)
}

/// A row's name and its current value, undecorated. The screen adds the
/// `< >` arrows to the row the cursor is on, so they read as "these keys,
/// on this row" rather than as part of every value.
///
/// Pure, so the wording of every setting can be checked without a window.
/// The button the card's doors name for a pad's player: the settings
/// table's for Enter, which the bridge presses for it. `None` at a
/// keyboard, whose doors name Enter in their own words.
fn door_button(
    tr: &crate::app::i18n::Tr,
    device: crate::app::gamepad::LastDevice,
) -> Option<String> {
    crate::app::gamepad::scheme::button_name(
        &crate::app::gamepad::Face::Settings { dial: false },
        crate::app::gamepad::scheme::Does::Confirm,
        tr,
        device.family(),
    )
    .filter(|_| device.is_pad())
}

pub(super) fn row_text(
    tr: &crate::app::i18n::Tr,
    settings: &GameSettings,
    caps: &crate::app::keycaps::KeyCaps,
    row: Row,
    reset: ResetPrompt,
    device: crate::app::gamepad::LastDevice,
) -> (String, String) {
    let on_off = |on: bool| if on { tr.val_on } else { tr.val_off }.to_string();
    let (label, value) = match row {
        Row::InputP1 | Row::InputP2 => {
            let seat = usize::from(row == Row::InputP2);
            return (
                fill(tr.set_seat_input, &[("n", &(seat + 1).to_string())]),
                match settings.seat_input[seat] {
                    SeatInput::Auto => tr.val_input_auto.to_string(),
                    SeatInput::Keys => tr.val_input_keys.to_string(),
                    SeatInput::Pad(n) => fill(tr.val_input_pad, &[("n", &(n + 1).to_string())]),
                },
            );
        }
        Row::CommitKeys => (
            tr.set_commit_keys,
            match settings.commit {
                CommitScheme::Ijkl => caps.legend(tr.val_ijkl),
                CommitScheme::Wasd => caps.legend(tr.val_wasd),
                CommitScheme::Arrows => tr.val_arrows.to_string(),
            },
        ),
        Row::RepeatDelay => (
            tr.set_repeat_delay,
            format!("{:.2}s", settings.repeat_delay),
        ),
        Row::RepeatRate => (
            tr.set_repeat_rate,
            format!("{:.2}s", settings.repeat_interval),
        ),
        Row::MusicOn => (tr.set_music_on, on_off(settings.music_on)),
        Row::Music => (tr.set_music, format!("{}%", settings.music_volume)),
        Row::SfxOn => (tr.set_sfx_on, on_off(settings.sfx_on)),
        Row::Sfx => (tr.set_sfx, format!("{}%", settings.sfx_volume)),
        Row::Speed => (
            tr.set_speed,
            format!("{}%", settings.puzzle_speed.percent()),
        ),
        Row::VersusMode => (
            tr.set_versus_mode,
            tr.team_modes[settings.team_mode.index()].to_string(),
        ),
        Row::Rumble => (tr.set_rumble, on_off(settings.rumble)),
        Row::Deadzone => (tr.set_deadzone, format!("{}%", settings.pad_deadzone)),
        Row::Palette => (
            tr.set_palette,
            if settings.colorblind {
                tr.val_palette_safe
            } else {
                tr.val_palette_classic
            }
            .to_string(),
        ),
        Row::Fullscreen => (tr.set_fullscreen, on_off(settings.fullscreen)),
        Row::UiScale => (tr.set_ui_scale, format!("{}%", settings.ui_scale)),
        Row::ReducedMotion => (tr.set_reduced_motion, on_off(settings.reduced_motion)),
        Row::Antialiasing => (tr.set_antialiasing, on_off(settings.antialiasing)),
        Row::ReplayCap => (tr.set_replay_cap, settings.replay_cap.to_string()),
        Row::Language => (tr.set_language, settings.language.native_name().to_string()),
        Row::Keyboard => (
            tr.set_keyboard,
            settings
                .keyboard
                .map_or(tr.val_auto, crate::app::keycaps::Layout::name)
                .to_string(),
        ),
        Row::UpdateCheck => (tr.set_update_check, on_off(settings.check_updates)),
        // Doors, not dials: no `< value >` decoration.
        Row::KeyBindings => {
            let open = match door_button(tr, device) {
                Some(button) => fill(tr.pad.val_open, &[("b", &button)]),
                None => tr.val_open.to_string(),
            };
            return (tr.set_key_bindings.to_string(), open);
        }
        Row::ResetProgress => {
            let (idle, armed) = match door_button(tr, device) {
                Some(button) => (
                    fill(tr.pad.val_reset, &[("b", &button)]),
                    fill(tr.pad.reset_confirm, &[("b", &button)]),
                ),
                None => (tr.val_reset.to_string(), tr.val_reset_confirm.to_string()),
            };
            let state = match reset {
                ResetPrompt::Idle => idle,
                ResetPrompt::Armed => armed,
                ResetPrompt::Done => tr.val_reset_done.to_string(),
            };
            return (tr.set_reset_progress.to_string(), state);
        }
    };
    (label.to_string(), value)
}

pub fn update_settings_ui(
    (settings, device): (Res<GameSettings>, Res<crate::app::gamepad::LastDevice>),
    caps: Res<crate::app::keycaps::KeyCaps>,
    menu: Res<SettingsMenu>,
    art: Res<crate::app::art::Art>,
    mut cells: Query<(&SettingsCell, &mut Text, &mut TextColor)>,
    mut rows: Query<(&SettingsRow, &mut BackgroundColor)>,
    mut flags: Query<&mut ImageNode, With<LanguageFlag>>,
) {
    let tr = settings.tr();
    // The chip follows the dial. Assigning only on a change keeps this off
    // Bevy's change detection every frame, which the image would otherwise
    // trip sixty times a second for a picture that almost never moves.
    let wanted = art.flag(settings.language);
    for mut flag in &mut flags {
        if flag.image != wanted {
            flag.image = wanted.clone();
        }
    }
    for (cell, mut text, mut color) in &mut cells {
        let row = Row::ALL[cell.0];
        let picked = cell.0 == menu.selected;
        let (label, value) = row_text(tr, &settings, &caps, row, menu.reset, *device);
        let line = match cell.1 {
            Half::Label => label,
            Half::Value if picked && has_arrows(row) => format!("< {value} >"),
            Half::Value => value,
        };
        let target = menu_ui::cell_ink(cell.1, picked);
        menu_ui::set_text(&mut text, &line);
        menu_ui::set_color(&mut color, target);
    }
    for (row, mut fill) in &mut rows {
        let ground = if row.0 == menu.selected {
            palette::GOLD.with_alpha(0.16)
        } else {
            Color::NONE
        };
        menu_ui::set_bg(&mut fill, ground);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::gamepad::LastDevice;
    use crate::app::i18n::EN;

    /// The placing-keys dial passes over the one-hand preset while a P1
    /// key read alone sits on one of its keys, which would then do two
    /// jobs, and stops on it once the keys are clear.
    #[test]
    fn the_dial_passes_over_a_scheme_that_would_clash() {
        use crate::app::binds::Action;
        use crate::app::cycle::Turn;
        let mut settings = GameSettings {
            commit: CommitScheme::Wasd,
            ..GameSettings::default()
        };
        assert_eq!(next_commit(&settings, Turn::Right), CommitScheme::Ijkl);
        settings.binds[1].set(Action::MoveUp, KeyCode::KeyU);
        settings.binds[0].set(Action::Remove, KeyCode::KeyI);
        assert_eq!(next_commit(&settings, Turn::Right), CommitScheme::Arrows);
        settings.commit = CommitScheme::Arrows;
        assert_eq!(next_commit(&settings, Turn::Left), CommitScheme::Wasd);
    }

    /// The card is built by walking [`SECTIONS`] with a running index, and
    /// every row's label is looked up by that index in [`Row::ALL`]. If the
    /// two disagree the screen shows one dial's name over another's value,
    /// silently.
    #[test]
    fn the_sections_are_the_row_order() {
        let flat: Vec<Row> = SECTIONS
            .iter()
            .flat_map(|column| column.iter())
            .flat_map(|(_, rows)| rows.iter().copied())
            .collect();
        assert_eq!(flat, Row::ALL.to_vec());
    }

    /// Every row says what it is and what it is set to, in every language.
    /// A row added without wording would show an empty dial.
    #[test]
    fn every_settings_row_reads_as_a_setting() {
        for lang in crate::app::i18n::ALL_LANGS {
            let settings = GameSettings {
                language: lang,
                ..GameSettings::default()
            };
            for row in Row::ALL {
                let (label, value) = row_text(
                    settings.tr(),
                    &settings,
                    &crate::app::keycaps::KeyCaps::default(),
                    row,
                    ResetPrompt::Idle,
                    LastDevice::Keyboard,
                );
                assert!(!label.trim().is_empty(), "{row:?} in {lang:?} has no name");
                assert!(!value.trim().is_empty(), "{row:?} in {lang:?} has no value");
            }
        }
    }

    /// Every row fits the two cells that hold it, in every language, on
    /// every stop of every dial.
    ///
    /// Both cells clip rather than wrap, so a row that overruns loses its
    /// tail with no warning: under a budget written in characters, the
    /// widest of them lost the closing bracket off the paired team mode in
    /// five languages ([`LABEL_W`] says why characters were the wrong unit).
    ///
    /// Every dial is walked to its widest stop rather than read at its
    /// default, because the default is not what a player leaves it on.
    #[test]
    fn every_row_fits_its_cell_in_every_language() {
        use crate::app::i18n::metrics::text_px;
        for lang in crate::app::i18n::ALL_LANGS {
            // The widest stop on each dial: three digits where the value is
            // a percentage, the longest of the named options, and the pad
            // seat, whose value counts a controller in.
            let settings = GameSettings {
                language: lang,
                seat_input: [SeatInput::Pad(9); crate::app::binds::BOUND_SEATS],
                music_volume: 100,
                sfx_volume: 100,
                puzzle_speed: crate::app::settings::PuzzleSpeed::Full,
                pad_deadzone: 100,
                ui_scale: 150,
                replay_cap: 99,
                colorblind: true,
                commit: CommitScheme::Ijkl,
                rumble: true,
                reduced_motion: true,
                check_updates: true,
                repeat_delay: 0.88,
                repeat_interval: 0.88,
                ..GameSettings::default()
            };
            let tr = settings.tr();
            for row in Row::ALL {
                for (reset, device) in [ResetPrompt::Idle, ResetPrompt::Armed, ResetPrompt::Done]
                    .into_iter()
                    .flat_map(|reset| {
                        [LastDevice::Keyboard, LastDevice::PAD].map(|device| (reset, device))
                    })
                {
                    for team_mode in crate::app::teams::TeamMode::ALL {
                        // Every layout by name, since "Auto" is
                        // the short one and nobody leaves a dial
                        // on its default because it was there.
                        // And every placing scheme, each its own words.
                        for (keyboard, commit) in
                            <Option<crate::app::keycaps::Layout> as Cycle>::VARIANTS
                                .iter()
                                .flat_map(|k| CommitScheme::VARIANTS.iter().map(move |c| (k, c)))
                        {
                            let settings = GameSettings {
                                team_mode,
                                keyboard: *keyboard,
                                commit: *commit,
                                ..settings.clone()
                            };
                            let (label, value) = row_text(
                                tr,
                                &settings,
                                &crate::app::keycaps::KeyCaps::default(),
                                row,
                                reset,
                                device,
                            );
                            let label_w = text_px(&label, ROW_FONT);
                            assert!(
                                label_w <= LABEL_W,
                                "{row:?} in {lang:?}: label {label:?} is {label_w:.1}px, \
                                 and the cell holds {LABEL_W}"
                            );
                            // The cursor's row wears the arrows, so the widest
                            // this value ever draws is with them on - on the
                            // rows that have them. The language row also gives
                            // up the head of its cell to the flag chip.
                            let decorated = if has_arrows(row) {
                                format!("< {value} >")
                            } else {
                                value
                            };
                            let chip = if row == Row::Language {
                                FLAG_W + FLAG_GAP
                            } else {
                                0.0
                            };
                            let value_w = text_px(&decorated, ROW_FONT) + chip;
                            assert!(
                                value_w <= VALUE_W,
                                "{row:?} in {lang:?}: value {decorated:?} is {value_w:.1}px, \
                                 and the cell holds {VALUE_W}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// The cells are the card, and the card has to fit the window the
    /// interface was drawn for: a row too long for its cell clips, and a
    /// cell widened to fix that runs the card off a 1280-wide screen.
    #[test]
    fn the_card_fits_the_window_it_was_drawn_for() {
        // Two columns of rows, each row a label and a value inside its own
        // padding, the columns spaced apart, all of it inside the card's
        // padding and border.
        let column = LABEL_W + VALUE_W + 2.0 * 10.0;
        let card = 2.0 * column + 28.0 + 2.0 * 22.0 + 2.0 * 1.0;
        assert!(
            card <= crate::app::settings::DESIGN_W,
            "the settings card is {card}px of the {}px it is allowed",
            crate::app::settings::DESIGN_W
        );
    }

    /// The rows that carry a number say the number, and it moves with the
    /// setting rather than being baked into the label.
    #[test]
    fn a_dial_shows_the_value_it_is_set_to() {
        let mut settings = GameSettings {
            music_volume: 42,
            ..GameSettings::default()
        };
        let caps = crate::app::keycaps::KeyCaps::default();
        let at = |s: &GameSettings, row| {
            row_text(&EN, s, &caps, row, ResetPrompt::Idle, LastDevice::Keyboard).1
        };
        assert!(at(&settings, Row::Music).contains("42%"));
        settings.ui_scale = 70;
        assert!(at(&settings, Row::UiScale).contains("70%"));
        settings.colorblind = true;
        assert!(at(&settings, Row::Palette).contains(EN.val_palette_safe));
        settings.team_mode = crate::app::teams::TeamMode::Trios;
        assert!(at(&settings, Row::VersusMode).contains(EN.team_modes[2]));
    }

    /// Wiping a campaign takes two presses, and the row says which one it is
    /// waiting for. One press must never be enough: the cursor passes over
    /// this row on its way down the list.
    #[test]
    fn resetting_progress_asks_twice() {
        assert_eq!(next_reset_prompt(ResetPrompt::Idle), ResetPrompt::Armed);
        assert_eq!(next_reset_prompt(ResetPrompt::Armed), ResetPrompt::Done);
        // Having done it once, the row offers again rather than firing.
        assert_eq!(next_reset_prompt(ResetPrompt::Done), ResetPrompt::Armed);

        let settings = GameSettings::default();
        let caps = crate::app::keycaps::KeyCaps::default();
        let line = |reset| {
            row_text(
                &EN,
                &settings,
                &caps,
                Row::ResetProgress,
                reset,
                LastDevice::Keyboard,
            )
            .1
        };
        assert!(line(ResetPrompt::Idle).contains(EN.val_reset));
        assert!(line(ResetPrompt::Armed).contains(EN.val_reset_confirm));
        assert!(line(ResetPrompt::Done).contains(EN.val_reset_done));
        // The three states have to read differently, or the confirmation is
        // invisible and the row is a one-press wipe after all.
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            let settings = GameSettings {
                language: lang,
                ..GameSettings::default()
            };
            for device in [LastDevice::Keyboard, LastDevice::PAD] {
                let say =
                    |reset| row_text(tr, &settings, &caps, Row::ResetProgress, reset, device).1;
                assert_ne!(say(ResetPrompt::Idle), say(ResetPrompt::Armed), "{lang:?}");
                assert_ne!(say(ResetPrompt::Armed), say(ResetPrompt::Done), "{lang:?}");
            }
        }
    }

    /// Enter on the reset row arms it and *stays* on the screen. Enter on
    /// every other row means "done here", so this row has to hold its own
    /// against that.
    ///
    /// Exactly one `update()`, and that is load-bearing: this app has no
    /// `InputPlugin`, so nothing clears `just_pressed` between frames and a
    /// second update would press Enter again, confirm the reset, and write
    /// over the real `progress.txt`. The seeded mark below is the tripwire.
    #[test]
    fn arming_the_reset_does_not_leave_the_screen() {
        use crate::app::progress::Progress;

        let mut progress = Progress::default();
        progress.mark("Welcome Ashore");

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<SettingsMenu>();
        app.insert_resource(progress);
        app.insert_resource(GameSettings::default());
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.add_systems(Update, settings_input);
        app.insert_resource(State::new(Screen::Settings));

        let reset_row = Row::ALL.iter().position(|&r| r == Row::ResetProgress);
        app.world_mut().resource_mut::<SettingsMenu>().selected =
            reset_row.expect("the row is in the ladder");

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();

        assert_eq!(
            app.world().resource::<SettingsMenu>().reset,
            ResetPrompt::Armed,
            "the first press arms it"
        );
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Settings,
            "and does not fall through to leaving the screen"
        );
        assert!(
            app.world()
                .resource::<Progress>()
                .is_cleared("Welcome Ashore"),
            "arming alone must not clear anything"
        );
    }

    /// Enter on a dial leaves for the menu, as the pad's legend says A does
    /// there, and on a door it does not. The reset is left out: pressed
    /// twice it writes over the real record (see the test above).
    #[test]
    fn enter_leaves_from_the_dials_only() {
        for row in Row::ALL {
            if row == Row::ResetProgress {
                assert!(!row.is_dial());
                continue;
            }
            let mut app = App::new();
            app.add_plugins(bevy::state::app::StatesPlugin);
            app.init_state::<Screen>();
            app.init_resource::<ButtonInput<KeyCode>>();
            app.init_resource::<SettingsMenu>();
            app.init_resource::<crate::app::progress::Progress>();
            app.insert_resource(GameSettings::default());
            app.init_resource::<crate::app::keycaps::KeyCaps>();
            app.add_systems(Update, settings_input);
            app.insert_resource(State::new(Screen::Settings));
            let at = Row::ALL.iter().position(|&r| r == row);
            app.world_mut().resource_mut::<SettingsMenu>().selected = at.expect("in the ladder");
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Enter);
            app.update();
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::Enter);
            keys.clear();
            app.update();
            let screen = *app.world().resource::<State<Screen>>().get();
            assert_eq!(screen == Screen::Menu, row.is_dial(), "{row:?}: {screen:?}");
        }
    }

    /// The reset clears every stage, and the lines kept for other builds
    /// with them.
    #[test]
    fn resetting_progress_clears_the_record() {
        use crate::app::progress::Progress;

        let mut progress = Progress::parse("cleared: beach:First Flood\n");
        progress.mark("Welcome Ashore");
        progress.mark("Undertow");
        progress.clear_all();
        assert!(!progress.is_cleared("Welcome Ashore"));
        assert!(!progress.is_cleared("Undertow"));
        assert!(progress.to_text().is_empty(), "and nothing to write back");
    }

    /// "Rounds kept" lowered and then the key bindings visited: the shelf
    /// keeps every round while the player is in there and on the way back,
    /// and is trimmed once, to the lowered cap, as Settings is left for
    /// the menu.
    #[test]
    fn a_visit_to_the_key_bindings_keeps_the_shelf_until_settings_is_left() {
        use crate::app::replays::{Library, file_name, shelf_in};
        let dir = crate::app::paths::test_dir("cap-through-controls");
        for n in 0..5u64 {
            std::fs::write(dir.join(file_name(1_751_328_000 + 60 * n, Some("P1"))), "x")
                .expect("stocked");
        }
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.insert_state(Screen::Menu);
        app.init_resource::<GameSettings>();
        app.init_resource::<SettingsMenu>();
        app.insert_resource(Library {
            dir: dir.clone(),
            ..Library::default()
        });
        app.add_systems(OnEnter(Screen::Settings), note_replay_cap);
        app.add_systems(OnExit(Screen::Settings), apply_replay_cap);
        app.add_systems(OnExit(Screen::Controls), apply_replay_cap);
        let go = |app: &mut App, to: Screen| {
            app.world_mut().resource_mut::<NextState<Screen>>().set(to);
            app.update();
        };
        app.world_mut().resource_mut::<GameSettings>().replay_cap = 20;
        go(&mut app, Screen::Settings);
        app.world_mut().resource_mut::<GameSettings>().replay_cap = 2;
        go(&mut app, Screen::Controls);
        assert_eq!(
            shelf_in(&dir).len(),
            5,
            "not on the way to the key bindings"
        );
        go(&mut app, Screen::Settings);
        assert_eq!(shelf_in(&dir).len(), 5, "nor on the way back");
        go(&mut app, Screen::Menu);
        assert_eq!(shelf_in(&dir).len(), 2, "trimmed as Settings is left");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
