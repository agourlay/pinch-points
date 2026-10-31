//! The beach lobby: LAN matchmaking for online Turf War.
//!
//! Hosting binds an ephemeral game port and broadcasts it once a second;
//! every lobby on the network lists discovered hosts and joins the one under
//! the cursor with Enter (a number key points at a row). Pairing completes through the normal online handshake, then
//! both sides drop into versus. Direct-IP play (`PINCH_JOIN`) still works
//! for beyond-the-LAN matches. When the match ends, the whole table walks
//! back in through [`Homecoming`], sockets still connected, so the next
//! game is a keypress rather than a rediscovery.
//!
//! A beach comes off the list two ways. Normally it says so, on leaving
//! the lobby or on starting the match. Failing that it simply stops being
//! heard, and ages off after [`HOST_TTL`]: nothing can be sent by a process
//! that was killed or a machine that lost power, so the timeout stays
//! underneath the farewell rather than being replaced by it.
//!
//! A host does not listen while it hosts: it cannot join anyone, and the
//! lobby port it would hold is one another instance on the same machine
//! might need.

mod discovery;
mod entry;
mod hosting;
mod input;
mod joining;
mod kick;
#[cfg(test)]
mod load;
mod partners;
mod pick;
#[cfg(test)]
mod pick_tests;
mod standing;
mod terms;
mod ui;
pub use discovery::*;
pub use entry::*;
pub use hosting::*;
pub use input::*;
pub use joining::*;
pub use kick::*;
pub use partners::{Seated, partner_rows, partners_open, seats_shown};
pub use pick::Picking;
pub use standing::*;
pub use terms::*;
pub use ui::*;

use crate::app::cycle::{Cycle, Turn};
use crate::app::i18n::fill;
use crate::app::match_setup::MatchConfig;
use crate::app::menu_ui::Nav;
use crate::app::net::{
    HostedBot, Invitation, LobbyReturn, Online, OnlineSession, PeerBook, PutOut,
};
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
/// clock that rang is wound back up to [`ANNOUNCE_EVERY`]. The beacon, the
/// joiner's greeting, and the running match's own copies of both keep time
/// this way.
pub(crate) fn once_a_second(clock: &mut f32, delta: f32) -> bool {
    *clock -= delta;
    if *clock > 0.0 {
        return false;
    }
    *clock = ANNOUNCE_EVERY;
    true
}

/// What the player was trying to do when they were asked to name
/// themselves first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Intent {
    Host,
    /// Take the beach that was on the cursor's row when the name was
    /// asked, by its address: the list re-sorts by name every frame, so a
    /// row number held while a name is typed can name another beach by
    /// the time Enter is pressed.
    Join(SocketAddr),
    /// Take the beach at this address, which is on no row: it was typed
    /// out because no beacon from it ever arrived.
    Dial(SocketAddr),
    /// Take that beach as a bot (route 1): a doorway first. By address,
    /// as `Join` is.
    JoinBot(SocketAddr),
}

