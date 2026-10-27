//! The settings card beside the board: the level's size, its seats on a
//! beach, and every [`Setting`] a kind of level has, each with what it
//! is set to and the key that turns it, the chosen row lit. The header's
//! status line used to carry the signposts and the seats beside the last
//! thing said, and every other setting only as the line a key press left
//! there, gone at the next one.

use super::setting::{self, Setting};
use super::{Command, EditorKeys, EditorState, NEXT_SETTING};
use crate::app::Sim;
use crate::app::gamepad::LastDevice;
use crate::app::gamepad::scheme::{self, Does, Face};
use crate::app::i18n::{Tr, fill};
use crate::app::menu_ui;
use crate::app::settings::GameSettings;
use crate::sim::{Board, Goal, LevelKind, TICKS_PER_SECOND};
use bevy::prelude::*;

/// One of the card's rows: the board's size and seats, which it reads off
/// the board, and the settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Row {
    Size,
    Seats,
    /// Whether a beach's seats walk as far from its holes as each other
    /// (`validate::balance`), read off the board like the seats.
    Balance,
    Setting(Setting),
}

/// Every row the card can show, in its order.
pub(super) const ROWS: [Row; 11] = [
    Row::Size,
    Row::Seats,
    Row::Balance,
    Row::Setting(Setting::Signposts),
    Row::Setting(Setting::Players),
    Row::Setting(Setting::Goal),
    Row::Setting(Setting::Time),
    Row::Setting(Setting::Gulls),
    Row::Setting(Setting::Holes),
    Row::Setting(Setting::Wrap),
    Row::Setting(Setting::Mirror),
];

/// The width of the column the rows' own keys stand in: two characters,
/// "F5" or "K", at the card's size.
const KEY_W: f32 = 20.0;

#[derive(Component)]
pub struct CardRow(Row);

#[derive(Component)]
pub struct CardValue(Row);

#[derive(Component)]
pub struct CardKey(Row);

/// The card's foot: how to choose a row and turn it.
#[derive(Component)]
pub struct CardHint;

impl Row {
    /// Whether a level of `kind` has this row: the size always, the seats
    /// on a beach, and the settings [`Setting::rows`] gives it.
    pub(super) fn shown(self, kind: LevelKind) -> bool {
        match self {
            Row::Size => true,
            Row::Seats | Row::Balance => kind == LevelKind::Arena,
            Row::Setting(setting) => Setting::rows(kind).contains(&setting),
        }
    }

