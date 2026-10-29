//! Dev-only environment hooks (`PINCH_*`): shortcuts past the menu,
//! headless screenshots, and campaign autoplay. All inert unless the
//! matching variable is set.
//!
//! Every `PINCH_*` variable the game answers to is read here, either by the
//! [`DevHook`] ladder or by one of the typed accessors below, so this file
//! is the complete inventory of the dev surface.

use crate::app::{Campaign, PendingActions, Phase, Screen, Sim, announce, match_setup, net};
use crate::sim::{Direction, PlayerAction, PlayerId, TideEvent};
use bevy::prelude::*;

/// `PINCH_WINDOW=WxH`: open at a given size instead of the default, so a
/// screen can be checked at the sizes players will actually drag it to.
pub(super) fn window_size() -> Option<(f32, f32)> {
    let raw = std::env::var("PINCH_WINDOW").ok()?;
    let (w, h) = raw.split_once(['x', 'X'])?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// `PINCH_ON_TOP=1`: keep the window above every other, so a screen
/// recording of its corner of the desktop (`ffmpeg -f x11grab`) films the
/// game rather than whatever the window manager left in front of it.
pub(super) fn on_top() -> bool {
    std::env::var("PINCH_ON_TOP").is_ok()
}

/// Whether this run keeps a window whatever the settings say:
/// `PINCH_WINDOW` sized it, or `PINCH_SCREENSHOT` came to photograph it
/// (see [`crate::app::settings::window_mode`]).
pub(super) fn window_pinned() -> bool {
    std::env::var("PINCH_WINDOW").is_ok() || screenshotting()
}

/// `PINCH_LOBBY_HOST=n`: host a LAN lobby unattended and launch once `n`
/// peers are aboard (`=1`, the historical value, launches on the first).
pub(super) fn auto_host_quota() -> Option<usize> {
    std::env::var("PINCH_LOBBY_HOST")
        .ok()
        .map(|quota| quota.parse().unwrap_or(1))
}

/// `PINCH_LOBBY_PARTNERS=S@N`: once `N` rivals are aboard, a host whose
/// table plays a team mode picks the next partners for seat `S` (counted
/// from 1) once, as P and then `S` on its lobby card do: before the
/// unattended launch of `PINCH_LOBBY_HOST=N`, for a match played in the
/// teams picked.
pub(super) fn lobby_partners() -> Option<(u8, usize)> {
    let value = std::env::var("PINCH_LOBBY_PARTNERS").ok()?;
    let (seat, rivals) = value.split_once('@')?;
    Some((seat.parse().ok()?, rivals.parse().ok()?))
}

/// `PINCH_LOBBY_PICK`: a host with somebody aboard presses start once,
/// unattended, which opens the map page for a screenshot of it; and on a
/// series' results card, goes on once, to the page for the next round.
pub(super) fn lobby_pick() -> bool {
    std::env::var("PINCH_LOBBY_PICK").is_ok()
}

/// `PINCH_LOBBY_PICK=go`: and on the map page, at the start and between
/// rounds, picks the card under the cursor a second later, so an
/// unattended match plays on.
pub(super) fn lobby_pick_go() -> bool {
    std::env::var("PINCH_LOBBY_PICK").is_ok_and(|value| value == "go")
}

/// `PINCH_LOBBY_JOIN`: join the first LAN host the lobby hears, unattended.
pub(super) fn auto_join() -> bool {
    std::env::var("PINCH_LOBBY_JOIN").is_ok()
}

/// `PINCH_LOBBY_JOIN_BOT`: join the first LAN host the lobby hears as a
/// bot (route 1), opening the doorway for it unattended.
pub(super) fn auto_join_bot() -> bool {
    std::env::var("PINCH_LOBBY_JOIN_BOT").is_ok()
}

/// `PINCH_LOBBY_WATCH`: the same, but as a spectator.
pub(super) fn auto_watch() -> bool {
    std::env::var("PINCH_LOBBY_WATCH").is_ok()
}

/// `PINCH_HOST=port`: host a direct online session, no lobby.
pub(super) fn direct_host() -> Option<String> {
    std::env::var("PINCH_HOST").ok()
}

/// `PINCH_JOIN=ip:port`: the joining half of the direct pair.
pub(super) fn direct_join() -> Option<String> {
    std::env::var("PINCH_JOIN").ok()
}

/// `PINCH_BOT_SEATS=n`: the top n of the AI's seats are left to bots
/// instead. Read by the match-setup hook (`PINCH_MATCH`) alone.
pub(super) fn bot_seats() -> Option<u8> {
    std::env::var("PINCH_BOT_SEATS").ok()?.parse().ok()
}

/// `PINCH_LAUNCH`: start the match on the setup screen as if Enter had
/// been pressed, opening the doorway for any bot seats.
pub(crate) fn launch() -> bool {
    std::env::var("PINCH_LAUNCH").is_ok()
}

/// `PINCH_BOT_KEY=KEY`: the doorway's first chair takes this key rather
/// than a fresh one, so a script can start its bot with a string it knew
/// in advance. Never for a doorway on the LAN: the key is the lock.
pub(crate) fn bot_key() -> Option<crate::bots::connstr::Key> {
    crate::bots::connstr::Key::parse(&std::env::var("PINCH_BOT_KEY").ok()?)
}

/// `PINCH_BOTS=n`: n AI players in a hosted lobby match (and in the direct
/// `PINCH_HOST`/`PINCH_JOIN` pair, where both sides must set it because
/// there is no lobby to agree it for them).
pub(super) fn bots() -> Option<u8> {
    std::env::var("PINCH_BOTS").ok()?.parse().ok()
}

/// `PINCH_THREADS=n`: the cap on Bevy's compute pool, for sweeping it on a
/// machine before deciding what the shipped cap should be. `0` lifts the cap
/// back to Bevy's own default, which is every core the io and async pools
/// did not take.
pub(super) fn compute_threads() -> Option<usize> {
    let n: usize = std::env::var("PINCH_THREADS").ok()?.parse().ok()?;
    Some(if n == 0 { usize::MAX } else { n })
}

/// `PINCH_ALL_PLUGINS`: keep every one of Bevy's default plugins, the
/// ones this game draws nothing with included (see
/// `schedule::trim_unused_plugins`), for measuring what they cost.
pub(super) fn all_plugins() -> bool {
    std::env::var("PINCH_ALL_PLUGINS").is_ok()
}

/// `PINCH_SEATS=n`: how many seats a skirmish sets the table for.
fn seats() -> Option<u8> {
    std::env::var("PINCH_SEATS").ok()?.parse().ok()
}

/// `PINCH_COOP`: the Tide Pool starts in co-op, two cursors on one pool of
/// arrows, so the second pair of hands can be shot without a C pressed.
pub(super) fn coop() -> bool {
    std::env::var("PINCH_COOP").is_ok()
}

/// `PINCH_SANDBOX`: a local arena with preloaded castles.
pub(super) fn sandbox() -> bool {
    std::env::var("PINCH_SANDBOX").is_ok()
}

/// `PINCH_MT_EXEC`: every main-world schedule back on Bevy's parallel
/// executor, for measuring it against the single-threaded one the game
/// runs on (see `schedule::run_with`).
pub(super) fn multi_threaded_executor() -> bool {
    std::env::var("PINCH_MT_EXEC").is_ok()
}

/// `PINCH_SHUFFLE=<seed>`: put every main-world schedule's unordered systems
/// in a shuffled order, seeded, so an ordering bug shows itself instead of
/// hiding behind whichever order the graph happens to settle on. Only in a
/// build with the `shuffle` feature (`cargo run --features shuffle`). The
/// game's single-threaded executor runs one fixed order per seed; with
/// `PINCH_MT_EXEC=1` the parallel one starts whatever system is ready
/// first.
///
/// The bug class it is for is the one that kept recurring here: two
/// systems reading the same key in the same frame with no order between
/// them (a chat line's Enter also leaving the results card, the pause
/// card's Enter also calling the next round). Each such bug was invisible
/// until the arbitrary order happened to go the wrong way.
#[cfg(feature = "shuffle")]
pub(super) fn shuffle_seed() -> Option<u64> {
    let seed = std::env::var("PINCH_SHUFFLE").ok()?;
    // Any word will do as a seed; a number is used as itself so a run can
    // be repeated from what its log line said.
    Some(seed.parse().unwrap_or_else(|_| {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        seed.hash(&mut hasher);
        hasher.finish()
    }))
}

/// `PINCH_NO_UPDATE`: never ask GitHub for a newer release this run, for
/// scripted launches and screenshots, and for the machine with no network
/// that should not spend eight seconds finding that out.
pub(super) fn no_update_check() -> bool {
    std::env::var("PINCH_NO_UPDATE").is_ok()
}

/// `PINCH_PAD_PROMPTS`: start as if a pad had just been pressed, so the
/// controller's prompts are up from the first frame, for a screenshot of
/// them without a pad to press.
pub(crate) fn pad_prompts() -> bool {
    std::env::var("PINCH_PAD_PROMPTS").is_ok()
}

/// `PINCH_PAD_PROMPTS=playstation|dualsense|nintendo`: those prompts with
/// that maker's button names, as a pad of theirs would have them. Any
/// other value is an Xbox pad's.
pub(crate) fn pad_family() -> crate::app::gamepad::PadFamily {
    use crate::app::gamepad::PadFamily;
    match std::env::var("PINCH_PAD_PROMPTS").as_deref() {
        Ok("playstation") => PadFamily::PlayStation,
        Ok("dualsense") => PadFamily::DualSense,
        Ok("nintendo") => PadFamily::Nintendo,
        _ => PadFamily::Xbox,
    }
}

/// `PINCH_QUIT_CARD`: open the main menu with its quit card up, as a
/// pad's Start raises it, for a screenshot of the card: its keyboard keys,
/// or with `PINCH_PAD_PROMPTS` its pad buttons.
pub(crate) fn quit_card() -> bool {
    std::env::var("PINCH_QUIT_CARD").is_ok()
}

/// `PINCH_PADS=<n>`: that many controllers plugged in from the start,
/// pads with nobody's hands on them, for screenshots of how the seats are
/// dealt without a pad to plug in. With `PINCH_PAD_PROMPTS` the first is
/// the pad the hands last used, the one that works the setup.
pub(crate) fn virtual_pads() -> usize {
    std::env::var("PINCH_PADS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
        .min(crate::sim::MAX_PLAYERS)
}

/// `PINCH_EDITOR_LEVEL=<path>`: open the editor with the level in that
/// file on the bench, as a pasted code would put it there, for a picture
/// of the editor over a level of its own.
pub fn editor_level() -> Option<std::path::PathBuf> {
    std::env::var_os("PINCH_EDITOR_LEVEL").map(std::path::PathBuf::from)
}

/// `PINCH_UPDATE_DEMO`: offer a made-up newer release on the menu without
/// asking GitHub, so the update card can be looked at before there is a
/// release to look at (and, once there is, whenever the wording changes).
pub(super) fn update_demo() -> bool {
    std::env::var("PINCH_UPDATE_DEMO").is_ok()
}

/// A shortcut past the menu, as asked for by the environment.
///
/// Read by one pure function over a variable lookup, so the precedence
/// (which hook wins when two are set) is explicit and testable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum DevHook {
    /// `PINCH_LOBBY_HOST` / `PINCH_LOBBY_JOIN`: the LAN lobby screen, and
    /// `PINCH_LOBBY_BROWSE` for its list of games with nothing pressed.
    Lobby,
    /// `PINCH_HOST` / `PINCH_JOIN`: straight into an online round.
    Online,
    /// `PINCH_SANDBOX`: a local arena with preloaded castles.
    Sandbox,
    /// `PINCH_AUTOPLAY=<level>`: the puzzle campaign from that level (1 is
    /// the first), playing its own authored solutions - or, with
    /// `PINCH_NOSOLVE=1`, playing none of them, so a timed level runs out.
    Autoplay {
        level: usize,
    },
    Editor,
    Settings,
    Controls,
    Achievements,
    /// `PINCH_LANGUAGE`: the language picker.
    Language,
    /// `PINCH_STAGES`: the campaign's stage list.
    StageSelect,
    /// `PINCH_SKIRMISH=classic|arena|<map>`: `classic` the generated 12x9,
    /// `arena` the handmade arena, else any map by its `--map` name
    /// ([`skirmish_map`]); plus `PINCH_SERIES=3|5` for a series: four
    /// seats, three of them AI. Any unrecognised value is the generated
    /// beach at the classic size.
    Skirmish {
        map: match_setup::MapChoice,
        series: crate::app::tournament::SeriesLength,
    },
    /// `PINCH_MATCH=1|2|3`: match setup, on that step (the first for any
    /// other value). `PINCH_MATCH_MAP=small|arena|...|random` picks step
    /// 2's card first.
    MatchSetup {
        step: match_setup::Step,
    },
    /// `PINCH_REPLAY`: watch the last saved round.
    Replay,
    /// `PINCH_LIBRARY`: the kept rounds.
    Replays,
}

impl DevHook {
    /// The hook the environment asks for, in precedence order. `var` is
    /// injected so the ladder can be tested without touching the real
    /// process environment.
    pub(super) fn from_env(var: impl Fn(&str) -> Option<String>) -> Option<DevHook> {
        let set = |name: &str| var(name).is_some();
        if set("PINCH_LOBBY_HOST")
            || set("PINCH_LOBBY_JOIN")
            || set("PINCH_LOBBY_JOIN_BOT")
            || set("PINCH_LOBBY_WATCH")
            || set("PINCH_LOBBY_BROWSE")
        {
            return Some(DevHook::Lobby);
        }
        if set("PINCH_HOST") || set("PINCH_JOIN") {
            return Some(DevHook::Online);
        }
        if set("PINCH_SANDBOX") {
            return Some(DevHook::Sandbox);
        }
        if let Some(level) = var("PINCH_AUTOPLAY") {
            return Some(DevHook::Autoplay {
                // `=1` has always meant "on", and level 1 is what it did.
                level: level.parse::<usize>().unwrap_or(1).max(1) - 1,
            });
        }
        for (name, hook) in [
            ("PINCH_EDITOR", DevHook::Editor),
            ("PINCH_EDITOR_LEVEL", DevHook::Editor),
            ("PINCH_SETTINGS", DevHook::Settings),
            ("PINCH_CONTROLS", DevHook::Controls),
            ("PINCH_ACHIEVEMENTS", DevHook::Achievements),
            ("PINCH_LANGUAGE", DevHook::Language),
            ("PINCH_LIBRARY", DevHook::Replays),
            ("PINCH_STAGES", DevHook::StageSelect),
        ] {
            if set(name) {
                return Some(hook);
            }
        }
        if let Some(size) = var("PINCH_SKIRMISH") {
            return Some(DevHook::Skirmish {
                map: skirmish_map("PINCH_SKIRMISH", &size),
                // Bare `PINCH_SERIES=1` still means the long series it
                // always meant; `=3` asks for the short one.
                series: match var("PINCH_SERIES").as_deref() {
                    None => crate::app::tournament::SeriesLength::Single,
                    Some("3") => crate::app::tournament::SeriesLength::BestOfThree,
                    Some(_) => crate::app::tournament::SeriesLength::BestOfFive,
                },
            });
        }
        if let Some(step) = var("PINCH_MATCH") {
            return Some(DevHook::MatchSetup {
                step: match step.as_str() {
                    "2" => match_setup::Step::Map,
                    "3" => match_setup::Step::Players,
                    _ => match_setup::Step::Game,
                },
            });
        }
        set("PINCH_REPLAY").then_some(DevHook::Replay)
    }
}

/// Act on the environment's shortcut, if it asked for one.
pub(super) fn kickoff(
    mut campaign: ResMut<Campaign>,
    mut online: ResMut<net::Online>,
    mut config: ResMut<match_setup::MatchConfig>,
    mut tournament: ResMut<crate::app::tournament::Tournament>,
    mut playback: ResMut<crate::app::Playback>,
    mut next_screen: ResMut<NextState<Screen>>,
    mut setup: ResMut<match_setup::MatchMenu>,
) {
    let Some(hook) = DevHook::from_env(|name| std::env::var(name).ok()) else {
        return;
    };
    match hook {
        DevHook::Lobby => {
            // The lobby sends the host's match terms, so the same hooks that
            // configure a local skirmish configure a hosted one.
            config.map = debug_map().unwrap_or(config.map);
            config.lobby_bots = bots().unwrap_or(config.lobby_bots);
            // `PINCH_SERIES` hosts a series, as it plays one locally.
            match std::env::var("PINCH_SERIES").as_deref() {
                Ok("3") => config.series = crate::app::tournament::SeriesLength::BestOfThree,
                Ok(_) => config.series = crate::app::tournament::SeriesLength::BestOfFive,
                Err(_) => {}
            }
            next_screen.set(Screen::Lobby);
        }
        DevHook::Online => {
            // Binding the socket is the side effect the enum cannot carry;
            // a failure here just leaves the player on the menu.
            if let Some(session) = net::session_from_env() {
                online.0 = Some(session);
                next_screen.set(Screen::Versus);
            }
        }
        DevHook::Sandbox => {
            next_screen.set(Screen::Versus);
        }
        DevHook::Autoplay { level } => {
            campaign.index = level.min(campaign.levels.len() - 1);
            next_screen.set(Screen::Puzzle);
        }
        DevHook::Editor => next_screen.set(Screen::Editor),
        DevHook::Settings => next_screen.set(Screen::Settings),
        DevHook::Controls => next_screen.set(Screen::Controls),
        DevHook::Achievements => next_screen.set(Screen::Achievements),
        DevHook::Language => next_screen.set(Screen::Language),
        DevHook::Replays => next_screen.set(Screen::Replays),
        DevHook::StageSelect => next_screen.set(Screen::StageSelect),
        DevHook::Skirmish { map, series } => {
            let seats = seats().unwrap_or(4);
            // A hand-typed PINCH_BOTS is not trusted to leave a human seat:
            // `with_ai` keeps one.
            let table = match_setup::MatchConfig::with_ai(seats, bots().unwrap_or(seats - 1));
            config.seats = table.seats;
            config.controllers = table.controllers;
            config.map = map;
            config.series = series;
            if series.is_series() {
                *tournament = crate::app::tournament::Tournament::start(series);
            }
            config.armed = true;
            next_screen.set(Screen::Versus);
        }
        DevHook::MatchSetup { step } => {
            // `PINCH_SEATS`/`PINCH_BOTS` apply here too, so the steps can
            // be seen with a full table rather than only the default pair:
            // the top PINCH_BOTS seats to the AI, the top PINCH_BOT_SEATS
            // of those to bots.
            let seats = seats().unwrap_or(config.seats);
            let ai = bots().unwrap_or(0);
            let table = match_setup::MatchConfig::with_ai(seats, ai);
            config.seats = table.seats;
            config.controllers = table.controllers;
            for slot in 0..bot_seats().unwrap_or(0).min(table.ai_seats()) {
                let seat = usize::from(config.seats - 1 - slot);
                config.controllers[seat] = crate::app::SeatController::Bot;
            }
            match std::env::var("PINCH_MATCH_MAP").ok().as_deref() {
                Some("random") => config.random = true,
                Some(name) => {
                    config.map = skirmish_map("PINCH_MATCH_MAP", name);
                    config.random = false;
                    match_setup::pick_the_map_in_force(&mut config);
                }
                None => {}
            }
            setup.step = step;
            next_screen.set(Screen::MatchSetup);
        }
        DevHook::Replay => {
            match std::fs::read_to_string(crate::app::replay_path())
                .map_err(|e| e.to_string())
                .and_then(|t| crate::sim::Replay::parse(&t))
            {
                Ok(replay) => {
                    playback.0 = Some((replay, 0));
                    next_screen.set(Screen::Versus);
                }
                Err(e) => warn!("PINCH_REPLAY: no replay to watch: {e}"),
            }
        }
    }
}

/// Dev hook: `PINCH_SKIRMISH=classic|arena|<map>` ([`skirmish_map`]) also
/// names the map for a hosted lobby match.
fn debug_map() -> Option<match_setup::MapChoice> {
    Some(skirmish_map(
        "PINCH_SKIRMISH",
        &std::env::var("PINCH_SKIRMISH").ok()?,
    ))
}

/// The map a `PINCH_SKIRMISH` value names, for a bots match and a hosted
/// lobby alike. `classic` is the generated beach at the classic size, as
/// it always was for these hooks (the bots' `--map` names that one
/// `generated` and the handmade arena `classic`), and `arena` the handmade
/// one a new player's Versus starts on. A name no map has falls back to
/// `classic` and says so in the log under `var`, the hook it came from,
/// rather than quietly playing a beach nobody asked for.
fn skirmish_map(var: &str, value: &str) -> match_setup::MapChoice {
    // `custom` takes the first handmade beach with castles enough, which
    // is how the wire path gets exercised without a menu; every other map
    // goes by the name the bots' `--map` gives it.
    match value {
        "classic" => match_setup::MapChoice::GenClassic,
        "arena" => match_setup::MapChoice::Classic,
        name => match_setup::MapChoice::from_token(name).unwrap_or_else(|| {
            warn!("{var}: no map called {name:?}, playing the classic generated beach");
            match_setup::MapChoice::GenClassic
        }),
    }
}

/// Whether a hook that reaches straight into the local board has to stand
/// down, which in an online round it does.
///
/// Peers agree by running the same frames from the same inputs. These two
/// hooks run on one machine and put something on one board, so the very
/// next hash is a desync: a real one, reported honestly, a few seconds
/// into a round that was started to watch something else entirely. It
/// looks exactly like the netcode having gone wrong, and it costs
/// somebody the time to prove it has not. `PINCH_SPECTATOR` already
/// refuses to echo a line the table was not given, for the same reason.
///
/// The online way to call an event down on a table is `PINCH_SPECTATOR`
/// with an event number: it goes through the vote the host counts and
/// comes back as an action on a frame, which every peer runs.
fn only_off_the_wire(online: &net::Online, var: &str) -> bool {
    if online.0.is_some() {
        warn!(
            "{var}: ignored in an online round, where one peer's board is \
             not its own to change; use PINCH_SPECTATOR=<event> instead"
        );
        return true;
    }
    false
}

/// Dev hook: `PINCH_TIDE=<0-8>` fires a real tide event a few seconds in
/// (or `PINCH_TIDE_AT` seconds in),
/// rather than the banner alone, so what the event *does* can be watched.
/// Seven is the castle swap. With `PINCH_TIDE_BY=<seat>` it lands as that
/// seat's spin, as a sparkling crab banked there would, rather than as a
/// call nobody spun. A list (`PINCH_TIDE=4,0`) fires each in turn three
/// seconds apart, the first as that seat's spin and the rest as calls, the
/// way a spectators' call lands on a running event.
pub(super) fn debug_tide(
    mut sim: ResMut<Sim>,
    online: Res<net::Online>,
    mut hook: Local<OneShot>,
    mut queued: Local<std::collections::VecDeque<(u64, TideEvent)>>,
    mut fired: Local<bool>,
) {
    if let Some(which) = hook.due("PINCH_TIDE", sim.0.ticks()) {
        if only_off_the_wire(&online, "PINCH_TIDE") {
            return;
        }
        let gap = 3 * u64::from(crate::sim::TICKS_PER_SECOND);
        let now = sim.0.ticks();
        *queued = which
            .split(',')
            .enumerate()
            .map(|(n, index)| {
                let event = TideEvent::ALL
                    [index.trim().parse::<usize>().unwrap_or(0) % TideEvent::ALL.len()];
                (now + gap * n as u64, event)
            })
            .collect();
    }
    let Some(&(at, event)) = queued.front() else {
        return;
    };
    if sim.0.ticks() < at {
        return;
    }
    queued.pop_front();
    let first = !std::mem::replace(&mut *fired, true);
    let spinner = std::env::var("PINCH_TIDE_BY")
        .ok()
        .and_then(|seat| seat.parse::<PlayerId>().ok());
    match spinner {
        Some(seat) if first => sim.0.force_spun_tide_event(event, seat),
        Some(_) | None => sim.0.force_tide_event(event, 0),
    }
}

/// The mid-round moments a screenshot can be taken of, each fired once.
#[derive(Default)]
pub(super) struct MomentHooks {
    pause: OneShot,
    over: OneShot,
    interlude: OneShot,
}

/// A hook that reads its variable once, waits for the round to have
/// something on the beach worth looking at, and then fires exactly once.
///
/// Seven hooks share it: `PINCH_TIDE`, `PINCH_LURE`, `PINCH_SPECTATOR`,
/// `PINCH_BANNER`, and the three [`MomentHooks`]. Each reads the
/// environment into a `Local` so it is not re-read every frame, waits
/// until the round has run a few seconds (or its `_AT` override), and
/// fires once.
#[derive(Default)]
pub(super) struct OneShot {
    /// The variable and its `_AT` override, read on the first frame.
    setting: Option<(Option<String>, Option<u64>)>,
    fired: bool,
}

impl OneShot {
    /// A few seconds in: long enough for crabs to be out and moving, so
    /// whatever the hook does happens over a board with something on it.
    const WAIT: u64 = 4;

    /// What `var` was set to, the one time this is due, and `None` on every
    /// other frame and for every unset variable.
    fn due(&mut self, var: &str, ticks: u64) -> Option<String> {
        self.due_after(var, ticks, Self::WAIT)
    }

    /// The same, for a hook that wants the round further along than most.
    ///
    /// `<var>_AT=<s>` moves any hook to that many seconds into the round,
    /// for a moment wanted on a beach that has filled up: a lure or a
    /// Crab Mania four seconds in has hardly a crab to show.
    fn due_after(&mut self, var: &str, ticks: u64, seconds: u64) -> Option<String> {
        let (setting, at) = self.setting.get_or_insert_with(|| {
            let at = std::env::var(format!("{var}_AT"))
                .ok()
                .and_then(|s| s.parse().ok());
            (std::env::var(var).ok(), at)
        });
        let setting = setting.clone()?;
        let seconds = at.unwrap_or(seconds);
        if self.fired || ticks < seconds * u64::from(crate::sim::TICKS_PER_SECOND) {
            return None;
        }
        self.fired = true;
        Some(setting)
    }
}

/// Dev hook: `PINCH_LURE=<seat>` starts a lure a few seconds in (or
/// `PINCH_LURE_AT` seconds in), which is otherwise something you wait for
/// a molting crab to do.
pub(super) fn debug_lure(mut sim: ResMut<Sim>, online: Res<net::Online>, mut hook: Local<OneShot>) {
    let Some(which) = hook.due("PINCH_LURE", sim.0.ticks()) else {
        return;
    };
    if only_off_the_wire(&online, "PINCH_LURE") {
        return;
    }
    let seat = which.parse::<u8>().unwrap_or(0);
    sim.0
        .force_lure(seat.min(crate::sim::MAX_PLAYERS as u8 - 1));
}

/// Dev hook: `PINCH_SPECTATOR=card|say|pick<n>|<0-6>` works the spectator keys for a watcher that
/// has no hands: `card` opens the event list for a screenshot, and a
/// number casts that vote a few seconds in, which is otherwise a thing
/// only a person standing behind a chair can do.
///
/// The whole point of it is the chain it exercises: a vote leaves the
/// watcher, the host counts it, settles on it, and puts it into an action
/// on a frame, and the event fires on every screen at once.
pub(super) fn debug_spectator(
    mut commands: Commands,
    sim: Res<Sim>,
    settings: Res<crate::app::settings::GameSettings>,
    mut card: ResMut<crate::app::spectators::SpectatorCard>,
    mut online: ResMut<net::Online>,
    mut hook: Local<OneShot>,
) {
    let Some(which) = hook.due("PINCH_SPECTATOR", sim.0.ticks()) else {
        return;
    };
    if !crate::app::spectators::is_spectating(&online) {
        return;
    }
    if which == "say" {
        // A line from a spoke, which is the path the hub has to repeat:
        // what a spectator says reaches the table only by the host passing
        // it on, and for a while it did not.
        // The same refusal the real path makes: the host drops a line
        // that resolves to no name, so echoing one here would show this
        // screen something nobody else was given. The hook had the bug it
        // was used to find, which is a poor thing for a hook to have.
        if let Some(session) = &mut online.0
            && let me = crate::app::lobby::speaking_as(session.bot.as_deref(), &settings.names[0])
                .to_string()
            && !me.is_empty()
        {
            let line = "hello from the back row";
            session
                .transport
                .send(crate::transport::NetMsg::chat(&me, line));
            session.heard.push((me, line.to_string()));
        }
        return;
    }
    if which == "card" {
        card.0 = true;
        crate::app::spectators::spawn_card(&mut commands, &settings);
        return;
    }
    // `pick<n>` calls seat n (1-based, as the list numbers them) to win,
    // through the same send the pick card makes.
    if let Some(seat) = which
        .strip_prefix("pick")
        .and_then(|n| n.parse::<u8>().ok())
    {
        if let Some(session) = &mut online.0 {
            let seat = seat.saturating_sub(1);
            session.stands.winner.my_pick = Some(seat);
            session
                .transport
                .send(crate::transport::NetMsg::SpectatorPick { seat });
        }
        return;
    }
    let at = which.parse::<usize>().unwrap_or(0);
    let event = crate::app::spectators::SPECTATOR_EVENTS
        [at % crate::app::spectators::SPECTATOR_EVENTS.len()];
    if let Some(session) = &mut online.0 {
        session
            .transport
            .send(crate::transport::NetMsg::SpectatorVote {
                event: event.index() as u8,
            });
    }
}

/// Dev hook: `PINCH_BANNER=lure|surge|<0-8>` raises one centre-screen
/// announcement a few seconds into the round, so the banner can be
/// screenshotted over a board with something on it, rather than waiting for
/// a sparkling crab to be banked, which may not happen for minutes.
pub(super) fn debug_banner(
    sim: Res<Sim>,
    mut announcer: ResMut<announce::Announcer>,
    mut hook: Local<OneShot>,
) {
    // Later than the others on purpose: a banner wants a board with crabs
    // spread over it behind the words.
    let Some(which) = hook.due_after("PINCH_BANNER", sim.0.ticks(), 8) else {
        return;
    };
    announcer.push(match which.as_str() {
        "lure" => announce::Announcement::Lure(0),
        "surge" => announce::Announcement::Surge,
        index => announce::Announcement::Tide {
            event: TideEvent::ALL[index.parse::<usize>().unwrap_or(0) % TideEvent::ALL.len()],
            by: None,
            keeper: None,
        },
    });
}

/// Whether this run exists to take a picture.
///
/// The visual harness reads probe pixels off the result, and the camera
/// shake (`boot::shake_camera`) moves every one of them. A shot run holds
/// the camera still.
pub(super) fn screenshotting() -> bool {
    std::env::var("PINCH_SCREENSHOT").is_ok()
}

/// Dev hook: with `PINCH_SCREENSHOT=/path/out.png`, saves a frame after
/// `PINCH_SCREENSHOT_AT` seconds (default 2) and exits shortly after. Used to
/// verify rendering headlessly; harmless otherwise.
///
/// `PINCH_SCREENSHOT_TICK=<n>` waits for the board's clock to reach tick
/// `n` instead, which on a deterministic beach is the same moment of play
/// every run, and `PINCH_SCREENSHOT_BURST=<count>` takes that many, one
/// every `PINCH_SCREENSHOT_EVERY` ticks (default 3), as `out_00.png`,
/// `out_01.png` and on: a short animation, read frame by frame.
pub(super) fn debug_screenshot(
    mut commands: Commands,
    time: Res<Time>,
    sim: Res<Sim>,
    mut taken: Local<u32>,
    mut done_at: Local<Option<f32>>,
    mut config: Local<Option<Option<ShotPlan>>>,
    mut exit: MessageWriter<AppExit>,
) {
    // Env lookups allocate; resolve them once, not every frame.
    let config = config.get_or_insert_with(ShotPlan::from_env);
    let Some(plan) = config else {
        return;
    };
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    if let Some(done) = *done_at {
        if time.elapsed_secs() > done + 1.5 {
            exit.write(AppExit::Success);
        }
        return;
    }
    let due = match plan.tick {
        Some(tick) => sim.0.ticks() >= tick + u64::from(*taken * plan.every),
        None => time.elapsed_secs() > plan.at,
    };
    if !due {
        return;
    }
    let path = match plan.burst {
        1 => plan.path.clone(),
        _ => {
            let stem = plan.path.strip_suffix(".png").unwrap_or(&plan.path);
            format!("{stem}_{:02}.png", *taken)
        }
    };
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
    *taken += 1;
    if *taken >= plan.burst {
        *done_at = Some(time.elapsed_secs());
    }
}

/// Dev hook: `PINCH_SLOWMO=<speed>` runs the whole game at that fraction
/// of real time, the round's clock and every animation with it. With a
/// screenshot burst it films a moment a few frames apart where a burst at
/// full speed, each shot costing a fraction of a second, would catch it
/// once.
pub(super) fn slow_motion(mut time: ResMut<Time<Virtual>>) {
    if let Some(speed) = std::env::var("PINCH_SLOWMO")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|s| *s > 0.0)
    {
        time.set_relative_speed(speed);
    }
}

