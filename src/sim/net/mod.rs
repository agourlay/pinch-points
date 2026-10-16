//! Online play, sim side (spec §7.6).
//!
//! `bevy_ggrs`/`bevy_matchbox` still target Bevy 0.18 (verified 2026-07;
//! the §9 risk-6 ecosystem lag is real), so this ships the spec's designated
//! fallback: **deterministic lockstep with a small input delay**. The
//! protocol lives here, transport-agnostic and engine-free, so it is
//! unit-testable and reusable; the shell owns sockets. The packed input
//! (3 bytes an action, 8 an [`InputMsg`]) and the `Board`'s clone + `state_hash` snapshots are exactly the
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
    /// The pause called and the pauses lifted. See [`PauseState`].
    pause: PauseState,
    /// Seats this peer commits for besides its own: a host speaking for
    /// the bots connected to it (route 2 of `docs/bot-seats.md`). Each gets
    /// an input on every frame the local seat does, so the table never
    /// waits on one; to the other peers they are players like any other.
    also: Vec<PlayerId>,
    /// The other players' inputs accepted here, kept for the same span as
    /// `history`: what the host passes on again for a player it gives up
    /// on (see [`Lockstep::abandon`]).
    heard: Vec<InputMsg>,
    /// Players given up on, and the frame from which their seat is empty.
    /// Their inputs for the frames before it are still the real ones, and
    /// still awaited and accepted.
    leaving: Vec<(PlayerId, u32)>,
}

/// A pause as one peer sees it: the frame the session is to freeze on,
/// and the highest pause ever lifted. Pure bookkeeping over frame numbers,
/// with the session's own frame and horizon handed in, so the rules that
/// keep peers converging on one pause (earliest proposal wins, echoes of a
/// lifted pause are dropped) sit together and apart from the input queue.
#[derive(Clone, Copy, Debug, Default)]
struct PauseState {
    /// The agreed frame the session freezes on, if a peer has called a
    /// pause.
    at: Option<u32>,
    /// The highest pause frame ever lifted here, by our own resume or a
    /// peer's. A `Pause` naming that frame or an earlier one is an echo of
    /// a pause that is over and is ignored. Without this the pause flapped:
    /// peers repeat `Pause` every tick, so the last echoes cross the
    /// `Resume` and re-pause whoever had just resumed.
    lifted: Option<u32>,
}

impl PauseState {
    /// This peer's own proposal, made when its next commit is `next`: the
    /// frame the pause in flight, earliest first, settles on.
    fn request(&mut self, next: u32, now: u32, horizon: u32) -> u32 {
        // Past every pause already lifted, or the proposal would be read as
        // an echo of one of them (see `lifted`). A frame the others may
        // already have committed past is still a sound pause frame: they
        // stop committing and the sim comes to rest a beat later.
        let mut frame = next + PAUSE_LEAD;
        if let Some(lifted) = self.lifted {
            frame = frame.max(lifted + 1);
        }
        self.propose(frame, now, horizon);
        self.at.unwrap_or(frame)
    }

    /// A proposal to freeze on `frame`, from here or a peer, taken when
    /// the session has simulated up to `now` and accepts frames up to
    /// `horizon`. See [`Lockstep::receive_pause`].
    fn propose(&mut self, frame: u32, now: u32, horizon: u32) {
        if frame < now || frame > horizon || self.is_lifted(frame) {
            return;
        }
        self.at = Some(match self.at {
            Some(existing) => existing.min(frame),
            None => frame,
        });
    }

    /// Lift this peer's pause; see [`Lockstep::resume`].
    fn resume(&mut self) -> u32 {
        self.lift(self.at.unwrap_or(0))
    }

