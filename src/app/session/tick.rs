//! The fixed tick: a replay's recorded inputs, an online frame the table
//! agreed on, or the local seats and their AI, fed to the sim one tick at
//! a time.

use super::*;

/// Fill in the AI seats' moves for a frame.
///
/// Online this runs on every peer over the board they have just agreed on,
/// which is why AI seats are free: no bandwidth, and no chance of
/// divergence, because it is the same state through the same code. A human
/// seat's action is never overwritten, since theirs arrived over the wire.
pub(crate) fn fill_bot_actions(
    board: &Board,
    bots: &Bots,
    actions: &mut [PlayerAction; MAX_PLAYERS],
) {
    debug_assert_eq!(actions.len(), bots.0.len(), "a seat with no bot slot");
    for (seat, action) in actions.iter_mut().enumerate() {
        if let Some(level) = bots.0[seat]
            && matches!(action, PlayerAction::None)
        {
            *action = bot_action(board, seat as u8, level);
        }
    }
}

/// The same thing for a round being played over the wire, where a seat can
/// become an AI part-way through.
///
/// [`Bots`] carries no frame. It is set when the notice reaches the shell,
/// which is a different instant on every peer: the host's own decision,
/// and a datagram on everybody else, landing on a peer that may still have
/// frames in hand it has not simulated yet. Filling from it alone put a
/// bot's move into frames the departed player's own inputs had already
/// arrived for, starting at a different frame on every screen, and the
/// round came apart within seconds of anyone dropping out.
///
/// The frame the seat was given up on travels with the notice, so it is
/// the one thing about the handover every peer does agree on. Read it,
/// and the chair turns AI in the same place everywhere.
fn fill_online_bots(
    board: &Board,
    bots: &Bots,
    abandoned: &[(u8, u32)],
    level: BotLevel,
    frame: u32,
    actions: &mut [PlayerAction; MAX_PLAYERS],
) {
    for (seat, action) in actions.iter_mut().enumerate() {
        if let Some(level) = ai_holding(bots, abandoned, level, seat, frame)
            && matches!(action, PlayerAction::None)
        {
            *action = bot_action(board, seat as u8, level);
        }
    }
}

/// Which level, if any, is playing `seat` on `frame`.
///
/// A seat dealt to the AI at the launch is one every peer has had since
/// before frame zero. One given up on mid-round is an AI from the frame
/// the notice named, and a human with a human's inputs before it.
pub(super) fn ai_holding(
    bots: &Bots,
    abandoned: &[(u8, u32)],
    level: BotLevel,
    seat: usize,
    frame: u32,
) -> Option<BotLevel> {
    match abandoned
        .iter()
        .find(|(gone, _)| usize::from(*gone) == seat)
    {
        Some((_, at)) => (frame >= *at).then_some(level),
        None => bots.0[seat],
    }
}

#[allow(clippy::too_many_arguments)]
pub(in crate::app) fn advance_sim(
    mut sim: ResMut<Sim>,
    mut pending: ResMut<PendingActions>,
    paused: Res<Paused>,
    mut online: ResMut<net::Online>,
    mut recorder: ResMut<Recorder>,
    mut playback: ResMut<Playback>,
    speed: Res<replays::PlaybackSpeed>,
    bots: Res<Bots>,
    mut tally: ResMut<awards::RoundTally>,
) {
    if paused.0 {
        return;
    }
    // Watching a replay: feed the recorded inputs, then stop. At 2x or 4x
    // the tick feeds that many frames, which is scrubbing without a
    // separate code path - the sim is the same either way.
    if let Some((replay, idx)) = &mut playback.0 {
        for _ in 0..speed.0.max(1) {
            let Some(actions) = replay.inputs.get(*idx).copied() else {
                break;
            };
            *idx += 1;
            let before = awards::Reading::of(&sim.0);
            sim.0.tick(&actions);
            tally.observe(before, &sim.0, &actions);
        }
        return;
    }
    // Online: local input goes through the lockstep session; frames simulate
    // only when every player's input is known.
    if let Some(session) = &mut online.0 {
        // A spectator has no seat: it commits nothing and simply simulates
        // the frames the players agree on.
        let local = session.session.seat().map(usize::from);
        // A frame the spectators have spoken for belongs to them. The host
        // has one action a frame like everybody else, so a call takes the
        // frame its own placement would have had, and that placement waits
        // rather than being thrown away.
        let call = session.stands.pending_call;
        let action = match call {
            Some(event) => PlayerAction::CallEvent(event),
            None => local.map_or(PlayerAction::None, |seat| pending.0[seat]),
        };
        // Borrowed past change detection: a mutable borrow marks the
        // resource changed whether or not a frame runs, and while a peer is
        // stalled none does. `observe_sim` reads the flag to skip a board
        // that has not moved, so it is set by hand below when one did.
        let sim_board = &mut sim.bypass_change_detection().0;
        let recording = &mut recorder.bypass_change_detection().0;
        let tally = &mut *tally;
        let bots = &*bots;
        // The level every peer gives an abandoned seat, from the terms the
        // table agreed on, so the chair plays the same on all of them.
        let level = {
            use crate::app::cycle::Cycle;
            BotLevel::from_index(usize::from(session.terms.bot_level))
        };
        let mut advanced = false;
        let committed = session.pump(action, |net| {
            if let Some(mut frame_actions) = net.session.advance() {
                // The lockstep carries only the humans; the AI seats are
                // derived from the frame every peer has just agreed on,
                // which is also the frame that says whether a seat given
                // up on mid-round was an AI yet.
                let at = net.session.frame().saturating_sub(1);
                fill_online_bots(
                    sim_board,
                    bots,
                    &net.abandoned,
                    level,
                    at,
                    &mut frame_actions,
                );
                let before = awards::Reading::of(sim_board);
                sim_board.tick(&frame_actions);
                tally.observe(before, sim_board, &frame_actions);
                if let Some(replay) = recording {
                    replay.record(frame_actions);
                }
                net.after_frame(sim_board.state_hash());
                advanced = true;
            }
        });
        // Late watchers who greeted this tick are sent the round now, off
        // the board as the lockstep's frame leaves it.
        session.send_catch_ups(&sim.bypass_change_detection().0);
        if advanced {
            sim.set_changed();
            recorder.set_changed();
        }
        if call.is_some() {
            // Only a committed call is done with: a stalled or paused
            // commit leaves it for the next frame, or they would have
            // voted for nothing.
            if committed {
                session.stands.pending_call = None;
            }
        } else if let Some(seat) = local
            && (committed || session.session.paused())
        {
            // Only a committed action leaves the queue; at the commit lead
            // (a stalled peer) the press is retried next tick, not dropped.
            // Presses made *during* a pause are dropped instead of queued:
            // nobody expects the game to act on them when it unfreezes.
            pending.0[seat] = PlayerAction::None;
        }
        return;
    }
    let mut actions = std::mem::take(&mut pending.0);
    fill_bot_actions(&sim.0, &bots, &mut actions);
    let before = awards::Reading::of(&sim.0);
    sim.0.tick(&actions);
    tally.observe(before, &sim.0, &actions);
    if let Some(replay) = &mut recorder.0 {
        replay.record(actions);
    }
}
