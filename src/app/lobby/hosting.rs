//! Putting a beach on the air and keeping it there: the beacon, the peers
//! who turn up, and the moment the round begins.
//!
//! The host is the one player who both talks and starts, which is why
//! nothing here acts on Enter while a line is being typed.

use super::*;
use crate::app::i18n::fill;

/// What a frame does about hosting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HostStep {
    /// Ask who is asking, before anything is put on the air.
    Ask,
    /// Everything is answered: put the beach up.
    Go,
    Nothing,
}

/// What a frame knows about hosting.
///
/// A struct rather than four bools in a row, because `host_step(false,
/// true, false, false)` says nothing at the call site and any two of them
/// could be swapped without the compiler noticing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct HostAsk {
    /// The beach has a name, which is the last thing the asking waits on.
    pub answered: bool,
    /// H, this frame.
    pub pressed_h: bool,
    /// Already at a beach, one's own or somebody else's.
    pub busy: bool,
    /// The unattended dev hook, which must never be stopped to answer a
    /// question nobody is sitting there to answer.
    pub auto: bool,
}

/// A frame in which nothing is pressed, nowhere has been reached and
/// nobody is automating. Every field named once, in one place, so a field
/// added later cannot be silently defaulted into tests that never mention
/// it, and so the tests say only what they change.
///
/// Beside the struct rather than inside a test module, because two test
/// modules want it and each had grown its own identical copy.
#[cfg(test)]
pub(super) fn idle() -> HostAsk {
    HostAsk {
        answered: false,
        pressed_h: false,
        busy: false,
        auto: false,
    }
}

/// Where this beach is, written the way somebody else would have to type
/// it: `ip:port`.
///
/// `None` when the machine has no address worth reading out: no route off
/// itself, or a socket that will not say what port it took. The
/// beacon carries the address for everyone who can hear it; this is for
/// the friend who cannot.
pub(super) fn address_here(transport: &UdpTransport) -> Option<String> {
    let port = transport.local_addr().ok()?.port();
    let ip = crate::transport::local_ip()?;
    Some(SocketAddr::new(ip, port).to_string())
}

/// Whether this frame asks about hosting, does it, or neither.
pub(super) fn host_step(ask: HostAsk) -> HostStep {
    if ask.answered {
        return HostStep::Go;
    }
    if ask.busy {
        return HostStep::Nothing;
    }
    match (ask.pressed_h || ask.auto, ask.auto) {
        (true, true) => HostStep::Go,
        (true, false) => HostStep::Ask,
        (false, _) => HostStep::Nothing,
    }
}

