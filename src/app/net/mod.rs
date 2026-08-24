//! Online versus, shell side: a UDP lockstep session driving the same
//! versus mode local play uses (spec §7.6 fallback path; see sim::net for
//! why lockstep rather than GGRS rollback today).
//!
//! Dev-grade session setup: `PINCH_HOST=<port>` hosts as player 0,
//! `PINCH_JOIN=<addr:port>` joins as player 1. Both boot straight into the
//! arena; the match starts once the handshake completes.

use crate::sim::{HASH_INTERVAL, Lockstep, MAX_PLAYERS, PlayerAction};
use crate::transport::{
    Announcer, MatchTerms, NetMsg, SeriesStanding, UdpTransport, name_from_wire, table_from_wire,
    wire_table,
};
use bevy::prelude::*;
use std::collections::VecDeque;

/// How many ticks a resume is repeated after the local session unpauses.
/// A second's worth: enough to ride out a loss burst, and harmless if the
/// peers already heard the first one.
const RESUME_ECHOES: u8 = 30;

pub struct OnlineSession {
    pub transport: UdpTransport,
    pub session: Lockstep,
    /// Seats in the match, humans plus AI. The lockstep only carries the
    /// humans, so this is not `session.player_count()`.
    pub seats: u8,
    /// What the lobby agreed the match is: AI seats, map, gulls, round
    /// length, light, team scoring, and the board's seed. Every peer builds
    /// from these, so nothing about the round is a local opinion.
    pub terms: MatchTerms,
    /// The host's handmade beach, compressed, when the round is played on
    /// one. A generated arena travels as a seed in [`Self::terms`]; a
    /// level somebody built has to travel as itself, because no peer but
    /// the host has the file. Empty for the built-in maps.
    pub beach: Vec<u8>,
    /// Host side: the seat each peer index was given, `None` for the
    /// watchers among them. Empty on a joiner, which answers nobody.
    pub peer_seats: Vec<Option<u8>>,
    /// Host side: what each peer index calls itself, from its greeting,
    /// and whether it asked to watch rather than play. Kept for the peers
    /// the launch plan does not cover: someone who queued mid-round holds
    /// no seat yet, so its name and its wish to watch have nowhere else to
    /// live until [`Self::call_next_round`] seats it. Grown as peers appear
    /// and shifted with the socket's own list by [`Self::forget_peer_row`].
    peer_names: Vec<String>,
    peer_watch: Vec<bool>,
    /// What each seat is called, agreed at the handshake so every peer
    /// shows the same table. Empty entries fall back to seat labels; local
    /// couch names never apply to an online round.
    pub names: [String; MAX_PLAYERS],
    /// First frame where the peers' state hashes disagreed, if ever: a
    /// determinism bug surfaced loudly rather than played through.
    pub desync_at: Option<u32>,
    /// Seconds the round has been unable to move, and the frame it has been
    /// unable to move off.
    ///
    /// Measured by the frame number rather than by whether the next frame's
    /// inputs happen to be in, because the second question is asked at a
    /// moment where the answer is always no. The sim runs on the fixed step
    /// and advances until it *cannot*; the tick that watches for stalls runs
    /// after it, in `Update`. So it always found the next frame's slots
    /// empty and always called a healthy round stalled. That put "waiting
    /// for Bob" on both screens of a round that was running perfectly, and
    /// made the socket-silence rule necessary to stop the host handing out
    /// its friends' castles five seconds into every match.
    ///
    /// "The picture has not moved" is the thing both readers actually want,
    /// and it is not a matter of timing within a frame.
    stalled_for: f32,
    stalled_on: u32,
    /// Who the status line is naming, and how long it has left to keep
    /// naming them.
    ///
    /// A latch rather than a reading of `stalled_for`, because that number
    /// snaps back to zero the moment one frame gets through, and a round
    /// that is limping gets a frame through all the time. Reading it
    /// directly put the line on and off once a second: a strobe in the
    /// corner of the eye, which is worse than saying nothing at all.
    waiting_on: Option<u8>,
    waiting_hold: f32,
    /// Seconds since each peer index was last heard from: anything at all,
    /// an input, a hash, a stray greeting.
    ///
    /// Kept beside the transport's own peer list and shifted with it. This
    /// is the difference between a player who has *gone* and one who is
    /// merely behind: a machine that is still talking to us has not left
    /// the room, however far its inputs have fallen back, and giving its
    /// castle away because a burst of loss held one frame up is how you
    /// lose a friend's round for them. A joiner keeps the one entry it has,
    /// the host, and calls the round off when that goes quiet.
    peer_silence: Vec<f32>,
    /// Seats given up on this round, each with the frame it was emptied
    /// from, so the shell can put an AI in each and say so once rather
    /// than every frame, and the host can keep telling the table.
    pub abandoned: Vec<(u8, u32)>,
    /// Host side: the beacon, carried out of the lobby so the beach stays
    /// on the air while the round runs, listed as in progress with its
    /// occupancy, for anyone who wants the next one. Silent on a joiner,
    /// and on a direct `PINCH_HOST` pair, which never announced at all.
    announcer: Farewell,
    /// Whether this session was formed in the beach lobby, which is the
    /// only place a finished match has to walk its table back to: the
    /// direct `PINCH_HOST` pair was never anywhere else to begin with.
    pub from_lobby: bool,
    /// What the beach is called, for the beacon it keeps up while the round
    /// runs. Not a player's name: the list is choosing between games.
    pub game_name: String,
    announce_in: f32,
    /// Joiner side: seconds until the next greeting while the results card
    /// is up.
    ///
    /// An established session says nothing at all between rounds. Inputs
    /// and hashes belong to the round that ended, and the host has nothing
    /// to send until somebody calls the next one, so both ends fall silent
    /// on purpose. That is fine until silence is being read as evidence,
    /// and then it is a joiner walking out of a perfectly good table
    /// twenty seconds into a results card. So a joiner keeps saying hello,
    /// on the same cadence the lobby uses, and the host's reply is what
    /// tells it the host is still there.
    greet_in: f32,
    /// A next round has been agreed and the session is armed for it; the
    /// shell reads this and walks everyone back into the arena. Cleared by
    /// whoever acts on it.
    pub next_round: bool,
    /// Where the series stands as the next round begins, as the host said
    /// it: the 1-based round number and the wins per seat, both re-dealt to
    /// this round's chairs. The shell folds it into its `Tournament` on the
    /// way into the arena, so a peer admitted mid-series joins the table's
    /// standings rather than starting its own, and a survivor whose seat
    /// moved keeps the wins it earned. `None` outside a series.
    pub series_standing: Option<SeriesStanding>,
    /// Ticks of `Resume` still to repeat (see [`RESUME_ECHOES`]).
    resume_echo: u8,
    own_hashes: VecDeque<(u32, u64)>,
    /// Peer hashes for frames we may not have simulated yet; compared (and
    /// drained) as our own hashes appear, so no exchanged check is skipped.
    peer_hashes: VecDeque<(u32, u64)>,
}

