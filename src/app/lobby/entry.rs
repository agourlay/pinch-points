//! Typing in the lobby: a name, a beach's name, or a line of chat.
//!
//! One editor for all three, because they are the same act (type, Enter,
//! Esc) and three private little text fields would be three chances to
//! forget that every other key falls silent while one is open.

use super::*;

/// What the keyboard is currently spelling out. One mechanism for all
/// three: see the module doc.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    /// Who you are. Asked for before anything else, because a beach full
    /// of unnamed players is nobody's idea of a game.
    PlayerName,
    /// What the beach is called. Asked when hosting, and what the list
    /// shows: with many games running, "Sam" is who, and "Room 3" is which.
    GameName,
    /// Where a beach is, typed out as `ip:port`.
    ///
    /// The way in when no beacon can reach: a network that drops
    /// broadcasts, a host on another subnet, a friend at the other end of a
    /// VPN. A hosted beach is a socket waiting to be greeted, so dialling
    /// one by hand joins just as picking it off the list would.
    Address,
    /// A line of chat.
    Chat,
}

/// What is being typed, and what has been typed so far.
#[derive(Clone, Debug)]
pub struct Typing {
    pub what: Entry,
    pub text: String,
    /// Set when the name is only being asked for on the way to something
    /// else: hosting, or joining the beach at this index.
    pub then: Option<Intent>,
    /// `text` is the game's suggestion rather than anything the player
    /// wrote: Enter (or South) takes it, and the first key typed replaces
    /// it instead of adding to it.
    pub suggested: bool,
}

/// What finishing a typed line means.
///
/// Split out from the system that acts on it so the whole walk from H to a
/// beach on the air can be tested without a keyboard, a socket or a
/// settings file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Answered {
    /// Not a usable answer. Ask the same thing again.
    AskAgain,
    /// A line for the table. Empty means the player thought better of it.
    Chat(String),
    /// Named themselves on the way to hosting; the beach needs one too.
    PlayerThenGame(String),
    /// Named themselves on the way to something that can now happen.
    PlayerThen(String, Option<Intent>),
    /// The beach is named, which is the last thing hosting waited on.
    GameNamed(String),
    /// An address that parsed. Who is dialling it is asked next.
    AddressGiven(SocketAddr),
    /// One that did not, handed back as typed: a mistyped address is
    /// almost always a good address with one character wrong, and clearing
    /// the line would make the player enter all of it again.
    BadAddress(String),
}

/// What was typed, and what it was being asked for.
pub(super) fn answer(open: &Typing, said: String) -> Answered {
    if said.is_empty() && matches!(open.what, Entry::PlayerName | Entry::GameName) {
        // A nameless player is a row that says nothing, and a nameless
        // beach is a row nobody can pick out of a hall full of them.
        return Answered::AskAgain;
    }
    match (open.what, open.then) {
        (Entry::Chat, _) => Answered::Chat(said),
        (Entry::PlayerName, Some(Intent::Host)) => Answered::PlayerThenGame(said),
        (Entry::PlayerName, then) => Answered::PlayerThen(said, then),
        (Entry::GameName, _) => Answered::GameNamed(said),
        // Strictly `ip:port`, with no name lookup behind it: resolving a
        // hostname blocks, and a lobby that freezes for a DNS timeout in
        // front of everyone is worse than one that will not take a name.
        (Entry::Address, _) => match said.parse::<SocketAddr>() {
            Ok(addr) => Answered::AddressGiven(addr),
            Err(_) => Answered::BadAddress(said),
        },
    }
}

impl Typing {
    pub fn chat() -> Typing {
        Typing {
            what: Entry::Chat,
            text: String::new(),
            then: None,
            suggested: false,
        }
    }

    /// Pre-filled with whatever it was called last, so a host putting the
    /// same beach up again only has to press Enter.
    pub fn game_name(was: &str) -> Typing {
        Typing {
            what: Entry::GameName,
            text: was.to_string(),
            then: None,
            suggested: false,
        }
    }

