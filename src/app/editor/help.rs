//! The editor's cards of words over the bench: every key and pad button
//! it answers to (Shift+F1, or Select+Start on a pad), and the tips a
//! first visit is shown once.
//!
//! The list of keys is not written down here. It is read off the tables
//! the prompts and the pad already read: each face's pad table
//! ([`scheme::binds`]), whose keys are the very keys the bridge presses
//! for a button and so the ones the editor's systems answer to; the keys
//! P1's bindings move ([`EditorKeys`]); the brushes ([`Brush::ALL`]); and
//! the settings card's rows with keys of their own ([`Row::key`]).
//! A key moved in any of them moves here with it.

use super::brush::Brush;
use super::card::{self, Row};
use super::{ARROWS, EditorCard, EditorKeys, EditorState, HELP_KEY, Mode, block_order, shifted};
use crate::app::gamepad::scheme::{self, Bind, Does, Face, Key, Pad};
use crate::app::gamepad::{LastDevice, PadFamily};
use crate::app::i18n::{Tr, fill};
use crate::app::keycaps::KeyCaps;
use crate::app::menu_ui;
use crate::app::settings::GameSettings;
use crate::sim::Direction;
use bevy::prelude::*;

/// One line of the help card: the keys, the pad's buttons, and what they
/// do, in the prompt's own words for it ([`Does::label`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Line {
    pub(super) keys: String,
    pub(super) pad: String,
    pub(super) does: String,
}

/// A heading and its lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Section {
    pub(super) title: String,
    pub(super) lines: Vec<Line>,
}

/// Lines gathered from tables, one per thing done: rows that share their
/// words share a line, as they share a slot in the prompt ("LB/RB:
/// brush").
#[derive(Default)]
struct Gather {
    lines: Vec<Gathered>,
}

/// A line being gathered: its words, its keys, and its buttons, each
/// with the way a stick button is pushed.
type Gathered = (String, Vec<String>, Vec<(String, Option<Direction>)>);

impl Gather {
    fn add(
        &mut self,
        does: String,
        keys: Option<String>,
        pad: Option<(String, Option<Direction>)>,
    ) {
        let at = match self.lines.iter().position(|(said, ..)| *said == does) {
            Some(at) => at,
            None => {
                self.lines.push((does, Vec::new(), Vec::new()));
                self.lines.len() - 1
            }
        };
        let (_, key_list, pad_list) = &mut self.lines[at];
        if let Some(keys) = keys.filter(|keys| !keys.is_empty() && !key_list.contains(keys)) {
            key_list.push(keys);
        }
        if let Some(pad) = pad.filter(|pad| !pad_list.contains(pad)) {
            pad_list.push(pad);
        }
    }

    /// The lines, the stick's ways run together after its name: "right
    /// stick ↑↓←→" rather than the name four times.
    fn lines(self) -> Vec<Line> {
        self.lines
            .into_iter()
            .map(|(does, keys, pads)| {
                let mut names: Vec<String> = Vec::new();
                for (name, way) in &pads {
                    let ways: String = pads
                        .iter()
                        .filter(|(other, _)| other == name)
                        .filter_map(|(_, way)| way.map(super::validate::arrow))
                        .collect();
                    let said = match way {
                        Some(_) => format!("{name} {ways}"),
                        None => name.clone(),
                    };
                    if !names.contains(&said) {
                        names.push(said);
                    }
                }
                Line {
                    keys: match keys.is_empty() {
                        true => "-".to_string(),
                        false => keys.join("/"),
                    },
                    pad: match names.is_empty() {
                        true => "-".to_string(),
                        false => names.join("/"),
                    },
                    does,
                }
            })
            .collect()
    }
}

/// The keyboard's spelling of the editor's keys, as its prompts spell
/// them ([`super::spell_editor_keys`]).
struct Spell<'a> {
    tr: &'a Tr,
    caps: &'a KeyCaps,
    keys: &'a EditorKeys,
    /// Whose labels the buttons are named by.
    family: PadFamily,
}

