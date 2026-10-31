//! The per-tick pump: what a session sends every fixed tick, what it
//! does with everything it hears while a round is played, and the state
//! hashes that keep the table honest about playing the same one.

use super::*;

/// How many ticks a resume is repeated after the local session unpauses.
/// A second's worth: enough to ride out a loss burst, and harmless if the
/// peers already heard the first one.
pub(super) const RESUME_ECHOES: u8 = 30;

/// How many ticks apart the host sends a round's `Start` again to a peer
/// it has not heard from in the round yet: a sixth of a second.
///
/// Otherwise a lost `Start` waits on the peer's next greeting, a second
/// apart in the lobby, with the table frozen on the seat meanwhile. A player
/// is sent it until its first inputs arrive; a watcher, which sends the
/// host nothing in a round, as long as a frame-zero `Start` is still any
/// use to it (`catch_up::FRAME_ZERO_REACH`), after which a greeting earns
/// it the round as it stands instead.
pub(super) const START_AGAIN: u32 = 5;

/// How many ticks apart the host tells each player how its clock stands
/// (`Batch::clock`): three times a second, slower than any round trip a
/// table plays across, so each reading sees the last nudge land.
const CLOCK_EVERY: u32 = 10;

/// How many ticks apart a watcher tells the host which frames it holds
/// (`Batch::wants`): ten times a second.
///
/// It sends nothing else in a round, and the word only trims what the host
/// sends it: a word a few ticks old has the host repeat a few more frames,
/// which is the same redundancy that rides out a lost datagram. Every tick,
/// three watchers would add 90 datagrams a second to the host's way in.
const WATCHER_WORD_EVERY: u32 = 3;

/// The most frames one `Clock` moves a player's commits: as far as the
/// commit lead reaches, so a late start is made up in a nudge or two.
const MOST_NUDGE: i64 = crate::sim::MAX_COMMIT_LEAD as i64;

/// How far apart the two readings of a `Clock` may be before they say
/// anything, in frames, on a round played at `delay`: a frame of it is
/// where the two ticks happen to fall against each other, and another is
/// a datagram a tick late.
///
/// One more past a delay of four, which the host picks for a table whose
/// round trips came back long or uneven ([`crate::sim::delay_for_round_trip`]).
/// The readings travel with the inputs, so a reading is as late as the
/// datagram carrying it: where datagrams are held back up to two ticks
/// each way, a level clock read three frames off often enough that two
/// such readings in a row nudged a player off level every few seconds,
/// and the table stalled on it until a pair nudged it back. A frame more
/// of difference left alone is well within a delay that long.
fn clock_slack(delay: u32) -> i64 {
    if delay > crate::sim::DEFAULT_DELAY + 1 {
        3
    } else {
        2
    }
}

/// The players' state hashes against our own.
#[derive(Default)]
pub(super) struct HashCheck {
    /// First frame where a player's state hash disagreed with ours, if
    /// ever: a determinism bug surfaced loudly rather than played through.
    pub(super) desync_at: Option<u32>,
    own: VecDeque<(u32, u64)>,
    /// Each peer's hashes, by its index on the socket, for frames we may
    /// not have simulated yet; compared as our own hashes appear, so no
    /// exchanged check is skipped.
    ///
    /// One list per peer rather than one for the table, so a peer sending
    /// a lot cannot push another's hashes out of the window before they
    /// are weighed, and moved down with the peers behind one forgotten
    /// (`forget`), like every other thing kept by index.
    peers: Vec<VecDeque<(u32, u64)>>,
}

/// The most lockstep frames one fixed tick plays: a lagging peer catches
/// up this many at a time. Read as well by whatever has to outlast the
/// ticks a single drawn frame can cover (`crate::sim::SWEEP_MEMORY`).
pub(crate) const CATCH_UP_FRAMES: usize = 2;

impl HashCheck {
    /// Our own hash for `frame`. A short window: the peers are at most a
    /// commit lead behind.
    pub(super) fn record_own(&mut self, frame: u32, hash: u64) {
        self.own.push_back((frame, hash));
        while self.own.len() > 16 {
            self.own.pop_front();
        }
    }

    /// Peer `peer`'s hash for `frame`, which we may not have simulated
    /// yet.
    pub(super) fn record_peer(&mut self, peer: usize, frame: u32, hash: u64) {
        if self.peers.len() <= peer {
            self.peers.resize_with(peer + 1, VecDeque::new);
        }
        let hashes = &mut self.peers[peer];
        hashes.push_back((frame, hash));
        while hashes.len() > 64 {
            hashes.pop_front();
        }
    }

    /// A peer is forgotten: its hashes go with it, and those of the peers
    /// behind it move down one with their rows.
    pub(super) fn forget(&mut self, peer: usize) {
        if peer < self.peers.len() {
            self.peers.remove(peer);
        }
    }

    /// Compare what has been exchanged, and remember the first frame that
    /// disagreed.
    pub(super) fn compare(&mut self) {
        for &(frame, peer) in self.peers.iter().flatten() {
            if let Some(&(_, own)) = self.own.iter().find(|(f, _)| *f == frame)
                && own != peer
            {
                self.desync_at.get_or_insert(frame);
            }
        }
    }

    /// Last round's disagreement is last round's; a new board starts
    /// level, and the hashes that proved it are gone with it.
    pub(super) fn reset(&mut self) {
        *self = HashCheck::default();
    }
}

impl OnlineSession {
    /// The first frame the peers' state hashes disagreed on, if ever.
    pub fn desync_at(&self) -> Option<u32> {
        self.hashes.desync_at
    }

    /// Call a pause, and tell the peers which frame it lands on. A
    /// spectator's Escape opens their own menu; the match plays on behind
    /// it, and they are still in step when they close it, so the lockstep
    /// names no frame and nothing is sent.
    pub fn request_pause(&mut self) {
        if let Some(frame) = self.session.request_pause() {
            let round = self.round_tag();
            self.transport.send(NetMsg::Pause { round, frame });
        }
    }

    /// Play on, and keep saying so for a moment (see `RESUME_ECHOES`).
    pub fn request_resume(&mut self) {
        if self.watching() {
            return;
        }
        let frame = self.session.resume();
        self.pace.resume_echo = RESUME_ECHOES;
        self.pace.clock_apart = None;
        let round = self.round_tag();
        self.transport.send(NetMsg::Resume { round, frame });
    }

    /// The frame a repeated resume names: the pause most recently lifted
    /// here.
    fn resumed_frame(&self) -> u32 {
        self.session.lifted_pause().unwrap_or(0)
    }

    /// One fixed-tick's worth of network pumping: commit and (re)send local
    /// input, drain the socket (the host relays joiner inputs to the other
    /// joiners and re-answers stray hellos with their seat), then simulate
    /// every frame that has complete inputs via `tick`. Records hashes on
    /// the [`HASH_INTERVAL`] cadence. Returns whether the local action was
    /// committed (false = at the commit lead; retry it next tick).
    pub fn pump(&mut self, local_action: PlayerAction, tick: impl FnMut(&mut Self)) -> bool {
        self.pump_with(local_action, &[], tick)
    }

