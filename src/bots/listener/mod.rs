//! The listening half of the bot protocol: the TCP socket bots connect to,
//! registration, and the routing of what they send to the games they sit in.
//!
//! Every byte that arrives here is treated as hostile (see *Hardening* in
//! `docs/bot-seats.md`): lines are capped, a flood is rate-limited and then
//! dropped, wrong keys cost the address that sent them, and nothing a bot
//! sends becomes a path, a command or a format string.
//!
//! Threads: one accepts, and each connection has a reader and a writer. A
//! game never writes to a socket itself: it hands a line to the writer's
//! queue and moves on, so a bot that stops reading costs only itself, and
//! is dropped once its queue is full rather than stalling a table.

mod admit;
mod allowance;
mod wire;

use super::connstr::{Key, Token};
use super::protocol::Reply;
use crate::sim::{Board, PlayerId};
use allowance::Allowance;
use serde_json::Value;
use std::collections::HashMap;
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use wire::{accept_loop, line_of, ping, routes_of, send, send_line};

/// A registered bot's number on this listener.
pub type BotId = usize;

/// One way in: a join key, and what it admits.
#[derive(Clone, Debug)]
pub struct Invite {
    pub key: Key,
    /// Registrations it has left; `None` for a key that is never spent,
    /// like a cup's shared invite.
    pub uses: Option<u32>,
    /// The owner this key belongs to, which overrides whatever a bot
    /// registering with it declares (`cup invite NAME`).
    pub owner: Option<String>,
    /// The seat this key was printed for, in an arena with several open.
    pub slot: Option<u32>,
}

/// Who may register.
#[derive(Clone, Debug)]
pub enum Admission {
    /// Anybody who can reach the port (`--open-registration`).
    Open,
    /// Only a bot holding one of these keys.
    Keys(Vec<Invite>),
}

#[derive(Clone, Debug)]
pub struct Config {
    pub admission: Admission,
    /// The most games one bot may play at once, whatever it asks for.
    pub parallel_cap: u32,
    /// The most bots this listener takes, if it is counting (an arena with
    /// `--open 2` has two chairs and no more).
    pub max_bots: Option<usize>,
    /// The most bots one owner may register (`--per-owner`).
    pub per_owner: Option<usize>,
}

impl Config {
    pub fn new(admission: Admission) -> Config {
        Config {
            admission,
            parallel_cap: 8,
            max_bots: None,
            per_owner: None,
        }
    }
}

/// What a registered bot said about itself, and what the listener decided.
#[derive(Clone, Debug)]
pub struct BotInfo {
    pub id: BotId,
    pub name: String,
    pub version: String,
    pub owner: Option<String>,
    /// The owner came from the bot, not from a per-author key: trust among
    /// friends, and labelled as such.
    pub owner_declared: bool,
    pub parallel: u32,
    /// The seat its key was printed for, if it was printed for one.
    pub slot: Option<u32>,
    pub addr: IpAddr,
}

impl BotInfo {
    /// Who the bot counts as for `--per-owner` and the cup's draw: see
    /// [`owner_key`].
    pub fn owner_key(&self) -> String {
        owner_key(self.owner.as_deref(), self.owner_declared, self.addr)
    }
}

/// Who a bot counts as, for one owner's cap and for keeping one owner's
/// bots apart: its owner, whatever the case, or, when it declared none,
/// the address it came from. An owner a bot declared is a different owner
/// from the same name bound by the host's per-author key: a rival on the
/// shared key calling itself "alice" is not the invited Alice, for the cap
/// or for the draw. An owner is display text with its control characters
/// removed, so neither reads as the other or as an address's key.
pub fn owner_key(owner: Option<&str>, declared: bool, addr: IpAddr) -> String {
    match (owner, declared) {
        (Some(owner), false) => folded(owner),
        (Some(owner), true) => format!("\u{2}{}", folded(owner)),
        (None, _) => format!("\u{1}{addr}"),
    }
}

/// Whether two owner names are the same owner: whatever the case, as
/// [`owner_key`] keys them.
pub fn same_owner(a: &str, b: &str) -> bool {
    folded(a) == folded(b)
}

