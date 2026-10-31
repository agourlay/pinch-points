//! What the keys mean while a round is on: the phase-flow input systems.
//!
//! The cursor file answers "where is the cursor and how does it move";
//! this one answers "what does a press do to the round": place a post,
//! start the run, retry, advance the campaign, queue a versus action,
//! leave the results card. None of it moves a cursor, and two of the
//! systems never read one.

use crate::app::cursor::{Cursor, FLASH_SECS, keymap, seat_keymap};
use crate::app::settings::GameSettings;
use crate::app::{Campaign, LoadLevel, PendingActions, Phase, PlacementDenied, Screen, Sim};
use crate::sim::{Board, Direction, PlayerAction};
use bevy::prelude::*;

/// What a press asks of the seat a cursor plays for, whichever hands it
/// came from. The keyboard and the pad each translate their own presses
/// into these, and one place says what each means, in a puzzle's setup
/// ([`apply_setup`]) and in a versus round ([`queue_versus`]). Which press
/// counts (a fresh one, or one held down) is the translator's call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeatCommand {
    /// A signpost pointing this way, on the cursor's tile.
    Place(Direction),
    /// The signpost on the cursor's tile taken up.
    Remove,
    /// Every signpost of the seat's taken up.
    ClearAll,
    /// The run begun. A puzzle's setup only: a versus round is already
    /// running.
    Start,
}

/// A command in a puzzle's setup, straight onto the still board: the one
/// seat's arrows, whichever hands are placing them (see `Coop`), or on a
/// co-op stage the cursor's own hand ([`crate::app::cursor::stage_seat`]).
pub fn apply_setup(
    command: SeatCommand,
    hand: &mut Cursor,
    sim: &mut Sim,
    next_phase: &mut NextState<Phase>,
    denied: &mut MessageWriter<PlacementDenied>,
) {
    let seat = crate::app::cursor::stage_seat(&sim.0, hand);
    match command {
        SeatCommand::Place(dir) => {
            // CapPolicy::Reject enforces the inventory; surface the
            // no-op, and say which no it was.
            let spent = sim.0.out_of_signposts(seat, hand.x, hand.y);
            if !sim.0.place_signpost(seat, hand.x, hand.y, dir) {
                hand.flash = FLASH_SECS;
                denied.write(PlacementDenied {
                    player: seat,
                    out_of_signposts: spent,
                });
            }
        }
        SeatCommand::Remove => {
            let _ = sim.0.remove_signpost(seat, hand.x, hand.y);
        }
        SeatCommand::ClearAll => clear_setup(sim, seat),
        SeatCommand::Start => next_phase.set(Phase::Running),
    }
}

/// A command in a versus round, queued for the next tick as the cursor's
/// seat's one action (spec §7.6): a later command in the same window
/// overwrites the earlier one. A placement the board would refuse is not
/// queued; the cursor flashes and the refusal is said instead.
pub fn queue_versus(
    command: SeatCommand,
    cursor: &mut Cursor,
    board: &Board,
    pending: &mut PendingActions,
    denied: &mut MessageWriter<PlacementDenied>,
) {
    // Online or not, a cursor carries the seat it places for: online
    // spawns one cursor and gives it the local seat.
    let seat = cursor.player;
    let (x, y) = (cursor.x, cursor.y);
    let action = match command {
        SeatCommand::Place(dir) => {
            if !board.can_place_signpost(seat, x, y) {
                cursor.flash = FLASH_SECS;
                denied.write(PlacementDenied {
                    player: seat,
                    out_of_signposts: board.out_of_signposts(seat, x, y),
                });
                return;
            }
            PlayerAction::Place { x, y, dir }
        }
        SeatCommand::Remove => PlayerAction::Remove { x, y },
        // Clear-all: one removal per tick until none remain, staying
        // inside the one-action-per-tick input model.
        SeatCommand::ClearAll => match board.first_signpost_of(seat) {
            Some((x, y)) => PlayerAction::Remove { x, y },
            None => return,
        },
        SeatCommand::Start => return,
    };
    pending.0[usize::from(seat)] = action;
}

