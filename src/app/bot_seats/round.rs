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
    /// ahead. `None` on a couch.
    lockstep: Option<Lockstep>,
    link: GameLink,
    game: u32,
    /// Every bot seat, in the order they were opened.
    seats: Vec<SeatHand>,
    /// How long a seat waits on a bot that is gone before the AI stands
    /// in: [`GRACE`], shorter in the tests.
    grace: Duration,
    /// Every bot has been told the round is over.
    finished: bool,
    names: Vec<String>,
    kinds: Vec<SeatKind>,
    clock: Clock,
    log: Box<dyn std::io::Write + Send + Sync>,
}

/// One bot seat: its driver, its hand, and how it has been lately.
struct SeatHand {
    driver: BotDriver,
    cursor: SeatCursor,
    /// What became of its last act, for the next tick's `you.last`.
    last: Option<(&'static str, Outcome)>,
    /// When its connection went, while it is gone.
    dropped_at: Option<Instant>,
    /// Since when its bot has been sent something and said nothing: a
    /// connection that hangs without closing is as gone as one that
    /// closed, and the AI stands in for it the same way.
    quiet_since: Option<Instant>,
    /// The game's AI is in its chair.
    standing_in: bool,
}

/// What an online round keeps of the acts it hands the lockstep.
#[derive(Default)]
struct Lockstep {
    /// Acts committed to the lockstep, each with the frame it lands on, so
    /// its outcome can be read when that frame is played.
    in_flight: std::collections::VecDeque<InFlight>,
    /// The act each seat handed the lockstep that it has not yet taken: a
    /// paused or stalled commit hands it back, and it goes out again on
    /// the next, rather than being walked past and lost.
    unconfirmed: [Option<(PlayerAction, Option<Act>)>; MAX_PLAYERS],
}

/// What a bot at a table with people is told of the game: the fair cursor
/// always, since a person is at it.
fn live_table<'a>(
    game: u32,
    names: &'a [String],
    kinds: &'a [SeatKind],
    clock: Clock,
) -> Table<'a> {
    Table {
        game,
        names,
        kinds,
        clock,
        cursor: true,
    }
}