    /// Pre-filled with the last address dialled, which on a network that
    /// needed dialling once is very likely the address again.
    pub fn address(was: &str) -> Typing {
        Typing {
            what: Entry::Address,
            text: was.to_string(),
            then: None,
            suggested: false,
        }
    }

    /// Asked before `then` can happen, pre-filled with the name on file.
    ///
    /// Asked *every* time, not only when there is no name yet: two
    /// instances on one machine share one settings file, so the second
    /// player inherits the first one's name, and a machine on a busy LAN is
    /// one somebody else was sitting at ten minutes ago. Enter accepts what
    /// is already there, so the cost of asking is one keystroke.
    pub fn player_name(then: Intent, was: &str) -> Typing {
        Typing {
            what: Entry::PlayerName,
            text: was.to_string(),
            then: Some(then),
            suggested: false,
        }
    }
}

impl Typing {
    /// The chat line opened by a pad's North, already saying `line`.
    pub fn chat_saying(line: &str) -> Typing {
        Typing {
            text: line.to_string(),
            ..Typing::chat()
        }
    }

    /// Offer `suggestion` in a box that would otherwise open empty.
    ///
    /// For a pad, which has no keys to spell with: an empty name box is
    /// one it can only close. A box with something in it is one South can
    /// answer. A keyboard loses nothing, since the first letter typed
    /// replaces the suggestion rather than landing after it.
    pub fn or_suggest(mut self, suggestion: impl FnOnce() -> String) -> Typing {
        if self.text.trim().is_empty() {
            self.text = suggestion();
            self.suggested = true;
        }
        self
    }
}

/// A name for a player who has never given one: "Crab 37". Two digits, so
/// the three pads that walk into a hall together are three rows apart.
pub(super) fn suggested_name(tr: &crate::app::i18n::Tr) -> String {
    let n = 10 + crate::app::clock::fresh_seed() % 90;
    fill(tr.lobby_suggest_name, &[("n", &n.to_string())])
}

/// A name for a beach nobody has named: after its host.
pub(super) fn suggested_beach(tr: &crate::app::i18n::Tr, host: &str) -> String {
    fill(tr.lobby_suggest_beach, &[("p", host)])
}

/// The quick-chat line after `said`, going round: the first if `said` is
/// none of them, which is where a line opened by T starts.
pub(super) fn next_quick_line(tr: &crate::app::i18n::Tr, said: &str) -> &'static str {
    let lines = &tr.quick_chat;
    let next = lines
        .iter()
        .position(|&line| line == said)
        .map_or(0, |at| (at + 1) % lines.len());
    lines[next]
}

/// This frame's keystrokes into the open line, and the answer if one
/// finished it.
///
/// Two things on top of [`type_a_line`], both for a pad. South reaches the
/// lobby as a pressed Enter with no keystroke behind it, the pad bridge
/// having no text to give, so a pressed Enter the keystrokes did not see
/// finishes the line too. And a suggestion is taken whole by Enter and
/// dropped whole by the first key that types or erases.
fn read_line(
    keys: &ButtonInput<KeyCode>,
    typed: &mut MessageReader<bevy::input::keyboard::KeyboardInput>,
    open: &mut Typing,
) -> Option<String> {
    let said = if open.suggested {
        let mut fresh = String::new();
        match type_a_line(typed, &mut fresh) {
            // Enter over a suggestion that nothing was typed across.
            Some(said) if said.is_empty() => Some(open.text.trim().to_string()),
            Some(said) => Some(said),
            None => {
                if !fresh.is_empty() || keys.just_pressed(KeyCode::Backspace) {
                    open.text = fresh;
                    open.suggested = false;
                }
                None
            }
        }
    } else {
        type_a_line(typed, &mut open.text)
    };
    said.or_else(|| {
        (keys.just_pressed(KeyCode::Enter) && !keys.just_pressed(KeyCode::Escape))
            .then(|| open.text.trim().to_string())
    })
}