/// One line in the feed, kept as who said it and what, rather than as the
/// finished string.
///
/// The two halves are painted differently, a name in its seat's colour
/// and a notice slanted, and a formatted `"who: line"` would have to be
/// picked apart at a colon the player may well have typed themselves.
#[derive(Default, Clone, PartialEq, Eq, Debug)]
pub struct Said {
    /// Who said it, or empty when the lobby itself is speaking: somebody
    /// arriving, somebody leaving.
    pub who: String,
    pub line: String,
    /// What the room said happened, for a room notice: the line is this
    /// machine's wording of it, and a host catching a newcomer up on the
    /// feed sends the notice again for the newcomer to word in its own
    /// language ([`crate::app::net::notice_line`]).
    pub notice: Option<crate::transport::Notice>,
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
    /// First row shown, so a list longer than its card can scroll.
    pub scroll: usize,
    /// How many rows the list shows: as many as its card has room for.
    rows: ListRows,
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
    pub table: Vec<TablePlace>,
    /// The seat each of [`Self::table`]'s places sits in, the AI behind
    /// them and how the table is scored ([`Seated`]): the host's own
    /// partner picks on the host, the roster's terms on a joiner.
    pub seated: Seated,
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
    /// Everything back to a lobby just walked into, but the beach name
    /// last given: kept so the next beach is pre-filled with it. Reset
    /// whole on the way in and out, the pre-fill its doc promised never
    /// survived to be offered.
    fn start_over(&mut self) {
        let game_name = std::mem::take(&mut self.game_name);
        *self = LobbyState {
            game_name,
            ..LobbyState::default()
        };
    }

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
        self.note(Said {
            who: who.to_string(),
            line: line.to_string(),
            notice: None,
        });
    }

    /// Put a room notice in the feed, worded in this machine's language.
    /// The lobby has no seats yet, so a seat a notice names is its label.
    pub fn tell(&mut self, tr: &crate::app::i18n::Tr, notice: crate::transport::Notice) {
        let label = |seat: u8| crate::app::seat_label(tr, seat);
        self.note(Said {
            who: String::new(),
            line: crate::app::net::notice_line(tr, &notice, label),
            notice: Some(notice),
        });
    }

    /// Add a line to the feed, dropping the oldest to stay within
    /// [`CHAT_LINES`].
    fn note(&mut self, said: Said) {
        if said.line.is_empty() {
            return;
        }
        self.chat.push(said);
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
    ///
    /// The beach's conversation and its table go with it. Left behind,
    /// a player put out of one beach (kicked, unanswered, on another
    /// build) carried its chat into the next one they joined, and a host
    /// sends its chat to every arrival, so hosting next handed the old
    /// beach's talk, kick notice and all, to strangers.
    fn let_go(&mut self) {
        let watching = self.watching();
        self.standing = Standing::Choosing { watching };
        self.chat.clear();
        self.table.clear();
    }

    /// The table as the lobby knows it: the host first, then every peer
    /// that came to play, by the name its greeting carried.
    pub fn roster(&self, tr: &crate::app::i18n::Tr, me: &str) -> Vec<TablePlace> {
        let nobody = PeerBook::default();
        let peers = self.hosted().map_or(&nobody, |hosted| &hosted.peers);
        table_of(peers, tr, me)
    }

    /// The beach this joiner is at, as its beacon described it: by the
    /// address it dialled, not the cursor, which a typed address never moved
    /// and a number key moves at will.
    pub fn joined_entry(&self) -> Option<&HostEntry> {
        let joined = self.joined()?;
        match joined.beach {
            Some(id) => self.hosts.iter().find(|host| host.id == id),
            None => {
                let there = joined.transport.peer_addr()?;
                self.hosts.iter().find(|host| host.addr == there)
            }
        }
    }

    /// What the beach under the cursor is called, for a card that is about
    /// to join it: its name, or its address when it gave none.
    pub fn joining_beach(&self) -> Option<String> {
        let host = &self.hosts[self.selected_index()?];
        Some(match host.name.is_empty() {
            true => host.addr.to_string(),
            false => host.name.clone(),
        })
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
        // Scroll only as far as it must to keep the cursor in view, and
        // never past a list that got shorter: a window opened below its
        // last full screen hides rows above and paints blanks below.
        let rows = self.rows_shown();
        self.scroll = self
            .scroll
            .min(at)
            .max(at.saturating_sub(rows - 1))
            .min(self.hosts.len().saturating_sub(rows));
    }

    /// How many rows of the list are in view. Everything that reads the
    /// window reads it here: the painter, the scroll bar and the cursor.
    pub fn rows_shown(&self) -> usize {
        self.rows.0
    }

    /// The beach in row `row` of the window, if that row shows one: not
    /// past the rows in view, and none while standing at a beach, where
    /// the list is not up.
    pub fn listed(&self, row: usize) -> Option<&HostEntry> {
        if self.standing().at_a_beach() || row >= self.rows_shown() {
            return None;
        }
        self.hosts.get(self.scroll + row)
    }

    /// Show `rows` rows from now on, the card having been resized. A window
    /// that grew takes in rows from above rather than showing blanks below
    /// the last beach, and the cursor stays in view either way.
    pub fn fit_rows(&mut self, rows: usize) {
        let rows = rows.max(1);
        if rows > self.rows.0 {
            self.scroll = self.scroll.min(self.hosts.len().saturating_sub(rows));
        }
        self.rows = ListRows(rows);
        self.settle_cursor();
    }

    /// Put the cursor on row `at` of the list as numbered on screen, from
    /// zero, if there is such a row. The rows are numbered from the top of
    /// the list rather than of the window, so the number beside a name is
    /// the one that points at it.
    pub(crate) fn point_at(&mut self, at: Option<usize>) {
        let Some(host) = at.and_then(|at| self.hosts.get(at)) else {
            return;
        };
        self.selected = Some(host.addr);
        self.settle_cursor();
    }

    /// Step the cursor, wrapping, and follow it with the window.
    fn step_cursor(&mut self, nav: Nav) {
        if self.hosts.is_empty() {
            return;
        }
        let len = self.hosts.len();
        let at = self.selected_index().unwrap_or(0);
        let next = crate::app::menu_ui::step(nav, at, len);
        self.selected = Some(self.hosts[next].addr);
        self.settle_cursor();
    }
}