/// What [`debug_screenshot`] was asked to take.
pub(super) struct ShotPlan {
    path: String,
    at: f32,
    tick: Option<u64>,
    burst: u32,
    every: u32,
}

impl ShotPlan {
    fn from_env() -> Option<ShotPlan> {
        fn num<T: std::str::FromStr>(key: &str) -> Option<T> {
            std::env::var(key).ok()?.parse().ok()
        }
        Some(ShotPlan {
            path: std::env::var("PINCH_SCREENSHOT").ok()?,
            at: num("PINCH_SCREENSHOT_AT").unwrap_or(2.0),
            tick: num("PINCH_SCREENSHOT_TICK"),
            burst: num("PINCH_SCREENSHOT_BURST").unwrap_or(1).max(1),
            every: num("PINCH_SCREENSHOT_EVERY").unwrap_or(3).max(1),
        })
    }
}

/// Dev hook: with `PINCH_FRAMES=1`, every five seconds print the frames'
/// average, 99th percentile and worst time to stderr. For judging a
/// screen's cost from its frame times rather than its CPU, which a vsynced
/// frame hides: a build that spends twice as long per frame still shows
/// the same frame rate until it misses one.
pub(super) fn frame_times(
    time: Res<Time<Real>>,
    mut on: Local<Option<bool>>,
    mut frames: Local<Vec<f32>>,
    mut since: Local<f32>,
) {
    if !*on.get_or_insert_with(|| std::env::var("PINCH_FRAMES").is_ok()) {
        return;
    }
    let dt = time.delta_secs() * 1000.0;
    if dt > 20.0 {
        eprintln!("slow frame at {:.3}s: {dt:.1} ms", time.elapsed_secs());
    }
    frames.push(dt);
    *since += dt;
    if *since < 5000.0 {
        return;
    }
    let mut sorted = std::mem::take(&mut *frames);
    sorted.sort_by(f32::total_cmp);
    let n = sorted.len();
    let mean = sorted.iter().sum::<f32>() / n as f32;
    eprintln!(
        "frames: {n} in {:.1}s, mean {mean:.2} ms, p99 {:.2} ms, worst {:.2} ms",
        *since / 1000.0,
        sorted[(n * 99 / 100).min(n - 1)],
        sorted[n - 1],
    );
    *since = 0.0;
}

