//! Who is still at the table, and who has gone.
//!
//! UDP will not say. What it gives is silence, and the whole of this file
//! is the difference between a machine that has stopped talking and a
//! player who is merely behind, because those two look identical from a
//! held-up frame and only one of them should cost somebody their castle.
//!
//! Both halves of that live here: the session's, which decides a seat has
//! gone, and the shell's, which puts an AI in the chair and tells the
//! table.

use super::*;
use crate::app::i18n::fill;
use crate::app::match_setup::TermsDials;
use crate::app::settings::GameSettings;
use crate::app::side_panels::EventLog;
use crate::app::{Controllers, Paused, RoundNotice, Screen, SeatController, SeatNames, palette};

/// Whether the round has stopped moving, for how long, and who the status
/// line is naming for it.
///
/// Measured by the frame number rather than by whether the next frame's
/// inputs happen to be in, because the second question is asked at a
/// moment where the answer is always no: the sim advances until it
/// *cannot*, and the tick that watches for stalls runs after it, in
/// `Update`. So it always found the next frame's slots empty and always
/// called a healthy round stalled.
#[derive(Default)]
pub(super) struct StallWatch {
    /// Seconds the round has been unable to move, and the frame it has
    /// been unable to move off.
    pub(super) stalled_for: f32,
    pub(super) stalled_on: u32,
    /// Who the status line is naming, and how long it has left to keep
    /// naming them.
    ///
    /// A latch rather than a reading of `stalled_for`, because that number
    /// snaps back to zero the moment one frame gets through, and a limping
    /// round gets a frame through all the time. Reading it directly put the
    /// line on and off once a second.
    pub(super) waiting_on: Option<u8>,
    pub(super) waiting_hold: f32,
}

impl StallWatch {
    /// Nothing is stuck and nobody is being waited for, as of `frame`: a
    /// new round, or a round holding still on purpose.
    pub(super) fn reset(&mut self, frame: u32) {
        let Self {
            stalled_for,
            stalled_on,
            waiting_on,
            waiting_hold,
        } = self;
        *stalled_for = 0.0;
        *stalled_on = frame;
        *waiting_hold = 0.0;
        *waiting_on = None;
    }
}

/// How long a round waits on a silent player before handing their seat to
/// an AI. Long enough that a burst of loss cannot be mistaken for somebody
/// leaving, since the resend tail repeats every tick, and short enough that a
/// table is not left staring at a still beach.
pub(super) const ABANDON_AFTER: f32 = 5.0;

/// How long a spectator may say nothing before the round forgets it.
///
/// Longer than [`ABANDON_AFTER`] on purpose. A silent seat holds the whole
/// table up, so five seconds is already generous; a silent spectator holds
/// up nothing at all, and the only thing gained by dropping it quickly is
/// the bandwidth of feeding a ghost. Ten seconds rides out a hiccup that
/// would be rude to eject somebody for when they are only watching.
///
/// Dropping them at all is the point: the round gives up only on *seats*,
/// through `awaiting`, and a spectator is awaited by nobody, so before
/// this one that walked away was fed the round for the rest of it and
/// counted in the audience the table was shown.
const WATCHER_GONE_AFTER: f32 = 10.0;

/// How long a joiner waits on a host that has stopped speaking before it
/// calls the round off.
///
/// Nothing can save a round whose host has gone: it alone relays every
/// input, decides who has left, and calls the next one. So this buys
/// nothing but a fair chance for a machine that stumbles (a big alt-tab, a
/// sleeping laptop lid, a hotel wifi hiccup). Four times the patience the
/// host shows an ordinary player.
const HOST_GONE_AFTER: f32 = 20.0;

/// How long a joiner hears nothing from its host, mid-round, before it
/// greets it again (`greet_a_quiet_host`). A host sends on every tick, so
/// a second of nothing is either a burst of loss or a host that has
/// forgotten this peer, and a greeting costs nothing in the first case.
const QUIET_HOST: f32 = 1.0;

/// How long a round held up on a seat whose player said goodbye waits
/// before handing it over: not the five seconds of [`ABANDON_AFTER`],
/// which are for telling a departure from a stumble, and the goodbye has
/// already told. Just long enough for an input that crossed it to land.
const GOODBYE_GRACE: f32 = 0.3;

/// Whether `peer` is gone by the rule every departure goes by: a goodbye
/// that has stood for [`GOODBYE_GRACE`] with no word since, or else a
/// silence of `patience`.
///
/// One rule for a seat and for the host. A goodbye is unauthenticated (any
/// machine on the LAN can send one with the peer's address on it), so it
/// earns a short grace whose clock any word from the peer stops
/// (`Peer::goodbye`), not belief on arrival. The joiner read the host's
/// goodbye at once, through the infinite silence it sets: one forged
/// datagram walked a whole table out of a round its host was still
/// running.
fn has_gone(peer: &Peer, patience: f32) -> bool {
    match peer.goodbye {
        Some(since) => since >= GOODBYE_GRACE,
        None => peer.silence >= patience,
    }
}

/// How long the picture has to have been still before the status line
/// names whoever it is waiting for. Under a second: long enough that the
/// ordinary rhythm of a three-frame input delay never trips it, short
/// enough that nobody has time to ask whether the game has crashed.
const SAY_WAITING_AFTER: f32 = 0.6;

