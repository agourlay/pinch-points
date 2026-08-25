//! The start screen: the title sign, the mode list, and the crab field
//! guide, over the beach postcard that [`scenery`] paints and animates.

mod scenery;

pub use scenery::{menu_ambience, refit_shore, tend_backdrop};

use crate::app::effects::VisualRng;
use crate::app::i18n::fill;
use crate::app::menu_ui;
use crate::app::menu_ui::card_shadow;
use crate::app::palette;
use crate::app::settings::GameSettings;
use crate::app::{Campaign, CampaignKind, Screen};
use crate::sim::challenge_levels;
use bevy::prelude::*;

/// The wordmark, spaced out by hand.
///
/// Set in the game's own face rather than a drawn pixel-font, letter-spaced
/// with figure spaces (U+2007, as wide as a digit) so it reads as a sign
/// rather than a sentence. The gap between the two words is three of them,
/// so "PINCH" and "POINTS" still read as two words at that spacing.
const TITLE: &str = "P\u{2007}I\u{2007}N\u{2007}C\u{2007}H\u{2007}\u{2007}\u{2007}\u{2007}P\u{2007}O\u{2007}I\u{2007}N\u{2007}T\u{2007}S";

/// One landing-menu entry, in display order (digit 1 launches the
/// first). The launch ladder and the i18n name/blurb tables follow this
/// order; matching on the enum keeps them from drifting.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry {
    TidePool,
    TurfWar,
    Driftwood,
    Replay,
    BeachLobby,
    Settings,
    BeachDay,
    Achievements,
    DailyChallenge,
}

impl MenuEntry {
    pub const ALL: [MenuEntry; 9] = [
        MenuEntry::TidePool,
        MenuEntry::TurfWar,
        MenuEntry::Driftwood,
        MenuEntry::Replay,
        MenuEntry::BeachLobby,
        MenuEntry::Settings,
        MenuEntry::BeachDay,
        MenuEntry::Achievements,
        MenuEntry::DailyChallenge,
    ];
}

/// Number of modes on the list (digit 1 launches entry 0, and so on).
pub const MENU_ENTRY_COUNT: usize = MenuEntry::ALL.len();

/// Which mode row the cursor is on. Persists across visits to the menu.
#[derive(Resource, Default)]
pub struct MenuList {
    pub selected: usize,
}

#[derive(Component)]
pub struct MenuRow(pub usize);

