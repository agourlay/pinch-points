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

mod chat;
mod picks;
mod vote;

pub use chat::*;
pub use picks::*;
pub use vote::*;

/// What the host says about the crowd, and the only word anyone else has
/// on it.
///
/// A struct and not a `(u8, u8, u8)`: with nothing but position to say
/// which is which, a reader that swapped the count for a countdown would
/// compile and run, and all three are seconds-or-a-number. The same
/// argument `HudText` makes about its own three strings.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Crowd {
    /// How many are watching this round, holding no chair in it.
    pub watching: u8,
    /// Seconds left to join an open vote, or zero when none is open.
    pub open: u8,
    /// Seconds until the next call may be made, or zero when none is owed.
    pub wait: u8,
}

/// Where the people without a chair stand in a round: their tide vote as
/// the host counts it and the host's word on it, a call settled and
/// waiting for a frame, and their calls for the winner. Every peer keeps
/// one; the host is the one that fills in the counts.
#[derive(Default)]
pub struct Stands {
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
    /// The host's word on the crowd's calls for the winner (on the host,
    /// what it is about to say), like [`Self::crowd`].
    pub picks: Picks,
    /// Host: whether the closing count has gone to the feed this round.
    pub picks_said: bool,
    /// A spectator's own call this round, if it made one.
    pub my_pick: Option<u8>,
    /// A spectator's calls over the whole session, for the results card.
    pub calls: Calls,
    /// How this round's call went, once the tide is in: the seat, and
    /// whether it won.
    pub last_call: Option<(u8, bool)>,
}

impl Stands {
    /// A new round: every call is for one round, while the record of them
    /// is the session's, and the vote's wait carries on with the crowd.
    pub fn new_round(&mut self) {
        self.picks = Picks::default();
        self.picks_said = false;
        self.my_pick = None;
        self.last_call = None;
    }

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

/// Whether this peer is a spectator: online, in a round, holding no seat.
pub fn is_spectating(online: &Online) -> bool {
    online
        .0
        .as_ref()
        .is_some_and(|session| session.session.watching())
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
        session.stands.votes.forget_open();
        return;
    }
    // What the crowd is told, before anything settles. Said when it
    // changes and once a second besides, rather than thirty times a
    // second: the change alone is one datagram UDP may keep, and this is
    // the only word anyone but the host has on the matter.
    let crowd = Crowd {
        watching: session.watchers_in_round().min(u8::MAX as usize) as u8,
        open: session
            .stands
            .votes
            .open()
            .map_or(0, |left| left.ceil() as u8),
        wait: session
            .stands
            .votes
            .waiting()
            .map_or(0, |left| left.ceil() as u8),
    };
    let again = crate::app::lobby::once_a_second(&mut session.stands.crowd_said, time.delta_secs());
    if crowd != session.stands.crowd || again {
        session.stands.crowd = crowd;
        session.transport.send(NetMsg::SpectatorTally {
            watching: crowd.watching,
            open: crowd.open,
            wait: crowd.wait,
        });
    }
    // The calls for the winner, on the same clock.
    say_the_calls(session, settings.tr(), again);
    let Some(event) = session.stands.votes.settle(time.delta_secs()) else {
        return;
    };
    session.stands.pending_call = Some(event);
    // Said by the room rather than by anyone in it: an empty name is the
    // beach's own voice, which is how the lobby already spells "this is
    // not a person talking".
    let tr = settings.tr();
    let line = crate::app::i18n::fill(tr.spectator_called, &[("e", tr.events[event.index()])]);
    session.transport.send(NetMsg::chat("", &line));
    session.heard.push((String::new(), line));
}

/// Either of the spectators' two lists, the tide's or the winner's.
type EitherCard = Or<(With<SpectatorCardUi>, With<PickCardUi>)>;

/// Put the spectators' things away with the round.
///
/// The card is an overlay like the pause card, so like the pause card it
/// has to be taken down by hand: left to itself it floats over the menu
/// of whoever walked out with it open. The half-typed line and the
/// settled-but-unsent call go the same way, for the same reason.
pub fn forget_spectating(
    mut commands: Commands,
    mut chat: ResMut<SpectatorChat>,
    (mut card, mut pick): (ResMut<SpectatorCard>, ResMut<PickCard>),
    mut online: ResMut<Online>,
    ui: Query<Entity, EitherCard>,
) {
    chat.0 = None;
    card.0 = false;
    pick.0 = false;
    for entity in &ui {
        commands.entity(entity).despawn();
    }
    if let Some(session) = &mut online.0 {
        session.stands.leave_round();
    }
}