    /// [`Self::pump`] for a peer that speaks for other seats besides its
    /// own (a host with bots connected to it): `extra` is their actions for
    /// the frame being committed.
    pub fn pump_with(
        &mut self,
        local_action: PlayerAction,
        extra: &[(u8, PlayerAction)],
        mut tick: impl FnMut(&mut Self),
    ) -> bool {
        let host = self.is_host();
        // Armed for the next round while the board on screen is still the
        // last one: a joiner or spectator still on the last frames of the
        // round the host has finished, when the host's call (or the round
        // as it stands, `take_catch_up_part`) reaches it. The lockstep is
        // the new round's then, and nothing of it is this board's. Ticked,
        // the old board played the new round's first frames out, and the
        // new board began at tick zero with the lockstep frames ahead: a
        // desync at the first hash, for the rest of the round. Committed,
        // a key meant for the old board was played on the new one.
        //
        // So the session only listens until the shell has walked it out of
        // the arena (`follow_the_called_round`), which is the next frame.
        // What it hears is the new round's, and kept for it.
        let armed = |net: &Self| net.next_round && !host;
        let nudging = self.session.nudging();
        let committed = !armed(self)
            && self
                .session
                .commit_local_with(local_action, extra)
                .is_some();
        // A commit refused at the commit lead or by a pause, rather than
        // held back by a nudge: the clock readings taken across it are of
        // the table waiting, not of this peer's pace (`keep_time`).
        if !committed && !nudging {
            self.pace.clock_apart = None;
        }
        for (msg, from) in self.transport.recv_all() {
            // Whatever it was, that peer is still there, which is all
            // `abandon_stalled` and `host_gone` go on.
            // Except a goodbye, which says the opposite, and whose grace
            // runs from the first copy rather than the last.
            if msg != NetMsg::Leaving {
                self.mark_heard(from);
            }
            self.hear_mid_round(host, from, msg);
        }
        // One row per peer the socket has, and never a row for one it
        // does not: rows are grown for a peer the socket just indexed and
        // dropped with it when it is forgotten. A row past the socket's
        // list is somebody else's seat, name or watch wish.
        debug_assert!(
            self.peers.len() <= self.transport.peer_count(),
            "{} peer rows for {} peers",
            self.peers.len(),
            self.transport.peer_count()
        );
        self.hashes.compare();
        // After the socket is drained: on the host, so what it sends each
        // peer has every input heard this tick, the hub passing a player's
        // commit on in the tick it arrives; and everywhere, so what a peer
        // says it holds is what it now does.
        self.send_this_tick(true);
        if armed(self) {
            return committed;
        }
        // At most a couple of frames per fixed tick: a lagging peer catches
        // up gradually instead of spiralling. And none that is not due yet
        // (`Lockstep::due`), so the frames come one a tick whatever rhythm
        // the network delivers the inputs in; a watcher keeps a few in hand
        // the same way (`Lockstep::playout`).
        for _ in 0..self.session.playout(CATCH_UP_FRAMES) {
            if !self.session.due() {
                break;
            }
            tick(self);
        }
        self.hashes.compare();
        committed
    }

    /// One fixed tick of a round dealt but not yet played: the interlude,
    /// the line-up and the count, on whichever end. Everything heard is
    /// taken as the round's pump takes it, and the tick's word goes out as
    /// it does there, but nothing is committed and no frame is played.
    ///
    /// Those are ten seconds and more between the host's call and the
    /// first frame, and a peer silent for them looked gone: a joiner whose
    /// `Start` was lost heard it again only once the host's round ran, then
    /// stood through its own interlude and count without a word while the
    /// host, waiting on its seat, counted the silence past [`ABANDON_AFTER`]
    /// and gave the seat to the AI. Listening here, the host sends a lost
    /// `Start` again within a sixth of a second ([`START_AGAIN`]), and
    /// every peer is heard every tick until its round runs.
    ///
    /// No clock reading goes out: the host commits nothing yet, so a gap
    /// read against its commits is of the count, not of anybody's pace.
    ///
    /// [`ABANDON_AFTER`]: presence::ABANDON_AFTER
    pub fn listen(&mut self) {
        let host = self.is_host();
        for (msg, from) in self.transport.recv_all() {
            if msg != NetMsg::Leaving {
                self.mark_heard(from);
            }
            self.hear_mid_round(host, from, msg);
        }
        self.send_this_tick(false);
    }

    /// [`Self::listen`], and the silences aged by the tick's `delta` as
    /// a running round ages them ([`Self::keep_in_touch`]): what the shell
    /// does every fixed tick from the call to the first frame.
    pub fn before_the_round(&mut self, delta: f32) {
        self.listen();
        self.keep_in_touch(delta);
    }

    /// What goes out every tick whatever was heard, one [`Batch`] to each
    /// peer following the round (`Self::follows_the_round`), never to the
    /// queue: a peer in line is not simulating anything.
    ///
    /// A joiner tells the host its own commits the host has not said it
    /// holds, and what it holds of everybody else's. The host tells each
    /// peer every seat's inputs that peer has not said it holds, and the
    /// round's notices. A watcher, which commits nothing, only says what it
    /// holds, and only now and then ([`WATCHER_WORD_EVERY`]).
    ///
    /// Pause state is repeated every tick rather than sent once: UDP drops,
    /// and the peer that misses a pause would otherwise sit watching a
    /// frozen beach with no card, while a missed resume would leave the
    /// session stopped for good. Both self-heal: the resume repeats for
    /// [`RESUME_ECHOES`] ticks, and a peer that missed every repeat is still
    /// sending its pause, which is answered with the resume again where it
    /// arrives. Likewise every seat given up on, for the rest of the round:
    /// a joiner that misses the one notice waits on that seat forever, and
    /// the host never notices, because a joiner that is waiting is still
    /// talking. The seat's inputs before that frame go on being resent to a
    /// peer still missing them, as everybody's do: the host played them,
    /// and nobody else can say them now the player has gone quiet.
    ///
    /// `clocks`: whether the host's clock readings may ride along, which
    /// they may once its own commits are running ([`Self::listen`]).
    fn send_this_tick(&mut self, clocks: bool) {
        self.pace.round_ticks = self.pace.round_ticks.saturating_add(1);
        let (pause, resume) = match self.session.pause_frame() {
            Some(frame) => (Some(frame), None),
            None if self.pace.resume_echo > 0 => {
                self.pace.resume_echo -= 1;
                (None, Some(self.resumed_frame()))
            }
            None => (None, None),
        };
        let word = Batch {
            round: self.round_tag(),
            inputs: Vec::new(),
            wants: Some(self.session.wants()),
            pause,
            resume,
            abandoned: Vec::new(),
            clock: None,
        };
        if !self.is_host() {
            let quiet = self.watching()
                && word.pause.is_none()
                && word.resume.is_none()
                && !self.pace.round_ticks.is_multiple_of(WATCHER_WORD_EVERY);
            if !quiet {
                let batch = Batch {
                    inputs: self.own_inputs_unheard(),
                    ..word
                };
                self.transport.send_batch(&batch, |_| true);
            }
            return;
        }
        if self.pace.round_ticks.is_multiple_of(START_AGAIN) {
            self.start_again();
        }
        let clock_due = clocks && self.pace.round_ticks.is_multiple_of(CLOCK_EVERY);
        for peer in 0..self.transport.peer_count() {
            if !self.follows_the_round(peer) {
                // The seats given up on go to everybody the socket has, in
                // line or not: a seat dropped while its network was down
                // greets on its way back, and is in line by then, which is
                // how it learns it was dropped (`dropped`) rather than
                // waiting out the host.
                if !self.abandoned.is_empty() {
                    let notice = Batch {
                        round: self.round_tag(),
                        abandoned: self.abandoned.clone(),
                        ..Batch::default()
                    };
                    self.transport.send_batch(&notice, |to| to == peer);
                }
                continue;
            }
            let batch = Batch {
                inputs: self.unheard_by(peer, u32::MAX),
                abandoned: self.abandoned.clone(),
                clock: clock_due.then(|| self.clock_for(peer)).flatten(),
                ..word.clone()
            };
            self.transport.send_batch(&batch, |to| to == peer);
        }
    }

    /// Joiner: this peer's own commits the host has not said it holds.
    fn own_inputs_unheard(&self) -> Vec<crate::sim::InputMsg> {
        let mut from = [None; MAX_PLAYERS];
        if let Some(seat) = self.session.seat() {
            let wanted = self.pace.host.wants.map(|wants| wants[usize::from(seat)]);
            from[usize::from(seat)] = self.session.resend_from(seat, wanted);
        }
        self.session.held_between(&from, u32::MAX)
    }

