//! Online play, sim side (spec §7.6).
//!
//! `bevy_ggrs`/`bevy_matchbox` still target Bevy 0.18 (verified 2026-07;
//! the §9 risk-6 ecosystem lag is real), so this ships the spec's designated
//! fallback: **deterministic lockstep with a small input delay**. The
//! protocol lives here, transport-agnostic and engine-free, so it is
//! unit-testable and reusable; the shell owns sockets. The 2-byte packed
//! input and the `Board`'s clone + `state_hash` snapshots are exactly the
//! groundwork GGRS-style rollback needs, so upgrading later is contained.
//!
//! Wire model: every tick, each peer sends its local action scheduled
//! `delay` frames ahead. A frame simulates only when every player's action
//! for it is known. On a LAN with delay 3 (100 ms at 30 Hz) the sim never
//! stalls; on jitter it waits rather than desyncs. Peers exchange state
//! hashes every [`HASH_INTERVAL`] frames to detect desync loudly.

use crate::sim::board::{MAX_PLAYERS, PlayerAction, PlayerId, TideEvent};
use crate::sim::direction::Direction;
use std::collections::BTreeMap;

/// Input delay in frames: 3 at 30 Hz = 100 ms, the spec's 2–3 frame range.
pub const DEFAULT_DELAY: u32 = 3;
/// How far ahead of its own next commit a peer schedules a pause. Far
/// enough that the other peers, whose commit counters run a frame or two
/// apart, have almost always not passed it yet, and near enough (a third
/// of a second) that pressing Escape feels like it stopped the game.
pub const PAUSE_LEAD: u32 = 10;
/// How far local commits may run ahead of the simulated frame before the
/// session pushes back on the caller (a peer is stalled or unreachable).
pub const MAX_COMMIT_LEAD: u32 = 30;
/// How often peers exchange state hashes for desync detection.
pub const HASH_INTERVAL: u32 = 30;

// --- 3-byte packed input (spec §7.6) ---------------------------------------

/// Pack an action for the wire: byte 0 is the cursor column, byte 1 the row,
/// byte 2 is op (bits 0-1: 0 none, 1 place, 2 remove) and direction
/// (bits 2-3, as `Direction::id`).
///
/// A byte per axis rather than the spec's nibble, which capped online boards
/// at 16 wide. The XL beach is 20.
pub fn encode_action(action: PlayerAction) -> [u8; 3] {
    match action {
        PlayerAction::None => [0, 0, 0],
        PlayerAction::Place { x, y, dir } => [x, y, 1 | (dir.id() << 2)],
        PlayerAction::Remove { x, y } => [x, y, 2],
        // Tag 3 was free and the other two bytes are unused here, so the
        // spectators' call costs the wire nothing. An older build decodes tag 3
        // as `None`, which is why the replay header moved to v2.
        PlayerAction::CallEvent(event) => [event.index() as u8, 0, 3],
    }
}

pub fn decode_action(bytes: [u8; 3]) -> PlayerAction {
    let (x, y) = (bytes[0], bytes[1]);
    match bytes[2] & 0b11 {
        1 => PlayerAction::Place {
            x,
            y,
            dir: Direction::from_id((bytes[2] >> 2) & 0b11),
        },
        2 => PlayerAction::Remove { x, y },
        3 => TideEvent::ALL
            .get(usize::from(x))
            .copied()
            .map_or(PlayerAction::None, PlayerAction::CallEvent),
        _ => PlayerAction::None,
    }
}

// --- lockstep session ------------------------------------------------------

/// A message to put on the wire: this player's action for a future frame.
/// 8 bytes packed via [`InputMsg::encode`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InputMsg {
    pub player: PlayerId,
    pub frame: u32,
    pub action: PlayerAction,
}

/// Bytes an [`InputMsg`] occupies on the wire.
pub const INPUT_BYTES: usize = 8;

impl InputMsg {
    pub fn encode(self) -> [u8; INPUT_BYTES] {
        let f = self.frame.to_le_bytes();
        let a = encode_action(self.action);
        [self.player, f[0], f[1], f[2], f[3], a[0], a[1], a[2]]
    }

    pub fn decode(bytes: [u8; INPUT_BYTES]) -> InputMsg {
        InputMsg {
            player: bytes[0],
            frame: u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]),
            action: decode_action([bytes[5], bytes[6], bytes[7]]),
        }
    }
}

/// Transport-agnostic lockstep state machine for one peer.
pub struct Lockstep {
    /// The seat this peer plays, or `None` for one that only watches.
    local: Option<PlayerId>,
    /// Every player in the session, local included.
    players: Vec<PlayerId>,
    delay: u32,
    /// Next frame to simulate.
    frame: u32,
    /// Next frame the local player commits an input for. Commits are strictly
    /// consecutive: every frame gets one local input, or the session would
    /// stall forever waiting for the skipped frame.
    next_commit: u32,
    /// Known actions per future frame.
    pending: BTreeMap<u32, [Option<PlayerAction>; MAX_PLAYERS]>,
    /// Recent local commits, kept for redundant resends: UDP drops packets,
    /// and the handshake itself can eat the first few, so every outgoing
    /// batch repeats this tail. Receivers ignore duplicates.
    history: Vec<InputMsg>,
    /// The agreed frame the session freezes on, if a peer has called a
    /// pause. See [`Lockstep::pause_at`].
    pause_at: Option<u32>,
    /// The highest pause frame ever lifted here, by our own resume or a
    /// peer's. A `Pause` naming that frame or an earlier one is an echo of
    /// a pause that is over and is ignored. Without this the pause flapped:
    /// peers repeat `Pause` every tick, so the last echoes cross the
    /// `Resume` and re-pause whoever had just resumed.
    lifted: Option<u32>,
}

