//! UDP transport for online lockstep (spec §7.6). Engine-free but IO-bound,
//! so it lives beside `sim` rather than inside it; the Bevy shell drives it.
//!
//! Datagrams are small and typed by their first byte, one tag per
//! [`NetMsg`] variant: the handshake, batched inputs (packed [`InputMsg`]s),
//! state hashes for desync detection, and the lobby, chat, spectator and
//! catch-up traffic beside them. The second byte is always
//! [`PROTOCOL_VERSION`], and a datagram written by any other version is
//! refused rather than guessed at.

mod beacon;
pub use beacon::*;

use crate::sim::{INPUT_BYTES, InputMsg};
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

/// The wire protocol this build speaks. Bump it whenever the layout of any
/// datagram changes, and whenever one is *added*, which is the case that is
/// easy to miss: chat, the roster and the abandonment notice all arrived
/// under version 4 before anybody noticed.
///
/// Bump it for a change in **what the sim does**, too. Lockstep sends
/// inputs, not outcomes, so two builds that tick the same inputs to
/// different boards are the worst version of this: every datagram decodes,
/// and the game quietly stops being the same game on both machines. Version
/// 6 is where gulls started catching crabs they walked head-on into, 7 is
/// where a `Start` grew a tail (the host's handmade beach), 8 is where
/// `Resume` and `Abandoned` each grew a frame number and gulls started
/// catching crabs across the seam of a wrapping board, 10 is where a
/// handmade beach that cannot seat the table gave way to the generated
/// arena the terms name, 11 is where a tick's inputs became one
/// datagram, 12 is where a spectator's greeting started carrying its
/// name, 13 is where a spectator arriving mid-round started being sent
/// the round as it stands (`CatchUp`) instead of a place in line, and 14
/// is where spectators started calling the winner (`SpectatorPick`,
/// `CrowdPicks`), 15 is where the terms started carrying how many
/// arrows each player may have standing, and 16 is where the game's AI
/// started walking under the fair cursor, every seat started saying what
/// holds it (a person, the AI, or a bot: the greeting's flag, the roster's
/// and the start's kinds), and a host could ask a peer to leave. 17 is
/// another version 10: the beaches bigger than the classic one started
/// generating more walls and rocks and taking more gulls, from the same
/// `Start` as before, and a five- or six-seat table outgrowing its map
/// started landing on the 16x11 rather than the 20x13. 18 is where a
/// versus flock started turning over, the oldest gull clear of the castles
/// flying off to make room, with the final rush raising the cap. 19 is
/// where versus stopped rolling golden crabs and started calling one every
/// three minutes, worth 25, with the call on the board.
///
/// Version 10 is the shape worth reading twice: not one byte of the `Start`
/// moved. Two builds hold the identical datagram, agree on every field in
/// it, and lay out different beaches from it.
///
/// Byte 1 of every datagram carries this number, and **that position is
/// frozen for all time**: it is how a build tells "I cannot read this"
/// apart from "I disagree with this", however the rest of the format
/// moves.
pub const PROTOCOL_VERSION: u8 = 19;

/// Connections a host accepts: five rivals (a six-seat table) and everyone
/// else who turned up. How many of them get a seat is the lobby's
/// business, not the socket's.
///
/// One pool for the lot, so at ten a full table left room for four
/// onlookers and the next person to arrive was ignored: no answer, and a
/// joiner that hears nothing reports the host as absent rather than the
/// beach as full.
///
/// Sixteen is affordable because a peer in line is no longer sent the round
/// it is not watching (see `PeerBook::follows_the_round`): someone waiting
/// costs a roster line and a beacon, where before it cost the host about
/// 180 datagrams a second. A *watcher* still costs that, being in the
/// round like everybody else.
pub const MAX_PEERS: usize = 16;

/// The receive buffer, which must hold the largest message whole: UDP
/// truncates a datagram to the buffer given, and a truncated message fails
/// decode silently. A test proves every message fits. Raised from 512 when
/// the host became able to send a handmade beach with the invitation.
const MAX_DATAGRAM: usize = 1024;

