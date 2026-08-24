//! Is there a newer release? Start-up asks GitHub, off-thread, and if the
//! answer is yes the game steps off the menu onto a page of its own, the
//! way a first run steps onto the language picker: the new version's
//! number, its release notes, and one question - update? Yes opens the
//! release page in the browser; no is no, until next start-up.
//!
//! Open, not install: the game is one binary that came from the release
//! page or from `cargo install`, and the honest thing a running binary can
//! do about a newer one is show the player where it is. The page is
//! reached from the menu and nowhere else, so an answer arriving mid-round
//! never lands on top of a beach.
//!
//! The check is the one thing the game says to the wider internet, so it
//! is a setting (on by default). Nothing waits on it: a machine with no
//! network, or a slow one, finds that out on the check thread inside a
//! short timeout, and the menu never hears of it.

pub mod github;
mod notes;

pub use github::{Release, Version};

use crate::app::i18n::fill;
use crate::app::menu_ui;
use crate::app::open::open_url;
use crate::app::palette;
use crate::app::settings::GameSettings;
use crate::app::{RoundNotice, Screen, dev};
use bevy::prelude::*;
use github::{demo_release, fetch_latest, latest_release_urls};
use notes::{notes_budget, notes_lines};
use std::sync::{Arc, OnceLock};

/// The page's two answers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    /// Hand the release page to the browser.
    Yes,
    /// Not now. Asked again next start-up, if it is still out there:
    /// on a shared network a version behind is a version that cannot
    /// join the others' beach, which is worth being reminded of.
    No,
}

impl Choice {
    pub const ALL: [Choice; 2] = [Choice::Yes, Choice::No];
}

/// The check, from the thread that runs it to the page that reports it.
#[derive(Resource, Default)]
pub struct UpdateCheck {
    /// The check thread's answer, filled in once it has one. `None` when
    /// no check is running: never started, or already read.
    reply: Option<Arc<OnceLock<Option<Release>>>>,
    /// A newer release, from the moment the answer is in until the page
    /// has been shown and left. The menu goes to the page while this
    /// stands, so an answer that lands on the same frame as a keypress
    /// is shown on the way back rather than lost.
    pub offer: Option<Release>,
    /// Which row of the page the cursor is on.
    pub selected: usize,
    /// The menu's notice as it stood when the page was stepped onto,
    /// put back on the way home.
    stashed_notice: String,
}

impl UpdateCheck {
    /// Take the thread's answer, if it is in.
    fn take_reply(&mut self) -> Option<Option<Release>> {
        let answered = self.reply.as_ref()?.get().cloned()?;
        self.reply = None;
        Some(answered)
    }

    /// Whether a release is worth the page: newer than this build.
    pub fn worth_offering(current: &Version, release: &Release) -> bool {
        release.version > *current
    }
}

/// Everything the page spawns.
#[derive(Component)]
pub struct NewVersionUi;

#[derive(Component)]
pub struct NewVersionRow(usize);

/// Kick the check off, on its own thread. Startup.
pub fn start_check(settings: Res<GameSettings>, mut check: ResMut<UpdateCheck>) {
    if dev::no_update_check() {
        return;
    }
    let slot = Arc::new(OnceLock::new());
    if dev::update_demo() {
        let _ = slot.set(Some(demo_release()));
        check.reply = Some(slot);
        return;
    }
    if !settings.check_updates {
        return;
    }
    let Some(urls) = latest_release_urls() else {
        warn!("update check: the manifest's repository is not on GitHub");
        return;
    };
    let answer = Arc::clone(&slot);
    let spawned = std::thread::Builder::new()
        .name("update-check".into())
        .spawn(move || {
            let _ = answer.set(fetch_latest(&urls));
        });
    match spawned {
        Ok(_) => check.reply = Some(slot),
        Err(e) => warn!("update check: could not start: {e}"),
    }
}

