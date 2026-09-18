//! Spectators: what the people with no seat can do while a round is played.
//!
//! A table seats six and a room holds more, so at a real beach there are
//! people standing behind the chairs. They already watch the round in step
//! with everyone else; this is what they can say about it.
//!
//! Only a spectator types. A player has both hands on the keys and a
//! letter key would be taken out of the round; a spectator has free hands,
//! which is the whole reason the job falls to them.

use crate::app::net::Online;
use crate::sim::TideEvent;
use crate::transport::NetMsg;
use bevy::prelude::*;

/// The line a spectator is typing, if one is open.
///
/// Its own resource rather than a field on the session: the session is
/// rebuilt between rounds and a half-typed line is not worth carrying
/// across, and the shell reads this to know the keyboard is spoken for.
#[derive(Resource, Default)]
pub struct SpectatorChat(pub Option<String>);

impl SpectatorChat {
    pub fn open(&self) -> bool {
        self.0.is_some()
    }
}

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
const WINDOW: f32 = 3.0;

/// Seconds spectators wait between calls, however many of them there are.
///
/// Shared, not one apiece: a bigger crowd should be louder, not more
/// interrupting, and a three-minute round has room for a handful.
const COOLDOWN: f32 = 25.0;

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

    /// Whether a vote is open, and how long there is left to join it.
    pub fn open(&self) -> Option<f32> {
        (self.open_for > 0.0).then_some(self.open_for)
    }

    /// Seconds until spectators may call again, if they are waiting.
    pub fn waiting(&self) -> Option<f32> {
        (self.cooldown > 0.0).then_some(self.cooldown)
    }
}

/// Whether this peer is a spectator: online, in a round, holding no seat.
pub fn is_spectating(online: &Online) -> bool {
    online
        .0
        .as_ref()
        .is_some_and(|session| session.session.watching())
}