/// One place at the lobby's table: who holds it, and what holds it, so a
/// bot at the beach wears the robot on the card that lists who is there.
/// One list of both, so a place and what holds it never count past each
/// other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TablePlace {
    pub name: String,
    pub kind: crate::sim::SeatKind,
}

/// The table as a host knows it: the host first, a person, then every
/// peer that came to play, by the name its greeting carried, as far as
/// there are chairs ([`PeerBook::at_the_table`]), each a bot or a person
/// as its greeting said. Onlookers are not at it, and a peer that has not
/// said what to call it yet still holds its chair, under a seat label
/// rather than a blank.
pub(super) fn table_of(peers: &PeerBook, tr: &crate::app::i18n::Tr, me: &str) -> Vec<TablePlace> {
    let mine = match me.trim().is_empty() {
        true => crate::app::seat_label(tr, 0),
        false => me.to_string(),
    };
    let mut table = vec![TablePlace {
        name: mine,
        kind: crate::sim::SeatKind::Human,
    }];
    for (_, peer) in peers.at_the_table() {
        table.push(TablePlace {
            name: match peer.name.is_empty() {
                true => crate::app::seat_label(tr, table.len() as u8),
                false => peer.name.clone(),
            },
            kind: match peer.bot {
                true => crate::sim::SeatKind::Bot,
                false => crate::sim::SeatKind::Human,
            },
        });
    }
    table
}

/// The names of a table's places, in order, for a test to compare.
#[cfg(test)]
pub(super) fn place_names(table: &[TablePlace]) -> Vec<&str> {
    table.iter().map(|place| place.name.as_str()).collect()
}

/// A finished match walking its table back into the lobby, still
/// connected: the results card's Enter fills this, the screen switches,
/// and [`enter_lobby`] - which otherwise starts from nothing - stands the
/// beach back up from it.
#[derive(Resource, Default)]
pub struct Homecoming(pub Option<LobbyReturn>);

/// Stand the beach back up from what the match handed back: the host goes
/// straight back on the air as open, a joiner back to its chair (or the
/// watching), both on the sockets the round was played over.
fn settle_back_in(
    state: &mut LobbyState,
    returned: LobbyReturn,
    tr: &'static crate::app::i18n::Tr,
) {
    match returned {
        LobbyReturn::Host {
            announcer,
            transport,
            game_name,
            mut peers,
            played,
            rounds_dealt,
            arranged,
        } => {
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
                kicking: None,
                bot_string: None,
                tail: RoundTail::kept(played, rounds_dealt),
                rounds_dealt,
                picking: None,
                partnering: false,
                arranged,
            });
        }
        LobbyReturn::Joiner {
            transport,
            watching,
            played_seed,
            bot,
        } => {
            state.standing =
                Standing::Joining(Joined::returned(transport, watching, played_seed, bot));
            state.feedback = match watching {
                true => tr.lobby_watching,
                false => tr.lobby_aboard,
            }
            .to_string();
        }
    }
}

