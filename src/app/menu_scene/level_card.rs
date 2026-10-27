//! A level code pasted on the menu: V takes a round's code into the
//! round, and a level's onto a card that asks what to do with it. Play
//! it (a stage on its own, a beach against the AI in the editor's
//! playtest), open it in the editor, or save it to the shelf.

use crate::app::gamepad::LastDevice;
use crate::app::gamepad::scheme::{Does, Face, button_name, legend};
use crate::app::i18n::{Tr, fill};
use crate::app::menu_ui;
use crate::app::palette;
use crate::app::settings::GameSettings;
use crate::app::{Campaign, Screen};
use crate::sim::{Level, LevelKind};
use bevy::prelude::*;

/// The card, while a pasted level is on it.
#[derive(Resource, Default)]
pub struct LevelCard {
    pub level: Option<Level>,
    /// S pressed once over a file holding a level already: the second
    /// press replaces it.
    replacing: bool,
}

impl LevelCard {
    pub fn is_open(&self) -> bool {
        self.level.is_some()
    }

    /// The card up with `level` on it.
    pub fn show(&mut self, level: Level) {
        *self = LevelCard {
            level: Some(level),
            replacing: false,
        };
    }
}

/// Where the card's choices take the player.
#[derive(bevy::ecs::system::SystemParam)]
pub struct CardDoors<'w> {
    campaign: ResMut<'w, Campaign>,
    opening: ResMut<'w, crate::app::editor::EditorOpening>,
    notice: ResMut<'w, crate::app::RoundNotice>,
    next_screen: ResMut<'w, NextState<Screen>>,
    /// Two players at the stage, which a pasted co-op stage seats.
    coop: ResMut<'w, crate::app::Coop>,
    /// A code taken in, for the trophy (`editor::taken_from_elsewhere`).
    taken: MessageWriter<'w, crate::app::CodeTaken>,
}

/// What a press on the card asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Choice {
    Play,
    Edit,
    Save,
    Close,
}

/// The card's keys: Enter plays, E edits, S saves (asked twice over a
/// level of the same name), Esc puts it away. A pad presses the same keys
/// through the card's table. Runs after the menu's own input, which gives
/// way while the card is up, so the Esc that closes the card does not also
/// leave the game.
pub fn answer_level_card(
    keyboard: crate::app::keycaps::Keyboard,
    (settings, device): (Res<GameSettings>, Res<LastDevice>),
    mut card: ResMut<LevelCard>,
    doors: CardDoors,
) {
    let CardDoors {
        mut campaign,
        mut opening,
        mut notice,
        mut next_screen,
        mut coop,
        mut taken,
    } = doors;
    let tr = settings.tr();
    let keys = &keyboard.keys;
    let Some(level) = card.level.clone() else {
        return;
    };
    let choice = if keys.just_pressed(KeyCode::Escape) {
        Choice::Close
    } else if menu_ui::enter(keys) {
        Choice::Play
    } else if keyboard.just('E') {
        Choice::Edit
    } else if keyboard.just('S') {
        Choice::Save
    } else {
        return;
    };
    let replacing = std::mem::take(&mut card.replacing);
    // The code is taken up by a play, an edit or a save that lands, as
    // a paste onto the editor's bench is: once, and only somebody else's.
    let took = |taken: &mut MessageWriter<crate::app::CodeTaken>| {
        if crate::app::editor::taken_from_elsewhere(&level) {
            taken.write(crate::app::CodeTaken);
        }
    };
    match choice {
        Choice::Close => {
            card.level = None;
            notice.0.clear();
        }
        Choice::Play => {
            card.level = None;
            notice.0.clear();
            took(&mut taken);
            match level.kind {
                // A stage of its own, after the Tide Pool's stages and the
                // player's: the stage list's door, without being filed.
                LevelKind::Puzzle => {
                    let mut list = crate::app::campaign::stage_list();
                    list.levels.push(level);
                    crate::app::campaign::disambiguate(&mut list.levels, list.builtins);
                    let last = list.levels.len() - 1;
                    campaign.reset(list);
                    campaign.index = last;
                    // A co-op stage is played by two: Enter on its card
                    // seats the second cursor, as C on the stage list does.
                    coop.0 |= campaign.current().is_coop();
                    next_screen.set(Screen::Puzzle);
                }
                // A beach is a match, and the editor's playtest is the
                // one that seats the AI round a beach nobody filed.
                LevelKind::Arena => {
                    opening.0 = Some(crate::app::editor::Opening {
                        level,
                        path: None,
                        test: true,
                    });
                    next_screen.set(Screen::Editor);
                }
            }
        }
        Choice::Edit => {
            card.level = None;
            notice.0.clear();
            took(&mut taken);
            opening.0 = Some(crate::app::editor::Opening {
                level,
                path: None,
                test: false,
            });
            next_screen.set(Screen::Editor);
        }
        // Asked before the save, which puts it on the shelf.
        Choice::Save => match (
            crate::app::editor::taken_from_elsewhere(&level),
            crate::app::editor::shelve(&level, replacing, tr),
        ) {
            (elsewhere, Ok(crate::app::editor::Shelved::Saved(file))) => {
                if elsewhere {
                    taken.write(crate::app::CodeTaken);
                }
                card.level = None;
                notice.0 = fill(tr.ed_saved_to, &[("file", &file)]);
            }
            (_, Ok(crate::app::editor::Shelved::Taken(name))) => {
                card.replacing = true;
                notice.0 = taken_line(tr, *device, &name);
            }
            (_, Err(said)) => notice.0 = said,
        },
    }
}

