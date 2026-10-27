//! The peers on a socket, as the shell knows them: one row per peer
//! index, kept in step with the transport's own list.
//!
//! As four parallel `Vec`s (seat, name, watch wish, silence), each was
//! grown to reach a new index by its own loop and shifted by hand when a
//! peer was forgotten: four lists that have to move together or not at
//! all. One row per peer cannot come apart.

/// Where a peer stands in the round, as the launch plan dealt it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Place {
    /// At the table since the launch, holding this chair.
    Seated(u8),
    /// Watching since the launch: in step from frame zero like every
    /// player, but holding no chair.
    Watching,
    /// Not in the launch plan, but watching this round all the same: came
    /// to watch after the launch and was sent the round as it stood
    /// (`net::catch_up`), so it follows from that frame on.
    ///
    /// Its own place rather than `Watching`, because the plan is the
    /// leading run of placed rows ([`PeerBook::planned`]), and a latecomer
    /// counted into it would be answered as a launch watcher: sent a
    /// `Start` for frame zero, which is the one thing lockstep cannot
    /// give anybody halfway through a round.
    LateWatching,
    /// Not in the launch plan: turned up mid-round and is in line for the
    /// next one. Also every peer a joiner or the direct `PINCH_HOST` pair
    /// knows, which keep no plan at all.
    #[default]
    Queued,
}

/// One peer, by its index on the socket.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Peer {
    pub place: Place,
    /// What it calls itself, from its greeting; empty until it says.
    pub name: String,
    /// Whether it asked to watch rather than play. Kept apart from the
    /// place because a wish greeted mid-round has no place to live in
    /// until the next round deals one.
    pub watch: bool,
    /// Seconds since it was last heard from: anything at all, an input, a
    /// hash, a stray greeting.
    pub silence: f32,
    /// The seat it called to win this round, as a spectator. On the row
    /// so it moves with the peer when one before it is forgotten.
    pub pick: Option<u8>,
    /// Its greeting said a bot drives its seat. The game behind the
    /// greeting sets this, never the name, so the seat wears the robot.
    pub bot: bool,
    /// Whether it said it was going, and how long ago.
    ///
    /// The real peer is still sending on every tick it runs, so the first
    /// word from it after a forged goodbye takes it back well inside the
    /// grace a seat gets before it is handed to the AI.
    pub goodbye: Goodbye,
    /// Host: the frame of the last pause it announced. It holds the
    /// table's pause while that one is in flight (see
    /// `OnlineSession::release_a_departed_pause`).
    pub pause: Option<u32>,
    /// Host: how long its last few answers to a `Ping` took, which picks
    /// the input delay of the next round it is dealt a chair in.
    pub round_trips: RoundTrips,
    /// Host: the newest frame it has committed in this round, as far as
    /// its inputs have arrived: what keeps its clock against the host's
    /// (`NetMsg::Clock`). `None` until its first inputs, which is also
    /// what says its `Start` got there; until then the host sends the
    /// `Start` again every few ticks.
    pub newest: Option<u32>,
    /// Host: the frames it last said it was missing, per seat
    /// (`transport::Batch::wants`), which what it is sent each tick is cut
    /// from. `None` until it says, and sent the whole resend tail till then.
    pub wants: Option<[u32; crate::sim::MAX_PLAYERS]>,
    /// Host, in the lobby: frozen at the table the host is picking the map
    /// for (`lobby::pick`), which the launch seats. On the row so it moves
    /// with the peer when one before it is forgotten, and goes with it when
    /// it leaves. Written afresh each time the map page opens, and read
    /// only while it is up.
    pub frozen: bool,
    /// Host: the seat the host's partner picks put it in
    /// ([`super::order`]), which every deal puts it back in while that
    /// seat is at the table. On the row so it moves with the peer.
    pub seat_wish: Option<u8>,
}

/// How many round trips a peer's row remembers: a few seconds of pings.
const ROUND_TRIPS: usize = 5;

/// How old a round trip may be, in seconds, and still say how the link
/// is now: a few pings' worth.
const FRESH_FOR: f32 = 8.0;

