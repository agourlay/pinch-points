//! The listener and a game, end to end over real sockets: a test client
//! registers, plays, drops, comes back, and misbehaves on purpose.

use super::connstr::Key;
use super::game::{self, GameSpec, Seat, Sinks};
use super::listener::{Admission, Config, Event, Invite, Listener};
use super::protocol::Clock;
use crate::sim::{Board, BotLevel, CapPolicy, CrabKind, Direction, Handedness, TileKind};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::Receiver;
use std::time::Duration;

/// A bot written for the tests: a socket and a line reader.
struct Client {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

impl Client {
    fn connect(addr: SocketAddr) -> Client {
        let stream = TcpStream::connect(addr).expect("connect");
        stream.set_nodelay(true).expect("nodelay");
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .expect("timeout");
        let reader = BufReader::new(stream.try_clone().expect("clone"));
        Client { stream, reader }
    }

    /// Write a message. A listener may already have closed a connection
    /// it refused, so a failed write is the test's to notice by what
    /// comes back, not a panic here.
    fn send(&mut self, msg: &Value) {
        let _ = writeln!(self.stream, "{msg}");
    }

    fn raw(&mut self, line: &str) {
        writeln!(self.stream, "{line}").expect("write");
    }

    /// The next message that is not a ping (pings are answered).
    fn next(&mut self) -> Option<Value> {
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line).ok()? == 0 {
                return None;
            }
            let msg: Value = serde_json::from_str(&line).expect("the listener writes JSON");
            if msg["type"] == "ping" {
                let id = msg["id"].clone();
                self.send(&json!({"type": "pong", "id": id}));
                continue;
            }
            return Some(msg);
        }
    }

    fn register(addr: SocketAddr, name: &str, key: Option<&Key>) -> (Client, Value) {
        let mut client = Client::connect(addr);
        let mut msg = json!({"type": "register", "protocol": 1, "name": name, "version": "t"});
        if let Some(key) = key {
            msg["key"] = json!(key.to_string());
        }
        client.send(&msg);
        let answer = client.next().expect("an answer");
        (client, answer)
    }
}

fn listen(admission: Admission) -> (Listener, Receiver<Event>) {
    Listener::bind(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        Config::new(admission),
    )
    .expect("bind")
}

fn invite(key: &Key, uses: Option<u32>) -> Invite {
    Invite {
        key: key.clone(),
        uses,
        owner: None,
        slot: None,
    }
}

/// A short round on a small beach: a crab walking straight at seat 0's
/// castle, and seat 1 across the way.
fn little_beach(ticks: u32) -> Board {
    let mut board = Board::new(12, 3, 9);
    board.set_tile(11, 1, TileKind::Castle(0));
    board.set_tile(0, 1, TileKind::Castle(1));
    // Two tiles from home and too far from seat 1 for the AI to reach for.
    board.spawn_crab(9, 1, Direction::Right, Handedness::Left, CrabKind::Giant);
    board.set_round_length(Some(ticks));
    board.set_signpost_rule(3, CapPolicy::Evict);
    board
}

fn spec(board: Board, seats: Vec<Seat>, deadline_ms: u32) -> GameSpec {
    spec_on(board, seats, deadline_ms, false)
}

fn spec_on(board: Board, seats: Vec<Seat>, deadline_ms: u32, live: bool) -> GameSpec {
    GameSpec {
        game: 7,
        replay_id: "t-g7".into(),
        board,
        names: seats
            .iter()
            .enumerate()
            .map(|(i, _)| format!("seat{i}"))
            .collect(),
        seats,
        clock: Clock {
            live,
            deadline_ms,
            input_delay: 0,
        },
        fair_cursor: false,
        forfeit_after: Duration::from_millis(300),
        ready_within: Duration::from_secs(5),
    }
}

fn play(listener: &Listener, spec: GameSpec) -> (game::GameResult, String) {
    let mut log = Vec::new();
    let mut console = |_: String| {};
    let result = {
        let mut sinks = Sinks {
            log: &mut log,
            trace: None,
            feed: None,
            console: &mut console,
            echo_notes: false,
        };
        game::play(listener, spec, &mut sinks)
    };
    (result, String::from_utf8(log).expect("utf8"))
}

