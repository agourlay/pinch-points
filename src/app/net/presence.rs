//! Who is still at the table, and who has gone.
//!
//! UDP will not say. What it gives is silence, and the whole of this file
//! is the difference between a machine that has stopped talking and a
//! player who is merely behind, because those two look identical from a
//! held-up frame and only one of them should cost somebody their castle.
//!
//! Both halves of that live here: the session's, which decides a seat has
//! gone, and the shell's, which puts an AI in the chair and tells the
//! table. They were a file apart, and the second read as an odd guest in
//! the middle of level loading.

use super::*;
use crate::app::cycle::Cycle;
use crate::app::i18n::fill;
use crate::app::settings::GameSettings;
use crate::app::side_panels::EventLog;
use crate::app::{Bots, Paused, RoundNotice, Screen, SeatNames, palette};
use crate::sim::BotLevel;

/// How long a round waits on a silent player before handing their seat to
/// an AI. Long enough that a burst of loss cannot be mistaken for somebody
/// leaving, since the resend tail repeats every tick, and short enough that a
/// table is not left staring at a still beach.
const ABANDON_AFTER: f32 = 5.0;

/// How long a joiner waits on a host that has stopped speaking before it
/// calls the round off.
///
/// Nothing can save a round whose host has gone: it alone relays every
/// input, decides who has left, and calls the next one. So this is not a
/// patience that buys anything. It is only long enough that a machine
/// which stumbles (a big alt-tab, a sleeping laptop lid, a hotel wifi
/// hiccup) is given a fair chance to come back before its friends are sent
/// home. Four times the patience the host shows an ordinary player, which
/// is the most a table will sit and stare anyway.
const HOST_GONE_AFTER: f32 = 20.0;

/// How long the picture has to have been still before the status line
/// names whoever it is waiting for. Under a second: long enough that the
/// ordinary rhythm of a three-frame input delay never trips it, short
/// enough that nobody has time to ask whether the game has crashed.
const SAY_WAITING_AFTER: f32 = 0.6;

/// How long that line stays up once it has been said, counted only while
/// the round is moving again, so a wait that keeps coming back holds one
/// steady line rather than blinking once per stumble.
///
/// It has to outlast the gap between two stalls in a round that is merely
/// limping, and that gap is at most [`SAY_WAITING_AFTER`] of fresh waiting
/// plus whatever play sits between them. A second and a half covers the
/// former twice over; longer than that and a round that has genuinely
/// recovered goes on being talked about.
const SAY_WAITING_FOR: f32 = 1.5;

/// The line has to be able to survive the gap it is there to cover, or it
/// is a strobe with extra steps.
const _: () = assert!(SAY_WAITING_FOR > SAY_WAITING_AFTER);

impl OnlineSession {
    /// A peer said something, which is all this needs to know: whatever it
    /// said, it is still there.
    pub(super) fn mark_heard(&mut self, peer: usize) {
        self.peers.heard(peer);
    }

    /// Age every peer's silence by a frame, taking in any the socket has
    /// picked up since the last one.
    pub(super) fn age_the_silence(&mut self, delta: f32) {
        // A peer the socket has registered has a row from its first
        // datagram, and a seat in the launch plan has one whether or not
        // its peer is still on the socket. A plan entry with no silence to
        // read would be a seat that could never be given up on, and a
        // round that stalls on a ghost forever.
        self.peers.reach(self.transport.peer_count());
        self.peers.age(delta);
    }

    /// How long the peer holding `seat` has said nothing at all. `None`
    /// for a seat no peer holds: the host's own, and the AI's.
    fn seat_silence(&self, seat: u8) -> Option<f32> {
        let peer = self.peers.holder_of(seat)?;
        self.peers.get(peer).map(|peer| peer.silence)
    }

    /// Whether the host has gone: a joiner's own verdict on the one peer it
    /// has, and the only thing it may decide by itself.
    ///
    /// Deciding *this* alone is safe where deciding a seat is not. An
    /// abandoned seat keeps playing under an AI and every peer must agree
    /// on the frame that happened; a joiner leaving takes nothing with it
    /// but itself.
    ///
    /// A pause makes no difference: the pump runs through one (that is
    /// what carries the resume), so a host that is there keeps talking
    /// however still the picture is, and one that has quit under the pause
    /// card should not leave the table sitting on it for good.
    pub fn host_gone(&self) -> bool {
        !self.is_host()
            && self
                .peers
                .get(0)
                .is_some_and(|host| host.silence >= HOST_GONE_AFTER)
    }