/// The most compressed beach an invitation may carry. Everything else in a
/// `Start` - the seats, the terms, six names at full width - takes a little
/// under two hundred bytes, and what is left is this.
///
/// It has to be a number the sender checks, because the failure is silent
/// at the other end: an oversized datagram is truncated to the receive
/// buffer, decode refuses the remains, and the joiner waits for an
/// invitation that was thrown away. A busy 20x13 beach packs to about 570
/// bytes and fits; one with a crab on every tile packs to over 1300.
///
/// `a_start_carrying_the_largest_beach_still_fits` holds the arithmetic.
pub const MAX_BEACH_BYTES: usize = 824;

/// A non-blocking UDP endpoint. A joiner talks to one peer (the host); a
/// host accepts up to [`MAX_PEERS`] of them, five rivals and a few
/// onlookers, and is the relay hub of the star.
pub struct UdpTransport {
    socket: UdpSocket,
    peers: Vec<SocketAddr>,
    /// Host side: register unknown senders as new peers (up to `max_peers`).
    accept_new: bool,
    max_peers: usize,
    /// Host side: addresses asked to leave. A greeting already in flight
    /// when the host forgot one, or a peer that missed every `Kicked`,
    /// would otherwise be taken back as a new peer the moment it next
    /// spoke. Each is told again instead. A person who means to come back
    /// dials again, from a fresh socket and so a fresh address.
    turned_away: Vec<SocketAddr>,
}

/// How many turned-away addresses a host remembers, oldest forgotten
/// first: enough for any evening, and bounded whoever keeps knocking.
const TURNED_AWAY: usize = 64;

