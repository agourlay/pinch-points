//! The editor's side panel: the kind toggle, the brush palette with its
//! sprites and letters, and the line saying what the cursor stands on.

use super::brush::{Brush, describe};
use super::{EditorKeys, EditorState};
use crate::app::Sim;
use crate::app::cursor::Cursor;
use crate::app::gamepad::LastDevice;
use crate::app::i18n::fill;
use crate::app::menu_ui;
use crate::app::settings::GameSettings;
use crate::sim::LevelKind;
use bevy::prelude::*;

#[derive(Component)]
pub struct EditorUi;

/// The width of the editor's two cards, the palette and the settings.
pub const CARD_W: f32 = 226.0;
/// How far each card stands in from the window's side.
pub const CARD_MARGIN: f32 = 16.0;
/// A card's padding inside its border.
pub(super) const CARD_PAD: f32 = 10.0;
/// A row's padding inside its lit background.
pub(super) const ROW_PAD: f32 = 6.0;
/// The gap between a row's cells: icon and name, label and value.
pub(super) const CELL_GAP: f32 = 8.0;
/// The side of a brush's icon.
const ICON_PX: f32 = 22.0;
/// The room a card's row has for its text: the card less its border, its
/// padding and the row's own, which the width guards measure against.
#[cfg(test)]
pub(super) const ROW_ROOM: f32 = CARD_W - 2.0 * 2.0 - 2.0 * CARD_PAD - 2.0 * ROW_PAD;

/// One row of the palette, by its place in [`Brush::ALL`].
#[derive(Component)]
pub struct PaletteRow(usize);

#[derive(Component)]
pub struct PaletteLabel(usize);

/// The line over the kind toggle, which names the key that flips it.
#[derive(Component)]
pub struct KindLabel;

/// One of the two rows of the kind toggle, by what it selects.
#[derive(Component)]
pub struct KindRow(LevelKind);

/// What the cursor is standing on, which is the other half of knowing what
/// a keypress is about to do.
#[derive(Component)]
pub struct UnderCursor;

/// What the loaded brush would make of the cursor's tile, for the brushes
/// that step through seats, ways and kinds: said only when it differs.
#[derive(Component)]
pub struct NextUnder;

