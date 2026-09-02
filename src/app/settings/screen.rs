//! The settings screen: the row ladder, its input, and its renderer.
//!
//! Split from the settings *model* next door, which the whole game reads
//! and which has no reason to depend on Bevy UI.

use super::{
    CommitScheme, DEADZONE_RANGE, GameSettings, REPEAT_DELAY_RANGE, REPEAT_INTERVAL_RANGE,
    SeatInput, UI_SCALE_MAX, UI_SCALE_MIN, VOLUME_RANGE,
};
use crate::app::Screen;
use crate::app::cycle::{Cycle, Turn, dial};
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
    UiScale,
    ReducedMotion,
    /// How many finished rounds the shelf keeps.
    ReplayCap,
    Language,
    /// Whether start-up asks GitHub for a newer release.
    UpdateCheck,
    /// Forgets every cleared stage. Last, and behind a confirmation.
    ResetProgress,
}

impl Row {
    /// Display and navigation order, which is [`SECTIONS`] flattened. The
    /// rows used to run in the order they were added, which put the pad
    /// deadzone six rows below the keys it belongs with.
    pub const ALL: [Row; 22] = [
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
        Row::UiScale,
        Row::ReducedMotion,
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
                Row::UiScale,
                Row::ReducedMotion,
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
/// [`DESIGN_W`](crate::app::settings::DESIGN_W), which is what keeps them
/// honest - `the_card_fits_the_window_it_was_drawn_for` holds them to it.
///
/// Both cells clip what overruns rather than wrapping it, so a row that
/// does not fit loses its tail with no warning anywhere -
/// `every_row_fits_its_cell_in_every_language` is what says so instead.
/// That check measures pixels rather than characters, because two faces
/// draw these rows: DejaVu Sans Mono at 0.602 em and the Japanese subset
/// at a full one.
const LABEL_W: f32 = 252.0;
const VALUE_W: f32 = 322.0;
/// A point under [`menu_ui::type_scale::ROW`], and the only list card in
/// the game that is.
///
/// Not drift: it is the size at which two columns of rows fit a
/// 1280-wide window in every language. At the scale's 19 the German duos
/// value runs 332px into a 322px cell, and the ten pixels the cell would
/// need are twenty on the card, which has eighteen to give. Both halves
/// are held by tests below; this line is the third thing they hold.
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
) {
    let tr = settings.tr();
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
            // of rows rather than a single run of them. Built by hand
            // before, which is how it came to be the only list card in the
            // game standing flat on the sand with no shadow under it.
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
                            for (group, rows) in column {
                                side.spawn((
                                    Text::new(group.label(tr)),
                                    TextFont {
                                        font_size: FontSize::Px(menu_ui::type_scale::BODY),
                                        ..default()
                                    },
                                    TextColor(palette::GOLD.with_alpha(0.55)),
                                    Node {
                                        margin: UiRect::top(Val::Px(if index == 0 {
                                            0.0
                                        } else {
                                            10.0
                                        }))
                                        .with_bottom(Val::Px(3.0))
                                        .with_left(Val::Px(10.0)),
                                        ..default()
                                    },
                                ));
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
            // Read-only controller mapping reference, under the card.
            for line in [tr.pad_help1, tr.pad_help2] {
                // On its own pill: below the card these lines sit on open
                // sand, and on a small window partly under the card, where
                // bare dim text disappears.
                wrap.spawn((
                    Text::new(line),
                    TextFont {
                        font_size: FontSize::Px(menu_ui::type_scale::BODY),
                        ..default()
                    },
                    TextLayout::no_wrap(),
                    TextColor(palette::PARCHMENT.with_alpha(0.70)),
                    Node {
                        padding: UiRect::axes(Val::Px(10.0), Val::Px(2.0)),
                        border_radius: BorderRadius::all(Val::Px(9.0)),
                        ..default()
                    },
                    BackgroundColor(palette::PILL_FILL.with_alpha(0.6)),
                ));
            }
        });
}