/// Dev hook: `PINCH_LIBRARY=<n>` opens the replay shelf with the cursor on
/// its `n`th round (from 1, so the historical `=1` is the top), and the
/// window scrolled to it as a walk down would leave it, so the middle and
/// the foot of a long shelf can be shot.
pub(super) fn debug_library_cursor(mut library: ResMut<crate::app::replays::Library>) {
    let Some(round) = std::env::var("PINCH_LIBRARY")
        .ok()
        .and_then(|n| n.parse::<usize>().ok())
    else {
        return;
    };
    library.selected = round.saturating_sub(1);
    library.settle();
}

/// Dev hook: `PINCH_LOBBY_BROWSE=<n>` holds the join list's cursor on its
/// `n`th beach (from 1, so the historical `=1` is the top) once that many
/// are on the air, the window scrolled to it as a walk down would leave
/// it, so the foot of a long list can be shot.
pub(super) fn debug_lobby_cursor(mut state: ResMut<crate::app::lobby::LobbyState>) {
    let Some(beach) = std::env::var("PINCH_LOBBY_BROWSE")
        .ok()
        .and_then(|n| n.parse::<usize>().ok())
        .and_then(|n| n.checked_sub(1))
    else {
        return;
    };
    if beach < state.hosts.len() && state.selected_index() != Some(beach) {
        state.point_at(Some(beach));
    }
}

