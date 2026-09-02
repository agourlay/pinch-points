//! UDP transport for online lockstep (spec §7.6). Engine-free but IO-bound,
//! so it lives beside `sim` rather than inside it; the Bevy shell drives it.
//!
//! Datagrams are tiny and typed by their first byte: hello (handshake),
//! input (packed [`InputMsg`]), or a state hash for desync detection. The
//! second byte is always [`PROTOCOL_VERSION`], and a datagram written by any
//! other version is refused rather than guessed at.

mod beacon;
pub use beacon::*;

use crate::sim::{INPUT_BYTES, InputMsg};
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

/// The wire protocol this build speaks. Bump it whenever the layout of any
/// datagram changes, and whenever one is *added*, which is the case that
/// is easy to miss: two builds both claiming the same version, one of them
/// sending a message the other has no tag for, is the silent disagreement
/// the byte exists to prevent. Chat, the roster and the
/// abandonment notice all arrived under version 4 before this was noticed.
///
/// Bump it for a change in **what the sim does**, too. Lockstep sends
/// inputs, not outcomes, so two builds that tick the same inputs to
/// different boards are the worst version of this: every datagram decodes,
/// and the game quietly stops being the same game on both machines. Version
/// 6 is where gulls started catching crabs they walked head-on into, 7
/// is where a `Start` grew a tail (the host's handmade beach, when the
/// round is played on one), 8 is where `Resume` and `Abandoned` each
/// grew a frame number, and gulls started catching crabs across the seam
/// of a wrapping board, and 10 is where a handmade beach that cannot seat
/// the table stopped being played on and gave way to the generated arena
/// the terms name.
///
/// That last one is the shape this paragraph is about, and worth reading
/// twice: not one byte of the `Start` moved. Two builds hold the identical
/// datagram, agree on every field in it, and lay out different beaches
/// from it. Nothing would have complained, and the hash check would have
/// called it a desync without ever saying why.
///
/// Byte 1 of every datagram carries this number, and **that position is
/// frozen for all time**: it is how a build tells "I cannot read this"
/// apart from "I disagree with this", however the rest of the format moves.
/// Without it two peers on different builds decode each other's messages and
/// silently disagree about the round, which looks like a desync or like
/// nothing at all.
pub const PROTOCOL_VERSION: u8 = 10;

/// Connections a host accepts: five rivals (a six-seat table) and a few
/// onlookers. How many of them get a seat is the lobby's business, not the
/// socket's.
pub const MAX_PEERS: usize = 9;

/// The receive buffer, which must hold the largest message whole: UDP
/// truncates a datagram to the buffer given, and a truncated message
/// fails decode silently. A test proves every message fits.
/// A datagram this build will read. Raised from 512 when the host became
/// able to send a handmade beach with the invitation: a compressed 20x13
/// level is a few hundred bytes on top of the seats and the names.
const MAX_DATAGRAM: usize = 1024;

/// The most compressed beach an invitation may carry. Everything else in a
/// `Start` - the seats, the terms, six names at full width - takes a little
/// under two hundred bytes, and what is left is this.
///
/// It has to be a number the sender checks, because the failure is silent
/// at the other end: an oversized datagram is truncated to the receive
/// buffer, decode refuses the remains, and the joiner sits waiting for an
/// invitation that arrived and was thrown away. A busy 20x13 beach packs to
/// about 570 bytes and fits; one with a crab on every tile packs to over
/// 1300 and does not.
///
/// `a_start_carrying_the_largest_beach_still_fits` holds the arithmetic.
pub const MAX_BEACH_BYTES: usize = 832;

/// A non-blocking UDP endpoint. A joiner talks to one peer (the host); a
/// host accepts up to [`MAX_PEERS`] of them, five rivals and a few
/// onlookers, and is the relay hub of the star.
pub struct UdpTransport {
    socket: UdpSocket,
    peers: Vec<SocketAddr>,
    /// Host side: register unknown senders as new peers (up to `max_peers`).
    accept_new: bool,
    max_peers: usize,
}

/// This machine's address on the network it would reach a beach over, or
/// `None` if it has no route out of itself.
///
/// A host binds `0.0.0.0`, so its own socket cannot say which of the
/// machine's addresses a friend should dial, and the standard library has
/// no way to list interfaces. So this asks the routing table instead:
/// connecting a UDP socket sends nothing at all, but it does make the
/// kernel choose the interface it *would* send from, and the socket then
/// knows the address of that interface. The target is a direction to
/// point in, nothing more: a documentation address (RFC 5737) that belongs
/// to nobody, and no datagram is ever sent to it.
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
    /// shifts with them, which is why this returns nothing clever and
    /// leaves the caller to do it: the lobby knows what it is keeping and
    /// this layer does not.
    ///
    /// UDP never says a peer has gone, so somebody has to decide it has.
    /// Without this a socket fills with ghosts: they hold seats, they are
    /// counted, and the table never has room again.
    pub fn forget(&mut self, index: usize) {
        if index < self.peers.len() {
            self.peers.remove(index);
        }
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
