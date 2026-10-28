//! What the header and the prompt line say, per screen.
//!
//! Pure functions of game state and the string table: no ECS, no spawning,
//! which makes them testable, and they are every player-facing line in
//! the game, so they are worth testing.

use crate::app::editor::EditorState;
use crate::app::gamepad::scheme::{self, Face, LastDevice, Pad, PadPick};
use crate::app::i18n::{Tr, fill};
use crate::app::lobby::LobbyState;
use crate::app::net::Online;
use crate::app::seating::Viewer;
use crate::app::settings::{GameSettings, KeyLegend};
use crate::app::teams::TeamMode;
use crate::app::{Campaign, Controllers, Phase, Playback, Screen, Seats, Sim, VersusPhase};
use crate::sim::{Goal, LURE_TICKS, TICKS_PER_SECOND, TideEvent};
use bevy::prelude::*;

/// What the three shared HUD slots say on a screen: the header's left
/// side, its right side, and the prompt pill along the bottom.
///
/// A struct and not a `(String, String, String)`: with nothing but
/// position to say which is which, a screen that swapped its status and
/// its prompt would compile and run.
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

/// The prompt while a box has the keyboard. Every key the screen's own line
/// names is a letter until the box closes, so that line would be promising
/// keys that type instead: on the lobby's Enter, a line sent and not a round
/// started. `esc_cancels` says whether Esc throws the line away or, like
/// every other way out, keeps it.
fn typing_prompt(tr: &Tr, esc_cancels: bool) -> String {
    match esc_cancels {
        true => tr.prompt_typing.to_string(),
        false => tr.prompt_typing_keep.to_string(),
    }
}

/// The match setup's keys: a name row's Tab, or the dials'.
/// The keyboard's line on match setup: each step's own keys, and while a
/// seat's name is typed, the name box's (W and S are letters there, and
/// Enter, Esc and Tab all keep it and put the box away: `type_a_name`).
/// Under a team mode step 3's line ends on the partner key, which the
/// pad's face offers on the same condition (`face`'s `partners`).
fn match_setup_prompt(tr: &Tr, menu: &crate::app::match_setup::MatchMenu, face: &Face) -> String {
    use crate::app::match_setup::Step;
    if menu.naming.is_some() {
        return typing_prompt(tr, false);
    }
    let line = match menu.step {
        Step::Game => tr.prompt_setup_game,
        Step::Map => tr.prompt_setup_map,
        // A/D said only where it turns the seat: not on player 1's row.
        Step::Players => match crate::app::match_setup::seat_turns(menu.seat) {
            true => tr.prompt_setup_players,
            false => tr.prompt_setup_players_p1,
        },
    };
    let partners =
        menu.step == Step::Players && matches!(face, Face::MatchSetup { partners: true, .. });
    match partners {
        true => format!("{line} | {}", tr.prompt_setup_partners),
        false => line.to_string(),
    }
}

