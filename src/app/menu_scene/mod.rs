//! The start screen: the title sign, the mode list, and the crab field
//! guide, over the beach postcard that [`scenery`] paints and animates.

mod level_card;
mod scenery;

pub use level_card::{LevelCard, answer_level_card, close_level_card, show_level_card};
pub use scenery::{menu_ambience, refit_shore, tend_backdrop};

use crate::app::effects::VisualRng;
use crate::app::gamepad::LastDevice;
use crate::app::i18n::fill;
use crate::app::menu_ui;
use crate::app::menu_ui::card_shadow;
use crate::app::palette;
use crate::app::settings::GameSettings;
use crate::app::{Campaign, Screen};
use bevy::prelude::*;

/// The wordmark, in plain letters in the game's display face. Not
/// letter-spaced with figure spaces: those space evenly only in a
/// monospace face.
const TITLE: &str = "PINCH POINTS";

/// One landing-menu entry, in display order (digit 1 launches the
/// first, a shortcut the list does not print). The launch ladder and the i18n name/blurb tables follow this
/// order; matching on the enum keeps them from drifting.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry {
    TidePool,
    TurfWar,
    Driftwood,
    Replay,
    BeachLobby,
    Settings,
    Achievements,
    DailyChallenge,
}

impl MenuEntry {
    pub const ALL: [MenuEntry; 8] = [
        MenuEntry::TidePool,
        MenuEntry::TurfWar,
        MenuEntry::Driftwood,
        MenuEntry::Replay,
        MenuEntry::BeachLobby,
        MenuEntry::Settings,
        MenuEntry::Achievements,
        MenuEntry::DailyChallenge,
    ];
}

/// Number of modes on the list (digit 1 points at entry 0, and so on).
pub const MENU_ENTRY_COUNT: usize = MenuEntry::ALL.len();

/// Which mode row the cursor is on. Persists across visits to the menu.
#[derive(Resource, Default)]
pub struct MenuList {
    pub selected: usize,
}

#[derive(Component)]
pub struct MenuRow(pub usize);

/// The two columns of a menu row. Separate text nodes rather than one
/// padded string, so each can carry its own size and ink: the mode name
/// leads and the blurb is an aside. In one colour they compete, and the
/// list reads as a wall.
///
/// No number column: the digit keys still move the cursor to a row, but a
/// console-style menu is walked, not dialled, a pad cannot press a digit,
/// and a printed number moved under anyone who had learned it whenever a
/// row came or went.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MenuCol {
    Name,
    Blurb,
}

/// One cell of one row.
#[derive(Component)]
pub struct MenuCell(pub usize, pub MenuCol);

/// Everything spawned for the menu decoration.
#[derive(Component)]
pub struct MenuArt;

/// The title sign and its tagline, floating on the sky.
fn spawn_title(commands: &mut Commands, settings: &GameSettings) {
    // Title card: floats over the sky, sized to its content so the
    // panorama shows around it.
    commands
        .spawn((
            MenuArt,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(44.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|wrap| {
            // The sign itself: the same deep-sea card with a gold hairline
            // the mode list, the crab legend and the stage list wear, which
            // also keeps pale letters legible where a cloud drifts behind.
            wrap.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(6.0),
                    padding: UiRect::axes(Val::Px(34.0), Val::Px(16.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(18.0)),
                    ..default()
                },
                BackgroundColor(palette::SIGN_FILL),
                BorderColor::all(palette::CARD_EDGE),
                card_shadow(),
                menu_ui::Framed,
            ))
            .with_children(|card| {
                card.spawn((
                    Text::new(TITLE),
                    menu_ui::display_font(menu_ui::type_scale::TITLE),
                    TextColor(palette::TITLE_INK),
                    TextShadow {
                        offset: Vec2::new(2.0, 3.0),
                        color: palette::TITLE_SHADOW,
                    },
                ));
                card.spawn((
                    Text::new(settings.tr().tagline),
                    TextFont {
                        font_size: FontSize::Px(menu_ui::type_scale::ROW),
                        ..default()
                    },
                    TextColor(palette::PARCHMENT),
                ));
            });
        });
}