/// What the line being typed did with this frame.
///
/// Three states. As an `Option<Option<Intent>>` the nesting hid a bug; see
/// the note at the end of [`drive_typing`]. Two of these are "the lobby may
/// not act on this frame", spelled differently enough to be hard to swap.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Typed {
    /// Nothing was being typed. The frame belongs to the lobby.
    Nothing,
    /// The line took the frame, still being written or dropped or finished
    /// into another question, and the rest of the lobby must stay quiet.
    Taken,
    /// An answer finished and freed the thing it was asked for, which may
    /// go ahead this same frame.
    Unblocked(Intent),
}

/// Drive the line being typed, and say what it unblocked.
///
/// Lifted out of `lobby_input`, which had eight jobs and no seams.
///
/// `quick` is a pad's North this frame, which on an open chat line moves
/// it to the next quick-chat phrase.
pub(super) fn drive_typing(
    keys: &ButtonInput<KeyCode>,
    typed: &mut MessageReader<bevy::input::keyboard::KeyboardInput>,
    quick: bool,
    settings: &mut GameSettings,
    caps: &crate::app::keycaps::KeyCaps,
    state: &mut LobbyState,
    tr: &'static crate::app::i18n::Tr,
) -> Typed {
    let mut intent: Option<Intent> = None;
    let Some(mut open) = state.typing.take() else {
        return Typed::Nothing;
    };
    if quick && open.what == Entry::Chat {
        open.text = next_quick_line(tr, &open.text).to_string();
    }
    {
        let finished = read_line(keys, typed, &mut open);
        let Some(said) = finished else {
            // Esc closes it; anything else and it is still being written.
            if !keys.just_pressed(KeyCode::Escape) {
                state.typing = Some(open);
            }
            return Typed::Taken;
        };
        match answer(&open, said) {
            Answered::AskAgain => {
                // Asked again with a suggestion in the box, not an empty
                // one, or a pad that got here would have no way on.
                let again = Typing {
                    text: String::new(),
                    suggested: false,
                    ..open
                };
                state.typing = Some(match again.what {
                    Entry::GameName => {
                        let host = settings.names[0].clone();
                        again.or_suggest(|| suggested_beach(tr, &host))
                    }
                    Entry::PlayerName => again.or_suggest(|| suggested_name(tr)),
                    // Never refused as empty (see `answer`), and a player's
                    // name is no suggestion for either.
                    Entry::Address | Entry::Chat => again,
                });
                state.feedback = tr.lobby_needs_name.to_string();
            }
            Answered::Chat(line) if line.is_empty() => {}
            Answered::Chat(line) => {
                let me = settings.names[0].clone();
                if let Some(transport) = state.transport() {
                    transport.send(NetMsg::chat(&me, &line));
                }
                state.say(&me, &line);
            }
            Answered::PlayerThenGame(name) => {
                name_myself(settings, caps, name);
                // After it is kept, so the beach is named after the name
                // the gate let through, as the refused-name path names it.
                let host = settings.names[0].clone();
                let was = state.game_name.clone();
                state.typing =
                    Some(Typing::game_name(&was).or_suggest(|| suggested_beach(tr, &host)));
            }
            Answered::PlayerThen(name, then) => {
                name_myself(settings, caps, name);
                intent = then;
            }
            Answered::GameNamed(name) => {
                state.game_name = name;
                intent = Some(Intent::Host);
            }
            // Kept before the beach is even dialled: whether anyone
            // answers or not, this is the address the player will want
            // back in the box on the next try.
            Answered::AddressGiven(addr) => {
                settings.last_beach = addr.to_string();
                settings.save(caps);
                state.typing = Some(
                    Typing::player_name(Intent::Dial(addr), &settings.names[0])
                        .or_suggest(|| suggested_name(tr)),
                );
            }
            Answered::BadAddress(text) => {
                state.typing = Some(Typing { text, ..open });
                state.feedback = tr.lobby_bad_address.to_string();
            }
        }
    }
    // This frame's keys belonged to the line, whatever came of it: only an
    // answer that unblocked something lets the rest of the lobby act, and
    // it acts on that intent rather than on the keystroke that finished
    // the line.
    //
    // Answering `Nothing` here instead, as `Some(Some(intent?))` did by way
    // of the `?`, hands the Enter that ended a line back to a lobby that
    // reads Enter as *join the beach under the cursor*: naming yourself on
    // the way to hosting dialled somebody else's beach with the same
    // keystroke.
    match intent {
        Some(intent) => Typed::Unblocked(intent),
        None => Typed::Taken,
    }
}

