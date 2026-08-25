//! Dev-only environment hooks (`PINCH_*`): shortcuts past the menu,
//! headless screenshots, and campaign autoplay. All inert unless the
//! matching variable is set.
//!
//! Every `PINCH_*` variable the game answers to is read here, either by
//! the [`DevHook`] ladder or by one of the typed accessors below, so this
//! file is the complete inventory of the dev surface and each variable is
//! parsed once.

use crate::app::{Campaign, PendingActions, Phase, Screen, Sim, announce, match_setup, net};
use crate::sim::{Direction, PlayerAction, TideEvent};
use bevy::prelude::*;

/// `PINCH_WINDOW=WxH`: open at a given size instead of the default, so a
/// screen can be checked at the sizes players will actually drag it to.
pub(super) fn window_size() -> Option<(f32, f32)> {
    let raw = std::env::var("PINCH_WINDOW").ok()?;
    let (w, h) = raw.split_once(['x', 'X'])?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// `PINCH_LOBBY_HOST=n`: host a LAN lobby unattended and launch once `n`
/// peers are aboard (`=1`, the historical value, launches on the first).
pub(super) fn auto_host_quota() -> Option<usize> {
    std::env::var("PINCH_LOBBY_HOST")
        .ok()
        .map(|quota| quota.parse().unwrap_or(1))
}

/// `PINCH_LOBBY_JOIN`: join the first LAN host the lobby hears, unattended.
pub(super) fn auto_join() -> bool {
    std::env::var("PINCH_LOBBY_JOIN").is_ok()
}

/// `PINCH_LOBBY_WATCH`: the same, but at the rail as a spectator.
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

/// `PINCH_BOTS=n`: n AI players in a hosted lobby match (and in the direct
/// `PINCH_HOST`/`PINCH_JOIN` pair, where both sides must set it because
/// there is no lobby to agree it for them).
pub(super) fn bots() -> Option<u8> {
    std::env::var("PINCH_BOTS").ok()?.parse().ok()
}

/// `PINCH_SEATS=n`: how many seats a skirmish sets the table for.
fn seats() -> Option<u8> {
    std::env::var("PINCH_SEATS").ok()?.parse().ok()
}

/// `PINCH_SANDBOX`: a local arena with preloaded castles.
pub(super) fn sandbox() -> bool {
    std::env::var("PINCH_SANDBOX").is_ok()
}

/// `PINCH_ST_EXEC`: every main-world schedule on the single-threaded
/// executor, for the backlog's CPU measurement.
pub(super) fn single_threaded_executor() -> bool {
    std::env::var("PINCH_ST_EXEC").is_ok()
}

/// `PINCH_NO_UPDATE`: never ask GitHub for a newer release this run, for
/// scripted launches and screenshots, and for the machine with no network
/// that should not spend eight seconds finding that out.
pub(super) fn no_update_check() -> bool {
    std::env::var("PINCH_NO_UPDATE").is_ok()
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
/// (which hook wins when two are set) is explicit and testable, and
/// `kickoff` only has to act on the answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum DevHook {
    /// `PINCH_LOBBY_HOST` / `PINCH_LOBBY_JOIN`: the LAN lobby screen.
    Lobby,
    /// `PINCH_HOST` / `PINCH_JOIN`: straight into an online round.
    Online,
    /// `PINCH_SANDBOX`: a local arena with preloaded castles.
    Sandbox,
    /// `PINCH_AUTOPLAY=<level>`: the puzzle campaign from that level (1 is
    /// the first), playing its own authored solutions.
    Autoplay {
        level: usize,
    },
    Editor,
    Settings,
    Controls,
    Achievements,
    /// `PINCH_LANGUAGE`: the language picker.
    Language,
    /// `PINCH_STAGES=tide|beach`: the stage list for either puzzle list.
    StageSelect {
        beach: bool,
    },
    /// `PINCH_SKIRMISH=classic|large|xl|ocean|custom`, plus `PINCH_SERIES=3|5`
    /// for a series: four seats, three of them AI. Any unrecognised
    /// value is the classic beach.
    Skirmish {
        map: match_setup::MapChoice,
        series: crate::app::tournament::SeriesLength,
    },
    /// `PINCH_MATCH`: the match-setup screen.
    MatchSetup,
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
        if set("PINCH_LOBBY_HOST") || set("PINCH_LOBBY_JOIN") || set("PINCH_LOBBY_WATCH") {
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
            ("PINCH_SETTINGS", DevHook::Settings),
            ("PINCH_CONTROLS", DevHook::Controls),
            ("PINCH_ACHIEVEMENTS", DevHook::Achievements),
            ("PINCH_LANGUAGE", DevHook::Language),
            ("PINCH_LIBRARY", DevHook::Replays),
        ] {
            if set(name) {
                return Some(hook);
            }
        }
        if let Some(list) = var("PINCH_STAGES") {
            return Some(DevHook::StageSelect {
                beach: list == "beach",
            });
        }
        if let Some(size) = var("PINCH_SKIRMISH") {
            return Some(DevHook::Skirmish {
                map: match size.as_str() {
                    "large" => match_setup::MapChoice::GenLarge,
                    "xl" => match_setup::MapChoice::GenXl,
                    "ocean" => match_setup::MapChoice::GenOcean,
                    "custom" => match_setup::MapChoice::Custom,
                    _ => match_setup::MapChoice::GenClassic,
                },
                // Bare `PINCH_SERIES=1` still means the long series it
                // always meant; `=3` asks for the short one.
                series: match var("PINCH_SERIES").as_deref() {
                    None => crate::app::tournament::SeriesLength::Single,
                    Some("3") => crate::app::tournament::SeriesLength::BestOfThree,
                    Some(_) => crate::app::tournament::SeriesLength::BestOfFive,
                },
            });
        }
        if set("PINCH_MATCH") {
            return Some(DevHook::MatchSetup);
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
) {
    let Some(hook) = DevHook::from_env(|name| std::env::var(name).ok()) else {
        return;
    };
    match hook {
        DevHook::Lobby => {
            // The lobby sends the host's match terms, so the same hooks that
            // configure a local skirmish configure a hosted one.
            config.map = debug_map().unwrap_or(config.map);
            config.bots = bots().unwrap_or(config.bots);
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
        DevHook::StageSelect { beach } => {
            if beach {
                let levels = crate::sim::challenge_levels();
                let builtins = levels.len();
                campaign.reset(crate::app::CampaignKind::BeachDay, levels, builtins);
            }
            next_screen.set(Screen::StageSelect);
        }
        DevHook::Skirmish { map, series } => {
            config.seats = seats().unwrap_or(4).clamp(2, crate::sim::MAX_PLAYERS as u8);
            config.bots = config.seats - 1;
            config.map = map;
            config.series = series;
            // A hand-typed PINCH_BOTS is not trusted to leave a human seat:
            // more bots than seats underflows `seats - bots` everywhere it
            // is used to count humans.
            config.bots = bots().unwrap_or(config.seats - 1).min(config.seats - 1);
            if series.is_series() {
                *tournament = crate::app::tournament::Tournament::start(series);
            }
            config.armed = true;
            next_screen.set(Screen::Versus);
        }
        DevHook::MatchSetup => {
            // `PINCH_SEATS`/`PINCH_BOTS` apply here too, so the screen can
            // be seen with a full table rather than only the default pair.
            config.seats = seats()
                .unwrap_or(config.seats)
                .clamp(2, crate::sim::MAX_PLAYERS as u8);
            config.bots = bots().unwrap_or(config.bots).min(config.seats - 1);
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

/// Dev hook: `PINCH_SKIRMISH=classic|large|xl|ocean|custom` also names the
/// map for a hosted lobby match.
fn debug_map() -> Option<match_setup::MapChoice> {
    Some(match std::env::var("PINCH_SKIRMISH").ok()?.as_str() {
        "large" => match_setup::MapChoice::GenLarge,
        "xl" => match_setup::MapChoice::GenXl,
        "ocean" => match_setup::MapChoice::GenOcean,
        // `custom` takes the first handmade beach with castles enough,
        // which is how the wire path gets exercised without a menu.
        "custom" => match_setup::MapChoice::Custom,
        _ => match_setup::MapChoice::GenClassic,
    })
}

/// Dev hook: `PINCH_TIDE=<0-7>` fires a real tide event a few seconds in,
/// rather than the banner alone, so what the event *does* can be watched.
/// Seven is the castle swap.
pub(super) fn debug_tide(mut sim: ResMut<Sim>, mut hook: Local<OneShot>) {
    let Some(which) = hook.due("PINCH_TIDE", sim.0.ticks()) else {
        return;
    };
    let index = which.parse::<usize>().unwrap_or(0) % TideEvent::ALL.len();
    sim.0.force_tide_event(TideEvent::ALL[index], 0);
}

/// A hook that reads its variable once, waits for the round to have
/// something on the beach worth looking at, and then fires exactly once.
///
/// Three hooks had this shape copied out: read the environment into a
/// `Local` so it is not re-read every frame, bail unless the round has run
/// a few seconds, set a `fired` flag. Three copies of a four-line rule is
/// three places for it to drift.
#[derive(Default)]
pub(super) struct OneShot {
    setting: Option<Option<String>>,
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
    fn due_after(&mut self, var: &str, ticks: u64, seconds: u64) -> Option<String> {
        let setting = self
            .setting
            .get_or_insert_with(|| std::env::var(var).ok())
            .clone()?;
        if self.fired || ticks < seconds * u64::from(crate::sim::TICKS_PER_SECOND) {
            return None;
        }
        self.fired = true;
        Some(setting)
    }
}

/// Dev hook: `PINCH_LURE=<seat>` starts a lure a few seconds in, which is
/// otherwise something you wait for a molting crab to do.
pub(super) fn debug_lure(mut sim: ResMut<Sim>, mut hook: Local<OneShot>) {
    let Some(which) = hook.due("PINCH_LURE", sim.0.ticks()) else {
        return;
    };
    let seat = which.parse::<u8>().unwrap_or(0);
    sim.0
        .force_lure(seat.min(crate::sim::MAX_PLAYERS as u8 - 1));
}

/// Dev hook: `PINCH_BANNER=lure|surge|<0-7>` raises one centre-screen
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
        index => announce::Announcement::Tide(
            TideEvent::ALL[index.parse::<usize>().unwrap_or(0) % TideEvent::ALL.len()],
        ),
    });
}

/// Dev hook: with `PINCH_SCREENSHOT=/path/out.png`, saves a frame after
/// `PINCH_SCREENSHOT_AT` seconds (default 2) and exits shortly after. Used to
/// verify rendering headlessly; harmless otherwise.
pub(super) fn debug_screenshot(
    mut commands: Commands,
    time: Res<Time>,
    mut fired: Local<bool>,
    mut config: Local<Option<Option<(String, f32)>>>,
    mut exit: MessageWriter<AppExit>,
) {
    // Env lookups allocate; resolve them once, not every frame.
    let config = config.get_or_insert_with(|| {
        std::env::var("PINCH_SCREENSHOT").ok().map(|path| {
            let at = std::env::var("PINCH_SCREENSHOT_AT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2.0);
            (path, at)
        })
    });
    let Some((path, at)) = config else {
        return;
    };
    let (path, at) = (path.clone(), *at);
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    if !*fired && time.elapsed_secs() > at {
        *fired = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if *fired && time.elapsed_secs() > at + 1.5 {
        exit.write(AppExit::Success);
    }
}

/// Dev hook: mid-round moments for screenshots. `PINCH_PAUSE=1` raises the
/// pause card a couple of seconds into a versus round; `PINCH_OVER=1` calls
/// the round over so the results card can be shot; `PINCH_INTERLUDE=1`
/// leaves for the series interlude with a mid-series tally on the card.
/// All inert unless set, like every hook here.
#[allow(clippy::too_many_arguments)]
pub(super) fn debug_moments(
    sim: Res<Sim>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut tournament: ResMut<crate::app::tournament::Tournament>,
    screen: Res<State<Screen>>,
    mut next_screen: ResMut<NextState<Screen>>,
    mut next_vphase: ResMut<NextState<crate::app::VersusPhase>>,
    mut pause_hook: Local<OneShot>,
    mut over_hook: Local<OneShot>,
    mut interlude_hook: Local<OneShot>,
) {
    if *screen.get() != Screen::Versus {
        return;
    }
    if pause_hook
        .due_after("PINCH_PAUSE", sim.0.ticks(), 2)
        .is_some()
    {
        // A synthetic Escape, so the real pause path opens the card: the
        // input plugin clears the press at the next frame's start.
        keys.press(KeyCode::Escape);
    }
    if over_hook
        .due_after("PINCH_OVER", sim.0.ticks(), 2)
        .is_some()
    {
        next_vphase.set(crate::app::VersusPhase::Over);
    }
    if interlude_hook
        .due_after("PINCH_INTERLUDE", sim.0.ticks(), 2)
        .is_some()
    {
        use crate::app::tournament::{SeriesLength, Tournament};
        let mut wins = [0; crate::sim::MAX_PLAYERS];
        wins[0] = 1;
        wins[1] = 1;
        *tournament = Tournament::taken_up(SeriesLength::BestOfFive, 3, wins);
        next_screen.set(Screen::Interlude);
    }
}

/// Dev hook: with `PINCH_AUTOPLAY=1`, the puzzle setup phase immediately
/// applies the level's authored solution and starts the run.
pub(super) fn debug_autoplay(
    mut sim: ResMut<Sim>,
    campaign: Res<Campaign>,
    mut enabled: Local<Option<bool>>,
    mut next_phase: ResMut<NextState<Phase>>,
) {
    if !*enabled.get_or_insert_with(|| std::env::var("PINCH_AUTOPLAY").is_ok()) {
        return;
    }
    let level = campaign.current();
    if let Err((x, y, _)) = level.place_solution(&mut sim.0) {
        warn!(
            "PINCH_AUTOPLAY: {:?} refused its own solution at ({x},{y})",
            level.name
        );
    }
    next_phase.set(Phase::Running);
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
            ("PINCH_HOST", DevHook::Online),
            ("PINCH_JOIN", DevHook::Online),
            ("PINCH_SANDBOX", DevHook::Sandbox),
            ("PINCH_EDITOR", DevHook::Editor),
            ("PINCH_SETTINGS", DevHook::Settings),
            ("PINCH_CONTROLS", DevHook::Controls),
            ("PINCH_ACHIEVEMENTS", DevHook::Achievements),
            ("PINCH_LANGUAGE", DevHook::Language),
            ("PINCH_LIBRARY", DevHook::Replays),
            ("PINCH_STAGES", DevHook::StageSelect { beach: false }),
            ("PINCH_AUTOPLAY", DevHook::Autoplay { level: 0 }),
            ("PINCH_MATCH", DevHook::MatchSetup),
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

        // The stage list takes which of the two puzzle lists to show.
        assert_eq!(
            DevHook::from_env(env(&[("PINCH_STAGES", "beach")])),
            Some(DevHook::StageSelect { beach: true })
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