/// What S says over a file that holds a level already, naming the key that
/// replaces it: S, or the pad's button for it.
fn taken_line(tr: &Tr, device: LastDevice, name: &str) -> String {
    match (device, button_name(&Face::LevelCard, Does::Save, tr)) {
        (LastDevice::Pad, Some(button)) => {
            fill(tr.pad.level_card_taken, &[("name", name), ("b", &button)])
        }
        _ => fill(tr.level_card_taken, &[("name", name)]),
    }
}

/// The card on screen.
#[derive(Component)]
pub struct LevelCardUi;

/// The card's line of keys, which follows the hands last used.
#[derive(Component)]
pub struct LevelCardKeys;

/// The card's keys as its line says them.
pub fn level_card_keys(tr: &Tr, device: LastDevice) -> String {
    match device {
        LastDevice::Keyboard => tr.level_card_keys.to_string(),
        LastDevice::Pad => legend(tr, &Face::LevelCard),
    }
}

/// Draw the card while it is up, and take it down once it is not.
pub fn show_level_card(
    mut commands: Commands,
    card: Res<LevelCard>,
    (settings, device): (Res<GameSettings>, Res<LastDevice>),
    drawn: Query<Entity, With<LevelCardUi>>,
    mut keys: Query<&mut Text, With<LevelCardKeys>>,
) {
    let tr = settings.tr();
    if card.is_open() == !drawn.is_empty() {
        for mut text in &mut keys {
            menu_ui::set_text(&mut text, &level_card_keys(tr, *device));
        }
        return;
    }
    for entity in &drawn {
        commands.entity(entity).despawn();
    }
    let Some(level) = &card.level else {
        return;
    };
    let kind = match level.kind {
        LevelKind::Puzzle => tr.ed_kind_puzzle,
        LevelKind::Arena => tr.ed_kind_arena,
    };
    commands
        .spawn((
            LevelCardUi,
            GlobalZIndex(menu_ui::layer::CARD),
            menu_ui::centred_overlay(),
        ))
        .with_children(|wrap| {
            wrap.spawn(menu_ui::screen_card()).with_children(|card| {
                card.spawn((
                    Text::new(level.name.clone()),
                    menu_ui::display_font(30.0),
                    TextColor(palette::GOLD),
                ));
                card.spawn((
                    Text::new(format!("[{kind}]")),
                    menu_ui::display_font(menu_ui::type_scale::ROW),
                    TextColor(palette::PARCHMENT.with_alpha(0.7)),
                ));
                card.spawn(menu_ui::spacer(10.0));
                card.spawn((
                    LevelCardKeys,
                    Text::new(level_card_keys(tr, *device)),
                    menu_ui::display_font(menu_ui::type_scale::ROW),
                    TextColor(palette::PARCHMENT),
                ));
            });
        });
}