/// Hosting: announce on a timer, gather joiners (up to five rivals, plus
/// onlookers), and launch on Enter (or once the auto-host quota fills).
#[allow(clippy::too_many_arguments)]
pub fn host_tick(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<GameSettings>,
    config: Res<crate::app::match_setup::MatchConfig>,
    beaches: Res<crate::app::match_setup::CustomBeaches>,
    mut state: ResMut<LobbyState>,
    mut online: ResMut<Online>,
    mut tournament: ResMut<crate::app::tournament::Tournament>,
    mut next_screen: ResMut<NextState<Screen>>,
    mut next_vphase: ResMut<NextState<VersusPhase>>,
) {
    if state.hosting.is_none() {
        return;
    }
    let tr = settings.tr();
    state.announce_in -= time.delta_secs();
    let do_announce = state.announce_in <= 0.0;
    if do_announce {
        state.announce_in = ANNOUNCE_EVERY;
    }
    // Read before the watcher list is taken below, and a frame stale by
    // the time it goes out, which costs nothing at one beacon a second.
    let taken = 1 + state.players_aboard() as u8;
    let game_name = state.game_name.clone();
    let mut watchers = std::mem::take(&mut state.watchers);
    let mut picked = Picked::default();
    let mut silence = std::mem::take(&mut state.peer_silence);
    let delta = time.delta_secs();
    for since in silence.iter_mut() {
        *since += delta;
    }
    // The host holds a seat too, so the table is never empty. Seats are the
    // hard limit rather than the configured count: an AI seat gives way to a
    // player who turns up for it.
    let on_air = crate::transport::OnAir {
        name: &game_name,
        host: &settings.names[0],
        taken,
        seats: MAX_PLAYERS as u8,
    };
    work_the_socket(
        &mut state,
        do_announce,
        on_air,
        &mut watchers,
        &mut silence,
        &mut picked,
    );
    for (_, name, text) in picked.said {
        let who = crate::transport::name_from_wire(&name);
        let line = crate::transport::chat_from_wire(&text);
        state.say(&who, &line);
    }
    note_arrivals_and_departures(&mut state, tr, picked.greeted, silence);
    // Counted after the departures, so a table that just lost somebody is
    // not announced as still holding them.
    let joined = match &state.hosting {
        Some((_, transport)) => transport.peer_count(),
        None => 0,
    };
    state.watchers = watchers;
    state.joined_peers = joined;
    // The host's own view of the table, and the peers': the same list,
    // sent on every beacon tick so a lost one costs a second, not a screen
    // that stays wrong until somebody joins or leaves.
    state.table = state.roster(tr, &settings.names[0]);
    if do_announce && let Some((_, transport)) = &state.hosting {
        let mut names = [[0u8; crate::transport::WIRE_NAME]; MAX_PLAYERS];
        for (slot, who) in names.iter_mut().zip(&state.table) {
            *slot = crate::transport::wire_name(who);
        }
        // The dials travel with the roster so a joiner's terms card shows
        // the match it is joining. The seed is not one of them yet - it is
        // struck fresh at launch - so a placeholder rides along and the
        // card ignores it.
        let terms = crate::app::match_setup::terms(&config, settings.team_mode, 0);
        transport.send(NetMsg::Roster {
            seats: state.table.len().min(MAX_PLAYERS) as u8,
            names,
            terms,
        });
    }
    let players_aboard = state.players_aboard();
    if joined > 0 {
        state.feedback = gathering_feedback(tr, &config, players_aboard, state.watchers.len());
    } else {
        // The last peer timed out: without this the line kept saying "1
        // rival aboard" over an empty table until somebody else turned up,
        // and Enter did nothing. Back to the waiting message it started on.
        state.feedback = match &state.hosting {
            Some((_, transport)) => match transport.local_addr() {
                Ok(addr) => fill(tr.lobby_hosting, &[("p", &addr.port().to_string())]),
                Err(_) => tr.lobby_hosting_noport.to_string(),
            },
            None => tr.lobby_hosting_noport.to_string(),
        };
    }
    let quota = crate::app::dev::auto_host_quota();
    let launch = should_launch(
        joined,
        state.typing.is_some(),
        keys.just_pressed(KeyCode::Enter),
        quota,
    );
    if launch {
        launch_the_match(
            &mut state,
            &settings,
            &config,
            &beaches,
            joined,
            &mut online,
            &mut tournament,
            &mut next_screen,
            &mut next_vphase,
        );
    }
}