/// Dev hook: `PINCH_DELETE=1` presses the replay shelf's delete once, a
/// second after the shelf opens, so the question it asks can be shot.
/// Pressed and let go at once, as [`debug_moments`] presses its Escape.
pub(super) fn debug_delete(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    time: Res<Time>,
    mut opened: Local<Option<f32>>,
    mut fired: Local<bool>,
) {
    if *fired || std::env::var("PINCH_DELETE").is_err() {
        return;
    }
    let now = time.elapsed_secs();
    if now - *opened.get_or_insert(now) < 1.0 {
        return;
    }
    *fired = true;
    if let Some(key) = caps.key_for(crate::app::replays::DELETE_CAP) {
        keys.press(key);
        keys.release(key);
    }
}

/// Dev hook: `PINCH_EDITOR_PRESS=F2,F2` presses those keys in the editor,
/// one a second, the first a second after it opens: a picture of what a
/// key says (F2's question over another level's file, the line after a
/// save) without a hand at the keyboard. Keys by Bevy's names, the
/// function keys as `F1`..`F12`, and `Shift+` in front of one holds Shift
/// with it until the next press (`Shift+F1`, the card of every key).
pub(super) fn debug_editor_press(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut opened: Local<Option<f32>>,
    mut pressed: Local<usize>,
) {
    let Ok(list) = std::env::var("PINCH_EDITOR_PRESS") else {
        return;
    };
    let now = time.elapsed_secs();
    let since = now - *opened.get_or_insert(now);
    let Some(name) = list.split(',').nth(*pressed) else {
        return;
    };
    if since < (*pressed + 1) as f32 {
        return;
    }
    *pressed += 1;
    keys.release(KeyCode::ShiftLeft);
    let (shift, name) = match name.trim().strip_prefix("Shift+") {
        Some(name) => (true, name),
        None => (false, name.trim()),
    };
    match key_named(name) {
        Some(key) => {
            if shift {
                keys.press(KeyCode::ShiftLeft);
            }
            keys.press(key);
            keys.release(key);
        }
        None => warn!("PINCH_EDITOR_PRESS: no key called {name:?}"),
    }
}