/// Where a round's seats write what they did: its decision log, and the
/// game's own log with every note, for the author watching it.
fn journal<'a>(
    log: &'a mut (dyn std::io::Write + Send + Sync),
    console: &'a mut dyn FnMut(String),
    game: u32,
) -> Journal<'a> {
    Journal {
        log,
        console,
        echo_notes: true,
        game,
    }
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
        let now = Instant::now();
        let seats: Vec<SeatHand> = seats
            .iter()
            .map(|&(seat, id)| {
                let name = names.get(usize::from(seat)).cloned().unwrap_or_default();
                SeatHand {
                    driver: BotDriver::new(id, seat, name, door.listener().connected(id), deadline),
                    cursor: SeatCursor::home(board, seat),
                    last: None,
                    dropped_at: None,
                    // Waiting on its `ready` from the start.
                    quiet_since: Some(now),
                    standing_in: false,
                }
            })
            .collect();
        let lockstep = online.then(Lockstep::default);
        let round = BotRound {
            // Online every seat's input is committed this far ahead for
            // the whole table, a bot's like a person's.
            clock: Clock {
                live: true,
                deadline_ms: LIVE_DEADLINE_MS,
                input_delay: match lockstep {
                    Some(_) => crate::sim::DEFAULT_DELAY,
                    None => 0,
                },
            },
            lockstep,
            link,
            game,
            seats,
            grace: GRACE,
            finished: false,
            names,
            kinds,
            log: Box::new(std::io::sink()),
        };
        for hand in &round.seats {
            let driver = &hand.driver;
            if driver.connected {
                let hello = protocol::hello(board, &round.table(), driver.seat, false);
                round.link.send(driver.id, &hello);
            }
        }
        round
    }

    fn table(&self) -> Table<'_> {
        live_table(self.game, &self.names, &self.kinds, self.clock)
    }

    /// The bot seat `seat`, if a bot holds it.
    fn hand(&self, seat: usize) -> Option<&SeatHand> {
        self.seats
            .iter()
            .find(|hand| usize::from(hand.driver.seat) == seat)
    }

    /// Take everything the bots have said since the last tick.
    fn drain(&mut self, t: u64, news: &mut Vec<(u8, News)>) {
        let mut console = |line: String| info!("{line}");
        let mut journal = journal(&mut self.log, &mut console, self.game);
        while let Some(msg) = self.link.try_recv() {
            let seat = msg.seat();
            if let Some(hand) = self.seats.iter_mut().find(|h| h.driver.seat == seat) {
                // Anything at all from it: it is not hung.
                hand.quiet_since = None;
                hand.driver.take(msg, t, &mut journal);
            }
        }
        // A bot comes back by resuming (`hello` again before its next
        // tick), or by speaking again after a silence.
        let now = Instant::now();
        for hand in &mut self.seats {
            let gone_for = match hand.driver.connected {
                true => {
                    hand.dropped_at = None;
                    hand.quiet_since.map(|since| now.duration_since(since))
                }
                false => Some(now.duration_since(*hand.dropped_at.get_or_insert(now))),
            };
            match gone_for {
                Some(gone) if gone >= self.grace => {
                    if !std::mem::replace(&mut hand.standing_in, true) {
                        news.push((hand.driver.seat, News::StandIn));
                    }
                }
                Some(_) => {}
                None => {
                    if std::mem::take(&mut hand.standing_in) {
                        news.push((hand.driver.seat, News::Back));
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
        let mut journal = journal(&mut self.log, &mut console, self.game);
        let mut committed = Vec::new();
        for hand in &mut self.seats {
            let seat = hand.driver.seat;
            let slot = usize::from(seat);
            let held = self
                .lockstep
                .as_ref()
                .and_then(|lockstep| lockstep.unconfirmed[slot]);
            let (action, act) = if let Some(held) = held {
                // Online, what the lockstep has not taken yet goes again,
                // with the hand and the bot's order held where they are.
                held
            } else if hand.standing_in {
                // The AI walks by the same rule, read off the board.
                let action = bot_action(board, seat, STAND_IN);
                (action, act_of(action))
            } else {
                let order = hand.driver.order(&mut journal);
                hand.cursor.step(board, seat, order, true)
            };
            actions[slot] = action;
            if let Some(lockstep) = &mut self.lockstep
                && action != PlayerAction::None
            {
                lockstep.unconfirmed[slot] = Some((action, act));
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
                && let Some(hand) = self.seats.iter_mut().find(|h| h.driver.seat == c.seat)
            {
                hand.last = Some((act.token(), result));
            }
        }
        let cursors: Vec<Option<(u8, u8)>> = (0..self.names.len())
            .map(|seat| self.hand(seat).map(|hand| hand.cursor.at))
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
        let BotRound {
            link,
            game,
            seats,
            names,
            kinds,
            clock,
            ..
        } = self;
        let table = live_table(*game, names, kinds, *clock);
        let mut send = |id: BotId, msg: &serde_json::Value| link.send(id, msg);
        for hand in seats {
            let seat = hand.driver.seat;
            let you = You {
                seat,
                last: hand.last,
            };
            // A tick still being answered is no matter here: the live
            // clock commits whatever is in hand, and waits for nobody.
            let turn = hand.driver.offer_tick(
                t,
                now,
                &mut send,
                || protocol::hello(board, &table, seat, true),
                || protocol::tick(board, *game, &you, &cursors),
            );
            if turn == Turn::Send {
                hand.quiet_since.get_or_insert(now);
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
        match self.lockstep {
            Some(_) => self.seats.iter().map(|hand| hand.driver.seat).collect(),
            None => Vec::new(),
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
        let Some(lockstep) = &mut self.lockstep else {
            return;
        };
        if let Some(held) = lockstep.unconfirmed.get_mut(usize::from(seat)) {
            *held = None;
        }
        if let Some(act) = act {
            lockstep.in_flight.push_back(InFlight {
                frame,
                seat,
                act,
                action,
                refused: None,
            });
            while lockstep.in_flight.len() > 256 {
                lockstep.in_flight.pop_front();
            }
        }
    }

    /// `frame` is about to be played from `board`: what the sim would say
    /// to each act on it.
    pub(super) fn before_frame(&mut self, frame: u32, board: &Board) {
        let Some(lockstep) = &mut self.lockstep else {
            return;
        };
        for entry in lockstep.in_flight.iter_mut().filter(|e| e.frame == frame) {
            entry.refused = refusal(board, entry.seat, entry.action);
        }
    }

    /// `frame` was played: what became of each act on it.
    pub(super) fn after_frame(&mut self, frame: u32, board: &Board) {
        let Some(lockstep) = &mut self.lockstep else {
            return;
        };
        while let Some(&entry) = lockstep.in_flight.front() {
            if entry.frame > frame {
                break;
            }
            lockstep.in_flight.pop_front();
            if entry.frame == frame
                && let Some(result) = outcome(
                    entry.act,
                    entry.action,
                    entry.refused,
                    board,
                    entry.seat,
                    u64::from(frame),
                )
                && let Some(hand) = self
                    .seats
                    .iter_mut()
                    .find(|hand| hand.driver.seat == entry.seat)
            {
                hand.last = Some((entry.act.token(), result));
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
                let players = self.seats.iter().map(|hand| hand.driver.id).collect();
                door.listener()
                    .keep_replay(id.clone(), players, replay.to_text());
                Some(id)
            }
            _ => None,
        };
        for hand in &self.seats {
            let driver = &hand.driver;
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
        assert_eq!(
            round.hand(1).expect("seat 1").last,
            Some(("place", Outcome::Accepted))
        );
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