/// Put the match on: seat everyone who came to play, agree the terms, and
/// walk the whole table into the arena.
///
/// Lifted out of `host_tick`, which had grown to nine jobs and two hundred
/// and fifty lines by accretion, a feature at a time, each reasonable on
/// its own. This is the one of them with a beginning and an end.
#[allow(clippy::too_many_arguments)]
fn launch_the_match(
    state: &mut LobbyState,
    settings: &GameSettings,
    config: &MatchConfig,
    beaches: &crate::app::match_setup::CustomBeaches,
    joined: usize,
    online: &mut Online,
    tournament: &mut crate::app::tournament::Tournament,
    next_screen: &mut NextState<Screen>,
    next_vphase: &mut NextState<VersusPhase>,
) {
    let watchers = state.watchers.clone();
    // The beacon does not stop at launch, it changes what it says: a
    // running beach cannot be joined, since lockstep has nothing to
    // catch a latecomer up with, but it can be queued for. The
    // announcer rides along into the session to keep saying so.
    let (announcer, transport) = state.hosting.take().expect("checked above");
    let plan = seat_plan(joined, &watchers);
    let humans = 1 + plan.iter().filter(|seat| seat.is_some()).count() as u8;
    // AI seats come from the host's match setup, and sit behind the
    // humans. The host's dials and its team-scoring setting travel with
    // them: a match is played on one set of terms, not four.
    //
    // A beach needs two castles, so the AI floor is raised to reach two
    // seats when only onlookers turned up (every peer a watcher, or a lone
    // watcher aboard): a one-seat `Start` is refused by every joiner's
    // decoder and would leave the host playing an empty beach. The same
    // floor `call_next_round` keeps between rounds.
    let bots = config
        .bots
        .min(MAX_PLAYERS as u8 - humans)
        .max(2u8.saturating_sub(humans));
    let seats = humans + bots;
    let seed = crate::app::clock::fresh_seed();
    // A beach the host built travels with the invitation; a generated one
    // is already described by the seed in the terms.
    let beach = crate::app::match_setup::beach_bytes(config, seats, beaches);
    let terms = MatchTerms {
        bots,
        // The beach has to hold everyone who turned up, which the host
        // could not have known when it picked one.
        map: map_for(config, seats).index() as u8,
        ..crate::app::match_setup::terms(config, settings.team_mode, seed)
    };
    // The table's names: the host is seat 0 under its own P1 name, and
    // each seated peer under the name its greeting carried. Empty slots
    // (AI seats, the nameless) fall back to seat labels on every screen.
    let mut names: [String; MAX_PLAYERS] = Default::default();
    names[0].clone_from(&settings.names[0]);
    for (peer, slot) in plan.iter().enumerate() {
        if let Some(seat) = slot
            && let Some(name) = state.peer_names.get(peer)
        {
            names[usize::from(*seat)] = name.clone();
        }
    }
    let wire_names = std::array::from_fn(|i| crate::transport::wire_name(&names[i]));
    // Seats go to the peers that came to play, in peer order; the
    // onlookers - and anyone the table ran out of chairs for - are told
    // they are watching.
    // A series begins at round one with an empty tally; a single round
    // carries a zero round the joiner reads as "no series".
    let (round, wins) = match terms.is_series() {
        true => (1u8, [0u8; MAX_PLAYERS]),
        false => (0, [0; MAX_PLAYERS]),
    };
    for (peer, slot) in plan.iter().enumerate() {
        transport.send_to(
            peer,
            NetMsg::Start {
                seats,
                seat: *slot,
                terms,
                names: wire_names,
                round,
                wins,
                beach: beach.clone(),
            },
        );
    }
    let peer_seats = plan;
    let mut session = OnlineSession::new(
        transport,
        Lockstep::new(0, (0..humans).collect(), DEFAULT_DELAY),
        seats,
        terms,
    );
    // The host plays on the same bytes it sent, not on its own copy of the
    // file: if the two ever disagreed, the hash check would find it and
    // nobody could say which of them was right.
    session.beach = beach;
    session.peer_seats = peer_seats;
    session.names = names;
    session.stay_on_air(announcer);
    session.from_lobby = true;
    session.game_name = state.game_name.clone();
    online.0 = Some(session);
    // The series is part of the terms, so every peer knows it is one
    // and tallies the same rounds. Without that the host alone would
    // count, and only the host would see a champion.
    *tournament = crate::app::tournament::Tournament::from_terms(terms, round, wins);
    next_vphase.set(VersusPhase::Running);
    next_screen.set(Screen::Versus);
}

