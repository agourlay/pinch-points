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

use super::connstr::{Key, Token};
use super::lookahead;
use super::protocol::{self, Incoming, PROTOCOL, Register, Reply};
use crate::sim::{Board, PlayerId};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// A registered bot's number on this listener.
pub type BotId = usize;

/// How long a fresh connection has to register before it is closed.
const REGISTER_WITHIN: Duration = Duration::from_secs(10);
/// Wrong keys from one address before that address is turned away.
const WRONG_KEYS: u32 = 3;
/// How long an address that guessed badly is turned away.
const LOCKOUT: Duration = Duration::from_secs(60);
/// Lines queued for a bot that is not reading before it is dropped.
const QUEUE: usize = 256;
/// Bytes queued for a bot that is not reading before it is dropped: a
/// couple of megabytes, whatever the lines are (a tick is about 10 KB, a
/// replay a few hundred). A single line bigger than this still goes out
/// to a bot with nothing else waiting.
const QUEUE_BYTES: usize = 2 * 1024 * 1024;
/// Connections that have not registered yet, in all and from one address.
/// Each holds a thread for up to [`REGISTER_WITHIN`], so without a cap a
/// few hundred silent connections would use the machine up.
const PENDING: usize = 64;
const PENDING_PER_ADDR: usize = 8;
/// The flood valve. A bot earns the right to send by being sent things by
/// a game: every tick (or hello, or end) written to it is worth `PER_LINE`
/// messages back (a reply and a few lookaheads), so a fast bot in a
/// fast-forward game, answering thousands of ticks a second, is never
/// mistaken for a flood. What the listener says back to the bot's own
/// requests (errors, replays, lookaheads) earns nothing, or a request that
/// draws an error would pay for itself. On top of that it may send
/// `PER_SECOND` a second of its own accord, and save up to `BURST`.
const BURST: f64 = 400.0;
const PER_SECOND: f64 = 50.0;
const PER_LINE: f64 = 6.0;
/// The most a bot can have saved up, earned and refilled together.
const CEILING: f64 = 2000.0;
/// Messages dropped for flooding before the connection goes, counted over
/// a sliding window: each drop is a strike, and strikes are forgiven at
/// `FORGIVE_PER_SECOND`. A bot that overruns once in a while is never
/// let go, and one that keeps on overrunning faster than it is forgiven
/// is, however slowly it does: one having `d` messages a second dropped
/// goes after 400 / (d - 40) seconds, ten at 80 a second, four at 140.
const FLOOD_STRIKES: f64 = 400.0;
const FORGIVE_PER_SECOND: f64 = 40.0;

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
        owner_key(self.owner.as_deref(), self.addr)
    }
}

/// Who a bot counts as, for one owner's cap and for keeping one owner's
/// bots apart: its owner, whatever the case, or, when it declared none,
/// the address it came from. An owner is display text with its control
/// characters removed, so it never reads as an address's key.
pub fn owner_key(owner: Option<&str>, addr: IpAddr) -> String {
    match owner {
        Some(owner) => owner.to_lowercase(),
        None => format!("\u{1}{addr}"),
    }
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
    wrong_keys: HashMap<IpAddr, (u32, Option<Instant>)>,
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
            send(
                &mut registry,
                id,
                &json!({"type": "error", "fatal": true, "message": why}),
            );
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
    pub const BURST: f64 = BURST;

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
}

/// Where to connect to wake the accept loop listening on `addr`.
fn wake_address(addr: SocketAddr) -> SocketAddr {
    match addr.ip() {
        ip if ip.is_unspecified() => SocketAddr::new([127, 0, 0, 1].into(), addr.port()),
        _ => addr,
    }
}

fn accept_loop(socket: &TcpListener, shared: &Arc<Shared>) {
    let me = socket.local_addr().ok();
    for stream in socket.incoming() {
        if shared.closed.load(Ordering::SeqCst)
            || me.is_some_and(|me| lock(&shared.stopped).contains(&me))
        {
            return;
        }
        let Ok(stream) = stream else {
            continue;
        };
        let shared = Arc::clone(shared);
        let _ = std::thread::Builder::new()
            .name("bot-conn".into())
            .spawn(move || serve(stream, &shared));
    }
}