#[cfg(test)]
mod tests {
    use super::vote::{COOLDOWN, WINDOW};
    use super::*;
    use crate::app::net::OnlineSession;
    use crate::sim::{DEFAULT_DELAY, Lockstep};
    use crate::transport::{MatchTerms, UdpTransport};

    use bevy::ecs::system::RunSystemOnce;
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};

    /// A round on screen with one peer in it, seated or not, and the three
    /// spectator systems wired in the order the schedule chains them.
    fn beach(local: Option<u8>) -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<crate::app::Screen>();
        app.init_state::<crate::app::VersusPhase>();
        app.insert_resource(State::new(crate::app::Screen::Versus));
        app.insert_resource(State::new(crate::app::VersusPhase::Running));
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<Time>();
        app.add_message::<KeyboardInput>();
        // Named, as the lobby leaves anyone who got this far: it asks
        // before it lets you watch.
        let mut settings = crate::app::settings::GameSettings::default();
        settings.set_name(0, "Wanda");
        app.insert_resource(settings);
        app.init_resource::<SpectatorChat>();
        app.init_resource::<SpectatorCard>();
        app.init_resource::<PickCard>();
        app.insert_resource(Online(Some(session(local))));
        app.add_systems(
            Update,
            (
                spectator_chat_input,
                spectator_vote_input,
                spectator_pick_input,
                settle_spectator_vote,
            )
                .chain(),
        );
        app
    }

    /// The host's word that the calls are open, as it would arrive.
    fn calls_open(app: &mut App, open: u8) {
        if let Some(session) = &mut app.world_mut().resource_mut::<Online>().0 {
            session.stands.picks.open = open;
        }
    }

    fn my_pick(app: &App) -> Option<u8> {
        app.world()
            .resource::<Online>()
            .0
            .as_ref()
            .and_then(|s| s.stands.my_pick)
    }

    /// P opens the seats while the calls are open, a number calls one and
    /// shuts the list, and a second call replaces the first.
    #[test]
    fn p_opens_the_seats_and_a_number_calls_one() {
        let mut app = beach(None);
        calls_open(&mut app, 20);
        tap(&mut app, KeyCode::KeyP);
        assert!(app.world().resource::<PickCard>().0, "P opened it");
        tap(&mut app, KeyCode::Digit2);
        assert!(!app.world().resource::<PickCard>().0, "a call shuts it");
        assert_eq!(my_pick(&app), Some(1));
        tap(&mut app, KeyCode::KeyP);
        tap(&mut app, KeyCode::Digit1);
        assert_eq!(my_pick(&app), Some(0), "a change of mind");
        // A seat past the table's is no call.
        tap(&mut app, KeyCode::KeyP);
        tap(&mut app, KeyCode::Digit5);
        assert_eq!(my_pick(&app), Some(0));
    }

    /// Once the calls are in, P does nothing; and a player has no call.
    #[test]
    fn no_calls_once_they_close_or_from_a_player() {
        let mut app = beach(None);
        calls_open(&mut app, 0);
        tap(&mut app, KeyCode::KeyP);
        assert!(!app.world().resource::<PickCard>().0);
        let mut app = beach(Some(0));
        calls_open(&mut app, 20);
        tap(&mut app, KeyCode::KeyP);
        assert!(!app.world().resource::<PickCard>().0, "a seated player");
    }

    /// The crowd's calls read most called first, the lower seat first on a
    /// tie, so every screen lists them the same.
    #[test]
    fn the_crowds_calls_read_most_called_first() {
        let picks = Picks {
            counts: [1, 0, 2, 1, 0, 0],
            open: 0,
        };
        assert_eq!(
            picks.line(|seat| format!("P{}", seat + 1)),
            "P3 ×2 · P1 ×1 · P4 ×1"
        );
        assert!(!Picks::default().any());
    }

    /// The tide comes in and a spectator's call is scored against the
    /// standings' own winners, into the session's record.
    #[test]
    fn a_call_is_scored_against_the_winner() {
        for (pick, right) in [(1, true), (0, false)] {
            let mut app = App::new();
            let mut board = crate::sim::classic_arena(false, 2);
            board.set_score(0, 3);
            board.set_score(1, 9);
            app.insert_resource(crate::app::Sim(board));
            app.insert_resource(crate::app::settings::GameSettings::default());
            let mut watcher = session(None);
            watcher.stands.my_pick = Some(pick);
            app.insert_resource(Online(Some(watcher)));
            app.add_systems(Update, score_the_call);
            app.update();
            let session = app
                .world()
                .resource::<Online>()
                .0
                .as_ref()
                .expect("session");
            assert_eq!(session.stands.last_call, Some((pick, right)));
            assert_eq!(
                (session.stands.calls.right, session.stands.calls.made),
                (u32::from(right), 1)
            );
        }
    }

    /// A key press as the window actually reports one: the button state
    /// *and* the keystroke message. Half of it is not a keyboard, and a
    /// harness sending only the button state finds Enter doing nothing,
    /// because the line being typed is driven by the messages.
    fn tap(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.world_mut().write_message(KeyboardInput {
            key_code: key,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }

    /// One character, as the keyboard actually delivers it: a key code and
    /// the text the layout made of it.
    fn type_char(app: &mut App, ch: &str) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyB,
            logical_key: Key::Character(ch.into()),
            state: ButtonState::Pressed,
            text: Some(ch.into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }

    fn saying(app: &App) -> Option<String> {
        app.world().resource::<SpectatorChat>().0.clone()
    }

    fn card_open(app: &App) -> bool {
        app.world().resource::<SpectatorCard>().0
    }

    fn heard(app: &App) -> Vec<(String, String)> {
        app.world()
            .resource::<Online>()
            .0
            .as_ref()
            .expect("a session")
            .heard
            .clone()
    }

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
    fn spectators_settle_on_one_event_and_then_wait() {
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
    fn spectators_cannot_call_an_event_that_favours_a_seat() {
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
    fn only_a_seatless_peer_is_a_spectator() {
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

    // --- saying something ------------------------------------------

    /// The whole of it: T opens a line, the letters land in it, Enter says
    /// it to the table, and the sender sees its own words, because its own
    /// datagram never comes back to it.
    #[test]
    fn t_opens_a_line_and_enter_says_it() {
        let mut app = beach(None);
        assert_eq!(saying(&app), None, "no line to begin with");

        tap(&mut app, KeyCode::KeyT);
        assert_eq!(saying(&app), Some(String::new()), "T opened one");

        type_char(&mut app, "h");
        type_char(&mut app, "i");
        assert_eq!(saying(&app), Some("hi".to_string()));

        tap(&mut app, KeyCode::Enter);
        assert_eq!(saying(&app), None, "saying it closes the line");
        assert_eq!(
            heard(&app),
            vec![("Wanda".to_string(), "hi".to_string())],
            "and the sender sees it too"
        );
    }

    /// Escape drops what was being written, and drops it unsaid.
    #[test]
    fn escape_drops_a_half_typed_line_without_saying_it() {
        let mut app = beach(None);
        tap(&mut app, KeyCode::KeyT);
        type_char(&mut app, "o");
        type_char(&mut app, "i");
        assert_eq!(saying(&app), Some("oi".to_string()));

        tap(&mut app, KeyCode::Escape);
        assert_eq!(saying(&app), None, "the line is gone");
        assert!(heard(&app).is_empty(), "and nobody heard it");
    }

    /// A player's T is a player's T. A letter taken for a chat line is a
    /// letter taken out of the round, which is why only the seatless type.
    #[test]
    fn a_seated_player_keeps_its_letters() {
        let mut app = beach(Some(0));
        tap(&mut app, KeyCode::KeyT);
        assert_eq!(saying(&app), None, "no line opened");
        type_char(&mut app, "x");
        assert_eq!(saying(&app), None, "and nowhere for a letter to go");
        assert!(heard(&app).is_empty());
    }

    /// Two ways a line comes to nothing. An empty one, because a blank
    /// row on a feed nine deep is a row spent on silence. And one from a
    /// player with no name, because the hub refuses a line that resolves
    /// to no name (an empty name is the beach's own voice), so echoing it
    /// here would show the sender something nobody else was given.
    #[test]
    fn an_empty_line_and_a_nameless_one_both_say_nothing() {
        let mut app = beach(None);
        tap(&mut app, KeyCode::KeyT);
        tap(&mut app, KeyCode::Enter);
        assert_eq!(saying(&app), None, "the line closed");
        assert!(heard(&app).is_empty(), "with nothing said");

        app.world_mut()
            .resource_mut::<crate::app::settings::GameSettings>()
            .set_name(0, "");
        tap(&mut app, KeyCode::KeyT);
        type_char(&mut app, "?");
        tap(&mut app, KeyCode::Enter);
        assert_eq!(saying(&app), None, "the line closed either way");
        assert!(heard(&app).is_empty(), "and nobody speaks as the beach");
    }

    /// The session ending under a half-typed line takes the line with it:
    /// there is nobody left to say it to, and it must not still be sitting
    /// there when the next round opens.
    #[test]
    fn losing_the_session_takes_a_half_typed_line_with_it() {
        let mut app = beach(None);
        tap(&mut app, KeyCode::KeyT);
        type_char(&mut app, "w");
        assert_eq!(saying(&app), Some("w".to_string()));

        app.world_mut().resource_mut::<Online>().0 = None;
        app.update();
        assert_eq!(saying(&app), None, "gone with the session");
    }

    // --- calling the tide ------------------------------------------

    /// E opens the list and a number picks from it, in one press each: a
    /// card with seven rows and a crowd behind it wants no cursor.
    #[test]
    fn e_opens_the_list_and_a_number_votes() {
        let mut app = beach(None);
        assert!(!card_open(&app), "shut to begin with");

        tap(&mut app, KeyCode::KeyE);
        assert!(card_open(&app), "E opened it");

        tap(&mut app, KeyCode::Digit3);
        assert!(!card_open(&app), "a pick shuts it behind itself");
    }

    /// Escape puts the list away without picking anything.
    #[test]
    fn escape_shuts_the_list_without_voting() {
        let mut app = beach(None);
        tap(&mut app, KeyCode::KeyE);
        assert!(card_open(&app));
        tap(&mut app, KeyCode::Escape);
        assert!(!card_open(&app), "shut again");
    }

    /// A seat is a job. Someone holding one is not standing behind a chair
    /// deciding what to do to the people who are.
    #[test]
    fn a_seated_player_cannot_open_the_list() {
        let mut app = beach(Some(0));
        tap(&mut app, KeyCode::KeyE);
        assert!(!card_open(&app), "no list for a player");
    }

    /// A line being typed takes the keyboard, and the list goes with it: a
    /// digit belongs to the line, not to the wheel.
    #[test]
    fn a_line_being_typed_shuts_the_list() {
        let mut app = beach(None);
        tap(&mut app, KeyCode::KeyE);
        assert!(card_open(&app));

        tap(&mut app, KeyCode::KeyT);
        assert_eq!(saying(&app), Some(String::new()), "the line opened");
        assert!(!card_open(&app), "and the list stood down");

        type_char(&mut app, "5");
        assert_eq!(saying(&app), Some("5".to_string()), "the digit was typed");
    }

    /// Once the tide is in the board is frozen and there is nothing to
    /// call onto it, so the key stops being offered rather than starting
    /// to do nothing.
    #[test]
    fn the_list_is_not_offered_once_the_tide_is_in() {
        let mut app = beach(None);
        tap(&mut app, KeyCode::KeyE);
        assert!(card_open(&app));

        app.insert_resource(State::new(crate::app::VersusPhase::Over));
        app.update();
        assert!(!card_open(&app), "the list went with the round");

        tap(&mut app, KeyCode::KeyE);
        assert!(!card_open(&app), "and does not come back");
    }

    // --- settling, and packing up ----------------------------------

    /// Only the host counts, so only the host has anything to hand to a
    /// frame. A spoke that somehow held votes would put an event into its
    /// own round and nobody else's, which is a desync wearing a party hat.
    #[test]
    fn only_the_host_settles_a_vote() {
        // Seat 0 is the host, seat 1 is a spoke. Neither is a spectator:
        // the host always holds a chair, which is why its own vote never
        // goes through these keys at all.
        for (seat, expected) in [(1, None), (0, Some(TideEvent::FreshSand))] {
            let mut app = beach(Some(seat));
            {
                let mut online = app.world_mut().resource_mut::<Online>();
                let session = online.0.as_mut().expect("a session");
                session
                    .stands
                    .votes
                    .cast(TideEvent::FreshSand.index() as u8);
            }
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(WINDOW + 0.1));
            app.update();

            let call = app
                .world()
                .resource::<Online>()
                .0
                .as_ref()
                .expect("a session")
                .stands
                .pending_call;
            assert_eq!(call, expected, "seat {seat}");
        }
    }

    /// The crowd's three numbers are told apart by name, not by where they
    /// sit. They are all small numbers and two of them are seconds, so a
    /// reader that took the count for a countdown would compile and run,
    /// which is the argument `HudText` makes about its own three strings.
    #[test]
    fn the_crowds_numbers_are_told_apart_by_name() {
        let crowd = Crowd {
            watching: 3,
            open: 2,
            wait: 0,
        };
        assert_eq!(crowd.watching, 3, "three at the back");
        assert_eq!(crowd.open, 2, "two seconds to join them");
        assert_eq!(crowd.wait, 0, "and nothing owed");
        assert_eq!(
            Crowd::default(),
            Crowd {
                watching: 0,
                open: 0,
                wait: 0
            }
        );
    }

    /// The wait belongs to the round it was earned in. It only ever ran
    /// while a round did, so carrying it meant carrying a clock that had
    /// stopped: a call made in the last seconds cost the crowd most of the
    /// next round, having sat out a results card and an interlude that
    /// cost it nothing.
    #[test]
    fn the_wait_ends_with_the_round_rather_than_freezing_into_the_next() {
        let mut votes = SpectatorVotes::default();
        votes.cast(TideEvent::FreshSand.index() as u8);
        assert_eq!(votes.settle(WINDOW), Some(TideEvent::FreshSand));
        assert!(votes.waiting().is_some(), "a wait was earned");

        // Within the round it stands: a second window may not jump it.
        votes.forget_open();
        assert!(votes.waiting().is_some(), "still waiting mid-round");

        votes.forget_all();
        assert!(votes.waiting().is_none(), "and the round takes it away");
        votes.cast(TideEvent::FreshSand.index() as u8);
        assert!(votes.open().is_some(), "the next round starts them fresh");
    }

    /// Leaving the round puts everything away. The list is an overlay like
    /// the pause card, so like the pause card nothing takes it down on its
    /// own: left to itself it floats over the menu of whoever walked out
    /// with it open.
    #[test]
    fn leaving_the_round_puts_everything_away() {
        let mut app = beach(None);
        tap(&mut app, KeyCode::KeyE);
        assert!(card_open(&app), "a list to leave open");
        assert!(
            app.world_mut()
                .query::<&SpectatorCardUi>()
                .iter(app.world())
                .next()
                .is_some(),
            "and something on screen to leave behind"
        );
        {
            let mut chat = app.world_mut().resource_mut::<SpectatorChat>();
            chat.0 = Some("half a thought".into());
        }
        {
            let mut online = app.world_mut().resource_mut::<Online>();
            let session = online.0.as_mut().expect("a session");
            session.stands.pending_call = Some(TideEvent::CastleSwap);
            session
                .stands
                .votes
                .cast(TideEvent::FreshSand.index() as u8);
        }

        let _ = app.world_mut().run_system_once(forget_spectating);

        assert!(!card_open(&app), "the list is shut");
        assert_eq!(saying(&app), None, "the line is dropped");
        assert_eq!(
            app.world_mut()
                .query::<&SpectatorCardUi>()
                .iter(app.world())
                .count(),
            0,
            "and nothing of it is left on screen"
        );
        let online = app.world().resource::<Online>();
        let session = online.0.as_ref().expect("a session");
        assert_eq!(
            session.stands.pending_call, None,
            "no call waiting on a frame"
        );
        assert!(session.stands.votes.open().is_none(), "no vote still open");
    }

    /// The crowd's size is said again on a clock, not only when it moves.
    ///
    /// Found by watching three real processes: the count settles at the
    /// launch and never changes again, so it travelled in exactly one
    /// datagram, sent on the first running frame - the frame a spectator
    /// is still walking out of the lobby on, where `work_the_socket`
    /// drops round business on the floor. The players saw "1 watching"
    /// and the spectator, alone in a crowd of one, saw nothing at all.
    #[test]
    fn the_crowd_is_told_its_size_again_without_being_asked() {
        let mut app = beach(Some(0));
        let port = {
            let online = app.world().resource::<Online>();
            let session = online.0.as_ref().expect("a session");
            session.transport.local_addr().expect("addr").port()
        };
        // Someone on the other end of the wire, so there is a peer to say
        // it to and a socket to read it off.
        let mut ear = UdpTransport::join(("127.0.0.1", port)).expect("join");
        ear.send(NetMsg::watch("Dee"));
        let mut tallies = 0;
        for _ in 0..3 {
            std::thread::sleep(std::time::Duration::from_millis(30));
            // The socket registers a peer from the datagram it sent, and
            // nothing is broadcast to a peer the socket has never met.
            {
                let mut online = app.world_mut().resource_mut::<Online>();
                let session = online.0.as_mut().expect("a session");
                let _ = session.transport.recv_all();
            }
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(1.1));
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(30));
            tallies += ear
                .recv_all()
                .into_iter()
                .filter(|(msg, _)| matches!(msg, NetMsg::SpectatorTally { .. }))
                .count();
        }
        assert!(
            tallies >= 2,
            "the crowd hears where it stands more than once, got {tallies}"
        );
    }
}