    /// Watch for a player who has stopped sending, and give up on them.
    ///
    /// The host alone decides, and says so; a peer that ran its own timer
    /// would fill the seat on whichever frame its own patience ran out, and
    /// two peers filling it on different frames is a desync.
    ///
    /// Two things have to be true, and the second is the one that matters:
    /// the round is held up by that seat, *and* the machine holding it has
    /// not said a word for [`ABANDON_AFTER`]. A held-up frame on its own is
    /// weak evidence: it is also what a burst of loss looks like, and what
    /// a peer that is a second behind looks like. A table where the slowest
    /// laptop loses its castle every few minutes is worse than one that
    /// waits. Silence is the strong evidence: every peer sends on
    /// every tick it runs, resending every commit a peer could still be
    /// missing, so a machine still in the room is a machine still talking.
    ///
    /// Returns what was given up on this call, which the tests read. The
    /// lasting record is `abandoned`, because a joiner is *told* rather
    /// than deciding and its seats never pass through here.
    pub fn abandon_stalled(&mut self, delta: f32, paused: bool) -> Vec<u8> {
        self.age_the_silence(delta);
        // A paused round is stalled on purpose, and the pause is agreed:
        // holding still is what every peer was asked to do.
        //
        // Both flags, because they are different pauses and only one of
        // them is ever set here: `paused` is the shell's ticker, which an
        // online round deliberately leaves running (see `pause_input`: a
        // frozen ticker would stop the pump that carries the resume), and
        // the session's own is the frame the table agreed to stop on. With
        // only the first, opening the pause card cost every rival their
        // castle five seconds later.
        if paused || self.session.paused() {
            // And a round holding still on purpose is not waiting on
            // anybody, so the line goes at once rather than lingering over
            // the pause card.
            self.stall.reset(self.session.frame());
            return Vec::new();
        }
        // Never itself. The local slot is empty whenever this peer has not
        // committed the frame yet, which is an ordinary moment and not a
        // departure. A host that gave its own castle to an AI would be
        // playing against itself.
        let mine = self.session.seat();
        let waiting: Vec<u8> = self
            .session
            .awaiting()
            .into_iter()
            .filter(|seat| Some(*seat) != mine)
            .collect();
        // The picture moved since this was last looked at, so nothing is
        // stuck, whatever the next frame's slots look like at this instant.
        // That is the question that cannot be asked from here.
        let at = self.session.frame();
        let stall = &mut self.stall;
        if at != stall.stalled_on || waiting.is_empty() {
            stall.stalled_on = at;
            stall.stalled_for = 0.0;
            // The line is let down gently. One frame getting through is
            // not the round recovering: a limping round gets one through
            // every other tick. The clock only runs while the picture is
            // actually moving, so a wait that keeps coming back reads as one
            // steady line rather than a dozen.
            stall.waiting_hold = (stall.waiting_hold - delta).max(0.0);
            if stall.waiting_hold == 0.0 {
                stall.waiting_on = None;
            }
            return Vec::new();
        }
        // Every peer keeps the clock, host or not: a joiner does not decide
        // anything with it, but it is what puts "waiting for Anna" on a
        // screen that has otherwise simply stopped moving.
        stall.stalled_for += delta;
        if stall.stalled_for > SAY_WAITING_AFTER {
            stall.waiting_hold = SAY_WAITING_FOR;
            // The first seat holding the frame up, and never this one: a
            // player is not told the round is waiting for themselves.
            stall.waiting_on = waiting.first().copied();
        }
        if !self.is_host() || self.stall.stalled_for < ABANDON_AFTER {
            return Vec::new();
        }
        let gone: Vec<u8> = waiting
            .into_iter()
            .filter(|seat| match self.seat_silence(*seat) {
                // Held up by them *and* not a word from them.
                Some(since) => since >= ABANDON_AFTER,
                // A seat this session cannot point at on the socket: the
                // direct `PINCH_HOST` pair keeps no launch plan, having
                // never been through a lobby. Nothing better to go on than
                // the held-up frame, as it was before.
                None => true,
            })
            .collect();
        if gone.is_empty() {
            // Still stuck, but on somebody who is plainly still there. Keep
            // the clock running rather than resetting it: the moment they
            // do go quiet, the wait is already served.
            return Vec::new();
        }
        self.stall.stalled_for = 0.0;
        // Emptied from the frame the round is held up on, and that frame
        // travels with the word so every peer empties from the same one
        // (see `Lockstep::abandon`). The notice is repeated every tick from
        // here on by `pump`, so a lost one costs a tick, not the round.
        let frame = self.session.frame();
        for seat in &gone {
            self.session.abandon(*seat, frame);
            self.transport
                .send(NetMsg::Abandoned { seat: *seat, frame });
            if !self.abandoned.iter().any(|(held, _)| held == seat) {
                self.abandoned.push((*seat, frame));
            }
            self.forget_seat(*seat);
        }
        gone
    }

