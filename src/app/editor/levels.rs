//! F4: "My levels", the shelf on a card over the bench. A new level, blank
//! or from a starter ([`Template`]), a code off the clipboard, and every
//! level saved, each to open onto the bench, a saved one's code to copy
//! and a saved one to delete (asked twice).

use super::share::{paste_level, take_level};
use super::templates::Template;
use super::{
    BoardSwap, EDITOR_BOARD, EditorNews, EditorState, Mode, Offer, Workbench, ask_first,
    replace_board, unsaved_work,
};
use crate::app::gamepad::LastDevice;
use crate::app::gamepad::scheme::{Does, Face, button_name};
use crate::app::i18n::{Tr, fill};
use crate::app::menu_ui;
use crate::app::settings::GameSettings;
use crate::sim::{Board, Level, LevelKind};
use bevy::prelude::*;

/// One row of the list.
#[derive(Clone, Debug)]
pub enum Entry {
    /// A new level under a fresh name: fresh sand, or a starter.
    New(Template),
    /// The level whose code is on the clipboard.
    Paste,
    /// A level on the shelf, and the file it is in.
    Saved {
        level: Box<Level>,
        path: std::path::PathBuf,
    },
}

/// The list while it is up.
#[derive(Clone, Debug)]
pub struct LevelList {
    pub entries: Vec<Entry>,
    /// The row chosen.
    pub at: usize,
    /// A saved level's row whose delete was pressed once: the second
    /// press deletes it, anything else takes the question back.
    deleting: Option<usize>,
}

/// How many rows the card shows at once; the list scrolls past them.
pub(super) const ROWS: usize = 10;

impl LevelList {
    /// The list as the shelf now stands: a new level and the starters, a
    /// code, then every level saved, in the order the stage list and the
    /// map dial read them.
    pub(super) fn read() -> LevelList {
        let saved = crate::app::campaign::load_custom_levels()
            .into_iter()
            .filter_map(|level| {
                let path = crate::app::campaign::filed_path(&level)?;
                Some(Entry::Saved {
                    level: Box::new(level),
                    path,
                })
            });
        LevelList {
            entries: Template::ALL
                .map(Entry::New)
                .into_iter()
                .chain([Entry::Paste])
                .chain(saved)
                .collect(),
            at: 0,
            deleting: None,
        }
    }

    /// The first row the card shows, so the chosen one is always in view.
    pub(super) fn first_shown(&self) -> usize {
        self.at
            .saturating_sub(ROWS - 1)
            .min(self.entries.len().saturating_sub(ROWS))
    }
}

/// A row as the card says it: what it does, or a saved level's name and
/// what it is.
pub(super) fn row_text(tr: &Tr, entry: &Entry) -> String {
    match entry {
        Entry::New(template) => format!("+ {}", template.label(tr)),
        Entry::Paste => tr.ed_levels_paste.to_string(),
        Entry::Saved { level, .. } => {
            let kind = match level.kind {
                LevelKind::Puzzle => tr.ed_kind_puzzle,
                LevelKind::Arena => tr.ed_kind_arena,
            };
            format!("{}  [{kind}]", level.name)
        }
    }
}

/// The cap that deletes a saved level, as a replay's list deletes one.
const DELETE_CAP: char = 'D';

/// What a press does on the list, read once and acted on once.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Act {
    Open,
    Copy,
    Delete,
    Close,
}