    pub(super) fn label(self, tr: &Tr) -> &'static str {
        match self {
            Row::Size => tr.ed_set_size,
            Row::Seats => tr.ed_set_seats,
            Row::Balance => tr.ed_set_balance,
            Row::Setting(Setting::Signposts) => tr.ed_set_posts,
            Row::Setting(Setting::Players) => tr.ed_set_players,
            Row::Setting(Setting::Goal) => tr.ed_set_goal,
            Row::Setting(Setting::Time) => tr.ed_set_time,
            Row::Setting(Setting::Gulls) => tr.ed_set_gulls,
            Row::Setting(Setting::Holes) => tr.ed_set_holes,
            Row::Setting(Setting::Wrap) => tr.ed_set_wrap,
            Row::Setting(Setting::Mirror) => tr.ed_set_mirror,
        }
    }

    /// What the row is set to on this bench.
    pub(super) fn value(self, tr: &Tr, state: &EditorState, board: &Board) -> String {
        let secs = |ticks: u32| format!("{}", ticks as f32 / TICKS_PER_SECOND as f32);
        match self {
            Row::Size => format!("{}x{}", board.width(), board.height()),
            // The table it seats, which is none while a player is skipped:
            // the same count the map dial offers it at.
            Row::Seats => board.table_seats().unwrap_or(0).to_string(),
            Row::Balance => match super::validate::balance(board) {
                super::validate::Balance::Unknown => "-".to_string(),
                super::validate::Balance::Even { .. } => tr.ed_balances[0].to_string(),
                super::validate::Balance::Uneven { .. } => tr.ed_balances[1].to_string(),
                super::validate::Balance::CutOff(_) => tr.ed_balances[2].to_string(),
            },
            // On a co-op stage the grant is P1's hand, and says so.
            Row::Setting(Setting::Signposts) => match state.second_hand {
                Some(_) => format!("{} {}", crate::app::seat_label(tr, 0), state.posts),
                None => state.posts.to_string(),
            },
            Row::Setting(Setting::Players) => match state.second_hand {
                None => "1".to_string(),
                Some(p2) => format!("2: {} {p2}", crate::app::seat_label(tr, 1)),
            },
            Row::Setting(Setting::Goal) => goal_value(tr, state.goal),
            Row::Setting(Setting::Time) => {
                fill(tr.ed_secs, &[("s", &setting::time_secs(board).to_string())])
            }
            Row::Setting(Setting::Gulls) => match board.rules().gull_period {
                0 => tr.val_off.to_string(),
                period => fill(tr.ed_every, &[("s", &secs(period))]),
            },
            Row::Setting(Setting::Holes) => {
                fill(tr.ed_every, &[("s", &secs(setting::hole_period(state)))])
            }
            Row::Setting(Setting::Wrap) => match board.wrap() {
                true => tr.val_on.to_string(),
                false => tr.val_off.to_string(),
            },
            Row::Setting(Setting::Mirror) => tr.ed_mirrors[state.symmetry.index()].to_string(),
        }
    }

    /// The key of the row's own at a keyboard, if it has one: F5 for the
    /// size, K and O for the flock and the wrap, wherever P1's bindings
    /// have put those ([`EditorKeys::command`]). The chosen row is turned
    /// by "+" and "-", which the card's foot says rather than each row
    /// saying it again; and a pad has no keys of a row's own, its size
    /// being on the Select layer the prompt line spells out.
    pub(super) fn key(
        self,
        device: LastDevice,
        keys: &EditorKeys,
        caps: &crate::app::keycaps::KeyCaps,
    ) -> String {
        match (self, device) {
            (_, LastDevice::Pad) => String::new(),
            (Row::Size, LastDevice::Keyboard) => caps.label(KeyCode::F5),
            (Row::Setting(Setting::Gulls), LastDevice::Keyboard) => {
                caps.label(keys.command(Command::Gulls))
            }
            (Row::Setting(Setting::Wrap), LastDevice::Keyboard) => {
                caps.label(keys.command(Command::Wrap))
            }
            (Row::Seats | Row::Balance | Row::Setting(_), LastDevice::Keyboard) => String::new(),
        }
    }
}

/// A goal as the card's goal row says it.
pub(super) fn goal_value(tr: &Tr, goal: Goal) -> String {
    match goal {
        Goal::AllCrabs => tr.ed_goal_all.to_string(),
        Goal::Survive => tr.ed_goal_survive.to_string(),
        Goal::Golden => tr.ed_goal_golden.to_string(),
        Goal::Bank(n) => fill(tr.ed_goal_bank, &[("n", &n.to_string())]),
    }
}

/// The card's foot: N and "+/-" at a keyboard, the Select layer's
/// triggers on a pad.
pub(super) fn hint(tr: &Tr, device: LastDevice, caps: &crate::app::keycaps::KeyCaps) -> String {
    match device {
        LastDevice::Keyboard => fill(tr.ed_settings_keys, &[("next", &caps.label(NEXT_SETTING))]),
        LastDevice::Pad => {
            let face = Face::Editor { layer: true };
            let names: Vec<&str> = scheme::binds(&face)
                .into_iter()
                .filter(|row| matches!(row.does, Does::PrevSetting | Does::NextSetting))
                .map(|row| row.pad.name(tr))
                .collect();
            let button = format!("{}+{}", scheme::Pad::Select.name(tr), names.join("/"));
            fill(tr.pad.settings_card, &[("b", &button)])
        }
    }
}

