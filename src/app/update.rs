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

use crate::app::i18n::fill;
use crate::app::menu_ui;
use crate::app::palette;
use crate::app::settings::GameSettings;
use crate::app::{RoundNotice, Screen, dev};
use bevy::prelude::*;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// The two places GitHub answers "which release is newest": the REST
/// API, which says so in JSON with the release notes attached, and the
/// releases page, which redirects to the newest one and says nothing else.
///
/// The API is asked first and is the one that gives the page its notes.
/// It is also rationed: sixty answers an hour to everyone behind one
/// public address, and a hall of players launching the game behind one
/// router is exactly the busy afternoon that runs it dry. When it is dry
/// the page is asked instead, which is not rationed that way, so the
/// offer still stands, only without the notes.
///
/// Both are built from the manifest's repository so a fork asks about
/// itself.
struct Where {
    api: String,
    page: String,
}

fn latest_release_urls() -> Option<Where> {
    let repo = env!("CARGO_PKG_REPOSITORY").strip_prefix("https://github.com/")?;
    Some(Where {
        api: format!("https://api.github.com/repos/{repo}/releases/latest"),
        page: format!("https://github.com/{repo}/releases/latest"),
    })
}

/// How long the check may take, connect to last byte, before the thread
/// gives up. Short: on a slow line the answer is worth less than the
/// bother of a page appearing after the player has settled in, and
/// nothing on the menu waits on it either way.
const TIMEOUT: Duration = Duration::from_secs(3);

/// The release notes block: how many characters across it wraps at, and
/// how many lines it runs to before "…". Sixty-four monospaced characters
/// at the notes' size is about the width the menu's own card runs to.
pub const NOTES_COLS: usize = 64;
pub const NOTES_MAX_LINES: usize = 12;

/// The page's height in UI pixels apart from the notes: the two lines
/// over them, the question and two rows under them, the card's padding
/// and the note beneath the card. What is left of the frame between the
/// bars is the notes' to fill, [`NOTES_LINE_H`] per line.
const PAGE_FIXED_H: f32 = 300.0;
const NOTES_LINE_H: f32 = 15.0;

/// How many note lines fit a frame this many UI pixels tall: the cap,
/// less on a short frame or a big UI scale, never fewer than two, so the
/// question and its answers stay off the header and the prompt.
pub fn notes_budget(frame_h: f32) -> usize {
    let room = ((frame_h - PAGE_FIXED_H) / NOTES_LINE_H).floor();
    if room.is_nan() {
        return 2;
    }
    // A negative room floors to nothing and a boundless one saturates;
    // the clamp answers for both.
    (room.max(0.0) as usize).clamp(2, NOTES_MAX_LINES)
}

/// A release version. Semver underneath, so a pre-release sorts below
/// the release it precedes: a `0.2.0-rc.1` build is told when `0.2.0` is
/// out, and an `rc` tagged latest is offered as the `rc` it is.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Version(semver::Version);

impl Version {
    /// `1.2.3`, `v1.2.3`, `v1.2.3-rc.1`. Anything else is not a version.
    pub fn parse(tag: &str) -> Option<Version> {
        let tag = tag.trim();
        let tag = tag.strip_prefix(['v', 'V']).unwrap_or(tag);
        semver::Version::parse(tag).ok().map(Version)
    }

    /// The version this binary was built as.
    pub fn current() -> Version {
        // The manifest's version is a valid semver or cargo would not
        // have built this; a test says so as well.
        Version::parse(env!("CARGO_PKG_VERSION"))
            .unwrap_or_else(|| Version(semver::Version::new(0, 0, 0)))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// What GitHub said the newest release is.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Release {
    pub version: Version,
    /// The tag as GitHub spells it (`v1.2.0`), for the log and the page.
    pub tag: String,
    /// The release page, for the browser.
    pub url: String,
    /// The release notes, as written: markdown, usually.
    pub notes: String,
}

/// What a release page's address must look like before it is handed to
/// a browser (or, on Windows, to `cmd`): on GitHub, over https, and made
/// of nothing a shell has opinions about. The reply is GitHub's own over
/// TLS, so this is belt to those braces, but the address is the one thing
/// here that leaves the process.
fn is_release_page(url: &str) -> bool {
    url.starts_with("https://github.com/")
        && url.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~' | '/' | ':' | '%' | '+')
        })
}