/// This machine's address on the network it would reach a beach over, or
/// `None` if it has no route out of itself.
///
/// A host binds `0.0.0.0`, so its own socket cannot say which of the
/// machine's addresses a friend should dial, and the standard library has
/// no way to list interfaces. So this asks the routing table: connecting a
/// UDP socket sends nothing, but it makes the kernel choose the interface
/// it *would* send from, and the socket then knows that interface's
/// address. The target is a direction to point in, a documentation address
/// (RFC 5737) no datagram is ever sent to.
pub fn local_ip() -> Option<std::net::IpAddr> {
    let probe = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    probe.connect(("203.0.113.1", 9)).ok()?;
    let ip = probe.local_addr().ok()?.ip();
    // A machine with no route out answers with either of these, and
    // neither is any use to somebody typing it in on another machine.
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

impl UdpTransport {
    /// Host: bind the given port; peers register from their first valid
    /// packet.
    pub fn host(port: u16) -> io::Result<UdpTransport> {
        let socket = UdpSocket::bind(("0.0.0.0", port))?;
        socket.set_nonblocking(true)?;
        Ok(UdpTransport {
            socket,
            peers: Vec::new(),
            accept_new: true,
            max_peers: MAX_PEERS,
            turned_away: Vec::new(),
        })
    }

    /// Joiner: bind an ephemeral port and talk only to the host.
    pub fn join(host: impl ToSocketAddrs) -> io::Result<UdpTransport> {
        let socket = UdpSocket::bind(("0.0.0.0", 0))?;
        socket.set_nonblocking(true)?;
        let peer = host
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unresolvable host"))?;
        Ok(UdpTransport {
            socket,
            peers: vec![peer],
            accept_new: false,
            max_peers: 1,
            turned_away: Vec::new(),
        })
    }

    pub fn connected(&self) -> bool {
        !self.peers.is_empty()
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// The address at the other end, for a socket that has exactly one:
    /// the joiner's. A host has many and answers `None`.
    pub fn peer_addr(&self) -> Option<SocketAddr> {
        match self.peers.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    /// Send to every peer.
    pub fn send(&self, msg: NetMsg) {
        let bytes = msg.encode();
        for peer in &self.peers {
            let _ = self.socket.send_to(&bytes, peer);
        }
    }

    /// Send a tick's worth of inputs, in as few datagrams as the cap
    /// allows, to every peer `hears` says is following the round.
    ///
    /// The audience is the caller's business and not the socket's: who is
    /// at the table, who is watching and who is in line for the next
    /// round is the lobby's bookkeeping.
    ///
    /// Encodes once per datagram rather than once per peer, and into the
    /// stack: this is the one path the game walks thousands of times a
    /// second, and a `Vec` per recipient was most of what it allocated.
    pub fn send_inputs(&self, msgs: &[InputMsg], mut hears: impl FnMut(usize) -> bool) {
        if msgs.is_empty() {
            return;
        }
        for chunk in msgs.chunks(MAX_INPUTS_PER_DATAGRAM) {
            let mut buf = [0u8; MAX_DATAGRAM];
            let len = encode_inputs(chunk, &mut buf);
            for (index, peer) in self.peers.iter().enumerate() {
                if hears(index) {
                    let _ = self.socket.send_to(&buf[..len], peer);
                }
            }
        }
    }

    /// Put arbitrary bytes on the wire: the only way to write a datagram
    /// this build would never write, as a foreign version is.
    #[cfg(test)]
    fn send_raw(&self, bytes: &[u8]) {
        for peer in &self.peers {
            let _ = self.socket.send_to(bytes, peer);
        }
    }

    /// Forget a peer, closing the gap behind it.
    ///
    /// Indices shift, and every list the caller keeps alongside `peers`
    /// shifts with them, which the caller does itself: the lobby knows what
    /// it is keeping and this layer does not.
    ///
    /// UDP never says a peer has gone, so somebody has to decide it has, or
    /// the socket fills with ghosts that hold seats and are counted.
    pub fn forget(&mut self, index: usize) {
        if index < self.peers.len() {
            self.peers.remove(index);
        }
    }

    /// Ask a peer to leave and keep it out: it is told (a few times, since
    /// one datagram is the one the network eats), forgotten like
    /// [`Self::forget`], and anything more from its address is answered
    /// with the same word rather than taken in as a new peer.
    pub fn turn_away(&mut self, index: usize) {
        let Some(&addr) = self.peers.get(index) else {
            return;
        };
        for _ in 0..3 {
            self.send_to(index, NetMsg::Kicked);
        }
        self.forget(index);
        if self.turned_away.len() >= TURNED_AWAY {
            self.turned_away.remove(0);
        }
        self.turned_away.push(addr);
    }

    /// Send to one peer by index (host relaying / seat assignment).
    pub fn send_to(&self, index: usize, msg: NetMsg) {
        debug_assert!(
            index < self.peers.len(),
            "sending to peer {index} of {}",
            self.peers.len()
        );
        if let Some(peer) = self.peers.get(index) {
            let _ = self.socket.send_to(&msg.encode(), peer);
        }
    }

    /// Drain every waiting datagram as `(message, peer index)`. Unknown
    /// senders become new peers on the host while capacity remains.
    pub fn recv_all(&mut self) -> Vec<(NetMsg, usize)> {
        let mut out = Vec::new();
        // The named `Start` is 162 bytes, and the 64-byte buffer that
        // preceded [`MAX_DATAGRAM`] silently ate every one of them.
        let mut buf = [0u8; MAX_DATAGRAM];
        loop {
            match self.socket.recv_from(&mut buf) {
                Ok((len, from)) => {
                    let Some(msg) = NetMsg::decode(&buf[..len]) else {
                        // A datagram we cannot read. If it is well-formed but
                        // from another build, answer so the sender learns why
                        // it is being ignored; anything else is noise. Either
                        // way it never claims a peer slot.
                        if NetMsg::peek_version(&buf[..len])
                            .is_some_and(|version| version != PROTOCOL_VERSION)
                        {
                            let reply = NetMsg::Incompatible {
                                version: PROTOCOL_VERSION,
                            };
                            let _ = self.socket.send_to(&reply.encode(), from);
                        }
                        continue;
                    };
                    if self.turned_away.contains(&from) {
                        let _ = self.socket.send_to(&NetMsg::Kicked.encode(), from);
                        continue;
                    }
                    let index = match self.peers.iter().position(|p| *p == from) {
                        Some(index) => index,
                        None if self.accept_new && self.peers.len() < self.max_peers => {
                            self.peers.push(from);
                            self.peers.len() - 1
                        }
                        None => continue,
                    };
                    out.push((msg, index));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        out
    }
}

mod msg;
mod wire;
pub use msg::*;
pub use wire::*;

#[cfg(test)]
mod tests {
    use super::*;

    /// A peer asked to leave stays out. Its greeting already in flight, or
    /// the next one from a joiner that missed the word, is answered with
    /// the word again rather than taking a fresh place at the table.
    #[test]
    fn a_peer_turned_away_is_not_taken_back() {
        let mut host = UdpTransport::host(0).expect("bind");
        let port = host.local_addr().expect("addr").port();
        let mut joiner = UdpTransport::join(("127.0.0.1", port)).expect("join");
        let settle = |host: &mut UdpTransport| {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                if !host.recv_all().is_empty() {
                    break;
                }
            }
        };
        joiner.send(NetMsg::hello("Bo"));
        settle(&mut host);
        assert_eq!(host.peer_count(), 1, "Bo is aboard");
        host.turn_away(0);
        assert_eq!(host.peer_count(), 0);
        // Bo greets again, as a joiner does every second.
        joiner.send(NetMsg::hello("Bo"));
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            assert!(host.recv_all().is_empty(), "nothing from Bo is taken in");
        }
        assert_eq!(host.peer_count(), 0, "and Bo has no place at the table");
        let kicked = joiner
            .recv_all()
            .into_iter()
            .filter(|(msg, _)| *msg == NetMsg::Kicked)
            .count();
        assert!(kicked > 3, "told on the way out and told again: {kicked}");
    }

    /// A host on another build answers the greeting instead of ignoring it,
    /// so the joiner learns why nothing is happening, and the stranger
    /// never takes up one of the host's peer slots.
    #[test]
    fn a_mismatched_greeting_is_answered_not_swallowed() {
        let mut host = UdpTransport::host(0).expect("bind");
        let port = host.local_addr().expect("addr").port();
        let mut joiner = UdpTransport::join(("127.0.0.1", port)).expect("join");
        let mut hello = NetMsg::hello("Anna").encode();
        hello[1] = PROTOCOL_VERSION.wrapping_add(1); // a build from the future
        let mut answer = None;
        for _ in 0..40 {
            joiner.send_raw(&hello);
            std::thread::sleep(std::time::Duration::from_millis(5));
            assert!(host.recv_all().is_empty(), "nothing the host can act on");
            if let Some((msg, _)) = joiner.recv_all().into_iter().next() {
                answer = Some(msg);
                break;
            }
        }
        assert_eq!(
            answer,
            Some(NetMsg::Incompatible {
                version: PROTOCOL_VERSION
            })
        );
        assert_eq!(host.peer_count(), 0, "and it claimed no seat at the table");
    }

    /// The socket takes peers up to [`MAX_PEERS`] and refuses the rest,
    /// without the refused ones costing anything already connected.
    #[test]
    fn host_accepts_peers_up_to_its_cap() {
        let mut host = UdpTransport::host(0).expect("bind");
        let port = host.local_addr().expect("addr").port();
        let joiners: Vec<UdpTransport> = (0..MAX_PEERS + 2)
            .map(|_| UdpTransport::join(("127.0.0.1", port)).expect("join"))
            .collect();
        for joiner in &joiners {
            joiner.send(NetMsg::hello(""));
        }
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let _ = host.recv_all();
            if host.peer_count() == MAX_PEERS {
                break;
            }
        }
        assert_eq!(
            host.peer_count(),
            MAX_PEERS,
            "the ones past the cap are refused"
        );
    }
}