/// The goodbye a hosted beach owes the network when its session ends.
///
/// A hosted beach stops being announced when its session goes, whichever
/// way that happens: the match ending, the player quitting to the menu, a
/// desync giving up. Holding the duty here, beside the announcer itself,
/// rather than at each of those exits is what keeps the promise: there is
/// no path that drops an announcing session without either saying goodbye
/// or deliberately taking the announcer back out through
/// [`OnlineSession::back_to_the_lobby`], the one exit where the beach is
/// not going anywhere.
struct Farewell {
    /// `None` on a joiner and on the direct pair, which never announced,
    /// and in the shell a session leaves behind when its beach goes home.
    announcer: Option<Announcer>,
    /// The game port the beacons named, taken when the announcer came
    /// aboard: the port never changes after the bind, and it has to be
    /// readable at drop time.
    port: u16,
}

impl Farewell {
    /// A session with nothing to announce and so nothing to owe.
    fn silent() -> Farewell {
        Farewell {
            announcer: None,
            port: 0,
        }
    }

    /// The announcer, while one is aboard.
    fn aboard(&self) -> Option<&Announcer> {
        self.announcer.as_ref()
    }

    /// Take the announcer back out, disarming the goodbye: the beach is
    /// going back on the air, not away.
    fn disarm(mut self) -> Option<Announcer> {
        self.announcer.take()
    }
}

