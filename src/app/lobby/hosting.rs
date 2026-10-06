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
/// A struct rather than four bools in a row: `host_step(false, true,
/// false, false)` says nothing at the call site.
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
/// nobody is automating, so the tests say only what they change. Beside
/// the struct rather than in a test module, because two test modules want
/// it.
#[cfg(test)]
pub(super) fn idle() -> HostAsk {
    HostAsk {
        answered: false,
        pressed_h: false,
        busy: false,
        auto: false,
    }
}

/// Peers as they stand on a socket: named, with the given ones watching.
#[cfg(test)]
pub(super) fn peers_named(names: &[&str], watchers: &[usize]) -> PeerBook {
    let mut peers = PeerBook::default();
    for (i, name) in names.iter().enumerate() {
        peers.row(i).name = name.to_string();
    }
    for watcher in watchers {
        peers.row(*watcher).watch = true;
    }
    peers
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

/// The way from the lobby into the arena: the session the match is
/// played over, the series it belongs to, and the screen and phase that
/// open on it.
#[derive(bevy::ecs::system::SystemParam)]
pub struct IntoArena<'w> {
    online: ResMut<'w, Online>,
    tournament: ResMut<'w, crate::app::tournament::Tournament>,
    next_screen: ResMut<'w, NextState<Screen>>,
    next_vphase: ResMut<'w, NextState<VersusPhase>>,
    /// The doorway bots come straight to this beach by (route 2).
    bots: ResMut<'w, crate::app::bot_seats::BotSeats>,
}

impl IntoArena<'_> {
    /// Walk the table in: the session and the series take over, and the
    /// round opens on its count.
    fn walk_in(&mut self, session: OnlineSession, series: crate::app::tournament::Tournament) {
        self.online.0 = Some(session);
        *self.tournament = series;
        self.next_vphase.set(VersusPhase::Countdown);
        self.next_screen.set(Screen::Versus);
    }
}

/// Hosting: announce on a timer, gather joiners (up to five rivals, plus
/// onlookers), and launch on Enter (or once the auto-host quota fills).
pub fn host_tick(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<GameSettings>,
    config: Res<MatchConfig>,
    beaches: Res<crate::app::match_setup::CustomBeaches>,
    mut state: ResMut<LobbyState>,
    mut arena: IntoArena,
) {
    let tr = settings.tr();
    let game_name = state.game_name.clone();
    let Some(hosted) = state.hosted_mut() else {
        return;
    };
    // The occupancy is a frame stale by the time the beacon goes out,
    // which costs nothing at one beacon a second. The host holds a seat
    // too, so the table is never empty; seats are the hard limit rather
    // than the configured count, since an AI seat gives way to a player
    // who turns up for it.
    // Bots straight to this beach (route 2) come in by a doorway on the
    // LAN while the host says they are welcome, one single-use key at a
    // time, and are let go the moment it says they are not.
    let arrived = host_the_bots(&mut arena.bots, &config, tr, hosted.players_aboard());
    let mut taken = 1 + hosted.players_aboard() as u8 + arrived.len() as u8;
    // The socket has room for `MAX_PEERS` and drops every sender past
    // that without a word, watchers counted. A beach whose socket is full
    // says it is full, or the next arrival is listed a chair, dials, and
    // hears nothing until "no answer - check the address".
    if hosted.transport.peer_count() >= crate::transport::MAX_PEERS {
        taken = MAX_PLAYERS as u8;
    }
    let on_air = crate::transport::OnAir {
        name: &game_name,
        host: &settings.names[0],
        taken,
        seats: MAX_PLAYERS as u8,
        bots: config.bots_welcome,
    };
    let picked = work_the_socket(hosted, time.delta_secs(), on_air);
    let do_announce = picked.announced;
    for (_, name, text) in picked.said {
        state.hear(&name, &text);
    }
    note_arrivals_and_departures(&mut state, tr, picked.greeted);
    // Counted after the departures, so a table that just lost somebody is
    // not announced as still holding them.
    let joined = state.hosted().map_or(0, |hosted| hosted.peers.len());
    // The host's own view of the table, and the peers': the same list,
    // sent on every beacon tick so a lost one costs a second, not a screen
    // that stays wrong until somebody joins or leaves.
    state.table = state.roster(tr, &settings.names[0]);
    state.table_kinds = state.roster_kinds();
    // The bots that came straight here sit after the people.
    for (_, name) in &arrived {
        if state.table.len() < MAX_PLAYERS {
            state.table.push(name.clone());
            state.table_kinds.push(crate::sim::SeatKind::Bot);
        }
    }
    state.bot_string = arena
        .bots
        .door
        .as_ref()
        .and_then(crate::app::bot_seats::Doorway::open_string);
    if do_announce && let Some(hosted) = state.hosted() {
        let names = crate::transport::wire_table(&state.table);
        // The dials travel with the roster so a joiner's terms card shows
        // the match it is joining. The seed is not one of them yet - it is
        // struck fresh at launch - so a placeholder rides along and the
        // card ignores it.
        let terms = crate::app::match_setup::terms(&config, settings.team_mode, 0);
        hosted.transport.send(NetMsg::Roster {
            seats: state.table.len().min(MAX_PLAYERS) as u8,
            names,
            terms,
            kinds: crate::transport::wire_kinds(&state.table_kinds),
        });
    }
    let (players_aboard, watchers_aboard, port) = state.hosted().map_or((0, 0, None), |hosted| {
        (
            hosted.players_aboard(),
            hosted.watchers_aboard(),
            hosted.transport.local_addr().ok().map(|addr| addr.port()),
        )
    });
    // A bot that came straight here is a rival aboard like any other.
    let rivals = joined + arrived.len();
    if rivals > 0 {
        let aboard = players_aboard + arrived.len();
        state.feedback = gathering_feedback(tr, &config, aboard, watchers_aboard);
    } else {
        // The last peer timed out: without this the line kept saying "1
        // rival aboard" over an empty table until somebody else turned up,
        // and Enter did nothing. Back to the waiting message it started on.
        state.feedback = match port {
            Some(port) => fill(tr.lobby_hosting, &[("p", &port.to_string())]),
            None => tr.lobby_hosting_noport.to_string(),
        };
    }
    let quota = crate::app::dev::auto_host_quota();
    let launch = should_launch(
        rivals,
        // Picking whom to ask to leave holds the keyboard as typing does:
        // the host runs before the lobby's keys, so an Enter pressed at the
        // "pick a number" prompt used to start the match.
        state.typing.is_some() || state.kicking,
        crate::app::menu_ui::enter(&keys),
        quota,
    );
    if launch {
        launch_the_match(&mut state, &settings, &config, &beaches, joined, &mut arena);
    }
}