/// Puzzle setup phase: spend the inventory directly on the board, and
/// Enter to run. N/P, which jump between levels, are [`browse_levels`].
///
/// Every cursor here places the one seat's arrows: in co-op the second is a
/// second pair of hands (see [`crate::app::Coop`]), on the second seat's
/// keys, and the IJKL one-hand preset stands down for it.
pub fn setup_input(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<GameSettings>,
    mut sim: ResMut<Sim>,
    mut next_phase: ResMut<NextState<Phase>>,
    mut denied: MessageWriter<PlacementDenied>,
    mut cursors: Query<&mut Cursor>,
) {
    // Esc is the pause card's key; it handles leaving.
    let commit = settings.commit_in_play(crate::app::cursor::has_partner(cursors.iter()));
    for mut hand in cursors.iter_mut().filter(|c| c.player < 2) {
        let map = keymap(&settings, hand.player, commit);
        let places = map
            .places
            .into_iter()
            .filter(|(key, _)| keys.just_pressed(*key));
        let commands = places
            .map(|(_, dir)| SeatCommand::Place(dir))
            .chain(keys.just_pressed(map.remove).then_some(SeatCommand::Remove))
            .chain(
                keys.just_pressed(map.clear_all)
                    .then_some(SeatCommand::ClearAll),
            );
        for command in commands {
            apply_setup(command, &mut hand, &mut sim, &mut next_phase, &mut denied);
        }
    }
    if crate::app::menu_ui::enter(&keys)
        && let Some(mut hand) = cursors.iter_mut().find(|c| c.player == 0)
    {
        let start = SeatCommand::Start;
        apply_setup(start, &mut hand, &mut sim, &mut next_phase, &mut denied);
    }
}

/// Clear-all in a puzzle's setup: every arrow of `seat`'s up at once (on
/// a co-op stage, the presser's hand and not the partner's). The board is
/// still, so there is no tick to space the removals over as versus does.
/// The controls table, the guide and the pad's help all promise this key
/// in a puzzle too, where it used to do nothing.
pub fn clear_setup(sim: &mut Sim, seat: u8) {
    while let Some((x, y)) = sim.0.first_signpost_of(seat) {
        if !sim.0.remove_signpost(seat, x, y) {
            break;
        }
    }
}

/// N/P during puzzle setup: the next level or the one before, as far as
/// the ladder goes. A stage not yet open is refused with the same flash
/// and word as a placement that cannot go down.
pub fn browse_levels(
    keyboard: crate::app::keycaps::Keyboard,
    pads: crate::app::gamepad::face::PadsOnFace,
    mut ladder: crate::app::progress::Ladder,
    mut load: MessageWriter<LoadLevel>,
    mut denied: MessageWriter<PlacementDenied>,
    mut cursors: Query<&mut Cursor>,
) {
    let partner = crate::app::Coop(crate::app::cursor::has_partner(cursors.iter()));
    let Some(mut cursor) = cursors.iter_mut().find(|c| c.player == 0) else {
        return;
    };
    let at = ladder.campaign.index;
    // Within the section the stage is in, the shipped campaign or the
    // player's own levels, as Enter's run is (`Campaign::is_last`): N on
    // the last shipped stage used to walk on into the player's levels,
    // and P on the first wrapped onto the last of those.
    let section = ladder.campaign.section(ladder.campaign.section_of(at));
    let (start, end) = (section.start, section.end);
    let len = end - start;
    // Browsing forward stops at the ladder: the stage list is the only
    // way past a stage you have not cleared. Backward from the first
    // shipped stage wraps onto the last, which is as locked as anything
    // ahead: the same ladder, the same refusal.
    use crate::app::gamepad::scheme::Does;
    let asked = if keyboard.global('N') || pads.pressed(Does::NextLevel) {
        start + (at - start + 1) % len
    } else if keyboard.global('P') || pads.pressed(Does::PrevLevel) {
        start + (at - start + len - 1) % len
    } else {
        return;
    };
    // A section of one level has nowhere to browse to: reloading it would
    // only sweep the posts the player has set up off the board.
    if asked == at {
        return;
    }
    // A co-op stage among the player's own wants a second cursor this
    // screen was not opened with: the stage list is where one sits down.
    if !ladder.campaign.needs_partner(asked, partner) && ladder.climb_to(asked) {
        load.write(LoadLevel { keep_posts: false });
    } else {
        cursor.flash = FLASH_SECS;
        denied.write(PlacementDenied {
            player: 0,
            out_of_signposts: false,
        });
    }
}

