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
/// and the game quietly stops being the same game on both machines.
///
/// - 6: gulls started catching crabs they walked head-on into.
/// - 7: a `Start` grew a tail (the host's handmade beach).
/// - 8: `Resume` and `Abandoned` each grew a frame number, and gulls
///   started catching crabs across the seam of a wrapping board.
/// - 10: a handmade beach that cannot seat the table gave way to the
///   generated arena the terms name.
/// - 11: a tick's inputs became one datagram.
/// - 12: a spectator's greeting started carrying its name.
/// - 13: a spectator arriving mid-round started being sent the round as
///   it stands (`CatchUp`) instead of a place in line.
/// - 14: spectators started calling the winner (`SpectatorPick`,
///   `CrowdPicks`).
/// - 15: the terms started carrying how many arrows each player may have
///   standing.
/// - 16: the game's AI started walking under the fair cursor, every seat
///   started saying what holds it (a person, the AI, or a bot: the
///   greeting's flag, the roster's and the start's kinds), and a host
///   could ask a peer to leave.
/// - 17: another version 10. The beaches bigger than the classic one
///   started generating more walls and rocks and taking more gulls, from
///   the same `Start` as before, and a five- or six-seat table outgrowing
///   its map started landing on the 16x11 rather than the 20x13.
/// - 18: a versus flock started turning over, the oldest gull clear of the
///   castles flying off to make room, with the final rush raising the cap.
/// - 19: versus stopped rolling golden crabs and started calling one every
///   three minutes (worth 25 then, 30 since 21), with the call on the
///   board.
/// - 20: the versus event wheel stopped locking after an event that is
///   over in the tick it fires (`timed_lock` in the rules).
/// - 21: versus Crab Monopoly started flying its crabs home, each banking
///   as it lands (`sweep_flight`, and the crabs in the air in every
///   snapshot), and the called golden crab went from 25 to 30.
/// - 22: a versus flock stopped losing a gull to a turnover whose new tile
///   was rock (`whole_flock`), a swept crab started banking for what it
///   was worth when Crab Monopoly fired (`sweep_keeps_worth`), a peer
///   started saying goodbye (`Leaving`), the roster started naming the
///   host's handmade map, and a handmade map started being played only
///   under terms that name it (`beach_in_play`), the terms moving off it
///   once the table outgrows its castles.
/// - 23: the round-length dial gained a one-minute stop in front of the
///   others, so the byte the terms carry for it names a different length
///   than it did, and a table of four or more started getting a spawner
///   hole for all but one player (the classic beach adding its corner
///   holes, a generated one never dealing four players the side pair
///   alone): another version 10.
/// - 24: the versus holes that open on the same tick started drawing which
///   of them the crab cap leaves out (`fair_spawns`), where they had been
///   tried top-left first.
/// - 25: the AI's moment to think in each window started taking the
///   board's seed, so a peer on 24 seats the same AI on a different
///   schedule.
/// - 26: a handmade beach started being built on the round's seed rather
///   than the one its file names (`Level::board_seeded`): another
///   version 10.
/// - 27: a mania's flood started coming out of two holes at a time, the
///   holes taking turns, where every hole had flooded at once.
/// - 28: an AI seat's hand started walking from where it last placed,
///   whether or not that post still stands, where a hand with no post up
///   had placed anywhere at once: the board carries it, and hashes it.
/// - 29: every `Start` started counting the rounds a host has dealt.
/// - 30: a spectators' call stopped firing on a beach whose event wheel is
///   off, a handmade one among them: a peer on 29 still fires it there.
/// - 31: a handmade beach with more castles than the table started being
///   built with the spare ones put away (`Level::table_board`), and one
///   whose castles skip a player stopped being played at all: another
///   version 10.
/// - 32: every `Start` started carrying the seats the results card before
///   it crowned (`Start::crowned`).
/// - 33: every `Start` started carrying the round's input delay
///   (`Start::delay`), which the host picks from the round trips it
///   measures with the new `Ping` and `Pong`, and the host started
///   keeping its players' clocks with it (`Clock`).
/// - 34: a tick's word between two peers became one `Batch`: inputs as runs
///   of a seat's frames, what the sender is missing (`wants`), from which
///   its receiver resends, and the pause, the clock and the seats given up
///   on riding along; the host sends each peer one, cut from every input
///   it holds, rather than passing each player's tail on as it came.
/// - 35: a host started answering a greeting it has no room for (`Full`)
///   and telling a would-be player past the last chair that it has none
///   (`NoChair`).
/// - 36: a batch says which round it is from (`Batch::round`), and its
///   clock rides the last of its datagrams rather than the first; the
///   lobby's roster says to each peer whether it has a chair
///   (`Roster::chair`), and `NoChair` is retired.
/// - 37: the edge spawner started sending for a gull `GULL_LEAD` ticks
///   before it lands, the board carrying (and hashing) the gulls on their
///   way in, so every ambient gull lands later than on 36 and the PRNG is
///   drawn in a different order.
/// - 38: a snapshot started carrying who steered each gull and what the
///   steered gulls took (`steered:` and `stolen:` lines), which a peer on
///   37 refuses as unknown keys when it is caught up mid-round.
/// - 39: the lure went from 10 s to 5 s and its quiet spell from 20 s to
///   30 s; a three-seat table plays on the four end holes, and an odd
///   table's seats are drawn afresh onto its castles (five seats on a
///   beach with edges take the four corners and one long-edge centre). A
///   38 peer lays out a different beach for three or five from the same
///   `Start` and plays every lure twice as long.
/// - 40: the map byte names seven handmade versus maps and Surprise
///   (indices 7 to 14, which a 39 peer reads as other maps), every versus
///   beach but a player's own is mirrored from the round's seed (Round the
///   World also rolled by whole strips), and the AI measures through the
///   seam of an open beach; a level file may say `seats:` and `roll:`,
///   which a 39 peer refuses as unknown keys in a replay or a sent beach.
///   A 39 peer builds a different board from the same `Start`, and its AI
///   seats move differently on the open ocean.
/// - 41: sparkling crabs went from 2% of spawns to 4%, taken from the
///   commons, so a 40 peer rolls a different kind from the same draw; and
///   the Hard AI values a molt's lure for the 5 s it runs, and a molt that
///   will get home while a lure runs or rests at its points alone, so a 40
///   peer moves its Hard seats differently.
/// - 42: a state hash, a pause, a resume and a seat given up on started
///   saying which round they are from, as a batch has since 36 (the round
///   tag, first in each), and a peer drops and counts any of another
///   round. A 41 peer reads the tag as the first byte of the frame. The
///   bot protocol went from 1 to 2 in the same build, over its own TCP
///   line rather than these datagrams: a bot registering with 2 is sent
///   each tick as what changed since the last (`bots::protocol::delta`).
/// - 43: the Hard AI counts what banking a molt keeps from the rivals, a
///   full lure for each rival past the first, so a 42 peer moves its Hard
///   seats differently at three seats and more.
/// - 44: the roster grew a byte saying the host is picking the map
///   (`Roster::picking`): the host now picks it once the table is frozen,
///   at the start in the lobby and between the rounds of a series on the
///   results card, and the joiners wait on that rather than a dial. A
///   series stopped stepping its map on by itself.
/// - 45: the terms grew the seat order (`MatchTerms::order`, six bytes):
///   the host picks partners in the lobby by moving people between seats,
///   so the AI need no longer hold the top seats, and every peer builds
///   its lockstep over the players' seats wherever they are. In the same
///   version every versus round turns the tide-event wheel, a handmade
///   beach included (`match_setup::apply_terms`): a 44 peer builds a
///   custom beach with the wheel off and plays it differently.
/// - 46: a gull raid takes a third of the castle's bank, not half
///   (`sim::raid_loss`), so a 45 peer keeps a different score after the
///   same raid.
/// - 47: two tide-event rules. A spun Castle Swap with three castle
///   owners or more leaves the spinner's castles out of the rotation
///   (`Board::castle_swap_keeper`), and Speed Up opens the holes twice as
///   often while it runs (`Board::hole_period`): a 46 peer hands castles
///   and spawns crabs differently after either.
/// - 48: four more handmade maps (The Fountain, Four Doors, Gull Road,
///   Back to Back), appended to the map byte as indices 15 to 18. A 47
///   peer reads those bytes round its own shorter list (`Cycle::from_index`
///   wraps) and builds another beach from the same `Start`.
/// - 49: Round Table retuned for six: its top and bottom holes one tile
///   nearer the middle and a wall behind each corner castle
///   (`levels/versus/round_table.txt`). The map byte is unchanged, so a 48
///   peer builds the old layout from the same `Start`.
/// - 50: the board started knowing who plays for whom (`Rules::teams`,
///   from the terms' team mode, hashed and carried by a snapshot as a
///   `sides:` line). Gull Attack spun in a round played in teams lands no
///   gull beside the spinner's partners' castles, so a 49 peer raids the
///   spinner's own team and hashes the board differently, and refuses a
///   catch-up snapshot over the unknown key. The AI reads the same split:
///   the Hard AI steers no gull at a partner's castle and counts the lure
///   a molt denies the rival teams alone, so a 49 peer moves its AI seats
///   differently in a round played in teams. The room's notices (who
///   joined, left or was sent away, a start called off, the spectators'
///   call and the crowd's picks) travel as a `Notice` of what happened,
///   a new tag, for each peer to word in its own language, where they had
///   been chat in the host's: a 49 peer drops every one unread.
///
/// Version 10 is the shape worth reading twice: not one byte of the `Start`
/// moved. Two builds hold the identical datagram, agree on every field in
/// it, and lay out different beaches from it.
///
/// Byte 1 of every datagram carries this number, and **that position is
/// frozen for all time**: it is how a build tells "I cannot read this"
/// apart from "I disagree with this", however the rest of the format
/// moves.
pub const PROTOCOL_VERSION: u8 = 50;

