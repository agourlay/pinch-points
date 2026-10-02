//! The one line every way in hands a bot: `pinch://HOST:PORT/KEY`, and the
//! two secrets that travel with a registration, the join key and the token.
//!
//! The key gets a bot in; the token is who it is afterwards. They are never
//! called by one name, here or on the wire (see `docs/bot-seats.md`).

use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::net::SocketAddr;

/// Where bots connect when nothing says otherwise: beside the lobby's UDP
/// range (`transport::beacon::LOBBY_PORTS`, 47700 to 47707).
pub const DEFAULT_PORT: u16 = 47710;

/// Crockford's base32 alphabet: no I, L, O or U, so a key read aloud across
/// a room cannot be misheard as a digit.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A join key: eight base32 characters, 40 bits, written in two groups of
/// four (`7F3K-9QXA`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Key(String);

impl Key {
    /// A fresh key, drawn from the operating system's randomness. Nothing
    /// is reused from a previous session.
    pub fn draw() -> Key {
        let bits = random_u64();
        let chars: String = (0..8)
            .map(|i| ALPHABET[((bits >> (i * 5)) & 31) as usize] as char)
            .collect();
        Key(chars)
    }

    /// Read a key as somebody typed it: case, the dash and the letters
    /// Crockford folds (`O` for zero, `I` and `L` for one) are forgiven.
    pub fn parse(text: &str) -> Option<Key> {
        let mut out = String::new();
        for c in text.chars() {
            let c = match c.to_ascii_uppercase() {
                '-' => continue,
                'O' => '0',
                'I' | 'L' => '1',
                c => c,
            };
            if !c.is_ascii() || !ALPHABET.contains(&(c as u8)) {
                return None;
            }
            out.push(c);
        }
        (out.len() == 8).then_some(Key(out))
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}-{}", &self.0[..4], &self.0[4..])
    }
}

/// A token: who a registered bot is for the life of the listener. Random,
/// never printed, never logged; `Debug` says so rather than showing it.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Token(String);

impl Token {
    pub fn draw() -> Token {
        let mut out = String::with_capacity(26);
        for _ in 0..2 {
            let bits = random_u64();
            for i in 0..12 {
                out.push(ALPHABET[((bits >> (i * 5)) & 31) as usize] as char);
            }
        }
        Token(out)
    }

    /// The token as the bot sent it. Compared, never shown.
    pub fn from_wire(text: &str) -> Token {
        Token(text.to_string())
    }

    /// What goes in the `registered` message, to the bot alone.
    pub fn reveal(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Token(..)")
    }
}

/// 64 bits nobody can predict from outside the process.
///
/// Built from the standard library alone: `RandomState` keys its hasher
/// from the operating system's randomness, and a fresh one per call mixed
/// with the clock and a counter is plenty for a key read off a screen at a
/// party and a token that only has to be unguessable. No new crate for it.
fn random_u64() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    h.finish()
}

/// A connection string: where to connect, whether through TLS, and the key
/// when there is one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConnString {
    /// `pinchs://`: connect with TLS, to a proxy in front of a server on
    /// the internet. The game itself only ever prints `pinch://`.
    pub tls: bool,
    pub host: String,
    pub port: u16,
    pub key: Option<Key>,
}

impl ConnString {
    /// The string for a listener at `addr`, with `key` if it asks for one.
    ///
    /// An unspecified address (`0.0.0.0`, listening on every interface) is
    /// no use to a bot, which has to be told an address it can reach; the
    /// caller passes the one it would like printed instead.
    pub fn new(host: impl Into<String>, port: u16, key: Option<Key>) -> ConnString {
        ConnString {
            tls: false,
            host: host.into(),
            port,
            key,
        }
    }

