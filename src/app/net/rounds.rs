//! The round after this one: who is still owed a seat, who has been
//! waiting in line for one, and the terms everybody agrees to play the
//! next board on.
//!
//! All of it is the host's business. A joiner keeps no launch plan and
//! answers nobody, since the star's spokes cannot hear each other, so
//! every decision here is made once, at the hub, and sent out.

use super::*;

/// What a host's `Start` invites this peer to: the size of the table, the
/// chair (or none, for a watcher), the terms, who else is at it, where
/// the series stands, and the host's own beach when it sent one.
///
/// Named rather than left the tuple it was, because a tuple is read by
/// counting: `(seats, seat, ..)` is a count and an index next to each
/// other, and the only thing keeping them apart was their order. Taken up
/// by [`OnlineSession::take_up`] whether it arrives in the lobby, mid-round
/// or on the results card, so there is one way to begin a round.
pub struct Invitation {
    pub seats: u8,
    pub seat: Option<u8>,
    pub terms: MatchTerms,
    pub names: [crate::transport::WireName; MAX_PLAYERS],
    /// Where the series stands, as the host says. A peer that greeted
    /// mid-series is seated with the table's own tally rather than
    /// starting a fresh one. `None` for a single round.
    pub standing: Option<SeriesStanding>,
    /// The host's own beach, when it sent one. A joiner has never seen the
    /// file it came from, so it arrives with the invitation or not at all.
    pub beach: Vec<u8>,
}

/// The lockstep a peer plays a round over: the `humans` low seats, from
/// the given chair, or from the rail for a watcher.
pub(crate) fn lockstep_for(seat: Option<u8>, humans: u8) -> Lockstep {
    let players: Vec<u8> = (0..humans).collect();
    match seat {
        None => Lockstep::observer(players, crate::sim::DEFAULT_DELAY),
        Some(seat) => Lockstep::new(seat, players, crate::sim::DEFAULT_DELAY),
    }
}

impl OnlineSession {
    /// A session formed on a host's invitation, as a joiner's is from the
    /// lobby: the socket it greeted over, and everything the `Start` said.
    pub fn invited(transport: UdpTransport, invitation: Invitation) -> OnlineSession {
        let Invitation {
            seats, seat, terms, ..
        } = &invitation;
        let mut session = OnlineSession::new(
            transport,
            lockstep_for(*seat, terms.humans(*seats)),
            *seats,
            *terms,
        );
        session.take_up(invitation);
        session
    }

    /// Take up an invitation: the host's beach, the table's names, and a
    /// round begun afresh on its terms (see [`Self::begin_round`] for
    /// everything that resets). Does not arm `next_round`, which is the
    /// caller's to say: a session formed on its first invitation is
    /// walking into the arena, not back into it.
    pub fn take_up(&mut self, invitation: Invitation) {
        let Invitation {
            seats,
            seat,
            terms,
            names,
            standing,
            beach,
        } = invitation;
        self.beach = beach;
        self.begin_round(seats, seat, terms, table_from_wire(&names));
        self.series_standing = standing;
    }

    /// The answer for a peer the socket picked up *after* the launch, or
    /// `None` for one that was at the table when the round began.
    ///
    /// Membership is the launch plan, not the seat: a peer seated as a
    /// spectator in the lobby is `Watching` in the plan, while a stranger
    /// who greeted mid-round is `Queued`. Both answer `None` to `seat_of`,
    /// so the plan is the only thing that tells them apart. Admitting the
    /// second as a spectator was how a latecomer used to end up staring at
    /// frame zero forever.
    pub(super) fn queue_place(&self, peer: usize) -> Option<NetMsg> {
        debug_assert!(
            self.is_host() || self.peers.planned() == 0,
            "a joiner keeps no launch plan and must never answer with one"
        );
        let seated = self.peers.planned();
        // Peers are registered in arrival order, so everyone between the
        // plan and this one is already waiting.
        let ahead = peer.checked_sub(seated)?;
        Some(NetMsg::Queued {
            ahead: ahead.min(u8::MAX as usize) as u8,
        })
    }

