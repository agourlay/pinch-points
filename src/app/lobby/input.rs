//! The lobby's keys: hosting, joining by number, watching, the doorway
//! for a bot, and the ways out.

use super::*;

/// The ways out of the lobby a key can take: to another screen, or
/// through a doorway for a bot.
#[derive(bevy::ecs::system::SystemParam)]
pub struct LobbyExits<'w> {
    next_screen: ResMut<'w, NextState<Screen>>,
    bots: ResMut<'w, crate::app::bot_seats::BotSeats>,
    /// Whether a round is under way, which a joiner walking into the
    /// arena is part of before the screen changes.
    online: Res<'w, Online>,
}

/// B on a beach: open a doorway for the bot that will join it.
fn open_door_for(
    state: &mut LobbyState,
    bots: &mut crate::app::bot_seats::BotSeats,
    tr: &'static crate::app::i18n::Tr,
    at: usize,
) {
    let Some(host) = state.hosts.get(at) else {
        return;
    };
    if !host.bots {
        state.feedback = tr.lobby_no_bots.to_string();
        return;
    }
    if !host.has_room() {
        state.feedback = tr.lobby_beach_full.to_string();
        return;
    }
    let addr = host.addr;
    state.selected = Some(addr);
    match crate::app::bot_seats::Doorway::open() {
        Ok(mut door) => {
            door.seat_bots(&[0]);
            bots.door = Some(door);
            bots.door_for = crate::app::bot_seats::DoorFor::Join(addr);
        }
        Err(e) => state.feedback = fill(tr.door_could_not_listen, &[("e", &e.to_string())]),
    }
}

