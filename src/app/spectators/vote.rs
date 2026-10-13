//! The spectators' tide vote: the events they may call, the vote the
//! host counts, and the card a spectator picks from.

use super::*;

/// Events spectators may call.
///
/// Seven of the nine. Crab Monopoly sends half the loose crabs to a
/// banker and Gull Attack spares one castle, and spectators have no castle to
/// spare and no bank to fill: a crowd that could hand a player points is
/// a crowd worth lobbying. What is left stirs the beach for everybody.
pub const SPECTATOR_EVENTS: [TideEvent; 7] = [
    TideEvent::CrabMania,
    TideEvent::GullMania,
    TideEvent::SpeedUp,
    TideEvent::SlowDown,
    TideEvent::FreshSand,
    TideEvent::CastleSwap,
    TideEvent::RightClaws,
];

/// Seconds a vote stays open once the first pick lands.
pub(super) const WINDOW: f32 = 3.0;

/// Seconds spectators wait between calls, however many of them there are.
///
/// Shared, not one apiece: a bigger crowd should be louder, not more
/// interrupting, and a three-minute round has room for a handful.
pub(super) const COOLDOWN: f32 = 25.0;

/// The spectators' tide vote as one peer holds it: the count (the host's
/// alone), the host's word on the crowd, and a call settled and waiting
/// for a frame.
#[derive(Default)]
pub struct TideVote {
    /// How the spectators' tide vote stands, counted by the host and
    /// nobody else.
    pub votes: SpectatorVotes,
    /// The last word the host said about the crowd.
    ///
    /// Only the host counts, so every other peer keeps the host's word
    /// here rather than a guess of its own. On the host it is what it is
    /// about to say.
    pub crowd: Crowd,
    /// Host: how long since it last said so.
    ///
    /// The word is repeated on a clock as well as on a change, because a
    /// count that only changes once a round travels in exactly one
    /// datagram, and UDP owes nobody that datagram. Worse than a lost
    /// pause, which the next tick makes good: the crowd's size is settled
    /// at the launch and never changes again, so the one send goes out
    /// while a spectator is still walking out of the lobby, where the
    /// lobby drops it as round business (`joining::work_the_socket`).
    /// That left the crowd unable to see itself for the whole round.
    pub crowd_said: f32,
    /// An event the spectators settled on, waiting for a frame to ride out on.
    ///
    /// Held rather than sent: it goes out as this seat's action, which is
    /// the only carriage a peer cannot advance past without.
    pub pending_call: Option<TideEvent>,
}

impl TideVote {
    /// Leaving the round: the settled-but-unsent call, the vote and its
    /// wait, and the last word about a crowd that has gone home, which
    /// would otherwise be carried into the next round until the host's
    /// first send happened to differ from it.
    pub fn leave_round(&mut self) {
        self.pending_call = None;
        self.votes.forget_all();
        self.crowd = Crowd::default();
    }
}

/// The vote as the host counts it.
///
/// Host side only, and deliberately outside the sim: what the spectators are
/// arguing about is nobody else's business and never has to agree
/// anywhere. Only the answer enters the round, and it enters as an action
/// on a frame, where it cannot be lost.
#[derive(Default)]
pub struct SpectatorVotes {
    /// Picks since the window opened: who picked (a peer index), and
    /// which `SPECTATOR_EVENTS` seat. One per voter, the latest standing,
    /// so a watcher who reopens the card and presses again changes its
    /// mind rather than stuffing the box. The winner calls were already
    /// one per peer (`Peer::pick`); this was a bare count.
    ballots: Vec<(usize, usize)>,
    /// Seconds left of an open window, or zero when none is open.
    open_for: f32,
    /// Seconds until spectators may call again.
    cooldown: f32,
}

impl SpectatorVotes {
    /// Take `voter`'s pick. The first one inside a quiet stretch opens
    /// the window; the rest fall into it, and a voter heard twice keeps
    /// only its latest.
    pub fn cast(&mut self, voter: usize, event: u8) {
        let Some(at) = SPECTATOR_EVENTS
            .iter()
            .position(|e| e.index() == usize::from(event))
        else {
            return; // not one of theirs, or not an event at all
        };
        if self.cooldown > 0.0 {
            return;
        }
        if self.open_for <= 0.0 {
            self.ballots.clear();
            self.open_for = WINDOW;
        }
        match self.ballots.iter_mut().find(|(who, _)| *who == voter) {
            Some(ballot) => ballot.1 = at,
            None => self.ballots.push((voter, at)),
        }
    }

    /// A peer has been forgotten: its ballot goes with it, and every voter
    /// behind it moves down one, as the peer indices themselves do. Left
    /// alone, the next voter to move into its index would replace its
    /// ballot rather than cast its own.
    pub fn voter_left(&mut self, peer: usize) {
        self.ballots.retain(|(who, _)| *who != peer);
        for (who, _) in &mut self.ballots {
            if *who > peer {
                *who -= 1;
            }
        }
    }