/// Leaving the menu puts the card away.
pub fn close_level_card(
    mut card: ResMut<LevelCard>,
    drawn: Query<Entity, With<LevelCardUi>>,
    mut commands: Commands,
) {
    *card = LevelCard::default();
    for entity in &drawn {
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card_app(level: Level) -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.insert_state(Screen::Menu);
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<GameSettings>();
        app.init_resource::<LastDevice>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<crate::app::cursor::LiveSeatKeys>();
        app.init_resource::<crate::app::editor::EditorOpening>();
        app.init_resource::<crate::app::RoundNotice>();
        app.init_resource::<crate::app::Coop>();
        app.add_message::<crate::app::CodeTaken>();
        app.insert_resource(Campaign::tide_pool());
        let mut card = LevelCard::default();
        card.show(level);
        app.insert_resource(card);
        app.add_systems(Update, answer_level_card);
        app
    }

    fn press(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(key);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
    }

    fn stage(name: &str) -> Level {
        let mut board = crate::sim::Board::new(6, 5, 2);
        board.set_tile(4, 2, crate::sim::TileKind::Castle(0));
        board.spawn_crab(
            1,
            2,
            crate::sim::Direction::Right,
            crate::sim::Handedness::Left,
            crate::sim::CrabKind::Common,
        );
        Level::from_board(name, 1, board).with_kind(LevelKind::Puzzle)
    }

    fn screen(app: &App) -> Screen {
        *app.world().resource::<State<Screen>>().get()
    }

    /// Enter plays a pasted stage as the last stage of the list, E opens
    /// it in the editor, and Esc only puts the card away.
    #[test]
    fn a_pasted_stage_is_played_or_edited() {
        let mut app = card_app(stage("Card Probe"));
        press(&mut app, KeyCode::Enter);
        assert_eq!(screen(&app), Screen::Puzzle);
        let campaign = app.world().resource::<Campaign>();
        assert_eq!(campaign.current().name, "Card Probe");
        assert!(!app.world().resource::<LevelCard>().is_open());

        let mut app = card_app(stage("Card Probe"));
        press(&mut app, KeyCode::KeyE);
        assert_eq!(screen(&app), Screen::Editor);
        let opening = app.world().resource::<crate::app::editor::EditorOpening>();
        let opened = opening.0.as_ref().expect("the editor opens on it");
        assert_eq!(
            (opened.level.name.as_str(), opened.test),
            ("Card Probe", false)
        );

        let mut app = card_app(stage("Card Probe"));
        press(&mut app, KeyCode::Escape);
        assert_eq!(screen(&app), Screen::Menu);
        assert!(!app.world().resource::<LevelCard>().is_open());
    }

    /// A code taken up on the card counts as one taken, as a paste onto the
    /// editor's bench does; the player's own level coming back does not.
    #[test]
    fn a_code_taken_on_the_card_counts() {
        let count = |app: &App| {
            app.world()
                .resource::<Messages<crate::app::CodeTaken>>()
                .len()
        };
        let mut app = card_app(stage("Card Taken Probe"));
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::KeyE);
        app.update();
        assert!(count(&app) > 0, "somebody's code, taken");

        let name = "Card Own Probe";
        let path = crate::app::editor::custom_dir().join("card-own-probe.txt");
        crate::app::paths::write_atomic(&path, stage(name).to_text()).expect("filed");
        let mut app = card_app(stage(name));
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::KeyE);
        app.update();
        assert_eq!(count(&app), 0, "the player's own");
        let _ = std::fs::remove_file(&path);
    }

    /// A pasted beach is played in the editor's playtest, against the AI.
    #[test]
    fn a_pasted_beach_is_played_against_the_ai() {
        let mut app = card_app(stage("Card Beach").with_kind(LevelKind::Arena));
        press(&mut app, KeyCode::Enter);
        assert_eq!(screen(&app), Screen::Editor);
        let opening = app.world().resource::<crate::app::editor::EditorOpening>();
        assert!(opening.0.as_ref().is_some_and(|opened| opened.test));
    }

    /// S files the level on the shelf, and over a file of the same name
    /// asks first and replaces it on the second press.
    #[test]
    fn s_saves_and_asks_before_replacing() {
        let name = "Card Save Probe";
        let path = crate::app::editor::custom_dir().join("card-save-probe.txt");
        let _ = std::fs::remove_file(&path);
        let mut app = card_app(stage(name));
        press(&mut app, KeyCode::KeyS);
        assert!(path.exists(), "saved");
        assert!(!app.world().resource::<LevelCard>().is_open());

        let mut app = card_app(stage(name));
        press(&mut app, KeyCode::KeyS);
        let notice = app.world().resource::<crate::app::RoundNotice>().0.clone();
        let tr = &crate::app::i18n::EN;
        assert_eq!(notice, fill(tr.level_card_taken, &[("name", name)]));
        assert!(app.world().resource::<LevelCard>().is_open(), "asked");
        press(&mut app, KeyCode::KeyS);
        assert!(!app.world().resource::<LevelCard>().is_open(), "replaced");
        let _ = std::fs::remove_file(&path);
    }
}