/// The card, on the board's left as the palette is on its right.
pub(super) fn spawn_settings_card(commands: &mut Commands, tr: &Tr) {
    let fine = |text: String, color: Color| {
        (
            Text::new(text),
            TextFont {
                font_size: FontSize::Px(menu_ui::type_scale::FINE),
                ..default()
            },
            TextColor(color),
        )
    };
    commands
        .spawn((
            super::EditorUi,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(70.0),
                left: Val::Px(super::palette::CARD_MARGIN),
                width: Val::Px(super::palette::CARD_W),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::all(Val::Px(super::palette::CARD_PAD)),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(12.0)),
                ..default()
            },
            BorderColor::all(crate::app::palette::CARD_EDGE),
            BackgroundColor(crate::app::palette::CARD_FILL),
            menu_ui::card_shadow(),
        ))
        .with_children(|card| {
            for row in ROWS {
                card.spawn((
                    CardRow(row),
                    Node {
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(super::palette::CELL_GAP),
                        padding: UiRect::axes(Val::Px(super::palette::ROW_PAD), Val::Px(3.0)),
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|line| {
                    line.spawn(fine(
                        row.label(tr).to_string(),
                        crate::app::palette::PARCHMENT,
                    ));
                    line.spawn(Node {
                        column_gap: Val::Px(super::palette::CELL_GAP),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|right| {
                        right.spawn((
                            CardValue(row),
                            fine(String::new(), crate::app::palette::PARCHMENT),
                        ));
                        // A column of its own, as wide on every row, so
                        // the values line up whether a row has a key or
                        // not.
                        right.spawn((
                            CardKey(row),
                            fine(String::new(), crate::app::palette::IDLE_ROW),
                            Node {
                                width: Val::Px(KEY_W),
                                ..default()
                            },
                        ));
                    });
                });
            }
            card.spawn((
                CardHint,
                fine(
                    String::new(),
                    crate::app::palette::PARCHMENT.with_alpha(0.55),
                ),
                Node {
                    margin: UiRect::top(Val::Px(6.0)),
                    ..default()
                },
            ));
        });
}

/// The card's texts, which the system writes each frame.
type CardTexts<'w, 's> = ParamSet<
    'w,
    's,
    (
        Query<
            'static,
            'static,
            (
                &'static CardValue,
                &'static mut Text,
                &'static mut TextColor,
            ),
        >,
        Query<'static, 'static, (&'static CardKey, &'static mut Text)>,
        Query<'static, 'static, &'static mut Text, With<CardHint>>,
    ),
>;

/// Show the rows the level's kind has, say what each is set to and what
/// turns it, and light the chosen one.
pub fn update_settings_card(
    state: Res<EditorState>,
    sim: Res<Sim>,
    (settings, device, caps): (
        Res<GameSettings>,
        Res<LastDevice>,
        Res<crate::app::keycaps::KeyCaps>,
    ),
    mut rows: Query<(&CardRow, &mut Node, &mut BackgroundColor)>,
    mut texts: CardTexts,
) {
    let tr = settings.tr();
    let board = &sim.0;
    let keys = EditorKeys::of(&settings);
    let chosen = Row::Setting(Setting::chosen(&state));
    for (row, mut node, mut fill_color) in &mut rows {
        menu_ui::set_shown(&mut node, row.0.shown(state.kind));
        let want = match row.0 == chosen {
            true => crate::app::palette::SELECTED_ROW.with_alpha(0.22),
            false => Color::NONE,
        };
        menu_ui::set_bg(&mut fill_color, want);
    }
    for (value, mut text, mut color) in &mut texts.p0() {
        menu_ui::set_text(&mut text, &value.0.value(tr, &state, board));
        let want = match value.0 == chosen {
            true => crate::app::palette::SELECTED_ROW,
            false => crate::app::palette::PARCHMENT,
        };
        menu_ui::set_color(&mut color, want);
    }
    for (key, mut text) in &mut texts.p1() {
        let said = key.0.key(*device, &keys, &caps);
        menu_ui::set_text(&mut text, &said);
    }
    for mut text in &mut texts.p2() {
        menu_ui::set_text(&mut text, &hint(tr, *device, &caps));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::i18n::metrics::text_px;

    /// The balance row says "cut off" of a seat some hole cannot reach,
    /// which is worse than uneven and was shown as it.
    #[test]
    fn the_balance_row_names_a_cut_off_seat() {
        use crate::sim::{Direction, Spawner, TileKind};
        let tr = &crate::app::i18n::EN;
        let mut board = Board::new(7, 3, 1);
        let hole = TileKind::Spawner(Spawner {
            dir: Direction::Up,
            period: 60,
        });
        board.set_tile(3, 1, hole);
        board.set_tile(0, 1, TileKind::Castle(0));
        board.set_tile(6, 1, TileKind::Castle(1));
        let state = EditorState::default();
        assert_eq!(Row::Balance.value(tr, &state, &board), tr.ed_balances[0]);
        for y in 0..3 {
            board.set_tile(5, y, TileKind::Rock);
        }
        assert_eq!(Row::Balance.value(tr, &state, &board), tr.ed_balances[2]);
    }

    /// Every row fits the card on one line in every language, at the
    /// longest value it can show: the label, the value and the row's own
    /// key, with the gaps between them. A row that wrapped pushed the card
    /// down over the prompt line on the shortest window.
    #[test]
    fn every_row_fits_the_card_in_every_language() {
        let room = super::super::palette::ROW_ROOM;
        let gap = super::super::palette::CELL_GAP;
        let font = menu_ui::type_scale::FINE;
        let mut over = Vec::new();
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            let goals = setting::GOALS.map(|goal| goal_value(tr, goal));
            let longest = |row: Row| -> Vec<String> {
                match row {
                    Row::Size => vec!["20x13".into()],
                    Row::Seats => vec!["6".into()],
                    Row::Balance => tr.ed_balances.iter().map(|word| word.to_string()).collect(),
                    Row::Setting(Setting::Signposts) => {
                        vec!["9".into(), format!("{} 9", crate::app::seat_label(tr, 0))]
                    }
                    Row::Setting(Setting::Players) => {
                        vec![
                            "1".into(),
                            format!("2: {} 9", crate::app::seat_label(tr, 1)),
                        ]
                    }
                    Row::Setting(Setting::Goal) => goals.to_vec(),
                    Row::Setting(Setting::Time) => vec![fill(tr.ed_secs, &[("s", "180")])],
                    Row::Setting(Setting::Gulls) => {
                        vec![fill(tr.ed_every, &[("s", "16")]), tr.val_off.to_string()]
                    }
                    Row::Setting(Setting::Holes) => vec![fill(tr.ed_every, &[("s", "6")])],
                    Row::Setting(Setting::Wrap) => {
                        vec![tr.val_on.to_string(), tr.val_off.to_string()]
                    }
                    Row::Setting(Setting::Mirror) => {
                        tr.ed_mirrors.iter().map(|name| name.to_string()).collect()
                    }
                }
            };
            for row in ROWS {
                // Its own key, moved to a function key where P1's keys
                // take its letter: F5, F7 or F8.
                let keys = match row {
                    Row::Size | Row::Setting(Setting::Gulls | Setting::Wrap) => ["F8"],
                    Row::Seats | Row::Balance | Row::Setting(_) => [""],
                };
                for value in longest(row) {
                    for key in &keys {
                        let used = text_px(row.label(tr), font)
                            + text_px(&value, font)
                            + KEY_W.max(text_px(key, font))
                            + 2.0 * gap;
                        if used > room {
                            over.push(format!("{lang:?} {row:?}: {used:.0}px {value} {key}"));
                        }
                    }
                }
            }
            // The foot wraps onto a second line at most.
            for foot in [
                hint(tr, LastDevice::Keyboard, &Default::default()),
                hint(tr, LastDevice::Pad, &Default::default()),
            ] {
                let used = text_px(&foot, font);
                if used > 2.0 * (room + 2.0 * super::super::palette::ROW_PAD) {
                    over.push(format!("{lang:?} foot: {used:.0}px {foot}"));
                }
            }
        }
        assert!(over.is_empty(), "past {room:.0}px:\n{}", over.join("\n"));
        assert!(
            text_px("F8", font) <= KEY_W,
            "a function key fits its column"
        );
    }

    /// The card says what each setting is set to from the bench itself:
    /// a carried goal, a stage's own time, the flock and the holes' rate.
    #[test]
    fn the_card_reads_the_bench() {
        use crate::app::i18n::EN;
        let mut state = EditorState::default();
        let mut board = Board::new(9, 7, 1);
        let value = |row, state: &EditorState, board: &Board| Row::value(row, &EN, state, board);
        assert_eq!(value(Row::Size, &state, &board), "9x7");
        assert_eq!(
            value(Row::Setting(Setting::Goal), &state, &board),
            EN.ed_goal_all
        );
        state.goal = Goal::Bank(5);
        assert_eq!(
            value(Row::Setting(Setting::Goal), &state, &board),
            "score 5"
        );
        assert_eq!(value(Row::Setting(Setting::Time), &state, &board), "60 s");
        board.rules_mut().round_length = Some(90 * TICKS_PER_SECOND);
        assert_eq!(value(Row::Setting(Setting::Time), &state, &board), "90 s");
        assert_eq!(value(Row::Setting(Setting::Gulls), &state, &board), "off");
        board.rules_mut().gull_period = 240;
        assert_eq!(
            value(Row::Setting(Setting::Gulls), &state, &board),
            "every 8 s"
        );
        assert_eq!(
            value(Row::Setting(Setting::Holes), &state, &board),
            "every 2 s"
        );
        // Which rows a kind has.
        assert!(!Row::Seats.shown(LevelKind::Puzzle));
        assert!(Row::Seats.shown(LevelKind::Arena));
        assert!(!Row::Setting(Setting::Signposts).shown(LevelKind::Arena));
        assert!(!Row::Setting(Setting::Gulls).shown(LevelKind::Arena));
    }
}
