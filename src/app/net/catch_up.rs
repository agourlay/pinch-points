//! A spectator who arrives with the round already running.
//!
//! Lockstep plays every peer from the same frame zero, so a latecomer used
//! to be put in line for the next round: a session begun at zero halfway
//! through a round is one nobody can send the inputs for, the resend tails
//! reaching only a second back. The host hands a watcher the round as it
//! stands instead, the board at some frame and the `Start` it would have
//! had at the launch, and the watcher starts its session at that frame,
//! where the tails still reach. Players still wait for the next round: a
//! chair is not something a round in progress can grow.
//!
//! The board does not fit a datagram, so the two travel packed together and
//! cut into [`NetMsg::CatchUp`] parts.

use super::OnlineSession;
use super::peers::Place;
use super::rounds::Invitation;
use crate::sim::{Board, Lockstep};
use crate::transport::{CATCH_UP_CHUNK, CATCH_UP_PARTS, NetMsg, UdpTransport};

/// A session's side of catching up: as a watcher, the board and frame it
/// was caught up at; as the host, who is owed the round as it stands.
#[derive(Default)]
pub struct CatchUp {
    /// The board this round is joined at, for a spectator that arrived
    /// with it already running: the arena starts from this rather than
    /// building frame zero from the terms. `None` for every round anyone
    /// was at from the start.
    pub board: Option<Board>,
    /// The frame a caught-up watcher's session started from, which it has
    /// to move off within a few seconds (see `catch_up_again`).
    pub(crate) from_frame: Option<u32>,
    /// Host: the late watchers owed the round as it stands, sent once the
    /// frames of this tick have been simulated and the board is the one
    /// the lockstep's frame says.
    pub(super) owed: Vec<usize>,
}

impl OnlineSession {
    /// A watcher's session for a round already running: the invitation as
    /// the launch would have given it, then a lockstep that starts at the
    /// frame the host's board was taken at, and that board for the arena
    /// to start from (see `catch_up`).
    pub fn caught_up(transport: UdpTransport, caught: CaughtUp) -> OnlineSession {
        let CaughtUp {
            invitation,
            frame,
            board,
        } = caught;
        let humans = invitation.terms.humans(invitation.seats);
        let mut session = OnlineSession::invited(transport, invitation);
        session.session =
            Lockstep::observer_from((0..humans).collect(), crate::sim::DEFAULT_DELAY, frame);
        session.stall.reset(frame);
        session.catch_up.board = Some(board);
        session.catch_up.from_frame = Some(frame);
        session
    }

    /// Host: whether `peer` came to watch after the launch, and so is one
    /// the round has to be handed to rather than one that has it already.
    ///
    /// A table formed in the lobby, however small: a host that launched
    /// against the AI with nobody else aboard keeps an empty plan, and every
    /// greeting after it is a latecomer. The direct `PINCH_HOST` pair has
    /// no lobby and no watchers to catch up.
    pub(super) fn late_watcher(&self, peer: usize) -> bool {
        self.home.from_lobby && self.queue_place(peer).is_some()
    }

    /// Host: send each late watcher owed one the round as it stands.
    ///
    /// Called after `pump`, with the board it has just simulated: that
    /// board is at the lockstep's frame, which is the frame the watcher's
    /// session will start from. Each part goes to the watcher alone.
    pub fn send_catch_ups(&mut self, board: &crate::sim::Board) {
        if self.catch_up.owed.is_empty() {
            return;
        }
        let frame = self.session.frame();
        // A round the tide has ended: the board stops ticking while the
        // lockstep still counts frames, so the two part, and there is
        // nothing left to watch anyway. The watcher keeps greeting and is
        // answered between rounds like everybody else.
        if board.round_over() || board.ticks() != u64::from(frame) {
            self.catch_up.owed.clear();
            return;
        }
        let parts = pack(&self.start_msg(None), frame, board);
        for peer in std::mem::take(&mut self.catch_up.owed) {
            if parts.is_empty() {
                // A board too big to send. No board this game builds comes
                // near, but a watcher sent nothing would greet for ever, so
                // it gets what every latecomer used to: a place in line.
                self.peers.row(peer).place = Place::Queued;
                let name = self
                    .peers
                    .get(peer)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                let answer = self.answer_greeting(peer, &name, true);
                self.transport.send_to(peer, answer);
                continue;
            }
            for part in &parts {
                self.transport.send_to(peer, part.clone());
            }
        }
    }
}

