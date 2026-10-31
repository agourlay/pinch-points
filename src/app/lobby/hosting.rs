//! Putting a beach on the air and keeping it there: the beacon, the peers
//! who turn up, and the moment the round begins.
//!
//! The host is the one player who both talks and starts, which is why
//! nothing here acts on Enter while a line is being typed.

use super::*;
use crate::app::i18n::fill;
use crate::transport::Notice;

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
pub(super) fn address_here(transport: &UdpTransport) -> Option<std::net::SocketAddrV4> {
    let port = transport.local_addr().ok()?.port();
    match crate::transport::local_ip()? {
        std::net::IpAddr::V4(ip) => Some(std::net::SocketAddrV4::new(ip, port)),
        // The probe binds an IPv4 socket, so never asked.
        std::net::IpAddr::V6(_) => None,
    }
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
    (keys, mut hooked, mut held, mut partnered): (
        Res<ButtonInput<KeyCode>>,
        Local<bool>,
        Local<u32>,
        Local<bool>,
    ),
    (settings, device): (Res<GameSettings>, Res<crate::app::gamepad::LastDevice>),
    (mut config, mut menu): (
        ResMut<MatchConfig>,
        ResMut<crate::app::match_setup::MatchMenu>,
    ),
    beaches: Res<crate::app::match_setup::CustomBeaches>,
    mut state: ResMut<LobbyState>,
    mut arena: IntoArena,
) {
    let tr = settings.tr();
    // Split, so the beacon borrows the beach's name while the socket works
    // rather than copying it every frame.
    let LobbyState {
        standing,
        game_name,
        ..
    } = &mut *state;
    let Standing::Hosting(hosted) = standing else {
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
    let mut sent_away = Vec::new();
    let arrived = host_the_bots(
        &mut arena.bots,
        &config,
        tr,
        hosted.players_aboard(),
        &mut sent_away,
    );
    let taken = chairs_taken(1 + hosted.players_aboard() as u8, arrived.len());
    let on_air = crate::transport::OnAir {
        name: game_name,
        host: &settings.names[0],
        taken,
        seats: MAX_PLAYERS as u8,
        bots: config.bots_welcome,
        // The socket has room for `MAX_PEERS`, watchers counted, and
        // answers every greeting past that with `Full`. Said on the air
        // too, so the list shows the beach full before anyone dials it.
        full: hosted.transport.peer_count() >= crate::transport::MAX_PEERS,
    };
    let picked = work_the_socket(hosted, time.delta_secs(), on_air);
    let do_announce = picked.announced;
    for (_, name, text) in picked.said {
        state.hear(&name, &text);
    }
    // The bots the dial turned away, by either road: those that came
    // through the doorway and those that joined the table.
    sent_away.extend(picked.sent_away);
    for who in &sent_away {
        tell_sent_away(&mut state, tr, who);
    }
    note_arrivals_and_departures(&mut state, tr, picked.greeted);
    // Counted after the departures, so a table that just lost somebody is
    // not announced as still holding them.
    let joined = state.hosted().map_or(0, |hosted| hosted.peers.len());
    // The host's own view of the table, and the peers': the same list,
    // sent on every beacon tick so a lost one costs a second, not a screen
    // that stays wrong until somebody joins or leaves.
    //
    // Built every frame: it is six names at most, and building it only
    // when something looked changed was wrong both ways. Every peer greets
    // once a second, so a greeting said nothing about a change and a
    // crowded beach rebuilt it forty times a second anyway, while a place
    // swapped for another in one frame (a seated peer leaving as one
    // without a chair slid in, a bot for a bot) kept the old table on
    // screen for up to a second.
    state.table = state.roster(tr, &crate::transport::name_as_sent(&settings.names[0]));
    seat_door_bots(&mut state, &arrived);
    state.seated = super::partners::host_seated(&state, &config, settings.team_mode, &arrived);
    if let Some(hosted) = state.hosted_mut() {
        hosted.bot_string = arena
            .bots
            .door
            .as_ref()
            .and_then(crate::app::bot_seats::Doorway::open_string);
    }
    if do_announce {
        send_roster(&state, &config, settings.team_mode, &beaches);
    }
    let (players_aboard, watchers_aboard, port) = state.hosted().map_or((0, 0, None), |hosted| {
        (
            hosted.players_aboard(),
            hosted.watchers_aboard(),
            hosted.transport.local_addr().ok().map(|addr| addr.port()),
        )
    });
    // A bot that came straight here is a rival aboard like any other. A
    // watcher counts here too, since Enter starts a table it is on (see
    // `should_launch`), though not as a player in the line it is shown.
    let rivals = joined + arrived.len();
    if rivals > 0 {
        let aboard = players_aboard + arrived.len();
        let ai = hosts_dealt(&state, &config).map_or(0, |dealt| dealt.bots);
        // The key that starts the match, as the hands last used know it.
        let start = crate::app::gamepad::scheme::spelled(
            tr,
            *device,
            &crate::app::gamepad::Face::Hosting { partners: false },
            crate::app::gamepad::scheme::Does::StartMatch,
            tr.key_enter,
        );
        state.feedback = gathering_feedback(tr, aboard, watchers_aboard, ai, &start);
    } else {
        // The last peer timed out: without this the line kept saying "1
        // rival aboard" over an empty table until somebody else turned up,
        // and Enter did nothing. Back to the waiting message it started on.
        state.feedback = match port {
            Some(port) => fill(tr.lobby_hosting, &[("p", &port.to_string())]),
            None => tr.lobby_hosting_noport.to_string(),
        };
    }
    // The host is on the map page: the table it froze is watched, and the
    // match goes on once a card is picked.
    if state
        .hosted()
        .is_some_and(|hosted| hosted.picking.is_some())
    {
        // The page's own heading names the table it picks for; said here
        // too, it was said twice.
        state.feedback.clear();
        // `PINCH_LOBBY_PICK=go`: the card under the cursor is picked a
        // second after the page opens, for an unattended run that plays on.
        *held += 1;
        if crate::app::dev::lobby_pick_go() && *held == 30 {
            pick::pick_the_card_here(&mut state, &mut config, &menu, &beaches);
        }
        let mut pick = Pick {
            config: &mut config,
            menu: &mut menu,
            beaches: &beaches,
        };
        tend_the_pick(
            &mut state, &settings, &mut pick, joined, &arrived, &mut arena,
        );
        return;
    }
    *held = 0;
    // `PINCH_LOBBY_PARTNERS`: the host's partner pick, unattended, once.
    if let Some((seat, at)) = crate::app::dev::lobby_partners()
        && rivals >= at
        && !std::mem::replace(&mut *partnered, true)
        && super::partners::partners_open(&state)
    {
        super::partners::pick_partners(&mut state, &arrived, seat.saturating_sub(1));
        send_roster(&state, &config, settings.team_mode, &beaches);
    }
    let quota = crate::app::dev::auto_host_quota();
    let launch = should_launch(
        rivals,
        // Picking whom to ask to leave holds the keyboard as typing does:
        // the host runs before the lobby's keys, so an Enter pressed at the
        // "pick a number" prompt used to start the match.
        state.typing.is_some()
            || state
                .hosted()
                .is_some_and(|hosted| hosted.kicking.is_some() || hosted.partnering),
        crate::app::menu_ui::enter(&keys),
        quota,
    );
    // The dev hook's start, pressed once somebody is aboard.
    let hook =
        rivals > 0 && crate::app::dev::lobby_pick() && !std::mem::replace(&mut *hooked, true);
    if !launch && !hook {
        return;
    }
    // Unattended, the match goes on on the map already set: there is
    // nobody at the screen to pick one.
    if quota.is_some_and(|quota| rivals >= quota) {
        launch_the_match(
            &mut state, &settings, &config, &beaches, joined, &mut arena, None,
        );
        return;
    }
    pick::open_the_pick(&mut state, &mut config, &mut menu, &beaches, &arrived);
    // At once rather than on the next beacon tick, so the table hears
    // the host is picking.
    send_roster(&state, &config, settings.team_mode, &beaches);
}

/// What the map page reads and sets: the host's setup, the page's cursor,
/// the shelf.
struct Pick<'a> {
    config: &'a mut MatchConfig,
    menu: &'a mut crate::app::match_setup::MatchMenu,
    beaches: &'a crate::app::match_setup::CustomBeaches,
}