    /// Host: every seat's inputs `peer` has not said it holds, but for the
    /// seat it plays, whose inputs are its own, for the frames before
    /// `until` (`Lockstep::owed`).
    pub(super) fn unheard_by(&self, peer: usize, until: u32) -> Vec<crate::sim::InputMsg> {
        let wants = self.peers.get(peer).and_then(|row| row.heard.wants);
        self.session.owed(wants, self.seat_of(peer), until)
    }

    /// Host: the round's `Start`, again, to every peer the deal placed that
    /// may still be without it (see [`START_AGAIN`]).
    fn start_again(&self) {
        let watchers_too = self.session.frame() < catch_up::FRAME_ZERO_REACH;
        for (peer, row) in self.peers.iter().enumerate() {
            let owed = match row.place {
                Place::Seated(_) => row.heard.newest.is_none(),
                Place::Watching => watchers_too,
                Place::LateWatching | Place::Queued => false,
            };
            if owed {
                self.transport.send_to(peer, self.start_msg(row.seat()));
            }
        }
    }

    /// Host: how far its own next commit runs past the newest of `peer`'s,
    /// for a player it has heard from.
    fn clock_for(&self, peer: usize) -> Option<i16> {
        let newest = self.peers.get(peer)?.heard.newest?;
        let gap = i64::from(self.session.next_commit()) - i64::from(newest);
        Some(gap.clamp(i16::MIN.into(), i16::MAX.into()) as i16)
    }

    /// Joiner: the host's clock (`Batch::clock`), read against the same
    /// gap the other way round, and this peer's commits nudged to close it.
    ///
    /// The host says how far its next commit runs past our newest: the
    /// clocks' difference plus the trip up. We see how far ours runs past
    /// its newest: the difference the other way plus the trip down. Half
    /// of one less the other is how far ahead of the host this peer is,
    /// the trips cancelling out.
    ///
    /// Within [`clock_slack`] that is noise and left alone, and past it
    /// only two readings in a row that agree are acted on, by the lesser
    /// of them: a nudge on one late datagram would push a peer a frame off
    /// the host for good, where the readings after it cannot see it.
    ///
    /// Not while a pause is called: every peer's commits stop at its frame,
    /// and a reading across the stop is of the stop. The pending one is
    /// dropped with it, and so is the nudge it gave (`Lockstep::nudge`).
    fn keep_time(&mut self, gap: i16) {
        let Some(newest) = self.pace.host.newest else {
            return;
        };
        if self.watching() {
            return;
        }
        if self.session.paused() {
            self.pace.clock_apart = None;
            return;
        }
        let ours = i64::from(self.session.next_commit()) - i64::from(newest);
        let apart = ours - i64::from(gap);
        let slack = clock_slack(self.session.delay());
        let past = |apart: i64| apart.signum() * (apart.abs() - slack).max(0);
        let (now, before) = (past(apart), self.pace.clock_apart.map_or(0, past));
        if now.signum() != 0 && now.signum() == before.signum() {
            let ahead = now.signum() * (now.abs().min(before.abs()) + 1) / 2;
            self.session
                .nudge(ahead.clamp(-MOST_NUDGE, MOST_NUDGE) as i32);
            // The next reading was taken before this nudge landed.
            self.pace.clock_apart = None;
        } else {
            self.pace.clock_apart = Some(apart);
        }
    }

