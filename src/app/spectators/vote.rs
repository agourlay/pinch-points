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

/// The vote as the host counts it.
///
/// Host side only, and deliberately outside the sim: what the spectators are
/// arguing about is nobody else's business and never has to agree
/// anywhere. Only the answer enters the round, and it enters as an action
/// on a frame, where it cannot be lost.
#[derive(Default)]
pub struct SpectatorVotes {
    /// Picks since the window opened, one count per `SPECTATOR_EVENTS` seat.
    tally: [u16; SPECTATOR_EVENTS.len()],
    /// Seconds left of an open window, or zero when none is open.
    open_for: f32,
    /// Seconds until spectators may call again.
    cooldown: f32,
}

impl SpectatorVotes {
    /// Take a spectator's pick. The first one inside a quiet stretch opens
    /// the window; the rest fall into it.
    pub fn cast(&mut self, event: u8) {
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
            self.tally = [0; SPECTATOR_EVENTS.len()];
            self.open_for = WINDOW;
        }
        self.tally[at] = self.tally[at].saturating_add(1);
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
        let (at, votes) = self
            .tally
            .iter()
            .enumerate()
            .max_by_key(|(at, votes)| (**votes, std::cmp::Reverse(*at)))?;
        (*votes > 0).then(|| SPECTATOR_EVENTS[at])
    }

    /// Drop an open window without settling it, leaving any wait alone.
    pub fn forget_open(&mut self) {
        self.open_for = 0.0;
        self.tally = [0; SPECTATOR_EVENTS.len()];
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
#[allow(clippy::too_many_arguments)]
pub fn spectator_vote_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    settings: Res<crate::app::settings::GameSettings>,
    chat: Res<SpectatorChat>,
    phase: Res<State<crate::app::VersusPhase>>,
    (mut card, pick): (ResMut<SpectatorCard>, Res<PickCard>),
    mut online: ResMut<Online>,
    ui: Query<Entity, With<SpectatorCardUi>>,
) {
    let shut = |commands: &mut Commands, card: &mut SpectatorCard| {
        card.0 = false;
        for entity in &ui {
            commands.entity(entity).despawn();
        }
    };
    // Typing takes the keyboard, losing a seat takes the job, and a round
    // that is over has no beach to call anything onto.
    let playing = *phase.get() == crate::app::VersusPhase::Running;
    if !is_spectating(&online) || chat.open() || !playing {
        if card.0 {
            shut(&mut commands, &mut card);
        }
        return;
    }
    if !card.0 {
        // Not over the other list: one card at a time, and both are numbers.
        if caps.just_pressed(&keys, 'E') && !pick.0 {
            card.0 = true;
            spawn_card(&mut commands, &settings);
        }
        return;
    }
    if keys.just_pressed(KeyCode::Escape) || caps.just_pressed(&keys, 'E') {
        shut(&mut commands, &mut card);
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
        shut(&mut commands, &mut card);
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
