//! The beach lobby: LAN matchmaking for online Turf War.
//!
//! Hosting binds an ephemeral game port and broadcasts it once a second;
//! every lobby on the network lists discovered hosts and joins one with a
//! number key. Pairing completes through the normal online handshake, then
//! both sides drop into versus. Direct-IP play (`PINCH_JOIN`) still works
//! for beyond-the-LAN matches. When the match ends, the whole table walks
//! back in through [`Homecoming`], sockets still connected, so the next
//! game is a keypress rather than a rediscovery.
//!
//! A beach comes off the list two ways. Normally it says so, on leaving
//! the lobby or on starting the match, since neither leaves anything
//! joinable behind. Failing that it simply stops being
//! heard, and ages off after [`HOST_TTL`]; nothing can be sent by a process
//! that was killed or a machine that lost power, so the timeout stays
//! underneath the farewell rather than being replaced by it.
//!
//! A host does not listen while it hosts: it cannot join anyone, and the
//! lobby port it would hold is one another instance on the same machine
//! might need.

mod discovery;
mod entry;
mod hosting;
mod joining;
mod terms;
mod ui;
pub use discovery::*;
pub use entry::*;
pub use hosting::*;
pub use joining::*;
pub use terms::*;
pub use ui::*;

use crate::app::cycle::{Cycle, Turn};
use crate::app::i18n::fill;
use crate::app::match_setup::MatchConfig;
use crate::app::net::{Invitation, LobbyReturn, Online, OnlineSession, PeerBook};
use crate::app::palette;
use crate::app::settings::GameSettings;
use crate::app::{Screen, VersusPhase};
use crate::sim::MAX_PLAYERS;
use crate::transport::{Announcer, Beacon, Discovery, MatchTerms, NetMsg, UdpTransport};
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;
use std::net::SocketAddr;

/// Also the cadence a running match keeps its beacon up at, which is why
/// it is not private: see `net::OnlineSession::keep_announcing`.
pub(crate) const ANNOUNCE_EVERY: f32 = 1.0;

/// Run a once-a-second clock down by `delta` and say whether it rang; a
/// clock that rang is wound back up to [`ANNOUNCE_EVERY`]. The beacon,
/// the joiner's greeting, and the running match's own copies of both keep
/// time this way, and each used to spell the three lines out for itself.
pub(crate) fn once_a_second(clock: &mut f32, delta: f32) -> bool {
    *clock -= delta;
    if *clock > 0.0 {
        return false;
    }
    *clock = ANNOUNCE_EVERY;
    true
}

/// What a joiner says to a host, every second until it is answered: a
/// `Watch` if it came to look, else a `Hello` carrying its name. The same
/// greeting from the lobby and from a results card, so the host reads the
/// wish to watch the same way wherever it was greeted from.
pub(crate) fn greeting(watching: bool, name: &str) -> NetMsg {
    match watching {
        true => NetMsg::Watch,
        false => NetMsg::hello(name),
    }
}

/// Where the player stands in the lobby, and everything that goes with
/// standing there.
///
/// Before this enum four separate fields said it (`hosting`, `joining`,
/// `watching`, and whether anything had been heard) and four different
/// pieces of the lobby each worked it out for themselves, each spelling
/// the same question differently. A combination nobody names is a
/// combination that drifts, and a field that only means anything in one
/// standing (the host's peers, the joiner's patience with a silent host)
/// is a field that can be read in the wrong one.
pub enum Standing {
    /// Choosing a beach, with W armed or not: armed, the next pick
    /// watches rather than plays.
    Choosing { watching: bool },
    /// Greeting a host, waiting to be given a seat.
    Joining(Joined),
    /// On the air, gathering a table.
    Hosting(Hosted),
}

impl Default for Standing {
    fn default() -> Self {
        Standing::Choosing { watching: false }
    }
}

impl Standing {
    /// Whether there is anybody at the other end: somebody to talk to, a
    /// table to show, and no list worth reading.
    pub fn at_a_beach(&self) -> bool {
        matches!(self, Standing::Joining(_) | Standing::Hosting(_))
    }

    /// A beach going on the air this frame.
    pub fn hosting(announcer: Announcer, transport: UdpTransport) -> Standing {
        Standing::Hosting(Hosted {
            announcer,
            transport,
            peers: PeerBook::default(),
            announce_in: 0.0,
        })
    }
}

/// A beach on the air: its beacon, the game socket gathering the table,
/// and everyone who has turned up on it.
pub struct Hosted {
    pub announcer: Announcer,
    pub transport: UdpTransport,
    /// Everyone on the socket, by its index there: the name its greeting
    /// carried, whether it asked to watch, and how long since it was last
    /// heard from. A joiner re-greets every second for as long as it sits
    /// in the lobby, so silence is the only evidence of leaving there is:
    /// UDP will never say so. Nobody holds a chair here; the plan is dealt
    /// at the launch.
    pub peers: PeerBook,
    announce_in: f32,
}

impl Hosted {
    /// Drop a peer from the socket and from the table, so whoever came
    /// after it moves up one in both.
    fn forget_peer(&mut self, peer: usize) {
        self.transport.forget(peer);
        self.peers.forget(peer);
    }