/// The three columns of a menu row. They are separate text nodes rather
/// than one padded string so each can carry its own size and ink: the
/// hotkey wants to recede, the mode name wants to lead, and the blurb is
/// an aside. As one string in one colour they competed, which is what made
/// the list read as a wall.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MenuCol {
    Key,
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
            // that the mode list, the crab legend and the stage list wear,
            // tying the postcard to the rest of the game and keeping pale
            // letters legible where a cloud drifts behind.
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
            ))
            .with_children(|card| {
                card.spawn((
                    Text::new(TITLE),
                    TextFont {
                        font_size: FontSize::Px(54.0),
                        ..default()
                    },
                    TextColor(palette::TITLE_INK),
                    TextShadow {
                        offset: Vec2::new(2.0, 3.0),
                        color: palette::TITLE_SHADOW,
                    },
                ));
                card.spawn((
                    Text::new(settings.tr().tagline),
                    TextFont {
                        font_size: FontSize::Px(18.0),
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
                // Under the title sign, with the sea behind it: the crab
                // legend that used to sit below has moved to the screens
                // where there are crabs to compare it against.
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
                            for (col, width, size) in [
                                (MenuCol::Key, 16.0, 17.0),
                                (MenuCol::Name, 216.0, 21.0),
                                (MenuCol::Blurb, 384.0, 17.0),
                            ] {
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
/// On the prompt line's own dark pill, at the other end of it, so the two
/// read as one strip along the bottom rather than as a label loose on the
/// sand.
fn spawn_version(commands: &mut Commands) {
    commands.spawn((
        MenuArt,
        Text::new(crate::app::update::Version::current().to_string()),
        TextFont {
            font_size: FontSize::Px(15.0),
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
                // Rows are keyed 1-9, one character wide apiece, which
                // keeps the name column straight.
                MenuCol::Key => (cell.0 + 1).to_string(),
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
        // Three inks, and the row you are on brightens all of them: the
        // name leads, the blurb explains, the hotkey is there when you
        // want it and out of the way when you do not.
        let target = match (cell.1, picked) {
            (MenuCol::Key, true) => palette::GOLD,
            (MenuCol::Key, false) => palette::PARCHMENT.with_alpha(0.30),
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
        // is nine rows of two columns; a caret at the far left is a long way
        // from the words it is pointing at.
        let ground = if row.0 == list.selected {
            palette::GOLD.with_alpha(0.16)
        } else {
            Color::NONE
        };
        menu_ui::set_bg(&mut fill, ground);
    }
}

/// The landing menu: W/S (or arrows) move the selection, Enter launches it,
/// and the digit hotkeys still jump straight into any mode.
#[allow(clippy::too_many_arguments)]
pub fn menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut exit: MessageWriter<AppExit>,
    mut list: ResMut<MenuList>,
    settings: Res<GameSettings>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    mut campaign: ResMut<Campaign>,
    mut config: ResMut<crate::app::match_setup::MatchConfig>,
    mut daily: ResMut<crate::app::Daily>,
    mut resuming: ResMut<crate::app::Resuming>,
    mut notice: ResMut<crate::app::RoundNotice>,
    mut clipboard: ResMut<Clipboard>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    // Every other screen leaves on Esc, and the menu is the one screen with
    // nowhere left to go: it leaves the game. Without this the only way out
    // is the window's own close button, which a fullscreen player has not
    // got.
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
        return;
    }
    // A pasted round is the other way onto a beach mid-play, and it does not
    // belong to any one row: V works wherever the cursor is.
    if caps.just_pressed(&keys, 'V') {
        match crate::app::suspend::round_from(
            crate::app::codes::paste(&mut clipboard),
            settings.tr(),
        ) {
            Ok(round) => {
                notice.0.clear();
                resuming.0 = Some(round);
                next_screen.set(Screen::Versus);
            }
            Err(complaint) => notice.0 = complaint,
        }
        return;
    }
    list.selected = menu_ui::nav(&keys, list.selected, MENU_ENTRY_COUNT);

    const HOTKEYS: [KeyCode; MENU_ENTRY_COUNT] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    let choice = if menu_ui::enter(&keys) {
        Some(list.selected)
    } else {
        HOTKEYS.iter().position(|&key| keys.just_pressed(key))
    };
    let Some(choice) = choice else {
        return;
    };
    list.selected = choice;
    match MenuEntry::ALL[choice] {
        MenuEntry::TidePool => {
            let (levels, builtins) = crate::app::campaign::tide_pool_levels();
            campaign.reset(CampaignKind::TidePool, levels, builtins);
            // Both lists go through the stage select, which is where the
            // player picks up where they left off.
            next_screen.set(Screen::StageSelect);
        }
        MenuEntry::TurfWar => next_screen.set(Screen::MatchSetup),
        MenuEntry::Driftwood => next_screen.set(Screen::Editor),
        // The library, which lists every round kept - including the newest.
        MenuEntry::Replay => next_screen.set(Screen::Replays),
        MenuEntry::BeachLobby => next_screen.set(Screen::Lobby),
        MenuEntry::Settings => next_screen.set(Screen::Settings),
        MenuEntry::BeachDay => {
            let levels = challenge_levels();
            let builtins = levels.len();
            campaign.reset(CampaignKind::BeachDay, levels, builtins);
            next_screen.set(Screen::StageSelect);
        }
        MenuEntry::Achievements => next_screen.set(Screen::Achievements),
        MenuEntry::DailyChallenge => {
            // The daily: today's arena, three fierce bots, standard round.
            config.seats = 4;
            config.bots = 3;
            config.bot_levels = [crate::sim::BotLevel::Hard; crate::sim::MAX_PLAYERS];
            config.map = crate::app::match_setup::MapChoice::GenClassic;
            config.gulls = crate::app::match_setup::GullPressure::Normal;
            config.round = crate::app::match_setup::RoundLength::Standard;
            config.armed = true;
            daily.active = true;
            next_screen.set(Screen::Versus);
        }
    }
}