    pub fn parse(text: &str) -> Result<ConnString, String> {
        let text = text.trim();
        let (tls, rest) = if let Some(rest) = text.strip_prefix("pinch://") {
            (false, rest)
        } else if let Some(rest) = text.strip_prefix("pinchs://") {
            (true, rest)
        } else {
            return Err("a connection string starts with pinch:// or pinchs://".to_string());
        };
        let (authority, key) = match rest.split_once('/') {
            Some((authority, "")) => (authority, None),
            Some((authority, key)) => (
                authority,
                Some(Key::parse(key).ok_or_else(|| format!("{key:?} is not a join key"))?),
            ),
            None => (rest, None),
        };
        let (host, port) = match authority.rsplit_once(':') {
            // An IPv6 literal is bracketed, so its colons are not the port's.
            Some((host, port)) if !port.contains(']') => (
                host,
                port.parse::<u16>()
                    .map_err(|_| format!("{port:?} is not a port"))?,
            ),
            _ => (authority, DEFAULT_PORT),
        };
        let host = host.trim_start_matches('[').trim_end_matches(']');
        if host.is_empty() {
            return Err("the connection string names no host".to_string());
        }
        Ok(ConnString {
            tls,
            host: host.to_string(),
            port,
            key,
        })
    }
}

impl fmt::Display for ConnString {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let scheme = if self.tls { "pinchs" } else { "pinch" };
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        write!(f, "{scheme}://{host}:{}", self.port)?;
        if let Some(key) = &self.key {
            write!(f, "/{key}")?;
        }
        Ok(())
    }
}

/// The address to print for a listener bound to `bound`: itself, unless it
/// listens on every interface, when it is this machine's address on the
/// LAN, found by asking the routing table which interface would carry a
/// packet out (no packet is sent).
pub fn reachable_host(bound: SocketAddr) -> String {
    if !bound.ip().is_unspecified() {
        return bound.ip().to_string();
    }
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("192.0.2.1:9")?;
            socket.local_addr()
        })
        .map_or_else(|_| "127.0.0.1".to_string(), |addr| addr.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connection_string_reads_back_as_it_was_written() {
        let s = ConnString::new("127.0.0.1", 47710, Some(Key::draw()));
        assert_eq!(ConnString::parse(&s.to_string()), Ok(s.clone()));
        let open = ConnString::new("192.168.1.20", 47710, None);
        assert_eq!(open.to_string(), "pinch://192.168.1.20:47710");
        assert_eq!(ConnString::parse("pinch://192.168.1.20:47710/"), Ok(open));
    }

    #[test]
    fn the_secure_form_and_a_missing_port_are_understood() {
        let s = ConnString::parse("pinchs://cup.example.org/7f3k-9qxa").expect("parses");
        assert!(s.tls);
        assert_eq!(s.port, DEFAULT_PORT);
        assert_eq!(s.key.map(|k| k.to_string()), Some("7F3K-9QXA".to_string()));
        assert_eq!(
            ConnString::parse("pinch://[::1]:5000").map(|s| s.host),
            Ok("::1".to_string())
        );
    }

    #[test]
    fn garbage_is_refused_with_a_reason() {
        assert!(ConnString::parse("http://x").is_err());
        assert!(ConnString::parse("pinch://:47710").is_err());
        assert!(ConnString::parse("pinch://h:port").is_err());
        assert!(ConnString::parse("pinch://h:1/NOTAKEY!").is_err());
    }

    #[test]
    fn a_key_is_forgiving_to_read_and_strict_about_length() {
        let key = Key::draw();
        let shown = key.to_string();
        assert_eq!(shown.len(), 9);
        assert_eq!(Key::parse(&shown.to_lowercase()), Some(key));
        assert_eq!(Key::parse("7F3K-9QXO"), Key::parse("7F3K-9QX0"));
        assert_eq!(Key::parse("7F3K"), None);
    }

    #[test]
    fn keys_and_tokens_are_fresh_every_time() {
        assert_ne!(Key::draw(), Key::draw());
        assert_ne!(Token::draw(), Token::draw());
        assert_eq!(format!("{:?}", Token::draw()), "Token(..)");
    }
}
