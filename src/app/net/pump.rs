//! The per-tick pump: what a session sends every fixed tick, what it
//! does with everything it hears while a round is played, and the state
//! hashes that keep the table honest about playing the same one.

use super::*;

/// How many ticks a resume is repeated after the local session unpauses.
/// A second's worth: enough to ride out a loss burst, and harmless if the
/// peers already heard the first one.
pub(super) const RESUME_ECHOES: u8 = 30;

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
        let committed = !armed(self)
            && self
                .session
                .commit_local_with(local_action, extra)
                .is_some();
        self.send_this_tick();
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
        if armed(self) {
            return committed;
        }
        // At most a couple of frames per fixed tick: a lagging peer catches
        // up gradually instead of spiralling.
        for _ in 0..2 {
            tick(self);
        }
        self.hashes.compare();
        committed
    }

    /// What goes out every tick whatever was heard: this peer's inputs,
    /// the pause state, and on the host every seat it has given up on.
    fn send_this_tick(&mut self) {
        // The newest commit and the whole resend tail behind it, in one
        // datagram: see `NetMsg::Inputs` for what that is worth at a full
        // table. To the table and the watchers, never to the queue: a peer in
        // line is not simulating anything (`Self::follows_the_round`).
        self.transport
            .send_inputs(self.session.recent_commits(), |peer| {
                self.follows_the_round(peer)
            });
        // Pause state is repeated every tick rather than sent once: UDP
        // drops, and the peer that misses a Pause would otherwise sit
        // watching a frozen beach with no card, while a missed Resume would
        // leave the session stopped for good. Both self-heal: the resume
        // repeats for `RESUME_ECHOES` ticks, and a peer that missed every
        // repeat is still sending its `Pause`, which is answered with the
        // resume again where it arrives.
        match self.session.pause_frame() {
            Some(frame) => self.transport.send(NetMsg::Pause { frame }),
            None if self.resume_echo > 0 => {
                self.resume_echo -= 1;
                self.transport.send(self.resume_msg());
            }
            None => {}
        }
        // Likewise every seat given up on, for the rest of the round: a
        // joiner that misses the one notice waits on that seat forever,
        // and the host never notices, because a joiner that is waiting is
        // still talking. Five bytes a seat a tick.
        //
        // With the notice, the seat's last inputs before that frame: the
        // host played them, and a peer whose relay of one was lost has
        // nobody else to hear it from now the player has gone quiet.
        if self.is_host() {
            for &(seat, frame) in &self.abandoned {
                self.transport.send(NetMsg::Abandoned { seat, frame });
                let owed = self.session.owed_by(seat, frame);
                if !owed.is_empty() {
                    self.transport
                        .send_inputs(&owed, |peer| self.follows_the_round(peer));
                }
            }
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
            NetMsg::Inputs(inputs) => self.take_inputs(host, from, inputs),
            NetMsg::Hash { frame, hash } => {
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
                if self.plays_this_round(from) && self.session.within_horizon(frame) {
                    self.hashes.record_peer(from, frame, hash);
                }
            }
            NetMsg::Pause { frame } => self.take_pause(host, from, frame),
            NetMsg::Resume { frame } => {
                // On the host, only a player's: anything else on the LAN
                // that could say `Resume` unpaused a table it was not at.
                // A joiner hears only the host.
                if !self.plays_this_round(from) {
                    return;
                }
                // Only news is echoed and relayed. Re-arming on an echo
                // would have every peer answer every other peer's
                // echoes, for the rest of the match.
                if let Some(frame) = self.session.receive_resume(frame) {
                    self.resume_echo = RESUME_ECHOES;
                    if host {
                        relay(&self.transport, from, NetMsg::Resume { frame });
                    }
                }
            }
            NetMsg::Start(start) if self.calls_the_next_round(&start) => {
                self.take_the_next_round(start);
            }
            NetMsg::Start(Invitation { names, .. }) => {
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
            // The host asked us to leave. It turns this address away
            // from now on, so there is no round left here for us.
            NetMsg::Kicked | NetMsg::NoBots => self.kicked |= !host,
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

    /// Inputs off the wire: weeded (on the host), believed, and passed on
    /// (by the host).
    fn take_inputs(&mut self, host: bool, from: usize, mut inputs: Vec<crate::sim::InputMsg>) {
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
        for &input in &inputs {
            self.session.receive(input);
        }
        if host {
            self.transport
                .send_inputs(&inputs, |peer| peer != from && self.follows_the_round(peer));
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
            self.transport.send_to(from, self.resume_msg());
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
        if host {
            // Whose pause it is, for `release_a_departed_pause`: the peer
            // that told the host something it did not know. Every peer
            // repeats the pause it is holding on every tick, the one it
            // heard from somebody else included, so taking every `Pause`
            // as a claim put the host's own pause on every row, and the
            // first peer to leave lifted it for the table.
            let news = before != Some(frame) && self.session.pause_frame() == Some(frame);
            if news {
                self.peers.row(from).pause = Some(frame);
            }
            relay(&self.transport, from, NetMsg::Pause { frame });
        }
    }

    /// Called by the tick closure after simulating a frame, with the fresh
    /// state hash.
    pub fn after_frame(&mut self, hash: u64) {
        let frame = self.session.frame();
        if frame.is_multiple_of(HASH_INTERVAL) {
            self.hashes.record_own(frame, hash);
            self.transport.send(NetMsg::Hash { frame, hash });
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
            crate::transport::UdpTransport::host(0).expect("socket"),
            crate::sim::Lockstep::new(0, vec![0, 1], crate::sim::DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        host.home.from_lobby = true;
        // Bo plays, Dee watches, and Cy is in line for the next round.
        host.peers.deal(&[Some(1), None]);
        host.peers.reach(3);
        host.hashes.record_own(30, 0xC0FFEE);
        for peer in [1, 2] {
            host.hear_mid_round(true, peer, NetMsg::Hash { frame: 30, hash: 1 });
        }
        host.hashes.compare();
        assert_eq!(host.desync_at(), None, "a bystander's hash raised it");
        host.hear_mid_round(true, 0, NetMsg::Hash { frame: 30, hash: 1 });
        host.hashes.compare();
        assert_eq!(host.desync_at(), Some(30), "and a player's still does");
    }

    /// A host whose table is Bo at seat 1 (peer 0), Dee watching (peer 1)
    /// and Cy in line for the next round (peer 2), at frame zero.
    fn table_of_three() -> OnlineSession {
        let mut host = OnlineSession::new(
            crate::transport::UdpTransport::host(0).expect("socket"),
            crate::sim::Lockstep::new(0, vec![0, 1], crate::sim::DEFAULT_DELAY),
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
        crate::sim::Lockstep::new(1, vec![0, 1], crate::sim::DEFAULT_DELAY)
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
                frame: 1770,
                hash: 1,
            },
        );
        host.hear_mid_round(true, 0, NetMsg::Resume { frame: 1790 });
        assert_eq!(host.session.lifted_pause(), None, "nothing was lifted");
        // Bo pauses early in the new round, and it is a pause.
        let early = bos_first_pause();
        host.hear_mid_round(true, 0, NetMsg::Pause { frame: early });
        assert_eq!(host.session.pause_frame(), Some(early));
        // And the round reaching frame 1770 finds no hash waiting there.
        host.hashes.record_own(1770, 2);
        host.hashes.compare();
        assert_eq!(host.desync_at(), None, "last round's hash raised it");
        // A hash this round can have reached is still weighed.
        host.hashes.record_own(30, 2);
        host.hear_mid_round(true, 0, NetMsg::Hash { frame: 30, hash: 1 });
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
            host.hear_mid_round(true, bystander, NetMsg::Pause { frame });
            assert!(!host.session.paused(), "peer {bystander} froze the table");
        }
        host.hear_mid_round(true, 0, NetMsg::Pause { frame });
        assert_eq!(host.session.pause_frame(), Some(frame), "Bo's pause");
        assert_eq!(host.peers.get(0).and_then(|row| row.pause), Some(frame));
        for bystander in [1, 2] {
            host.hear_mid_round(true, bystander, NetMsg::Resume { frame });
            assert!(host.session.paused(), "peer {bystander} lifted Bo's pause");
        }
        host.hear_mid_round(true, 0, NetMsg::Resume { frame });
        assert!(!host.session.paused(), "and Bo's own Continue lifts it");
    }

    /// A pause is this round's. Bo's pause frame from the last round,
    /// kept on the row, matched the host's own pause at the same frame in
    /// the next, and Bo leaving lifted it for the table.
    #[test]
    fn a_pause_called_last_round_is_nobodys_in_this_one() {
        let mut host = table_of_three();
        let frame = bos_first_pause();
        host.hear_mid_round(true, 0, NetMsg::Pause { frame });
        host.hear_mid_round(true, 0, NetMsg::Resume { frame });
        assert_eq!(host.peers.get(0).and_then(|row| row.pause), Some(frame));

        host.begin_round(2, Some(0), MatchTerms::default(), Default::default());
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
                crate::transport::UdpTransport::host(0).expect("socket"),
                crate::sim::Lockstep::new(0, vec![0, 1], DEFAULT_DELAY),
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
                vec![InputMsg {
                    player: 1,
                    frame: DEFAULT_DELAY,
                    action: PlayerAction::None,
                }],
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