/// An owner name with its case let go, which is all the two above compare.
fn folded(owner: &str) -> String {
    owner.to_lowercase()
}

/// What the console hears about.
#[derive(Clone, Debug)]
pub enum Event {
    Registered(BotId),
    Reconnected(BotId),
    Dropped(BotId),
    /// A connection was turned away, and why. For the console, which says
    /// so; never with a token in it.
    Refused(SocketAddr, String),
}

/// What a game hears from its seats.
#[derive(Debug)]
pub enum GameMsg {
    Reply {
        seat: PlayerId,
        reply: Reply,
        at: Instant,
    },
    Ready {
        seat: PlayerId,
    },
    /// Something the protocol does not say, from a bot in this game.
    Garbled {
        seat: PlayerId,
        why: String,
        at: Instant,
    },
    /// The bot's connection went.
    Dropped {
        seat: PlayerId,
    },
    /// It came back with its token, on the connection with this serial.
    /// The old one may not have been seen to go: a reconnect while it is
    /// still open replaces it with nothing said of its going.
    Back {
        seat: PlayerId,
        serial: u64,
    },
}

impl GameMsg {
    /// The seat it came from.
    pub fn seat(&self) -> PlayerId {
        match self {
            GameMsg::Reply { seat, .. }
            | GameMsg::Ready { seat }
            | GameMsg::Garbled { seat, .. }
            | GameMsg::Dropped { seat }
            | GameMsg::Back { seat, .. } => *seat,
        }
    }
}

/// The board a game is on now, for answering lookaheads off the game's own
/// thread.
pub struct View {
    pub board: Board,
    pub cursors: Vec<Option<(u8, u8)>>,
    /// Lookahead ticks each seat may still spend on this tick.
    pub budget: Vec<u32>,
}

struct Route {
    seats: Vec<(BotId, PlayerId)>,
    tx: Sender<GameMsg>,
    view: Arc<Mutex<Option<View>>>,
}

/// A game's line to the listener: what its seats send, and the board it
/// publishes for their lookaheads. Dropping it closes the route.
///
/// The receiver sits behind a lock only so a link can live in a Bevy
/// resource, which has to be shareable; one game reads it at a time.
pub struct GameLink {
    rx: Mutex<Receiver<GameMsg>>,
    pub view: Arc<Mutex<Option<View>>>,
    game: u32,
    shared: Arc<Shared>,
}

impl GameLink {
    /// The next thing a seat said, if anything is waiting.
    pub fn try_recv(&self) -> Option<GameMsg> {
        lock(&self.rx).try_recv().ok()
    }

    /// The next thing a seat says, waiting up to `timeout` for it.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<GameMsg, mpsc::RecvTimeoutError> {
        lock(&self.rx).recv_timeout(timeout)
    }

    /// Write to one of this game's bots.
    pub fn send(&self, id: BotId, msg: &Value) -> bool {
        let line = line_of(msg);
        send_line(&mut lock(&self.shared.registry), id, line, true)
    }
}

impl Drop for GameLink {
    fn drop(&mut self) {
        lock(&self.shared.registry).routes.remove(&self.game);
    }
}

struct Conn {
    serial: u64,
    tx: SyncSender<Arc<str>>,
    stream: TcpStream,
    /// Bytes handed to the writer and not yet written.
    queued: Arc<AtomicUsize>,
}

struct Bot {
    info: BotInfo,
    token: Token,
    /// The key it was let in with, if that key was printed for it alone
    /// (an arena's per-seat key, which names its `slot`): with its name, what lets it back in
    /// without its token (see [`comeback`](admit::comeback)). A shared key is held by every
    /// entrant and proves nothing about which of them this is, so it is
    /// not kept.
    key: Option<Key>,
    conn: Option<Conn>,
    /// Round trips measured by ping, newest last.
    rtts: Vec<Duration>,
    pings: HashMap<u64, Instant>,
    allowance: Allowance,
    /// Gone, and its chair given away ([`Listener::retire`]): its token no
    /// longer brings it back, and its name is free for a bot started anew.
    retired: bool,
    /// A game was played without it, forfeited for never coming, and it has
    /// not connected since ([`Listener::absent`]).
    absent: bool,
}

