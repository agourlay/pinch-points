//! Picking partners under a team mode: who a seat's player teams up with,
//! chosen from the other seats, on the couch's step 3 and on an online
//! host's lobby card.
//!
//! The teams themselves never change: a seat plays for
//! [`TeamMode::team_of`] its number, the split that keeps every team fair
//! on the castle spots (see [`crate::app::teams`]). A pick moves people
//! instead, swapping whole seats (who plays it, its name, its pad) so the
//! partner picked sits in a seat of the picker's team. A seat that cannot
//! move stays out of every swap: P1, the person working the menu, and on
//! the couch the keyboard's second player, whose keys are wired to seat 2.

use crate::app::teams::TeamMode;
use crate::sim::MAX_PLAYERS;

/// Who sits in each seat, each person numbered by the seat they sat in
/// when the picking began, so the number moves with them through the
/// swaps ([`next_pick`], [`reseated`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Who(pub [u8; MAX_PLAYERS]);

impl Default for Who {
    /// Everybody in the seat they started in.
    fn default() -> Self {
        Who(std::array::from_fn(|seat| seat as u8))
    }
}

/// One way a seat's team can be made up: the teammates it then has (in
/// seat order, each named by the seat it is in now) and the swaps that
/// make it, each a pair of seats whose occupants trade places. The swaps
/// never share a seat, so their order does not matter.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Pick {
    pub mates: Vec<u8>,
    pub swaps: Vec<(u8, u8)>,
}

impl Pick {
    /// Where the occupant of `seat` sits once the swaps are made.
    pub fn moved(&self, seat: u8) -> u8 {
        self.swaps
            .iter()
            .find_map(|&(a, b)| match seat {
                s if s == a => Some(b),
                s if s == b => Some(a),
                _ => None,
            })
            .unwrap_or(seat)
    }
}

/// Every team `seat` can be given at a table of `seats` under `mode`,
/// teammates in seat order and the sets in order, the team it has now
/// among them with no swaps; and whether a seat in `pinned` held any other
/// back. `fixed` and `pinned` are the seats that never move: `fixed` for a
/// reason the screen need not give (P1), `pinned` for one it says.
pub fn picks(
    mode: TeamMode,
    seats: u8,
    seat: u8,
    fixed: &[u8],
    pinned: &[u8],
) -> (Vec<Pick>, bool) {
    if mode == TeamMode::Solo || !mode.fits(seats) || seat >= seats {
        return (Vec::new(), false);
    }
    let size = usize::from(seats / mode.teams(seats));
    let others: Vec<u8> = (0..seats).filter(|&other| other != seat).collect();
    let stays = |other: u8| fixed.contains(&other) || pinned.contains(&other);
    let stays_unpinned = |other: u8| fixed.contains(&other);
    let mut out = Vec::new();
    let mut held_back = false;
    for mates in choose(&others, size - 1) {
        match placed(mode, seats, seat, &mates, stays) {
            Some(swaps) => out.push(Pick { mates, swaps }),
            None => held_back |= placed(mode, seats, seat, &mates, stays_unpinned).is_some(),
        }
    }
    (out, held_back)
}

/// The team `seat` plays in now: its teammates, in seat order.
pub fn mates_of(mode: TeamMode, seats: u8, seat: u8) -> Vec<u8> {
    mode.seats_of(mode.team_of(seat), seats)
        .into_iter()
        .filter(|&other| other != seat)
        .collect()
}

