//! What one datagram says, and the bytes it says it in.
//!
//! Byte 0 tags the message and byte 1 is the [`PROTOCOL_VERSION`], frozen
//! there for all time so a build can tell "I cannot read this" from "I
//! disagree with this". Adding a tag is a version bump exactly as much as
//! changing a layout is, and it is the half that gets forgotten, because
//! nothing stops compiling when you do.

use super::*;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum NetMsg {
    /// Handshake ping; the host learns the peer's address (and what to
    /// call them) from it.
    Hello {
        name: WireName,
    },
    /// Handshake ping from a peer that wants to watch, not play. Repeated
    /// like `Hello` until a `Start` lands.
    Watch,
    Input(InputMsg),
    /// State fingerprint after `frame`, for loud desync detection.
    Hash {
        frame: u32,
        hash: u64,
    },
    /// Host → joiner: the match begins with `seats` seats on `terms`; you are
    /// `seat`, and the table is called `names` (empty entries fall back to
    /// seat labels). Re-sent whenever a joiner is still saying hello.
    Start {
        seats: u8,
        /// The seat this peer is given, or `None` for a peer that came to
        /// watch. On the wire that `None` is [`SPECTATOR_SEAT`], a number
        /// outside the range of real seats. In memory it is an absence,
        /// which is what a watcher is, and what the launch plan beside it
        /// has called one all along.
        seat: Option<u8>,
        terms: MatchTerms,
        names: [WireName; crate::sim::MAX_PLAYERS],
        /// Where the series stands as this round begins: its 1-based
        /// number, and the rounds each *seat* has won so far. Zero and
        /// empty for a single round.
        ///
        /// The host says, because seats move: a peer that leaves between
        /// rounds frees its chair and everyone behind it moves up one, so
        /// a tally each peer kept by seat number credited the departed
        /// player's rounds to whoever moved into the seat. The host holds
        /// the mapping and re-deals the tally with the chairs; a peer
        /// admitted from the queue mid-series learns the standings the
        /// same way, rather than starting a series of its own.
        round: u8,
        wins: [u8; crate::sim::MAX_PLAYERS],
        /// The beach itself, when the host picked one it built rather than
        /// one both peers already have. A generated arena travels as a
        /// seed; a handmade one has to travel as itself, because the
        /// joiner has never seen the file. Empty for the built-in maps.
        ///
        /// Compressed with the same coder the share codes use, and it has
        /// to be: a 20x13 beach is fifteen hundred characters of text and
        /// the datagram is a kilobyte.
        beach: Vec<u8>,
    },
    /// Someone hit pause: everybody stops committing at `frame`. Re-sent
    /// every tick while paused, so a dropped datagram costs a moment of
    /// confusion rather than a stuck match.
    Pause {
        frame: u32,
    },
    /// Play on: the pause that was to freeze on `frame` is lifted. Also
    /// re-sent until the sim visibly moves again. The frame is what lets a
    /// peer tell the `Pause` echoes still in flight from before the resume
    /// (see `Lockstep::receive_pause`) from a fresh pause.
    Resume {
        frame: u32,
    },
    /// Host → the table: `seat` has stopped sending and an AI is taking
    /// the chair.
    ///
    /// Only the host says so, and everyone acts on its word rather than on
    /// their own patience. A peer that decided for itself would fill the
    /// seat on whichever frame its own timer happened to run out, and two
    /// peers filling it on different frames is a desync, which lockstep has
    /// no way to recover from.
    ///
    /// `frame` is the one the host was held up on, and every peer empties
    /// the seat from there: they do not all hold the same inputs from a
    /// player who has gone quiet (the host relays each as it arrives, and
    /// a peer that missed the relay of one may hold a later one), so
    /// "from the frame you are stuck on" is not the same frame everywhere.
    /// Repeated for the rest of the round, since a lost one leaves that
    /// peer frozen while the others play on.
    Abandoned {
        seat: u8,
        frame: u32,
    },
    /// Host → the lobby: who is at the table right now, in seat order.
    ///
    /// A joiner has only ever spoken to the host, so without this it knows
    /// nobody else is even there until the match starts. Re-sent whenever
    /// the table changes and on a timer besides, since a roster that went
    /// missing would leave a screen wrong for good rather than briefly.
    Roster {
        seats: u8,
        names: [WireName; crate::sim::MAX_PLAYERS],
        /// The host's dials as they stand, so a joiner's card shows the
        /// match it is joining rather than its own setup screen's idea of
        /// one. Everything but the seed is meaningful before the launch.
        terms: MatchTerms,
    },
    /// A line said in the lobby, and who said it. The sender names itself
    /// rather than the host stamping it: a joiner's greeting is the only
    /// other place the host learns a name, and a peer that never greeted
    /// would otherwise speak anonymously.
    ///
    /// Relayed by the host to the rest of the table, like an input: the
    /// spokes of the star cannot hear each other.
    Chat {
        name: WireName,
        text: WireChat,
    },
    /// Host → a peer that turned up after the launch: the round is under
    /// way and cannot take you, but you are in line for the next one, with
    /// `ahead` people in front of you.
    ///
    /// The answer to a greeting that used to be met with a spectator seat,
    /// which was worse than useless: lockstep replays from frame zero, so
    /// such a peer built a board nobody would ever send it inputs for and
    /// sat there, apparently connected, forever.
    Queued {
        ahead: u8,
    },
    /// "I speak protocol `version`, and what you sent me is not it." The
    /// answer to a datagram from another build, so a mismatched joiner is
    /// told why nothing is happening instead of greeting a host that ignores
    /// it forever.
    ///
    /// The one message exempt from the version gate, and the one whose layout
    /// is frozen along with the version byte: every build, past and future,
    /// can read `[TAG_INCOMPATIBLE, version]`.
    Incompatible {
        version: u8,
    },
}