/// Connections a host accepts: five rivals (a six-seat table) and everyone
/// else who turned up. How many of them get a seat is the lobby's
/// business, not the socket's.
///
/// One pool for the lot, sized for a watched final at a party: a table of
/// six and some thirty people round it. A peer in line is not sent the
/// round it is not watching (see `PeerBook::follows_by_plan`), so it costs
/// a roster line and a ping; a watcher costs the host one datagram a tick
/// (about 31 a second): with 26 watching, a soaked host sends 961 a
/// second (77 KB/s) in about 70 microseconds a tick, and a full socket
/// comes to about 1250.
///
/// The greeting past the cap is answered with [`NetMsg::Full`] rather than
/// dropped: a joiner that hears nothing reports the host as absent rather
/// than the beach as full.
pub const MAX_PEERS: usize = 40;

/// The largest datagram this build sends, and its receive buffer, which
/// must hold the largest message whole: UDP truncates a datagram to the
/// buffer given, and a truncated message fails decode silently. A test
/// proves every message fits. Raised from 512 when the host became able to
/// send a handmade beach with the invitation, and from 1024 when a tick's
/// inputs to each peer became one datagram (`write_pieces`).
///
/// Under any MTU worth worrying about: a datagram past the path MTU is
/// fragmented, and a fragment lost is the whole datagram lost, so a batch
/// that needed two IP packets would be a resend that fails twice as often
/// as the thing it repairs.
pub const MAX_DATAGRAM: usize = 1200;

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
pub const MAX_BEACH_BYTES: usize = 823;

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
    /// With the word each was turned away with, which it is told again.
    turned_away: Vec<(SocketAddr, NetMsg)>,
}

