//! How a round is scored: everyone for themselves, or in teams.
//!
//! Teams are presentation and win condition only. The sim has never known
//! about them, which keeps them lockstep-safe. What lives here is the
//! seat-to-team map, and it is chosen for fairness rather than convenience:
//! castle spots come in 180-degree pairs (see
//! [`castle_spots`](crate::sim::castle_spots)), so a team split is fair when
//! every team is either built from whole pairs or is the mirror image of
//! another team. That is why pairs block up (`{0,1} {2,3} {4,5}`, each an
//! opposite pair) while trios interleave (`{0,2,4}` against `{1,3,5}`, two
//! halves that map onto each other). Blocking the trios would put two top
//! corners on one side and two edges on the other.

use crate::app::cycle::Cycle;
use crate::app::net;
use crate::app::settings::GameSettings;
use crate::sim::MAX_PLAYERS;

/// Who counts as "us" when the tide comes in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TeamMode {
    /// Free-for-all: every seat for itself.
    #[default]
    Solo,
    /// Pairs: 2v2 at four seats, 2v2v2 at six.
    Pairs,
    /// Two trios, six seats only.
    Trios,
}

impl TeamMode {
    pub const ALL: [TeamMode; 3] = [TeamMode::Solo, TeamMode::Pairs, TeamMode::Trios];

    /// Whether this mode can be played with `seats` players. Every team has
    /// to be the same size, and there have to be at least two of them.
    pub fn fits(self, seats: u8) -> bool {
        match self {
            TeamMode::Solo => true,
            TeamMode::Pairs => seats >= 4 && seats.is_multiple_of(2),
            TeamMode::Trios => seats == 6,
        }
    }

    /// The team a seat plays for.
    pub fn team_of(self, seat: u8) -> u8 {
        match self {
            TeamMode::Solo => seat,
            // Whole opposite pairs: {0,1} {2,3} {4,5}.
            TeamMode::Pairs => seat / 2,
            // Two mirror-image halves: {0,2,4} and {1,3,5}.
            TeamMode::Trios => seat % 2,
        }
    }

    /// How many teams are on the beach.
    pub fn teams(self, seats: u8) -> u8 {
        match self {
            TeamMode::Solo => seats,
            TeamMode::Pairs => seats / 2,
            TeamMode::Trios => 2,
        }
    }

    /// The seats that play for `team`, in seat order.
    pub fn seats_of(self, team: u8, seats: u8) -> Vec<u8> {
        (0..seats).filter(|&s| self.team_of(s) == team).collect()
    }

    /// The wire and save-file form.
    pub fn index(self) -> usize {
        Cycle::index(self)
    }

    pub fn from_index(index: usize) -> TeamMode {
        <TeamMode as Cycle>::from_index(index)
    }

    /// Stable settings-file key.
    pub fn key(self) -> &'static str {
        match self {
            TeamMode::Solo => "ffa",
            TeamMode::Pairs => "pairs",
            TeamMode::Trios => "trios",
        }
    }

    pub fn from_key(key: &str) -> TeamMode {
        match key {
            // "2v2" is the older spelling, from when pairs were the only
            // teams there were.
            "pairs" | "2v2" => TeamMode::Pairs,
            "trios" => TeamMode::Trios,
            _ => TeamMode::Solo,
        }
    }
}

impl Cycle for TeamMode {
    const VARIANTS: &'static [Self] = &TeamMode::ALL;
}

/// How this round is scored.
///
/// Online the answer is the host's, carried in the agreed terms. Were it
/// each peer's own setting, two people could watch the same round and be
/// shown different winners. Offline it is this machine's setting. Either way
/// a mode that does not fit the seat count falls back to free-for-all rather
/// than inventing a lopsided team.
pub(crate) fn in_play(settings: &GameSettings, online: &net::Online, seats: u8) -> TeamMode {
    let wanted = match &online.0 {
        Some(session) => TeamMode::from_index(usize::from(session.terms.teams)),
        None => settings.team_mode,
    };
    if wanted.fits(seats) {
        wanted
    } else {
        TeamMode::Solo
    }
}

/// Each team's total, indexed by team.
pub fn team_scores(scores: &[u32; MAX_PLAYERS], seats: u8, mode: TeamMode) -> Vec<u32> {
    (0..mode.teams(seats))
        .map(|team| {
            mode.seats_of(team, seats)
                .iter()
                .map(|&seat| scores[usize::from(seat)])
                .sum()
        })
        .collect()
}