/// Keep the doorway for bots straight to this beach (route 2) as the host
/// wants it, and say who has come in by it.
fn host_the_bots(
    bots: &mut crate::app::bot_seats::BotSeats,
    config: &MatchConfig,
    tr: &'static crate::app::i18n::Tr,
    people: usize,
) -> Vec<(crate::bots::listener::BotId, String)> {
    if !config.bots_welcome {
        bots.door = None;
        return Vec::new();
    }
    if bots.door.is_none() {
        match crate::app::bot_seats::Doorway::open_lan() {
            Ok(door) => bots.door = Some(door),
            Err(e) => {
                bots.feedback = fill(tr.door_could_not_listen, &[("e", &e.to_string())]);
                return Vec::new();
            }
        }
    }
    let Some(door) = &mut bots.door else {
        return Vec::new();
    };
    door.poll();
    let arrived = door.arrived(tr);
    // The host's own chair and each person's come first.
    let room = 1 + people + arrived.len() < MAX_PLAYERS;
    door.keep_a_chair_open(room);
    arrived
}

/// Put the match on: seat everyone who came to play, agree the terms, and
/// walk the whole table into the arena.
///
/// Lifted out of `host_tick`, which had grown to nine jobs: this is the
/// one of them with a beginning and an end.
fn launch_the_match(
    state: &mut LobbyState,
    settings: &GameSettings,
    config: &MatchConfig,
    beaches: &crate::app::match_setup::CustomBeaches,
    joined: usize,
    arena: &mut IntoArena,
) {
    // The beacon does not stop at launch, it changes what it says: a
    // running beach cannot be joined, since lockstep has nothing to
    // catch a latecomer up with, but it can be queued for. The
    // announcer rides along into the session to keep saying so, and the
    // peers ride along to be dealt their chairs.
    let Standing::Hosting(Hosted {
        announcer,
        transport,
        peers,
        announce_in: _,
    }) = std::mem::take(&mut state.standing)
    else {
        unreachable!("launching a match from a lobby that is not hosting");
    };
    debug_assert_eq!(joined, peers.len());
    let plan = seat_plan(&peers);
    let people = 1 + plan.iter().filter(|seat| seat.is_some()).count() as u8;
    // The bots that came straight here (route 2) sit after the people, as
    // many as there are chairs for; the host speaks for each of them.
    let bots_here: Vec<(u8, crate::bots::listener::BotId, String)> = arena
        .bots
        .door
        .as_ref()
        .map(|door| door.arrived(settings.tr()))
        .unwrap_or_default()
        .into_iter()
        .take(MAX_PLAYERS - usize::from(people))
        .enumerate()
        .map(|(i, (id, name))| (people + i as u8, id, name))
        .collect();
    let humans = people + bots_here.len() as u8;
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
            && let Some(peer) = peers.get(peer)
        {
            names[usize::from(*seat)].clone_from(&peer.name);
        }
    }
    for (seat, _, name) in &bots_here {
        names[usize::from(*seat)].clone_from(name);
    }
    let wire_names = crate::transport::wire_table(&names);
    // What holds each seat: the host's own a person, a peer's whatever its
    // greeting said, the host's bots bots, and the top seats the AI's.
    let mut kinds = [crate::sim::SeatKind::Human; MAX_PLAYERS];
    for (seat, ..) in &bots_here {
        kinds[usize::from(*seat)] = crate::sim::SeatKind::Bot;
    }
    for (peer, slot) in plan.iter().enumerate() {
        if let Some(seat) = slot
            && peers.get(peer).is_some_and(|peer| peer.bot)
        {
            kinds[usize::from(*seat)] = crate::sim::SeatKind::Bot;
        }
    }
    for seat in humans..seats {
        kinds[usize::from(seat)] = crate::sim::SeatKind::Ai;
    }
    let wire_kinds = crate::transport::wire_kinds(&kinds);
    // Seats go to the peers that came to play, in peer order; the
    // onlookers - and anyone the table ran out of chairs for - are told
    // they are watching.
    // A series begins at round one with an empty tally; a single round
    // carries no standing at all.
    let standing = terms
        .is_series()
        .then(crate::transport::SeriesStanding::opening);
    for (peer, slot) in plan.iter().enumerate() {
        transport.send_to(
            peer,
            NetMsg::Start {
                seats,
                seat: *slot,
                terms,
                names: wire_names,
                kinds: wire_kinds,
                standing,
                beach: beach.clone(),
            },
        );
    }
    let mut session = OnlineSession::new(
        transport,
        crate::app::net::lockstep_for(Some(0), humans),
        seats,
        terms,
    );
    // The host plays on the same bytes it sent, not on its own copy of the
    // file: if the two ever disagreed, the hash check would find it and
    // nobody could say which of them was right.
    session.beach = beach;
    session.peers = peers;
    session.peers.deal(&plan);
    session.names = names;
    session.kinds = kinds;
    let spoken: Vec<u8> = bots_here.iter().map(|(seat, ..)| *seat).collect();
    session.session.speak_for(&spoken);
    session.bots_here = bots_here;
    session.stay_on_air(announcer);
    session.home.from_lobby = true;
    session.home.game_name = state.game_name.clone();
    session.home.bots_welcome = config.bots_welcome;
    // The series is part of the terms, so every peer knows it is one
    // and tallies the same rounds. Without that the host alone would
    // count, and only the host would see a champion.
    let series = crate::app::tournament::Tournament::from_terms(terms, standing);
    arena.walk_in(session, series);
}