/// The last few round trips the host measured to one peer, in seconds,
/// each with how long ago it was measured.
///
/// What counts is the slow end, not a mean: an input that misses its frame
/// stalls the whole table, so a link that is quick most of the time and
/// slow now and then has to be played as the slow one. But not the one
/// slowest answer of a few: a single hitch (a busy frame on either
/// machine, one Wi-Fi retry) set the whole next round to the longest delay
/// there is. So the second slowest, once there are three to choose from,
/// of the answers still fresh ([`FRESH_FOR`]): the pings stop while a
/// round is played, and what the link did before it says little about it
/// on the results card a few minutes later, once the card's own pings are
/// in.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct RoundTrips {
    samples: [(f32, f32); ROUND_TRIPS],
    taken: usize,
}

impl RoundTrips {
    /// One more answer, `seconds` after its ping went out. The oldest is
    /// forgotten once there are more than [`ROUND_TRIPS`].
    pub fn note(&mut self, seconds: f32) {
        self.samples[self.taken % ROUND_TRIPS] = (seconds, 0.0);
        self.taken += 1;
    }

    /// `delta` seconds more since every answer remembered.
    pub fn age(&mut self, delta: f32) {
        for (_, age) in &mut self.samples {
            *age += delta;
        }
    }

    /// The round trip to play this link as, or `None` before its first
    /// answer: the second slowest of the fresh answers, or the slowest of
    /// one or two. With none fresh, the same of the answers there are,
    /// stale as they are: a link last heard a while ago is still better
    /// known than one never heard.
    pub fn telling(&self) -> Option<f32> {
        let remembered = &self.samples[..self.taken.min(ROUND_TRIPS)];
        let fresh: Vec<f32> = remembered
            .iter()
            .filter(|(_, age)| *age <= FRESH_FOR)
            .map(|(seconds, _)| *seconds)
            .collect();
        let mut chosen = match fresh.is_empty() {
            true => remembered.iter().map(|(seconds, _)| *seconds).collect(),
            false => fresh,
        };
        chosen.sort_by(f32::total_cmp);
        let hitch = usize::from(chosen.len() >= 3);
        chosen.iter().rev().nth(hitch).copied()
    }
}

impl Peer {
    /// The chair it holds, if it holds one.
    pub fn seat(&self) -> Option<u8> {
        match self.place {
            Place::Seated(seat) => Some(seat),
            Place::Watching | Place::LateWatching | Place::Queued => None,
        }
    }

    /// Whether it asked to watch rather than play, which is what the next
    /// deal reads.
    ///
    /// Not whether it holds no chair: a player dealt none because the
    /// table was full did not ask to watch, and is seated when one frees.
    /// Read off the place, that player was a watcher for good, every round
    /// of a series and every launch after, while the AI took the chairs.
    /// (A late watcher holds its place because it asked, so the wish says
    /// that too.)
    pub fn watches(&self) -> bool {
        self.watch
    }

    /// Whether it is owed a chair at the next deal without holding one:
    /// in line for the next round, or sitting this one out because the
    /// table was full when it was dealt. Either way, not by wish.
    pub fn wants_a_chair(&self) -> bool {
        !self.watch && self.seat().is_none()
    }

    /// Whether it said it was going (`NetMsg::Leaving`), and has said
    /// nothing since.
    pub fn left(&self) -> bool {
        self.goodbye.said()
    }
}

/// A goodbye (`NetMsg::Leaving`) and the seconds it has stood, from its
/// first copy, with no other word from whoever said it.
///
/// Its own clock, because a goodbye is a claim nothing checks: any machine
/// on the LAN can send one with the peer's address on it. Believed on
/// arrival, one forged datagram emptied a table. So it is believed only
/// once it has stood for a grace (`stood`), and any other word from the
/// same peer takes it back (`heard`).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Goodbye(Option<f32>);

impl Goodbye {
    /// Something other than a goodbye was heard: the goodbye, if any, is
    /// taken back.
    pub fn heard(&mut self) {
        self.0 = None;
    }

    /// A goodbye was heard. The clock starts at the first copy and is not
    /// restarted by the repeats ([`crate::transport::UdpTransport`] says
    /// it more than once).
    pub fn spoke(&mut self) {
        self.0.get_or_insert(0.0);
    }

    /// `delta` seconds more for a goodbye to have stood.
    pub fn age(&mut self, delta: f32) {
        if let Some(since) = &mut self.0 {
            *since += delta;
        }
    }

    /// Whether a goodbye was said and not taken back since.
    pub fn said(&self) -> bool {
        self.0.is_some()
    }

