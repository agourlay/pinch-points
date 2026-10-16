//! The connections themselves: accepting, reading and writing lines, and
//! routing what a registered bot says to the games it sits in.

use super::admit::{Pending, locked_out, register_bot};
use super::{BotId, Conn, Event, GameMsg, Registry, Shared, View, emit, lock};
use crate::bots::lookahead;
use crate::bots::protocol::{self, Incoming};
use crate::sim::PlayerId;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a fresh connection has to register before it is closed.
const REGISTER_WITHIN: Duration = Duration::from_secs(10);
/// Lines queued for a bot that is not reading before it is dropped.
const QUEUE: usize = 256;
/// Bytes queued for a bot that is not reading before it is dropped: a
/// couple of megabytes, whatever the lines are (a tick is about 10 KB, a
/// replay a few hundred). A single line bigger than this still goes out
/// to a bot with nothing else waiting.
const QUEUE_BYTES: usize = 2 * 1024 * 1024;

pub(super) fn accept_loop(socket: &TcpListener, shared: &Arc<Shared>) {
    let me = socket.local_addr().ok();
    for stream in socket.incoming() {
        if shared.closed.load(Ordering::SeqCst) {
            return;
        }
        // Stopped: the address is taken off the list as this loop ends,
        // so a socket bound to the same port later (the OS hands a freed
        // port out again) does not take its first bot for this wake-up.
        if let Some(me) = me {
            let mut stopped = lock(&shared.stopped);
            if let Some(at) = stopped.iter().position(|a| *a == me) {
                stopped.remove(at);
                return;
            }
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

/// Write a refusal and close. The message says why, never with a secret
/// in it.
fn refuse(mut stream: TcpStream, shared: &Shared, peer: SocketAddr, why: &str) {
    let line = format!("{}\n", protocol::fatal(why));
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

/// A connection's two handles: the one kept to shut it, and the one its
/// writer thread writes on. Both taken before anything is registered, so
/// a socket that cannot be cloned is refused, never a bot registered with
/// no connection, told nothing, its name held.
pub(super) struct Wire {
    stream: TcpStream,
    writer: TcpStream,
}

impl Wire {
    pub(super) fn of(stream: &TcpStream) -> std::io::Result<Wire> {
        Ok(Wire {
            stream: stream.try_clone()?,
            writer: stream.try_clone()?,
        })
    }
}

/// Give bot `id` this connection, closing any older one it had.
pub(super) fn attach(registry: &mut Registry, id: BotId, wire: Wire) -> u64 {
    let Wire { stream, writer } = wire;
    registry.next_serial += 1;
    let serial = registry.next_serial;
    let (tx, rx) = mpsc::sync_channel::<Arc<str>>(QUEUE);
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
pub(super) fn line_of(msg: &Value) -> Arc<str> {
    format!("{msg}\n").into()
}

/// Answer a bot: an error, a replay, a lookahead. Earns it nothing.
pub(super) fn send(registry: &mut Registry, id: BotId, msg: &Value) -> bool {
    send_line(registry, id, line_of(msg), false)
}

/// Queue a line for bot `id`. `earns` for a game's own lines, which the
/// bot is owed the right to answer (see [`PER_LINE`](super::allowance::PER_LINE)).
pub(super) fn send_line(registry: &mut Registry, id: BotId, line: Arc<str>, earns: bool) -> bool {
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

pub(super) fn ping(registry: &mut Registry, id: BotId) {
    registry.next_ping += 1;
    let n = registry.next_ping;
    if send(registry, id, &json!({"type": "ping", "id": n})) {
        registry.bots[id].pings.insert(n, Instant::now());
    }
}

/// The routes bot `id` has a seat in, with the seat.
pub(super) fn routes_of(registry: &Registry, id: BotId) -> Vec<(Sender<GameMsg>, PlayerId)> {
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
                        &protocol::fatal("a line longer than 64 KiB"),
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
                    send(&mut registry, id, &protocol::fatal("flooding"));
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
                    &protocol::error(&format!("you have no seat in game {game}"), None),
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
                send(&mut registry, id, &protocol::error(&why, None));
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
            send(&mut registry, id, &protocol::error(&why, game));
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
                    protocol::error(&format!("no replay {replay:?} of a game you played"), None)
                }
            });
            send_line(&mut lock(&shared.registry), id, answer, false);
        }
        Incoming::Register(_) => {
            send(
                &mut registry,
                id,
                &protocol::error("already registered", None),
            );
        }
        Incoming::Simulate { game, ticks, plan } => {
            let Some(SeatRoute { seat, view, .. }) = seat_in(&registry, id, game) else {
                send(
                    &mut registry,
                    id,
                    &protocol::error(&format!("you have no seat in game {game}"), None),
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
                None => Err(protocol::error("the game has not started", None)),
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