/// Keep the name the player gave, tidied and on disk. Their own name is
/// the one thing about them every other screen in the hall will read.
pub(super) fn name_myself(
    settings: &mut GameSettings,
    caps: &crate::app::keycaps::KeyCaps,
    name: String,
) {
    // Through the gate, not around it: this is a name the player typed,
    // and `type_a_line` only holds it to the chat line's length.
    settings.set_name(0, &name);
    settings.tidy_name(0);
    settings.save(caps);
}

/// The keyboard belongs to the line being typed: characters land in it,
/// Backspace rubs one out, Enter says it and Esc drops it. Bevy's `text`
/// rather than the key codes, so the player's own layout and shift key
/// decide what a keystroke means, the same way a name is typed.
///
/// Returns the finished line, if Enter finished one.
///
/// `pub(crate)` because a spectator types during a round too, which is a
/// fourth caller of the same act: type, Enter, Esc, capped at the length a
/// chat line goes onto the wire at.
pub(crate) fn type_a_line(
    typed: &mut MessageReader<bevy::input::keyboard::KeyboardInput>,
    line: &mut String,
) -> Option<String> {
    use crate::app::typing::{Keystroke, keystrokes};
    let mut sent = None;
    let mut dropped = false;
    let ends = [KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Escape];
    for stroke in keystrokes(typed, &ends) {
        match stroke {
            Keystroke::Done(KeyCode::Escape) => dropped = true,
            // Enter, on either side of the keyboard: the line goes out.
            Keystroke::Done(_) => sent = Some(std::mem::take(line)),
            Keystroke::Erase => {
                line.pop();
            }
            Keystroke::Char(ch) if line.chars().count() < crate::transport::CHAT_CHARS => {
                line.push(ch)
            }
            Keystroke::Char(_) => {}
        }
    }
    if dropped {
        line.clear();
        return None;
    }
    sent.map(|line| line.trim().to_string())
}

#[cfg(test)]
mod door_tests {
    use super::*;

    /// The walk from pressing H to a beach on the air, in the order a
    /// player actually makes it. This is the way into every online game
    /// there is; if it has a hole, nothing behind it can be reached.
    #[test]
    fn h_leads_to_a_beach_on_the_air() {
        // Nothing yet, nothing pressed.
        assert_eq!(host_step(idle()), HostStep::Nothing);

        // H asks who is asking, rather than hosting on the spot.
        assert_eq!(
            host_step(HostAsk {
                pressed_h: true,
                ..idle()
            }),
            HostStep::Ask
        );
        let asked = Typing::player_name(Intent::Host, "");
        assert_eq!(asked.what, Entry::PlayerName);
        assert_eq!(asked.then, Some(Intent::Host));

        // A name, and the beach is asked about next, not hosted yet.
        let next = answer(&asked, "Bob".into());
        assert_eq!(next, Answered::PlayerThenGame("Bob".into()));
        let asked = Typing::game_name("");
        assert_eq!(asked.what, Entry::GameName);

        // The beach's name is the last thing it waited on.
        assert_eq!(
            answer(&asked, "Room 3".into()),
            Answered::GameNamed("Room 3".into())
        );
        assert_eq!(
            host_step(HostAsk {
                answered: true,
                ..idle()
            }),
            HostStep::Go,
            "answered, and nothing else needed pressing"
        );
    }

    /// The same walk when the player already has a name: still asked, and
    /// still arriving. Pre-filled means one keystroke, not none.
    #[test]
    fn a_player_who_has_a_name_is_still_asked() {
        let asked = Typing::player_name(Intent::Host, "Bob");
        assert_eq!(asked.text, "Bob", "pre-filled, so Enter accepts it");
        assert_eq!(
            answer(&asked, "Bob".into()),
            Answered::PlayerThenGame("Bob".into())
        );
    }