fn emit(shared: &Shared, event: Event) {
    let _ = lock(&shared.events).send(event);
}

/// Write a refusal and close. The message says why, never with a secret
/// in it.
fn refuse(mut stream: TcpStream, shared: &Shared, peer: SocketAddr, why: &str) {
    let line = format!(
        "{}\n",
        json!({"type": "error", "fatal": true, "message": why})
    );
    let _ = stream.write_all(line.as_bytes());
    let _ = stream.shutdown(Shutdown::Both);
    emit(shared, Event::Refused(peer, why.to_string()));
}

/// Read one line of at most `cap` bytes into `buf`. `Ok(false)` at the end
/// of the stream; an error for a line over the cap, which is never read to
/// its end.
///
/// With a `deadline` the whole line has to be in by then, an error
/// (`TimedOut`) if not. A read timeout alone restarts with every byte, so
/// a connection dribbling one byte every few seconds would hold its place
/// among those waiting to register for as long as it liked; here each read
/// is given only what is left of the time.
fn read_line(
    reader: &mut BufReader<TcpStream>,
    buf: &mut Vec<u8>,
    cap: usize,
    deadline: Option<Instant>,
) -> std::io::Result<bool> {
    buf.clear();
    loop {
        if let Some(deadline) = deadline
            && reader.buffer().is_empty()
        {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "no whole line in time",
                ));
            }
            reader.get_ref().set_read_timeout(Some(left))?;
        }
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(!buf.is_empty());
        }
        let (chunk, done) = match available.iter().position(|&b| b == b'\n') {
            Some(i) => (&available[..i], i + 1),
            None => (available, available.len()),
        };
        if buf.len() + chunk.len() > cap {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "a line longer than 64 KiB",
            ));
        }
        buf.extend_from_slice(chunk);
        let newline = done > chunk.len();
        reader.consume(done);
        if newline {
            return Ok(true);
        }
    }
}

/// One connection, from its first byte to its last.
fn serve(stream: TcpStream, shared: &Arc<Shared>) {
    let Ok(peer) = stream.peer_addr() else {
        return;
    };
    // Nagle meeting delayed ACKs can hold a small line for 40 ms, longer
    // than a whole deadline.
    let _ = stream.set_nodelay(true);
    let Some(pending) = Pending::take(shared, peer.ip()) else {
        refuse(
            stream,
            shared,
            peer,
            "too many connections waiting to register; try again shortly",
        );
        return;
    };
    // Ten seconds for the whole registration line, however it trickles in.
    let deadline = Instant::now() + REGISTER_WITHIN;
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let mut buf = Vec::new();
    let register = match read_line(&mut reader, &mut buf, protocol::MAX_LINE, Some(deadline)) {
        Ok(true) => {
            let Incoming::Register(register) =
                protocol::parse_incoming(&String::from_utf8_lossy(&buf))
            else {
                refuse(stream, shared, peer, "the first message is a register");
                return;
            };
            register
        }
        Ok(false) | Err(_) => {
            refuse(stream, shared, peer, "no registration within ten seconds");
            return;
        }
    };
    // The lockout guards the keys, which are short enough to guess at. A
    // token is not, so a bot coming back with one is never turned away
    // for what somebody else at its address (behind the same proxy, say)
    // got wrong.
    if register.token.is_none() && locked_out(shared, peer.ip()) {
        refuse(
            stream,
            shared,
            peer,
            "too many wrong keys from this address; try again in a minute",
        );
        return;
    }
    let _ = stream.set_read_timeout(None);
    let registered = register_bot(shared, &stream, peer, register);
    drop(pending);
    let (id, serial) = match registered {
        Ok(ok) => ok,
        Err(why) => {
            refuse(stream, shared, peer, &why);
            return;
        }
    };
    read_loop(&mut reader, shared, id, serial);
    disconnect(shared, id, serial);
}

