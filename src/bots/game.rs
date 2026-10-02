//! One game with bots in it, from `hello` to `end`, for the arena and cups.
//!
//! The sim is never told who sits where: it takes one action per seat per
//! tick, and this file fills those slots from the seat controllers, the
//! game's AI and bots over the protocol. Everything here is the seat layer:
//! the clock, the fair cursor, the log, the trace and the standings'
//! numbers. The board ticks exactly as it would anywhere else, and the
//! replay records the actions actually committed, so a replay reproduces
//! the game whatever the bots did.

use super::cursor::SeatCursor;
use super::diff::{Before, Happened};
use super::listener::{BotId, GameLink, GameMsg, Listener, View, lock};
use super::lookahead;
use super::protocol::{self, Act, Clock, Outcome, SeatKind, Table, You};
pub use super::seat::SeatResult;
use super::seat::{BotDriver, Journal, Turn, act_of, outcome, placings, refusal};
use crate::sim::{
    Board, BotLevel, Hand, Level, MAX_PLAYERS, PlayerAction, PlayerId, Replay, bot_action_with,
};
use serde_json::{Value, json};
use std::io::Write;
use std::sync::mpsc::{RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// Who decides for a seat.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Seat {
    /// The game's AI at a difficulty: a pure function of the board.
    Ai(BotLevel),
    /// A bot connected over the protocol.
    Bot(BotId),
}

impl Seat {
    pub fn kind(self) -> SeatKind {
        match self {
            Seat::Ai(_) => SeatKind::Ai,
            Seat::Bot(_) => SeatKind::Bot,
        }
    }
}

/// The name the game's AI goes by at a level.
pub fn ai_name(level: BotLevel) -> &'static str {
    match level {
        BotLevel::Easy => "Easy AI",
        BotLevel::Normal => "Normal AI",
        BotLevel::Hard => "Hard AI",
    }
}

/// One game, before it is played.
pub struct GameSpec {
    pub game: u32,
    /// What a bot asks for to fetch this game's replay (`t1-g17`).
    pub replay_id: String,
    pub board: Board,
    pub seats: Vec<Seat>,
    pub names: Vec<String>,
    pub clock: Clock,
    /// The fair cursor rule: every seat's hand walks at a person's pace.
    pub fair_cursor: bool,
    /// How long a bot that is not connected when the game starts is waited
    /// for before its seat is played idle and scored as a forfeit.
    pub forfeit_after: Duration,
    /// How long a bot has to answer `hello` with `ready`.
    pub ready_within: Duration,
}

pub struct GameResult {
    pub seats: Vec<SeatResult>,
    pub replay: Replay,
    pub ticks: u64,
    pub elapsed: Duration,
}

/// What a game writes as it goes, besides its result.
pub struct Sinks<'a> {
    /// The decision log: every decision a bot made, with its tick, its
    /// note, and whether it was refused or late.
    pub log: &'a mut dyn Write,
    /// `--trace`: every tick message and the actions that followed it.
    pub trace: Option<&'a mut dyn Write>,
    /// `--watch`: the game, tick by tick, to the window drawing it.
    pub feed: Option<Sender<Feed>>,
    /// Lines worth printing on the console as they happen (a bot dropped,
    /// a bot forfeited).
    pub console: &'a mut dyn FnMut(String),
    /// Print every bot's notes on the console too: with `--watch` the
    /// author reads them beside the window, as the move happens.
    pub echo_notes: bool,
}

/// A game going out to a window as it is played.
pub enum Feed {
    Start(Box<Replay>),
    Tick([PlayerAction; MAX_PLAYERS]),
}

/// A game in play.
struct Match<'a, 's> {
    listener: &'a Listener,
    link: GameLink,
    spec: GameSpec,
    kinds: Vec<SeatKind>,
    sinks: &'a mut Sinks<'s>,
    /// One per seat; `None` for the game's AI.
    bots: Vec<Option<BotDriver>>,
    /// The AI's numbers (the bots keep their own).
    ai: Vec<SeatResult>,
    cursors: Vec<SeatCursor>,
    last: Vec<Option<(&'static str, Outcome)>>,
    replay: Replay,
}

impl Match<'_, '_> {
    fn n(&self) -> usize {
        self.spec.seats.len()
    }

    fn table(&self) -> Table<'_> {
        Table {
            game: self.spec.game,
            names: &self.spec.names,
            kinds: &self.kinds,
            clock: self.spec.clock,
            cursor: self.spec.fair_cursor,
        }
    }

