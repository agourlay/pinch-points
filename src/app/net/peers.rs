//! The peers on a socket, as the shell knows them: one row per peer
//! index, kept in step with the transport's own list.
//!
//! Everything the shell keeps about a peer used to live in a parallel
//! `Vec` of its own (seat, name, watch wish, silence), each grown to reach
//! a new index by its own loop and each shifted by hand when a peer was
//! forgotten. Four lists that have to move together or not at all is a
//! bug waiting for the one site that forgets; one row per peer cannot
//! come apart.

/// Where a peer stands in the round, as the launch plan dealt it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Place {
    /// At the table since the launch, holding this chair.
    Seated(u8),
    /// At the rail since the launch: in step from frame zero like every
    /// player, but holding no chair.
    Watching,
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
}

impl Peer {
    /// The chair it holds, if it holds one.
    pub fn seat(&self) -> Option<u8> {
        match self.place {
            Place::Seated(seat) => Some(seat),
            Place::Watching | Place::Queued => None,
        }
    }

    /// Whether it watches rather than plays: dealt the rail at the
    /// launch, or asked for it since.
    pub fn watches(&self) -> bool {
        self.place == Place::Watching || self.watch
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

    /// A peer said something, which is all this needs to know.
    pub fn heard(&mut self, peer: usize) {
        self.row(peer).silence = 0.0;
    }

    /// Age every silence by a frame.
    pub fn age(&mut self, delta: f32) {
        for peer in &mut self.0 {
            peer.silence += delta;
        }
    }

    /// Nobody is late for a round that has not begun.
    pub fn hush(&mut self) {
        for peer in &mut self.0 {
            peer.silence = 0.0;
        }
    }

    /// Deal a launch plan, one place per peer: a chair, or the rail for a
    /// `None`. The plan covers everyone on the socket, so a row past it is
    /// a peer that has gone, and is dropped with the old plan.
    pub fn deal(&mut self, plan: &[Option<u8>]) {
        self.fit(plan.len());
        for (peer, slot) in self.0.iter_mut().zip(plan) {
            peer.place = match slot {
                Some(seat) => Place::Seated(*seat),
                None => Place::Watching,
            };
        }
    }

    /// How many peers the launch plan covers. Peers are registered in
    /// arrival order and the plan is dealt to everyone at the table when
    /// the round begins, so the planned peers are the leading run and
    /// everyone after them is in line.
    pub fn planned(&self) -> usize {
        self.0
            .iter()
            .take_while(|peer| peer.place != Place::Queued)
            .count()
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

    /// Back in the lobby nobody holds a chair, and a peer that was at the
    /// rail is one that asked to watch: the next launch deals from the
    /// wish alone.
    pub fn unseat(&mut self) {
        for peer in &mut self.0 {
            peer.watch = peer.watches();
            peer.place = Place::Queued;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        peers.deal(&[Some(1), None, Some(2)]);
        peers.row(2).watch = true;
        peers.unseat();
        let watching: Vec<bool> = peers.iter().map(Peer::watches).collect();
        assert_eq!(watching, [false, true, true]);
        assert!(peers.iter().all(|p| p.seat().is_none()));
        assert_eq!(peers.planned(), 0);
    }
}
