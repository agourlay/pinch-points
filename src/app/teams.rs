//! How a round is scored: everyone for themselves, or in teams.
//!
//! Teams are mostly presentation and win condition. The sim knows only
//! each seat's team ([`crate::sim::Rules::teams`], from [`TeamMode::sides`]),
//! for the rules that pick on rivals, and it holds that in the hashed
//! rules so every peer agrees. What lives here is the seat-to-team map, and it is chosen for fairness rather than convenience:
//! castle spots come in 180-degree pairs (see
//! [`castle_spots`](crate::sim::castle_spots)), so a team split is fair when
//! every team is either built from whole pairs or is the mirror image of
//! another team. That is why pairs block up (`{0,1} {2,3} {4,5}`, each an
//! opposite pair) while trios interleave (`{0,2,4}` against `{1,3,5}`, two
//! halves that map onto each other). Blocking the trios would put two top
//! corners on one side and two edges on the other.

use crate::app::cycle::Cycle;
use crate::app::match_setup::TermsDials;
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

    /// The mode a table of `seats` actually plays under this one: itself
    /// where it fits, else free-for-all.
    pub fn played_by(self, seats: u8) -> TeamMode {
        match self.fits(seats) {
            true => self,
            false => TeamMode::Solo,
        }
    }

    /// The fewest seats past `seats` this mode fits, if any: where a
    /// table playing free-for-all under it starts playing it.
    pub fn fits_from(self, seats: u8) -> Option<u8> {
        (seats.saturating_add(1)..=MAX_PLAYERS as u8).find(|&more| self.fits(more))
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

    /// Each seat's team as the board's rules hold it
    /// ([`crate::sim::Rules::teams`]): `None` for free-for-all, where
    /// every seat is its own. Give it the mode a table plays
    /// ([`Self::played_by`]).
    pub fn sides(self) -> Option<[u8; MAX_PLAYERS]> {
        match self {
            TeamMode::Solo => None,
            TeamMode::Pairs | TeamMode::Trios => {
                Some(std::array::from_fn(|seat| self.team_of(seat as u8)))
            }
        }
    }

    /// The mode whose [`Self::sides`] a board's rules hold: free-for-all
    /// for none, and for a split no mode makes (a hand-edited file).
    pub fn of_sides(sides: Option<[u8; MAX_PLAYERS]>) -> TeamMode {
        TeamMode::ALL
            .into_iter()
            .find(|mode| mode.sides() == sides)
            .unwrap_or(TeamMode::Solo)
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

/// The scoring row as it reads at a table of `seats`, in setup's step 1
/// and on the lobby's terms card: what will be scored, which a mode the
/// table does not fit is not (the round falls back to free-for-all). Where
/// the mode can be dialled (`dialled`), the row also says the mode set and
/// the seats it starts at, so a mode set before the table fits it is not
/// lost from sight.
pub fn row_value(tr: &crate::app::i18n::Tr, mode: TeamMode, seats: u8, dialled: bool) -> String {
    let played = mode.played_by(seats);
    match mode.fits_from(seats).filter(|_| dialled && played != mode) {
        Some(from) => crate::app::i18n::fill(
            tr.team_mode_waits,
            &[
                ("m", tr.team_mode_names[mode.index()]),
                ("n", &from.to_string()),
            ],
        ),
        None => tr.team_modes[played.index()].to_string(),
    }
}

impl Cycle for TeamMode {
    const VARIANTS: &'static [Self] = &TeamMode::ALL;
}

/// How a round is scored, from inputs already read: see [`Scoring`],
/// which a system asks.
///
/// Online the answer is the host's, carried in the agreed terms: as each
/// peer's own setting, two people could watch the same round and be shown
/// different winners. A `replay` being watched is scored the way it was
/// played, which it records: read off the setting, a round played in pairs
/// and watched after switching to free-for-all crowned its top single
/// seat. Otherwise it is `local`: this machine's mode ([`local`]) before a
/// round, and the board's own ([`on_board`]) once one is on the table.
/// Either way a mode that does not fit the seat count falls back to
/// free-for-all.
pub(crate) fn in_play(
    local: TeamMode,
    online: &net::Online,
    replay: Option<&crate::sim::Replay>,
    seats: u8,
) -> TeamMode {
    let wanted = match (replay, &online.0) {
        (Some(replay), _) => recorded(replay),
        (None, Some(session)) => session.terms.teams(),
        (None, None) => local,
    };
    wanted.played_by(seats)
}

/// How a round played at this machine is scored: the player's setting,
/// except on the daily. The daily is one board the whole world plays on
/// the same table, every seat for itself, and a score is only comparable
/// with another day's (and another player's) when it was not won as a
/// pair: its four seats fit pairs, and under that setting the results
/// card, the recording, the shelf and the trophies all crowned a team.
pub(crate) fn local(settings: &GameSettings, daily: &crate::app::Daily) -> TeamMode {
    if daily.active {
        TeamMode::Solo
    } else {
        settings.team_mode
    }
}

/// How the round on `board` is scored offline: the teams its rules hold,
/// which Gull Attack and the AI play by. A round set up here was given
/// [`local`]'s; a pasted code brings the teams it was played in, and read
/// off the setting instead, a pairs code pasted under free-for-all was
/// played in pairs but crowned one seat.
pub(crate) fn on_board(board: &crate::sim::Board) -> TeamMode {
    TeamMode::of_sides(board.rules().teams)
}

/// This machine's own say in how a round is scored ([`local`]). Its own
/// param for a system that writes the online session, which [`Scoring`]
/// reads: such a system hands [`in_play`] this and the session itself.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct LocalScoring<'w> {
    pub settings: bevy::prelude::Res<'w, GameSettings>,
    pub daily: bevy::prelude::Res<'w, crate::app::Daily>,
}

impl LocalScoring<'_> {
    /// The mode a round played here, offline, is scored in: see [`local`].
    pub fn mode(&self) -> TeamMode {
        local(&self.settings, &self.daily)
    }
}