impl Drop for Farewell {
    fn drop(&mut self) {
        if let Some(announcer) = &self.announcer {
            announcer.closing(self.port);
        }
    }
}

/// What survives of a session when its table walks back to the lobby
/// together at the end of a match: the sockets, still connected, and the
/// bookkeeping the lobby needs to stand the beach back up.
pub struct LobbyReturn {
    /// Host side: the beacon, going back on the air as open. `None` for a
    /// joiner.
    pub announcer: Option<Announcer>,
    pub transport: UdpTransport,
    pub game_name: String,
    /// Host side: what each registered peer calls itself, by peer index.
    pub peer_names: Vec<String>,
    /// Host side: the peers that watch rather than play, by the same rule
    /// the next round would have seated them.
    pub watchers: Vec<usize>,
    /// Joiner side: this peer was at the rail, and greets with `Watch`.
    pub watching: bool,
    pub host: bool,
    /// The seed of the round just played. A host still on its results
    /// card re-answers every greeting with that round's `Start`, and a
    /// lobby that has just walked out of it must read the repeat as
    /// stale rather than as an invitation straight back in.
    pub played_seed: u64,
}

#[derive(Resource, Default)]
pub struct Online(pub Option<OnlineSession>);

/// Pass `msg` from peer `from` on to every other peer. Star topology: the
/// spokes cannot hear each other, so whatever one says reaches the rest
/// only by the hub repeating it, and inputs, pauses, resumes and chat
/// are all repeated this same way.
pub(crate) fn relay(transport: &UdpTransport, from: usize, msg: NetMsg) {
    for other in 0..transport.peer_count() {
        if other != from {
            transport.send_to(other, msg.clone());
        }
    }
}

/// Drain the session while the results card is up. See
/// [`OnlineSession::poll_between_rounds`] for why this cannot wait for the
/// sim: the sim is exactly what has stopped.
pub fn poll_between_rounds(time: Res<Time>, mut online: ResMut<Online>) {
    if let Some(session) = &mut online.0 {
        session.poll_between_rounds(time.delta_secs());
    }
}

mod presence;
mod rounds;
pub(crate) use presence::{abandon_the_departed, leave_a_hostless_round};
pub use rounds::Invitation;
pub(crate) use rounds::lockstep_for;

impl OnlineSession {
    pub fn new(
        transport: UdpTransport,
        session: Lockstep,
        seats: u8,
        terms: MatchTerms,
    ) -> OnlineSession {
        OnlineSession {
            transport,
            session,
            seats,
            terms,
            beach: Vec::new(),
            peer_seats: Vec::new(),
            peer_names: Vec::new(),
            peer_watch: Vec::new(),
            names: Default::default(),
            stalled_for: 0.0,
            stalled_on: 0,
            waiting_on: None,
            waiting_hold: 0.0,
            peer_silence: Vec::new(),
            abandoned: Vec::new(),
            announcer: Farewell::silent(),
            from_lobby: false,
            game_name: String::new(),
            announce_in: 0.0,
            greet_in: 0.0,
            next_round: false,
            series_standing: None,
            desync_at: None,
            resume_echo: 0,
            own_hashes: VecDeque::new(),
            peer_hashes: VecDeque::new(),
        }
    }