#[derive(Default)]
struct Registry {
    bots: Vec<Bot>,
    routes: HashMap<u32, Route>,
    /// Finished games' replays, and who may fetch them.
    replays: HashMap<String, (Vec<BotId>, Arc<str>)>,
    wrong_keys: HashMap<IpAddr, admit::Strikes>,
    /// Connections not registered yet, by address.
    pending: HashMap<IpAddr, usize>,
    next_serial: u64,
    next_ping: u64,
}

struct Shared {
    registry: Mutex<Registry>,
    config: Mutex<Config>,
    events: Mutex<Sender<Event>>,
    /// Every address an accept loop is listening on, to wake it when the
    /// listener closes.
    bound: Mutex<Vec<SocketAddr>>,
    /// Addresses no longer listened on, though the listener goes on: a
    /// card's LAN socket, closed again. Their accept loops end on waking.
    stopped: Mutex<Vec<SocketAddr>>,
    closed: std::sync::atomic::AtomicBool,
}

/// A held lock, whatever a panicking thread left it as: the registry is
/// plain data and every write to it leaves it whole.
pub(super) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The listening socket and everything registered with it.
#[derive(Clone)]
pub struct Listener {
    shared: Arc<Shared>,
    addr: SocketAddr,
}

impl Listener {
    /// Listen on `addr`. The events receiver is the console's.
    pub fn bind(addr: SocketAddr, config: Config) -> std::io::Result<(Listener, Receiver<Event>)> {
        let socket = TcpListener::bind(addr)?;
        let addr = socket.local_addr()?;
        let (events, rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            registry: Mutex::new(Registry::default()),
            config: Mutex::new(config),
            events: Mutex::new(events),
            bound: Mutex::new(vec![addr]),
            stopped: Mutex::new(Vec::new()),
            closed: std::sync::atomic::AtomicBool::new(false),
        });
        let accepting = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("bot-accept".into())
            .spawn(move || accept_loop(&socket, &accepting))?;
        Ok((Listener { shared, addr }, rx))
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Listen on a second address too, into the same registry: a card that
    /// listened on this machine alone, opened to the LAN. Returns where the
    /// new socket landed.
    pub fn also_listen(&self, addr: SocketAddr) -> std::io::Result<SocketAddr> {
        let socket = TcpListener::bind(addr)?;
        let bound = socket.local_addr()?;
        // An accept loop on this port that was stopped has ended (the port
        // could not be bound again otherwise) and should have taken it off
        // the list itself; make sure, or this loop would end at its first
        // connection.
        lock(&self.shared.stopped).retain(|a| *a != bound);
        lock(&self.shared.bound).push(bound);
        let accepting = Arc::clone(&self.shared);
        std::thread::Builder::new()
            .name("bot-accept".into())
            .spawn(move || accept_loop(&socket, &accepting))?;
        Ok(bound)
    }

    /// Stop listening on one address opened by [`Self::also_listen`],
    /// keeping the rest: a card opened to the LAN and closed again. The
    /// bots already in stay in, whichever way they came.
    pub fn stop_listening(&self, addr: SocketAddr) {
        {
            let mut bound = lock(&self.shared.bound);
            let Some(at) = bound.iter().position(|a| *a == addr) else {
                return;
            };
            bound.remove(at);
        }
        lock(&self.shared.stopped).push(addr);
        let _ = TcpStream::connect_timeout(&wake_address(addr), Duration::from_millis(200));
    }

    /// Stop listening and drop every bot: the sockets close, the threads
    /// end. The arena and a cup never need to, since their process ends;
    /// the game opens a listener for a card and closes it with the card.
    pub fn close(&self) {
        if self.shared.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        // Each accept loop is woken by a connection of its own, and sees
        // the flag before it serves anybody.
        for addr in lock(&self.shared.bound).iter() {
            let _ = TcpStream::connect_timeout(&wake_address(*addr), Duration::from_millis(200));
        }
        let mut registry = lock(&self.shared.registry);
        for bot in &mut registry.bots {
            if let Some(conn) = bot.conn.take() {
                let _ = conn.stream.shutdown(Shutdown::Both);
            }
        }
        registry.routes.clear();
    }