    /// Joining takes the shorter road: a name, and then the beach that was
    /// under the cursor when they were stopped.
    #[test]
    fn joining_asks_the_name_and_then_joins() {
        let asked = Typing::player_name(Intent::Join(3), "");
        assert_eq!(
            answer(&asked, "Cy".into()),
            Answered::PlayerThen("Cy".into(), Some(Intent::Join(3))),
            "and remembers which beach, not merely that there was one"
        );
    }

    /// The other way in, for a beach no beacon reached: J, an address, a
    /// name, and the same dialling as any row of the list.
    ///
    /// The address is asked for *first* and the name second, so the name
    /// step is the `PlayerThen` every joiner already walks through.
    #[test]
    fn an_address_is_asked_first_and_the_name_second() {
        let asked = Typing::address("");
        assert_eq!(asked.what, Entry::Address);
        let there: SocketAddr = "192.168.1.5:47777".parse().expect("addr");
        assert_eq!(
            answer(&asked, "192.168.1.5:47777".into()),
            Answered::AddressGiven(there)
        );

        // And then who is dialling, down the road every joiner takes.
        let asked = Typing::player_name(Intent::Dial(there), "");
        assert_eq!(
            answer(&asked, "Cy".into()),
            Answered::PlayerThen("Cy".into(), Some(Intent::Dial(there))),
            "and remembers where it was going"
        );
    }

    /// A mistyped address comes back as typed rather than cleared: it is
    /// almost always a good address with one character wrong, and fifteen
    /// characters is a lot to ask for twice.
    #[test]
    fn a_mistyped_address_is_handed_back() {
        let asked = Typing::address("");
        for typo in ["192.168.1.5", "192.168.1.5:", "over there", ""] {
            assert_eq!(
                answer(&asked, typo.into()),
                Answered::BadAddress(typo.into()),
                "{typo:?}"
            );
        }
        // A port on its own is not a beach either: an address names a
        // machine, and there is no default one to assume.
        assert_eq!(
            answer(&asked, "47777".into()),
            Answered::BadAddress("47777".into())
        );
        // IPv6 is written the way the standard library writes it.
        assert_eq!(
            answer(&asked, "[::1]:47777".into()),
            Answered::AddressGiven("[::1]:47777".parse().expect("addr"))
        );
    }

    /// An empty answer is not an answer. Nothing may go on the air without
    /// a name, and the same keystroke that would have confirmed it asks
    /// again instead of silently doing nothing.
    #[test]
    fn an_empty_name_is_refused_at_both_doors() {
        for open in [
            Typing::player_name(Intent::Host, ""),
            Typing::player_name(Intent::Join(0), ""),
            Typing::game_name(""),
        ] {
            assert_eq!(answer(&open, String::new()), Answered::AskAgain, "{open:?}");
            assert_eq!(answer(&open, "  ".trim().to_string()), Answered::AskAgain);
        }
        // Chat is the exception: an empty line is a change of mind, and
        // being nagged for one would be absurd.
        assert_eq!(
            answer(&Typing::chat(), String::new()),
            Answered::Chat(String::new())
        );
    }

    /// A name is asked for *while browsing*, before there is a beach to be
    /// at. With the prompt inside the chat card, which is on the face that
    /// is hidden then, H turned every key into text and appeared to do
    /// nothing. The prompt belongs to neither face.
    #[test]
    fn the_prompt_is_not_on_a_face_that_can_be_hidden() {
        let mut app = App::new();
        app.insert_resource(GameSettings::default());
        app.init_resource::<LobbyState>();
        app.add_systems(
            Startup,
            |mut commands: Commands, settings: Res<GameSettings>| {
                spawn_lobby_ui(&mut commands, settings.tr(), &LobbyArt::default());
            },
        );
        app.add_systems(Update, update_lobby_view);
        app.update();

        // Browsing: nothing hosted, nothing joined, so the table face is
        // hidden, which is when H is pressed.
        let world = app.world_mut();
        assert!(!world.resource::<LobbyState>().can_chat());
        let mut rows = world.query::<(Entity, &ChatRow)>();
        let prompt = rows
            .iter(world)
            .find(|(_, row)| row.0 == CHAT_LINES)
            .map(|(entity, _)| entity)
            .expect("the row the question is asked on");

        let mut at = prompt;
        let mut hops = 0;
        while let Some(parent) = world.get::<ChildOf>(at) {
            at = parent.parent();
            hops += 1;
            assert!(hops < 16, "walked too far up; is the tree a loop?");
            assert!(
                world.get::<TableView>(at).is_none(),
                "the prompt hangs off the face that is hidden while browsing"
            );
            assert!(
                world.get::<BrowseView>(at).is_none(),
                "nor may it hang off the other one, hidden the rest of the time"
            );
            if let Some(node) = world.get::<Node>(at) {
                assert_ne!(node.display, Display::None, "an ancestor is hidden");
            }
        }
    }