/// T opens a line, Enter says it, Esc drops it.
///
/// The key is read only for a spectator, so a player's T is still a player's
/// T: nothing here can take a letter out of a round being played.
pub fn spectator_chat_input(
    keys: Res<ButtonInput<KeyCode>>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    settings: Res<crate::app::settings::GameSettings>,
    mut chat: ResMut<SpectatorChat>,
    mut online: ResMut<Online>,
) {
    if !is_spectating(&online) {
        // A seat was handed out mid-round, or the session ended under a
        // half-typed line. Either way the line is not going anywhere.
        chat.0 = None;
        typed.clear();
        return;
    }
    let Some(line) = &mut chat.0 else {
        // Not typing: the keystrokes belong to whatever else reads them,
        // and T is the way in.
        if caps.just_pressed(&keys, 'T') {
            chat.0 = Some(String::new());
            typed.clear();
        }
        return;
    };
    let said = crate::app::lobby::type_a_line(&mut typed, line);
    if keys.just_pressed(KeyCode::Escape) {
        chat.0 = None;
        return;
    }
    let Some(said) = said else {
        return;
    };
    chat.0 = None;
    if said.trim().is_empty() {
        return;
    }
    let me = settings.names[0].clone();
    if let Some(session) = &mut online.0 {
        session.transport.send(NetMsg::chat(&me, &said));
        // Said to the table, and shown here too: the sender is a peer like
        // any other and its own datagram never comes back to it.
        session.heard.push((me, said));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::net::OnlineSession;
    use crate::sim::{DEFAULT_DELAY, Lockstep};
    use crate::transport::{MatchTerms, UdpTransport};

    fn session(local: Option<u8>) -> OnlineSession {
        let step = match local {
            Some(seat) => Lockstep::new(seat, vec![0, 1], DEFAULT_DELAY),
            None => Lockstep::observer(vec![0, 1], DEFAULT_DELAY),
        };
        OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            step,
            2,
            MatchTerms::default(),
        )
    }

    /// The window, the majority, the tie-break and the cooldown.
    #[test]
    fn the_rail_settles_on_one_event_and_then_waits() {
        let mut votes = SpectatorVotes::default();
        assert!(votes.open().is_none(), "nothing open to begin with");
        assert_eq!(votes.settle(1.0), None, "and nothing to settle");

        // The first pick opens the window; the rest fall into it.
        let fresh = TideEvent::FreshSand.index() as u8;
        let swap = TideEvent::CastleSwap.index() as u8;
        votes.cast(fresh);
        assert!(votes.open().is_some(), "the first pick opened it");
        votes.cast(swap);
        votes.cast(swap);

        // It stays open for its few seconds, then gives the majority.
        assert_eq!(votes.settle(1.0), None, "still open");
        assert_eq!(votes.settle(WINDOW), Some(TideEvent::CastleSwap));

        // And then they wait, however many of them there are.
        assert!(votes.waiting().is_some());
        votes.cast(fresh);
        assert!(
            votes.open().is_none(),
            "a vote during the wait opens nothing"
        );
        assert_eq!(votes.settle(COOLDOWN), None);
        assert!(votes.waiting().is_none(), "the wait is over");
        votes.cast(fresh);
        assert!(votes.open().is_some(), "and they may call again");
    }

    /// A tie goes to the earlier event on the list, which is arbitrary but
    /// fixed: broken by whichever datagram the socket handed over first,
    /// the same room's same vote would come out differently twice.
    #[test]
    fn a_tied_vote_breaks_the_same_way_every_time() {
        let pick = |order: [TideEvent; 2]| {
            let mut votes = SpectatorVotes::default();
            for event in order {
                votes.cast(event.index() as u8);
            }
            votes.settle(WINDOW)
        };
        let early = SPECTATOR_EVENTS[1];
        let late = SPECTATOR_EVENTS[5];
        assert_eq!(pick([early, late]), Some(early));
        assert_eq!(pick([late, early]), Some(early), "whichever arrived first");
    }

    /// A window still open when the tide comes in settles onto nothing.
    /// It used to settle into a call held in hand, which then rode out on
    /// the first frame of the *next* round: an event nobody watching had
    /// voted for, in a round that had not started when they voted.
    #[test]
    fn a_vote_still_open_at_the_wave_is_dropped_rather_than_held() {
        let mut votes = SpectatorVotes::default();
        votes.cast(TideEvent::FreshSand.index() as u8);
        assert!(votes.open().is_some());

        votes.forget_open();
        assert!(votes.open().is_none(), "nothing left to settle");
        assert_eq!(votes.settle(WINDOW), None, "and nothing settles");
        assert!(
            votes.waiting().is_none(),
            "a vote that never happened costs no wait"
        );

        // The wait itself is left alone, so dropping a window mid-cooldown
        // does not hand the next one back early.
        let mut votes = SpectatorVotes::default();
        votes.cast(TideEvent::FreshSand.index() as u8);
        assert!(votes.settle(WINDOW).is_some());
        votes.forget_open();
        assert!(votes.waiting().is_some(), "still waiting");
    }

    /// Two of the nine hand a player something, and spectators have no seat
    /// to be handed anything: a crowd that could bank for you is a crowd
    /// worth lobbying.
    #[test]
    fn the_rail_cannot_call_an_event_that_favours_a_seat() {
        for event in [TideEvent::Monopoly, TideEvent::GullAttack] {
            assert!(!SPECTATOR_EVENTS.contains(&event), "{event:?}");
            let mut votes = SpectatorVotes::default();
            votes.cast(event.index() as u8);
            assert!(votes.open().is_none(), "{event:?} is not on the list");
        }
        assert_eq!(
            SPECTATOR_EVENTS.len() + 2,
            TideEvent::ALL.len(),
            "and the rest are"
        );
    }

    /// Spectators are the people with no seat, and only they type: a player's
    /// hands are on the keys and a letter taken for a chat line is a
    /// letter taken out of the round.
    #[test]
    fn only_a_seatless_peer_is_is_spectating() {
        assert!(
            !is_spectating(&Online::default()),
            "nobody is online at all"
        );
        assert!(
            !is_spectating(&Online(Some(session(Some(0))))),
            "a player holds a seat"
        );
        assert!(
            is_spectating(&Online(Some(session(None)))),
            "a watcher holds none"
        );
    }
}