/// Fold this tick's greetings and silences into the table: who has just
/// arrived, and who has stopped answering.
///
/// Both are news for the feed and for every peer, and both are the same
/// kind of bookkeeping, which is why they sit together rather than at
/// opposite ends of a two-hundred-line system.
fn note_arrivals_and_departures(
    state: &mut LobbyState,
    tr: &'static crate::app::i18n::Tr,
    greeted: Vec<(usize, String)>,
    silence: Vec<f32>,
) {
    for (from, told) in greeted {
        if state.peer_names.len() <= from {
            state.peer_names.resize(from + 1, String::new());
        }
        // A greeting arrives every second; only the first one, or a change
        // of mind about the name, is news worth putting in the feed.
        let arrived = state.peer_names[from] != told;
        state.peer_names[from].clone_from(&told);
        if arrived {
            let notice = fill(tr.lobby_joined, &[("p", &told)]);
            state.say("", &notice);
            announce_to_table(state, "", &notice);
        }
    }
    state.peer_silence = silence;
    // Kept as long as the silence list it is shifted beside, so a watcher
    // at the top of the table - which greets with `Watch`, never `Hello`,
    // and so is never named here - does not leave the two lists different
    // lengths for `forget_peer` to trip over.
    if state.peer_names.len() < state.peer_silence.len() {
        state
            .peer_names
            .resize(state.peer_silence.len(), String::new());
    }
    // Anyone unheard for too long has walked away: UDP will not say so, and
    // a table that keeps counting them never has room again.
    let gone: Vec<usize> = (0..state.peer_silence.len())
        .rev()
        .filter(|peer| state.peer_silence[*peer] > PEER_TTL)
        .collect();
    for peer in gone {
        let who = state.peer_names.get(peer).cloned().unwrap_or_default();
        state.forget_peer(peer);
        if !who.is_empty() {
            let notice = fill(tr.lobby_left, &[("p", &who)]);
            state.say("", &notice);
            announce_to_table(state, "", &notice);
        }
    }
}

/// What one tick on the socket picked up: who named themselves, and what
/// was said. Bundled because they travel together and a function with
/// eight arguments is a function asking to be given a noun.
#[derive(Default)]
struct Picked {
    greeted: Vec<(usize, String)>,
    said: Vec<(
        usize,
        crate::transport::WireName,
        crate::transport::WireChat,
    )>,
}

/// One tick on the host's own socket: put the beacon up, take in what the
/// peers sent, and pass the chat along to the rest of the table.
///
/// Kept whole because the borrow of `hosting` runs the length of it; what
/// it learns goes back to the caller to be folded into the lobby, which
/// needs the rest of the state that borrow is holding.
#[cfg_attr(debug_assertions, track_caller)]
fn work_the_socket(
    state: &mut LobbyState,
    do_announce: bool,
    on_air: crate::transport::OnAir<'_>,
    watchers: &mut Vec<usize>,
    silence: &mut Vec<f32>,
    picked: &mut Picked,
) {
    let Picked { greeted, said } = picked;
    debug_assert!(
        state.hosting.is_some(),
        "working a socket that is not being hosted"
    );
    let mut heard_from: Vec<usize> = Vec::new();
    if let Some((announcer, transport)) = &mut state.hosting {
        if do_announce && let Ok(addr) = transport.local_addr() {
            announcer.announce(addr.port(), on_air);
        }
        // recv_all registers joiners from their greetings; a Watch tells us
        // that peer is here to look, not to play, and a Hello says what to
        // call whoever sent it.
        for (msg, from) in transport.recv_all() {
            heard_from.push(from);
            // A peer the socket has only just registered has no slot in
            // any of the lists kept alongside it.
            while silence.len() < transport.peer_count() {
                silence.push(0.0);
            }
            match msg {
                NetMsg::Watch if !watchers.contains(&from) => watchers.push(from),
                NetMsg::Hello { name } => {
                    let told = crate::transport::name_from_wire(&name);
                    if !told.is_empty() {
                        greeted.push((from, told));
                    }
                }
                // The spokes of the star cannot hear each other, so the
                // hub repeats what it is told before showing it.
                NetMsg::Chat { name, text } => said.push((from, name, text)),
                NetMsg::Watch
                | NetMsg::Input(_)
                | NetMsg::Hash { .. }
                | NetMsg::Start { .. }
                | NetMsg::Pause { .. }
                | NetMsg::Resume { .. }
                | NetMsg::Queued { .. }
                | NetMsg::Roster { .. }
                | NetMsg::Abandoned { .. }
                | NetMsg::Incompatible { .. } => {}
            }
        }
        // Anything at all from a peer is proof it is still there.
        for heard in &heard_from {
            if let Some(silence) = silence.get_mut(*heard) {
                *silence = 0.0;
            }
        }
        for (from, name, text) in said.iter() {
            for other in 0..transport.peer_count() {
                if other != *from {
                    transport.send_to(
                        other,
                        NetMsg::Chat {
                            name: *name,
                            text: *text,
                        },
                    );
                }
            }
        }
    }
}