    /// The Enter that finishes an answer is not also an Enter on the list.
    ///
    /// Two questions in a row (a name and then what the beach is called, a
    /// name and then where it is) are the only places where finishing a
    /// line leaves another one open. The lobby reads Enter as "join the
    /// beach under the cursor", so that same keystroke dialled one: press
    /// H, type your name, and be joined to somebody else's game with "name
    /// your beach" still on screen.
    #[test]
    fn finishing_one_question_does_not_join_the_beach_under_the_cursor() {
        use bevy::input::ButtonState;
        use bevy::input::keyboard::{Key, KeyboardInput};

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<LobbyState>();
        app.init_resource::<MatchConfig>();
        app.init_resource::<crate::app::match_setup::CustomBeaches>();
        app.insert_resource(GameSettings::default());
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.add_message::<KeyboardInput>();
        app.add_systems(Update, lobby_input);

        // One beach on the air, with the cursor on it.
        let there: SocketAddr = "10.0.0.7:47777".parse().expect("addr");
        {
            let mut state = app.world_mut().resource_mut::<LobbyState>();
            state.hosts = vec![HostEntry {
                addr: there,
                id: 7,
                name: "somebody else's".into(),
                host: "Sam".into(),
                taken: 1,
                seats: 6,
                running: false,
                age: 0.0,
            }];
            state.selected = Some(there);
        }

        let press_enter = |app: &mut App| {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Enter);
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::Enter,
                logical_key: Key::Enter,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
        };

        // Naming yourself on the way to hosting.
        app.world_mut().resource_mut::<LobbyState>().typing =
            Some(Typing::player_name(Intent::Host, "Bob"));
        press_enter(&mut app);
        let state = app.world().resource::<LobbyState>();
        assert_eq!(
            state.typing.as_ref().map(|open| open.what),
            Some(Entry::GameName),
            "the next question is up"
        );
        assert!(
            state.joined().is_none(),
            "and nothing was dialled behind it"
        );