/// Puzzle running phase: R (or the running table's pad button) resets to
/// setup, keeping placed posts.
///
/// Esc is the pause card's key, as it is in setup and in versus. Toggling
/// `Paused` on it as well, the card read that as the state to hand back on
/// closing, so Continue left the round frozen with no card up.
///
/// Not once the sim has settled the run: `check_outcome` reads the same
/// [`crate::sim::Level::outcome`] later this frame and calls it won or
/// lost, and an R let through as well had the clear recorded on the won
/// card's way in and then the beach reloaded from under it. The board did
/// finish, so the outcome stands and the R is the card's to answer.
pub fn running_input(
    keyboard: crate::app::keycaps::Keyboard,
    pads: crate::app::gamepad::face::PadsOnFace,
    sim: Res<Sim>,
    campaign: Res<Campaign>,
    mut load: MessageWriter<LoadLevel>,
) {
    let settled = campaign.current().outcome(&sim.0) != crate::sim::PuzzleOutcome::Running;
    let reset = keyboard.global('R') || pads.pressed(crate::app::gamepad::scheme::Does::Reset);
    if reset && !settled {
        load.write(LoadLevel { keep_posts: true });
    }
}

/// Puzzle won/lost phase: Enter advances (or retries after a loss), R resets to
/// setup with the arrows the run began with.
pub fn done_input(
    keyboard: crate::app::keycaps::Keyboard,
    phase: Res<State<Phase>>,
    coop: Res<crate::app::Coop>,
    mut campaign: ResMut<Campaign>,
    mut next_screen: ResMut<NextState<Screen>>,
    mut load: MessageWriter<LoadLevel>,
) {
    if keyboard.keys.just_pressed(KeyCode::Escape) {
        // Back to the stage list, which is the door every puzzle is entered
        // through; Esc again from there reaches the menu.
        next_screen.set(Screen::StageSelect);
        return;
    }
    if crate::app::menu_ui::enter(&keyboard.keys) {
        if *phase.get() != Phase::Won {
            load.write(LoadLevel { keep_posts: true });
            return;
        }
        // The end of the list is the end of the list: wrapping to level one
        // reads as the game not having noticed, the card saying "that was
        // the last level" and then putting you back on the first. The
        // shipped campaign ends with its last shipped stage too, rather
        // than walking on into the player's own levels.
        if campaign.is_last() {
            next_screen.set(Screen::Menu);
            return;
        }
        // The next of the player's own levels is a co-op stage and only
        // one is seated: back to the stage list, which says why.
        if campaign.needs_partner(campaign.index + 1, *coop) {
            next_screen.set(Screen::StageSelect);
            return;
        }
        campaign.index += 1;
        load.write(LoadLevel { keep_posts: false });
        // An R in the same frame would reload with this stage's posts
        // carried onto the next one.
        return;
    }
    if keyboard.global('R') {
        load.write(LoadLevel { keep_posts: true });
    }
}

/// Versus: every player's commits go through `PendingActions` and take effect
/// on the next 30 Hz tick, the input path rollback netcode will feed.
/// One action per player per tick (spec §7.6); a later press in the same
/// window overwrites the earlier one.
pub fn versus_input(
    keys: Res<ButtonInput<KeyCode>>,
    sim: Res<Sim>,
    settings: Res<GameSettings>,
    online: Res<crate::app::net::Online>,
    mut pending: ResMut<PendingActions>,
    mut denied: MessageWriter<PlacementDenied>,
    mut cursors: Query<&mut Cursor>,
) {
    // Esc is the pause card's key; it handles leaving (including a dead
    // online session or truncated replay).
    let board = &sim.0;
    let commit = settings.commit_in_play(true);
    for mut cursor in &mut cursors {
        let Some(map) = seat_keymap(&settings, cursor.player, online.0.is_some(), commit) else {
            continue;
        };
        // Clear-all is read held, not pressed: it queues one removal a
        // tick for as long as the key is down.
        let places = map
            .places
            .into_iter()
            .filter(|(key, _)| keys.just_pressed(*key));
        let commands = places
            .map(|(_, dir)| SeatCommand::Place(dir))
            .chain(keys.just_pressed(map.remove).then_some(SeatCommand::Remove))
            .chain(keys.pressed(map.clear_all).then_some(SeatCommand::ClearAll));
        for command in commands {
            queue_versus(command, &mut cursor, board, &mut pending, &mut denied);
        }
    }
}

