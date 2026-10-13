//! A round played with bots in it: each bot seat driven from its replies
//! on the live clock, the AI standing in for one that is gone, and online,
//! the acts handed to the lockstep followed to the frames they land on.

use super::doorway::Doorway;
use crate::bots::cursor::SeatCursor;
use crate::bots::listener::{BotId, GameLink, View};
use crate::bots::lookahead;
use crate::bots::protocol::{self, Act, Clock, Outcome, SeatKind, Table, You};
use crate::bots::seat::{BotDriver, Journal, Turn, act_of, outcome, placings, refusal};
use crate::sim::{Board, BotLevel, MAX_PLAYERS, PlayerAction, Refusal, Replay, bot_action};
use bevy::prelude::*;
use std::time::{Duration, Instant};

/// How long a dropped bot's seat idles before the game's AI stands in.
pub const GRACE: Duration = Duration::from_secs(5);

/// The level the game's AI plays a bot's seat at while the bot is away,
/// and the seat's level in a round copied with a bot in it.
pub const STAND_IN: BotLevel = BotLevel::Normal;

/// The one deadline a bot at a table with people gets: a tick.
const LIVE_DEADLINE_MS: u32 = 1000 / crate::sim::TICKS_PER_SECOND;

/// What is happening at a bot's seat that the table should be told.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum News {
    /// Its connection went and the grace is up: the AI is in its chair.
    StandIn,
    /// It came back, and has its seat again.
    Back,
}