/// Cut the round as it stands into the parts that carry it: `start`, the
/// invitation a watcher gets, and `board` at `frame`.
///
/// Empty if the board will not fit in [`CATCH_UP_PARTS`], which no board
/// this game builds comes near; a watcher then simply waits for the next
/// round, as every latecomer used to.
pub fn pack(start: &NetMsg, frame: u32, board: &Board) -> Vec<NetMsg> {
    let invitation = start.clone().encode();
    let Ok(len) = u16::try_from(invitation.len()) else {
        return Vec::new();
    };
    let mut payload = len.to_le_bytes().to_vec();
    payload.extend_from_slice(&invitation);
    payload.extend_from_slice(board.to_snapshot().as_bytes());
    let packed = crate::lzw::compress(&payload, 8);
    let pieces: Vec<&[u8]> = packed.chunks(CATCH_UP_CHUNK).collect();
    let Ok(parts) = u8::try_from(pieces.len()) else {
        return Vec::new();
    };
    if parts > CATCH_UP_PARTS {
        return Vec::new();
    }
    pieces
        .into_iter()
        .enumerate()
        .map(|(part, piece)| NetMsg::CatchUp {
            frame,
            part: part as u8,
            parts,
            bytes: piece.to_vec(),
        })
        .collect()
}

/// A snapshot put back together: the invitation, and the board at `frame`.
pub struct CaughtUp {
    pub invitation: Invitation,
    pub frame: u32,
    pub board: Board,
}

/// The parts of a snapshot as they arrive.
#[derive(Default)]
pub struct Assembly {
    frame: Option<u32>,
    parts: Vec<Option<Vec<u8>>>,
}

impl Assembly {
    /// Take one part, and the whole round once the last part is in.
    ///
    /// Only the newest snapshot is gathered: a part of a later one starts
    /// over, and a part of an earlier one is a leftover of the snapshot
    /// just abandoned. A whole that will not unpack is dropped, and the
    /// next greeting brings another.
    pub fn take(&mut self, frame: u32, part: u8, parts: u8, bytes: Vec<u8>) -> Option<CaughtUp> {
        match self.frame {
            Some(current) if frame < current => return None,
            Some(current) if frame == current && self.parts.len() == usize::from(parts) => {}
            _ => {
                self.frame = Some(frame);
                self.parts = vec![None; usize::from(parts)];
            }
        }
        *self.parts.get_mut(usize::from(part))? = Some(bytes);
        if self.parts.iter().any(Option::is_none) {
            return None;
        }
        let packed: Vec<u8> = std::mem::take(&mut self.parts)
            .into_iter()
            .flatten()
            .flatten()
            .collect();
        self.frame = None;
        unpack(&packed, frame)
    }
}