/// Versus results: Enter continues a running series, otherwise back to
/// the menu - or, for a match formed in the beach lobby, back to that
/// lobby with the whole table still connected. At a table set up here, R
/// plays it again ([`crate::app::rematch::rematch`]).
///
/// Online, the host's Enter opens its map page for the next round
/// ([`crate::app::next_map`]), and the card picked there calls the next
/// round for the whole table, admitting anyone who queued while this one
/// played; every peer is walked back into the arena when the invitation
/// lands. Mid-series a
/// joiner's Enter is its own way out, since the series plays on without it;
/// once the match is over, Enter takes everyone back to the lobby.
pub fn versus_over_input(
    mut keyboard: crate::app::keycaps::KeyboardMut,
    (pause, time, mut next_map): (Res<crate::app::pause::PauseMenu>, Res<Time>, NextMapPage),
    mut tournament: ResMut<crate::app::tournament::Tournament>,
    mut online: ResMut<crate::app::net::Online>,
    mut homecoming: ResMut<crate::app::lobby::Homecoming>,
    mut next_screen: ResMut<NextState<Screen>>,
    mut again: crate::app::rematch::PlayAgain,
) {
    let series_on = tournament.is_running();
    // The invitation may already have arrived, in which case nobody
    // pressed anything here: the host decided and this peer follows.
    //
    // Left set on purpose: `end_versus` reads it as "keep me", and the
    // interlude clears it once the session is safely through.
    if online.0.as_ref().is_some_and(|session| session.next_round) {
        next_screen.set(Screen::Interlude);
        return;
    }
    // The host's map page for the next round has the keys while it is up.
    if next_map.page.is_up()
        && let Some(session) = &mut online.0
    {
        let empty = crate::app::match_setup::CustomBeaches::default();
        let beaches = again.beaches.as_deref().unwrap_or(&empty);
        // `PINCH_LOBBY_PICK=go`: the dev hook picks the card under the
        // cursor a second after the page opens, for a run that plays on.
        *next_map.held += 1;
        let mut go = ButtonInput::<KeyCode>::default();
        if crate::app::dev::lobby_pick_go() && *next_map.held == 30 {
            go.press(KeyCode::Enter);
        }
        let keys = match go.get_pressed().next() {
            Some(_) => &go,
            None => &*keyboard.keys,
        };
        let press = crate::app::next_map::next_map_keys(
            keys,
            &mut next_map.page,
            session,
            &mut again.config,
            &mut next_map.menu,
            beaches,
            tournament.standing(),
        );
        if let crate::app::next_map::NextMapPress::Called(standing) = press {
            if let Some(crate::transport::SeriesStanding { round, wins }) = standing {
                tournament.round = round;
                tournament.wins = wins;
            }
            next_screen.set(Screen::Interlude);
        }
        if !next_map.page.is_up() {
            *next_map.held = 0;
            // The key that put the page away is spent: the pause card
            // behind it in the frame, whose condition the page no longer
            // holds, would otherwise open on the same Esc.
            crate::app::menu_ui::spend(&mut keyboard.keys, &crate::app::menu_ui::CLOSERS);
        }
        return;
    }
    // The pause card's Enter is the card's. Not gated in the schedule like
    // the other play keys, because the invitation above is followed with
    // the card up or not.
    if pause.is_open() {
        return;
    }
    let keys = &*keyboard.keys;
    let now = time.elapsed_secs();
    let r = keyboard.global('R');
    let other_key = !r && keys.get_just_pressed().next().is_some();
    crate::app::rematch::settle_rematch_ask(&mut again.table, other_key, now);
    // The dev hook's Enter, pressed once on a series' card by the host.
    let hook = after_round(&online, series_on) == AfterRound::NextMap
        && crate::app::dev::lobby_pick()
        && !std::mem::replace(&mut *next_map.hooked, true);
    if !crate::app::menu_ui::enter(keys) && !hook {
        let rematch = crate::app::rematch::rematch(&again.table, &online, &tournament);
        if let Some(rematch) = rematch.filter(|_| r) {
            let pressed =
                crate::app::rematch::press_rematch(&mut again.table, rematch, &tournament, now);
            if let Some(rematch) = pressed {
                let rematch = crate::app::rematch::Again::Rematch(rematch);
                again.ask(rematch, &mut tournament, &mut next_screen);
            }
        }
        return;
    }
    match after_round(&online, series_on) {
        // A local series steps its map on by itself.
        AfterRound::NextRound => next_screen.set(Screen::Interlude),
        // The online host picks the next round's map on the map page,
        // which calls the round (`next_map_keys`).
        AfterRound::NextMap => {
            if let Some(session) = &mut online.0 {
                let empty = crate::app::match_setup::CustomBeaches::default();
                let beaches = again.beaches.as_deref().unwrap_or(&empty);
                crate::app::next_map::open_next_map(
                    &mut next_map.page,
                    session,
                    &mut again.config,
                    &mut next_map.menu,
                    beaches,
                );
            }
        }
        AfterRound::Menu => next_screen.set(Screen::Menu),
        AfterRound::Lobby => {
            // The match is over and it was formed in the lobby: the whole
            // table goes back there together, sockets and all.
            let session = online.0.take().expect("the lobby door needs a session");
            homecoming.0 = Some(session.back_to_the_lobby());
            next_screen.set(Screen::Lobby);
        }
    }
}

