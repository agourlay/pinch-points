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

use crate::app::i18n::fill;
use crate::app::{Controllers, SeatController, SeatKinds, SeatNames, Sim};
use crate::bots::connstr::{ConnString, DEFAULT_PORT, Key};
use crate::bots::cursor::SeatCursor;
use crate::bots::listener::{Admission, BotId, Config, Event, GameLink, Invite, Listener, View};
use crate::bots::lookahead;
use crate::bots::protocol::{self, Act, Clock, Outcome, SeatKind, Table, You};
use crate::bots::seat::{BotDriver, Journal, Turn, act_of, outcome, placings, refusal};
use crate::sim::{Board, BotLevel, MAX_PLAYERS, PlayerAction, Refusal, Replay, bot_action};
use bevy::prelude::*;
use std::net::SocketAddr;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// What a bot that came in but was dealt no seat is told as it goes.
const NO_SEAT: &str = "the table filled up before this bot had a seat";

/// How long a dropped bot's seat idles before the game's AI stands in.
pub const GRACE: Duration = Duration::from_secs(5);

/// The level the game's AI plays a bot's seat at while the bot is away,
/// and the seat's level in a round copied with a bot in it.
pub const STAND_IN: BotLevel = BotLevel::Normal;

/// The one deadline a bot at a table with people gets: a tick.
const LIVE_DEADLINE_MS: u32 = 1000 / crate::sim::TICKS_PER_SECOND;

/// One seat waiting for a bot, or held by one.
pub struct Chair {
    /// Which chair: a number of its own, so a chair keeps its key and its
    /// bot while others come and go around it.
    id: u32,
    pub seat: u8,
    pub key: Key,
    pub bot: Option<BotId>,
}

/// A listener on this machine for bots to register with, a chair per seat
/// that wants one.
pub struct Doorway {
    listener: Listener,
    /// Behind a lock so the doorway can live in a resource.
    events: std::sync::Mutex<Receiver<Event>>,
    /// Where it listens for this machine, and for the LAN once opened to it.
    here: SocketAddr,
    lan: Option<SocketAddr>,
    /// Whether the strings shown are the LAN's.
    pub showing_lan: bool,
    pub chairs: Vec<Chair>,
    next_chair: u32,
    /// Each chair is a seat's (a couch's bot seats, a bot joining a
    /// party), rather than the next of however many come (a host's door):
    /// a bot that goes leaves its seat wanting a bot, with a fresh key.
    seats_fixed: bool,
}

impl Drop for Doorway {
    fn drop(&mut self) {
        self.listener.close();
    }
}

