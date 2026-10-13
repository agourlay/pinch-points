//! Bots in the game itself: seats on a couch held by bots, and the seat a
//! bot drives when it joins somebody's party through its author's game.
//!
//! The game never runs a bot (`docs/bot-seats.md`). It opens a doorway, a
//! listener on this machine with a key per chair, prints the string to
//! start each bot with, and once a bot has registered drives that bot's
//! seat from its replies: on the live clock, a tick sent each fixed step
//! and the newest reply in hand committed on the next, under the fair
//! cursor, since a person is at the table. A bot whose connection drops
//! idles for a grace period and then the game's AI stands in for it,
//! until it comes back with its token.

mod card;
mod doorway;
mod round;
#[cfg(test)]
mod test_bots;

pub use card::{door_card, door_input};
pub use doorway::Doorway;
pub use round::{BotRound, Committed, News, STAND_IN};

use crate::app::i18n::fill;
use crate::app::{Controllers, SeatController, SeatKinds, SeatNames, Sim};
use crate::bots::listener::BotId;
use crate::bots::protocol::Act;
use crate::sim::{Board, MAX_PLAYERS, PlayerAction, Replay};
use bevy::prelude::*;
use std::net::SocketAddr;

/// Bots in this game: the doorway they came in by, and the round being
/// played with them.
#[derive(Resource, Default)]
pub struct BotSeats {
    pub door: Option<Doorway>,
    round: Option<BotRound>,
    /// What the table should be told, for [`announce`] to word.
    pub news: Vec<(u8, News)>,
    /// A doorway card is up, and what it is for.
    pub waiting: Option<Waiting>,
    /// What the card has to say about the last key pressed on it.
    pub feedback: String,
    /// The beach a bot is joining once it has registered (route 1).
    pub join_to: Option<SocketAddr>,
    games: u32,
}

/// What a doorway card is waiting for its bots in order to do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Waiting {
    /// Start the match being set up on this couch.
    Couch,
    /// Join the beach picked in the lobby as a bot (route 1).
    Join,
}

impl BotSeats {
    /// The doorway a bot joined a beach through (route 1) is for that
    /// beach alone. Once this machine is no longer at it (asked to leave,
    /// never answered, on another build, or gone of its own accord),
    /// the doorway closes and its bot is let go, rather than sitting
    /// registered on this machine's port with nowhere to play.
    /// `at_the_beach` is whether this machine is still greeting it or
    /// playing at it.
    pub fn leave_the_beach(&mut self, at_the_beach: bool) {
        if self.join_to.is_some() && self.waiting.is_none() && !at_the_beach {
            self.door = None;
            self.join_to = None;
        }
    }

    /// Online, the seats bots drive from this machine: this peer's own
    /// (route 1), or the host's bots (route 2).
    pub fn online_seats(&self) -> Vec<u8> {
        self.round
            .as_ref()
            .map_or_else(Vec::new, BotRound::online_seats)
    }

    /// Online, every bot seat's act, for the lockstep to commit.
    pub fn commit_online(&mut self, board: &Board) -> Vec<(u8, PlayerAction, Option<Act>)> {
        let BotSeats { round, news, .. } = self;
        match round {
            Some(round) => round.commit_online(board, news),
            None => Vec::new(),
        }
    }

    /// The lockstep took `seat`'s act for `frame`: see
    /// [`BotRound::committed`].
    pub fn committed(&mut self, frame: u32, seat: u8, act: Option<Act>, action: PlayerAction) {
        if let Some(round) = self.round.as_mut() {
            round.committed(frame, seat, act, action);
        }
    }

    /// `frame` is about to be played from `board`: see
    /// [`BotRound::before_frame`].
    pub fn before_frame(&mut self, frame: u32, board: &Board) {
        if let Some(round) = self.round.as_mut() {
            round.before_frame(frame, board);
        }
    }

    /// `frame` was played: see [`BotRound::after_frame`].
    pub fn after_frame(&mut self, frame: u32, board: &Board) {
        if let Some(round) = self.round.as_mut() {
            round.after_frame(frame, board);
        }
    }