        // And the same walk on the road that has no list behind it at all.
        app.world_mut().resource_mut::<LobbyState>().typing =
            Some(Typing::address("10.0.0.9:47777"));
        press_enter(&mut app);
        let state = app.world().resource::<LobbyState>();
        assert_eq!(
            state.typing.as_ref().map(|open| open.what),
            Some(Entry::PlayerName),
            "who is dialling is asked next"
        );
        assert!(
            state.joined().is_none(),
            "and the cursor's beach is not taken"
        );
    }

    /// The lobby with one pad plugged in and no keyboard typing anything.
    fn pad_lobby() -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<LobbyState>();
        app.init_resource::<MatchConfig>();
        app.init_resource::<crate::app::match_setup::CustomBeaches>();
        app.insert_resource(GameSettings::default());
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.add_message::<bevy::input::keyboard::KeyboardInput>();
        app.world_mut().spawn(Gamepad::default());
        app.add_systems(Update, lobby_input);
        app
    }

    /// South as the lobby hears it: a pressed Enter, and no keystroke.
    fn south(app: &mut App) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
    }

    fn north(app: &mut App) {
        let pad = app
            .world_mut()
            .query_filtered::<Entity, With<Gamepad>>()
            .single(app.world())
            .expect("one pad");
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("the pad");
        gamepad.digital_mut().press(GamepadButton::North);
        app.update();
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("the pad");
        gamepad.digital_mut().release(GamepadButton::North);
        gamepad.digital_mut().clear();
    }

    fn typing(app: &App) -> Option<Typing> {
        app.world().resource::<LobbyState>().typing.clone()
    }

    /// A player who has never given a name gets one to accept, and South
    /// accepts it: a pad with no keys to spell with still gets through the
    /// door, and on to naming the beach, which is suggested the same way.
    #[test]
    fn a_pad_takes_the_suggested_names() {
        let mut app = pad_lobby();
        app.world_mut().resource_mut::<LobbyState>().typing =
            Some(Typing::player_name(Intent::Host, "").or_suggest(|| "Crab 42".into()));
        south(&mut app);
        assert_eq!(app.world().resource::<GameSettings>().names[0], "Crab 42");
        let next = typing(&app).expect("the beach's name is asked next");
        assert_eq!(next.what, Entry::GameName);
        assert_eq!(next.text, "Crab 42's beach");
        assert!(next.suggested);
    }

    /// A name already on file is no suggestion, and South takes it as is.
    #[test]
    fn a_pad_takes_the_name_on_file() {
        let asked = Typing::player_name(Intent::Host, "Maya").or_suggest(|| "Crab 42".into());
        assert_eq!(asked.text, "Maya");
        assert!(!asked.suggested);
        let mut app = pad_lobby();
        app.world_mut().resource_mut::<LobbyState>().typing = Some(asked);
        south(&mut app);
        assert_eq!(app.world().resource::<GameSettings>().names[0], "Maya");
    }

    /// The first key typed over a suggestion replaces it, rather than
    /// writing "Crab 42Bo".
    #[test]
    fn typing_over_a_suggestion_replaces_it() {
        use bevy::input::ButtonState;
        use bevy::input::keyboard::{Key, KeyboardInput};
        let mut app = pad_lobby();
        app.world_mut().resource_mut::<LobbyState>().typing =
            Some(Typing::player_name(Intent::Host, "").or_suggest(|| "Crab 42".into()));
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyB,
            logical_key: Key::Character("B".into()),
            state: ButtonState::Pressed,
            text: Some("B".into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        let open = typing(&app).expect("still being typed");
        assert_eq!(open.text, "B");
        assert!(!open.suggested);
    }

    /// North opens the chat line on the first phrase and walks the list,
    /// round again at the end, and South says the one showing.
    #[test]
    fn north_walks_the_quick_chat_and_south_says_it() {
        let tr = GameSettings::default().tr();
        let mut app = pad_lobby();
        app.world_mut().resource_mut::<LobbyState>().typing = Some(Typing::chat());
        north(&mut app);
        assert_eq!(typing(&app).expect("open").text, tr.quick_chat[0]);
        north(&mut app);
        assert_eq!(typing(&app).expect("open").text, tr.quick_chat[1]);
        for _ in 2..tr.quick_chat.len() {
            north(&mut app);
        }
        north(&mut app);
        assert_eq!(
            typing(&app).expect("open").text,
            tr.quick_chat[0],
            "round again"
        );
        north(&mut app);
        south(&mut app);
        let state = app.world().resource::<LobbyState>();
        assert!(state.typing.is_none(), "said and closed");
        assert_eq!(
            state.chat.last().map(|said| said.line.as_str()),
            Some(tr.quick_chat[1])
        );
    }

    /// Erase a suggestion and press Enter on the empty box, and it comes
    /// back with a suggestion in it, not empty: a pad that got there would
    /// otherwise have no way on.
    #[test]
    fn a_refused_name_is_asked_again_with_a_suggestion() {
        let mut app = pad_lobby();
        app.world_mut().resource_mut::<LobbyState>().typing =
            Some(Typing::player_name(Intent::Host, ""));
        south(&mut app);
        let again = typing(&app).expect("asked again");
        assert_eq!(again.what, Entry::PlayerName);
        assert!(again.suggested && !again.text.is_empty(), "{again:?}");
    }
}