impl Doorway {
    /// A doorway on this machine: the bots' own port if it is free, any
    /// port if not (a second game on one machine, an arena running).
    pub fn open() -> std::io::Result<Doorway> {
        let config = Config::new(Admission::Keys(Vec::new()));
        let (listener, events) = Listener::bind(
            SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)),
            config.clone(),
        )
        .or_else(|_| Listener::bind(SocketAddr::from(([127, 0, 0, 1], 0)), config))?;
        let here = listener.local_addr();
        Ok(Doorway {
            listener,
            events: std::sync::Mutex::new(events),
            here,
            lan: None,
            showing_lan: false,
            chairs: Vec::new(),
            next_chair: 0,
            seats_fixed: false,
        })
    }

    /// A doorway on the LAN, for bots straight to a host (route 2): the
    /// bots' own port on every interface if it is free, any port if not.
    /// The key is the lock here, a single-use one per chair.
    pub fn open_lan() -> std::io::Result<Doorway> {
        let config = Config::new(Admission::Keys(Vec::new()));
        let (listener, events) = Listener::bind(
            SocketAddr::from(([0, 0, 0, 0], DEFAULT_PORT)),
            config.clone(),
        )
        .or_else(|_| Listener::bind(SocketAddr::from(([0, 0, 0, 0], 0)), config))?;
        let here = listener.local_addr();
        Ok(Doorway {
            listener,
            events: std::sync::Mutex::new(events),
            here,
            lan: Some(here),
            showing_lan: true,
            chairs: Vec::new(),
            next_chair: 0,
            seats_fixed: false,
        })
    }

    /// Keep one chair open for the next bot while there is room for it:
    /// once a bot has taken the open one, a fresh key is drawn for the next,
    /// so a key read off the screen is worth nothing once it is spent.
    /// With no room the open chair is withdrawn, key and all, so the
    /// string stops being shown and a bot started with it is refused
    /// rather than registered to a seat that is not there.
    pub fn keep_a_chair_open(&mut self, room: bool) {
        if !room {
            for chair in self.chairs.iter().filter(|chair| chair.bot.is_none()) {
                self.listener.withdraw(chair.id);
            }
            self.chairs.retain(|chair| chair.bot.is_some());
            return;
        }
        let open = self.chairs.iter().any(|chair| chair.bot.is_none());
        if !open {
            let id = self.next_chair;
            self.next_chair += 1;
            let chair = self.keyed_chair(id, self.chairs.len() as u8);
            self.chairs.push(chair);
        }
    }

    /// Chair `id` for `seat`, empty, with a fresh single-use key let in.
    fn keyed_chair(&self, id: u32, seat: u8) -> Chair {
        let key = match (id, crate::app::dev::bot_key()) {
            (0, Some(key)) => key,
            _ => Key::draw(),
        };
        self.listener.invite(Invite {
            key: key.clone(),
            uses: Some(1),
            owner: None,
            slot: Some(id),
        });
        Chair {
            id,
            seat,
            key,
            bot: None,
        }
    }

    /// The string for the chair still waiting for its bot, if one is.
    pub fn open_string(&self) -> Option<String> {
        self.chairs
            .iter()
            .find(|chair| chair.bot.is_none())
            .map(|chair| self.string(chair))
    }

    /// The bots that have come in, in the order they came: each with what
    /// its seat is called, from the name and the owner it declared. The
    /// host cannot check the owner, which the design says out loud.
    pub fn arrived(&self, tr: &crate::app::i18n::Tr) -> Vec<(BotId, String)> {
        self.chairs
            .iter()
            .filter_map(|chair| {
                // Only those still here: one gone is not seated.
                let id = chair.bot.filter(|&id| self.listener.connected(id))?;
                let info = self.listener.bot(id)?;
                let name = match info.owner {
                    Some(owner) => fill(tr.bot_owned, &[("bot", &info.name), ("o", &owner)]),
                    None => fill(tr.bot_unowned, &[("bot", &info.name)]),
                };
                Some((info.id, name))
            })
            .collect()
    }

    /// Ask a bot to go: its chair is given up and its connection closed.
    pub fn let_go(&mut self, id: BotId) {
        self.chairs.retain(|chair| chair.bot != Some(id));
        self.listener.disconnect(id);
    }

    /// A host's round has been dealt, with these of its bots seated
    /// (route 2). Every other bot that came in is told it has no seat and
    /// let go: the people and the bots before it took the chairs, and left
    /// registered it would hear nothing but pings. The open chair is
    /// withdrawn too, since nobody is dealt in until the table is back in
    /// the lobby, which opens one again.
    pub fn seat_the_table(&mut self, seated: &[BotId]) {
        let unseated: Vec<BotId> = self
            .chairs
            .iter()
            .filter_map(|chair| chair.bot)
            .filter(|id| !seated.contains(id))
            .collect();
        for id in unseated {
            self.chairs.retain(|chair| chair.bot != Some(id));
            self.listener.dismiss(id, NO_SEAT);
        }
        self.keep_a_chair_open(false);
    }

    /// Chairs for exactly these seats: a chair already holding a bot for a
    /// seat still wanting one keeps it, and a seat new to the list gets a
    /// fresh, single-use key.
    pub fn seat_bots(&mut self, seats: &[u8]) {
        self.seats_fixed = true;
        self.chairs.retain(|chair| seats.contains(&chair.seat));
        for &seat in seats {
            if self.chairs.iter().any(|chair| chair.seat == seat) {
                continue;
            }
            let id = self.next_chair;
            self.next_chair += 1;
            let chair = self.keyed_chair(id, seat);
            self.chairs.push(chair);
        }
        self.chairs.sort_by_key(|chair| chair.seat);
    }

    /// Take what the listener heard: each registration sits its bot in the
    /// chair its key was printed for, and a bot that went gives its chair
    /// up, which finishes with it: started again it registers afresh,
    /// under the same name. A seat's chair gets a fresh key for the next
    /// bot; at a host's door the chair goes, and the next bot takes the
    /// one kept open.
    pub fn poll(&mut self) {
        let heard: Vec<Event> = {
            let events = self
                .events
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::iter::from_fn(|| events.try_recv().ok()).collect()
        };
        for event in heard {
            match event {
                Event::Registered(id) => {
                    if let Some(info) = self.listener.bot(id)
                        && let Some(chair) = self
                            .chairs
                            .iter_mut()
                            .find(|c| Some(c.id) == info.slot && c.bot.is_none())
                    {
                        chair.bot = Some(id);
                    }
                }
                // Read late: a bot that has come back since keeps its chair.
                Event::Dropped(id) if !self.listener.connected(id) => {
                    let Some(at) = self.chairs.iter().position(|c| c.bot == Some(id)) else {
                        continue;
                    };
                    self.listener.retire(id);
                    match self.seats_fixed {
                        true => {
                            let (chair, seat) = (self.chairs[at].id, self.chairs[at].seat);
                            self.chairs[at] = self.keyed_chair(chair, seat);
                        }
                        false => {
                            self.chairs.remove(at);
                        }
                    }
                }
                Event::Dropped(_) | Event::Reconnected(_) | Event::Refused(..) => {}
            }
        }
    }

    /// Every chair has its bot.
    pub fn ready(&self) -> bool {
        self.chairs.iter().all(|chair| chair.bot.is_some())
    }

    /// Show the strings on the LAN address rather than this machine's,
    /// listening there too from the first time it is asked for.
    /// Turned back, the LAN socket closes again: a card showing this
    /// machine's strings is not still taking bots from the network.
    pub fn toggle_lan(&mut self) -> std::io::Result<()> {
        match self.lan {
            // A host's doorway listens on the LAN and nowhere else.
            Some(lan) if lan == self.here => {}
            Some(lan) => {
                self.listener.stop_listening(lan);
                self.lan = None;
            }
            None => {
                self.lan = Some(
                    self.listener
                        .also_listen(SocketAddr::from(([0, 0, 0, 0], 0)))?,
                );
            }
        }
        self.showing_lan = self.lan.is_some();
        Ok(())
    }

    /// The string a chair's bot is started with.
    pub fn string(&self, chair: &Chair) -> String {
        let (host, port) = match (self.showing_lan, self.lan) {
            (true, Some(lan)) => (crate::bots::connstr::reachable_host(lan), lan.port()),
            _ => (self.here.ip().to_string(), self.here.port()),
        };
        ConnString::new(host, port, Some(chair.key.clone())).to_string()
    }

    /// What a chair's bot said it is: `Greedy 0.3`, so the author can tell
    /// it is the build they meant to start.
    pub fn bot_line(&self, chair: &Chair) -> Option<(String, Option<Duration>)> {
        let info = self.listener.bot(chair.bot?)?;
        let name = match info.version.is_empty() {
            true => info.name,
            false => format!("{} {}", info.name, info.version),
        };
        Some((name, self.listener.rtt(info.id)))
    }

    /// The seat name for a bot joining a party from this machine (route
    /// 1): its owner is whoever is at this machine, not what it declared.
    pub fn named_for(&self, tr: &crate::app::i18n::Tr, owner: &str) -> Option<String> {
        let info = self.listener.bot(self.chairs.first()?.bot?)?;
        Some(match owner.trim().is_empty() {
            true => fill(tr.bot_unowned, &[("bot", &info.name)]),
            false => fill(tr.bot_owned, &[("bot", &info.name), ("o", owner.trim())]),
        })
    }

    /// What the seat a bot holds is called: its registered name and its
    /// owner's, "Greedy (Ana)", or its name alone if it gave none. The
    /// robot beside it on every screen says it is a bot.
    pub fn seat_name(&self, tr: &crate::app::i18n::Tr, seat: u8) -> Option<String> {
        let chair = self.chairs.iter().find(|chair| chair.seat == seat)?;
        let info = self.listener.bot(chair.bot?)?;
        Some(match info.owner {
            Some(owner) => fill(tr.bot_owned, &[("bot", &info.name), ("o", &owner)]),
            None => fill(tr.bot_unowned, &[("bot", &info.name)]),
        })
    }
}

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
    fn open(
        door: &Doorway,
        board: &Board,
        seats: &[(u8, BotId)],
        names: Vec<String>,
        kinds: Vec<SeatKind>,
        online: bool,
        game: u32,
    ) -> BotRound {
        let deadline = Duration::from_millis(u64::from(LIVE_DEADLINE_MS));
        let drivers: Vec<BotDriver> = seats
            .iter()
            .map(|&(seat, id)| {
                let name = names.get(usize::from(seat)).cloned().unwrap_or_default();
                BotDriver::new(id, seat, name, door.listener.connected(id), deadline)
            })
            .collect();
        let link = door
            .listener
            .open_game(game, seats.iter().map(|&(seat, id)| (id, seat)).collect());
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
            let seat = match &msg {
                crate::bots::listener::GameMsg::Reply { seat, .. }
                | crate::bots::listener::GameMsg::Ready { seat }
                | crate::bots::listener::GameMsg::Garbled { seat, .. }
                | crate::bots::listener::GameMsg::Dropped { seat }
                | crate::bots::listener::GameMsg::Back { seat } => *seat,
            };
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
    fn commit_online(
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

    /// Send the board as it now stands to every bot awake for it, without
    /// any outcome to report first (online, outcomes arrive with frames).
    fn send_next(&mut self, board: &Board) {
        self.after(board, Vec::new());
    }

    /// The round is over: every bot is told how it went, once, however
    /// many steps the tide stays in for. The round's recording is kept
    /// first, so a bot that asks for the replay `end` names the moment it
    /// hears it finds it there; with nothing recorded, `end` names none.
    fn finish(&mut self, board: &Board, door: Option<&Doorway>, replay: Option<&Replay>) {
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
                door.listener
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
        match &self.round {
            Some(round) if round.online => round.drivers.iter().map(|d| d.seat).collect(),
            Some(_) | None => Vec::new(),
        }
    }

    /// Online, every bot seat's act, for the lockstep to commit.
    pub fn commit_online(&mut self, board: &Board) -> Vec<(u8, PlayerAction, Option<Act>)> {
        let BotSeats { round, news, .. } = self;
        match round {
            Some(round) => round.commit_online(board, news),
            None => Vec::new(),
        }
    }

    /// The lockstep took `seat`'s act for `frame`: remember it until the
    /// frame is played, when its outcome is known. One it did not take
    /// (paused, stalled, or the frame given to a tide call) is not passed
    /// here, and goes out again on the next commit.
    pub fn committed(&mut self, frame: u32, seat: u8, act: Option<Act>, action: PlayerAction) {
        if let Some(held) = self
            .round
            .as_mut()
            .and_then(|round| round.unconfirmed.get_mut(usize::from(seat)))
        {
            *held = None;
        }
        if let (Some(round), Some(act)) = (self.round.as_mut(), act) {
            round.in_flight.push_back(InFlight {
                frame,
                seat,
                act,
                action,
                refused: None,
            });
            while round.in_flight.len() > 256 {
                round.in_flight.pop_front();
            }
        }
    }

    /// `frame` is about to be played from `board`: what the sim would say
    /// to each act on it.
    pub fn before_frame(&mut self, frame: u32, board: &Board) {
        let Some(round) = self.round.as_mut() else {
            return;
        };
        for entry in round.in_flight.iter_mut().filter(|e| e.frame == frame) {
            entry.refused = refusal(board, entry.seat, entry.action);
        }
    }

    /// `frame` was played: what became of each act on it.
    pub fn after_frame(&mut self, frame: u32, board: &Board) {
        let Some(round) = self.round.as_mut() else {
            return;
        };
        while let Some(&entry) = round.in_flight.front() {
            if entry.frame > frame {
                break;
            }
            round.in_flight.pop_front();
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
                round.last[usize::from(entry.seat)] = Some((entry.act.token(), result));
            }
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

// --- the doorway card ------------------------------------------------------

/// The doorway card: the one line each bot needs, a way to copy it, and
/// what has connected so far.
#[derive(Component)]
pub struct DoorCard;

/// One line of it, by its place on the card.
#[derive(Component)]
pub struct DoorLine(usize);

/// Lines the card holds: a heading, the instruction, a chair a seat, the
/// feedback, the keys.
const DOOR_LINES: usize = 3 + MAX_PLAYERS + 1;

/// What the card says, line by line, as it stands.
fn door_lines(bots: &BotSeats, tr: &crate::app::i18n::Tr, beach: &str) -> Vec<(String, Color)> {
    let mut lines = vec![(String::new(), Color::NONE); DOOR_LINES];
    let (Some(waiting), Some(door)) = (&bots.waiting, &bots.door) else {
        return lines;
    };
    let title = match waiting {
        Waiting::Couch => tr.door_title_table.to_string(),
        Waiting::Join => fill(tr.door_title_join, &[("b", beach)]),
    };
    lines[0] = (title, crate::app::palette::GOLD);
    lines[1] = (
        tr.door_start_with.to_string(),
        crate::app::palette::PARCHMENT,
    );
    for (i, chair) in door.chairs.iter().take(MAX_PLAYERS).enumerate() {
        let who = crate::app::seat_label(tr, chair.seat);
        let line = match door.bot_line(chair) {
            Some((name, rtt)) => {
                let ms = rtt.map_or("-".to_string(), |d| {
                    format!("{:.1}", d.as_secs_f64() * 1000.0)
                });
                format!(
                    "{who}  {}",
                    fill(tr.door_connected, &[("n", &name), ("ms", &ms)])
                )
            }
            None => format!("{who}  {}   {}", door.string(chair), tr.door_waiting),
        };
        let ink = match chair.bot {
            Some(_) => crate::app::palette::player_color(chair.seat).lighter(0.2),
            None => Color::WHITE,
        };
        lines[2 + i] = (line, ink);
    }
    lines[2 + MAX_PLAYERS] = (
        bots.feedback.clone(),
        crate::app::palette::PARCHMENT.with_alpha(0.8),
    );
    let keys = match door.showing_lan {
        true => tr.door_prompt_lan,
        false => tr.door_prompt,
    };
    lines[DOOR_LINES - 1] = (
        keys.to_string(),
        crate::app::palette::PARCHMENT.with_alpha(0.7),
    );
    lines
}

/// Put the card up while a doorway is waiting, keep its lines current,
/// and take it down when the wait is over.
pub fn door_card(
    mut commands: Commands,
    bots: Res<BotSeats>,
    settings: Res<crate::app::settings::GameSettings>,
    lobby: Res<crate::app::lobby::LobbyState>,
    cards: Query<Entity, With<DoorCard>>,
    mut lines: Query<(&DoorLine, &mut Text, &mut TextColor, &mut Node)>,
) {
    let up = bots.waiting.is_some() && bots.door.is_some();
    if !up {
        for card in &cards {
            commands.entity(card).despawn();
        }
        return;
    }
    if cards.is_empty() {
        commands
            .spawn((
                DoorCard,
                crate::app::menu_ui::centred_overlay(),
                BackgroundColor(Color::BLACK.with_alpha(0.55)),
                GlobalZIndex(40),
            ))
            .with_children(|wrap| {
                wrap.spawn(crate::app::menu_ui::screen_card())
                    .with_children(|card| {
                        for i in 0..DOOR_LINES {
                            card.spawn((
                                DoorLine(i),
                                Text::new(""),
                                if i == 0 {
                                    crate::app::menu_ui::display_font(
                                        crate::app::menu_ui::type_scale::DISPLAY * 0.7,
                                    )
                                } else {
                                    TextFont {
                                        font_size: FontSize::Px(
                                            crate::app::menu_ui::type_scale::BODY,
                                        ),
                                        ..default()
                                    }
                                },
                                TextColor(Color::WHITE),
                                Node {
                                    align_self: AlignSelf::FlexStart,
                                    ..default()
                                },
                            ));
                        }
                    });
            });
        return;
    }
    let beach = lobby.joining_beach().unwrap_or_default();
    let said = door_lines(&bots, settings.tr(), &beach);
    for (line, mut text, mut ink, mut node) in &mut lines {
        let (words, color) = &said[line.0];
        crate::app::menu_ui::set_text(&mut text, words);
        crate::app::menu_ui::set_color(&mut ink, *color);
        crate::app::menu_ui::set_shown(&mut node, !words.is_empty());
    }
}

/// The card's keys: C copies the first string still waiting for its bot,
/// L shows the LAN's strings or this machine's, Esc backs out and gives
/// the keys up.
pub fn door_input(
    keyboard: crate::app::keycaps::Keyboard,
    mut bots: ResMut<BotSeats>,
    mut clipboard: ResMut<Clipboard>,
    settings: Res<crate::app::settings::GameSettings>,
) {
    let crate::app::keycaps::Keyboard { keys, caps } = keyboard;
    if bots.waiting.is_none() {
        return;
    }
    let tr = settings.tr();
    if keys.just_pressed(KeyCode::Escape) {
        bots.waiting = None;
        bots.feedback.clear();
        bots.door = None;
        return;
    }
    let BotSeats {
        door,
        waiting,
        feedback,
        ..
    } = &mut *bots;
    let Some(door) = door else {
        *waiting = None;
        return;
    };
    door.poll();
    if caps.just_pressed(&keys, 'C')
        && let Some(chair) = door.chairs.iter().find(|chair| chair.bot.is_none())
    {
        let string = door.string(chair);
        *feedback = match clipboard.set_text(string.clone()) {
            Ok(()) => fill(tr.door_copied, &[("s", &string)]),
            Err(e) => fill(tr.code_copy_failed, &[("e", &e.to_string())]),
        };
    }
    if caps.just_pressed(&keys, 'L')
        && let Err(e) = door.toggle_lan()
    {
        *feedback = fill(tr.door_could_not_listen, &[("e", &e.to_string())]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};

    /// Register with a chair's string, the way a bot does.
    fn register(string: &str, name: &str, owner: &str) -> std::net::TcpStream {
        let parsed = ConnString::parse(string).expect("a string the card printed");
        let mut stream =
            std::net::TcpStream::connect((parsed.host.as_str(), parsed.port)).expect("connect");
        let key = parsed.key.map(|k| k.to_string()).unwrap_or_default();
        let line = serde_json::json!({
            "type": "register", "protocol": 1, "name": name, "owner": owner, "key": key,
        });
        writeln!(stream, "{line}").expect("write");
        let mut answer = String::new();
        BufReader::new(stream.try_clone().expect("clone"))
            .read_line(&mut answer)
            .expect("read");
        assert!(answer.contains("registered"), "{answer}");
        stream
    }

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
        let here = bots.door.as_ref().expect("open").here;
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

    /// Wait for the doorway to seat a bot in chair `at`.
    fn seated(door: &mut Doorway, at: usize) {
        for _ in 0..200 {
            door.poll();
            if door.chairs.get(at).is_some_and(|chair| chair.bot.is_some()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("no bot sat in chair {at}");
    }

    /// Try to register with a string, and say what came back.
    fn try_register(string: &str, name: &str) -> String {
        let parsed = ConnString::parse(string).expect("a string the card printed");
        let Ok(mut stream) = std::net::TcpStream::connect_timeout(
            &format!("{}:{}", parsed.host, parsed.port)
                .parse()
                .expect("an address"),
            Duration::from_millis(200),
        ) else {
            return "refused".to_string();
        };
        let key = parsed.key.map(|k| k.to_string()).unwrap_or_default();
        let line = serde_json::json!({
            "type": "register", "protocol": 1, "name": name, "key": key,
        });
        let _ = writeln!(stream, "{line}");
        let mut answer = String::new();
        let _ = BufReader::new(stream).read_line(&mut answer);
        answer
    }

    /// A full table withdraws its open chair: the string stops being
    /// shown, and a bot started with it anyway is refused rather than
    /// registered to a seat that is not there.
    #[test]
    fn a_full_table_withdraws_its_open_chair() {
        let mut door = Doorway::open().expect("a port on this machine");
        door.keep_a_chair_open(true);
        let string = door.open_string().expect("a chair is open");
        door.keep_a_chair_open(false);
        assert_eq!(door.open_string(), None, "no string is shown");
        assert!(door.chairs.is_empty());
        let answer = try_register(&string, "Late");
        assert!(answer.contains("error"), "refused: {answer}");
        // Room again: a fresh chair, with a key of its own.
        door.keep_a_chair_open(true);
        let fresh = door.open_string().expect("a chair is open again");
        assert_ne!(fresh, string, "the old key is not reissued");
    }

    /// A bot that came in but was dealt no seat is told why and let go,
    /// and the chair a bot could still come in by is withdrawn while the
    /// round is played.
    #[test]
    fn a_bot_dealt_no_seat_is_told_and_let_go() {
        let mut door = Doorway::open().expect("a port on this machine");
        door.keep_a_chair_open(true);
        let first = register(&door.open_string().expect("open"), "First", "Ana");
        seated(&mut door, 0);
        door.keep_a_chair_open(true);
        let second = register(&door.open_string().expect("open"), "Second", "Bo");
        seated(&mut door, 1);
        door.keep_a_chair_open(true);
        let late = door.open_string().expect("a third chair is open");
        let tr = &crate::app::i18n::EN;
        let ids: Vec<BotId> = door.arrived(tr).into_iter().map(|(id, _)| id).collect();
        // The table had room for the first bot only.
        door.seat_the_table(&ids[..1]);
        assert_eq!(door.arrived(tr).len(), 1, "the second bot was let go");
        assert_eq!(door.open_string(), None, "and no chair is open mid-round");
        second
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        let mut said = String::new();
        let mut reader = BufReader::new(second);
        while reader.read_line(&mut said).is_ok_and(|n| n > 0) {
            if said.contains("error") {
                break;
            }
        }
        assert!(said.contains(NO_SEAT), "told why: {said}");
        let answer = try_register(&late, "Late");
        assert!(
            answer.contains("error"),
            "the open key was taken back: {answer}"
        );
        drop(first);
    }

    /// Opened to the LAN and turned back, the card stops listening there:
    /// showing this machine's strings again is not still taking bots from
    /// the network.
    #[test]
    fn turning_the_lan_off_closes_its_socket() {
        let mut door = Doorway::open().expect("a port on this machine");
        door.toggle_lan().expect("listen on the LAN");
        let lan = door.lan.expect("a LAN socket");
        assert!(door.showing_lan);
        let wake = SocketAddr::from(([127, 0, 0, 1], lan.port()));
        assert!(std::net::TcpStream::connect_timeout(&wake, Duration::from_millis(200)).is_ok());
        door.toggle_lan().expect("back to this machine");
        assert!(!door.showing_lan);
        let mut refused = false;
        for _ in 0..100 {
            if std::net::TcpStream::connect_timeout(&wake, Duration::from_millis(50)).is_err() {
                refused = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(refused, "nobody listens on {wake} any more");
        // And on again, on a socket of its own.
        door.toggle_lan().expect("listen on the LAN again");
        assert!(door.showing_lan);
        assert!(door.lan.is_some());
    }

    #[test]
    fn a_doorway_seats_each_bot_in_the_chair_its_key_was_printed_for() {
        let mut door = Doorway::open().expect("a port on this machine");
        door.seat_bots(&[2, 3]);
        assert_eq!(door.chairs.len(), 2);
        assert!(!door.ready());
        let for_three = door.string(&door.chairs[1]);
        let _bot = register(&for_three, "Greedy", "Ana");
        for _ in 0..200 {
            door.poll();
            if door.chairs[1].bot.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(door.chairs[1].seat, 3);
        assert!(
            door.chairs[1].bot.is_some(),
            "the bot sat where its key said"
        );
        assert!(door.chairs[0].bot.is_none());
        let tr = &crate::app::i18n::EN;
        assert_eq!(door.seat_name(tr, 3).as_deref(), Some("Greedy (Ana)"));
        // A seat that stops wanting a bot gives its chair up; one that
        // keeps wanting one keeps its bot.
        door.seat_bots(&[3]);
        assert_eq!(door.chairs.len(), 1);
        assert!(door.ready());
    }

    /// A bot on the other end of a socket, reading what the game says.
    struct TestBot {
        stream: std::net::TcpStream,
        reader: BufReader<std::net::TcpStream>,
    }

    impl TestBot {
        fn register(string: &str) -> TestBot {
            let parsed = ConnString::parse(string).expect("a string the card printed");
            let stream =
                std::net::TcpStream::connect((parsed.host.as_str(), parsed.port)).expect("connect");
            let reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut bot = TestBot { stream, reader };
            let key = parsed.key.map(|k| k.to_string()).unwrap_or_default();
            bot.send(&serde_json::json!({
                "type": "register", "protocol": 1, "name": "Greedy", "key": key,
            }));
            let mut answer = String::new();
            bot.stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("timeout");
            let _ = bot.reader.read_line(&mut answer);
            assert!(answer.contains("\"registered\""), "{answer}");
            bot
        }

        fn send(&mut self, msg: &serde_json::Value) {
            writeln!(self.stream, "{msg}").expect("write");
        }

        /// The next message of type `kind`, skipping the others, if one
        /// comes within `within`.
        fn next(&mut self, kind: &str, within: Duration) -> Option<serde_json::Value> {
            let until = Instant::now() + within;
            loop {
                let left = until.checked_duration_since(Instant::now())?;
                self.stream
                    .set_read_timeout(Some(left.max(Duration::from_millis(1))))
                    .expect("timeout");
                let mut line = String::new();
                match self.reader.read_line(&mut line) {
                    Ok(0) | Err(_) => return None,
                    Ok(_) => {}
                }
                let msg: serde_json::Value = serde_json::from_str(&line).expect("json");
                if msg["type"] == kind {
                    return Some(msg);
                }
            }
        }
    }

    /// Poll `door` until `done` holds, for a few seconds at most.
    fn poll_until(door: &mut Doorway, done: impl Fn(&Doorway) -> bool) -> bool {
        for _ in 0..400 {
            door.poll();
            if done(door) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

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
        board.set_round_length(Some(1));
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

    /// A bot that goes before the match starts gives its chair up: the
    /// card stops counting it, and its seat gets a fresh key.
    #[test]
    fn a_bot_that_goes_gives_its_chair_up() {
        let mut door = Doorway::open().expect("a port on this machine");
        door.seat_bots(&[1]);
        let spent = door.string(&door.chairs[0]);
        let bot = TestBot::register(&spent);
        assert!(poll_until(&mut door, |d| d.ready()));
        drop(bot);
        assert!(
            poll_until(&mut door, |d| !d.ready()),
            "a dead bot kept its chair"
        );
        assert!(door.arrived(&crate::app::i18n::EN).is_empty());
        assert_eq!(door.chairs.len(), 1, "the seat still wants a bot");
        assert_ne!(door.string(&door.chairs[0]), spent, "a fresh key");
        // And a bot can take it with that key.
        let _next = TestBot::register(&door.string(&door.chairs[0]));
        assert!(poll_until(&mut door, |d| d.ready()));
    }

    /// The same at a host's door (route 2), where chairs are not seats:
    /// the dead bot's chair goes, and one chair stays open for the next.
    #[test]
    fn a_bot_that_leaves_a_hosts_door_is_not_seated() {
        let mut door = Doorway::open().expect("a port on this machine");
        door.keep_a_chair_open(true);
        let bot = TestBot::register(&door.open_string().expect("an open chair"));
        assert!(poll_until(&mut door, |d| d
            .arrived(&crate::app::i18n::EN)
            .len()
            == 1));
        door.keep_a_chair_open(true);
        drop(bot);
        assert!(
            poll_until(&mut door, |d| d.arrived(&crate::app::i18n::EN).is_empty()),
            "a dead bot is still at the beach"
        );
        door.keep_a_chair_open(true);
        assert_eq!(door.chairs.len(), 1, "{} chairs", door.chairs.len());
        assert!(door.chairs[0].bot.is_none());
    }
}