/// The brush palette: every paintable thing, its sprite, and the letter
/// that loads it.
///
/// The editor said `R C H L W P B G O tiles` along the bottom and left the
/// rest to memory, which is a poor deal for a screen whose job is making
/// things.
pub fn spawn_editor_ui(
    mut commands: Commands,
    art: Res<crate::app::art::Art>,
    settings: Res<GameSettings>,
) {
    let tr = settings.tr();
    let keys = EditorKeys::of(&settings);
    super::card::spawn_settings_card(&mut commands, tr);
    super::playtest::spawn_banner(&mut commands);
    commands
        .spawn((
            EditorUi,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(70.0),
                right: Val::Px(CARD_MARGIN),
                width: Val::Px(CARD_W),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::all(Val::Px(CARD_PAD)),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(12.0)),
                ..default()
            },
            BorderColor::all(crate::app::palette::CARD_EDGE),
            BackgroundColor(crate::app::palette::CARD_FILL),
            menu_ui::card_shadow(),
        ))
        .with_children(|panel| {
            // What is being built, above the brushes and lit like them: the
            // choice decides which list the level joins, and a toggle you
            // cannot see the state of is a toggle you press to find out.
            panel.spawn((
                KindLabel,
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
                            column_gap: Val::Px(CELL_GAP),
                            padding: UiRect::axes(Val::Px(ROW_PAD), Val::Px(3.0)),
                            border_radius: BorderRadius::all(Val::Px(6.0)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .with_children(|row| {
                        spawn_icon(row, *brush, &art);
                        row.spawn((
                            PaletteLabel(i),
                            Text::new(brush_line(tr, *brush, LastDevice::Keyboard, &keys)),
                            TextFont {
                                font_size: FontSize::Px(menu_ui::type_scale::BODY),
                                ..default()
                            },
                            TextColor(crate::app::palette::PARCHMENT),
                        ));
                    });
            }
            for (line, top) in [(true, 8.0), (false, 0.0)] {
                let mut text = panel.spawn((
                    Text::new(String::new()),
                    TextFont {
                        font_size: FontSize::Px(menu_ui::type_scale::FINE),
                        ..default()
                    },
                    TextColor(crate::app::palette::IDLE_ROW),
                    Node {
                        margin: UiRect::top(Val::Px(top)),
                        ..default()
                    },
                ));
                match line {
                    true => text.insert(UnderCursor),
                    false => text.insert(NextUnder),
                };
            }
        });
}

/// A brush's icon on its row. The crab's sprite is drawn to be tinted and
/// to stand on sand: white, with a dark outline, it was a pale blob on the
/// dark card. It stands on a coin of sand in a common crab's colours, as
/// the field guide draws its crabs.
fn spawn_icon(row: &mut ChildSpawnerCommands, brush: Brush, art: &crate::app::art::Art) {
    let size = Node {
        width: Val::Px(ICON_PX),
        height: Val::Px(ICON_PX),
        ..default()
    };
    if brush != Brush::Crab {
        row.spawn((ImageNode::new(brush.icon(art)), size));
        return;
    }
    let layer = |image: &Handle<Image>, color: Color| {
        (
            ImageNode::new(image.clone()).with_color(color),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
        )
    };
    let kind = crate::sim::CrabKind::Common;
    row.spawn((
        Node {
            border_radius: BorderRadius::MAX,
            ..size
        },
        BackgroundColor(crate::app::palette::LEGEND_SAND),
    ))
    .with_children(|coin| {
        coin.spawn(layer(&art.crab, crate::app::creatures::body_color(kind)));
        coin.spawn(layer(
            &art.claw,
            crate::app::creatures::claw_color(crate::sim::Handedness::Right, false),
        ));
    });
}

/// The palette's rows that light up for what is chosen: the brushes,
/// their labels, and the level kinds.
#[derive(bevy::ecs::system::SystemParam)]
pub struct PaletteRows<'w, 's> {
    brushes: Query<'w, 's, (&'static PaletteRow, &'static mut BackgroundColor)>,
    kinds: Query<'w, 's, (&'static KindRow, &'static mut BackgroundColor), Without<PaletteRow>>,
    labels: BrushLabels<'w, 's>,
}

/// A brush's row on the palette: the letter that loads it, then its name.
/// A pad's player loads brushes with the bumpers and has no letters, and
/// a letter beside the name would read as the pad button of the same
/// letter, so their row is the name alone. A letter P1's movement or wall
/// keys have taken loads nothing ([`EditorKeys::brush`]), and a dash
/// stands in its place: the brush is reached with Tab and Space.
fn brush_line(
    tr: &crate::app::i18n::Tr,
    brush: Brush,
    device: LastDevice,
    keys: &EditorKeys,
) -> String {
    match (device, keys.brush(brush)) {
        (LastDevice::Keyboard, Some(_)) => format!("{}  {}", brush.letter(), brush.label(tr)),
        (LastDevice::Keyboard, None) => format!("-  {}", brush.label(tr)),
        (LastDevice::Pad(_), _) => brush.label(tr).to_string(),
    }
}

/// The line over the kind toggle. Every language opens it with the key,
/// "F6", which a pad's player is told as the editor table's button for it.
fn kind_line(tr: &crate::app::i18n::Tr, device: LastDevice) -> String {
    use crate::app::gamepad::scheme::{Does, button_name};
    let face = crate::app::gamepad::Face::Editor { layer: true };
    match (device, button_name(&face, Does::Kind, tr, device.family())) {
        (LastDevice::Pad(_), Some(button)) => tr.ed_kind_row.replacen("F6", &button, 1),
        _ => tr.ed_kind_row.to_string(),
    }
}

/// The brushes' names on the palette, apart from the panel's other lines.
type BrushLabels<'w, 's> = Query<
    'w,
    's,
    (
        &'static PaletteLabel,
        &'static mut Text,
        &'static mut TextColor,
    ),
    (Without<UnderCursor>, Without<KindLabel>, Without<NextUnder>),
>;

/// The panel's lines of its own: what the cursor stands on, what the
/// brush would make of it, and the one over the kind toggle.
type PanelLines<'w, 's> = ParamSet<
    'w,
    's,
    (
        Query<'static, 'static, &'static mut Text, With<UnderCursor>>,
        Query<'static, 'static, &'static mut Text, With<KindLabel>>,
        Query<'static, 'static, (&'static mut Text, &'static mut Node), With<NextUnder>>,
    ),
>;

/// Light the loaded brush and the chosen kind, and say what the cursor is
/// standing on.
pub fn update_editor_palette(
    state: Res<EditorState>,
    sim: Res<Sim>,
    (settings, device): (Res<GameSettings>, Res<LastDevice>),
    cursors: Query<&Cursor>,
    palette_rows: PaletteRows,
    mut lines: PanelLines,
) {
    let PaletteRows {
        brushes: mut rows,
        mut kinds,
        mut labels,
    } = palette_rows;
    let tr = settings.tr();
    let keys = EditorKeys::of(&settings);
    for mut text in &mut lines.p1() {
        menu_ui::set_text(&mut text, &kind_line(tr, *device));
    }
    for (row, mut fill) in &mut kinds {
        let want = match row.0 == state.kind {
            true => crate::app::palette::SELECTED_ROW.with_alpha(0.22),
            false => Color::NONE,
        };
        menu_ui::set_bg(&mut fill, want);
    }
    let at = Brush::ALL.iter().position(|b| *b == state.brush);
    for (row, mut fill) in &mut rows {
        let want = match Some(row.0) == at {
            true => crate::app::palette::SELECTED_ROW.with_alpha(0.22),
            false => Color::NONE,
        };
        menu_ui::set_bg(&mut fill, want);
    }
    for (label, mut text, mut color) in &mut labels {
        let want = match Some(label.0) == at {
            true => crate::app::palette::SELECTED_ROW,
            false => crate::app::palette::PARCHMENT,
        };
        menu_ui::set_color(&mut color, want);
        menu_ui::set_text(
            &mut text,
            &brush_line(tr, Brush::ALL[label.0], *device, &keys),
        );
    }
    let Some(cursor) = cursors.iter().next() else {
        return;
    };
    let board = &sim.0;
    let (under, next) = under_and_next(board, cursor.x, cursor.y, &state, tr);
    for mut text in &mut lines.p0() {
        menu_ui::set_text(&mut text, &under);
    }
    for (mut text, mut node) in &mut lines.p2() {
        menu_ui::set_shown(&mut node, next.is_some());
        menu_ui::set_text(&mut text, next.as_deref().unwrap_or_default());
    }
}

/// `text` broken onto lines the card's width holds, at its spaces and
/// nowhere else. Left to the shaper, a line broke before the arrow at its
/// end however the arrow was glued to the word before it, and set it on a
/// line of its own; the readout's words are joined by no-break spaces
/// where they belong together, which this leaves whole.
fn fit_lines(text: &str) -> String {
    let room = CARD_W - 2.0 * 2.0 - 2.0 * CARD_PAD;
    let mut lines: Vec<String> = Vec::new();
    for word in text.split(' ') {
        match lines.last_mut() {
            Some(line) if readout_px(&format!("{line} {word}")) <= room => {
                line.push(' ');
                line.push_str(word);
            }
            Some(_) | None => lines.push(word.to_string()),
        }
    }
    lines.join("\n")
}

/// How wide `line` draws in the readout's face and size: the UI face is
/// monospaced, every character it has one advance wide, and what it lacks
/// (kana and kanji) falls back to the Japanese subset at a full em. The
/// font files are not read at run time, so this counts as
/// `i18n::metrics::text_px` measures, which a test holds it to.
fn readout_px(line: &str) -> f32 {
    const UI_ADVANCE: f32 = 1233.0 / 2048.0;
    line.chars()
        .map(|ch| match ch {
            crate::app::i18n::WORD_JOINER => 0.0,
            '\u{2e80}'.. => 1.0,
            _ => UI_ADVANCE,
        })
        .sum::<f32>()
        * menu_ui::type_scale::FINE
}

/// The readout's two lines: what is on the tile, and what one press of
/// the loaded brush would make of it when that is something else. Read by
/// pressing the brush on a copy of the board with the very function the
/// press uses, the mirror's refusals included ([`super::press_fairly`]),
/// so the line cannot promise what the key does not do.
pub(super) fn under_and_next(
    board: &crate::sim::Board,
    x: u8,
    y: u8,
    state: &EditorState,
    tr: &crate::app::i18n::Tr,
) -> (String, Option<String>) {
    if x >= board.width() || y >= board.height() {
        return (String::new(), None);
    }
    let here = describe(board, x, y, tr);
    let mut after = board.clone();
    let period = super::setting::hole_period(state);
    let _ = super::press_fairly(state, &mut after, (x, y), period);
    let next = describe(&after, x, y, tr);
    let under = fit_lines(&fill(tr.ed_under, &[("t", &here)]));
    (
        under,
        (next != here).then(|| fit_lines(&fill(tr.ed_next, &[("t", &next)]))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::i18n::metrics::text_px;
    use crate::sim::{Board, CrabKind, Direction, Handedness, TileKind};

    /// A brush whose letter P1's keys have taken shows a dash for it:
    /// under the one-hand preset with P1 moving on the arrows, the walls
    /// are on IJKL, and L toggles a wall rather than loading the log.
    #[test]
    fn a_taken_letter_is_not_offered() {
        use crate::app::binds::Action;
        let tr = &crate::app::i18n::EN;
        let mut settings = GameSettings::default();
        let keys = EditorKeys::of(&settings);
        assert_eq!(
            brush_line(tr, Brush::Log, LastDevice::Keyboard, &keys),
            format!("L  {}", Brush::Log.label(tr))
        );
        settings.commit = crate::app::settings::CommitScheme::Ijkl;
        for (action, key) in [
            (Action::MoveUp, KeyCode::ArrowUp),
            (Action::MoveDown, KeyCode::ArrowDown),
            (Action::MoveLeft, KeyCode::ArrowLeft),
            (Action::MoveRight, KeyCode::ArrowRight),
            (Action::PlaceUp, KeyCode::KeyW),
            (Action::PlaceDown, KeyCode::KeyS),
            (Action::PlaceLeft, KeyCode::KeyA),
            (Action::PlaceRight, KeyCode::KeyD),
        ] {
            settings.binds[0].set(action, key);
        }
        let keys = EditorKeys::of(&settings);
        assert_eq!(keys.brush(Brush::Log), None);
        assert_eq!(
            brush_line(tr, Brush::Log, LastDevice::Keyboard, &keys),
            format!("-  {}", Brush::Log.label(tr))
        );
        assert_eq!(
            brush_line(tr, Brush::Log, LastDevice::PAD, &keys),
            Brush::Log.label(tr)
        );
    }

    /// Every brush's row fits beside its icon on one line, in every
    /// language and both hands: "Sand (erases)" wrapped onto two lines.
    #[test]
    fn every_brush_fits_its_row_in_every_language() {
        let room = ROW_ROOM - ICON_PX - CELL_GAP;
        let keys = EditorKeys::of(&GameSettings::default());
        let mut over = Vec::new();
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            for brush in Brush::ALL {
                for device in [LastDevice::Keyboard, LastDevice::PAD] {
                    let line = brush_line(tr, brush, device, &keys);
                    let used = text_px(&line, menu_ui::type_scale::BODY);
                    if used > room {
                        over.push(format!("{lang:?}: {used:.0}px {line}"));
                    }
                }
            }
            for kind in [tr.ed_kind_puzzle, tr.ed_kind_arena] {
                let used = text_px(kind, menu_ui::type_scale::BODY);
                if used > ROW_ROOM {
                    over.push(format!("{lang:?}: {used:.0}px {kind}"));
                }
            }
        }
        assert!(over.is_empty(), "past {room:.0}px:\n{}", over.join("\n"));
    }

    /// The readout says what is on the tile, a crab by its kind, claw and
    /// facing and a castle by its seat, and what the loaded brush would
    /// make of it, read off the press itself.
    #[test]
    fn the_readout_says_what_is_there_and_what_comes_next() {
        use crate::app::i18n::EN;
        let mut board = Board::new(6, 5, 1);
        board.spawn_crab(1, 1, Direction::Right, Handedness::Left, CrabKind::Giant);
        board.set_tile(3, 3, TileKind::Castle(2));
        let mut state = EditorState {
            brush: Brush::Crab,
            ..EditorState::default()
        };
        let (under, next) = under_and_next(&board, 1, 1, &state, &EN);
        // Broken onto the card's lines at a space, never before the arrow.
        assert_eq!(under, "under: giant crab,\nleft claw\u{a0}\u{2060}→");
        assert_eq!(
            next.as_deref(),
            Some("next: giant crab,\nright claw\u{a0}\u{2060}→")
        );
        for line in under.lines() {
            let room = CARD_W - 2.0 * 2.0 - 2.0 * CARD_PAD;
            assert!(text_px(line, menu_ui::type_scale::FINE) <= room, "{line}");
        }
        // The run-time count of a line's width is the one the guards use,
        // in every language's readout.
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            let said = [
                tr.ed_crab_kinds.join(" "),
                tr.ed_claws.join(" "),
                tr.ed_sand.into(),
            ];
            for line in said {
                let want = text_px(&line, menu_ui::type_scale::FINE);
                assert!((readout_px(&line) - want).abs() < 0.01, "{lang:?}: {line}");
            }
        }
        state.brush = Brush::Castle;
        let (under, next) = under_and_next(&board, 3, 3, &state, &EN);
        assert_eq!(under, "under: Castle P3");
        assert_eq!(next.as_deref(), Some("next: Castle P4"));
        let (under, next) = under_and_next(&board, 0, 0, &state, &EN);
        assert_eq!(under, "under: Sand");
        assert_eq!(next.as_deref(), Some("next: Castle P1"));
        // A brush that leaves the tile as it is has nothing to add.
        state.brush = Brush::Sand;
        assert_eq!(under_and_next(&board, 0, 0, &state, &EN).1, None);
    }
}