/// The payload [`pack`] made, read back.
fn unpack(packed: &[u8], frame: u32) -> Option<CaughtUp> {
    let payload = crate::lzw::decompress(packed, 8)?;
    let len = usize::from(u16::from_le_bytes(payload.get(..2)?.try_into().ok()?));
    let start = NetMsg::decode(payload.get(2..2 + len)?)?;
    let board = Board::parse_snapshot(std::str::from_utf8(payload.get(2 + len..)?).ok()?).ok()?;
    // A watcher is the only thing this ever seats, and a board whose clock
    // is not the frame it came with would start the session out of step
    // with the beach it draws.
    if board.ticks() != u64::from(frame) {
        return None;
    }
    let NetMsg::Start {
        seats,
        seat: None,
        terms,
        names,
        standing,
        beach,
    } = start
    else {
        return None;
    };
    Some(CaughtUp {
        invitation: Invitation {
            seats,
            seat: None,
            terms,
            names,
            standing,
            beach,
        },
        frame,
        board,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{BotLevel, MAX_PLAYERS, PlayerAction, bot_action, generate_arena};
    use crate::transport::{MatchTerms, wire_name};

    fn start() -> NetMsg {
        NetMsg::Start {
            seats: 6,
            seat: None,
            terms: MatchTerms::default(),
            names: [wire_name("WWWWWWWWWWWWWWWWWWWWWWWW"); MAX_PLAYERS],
            standing: None,
            beach: Vec::new(),
        }
    }

    /// The busiest board there is: six bots on the XL beach, deep into a
    /// round, with crabs and gulls and posts everywhere.
    fn crowded() -> Board {
        let mut board = generate_arena(5, 6, 20, 13);
        for _ in 0..2400 {
            let actions: [PlayerAction; MAX_PLAYERS] =
                std::array::from_fn(|seat| bot_action(&board, seat as u8, BotLevel::Hard));
            board.tick(&actions);
        }
        board
    }

    fn pieces(parts: &[NetMsg]) -> Vec<(u32, u8, u8, Vec<u8>)> {
        parts
            .iter()
            .map(|msg| {
                let NetMsg::CatchUp {
                    frame,
                    part,
                    parts,
                    bytes,
                } = msg
                else {
                    panic!("not a part: {msg:?}");
                };
                (*frame, *part, *parts, bytes.clone())
            })
            .collect()
    }

    /// Packed, cut, sent through the wire format and put back together, in
    /// any order, the round is the round: the same board to the hash, and
    /// the same invitation. And the busiest board fits comfortably.
    #[test]
    fn a_crowded_round_survives_the_trip_in_parts() {
        let board = crowded();
        let frame = board.ticks() as u32;
        let parts = pack(&start(), frame, &board);
        assert!(!parts.is_empty());
        assert!(
            parts.len() <= usize::from(CATCH_UP_PARTS) / 4,
            "{} parts: well inside the bound",
            parts.len()
        );
        let wired: Vec<NetMsg> = parts
            .into_iter()
            .map(|msg| NetMsg::decode(&msg.encode()).expect("decodes"))
            .collect();
        let mut assembly = Assembly::default();
        let mut whole = None;
        for (frame, part, parts, bytes) in pieces(&wired).into_iter().rev() {
            assert!(whole.is_none(), "complete before the last part");
            whole = assembly.take(frame, part, parts, bytes);
        }
        let caught = whole.expect("put back together");
        assert_eq!(caught.frame, frame);
        assert_eq!(caught.board.state_hash(), board.state_hash());
        assert_eq!(caught.invitation.seats, 6);
        assert_eq!(caught.invitation.seat, None);
    }

    /// Parts of two snapshots never make one: a part of a newer snapshot
    /// starts over, a leftover of an older one is ignored, and the newer
    /// one completes on its own parts.
    #[test]
    fn parts_of_two_snapshots_are_never_mixed() {
        let mut board = crowded();
        let old = pieces(&pack(&start(), board.ticks() as u32, &board));
        board.tick(&[PlayerAction::None; MAX_PLAYERS]);
        let new = pieces(&pack(&start(), board.ticks() as u32, &board));
        assert!(old.len() > 1, "a snapshot in several parts");
        let mut assembly = Assembly::default();
        // Half of the old one, then all of the new one with a stray old
        // part landing in the middle of it.
        let (o_frame, o_part, o_parts, o_bytes) = old[0].clone();
        assert!(assembly.take(o_frame, o_part, o_parts, o_bytes).is_none());
        let mut whole = None;
        for (i, (frame, part, parts, bytes)) in new.iter().cloned().enumerate() {
            if i == 1 {
                let (f, p, n, b) = old[1].clone();
                assert!(assembly.take(f, p, n, b).is_none(), "a leftover is ignored");
            }
            whole = assembly.take(frame, part, parts, bytes);
        }
        let caught = whole.expect("the newer snapshot completes");
        assert_eq!(caught.board.state_hash(), board.state_hash());
    }

    /// What is not a watcher's invitation, or a board whose clock does not
    /// match the frame it came under, is refused rather than taken up.
    #[test]
    fn a_snapshot_that_does_not_add_up_is_refused() {
        let board = crowded();
        let frame = board.ticks() as u32;
        let seated = NetMsg::Start {
            seats: 6,
            seat: Some(1),
            terms: MatchTerms::default(),
            names: [wire_name(""); MAX_PLAYERS],
            standing: None,
            beach: Vec::new(),
        };
        for (msg, at) in [(seated, frame), (start(), frame + 1)] {
            let mut assembly = Assembly::default();
            let mut whole = None;
            for (_, part, parts, bytes) in pieces(&pack(&msg, frame, &board)) {
                whole = assembly.take(at, part, parts, bytes);
            }
            assert!(whole.is_none());
        }
    }
}

#[cfg(test)]
mod session_tests {
    use super::super::*;
    use super::*;
    use crate::sim::{DEFAULT_DELAY, InputMsg, generate_arena};

    /// Everything waiting on `socket`, after giving it a moment to arrive.
    fn drain(socket: &mut UdpTransport) -> Vec<NetMsg> {
        let mut heard = Vec::new();
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            heard.extend(socket.recv_all().into_iter().map(|(msg, _)| msg));
        }
        heard
    }

    /// The whole walk over real sockets: a host sixty frames into a round,
    /// a watcher greeting it, and the round arriving at the watcher in
    /// parts that put back together into the host's own board at the
    /// host's own frame. A player greeting at the same moment is still put
    /// in line: a round in progress can take an onlooker, not a chair.
    #[test]
    fn a_watcher_greeting_mid_round_is_sent_the_round_and_a_player_is_queued() {
        let mut host = OnlineSession::new(
            UdpTransport::host(0).expect("host socket"),
            Lockstep::new(0, vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        host.home.from_lobby = true;
        let port = host.transport.local_addr().expect("addr").port();
        let bo = UdpTransport::join(("127.0.0.1", port)).expect("join");
        bo.send(NetMsg::hello("Bo"));
        drain(&mut host.transport);
        host.peers.deal(&[Some(1)]);

        // Sixty frames of round, Bo's inputs arriving as they would.
        let mut board = generate_arena(3, 2, 12, 9);
        for frame in 0..60 {
            host.session.commit_local(PlayerAction::None);
            host.session.receive(InputMsg {
                player: 1,
                frame: DEFAULT_DELAY + frame,
                action: PlayerAction::None,
            });
            while let Some(actions) = host.session.advance() {
                board.tick(&actions);
            }
        }
        assert!(host.session.frame() > 50);

        let mut dee = UdpTransport::join(("127.0.0.1", port)).expect("join");
        dee.send(NetMsg::watch("Dee"));
        let mut cy = UdpTransport::join(("127.0.0.1", port)).expect("join");
        std::thread::sleep(std::time::Duration::from_millis(20));
        cy.send(NetMsg::hello("Cy"));
        std::thread::sleep(std::time::Duration::from_millis(20));
        host.pump(PlayerAction::None, |_| {});
        host.send_catch_ups(&board);

        assert_eq!(host.peers.planned(), 1, "the plan is still the launch's");
        assert_eq!(
            host.peers.get(1).map(|p| p.place),
            Some(Place::LateWatching)
        );
        assert!(
            host.peers.follows_the_round(1),
            "Dee is sent the round from now on"
        );
        assert!(host.watching_this_round(1), "and counts in the crowd");
        assert!(!host.peers.follows_the_round(2), "Cy, in line, is not");

        let mut assembly = catch_up::Assembly::default();
        let caught = drain(&mut dee)
            .into_iter()
            .find_map(|msg| {
                let NetMsg::CatchUp {
                    frame,
                    part,
                    parts,
                    bytes,
                } = msg
                else {
                    return None;
                };
                assembly.take(frame, part, parts, bytes)
            })
            .expect("the round arrived whole");
        assert_eq!(caught.frame, host.session.frame());
        assert_eq!(caught.board.state_hash(), board.state_hash());
        assert_eq!(caught.invitation.seat, None);
        let session = OnlineSession::caught_up(dee, caught);
        assert!(session.watching());
        assert_eq!(session.session.frame(), host.session.frame());
        assert!(session.catch_up.board.is_some());

        let queued = drain(&mut cy)
            .into_iter()
            .any(|msg| matches!(msg, NetMsg::Queued { .. }));
        assert!(queued, "a player arriving mid-round waits for the next one");
    }

    /// The direct `PINCH_HOST` pair has no lobby and nobody to catch up.
    /// A lobby host playing the AI alone keeps no plan either, and there
    /// everyone who greets is a latecomer.
    #[test]
    fn a_lobby_table_catches_up_whoever_came_late_however_small() {
        let mut host = OnlineSession::new(
            UdpTransport::host(0).expect("host socket"),
            Lockstep::new(0, vec![0], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        assert!(!host.late_watcher(0), "the direct pair");
        host.home.from_lobby = true;
        assert_eq!(host.peers.planned(), 0, "nobody else sat down");
        assert!(host.late_watcher(0), "the host alone with the AI");
        host.peers.deal(&[Some(1)]);
        assert!(!host.late_watcher(0), "one at the launch is not late");
        assert!(host.late_watcher(1));
    }

    /// Once the tide is in the board stops ticking and nothing is sent: the
    /// watcher is owed nothing more, and asks again between rounds.
    #[test]
    fn a_finished_round_is_not_sent_to_anybody() {
        let mut host = OnlineSession::new(
            UdpTransport::host(0).expect("host socket"),
            Lockstep::new(0, vec![0], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        host.home.from_lobby = true;
        let port = host.transport.local_addr().expect("addr").port();
        let mut dee = UdpTransport::join(("127.0.0.1", port)).expect("join");
        dee.send(NetMsg::watch("Dee"));
        std::thread::sleep(std::time::Duration::from_millis(20));
        host.pump(PlayerAction::None, |_| {});
        let mut board = generate_arena(3, 2, 12, 9);
        board.set_round_length(Some(0));
        assert!(board.round_over());
        host.send_catch_ups(&board);
        assert!(host.catch_up.owed.is_empty());
        let sent = drain(&mut dee)
            .into_iter()
            .any(|msg| matches!(msg, NetMsg::CatchUp { .. }));
        assert!(!sent, "nothing to watch, nothing sent");
    }

    /// A caught-up watcher that never moves off its first frame goes back
    /// to the lobby to be caught up again, rather than standing on a still
    /// beach until the host gives up on it. One that is moving stays.
    #[test]
    fn a_watcher_stuck_on_its_first_frame_asks_again() {
        use crate::app::Screen;
        for stuck in [true, false] {
            let mut app = App::new();
            app.add_plugins(bevy::state::app::StatesPlugin);
            app.init_state::<Screen>();
            app.init_resource::<Time>();
            app.init_resource::<crate::app::lobby::Homecoming>();
            let mut session = OnlineSession::new(
                UdpTransport::join(("127.0.0.1", 47998)).expect("join"),
                Lockstep::observer_from(vec![0, 1], DEFAULT_DELAY, 400),
                2,
                MatchTerms::default(),
            );
            session.catch_up.from_frame = Some(if stuck { 400 } else { 399 });
            app.insert_resource(Online(Some(session)));
            app.add_systems(Update, catch_up_again);
            for _ in 0..4 {
                app.world_mut()
                    .resource_mut::<Time>()
                    .advance_by(std::time::Duration::from_secs(1));
                app.update();
            }
            let gone = app.world().resource::<Online>().0.is_none();
            assert_eq!(gone, stuck);
            assert_eq!(
                app.world()
                    .resource::<crate::app::lobby::Homecoming>()
                    .0
                    .is_some(),
                stuck
            );
        }
    }
}