    /// Drop the peer holding `seat` from the socket as well as from the
    /// round.
    ///
    /// Abandoning only unsticks the play; without this the departed peer is
    /// still counted, still holds its place in the launch plan, and is
    /// still dealt a seat in the round after. That round stalls on it for
    /// five seconds and gives up on the same ghost all over again, every
    /// round, until somebody goes back to the lobby.
    fn forget_seat(&mut self, seat: u8) {
        debug_assert!(usize::from(seat) < MAX_PLAYERS, "no such seat: {seat}");
        let Some(peer) = self.peers.holder_of(seat) else {
            return;
        };
        self.transport.forget(peer);
        // The row goes with the socket's entry, so nothing kept about the
        // departed is read against whoever moved up into its index.
        self.peers.forget(peer);
        debug_assert!(
            self.peers.len() <= self.transport.peer_count(),
            "a peer's row outlived the peer it was kept for"
        );
    }

    /// The seat the status line should name, if any.
    ///
    /// A lockstep frame runs only when every seat's input is in, so a
    /// still picture is the ordinary shape of somebody else's trouble.
    /// This lets the screen say whose, instead of leaving a table
    /// of people asking each other whether it has crashed.
    pub fn waiting_on(&self) -> Option<u8> {
        self.stall
            .waiting_on
            .filter(|_| self.stall.waiting_hold > 0.0)
    }
}

// --- the shell's half ------------------------------------------------

/// Hand an abandoned seat to the AI, and say so where it cannot be missed.
///
/// The sim already fills AI seats deterministically on every peer, so the
/// round simply carries on with a bot in the chair: no rollback, no
/// re-agreement, nothing to go out of step. What the shell adds is the
/// bot's level (the one the table agreed on) and telling the players, who
/// would otherwise watch a rival turn strange without explanation.
pub(crate) fn abandon_the_departed(
    time: Res<Time>,
    paused: Res<Paused>,
    settings: Res<GameSettings>,
    names: Res<SeatNames>,
    mut online: ResMut<Online>,
    mut bots: ResMut<Bots>,
    mut log: ResMut<EventLog>,
) {
    let Some(session) = &mut online.0 else {
        return;
    };
    session.abandon_stalled(time.delta_secs(), paused.0);
    let level = BotLevel::from_index(usize::from(session.terms.bot_level));
    let tr = settings.tr();
    // Both roads end in the same list (the host decides, a joiner is told),
    // so reading it is how this stays one piece of code rather than two.
    // A seat that already has a bot in it is a seat already announced.
    for (seat, _) in &session.abandoned {
        let Some(slot) = bots.0.get_mut(usize::from(*seat)) else {
            continue;
        };
        if slot.is_some() {
            continue;
        }
        *slot = Some(level);
        log.push(
            fill(tr.online_seat_abandoned, &[("p", &names.label(tr, *seat))]),
            palette::player_color(*seat),
        );
    }
}