impl Spell<'_> {
    fn code(&self, key: KeyCode) -> String {
        // The prompts say "+/-" for the dial, whichever key carries the
        // sign (`dial_press`), "Esc" for Escape, and Space in their own
        // words.
        const SAID: [(KeyCode, &str); 3] = [
            (KeyCode::Equal, "+"),
            (KeyCode::Minus, "-"),
            (KeyCode::Escape, "Esc"),
        ];
        if key == KeyCode::Space {
            return self.tr.ed_space_key.to_string();
        }
        match SAID.iter().find(|(said, _)| *said == key) {
            Some((_, cap)) => cap.to_string(),
            None => self.caps.label(key),
        }
    }

    fn block(&self, group: [(KeyCode, Direction); 4]) -> String {
        match group == ARROWS {
            true => self.tr.ed_arrow_keys.to_string(),
            false => super::spell_block(self.caps, block_order(group)),
        }
    }

    fn moves(&self) -> String {
        self.keys.spell_moves(self.tr, self.caps)
    }

    /// The key a row of a pad table presses, as the keyboard's line for
    /// the same thing names it. `Direct` rows are read off the pad by the
    /// board's own systems, and their keys are the ones those systems read
    /// at a keyboard: P1's movement, place and remove keys, and the
    /// playtest's Enter, R and Esc (`playtest::editor_test_input`).
    fn key(&self, row: &Bind) -> Option<String> {
        Some(match row.key {
            Key::Code(key) => self.code(key),
            Key::Shifted(key) => shifted(self.caps, key),
            Key::Cap(letter) => letter.to_string(),
            Key::Wall(_) => self.block(self.keys.walls),
            Key::Command(command) => self.code(self.keys.command(command)),
            Key::ShiftedCommand(command) => shifted(self.caps, self.keys.command(command)),
            Key::Direct if row.does == Does::Move => self.moves(),
            Key::Direct if matches!(row.does, Does::Place(_)) => self.block(self.keys.place),
            Key::Direct if row.does == Does::Remove => self.code(self.keys.remove),
            Key::Direct if row.does == Does::Start => self.code(super::playtest::RUN_KEY),
            Key::Direct if row.does == Does::Reset => super::playtest::RETRY_CAP.to_string(),
            Key::Direct if row.does == Does::EndTest => self.code(super::playtest::END_KEY),
            Key::Direct | Key::Pick | Key::Step(_) | Key::Layer => return None,
        })
    }
}

/// A pad table's rows into `gather`, the Select layer's buttons named
/// with Select in front, as the prompt does.
fn gather_face(gather: &mut Gather, spell: &Spell, face: &Face, keep: impl Fn(Does) -> bool) {
    let tr = spell.tr;
    let layer = *face == (Face::Editor { layer: true });
    for row in scheme::binds(face) {
        if !keep(row.does) || row.key == Key::Layer {
            continue;
        }
        let way = match row.pad {
            Pad::Flick(dir) => Some(dir),
            Pad::A
            | Pad::B
            | Pad::X
            | Pad::Y
            | Pad::LB
            | Pad::RB
            | Pad::LT
            | Pad::RT
            | Pad::Start
            | Pad::Select
            | Pad::RS
            | Pad::Up
            | Pad::Down
            | Pad::Left
            | Pad::Right
            | Pad::Move => None,
        };
        let name = row.pad.name(tr, spell.family).to_string();
        let name = match layer {
            true => format!("{}+{name}", Pad::Select.name(tr, spell.family)),
            false => name,
        };
        gather.add(
            row.does.label(tr).to_string(),
            spell.key(&row),
            Some((name, way)),
        );
    }
}