    /// Whether a goodbye has stood for `grace` with no other word since:
    /// the moment it is believed.
    pub fn stood(&self, grace: f32) -> bool {
        self.0.is_some_and(|since| since >= grace)
    }
}

/// Follow a peer index across `gone` being forgotten: everyone behind it
/// moves down one, as the transport's own list closes the gap. `false`
/// when `index` was the forgotten peer itself, which has nothing left to
/// name, so a `retain_mut` drops it.
pub fn renumber(index: &mut usize, gone: usize) -> bool {
    match (*index).cmp(&gone) {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Equal => false,
        std::cmp::Ordering::Greater => {
            *index -= 1;
            true
        }
    }
}

/// The peers, indexed as the transport indexes them.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct PeerBook(Vec<Peer>);

impl PeerBook {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, peer: usize) -> Option<&Peer> {
        self.0.get(peer)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Peer> {
        self.0.iter()
    }

    /// The peers who would sit at the table, each with its index in the
    /// book: those that came to play, in order, as far as the chairs past
    /// the host's go, which is who [`Self::seat_plan`] seats. The lobby's
    /// card lists these, its count is these, and the host's K numbers
    /// these first, so a would-be player past the last chair is on none
    /// of the card's rows.
    pub fn at_the_table(&self) -> impl Iterator<Item = (usize, &Peer)> {
        self.by_place()
            .filter(|&(_, _, sits)| sits)
            .map(|(at, peer, _)| (at, peer))
    }

    /// Everyone else on the socket, each with its index in the book: the
    /// onlookers, and the would-be players past the last chair, in the
    /// order they came. On no row of the card, but still somebody the
    /// host may have to ask to leave, which its K numbers on past the
    /// table for.
    pub fn off_the_table(&self) -> impl Iterator<Item = (usize, &Peer)> {
        self.by_place()
            .filter(|&(_, _, sits)| !sits)
            .map(|(at, peer, _)| (at, peer))
    }