/// How far behind the local simulated frame a peer that is still talking
/// to us can be, in frames: `delay + MAX_COMMIT_LEAD`.
///
/// We only advance a frame once that peer's input for it is in, and it
/// only commits that far past its own stalled frame; so our frame is at
/// most this far past theirs, and everything we committed from that far
/// back is something they may still be missing. [`Lockstep::recent_commits`]
/// therefore keeps every commit from `frame - span` on, which with commits
/// running up to `frame + span - 1` is `2 * span` messages at most. A fixed
/// window of forty deadlocked under a one-way loss burst, with the commit
/// the stalled peer needed already scrolled out.
fn resend_span(delay: u32) -> u32 {
    delay + MAX_COMMIT_LEAD
}

impl Lockstep {
    pub fn new(local: PlayerId, players: Vec<PlayerId>, delay: u32) -> Lockstep {
        assert!(players.contains(&local));
        Lockstep::seated(Some(local), players, delay)
    }

    /// A session that watches: it receives every player's input and
    /// simulates the same frames, but commits nothing and is waited for by
    /// nobody. A spectator falling behind is a spectator's problem.
    pub fn observer(players: Vec<PlayerId>, delay: u32) -> Lockstep {
        Lockstep::seated(None, players, delay)
    }

    /// A session that watches from `frame` on, for a spectator who arrived
    /// with the round already running: the host sent the board as it stood
    /// at `frame`, so that is where this session starts simulating.
    ///
    /// Nothing is prefilled, unlike a session from frame zero: the frames
    /// here were committed by players long since, and arrive in the resend
    /// tails every peer keeps sending.
    pub fn observer_from(players: Vec<PlayerId>, delay: u32, frame: u32) -> Lockstep {
        let mut session = Lockstep::seated(None, players, delay);
        if frame > 0 {
            session.pending.clear();
            session.frame = frame;
            session.next_commit = frame + delay;
        }
        session
    }

    fn seated(local: Option<PlayerId>, players: Vec<PlayerId>, delay: u32) -> Lockstep {
        assert!(
            players
                .iter()
                .all(|&p| crate::sim::board::seat(p).is_some()),
            "player id out of range"
        );
        let mut session = Lockstep {
            local,
            players,
            delay,
            frame: 0,
            next_commit: delay,
            pending: BTreeMap::new(),
            history: Vec::new(),
            pause_at: None,
            lifted: None,
        };
        // The first `delay` frames have no committed inputs by construction;
        // both sides agree they are all None.
        for frame in 0..delay {
            let slot = session.slot(frame);
            for entry in slot.iter_mut() {
                *entry = Some(PlayerAction::None);
            }
        }
        session
    }

    fn slot(&mut self, frame: u32) -> &mut [Option<PlayerAction>; MAX_PLAYERS] {
        let players = &self.players;
        self.pending.entry(frame).or_insert_with(|| {
            let mut slot = [None; MAX_PLAYERS];
            for p in 0..MAX_PLAYERS as u8 {
                if !players.contains(&p) {
                    slot[p as usize] = Some(PlayerAction::None); // absent seats
                }
            }
            slot
        })
    }

    // --- pause protocol ---------------------------------------------------
    //
    // Lockstep cannot simply stop simulating on one peer: a frame runs only
    // when *every* player's input for it is known, so a peer that silently
    // held its inputs back would stall the others without explanation and
    // then flood them on resume. The pause is therefore agreed as a frame
    // number. Every peer stops committing at that frame, so no peer can
    // complete it, so the sim halts on the same frame everywhere: the
    // natural stall, used on purpose. Resuming just reopens commits.
    //
    // Nothing here can desync: a peer whose commits already ran past the
    // pause frame keeps those commits (they are already sent and recorded)
    // and simply stops making new ones. The frames still simulate in the
    // same order with the same inputs on every peer.

    /// Call a pause. Returns the frame the session will freeze on, to be
    /// broadcast to the peers; a pause already in flight wins if it lands
    /// earlier, so two players hitting Escape together agree. `None` for a
    /// spectator, who does not get to stop everyone else's match and has
    /// nothing to broadcast.
    pub fn request_pause(&mut self) -> Option<u32> {
        if self.watching() {
            return None;
        }
        // Past every pause already lifted, or the proposal would be read as
        // an echo of one of them (see `lifted`). A frame the others may
        // already have committed past is still a sound pause frame: they
        // stop committing and the sim comes to rest a beat later.
        let mut frame = self.next_commit + PAUSE_LEAD;
        if let Some(lifted) = self.lifted {
            frame = frame.max(lifted + 1);
        }
        self.receive_pause(frame);
        Some(self.pause_at.unwrap_or(frame))
    }

    /// A peer called a pause at `frame`. The earliest proposal wins, so
    /// every peer converges on one frame however the messages interleave.
    ///
    /// Two kinds of `Pause` are not proposals and are dropped: one naming
    /// a frame this peer has already simulated (every player committed
    /// past it, so nobody is stopping there), and one naming a pause that
    /// has since been lifted (see `lifted`). Both are the per-tick echoes
    /// of a pause that is over, arriving after the resume.
    pub fn receive_pause(&mut self, frame: u32) {
        if frame < self.frame
            || frame > self.horizon()
            || self.lifted.is_some_and(|lifted| frame <= lifted)
        {
            return;
        }
        self.pause_at = Some(match self.pause_at {
            Some(existing) => existing.min(frame),
            None => frame,
        });
    }

    /// Lift the pause and let commits flow again. Safe to call unpaused.
    /// Returns the frame the lifted pause was to freeze on (the highest
    /// ever lifted here, if there was no pause to lift), for the peers.
    pub fn resume(&mut self) -> u32 {
        self.receive_resume(self.pause_at.unwrap_or(0))
    }

    /// A peer lifted the pause that was to freeze on `frame`. Whatever
    /// pause is in flight here is lifted with it: the peers agree on one
    /// frame, and a peer that had not yet heard the earliest proposal is
    /// resuming from the same pause under a later number.
    pub fn receive_resume(&mut self, frame: u32) -> u32 {
        // Off the wire, so bounded like every other frame number that
        // arrives that way: `lifted` is added to when the next pause is
        // proposed, and a peer naming the last frame there is would have
        // had that addition overflow. One short of the horizon, so that
        // next pause, one past this, is still a frame this peer accepts.
        let frame = frame.min(self.horizon().saturating_sub(1));
        let lifted = [self.lifted, self.pause_at, Some(frame)]
            .into_iter()
            .flatten()
            .max()
            .unwrap_or(0);
        self.lifted = Some(lifted);
        self.pause_at = None;
        lifted
    }