pub fn enter_lobby(
    mut commands: Commands,
    mut state: ResMut<LobbyState>,
    mut homecoming: ResMut<Homecoming>,
    mut notice: ResMut<crate::app::RoundNotice>,
    settings: Res<GameSettings>,
    art: Res<crate::app::art::Art>,
) {
    state.start_over();
    spawn_lobby_ui(&mut commands, settings.tr(), &LobbyArt::from_art(&art));
    if let Some(returned) = homecoming.0.take() {
        settle_back_in(&mut state, returned, settings.tr());
    }
    // Word of the round just left, for somebody the round put out rather
    // than let go: the host walked off, or this peer was dropped. The
    // menu's own slot for it is no use to a player who was never sent
    // there, and taken rather than read, so it is not still hanging about
    // the next time the menu comes up.
    let why = std::mem::take(&mut notice.0);
    if !why.is_empty() {
        // The status line, and not the feed: the feed panel is not even
        // on screen while a player is browsing, and a line about the
        // beach they were thrown out of would surface inside the next
        // beach's conversation. Nothing on the browsing path writes this
        // slot, so the reason stays put until they pick something.
        state.feedback = why;
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
                state.feedback = fill(
                    settings.tr().lobby_discovery_unavailable,
                    &[("e", &e.to_string())],
                );
            }
        }
    }
}