/// What to call a team: the names of the seats on it, joined. With nobody
/// named that reads "P1+P2", and with names it reads "Anna+Bo". Either way
/// it says who, which "Team A" never did.
pub fn label(
    settings: &GameSettings,
    names: &crate::app::SeatNames,
    mode: TeamMode,
    team: u8,
    seats: u8,
) -> String {
    mode.seats_of(team, seats)
        .iter()
        .map(|&seat| names.label(settings.tr(), seat))
        .collect::<Vec<_>>()
        .join("+")
}

/// The seat whose colour and order stand for a team: its lowest.
pub fn face_of(mode: TeamMode, team: u8, seats: u8) -> u8 {
    mode.seats_of(team, seats).first().copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::castle_spots;

    #[test]
    fn modes_only_offer_themselves_where_they_fit() {
        assert!(TeamMode::Solo.fits(2) && TeamMode::Solo.fits(5));
        assert!(!TeamMode::Pairs.fits(2), "one pair is not a match");
        assert!(!TeamMode::Pairs.fits(5), "odd seats cannot pair up");
        assert!(TeamMode::Pairs.fits(4) && TeamMode::Pairs.fits(6));
        assert!(!TeamMode::Trios.fits(4) && TeamMode::Trios.fits(6));
        assert_eq!(TeamMode::Pairs.teams(6), 3, "2v2v2");
        assert_eq!(TeamMode::Trios.teams(6), 2, "3v3");
    }

    /// The point of the seat map: on a board whose castle spots come in
    /// 180-degree pairs, every team must be built from whole pairs or be the
    /// mirror image of another team. Anything else hands one side more
    /// corners than the other.
    #[test]
    fn every_split_is_symmetric_on_the_board() {
        // Spot `2k` and spot `2k+1` are each other's 180-degree image; call
        // that the pair index.
        let pair_of = |seat: u8| seat / 2;

        // Pairs: each team is exactly one whole pair, so each team is its own
        // mirror image.
        for seats in [4u8, 6] {
            for team in 0..TeamMode::Pairs.teams(seats) {
                let members = TeamMode::Pairs.seats_of(team, seats);
                assert_eq!(members.len(), 2);
                assert_eq!(
                    pair_of(members[0]),
                    pair_of(members[1]),
                    "a pair team must hold both ends of one axis"
                );
            }
        }

        // Trios: the two teams are each other's mirror image, seat for seat.
        let a = TeamMode::Trios.seats_of(0, 6);
        let b = TeamMode::Trios.seats_of(1, 6);
        assert_eq!(a, vec![0, 2, 4]);
        assert_eq!(b, vec![1, 3, 5]);
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(pair_of(*x), pair_of(*y), "{x} and {y} are not opposites");
        }

        // And the pairing really is 180-degree symmetric on a real board.
        let (w, h) = (20u8, 13);
        let spots = castle_spots(w, h);
        for pair in 0..3usize {
            let (x0, y0) = spots[pair * 2];
            let (x1, y1) = spots[pair * 2 + 1];
            assert_eq!((w - 1 - x0, h - 1 - y0), (x1, y1), "pair {pair}");
        }
    }

    #[test]
    fn team_totals_add_up() {
        let scores = [1, 2, 3, 4, 5, 6];
        assert_eq!(team_scores(&scores, 6, TeamMode::Pairs), vec![3, 7, 11]);
        assert_eq!(team_scores(&scores, 6, TeamMode::Trios), vec![9, 12]);
        assert_eq!(team_scores(&scores, 4, TeamMode::Pairs), vec![3, 7]);
        assert_eq!(
            team_scores(&scores, 4, TeamMode::Solo),
            vec![1, 2, 3, 4],
            "solo teams are seats"
        );
    }

    #[test]
    fn keys_round_trip_and_the_old_name_still_loads() {
        for mode in TeamMode::ALL {
            assert_eq!(TeamMode::from_key(mode.key()), mode);
            assert_eq!(TeamMode::from_index(mode.index()), mode);
        }
        assert_eq!(TeamMode::from_key("2v2"), TeamMode::Pairs, "old setting");
        assert_eq!(TeamMode::from_key("nonsense"), TeamMode::Solo);
    }
}