/// Fold this tick's greetings and silences into the table: who has just
/// arrived, and who has stopped answering.
///
/// Both are news for the feed and for every peer, and both are the same
/// kind of bookkeeping, so they sit together.
fn note_arrivals_and_departures(
    state: &mut LobbyState,
    tr: &'static crate::app::i18n::Tr,
    greeted: Vec<(usize, String)>,
) {
    for (from, told) in greeted {
        let Some(hosted) = state.hosted_mut() else {
            return;
        };
        // A greeting arrives every second; only the first one, or a change
        // of mind about the name, is news worth putting in the feed.
        let row = hosted.peers.row(from);
        let arrived = row.name != told;
        // The first greeting that names them, as against a later one that
        // changes the name: only the first has a room to be shown.
        let newcomer = row.name.is_empty();
        // Said of whoever turned up, in their own terms: the table counts
        // its rivals, so announcing a spectator as one more of them would
        // have every player looking for a chair that was never taken.
        let watching = row.watch;
        row.name.clone_from(&told);
        if newcomer {
            catch_up_on_the_feed(state, from);
        }
        if arrived {
            let line = match watching {
                true => tr.lobby_watching_joined,
                false => tr.lobby_joined,
            };
            let notice = fill(line, &[("p", &told)]);
            state.say("", &notice);
            announce_to_table(state, "", &notice);
        }
    }
    // Anyone unheard for too long has walked away: UDP will not say so, and
    // a table that keeps counting them never has room again.
    let gone: Vec<usize> = state.hosted().map_or(Vec::new(), |hosted| {
        (0..hosted.peers.len())
            .rev()
            .filter(|peer| {
                hosted
                    .peers
                    .get(*peer)
                    .is_some_and(|p| p.silence > PEER_TTL)
            })
            .collect()
    });
    for peer in gone {
        let Some(hosted) = state.hosted_mut() else {
            return;
        };
        let who = hosted
            .peers
            .get(peer)
            .map_or_else(String::new, |p| p.name.clone());
        hosted.forget_peer(peer);
        if !who.is_empty() {
            let notice = fill(tr.lobby_left, &[("p", &who)]);
            state.say("", &notice);
            announce_to_table(state, "", &notice);
        }
    }
}

