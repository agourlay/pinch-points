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

use super::rounds::Invitation;
use crate::sim::Board;
use crate::transport::{CATCH_UP_CHUNK, CATCH_UP_PARTS, NetMsg};

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