/// A key by the name a dev hook gives it: Bevy's spelling ("KeyV",
/// "Escape"), the function keys among them.
fn key_named(name: &str) -> Option<KeyCode> {
    const NAMED: [KeyCode; 15] = [
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::F10,
        KeyCode::F11,
        KeyCode::F12,
        KeyCode::Escape,
        KeyCode::Enter,
        KeyCode::Tab,
    ];
    NAMED
        .into_iter()
        .find(|&key| crate::app::binds::key_name(key) == name)
        .or_else(|| crate::app::binds::any_key_from_name(name))
}

/// Dev hook: `PINCH_KEEP=1` presses the results card's keep key once, a
/// second after the card goes up, so the card can be shot before and
/// after the round is kept.
pub(super) fn debug_keep(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    time: Res<Time>,
    mut opened: Local<Option<f32>>,
    mut fired: Local<bool>,
) {
    if *fired || std::env::var("PINCH_KEEP").is_err() {
        return;
    }
    let now = time.elapsed_secs();
    if now - *opened.get_or_insert(now) < 1.0 {
        return;
    }
    *fired = true;
    if let Some(key) = caps.key_for(crate::app::replays::KEEP_CAP) {
        keys.press(key);
        keys.release(key);
    }
}

/// Dev hook: mid-round moments for screenshots. `PINCH_PAUSE=1` raises the
/// pause card a couple of seconds into a versus round; `PINCH_OVER=1` calls
/// the round over so the results card can be shot, offering the round as
/// it stands to the shelf as a finished one would be; `PINCH_INTERLUDE=1`
/// leaves for the series interlude with a mid-series tally on the card,
/// and the round offered there the same way.
/// All inert unless set, like every hook here.
pub(super) fn debug_moments(
    sim: Res<Sim>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut tournament: ResMut<crate::app::tournament::Tournament>,
    screen: Res<State<Screen>>,
    mut next_screen: ResMut<NextState<Screen>>,
    (mut next_vphase, recorder, mut keep): (
        ResMut<NextState<crate::app::VersusPhase>>,
        Res<crate::app::Recorder>,
        ResMut<crate::app::replays::RoundToKeep>,
    ),
    mut hooks: Local<MomentHooks>,
) {
    let MomentHooks {
        pause: pause_hook,
        over: over_hook,
        interlude: interlude_hook,
    } = &mut *hooks;
    if *screen.get() != Screen::Versus {
        return;
    }
    if pause_hook
        .due_after("PINCH_PAUSE", sim.0.ticks(), 2)
        .is_some()
    {
        // A synthetic Escape, so the real pause path opens the card. Let
        // go of at once: the input plugin clears only the `just_` sets, so
        // a key left pressed stays held and swallows the real Escape that
        // should close the card. `just_pressed` survives the release.
        keys.press(KeyCode::Escape);
        keys.release(KeyCode::Escape);
    }
    if over_hook
        .due_after("PINCH_OVER", sim.0.ticks(), 2)
        .is_some()
    {
        next_vphase.set(crate::app::VersusPhase::Over);
        offer_as_finished(&recorder, &mut keep);
    }
    if interlude_hook
        .due_after("PINCH_INTERLUDE", sim.0.ticks(), 2)
        .is_some()
    {
        use crate::app::tournament::{SeriesLength, Tournament};
        let mut wins = [0; crate::sim::MAX_PLAYERS];
        wins[0] = 1;
        wins[1] = 1;
        // The length `PINCH_SERIES` asked for, so the card can be shot at
        // either. Hardcoding the long one here left the best-of-three card
        // unreachable, and it read "Round 3 of 5" for a year. Bare
        // `PINCH_INTERLUDE=1`, with no series in play, still demos the
        // long one, which is what it always did.
        let length = match tournament.length.is_series() {
            true => tournament.length,
            false => SeriesLength::BestOfFive,
        };
        // Two rounds are on the tally, so the card should read as the
        // third. One less than that, because `enter_interlude` counts the
        // round on the way in and this screen is being entered.
        let round = 3.min(length.rounds()) - 1;
        *tournament = Tournament::taken_up(length, round, wins);
        // The round left offered, as a finished one leaves it for the
        // interlude, so the card can be shot with the keep key on it.
        offer_as_finished(&recorder, &mut keep);
        next_screen.set(Screen::Interlude);
    }
}

/// Offer the round being recorded to the shelf as a finished one would be.
/// Filed as a draw: the hooks call the round over without asking who was
/// ahead.
fn offer_as_finished(recorder: &crate::app::Recorder, keep: &mut crate::app::replays::RoundToKeep) {
    if let Some(replay) = &recorder.0 {
        *keep = crate::app::replays::RoundToKeep::Offered(Box::new(crate::app::replays::Unkept {
            text: replay.to_text(),
            stamp: crate::app::clock::now_secs(),
            winner: None,
        }));
    }
}

/// Dev hook: with `PINCH_AUTOPLAY=<level>` set (the level is read by
/// [`DevHook::Autoplay`]; any value turns this on), the puzzle setup phase
/// immediately applies the level's authored solution and starts the run.
///
/// `PINCH_NOSOLVE=1` starts the run without placing anything, so a timed
/// level plays itself out and loses. That is the only way to reach the end
/// of a round unattended: every other path out of a level is a win, and a
/// win stops the clock before it can run out.
///
/// `PINCH_STAY=1` opens the level and leaves it in its setup, nothing
/// placed and nothing started, for a picture of the setup's own lines;
/// `PINCH_STAY=placed` puts the authored solution down and stays in the
/// setup, for a picture of the arrows as they stand before the run.
pub(super) fn debug_autoplay(
    mut sim: ResMut<Sim>,
    campaign: Res<Campaign>,
    mut enabled: Local<Option<bool>>,
    mut next_phase: ResMut<NextState<Phase>>,
) {
    let stay = std::env::var("PINCH_STAY").ok();
    let placed = stay.as_deref() == Some("placed");
    let wanted = || std::env::var("PINCH_AUTOPLAY").is_ok() && (stay.is_none() || placed);
    if !*enabled.get_or_insert_with(wanted) {
        return;
    }
    if placed {
        *enabled = Some(false);
        if let Err((x, y, _, _)) = campaign.current().place_solution(&mut sim.0) {
            warn!("PINCH_STAY=placed: refused at ({x},{y})");
        }
        return;
    }
    if std::env::var("PINCH_NOSOLVE").is_err() {
        let level = campaign.current();
        if let Err((x, y, _, _)) = level.place_solution(&mut sim.0) {
            warn!(
                "PINCH_AUTOPLAY: {:?} refused its own solution at ({x},{y})",
                level.name
            );
        }
    }
    next_phase.set(Phase::Running);
}

/// `PINCH_AUTOPILOT=easy|normal|hard`, or `=1` for normal: the level the
/// local seat plays itself at. Anything else names no level and leaves the
/// seat to its player.
pub(super) fn autopilot_level() -> Option<crate::sim::BotLevel> {
    use crate::sim::BotLevel;
    match std::env::var("PINCH_AUTOPILOT").ok()?.as_str() {
        "easy" => Some(BotLevel::Easy),
        "1" | "normal" => Some(BotLevel::Normal),
        "hard" => Some(BotLevel::Hard),
        _ => None,
    }
}

/// Dev hook: play this process's own seat with the bot brain, through the
/// same pending-actions path a keystroke takes.
///
/// `PINCH_BOTS` is not this. An AI seat is derived: it sits *beside* the
/// humans, the lockstep never carries it, and every peer works out its
/// moves from the frame they have all agreed on, so a table of five AI
/// seats puts nothing on the wire. This drives a *human* seat instead, so
/// the placements are committed and relayed exactly as a player's are and
/// six processes can play a real round with nobody at the keyboards.
///
/// Only ever a seat this process holds: a spectator commits nothing, and a
/// peer that reached for another's chair would be refused by the host and
/// would be asking for the desync the seat check exists to prevent.
///
/// With `PINCH_AUTOPILOT_WALK=1` the seat's cursor walks to each tile the
/// bot picks, a step every key-repeat interval, and the arrow goes down
/// only once it is there: a filmed round shows a cursor steering, not one
/// parked in a corner while arrows appear elsewhere. Off by default, since
/// the walk delays every placement and the measurements want the bot as
/// quick as it is.
pub(super) fn debug_autopilot(
    current: crate::app::sim_events::CurrentBoard,
    online: Res<net::Online>,
    (time, settings, keys): (
        Res<Time>,
        Res<crate::app::settings::GameSettings>,
        Res<ButtonInput<KeyCode>>,
    ),
    mut pending: ResMut<PendingActions>,
    mut cursors: Query<&mut crate::app::cursor::Cursor>,
    mut pilot: Local<Option<Autopilot>>,
) {
    let walk = pilot.get_or_insert_with(|| Autopilot {
        level: autopilot_level(),
        walks: std::env::var("PINCH_AUTOPILOT_WALK").is_ok(),
        heading: None,
        wait: 0.0,
        board: crate::app::sim_events::BoardStamp::default(),
    });
    let Some(level) = walk.level else {
        return;
    };
    // A heading belongs to the board it was picked on: a new round, a
    // rematch or a seek starts the walk over.
    if walk.board.is_new(&current) {
        walk.heading = None;
    }
    let board = &current.sim.0;
    let seat = match online.0.as_ref() {
        Some(session) => match session.session.seat() {
            Some(seat) => seat,
            None => return,
        },
        None => 0,
    };
    let Some(slot) = pending.0.get_mut(usize::from(seat)) else {
        return;
    };
    // Never over a real one: a hand on the keys still beats the autopilot,
    // which is what makes this usable to watch as well as to measure.
    if !matches!(slot, PlayerAction::None) {
        return;
    }
    if !walk.walks {
        *slot = crate::sim::bot_action(board, seat, level);
        return;
    }
    let Some(mut cursor) = cursors.iter_mut().find(|cursor| cursor.player == seat) else {
        return;
    };
    let commit = settings.commit_in_play(true);
    let held = crate::app::cursor::seat_keymap(&settings, seat, online.0.is_some(), commit)
        .is_some_and(|map| keys.any_pressed(map.move_keys()));
    let step = Step {
        held,
        dt: time.delta_secs(),
        interval: settings.repeat_interval,
    };
    *slot = walk.steer(board, seat, level, &mut cursor, step);
}