    /// Run the window down; the event the spectators settled on, once it closes.
    ///
    /// The tie-break is the earlier seat in [`SPECTATOR_EVENTS`], which is
    /// arbitrary but fixed: a tie broken by whichever vote the socket
    /// happened to hand over first would make the same room's same vote
    /// come out differently twice.
    pub fn settle(&mut self, seconds: f32) -> Option<TideEvent> {
        self.cooldown = (self.cooldown - seconds).max(0.0);
        if self.open_for <= 0.0 {
            return None;
        }
        self.open_for -= seconds;
        if self.open_for > 0.0 {
            return None;
        }
        self.open_for = 0.0;
        self.cooldown = COOLDOWN;
        let mut tally = [0u16; SPECTATOR_EVENTS.len()];
        for (_, at) in self.ballots.drain(..) {
            tally[at] = tally[at].saturating_add(1);
        }
        let (at, votes) = tally
            .iter()
            .enumerate()
            .max_by_key(|(at, votes)| (**votes, std::cmp::Reverse(*at)))?;
        (*votes > 0).then(|| SPECTATOR_EVENTS[at])
    }

    /// Drop an open window without settling it, leaving any wait alone.
    pub fn forget_open(&mut self) {
        self.open_for = 0.0;
        self.ballots.clear();
    }

    /// Forget the vote and the wait both: the round they belonged to is
    /// over.
    ///
    /// The wait only ever ran while a round did, so carrying it into the
    /// next one meant carrying a clock that had stopped: a call made in
    /// the last seconds cost the crowd most of the following round,
    /// having sat through a results card and an interlude that cost it
    /// nothing. Each round hands them their call back.
    pub fn forget_all(&mut self) {
        *self = SpectatorVotes::default();
    }

    /// Whether a vote is open, and how long there is left to join it.
    pub fn open(&self) -> Option<f32> {
        (self.open_for > 0.0).then_some(self.open_for)
    }

    /// Seconds until spectators may call again, if they are waiting.
    pub fn waiting(&self) -> Option<f32> {
        (self.cooldown > 0.0).then_some(self.cooldown)
    }
}

/// The spectators' event list, while one has it open.
#[derive(Component)]
pub struct SpectatorCardUi;

/// Whether the spectators' event list is on screen.
#[derive(Resource, Default)]
pub struct SpectatorCard(pub bool);

/// Open the list, pick from it, or put it away.
///
/// Numbers rather than a cursor: a card with seven rows and a crowd
/// behind it wants one press, not four. The keys are the same ones the
/// menu already numbers its modes with.
pub fn spectator_vote_input(
    mut card: ListCard<SpectatorCardUi, SpectatorCard>,
    keyboard: crate::app::keycaps::KeyboardMut,
    settings: Res<crate::app::settings::GameSettings>,
    watching: Watching<PickCard>,
    mut online: ResMut<Online>,
) {
    let crate::app::keycaps::KeyboardMut { mut keys, caps } = keyboard;
    // Typing takes the keyboard, losing a seat takes the job, and a round
    // that is over has no beach to call anything onto.
    if !is_spectating(&online) || !watching.free() {
        if card.is_up() {
            card.shut();
        }
        return;
    }
    if !card.is_up() {
        // Not over the other list: one card at a time, and both are numbers.
        if caps.just_pressed(&keys, 'E') && !watching.other_up() {
            spawn_card(card.raise(), &settings);
        }
        return;
    }
    if keys.just_pressed(KeyCode::Escape) || caps.just_pressed(&keys, 'E') {
        card.shut();
        // Or the pause card, behind it in the frame, opens on the same Esc.
        crate::app::menu_ui::spend(&mut keys, &[KeyCode::Escape]);
        return;
    }
    if let Some(at) = crate::app::menu_ui::number_pressed(&keys, SPECTATOR_EVENTS.len()) {
        if let Some(session) = &mut online.0 {
            // Straight to the host, which is the only peer that counts.
            // Never counted here on the way past: the host holds seat 0
            // and so is never a spectator, so this is always a spoke.
            session.transport.send(NetMsg::SpectatorVote {
                event: SPECTATOR_EVENTS[at].index() as u8,
            });
        }
        card.shut();
    }
}

pub(crate) fn spawn_card(commands: &mut Commands, settings: &crate::app::settings::GameSettings) {
    let tr = settings.tr();
    let rows = SPECTATOR_EVENTS.into_iter().enumerate().map(|(at, event)| {
        (
            format!("{}  {}", at + 1, tr.events[event.index()]),
            crate::app::palette::PARCHMENT,
        )
    });
    spawn_list_card(commands, SpectatorCardUi, tr.spectator_call_title, rows);
}