/// What one tick on the socket picked up: whether the beacon went out, who
/// named themselves, and what was said. Bundled because they travel
/// together.
#[derive(Default)]
struct Picked {
    /// The once-a-second tick fell this frame, so the roster goes out too.
    announced: bool,
    greeted: Vec<(usize, String)>,
    /// Peers that greeted as bots at a beach that does not welcome them,
    /// turned away once the batch is read.
    unwelcome: Vec<usize>,
    said: Vec<(
        usize,
        crate::transport::WireName,
        crate::transport::WireChat,
    )>,
}

/// One tick on the host's own socket: age the table, put the beacon up,
/// take in what the peers sent, and pass the chat along to the rest of
/// the table.
///
/// What it learns goes back to the caller to be folded into the lobby,
/// which needs the rest of the state this borrow of the beach is holding.
fn work_the_socket(hosted: &mut Hosted, delta: f32, on_air: crate::transport::OnAir<'_>) -> Picked {
    let Hosted {
        announcer,
        transport,
        peers,
        announce_in,
    } = hosted;
    let mut picked = Picked {
        announced: once_a_second(announce_in, delta),
        ..Picked::default()
    };
    peers.age(delta);
    if picked.announced
        && let Ok(addr) = transport.local_addr()
    {
        announcer.announce(addr.port(), on_air);
    }
    // recv_all registers joiners from their greetings; a Watch tells us
    // that peer is here to look, not to play, and a Hello says what to
    // call whoever sent it.
    for (msg, from) in transport.recv_all() {
        // A peer the socket has only just registered has no row yet, and
        // anything at all from a peer is proof it is still there.
        peers.reach(transport.peer_count());
        peers.heard(from);
        match msg {
            // A spectator greets with its name too, so the room can be
            // told who turned up to watch and the host has something to
            // check a spectator's chat line against.
            NetMsg::Watch { name } => {
                peers.row(from).watch = true;
                let told = crate::transport::name_from_wire(&name);
                if !told.is_empty() {
                    picked.greeted.push((from, told));
                }
            }
            // Round things, and the lobby has no round to call one in.
            NetMsg::SpectatorVote { .. } | NetMsg::SpectatorTally { .. } => {}
            // A bot at a beach that has said bots are not welcome is
            // asked to go, once the socket is drained: forgetting it now
            // would shift the indices the rest of this batch carries.
            NetMsg::Hello { bot: true, .. } if !on_air.bots => {
                if !picked.unwelcome.contains(&from) {
                    picked.unwelcome.push(from);
                }
            }
            NetMsg::Hello { name, bot } => {
                peers.row(from).bot = bot;
                let told = crate::transport::name_from_wire(&name);
                if !told.is_empty() {
                    picked.greeted.push((from, told));
                }
            }
            // The spokes of the star cannot hear each other, so the
            // hub repeats what it is told before showing it - under the
            // name it knows the sender by, not the one the datagram
            // claims. Chat is the only message that names its own sender,
            // and the host is the only peer that can check the claim,
            // since it alone knows which socket the line came from.
            //
            // Unchecked, a peer could speak under a rival's name, or under
            // the empty one, which is worse: an empty name is the room
            // itself talking (`Said::is_notice`). So an empty name is
            // refused whoever sends it.
            //
            // The claim stands only where there is nothing to check it
            // against, which is now a peer whose greeting has not been
            // written down yet: a tick at most, and the same tick for a
            // watcher as for a player, since both greet by name.
            NetMsg::Chat { name, text } => {
                let known = peers.get(from).map_or("", |peer| peer.name.as_str());
                let who = match known.is_empty() {
                    false => crate::transport::wire_name(known),
                    true => name,
                };
                if !crate::transport::name_from_wire(&who).is_empty() {
                    picked.said.push((from, who, text));
                }
            }
            NetMsg::Inputs(_)
            | NetMsg::Hash { .. }
            | NetMsg::Start { .. }
            | NetMsg::Pause { .. }
            | NetMsg::Resume { .. }
            | NetMsg::Queued { .. }
            | NetMsg::Roster { .. }
            | NetMsg::Abandoned { .. }
            | NetMsg::CatchUp { .. }
            | NetMsg::SpectatorPick { .. }
            | NetMsg::CrowdPicks { .. }
            | NetMsg::Incompatible { .. }
            | NetMsg::Kicked => {}
        }
    }
    // Highest first, so each index still names the peer it was read from.
    picked.unwelcome.sort_unstable();
    for &peer in picked.unwelcome.iter().rev() {
        transport.turn_away(peer);
        peers.forget(peer);
    }
    // And everyone after a forgotten peer moved up one place per peer.
    let unwelcome = &picked.unwelcome;
    let moved = |from: usize| from - unwelcome.iter().filter(|&&gone| gone < from).count();
    picked.greeted.retain(|(from, _)| !unwelcome.contains(from));
    picked.said.retain(|(from, ..)| !unwelcome.contains(from));
    for (from, _) in &mut picked.greeted {
        *from = moved(*from);
    }
    for (from, ..) in &mut picked.said {
        *from = moved(*from);
    }
    for (from, name, text) in picked.said.iter() {
        crate::app::net::relay(
            transport,
            *from,
            NetMsg::Chat {
                name: *name,
                text: *text,
            },
        );
    }
    picked
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
/// host is the one player who both talks and starts, and `host_tick` runs
/// ahead of the input that owns the keyboard, so Enter launched the match
/// out from under a half-written sentence.
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

/// Show a newcomer what the room has already said.
///
/// The host relays a line at the moment it is said and never again, so
/// somebody who turns up later walks into a beach that looks as though
/// nobody has ever spoken in it: the arrivals before theirs, the host's
/// word about the terms, every plan anyone made. Sent once, on the
/// greeting that first names them, and no longer than the feed itself,
/// which keeps [`CHAT_LINES`](CHAT_LINES).
///
/// To that peer alone, not to the table: everyone else was there.
fn catch_up_on_the_feed(state: &LobbyState, peer: usize) {
    let Some(hosted) = state.hosted() else {
        return;
    };
    for said in &state.chat {
        hosted
            .transport
            .send_to(peer, NetMsg::chat(&said.who, &said.line));
    }
}

/// Say something to every peer at the table, as the host. An empty `who`
/// is the lobby speaking rather than a player.
/// The host asks the peer at place `at` of the table (1 is the host's own)
/// to leave: it is told, and forgotten, and everyone else hears why the
/// chair is empty. The table is the host and the peers who came to play,
/// in order, which is how the AT THIS BEACH card numbers them.
pub(super) fn ask_to_leave(
    state: &mut LobbyState,
    bots: &mut crate::app::bot_seats::BotSeats,
    tr: &'static crate::app::i18n::Tr,
    at: usize,
) {
    let Some(hosted) = state.hosted_mut() else {
        return;
    };
    let people: Vec<usize> = (0..hosted.peers.len())
        .filter(|&peer| hosted.peers.get(peer).is_some_and(|p| !p.watch))
        .collect();
    // Past the people sit the bots that came straight here.
    if let Some(bot) = at.checked_sub(2 + people.len())
        && let Some(door) = &mut bots.door
        && let Some((id, who)) = door.arrived(tr).get(bot).cloned()
    {
        door.let_go(id);
        let notice = fill(tr.lobby_kicked_feed, &[("p", &who)]);
        state.say("", &notice);
        announce_to_table(state, "", &notice);
        return;
    }
    let Some(&peer) = people.get(at.wrapping_sub(2)) else {
        return;
    };
    let who = hosted
        .peers
        .get(peer)
        .map_or_else(String::new, |p| p.name.clone());
    // Told, forgotten, and kept out: a greeting already on its way would
    // otherwise seat it again as a ghost row (`UdpTransport::turn_away`).
    hosted.turn_away(peer);
    let notice = fill(tr.lobby_kicked_feed, &[("p", &who)]);
    state.say("", &notice);
    announce_to_table(state, "", &notice);
}

pub(super) fn announce_to_table(state: &LobbyState, who: &str, line: &str) {
    if let Some(hosted) = state.hosted() {
        hosted.transport.send(NetMsg::chat(who, line));
    }
}

/// Which peers get a seat, in peer order, and which are watching (`None`).
///
/// Seats run out before connections do: the socket takes
/// [`MAX_PEERS`](crate::transport::MAX_PEERS) and the table has
/// [`MAX_PLAYERS`] chairs, one of them the host's. The surplus is seated
/// as onlookers rather than handed a seat number the sim has no slot for,
/// so a beach holds the host and sixteen others in any mix: five rivals
/// and eleven watching, or one AI and sixteen watching.
///
/// Named rather than counted, because the count moved: this said nine
/// from before the socket was raised to sixteen, which is the change that
/// made a peer in line cheap enough to allow more of them.
pub(super) fn seat_plan(peers: &PeerBook) -> Vec<Option<u8>> {
    let mut next = 1u8; // seat 0 is the host's
    peers
        .iter()
        .map(|peer| {
            if peer.watch || usize::from(next) >= MAX_PLAYERS {
                return None;
            }
            let seat = next;
            next += 1;
            Some(seat)
        })
        .collect()
}

/// Tell the network a hosted beach is going away, if this lobby is hosting
/// one: the host leaving the screen. Starting the match does not come
/// through here: the launch takes the beacon into the round, which keeps
/// the beach listed as in progress and says goodbye when it ends. A joiner
/// still in the lobby reads this goodbye as its host closing the beach.
pub(super) fn say_goodbye(state: &LobbyState) {
    if let Some(hosted) = state.hosted()
        && let Ok(addr) = hosted.transport.local_addr()
    {
        hosted.announcer.closing(addr.port());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` nameless peers, the given ones watching.
    fn table(n: usize, watchers: &[usize]) -> PeerBook {
        let names = vec![""; n];
        peers_named(&names, watchers)
    }

    /// Seats go to the peers that came to play, in the order they arrived,
    /// with the host holding seat zero throughout.
    #[test]
    fn the_table_is_dealt_in_arrival_order_behind_the_host() {
        assert_eq!(seat_plan(&table(0, &[])), Vec::<Option<u8>>::new());
        assert_eq!(
            seat_plan(&table(1, &[])),
            vec![Some(1)],
            "the host keeps zero"
        );
        assert_eq!(seat_plan(&table(3, &[])), vec![Some(1), Some(2), Some(3)]);
    }

    /// An onlooker takes no chair, and the players behind it close up
    /// rather than inheriting a gap.
    #[test]
    fn watchers_are_stepped_over_and_the_seats_close_up() {
        assert_eq!(seat_plan(&table(3, &[0])), vec![None, Some(1), Some(2)]);
        assert_eq!(seat_plan(&table(3, &[1])), vec![Some(1), None, Some(2)]);
        assert_eq!(seat_plan(&table(3, &[0, 1, 2])), vec![None, None, None]);
    }

    /// Every answer `host_step` can give: it asks who is asking before
    /// anything goes on the air, does nothing once there is already a
    /// beach, and never stops the dev hook to ask a question.
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
        assert_eq!(
            seat_plan(&table(4, &[])),
            vec![Some(1), Some(2), Some(3), Some(4)]
        );
        // Peers 1 and 3 came to watch, so the seats close up behind them.
        assert_eq!(
            seat_plan(&table(4, &[1, 3])),
            vec![Some(1), None, Some(2), None]
        );
        // Every connection the socket will take, all of them here to play.
        let full = seat_plan(&table(crate::transport::MAX_PEERS, &[]));
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
    /// it cannot mean both. `host_tick` runs before the input that owns the
    /// keyboard, so an Enter meant to send a line launched the round.
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

    /// The host asks the second place at its table to leave: that peer is
    /// told, the host forgets it, and the bot flag its greeting carried
    /// was on its row until then.
    #[test]
    fn a_peer_asked_to_leave_is_told_and_forgotten() {
        let mut state = LobbyState {
            standing: Standing::hosting(
                Announcer::new(0xC0FFEE).expect("announcer"),
                UdpTransport::host(0).expect("game socket"),
            ),
            ..LobbyState::default()
        };
        let port = state
            .hosted()
            .expect("hosting")
            .transport
            .local_addr()
            .expect("addr")
            .port();
        let mut bot = UdpTransport::join(("127.0.0.1", port)).expect("join");
        bot.send(NetMsg::hello_bot("Greedy (Ana)"));
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let hosted = state.hosted_mut().expect("hosting");
            let welcome = crate::transport::OnAir {
                bots: true,
                ..crate::transport::OnAir::default()
            };
            let picked = work_the_socket(hosted, 0.0, welcome);
            for (from, told) in picked.greeted {
                hosted.peers.row(from).name = told;
            }
            if hosted
                .peers
                .get(0)
                .is_some_and(|peer| peer.name == "Greedy (Ana)")
            {
                break;
            }
        }
        assert!(
            state
                .hosted()
                .expect("hosting")
                .peers
                .get(0)
                .is_some_and(|p| p.bot),
            "the greeting said a bot drives this seat"
        );
        assert_eq!(
            state.roster_kinds(),
            vec![crate::sim::SeatKind::Human, crate::sim::SeatKind::Bot]
        );
        ask_to_leave(
            &mut state,
            &mut crate::app::bot_seats::BotSeats::default(),
            &crate::app::i18n::EN,
            2,
        );
        assert_eq!(state.hosted().expect("hosting").peers.len(), 0, "forgotten");
        assert!(
            state
                .chat
                .iter()
                .any(|said| said.line.contains("asked to leave"))
        );
        let mut told = false;
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            told |= bot
                .recv_all()
                .into_iter()
                .any(|(msg, _)| msg == NetMsg::Kicked);
            if told {
                break;
            }
        }
        assert!(told, "the peer was told why");
    }

    /// A beach that said bots are not welcome turns a bot's greeting
    /// away: told to go, and kept off the table. A person greeting in the
    /// same batch, after it, keeps their own place and name.
    #[test]
    fn a_bot_is_not_seated_where_bots_are_not_welcome() {
        let mut state = LobbyState {
            standing: Standing::hosting(
                Announcer::new(0xB07).expect("announcer"),
                UdpTransport::host(0).expect("game socket"),
            ),
            ..LobbyState::default()
        };
        let port = state
            .hosted()
            .expect("hosting")
            .transport
            .local_addr()
            .expect("addr")
            .port();
        let mut bot = UdpTransport::join(("127.0.0.1", port)).expect("join");
        let person = UdpTransport::join(("127.0.0.1", port)).expect("join");
        bot.send(NetMsg::hello_bot("Greedy (Ana)"));
        std::thread::sleep(std::time::Duration::from_millis(20));
        person.send(NetMsg::hello("Bo"));
        let unwelcome = crate::transport::OnAir {
            bots: false,
            ..crate::transport::OnAir::default()
        };
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let hosted = state.hosted_mut().expect("hosting");
            let picked = work_the_socket(hosted, 0.0, unwelcome);
            for (from, told) in picked.greeted {
                hosted.peers.row(from).name = told;
            }
            if hosted.peers.get(0).is_some_and(|peer| peer.name == "Bo") {
                break;
            }
        }
        let hosted = state.hosted().expect("hosting");
        assert_eq!(hosted.peers.len(), 1, "only the person is at the table");
        assert_eq!(hosted.transport.peer_count(), 1);
        assert!(
            hosted
                .peers
                .get(0)
                .is_some_and(|p| p.name == "Bo" && !p.bot)
        );
        let mut told = false;
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            told |= bot
                .recv_all()
                .into_iter()
                .any(|(msg, _)| msg == NetMsg::Kicked);
            if told {
                break;
            }
        }
        assert!(told, "the bot was told to go");
    }

    /// Chat is the one message that names its own sender, and the host is
    /// the only peer that can check the claim. A line comes back under the
    /// name its sender greeted with, whatever the datagram says, including
    /// the empty name that is the room's own voice, so a forged notice
    /// comes out as somebody saying something odd.
    ///
    /// Over a real socket, because the whole point is which end of it the
    /// line arrived on.
    #[test]
    fn a_peer_speaks_under_the_name_it_greeted_with() {
        let mut state = LobbyState {
            standing: Standing::hosting(
                Announcer::new(0xC0FFEE).expect("announcer"),
                UdpTransport::host(0).expect("game socket"),
            ),
            ..LobbyState::default()
        };
        let port = state
            .hosted()
            .expect("hosting")
            .transport
            .local_addr()
            .expect("addr")
            .port();
        let bo = UdpTransport::join(("127.0.0.1", port)).expect("join");
        bo.send(NetMsg::hello("Bo"));

        // Drain until the greeting has been folded into Bo's row, which is
        // what the stamping reads.
        let mut said = Vec::new();
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let hosted = state.hosted_mut().expect("hosting");
            let picked = work_the_socket(hosted, 0.0, crate::transport::OnAir::default());
            for (from, told) in picked.greeted {
                hosted.peers.row(from).name = told;
            }
            said.extend(picked.said);
            if state
                .hosted()
                .expect("hosting")
                .peers
                .get(0)
                .is_some_and(|peer| peer.name == "Bo")
            {
                break;
            }
        }
        assert_eq!(
            state
                .hosted()
                .expect("hosting")
                .peers
                .get(0)
                .map(|p| p.name.as_str()),
            Some("Bo"),
            "the greeting is on file"
        );

        // Bo now says three things: one under its own name, one wearing a
        // rival's, and one wearing none.
        bo.send(NetMsg::chat("Bo", "ready?"));
        bo.send(NetMsg::chat("Anna", "I am Anna, honest"));
        bo.send(NetMsg::chat("", "Anna has left the beach"));
        said.clear();
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let hosted = state.hosted_mut().expect("hosting");
            said.extend(work_the_socket(hosted, 0.0, crate::transport::OnAir::default()).said);
            if said.len() >= 2 {
                break;
            }
        }
        let heard: Vec<(String, String)> = said
            .iter()
            .map(|(_, name, text)| {
                (
                    crate::transport::name_from_wire(name),
                    crate::transport::chat_from_wire(text),
                )
            })
            .collect();
        assert_eq!(
            heard,
            [
                ("Bo".to_string(), "ready?".to_string()),
                ("Bo".to_string(), "I am Anna, honest".to_string()),
                ("Bo".to_string(), "Anna has left the beach".to_string()),
            ],
            "every line under the name Bo greeted with: no rival's name to \
             wear, and no forging the room's"
        );
    }

    /// Somebody who turns up late is shown the room, not an empty one.
    ///
    /// The host relays a line as it is said and never again, so everything
    /// said before a peer arrived was said to a beach that peer could not
    /// see. On a busy evening that is the whole conversation: who else
    /// turned up, what the host said about the terms, who is waiting for
    /// whom. Found by joining a beach two peers had already been talking
    /// on and finding the feed blank but for my own arrival.
    #[test]
    fn a_late_arrival_is_shown_what_the_room_already_said() {
        let mut state = LobbyState {
            standing: Standing::hosting(
                Announcer::new(0xC0FFEE).expect("announcer"),
                UdpTransport::host(0).expect("game socket"),
            ),
            ..LobbyState::default()
        };
        // What the room said before anybody new was listening.
        state.say("Ann", "anyone up for a round?");
        state.say("", "Bo joined");
        let port = state
            .hosted()
            .expect("hosting")
            .transport
            .local_addr()
            .expect("addr")
            .port();

        let mut latecomer = UdpTransport::join(("127.0.0.1", port)).expect("join");
        latecomer.send(NetMsg::hello("Cy"));

        let mut greeted = Vec::new();
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let hosted = state.hosted_mut().expect("hosting");
            greeted
                .extend(work_the_socket(hosted, 0.0, crate::transport::OnAir::default()).greeted);
            if !greeted.is_empty() {
                break;
            }
        }
        assert_eq!(greeted, [(0, "Cy".to_string())], "Cy is at the door");
        note_arrivals_and_departures(&mut state, &crate::app::i18n::EN, greeted);

        let mut heard = Vec::new();
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            for (msg, _) in latecomer.recv_all() {
                if let NetMsg::Chat { name, text } = msg {
                    heard.push((
                        crate::transport::name_from_wire(&name),
                        crate::transport::chat_from_wire(&text),
                    ));
                }
            }
            if heard.len() >= 3 {
                break;
            }
        }
        assert_eq!(
            heard,
            [
                ("Ann".to_string(), "anyone up for a round?".to_string()),
                (String::new(), "Bo joined".to_string()),
                (String::new(), "Cy joined".to_string()),
            ],
            "the room as it stands, oldest first, and then their own arrival"
        );
    }

    /// A spectator is announced in its own words and then held to its
    /// name like anybody else.
    ///
    /// Both halves were missing while `Watch` carried no name: the feed
    /// announced every player and no spectator at all, so a host on a busy
    /// beach could not tell who had turned up to watch, and the check on
    /// who a chat line claims to be from had nothing to check against, so
    /// the one kind of peer nobody can see was the one kind that could
    /// speak under a rival's name.
    #[test]
    fn a_spectator_is_announced_by_name_and_then_held_to_it() {
        let mut state = LobbyState {
            standing: Standing::hosting(
                Announcer::new(0xDECAF).expect("announcer"),
                UdpTransport::host(0).expect("game socket"),
            ),
            ..LobbyState::default()
        };
        let port = state
            .hosted()
            .expect("hosting")
            .transport
            .local_addr()
            .expect("addr")
            .port();
        let watcher = UdpTransport::join(("127.0.0.1", port)).expect("join");
        watcher.send(NetMsg::watch("Dee"));

        // Drain until the greeting lands, then write it down the way the
        // lobby's own tick does.
        let mut greeted = Vec::new();
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let hosted = state.hosted_mut().expect("hosting");
            greeted
                .extend(work_the_socket(hosted, 0.0, crate::transport::OnAir::default()).greeted);
            if !greeted.is_empty() {
                break;
            }
        }
        assert_eq!(
            greeted,
            [(0, "Dee".to_string())],
            "a spectator says what to call it, like everybody else"
        );
        note_arrivals_and_departures(&mut state, &crate::app::i18n::EN, greeted);
        assert!(
            state.chat.iter().any(|said| said.line == "Dee is watching"),
            "and the room is told, in the words for watching rather than \
             for taking a chair: {:?}",
            state.chat
        );

        watcher.send(NetMsg::chat("", "Anna has left the beach"));
        watcher.send(NetMsg::chat("Cy", "good luck!"));
        let mut said = Vec::new();
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let hosted = state.hosted_mut().expect("hosting");
            said.extend(work_the_socket(hosted, 0.0, crate::transport::OnAir::default()).said);
            if !said.is_empty() {
                break;
            }
        }
        let heard: Vec<(String, String)> = said
            .iter()
            .map(|(_, name, text)| {
                (
                    crate::transport::name_from_wire(name),
                    crate::transport::chat_from_wire(text),
                )
            })
            .collect();
        assert_eq!(
            heard,
            [
                ("Dee".to_string(), "Anna has left the beach".to_string()),
                ("Dee".to_string(), "good luck!".to_string()),
            ],
            "both lines under the name it greeted with: no rival's name to \
             wear, and the room's own voice no longer on offer either, \
             which it was for as long as a spectator had no name to give \
             back"
        );
    }
}
