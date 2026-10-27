//! What one datagram says, and the bytes it says it in.
//!
//! Byte 0 tags the message and byte 1 is the [`PROTOCOL_VERSION`], frozen
//! across versions so a build can tell "I cannot read this" from "I
//! disagree with this".

use super::*;
use crate::sim::InputMsg;

/// Host → joiner: the match begins with `seats` seats on `terms`; you are
/// `seat`, and the table is called `names` (empty entries fall back to
/// seat labels). Re-sent whenever a joiner is still saying hello.
///
/// One type for the message and for what it invites a peer to, taken up
/// by `OnlineSession::take_up` whether it arrives in the lobby, mid-round
/// or on the results card, so there is one way to begin a round. Named
/// fields rather than a tuple: `(seats, seat, ..)` is a count and an index
/// next to each other, kept apart only by their order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Start {
    pub seats: u8,
    /// The seat this peer is given, or `None` for a peer that came to
    /// watch. On the wire that `None` is [`SPECTATOR_SEAT`], a number
    /// outside the range of real seats.
    pub seat: Option<u8>,
    pub terms: MatchTerms,
    pub names: [WireName; crate::sim::MAX_PLAYERS],
    /// What holds each seat (see [`kind_byte`]): the round names its
    /// bots, so every peer draws the robot on them.
    pub kinds: [u8; crate::sim::MAX_PLAYERS],
    /// Where the series stands as this round begins, or `None` for a
    /// single round. On the wire the absence is a round number of
    /// zero, which no series ever reaches.
    ///
    /// The host says, because seats move: a peer that leaves between
    /// rounds frees its chair and everyone behind it moves up one, so
    /// a tally kept by seat number credited the departed player's
    /// rounds to whoever moved in. The host re-deals the tally with
    /// the chairs, and a peer admitted from the queue mid-series
    /// learns the standings the same way.
    pub standing: Option<SeriesStanding>,
    /// The beach itself, when the host picked one it built rather than
    /// one both peers already have. A generated arena travels as a
    /// seed; a handmade one has to travel as itself, because the
    /// joiner has never seen the file. Empty for the built-in maps.
    ///
    /// Compressed with the same coder the share codes use, and it has
    /// to be: a 20x13 beach is fifteen hundred characters of text and
    /// the datagram is a kilobyte.
    pub beach: Vec<u8>,
    /// How many rounds the host has dealt this match, this one included:
    /// a call for the next round is numbered past every invitation before
    /// it, so one still on the wire from the last round is never taken
    /// for the next (`OnlineSession::is_next_round`).
    pub deal: u32,
    /// The seats the host's results card crowned for the round before
    /// this one, a bit each by that round's chairs, or none when no round
    /// came before. A joiner a frame or two short of that round's end
    /// when this call lands is walked on to it without ever seeing the
    /// tide come in (`follow_the_called_round`), and counts the round it
    /// played, won or lost, on the host's word. Packed by
    /// [`crowned_bits`], read by [`is_crowned`].
    pub crowned: u8,
    /// The input delay the round is played with, in frames: the host's
    /// pick from how quickly its players answer it
    /// (`crate::sim::delay_for_round_trip`). Every peer has to play with
    /// the same one, since the frames nobody has committed for yet are
    /// the first `delay` of them.
    pub delay: u8,
}

#[cfg(test)]
impl Start {
    /// A two-seat call to a watcher, on the default terms, with nobody
    /// named, no series, no beach of the host's own, and nothing dealt or
    /// crowned before it: what a test changes the rest of.
    pub fn blank() -> Start {
        Start {
            seats: 2,
            seat: None,
            terms: MatchTerms::default(),
            names: [[0u8; WIRE_NAME]; crate::sim::MAX_PLAYERS],
            kinds: [0; crate::sim::MAX_PLAYERS],
            standing: None,
            beach: Vec::new(),
            deal: 0,
            crowned: 0,
            delay: crate::sim::DEFAULT_DELAY as u8,
        }
    }
}

/// The seats a results card crowned, packed for [`Start::crowned`]: a bit
/// per seat, seat 0 the lowest.
pub fn crowned_bits(crowned: &[bool; crate::sim::MAX_PLAYERS]) -> u8 {
    crowned
        .iter()
        .enumerate()
        .filter(|(_, won)| **won)
        .fold(0, |bits, (seat, _)| bits | 1 << seat)
}

