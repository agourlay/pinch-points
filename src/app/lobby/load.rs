//! The lobby under a crowd: many beaches on the air and many people
//! browsing, joining and talking at once, each a headless app running only
//! the lobby's own systems over real loopback sockets, in simulated time.
//!
//! Every beacon here goes to the listeners' own ports on loopback
//! ([`Air::loopback`], [`Discovery::bind_on`]), so a run hears nothing of
//! the real lobby ladder or of any other test, and puts nothing on it. A
//! port the system picks is never a lobby port ([`crate::transport::bind_udp`]):
//! a listener that drew one once heard the beacon tests running beside it.
//!
//! `a_small_hall_holds_together` runs with the suite. The big hall (12
//! beaches, 50 people) and the stress hall (64 and 200) are `#[ignore]`d:
//! `cargo test --locked big_hall -- --ignored --nocapture`, and `HALL=63x200`
//! for the stress run.

use super::*;
use crate::transport::Air;
use std::time::{Duration, Instant};

/// One simulated frame.
const FRAME: f32 = 1.0 / 30.0;

/// A headless lobby: the lobby's systems on their own world.
struct Lobby {
    app: App,
    /// How long each update took, in microseconds.
    took: Vec<u32>,
}

impl Lobby {
    fn new(state: LobbyState, name: &str) -> Lobby {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_state::<VersusPhase>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::gamepad::LastDevice>();
        app.init_resource::<Online>();
        app.init_resource::<crate::app::tournament::Tournament>();
        app.init_resource::<crate::app::bot_seats::BotSeats>();
        app.init_resource::<crate::app::match_setup::CustomBeaches>();
        app.init_resource::<crate::app::match_setup::MatchMenu>();
        // No doorway for bots: that is a socket of its own on a real port.
        app.insert_resource(MatchConfig {
            bots_welcome: false,
            ..MatchConfig::default()
        });
        let mut settings = GameSettings::default();
        settings.names[0] = name.to_string();
        app.insert_resource(settings);
        app.insert_resource(Time::<()>::default());
        app.insert_resource(state);
        app.add_systems(Update, (discover, host_tick, join_tick).chain());
        Lobby {
            app,
            took: Vec::new(),
        }
    }

    /// A beach on the air, called `name`, beaconing at `ports`.
    fn host(name: &str, id: u64, ports: &[u16]) -> Lobby {
        let state = LobbyState {
            standing: Standing::hosting(
                Announcer::on(id, Air::loopback(ports)).expect("announcer"),
                UdpTransport::host(0).expect("game socket"),
            ),
            game_name: name.to_string(),
            ..LobbyState::default()
        };
        Lobby::new(state, &format!("{name} host"))
    }

    /// Somebody browsing, listening on a port of their own.
    fn listener(name: &str) -> Lobby {
        let state = LobbyState {
            discovery: Some(Discovery::bind_on(&[0]).expect("listen")),
            ..LobbyState::default()
        };
        Lobby::new(state, name)
    }

    fn port(&self) -> u16 {
        self.state()
            .discovery
            .as_ref()
            .and_then(Discovery::port)
            .expect("listening")
    }

    fn step(&mut self) {
        self.app
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(FRAME));
        let started = Instant::now();
        self.app.update();
        let us = started.elapsed().as_micros();
        self.took.push(u32::try_from(us).unwrap_or(u32::MAX));
    }

    fn state(&self) -> &LobbyState {
        self.app.world().resource::<LobbyState>()
    }

    fn state_mut(&mut self) -> Mut<'_, LobbyState> {
        self.app.world_mut().resource_mut::<LobbyState>()
    }

    /// The beaches this lobby lists, by address, in the order it lists them.
    fn listed(&self) -> Vec<SocketAddr> {
        self.state().hosts.iter().map(|host| host.addr).collect()
    }

    /// Where its own game socket is, as a listener on this machine dials it.
    fn here(&self) -> SocketAddr {
        let port = self
            .state()
            .hosted()
            .and_then(|hosted| hosted.transport.local_addr().ok())
            .expect("hosting")
            .port();
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn dial(&mut self, addr: SocketAddr) {
        let settings = self.app.world().resource::<GameSettings>().clone();
        dial_at(
            &mut self.state_mut(),
            &settings,
            &crate::app::i18n::EN,
            addr,
        );
    }

    /// Say something to the beach, as T and Enter do.
    fn say(&mut self, line: &str) {
        let name = self.app.world().resource::<GameSettings>().names[0].clone();
        let mut state = self.state_mut();
        if let Some(transport) = state.transport() {
            transport.send(NetMsg::chat(&name, line));
        }
        state.say(&name, line);
    }

    /// How many times `line` is in this lobby's feed.
    fn heard(&self, line: &str) -> usize {
        self.state()
            .chat
            .iter()
            .filter(|said| said.line == line)
            .count()
    }
}

/// Run every lobby `frames` frames, each once a frame in turn, the
/// beaches first.
fn run(hosts: &mut [Lobby], listeners: &mut [Lobby], frames: usize) {
    for _ in 0..frames {
        for lobby in hosts.iter_mut().chain(listeners.iter_mut()) {
            lobby.step();
        }
    }
}