    /// Close one bot's connection: a host asked it to leave. It is let go
    /// for good ([`Self::let_go`]): the host has given its chair away, so
    /// its token no longer brings it back, and its name is free.
    pub fn disconnect(&self, id: BotId) {
        self.let_go(id, None);
    }

    /// Tell one bot why it is being let go (the table filled up before it
    /// had a seat, say), and close its connection once that has been
    /// written. Let go for good, as [`Self::disconnect`] is.
    pub fn dismiss(&self, id: BotId, why: &str) {
        self.let_go(id, Some(why));
    }

    /// Let a bot go on the host's word, telling it `why` first if there is
    /// something to tell. It is retired, so its name is free and its token
    /// lands nowhere, and its going is reported like any other: the games
    /// it sits in hear it dropped, and so does whoever reads the events.
    /// Its own reader, finding the connection no longer its, says nothing
    /// more.
    fn let_go(&self, id: BotId, why: Option<&str>) {
        let mut registry = lock(&self.shared.registry);
        if let Some(why) = why {
            send(&mut registry, id, &super::protocol::fatal(why));
        }
        let Some(bot) = registry.bots.get_mut(id) else {
            return;
        };
        bot.retired = true;
        let Some(conn) = bot.conn.take() else {
            return;
        };
        // With something said, the writer drains the queue and then shuts
        // the socket; with nothing, it is shut now.
        if why.is_none() {
            let _ = conn.stream.shutdown(Shutdown::Both);
        }
        drop(conn);
        let routes = routes_of(&registry, id);
        drop(registry);
        for (tx, seat) in routes {
            let _ = tx.send(GameMsg::Dropped { seat });
        }
        emit(&self.shared, Event::Dropped(id));
    }

    /// Finish with a bot that went and whose chair was given away: its
    /// token no longer brings it back, and its name is free, so the same
    /// bot started again registers afresh rather than being told its name
    /// is taken by the one that died.
    pub fn retire(&self, id: BotId) {
        let mut registry = lock(&self.shared.registry);
        if let Some(bot) = registry.bots.get_mut(id) {
            bot.retired = true;
            if let Some(conn) = bot.conn.take() {
                let _ = conn.stream.shutdown(Shutdown::Both);
            }
        }
    }

    /// Take back the keys printed for a slot nobody has taken: a key for
    /// a chair that is no longer there seats nobody.
    pub fn withdraw(&self, slot: u32) {
        let mut config = lock(&self.shared.config);
        if let Admission::Keys(keys) = &mut config.admission {
            keys.retain(|invite| invite.slot != Some(slot));
        }
    }

    /// Add a way in (a cup's per-author invite, a card's next key).
    pub fn invite(&self, invite: Invite) {
        let mut config = lock(&self.shared.config);
        match &mut config.admission {
            Admission::Keys(keys) => keys.push(invite),
            Admission::Open => config.admission = Admission::Keys(vec![invite]),
        }
    }

    /// Stop taking new registrations; the bots already in stay in.
    pub fn close_registration(&self) {
        let mut config = lock(&self.shared.config);
        let registered = lock(&self.shared.registry).bots.len();
        config.max_bots = Some(registered);
    }

    /// Take registrations again after [`close_registration`]: a cup that
    /// closed to count its field and found it could not be drawn.
    ///
    /// [`close_registration`]: Listener::close_registration
    pub fn reopen_registration(&self) {
        lock(&self.shared.config).max_bots = None;
    }

    pub fn bot(&self, id: BotId) -> Option<BotInfo> {
        lock(&self.shared.registry)
            .bots
            .get(id)
            .map(|b| b.info.clone())
    }

    pub fn bots(&self) -> Vec<BotInfo> {
        lock(&self.shared.registry)
            .bots
            .iter()
            .map(|b| b.info.clone())
            .collect()
    }

