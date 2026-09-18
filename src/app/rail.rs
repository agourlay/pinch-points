//! The rail: what the people with no seat can do while a round is played.
//!
//! A table seats six and a room holds more, so at a real beach there are
//! people standing behind the chairs. They already watch the round in step
//! with everyone else; this is what they can say about it.
//!
//! Only a spectator types. A player has both hands on the keys and a
//! letter key would be taken out of the round; the rail has free hands,
//! which is the whole reason the job falls to them.

use crate::app::net::Online;
use crate::sim::TideEvent;
use crate::transport::NetMsg;
use bevy::prelude::*;

/// The line the rail is typing, if one is open.
///
/// Its own resource rather than a field on the session: the session is
/// rebuilt between rounds and a half-typed line is not worth carrying
/// across, and the shell reads this to know the keyboard is spoken for.
#[derive(Resource, Default)]
pub struct RailChat(pub Option<String>);

impl RailChat {
    pub fn open(&self) -> bool {
        self.0.is_some()
    }
}

/// Events the rail may call.
///
/// Seven of the nine. Crab Monopoly sends half the loose crabs to a
/// banker and Gull Attack spares one castle, and the rail has no castle to
/// spare and no bank to fill: a crowd that could hand a player points is
/// a crowd worth lobbying. What is left stirs the beach for everybody.
pub const RAIL_EVENTS: [TideEvent; 7] = [
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

/// Seconds the rail waits between calls, however many of them there are.
///
/// Shared, not one apiece: a bigger crowd should be louder, not more
/// interrupting, and a three-minute round has room for a handful.
const COOLDOWN: f32 = 25.0;

/// The vote as the host counts it.
///
/// Host side only, and deliberately outside the sim: what the rail is
/// arguing about is nobody else's business and never has to agree
/// anywhere. Only the answer enters the round, and it enters as an action
/// on a frame, where it cannot be lost.
#[derive(Default)]
pub struct RailVotes {
    /// Picks since the window opened, one count per `RAIL_EVENTS` seat.
    tally: [u16; RAIL_EVENTS.len()],
    /// Seconds left of an open window, or zero when none is open.
    open_for: f32,
    /// Seconds until the rail may call again.
    cooldown: f32,
}

impl RailVotes {
    /// Take a spectator's pick. The first one inside a quiet stretch opens
    /// the window; the rest fall into it.
    pub fn cast(&mut self, event: u8) {
        let Some(at) = RAIL_EVENTS
            .iter()
            .position(|e| e.index() == usize::from(event))
        else {
            return; // not one of the rail's, or not an event at all
        };
        if self.cooldown > 0.0 {
            return;
        }
        if self.open_for <= 0.0 {
            self.tally = [0; RAIL_EVENTS.len()];
            self.open_for = WINDOW;
        }
        self.tally[at] = self.tally[at].saturating_add(1);
    }

    /// Run the window down; the event the rail settled on, once it closes.
    ///
    /// The tie-break is the earlier seat in [`RAIL_EVENTS`], which is
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
        (*votes > 0).then(|| RAIL_EVENTS[at])
    }

    /// Whether a vote is open, and how long the rail has left to join it.
    pub fn open(&self) -> Option<f32> {
        (self.open_for > 0.0).then_some(self.open_for)
    }

    /// Seconds until the rail may call again, if it is waiting.
    pub fn waiting(&self) -> Option<f32> {
        (self.cooldown > 0.0).then_some(self.cooldown)
    }
}

/// Whether this peer is at the rail: online, in a round, holding no seat.
pub fn at_the_rail(online: &Online) -> bool {
    online
        .0
        .as_ref()
        .is_some_and(|session| session.session.watching())
}