/// Whether `seat` is one of the seats [`crowned_bits`] packed into `bits`.
/// A seat past the byte's eight was never crowned.
pub fn is_crowned(bits: u8, seat: u8) -> bool {
    bits & 1u8.checked_shl(u32::from(seat)).unwrap_or(0) != 0
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum NetMsg {
    /// Handshake ping; the host learns the peer's address (and what to
    /// call them) from it, and whether a bot is driving its seat: the
    /// game sets that flag, never the name, so the table always knows.
    Hello { name: WireName, bot: bool },
    /// Handshake ping from a peer that wants to watch, not play. Repeated
    /// like `Hello` until a `Start` lands.
    ///
    /// It carries a name for the same reason `Hello` does, and the reason
    /// is sharper here: the host vets a chat line against the name it
    /// knows the sender by (see `OnlineSession::take_chat`), and a
    /// spectator it has no name for is a peer whose claim about who is
    /// talking nobody can check.
    Watch { name: WireName },
    /// A peer's word about the round, every tick: the inputs its receiver
    /// may still be missing, what the sender itself is missing, and the
    /// round's standing notices. See [`Batch`].
    Inputs(Batch),
    /// State fingerprint after `frame`, for loud desync detection, of the
    /// round tagged `round` ([`Batch::round`]): every round's frames count
    /// from zero, so a hash from the last round still on the wire names a
    /// frame this one also has, of a different board.
    Hash { round: u8, frame: u32, hash: u64 },
    /// Host → joiner: the round begins (see [`Start`]).
    Start(Start),
    /// Someone hit pause: everybody stops committing at `frame`. Re-sent
    /// every tick while paused, so a dropped datagram costs a moment of
    /// confusion rather than a stuck match. Of the round tagged `round`,
    /// as every lockstep message is ([`Batch::round`]).
    Pause { round: u8, frame: u32 },
    /// Play on: the pause that was to freeze on `frame` is lifted. Also
    /// re-sent until the sim visibly moves again. The frame is what lets a
    /// peer tell the `Pause` echoes still in flight from before the resume
    /// (see `Lockstep::receive_pause`) from a fresh pause; the round tag
    /// ([`Batch::round`]) tells the last round's from this one's.
    Resume { round: u8, frame: u32 },
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
    /// the seat from there: peers do not all hold the same inputs from a
    /// player who has gone quiet, so "from the frame you are stuck on" is
    /// not the same frame everywhere. Repeated for the rest of the round,
    /// since a lost one leaves that peer frozen while the others play on.
    /// Of the round tagged `round` ([`Batch::round`]).
    Abandoned { round: u8, seat: u8, frame: u32 },
    /// Host → the lobby: who is at the table right now, in seat order.
    ///
    /// A joiner has only ever spoken to the host, so without this it knows
    /// nobody else is even there until the match starts. Re-sent whenever
    /// the table changes and on a timer besides, since a roster that went
    /// missing would leave a screen wrong for good rather than briefly.
    Roster {
        /// How many places the table has, people and bots, in seat order:
        /// the leading `seats` of `names` and `kinds`. The AI is counted
        /// in the terms, and a place may well have no name.
        seats: u8,
        names: [WireName; crate::sim::MAX_PLAYERS],
        /// The host's dials as they stand, so a joiner's card shows the
        /// match it is joining rather than its own setup screen's idea of
        /// one. Everything but the seed is meaningful before the launch.
        terms: MatchTerms,
        /// What holds each place at the table (see [`kind_byte`]), so a
        /// bot wears the robot on every screen in the room.
        kinds: [u8; crate::sim::MAX_PLAYERS],
        /// The name of the host's handmade beach when the terms say
        /// `Custom`, empty otherwise. The terms carry only "a handmade
        /// beach", and a joiner reading that against its own shelf named
        /// whatever beach *it* had first, or the wrong one entirely.
        beach: WireName,
        /// The recipient's chair, in the lobby's roster, which is written
        /// for each peer: `Some(false)` for a peer that came to play and is
        /// past the table's last chair, which the launch seats watching
        /// until one frees, `Some(true)` for everyone else. `None` in a
        /// round's roster, sent to a peer in line with the `Queued` that
        /// says where it stands.
        ///
        /// In the roster rather than a datagram of its own ahead of it: a
        /// lost word read as a chair come free, and the line flickered.
        /// And a lobby roster is itself the word that a round is over: a
        /// peer in line for it is at the table again.
        chair: Option<bool>,
        /// The host has started the match and is picking the map for the
        /// table as it stands, which every joiner's status line says, so
        /// a wait at the lobby card is not read as the host gone quiet. Sent
        /// with a round's roster too, between the rounds of a series, while
        /// the host picks the next round's map.
        picking: bool,
    },
    /// Host → one peer: the host asked you to leave. The peer goes back to
    /// choosing a beach, and the host forgets it.
    Kicked,
    /// Host → one peer that greeted as a bot: this beach is not taking
    /// bots. Asked to go like [`NetMsg::Kicked`], with the reason, which
    /// the peer could not tell from a host simply asking it to leave.
    NoBots,
    /// Host → a greeting it has no room for: its socket holds every peer it
    /// takes ([`MAX_PEERS`]), watchers counted, so there is no place
    /// at this beach to play or to watch. Said rather than left unanswered:
    /// a joiner that hears nothing waits out its patience and is told to
    /// check an address that was right all along, and the beach goes from
    /// its list.
    Full,
    /// "I am going": said by a socket as it closes, to every peer it has
    /// (see `UdpTransport`'s `Drop`). From a joiner, the host forgets it at
    /// once rather than waiting out its silence, which left a launch or the
    /// next round inside that window seating a ghost and freezing the
    /// table on it until its silence ran out. From the host, the joiners know the beach
    /// is gone rather than waiting twenty seconds to decide so.
    Leaving,
    /// A line said in the lobby, and who said it. The sender names itself
    /// rather than the host stamping it: a joiner's greeting is the only
    /// other place the host learns a name, and a peer that never greeted
    /// would otherwise speak anonymously.
    ///
    /// Relayed by the host to the rest of the table, like an input: the
    /// spokes of the star cannot hear each other.
    Chat { name: WireName, text: WireChat },
    /// Host → everyone, once a second while it changes: how many are
    /// watching, how long a vote has left to join, and how long until the
    /// next one may be called. All in whole seconds, zero for none.
    ///
    /// Only the host counts, so only the host can say. Without it a
    /// spectator presses a key into silence: it cannot tell whether a vote
    /// is open, how many of them there are to out-vote, or why nothing
    /// happened when the crowd is still waiting.
    SpectatorTally { watching: u8, open: u8, wait: u8 },
    /// A spectator's pick for the next tide event the spectators call.
    ///
    /// Only the host counts these. It is the one peer everybody waits on
    /// anyway, so it is the one that can put the result where nobody can
    /// miss it: into an action, on a frame.
    SpectatorVote { event: u8 },
    /// A spectator's call for who wins this round: a seat, picked while the
    /// host still takes picks. Only the host counts, as with the vote, and
    /// a later pick from the same spectator replaces an earlier one.
    SpectatorPick { seat: u8 },
    /// Host → everyone, once a second while picks may still change: how
    /// many spectators have called each seat, and the whole seconds left to
    /// call one, zero once the calls are in. The players see it too; what
    /// the crowd thinks of them is half the point.
    CrowdPicks {
        picks: [u8; crate::sim::MAX_PLAYERS],
        open: u8,
    },
    /// Host → a peer that turned up after the launch: the round is under
    /// way and cannot take you, but you are in line for the next one, with
    /// `ahead` people in front of you.
    ///
    /// The answer to a greeting that used to be met with a spectator seat:
    /// lockstep replays from frame zero, so such a peer built a board
    /// nobody would ever send it inputs for and sat there, apparently
    /// connected, forever.
    Queued { ahead: u8 },
    /// Host → a spectator that arrived with the round already running: one
    /// part of the round as it stands at `frame`, the `Start` it would have
    /// been sent at the launch and the board, packed together and cut into
    /// `parts` pieces, of which this is number `part`.
    ///
    /// In parts because the board does not fit a datagram: a mid-round
    /// beach carries every crab and gull and post on it. Keyed by `frame`,
    /// so pieces of two different snapshots can never be put together; a
    /// piece that went astray is made good by the next greeting, which is
    /// answered with a whole fresh snapshot.
    CatchUp {
        frame: u32,
        part: u8,
        parts: u8,
        bytes: Vec<u8>,
    },
    /// "I speak protocol `version`, and what you sent me is not it." The
    /// answer to a datagram from another build, so a mismatched joiner is
    /// told why nothing is happening instead of greeting a host that ignores
    /// it forever.
    ///
    /// The one message exempt from the version gate, and the one whose layout
    /// is frozen along with the version byte: every build, past and future,
    /// can read `[TAG_INCOMPATIBLE, version]`.
    Incompatible { version: u8 },
    /// Host → a peer, about once a second while the table gathers and
    /// between rounds: answer with this `stamp`. The host's own clock, in
    /// milliseconds, which only the host reads.
    Ping { stamp: u32 },
    /// The answer to a [`NetMsg::Ping`], with its `stamp`: how the host
    /// learns each peer's round trip, which picks the next round's input
    /// delay ([`Start::delay`]).
    Pong { stamp: u32 },
    /// Host → one player, a few times a second in a round: how far the
    /// host's next commit runs past the newest commit it has heard from
    /// that player. Set against the same reading the other way round, it
    /// says how far apart the two clocks are, with the trip between them
    /// cancelling out (`OnlineSession::keep_time`).
    Clock { gap: i16 },
}

/// What one peer tells another every tick of a round, in one datagram.
///
/// One datagram a tick to each peer, not one per seat or per notice: the
/// host sends each peer one, carrying every seat's inputs that peer has
/// not said it holds and the notices beside them. Passing each player's
/// tail on as it came, with the pause, the clock and every seat given up
/// on as datagrams of their own, is 43 a tick at a table of six with
/// three watching; this is eight.
///
/// Every input not yet acknowledged goes out again every tick, so a lost
/// datagram is made good by the next one 33 ms later, and the word that
/// shrinks the tail is the receiver's own [`Batch::wants`]: a peer that
/// says nothing (or whose word is lost) is sent the whole tail.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Batch {
    /// Which round the sender is playing: the low byte of the deal that
    /// began it (`OnlineSession::rounds_dealt`). A batch of any other round
    /// is the last one's still on the wire, or the next one's reaching a
    /// peer still finishing this, and nothing in it is about the round the
    /// receiver is playing: its frames, its notices and its clock all name
    /// another lockstep. Frame numbers cannot tell them apart, since every
    /// round counts from zero.
    pub round: u8,
    /// The inputs, travelling as runs of one seat's consecutive frames.
    pub inputs: Vec<InputMsg>,
    /// For every seat, the first frame the sender does not hold
    /// (`Lockstep::wants`): its receiver resends that seat from there.
    pub wants: Option<[u32; crate::sim::MAX_PLAYERS]>,
    /// The pause the sender is holding, repeated every tick while it holds
    /// it: everybody stops committing at this frame.
    pub pause: Option<u32>,
    /// The pause the sender lifted, repeated for a while after (see
    /// [`NetMsg::Resume`]).
    pub resume: Option<u32>,
    /// Host only: every seat given up on this round, and the frame each was
    /// emptied from, repeated for the rest of it (see [`NetMsg::Abandoned`]).
    pub abandoned: Vec<(u8, u32)>,
    /// Host only, a few times a second: how its clock stands against this
    /// player's (see [`NetMsg::Clock`]).
    pub clock: Option<i16>,
}

/// The [`Batch::round`] of the round a socket dealt `rounds_dealt`-th: the
/// low byte of the count, which is all a batch has to tell the round it
/// belongs to from the one before or after it.
pub fn round_tag(rounds_dealt: u32) -> u8 {
    rounds_dealt.to_le_bytes()[0]
}

/// Where a series stands as a round begins: its 1-based number, and the
/// rounds each *seat* has won so far.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SeriesStanding {
    pub round: u8,
    pub wins: [u8; crate::sim::MAX_PLAYERS],
}

impl SeriesStanding {
    /// The standing at the top of a series: round one, nothing won.
    pub fn opening() -> SeriesStanding {
        SeriesStanding {
            round: 1,
            wins: [0; crate::sim::MAX_PLAYERS],
        }
    }
}

impl NetMsg {
    /// A greeting carrying `name` in wire form.
    pub fn hello(name: &str) -> NetMsg {
        NetMsg::Hello {
            name: wire_name(name),
            bot: false,
        }
    }

    /// The greeting of a game whose seat a bot will drive.
    pub fn hello_bot(name: &str) -> NetMsg {
        NetMsg::Hello {
            name: wire_name(name),
            bot: true,
        }
    }

    /// Whether this is how a newcomer opens: a greeting, to play or to
    /// watch. Every way onto a beach starts with one (the lobby, a bot's
    /// seat, a spectator, a player in line or coming back after being
    /// forgotten, the direct `PINCH_JOIN` pair), and it repeats until it is
    /// answered, so a host takes nobody new on anything else.
    pub fn greets(&self) -> bool {
        matches!(self, NetMsg::Hello { .. } | NetMsg::Watch { .. })
    }

    /// The same greeting from somebody who came to watch. Named, because
    /// the host has to be able to call the crowd something and to check
    /// what the crowd says it is called.
    pub fn watch(name: &str) -> NetMsg {
        NetMsg::Watch {
            name: wire_name(name),
        }
    }

    /// A host's `Ping`, stamped with its own clock now.
    pub fn ping() -> NetMsg {
        NetMsg::Ping {
            stamp: ping_stamp(),
        }
    }

    /// A line of chat as `who` said it, both halves in wire form. An empty
    /// `who` is the lobby itself speaking.
    pub fn chat(who: &str, line: &str) -> NetMsg {
        NetMsg::Chat {
            name: wire_name(who),
            text: wire_chat(line),
        }
    }
}

