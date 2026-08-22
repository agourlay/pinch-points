//! What the header and the prompt line say, per screen.
//!
//! Pure functions of game state and the string table: no ECS, no spawning,
//! which makes them testable, and they are every player-facing line in
//! the game, so they are worth testing.

use crate::app::editor::EditorState;
use crate::app::i18n::{Tr, fill};
use crate::app::lobby::LobbyState;
use crate::app::net::Online;
use crate::app::settings::GameSettings;
use crate::app::teams::TeamMode;
use crate::app::{Bots, Campaign, CampaignKind, Phase, Playback, Screen, Seats, Sim, VersusPhase};
use crate::sim::{Goal, LURE_TICKS, TideEvent};
use bevy::prelude::*;

pub(super) const CLOCK_CALM: Color = Color::srgb(0.95, 0.93, 0.84);
pub(super) const CLOCK_RED: Color = Color::srgb(0.96, 0.25, 0.18);
pub(super) const CLOCK_RED_BRIGHT: Color = Color::srgb(1.0, 0.55, 0.25);

/// mm:ss for a remaining-tick count.
pub(crate) fn clock_text(ticks: u64) -> String {
    let secs = ticks / u64::from(crate::sim::TICKS_PER_SECOND);
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// How much of a round the screen draws as its closing emergency.
///
/// `band` is what that stretch is worth on a round with a middle to it: 30 s
/// of red, the last 10 s of it blinking. A round shorter than the band has
/// no middle, and the fixed figure then covers the whole of it - which is
/// how every timed level in the game came to be drawn as one long panic.
/// The longest round any level file asks for is 900 ticks, exactly
/// [`crate::sim::SURGE_TICKS`], so *no* shipped level ever left the red.
/// Dry Feet, at 240 ticks, ran all eight of its seconds under a blinking
/// clock and heaving water, on a level about walking round a puddle.
///
/// Below the band the last third stands in for it. Versus rounds are two
/// minutes and up and keep the fixed 30 s exactly as before.
///
/// This is the *drawn* scramble only. [`crate::sim::Board::in_surge`] is
/// the sim's own 30 s rule - it doubles the gull spawn rate and gates tide
/// events - and it stays where it is: moving it would move every replay.
pub(crate) fn urgency_band(round: Option<u32>, band: u32) -> u64 {
    match round {
        Some(round) if round <= band => u64::from(round) / 3,
        _ => u64::from(band),
    }
}

/// The clock colour ramp: calm, red inside the closing band, blinking
/// half-seconds for the last third of that. `blink` is off under reduced
/// motion, where the final stretch simply holds the bright red instead of
/// flashing. `round` is the board's round length, absent on an untimed
/// puzzle counting down the campaign tick limit.
pub(crate) fn clock_color(ticks: u64, round: Option<u32>, elapsed: f32, blink: bool) -> Color {
    const BLINK_TICKS: u64 = 10 * crate::sim::TICKS_PER_SECOND as u64;
    let red = urgency_band(round, crate::sim::SURGE_TICKS);
    // The blink is the last third of the red, and never more than the ten
    // seconds it is worth on a long round: the two have to stay in that
    // order, or a short round starts blinking the instant it turns red.
    if ticks <= BLINK_TICKS.min(red / 3) {
        if !blink || ((elapsed * 2.0) as u32).is_multiple_of(2) {
            CLOCK_RED
        } else {
            CLOCK_RED_BRIGHT
        }
    } else if ticks <= red {
        CLOCK_RED
    } else {
        CLOCK_CALM
    }
}

/// What the three shared HUD slots say on a screen: the header's left
/// side, its right side, and the prompt pill along the bottom.
///
/// A struct and not a `(String, String, String)`, which is what six
/// functions here used to hand back. Every one of them built the three in
/// a different order internally, and nothing but position said which was
/// which: a screen that swapped its status and its prompt would compile,
/// run, and read as a screen whose header had gone strange.
pub(super) struct HudText {
    /// Left of the header bar: where you are.
    pub title: String,
    /// Right of the header bar: how the round stands.
    pub status: String,
    /// The pill along the bottom: which keys do what, here, now.
    pub prompt: String,
}

impl HudText {
    fn new(
        title: impl Into<String>,
        status: impl Into<String>,
        prompt: impl Into<String>,
    ) -> HudText {
        HudText {
            title: title.into(),
            status: status.into(),
            prompt: prompt.into(),
        }
    }
}

pub(super) fn lobby_text(tr: &Tr, lobby: &LobbyState) -> HudText {
    let title = tr.title_lobby.to_string();
    let status = lobby.feedback.clone();
    use crate::app::lobby::Standing;
    let prompt = match lobby.standing() {
        // W armed is the one bit of lobby state a player has to be told
        // about, since it changes what picking a beach does.
        Standing::ChoosingToWatch => tr.lobby_watch_armed.to_string(),
        Standing::Joining => tr.lobby_aboard_prompt.to_string(),
        Standing::Hosting => tr.lobby_broadcasting.to_string(),
        Standing::Choosing if lobby.hosts.is_empty() => tr.lobby_none_yet.to_string(),
        Standing::Choosing => {
            // The beaches themselves are a list of their own now, spawned by
            // the lobby: this line says what to do with it, and how many there
            // are, which the visible rows may not show all of.
            match lobby.hosts.len() {
                1 => tr.lobby_join_list_one.to_string(),
                n => fill(tr.lobby_join_list, &[("n", &n.to_string())]),
            }
        }
    };
    HudText::new(title, status, prompt)
}

pub(super) fn editor_text(tr: &Tr, editor: &EditorState, sim: &Sim) -> HudText {
    let testing = editor.testing.is_some();
    // The title carries the level's name, because the name is now the file
    // it saves to and the caption the stage list will show: it has to be
    // somewhere a player can see it before pressing F2.
    // What is being built is as much a part of the title as what it is
    // called: the two kinds save to different lists, and an author who
    // finds that out at the map dial found out too late.
    let kind = match editor.kind {
        crate::sim::LevelKind::Puzzle => tr.ed_kind_puzzle,
        crate::sim::LevelKind::Arena => tr.ed_kind_arena,
    };
    let title = if testing {
        tr.title_playtest.to_string()
    } else {
        // The kind sits in front of the name, not after it: the caret that
        // shows the keyboard is spelling a name has to stay on the end of
        // the thing being spelled.
        format!(
            "{} [{kind}] - {}{}",
            tr.title_editor,
            editor.name,
            if editor.naming { "_" } else { "" }
        )
    };
    // A beach is not played with a granted inventory, so the number beside
    // the feedback is the one that decides whether it can be played at all:
    // how many seats it has castles for.
    let counted = match editor.kind {
        crate::sim::LevelKind::Puzzle => fill(tr.ed_posts, &[("n", &editor.posts.to_string())]),
        crate::sim::LevelKind::Arena => {
            fill(tr.ed_seats, &[("n", &sim.0.castle_seats().to_string())])
        }
    };
    let status = fill(&counted, &[("msg", &editor.feedback)]);
    let prompt = if testing {
        tr.ed_playtest_prompt.to_string()
    } else {
        tr.ed_prompt.to_string()
    };
    HudText::new(title, status, prompt)
}

pub(super) fn puzzle_text(
    tr: &Tr,
    lang: crate::app::i18n::Lang,
    campaign: &Campaign,
    sim: &Sim,
    phase: &State<Phase>,
    custom_keys: bool,
) -> HudText {
    let level = campaign.current();
    let campaign_title = match campaign.kind {
        CampaignKind::TidePool => tr.title_tide_pool,
        CampaignKind::BeachDay => tr.title_beach_day,
    };
    let title = format!(
        "{} {}/{} - {}",
        campaign_title,
        campaign.index + 1,
        campaign.levels.len(),
        lang.level_name(&level.name)
    );
    let used = sim.0.signpost_count(0);
    let saved = fill(
        tr.saved_count,
        &[
            ("a", &sim.0.crabs_banked().to_string()),
            (
                "b",
                &sim.0.crabs_spawned().max(level.crab_count()).to_string(),
            ),
        ],
    );
    // What the level asks for. The default goal has nothing to state beyond
    // the tally of crabs already home; the rest name their own target.
    let goal = match level.goal {
        Goal::AllCrabs => saved,
        Goal::Bank(n) => fill(
            tr.goal_bank,
            &[
                ("a", &sim.0.crabs_banked().to_string()),
                ("b", &n.to_string()),
                ("t", ""),
            ],
        ),
        Goal::Survive => fill(tr.goal_survive, &[("t", "")]),
        Goal::Golden => fill(tr.goal_golden, &[("t", "")]),
    };
    // The inventory goes in front of it, on every goal. It used to be
    // printed only under `AllCrabs`, so nine campaign levels and all eight
    // Beach Day stages named a target and never said how many signposts
    // paid for it - and players read a missing number as "as many as I
    // like". A level that hands out none says so in the prompt instead.
    let status = match level.posts {
        0 => goal,
        posts => format!(
            "{} | {goal}",
            fill(
                tr.signposts_count,
                &[("a", &used.to_string()), ("b", &posts.to_string())]
            )
        ),
    };
    let prompt = match phase.get() {
        Phase::Setup if level.posts == 0 => tr.prompt_setup_no_posts.to_string(),
        Phase::Setup if used >= level.posts as usize => tr.prompt_setup_full.to_string(),
        Phase::Setup if custom_keys => tr.prompt_setup_custom.to_string(),
        Phase::Setup => tr.prompt_setup.to_string(),
        Phase::Running => tr.prompt_running.to_string(),
        // On the last level the card says the run is over and Enter goes
        // home; a prompt line still offering "next level" under it is the
        // game arguing with itself.
        Phase::Won if campaign.index + 1 == campaign.levels.len() => tr.last_level.to_string(),
        Phase::Won => tr.prompt_won.to_string(),
        Phase::Lost => tr.prompt_lost.to_string(),
    };
    HudText::new(title, status, prompt)
}

/// The busiest screen there is, and the one that reads off the most: it
/// takes the whole [`Readout`] rather than ten of its fields, which had
/// grown past the point where the order of two `&Res`es of the same shape
/// was checked by anything but eyesight.
pub(super) fn versus_text(r: &Readout) -> HudText {
    let Readout {
        tr,
        sim,
        seats,
        settings,
        names,
        playback,
        online,
        bots,
        vphase,
        tournament,
        speed,
        ..
    } = *r;
    let scores = sim.0.scores();
    let team_mode = crate::app::teams::in_play(settings, online, seats.0);
    let mut mode = if playback.0.is_some() {
        tr.title_replay.to_string()
    } else if let Some(session) = &online.0 {
        match session.session.seat() {
            Some(seat) => fill(tr.title_online, &[("p", &names.label(tr, seat))]),
            None => tr.title_watching.to_string(),
        }
    } else if bots.0.iter().any(Option::is_some) {
        let count = bots.0.iter().filter(|b| b.is_some()).count();
        fill(
            tr.title_vs_ai,
            &[("n", &count.to_string()), ("p", &names.label(tr, 0))],
        )
    } else {
        tr.title_turf_war.to_string()
    };
    if team_mode != TeamMode::Solo {
        // Every team's total, in team order, so a 2v2v2 reads as three
        // numbers rather than two.
        let totals = crate::app::teams::team_scores(scores, seats.0, team_mode)
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join("  vs  ");
        mode = fill(tr.team_banner, &[("mode", &mode), ("s", &totals)]);
    }
    // The clock itself lives in the big top-centre element; the
    // status slot carries only banners.
    let mut status = if playback.0.is_some() && speed > 1 {
        fill(tr.replay_speed, &[("n", &speed.to_string())])
    } else if sim.0.in_surge() && !sim.0.round_over() {
        tr.the_gulls.to_string()
    } else {
        String::new()
    };
    if let Some((event, at)) = sim.0.last_event()
        && sim.0.ticks().saturating_sub(at) < 90
    {
        status = fill(tr.tide_event, &[("e", event_name(tr, event))]);
    }
    if let Some((owner, remaining)) = sim.0.lure() {
        let seat = names.label(tr, owner);
        // First moments name the trigger; then a live countdown.
        status = if remaining > LURE_TICKS - 60 {
            fill(tr.lure_started, &[("p", &seat)])
        } else {
            fill(
                tr.lure_banner,
                &[("s", &remaining.div_ceil(30).to_string()), ("p", &seat)],
            )
        };
    }
    if let Some(session) = &online.0 {
        // Whose fault the still picture is, in the order the answer is
        // worth having: a desync is the round being wrong, an empty socket
        // is nobody there yet, and a stall is somebody in particular.
        if let Some(frame) = session.desync_at {
            status = fill(tr.desync, &[("f", &frame.to_string())]);
        } else if !session.transport.connected() {
            status = tr.waiting_peer.to_string();
        } else if let Some(seat) = session.waiting_on() {
            // Decided once a frame by the session rather than read off the
            // stall clock here: at three frames of input delay a moment's
            // wait is the ordinary rhythm of the thing, and a line that
            // came and went with each of them was a strobe in the corner
            // of the eye.
            status = fill(tr.waiting_for, &[("p", &names.label(tr, seat))]);
        }
    }
    let prompt = match vphase.get() {
        // A replay - or someone else's match - is watched, not played: no
        // control legend for a seat you do not have.
        VersusPhase::Running
            if playback.0.is_some() || online.0.as_ref().is_some_and(|s| s.session.watching()) =>
        {
            tr.prompt_enter_menu.to_string()
        }
        VersusPhase::Running if online.0.is_some() || bots.0.iter().any(Option::is_some) => {
            tr.prompt_versus_short.to_string()
        }
        VersusPhase::Running if settings.custom_binds() => tr.prompt_versus_custom.to_string(),
        VersusPhase::Running => tr.prompt_versus_local.to_string(),
        // A finished lobby match goes back to the lobby, together; the
        // prompt must not promise the menu it will not reach. Mid-series
        // the card's own "Enter: next round" hint speaks instead.
        VersusPhase::Over
            if online.0.as_ref().is_some_and(|session| session.from_lobby)
                && !(tournament.active && !tournament.finished) =>
        {
            tr.prompt_enter_lobby.to_string()
        }
        VersusPhase::Over => tr.prompt_enter_menu.to_string(),
    };
    HudText::new(mode, status, prompt)
}

pub(crate) fn event_name(tr: &Tr, event: TideEvent) -> &'static str {
    tr.events[event.index()]
}