/// The list's keys: W/S or the arrows choose, Enter opens (asked twice
/// over unsaved work, as every way of replacing the bench is), C copies a
/// saved level's code, D or Delete deletes one (asked twice), and Esc or
/// F4 again puts the list away. A pad presses the same keys through the
/// list's table. Leaving it, the commands sit the frame out
/// ([`Mode::JustClosed`]): its Enter or Esc would start a playtest or
/// leave the editor.
pub fn editor_levels_input(
    keyboard: crate::app::keycaps::Keyboard,
    (settings, device): (Res<GameSettings>, Res<LastDevice>),
    bench: Workbench,
    mut clipboard: ResMut<Clipboard>,
    news: EditorNews,
    mut swap: BoardSwap,
    mut ledger: (
        Res<crate::app::Campaign>,
        ResMut<crate::app::progress::Progress>,
    ),
) {
    let Workbench { mut sim, mut state } = bench;
    let EditorNews {
        mut shared,
        mut taken,
        ..
    } = news;
    let tr = settings.tr();
    let keys = &keyboard.keys;
    let Mode::Levels(list) = &mut state.mode else {
        return;
    };
    let act = if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::F4) {
        Some(Act::Close)
    } else if menu_ui::enter(keys) {
        Some(Act::Open)
    } else if keyboard.global('C') {
        Some(Act::Copy)
    } else if keyboard.just(DELETE_CAP) || keys.just_pressed(KeyCode::Delete) {
        Some(Act::Delete)
    } else {
        None
    };
    let at = menu_ui::nav(keys, list.at, list.entries.len());
    let moved = at != list.at;
    list.at = at;
    // A question stands until something else is said: moving, or another
    // of the list's keys. A frame with no key pressed says nothing, and
    // taking the question back on it made every second press a first.
    let other = |asked: Act| moved || act.is_some_and(|act| act != asked);
    if other(Act::Delete) {
        list.deleting = None;
    }
    let entry = list.entries.get(at).cloned();
    if other(Act::Open) {
        super::shelf::take_back_offer(&mut state);
    }
    if moved {
        state.feedback.clear();
    }
    let (Some(act), Some(entry)) = (act, entry) else {
        return;
    };
    match (act, entry) {
        (Act::Close, _) => {
            state.mode = Mode::JustClosed;
            state.feedback.clear();
        }
        (Act::Open, Entry::Paste) => {
            let pasted = crate::app::codes::paste(&mut clipboard);
            if paste_level(
                &mut state,
                &mut sim,
                pasted,
                (tr, *device),
                &mut taken,
                &mut swap,
            ) {
                state.mode = Mode::JustClosed;
            }
        }
        (Act::Open, entry @ (Entry::New(_) | Entry::Saved { .. })) => {
            let at_stake = unsaved_work(&state, &sim.0);
            if !ask_first(
                &mut state,
                Offer::Open,
                at_stake,
                Offer::Open.warning(tr, *device),
            ) {
                return;
            }
            match entry {
                Entry::Saved { level, path } => {
                    replace_board(level.board(), &mut sim, &mut state, &mut swap);
                    open_saved(&mut state, &level, path, &sim.0, tr);
                }
                Entry::New(template) => open_new(&mut state, &mut sim, template, tr, &mut swap),
                Entry::Paste => {}
            }
            state.mode = Mode::JustClosed;
        }
        (Act::Copy, Entry::Saved { level, .. }) => {
            let (said, took) = crate::app::codes::copy_counted(
                &mut clipboard,
                tr,
                crate::share::Kind::Level,
                level.to_text().as_bytes(),
                tr.code_copied,
            );
            state.feedback = said;
            if took {
                shared.write(crate::app::CodeShared);
            }
        }
        (Act::Delete, Entry::Saved { level, path }) => {
            let Mode::Levels(list) = &mut state.mode else {
                return;
            };
            if list.deleting != Some(at) {
                list.deleting = Some(at);
                let key = match (
                    *device,
                    button_name(&Face::LevelList, Does::Delete, tr, device.family()),
                ) {
                    (LastDevice::Pad(_), Some(button)) => button,
                    // The cap the press is read by, whichever key wears it.
                    _ => DELETE_CAP.to_string(),
                };
                state.feedback = fill(tr.replay_delete_confirm, &[("k", &key), ("r", &level.name)]);
                return;
            }
            state.feedback = match std::fs::remove_file(&path) {
                Ok(()) => {
                    forget_clear(&level, &ledger.0, &mut ledger.1);
                    fill(tr.ed_level_deleted, &[("name", &level.name)])
                }
                Err(e) => fill(
                    tr.replay_delete_failed,
                    &[("r", &level.name), ("e", &e.to_string())],
                ),
            };
            // The bench's own file gone, what it held is no longer on the
            // shelf: leaving asks again, and saving makes a new file, under
            // the level's name, not the gone file's (`shelf::save_target`).
            // Only the file the bench's level is filed in: another this
            // bench wrote (the level under a name it has since left) is
            // not the one the level on the bench is safe in.
            let filed_in = super::shelf::save_target(&state);
            let opened = state
                .filing
                .opened
                .as_ref()
                .is_some_and(|(file, _)| *file == path);
            if opened {
                state.filing.opened = None;
            }
            state.filing.mine.remove(&path);
            if path == filed_in {
                state.filing.filed = None;
            }
            let mut fresh = LevelList::read();
            fresh.at = at.min(fresh.entries.len() - 1);
            state.mode = Mode::Levels(Box::new(fresh));
        }
        (Act::Copy | Act::Delete, Entry::New(_) | Entry::Paste) => {}
    }
}