/// The next team `seat` can be given after the one it has ([`picks`]),
/// round the list, or `None` when it can be given no other. `who` says who
/// sits in each seat, as a number that moves with them (the seat each sat
/// in when the picking began): the list goes round the people, so a pick
/// made by swapping seats does not bring the same seat's new occupant
/// round again, and every partner comes up in turn.
pub fn next_pick(
    mode: TeamMode,
    seats: u8,
    seat: u8,
    (fixed, pinned): (&[u8], &[u8]),
    who: &[u8],
) -> Option<Pick> {
    let (mut all, _) = picks(mode, seats, seat, fixed, pinned);
    let people = |mates: &[u8]| {
        let mut ids: Vec<u8> = mates
            .iter()
            .map(|&mate| who.get(usize::from(mate)).copied().unwrap_or(mate))
            .collect();
        ids.sort_unstable();
        ids
    };
    all.sort_by_key(|pick| people(&pick.mates));
    let now = mates_of(mode, seats, seat);
    let at = all.iter().position(|pick| pick.mates == now)?;
    let next = all.get((at + 1) % all.len())?;
    (next.mates != now).then(|| next.clone())
}

/// Who sits in each seat once `pick`'s swaps are made, `who` being who
/// sat in each before ([`next_pick`]).
pub fn reseated(who: &mut [u8], pick: &Pick) {
    for &(a, b) in &pick.swaps {
        who.swap(usize::from(a), usize::from(b));
    }
}

/// Swaps that put `seat` and `mates` in one team: `seat`'s own team when
/// the seats moving can move, so the picker stays where they are, else
/// the first other team that takes them. `None` when every way moves a
/// seat that `stays`.
fn placed(
    mode: TeamMode,
    seats: u8,
    seat: u8,
    mates: &[u8],
    stays: impl Fn(u8) -> bool,
) -> Option<Vec<(u8, u8)>> {
    let own = mode.team_of(seat);
    let teams = std::iter::once(own).chain((0..mode.teams(seats)).filter(|&team| team != own));
    let wanted: Vec<u8> = std::iter::once(seat).chain(mates.iter().copied()).collect();
    for team in teams {
        let members = mode.seats_of(team, seats);
        let mut coming: Vec<u8> = wanted
            .iter()
            .copied()
            .filter(|s| !members.contains(s))
            .collect();
        let going: Vec<u8> = members
            .iter()
            .copied()
            .filter(|s| !wanted.contains(s))
            .collect();
        coming.sort_unstable();
        if coming.iter().chain(&going).any(|&s| stays(s)) {
            continue;
        }
        return Some(coming.into_iter().zip(going).collect());
    }
    None
}