    /// Peers here to play, the ones who fill the seats. Capped at the five
    /// chairs beside the host's: seats run out before the socket does (it
    /// takes nine), and `seat_plan` turns the surplus into onlookers, so a
    /// table of eight would-be rivals is "5 aboard", not "8 aboard - Enter
    /// to start (up to 5)", and the beacon never reads "9/6".
    pub fn players_aboard(&self) -> usize {
        let seatable = MAX_PLAYERS - 1;
        self.peers
            .iter()
            .filter(|peer| !peer.watch)
            .count()
            .min(seatable)
    }

    /// Peers here to look rather than play.
    fn watchers_aboard(&self) -> usize {
        self.peers.iter().filter(|peer| peer.watch).count()
    }
}

/// A beach being greeted: the socket greeting it, and what has come back.
pub struct Joined {
    pub transport: UdpTransport,
    /// We asked to watch, so we greet with `Watch` and expect no seat back.
    pub watching: bool,
    hello_in: f32,
    /// Seconds since the host last said anything, and whether it has ever
    /// said anything at all.
    ///
    /// Dialling opens a socket, which on UDP proves nothing: there may be
    /// no machine at that address, or no game on it, or a firewall in the
    /// way, and all three look just like a host who has not started the
    /// round yet. These two are how the difference gets onto the screen.
    host_silence: f32,
    host_answered: bool,
    /// The seed of the round just played, set when a finished match walks
    /// back into this lobby still connected. The host may still be on its
    /// results card, where it re-answers every greeting with that round's
    /// `Start`; without this the lobby would read the repeat as an
    /// invitation and walk straight back into the finished round.
    played_seed: Option<u64>,
    /// The host's dials, from its [`NetMsg::Roster`]. The terms card
    /// paints these rather than this machine's own [`MatchConfig`], which
    /// is nothing to do with the match being joined; `None` until the
    /// first roster lands.
    pub terms: Option<MatchTerms>,
}

impl Joined {
    /// A beach dialled this frame: calling, not aboard, since nothing has
    /// answered yet and on UDP an open socket is no evidence that anything
    /// will.
    pub fn dialled(transport: UdpTransport, watching: bool) -> Joined {
        Joined {
            transport,
            watching,
            hello_in: ANNOUNCE_EVERY,
            host_silence: 0.0,
            host_answered: false,
            played_seed: None,
            terms: None,
        }
    }

    /// A beach walked back into from a finished round: the host was
    /// talking moments ago, so this is a beach, not an unanswered address,
    /// until the silence rule says otherwise, and it greets again at once.
    pub fn returned(transport: UdpTransport, watching: bool, played_seed: u64) -> Joined {
        Joined {
            transport,
            watching,
            hello_in: 0.0,
            host_silence: 0.0,
            host_answered: true,
            played_seed: Some(played_seed),
            terms: None,
        }
    }

    /// The seed of the round this table walked out of, if it did.
    pub fn played_seed(&self) -> Option<u64> {
        self.played_seed
    }
}

/// What the player was trying to do when they were asked to name
/// themselves first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Intent {
    Host,
    /// Take the beach on this row of the list.
    Join(usize),
    /// Take the beach at this address, which is on no row: it was typed
    /// out because no beacon from it ever arrived.
    Dial(SocketAddr),
}

/// One line in the feed, kept as who said it and what, rather than as the
/// finished string.
///
/// The two halves are painted differently, a name in its seat's colour
/// and a notice slanted, and a formatted `"who: line"` would have to be
/// picked apart again at the colon to do it, on a name the player chose
/// and may well have put a colon in.
#[derive(Default, Clone, PartialEq, Eq, Debug)]
pub struct Said {
    /// Who said it, or empty when the lobby itself is speaking: somebody
    /// arriving, somebody leaving.
    pub who: String,
    pub line: String,
}

impl Said {
    /// Whether this is the room talking rather than a player, which is
    /// what the slant on it says.
    pub fn is_notice(&self) -> bool {
        self.who.is_empty()
    }
}

#[derive(Resource, Default)]
pub struct LobbyState {
    discovery: Option<Discovery>,
    /// Where the player stands, and everything that goes with it.
    pub standing: Standing,
    pub hosts: Vec<HostEntry>,
    /// The beach under the cursor, remembered by address rather than by
    /// position. The list re-sorts as beaches come and go and as they fill
    /// up, so an index would silently point at a different game between the
    /// frame a player reads it and the frame they press Enter.
    pub selected: Option<SocketAddr>,
    /// First row shown, so a list longer than [`LIST_ROWS`] can scroll.
    pub scroll: usize,
    /// What has been said in this lobby, oldest first, capped at
    /// [`CHAT_LINES`]. Cleared with everything else on leaving.
    pub chat: Vec<Said>,
    /// What the keyboard is spelling out, if anything. While this is
    /// `Some`, every other lobby key is text.
    pub typing: Option<Typing>,
    /// Which dial the host has under the cursor.
    pub dial: usize,
    /// Who is at this beach, as this peer knows it: computed on the host,
    /// and told to a joiner by [`NetMsg::Roster`], since a joiner has only
    /// ever spoken to the host and would otherwise think itself alone.
    pub table: Vec<String>,
    /// What this beach is called, once its host has said. Announced in
    /// place of the host's own name: the list is choosing between games.
    /// Beside the standing rather than inside [`Hosted`] because it is
    /// asked for before the beach goes on the air, and kept after it comes
    /// off so the next one is pre-filled.
    pub game_name: String,
    pub feedback: String,
    auto_done: bool,
}

