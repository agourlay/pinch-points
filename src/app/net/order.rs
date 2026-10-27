//! Who sits in which seat at an online table whose host picked partners.
//!
//! The host deals a table in places: itself, then the players and its own
//! bots in the order it deals them, then the AI. Unarranged, each place is
//! its own seat, which is how every table was dealt before partners could
//! be picked. Once the host picks partners in the lobby
//! ([`crate::app::partners`]) it remembers the seat it put everybody in:
//! each peer on its row ([`super::Peer::seat_wish`]), its bots by who they
//! are and the AI by seat ([`Arranged`]). Every deal after that, at the
//! launch and before every round, puts each place back in the seat it was
//! arranged in while that seat is still at the table, and seats anybody
//! new in what is left, so the teams picked hold for the whole match.
//! The order travels in the terms ([`MatchTerms::order`]), which is how
//! every peer knows where the AI sits.

use crate::app::teams::TeamMode;
use crate::bots::listener::BotId;
use crate::sim::MAX_PLAYERS;
use crate::transport::{IN_SEAT_ORDER, MatchTerms};

/// The seats a host's partner picks put its own bots and the AI in. The
/// peers' seats are on their rows, which move with them.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Arranged {
    /// Each of the host's own bots (route 2), by who it is, and its seat.
    pub bots: Vec<(BotId, u8)>,
    /// The seats the AI was put in, in place order.
    pub ai: Vec<u8>,
}

/// Whether partner picks seat a table scored as `teams` ([`TeamMode::played_by`]
/// its seats): only a team mode has partners. Under free-for-all the picks
/// are set aside, not forgotten, so a host that turns scoring back to
/// teams finds them as it left them.
pub fn picks_hold(teams: TeamMode) -> bool {
    teams != TeamMode::Solo
}

/// The seat order for a table of `seats` whose first `humans` places are
/// the host and its players and bots, and the rest the AI: each player's
/// place in the seat it `wants` (by place; the host's own is seat 0
/// whatever it says) while that seat is at the table and nobody before it
/// took it, the AI in the seats in `ai` likewise, then the players nobody
/// placed in the lowest seats left and the AI in the rest. With no wants
/// at all, every place is its own seat.
pub fn seat_order(wants: &[Option<u8>], ai: &[u8], humans: u8, seats: u8) -> [u8; MAX_PLAYERS] {
    let seats = seats.min(MAX_PLAYERS as u8);
    let humans = humans.min(seats);
    let mut deal = Deal {
        order: IN_SEAT_ORDER,
        taken: [false; MAX_PLAYERS],
        placed: [false; MAX_PLAYERS],
    };
    deal.put(0, 0);
    for place in 1..humans {
        if let Some(Some(seat)) = wants.get(usize::from(place))
            && *seat < seats
            && deal.free(*seat)
        {
            deal.put(place, *seat);
        }
    }
    let mut ai_places = humans..seats;
    for &seat in ai {
        if seat >= seats || !deal.free(seat) {
            continue;
        }
        let Some(place) = ai_places.next() else {
            break;
        };
        deal.put(place, seat);
    }
    for place in 0..seats {
        if deal.placed[usize::from(place)] {
            continue;
        }
        let Some(seat) = (0..seats).find(|&seat| deal.free(seat)) else {
            break;
        };
        deal.put(place, seat);
    }
    deal.order
}

/// A seat order being dealt: the order so far, the seats taken and the
/// places seated.
struct Deal {
    order: [u8; MAX_PLAYERS],
    taken: [bool; MAX_PLAYERS],
    placed: [bool; MAX_PLAYERS],
}

impl Deal {
    fn put(&mut self, place: u8, seat: u8) {
        self.order[usize::from(place)] = seat;
        self.taken[usize::from(seat)] = true;
        self.placed[usize::from(place)] = true;
    }

    fn free(&self, seat: u8) -> bool {
        !self.taken[usize::from(seat)]
    }
}

/// What a dealt table remembers of its order for the next deal: the seat
/// of each of the host's bots, and the AI's seats.
pub fn remember(terms: MatchTerms, seats: u8, bots: &[super::HostedBot]) -> Arranged {
    Arranged {
        bots: bots.iter().map(|bot| (bot.id, bot.seat)).collect(),
        ai: terms.ai_seats(seats),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nobody arranged: every place its own seat, the AI on top.
    #[test]
    fn unarranged_is_in_seat_order() {
        assert_eq!(seat_order(&[], &[], 4, 6), IN_SEAT_ORDER);
        assert_eq!(seat_order(&[None, None], &[], 2, 4), IN_SEAT_ORDER);
    }

    /// The host beside the AI and the friend beside the other AI: each
    /// back in the seat it was put in.
    #[test]
    fn an_arrangement_is_dealt_again() {
        let order = seat_order(&[None, Some(2)], &[1, 3], 2, 4);
        assert_eq!(&order[..4], [0, 2, 1, 3]);
    }

    /// A player gone: the others keep their seats and the AI fills the
    /// hole; a newcomer takes the lowest seat left, before the AI that was
    /// never there.
    #[test]
    fn the_table_changing_keeps_who_stayed() {
        // Three players were in seats 3, 1 and 2; the one in 1 left.
        let order = seat_order(&[None, Some(3), Some(2)], &[1], 3, 4);
        assert_eq!(&order[..4], [0, 3, 2, 1]);
        // A newcomer at a table whose AI was in seat 1: it takes 2, which
        // nobody wanted, and the AI keeps 1.
        let order = seat_order(&[None, Some(3), None], &[1], 3, 4);
        assert_eq!(&order[..4], [0, 3, 2, 1]);
        // A seat past a smaller table is no seat: dealt afresh.
        let order = seat_order(&[None, Some(5)], &[], 2, 2);
        assert_eq!(&order[..2], [0, 1]);
    }

    /// Two places wanting one seat: the first has it, the second the
    /// lowest left; the host's seat is never anybody else's.
    #[test]
    fn a_seat_is_held_once() {
        let order = seat_order(&[Some(1), Some(0), Some(2), Some(2)], &[], 4, 4);
        assert_eq!(&order[..4], [0, 1, 2, 3]);
        let mut sorted = order;
        sorted.sort_unstable();
        assert_eq!(sorted, IN_SEAT_ORDER);
    }
}
