//! A tick's inputs and notices as one datagram, and how a big one is cut.

use super::*;
use crate::sim::InputMsg;

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
pub(super) fn write_batch(batch: &Batch, out: &mut Vec<u8>) {
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
pub(super) fn read_batch(body: &[u8]) -> Option<Batch> {
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

#[cfg(test)]
mod tests {
    use super::super::tests::{longest_span, notices, tail};
    use super::*;

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
}