/// How the round is scored, and so who leads it: what a system asks
/// rather than reading the resources [`in_play`] goes by itself.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Scoring<'w> {
    pub local: LocalScoring<'w>,
    pub online: bevy::prelude::Res<'w, net::Online>,
    pub playback: bevy::prelude::Res<'w, crate::app::Playback>,
    pub sim: bevy::prelude::Res<'w, crate::app::Sim>,
}

impl Scoring<'_> {
    /// How the round on screen is scored, a replay's the way it was played
    /// and an offline one by its board ([`on_board`]).
    pub fn mode(&self, seats: u8) -> TeamMode {
        let replay = self.playback.0.as_ref().map(|(replay, _)| replay);
        in_play(on_board(&self.sim.0), &self.online, replay, seats)
    }

    /// How the round being played here is scored, never one being
    /// watched: a series' tally, which a replay plays no part in.
    pub fn live_mode(&self, seats: u8) -> TeamMode {
        in_play(self.local.mode(), &self.online, None, seats)
    }

    /// Which seats lead the round on screen ([`leading_seats`]): the one
    /// function the board's crown and the score panel's both ask, so the
    /// two never disagree.
    ///
    /// [`leading_seats`]: crate::app::seating::leading_seats
    pub fn leaders(&self, scores: &[u32; MAX_PLAYERS], seats: u8) -> [bool; MAX_PLAYERS] {
        crate::app::seating::leading_seats(scores, seats, self.mode(seats))
    }
}

/// The scoring mode a recording was played under.
pub fn recorded(replay: &crate::sim::Replay) -> TeamMode {
    TeamMode::from_index(usize::from(replay.teams))
}

/// Who took a round, named the way the results card's headline names
/// them: the top seat, or under a team mode every seat of the top team
/// joined ("Anna+Bo"). `None` for a dead heat. `label` names a seat.
///
/// What a round is filed under on the shelf, live and pasted alike, so
/// the file says who the card said won: filed by the first seat of the
/// winning team, a pairs round went on the shelf as one player's win.
pub fn winner_label(
    scores: &[u32; MAX_PLAYERS],
    seats: u8,
    mode: TeamMode,
    label: impl Fn(u8) -> String,
) -> Option<String> {
    let leaders = crate::app::seating::leading_seats(scores, seats, mode);
    let seat = leaders.iter().position(|&led| led)? as u8;
    Some(match mode {
        TeamMode::Solo => label(seat),
        TeamMode::Pairs | TeamMode::Trios => team_label(mode, mode.team_of(seat), seats, label),
    })
}