/// `map_pick` whether the host's map page has its cursor on a card Enter
/// picks ([`crate::app::match_setup::card_to_pick`]): elsewhere on it the
/// prompt does not offer Enter, which does nothing there.
pub(super) fn lobby_text(tr: &Tr, lobby: &LobbyState, map_pick: bool) -> HudText {
    let title = tr.title_lobby.to_string();
    let status = lobby.feedback.clone();
    use crate::app::lobby::Standing;
    // A line being typed takes every key (`drive_typing`), and Esc closes
    // the box without sending it.
    if lobby.typing.is_some() {
        return HudText::new(title, status, typing_prompt(tr, true));
    }
    let prompt = match lobby.standing() {
        // W armed is the one bit of lobby state a player has to be told
        // about, since it changes what picking a beach does.
        Standing::Choosing { watching: true } => tr.lobby_watch_armed.to_string(),
        // A peer in line is not waiting for a launch either: the round it
        // is queued for is the one already being played without it. And
        // one that came to watch is not in line for a chair, so it is not
        // told it will be sitting in one.
        Standing::Joining(joined) if joined.queued.is_some() => match joined.watching {
            true => tr.lobby_queued_watch_prompt.to_string(),
            false => tr.lobby_queued_prompt.to_string(),
        },
        // "Aboard" is the joiner's word, and a spectator is not: the
        // status line right above this one says "watching", and the two
        // of them describing the same peer differently is the whole of
        // what a player has to go on before the round starts.
        Standing::Joining(joined) if joined.watching => tr.lobby_watching_prompt.to_string(),
        // Past the last chair: the launch seats it watching, and the
        // status line above says so.
        Standing::Joining(joined) if joined.no_chair => tr.lobby_no_chair_prompt.to_string(),
        Standing::Joining(_) => tr.lobby_aboard_prompt.to_string(),
        // The map page, once the host has started the match.
        Standing::Hosting(hosted) if hosted.picking.is_some() => match map_pick {
            true => tr.lobby_pick_prompt.to_string(),
            false => tr.lobby_pick_prompt_no_pick.to_string(),
        },
        // Picking whom to ask to leave: the prompt says how.
        Standing::Hosting(crate::app::lobby::Hosted {
            peers,
            kicking: Some(page),
            ..
        }) => crate::app::lobby::kick_prompt(tr, peers, *page),
        // Picking partners: the seat numbers name whose partners change.
        Standing::Hosting(hosted) if hosted.partnering => {
            let seats = lobby.table.len() + usize::from(lobby.seated.ai);
            fill(tr.lobby_partner_prompt, &[("n", &seats.to_string())])
        }
        // Under a team mode, P picks partners.
        Standing::Hosting(_) => match crate::app::lobby::partners_open(lobby) {
            true => format!("{} | {}", tr.lobby_broadcasting, tr.prompt_setup_partners),
            false => tr.lobby_broadcasting.to_string(),
        },
        Standing::Choosing { watching: false } if lobby.hosts.is_empty() => {
            tr.lobby_none_yet.to_string()
        }
        Standing::Choosing { watching: false } => {
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

/// The editor's lines. Its prompt names the move keys as `{moves}` in
/// place of the language's "WASD": the cursor walks on P1's bindings,
/// which `editor::spell_editor_keys` spells once the legend has run.
pub(super) fn editor_text(
    tr: &Tr,
    editor: &EditorState,
    board: &crate::sim::Board,
    settings: &GameSettings,
) -> HudText {
    let testing = editor.is_testing();
    // The title carries the level's name, which is the file it saves to
    // and the caption the stage list shows, so it has to be visible before
    // F2. What is being built belongs there too: the two kinds save to
    // different lists, and finding that out at the map dial is too late.
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
            if editor.is_naming() { "_" } else { "" }
        )
    };
    // What was last said, and nothing else: the signposts, the seats, the
    // goal and every other setting are rows of the settings card, there
    // the whole time rather than only after the key that turned them.
    // A playtest shows what its player would see instead: a stage's
    // arrows and goal, a beach's scores and time.
    let status = match editor.test() {
        Some(test) => crate::app::editor::test_status(tr, test, board),
        None => editor.feedback.clone(),
    };
    let prompt = match editor.test() {
        // F1's box has the keyboard: Enter keeps the name, Esc puts the
        // old one back (`type_a_name`).
        _ if editor.is_naming() => typing_prompt(tr, true),
        // "My levels" has the keys (`editor::editor_levels_input`).
        _ if matches!(editor.mode, crate::app::editor::Mode::Levels(_)) => {
            tr.ed_levels_keys.to_string()
        }
        // Any key puts a card away (`editor::editor_card_input`).
        _ if editor.card().is_some() => tr.ed_card_close.to_string(),
        Some(test) => crate::app::editor::test_prompt(tr, test, settings),
        None => tr.ed_prompt.replacen("WASD", "{moves}", 1),
    };
    HudText::new(title, status, prompt)
}

/// A stage's status line: the arrows standing of those it grants, and
/// its goal with the tally toward it. The stage list's own run and the
/// editor's playtest of a stage both say it this way.
pub(crate) fn puzzle_status(
    tr: &Tr,
    level: &crate::sim::Level,
    board: &crate::sim::Board,
) -> String {
    let used = board.signpost_count(0);
    let saved = fill(
        tr.saved_count,
        &[
            ("a", &board.crabs_banked().to_string()),
            (
                "b",
                &board.crabs_spawned().max(level.crab_count()).to_string(),
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
                ("a", &board.crabs_banked().to_string()),
                ("b", &n.to_string()),
                ("t", ""),
            ],
        ),
        Goal::Survive => fill(tr.goal_survive, &[("t", "")]),
        Goal::Golden => fill(tr.goal_golden, &[("t", "")]),
    };
    // The inventory goes in front of it, on every goal. Printed only under
    // `AllCrabs`, nine campaign levels named a target without saying how many signposts paid for it, which reads
    // as "as many as I like". A level that hands out none says so in the
    // prompt instead.
    if let Some([left, right]) = level.hands() {
        let count = fill(
            &crate::app::name_the_pair(tr, tr.hands_count),
            &[
                ("a", &board.signpost_count(0).to_string()),
                ("b", &left.to_string()),
                ("c", &board.signpost_count(1).to_string()),
                ("d", &right.to_string()),
            ],
        );
        return format!("{count} | {goal}");
    }
    match level.posts {
        0 => goal,
        posts => format!(
            "{} | {goal}",
            fill(
                tr.signposts_count,
                &[("a", &used.to_string()), ("b", &posts.to_string())]
            )
        ),
    }
}

/// Whether every arrow a stage grants is down, every hand's on a co-op
/// stage, which the stage's `posts` totals: the "all arrows placed" line,
/// the keyboard's and the pad's. Only under a fixed inventory, since on a
/// level whose rule evicts the next arrow replaces the oldest.
fn arrows_all_down(level: &crate::sim::Level, board: &crate::sim::Board) -> bool {
    let hands: u8 = match level.hands() {
        Some(_) => 2,
        None => 1,
    };
    let used: usize = (0..hands).map(|seat| board.signpost_count(seat)).sum();
    used >= usize::from(level.posts) && board.signpost_rule().1 == crate::sim::CapPolicy::Reject
}

pub(super) fn puzzle_text(
    tr: &Tr,
    lang: crate::app::i18n::Lang,
    campaign: &Campaign,
    coop: bool,
    sim: &Sim,
    phase: &State<Phase>,
    keys: KeyLegend,
) -> HudText {
    let level = campaign.current();
    // The player's own levels take the stage list's own label for them in
    // place of the list's name, so driftwood three is never read as stage
    // three. In place of, and not as well as: the header bar runs from the
    // window's edge to the clock in its middle, and the longest shipped
    // level name already reaches most of the way there.
    let crate::app::campaign::Place { at, of, section } = campaign.place();
    let section = match section {
        crate::app::campaign::Section::Custom => tr.stage_custom,
        crate::app::campaign::Section::Coop => tr.stage_coop,
        crate::app::campaign::Section::TidePool => tr.title_tide_pool,
    };
    let title = format!(
        "{section} {at}/{of} - {}",
        campaign.shown_name(campaign.index, lang)
    );
    let status = puzzle_status(tr, level, &sim.0);
    let prompt = match phase.get() {
        Phase::Setup if level.posts == 0 => tr.prompt_setup_no_posts.to_string(),
        Phase::Setup if keys == KeyLegend::Own => tr.prompt_setup_custom.to_string(),
        // Two sets of hands, so both sets of keys: the solo line names one.
        Phase::Setup if coop => crate::app::name_the_pair(
            tr,
            match keys {
                KeyLegend::Swapped => tr.prompt_setup_coop_swapped,
                KeyLegend::Stock | KeyLegend::Own => tr.prompt_setup_coop,
            },
        ),
        // Full is only full under a fixed inventory. A player's level file
        // may state an evicting rule (`rule: evict`), where the next arrow
        // replaces the oldest, so "remove one first" would be wrong there.
        // After the two
        // above, because this line names the stock key, and neither a
        // rebound key nor the second pair of hands is Space.
        Phase::Setup if arrows_all_down(level, &sim.0) => tr.prompt_setup_full.to_string(),
        Phase::Setup => match keys {
            KeyLegend::Swapped => tr.prompt_setup_swapped,
            KeyLegend::Stock | KeyLegend::Own => tr.prompt_setup,
        }
        .to_string(),
        Phase::Running => tr.prompt_running.to_string(),
        // On the last level the card says the run is over and Enter goes
        // home; a prompt line still offering "next level" under it is the
        // game arguing with itself.
        Phase::Won if campaign.is_last() => tr.last_level.to_string(),
        Phase::Won if level.goal.saves_every_crab() => tr.prompt_won.to_string(),
        Phase::Won => tr.prompt_won_goal.to_string(),
        Phase::Lost => crate::app::results::loss_words(tr, level.goal)
            .1
            .to_string(),
    };
    HudText::new(title, status, prompt)
}

/// Who the header is talking to, asked the way the header has always
/// asked it: a recording playing first, then the session.
///
/// The seat questions ([`Viewer::of`]) ask the session first. The two
/// disagree only with a recording playing under a live session, and each
/// keeps the answer it has always given there: the header and the round's
/// prompt call that a replay, while the results card's prompt (which never
/// looked at the recording) and every seat question go by the session.
pub(super) fn header_viewer(online: &Online, playback: &Playback) -> Viewer {
    match playback.0 {
        Some(_) => Viewer::Replay,
        None => Viewer::of(online, false),
    }
}

/// The busiest screen there is, and the one that reads off the most: it
/// takes the whole [`Readout`] rather than ten of its fields, where the
/// order of two `&Res`es of the same shape was checked by eyesight.
pub(super) fn versus_text(r: &Readout) -> HudText {
    let Readout {
        tr,
        sim,
        seats,
        settings,
        names,
        playback,
        daily,
        online,
        controllers,
        vphase,
        tournament,
        speed,
        rematch,
        keep,
        ..
    } = *r;
    let scores = sim.0.scores();
    let replay = playback.0.as_ref().map(|(replay, _)| replay);
    let team_mode = crate::app::teams::in_play(
        crate::app::teams::local(settings, daily),
        online,
        replay,
        seats.0,
    );
    let viewer = header_viewer(online, playback);
    let mut mode = match viewer {
        Viewer::Replay => tr.title_replay.to_string(),
        Viewer::Peer(seat) => fill(tr.title_online, &[("p", &names.label(tr, seat))]),
        Viewer::Spectator => tr.title_watching.to_string(),
        // "You are P1" only when there is one "you": a couch of two humans
        // and an AI has no single player to address.
        Viewer::Table if controllers.ai_count() > 0 => {
            let n = controllers.ai_count().to_string();
            match controllers.local_count(seats.0) == 1 {
                true => fill(tr.title_vs_ai, &[("n", &n), ("p", &names.label(tr, 0))]),
                false => format!(
                    "{}{}",
                    tr.title_turf_war,
                    fill(tr.lobby_ai_seats, &[("n", &n)])
                ),
            }
        }
        Viewer::Table => tr.title_turf_war.to_string(),
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
    let mut status = if viewer == Viewer::Replay && speed > 1 {
        let next = crate::app::replays::PlaybackSpeed(speed).stepped().0;
        // The button that steps the speed, on a pad's line: the replay
        // table's, as the transport reads it.
        let template = match (
            r.device,
            scheme::button_name(r.face, scheme::Does::Speed, tr),
        ) {
            (LastDevice::Pad, Some(button)) => fill(tr.pad.replay_speed, &[("b", &button)]),
            _ => tr.replay_speed.to_string(),
        };
        fill(
            &template,
            &[("n", &speed.to_string()), ("next", &next.to_string())],
        )
    } else if sim.0.in_surge() && !sim.0.round_over() {
        tr.the_gulls.to_string()
    } else {
        String::new()
    };
    // Three seconds of the event's name, then the status slot is free.
    if let Some((event, at)) = sim.0.last_event()
        && sim.0.ticks().saturating_sub(at) < u64::from(3 * TICKS_PER_SECOND)
    {
        status = fill(tr.tide_event, &[("e", event_name(tr, event))]);
    }
    if let Some((owner, remaining)) = sim.0.lure() {
        let seat = names.label(tr, owner);
        // The first two seconds name the trigger; then a live countdown.
        status = if remaining > LURE_TICKS - 2 * TICKS_PER_SECOND {
            fill(tr.lure_started, &[("p", &seat)])
        } else {
            fill(
                tr.lure_banner,
                &[
                    ("s", &remaining.div_ceil(TICKS_PER_SECOND).to_string()),
                    ("p", &seat),
                ],
            )
        };
    }
    if let Some(session) = &online.0 {
        // Whose fault the still picture is, in the order the answer is
        // worth having: a desync is the round being wrong, an empty socket
        // is nobody there yet, and a stall is somebody in particular.
        if let Some(frame) = session.desync_at() {
            status = fill(tr.desync, &[("f", &frame.to_string())]);
        } else if !session.transport.connected() {
            status = tr.waiting_peer.to_string();
        } else if *vphase.get() == VersusPhase::Over && !session.is_host() && session.picking_map {
            // The table waits on the host's map page for the next round.
            status = tr.host_picking_next.to_string();
        } else if let Some(seat) = session.waiting_on() {
            // Decided once a frame by the session rather than read off the
            // stall clock here: at three frames of input delay a moment's
            // wait is the ordinary rhythm, and a line that came and went
            // with each of them is a strobe.
            status = fill(tr.waiting_for, &[("p", &names.label(tr, seat))]);
        }
    }
    // The crowd, in whatever room the banners leave: a table that never
    // hears anyone is watching learns it has an audience from the moment
    // that audience calls a tide event down on it. Last, so a gull, a tide
    // event or a desync always has the slot instead.
    if status.is_empty() && r.crowd.watching > 0 {
        status = fill(tr.spectator_count, &[("n", &r.crowd.watching.to_string())]);
    }
    // The count before a round shows the round's own keys: three seconds
    // of a still beach is when a table reads them.
    let prompt = match vphase.get() {
        // A recording is watched, not played, but it *is* driven: the
        // transport bar at the foot of the board takes a pause and a
        // speed, and keys nobody is told about are keys nobody presses.
        VersusPhase::Countdown | VersusPhase::Running if viewer == Viewer::Replay => {
            tr.prompt_replay_transport.to_string()
        }
        // Someone else's live match has no seat here, so no control
        // legend: a spectator holds nothing and places nothing. What they
        // do have is free hands, which is why they are the ones who talk.
        VersusPhase::Countdown | VersusPhase::Running if viewer == Viewer::Spectator => {
            match r.spectator_typing {
                Some(line) => format!("> {line}_"),
                // What the key does right now, in the crowd's own terms: a
                // vote open to join, a wait to sit out, or the offer. A
                // spectator pressing into silence could not tell which.
                //
                // None of the three on a beach whose wheel is off: the key
                // opens nothing there, and naming it would be a promise.
                None => {
                    let call = match (sim.0.events_enabled(), r.crowd.open, r.crowd.wait) {
                        (false, ..) => None,
                        (true, 0, 0) => Some(tr.spectator_call_hint.to_string()),
                        (true, 0, wait) => {
                            Some(fill(tr.spectator_wait, &[("n", &wait.to_string())]))
                        }
                        (true, open, _) => {
                            Some(fill(tr.spectator_vote_open, &[("n", &open.to_string())]))
                        }
                    };
                    // While the winner can still be called, that is the
                    // offer instead: the round's first half minute, and the
                    // one thing a spectator cannot do later. The tide's
                    // key works all the same, and is named again after.
                    let session = online.0.as_ref();
                    let open = session.map_or(0, |s| s.stands.winner.picks.open);
                    let call = match (open, session.and_then(|s| s.stands.winner.my_pick)) {
                        (0, _) => call,
                        (n, None) => Some(fill(tr.spectator_pick_hint, &[("n", &n.to_string())])),
                        (n, Some(seat)) => Some(fill(
                            tr.spectator_picked,
                            &[("p", &names.label(tr, seat)), ("n", &n.to_string())],
                        )),
                    };
                    let rest = format!("{} | {}", tr.lobby_chat_hint, tr.prompt_esc_menu);
                    match call {
                        Some(call) => format!("{call} | {rest}"),
                        None => rest,
                    }
                }
            }
        }
        // The tide is in: the board is frozen and there is nothing to call
        // onto it, but the table is still talking, and this is where it
        // talks most. Asked of the session alone, as it always has been:
        // see `header_viewer`.
        VersusPhase::Over if Viewer::of(online, false) == Viewer::Spectator => {
            match r.spectator_typing {
                Some(line) => format!("> {line}_"),
                None => format!(
                    "{} | {}",
                    tr.lobby_chat_hint,
                    crate::app::rematch::over_prompt(
                        tr,
                        online,
                        tournament.is_running(),
                        rematch,
                        keep
                    )
                ),
            }
        }
        // Rebound keys first: the short legend against the AI or online
        // names the stock keys too, and input reads the player's own.
        // Asked with the keyboard shared, as `versus_input` reads it: the
        // one-hand preset stands down to the stock legend, and the mirrored
        // keys keep their own.
        VersusPhase::Countdown | VersusPhase::Running
            if settings.key_legend(true) == KeyLegend::Own =>
        {
            tr.prompt_versus_custom.to_string()
        }
        // The short legend names P1's keys alone, so it is for a table
        // where one person plays at these keys. A couch of two against an
        // AI still has P2 on the second block, who needs the local legend.
        // A table of machines alone has nobody to teach either way, and
        // keeps the lighter line.
        VersusPhase::Countdown | VersusPhase::Running
            if online.0.is_some() || controllers.local_count(seats.0) <= 1 =>
        {
            match settings.key_legend(true) {
                KeyLegend::Swapped => tr.prompt_versus_short_swapped,
                KeyLegend::Stock | KeyLegend::Own => tr.prompt_versus_short,
            }
            .to_string()
        }
        VersusPhase::Countdown | VersusPhase::Running => match settings.key_legend(true) {
            KeyLegend::Swapped => tr.prompt_versus_local_swapped,
            KeyLegend::Stock | KeyLegend::Own => tr.prompt_versus_local,
        }
        .to_string(),
        // Whatever Enter actually does here, named by the one function the
        // key itself reads: a finished lobby match goes back to the lobby
        // together, a series plays on, and the prompt must not promise a
        // door the key does not open.
        // The host's map page for the next round has the keys.
        VersusPhase::Over if crate::app::next_map::page_up(r.match_menu) => match r.map_pick {
            true => tr.series_pick_prompt.to_string(),
            false => tr.series_pick_prompt_no_pick.to_string(),
        },
        VersusPhase::Over => {
            crate::app::rematch::over_prompt(tr, online, tournament.is_running(), rematch, keep)
        }
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
    /// Two players at the Tide Pool (`app::Coop`), which changes what the
    /// setup prompt and the stage list say.
    pub coop: bool,
    pub phase: &'a State<Phase>,
    pub vphase: &'a State<VersusPhase>,
    pub editor: &'a EditorState,
    pub online: &'a Online,
    pub playback: &'a Playback,
    /// Whether the round is the daily, which is scored every seat for
    /// itself whatever the setting says.
    pub daily: &'a crate::app::Daily,
    pub lobby: &'a LobbyState,
    /// Whether a series is mid-flight, which changes what the results
    /// card's Enter does and so what the prompt may promise.
    pub tournament: &'a crate::app::tournament::Tournament,
    pub seats: &'a Seats,
    pub settings: &'a GameSettings,
    /// What this keyboard's caps say: a resource of its own, since it is
    /// learned state rather than a preference.
    pub keycaps: &'a crate::app::keycaps::KeyCaps,
    pub names: &'a crate::app::SeatNames,
    pub controllers: &'a Controllers,
    pub library: &'a crate::app::replays::Library,
    /// What the menu has to say about a round code copied or pasted.
    pub notice: &'a crate::app::RoundNotice,
    /// Which row the match setup is on: Enter means something else on the
    /// name rows, and the prompt line has to say so.
    pub match_menu: &'a crate::app::match_setup::MatchMenu,
    /// Whether the pause card is up and holding the keyboard.
    pub paused: bool,
    /// The line a spectator is saying to the table, while one is open.
    pub spectator_typing: Option<&'a str>,
    /// The host's word on the crowd.
    pub crowd: crate::app::spectators::Crowd,
    pub speed: u8,
    /// The face of the game whose pad table is in force, as the bridge
    /// and the screens that read the pad see it.
    pub face: &'a Face,
    /// Which hands the player last reached with, which the prompt names
    /// the controls of.
    pub device: LastDevice,
    /// Where the pad's pick stands on a list it picks from.
    pub pick: PadPick,
    /// What R plays again on the results card, at a table set up here
    /// ([`crate::app::rematch::rematch`]).
    pub rematch: Option<crate::app::rematch::Rematch>,
    /// What the results card offers the replay shelf
    /// ([`crate::app::replays::RoundToKeep::offer`]).
    pub keep: crate::app::replays::KeepOffer,
    /// Whether a host's map page has its cursor on a card Enter picks
    /// ([`crate::app::match_setup::card_to_pick`]).
    pub map_pick: bool,
}

/// What the header, the status slot and the prompt say on `screen`.
///
/// The music toggle is added here rather than written into each play
/// screen's prompt: there are eight of those in every language, and a key
/// that works everywhere should not be a line eight strings have to
/// remember to carry.
pub(super) fn screen_text(screen: Screen, r: &Readout) -> HudText {
    let mut said = screen_text_for(screen, r);
    // A pad's player is told the pad's buttons, from the table the bridge
    // and the screens read. None of the keyboard's spelling applies.
    if r.device == LastDevice::Pad {
        said.prompt = pad_prompt(r);
        return said;
    }
    if matches!(screen, Screen::Versus | Screen::Puzzle) {
        // The pause card has the keyboard while it is up, so the line
        // names its keys rather than the beach's: Escape is the way on
        // there, not the way to a pause the player is already in. The
        // card is an overlay and not a screen of its own, which is why
        // this is a coat of paint over the screen's own answer.
        if r.paused {
            said.prompt = r.tr.prompt_paused.to_string();
        }
        said.prompt = format!("{} | {}", said.prompt, r.tr.prompt_mute);
    }
    // Spelled in this keyboard's caps here, once, rather than in each of
    // the legends that name the move keys.
    said.prompt = r.keycaps.legend(&said.prompt);
    // The editor's keys follow P1's bindings and are spelled after the
    // legend: see `spell_editor_keys`.
    if screen == Screen::Editor {
        said.prompt =
            crate::app::editor::spell_editor_keys(&said.prompt, r.tr, r.keycaps, r.settings);
    }
    said
}

/// The prompt while a pad was the last thing touched: the line in front
/// of the controls, if the face has one to say, and the face's controls
/// as its table has them ([`scheme::legend`]).
pub(super) fn pad_prompt(r: &Readout) -> String {
    let tr = r.tr;
    let p = &tr.pad;
    let lead: Option<String> = match r.face {
        // A spectator's line as it is typed, in either hands: the box
        // has no other place on the screen. First, so a line long enough
        // to crowd the prompt shrinks or wraps the hint beside it and is
        // never itself what goes.
        Face::Typing(_) => Some(match r.spectator_typing {
            Some(line) => format!("> {line}_ | {}", p.steam_keyboard),
            None => p.steam_keyboard.to_string(),
        }),
        Face::Browse { watching: true, .. } => Some(p.watch_armed.to_string()),
        Face::Browse {
            watching: false, ..
        } => Some(match r.lobby.hosts.len() {
            0 => p.none_yet.to_string(),
            1 => p.open_game.to_string(),
            n => fill(p.open_games, &[("n", &n.to_string())]),
        }),
        Face::Hosting { .. } => Some(p.hosting.to_string()),
        // The page's own heading names the table it picks for.
        Face::HostMap { .. } | Face::NextMap { .. } => None,
        Face::Kicking { picks, .. } => picks
            .rows
            .get(r.pick.row(picks))
            .map(|(n, who)| fill(p.kick_who, &[("p", &format!("{n}. {who}"))])),
        Face::Card(picks) => picks
            .rows
            .get(r.pick.row(picks))
            .map(|(n, who)| format!("{n}. {who}")),
        Face::AtBeach => r.lobby.joined().map(|joined| {
            match (joined.queued.is_some(), joined.watching, joined.no_chair) {
                (true, true, _) => p.queued_watch,
                (true, false, _) => p.queued,
                (false, true, _) => p.watching,
                (false, false, true) => p.no_chair,
                (false, false, false) => p.joined,
            }
            .to_string()
        }),
        Face::PuzzleSetup { posts: false, .. } => Some(p.no_posts.to_string()),
        Face::PuzzleSetup { posts: true, .. } => {
            // Full as the keyboard's line has it.
            arrows_all_down(r.campaign.current(), &r.sim.0).then(|| p.full.to_string())
        }
        Face::PuzzleRunning => Some(p.running.to_string()),
        Face::PuzzleWon { last: true } => Some(p.last_level.to_string()),
        Face::PuzzleWon { last: false } => Some(
            match r.campaign.current().goal.saves_every_crab() {
                true => p.won,
                false => p.won_goal,
            }
            .to_string(),
        ),
        Face::PuzzleLost { .. } => Some(
            match r.campaign.current().goal.saves_every_crab() {
                true => p.lost,
                false => p.lost_goal,
            }
            .to_string(),
        ),
        Face::Watching { .. } => spectator_call(r),
        Face::Results { keep, .. } | Face::Interlude { keep } => {
            crate::app::replays::kept_lead(tr, *keep).map(String::from)
        }
        // Held, the second layer's buttons are Select and each of them.
        Face::Editor { layer: true } => Some(format!("{} +", Pad::Select.name(tr))),
        Face::Menu
        | Face::Quitting
        | Face::Settings
        | Face::Controls
        | Face::MatchSetup { .. }
        | Face::StageSelect { .. }
        | Face::Replays
        | Face::Achievements
        | Face::Language
        | Face::NewVersion
        | Face::Door { .. }
        | Face::Paused
        | Face::Round
        | Face::Replay
        | Face::Editor { layer: false }
        | Face::LevelList
        | Face::EditorCard
        | Face::LevelCard
        | Face::Playtest(_) => None,
    };
    let controls = scheme::legend(tr, r.face);
    match (lead, r.face) {
        (Some(lead), Face::Editor { layer: true }) => format!("{lead} {controls}"),
        (Some(lead), _) => format!("{lead} | {controls}"),
        (None, _) => controls,
    }
}

/// What a spectator's calls stand at, for the pad's line: a vote open or
/// waited on, and the winner still to call or called. The keyboard's line
/// says the same with its keys in it.
fn spectator_call(r: &Readout) -> Option<String> {
    let tr = r.tr;
    let vote = match (r.sim.0.events_enabled(), r.crowd.open, r.crowd.wait) {
        (false, ..) | (true, 0, 0) => None,
        (true, 0, wait) => Some(fill(tr.spectator_wait, &[("n", &wait.to_string())])),
        (true, open, _) => Some(fill(tr.spectator_vote_open, &[("n", &open.to_string())])),
    };
    let session = r.online.0.as_ref();
    let open = session.map_or(0, |s| s.stands.winner.picks.open);
    match (open, session.and_then(|s| s.stands.winner.my_pick)) {
        (0, _) => vote,
        (n, None) => Some(fill(tr.pad.pick_open, &[("n", &n.to_string())])),
        (n, Some(seat)) => Some(fill(
            tr.spectator_picked,
            &[("p", &r.names.label(tr, seat)), ("n", &n.to_string())],
        )),
    }
}

pub(super) fn screen_text_for(screen: Screen, r: &Readout) -> HudText {
    match screen {
        // The menu's status slot carries word of a code that would not
        // load: the only news the menu ever has. Under the quit card the
        // line is the card's keys, Esc going back there rather than out.
        Screen::Menu => HudText::new(
            String::new(),
            r.notice.0.clone(),
            match matches!(r.face, Face::Quitting) {
                true => crate::app::menu_scene::quit_card_keys(r.tr, LastDevice::Keyboard),
                false => r.tr.menu_prompt.to_string(),
            },
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
            match_setup_prompt(r.tr, r.match_menu, r.face),
        ),
        Screen::Achievements => HudText::new(
            r.tr.title_achievements.to_string(),
            String::new(),
            r.tr.prompt_achievements.to_string(),
        ),
        Screen::Replays => HudText::new(
            r.tr.title_replays.to_string(),
            r.library.feedback.clone(),
            r.tr.prompt_replays.to_string(),
        ),
        Screen::StageSelect => HudText::new(
            format!("{} - {}", r.tr.title_tide_pool, r.tr.title_stages),
            // A code copied off one of the player's own stages, or why
            // not, over the co-op line.
            match (r.notice.0.is_empty(), r.coop) {
                (false, _) => r.notice.0.clone(),
                (true, true) => crate::app::name_the_pair(r.tr, r.tr.stage_coop_on),
                (true, false) => String::new(),
            },
            // The player's own stages are theirs to edit and share, which
            // the pad's table answers to on the same face.
            match matches!(r.face, Face::StageSelect { custom: true }) {
                true => r.tr.prompt_stages_custom.to_string(),
                false => r.tr.prompt_stages.to_string(),
            },
        ),
        // Nothing to press but the round just played, while it can still
        // be kept: the same offer the results card made, which survives
        // into the doorway to the next round.
        Screen::Interlude => HudText::new(
            r.tr.title_turf_war.to_string(),
            String::new(),
            crate::app::replays::keep_words(r.tr, r.keep)
                .unwrap_or_default()
                .to_string(),
        ),
        Screen::Lobby => lobby_text(r.tr, r.lobby, r.map_pick),
        Screen::Editor => editor_text(r.tr, r.editor, &r.sim.0, r.settings),
        Screen::Puzzle => puzzle_text(
            r.tr,
            r.lang,
            r.campaign,
            r.coop,
            r.sim,
            r.phase,
            // Co-op sets the one-hand preset aside (the partner has IJKL),
            // so there only a rebinding makes the keys the player's own.
            r.settings.key_legend(r.coop),
        ),
        // Both lines are already in the language under the cursor: moving
        // it sets the language, so the header and the prompt preview
        // whatever is highlighted. The status slot stays empty, its note
        // sitting under the card beside the list instead.
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

    /// Everything a [`Readout`] borrows, at its plainest: the shipped Tide
    /// Pool, a blank two-seat versus beach in its round, offline, with the
    /// stock settings. A test names only what it is about:
    /// `Readout { field: .., ..fx.readout() }`.
    struct Fixture {
        sim: Sim,
        campaign: Campaign,
        phase: State<Phase>,
        vphase: State<VersusPhase>,
        editor: EditorState,
        online: Online,
        playback: Playback,
        daily: crate::app::Daily,
        lobby: LobbyState,
        tournament: crate::app::tournament::Tournament,
        seats: Seats,
        settings: GameSettings,
        keycaps: crate::app::keycaps::KeyCaps,
        names: crate::app::SeatNames,
        controllers: Controllers,
        library: crate::app::replays::Library,
        notice: crate::app::RoundNotice,
        match_menu: crate::app::match_setup::MatchMenu,
    }

    impl Default for Fixture {
        fn default() -> Self {
            Fixture {
                sim: Sim(Board::new(9, 7, 1)),
                campaign: Campaign::tide_pool(),
                phase: State::new(Phase::Setup),
                vphase: State::new(VersusPhase::Running),
                editor: EditorState::default(),
                online: Online::default(),
                playback: Playback::default(),
                daily: crate::app::Daily::default(),
                lobby: LobbyState::default(),
                tournament: crate::app::tournament::Tournament::default(),
                seats: Seats(2),
                settings: GameSettings::default(),
                keycaps: crate::app::keycaps::KeyCaps::default(),
                names: crate::app::SeatNames::default(),
                controllers: Controllers::default(),
                library: crate::app::replays::Library::default(),
                notice: crate::app::RoundNotice::default(),
                match_menu: crate::app::match_setup::MatchMenu::default(),
            }
        }
    }

    impl Fixture {
        fn readout(&self) -> Readout<'_> {
            let Self {
                sim,
                campaign,
                phase,
                vphase,
                editor,
                online,
                playback,
                daily,
                lobby,
                tournament,
                seats,
                settings,
                keycaps,
                names,
                controllers,
                library,
                notice,
                match_menu,
            } = self;
            Readout {
                tr: settings.tr(),
                lang: settings.language,
                sim,
                campaign,
                coop: false,
                phase,
                vphase,
                editor,
                online,
                playback,
                daily,
                lobby,
                tournament,
                seats,
                settings,
                keycaps,
                names,
                controllers,
                library,
                notice,
                match_menu,
                paused: false,
                spectator_typing: None,
                crowd: Default::default(),
                speed: 1,
                face: &Face::Menu,
                device: LastDevice::Keyboard,
                pick: PadPick::default(),
                rematch: None,
                keep: crate::app::replays::KeepOffer::Nothing,
                map_pick: true,
            }
        }
    }

    /// A pad's player on a co-op stage is told every arrow is down once
    /// both hands are, as the keyboard's line reads it: the stage's
    /// `posts` is the two hands together, and P1's alone never reach it.
    #[test]
    fn a_pad_says_full_when_both_hands_are_down() {
        let level = crate::sim::coop_levels().remove(0);
        let mut board = level.board();
        let fx = |board: &Board| Fixture {
            sim: Sim(board.clone()),
            campaign: Campaign::tide_pool_of(vec![level.clone()]),
            ..Fixture::default()
        };
        let face = Face::PuzzleSetup {
            posts: true,
            hint: false,
        };
        let pad = |fx: &Fixture| {
            pad_prompt(&Readout {
                face: &face,
                device: LastDevice::Pad,
                ..fx.readout()
            })
        };
        assert!(!pad(&fx(&board)).starts_with(EN.pad.full), "none down");
        level
            .place_solution(&mut board)
            .expect("its solution goes down");
        let full = pad(&fx(&board));
        assert!(full.starts_with(EN.pad.full), "{full}");
    }

    /// Every screen has to say what it is and what the keys do: one added
    /// without a HUD arm shows the last screen's header.
    #[test]
    fn every_screen_names_itself_and_its_keys() {
        let fx = Fixture::default();
        let readout = fx.readout();
        for screen in Screen::ALL {
            let said = screen_text(screen, &readout);
            let (title, prompt) = (&said.title, &said.prompt);
            // The menu is the one screen with no header: it is a postcard,
            // and its own art says where you are.
            if screen != Screen::Menu {
                assert!(!title.is_empty(), "{screen:?} has no header");
            }
            // The interlude is a between-rounds breather with nothing to
            // press but the keep key, and no round to keep here; everything
            // else tells you a key.
            if screen != Screen::Interlude {
                assert!(!prompt.is_empty(), "{screen:?} offers no keys");
            }
        }
    }

    /// No prompt names a key twice. `screen_text` appends the mute legend
    /// to every board prompt itself, so a string that carries one anyway
    /// reads "M: mute | M: mute", in two wordings where a translation
    /// picked different words.
    ///
    /// Checked with a recording playing, because that arm is the one the
    /// screen census above cannot reach: it builds its readout with
    /// `Playback::default()`, so the transport prompt never ran there.
    #[test]
    fn no_prompt_names_the_mute_key_twice() {
        use crate::app::{Playback, Seats};
        use crate::sim::{Replay, campaign_levels};

        let watching = Playback(Some((
            Replay::new(campaign_levels().first().expect("a campaign level").clone()),
            0,
        )));
        for lang in crate::app::i18n::ALL_LANGS {
            let fx = Fixture {
                settings: GameSettings {
                    language: lang,
                    ..GameSettings::default()
                },
                ..Fixture::default()
            };
            let tr = fx.settings.tr();
            let readout = Readout {
                phase: &State::new(Phase::Running),
                playback: &watching,
                seats: &Seats::default(),
                ..fx.readout()
            };
            let prompt = screen_text(Screen::Versus, &readout).prompt;
            let mute = tr.prompt_mute;
            assert_eq!(
                prompt.matches(mute).count(),
                1,
                "{lang:?} names the mute key more than once: {prompt}"
            );
        }
    }

    /// Under the main menu's quit card the keyboard's line is the card's
    /// keys, the ones `menu_input` reads there: Esc goes back, so the
    /// menu's "Esc: quit" would be a line that lies.
    #[test]
    fn the_quit_cards_line_names_its_keys() {
        let fx = Fixture::default();
        let tr = fx.settings.tr();
        let menu = screen_text(Screen::Menu, &fx.readout()).prompt;
        assert_eq!(menu, tr.menu_prompt);
        let quitting = Readout {
            face: &Face::Quitting,
            ..fx.readout()
        };
        assert_eq!(
            screen_text(Screen::Menu, &quitting).prompt,
            "Esc: back | Enter: quit"
        );
        let pad = Readout {
            face: &Face::Quitting,
            device: LastDevice::Pad,
            ..fx.readout()
        };
        assert_eq!(
            screen_text(Screen::Menu, &pad).prompt,
            crate::app::menu_scene::quit_card_keys(tr, LastDevice::Pad)
        );
    }

    /// The editor header names the mode it is in and carries the solver's
    /// last word; playtesting swaps both the title and the key legend.
    /// The speed line names where S goes next, from the same step the key
    /// takes: at the top speed that is back to x1, not "faster".
    #[test]
    fn the_replay_speed_line_names_the_speed_s_goes_to() {
        use crate::app::{Playback, Seats};
        use crate::sim::{Replay, campaign_levels};

        let watching = Playback(Some((
            Replay::new(campaign_levels().first().expect("a campaign level").clone()),
            0,
        )));
        let fx = Fixture::default();
        for (speed, want) in [(2, "x2  (S: x4)"), (4, "x4  (S: x1)")] {
            let readout = Readout {
                phase: &State::new(Phase::Running),
                playback: &watching,
                seats: &Seats::default(),
                speed,
                ..fx.readout()
            };
            let status = screen_text(Screen::Versus, &readout).status;
            assert!(status.starts_with(want), "x{speed}: {status}");
        }
    }

    #[test]
    fn the_editor_header_follows_the_playtest_toggle() {
        let mut editor = EditorState::default();
        editor.posts = 3;
        editor.name = "Gull Alley".into();
        editor.feedback = "solvable: 2".into();
        let HudText {
            title,
            status,
            prompt,
        } = editor_text(&EN, &editor, &Board::new(4, 4, 0), &GameSettings::default());
        assert!(title.starts_with(EN.title_editor), "{title}");
        assert!(title.contains("Gull Alley"), "the name is on the header");
        // And what is being built, which decides which list it saves to.
        assert!(title.contains(EN.ed_kind_puzzle), "{title}");
        // The status is what was last said; the settings are on the card.
        assert_eq!(status, "solvable: 2");
        assert_eq!(prompt, EN.ed_prompt.replacen("WASD", "{moves}", 1));

        editor.kind = crate::sim::LevelKind::Arena;
        let beach = editor_text(&EN, &editor, &Board::new(4, 4, 0), &GameSettings::default());
        assert!(beach.title.contains(EN.ed_kind_arena), "{}", beach.title);
        assert_eq!(beach.status, "solvable: 2", "{}", beach.status);
        editor.kind = crate::sim::LevelKind::Puzzle;

        // Typing shows a caret, so it is obvious the keyboard is spelling a
        // name rather than picking brushes.
        editor.mode = crate::app::editor::Mode::Naming { was: String::new() };
        let typing = editor_text(&EN, &editor, &Board::new(4, 4, 0), &GameSettings::default());
        assert!(typing.title.ends_with('_'), "{}", typing.title);
        // And the prompt names the box's keys, not the brushes': every one
        // of those is a letter until Enter or Esc.
        assert_eq!(typing.prompt, EN.prompt_typing);
        editor.mode = crate::app::editor::Mode::Painting;

        editor.mode = crate::app::editor::Mode::testing(Board::new(4, 4, 0));
        let HudText {
            title,
            status,
            prompt,
        } = editor_text(&EN, &editor, &Board::new(4, 4, 0), &GameSettings::default());
        assert_eq!(title, EN.title_playtest);
        // A running stage: R takes it back, and the line over it is the
        // stage's own count of arrows and crabs, as the stage list shows it.
        assert_eq!(prompt, EN.ed_test_retry);
        assert!(status.starts_with(&fill(EN.signposts_count, &[("a", "0"), ("b", "3")])));
    }

    /// The lobby prompt is a small state machine: aboard, broadcasting,
    /// nothing found yet, or how to work the list of beaches. The beaches
    /// themselves are rows of their own; see `HostEntry::label`.
    #[test]
    fn the_lobby_prompt_tracks_the_connection_state() {
        let mut lobby = LobbyState::default();
        assert_eq!(lobby_text(&EN, &lobby, true).prompt, EN.lobby_none_yet);

        for at in 1..=3u8 {
            lobby.hosts.push(crate::app::lobby::HostEntry {
                addr: format!("10.0.0.{at}:47777").parse().expect("addr"),
                id: u64::from(at),
                name: String::new(),
                host: String::new(),
                taken: 1,
                seats: 6,
                running: false,
                bots: false,
                full: false,
                age: 0.0,
                heard: 1,
            });
        }
        // The count is the part that matters: a hall with more games than
        // rows should not look like it has only as many as fit.
        let prompt = lobby_text(&EN, &lobby, true).prompt;
        assert!(prompt.contains('3'), "how many are out there: {prompt}");

        lobby.feedback = "hosting on port 47777".into();
        assert_eq!(
            lobby_text(&EN, &lobby, true).status,
            "hosting on port 47777"
        );

        // A line being typed has every key: the host's Enter sends it and
        // starts nothing, so the line does not offer a start.
        lobby.typing = Some(crate::app::lobby::Typing::chat());
        assert_eq!(lobby_text(&EN, &lobby, true).prompt, EN.prompt_typing);
    }

    /// Each setup step says its own keys, and while a seat's name is being
    /// typed, W/S and Enter are letters and a way out of the box, not the
    /// dials and a start.
    #[test]
    fn the_match_setup_prompt_follows_the_step_and_the_name_box() {
        use crate::app::match_setup::{MatchMenu, Step};
        let mut menu = MatchMenu {
            seat: 1,
            ..MatchMenu::default()
        };
        for (step, line) in [
            (Step::Game, EN.prompt_setup_game),
            (Step::Map, EN.prompt_setup_map),
            (Step::Players, EN.prompt_setup_players),
        ] {
            menu.step = step;
            assert_eq!(match_setup_prompt(&EN, &menu, &Face::Menu), line);
        }
        // On player 1's row A/D turns nothing, and the line does not say
        // it does.
        menu.seat = 0;
        let p1 = match_setup_prompt(&EN, &menu, &Face::Menu);
        assert_eq!(p1, EN.prompt_setup_players_p1);
        assert!(!p1.contains("A/D") && p1.contains("Tab"), "{p1}");
        // Under a team mode it ends on the partner key, as the pad's
        // face offers it.
        let teams = Face::MatchSetup {
            step: Step::Players,
            join: false,
            partners: true,
        };
        let line = match_setup_prompt(&EN, &menu, &teams);
        assert!(line.ends_with(" | P: partners"), "{line}");
        menu.seat = 1;
        menu.naming = Some(0);
        assert_eq!(
            match_setup_prompt(&EN, &menu, &Face::Menu),
            EN.prompt_typing_keep
        );
    }

    /// Three ways to be at somebody else's beach, and three lines.
    ///
    /// Aboard is the joiner's word. A spectator is not aboard, and a peer
    /// in line is not waiting for a launch: the round it is queued for
    /// started without it. The prompt matched on `Joining` alone and told
    /// all three they were aboard and the match would start when the host
    /// launched, while the status line an inch above said "watching" or
    /// "Round in progress". Found by putting each of them on a real beach.
    #[test]
    fn each_way_of_being_at_a_beach_gets_its_own_prompt() {
        use crate::app::lobby::{Joined, Standing};
        use crate::transport::UdpTransport;

        let joined_past = |watching, queued, no_chair| {
            let mut lobby = LobbyState::default();
            let mut aboard =
                Joined::returned(UdpTransport::host(0).expect("socket"), watching, 0, None);
            aboard.queued = queued;
            aboard.no_chair = no_chair;
            lobby.standing = Standing::Joining(aboard);
            lobby_text(&EN, &lobby, true).prompt
        };
        let joined = |watching, queued| joined_past(watching, queued, false);
        assert_eq!(joined(false, None), EN.lobby_aboard_prompt);
        assert_eq!(joined(true, None), EN.lobby_watching_prompt);
        // And a peer in line is waiting for a round that has already
        // started, which is not what either of the other two say. Two of
        // those as well: somebody who armed W and dialled in mid-round is
        // queued like anybody else, and is not in line for a chair.
        assert_eq!(joined(false, Some(0)), EN.lobby_queued_prompt);
        assert_eq!(joined(true, Some(2)), EN.lobby_queued_watch_prompt);
        // A player past the table's last chair is not aboard either: the
        // launch seats it watching.
        assert_eq!(joined_past(false, None, true), EN.lobby_no_chair_prompt);
        let all = [
            EN.lobby_aboard_prompt,
            EN.lobby_watching_prompt,
            EN.lobby_queued_prompt,
            EN.lobby_queued_watch_prompt,
            EN.lobby_no_chair_prompt,
        ];
        for (at, line) in all.iter().enumerate() {
            assert!(
                !all[at + 1..].contains(line),
                "each standing wants its own line: {line}"
            );
        }
    }

    /// The header titles the screen by the recording when one is playing,
    /// even under a live session, where every seat question goes by the
    /// session instead (`seating::Viewer::of`). Both answers are the ones
    /// they have always given; this pins the header's side of it.
    #[test]
    fn the_header_names_a_recording_before_the_session_under_it() {
        let recording = Playback(Some((
            crate::sim::Replay::new(campaign_levels().first().expect("a level").clone()),
            0,
        )));
        let none = Playback::default();
        let session = |seat: Option<u8>| {
            let step = match seat {
                Some(seat) => {
                    crate::sim::Lockstep::new(seat, vec![0, 1], crate::sim::DEFAULT_DELAY)
                }
                None => crate::sim::Lockstep::observer(vec![0, 1], crate::sim::DEFAULT_DELAY),
            };
            Online(Some(crate::app::net::OnlineSession::new(
                crate::transport::UdpTransport::host(0).expect("socket"),
                step,
                2,
                crate::transport::MatchTerms::default(),
            )))
        };
        let offline = Online::default();
        assert_eq!(header_viewer(&offline, &none), Viewer::Table);
        assert_eq!(header_viewer(&offline, &recording), Viewer::Replay);
        for seat in [Some(1), None] {
            let online = session(seat);
            assert_eq!(
                header_viewer(&online, &recording),
                Viewer::Replay,
                "{seat:?}"
            );
            assert_ne!(
                Viewer::of(&online, true),
                Viewer::Replay,
                "the seat questions go by the session: {seat:?}"
            );
        }
        assert_eq!(header_viewer(&session(Some(1)), &none), Viewer::Peer(1));
        assert_eq!(header_viewer(&session(None), &none), Viewer::Spectator);
    }

    /// The host's map page between rounds offers Enter only where Enter
    /// picks: on a greyed card, or the one saying how to make a map, the
    /// prompt walks and goes back and says nothing of a next round. The
    /// lobby's page the same.
    #[test]
    fn the_map_pages_prompt_offers_enter_only_on_a_card_it_picks() {
        let mut fx = Fixture {
            vphase: State::new(VersusPhase::Over),
            ..Fixture::default()
        };
        fx.match_menu.online = true;
        fx.match_menu.step = crate::app::match_setup::Step::Map;
        let said = |map_pick: bool| {
            versus_text(&Readout {
                map_pick,
                ..fx.readout()
            })
            .prompt
        };
        assert_eq!(said(true), EN.series_pick_prompt);
        assert_eq!(said(false), EN.series_pick_prompt_no_pick);
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            for (line, whole) in [
                (tr.lobby_pick_prompt_no_pick, tr.lobby_pick_prompt),
                (tr.series_pick_prompt_no_pick, tr.series_pick_prompt),
            ] {
                assert!(!line.contains(tr.key_enter), "{lang:?}: {line}");
                // The way back, as the whole prompt says it.
                let back = whole.rsplit(" | ").next().unwrap_or_default();
                assert!(line.ends_with(back), "{lang:?}: {line}");
            }
            for line in [tr.lobby_pick_prompt, tr.series_pick_prompt] {
                assert!(line.contains(tr.key_enter), "{lang:?}: {line}");
            }
        }
    }

    /// What the key does right now, in the crowd's own terms. A spectator
    /// pressing E into silence could not tell whether a vote was open, how
    /// long was left to join it, or why nothing happened while the crowd
    /// was waiting; all three read the same.
    #[test]
    fn a_spectators_prompt_says_what_the_tide_key_will_do() {
        use crate::app::spectators::Crowd;

        let fx = Fixture::default();
        // A watching session: no seat, so the spectator's arm is the one
        // the prompt takes.
        let watching = Online(Some(crate::app::net::OnlineSession::new(
            crate::transport::UdpTransport::host(0).expect("socket"),
            crate::sim::Lockstep::observer(vec![0, 1], crate::sim::DEFAULT_DELAY),
            2,
            crate::transport::MatchTerms::default(),
        )));
        // A versus beach, whose wheel turns.
        let mut board = Board::new(9, 7, 1);
        board.set_events_enabled(true);
        let beach = Sim(board);
        let handmade = Sim(Board::new(9, 7, 1));
        let said_on = |sim: &Sim, crowd: Crowd| {
            versus_text(&Readout {
                sim,
                online: &watching,
                crowd,
                ..fx.readout()
            })
            .prompt
        };
        let said = |crowd: Crowd| said_on(&beach, crowd);

        // A beach whose wheel is off offers no call at all, whatever the
        // crowd's numbers say, and still names T.
        let off = said_on(&handmade, Crowd::default());
        assert!(
            !off.contains(EN.spectator_call_hint) && off.starts_with(EN.lobby_chat_hint),
            "no call offered: {off}"
        );

        let idle = said(Crowd::default());
        assert!(
            idle.starts_with(EN.spectator_call_hint),
            "the offer: {idle}"
        );

        let open = said(Crowd {
            open: 2,
            ..Default::default()
        });
        assert!(
            open.starts_with(&fill(EN.spectator_vote_open, &[("n", "2")])),
            "a vote to join: {open}"
        );

        let wait = said(Crowd {
            wait: 9,
            ..Default::default()
        });
        assert!(
            wait.starts_with(&fill(EN.spectator_wait, &[("n", "9")])),
            "a wait to sit out: {wait}"
        );

        // An open vote wins over a wait, since one is running right now.
        let both = said(Crowd {
            open: 2,
            wait: 9,
            watching: 3,
        });
        assert!(
            both.starts_with(&fill(EN.spectator_vote_open, &[("n", "2")])),
            "the open one: {both}"
        );

        for state in [idle, open, wait, both] {
            assert!(state.contains(EN.lobby_chat_hint), "and T is always there");
        }
    }

    /// The results card's prompt names the keep key at a keyboard and the
    /// results table's button on a pad while the round is on offer, says
    /// it is kept once it is, and says nothing of it with no recording.
    #[test]
    fn the_prompt_offers_the_round_in_the_hands_last_used() {
        use crate::app::play_input::AfterRound;
        use crate::app::replays::KeepOffer;
        let fx = Fixture::default();
        let said = |keep, device| {
            let face = Face::Results {
                door: AfterRound::Menu,
                talk: false,
                rematch: None,
                keep,
            };
            screen_text(
                Screen::Versus,
                &Readout {
                    vphase: &State::new(VersusPhase::Over),
                    keep,
                    device,
                    face: &face,
                    ..fx.readout()
                },
            )
            .prompt
        };
        let keys = said(KeepOffer::Offered, LastDevice::Keyboard);
        assert!(keys.contains("G: keep replay"), "{keys}");
        let pad = said(KeepOffer::Offered, LastDevice::Pad);
        assert!(pad.contains("X: keep replay"), "{pad}");
        for device in [LastDevice::Keyboard, LastDevice::Pad] {
            let kept = said(KeepOffer::Kept, device);
            assert!(kept.contains(EN.replay_kept), "{device:?}: {kept}");
            assert!(!kept.contains("keep replay"), "{device:?}: {kept}");
            let none = said(KeepOffer::Nothing, device);
            assert!(!none.contains("replay"), "{device:?}: {none}");
        }
    }

    /// The interlude's prompt carries the results card's offer on: the
    /// keep key at a keyboard and X on a pad while the round before it is
    /// on offer, the word that it is kept once it is, and nothing with no
    /// round to keep.
    #[test]
    fn the_interlude_offers_the_round_before_it() {
        use crate::app::replays::KeepOffer;
        let fx = Fixture::default();
        let said = |keep, device| {
            let face = Face::Interlude { keep };
            screen_text(
                Screen::Interlude,
                &Readout {
                    keep,
                    device,
                    face: &face,
                    ..fx.readout()
                },
            )
            .prompt
        };
        assert_eq!(
            said(KeepOffer::Offered, LastDevice::Keyboard),
            "G: keep replay"
        );
        let pad = said(KeepOffer::Offered, LastDevice::Pad);
        assert!(pad.contains("X: keep replay"), "{pad}");
        for device in [LastDevice::Keyboard, LastDevice::Pad] {
            let kept = said(KeepOffer::Kept, device);
            assert!(kept.contains(EN.replay_kept), "{device:?}: {kept}");
            assert!(!kept.contains("keep replay"), "{device:?}: {kept}");
            let none = said(KeepOffer::Nothing, device);
            assert!(!none.contains("replay"), "{device:?}: {none}");
        }
    }

    /// A spectator is told which key gets it out, and it is not the key
    /// the line used to name.
    ///
    /// Two of them, both found by watching a real round end. Mid-round the
    /// line said "Enter: menu", but Enter does nothing while watching: it
    /// is Escape that opens the card, since a spectator cannot pause a
    /// table it is not playing at. And on the results card it still said
    /// "Enter: menu" while `versus_over_input` walked that spectator back
    /// to the lobby with the rest of the table, which is the one thing
    /// `after_round` exists to keep every reader agreeing on.
    #[test]
    fn a_spectator_is_told_the_key_that_actually_lets_it_out() {
        let fx = Fixture::default();
        let mut session = crate::app::net::OnlineSession::new(
            crate::transport::UdpTransport::host(0).expect("socket"),
            crate::sim::Lockstep::observer(vec![0, 1], crate::sim::DEFAULT_DELAY),
            2,
            crate::transport::MatchTerms::default(),
        );
        // Came in through a lobby, which is where Enter sends it back to.
        session.home.from_lobby = true;
        let watching = Online(Some(session));
        let said = |phase| {
            versus_text(&Readout {
                vphase: &State::new(phase),
                online: &watching,
                ..fx.readout()
            })
            .prompt
        };

        let running = said(VersusPhase::Running);
        assert!(
            running.contains(EN.prompt_esc_menu),
            "Escape is the way out of a round you are watching: {running}"
        );
        assert!(
            !running.contains(EN.prompt_enter_menu),
            "and Enter is not, so it must not be offered: {running}"
        );

        let over = said(VersusPhase::Over);
        assert!(
            over.contains(EN.prompt_enter_lobby),
            "and the card names the door the key opens: {over}"
        );
        assert_eq!(
            crate::app::play_input::after_round(&watching, false),
            crate::app::play_input::AfterRound::Lobby,
            "which is the lobby, as the key itself reads it"
        );
    }

    /// The table is told there is a crowd, in whatever room the banners
    /// leave. Without it a called tide event is the first anyone at the
    /// table hears that anyone is watching, and it arrives together with
    /// its own consequences.
    #[test]
    fn the_table_is_told_it_has_an_audience_when_nothing_louder_is_saying_anything() {
        let fx = Fixture::default();
        let status = |watching: u8, surging: bool| {
            let mut board = Board::new(9, 7, 1);
            if surging {
                board.rules_mut().round_length = Some(crate::sim::SURGE_TICKS);
            }
            versus_text(&Readout {
                sim: &Sim(board),
                crowd: crate::app::spectators::Crowd {
                    watching,
                    ..Default::default()
                },
                ..fx.readout()
            })
            .status
        };

        assert_eq!(status(0, false), "", "nobody watching, nothing said");
        assert_eq!(
            status(3, false),
            fill(EN.spectator_count, &[("n", "3")]),
            "three at the back, and room to say so"
        );
        assert_eq!(
            status(3, true),
            EN.the_gulls,
            "and the flock coming in always has the slot instead"
        );
    }

    /// While the pause card holds the keyboard, the prompt names the
    /// card's keys. It used to go on listing the beach's, offering "Esc
    /// pause" to a player already paused and saying nothing about the
    /// rows in front of them.
    #[test]
    fn the_prompt_names_the_pause_cards_keys_while_it_is_up() {
        let fx = Fixture::default();
        let said = |screen, paused| {
            screen_text(
                screen,
                &Readout {
                    paused,
                    ..fx.readout()
                },
            )
            .prompt
        };

        for screen in [Screen::Puzzle, Screen::Versus] {
            let playing = said(screen, false);
            let paused = said(screen, true);
            assert_ne!(playing, paused, "{screen:?}");
            assert!(paused.starts_with(EN.prompt_paused), "{screen:?}: {paused}");
            // The mute key is global and still works, so it stays on the end.
            assert!(paused.ends_with(EN.prompt_mute), "{screen:?}: {paused}");
        }
    }

    /// Under the mirrored placing keys every legend that teaches the keys
    /// names the mirror: a stage's setup alone and in co-op, and both
    /// versus lines. Never the stock line, which teaches the opposite, and
    /// not the "your keys" line either, the right words being cheap.
    #[test]
    fn the_mirrored_keys_have_their_own_legend() {
        use crate::app::settings::CommitScheme;
        use crate::app::{Controllers, SeatController};
        let fx = Fixture::default();
        assert!(
            fx.campaign.current().posts > 0,
            "a stage with arrows to place"
        );
        let wasd = GameSettings {
            commit: CommitScheme::Wasd,
            ..GameSettings::default()
        };
        assert_eq!(wasd.key_legend(false), KeyLegend::Swapped);
        assert_eq!(
            wasd.key_legend(true),
            KeyLegend::Swapped,
            "kept when shared"
        );

        let puzzle = |coop| {
            screen_text(
                Screen::Puzzle,
                &Readout {
                    settings: &wasd,
                    coop,
                    ..fx.readout()
                },
            )
            .prompt
        };
        let solo = puzzle(false);
        assert!(solo.starts_with(EN.prompt_setup_swapped), "{solo}");
        assert!(solo.starts_with("arrow keys move | WASD place"), "{solo}");
        let pair = puzzle(true);
        let coop = crate::app::name_the_pair(&EN, EN.prompt_setup_coop_swapped);
        assert!(pair.starts_with(&coop), "{pair}");

        let versus = |controllers: &Controllers| {
            versus_text(&Readout {
                settings: &wasd,
                controllers,
                ..fx.readout()
            })
            .prompt
        };
        let mut against_ai = Controllers::default();
        against_ai.0[1] = SeatController::Ai(crate::sim::BotLevel::Normal);
        assert_eq!(versus(&against_ai), EN.prompt_versus_short_swapped);
        assert_eq!(
            versus(&Controllers::default()),
            EN.prompt_versus_local_swapped
        );
    }

    /// P1 against a bot has one cursor, like P1 against the AI: the
    /// prompt names P1's keys alone, not a P2 whose keys drive nothing.
    #[test]
    fn a_match_against_a_bot_prompts_for_one_player() {
        use crate::app::{Controllers, SeatController};

        let fx = Fixture::default();
        let said = |controllers: &Controllers| {
            versus_text(&Readout {
                controllers,
                ..fx.readout()
            })
            .prompt
        };
        let mut against_ai = Controllers::default();
        against_ai.0[1] = SeatController::Ai(crate::sim::BotLevel::Normal);
        let mut against_bot = Controllers::default();
        against_bot.0[1] = SeatController::Bot;
        assert_eq!(said(&against_ai), EN.prompt_versus_short);
        assert_eq!(said(&against_bot), EN.prompt_versus_short);
        assert_eq!(said(&Controllers::default()), EN.prompt_versus_local);
    }

    /// A couch of two humans and an AI has a P2 on the second block of
    /// keys: the prompt names both players' keys, and the header names
    /// the AI without telling one "you" which seat is theirs.
    #[test]
    fn two_humans_against_an_ai_see_both_players_keys() {
        use crate::app::{Controllers, SeatController, Seats};

        let fx = Fixture::default();
        let said = |controllers: &Controllers| {
            versus_text(&Readout {
                seats: &Seats(3),
                controllers,
                ..fx.readout()
            })
        };
        let mut couch = Controllers::default();
        couch.0[2] = SeatController::Ai(crate::sim::BotLevel::Normal);
        let text = said(&couch);
        assert_eq!(text.prompt, EN.prompt_versus_local);
        assert_eq!(text.title, "Versus + 1 AI");
        let mut lone = couch;
        lone.0[1] = SeatController::Ai(crate::sim::BotLevel::Normal);
        let text = said(&lone);
        assert_eq!(text.prompt, EN.prompt_versus_short);
        assert!(text.title.contains("you are"), "{}", text.title);
    }

    /// And the prompt line says so too. The screen census below builds its
    /// readout with a single round, so the mid-series arm is one it cannot
    /// reach.
    #[test]
    fn the_prompt_carries_a_local_series_on_rather_than_offering_the_menu() {
        use crate::app::tournament::{SeriesLength, Tournament};

        let fx = Fixture::default();
        let said = |tournament: &Tournament| {
            versus_text(&Readout {
                vphase: &State::new(VersusPhase::Over),
                tournament,
                ..fx.readout()
            })
            .prompt
        };

        assert_eq!(
            said(&Tournament::start(SeriesLength::BestOfThree)),
            EN.tour_next
        );
        // A single round is done with, and a decided series too.
        assert_eq!(said(&Tournament::default()), EN.prompt_enter_menu);
        let mut decided = Tournament::start(SeriesLength::BestOfThree);
        decided.wins[0] = 2;
        decided.state = crate::app::tournament::SeriesState::Decided;
        assert_eq!(said(&decided), EN.prompt_enter_menu);
    }

    /// The player's own levels are a shelf behind the shipped list, not
    /// stages a hundred and one and up. The header counts them from one
    /// under the stage list's own label for them, and the shipped campaign
    /// ends at its last shipped stage even with driftwood saved behind it:
    /// the results card says the run is over and Enter goes home there, so
    /// a prompt still offering the next level is the game arguing with
    /// itself.
    #[test]
    fn driftwood_is_a_shelf_behind_the_campaign_not_more_of_it() {
        let shipped = campaign_levels();
        let builtins = shipped.len();
        let mut levels = shipped.clone();
        levels.extend([shipped[0].clone(), shipped[1].clone()]);
        let mut campaign = Campaign {
            builtins,
            ..Campaign::tide_pool_of(levels)
        };
        let said = |campaign: &Campaign, phase| {
            let sim = Sim(campaign.current().board());
            puzzle_text(
                &EN,
                Lang::En,
                campaign,
                false,
                &sim,
                &State::new(phase),
                KeyLegend::Stock,
            )
        };

        // A shipped stage counts against the shipped list, not against the
        // list plus whatever the player has built.
        let title = said(&campaign, Phase::Setup).title;
        assert!(title.contains(&format!("1/{builtins}")), "{title}");
        assert!(!title.contains(EN.stage_custom), "{title}");

        campaign.index = builtins - 1;
        assert_eq!(said(&campaign, Phase::Won).prompt, EN.last_level);

        // The first of the player's own is one of two, under its own label.
        campaign.index = builtins;
        let HudText { title, prompt, .. } = said(&campaign, Phase::Won);
        assert!(title.contains(EN.stage_custom), "{title}");
        assert!(title.contains("1/2"), "{title}");
        // The label stands in for the list's name rather than joining it:
        // the header bar ends at the clock in the middle of the window.
        assert!(!title.contains(EN.title_tide_pool), "{title}");
        assert_eq!(prompt, EN.prompt_won);

        // And the last of them ends the list.
        campaign.index = builtins + 1;
        let HudText { title, prompt, .. } = said(&campaign, Phase::Won);
        assert!(title.contains("2/2"), "{title}");
        assert_eq!(prompt, EN.last_level);
    }

    /// The puzzle header counts what the level asks for, and the prompt
    /// changes when the inventory runs out: the difference between "place
    /// your signposts" and "you have none left, press Enter".
    #[test]
    fn the_puzzle_prompt_reacts_to_a_spent_inventory() {
        let campaign = Campaign::tide_pool();
        let level: &Level = campaign.current();
        let posts = level.posts;
        let mut sim = Sim(level.board());
        let phase = State::new(Phase::Setup);

        let HudText { title, prompt, .. } = puzzle_text(
            &EN,
            Lang::En,
            &campaign,
            false,
            &sim,
            &phase,
            KeyLegend::Stock,
        );
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
            let prompt = puzzle_text(
                &EN,
                Lang::En,
                &campaign,
                false,
                &sim,
                &phase,
                KeyLegend::Stock,
            )
            .prompt;
            assert_eq!(prompt, EN.prompt_setup_full);
        }

        // Rebound keys retire the stock legend rather than teach wrong keys.
        let prompt = puzzle_text(
            &EN,
            Lang::En,
            &campaign,
            false,
            &Sim(level.board()),
            &phase,
            KeyLegend::Own,
        )
        .prompt;
        assert_eq!(prompt, EN.prompt_setup_custom);
        // In co-op the stock line names both players' keys, and rebound
        // keys still win: the pair's keys are the settings' then.
        let coop = |keys| {
            puzzle_text(
                &EN,
                Lang::En,
                &campaign,
                true,
                &Sim(level.board()),
                &phase,
                keys,
            )
            .prompt
        };
        if posts > 0 {
            assert_eq!(
                coop(KeyLegend::Stock),
                crate::app::name_the_pair(&EN, EN.prompt_setup_coop)
            );
        }
        assert_eq!(coop(KeyLegend::Own), EN.prompt_setup_custom);
        // And so does the one-hand preset: placement is on IJKL then, not
        // the arrows the stock legend names.
        let one_hand = GameSettings {
            commit: crate::app::settings::CommitScheme::Ijkl,
            ..GameSettings::default()
        };
        assert_eq!(one_hand.key_legend(false), KeyLegend::Own);
        assert_eq!(GameSettings::default().key_legend(false), KeyLegend::Stock);
        // Unless a partner has IJKL, which puts the pair on the arrows.
        assert_eq!(one_hand.key_legend(true), KeyLegend::Stock);
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

    /// The prompt follows the hands last used: the keyboard's keys, and
    /// once a pad is touched the pad's buttons, from the face's table, on
    /// every screen. The header and the status stay as they were.
    #[test]
    fn the_prompt_follows_the_hands_last_used() {
        let fx = Fixture::default();
        let keys = fx.readout();
        let pad = Readout {
            device: LastDevice::Pad,
            ..fx.readout()
        };
        let said = screen_text(Screen::Menu, &keys);
        assert!(said.prompt.contains("Enter: play"), "{}", said.prompt);
        let said = screen_text(Screen::Menu, &pad);
        assert_eq!(said.prompt, scheme::legend(&EN, &Face::Menu));
        assert!(said.prompt.contains("A: play"), "{}", said.prompt);
        // Each screen with the face it shows at rest, and the pad's line
        // there ends on that face's own legend: a screen whose prompt came
        // off another face's table would name buttons that do nothing.
        let mut legends = Vec::new();
        for screen in Screen::ALL {
            let face = match screen {
                Screen::Menu => Face::Menu,
                Screen::Settings => Face::Settings,
                Screen::Controls => Face::Controls,
                Screen::MatchSetup => Face::MatchSetup {
                    step: crate::app::match_setup::Step::Game,
                    join: true,
                    partners: false,
                },
                Screen::Achievements => Face::Achievements,
                Screen::Replays => Face::Replays,
                Screen::StageSelect => Face::StageSelect { custom: false },
                Screen::Interlude => Face::Interlude {
                    keep: crate::app::replays::KeepOffer::Nothing,
                },
                Screen::Lobby => Face::Browse {
                    hosts: false,
                    watching: false,
                },
                Screen::Editor => Face::Editor { layer: false },
                Screen::Puzzle => Face::PuzzleSetup {
                    posts: true,
                    hint: false,
                },
                Screen::Versus => Face::Round,
                Screen::Language => Face::Language,
                Screen::NewVersion => Face::NewVersion,
            };
            let keyboard = screen_text(
                screen,
                &Readout {
                    face: &face,
                    ..fx.readout()
                },
            );
            let pads = screen_text(
                screen,
                &Readout {
                    face: &face,
                    device: LastDevice::Pad,
                    ..fx.readout()
                },
            );
            let legend = scheme::legend(&EN, &face);
            assert_eq!(keyboard.title, pads.title, "{screen:?}");
            assert!(
                pads.prompt.ends_with(&legend),
                "{screen:?}: {:?} is not {face:?}'s legend {legend:?}",
                pads.prompt
            );
            assert_ne!(keyboard.prompt, pads.prompt, "{screen:?} did not switch");
            legends.push((screen, legend));
        }
        // And the faces are told apart: no two screens share one line.
        for (i, (screen, legend)) in legends.iter().enumerate() {
            for (other, theirs) in &legends[i + 1..] {
                assert_ne!(legend, theirs, "{screen:?} and {other:?}");
            }
        }
    }

    /// A pad's prompt names each button with what it does, in the words
    /// of the table, and the line in front of the controls where the face
    /// has something to say first.
    #[test]
    fn a_pads_prompt_leads_with_what_the_face_has_to_say() {
        let fx = Fixture::default();
        let pad = |face: &Face| {
            pad_prompt(&Readout {
                face,
                device: LastDevice::Pad,
                ..fx.readout()
            })
        };
        assert_eq!(
            pad(&Face::PuzzleRunning),
            format!("{} | Y: reset | Start: pause", EN.pad.running)
        );
        assert!(
            pad(&Face::Typing(scheme::TextBox::Cancels))
                .starts_with("Steam + X: keyboard | A: confirm")
        );
        assert_eq!(
            pad(&Face::Editor { layer: true }),
            format!(
                "Select + {}",
                scheme::legend(&EN, &Face::Editor { layer: true })
            )
        );
        let picks = scheme::Picks {
            list: scheme::PickList::Kick,
            rows: vec![(2, "Ann".to_string()), (3, "Bob".to_string())],
        };
        assert!(
            pad(&Face::Kicking {
                picks,
                pages: false
            })
            .starts_with("Ask who to leave? 2. Ann |"),
            "the pick stands on the first row"
        );
    }

    /// A spectator typing a line on a pad sees the line, as at a keyboard,
    /// with the pad's hint after it.
    #[test]
    fn a_spectators_line_shows_while_a_pad_is_in_hand() {
        let fx = Fixture::default();
        let face = Face::Typing(scheme::TextBox::Cancels);
        let said = screen_text(
            Screen::Versus,
            &Readout {
                face: &face,
                device: LastDevice::Pad,
                spectator_typing: Some("nice save"),
                ..fx.readout()
            },
        );
        assert_eq!(
            said.prompt,
            format!(
                "> nice save_ | {} | {}",
                EN.pad.steam_keyboard,
                scheme::legend(&EN, &face)
            )
        );
    }

    /// Each screen a line is shown on, and whether a recording is playing
    /// there.
    type Shown = Vec<(Screen, bool)>;

    /// Where a pad's face is shown: each screen it can stand on, and
    /// whether a recording is playing there. A face on more than one has
    /// to fit the tightest of them.
    fn face_screens(face: &Face) -> Shown {
        use scheme::TextBox;
        let live = |screen| vec![(screen, false)];
        match face {
            Face::Menu | Face::Quitting | Face::LevelCard => live(Screen::Menu),
            Face::Settings => live(Screen::Settings),
            Face::Controls => live(Screen::Controls),
            Face::MatchSetup { .. } | Face::Typing(TextBox::Keeps) => live(Screen::MatchSetup),
            Face::StageSelect { .. } => live(Screen::StageSelect),
            Face::Replays => live(Screen::Replays),
            Face::Achievements => live(Screen::Achievements),
            Face::Language => live(Screen::Language),
            Face::NewVersion => live(Screen::NewVersion),
            Face::Interlude { .. } => live(Screen::Interlude),
            Face::Door { .. } => vec![(Screen::MatchSetup, false), (Screen::Lobby, false)],
            Face::Typing(TextBox::Chat)
            | Face::Browse { .. }
            | Face::Hosting { .. }
            | Face::Kicking { .. }
            | Face::AtBeach => live(Screen::Lobby),
            Face::HostMap { .. } => live(Screen::Lobby),
            // Over the results card between rounds.
            Face::NextMap { .. } => live(Screen::Versus),
            Face::Typing(TextBox::Cancels) => vec![
                (Screen::Editor, false),
                (Screen::Lobby, false),
                (Screen::Versus, false),
            ],
            Face::PuzzleSetup { .. }
            | Face::PuzzleRunning
            | Face::PuzzleWon { .. }
            | Face::PuzzleLost { .. } => live(Screen::Puzzle),
            Face::Paused => vec![(Screen::Puzzle, false), (Screen::Versus, false)],
            Face::Round | Face::Watching { .. } | Face::Card(_) => live(Screen::Versus),
            Face::Replay => vec![(Screen::Versus, true)],
            // A recording runs on to its own results.
            Face::Results { .. } => vec![(Screen::Versus, false), (Screen::Versus, true)],
            Face::Editor { .. } | Face::LevelList | Face::EditorCard | Face::Playtest(_) => {
                live(Screen::Editor)
            }
        }
    }

    /// Every line a pad's prompt can show in `tr`, with the longest line
    /// each face can lead with, and where it is shown.
    fn pad_prompts(tr: &'static Tr) -> Vec<(Shown, String)> {
        let p = &tr.pad;
        let name = "W".repeat(crate::app::settings::NAME_MAX);
        let n = |template: &str| fill(template, &[("n", "99"), ("p", &format!("9. {name}"))]);
        let longest_event = tr
            .events
            .iter()
            .max_by_key(|e| crate::app::i18n::metrics::display_px(e, 1.0) as u32)
            .copied()
            .unwrap_or("");
        let mut out = Vec::new();
        for face in crate::app::gamepad::face::every_face() {
            let leads: Vec<String> = match &face {
                Face::Typing(_) => vec![p.steam_keyboard.to_string()],
                Face::Browse { .. } => [p.watch_armed, p.none_yet, p.open_game, p.open_games]
                    .map(n)
                    .to_vec(),
                Face::Hosting { .. } => vec![p.hosting.to_string()],
                Face::HostMap { .. } | Face::NextMap { .. } => vec![String::new()],
                Face::Kicking { .. } => vec![n(p.kick_who)],
                Face::Card(_) => vec![format!("9. {longest_event}"), format!("6. {name}")],
                Face::AtBeach => [p.queued_watch, p.queued, p.watching, p.no_chair, p.joined]
                    .map(String::from)
                    .to_vec(),
                Face::PuzzleSetup { posts: false, .. } => vec![p.no_posts.to_string()],
                Face::PuzzleSetup { posts: true, .. } => {
                    vec![String::new(), p.full.to_string()]
                }
                Face::PuzzleRunning => vec![p.running.to_string()],
                Face::PuzzleWon { .. } => {
                    [p.last_level, p.won, p.won_goal].map(String::from).to_vec()
                }
                Face::PuzzleLost { .. } => [p.lost, p.lost_goal].map(String::from).to_vec(),
                Face::Watching { .. } => [
                    tr.spectator_wait,
                    tr.spectator_vote_open,
                    p.pick_open,
                    tr.spectator_picked,
                ]
                .map(n)
                .to_vec(),
                Face::Editor { layer: true } => vec![format!("{} +", Pad::Select.name(tr))],
                Face::Results { keep, .. } | Face::Interlude { keep } => {
                    vec![
                        crate::app::replays::kept_lead(tr, *keep)
                            .map_or_else(String::new, String::from),
                    ]
                }
                Face::Menu
                | Face::Quitting
                | Face::Settings
                | Face::Controls
                | Face::MatchSetup { .. }
                | Face::StageSelect { .. }
                | Face::Replays
                | Face::Achievements
                | Face::Language
                | Face::NewVersion
                | Face::Door { .. }
                | Face::Paused
                | Face::Round
                | Face::Replay
                | Face::Editor { layer: false }
                | Face::LevelList
                | Face::EditorCard
                | Face::LevelCard
                | Face::Playtest(_) => vec![String::new()],
            };
            let controls = scheme::legend(tr, &face);
            for lead in leads {
                let line = match (lead.is_empty(), &face) {
                    (true, _) => controls.clone(),
                    (false, Face::Editor { layer: true }) => format!("{lead} {controls}"),
                    (false, _) => format!("{lead} | {controls}"),
                };
                out.push((face_screens(&face), line));
            }
        }
        out
    }

    /// A table whose results card opens `door`, as `after_round` reads it
    /// with a series running only for the next round.
    fn door_online(door: crate::app::play_input::AfterRound) -> Online {
        use crate::app::play_input::{AfterRound, after_round};
        let online = match door {
            AfterRound::NextRound | AfterRound::Menu => Online::default(),
            AfterRound::NextMap => Online(Some(crate::app::net::OnlineSession::new(
                crate::transport::UdpTransport::host(0).expect("socket"),
                crate::sim::Lockstep::new(0, vec![0, 1], crate::sim::DEFAULT_DELAY),
                2,
                crate::transport::MatchTerms::default(),
            ))),
            AfterRound::Lobby => {
                let mut session = crate::app::net::OnlineSession::new(
                    crate::transport::UdpTransport::host(0).expect("socket"),
                    crate::sim::Lockstep::observer(vec![0, 1], crate::sim::DEFAULT_DELAY),
                    2,
                    crate::transport::MatchTerms::default(),
                );
                session.home.from_lobby = true;
                Online(Some(session))
            }
        };
        let series_on = matches!(door, AfterRound::NextRound | AfterRound::NextMap);
        assert_eq!(after_round(&online, series_on), door);
        online
    }

    /// Every line the keyboard's prompt can show in `lang`, and the screen
    /// it is shown on and whether a recording is playing there. Built
    /// through the functions the screens call, so a line made of parts is
    /// measured whole, the mute key the play screens add included; the
    /// numbers and names in it are the widest they come.
    fn keyboard_prompts(lang: Lang) -> Vec<(Screen, bool, String)> {
        let fx = Fixture {
            settings: GameSettings {
                language: lang,
                ..GameSettings::default()
            },
            ..Fixture::default()
        };
        let tr = fx.settings.tr();
        let name = "W".repeat(crate::app::settings::NAME_MAX);
        let mut out = Vec::new();
        // The screens with one line, as the screen itself says it.
        for screen in [
            Screen::Menu,
            Screen::Settings,
            Screen::Controls,
            Screen::Achievements,
            Screen::Replays,
            Screen::StageSelect,
            Screen::Language,
            Screen::NewVersion,
            Screen::Editor,
        ] {
            out.push((screen, false, screen_text(screen, &fx.readout()).prompt));
        }
        out.push((
            Screen::StageSelect,
            false,
            tr.prompt_stages_custom.to_string(),
        ));
        for line in [
            tr.prompt_setup_game.to_string(),
            tr.prompt_setup_map.to_string(),
            tr.prompt_setup_players.to_string(),
            tr.prompt_setup_players_p1.to_string(),
            format!("{} | {}", tr.prompt_setup_players, tr.prompt_setup_partners),
            format!(
                "{} | {}",
                tr.prompt_setup_players_p1, tr.prompt_setup_partners
            ),
            typing_prompt(tr, false),
        ] {
            out.push((Screen::MatchSetup, false, line));
        }
        let spell =
            |line: &str| crate::app::editor::spell_editor_keys(line, tr, &fx.keycaps, &fx.settings);
        for line in [
            typing_prompt(tr, true),
            spell(tr.ed_playtest_prompt),
            spell(tr.ed_test_setup),
            spell(&crate::app::name_the_pair(tr, tr.ed_test_setup_coop)),
            spell(&crate::app::name_the_pair(
                tr,
                tr.ed_test_setup_coop_swapped,
            )),
            spell(tr.ed_test_setup_coop_custom),
            spell(tr.ed_test_retry),
            tr.ed_levels_keys.to_string(),
        ] {
            out.push((Screen::Editor, false, line));
        }
        let kick = format!(
            "{} | 7. {name}  8. {name}  9. {name} | {}",
            tr.lobby_kick_ask,
            fill(tr.lobby_kick_more, &[("p", "9"), ("n", "9")])
        );
        for line in [
            typing_prompt(tr, true),
            tr.lobby_watch_armed.to_string(),
            tr.lobby_queued_watch_prompt.to_string(),
            tr.lobby_queued_prompt.to_string(),
            tr.lobby_watching_prompt.to_string(),
            tr.lobby_no_chair_prompt.to_string(),
            tr.lobby_aboard_prompt.to_string(),
            kick,
            tr.lobby_broadcasting.to_string(),
            format!("{} | {}", tr.lobby_broadcasting, tr.prompt_setup_partners),
            fill(tr.lobby_partner_prompt, &[("n", "6")]),
            tr.lobby_pick_prompt.to_string(),
            tr.lobby_pick_prompt_no_pick.to_string(),
            tr.lobby_none_yet.to_string(),
            tr.lobby_join_list_one.to_string(),
            fill(tr.lobby_join_list, &[("n", "99")]),
        ] {
            out.push((Screen::Lobby, false, line));
        }
        // The play screens add the mute key to whatever they say.
        let mut play = |screen, replaying, line: String| {
            let line = fx.keycaps.legend(&format!("{line} | {}", tr.prompt_mute));
            out.push((screen, replaying, line));
        };
        for line in [
            tr.prompt_setup_no_posts.to_string(),
            tr.prompt_setup_custom.to_string(),
            crate::app::name_the_pair(tr, tr.prompt_setup_coop),
            crate::app::name_the_pair(tr, tr.prompt_setup_coop_swapped),
            tr.prompt_setup_full.to_string(),
            tr.prompt_setup.to_string(),
            tr.prompt_setup_swapped.to_string(),
            tr.prompt_running.to_string(),
            tr.last_level.to_string(),
            tr.prompt_won.to_string(),
            tr.prompt_won_goal.to_string(),
            tr.prompt_lost.to_string(),
            tr.prompt_lost_goal.to_string(),
            tr.prompt_paused.to_string(),
        ] {
            play(Screen::Puzzle, false, line);
        }
        let mut versus = vec![
            tr.prompt_versus_custom.to_string(),
            tr.prompt_versus_short.to_string(),
            tr.prompt_versus_local.to_string(),
            tr.prompt_versus_short_swapped.to_string(),
            tr.prompt_versus_local_swapped.to_string(),
            tr.prompt_paused.to_string(),
        ];
        let rest = format!("{} | {}", tr.lobby_chat_hint, tr.prompt_esc_menu);
        versus.push(rest.clone());
        versus.push(tr.series_pick_prompt.to_string());
        versus.push(tr.series_pick_prompt_no_pick.to_string());
        for call in [
            tr.spectator_call_hint.to_string(),
            fill(tr.spectator_wait, &[("n", "99")]),
            fill(tr.spectator_vote_open, &[("n", "99")]),
            fill(tr.spectator_pick_hint, &[("n", "99")]),
            fill(tr.spectator_picked, &[("p", &name), ("n", "99")]),
        ] {
            versus.push(format!("{call} | {rest}"));
        }
        // The results card's line, as `over_prompt` builds it, for every
        // door, rematch and offer to keep the round, and a spectator's
        // with the chat key in front.
        {
            use crate::app::play_input::AfterRound;
            use crate::app::rematch::Rematch;
            use crate::app::replays::KeepOffer;
            for door in [
                AfterRound::NextRound,
                AfterRound::NextMap,
                AfterRound::Lobby,
                AfterRound::Menu,
            ] {
                let online = door_online(door);
                let series_on = matches!(door, AfterRound::NextRound | AfterRound::NextMap);
                for keep in [KeepOffer::Nothing, KeepOffer::Offered, KeepOffer::Kept] {
                    let over = |again| {
                        crate::app::rematch::over_prompt(tr, &online, series_on, again, keep)
                    };
                    versus.push(format!("{} | {}", tr.lobby_chat_hint, over(None)));
                    for again in [
                        None,
                        Some(Rematch::Round),
                        Some(Rematch::Series),
                        Some(Rematch::SeriesAsked),
                    ] {
                        versus.push(over(again));
                    }
                }
            }
        }
        for line in versus {
            play(Screen::Versus, false, line);
        }
        play(Screen::Versus, true, tr.prompt_replay_transport.to_string());
        out
    }

    /// Every prompt in every language, from the keyboard and from a pad,
    /// fits the line its screen sets it on ([`super::super::prompt_band`]):
    /// one row at full size on the card screens and under a replay, one
    /// row shrunk no further than the fine print over the crab legend, and
    /// two rows where the screen keeps a second clear. At the narrowest the
    /// interface is, which is also the Steam Deck's 1280.
    ///
    /// A line too long wraps up over the card above it, or over the legend.
    #[test]
    fn every_prompt_fits_its_band() {
        use super::super::{prompt_band, prompt_fits};
        let mut long = Vec::new();
        for lang in crate::app::i18n::ALL_LANGS {
            let mut lines: Vec<(Shown, String, &str)> = keyboard_prompts(lang)
                .into_iter()
                .map(|(screen, replaying, line)| (vec![(screen, replaying)], line, "keys"))
                .collect();
            lines.extend(
                pad_prompts(lang.tr())
                    .into_iter()
                    .map(|(screens, line)| (screens, line, "pad")),
            );
            for (screens, line, hands) in lines {
                for (screen, replaying) in screens {
                    if let Err(why) = prompt_fits(prompt_band(screen, replaying), &line) {
                        long.push(format!("{lang:?} {hands} {screen:?}: {why}: {line}"));
                    }
                }
            }
        }
        assert!(long.is_empty(), "too long:\n{}", long.join("\n"));
    }
}