/// How many turned-away addresses a host remembers, oldest forgotten
/// first: enough for any evening, and bounded whoever keeps knocking.
const TURNED_AWAY: usize = 64;

/// How many times a closing socket says [`NetMsg::Leaving`]. It is sent at
/// a moment that does not come again, and one datagram is the one the
/// network eats; the silence rules stay as the backstop for a goodbye that
/// never arrives (a crash, a pulled cable).
const GOODBYES: usize = 3;

/// A socket that closes says so to everyone it has.
///
/// On drop rather than at each place a session or a beach is let go: a
/// socket is dropped exactly when this machine stops talking on it, and
/// never when it is handed on (into a round, back to the lobby), which
/// moves it. So no way out of a beach (Esc in the lobby, Enter on the
/// results card mid-series, the menu, the pause card, the window closing)
/// can forget to say goodbye, the same promise the beacon's farewell
/// makes.
impl Drop for UdpTransport {
    fn drop(&mut self) {
        for _ in 0..GOODBYES {
            self.send(NetMsg::Leaving);
        }
    }
}

/// This machine's own clock in milliseconds, for a [`NetMsg::Ping`]'s
/// stamp. Only the machine that wrote a stamp reads it back, so any origin
/// does: this one is the first time it is asked. Wraps after seven weeks,
/// which [`round_trip_since`] reads through.
pub fn ping_stamp() -> u32 {
    if let Some(ms) = STEERED_PING_CLOCK.get() {
        return ms;
    }
    static ORIGIN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let elapsed = ORIGIN.get_or_init(std::time::Instant::now).elapsed();
    // Truncated on purpose: the stamp is read back as a difference.
    elapsed.as_millis() as u32
}