/// Strike a deleted level's clear: the tick filed under its file, which
/// the next level saved under its name would take over, and the one an
/// older build filed under the name the stage list shows it by, which the
/// next level alone under that name would adopt (see
/// [`crate::app::progress::Progress::adopt_legacy`]). That name is never a
/// shipped stage's: the list renames a level of the player's that would
/// read as one.
fn forget_clear(
    level: &Level,
    campaign: &crate::app::Campaign,
    progress: &mut crate::app::progress::Progress,
) {
    let Some(key) = level.filing.as_ref().map(|filing| filing.key.as_str()) else {
        return;
    };
    let legacy = campaign
        .levels
        .iter()
        .skip(campaign.builtins)
        .filter_map(|level| level.filing.as_ref())
        .find(|filing| filing.key == key)
        .and_then(|filing| filing.legacy.as_deref());
    let keys: Vec<&str> = std::iter::once(key).chain(legacy).collect();
    crate::app::progress::forget(progress, &keys);
}

/// A new level onto the bench, under a name no saved level has. Blank, it
/// is fresh sand of the bench's own kind with the mirror off, as on any
/// level brought onto the bench; from a starter, the starter's level,
/// kind and all, with the mirror it was drawn with on, so the author's
/// next edits keep it fair.
fn open_new(
    state: &mut EditorState,
    sim: &mut crate::app::Sim,
    template: Template,
    tr: &Tr,
    swap: &mut BoardSwap,
) {
    let level = template.level();
    let board = match &level {
        Some(level) => level.board(),
        None => Board::new(EDITOR_BOARD.0, EDITOR_BOARD.1, 0xED17),
    };
    replace_board(board, sim, state, swap);
    state.start_afresh(tr, &sim.0);
    state.feedback = match level {
        Some(level) => {
            let name = super::shelf::fresh_name(tr);
            take_level(state, &level, &sim.0, tr);
            state.name = name;
            // Untouched, a starter is nobody's work: another can take its
            // place without asking.
            state.filing.filed = Some(super::filed_text(state, &sim.0));
            fill(tr.ed_opened, &[("name", &template.label(tr))])
        }
        None => tr.ed_fresh_sand.into(),
    };
    state.symmetry = template.symmetry();
}

/// A saved level onto the bench: everything [`take_level`] takes, and the
/// file it came from is the bench's own, so F2 saves the level back where
/// it was found without asking, and Esc leaves at once while it is as it
/// was saved.
pub(super) fn open_saved(
    state: &mut EditorState,
    level: &Level,
    path: std::path::PathBuf,
    board: &Board,
    tr: &Tr,
) {
    take_level(state, level, board, tr);
    state.filing.mine.insert(path.clone());
    state.filing.opened = Some((path, state.name.clone()));
    state.filing.filed = Some(super::filed_text(state, board));
    state.feedback = fill(tr.ed_opened, &[("name", &state.name)]);
}

/// The list's card.
#[derive(Component)]
pub struct LevelsCard;