/// A connection's place among those waiting to register, given back when
/// it registers or goes.
struct Pending<'a> {
    shared: &'a Shared,
    ip: IpAddr,
}

impl<'a> Pending<'a> {
    fn take(shared: &'a Shared, ip: IpAddr) -> Option<Pending<'a>> {
        let mut registry = lock(&shared.registry);
        let all: usize = registry.pending.values().sum();
        let here = registry.pending.entry(ip).or_insert(0);
        if all >= PENDING || *here >= PENDING_PER_ADDR {
            if *here == 0 {
                registry.pending.remove(&ip);
            }
            return None;
        }
        *here += 1;
        Some(Pending { shared, ip })
    }
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        let mut registry = lock(&self.shared.registry);
        if let Some(here) = registry.pending.get_mut(&self.ip) {
            *here -= 1;
            if *here == 0 {
                registry.pending.remove(&self.ip);
            }
        }
    }
}

fn locked_out(shared: &Shared, ip: IpAddr) -> bool {
    let mut registry = lock(&shared.registry);
    match registry.wrong_keys.get(&ip) {
        Some((_, Some(until))) if Instant::now() < *until => true,
        Some((_, Some(_))) => {
            registry.wrong_keys.remove(&ip);
            false
        }
        _ => false,
    }
}

/// A wrong key costs the address that sent it, never the key: rotating the
/// key on a bad guess would let anyone on the LAN keep a host's card
/// changing just by guessing badly.
fn wrong_key(shared: &Shared, ip: IpAddr) {
    let mut registry = lock(&shared.registry);
    let entry = registry.wrong_keys.entry(ip).or_insert((0, None));
    entry.0 += 1;
    if entry.0 >= WRONG_KEYS {
        entry.1 = Some(Instant::now() + LOCKOUT);
    }
}