/// What one frame of the walk sees besides the board: whether a human
/// holds one of the seat's move keys, the frame's length, and the key
/// repeat interval the cursor steps at.
#[derive(Clone, Copy)]
pub(super) struct Step {
    held: bool,
    dt: f32,
    interval: f32,
}

/// Whether `action` would still do something on `board`, by the sim's own
/// rule: a heading picked a few steps ago can point at a tile that has
/// since taken an arrow or lost the post it meant to lift.
fn still_stands(board: &crate::sim::Board, seat: u8, action: PlayerAction) -> bool {
    match action {
        PlayerAction::Place { x, y, .. } => board.placement_refusal(seat, x, y).is_none(),
        PlayerAction::Remove { x, y } => board.removal_refusal(seat, x, y).is_none(),
        PlayerAction::None | PlayerAction::CallEvent(_) => true,
    }
}

/// The autopilot's level, read once, and where `PINCH_AUTOPILOT_WALK` is
/// taking the cursor, when it next steps, and the board it was picked on.
pub(super) struct Autopilot {
    level: Option<crate::sim::BotLevel>,
    walks: bool,
    heading: Option<PlayerAction>,
    wait: f32,
    board: crate::app::sim_events::BoardStamp,
}

impl Autopilot {
    /// One frame of the walk: the action to queue (`None` while walking),
    /// moving `cursor` a step toward the heading every key-repeat interval.
    ///
    /// A human holding a move key has the cursor: the walk stands aside
    /// and forgets its heading, rather than pulling the cursor back every
    /// interval. A heading that no longer stands ([`still_stands`]) is
    /// dropped and picked again, and is checked once more on arrival, so
    /// the walk never queues an action the sim would refuse.
    fn steer(
        &mut self,
        board: &crate::sim::Board,
        seat: u8,
        level: crate::sim::BotLevel,
        cursor: &mut crate::app::cursor::Cursor,
        step: Step,
    ) -> PlayerAction {
        if step.held {
            self.heading = None;
            return PlayerAction::None;
        }
        let wanted = match self.heading {
            Some(action) if still_stands(board, seat, action) => action,
            Some(_) | None => crate::sim::bot_action(board, seat, level),
        };
        let (x, y) = match wanted {
            PlayerAction::Place { x, y, .. } | PlayerAction::Remove { x, y } => (x, y),
            PlayerAction::None | PlayerAction::CallEvent(_) => {
                self.heading = None;
                return wanted;
            }
        };
        if (cursor.x, cursor.y) == (x, y) {
            self.heading = None;
            return wanted;
        }
        self.heading = Some(wanted);
        self.wait -= step.dt;
        if self.wait > 0.0 {
            return PlayerAction::None;
        }
        self.wait = step.interval;
        // One axis at a time, the longer first, as a thumb on the keys goes.
        let (dx, dy) = (
            i16::from(x) - i16::from(cursor.x),
            i16::from(y) - i16::from(cursor.y),
        );
        if dx.abs() >= dy.abs() {
            cursor.x = cursor.x.saturating_add_signed(dx.signum() as i8);
        } else {
            cursor.y = cursor.y.saturating_add_signed(dy.signum() as i8);
        }
        PlayerAction::None
    }
}

/// Dev hook: with `PINCH_NET_PROBE=1`, submit one scripted signpost three
/// seconds into a versus round, through the normal pending-actions path (so
/// online it rides the lockstep like a real keystroke). On the classic arena
/// an Up arrow at (1,4) routes the left spawner stream into P1's castle:
/// a deterministic, decisive round for end-to-end validation.
pub(super) fn debug_net_probe(
    sim: Res<Sim>,
    online: Res<net::Online>,
    mut pending: ResMut<PendingActions>,
    mut fired: Local<bool>,
    mut enabled: Local<Option<bool>>,
) {
    if *fired
        || !*enabled.get_or_insert_with(|| std::env::var("PINCH_NET_PROBE").is_ok())
        || sim.0.ticks() < 90
    {
        return;
    }
    *fired = true;
    let seat = online
        .0
        .as_ref()
        .and_then(|session| session.session.seat())
        .unwrap_or(0);
    pending.0[seat as usize] = PlayerAction::Place {
        x: 1,
        y: 4,
        dir: Direction::Up,
    };
    info!("net probe: seat {seat} placed (1,4) Up");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The walk stands aside for a human's move keys, and never queues a
    /// heading the board has since refused: a tile that took a rock (or a
    /// rival's arrow) while the cursor walked to it.
    #[test]
    fn the_walk_yields_to_keys_and_drops_a_stale_heading() {
        use crate::app::cursor::Cursor;
        use crate::sim::{Board, BotLevel, TileKind};
        let board = Board::new(7, 5, 1);
        let target = PlayerAction::Place {
            x: 5,
            y: 3,
            dir: Direction::Up,
        };
        let mut walk = Autopilot {
            level: Some(BotLevel::Normal),
            walks: true,
            heading: Some(target),
            wait: 0.0,
            board: crate::app::sim_events::BoardStamp::default(),
        };
        let mut cursor = Cursor::seated(0);
        let step = |held| Step {
            held,
            dt: 1.0,
            interval: 0.1,
        };

        let queued = walk.steer(&board, 0, BotLevel::Normal, &mut cursor, step(true));
        assert_eq!(queued, PlayerAction::None);
        assert_eq!((cursor.x, cursor.y), (0, 0), "the hand has the cursor");
        assert_eq!(walk.heading, None, "and the heading is let go");

        // Walking to a tile that stands: one step, then on arrival it goes.
        walk.heading = Some(target);
        walk.steer(&board, 0, BotLevel::Normal, &mut cursor, step(false));
        assert_eq!((cursor.x, cursor.y), (1, 0));
        assert_eq!(walk.heading, Some(target));
        (cursor.x, cursor.y) = (5, 3);
        let queued = walk.steer(&board, 0, BotLevel::Normal, &mut cursor, step(false));
        assert_eq!(queued, target, "it places on arrival");

        // The same arrival on a tile that has since turned to rock does not.
        let mut rocky = board.clone();
        rocky.set_tile(5, 3, TileKind::Rock);
        walk.heading = Some(target);
        let queued = walk.steer(&rocky, 0, BotLevel::Normal, &mut cursor, step(false));
        assert_ne!(queued, target, "a refused heading is never queued");
        assert_ne!(walk.heading, Some(target), "and is dropped");
        assert!(still_stands(&rocky, 0, queued));
    }

    /// The Gates' old name still picks it in the hooks, and a name no map
    /// has falls back to the classic generated beach.
    #[test]
    fn the_old_kelp_keep_name_still_picks_the_gates() {
        use match_setup::MapChoice;
        assert_eq!(skirmish_map("T", "kelp-keep"), MapChoice::TheGates);
        assert_eq!(skirmish_map("T", "gates"), MapChoice::TheGates);
        assert_eq!(skirmish_map("T", "no-such-beach"), MapChoice::GenClassic);
    }

    /// A lookup over a fixed list, standing in for the environment.
    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_string())
        }
    }

    #[test]
    fn no_hooks_means_the_menu() {
        assert_eq!(DevHook::from_env(env(&[])), None);
        assert_eq!(DevHook::from_env(env(&[("PATH", "/usr/bin")])), None);
    }

    /// Every hook is reachable on its own.
    #[test]
    fn each_hook_is_reachable() {
        for (var, expected) in [
            ("PINCH_LOBBY_HOST", DevHook::Lobby),
            ("PINCH_LOBBY_JOIN", DevHook::Lobby),
            ("PINCH_LOBBY_JOIN_BOT", DevHook::Lobby),
            ("PINCH_LOBBY_BROWSE", DevHook::Lobby),
            ("PINCH_HOST", DevHook::Online),
            ("PINCH_JOIN", DevHook::Online),
            ("PINCH_SANDBOX", DevHook::Sandbox),
            ("PINCH_EDITOR", DevHook::Editor),
            ("PINCH_EDITOR_LEVEL", DevHook::Editor),
            ("PINCH_SETTINGS", DevHook::Settings),
            ("PINCH_CONTROLS", DevHook::Controls),
            ("PINCH_ACHIEVEMENTS", DevHook::Achievements),
            ("PINCH_LANGUAGE", DevHook::Language),
            ("PINCH_LIBRARY", DevHook::Replays),
            ("PINCH_STAGES", DevHook::StageSelect),
            ("PINCH_AUTOPLAY", DevHook::Autoplay { level: 0 }),
            (
                "PINCH_MATCH",
                DevHook::MatchSetup {
                    step: match_setup::Step::Game,
                },
            ),
            ("PINCH_REPLAY", DevHook::Replay),
        ] {
            assert_eq!(
                DevHook::from_env(env(&[(var, "1")])),
                Some(expected),
                "{var}"
            );
        }
    }

    /// The skirmish hook carries its map size, defaulting to the classic
    /// board for anything it does not recognise, and picks up the series
    /// flag only alongside it.
    #[test]
    fn skirmish_reads_its_size_and_series_flag() {
        use match_setup::MapChoice;
        let skirmish = |vars: &[(&str, &str)]| DevHook::from_env(env(vars));
        use crate::app::tournament::SeriesLength;
        assert_eq!(
            skirmish(&[("PINCH_SKIRMISH", "large")]),
            Some(DevHook::Skirmish {
                map: MapChoice::GenLarge,
                series: SeriesLength::Single
            })
        );
        // `classic` the generated 12x9, `arena` the handmade one.
        for (token, map) in [
            ("classic", MapChoice::GenClassic),
            ("arena", MapChoice::Classic),
        ] {
            assert_eq!(
                skirmish(&[("PINCH_SKIRMISH", token)]),
                Some(DevHook::Skirmish {
                    map,
                    series: SeriesLength::Single
                }),
                "{token}"
            );
        }
        assert_eq!(
            skirmish(&[("PINCH_SKIRMISH", "xl"), ("PINCH_SERIES", "1")]),
            Some(DevHook::Skirmish {
                map: MapChoice::GenXl,
                series: SeriesLength::BestOfFive
            }),
            "the historical bare flag still means the long series"
        );
        assert_eq!(
            skirmish(&[("PINCH_SKIRMISH", "xl"), ("PINCH_SERIES", "3")]),
            Some(DevHook::Skirmish {
                map: MapChoice::GenXl,
                series: SeriesLength::BestOfThree
            })
        );
        assert_eq!(
            skirmish(&[("PINCH_SKIRMISH", "wibble")]),
            Some(DevHook::Skirmish {
                map: MapChoice::GenClassic,
                series: SeriesLength::Single
            })
        );
        // Autoplay carries the level to start on; the historical `=1`
        // still means the first level.
        assert_eq!(
            DevHook::from_env(env(&[("PINCH_AUTOPLAY", "1")])),
            Some(DevHook::Autoplay { level: 0 })
        );
        assert_eq!(
            DevHook::from_env(env(&[("PINCH_AUTOPLAY", "25")])),
            Some(DevHook::Autoplay { level: 24 })
        );

        // PINCH_SERIES alone is not a shortcut; it only qualifies one.
        assert_eq!(skirmish(&[("PINCH_SERIES", "1")]), None);
    }

    /// Precedence, and the reason this is one function at all: the
    /// lobby beats a direct session, a session beats the sandbox, and the
    /// screens beat the skirmish.
    #[test]
    fn the_ladder_has_a_fixed_precedence() {
        let all = env(&[
            ("PINCH_LOBBY_HOST", "1"),
            ("PINCH_HOST", "47777"),
            ("PINCH_SANDBOX", "1"),
            ("PINCH_EDITOR", "1"),
            ("PINCH_SKIRMISH", "xl"),
            ("PINCH_REPLAY", "1"),
        ]);
        assert_eq!(DevHook::from_env(all), Some(DevHook::Lobby));
        assert_eq!(
            DevHook::from_env(env(&[("PINCH_HOST", "47777"), ("PINCH_SANDBOX", "1")])),
            Some(DevHook::Online)
        );
        assert_eq!(
            DevHook::from_env(env(&[("PINCH_SANDBOX", "1"), ("PINCH_EDITOR", "1")])),
            Some(DevHook::Sandbox)
        );
        assert_eq!(
            DevHook::from_env(env(&[("PINCH_EDITOR", "1"), ("PINCH_SKIRMISH", "xl")])),
            Some(DevHook::Editor)
        );
        assert_eq!(
            DevHook::from_env(env(&[("PINCH_SKIRMISH", "xl"), ("PINCH_REPLAY", "1")])),
            Some(DevHook::Skirmish {
                map: match_setup::MapChoice::GenXl,
                series: crate::app::tournament::SeriesLength::Single
            })
        );
    }
}