    /// The frame this session is frozen on, if paused.
    pub fn pause_frame(&self) -> Option<u32> {
        self.pause_at
    }

    /// The frame of the pause most recently lifted, if any ever was: what
    /// a repeated `Resume` names.
    pub fn lifted_pause(&self) -> Option<u32> {
        self.lifted
    }

    /// Whether a pause has been called (the freeze itself lands a beat
    /// later, when the simulated frame reaches [`Lockstep::pause_frame`]).
    pub fn paused(&self) -> bool {
        self.pause_at.is_some()
    }

    /// Whether the sim has actually come to rest on the pause frame: the
    /// moment the picture on screen stops moving.
    pub fn frozen(&self) -> bool {
        self.pause_at.is_some_and(|at| self.frame >= at)
    }

    /// Commit the local action for the next input frame; returns the message
    /// to send to every peer, or `None` if the sim has fallen too far behind
    /// (a stalled peer) or the session is paused. The caller should retry
    /// the action next frame rather than let commits run unboundedly ahead.
    pub fn commit_local(&mut self, action: PlayerAction) -> Option<InputMsg> {
        if self.watching() {
            return None; // nothing to commit, and nobody waiting for it
        }
        if self.pause_at.is_some_and(|at| self.next_commit >= at) {
            return None;
        }
        if self.next_commit >= self.frame + self.delay + MAX_COMMIT_LEAD {
            return None;
        }
        let frame = self.next_commit;
        self.next_commit += 1;
        let local = self.local?;
        // Normalize through the wire encoding so the local sim executes
        // exactly what remote sims will decode. Lossless for any board whose
        // tiles fit a byte per axis, which is every board there is.
        let action = decode_action(encode_action(action));
        debug_assert!(
            usize::from(local) < MAX_PLAYERS,
            "committing for seat {local}, which is not at the table"
        );
        self.slot(frame)[local as usize] = Some(action);
        let msg = InputMsg {
            player: local,
            frame,
            action,
        };
        self.history.push(msg);
        self.trim_history();
        Some(msg)
    }

    /// Drop the commits no peer can still be missing (see [`resend_span`]).
    fn trim_history(&mut self) {
        let oldest_wanted = self.frame.saturating_sub(resend_span(self.delay));
        let keep = self
            .history
            .iter()
            .position(|msg| msg.frame >= oldest_wanted)
            .unwrap_or(self.history.len());
        if keep > 0 {
            self.history.drain(..keep);
        }
    }

    /// The recent local commits, oldest first: every one a peer still in
    /// step with us could be missing (see [`resend_span`]). Resend these
    /// every step so packet loss (or a not-yet-completed handshake) cannot
    /// stall the peer.
    pub fn recent_commits(&self) -> &[InputMsg] {
        &self.history
    }

    /// Which players the next frame is still waiting on. Empty means it is
    /// ready to simulate; anything else is who everybody is held up by.
    ///
    /// Read-only, because the HUD asks it every frame to put a name on
    /// screen. Only seated players can hold a frame up; an absent seat is
    /// filled at the moment its frame is made.
    pub fn awaiting(&self) -> Vec<PlayerId> {
        let Some(slot) = self.pending.get(&self.frame) else {
            // No frame made yet means nothing has arrived for it, so
            // everybody still seated is being waited on.
            return self.players.clone();
        };
        self.players
            .iter()
            .copied()
            .filter(|player| slot[*player as usize].is_none())
            .collect()
    }

    /// Give up on a player who has stopped sending, from `frame` on.
    ///
    /// Every frame from there fills their slot the way an absent seat is
    /// filled, including the frames already waiting on it, which unsticks
    /// the round in the same breath. What moves into the empty chair is not
    /// this layer's business: the shell puts an AI there, and every peer
    /// derives the same moves for it from the same board.
    ///
    /// The frame travels with the decision, because the peers do not all
    /// hold the same inputs from a player who has gone quiet: a peer that
    /// missed the relay of frame `n` may well hold `n + 2`, and filling
    /// only the empty slots had it play `n + 2` as sent while the host
    /// played it empty. Every slot from `frame` on is emptied instead,
    /// whatever it held. No peer can have simulated past `frame`, since
    /// none hears from a player except through the decider.
    pub fn abandon(&mut self, player: PlayerId, frame: u32) {
        debug_assert!(
            usize::from(player) < MAX_PLAYERS,
            "no such seat to abandon: {player}"
        );
        self.players.retain(|seated| *seated != player);
        for (&at, slot) in self.pending.iter_mut() {
            if at >= frame || slot[player as usize].is_none() {
                slot[player as usize] = Some(PlayerAction::None);
            }
        }
    }

    /// The furthest frame a peer may name: as far ahead of the frame being
    /// simulated as a resend span twice over. Frame numbers arrive off the
    /// wire, and one past this is either a peer that has run away from the
    /// table or one that is not playing fair; both are dropped.
    fn horizon(&self) -> u32 {
        self.frame + 2 * resend_span(self.delay)
    }

    /// Feed a peer's message (duplicates and already-simulated frames are
    /// ignored, so resends are harmless).
    ///
    /// A frame past the [`horizon`](Self::horizon) is ignored too: every
    /// frame accepted here makes a slot, and a peer naming frames up to
    /// `u32::MAX` would grow the table without bound.
    pub fn receive(&mut self, msg: InputMsg) {
        let horizon = self.horizon();
        if msg.frame < self.frame || msg.frame > horizon || !self.players.contains(&msg.player) {
            return;
        }
        // Past the guard, the sender is a seated player, so the index below
        // is in range, which is the only reason it is written as one.
        debug_assert!(usize::from(msg.player) < MAX_PLAYERS);
        let slot = self.slot(msg.frame);
        if slot[msg.player as usize].is_none() {
            slot[msg.player as usize] = Some(msg.action);
        }
    }