/// One host tick with the map page up: a card picked starts the match on
/// the table as frozen, and anyone frozen at it leaving calls the start
/// off, back to the lobby card with the room told who went. Anybody new
/// is no change: they watch this match.
fn tend_the_pick(
    state: &mut LobbyState,
    settings: &GameSettings,
    pick: &mut Pick,
    joined: usize,
    door: &[(crate::bots::listener::BotId, String)],
    arena: &mut IntoArena,
) {
    let Some((gone, chosen)) = state.hosted().and_then(|hosted| {
        let picking = hosted.picking.as_ref()?;
        Some((
            pick::gone_from_the_table(picking, &hosted.peers, door),
            picking.chosen,
        ))
    }) else {
        return;
    };
    if gone.is_none() && !chosen {
        return;
    }
    let Some(picking) = state.hosted_mut().and_then(|hosted| hosted.picking.take()) else {
        return;
    };
    if let Some(gone) = gone {
        picking.page.close(
            crate::app::match_setup::Close::PutBack,
            pick.config,
            pick.menu,
            pick.beaches,
        );
        let bot = match gone {
            pick::Gone::Player => false,
            pick::Gone::Bot => true,
        };
        tell_the_room(state, settings.tr(), Notice::PickCalledOff { bot });
        send_roster(state, pick.config, settings.team_mode, pick.beaches);
        return;
    }
    let frozen = Frozen {
        bots: picking.bots.clone(),
        seed: pick.menu.seed,
    };
    launch_the_match(
        state,
        settings,
        pick.config,
        pick.beaches,
        joined,
        arena,
        Some(frozen),
    );
    picking.page.close(
        crate::app::match_setup::Close::Keep,
        pick.config,
        pick.menu,
        pick.beaches,
    );
}