thread_local! {
    /// The milliseconds [`ping_stamp`] reads on this thread in place of
    /// the machine's clock, while a harness steers it
    /// ([`steer_ping_clock`]). Never set in the game.
    static STEERED_PING_CLOCK: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

/// Read `ms` as the time for every [`ping_stamp`] on this thread: for a
/// harness that plays a table at virtual time (`examples/soak`), where a
/// round trip of a few virtual ticks is over in microseconds of the real
/// clock and would pick every next round the shortest delay there is.
/// Per thread, so tables played side by side keep their own time.
pub fn steer_ping_clock(ms: u32) {
    STEERED_PING_CLOCK.set(Some(ms));
}

/// The longest round trip a [`NetMsg::Pong`] is believed about. Anything
/// slower is a stamp from somewhere else (or from a ping long forgotten),
/// and a link that slow is played at the longest delay anyway.
const SLOWEST_ROUND_TRIP_MS: u32 = 5_000;

/// The seconds since [`ping_stamp`] read `stamp`, for the `Pong` that
/// carries it back, or `None` for a stamp this machine cannot have
/// written lately.
pub fn round_trip_since(stamp: u32) -> Option<f32> {
    let ms = ping_stamp().wrapping_sub(stamp);
    (ms <= SLOWEST_ROUND_TRIP_MS).then(|| ms as f32 / 1000.0)
}

/// A UDP socket on every interface at `port`, where 0 asks the system for
/// a free one: never one of the [`LOBBY_PORTS`]. They sit inside the range
/// the system hands out (32768 to 60999 on Linux), so a game socket or an
/// announcer could draw one and take a rung of the ladder from a lobby on
/// this machine, and a test's listener that drew one heard every real
/// beacon on the LAN.
pub fn bind_udp(port: u16) -> io::Result<UdpSocket> {
    match port {
        0 => off_the_ladder(&LOBBY_PORTS, || UdpSocket::bind(("0.0.0.0", 0))),
        port => UdpSocket::bind(("0.0.0.0", port)),
    }
}

/// The first socket `bind` gives whose port is not on `ladder`. The ones
/// turned down are held until then, so the system cannot hand the same
/// port straight back, and with every rung held the next one is off it.
fn off_the_ladder(
    ladder: &[u16],
    mut bind: impl FnMut() -> io::Result<UdpSocket>,
) -> io::Result<UdpSocket> {
    let mut turned_down = Vec::new();
    loop {
        let socket = bind()?;
        if !ladder.contains(&socket.local_addr()?.port()) {
            return Ok(socket);
        }
        turned_down.push(socket);
    }
}

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
        let socket = bind_udp(port)?;
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
        let socket = bind_udp(0)?;
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

    /// Send one batch, in as few datagrams as [`MAX_DATAGRAM`] allows
    /// (`write_pieces`), to every peer `hears` says is following the round.
    ///
    /// The audience is the caller's business and not the socket's: who is
    /// at the table, who is watching and who is in line for the next
    /// round is the lobby's bookkeeping. Encoded once, whoever hears it.
    pub fn send_batch(&self, batch: &Batch, mut hears: impl FnMut(usize) -> bool) {
        let audience: Vec<&SocketAddr> = self
            .peers
            .iter()
            .enumerate()
            .filter(|(index, _)| hears(*index))
            .map(|(_, peer)| peer)
            .collect();
        if audience.is_empty() {
            return;
        }
        write_pieces(batch, |bytes| {
            for peer in &audience {
                let _ = self.socket.send_to(bytes, peer);
            }
        });
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
        self.turn_away_saying(index, NetMsg::Kicked);
    }

    /// [`Self::turn_away`] with the word the peer is told and told again:
    /// [`NetMsg::NoBots`] for a bot at a beach not taking them.
    pub fn turn_away_saying(&mut self, index: usize, word: NetMsg) {
        let Some(&addr) = self.peers.get(index) else {
            return;
        };
        for _ in 0..3 {
            self.send_to(index, word.clone());
        }
        self.forget(index);
        if self.turned_away.len() >= TURNED_AWAY {
            self.turned_away.remove(0);
        }
        self.turned_away.push((addr, word));
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
    /// senders that greet become new peers on the host while capacity
    /// remains; anything else from them is dropped.
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
                    if let Some((_, word)) = self.turned_away.iter().find(|(gone, _)| *gone == from)
                    {
                        let _ = self.socket.send_to(&word.clone().encode(), from);
                        continue;
                    }
                    let index = match self.peers.iter().position(|p| *p == from) {
                        Some(index) => index,
                        // Only a greeting makes a newcomer (`NetMsg::greets`).
                        // Anything else from an address nobody greeted from
                        // is a stray, or a peer forgotten that will greet
                        // again. One `Pause` from anything on the LAN froze
                        // a running table, an `Inputs` was dealt a chair,
                        // and the second or third copy of a goodbye whose
                        // sender was forgotten on the first sat a nameless
                        // row at the table until the next sweep.
                        None if self.accept_new && msg.greets() => {
                            if self.peers.len() >= self.max_peers {
                                // No room, said so: once per greeting,
                                // which a joiner repeats once a second.
                                let _ = self.socket.send_to(&NetMsg::Full.encode(), from);
                                continue;
                            }
                            self.peers.push(from);
                            self.peers.len() - 1
                        }
                        None => continue,
                    };
                    out.push((msg, index));
                }
                // Nothing to take from this one, and more may be waiting.
                Err(e) if drain_goes_on(&e) => {}
                Err(_) => break,
            }
        }
        out
    }
}