/// The line under the title while gathering: who is aboard, who is only
/// watching, and how many chairs the AI will take in behind them.
///
/// Pure, so what a host reads while waiting is checkable without a socket.
pub(super) fn gathering_feedback(
    tr: &'static crate::app::i18n::Tr,
    config: &MatchConfig,
    players_aboard: usize,
    watching: usize,
) -> String {
    let mut line = match players_aboard {
        1 => tr.lobby_rivals_one.to_string(),
        n => fill(tr.lobby_rivals_many, &[("n", &n.to_string())]),
    };
    if watching > 0 {
        line += &fill(tr.lobby_watchers, &[("n", &watching.to_string())]);
    }
    // The AI seats come from the host's dials, not from this count, so say
    // how many are coming rather than surprising the table with them.
    let bots = config
        .bots
        .min((MAX_PLAYERS as u8).saturating_sub((players_aboard + 1) as u8));
    if bots > 0 {
        line += &fill(tr.lobby_ai_seats, &[("n", &bots.to_string())]);
    }
    line
}

/// Whether this frame starts the match.
///
/// Not while a line is being typed, which is the entire job of this: the
/// host is the one player who both talks and starts, `host_tick` runs
/// ahead of the input that owns the keyboard, and Enter therefore launched
/// the match out from under a half-written sentence. It could not send a
/// message at all.
///
/// The unattended quota is exempt: nothing is being typed on a machine
/// nobody is sitting at.
pub(super) fn should_launch(
    joined: usize,
    typing: bool,
    enter: bool,
    quota: Option<usize>,
) -> bool {
    if joined == 0 {
        return false;
    }
    match quota {
        Some(quota) if joined >= quota => true,
        _ => enter && !typing,
    }
}

/// Say something to every peer at the table, as the host. An empty `who`
/// is the lobby speaking rather than a player.
pub(super) fn announce_to_table(state: &LobbyState, who: &str, line: &str) {
    if let Some((_, transport)) = &state.hosting {
        transport.send(NetMsg::Chat {
            name: crate::transport::wire_name(who),
            text: crate::transport::wire_chat(line),
        });
    }
}

/// Which peers get a seat, in peer order, and which are watching (`None`).
///
/// Seats run out before connections do: the socket takes nine peers and the
/// table has six chairs. The surplus is seated as onlookers rather than
/// handed a seat number the sim has no slot for.
pub(super) fn seat_plan(peers: usize, watchers: &[usize]) -> Vec<Option<u8>> {
    debug_assert!(
        watchers.iter().all(|w| *w < peers.max(1)),
        "a watcher at {watchers:?} among {peers} peers"
    );
    let mut next = 1u8; // seat 0 is the host's
    (0..peers)
        .map(|peer| {
            if watchers.contains(&peer) || usize::from(next) >= MAX_PLAYERS {
                return None;
            }
            let seat = next;
            next += 1;
            Some(seat)
        })
        .collect()
}