/// Take a registration, or say why not. Returns the bot's id and this
/// connection's serial.
fn register_bot(
    shared: &Arc<Shared>,
    stream: &TcpStream,
    peer: SocketAddr,
    register: Register,
) -> Result<(BotId, u64), String> {
    if register.protocol != PROTOCOL {
        return Err(format!(
            "this listener speaks protocol {PROTOCOL}, not {}",
            register.protocol
        ));
    }
    let writer = stream.try_clone().map_err(|e| e.to_string())?;
    // A reconnect: the token is who the bot is, and nothing else is asked.
    // A wrong one is no strike against the address: a token is too long
    // to guess, and a stale one is a bot that outlived its listener, not
    // somebody trying keys.
    if let Some(token) = &register.token {
        let token = Token::from_wire(token);
        let mut registry = lock(&shared.registry);
        let Some(id) = registry.bots.iter().position(|b| b.token == token) else {
            return Err("that token is not one this listener issued".to_string());
        };
        // Only the bot itself holds its token, so it may hear what became
        // of it.
        if registry.bots[id].retired {
            return Err(
                "this bot was let go here and its seat given away; register afresh".to_string(),
            );
        }
        let serial = attach(&mut registry, id, writer);
        let msg = json!({
            "type": "registered", "name": registry.bots[id].info.name,
            "token": registry.bots[id].token.reveal(), "resumed": true,
        });
        send(&mut registry, id, &msg);
        let routes: Vec<(Sender<GameMsg>, PlayerId)> = routes_of(&registry, id);
        drop(registry);
        for (tx, seat) in routes {
            let _ = tx.send(GameMsg::Back { seat, serial });
        }
        emit(shared, Event::Reconnected(id));
        return Ok((id, serial));
    }
    if register.name.is_empty() {
        return Err("a registration needs a `name`".to_string());
    }
    let mut config = lock(&shared.config);
    let mut registry = lock(&shared.registry);
    // The key, when one is asked for: found first and spent last, so a
    // registration refused for its name does not burn a single-use key.
    let invite = match &config.admission {
        Admission::Open => None,
        Admission::Keys(invites) => {
            let Some(given) = register.key.as_deref() else {
                return Err("this listener needs the key from its connection string".to_string());
            };
            let found = Key::parse(given).and_then(|key| {
                invites
                    .iter()
                    .position(|i| i.key == key && i.uses != Some(0))
            });
            let Some(found) = found else {
                drop(registry);
                drop(config);
                wrong_key(shared, peer.ip());
                return Err("that key is not one this listener is taking".to_string());
            };
            Some(found)
        }
    };
    if config
        .max_bots
        .is_some_and(|max| registry.bots.len() >= max)
    {
        return Err("registration is closed".to_string());
    }
    if registry
        .bots
        .iter()
        .any(|b| !b.retired && b.info.name.eq_ignore_ascii_case(&register.name))
    {
        return Err(format!("the name {:?} is taken here", register.name));
    }
    let (bound_owner, slot) = match (&mut config.admission, invite) {
        (Admission::Keys(invites), Some(i)) => (invites[i].owner.clone(), invites[i].slot),
        _ => (None, None),
    };
    let owner_declared = bound_owner.is_none();
    let owner = bound_owner.or(register.owner);
    // A bot that declares no owner is counted under its address, so leaving
    // the owner out is no way round the cap.
    if let Some(cap) = config.per_owner {
        let key = owner_key(owner.as_deref(), peer.ip());
        let theirs = registry
            .bots
            .iter()
            .filter(|b| !b.retired && b.info.owner_key() == key)
            .count();
        if theirs >= cap {
            return Err(match &owner {
                Some(owner) => {
                    format!("{owner} already has {theirs} bot(s) entered, the most allowed")
                }
                None => format!(
                    "{} already has {theirs} bot(s) entered with no owner, the most allowed; \
                     declare an `owner`",
                    peer.ip()
                ),
            });
        }
    }
    if let (Admission::Keys(invites), Some(i)) = (&mut config.admission, invite)
        && let Some(uses) = &mut invites[i].uses
    {
        *uses -= 1;
    }
    let id = registry.bots.len();
    let token = Token::draw();
    registry.bots.push(Bot {
        info: BotInfo {
            id,
            name: register.name.clone(),
            version: register.version,
            owner,
            owner_declared,
            parallel: register.parallel.min(config.parallel_cap).max(1),
            slot,
            addr: peer.ip(),
        },
        token,
        conn: None,
        rtts: Vec::new(),
        pings: HashMap::new(),
        allowance: Allowance::new(),
        retired: false,
        absent: false,
    });
    drop(config);
    let serial = attach(&mut registry, id, writer);
    let msg = json!({
        "type": "registered", "name": register.name,
        "token": registry.bots[id].token.reveal(),
    });
    send(&mut registry, id, &msg);
    // A few pings straight away, so the console has a round trip to show.
    for _ in 0..3 {
        ping(&mut registry, id);
    }
    drop(registry);
    emit(shared, Event::Registered(id));
    Ok((id, serial))
}

/// Give bot `id` this connection, closing any older one it had.
fn attach(registry: &mut Registry, id: BotId, stream: TcpStream) -> u64 {
    registry.next_serial += 1;
    let serial = registry.next_serial;
    let (tx, rx) = mpsc::sync_channel::<Arc<str>>(QUEUE);
    let Ok(writer) = stream.try_clone() else {
        return serial;
    };
    let queued = Arc::new(AtomicUsize::new(0));
    let written = Arc::clone(&queued);
    let _ = std::thread::Builder::new()
        .name("bot-write".into())
        .spawn(move || write_loop(writer, &rx, &written));
    let conn = Conn {
        serial,
        tx,
        stream,
        queued,
    };
    // Here, whatever games went by without it.
    registry.bots[id].absent = false;
    if let Some(old) = registry.bots[id].conn.replace(conn) {
        let _ = old.stream.shutdown(Shutdown::Both);
    }
    serial
}

