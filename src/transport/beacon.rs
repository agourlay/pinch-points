//! The other protocol on the wire: how a beach is found in the first
//! place.
//!
//! Nothing here is the match protocol and none of its rules apply. There
//! is no version byte: the magic is the version, and the packet has only
//! ever grown at the tail, so a build that predates a field reads the ones
//! before it and ignores the rest. It is broadcast rather than addressed,
//! it goes to a fixed ladder of ports rather than an agreed one, and it is
//! the only traffic a machine sends before it knows anybody is listening.

use super::*;

// --- LAN lobby discovery ---------------------------------------------------

/// Ports the lobby listens on for host announcements. Several, so multiple
/// instances on one machine (loopback testing, shared houses) can all bind
/// one; announcers send to every port.
// Sized for a full table on one machine: five joiners listening at once
// (six seats less the host, who only announces), plus spares for a
// spectator or a lingering socket. The 4-port ladder this replaces was the
// four-player era's, and silently capped same-machine lobbies at three
// joiners.
pub const LOBBY_PORTS: [u16; 8] = [47700, 47701, 47702, 47703, 47704, 47705, 47706, 47707];
pub(super) const ANNOUNCE_MAGIC: &[u8; 5] = b"PNCH1";

/// What a beacon says about the beach it names.
///
/// The name and the occupancy hang off [`Beacon::Here`] because that is the
/// only place they mean anything: a farewell is matched by address, and
/// names a beach that is about to stop existing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Beacon {
    /// A beach on the air: who runs it, how full the table is, and whether
    /// the round has begun. Repeated on a timer for as long as it stands,
    /// so a player who was not listening yet can still find it.
    ///
    /// A `running` beach cannot be joined, since lockstep replays from
    /// frame zero and has nothing to catch a latecomer up with. It is still
    /// worth listing, because a player can queue for the next round if
    /// `taken` is short of `seats`.
    Here {
        /// Who is announcing, as opposed to from where. See [`Beacon::id`].
        id: u64,
        name: String,
        /// The player who put the beach up, under their own name, which is
        /// not the beach's. "Room 3" says nothing about whose room it is,
        /// and in a hall of eight games whose it is is what a player is
        /// actually looking for. Empty from a build that sent none.
        host: String,
        taken: u8,
        seats: u8,
        running: bool,
    },
    /// Going away for good: the host is leaving the lobby. Lobbies drop the
    /// beach on hearing it instead of waiting out the silence.
    Closing { id: u64 },
}

impl Beacon {
    /// Who is announcing: a number the host draws once and repeats in
    /// every beacon it sends.
    ///
    /// A beach is identified by this and not by the address it arrived
    /// from, because one beach reaches a listener from *two* addresses.
    /// Every beacon goes to the broadcast address and to loopback, because
    /// real broadcast does not always come back round to the machine that
    /// sent it and same-machine play has to work. So a second instance on
    /// the host's own machine hears both copies, one sourced from 127.0.0.1
    /// and one from the LAN address, and used to list the one game twice.
    /// It also spares the list from two machines that happen to draw the
    /// same ephemeral port.
    ///
    /// Zero from a build that announced no id, which is keyed by address as
    /// it always was.
    pub fn id(&self) -> u64 {
        match self {
            Beacon::Here { id, .. } | Beacon::Closing { id } => *id,
        }
    }

    /// Whether this beach has room for one more, running or not.
    pub fn has_room(&self) -> bool {
        match self {
            // A beacon too old to carry occupancy says nothing about its
            // table, and an unknown table is treated as having room: such a
            // build meant "join me" by announcing at all, and reading it as
            // full would hide it from the list entirely.
            Beacon::Here { seats: 0, .. } => true,
            Beacon::Here { taken, seats, .. } => taken < seats,
            Beacon::Closing { .. } => false,
        }
    }
}