/// Watch for the check's answer and, on a newer release, step onto the
/// page. Menu only, so the answer waits for the menu rather than
/// interrupting a round or the first-run language picker.
///
/// The setting is read here as well as at start-up: a player who went
/// straight to Settings and turned the check off has answered the
/// question before it was asked, and a reply that landed meanwhile is
/// dropped rather than shown.
pub fn poll_check(
    settings: Res<GameSettings>,
    mut check: ResMut<UpdateCheck>,
    mut notice: ResMut<RoundNotice>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    if !settings.check_updates && !dev::update_demo() {
        check.reply = None;
        check.offer = None;
        return;
    }
    if let Some(Some(release)) = check.take_reply()
        && UpdateCheck::worth_offering(&Version::current(), &release)
    {
        check.offer = Some(release);
        check.selected = 0;
    }
    if check.offer.is_some() {
        // The page is a detour off the menu, and leaving the menu clears
        // its notice slot: a save that failed a moment ago must still be
        // on the menu when the detour is over. Carried across by hand.
        check.stashed_notice = std::mem::take(&mut notice.0);
        next_screen.set(Screen::NewVersion);
    }
}

/// The page: the version and its notes on a card, the question under
/// them, and the note that the asking can be turned off.
pub fn enter_new_version(
    mut commands: Commands,
    settings: Res<GameSettings>,
    check: Res<UpdateCheck>,
    ui_scale: Res<UiScale>,
    windows: Query<&Window>,
) {
    let Some(release) = &check.offer else {
        return;
    };
    let tr = settings.tr();
    // The frame between the bars, in UI pixels: what the notes may fill.
    // Read the applied scale, as the camera does, since a small window
    // shrinks the interface as well.
    let frame_h = windows
        .single()
        .map_or(crate::app::settings::DESIGN_H, |window| {
            window.height() / ui_scale.0
        })
        - 2.0 * menu_ui::BAR_H;
    let mut lines = notes_lines(&release.notes, notes_budget(frame_h));
    if lines.is_empty() {
        lines.push(tr.update_no_notes.to_string());
    }
    commands
        .spawn((NewVersionUi, menu_ui::between_bars()))
        .with_children(|wrap| {
            wrap.spawn(menu_ui::screen_card()).with_children(|card| {
                card.spawn((
                    Text::new(fill(
                        tr.update_title,
                        &[("v", &release.version.to_string())],
                    )),
                    TextFont {
                        font_size: FontSize::Px(26.0),
                        ..default()
                    },
                    TextColor(palette::GOLD),
                ));
                card.spawn((
                    Text::new(fill(
                        tr.update_have,
                        &[("v", &Version::current().to_string())],
                    )),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(palette::PARCHMENT.with_alpha(0.55)),
                ));
                // The notes: a left-aligned block on a card whose rows are
                // centred, so it gets a column of its own.
                card.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::FlexStart,
                        align_self: AlignSelf::Stretch,
                        row_gap: Val::Px(1.0),
                        margin: UiRect::vertical(Val::Px(8.0)),
                        padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
                        border: UiRect::top(Val::Px(1.0)).with_bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(palette::CARD_EDGE),
                ))
                .with_children(|notes| {
                    for line in lines {
                        notes.spawn((
                            Text::new(line),
                            TextFont {
                                font_size: FontSize::Px(14.0),
                                ..default()
                            },
                            TextLayout::no_wrap(),
                            TextColor(palette::PARCHMENT),
                        ));
                    }
                });
                card.spawn((
                    Text::new(tr.update_question),
                    TextFont {
                        font_size: FontSize::Px(20.0),
                        ..default()
                    },
                    TextColor(palette::PARCHMENT),
                ));
                menu_ui::spawn_rows(card, Choice::ALL.len(), 22.0, NewVersionRow);
            });
            wrap.spawn((
                Text::new(tr.update_note),
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextLayout::no_wrap(),
                TextColor(palette::PARCHMENT.with_alpha(0.40)),
            ));
        });
}

/// Read the page's keys: where the cursor is now, and what was chosen, if
/// anything. Enter takes the row; Escape is no, as leaving is on every
/// other screen.
pub fn pick(keys: &ButtonInput<KeyCode>, selected: usize) -> (usize, Option<Choice>) {
    let selected = menu_ui::nav(keys, selected, Choice::ALL.len());
    let choice = if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        Some(Choice::ALL[selected])
    } else if keys.just_pressed(KeyCode::Escape) {
        Some(Choice::No)
    } else {
        None
    };
    (selected, choice)
}