/// The host's map page between the rounds of an online series, and the
/// page's cursor.
#[derive(bevy::ecs::system::SystemParam)]
pub struct NextMapPage<'w, 's> {
    page: ResMut<'w, crate::app::next_map::NextMap>,
    menu: ResMut<'w, crate::app::match_setup::MatchMenu>,
    /// The dev hook's Enter on a series' card was pressed.
    hooked: Local<'s, bool>,
    /// Frames the page has been up, for the dev hook's pick.
    held: Local<'s, u32>,
}

/// What Enter does when the tide is in, named by the one function the key
/// itself reads.
///
/// The prompt line, the spectators' line and the results card all want
/// it, and each used to spell it out for itself: a flat "Enter: menu" for
/// a spectator walked back to the lobby, and "Enter: next round" on the
/// card of a joiner whose Enter leaves the series.
pub fn enter_door(
    tr: &crate::app::i18n::Tr,
    online: &crate::app::net::Online,
    series_on: bool,
) -> &'static str {
    match after_round(online, series_on) {
        AfterRound::NextRound => tr.tour_next,
        AfterRound::NextMap => tr.tour_next_map,
        AfterRound::Lobby => tr.prompt_enter_lobby,
        AfterRound::Menu => tr.prompt_enter_menu,
    }
}

/// Where Enter leads once a versus round is over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AfterRound {
    /// On into the next round of a series, through the interlude.
    NextRound,
    /// The online host, mid-series: its map page for the next round,
    /// which calls it ([`crate::app::next_map`]).
    NextMap,
    /// Back to the lobby the whole table came from.
    Lobby,
    Menu,
}