impl LobbyState {
    /// A line of chat as it came off the wire, into the feed: both halves
    /// tidied on the way, since a stranger on the LAN wrote them.
    pub fn hear(&mut self, name: &crate::transport::WireName, text: &crate::transport::WireChat) {
        let who = crate::transport::name_from_wire(name);
        let line = crate::transport::chat_from_wire(text);
        self.say(&who, &line);
    }

    /// Add a line to the feed, dropping the oldest to stay within
    /// [`CHAT_LINES`].
    pub fn say(&mut self, who: &str, line: &str) {
        if line.is_empty() {
            return;
        }
        self.chat.push(Said {
            who: who.to_string(),
            line: line.to_string(),
        });
        let over = self.chat.len().saturating_sub(CHAT_LINES);
        self.chat.drain(..over);
    }

    /// Where the player stands, the question most of the lobby is really
    /// asking.
    pub fn standing(&self) -> &Standing {
        &self.standing
    }

    /// Whether this lobby has anyone to say anything to.
    pub fn can_chat(&self) -> bool {
        self.standing.at_a_beach()
    }

    pub fn hosting(&self) -> bool {
        self.hosted().is_some()
    }

    /// The beach this lobby has on the air, if it is hosting one.
    pub fn hosted(&self) -> Option<&Hosted> {
        match &self.standing {
            Standing::Hosting(hosted) => Some(hosted),
            Standing::Choosing { .. } | Standing::Joining(_) => None,
        }
    }

    fn hosted_mut(&mut self) -> Option<&mut Hosted> {
        match &mut self.standing {
            Standing::Hosting(hosted) => Some(hosted),
            Standing::Choosing { .. } | Standing::Joining(_) => None,
        }
    }

    /// The beach this lobby is greeting, if it is greeting one.
    pub fn joined(&self) -> Option<&Joined> {
        match &self.standing {
            Standing::Joining(joined) => Some(joined),
            Standing::Choosing { .. } | Standing::Hosting(_) => None,
        }
    }

    fn joined_mut(&mut self) -> Option<&mut Joined> {
        match &mut self.standing {
            Standing::Joining(joined) => Some(joined),
            Standing::Choosing { .. } | Standing::Hosting(_) => None,
        }
    }

    /// Whoever is at the other end, host or table, to say something to.
    fn transport(&self) -> Option<&UdpTransport> {
        match &self.standing {
            Standing::Hosting(hosted) => Some(&hosted.transport),
            Standing::Joining(joined) => Some(&joined.transport),
            Standing::Choosing { .. } => None,
        }
    }

    /// Whether this player means to watch rather than play: W armed while
    /// choosing, or a beach greeted with `Watch`. A host plays.
    pub fn watching(&self) -> bool {
        match &self.standing {
            Standing::Choosing { watching } => *watching,
            Standing::Joining(joined) => joined.watching,
            Standing::Hosting(_) => false,
        }
    }

    /// Arm or disarm watching, wherever it can still be changed: while
    /// choosing, and on a greeting already in flight, which the dev hook
    /// sets the moment it joins.
    fn set_watching(&mut self, on: bool) {
        match &mut self.standing {
            Standing::Choosing { watching } => *watching = on,
            Standing::Joining(joined) => joined.watching = on,
            Standing::Hosting(_) => {}
        }
    }

    /// Back to choosing, keeping W armed as it was.
    fn let_go(&mut self) {
        let watching = self.watching();
        self.standing = Standing::Choosing { watching };
    }

    /// The table as the lobby knows it: the host first, then every peer
    /// that came to play, by the name its greeting carried.
    pub fn roster(&self, tr: &crate::app::i18n::Tr, me: &str) -> Vec<String> {
        let nobody = PeerBook::default();
        let peers = self.hosted().map_or(&nobody, |hosted| &hosted.peers);
        table_of(peers, tr, me)
    }

    /// Where the cursor sits in the current list, if the beach it names is
    /// still on the air.
    pub fn selected_index(&self) -> Option<usize> {
        let want = self.selected?;
        self.hosts.iter().position(|host| host.addr == want)
    }

    /// Keep the cursor on something real, and keep it on screen.
    ///
    /// Called after every refresh: beaches appear, fill up and go away
    /// while the list is being read, and a cursor pointing at one that has
    /// gone should fall to a neighbour rather than to whatever slid into
    /// its row.
    fn settle_cursor(&mut self) {
        if self.hosts.is_empty() {
            self.selected = None;
            self.scroll = 0;
            return;
        }
        let at = match self.selected_index() {
            Some(at) => at,
            None => {
                // The beach it named is gone: take the first, which under a
                // stable sort is a predictable place to land.
                self.selected = self.hosts.first().map(|host| host.addr);
                0
            }
        };
        // Scroll only as far as it must to keep the cursor in view.
        self.scroll = self.scroll.min(at).max(at.saturating_sub(LIST_ROWS - 1));
        self.scroll = self.scroll.min(self.hosts.len().saturating_sub(1));
    }