/// Lobby input: host, join by number, leave, and say something.
pub fn lobby_input(
    keyboard: crate::app::keycaps::Keyboard,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut settings: ResMut<GameSettings>,
    dials: crate::app::match_setup::Dials,
    mut state: ResMut<LobbyState>,
    exits: LobbyExits,
    pads: Query<&Gamepad>,
) {
    let crate::app::keycaps::Keyboard { keys, caps } = keyboard;
    let LobbyExits {
        mut next_screen,
        mut bots,
        online,
    } = exits;
    bots.leave_the_beach(state.joined().is_some() || online.0.is_some());
    // A doorway card is up for a bot about to join a beach: its keys are
    // its own (see `bot_seats::door_input`), and the moment its bot is in,
    // this game dials the beach as that bot's seat.
    if bots.door_for.card_up() {
        typed.clear();
        let tr = settings.tr();
        if let Some(door) = &bots.door
            && door.ready()
            && let crate::app::bot_seats::DoorFor::Join(addr) = bots.door_for
        {
            let name = door.named_for(tr, &settings.names[0]);
            bots.door_for = crate::app::bot_seats::DoorFor::Joined(addr);
            dial_as(&mut state, &settings, tr, addr, name);
        }
        return;
    }
    let crate::app::match_setup::Dials {
        mut config,
        beaches,
    } = dials;
    let tr = settings.tr();
    let tr: &'static crate::app::i18n::Tr = tr;
    // A pad's North is quick chat: the one way a player with no keyboard
    // has to say anything at all.
    let quick = pads
        .iter()
        .any(|pad| pad.just_pressed(GamepadButton::North));
    // Set when an answer finishes and the thing it was blocking can now go
    // ahead in the same frame.
    let mut intent: Option<Intent> = None;
    // While anything is being typed the keyboard is text and nothing else:
    // H would host, W would arm watching, and a digit would join a beach,
    // none of which anyone means halfway through "wait for me".
    match drive_typing(
        &keys,
        &mut typed,
        quick,
        &mut settings,
        &caps,
        &mut state,
        tr,
    ) {
        // The keyboard is busy: every other key in this lobby is text.
        Typed::Taken => return,
        // An answer finished and freed something to happen this frame.
        Typed::Unblocked(unblocked) => intent = Some(unblocked),
        Typed::Nothing => {}
    }
    // The reader must be drained even when nothing is being typed, or the
    // first line composed picks up every keystroke since the lobby opened.
    typed.clear();
    // T for talk, once there is anyone to talk to.
    if intent.is_none() && caps.just_pressed(&keys, 'T') && state.can_chat() {
        state.typing = Some(Typing::chat());
        return;
    }
    // North for the same line, saying the first quick-chat phrase already.
    if intent.is_none() && quick && state.can_chat() {
        state.typing = Some(Typing::chat_saying(tr.quick_chat[0]));
        return;
    }
    // J for a beach the list will never show: one whose beacons the
    // network drops, or that is a subnet away. Only while browsing, since
    // from a beach you are already at there is nothing to dial, and never
    // over a question already on screen, which a finished answer may have
    // opened earlier in this same frame.
    if intent.is_none()
        && caps.just_pressed(&keys, 'J')
        && !state.standing().at_a_beach()
        && state.typing.is_none()
    {
        state.typing = Some(Typing::address(&settings.last_beach));
        return;
    }
    let auto_host = crate::app::dev::auto_host_quota().is_some() && !state.auto_done;
    // PINCH_LOBBY_WATCH joins the first host found as a spectator, which is
    // the only way to watch: a session replays from frame zero, so an
    // onlooker has to be in the lobby with everyone else.
    let auto_watch = crate::app::dev::auto_watch();
    let auto_join =
        (crate::app::dev::auto_join() || auto_watch) && !state.auto_done && !state.hosts.is_empty();
    if auto_watch {
        state.set_watching(true);
    }
    // B: join the beach under the cursor as a bot, through a doorway. The
    // dev hook presses it on the first beach heard.
    if crate::app::dev::auto_join_bot() && !state.auto_done && !state.hosts.is_empty() {
        state.auto_done = true;
        open_door_for(&mut state, &mut bots, tr, 0);
        return;
    }
    let join_bot = match intent {
        // Gone off the air while the name was typed: nothing is taken in
        // its place, and the player is told why.
        Some(Intent::JoinBot(addr)) => {
            let at = state.hosts.iter().position(|host| host.addr == addr);
            if at.is_none() {
                state.feedback = tr.lobby_host_closed.to_string();
            }
            at
        }
        _ if intent.is_none()
            && caps.just_pressed(&keys, 'B')
            && !state.standing().at_a_beach()
            && state.typing.is_none() =>
        {
            state.selected_index()
        }
        _ => None,
    };
    if let Some(at) = join_bot {
        if settings.names[0].trim().is_empty() && intent.is_none() {
            state.typing = Some(
                Typing::player_name(Intent::JoinBot(state.hosts[at].addr), &settings.names[0])
                    .or_suggest(|| suggested_name(tr)),
            );
        } else {
            open_door_for(&mut state, &mut bots, tr, at);
        }
        return;
    }
    let step = host_step(HostAsk {
        answered: intent == Some(Intent::Host),
        pressed_h: caps.just_pressed(&keys, 'H'),
        busy: state.standing().at_a_beach(),
        auto: auto_host,
    });
    if step == HostStep::Ask {
        // Stopped at the door until we know who is asking and what the
        // beach is to be called. Both are what the rest of the hall reads.
        state.typing = Some(
            Typing::player_name(Intent::Host, &settings.names[0]).or_suggest(|| suggested_name(tr)),
        );
        return;
    }
    if step == HostStep::Go && !state.standing.at_a_beach() {
        state.auto_done = auto_host;
        match (
            Announcer::new(crate::app::clock::fresh_seed()),
            UdpTransport::host(0),
        ) {
            (Ok(announcer), Ok(transport)) => {
                state.feedback = match transport.local_addr() {
                    Ok(addr) => fill(tr.lobby_hosting, &[("p", &addr.port().to_string())]),
                    Err(_) => tr.lobby_hosting_noport.into(),
                };
                // Where this beach is, in the feed rather than the status
                // line: the status line is taken over by the rival count
                // the moment somebody arrives, and this is the number a
                // friend on a network that eats broadcasts has to be told
                // before they can dial it.
                if let Some(here) = address_here(&transport) {
                    let notice = fill(tr.lobby_hosting_at, &[("a", &here)]);
                    state.say("", &notice);
                }
                state.standing = Standing::hosting(announcer, transport);
                // Stop listening. A host cannot join anyone, and holding a
                // lobby port it will never read would deny one to another
                // instance on the same machine. It also stops the host
                // hearing its own beacon come back round the loopback and
                // listing itself.
                state.discovery = None;
                state.hosts.clear();
            }
            (Err(e), _) | (_, Err(e)) => {
                state.feedback = fill(tr.lobby_could_not_host, &[("e", &e.to_string())]);
            }
        }
    }
    // K: ask somebody to leave. The next number names them, Esc does not.
    let typing = state.typing.is_some();
    if let Some(hosted) = state.hosted_mut()
        && !typing
        && caps.just_pressed(&keys, 'K')
    {
        hosted.kicking = true;
        return;
    }
    if let Some(hosted) = state.hosted_mut()
        && hosted.kicking
    {
        if keys.just_pressed(KeyCode::Escape) {
            hosted.kicking = false;
            return;
        }
        if let Some(at) =
            crate::app::menu_ui::number_pressed(&keys, crate::app::menu_ui::NUMBER_KEYS.len())
        {
            hosted.kicking = false;
            ask_to_leave(&mut state, &mut bots, tr, at + 1)
        }
        return;
    }
    if state.hosting() {
        turn_the_dials(
            &keys,
            &mut settings,
            &caps,
            &mut config,
            &mut state,
            &beaches,
        );
    }
    if !state.standing.at_a_beach() {
        walk_the_list(&keys, &mut state);
    }
    take_a_beach(&keys, &settings, &mut state, tr, intent, auto_join);
    // By the cap, like H, B, T and K on the same prompt: on AZERTY the
    // key QWERTY calls W says Z, and the one that says W is elsewhere.
    if caps.just_pressed(&keys, 'W')
        && let Standing::Choosing { watching } = &mut state.standing
    {
        *watching = !*watching;
    }
    if keys.just_pressed(KeyCode::Escape) {
        next_screen.set(Screen::Menu);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W arms watching by the cap that says W, like every other letter on
    /// the prompt. On AZERTY that is the key QWERTY calls Z, and the key
    /// QWERTY calls W (which says Z there) does nothing.
    #[test]
    fn w_arms_watching_by_its_cap_on_azerty() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<LobbyState>();
        app.init_resource::<MatchConfig>();
        app.init_resource::<crate::app::bot_seats::BotSeats>();
        app.init_resource::<Online>();
        app.init_resource::<crate::app::match_setup::CustomBeaches>();
        app.insert_resource(GameSettings::default());
        let mut azerty = crate::app::keycaps::KeyCaps::default();
        azerty.learn(KeyCode::KeyW, 'z');
        app.insert_resource(azerty);
        app.add_message::<bevy::input::keyboard::KeyboardInput>();
        app.add_systems(Update, lobby_input);
        let tap = |app: &mut App, key: KeyCode| {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
        };
        tap(&mut app, KeyCode::KeyW);
        assert!(
            !app.world().resource::<LobbyState>().watching(),
            "that key says Z on this keyboard"
        );
        tap(&mut app, KeyCode::KeyZ);
        assert!(
            app.world().resource::<LobbyState>().watching(),
            "the cap that says W"
        );
    }
}