/// Read a release out of the GitHub API's `releases/latest` reply. `None`
/// for anything that is not one, a tag that is not a version included.
pub fn parse_release(json: &str) -> Option<Release> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let tag = value.get("tag_name")?.as_str()?.trim();
    let version = Version::parse(tag)?;
    let url = value.get("html_url")?.as_str()?.trim();
    if !is_release_page(url) {
        return None;
    }
    let notes = value
        .get("body")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    Some(Release {
        version,
        tag: tag.to_string(),
        url: url.to_string(),
        notes,
    })
}

/// The release notes as the page shows them: the markdown flattened to
/// plain lines, wrapped to [`NOTES_COLS`], and cut at `max_lines` (see
/// [`notes_budget`]) with an ellipsis. Empty notes read as one empty
/// list, and the page says so in words instead.
pub fn notes_lines(notes: &str, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in notes.lines() {
        let line = flatten_markdown(raw);
        // Collapse runs of blank lines, and never open on one.
        if line.is_empty() {
            if lines.last().is_some_and(|last| !last.is_empty()) {
                lines.push(String::new());
            }
            continue;
        }
        for wrapped in wrap(&line, NOTES_COLS) {
            lines.push(wrapped);
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines.saturating_sub(1));
        lines.push("…".to_string());
    }
    lines
}

/// One line of markdown as plain text: headings lose their hashes,
/// bullets become bullets, emphasis and code marks go, links keep their
/// words, HTML tags go (GitHub's own notes open with a comment and like a
/// `<details>`), and a rule is nothing at all.
fn flatten_markdown(raw: &str) -> String {
    let untagged = strip_tags(raw);
    let line = untagged.trim_end();
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    if trimmed.chars().all(|c| c == '-' || c == '*' || c == '=') {
        return String::new();
    }
    let (lead, body) = if let Some(rest) = trimmed.strip_prefix('#') {
        (String::new(), rest.trim_start_matches('#').trim_start())
    } else if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        // Nested bullets keep their indent, halved: two spaces per level
        // is how the source is usually written and four is a lot of card.
        (format!("{}• ", " ".repeat(indent / 2)), rest)
    } else {
        (String::new(), trimmed)
    };
    let mut out = lead;
    let mut rest = body;
    // `[words](url)` keeps the words, `![alt](url)` its alt. Found from
    // the `](` and back to the nearest `[`, so `[#12] and [docs](url)`
    // keeps its first bracket. Anything else with a bracket in it goes
    // through as written.
    while let Some(join) = rest.find("](") {
        let Some(open) = rest[..join].rfind('[') else {
            out.push_str(&rest[..join + 2]);
            rest = &rest[join + 2..];
            continue;
        };
        let Some(close) = rest[join + 2..].find(')') else {
            break;
        };
        out.push_str(rest[..open].strip_suffix('!').unwrap_or(&rest[..open]));
        out.push_str(&rest[open + 1..join]);
        rest = &rest[join + 2 + close + 1..];
    }
    out.push_str(rest);
    out.replace("**", "").replace('`', "")
}

