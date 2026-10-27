//! Partners at an online table: the host picks them on the lobby card
//! before the start, by the same rule the couch's step 3 picks them
//! ([`crate::app::partners`]), and every screen at the beach shows the
//! table in the seats that makes, team by team.
//!
//! Only the host picks: the table is the host's to deal, and a pick moves
//! other people. A pick moves people between seats, never a team between
//! seats, so the teams stay the fair split by seat. The seats each place
//! is put in are remembered for every deal of the match
//! ([`crate::app::net::order`]) and travel in the terms
//! ([`MatchTerms::order`]), so the joiners' cards and the line-up show
//! the teams the host picked. Between the rounds of a series the teams
//! hold as they were picked: there is no picking on the results card.

use super::*;
use crate::app::match_setup::TermsDials;
use crate::app::net::order::{Arranged, seat_order};
use crate::app::teams::TeamMode;
use crate::bots::listener::BotId;

/// The table as it is seated, beside [`LobbyState::table`]'s places: the
/// seat each place sits in, how many AI sit after the places, and how the
/// table is scored. Worked out each tick on the host
/// from its own dials and picks, and read off the roster's terms on a
/// joiner.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Seated {
    pub order: [u8; MAX_PLAYERS],
    pub ai: u8,
    pub teams: TeamMode,
}

impl Default for Seated {
    fn default() -> Self {
        Seated {
            order: crate::transport::IN_SEAT_ORDER,
            ai: 0,
            teams: TeamMode::Solo,
        }
    }
}

impl Seated {
    /// The table a roster's terms describe, its `places` the people and
    /// bots named on it.
    pub fn from_terms(terms: &MatchTerms, places: u8) -> Seated {
        let seats = places.saturating_add(terms.bots).min(MAX_PLAYERS as u8);
        Seated {
            order: terms.order,
            ai: seats - places.min(seats),
            teams: terms.teams().played_by(seats),
        }
    }

    /// The seat place `place` sits in.
    pub fn seat_of(&self, place: usize) -> u8 {
        self.order.get(place).copied().unwrap_or(place as u8)
    }
}

/// One seat of the table as the lobby card shows it: who holds it, its
/// seat, and whether a bot or the AI does.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SeatShown {
    pub seat: u8,
    pub name: String,
    pub kind: crate::sim::SeatKind,
}

/// The table seat by seat, the AI's seats included, in the order the card
/// lists them: under a team mode team by team, as the line-up lists them
/// ([`crate::app::lineup::seat_order`]); else the people and bots in their
/// places, and no AI, which the terms card counts.
pub fn seats_shown(state: &LobbyState, tr: &crate::app::i18n::Tr) -> Vec<SeatShown> {
    let seated = state.seated;
    let places = state.table.len().min(MAX_PLAYERS);
    let seats = (places as u8 + seated.ai).min(MAX_PLAYERS as u8);
    let at = |seat: u8| -> SeatShown {
        let place = (0..usize::from(seats)).find(|&place| seated.seat_of(place) == seat);
        match place.filter(|&place| place < places) {
            Some(place) => SeatShown {
                seat,
                name: state.table[place].clone(),
                kind: state
                    .table_kinds
                    .get(place)
                    .copied()
                    .unwrap_or(crate::sim::SeatKind::Human),
            },
            // As the scoreboard names a seat the AI holds: its label, marked.
            None => SeatShown {
                seat,
                name: format!("{}{}", crate::app::seat_label(tr, seat), tr.tag_ai),
                kind: crate::sim::SeatKind::Ai,
            },
        }
    };
    match seated.teams {
        TeamMode::Solo => (0..places).map(|place| at(seated.seat_of(place))).collect(),
        TeamMode::Pairs | TeamMode::Trios => crate::app::lineup::seat_order(seated.teams, seats)
            .into_iter()
            .map(|(seat, _)| at(seat))
            .collect(),
    }
}

/// The host's table as its places stand: the peers at it, in peer order,
/// and its own bots by who they are, after the host's own place 0.
fn places(hosted: &Hosted, door: &[(BotId, String)]) -> (Vec<usize>, Vec<BotId>) {
    let peers: Vec<usize> = hosted
        .peers
        .at_the_table()
        .map(|(peer, _)| peer)
        .take(MAX_PLAYERS - 1)
        .collect();
    let room = MAX_PLAYERS - 1 - peers.len();
    let bots = door.iter().take(room).map(|(id, _)| *id).collect();
    (peers, bots)
}