    /// Keep the beach on the air while the round runs, so a lobby can list
    /// it as in progress rather than not at all. Hosts only, and the beacon
    /// says running, so nobody is offered a join that lockstep could not
    /// honour; what it offers is a place in the queue.
    ///
    /// `taken` counts the humans, not the table: an AI seat gives way to a
    /// player who wants it, the same way the lobby fills bots in behind
    /// whoever turned up.
    pub fn keep_announcing(&mut self, delta: f32) {
        if self.announcer.aboard().is_none()
            || !crate::app::lobby::once_a_second(&mut self.announce_in, delta)
        {
            return;
        }
        let taken = self.session.player_count() as u8;
        let (Some(announcer), Ok(addr)) = (self.announcer.aboard(), self.transport.local_addr())
        else {
            return;
        };
        announcer.running(
            addr.port(),
            crate::transport::OnAir {
                name: &self.game_name,
                // Seat zero is the host's, and the host is the only one that
                // announces.
                host: &self.names[0],
                taken,
                seats: MAX_PLAYERS as u8,
            },
        );
    }

    /// Host: carry the beacon out of the lobby, and with it the goodbye
    /// owed to the network from now on.
    pub fn stay_on_air(&mut self, announcer: Announcer) {
        // Port 0 if the socket will not say: the goodbye is matched by
        // the announcer's id first, so it still clears the right row.
        let port = self
            .transport
            .local_addr()
            .map(|addr| addr.port())
            .unwrap_or(0);
        self.announcer = Farewell {
            announcer: Some(announcer),
            port,
        };
    }

    /// Dismantle a finished match into what the lobby needs to stand the
    /// beach back up, goodbye disarmed: this table is going back to the
    /// lobby together, not away.
    pub fn back_to_the_lobby(self) -> LobbyReturn {
        let host = self.is_host();
        let watching = self.watching();
        let peers = self.transport.peer_count();
        // Who watches, by the same rule `next_plan` would have seated
        // them: a spectator's `None` in the launch plan, or a watch wish
        // greeted mid-round.
        let watchers = (0..peers)
            .filter(|&peer| {
                matches!(self.peer_seats.get(peer), Some(None))
                    || self.peer_watch.get(peer).copied().unwrap_or(false)
            })
            .collect();
        let peer_names = (0..peers)
            .map(|peer| self.peer_name(peer).unwrap_or_default().to_string())
            .collect();
        let Self {
            transport,
            session: _,
            seats: _,
            terms,
            beach: _,
            peer_seats: _,
            peer_names: _,
            peer_watch: _,
            names: _,
            desync_at: _,
            stalled_for: _,
            stalled_on: _,
            waiting_on: _,
            waiting_hold: _,
            peer_silence: _,
            abandoned: _,
            announcer,
            from_lobby: _,
            game_name,
            announce_in: _,
            greet_in: _,
            next_round: _,
            series_standing: _,
            resume_echo: _,
            own_hashes: _,
            peer_hashes: _,
        } = self;
        LobbyReturn {
            announcer: announcer.disarm(),
            transport,
            game_name,
            peer_names,
            watchers,
            watching,
            host,
            played_seed: terms.seed,
        }
    }

    /// Call a pause, and tell the peers which frame it lands on. A
    /// spectator's Escape opens their own menu; the match plays on behind
    /// it, and they are still in step when they close it, so the lockstep
    /// names no frame and nothing is sent.
    pub fn request_pause(&mut self) {
        if let Some(frame) = self.session.request_pause() {
            self.transport.send(NetMsg::Pause { frame });
        }
    }

    /// Play on, and keep saying so for a moment (see `RESUME_ECHOES`).
    pub fn request_resume(&mut self) {
        if self.watching() {
            return;
        }
        let frame = self.session.resume();
        self.resume_echo = RESUME_ECHOES;
        self.transport.send(NetMsg::Resume { frame });
    }

    /// The `Resume` to repeat: the pause most recently lifted here.
    fn resume_msg(&self) -> NetMsg {
        NetMsg::Resume {
            frame: self.session.lifted_pause().unwrap_or(0),
        }
    }

    /// Is this session the star's hub? Seat 0 hosts and relays.
    ///
    /// Public because the shell has to know who calls the next round, which
    /// it did through a second method of the same one line, under a second
    /// copy of this sentence.
    pub fn is_host(&self) -> bool {
        self.session.seat() == Some(0)
    }