#[test]
fn a_bot_registers_plays_and_is_told_how_it_went() {
    let key = Key::draw();
    let (listener, events) = listen(Admission::Keys(vec![invite(&key, Some(1))]));
    let addr = listener.local_addr();
    let (mut bot, answer) = Client::register(addr, "Plucky", Some(&key));
    assert_eq!(answer["type"], "registered");
    assert!(answer["token"].as_str().is_some_and(|t| t.len() >= 20));
    let Ok(Event::Registered(id)) = events.recv_timeout(Duration::from_secs(5)) else {
        panic!("no registration event");
    };
    let player = std::thread::spawn(move || {
        let mut ticks = 0;
        let mut refused = None;
        while let Some(msg) = bot.next() {
            match msg["type"].as_str() {
                Some("hello") => {
                    assert_eq!(msg["seat"], 0);
                    assert_eq!(msg["kinds"], json!(["bot", "ai"]));
                    bot.send(&json!({"type": "ready", "game": msg["game"]}));
                }
                Some("tick") => {
                    ticks += 1;
                    let t = msg["tick"].clone();
                    if refused.is_none() && msg["you"]["last"]["accepted"] == false {
                        refused = Some(msg["you"]["last"]["reason"].clone());
                    }
                    // First a placement on a castle, which is refused, then
                    // nothing.
                    let act = if t == 0 {
                        json!({"game": 7, "tick": t, "act": "place", "x": 0, "y": 1, "dir": "up", "note": "hi"})
                    } else {
                        json!({"game": 7, "tick": t, "act": "none"})
                    };
                    bot.send(&act);
                }
                Some("end") => return (ticks, refused, msg),
                _ => {}
            }
        }
        panic!("the connection closed before the end");
    });
    let (result, log) = play(
        &listener,
        spec(
            little_beach(120),
            vec![Seat::Bot(id), Seat::Ai(BotLevel::Easy)],
            1000,
        ),
    );
    let (ticks, refused, end) = player.join().expect("player");
    assert_eq!(ticks, 120, "one tick message per tick of the round");
    assert_eq!(refused, Some(json!("castle")));
    assert_eq!(result.seats[0].rejected, 1);
    assert_eq!(result.seats[0].banked, 1, "the giant walked home");
    assert_eq!(end["scores"][0], 10);
    assert_eq!(end["placing"], 1);
    assert_eq!(end["replay"], "t-g7");
    assert!(log.contains("note: hi"), "{log}");
    assert!(log.contains("refused: castle"), "{log}");
    assert_eq!(result.replay.inputs.len(), 120);
}

#[test]
fn a_second_bot_on_a_single_use_key_is_turned_away() {
    let key = Key::draw();
    let (listener, _events) = listen(Admission::Keys(vec![invite(&key, Some(1))]));
    let addr = listener.local_addr();
    let (_first, answer) = Client::register(addr, "One", Some(&key));
    assert_eq!(answer["type"], "registered");
    let (_second, answer) = Client::register(addr, "Two", Some(&key));
    assert_eq!(answer["type"], "error");
    assert_eq!(answer["fatal"], true);
}

#[test]
fn a_taken_name_does_not_spend_the_key() {
    let key = Key::draw();
    let (listener, _events) = listen(Admission::Keys(vec![invite(&key, Some(2))]));
    let addr = listener.local_addr();
    let (_a, answer) = Client::register(addr, "Same", Some(&key));
    assert_eq!(answer["type"], "registered");
    let (_b, answer) = Client::register(addr, "same", Some(&key));
    assert_eq!(
        answer["type"], "error",
        "names are unique, whatever the case"
    );
    let (_c, answer) = Client::register(addr, "Other", Some(&key));
    assert_eq!(
        answer["type"], "registered",
        "the refused one did not use the key up"
    );
}

#[test]
fn three_wrong_keys_turn_the_address_away_and_not_the_key() {
    let key = Key::draw();
    let (listener, _events) = listen(Admission::Keys(vec![invite(&key, None)]));
    let addr = listener.local_addr();
    for _ in 0..3 {
        let (_c, answer) = Client::register(addr, "Guess", Some(&Key::draw()));
        assert_eq!(answer["type"], "error");
    }
    // The right key from the same address is refused for now...
    let (_c, answer) = Client::register(addr, "Honest", Some(&key));
    assert_eq!(answer["type"], "error");
    assert!(
        answer["message"]
            .as_str()
            .is_some_and(|m| m.contains("minute"))
    );
}