/// A line with its `<tags>` taken out: `<b>`, `</details>`, `<!-- -->`.
///
/// Only what reads as a tag. A closing `</…>` or a comment `<!…>` always
/// is; an opening one is a lowercase name standing on its own - after a
/// space or at the start, never glued to a word - and closed on the line.
/// So `Res<Time>` and `Vec<usize>` keep their arguments (this is a Rust
/// project's release notes), `a < b` keeps its sign, and an autolink
/// `<https://…>` keeps its address.
fn strip_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    // What stood just before `rest` in the line: `>` once a tag has been
    // taken out, so `</h2><br/>` reads as two tags, not one glued word.
    let mut before: Option<char> = None;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let glued = rest[..open]
            .chars()
            .last()
            .or(before)
            .is_some_and(char::is_alphanumeric);
        rest = &rest[open..];
        let Some(close) = rest.find('>') else {
            break;
        };
        let inner = &rest[1..close];
        let name = inner.split([' ', '\t', '/']).next().unwrap_or("");
        let tag_like = inner.starts_with('/')
            || inner.starts_with('!')
            || (!glued
                && !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-' || c.is_ascii_digit()));
        let autolink =
            (inner.starts_with("https://") || inner.starts_with("http://")) && !inner.contains(' ');
        if autolink {
            out.push_str(inner);
        } else if !tag_like {
            out.push('<');
            out.push_str(inner);
            out.push('>');
        }
        before = Some('>');
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

/// Word-wrap one line at `cols` characters, breaking a longer word where
/// it must. Continuation lines carry the bullet's indent.
fn wrap(line: &str, cols: usize) -> Vec<String> {
    let lead: String = line.chars().take_while(|&c| c == ' ' || c == '•').collect();
    let indent = " ".repeat(lead.chars().count());
    let mut out = Vec::new();
    let mut current = lead.clone();
    // Nothing but the lead on this line yet.
    let mut fresh = true;
    for mut word in line[lead.len()..].split(' ') {
        while !word.is_empty() {
            let used = current.chars().count() + usize::from(!fresh);
            if used + word.chars().count() <= cols {
                if !fresh {
                    current.push(' ');
                }
                current.push_str(word);
                fresh = false;
                break;
            }
            if !fresh {
                out.push(std::mem::replace(&mut current, indent.clone()));
                fresh = true;
                continue;
            }
            // Wider than the line on its own: cut it.
            let room = cols.saturating_sub(current.chars().count()).max(1);
            let cut = word.char_indices().nth(room).map_or(word.len(), |(i, _)| i);
            current.push_str(&word[..cut]);
            out.push(std::mem::replace(&mut current, indent.clone()));
            word = &word[cut..];
        }
    }
    if !fresh {
        out.push(current);
    }
    out
}

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

/// Ask GitHub. Blocking; runs on the check thread. `None` for any kind of
/// no: offline, too slow, no release yet, or an answer that is not a
/// release. Each is logged at a level nobody is shown, and none is the
/// player's problem.
fn fetch_latest(urls: &Where) -> Option<Release> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(concat!("pinch-points/", env!("CARGO_PKG_VERSION")))
        // The page's answer *is* its redirect; following it would fetch a
        // release page's worth of HTML to learn what the header said.
        .max_redirects(0)
        .build()
        .new_agent();
    let release = match ask_api(&agent, &urls.api) {
        Ok(release) => release,
        // 403 is how the API says its hourly ration is spent (429 is what
        // it is documented to say). Not fatal: the page knows the tag.
        Err(ureq::Error::StatusCode(403 | 429)) => {
            info!("update check: the API is rationed out; asking the releases page");
            ask_page(&agent, &urls.page)
        }
        Err(e) => {
            info!("update check: {e}");
            None
        }
    };
    match &release {
        Some(release) => info!("update check: newest release is {}", release.tag),
        None => info!("update check: no release to offer"),
    }
    release
}

/// The API's answer, notes and all. `Ok(None)` for a reply that is not a
/// release; the error for a reply that is not a 2xx.
fn ask_api(agent: &ureq::Agent, url: &str) -> Result<Option<Release>, ureq::Error> {
    let body = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()?
        .body_mut()
        .read_to_string()?;
    Ok(parse_release(&body))
}

/// The page's answer: the tag in the redirect, and no notes.
fn ask_page(agent: &ureq::Agent, url: &str) -> Option<Release> {
    let response = match agent.get(url).call() {
        Ok(response) => response,
        Err(e) => {
            info!("update check: {e}");
            return None;
        }
    };
    if !response.status().is_redirection() {
        info!("update check: the releases page did not redirect");
        return None;
    }
    let location = response.headers().get("location")?.to_str().ok()?;
    release_from_redirect(location)
}

/// A release read off where the releases page sends the browser:
/// `.../releases/tag/v1.2.0`. Notes are not to be had this way.
pub fn release_from_redirect(location: &str) -> Option<Release> {
    let location = location.trim();
    let (_, tag) = location.rsplit_once("/releases/tag/")?;
    if !is_release_page(location) || tag.is_empty() || tag.contains('/') {
        return None;
    }
    let version = Version::parse(tag)?;
    Some(Release {
        version,
        tag: tag.to_string(),
        url: location.to_string(),
        notes: String::new(),
    })
}