/// The beacon kind, in byte 7; the beach's name in the [`WIRE_NAME`] bytes
/// after it; then the seats taken and the seats there are, the announcer's
/// id, and the name of the player who put the beach up. All appended
/// rather than folded into the magic, so a build that predates any of them
/// still reads the first seven bytes as the announcement they are: an old
/// listener ignores the tail, and a new one reads a short beacon as an
/// unnamed open beach, as such a build meant by it.
pub(super) const BEACON_OPEN: u8 = 0;
pub(super) const BEACON_CLOSING: u8 = 1;
pub(super) const BEACON_RUNNING: u8 = 2;
pub(super) const BEACON_NAME_AT: usize = 8;
pub(super) const BEACON_TAKEN_AT: usize = BEACON_NAME_AT + WIRE_NAME;
pub(super) const BEACON_SEATS_AT: usize = BEACON_TAKEN_AT + 1;
pub(super) const BEACON_ID_AT: usize = BEACON_SEATS_AT + 1;
/// The packet as it stood before the host's own name was added, and the
/// length a beacon has to reach for its table and its id to be believed.
/// A constant of its own rather than `BEACON_BYTES`, which is now longer:
/// reading the two against each other keeps a beacon from the build before
/// this one from losing everything but its beach name.
pub(super) const BEACON_TABLE_BYTES: usize = BEACON_ID_AT + 8;
pub(super) const BEACON_HOST_AT: usize = BEACON_TABLE_BYTES;
pub(super) const BEACON_BYTES: usize = BEACON_HOST_AT + WIRE_NAME;

/// The receive buffer for a beacon, which must hold the largest one whole:
/// UDP truncates a datagram to the buffer given, so a short buffer would
/// silently cost every beacon its name, the way a 64-byte buffer once ate
/// every named `Start`. A test proves the packet fits.
///
/// Grown past 64 when the host's name pushed the packet to 66. A build
/// still reading into 64 is unharmed by that: the kernel hands it the
/// first 64 bytes, which is every field it knew about anyway, and it goes
/// on reading them where they have always been. Growing at the tail is
/// what makes truncation survivable.
pub(super) const MAX_BEACON: usize = 96;

/// How many times a farewell goes out. An `Open` beacon can afford to be
/// dropped, since another follows in a second. A farewell is sent once, at
/// a moment that does not come again, and it does nothing at all if that
/// one datagram is the one the network eats.
pub(super) const FAREWELL_REPEATS: usize = 3;

/// How often a beacon goes out on every [`LOBBY_PORTS`] port rather than
/// on the first alone.
///
/// The ladder is there so several instances on *one machine* can each bind
/// a port, which is testing and shared houses, not a hall. On the air it
/// multiplied every announcement by eight: measured at 24 datagrams a
/// second per host, 16 of them broadcast, and eight beaches on the air came
/// to 192 a second between them. Broadcast is the expensive kind: every
/// machine on the network takes delivery of it, and on Wi-Fi it is sent at
/// the slowest rate every station can hear, so a hall's worth of it is
/// airtime taken from the games.
///
/// The loopback copies still go to every port every time, because that is
/// the case the ladder exists for and loopback costs the hall nothing. It
/// is only the broadcast half that thins, so what a second instance on this
/// machine hears does not change at all. A listener on another machine
/// holding a port past the first is a machine running several copies of the
/// game, and it now learns of a beach within this many seconds rather than
/// one.
pub(super) const WIDE_EVERY: u32 = 4;

/// How often the machine's own network is looked up again.
///
/// It was worked out once, when the announcer was made, on the reasoning
/// that a machine's address does not change while a beach stands. At a
/// party it does: a DHCP lease comes back on another subnet, or a laptop
/// roams to a second access point. The directed broadcast then names a
/// network this machine is no longer on, for as long as the process runs,
/// and every access point that drops the limited broadcast (the reason
/// the directed one is sent at all) stops carrying this beach. The host
/// sees nothing wrong: it is still announcing, and its own loopback copies
/// still come back.
///
/// Eight seconds is the compromise the original comment was after. Asking
/// costs a socket bound and connected, which sends nothing and takes
/// microseconds, so once every eight beacons is far from the "socket a
/// second" that was worth avoiding, and a beach that moved is findable
/// again within eight seconds rather than never.
pub(super) const SUBNET_RECHECK_EVERY: u32 = 8;

/// Whether this beacon is one of the ones that re-asks the routing table.
/// Round zero does not: `new` has just asked.
pub(super) fn due_for_recheck(round: u32) -> bool {
    round != 0 && round.is_multiple_of(SUBNET_RECHECK_EVERY)
}

/// The ports this beacon's *broadcast* half goes to: all of them on every
/// `WIDE_EVERY`th announcement, and on a farewell, which is sent once at a
/// moment that does not come again and has to reach every listener there
/// is. The first port otherwise.
pub(super) fn wide_ports(round: u32, farewell: bool) -> &'static [u16] {
    match farewell || round.is_multiple_of(WIDE_EVERY) {
        true => &LOBBY_PORTS,
        false => &LOBBY_PORTS[..1],
    }
}

