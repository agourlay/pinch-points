//! Who steered the gulls: credit for the raids a player's arrows walked a
//! gull into.

use super::*;

/// Each gull's steerer, and what the steered gulls took.
///
/// A gull is steered by the owner of the last signpost it turned on, and a
/// raid it makes is credited to that seat (a gull no arrow ever turned is
/// wild, and its raids are nobody's). Read only by the feed and the
/// round's awards, never by the rules, so it stays out of the fingerprint
/// like [`events::Tide::spun_by`]. It is still the board's own state,
/// written by the tick every peer plays the same way, copied with the
/// board and carried by a snapshot, so a round picked back up or caught up
/// with mid-round credits the same raids on every screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Steering {
    /// The seat whose signpost each steered gull last turned on, by gull
    /// id, in the order they were first steered. A gull that leaves the
    /// beach leaves this list on the same tick.
    pub(super) by_gull: Vec<(u32, PlayerId)>,
    /// Points the gulls each seat steered have taken, as
    /// `taken[steerer][victim]`. A seat's own castle is on the diagonal:
    /// an arrow can walk a gull home as easily as next door.
    pub(super) taken: [[u32; MAX_PLAYERS]; MAX_PLAYERS],
}

impl Steering {
    /// `gull` turned on a signpost of `owner`'s.
    pub(super) fn steer(&mut self, gull: u32, owner: PlayerId) {
        match self.by_gull.iter_mut().find(|(id, _)| *id == gull) {
            Some((_, by)) => *by = owner,
            None => self.by_gull.push((gull, owner)),
        }
    }

    /// Who last steered `gull`, if anybody did.
    pub(super) fn steerer(&self, gull: u32) -> Option<PlayerId> {
        self.by_gull
            .iter()
            .find(|(id, _)| *id == gull)
            .map(|&(_, by)| by)
    }

    /// `gull` raided `victim` for `lost` points: put it on its steerer's
    /// account, if it had one.
    pub(super) fn credit(&mut self, gull: u32, victim: PlayerId, lost: u32) {
        if let Some(by) = self.steerer(gull)
            && let (Some(row), Some(_)) = (seat(by), seat(victim))
        {
            let cell = &mut self.taken[row][usize::from(victim)];
            *cell = cell.saturating_add(lost);
        }
    }

    /// Forget the gulls no longer on the beach.
    pub(super) fn keep_only(&mut self, gulls: &[Gull]) {
        self.by_gull
            .retain(|(id, _)| gulls.iter().any(|gull| gull.id == *id));
    }

    /// [`Board::copy_from`]'s share, the list's allocation kept.
    pub(super) fn copy_from(&mut self, other: &Self) {
        let Self { by_gull, taken } = other;
        self.by_gull.clear();
        self.by_gull.extend_from_slice(by_gull);
        self.taken = *taken;
    }
}

impl Board {
    /// Who last steered the gull with this id: the owner of the last
    /// signpost it turned on, or `None` for a wild gull.
    pub fn gull_steerer(&self, gull: u32) -> Option<PlayerId> {
        self.steering.steerer(gull)
    }

    /// Points the gulls `steerer` steered took from `victim`'s castle this
    /// round, `steerer == victim` included. Castle by castle, because
    /// which of them were stolen from a rival depends on the teams, which
    /// the sim does not know: the reader sums the ones it counts.
    pub fn stolen(&self, steerer: PlayerId, victim: PlayerId) -> u32 {
        match (seat(steerer), seat(victim)) {
            (Some(a), Some(v)) => self.steering.taken[a][v],
            _ => 0,
        }
    }
}

/// What a raid on a castle holding `score` points takes: a third, rounded
/// up for the gull (spec §3.4). A third rather than half: a wild gull nobody
/// steered decided too many rounds, and a third still leaves the raid the
/// game's comeback. The one rule [`Board::damage_castle`] plays and the
/// rules tip quotes.
pub fn raid_loss(score: u32) -> u32 {
    score.div_ceil(3)
}