    fn hello(&self, seat: PlayerId, resumed: bool) -> Value {
        protocol::hello(&self.spec.board, &self.table(), seat, resumed)
    }

    /// Hand one message to the seat it is for.
    fn take(&mut self, msg: GameMsg, t: u64) -> bool {
        let seat = match &msg {
            GameMsg::Reply { seat, .. }
            | GameMsg::Ready { seat }
            | GameMsg::Garbled { seat, .. }
            | GameMsg::Dropped { seat }
            | GameMsg::Back { seat } => usize::from(*seat),
        };
        let Some(Some(bot)) = self.bots.get_mut(seat) else {
            return false;
        };
        let mut journal = Journal {
            log: &mut *self.sinks.log,
            console: &mut *self.sinks.console,
            echo_notes: self.sinks.echo_notes,
            game: self.spec.game,
        };
        bot.take(msg, t, &mut journal)
    }

    /// `hello` to every bot, then wait until each is ready or forfeits.
    fn greet(&mut self) {
        for seat in 0..self.n() {
            if let Some(bot) = &self.bots[seat]
                && bot.connected
            {
                self.listener
                    .send(bot.id, &self.hello(seat as PlayerId, false));
            }
        }
        loop {
            let now = Instant::now();
            let mut waiting = false;
            for bot in self.bots.iter_mut().flatten() {
                if bot.ready || bot.forfeit {
                    continue;
                }
                let limit = if bot.connected {
                    self.spec.ready_within
                } else {
                    self.spec.forfeit_after
                };
                if now.duration_since(bot.waiting_since) < limit {
                    waiting = true;
                    continue;
                }
                bot.forfeit = true;
                let why = if bot.connected {
                    "never said ready"
                } else {
                    "never came"
                };
                let game = self.spec.game;
                (self.sinks.console)(format!("  {} forfeits game {game}: {why}", bot.name));
                let _ = writeln!(
                    self.sinks.log,
                    "g{game} P{} {}: forfeit, {why}",
                    bot.seat + 1,
                    bot.name
                );
            }
            if !waiting {
                return;
            }
            let msg = match self.link.recv_timeout(Duration::from_millis(50)) {
                Ok(msg) => msg,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            // Before the first tick a bot that comes back is greeted afresh
            // and given its ten seconds from now.
            if let GameMsg::Back { seat } = msg
                && let Some(Some(bot)) = self.bots.get_mut(usize::from(seat))
                && !bot.forfeit
            {
                bot.connected = true;
                bot.waiting_since = Instant::now();
                let id = bot.id;
                self.listener.send(id, &self.hello(seat, false));
                continue;
            }
            if let GameMsg::Dropped { seat } = msg
                && let Some(Some(bot)) = self.bots.get_mut(usize::from(seat))
            {
                bot.connected = false;
                bot.ready = false;
                continue;
            }
            self.take(msg, 0);
        }
    }

    /// Send tick `t` to every bot awake for it. Returns how many answers
    /// the tick waits for.
    fn send_tick(&mut self, t: u64, cursors: &[Option<(u8, u8)>]) -> usize {
        let now = Instant::now();
        let mut expecting = 0;
        for seat in 0..self.n() {
            let Some(bot) = &mut self.bots[seat] else {
                continue;
            };
            let id = bot.id;
            let rehello = std::mem::take(&mut bot.rehello) && bot.playing();
            let turn = bot.turn(t, now);
            if rehello {
                self.listener.send(id, &self.hello(seat as PlayerId, true));
            }
            match turn {
                Turn::Skip => {}
                Turn::Busy => expecting += 1,
                Turn::Send => {
                    let you = You {
                        seat: seat as PlayerId,
                        last: self.last[seat],
                    };
                    let msg = protocol::tick(&self.spec.board, self.spec.game, &you, cursors);
                    if self.listener.send(id, &msg)
                        && let Some(bot) = &mut self.bots[seat]
                    {
                        bot.sent(t, now);
                        expecting += 1;
                    }
                }
            }
        }
        expecting
    }

    /// Wait for this tick's answers: live, until the wall clock's next
    /// tick; fast-forward, until every bot has answered or the deadline
    /// has passed, whichever is first.
    fn gather(&mut self, t: u64, until: Instant, mut expecting: usize) {
        let live = self.spec.clock.live;
        loop {
            if !live && expecting == 0 {
                return;
            }
            let now = Instant::now();
            if now >= until {
                return;
            }
            match self.link.recv_timeout(until - now) {
                Ok(msg) => {
                    if self.take(msg, t) {
                        expecting = expecting.saturating_sub(1);
                    }
                }
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Every seat's action for this tick, and the act each one carried out.
    fn commit(&mut self) -> ([PlayerAction; MAX_PLAYERS], Vec<Option<Act>>) {
        let mut actions = [PlayerAction::None; MAX_PLAYERS];
        let mut done = vec![None; self.n()];
        let board = &self.spec.board;
        for seat in 0..self.n() {
            let player = seat as PlayerId;
            let act = match (&mut self.bots[seat], self.spec.seats[seat]) {
                (Some(bot), _) => {
                    let mut journal = Journal {
                        log: &mut *self.sinks.log,
                        console: &mut *self.sinks.console,
                        echo_notes: self.sinks.echo_notes,
                        game: self.spec.game,
                    };
                    bot.order(&mut journal)
                }
                (None, Seat::Ai(level)) => {
                    let hand = if self.spec.fair_cursor {
                        Hand::Fair
                    } else {
                        Hand::Instant
                    };
                    let action = bot_action_with(board, player, level, hand);
                    actions[seat] = action;
                    if let Some(act) = act_of(action) {
                        self.cursors[seat].at = act.target().unwrap_or(self.cursors[seat].at);
                        done[seat] = Some(act);
                    }
                    continue;
                }
                (None, Seat::Bot(_)) => Act::None,
            };
            let (action, landed) =
                self.cursors[seat].step(board, player, act, self.spec.fair_cursor);
            actions[seat] = action;
            done[seat] = landed;
        }
        (actions, done)
    }

    fn result_mut(&mut self, seat: usize) -> &mut SeatResult {
        match &mut self.bots[seat] {
            Some(bot) => &mut bot.result,
            None => &mut self.ai[seat],
        }
    }

    /// One tick of the round.
    fn tick(&mut self, wall: &mut Instant) {
        let t = self.spec.board.ticks();
        let n = self.n();
        let cursors: Vec<Option<(u8, u8)>> = self.cursors.iter().map(|c| Some(c.at)).collect();
        if self.bots.iter().any(Option::is_some) {
            let cap = if self.spec.clock.live {
                lookahead::CAP_LIVE
            } else {
                lookahead::CAP_FAST
            };
            *lock(&self.link.view) = Some(View {
                board: self.spec.board.clone(),
                cursors: cursors.clone(),
                budget: vec![cap; n],
            });
        }
        // Whatever arrived since the last tick (a late reply, a bot coming
        // back) is taken first: it decides who is sent this one.
        while let Some(msg) = self.link.try_recv() {
            self.take(msg, t);
        }
        let sent_at = Instant::now();
        let expecting = self.send_tick(t, &cursors);
        let period = Duration::from_secs_f64(1.0 / f64::from(crate::sim::TICKS_PER_SECOND));
        let until = if self.spec.clock.live {
            *wall += period;
            *wall
        } else {
            sent_at + Duration::from_millis(u64::from(self.spec.clock.deadline_ms))
        };
        self.gather(t, until, expecting);
        // A live table that fell behind catches up rather than drifting.
        if self.spec.clock.live && Instant::now() > *wall + period * 4 {
            *wall = Instant::now();
        }
        let (actions, done) = self.commit();
        let refusals: Vec<_> = (0..n)
            .map(|seat| refusal(&self.spec.board, seat as PlayerId, actions[seat]))
            .collect();
        if let Some(trace) = self.sinks.trace.as_deref_mut() {
            let you = You {
                seat: 0,
                last: None,
            };
            let acts: Vec<Value> = done
                .iter()
                .map(|d| d.unwrap_or(Act::None).to_json())
                .collect();
            let line = json!({
                "state": protocol::tick(&self.spec.board, self.spec.game, &you, &cursors),
                "actions": acts,
            });
            let _ = writeln!(trace, "{line}");
        }
        let before = Before::of(&self.spec.board);
        self.spec.board.tick(&actions);
        self.replay.record(actions);
        if let Some(feed) = &self.sinks.feed {
            let _ = feed.send(Feed::Tick(actions));
        }
        for seat in 0..n {
            let Some(act) = done[seat] else {
                continue;
            };
            let Some(outcome) = outcome(
                act,
                actions[seat],
                refusals[seat],
                &self.spec.board,
                seat as PlayerId,
                t,
            ) else {
                continue;
            };
            self.last[seat] = Some((act.token(), outcome));
            if let Outcome::Refused(why) = outcome {
                self.result_mut(seat).rejected += 1;
                if let Some(bot) = &self.bots[seat] {
                    let _ = writeln!(
                        self.sinks.log,
                        "g{} t{t} P{} {}: {} refused: {}",
                        self.spec.game,
                        seat + 1,
                        bot.name,
                        act.describe(),
                        why.token()
                    );
                }
            }
        }
        let mut happened = Vec::new();
        before.diff(&self.spec.board, &mut happened);
        for what in happened {
            match what {
                Happened::Banked { owner, .. } if usize::from(owner) < n => {
                    self.result_mut(usize::from(owner)).banked += 1;
                }
                Happened::Raided { owner, .. } if usize::from(owner) < n => {
                    self.result_mut(usize::from(owner)).raided += 1;
                }
                Happened::Banked { .. }
                | Happened::Raided { .. }
                | Happened::Eaten { .. }
                | Happened::PostWorn { .. }
                | Happened::PostGone { .. } => {}
            }
        }
    }

    /// Tell every bot how it went, and hand back the numbers.
    fn finish(mut self, started: Instant) -> GameResult {
        *lock(&self.link.view) = None;
        // Replies still in flight for the last ticks are drained so their
        // timing counts.
        let t = self.spec.board.ticks();
        while let Some(msg) = self.link.try_recv() {
            self.take(msg, t);
        }
        let n = self.n();
        let forfeits: Vec<bool> = (0..n)
            .map(|seat| self.bots[seat].as_ref().is_some_and(|b| b.forfeit))
            .collect();
        let scores: Vec<u32> = self.spec.board.scores()[..n].to_vec();
        let places = placings(&scores, &forfeits);
        // Kept before anybody is told the game is over, so a bot that asks
        // for the replay the moment it hears `end` finds it there.
        let players = self.bots.iter().flatten().map(|b| b.id).collect();
        self.listener
            .keep_replay(self.spec.replay_id.clone(), players, self.replay.to_text());
        for seat in 0..n {
            if let Some(bot) = &mut self.bots[seat] {
                bot.close();
                self.listener.send(
                    bot.id,
                    &json!({
                        "type": "end", "game": self.spec.game, "scores": scores,
                        "placing": places[seat], "replay": self.spec.replay_id,
                    }),
                );
            }
            let result = self.result_mut(seat);
            result.score = scores[seat];
            result.placing = places[seat];
        }
        let Match {
            link,
            spec,
            bots,
            ai,
            replay,
            ..
        } = self;
        drop(link);
        GameResult {
            seats: bots
                .into_iter()
                .zip(ai)
                .map(|(bot, ai)| bot.map_or(ai, |b| b.result))
                .collect(),
            replay,
            ticks: spec.board.ticks(),
            elapsed: started.elapsed(),
        }
    }
}

/// Play `spec` to the tide, with the listener carrying the bots' messages.
pub fn play(listener: &Listener, spec: GameSpec, sinks: &mut Sinks) -> GameResult {
    let n = spec.seats.len();
    let kinds: Vec<SeatKind> = spec.seats.iter().map(|s| s.kind()).collect();
    let mut replay = Replay::new(Level::from_board("Arena", 3, spec.board.clone()));
    for (i, name) in spec.names.iter().enumerate().take(MAX_PLAYERS) {
        replay.names[i] = name.clone();
        replay.kinds[i] = kinds[i];
    }
    if let Some(feed) = &sinks.feed {
        let _ = feed.send(Feed::Start(Box::new(replay.clone())));
    }
    let deadline = Duration::from_millis(u64::from(spec.clock.deadline_ms));
    let bots: Vec<Option<BotDriver>> = spec
        .seats
        .iter()
        .enumerate()
        .map(|(seat, s)| match s {
            Seat::Bot(id) => Some(BotDriver::new(
                *id,
                seat as PlayerId,
                spec.names[seat].clone(),
                listener.connected(*id),
                deadline,
            )),
            Seat::Ai(_) => None,
        })
        .collect();
    let routes = bots.iter().flatten().map(|b| (b.id, b.seat)).collect();
    let link = listener.open_game(spec.game, routes);
    let cursors = (0..n)
        .map(|s| SeatCursor::home(&spec.board, s as PlayerId))
        .collect();
    let mut game = Match {
        listener,
        link,
        spec,
        kinds,
        sinks,
        bots,
        ai: vec![SeatResult::default(); n],
        cursors,
        last: vec![None; n],
        replay,
    };
    game.greet();
    let started = Instant::now();
    let mut wall = Instant::now();
    while !game.spec.board.round_over() {
        game.tick(&mut wall);
    }
    game.finish(started)
}