/// The table the host froze to pick the map, which the launch seats (the
/// peers frozen on their rows, these bots by who they are), and the seed
/// the map page's pictures were drawn with, which it is played on.
#[derive(Clone, Debug)]
struct Frozen {
    bots: Vec<crate::bots::listener::BotId>,
    seed: u64,
}

/// Seat the bots that came straight to this beach (route 2) after the
/// people on the host's table, as far as there are chairs.
fn seat_door_bots(state: &mut LobbyState, arrived: &[(crate::bots::listener::BotId, String)]) {
    for (_, name) in arrived {
        if state.table.len() < MAX_PLAYERS {
            state.table.push(super::TablePlace {
                name: name.clone(),
                kind: crate::sim::SeatKind::Bot,
            });
        }
    }
}

/// Send the host's table to every peer: who sits where, and the dials as
/// the launch would deal this table, so a joiner's terms card shows the
/// match it is joining, and to each whether there is a chair for it
/// (`NetMsg::Roster::chair`). The lobby's half of
/// [`OnlineSession`]'s own roster, which a round sends
/// from its copy of both.
pub(super) fn send_roster(
    state: &LobbyState,
    config: &MatchConfig,
    teams: crate::app::teams::TeamMode,
    beaches: &crate::app::match_setup::CustomBeaches,
) {
    let (Some(hosted), Some(dealt)) = (state.hosted(), hosts_dealt(state, config)) else {
        return;
    };
    // The AI and the map as the launch would deal this table, which the
    // joiner's card reads, not the dials they come from. The seed is not
    // one of them yet, it is struck fresh at launch, so a placeholder
    // rides along and the card ignores it.
    let terms = MatchTerms {
        bots: dealt.bots,
        map: dealt.map.index() as u8,
        order: state.seated.order,
        ..crate::app::match_setup::terms(config, teams, 0)
    };
    let names: Vec<String> = state.table.iter().map(|place| place.name.clone()).collect();
    let kinds: Vec<crate::sim::SeatKind> = state.table.iter().map(|place| place.kind).collect();
    let roster = |chair| NetMsg::Roster {
        seats: humans_here(state),
        names: crate::transport::wire_table(&names),
        terms,
        kinds: crate::transport::wire_kinds(&kinds),
        // Named only if the table as it now sits would play it.
        beach: roster_beach(config, dealt.seats, beaches),
        chair: Some(chair),
        picking: hosted.picking.is_some(),
    };
    // A peer that came to play past the table's last chair has none: the
    // launch seats it watching ([`PeerBook::seat_plan`]), and its line has
    // to say so before then. While the host picks the map, the launch
    // seats the table it froze and nobody else ([`pick::frozen_plan`]), so
    // one that sat down since has none either, whoever it is.
    let chairless: Vec<usize> = match hosted.picking {
        Some(_) => {
            let plan = pick::frozen_plan(&hosted.peers);
            hosted
                .peers
                .iter()
                .enumerate()
                .filter(|(peer, row)| row.wants_a_chair() && plan[*peer].is_none())
                .map(|(peer, _)| peer)
                .collect()
        }
        None => hosted
            .peers
            .off_the_table()
            .filter(|(_, row)| row.wants_a_chair())
            .map(|(peer, _)| peer)
            .collect(),
    };
    for peer in 0..hosted.transport.peer_count() {
        hosted
            .transport
            .send_to(peer, roster(!chairless.contains(&peer)));
    }
}

/// The chairs a beacon says are taken, with `people` seated (the host
/// among them) and `bots` come straight here (route 2).
///
/// A person who turns up is seated ahead of the bots, which give way
/// ([`crate::app::net::seat_the_hosts_bots`]), so the bots count only as
/// far as the launch would still seat them with one more person aboard.
/// Counted whole, a table of people and bots would be listed full to a
/// person the launch would seat.
pub(crate) fn chairs_taken(people: u8, bots: usize) -> u8 {
    let held = bots.min(crate::app::net::chairs_for_bots(people.saturating_add(1)));
    people.saturating_add(held as u8).min(MAX_PLAYERS as u8)
}