    /// Drain the socket between rounds, when the sim is stopped and `pump`
    /// is therefore not running either.
    ///
    /// The results card is when people turn up wanting in, and when the
    /// host decides there will be another round, so the two messages that
    /// matter here are a greeting to queue and the invitation that answers
    /// it. Inputs and hashes belong to the round that just ended, and are
    /// dropped with it.
    ///
    /// A joiner also keeps greeting, which is not for the host's benefit:
    /// the answer is what proves the host is still there. Nothing else is
    /// sent between rounds, so without it the silence clock would grow on
    /// a table where everybody is present and simply reading the scores,
    /// and [`Self::host_gone`] would call the round off under them.
    pub fn poll_between_rounds(&mut self, delta: f32) {
        let host = self.is_host();
        self.age_the_silence(delta);
        if !host && crate::app::lobby::once_a_second(&mut self.home.greet_in, delta) {
            // The same greeting the lobby sends, wish to watch included: a
            // watcher on the results card used to greet as a nameless
            // player, which the host reads the same way between rounds
            // (a watcher's chair is in the launch plan already) but is
            // not what the peer means.
            let me = self
                .session
                .seat()
                .map_or("", |seat| &self.names[usize::from(seat)]);
            self.transport
                .send(crate::app::lobby::greeting(self.watching(), me));
        }
        for (msg, from) in self.transport.recv_all() {
            self.mark_heard(from);
            match msg {
                NetMsg::Hello { .. } | NetMsg::Watch => {
                    if host {
                        let answer = self
                            .queue_place(from)
                            .unwrap_or_else(|| self.start_msg(self.seat_of(from)));
                        self.transport.send_to(from, answer);
                    }
                }
                NetMsg::Start {
                    seats,
                    seat,
                    terms,
                    names,
                    standing,
                    beach,
                } => {
                    if !host && self.is_next_round(&terms) {
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
                }
                NetMsg::Input(_)
                | NetMsg::Hash { .. }
                | NetMsg::Pause { .. }
                | NetMsg::Resume { .. }
                | NetMsg::Queued { .. }
                | NetMsg::Chat { .. }
                | NetMsg::Roster { .. }
                | NetMsg::Abandoned { .. }
                | NetMsg::Incompatible { .. } => {}
            }
        }
    }

    /// Host: the table for the next round, admitting whoever queued while
    /// this one played.
    ///
    /// Seats are handed out in peer order as they were at the launch, so
    /// those still here keep theirs, and the queue fills what is left,
    /// pushing the AI back a seat at a time. Returns the new plan, one
    /// entry per peer, `None` for a peer that watches or that the table
    /// could not fit.
    fn next_plan(&self, peers: usize) -> Vec<Option<u8>> {
        let mut next = 1u8; // the host keeps seat 0
        (0..peers)
            .map(|peer| {
                // A watcher, whether it sat out the launch (at the rail in
                // the plan) or queued mid-round with W armed (a remembered
                // wish), keeps no chair.
                let watches = self.peers.get(peer).is_some_and(Peer::watches);
                if watches || usize::from(next) >= MAX_PLAYERS {
                    return None;
                }
                let seat = next;
                next += 1;
                Some(seat)
            })
            .collect()
    }

    /// Host: write down a peer's name against its socket index, growing the
    /// row to reach it. Kept for peers the seat table does not cover yet.
    pub(super) fn remember_peer_name(&mut self, peer: usize, name: &str) {
        self.peers.row(peer).name = name.to_string();
    }

    /// Host: note that a peer asked to watch rather than play.
    pub(super) fn note_watch_wish(&mut self, peer: usize) {
        self.peers.row(peer).watch = true;
    }

    /// The name a peer index goes by, from its greeting: its seat's name if
    /// it holds one, else what it greeted with while queued.
    pub(super) fn peer_name(&self, peer: usize) -> Option<&str> {
        if let Some(seat) = self.peers.seat_of(peer)
            && let Some(name) = self.names.get(usize::from(seat))
            && !name.is_empty()
        {
            return Some(name);
        }
        self.peers
            .get(peer)
            .map(|peer| peer.name.as_str())
            .filter(|name| !name.is_empty())
    }

    /// Host: call the next round on `terms`, admitting the queue, and tell
    /// every peer which seat it now holds. Arms this session too, so host
    /// and joiners take the same path back into the arena.
    ///
    /// `standing` is the series as it is now, by *this* round's seats, or
    /// `None` outside a series; the return value is the same standing
    /// re-dealt to the seats the next round hands out, which the caller
    /// folds back into its `Tournament`. Seats are re-dealt in peer order
    /// every round (they have to stay the contiguous `0..humans` the sim
    /// fills the top of with AI), so a peer that leaves shifts everyone
    /// behind it up a chair; carrying the wins across by hand is what
    /// keeps a survivor's rounds its own rather than the next player's.
    pub fn call_next_round(
        &mut self,
        mut terms: MatchTerms,
        standing: Option<SeriesStanding>,
    ) -> Option<SeriesStanding> {
        let peers = self.transport.peer_count();
        let plan = self.next_plan(peers);
        let standing = standing.map(|SeriesStanding { round, wins }| {
            // The wins follow the chairs: seat 0 is always the host's, and
            // each peer's new seat inherits what its old seat had won.
            let mut new_wins = [0u8; MAX_PLAYERS];
            new_wins[0] = wins[0];
            for (peer, slot) in plan.iter().enumerate() {
                if let Some(new_seat) = slot
                    && let Some(old_seat) = self.peers.seat_of(peer)
                {
                    new_wins[usize::from(*new_seat)] = wins[usize::from(old_seat)];
                }
            }
            // The number of the round about to begin: the caller passes
            // the one just played, and every peer shows the same next
            // number without each counting for itself.
            SeriesStanding {
                round: round.saturating_add(1),
                wins: new_wins,
            }
        });
        let humans = 1 + plan.iter().flatten().count() as u8;
        // A beach needs two castles, and a host whose table has emptied is
        // still entitled to another round, against the AI, since playing
        // itself is not a round. The same floor `seat_count` keeps.
        terms.bots = terms
            .bots
            .min(MAX_PLAYERS as u8 - humans)
            .max(2u8.saturating_sub(humans));
        let seats = humans + terms.bots;
        // Names travel with the invitation, as they did at the launch: a
        // player admitted from the queue is a stranger to every other
        // screen until this says otherwise.
        let mut names: [String; MAX_PLAYERS] = Default::default();
        names[0].clone_from(&self.names[0]);
        for (peer, slot) in plan.iter().enumerate() {
            if let Some(seat) = slot
                && let Some(name) = self.peer_name(peer)
            {
                names[usize::from(*seat)] = name.to_string();
            }
        }
        let wire = wire_table(&names);
        for (peer, slot) in plan.iter().enumerate() {
            self.transport.send_to(
                peer,
                NetMsg::Start {
                    seats,
                    seat: *slot,
                    terms,
                    names: wire,
                    standing,
                    beach: self.beach.clone(),
                },
            );
        }
        self.peers.deal(&plan);
        self.begin_round(seats, Some(0), terms, names);
        self.series_standing = standing;
        self.next_round = true;
        standing
    }

    /// Take up the terms of a new round: a fresh board, a lockstep back at
    /// frame zero, and whatever the table is called now.
    ///
    /// The seed is what marks this a *new* round rather than the stale
    /// `Start` a host re-answers stray greetings with, so a caller that
    /// does not change it will find nothing happens.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn begin_round(
        &mut self,
        seats: u8,
        seat: Option<u8>,
        terms: MatchTerms,
        names: [String; MAX_PLAYERS],
    ) {
        debug_assert!(
            (2..=MAX_PLAYERS as u8).contains(&seats),
            "a round for {seats} seats, which no per-seat array can hold"
        );
        debug_assert!(
            seat.is_none_or(|seat| seat < seats),
            "seated at {seat:?} of {seats}"
        );
        self.session = lockstep_for(seat, terms.humans(seats));
        self.seats = seats;
        self.terms = terms;
        self.names = names;
        self.hashes.reset();
        self.resume_echo = 0;
        // And nobody is late for a round that has not begun. The results
        // card is a place a table sits for a while, and carrying that
        // silence into the new round would call the host gone on its first
        // frame.
        self.stall.reset(self.session.frame());
        self.peers.hush();
    }