impl NetMsg {
    /// A greeting carrying `name` in wire form.
    pub fn hello(name: &str) -> NetMsg {
        NetMsg::Hello {
            name: wire_name(name),
        }
    }
}

// Byte 0 of every datagram.
//
// Adding a line here is a `PROTOCOL_VERSION` bump, exactly as much as
// changing the layout of an existing message is, and it is the half that
// gets forgotten, because nothing stops compiling when you do. Two builds
// both claiming the same version, one of them sending a tag the other has
// never heard of, is the silent disagreement that byte exists to prevent:
// the older one reads it as noise and simply never acts on it.
// Chat, the roster and the abandonment notice all shipped under version 4
// before anybody noticed.
//
// So a new tag is three lines, not one: the tag, `HIGHEST_TAG` below it,
// and the version.
const TAG_HELLO: u8 = 0;
const TAG_INPUT: u8 = 1;
const TAG_HASH: u8 = 2;
const TAG_START: u8 = 3;
const TAG_PAUSE: u8 = 4;
const TAG_RESUME: u8 = 5;
const TAG_WATCH: u8 = 6;
const TAG_INCOMPATIBLE: u8 = 7;
const TAG_QUEUED: u8 = 8;
const TAG_CHAT: u8 = 9;
const TAG_ROSTER: u8 = 10;
const TAG_ABANDONED: u8 = 11;
/// The last of them, which `peek_version` uses to tell one of ours from
/// stray traffic on the port. Kept here rather than written into that
/// check, so the line to update sits directly under the line being added.
const HIGHEST_TAG: u8 = TAG_ABANDONED;

/// How a peer that came to watch is written down in a `Start`: outside
/// the range of real seats, so it cannot collide with one.
///
/// A wire detail and nothing more. Everything above this file says `None`,
/// and the two are only ever exchanged in the codec below.
const SPECTATOR_SEAT: u8 = u8::MAX;