    /// Online, after the tick: the board as it now stands to the bots.
    /// `replay` is the round's recording so far, kept for the bots once
    /// the tide is in.
    pub fn send_online(&mut self, board: &Board, replay: Option<&Replay>) {
        let BotSeats { door, round, .. } = self;
        if let Some(round) = round.as_mut() {
            round.send_next(board);
            if board.round_over() {
                round.finish(board, door.as_ref(), replay);
            }
        }
    }

    /// Fill the bot seats' actions for the tick about to run from `board`.
    /// `None` when no bot holds a seat.
    pub fn commit(
        &mut self,
        board: &Board,
        actions: &mut [PlayerAction; MAX_PLAYERS],
    ) -> Option<Vec<Committed>> {
        let BotSeats { round, news, .. } = self;
        Some(round.as_mut()?.commit(board, actions, news))
    }

    /// After a tick on this machine: outcomes, the next tick out, and the
    /// end of the round if this was it, with `replay` kept for the bots.
    pub fn after(
        &mut self,
        board: &Board,
        committed: Option<Vec<Committed>>,
        replay: Option<&Replay>,
    ) {
        let BotSeats { door, round, .. } = self;
        let (Some(round), Some(committed)) = (round.as_mut(), committed) else {
            return;
        };
        round.after(board, committed);
        if board.round_over() {
            round.finish(board, door.as_ref(), replay);
        }
    }
}

impl BotSeats {
    /// The round in play, if bots hold seats in it.
    pub fn round_mut(&mut self) -> Option<&mut BotRound> {
        self.round.as_mut()
    }
}

/// What the fixed tick reads to fill every seat: who decides for each, and
/// the bots among them.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Drivers<'w> {
    pub controllers: Res<'w, Controllers>,
    pub bots: ResMut<'w, BotSeats>,
}

impl Drivers<'_> {
    /// See [`BotSeats::commit`].
    pub fn commit(
        &mut self,
        board: &Board,
        actions: &mut [PlayerAction; MAX_PLAYERS],
    ) -> Option<Vec<Committed>> {
        self.bots.commit(board, actions)
    }

    /// After the tick: see [`BotSeats::after`].
    pub fn after(
        &mut self,
        board: &Board,
        committed: Option<Vec<Committed>>,
        replay: Option<&Replay>,
    ) {
        self.bots.after(board, committed, replay);
    }
}

/// Who sits where as a round opens, as the bots' seats need it.
#[derive(bevy::ecs::system::SystemParam)]
pub struct RoundTable<'w> {
    controllers: Res<'w, Controllers>,
    online: Res<'w, crate::app::net::Online>,
    names: ResMut<'w, SeatNames>,
    kinds: Res<'w, SeatKinds>,
}

/// Open the round's bot seats, on entering the arena: a driver and a hand
/// for every seat a bot holds, `hello` to each, and the seat named for its
/// bot. Chained after the names are resolved, which it overwrites for
/// those seats, and before the panels are drawn from them.
///
/// Online the one seat a bot can hold here is this peer's own, when it
/// joined as a bot (route 1): its name came over the wire with the rest.
pub fn begin_round(
    table: RoundTable,
    sim: Res<Sim>,
    settings: Res<crate::app::settings::GameSettings>,
    mut bots: ResMut<BotSeats>,
    mut recorder: ResMut<crate::app::Recorder>,
) {
    let RoundTable {
        controllers,
        online,
        mut names,
        kinds,
    } = table;
    bots.round = None;
    bots.news.clear();
    let mine = online
        .0
        .as_ref()
        .filter(|session| session.bot)
        .and_then(|session| session.session.seat());
    // A host's bots (route 2) are named and seated by the session already.
    let hosted: Vec<(u8, BotId)> = online
        .0
        .as_ref()
        .map(|session| {
            session
                .bots_here
                .iter()
                .map(|(seat, id, _)| (*seat, *id))
                .collect()
        })
        .unwrap_or_default();
    if mine.is_none()
        && online.0.as_ref().is_some_and(|session| session.is_host())
        && let Some(door) = &mut bots.door
    {
        let seated: Vec<BotId> = hosted.iter().map(|(_, id)| *id).collect();
        door.seat_the_table(&seated);
    }
    let seats: Vec<u8> = match mine {
        Some(seat) => vec![seat],
        None if online.0.is_some() => hosted.iter().map(|(seat, _)| *seat).collect(),
        None => (0..MAX_PLAYERS as u8)
            .filter(|&seat| controllers.0[usize::from(seat)] == SeatController::Bot)
            .collect(),
    };
    if seats.is_empty() {
        return;
    }
    bots.games += 1;
    let game = bots.games;
    let Some(door) = &bots.door else {
        return;
    };
    let tr = settings.tr();
    if online.0.is_none() {
        for &seat in &seats {
            if let Some(name) = door.seat_name(tr, seat) {
                names.0[usize::from(seat)] = name;
            }
        }
        if let Some(replay) = &mut recorder.0 {
            replay.names = names.0.clone();
        }
    }
    let board = &sim.0;
    let n = usize::from(board.seats_in_play()).max(2);
    let held: Vec<(u8, BotId)> = seats
        .iter()
        .filter_map(|&seat| {
            // Online the doorway's one chair is the bot's, whichever seat
            // the host dealt this peer.
            let id = match (mine, hosted.iter().find(|(s, _)| *s == seat)) {
                (Some(_), _) => door.chairs.first().and_then(|c| c.bot),
                (None, Some(&(_, id))) => Some(id),
                (None, None) => door
                    .chairs
                    .iter()
                    .find(|c| c.seat == seat)
                    .and_then(|c| c.bot),
            };
            Some((seat, id?))
        })
        .collect();
    let round = BotRound::open(
        door,
        board,
        &held,
        (0..n).map(|seat| names.label(tr, seat as u8)).collect(),
        kinds.0[..n].to_vec(),
        online.0.is_some(),
        game,
    );
    bots.round = Some(round);
}