/// One of the card's rows, by its place among those shown.
#[derive(Component)]
pub struct LevelsRow(usize);

/// The card's width.
const CARD_W: f32 = 580.0;

/// The room a row has for its text.
#[cfg(test)]
const ROW_ROOM: f32 = CARD_W - 2.0 * 2.0 - 2.0 * 18.0 - 2.0 * 10.0;

/// Put the card up while the list is, and take it down once it is not;
/// write its rows from the list each frame.
pub fn show_level_list(
    mut commands: Commands,
    state: Res<EditorState>,
    settings: Res<GameSettings>,
    cards: Query<Entity, With<LevelsCard>>,
    mut rows: Query<(&LevelsRow, &mut Text, &mut TextColor, &mut Node)>,
    mut fills: Query<(&LevelsRow, &mut BackgroundColor, &mut Node), Without<Text>>,
) {
    let tr = settings.tr();
    let Mode::Levels(list) = &state.mode else {
        for card in &cards {
            commands.entity(card).despawn();
        }
        return;
    };
    if cards.is_empty() {
        spawn_card(&mut commands, tr);
        return;
    }
    let first = list.first_shown();
    for (row, mut text, mut color, mut node) in &mut rows {
        let index = first + row.0;
        let entry = list.entries.get(index);
        menu_ui::set_shown(&mut node, entry.is_some());
        let said = entry.map(|entry| row_text(tr, entry)).unwrap_or_default();
        menu_ui::set_text(&mut text, &said);
        let ink = match index == list.at {
            true => crate::app::palette::SELECTED_ROW,
            false => crate::app::palette::PARCHMENT,
        };
        menu_ui::set_color(&mut color, ink);
    }
    for (row, mut fill, mut node) in &mut fills {
        menu_ui::set_shown(&mut node, first + row.0 < list.entries.len());
        let lit = match first + row.0 == list.at {
            true => crate::app::palette::SELECTED_ROW.with_alpha(0.22),
            false => Color::NONE,
        };
        menu_ui::set_bg(&mut fill, lit);
    }
}