/// One round played with bots in it.
pub struct BotRound {
    /// Online: the seats are this peer's own (route 1) or the host's bots
    /// (route 2), and their acts are committed to the lockstep a frame
    /// ahead. Each is kept with the frame it lands on, so its outcome can
    /// be read when that frame is played.
    online: bool,
    in_flight: std::collections::VecDeque<InFlight>,
    link: GameLink,
    game: u32,
    drivers: Vec<BotDriver>,
    cursors: [Option<SeatCursor>; MAX_PLAYERS],
    last: [Option<(&'static str, Outcome)>; MAX_PLAYERS],
    dropped_at: [Option<Instant>; MAX_PLAYERS],
    /// Since when a seat's bot has been sent something and said nothing:
    /// a connection that hangs without closing is as gone as one that
    /// closed, and the AI stands in for it the same way.
    quiet_since: [Option<Instant>; MAX_PLAYERS],
    standing_in: [bool; MAX_PLAYERS],
    /// How long a seat waits on a bot that is gone before the AI stands
    /// in: [`GRACE`], shorter in the tests.
    grace: Duration,
    /// Online, the act each seat handed the lockstep that it has not yet
    /// taken: a paused or stalled commit hands it back, and it goes out
    /// again on the next, rather than being walked past and lost.
    unconfirmed: [Option<(PlayerAction, Option<Act>)>; MAX_PLAYERS],
    /// Every bot has been told the round is over.
    finished: bool,
    names: Vec<String>,
    kinds: Vec<SeatKind>,
    clock: Clock,
    log: Box<dyn std::io::Write + Send + Sync>,
}

/// An act committed to the lockstep, waiting for its frame to be played.
#[derive(Clone, Copy)]
struct InFlight {
    frame: u32,
    seat: u8,
    act: Act,
    action: PlayerAction,
    refused: Option<Refusal>,
}

/// An act the seat layer committed this tick, and what the sim would say
/// to it, for the outcome once the tick has run.
pub struct Committed {
    seat: u8,
    act: Option<Act>,
    action: PlayerAction,
    refused: Option<Refusal>,
}

impl BotRound {
    /// Open a round for the bots in `seats`: a driver and a hand for each,
    /// their game routed to this round, and `hello` to every one that is
    /// connected. `names` and `kinds` are the whole table's.
    pub(super) fn open(
        door: &Doorway,
        board: &Board,
        seats: &[(u8, BotId)],
        names: Vec<String>,
        kinds: Vec<SeatKind>,
        online: bool,
        game: u32,
    ) -> BotRound {
        let deadline = Duration::from_millis(u64::from(LIVE_DEADLINE_MS));
        // The route first, then who is connected: a bot that comes back in
        // between is seen connected or heard coming back, never neither.
        let link = door
            .listener()
            .open_game(game, seats.iter().map(|&(seat, id)| (id, seat)).collect());
        let drivers: Vec<BotDriver> = seats
            .iter()
            .map(|&(seat, id)| {
                let name = names.get(usize::from(seat)).cloned().unwrap_or_default();
                BotDriver::new(id, seat, name, door.listener().connected(id), deadline)
            })
            .collect();
        let mut cursors = [None; MAX_PLAYERS];
        for &(seat, _) in seats {
            cursors[usize::from(seat)] = Some(SeatCursor::home(board, seat));
        }
        let now = Instant::now();
        let mut quiet_since = [None; MAX_PLAYERS];
        for &(seat, _) in seats {
            // Waiting on its `ready` from the start.
            quiet_since[usize::from(seat)] = Some(now);
        }
        let round = BotRound {
            online,
            in_flight: std::collections::VecDeque::new(),
            link,
            game,
            drivers,
            cursors,
            last: [None; MAX_PLAYERS],
            dropped_at: [None; MAX_PLAYERS],
            quiet_since,
            standing_in: [false; MAX_PLAYERS],
            grace: GRACE,
            unconfirmed: [None; MAX_PLAYERS],
            finished: false,
            names,
            kinds,
            clock: Clock {
                live: true,
                deadline_ms: LIVE_DEADLINE_MS,
                // Online every seat's input is committed this far ahead for
                // the whole table, a bot's like a person's.
                input_delay: match online {
                    true => crate::sim::DEFAULT_DELAY,
                    false => 0,
                },
            },
            log: Box::new(std::io::sink()),
        };
        for driver in &round.drivers {
            if driver.connected {
                let hello = protocol::hello(board, &round.table(), driver.seat, false);
                round.link.send(driver.id, &hello);
            }
        }
        round
    }

    fn table(&self) -> Table<'_> {
        Table {
            game: self.game,
            names: &self.names,
            kinds: &self.kinds,
            clock: self.clock,
            cursor: true,
        }
    }

    /// Take everything the bots have said since the last tick.
    fn drain(&mut self, t: u64, news: &mut Vec<(u8, News)>) {
        let mut console = |line: String| info!("{line}");
        let mut journal = Journal {
            log: &mut self.log,
            console: &mut console,
            echo_notes: true,
            game: self.game,
        };
        while let Some(msg) = self.link.try_recv() {
            let seat = msg.seat();
            if let Some(driver) = self.drivers.iter_mut().find(|d| d.seat == seat) {
                // Anything at all from it: it is not hung.
                if let Some(quiet) = self.quiet_since.get_mut(usize::from(seat)) {
                    *quiet = None;
                }
                driver.take(msg, t, &mut journal);
            }
        }
        // A bot comes back by resuming (`hello` again before its next
        // tick), or by speaking again after a silence.
        let now = Instant::now();
        for driver in &mut self.drivers {
            let seat = usize::from(driver.seat);
            let gone_for = match driver.connected {
                true => {
                    self.dropped_at[seat] = None;
                    self.quiet_since[seat].map(|since| now.duration_since(since))
                }
                false => Some(now.duration_since(*self.dropped_at[seat].get_or_insert(now))),
            };
            match gone_for {
                Some(gone) if gone >= self.grace => {
                    if !std::mem::replace(&mut self.standing_in[seat], true) {
                        news.push((driver.seat, News::StandIn));
                    }
                }
                Some(_) => {}
                None => {
                    if std::mem::take(&mut self.standing_in[seat]) {
                        news.push((driver.seat, News::Back));
                    }
                }
            }
        }
    }

    /// Fill the bot seats' actions for the tick about to run from `board`.
    pub fn commit(
        &mut self,
        board: &Board,
        actions: &mut [PlayerAction; MAX_PLAYERS],
        news: &mut Vec<(u8, News)>,
    ) -> Vec<Committed> {
        let t = board.ticks();
        self.drain(t, news);
        let mut console = |line: String| info!("{line}");
        let mut journal = Journal {
            log: &mut self.log,
            console: &mut console,
            echo_notes: true,
            game: self.game,
        };
        let mut committed = Vec::new();
        for driver in &mut self.drivers {
            let seat = driver.seat;
            let slot = usize::from(seat);
            let (action, act) = if let Some(held) = self.unconfirmed[slot] {
                // Online, what the lockstep has not taken yet goes again,
                // with the hand and the bot's order held where they are.
                held
            } else if self.standing_in[slot] {
                // The AI walks by the same rule, read off the board.
                let action = bot_action(board, seat, STAND_IN);
                (action, act_of(action))
            } else {
                let order = driver.order(&mut journal);
                match &mut self.cursors[slot] {
                    Some(cursor) => cursor.step(board, seat, order, true),
                    None => (PlayerAction::None, None),
                }
            };
            actions[slot] = action;
            if self.online && action != PlayerAction::None {
                self.unconfirmed[slot] = Some((action, act));
            }
            committed.push(Committed {
                seat,
                act,
                action,
                refused: refusal(board, seat, action),
            });
        }
        committed
    }

    /// After the tick: what became of each act, then the next tick out.
    pub fn after(&mut self, board: &Board, committed: Vec<Committed>) {
        let t = board.ticks();
        for c in committed {
            let Some(act) = c.act else {
                continue;
            };
            if let Some(result) =
                outcome(act, c.action, c.refused, board, c.seat, t.saturating_sub(1))
            {
                self.last[usize::from(c.seat)] = Some((act.token(), result));
            }
        }
        let cursors: Vec<Option<(u8, u8)>> = (0..self.names.len())
            .map(|seat| self.cursors.get(seat).copied().flatten().map(|c| c.at))
            .collect();
        *self
            .link
            .view
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(View {
            board: board.clone(),
            cursors: cursors.clone(),
            budget: vec![lookahead::CAP_LIVE; self.names.len()],
        });
        let now = Instant::now();
        for i in 0..self.drivers.len() {
            let seat = self.drivers[i].seat;
            if std::mem::take(&mut self.drivers[i].rehello) && self.drivers[i].playing() {
                let hello = protocol::hello(board, &self.table(), seat, true);
                self.link.send(self.drivers[i].id, &hello);
            }
            if self.drivers[i].turn(t, now) != Turn::Send {
                continue;
            }
            let you = You {
                seat,
                last: self.last[usize::from(seat)],
            };
            let msg = protocol::tick(board, self.game, &you, &cursors);
            if self.link.send(self.drivers[i].id, &msg) {
                self.drivers[i].sent(t, now);
                self.quiet_since[usize::from(seat)].get_or_insert(now);
            }
        }
    }

    /// Online, every bot seat's act, and what it carried out.
    pub(super) fn commit_online(
        &mut self,
        board: &Board,
        news: &mut Vec<(u8, News)>,
    ) -> Vec<(u8, PlayerAction, Option<Act>)> {
        let mut actions = [PlayerAction::None; MAX_PLAYERS];
        self.commit(board, &mut actions, news)
            .into_iter()
            .map(|c| (c.seat, c.action, c.act))
            .collect()
    }

    /// Online, the seats bots drive from this machine; none on a couch.
    pub(super) fn online_seats(&self) -> Vec<u8> {
        match self.online {
            true => self.drivers.iter().map(|d| d.seat).collect(),
            false => Vec::new(),
        }
    }

    /// The lockstep took `seat`'s act for `frame`: remember it until the
    /// frame is played, when its outcome is known. One it did not take
    /// (paused, stalled, or the frame given to a tide call) is not passed
    /// here, and goes out again on the next commit.
    pub(super) fn committed(
        &mut self,
        frame: u32,
        seat: u8,
        act: Option<Act>,
        action: PlayerAction,
    ) {
        if let Some(held) = self.unconfirmed.get_mut(usize::from(seat)) {
            *held = None;
        }
        if let Some(act) = act {
            self.in_flight.push_back(InFlight {
                frame,
                seat,
                act,
                action,
                refused: None,
            });
            while self.in_flight.len() > 256 {
                self.in_flight.pop_front();
            }
        }
    }

    /// `frame` is about to be played from `board`: what the sim would say
    /// to each act on it.
    pub(super) fn before_frame(&mut self, frame: u32, board: &Board) {
        for entry in self.in_flight.iter_mut().filter(|e| e.frame == frame) {
            entry.refused = refusal(board, entry.seat, entry.action);
        }
    }

    /// `frame` was played: what became of each act on it.
    pub(super) fn after_frame(&mut self, frame: u32, board: &Board) {
        while let Some(&entry) = self.in_flight.front() {
            if entry.frame > frame {
                break;
            }
            self.in_flight.pop_front();
            if entry.frame == frame
                && let Some(result) = outcome(
                    entry.act,
                    entry.action,
                    entry.refused,
                    board,
                    entry.seat,
                    u64::from(frame),
                )
            {
                self.last[usize::from(entry.seat)] = Some((entry.act.token(), result));
            }
        }
    }

    /// Send the board as it now stands to every bot awake for it, without
    /// any outcome to report first (online, outcomes arrive with frames).
    pub(super) fn send_next(&mut self, board: &Board) {
        self.after(board, Vec::new());
    }

    /// The round is over: every bot is told how it went, once, however
    /// many steps the tide stays in for. The round's recording is kept
    /// first, so a bot that asks for the replay `end` names the moment it
    /// hears it finds it there; with nothing recorded, `end` names none.
    pub(super) fn finish(
        &mut self,
        board: &Board,
        door: Option<&Doorway>,
        replay: Option<&Replay>,
    ) {
        if std::mem::replace(&mut self.finished, true) {
            return;
        }
        let n = self.names.len();
        let scores: Vec<u32> = board.scores()[..n].to_vec();
        let places = placings(&scores, &vec![false; n]);
        let kept = match (door, replay) {
            (Some(door), Some(replay)) => {
                let id = format!("p-g{}", self.game);
                let players = self.drivers.iter().map(|driver| driver.id).collect();
                door.listener()
                    .keep_replay(id.clone(), players, replay.to_text());
                Some(id)
            }
            _ => None,
        };
        for driver in &self.drivers {
            let msg = serde_json::json!({
                "type": "end", "game": self.game, "scores": scores,
                "placing": places[usize::from(driver.seat)], "replay": kept,
            });
            self.link.send(driver.id, &msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::bot_seats::BotSeats;
    use crate::app::bot_seats::test_bots::{TestBot, poll_until};
    use std::io::BufRead;

    /// Seat 1's castle, and the board two seats play on.
    const CASTLE: (u8, u8) = (7, 5);

    fn beach() -> Board {
        let mut board = Board::new(9, 7, 3);
        board.set_tile(1, 1, crate::sim::TileKind::Castle(0));
        board.set_tile(CASTLE.0, CASTLE.1, crate::sim::TileKind::Castle(1));
        board
    }

    /// A round with a bot registered in seat 1, against a person in seat 0.
    fn a_round_with_a_bot(online: bool) -> (BotSeats, TestBot, Board) {
        let mut door = Doorway::open().expect("a port on this machine");
        door.seat_bots(&[1]);
        let bot = TestBot::register(&door.string(&door.chairs[0]));
        assert!(poll_until(&mut door, |d| d.ready()), "the bot sat down");
        let id = door.chairs[0].bot.expect("seated");
        let board = beach();
        let round = BotRound::open(
            &door,
            &board,
            &[(1, id)],
            vec!["Ana".into(), "Greedy".into()],
            vec![SeatKind::Human, SeatKind::Bot],
            online,
            1,
        );
        let bots = BotSeats {
            door: Some(door),
            round: Some(round),
            ..BotSeats::default()
        };
        (bots, bot, board)
    }

    fn over(mut board: Board) -> (Board, Replay) {
        let replay = Replay::new(crate::sim::Level::from_board("T", 3, board.clone()));
        board.rules_mut().round_length = Some(1);
        board.tick_idle();
        assert!(board.round_over());
        (board, replay)
    }

    /// The `end` a couch bot is sent names a replay it can fetch, and it is
    /// sent once, however many steps the tide stays in for.
    #[test]
    fn a_couch_bot_is_told_once_and_can_fetch_the_replay_it_is_told_of() {
        let (mut bots, mut bot, board) = a_round_with_a_bot(false);
        let (board, replay) = over(board);
        bots.after(&board, Some(Vec::new()), Some(&replay));
        let end = bot
            .next("end", Duration::from_secs(5))
            .expect("told the round is over");
        let id = end["replay"].as_str().expect("a replay named").to_string();
        bot.send(&serde_json::json!({"type": "replay", "id": id}));
        let mut answer = String::new();
        bot.stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        bot.reader.read_line(&mut answer).expect("an answer");
        let answer: serde_json::Value = serde_json::from_str(&answer).expect("json");
        assert_eq!(answer["type"], "replay", "{answer}");
        assert_eq!(answer["text"], replay.to_text());
        // The tide stays in; the bot has been told.
        bots.after(&board, Some(Vec::new()), Some(&replay));
        assert!(
            bot.next("end", Duration::from_millis(300)).is_none(),
            "told twice"
        );
    }

    /// Online, a bot seat is told the round is over like a couch one.
    #[test]
    fn an_online_bot_is_told_when_the_round_is_over() {
        let (mut bots, mut bot, board) = a_round_with_a_bot(true);
        let (board, replay) = over(board);
        bots.send_online(&board, Some(&replay));
        assert!(
            bot.next("end", Duration::from_secs(5)).is_some(),
            "no end for an online bot"
        );
    }

    /// Online, an act the lockstep did not take (a pause, a stall) is
    /// handed over again on the next commit, not lost with the walk that
    /// led to it.
    #[test]
    fn an_act_the_lockstep_did_not_take_goes_out_again() {
        let (mut bots, mut bot, board) = a_round_with_a_bot(true);
        bot.next("hello", Duration::from_secs(5)).expect("hello");
        bot.send(&serde_json::json!({"type": "ready", "game": 1}));
        let (x, y) = (CASTLE.0 - 1, CASTLE.1);
        bot.send(&serde_json::json!({
            "game": 1, "tick": 0, "act": "place", "x": x, "y": y, "dir": "left",
        }));
        let place = PlayerAction::Place {
            x,
            y,
            dir: crate::sim::Direction::Left,
        };
        let mut landed = None;
        for _ in 0..400 {
            let driven = bots.commit_online(&board);
            if let Some(&(1, action, act)) = driven.first()
                && action == place
            {
                landed = Some(act);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let act = landed.expect("the place landed");
        // Not committed: the next commit carries it again.
        let again = bots.commit_online(&board);
        assert_eq!(again.first().map(|d| d.1), Some(place), "the act was lost");
        // Committed: it is done with.
        bots.committed(5, 1, act, place);
        let after = bots.commit_online(&board);
        assert_eq!(after.first().map(|d| d.1), Some(PlayerAction::None));
    }

    /// A placement the board refused before the tick, on a tile an earlier
    /// seat freed during it, stands: the bot is told it was accepted.
    #[test]
    fn a_place_on_a_tile_freed_earlier_in_the_tick_is_accepted() {
        let (mut bots, _bot, mut board) = a_round_with_a_bot(false);
        let (x, y) = (CASTLE.0 - 1, CASTLE.1);
        let dir = crate::sim::Direction::Left;
        assert!(board.place_signpost(1, x, y, dir));
        board.tick_idle();
        let round = bots.round.as_mut().expect("a round");
        round.after(
            &board,
            vec![Committed {
                seat: 1,
                act: Some(Act::Place { x, y, dir }),
                action: PlayerAction::Place { x, y, dir },
                refused: Some(Refusal::RivalPost),
            }],
        );
        assert_eq!(round.last[1], Some(("place", Outcome::Accepted)));
    }

    /// A bot that is connected but says nothing (hung, or its machine left
    /// the network without closing) is stood in for like one that dropped,
    /// and has its seat back once it speaks.
    #[test]
    fn a_silent_bot_is_stood_in_for_and_gets_its_seat_back() {
        let (mut bots, mut bot, board) = a_round_with_a_bot(false);
        bots.round.as_mut().expect("a round").grace = Duration::from_millis(50);
        let mut actions = [PlayerAction::None; MAX_PLAYERS];
        let mut stood_in = false;
        for _ in 0..100 {
            let committed = bots.commit(&board, &mut actions);
            bots.after(&board, committed, None);
            if bots.news.contains(&(1, News::StandIn)) {
                stood_in = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(stood_in, "nobody stood in for a silent bot");
        bots.news.clear();
        bot.send(&serde_json::json!({"type": "ready", "game": 1}));
        let mut back = false;
        for _ in 0..400 {
            let committed = bots.commit(&board, &mut actions);
            if bots.news.contains(&(1, News::Back)) {
                back = true;
                break;
            }
            bots.after(&board, committed, None);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(back, "the bot spoke and did not get its seat back");
    }
}
