//! Bots for the doorway's and the round's tests: test clients that
//! register with a chair's string the way a bot does.

use super::Doorway;
use crate::bots::connstr::ConnString;
use std::io::{BufRead, BufReader, Write};
use std::time::{Duration, Instant};

/// Register with a chair's string, the way a bot does.
pub(crate) fn register(string: &str, name: &str, owner: &str) -> std::net::TcpStream {
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

/// Wait for the doorway to seat a bot in chair `at`.
pub(crate) fn seated(door: &mut Doorway, at: usize) {
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
pub(super) fn try_register(string: &str, name: &str) -> String {
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

/// A bot on the other end of a socket, reading what the game says.
pub(super) struct TestBot {
    pub(super) stream: std::net::TcpStream,
    pub(super) reader: BufReader<std::net::TcpStream>,
}

impl TestBot {
    pub(crate) fn register(string: &str) -> TestBot {
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

    pub(super) fn send(&mut self, msg: &serde_json::Value) {
        writeln!(self.stream, "{msg}").expect("write");
    }

    /// The next message of type `kind`, skipping the others, if one
    /// comes within `within`.
    pub(super) fn next(&mut self, kind: &str, within: Duration) -> Option<serde_json::Value> {
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
pub(super) fn poll_until(door: &mut Doorway, done: impl Fn(&Doorway) -> bool) -> bool {
    for _ in 0..400 {
        door.poll();
        if done(door) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}