    /// How many lines bot `id` may still send before it is refused.
    #[cfg(test)]
    pub fn allowance(&self, id: BotId) -> f64 {
        lock(&self.shared.registry).bots[id].allowance.level
    }

    /// What a bot may hold in hand and no more, once it stops earning.
    #[cfg(test)]
    pub const BURST: f64 = allowance::BURST;

    pub fn connected(&self, id: BotId) -> bool {
        lock(&self.shared.registry)
            .bots
            .get(id)
            .is_some_and(|b| b.conn.is_some())
    }

    /// Note that a game was forfeited because bot `id` never came to it.
    pub fn mark_absent(&self, id: BotId) {
        let mut registry = lock(&self.shared.registry);
        if let Some(bot) = registry.bots.get_mut(id)
            && bot.conn.is_none()
        {
            bot.absent = true;
        }
    }

    /// Bot `id` has already forfeited a game for never coming, and has not
    /// connected since: the next game need not wait out the whole forfeit
    /// timeout for it again. Connecting clears it.
    pub fn absent(&self, id: BotId) -> bool {
        lock(&self.shared.registry)
            .bots
            .get(id)
            .is_some_and(|b| b.absent && b.conn.is_none())
    }

    /// The median round trip measured by ping, if any came back.
    pub fn rtt(&self, id: BotId) -> Option<Duration> {
        let registry = lock(&self.shared.registry);
        let mut rtts = registry.bots.get(id)?.rtts.clone();
        rtts.sort();
        rtts.get(rtts.len() / 2).copied()
    }

    /// Ask every connected bot for a pong, outside any deadline.
    pub fn ping_all(&self) {
        let mut registry = lock(&self.shared.registry);
        let ids: Vec<BotId> = (0..registry.bots.len()).collect();
        for id in ids {
            ping(&mut registry, id);
        }
    }

    /// Write one of a game's messages to a bot. False if it is not
    /// connected, or was just dropped for not reading.
    pub fn send(&self, id: BotId, msg: &Value) -> bool {
        let line = line_of(msg);
        send_line(&mut lock(&self.shared.registry), id, line, true)
    }

    /// Write a game's `hello` to a bot, as [`Self::send`] does, and say
    /// which connection it went to: a game that hears the bot came back
    /// on that same connection knows it has been greeted there already.
    pub fn greet(&self, id: BotId, msg: &Value) -> Option<u64> {
        let line = line_of(msg);
        let mut registry = lock(&self.shared.registry);
        let serial = registry.bots.get(id)?.conn.as_ref()?.serial;
        send_line(&mut registry, id, line, true).then_some(serial)
    }

    /// Open a game's route: its seats' messages come to the returned link.
    pub fn open_game(&self, game: u32, seats: Vec<(BotId, PlayerId)>) -> GameLink {
        let (tx, rx) = mpsc::channel();
        let view = Arc::new(Mutex::new(None));
        lock(&self.shared.registry).routes.insert(
            game,
            Route {
                seats,
                tx,
                view: Arc::clone(&view),
            },
        );
        GameLink {
            rx: Mutex::new(rx),
            view,
            game,
            shared: Arc::clone(&self.shared),
        }
    }

    /// Keep a finished game's replay for the bots that played in it.
    pub fn keep_replay(&self, id: String, players: Vec<BotId>, text: String) {
        lock(&self.shared.registry)
            .replays
            .insert(id, (players, text.into()));
    }

    /// Let a kept replay go: nobody fetches it after this.
    pub fn forget_replay(&self, id: &str) {
        lock(&self.shared.registry).replays.remove(id);
    }
}

/// Where to connect to wake the accept loop listening on `addr`.
fn wake_address(addr: SocketAddr) -> SocketAddr {
    match addr.ip() {
        ip if ip.is_unspecified() => SocketAddr::new([127, 0, 0, 1].into(), addr.port()),
        _ => addr,
    }
}

fn emit(shared: &Shared, event: Event) {
    let _ = lock(&shared.events).send(event);
}