/// One dial: its name in a fixed gutter and its value in another, so every
/// value on the card starts at the same x and the column can be read down.
///
/// The language row is the one exception: its value carries a flag chip in
/// front of the name, which pushes that one value right by the width of the
/// chip. A blank slot on every other row would keep the column
/// perfectly straight and cost every one of them the same width for
/// nothing, so the indent stands.
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
                    crate::app::progress::save(&progress);
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
            | Row::UiScale
            | Row::ReducedMotion
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
        Row::CommitKeys => settings.commit = settings.commit.cycled(turn),
        Row::RepeatDelay => {
            settings.repeat_delay = f32::clamp(
                settings.repeat_delay + step * 0.05,
                *REPEAT_DELAY_RANGE.start(),
                *REPEAT_DELAY_RANGE.end(),
            );
        }
        Row::RepeatRate => {
            settings.repeat_interval = f32::clamp(
                settings.repeat_interval + step * 0.02,
                *REPEAT_INTERVAL_RANGE.start(),
                *REPEAT_INTERVAL_RANGE.end(),
            );
        }
        Row::MusicOn => settings.music_on = !settings.music_on,
        Row::SfxOn => settings.sfx_on = !settings.sfx_on,
        Row::Music => settings.music_volume = dial(settings.music_volume, turn, 10, VOLUME_RANGE),
        Row::Sfx => settings.sfx_volume = dial(settings.sfx_volume, turn, 10, VOLUME_RANGE),
        Row::VersusMode => settings.team_mode = settings.team_mode.cycled(turn),
        Row::Speed => {
            settings.puzzle_speed = match (settings.puzzle_speed, turn) {
                (100, Turn::Left) | (50, Turn::Right) => 75,
                (75, Turn::Left) => 50,
                (75, Turn::Right) => 100,
                (other, _) => other,
            };
        }
        Row::Rumble => settings.rumble = !settings.rumble,
        Row::Deadzone => {
            settings.pad_deadzone = dial(settings.pad_deadzone, turn, 10, DEADZONE_RANGE);
        }
        Row::Palette => settings.colorblind = !settings.colorblind,
        Row::UiScale => {
            settings.ui_scale = dial(settings.ui_scale, turn, 10, UI_SCALE_MIN..=UI_SCALE_MAX);
        }
        Row::ReducedMotion => settings.reduced_motion = !settings.reduced_motion,
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

/// What one row reads as: its label and the value beside it, or the whole
/// line for the rows that are doors rather than dials.
///
/// Whether the cursor puts `< >` around this row's value. Doors do not
/// step through anything, so they wear none - and the widest a value ever
/// draws depends on the answer, which is why this is one rule rather than
/// one in the screen and another in the test that measures it.
fn has_arrows(row: Row) -> bool {
    !matches!(row, Row::KeyBindings | Row::ResetProgress)
}

/// Pure, so the wording of every setting can be checked without a window -
/// this is the screen a player spends the most time reading.
/// A row's name and its current value, undecorated. The screen adds the
/// `< >` arrows to the row the cursor is on, so the arrows read as "these
/// keys, on this row" rather than as part of every value.
pub(super) fn row_text(
    tr: &crate::app::i18n::Tr,
    settings: &GameSettings,
    caps: &crate::app::keycaps::KeyCaps,
    row: Row,
    reset: ResetPrompt,
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
        Row::Speed => (tr.set_speed, format!("{}%", settings.puzzle_speed)),
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
        Row::UiScale => (tr.set_ui_scale, format!("{}%", settings.ui_scale)),
        Row::ReducedMotion => (tr.set_reduced_motion, on_off(settings.reduced_motion)),
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
            return (tr.set_key_bindings.to_string(), tr.val_open.to_string());
        }
        Row::ResetProgress => {
            let state = match reset {
                ResetPrompt::Idle => tr.val_reset,
                ResetPrompt::Armed => tr.val_reset_confirm,
                ResetPrompt::Done => tr.val_reset_done,
            };
            return (tr.set_reset_progress.to_string(), state.to_string());
        }
    };
    (label.to_string(), value)
}