/// How long that line stays up once it has been said, counted only while
/// the round is moving again, so a wait that keeps coming back holds one
/// steady line rather than blinking once per stumble.
///
/// It has to outlast the gap between two stalls in a limping round, which
/// is at most [`SAY_WAITING_AFTER`] of fresh waiting plus whatever play
/// sits between them. Longer than this and a round that has genuinely
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

    /// Joiner, mid-round: greet a host that has gone quiet, about once a
    /// second until it is heard again.
    ///
    /// The host admits nobody it does not know except on a greeting
    /// (`NetMsg::greets`), and a round sends none: inputs, hashes and
    /// pauses only. So a peer the host has forgotten had no way back. A
    /// seat whose Wi-Fi dropped past [`ABANDON_AFTER`] missed its one
    /// abandonment notice and froze until it was told, wrongly, that the
    /// host had left; a watcher forgotten as silent did the same; and the
    /// direct `PINCH_JOIN` joiner whose one hello was lost was never let
    /// in at all. Greeted again, the host answers: a dropped seat is told
    /// of its abandonment with the rest of the table (`dropped`), a
    /// watcher is handed the round as it stands, and a peer still at the
    /// table is told what it already knows.
    ///
    /// Only once the host has been quiet for [`QUIET_HOST`]: a host in a
    /// running round speaks every tick, so a joiner hearing it is let in.
    fn greet_a_quiet_host(&mut self, delta: f32) {
        if self.is_host() {
            return;
        }
        let quiet = self
            .peers
            .get(0)
            .is_none_or(|host| host.silence >= QUIET_HOST);
        if !quiet {
            // Armed, so the first greeting goes the moment the host has
            // been quiet long enough rather than up to a second later.
            self.home.greet_in = 0.0;
            return;
        }
        if crate::app::lobby::once_a_second(&mut self.home.greet_in, delta) {
            self.transport.send(self.own_greeting());
        }
    }

    /// The peer holding `seat`, and with it how long that peer has said
    /// nothing at all. `None` for a seat no peer holds: the host's own,
    /// and the AI's.
    fn seat_holder(&self, seat: u8) -> Option<&Peer> {
        let peer = self.peers.holder_of(seat)?;
        self.peers.get(peer)
    }

    /// Whether the host has gone: a joiner's own verdict on the one peer it
    /// has, and the only thing it may decide by itself.
    ///
    /// Deciding *this* alone is safe where deciding a seat is not: an
    /// abandoned seat keeps playing under an AI and every peer must agree
    /// on the frame that happened, while a joiner leaving takes nothing
    /// with it but itself.
    ///
    /// A pause makes no difference: the pump runs through one (that is
    /// what carries the resume), so a host that is there keeps talking
    /// however still the picture is.
    ///
    /// A host that said goodbye is gone a moment later, by the rule a
    /// seat's goodbye goes by ([`has_gone`]).
    pub fn host_gone(&self) -> bool {
        !self.is_host()
            && self
                .peers
                .get(0)
                .is_some_and(|host| has_gone(host, HOST_GONE_AFTER))
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
    /// also what a burst of loss looks like, and what a peer a second
    /// behind looks like. Silence is the strong evidence: every peer sends
    /// on every tick it runs, resending every commit a peer could still be
    /// missing, so a machine still in the room is still talking.
    ///
    /// Returns what was given up on this call, which the tests read. The
    /// lasting record is `abandoned`, because a joiner is *told* rather
    /// than deciding and its seats never pass through here.
    pub fn abandon_stalled(&mut self, delta: f32, paused: bool) -> Vec<u8> {
        self.age_the_silence(delta);
        self.greet_a_quiet_host(delta);
        // Paused or not: nobody who holds no chair holds the round up, and
        // a pause is no reason to go on counting somebody who has left.
        if self.is_host() {
            self.forget_gone_bystanders();
        }
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
            // The line is let down gently: one frame getting through is
            // not the round recovering, since a limping round gets one
            // through every other tick. The hold only runs down while the
            // picture is actually moving.
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
        if !self.is_host() {
            return Vec::new();
        }
        let served = self.stall.stalled_for >= ABANDON_AFTER;
        let gone: Vec<u8> = waiting
            .into_iter()
            .filter(|seat| match self.seat_holder(*seat) {
                // Said it was going, and has not been heard since: the
                // round being held up on it is all there is left to wait
                // for, after a moment for any input that crossed the
                // goodbye on the way.
                //
                // That moment is counted from the goodbye, not from the
                // stall, and anything heard from the peer in it takes the
                // goodbye back (`Peer::goodbye`). A goodbye is
                // unauthenticated, so one forged with a player's address
                // would otherwise hand a lagging castle to the AI at once.
                //
                // Otherwise held up by them *and* not a word from them.
                Some(peer) => (peer.left() || served) && has_gone(peer, ABANDON_AFTER),
                // A seat this session cannot point at on the socket: the
                // direct `PINCH_HOST` pair keeps no launch plan, having
                // never been through a lobby. Nothing better to go on than
                // the held-up frame, as it was before.
                None => served,
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
            // To everyone, the dropped peer included, and before it is
            // forgotten: its own abandonment is how it learns it was
            // dropped (`dropped`) rather than waiting out the host.
            self.transport
                .send(NetMsg::Abandoned { seat: *seat, frame });
            if !self.abandoned.iter().any(|(held, _)| held == seat) {
                self.abandoned.push((*seat, frame));
            }
            self.forget_seat(*seat);
        }
        gone
    }

    /// Host: lift the pause `peer` called, now that it has said goodbye.
    ///
    /// A seat whose player said goodbye goes to the AI the moment the round
    /// waits on it, but a paused round waits on nobody (`abandon_stalled`),
    /// so a player who paused and left had the table frozen on its pause
    /// card until somebody else pressed Continue. Lifted as that player's
    /// own Continue would have, which any player's lifts for the table.
    /// Only the pause it called: one called since by somebody else, or a
    /// pause of its that was lifted already, is not its to lift.
    pub(super) fn release_a_departed_pause(&mut self, peer: usize) {
        // The pause in flight, and no other: a pause of its that was
        // lifted is not, and neither is one it called that an earlier
        // proposal from somebody else then overtook.
        let holds = self.is_host()
            && self.session.pause_frame().is_some()
            && self.peers.get(peer).and_then(|row| row.pause) == self.session.pause_frame();
        if holds {
            self.request_resume();
        }
    }

    /// Drop the peer holding `seat` from the socket as well as from the
    /// round.
    ///
    /// Abandoning only unsticks the play; without this the departed peer is
    /// still counted, still holds its place in the launch plan, and is
    /// still dealt a seat in the round after, which stalls on the same
    /// ghost all over again.
    fn forget_seat(&mut self, seat: u8) {
        debug_assert!(usize::from(seat) < MAX_PLAYERS, "no such seat: {seat}");
        let Some(peer) = self.peers.holder_of(seat) else {
            return;
        };
        self.forget_peer(peer);
        debug_assert!(
            self.peers.len() <= self.transport.peer_count(),
            "a peer's row outlived the peer it was kept for"
        );
    }

    /// Drop a peer from the socket and from everything kept by its index.
    ///
    /// The row goes with the socket's entry, so nothing kept about the
    /// departed is read against whoever moved up into its index, and the
    /// same goes for a tide vote it had standing and the hashes it sent.
    pub(super) fn forget_peer(&mut self, peer: usize) {
        self.transport.forget(peer);
        self.peers.forget(peer);
        self.stands.tide.votes.voter_left(peer);
        self.hashes.forget(peer);
        self.catch_up.forget(peer);
    }

    /// Host, during the round: drop every spectator, and everyone in line
    /// for the next round, that has gone quiet or said goodbye.
    ///
    /// Not a seat: that is the round's own business, and is given up on
    /// through `awaiting` when it holds a frame up.
    ///
    /// A peer in line is sent no frames, but it is not free. Left until
    /// the results card's sweep ([`Self::forget_the_silent`]), one that
    /// walked away counted as a chair taken all round: on the beacon, in
    /// every newcomer's place in the queue, and against the room the host
    /// has for one more, so a table with a chair free turned people away.
    /// It greets once a second from the lobby while it waits, so its
    /// silence means what a watcher's does. Only at a table formed in the
    /// lobby: the direct `PINCH_HOST` pair keeps no plan, so its one
    /// player is in line by the book, and is the round's own business.
    pub(super) fn forget_gone_bystanders(&mut self) {
        let lobby = self.home.from_lobby;
        let gone = self.peers.to_forget(|row| {
            let bystander = match row.place {
                Place::Watching | Place::LateWatching => true,
                Place::Queued => lobby,
                Place::Seated(_) => false,
            };
            bystander && row.silence >= WATCHER_GONE_AFTER
        });
        for peer in gone {
            self.forget_peer(peer);
        }
    }

    /// Host, between rounds: drop every peer that has said nothing for
    /// [`ABANDON_AFTER`], the same patience the round itself has.
    ///
    /// The results card is the one place a peer leaves without the round
    /// noticing: Escape, the menu, a crash, and nothing stalls, because
    /// nothing is being simulated. Left on the socket, it is dealt a chair
    /// in the next round and freezes every table on the ghost. A joiner on
    /// the card greets once a second so that silence means something.
    pub(super) fn forget_the_silent(&mut self) {
        debug_assert!(self.is_host(), "only the host keeps the table");
        for peer in self.peers.to_forget(|row| row.silence >= ABANDON_AFTER) {
            self.forget_peer(peer);
        }
    }

    /// Whether the host gave up on this seat: its own abandonment came
    /// over the wire. The host stops talking to a seat it has dropped, so
    /// left to [`Self::host_gone`] the joiner would sit on a frozen beach
    /// for twenty seconds and then be told the host had left, which is not
    /// what happened.
    pub fn dropped(&self) -> bool {
        !self.is_host()
            && self
                .session
                .seat()
                .is_some_and(|mine| self.abandoned.iter().any(|(seat, _)| *seat == mine))
    }

    /// The seat the status line should name, if any.
    ///
    /// A lockstep frame runs only when every seat's input is in, so a
    /// still picture is the ordinary shape of somebody else's trouble.
    /// This lets the screen say whose.
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
    mut controllers: ResMut<Controllers>,
    mut log: ResMut<EventLog>,
) {
    let Some(session) = &mut online.0 else {
        return;
    };
    session.abandon_stalled(time.delta_secs(), paused.0);
    let level = session.terms.bot_level();
    let tr = settings.tr();
    // Both roads end in the same list (the host decides, a joiner is told),
    // so reading it is how this stays one piece of code rather than two.
    // A seat that already has a bot in it is a seat already announced.
    for (seat, _) in &session.abandoned {
        let Some(slot) = controllers.0.get_mut(usize::from(*seat)) else {
            continue;
        };
        if slot.ai().is_some() {
            continue;
        }
        *slot = SeatController::Ai(level);
        log.push(
            fill(tr.online_seat_abandoned, &[("p", &names.label(tr, *seat))]),
            palette::player_color(*seat),
        );
    }
}

/// How long a caught-up watcher may sit on the frame it was caught up at
/// before it asks again. The inputs from that frame on come from the
/// players' resend tails, which reach about a second back; a watcher that
/// took longer than that to walk into the arena never gets them.
const CATCH_UP_PATIENCE: f32 = 3.0;

/// A late watcher whose round never started moving goes back to the lobby
/// and catches up again.
///
/// Rare: the tails cover the second it takes to walk from the lobby into
/// the arena. But a slow machine can take longer, and then the frame the
/// snapshot was taken at has scrolled out of every tail, and the beach
/// would stand still until the host gave up on a watcher that never
/// hashed. The lobby greets the host the moment it opens, and a watcher's
/// greeting mid-round is answered with a fresh snapshot, so the way back
/// is the way in, a second later.
pub(crate) fn catch_up_again(
    time: Res<Time>,
    mut online: ResMut<Online>,
    mut homecoming: ResMut<crate::app::lobby::Homecoming>,
    mut next_screen: ResMut<NextState<Screen>>,
    mut stuck: Local<f32>,
) {
    let waiting = online.0.as_ref().is_some_and(|session| {
        session.catch_up.from_frame == Some(session.session.frame()) && !session.session.paused()
    });
    if !waiting {
        *stuck = 0.0;
        return;
    }
    *stuck += time.delta_secs();
    if *stuck < CATCH_UP_PATIENCE {
        return;
    }
    *stuck = 0.0;
    if let Some(session) = online.0.take() {
        homecoming.0 = Some(session.back_to_the_lobby());
        next_screen.set(Screen::Lobby);
    }
}

/// Walk a joiner out of a round whose host has gone, and say why.
///
/// Every other kind of departure leaves a round that can go on: a rival's
/// castle is handed to an AI and the beach plays out. A host's cannot. It
/// is the hub of the star: it relays every input, decides who has left,
/// and calls the next round. Without this the table waits on a still beach
/// forever, with nothing on screen to suggest Escape is the way out.
///
/// Out to *where* is the lobby, when that is where the beach was found.
/// The round is over; the hall it was found in is not, and on a busy
/// evening there are other beaches on the air in it. Dropping the player
/// at the main menu made them walk back in through the lobby door to
/// reach the game next door. A direct `PINCH_HOST` pair has no hall to be
/// put back in, and goes to the menu as before.
pub(crate) fn leave_a_hostless_round(
    settings: Res<GameSettings>,
    online: Res<Online>,
    mut notice: ResMut<RoundNotice>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    let Some(session) = online.0.as_ref() else {
        return;
    };
    let tr = settings.tr();
    let why = if session.kicked {
        tr.lobby_kicked_you
    } else if session.dropped() {
        tr.online_dropped
    } else if session.host_gone() {
        tr.online_host_gone
    } else {
        return;
    };
    // Read by whichever screen takes them, and taken there rather than
    // left to linger. The session itself needs no dropping here: nothing
    // armed it for another round, so `end_versus` lets it go on the way
    // out of the arena.
    notice.0 = why.to_string();
    next_screen.set(match session.home.from_lobby {
        true => Screen::Lobby,
        false => Screen::Menu,
    });
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
    /// the pause card leaves the ticker running so the pump keeps carrying
    /// the resume. Read from that flag alone, pausing for a moment cost
    /// every rival their castle five seconds later.
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

    /// A player who said goodbye mid-round is not waited on for the five
    /// seconds that tell a departure from a stumble: the goodbye already
    /// told. Its castle goes to the AI as soon as the round is held up on
    /// it (after a moment for an input that crossed the goodbye), and the
    /// peer is forgotten with it.
    #[test]
    fn a_player_who_said_goodbye_is_not_waited_for() {
        let transport = UdpTransport::host(0).expect("game socket");
        let mut session = OnlineSession::new(
            transport,
            Lockstep::new(0, vec![0, 1], 0),
            2,
            MatchTerms::default(),
        );
        session.peers.deal(&[Some(1)]);
        session.peers.said_goodbye(0);
        assert!(session.peers.get(0).is_some_and(Peer::left));
        // The first look at the held-up frame starts the clock.
        assert!(session.abandon_stalled(0.1, false).is_empty());
        assert!(
            session.abandon_stalled(0.1, false).is_empty(),
            "a moment for an input that crossed the goodbye"
        );
        assert_eq!(
            session.abandon_stalled(GOODBYE_GRACE, false),
            vec![1],
            "and not the five seconds a silent seat gets"
        );
        assert_eq!(session.peers.len(), 0, "forgotten with its seat");
    }

    /// A peer that is still talking keeps its castle, however far behind
    /// its inputs are.
    ///
    /// A held-up frame is weak evidence: it is equally what a burst of loss
    /// looks like, and what a laptop half a second behind looks like. The
    /// strong evidence is silence on the socket, since every peer sends on
    /// every tick it runs.
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

    /// A goodbye is a claim nothing checks: anyone on the LAN can send one
    /// with a player's address on it. The real player goes on talking, and
    /// the first word from it takes the goodbye back, so the castle stays.
    /// And the grace is counted from the goodbye, not from a stall that
    /// was already long when it landed, or a forged goodbye on a lagging
    /// seat handed the castle over on the very next look.
    #[test]
    fn a_goodbye_taken_back_by_talking_keeps_the_castle() {
        let transport = UdpTransport::host(0).expect("game socket");
        let mut session = OnlineSession::new(
            transport,
            Lockstep::new(0, vec![0, 1], 0),
            2,
            MatchTerms::default(),
        );
        session.peers.deal(&[Some(1)]);
        // Held up on seat 1 for a good second, its machine talking all the
        // while.
        for _ in 0..12 {
            session.mark_heard(0);
            assert!(session.abandon_stalled(0.1, false).is_empty());
        }
        session.peers.said_goodbye(0);
        assert!(
            session.abandon_stalled(0.1, false).is_empty(),
            "a moment from the goodbye, whatever the stall"
        );
        // And then the player speaks again, every tick, as one still at
        // the table does.
        session.mark_heard(0);
        assert!(!session.peers.get(0).is_some_and(Peer::left));
        for _ in 0..30 {
            session.mark_heard(0);
            assert!(
                session.abandon_stalled(0.1, false).is_empty(),
                "a forged goodbye cost a castle"
            );
        }
        assert!(session.abandoned.is_empty());
    }

    /// A player who paused and then left: the goodbye lifts the pause it
    /// called, as its own Continue would have, or the table sat frozen on
    /// its pause card (a paused round waits on nobody, so the castle never
    /// went either). A pause somebody else called is not the leaver's to
    /// lift.
    #[test]
    fn a_pause_goes_with_the_player_who_called_it() {
        for leaver_paused in [true, false] {
            let mut host = OnlineSession::new(
                UdpTransport::host(0).expect("host socket"),
                Lockstep::new(0, vec![0, 1], crate::sim::DEFAULT_DELAY),
                2,
                MatchTerms::default(),
            );
            let port = host.transport.local_addr().expect("addr").port();
            let bo = UdpTransport::join(("127.0.0.1", port)).expect("join");
            bo.send(NetMsg::hello("Bo"));
            std::thread::sleep(std::time::Duration::from_millis(20));
            host.pump(PlayerAction::None, |_| {});
            assert_eq!(host.transport.peer_count(), 1, "Bo is aboard");
            host.peers.deal(&[Some(1)]);
            if leaver_paused {
                let mut bos = Lockstep::new(1, vec![0, 1], crate::sim::DEFAULT_DELAY);
                let frame = bos.request_pause().expect("a player may pause");
                bo.send(NetMsg::Pause { frame });
                std::thread::sleep(std::time::Duration::from_millis(20));
                host.pump(PlayerAction::None, |_| {});
            } else {
                host.request_pause();
                // Bo heard it, and says so on every tick it holds there, as
                // every peer does. Taken as Bo's own pause, the goodbye
                // below lifted the host's pause for the table.
                let frame = host.session.pause_frame().expect("paused");
                bo.send(NetMsg::Pause { frame });
                std::thread::sleep(std::time::Duration::from_millis(20));
                host.pump(PlayerAction::None, |_| {});
            }
            assert!(host.session.paused());
            drop(bo);
            std::thread::sleep(std::time::Duration::from_millis(20));
            host.pump(PlayerAction::None, |_| {});
            assert!(host.peers.get(0).is_some_and(Peer::left), "said goodbye");
            assert_eq!(
                host.session.paused(),
                !leaver_paused,
                "the leaver paused: {leaver_paused}"
            );
        }
    }

    /// A watcher repeats the table's pause like everybody else, and calls
    /// none of its own: a watcher walking out mid-pause leaves the pause
    /// where it was, and only the player who called it lifts it by going.
    #[test]
    fn a_watcher_leaving_does_not_lift_the_pause() {
        let mut host = OnlineSession::new(
            UdpTransport::host(0).expect("host socket"),
            Lockstep::new(0, vec![0, 1], crate::sim::DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        let port = host.transport.local_addr().expect("addr").port();
        let bo = UdpTransport::join(("127.0.0.1", port)).expect("join");
        let dee = UdpTransport::join(("127.0.0.1", port)).expect("join");
        for (socket, msg) in [(&bo, NetMsg::hello("Bo")), (&dee, NetMsg::watch("Dee"))] {
            socket.send(msg);
            std::thread::sleep(std::time::Duration::from_millis(20));
            host.pump(PlayerAction::None, |_| {});
        }
        assert_eq!(host.transport.peer_count(), 2, "Bo and Dee are aboard");
        host.peers.deal(&[Some(1), None]);

        // Bo pauses, and Dee, holding on it too, echoes it back.
        let mut bos = Lockstep::new(1, vec![0, 1], crate::sim::DEFAULT_DELAY);
        let frame = bos.request_pause().expect("a player may pause");
        bo.send(NetMsg::Pause { frame });
        std::thread::sleep(std::time::Duration::from_millis(20));
        host.pump(PlayerAction::None, |_| {});
        dee.send(NetMsg::Pause { frame });
        std::thread::sleep(std::time::Duration::from_millis(20));
        host.pump(PlayerAction::None, |_| {});
        assert!(host.session.paused());

        drop(dee);
        std::thread::sleep(std::time::Duration::from_millis(20));
        host.pump(PlayerAction::None, |_| {});
        assert!(
            host.peers.get(1).is_some_and(Peer::left),
            "Dee said goodbye"
        );
        assert!(host.session.paused(), "Dee's goodbye lifted Bo's pause");

        drop(bo);
        std::thread::sleep(std::time::Duration::from_millis(20));
        host.pump(PlayerAction::None, |_| {});
        assert!(!host.session.paused(), "and Bo's own goodbye lifts it");
    }

    /// Somebody who turned up mid-round to play the next one, and then
    /// walked off before it, is forgotten when it says goodbye rather
    /// than on the results card. Until then it was a chair taken: on the
    /// beacon, in every newcomer's place in the queue, and against the
    /// room the host had for anyone else.
    #[test]
    fn a_player_in_line_who_leaves_mid_round_frees_the_place() {
        let mut host = hosting(vec![0, 1]);
        host.home.from_lobby = true;
        let port = host.transport.local_addr().expect("addr").port();
        let bo = UdpTransport::join(("127.0.0.1", port)).expect("join");
        bo.send(NetMsg::hello("Bo"));
        std::thread::sleep(std::time::Duration::from_millis(20));
        host.pump(PlayerAction::None, |_| {});
        host.peers.deal(&[Some(1)]);

        // Cy greets with the round under way, and is put in line.
        let cy = UdpTransport::join(("127.0.0.1", port)).expect("join");
        cy.send(NetMsg::hello("Cy"));
        std::thread::sleep(std::time::Duration::from_millis(20));
        host.pump(PlayerAction::None, |_| {});
        assert_eq!(host.transport.peer_count(), 2, "Cy is aboard");
        assert_eq!(host.players_spoken_for(), 3, "Cy is owed a chair");

        drop(cy);
        std::thread::sleep(std::time::Duration::from_millis(20));
        bo.send(NetMsg::Hash { frame: 0, hash: 0 });
        host.pump(PlayerAction::None, |_| {});
        host.abandon_stalled(0.0, false);
        assert_eq!(host.transport.peer_count(), 1, "Cy is forgotten");
        assert_eq!(host.players_spoken_for(), 2, "and no chair is held for it");
        assert_eq!(host.peers.seat_of(0), Some(1), "Bo keeps the seat");
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

    /// The host's goodbye goes by the rule a seat's does: believed after a
    /// moment's grace, and taken back by any word from the host in it. A
    /// goodbye is a claim nothing checks, and read at once, one forged
    /// with the host's address walked the whole table out of a round its
    /// host was still running.
    #[test]
    fn a_hosts_goodbye_is_believed_after_the_grace() {
        let mut joiner = OnlineSession::new(
            UdpTransport::host(0).expect("game socket"),
            Lockstep::new(1, vec![0, 1], 0),
            2,
            MatchTerms::default(),
        );
        joiner.mark_heard(0);
        // Forged: the host is still talking.
        joiner.peers.said_goodbye(0);
        assert!(!joiner.host_gone(), "believed on arrival");
        joiner.abandon_stalled(GOODBYE_GRACE / 2.0, false);
        joiner.mark_heard(0);
        joiner.abandon_stalled(GOODBYE_GRACE * 2.0, false);
        assert!(!joiner.host_gone(), "a host still talking is not gone");
        // Real: nothing more from the host.
        joiner.peers.said_goodbye(0);
        joiner.abandon_stalled(GOODBYE_GRACE / 2.0, false);
        assert!(!joiner.host_gone(), "still within the grace");
        joiner.abandon_stalled(GOODBYE_GRACE, false);
        assert!(joiner.host_gone(), "and then the host is gone");
    }

    /// A round that falls apart puts the player back in the hall it was
    /// found in, not out at the front door.
    ///
    /// The hall is where the other beaches are, and on a busy evening
    /// there are some: dropping the player at the main menu made them walk
    /// back in through the lobby door to reach the game next door. A pair
    /// wired up by hand has no hall to be put back in, so that one still
    /// goes to the menu.
    #[test]
    fn a_round_without_a_host_hands_the_player_back_to_the_lobby() {
        let walked_out = |from_lobby| {
            let mut app = App::new();
            app.add_plugins(bevy::state::app::StatesPlugin);
            app.init_state::<Screen>();
            app.insert_resource(State::new(Screen::Versus));
            app.insert_resource(GameSettings::default());
            app.init_resource::<RoundNotice>();

            let mut joiner = OnlineSession::new(
                UdpTransport::host(0).expect("game socket"),
                Lockstep::new(1, vec![0, 1], 0),
                2,
                MatchTerms::default(),
            );
            joiner.home.from_lobby = from_lobby;
            joiner.mark_heard(0);
            joiner.abandon_stalled(HOST_GONE_AFTER + 1.0, false);
            assert!(joiner.host_gone(), "the host really is gone");
            app.insert_resource(Online(Some(joiner)));

            app.add_systems(Update, leave_a_hostless_round);
            app.update();
            app.update(); // the state change lands
            let screen = *app.world().resource::<State<Screen>>().get();
            (screen, app.world().resource::<RoundNotice>().0.clone())
        };

        let (screen, why) = walked_out(true);
        assert_eq!(screen, Screen::Lobby, "back to the hall it came from");
        assert_eq!(why, crate::app::i18n::EN.online_host_gone, "and told why");

        let (screen, why) = walked_out(false);
        assert_eq!(screen, Screen::Menu, "a hand-wired pair has no hall");
        assert_eq!(why, crate::app::i18n::EN.online_host_gone);
    }

    /// A joiner the host asks to leave while the results card is up, or
    /// mid-round, hears it and walks out with the reason. Ignored, it
    /// greeted on and was seated again at the next round.
    #[test]
    fn a_joiner_asked_to_leave_on_the_card_walks_out() {
        let mut host = UdpTransport::host(0).expect("host socket");
        let port = host.local_addr().expect("addr").port();
        let mut joiner = OnlineSession::new(
            UdpTransport::join(("127.0.0.1", port)).expect("join"),
            Lockstep::new(1, vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        joiner.home.from_lobby = true;
        // The joiner greets from the card; the host turns it away.
        let mut greeted = false;
        for _ in 0..200 {
            joiner.poll_between_rounds(1.0);
            std::thread::sleep(std::time::Duration::from_millis(5));
            if !host.recv_all().is_empty() {
                greeted = true;
                break;
            }
        }
        assert!(greeted, "the joiner greeted");
        host.turn_away(0);
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            joiner.poll_between_rounds(0.0);
            if joiner.kicked {
                break;
            }
        }
        assert!(joiner.kicked, "the joiner heard it");

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::Versus));
        app.insert_resource(GameSettings::default());
        app.init_resource::<RoundNotice>();
        app.insert_resource(Online(Some(joiner)));
        app.add_systems(Update, leave_a_hostless_round);
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Lobby
        );
        assert_eq!(
            app.world().resource::<RoundNotice>().0,
            crate::app::i18n::EN.lobby_kicked_you,
            "and was told why"
        );
    }

    /// A table reading the scores together is not a table whose host has
    /// gone, however quiet it is.
    ///
    /// A settled session falls silent between rounds on purpose (see
    /// `greet_in`), so silence read as evidence there walks every joiner
    /// out of a perfectly good table. The greeting is what makes it mean
    /// something, and this runs over real sockets so a reply really comes
    /// back.
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
    /// A lockstep sim advances until it *cannot*, on the fixed step, and
    /// the tick that watches for stalls runs afterwards, in `Update`. So
    /// "are the next frame's inputs in?" is asked at the one moment where
    /// the answer is always no, and every healthy round read as
    /// permanently stalled. The picture moving is the only thing either
    /// reader wants to know.
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
    /// Read straight off the stall clock it strobes: that clock snaps back
    /// to zero the instant one frame gets through, and a limping round does
    /// that over and over.
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
        /// the sim advances on the fixed step, and only then does the tick
        /// that watches for stalls get a look, always finding the *next*
        /// frame's slots empty.
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
        // Every stretch blinked the line out and every stall brought it
        // back, which is the strobe.
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

    /// Abandoning has to empty the chair for good. A peer that is still
    /// counted holds its place in the launch plan, is dealt a seat in the
    /// round after, and stalls that one too.
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
        assert!(
            host.abandoned.is_empty(),
            "a new round has given up on nobody yet"
        );
    }

    /// A joiner told, last round, that some seat was given up on must not
    /// carry it into the next. Re-seated at that number, it would read
    /// itself dropped and walk out, and its seat would play the AI from
    /// the old frame on.
    #[test]
    fn last_rounds_dropped_seat_is_not_this_rounds() {
        let mut joiner = OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            Lockstep::new(2, vec![0, 1, 2], DEFAULT_DELAY),
            3,
            MatchTerms::default(),
        );
        // Seat one was given up on at frame 40 of the round just played.
        joiner.abandoned.push((1, 40));
        // Next round, one fewer at the table: this joiner moves up to one.
        joiner.take_up(Invitation {
            seats: 2,
            seat: Some(1),
            terms: MatchTerms {
                seed: 9,
                ..MatchTerms::default()
            },
            names: Default::default(),
            kinds: [0; MAX_PLAYERS],
            standing: None,
            beach: Vec::new(),
        });
        assert!(joiner.abandoned.is_empty());
        assert!(!joiner.dropped(), "its new seat was never given up on");
    }

    /// A host never gives up on itself: its own slot is empty whenever it
    /// has not committed this frame yet, which is an ordinary moment.
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
    /// and the feed says so. Built as the app builds it, since it is the
    /// wiring between the parts that breaks.
    #[test]
    fn a_departed_player_is_replaced_by_an_ai_and_the_feed_says_so() {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default());
        app.insert_resource(Paused(false));
        app.insert_resource(GameSettings::default());
        app.init_resource::<SeatNames>();
        app.init_resource::<Controllers>();
        app.init_resource::<EventLog>();

        let mut session = OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            Lockstep::new(0, vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        // Run the round into the stall the missing player causes.
        for _ in 0..40 {
            session.pump(
                PlayerAction::None,
                |net| {
                    while net.session.advance().is_some() {}
                },
            );
        }
        app.insert_resource(Online(Some(session)));
        app.add_systems(Update, abandon_the_departed);

        // Not yet: a burst of loss is not somebody leaving.
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(500));
        app.update();
        assert_eq!(
            app.world().resource::<Controllers>().ai(1),
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
            app.world().resource::<Controllers>().ai(1).is_some(),
            "the empty castle has nobody in it"
        );
        let log = app.world().resource::<EventLog>();
        assert_eq!(log.0.len(), 1, "the players are told, once");

        // The round really does move again, with the AI supplying the seat.
        let mut board = classic_arena(false, 2);
        let controllers = *app.world().resource::<Controllers>();
        let controllers = &controllers;
        let mut online = app.world_mut().resource_mut::<Online>();
        let session = online.0.as_mut().expect("a session");
        let mut moved = 0;
        for _ in 0..40 {
            session.pump(PlayerAction::None, |net| {
                while let Some(mut actions) = net.session.advance() {
                    fill_bot_actions(&board, controllers, &mut actions);
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

    /// Let the datagrams in flight land.
    fn settle() {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    /// A joiner greets a host that has gone quiet mid-round, about once a
    /// second, and says nothing while the host is talking. The direct
    /// `PINCH_JOIN` joiner leans on this: its one hello at boot, lost or
    /// sent before the host was up, used to be the only one.
    #[test]
    fn a_joiner_greets_a_quiet_host_until_it_hears_it() {
        let mut host = UdpTransport::host(0).expect("host socket");
        let port = host.local_addr().expect("addr").port();
        let mut joiner = OnlineSession::new(
            UdpTransport::join(("127.0.0.1", port)).expect("join"),
            Lockstep::new(1, vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        joiner.names[1] = "Bo".into();
        let greetings = |host: &mut UdpTransport| {
            settle();
            host.recv_all()
                .into_iter()
                .filter(|(msg, _)| msg.greets())
                .count()
        };

        joiner.abandon_stalled(0.5, false);
        assert_eq!(greetings(&mut host), 0, "half a second is nothing yet");
        joiner.abandon_stalled(0.6, false);
        assert_eq!(greetings(&mut host), 1, "a quiet host is greeted");
        joiner.abandon_stalled(0.5, false);
        assert_eq!(greetings(&mut host), 0, "about once a second");
        joiner.abandon_stalled(0.6, false);
        assert_eq!(greetings(&mut host), 1, "and again");

        // The host speaks, as it does on every tick of a round.
        host.send(NetMsg::Hash { frame: 0, hash: 0 });
        settle();
        joiner.pump(PlayerAction::None, |_| {});
        for _ in 0..4 {
            joiner.abandon_stalled(0.2, false);
            host.send(NetMsg::Hash { frame: 0, hash: 0 });
            settle();
            joiner.pump(PlayerAction::None, |_| {});
        }
        assert_eq!(greetings(&mut host), 0, "a host heard is not greeted");
    }

    /// A seat the host gave up on while its network was down comes back
    /// to a host that has forgotten it, and is told it was dropped.
    ///
    /// The host admits a stranger only on a greeting, and a round sends
    /// none, so the seat's inputs were ignored: it froze, and twenty
    /// seconds later was told the host had left, which is not what
    /// happened. A greeting while still at the table is answered with
    /// what the seat already knows, and changes nothing.
    #[test]
    fn a_dropped_seat_that_comes_back_hears_it_was_dropped() {
        let mut host = hosting(vec![0, 1]);
        host.home.from_lobby = true;
        let port = host.transport.local_addr().expect("addr").port();
        let mut joiner = OnlineSession::new(
            UdpTransport::join(("127.0.0.1", port)).expect("join"),
            Lockstep::new(1, vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        joiner.home.from_lobby = true;
        joiner.names[1] = "Bo".into();
        joiner.transport.send(joiner.own_greeting());
        settle();
        host.pump(PlayerAction::None, |_| {});
        host.peers.deal(&[Some(1)]);

        // Still at the table, and greeting anyway: harmless.
        joiner.abandon_stalled(QUIET_HOST + 0.1, false);
        settle();
        host.pump(PlayerAction::None, |_| {});
        settle();
        joiner.pump(PlayerAction::None, |_| {});
        assert_eq!(host.transport.peer_count(), 1, "nobody new");
        assert_eq!(host.peers.seat_of(0), Some(1), "Bo keeps the seat");
        assert!(!joiner.next_round, "the round's own Start is no news");
        assert_eq!(joiner.session.seat(), Some(1));
        assert!(!joiner.dropped());

        // Its network goes; the host gives the seat to the AI, and the one
        // notice it sends while the seat is unreachable is lost.
        let frame = host.session.frame();
        host.session.abandon(1, frame);
        host.abandoned.push((1, frame));
        host.forget_seat(1);
        assert_eq!(host.transport.peer_count(), 0, "forgotten");
        settle();
        let _ = joiner.transport.recv_all();

        // Back, it hears nothing, greets, and is told on the host's next
        // tick, which repeats every abandonment to everyone it has.
        joiner.abandon_stalled(QUIET_HOST + 0.1, false);
        settle();
        host.pump(PlayerAction::None, |_| {});
        host.pump(PlayerAction::None, |_| {});
        settle();
        joiner.pump(PlayerAction::None, |_| {});
        assert!(
            joiner.dropped(),
            "told it was dropped, not that the host left"
        );
        assert!(!joiner.host_gone());
    }

    /// A watcher forgotten while its network was down greets on its way
    /// back and is handed the round as it stands, which it takes: its own
    /// board is past anything the tails can bring back. One a frame or two
    /// behind keeps its board.
    #[test]
    fn a_forgotten_watcher_is_caught_up_again() {
        use crate::sim::{InputMsg, generate_arena};
        let mut host = hosting(vec![0, 1]);
        host.home.from_lobby = true;
        let port = host.transport.local_addr().expect("addr").port();
        let bo = UdpTransport::join(("127.0.0.1", port)).expect("join");
        bo.send(NetMsg::hello("Bo"));
        let mut dee = OnlineSession::new(
            UdpTransport::join(("127.0.0.1", port)).expect("join"),
            Lockstep::observer(vec![0, 1], DEFAULT_DELAY),
            2,
            host.terms,
        );
        dee.home.from_lobby = true;
        dee.home.wants_to_watch = true;
        settle();
        dee.transport.send(dee.own_greeting());
        settle();
        host.pump(PlayerAction::None, |_| {});
        host.peers.deal(&[Some(1), None]);

        let mut board = generate_arena(3, 2, 12, 9);
        for frame in 0..80 {
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
        // Dee heard none of it, and has been forgotten as silent.
        host.forget_peer(1);
        settle();
        let _ = dee.transport.recv_all();
        assert!(!dee.lost_the_thread(dee.session.frame() + 2), "a hiccup");

        dee.abandon_stalled(QUIET_HOST + 0.1, false);
        settle();
        host.pump(PlayerAction::None, |_| {});
        host.send_catch_ups(&board);
        for _ in 0..40 {
            settle();
            dee.pump(PlayerAction::None, |_| {});
            if dee.next_round {
                break;
            }
        }
        assert!(dee.next_round, "on its way back into the round");
        assert!(dee.watching());
        assert_eq!(dee.catch_up.from_frame, Some(host.session.frame()));
        drop(bo);
    }

    /// A goodbye on the results card in the host's name is answered with a
    /// greeting at once, and a host still there answers it inside the
    /// grace. Left to the once-a-second greeting, a forged one was
    /// believed most of the time.
    #[test]
    fn a_forged_goodbye_on_the_card_is_taken_back_by_the_host() {
        let mut host = hosting(vec![0, 1]);
        let port = host.transport.local_addr().expect("addr").port();
        let mut joiner = OnlineSession::new(
            UdpTransport::join(("127.0.0.1", port)).expect("join"),
            Lockstep::new(1, vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        joiner.poll_between_rounds(1.0);
        settle();
        host.poll_between_rounds(0.0);
        settle();
        joiner.poll_between_rounds(0.0);
        assert_eq!(host.transport.peer_count(), 1);

        // Sent from the host's own address, as a forger would make it look.
        host.transport.send(NetMsg::Leaving);
        settle();
        joiner.poll_between_rounds(0.0);
        assert!(joiner.peers.get(0).is_some_and(Peer::left), "heard");
        settle();
        host.poll_between_rounds(0.0);
        settle();
        joiner.poll_between_rounds(GOODBYE_GRACE / 2.0);
        joiner.poll_between_rounds(GOODBYE_GRACE);
        assert!(!joiner.host_gone(), "the host answered");
    }
}