/// A name a beacon carries at `at`, the beach's or its host's, and empty
/// from a build that sent none or a host that never typed one. Sanitized
/// like any other name off the wire, because that is what it is: bytes a
/// stranger on the LAN wrote.
pub(super) fn beacon_name(buf: &[u8], len: usize, at: usize) -> String {
    buf.get(at..at + WIRE_NAME)
        .filter(|_| len >= at + WIRE_NAME)
        .and_then(|bytes| WireName::try_from(bytes).ok())
        .map(|wire| name_from_wire(&wire))
        .unwrap_or_default()
}

/// What a beacon has to say about the beach it names: what it is called,
/// who put it up, and how full it is.
///
/// A struct rather than four loose arguments, two of them `&str`: nothing
/// in `announce(port, "Anna", "Room 3", 2, 6)` would complain if the two
/// names were the wrong way round, and every beach in the hall would
/// silently be listed under its host's name and hosted by its own.
#[derive(Clone, Copy, Default)]
pub struct OnAir<'a> {
    /// What the beach is called.
    pub name: &'a str,
    /// The player who put it up, under their own name.
    pub host: &'a str,
    pub taken: u8,
    pub seats: u8,
}

/// Listens for host announcements on one of the [`LOBBY_PORTS`].
pub struct Discovery {
    socket: UdpSocket,
}