/// Every way to choose `n` of `from`, each in `from`'s order, the sets in
/// order.
fn choose(from: &[u8], n: usize) -> Vec<Vec<u8>> {
    if n == 0 {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    for (at, &first) in from.iter().enumerate() {
        for mut rest in choose(&from[at + 1..], n - 1) {
            rest.insert(0, first);
            out.push(rest);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Make `pick`'s swaps on a table of who sits where, by seat.
    fn swapped(mut table: Vec<char>, pick: &Pick) -> Vec<char> {
        for &(a, b) in &pick.swaps {
            table.swap(usize::from(a), usize::from(b));
        }
        table
    }

    /// Pairs at four: P1 picks each of the others in turn, and the one
    /// picked comes to seat 2, P1's team, while P1 never moves.
    #[test]
    fn p1_picks_each_partner_in_turn() {
        let mode = TeamMode::Pairs;
        let mut table = vec!['a', 'b', 'c', 'd'];
        let mut who = vec![0, 1, 2, 3];
        let mut partners = Vec::new();
        for _ in 0..6 {
            let pick = next_pick(mode, 4, 0, (&[0], &[]), &who).expect("a pick");
            assert!(
                pick.swaps.iter().all(|&(a, b)| a != 0 && b != 0),
                "P1 stays"
            );
            table = swapped(table, &pick);
            reseated(&mut who, &pick);
            partners.push(table[1]);
        }
        assert_eq!(partners, ['c', 'd', 'b', 'c', 'd', 'b'], "round the others");
    }

    /// A seat picking P1, who never moves, goes to P1's team itself.
    #[test]
    fn picking_p1_moves_the_picker() {
        let mode = TeamMode::Pairs;
        let (all, _) = picks(mode, 4, 3, &[0], &[]);
        let with_p1 = all
            .iter()
            .find(|pick| pick.mates == [0])
            .expect("P1 can be picked");
        assert_eq!(with_p1.swaps, vec![(3, 1)]);
        assert_eq!(with_p1.moved(3), 1, "the picker sits beside P1");
    }

    /// Every pick keeps the teams whole and fair: each is a swap of
    /// seats, so every seat is still sat in once, and the teams are still
    /// the seats `team_of` says.
    #[test]
    fn every_pick_is_a_shuffle_of_whole_seats() {
        for (mode, seats) in [
            (TeamMode::Pairs, 4),
            (TeamMode::Pairs, 6),
            (TeamMode::Trios, 6),
        ] {
            for seat in 0..seats {
                let (all, held) = picks(mode, seats, seat, &[0], &[]);
                assert!(!held);
                // Anybody can be the picker's teammate, P1 included.
                for other in 0..seats {
                    if other != seat {
                        assert!(all.iter().any(|pick| pick.mates.contains(&other)));
                    }
                }
                for pick in &all {
                    let table: Vec<char> = (0..seats).map(|s| char::from(b'a' + s)).collect();
                    let after = swapped(table.clone(), pick);
                    let mut sorted = after.clone();
                    sorted.sort_unstable();
                    assert_eq!(sorted, table, "{mode:?} {seat} {pick:?}");
                    assert_eq!(after[0], 'a', "P1 never moves");
                    // The picker and its mates share a team afterwards.
                    let at =
                        |who: char| after.iter().position(|&c| c == who).expect("seated") as u8;
                    let picker = at(table[usize::from(seat)]);
                    for &mate in &pick.mates {
                        let mate = at(table[usize::from(mate)]);
                        assert_eq!(mode.team_of(mate), mode.team_of(picker), "{pick:?}");
                    }
                }
            }
        }
    }

    /// Trios: P1 picks two teammates, every two of the other five that can
    /// be had, ten in all, and the current two among them.
    #[test]
    fn trios_offer_every_two_teammates() {
        let (all, _) = picks(TeamMode::Trios, 6, 0, &[0], &[]);
        assert_eq!(all.len(), 10);
        assert!(
            all.iter()
                .any(|pick| pick.mates == [2, 4] && pick.swaps.is_empty())
        );
    }

    /// The keyboard's P2 pinned to seat 2: at four in pairs P1 and P2 are
    /// a team for good, nobody can pick either of them, and that is said
    /// as held back.
    #[test]
    fn a_pinned_seat_never_moves_and_says_so() {
        let mode = TeamMode::Pairs;
        let who = [0, 1, 2, 3, 4, 5];
        assert_eq!(next_pick(mode, 4, 0, (&[0], &[1]), &who), None);
        let (all, held) = picks(mode, 4, 2, &[0], &[1]);
        assert!(held, "P1 and P2 held back");
        assert_eq!(all.len(), 1, "P4, already the partner");
        // At six in pairs the others still pair up among themselves.
        let pick = next_pick(mode, 6, 2, (&[0], &[1]), &who).expect("a pick");
        assert!(pick.swaps.iter().all(|&(a, b)| a > 1 && b > 1), "{pick:?}");
        // Trios at six: P2 stays in the second trio, the rest move round.
        let (all, held) = picks(TeamMode::Trios, 6, 0, &[0], &[1]);
        assert!(held);
        assert!(all.iter().all(|pick| !pick.mates.contains(&1)));
        assert_eq!(all.len(), 6, "two of the four free seats");
    }

    /// No teams, no picks: free-for-all, and a mode the table does not fit.
    #[test]
    fn no_picks_without_teams() {
        assert_eq!(picks(TeamMode::Solo, 4, 0, &[0], &[]), (Vec::new(), false));
        assert_eq!(picks(TeamMode::Pairs, 5, 0, &[0], &[]), (Vec::new(), false));
        assert_eq!(picks(TeamMode::Trios, 4, 0, &[0], &[]), (Vec::new(), false));
    }
}