// Byte 0 of every datagram.
//
// A new tag is three lines, not one: the tag, `HIGHEST_TAG` below it, and
// a `PROTOCOL_VERSION` bump. Nothing stops compiling if you skip the bump,
// and then two builds claim the same version while one sends a tag the
// other reads as noise: chat, the roster and the abandonment notice all
// shipped under version 4 before anybody noticed.
const TAG_HELLO: u8 = 0;
/// Retired in version 11, when a tick's inputs became one datagram rather
/// than thirty-four. The number stays spoken for and is never handed to
/// another message: a version 10 peer still sends it, and `peek_version`
/// only answers `Incompatible` for a tag inside the range, which is what
/// tells that peer why its round never starts.
const TAG_RETIRED_INPUT: u8 = 1;
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
const TAG_INPUTS: u8 = 12;
const TAG_SPECTATOR_VOTE: u8 = 13;
const TAG_SPECTATOR_TALLY: u8 = 14;
const TAG_CATCH_UP: u8 = 15;
const TAG_SPECTATOR_PICK: u8 = 16;
const TAG_CROWD_PICKS: u8 = 17;
const TAG_KICKED: u8 = 18;
const TAG_LEAVING: u8 = 19;
const TAG_NO_BOTS: u8 = 20;
const TAG_PING: u8 = 21;
const TAG_PONG: u8 = 22;
const TAG_CLOCK: u8 = 23;
const TAG_FULL: u8 = 24;
/// Retired in version 36, when the lobby's roster began to carry each
/// peer's chair (`NetMsg::Roster::chair`). Spoken for, like
/// `TAG_RETIRED_INPUT`, and still the highest tag.
const TAG_RETIRED_NO_CHAIR: u8 = 25;
/// The last of them, which `peek_version` uses to tell one of ours from
/// stray traffic on the port.
///
/// It said `TAG_INPUTS` through two more tags, so a build of another
/// version sending the spectators' two was ignored rather than told why.
const HIGHEST_TAG: u8 = TAG_RETIRED_NO_CHAIR;

// What a `Batch` carries besides its inputs, a bit each in its flags byte.
const CARRIES_WANTS: u8 = 1;
const CARRIES_PAUSE: u8 = 2;
const CARRIES_RESUME: u8 = 4;
const CARRIES_CLOCK: u8 = 8;
const CARRIES_ABANDONED: u8 = 16;
const CARRIES_ANY: u8 =
    CARRIES_WANTS | CARRIES_PAUSE | CARRIES_RESUME | CARRIES_CLOCK | CARRIES_ABANDONED;

/// Bytes a run of a seat's inputs takes before its actions: the seat, its
/// first frame and how many frames it runs.
const RUN_HEAD: usize = 6;
/// Bytes one packed action takes.
const ACTION_BYTES: usize = 3;
/// The most frames one run carries, its count being a byte.
const RUN_MOST: usize = u8::MAX as usize;

/// The most of a snapshot one `CatchUp` carries: the datagram less its
/// header (tag, version, frame, part, parts and a length), with room to
/// spare under a 1500-byte MTU so no part is ever fragmented.
pub const CATCH_UP_CHUNK: usize = 900;

/// The most parts a snapshot may be cut into. A crowded XL beach packs to
/// a few; this is room for many times that, and a bound on what a
/// receiver will hold for a snapshot still arriving.
pub const CATCH_UP_PARTS: u8 = 32;

/// How a peer that came to watch is written down in a `Start`: outside
/// the range of real seats, so it cannot collide with one. A wire detail:
/// everything above this file says `None`.
const SPECTATOR_SEAT: u8 = u8::MAX;

/// What holds a seat, as one byte on the wire: 0 a person, 1 the game's
/// AI, 2 a bot. Anything else reads as a person, which is what a seat is
/// unless somebody says otherwise.
pub fn kind_byte(kind: crate::sim::SeatKind) -> u8 {
    match kind {
        crate::sim::SeatKind::Human => 0,
        crate::sim::SeatKind::Ai => 1,
        crate::sim::SeatKind::Bot => 2,
    }
}

/// A table's kinds in wire form, a byte a seat; places past the table
/// are people, which is what a byte of nought says.
pub fn wire_kinds(kinds: &[crate::sim::SeatKind]) -> [u8; crate::sim::MAX_PLAYERS] {
    let mut out = [0u8; crate::sim::MAX_PLAYERS];
    for (slot, kind) in out.iter_mut().zip(kinds) {
        *slot = kind_byte(*kind);
    }
    out
}

/// The kind a byte names; see [`kind_byte`].
pub fn kind_from_byte(byte: u8) -> crate::sim::SeatKind {
    match byte {
        1 => crate::sim::SeatKind::Ai,
        2 => crate::sim::SeatKind::Bot,
        _ => crate::sim::SeatKind::Human,
    }
}

/// Everything about a match that every peer has to agree on, or the boards
/// diverge (or, for `teams`, two peers score the same round differently).
///
/// Held as plain numbers rather than the app's enums: this layer is the wire
/// and knows nothing about menus. The app maps them at the edge.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatchTerms {
    /// How many seats the AI holds: the last places of the table, which
    /// [`MatchTerms::order`] puts in their seats.
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
    /// trios).
    pub teams: u8,
    /// The board's PRNG seed, so every peer builds the same beach. Also
    /// what tells a lobby that has just walked back from a round the
    /// stale `Start` of that very round (`a_fresh_invitation`).
    pub seed: u64,
    /// How long the series runs, as a `SeriesLength` index: one round,
    /// best of three, best of five. Every peer keeps its own tally, and
    /// they agree because they are counting the same deterministic boards,
    /// but only if they all know how many rounds take it.
    pub series: u8,
    /// How many arrows each player may have standing. A count, not an
    /// index, and not trusted: the app clamps it to its own dial's range.
    pub posts: u8,
    /// The seat each place at the table sits in, by place: the host first,
    /// then the players and the host's own bots in the order the host
    /// deals them, then the AI ([`MatchTerms::bots`] of them). The places
    /// in seat order unless the host picked partners, which moves people
    /// between seats so the teams (fixed by seat, for fairness) are the
    /// ones picked. Always a permutation with the host in seat 0:
    /// anything else is refused on the wire ([`MatchTerms::decode`]).
    pub order: [u8; crate::sim::MAX_PLAYERS],
}

/// Every place in its own seat: the order nobody rearranged.
pub const IN_SEAT_ORDER: [u8; crate::sim::MAX_PLAYERS] = [0, 1, 2, 3, 4, 5];

/// Written out because the arrows are not zero by default: terms nobody
/// set play the versus rule's three, as every round did before the count
/// travelled.
impl Default for MatchTerms {
    fn default() -> Self {
        MatchTerms {
            bots: 0,
            bot_level: 0,
            map: 0,
            gulls: 0,
            round: 0,
            teams: 0,
            seed: 0,
            series: 0,
            posts: crate::sim::MAX_SIGNPOSTS_PER_PLAYER as u8,
            order: IN_SEAT_ORDER,
        }
    }
}

impl MatchTerms {
    /// Whether these terms call for a series at all. Index 0 is the dial's
    /// "single round"; every other index is some best-of. The only reading
    /// of the byte the app should make: comparing against a particular
    /// index once left best-of-five launching as a single round on every
    /// joiner.
    pub fn is_series(self) -> bool {
        self.series != 0
    }

    /// How many humans (people and the host's bots) a table of `seats`
    /// has: the `seats - bots` places before the AI's, which the lockstep
    /// is built over wherever [`MatchTerms::order`] seats them
    /// ([`MatchTerms::player_seats`]). Never fewer than one, since seat
    /// zero is the host's whatever the dials say.
    pub fn humans(self, seats: u8) -> u8 {
        seats.saturating_sub(self.bots).max(1)
    }

    /// The seat place `place` sits in ([`MatchTerms::order`]). A place past
    /// the table's six is its own seat.
    pub fn seat_of_place(self, place: u8) -> u8 {
        self.order.get(usize::from(place)).copied().unwrap_or(place)
    }

    /// The seats the people and the host's own bots hold at a table of
    /// `seats`, lowest first: every seat but the AI's, which the lockstep
    /// is built over.
    pub fn player_seats(self, seats: u8) -> Vec<u8> {
        let mut out: Vec<u8> = (0..self.humans(seats))
            .map(|place| self.seat_of_place(place))
            .collect();
        out.sort_unstable();
        out
    }

    /// The seats the AI holds at a table of `seats`, in place order: the
    /// last `bots` places, where the order puts them. Every place, the
    /// host's included, when the AI is the whole table.
    pub fn ai_seats(self, seats: u8) -> Vec<u8> {
        (seats.saturating_sub(self.bots)..seats)
            .map(|place| self.seat_of_place(place))
            .collect()
    }

    /// Whether the order seats a table of `seats` within it: each of its
    /// places in one of its seats. The order is a permutation of six, so a
    /// smaller table could still send a place past its last seat.
    pub fn order_fits(self, seats: u8) -> bool {
        (0..seats).all(|place| self.seat_of_place(place) < seats)
    }
}

impl MatchTerms {
    const BYTES: usize = 22;

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
        out[15] = self.posts;
        out[16..22].copy_from_slice(&self.order);
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
            posts: *bytes.get(15)?,
            order: seat_order(bytes.get(16..22)?)?,
        })
    }
}

/// A seat order off the wire ([`MatchTerms::order`]): a permutation of the
/// six seats with the host in seat 0, or nothing. Squeezed into range
/// instead, two places would share a seat, or the host lose its own.
fn seat_order(bytes: &[u8]) -> Option<[u8; crate::sim::MAX_PLAYERS]> {
    let order: [u8; crate::sim::MAX_PLAYERS] = bytes.try_into().ok()?;
    let mut sorted = order;
    sorted.sort_unstable();
    (order[0] == 0 && sorted == IN_SEAT_ORDER).then_some(order)
}