/// Which door Enter opens when the tide has come in.
///
/// A function of its own because two readers need the answer: the key
/// itself, and the prompt line that tells the player what the key does.
/// Spelled out twice, they disagreed - mid-series the card said "Enter:
/// next round" over a prompt line promising the menu, which is the game
/// arguing with itself.
pub fn after_round(online: &crate::app::net::Online, series_on: bool) -> AfterRound {
    let Some(session) = &online.0 else {
        // Local: a series plays on, a single round is done with.
        return match series_on {
            true => AfterRound::NextRound,
            false => AfterRound::Menu,
        };
    };
    if session.is_host() && series_on {
        return AfterRound::NextMap;
    }
    // Mid-series a joiner's Enter still means leaving, since the series
    // plays on without it, and a direct `PINCH_HOST` pair has no lobby to
    // go back to.
    match series_on || !session.home.from_lobby {
        true => AfterRound::Menu,
        false => AfterRound::Lobby,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::progress::Progress;
    use crate::sim::{Goal, Level, campaign_levels};

    /// The prompt line and the key have to name the same door. At a local
    /// table Enter carries a series on into the next round, and the prompt
    /// used to say "Enter: menu" under a card reading "Enter: next round".
    #[test]
    fn a_local_series_carries_enter_on_to_the_next_round() {
        let alone = crate::app::net::Online::default();
        assert_eq!(after_round(&alone, true), AfterRound::NextRound);
        assert_eq!(after_round(&alone, false), AfterRound::Menu);
    }

    /// Clear-all in setup takes every arrow up at once.
    #[test]
    fn clear_all_empties_a_puzzle_setup() {
        let level = campaign_levels()
            .into_iter()
            .find(|level| level.posts >= 2)
            .expect("a stage with two arrows");
        let mut sim = Sim(level.board());
        let mut placed = 0;
        for (x, y, _) in sim.0.tiles().collect::<Vec<_>>() {
            if placed < 2 && sim.0.place_signpost(0, x, y, Direction::Up) {
                placed += 1;
            }
        }
        assert_eq!(sim.0.signpost_count(0), 2);
        clear_setup(&mut sim, 0);
        assert_eq!(sim.0.signpost_count(0), 0);
    }

    /// Three shipped stages, and one of the player's own behind them when
    /// asked for.
    fn campaign_with(index: usize, mine: bool) -> Campaign {
        let mut levels: Vec<Level> = campaign_levels().into_iter().take(3).collect();
        if mine {
            let mut mine = levels[0].clone();
            mine.name = "Mine".into();
            levels.push(mine);
        }
        Campaign {
            index,
            builtins: 3,
            ..Campaign::tide_pool_of(levels)
        }
    }

    fn campaign(index: usize) -> Campaign {
        campaign_with(index, true)
    }

    fn tap(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(key);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
    }

    /// The won card, with the list at `index`.
    fn won(index: usize) -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_state::<Phase>();
        app.insert_resource(State::new(Screen::Puzzle));
        app.insert_resource(State::new(Phase::Won));
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::gamepad::PadFace>();
        app.init_resource::<crate::app::gamepad::LastDevice>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<crate::app::cursor::LiveSeatKeys>();
        app.insert_resource(campaign(index));
        app.init_resource::<crate::app::Coop>();
        app.add_message::<LoadLevel>();
        app.add_systems(Update, done_input);
        app
    }

    fn loads(app: &App) -> usize {
        app.world()
            .resource::<Messages<LoadLevel>>()
            .iter_current_update_messages()
            .count()
    }

    /// R on the frame the sim settles a run is not also a reload: the win
    /// is recorded as the won card comes up, and a reload behind it put
    /// the beach back in setup under that card. Mid-run R still resets.
    #[test]
    fn r_on_the_frame_a_puzzle_is_won_does_not_reload_it() {
        for (goal, reloads) in [(Goal::Bank(0), 0), (Goal::AllCrabs, 1)] {
            let mut app = App::new();
            app.init_resource::<ButtonInput<KeyCode>>();
            app.init_resource::<crate::app::gamepad::PadFace>();
            app.init_resource::<crate::app::gamepad::LastDevice>();
            app.init_resource::<crate::app::keycaps::KeyCaps>();
            app.init_resource::<crate::app::cursor::LiveSeatKeys>();
            let mut campaign = campaign(0);
            campaign.levels[0].goal = goal;
            app.insert_resource(Sim(campaign.current().board()));
            app.insert_resource(campaign);
            app.add_message::<LoadLevel>();
            app.add_systems(Update, running_input);
            tap(&mut app, KeyCode::KeyR);
            assert_eq!(loads(&app), reloads, "{goal:?}");
        }
    }

    /// The shipped campaign ends with its last shipped stage: Enter there
    /// goes to the menu rather than walking on into the player's own
    /// levels behind it, which are a shelf and not stage four.
    #[test]
    fn enter_after_the_last_shipped_stage_goes_to_the_menu() {
        let mut app = won(2);
        tap(&mut app, KeyCode::Enter);
        assert_eq!(loads(&app), 0, "nothing loaded");
        app.update();
        assert_eq!(*app.world().resource::<State<Screen>>().get(), Screen::Menu);
        assert_eq!(
            app.world().resource::<Campaign>().index,
            2,
            "the list stays put"
        );
    }

    /// In the middle of the list Enter loads the next stage, and the last
    /// of the player's own levels ends the list the same way the campaign
    /// ends.
    #[test]
    fn enter_walks_the_list_and_stops_at_its_end() {
        let mut app = won(0);
        tap(&mut app, KeyCode::Enter);
        assert_eq!(loads(&app), 1, "the next stage is loaded");
        assert_eq!(app.world().resource::<Campaign>().index, 1);
        app.update();
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Puzzle
        );

        let mut app = won(3);
        tap(&mut app, KeyCode::Enter);
        assert_eq!(loads(&app), 0);
        app.update();
        assert_eq!(*app.world().resource::<State<Screen>>().get(), Screen::Menu);
    }

    /// Whether each reload asked this frame keeps the posts.
    fn kept(app: &App) -> Vec<bool> {
        app.world()
            .resource::<Messages<LoadLevel>>()
            .iter_current_update_messages()
            .map(|load| load.keep_posts)
            .collect()
    }

    /// R on either card and Enter after a loss retry the stage with the
    /// arrows the run began with; Enter after a win goes on to the next
    /// stage bare, even with R in the same frame, which would otherwise
    /// carry this stage's arrows onto it.
    #[test]
    fn a_retry_keeps_the_arrows_and_the_next_stage_starts_bare() {
        let mut app = won(0);
        tap(&mut app, KeyCode::KeyR);
        assert_eq!(kept(&app), [true], "R on the won card");
        assert_eq!(app.world().resource::<Campaign>().index, 0);

        app.insert_resource(State::new(Phase::Lost));
        tap(&mut app, KeyCode::Enter);
        assert_eq!(kept(&app), [true], "Enter on the lost card");
        assert_eq!(app.world().resource::<Campaign>().index, 0);

        app.insert_resource(State::new(Phase::Won));
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::Enter);
            keys.press(KeyCode::KeyR);
        }
        app.update();
        assert_eq!(kept(&app), [false], "Enter and R together");
        assert_eq!(app.world().resource::<Campaign>().index, 1);
    }

    /// Won, with a co-op stage next and only one player seated: Enter goes
    /// back to the stage list, which says why, rather than loading a stage
    /// nobody can finish alone.
    #[test]
    fn enter_before_a_coop_stage_alone_goes_to_the_list() {
        let mut app = won(0);
        {
            let mut campaign = app.world_mut().resource_mut::<Campaign>();
            campaign.levels[1] = campaign.levels[1].with_hands(Some([1, 1]), 2);
        }
        tap(&mut app, KeyCode::Enter);
        assert!(kept(&app).is_empty(), "nothing loaded");
        assert_eq!(app.world().resource::<Campaign>().index, 0);
        app.update();
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::StageSelect
        );
    }

    /// Num Enter is Enter on the puzzle's cards as on every menu and the
    /// versus results: it takes the next stage from the Won card.
    #[test]
    fn num_enter_takes_the_won_cards_door() {
        let mut app = won(0);
        tap(&mut app, KeyCode::NumpadEnter);
        assert_eq!(loads(&app), 1, "the next stage is loaded");
        assert_eq!(app.world().resource::<Campaign>().index, 1);
    }

    /// Browsing from setup honours the ladder both ways: P from the first
    /// stage wraps onto the end of the shipped list, which is as locked as
    /// any stage ahead, and is refused with the same flash; once the first
    /// stage is cleared N opens the second, and P walks back. A level of
    /// the player's own is on the shelf behind them, and the wrap does not
    /// land on it: browsing stays within the shipped campaign.
    #[test]
    fn browsing_from_setup_honours_the_ladder_both_ways() {
        let mut app = browsing_app(campaign_with(0, true));
        tap(&mut app, KeyCode::KeyP);
        assert_eq!(
            app.world().resource::<Campaign>().index,
            0,
            "the end is locked"
        );
        assert_eq!(denials(&app), 1, "and refused out loud");
        tap(&mut app, KeyCode::KeyN);
        assert_eq!(
            app.world().resource::<Campaign>().index,
            0,
            "so is the second"
        );
        assert_eq!(denials(&app), 1);

        let first = app.world().resource::<Campaign>().levels[0].name.clone();
        app.world_mut().resource_mut::<Progress>().mark(&first);
        tap(&mut app, KeyCode::KeyN);
        assert_eq!(
            app.world().resource::<Campaign>().index,
            1,
            "cleared, so open"
        );
        assert_eq!(loads(&app), 1);
        tap(&mut app, KeyCode::KeyP);
        assert_eq!(app.world().resource::<Campaign>().index, 0, "and back");
        assert_eq!(denials(&app), 0);
    }

    /// Setup with `campaign` loaded and browsing live, P1's cursor down.
    fn browsing_app(campaign: Campaign) -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Phase>();
        app.insert_resource(State::new(Phase::Setup));
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::gamepad::PadFace>();
        app.init_resource::<crate::app::gamepad::LastDevice>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<crate::app::cursor::LiveSeatKeys>();
        app.insert_resource(GameSettings::default());
        app.init_resource::<Progress>();
        app.insert_resource(Sim(campaign.current().board()));
        app.insert_resource(campaign);
        app.add_message::<LoadLevel>();
        app.add_message::<PlacementDenied>();
        app.add_systems(Update, browse_levels);
        let mut cursor = Cursor::seated(0);
        (cursor.x, cursor.y) = (1, 1);
        app.world_mut().spawn((cursor, Transform::default()));
        app
    }

    fn denials(app: &App) -> usize {
        app.world()
            .resource::<Messages<PlacementDenied>>()
            .iter_current_update_messages()
            .count()
    }

    /// The player's own shelf with one level on it: N and P have nowhere
    /// to go, and must not reload the level, which would sweep the posts
    /// already set up off the board.
    #[test]
    fn browsing_a_shelf_of_one_keeps_the_setup() {
        let mut app = browsing_app(campaign_with(3, true));
        for key in [KeyCode::KeyN, KeyCode::KeyP] {
            tap(&mut app, key);
            assert_eq!(app.world().resource::<Campaign>().index, 3, "{key:?}");
            assert_eq!(loads(&app), 0, "{key:?} reloads nothing");
            assert_eq!(denials(&app), 0, "{key:?} refuses nothing");
        }
    }

    /// `command` from seat `seat`'s cursor at `(x, y)` on `board`, queued
    /// over `pending` as a versus tick would queue it.
    fn queued(
        board: &Board,
        seat: u8,
        (x, y): (u8, u8),
        command: SeatCommand,
        pending: PendingActions,
    ) -> PendingActions {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.add_message::<PlacementDenied>();
        let board = board.clone();
        app.world_mut()
            .run_system_once(move |mut denied: MessageWriter<PlacementDenied>| {
                let mut cursor = Cursor::seated(seat);
                (cursor.x, cursor.y) = (x, y);
                let mut pending = PendingActions(pending.0);
                queue_versus(command, &mut cursor, &board, &mut pending, &mut denied);
                pending
            })
            .expect("queued")
    }

    /// Clear-all in a round takes the seat's own arrows up one a tick, the
    /// first of them each time, until none are left, and then queues
    /// nothing. A rival's arrow ahead of them in reading order is never
    /// one of them, and a held RB that queued one removal and stopped
    /// would leave the rest standing.
    #[test]
    fn clear_all_in_a_round_takes_the_seats_own_arrows_one_a_tick() {
        let mut board = crate::sim::classic_arena(false, 2);
        let tiles: Vec<(u8, u8)> = board.tiles().map(|(x, y, _)| (x, y)).collect();
        let mut free = tiles
            .into_iter()
            .filter(|&(x, y)| board.can_place_signpost(1, x, y))
            .collect::<Vec<_>>()
            .into_iter();
        let rival = free.next().expect("a tile");
        assert!(board.place_signpost(1, rival.0, rival.1, Direction::Up));
        let mut own = Vec::new();
        for (x, y) in free {
            if own.len() < 2 && board.place_signpost(0, x, y, Direction::Left) {
                own.push((x, y));
            }
        }
        assert_eq!(own.len(), 2);
        let none = || PendingActions([PlayerAction::None; crate::sim::MAX_PLAYERS]);
        for &(x, y) in &own {
            let pending = queued(&board, 0, (0, 0), SeatCommand::ClearAll, none());
            assert_eq!(pending.0[0], PlayerAction::Remove { x, y });
            assert_eq!(pending.0[1], PlayerAction::None, "the rival's seat");
            assert!(board.remove_signpost(0, x, y));
        }
        let earlier = PlayerAction::Place {
            x: 1,
            y: 1,
            dir: Direction::Up,
        };
        let mut before = none();
        before.0[0] = earlier;
        let pending = queued(&board, 0, (0, 0), SeatCommand::ClearAll, before);
        assert_eq!(pending.0[0], earlier, "nothing left, nothing queued");
        assert!(board.signpost_at(rival.0, rival.1).is_some());

        // Remove is the tile under the cursor, for the cursor's seat.
        let pending = queued(&board, 1, rival, SeatCommand::Remove, none());
        assert_eq!(
            pending.0[1],
            PlayerAction::Remove {
                x: rival.0,
                y: rival.1
            }
        );
        assert_eq!(pending.0[0], PlayerAction::None);
    }
}