/// The made-up release `PINCH_UPDATE_DEMO` offers.
fn demo_release() -> Release {
    let mut version = Version::current();
    version.0.minor += 1;
    version.0.patch = 0;
    version.0.pre = semver::Prerelease::EMPTY;
    Release {
        tag: version.to_string(),
        url: format!("{}/releases", env!("CARGO_PKG_REPOSITORY")),
        notes: "## What's new\n\n\
                - A **new tide event**: the undertow, which drags every loose crab one tile seaward.\n\
                - Two more Beach Day challenges.\n\
                - The lobby names the beach that went off the air instead of just dropping it.\n\n\
                ## Fixes\n\n\
                - Pairs mode no longer overflows the settings row in German.\n\
                - See [the changelog](https://example.invalid/changelog) for the rest.\n"
            .to_string(),
        version,
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

/// Hand a URL to the desktop's opener. Only ever given a release page's
/// GitHub `https://` address, checked at parse time.
///
/// `Ok` means the opener started, not that a browser has the page: on a
/// desktop with no handler `xdg-open` starts fine and fails a moment
/// later, and it can also sit for as long as the browser it launched
/// runs. So it is not waited on here; a thread reaps it and logs how it
/// went, and the notice says "handed to", which is what is known.
#[cfg_attr(test, allow(clippy::unnecessary_wraps))]
fn open_url(url: &str) -> std::io::Result<()> {
    #[cfg(test)]
    {
        // Tests must not launch anyone's browser: they only note the ask.
        tests::OPENED.lock().unwrap().push(url.to_string());
        Ok(())
    }
    #[cfg(not(test))]
    open_url_for_real(url)
}

/// The opener itself, kept apart so tests can stand in for it.
#[cfg(not(test))]
fn open_url_for_real(url: &str) -> std::io::Result<()> {
    let mut command = if cfg!(target_os = "windows") {
        // `start` is a cmd built-in; the empty string is the window title
        // it would otherwise take the URL for.
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    std::thread::spawn(move || match child.wait() {
        Ok(status) if status.success() => info!("update check: the release page was handed over"),
        Ok(status) => warn!("update check: the opener gave up: {status}"),
        Err(e) => warn!("update check: could not wait on the opener: {e}"),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every address the page asked a browser for, in order.
    pub(super) static OPENED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

    #[test]
    fn versions_parse_and_order() {
        let v = |s| Version::parse(s);
        assert_eq!(v("v1.2.3"), Some(Version(semver::Version::new(1, 2, 3))));
        assert_eq!(v("1.2.3"), v("v1.2.3"));
        assert_eq!(v(" V1.2.3 "), v("v1.2.3"));
        // A pre-release is a version, below the release it precedes.
        assert!(v("v1.2.3-rc.1").is_some());
        assert!(v("v1.2.3-rc.1") < v("v1.2.3"));
        assert!(v("v1.2.3-rc.1") > v("v1.2.2"));
        assert!(v("v1.2.3+build.7").is_some());
        assert_eq!(v("v1.2"), None);
        assert_eq!(v("v1.2.3.4"), None);
        assert_eq!(v("latest"), None);
        assert_eq!(v(""), None);
        assert_eq!(v("v1.x.3"), None);
        assert!(v("v1.10.0") > v("v1.9.9"));
        assert!(v("v2.0.0") > v("v1.99.99"));
        assert!(v("v0.1.1") > v("v0.1.0"));
        assert_eq!(v("v0.1.0").unwrap().to_string(), "v0.1.0");
        assert_eq!(v("0.2.0-rc.1").unwrap().to_string(), "v0.2.0-rc.1");
    }

    /// The manifest's version parses, or the check compares against
    /// 0.0.0 and offers every release ever made.
    #[test]
    fn this_build_has_a_version() {
        assert_eq!(
            Some(Version::current()),
            Version::parse(env!("CARGO_PKG_VERSION"))
        );
        assert!(Version::current() > Version::parse("0.0.0").unwrap());
    }

    /// The addresses come from the manifest, so a fork asks about itself
    /// rather than about this repository.
    #[test]
    fn the_addresses_name_the_repository() {
        let urls = latest_release_urls().expect("a GitHub repository");
        assert_eq!(
            urls.api,
            "https://api.github.com/repos/agourlay/pinch-points/releases/latest"
        );
        assert_eq!(
            urls.page,
            "https://github.com/agourlay/pinch-points/releases/latest"
        );
    }

    /// The rationed-out fallback: the redirect names the tag, and only
    /// a redirect to a release tag counts.
    #[test]
    fn a_redirect_to_a_tag_is_a_release_without_notes() {
        let release =
            release_from_redirect("https://github.com/agourlay/pinch-points/releases/tag/v0.3.1")
                .expect("a release");
        assert_eq!(release.tag, "v0.3.1");
        assert_eq!(release.version, Version::parse("0.3.1").unwrap());
        assert_eq!(release.notes, "");
        assert!(release.url.ends_with("/releases/tag/v0.3.1"));
        assert_eq!(
            release_from_redirect("https://github.com/agourlay/pinch-points/releases"),
            None
        );
        assert_eq!(
            release_from_redirect("https://github.com/agourlay/pinch-points/releases/tag/"),
            None
        );
        assert_eq!(
            release_from_redirect("https://github.com/x/y/releases/tag/nightly"),
            None
        );
        assert_eq!(
            release_from_redirect("https://evil.example/x/y/releases/tag/v1.0.0"),
            None
        );
        assert_eq!(
            release_from_redirect("http://github.com/x/y/releases/tag/v1.0.0"),
            None
        );
    }

    /// The shape of GitHub's `releases/latest` reply, cut down to the
    /// fields read. Everything else in it is ignored, and a body that is
    /// missing or null is empty notes rather than no release.
    #[test]
    fn a_github_reply_is_read_as_a_release() {
        let json = r#"{
            "url": "https://api.github.com/repos/agourlay/pinch-points/releases/1",
            "html_url": "https://github.com/agourlay/pinch-points/releases/tag/v0.2.0",
            "tag_name": "v0.2.0",
            "name": "Pinch Points 0.2.0",
            "draft": false,
            "prerelease": false,
            "body": "New tide\r\n\r\n- Undertow\r\n",
            "assets": []
        }"#;
        let release = parse_release(json).expect("a release");
        assert_eq!(release.tag, "v0.2.0");
        assert_eq!(release.version, Version::parse("0.2.0").unwrap());
        assert_eq!(
            release.url,
            "https://github.com/agourlay/pinch-points/releases/tag/v0.2.0"
        );
        assert_eq!(release.notes, "New tide\r\n\r\n- Undertow\r\n");

        let no_body = r#"{"tag_name": "v0.2.0", "html_url": "https://github.com/x/y/releases/tag/v0.2.0", "body": null}"#;
        assert_eq!(parse_release(no_body).unwrap().notes, "");
    }

    /// What is not a release: GitHub's 404 body, a tag that is not a
    /// version, a page that is not https, and nonsense.
    #[test]
    fn what_is_not_a_release() {
        assert_eq!(
            parse_release(r#"{"message": "Not Found", "status": "404"}"#),
            None
        );
        assert_eq!(
            parse_release(r#"{"tag_name": "nightly", "html_url": "https://github.com/x/y"}"#),
            None
        );
        assert_eq!(
            parse_release(r#"{"tag_name": "v1.0.0", "html_url": "http://github.com/x/y"}"#),
            None
        );
        // Off GitHub, or carrying something a shell would read: not ours.
        assert_eq!(
            parse_release(r#"{"tag_name": "v1.0.0", "html_url": "https://example.com/x"}"#),
            None
        );
        assert_eq!(
            parse_release(
                r#"{"tag_name": "v1.0.0", "html_url": "https://github.com/x/y/releases/tag/v1.0.0 & calc"}"#
            ),
            None
        );
        assert_eq!(
            parse_release(r#"{"tag_name": "v1.0.0", "html_url": "javascript:alert(1)"}"#),
            None
        );
        assert_eq!(parse_release("<html>rate limited</html>"), None);
        assert_eq!(parse_release(""), None);
    }

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

    /// The notes read as plain lines: headings, bullets, emphasis, links
    /// and rules all come out as words, blank runs collapse, and the
    /// block ends with an ellipsis when there is more.
    #[test]
    fn release_notes_flatten_to_plain_lines() {
        let notes = "<!-- Release notes generated using ... -->\r\n## What's new\r\n\r\n\r\n- A **big** one, see [the docs](https://x/y).\r\n  - nested `code`\r\n\r\n---\r\n<details><summary>more</summary>\r\n**Full Changelog**: https://x/compare/a...b\r\n</details>\r\n\r\n";
        assert_eq!(
            notes_lines(notes, 12),
            vec![
                "What's new",
                "",
                "• A big one, see the docs.",
                " • nested code",
                "",
                "more",
                "Full Changelog: https://x/compare/a...b",
            ]
        );
        // What is not a tag stays: a less-than, a type argument (this is
        // a Rust project's notes), an autolink's address.
        assert_eq!(notes_lines("a < b and c > d", 12), vec!["a < b and c > d"]);
        assert_eq!(
            notes_lines("Res<Time>, Vec<usize> and Option<Vec<u8>>", 12),
            vec!["Res<Time>, Vec<usize> and Option<Vec<u8>>"]
        );
        assert_eq!(notes_lines("<h2>Big</h2><br/>text", 12), vec!["Bigtext"]);
        assert_eq!(
            notes_lines("<b>bold</b> <https://x/y>", 12),
            vec!["bold https://x/y"]
        );
        // Brackets before a link are not the link.
        assert_eq!(
            notes_lines("see [#12] and [the docs](https://x) ![img](https://y)", 12),
            vec!["see [#12] and the docs img"]
        );
        assert_eq!(notes_lines("", 12), Vec::<String>::new());
        assert_eq!(notes_lines("\n\n---\n\n", 12), Vec::<String>::new());

        let long: String = (0..40).map(|i| format!("- line {i}\n")).collect();
        let lines = notes_lines(&long, NOTES_MAX_LINES);
        assert_eq!(lines.len(), NOTES_MAX_LINES);
        assert_eq!(lines.last().map(String::as_str), Some("…"));
        assert_eq!(lines[0], "• line 0");
        // A short frame gets fewer lines, never fewer than two.
        assert_eq!(notes_lines(&long, 4).len(), 4);
        assert_eq!(notes_lines(&long, 2), vec!["• line 0", "…"]);
    }

    /// The notes give way before the question does: a tall frame gets
    /// the full cap, a short one (a small window, a big UI scale) fewer,
    /// and nothing sillier than two.
    #[test]
    fn the_notes_fit_the_frame() {
        assert_eq!(notes_budget(720.0 - 104.0), NOTES_MAX_LINES);
        // 1280x720 at 150% UI scale: 480 tall, 376 between the bars.
        assert!(notes_budget(376.0) < NOTES_MAX_LINES);
        assert!(notes_budget(376.0) >= 2);
        assert_eq!(notes_budget(0.0), 2);
        assert_eq!(notes_budget(f32::NAN), 2);
        assert_eq!(notes_budget(f32::INFINITY), NOTES_MAX_LINES);
    }

    /// Long lines wrap at the column, on spaces where there are any,
    /// continuation lines under a bullet keep its indent, and a word wider
    /// than the card is cut rather than run off it.
    #[test]
    fn long_lines_wrap_to_the_card() {
        let words = "word ".repeat(30);
        for line in notes_lines(&words, 12) {
            assert!(line.chars().count() <= NOTES_COLS, "{line:?}");
            assert!(!line.ends_with(' '));
        }
        let bullet = format!("- {}", "crab ".repeat(20).trim());
        let lines = notes_lines(&bullet, 12);
        assert!(lines.len() >= 2);
        assert!(lines[0].starts_with("• crab"));
        assert!(lines[1].starts_with("  crab"), "{:?}", lines[1]);
        let wall = "x".repeat(150);
        let lines = notes_lines(&wall, 12);
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|l| l.chars().count() <= NOTES_COLS));
        assert_eq!(lines.concat(), wall);
        // Wide characters count as one column each, not one byte.
        let accents = "é".repeat(70);
        let lines = notes_lines(&accents, 12);
        assert_eq!(lines[0].chars().count(), NOTES_COLS);
        assert_eq!(lines.concat(), accents);
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

    /// The demo release is a real one as far as the page is concerned:
    /// newer than this build, and its notes exercise the flattening.
    #[test]
    fn the_demo_release_is_offered() {
        let demo = demo_release();
        assert!(UpdateCheck::worth_offering(&Version::current(), &demo));
        assert!(demo.url.starts_with("https://github.com/"));
        assert!(notes_lines(&demo.notes, 12).len() > 4);
    }
}