/// Walk a joiner out of a round whose host has gone, and say why.
///
/// Every other kind of departure leaves a round that can go on: a rival's
/// castle is handed to an AI and the beach plays out. A host's cannot. It
/// is the hub of the star: it relays every input, decides who has left,
/// and calls the next round. A table whose host has vanished is not
/// waiting for anything. Before this, it waited anyway: a still beach,
/// forever, with no word about why and nothing on screen to suggest that
/// Escape was the way out.
pub(crate) fn leave_a_hostless_round(
    settings: Res<GameSettings>,
    online: Res<Online>,
    mut notice: ResMut<RoundNotice>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    if online.0.as_ref().is_some_and(OnlineSession::host_gone) {
        // The menu drops the session on its way in, and reads the notice
        // once it is there.
        notice.0 = settings.tr().online_host_gone.to_string();
        next_screen.set(Screen::Menu);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::session::fill_bot_actions;
    use crate::sim::{DEFAULT_DELAY, PlayerAction, classic_arena};
    use crate::transport::UdpTransport;

    /// A paused round is a stalled round on purpose, and the host must not
    /// give up on the table for holding still.
    ///
    /// The pause *is* the stall: every peer stops committing at the agreed
    /// frame, so nobody completes it and `awaiting` names everybody. The
    /// shell's own `Paused` flag is never set in an online round, because
    /// the pause card leaves the ticker running on purpose so the network
    /// pump keeps carrying the resume. So the flag alone said "nothing is
    /// paused", and five seconds later the host handed every rival's castle
    /// to an AI and went on simulating a round its peers were no longer part
    /// of. Pausing for a moment cost you the game.
    #[test]
    fn a_paused_round_is_not_an_abandoned_one() {
        // Delay zero, so frame zero is waiting on both seats from the off:
        // the state a pause holds the round in.
        let transport = UdpTransport::host(0).expect("game socket");
        let mut session = OnlineSession::new(
            transport,
            Lockstep::new(0, vec![0, 1], 0),
            2,
            MatchTerms::default(),
        );
        session.peers.deal(&[Some(1)]);

        // Somebody hit Escape. Every peer stops on one frame, and stays
        // there for as long as the card is up.
        session.request_pause();
        let gone = session.abandon_stalled(ABANDON_AFTER * 3.0, false);
        assert!(gone.is_empty(), "gave up on {gone:?} for pausing");
        assert!(session.abandoned.is_empty());

        // Play on, and the patience starts again from zero rather than
        // firing on the first tick after the resume.
        session.session.resume();
        let gone = session.abandon_stalled(ABANDON_AFTER - 0.1, false);
        assert!(gone.is_empty(), "the timer restarts with the round");
        let gone = session.abandon_stalled(0.2, false);
        assert_eq!(gone, vec![1], "and a seat that really is silent still goes");
    }

    /// A peer that is still talking keeps its castle, however far behind
    /// its inputs are.
    ///
    /// A held-up frame is weak evidence: it is equally what a burst of loss
    /// looks like, and what a laptop half a second behind looks like. The
    /// strong evidence is silence on the socket, since every peer sends on
    /// every tick it runs. The slowest machine at the table losing its
    /// castle to an AI every few minutes is the failure that makes people
    /// stop playing.
    #[test]
    fn a_peer_that_is_still_talking_keeps_its_castle() {
        let transport = UdpTransport::host(0).expect("game socket");
        let mut session = OnlineSession::new(
            transport,
            Lockstep::new(0, vec![0, 1], 0),
            2,
            MatchTerms::default(),
        );
        session.peers.deal(&[Some(1)]);

        // Held up on seat 1 for four times the patience, a frame at a time,
        // but its machine keeps talking through every one of them.
        let frame = 0.1;
        for _ in 0..(ABANDON_AFTER * 4.0 / frame) as usize {
            session.mark_heard(0);
            assert!(
                session.abandon_stalled(frame, false).is_empty(),
                "gave up on somebody who is right there"
            );
        }
        assert!(session.abandoned.is_empty());

        // And then the machine goes quiet, which is how leaving looks from
        // out here. The frame was already overdue, so the seat goes as
        // soon as the silence adds up.
        let mut gone = Vec::new();
        for _ in 0..(ABANDON_AFTER * 2.0 / frame) as usize {
            gone = session.abandon_stalled(frame, false);
            if !gone.is_empty() {
                break;
            }
        }
        assert_eq!(gone, vec![1], "a seat that really is silent still goes");
        assert_eq!(session.abandoned.len(), 1);
        assert_eq!(session.abandoned[0].0, 1);
    }

    /// A joiner whose host has gone quiet calls the round off itself.
    ///
    /// Nothing else can: the host relays every input, decides who has left,
    /// and calls the next round, so a table whose host has vanished is not
    /// waiting for anything. Deciding this alone is safe where deciding a
    /// seat is not: leaving takes nothing with it but yourself.
    #[test]
    fn a_joiner_gives_up_on_a_host_that_has_gone() {
        let transport = UdpTransport::host(0).expect("game socket");
        let mut joiner = OnlineSession::new(
            transport,
            Lockstep::new(1, vec![0, 1], 0),
            2,
            MatchTerms::default(),
        );
        assert!(!joiner.host_gone(), "nothing has had time to go wrong");

        joiner.mark_heard(0);
        joiner.abandon_stalled(HOST_GONE_AFTER - 1.0, false);
        assert!(!joiner.host_gone(), "still within a fair stumble");
        joiner.abandon_stalled(2.0, false);
        assert!(joiner.host_gone(), "and then the host is gone");

        // A host never decides this about itself, however quiet the table.
        let mut host = OnlineSession::new(
            UdpTransport::host(0).expect("game socket"),
            Lockstep::new(0, vec![0, 1], 0),
            2,
            MatchTerms::default(),
        );
        host.peers.deal(&[Some(1)]);
        host.abandon_stalled(HOST_GONE_AFTER * 2.0, false);
        assert!(!host.host_gone(), "the host is the host");
    }

    /// A table reading the scores together is not a table whose host has
    /// gone, however quiet it is.
    ///
    /// The trap under this fix, and the worse bug of the two: a settled
    /// session falls silent between rounds on purpose (see `greet_in`), so
    /// silence read as evidence there walks every joiner out of a
    /// perfectly good table.
    ///
    /// The greeting is what makes the silence mean something. Over real
    /// sockets, because the point is that a reply actually comes back.
    #[test]
    fn a_quiet_results_card_is_not_a_host_that_has_gone() {
        let mut host = OnlineSession::new(
            UdpTransport::host(0).expect("game socket"),
            Lockstep::new(0, vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        let port = host.transport.local_addr().expect("addr").port();
        let mut joiner = OnlineSession::new(
            UdpTransport::join(("127.0.0.1", port)).expect("join"),
            Lockstep::new(1, vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );

        // Twice the patience, spent sitting on the card. Nobody presses
        // anything; the only traffic is the greeting and its answer.
        let step = 0.5;
        for _ in 0..(HOST_GONE_AFTER * 2.0 / step) as usize {
            joiner.poll_between_rounds(step);
            std::thread::sleep(std::time::Duration::from_millis(2));
            host.poll_between_rounds(step);
            std::thread::sleep(std::time::Duration::from_millis(2));
            joiner.poll_between_rounds(0.0);
            assert!(
                !joiner.host_gone(),
                "walked out on a host that is answering"
            );
        }
        assert_eq!(host.transport.peer_count(), 1, "and the host heard it");

        // Now the host stops answering, which is the case this is all for.
        drop(host);
        for _ in 0..(HOST_GONE_AFTER * 2.0 / step) as usize {
            joiner.poll_between_rounds(step);
        }
        assert!(joiner.host_gone(), "and a card over a dead host says so");
    }

    /// A round that is running says nothing at all, and gives nobody's
    /// castle away.
    ///
    /// This is the bug under the reported one. A lockstep sim advances
    /// until it *cannot*, and it does that on the fixed step; the tick that
    /// watches for stalls runs afterwards, in `Update`. So "are the next
    /// frame's inputs in?" is asked at the one moment in the frame where
    /// the answer is always no, and every healthy online round was read as
    /// permanently stalled: both screens said "waiting for Bob" through a
    /// match that was running perfectly, and the host was five seconds from
    /// handing every rival's seat to an AI at all times, held back by
    /// nothing but the socket-silence rule.
    ///
    /// The picture moving is the only thing either reader wanted to know,
    /// and it is not a matter of timing within a frame.
    #[test]
    fn a_round_that_is_running_says_nothing() {
        use crate::sim::PlayerAction;

        let transport = UdpTransport::host(0).expect("game socket");
        let mut session = OnlineSession::new(
            transport,
            Lockstep::new(0, vec![0, 1], 0),
            2,
            MatchTerms::default(),
        );
        session.peers.deal(&[Some(1)]);
        let frame = 1.0 / 60.0;

        // Ten seconds of an ordinary round: every input arrives, every
        // frame runs. Twice the patience the host shows a silent player.
        for _ in 0..(10.0 / frame) as usize {
            let at = session.session.frame();
            session.session.commit_local(PlayerAction::None);
            session.session.receive(crate::sim::InputMsg {
                player: 1,
                frame: at,
                action: PlayerAction::None,
            });
            assert!(session.session.advance().is_some(), "the frame ran");
            session.mark_heard(0);
            let gone = session.abandon_stalled(frame, false);
            assert!(gone.is_empty(), "gave {gone:?} away mid-round");
            assert_eq!(
                session.waiting_on(),
                None,
                "said so about a round that is running"
            );
        }
        assert!(session.abandoned.is_empty());
    }

    /// A round that keeps stopping and starting says so once, not once per
    /// stumble.
    ///
    /// The bug this was reported as: "constant flickering in the corner".
    /// The line was read straight off the stall clock, and that clock snaps
    /// back to zero the instant one frame gets through, and a limping round
    /// does that over and over. So the line came up, a
    /// frame landed, it went, the wait built back past the threshold, it
    /// came up again: a strobe in the corner of the eye of somebody trying
    /// to play.
    #[test]
    fn a_round_that_keeps_stumbling_says_so_once() {
        use crate::sim::PlayerAction;

        let transport = UdpTransport::host(0).expect("game socket");
        let mut session = OnlineSession::new(
            transport,
            Lockstep::new(0, vec![0, 1], 0),
            2,
            MatchTerms::default(),
        );
        session.peers.deal(&[Some(1)]);
        let frame = 1.0 / 60.0;

        /// A frame of a round that is moving, in the order the app runs it:
        /// the sim advances on the fixed step, as far as it can, and only
        /// then does the tick that watches for stalls get a look. It
        /// therefore always finds the *next* frame's slots empty. That is
        /// why this cannot be judged by looking at them.
        fn moving(session: &mut OnlineSession, delta: f32) {
            let at = session.session.frame();
            session.session.commit_local(PlayerAction::None);
            session.session.receive(crate::sim::InputMsg {
                player: 1,
                frame: at,
                action: PlayerAction::None,
            });
            assert!(session.session.advance().is_some(), "the frame went");
            assert!(
                !session.session.awaiting().is_empty(),
                "a healthy round looks stalled from here, every single time"
            );
            session.mark_heard(0);
            session.abandon_stalled(delta, false);
        }

        /// A frame of a round that is not: seat 1's input has not arrived,
        /// so nothing runs.
        fn stalling(session: &mut OnlineSession, delta: f32) {
            session.mark_heard(0);
            session.abandon_stalled(delta, false);
        }

        // Seat 1 is late. Nothing is said at first: a moment's wait is the
        // ordinary rhythm of a delayed lockstep, not news.
        stalling(&mut session, frame);
        assert_eq!(session.waiting_on(), None, "not for an ordinary moment");
        for _ in 0..(SAY_WAITING_AFTER / frame) as usize + 1 {
            stalling(&mut session, frame);
        }
        assert_eq!(session.waiting_on(), Some(1), "and then it says whose");

        // The round limps: a stretch of play, a stall, a stretch of play.
        // Every one of those stretches used to blink the line out and every
        // stall used to bring it back, which is the strobe as reported.
        for _ in 0..20 {
            for _ in 0..(0.3 / frame) as usize {
                moving(&mut session, frame);
                assert_eq!(session.waiting_on(), Some(1), "the line blinked out");
            }
            for _ in 0..(SAY_WAITING_AFTER / frame) as usize + 1 {
                stalling(&mut session, frame);
                assert_eq!(session.waiting_on(), Some(1), "nor back on again");
            }
        }

        // A round that really does recover drops the line after a beat, not
        // on the first frame through. The same rule, read the other way
        // round.
        moving(&mut session, frame);
        assert_eq!(
            session.waiting_on(),
            Some(1),
            "gone on the very first frame is the strobe again"
        );
        for _ in 0..(SAY_WAITING_FOR / frame) as usize + 6 {
            moving(&mut session, frame);
        }
        assert_eq!(
            session.waiting_on(),
            None,
            "and a healthy round says nothing"
        );
    }

    /// Play until the round can go no further. A fresh session is not
    /// stalled: the delay window is pre-filled, so the first few frames
    /// simulate whether anyone has sent anything or not, and only after
    /// them does a silent seat start holding the table up.
    fn run_into_the_stall(session: &mut OnlineSession) {
        for _ in 0..40 {
            session.pump(
                PlayerAction::None,
                |net| {
                    while net.session.advance().is_some() {}
                },
            );
        }
        assert!(
            !session.session.awaiting().is_empty(),
            "expected the round to be held up by somebody"
        );
        // And one look at the round while no time passes, so the stall clock
        // knows which frame it stopped on. The real caller looks sixty times
        // a second; a test that leaps five seconds in a single call would
        // otherwise be timing a frame the round had only just arrived at.
        session.abandon_stalled(0.0, false);
    }

    fn hosting(players: Vec<u8>) -> OnlineSession {
        OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            Lockstep::new(0, players, DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        )
    }

    /// The measured behaviour that started this: a player walks away and
    /// the round advances three frames (the input-delay window) and then
    /// stops dead, forever, with nothing said. It has to come back.
    #[test]
    fn a_round_left_hanging_starts_again_without_the_player() {
        let mut session = hosting(vec![0, 1]);
        let mut frames = 0;
        let step = |session: &mut OnlineSession, frames: &mut u32| {
            session.pump(PlayerAction::None, |net| {
                while net.session.advance().is_some() {
                    *frames += 1;
                }
            });
        };
        // Seat 1 never sends anything at all.
        for _ in 0..40 {
            step(&mut session, &mut frames);
        }
        let stuck_at = frames;
        assert!(
            stuck_at <= DEFAULT_DELAY,
            "it got no further than the delay"
        );

        // Patience runs out only after the wait, not before: a burst of
        // lost packets must not be mistaken for somebody leaving.
        assert!(
            session
                .abandon_stalled(ABANDON_AFTER / 2.0, false)
                .is_empty()
        );
        assert_eq!(frames, stuck_at, "and nothing has moved yet");

        let given_up = session.abandon_stalled(ABANDON_AFTER, false);
        assert_eq!(given_up, vec![1], "the seat that went quiet");
        for _ in 0..40 {
            step(&mut session, &mut frames);
        }
        assert!(
            frames > stuck_at,
            "the round is still frozen at frame {stuck_at}"
        );
    }

    /// A paused round is stalled on purpose and by agreement. Giving up on
    /// everybody who is politely waiting would be a rout.
    #[test]
    fn a_pause_is_not_a_departure() {
        let mut session = hosting(vec![0, 1]);
        run_into_the_stall(&mut session);
        for _ in 0..20 {
            assert!(session.abandon_stalled(ABANDON_AFTER, true).is_empty());
        }
    }

    /// Only the host decides (see [`OnlineSession::abandon_stalled`]):
    /// two peers filling a seat on different frames is a desync lockstep
    /// cannot recover from.
    #[test]
    fn a_joiner_never_decides_for_itself() {
        let mut joiner = OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            Lockstep::new(1, vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        assert!(!joiner.is_host());
        run_into_the_stall(&mut joiner);
        for _ in 0..20 {
            assert!(joiner.abandon_stalled(ABANDON_AFTER, false).is_empty());
        }
        assert!(joiner.abandoned.is_empty(), "and nothing was given up");
    }

    /// Abandoning has to empty the chair for good. Unsticking the play is
    /// only half of it: a peer that is still counted still holds its place
    /// in the launch plan, is dealt a seat in the round after, and stalls
    /// that one too: five seconds of nothing at the start of every round
    /// from then on, for a player who left once.
    #[test]
    fn an_abandoned_player_does_not_haunt_the_next_round() {
        let mut host = hosting(vec![0, 1]);
        let port = host.transport.local_addr().expect("addr").port();
        let joiner = UdpTransport::join(("127.0.0.1", port)).expect("join");
        joiner.send(NetMsg::hello("Bo"));
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            host.poll_between_rounds(0.0);
            if host.transport.peer_count() == 1 {
                break;
            }
        }
        assert_eq!(host.transport.peer_count(), 1);
        host.peers.deal(&[Some(1)]);
        drop(joiner);

        run_into_the_stall(&mut host);
        assert_eq!(host.abandon_stalled(ABANDON_AFTER * 2.0, false), vec![1]);
        assert_eq!(host.transport.peer_count(), 0, "and gone from the socket");
        assert!(host.peers.is_empty(), "and from the launch plan");

        // The round after is the host's alone, and waits on nobody. It is
        // still a two-castle beach: a host left by itself plays the AI
        // rather than plays itself.
        host.call_next_round(
            MatchTerms {
                seed: 42,
                ..MatchTerms::default()
            },
            None,
        );
        assert_eq!(host.session.player_count(), 1, "still waiting on a ghost");
        assert_eq!(host.seats, 2, "a beach needs two castles");
        assert_eq!(host.terms.bots, 1, "and somebody in the other one");
    }

    /// A host never gives up on itself. Its own slot is empty whenever it
    /// has not committed this frame yet, which is an ordinary moment. A
    /// host that handed its own castle to an AI would be sitting there
    /// watching a bot play its round.
    #[test]
    fn a_host_never_abandons_its_own_castle() {
        let mut session = hosting(vec![0]);
        // Alone at the table, and the only slot ever outstanding is its own.
        for _ in 0..40 {
            session.pump(
                PlayerAction::None,
                |net| {
                    while net.session.advance().is_some() {}
                },
            );
        }
        for _ in 0..20 {
            assert!(
                session
                    .abandon_stalled(ABANDON_AFTER * 2.0, false)
                    .is_empty()
            );
        }
        assert!(session.abandoned.is_empty());
    }

    /// Giving up is remembered, so the seat is filled and announced once
    /// rather than every frame the round keeps running.
    #[test]
    fn a_seat_is_given_up_once() {
        let mut session = hosting(vec![0, 1]);
        run_into_the_stall(&mut session);
        assert_eq!(session.abandon_stalled(ABANDON_AFTER * 2.0, false), vec![1]);
        assert_eq!(session.abandoned.len(), 1);
        assert_eq!(session.abandoned[0].0, 1);
        // The round moves now, so there is nothing left to be waited on.
        for _ in 0..10 {
            assert!(
                session
                    .abandon_stalled(ABANDON_AFTER * 2.0, false)
                    .is_empty()
            );
        }
        assert_eq!(session.abandoned.len(), 1, "still just the one");
    }

    /// The whole thing, driven by the system that really runs it: a player
    /// stops sending, the round comes back, an AI is holding their castle,
    /// and the feed says so. Built as the app builds it, because the parts
    /// were each right the last time something like this broke and it was
    /// the wiring between them that was not.
    #[test]
    fn a_departed_player_is_replaced_by_an_ai_and_the_feed_says_so() {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default());
        app.insert_resource(Paused(false));
        app.insert_resource(GameSettings::default());
        app.init_resource::<SeatNames>();
        app.init_resource::<Bots>();
        app.init_resource::<EventLog>();

        let mut session = OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            Lockstep::new(0, vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        // Run the round into the stall the missing player causes.
        for _ in 0..40 {
            session.pump(crate::sim::PlayerAction::None, |net| {
                while net.session.advance().is_some() {}
            });
        }
        app.insert_resource(Online(Some(session)));
        app.add_systems(Update, abandon_the_departed);

        // Not yet: a burst of loss is not somebody leaving.
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(500));
        app.update();
        assert_eq!(
            app.world().resource::<Bots>().0[1],
            None,
            "gave up on them after half a second"
        );
        assert!(app.world().resource::<EventLog>().0.is_empty());

        // And then it is.
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(6));
        app.update();
        assert!(
            app.world().resource::<Bots>().0[1].is_some(),
            "the empty castle has nobody in it"
        );
        let log = app.world().resource::<EventLog>();
        assert_eq!(log.0.len(), 1, "the players are told, once");

        // The round really does move again, with the AI supplying the seat.
        let mut board = classic_arena(false, 2);
        let bots = Bots(app.world().resource::<Bots>().0);
        let bots = &bots;
        let mut online = app.world_mut().resource_mut::<Online>();
        let session = online.0.as_mut().expect("a session");
        let mut moved = 0;
        for _ in 0..40 {
            session.pump(crate::sim::PlayerAction::None, |net| {
                while let Some(mut actions) = net.session.advance() {
                    fill_bot_actions(&board, bots, &mut actions);
                    board.tick(&actions);
                    moved += 1;
                }
            });
        }
        assert!(moved > 0, "the beach is still frozen");

        // Said once and not once a frame, however long it plays on.
        app.update();
        assert_eq!(app.world().resource::<EventLog>().0.len(), 1);
    }
}