/// Write a connection's lines until its sender goes, then close it. The
/// close is here, after the queue is drained, so the last thing said to a
/// bot (a fatal error, say) reaches it before the socket shuts.
fn write_loop(mut stream: TcpStream, rx: &Receiver<Arc<str>>, queued: &AtomicUsize) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    for line in rx {
        let written = stream.write_all(line.as_bytes());
        queued.fetch_sub(line.len(), Ordering::SeqCst);
        if written.is_err() {
            break;
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
}

/// A message as the line that carries it. Made before the registry is
/// locked: a replay or a lookahead's answer is a lot of text to write out.
fn line_of(msg: &Value) -> Arc<str> {
    format!("{msg}\n").into()
}

/// Answer a bot: an error, a replay, a lookahead. Earns it nothing.
fn send(registry: &mut Registry, id: BotId, msg: &Value) -> bool {
    send_line(registry, id, line_of(msg), false)
}

/// Queue a line for bot `id`. `earns` for a game's own lines, which the
/// bot is owed the right to answer (see [`PER_LINE`]).
fn send_line(registry: &mut Registry, id: BotId, line: Arc<str>, earns: bool) -> bool {
    let Some(bot) = registry.bots.get_mut(id) else {
        return false;
    };
    let Some(conn) = &bot.conn else {
        return false;
    };
    let len = line.len();
    let waiting = conn.queued.load(Ordering::SeqCst);
    // A bot that has stopped reading is dropped rather than waited for.
    if waiting > 0 && waiting + len > QUEUE_BYTES {
        let _ = conn.stream.shutdown(Shutdown::Both);
        return false;
    }
    conn.queued.fetch_add(len, Ordering::SeqCst);
    match conn.tx.try_send(line) {
        Ok(()) => {
            if earns {
                bot.allowance.credit();
            }
            true
        }
        Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
            conn.queued.fetch_sub(len, Ordering::SeqCst);
            let _ = conn.stream.shutdown(Shutdown::Both);
            false
        }
    }
}

fn ping(registry: &mut Registry, id: BotId) {
    registry.next_ping += 1;
    let n = registry.next_ping;
    if send(registry, id, &json!({"type": "ping", "id": n})) {
        registry.bots[id].pings.insert(n, Instant::now());
    }
}

/// The routes bot `id` has a seat in, with the seat.
fn routes_of(registry: &Registry, id: BotId) -> Vec<(Sender<GameMsg>, PlayerId)> {
    registry
        .routes
        .values()
        .flat_map(|route| {
            route
                .seats
                .iter()
                .filter(|(bot, _)| *bot == id)
                .map(|(_, seat)| (route.tx.clone(), *seat))
        })
        .collect()
}

/// A bot's way into one game: where its messages go, its seat, and the
/// board its lookaheads run on.
struct SeatRoute {
    tx: Sender<GameMsg>,
    seat: PlayerId,
    view: Arc<Mutex<Option<View>>>,
}

/// The seat bot `id` holds in `game`, and the route to it.
fn seat_in(registry: &Registry, id: BotId, game: u32) -> Option<SeatRoute> {
    let route = registry.routes.get(&game)?;
    let seat = route.seats.iter().find(|(bot, _)| *bot == id)?.1;
    Some(SeatRoute {
        tx: route.tx.clone(),
        seat,
        view: Arc::clone(&route.view),
    })
}

/// What a bot may still send before it is flooding: see [`BURST`].
struct Allowance {
    level: f64,
    at: Instant,
    /// Messages dropped lately, less what time has forgiven: see
    /// [`FLOOD_STRIKES`]. Not cleared by a message that gets through, or a
    /// steady flood, let through a message every refill, would never add
    /// up to anything.
    strikes: f64,
}

impl Allowance {
    fn new() -> Allowance {
        Allowance {
            level: BURST,
            at: Instant::now(),
            strikes: 0.0,
        }
    }