    /// A peer's resume of the pause at `frame`; see
    /// [`Lockstep::receive_resume`].
    fn receive_resume(&mut self, frame: u32, horizon: u32) -> Option<u32> {
        // Off the wire, so bounded like every other frame number that
        // arrives that way: `lifted` is added to when the next pause is
        // proposed, and a peer naming the last frame there is would have
        // had that addition overflow. One short of the horizon, so that
        // next pause, one past this, is still a frame this peer accepts.
        let frame = frame.min(horizon.saturating_sub(1));
        if self.is_lifted(frame) {
            return None;
        }
        Some(self.lift(frame))
    }

    /// Lift the pause in flight, if any, and remember the highest frame
    /// lifted so far.
    fn lift(&mut self, frame: u32) -> u32 {
        let lifted = [self.lifted, self.at, Some(frame)]
            .into_iter()
            .flatten()
            .max()
            .unwrap_or(0);
        self.lifted = Some(lifted);
        self.at = None;
        lifted
    }

    /// Whether a pause at `frame` is one already lifted here.
    fn is_lifted(&self, frame: u32) -> bool {
        self.lifted.is_some_and(|lifted| frame <= lifted)
    }

    /// Whether `frame` is at or past the pause in flight: the sim stops
    /// there, and so do commits.
    fn reached(&self, frame: u32) -> bool {
        self.at.is_some_and(|at| frame >= at)
    }
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
            also: Vec::new(),
            pause: PauseState::default(),
            heard: Vec::new(),
            leaving: Vec::new(),
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
        let leaving = &self.leaving;
        self.pending.entry(frame).or_insert_with(|| {
            let mut slot = [None; MAX_PLAYERS];
            for p in 0..MAX_PLAYERS as u8 {
                let still_owed = leaving
                    .iter()
                    .any(|&(gone, from)| gone == p && frame < from);
                if !players.contains(&p) && !still_owed {
                    slot[p as usize] = Some(PlayerAction::None); // absent seats
                }
            }
            slot
        })
    }

    /// Whether `player`'s input for `frame` is one this session takes: a
    /// seated player's, or one given up on from a later frame.
    fn takes(&self, player: PlayerId, frame: u32) -> bool {
        self.players.contains(&player)
            || self
                .leaving
                .iter()
                .any(|&(gone, from)| gone == player && frame < from)
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
        let (now, horizon) = (self.frame, self.horizon());
        Some(self.pause.request(self.next_commit, now, horizon))
    }

    /// A peer called a pause at `frame`. The earliest proposal wins, so
    /// every peer converges on one frame however the messages interleave.
    ///
    /// Two kinds of `Pause` are not proposals and are dropped: one naming
    /// a frame this peer has already simulated (every player committed
    /// past it, so nobody is stopping there), and one naming a pause that
    /// has since been lifted (see [`PauseState::lifted`]). Both are the per-tick echoes
    /// of a pause that is over, arriving after the resume.
    pub fn receive_pause(&mut self, frame: u32) {
        let (now, horizon) = (self.frame, self.horizon());
        self.pause.propose(frame, now, horizon);
    }

    /// Lift the pause and let commits flow again. Safe to call unpaused.
    /// Returns the frame the lifted pause was to freeze on (the highest
    /// ever lifted here, if there was no pause to lift), for the peers.
    pub fn resume(&mut self) -> u32 {
        self.pause.resume()
    }

    /// A peer lifted the pause that was to freeze on `frame`. Whatever
    /// pause is in flight here is lifted with it: the peers agree on one
    /// frame, and a peer that had not yet heard the earliest proposal is
    /// resuming from the same pause under a later number.
    ///
    /// `None` for a resume of a pause already lifted here: the per-tick
    /// echo of an old resume, which says nothing new. It must not lift the
    /// pause in flight, which is a later one (every proposal is past
    /// `lifted`), or the first resume of a match would cancel every pause
    /// after it for as long as its echoes kept arriving.
    ///
    /// Nor is a resume past the [`horizon`](Self::horizon) while no pause
    /// is in flight here: there is nothing for it to lift, and the one
    /// thing it would do is raise `lifted`, so this peer's next pause and
    /// every peer's early one read as echoes. That is a resume from the
    /// last round still on the wire when this one began at frame zero. With
    /// a pause in flight it still lifts it: a peer that resumed its own
    /// later proposal before hearing the earliest names a frame this one
    /// may never have accepted.
    pub fn receive_resume(&mut self, frame: u32) -> Option<u32> {
        if !self.within_horizon(frame) && !self.paused() {
            return None;
        }
        let horizon = self.horizon();
        self.pause.receive_resume(frame, horizon)
    }

    /// Whether a `Pause` naming `frame` is of a pause already lifted here:
    /// a peer still announcing it never heard the resume.
    pub fn is_lifted(&self, frame: u32) -> bool {
        self.pause.is_lifted(frame)
    }

    /// The frame this session is frozen on, if paused.
    pub fn pause_frame(&self) -> Option<u32> {
        self.pause.at
    }

    /// The frame of the pause most recently lifted, if any ever was: what
    /// a repeated `Resume` names.
    pub fn lifted_pause(&self) -> Option<u32> {
        self.pause.lifted
    }

    /// Whether a pause has been called (the freeze itself lands a beat
    /// later, when the simulated frame reaches [`Lockstep::pause_frame`]).
    pub fn paused(&self) -> bool {
        self.pause.at.is_some()
    }

    /// Whether the sim has actually come to rest on the pause frame: the
    /// moment the picture on screen stops moving.
    pub fn frozen(&self) -> bool {
        self.pause.reached(self.frame)
    }

    /// Commit the local action for the next input frame; returns the message
    /// to send to every peer, or `None` if the sim has fallen too far behind
    /// (a stalled peer) or the session is paused. The caller should retry
    /// the action next frame rather than let commits run unboundedly ahead.
    /// Speak for `seats` too, from the next commit on (see `also`). Seats
    /// that are not players of this session are ignored.
    pub fn speak_for(&mut self, seats: &[PlayerId]) {
        self.also = seats
            .iter()
            .copied()
            .filter(|seat| self.players.contains(seat) && Some(*seat) != self.local)
            .collect();
    }

    /// The seats this peer speaks for besides its own.
    pub fn speaks_for(&self) -> &[PlayerId] {
        &self.also
    }

    /// [`Self::commit_local`], and on the same frame an input for every
    /// seat this peer speaks for: the one given in `extra`, or nothing for
    /// a seat it does not name. All or none: a commit refused for the local
    /// seat is refused for every seat.
    pub fn commit_local_with(
        &mut self,
        action: PlayerAction,
        extra: &[(PlayerId, PlayerAction)],
    ) -> Option<InputMsg> {
        let msg = self.commit_local(action)?;
        for seat in self.also.clone() {
            let given = extra
                .iter()
                .find(|(s, _)| *s == seat)
                .map_or(PlayerAction::None, |(_, a)| *a);
            let given = decode_action(encode_action(given));
            self.slot(msg.frame)[usize::from(seat)] = Some(given);
            self.history.push(InputMsg {
                player: seat,
                frame: msg.frame,
                action: given,
            });
        }
        Some(msg)
    }

    pub fn commit_local(&mut self, action: PlayerAction) -> Option<InputMsg> {
        if self.watching() {
            return None; // nothing to commit, and nobody waiting for it
        }
        if self.pause.reached(self.next_commit) {
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
    /// screen. Only seated players can hold a frame up, and one given up on
    /// that still owes the frame (see `abandon`); an absent seat is filled
    /// at the moment its frame is made.
    pub fn awaiting(&self) -> Vec<PlayerId> {
        // A seat given up on from a later frame still owes this one (see
        // `abandon`), and is waited on like a seated player until it pays.
        let frame = self.frame;
        let owing = self
            .leaving
            .iter()
            .filter(|&&(_, from)| frame < from)
            .map(|&(gone, _)| gone);
        let waited: Vec<PlayerId> = self.players.iter().copied().chain(owing).collect();
        let Some(slot) = self.pending.get(&self.frame) else {
            // No frame made yet means nothing has arrived for it, so
            // everybody still seated is being waited on.
            return waited;
        };
        waited
            .into_iter()
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
    ///
    /// The frames *before* `frame` are the other way round: the decider
    /// played every one of them with the player's real input, and a peer
    /// behind it may still be missing some (the relay was lost, and the
    /// player has stopped resending). Those slots are left waiting and
    /// still accept the player's input, which the decider sends again
    /// ([`Lockstep::owed_by`]). Filled empty, as they used to be, that peer
    /// played a frame the host never did, and lockstep cannot recover a
    /// desync.
    pub fn abandon(&mut self, player: PlayerId, frame: u32) {
        debug_assert!(
            usize::from(player) < MAX_PLAYERS,
            "no such seat to abandon: {player}"
        );
        self.players.retain(|seated| *seated != player);
        if !self.leaving.iter().any(|&(gone, _)| gone == player) {
            self.leaving.push((player, frame));
        }
        for (&at, slot) in self.pending.iter_mut() {
            if at >= frame {
                slot[player as usize] = Some(PlayerAction::None);
            }
        }
    }

    /// The inputs a player given up on at `frame` sent for the frames
    /// before it, as far back as a peer still in step could be missing
    /// them: what the decider repeats alongside the notice.
    ///
    /// Bounded by the same span as `heard`, read against the frame now
    /// rather than the last time anything arrived: at a table with nobody
    /// else left to hear from, `heard` is never trimmed, and the repeat
    /// went out every tick until the round ended.
    pub fn owed_by(&self, player: PlayerId, frame: u32) -> Vec<InputMsg> {
        let oldest_wanted = self.frame.saturating_sub(resend_span(self.delay));
        self.heard
            .iter()
            .copied()
            .filter(|msg| msg.player == player && msg.frame < frame && msg.frame >= oldest_wanted)
            .collect()
    }

    /// The furthest frame a peer may name: as far ahead of the frame being
    /// simulated as a resend span twice over. Frame numbers arrive off the
    /// wire, and one past this is either a peer that has run away from the
    /// table or one that is not playing fair; both are dropped.
    fn horizon(&self) -> u32 {
        self.frame + 2 * resend_span(self.delay)
    }

    /// Whether a frame number off the wire is one a peer may name now: not
    /// past the [`horizon`](Self::horizon). Inputs, pauses and state hashes
    /// past it are dropped alike, which is also what keeps the last round's
    /// stragglers (named near its end) out of a round begun at frame zero.
    pub fn within_horizon(&self, frame: u32) -> bool {
        frame <= self.horizon()
    }

    /// Feed a peer's message (duplicates and already-simulated frames are
    /// ignored, so resends are harmless).
    ///
    /// A frame past the [`horizon`](Self::horizon) is ignored too: every
    /// frame accepted here makes a slot, and a peer naming frames up to
    /// `u32::MAX` would grow the table without bound.
    pub fn receive(&mut self, msg: InputMsg) {
        if msg.frame < self.frame
            || !self.within_horizon(msg.frame)
            || !self.takes(msg.player, msg.frame)
        {
            return;
        }
        // Past the guard, the sender is a seated player (or one leaving),
        // so the index below is in range, which is the only reason it is
        // written as one.
        debug_assert!(usize::from(msg.player) < MAX_PLAYERS);
        let slot = self.slot(msg.frame);
        if slot[msg.player as usize].is_none() {
            slot[msg.player as usize] = Some(msg.action);
            self.heard.push(msg);
            let oldest_wanted = self.frame.saturating_sub(resend_span(self.delay));
            self.heard.retain(|kept| kept.frame >= oldest_wanted);
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
        let frame = self.frame;
        self.leaving.retain(|&(_, from)| from > frame);
        Some(actions)
    }

    /// The frame the next local commit will be for: what a seat driven
    /// from elsewhere (a bot) needs to match its outcome to a frame.
    pub fn next_commit(&self) -> u32 {
        self.next_commit
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
mod tests;
