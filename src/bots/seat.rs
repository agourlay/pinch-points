//! One bot's seat in one game: what it was sent, what it answered, and
//! what it gets to do this tick.
//!
//! The rules of the clock live here, once, for every place a bot plays: the
//! arena's and a cup's own loop (`game`), and the real game at a party,
//! where the lockstep session decides when a tick happens instead.
//!
//! - The newest reply in hand wins; one answering an older tick than a
//!   reply already held is stale and dropped.
//! - A late reply still counts at the next commit, and is counted as late.
//! - A bot that asked to `wait` is not sent the ticks it skipped.
//! - A bot still busy with a tick is not sent the next one, so a bot slower
//!   than its deadline is a tick behind rather than buried under boards it
//!   will never read; the ticks it misses that way are skipped, as if it
//!   had waited.

use super::listener::{BotId, GameMsg};
use super::protocol::{Act, Outcome, Reply};
use crate::sim::{Board, PlayerAction, PlayerId, Refusal};
use std::collections::VecDeque;
use std::io::Write;
use std::time::{Duration, Instant};

/// The least a tick that was never answered holds up the next one.
const GIVE_UP: Duration = Duration::from_millis(250);
/// How many recent ticks a seat remembers sending, to time replies by.
const RECENT: usize = 64;

/// What one seat did, for the standings.
#[derive(Clone, Debug, Default)]
pub struct SeatResult {
    pub score: u32,
    /// 1 for first; seats that tie share a place.
    pub placing: u32,
    /// Crabs that walked (or were swept) into its castle.
    pub banked: u32,
    /// Times a gull carried off some of its bank.
    pub raided: u32,
    /// Placements and removals the sim turned down.
    pub rejected: u32,
    /// Ticks the bot was sent.
    pub sent: u32,
    /// Replies that came after their deadline.
    pub late: u32,
    /// Ticks sent that no reply ever answered.
    pub missed: u32,
    /// Time from each tick sent to its reply read, in milliseconds.
    pub reply_ms: Vec<f64>,
    /// Never played: not there, or not ready, when the game began.
    pub forfeit: bool,
}

/// Whether a seat is sent this tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Turn {
    /// Send it the tick.
    Send,
    /// Still answering an earlier one: not sent this tick, but the tick
    /// waits its deadline for that answer.
    Busy,
    /// Asleep (`wait`), gone, or out of the game: nothing to send or wait
    /// for.
    Skip,
}

/// Where a seat writes what it did: the decision log, and the console when
/// the author is watching.
pub struct Journal<'a> {
    pub log: &'a mut dyn Write,
    pub console: &'a mut dyn FnMut(String),
    pub echo_notes: bool,
    pub game: u32,
}

/// One bot's seat.
pub struct BotDriver {
    pub id: BotId,
    pub seat: PlayerId,
    pub name: String,
    pub connected: bool,
    pub ready: bool,
    pub forfeit: bool,
    /// When the seat started waiting: for a connection, or for `ready`.
    pub waiting_since: Instant,
    /// Send `hello` again (with `resumed`) before the next tick.
    pub rehello: bool,
    deadline: Duration,
    /// The newest reply in hand.
    in_hand: Option<(Reply, Instant)>,
    /// The first tick it wants to be sent after a `wait`.
    awake_at: u64,
    /// Recent ticks sent and when, for timing replies.
    sent: VecDeque<(u64, Instant)>,
    /// Ticks a reply has answered, among those recently sent.
    answered: VecDeque<u64>,
    answered_count: u32,
    /// The last tick sent, while no reply to it or a later one has come.
    outstanding: Option<u64>,
    pub result: SeatResult,
}

impl BotDriver {
    pub fn new(
        id: BotId,
        seat: PlayerId,
        name: String,
        connected: bool,
        deadline: Duration,
    ) -> BotDriver {
        BotDriver {
            id,
            seat,
            name,
            connected,
            ready: false,
            forfeit: false,
            waiting_since: Instant::now(),
            rehello: false,
            deadline,
            in_hand: None,
            awake_at: 0,
            sent: VecDeque::new(),
            answered: VecDeque::new(),
            answered_count: 0,
            outstanding: None,
            result: SeatResult::default(),
        }
    }

    /// In the game and able to hear it.
    pub fn playing(&self) -> bool {
        self.connected && self.ready && !self.forfeit
    }

    /// Whether to send tick `t` now.
    pub fn turn(&mut self, t: u64, now: Instant) -> Turn {
        if !self.playing() {
            return Turn::Skip;
        }
        // A bot that never answers at all is let off the hook after a
        // while, so one swallowed reply does not silence it for good.
        if let Some(waiting) = self.outstanding
            && self
                .sent
                .iter()
                .find(|(tick, _)| *tick == waiting)
                .is_none_or(|(_, at)| now.duration_since(*at) > (self.deadline * 8).max(GIVE_UP))
        {
            self.outstanding = None;
        }
        if self.outstanding.is_some() {
            Turn::Busy
        } else if t < self.awake_at {
            Turn::Skip
        } else {
            Turn::Send
        }
    }

