//! The crowd's side of a session: who is watching this round, and the
//! word the people without a chair may send the host (a tide vote, a
//! call for the winner, a line to the table) with the rule for whose word
//! is taken.

use super::*;

impl OnlineSession {
    /// Whether `peer` was dealt a place watching this round.
    ///
    /// The place itself, and nothing derived from it. Not `Peer::watches`,
    /// which is a wish as much as a place: true of somebody in line for
    /// the *next* round, who is sent no frames and sees nothing, and of a
    /// seated player that has asked to watch next time while still
    /// playing this one.
    ///
    /// And not "follows the round without a chair" either, which is what
    /// this asked first. That reads right until the book holds no plan,
    /// which `follows_the_round` treats as "everybody hears" so that a
    /// joiner and the direct `PINCH_HOST` pair keep talking to the one
    /// peer they have. In a direct pair the host then had its opponent
    /// down as a spectator: counted in the audience, and allowed to call
    /// tide events onto the game it was playing.
    pub fn watching_this_round(&self, peer: usize) -> bool {
        self.peers
            .get(peer)
            .is_some_and(|peer| matches!(peer.place, Place::Watching | Place::LateWatching))
    }

    /// How many are watching this round.
    pub fn watchers_in_round(&self) -> usize {
        (0..self.peers.len())
            .filter(|&peer| self.watching_this_round(peer))
            .count()
    }

    /// Take a pick for the next tide event, from someone entitled to make
    /// one.
    ///
    /// Counted only from a peer watching this round. The sender used to go
    /// unread, so a seated player could call tide events down on the table
    /// it was playing at, and a peer in line for the next round, sent no
    /// frames and with no board on screen, could vote on this one. With
    /// nobody watching at all, one stray datagram had the beach announcing
    /// what the spectators had called.
    ///
    /// Its own function, like [`Self::take_chat`], because the rule about
    /// who may be heard is the part worth being able to test.
    pub(super) fn take_vote(&mut self, host: bool, from: usize, event: u8) {
        if host && self.watching_this_round(from) {
            self.stands.votes.cast(event);
        }
    }

    /// Host: seconds left for the crowd to call the winner, zero once the
    /// calls are in. From the lockstep's frame, which is where the round is
    /// for everybody.
    pub fn picks_open_for(&self) -> u8 {
        use crate::app::spectators::PICKS_OPEN_FOR;
        let frame = self.session.frame();
        let left = PICKS_OPEN_FOR.saturating_sub(frame);
        left.div_ceil(crate::sim::TICKS_PER_SECOND)
            .min(u32::from(u8::MAX)) as u8
    }

    /// Host: the calls as they stand, counted from the peers watching this
    /// round alone.
    pub fn crowd_picks_now(&self) -> crate::app::spectators::Picks {
        let mut counts = [0u8; MAX_PLAYERS];
        for peer in 0..self.peers.len() {
            if !self.watching_this_round(peer) {
                continue;
            }
            if let Some(seat) = self.peers.get(peer).and_then(|row| row.pick)
                && seat < self.seats
            {
                counts[usize::from(seat)] = counts[usize::from(seat)].saturating_add(1);
            }
        }
        crate::app::spectators::Picks {
            counts,
            open: self.picks_open_for(),
        }
    }

    /// Take a call for the winner, from someone entitled to make one, while
    /// the calls are open. Watching this round, as with the vote, and a
    /// seat at this table.
    pub(super) fn take_pick(&mut self, host: bool, from: usize, seat: u8) {
        if host && self.watching_this_round(from) && seat < self.seats && self.picks_open_for() > 0
        {
            self.peers.row(from).pick = Some(seat);
        }
    }

    /// Take a line said to the table, and pass it on if this is the hub.
    ///
    /// Two jobs the lobby already does and the round did not. A spoke
    /// cannot hear another spoke, so a line reaches the table only by the
    /// host repeating it: without that, "say something to the table" said
    /// it to exactly one person. And the name is checked against the one
    /// the peer greeted with, because an empty name is the beach's own
    /// voice here (it is what announces a called event), so a peer that
    /// has a name may not drop it and speak as the room.
    pub(super) fn take_chat(
        &self,
        host: bool,
        from: usize,
        name: crate::transport::WireName,
        text: crate::transport::WireChat,
    ) -> Option<(String, String)> {
        // Host side only, as in the lobby. The hub vets what it repeats,
        // and a spoke takes the vetted line as it stands: a joiner that
        // re-applied this would stamp the host's name onto the beach's own
        // voice, which is the one line that is supposed to have none.
        let known = match host {
            true => self.peers.get(from).map_or("", |peer| peer.name.as_str()),
            false => "",
        };
        let name = match known.is_empty() {
            false => crate::transport::wire_name(known),
            true => name,
        };
        let (who, line) = (name_from_wire(&name), chat_from_wire(&text));
        if line.is_empty() {
            return None;
        }
        // The beach's own voice is the host's to use, so on the hub a line
        // that ends up nameless is refused outright rather than repeated:
        // a watcher greets with a bare `Watch` and may never have said
        // what to call it, and one that has no name is not the room. A
        // spoke has only the hub to hear from, so a nameless line reaching
        // it came from the beach and is passed through.
        if host && who.is_empty() {
            return None;
        }
        if host {
            relay(&self.transport, from, NetMsg::Chat { name, text });
        }
        Some((who, line))
    }
}
