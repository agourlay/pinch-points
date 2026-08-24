//! The other end of the handshake: greeting a host until it answers, and
//! taking up the invitation when it does.
//!
//! A joiner talks to nobody but the host, which is why the table and every
//! line said at it have to be relayed rather than overheard.

use super::*;
use crate::app::i18n::fill;

/// What pressing a number, or Enter, or having just given a name, comes
/// to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    /// No beach was asked for, or the one asked for is not there.
    Nothing,
    /// Ask who is sitting here before seating them.
    AskName(usize),
    /// Take this one.
    Take(usize),
    /// It is full, this round and the next; say so rather than dialling.
    Full(usize),
}

/// What a frame knows about joining.
///
/// A struct rather than three bools in a row behind two `Option<usize>`s,
/// for the reason its sibling [`HostAsk`] is one: `which_beach(JoinAsk { digit: Some(1), enter_on: /// None, , ..asking() }, &hosts)` says nothing at the call site,
/// and any two of those could be swapped without the compiler noticing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct JoinAsk {
    /// A number key, as a row of the list.
    pub digit: Option<usize>,
    /// Enter, on the row under the cursor.
    pub enter_on: Option<usize>,
    /// What the player was doing when they were stopped and asked their
    /// name, if they were.
    pub intent: Option<Intent>,
    /// The unattended dev hook, which takes the first beach it hears and
    /// must never be stopped to answer a question nobody is there for.
    pub auto_join: bool,
    /// Already at a beach, one's own or somebody else's.
    pub busy: bool,
    /// There is a name on file, so there is nothing to ask.
    pub named: bool,
    /// The player armed W: they mean to watch, which needs no chair, so a
    /// full beach is still theirs to take.
    pub to_watch: bool,
}
/// Which beach a frame is asking for, and what has to happen before it can
/// be had.
///
/// Split from the socket work so the decision can be tested: which row a
/// key means, that a name is asked for first, that a beach already left
/// behind is not dialled again, and that a full one is refused rather than
/// queued for.
pub(super) fn which_beach(ask: JoinAsk, hosts: &[HostEntry]) -> Pick {
    let JoinAsk {
        digit,
        enter_on,
        intent,
        auto_join,
        busy,
        named,
        to_watch,
    } = ask;
    let asked = match (intent, auto_join) {
        // A name was just given on the way here; the beach it was given for
        // is the one to take, if it is still on the air.
        (Some(Intent::Join(at)), _) => Some(at),
        // Neither of these is a row of this list: hosting is not joining,
        // and a dialled address was typed because it is on no row.
        // Both are dealt with by the caller.
        (Some(Intent::Host | Intent::Dial(_)), _) => return Pick::Nothing,
        (None, true) => Some(0),
        (None, false) => digit.or(enter_on),
    };
    let Some(at) = asked.filter(|at| *at < hosts.len()) else {
        return Pick::Nothing;
    };
    if busy {
        return Pick::Nothing;
    }
    // A watcher takes no seat, so a full beach is no obstacle: the host
    // answers a `Watch` with a spectator's place however many are aboard.
    if !to_watch && !hosts[at].has_room() {
        return Pick::Full(at);
    }
    // The dev hooks join unattended; a prompt nobody is there to answer
    // would hang them.
    match named || auto_join || intent.is_some() {
        true => Pick::Take(at),
        false => Pick::AskName(at),
    }
}
/// Take a beach: by its number, by Enter on the cursor, or because a name
/// was just given on the way to one.
///
/// The last of `lobby_input`'s eight jobs to come out of it.
pub(super) fn take_a_beach(
    keys: &ButtonInput<KeyCode>,
    settings: &GameSettings,
    state: &mut LobbyState,
    tr: &'static crate::app::i18n::Tr,
    intent: Option<Intent>,
    auto_join: bool,
) {
    const DIGITS: [KeyCode; 9] = [
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
    // A dialled address skips the list entirely: there is no row to pick,
    // no beacon to have heard, and nothing about it to be full or in
    // progress, because none of that is known until the host answers.
    if let Some(Intent::Dial(addr)) = intent {
        if !state.standing().at_a_beach() {
            dial_at(state, settings, tr, addr);
        }
        return;
    }
    let digit = DIGITS.iter().position(|k| keys.just_pressed(*k));
    let enter_on = keys
        .just_pressed(KeyCode::Enter)
        .then(|| state.selected_index())
        .flatten();
    let pick = which_beach(
        JoinAsk {
            digit,
            enter_on,
            intent,
            auto_join,
            busy: state.standing().at_a_beach(),
            named: !settings.names[0].trim().is_empty(),
            to_watch: state.watching,
        },
        &state.hosts,
    );
    if auto_join && matches!(pick, Pick::Take(_)) {
        state.auto_done = true;
    }
    match pick {
        Pick::Nothing => {}
        // The table shows everyone by name, so who is sitting here is
        // asked before they are seated, every time. See `player_name`.
        Pick::AskName(at) => {
            state.typing = Some(Typing::player_name(Intent::Join(at), &settings.names[0]));
        }
        // A full beach is worth neither joining nor queueing for: there is
        // no chair for this player at the end of it either way.
        Pick::Full(_) => state.feedback = tr.lobby_beach_full.to_string(),
        Pick::Take(at) => dial(state, settings, tr, at),
    }
}
/// Open a socket to the beach on row `at` and greet it.
fn dial(
    state: &mut LobbyState,
    settings: &GameSettings,
    tr: &'static crate::app::i18n::Tr,
    at: usize,
) {
    debug_assert!(
        at < state.hosts.len(),
        "dialling row {at} of {}",
        state.hosts.len()
    );
    let Some(host) = state.hosts.get(at) else {
        return;
    };
    dial_at(state, settings, tr, host.addr);
}
/// How long a joiner keeps calling a beach that never answers.
///
/// The host answers a greeting the moment it hears one, and sends the
/// table round every [`ANNOUNCE_EVERY`] besides, so silence this long is
/// not a lost packet: there is nothing at that address. Longer than
/// [`HOST_TTL`], so a beach that is merely being slow about its beacon is
/// never given up on before the list would give up on it.
pub(super) const NO_ANSWER_AFTER: f32 = 6.0;
/// Whether anybody is actually there, said out loud.
///
/// A dialled socket is not a connection, since UDP tells nobody anything,
/// so until the host says a word this peer is *calling*, not aboard. Saying
/// "aboard" from the first frame is what made a mistyped address, a
/// firewall and a host who quit thirty seconds ago all look like a beach
/// that was simply slow to start: the screen said everything was fine and
/// nothing ever happened.
///
/// Returns whether the call was given up on, in which case the frame is
/// over: there is no socket left to read.
fn answer_the_silence(
    state: &mut LobbyState,
    tr: &'static crate::app::i18n::Tr,
    heard: bool,
    delta: f32,
) -> bool {
    if heard {
        // The first word is the moment this becomes a beach rather than an
        // address. Later ones only keep the patience topped up.
        if !state.host_answered {
            state.host_answered = true;
            state.feedback = match state.watching {
                true => tr.lobby_watching.to_string(),
                false => tr.lobby_aboard.to_string(),
            };
        }
        state.host_silence = 0.0;
        return false;
    }
    state.host_silence += delta;
    if state.host_silence < NO_ANSWER_AFTER {
        return false;
    }
    // Let the socket go rather than call forever. The address is still in
    // settings, so J offers it back pre-filled and a typo costs one
    // character rather than the whole line.
    let called = state
        .joining
        .as_ref()
        .and_then(|transport| transport.peer_addr())
        .map(|addr| addr.to_string())
        .unwrap_or_default();
    state.joining = None;
    state.table.clear();
    state.joined_terms = None;
    state.feedback = fill(tr.lobby_no_answer, &[("a", &called)]);
    true
}
/// Open a socket to a beach at a known address and greet it.
///
/// The bottom of both roads in: the row picked off the list, and the
/// address typed out by hand. A beach that was never heard from is joined
/// by the same greeting as one that was: the beacon is how a beach is
/// *found*, never how it is entered.
pub(super) fn dial_at(
    state: &mut LobbyState,
    settings: &GameSettings,
    tr: &'static crate::app::i18n::Tr,
    addr: SocketAddr,
) {
    match UdpTransport::join(addr) {
        Ok(transport) => {
            let watching = state.watching;
            transport.send(if watching {
                NetMsg::Watch
            } else {
                NetMsg::hello(&settings.names[0])
            });
            state.joining = Some(transport);
            state.hello_in = ANNOUNCE_EVERY;
            // Calling, not aboard: nothing has answered yet, and on UDP
            // an open socket is no evidence that anything will.
            state.host_answered = false;
            state.host_silence = 0.0;
            state.feedback = fill(tr.lobby_calling, &[("a", &addr.to_string())]);
        }
        Err(e) => state.feedback = fill(tr.lobby_could_not_join, &[("e", &e.to_string())]),
    }
}
/// What a host's `Start` invites this peer to: the size of the table, the
/// chair (or none, for a watcher), the terms, and who else is at it.
///
/// Named rather than left the four-tuple it was, because a tuple is read
/// by counting: `(seats, seat, ..)` is a count and an index next to each
/// other, and the only thing keeping them apart was their order.
pub(super) struct Invitation {
    pub seats: u8,
    pub seat: Option<u8>,
    pub terms: MatchTerms,
    pub names: [crate::transport::WireName; MAX_PLAYERS],
    /// Where the series stands, as the host says: round number and wins by
    /// seat. A peer that greeted mid-series is seated with the table's own
    /// tally rather than starting a fresh one. Round zero means no series.
    pub round: u8,
    pub wins: [u8; MAX_PLAYERS],
    /// The host's own beach, when it sent one. A joiner has never seen the
    /// file it came from, so it arrives with the invitation or not at all.
    pub beach: Vec<u8>,
}
/// Whether a `Start` off the wire invites this lobby to a new round,
/// rather than repeating the one it just walked out of.
///
/// A host still sitting on its results card re-answers every greeting
/// with the finished round's `Start`, which is right for a latecomer who
/// missed the launch and wrong for a table that came back here from that
/// very round. The seed is what tells them apart: a fresh round is struck
/// on a fresh seed, the same mark `OnlineSession::is_next_round` reads.
pub(super) fn a_fresh_invitation(played: Option<u64>, invitation: &Invitation) -> bool {
    played != Some(invitation.terms.seed)
}

/// Take up the host's invitation: build the session it describes, arm a
/// series if it is one, and walk into the arena.
pub(super) fn accept_the_invitation(
    state: &mut LobbyState,
    online: &mut Online,
    tournament: &mut crate::app::tournament::Tournament,
    next_screen: &mut NextState<Screen>,
    next_vphase: &mut NextState<VersusPhase>,
    started: Option<Invitation>,
) {
    if let Some(Invitation {
        seats,
        seat,
        terms,
        names,
        round,
        wins,
        beach,
    }) = started
    {
        debug_assert!(
            (2..=MAX_PLAYERS as u8).contains(&seats),
            "invited to a {seats}-seat beach, which decode should have refused"
        );
        debug_assert!(
            seat.is_none_or(|seat| seat < seats),
            "seated at {seat:?} of {seats}"
        );
        let transport = state.joining.take().expect("checked above");
        let humans = seats.saturating_sub(terms.bots).max(1);
        let players: Vec<u8> = (0..humans).collect();
        let session = match seat {
            None => Lockstep::observer(players, DEFAULT_DELAY),
            Some(seat) => Lockstep::new(seat, players, DEFAULT_DELAY),
        };
        let mut session = OnlineSession::new(transport, session, seats, terms);
        // The host's own beach, if it sent one: nothing else on this
        // machine can build it.
        session.beach = beach;
        session.names = std::array::from_fn(|i| crate::transport::name_from_wire(&names[i]));
        // Formed here, so a finished match knows it has a lobby to walk
        // this table back to.
        session.from_lobby = true;
        online.0 = Some(session);
        // The host's invitation says whether this is a series, and where
        // it stands: a joiner that assumed otherwise would stop after one
        // round, and one admitted mid-series would start its own tally at
        // zero and disagree with the table for the rest of the match.
        *tournament = crate::app::tournament::Tournament::from_terms(terms, round, wins);
        next_vphase.set(VersusPhase::Running);
        next_screen.set(Screen::Versus);
    }
}
/// Joining: keep greeting the host until our seat assignment arrives
/// (Start rides UDP; hellos are re-answered until one lands).
pub fn join_tick(
    time: Res<Time>,
    settings: Res<GameSettings>,
    mut state: ResMut<LobbyState>,
    mut online: ResMut<Online>,
    mut tournament: ResMut<crate::app::tournament::Tournament>,
    mut next_screen: ResMut<NextState<Screen>>,
    mut next_vphase: ResMut<NextState<VersusPhase>>,
) {
    if state.joining.is_none() {
        return;
    }
    state.hello_in -= time.delta_secs();
    let say_hello = state.hello_in <= 0.0;
    if say_hello {
        state.hello_in = ANNOUNCE_EVERY;
    }
    let mut started = None;
    let mut mismatch = None;
    let mut queued = None;
    let mut said: Vec<(crate::transport::WireName, crate::transport::WireChat)> = Vec::new();
    let mut table: Option<Vec<String>> = None;
    let mut host_terms: Option<MatchTerms> = None;
    let mut heard = false;
    let watching = state.watching;
    if let Some(transport) = &mut state.joining {
        if say_hello {
            transport.send(if watching {
                NetMsg::Watch
            } else {
                NetMsg::hello(&settings.names[0])
            });
        }
        for (msg, _) in transport.recv_all() {
            // Anything at all, even a message this screen throws away, is
            // proof somebody is there, which is the one thing a joiner
            // cannot otherwise tell.
            heard = true;
            match msg {
                NetMsg::Start {
                    seats,
                    seat,
                    terms,
                    names,
                    round,
                    wins,
                    beach,
                } => {
                    started = Some(Invitation {
                        seats,
                        seat,
                        terms,
                        names,
                        round,
                        wins,
                        beach,
                    })
                }
                NetMsg::Incompatible { version } => mismatch = Some(version),
                // That beach is mid-round. The greeting already repeats
                // every second, so it doubles as the place in line: stay
                // here, keep saying hello, and the host answers with a
                // `Start` when the next round comes round.
                NetMsg::Queued { ahead } => queued = Some(ahead),
                NetMsg::Chat { name, text } => said.push((name, text)),
                // Who else is here. A joiner has spoken to nobody but the
                // host and would otherwise sit at an apparently empty beach.
                NetMsg::Roster { names, terms, .. } => {
                    table = Some(
                        names
                            .iter()
                            .map(crate::transport::name_from_wire)
                            .take_while(|name| !name.is_empty())
                            .collect(),
                    );
                    host_terms = Some(terms);
                }
                NetMsg::Hello { .. }
                | NetMsg::Watch
                | NetMsg::Input(_)
                | NetMsg::Hash { .. }
                | NetMsg::Pause { .. }
                | NetMsg::Abandoned { .. }
                | NetMsg::Resume { .. } => {}
            }
        }
    }
    if answer_the_silence(&mut state, settings.tr(), heard, time.delta_secs()) {
        return;
    }
    // That beach is running a different build. Say so and let go of the
    // socket: greeting it again would only be ignored again, and a joiner
    // left saying hello forever looks exactly like a network fault.
    if let Some(version) = mismatch {
        state.joining = None;
        state.feedback = fill(
            settings.tr().lobby_version_clash,
            &[
                ("t", &version.to_string()),
                ("o", &crate::transport::PROTOCOL_VERSION.to_string()),
            ],
        );
        return;
    }
    if let Some(table) = table {
        state.table = table;
    }
    if let Some(terms) = host_terms {
        state.joined_terms = Some(terms);
    }
    for (name, text) in said {
        let who = crate::transport::name_from_wire(&name);
        let line = crate::transport::chat_from_wire(&text);
        state.say(&who, &line);
    }
    if let Some(ahead) = queued {
        let tr = settings.tr();
        state.feedback = match ahead {
            0 => tr.lobby_queued_next.to_string(),
            n => fill(tr.lobby_queued_behind, &[("n", &n.to_string())]),
        };
    }
    let started = started.filter(|invitation| a_fresh_invitation(state.played_seed, invitation));
    accept_the_invitation(
        &mut state,
        &mut online,
        &mut tournament,
        &mut next_screen,
        &mut next_vphase,
        started,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame in which somebody with a name on file is choosing a beach
    /// and nothing else is going on. Every field named once here, so a new
    /// one cannot be silently defaulted into the tests below, which then
    /// say only what they change.
    fn asking() -> JoinAsk {
        JoinAsk {
            digit: None,
            enter_on: None,
            intent: None,
            auto_join: false,
            busy: false,
            named: true,
            to_watch: false,
        }
    }

    fn beaches(spec: &[(u8, u8)]) -> Vec<HostEntry> {
        spec.iter()
            .enumerate()
            .map(|(i, (taken, seats))| HostEntry {
                addr: format!("10.0.0.{}:47777", i + 1).parse().expect("addr"),
                id: i as u64 + 1,
                name: format!("beach{i}"),
                host: "Sam".to_string(),
                taken: *taken,
                seats: *seats,
                running: false,
                age: 0.0,
            })
            .collect()
    }

    /// Three beaches, room at all of them, and a player who has a name.
    fn open() -> Vec<HostEntry> {
        beaches(&[(1, 6), (1, 6), (1, 6)])
    }

    /// A beach dialled by hand is greeted exactly as one picked off the
    /// list, because it is the same greeting down the same road: the
    /// beacon is how a beach is found, never how it is entered. Over a
    /// real socket, since the point of the whole feature is that no
    /// broadcast was involved in getting here.
    #[test]
    fn a_dialled_beach_hears_the_same_greeting_as_a_listed_one() {
        let mut host = UdpTransport::host(0).expect("bind");
        let port = host.local_addr().expect("addr").port();
        let mut state = LobbyState::default();
        let mut settings = GameSettings::default();
        settings.names[0] = "Cy".into();
        let there: SocketAddr = format!("127.0.0.1:{port}").parse().expect("addr");
        dial_at(&mut state, &settings, &crate::app::i18n::EN, there);
        assert!(state.joining.is_some(), "the socket is open and greeting");
        let mut greeted = None;
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            for (msg, _) in host.recv_all() {
                if let NetMsg::Hello { name, .. } = msg {
                    greeted = Some(crate::transport::name_from_wire(&name));
                }
            }
            if greeted.is_some() {
                break;
            }
        }
        assert_eq!(greeted.as_deref(), Some("Cy"), "the host heard who called");
    }

    /// Which row a key means. A digit picks by position and Enter takes
    /// whatever the cursor is on, and neither may reach past the list:
    /// pressing 7 with three beaches on the air is not a beach.
    #[test]
    fn a_key_picks_the_row_it_names_and_no_further() {
        let hosts = open();
        assert_eq!(
            which_beach(
                JoinAsk {
                    digit: Some(1),
                    ..asking()
                },
                &hosts
            ),
            Pick::Take(1)
        );
        assert_eq!(
            which_beach(
                JoinAsk {
                    digit: Some(6),
                    ..asking()
                },
                &hosts
            ),
            Pick::Nothing,
            "the seventh of three"
        );
        assert_eq!(
            which_beach(
                JoinAsk {
                    enter_on: Some(2),
                    ..asking()
                },
                &hosts
            ),
            Pick::Take(2),
            "Enter takes the cursor's beach"
        );
        assert_eq!(
            which_beach(asking(), &hosts),
            Pick::Nothing,
            "and nothing pressed takes nothing"
        );
        assert_eq!(
            which_beach(
                JoinAsk {
                    digit: Some(0),
                    ..asking()
                },
                &[]
            ),
            Pick::Nothing,
            "nor does a key with an empty hall"
        );
    }

    /// Nobody is seated before they have said who they are, and the beach
    /// they were stopped on the way to is the one they get afterwards.
    #[test]
    fn a_nameless_player_is_asked_first_and_lands_where_they_meant() {
        let hosts = open();
        assert_eq!(
            which_beach(
                JoinAsk {
                    digit: Some(2),
                    named: false,
                    ..asking()
                },
                &hosts
            ),
            Pick::AskName(2),
            "asked, and asked about the right beach"
        );
        // Answering comes back as an intent naming that same beach.
        assert_eq!(
            which_beach(
                JoinAsk {
                    intent: Some(Intent::Join(2)),
                    ..asking()
                },
                &hosts
            ),
            Pick::Take(2)
        );
        // A beach that went away while the name was being typed is not
        // dialled, and nothing else is dialled in its place.
        assert_eq!(
            which_beach(
                JoinAsk {
                    intent: Some(Intent::Join(2)),
                    ..asking()
                },
                &hosts[..1]
            ),
            Pick::Nothing
        );
    }

    /// A full beach is refused outright: there is no chair at the end of
    /// it, this round or the next, so queueing would be a lie.
    #[test]
    fn a_full_beach_is_refused_rather_than_queued_for() {
        let hosts = beaches(&[(6, 6), (5, 6), (0, 0)]);
        assert_eq!(
            which_beach(
                JoinAsk {
                    digit: Some(0),
                    ..asking()
                },
                &hosts
            ),
            Pick::Full(0)
        );
        assert_eq!(
            which_beach(
                JoinAsk {
                    digit: Some(1),
                    ..asking()
                },
                &hosts
            ),
            Pick::Take(1),
            "one chair is still a chair"
        );
        assert_eq!(
            which_beach(
                JoinAsk {
                    digit: Some(2),
                    ..asking()
                },
                &hosts
            ),
            Pick::Take(2),
            "a beach that described no table is not a full one"
        );
    }

    /// Already somewhere, hosting or joining, and the keys mean nothing.
    /// Without this a digit would open a second socket over the first.
    #[test]
    fn keys_do_nothing_once_you_are_already_at_a_beach() {
        let hosts = open();
        for pressed in [Some(0), Some(1)] {
            assert_eq!(
                which_beach(
                    JoinAsk {
                        digit: pressed,
                        busy: true,
                        ..asking()
                    },
                    &hosts
                ),
                Pick::Nothing
            );
        }
        assert_eq!(
            which_beach(
                JoinAsk {
                    intent: Some(Intent::Join(0)),
                    busy: true,
                    ..asking()
                },
                &hosts
            ),
            Pick::Nothing
        );
    }

    /// Hosting is somebody else's intent entirely and must not be mistaken
    /// for a join, or pressing H would dial the beach under the cursor.
    #[test]
    fn the_intent_to_host_is_not_the_intent_to_join() {
        let hosts = open();
        assert_eq!(
            which_beach(
                JoinAsk {
                    enter_on: Some(1),
                    intent: Some(Intent::Host),
                    ..asking()
                },
                &hosts
            ),
            Pick::Nothing
        );
    }

    /// A beach that never answers is given up on and said so, rather than
    /// called forever under a screen that claims you are aboard.
    ///
    /// This is what a mistyped address looks like, and a firewall, and a
    /// friend who quit thirty seconds ago, and on UDP all three look just
    /// like a host who has not started the round yet. Silence is
    /// the only evidence there is, so it has to be worth something.
    #[test]
    fn a_beach_that_never_answers_is_given_up_on() {
        // Nothing is listening at this address; that is the point.
        let mut state = LobbyState {
            joining: Some(UdpTransport::join(("127.0.0.1", 47999)).expect("join")),
            ..LobbyState::default()
        };

        let quiet = answer_the_silence(&mut state, &crate::app::i18n::EN, false, 1.0);
        assert!(!quiet, "one second is a lost packet, not an empty address");
        assert!(state.joining.is_some(), "still calling");

        let quiet = answer_the_silence(&mut state, &crate::app::i18n::EN, false, NO_ANSWER_AFTER);
        assert!(quiet, "and then it stops calling");
        assert!(state.joining.is_none(), "the socket is let go");
        assert!(
            state.feedback.contains("127.0.0.1:47999"),
            "and says which address went unanswered: {:?}",
            state.feedback
        );
        assert!(
            !state.standing().at_a_beach(),
            "and the lobby is back to browsing"
        );
    }

    /// The first word back is the moment a called address becomes a beach.
    #[test]
    fn the_first_word_is_what_makes_it_a_beach() {
        let mut state = LobbyState {
            joining: Some(UdpTransport::join(("127.0.0.1", 47999)).expect("join")),
            feedback: "calling...".into(),
            ..LobbyState::default()
        };

        answer_the_silence(&mut state, &crate::app::i18n::EN, true, 0.1);
        assert_eq!(state.feedback, crate::app::i18n::EN.lobby_aboard);
        assert!(state.host_answered);

        // And the patience is topped up by every word after it, so a long
        // wait in a lobby is never mistaken for an empty address.
        answer_the_silence(
            &mut state,
            &crate::app::i18n::EN,
            false,
            NO_ANSWER_AFTER - 0.1,
        );
        answer_the_silence(&mut state, &crate::app::i18n::EN, true, 0.1);
        let quiet = answer_the_silence(&mut state, &crate::app::i18n::EN, false, 1.0);
        assert!(!quiet, "the clock restarted when the host spoke");
        assert!(state.joining.is_some());
    }

    /// Nor is a dialled address. It names no row of this list, which is
    /// why it was typed at all, and reading it as one would take whatever
    /// happened to be under the cursor instead: the beach the player could
    /// already see and did not ask for.
    #[test]
    fn a_dialled_address_is_not_a_row_of_the_list() {
        let hosts = open();
        let there: SocketAddr = "192.168.1.5:47777".parse().expect("addr");
        assert_eq!(
            which_beach(
                JoinAsk {
                    enter_on: Some(1),
                    intent: Some(Intent::Dial(there)),
                    ..asking()
                },
                &hosts
            ),
            Pick::Nothing
        );
        // And with nothing on the air at all, which is the state the
        // whole feature exists for.
        assert_eq!(
            which_beach(
                JoinAsk {
                    intent: Some(Intent::Dial(there)),
                    ..asking()
                },
                &[]
            ),
            Pick::Nothing
        );
    }

    /// The round a table has just walked out of is not an invitation back
    /// into it.
    ///
    /// A host sitting on its results card re-answers every greeting with
    /// that round's `Start`, which is right for a latecomer and wrong for
    /// the peers that were in it: they greet the moment they are back in
    /// the lobby, hear the repeat, and would bounce straight back into
    /// the finished round. The seed is what tells a fresh round from a
    /// repeat, the same mark the session reads mid-match.
    #[test]
    fn the_round_just_played_is_not_an_invitation_back_into_it() {
        let invitation = |seed| Invitation {
            seats: 2,
            seat: Some(1),
            terms: MatchTerms {
                seed,
                ..MatchTerms::default()
            },
            names: [[0u8; crate::transport::WIRE_NAME]; MAX_PLAYERS],
            round: 0,
            wins: [0; MAX_PLAYERS],
            beach: Vec::new(),
        };
        assert!(
            !a_fresh_invitation(Some(42), &invitation(42)),
            "the host is repeating the round we just left"
        );
        assert!(
            a_fresh_invitation(Some(42), &invitation(43)),
            "a fresh seed is a fresh round, and this table is in it"
        );
        // A lobby that has played nothing takes any invitation, which is
        // every ordinary join.
        assert!(a_fresh_invitation(None, &invitation(42)));
    }

    /// The dev hook joins the first beach it hears, unattended, so it is
    /// never stopped for a name nobody is there to type.
    #[test]
    fn the_unattended_hook_takes_the_first_beach_without_being_asked() {
        let hosts = open();
        assert_eq!(
            which_beach(
                JoinAsk {
                    auto_join: true,
                    named: false,
                    ..asking()
                },
                &hosts
            ),
            Pick::Take(0),
            "nameless, and taken anyway"
        );
        assert_eq!(
            which_beach(
                JoinAsk {
                    auto_join: true,
                    named: false,
                    ..asking()
                },
                &[]
            ),
            Pick::Nothing,
            "but it cannot take what is not there"
        );
    }
}