    /// Tick `t` went out at `at`.
    pub fn sent(&mut self, t: u64, at: Instant) {
        self.outstanding = Some(t);
        self.sent.push_back((t, at));
        if self.sent.len() > RECENT {
            self.sent.pop_front();
        }
        self.result.sent += 1;
    }

    /// Take a message for this seat while tick `t` is the board's. True
    /// when it is the answer the tick was waiting for.
    pub fn take(&mut self, msg: GameMsg, t: u64, journal: &mut Journal) -> bool {
        let (game, seat, name) = (journal.game, usize::from(self.seat) + 1, &self.name);
        match msg {
            GameMsg::Reply { reply, at, .. } => self.reply(reply, at, t, journal),
            GameMsg::Ready { .. } => {
                self.ready = !self.forfeit;
                false
            }
            GameMsg::Garbled { why, .. } => {
                let _ = writeln!(
                    journal.log,
                    "g{game} t{t} P{seat} {name}: garbled, read as none: {why}"
                );
                // Whatever it was, the bot said something: it is not stuck.
                self.outstanding.take().is_some()
            }
            GameMsg::Dropped { .. } => {
                self.connected = false;
                (journal.console)(format!("  {name} dropped at tick {t}; its seat idles"));
                let _ = writeln!(
                    journal.log,
                    "g{game} t{t} P{seat} {name}: connection dropped"
                );
                self.outstanding.take().is_some()
            }
            GameMsg::Back { .. } => {
                self.connected = true;
                self.rehello = true;
                self.ready = !self.forfeit;
                self.awake_at = 0;
                self.outstanding = None;
                (journal.console)(format!("  {name} is back at tick {t}"));
                let _ = writeln!(journal.log, "g{game} t{t} P{seat} {name}: reconnected");
                false
            }
        }
    }

    fn reply(&mut self, reply: Reply, at: Instant, t: u64, journal: &mut Journal) -> bool {
        // A seat out of the game (forfeited, or never ready) plays nothing,
        // whatever its bot goes on sending.
        if !self.playing() {
            return false;
        }
        if reply.tick > t {
            let _ = writeln!(
                journal.log,
                "g{} t{t} P{} {}: a reply for tick {}, not sent yet, dropped",
                journal.game,
                usize::from(self.seat) + 1,
                self.name,
                reply.tick
            );
            return false;
        }
        let unblocked = self
            .outstanding
            .is_some_and(|waiting| reply.tick >= waiting);
        if unblocked {
            self.outstanding = None;
        }
        if let Some(&(_, sent)) = self.sent.iter().find(|(tick, _)| *tick == reply.tick)
            && !self.answered.contains(&reply.tick)
        {
            self.answered.push_back(reply.tick);
            self.answered_count += 1;
            if self.answered.len() > RECENT {
                self.answered.pop_front();
            }
            let took = at.duration_since(sent);
            self.result.reply_ms.push(took.as_secs_f64() * 1000.0);
            if took > self.deadline {
                self.result.late += 1;
            }
        }
        // The newest reply in hand wins; one answering an older tick than
        // the one already held is stale.
        if self
            .in_hand
            .as_ref()
            .is_none_or(|(held, _)| held.tick < reply.tick)
        {
            // Its `wait` holds from the moment it is read: a reply taken
            // between ticks asks not to be sent the very next one, which is
            // decided before anything commits.
            self.awake_at = reply.tick + 1 + u64::from(reply.wait);
            self.in_hand = Some((reply, at));
        }
        unblocked
    }

    /// What the bot asks this seat to do at the commit: its newest reply,
    /// logged, or nothing.
    pub fn order(&mut self, journal: &mut Journal) -> Act {
        let Some((reply, at)) = self.in_hand.take() else {
            return Act::None;
        };
        self.log(&reply, at, journal);
        reply.act
    }

    fn log(&self, reply: &Reply, at: Instant, journal: &mut Journal) {
        if journal.echo_notes
            && let Some(note) = &reply.note
        {
            (journal.console)(format!(
                "  t{} {}: {} - {note}",
                reply.tick,
                self.name,
                reply.act.describe()
            ));
        }
        if reply.act == Act::None
            && reply.note.is_none()
            && reply.garbled.is_none()
            && reply.wait == 0
        {
            return;
        }
        let late = self
            .sent
            .iter()
            .find(|(tick, _)| *tick == reply.tick)
            .is_some_and(|(_, sent)| at.duration_since(*sent) > self.deadline);
        let mut line = format!(
            "g{} t{} P{} {}: {}",
            journal.game,
            reply.tick,
            usize::from(self.seat) + 1,
            self.name,
            reply.act.describe()
        );
        if reply.wait > 0 {
            line.push_str(&format!(" wait {}", reply.wait));
        }
        if late {
            line.push_str(" [late]");
        }
        if let Some(why) = &reply.garbled {
            line.push_str(&format!(" [garbled: {why}]"));
        }
        if let Some(note) = &reply.note {
            line.push_str(&format!("  note: {}", note.replace('\n', " / ")));
        }
        let _ = writeln!(journal.log, "{line}");
    }