/// The runs a list of inputs travels as: each run a stretch of
/// consecutive elements of one seat on consecutive frames, at most
/// [`RUN_MOST`] long, as `(start, len)` into the list.
fn runs(inputs: &[InputMsg]) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut at = 0;
    std::iter::from_fn(move || {
        let first = inputs.get(at)?;
        let mut len = 1;
        while len < RUN_MOST
            && inputs.get(at + len).is_some_and(|next| {
                next.player == first.player
                    && Some(next.frame) == first.frame.checked_add(len as u32)
            })
        {
            len += 1;
        }
        let run = (at, len);
        at += len;
        Some(run)
    })
}

/// A batch's bytes after the tag and version, onto `out`.
///
/// The inputs go as runs of consecutive elements ([`runs`]), so a sender
/// that wants them short hands them in seat by seat, frames in order
/// ([`write_pieces`] does). At most 255 runs: anything past that is left
/// off, which no sender does.
fn write_batch(batch: &Batch, out: &mut Vec<u8>) {
    let Batch {
        round,
        inputs,
        wants,
        pause,
        resume,
        abandoned,
        clock,
    } = batch;
    let mut flags = 0;
    for (carried, bit) in [
        (wants.is_some(), CARRIES_WANTS),
        (pause.is_some(), CARRIES_PAUSE),
        (resume.is_some(), CARRIES_RESUME),
        (clock.is_some(), CARRIES_CLOCK),
        (!abandoned.is_empty(), CARRIES_ABANDONED),
    ] {
        if carried {
            flags |= bit;
        }
    }
    out.push(*round);
    out.push(flags);
    if let Some(wants) = wants {
        // From the least of them, a byte each past it: a peer's wants all
        // sit within a horizon of its own frame. One further apart than a
        // byte reaches is written as wanting less, which costs a resend
        // and nothing else.
        let base = wants.iter().copied().min().unwrap_or(0);
        out.extend_from_slice(&base.to_le_bytes());
        out.extend(wants.iter().map(|want| (want - base).min(255) as u8));
    }
    if let Some(frame) = pause {
        out.extend_from_slice(&frame.to_le_bytes());
    }
    if let Some(frame) = resume {
        out.extend_from_slice(&frame.to_le_bytes());
    }
    if let Some(gap) = clock {
        out.extend_from_slice(&gap.to_le_bytes());
    }
    if !abandoned.is_empty() {
        let kept = &abandoned[..abandoned.len().min(crate::sim::MAX_PLAYERS)];
        out.push(kept.len() as u8);
        for (seat, frame) in kept {
            out.push(*seat);
            out.extend_from_slice(&frame.to_le_bytes());
        }
    }
    let count_at = out.len();
    out.push(0);
    let mut count = 0u8;
    for (start, len) in runs(inputs) {
        let Some(next) = count.checked_add(1) else {
            debug_assert!(false, "more than 255 runs in one batch");
            break;
        };
        count = next;
        let first = inputs[start];
        out.push(first.player);
        out.extend_from_slice(&first.frame.to_le_bytes());
        out.push(len as u8);
        for input in &inputs[start..start + len] {
            out.extend_from_slice(&crate::sim::encode_action(input.action));
        }
    }
    out[count_at] = count;
}

/// A batch read back from the bytes after the tag and version, or `None`
/// for anything [`write_batch`] would not have written.
fn read_batch(body: &[u8]) -> Option<Batch> {
    let mut at = 0;
    let mut take = |len: usize| {
        let bytes = body.get(at..at + len);
        at += len;
        bytes
    };
    let round = *take(1)?.first()?;
    let flags = *take(1)?.first()?;
    if flags & !CARRIES_ANY != 0 {
        return None;
    }
    let mut batch = Batch {
        round,
        ..Batch::default()
    };
    if flags & CARRIES_WANTS != 0 {
        let base = u32::from_le_bytes(take(4)?.try_into().ok()?);
        let mut wants = [0; crate::sim::MAX_PLAYERS];
        for (want, offset) in wants.iter_mut().zip(take(crate::sim::MAX_PLAYERS)?) {
            *want = base.checked_add(u32::from(*offset))?;
        }
        batch.wants = Some(wants);
    }
    if flags & CARRIES_PAUSE != 0 {
        batch.pause = Some(u32::from_le_bytes(take(4)?.try_into().ok()?));
    }
    if flags & CARRIES_RESUME != 0 {
        batch.resume = Some(u32::from_le_bytes(take(4)?.try_into().ok()?));
    }
    if flags & CARRIES_CLOCK != 0 {
        batch.clock = Some(i16::from_le_bytes(take(2)?.try_into().ok()?));
    }
    if flags & CARRIES_ABANDONED != 0 {
        // A seat outside the table is not one anything can be done about,
        // and acting on it would index off the end; nor is an empty list,
        // which the flag says is not one.
        let count = usize::from(*take(1)?.first()?);
        if count == 0 || count > crate::sim::MAX_PLAYERS {
            return None;
        }
        for _ in 0..count {
            let notice = take(5)?;
            let seat = notice[0];
            if usize::from(seat) >= crate::sim::MAX_PLAYERS {
                return None;
            }
            batch
                .abandoned
                .push((seat, u32::from_le_bytes(notice[1..5].try_into().ok()?)));
        }
    }
    let runs = *take(1)?.first()?;
    for _ in 0..runs {
        let head = take(RUN_HEAD)?;
        let player = head[0];
        let first = u32::from_le_bytes(head[1..5].try_into().ok()?);
        let len = usize::from(head[5]);
        // A seat past the table indexes nothing, and a run past the last
        // frame there is names frames that cannot be.
        if usize::from(player) >= crate::sim::MAX_PLAYERS {
            return None;
        }
        let actions = take(len * ACTION_BYTES)?;
        for (offset, packed) in actions.as_chunks::<ACTION_BYTES>().0.iter().enumerate() {
            batch.inputs.push(InputMsg {
                player,
                frame: first.checked_add(offset as u32)?,
                action: crate::sim::decode_action(*packed),
            });
        }
    }
    Some(batch)
}

/// The bytes of a batch, cut into datagrams of at most [`MAX_DATAGRAM`]
/// bytes each, every one handed to `emit` as it is written.
///
/// Cut in the order the inputs come in, which for a sender is frame by
/// frame (`Lockstep::held_between`): each datagram carries the frames
/// after the last one's, so losing one of several holds up only the
/// frames from its own on, and the frames the others carried before it
/// still play. Each packs its share seat by seat, as runs. The first
/// carries the notices but the clock, and the last the clock: a clock is
/// read against the newest of the sender's own inputs heard
/// (`OnlineSession::keep_time`), which the last datagram carries, so the
/// reading never lands ahead of the frames it was taken against. Every
/// one carries the round and `wants`, which is what keeps the next ones
/// short. A batch with no inputs is one datagram.
pub fn write_pieces(batch: &Batch, mut emit: impl FnMut(&[u8])) {
    let notices = Batch {
        inputs: Vec::new(),
        clock: None,
        ..batch.clone()
    };
    let wants_only = Batch {
        round: batch.round,
        wants: batch.wants,
        ..Batch::default()
    };
    let mut out = Vec::with_capacity(MAX_DATAGRAM);
    let mut piece = notices;
    let mut write = |piece: &mut Batch, out: &mut Vec<u8>| {
        // Seat by seat, each seat's frames in the order they came.
        piece.inputs.sort_by_key(|input| input.player);
        out.clear();
        out.push(TAG_INPUTS);
        out.push(PROTOCOL_VERSION);
        write_batch(piece, out);
        debug_assert!(out.len() <= MAX_DATAGRAM, "{} bytes", out.len());
        emit(out);
    };
    let head = |piece: &Batch| {
        let mut out = Vec::new();
        write_batch(piece, &mut out);
        2 + out.len()
    };
    // Room kept in every piece for the clock, which rides whichever one
    // turns out to be the last.
    let clock_room = if batch.clock.is_some() { 2 } else { 0 };
    let mut size = head(&piece) + clock_room;
    // Per seat, the last frame this datagram has for it and how long the
    // run ending there is: what the next input of the seat costs.
    let mut last: [Option<(u32, usize)>; crate::sim::MAX_PLAYERS] = Default::default();
    for &input in &batch.inputs {
        let seat = usize::from(input.player).min(crate::sim::MAX_PLAYERS - 1);
        let extends = last[seat].is_some_and(|(frame, len)| {
            len < RUN_MOST && Some(input.frame) == frame.checked_add(1)
        });
        let cost = ACTION_BYTES + if extends { 0 } else { RUN_HEAD };
        if size + cost > MAX_DATAGRAM && !piece.inputs.is_empty() {
            write(&mut piece, &mut out);
            piece = wants_only.clone();
            size = head(&piece) + clock_room + ACTION_BYTES + RUN_HEAD;
            last = Default::default();
            last[seat] = Some((input.frame, 1));
        } else {
            size += cost;
            last[seat] = Some(match (extends, last[seat]) {
                (true, Some((_, len))) => (input.frame, len + 1),
                _ => (input.frame, 1),
            });
        }
        piece.inputs.push(input);
    }
    piece.clock = batch.clock;
    write(&mut piece, &mut out);
}