    /// Step the cursor by one, wrapping, and follow it with the window.
    fn step_cursor(&mut self, down: bool) {
        if self.hosts.is_empty() {
            return;
        }
        let len = self.hosts.len();
        let at = self.selected_index().unwrap_or(0);
        let next = match down {
            true => (at + 1) % len,
            false => (at + len - 1) % len,
        };
        self.selected = Some(self.hosts[next].addr);
        self.settle_cursor();
    }
}

/// The table as a host knows it: the host first, then every peer that
/// came to play, by the name its greeting carried. Onlookers are not at
/// it, and a peer that has not said what to call it yet still holds its
/// chair, under a seat label rather than a blank.
pub(super) fn table_of(peers: &PeerBook, tr: &crate::app::i18n::Tr, me: &str) -> Vec<String> {
    let mine = match me.trim().is_empty() {
        true => crate::app::seat_label(tr, 0),
        false => me.to_string(),
    };
    let mut table = vec![mine];
    for peer in peers.iter().filter(|peer| !peer.watch) {
        table.push(match peer.name.is_empty() {
            true => crate::app::seat_label(tr, table.len() as u8),
            false => peer.name.clone(),
        });
    }
    table
}

/// A finished match walking its table back into the lobby, still
/// connected: the results card's Enter fills this, the screen switches,
/// and [`enter_lobby`] - which otherwise starts from nothing - stands the
/// beach back up from it.
#[derive(Resource, Default)]
pub struct Homecoming(pub Option<LobbyReturn>);

/// Stand the beach back up from what the match handed back: the host goes
/// straight back on the air as open, a joiner back to its chair (or the
/// rail), both on the sockets the round was played over.
fn settle_back_in(
    state: &mut LobbyState,
    returned: LobbyReturn,
    tr: &'static crate::app::i18n::Tr,
) {
    let LobbyReturn {
        announcer,
        transport,
        game_name,
        mut peers,
        watching,
        host,
        played_seed,
    } = returned;
    if host {
        // A host that never announced (the direct pair) has no beach to
        // stand back up, and `from_lobby` keeps it from coming this way.
        let Some(announcer) = announcer else { return };
        // One row per peer the socket still has, no more and no fewer:
        // the rows are indexed by the socket's own list.
        peers.fit(transport.peer_count());
        peers.hush();
        state.game_name = game_name;
        // Back on the air this frame, as open: the round the beacon was
        // calling "running" is over.
        state.standing = Standing::Hosting(Hosted {
            announcer,
            transport,
            peers,
            announce_in: 0.0,
        });
    } else {
        state.standing = Standing::Joining(Joined::returned(transport, watching, played_seed));
        state.feedback = match watching {
            true => tr.lobby_watching,
            false => tr.lobby_aboard,
        }
        .to_string();
    }
}

pub fn enter_lobby(
    mut commands: Commands,
    mut state: ResMut<LobbyState>,
    mut homecoming: ResMut<Homecoming>,
    settings: Res<GameSettings>,
    art: Res<crate::app::art::Art>,
) {
    *state = LobbyState::default();
    spawn_lobby_ui(&mut commands, settings.tr(), &LobbyArt::from_art(&art));
    if let Some(returned) = homecoming.0.take() {
        settle_back_in(&mut state, returned, settings.tr());
    }
    // A host does not listen while it hosts (see the module doc), which a
    // table walking back in together arrives already doing.
    if state.hosting() {
        return;
    }
    match Discovery::bind() {
        Ok(discovery) => {
            state.discovery = Some(discovery);
            if state.feedback.is_empty() {
                state.feedback = settings.tr().lobby_listening.into();
            }
        }
        Err(e) => {
            if state.feedback.is_empty() {
                state.feedback = format!("discovery unavailable: {e}");
            }
        }
    }
}

pub fn exit_lobby(mut state: ResMut<LobbyState>) {
    say_goodbye(&state);
    *state = LobbyState::default();
}