#[test]
fn a_protocol_this_build_does_not_speak_is_refused_by_number() {
    let (listener, _events) = listen(Admission::Open);
    let mut client = Client::connect(listener.local_addr());
    client.send(&json!({"type": "register", "protocol": 2, "name": "Future"}));
    let answer = client.next().expect("answer");
    assert_eq!(answer["type"], "error");
    assert!(
        answer["message"]
            .as_str()
            .is_some_and(|m| m.contains("protocol 1"))
    );
}

#[test]
fn a_line_too_long_closes_the_connection() {
    let (listener, _events) = listen(Admission::Open);
    let (mut bot, _) = Client::register(listener.local_addr(), "Wordy", None);
    let long = format!("{{\"note\": \"{}\"}}", "x".repeat(70 * 1024));
    // The listener may close before the whole line is written.
    let _ = writeln!(bot.stream, "{long}");
    let mut saw_error = false;
    while let Some(msg) = bot.next() {
        saw_error |= msg["type"] == "error" && msg["fatal"] == true;
    }
    assert!(saw_error);
}

#[test]
fn a_bot_that_comes_back_with_its_token_plays_on() {
    let (listener, events) = listen(Admission::Open);
    let addr = listener.local_addr();
    let (mut bot, answer) = Client::register(addr, "Phoenix", None);
    let token = answer["token"].as_str().expect("token").to_string();
    let Ok(Event::Registered(id)) = events.recv_timeout(Duration::from_secs(5)) else {
        panic!("no registration event");
    };
    let player = std::thread::spawn(move || {
        // Play a few ticks, hang up, and come back with the token.
        let mut seen = 0;
        while let Some(msg) = bot.next() {
            if msg["type"] == "hello" {
                bot.send(&json!({"type": "ready", "game": msg["game"]}));
            }
            if msg["type"] == "tick" {
                seen += 1;
                bot.send(&json!({"game": 7, "tick": msg["tick"], "act": "none"}));
                if seen == 5 {
                    break;
                }
            }
        }
        drop(bot);
        std::thread::sleep(Duration::from_millis(100));
        let mut back = Client::connect(addr);
        back.send(&json!({"type": "register", "protocol": 1, "token": token}));
        let answer = back.next().expect("registered again");
        assert_eq!(answer["type"], "registered");
        assert_eq!(answer["name"], "Phoenix");
        let hello = back.next().expect("hello again");
        assert_eq!(hello["type"], "hello");
        assert_eq!(hello["resumed"], true);
        let mut after = 0;
        while let Some(msg) = back.next() {
            if msg["type"] == "tick" {
                after += 1;
                back.send(&json!({"game": 7, "tick": msg["tick"], "act": "none"}));
            }
            if msg["type"] == "end" {
                return after;
            }
        }
        panic!("no end");
    });
    // On the live clock: in fast-forward a table whose only bot has gone
    // has nobody to wait for, and runs to the tide before it is back.
    let (result, log) = play(
        &listener,
        spec_on(
            little_beach(150),
            vec![Seat::Bot(id), Seat::Ai(BotLevel::Easy)],
            33,
            true,
        ),
    );
    let after = player.join().expect("player");
    assert!(
        after > 100,
        "it played on after coming back ({after} ticks)"
    );
    assert!(!result.seats[0].forfeit);
    assert!(
        log.contains("connection dropped") && log.contains("reconnected"),
        "{log}"
    );
}