/// Whether a failed `recv_from` leaves more to drain. `WouldBlock` is the
/// end of the queue; the two below are a *previous send* bouncing.
///
/// On Windows an unconnected UDP socket reports the ICMP "port
/// unreachable" that came back for an earlier datagram as an error on the
/// next receive. A host keeps sending to a peer that has just quit (until
/// it is aged out), so every tick would surface one, and stopping the
/// drain there left everybody else's inputs waiting in the queue for a
/// tick: the table stuttered for the one who left. The error is about
/// somebody else and says nothing about what is still waiting, so the
/// drain goes on. Linux never reports these on an unconnected socket.
pub(crate) fn drain_goes_on(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionRefused
    )
}

mod msg;
mod wire;
pub use msg::*;
pub use wire::*;

#[cfg(test)]
mod tests {
    use super::*;

    /// A system port that lands on the ladder is turned down, and held
    /// while the next is drawn: here the first two sockets drawn stand for
    /// lobby ports, and the third is the one kept.
    #[test]
    fn a_free_port_is_never_a_lobby_port() {
        let mut drawn: Vec<UdpSocket> = (0..3)
            .map(|_| UdpSocket::bind(("127.0.0.1", 0)).expect("bind"))
            .collect();
        let ports: Vec<u16> = drawn
            .iter()
            .map(|socket| socket.local_addr().expect("port").port())
            .collect();
        drawn.reverse();
        let mut draws = 0;
        let kept = off_the_ladder(&ports[..2], || {
            draws += 1;
            Ok(drawn.pop().expect("a socket left"))
        })
        .expect("a socket");
        assert_eq!(kept.local_addr().expect("port").port(), ports[2]);
        assert_eq!(draws, 3, "both ladder ports turned down");
        let socket = bind_udp(0).expect("bind");
        let port = socket.local_addr().expect("port").port();
        assert!(!LOBBY_PORTS.contains(&port), "{port} is a lobby port");
    }

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