    /// Whether this peer watches rather than plays.
    pub fn watching(&self) -> bool {
        self.session.watching()
    }

    /// One fixed-tick's worth of network pumping: commit and (re)send local
    /// input, drain the socket (the host relays joiner inputs to the other
    /// joiners and re-answers stray hellos with their seat), then simulate
    /// every frame that has complete inputs via `tick`. Records hashes on
    /// the [`HASH_INTERVAL`] cadence. Returns whether the local action was
    /// committed (false = at the commit lead; retry it next tick).
    pub fn pump(&mut self, local_action: PlayerAction, mut tick: impl FnMut(&mut Self)) -> bool {
        let committed = self.session.commit_local(local_action).is_some();
        for &msg in self.session.recent_commits() {
            self.transport.send(NetMsg::Input(msg));
        }
        // Pause state is repeated every tick rather than sent once: UDP
        // drops, and the peer that misses a Pause would otherwise sit
        // watching a frozen beach with no card, while a missed Resume would
        // leave the session stopped for good. Both self-heal here: the
        // resume repeats until frames actually start moving again.
        match self.session.pause_frame() {
            Some(frame) => self.transport.send(NetMsg::Pause { frame }),
            None if self.resume_echo > 0 => {
                self.resume_echo -= 1;
                self.transport.send(self.resume_msg());
            }
            None => {}
        }
        let host = self.is_host();
        // Likewise every seat given up on, for the rest of the round: a
        // joiner that misses the one notice waits on that seat forever,
        // and the host never notices, because a joiner that is waiting is
        // still talking. Five bytes a seat a tick.
        if host {
            for &(seat, frame) in &self.abandoned {
                self.transport.send(NetMsg::Abandoned { seat, frame });
            }
        }
        for (msg, from) in self.transport.recv_all() {
            // Whatever it was, that peer is still there, which is all
            // `abandon_stalled` and `host_gone` go on.
            self.mark_heard(from);
            match msg {
                // A peer that missed the Start datagram keeps greeting us;
                // repeat what it is until it stops. A watcher is told it is
                // watching, so the seat it gets is not one.
                NetMsg::Hello { name } => {
                    if host {
                        // Its name is written down whether it is at the
                        // table or waiting in line: a peer that queues
                        // mid-round holds no seat to keep it in yet, and
                        // without this it was seated nameless next round.
                        let told = name_from_wire(&name);
                        if !told.is_empty() {
                            self.remember_peer_name(from, &told);
                        }
                        if let Some(queued) = self.queue_place(from) {
                            self.transport.send_to(from, queued);
                            continue;
                        }
                        let seat = self.seat_of(from);
                        if let Some(slot) =
                            seat.and_then(|seat| self.names.get_mut(usize::from(seat)))
                            && !told.is_empty()
                        {
                            *slot = told;
                        }
                        let start = self.start_msg(seat);
                        self.transport.send_to(from, start);
                    }
                }
                NetMsg::Watch => {
                    if host {
                        // Watching is no more possible than playing once the
                        // round is under way: a spectator simulates the same
                        // frames as everyone else, from the same frame zero.
                        // The wish is remembered, so a peer that armed W and
                        // dialled in mid-round is seated as an onlooker next
                        // round rather than dealt a chair it never asked for.
                        self.note_watch_wish(from);
                        let answer = self
                            .queue_place(from)
                            .unwrap_or_else(|| self.start_msg(None));
                        self.transport.send_to(from, answer);
                    }
                }
                NetMsg::Input(input) => {
                    // The host knows which seat each peer was given, and
                    // takes inputs for that seat alone: a peer speaking for
                    // another's seat (a bug, or a spectator with ideas)
                    // would otherwise be believed by whoever heard it
                    // first, and the seats would diverge. A joiner hears
                    // only the host, which has already done this. The
                    // direct `PINCH_HOST` pair keeps no plan, having never
                    // been through a lobby; with nothing to check against
                    // it takes any seat but its own, as it always did.
                    if host {
                        let allowed = if self.peer_seats.is_empty() {
                            Some(input.player) != self.session.seat()
                        } else {
                            self.seat_of(from) == Some(input.player)
                        };
                        if !allowed {
                            continue;
                        }
                    }
                    self.session.receive(input);
                    if host {
                        relay(&self.transport, from, NetMsg::Input(input));
                    }
                }
                NetMsg::Hash { frame, hash } => {
                    // The peer may be ahead of us; buffer until we simulate
                    // that frame ourselves.
                    self.peer_hashes.push_back((frame, hash));
                    while self.peer_hashes.len() > 64 {
                        self.peer_hashes.pop_front();
                    }
                }
                NetMsg::Pause { frame } => {
                    self.session.receive_pause(frame);
                    if host {
                        relay(&self.transport, from, NetMsg::Pause { frame });
                    }
                }
                NetMsg::Resume { frame } => {
                    let frame = self.session.receive_resume(frame);
                    self.resume_echo = RESUME_ECHOES;
                    if host {
                        relay(&self.transport, from, NetMsg::Resume { frame });
                    }
                }
                NetMsg::Start {
                    seats,
                    seat,
                    terms,
                    names,
                    standing,
                    beach,
                } if !host && self.is_next_round(&terms) => {
                    // The host has called the next round. A fresh seed is
                    // what says so; everything else about the round may
                    // well be the same. Taking it up here only rearms the
                    // session; the board is rebuilt on the way back into
                    // the arena, by the same path a first round uses.
                    self.take_up(Invitation {
                        seats,
                        seat,
                        terms,
                        names,
                        standing,
                        beach,
                    });
                    self.next_round = true;
                }
                NetMsg::Start { names, .. } => {
                    // Stale as a launch signal, but a joiner in the direct
                    // PINCH_HOST/PINCH_JOIN pair, which never waits in a
                    // lobby, learns the table's names from it.
                    if !host {
                        for (slot, name) in self.names.iter_mut().zip(table_from_wire(&names)) {
                            if !name.is_empty() {
                                *slot = name;
                            }
                        }
                    }
                }
                // Someone on another build is talking to this port. Every
                // peer in a running match paired before it started, so this
                // is a stranger, not a member: the match plays on without it.
                NetMsg::Incompatible { .. } => {}
                // Only a host hands these out, and a host never receives
                // one: a joiner in the queue is still in the lobby.
                NetMsg::Queued { .. } => {}
                // Chat is a lobby thing. A line that arrives mid-round is
                // from a peer still sitting on its lobby screen, and there
                // is nowhere here to show it.
                NetMsg::Chat { .. } => {}
                NetMsg::Abandoned { seat, frame } => {
                    // The host's word, not our own patience. Idempotent,
                    // because it is repeated against packet loss.
                    if !host && !self.abandoned.iter().any(|(gone, _)| *gone == seat) {
                        self.session.abandon(seat, frame);
                        self.abandoned.push((seat, frame));
                    }
                }
                // The lobby's business, and the lobby is behind us.
                NetMsg::Roster { .. } => {}
            }
        }
        self.compare_hashes();
        // At most a couple of frames per fixed tick: a lagging peer catches
        // up gradually instead of spiralling.
        for _ in 0..2 {
            tick(self);
        }
        self.compare_hashes();
        committed
    }