/// Everything the header and prompt line can draw on. Bundled so the
/// per-screen text is a pure function rather than a match inside a system
/// with fourteen resources - and so a test can ask every screen what it says.
pub(super) struct Readout<'a> {
    pub tr: &'static Tr,
    pub lang: crate::app::i18n::Lang,
    pub sim: &'a Sim,
    pub campaign: &'a Campaign,
    pub phase: &'a State<Phase>,
    pub vphase: &'a State<VersusPhase>,
    pub editor: &'a EditorState,
    pub online: &'a Online,
    pub playback: &'a Playback,
    pub lobby: &'a LobbyState,
    /// Whether a series is mid-flight, which changes what the results
    /// card's Enter does and so what the prompt may promise.
    pub tournament: &'a crate::app::tournament::Tournament,
    pub seats: &'a Seats,
    pub settings: &'a GameSettings,
    pub names: &'a crate::app::SeatNames,
    pub bots: &'a Bots,
    pub library: &'a crate::app::replays::Library,
    /// What the menu has to say about a round code copied or pasted.
    pub notice: &'a crate::app::RoundNotice,
    /// Which row the match setup is on: Enter means something else on the
    /// name rows, and the prompt line has to say so.
    pub match_menu: &'a crate::app::match_setup::MatchMenu,
    pub speed: u8,
}