impl NetMsg {
    pub fn encode(self) -> Vec<u8> {
        // Byte 0 tags the message, byte 1 says who wrote it; the payload
        // starts at byte 2.
        let mut bytes = match self {
            NetMsg::Hello { .. } => vec![TAG_HELLO],
            NetMsg::Watch { .. } => vec![TAG_WATCH],
            NetMsg::Queued { .. } => vec![TAG_QUEUED],
            NetMsg::Chat { .. } => vec![TAG_CHAT],
            NetMsg::Roster { .. } => vec![TAG_ROSTER],
            NetMsg::Abandoned { .. } => vec![TAG_ABANDONED],
            NetMsg::Inputs(_) => vec![TAG_INPUTS],
            NetMsg::Hash { .. } => vec![TAG_HASH],
            NetMsg::Start(_) => vec![TAG_START],
            NetMsg::Pause { .. } => vec![TAG_PAUSE],
            NetMsg::SpectatorVote { .. } => vec![TAG_SPECTATOR_VOTE],
            NetMsg::SpectatorTally { .. } => vec![TAG_SPECTATOR_TALLY],
            NetMsg::Resume { .. } => vec![TAG_RESUME],
            NetMsg::CatchUp { .. } => vec![TAG_CATCH_UP],
            NetMsg::SpectatorPick { .. } => vec![TAG_SPECTATOR_PICK],
            NetMsg::CrowdPicks { .. } => vec![TAG_CROWD_PICKS],
            NetMsg::Kicked => vec![TAG_KICKED],
            NetMsg::NoBots => vec![TAG_NO_BOTS],
            NetMsg::Full => vec![TAG_FULL],
            NetMsg::Leaving => vec![TAG_LEAVING],
            NetMsg::Ping { .. } => vec![TAG_PING],
            NetMsg::Pong { .. } => vec![TAG_PONG],
            NetMsg::Clock { .. } => vec![TAG_CLOCK],
            NetMsg::Incompatible { version } => return vec![TAG_INCOMPATIBLE, version],
        };
        bytes.push(PROTOCOL_VERSION);
        match self {
            NetMsg::Incompatible { .. }
            | NetMsg::Kicked
            | NetMsg::NoBots
            | NetMsg::Full
            | NetMsg::Leaving => {}
            // The round first, where every lockstep message keeps it.
            NetMsg::Resume { round, frame } | NetMsg::Pause { round, frame } => {
                bytes.push(round);
                bytes.extend_from_slice(&frame.to_le_bytes());
            }
            NetMsg::Ping { stamp } | NetMsg::Pong { stamp } => {
                bytes.extend_from_slice(&stamp.to_le_bytes());
            }
            NetMsg::Clock { gap } => bytes.extend_from_slice(&gap.to_le_bytes()),
            NetMsg::Queued { ahead } => bytes.push(ahead),
            NetMsg::Abandoned { round, seat, frame } => {
                bytes.push(round);
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
                kinds,
                beach,
                chair,
                picking,
            } => {
                bytes.push(seats);
                for name in names {
                    bytes.extend_from_slice(&name);
                }
                bytes.extend_from_slice(&terms.encode());
                bytes.extend_from_slice(&kinds);
                bytes.extend_from_slice(&beach);
                bytes.push(match chair {
                    None => 0,
                    Some(true) => 1,
                    Some(false) => 2,
                });
                bytes.push(u8::from(picking));
            }
            NetMsg::Hello { name, bot } => {
                bytes.extend_from_slice(&name);
                bytes.push(u8::from(bot));
            }
            NetMsg::Watch { name } => bytes.extend_from_slice(&name),
            // Whole, however long: a sender cuts a batch into datagrams
            // with `write_pieces`, which each come back as one of these.
            NetMsg::Inputs(ref batch) => write_batch(batch, &mut bytes),
            NetMsg::Hash { round, frame, hash } => {
                bytes.push(round);
                bytes.extend_from_slice(&frame.to_le_bytes());
                bytes.extend_from_slice(&hash.to_le_bytes());
            }
            NetMsg::Start(Start {
                seats,
                seat,
                terms,
                names,
                kinds,
                standing,
                beach,
                deal,
                crowned,
                delay,
            }) => {
                bytes.push(seats);
                bytes.push(seat.unwrap_or(SPECTATOR_SEAT));
                bytes.extend_from_slice(&terms.encode());
                for name in names {
                    bytes.extend_from_slice(&name);
                }
                bytes.extend_from_slice(&kinds);
                // Round zero is "no series", as `SPECTATOR_SEAT` is "no
                // seat": both are numbers the real thing never takes.
                let SeriesStanding { round, wins } = standing.unwrap_or(SeriesStanding {
                    round: 0,
                    wins: [0; crate::sim::MAX_PLAYERS],
                });
                bytes.push(round);
                bytes.extend_from_slice(&wins);
                bytes.extend_from_slice(&deal.to_le_bytes());
                bytes.push(crowned);
                bytes.push(delay);
                // Length-prefixed and last, so the fixed part above stays
                // where it was and a beach of any size is one read.
                let len = u16::try_from(beach.len()).unwrap_or(0);
                bytes.extend_from_slice(&len.to_le_bytes());
                bytes.extend_from_slice(&beach[..usize::from(len)]);
            }
            NetMsg::CatchUp {
                frame,
                part,
                parts,
                bytes: ref piece,
            } => {
                debug_assert!(piece.len() <= CATCH_UP_CHUNK, "{} bytes", piece.len());
                let len = piece.len().min(CATCH_UP_CHUNK);
                bytes.extend_from_slice(&frame.to_le_bytes());
                bytes.push(part);
                bytes.push(parts);
                bytes.extend_from_slice(&(len as u16).to_le_bytes());
                bytes.extend_from_slice(&piece[..len]);
            }
            NetMsg::SpectatorVote { event } => bytes.push(event),
            NetMsg::SpectatorPick { seat } => bytes.push(seat),
            NetMsg::CrowdPicks { picks, open } => {
                bytes.extend_from_slice(&picks);
                bytes.push(open);
            }
            NetMsg::SpectatorTally {
                watching,
                open,
                wait,
            } => bytes.extend_from_slice(&[watching, open, wait]),
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
                bot: *body.get(WIRE_NAME)? != 0,
            }),
            TAG_KICKED => Some(NetMsg::Kicked),
            TAG_NO_BOTS => Some(NetMsg::NoBots),
            TAG_FULL => Some(NetMsg::Full),
            TAG_RETIRED_NO_CHAIR => None,
            TAG_LEAVING => Some(NetMsg::Leaving),
            TAG_WATCH => Some(NetMsg::Watch {
                name: body.get(..WIRE_NAME)?.try_into().ok()?,
            }),
            TAG_INPUTS => read_batch(body).map(NetMsg::Inputs),
            // Spoken for and unread: a peer on this version never sends
            // it, and one on an older version is answered by the caller.
            TAG_RETIRED_INPUT => None,
            TAG_HASH => {
                let round = *body.first()?;
                let frame = u32::from_le_bytes(body.get(1..5)?.try_into().ok()?);
                let hash = u64::from_le_bytes(body.get(5..13)?.try_into().ok()?);
                Some(NetMsg::Hash { round, frame, hash })
            }
            TAG_START => {
                let terms = MatchTerms::decode(body.get(2..)?)?;
                let names = read_table(body.get(2 + MatchTerms::BYTES..)?)?;
                let (seats, seat) = (*body.first()?, *body.get(1)?);
                // A table this build cannot sit at is refused outright
                // rather than squeezed into range: the seat number goes on
                // to index the lockstep's own slots. Watching is the one
                // seat legitimately outside the range.
                //
                // The joiner builds its lockstep over the players' seats,
                // every seat but the AI's ([`MatchTerms::player_seats`]).
                // A `Start` seating us in an AI's chair would have it play
                // a session it is not a player of, which the lockstep
                // meets with a panic, and an order seating a place off the
                // table would index past it.
                if !(2..=crate::sim::MAX_PLAYERS as u8).contains(&seats)
                    || !terms.order_fits(seats)
                    || (seat != SPECTATOR_SEAT && !terms.player_seats(seats).contains(&seat))
                {
                    return None;
                }
                let seat = (seat != SPECTATOR_SEAT).then_some(seat);
                debug_assert!(seat.is_none_or(|seat| seat < seats));
                let kinds_at = 2 + MatchTerms::BYTES + WIRE_NAME * crate::sim::MAX_PLAYERS;
                let kinds: [u8; crate::sim::MAX_PLAYERS] = body
                    .get(kinds_at..kinds_at + crate::sim::MAX_PLAYERS)?
                    .try_into()
                    .ok()?;
                let series_at = kinds_at + crate::sim::MAX_PLAYERS;
                let round = *body.get(series_at)?;
                let wins: [u8; crate::sim::MAX_PLAYERS] = body
                    .get(series_at + 1..series_at + 1 + crate::sim::MAX_PLAYERS)?
                    .try_into()
                    .ok()?;
                let standing = (round != 0).then_some(SeriesStanding { round, wins });
                let deal_at = series_at + 1 + crate::sim::MAX_PLAYERS;
                let deal = u32::from_le_bytes(body.get(deal_at..deal_at + 4)?.try_into().ok()?);
                let crowned = *body.get(deal_at + 4)?;
                // A delay no host picks is no round to play: the lockstep
                // would wait on more uncommitted frames than any peer
                // keeps a resend tail for.
                let delay = *body.get(deal_at + 5)?;
                if u32::from(delay) > crate::sim::MAX_DELAY {
                    return None;
                }
                let after = deal_at + 6;
                // Every encoder of this version writes the length, so a
                // datagram that ends before it is not one of ours.
                let len = usize::from(u16::from_le_bytes(
                    body.get(after..after + 2)?.try_into().ok()?,
                ));
                let beach = body.get(after + 2..after + 2 + len)?.to_vec();
                Some(NetMsg::Start(Start {
                    seats,
                    seat,
                    terms,
                    names,
                    kinds,
                    standing,
                    beach,
                    deal,
                    crowned,
                    delay,
                }))
            }
            TAG_PING => Some(NetMsg::Ping {
                stamp: u32::from_le_bytes(body.get(..4)?.try_into().ok()?),
            }),
            TAG_PONG => Some(NetMsg::Pong {
                stamp: u32::from_le_bytes(body.get(..4)?.try_into().ok()?),
            }),
            TAG_CLOCK => Some(NetMsg::Clock {
                gap: i16::from_le_bytes(body.get(..2)?.try_into().ok()?),
            }),
            TAG_PAUSE => Some(NetMsg::Pause {
                round: *body.first()?,
                frame: u32::from_le_bytes(body.get(1..5)?.try_into().ok()?),
            }),
            TAG_CATCH_UP => {
                let frame = u32::from_le_bytes(body.get(..4)?.try_into().ok()?);
                let (part, parts) = (*body.get(4)?, *body.get(5)?);
                let len = usize::from(u16::from_le_bytes(body.get(6..8)?.try_into().ok()?));
                // A piece of a snapshot nobody could finish is refused
                // here, before a receiver sets aside room for it.
                if parts == 0 || parts > CATCH_UP_PARTS || part >= parts || len > CATCH_UP_CHUNK {
                    return None;
                }
                Some(NetMsg::CatchUp {
                    frame,
                    part,
                    parts,
                    bytes: body.get(8..8 + len)?.to_vec(),
                })
            }
            TAG_SPECTATOR_VOTE => Some(NetMsg::SpectatorVote {
                event: *body.first()?,
            }),
            // A seat off the table is nobody to call, and would index off
            // the end of the count.
            TAG_SPECTATOR_PICK => {
                let seat = *body.first()?;
                (seat < crate::sim::MAX_PLAYERS as u8).then_some(NetMsg::SpectatorPick { seat })
            }
            TAG_CROWD_PICKS => Some(NetMsg::CrowdPicks {
                picks: body.get(..crate::sim::MAX_PLAYERS)?.try_into().ok()?,
                open: *body.get(crate::sim::MAX_PLAYERS)?,
            }),
            TAG_SPECTATOR_TALLY => Some(NetMsg::SpectatorTally {
                watching: *body.first()?,
                open: *body.get(1)?,
                wait: *body.get(2)?,
            }),
            TAG_RESUME => Some(NetMsg::Resume {
                round: *body.first()?,
                frame: u32::from_le_bytes(body.get(1..5)?.try_into().ok()?),
            }),
            TAG_QUEUED => Some(NetMsg::Queued {
                ahead: *body.first()?,
            }),
            TAG_ABANDONED => {
                let (round, seat) = (*body.first()?, *body.get(1)?);
                let frame = u32::from_le_bytes(body.get(2..6)?.try_into().ok()?);
                // A seat outside the table is not one anything can be done
                // about, and acting on it would index off the end.
                (seat < crate::sim::MAX_PLAYERS as u8).then_some(NetMsg::Abandoned {
                    round,
                    seat,
                    frame,
                })
            }
            TAG_ROSTER => {
                let table = body.get(1..)?;
                let names = read_table(table)?;
                let terms_at = WIRE_NAME * crate::sim::MAX_PLAYERS;
                let terms = MatchTerms::decode(table.get(terms_at..)?)?;
                let kinds_at = terms_at + MatchTerms::BYTES;
                let kinds = table
                    .get(kinds_at..kinds_at + crate::sim::MAX_PLAYERS)?
                    .try_into()
                    .ok()?;
                let beach_at = kinds_at + crate::sim::MAX_PLAYERS;
                let beach = table.get(beach_at..beach_at + WIRE_NAME)?.try_into().ok()?;
                let chair = match table.get(beach_at + WIRE_NAME)? {
                    0 => None,
                    1 => Some(true),
                    2 => Some(false),
                    _ => return None,
                };
                let picking = match table.get(beach_at + WIRE_NAME + 1)? {
                    0 => false,
                    1 => true,
                    _ => return None,
                };
                Some(NetMsg::Roster {
                    seats: *body.first()?,
                    names,
                    terms,
                    kinds,
                    beach,
                    chair,
                    picking,
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

/// A full table of names off the front of `bytes`, or `None` if the
/// datagram ends before the table does. Both messages that carry one
/// (`Start`, `Roster`) read it this way.
fn read_table(bytes: &[u8]) -> Option<[WireName; crate::sim::MAX_PLAYERS]> {
    let mut names = [[0u8; WIRE_NAME]; crate::sim::MAX_PLAYERS];
    for (i, name) in names.iter_mut().enumerate() {
        *name = bytes
            .get(i * WIRE_NAME..(i + 1) * WIRE_NAME)?
            .try_into()
            .ok()?;
    }
    Some(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every message must encode within the receive buffer, or the kernel
    /// truncates it on arrival and decode drops it without a trace, which
    /// is how a 64-byte buffer once swallowed the named `Start` whole.
    #[test]
    fn every_message_fits_the_receive_buffer() {
        let widest = wire_name("WWWWWWWWWWWWWWWWWWWWWWWW");
        for msg in [
            NetMsg::Hello {
                name: widest,
                bot: true,
            },
            NetMsg::Kicked,
            NetMsg::NoBots,
            NetMsg::Full,
            NetMsg::Leaving,
            NetMsg::Chat {
                name: widest,
                text: wire_chat(&"W".repeat(CHAT_CHARS)),
            },
            NetMsg::Start(Start {
                seats: 6,
                seat: Some(5),
                names: [widest; crate::sim::MAX_PLAYERS],
                // The widest a Start gets: a full table of longest names
                // and the largest beach the sender will hand it.
                beach: vec![0xAB; MAX_BEACH_BYTES],
                ..Start::blank()
            }),
            NetMsg::Hash {
                round: u8::MAX,
                frame: u32::MAX,
                hash: u64::MAX,
            },
            NetMsg::CatchUp {
                frame: u32::MAX,
                part: CATCH_UP_PARTS - 1,
                parts: CATCH_UP_PARTS,
                bytes: vec![0xAB; CATCH_UP_CHUNK],
            },
            // A player's longest tail with every notice, which is the one
            // message whose size a caller chooses rather than the format
            // fixing it. Anything longer is cut (`write_pieces`).
            NetMsg::Inputs(Batch {
                inputs: tail(5, 2 * longest_span()),
                ..notices()
            }),
        ] {
            let len = msg.clone().encode().len();
            assert!(len <= MAX_DATAGRAM, "{len} bytes: {msg:?}");
        }
    }

    /// The longest a resend span gets: `Lockstep`'s, at the longest delay.
    fn longest_span() -> usize {
        (crate::sim::MAX_DELAY + crate::sim::RESEND_LEAD) as usize
    }

    /// `frames` consecutive inputs of `seat`, every one a placement out
    /// at the far corner of the XL beach.
    fn tail(seat: u8, frames: usize) -> Vec<InputMsg> {
        (0..frames as u32)
            .map(|frame| InputMsg {
                player: seat,
                frame: 70_000 + frame,
                action: PlayerAction::Place {
                    x: 19,
                    y: 12,
                    dir: Direction::Left,
                },
            })
            .collect()
    }

    /// A batch carrying every notice there is, and no inputs.
    fn notices() -> Batch {
        Batch {
            round: 7,
            inputs: Vec::new(),
            wants: Some([70_000, 70_002, 70_001, 70_040, 70_000, 70_003]),
            pause: Some(70_050),
            resume: Some(70_010),
            abandoned: vec![(3, 69_000), (5, 69_500)],
            clock: Some(-3),
        }
    }

    /// The datagrams [`write_pieces`] cuts `batch` into.
    fn pieces(batch: &Batch) -> Vec<Vec<u8>> {
        let mut pieces = Vec::new();
        write_pieces(batch, |bytes| pieces.push(bytes.to_vec()));
        pieces
    }

    /// A player's tail as long as the lockstep can ever hold it is one
    /// datagram, notices and all: `resend_span` keeps commits from
    /// `frame - span` while they run to `frame + span - 1`, so twice the
    /// span is the most a seat's tail can be. One seat's frames travel as
    /// one run, three bytes a frame.
    #[test]
    fn a_full_resend_tail_is_one_datagram() {
        let batch = Batch {
            inputs: tail(5, 2 * longest_span()),
            ..notices()
        };
        let pieces = pieces(&batch);
        assert_eq!(pieces.len(), 1);
        assert!(pieces[0].len() <= 300, "{} bytes", pieces[0].len());
        assert_eq!(NetMsg::decode(&pieces[0]), Some(NetMsg::Inputs(batch)));
    }

    /// The most a host ever sends one peer in a tick, every seat's longest
    /// tail at once, is cut into datagrams that each fit, and comes back
    /// whole: every input once, in datagrams that each take up the frames
    /// where the last left off, so one that is lost holds up only the
    /// frames it carried and those after. The notices ride the first, the
    /// clock the last, and the round and what the sender wants ride them
    /// all.
    #[test]
    fn a_batch_too_big_for_a_datagram_comes_back_whole_in_several() {
        let frames = 2 * longest_span();
        let mut inputs: Vec<InputMsg> = (0..crate::sim::MAX_PLAYERS as u8)
            .flat_map(|seat| tail(seat, frames))
            .collect();
        // Frame by frame, as `Lockstep::held_between` hands them over.
        inputs.sort_by_key(|input| (input.frame, input.player));
        let batch = Batch {
            inputs: inputs.clone(),
            ..notices()
        };
        let pieces = pieces(&batch);
        assert!(pieces.len() > 1, "{} datagram", pieces.len());
        let mut back = Vec::new();
        let mut reached = 0;
        for (index, bytes) in pieces.iter().enumerate() {
            assert!(bytes.len() <= MAX_DATAGRAM, "{} bytes", bytes.len());
            let Some(NetMsg::Inputs(piece)) = NetMsg::decode(bytes) else {
                panic!("datagram {index} does not decode");
            };
            assert_eq!(piece.wants, batch.wants, "datagram {index}");
            let first = index == 0;
            assert_eq!(piece.round, batch.round, "datagram {index}");
            assert_eq!(piece.pause.is_some(), first, "datagram {index}");
            assert_eq!(piece.abandoned.is_empty(), !first, "datagram {index}");
            let last = index == pieces.len() - 1;
            assert_eq!(piece.clock.is_some(), last, "datagram {index}");
            let lowest = piece.inputs.iter().map(|input| input.frame).min();
            assert!(lowest >= Some(reached), "datagram {index} goes back");
            reached = piece
                .inputs
                .iter()
                .map(|input| input.frame)
                .max()
                .unwrap_or(0);
            back.extend(piece.inputs);
        }
        back.sort_by_key(|input| (input.frame, input.player));
        assert_eq!(back, inputs);
        // And as full as the cap lets them be.
        let sent: usize = pieces.iter().map(Vec::len).sum();
        assert!(
            sent < (pieces.len() - 1) * MAX_DATAGRAM + 100 + MAX_DATAGRAM / 2,
            "{sent} bytes in {} datagrams",
            pieces.len()
        );
    }

    /// A batch with nothing to say but what its sender wants is still a
    /// datagram: it is what tells the host where to resend from.
    #[test]
    fn a_batch_with_no_inputs_is_one_small_datagram() {
        let batch = Batch {
            wants: notices().wants,
            ..Batch::default()
        };
        let pieces = pieces(&batch);
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].len(), 2 + 2 + 4 + crate::sim::MAX_PLAYERS + 1);
        assert_eq!(NetMsg::decode(&pieces[0]), Some(NetMsg::Inputs(batch)));
        // A seat further on than a byte past the least reaches is written
        // as wanting less, which costs a resend and nothing else.
        let far = NetMsg::Inputs(Batch {
            wants: Some([9, 9 + 300, 9, 9, 9, 9]),
            ..Batch::default()
        });
        let Some(NetMsg::Inputs(back)) = NetMsg::decode(&far.encode()) else {
            panic!("does not decode");
        };
        assert_eq!(back.wants, Some([9, 9 + 255, 9, 9, 9, 9]));
    }

    /// The retired tag earns its keep at exactly one moment: a version 10
    /// peer dials in, sends the inputs it has always sent, and has to be
    /// told why nothing happens. That answer only goes out for a tag
    /// inside the range, so the number stays spoken for.
    #[test]
    fn a_version_ten_input_is_refused_and_answered() {
        let mut old = vec![TAG_RETIRED_INPUT, 10];
        old.extend_from_slice(&[0u8; 8]);
        assert_eq!(NetMsg::decode(&old), None, "not read as anything");
        assert_eq!(
            NetMsg::peek_version(&old),
            Some(10),
            "and answered with why"
        );
    }

    /// A bot is said to be one in every message that names a seat: the
    /// greeting's flag, and the kinds the roster and the start carry.
    #[test]
    fn what_holds_a_seat_survives_the_wire() {
        for bot in [false, true] {
            let hello = NetMsg::Hello {
                name: wire_name("Greedy (Ana)"),
                bot,
            };
            assert_eq!(NetMsg::decode(&hello.clone().encode()), Some(hello));
        }
        let kinds = wire_kinds(&[
            crate::sim::SeatKind::Human,
            crate::sim::SeatKind::Bot,
            crate::sim::SeatKind::Ai,
        ]);
        assert_eq!(kinds, [0, 2, 1, 0, 0, 0]);
        let roster = NetMsg::Roster {
            seats: 3,
            names: [wire_name("x"); crate::sim::MAX_PLAYERS],
            terms: MatchTerms::default(),
            kinds,
            beach: wire_name("The Pier"),
            chair: Some(false),
            picking: true,
        };
        assert_eq!(NetMsg::decode(&roster.clone().encode()), Some(roster));
        let start = NetMsg::Start(Start {
            seats: 3,
            seat: Some(1),
            names: [wire_name("x"); crate::sim::MAX_PLAYERS],
            kinds,
            beach: vec![1, 2, 3],
            ..Start::blank()
        });
        assert_eq!(NetMsg::decode(&start.clone().encode()), Some(start));
        assert_eq!(
            NetMsg::decode(&NetMsg::Kicked.encode()),
            Some(NetMsg::Kicked)
        );
        assert_eq!(
            NetMsg::decode(&NetMsg::Leaving.encode()),
            Some(NetMsg::Leaving)
        );
        assert_eq!(kind_from_byte(2), crate::sim::SeatKind::Bot);
        assert_eq!(
            kind_from_byte(99),
            crate::sim::SeatKind::Human,
            "unknown reads as a person"
        );
    }

    /// [`MAX_BEACH_BYTES`] is what the sender trusts, so it has to be a
    /// number this encoder agrees with: the widest possible invitation
    /// carrying the largest allowed beach must still fit the buffer, with
    /// room to spare for a field somebody adds to `Start` later.
    #[test]
    fn a_start_carrying_the_largest_beach_still_fits() {
        let widest = wire_name("WWWWWWWWWWWWWWWWWWWWWWWW");
        let len = NetMsg::Start(Start {
            seats: 6,
            seat: Some(5),
            names: [widest; crate::sim::MAX_PLAYERS],
            beach: vec![0xAB; MAX_BEACH_BYTES],
            ..Start::blank()
        })
        .encode()
        .len();
        assert!(len <= MAX_DATAGRAM, "{len} bytes");
        let spare = MAX_DATAGRAM - len;
        assert!(spare >= 16, "only {spare} bytes of slack left");
    }

    /// The series byte is an *index*, and every index but zero names some
    /// best-of. Read any narrower and it lies: `== 1` is best-of-three and
    /// nothing else, which once launched every best-of-five as a single
    /// round on each joiner while the host went on counting a series. The
    /// whole byte, because a peer on another build picks the number.
    #[test]
    fn every_series_index_but_zero_names_a_series() {
        let dialled = |series| MatchTerms {
            series,
            ..MatchTerms::default()
        };
        assert!(!dialled(0).is_series(), "the dial's single round");
        for index in 1..=u8::MAX {
            assert!(dialled(index).is_series(), "index {index}");
        }
    }

    /// The humans at a table: how many places a joiner builds its lockstep
    /// over, and this file's own decoder refuses a seat outside of.
    ///
    /// The AI holds the last places, so the humans are what is left
    /// before them - but never none. Seat zero is the host's whatever the
    /// dials say, and a `bots` byte from a broken or hostile build must
    /// come to a floor rather than wrapping the count back up under it.
    #[test]
    fn the_humans_are_what_the_ai_leaves_and_never_none() {
        let with = |bots| MatchTerms {
            bots,
            ..MatchTerms::default()
        };
        assert_eq!(with(0).humans(6), 6, "no AI, every chair a player's");
        assert_eq!(with(4).humans(6), 2, "four bots behind two people");
        assert_eq!(with(5).humans(6), 1, "the host alone");
        assert_eq!(with(6).humans(6), 1, "and still the host alone");
        assert_eq!(with(u8::MAX).humans(6), 1, "however wild the byte");
        assert_eq!(with(0).humans(0), 1, "even a table of nobody seats one");
        // Whatever it answers is a count the per-seat arrays can hold, which
        // is what the `Start` decoder leans on when it refuses a seat.
        for seats in 0..=u8::MAX {
            for bots in [0, 1, 5, 6, 7, u8::MAX] {
                let humans = with(bots).humans(seats);
                assert!(humans >= 1, "{seats} seats, {bots} bots");
                assert!(humans <= seats.max(1), "{seats} seats, {bots} bots");
            }
        }
    }

    /// A call for a seat no table has is refused on the way in.
    #[test]
    fn a_pick_off_the_table_is_refused() {
        let mut bytes = NetMsg::SpectatorPick { seat: 1 }.encode();
        *bytes.last_mut().expect("a seat byte") = crate::sim::MAX_PLAYERS as u8;
        assert!(NetMsg::decode(&bytes).is_none());
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(NetMsg::decode(&[]).is_none());
        assert!(NetMsg::decode(&[0xFF, 1, 2, 3]).is_none());
        assert!(NetMsg::decode(b"PNCH?").is_none());
    }

    use crate::sim::{Direction, PlayerAction};

    #[test]
    fn messages_round_trip() {
        for msg in [
            NetMsg::hello("Anna"),
            NetMsg::hello("Überlang-Name-über-die-Kappe-hinaus"),
            NetMsg::watch("Dee"),
            NetMsg::Resume { round: 0, frame: 0 },
            NetMsg::Resume {
                round: 255,
                frame: 70_000,
            },
            NetMsg::Pause { round: 3, frame: 7 },
            NetMsg::SpectatorVote { event: 8 },
            NetMsg::SpectatorTally {
                watching: 3,
                open: 2,
                wait: 0,
            },
            NetMsg::SpectatorPick { seat: 5 },
            NetMsg::CrowdPicks {
                picks: [0, 3, 1, 0, 0, 2],
                open: 27,
            },
            NetMsg::CatchUp {
                frame: 901,
                part: 1,
                parts: 3,
                bytes: vec![7; 40],
            },
            NetMsg::Inputs(Batch {
                inputs: vec![InputMsg {
                    player: 1,
                    frame: 42,
                    action: PlayerAction::Place {
                        x: 3,
                        y: 8,
                        dir: Direction::Down,
                    },
                }],
                ..Batch::default()
            }),
            // An empty batch, and one with every notice and nothing else.
            NetMsg::Inputs(Batch::default()),
            NetMsg::Inputs(notices()),
            // What the sender wants, as far apart as a byte past the
            // least of them reaches.
            NetMsg::Inputs(Batch {
                wants: Some([9, 9 + 255, 9, 9, 9, 9]),
                ..Batch::default()
            }),
            // The XL beach is 20 wide: a placement out in column 18 has to
            // survive the trip, which it did not while a tile was a nibble.
            // Around it, the shape a real batch has: runs of several frames
            // of one seat, and seats taking turns.
            NetMsg::Inputs(Batch {
                inputs: vec![
                    InputMsg {
                        player: 2,
                        frame: 3999,
                        action: PlayerAction::None,
                    },
                    InputMsg {
                        player: 5,
                        frame: 4000,
                        action: PlayerAction::Place {
                            x: 18,
                            y: 11,
                            dir: Direction::Left,
                        },
                    },
                    InputMsg {
                        player: 5,
                        frame: 4001,
                        action: PlayerAction::None,
                    },
                    InputMsg {
                        player: 5,
                        frame: 4002,
                        action: PlayerAction::Place {
                            x: 0,
                            y: 0,
                            dir: Direction::Up,
                        },
                    },
                    InputMsg {
                        player: 2,
                        frame: 4000,
                        action: PlayerAction::Remove { x: 4, y: 4 },
                    },
                ],
                ..notices()
            }),
            NetMsg::Hash {
                round: 9,
                frame: 990,
                hash: 0xDEAD_BEEF_0BAD_F00D,
            },
            NetMsg::Start(Start {
                seats: 6,
                seat: Some(3),
                terms: MatchTerms {
                    bots: 2,
                    teams: 1,
                    seed: 0x1234_5678_9ABC_DEF0,
                    ..MatchTerms::default()
                },
                names: std::array::from_fn(|i| wire_name(&format!("Seat {i}"))),
                beach: b"a handmade beach".to_vec(),
                deal: 0x0102_0304,
                crowned: 0b0000_0101,
                delay: crate::sim::MAX_DELAY as u8,
                ..Start::blank()
            }),
            // Mid-series: the round and the tally come back as they went,
            // and as a presence rather than as the zero that means none.
            NetMsg::Start(Start {
                seats: 4,
                seat: Some(2),
                terms: MatchTerms {
                    series: 2,
                    ..MatchTerms::default()
                },
                standing: Some(SeriesStanding {
                    round: 3,
                    wins: [1, 0, 1, 0, 0, 0],
                }),
                ..Start::blank()
            }),
            NetMsg::Incompatible { version: 9 },
            NetMsg::Ping { stamp: 0 },
            NetMsg::Pong { stamp: 0xFEED_BEEF },
            NetMsg::Clock { gap: -3 },
            NetMsg::Clock { gap: i16::MAX },
            NetMsg::Queued { ahead: 0 },
            NetMsg::Queued { ahead: 4 },
            NetMsg::Full,
            NetMsg::Chat {
                name: wire_name("Anna"),
                text: wire_chat("wait for me!"),
            },
            NetMsg::Roster {
                seats: 4,
                names: std::array::from_fn(|i| wire_name(&format!("P{i}"))),
                kinds: [0; crate::sim::MAX_PLAYERS],
                terms: MatchTerms {
                    bots: 1,
                    map: 3,
                    ..MatchTerms::default()
                },
                beach: [0; WIRE_NAME],
                chair: None,
                picking: false,
            },
            NetMsg::Abandoned {
                round: 0,
                seat: 0,
                frame: 0,
            },
            NetMsg::Abandoned {
                round: 1,
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
        let good = NetMsg::Start(Start {
            seats: 6,
            seat: Some(5),
            ..Start::blank()
        });
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
            Some(NetMsg::Start(Start { seat: None, .. }))
        ));
        // An AI's chair is not one a joiner can be sat in either: the
        // lockstep it builds carries only the humans, and being seated
        // outside it was a panic, not a refusal.
        let with_bots = |seats, bots, seat| {
            NetMsg::decode(
                &NetMsg::Start(Start {
                    seats,
                    seat: Some(seat),
                    terms: MatchTerms {
                        bots,
                        ..MatchTerms::default()
                    },
                    ..Start::blank()
                })
                .encode(),
            )
        };
        assert!(with_bots(6, 4, 1).is_some(), "the last human seat");
        assert!(with_bots(6, 4, 2).is_none(), "the first AI seat");
        assert!(with_bots(6, 4, 3).is_none());
        assert!(with_bots(6, 6, 1).is_none(), "more bots than chairs");
        assert!(with_bots(6, 6, 0).is_some(), "seat zero always stands");
    }

    /// The seat order rides in the terms of every `Start` and roster: a
    /// joiner sat where the host's partner picks put it, the AI wherever
    /// the order says, and nobody in an AI's chair. An order that is not a
    /// permutation with the host in seat 0, or that seats a place off the
    /// table, is refused.
    #[test]
    fn the_seat_order_travels_and_a_broken_one_is_refused() {
        let order = [0, 2, 1, 3, 4, 5];
        let start = |seat, order| {
            NetMsg::Start(Start {
                seats: 4,
                seat: Some(seat),
                terms: MatchTerms {
                    bots: 2,
                    order,
                    ..MatchTerms::default()
                },
                ..Start::blank()
            })
        };
        let good = start(2, order);
        assert_eq!(NetMsg::decode(&good.clone().encode()), Some(good));
        assert!(
            NetMsg::decode(&start(1, order).encode()).is_none(),
            "seat 1 is an AI's under this order"
        );
        for broken in [[1, 0, 2, 3, 4, 5], [0, 1, 1, 3, 4, 5], [0, 1, 2, 3, 4, 9]] {
            assert!(
                NetMsg::decode(&start(2, broken).encode()).is_none(),
                "{broken:?}"
            );
        }
        // A permutation, but a place of this four-seat table in seat 5.
        assert!(NetMsg::decode(&start(2, [0, 5, 1, 3, 4, 2]).encode()).is_none());
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
            NetMsg::watch("Dee").encode(),
            NetMsg::Queued { ahead: 3 }.encode(),
            NetMsg::Chat {
                name: wire_name("Bo"),
                text: wire_chat("ready?"),
            }
            .encode(),
            NetMsg::Start(Start {
                seats: 6,
                seat: Some(2),
                ..Start::blank()
            })
            .encode(),
            NetMsg::Hash {
                round: 1,
                frame: 7,
                hash: 9,
            }
            .encode(),
            // The counts are lengths off the wire, and the fields in the
            // protocol that say how much to read.
            NetMsg::Inputs(Batch {
                inputs: (0..6)
                    .map(|frame| InputMsg {
                        player: (frame % 2) as u8,
                        frame: 9 + frame / 2,
                        action: crate::sim::PlayerAction::None,
                    })
                    .collect(),
                wants: Some([9, 12, 9, 9, 10, 9]),
                pause: Some(40),
                abandoned: vec![(1, 30)],
                clock: Some(2),
                ..Batch::default()
            })
            .encode(),
            ANNOUNCE_MAGIC.to_vec(),
            Vec::new(),
        ];
        // And a beacon, built the way the announcer builds one.
        let mut beacon = ANNOUNCE_MAGIC.to_vec();
        beacon.extend_from_slice(&49213u16.to_le_bytes());
        beacon.push(Kind::Running as u8);
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
                if let NetMsg::Start(Start { seats, seat, .. }) = msg {
                    assert!((2..=crate::sim::MAX_PLAYERS as u8).contains(&seats));
                    assert!(seat.is_none_or(|seat| seat < seats));
                }
                // A seat that decoded is a seat something will index by.
                if let NetMsg::Abandoned { seat, .. } = msg {
                    assert!(usize::from(seat) < crate::sim::MAX_PLAYERS);
                }
            }
            let _ = NetMsg::peek_version(&bytes);
            // And the beacon decoder, names and all: the host's name is
            // the field that runs off a short packet.
            let _ = decode(&bytes);
        }
    }
}
