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
use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream};
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
/// Lines queued for a bot that is not reading before it is dropped. A
/// tick message is about 10 KB, so this is a couple of megabytes.
const QUEUE: usize = 256;
/// The flood valve. A bot earns the right to send by being sent things:
/// every line the listener writes it is worth `PER_LINE` messages back (a
/// reply and a few lookaheads), so a fast bot in a fast-forward game,
/// answering thousands of ticks a second, is never mistaken for a flood.
/// On top of that it may send `PER_SECOND` a second of its own accord, and
/// save up to `BURST`.
const BURST: f64 = 400.0;
const PER_SECOND: f64 = 50.0;
const PER_LINE: f64 = 6.0;
/// The most a bot can have saved up, earned and refilled together.
const CEILING: f64 = 2000.0;
/// Messages dropped for flooding before the connection goes.
const FLOOD_STRIKES: u32 = 400;

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
    /// It came back with its token.
    Back {
        seat: PlayerId,
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
pub struct GameLink {
    pub rx: Receiver<GameMsg>,
    pub view: Arc<Mutex<Option<View>>>,
    game: u32,
    shared: Arc<Shared>,
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
}

struct Bot {
    info: BotInfo,
    token: Token,
    conn: Option<Conn>,
    /// Round trips measured by ping, newest last.
    rtts: Vec<Duration>,
    pings: HashMap<u64, Instant>,
    allowance: Allowance,
}

#[derive(Default)]
struct Registry {
    bots: Vec<Bot>,
    routes: HashMap<u32, Route>,
    /// Finished games' replays, and who may fetch them.
    replays: HashMap<String, (Vec<BotId>, Arc<str>)>,
    wrong_keys: HashMap<IpAddr, (u32, Option<Instant>)>,
    next_serial: u64,
    next_ping: u64,
}

struct Shared {
    registry: Mutex<Registry>,
    config: Mutex<Config>,
    events: Mutex<Sender<Event>>,
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

    pub fn connected(&self, id: BotId) -> bool {
        lock(&self.shared.registry)
            .bots
            .get(id)
            .is_some_and(|b| b.conn.is_some())
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

    /// Write one message to a bot. False if it is not connected, or was
    /// just dropped for not reading.
    pub fn send(&self, id: BotId, msg: &Value) -> bool {
        send(&mut lock(&self.shared.registry), id, msg)
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
            rx,
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

fn accept_loop(socket: &TcpListener, shared: &Arc<Shared>) {
    for stream in socket.incoming() {
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
fn read_line(
    reader: &mut BufReader<TcpStream>,
    buf: &mut Vec<u8>,
    cap: usize,
) -> std::io::Result<bool> {
    buf.clear();
    loop {
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
    if locked_out(shared, peer.ip()) {
        refuse(
            stream,
            shared,
            peer,
            "too many wrong keys from this address; try again in a minute",
        );
        return;
    }
    let _ = stream.set_read_timeout(Some(REGISTER_WITHIN));
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let mut buf = Vec::new();
    let register = match read_line(&mut reader, &mut buf, protocol::MAX_LINE) {
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
    let _ = stream.set_read_timeout(None);
    let (id, serial) = match register_bot(shared, &stream, peer, register) {
        Ok(ok) => ok,
        Err(why) => {
            refuse(stream, shared, peer, &why);
            return;
        }
    };
    read_loop(&mut reader, shared, id, serial);
    disconnect(shared, id, serial);
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
    if let Some(token) = &register.token {
        let token = Token::from_wire(token);
        let mut registry = lock(&shared.registry);
        let Some(id) = registry.bots.iter().position(|b| b.token == token) else {
            drop(registry);
            wrong_key(shared, peer.ip());
            return Err("that token is not one this listener issued".to_string());
        };
        let serial = attach(&mut registry, id, writer);
        let msg = json!({
            "type": "registered", "name": registry.bots[id].info.name,
            "token": registry.bots[id].token.reveal(), "resumed": true,
        });
        send(&mut registry, id, &msg);
        let routes: Vec<(Sender<GameMsg>, PlayerId)> = routes_of(&registry, id);
        drop(registry);
        for (tx, seat) in routes {
            let _ = tx.send(GameMsg::Back { seat });
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
        .any(|b| b.info.name.eq_ignore_ascii_case(&register.name))
    {
        return Err(format!("the name {:?} is taken here", register.name));
    }
    let (bound_owner, slot) = match (&mut config.admission, invite) {
        (Admission::Keys(invites), Some(i)) => (invites[i].owner.clone(), invites[i].slot),
        _ => (None, None),
    };
    let owner_declared = bound_owner.is_none();
    let owner = bound_owner.or(register.owner);
    if let (Some(cap), Some(owner)) = (config.per_owner, &owner) {
        let theirs = registry
            .bots
            .iter()
            .filter(|b| {
                b.info
                    .owner
                    .as_deref()
                    .is_some_and(|o| o.eq_ignore_ascii_case(owner))
            })
            .count();
        if theirs >= cap {
            return Err(format!(
                "{owner} already has {theirs} bot(s) entered, the most allowed"
            ));
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
    let _ = std::thread::Builder::new()
        .name("bot-write".into())
        .spawn(move || write_loop(writer, &rx));
    if let Some(old) = registry.bots[id].conn.replace(Conn { serial, tx, stream }) {
        let _ = old.stream.shutdown(Shutdown::Both);
    }
    serial
}

/// Write a connection's lines until its sender goes, then close it. The
/// close is here, after the queue is drained, so the last thing said to a
/// bot (a fatal error, say) reaches it before the socket shuts.
fn write_loop(mut stream: TcpStream, rx: &Receiver<Arc<str>>) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    for line in rx {
        if stream.write_all(line.as_bytes()).is_err() {
            break;
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
}

fn send(registry: &mut Registry, id: BotId, msg: &Value) -> bool {
    let Some(bot) = registry.bots.get_mut(id) else {
        return false;
    };
    let Some(conn) = &bot.conn else {
        return false;
    };
    let line: Arc<str> = format!("{msg}\n").into();
    match conn.tx.try_send(line) {
        Ok(()) => {
            bot.allowance.credit();
            true
        }
        // A bot that has stopped reading is dropped rather than waited for.
        Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
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
    strikes: u32,
}

impl Allowance {
    fn new() -> Allowance {
        Allowance {
            level: BURST,
            at: Instant::now(),
            strikes: 0,
        }
    }

    /// Spend one message's worth, if there is one.
    fn take(&mut self) -> bool {
        let now = Instant::now();
        // The clock refills up to the burst; what was earned by being sent
        // lines may stand above it, up to the ceiling.
        let refilled = self.level + now.duration_since(self.at).as_secs_f64() * PER_SECOND;
        self.level = refilled.min(self.level.max(BURST));
        self.at = now;
        if self.level >= 1.0 {
            self.level -= 1.0;
            self.strikes = 0;
            true
        } else {
            self.strikes += 1;
            false
        }
    }

    /// A line went out to the bot: it may answer it.
    fn credit(&mut self) {
        self.level = (self.level + PER_LINE).min(CEILING);
    }
}

fn read_loop(reader: &mut BufReader<TcpStream>, shared: &Arc<Shared>, id: BotId, serial: u64) {
    let mut buf = Vec::new();
    loop {
        match read_line(reader, &mut buf, protocol::MAX_LINE) {
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
                if allowance.strikes > FLOOD_STRIKES {
                    send(
                        &mut registry,
                        id,
                        &json!({"type": "error", "fatal": true, "message": "flooding"}),
                    );
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
        Incoming::Replay { id: replay } => {
            let answer = match registry.replays.get(&replay) {
                Some((players, text)) if players.contains(&id) => {
                    json!({"type": "replay", "id": replay, "text": &**text})
                }
                _ => {
                    json!({"type": "error", "message": format!("no replay {replay:?} of a game you played")})
                }
            };
            send(&mut registry, id, &answer);
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
            // registry let go: a bot's thinking is never a hitch anyone
            // else feels.
            drop(registry);
            let answer = {
                let mut view = lock(&view);
                match view.as_mut() {
                    Some(view) => lookahead::answer(view, game, seat, ticks, &plan),
                    None => json!({"type": "error", "message": "the game has not started"}),
                }
            };
            let mut registry = lock(&shared.registry);
            send(&mut registry, id, &answer);
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