/// Tell the network a hosted beach is going away, if this lobby is hosting
/// one. Both ways out of hosting come through here: leaving the screen, and
/// starting the match.
pub(super) fn say_goodbye(state: &LobbyState) {
    if let Some((announcer, transport)) = &state.hosting
        && let Ok(addr) = transport.local_addr()
    {
        announcer.closing(addr.port());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seats go to the peers that came to play, in the order they arrived,
    /// with the host holding seat zero throughout.
    #[test]
    fn the_table_is_dealt_in_arrival_order_behind_the_host() {
        assert_eq!(seat_plan(0, &[]), Vec::<Option<u8>>::new());
        assert_eq!(seat_plan(1, &[]), vec![Some(1)], "the host keeps zero");
        assert_eq!(seat_plan(3, &[]), vec![Some(1), Some(2), Some(3)]);
    }

    /// An onlooker takes no chair, and the players behind it close up
    /// rather than inheriting a gap.
    #[test]
    fn watchers_are_stepped_over_and_the_seats_close_up() {
        assert_eq!(seat_plan(3, &[0]), vec![None, Some(1), Some(2)]);
        assert_eq!(seat_plan(3, &[1]), vec![Some(1), None, Some(2)]);
        assert_eq!(seat_plan(3, &[0, 1, 2]), vec![None, None, None]);
    }

    /// Every answer `host_step` can give. It asks who is asking before
    /// anything goes on the air, does nothing once there is already a
    /// beach, and never stops the dev hook to ask a question nobody is
    /// sitting there to answer.
    #[test]
    fn hosting_asks_before_it_announces() {
        assert_eq!(host_step(idle()), HostStep::Nothing);
        assert_eq!(
            host_step(HostAsk {
                pressed_h: true,
                ..idle()
            }),
            HostStep::Ask
        );
        assert_eq!(
            host_step(HostAsk {
                answered: true,
                ..idle()
            }),
            HostStep::Go
        );
        // Busy is busy, pressed or not: without this the prompt would
        // open over a running lobby.
        assert_eq!(
            host_step(HostAsk {
                pressed_h: true,
                busy: true,
                ..idle()
            }),
            HostStep::Nothing
        );
        assert_eq!(
            host_step(HostAsk {
                busy: true,
                ..idle()
            }),
            HostStep::Nothing
        );
        // And the unattended hook goes straight through, H or no H: a
        // prompt on a machine nobody is sitting at is a hang.
        assert_eq!(
            host_step(HostAsk {
                auto: true,
                ..idle()
            }),
            HostStep::Go
        );
        assert_eq!(
            host_step(HostAsk {
                pressed_h: true,
                auto: true,
                ..idle()
            }),
            HostStep::Go
        );
    }

    /// The socket takes more peers than the table has chairs, so the extras
    /// watch rather than take a seat the sim has no slot for.
    #[test]
    fn the_table_runs_out_of_chairs_before_the_socket_runs_out_of_peers() {
        use crate::sim::MAX_PLAYERS;

        // Four rivals on a six-seat table: seats 1-4 behind the host's 0.
        assert_eq!(seat_plan(4, &[]), vec![Some(1), Some(2), Some(3), Some(4)]);
        // Peers 1 and 3 came to watch, so the seats close up behind them.
        assert_eq!(seat_plan(4, &[1, 3]), vec![Some(1), None, Some(2), None]);
        // Every connection the socket will take, all of them here to play.
        let full = seat_plan(crate::transport::MAX_PEERS, &[]);
        assert_eq!(
            full.iter().filter(|seat| seat.is_some()).count(),
            MAX_PLAYERS - 1,
            "the host holds the sixth chair"
        );
        assert!(
            full.iter()
                .flatten()
                .all(|&seat| usize::from(seat) < MAX_PLAYERS),
            "no peer is given a seat the sim has no slot for"
        );
        assert!(full.last().unwrap().is_none(), "the late ones watch");
    }

    /// Enter means two things to a host, say this and start the match, and
    /// it cannot mean both. `host_tick` runs before the input that owns
    /// the keyboard, so an Enter meant to send a line used to launch the
    /// round instead, which left the host unable to say anything at all.
    #[test]
    fn a_half_written_sentence_does_not_start_the_match() {
        // The everyday case: rivals aboard, nothing being typed, Enter goes.
        assert!(should_launch(1, false, true, None));
        assert!(
            !should_launch(1, false, false, None),
            "and nothing without it"
        );
        assert!(
            !should_launch(0, false, true, None),
            "nor with an empty table"
        );

        // Mid-sentence, Enter belongs to the sentence.
        assert!(!should_launch(1, true, true, None));
        assert!(!should_launch(3, true, true, None));

        // The unattended quota is exempt: nobody is sitting at that machine
        // to be typing, and it must not wait for an Enter that never comes.
        assert!(should_launch(2, false, false, Some(2)));
        assert!(should_launch(2, true, false, Some(2)));
        assert!(!should_launch(1, false, false, Some(2)), "not filled yet");
    }
}