    fn compare_hashes(&mut self) {
        for &(frame, peer) in &self.peer_hashes {
            if let Some(&(_, own)) = self.own_hashes.iter().find(|(f, _)| *f == frame)
                && own != peer
            {
                self.desync_at.get_or_insert(frame);
            }
        }
    }

    /// Called by the tick closure after simulating a frame, with the fresh
    /// state hash.
    pub fn after_frame(&mut self, hash: u64) {
        let frame = self.session.frame();
        if frame.is_multiple_of(HASH_INTERVAL) {
            self.own_hashes.push_back((frame, hash));
            while self.own_hashes.len() > 16 {
                self.own_hashes.pop_front();
            }
            self.transport.send(NetMsg::Hash { frame, hash });
        }
    }
}

/// Parse the dev env vars into a ready session, if requested.
/// `PINCH_HOST=47777` or `PINCH_JOIN=192.168.1.10:47777`.
pub fn session_from_env() -> Option<OnlineSession> {
    // The direct hooks skip the lobby, so there is no Start to agree on:
    // both processes must be told the same terms. `PINCH_BOTS=n` seats n AI
    // players behind the two humans and has to be set on both sides.
    let bots: u8 = crate::app::dev::bots().unwrap_or(0);
    // Two humans and the AI behind them; MAX_PLAYERS is the ceiling, and
    // the clamp is on the bots so a wild PINCH_BOTS cannot wrap the sum
    // back under the two humans.
    let humans = 2u8;
    let bots = bots.min(crate::sim::MAX_PLAYERS as u8 - humans);
    let seats = humans + bots;
    let terms = MatchTerms {
        bots,
        ..crate::app::match_setup::terms(
            &crate::app::match_setup::MatchConfig::default(),
            crate::app::teams::TeamMode::Solo,
            0,
        )
    };
    // The name this player goes by online: their P1 name from settings,
    // read straight off disk because the dev hooks run before the app's
    // resources are wired up.
    let own = crate::app::settings::GameSettings::load().names[0].clone();
    if let Some(port) = crate::app::dev::direct_host() {
        let port: u16 = port.parse().ok()?;
        let transport = UdpTransport::host(port).ok()?;
        info!("hosting on UDP port {port}, waiting for a peer ({seats} seats)");
        let mut session = OnlineSession::new(
            transport,
            Lockstep::new(0, vec![0, 1], crate::sim::DEFAULT_DELAY),
            seats,
            terms,
        );
        session.names[0] = own;
        return Some(session);
    }
    if let Some(addr) = crate::app::dev::direct_join() {
        let transport = UdpTransport::join(addr.as_str()).ok()?;
        transport.send(NetMsg::hello(&own));
        info!("joining {addr} ({seats} seats)");
        let mut session = OnlineSession::new(
            transport,
            Lockstep::new(1, vec![0, 1], crate::sim::DEFAULT_DELAY),
            seats,
            terms,
        );
        session.names[1] = own;
        return Some(session);
    }
    None
}