/// Lobby input: host, join by number, leave, and say something.
#[allow(clippy::too_many_arguments)]
pub fn lobby_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    beaches: Res<crate::app::match_setup::CustomBeaches>,
    mut settings: ResMut<GameSettings>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    mut config: ResMut<MatchConfig>,
    mut state: ResMut<LobbyState>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    let tr = settings.tr();
    let tr: &'static crate::app::i18n::Tr = tr;
    // Set when an answer finishes and the thing it was blocking can now go
    // ahead in the same frame.
    let mut intent: Option<Intent> = None;
    // While anything is being typed the keyboard is text and nothing else:
    // H would host, W would arm watching, and a digit would join a beach,
    // none of which anyone means halfway through "wait for me".
    match drive_typing(&keys, &mut typed, &mut settings, &caps, &mut state, tr) {
        // The keyboard is busy: every other key in this lobby is text.
        Typed::Taken => return,
        // An answer finished and freed something to happen this frame.
        Typed::Unblocked(unblocked) => intent = Some(unblocked),
        Typed::Nothing => {}
    }
    // The reader must be drained even when nothing is being typed, or the
    // first line composed picks up every keystroke since the lobby opened.
    typed.clear();
    // T for talk, once there is anyone to talk to.
    if intent.is_none() && caps.just_pressed(&keys, 'T') && state.can_chat() {
        state.typing = Some(Typing::chat());
        return;
    }
    // J for a beach the list will never show: one whose beacons the
    // network drops, or that is a subnet away. Only while browsing, since
    // from a beach you are already at there is nothing to dial, and never
    // over a question already on screen, which a finished answer may have
    // opened earlier in this same frame.
    if intent.is_none()
        && keys.just_pressed(KeyCode::KeyJ)
        && !state.standing().at_a_beach()
        && state.typing.is_none()
    {
        state.typing = Some(Typing::address(&settings.last_beach));
        return;
    }
    let auto_host = crate::app::dev::auto_host_quota().is_some() && !state.auto_done;
    // PINCH_LOBBY_WATCH joins the first host found as a spectator, which is
    // the only way to watch: a session replays from frame zero, so an
    // onlooker has to be in the lobby with everyone else.
    let auto_watch = crate::app::dev::auto_watch();
    let auto_join =
        (crate::app::dev::auto_join() || auto_watch) && !state.auto_done && !state.hosts.is_empty();
    if auto_watch {
        state.set_watching(true);
    }
    let step = host_step(HostAsk {
        answered: intent == Some(Intent::Host),
        pressed_h: caps.just_pressed(&keys, 'H'),
        busy: state.standing().at_a_beach(),
        auto: auto_host,
    });
    if step == HostStep::Ask {
        // Stopped at the door until we know who is asking and what the
        // beach is to be called. Both are what the rest of the hall reads.
        state.typing = Some(Typing::player_name(Intent::Host, &settings.names[0]));
        return;
    }
    if step == HostStep::Go && !state.standing.at_a_beach() {
        state.auto_done = auto_host;
        match (
            Announcer::new(crate::app::clock::fresh_seed()),
            UdpTransport::host(0),
        ) {
            (Ok(announcer), Ok(transport)) => {
                state.feedback = match transport.local_addr() {
                    Ok(addr) => fill(tr.lobby_hosting, &[("p", &addr.port().to_string())]),
                    Err(_) => tr.lobby_hosting_noport.into(),
                };
                // Where this beach is, in the feed rather than the status
                // line: the status line is taken over by the rival count
                // the moment somebody arrives, and this is the number a
                // friend on a network that eats broadcasts has to be told
                // before they can dial it.
                if let Some(here) = address_here(&transport) {
                    let notice = fill(tr.lobby_hosting_at, &[("a", &here)]);
                    state.say("", &notice);
                }
                state.standing = Standing::hosting(announcer, transport);
                // Stop listening. A host cannot join anyone, so the list is
                // no use to it, and holding a lobby port it will never read
                // would deny one to another instance on the same machine,
                // which is why there are eight. It also stops
                // the host hearing its own beacon come back round the
                // loopback and listing itself.
                state.discovery = None;
                state.hosts.clear();
            }
            (Err(e), _) | (_, Err(e)) => {
                state.feedback = fill(tr.lobby_could_not_host, &[("e", &e.to_string())]);
            }
        }
    }
    if state.hosting() {
        turn_the_dials(
            &keys,
            &mut settings,
            &caps,
            &mut config,
            &mut state,
            &beaches,
        );
    }
    if !state.standing.at_a_beach() {
        walk_the_list(&keys, &mut state);
    }
    take_a_beach(&keys, &settings, &mut state, tr, intent, auto_join);
    if keys.just_pressed(KeyCode::KeyW)
        && let Standing::Choosing { watching } = &mut state.standing
    {
        *watching = !*watching;
    }
    if keys.just_pressed(KeyCode::Escape) {
        next_screen.set(Screen::Menu);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(last: u8) -> SocketAddr {
        format!("10.0.0.{last}:47777").parse().expect("addr")
    }

    /// A beach on the air, as `refresh_hosts` hears it.
    fn open(last: u8) -> (SocketAddr, Beacon) {
        beach(last, "", 1, 6, false)
    }

    /// The same, said in full.
    fn beach(last: u8, name: &str, taken: u8, seats: u8, running: bool) -> (SocketAddr, Beacon) {
        (
            addr(last),
            Beacon::Here {
                id: u64::from(last),
                name: name.to_string(),
                host: "Sam".to_string(),
                taken,
                seats,
                running,
            },
        )
    }

    /// A beach that keeps announcing stays on the list; one that goes quiet
    /// ages off it, so the join list only ever offers live hosts.
    #[test]
    fn hosts_expire_when_they_stop_announcing() {
        let mut hosts = Vec::new();
        refresh_hosts(&mut hosts, &[open(7)], 0.0);
        assert_eq!(hosts.len(), 1, "a new beach joins the list");

        // Heard again: still fresh, and not duplicated.
        refresh_hosts(&mut hosts, &[open(7)], 1.0);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].age, 0.0);

        // Silence, but not yet long enough.
        refresh_hosts(&mut hosts, &[], HOST_TTL - 0.5);
        assert_eq!(hosts.len(), 1, "given a moment's grace");

        // Silence past the deadline.
        refresh_hosts(&mut hosts, &[], 1.0);
        assert!(hosts.is_empty(), "a host that quit leaves the list");
    }

    /// A host that leaves properly is gone at once, rather than sitting in
    /// the list for the whole timeout offering a beach nobody can join. The
    /// ageing stays underneath it for the exits that cannot be announced:
    /// a killed process, a pulled cable.
    #[test]
    fn a_farewell_clears_a_host_without_waiting_out_the_timeout() {
        let mut hosts = Vec::new();
        refresh_hosts(&mut hosts, &[open(1), open(2)], 0.0);
        assert_eq!(hosts.len(), 2);

        // No time passes at all, and it is still gone.
        refresh_hosts(&mut hosts, &[(addr(1), Beacon::Closing { id: 1 })], 0.0);
        assert_eq!(hosts.len(), 1, "the one that said goodbye left");
        assert_eq!(hosts[0].addr, addr(2), "and only that one");

        // A farewell for a beach nobody had listed is simply nothing.
        refresh_hosts(&mut hosts, &[(addr(9), Beacon::Closing { id: 9 })], 0.0);
        assert_eq!(hosts.len(), 1);

        // Opened and closed inside one frame: the order it arrived in wins,
        // so a beach that is already gone is never briefly offered.
        refresh_hosts(
            &mut hosts,
            &[open(5), (addr(5), Beacon::Closing { id: 5 })],
            0.0,
        );
        assert!(
            hosts.iter().all(|host| host.addr != addr(5)),
            "a beach that opened and shut in one frame is not listed"
        );
        // And the same two the other way round is a beach that came back.
        refresh_hosts(
            &mut hosts,
            &[(addr(5), Beacon::Closing { id: 5 }), open(5)],
            0.0,
        );
        assert!(hosts.iter().any(|host| host.addr == addr(5)));
    }

    /// What a host reads while it waits. Pulled out of a two-hundred-line
    /// system so it can be read at all, and so this can check the counting:
    /// one rival is not "1 rivals", onlookers are named separately from
    /// players, and the AI is only ever offered the chairs left over.
    #[test]
    fn the_gathering_line_counts_who_is_actually_there() {
        use crate::app::i18n::EN;
        let none = MatchConfig {
            bots: 0,
            ..MatchConfig::default()
        };
        assert_eq!(gathering_feedback(&EN, &none, 1, 0), EN.lobby_rivals_one);
        assert!(gathering_feedback(&EN, &none, 3, 0).contains('3'));

        // Onlookers are counted, and counted apart from the players.
        let watched = gathering_feedback(&EN, &none, 2, 2);
        assert!(watched.len() > gathering_feedback(&EN, &none, 2, 0).len());

        // The AI takes what the table leaves and no more, however many the
        // host asked for. Five aboard plus the host is a full beach.
        let greedy = MatchConfig {
            bots: 5,
            ..MatchConfig::default()
        };
        let full = gathering_feedback(&EN, &greedy, MAX_PLAYERS - 1, 0);
        assert_eq!(
            full,
            gathering_feedback(&EN, &none, MAX_PLAYERS - 1, 0),
            "no room for a bot, so no promise of one"
        );
    }

    /// The other half of it: a hosting lobby really does put a farewell on
    /// the wire, and a lobby that never hosted has nothing to say. Both
    /// ways out of hosting, leaving the screen and starting the match, go
    /// through this one call.
    #[test]
    fn a_hosting_lobby_announces_its_own_departure() {
        let mut discovery = Discovery::bind().expect("bind lobby port");
        let mut state = LobbyState::default();
        // Beacons go to every lobby port on the machine, and the rest of the
        // suite is announcing while this runs, so "heard nothing at all" is
        // not a fact about this test. Take the game port first and judge
        // every packet by whether it names ours.
        let transport = UdpTransport::host(0).expect("game socket");
        let port = transport.local_addr().expect("addr").port();
        let mine = |heard: Vec<(std::net::SocketAddr, Beacon)>| {
            heard
                .into_iter()
                .filter(|(addr, _)| addr.port() == port)
                .collect::<Vec<_>>()
        };

        // Nothing hosted, nothing said.
        say_goodbye(&state);
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(
            mine(discovery.poll()).is_empty(),
            "a joiner has no beach to close"
        );

        state.standing = Standing::hosting(Announcer::new(0xB0A7).expect("announcer"), transport);
        say_goodbye(&state);
        let mut heard = Vec::new();
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            heard.extend(mine(discovery.poll()));
            if !heard.is_empty() {
                break;
            }
        }
        let (_, beacon) = heard.first().expect("the farewell went out");
        assert!(matches!(beacon, Beacon::Closing { .. }), "{beacon:?}");
    }

    /// Several beaches can be on the air at once, each aging on its own.
    #[test]
    fn hosts_age_independently() {
        let mut hosts = Vec::new();
        refresh_hosts(&mut hosts, &[open(1), open(2)], 0.0);
        assert_eq!(hosts.len(), 2);
        // Only the first keeps talking.
        refresh_hosts(&mut hosts, &[open(1)], HOST_TTL - 0.1);
        assert_eq!(hosts.len(), 2, "the quiet one is on its last legs");
        refresh_hosts(&mut hosts, &[open(1)], 0.2);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].addr, addr(1));
    }
}