/// Everything about a match that every peer has to agree on, or the boards
/// diverge (or, for `teams`, two peers score the same round differently).
///
/// Held as plain numbers rather than the app's enums: this layer is the wire
/// and knows nothing about menus. The app maps them at the edge.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MatchTerms {
    /// Seats the AI holds, counting down from the top seat.
    pub bots: u8,
    /// AI difficulty index (easy/normal/fierce).
    pub bot_level: u8,
    /// Map choice index.
    pub map: u8,
    /// Gull pressure index.
    pub gulls: u8,
    /// Round length index.
    pub round: u8,
    /// How the round is scored, as a team-mode index (free-for-all, pairs,
    /// trios). A byte rather than a flag since 2026-07-30, when teams stopped
    /// being only 2v2.
    pub teams: u8,
    /// The board's PRNG seed, so every peer builds the same beach. Also
    /// what tells a fresh `Start` from the stale one a host re-answers a
    /// stray greeting with: a new seed is a new round.
    pub seed: u64,
    /// Best-of-5 rather than a single round. Every peer keeps its own
    /// tally, and they agree because they are counting the same
    /// deterministic boards, but only if they all know it is a series.
    pub series: u8,
}

impl MatchTerms {
    const BYTES: usize = 15;

    fn encode(self) -> [u8; Self::BYTES] {
        let mut out = [0u8; Self::BYTES];
        out[0] = self.bots;
        out[1] = self.bot_level;
        out[2] = self.map;
        out[3] = self.gulls;
        out[4] = self.round;
        out[5] = self.teams;
        out[6..14].copy_from_slice(&self.seed.to_le_bytes());
        out[14] = self.series;
        out
    }

    fn decode(bytes: &[u8]) -> Option<MatchTerms> {
        let seed = u64::from_le_bytes(bytes.get(6..14)?.try_into().ok()?);
        Some(MatchTerms {
            bots: *bytes.first()?,
            bot_level: *bytes.get(1)?,
            map: *bytes.get(2)?,
            gulls: *bytes.get(3)?,
            round: *bytes.get(4)?,
            teams: *bytes.get(5)?,
            seed,
            series: *bytes.get(14)?,
        })
    }
}

impl NetMsg {
    pub fn encode(self) -> Vec<u8> {
        // Byte 0 tags the message, byte 1 says who wrote it; the payload
        // starts at byte 2.
        let mut bytes = match self {
            NetMsg::Hello { .. } => vec![TAG_HELLO],
            NetMsg::Watch => vec![TAG_WATCH],
            NetMsg::Queued { .. } => vec![TAG_QUEUED],
            NetMsg::Chat { .. } => vec![TAG_CHAT],
            NetMsg::Roster { .. } => vec![TAG_ROSTER],
            NetMsg::Abandoned { .. } => vec![TAG_ABANDONED],
            NetMsg::Input(_) => vec![TAG_INPUT],
            NetMsg::Hash { .. } => vec![TAG_HASH],
            NetMsg::Start { .. } => vec![TAG_START],
            NetMsg::Pause { .. } => vec![TAG_PAUSE],
            NetMsg::Resume { .. } => vec![TAG_RESUME],
            NetMsg::Incompatible { version } => return vec![TAG_INCOMPATIBLE, version],
        };
        bytes.push(PROTOCOL_VERSION);
        match self {
            NetMsg::Watch | NetMsg::Incompatible { .. } => {}
            NetMsg::Resume { frame } => bytes.extend_from_slice(&frame.to_le_bytes()),
            NetMsg::Queued { ahead } => bytes.push(ahead),
            NetMsg::Abandoned { seat, frame } => {
                bytes.push(seat);
                bytes.extend_from_slice(&frame.to_le_bytes());
            }
            NetMsg::Chat { name, text } => {
                bytes.extend_from_slice(&name);
                bytes.extend_from_slice(&text);
            }
            NetMsg::Roster {
                seats,
                names,
                terms,
            } => {
                bytes.push(seats);
                for name in names {
                    bytes.extend_from_slice(&name);
                }
                bytes.extend_from_slice(&terms.encode());
            }
            NetMsg::Hello { name } => bytes.extend_from_slice(&name),
            NetMsg::Input(msg) => bytes.extend_from_slice(&msg.encode()),
            NetMsg::Hash { frame, hash } => {
                bytes.extend_from_slice(&frame.to_le_bytes());
                bytes.extend_from_slice(&hash.to_le_bytes());
            }
            NetMsg::Start {
                seats,
                seat,
                terms,
                names,
                round,
                wins,
                beach,
            } => {
                bytes.push(seats);
                bytes.push(seat.unwrap_or(SPECTATOR_SEAT));
                bytes.extend_from_slice(&terms.encode());
                for name in names {
                    bytes.extend_from_slice(&name);
                }
                bytes.push(round);
                bytes.extend_from_slice(&wins);
                // Length-prefixed and last, so the fixed part above stays
                // where it was and a beach of any size is one read.
                let len = u16::try_from(beach.len()).unwrap_or(0);
                bytes.extend_from_slice(&len.to_le_bytes());
                bytes.extend_from_slice(&beach[..usize::from(len)]);
            }
            NetMsg::Pause { frame } => bytes.extend_from_slice(&frame.to_le_bytes()),
        }
        bytes
    }