/// What the header, the status slot and the prompt line say on `screen`.
/// What the header, the status slot and the prompt say on `screen`.
///
/// The music toggle is added here rather than written into each play
/// screen's prompt: there are eight of those in every language, and a
/// key that works everywhere should not be a line eight strings have to
/// remember to carry.
pub(super) fn screen_text(screen: Screen, r: &Readout) -> HudText {
    let mut said = screen_text_for(screen, r);
    if matches!(screen, Screen::Versus | Screen::Puzzle) {
        said.prompt = format!("{} | {}", said.prompt, r.tr.prompt_mute);
    }
    // Spelled in this keyboard's caps here, once, rather than in each of
    // the legends that name the move keys.
    said.prompt = r.settings.keycaps.legend(&said.prompt);
    said
}

pub(super) fn screen_text_for(screen: Screen, r: &Readout) -> HudText {
    match screen {
        // The menu's status slot carries word of a code that would not
        // load: the only news the menu ever has.
        Screen::Menu => HudText::new(
            String::new(),
            r.notice.0.clone(),
            r.tr.menu_prompt.to_string(),
        ),
        Screen::Settings => HudText::new(
            r.tr.title_settings.to_string(),
            String::new(),
            r.tr.prompt_settings.to_string(),
        ),
        Screen::Controls => HudText::new(
            r.tr.title_controls.to_string(),
            String::new(),
            r.tr.prompt_controls.to_string(),
        ),
        Screen::MatchSetup => HudText::new(
            r.tr.title_match_setup.to_string(),
            String::new(),
            if matches!(
                crate::app::match_setup::Row::ALL[r.match_menu.selected],
                crate::app::match_setup::Row::Name(_)
            ) {
                r.tr.prompt_match_name.to_string()
            } else {
                r.tr.prompt_match_setup.to_string()
            },
        ),
        Screen::Achievements => HudText::new(
            r.tr.title_achievements.to_string(),
            String::new(),
            r.tr.prompt_esc_menu.to_string(),
        ),
        Screen::Replays => HudText::new(
            r.tr.title_replays.to_string(),
            r.library.feedback.clone(),
            r.tr.prompt_replays.to_string(),
        ),
        Screen::StageSelect => HudText::new(
            format!(
                "{} - {}",
                match r.campaign.kind {
                    CampaignKind::TidePool => r.tr.title_tide_pool,
                    CampaignKind::BeachDay => r.tr.title_beach_day,
                },
                r.tr.title_stages
            ),
            String::new(),
            r.tr.prompt_stages.to_string(),
        ),
        Screen::Interlude => HudText::new(
            r.tr.title_turf_war.to_string(),
            String::new(),
            String::new(),
        ),
        Screen::Lobby => lobby_text(r.tr, r.lobby),
        Screen::Editor => editor_text(r.tr, r.editor, r.sim),
        Screen::Puzzle => puzzle_text(
            r.tr,
            r.lang,
            r.campaign,
            r.sim,
            r.phase,
            !r.settings.stock_legend(),
        ),
        // Both lines are already in the language under the cursor: moving
        // it sets the language, so the header and the prompt are the
        // preview of whatever is highlighted. The status slot stays empty
        // - the note that belongs there sits under the card instead,
        // beside the list rather than in the far corner of the header.
        Screen::Language => HudText::new(
            r.tr.title_pick_language.to_string(),
            String::new(),
            r.tr.prompt_pick_language.to_string(),
        ),
        Screen::NewVersion => HudText::new(
            r.tr.title_new_version.to_string(),
            String::new(),
            r.tr.prompt_new_version.to_string(),
        ),
        Screen::Versus => versus_text(r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::i18n::{EN, Lang};
    use crate::sim::{Board, Level, campaign_levels};

    /// Every screen has to say what it is and what the keys do. A screen
    /// added without a HUD arm would otherwise show the last screen's
    /// header, which is the kind of thing nobody notices until a player
    /// does.
    #[test]
    fn every_screen_names_itself_and_its_keys() {
        use crate::app::{Bots, Campaign, CampaignKind, Playback, Seats};
        use crate::sim::{Board, campaign_levels};

        let levels = campaign_levels();
        let builtins = levels.len();
        let campaign = Campaign {
            kind: CampaignKind::TidePool,
            levels,
            index: 0,
            builtins,
        };
        let settings = GameSettings::default();
        let readout = Readout {
            tr: &EN,
            lang: Lang::En,
            sim: &Sim(Board::new(9, 7, 1)),
            campaign: &campaign,
            phase: &State::new(Phase::Setup),
            vphase: &State::new(VersusPhase::Running),
            editor: &EditorState::default(),
            online: &Online::default(),
            playback: &Playback::default(),
            lobby: &LobbyState::default(),
            tournament: &crate::app::tournament::Tournament::default(),
            seats: &Seats(2),
            settings: &settings,
            names: &crate::app::SeatNames::default(),
            bots: &Bots::default(),
            library: &crate::app::replays::Library::default(),
            notice: &crate::app::RoundNotice::default(),
            match_menu: &crate::app::match_setup::MatchMenu::default(),
            speed: 1,
        };
        for screen in Screen::ALL {
            let said = screen_text(screen, &readout);
            let (title, prompt) = (&said.title, &said.prompt);
            // The menu is the one screen with no header: it is a postcard,
            // and its own art says where you are.
            if screen != Screen::Menu {
                assert!(!title.is_empty(), "{screen:?} has no header");
            }
            // The interlude is a between-rounds breather with nothing to
            // press; everything else tells you a key.
            if screen != Screen::Interlude {
                assert!(!prompt.is_empty(), "{screen:?} offers no keys");
            }
        }
    }

    /// The editor header names the mode it is in and carries the solver's
    /// last word; playtesting swaps both the title and the key legend.
    #[test]
    fn the_editor_header_follows_the_playtest_toggle() {
        let mut editor = EditorState::default();
        editor.posts = 3;
        editor.name = "Gull Alley".into();
        editor.feedback = "solvable: 2".into();
        // Two castles on the sand, so the beach reading has something to
        // count when the kind is flipped.
        let mut board = Board::new(9, 7, 1);
        board.set_tile(0, 0, crate::sim::TileKind::Castle(0));
        board.set_tile(8, 6, crate::sim::TileKind::Castle(1));
        let sim = Sim(board);
        let HudText {
            title,
            status,
            prompt,
        } = editor_text(&EN, &editor, &sim);
        assert!(title.starts_with(EN.title_editor), "{title}");
        assert!(title.contains("Gull Alley"), "the name is on the header");
        // And what is being built, which decides which list it saves to.
        assert!(title.contains(EN.ed_kind_puzzle), "{title}");
        assert!(status.contains('3') && status.contains("solvable: 2"));
        assert_eq!(prompt, EN.ed_prompt);

        // As a beach the status counts seats instead of an inventory the
        // match will never read.
        editor.kind = crate::sim::LevelKind::Arena;
        let beach = editor_text(&EN, &editor, &sim);
        assert!(beach.title.contains(EN.ed_kind_arena), "{}", beach.title);
        assert_eq!(beach.status, "seats: 2 | solvable: 2", "{}", beach.status);
        editor.kind = crate::sim::LevelKind::Puzzle;

        // Typing shows a caret, so it is obvious the keyboard is spelling a
        // name rather than picking brushes.
        editor.naming = true;
        let typing = editor_text(&EN, &editor, &sim).title;
        assert!(typing.ends_with('_'), "{typing}");
        editor.naming = false;

        editor.testing = Some(Board::new(4, 4, 0));
        let HudText { title, prompt, .. } = editor_text(&EN, &editor, &sim);
        assert_eq!(title, EN.title_playtest);
        assert_eq!(prompt, EN.ed_playtest_prompt);
    }

    /// The lobby prompt is a small state machine: aboard, broadcasting,
    /// nothing found yet, or how to work the list of beaches. The beaches
    /// themselves are rows of their own; see `HostEntry::label`.
    #[test]
    fn the_lobby_prompt_tracks_the_connection_state() {
        let mut lobby = LobbyState::default();
        assert_eq!(lobby_text(&EN, &lobby).prompt, EN.lobby_none_yet);

        for at in 1..=3u8 {
            lobby.hosts.push(crate::app::lobby::HostEntry {
                addr: format!("10.0.0.{at}:47777").parse().expect("addr"),
                id: u64::from(at),
                name: String::new(),
                host: String::new(),
                taken: 1,
                seats: 6,
                running: false,
                age: 0.0,
            });
        }
        // The count is the part that matters: a hall with more games than
        // rows should not look like it has only as many as fit.
        let prompt = lobby_text(&EN, &lobby).prompt;
        assert!(prompt.contains('3'), "how many are out there: {prompt}");

        lobby.feedback = "hosting on port 47777".into();
        assert_eq!(lobby_text(&EN, &lobby).status, "hosting on port 47777");
    }

    /// The puzzle header counts what the level asks for, and the prompt
    /// changes when the inventory runs out: the difference between "place
    /// your signposts" and "you have none left, press Enter".
    #[test]
    fn the_puzzle_prompt_reacts_to_a_spent_inventory() {
        let levels = campaign_levels();
        let builtins = levels.len();
        let campaign = Campaign {
            kind: CampaignKind::TidePool,
            levels,
            index: 0,
            builtins,
        };
        let level: &Level = campaign.current();
        let posts = level.posts;
        let mut sim = Sim(level.board());
        let phase = State::new(Phase::Setup);

        let HudText { title, prompt, .. } =
            puzzle_text(&EN, Lang::En, &campaign, &sim, &phase, false);
        assert!(title.starts_with(EN.title_tide_pool), "{title}");
        assert!(
            title.contains("1/"),
            "the level's place in the list: {title}"
        );
        assert_eq!(
            prompt,
            if posts == 0 {
                EN.prompt_setup_no_posts
            } else {
                EN.prompt_setup
            }
        );

        // Spend the inventory: the prompt switches to the "full" advice.
        if posts > 0 {
            let mut placed = 0;
            'fill: for y in 0..sim.0.height() {
                for x in 0..sim.0.width() {
                    if placed == posts {
                        break 'fill;
                    }
                    if sim.0.place_signpost(0, x, y, crate::sim::Direction::Up) {
                        placed += 1;
                    }
                }
            }
            let prompt = puzzle_text(&EN, Lang::En, &campaign, &sim, &phase, false).prompt;
            assert_eq!(prompt, EN.prompt_setup_full);
        }

        // Rebound keys retire the stock legend rather than teach wrong keys.
        let prompt =
            puzzle_text(&EN, Lang::En, &campaign, &Sim(level.board()), &phase, true).prompt;
        assert_eq!(prompt, EN.prompt_setup_custom);
        // And so does the one-hand preset: placement is on IJKL then, not
        // the arrows the stock legend names.
        let one_hand = crate::app::settings::GameSettings {
            ijkl_commits: true,
            ..crate::app::settings::GameSettings::default()
        };
        assert!(!one_hand.stock_legend());
        assert!(crate::app::settings::GameSettings::default().stock_legend());
    }

    /// Every tide event maps to a distinct, non-empty localized name; an
    /// off-by-one here mislabels every banner and log line.
    #[test]
    fn every_tide_event_has_a_distinct_name() {
        let names: Vec<&str> = TideEvent::ALL
            .iter()
            .map(|&event| event_name(&EN, event))
            .collect();
        for name in &names {
            assert!(!name.is_empty());
        }
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            names.len(),
            "duplicate event names: {names:?}"
        );
    }

    #[test]
    fn clock_formats_minutes_and_seconds() {
        let tps = u64::from(crate::sim::TICKS_PER_SECOND);
        assert_eq!(clock_text(0), "0:00");
        assert_eq!(clock_text(29 * tps), "0:29");
        assert_eq!(clock_text(90 * tps), "1:30");
        assert_eq!(clock_text(600 * tps), "10:00");
    }

    /// The clock reddens inside the last 30 seconds and blinks for the
    /// last 10, unless the player asked for less motion, where it holds.
    /// A long round - versus, or the untimed puzzle backstop - is what
    /// those flat figures are for, and they are unchanged on one.
    #[test]
    fn the_clock_reddens_then_blinks() {
        let tps = u64::from(crate::sim::TICKS_PER_SECOND);
        let long = Some(3 * 60 * crate::sim::TICKS_PER_SECOND);
        for round in [None, long] {
            assert_eq!(clock_color(60 * tps, round, 0.0, true), CLOCK_CALM);
            assert_eq!(clock_color(20 * tps, round, 0.0, true), CLOCK_RED);
            // Inside ten seconds the colour alternates with the wall clock.
            assert_eq!(clock_color(5 * tps, round, 0.0, true), CLOCK_RED);
            assert_eq!(clock_color(5 * tps, round, 0.5, true), CLOCK_RED_BRIGHT);
            // Reduced motion: red, but steady.
            assert_eq!(clock_color(5 * tps, round, 0.5, false), CLOCK_RED);
        }
    }

    /// A round shorter than the flat band still has a calm stretch.
    ///
    /// Every timed level in the game is shorter than [`SURGE_TICKS`] - the
    /// longest `round:` any file asks for is 900, which is that figure
    /// exactly - so under the old flat rule not one of them ever showed a
    /// calm clock. Dry Feet is 240 ticks and was red and blinking from its
    /// first frame to its last.
    #[test]
    fn a_short_round_is_not_one_long_emergency() {
        let dry_feet = Some(240);
        // Two thirds of the way in, still calm; the last third reddens and
        // the very end of that blinks.
        assert_eq!(clock_color(200, dry_feet, 0.0, true), CLOCK_CALM);
        assert_eq!(clock_color(100, dry_feet, 0.0, true), CLOCK_CALM);
        assert_eq!(clock_color(60, dry_feet, 0.0, true), CLOCK_RED);
        assert_eq!(clock_color(20, dry_feet, 0.5, true), CLOCK_RED_BRIGHT);
        // The red always arrives before the blink, on any round length.
        for round in [240u32, 300, 900, 1800, 3600] {
            let red = urgency_band(Some(round), crate::sim::SURGE_TICKS);
            let blink = (0..=round)
                .rev()
                .map(u64::from)
                .find(|&t| clock_color(t, Some(round), 0.5, true) == CLOCK_RED_BRIGHT);
            assert!(
                blink.is_some_and(|blink| blink < red),
                "round {round}: reddens at {red}, blinks at {blink:?}"
            );
        }
    }

    /// The flat band is what a round long enough to have a middle gets;
    /// a shorter one gets its last third instead.
    #[test]
    fn the_urgency_band_shrinks_only_for_short_rounds() {
        assert_eq!(urgency_band(None, 900), 900);
        assert_eq!(urgency_band(Some(3600), 900), 900);
        assert_eq!(urgency_band(Some(901), 900), 900);
        // At and below the band, the last third of the round.
        assert_eq!(urgency_band(Some(900), 900), 300);
        assert_eq!(urgency_band(Some(240), 900), 80);
    }
}