/// Run the host's open vote down and hand the answer to the next frame.
///
/// Only the host does this, because only the host counts: every other
/// peer learns what the spectators decided when the frame carrying it arrives.
pub fn settle_spectator_vote(
    time: Res<Time>,
    settings: Res<crate::app::settings::GameSettings>,
    phase: Res<State<crate::app::VersusPhase>>,
    mut online: ResMut<Online>,
) {
    let Some(session) = &mut online.0 else {
        return;
    };
    if !session.is_host() {
        return;
    }
    if *phase.get() != crate::app::VersusPhase::Running {
        // The tide is in and the board is frozen. A window still open when
        // it came in settles onto nothing, rather than waiting in hand for
        // a frame and firing at the start of the next round, called by
        // nobody who is still watching.
        session.forget_open_vote();
        return;
    }
    let Some(event) = session.spectators.settle(time.delta_secs()) else {
        return;
    };
    session.pending_call = Some(event);
    // Said by the room rather than by anyone in it: an empty name is the
    // beach's own voice, which is how the lobby already spells "this is
    // not a person talking".
    let tr = settings.tr();
    let line = crate::app::i18n::fill(tr.spectator_called, &[("e", tr.events[event.index()])]);
    session.transport.send(NetMsg::chat("", &line));
    session.heard.push((String::new(), line));
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
    mut card: ResMut<SpectatorCard>,
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
        if caps.just_pressed(&keys, 'E') {
            card.0 = true;
            spawn_card(&mut commands, &settings);
        }
        return;
    }
    if keys.just_pressed(KeyCode::Escape) || caps.just_pressed(&keys, 'E') {
        shut(&mut commands, &mut card);
        return;
    }
    const PICKS: [KeyCode; 7] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
    ];
    for (at, key) in PICKS.into_iter().enumerate() {
        if !keys.just_pressed(key) {
            continue;
        }
        if let Some(session) = &mut online.0 {
            // Straight to the host, which is the only peer that counts.
            // Never counted here on the way past: the host holds seat 0
            // and so is never a spectator, so this is always a spoke.
            session.transport.send(NetMsg::SpectatorVote {
                event: SPECTATOR_EVENTS[at].index() as u8,
            });
        }
        shut(&mut commands, &mut card);
        return;
    }
}

pub(super) fn spawn_card(commands: &mut Commands, settings: &crate::app::settings::GameSettings) {
    use crate::app::{menu_ui, palette};
    let tr = settings.tr();
    commands
        .spawn((
            SpectatorCardUi,
            GlobalZIndex(20),
            menu_ui::centred_overlay(),
        ))
        .with_children(|wrap| {
            wrap.spawn(menu_ui::screen_card()).with_children(|card| {
                card.spawn((
                    Text::new(tr.spectator_call_title),
                    TextFont {
                        font_size: FontSize::Px(menu_ui::type_scale::HEADING),
                        ..default()
                    },
                    TextColor(palette::GOLD),
                ));
                for (at, event) in SPECTATOR_EVENTS.into_iter().enumerate() {
                    card.spawn((
                        Text::new(format!("{}  {}", at + 1, tr.events[event.index()])),
                        TextFont {
                            font_size: FontSize::Px(menu_ui::type_scale::ROW),
                            ..default()
                        },
                        TextColor(palette::PARCHMENT),
                    ));
                }
            });
        });
}

/// Put the spectators' things away with the round.
///
/// The card is an overlay like the pause card, so like the pause card it
/// has to be taken down by hand: left to itself it floats over the menu
/// of whoever walked out with it open. The half-typed line and the
/// settled-but-unsent call go the same way, for the same reason.
pub fn forget_spectating(
    mut commands: Commands,
    mut chat: ResMut<SpectatorChat>,
    mut card: ResMut<SpectatorCard>,
    mut online: ResMut<Online>,
    ui: Query<Entity, With<SpectatorCardUi>>,
) {
    chat.0 = None;
    card.0 = false;
    for entity in &ui {
        commands.entity(entity).despawn();
    }
    if let Some(session) = &mut online.0 {
        session.pending_call = None;
        session.forget_open_vote();
    }
}