    /// Spend one message's worth, if there is one.
    fn take(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.at).as_secs_f64();
        // The clock refills up to the burst; what was earned by being sent
        // lines may stand above it, up to the ceiling.
        let refilled = self.level + elapsed * PER_SECOND;
        self.level = refilled.min(self.level.max(BURST));
        self.strikes = (self.strikes - elapsed * FORGIVE_PER_SECOND).max(0.0);
        self.at = now;
        if self.level >= 1.0 {
            self.level -= 1.0;
            true
        } else {
            self.strikes += 1.0;
            false
        }
    }

    /// Dropped so much lately that it is flooding.
    fn flooding(&self) -> bool {
        self.strikes > FLOOD_STRIKES
    }

    /// A line went out to the bot: it may answer it.
    fn credit(&mut self) {
        self.level = (self.level + PER_LINE).min(CEILING);
    }
}

/// Read and throw away what the bot is still sending, for a moment, before
/// its connection closes. A socket closed with unread input is reset, and a
/// reset can throw away the error on its way out: a bot that floods would
/// never learn why it was let go.
fn hang_up(reader: &mut BufReader<TcpStream>) {
    let until = Instant::now() + Duration::from_secs(1);
    let _ = reader
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(50)));
    let mut sink = [0u8; 4096];
    while Instant::now() < until {
        match reader.read(&mut sink) {
            Ok(0) => return,
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return,
        }
    }
}

fn read_loop(reader: &mut BufReader<TcpStream>, shared: &Arc<Shared>, id: BotId, serial: u64) {
    let mut buf = Vec::new();
    loop {
        match read_line(reader, &mut buf, protocol::MAX_LINE, None) {
            Ok(true) => {}
            Ok(false) => return,
            Err(e) => {
                let mut registry = lock(&shared.registry);
                if e.kind() == std::io::ErrorKind::InvalidData {
                    send(
                        &mut registry,
                        id,
                        &json!({"type": "error", "fatal": true, "message": "a line longer than 64 KiB"}),
                    );
                    drop(registry);
                    hang_up(reader);
                }
                return;
            }
        }
        let at = Instant::now();
        {
            let mut registry = lock(&shared.registry);
            // A connection that has been replaced by a reconnect reads no
            // more.
            if registry.bots[id].conn.as_ref().map(|c| c.serial) != Some(serial) {
                return;
            }
            let allowance = &mut registry.bots[id].allowance;
            if !allowance.take() {
                if allowance.flooding() {
                    send(
                        &mut registry,
                        id,
                        &json!({"type": "error", "fatal": true, "message": "flooding"}),
                    );
                    drop(registry);
                    hang_up(reader);
                    return;
                }
                continue;
            }
        }
        let line = String::from_utf8_lossy(&buf);
        handle(shared, id, protocol::parse_incoming(&line), at);
    }
}