/// Keep the doorway for bots straight to this beach (route 2) as the host
/// wants it, and say who has come in by it. The bots seated there when the
/// host stops welcoming them go with the door, named into `sent_away` for
/// the room to be told, as a bot that joined the table is.
fn host_the_bots(
    bots: &mut crate::app::bot_seats::BotSeats,
    config: &MatchConfig,
    tr: &'static crate::app::i18n::Tr,
    people: usize,
    sent_away: &mut Vec<String>,
) -> Vec<(crate::bots::listener::BotId, String)> {
    if !config.bots_welcome {
        if let Some(door) = bots.door.take() {
            sent_away.extend(door.arrived(tr).into_iter().map(|(_, who)| who));
        }
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
    // The host's own chair and each person's come first. A bot that has
    // dropped still holds its chair while it may come back, so it is
    // counted: left out, the door opened another, and the bot coming
    // back made one more than the table seats.
    let held = door
        .chairs
        .iter()
        .filter(|chair| chair.bot.is_some())
        .count();
    let room = 1 + people + held < MAX_PLAYERS;
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
    frozen: Option<Frozen>,
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
        kicking: _,
        bot_string: _,
        tail: _,
        rounds_dealt,
        picking: _,
        partnering: _,
        mut arranged,
    }) = std::mem::take(&mut state.standing)
    else {
        unreachable!("launching a match from a lobby that is not hosting");
    };
    let mut peers = peers;
    debug_assert_eq!(joined, peers.len());
    // A player who sat down while the host picked the map watches this
    // match: the map was picked for the table without them.
    let mut plan = match frozen {
        Some(_) => pick::frozen_plan(&peers),
        None => peers.seat_plan(peers.len()),
    };
    let people = 1 + plan.iter().filter(|seat| seat.is_some()).count() as u8;
    // The bots that came straight here (route 2) sit after the people, as
    // many as there are chairs for; the host speaks for each of them.
    let mut bots_here: Vec<HostedBot> = arena
        .bots
        .door
        .as_ref()
        .map(|door| door.arrived(settings.tr()))
        .unwrap_or_default()
        .into_iter()
        .filter(|(id, _)| {
            frozen
                .as_ref()
                .is_none_or(|frozen| frozen.bots.contains(id))
        })
        .map(|(id, name)| HostedBot { seat: 0, id, name })
        .collect();
    crate::app::net::seat_the_hosts_bots(&mut bots_here, people);
    let humans = people + bots_here.len() as u8;
    // AI seats come from the host's match setup, and sit behind the
    // humans. The host's dials and its team-scoring setting travel with
    // them: a match is played on one set of terms, not four.
    //
    // A beach needs two castles, so the AI floor is raised to reach two
    // seats when only onlookers turned up (every peer a watcher, or a lone
    // watcher aboard): a one-seat `Start` would leave the host playing an
    // empty beach.
    let dealt = dealt(config, humans);
    let (bots, seats) = (dealt.bots, dealt.seats);
    // On the seed the map page drew its pictures with, so the beach
    // previewed is the beach played.
    let seed = frozen
        .as_ref()
        .map_or_else(crate::app::clock::fresh_seed, |frozen| frozen.seed);
    // The Random card draws now, from the maps that seat this table and
    // can travel to it, and the terms name the map it landed on.
    let drawn;
    let config = match config.pick {
        crate::app::match_setup::Pick::Random { .. } => {
            let mut table = MatchConfig {
                seats,
                ..config.clone()
            };
            crate::app::match_setup::draw_online_map(
                &mut table,
                beaches,
                crate::app::clock::fresh_seed(),
            );
            drawn = table;
            &drawn
        }
        crate::app::match_setup::Pick::InForce | crate::app::match_setup::Pick::Moved(_) => config,
    };
    // A beach the host built travels with the invitation; a generated one
    // is already described by the seed in the terms.
    let beach = crate::app::match_setup::beach_bytes(config, seats, beaches);
    let mut terms = MatchTerms {
        bots,
        // The beach has to hold everyone who turned up, which the host
        // could not have known when it picked one.
        map: map_for(config, seats).index() as u8,
        ..crate::app::match_setup::terms(config, settings.team_mode, seed)
    };
    // Everybody in the seat the host's partner picks put them in.
    crate::app::net::arrange_the_deal(
        (&mut plan, &mut bots_here),
        &mut peers,
        arranged.as_mut(),
        &mut terms,
        seats,
    );
    let names = seat_names(&settings.names[0], &plan, &peers, &bots_here);
    let kinds = crate::app::net::seat_kinds(&plan, &peers, &bots_here, terms, seats);
    // A series begins at round one with an empty tally; a single round
    // carries no standing at all.
    let standing = terms
        .is_series()
        .then(crate::transport::SeriesStanding::opening);
    // The input delay from how quickly the people about to be seated
    // have been answering the host's pings.
    let delay = crate::app::net::input_delay(&peers, &plan);
    let mut session = OnlineSession::new(
        transport,
        crate::app::net::lockstep_for(Some(0), terms.player_seats(seats), delay),
        seats,
        terms,
    );
    // The host plays on the same bytes it sent, not on its own copy of the
    // file: if the two ever disagreed, the hash check would find it and
    // nobody could say which of them was right.
    session.beach = beach;
    session.peers = peers;
    session.names = names;
    session.arranged = arranged;
    // Counted on from the match before on this socket, so its first round
    // is dealt after every round a joiner may still be sitting over.
    session.rounds_dealt = rounds_dealt;
    session.deal(&plan, kinds, bots_here, standing);
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