pub fn exit_lobby(
    mut state: ResMut<LobbyState>,
    dials: crate::app::match_setup::Dials,
    mut menu: ResMut<crate::app::match_setup::MatchMenu>,
) {
    say_goodbye(&state);
    // A map page still up goes with the lobby, the setup put back as it
    // was. The launch takes it down itself, keeping the pick.
    let crate::app::match_setup::Dials {
        mut config,
        beaches,
    } = dials;
    if let Some(picking) = state.hosted_mut().and_then(|hosted| hosted.picking.take()) {
        picking.page.close(
            crate::app::match_setup::Close::PutBack,
            &mut config,
            &mut menu,
            &beaches,
        );
    }
    state.start_over();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A player the round put out walks into the lobby with the reason on
    /// its status line, taken so the menu does not say it again; a host
    /// walking its table back is hosting again, and does not listen for
    /// beaches while it hosts.
    #[test]
    fn the_lobby_says_why_a_round_put_this_player_out() {
        use bevy::ecs::system::RunSystemOnce;
        let lobby = |homecoming: Option<LobbyReturn>| {
            let mut app = App::new();
            app.init_resource::<LobbyState>();
            app.insert_resource(Homecoming(homecoming));
            app.insert_resource(crate::app::RoundNotice("The host left.".into()));
            app.init_resource::<GameSettings>();
            app.insert_resource(crate::app::art::Art::blank());
            app.world_mut()
                .run_system_once(enter_lobby)
                .expect("entered");
            app
        };
        let app = lobby(None);
        let state = app.world().resource::<LobbyState>();
        assert_eq!(state.feedback, "The host left.");
        assert!(
            app.world()
                .resource::<crate::app::RoundNotice>()
                .0
                .is_empty()
        );

        let mut session = crate::app::net::OnlineSession::new(
            UdpTransport::host(0).expect("game socket"),
            crate::sim::Lockstep::new(0, vec![0, 1], crate::sim::DEFAULT_DELAY),
            2,
            crate::transport::MatchTerms::default(),
        );
        session
            .stay_on_air(Announcer::on(0x9A9B, crate::transport::Air::loopback(&[])).expect("air"));
        session.home.from_lobby = true;
        let app = lobby(Some(session.back_to_the_lobby()));
        let state = app.world().resource::<LobbyState>();
        assert!(state.hosting());
        assert!(state.discovery.is_none(), "a host listening for beaches");
    }

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
                bots: false,
                full: false,
            },
        )
    }

    /// A beach that keeps announcing stays on the list; one that goes quiet
    /// ages off it, so the join list only ever offers live hosts.
    #[test]
    fn hosts_expire_when_they_stop_announcing() {
        let mut hosts = Vec::new();
        refresh_open(&mut hosts, &[open(7)], 0.0);
        assert_eq!(hosts.len(), 1, "a new beach joins the list");

        // Heard again: still fresh, and not duplicated.
        refresh_open(&mut hosts, &[open(7)], 1.0);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].age, 0.0);

        // Silence, but not yet long enough.
        refresh_open(&mut hosts, &[], HOST_TTL - 0.5);
        assert_eq!(hosts.len(), 1, "given a moment's grace");

        // Silence past the deadline.
        refresh_open(&mut hosts, &[], 1.0);
        assert!(hosts.is_empty(), "a host that quit leaves the list");
    }

    /// A host that leaves properly is gone at once, rather than sitting in
    /// the list for the whole timeout. The ageing stays underneath for the
    /// exits that cannot be announced: a killed process, a pulled cable.
    #[test]
    fn a_farewell_clears_a_host_without_waiting_out_the_timeout() {
        let mut hosts = Vec::new();
        refresh_open(&mut hosts, &[open(1), open(2)], 0.0);
        assert_eq!(hosts.len(), 2);

        // No time passes at all, and it is still gone.
        refresh_open(&mut hosts, &[(addr(1), Beacon::Closing { id: 1 })], 0.0);
        assert_eq!(hosts.len(), 1, "the one that said goodbye left");
        assert_eq!(hosts[0].addr, addr(2), "and only that one");

        // A farewell for a beach nobody had listed is simply nothing.
        refresh_open(&mut hosts, &[(addr(9), Beacon::Closing { id: 9 })], 0.0);
        assert_eq!(hosts.len(), 1);

        // Opened and closed inside one frame: the order it arrived in wins,
        // so a beach that is already gone is never briefly offered.
        refresh_open(
            &mut hosts,
            &[open(5), (addr(5), Beacon::Closing { id: 5 })],
            0.0,
        );
        assert!(
            hosts.iter().all(|host| host.addr != addr(5)),
            "a beach that opened and shut in one frame is not listed"
        );
        // And the same two the other way round is a beach that came back.
        refresh_open(
            &mut hosts,
            &[(addr(5), Beacon::Closing { id: 5 }), open(5)],
            0.0,
        );
        assert!(hosts.iter().any(|host| host.addr == addr(5)));
    }

    /// What a host reads while it waits: one rival is not "1 rivals",
    /// onlookers are named separately from players, and the AI is only
    /// ever offered the chairs left over.
    #[test]
    fn the_gathering_line_counts_who_is_actually_there() {
        use crate::app::i18n::EN;
        let none = MatchConfig {
            lobby_bots: 0,
            ..MatchConfig::default()
        };
        // The AI the launch deals behind the host and `aboard`.
        let ai = |config: &MatchConfig, aboard: usize| dealt(config, 1 + aboard as u8).bots;
        let line = |config: &MatchConfig, aboard: usize, watching: usize| {
            gathering_feedback(&EN, aboard, watching, ai(config, aboard), EN.key_enter)
        };
        let enter = [("k", EN.key_enter)];
        assert_eq!(line(&none, 1, 0), fill(EN.lobby_rivals_one, &enter));
        // Only watchers aboard: Enter starts against the AI, and the line
        // says no player came rather than counting zero of them.
        let onlookers = line(&none, 0, 1);
        assert!(
            onlookers.starts_with(&fill(EN.lobby_rivals_none, &enter)),
            "{onlookers}"
        );
        assert!(
            onlookers.contains(&fill(EN.lobby_ai_seats, &[("n", "1")])),
            "{onlookers}"
        );
        assert!(line(&none, 3, 0).contains('3'));

        // Onlookers are counted, and counted apart from the players.
        let watched = line(&none, 2, 2);
        assert!(watched.len() > line(&none, 2, 0).len());

        // The AI takes what the table leaves and no more, however many the
        // host asked for. Five aboard plus the host is a full beach.
        let greedy = MatchConfig {
            lobby_bots: 5,
            ..MatchConfig::default()
        };
        let full = line(&greedy, MAX_PLAYERS - 1, 0);
        assert_eq!(
            full,
            line(&none, MAX_PLAYERS - 1, 0),
            "no room for a bot, so no promise of one"
        );
    }

    /// A hosting lobby really does put a farewell on the wire, and a lobby
    /// that never hosted has nothing to say. Both ways out of hosting,
    /// leaving the screen and starting the match, go through this call.
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
        let mine = |heard: Vec<(SocketAddr, Beacon)>| {
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
        refresh_open(&mut hosts, &[open(1), open(2)], 0.0);
        assert_eq!(hosts.len(), 2);
        // Only the first keeps talking.
        refresh_open(&mut hosts, &[open(1)], HOST_TTL - 0.1);
        assert_eq!(hosts.len(), 2, "the quiet one is on its last legs");
        refresh_open(&mut hosts, &[open(1)], 0.2);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].addr, addr(1));
    }
}