#[cfg(test)]
mod homecoming_tests {
    use super::*;
    use crate::app::i18n::EN;
    use crate::app::net::PeerBook;

    use crate::app::lobby::hosting::peers_named;

    /// A hosting socket with `peers` joiners registered on it, as the one
    /// coming back from a match really is: the count is the socket's own,
    /// so a table walking home has to arrive with its peers still on it.
    /// The joiner sockets are handed back to be kept alive; dropping them
    /// would close the far end mid-test.
    fn with_peers(peers: usize) -> (UdpTransport, Vec<UdpTransport>) {
        let mut transport = UdpTransport::host(0).expect("game socket");
        let port = transport.local_addr().expect("addr").port();
        let mut joiners = Vec::new();
        for want in 1..=peers {
            let joiner = UdpTransport::join(("127.0.0.1", port)).expect("join");
            joiner.send(NetMsg::hello("someone"));
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                let _ = transport.recv_all();
                if transport.peer_count() >= want {
                    break;
                }
            }
            assert_eq!(transport.peer_count(), want, "peer {want} registered");
            joiners.push(joiner);
        }
        (transport, joiners)
    }

    /// A host walks back in still on the air: the same socket, the same
    /// beach name, the peers still counted, and the list it does not read
    /// while hosting left unbound.
    #[test]
    fn a_host_comes_home_still_hosting() {
        let (transport, _joiners) = with_peers(2);
        let port = transport.local_addr().expect("addr").port();
        let mut state = LobbyState::default();
        settle_back_in(
            &mut state,
            LobbyReturn {
                announcer: Some(Announcer::new(0xB0A7).expect("announcer")),
                transport,
                game_name: "Room 3".into(),
                peers: peers_named(&["Bo", "Cy"], &[1]),
                watching: false,
                host: true,
                played_seed: 42,
            },
            &EN,
        );
        assert!(state.hosting());
        assert_eq!(state.game_name, "Room 3");
        let hosted = state.hosted().expect("hosting");
        assert_eq!(
            hosted.transport.local_addr().expect("addr").port(),
            port,
            "the socket the table is still talking to"
        );
        assert_eq!(hosted.announce_in, 0.0, "back on the air this frame");
        assert_eq!(
            state.roster(&EN, "Anna"),
            ["Anna", "Bo"],
            "the table as it stood, minus the one at the rail"
        );
    }

    /// The rows a host is handed back have to be one per peer the socket
    /// still has, or a row and the peer it was kept for come apart the
    /// first time somebody leaves.
    #[test]
    fn a_short_name_list_is_padded_to_the_peers_it_indexes() {
        let (transport, _joiners) = with_peers(2);
        let mut state = LobbyState::default();
        settle_back_in(
            &mut state,
            LobbyReturn {
                announcer: Some(Announcer::new(1).expect("announcer")),
                transport,
                game_name: String::new(),
                // Fewer names than the socket knows peers.
                peers: peers_named(&["Bo"], &[0]),
                watching: false,
                host: true,
                played_seed: 0,
            },
            &EN,
        );
        let hosted = state.hosted().expect("hosting");
        assert_eq!(
            hosted.peers.len(),
            hosted.transport.peer_count(),
            "one row per peer the socket knows"
        );
    }

    /// A joiner walks back in already aboard: the socket it played over,
    /// the seed it played on, and no "calling..." for a host that was
    /// talking to it a moment ago.
    #[test]
    fn a_joiner_comes_home_aboard() {
        let mut state = LobbyState::default();
        settle_back_in(
            &mut state,
            LobbyReturn {
                announcer: None,
                transport: UdpTransport::join(("127.0.0.1", 47999)).expect("join"),
                game_name: String::new(),
                peers: PeerBook::default(),
                watching: true,
                host: false,
                played_seed: 42,
            },
            &EN,
        );
        let joined = state.joined().expect("joining");
        assert!(joined.watching, "still at the rail");
        assert_eq!(joined.played_seed(), Some(42));
        assert!(joined.host_answered, "the host was talking a moment ago");
        assert_eq!(state.feedback, EN.lobby_watching);
    }
}