/// Every key and button the editor answers to, in two columns of
/// sections: building on the bench in the first; "My levels", the
/// playtest and a custom stage's keys on the stage list in the second.
/// The buttons are named as a pad of `family` has them printed.
pub(super) fn sections(
    tr: &Tr,
    keys: &EditorKeys,
    caps: &KeyCaps,
    family: PadFamily,
) -> [Vec<Section>; 2] {
    let spell = Spell {
        tr,
        caps,
        keys,
        family,
    };
    let any = |_: Does| true;

    let mut bench = Gather::default();
    gather_face(&mut bench, &spell, &Face::Editor { layer: false }, any);
    gather_face(&mut bench, &spell, &Face::Editor { layer: true }, any);
    // The brushes' letters, which paint as they choose: those P1's
    // movement and wall keys have left them ([`EditorKeys::brush`]).
    let letters: Vec<String> = Brush::ALL
        .iter()
        .filter_map(|&b| keys.brush(b))
        .map(|key| caps.label(key))
        .collect();
    bench.add(
        tr.ed_help_brushes.to_string(),
        Some(letters.join(" ")),
        None,
    );
    // The card's rows with a letter of their own: the flock and the wrap.
    for row in card::ROWS {
        let key = row.key(LastDevice::Keyboard, keys, caps);
        if row != Row::Size && !key.is_empty() {
            bench.add(row.label(tr).to_string(), Some(key), None);
        }
    }

    let mut list = Gather::default();
    gather_face(&mut list, &spell, &Face::LevelList, any);

    let mut test = Gather::default();
    for phase in crate::app::editor::TestPhase::ALL {
        gather_face(&mut test, &spell, &Face::Playtest(phase), any);
    }

    let mut stages = Gather::default();
    gather_face(
        &mut stages,
        &spell,
        &Face::StageSelect { custom: true },
        |does| matches!(does, Does::Edit | Does::CopyCode),
    );

    let section = |title: &str, gather: Gather| Section {
        title: title.to_string(),
        lines: gather.lines(),
    };
    [
        vec![section(tr.ed_help_sections[0], bench)],
        vec![
            section(tr.ed_levels_title, list),
            section(tr.ed_help_sections[1], test),
            section(tr.ed_help_sections[2], stages),
        ],
    ]
}

/// The tips a first visit is shown, the key for every key filled in for
/// the hands last used.
pub(super) fn tips(tr: &Tr, device: LastDevice, caps: &KeyCaps) -> Vec<String> {
    let help = scheme::spelled(
        tr,
        device,
        &Face::Editor { layer: true },
        Does::Help,
        &shifted(caps, HELP_KEY),
    );
    tr.ed_tips
        .iter()
        .map(|tip| fill(tip, &[("help", &help), ("mirror", tr.ed_set_mirror)]))
        .collect()
}

/// Any key or button puts the card away, and a first visit's tips, once
/// put away, are not shown again: the settings remember it.
///
/// Read before the editor's own keys (`editor_input`), which sit out the
/// frame the card goes ([`Mode::JustClosed`]): the key that put it away
/// is the card's, not a brush's or a command's.
pub fn editor_card_input(
    keyboard: crate::app::keycaps::Keyboard,
    mut settings: ResMut<GameSettings>,
    mut state: ResMut<EditorState>,
) {
    let Some(card) = state.card() else {
        return;
    };
    if keyboard.keys.get_just_pressed().next().is_none() {
        return;
    }
    state.mode = Mode::JustClosed;
    if card == EditorCard::Tips && settings.editor_tips {
        settings.editor_tips = false;
        settings.save(&keyboard.caps);
    }
}

/// The card on screen, and what it was drawn for, so it is drawn again
/// when the card, the hands or the language change.
#[derive(Component)]
pub struct CardShown {
    card: EditorCard,
    device: LastDevice,
    lang: crate::app::i18n::Lang,
}

/// The card's width, and its columns'.
const CARD_W: f32 = 1240.0;
const CARD_PAD: f32 = 18.0;
const COLUMN_GAP: f32 = 28.0;
const COLUMN_W: f32 = (CARD_W - 2.0 * CARD_PAD - 4.0 - COLUMN_GAP) / 2.0;
/// The three cells of a line, and the gap between them.
const KEYS_W: f32 = 160.0;
const PAD_W: f32 = 220.0;
const CELL_GAP: f32 = 8.0;
const DOES_W: f32 = COLUMN_W - KEYS_W - PAD_W - 2.0 * CELL_GAP;
/// The tips card's width, and the room its lines have.
const TIPS_W: f32 = 640.0;