/// A team's seats, each named by `label`, joined: see [`label`].
pub fn team_label(mode: TeamMode, team: u8, seats: u8, label: impl Fn(u8) -> String) -> String {
    mode.seats_of(team, seats)
        .into_iter()
        .map(label)
        .collect::<Vec<_>>()
        .join("+")
}

/// Each team's total, indexed by team.
///
/// Added up to the most a total can hold rather than past it: a kept
/// round's scores are read from a file anyone can edit, and every reader
/// of a total (the card, the banner, the shelf) goes through here.
pub fn team_scores(scores: &[u32; MAX_PLAYERS], seats: u8, mode: TeamMode) -> Vec<u32> {
    (0..mode.teams(seats))
        .map(|team| {
            mode.seats_of(team, seats)
                .iter()
                .fold(0u32, |total, &seat| {
                    total.saturating_add(scores[usize::from(seat)])
                })
        })
        .collect()
}

/// What the top of the table banked: the winner's score, or under a team
/// mode the winning team's total, and for a dead heat the score the
/// leaders tied on. The number on the results card's first row, which
/// ranks by [`team_scores`] too; what the replay shelf says a round was
/// won with.
pub fn top_total(scores: &[u32; MAX_PLAYERS], seats: u8, mode: TeamMode) -> u32 {
    team_scores(scores, seats, mode)
        .into_iter()
        .max()
        .unwrap_or(0)
}

/// Each seat's place when the tide comes in, 1 for first, ranked the way
/// the results card ranks its rows: by the team's total under a team mode,
/// every seat sharing its team's place, and level totals sharing a place.
/// What a bot is told it finished, so its `end` agrees with the card.
pub fn placings(scores: &[u32; MAX_PLAYERS], seats: u8, mode: TeamMode) -> Vec<u32> {
    let totals = team_scores(scores, seats, mode);
    let places = crate::bots::seat::ranks(&totals);
    (0..seats)
        .map(|seat| places[usize::from(mode.team_of(seat))])
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
    team_label(mode, team, seats, |seat| names.label(settings.tr(), seat))
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
        // A total stops at the most it can hold: kept rounds' scores come
        // from files anyone can edit.
        let most = [u32::MAX; MAX_PLAYERS];
        assert_eq!(team_scores(&most, 6, TeamMode::Trios), vec![u32::MAX; 2]);
        assert_eq!(top_total(&most, 4, TeamMode::Pairs), u32::MAX);
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

    /// A pasted code plays by the teams on its board, Gull Attack and the
    /// AI included, so it is scored by them too, whatever the paster's
    /// setting says: both ways round.
    #[test]
    fn a_pasted_round_is_scored_by_its_own_board() {
        let online = net::Online::default();
        let mut board = crate::sim::Board::new(5, 5, 0);
        board.rules_mut().teams = TeamMode::Pairs.sides();
        assert_eq!(in_play(on_board(&board), &online, None, 4), TeamMode::Pairs);
        board.rules_mut().teams = None;
        assert_eq!(in_play(on_board(&board), &online, None, 4), TeamMode::Solo);
        for mode in TeamMode::ALL {
            assert_eq!(TeamMode::of_sides(mode.sides()), mode);
        }
        // A split no mode makes, from a hand-edited file, is scored seat by seat.
        assert_eq!(TeamMode::of_sides(Some([0, 1, 1, 0, 2, 2])), TeamMode::Solo);
    }

    /// The daily's four seats fit pairs, and it was crowned a pair under
    /// that setting: on its results card, in its recording, on the shelf
    /// and in the trophies, all of which ask this.
    #[test]
    fn the_daily_is_every_seat_for_itself_whatever_the_setting() {
        let settings = GameSettings {
            team_mode: TeamMode::Pairs,
            ..GameSettings::default()
        };
        let online = net::Online::default();
        let mut daily = crate::app::Daily::default();
        let mode = |daily: &crate::app::Daily| in_play(local(&settings, daily), &online, None, 4);
        assert_eq!(mode(&daily), TeamMode::Pairs);
        daily.active = true;
        assert_eq!(mode(&daily), TeamMode::Solo);
    }
}