impl Discovery {
    pub fn bind() -> io::Result<Discovery> {
        let mut last_err = io::Error::new(io::ErrorKind::AddrInUse, "no lobby port free");
        for port in LOBBY_PORTS {
            match UdpSocket::bind(("0.0.0.0", port)) {
                Ok(socket) => {
                    socket.set_nonblocking(true)?;
                    return Ok(Discovery { socket });
                }
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    /// Drain beacons: `(host address for joining, what it said)`.
    ///
    /// The address is assembled from two places, because neither half is
    /// available from one: the IP is the sender's, observed off the
    /// datagram, and the port is the one carried in the payload. The
    /// announcer socket is not the game socket, and a host has no reliable
    /// way to learn its own address to put in the packet.
    pub fn poll(&mut self) -> Vec<(SocketAddr, Beacon)> {
        let mut out = Vec::new();
        let mut buf = [0u8; MAX_BEACON];
        loop {
            match self.socket.recv_from(&mut buf) {
                Ok((len, from)) => {
                    if len >= 7 && &buf[..5] == ANNOUNCE_MAGIC {
                        let game_port = u16::from_le_bytes([buf[5], buf[6]]);
                        // No kind byte means a build from before farewells,
                        // whose exits are still caught by the timeout.
                        // Zero from a build that sent no id, which keys by
                        // address as it always did.
                        let id = match len >= BEACON_TABLE_BYTES {
                            true => u64::from_le_bytes(
                                buf[BEACON_ID_AT..BEACON_TABLE_BYTES]
                                    .try_into()
                                    .expect("eight bytes"),
                            ),
                            false => 0,
                        };
                        // The kind, and only from a packet that reached the
                        // byte it lives in. `buf` is read into once and
                        // reused for every datagram of the drain, so byte 7
                        // of a beacon too short to have one is byte 7 of
                        // whatever came before it: a pre-farewell beacon
                        // arriving behind a running one was listed as in
                        // progress, unjoinable, on the strength of the
                        // previous packet. Every other field already reads
                        // its length first; this one is the one that did not.
                        let kind = (len >= 8).then(|| buf[7]);
                        let beacon = match kind {
                            Some(BEACON_CLOSING) => Beacon::Closing { id },
                            kind => {
                                // Occupancy is the last thing appended, so a
                                // beacon too short to carry it reads as a
                                // table of unknown size, shown as open and
                                // never as full.
                                let (taken, seats) = match len >= BEACON_TABLE_BYTES {
                                    true => (buf[BEACON_TAKEN_AT], buf[BEACON_SEATS_AT]),
                                    false => (0, 0),
                                };
                                Beacon::Here {
                                    id,
                                    name: beacon_name(&buf, len, BEACON_NAME_AT),
                                    host: beacon_name(&buf, len, BEACON_HOST_AT),
                                    taken,
                                    seats,
                                    running: kind == Some(BEACON_RUNNING),
                                }
                            }
                        };
                        let mut host = from;
                        host.set_port(game_port);
                        out.push((host, beacon));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        out
    }
}

/// Broadcasts "there is a game at this port" for a hosting player.
pub struct Announcer {
    socket: UdpSocket,
    /// This host's own number, drawn once and repeated in every beacon.
    /// See [`Beacon::id`] for what it is for.
    id: u64,
    /// The broadcast address of the network this machine is actually on,
    /// where one can be worked out, re-asked every
    /// [`SUBNET_RECHECK_EVERY`] beacons. See [`subnet_broadcast`] for what
    /// it is, and [`Announcer::subnet`] for why it does not simply sit
    /// where `new` put it.
    ///
    /// Held as the address's bits, with zero for "none": `0.0.0.0` is not
    /// a broadcast address any network has, so it is free to mean absence,
    /// and an atomic keeps every send path on `&self`. A farewell goes out
    /// from `Drop`, which has nothing else.
    subnet: std::sync::atomic::AtomicU32,
    /// Beacons sent so far, which picks the ones that go out on the whole
    /// port ladder. See [`WIDE_EVERY`]. Atomic rather than `&mut`, because
    /// a farewell is sent from `Drop` with nothing but a shared borrow.
    sent: std::sync::atomic::AtomicU32,
}

/// The directed broadcast address of `ip`'s own network, assuming the /24
/// that a home or classroom LAN almost always is: `192.168.1.8` becomes
/// `192.168.1.255`.
///
/// A second address to shout at, not a replacement. `255.255.255.255` is
/// the correct one and is still sent first, but plenty of consumer access
/// points drop the limited broadcast while forwarding a directed one, and
/// a beach nobody can hear is a beach that does not exist. Where the guess
/// is wrong (a /16 site network) the packet simply reaches fewer machines
/// than it might, which is where we already were.
///
/// Only IPv4: there is no broadcast in IPv6, and the game's discovery has
/// never spoken it.
pub fn subnet_broadcast(ip: std::net::IpAddr) -> Option<std::net::Ipv4Addr> {
    let std::net::IpAddr::V4(ip) = ip else {
        return None;
    };
    let [a, b, c, _] = ip.octets();
    Some(std::net::Ipv4Addr::new(a, b, c, 255))
}

/// An address as the bits [`Announcer::subnet`] holds, and `0` for none.
fn bits_of(addr: Option<std::net::Ipv4Addr>) -> u32 {
    addr.map_or(0, u32::from)
}

impl Announcer {
    /// The network this machine is on right now, as the beacon should
    /// address it, or `None` where it has no route out of itself.
    ///
    /// Re-asked on the [`SUBNET_RECHECK_EVERY`] cadence rather than every
    /// beacon, and never on the send path's critical stretch: the lookup
    /// happens first, the answer is stored, and the sends read it back.
    fn subnet(&self, round: u32) -> Option<std::net::Ipv4Addr> {
        use std::sync::atomic::Ordering::Relaxed;
        if due_for_recheck(round) {
            let fresh = bits_of(crate::transport::local_ip().and_then(subnet_broadcast));
            self.subnet.store(fresh, Relaxed);
        }
        match self.subnet.load(Relaxed) {
            0 => None,
            bits => Some(std::net::Ipv4Addr::from(bits)),
        }
    }

    /// `id` is drawn by the caller, since this layer has no clock and no
    /// PRNG. It must differ between hosts and stay put for as long as one
    /// runs.
    ///
    /// This socket's own port is mixed in before it is used. The caller's
    /// number comes from a clock, and two instances started together on a
    /// machine whose clock is coarse would draw the same one, collapsing
    /// two beaches into a single row on the very machine somebody is
    /// testing with two windows open. An ephemeral port is the one thing
    /// the OS promises not to hand out twice at once.
    pub fn new(id: u64) -> io::Result<Announcer> {
        let socket = UdpSocket::bind(("0.0.0.0", 0))?;
        socket.set_broadcast(true)?;
        socket.set_nonblocking(true)?;
        let port = u64::from(socket.local_addr()?.port());
        Ok(Announcer {
            socket,
            id: id.rotate_left(16) ^ port,
            subnet: std::sync::atomic::AtomicU32::new(bits_of(
                crate::transport::local_ip().and_then(subnet_broadcast),
            )),
            sent: std::sync::atomic::AtomicU32::new(0),
        })
    }

    /// "There is a game at this port, and this is who is running it." Sent
    /// on a timer while the lobby stands. The name is what makes a hall of
    /// several beaches readable: an address says which machine, never whose
    /// game.
    pub fn announce(&self, game_port: u16, on_air: OnAir<'_>) {
        self.beacon(game_port, BEACON_OPEN, on_air, 1);
    }

    /// "That game has begun, and this is how full it is." A running beach
    /// stays on the air so a player can see it and queue for the next round
    /// rather than finding nothing and assuming the network is broken.
    pub fn running(&self, game_port: u16, on_air: OnAir<'_>) {
        self.beacon(game_port, BEACON_RUNNING, on_air, 1);
    }

    /// "That game is over." Sent when the host leaves the lobby or starts
    /// the match, so every list drops the beach at once instead of offering
    /// it until the silence adds up: a beach nobody can join any more.
    ///
    /// Best-effort by nature: a farewell cannot be sent by a process that
    /// was killed or a machine that lost power, which is why the timeout
    /// stays as the backstop rather than being replaced by this.
    pub fn closing(&self, game_port: u16) {
        // Nameless and tableless: a farewell is matched by address, and the
        // beach it names is about to stop existing anyway.
        self.beacon(
            game_port,
            BEACON_CLOSING,
            OnAir::default(),
            FAREWELL_REPEATS,
        );
    }

    /// To the whole LAN, to this machine's own network by name, and to
    /// localhost (loopback testing; real broadcast does not always loop
    /// back), on every lobby port.
    fn beacon(&self, game_port: u16, kind: u8, on_air: OnAir<'_>, times: usize) {
        let OnAir {
            name,
            host,
            taken,
            seats,
        } = on_air;
        let mut packet = ANNOUNCE_MAGIC.to_vec();
        packet.extend_from_slice(&game_port.to_le_bytes());
        packet.push(kind);
        packet.extend_from_slice(&wire_name(name));
        packet.push(taken);
        packet.push(seats);
        packet.extend_from_slice(&self.id.to_le_bytes());
        packet.extend_from_slice(&wire_name(host));
        debug_assert_eq!(packet.len(), BEACON_BYTES);
        let round = self.sent.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let wide = wide_ports(round, times > 1);
        let subnet = self.subnet(round);
        for _ in 0..times {
            for port in LOBBY_PORTS {
                if wide.contains(&port) {
                    let _ = self.socket.send_to(&packet, ("255.255.255.255", port));
                    if let Some(subnet) = subnet {
                        let _ = self.socket.send_to(&packet, (subnet, port));
                    }
                }
                // Every port, every time: this is the copy the ladder is
                // for, and it never leaves the machine.
                let _ = self.socket.send_to(&packet, ("127.0.0.1", port));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lookup cadence: not on the first beacon, because `new` has just
    /// asked, and every [`SUBNET_RECHECK_EVERY`] after.
    #[test]
    fn the_network_is_asked_about_again_on_a_cadence() {
        let due: Vec<u32> = (0..20).filter(|round| due_for_recheck(*round)).collect();
        assert_eq!(due, [8, 16]);
    }

    /// The bug this fixes: an address worked out once and kept for the life
    /// of the process. A lease that comes back on another subnet, or a
    /// laptop that roams, left the directed broadcast naming a network the
    /// machine had left, and the host went on announcing into it.
    ///
    /// Asserted as "the stale one does not survive" rather than against any
    /// particular address, since what this machine answers is this
    /// machine's business: the planted value is a documentation address
    /// (RFC 5737) that no routing table can hand back.
    #[test]
    fn a_subnet_that_went_stale_does_not_outlive_the_recheck() {
        use std::sync::atomic::Ordering::Relaxed;
        let announcer = Announcer::new(0x5AB1).expect("announcer");
        let stale = std::net::Ipv4Addr::new(203, 0, 113, 255);
        announcer.subnet.store(u32::from(stale), Relaxed);
        // A round that is not due leaves it exactly where it was, which is
        // what keeps this off the routing table every second.
        assert_eq!(announcer.subnet(1), Some(stale));
        assert_eq!(announcer.subnet(SUBNET_RECHECK_EVERY - 1), Some(stale));
        // A round that is due replaces it, whatever this machine answers.
        assert_ne!(
            announcer.subnet(SUBNET_RECHECK_EVERY),
            Some(stale),
            "a stale network outlived the recheck"
        );
    }

    /// What the hall pays for discovery, and what it stopped paying.
    ///
    /// The broadcast half is the half every machine on the network takes
    /// delivery of, so it is the half that has to be thin; the loopback
    /// copies are this machine's own business and stay on every port.
    #[test]
    fn only_every_fourth_beacon_shouts_at_the_whole_ladder() {
        let wide: Vec<usize> = (0..12).map(|n| wide_ports(n, false).len()).collect();
        assert_eq!(wide, [8, 1, 1, 1, 8, 1, 1, 1, 8, 1, 1, 1]);
        // Which is 2.9 broadcast datagrams a second where there were 16:
        // one port every second, all eight every fourth, twice over for
        // the limited and the directed address.
        let sent: usize = wide.iter().take(WIDE_EVERY as usize).sum::<usize>() * 2;
        assert_eq!(sent, 22, "per {WIDE_EVERY} seconds");
        // A farewell is never thinned: it is sent once, at a moment that
        // does not come again, and a listener that misses it keeps offering
        // a beach that has gone.
        for round in 0..12 {
            assert_eq!(wide_ports(round, true).len(), LOBBY_PORTS.len(), "{round}");
        }
    }

    /// The thinned beacon has to go to the port a lone listener holds, and
    /// that is the first: `Discovery::bind` walks [`LOBBY_PORTS`] in order
    /// and stops at the first one free, so the one machine in a hall
    /// running a single copy of the game is always on `LOBBY_PORTS[0]`.
    /// Sent anywhere else, a beach would be heard every fourth second
    /// instead of every second by everybody.
    ///
    /// Asserted rather than bound: every other test in this process is
    /// taking lobby ports too, so what `bind` answers here says nothing.
    #[test]
    fn the_thinned_beacon_goes_to_the_port_a_lone_listener_holds() {
        assert_eq!(wide_ports(1, false), &LOBBY_PORTS[..1]);
    }

    /// The second address a beacon is shouted at: this machine's own
    /// network, for the access points that forward a directed broadcast
    /// while dropping `255.255.255.255`.
    #[test]
    fn a_beacon_also_names_the_network_it_is_on() {
        let broadcast =
            |ip: &str| subnet_broadcast(ip.parse().expect("addr")).map(|addr| addr.to_string());
        assert_eq!(broadcast("192.168.1.8"), Some("192.168.1.255".into()));
        assert_eq!(broadcast("10.0.0.1"), Some("10.0.0.255".into()));
        // Only the last octet moves, whatever the first three are.
        assert_eq!(broadcast("10.1.2.3"), Some("10.1.2.255".into()));
        // Already a broadcast address: shouting at it is still right.
        assert_eq!(broadcast("192.168.1.255"), Some("192.168.1.255".into()));
        // There is no broadcast in IPv6, and discovery has never spoken it:
        // loopback, a global address and a v4-mapped one all decline.
        assert_eq!(broadcast("::1"), None);
        assert_eq!(broadcast("2001:db8::8"), None);
        assert_eq!(broadcast("::ffff:10.1.2.3"), None);
    }

    /// Hear a beacon, and hear what it said. The port comes from the
    /// payload and the address from the datagram, so the two halves have to
    /// meet correctly for a host to be joinable at all.
    #[test]
    fn discovery_hears_beacons_on_loopback() {
        let mut discovery = Discovery::bind().expect("bind lobby port");
        let announcer = Announcer::new(0xB0A7).expect("announcer");
        // Beacons go to every lobby port, and the rest of the suite is
        // announcing on this machine too: only the ones naming our own port
        // are ours to assert on.
        let heard = |discovery: &mut Discovery, send: &dyn Fn()| {
            let mut found = Vec::new();
            for _ in 0..20 {
                send();
                std::thread::sleep(std::time::Duration::from_millis(10));
                found.extend(
                    discovery
                        .poll()
                        .into_iter()
                        .filter(|(a, _)| a.port() == 48123),
                );
                if !found.is_empty() {
                    break;
                }
            }
            found
        };
        let told = |taken| OnAir {
            name: "Room 3",
            host: "Anna",
            taken,
            seats: 6,
        };
        let open = heard(&mut discovery, &|| announcer.announce(48123, told(3)));
        let (host, beacon) = open.first().expect("host discovered");
        assert_eq!(host.port(), 48123, "the port the payload named");
        // The id is the announcer's own, port mixed in, so it is compared
        // against what it says rather than against what was asked for.
        let Beacon::Here {
            id,
            name,
            host: whose,
            taken,
            seats,
            running,
        } = beacon.clone()
        else {
            panic!("an open beach, not {beacon:?}")
        };
        assert_eq!(
            (name.as_str(), whose.as_str(), taken, seats, running),
            ("Room 3", "Anna", 3, 6, false),
            "what the beach is called, whose it is and how full: none of \
             which an address says"
        );
        assert_ne!(id, 0, "and which beach, so two are never one row");

        // The same beach once the round has begun: still listed, no longer
        // joinable, and with a chair going spare to queue for.
        let running = heard(&mut discovery, &|| announcer.running(48123, told(5)));
        let (_, beacon) = running.first().expect("a running beach still says so");
        assert!(
            matches!(beacon, Beacon::Here { running: true, .. }),
            "{beacon:?}"
        );
        assert!(beacon.has_room(), "five of six seats is room for one more");

        // And the farewell, which spares every lobby the timeout.
        let bye = heard(&mut discovery, &|| announcer.closing(48123));
        let (host, beacon) = bye.first().expect("farewell heard");
        assert_eq!(host.port(), 48123);
        assert!(matches!(beacon, Beacon::Closing { .. }), "{beacon:?}");
    }

    /// Two hosts that drew the same number from the clock are still two
    /// hosts. The clock is the caller's, its resolution is the platform's,
    /// and two windows opened together on one machine is the everyday case,
    /// not the exotic one.
    #[test]
    fn two_announcers_from_one_clock_reading_are_still_two() {
        let (a, b) = (
            Announcer::new(0x1234_5678).expect("one"),
            Announcer::new(0x1234_5678).expect("two"),
        );
        assert_ne!(a.id, b.id, "same reading, different beaches");
    }

    /// A beacon must fit its receive buffer whole, or the kernel truncates
    /// it and the name goes missing without a trace, the failure the
    /// `Start` datagram already taught this file once. The widest name is
    /// the check that matters, since that is the part that grew.
    #[test]
    fn every_beacon_fits_the_receive_buffer() {
        let mut packet = ANNOUNCE_MAGIC.to_vec();
        packet.extend_from_slice(&u16::MAX.to_le_bytes());
        packet.push(BEACON_OPEN);
        packet.extend_from_slice(&wire_name("WWWWWWWWWWWWWWWWWWWWWWWW"));
        packet.push(6);
        packet.push(6);
        packet.extend_from_slice(&u64::MAX.to_le_bytes());
        packet.extend_from_slice(&wire_name("WWWWWWWWWWWWWWWWWWWWWWWW"));
        assert!(
            packet.len() <= MAX_BEACON,
            "{} bytes of beacon, {MAX_BEACON}-byte buffer",
            packet.len()
        );
        assert_eq!(packet.len(), BEACON_BYTES, "the whole layout");
    }

    /// The host's own name is the newest thing on the tail, and the build
    /// before it sent a packet that stopped at the id. Such a beacon must
    /// keep everything it *did* say (its beach name, its table, and above
    /// all its id, which stops one beach being listed twice) and lose only
    /// the name it never carried. Reading a length against `BEACON_BYTES`
    /// rather than [`BEACON_TABLE_BYTES`] is how it would lose the lot.
    #[test]
    fn a_beacon_from_before_the_host_had_a_name_keeps_the_rest() {
        let mut discovery = Discovery::bind().expect("bind lobby port");
        let socket = UdpSocket::bind(("0.0.0.0", 0)).expect("sender");
        // Byte for byte what the previous build put on the wire.
        let mut old = ANNOUNCE_MAGIC.to_vec();
        old.extend_from_slice(&48125u16.to_le_bytes());
        old.push(BEACON_OPEN);
        old.extend_from_slice(&wire_name("Room 3"));
        old.push(2);
        old.push(6);
        old.extend_from_slice(&0x5EA5u64.to_le_bytes());
        assert_eq!(old.len(), BEACON_TABLE_BYTES, "the packet as it was");
        let mut found = Vec::new();
        for _ in 0..20 {
            for port in LOBBY_PORTS {
                let _ = socket.send_to(&old, ("127.0.0.1", port));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            found.extend(
                discovery
                    .poll()
                    .into_iter()
                    .filter(|(a, _)| a.port() == 48125),
            );
            if !found.is_empty() {
                break;
            }
        }
        let (_, beacon) = found.first().expect("still a beacon");
        assert_eq!(
            *beacon,
            Beacon::Here {
                id: 0x5EA5,
                name: "Room 3".to_string(),
                host: String::new(),
                taken: 2,
                seats: 6,
                running: false,
            },
            "everything it said, and nothing invented for what it did not"
        );
    }

    /// The kind byte is an addition to a packet that shipped without one, so
    /// a beacon from a build that predates it still reads, as `Open`, which
    /// leaves such a host to be timed out the old way rather than never
    /// listed at all.
    #[test]
    fn a_beacon_without_a_kind_byte_still_says_here() {
        let mut discovery = Discovery::bind().expect("bind lobby port");
        let socket = UdpSocket::bind(("0.0.0.0", 0)).expect("sender");
        socket.set_broadcast(true).expect("broadcast");
        // Exactly what the old announcer put on the wire: magic and port.
        let mut old = ANNOUNCE_MAGIC.to_vec();
        old.extend_from_slice(&48124u16.to_le_bytes());
        assert_eq!(old.len(), 7, "the packet as it was before farewells");
        let mut found = Vec::new();
        for _ in 0..20 {
            for port in LOBBY_PORTS {
                let _ = socket.send_to(&old, ("127.0.0.1", port));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            found.extend(
                discovery
                    .poll()
                    .into_iter()
                    .filter(|(a, _)| a.port() == 48124),
            );
            if !found.is_empty() {
                break;
            }
        }
        let (host, beacon) = found.first().expect("an old beacon is still a beacon");
        assert_eq!(host.port(), 48124);
        assert_eq!(
            *beacon,
            Beacon::Here {
                id: 0,
                name: String::new(),
                host: String::new(),
                taken: 0,
                seats: 0,
                running: false,
            },
            "nameless and tableless, listed by address as it always was"
        );
        assert!(
            beacon.has_room(),
            "a table it never described is not a full one"
        );
    }

    /// One buffer serves every datagram of a drain, so a beacon too short
    /// to carry a field must not be read as carrying the last one's. The
    /// kind byte is where that bit: a pre-farewell beacon arriving behind
    /// a running one inherited its kind and was listed as a round already
    /// under way, which is a beach nobody may join.
    #[test]
    fn a_short_beacon_behind_a_long_one_borrows_none_of_it() {
        let mut discovery = Discovery::bind().expect("bind lobby port");
        let socket = UdpSocket::bind(("0.0.0.0", 0)).expect("sender");
        // A full beacon saying its round has begun, and behind it the
        // seven bytes a build from before farewells sends, both on the
        // wire before either is read.
        let mut running = ANNOUNCE_MAGIC.to_vec();
        running.extend_from_slice(&48126u16.to_le_bytes());
        running.push(BEACON_RUNNING);
        running.extend_from_slice(&wire_name("Room 3"));
        running.push(2);
        running.push(6);
        running.extend_from_slice(&0x5EA5u64.to_le_bytes());
        running.extend_from_slice(&wire_name("Anna"));
        assert_eq!(running.len(), BEACON_BYTES);
        let mut old = ANNOUNCE_MAGIC.to_vec();
        old.extend_from_slice(&48127u16.to_le_bytes());
        assert_eq!(old.len(), 7, "the packet as it was before farewells");

        let mut found = Vec::new();
        for _ in 0..20 {
            for port in LOBBY_PORTS {
                let _ = socket.send_to(&running, ("127.0.0.1", port));
                let _ = socket.send_to(&old, ("127.0.0.1", port));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            found.extend(
                discovery
                    .poll()
                    .into_iter()
                    .filter(|(a, _)| a.port() == 48127),
            );
            if !found.is_empty() {
                break;
            }
        }
        let (_, beacon) = found.first().expect("the short beacon is heard");
        assert_eq!(
            *beacon,
            Beacon::Here {
                id: 0,
                name: String::new(),
                host: String::new(),
                taken: 0,
                seats: 0,
                running: false,
            },
            "an open beach, not the kind of the packet before it"
        );
    }
}