    /// A goodbye from an address the socket does not know is dropped: the
    /// later copies of one whose sender was forgotten on the first used to
    /// be admitted as a brand new peer, a nameless row holding a slot.
    #[test]
    fn a_stray_goodbye_admits_nobody() {
        let mut host = UdpTransport::host(0).expect("bind");
        let port = host.local_addr().expect("addr").port();
        let stranger = UdpTransport::join(("127.0.0.1", port)).expect("join");
        for _ in 0..3 {
            stranger.send(NetMsg::Leaving);
        }
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            assert!(host.recv_all().is_empty(), "nothing to act on");
        }
        assert_eq!(host.peer_count(), 0, "and nobody new at the table");
        // Anything else from it is a greeting as ever.
        stranger.send(NetMsg::hello("Bo"));
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            if !host.recv_all().is_empty() {
                break;
            }
        }
        assert_eq!(host.peer_count(), 1);
    }

    /// Only a greeting seats a newcomer. Anything else that decodes from
    /// an address nobody greeted from is dropped: one stray `Pause` froze
    /// a running table, and a stray `Inputs` was given a lobby row and
    /// dealt a chair at the next round.
    #[test]
    fn only_a_greeting_admits_a_newcomer() {
        let mut host = UdpTransport::host(0).expect("bind");
        let port = host.local_addr().expect("addr").port();
        let stranger = UdpTransport::join(("127.0.0.1", port)).expect("join");
        stranger.send(NetMsg::Pause {
            round: 0,
            frame: 40,
        });
        stranger.send(NetMsg::Resume {
            round: 0,
            frame: 40,
        });
        stranger.send(NetMsg::Hash {
            round: 0,
            frame: 30,
            hash: 1,
        });
        stranger.send(NetMsg::chat("Bo", "hi"));
        stranger.send_batch(
            &Batch {
                inputs: vec![crate::sim::InputMsg {
                    player: 1,
                    frame: 3,
                    action: crate::sim::PlayerAction::None,
                }],
                ..Batch::default()
            },
            |_| true,
        );
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            assert!(host.recv_all().is_empty(), "nothing from a stranger");
        }
        assert_eq!(host.peer_count(), 0, "and nobody new at the table");
        // A greeting, to play or to watch, is how anybody comes in.
        let watcher = UdpTransport::join(("127.0.0.1", port)).expect("join");
        stranger.send(NetMsg::hello_bot("Greedy"));
        watcher.send(NetMsg::watch("Dee"));
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            host.recv_all();
            if host.peer_count() == 2 {
                break;
            }
        }
        assert_eq!(host.peer_count(), 2, "both greetings were taken");
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

    /// A `Pong` is timed by the stamp its `Ping` carried: read back at
    /// once it is a round trip of next to nothing, and a stamp this
    /// machine cannot have written lately says nothing at all.
    #[test]
    fn a_pong_is_timed_by_its_own_stamp() {
        let now = ping_stamp();
        assert!(round_trip_since(now).is_some_and(|seconds| seconds < 1.0));
        assert_eq!(round_trip_since(now.wrapping_add(60_000)), None);
        assert_eq!(round_trip_since(now.wrapping_sub(60_000)), None);
    }

    /// A socket that closes says so, to every peer it has, and one that
    /// is handed on (moved) says nothing.
    #[test]
    fn a_closing_socket_says_goodbye() {
        let mut host = UdpTransport::host(0).expect("bind");
        let port = host.local_addr().expect("addr").port();
        let joiner = UdpTransport::join(("127.0.0.1", port)).expect("join");
        joiner.send(NetMsg::hello("Bo"));
        let moved = joiner;
        std::thread::sleep(std::time::Duration::from_millis(20));
        let heard: Vec<NetMsg> = host.recv_all().into_iter().map(|(msg, _)| msg).collect();
        assert_eq!(heard, [NetMsg::hello("Bo")], "nothing said on the move");
        drop(moved);
        let mut goodbyes = 0;
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            goodbyes += host
                .recv_all()
                .into_iter()
                .filter(|(msg, peer)| *msg == NetMsg::Leaving && *peer == 0)
                .count();
            if goodbyes == GOODBYES {
                break;
            }
        }
        assert_eq!(goodbyes, GOODBYES);
    }

    /// A bounced earlier send (what Windows reports for a departed peer)
    /// does not end the drain; running out of datagrams, or a real fault,
    /// does.
    #[test]
    fn a_bounced_send_does_not_end_the_drain() {
        use io::ErrorKind::*;
        for kind in [ConnectionReset, ConnectionRefused] {
            assert!(drain_goes_on(&io::Error::from(kind)), "{kind:?}");
        }
        for kind in [WouldBlock, PermissionDenied, Other] {
            assert!(!drain_goes_on(&io::Error::from(kind)), "{kind:?}");
        }
    }

    /// The socket takes peers up to [`MAX_PEERS`] and refuses the rest,
    /// out loud and without the refused ones costing anything already
    /// connected.
    #[test]
    fn host_accepts_peers_up_to_its_cap() {
        let mut host = UdpTransport::host(0).expect("bind");
        let port = host.local_addr().expect("addr").port();
        let mut joiners: Vec<UdpTransport> = (0..MAX_PEERS + 2)
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
        std::thread::sleep(std::time::Duration::from_millis(20));
        let told: Vec<bool> = joiners
            .iter_mut()
            .map(|joiner| {
                joiner
                    .recv_all()
                    .iter()
                    .any(|(msg, _)| *msg == NetMsg::Full)
            })
            .collect();
        assert_eq!(
            told.iter().filter(|full| **full).count(),
            2,
            "and each of them is told the beach is full"
        );
    }
}