    /// If every player's action for the next frame is known, pop it for
    /// simulation. `None` means "stall this render frame": never guess.
    pub fn advance(&mut self) -> Option<[PlayerAction; MAX_PLAYERS]> {
        // Nothing waits for a frame already played: `receive` refuses one,
        // and each frame's slot is taken as it is played. A stale slot
        // would be a frame played twice, or a table that grows.
        debug_assert!(
            self.pending
                .keys()
                .next()
                .is_none_or(|&at| at >= self.frame),
            "inputs kept for a frame already played"
        );
        let slot = self.slot(self.frame);
        if slot.iter().any(|a| a.is_none()) {
            return None;
        }
        let actions = std::array::from_fn(|i| slot[i].unwrap_or(PlayerAction::None));
        self.pending.remove(&self.frame);
        self.frame += 1;
        Some(actions)
    }

    pub fn frame(&self) -> u32 {
        self.frame
    }

    /// Whether this peer is watching rather than playing.
    pub fn watching(&self) -> bool {
        self.local.is_none()
    }

    /// The seat this peer plays, if it plays one.
    pub fn seat(&self) -> Option<PlayerId> {
        self.local
    }

    pub fn player_count(&self) -> usize {
        self.players.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::board::Board;
    use crate::sim::{CrabKind, Handedness, Spawner, TileKind};

    /// A spectator handed the board at frame `n` and a session from `n`
    /// plays the rest of the round exactly as the table does: the snapshot
    /// carries everything the sim reads, and the inputs from `n` on are
    /// the ones the players' resend tails still hold.
    #[test]
    fn a_late_watcher_from_a_snapshot_keeps_step_with_the_table() {
        use crate::sim::{BotLevel, bot_action, generate_arena};
        let players = vec![0u8, 1, 2];
        let mut table = generate_arena(21, 4, 20, 13);
        let mut host = Lockstep::new(0, players.clone(), DEFAULT_DELAY);
        // Every input on the wire, as a stand-in for the resend tails.
        let mut sent: Vec<InputMsg> = Vec::new();
        let mut late: Option<(Lockstep, Board)> = None;
        for step in 0..900u32 {
            // Each seat's input for the frame it commits, as the bots would
            // play it; seat 3 is an AI the lockstep never carries.
            for &seat in &players {
                let msg = InputMsg {
                    player: seat,
                    frame: DEFAULT_DELAY + step,
                    action: bot_action(&table, seat, BotLevel::Hard),
                };
                if seat == 0 {
                    host.commit_local(msg.action);
                } else {
                    host.receive(msg);
                }
                sent.push(msg);
                if let Some((watcher, _)) = &mut late {
                    watcher.receive(msg);
                }
            }
            while let Some(mut actions) = host.advance() {
                actions[3] = bot_action(&table, 3, BotLevel::Normal);
                table.tick(&actions);
            }
            if step == 300 {
                // The board through its text, as the wire carries it.
                let board = Board::parse_snapshot(&table.to_snapshot()).expect("parses");
                assert_eq!(board.state_hash(), table.state_hash());
                let mut watcher =
                    Lockstep::observer_from(players.clone(), DEFAULT_DELAY, host.frame());
                assert_eq!(watcher.frame(), host.frame());
                // The frames already committed, which the tails repeat.
                for &msg in &sent {
                    watcher.receive(msg);
                }
                late = Some((watcher, board));
            }
            if let Some((watcher, board)) = &mut late {
                while let Some(mut actions) = watcher.advance() {
                    actions[3] = bot_action(board, 3, BotLevel::Normal);
                    board.tick(&actions);
                }
            }
        }
        let (mut watcher, board) = late.expect("the watcher arrived");
        assert!(host.frame() > 600, "the round ran: {}", host.frame());
        assert_eq!(watcher.frame(), host.frame(), "caught up to the table");
        assert_eq!(
            board.state_hash(),
            table.state_hash(),
            "and on the same beach"
        );
        assert!(
            watcher.commit_local(PlayerAction::None).is_none(),
            "it only watches"
        );
    }

    /// A session from frame zero still prefills its first frames; one from
    /// later prefills nothing, those frames having been played by others.
    #[test]
    fn a_session_from_a_later_frame_waits_for_real_inputs() {
        let fresh = Lockstep::observer_from(vec![0, 1], DEFAULT_DELAY, 0);
        assert!(fresh.awaiting().is_empty(), "frame zero is agreed idle");
        let mut late = Lockstep::observer_from(vec![0, 1], DEFAULT_DELAY, 500);
        assert_eq!(late.awaiting(), vec![0, 1]);
        assert!(late.advance().is_none());
        // Inputs from before its first frame are nothing to it.
        late.receive(InputMsg {
            player: 0,
            frame: 499,
            action: PlayerAction::None,
        });
        for player in [0, 1] {
            late.receive(InputMsg {
                player,
                frame: 500,
                action: PlayerAction::None,
            });
        }
        assert!(late.advance().is_some());
        assert_eq!(late.frame(), 501);
    }

    /// A stalled peer pushes back on local commits at the lead cap, and the
    /// queue reopens as soon as frames advance.
    #[test]
    fn commit_lead_is_bounded_and_recovers() {
        let mut session = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        let mut accepted = 0;
        for _ in 0..100 {
            if session.commit_local(PlayerAction::None).is_some() {
                accepted += 1;
            }
        }
        assert_eq!(
            accepted, MAX_COMMIT_LEAD as usize,
            "commits stop at the lead cap while the sim is stalled"
        );
        // The peer's inputs arrive; frames advance; commits reopen.
        for frame in DEFAULT_DELAY..DEFAULT_DELAY + 5 {
            session.receive(InputMsg {
                player: 1,
                frame,
                action: PlayerAction::None,
            });
        }
        let mut advanced = 0;
        while session.advance().is_some() {
            advanced += 1;
        }
        assert_eq!(advanced, DEFAULT_DELAY as usize + 5);
        assert!(session.commit_local(PlayerAction::None).is_some());
    }

    /// The pause is an agreement about a frame, not a local freeze: both
    /// peers stop committing at it, so neither can complete it, so the sim
    /// halts on the same frame on both, and resuming picks straight back up
    /// with no gap and no repeated frame.
    #[test]
    fn a_pause_halts_both_peers_on_one_frame() {
        let players = vec![0u8, 1];
        let mut a = Lockstep::new(0, players.clone(), DEFAULT_DELAY);
        let mut b = Lockstep::new(1, players, DEFAULT_DELAY);
        let mut board_a = test_board(3);
        let mut board_b = test_board(3);
        let mut pause_at = None;

        let run = |a: &mut Lockstep, b: &mut Lockstep, ba: &mut Board, bb: &mut Board| {
            let msgs: Vec<_> = [
                a.commit_local(PlayerAction::None),
                b.commit_local(PlayerAction::None),
            ]
            .into_iter()
            .flatten()
            .collect();
            for msg in msgs {
                if msg.player == 0 {
                    b.receive(msg);
                } else {
                    a.receive(msg);
                }
            }
            while let Some(actions) = a.advance() {
                ba.tick(&actions);
            }
            while let Some(actions) = b.advance() {
                bb.tick(&actions);
            }
        };

        for step in 0..80 {
            if step == 20 {
                // Peer A hits Escape; the pause frame rides the wire.
                let frame = a.request_pause().expect("a player may pause");
                b.receive_pause(frame);
                pause_at = Some(frame);
            }
            run(&mut a, &mut b, &mut board_a, &mut board_b);
        }
        let pause_at = pause_at.expect("a pause was called");
        assert!(a.frozen() && b.frozen(), "both peers came to rest");
        assert_eq!(a.frame(), pause_at, "stopped on the agreed frame");
        assert_eq!(b.frame(), pause_at, "and so did the peer");
        assert_eq!(board_a.state_hash(), board_b.state_hash());

        // Peer B presses Continue; both play on from where they stopped.
        b.resume();
        a.resume();
        for _ in 0..40 {
            run(&mut a, &mut b, &mut board_a, &mut board_b);
        }
        assert!(a.frame() > pause_at + 20, "the match ran on");
        assert_eq!(a.frame(), b.frame());
        assert_eq!(
            board_a.state_hash(),
            board_b.state_hash(),
            "pausing desynced the peers"
        );
    }

    /// A watcher simulates the same frames from the players' inputs, commits
    /// nothing, and (the part that matters) is never waited for: the two
    /// players advance whether or not it keeps up.
    #[test]
    fn a_watcher_follows_without_being_waited_for() {
        let players = vec![0u8, 1];
        let mut a = Lockstep::new(0, players.clone(), DEFAULT_DELAY);
        let mut b = Lockstep::new(1, players.clone(), DEFAULT_DELAY);
        let mut watcher = Lockstep::observer(players, DEFAULT_DELAY);
        assert!(watcher.watching());
        assert_eq!(watcher.seat(), None);
        assert_eq!(a.seat(), Some(0));

        // Whatever it is handed, it commits nothing and sends nothing.
        assert!(
            watcher
                .commit_local(PlayerAction::Remove { x: 1, y: 1 })
                .is_none()
        );
        assert!(watcher.recent_commits().is_empty());

        let place = PlayerAction::Place {
            x: 2,
            y: 3,
            dir: Direction::Up,
        };
        for frame in 0..20u32 {
            let from_a = a.commit_local(if frame == 4 {
                place
            } else {
                PlayerAction::None
            });
            let from_b = b.commit_local(PlayerAction::None);
            for msg in [from_a, from_b].into_iter().flatten() {
                a.receive(msg);
                b.receive(msg);
                watcher.receive(msg);
            }
            // The players never stall on the watcher.
            assert!(a.advance().is_some(), "player a stalled at {frame}");
            assert!(b.advance().is_some(), "player b stalled at {frame}");
        }
        // The watcher replays exactly what they played, in order.
        let mut seen = Vec::new();
        while let Some(actions) = watcher.advance() {
            seen.push(actions);
        }
        // The first `delay` frames are agreed-empty by construction, so a
        // fresh session can always run those before any input arrives.
        assert_eq!(
            seen.len(),
            20 + DEFAULT_DELAY as usize,
            "the watcher saw every frame"
        );
        assert!(
            seen.iter().any(|frame| frame[0] == place),
            "and the placement among them"
        );
        // It cannot stop the match, either.
        assert_eq!(watcher.request_pause(), None, "nothing to broadcast");
        assert!(watcher.pause_frame().is_none(), "a watcher cannot pause");
    }

    /// Two players hitting Escape in the same breath must not each freeze on
    /// their own frame: the earlier proposal wins on every peer.
    #[test]
    fn simultaneous_pauses_settle_on_the_earlier_frame() {
        let mut session = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        let mine = session.request_pause().expect("a player may pause");
        session.receive_pause(mine + 4);
        assert_eq!(session.pause_frame(), Some(mine), "later proposal ignored");
        session.receive_pause(mine - 4);
        assert_eq!(session.pause_frame(), Some(mine - 4), "earlier one wins");
        // Repeats of the same pause (they are resent every tick) are inert.
        session.receive_pause(mine - 4);
        assert_eq!(session.pause_frame(), Some(mine - 4));
        session.resume();
        assert!(!session.paused() && !session.frozen());
    }

    /// Three peers exchanging inputs (as the host relay would deliver them)
    /// must stay bit-identical: the protocol itself is seat-count agnostic.
    #[test]
    fn three_player_lockstep_stays_bit_identical() {
        let players = vec![0u8, 1, 2];
        let mut sessions: Vec<Lockstep> = (0..3u8)
            .map(|p| Lockstep::new(p, players.clone(), DEFAULT_DELAY))
            .collect();
        let mut boards: Vec<Board> = (0..3).map(|_| test_board(4)).collect();
        for step in 0u32..300 {
            let mut outgoing = Vec::new();
            for (i, session) in sessions.iter_mut().enumerate() {
                let action = if step % (20 + i as u32 * 7) == 3 {
                    PlayerAction::Place {
                        x: (step % 12) as u8,
                        y: (i as u8 * 2 + 1) % 9,
                        dir: Direction::Down,
                    }
                } else {
                    PlayerAction::None
                };
                outgoing.extend(session.commit_local(action));
            }
            for msg in outgoing {
                for (i, session) in sessions.iter_mut().enumerate() {
                    if i as u8 != msg.player {
                        session.receive(msg);
                    }
                }
            }
            for (session, board) in sessions.iter_mut().zip(&mut boards) {
                while let Some(actions) = session.advance() {
                    board.tick(&actions);
                }
            }
        }
        assert!(sessions[0].frame() > 250, "made progress");
        assert_eq!(boards[0].state_hash(), boards[1].state_hash());
        assert_eq!(boards[1].state_hash(), boards[2].state_hash());
    }

    fn roundtrip(action: PlayerAction) {
        let decoded = decode_action(encode_action(action));
        assert_eq!(format!("{action:?}"), format!("{decoded:?}"));
    }

    /// The spectators' call rides in a seat's input, and that is the whole
    /// reason it is an action at all: a peer cannot simulate a frame
    /// without every seat's input for it, so the call arrives with the
    /// frame it belongs to or the frame does not run. A side channel would
    /// have been echoed and hoped for, and a peer that missed every echo
    /// would have played a different round without knowing.
    #[test]
    fn a_called_event_travels_in_the_frame_it_belongs_to() {
        for event in TideEvent::ALL {
            roundtrip(PlayerAction::CallEvent(event));
        }

        // Two peers, one frame, the call in seat 0's slot: both run it.
        let mut a = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        let mut b = Lockstep::new(1, vec![0, 1], DEFAULT_DELAY);
        let called = PlayerAction::CallEvent(TideEvent::FreshSand);
        for frame in 0..=DEFAULT_DELAY {
            let from_a = a.commit_local(if frame == DEFAULT_DELAY {
                called
            } else {
                PlayerAction::None
            });
            let from_b = b.commit_local(PlayerAction::None);
            if let Some(msg) = from_a {
                b.receive(msg);
            }
            if let Some(msg) = from_b {
                a.receive(msg);
            }
        }
        let mut seen = Vec::new();
        while let (Some(one), Some(two)) = (a.advance(), b.advance()) {
            assert_eq!(
                format!("{one:?}"),
                format!("{two:?}"),
                "both peers run the same frame"
            );
            seen.push(one[0]);
        }
        assert!(
            seen.iter()
                .any(|a| format!("{a:?}") == format!("{called:?}")),
            "the call came out of the frame it went into: {seen:?}"
        );
    }

    #[test]
    fn packed_input_round_trips() {
        roundtrip(PlayerAction::None);
        for dir in [
            Direction::Up,
            Direction::Right,
            Direction::Down,
            Direction::Left,
        ] {
            roundtrip(PlayerAction::Place { x: 11, y: 8, dir });
        }
        roundtrip(PlayerAction::Remove { x: 0, y: 15 });
        // Past 16 wide, where a nibble per axis would have wrapped: the XL
        // beach is 20.
        for x in 16..20u8 {
            roundtrip(PlayerAction::Place {
                x,
                y: 11,
                dir: Direction::Right,
            });
            roundtrip(PlayerAction::Remove { x, y: 12 });
        }
        let msg = InputMsg {
            player: 3,
            frame: 123_456,
            action: PlayerAction::Place {
                x: 7,
                y: 2,
                dir: Direction::Left,
            },
        };
        assert_eq!(InputMsg::decode(msg.encode()), msg);
    }

    fn test_board(seed: u64) -> Board {
        let mut board = Board::new(12, 9, seed);
        board.set_tile(11, 4, TileKind::Castle(0));
        board.set_tile(0, 4, TileKind::Castle(1));
        board.set_tile(
            5,
            0,
            TileKind::Spawner(Spawner {
                dir: Direction::Down,
                period: 25,
            }),
        );
        board.spawn_crab(2, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        board.spawn_gull(9, 7, Direction::Left);
        board
    }

    /// Two peers, out-of-order delivery with lag spikes: both must simulate
    /// identical frames and end bit-identical.
    #[test]
    fn lockstep_peers_stay_bit_identical() {
        let players = vec![0u8, 1u8];
        let mut a = Lockstep::new(0, players.clone(), DEFAULT_DELAY);
        let mut b = Lockstep::new(1, players, DEFAULT_DELAY);
        let mut board_a = test_board(9);
        let mut board_b = test_board(9);
        let (mut queue_ab, mut queue_ba): (Vec<InputMsg>, Vec<InputMsg>) = (vec![], vec![]);

        for step in 0u32..600 {
            // Local "input sampling": scripted, different per peer.
            let act_a = if step % 50 == 7 {
                PlayerAction::Place {
                    x: (step % 12) as u8,
                    y: 3,
                    dir: Direction::Down,
                }
            } else {
                PlayerAction::None
            };
            let act_b = if step % 70 == 11 {
                PlayerAction::Place {
                    x: 4,
                    y: (step % 9) as u8,
                    dir: Direction::Left,
                }
            } else {
                PlayerAction::None
            };
            queue_ab.extend(a.commit_local(act_a));
            queue_ba.extend(b.commit_local(act_b));

            // Artificial network: hold messages back during "lag spikes",
            // deliver newest-first (reordering) otherwise.
            if !(step % 90 < 8) {
                while let Some(msg) = queue_ab.pop() {
                    b.receive(msg);
                }
                while let Some(msg) = queue_ba.pop() {
                    a.receive(msg);
                }
            }

            // Each peer simulates as many frames as it can.
            while let Some(actions) = a.advance() {
                board_a.tick(&actions);
            }
            while let Some(actions) = b.advance() {
                board_b.tick(&actions);
            }
        }
        // Flush and drain: both converge on the same final frame.
        while let Some(msg) = queue_ab.pop() {
            b.receive(msg);
        }
        while let Some(msg) = queue_ba.pop() {
            a.receive(msg);
        }
        while let Some(actions) = a.advance() {
            board_a.tick(&actions);
        }
        while let Some(actions) = b.advance() {
            board_b.tick(&actions);
        }
        let min_frame = a.frame().min(b.frame());
        assert!(min_frame > 500, "sessions made progress ({min_frame})");
        // Winding the faster board back is impossible, so instead both
        // drained all available frames; with every message delivered, the
        // frames are equal.
        assert_eq!(a.frame(), b.frame());
        assert_eq!(
            board_a.state_hash(),
            board_b.state_hash(),
            "lockstep peers diverged"
        );
    }
}

#[cfg(test)]
mod abandon_tests {
    use super::*;

    /// A player that stops sending holds up everybody, because a frame
    /// simulates only when every seat's action is known. Giving up on them
    /// has to unstick the frame already waiting, not merely the ones after
    /// it, or the round stays frozen on the frame that proved they had
    /// gone.
    #[test]
    fn abandoning_a_player_unsticks_the_frame_they_were_holding() {
        let mut session = Lockstep::new(0, vec![0, 1], 0);
        session.commit_local(PlayerAction::None);
        assert_eq!(session.awaiting(), vec![1], "held up by the one who left");
        assert!(session.advance().is_none(), "and going nowhere");

        session.abandon(1, 0);
        assert!(session.awaiting().is_empty(), "nobody left to wait for");
        assert!(session.advance().is_some(), "the round moves again");
    }

    /// And it keeps moving: the seat is filled as an absent one from then
    /// on, so the next frame does not stall all over again.
    #[test]
    fn an_abandoned_seat_stays_abandoned() {
        let mut session = Lockstep::new(0, vec![0, 1], 0);
        session.abandon(1, 0);
        for frame in 0..30 {
            session.commit_local(PlayerAction::None);
            assert!(
                session.advance().is_some(),
                "stalled again at frame {frame}"
            );
        }
    }

    /// The seat is emptied, not filled: what the sim gets for it is a plain
    /// no-op, so the shell can drop an AI in on top without the two of them
    /// fighting over the same slot.
    #[test]
    fn an_abandoned_seat_is_handed_over_empty() {
        let mut session = Lockstep::new(0, vec![0, 1], 0);
        session.abandon(1, 0);
        session.commit_local(PlayerAction::None);
        let actions = session.advance().expect("moves");
        assert_eq!(actions[1], PlayerAction::None);
    }

    /// Everyone else is untouched: giving up on one player must not drop
    /// the round for the rest.
    #[test]
    fn the_others_are_still_waited_for() {
        let mut session = Lockstep::new(0, vec![0, 1, 2], 0);
        session.commit_local(PlayerAction::None);
        session.abandon(1, 0);
        assert_eq!(session.awaiting(), vec![2], "still owed seat two");
        assert!(session.advance().is_none());
        session.receive(InputMsg {
            player: 2,
            frame: 0,
            action: PlayerAction::None,
        });
        assert!(session.advance().is_some());
    }
}

#[cfg(test)]
mod loss_tests {
    use super::*;

    /// Two peers over a lossy link, driven by a delivery schedule: each
    /// step both commit, then whatever `deliver` allows crosses, then both
    /// simulate what they can. Returns the frames they reached.
    fn run(steps: usize, mut deliver: impl FnMut(usize, PlayerId) -> bool) -> (Lockstep, Lockstep) {
        let players = vec![0u8, 1];
        let mut a = Lockstep::new(0, players.clone(), DEFAULT_DELAY);
        let mut b = Lockstep::new(1, players, DEFAULT_DELAY);
        for step in 0..steps {
            a.commit_local(PlayerAction::None);
            b.commit_local(PlayerAction::None);
            // Every tick resends the whole tail, as the shell does.
            let from_a: Vec<InputMsg> = a.recent_commits().to_vec();
            let from_b: Vec<InputMsg> = b.recent_commits().to_vec();
            if deliver(step, 0) {
                for msg in from_a {
                    b.receive(msg);
                }
            }
            if deliver(step, 1) {
                for msg in from_b {
                    a.receive(msg);
                }
            }
            while a.advance().is_some() {}
            while b.advance().is_some() {}
        }
        (a, b)
    }

    /// One direction of the link goes dark for longer than a lead, while
    /// the other keeps flowing. The commit the quiet peer is stuck on is
    /// then two leads old on the loud one, which a fixed forty-deep tail
    /// had already let go, freezing the session for good with both peers
    /// "still talking".
    #[test]
    fn a_one_way_loss_burst_does_not_deadlock_the_session() {
        let blackout = 20..90;
        let (a, b) = run(300, |step, from| from != 0 || !blackout.contains(&step));
        assert_eq!(a.frame(), b.frame(), "back in step");
        assert!(a.frame() > 250, "and the round ran on ({})", a.frame());
    }

    /// The tail is not unbounded either: it holds what a peer could be
    /// missing, and no more.
    #[test]
    fn the_resend_tail_stays_within_two_leads() {
        let (a, _) = run(200, |_, _| true);
        let span = resend_span(DEFAULT_DELAY) as usize;
        assert!(
            a.recent_commits().len() <= 2 * span,
            "{}",
            a.recent_commits().len()
        );
    }
}

#[cfg(test)]
mod pause_echo_tests {
    use super::*;

    /// A peer that hears the resume, then a `Pause` echo still in flight
    /// from before it, must not pause again: peers repeat their pause frame
    /// every tick, so those echoes always cross the resume, and taken at
    /// face value the two flap between paused and playing for good.
    #[test]
    fn a_stale_pause_echo_after_the_resume_is_ignored() {
        let mut a = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        let at = a.request_pause().expect("a player may pause");
        // Frozen on it, as a peer that heard the pause in time would be.
        while a.frame() < at {
            a.commit_local(PlayerAction::None);
            a.receive(InputMsg {
                player: 1,
                frame: a.frame(),
                action: PlayerAction::None,
            });
            assert!(a.advance().is_some());
        }
        assert!(a.frozen());
        // The peer resumes; its last echo of the pause is still on the wire.
        a.receive_resume(at);
        assert!(!a.paused());
        a.receive_pause(at);
        assert!(!a.paused(), "an echo of a lifted pause is not a pause");
        // Nor is one from a pause that never reached us until after the
        // resume was heard: our own frame is already past it.
        a.receive_pause(at.saturating_sub(1));
        assert!(!a.paused());
        // A fresh pause is a fresh pause.
        let again = a.request_pause().expect("a player may pause");
        assert!(again > at);
        assert!(a.paused());
    }

    /// The same, when the resume comes first and the peer never heard the
    /// pause at all: the frame the resume names is enough to know the
    /// echo is stale.
    #[test]
    fn a_resume_names_the_pause_it_lifts() {
        let mut a = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        a.receive_resume(40);
        assert_eq!(a.lifted_pause(), Some(40));
        a.receive_pause(40);
        assert!(!a.paused());
        a.receive_pause(41);
        assert!(a.paused(), "a later frame is a new pause");
        // Resuming locally lifts through the highest frame known.
        assert_eq!(a.resume(), 41);
    }

    /// A pause proposal that would be read as an echo of a lifted one is
    /// pushed past it, so a second Escape shortly after a resume still
    /// pauses, everywhere.
    #[test]
    fn a_new_pause_is_always_past_the_lifted_one() {
        let mut a = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        a.receive_resume(50);
        let at = a.request_pause().expect("a player may pause");
        assert!(at > 50, "{at}");
        assert_eq!(a.pause_frame(), Some(at));
    }

    /// A resume names a frame off the wire, and the next local pause is
    /// proposed one past it. A peer naming the last frame there is made
    /// that addition overflow: a panic in debug, and in release a proposal
    /// that wrapped to zero and was broadcast anyway. Bounded to the
    /// horizon like every input frame.
    #[test]
    fn a_resume_from_the_end_of_time_cannot_break_the_next_pause() {
        let mut a = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        let lifted = a.receive_resume(u32::MAX);
        assert!(lifted <= 2 * resend_span(DEFAULT_DELAY), "{lifted}");
        let at = a.request_pause().expect("a player may pause");
        assert!(at > lifted, "{at}");
        assert_eq!(a.pause_frame(), Some(at), "and the pause is really held");
        // A pause proposal from past the horizon is dropped the same way.
        let mut b = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        b.receive_pause(u32::MAX);
        assert_eq!(b.pause_frame(), None);
    }
}

#[cfg(test)]
mod abandon_frame_tests {
    use super::*;

    /// Two peers give up on the same seat from the same frame and end up
    /// with the same inputs for it, whatever each of them happened to hold:
    /// the one that held a later input from the departed player plays it
    /// empty like everyone else, rather than as sent.
    #[test]
    fn peers_holding_different_inputs_agree_after_the_abandonment() {
        let mut host = Lockstep::new(0, vec![0, 1, 2], 0);
        let mut peer = Lockstep::new(2, vec![0, 1, 2], 0);
        let place = PlayerAction::Place {
            x: 1,
            y: 1,
            dir: Direction::Up,
        };
        // Seat 1's frame 0 never reached the host (nor, therefore, the
        // peer), but its frame 2 reached the host, and the relay of it
        // reached the peer; the relay of frame 1 was lost on the way.
        for frame in [1, 2] {
            host.receive(InputMsg {
                player: 1,
                frame,
                action: place,
            });
        }
        peer.receive(InputMsg {
            player: 1,
            frame: 2,
            action: place,
        });
        // The host is held up on frame 0 and gives up there.
        let at = host.frame();
        host.abandon(1, at);
        peer.abandon(1, at);
        for frame in 0..3 {
            for session in [&mut host, &mut peer] {
                session.commit_local(PlayerAction::None);
                let other = if session.seat() == Some(0) { 2 } else { 0 };
                session.receive(InputMsg {
                    player: other,
                    frame,
                    action: PlayerAction::None,
                });
            }
            let from_host = host.advance().expect("host moves");
            let from_peer = peer.advance().expect("peer moves");
            assert_eq!(from_host[1], PlayerAction::None, "frame {frame}");
            assert_eq!(from_host, from_peer, "frame {frame}");
        }
    }

    /// Inputs from the departed player that arrive after the abandonment
    /// are ignored, however far ahead they name.
    #[test]
    fn a_departed_seat_is_no_longer_heard() {
        let mut session = Lockstep::new(0, vec![0, 1], 0);
        session.abandon(1, 0);
        session.receive(InputMsg {
            player: 1,
            frame: 5,
            action: PlayerAction::Remove { x: 0, y: 0 },
        });
        for _ in 0..6 {
            session.commit_local(PlayerAction::None);
            let actions = session.advance().expect("moves");
            assert_eq!(actions[1], PlayerAction::None);
        }
    }

    /// A frame further ahead than any peer in step could have committed
    /// makes no slot: a stranger naming frames to the horizon would
    /// otherwise grow the table without bound.
    #[test]
    fn inputs_beyond_the_horizon_are_dropped() {
        let mut session = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        let before = session.pending.len();
        session.receive(InputMsg {
            player: 1,
            frame: u32::MAX,
            action: PlayerAction::None,
        });
        session.receive(InputMsg {
            player: 1,
            frame: 1000,
            action: PlayerAction::None,
        });
        assert_eq!(session.pending.len(), before);
        session.receive(InputMsg {
            player: 1,
            frame: 2 * resend_span(DEFAULT_DELAY),
            action: PlayerAction::None,
        });
        assert_eq!(
            session.pending.len(),
            before + 1,
            "the horizon itself is in"
        );
    }
}