#[cfg(test)]
mod homecoming_tests {
    use super::*;
    use crate::sim::DEFAULT_DELAY;
    use crate::transport::{Beacon, Discovery};

    fn hosting_session(seed: u64) -> OnlineSession {
        OnlineSession::new(
            UdpTransport::host(0).expect("game socket"),
            Lockstep::new(0, vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms {
                seed,
                ..MatchTerms::default()
            },
        )
    }

    /// Beacons from other tests share the machine, so every packet is
    /// judged by whether it names our game port.
    fn heard_for(discovery: &mut Discovery, port: u16) -> Vec<Beacon> {
        let mut heard = Vec::new();
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            heard.extend(
                discovery
                    .poll()
                    .into_iter()
                    .filter(|(addr, _)| addr.port() == port)
                    .map(|(_, beacon)| beacon),
            );
            if !heard.is_empty() {
                break;
            }
        }
        heard
    }

    /// The goodbye still fires on every ordinary way out - quitting to the
    /// menu, a desync giving up, the process letting go - which is the
    /// promise the [`Farewell`] guard carries for the whole session.
    #[test]
    fn a_dropped_session_still_says_goodbye() {
        let mut discovery = Discovery::bind().expect("lobby port");
        let mut session = hosting_session(0);
        let port = session.transport.local_addr().expect("addr").port();
        session.stay_on_air(Announcer::new(0xD00D).expect("announcer"));
        drop(session);
        let heard = heard_for(&mut discovery, port);
        let beacon = heard.first().expect("the goodbye went out");
        assert!(matches!(beacon, Beacon::Closing { .. }), "{beacon:?}");
    }