    /// The game is over: what was sent and never answered is missed.
    pub fn close(&mut self) {
        self.result.missed = self.result.sent.saturating_sub(self.answered_count);
        self.result.forfeit = self.forfeit;
    }
}

/// What the sim would say to `action` from `seat`, asked before the tick.
pub fn refusal(board: &Board, seat: PlayerId, action: PlayerAction) -> Option<Refusal> {
    match action {
        PlayerAction::Place { x, y, .. } => board.placement_refusal(seat, x, y),
        PlayerAction::Remove { x, y } => board.removal_refusal(seat, x, y),
        PlayerAction::None | PlayerAction::CallEvent(_) => None,
    }
}

/// What became of `act` once the tick ran: `refused` is what the board
/// said before it, `after` the board the tick left. `None` for an act with
/// nothing to report (`none`, `move`).
///
/// The board the tick left has the last word. Seats act one at a time in
/// the tick's order, so a tile taken before the tick can be given up by a
/// seat ahead in the order and then placed on, and a free one taken by a
/// seat ahead: either way, a post of this seat's stamped this tick is a
/// placement that landed, and no such post is one that did not.
pub fn outcome(
    act: Act,
    action: PlayerAction,
    refused: Option<Refusal>,
    after: &Board,
    seat: PlayerId,
    tick: u64,
) -> Option<Outcome> {
    if matches!(act, Act::None | Act::Move { .. }) {
        return None;
    }
    if let PlayerAction::Place { x, y, .. } = action
        && after
            .signpost_at(x, y)
            .is_some_and(|post| post.owner == seat && post.placed == tick)
    {
        return Some(Outcome::Accepted);
    }
    Some(match (refused, action) {
        (Some(why), _) => Outcome::Refused(why),
        // Clear with nothing to clear: nothing was asked of the sim.
        (None, PlayerAction::None) => Outcome::Refused(Refusal::NoPost),
        (None, PlayerAction::Place { x, y, .. }) => match after.signpost_at(x, y) {
            // Lost the tile to a seat the tick's order reached first.
            Some(post) if post.owner != seat && post.placed == tick => {
                Outcome::Refused(Refusal::RivalPost)
            }
            _ => Outcome::Accepted,
        },
        (None, PlayerAction::Remove { .. } | PlayerAction::CallEvent(_)) => Outcome::Accepted,
    })
}

/// The act a game-AI action is, for its cursor and the trace.
pub fn act_of(action: PlayerAction) -> Option<Act> {
    match action {
        PlayerAction::None | PlayerAction::CallEvent(_) => None,
        PlayerAction::Place { x, y, dir } => Some(Act::Place { x, y, dir }),
        PlayerAction::Remove { x, y } => Some(Act::Remove { x, y }),
    }
}