/// The host's table as seated now ([`Seated`]): its places in the seats
/// its partner picks put them in, from what the host remembers of them
/// ([`seat_order`]) while a team mode is played, behind them the AI its
/// dials deal, scored as its setting says where the table fits it.
pub(super) fn host_seated(
    state: &LobbyState,
    config: &MatchConfig,
    teams: TeamMode,
    door: &[(BotId, String)],
) -> Seated {
    let (Some(hosted), Some(dealt)) = (state.hosted(), hosts_dealt(state, config)) else {
        return Seated::default();
    };
    let humans = dealt.seats - dealt.bots;
    let teams = teams.played_by(dealt.seats);
    let held = crate::app::net::order::picks_hold(teams);
    let order = match hosted.arranged.as_ref().filter(|_| held) {
        None => crate::transport::IN_SEAT_ORDER,
        Some(arranged) => {
            let (peers, bots) = places(hosted, door);
            let wants: Vec<Option<u8>> = std::iter::once(None)
                .chain(
                    peers
                        .iter()
                        .map(|&peer| hosted.peers.get(peer).and_then(|row| row.seat_wish)),
                )
                .chain(bots.iter().map(|id| {
                    arranged
                        .bots
                        .iter()
                        .find(|(bot, _)| bot == id)
                        .map(|&(_, seat)| seat)
                }))
                .collect();
            seat_order(&wants, &arranged.ai, humans, dealt.seats)
        }
    };
    Seated {
        order,
        ai: dealt.bots,
        teams,
    }
}

/// Whether the host can pick partners at its table as it sits: a team
/// mode is played there.
pub fn partners_open(state: &LobbyState) -> bool {
    state.hosting() && state.seated.teams != TeamMode::Solo
}

/// The host picks the next partners for `seat` ([`crate::app::partners`]):
/// the people in the seats the pick swaps trade them, and each place's
/// new seat is remembered, the peers' on their rows, the host's bots by
/// who they are and the AI's by seat. P1's seat is the host's and never
/// moves; online nobody else is pinned. Nothing happens past the table or
/// where the seat can be given no other partner. The seat the picker sits
/// in afterwards, when a pick was made.
pub(super) fn pick_partners(
    state: &mut LobbyState,
    door: &[(BotId, String)],
    seat: u8,
) -> Option<u8> {
    let seated = state.seated;
    let places_here = state.table.len().min(MAX_PLAYERS) as u8;
    let seats = places_here.saturating_add(seated.ai).min(MAX_PLAYERS as u8);
    if seat >= seats || seated.teams == TeamMode::Solo {
        return None;
    }
    let mut who = [0u8; MAX_PLAYERS];
    for place in 0..seats {
        who[usize::from(seated.seat_of(usize::from(place)))] = place;
    }
    let pick = crate::app::partners::next_pick(seated.teams, seats, seat, (&[0], &[]), &who)?;
    let mut order = seated.order;
    for slot in order.iter_mut().take(usize::from(seats)) {
        *slot = pick.moved(*slot);
    }
    let hosted = state.hosted_mut()?;
    let (peers, bots) = places(hosted, door);
    for (at, &peer) in peers.iter().enumerate() {
        hosted.peers.row(peer).seat_wish = Some(order[at + 1]);
    }
    let first_bot = 1 + peers.len();
    hosted.arranged = Some(Arranged {
        bots: bots
            .iter()
            .enumerate()
            .map(|(at, id)| (*id, order[first_bot + at]))
            .collect(),
        ai: (places_here..seats)
            .map(|place| order[usize::from(place)])
            .collect(),
    });
    state.seated.order = order;
    Some(pick.moved(seat))
}

/// The rows the host's pad picks a seat from while picking partners: each
/// seat's number and who holds it, in the order the card lists them, team
/// by team.
pub fn partner_rows(state: &LobbyState, tr: &crate::app::i18n::Tr) -> Vec<(u8, String)> {
    seats_shown(state, tr)
        .into_iter()
        .map(|shown| (shown.seat + 1, shown.name))
        .collect()
}