    /// Whether a `Start` off the wire is the next round rather than the
    /// stale one a host re-answers a greeting with.
    pub fn is_next_round(&self, terms: &MatchTerms) -> bool {
        terms.seed != self.terms.seed
    }

    /// The `Start` the host answers a stray greeting with: the terms plus
    /// the current name table, so a joiner that missed the launch still
    /// learns what everyone is called.
    pub(super) fn start_msg(&self, seat: Option<u8>) -> NetMsg {
        // The stale `Start` a greeting is re-answered with carries the same
        // seed as the round in play, so a joiner reads it for the names and
        // does not mistake it for a fresh round; the series standing rides
        // along all the same, in case this is the message that seats a
        // latecomer next round.
        NetMsg::Start {
            seats: self.seats,
            seat,
            terms: self.terms,
            names: wire_table(&self.names),
            standing: self.series_standing,
            beach: self.beach.clone(),
        }
    }

    /// The seat a peer index holds, or `None` for a watcher, and for a
    /// peer the plan has never heard of. The host keeps the same list the
    /// lobby handed out, so a late `Hello` is answered with the seat that
    /// peer already has.
    pub(super) fn seat_of(&self, peer: usize) -> Option<u8> {
        self.peers.seat_of(peer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::DEFAULT_DELAY;
    use crate::transport::UdpTransport;

    /// A session as the host holds it: seat 0, with a launch plan naming
    /// everyone who was at the table when the round began.
    fn hosting(plan: Vec<Option<u8>>) -> OnlineSession {
        let transport = UdpTransport::host(0).expect("game socket");
        let players = (0..=plan.iter().flatten().count() as u8).collect();
        let mut session = OnlineSession::new(
            transport,
            Lockstep::new(0, players, DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        session.peers.deal(&plan);
        session
    }

    /// The launch plan is what membership means, not the seat number. A
    /// peer seated as a spectator in the lobby and a stranger who greeted
    /// mid-round both answer `None` to `seat_of`, and only one of them can
    /// be served: lockstep replays from frame zero, so the stranger would
    /// build a board nobody will ever send it inputs for.
    #[test]
    fn a_latecomer_is_queued_and_a_lobby_spectator_is_not() {
        // Two peers at launch: one seated, one watching.
        let session = hosting(vec![Some(1), None]);
        assert_eq!(session.queue_place(0), None, "a seated peer plays on");
        assert_eq!(
            session.queue_place(1),
            None,
            "and so does one that came to watch: it has been in step since \
             frame zero, which is the whole difference"
        );
        // Anyone the socket picked up afterwards is in line, in arrival
        // order, and told how many are in front of them.
        assert_eq!(session.queue_place(2), Some(NetMsg::Queued { ahead: 0 }));
        assert_eq!(session.queue_place(3), Some(NetMsg::Queued { ahead: 1 }));
        assert_eq!(session.queue_place(9), Some(NetMsg::Queued { ahead: 7 }));
    }

    /// A joiner holds no plan at all, and answers nobody: the star's spokes
    /// never field a greeting.
    #[test]
    fn a_joiner_queues_nobody() {
        let session = hosting(Vec::new());
        assert!(!session.is_host() || session.peers.planned() == 0);
        // With an empty plan every peer index looks late, which is only ever
        // consulted on the host; the guard that matters is `if host`.
        assert_eq!(session.queue_place(0), Some(NetMsg::Queued { ahead: 0 }));
    }
}

#[cfg(test)]
mod next_round_tests {
    use super::*;
    use crate::sim::DEFAULT_DELAY;
    use crate::transport::UdpTransport;

    fn terms(seed: u64) -> MatchTerms {
        MatchTerms {
            seed,
            series: 1,
            ..MatchTerms::default()
        }
    }

    /// A host and a real joiner over loopback, played to the point where
    /// the host calls another round. The invitation has to reach the joiner
    /// and rearm it on the same terms, or the two rebuild different beaches
    /// and the round is a desync from frame zero.
    #[test]
    fn a_called_round_rearms_both_ends_on_the_same_terms() {
        let mut host = OnlineSession::new(
            UdpTransport::host(0).expect("host socket"),
            Lockstep::new(0, vec![0, 1], DEFAULT_DELAY),
            2,
            terms(111),
        );
        let port = host.transport.local_addr().expect("addr").port();
        let mut joiner = OnlineSession::new(
            UdpTransport::join(("127.0.0.1", port)).expect("join"),
            Lockstep::new(1, vec![0, 1], DEFAULT_DELAY),
            2,
            terms(111),
        );
        // The greeting is what registers the joiner as a peer host-side.
        joiner.transport.send(NetMsg::hello("Bo"));
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            host.poll_between_rounds(0.0);
            if host.transport.peer_count() == 1 {
                break;
            }
        }
        assert_eq!(host.transport.peer_count(), 1, "the joiner is at the table");
        host.peers.deal(&[Some(1)]);
        host.names[0] = "Anna".into();
        host.names[1] = "Bo".into();

        host.call_next_round(terms(222), None);
        assert!(host.next_round, "the host arms itself along with the table");
        assert_eq!(host.terms.seed, 222);

        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            joiner.poll_between_rounds(0.0);
            if joiner.next_round {
                break;
            }
        }
        assert!(joiner.next_round, "the invitation reached the joiner");
        assert_eq!(
            joiner.terms, host.terms,
            "and both will build the same beach from it"
        );
        assert_eq!(joiner.seats, host.seats);
        assert_eq!(joiner.names[0], "Anna", "the table travels with it");
        assert_eq!(joiner.session.seat(), Some(1), "keeping its own chair");
        assert_eq!(joiner.session.frame(), 0, "a new round starts at zero");
        assert_eq!(host.session.frame(), 0);
    }

    /// The stale `Start` a host re-answers a stray greeting with must not
    /// read as a new round, or every late hello would restart the match.
    #[test]
    fn the_same_seed_is_not_a_new_round() {
        let session = OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            Lockstep::new(0, vec![0], DEFAULT_DELAY),
            2,
            terms(111),
        );
        assert!(!session.is_next_round(&terms(111)), "the round it is in");
        assert!(
            session.is_next_round(&terms(112)),
            "a beach it has not seen"
        );
    }

    /// Whoever queued while the round played gets a chair in the next one,
    /// and the AI gives way to them rather than the other way about.
    #[test]
    fn the_queue_is_seated_next_round() {
        let mut host = OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            Lockstep::new(0, vec![0, 1], DEFAULT_DELAY),
            2,
            terms(1),
        );
        // One peer played, one watched, two turned up while they played.
        host.peers.deal(&[Some(1), None]);
        let plan = host.next_plan(4);
        assert_eq!(
            plan,
            vec![Some(1), None, Some(2), Some(3)],
            "the player keeps its seat, the watcher keeps watching, and the \
             two who waited are seated behind them"
        );
        // The AI takes what the humans leave, however much the terms asked
        // for. No peer ever connected to this socket, so the table the call
        // actually plans is the host alone, five chairs for the AI.
        let mut greedy = terms(2);
        greedy.bots = 5;
        host.call_next_round(greedy, None);
        assert_eq!(host.seats, MAX_PLAYERS as u8);
        assert_eq!(host.terms.bots, MAX_PLAYERS as u8 - 1);
        // And the plan is drawn from who is actually connected, not from
        // last round's list: a peer that left does not hold a chair.
        assert!(host.peers.is_empty(), "nobody is connected any more");
    }
}