/// Placings, 1 for first, by score. Seats that tie share the place; a
/// forfeit is last, whatever it banked, with the others placed among
/// themselves.
pub fn placings(scores: &[u32], forfeits: &[bool]) -> Vec<u32> {
    let playing: Vec<u32> = scores
        .iter()
        .zip(forfeits)
        .filter(|(_, f)| !**f)
        .map(|(s, _)| *s)
        .collect();
    scores
        .iter()
        .zip(forfeits)
        .map(|(&score, &forfeit)| {
            if forfeit {
                playing.len() as u32 + 1
            } else {
                1 + playing.iter().filter(|&&other| other > score).count() as u32
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ties_share_a_place_and_a_forfeit_comes_last() {
        assert_eq!(placings(&[12, 3, 20, 7], &[false; 4]), vec![2, 4, 1, 3]);
        assert_eq!(placings(&[5, 5, 1], &[false; 3]), vec![1, 1, 3]);
        assert_eq!(placings(&[50, 3, 20], &[true, false, false]), vec![3, 2, 1]);
    }

    fn reply(tick: u64, act: Act) -> GameMsg {
        GameMsg::Reply {
            seat: 0,
            reply: Reply {
                game: 1,
                tick,
                act,
                wait: 0,
                note: None,
                garbled: None,
            },
            at: Instant::now(),
        }
    }

    #[test]
    fn the_newest_reply_wins_and_a_busy_bot_is_not_sent_more() {
        let mut log = Vec::new();
        let mut console = |_: String| {};
        let mut journal = Journal {
            log: &mut log,
            console: &mut console,
            echo_notes: false,
            game: 1,
        };
        let mut seat = BotDriver::new(0, 0, "b".into(), true, Duration::from_millis(33));
        seat.ready = true;
        let now = Instant::now();
        assert_eq!(seat.turn(0, now), Turn::Send);
        seat.sent(0, now);
        assert_eq!(seat.turn(1, now), Turn::Busy, "still answering tick 0");
        let place = Act::Place {
            x: 1,
            y: 1,
            dir: crate::sim::Direction::Up,
        };
        assert!(seat.take(reply(0, place), 1, &mut journal));
        assert_eq!(seat.turn(1, now), Turn::Send);
        seat.sent(1, now);
        // A stale answer to tick 0 after one to tick 1 is dropped.
        assert!(seat.take(reply(1, Act::Clear), 1, &mut journal));
        assert!(!seat.take(reply(0, Act::None), 1, &mut journal));
        assert_eq!(seat.order(&mut journal), Act::Clear);
        assert_eq!(seat.order(&mut journal), Act::None, "taken once");
        // An answer for a tick never sent is not believed.
        assert!(!seat.take(reply(9, place), 2, &mut journal));
        assert_eq!(seat.order(&mut journal), Act::None);
    }

    fn quiet() -> (Vec<u8>, impl FnMut(String)) {
        (Vec::new(), |_: String| {})
    }

    /// A `wait` read between ticks is a wait for the very next one: the
    /// seat is not sent the tick it asked to sleep through.
    #[test]
    fn a_wait_read_between_ticks_skips_the_next_one() {
        let (mut log, mut console) = quiet();
        let mut journal = Journal {
            log: &mut log,
            console: &mut console,
            echo_notes: false,
            game: 1,
        };
        let mut seat = BotDriver::new(0, 0, "b".into(), true, Duration::from_millis(33));
        seat.ready = true;
        let now = Instant::now();
        seat.sent(0, now);
        let GameMsg::Reply {
            mut reply, seat: s, ..
        } = reply(0, Act::None)
        else {
            unreachable!()
        };
        reply.wait = 5;
        // Read in the drain at the top of tick 1, before anything commits.
        seat.take(
            GameMsg::Reply {
                seat: s,
                reply,
                at: now,
            },
            1,
            &mut journal,
        );
        assert_eq!(seat.turn(1, now), Turn::Skip, "it asked to sleep");
        assert_eq!(seat.turn(6, now), Turn::Send, "and wakes when it said");
    }

    /// A seat that forfeited plays nothing, whatever its bot goes on
    /// sending.
    #[test]
    fn a_forfeited_seat_does_nothing_its_bot_says() {
        let (mut log, mut console) = quiet();
        let mut journal = Journal {
            log: &mut log,
            console: &mut console,
            echo_notes: false,
            game: 1,
        };
        let mut seat = BotDriver::new(0, 0, "b".into(), true, Duration::from_millis(33));
        seat.forfeit = true;
        let place = Act::Place {
            x: 1,
            y: 1,
            dir: crate::sim::Direction::Up,
        };
        seat.take(reply(0, place), 3, &mut journal);
        assert_eq!(seat.order(&mut journal), Act::None);
    }

    /// Seats act one at a time, so a tile one seat gives up is free for
    /// the next in the tick's order: a placement that took it was
    /// accepted, whatever the board said before the tick.
    #[test]
    fn a_tile_freed_earlier_in_the_tick_is_an_accepted_placement() {
        use crate::sim::{Direction, TileKind};
        let mut board = Board::new(6, 3, 1);
        board.set_tile(0, 1, TileKind::Castle(0));
        board.set_tile(5, 1, TileKind::Castle(1));
        board.set_signpost_rule(3, crate::sim::CapPolicy::Evict);
        assert!(board.place_signpost(0, 2, 1, Direction::Up));
        // Tick 0 leads with seat 0.
        let mut actions = [PlayerAction::None; crate::sim::MAX_PLAYERS];
        actions[0] = PlayerAction::Remove { x: 2, y: 1 };
        actions[1] = PlayerAction::Place {
            x: 2,
            y: 1,
            dir: Direction::Down,
        };
        let tick = board.ticks();
        let refused = refusal(&board, 1, actions[1]);
        assert_eq!(
            refused,
            Some(Refusal::RivalPost),
            "before the tick, it is taken"
        );
        board.tick(&actions);
        let post = board.signpost_at(2, 1).expect("seat 1's post");
        assert_eq!(post.owner, 1, "seat 0 went first and let it go");
        let act = Act::Place {
            x: 2,
            y: 1,
            dir: Direction::Down,
        };
        assert_eq!(
            outcome(act, actions[1], refused, &board, 1, tick),
            Some(Outcome::Accepted)
        );
    }
}