/// Leaving the arena: the round is over for the bots too.
pub fn end_round(mut bots: ResMut<BotSeats>) {
    bots.round = None;
}

/// Back at the menu: nobody is setting up a table, so the doorway closes
/// and its bots are let go.
pub fn close_door(mut bots: ResMut<BotSeats>) {
    *bots = BotSeats::default();
}

/// Tell the table what happened at a bot's seat.
pub fn announce(
    mut bots: ResMut<BotSeats>,
    names: Res<SeatNames>,
    settings: Res<crate::app::settings::GameSettings>,
    mut log: ResMut<crate::app::side_panels::EventLog>,
) {
    if bots.news.is_empty() {
        return;
    }
    let tr = settings.tr();
    for (seat, news) in std::mem::take(&mut bots.news) {
        let who = names.label(tr, seat);
        let line = match news {
            News::StandIn => fill(tr.bot_stand_in, &[("p", &who)]),
            News::Back => fill(tr.bot_back, &[("p", &who)]),
        };
        log.push(line, crate::app::palette::player_color(seat));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A bot that joined a beach through this machine (route 1) is let go
    /// once the machine is no longer at that beach: asked to leave, never
    /// answered, or on another build. Its doorway does not linger.
    #[test]
    fn a_failed_join_as_a_bot_closes_its_doorway() {
        let mut bots = BotSeats {
            door: Some(Doorway::open().expect("a port on this machine")),
            join_to: Some(SocketAddr::from(([127, 0, 0, 1], 1))),
            ..BotSeats::default()
        };
        let here = bots.door.as_ref().expect("open").here();
        // Still at the beach: nothing changes.
        bots.leave_the_beach(true);
        assert!(bots.door.is_some());
        // While the card waits for its bot, nothing has been dialled yet.
        bots.waiting = Some(Waiting::Join);
        bots.leave_the_beach(false);
        assert!(bots.door.is_some(), "the card is still up");
        // Dialled, and then put out of the beach.
        bots.waiting = None;
        bots.leave_the_beach(false);
        assert!(bots.door.is_none(), "the doorway closed");
        assert!(bots.join_to.is_none());
        // And its port with it.
        let mut refused = false;
        for _ in 0..100 {
            if std::net::TcpStream::connect_timeout(&here, Duration::from_millis(50)).is_err() {
                refused = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(refused, "nobody listens on {here} any more");
        // A host's doorway (route 2) has no beach to leave.
        let mut host = BotSeats {
            door: Some(Doorway::open().expect("a port on this machine")),
            ..BotSeats::default()
        };
        host.leave_the_beach(false);
        assert!(host.door.is_some());
    }
}