    /// Every peer, with whether it would sit at the table: the one rule
    /// both [`Self::at_the_table`] and [`Self::off_the_table`] read, so
    /// the two always split the book between them.
    fn by_place(&self) -> impl Iterator<Item = (usize, &Peer, bool)> {
        let mut chairs = crate::sim::MAX_PLAYERS - 1;
        self.0.iter().enumerate().map(move |(at, peer)| {
            let sits = !peer.watches() && chairs > 0;
            chairs -= usize::from(sits);
            (at, peer, sits)
        })
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Peer> {
        self.0.iter_mut()
    }

    /// Grow to cover `count` peers; new rows are blank and queued.
    pub fn reach(&mut self, count: usize) {
        if self.0.len() < count {
            self.0.resize_with(count, Peer::default);
        }
    }

    /// The row for `peer`, grown to reach it: a peer the socket has only
    /// just registered has no row until something is written about it.
    pub fn row(&mut self, peer: usize) -> &mut Peer {
        self.reach(peer + 1);
        &mut self.0[peer]
    }

    /// Exactly `count` rows: grown to reach a peer nothing has been
    /// written about yet, cut back past one the socket no longer has.
    pub fn fit(&mut self, count: usize) {
        self.reach(count);
        self.0.truncate(count);
    }

    /// Drop a peer's row, closing the gap so every index behind it moves
    /// with the transport's own list.
    pub fn forget(&mut self, peer: usize) {
        if peer < self.0.len() {
            self.0.remove(peer);
        }
    }

    /// A peer said something, which is all this needs to know: a goodbye
    /// before it included, since a peer that has gone says nothing more.
    pub fn heard(&mut self, peer: usize) {
        let row = self.row(peer);
        row.silence = 0.0;
        row.goodbye.heard();
    }

    /// The peer answered a `Ping` stamped `stamp`: one more round trip for
    /// its row, unless the stamp is none this machine wrote lately.
    pub fn answered(&mut self, peer: usize, stamp: u32) {
        if let Some(seconds) = crate::transport::round_trip_since(stamp) {
            self.row(peer).round_trips.note(seconds);
        }
    }

    /// The peer said it is going (`NetMsg::Leaving`): its silence is
    /// already as long as any patience, so every rule that forgets a quiet
    /// peer (the lobby's, the results card's, a watcher's) forgets this one
    /// at its next look rather than seconds later, and a seat it holds is
    /// given up on the moment the round waits on it.
    ///
    /// A goodbye is not also a word that is heard: it would take itself
    /// back ([`Goodbye`]).
    pub fn said_goodbye(&mut self, peer: usize) {
        let row = self.row(peer);
        row.silence = f32::INFINITY;
        row.goodbye.spoke();
    }

    /// Age every silence by a frame, every goodbye, and every round trip.
    pub fn age(&mut self, delta: f32) {
        for peer in &mut self.0 {
            peer.silence += delta;
            peer.goodbye.age(delta);
            peer.round_trips.age(delta);
        }
    }

    /// Nobody is late for a round that has not begun.
    pub fn hush(&mut self) {
        for peer in &mut self.0 {
            peer.silence = 0.0;
            peer.goodbye.heard();
        }
    }

    /// Deal a launch plan, one place per peer: a chair, or a watching place for a
    /// `None`. The plan covers everyone on the socket, so a row past it is
    /// a peer that has gone, and is dropped with the old plan.
    pub fn deal(&mut self, plan: &[Option<u8>]) {
        self.fit(plan.len());
        for (peer, slot) in self.0.iter_mut().zip(plan) {
            peer.newest = None;
            peer.wants = None;
            peer.place = match slot {
                Some(seat) => Place::Seated(*seat),
                None => Place::Watching,
            };
        }
    }

    /// Which of the first `peers` peers get a chair, in peer order, and
    /// which are watching (`None`). The host keeps seat 0, so the chairs
    /// are dealt from 1.
    ///
    /// Seats run out before connections do: the socket takes
    /// [`MAX_PEERS`](crate::transport::MAX_PEERS) and the table has
    /// [`MAX_PLAYERS`](crate::sim::MAX_PLAYERS) chairs, one of them the
    /// host's. The surplus is seated as onlookers rather than handed a seat
    /// number the sim has no slot for, so a beach holds the host and
    /// `MAX_PEERS` others in any mix: five rivals and the rest watching, or
    /// one AI and all of them watching.
    ///
    /// A watcher keeps no chair: one that asked, at the launch or since.
    /// One that sat the last round out for want of a chair is dealt one
    /// when there is one, and so is a peer the socket has that no row has
    /// been written for yet. The same deal at the launch and before every
    /// round after it, so those still here keep their seats and the queue
    /// fills what is left, pushing the AI back a seat at a time.
    pub fn seat_plan(&self, peers: usize) -> Vec<Option<u8>> {
        let mut next = 1u8; // seat 0 is the host's
        (0..peers)
            .map(|peer| {
                let watches = self.get(peer).is_some_and(Peer::watches);
                if watches || usize::from(next) >= crate::sim::MAX_PLAYERS {
                    return None;
                }
                let seat = next;
                next += 1;
                Some(seat)
            })
            .collect()
    }

    /// Freeze the table `plan` seats: each peer it gives a chair marked
    /// frozen on its row, and everyone else not, rows grown to cover the
    /// plan. What a host picking a map freezes, in the lobby and between
    /// rounds, for [`Self::frozen_plan`] to deal from.
    pub fn freeze(&mut self, plan: &[Option<u8>]) {
        self.reach(plan.len());
        for (row, seat) in self.0.iter_mut().zip(plan) {
            row.frozen = seat.is_some();
        }
    }

    /// The seat plan for the first `peers` peers while the table is frozen
    /// ([`Self::freeze`]): a chair, in peer order, for each peer frozen at
    /// it that still wants to play, and none for anybody else, who turned
    /// up while the host picked the map and watches the round it was
    /// picked for, as a player past the last chair does. It is dealt one
    /// the round after, which [`Self::seat_plan`] deals.
    pub fn frozen_plan(&self, peers: usize) -> Vec<Option<u8>> {
        let mut next = 1u8; // seat 0 is the host's
        (0..peers)
            .map(|peer| {
                let sits = self
                    .get(peer)
                    .is_some_and(|row| row.frozen && !row.watches());
                if !sits {
                    return None;
                }
                let seat = next;
                next += 1;
                Some(seat)
            })
            .collect()
    }

    /// The name a chat line from `from` goes out under: the one the peer
    /// greeted with, whatever the line itself claims, and the claim only
    /// for a peer that never said what to call it.
    ///
    /// An empty name is the beach's own voice (it is what announces a
    /// called event), so a peer that has a name may not drop it and speak
    /// as the room, nor wear somebody else's. Only the host vets: a spoke
    /// takes the hub's line as it stands. Refusing a line that still comes
    /// out nameless is the caller's, on the host.
    pub fn vouch(
        &self,
        from: usize,
        claimed: crate::transport::WireName,
    ) -> crate::transport::WireName {
        match self.get(from).map_or("", |peer| peer.name.as_str()) {
            "" => claimed,
            known => crate::transport::wire_name(known),
        }
    }

    /// The peers `gone` says to forget, highest index first: the order they
    /// can be forgotten in one after another, since forgetting a peer moves
    /// only the ones behind it, which have been dealt with already.
    pub fn to_forget(&self, gone: impl Fn(&Peer) -> bool) -> Vec<usize> {
        (0..self.0.len())
            .rev()
            .filter(|&peer| gone(&self.0[peer]))
            .collect()
    }

    /// How many peers the launch plan covers. Peers are registered in
    /// arrival order and the plan is dealt to everyone at the table when
    /// the round begins, so the planned peers are the leading run and
    /// everyone after them is in line.
    pub fn planned(&self) -> usize {
        self.0
            .iter()
            .take_while(|peer| matches!(peer.place, Place::Seated(_) | Place::Watching))
            .count()
    }

    /// Whether `peer` is following the round by the plan, and so needs its
    /// inputs: at the table, watching in step from frame zero, or caught up
    /// with it since. A peer in line for the next round is not, and sent
    /// the whole lockstep anyway costs about 180 datagrams a second for
    /// frames it cannot use: four people waiting turned a table's 930 a
    /// second into 1657.
    ///
    /// By the plan alone. `Queued` is the default place, so a joiner (which
    /// knows one peer, the host) and the direct `PINCH_HOST` pair (which
    /// deals no plan for its first round) have their one peer in line by
    /// this book, and send to it all the same: that is the session's call
    /// (`OnlineSession::follows_the_round`), which knows which of them it
    /// is. An empty plan is not enough to say so: a table formed in the
    /// lobby and launched against the AI has one too, and everybody on its
    /// socket is in line.
    pub fn follows_by_plan(&self, peer: usize) -> bool {
        peer < self.planned()
            || self
                .get(peer)
                .is_some_and(|row| row.place == Place::LateWatching)
    }

    /// The peer holding `seat`, if any does.
    pub fn holder_of(&self, seat: u8) -> Option<usize> {
        self.0.iter().position(|peer| peer.seat() == Some(seat))
    }

    /// The chair `peer` holds, or `None` for a watcher and for a peer the
    /// plan has never heard of.
    pub fn seat_of(&self, peer: usize) -> Option<u8> {
        self.0.get(peer).and_then(Peer::seat)
    }

    /// Back in the lobby nobody holds a chair: the next launch deals from
    /// the wish alone.
    pub fn unseat(&mut self) {
        for peer in &mut self.0 {
            peer.place = Place::Queued;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Forgetting peers 1 and 3, highest first as every caller does,
    /// leaves the indices behind each where the transport's list puts
    /// them, and drops the forgotten ones.
    #[test]
    fn indices_follow_their_peers_down() {
        let mut said = vec![0, 1, 2, 3, 4];
        for gone in [3, 1] {
            said.retain_mut(|at| renumber(at, gone));
        }
        assert_eq!(said, [0, 1, 2], "peers 0, 2 and 4, renumbered");
    }

    /// Forgetting a peer moves every row behind it down one, and the row
    /// keeps its name, its wish and its silence together: the failure this
    /// type exists to rule out is somebody wearing another player's name.
    #[test]
    fn forgetting_a_peer_moves_the_rows_behind_it_together() {
        let mut peers = PeerBook::default();
        peers.row(0).name = "Bo".into();
        peers.row(1).name = "Cy".into();
        peers.row(2).name = "Dee".into();
        peers.row(2).watch = true;
        peers.row(2).silence = 3.0;
        peers.deal(&[Some(1), Some(2), None]);
        peers.forget(0);
        assert_eq!(peers.len(), 2);
        assert_eq!(peers.get(0).map(|p| p.name.as_str()), Some("Cy"));
        let dee = peers.get(1).expect("Dee moved down");
        assert_eq!(dee.name, "Dee");
        assert!(dee.watches(), "the wish followed Dee down");
        assert_eq!(dee.silence, 3.0, "and so did the silence");
        assert_eq!(dee.place, Place::Watching);
        assert_eq!(peers.seat_of(0), Some(2), "Cy keeps the chair it held");
    }

    /// The plan is the leading run of placed peers: everyone the socket
    /// picked up after the launch is in line behind it, and a row grown
    /// for a stray greeting is queued rather than seated.
    #[test]
    fn the_plan_is_the_leading_run_and_the_rest_are_in_line() {
        let mut peers = PeerBook::default();
        peers.deal(&[Some(1), None]);
        assert_eq!(peers.planned(), 2);
        peers.heard(3);
        assert_eq!(peers.len(), 4, "grown to reach the stranger");
        assert_eq!(peers.planned(), 2, "and the stranger is in line");
        assert_eq!(peers.holder_of(1), Some(0));
        assert_eq!(peers.holder_of(0), None, "the host's chair is nobody's");
        // Dealt again for a smaller table, the rows past the plan go with
        // the peers that left rather than keeping stale chairs.
        peers.deal(&[Some(1)]);
        assert_eq!(peers.planned(), 1);
        assert_eq!(peers.len(), 1);
    }

    /// Walking back to the lobby keeps who watches and forgets who sat
    /// where: the next launch deals the chairs again from the wishes.
    #[test]
    fn unseating_keeps_the_watchers_and_frees_the_chairs() {
        let mut peers = PeerBook::default();
        peers.deal(&[Some(1), None, Some(2), None]);
        // Peer one asked to watch at the launch, peer two since; peer
        // three was dealt no chair because there was none, and asked for
        // nothing.
        peers.row(1).watch = true;
        peers.row(2).watch = true;
        peers.unseat();
        let watching: Vec<bool> = peers.iter().map(Peer::watches).collect();
        assert_eq!(watching, [false, true, true, false]);
        assert!(peers.iter().all(|p| p.seat().is_none()));
        assert_eq!(peers.planned(), 0);
    }

    /// Who the round's inputs are for. A peer in line is not simulating
    /// anything and used to be sent the lot anyway.
    #[test]
    fn only_the_table_and_the_watchers_follow_the_round() {
        let mut peers = PeerBook::default();
        peers.reach(4);
        peers.deal(&[Some(1), None]);
        peers.reach(4);
        assert_eq!(peers.planned(), 2, "a seat and a watching place");
        assert!(peers.follows_by_plan(0), "the seated peer");
        assert!(peers.follows_by_plan(1), "the watcher, in step from zero");
        assert!(!peers.follows_by_plan(2), "the one in line");
        assert!(!peers.follows_by_plan(3), "and the one behind it");
    }

    /// A watcher that arrived late follows the round without joining the
    /// plan: the peer in line behind it is still in line, and so is the
    /// one in front of it, whatever order they greeted in.
    #[test]
    fn a_late_watcher_follows_the_round_outside_the_plan() {
        let mut peers = PeerBook::default();
        peers.deal(&[Some(1), None]);
        peers.reach(4);
        // As the host takes one in: its greeting asked to watch.
        peers.row(2).watch = true;
        peers.row(2).place = Place::LateWatching;
        assert_eq!(peers.planned(), 2, "the plan is still the launch's");
        assert!(
            peers.follows_by_plan(2),
            "the late watcher is sent the round"
        );
        assert!(!peers.follows_by_plan(3), "the one in line is not");
        assert!(peers.get(2).is_some_and(Peer::watches));
        assert_eq!(peers.seat_of(2), None);
        // First in line, straight after the plan, it still does not extend it.
        let mut peers = PeerBook::default();
        peers.deal(&[Some(1)]);
        peers.row(1).place = Place::LateWatching;
        peers.row(2).place = Place::Queued;
        assert_eq!(peers.planned(), 1);
    }

    /// A book with no plan has nobody following the round by it: whether
    /// its peers hear anyway is the session's to say (a joiner's host, the
    /// direct pair's one peer), not the book's.
    #[test]
    fn a_book_with_no_plan_has_nobody_following_by_it() {
        let mut peers = PeerBook::default();
        peers.reach(2);
        assert_eq!(peers.planned(), 0, "nothing was ever dealt");
        assert!(!peers.follows_by_plan(0));
        assert!(!peers.follows_by_plan(1));
        peers.row(1).place = Place::LateWatching;
        assert!(peers.follows_by_plan(1), "but a late watcher does");
    }
}