fn handle(shared: &Arc<Shared>, id: BotId, msg: Incoming, at: Instant) {
    let mut registry = lock(&shared.registry);
    match msg {
        Incoming::Reply(reply) => match seat_in(&registry, id, reply.game) {
            Some(SeatRoute { tx, seat, .. }) => {
                let _ = tx.send(GameMsg::Reply { seat, reply, at });
            }
            None => {
                let game = reply.game;
                send(
                    &mut registry,
                    id,
                    &json!({"type": "error", "message": format!("you have no seat in game {game}")}),
                );
            }
        },
        Incoming::Ready { game } => {
            if let Some(SeatRoute { tx, seat, .. }) = seat_in(&registry, id, game) {
                let _ = tx.send(GameMsg::Ready { seat });
            }
        }
        Incoming::Garbled { game, why } => {
            let routes: Vec<(Sender<GameMsg>, PlayerId)> = match game {
                Some(game) => seat_in(&registry, id, game)
                    .map(|route| (route.tx, route.seat))
                    .into_iter()
                    .collect(),
                None => routes_of(&registry, id),
            };
            if routes.is_empty() {
                send(&mut registry, id, &json!({"type": "error", "message": why}));
            }
            for (tx, seat) in routes {
                let _ = tx.send(GameMsg::Garbled {
                    seat,
                    why: why.clone(),
                    at,
                });
            }
        }
        Incoming::Pong { id: n } => {
            let bot = &mut registry.bots[id];
            if let Some(sent) = bot.pings.remove(&n) {
                bot.rtts.push(at.duration_since(sent));
                if bot.rtts.len() > 64 {
                    bot.rtts.remove(0);
                }
            }
        }
        Incoming::BadRequest { game, why } => {
            let mut error = json!({"type": "error", "message": why});
            if let Some(game) = game {
                error["game"] = json!(game);
            }
            send(&mut registry, id, &error);
        }
        Incoming::Replay { id: replay } => {
            let text = match registry.replays.get(&replay) {
                Some((players, text)) if players.contains(&id) => Some(Arc::clone(text)),
                _ => None,
            };
            // A replay is a few hundred kilobytes to write out, done with
            // the registry let go: every game's every tick goes through it.
            drop(registry);
            let answer = line_of(&match text {
                Some(text) => json!({"type": "replay", "id": replay, "text": &*text}),
                None => {
                    json!({"type": "error", "message": format!("no replay {replay:?} of a game you played")})
                }
            });
            send_line(&mut lock(&shared.registry), id, answer, false);
        }
        Incoming::Register(_) => {
            send(
                &mut registry,
                id,
                &json!({"type": "error", "message": "already registered"}),
            );
        }
        Incoming::Simulate { game, ticks, plan } => {
            let Some(SeatRoute { seat, view, .. }) = seat_in(&registry, id, game) else {
                send(
                    &mut registry,
                    id,
                    &json!({"type": "error", "message": format!("you have no seat in game {game}")}),
                );
                return;
            };
            // The lookahead runs on this connection's own thread, with the
            // registry and the game's view both let go: it is paid for
            // and copied under the view's lock, and run without it. A
            // bot's thinking is never a hitch anyone else feels, its own
            // game included.
            drop(registry);
            let charged = match lock(&view).as_mut() {
                Some(view) => lookahead::charge(view, game, seat, ticks),
                None => Err(json!({"type": "error", "message": "the game has not started"})),
            };
            let answer = line_of(&match charged {
                Ok(charged) => charged.answer(game, seat, &plan),
                Err(refused) => refused,
            });
            send_line(&mut lock(&shared.registry), id, answer, false);
        }
    }
}

fn disconnect(shared: &Arc<Shared>, id: BotId, serial: u64) {
    let mut registry = lock(&shared.registry);
    let bot = &mut registry.bots[id];
    // Only this connection's own going counts: a reconnect has already put
    // a newer one in its place.
    if bot.conn.as_ref().map(|c| c.serial) != Some(serial) {
        return;
    }
    // Dropping the sender lets the writer finish what it has and close.
    bot.conn = None;
    let routes = routes_of(&registry, id);
    drop(registry);
    for (tx, seat) in routes {
        let _ = tx.send(GameMsg::Dropped { seat });
    }
    emit(shared, Event::Dropped(id));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A registration line dribbled in a byte at a time is held to one
    /// deadline for the whole line: each byte does not buy it more time.
    #[test]
    fn a_line_dribbled_in_is_held_to_one_deadline() {
        let socket = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).expect("bind");
        let mut dribbler = TcpStream::connect(socket.local_addr().expect("addr")).expect("connect");
        let (served, _) = socket.accept().expect("accept");
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writer = {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    if dribbler.write_all(b"x").is_err() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            })
        };
        let mut reader = BufReader::new(served);
        let mut buf = Vec::new();
        let started = Instant::now();
        let read = read_line(
            &mut reader,
            &mut buf,
            protocol::MAX_LINE,
            Some(started + Duration::from_millis(300)),
        );
        let took = started.elapsed();
        stop.store(true, Ordering::SeqCst);
        let _ = writer.join();
        // Out of time between reads, or in the middle of the last one,
        // which a socket reports as it would a non-blocking read.
        assert!(
            read.is_err_and(|e| matches!(
                e.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            )),
            "no line, and no error"
        );
        assert!(!buf.is_empty(), "the bytes did arrive");
        assert!(took < Duration::from_secs(2), "held for {took:?}");
    }
}