/// Put the card up while one is, and take it down once it is not.
pub fn show_editor_card(
    mut commands: Commands,
    state: Res<EditorState>,
    (settings, device, caps): (Res<GameSettings>, Res<LastDevice>, Res<KeyCaps>),
    (last_pad, pads): (Res<crate::app::gamepad::LastPad>, Query<&Gamepad>),
    shown: Query<(Entity, &CardShown)>,
) {
    let want = state.card();
    let lang = settings.language;
    for (entity, card) in &shown {
        if Some(card.card) != want || card.device != *device || card.lang != lang {
            commands.entity(entity).despawn();
        } else {
            return;
        }
    }
    let Some(card) = want else {
        return;
    };
    let tr = settings.tr();
    let keys = EditorKeys::of(&settings);
    let root = commands
        .spawn((
            super::EditorUi,
            CardShown {
                card,
                device: *device,
                lang,
            },
            GlobalZIndex(menu_ui::layer::CARD),
            menu_ui::centred_overlay(),
            // The bench dimmed behind the card, which is all words.
            BackgroundColor(Color::BLACK.with_alpha(0.35)),
        ))
        .id();
    let width = match card {
        EditorCard::Help => CARD_W,
        EditorCard::Tips => TIPS_W,
    };
    let panel = commands
        .spawn((
            Node {
                width: Val::Px(width),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                padding: UiRect::all(Val::Px(CARD_PAD)),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(12.0)),
                ..default()
            },
            BorderColor::all(crate::app::palette::CARD_EDGE),
            BackgroundColor(crate::app::palette::CARD_FILL),
            menu_ui::card_shadow(),
        ))
        .id();
    commands.entity(root).add_child(panel);
    let title = match card {
        EditorCard::Help => tr.ed_help_title,
        EditorCard::Tips => tr.ed_tips_title,
    };
    commands.entity(panel).with_children(|card_body| {
        card_body.spawn((
            Text::new(title),
            menu_ui::display_font(26.0),
            TextColor(crate::app::palette::GOLD),
            Node {
                margin: UiRect::bottom(Val::Px(6.0)),
                ..default()
            },
        ));
        match card {
            EditorCard::Help => {
                let family = crate::app::gamepad::reference_family(*device, *last_pad, &pads);
                spawn_help(card_body, tr, &keys, &caps, family);
            }
            EditorCard::Tips => {
                for tip in tips(tr, *device, &caps) {
                    card_body.spawn((
                        Text::new(tip),
                        TextFont {
                            font_size: FontSize::Px(menu_ui::type_scale::BODY),
                            ..default()
                        },
                        TextColor(crate::app::palette::PARCHMENT),
                        Node {
                            margin: UiRect::bottom(Val::Px(4.0)),
                            ..default()
                        },
                    ));
                }
            }
        }
    });
}

fn fine(text: String, color: Color, width: f32) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(menu_ui::type_scale::FINE),
            ..default()
        },
        TextLayout::no_wrap(),
        TextColor(color),
        Node {
            width: Val::Px(width),
            ..default()
        },
    )
}