    /// One message heard while the round is played.
    fn hear_mid_round(&mut self, host: bool, from: usize, msg: NetMsg) {
        match msg {
            // A peer that missed the Start datagram keeps greeting us;
            // repeat what it is until it stops. A watcher is told it is
            // watching, so the seat it gets is not one.
            NetMsg::Hello { name, bot } => {
                if host && !self.refuse_a_bot(from, bot) {
                    self.player_greeted_mid_round(from, &name_from_wire(&name), bot);
                }
            }
            NetMsg::Watch { name } => {
                if host {
                    self.watcher_greeted_mid_round(from, &name_from_wire(&name));
                }
            }
            NetMsg::Inputs(batch) => self.take_inputs(host, from, batch),
            NetMsg::Hash { round, frame, hash } => {
                // The peer may be ahead of us; buffer until we simulate
                // that frame ourselves. Only a player's: a watcher that
                // came apart from the table (or anything on the LAN that
                // can say `Hash`) put "desync" on the host's screen for the
                // rest of the round, about a beach nobody was playing on.
                //
                // And only one naming a frame this round can have reached
                // (`Lockstep::within_horizon`): a hash sent at the end of
                // the last round, still on the wire when this one began,
                // was kept with no frame to bound it, and a long round
                // weighed it against its own frame of that number.
                if self.of_this_round(round)
                    && self.plays_this_round(from)
                    && self.session.within_horizon(frame)
                {
                    self.hashes.record_peer(from, frame, hash);
                }
            }
            NetMsg::Pause { round, frame } => {
                if self.of_this_round(round) {
                    self.take_pause(host, from, frame);
                }
            }
            NetMsg::Resume { round, frame } => {
                if self.of_this_round(round) {
                    self.take_resume(host, from, frame);
                }
            }
            NetMsg::Start(start) if self.calls_the_next_round(&start) => {
                self.take_the_next_round(start);
            }
            NetMsg::Start(Invitation { names, deal, .. }) => {
                // Stale as a launch signal, but a joiner in the direct
                // PINCH_HOST/PINCH_JOIN pair, which never waits in a
                // lobby, learns the table's names from it. Only from this
                // round's deal: the host answers results-card greetings
                // with the last round's, whose seats were dealt before
                // somebody left, and one arriving late named every seat
                // wrong for the rest of the round.
                if !host && deal == self.rounds_dealt {
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
            // The host asked us to leave. It turns this address away
            // from now on, so there is no round left here for us.
            NetMsg::Kicked if !host => PutOut::hear(&mut self.put_out, PutOut::Kicked),
            NetMsg::NoBots if !host => PutOut::hear(&mut self.put_out, PutOut::NoBots),
            // Only a greeting from a stranger is answered so, and this peer
            // is one again only if the host forgot it and filled up since.
            NetMsg::Full if !host => PutOut::hear(&mut self.put_out, PutOut::Full),
            NetMsg::Kicked | NetMsg::NoBots | NetMsg::Full => {}
            // Somebody closed their socket. On the host, a seat it
            // held goes to the AI the moment the round waits on it,
            // and anyone else is forgotten at the next sweep. On a
            // joiner it is the host, and the round is over
            // (`host_gone`) without twenty seconds of a still beach.
            NetMsg::Leaving => {
                self.heard_a_goodbye(from);
                self.release_a_departed_pause(from);
            }
            // Only a host hands this out, and a host never receives
            // one: a joiner in the queue is still in the lobby.
            NetMsg::Queued { .. } => {}
            // The next round as it stands, reaching a joiner still in
            // the last one: the same as a `Start` for it, but walked
            // into part way through (`take_catch_up_part`).
            NetMsg::CatchUp {
                frame,
                part,
                parts,
                bytes,
            } => {
                self.take_catch_up_part(frame, part, parts, bytes);
            }
            // A spectator talking. Players have both hands on the keys
            // and no way to answer, which is the point: the people
            // with nothing to hold are the ones who can say something,
            // and the round's own feed is where it lands.
            NetMsg::Chat { name, text } => self.hear_chat(host, from, name, text),
            // The room's word, which only the host speaks: worded in this
            // peer's own language when the feed takes it.
            NetMsg::Notice(notice) => {
                if !host {
                    self.notices.push(notice);
                }
            }
            // Only the host counts. Every peer hears the answer, and
            // hears it as an action on a frame rather than as a claim
            // about one, so there is nothing here for anyone else to
            // agree with or miss.
            // The host's word on where the vote stands. It counts, so
            // it says; nobody else has anything to compare it against.
            NetMsg::SpectatorTally {
                watching,
                open,
                wait,
            } => {
                if !host {
                    self.stands.tide.crowd = crate::app::spectators::Crowd {
                        watching,
                        open,
                        wait,
                    };
                }
            }
            // Counted only from someone who is actually watching this
            // round. The sender was never looked at, so a seated
            // player could call tide events down on the table it was
            // playing at, and a peer in line for the next round, which
            // is sent no frames and has no board on screen, could vote
            // on this one. With nobody watching at all, one stray
            // datagram had the beach announcing what the spectators
            // had called.
            NetMsg::SpectatorVote { event } => self.take_vote(host, from, event),
            NetMsg::SpectatorPick { seat } => self.take_pick(host, from, seat),
            // The host's word on the calls, like the tally.
            NetMsg::CrowdPicks { picks, open } => {
                if !host {
                    self.stands.winner.picks = crate::app::spectators::Picks {
                        counts: picks,
                        open,
                    };
                }
            }
            NetMsg::Abandoned { round, seat, frame } => {
                if self.of_this_round(round) {
                    self.take_abandoned(host, seat, frame);
                }
            }
            // The lobby's business, and the lobby is behind us.
            NetMsg::Roster { .. } => {}
            NetMsg::Ping { stamp } => self.answer_ping(host, stamp),
            NetMsg::Pong { stamp } => self.take_pong(host, from, stamp),
            NetMsg::Clock { gap } => self.take_clock(host, gap),
        }
    }

    /// Host: a player's greeting mid-round, from one that has not heard
    /// its `Start` or one in line for the next round.
    fn player_greeted_mid_round(&mut self, from: usize, told: &str, bot: bool) {
        self.peers.row(from).bot = bot;
        // A player dealt a watching place for want of a chair, whose
        // `Start` went astray: the round as it stands, like any watcher
        // (`lost_its_start`).
        if self.lost_its_start(from) {
            self.owe_the_round(from);
        } else {
            let answer = self.answer_greeting(from, told, false);
            self.transport.send_to(from, answer);
        }
    }

    /// Host: a watcher's greeting mid-round.
    fn watcher_greeted_mid_round(&mut self, from: usize, told: &str) {
        if self.lost_its_start(from) {
            self.owe_the_round(from);
        } else if self.late_watcher(from) {
            // Somebody who came to watch after the launch is sent the
            // round as it stands, once this tick's frames are in, and
            // watches from there (`catch_up`). One from the launch is told
            // its place again, as ever. A greeting repeats until the
            // watcher is in, so a snapshot that went astray is simply sent
            // again, fresher.
            if !told.is_empty() {
                self.remember_peer_name(from, told);
            }
            self.note_watch_wish(from);
            self.peers.row(from).place = Place::LateWatching;
            self.owe_the_round(from);
        } else {
            let answer = self.answer_greeting(from, told, true);
            self.transport.send_to(from, answer);
        }
    }

    /// A peer's word for the tick: its inputs weeded (on the host) and
    /// believed, what it holds noted for what is sent it next, and the
    /// notices it carries taken as each would be on its own.
    ///
    /// Only a batch of the round this session plays: one of the last round,
    /// still on the wire when the next was dealt, or of the next, reaching
    /// a peer still finishing this one, names frames, seats and clocks of
    /// another lockstep. Read as this round's, a joiner's last inputs had
    /// the host's clock pull it hundreds of frames back for the whole of
    /// the next round, and say its `Start` had arrived when it had not.
    fn take_inputs(&mut self, host: bool, from: usize, batch: Batch) {
        let Batch {
            round,
            mut inputs,
            wants,
            pause,
            resume,
            abandoned,
            clock,
        } = batch;
        if !self.of_this_round(round) {
            return;
        }
        // The host knows which seat each peer was given, and takes inputs
        // for that seat alone: a peer speaking for another's seat (a bug,
        // or a spectator with ideas) would otherwise be believed by
        // whoever heard it first, and the seats would diverge. A joiner
        // hears only the host, which has already done this. The direct
        // `PINCH_HOST` pair keeps no plan for its first round, having never
        // been through a lobby; with nothing to check against it takes any
        // seat but its own. An empty plan at a table formed in the lobby is
        // not that: nobody on its socket was dealt a seat, so nobody's
        // inputs are taken, or anyone in line could play the AI's chair.
        //
        // Weeded before anything is believed *or* passed on: what the host
        // relays is what it accepted.
        if host {
            let unplanned = self.keeps_no_plan();
            let own = self.session.seat();
            let theirs = self.seat_of(from);
            inputs.retain(|input| match unplanned {
                true => Some(input.player) != own,
                false => theirs == Some(input.player),
            });
            // A tide event is the spectators' call, carried on the host's
            // own seat. From any other seat it is a player calling Monopoly
            // on their own behalf, which no key here makes and the host
            // does not pass on. Emptied rather than dropped: the frame
            // still needs the seat's input to run.
            for input in &mut inputs {
                if matches!(input.action, crate::sim::PlayerAction::CallEvent(_)) {
                    input.action = PlayerAction::None;
                }
            }
        }
        // The newest frame its sender has committed, for the clocks: on
        // the host each player's, on a joiner the host's own seat's. Only
        // from inputs this round takes (`Lockstep::receive`).
        let mut newest = None;
        for &input in &inputs {
            if self.session.receive(input) && (host || input.player == 0) {
                newest = newest.max(Some(input.frame));
            }
        }
        // What it holds, for what it is sent next.
        let wants = self.wants_of_this_round(wants);
        if host {
            // Its inputs are also what say its `Start` got there.
            self.peers.row(from).heard.take(newest, wants);
        } else {
            self.pace.host.take(newest, wants);
        }
        if let Some(frame) = pause {
            self.take_pause(host, from, frame);
        }
        if let Some(frame) = resume {
            self.take_resume(host, from, frame);
        }
        for (seat, frame) in abandoned {
            self.take_abandoned(host, seat, frame);
        }
        if let Some(gap) = clock {
            self.take_clock(host, gap);
        }
    }

    /// A resume off the wire.
    fn take_resume(&mut self, host: bool, from: usize, frame: u32) {
        // On the host, only a player's: anything else on the LAN that could
        // say `Resume` unpaused a table it was not at. A joiner hears only
        // the host.
        if !self.plays_this_round(from) {
            return;
        }
        // Only news is echoed and relayed. Re-arming on an echo would have
        // every peer answer every other peer's echoes, for the rest of the
        // match.
        if let Some(frame) = self.session.receive_resume(frame) {
            self.pace.resume_echo = RESUME_ECHOES;
            self.pace.clock_apart = None;
            if host {
                let round = self.round_tag();
                relay(&self.transport, from, NetMsg::Resume { round, frame });
            }
        }
    }

    /// The host's word that `seat` was given up on from `frame`: not our
    /// own patience. Idempotent, because it is repeated against packet
    /// loss.
    ///
    /// Only of a seat this round plays, at a frame it can have reached
    /// (`Lockstep::within_horizon`): a notice naming anything else is not
    /// about this round, and taken, it emptied a seat on this peer alone
    /// that every other peer went on playing.
    fn take_abandoned(&mut self, host: bool, seat: u8, frame: u32) {
        if host || !self.session.within_horizon(frame) || !self.session.plays(seat) {
            return;
        }
        if !self.abandoned.iter().any(|(gone, _)| *gone == seat) {
            self.session.abandon(seat, frame);
            self.abandoned.push((seat, frame));
        }
    }

    /// The host's clock, which only the host keeps.
    fn take_clock(&mut self, host: bool, gap: i16) {
        if !host {
            self.keep_time(gap);
        }
    }

    /// A pause off the wire, or a peer still announcing one lifted here.
    fn take_pause(&mut self, host: bool, from: usize, frame: u32) {
        // Past anything a peer at this table can name: a straggler from
        // the last round, or nobody's pause at all. The lockstep drops it
        // too (`receive_pause`); dropped here, the host does not relay it.
        if !self.session.within_horizon(frame) {
            return;
        }
        // A peer still announcing a pause this one has lifted missed every
        // repeat of the resume, and would sit frozen on it for good,
        // holding the table up while still talking: told again, to it
        // alone. Nothing loops, since a resume the peer already had is
        // not news to it.
        if self.session.is_lifted(frame) {
            let frame = self.resumed_frame();
            let round = self.round_tag();
            self.transport
                .send_to(from, NetMsg::Resume { round, frame });
            return;
        }
        // Only a player stops the table. A watcher calls no pause
        // (`request_pause`), and one still repeating the table's is
        // holding it already; anybody else on the LAN that can say
        // `Pause` froze a round it was not at. Answered above all the
        // same if it is announcing a pause lifted here.
        if !self.plays_this_round(from) {
            return;
        }
        let before = self.session.pause_frame();
        self.session.receive_pause(frame);
        // Only news is passed on: a pause that moved the host's own. Every
        // player repeats its pause every tick, and relaying each copy to
        // everyone else would have a paused six-seat table send more than
        // twice what a playing one does. A copy that says nothing new is already
        // on its way to every peer as the host's own `Pause`, which
        // `send_this_tick` repeats every tick and which carries the
        // earliest frame, the one every peer settles on.
        let news = before != Some(frame) && self.session.pause_frame() == Some(frame);
        if host && news {
            // Whose pause it is, for `release_a_departed_pause`: the peer
            // that told the host something it did not know. Every peer
            // repeats the pause it is holding on every tick, the one it
            // heard from somebody else included, so taking every `Pause`
            // as a claim put the host's own pause on every row, and the
            // first peer to leave lifted it for the table.
            self.peers.row(from).pause = Some(frame);
            let round = self.round_tag();
            relay(&self.transport, from, NetMsg::Pause { round, frame });
        }
    }

    /// Called by the tick closure after simulating a frame, with the
    /// board's state hash to take if this frame is one the peers compare.
    ///
    /// Asked for rather than handed over: hashing the board walks all of
    /// it, and only one frame in [`HASH_INTERVAL`] is compared. The same
    /// frames on every peer, since the schedule is the frame number.
    pub fn after_frame(&mut self, hash: impl FnOnce() -> u64) {
        let frame = self.session.frame();
        if frame.is_multiple_of(HASH_INTERVAL) {
            let hash = hash();
            self.hashes.record_own(frame, hash);
            let round = self.round_tag();
            self.transport.send(NetMsg::Hash { round, frame, hash });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The desync check has to work across the gap between two peers, and
    /// the gap runs both ways: a peer a commit lead ahead of us sends its
    /// hash for a frame we have not simulated yet.
    ///
    /// So a peer's hash is kept and weighed once our own for that frame
    /// appears. Comparing only what was already in hand skipped exactly the
    /// checks a lagging peer sends.
    #[test]
    fn a_peer_hash_that_arrives_early_is_still_weighed() {
        let mut hashes = HashCheck::default();
        // It arrives first, and there is nothing yet to hold it against.
        hashes.record_peer(0, 60, 0xBAD_B0A7);
        hashes.compare();
        assert_eq!(hashes.desync_at, None, "nothing to compare it to yet");
        // A frame in between, which both sides agree on, is not a desync.
        hashes.record_own(30, 0xC0FFEE);
        hashes.record_peer(0, 30, 0xC0FFEE);
        hashes.compare();
        assert_eq!(hashes.desync_at, None, "and they agree about frame 30");
        // We reach 60 at last, and the hash that has been waiting is what
        // catches it.
        hashes.record_own(60, 0x600D_B0A7);
        hashes.compare();
        assert_eq!(hashes.desync_at, Some(60), "the waiting hash caught it");
    }

    /// A round that agreed all the way through never raises one, however
    /// many hashes crossed: a loud check that cries on a healthy round is
    /// one nobody believes on a sick one.
    #[test]
    fn a_round_that_agrees_is_never_flagged_and_a_new_one_starts_level() {
        let mut hashes = HashCheck::default();
        for frame in (0..300).step_by(HASH_INTERVAL as usize) {
            let hash = u64::from(frame) * 0x9E37_79B9;
            hashes.record_own(frame, hash);
            hashes.record_peer(0, frame, hash);
            hashes.compare();
        }
        assert_eq!(hashes.desync_at, None, "every frame agreed");

        // Last round's disagreement is last round's. A new board starts
        // level, or one bad round would paint every round after it.
        hashes.record_own(30, 1);
        hashes.record_peer(0, 30, 2);
        hashes.compare();
        assert_eq!(hashes.desync_at, Some(30));
        hashes.reset();
        hashes.compare();
        assert_eq!(hashes.desync_at, None, "and the new board is level");
    }

    /// Only a player's hash can call the table out of step. A watcher that
    /// came apart, or anything on the LAN that can say `Hash`, put "desync"
    /// on the host's screen for the rest of the round, about a beach nobody
    /// was playing on.
    #[test]
    fn only_a_player_can_raise_a_desync() {
        let mut host = OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            Lockstep::new(0, vec![0, 1], crate::sim::DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        host.home.from_lobby = true;
        // Bo plays, Dee watches, and Cy is in line for the next round.
        host.peers.deal(&[Some(1), None]);
        host.peers.reach(3);
        host.hashes.record_own(30, 0xC0FFEE);
        for peer in [1, 2] {
            host.hear_mid_round(
                true,
                peer,
                NetMsg::Hash {
                    round: 0,
                    frame: 30,
                    hash: 1,
                },
            );
        }
        host.hashes.compare();
        assert_eq!(host.desync_at(), None, "a bystander's hash raised it");
        host.hear_mid_round(
            true,
            0,
            NetMsg::Hash {
                round: 0,
                frame: 30,
                hash: 1,
            },
        );
        host.hashes.compare();
        assert_eq!(host.desync_at(), Some(30), "and a player's still does");
    }

    /// The room's word, the crowd's tally and the count of calls are the
    /// host's to say: a peer sending any of them to the host is not
    /// believed, or a joiner could forge a line into the host's feed or a
    /// count onto its results card. A spoke takes each from the host. A
    /// line said mid-round reaches the feed on both, the host's under the
    /// name the peer greeted with.
    #[test]
    fn only_the_host_says_the_rooms_word_and_any_peer_can_chat() {
        use crate::app::spectators::{Crowd, Picks};
        use crate::transport::{Notice, wire_chat, wire_name};
        let words = || {
            [
                NetMsg::Notice(Notice::SentAway {
                    name: wire_name("Anna"),
                }),
                NetMsg::CrowdPicks {
                    picks: [0, 3, 0, 0, 0, 0],
                    open: 5,
                },
                NetMsg::SpectatorTally {
                    watching: 4,
                    open: 3,
                    wait: 2,
                },
                NetMsg::Chat {
                    name: wire_name("Bo"),
                    text: wire_chat("go P2"),
                },
            ]
        };
        let session = |seat| {
            OnlineSession::new(
                UdpTransport::host(0).expect("socket"),
                Lockstep::new(seat, vec![0, 1], crate::sim::DEFAULT_DELAY),
                2,
                MatchTerms::default(),
            )
        };

        let mut host = session(0);
        host.remember_peer_name(0, "Dee");
        for word in words() {
            host.hear_mid_round(true, 0, word);
        }
        assert_eq!(host.notices, [], "a forged notice");
        assert_eq!(host.stands.winner.picks, Picks::default(), "forged calls");
        assert_eq!(host.stands.tide.crowd, Crowd::default(), "a forged tally");
        assert_eq!(host.heard, [("Dee".to_string(), "go P2".to_string())]);

        let mut spoke = session(1);
        for word in words() {
            spoke.hear_mid_round(false, 0, word);
        }
        assert_eq!(
            spoke.notices,
            [Notice::SentAway {
                name: wire_name("Anna")
            }]
        );
        assert_eq!(
            spoke.stands.winner.picks,
            Picks {
                counts: [0, 3, 0, 0, 0, 0],
                open: 5
            }
        );
        assert_eq!(
            spoke.stands.tide.crowd,
            Crowd {
                watching: 4,
                open: 3,
                wait: 2
            }
        );
        assert_eq!(spoke.heard, [("Bo".to_string(), "go P2".to_string())]);
    }

    /// A bot turned away mid-round for being a bot walks out told the
    /// beach is not taking bots, the line the lobby gives it too, and not
    /// that the host asked it to leave: the host never singled it out.
    #[test]
    fn a_bot_turned_away_mid_round_hears_the_beach_is_not_taking_bots() {
        use crate::app::{RoundNotice, Screen, settings::GameSettings};
        let host = UdpTransport::host(0).expect("host socket");
        let port = host.local_addr().expect("addr").port();
        let mut joiner = OnlineSession::new(
            UdpTransport::join(("127.0.0.1", port)).expect("join"),
            Lockstep::new(1, vec![0, 1], crate::sim::DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        joiner.home.from_lobby = true;
        joiner.hear_mid_round(false, 0, NetMsg::NoBots);
        joiner.hear_mid_round(false, 0, NetMsg::Kicked);
        assert_eq!(joiner.put_out, Some(PutOut::NoBots));

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::Versus));
        app.insert_resource(GameSettings::default());
        app.init_resource::<RoundNotice>();
        app.insert_resource(Online(Some(joiner)));
        app.add_systems(Update, leave_a_hostless_round);
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Lobby
        );
        assert_eq!(
            app.world().resource::<RoundNotice>().0,
            crate::app::i18n::EN.lobby_no_bots,
        );
    }

    /// A player kicked on the interlude walks out then, to the lobby and
    /// told why, rather than after the interlude and the whole count; and
    /// the session armed for the round it will not play is let go.
    #[test]
    fn a_player_kicked_on_the_interlude_walks_out_at_once() {
        use crate::app::{RoundNotice, Screen, settings::GameSettings};
        let host = UdpTransport::host(0).expect("host socket");
        let port = host.local_addr().expect("addr").port();
        let mut joiner = OnlineSession::new(
            UdpTransport::join(("127.0.0.1", port)).expect("join"),
            Lockstep::new(1, vec![0, 1], crate::sim::DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        joiner.home.from_lobby = true;
        joiner.next_round = true;
        joiner.hear_mid_round(false, 0, NetMsg::Kicked);

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::Interlude));
        app.insert_resource(GameSettings::default());
        app.init_resource::<RoundNotice>();
        app.insert_resource(Online(Some(joiner)));
        app.add_systems(Update, leave_a_hostless_round);
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Lobby
        );
        assert_eq!(
            app.world().resource::<RoundNotice>().0,
            PutOut::Kicked.feedback(&crate::app::i18n::EN),
        );
        assert!(app.world().resource::<Online>().0.is_none());
    }

    /// A host whose table is Bo at seat 1 (peer 0), Dee watching (peer 1)
    /// and Cy in line for the next round (peer 2), at frame zero.
    fn table_of_three() -> OnlineSession {
        let mut host = OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            Lockstep::new(0, vec![0, 1], crate::sim::DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        host.home.from_lobby = true;
        host.peers.deal(&[Some(1), None]);
        host.peers.reach(3);
        host
    }

    /// The frame Bo's first pause of a round lands on.
    fn bos_first_pause() -> u32 {
        Lockstep::new(1, vec![0, 1], crate::sim::DEFAULT_DELAY)
            .request_pause()
            .expect("a player may pause")
    }

    /// The last round's stragglers, still on the wire when the next one
    /// began at frame zero: a hash for a frame near its end was kept with
    /// nothing to bound it, and a long round weighed it against its own
    /// frame of that number; a resume of its last pause set `lifted` near
    /// the horizon, and a player's early pause in the new round read as an
    /// echo of it. Both name frames past anything this round can have
    /// reached.
    #[test]
    fn the_last_rounds_stragglers_are_dropped() {
        let mut host = table_of_three();
        host.hear_mid_round(
            true,
            0,
            NetMsg::Hash {
                round: 0,
                frame: 1770,
                hash: 1,
            },
        );
        host.hear_mid_round(
            true,
            0,
            NetMsg::Resume {
                round: 0,
                frame: 1790,
            },
        );
        assert_eq!(host.session.lifted_pause(), None, "nothing was lifted");
        // Bo pauses early in the new round, and it is a pause.
        let early = bos_first_pause();
        host.hear_mid_round(
            true,
            0,
            NetMsg::Pause {
                round: 0,
                frame: early,
            },
        );
        assert_eq!(host.session.pause_frame(), Some(early));
        // And the round reaching frame 1770 finds no hash waiting there.
        host.hashes.record_own(1770, 2);
        host.hashes.compare();
        assert_eq!(host.desync_at(), None, "last round's hash raised it");
        // A hash this round can have reached is still weighed.
        host.hashes.record_own(30, 2);
        host.hear_mid_round(
            true,
            0,
            NetMsg::Hash {
                round: 0,
                frame: 30,
                hash: 1,
            },
        );
        host.hashes.compare();
        assert_eq!(host.desync_at(), Some(30));
    }

    /// Only a player stops the table or starts it again. A watcher, a peer
    /// in line, or anything else on the LAN that can say `Pause` froze a
    /// running round, and one `Resume` lifted the pause a player had
    /// called.
    #[test]
    fn only_a_player_pauses_or_resumes_the_table() {
        let mut host = table_of_three();
        let frame = bos_first_pause();
        for bystander in [1, 2] {
            host.hear_mid_round(true, bystander, NetMsg::Pause { round: 0, frame });
            assert!(!host.session.paused(), "peer {bystander} froze the table");
        }
        host.hear_mid_round(true, 0, NetMsg::Pause { round: 0, frame });
        assert_eq!(host.session.pause_frame(), Some(frame), "Bo's pause");
        assert_eq!(host.peers.get(0).and_then(|row| row.pause), Some(frame));
        for bystander in [1, 2] {
            host.hear_mid_round(true, bystander, NetMsg::Resume { round: 0, frame });
            assert!(host.session.paused(), "peer {bystander} lifted Bo's pause");
        }
        host.hear_mid_round(true, 0, NetMsg::Resume { round: 0, frame });
        assert!(!host.session.paused(), "and Bo's own Continue lifts it");
    }

    /// A pause is this round's. Bo's pause frame from the last round,
    /// kept on the row, matched the host's own pause at the same frame in
    /// the next, and Bo leaving lifted it for the table.
    #[test]
    fn a_pause_called_last_round_is_nobodys_in_this_one() {
        let mut host = table_of_three();
        let frame = bos_first_pause();
        host.hear_mid_round(true, 0, NetMsg::Pause { round: 0, frame });
        host.hear_mid_round(true, 0, NetMsg::Resume { round: 0, frame });
        assert_eq!(host.peers.get(0).and_then(|row| row.pause), Some(frame));

        host.begin_round(
            2,
            Some(0),
            MatchTerms::default(),
            Default::default(),
            crate::sim::DEFAULT_DELAY,
        );
        host.peers.deal(&[Some(1), None]);
        assert_eq!(host.peers.get(0).and_then(|row| row.pause), None);
        host.request_pause();
        assert_eq!(host.session.pause_frame(), Some(frame), "the same frame");
        host.hear_mid_round(true, 0, NetMsg::Leaving);
        assert!(
            host.session.paused(),
            "Bo's goodbye lifted the host's pause"
        );
    }

    /// The direct `PINCH_HOST` pair deals no plan for its first round,
    /// and its host takes its one peer's inputs for any seat but its own,
    /// and sends it the round. A table formed in the lobby with an empty
    /// plan is another thing: launched against the AI with nobody aboard,
    /// or with every planned peer gone, everybody on its socket is in line
    /// for the next round. Read as the direct pair, each was streamed the
    /// lockstep it has no board for, and could play any chair it liked.
    #[test]
    fn an_empty_plan_at_a_lobby_table_seats_nobody() {
        use crate::sim::{DEFAULT_DELAY, InputMsg};

        for from_lobby in [false, true] {
            let mut host = OnlineSession::new(
                UdpTransport::host(0).expect("socket"),
                Lockstep::new(0, vec![0, 1], DEFAULT_DELAY),
                2,
                MatchTerms::default(),
            );
            host.home.from_lobby = from_lobby;
            // One peer, never dealt a place.
            host.peers.reach(1);
            assert_eq!(host.peers.planned(), 0);
            host.session.commit_local(PlayerAction::None);
            while host.session.advance().is_some() {}
            assert_eq!(host.session.awaiting(), vec![1], "waiting on seat 1");

            host.take_inputs(
                true,
                0,
                Batch {
                    inputs: vec![InputMsg {
                        player: 1,
                        frame: DEFAULT_DELAY,
                        action: PlayerAction::None,
                    }],
                    ..Batch::default()
                },
            );
            assert_eq!(
                host.session.awaiting().is_empty(),
                !from_lobby,
                "seat 1 taken from a peer in line, from the lobby: {from_lobby}"
            );
            assert_eq!(
                host.follows_the_round(0),
                !from_lobby,
                "and sent the round, from the lobby: {from_lobby}"
            );
        }
    }

    /// A host and `n` joiners registered on its socket in order, so their
    /// peer indices are known, each with its own socket to read.
    fn aboard(players: Vec<u8>, n: usize) -> (OnlineSession, Vec<UdpTransport>) {
        let mut host = OnlineSession::new(
            UdpTransport::host(0).expect("host socket"),
            Lockstep::new(0, players, crate::sim::DEFAULT_DELAY),
            3,
            MatchTerms::default(),
        );
        host.home.from_lobby = true;
        let port = host.transport.local_addr().expect("addr").port();
        let mut joiners = Vec::new();
        for want in 1..=n {
            let joiner = UdpTransport::join(("127.0.0.1", port)).expect("join");
            joiner.send(NetMsg::hello("J"));
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                let _ = host.transport.recv_all();
                if host.transport.peer_count() == want {
                    break;
                }
            }
            assert_eq!(host.transport.peer_count(), want);
            joiners.push(joiner);
        }
        host.peers.reach(n);
        (host, joiners)
    }

    /// What a joiner's socket has been sent, once it has had a moment to
    /// arrive.
    fn heard(joiner: &mut UdpTransport) -> Vec<NetMsg> {
        std::thread::sleep(std::time::Duration::from_millis(20));
        joiner.recv_all().into_iter().map(|(msg, _)| msg).collect()
    }

    /// Every player repeats its pause every tick, and the host passes on
    /// only a copy that changed its own: relaying each copy to everyone
    /// would have a paused six-seat table send more than twice what a
    /// playing one does.
    /// The rest is already on its way as the host's own `Pause`.
    #[test]
    fn a_pause_that_is_no_news_is_not_relayed() {
        let (mut host, mut joiners) = aboard(vec![0, 1, 2], 2);
        host.peers.deal(&[Some(1), Some(2)]);
        let frame = bos_first_pause();
        for _ in 0..5 {
            host.hear_mid_round(true, 0, NetMsg::Pause { round: 0, frame });
        }
        let pauses = heard(&mut joiners[1])
            .into_iter()
            .filter(|msg| matches!(msg, NetMsg::Pause { .. }))
            .count();
        assert_eq!(pauses, 1, "the news, once");
        assert_eq!(host.session.pause_frame(), Some(frame));
    }

    /// A lost `Start` would leave a player off the table until its next
    /// greeting, the round frozen on its seat meanwhile. The host sends it
    /// again every few ticks until the player's inputs arrive, and to a
    /// watcher, which sends nothing in a round, while a frame-zero `Start`
    /// is still any use to it.
    #[test]
    fn a_start_goes_again_until_the_player_is_heard() {
        let (mut host, mut joiners) = aboard(vec![0, 1], 2);
        let kinds = [crate::sim::SeatKind::Human; MAX_PLAYERS];
        host.deal(&[Some(1), None], kinds, Vec::new(), None);
        // Both went astray.
        for joiner in &mut joiners {
            let _ = heard(joiner);
        }
        let starts = |joiner: &mut UdpTransport| {
            heard(joiner)
                .into_iter()
                .filter(|msg| matches!(msg, NetMsg::Start(_)))
                .count()
        };
        for _ in 0..START_AGAIN {
            host.pump(PlayerAction::None, |_| {});
        }
        assert_eq!(starts(&mut joiners[0]), 1, "the player is sent it again");
        assert_eq!(starts(&mut joiners[1]), 1, "and the watcher");
        // The player's first inputs are what say it is in, but only of
        // this round: the last round's, still on the wire, say nothing.
        let last_rounds = |round: u8| Batch {
            round,
            inputs: vec![crate::sim::InputMsg {
                player: 1,
                frame: 5400,
                action: PlayerAction::None,
            }],
            ..Batch::default()
        };
        host.take_inputs(true, 0, last_rounds(host.round_tag().wrapping_sub(1)));
        host.take_inputs(true, 0, last_rounds(host.round_tag()));
        for _ in 0..START_AGAIN {
            host.pump(PlayerAction::None, |_| {});
        }
        assert_eq!(starts(&mut joiners[0]), 1, "the last round's say nothing");
        assert_eq!(starts(&mut joiners[1]), 1);
        host.take_inputs(
            true,
            0,
            Batch {
                round: host.round_tag(),
                inputs: vec![crate::sim::InputMsg {
                    player: 1,
                    frame: crate::sim::DEFAULT_DELAY,
                    action: PlayerAction::None,
                }],
                ..Batch::default()
            },
        );
        for _ in 0..START_AGAIN {
            host.pump(PlayerAction::None, |_| {});
        }
        assert_eq!(starts(&mut joiners[0]), 0, "heard, so it has it");
        assert_eq!(starts(&mut joiners[1]), 1, "the watcher still might not");
    }

    /// A lockstep datagram from the last round, landing once the next one
    /// has begun, is dropped and counted, whatever frame it names. Every
    /// round counts its frames from zero, so frame 30 of the last round is
    /// a frame this one reaches too: only the round tag tells them apart.
    /// Sent over a real socket, the way a late packet arrives, and then
    /// the same words tagged with this round, which are taken.
    #[test]
    fn a_stale_rounds_packets_are_dropped_and_counted() {
        use crate::sim::{DEFAULT_DELAY, Direction, InputMsg};
        let (mut host, joiners) = aboard(vec![0, 1], 1);
        host.peers.deal(&[Some(1)]);
        // The second round on this socket: the last one's tag is one less.
        host.rounds_dealt = 2;
        let (now, last) = (host.round_tag(), host.round_tag().wrapping_sub(1));
        let pause = bos_first_pause();
        let bo = &joiners[0];
        let say = |round: u8| {
            let batch = Batch {
                round,
                inputs: vec![InputMsg {
                    player: 1,
                    frame: DEFAULT_DELAY,
                    action: PlayerAction::Place {
                        x: 3,
                        y: 3,
                        dir: Direction::Up,
                    },
                }],
                ..Batch::default()
            };
            bo.send_batch(&batch, |_| true);
            bo.send(NetMsg::Hash {
                round,
                frame: HASH_INTERVAL,
                hash: 1,
            });
            bo.send(NetMsg::Pause {
                round,
                frame: pause,
            });
            std::thread::sleep(std::time::Duration::from_millis(30));
        };
        let mut from = [None; MAX_PLAYERS];
        from[1] = Some(0);
        let bos = |host: &OnlineSession| host.session.held_between(&from, u32::MAX).len();
        say(last);
        for (msg, from) in host.transport.recv_all() {
            host.hear_mid_round(true, from, msg);
        }
        assert_eq!(host.stale_rounds, 3, "the batch, the hash and the pause");
        assert_eq!(bos(&host), 0, "its input was taken");
        assert!(!host.session.paused(), "its pause froze this round");
        host.hashes.record_own(HASH_INTERVAL, 2);
        host.hashes.compare();
        assert_eq!(host.desync_at(), None, "its hash raised a desync");

        say(now);
        for (msg, from) in host.transport.recv_all() {
            host.hear_mid_round(true, from, msg);
        }
        assert_eq!(host.stale_rounds, 3, "this round's are not stale");
        assert_eq!(bos(&host), 1, "this round's input");
        assert_eq!(host.session.pause_frame(), Some(pause));
        host.hashes.compare();
        assert_eq!(host.desync_at(), Some(HASH_INTERVAL));

        // A joiner drops the host's word that a seat was given up on, when
        // it is the last round's: taken, it emptied a seat on this peer
        // alone.
        let mut joiner = OnlineSession::new(
            UdpTransport::join(("127.0.0.1", 9)).expect("socket"),
            Lockstep::new(1, vec![0, 1, 2], DEFAULT_DELAY),
            3,
            MatchTerms::default(),
        );
        joiner.rounds_dealt = 2;
        let seat = |round| NetMsg::Abandoned {
            round,
            seat: 2,
            frame: 20,
        };
        joiner.hear_mid_round(false, 0, seat(last));
        assert!(joiner.abandoned.is_empty());
        assert_eq!(joiner.stale_rounds, 1);
        joiner.hear_mid_round(false, 0, seat(now));
        assert_eq!(joiner.abandoned, vec![(2, 20)]);
    }

    /// A joiner takes the table's names only from this round's `Start`: the
    /// last round's, the host's answer to a results-card greeting arriving
    /// late, dealt the seats before somebody left and moved everyone up.
    #[test]
    fn the_last_rounds_start_does_not_rename_this_ones_seats() {
        use crate::transport::wire_name;
        let mut joiner = OnlineSession::new(
            UdpTransport::join(("127.0.0.1", 9)).expect("socket"),
            Lockstep::new(1, vec![0, 1], crate::sim::DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        joiner.rounds_dealt = 2;
        joiner.names[1] = "Bo".to_string();
        let start = |deal, name| {
            let mut names = [wire_name(""); crate::sim::MAX_PLAYERS];
            names[1] = wire_name(name);
            NetMsg::Start(Invitation {
                names,
                deal,
                ..Invitation::blank()
            })
        };
        joiner.hear_mid_round(false, 0, start(1, "Cy"));
        assert_eq!(joiner.names[1], "Bo", "the last round's seats");
        joiner.hear_mid_round(false, 0, start(2, "Dee"));
        assert_eq!(joiner.names[1], "Dee", "this round's");
    }

    /// A joiner takes only this round's batches, and only a notice about
    /// a seat this round plays at a frame it can have reached: the last
    /// round's, still on the wire, gave up on a seat on this peer alone,
    /// and its clock and frames pulled this one's off the host's.
    #[test]
    fn the_last_rounds_batches_are_not_this_ones() {
        use crate::sim::InputMsg;
        let mut joiner = OnlineSession::new(
            UdpTransport::join(("127.0.0.1", 9)).expect("socket"),
            Lockstep::new(1, vec![0, 1, 2], crate::sim::DEFAULT_DELAY),
            3,
            MatchTerms::default(),
        );
        joiner.rounds_dealt = 2;
        let round = joiner.round_tag();
        let last = Batch {
            round: round.wrapping_sub(1),
            inputs: vec![InputMsg {
                player: 0,
                frame: 10,
                action: PlayerAction::None,
            }],
            abandoned: vec![(2, 20)],
            clock: Some(-500),
            ..Batch::default()
        };
        joiner.take_inputs(false, 0, last.clone());
        assert_eq!(joiner.stale_rounds, 1, "dropped, and counted");
        assert!(joiner.abandoned.is_empty(), "the last round's notice");
        assert_eq!(joiner.pace.host.newest, None, "the last round's inputs");
        // This round's, naming a frame it cannot have reached, or a seat it
        // does not play.
        for abandoned in [(2, 5000), (4, 20)] {
            joiner.take_inputs(
                false,
                0,
                Batch {
                    round,
                    abandoned: vec![abandoned],
                    ..Batch::default()
                },
            );
            assert!(joiner.abandoned.is_empty(), "{abandoned:?}");
        }
        joiner.take_inputs(false, 0, Batch { round, ..last });
        assert_eq!(joiner.abandoned, vec![(2, 20)], "this round's is taken");
        assert_eq!(joiner.pace.host.newest, Some(10));
    }

    /// The peers compare a hash one frame in [`HASH_INTERVAL`], and the
    /// board is hashed for those frames alone: on every peer the same
    /// ones, since the schedule is the frame number.
    #[test]
    fn only_the_frames_compared_are_hashed() {
        let mut solo = OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            Lockstep::new(0, vec![0], 0),
            2,
            MatchTerms::default(),
        );
        let mut hashed = Vec::new();
        for _ in 0..2 * HASH_INTERVAL {
            solo.session.commit_local(PlayerAction::None);
            solo.session.advance().expect("nobody to wait for");
            let frame = solo.session.frame();
            solo.after_frame(|| {
                hashed.push(frame);
                u64::from(frame)
            });
        }
        assert_eq!(hashed, vec![HASH_INTERVAL, 2 * HASH_INTERVAL]);
    }

    /// A player whose round began well after the host's is told so by the
    /// host's clock, and once two readings agree it commits extra empty
    /// frames to catch up, rather than leaving every input at the table
    /// waiting on its late clock for the rest of the round.
    #[test]
    fn a_late_player_is_nudged_level_with_the_host() {
        let mut joiner = OnlineSession::new(
            UdpTransport::join(("127.0.0.1", 9)).expect("socket"),
            Lockstep::new(1, vec![0, 1], crate::sim::DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        // The host has committed up to frame 20; this player not at all,
        // so the host's next commit runs 19 past the newest it has had.
        joiner.take_inputs(
            false,
            0,
            Batch {
                inputs: vec![crate::sim::InputMsg {
                    player: 0,
                    frame: 20,
                    action: PlayerAction::None,
                }],
                ..Batch::default()
            },
        );
        let gap = 21 - (crate::sim::DEFAULT_DELAY as i16 - 1);
        joiner.hear_mid_round(false, 0, NetMsg::Clock { gap });
        let before = joiner.session.next_commit();
        joiner.session.commit_local_with(PlayerAction::None, &[]);
        assert_eq!(
            joiner.session.next_commit(),
            before + 1,
            "one reading is not enough"
        );
        joiner.hear_mid_round(false, 0, NetMsg::Clock { gap });
        let before = joiner.session.next_commit();
        joiner.session.commit_local_with(PlayerAction::None, &[]);
        assert_eq!(
            joiner.session.next_commit(),
            before + 2,
            "two agree: catch up"
        );
    }

    /// Hashes are kept by peer index, and move down with the peers behind
    /// one that is forgotten: a departed peer's go with it.
    #[test]
    fn a_forgotten_peers_hashes_go_with_it() {
        let mut hashes = HashCheck::default();
        hashes.record_peer(0, 30, 2);
        hashes.record_peer(1, 30, 1);
        hashes.forget(0);
        hashes.record_own(30, 1);
        hashes.compare();
        assert_eq!(hashes.desync_at, None, "the one that disagreed has gone");
    }
}