    /// The one exit where the beach is not going anywhere: walking the
    /// table back to the lobby keeps the socket, keeps the beacon, and
    /// says no goodbye, because the same beach is about to announce
    /// itself open again.
    #[test]
    fn the_way_back_to_the_lobby_says_no_goodbye() {
        let mut discovery = Discovery::bind().expect("lobby port");
        let mut session = hosting_session(42);
        let port = session.transport.local_addr().expect("addr").port();
        session.stay_on_air(Announcer::new(0xB0A7).expect("announcer"));
        session.game_name = "Room 3".into();
        session.from_lobby = true;
        let returned = session.back_to_the_lobby();
        assert!(returned.host);
        assert!(!returned.watching);
        assert_eq!(returned.played_seed, 42, "the round it walked out of");
        assert_eq!(returned.game_name, "Room 3");
        assert_eq!(
            returned.transport.local_addr().expect("addr").port(),
            port,
            "the same socket every peer is still talking to"
        );
        assert!(
            returned.announcer.is_some(),
            "the beacon rides home to announce again"
        );
        std::thread::sleep(std::time::Duration::from_millis(40));
        assert!(
            discovery
                .poll()
                .into_iter()
                .all(|(addr, _)| addr.port() != port),
            "no goodbye for a beach that is coming home"
        );
    }

    /// What the lobby gets back to seat people with: each peer's name as
    /// its greeting carried it, and the watchers by the same rule the next
    /// round would have used - a spectator's `None` in the launch plan, or
    /// a watch wish greeted mid-round.
    #[test]
    fn the_table_walks_back_with_its_names_and_watchers() {
        let mut session = hosting_session(0);
        let port = session.transport.local_addr().expect("addr").port();
        // Two peers, registered one at a time so their indices are known:
        // Bo plays, and somebody nameless watches from the rail.
        let bo = UdpTransport::join(("127.0.0.1", port)).expect("join");
        bo.send(NetMsg::hello("Bo"));
        let rail = UdpTransport::join(("127.0.0.1", port)).expect("join");
        for want in [1, 2] {
            if want == 2 {
                rail.send(NetMsg::Watch);
            }
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                for (msg, from) in session.transport.recv_all() {
                    match msg {
                        NetMsg::Hello { name } => {
                            let told = name_from_wire(&name);
                            session.remember_peer_name(from, &told);
                        }
                        NetMsg::Watch => session.note_watch_wish(from),
                        NetMsg::Input(_)
                        | NetMsg::Hash { .. }
                        | NetMsg::Start { .. }
                        | NetMsg::Pause { .. }
                        | NetMsg::Resume { .. }
                        | NetMsg::Queued { .. }
                        | NetMsg::Chat { .. }
                        | NetMsg::Roster { .. }
                        | NetMsg::Abandoned { .. }
                        | NetMsg::Incompatible { .. } => {}
                    }
                }
                if session.transport.peer_count() >= want {
                    break;
                }
            }
            assert_eq!(session.transport.peer_count(), want, "peer registered");
        }
        session.peer_seats = vec![Some(1), None];
        let returned = session.back_to_the_lobby();
        assert_eq!(
            returned.peer_names,
            vec!["Bo".to_string(), String::new()],
            "each chair keeps the name its greeting carried"
        );
        assert_eq!(returned.watchers, vec![1], "and the rail stays the rail");
    }

    /// A joiner's way back: no beacon to carry, but the socket, the seed
    /// it played on, and whether it was watching all come home with it.
    #[test]
    fn a_watching_joiner_comes_home_to_the_rail() {
        let session = OnlineSession::new(
            UdpTransport::join(("127.0.0.1", 47999)).expect("join"),
            Lockstep::observer(vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms {
                seed: 7,
                ..MatchTerms::default()
            },
        );
        let returned = session.back_to_the_lobby();
        assert!(!returned.host);
        assert!(returned.watching, "the rail is remembered");
        assert!(returned.announcer.is_none(), "a joiner has no beacon");
        assert_eq!(returned.played_seed, 7);
    }
}