#[test]
fn a_bot_that_never_comes_forfeits_and_places_last() {
    let (listener, events) = listen(Admission::Open);
    let (bot, _) = Client::register(listener.local_addr(), "Ghost", None);
    let Ok(Event::Registered(id)) = events.recv_timeout(Duration::from_secs(5)) else {
        panic!("no registration event");
    };
    drop(bot);
    while listener.connected(id) {
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut board = little_beach(60);
    // The ghost's castle would bank the giant on its own.
    board.set_score(1, 0);
    let (result, _) = play(
        &listener,
        spec(board, vec![Seat::Bot(id), Seat::Ai(BotLevel::Easy)], 50),
    );
    assert!(result.seats[0].forfeit);
    assert_eq!(result.seats[0].placing, 2);
    assert_eq!(result.seats[1].placing, 1);
}

#[test]
fn a_lookahead_is_answered_off_the_game_and_a_replay_fetched_after_it() {
    let (listener, events) = listen(Admission::Open);
    let addr = listener.local_addr();
    let (mut bot, _) = Client::register(addr, "Seer", None);
    let Ok(Event::Registered(id)) = events.recv_timeout(Duration::from_secs(5)) else {
        panic!("no registration event");
    };
    let player = std::thread::spawn(move || {
        let mut lookahead = None;
        while let Some(msg) = bot.next() {
            match msg["type"].as_str() {
                Some("hello") => bot.send(&json!({"type": "ready", "game": 7})),
                Some("tick") if msg["tick"] == 0 => {
                    bot.send(&json!({"type": "simulate", "game": 7, "ticks": 100, "plan": []}));
                    let answer = bot.next().expect("simulated");
                    lookahead = Some(answer);
                    bot.send(&json!({"game": 7, "tick": 0, "act": "none"}));
                }
                Some("tick") => bot.send(&json!({"game": 7, "tick": msg["tick"], "act": "none"})),
                Some("end") => {
                    bot.send(&json!({"type": "replay", "id": "t-g7"}));
                    let replay = bot.next().expect("replay");
                    return (lookahead, replay);
                }
                _ => {}
            }
        }
        panic!("no end");
    });
    play(
        &listener,
        spec(
            little_beach(80),
            vec![Seat::Bot(id), Seat::Ai(BotLevel::Easy)],
            1000,
        ),
    );
    let (lookahead, replay) = player.join().expect("player");
    let lookahead = lookahead.expect("a lookahead");
    assert_eq!(lookahead["type"], "simulated");
    assert_eq!(lookahead["ticks"], 80, "it stops where the round does");
    assert!(
        lookahead["events"]
            .as_array()
            .is_some_and(|e| e.iter().any(|e| e["what"] == "banked" && e["owner"] == 0)),
        "{lookahead}"
    );
    assert_eq!(replay["type"], "replay");
    let text = replay["text"].as_str().expect("text");
    let parsed = crate::sim::Replay::parse(text).expect("a replay this build reads");
    assert_eq!(parsed.inputs.len(), 80);
    assert_eq!(parsed.names[0], "seat0");
    assert_eq!(parsed.kinds[0], crate::sim::SeatKind::Bot);
}

#[test]
fn a_slow_bot_is_late_but_never_buried_under_ticks() {
    let (listener, events) = listen(Admission::Open);
    let (mut bot, _) = Client::register(listener.local_addr(), "Sloth", None);
    let Ok(Event::Registered(id)) = events.recv_timeout(Duration::from_secs(5)) else {
        panic!("no registration event");
    };
    let player = std::thread::spawn(move || {
        let mut ticks = 0;
        while let Some(msg) = bot.next() {
            match msg["type"].as_str() {
                Some("hello") => bot.send(&json!({"type": "ready", "game": 7})),
                Some("tick") => {
                    ticks += 1;
                    std::thread::sleep(Duration::from_millis(12));
                    bot.send(&json!({"game": 7, "tick": msg["tick"], "act": "none"}));
                }
                Some("end") => return ticks,
                _ => {}
            }
        }
        panic!("no end");
    });
    let (result, _) = play(
        &listener,
        spec(
            little_beach(90),
            vec![Seat::Bot(id), Seat::Ai(BotLevel::Easy)],
            5,
        ),
    );
    let ticks = player.join().expect("player");
    let seat = &result.seats[0];
    assert!(seat.late > 0, "a 12 ms bot on a 5 ms deadline is late");
    assert!(
        ticks < 60,
        "and is not sent ticks it has not caught up with ({ticks})"
    );
    assert_eq!(seat.sent as usize, ticks);
}

#[test]
fn garbage_counts_as_none_and_is_logged() {
    let (listener, events) = listen(Admission::Open);
    let (mut bot, _) = Client::register(listener.local_addr(), "Babbler", None);
    let Ok(Event::Registered(id)) = events.recv_timeout(Duration::from_secs(5)) else {
        panic!("no registration event");
    };
    let player = std::thread::spawn(move || {
        while let Some(msg) = bot.next() {
            match msg["type"].as_str() {
                Some("hello") => bot.send(&json!({"type": "ready", "game": 7})),
                Some("tick") if msg["tick"] == 3 => bot.raw("{this is not json"),
                Some("tick") if msg["tick"] == 4 => {
                    bot.send(&json!({"game": 7, "tick": 4, "act": "teleport", "x": 1, "y": 1}));
                }
                Some("tick") => bot.send(&json!({"game": 7, "tick": msg["tick"], "act": "none"})),
                Some("end") => return,
                _ => {}
            }
        }
    });
    let (result, log) = play(
        &listener,
        spec(
            little_beach(10),
            vec![Seat::Bot(id), Seat::Ai(BotLevel::Easy)],
            1000,
        ),
    );
    player.join().expect("player");
    assert!(log.contains("garbled"), "{log}");
    assert!(log.contains("teleport"), "{log}");
    assert_eq!(result.seats[0].rejected, 0);
}

#[test]
fn a_per_author_key_names_the_owner_and_one_owner_enters_once() {
    let shared = Key::draw();
    let ana = Key::draw();
    let mut config = Config::new(Admission::Keys(vec![invite(&shared, None)]));
    config.per_owner = Some(1);
    let (listener, events) =
        Listener::bind(SocketAddr::from(([127, 0, 0, 1], 0)), config).expect("bind");
    listener.invite(Invite {
        key: ana.clone(),
        uses: None,
        owner: Some("Ana".into()),
        slot: None,
    });
    let addr = listener.local_addr();
    // Whatever it claims, a bot on Ana's key is Ana's.
    let mut client = Client::connect(addr);
    client.send(&json!({
        "type": "register", "protocol": 1, "name": "Sly", "owner": "Somebody Else",
        "key": ana.to_string(),
    }));
    assert_eq!(client.next().expect("answer")["type"], "registered");
    let Ok(Event::Registered(id)) = events.recv_timeout(Duration::from_secs(5)) else {
        panic!("no registration event");
    };
    let info = listener.bot(id).expect("info");
    assert_eq!(info.owner.as_deref(), Some("Ana"));
    assert!(!info.owner_declared);
    // A second bot declaring Ana on the shared key is one too many.
    let mut second = Client::connect(addr);
    second.send(&json!({
        "type": "register", "protocol": 1, "name": "Sly2", "owner": "ana",
        "key": shared.to_string(),
    }));
    let answer = second.next().expect("answer");
    assert_eq!(answer["type"], "error", "{answer}");
}

#[test]
fn a_closed_listener_lets_its_port_and_its_bots_go() {
    let (listener, events) = listen(Admission::Open);
    let addr = listener.local_addr();
    let (mut bot, _) = Client::register(addr, "Leaving", None);
    let Ok(Event::Registered(_)) = events.recv_timeout(Duration::from_secs(5)) else {
        panic!("no registration event");
    };
    listener.close();
    // The bot hears the end of its connection.
    while bot.next().is_some() {}
    // And the port is free to take again.
    let mut again = None;
    for _ in 0..250 {
        if let Ok(socket) = std::net::TcpListener::bind(addr) {
            again = Some(socket);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(again.is_some(), "the port was never let go");
}

fn registered(events: &Receiver<Event>) -> usize {
    loop {
        match events.recv_timeout(Duration::from_secs(5)) {
            Ok(Event::Registered(id)) => return id,
            Ok(_) => {}
            Err(_) => panic!("no registration event"),
        }
    }
}

/// A request that draws an error must not pay for itself. Each answer is a
/// line sent, and lines sent are what earn a bot the right to send more: if
/// an error earned it too, a bot could ask for errors forever.
#[test]
fn an_answer_to_a_bad_request_earns_nothing() {
    let (listener, events) = listen(Admission::Open);
    let (mut bot, _) = Client::register(listener.local_addr(), "Drum", None);
    let id = registered(&events);
    // One at a time, each answer read before the next is asked: no flood,
    // only a bot that keeps asking for the same error.
    for _ in 0..300 {
        bot.send(&json!({"type": "register", "protocol": 1, "name": "Drum"}));
        assert_eq!(bot.next().expect("an answer")["type"], "error");
    }
    // Time refills an allowance up to the burst and no further; only lines
    // that earn can lift it past that.
    assert!(
        listener.allowance(id) <= Listener::BURST,
        "300 errors earned the bot {} lines",
        listener.allowance(id)
    );
}

/// And a bot that asks for nothing but errors, as fast as it can, is let
/// go: told it is flooding, or, on a machine too busy to write the errors
/// out as fast as they are asked for, dropped for not reading them.
#[test]
fn a_flood_of_requests_answered_with_errors_is_let_go() {
    let (listener, events) = listen(Admission::Open);
    let (mut bot, _) = Client::register(listener.local_addr(), "Drum", None);
    let id = registered(&events);
    let again = json!({"type": "register", "protocol": 1, "name": "Drum"}).to_string();
    // Far more than the burst, written while the answers are read. A flood
    // is that many refusals in a row, and a reader slowed down by a busy
    // machine earns a line back every 20 ms, which starts the count again:
    // a long flood still trips it.
    let mut flood = bot.stream.try_clone().expect("a second handle");
    let writer = std::thread::spawn(move || {
        for _ in 0..20_000 {
            if writeln!(flood, "{again}").is_err() {
                break;
            }
        }
    });
    while let Some(msg) = bot.next() {
        if msg["type"] == "error" && msg["fatal"] == true {
            assert_eq!(msg["message"], "flooding", "{msg}");
            break;
        }
    }
    drop(bot);
    let _ = writer.join();
    let gone = (0..250).any(|_| {
        std::thread::sleep(Duration::from_millis(20));
        !listener.connected(id)
    });
    assert!(gone, "a flood and still welcome");
}

/// Wrong keys from an address lock its keys out, not its tokens: a bot
/// coming back with its own token is let in, and a stale token is no
/// strike against anyone sharing the address (behind a cup's proxy, say).
#[test]
fn a_locked_out_address_still_takes_a_bot_back_by_its_token() {
    let key = Key::draw();
    let (listener, events) = listen(Admission::Keys(vec![invite(&key, None)]));
    let addr = listener.local_addr();
    let (keeper, answer) = Client::register(addr, "Keeper", Some(&key));
    let token = answer["token"].as_str().expect("token").to_string();
    let id = registered(&events);
    // Three stale tokens cost nobody anything.
    for _ in 0..3 {
        let mut stale = Client::connect(addr);
        stale.send(&json!({"type": "register", "protocol": 1, "token": "0".repeat(26)}));
        assert_eq!(stale.next().expect("answer")["type"], "error");
    }
    let (_other, answer) = Client::register(addr, "Other", Some(&key));
    assert_eq!(answer["type"], "registered", "{answer}");
    // Three wrong keys lock the address's keys out...
    for _ in 0..3 {
        let (_c, answer) = Client::register(addr, "Guess", Some(&Key::draw()));
        assert_eq!(answer["type"], "error");
    }
    let (_c, answer) = Client::register(addr, "Late", Some(&key));
    assert_eq!(answer["type"], "error", "the keys are locked out");
    // ...and not the bot that drops and comes back with its token.
    drop(keeper);
    while listener.connected(id) {
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut back = Client::connect(addr);
    back.send(&json!({"type": "register", "protocol": 1, "token": token}));
    let answer = back.next().expect("answer");
    assert_eq!(answer["type"], "registered", "{answer}");
}

/// Connections that never register are capped, so a few hundred silent
/// ones cannot use up the machine; one over the cap is told so at once,
/// and the places come back as they go.
#[test]
fn silent_connections_are_capped_and_told_so_at_once() {
    let (listener, _events) = listen(Admission::Open);
    let addr = listener.local_addr();
    let mut silent: Vec<Client> = Vec::new();
    for _ in 0..8 {
        silent.push(Client::connect(addr));
    }
    // Each has been taken in before the next is tried.
    std::thread::sleep(Duration::from_millis(200));
    let started = std::time::Instant::now();
    let mut over = Client::connect(addr);
    let answer = over.next().expect("an answer");
    assert_eq!(answer["type"], "error");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "told after {:?}",
        started.elapsed()
    );
    drop(silent);
    std::thread::sleep(Duration::from_millis(200));
    let (_bot, answer) = Client::register(addr, "Patient", None);
    assert_eq!(answer["type"], "registered", "{answer}");
}

/// A lookahead runs off the game's lock: the game publishes its board
/// through it every tick, and must never wait out somebody's 600 ticks of
/// thinking to do so.
#[test]
fn a_lookahead_does_not_hold_the_game_up() {
    use super::listener::{View, lock};
    let (listener, events) = listen(Admission::Open);
    let (mut bot, _) = Client::register(listener.local_addr(), "Thinker", None);
    let id = registered(&events);
    let link = listener.open_game(7, vec![(id, 0)]);
    let board = crate::sim::classic_arena(false, 2);
    let ticks = 3000;
    let alone = std::time::Instant::now();
    super::lookahead::run(&board, 0, ticks, &[]);
    let alone = alone.elapsed();
    *lock(&link.view) = Some(View {
        board,
        cursors: vec![None, None],
        budget: vec![ticks, ticks],
    });
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let thinker = {
        let done = std::sync::Arc::clone(&done);
        std::thread::spawn(move || {
            bot.send(&json!({"type": "simulate", "game": 7, "ticks": ticks, "plan": []}));
            let answer = bot.next().expect("simulated");
            done.store(true, std::sync::atomic::Ordering::SeqCst);
            answer
        })
    };
    let mut longest = Duration::ZERO;
    while !done.load(std::sync::atomic::Ordering::SeqCst) {
        let asked = std::time::Instant::now();
        drop(lock(&link.view));
        longest = longest.max(asked.elapsed());
        std::thread::sleep(Duration::from_millis(1));
    }
    let answer = thinker.join().expect("thinker");
    assert_eq!(answer["type"], "simulated", "{answer}");
    assert!(
        longest < alone / 2,
        "the game waited {longest:?} for a lookahead that takes {alone:?}"
    );
}

/// A bot that asks for replays and never reads them is dropped once a
/// couple of megabytes are waiting for it, not after hundreds of them.
#[test]
fn a_bot_that_asks_for_replays_and_never_reads_is_dropped() {
    let (listener, events) = listen(Admission::Open);
    let (mut bot, _) = Client::register(listener.local_addr(), "Hoarder", None);
    let id = registered(&events);
    listener.keep_replay("big".into(), vec![id], "x".repeat(200 * 1024));
    let ask = json!({"type": "replay", "id": "big"}).to_string();
    for _ in 0..150 {
        if writeln!(bot.stream, "{ask}").is_err() {
            break;
        }
    }
    let mut dropped = false;
    while let Ok(event) = events.recv_timeout(Duration::from_secs(5)) {
        if matches!(event, Event::Dropped(gone) if gone == id) {
            dropped = true;
            break;
        }
    }
    assert!(
        dropped,
        "30 MB asked for and never read, and still connected"
    );
}

/// A `simulate` the listener cannot read is answered with an error and is
/// no answer to the tick: the bot's reply after it is its move.
#[test]
fn a_garbled_lookahead_is_an_error_and_not_the_move() {
    let (listener, events) = listen(Admission::Open);
    let (mut bot, _) = Client::register(listener.local_addr(), "Fumble", None);
    let id = registered(&events);
    let player = std::thread::spawn(move || {
        let mut after = None;
        while let Some(msg) = bot.next() {
            match msg["type"].as_str() {
                Some("hello") => bot.send(&json!({"type": "ready", "game": 7})),
                Some("tick") if msg["tick"] == 0 => {
                    bot.send(&json!({
                        "type": "simulate", "game": 7, "ticks": 10,
                        "plan": [{"at": 0, "act": "place", "x": 9, "y": 1}],
                    }));
                    after = bot.next();
                    bot.send(&json!({"game": 7, "tick": 0, "act": "place", "x": 9, "y": 0, "dir": "down"}));
                }
                Some("tick") => bot.send(&json!({"game": 7, "tick": msg["tick"], "act": "none"})),
                Some("end") => return after,
                _ => {}
            }
        }
        panic!("no end");
    });
    let (result, _) = play(
        &listener,
        spec(
            little_beach(10),
            vec![Seat::Bot(id), Seat::Ai(BotLevel::Easy)],
            1000,
        ),
    );
    let after = player.join().expect("player").expect("an answer");
    assert_eq!(after["type"], "error", "{after}");
    assert_eq!(after["game"], 7, "{after}");
    let placed = result.replay.inputs.first().map(|actions| actions[0]);
    assert!(
        matches!(
            placed,
            Some(crate::sim::PlayerAction::Place { x: 9, y: 0, .. })
        ),
        "{placed:?}"
    );
}