/// The help card's two columns of sections, each line what is done, its
/// keys and its buttons, under a head naming the hands.
fn spawn_help(
    body: &mut ChildSpawnerCommands,
    tr: &Tr,
    keys: &EditorKeys,
    caps: &KeyCaps,
    family: PadFamily,
) {
    let columns = sections(tr, keys, caps, family);
    body.spawn(Node {
        column_gap: Val::Px(COLUMN_GAP),
        align_items: AlignItems::FlexStart,
        ..default()
    })
    .with_children(|row| {
        for column in columns {
            row.spawn(Node {
                width: Val::Px(COLUMN_W),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(1.0),
                ..default()
            })
            .with_children(|col| {
                let line = |col: &mut ChildSpawnerCommands, cells: [(String, Color); 3]| {
                    col.spawn(Node {
                        column_gap: Val::Px(CELL_GAP),
                        ..default()
                    })
                    .with_children(|line| {
                        let [(does, a), (keys, b), (pad, c)] = cells;
                        line.spawn(fine(does, a, DOES_W));
                        line.spawn(fine(keys, b, KEYS_W));
                        line.spawn(fine(pad, c, PAD_W));
                    });
                };
                let dim = crate::app::palette::PARCHMENT.with_alpha(0.55);
                line(
                    col,
                    [
                        (String::new(), dim),
                        (tr.ed_help_heads[0].to_string(), dim),
                        (tr.ed_help_heads[1].to_string(), dim),
                    ],
                );
                for section in column {
                    col.spawn((
                        Text::new(section.title),
                        TextFont {
                            font_size: FontSize::Px(menu_ui::type_scale::BODY),
                            ..default()
                        },
                        TextColor(crate::app::palette::GOLD),
                        Node {
                            margin: UiRect::top(Val::Px(6.0)),
                            ..default()
                        },
                    ));
                    for Line { keys, pad, does } in section.lines {
                        line(
                            col,
                            [
                                (does, crate::app::palette::PARCHMENT),
                                (keys, crate::app::palette::SELECTED_ROW),
                                (pad, crate::app::palette::IDLE_ROW),
                            ],
                        );
                    }
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::i18n::metrics::text_px;

    fn stock() -> (EditorKeys, KeyCaps) {
        (EditorKeys::of(&GameSettings::default()), KeyCaps::default())
    }

    fn all_lines(columns: &[Vec<Section>; 2]) -> Vec<Line> {
        columns
            .iter()
            .flatten()
            .flat_map(|section| section.lines.clone())
            .collect()
    }

    /// Every control on every face of the editor's has its line, in the
    /// prompt's words, with the key the bridge presses for it: a button
    /// added to a table is on the card without anyone writing it down.
    #[test]
    fn the_card_lists_every_control_of_the_editors_faces() {
        let tr = &crate::app::i18n::EN;
        let (keys, caps) = stock();
        let lines = all_lines(&sections(tr, &keys, &caps, PadFamily::Xbox));
        let mut faces = vec![
            Face::Editor { layer: false },
            Face::Editor { layer: true },
            Face::LevelList,
        ];
        faces.extend(crate::app::editor::TestPhase::ALL.map(Face::Playtest));
        for face in faces {
            for row in scheme::binds(&face) {
                if row.key == Key::Layer {
                    continue;
                }
                let words = row.does.label(tr);
                let line = lines
                    .iter()
                    .find(|line| line.does == words)
                    .unwrap_or_else(|| panic!("{face:?}: no line for {row:?}"));
                assert_ne!(line.keys, "-", "{face:?}: {row:?} has no key");
                assert!(
                    line.pad
                        .contains(row.pad.name(tr, crate::app::gamepad::PadFamily::Xbox)),
                    "{face:?}: {row:?} not on {line:?}"
                );
            }
        }
        // The keys only a keyboard has: redo, the smaller size, the row
        // back, the brushes and the card's own letters.
        let said = |words: &str| {
            lines
                .iter()
                .find(|line| line.does == words)
                .unwrap_or_else(|| panic!("no line for {words}"))
                .keys
                .clone()
        };
        assert_eq!(said(Does::Undo.label(tr)), "Z/Shift+Z");
        assert_eq!(said(Does::Size.label(tr)), "F5/Shift+F5");
        // And a pad's both ways too, on the stick under Select.
        let size = lines
            .iter()
            .find(|line| line.does == Does::Size.label(tr))
            .expect("a size line");
        assert_eq!(size.pad, "Select+right stick ↑↓");
        assert_eq!(said(Does::PrevSetting.label(tr)), "Shift+N/N");
        assert_eq!(said(Does::Help.label(tr)), "Shift+F1");
        assert_eq!(said(Does::LoadLevel.label(tr)), "F4");
        assert_eq!(said(tr.ed_help_brushes), "X R C H L E P B G");
        assert_eq!(said(tr.ed_set_gulls), "K");
        assert_eq!(said(tr.ed_set_wrap), "O");
        // The stage list's keys for a level of the player's own.
        let columns = sections(tr, &keys, &caps, PadFamily::Xbox);
        let stages: Vec<(String, String)> = columns[1][2]
            .lines
            .iter()
            .map(|line| (line.keys.clone(), line.pad.clone()))
            .collect();
        assert_eq!(
            stages,
            [("E".into(), "X".into()), ("F3".into(), "Select".into())]
        );
    }

    /// With P1 walking on the arrows, the walls and the playtest's
    /// signposts move to P1's place keys, and a command whose letter a
    /// place key takes moves to its function key: the card says so, from
    /// the same answer the keys and the prompt read.
    #[test]
    fn the_card_follows_p1s_bindings() {
        use crate::app::binds::Action;
        let tr = &crate::app::i18n::EN;
        let mut settings = GameSettings::default();
        for (action, key) in [
            (Action::MoveUp, KeyCode::ArrowUp),
            (Action::MoveDown, KeyCode::ArrowDown),
            (Action::MoveLeft, KeyCode::ArrowLeft),
            (Action::MoveRight, KeyCode::ArrowRight),
            (Action::PlaceUp, KeyCode::KeyW),
            (Action::PlaceDown, KeyCode::KeyK),
            (Action::PlaceLeft, KeyCode::KeyA),
            (Action::PlaceRight, KeyCode::KeyD),
            (Action::Remove, KeyCode::KeyX),
        ] {
            settings.binds[0].set(action, key);
        }
        let keys = EditorKeys::of(&settings);
        let caps = KeyCaps::default();
        let lines = all_lines(&sections(tr, &keys, &caps, PadFamily::Xbox));
        let said = |words: &str| {
            lines
                .iter()
                .find(|line| line.does == words)
                .map(|line| line.keys.clone())
                .unwrap_or_default()
        };
        assert_eq!(said(Does::Move.label(tr)), "arrow keys");
        assert_eq!(said(Does::Walls.label(tr)), "WAKD");
        assert_eq!(said(Does::Place(Direction::Up).label(tr)), "WAKD");
        assert_eq!(said(Does::Remove.label(tr)), "X");
        assert_eq!(said(tr.ed_set_gulls), "F7", "K is a wall now");
        assert_eq!(said(Does::Turn.label(tr)), "T");
    }

    /// The brushes' line names only the letters that load one: with P1
    /// moving up on E, E walks the cursor, and Weed is Tab's alone.
    #[test]
    fn the_brushes_line_leaves_out_a_taken_letter() {
        use crate::app::binds::Action;
        let tr = &crate::app::i18n::EN;
        let caps = KeyCaps::default();
        let brushes = |settings: &GameSettings| {
            let keys = EditorKeys::of(settings);
            all_lines(&sections(tr, &keys, &caps, PadFamily::Xbox))
                .into_iter()
                .find(|line| line.does == tr.ed_help_brushes)
                .map(|line| line.keys)
                .unwrap_or_default()
        };
        let mut settings = GameSettings::default();
        assert_eq!(brushes(&settings), "X R C H L E P B G");
        settings.binds[0].set(Action::MoveUp, KeyCode::KeyE);
        assert_eq!(brushes(&settings), "X R C H L P B G");
    }

    /// Every cell fits its column in every language, at the stock keys
    /// and at the longest the keys can be spelled; and every tip fits the
    /// tips card on one line, or two at the most.
    #[test]
    fn every_line_fits_its_cell_in_every_language() {
        let font = menu_ui::type_scale::FINE;
        let mut over = Vec::new();
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            for commit in [
                crate::app::settings::CommitScheme::default(),
                crate::app::settings::CommitScheme::Wasd,
                crate::app::settings::CommitScheme::Ijkl,
            ] {
                let settings = GameSettings {
                    commit,
                    ..GameSettings::default()
                };
                let keys = EditorKeys::of(&settings);
                let caps = KeyCaps::default();
                // Every maker's button names: "Share+R2" is a PlayStation
                // pad's.
                for family in PadFamily::ALL {
                    let columns = sections(tr, &keys, &caps, family);
                    for line in all_lines(&columns) {
                        let used = text_px(&line.pad, font);
                        if used > PAD_W {
                            over.push(format!("{lang:?}: {used:.0}px of {PAD_W:.0}: {}", line.pad));
                        }
                    }
                }
                let columns = sections(tr, &keys, &caps, PadFamily::Xbox);
                for line in all_lines(&columns) {
                    for (cell, room) in [
                        (&line.keys, KEYS_W),
                        (&line.pad, PAD_W),
                        (&line.does, DOES_W),
                    ] {
                        let used = text_px(cell, font);
                        if used > room {
                            over.push(format!("{lang:?}: {used:.0}px of {room:.0}: {cell}"));
                        }
                    }
                }
                let rows: usize = columns
                    .iter()
                    .map(|column| {
                        column
                            .iter()
                            .map(|section| section.lines.len() + 1)
                            .sum::<usize>()
                            + 1
                    })
                    .max()
                    .unwrap_or(0);
                assert!(rows <= 26, "{lang:?}: {rows} rows in a column");
            }
            for device in [LastDevice::Keyboard, LastDevice::PAD] {
                for tip in tips(tr, device, &KeyCaps::default()) {
                    let used = text_px(&tip, menu_ui::type_scale::BODY);
                    if used > 2.0 * (TIPS_W - 2.0 * CARD_PAD - 4.0) {
                        over.push(format!("{lang:?} tip: {used:.0}px: {tip}"));
                    }
                }
            }
            for heading in tr.ed_help_heads {
                if text_px(heading, font) > KEYS_W {
                    over.push(format!("{lang:?}: {heading}"));
                }
            }
        }
        assert!(over.is_empty(), "too wide:\n{}", over.join("\n"));
    }

    /// The editor's keys in their order in the frame, a card first.
    fn card_app() -> App {
        let mut app = super::super::tests::bench_app();
        let mut cursor = crate::app::cursor::Cursor::seated(0);
        (cursor.x, cursor.y) = (1, 1);
        app.world_mut().spawn((cursor, Transform::default()));
        app.add_systems(
            Update,
            (
                editor_card_input.run_if(super::super::editor_on_card),
                super::super::editor_input,
                super::super::editor_commands,
            )
                .chain(),
        );
        app
    }

    /// Shift+F1 puts the card of keys up, as the prompt and the card say;
    /// the next key puts it away and does nothing else, not even paint;
    /// and F1 alone still names the level.
    #[test]
    fn shift_f1_shows_every_key_and_any_key_puts_it_away() {
        let mut app = card_app();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::ShiftLeft);
            keys.press(HELP_KEY);
        }
        app.update();
        let card = |app: &App| app.world().resource::<EditorState>().card();
        assert_eq!(card(&app), Some(EditorCard::Help));
        super::super::tests::tap(&mut app, KeyCode::Space);
        assert_eq!(card(&app), None, "a key puts it away");
        assert_eq!(
            app.world().resource::<crate::app::Sim>().0.tile_at(1, 1),
            crate::sim::TileKind::Empty,
            "and paints nothing"
        );
        super::super::tests::tap(&mut app, KeyCode::F1);
        assert!(app.world().resource::<EditorState>().is_naming());
    }

    /// The tips are put away for good: once seen, the settings say so.
    #[test]
    fn the_tips_are_shown_until_put_away_once() {
        let mut app = card_app();
        app.world_mut().resource_mut::<EditorState>().mode = Mode::Card(EditorCard::Tips);
        assert!(app.world().resource::<GameSettings>().editor_tips);
        super::super::tests::tap(&mut app, KeyCode::Enter);
        assert!(!app.world().resource::<GameSettings>().editor_tips);
        let state = app.world().resource::<EditorState>();
        assert_eq!(state.card(), None);
        assert!(
            !state.is_testing(),
            "the key that put it away is the card's"
        );
    }
}