/// T opens a line, Enter says it, Esc drops it.
///
/// The key is read only at the rail, so a player's T is still a player's
/// T: nothing here can take a letter out of a round being played.
pub fn rail_chat_input(
    keys: Res<ButtonInput<KeyCode>>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    settings: Res<crate::app::settings::GameSettings>,
    mut chat: ResMut<RailChat>,
    mut online: ResMut<Online>,
) {
    if !at_the_rail(&online) {
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
        let mut votes = RailVotes::default();
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

        // And then the rail waits, however many of them there are.
        assert!(votes.waiting().is_some());
        votes.cast(fresh);
        assert!(
            votes.open().is_none(),
            "a vote during the wait opens nothing"
        );
        assert_eq!(votes.settle(COOLDOWN), None);
        assert!(votes.waiting().is_none(), "the wait is over");
        votes.cast(fresh);
        assert!(votes.open().is_some(), "and the rail may call again");
    }

    /// A tie goes to the earlier event on the list, which is arbitrary but
    /// fixed: broken by whichever datagram the socket handed over first,
    /// the same room's same vote would come out differently twice.
    #[test]
    fn a_tied_vote_breaks_the_same_way_every_time() {
        let pick = |order: [TideEvent; 2]| {
            let mut votes = RailVotes::default();
            for event in order {
                votes.cast(event.index() as u8);
            }
            votes.settle(WINDOW)
        };
        let early = RAIL_EVENTS[1];
        let late = RAIL_EVENTS[5];
        assert_eq!(pick([early, late]), Some(early));
        assert_eq!(pick([late, early]), Some(early), "whichever arrived first");
    }

    /// Two of the nine hand a player something, and the rail has no seat
    /// to be handed anything: a crowd that could bank for you is a crowd
    /// worth lobbying.
    #[test]
    fn the_rail_cannot_call_an_event_that_favours_a_seat() {
        for event in [TideEvent::Monopoly, TideEvent::GullAttack] {
            assert!(!RAIL_EVENTS.contains(&event), "{event:?}");
            let mut votes = RailVotes::default();
            votes.cast(event.index() as u8);
            assert!(votes.open().is_none(), "{event:?} is not on the list");
        }
        assert_eq!(
            RAIL_EVENTS.len() + 2,
            TideEvent::ALL.len(),
            "and the rest are"
        );
    }

    /// The rail is the people with no seat, and only they type: a player's
    /// hands are on the keys and a letter taken for a chat line is a
    /// letter taken out of the round.
    #[test]
    fn only_a_seatless_peer_is_at_the_rail() {
        assert!(!at_the_rail(&Online::default()), "nobody is online at all");
        assert!(
            !at_the_rail(&Online(Some(session(Some(0))))),
            "a player holds a seat"
        );
        assert!(
            at_the_rail(&Online(Some(session(None)))),
            "a watcher holds none"
        );
    }
}

/// Run the host's open vote down and hand the answer to the next frame.
///
/// Only the host does this, because only the host counts: every other
/// peer learns what the rail decided when the frame carrying it arrives.
pub fn settle_rail_vote(
    time: Res<Time>,
    settings: Res<crate::app::settings::GameSettings>,
    mut online: ResMut<Online>,
) {
    let Some(session) = &mut online.0 else {
        return;
    };
    if !session.is_host() {
        return;
    }
    let Some(event) = session.rail.settle(time.delta_secs()) else {
        return;
    };
    session.pending_call = Some(event);
    // Said by the room rather than by anyone in it: an empty name is the
    // beach's own voice, which is how the lobby already spells "this is
    // not a person talking".
    let tr = settings.tr();
    let line = crate::app::i18n::fill(tr.rail_called, &[("e", tr.events[event.index()])]);
    session.transport.send(NetMsg::chat("", &line));
    session.heard.push((String::new(), line));
}

/// The rail's event list, while a spectator has it open.
#[derive(Component)]
pub struct RailCardUi;

/// Whether the rail's event list is on screen.
#[derive(Resource, Default)]
pub struct RailCard(pub bool);

/// Open the list, pick from it, or put it away.
///
/// Numbers rather than a cursor: a card with seven rows and a crowd
/// behind it wants one press, not four. The keys are the same ones the
/// menu already numbers its modes with.
#[allow(clippy::too_many_arguments)]
pub fn rail_vote_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    settings: Res<crate::app::settings::GameSettings>,
    chat: Res<RailChat>,
    mut card: ResMut<RailCard>,
    mut online: ResMut<Online>,
    ui: Query<Entity, With<RailCardUi>>,
) {
    let shut = |commands: &mut Commands, card: &mut RailCard| {
        card.0 = false;
        for entity in &ui {
            commands.entity(entity).despawn();
        }
    };
    // Typing takes the keyboard, and losing a seat takes the job.
    if !at_the_rail(&online) || chat.open() {
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
            let event = RAIL_EVENTS[at].index() as u8;
            session.transport.send(NetMsg::RailVote { event });
            // The host counts its own rail too, and a host that is
            // watching is a host all the same.
            if session.is_host() {
                session.rail.cast(event);
            }
        }
        shut(&mut commands, &mut card);
        return;
    }
}

pub(super) fn spawn_card(commands: &mut Commands, settings: &crate::app::settings::GameSettings) {
    use crate::app::{menu_ui, palette};
    let tr = settings.tr();
    commands
        .spawn((RailCardUi, GlobalZIndex(20), menu_ui::centred_overlay()))
        .with_children(|wrap| {
            wrap.spawn(menu_ui::screen_card()).with_children(|card| {
                card.spawn((
                    Text::new(tr.rail_call_title),
                    TextFont {
                        font_size: FontSize::Px(menu_ui::type_scale::HEADING),
                        ..default()
                    },
                    TextColor(palette::GOLD),
                ));
                for (at, event) in RAIL_EVENTS.into_iter().enumerate() {
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