#[cfg(test)]
mod homecoming_tests {
    use super::*;
    use crate::app::i18n::EN;
    use crate::sim::{DEFAULT_DELAY, Lockstep};

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

    /// Leaving the lobby and coming back starts it over, but keeps the
    /// beach name last given, which the next beach is pre-filled with.
    #[test]
    fn the_beach_name_outlives_the_lobby() {
        let mut state = LobbyState {
            game_name: "Room 3".into(),
            feedback: "something".into(),
            dial: 4,
            ..LobbyState::default()
        };
        state.say("Ana", "hi");
        state.start_over();
        assert_eq!(state.game_name, "Room 3");
        assert!(state.chat.is_empty());
        assert!(state.feedback.is_empty());
        assert_eq!(state.dial, 0);
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
            LobbyReturn::Host {
                announcer: Some(Announcer::new(0xB0A7).expect("announcer")),
                transport,
                game_name: "Room 3".into(),
                peers: peers_named(&["Bo", "Cy"], &[1]),
                played: Box::new(Lockstep::new(0, vec![0, 1], DEFAULT_DELAY)),
                rounds_dealt: 3,
                arranged: None,
            },
            &EN,
        );
        assert!(state.hosting());
        assert_eq!(state.hosted().map(|hosted| hosted.rounds_dealt), Some(3));
        assert_eq!(state.game_name, "Room 3");
        let hosted = state.hosted().expect("hosting");
        assert_eq!(
            hosted.transport.local_addr().expect("addr").port(),
            port,
            "the socket the table is still talking to"
        );
        assert_eq!(hosted.announce_in, 0.0, "back on the air this frame");
        assert_eq!(
            place_names(&state.roster(&EN, "Anna")),
            ["Anna", "Bo"],
            "the table as it stood, minus the one watching"
        );
    }

    /// A host back in the lobby keeps repeating the end of the round it
    /// walked away from, for a while: the last frames to everybody on the
    /// beacon's tick, at once to a peer still in the round whatever it is
    /// missing, a spectator far behind included, and to nobody once the
    /// while is up. A batch of another round asks for nothing.
    #[test]
    fn the_lobby_repeats_the_rounds_end_for_a_while() {
        use crate::sim::{InputMsg, PlayerAction};
        use crate::transport::Batch;
        let (transport, mut joiners) = with_peers(2);
        // A round of 300 frames, the host's and Bo's, played out.
        let mut played = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        while played.frame() < 300 {
            if let Some(ours) = played.commit_local(PlayerAction::None) {
                played.receive(InputMsg { player: 1, ..ours });
            }
            while played.due() && played.advance().is_some() {}
        }
        let rounds_dealt = 4;
        let round = crate::transport::round_tag(rounds_dealt);
        let frames = |joiner: &mut UdpTransport| -> Vec<u32> {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let mut frames: Vec<u32> = joiner
                .recv_all()
                .into_iter()
                .filter_map(|(msg, _)| {
                    let NetMsg::Inputs(batch) = msg else {
                        return None;
                    };
                    (batch.round == round).then_some(batch.inputs)
                })
                .flatten()
                .map(|input| input.frame)
                .collect();
            frames.sort_unstable();
            frames.dedup();
            frames
        };
        let mut tail = RoundTail::kept(Box::new(played), rounds_dealt);
        // A spectator at frame 180, four seconds short of the end.
        let behind = Batch {
            round,
            wants: Some([180; MAX_PLAYERS]),
            ..Batch::default()
        };
        tail.repeat(&transport, 0.1, false, &[(1, behind.clone())]);
        assert!(
            frames(&mut joiners[0]).is_empty(),
            "only the peer that asked"
        );
        let sent = frames(&mut joiners[1]);
        assert_eq!(sent.first(), Some(&180), "from where it stands: {sent:?}");
        assert!(
            sent.iter().all(|&frame| frame < 300),
            "and none past the end"
        );
        let last = sent.last().copied().unwrap_or(0);
        let mut wants = behind.clone();
        wants.wants = Some([last + 1; MAX_PLAYERS]);
        tail.repeat(&transport, 0.1, false, &[(1, wants)]);
        assert_eq!(
            frames(&mut joiners[1]).last(),
            Some(&299),
            "to the very end"
        );
        // Another round's batch is answered with nothing.
        let stale = Batch {
            round: round.wrapping_sub(1),
            ..behind.clone()
        };
        tail.repeat(&transport, 0.1, false, &[(1, stale)]);
        assert!(frames(&mut joiners[1]).is_empty(), "the last round's");
        tail.repeat(&transport, 0.1, true, &[]);
        let everybody = [frames(&mut joiners[0]), frames(&mut joiners[1])];
        assert!(
            everybody.iter().all(|sent| sent.last() == Some(&299)),
            "the tick: {everybody:?}"
        );
        tail.repeat(&transport, 60.0, true, &[(1, behind)]);
        assert!(frames(&mut joiners[0]).is_empty() && frames(&mut joiners[1]).is_empty());
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
            LobbyReturn::Host {
                announcer: Some(Announcer::new(1).expect("announcer")),
                transport,
                game_name: String::new(),
                // Fewer names than the socket knows peers.
                peers: peers_named(&["Bo"], &[0]),
                played: Box::new(Lockstep::new(0, vec![0, 1], DEFAULT_DELAY)),
                rounds_dealt: 0,
                arranged: None,
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
            LobbyReturn::Joiner {
                transport: UdpTransport::join(("127.0.0.1", 47999)).expect("join"),
                watching: true,
                played_seed: 42,
                bot: None,
            },
            &EN,
        );
        let joined = state.joined().expect("joining");
        assert!(joined.watching, "still watching");
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
        state.standing = hosting_lobby();
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
            standing: hosting_lobby(),
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
        assert_eq!(
            place_names(&state.roster(&EN, "Anna")),
            ["Anna", "Bo", "Cy"]
        );

        // Peer 0 came to watch, so the table is the host and Cy.
        let state = seated(&["Bo", "Cy"], &[0]);
        assert_eq!(place_names(&state.roster(&EN, "Anna")), ["Anna", "Cy"]);

        // A nameless host, and a nameless peer, fall back to seat labels
        // rather than to blanks.
        let state = seated(&[""], &[]);
        let table = state.roster(&EN, "");
        assert_eq!(table.len(), 2);
        assert!(table.iter().all(|who| !who.name.is_empty()), "{table:?}");

        // And a lobby that is not hosting seats nobody but its owner.
        let state = LobbyState::default();
        assert_eq!(place_names(&state.roster(&EN, "Anna")), ["Anna"]);
    }

    /// Dropping a peer has to move the rows behind it with the socket's
    /// own list, or the row after it silently takes its name and its wish
    /// to watch, and nobody notices until somebody is labelled as somebody
    /// else.
    #[test]
    fn forgetting_a_peer_moves_everything_that_indexed_it() {
        let mut state = seated(&["Bo", "Cy", "Dee"], &[2]);
        assert_eq!(
            place_names(&state.roster(&EN, "Anna")),
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
            place_names(&state.roster(&EN, "Anna")),
            ["Anna", "Cy"],
            "Dee still watches"
        );

        // The watcher itself leaves, and takes only its own wish.
        state.hosted_mut().expect("hosting").forget_peer(1);
        let hosted = state.hosted().expect("hosting");
        assert_eq!(hosted.peers.len(), 1);
        assert_eq!(hosted.watchers_aboard(), 0);
        assert_eq!(place_names(&state.roster(&EN, "Anna")), ["Anna", "Cy"]);
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

        state.standing = hosting_lobby();
        assert!(state.hosting());
        assert!(!state.watching(), "a host plays");
        assert!(state.standing().at_a_beach());
    }

    /// The point of naming the standing: four places used to ask this
    /// question and each spelled it differently.
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