/// The table's names: the host is seat 0 under its own P1 name, and each
/// seated peer under the name its greeting carried. Empty slots (AI
/// seats, the nameless) fall back to seat labels on every screen.
///
/// The host's own name as the joiners will read it off the `Start`: a
/// name wider than its 24 bytes is cut there, and the host's own screen
/// used to keep it whole, so the scores and the feed named one player two
/// ways. The peers' names came off the wire already.
fn seat_names(
    me: &str,
    plan: &[Option<u8>],
    peers: &PeerBook,
    bots_here: &[HostedBot],
) -> [String; MAX_PLAYERS] {
    crate::app::net::seat_names(
        crate::transport::name_as_sent(me),
        plan,
        |peer| peers.get(peer).map(|peer| peer.name.as_str()),
        bots_here,
    )
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
            let notice = Notice::Joined {
                name: crate::transport::wire_name(&told),
                watching,
            };
            tell_the_room(state, tr, notice);
        }
    }
    // Anyone unheard for too long has walked away: UDP will not say so, and
    // a table that keeps counting them never has room again.
    let gone: Vec<usize> = state.hosted().map_or(Vec::new(), |hosted| {
        hosted.peers.to_forget(|row| row.silence > PEER_TTL)
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
            let name = crate::transport::wire_name(&who);
            tell_the_room(state, tr, Notice::Left { name });
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
    /// The names of those of them who were at the table, for the room to
    /// be told: a bot the host's dial sent away vanished without a word.
    sent_away: Vec<String>,
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
        kicking: _,
        bot_string: _,
        tail,
        rounds_dealt: _,
        picking: _,
        partnering: _,
        arranged: _,
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
    // And how quickly each peer answers, once a second with the beacon.
    if picked.announced {
        transport.send(NetMsg::ping());
    }
    // Peers still playing the round this table walked back from.
    let mut still_playing = Vec::new();
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
                let who = peers.vouch(from, name);
                if !crate::transport::name_from_wire(&who).is_empty() {
                    picked.said.push((from, who, text));
                }
            }
            // A peer still in the round the table walked back from,
            // resending its commits: it may be short of the last frames.
            NetMsg::Inputs(batch) => still_playing.push((from, batch)),
            // How long the round trip to it takes, for the input delay
            // the launch picks (`input_delay`).
            NetMsg::Pong { stamp } => peers.answered(from, stamp),
            // The room's word is the host's own to say.
            NetMsg::Notice(_)
            | NetMsg::Hash { .. }
            | NetMsg::Start(_)
            | NetMsg::Pause { .. }
            | NetMsg::Resume { .. }
            | NetMsg::Queued { .. }
            | NetMsg::Roster { .. }
            | NetMsg::Abandoned { .. }
            | NetMsg::CatchUp { .. }
            | NetMsg::SpectatorPick { .. }
            | NetMsg::CrowdPicks { .. }
            | NetMsg::Incompatible { .. }
            | NetMsg::Ping { .. }
            | NetMsg::Clock { .. }
            | NetMsg::Kicked
            | NetMsg::NoBots
            | NetMsg::Full => {}
            // Gone, and said so: swept with the silent this same tick
            // (`note_arrivals_and_departures`), with the same word in the
            // feed, rather than counted aboard for four more seconds and
            // dealt a chair if the host launches inside them.
            NetMsg::Leaving => peers.said_goodbye(from),
        }
    }
    // Before anyone is forgotten below, while the indices still name the
    // peers they were read from.
    tail.repeat(transport, delta, picked.announced, &still_playing);
    // Highest first, so each index still names the peer it was read from.
    picked.unwelcome.sort_unstable();
    for &peer in picked.unwelcome.iter().rev() {
        // Told why, not just asked to go: a bot sent away by the host's
        // dial read "the host asked you to leave", as if singled out.
        if let Some(name) = peers.get(peer).map(|row| row.name.clone())
            && !name.is_empty()
        {
            picked.sent_away.push(name);
        }
        transport.turn_away_saying(peer, NetMsg::NoBots);
        peers.forget(peer);
        // And what the others said this frame follows them down.
        let renumber = crate::app::net::renumber;
        picked.greeted.retain_mut(|(from, _)| renumber(from, peer));
        picked.said.retain_mut(|(from, ..)| renumber(from, peer));
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
/// watching, and the `ai` seats the launch deals in behind them
/// ([`hosts_dealt`]), and `start`, the key or button that
/// starts the match.
///
/// Pure, so what a host reads while waiting is checkable without a socket.
pub(super) fn gathering_feedback(
    tr: &'static crate::app::i18n::Tr,
    players_aboard: usize,
    watching: usize,
    ai: u8,
    start: &str,
) -> String {
    let mut line = match players_aboard {
        // Only watchers aboard, which is a table Enter starts all the same:
        // the AI takes the chairs (`terms::dealt` raises it to two seats),
        // and its count follows below. "0 players joined" read as though
        // the watchers had been counted as players and then lost.
        0 => fill(tr.lobby_rivals_none, &[("k", start)]),
        1 => fill(tr.lobby_rivals_one, &[("k", start)]),
        n => fill(tr.lobby_rivals_many, &[("n", &n.to_string()), ("k", start)]),
    };
    if watching > 0 {
        line += &fill(tr.lobby_watchers, &[("n", &watching.to_string())]);
    }
    // The AI seats come from the host's dials, not from this count, so say
    // how many are coming rather than surprising the table with them.
    if ai > 0 {
        line += &fill(tr.lobby_ai_seats, &[("n", &ai.to_string())]);
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
        // A notice again as what happened, for the newcomer to word.
        let msg = match said.notice {
            Some(notice) => NetMsg::Notice(notice),
            None => NetMsg::chat(&said.who, &said.line),
        };
        hosted.transport.send_to(peer, msg);
    }
}

/// Tell the room the host's word sent `who` away: a peer or a bot asked to
/// leave, or a bot the "Bots welcome" dial turned away, by either road.
/// One line for all of them, so none goes without it.
pub(super) fn tell_sent_away(state: &mut LobbyState, tr: &crate::app::i18n::Tr, who: &str) {
    let name = crate::transport::wire_name(who);
    tell_the_room(state, tr, Notice::SentAway { name });
}

/// Say a notice to the room as the lobby itself: in this host's own feed,
/// and to every peer at the table. Sent as what happened rather than in
/// this host's words, so each peer reads it in its own language.
fn tell_the_room(state: &mut LobbyState, tr: &crate::app::i18n::Tr, notice: Notice) {
    state.tell(tr, notice);
    if let Some(hosted) = state.hosted() {
        hosted.transport.send(NetMsg::Notice(notice));
    }
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

    /// The beacon lists a beach full only when the launch could not seat
    /// one more person: bots give way to people, so five people and a bot
    /// are not a full table, and a person who walks up is seated. Counted
    /// whole, the list refused the sixth person the launch would have sat.
    #[test]
    fn the_list_counts_bots_as_the_launch_seats_them() {
        for people in 1..=MAX_PLAYERS as u8 {
            for bots in 0..MAX_PLAYERS {
                let taken = chairs_taken(people, bots);
                let full = taken >= MAX_PLAYERS as u8;
                // One more person is seated whole, and the bots behind
                // them are as many as there are chairs for.
                let room_for_one_more = usize::from(people) < MAX_PLAYERS;
                assert_eq!(full, !room_for_one_more, "{people} people, {bots} bots");
                assert!(taken >= people, "every person is counted");
                // Every bot the launch would seat now is counted, but one.
                let seated_now = bots.min(crate::app::net::chairs_for_bots(people));
                assert!(
                    usize::from(taken) + 1 >= usize::from(people) + seated_now,
                    "{people} people, {bots} bots read {taken}"
                );
            }
        }
        assert_eq!(chairs_taken(5, 1), 5, "five and a bot: a chair for a sixth");
        assert_eq!(chairs_taken(2, 1), 3, "a bot that will sit is counted");
    }

    /// The launch's deal: every peer the lobby holds.
    fn seat_plan(peers: &PeerBook) -> Vec<Option<u8>> {
        peers.seat_plan(peers.len())
    }

    /// `n` nameless peers, the given ones watching.
    fn table(n: usize, watchers: &[usize]) -> PeerBook {
        let names = vec![""; n];
        peers_named(&names, watchers)
    }

    /// The host's own seat carries its name as the joiners will read it:
    /// a Japanese name of twelve is eight on the wire, and was twelve on
    /// the host's own scores.
    #[test]
    fn the_host_wears_the_name_the_wire_gives_it() {
        let peers = peers_named(&["Bo"], &[]);
        let wide = "あ".repeat(12);
        let names = seat_names(&wide, &seat_plan(&peers), &peers, &[]);
        assert_eq!(names[0], "あ".repeat(8));
        assert_eq!(
            names[0],
            crate::transport::table_from_wire(&crate::transport::wire_table(&names))[0],
            "what the host shows is what the joiners decode"
        );
        assert_eq!(names[1], "Bo");
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
            standing: hosting_lobby(),
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
                full: false,
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
            state
                .roster(&crate::app::i18n::EN, "")
                .iter()
                .map(|place| place.kind)
                .collect::<Vec<_>>(),
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

    /// A joiner leaving the lobby says so, and the host lets it go the
    /// same tick, with the same word in the feed the silence rule gives:
    /// not counted aboard for four more seconds, and not dealt a chair if
    /// the host launches inside them.
    #[test]
    fn a_joiner_that_says_goodbye_is_let_go_at_once() {
        let mut state = LobbyState {
            standing: hosting_lobby(),
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
        let tick = |state: &mut LobbyState| {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let hosted = state.hosted_mut().expect("hosting");
            let picked = work_the_socket(hosted, 0.0, crate::transport::OnAir::default());
            note_arrivals_and_departures(state, &crate::app::i18n::EN, picked.greeted);
        };
        for _ in 0..40 {
            tick(&mut state);
            if state.hosted().expect("hosting").players_aboard() == 1 {
                break;
            }
        }
        assert_eq!(state.hosted().expect("hosting").players_aboard(), 1);
        drop(bo);
        for _ in 0..40 {
            tick(&mut state);
            if state.hosted().expect("hosting").peers.is_empty() {
                break;
            }
        }
        let hosted = state.hosted().expect("hosting");
        assert_eq!(hosted.transport.peer_count(), 0, "let go at once");
        assert!(hosted.peers.is_empty());
        assert!(
            state.chat.iter().any(|said| said.line == "Bo left"),
            "and the table is told: {:?}",
            state.chat
        );
    }

    /// A beach that said bots are not welcome turns a bot's greeting
    /// away: told to go, and kept off the table. A person greeting in the
    /// same batch, after it, keeps their own place and name.
    #[test]
    fn a_bot_is_not_seated_where_bots_are_not_welcome() {
        let mut state = LobbyState {
            standing: hosting_lobby(),
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
            full: false,
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
                .any(|(msg, _)| msg == NetMsg::NoBots);
            if told {
                break;
            }
        }
        assert!(told, "the bot was told to go, and why");
    }

    /// A bot that came straight to the beach (route 2) and is turned away
    /// when the host stops welcoming bots is named to the room too, as one
    /// that joined the table is: it used to go with the door, unsaid.
    #[test]
    fn a_bot_at_the_door_sent_away_by_the_dial_is_named_too() {
        use crate::app::bot_seats::test_bots::{register, seated};
        let tr = &crate::app::i18n::EN;
        let mut bots = crate::app::bot_seats::BotSeats::default();
        let mut config = MatchConfig::default();
        let mut sent_away = Vec::new();
        host_the_bots(&mut bots, &config, tr, 0, &mut sent_away);
        let door = bots.door.as_mut().expect("the door opened");
        let string = door.open_string().expect("a chair waits");
        let _bot = register(&string, "Greedy", "Ana");
        seated(door, 0);

        config.bots_welcome = false;
        host_the_bots(&mut bots, &config, tr, 0, &mut sent_away);
        assert!(bots.door.is_none(), "the door shut");
        assert_eq!(sent_away, ["Greedy (Ana)"], "and the bot was named");
    }

    /// A bot already at the table when the host says bots are no longer
    /// welcome is sent away by name, for the room to be told: it used to
    /// vanish without a word.
    #[test]
    fn a_bot_sent_away_by_the_dial_is_named_to_the_room() {
        let mut state = LobbyState {
            standing: hosting_lobby(),
            ..LobbyState::default()
        };
        let port = state
            .hosted()
            .expect("hosting")
            .transport
            .local_addr()
            .expect("addr")
            .port();
        let bot = UdpTransport::join(("127.0.0.1", port)).expect("join");
        let on_air = |bots| crate::transport::OnAir {
            bots,
            full: false,
            ..crate::transport::OnAir::default()
        };
        bot.send(NetMsg::hello_bot("Greedy (Ana)"));
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let hosted = state.hosted_mut().expect("hosting");
            let picked = work_the_socket(hosted, 0.0, on_air(true));
            for (from, told) in picked.greeted {
                hosted.peers.row(from).name = told;
            }
            if hosted
                .peers
                .get(0)
                .is_some_and(|peer| !peer.name.is_empty())
            {
                break;
            }
        }
        // The host says not here; the bot greets again, as it does every
        // second.
        bot.send(NetMsg::hello_bot("Greedy (Ana)"));
        let mut sent_away = Vec::new();
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let hosted = state.hosted_mut().expect("hosting");
            sent_away.extend(work_the_socket(hosted, 0.0, on_air(false)).sent_away);
            if !sent_away.is_empty() {
                break;
            }
        }
        assert_eq!(sent_away, ["Greedy (Ana)"]);
        assert_eq!(state.hosted().expect("hosting").peers.len(), 0);
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
            standing: hosting_lobby(),
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
            standing: hosting_lobby(),
            ..LobbyState::default()
        };
        // What the room said before anybody new was listening.
        state.say("Ann", "anyone up for a round?");
        let bo = Notice::Joined {
            name: crate::transport::wire_name("Bo"),
            watching: false,
        };
        state.tell(&crate::app::i18n::EN, bo);
        // The host's own word of where the beach is, a notice too.
        let at = Notice::HostingAt {
            addr: "192.168.1.20:41234".parse().expect("addr"),
        };
        state.tell(&crate::app::i18n::EN, at);
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

        // Worded by the latecomer, whose game here is in French: the
        // room's notices come as what happened, not in the host's English.
        let fr = crate::app::i18n::Lang::Fr.tr();
        let mut heard = Vec::new();
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            for (msg, _) in latecomer.recv_all() {
                if let NetMsg::Chat { name, text } = msg {
                    heard.push((
                        crate::transport::name_from_wire(&name),
                        crate::transport::chat_from_wire(&text),
                    ));
                } else if let NetMsg::Notice(notice) = msg {
                    let line = crate::app::net::notice_line(fr, &notice, |seat| seat.to_string());
                    heard.push((String::new(), line));
                }
            }
            if heard.len() >= 4 {
                break;
            }
        }
        assert_eq!(
            heard,
            [
                ("Ann".to_string(), "anyone up for a round?".to_string()),
                (String::new(), fill(fr.lobby_joined, &[("p", "Bo")])),
                (
                    String::new(),
                    fill(fr.lobby_hosting_at, &[("a", "192.168.1.20:41234")])
                ),
                (String::new(), fill(fr.lobby_joined, &[("p", "Cy")])),
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
            standing: hosting_lobby(),
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

    /// A match launched from a lobby the table walked back to calls in a
    /// joiner still on the last match's results card: its first `Start` is
    /// dealt after every round of the match before. Numbered from one
    /// again, it read as a round that joiner had already been dealt, and
    /// the joiner sat out the new match while the host's table waited on
    /// it for good.
    #[test]
    fn a_new_match_calls_in_a_joiner_still_on_the_last_card() {
        use bevy::ecs::system::RunSystemOnce;

        let mut state = LobbyState {
            standing: hosting_lobby(),
            ..LobbyState::default()
        };
        let hosted = state.hosted_mut().expect("hosting");
        let port = hosted.transport.local_addr().expect("addr").port();
        let joiner = UdpTransport::join(("127.0.0.1", port)).expect("join");
        joiner.send(NetMsg::hello("Bo"));
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let _ = hosted.transport.recv_all();
            if hosted.transport.peer_count() == 1 {
                break;
            }
        }
        hosted.peers = peers_named(&["Bo"], &[]);
        // Three rounds of a best of five played on this socket, as the
        // match walking home hands them back (`back_to_the_lobby`).
        hosted.rounds_dealt = 3;
        // The joiner, still reading the scores of the third.
        let mut on_the_card = OnlineSession::new(
            joiner,
            crate::app::net::lockstep_for(Some(1), (0..2).collect(), crate::sim::DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        on_the_card.rounds_dealt = 3;

        let mut app = App::new();
        app.insert_resource(GameSettings::default());
        app.init_resource::<MatchConfig>();
        app.init_resource::<crate::app::match_setup::CustomBeaches>();
        app.init_resource::<Online>();
        app.init_resource::<crate::app::tournament::Tournament>();
        app.init_resource::<crate::app::bot_seats::BotSeats>();
        app.init_resource::<NextState<Screen>>();
        app.init_resource::<NextState<VersusPhase>>();
        app.insert_resource(state);
        app.world_mut()
            .run_system_once(
                |mut state: ResMut<LobbyState>,
                 settings: Res<GameSettings>,
                 config: Res<MatchConfig>,
                 beaches: Res<crate::app::match_setup::CustomBeaches>,
                 mut arena: IntoArena| {
                    launch_the_match(
                        &mut state, &settings, &config, &beaches, 1, &mut arena, None,
                    );
                },
            )
            .expect("the launch ran");

        let mut start = None;
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            start = on_the_card
                .transport
                .recv_all()
                .into_iter()
                .find_map(|(msg, _)| {
                    let NetMsg::Start(start) = msg else {
                        return None;
                    };
                    Some(start)
                });
            if start.is_some() {
                break;
            }
        }
        let start = start.expect("the launch sent the joiner its place");
        assert!(
            on_the_card.is_next_round(&start),
            "the new match's first round, dealt {}, is after the card's {}",
            start.deal,
            on_the_card.rounds_dealt
        );
        // And a stale answer from the match before is still refused.
        let stale = Invitation {
            deal: 3,
            ..start.clone()
        };
        assert!(!on_the_card.is_next_round(&stale));
    }
}