fn spawn_card(commands: &mut Commands, tr: &Tr) {
    commands
        .spawn((
            super::EditorUi,
            LevelsCard,
            GlobalZIndex(menu_ui::layer::CARD),
            menu_ui::centred_overlay(),
        ))
        .with_children(|wrap| {
            wrap.spawn((
                Node {
                    width: Val::Px(CARD_W),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    padding: UiRect::all(Val::Px(18.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(12.0)),
                    ..default()
                },
                BorderColor::all(crate::app::palette::CARD_EDGE),
                BackgroundColor(crate::app::palette::CARD_FILL),
                menu_ui::card_shadow(),
            ))
            .with_children(|card| {
                card.spawn((
                    Text::new(tr.ed_levels_title),
                    menu_ui::display_font(26.0),
                    TextColor(crate::app::palette::GOLD),
                    Node {
                        margin: UiRect::bottom(Val::Px(8.0)),
                        ..default()
                    },
                ));
                for slot in 0..ROWS {
                    card.spawn((
                        LevelsRow(slot),
                        Node {
                            padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
                            border_radius: BorderRadius::all(Val::Px(6.0)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .with_children(|row| {
                        row.spawn((
                            LevelsRow(slot),
                            Text::new(String::new()),
                            TextFont {
                                font_size: FontSize::Px(menu_ui::type_scale::BODY),
                                ..default()
                            },
                            TextLayout::no_wrap(),
                            TextColor(crate::app::palette::PARCHMENT),
                            Node::default(),
                        ));
                    });
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::super::tests::{bench_app, tap};
    use super::super::{editor_commands, editor_input};
    use super::*;
    use crate::app::Sim;
    use crate::sim::{CrabKind, Direction, Handedness, TileKind};

    /// Every row fits the card in every language: the longest name a
    /// level may have, with the longer of the two kinds after it.
    #[test]
    fn every_row_fits_the_card_in_every_language() {
        use crate::app::i18n::metrics::text_px;
        let name = "W".repeat(super::super::naming::NAME_MAX);
        let mut over = Vec::new();
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            for kind in [LevelKind::Puzzle, LevelKind::Arena] {
                let level = Level::from_board(name.clone(), 3, Board::new(4, 4, 1)).with_kind(kind);
                let saved = Entry::Saved {
                    level: Box::new(level),
                    path: std::path::PathBuf::new(),
                };
                let news = Template::ALL.map(Entry::New);
                for entry in news.into_iter().chain([Entry::Paste, saved]) {
                    let line = row_text(tr, &entry);
                    let used = text_px(&line, menu_ui::type_scale::BODY);
                    if used > ROW_ROOM {
                        over.push(format!("{lang:?}: {used:.0}px {line}"));
                    }
                }
            }
            let title = crate::app::i18n::metrics::display_px(tr.ed_levels_title, 26.0);
            if title > ROW_ROOM {
                over.push(format!("{lang:?}: {title:.0}px {}", tr.ed_levels_title));
            }
        }
        assert!(
            over.is_empty(),
            "past {ROW_ROOM:.0}px:\n{}",
            over.join("\n")
        );
    }

    /// The editor with the list's system in its place in the chain.
    fn list_app() -> App {
        let mut app = bench_app();
        let mut cursor = crate::app::cursor::Cursor::seated(0);
        (cursor.x, cursor.y) = (1, 1);
        app.world_mut().spawn((cursor, Transform::default()));
        let has_keys = |state: Res<EditorState>| !state.has_keys();
        app.add_systems(
            Update,
            (
                editor_input,
                editor_levels_input.run_if(super::super::editor_on_levels),
                editor_commands.run_if(has_keys),
            )
                .chain(),
        );
        app
    }

    fn list(app: &App) -> Option<LevelList> {
        match &app.world().resource::<EditorState>().mode {
            Mode::Levels(list) => Some((**list).clone()),
            Mode::Painting
            | Mode::Naming { .. }
            | Mode::JustClosed
            | Mode::Testing(_)
            | Mode::Card(_) => None,
        }
    }

    /// Put the list's choice on the level filed at `path`, as the arrows
    /// would: other tests share the shelf and may file levels ahead of
    /// it between one press and the next.
    fn choose(app: &mut App, path: &std::path::Path) {
        let mut state = app.world_mut().resource_mut::<EditorState>();
        let Mode::Levels(list) = &mut state.mode else {
            panic!("the list is not up");
        };
        list.at = list
            .entries
            .iter()
            .position(|entry| matches!(entry, Entry::Saved { path: p, .. } if p == path))
            .expect("the saved level is listed");
    }

    /// Deleting another file this bench wrote leaves the level on the
    /// bench saved: it is filed in the file its name makes, and Esc asked
    /// about it as if it would be lost. Deleting that file does ask.
    #[test]
    fn deleting_another_file_the_bench_wrote_keeps_it_saved() {
        let mut app = list_app();
        let (first, second) = ("Delete Other Probe A", "Delete Other Probe B");
        let paths = [first, second].map(super::super::shelf::save_path);
        for path in &paths {
            let _ = std::fs::remove_file(path);
        }
        {
            let board = &mut app.world_mut().resource_mut::<Sim>().0;
            board.set_tile(3, 1, TileKind::Castle(0));
            board.spawn_crab(1, 1, Direction::Right, Handedness::Left, CrabKind::Common);
        }
        app.world_mut().resource_mut::<EditorState>().name = first.into();
        tap(&mut app, KeyCode::F2);
        app.world_mut().resource_mut::<EditorState>().name = second.into();
        tap(&mut app, KeyCode::F2);
        let unsaved = |app: &App| {
            let world = app.world();
            super::super::unsaved_work(world.resource::<EditorState>(), &world.resource::<Sim>().0)
        };
        assert!(paths.iter().all(|path| path.exists()));
        assert!(!unsaved(&app), "saved");
        tap(&mut app, KeyCode::F4);
        choose(&mut app, &paths[0]);
        tap(&mut app, KeyCode::KeyD);
        tap(&mut app, KeyCode::KeyD);
        assert!(!paths[0].exists(), "deleted");
        assert!(!unsaved(&app), "the level on the bench is still filed");
        choose(&mut app, &paths[1]);
        tap(&mut app, KeyCode::KeyD);
        tap(&mut app, KeyCode::KeyD);
        assert!(!paths[1].exists(), "deleted");
        assert!(unsaved(&app), "its own file gone, it is not");
    }

    /// F4 puts the list up and Esc takes it down, the editor staying, and
    /// a level saved from the bench is on it, to be opened onto a fresh
    /// bench as its own file and deleted with a second press.
    #[test]
    fn the_list_opens_a_saved_level_and_deletes_one_asked_twice() {
        let mut app = list_app();
        let name = "List Probe";
        let path = super::super::shelf::save_path(name);
        let _ = std::fs::remove_file(&path);
        app.world_mut().resource_mut::<EditorState>().name = name.into();
        app.world_mut()
            .resource_mut::<crate::app::Sim>()
            .0
            .set_wrap(true);
        tap(&mut app, KeyCode::F2);
        tap(&mut app, KeyCode::F4);
        let shown = list(&app).expect("the list is up");
        assert!(matches!(
            shown.entries[..5],
            [
                Entry::New(Template::Blank),
                Entry::New(Template::Puzzle),
                Entry::New(Template::Beach(2)),
                Entry::New(Template::Beach(4)),
                Entry::Paste
            ]
        ));
        tap(&mut app, KeyCode::ArrowDown);
        assert_eq!(list(&app).expect("up").at, 1, "the arrows choose");
        tap(&mut app, KeyCode::Escape);
        assert!(list(&app).is_none(), "Esc puts it away");
        app.update();
        let state = app.world().resource::<EditorState>();
        assert!(matches!(state.mode, Mode::Painting));
        assert_eq!(state.offer, None, "and does not also ask to leave");

        // A new level: the bench was saved, so it goes at once.
        tap(&mut app, KeyCode::F4);
        tap(&mut app, KeyCode::Enter);
        assert!(list(&app).is_none());
        assert!(
            !app.world().resource::<crate::app::Sim>().0.wrap(),
            "fresh sand"
        );
        assert_ne!(app.world().resource::<EditorState>().name, name);
        assert!(!app.world().resource::<EditorState>().is_testing());

        // Opened, the level is the bench's own file again.
        tap(&mut app, KeyCode::F4);
        choose(&mut app, &path);
        tap(&mut app, KeyCode::Enter);
        let state = app.world().resource::<EditorState>();
        assert_eq!(state.name, name);
        assert!(state.filing.mine.contains(&path), "its file is the bench's");
        assert!(app.world().resource::<crate::app::Sim>().0.wrap());

        // Over unsaved work, opening asks first and the second Enter opens.
        app.world_mut()
            .resource_mut::<crate::app::Sim>()
            .0
            .set_tile(3, 3, crate::sim::TileKind::Rock);
        tap(&mut app, KeyCode::F4);
        choose(&mut app, &path);
        tap(&mut app, KeyCode::Enter);
        assert!(list(&app).is_some(), "the first Enter asks");
        tap(&mut app, KeyCode::Enter);
        assert!(list(&app).is_none(), "the second opens");
        let board = &app.world().resource::<crate::app::Sim>().0;
        assert_eq!(board.tile_at(3, 3), crate::sim::TileKind::Empty);

        // Deleted with a second press, and only then, and its clear with
        // it: a later level saved under its name takes its file name.
        let key = format!(
            "file:custom/{}",
            path.file_name().expect("a file").to_string_lossy()
        );
        app.world_mut()
            .resource_mut::<crate::app::progress::Progress>()
            .mark(&key);
        tap(&mut app, KeyCode::F4);
        choose(&mut app, &path);
        tap(&mut app, KeyCode::KeyD);
        assert!(path.exists(), "the first press asks");
        tap(&mut app, KeyCode::KeyD);
        assert!(!path.exists(), "the second deletes");
        let state = app.world().resource::<EditorState>();
        assert!(!state.filing.mine.contains(&path));
        assert!(state.filing.filed.is_none(), "the bench is unsaved again");
        let progress = app.world().resource::<crate::app::progress::Progress>();
        assert!(!progress.is_cleared(&key), "its clear goes with it");
    }

    /// A level deleted while the check its save started is running stays
    /// deleted: the check's answer no longer writes its way through into
    /// the file, which brought the level back.
    #[test]
    fn a_save_check_does_not_bring_a_deleted_level_back() {
        use super::super::validate::{Validation, Verdict, poll_solver};
        let mut app = list_app();
        app.add_systems(Update, poll_solver);
        let name = "Deleted Probe";
        let path = super::super::shelf::save_path(name);
        crate::app::paths::write_atomic(
            &path,
            Level::from_board(name, 1, Board::new(4, 4, 1)).to_text(),
        )
        .expect("filed");
        {
            let mut state = app.world_mut().resource_mut::<EditorState>();
            state.name = name.into();
            state.filing.mine.insert(path.clone());
            let mut check = Validation::pending();
            check.join(path.clone(), name.into());
            state.check.solver = Some(check);
        }
        tap(&mut app, KeyCode::F4);
        choose(&mut app, &path);
        tap(&mut app, KeyCode::KeyD);
        tap(&mut app, KeyCode::KeyD);
        assert!(!path.exists(), "deleted");
        {
            let state = app.world().resource::<EditorState>();
            let check = state.check.solver.as_ref().expect("the check runs on");
            *check.slot.lock().unwrap() = Some(Verdict {
                outcome: crate::sim::SolveOutcome::Found(Vec::new()),
                needs: None,
                shared: None,
                coop: false,
            });
        }
        app.update();
        assert!(app.world().resource::<EditorState>().check.solver.is_none());
        assert!(!path.exists(), "and stays deleted");
    }

    /// A level opened from a file its name would not make (an older
    /// build's, or one named by hand) is saved back into that file, not
    /// copied into a second one beside it; renamed, it is a level of its
    /// own under its new name.
    #[test]
    fn a_level_opened_from_its_file_saves_back_into_it() {
        let mut app = list_app();
        let name = "Hand Named Probe";
        let path = custom_dir_file("hand-made-by-hand-probe.txt");
        let own = super::super::shelf::save_path(name);
        let _ = std::fs::remove_file(&own);
        crate::app::paths::write_atomic(
            &path,
            Level::from_board(name, 1, Board::new(4, 4, 1)).to_text(),
        )
        .expect("filed");
        let level = Level::parse(&std::fs::read_to_string(&path).expect("reads")).expect("parses");
        {
            let world = app.world_mut();
            world.resource_mut::<crate::app::Sim>().0 = level.board();
            let board = world.resource::<crate::app::Sim>().0.clone();
            let mut state = world.resource_mut::<EditorState>();
            open_saved(
                &mut state,
                &level,
                path.clone(),
                &board,
                &crate::app::i18n::EN,
            );
        }
        app.world_mut()
            .resource_mut::<crate::app::Sim>()
            .0
            .set_wrap(true);
        tap(&mut app, KeyCode::F2);
        assert!(!own.exists(), "no second copy under the name");
        assert!(
            std::fs::read_to_string(&path)
                .expect("there")
                .contains("wrap"),
            "saved back where it was found"
        );
        app.world_mut().resource_mut::<EditorState>().name = "Hand Named Probe Two".into();
        let renamed = super::super::shelf::save_path("Hand Named Probe Two");
        let _ = std::fs::remove_file(&renamed);
        tap(&mut app, KeyCode::F2);
        assert!(renamed.exists(), "renamed, a level of its own");
        for file in [&path, &own, &renamed] {
            let _ = std::fs::remove_file(file);
        }
    }

    /// The bench's own hand-named file deleted from the list stays
    /// deleted: the next F2 files the level under its name, and does not
    /// write the gone file back at its old path.
    #[test]
    fn a_deleted_hand_named_file_is_not_saved_back() {
        let mut app = list_app();
        let name = "Deleted Hand Probe";
        let path = custom_dir_file("deleted-by-hand-probe.txt");
        let own = super::super::shelf::save_path(name);
        let _ = std::fs::remove_file(&own);
        crate::app::paths::write_atomic(
            &path,
            Level::from_board(name, 1, Board::new(4, 4, 1)).to_text(),
        )
        .expect("filed");
        let level = Level::parse(&std::fs::read_to_string(&path).expect("reads")).expect("parses");
        {
            let world = app.world_mut();
            world.resource_mut::<crate::app::Sim>().0 = level.board();
            let board = world.resource::<crate::app::Sim>().0.clone();
            let mut state = world.resource_mut::<EditorState>();
            open_saved(
                &mut state,
                &level,
                path.clone(),
                &board,
                &crate::app::i18n::EN,
            );
        }
        tap(&mut app, KeyCode::F4);
        choose(&mut app, &path);
        tap(&mut app, KeyCode::KeyD);
        tap(&mut app, KeyCode::KeyD);
        assert!(!path.exists(), "deleted");
        tap(&mut app, KeyCode::Escape);
        app.update();
        tap(&mut app, KeyCode::F2);
        let back = path.exists();
        let filed = own.exists();
        for file in [&path, &own] {
            let _ = std::fs::remove_file(file);
        }
        assert!(!back, "not written back where it was deleted");
        assert!(filed, "filed under its name");
    }

    fn custom_dir_file(file: &str) -> std::path::PathBuf {
        super::super::shelf::custom_dir().join(file)
    }

    /// A starter opens as its own level under a fresh name, kind and all,
    /// with the mirror it was drawn with still painting.
    #[test]
    fn a_starter_opens_with_its_mirror_on() {
        let mut app = list_app();
        tap(&mut app, KeyCode::F4);
        {
            let mut state = app.world_mut().resource_mut::<EditorState>();
            let Mode::Levels(list) = &mut state.mode else {
                panic!("the list is not up");
            };
            list.at = list
                .entries
                .iter()
                .position(|entry| matches!(entry, Entry::New(Template::Beach(2))))
                .expect("listed");
        }
        tap(&mut app, KeyCode::Enter);
        let state = app.world().resource::<EditorState>();
        assert_eq!(state.kind, LevelKind::Arena);
        assert_eq!(state.symmetry, super::super::Symmetry::LeftRight);
        assert_eq!(
            state.name,
            super::super::shelf::fresh_name(&crate::app::i18n::EN)
        );
        let board = &app.world().resource::<crate::app::Sim>().0;
        assert_eq!(board.table_seats(), Some(2));

        // Untouched, the starter is no work to lose: a blank level takes
        // its place at the first Enter, and with the mirror off, as every
        // level brought onto the bench starts.
        tap(&mut app, KeyCode::F4);
        tap(&mut app, KeyCode::Enter);
        assert!(list(&app).is_none(), "opened at once");
        let state = app.world().resource::<EditorState>();
        assert_eq!(state.symmetry, super::super::Symmetry::Off);
        assert_eq!(
            app.world().resource::<crate::app::Sim>().0.table_seats(),
            Some(0)
        );
    }

    /// A level opened onto the bench starts with the mirror off, whatever
    /// the level before it was drawn with.
    #[test]
    fn an_opened_level_starts_with_the_mirror_off() {
        let mut state = EditorState {
            symmetry: super::super::Symmetry::Both,
            ..EditorState::default()
        };
        let level = Level::from_board("Plain", 1, Board::new(6, 5, 1));
        take_level(&mut state, &level, &level.board(), &crate::app::i18n::EN);
        assert_eq!(state.symmetry, super::super::Symmetry::Off);
    }
}