/// `PINCH_PRESENT=immediate|mailbox|novsync|vsync`: how the window presents,
/// for measuring a screen uncapped. Profiling only: tearing and a busy GPU
/// are the price of seeing past the display's refresh.
pub(super) fn present_mode() -> Option<bevy::window::PresentMode> {
    use bevy::window::PresentMode;
    Some(match std::env::var("PINCH_PRESENT").ok()?.as_str() {
        "immediate" => PresentMode::Immediate,
        "mailbox" => PresentMode::Mailbox,
        "novsync" => PresentMode::AutoNoVsync,
        _ => PresentMode::AutoVsync,
    })
}

/// `PINCH_PROFILE=<secs>`: a GPU profile of whatever screen the other hooks
/// open. After `PINCH_PROFILE_WARMUP` seconds (default 8), sample every
/// render pass's GPU time (wgpu timestamp queries, through Bevy's render
/// diagnostics), the frame time and the process's CPU time for `<secs>`
/// seconds, print each pass's mean and 95th percentile, append them to
/// `PINCH_PROFILE_OUT` (if set) under the label `PINCH_PROFILE_LABEL`, and
/// exit. Pair it with `PINCH_PRESENT=immediate` to see past vsync.
///
/// `PINCH_HIDE` (a comma list of `sand`, `board`, `sea`, `pools`,
/// `shadows`, `particles`, `sprites`, `ui`, `boxshadows`) hides one kind of
/// thing, and `PINCH_GFX` (`nohdr`, `nograde`, `msaa0`, `msaa4`,
/// `shadowsamples<n>`) changes one stage of the camera, so each cost can be
/// measured by its absence.
///
/// `PINCH_PROFILE_CPU=1` leaves the GPU timestamp queries out (they cost CPU
/// time of their own every frame) and reports the frame time, the process's
/// CPU time and each thread's share of it.
#[derive(Resource, Clone, Debug, Default)]
pub(super) struct Profile {
    warmup: f32,
    span: f32,
    hide: Vec<String>,
    gfx: Vec<String>,
    cpu_only: bool,
}

impl Profile {
    pub(super) fn from_env() -> Option<Profile> {
        let span = std::env::var("PINCH_PROFILE").ok()?.parse().ok()?;
        let list = |key: &str| -> Vec<String> {
            std::env::var(key)
                .unwrap_or_default()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        };
        Some(Profile {
            warmup: std::env::var("PINCH_PROFILE_WARMUP")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8.0),
            span,
            hide: list("PINCH_HIDE"),
            gfx: list("PINCH_GFX"),
            cpu_only: std::env::var("PINCH_PROFILE_CPU").is_ok(),
        })
    }

    fn hides(&self, what: &str) -> bool {
        self.hide.iter().any(|h| h == what)
    }

    fn takes_off(&self, what: &str) -> bool {
        self.gfx.iter().any(|g| g == what)
    }
}

/// `PINCH_GFX=msaa0|msaa4`: the anti-aliasing a profile run forces, over
/// the settings.
/// Read once: it is asked every frame.
pub(super) fn msaa_override() -> Option<bool> {
    static SAID: std::sync::OnceLock<Option<bool>> = std::sync::OnceLock::new();
    *SAID.get_or_init(|| {
        let gfx = std::env::var("PINCH_GFX").ok()?;
        let mut said = None;
        for g in gfx.split(',') {
            match g {
                "msaa0" => said = Some(false),
                "msaa4" => said = Some(true),
                _ => {}
            }
        }
        said
    })
}

/// Install the profiler when `PINCH_PROFILE` asks for it.
pub(super) fn install_profiling(app: &mut App) {
    let Some(profile) = Profile::from_env() else {
        return;
    };
    if !profile.cpu_only {
        app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
    }
    app.add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
        .insert_resource(profile)
        .init_resource::<Churn>()
        .add_systems(
            Last,
            (profile_camera, profile_hide, profile_churn, profile_sample).chain(),
        );
}

/// What changed each frame that the renderer pays for again: sprites
/// (each distinct look is its own material in Bevy's sprite meshes), sprite
/// materials made, texts and UI nodes, summed over the sample window.
#[derive(Resource, Default)]
struct Churn {
    frames: u32,
    sprites: u64,
    sprites_changed: u64,
    materials_added: u64,
    texts_changed: u64,
    nodes_changed: u64,
    by_image: std::collections::HashMap<AssetId<Image>, u64>,
}

type SpriteMaterial = SpriteMeshMaterial;

fn profile_churn(
    mut churn: ResMut<Churn>,
    profile: Res<Profile>,
    time: Res<Time<Real>>,
    sprites: Query<Ref<Sprite>>,
    texts: Query<(), Changed<Text>>,
    nodes: Query<(), Changed<Node>>,
    mut materials: MessageReader<AssetEvent<SpriteMaterial>>,
) {
    let added = materials
        .read()
        .filter(|e| matches!(e, AssetEvent::Added { .. }))
        .count() as u64;
    if time.elapsed_secs() < profile.warmup {
        return;
    }
    churn.frames += 1;
    churn.sprites += sprites.iter().count() as u64;
    for sprite in sprites.iter().filter(Ref::is_changed) {
        churn.sprites_changed += 1;
        *churn.by_image.entry(sprite.image.id()).or_default() += 1;
    }
    churn.materials_added += added;
    churn.texts_changed += texts.iter().count() as u64;
    churn.nodes_changed += nodes.iter().count() as u64;
}

/// [`Churn`] as the line [`profile_sample`] prints, the images of the
/// sprites changed most often named.
#[derive(bevy::ecs::system::SystemParam)]
struct ChurnLine<'w> {
    churn: Res<'w, Churn>,
    assets: Res<'w, AssetServer>,
}

impl ChurnLine<'_> {
    fn line(&self) -> String {
        let churn = &*self.churn;
        let n = f64::from(churn.frames.max(1));
        let mut top: Vec<_> = churn.by_image.iter().collect();
        top.sort_by(|a, b| b.1.cmp(a.1));
        let top: Vec<String> = top
            .into_iter()
            .take(8)
            .map(|(id, count)| {
                let name = self
                    .assets
                    .get_path(*id)
                    .map_or_else(|| format!("{id:?}"), |p| p.to_string());
                format!("{name} {:.1}", *count as f64 / n)
            })
            .collect();
        format!(
            "sprites {:.0}, sprites changed {:.1}, sprite materials added {:.1}, texts changed {:.1}, nodes changed {:.1} (per frame); changed by image: {}",
            churn.sprites as f64 / n,
            churn.sprites_changed as f64 / n,
            churn.materials_added as f64 / n,
            churn.texts_changed as f64 / n,
            churn.nodes_changed as f64 / n,
            top.join(", "),
        )
    }
}