/// The mode list, centred over the sea.
fn spawn_mode_list(commands: &mut Commands) {
    commands
        .spawn((
            MenuArt,
            Node {
                position_type: PositionType::Absolute,
                // Under the title sign, with the sea behind it. The crab
                // legend belongs on the screens with crabs to compare it
                // against, not here.
                top: Val::Px(196.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|wrap| {
            wrap.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    // The rows carry their own padding now, so the gap
                    // between them is a hairline rather than a blank line.
                    row_gap: Val::Px(2.0),
                    padding: UiRect::axes(Val::Px(20.0), Val::Px(16.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(18.0)),
                    ..default()
                },
                BackgroundColor(palette::CARD_FILL),
                BorderColor::all(palette::CARD_EDGE),
                card_shadow(),
                menu_ui::Framed,
            ))
            .with_children(|panel| {
                for row in 0..MENU_ENTRY_COUNT {
                    panel
                        .spawn((
                            MenuRow(row),
                            Node {
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(14.0),
                                padding: UiRect::axes(Val::Px(12.0), Val::Px(4.0)),
                                border_radius: BorderRadius::all(Val::Px(8.0)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|line| {
                            for (col, width, size) in
                                [(MenuCol::Name, 216.0, 21.0), (MenuCol::Blurb, 384.0, 17.0)]
                            {
                                line.spawn((
                                    Node {
                                        width: Val::Px(width),
                                        flex_shrink: 0.0,
                                        overflow: Overflow::clip_x(),
                                        ..default()
                                    },
                                    children![(
                                        MenuCell(row, col),
                                        Text::new(""),
                                        TextFont {
                                            font_size: FontSize::Px(size),
                                            ..default()
                                        },
                                        TextLayout::no_wrap(),
                                        TextColor(palette::IDLE_ROW),
                                    )],
                                ));
                            }
                        });
                }
            });
        });
}

pub fn enter_menu(mut commands: Commands, settings: Res<GameSettings>) {
    spawn_title(&mut commands, &settings);
    spawn_mode_list(&mut commands);
    spawn_version(&mut commands);
}

/// The build's version, small, in the bottom-right corner: the one thing
/// worth reading off a screenshot when a friend's beach will not take a
/// join, and the number the new-version page is comparing against.
///
/// On the prompt line's own dark pill, at the other end, so the two read as
/// one strip along the bottom rather than a label loose on the sand.
fn spawn_version(commands: &mut Commands) {
    commands.spawn((
        MenuArt,
        Text::new(crate::app::update::Version::current().to_string()),
        TextFont {
            font_size: FontSize::Px(menu_ui::type_scale::BODY),
            ..default()
        },
        TextLayout::no_wrap(),
        TextColor(palette::PARCHMENT.with_alpha(0.75)),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(8.0),
            right: Val::Px(12.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)),
            border_radius: BorderRadius::all(Val::Px(11.0)),
            ..default()
        },
        BackgroundColor(palette::PILL_FILL),
    ));
}

/// Style the mode rows around the selection, settings-menu style.
pub fn update_menu_rows(
    list: Res<MenuList>,
    settings: Res<GameSettings>,
    mut cells: Query<(&MenuCell, &mut Text, &mut TextColor)>,
    mut rows: Query<(&MenuRow, &mut BackgroundColor)>,
) {
    let tr = settings.tr();
    for (cell, mut text, mut color) in &mut cells {
        let picked = cell.0 == list.selected;
        let line = {
            match cell.1 {
                MenuCol::Name => tr.menu_names[cell.0].to_string(),
                // The daily is the one row whose blurb is not the same
                // every day, and the date is the whole point of it: you
                // want to know whether you have already played today's.
                MenuCol::Blurb if MenuEntry::ALL[cell.0] == MenuEntry::DailyChallenge => {
                    let (day, month) = crate::app::clock::civil_date(crate::app::Daily::today());
                    fill(
                        tr.menu_daily_blurb,
                        &[("d", &format!("{day:02}")), ("m", &format!("{month:02}"))],
                    )
                }
                MenuCol::Blurb => tr.menu_blurbs[cell.0].to_string(),
            }
        };
        // Two inks, and the row you are on brightens both: the name leads
        // and the blurb explains.
        let target = match (cell.1, picked) {
            (MenuCol::Name, true) => Color::WHITE,
            (MenuCol::Name, false) => palette::PARCHMENT.with_alpha(0.92),
            (MenuCol::Blurb, true) => palette::PARCHMENT.with_alpha(0.85),
            (MenuCol::Blurb, false) => palette::PARCHMENT.with_alpha(0.45),
        };
        menu_ui::set_text(&mut text, &line);
        menu_ui::set_color(&mut color, target);
    }
    for (row, mut fill) in &mut rows {
        // A band behind the cursor rather than a marker beside it. The list
        // is a column of rows two words wide; a caret at the far left is a long way
        // from the words it is pointing at.
        let ground = if row.0 == list.selected {
            palette::GOLD.with_alpha(0.16)
        } else {
            Color::NONE
        };
        menu_ui::set_bg(&mut fill, ground);
    }
}

/// Where the menu's choice takes the player.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Launch<'w> {
    campaign: ResMut<'w, Campaign>,
    daily: ResMut<'w, crate::app::Daily>,
    next_screen: ResMut<'w, NextState<Screen>>,
}

/// A pasted code is the other way onto a beach mid-play, or onto a level
/// somebody made, and it does not belong to any one row: V works wherever
/// the cursor is, but not under the quit card or the level card. A
/// round's code goes straight into the round; a level's puts up the card
/// that asks what to do with it ([`LevelCard`]).
pub fn paste_round(
    keyboard: crate::app::keycaps::Keyboard,
    settings: Res<GameSettings>,
    mut clipboard: ResMut<Clipboard>,
    mut notice: ResMut<crate::app::RoundNotice>,
    mut resuming: ResMut<crate::app::Resuming>,
    mut next_screen: ResMut<NextState<Screen>>,
    (quit, mut card): (Res<QuitCard>, ResMut<LevelCard>),
) {
    if quit.open || card.is_open() || !keyboard.just('V') || menu_ui::screen_chosen(&next_screen) {
        return;
    }
    let pasted = crate::app::codes::paste(&mut clipboard);
    if matches!(pasted, Some((crate::share::Kind::Level, _))) {
        match crate::app::editor::level_from_code(pasted, settings.tr()) {
            Ok(level) => {
                notice.0.clear();
                card.show(level);
            }
            Err(complaint) => notice.0 = complaint,
        }
        return;
    }
    match crate::app::suspend::round_from(pasted, settings.tr()) {
        Ok(round) => {
            notice.0.clear();
            resuming.0 = Some(round);
            next_screen.set(Screen::Versus);
        }
        Err(complaint) => notice.0 = complaint,
    }
}

/// The main menu's quit card: up while a pad's player is asked whether to
/// leave the game.
///
/// A pad's Start asks rather than leaves, as the keyboard's Esc does: Start
/// is the button a player reaches for to get going, and on the first
/// screen a stray press would close the game under them. Esc on the menu
/// stays an instant quit, the way every desktop game reads it; on the
/// card it is the keyboard's way back, and Enter its way out
/// ([`quit_card_keys`]).
#[derive(Resource, Default)]
pub struct QuitCard {
    pub open: bool,
}

/// The quit card on screen.
#[derive(Component)]
pub struct QuitUi;

/// The quit card's line of keys, which follows the hands last used.
#[derive(Component)]
pub struct QuitKeys;

/// The quit card's keys as its line says them: the keyboard's (Esc back,
/// Enter quits, what [`menu_input`] reads there) or the pad's, as the
/// card's own table names them.
pub fn quit_card_keys(tr: &crate::app::i18n::Tr, device: LastDevice) -> String {
    match device {
        LastDevice::Keyboard => tr.prompt_quitting.to_string(),
        LastDevice::Pad(family) => {
            crate::app::gamepad::scheme::legend(tr, family, &crate::app::gamepad::Face::Quitting)
        }
    }
}

/// Draw the quit card while it is up, and take it down once it is not: its
/// question, and its keys for the hands last used, rewritten when they
/// change under it.
pub fn show_quit_card(
    mut commands: Commands,
    card: Res<QuitCard>,
    (settings, device): (Res<GameSettings>, Res<LastDevice>),
    drawn: Query<Entity, With<QuitUi>>,
    mut keys: Query<&mut Text, With<QuitKeys>>,
) {
    let tr = settings.tr();
    if card.open == !drawn.is_empty() {
        for mut text in &mut keys {
            menu_ui::set_text(&mut text, &quit_card_keys(tr, *device));
        }
        return;
    }
    for entity in &drawn {
        commands.entity(entity).despawn();
    }
    if !card.open {
        return;
    }
    let legend = quit_card_keys(tr, *device);
    commands
        .spawn((
            QuitUi,
            GlobalZIndex(menu_ui::layer::CARD),
            menu_ui::centred_overlay(),
        ))
        .with_children(|wrap| {
            wrap.spawn(menu_ui::screen_card()).with_children(|card| {
                card.spawn((
                    Text::new(tr.quit_ask),
                    menu_ui::display_font(30.0),
                    TextColor(palette::GOLD),
                ));
                card.spawn(menu_ui::spacer(10.0));
                card.spawn((
                    QuitKeys,
                    Text::new(legend),
                    menu_ui::display_font(menu_ui::type_scale::ROW),
                    TextColor(palette::PARCHMENT),
                ));
            });
        });
}

/// Leaving the menu puts the quit card away.
pub fn close_quit_card(
    mut card: ResMut<QuitCard>,
    drawn: Query<Entity, With<QuitUi>>,
    mut commands: Commands,
) {
    card.open = false;
    for entity in &drawn {
        commands.entity(entity).despawn();
    }
}

/// The landing menu: W/S (or arrows) move the selection, the digit keys
/// put it on that row, and Enter launches it. A digit only points, as it
/// does in the lobby's list: with no numbers printed beside the rows, a
/// stray press launching a mode was a mode nobody asked for.
///
/// Esc leaves the game. A pad's Start raises the quit card instead
/// ([`QuitCard`]), whose A or Enter leaves and whose B, Start or Esc puts
/// it away; the menu waits while it is up.
pub fn menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    pads: crate::app::gamepad::face::PadsOnFace,
    (mut quit, level_card): (ResMut<QuitCard>, Res<LevelCard>),
    mut exit: MessageWriter<AppExit>,
    mut list: ResMut<MenuList>,
    launch: Launch,
) {
    let Launch {
        mut campaign,
        mut daily,
        mut next_screen,
    } = launch;
    use crate::app::gamepad::scheme::Does;
    // A pasted level's card has the keys while it is up
    // ([`answer_level_card`]), Esc among them.
    if level_card.is_open() {
        return;
    }
    // The card's keys, the keyboard's as well as the pad's: Esc goes back
    // there, as it does on every other card, rather than leaving.
    if quit.open {
        if pads.pressed(Does::Quit) || menu_ui::enter(&keys) {
            exit.write(AppExit::Success);
        } else if pads.pressed(Does::Back) || keys.just_pressed(KeyCode::Escape) {
            quit.open = false;
        }
        return;
    }
    // Every other screen leaves on Esc, and the menu is the one screen with
    // nowhere left to go: it leaves the game. Without this the only way out
    // is the window's own close button, which a fullscreen player has not
    // got.
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
        return;
    }
    if pads.pressed(Does::Quit) {
        quit.open = true;
        return;
    }
    list.selected = menu_ui::nav(&keys, list.selected, MENU_ENTRY_COUNT);
    if let Some(row) = menu_ui::number_pressed(&keys, MENU_ENTRY_COUNT) {
        list.selected = row;
        return;
    }
    if !menu_ui::enter(&keys) {
        return;
    }
    match MenuEntry::ALL[list.selected] {
        MenuEntry::TidePool => {
            campaign.reset(crate::app::campaign::stage_list());
            // Through the stage select, which is where the player picks up
            // where they left off.
            next_screen.set(Screen::StageSelect);
        }
        MenuEntry::TurfWar => next_screen.set(Screen::MatchSetup),
        MenuEntry::Driftwood => next_screen.set(Screen::Editor),
        // The library, which lists every round kept - including the newest.
        MenuEntry::Replay => next_screen.set(Screen::Replays),
        MenuEntry::BeachLobby => next_screen.set(Screen::Lobby),
        MenuEntry::Settings => next_screen.set(Screen::Settings),
        MenuEntry::Achievements => next_screen.set(Screen::Achievements),
        MenuEntry::DailyChallenge => {
            // The daily brings its own table (`MatchConfig::daily`), read
            // by `load_versus` while this is set.
            daily.active = true;
            next_screen.set(Screen::Versus);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The menu as it runs: the face read from the quit card, the pad
    /// bridge pressing the face's keys, the menu's input, the quit card,
    /// and one pad plugged in.
    fn menu_app() -> (App, Entity) {
        use crate::app::gamepad::{Bridged, PadFace, PadPick, face::menu_face};
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.insert_state(Screen::Menu);
        app.add_message::<AppExit>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<MenuList>();
        app.init_resource::<QuitCard>();
        app.init_resource::<LevelCard>();
        app.init_resource::<crate::app::Daily>();
        app.insert_resource(Campaign::tide_pool());
        app.init_resource::<PadFace>();
        app.init_resource::<Bridged>();
        app.init_resource::<PadPick>();
        app.init_resource::<Time>();
        app.init_resource::<LastDevice>();
        app.init_resource::<GameSettings>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<crate::app::cursor::LiveSeatKeys>();
        let face = |quit: Res<QuitCard>, card: Res<LevelCard>, mut face: ResMut<PadFace>| {
            face.0 = menu_face(&quit, &card);
        };
        app.add_systems(
            Update,
            (
                face,
                crate::app::gamepad::pad_menu_bridge,
                menu_input,
                show_quit_card,
                crate::app::gamepad::pad_bridge_release,
            )
                .chain(),
        );
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        (app, pad)
    }

    /// One press of `button` on `pad`, over two frames: down, then up.
    fn tap(app: &mut App, pad: Entity, button: GamepadButton) {
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("the pad");
        gamepad.digital_mut().clear();
        gamepad.digital_mut().press(button);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("the pad");
        gamepad.digital_mut().release(button);
        gamepad.digital_mut().clear();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    }

    fn quit_asked(app: &App) -> bool {
        app.world().resource::<QuitCard>().open
    }

    fn left(app: &App) -> bool {
        !app.world().resource::<Messages<AppExit>>().is_empty()
    }

    fn cards_drawn(app: &mut App) -> usize {
        app.world_mut()
            .query_filtered::<Entity, With<QuitUi>>()
            .iter(app.world())
            .count()
    }

    /// A pad's Start on the menu asks before leaving: the card comes up
    /// and holds the menu still under it, B or Start again puts it away,
    /// and A on it leaves the game. Through the bridge, as a pad plays.
    #[test]
    fn a_pads_start_asks_before_leaving() {
        use crate::app::gamepad::scheme::{Face, legend};
        let tr = &crate::app::i18n::EN;
        assert!(
            legend(tr, crate::app::gamepad::PadFamily::Xbox, &Face::Menu).contains("Start: quit")
        );
        let (mut app, pad) = menu_app();
        app.update();
        tap(&mut app, pad, GamepadButton::Start);
        assert!(quit_asked(&app), "Start raises the card");
        assert!(!left(&app), "and leaves nothing yet");
        assert_eq!(cards_drawn(&mut app), 1);
        // The menu waits under the card: the d-pad and A as Enter do not
        // reach it, nor launch anything.
        tap(&mut app, pad, GamepadButton::DPadDown);
        assert_eq!(app.world().resource::<MenuList>().selected, 0);
        tap(&mut app, pad, GamepadButton::East);
        assert!(!quit_asked(&app), "B puts it away");
        assert_eq!(cards_drawn(&mut app), 0);
        assert_eq!(*app.world().resource::<State<Screen>>().get(), Screen::Menu);
        tap(&mut app, pad, GamepadButton::Start);
        tap(&mut app, pad, GamepadButton::Start);
        assert!(!quit_asked(&app), "so does Start again");
        assert!(!left(&app));
        tap(&mut app, pad, GamepadButton::DPadDown);
        assert_eq!(
            app.world().resource::<MenuList>().selected,
            1,
            "the menu walks again"
        );
        tap(&mut app, pad, GamepadButton::Start);
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("the pad");
        gamepad.digital_mut().press(GamepadButton::South);
        app.update();
        assert!(left(&app), "A on the card leaves the game");
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Menu,
            "and launches nothing on the way"
        );
    }

    /// The quit card at the keyboard: Esc puts it away rather than
    /// leaving, Enter leaves, and the card's line names those keys while
    /// the keyboard was the last hands used, the pad's buttons while a pad
    /// was. It used to swallow Enter and leave on Esc, its line naming
    /// only the pad's buttons.
    #[test]
    fn the_quit_card_answers_the_keyboard_and_says_so() {
        let tr = &crate::app::i18n::EN;
        assert_eq!(
            quit_card_keys(tr, LastDevice::Keyboard),
            "Esc: back | Enter: quit"
        );
        assert_eq!(
            quit_card_keys(tr, LastDevice::PAD),
            "A: quit | B/Start: back"
        );
        let (mut app, pad) = menu_app();
        app.update();
        let key = |app: &mut App, key: KeyCode| {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            app.update();
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(key);
            keys.clear();
            app.update();
        };
        let line = |app: &mut App| {
            app.world_mut()
                .query_filtered::<&Text, With<QuitKeys>>()
                .single(app.world())
                .map(|text| text.0.clone())
                .expect("the card's keys")
        };
        tap(&mut app, pad, GamepadButton::Start);
        assert!(quit_asked(&app));
        app.insert_resource(LastDevice::Keyboard);
        app.update();
        assert_eq!(line(&mut app), "Esc: back | Enter: quit");
        app.insert_resource(LastDevice::PAD);
        app.update();
        assert_eq!(line(&mut app), "A: quit | B/Start: back");
        key(&mut app, KeyCode::Escape);
        assert!(!quit_asked(&app), "Esc puts the card away");
        assert!(!left(&app), "and leaves nothing");
        tap(&mut app, pad, GamepadButton::Start);
        key(&mut app, KeyCode::Enter);
        assert!(left(&app), "Enter on the card leaves the game");
        assert_eq!(*app.world().resource::<State<Screen>>().get(), Screen::Menu);
    }

    /// The keyboard's Esc on the menu leaves at once, without a card.
    #[test]
    fn escape_leaves_at_once() {
        let (mut app, _) = menu_app();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(left(&app));
        assert!(!quit_asked(&app), "without asking");
    }

    /// A digit puts the cursor on its row and launches nothing; Enter
    /// launches the row the cursor is on.
    #[test]
    fn a_digit_points_and_enter_launches() {
        let (mut app, _) = menu_app();
        let press = |app: &mut App, key: KeyCode| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(key);
            app.update();
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(key);
            keys.clear();
            app.update();
        };
        press(&mut app, KeyCode::Digit3);
        assert_eq!(app.world().resource::<MenuList>().selected, 2);
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Menu,
            "a digit launches nothing"
        );
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Editor,
            "Enter launches the row pointed at"
        );
    }
}