fn frames(seconds: f32) -> usize {
    (seconds / FRAME).ceil() as usize
}

/// What a hall run measured, for the ignored runs to print.
struct Measured {
    p50_us: u32,
    p99_us: u32,
    max_us: u32,
}

/// The hall, end to end: `beaches` hosts and `people` listeners. What
/// every update took is measured, not held to anything: wall-clock time
/// on a machine running the rest of the suite beside it says more about
/// the machine than about the lobby.
fn hall(beaches: usize, people: usize) -> Measured {
    assert!(beaches >= 3 && people >= 10, "the script needs this many");
    // A full list takes no newcomer (`the_row_to_give_up`), and AAA has to
    // be listed.
    assert!(beaches < MAX_BEACHES, "room on the list for one more");
    let mut listeners: Vec<Lobby> = (0..people)
        .map(|i| Lobby::listener(&format!("P{i:03}")))
        .collect();
    let ports: Vec<u16> = listeners.iter().map(Lobby::port).collect();
    let mut hosts: Vec<Lobby> = (0..beaches)
        .map(|b| Lobby::host(&format!("Beach {b:02}"), 0x1000 + b as u64, &ports))
        .collect();

    // Every listener lists every beach in the first frame, in one order: a
    // beach beacons on its first frame, and on loopback it lands in that
    // frame's read.
    run(&mut hosts, &mut listeners, 1);
    for listener in &listeners {
        assert_eq!(
            listener.listed().len(),
            beaches,
            "listed in the first frame"
        );
    }
    let order = listeners[0].listed();
    for listener in &listeners {
        assert_eq!(listener.listed(), order, "one order on every screen");
    }

    // Nothing flickers while its beach lives: four seconds more, every
    // listener holding every row every frame.
    for _ in 0..frames(4.0) {
        run(&mut hosts, &mut listeners, 1);
        for listener in &listeners {
            assert_eq!(listener.listed(), order, "a row came and went");
        }
    }

    // The cursor stays on its beach when one sorting ahead of it appears:
    // pointed at by its number, as the key does it.
    let meant = order[2];
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::Digit3);
    walk_the_list(&keys, &mut listeners[0].state_mut());
    assert_eq!(
        listeners[0].state().selected,
        Some(meant),
        "3 is the third row"
    );
    hosts.push(Lobby::host("AAA", 0xAAA, &ports));
    for _ in 0..frames(2.0) {
        run(&mut hosts, &mut listeners, 1);
    }
    let first = listeners[0].listed();
    assert_eq!(first.len(), beaches + 1, "AAA is listed");
    assert_eq!(listeners[0].state().hosts[0].name, "AAA", "and sorts first");
    assert_eq!(
        listeners[0].state().selected,
        Some(meant),
        "the cursor stayed on its beach"
    );
    assert_eq!(
        listeners[0].state().selected_index(),
        Some(3),
        "one row down"
    );

    // A goodbye takes a beach off every list within a few frames.
    let gone = hosts.pop().expect("AAA");
    say_goodbye(gone.state());
    drop(gone);
    for _ in 0..3 {
        run(&mut hosts, &mut listeners, 1);
    }
    for listener in &listeners {
        assert_eq!(listener.listed(), order, "AAA is gone at once");
    }

    // A join storm: seven people press join on beach 0 in the same frame.
    // Five are seated, and two are told there is no chair. Everyone else
    // joins the other beaches round the table, as people do.
    let beach_0 = hosts[0].here();
    for listener in listeners.iter_mut().take(7) {
        listener.dial(beach_0);
    }
    let others: Vec<SocketAddr> = hosts[1..beaches - 1].iter().map(Lobby::here).collect();
    let mut seated_at = vec![0usize; people];
    for (i, listener) in listeners.iter_mut().enumerate().skip(7) {
        let at = (i - 7) % others.len();
        seated_at[i] = 1 + at;
        listener.dial(others[at]);
    }
    for _ in 0..frames(2.5) {
        run(&mut hosts, &mut listeners, 1);
    }
    let host_0 = hosts[0].state().hosted().expect("hosting");
    assert_eq!(host_0.peers.len(), 7, "all seven are on the socket");
    assert_eq!(
        host_0.players_aboard(),
        MAX_PLAYERS - 1,
        "five at the table"
    );
    let storm: Vec<&LobbyState> = listeners.iter().take(7).map(Lobby::state).collect();
    let chairless = storm
        .iter()
        .filter(|state| state.joined().is_some_and(|joined| joined.no_chair))
        .count();
    assert_eq!(chairless, 2, "two told there is no chair");
    for state in &storm {
        let no_chair = state.joined().is_some_and(|joined| joined.no_chair);
        let line = match no_chair {
            true => crate::app::i18n::EN.lobby_no_chair,
            false => crate::app::i18n::EN.lobby_aboard,
        };
        assert_eq!(state.feedback, line, "the line says what launch will do");
    }

    // Chat reaches everybody at its beach once, and nobody anywhere else.
    listeners[0].say("ready on 0?");
    listeners[7].say("ready on 1?");
    for _ in 0..frames(1.0) {
        run(&mut hosts, &mut listeners, 1);
    }
    let beach_1 = seated_at[7];
    for (b, host) in hosts.iter().enumerate() {
        assert_eq!(host.heard("ready on 0?"), usize::from(b == 0), "host {b}");
        assert_eq!(
            host.heard("ready on 1?"),
            usize::from(b == beach_1),
            "host {b}"
        );
    }
    for (i, listener) in listeners.iter().enumerate() {
        let on_0 = i < 7;
        let on_1 = i >= 7 && seated_at[i] == beach_1;
        assert_eq!(listener.heard("ready on 0?"), usize::from(on_0), "P{i}");
        assert_eq!(listener.heard("ready on 1?"), usize::from(on_1), "P{i}");
    }

    // A beach whose socket is full answers the next dialler "full", not
    // the silence that read as a wrong address. Filled by bare sockets,
    // greeting once a second as a lobby does.
    let mut packed = Lobby::host("Packed", FULL_ID, &ports);
    let packed_at = packed.here();
    let crowd: Vec<UdpTransport> = (0..crate::transport::MAX_PEERS)
        .map(|_| UdpTransport::join(packed_at).expect("join"))
        .collect();
    let greet = |crowd: &[UdpTransport]| {
        for (i, peer) in crowd.iter().enumerate() {
            peer.send(NetMsg::watch(&format!("W{i}")));
        }
    };
    greet(&crowd);
    for _ in 0..frames(0.5) {
        packed.step();
    }
    assert_eq!(
        packed
            .state()
            .hosted()
            .map(|hosted| hosted.transport.peer_count()),
        Some(crate::transport::MAX_PEERS)
    );
    // It keeps calling while a place may yet free, and is let go, told the
    // beach is full, once the patience is up.
    let mut late = Lobby::listener("Late");
    late.dial(packed_at);
    let mut let_go_after = None;
    for frame in 0..frames(NO_ANSWER_AFTER + 1.5) {
        if frame % 30 == 0 {
            greet(&crowd);
        }
        packed.step();
        late.step();
        if let_go_after.is_none() && late.state().joined().is_none() {
            let_go_after = Some(frame as f32 * FRAME);
        }
    }
    let let_go_after = let_go_after.expect("the dialler was let go");
    assert!(
        let_go_after >= NO_ANSWER_AFTER - 1.0,
        "let go after {let_go_after} s, while a place could still free"
    );
    assert_eq!(late.state().feedback, crate::app::i18n::EN.lobby_no_room);

    // A beach that drops without a word goes once its silence outlasts
    // the list's patience, and not before.
    let silent = hosts.pop().expect("a beach");
    let silent_at = silent.here();
    drop(silent);
    let ttl = host_ttl(crate::transport::beacons_heard_one_in(ports[0]));
    let listed = |listeners: &[Lobby]| {
        listeners
            .iter()
            .filter(|l| l.listed().contains(&silent_at))
            .count()
    };
    for _ in 0..frames(ttl - 2.0) {
        run(&mut hosts, &mut listeners, 1);
    }
    assert_eq!(
        listed(&listeners),
        people,
        "kept while it may be lost beacons"
    );
    for _ in 0..frames(2.5) {
        run(&mut hosts, &mut listeners, 1);
    }
    assert_eq!(listed(&listeners), 0, "and gone once it cannot be");

    let mut took: Vec<u32> = hosts
        .iter()
        .chain(listeners.iter())
        .flat_map(|lobby| lobby.took.iter().copied())
        .collect();
    took.sort_unstable();
    let at = |q: f64| took[((took.len() as f64 * q) as usize).min(took.len() - 1)];
    Measured {
        p50_us: at(0.5),
        p99_us: at(0.99),
        max_us: *took.last().expect("updates"),
    }
}

/// The packed beach's beacon id.
const FULL_ID: u64 = 0xF011;

/// Three beaches and ten people: the whole script, small enough for the
/// suite.
#[test]
fn a_small_hall_holds_together() {
    let measured = hall(3, 10);
    println!(
        "hall 3x10: update p50 {} us p99 {} us",
        measured.p50_us, measured.p99_us
    );
}

/// Twelve beaches and fifty people, the party this is for; `HALL=63x200`
/// for the stress run, the most beaches a list holds with room for AAA. Prints what it measured.
#[test]
#[ignore = "big: run with --ignored"]
fn big_hall() {
    let (beaches, people) = std::env::var("HALL")
        .ok()
        .and_then(|hall| {
            let (b, p) = hall.split_once('x')?;
            Some((b.parse().ok()?, p.parse().ok()?))
        })
        .unwrap_or((12, 50));
    let started = Instant::now();
    let measured = hall(beaches, people);
    println!(
        "hall {beaches}x{people}: update p50 {} us p99 {} us max {} us, wall {:?}",
        measured.p50_us,
        measured.p99_us,
        measured.max_us,
        started.elapsed()
    );
}