/// Change the camera and the cards as `PINCH_GFX` and `PINCH_HIDE=boxshadows`
/// say, every frame, since screens spawn new cards all along.
fn profile_camera(
    mut commands: Commands,
    profile: Res<Profile>,
    cameras: Query<Entity, With<Camera2d>>,
    box_shadows: Query<Entity, With<BoxShadow>>,
) {
    for camera in &cameras {
        let mut entity = commands.entity(camera);
        if profile.takes_off("nohdr") {
            entity.remove::<bevy::camera::Hdr>();
        }
        if profile.takes_off("nograde") {
            entity.insert(bevy::render::view::ColorGrading::default());
        }
        if let Some(n) = profile
            .gfx
            .iter()
            .find_map(|g| g.strip_prefix("shadowsamples")?.parse().ok())
        {
            entity.insert(BoxShadowSamples(n));
        }
    }
    if profile.hides("boxshadows") {
        for card in &box_shadows {
            commands.entity(card).remove::<BoxShadow>();
        }
    }
}

type SandQuad = MeshMaterial2d<crate::app::materials::SandMaterial>;
type WaterQuad = MeshMaterial2d<crate::app::materials::WaterMaterial>;
type WaterOnly = (Without<Sprite>, Without<SandQuad>);
type FadeQuad = MeshMaterial2d<crate::app::materials::FadeMaterial>;
type FadeOnly = (
    With<FadeQuad>,
    Without<Sprite>,
    Without<SandQuad>,
    Without<WaterQuad>,
    Without<Node>,
);
type UiRoot = (
    With<Node>,
    Without<ChildOf>,
    Without<Sprite>,
    Without<SandQuad>,
    Without<WaterQuad>,
);

/// Hide what `PINCH_HIDE` names, every frame, since rounds spawn new
/// things all along.
fn profile_hide(
    profile: Res<Profile>,
    art: Option<Res<crate::app::art::Art>>,
    mut sand: Query<(&mut Visibility, &SandQuad), Without<Sprite>>,
    mut water: Query<(&mut Visibility, &WaterQuad), WaterOnly>,
    mut sprites: Query<(&mut Visibility, &Sprite, Has<crate::app::effects::Particle>)>,
    mut roots: Query<&mut Visibility, UiRoot>,
    mut fades: Query<&mut Visibility, FadeOnly>,
) {
    let Some(art) = art else {
        return;
    };
    if profile.hide.is_empty() {
        return;
    }
    let shaded = &art.shaded;
    for (mut visibility, material) in &mut sand {
        let board = material.0 == shaded.board_sand;
        if (board && profile.hides("board")) || (!board && profile.hides("sand")) {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
    for (mut visibility, material) in &mut water {
        let sea = material.0 == shaded.sea || material.0 == shaded.ocean;
        if (sea && profile.hides("sea")) || (!sea && profile.hides("pools")) {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
    let shadows = [
        &art.shadow,
        &art.gull_shadow,
        &art.gull_shadow_b,
        &art.cloud,
    ];
    for (mut visibility, sprite, particle) in &mut sprites {
        let shadow = shadows.contains(&&sprite.image);
        if profile.hides("sprites")
            || (particle && profile.hides("particles"))
            || (shadow && profile.hides("shadows"))
        {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
    // Every fading quad is a particle (the footprints).
    if profile.hides("sprites") || profile.hides("particles") {
        for mut visibility in &mut fades {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
    if profile.hides("ui") {
        for mut visibility in &mut roots {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
}

/// What [`profile_sample`] has gathered: per diagnostic path, one value a
/// frame, and the last measurement read so none is counted twice.
#[derive(Default)]
struct ProfileSamples {
    started: Option<(f32, f64)>,
    threads: std::collections::BTreeMap<u32, (String, f64)>,
    seen: std::collections::HashMap<String, std::time::Instant>,
    values: std::collections::BTreeMap<String, Vec<f64>>,
    frames: u32,
}

/// The process's CPU time so far, user and system, in seconds (Linux).
fn process_cpu_secs() -> f64 {
    let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else {
        return 0.0;
    };
    // Fields after the command's closing parenthesis; utime and stime are
    // the 12th and 13th of those.
    let rest = stat.rsplit_once(')').map_or("", |(_, r)| r);
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let ticks: f64 = fields.get(11..13).map_or(0.0, |f| {
        f.iter().filter_map(|v| v.parse::<f64>().ok()).sum()
    });
    ticks / 100.0
}

/// Each thread's CPU time so far, user and system, in seconds, by thread id
/// with its name (Linux).
fn thread_cpu_secs() -> std::collections::BTreeMap<u32, (String, f64)> {
    let mut out = std::collections::BTreeMap::new();
    let Ok(tasks) = std::fs::read_dir("/proc/self/task") else {
        return out;
    };
    for task in tasks.flatten() {
        let Some(tid) = task.file_name().to_str().and_then(|t| t.parse().ok()) else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(task.path().join("stat")) else {
            continue;
        };
        let name = stat
            .split_once('(')
            .and_then(|(_, r)| r.rsplit_once(')'))
            .map_or(String::new(), |(n, _)| n.to_owned());
        let rest = stat.rsplit_once(')').map_or("", |(_, r)| r);
        let fields: Vec<&str> = rest.split_whitespace().collect();
        let ticks: f64 = fields.get(11..13).map_or(0.0, |f| {
            f.iter().filter_map(|v| v.parse::<f64>().ok()).sum()
        });
        out.insert(tid, (name, ticks / 100.0));
    }
    out
}

fn profile_sample(
    profile: Res<Profile>,
    time: Res<Time<Real>>,
    store: Res<bevy::diagnostic::DiagnosticsStore>,
    windows: Query<&Window>,
    mut samples: Local<ProfileSamples>,
    mut exit: MessageWriter<AppExit>,
    churn: ChurnLine,
) {
    let now = time.elapsed_secs();
    if now < profile.warmup {
        return;
    }
    if samples.started.is_none() {
        samples.threads = thread_cpu_secs();
    }
    let (start, cpu_start) = *samples
        .started
        .get_or_insert_with(|| (now, process_cpu_secs()));
    // Mark everything already in the store as seen on the first frame, so
    // the warm-up's measurements stay out.
    let first = samples.frames == 0;
    samples.frames += 1;
    for diagnostic in store.iter() {
        let path = diagnostic.path().as_str().to_owned();
        let last = samples.seen.get(&path).copied();
        let mut per_frame: Vec<(std::time::Instant, f64)> = Vec::new();
        for m in diagnostic.measurements() {
            if last.is_some_and(|l| m.time <= l) {
                continue;
            }
            // Spans sharing a path in one frame arrive with one timestamp:
            // they add up to that frame's cost.
            match per_frame.last_mut() {
                Some((t, v)) if *t == m.time => *v += m.value,
                _ => per_frame.push((m.time, m.value)),
            }
        }
        if let Some((t, _)) = per_frame.last() {
            samples.seen.insert(path.clone(), *t);
        }
        if !first {
            samples
                .values
                .entry(path)
                .or_default()
                .extend(per_frame.into_iter().map(|(_, v)| v));
        }
    }
    if now - start < profile.span {
        return;
    }
    let cpu = process_cpu_secs() - cpu_start;
    let frames = samples.frames.saturating_sub(1).max(1);
    let size = windows.iter().next().map_or(String::new(), |w| {
        format!("{}x{}", w.physical_width(), w.physical_height())
    });
    let label = std::env::var("PINCH_PROFILE_LABEL").unwrap_or_default();
    let mut lines = vec![format!(
        "# {label} {size} frames {frames} in {:.1}s, cpu {:.2} ms/frame (all threads)",
        now - start,
        cpu * 1000.0 / f64::from(frames),
    )];
    // Per thread name (Bevy's pool workers share one name each), CPU ms per
    // frame and cores busy over the window.
    let mut by_name: std::collections::BTreeMap<String, (f64, u32)> = Default::default();
    for (tid, (name, secs)) in thread_cpu_secs() {
        let before = samples.threads.get(&tid).map_or(0.0, |(_, s)| *s);
        let entry = by_name.entry(name).or_default();
        entry.0 += secs - before;
        entry.1 += 1;
    }
    let wall = f64::from(now - start).max(1e-6);
    let mut named: Vec<_> = by_name.into_iter().filter(|(_, (s, _))| *s > 0.0).collect();
    named.sort_by(|a, b| b.1.0.total_cmp(&a.1.0));
    for (name, (secs, count)) in named {
        lines.push(format!(
            "{label}\t{size}\tthread/{name} x{count}\t{:.3}\t{:.2}\t0",
            secs * 1000.0 / f64::from(frames),
            secs / wall,
        ));
    }
    lines.push(format!("# {label} churn: {}", churn.line()));
    lines.push(format!(
        "{label}\t{size}\tcpu_ms_per_frame\t{:.3}\t{:.2}\t{frames}",
        cpu * 1000.0 / f64::from(frames),
        cpu / wall,
    ));
    let stat = |v: &[f64]| -> (f64, f64) {
        let mut sorted = v.to_vec();
        sorted.sort_by(f64::total_cmp);
        let n = sorted.len();
        let mean = sorted.iter().sum::<f64>() / n as f64;
        (mean, sorted[(n * 95 / 100).min(n - 1)])
    };
    for (path, values) in &samples.values {
        let keep = path.ends_with("elapsed_gpu")
            || path.ends_with("elapsed_cpu")
            || path.starts_with("fps")
            || path.starts_with("frame_time");
        if !keep || values.is_empty() {
            continue;
        }
        let (mean, p95) = stat(values);
        lines.push(format!(
            "{label}\t{size}\t{path}\t{mean:.3}\t{p95:.3}\t{}",
            values.len()
        ));
    }
    let text = lines.join("\n");
    eprintln!("{text}");
    if let Ok(out) = std::env::var("PINCH_PROFILE_OUT") {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(out)
        {
            let _ = writeln!(file, "{text}");
        }
    }
    exit.write(AppExit::Success);
}