#[cfg(test)]
mod chat_tests {
    use super::*;

    /// The feed keeps the last few lines and no more: a lobby is a place to
    /// say "ready?", not a place to scroll back through.
    #[test]
    fn the_feed_keeps_the_last_few_lines() {
        let mut state = LobbyState::default();
        for n in 0..CHAT_LINES + 4 {
            state.say("Anna", &format!("line {n}"));
        }
        assert_eq!(state.chat.len(), CHAT_LINES);
        assert_eq!(state.chat[0].line, "line 4", "{:?}", state.chat);
        assert_eq!(
            state.chat.last().unwrap().who,
            "Anna",
            "each line remembers who said it"
        );
        // Nothing said is nothing shown: an empty line is not a turn.
        state.say("Anna", "");
        assert_eq!(state.chat.len(), CHAT_LINES);
    }

    /// A notice is the lobby speaking. Nobody said it, and that is the
    /// whole of what tells the feed to slant it.
    #[test]
    fn a_notice_has_nobody_behind_it() {
        let mut state = LobbyState::default();
        state.say("", "Anna joined");
        state.say("Anna", "ready?");
        assert!(state.chat[0].is_notice(), "nobody said it");
        assert!(!state.chat[1].is_notice(), "Anna did");
    }

    /// Chat needs someone to chat with. Alone in the lobby, watching the
    /// list, there is nobody on the other end and the key does nothing.
    #[test]
    fn there_is_nobody_to_talk_to_until_there_is() {
        let mut state = LobbyState::default();
        assert!(!state.can_chat(), "browsing the list is not a conversation");
        state.standing = Standing::hosting(
            Announcer::new(0xB0A7).expect("announcer"),
            UdpTransport::host(0).expect("socket"),
        );
        assert!(state.can_chat(), "a host has a table to address");
        state.standing = Standing::Joining(Joined::dialled(
            UdpTransport::join(("127.0.0.1", 47999)).expect("join"),
            false,
        ));
        assert!(state.can_chat(), "and a joiner has a host");
    }
}