    /// The protocol version a datagram was written by, whatever else it
    /// says, and `None` for anything that is not one of our messages at
    /// all. Byte 1 is frozen across versions so this always answers, and
    /// the tag is checked first so stray traffic on the port draws no reply.
    pub fn peek_version(bytes: &[u8]) -> Option<u8> {
        let tag = *bytes.first()?;
        (tag <= HIGHEST_TAG)
            .then(|| bytes.get(1).copied())
            .flatten()
    }

    /// Decode a datagram this build can act on. A message from another
    /// protocol version is refused here rather than half-understood. The
    /// sole exception is the refusal message itself, which every version
    /// can read by construction.
    pub fn decode(bytes: &[u8]) -> Option<NetMsg> {
        let tag = *bytes.first()?;
        let version = *bytes.get(1)?;
        if tag == TAG_INCOMPATIBLE {
            return Some(NetMsg::Incompatible { version });
        }
        if version != PROTOCOL_VERSION {
            return None;
        }
        let body = bytes.get(2..)?;
        match tag {
            TAG_HELLO => Some(NetMsg::Hello {
                name: body.get(..WIRE_NAME)?.try_into().ok()?,
            }),
            TAG_WATCH => Some(NetMsg::Watch),
            TAG_INPUT => {
                let payload: [u8; INPUT_BYTES] = body.get(..INPUT_BYTES)?.try_into().ok()?;
                Some(NetMsg::Input(InputMsg::decode(payload)))
            }
            TAG_HASH => {
                let frame = u32::from_le_bytes(body.get(..4)?.try_into().ok()?);
                let hash = u64::from_le_bytes(body.get(4..12)?.try_into().ok()?);
                Some(NetMsg::Hash { frame, hash })
            }
            TAG_START => {
                let terms = MatchTerms::decode(body.get(2..)?)?;
                let mut names = [[0u8; WIRE_NAME]; crate::sim::MAX_PLAYERS];
                let table = body.get(2 + MatchTerms::BYTES..)?;
                for (i, name) in names.iter_mut().enumerate() {
                    *name = table
                        .get(i * WIRE_NAME..(i + 1) * WIRE_NAME)?
                        .try_into()
                        .ok()?;
                }
                let (seats, seat) = (*body.first()?, *body.get(1)?);
                // A table this build cannot sit at is refused outright
                // rather than squeezed into range. Every per-seat array is
                // `MAX_PLAYERS` long and the seat number goes on to index
                // the lockstep's own slots, so a `Start` naming more seats
                // than there are chairs, or seating us at one that is not
                // at the table, is not a message to act on. Watching is the
                // one seat legitimately outside the range.
                //
                // The AI holds the top seats, so the humans are the low
                // `seats - bots` of them, and that is the range the joiner
                // builds its lockstep from: a `Start` seating us in an AI's
                // chair would have it play a session it is not a player of,
                // which the lockstep refuses with a panic. Refused here
                // instead, with the rest of the unplayable tables.
                let humans = seats.saturating_sub(terms.bots).max(1);
                if !(2..=crate::sim::MAX_PLAYERS as u8).contains(&seats)
                    || (seat != SPECTATOR_SEAT && seat >= humans)
                {
                    return None;
                }
                let seat = (seat != SPECTATOR_SEAT).then_some(seat);
                debug_assert!(seat.is_none_or(|seat| seat < seats));
                let series_at = 2 + MatchTerms::BYTES + WIRE_NAME * crate::sim::MAX_PLAYERS;
                let round = *body.get(series_at)?;
                let wins: [u8; crate::sim::MAX_PLAYERS] = body
                    .get(series_at + 1..series_at + 1 + crate::sim::MAX_PLAYERS)?
                    .try_into()
                    .ok()?;
                let after = series_at + 1 + crate::sim::MAX_PLAYERS;
                let beach = match body.get(after..after + 2) {
                    Some(len) => {
                        let len = usize::from(u16::from_le_bytes(len.try_into().ok()?));
                        body.get(after + 2..after + 2 + len)?.to_vec()
                    }
                    None => Vec::new(),
                };
                Some(NetMsg::Start {
                    seats,
                    seat,
                    terms,
                    names,
                    round,
                    wins,
                    beach,
                })
            }
            TAG_PAUSE => Some(NetMsg::Pause {
                frame: u32::from_le_bytes(body.get(..4)?.try_into().ok()?),
            }),
            TAG_RESUME => Some(NetMsg::Resume {
                frame: u32::from_le_bytes(body.get(..4)?.try_into().ok()?),
            }),
            TAG_QUEUED => Some(NetMsg::Queued {
                ahead: *body.first()?,
            }),
            TAG_ABANDONED => {
                let seat = *body.first()?;
                let frame = u32::from_le_bytes(body.get(1..5)?.try_into().ok()?);
                // A seat outside the table is not one anything can be done
                // about, and acting on it would index off the end.
                (seat < crate::sim::MAX_PLAYERS as u8).then_some(NetMsg::Abandoned { seat, frame })
            }
            TAG_ROSTER => {
                let table = body.get(1..)?;
                let mut names = [[0u8; WIRE_NAME]; crate::sim::MAX_PLAYERS];
                for (i, name) in names.iter_mut().enumerate() {
                    *name = table
                        .get(i * WIRE_NAME..(i + 1) * WIRE_NAME)?
                        .try_into()
                        .ok()?;
                }
                let terms = MatchTerms::decode(table.get(WIRE_NAME * crate::sim::MAX_PLAYERS..)?)?;
                Some(NetMsg::Roster {
                    seats: *body.first()?,
                    names,
                    terms,
                })
            }
            TAG_CHAT => Some(NetMsg::Chat {
                name: body.get(..WIRE_NAME)?.try_into().ok()?,
                text: body
                    .get(WIRE_NAME..WIRE_NAME + WIRE_CHAT)?
                    .try_into()
                    .ok()?,
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every message must encode within the receive buffer, or the kernel
    /// truncates it on arrival and decode drops it without a trace, which
    /// is how a 64-byte buffer once swallowed the named `Start` whole.
    #[test]
    fn every_message_fits_the_receive_buffer() {
        let widest = super::wire_name("WWWWWWWWWWWWWWWWWWWWWWWW");
        for msg in [
            super::NetMsg::Hello { name: widest },
            super::NetMsg::Chat {
                name: widest,
                text: super::wire_chat(&"W".repeat(super::CHAT_CHARS)),
            },
            super::NetMsg::Start {
                seats: 6,
                seat: Some(5),
                terms: super::MatchTerms::default(),
                names: [widest; crate::sim::MAX_PLAYERS],
                // The widest a Start gets: a full table of longest names
                // and the largest beach the sender will hand it.
                round: 0,
                wins: [0; crate::sim::MAX_PLAYERS],
                beach: vec![0xAB; super::MAX_BEACH_BYTES],
            },
            super::NetMsg::Hash {
                frame: u32::MAX,
                hash: u64::MAX,
            },
        ] {
            let len = msg.clone().encode().len();
            assert!(len <= super::MAX_DATAGRAM, "{len} bytes: {msg:?}");
        }
    }

    /// [`MAX_BEACH_BYTES`] is what the sender trusts, so it has to be a
    /// number this encoder agrees with: the widest possible invitation
    /// carrying the largest allowed beach must still fit the buffer, with
    /// room to spare for a field somebody adds to `Start` later.
    #[test]
    fn a_start_carrying_the_largest_beach_still_fits() {
        let widest = super::wire_name("WWWWWWWWWWWWWWWWWWWWWWWW");
        let len = super::NetMsg::Start {
            seats: 6,
            seat: Some(5),
            terms: super::MatchTerms::default(),
            names: [widest; crate::sim::MAX_PLAYERS],
            round: 0,
            wins: [0; crate::sim::MAX_PLAYERS],
            beach: vec![0xAB; super::MAX_BEACH_BYTES],
        }
        .encode()
        .len();
        assert!(len <= super::MAX_DATAGRAM, "{len} bytes");
        let spare = super::MAX_DATAGRAM - len;
        assert!(spare >= 16, "only {spare} bytes of slack left");
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(super::NetMsg::decode(&[]).is_none());
        assert!(super::NetMsg::decode(&[0xFF, 1, 2, 3]).is_none());
        assert!(super::NetMsg::decode(b"PNCH?").is_none());
    }

    use crate::sim::{Direction, PlayerAction};

    #[test]
    fn messages_round_trip() {
        for msg in [
            NetMsg::hello("Anna"),
            NetMsg::hello("Überlang-Name-über-die-Kappe-hinaus"),
            NetMsg::Watch,
            NetMsg::Resume { frame: 0 },
            NetMsg::Resume { frame: 70_000 },
            NetMsg::Pause { frame: 7 },
            NetMsg::Input(InputMsg {
                player: 1,
                frame: 42,
                action: PlayerAction::Place {
                    x: 3,
                    y: 8,
                    dir: Direction::Down,
                },
            }),
            // The XL beach is 20 wide: a placement out in column 18 has to
            // survive the trip, which it did not while a tile was a nibble.
            NetMsg::Input(InputMsg {
                player: 5,
                frame: 4000,
                action: PlayerAction::Place {
                    x: 18,
                    y: 11,
                    dir: Direction::Left,
                },
            }),
            NetMsg::Hash {
                frame: 990,
                hash: 0xDEAD_BEEF_0BAD_F00D,
            },
            NetMsg::Start {
                seats: 6,
                seat: Some(3),
                terms: MatchTerms {
                    bots: 2,
                    teams: 1,
                    seed: 0x1234_5678_9ABC_DEF0,
                    ..MatchTerms::default()
                },
                names: std::array::from_fn(|i| wire_name(&format!("Seat {i}"))),
                round: 0,
                wins: [0; crate::sim::MAX_PLAYERS],
                beach: b"a handmade beach".to_vec(),
            },
            NetMsg::Incompatible { version: 9 },
            NetMsg::Queued { ahead: 0 },
            NetMsg::Queued { ahead: 4 },
            NetMsg::Chat {
                name: wire_name("Anna"),
                text: wire_chat("wait for me!"),
            },
            NetMsg::Roster {
                seats: 4,
                names: std::array::from_fn(|i| wire_name(&format!("P{i}"))),
                terms: MatchTerms {
                    bots: 1,
                    map: 3,
                    ..MatchTerms::default()
                },
            },
            NetMsg::Abandoned { seat: 0, frame: 0 },
            NetMsg::Abandoned {
                seat: crate::sim::MAX_PLAYERS as u8 - 1,
                frame: 123_456,
            },
        ] {
            assert_eq!(NetMsg::decode(&msg.clone().encode()), Some(msg));
        }
    }

    /// A `Start` is the one message that sizes the table, and a joiner acts
    /// on it before anything else has looked at it: the seat count becomes
    /// the length every per-seat loop runs to, and the seat number indexes
    /// the lockstep's slots directly. Both are refused here rather than
    /// trusted, since a host on a broken build can say anything.
    #[test]
    fn a_start_that_seats_more_than_the_table_is_refused() {
        let good = NetMsg::Start {
            seats: 6,
            seat: Some(5),
            terms: MatchTerms::default(),
            names: [[0u8; WIRE_NAME]; crate::sim::MAX_PLAYERS],
            round: 0,
            wins: [0; crate::sim::MAX_PLAYERS],
            beach: Vec::new(),
        };
        assert_eq!(NetMsg::decode(&good.clone().encode()), Some(good.clone()));
        // Byte 2 is `seats` and byte 3 is `seat`, just past the tag and
        // version. Nothing outside the table survives the trip.
        for (seats, seat) in [(7, 0), (255, 0), (1, 0), (0, 0), (4, 4), (4, 200)] {
            let mut bytes = good.clone().encode();
            bytes[2] = seats;
            bytes[3] = seat;
            assert_eq!(NetMsg::decode(&bytes), None, "{seats} seats, sat at {seat}");
        }
        // Watching is outside the seat range on purpose and stays legal,
        // and comes back as the absence it means rather than as the number
        // it travelled in.
        let mut watching = good.clone().encode();
        watching[3] = SPECTATOR_SEAT;
        assert!(matches!(
            NetMsg::decode(&watching),
            Some(NetMsg::Start { seat: None, .. })
        ));
        // An AI's chair is not one a joiner can be sat in either: the
        // lockstep it builds carries only the humans, and being seated
        // outside it was a panic, not a refusal.
        let with_bots = |seats, bots, seat| {
            NetMsg::decode(
                &NetMsg::Start {
                    seats,
                    seat: Some(seat),
                    terms: MatchTerms {
                        bots,
                        ..MatchTerms::default()
                    },
                    names: [[0u8; WIRE_NAME]; crate::sim::MAX_PLAYERS],
                    round: 0,
                    wins: [0; crate::sim::MAX_PLAYERS],
                    beach: Vec::new(),
                }
                .encode(),
            )
        };
        assert!(with_bots(6, 4, 1).is_some(), "the last human seat");
        assert!(with_bots(6, 4, 2).is_none(), "the first AI seat");
        assert!(with_bots(6, 4, 3).is_none());
        assert!(with_bots(6, 6, 1).is_none(), "more bots than chairs");
        assert!(with_bots(6, 6, 0).is_some(), "seat zero always stands");
    }

    /// The whole point of the version byte: a datagram from another build is
    /// refused rather than half-understood.
    #[test]
    fn another_build_is_refused_and_told_so() {
        let mut hello = NetMsg::hello("Anna").encode();
        assert_eq!(hello[1], PROTOCOL_VERSION, "the version byte is byte 1");
        hello[1] = PROTOCOL_VERSION.wrapping_add(1);
        assert_eq!(NetMsg::decode(&hello), None, "not ours, so not acted on");
        assert_eq!(
            NetMsg::peek_version(&hello),
            Some(PROTOCOL_VERSION.wrapping_add(1)),
            "but it can still be identified, which is what gets it answered"
        );
        // The refusal itself is exempt from the gate: it is the one message
        // every version must be able to read, whatever the sender speaks.
        let refusal = NetMsg::Incompatible { version: 77 }.encode();
        assert_eq!(refusal.len(), 2, "its layout is frozen at two bytes");
        assert_eq!(
            NetMsg::decode(&refusal),
            Some(NetMsg::Incompatible { version: 77 })
        );
        // Stray traffic on the port is not a version clash and draws no reply.
        assert_eq!(NetMsg::peek_version(&[0xFE, 3]), None);
        assert_eq!(NetMsg::peek_version(b"PNCH1"), None, "an announcement");
    }
}

#[cfg(test)]
mod wire_fuzz_probe {
    use super::*;

    /// Every byte string a hostile or broken peer could put on the port,
    /// through both decoders. Neither may panic, hang, or allocate wildly:
    /// this is the one surface the LAN can reach without being invited.
    #[test]
    fn no_datagram_can_break_a_decoder() {
        let mut rng = crate::sim::Pcg32::new(0xDEAD_BEEF, 0x1357);
        // Seed with real messages, so mutations land near valid ones.
        let mut seeds: Vec<Vec<u8>> = vec![
            NetMsg::hello("Anna").encode(),
            NetMsg::Watch.encode(),
            NetMsg::Queued { ahead: 3 }.encode(),
            NetMsg::Chat {
                name: wire_name("Bo"),
                text: wire_chat("ready?"),
            }
            .encode(),
            NetMsg::Start {
                seats: 6,
                seat: Some(2),
                terms: MatchTerms::default(),
                names: [[0u8; WIRE_NAME]; crate::sim::MAX_PLAYERS],
                round: 0,
                wins: [0; crate::sim::MAX_PLAYERS],
                beach: Vec::new(),
            }
            .encode(),
            NetMsg::Hash { frame: 7, hash: 9 }.encode(),
            ANNOUNCE_MAGIC.to_vec(),
            Vec::new(),
        ];
        // And a beacon, built the way the announcer builds one.
        let mut beacon = ANNOUNCE_MAGIC.to_vec();
        beacon.extend_from_slice(&49213u16.to_le_bytes());
        beacon.push(BEACON_RUNNING);
        beacon.extend_from_slice(&wire_name("Room 3"));
        beacon.push(4);
        beacon.push(6);
        beacon.extend_from_slice(&0x5EA5u64.to_le_bytes());
        seeds.push(beacon);

        for round in 0..80_000u32 {
            let mut bytes = seeds[(round as usize) % seeds.len()].clone();
            for _ in 0..(rng.next_u32() % 6) + 1 {
                if bytes.is_empty() {
                    bytes.push((rng.next_u32() % 256) as u8);
                    continue;
                }
                let at = (rng.next_u32() as usize) % bytes.len();
                match rng.next_u32() % 4 {
                    0 => bytes[at] = (rng.next_u32() % 256) as u8,
                    1 => drop(bytes.remove(at)),
                    2 => bytes.insert(at, (rng.next_u32() % 256) as u8),
                    _ => bytes.truncate(at),
                }
            }
            // The message decoder, and the version peek that answers strays.
            if let Some(msg) = NetMsg::decode(&bytes) {
                // Anything decoded must survive a round trip, or the host
                // would relay something other than what it was told.
                assert_eq!(
                    NetMsg::decode(&msg.clone().encode()),
                    Some(msg.clone()),
                    "{bytes:?}"
                );
                // And a Start that decoded must be one we can actually seat.
                if let NetMsg::Start { seats, seat, .. } = msg {
                    assert!((2..=crate::sim::MAX_PLAYERS as u8).contains(&seats));
                    assert!(seat.is_none_or(|seat| seat < seats));
                }
                // A seat that decoded is a seat something will index by.
                if let NetMsg::Abandoned { seat, .. } = msg {
                    assert!(usize::from(seat) < crate::sim::MAX_PLAYERS);
                }
            }
            let _ = NetMsg::peek_version(&bytes);
            // The beacon decoder shares the port with none of that, but
            // shares the network with all of it.
            // Both names it carries: the beach's, and the host's behind it,
            // which is the one that runs off the end of a short packet.
            let _ = beacon_name(&bytes, bytes.len(), BEACON_NAME_AT);
            let _ = beacon_name(&bytes, bytes.len(), BEACON_HOST_AT);
        }
    }
}