/// Walk the page and answer it. Either answer goes back to the menu.
pub fn new_version_input(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<GameSettings>,
    mut check: ResMut<UpdateCheck>,
    mut notice: ResMut<RoundNotice>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    let (selected, choice) = pick(&keys, check.selected);
    check.selected = selected;
    let Some(choice) = choice else {
        return;
    };
    // Yes opens the page whatever the menu was saying. What the menu had
    // to say before the detour then comes first: a round that failed to
    // save outranks news that a browser was handed a page. A browser
    // that could not be opened still wins the line, though, since that
    // notice carries the address to type by hand and nothing else does.
    let mut said = std::mem::take(&mut check.stashed_notice);
    if choice == Choice::Yes
        && let Some(release) = &check.offer
    {
        let tr = settings.tr();
        match open_url(&release.url) {
            Ok(()) if said.is_empty() => said = tr.update_opened.to_string(),
            Ok(()) => {}
            Err(e) => {
                warn!("update check: could not open a browser: {e}");
                said = fill(tr.update_open_failed, &[("url", &release.url)]);
            }
        }
    }
    notice.0 = said;
    next_screen.set(Screen::Menu);
}

/// Paint the rows around the selection.
pub fn update_new_version_rows(
    check: Res<UpdateCheck>,
    settings: Res<GameSettings>,
    mut rows: Query<(&NewVersionRow, &mut Text, &mut TextColor)>,
) {
    let tr = settings.tr();
    let labels = [tr.update_yes, tr.update_no];
    for (row, mut text, mut color) in &mut rows {
        menu_ui::paint_row(
            row.0 == check.selected,
            labels[row.0],
            &mut text,
            &mut color,
        );
    }
}