#[cfg(test)]
mod table_tests {
    use super::*;
    use crate::app::i18n::EN;
    use crate::app::lobby::hosting::peers_named;

    /// A hosting lobby with these peers on it, as greeted.
    fn seated(names: &[&str], watchers: &[usize]) -> LobbyState {
        let mut state = LobbyState {
            standing: Standing::hosting(
                Announcer::new(0xB0A7).expect("announcer"),
                UdpTransport::host(0).expect("socket"),
            ),
            ..LobbyState::default()
        };
        state.hosted_mut().expect("hosting").peers = peers_named(names, watchers);
        state
    }

    /// The table is the host and everyone who came to play, in peer order.
    /// Onlookers are not at it, they are watching it, and a peer that has
    /// not said what to call it yet still holds its chair.
    #[test]
    fn the_table_is_the_host_and_the_players() {
        let state = seated(&["Bo", "Cy"], &[]);
        assert_eq!(state.roster(&EN, "Anna"), ["Anna", "Bo", "Cy"]);

        // Peer 0 came to watch, so the table is the host and Cy.
        let state = seated(&["Bo", "Cy"], &[0]);
        assert_eq!(state.roster(&EN, "Anna"), ["Anna", "Cy"]);

        // A nameless host, and a nameless peer, fall back to seat labels
        // rather than to blanks: a row that says nothing is worse than a
        // row that says "P2".
        let state = seated(&[""], &[]);
        let table = state.roster(&EN, "");
        assert_eq!(table.len(), 2);
        assert!(table.iter().all(|who| !who.is_empty()), "{table:?}");

        // And a lobby that is not hosting seats nobody but its owner.
        let state = LobbyState::default();
        assert_eq!(state.roster(&EN, "Anna"), ["Anna"]);
    }

    /// Dropping a peer has to move the rows behind it with the socket's
    /// own list, or the row after it silently takes its name and its
    /// wish to watch. This is the failure that would be invisible until
    /// somebody found themselves labelled as somebody else.
    #[test]
    fn forgetting_a_peer_moves_everything_that_indexed_it() {
        let mut state = seated(&["Bo", "Cy", "Dee"], &[2]);
        assert_eq!(
            state.roster(&EN, "Anna"),
            ["Anna", "Bo", "Cy"],
            "Dee watches"
        );

        // Bo leaves. Cy and Dee shift down, and Dee is still the watcher.
        state.hosted_mut().expect("hosting").forget_peer(0);
        let hosted = state.hosted().expect("hosting");
        let names: Vec<&str> = hosted.peers.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Cy", "Dee"]);
        assert_eq!(hosted.watchers_aboard(), 1, "the wish followed Dee down");
        assert_eq!(hosted.players_aboard(), 1);
        assert_eq!(
            state.roster(&EN, "Anna"),
            ["Anna", "Cy"],
            "Dee still watches"
        );

        // The watcher itself leaves, and takes only its own wish.
        state.hosted_mut().expect("hosting").forget_peer(1);
        let hosted = state.hosted().expect("hosting");
        assert_eq!(hosted.peers.len(), 1);
        assert_eq!(hosted.watchers_aboard(), 0);
        assert_eq!(state.roster(&EN, "Anna"), ["Anna", "Cy"]);
    }
}

#[cfg(test)]
mod standing_tests {
    use super::*;

    /// Where the player stands is one value, and the wish to watch rides
    /// with it: armed while choosing, carried onto the greeting, and
    /// meaningless to a host, who plays.
    #[test]
    fn the_standing_carries_the_wish_to_watch_with_it() {
        let mut state = LobbyState::default();
        assert!(!state.standing().at_a_beach(), "nobody to talk to yet");
        assert!(!state.watching());

        state.set_watching(true);
        assert!(state.watching());
        assert!(!state.standing().at_a_beach());

        let watching = state.watching();
        state.standing = Standing::Joining(Joined::dialled(
            UdpTransport::join(("127.0.0.1", 47999)).expect("join"),
            watching,
        ));
        assert!(state.joined().is_some());
        assert!(state.watching(), "the wish went with the greeting");
        assert!(state.standing().at_a_beach());

        // Letting the beach go keeps W armed as it was.
        state.let_go();
        assert!(state.joined().is_none());
        assert!(state.watching());

        state.standing = Standing::hosting(
            Announcer::new(1).expect("announcer"),
            UdpTransport::host(0).expect("socket"),
        );
        assert!(state.hosting());
        assert!(!state.watching(), "a host plays");
        assert!(state.standing().at_a_beach());
    }

    /// The whole point of naming it: four places asked this question and
    /// each spelled it differently. They must not drift apart again.
    #[test]
    fn everything_that_asks_gets_the_same_answer() {
        let mut state = LobbyState::default();
        for watching in [false, true] {
            state.set_watching(watching);
            assert_eq!(state.can_chat(), state.standing().at_a_beach());
        }
        state.standing = Standing::Joining(Joined::dialled(
            UdpTransport::join(("127.0.0.1", 47998)).expect("join"),
            false,
        ));
        assert_eq!(state.can_chat(), state.standing().at_a_beach());
    }
}