pub fn update_settings_ui(
    settings: Res<GameSettings>,
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
        let (label, value) = row_text(tr, &settings, &caps, row, menu.reset);
        let line = match cell.1 {
            Half::Label => label,
            Half::Value if picked && has_arrows(row) => format!("< {value} >"),
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
    use crate::app::i18n::EN;

    /// Every row says what it is and what it is set to, in every language.
    /// A row added without wording would otherwise show an empty dial.
    /// The card is built by walking [`SECTIONS`] with a running index, and
    /// every row's label is looked up by that index in [`Row::ALL`]. If the
    /// two ever disagree the screen shows one dial's name over another's
    /// value, silently. The comment on `Row::ALL` says a test keeps them in
    /// step; this is it.
    #[test]
    fn the_sections_are_the_row_order() {
        let flat: Vec<Row> = SECTIONS
            .iter()
            .flat_map(|column| column.iter())
            .flat_map(|(_, rows)| rows.iter().copied())
            .collect();
        assert_eq!(flat, Row::ALL.to_vec());
    }

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
    /// tail with no warning anywhere - the widest of them lost the closing
    /// bracket off the paired team mode in five languages, and had done
    /// since the mode was added, with a budget written in characters and
    /// nothing to check it against ([`LABEL_W`] says why characters were
    /// the wrong unit besides).
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
                puzzle_speed: 100,
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
                for reset in [ResetPrompt::Idle, ResetPrompt::Armed, ResetPrompt::Done] {
                    for team_mode in crate::app::teams::TeamMode::ALL {
                        // Every layout by name, since "Auto" is
                        // the short one and nobody leaves a dial
                        // on its default because it was there.
                        for keyboard in <Option<crate::app::keycaps::Layout> as Cycle>::VARIANTS {
                            let settings = GameSettings {
                                team_mode,
                                keyboard: *keyboard,
                                ..settings.clone()
                            };
                            let (label, value) = row_text(
                                tr,
                                &settings,
                                &crate::app::keycaps::KeyCaps::default(),
                                row,
                                reset,
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
    /// interface was drawn for. This is the other half of the budget: a
    /// row too long for its cell clips, and a cell too wide for the card
    /// runs the whole thing off the edge of a 1280-wide screen. Widening
    /// one to fix the first is what would cause the second.
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
        let at = |s: &GameSettings, row| row_text(&EN, s, &caps, row, ResetPrompt::Idle).1;
        assert!(at(&settings, Row::Music).contains("42%"));
        settings.ui_scale = 130;
        assert!(at(&settings, Row::UiScale).contains("130%"));
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
        let line = |reset| row_text(&EN, &settings, &caps, Row::ResetProgress, reset).1;
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
            let say = |reset| row_text(tr, &settings, &caps, Row::ResetProgress, reset).1;
            assert_ne!(say(ResetPrompt::Idle), say(ResetPrompt::Armed), "{lang:?}");
            assert_ne!(say(ResetPrompt::Armed), say(ResetPrompt::Done), "{lang:?}");
        }
    }

    /// Enter on the reset row arms it and *stays* on the screen. Enter on
    /// every other row means "done here", so this row has to hold its own
    /// against that.
    ///
    /// Exactly one `update()`, and that is load-bearing: this app has no
    /// `InputPlugin`, so nothing clears `just_pressed` between frames and a
    /// second update would press Enter again, confirm the reset, and write
    /// over the real `progress.txt` in the tester's data directory. The
    /// seeded mark below is the tripwire if anyone adds one.
    #[test]
    fn arming_the_reset_does_not_leave_the_screen() {
        use crate::app::CampaignKind;
        use crate::app::progress::Progress;

        let mut progress = Progress::default();
        progress.mark(CampaignKind::TidePool, "Welcome Ashore");

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
                .is_cleared(CampaignKind::TidePool, "Welcome Ashore"),
            "arming alone must not clear anything"
        );
    }

    /// The reset clears both lists, since they share one record.
    #[test]
    fn resetting_progress_clears_both_campaigns() {
        use crate::app::CampaignKind;
        use crate::app::progress::Progress;

        let mut progress = Progress::default();
        progress.mark(CampaignKind::TidePool, "Welcome Ashore");
        progress.mark(CampaignKind::BeachDay, "First Flood");
        progress.clear_all();
        assert!(!progress.is_cleared(CampaignKind::TidePool, "Welcome Ashore"));
        assert!(!progress.is_cleared(CampaignKind::BeachDay, "First Flood"));
        assert!(progress.to_text().is_empty(), "and nothing to write back");
    }
}