/// Leaving the page is the end of the offer: it comes back only with a
/// fresh answer, and there is one per start-up.
pub fn exit_new_version(
    mut commands: Commands,
    mut check: ResMut<UpdateCheck>,
    ui: Query<Entity, With<NewVersionUi>>,
) {
    check.offer = None;
    for entity in &ui {
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::open::tests::OPENED;

    /// The page goes up for a newer release only: the same version, or an
    /// older one still on the release page, is nothing to say.
    #[test]
    fn only_a_newer_release_is_offered() {
        let release = |tag: &str| Release {
            version: Version::parse(tag).unwrap(),
            tag: tag.to_string(),
            url: "https://x/y".to_string(),
            notes: String::new(),
        };
        let current = Version::parse("v0.5.0").unwrap();
        assert!(UpdateCheck::worth_offering(&current, &release("v0.5.1")));
        assert!(UpdateCheck::worth_offering(&current, &release("v1.0.0")));
        assert!(!UpdateCheck::worth_offering(&current, &release("v0.5.0")));
        assert!(!UpdateCheck::worth_offering(&current, &release("v0.4.9")));
        // A release candidate of the next version is news; one of this
        // version is old news.
        assert!(UpdateCheck::worth_offering(
            &current,
            &release("v0.6.0-rc.1")
        ));
        assert!(!UpdateCheck::worth_offering(
            &current,
            &release("v0.5.0-rc.2")
        ));
        // And a candidate build hears when its release lands.
        let candidate = Version::parse("v0.5.0-rc.2").unwrap();
        assert!(UpdateCheck::worth_offering(&candidate, &release("v0.5.0")));
    }

    /// The keys, read the way the page reads them: W/S walk the two rows,
    /// Enter takes the one under the cursor, Escape is no wherever the
    /// cursor is.
    #[test]
    fn the_keys_walk_and_take_the_rows() {
        let mut keys = ButtonInput::<KeyCode>::default();
        assert_eq!(pick(&keys, 0), (0, None));
        keys.press(KeyCode::KeyS);
        assert_eq!(pick(&keys, 0), (1, None));
        assert_eq!(pick(&keys, 1), (0, None));
        keys.clear();
        keys.press(KeyCode::Enter);
        assert_eq!(pick(&keys, 0), (0, Some(Choice::Yes)));
        assert_eq!(pick(&keys, 1), (1, Some(Choice::No)));
        keys.clear();
        keys.press(KeyCode::Escape);
        assert_eq!(pick(&keys, 0), (0, Some(Choice::No)));
    }

    /// The whole detour, headless: the answer lands, the menu steps onto
    /// the page, no steps back to the menu, and the offer is spent - the
    /// next visit to the menu stays on the menu.
    #[test]
    fn a_newer_release_takes_the_menu_to_the_page_and_no_brings_it_back() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.insert_resource(GameSettings::default());
        app.init_resource::<RoundNotice>();
        app.init_resource::<UpdateCheck>();
        app.init_resource::<UiScale>();
        // No window: the page falls back to the design height.
        app.add_systems(
            Update,
            (
                poll_check.run_if(in_state(Screen::Menu)),
                (new_version_input, update_new_version_rows)
                    .chain()
                    .run_if(in_state(Screen::NewVersion)),
            ),
        );
        app.add_systems(OnEnter(Screen::NewVersion), enter_new_version);
        app.add_systems(OnExit(Screen::NewVersion), exit_new_version);
        let screen = |app: &App| *app.world().resource::<State<Screen>>().get();
        let tap = |app: &mut App, key: KeyCode| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(key);
            app.update();
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(key);
            keys.clear();
        };

        // The answer arrives: a release newer than this build.
        let slot = Arc::new(OnceLock::new());
        let _ = slot.set(Some(Release {
            version: Version::parse("v9.0.0").unwrap(),
            tag: "v9.0.0".to_string(),
            url: "https://x/y".to_string(),
            notes: "- one\n- two".to_string(),
        }));
        app.world_mut().resource_mut::<UpdateCheck>().reply = Some(slot);
        app.update();
        app.update();
        assert_eq!(screen(&app), Screen::NewVersion);
        // The page is spawned, notes and all.
        let mut rows = app.world_mut().query::<&NewVersionRow>();
        assert_eq!(rows.iter(app.world()).count(), Choice::ALL.len());
        let mut ui = app.world_mut().query::<&NewVersionUi>();
        assert_eq!(ui.iter(app.world()).count(), 1);

        // Down to "no", and take it.
        tap(&mut app, KeyCode::KeyS);
        assert_eq!(app.world().resource::<UpdateCheck>().selected, 1);
        tap(&mut app, KeyCode::Enter);
        app.update();
        assert_eq!(screen(&app), Screen::Menu);
        assert!(app.world().resource::<UpdateCheck>().offer.is_none());
        assert_eq!(ui.iter(app.world()).count(), 0);
        assert_eq!(app.world().resource::<RoundNotice>().0, "");
        // Spent: the menu stays the menu.
        app.update();
        app.update();
        assert_eq!(screen(&app), Screen::Menu);

        // A notice standing on the menu survives the detour: the page is
        // not what the menu was talking about.
        let slot = Arc::new(OnceLock::new());
        let _ = slot.set(Some(Release {
            version: Version::parse("v9.0.1").unwrap(),
            tag: "v9.0.1".to_string(),
            url: "https://x/y".to_string(),
            notes: String::new(),
        }));
        app.world_mut().resource_mut::<UpdateCheck>().reply = Some(slot);
        app.world_mut().resource_mut::<RoundNotice>().0 = "could not put the round down".into();
        app.update();
        app.update();
        assert_eq!(screen(&app), Screen::NewVersion);
        // As the schedule does on the way out of the menu.
        app.world_mut().resource_mut::<RoundNotice>().0.clear();
        tap(&mut app, KeyCode::Escape);
        app.update();
        assert_eq!(screen(&app), Screen::Menu);
        assert_eq!(
            app.world().resource::<RoundNotice>().0,
            "could not put the round down"
        );

        // Yes with a notice standing: the browser is still asked, and
        // the notice still has the line.
        let slot = Arc::new(OnceLock::new());
        let _ = slot.set(Some(Release {
            version: Version::parse("v9.0.2").unwrap(),
            tag: "v9.0.2".to_string(),
            url: "https://github.com/x/y/releases/tag/v9.0.2".to_string(),
            notes: String::new(),
        }));
        app.world_mut().resource_mut::<UpdateCheck>().reply = Some(slot);
        app.world_mut().resource_mut::<RoundNotice>().0 = "could not put the round down".into();
        app.update();
        app.update();
        assert_eq!(screen(&app), Screen::NewVersion);
        app.world_mut().resource_mut::<RoundNotice>().0.clear();
        let opened_before = OPENED.lock().unwrap().len();
        tap(&mut app, KeyCode::Enter);
        app.update();
        assert_eq!(screen(&app), Screen::Menu);
        assert_eq!(
            app.world().resource::<RoundNotice>().0,
            "could not put the round down"
        );
        assert_eq!(
            OPENED.lock().unwrap()[opened_before..],
            ["https://github.com/x/y/releases/tag/v9.0.2".to_string()]
        );

        // Yes with nothing standing says the page was handed over. (The
        // menu is left with its notice read, as it is in the schedule.)
        app.world_mut().resource_mut::<RoundNotice>().0.clear();
        let slot = Arc::new(OnceLock::new());
        let _ = slot.set(Some(Release {
            version: Version::parse("v9.0.3").unwrap(),
            tag: "v9.0.3".to_string(),
            url: "https://github.com/x/y/releases/tag/v9.0.3".to_string(),
            notes: String::new(),
        }));
        app.world_mut().resource_mut::<UpdateCheck>().reply = Some(slot);
        app.update();
        app.update();
        assert_eq!(screen(&app), Screen::NewVersion);
        tap(&mut app, KeyCode::Enter);
        app.update();
        assert_eq!(screen(&app), Screen::Menu);
        let tr = GameSettings::default().tr();
        assert_eq!(app.world().resource::<RoundNotice>().0, tr.update_opened);
        assert_eq!(
            OPENED.lock().unwrap().last().map(String::as_str),
            Some("https://github.com/x/y/releases/tag/v9.0.3")
        );
    }

    /// Turning the check off in Settings, on the same run it was started,
    /// drops the answer rather than showing it.
    #[test]
    fn switching_the_check_off_drops_an_answer_in_flight() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(GameSettings {
            check_updates: false,
            ..GameSettings::default()
        });
        app.init_resource::<RoundNotice>();
        app.init_resource::<UpdateCheck>();
        app.add_systems(Update, poll_check.run_if(in_state(Screen::Menu)));
        let slot = Arc::new(OnceLock::new());
        let _ = slot.set(Some(demo_release()));
        app.world_mut().resource_mut::<UpdateCheck>().reply = Some(slot);
        app.update();
        app.update();
        assert_eq!(*app.world().resource::<State<Screen>>().get(), Screen::Menu);
        let check = app.world().resource::<UpdateCheck>();
        assert!(check.reply.is_none() && check.offer.is_none());
    }

    /// An answer that is not newer never leaves the menu, and neither does
    /// no answer at all: the check failing is the check saying nothing.
    #[test]
    fn nothing_newer_is_nothing_said() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_resource::<UpdateCheck>();
        app.insert_resource(GameSettings::default());
        app.init_resource::<RoundNotice>();
        app.add_systems(Update, poll_check.run_if(in_state(Screen::Menu)));
        let screen = |app: &App| *app.world().resource::<State<Screen>>().get();
        for reply in [
            None,
            Some(Release {
                version: Version::current(),
                tag: Version::current().to_string(),
                url: "https://x/y".to_string(),
                notes: String::new(),
            }),
        ] {
            let slot = Arc::new(OnceLock::new());
            let _ = slot.set(reply);
            app.world_mut().resource_mut::<UpdateCheck>().reply = Some(slot);
            app.update();
            app.update();
            assert_eq!(screen(&app), Screen::Menu);
            assert!(app.world().resource::<UpdateCheck>().reply.is_none());
        }
    }
}
