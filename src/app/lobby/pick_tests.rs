//! The host picking the map at the start, driven as players drive it: a
//! host and its joiners, each a headless app running the lobby's own
//! systems over real loopback sockets in simulated time, the host's keys
//! pressed into its input system.

use super::*;
use crate::app::i18n::EN;
use crate::app::match_setup::{Card, CustomBeaches, MapChoice, MatchMenu, Slot, map_slots};
use crate::transport::Air;
use std::time::Duration;

/// One simulated frame.
const FRAME: f32 = 1.0 / 30.0;

/// A headless lobby with its keys: the host's tick, the joiner's, and the
/// input that reads the keyboard.
struct Peer {
    app: App,
}

impl Peer {
    fn new(state: LobbyState, name: &str, config: MatchConfig, shelf: CustomBeaches) -> Peer {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_state::<VersusPhase>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::gamepad::PadFace>();
        app.init_resource::<crate::app::gamepad::LastDevice>();
        app.init_resource::<Online>();
        app.init_resource::<crate::app::tournament::Tournament>();
        app.init_resource::<crate::app::bot_seats::BotSeats>();
        app.init_resource::<MatchMenu>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<crate::app::cursor::LiveSeatKeys>();
        app.add_message::<bevy::input::keyboard::KeyboardInput>();
        app.insert_resource(shelf);
        // No doorway for bots: that is a socket of its own on a real port.
        app.insert_resource(MatchConfig {
            bots_welcome: false,
            ..config
        });
        let mut settings = GameSettings::default();
        settings.names[0] = name.to_string();
        app.insert_resource(settings);
        app.insert_resource(Time::<()>::default());
        app.insert_resource(state);
        app.add_systems(Update, (host_tick, join_tick, lobby_input).chain());
        Peer { app }
    }

    /// A beach on the air, beaconing to nobody.
    fn host(config: MatchConfig, shelf: CustomBeaches) -> Peer {
        let state = LobbyState {
            standing: Standing::hosting(
                Announcer::on(0x9A9A, Air::loopback(&[])).expect("announcer"),
                UdpTransport::host(0).expect("game socket"),
            ),
            game_name: "Pick".to_string(),
            ..LobbyState::default()
        };
        Peer::new(state, "Host", config, shelf)
    }

    /// Somebody at `host`'s beach, greeting it.
    fn joiner(name: &str, host: &Peer) -> Peer {
        let mut peer = Peer::new(
            LobbyState::default(),
            name,
            MatchConfig::default(),
            CustomBeaches::default(),
        );
        let settings = peer.app.world().resource::<GameSettings>().clone();
        dial_at(&mut peer.state_mut(), &settings, &EN, host.here());
        peer
    }

    fn step(&mut self) {
        self.app
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(FRAME));
        self.app.update();
    }

    /// `key` down for one of this peer's frames.
    fn press(&mut self, key: KeyCode) {
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
    }

    fn release(&mut self) {
        let mut keys = self.app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release_all();
        keys.clear();
    }

    fn state(&self) -> &LobbyState {
        self.app.world().resource::<LobbyState>()
    }

    fn state_mut(&mut self) -> Mut<'_, LobbyState> {
        self.app.world_mut().resource_mut::<LobbyState>()
    }

    fn config(&self) -> &MatchConfig {
        self.app.world().resource::<MatchConfig>()
    }

    fn menu(&self) -> &MatchMenu {
        self.app.world().resource::<MatchMenu>()
    }

    fn picking(&self) -> Option<&Picking> {
        self.state()
            .hosted()
            .and_then(|hosted| hosted.picking.as_ref())
    }

    fn session(&self) -> Option<&crate::app::net::OnlineSession> {
        self.app.world().resource::<Online>().0.as_ref()
    }

    fn here(&self) -> SocketAddr {
        let port = self
            .state()
            .hosted()
            .and_then(|hosted| hosted.transport.local_addr().ok())
            .expect("hosting")
            .port();
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn heard(&self, line: &str) -> bool {
        self.state().chat.iter().any(|said| said.line == line)
    }
}

/// Every peer a frame, the host first, `frames` times.
fn run(peers: &mut [&mut Peer], frames: usize) {
    for _ in 0..frames {
        for peer in peers.iter_mut() {
            peer.step();
        }
    }
}

fn frames(seconds: f32) -> usize {
    (seconds / FRAME).ceil() as usize
}

/// The host presses `key` for one frame, everyone running it.
fn host_taps(host: &mut Peer, others: &mut [&mut Peer], key: KeyCode) {
    host.press(key);
    host.step();
    host.release();
    for peer in others.iter_mut() {
        peer.step();
    }
}

/// A host and one joiner who has sat down.
fn a_table_of_two(config: MatchConfig, shelf: CustomBeaches) -> (Peer, Peer) {
    let mut host = Peer::host(config, shelf);
    let mut joe = Peer::joiner("Joe", &host);
    run(&mut [&mut host, &mut joe], frames(1.5));
    assert_eq!(host.state().hosted().map(Hosted::players_aboard), Some(1));
    (host, joe)
}

/// Enter opens the map page over the lobby for the table as it sits,
/// greyed for it, and the cursor on the host's last pick; the joiner's
/// line says the host is picking; Esc goes back with the setup as it was.
#[test]
fn enter_opens_the_map_page_and_esc_goes_back() {
    let config = MatchConfig {
        map: MapChoice::TheMoat,
        seats: 5,
        ..MatchConfig::default()
    };
    let (mut host, mut joe) = a_table_of_two(config, CustomBeaches::default());
    host_taps(&mut host, &mut [&mut joe], KeyCode::Enter);
    let picking = host.picking().expect("the map page is up");
    assert_eq!((picking.people, picking.bots.len()), (2, 0));
    assert!(host.menu().host_page_up());
    assert_eq!(host.config().seats, 2, "the cards grey by the table");
    let slots = map_slots(&CustomBeaches::default());
    assert_eq!(
        slots[host.menu().slot].1,
        Slot::Card(Card::Map(MapChoice::TheMoat)),
        "on the host's last pick"
    );
    run(&mut [&mut host, &mut joe], frames(0.5));
    assert!(
        host.state().feedback.is_empty(),
        "the page's heading names the table, once"
    );
    // A pad's face is the page's: A picks as Enter does, B goes back as
    // Esc does, and the keyboard's prompt names both.
    use crate::app::gamepad::scheme::{Face, legend};
    let face = crate::app::gamepad::face::lobby_face(
        host.state(),
        &crate::app::bot_seats::BotSeats::default(),
        &EN,
        true,
    );
    assert_eq!(face, Face::HostMap { pick: true });
    let pad = legend(&EN, crate::app::gamepad::PadFamily::Xbox, &face);
    assert!(pad.contains(EN.pad.back), "{pad}");
    assert!(EN.lobby_pick_prompt.contains("Enter") && EN.lobby_pick_prompt.contains("Esc"));
    assert_eq!(joe.state().feedback, EN.lobby_host_picking);

    // The Enter that opened it did not also pick, and walking picks as
    // step 2 does; Esc puts it all back.
    host_taps(&mut host, &mut [&mut joe], KeyCode::KeyD);
    assert_ne!(host.config().map, MapChoice::TheMoat);
    host_taps(&mut host, &mut [&mut joe], KeyCode::Escape);
    assert!(host.picking().is_none(), "back to the lobby card");
    assert!(host.session().is_none(), "nothing started");
    assert!(!host.menu().online);
    assert_eq!(
        (host.config().map, host.config().seats),
        (MapChoice::TheMoat, 5),
        "the setup as it was"
    );
    assert_eq!(host.state().hosted().map(Hosted::players_aboard), Some(1));
    run(&mut [&mut host, &mut joe], frames(0.5));
    assert_eq!(joe.state().feedback, EN.lobby_aboard, "nobody dropped");
}

/// Leaving the lobby with the map page up closes it as Esc does: the
/// setup the page greyed and walked is put back as it was.
#[test]
fn leaving_the_lobby_under_the_page_puts_the_setup_back() {
    use bevy::ecs::system::RunSystemOnce;
    let config = MatchConfig {
        map: MapChoice::TheMoat,
        seats: 5,
        ..MatchConfig::default()
    };
    let (mut host, mut joe) = a_table_of_two(config, CustomBeaches::default());
    host_taps(&mut host, &mut [&mut joe], KeyCode::Enter);
    host_taps(&mut host, &mut [&mut joe], KeyCode::KeyD);
    assert!(host.picking().is_some());
    assert_ne!(
        (host.config().map, host.config().seats),
        (MapChoice::TheMoat, 5)
    );
    host.app
        .world_mut()
        .run_system_once(exit_lobby)
        .expect("left");
    assert!(host.picking().is_none());
    assert!(!host.menu().online, "the setup is the menu's again");
    assert_eq!(
        (host.config().map, host.config().seats),
        (MapChoice::TheMoat, 5),
        "the setup as it was"
    );
}

/// A card picked on the page starts the match on it: the host's session
/// and the joiner's are dealt the map picked, on the seed its picture was
/// drawn with, and the setup keeps the pick on its own seat count.
#[test]
fn the_pick_reaches_the_joiner() {
    let (mut host, mut joe) = a_table_of_two(MatchConfig::default(), CustomBeaches::default());
    host_taps(&mut host, &mut [&mut joe], KeyCode::Enter);
    let seed = host.menu().seed;
    // Classic, then the first handmade map along the page.
    host_taps(&mut host, &mut [&mut joe], KeyCode::KeyD);
    assert_eq!(host.config().map, MapChoice::TheGates);
    host_taps(&mut host, &mut [&mut joe], KeyCode::Enter);
    run(&mut [&mut host, &mut joe], frames(1.0));
    let terms = host.session().expect("the host walked in").terms;
    use crate::app::match_setup::TermsDials;
    assert_eq!((terms.map(), terms.seed), (MapChoice::TheGates, seed));
    let theirs = joe.session().expect("the joiner walked in").terms;
    assert_eq!((theirs.map(), theirs.seed), (MapChoice::TheGates, seed));
    assert!(!host.menu().online, "the page went with the launch");
    assert_eq!(
        (host.config().map, host.config().seats),
        (MapChoice::TheGates, 2),
        "the pick is kept for next time"
    );
}

/// The page greys by the table frozen at the start: five seats are not
/// offered the Duel, a press on it does not pick it, and Enter on it
/// starts nothing. A shelf beach too big to travel plays on the couch and
/// not here, and its line says why.
#[test]
fn the_page_greys_by_the_frozen_table() {
    use crate::app::match_setup::{Beach, card_open};
    let config = MatchConfig {
        lobby_bots: 3,
        ..MatchConfig::default()
    };
    let (mut host, mut joe) = a_table_of_two(config, CustomBeaches(vec![soup()]));
    host_taps(&mut host, &mut [&mut joe], KeyCode::Enter);
    assert!(host.picking().is_some());
    assert_eq!(host.config().seats, 5, "greyed for the frozen table");
    let shelf = host.app.world().resource::<CustomBeaches>();
    let duel = Card::Map(MapChoice::Duel);
    assert!(!card_open(duel, 5, shelf, true));
    assert!(card_open(Card::Random, 5, shelf, true));
    assert!(card_open(Card::Map(MapChoice::Surprise), 5, shelf, true));
    // The soup seats five on the couch, and cannot travel.
    let big = Card::Shelf(0);
    assert!(Beach::too_big_to_send(&shelf.0[0]));
    assert!(card_open(big, 5, shelf, false) && !card_open(big, 5, shelf, true));
    let note = crate::app::match_setup::card_note(&EN, big, host.config(), shelf, true);
    assert_eq!(note, fill(EN.host_map_too_big, &[("m", "Soup")]));
    let note = crate::app::match_setup::card_note(&EN, duel, host.config(), shelf, true);
    assert!(note.contains("this table has 5"), "{note}");
    // Walk onto the Duel: not picked, and Enter there starts nothing.
    let slots = map_slots(shelf);
    let at = slots
        .iter()
        .position(|(_, slot)| *slot == Slot::Card(duel))
        .expect("the Duel");
    let before = host.config().map;
    host.app.world_mut().resource_mut::<MatchMenu>().slot = at - 1;
    host_taps(&mut host, &mut [&mut joe], KeyCode::KeyD);
    assert_eq!(host.menu().slot, at);
    assert_eq!(host.config().map, before, "a greyed card is not picked");
    host_taps(&mut host, &mut [&mut joe], KeyCode::Enter);
    run(&mut [&mut host, &mut joe], frames(0.3));
    assert!(host.picking().is_some() && host.session().is_none());
}

/// A shelf beach with a crab on every free tile and six castles: too big
/// for an invitation.
fn soup() -> crate::app::match_setup::Beach {
    use crate::sim::{Board, CrabKind, Direction, Handedness, LevelKind, TileKind};
    let mut board = Board::new(20, 13, 0xBEEF);
    for owner in 0..MAX_PLAYERS as u8 {
        board.set_tile(owner, 0, TileKind::Castle(owner));
    }
    for x in 0..20u8 {
        for y in 0..13u8 {
            if board.tile_at(x, y) == TileKind::Empty {
                board.spawn_crab(
                    x,
                    y,
                    Direction::Right,
                    Handedness::Left,
                    CrabKind::Sparkling,
                );
            }
        }
    }
    crate::app::match_setup::Beach::new(
        crate::sim::Level::from_board("Soup", 3, board).with_kind(LevelKind::Arena),
    )
}

/// The lobby goes on under the page: the joiner keeps its chair through
/// seconds past the lobby's patience, its chat reaches the host, a player
/// sitting down meanwhile watches the match rather than changing the
/// table the map was picked for, and one frozen at the table leaving
/// calls the start off, with the room told why.
#[test]
fn the_lobby_keeps_ticking_under_the_page() {
    let (mut host, mut joe) = a_table_of_two(MatchConfig::default(), CustomBeaches::default());
    host_taps(&mut host, &mut [&mut joe], KeyCode::Enter);
    run(&mut [&mut host, &mut joe], frames(PEER_TTL + 2.0));
    assert!(host.picking().is_some());
    assert_eq!(host.state().hosted().map(Hosted::players_aboard), Some(1));
    {
        let state = joe.state_mut();
        if let Some(transport) = state.transport() {
            transport.send(NetMsg::chat("Joe", "take the moat"));
        }
    }
    run(&mut [&mut host, &mut joe], frames(0.5));
    assert!(host.heard("take the moat"), "chat goes on");

    // Kim sits down while the host picks: the page stays up, and Kim
    // watches this match.
    let mut kim = Peer::joiner("Kim", &host);
    run(&mut [&mut host, &mut joe, &mut kim], frames(1.5));
    assert_eq!(host.state().hosted().map(Hosted::players_aboard), Some(2));
    assert!(host.picking().is_some(), "a newcomer does not call it off");
    assert!(
        kim.state()
            .joined()
            .is_some_and(|joined| joined.berth == crate::app::lobby::Berth::NoChair)
    );
    host_taps(&mut host, &mut [&mut joe, &mut kim], KeyCode::Enter);
    run(&mut [&mut host, &mut joe, &mut kim], frames(1.0));
    let session = host.session().expect("the match started");
    assert_eq!(session.seats, 2, "the table the map was picked for");
    assert!(joe.session().is_some_and(|s| s.session.seat() == Some(1)));
    assert!(kim.session().is_some_and(|s| s.session.seat().is_none()));
}

/// Somebody frozen at the table leaving while the host picks calls the
/// start off: back to the lobby card, the setup as it was, and the room
/// told why.
#[test]
fn a_player_leaving_calls_the_start_off() {
    let (mut host, mut joe) = a_table_of_two(MatchConfig::default(), CustomBeaches::default());
    host_taps(&mut host, &mut [&mut joe], KeyCode::Enter);
    assert!(host.picking().is_some());
    if let Some(transport) = joe.state().transport() {
        transport.send(NetMsg::Leaving);
    }
    drop(joe);
    run(&mut [&mut host], frames(1.0));
    assert!(host.picking().is_none(), "called off");
    assert!(host.session().is_none());
    assert!(!host.menu().online);
    assert!(host.heard(EN.lobby_pick_called_off));
}

/// Frozen were the host, Joe and Ann. Kim sits down while the host picks
/// and is told there is no chair for her this match; then Joe leaves. The
/// table is three again, but not the three frozen: the start is called
/// off rather than launched with Kim in Joe's chair on a map picked
/// without her.
#[test]
fn a_frozen_player_leaving_is_not_hidden_by_a_newcomer() {
    let mut host = Peer::host(MatchConfig::default(), CustomBeaches::default());
    let mut joe = Peer::joiner("Joe", &host);
    let mut ann = Peer::joiner("Ann", &host);
    run(&mut [&mut host, &mut joe, &mut ann], frames(1.5));
    assert_eq!(host.state().hosted().map(Hosted::players_aboard), Some(2));
    host_taps(&mut host, &mut [&mut joe, &mut ann], KeyCode::Enter);
    assert!(host.picking().is_some());
    let mut kim = Peer::joiner("Kim", &host);
    run(&mut [&mut host, &mut joe, &mut ann, &mut kim], frames(1.5));
    assert!(host.picking().is_some(), "a newcomer does not call it off");
    assert!(
        kim.state()
            .joined()
            .is_some_and(|joined| joined.berth == crate::app::lobby::Berth::NoChair)
    );
    if let Some(transport) = joe.state().transport() {
        transport.send(NetMsg::Leaving);
    }
    drop(joe);
    run(&mut [&mut host, &mut ann, &mut kim], frames(1.0));
    assert!(host.picking().is_none(), "called off");
    assert!(host.session().is_none());
    assert!(host.heard(EN.lobby_pick_called_off));
}

/// The host picks partners on the lobby card in pairs, the AI included:
/// P, then the seat whose partner changes, as often as it is pressed. The
/// joiner's card shows the teams the host picked, and the launch deals
/// them: the joiner sat where the pick put it, the AI where the order
/// says, each peer's lockstep over the players' seats wherever they are.
#[test]
fn the_host_picks_partners_and_the_launch_deals_them() {
    use crate::app::teams::TeamMode;
    let config = MatchConfig {
        lobby_bots: 2,
        ..MatchConfig::default()
    };
    let (mut host, mut joe) = a_table_of_two(config, CustomBeaches::default());
    host.app
        .world_mut()
        .resource_mut::<GameSettings>()
        .team_mode = TeamMode::Pairs;
    run(&mut [&mut host, &mut joe], frames(0.5));
    let shown = |peer: &Peer| -> Vec<(u8, String)> {
        seats_shown(peer.state(), &EN)
            .into_iter()
            .map(|seat| (seat.seat, seat.name))
            .collect()
    };
    let ai = |seat: u8| format!("{}{}", crate::app::seat_label(&EN, seat), EN.tag_ai);
    assert_eq!(
        shown(&host),
        [
            (0, "Host".to_string()),
            (1, "Joe".to_string()),
            (2, ai(2)),
            (3, ai(3))
        ],
        "the host and Joe against the AI, team by team"
    );
    // A pad's face offers the pick, and the keyboard's line names P.
    let face = crate::app::gamepad::face::lobby_face(
        host.state(),
        &crate::app::bot_seats::BotSeats::default(),
        &EN,
        false,
    );
    assert_eq!(face, crate::app::gamepad::Face::Hosting { partners: true });
    host_taps(&mut host, &mut [&mut joe], KeyCode::KeyP);
    assert!(
        host.state()
            .hosted()
            .is_some_and(|hosted| hosted.partnering)
    );
    // Seat 1 (the host's) picks its next partner: the AI in P3 comes to
    // P2, the host's team, and Joe goes to P3.
    host_taps(&mut host, &mut [&mut joe], KeyCode::Digit1);
    host_taps(&mut host, &mut [&mut joe], KeyCode::Escape);
    assert!(
        host.state()
            .hosted()
            .is_some_and(|hosted| !hosted.partnering)
    );
    run(&mut [&mut host, &mut joe], frames(1.5));
    let picked = [
        (0, "Host".to_string()),
        (1, ai(1)),
        (2, "Joe".to_string()),
        (3, ai(3)),
    ];
    assert_eq!(shown(&host), picked);
    assert_eq!(shown(&joe), picked, "the joiner's card shows the teams");
    // The start: the map page, a card, and the match dealt as picked.
    host_taps(&mut host, &mut [&mut joe], KeyCode::Enter);
    host_taps(&mut host, &mut [&mut joe], KeyCode::Enter);
    run(&mut [&mut host, &mut joe], frames(1.0));
    let session = host.session().expect("the host walked in");
    assert_eq!(&session.terms.order[..4], [0, 2, 1, 3]);
    assert_eq!(session.terms.ai_seats(4), [1, 3]);
    assert_eq!(session.names[2], "Joe");
    let theirs = joe.session().expect("Joe walked in");
    assert_eq!(
        theirs.session.seat(),
        Some(2),
        "Joe sits where the pick put him"
    );
    assert_eq!(theirs.terms, session.terms, "one table on every peer");
    let controllers = crate::app::session::online_controllers_for_tests(theirs);
    assert_eq!(
        controllers.0[..4],
        [
            crate::app::SeatController::Remote,
            crate::app::SeatController::Ai(crate::sim::BotLevel::Normal),
            crate::app::SeatController::Local,
            crate::app::SeatController::Ai(crate::sim::BotLevel::Normal),
        ]
    );
}

/// Partners picked in pairs, then the host turns scoring to free-for-all:
/// the picks are set aside, so the card and the deal seat everybody in
/// place order with the AI on top, and kept, so back in pairs the teams
/// picked hold again.
#[test]
fn partner_picks_are_set_aside_under_free_for_all() {
    use crate::app::teams::TeamMode;
    let config = MatchConfig {
        lobby_bots: 2,
        ..MatchConfig::default()
    };
    let (mut host, mut joe) = a_table_of_two(config, CustomBeaches::default());
    let scoring = |host: &mut Peer, mode: TeamMode| {
        host.app
            .world_mut()
            .resource_mut::<GameSettings>()
            .team_mode = mode;
    };
    scoring(&mut host, TeamMode::Pairs);
    run(&mut [&mut host, &mut joe], frames(0.5));
    host_taps(&mut host, &mut [&mut joe], KeyCode::KeyP);
    host_taps(&mut host, &mut [&mut joe], KeyCode::Digit1);
    host_taps(&mut host, &mut [&mut joe], KeyCode::Escape);
    run(&mut [&mut host, &mut joe], frames(0.5));
    assert_eq!(host.state().seated.order[..4], [0, 2, 1, 3], "picked");
    scoring(&mut host, TeamMode::Solo);
    run(&mut [&mut host, &mut joe], frames(0.5));
    assert_eq!(host.state().seated.order[..4], [0, 1, 2, 3], "set aside");
    scoring(&mut host, TeamMode::Pairs);
    run(&mut [&mut host, &mut joe], frames(0.5));
    assert_eq!(host.state().seated.order[..4], [0, 2, 1, 3], "kept");
    scoring(&mut host, TeamMode::Solo);
    run(&mut [&mut host, &mut joe], frames(0.5));
    host_taps(&mut host, &mut [&mut joe], KeyCode::Enter);
    host_taps(&mut host, &mut [&mut joe], KeyCode::Enter);
    run(&mut [&mut host, &mut joe], frames(1.0));
    let session = host.session().expect("the host walked in");
    assert_eq!(&session.terms.order[..4], [0, 1, 2, 3]);
    assert_eq!(session.terms.ai_seats(4), [2, 3], "the AI on top");
    let theirs = joe.session().expect("Joe walked in");
    assert_eq!(theirs.session.seat(), Some(1));
}

/// While the host picks partners the keys are the pick's: K asks nobody
/// to leave and T opens no chat. A pad's pick list runs in the card's
/// order, team by team, and its cursor follows the person it picks for:
/// a pick that moves them to another seat takes the cursor along, so the
/// next A picks for them again, as step 3's cursor does on the couch.
#[test]
fn partnering_keeps_the_keys_and_the_pad_follows_the_picker() {
    use crate::app::gamepad::{PadPick, scheme::PickList};
    use crate::app::teams::TeamMode;
    let config = MatchConfig {
        lobby_bots: 4,
        ..MatchConfig::default()
    };
    let (mut host, mut joe) = a_table_of_two(config, CustomBeaches::default());
    host.app.init_resource::<PadPick>();
    host.app
        .world_mut()
        .resource_mut::<GameSettings>()
        .team_mode = TeamMode::Trios;
    run(&mut [&mut host, &mut joe], frames(0.5));
    host_taps(&mut host, &mut [&mut joe], KeyCode::KeyP);
    host_taps(&mut host, &mut [&mut joe], KeyCode::KeyK);
    host_taps(&mut host, &mut [&mut joe], KeyCode::KeyT);
    let hosted = host.state().hosted().expect("hosting");
    assert!(hosted.partnering, "still picking");
    assert!(hosted.kicking.is_none(), "K asks nobody to leave");
    assert!(host.state().typing.is_none(), "T opens no chat");
    let card = |host: &Peer| -> Vec<u8> {
        seats_shown(host.state(), &EN)
            .into_iter()
            .map(|seat| seat.seat + 1)
            .collect()
    };
    let rows = partner_rows(host.state(), &EN);
    let numbers: Vec<u8> = rows.iter().map(|(number, _)| *number).collect();
    assert_eq!(numbers, card(&host), "the pad's list is the card's");
    assert_eq!(numbers, [1, 3, 5, 2, 4, 6]);
    // Joe picks again and again: wherever a pick seats him, the pad's
    // cursor stands on his row.
    let mut moved = false;
    for _ in 0..6 {
        let at = partner_rows(host.state(), &EN)
            .into_iter()
            .find(|(_, name)| name == "Joe")
            .map(|(number, _)| number)
            .expect("Joe at the table");
        let digit = crate::app::menu_ui::NUMBER_KEYS[usize::from(at) - 1];
        host_taps(&mut host, &mut [&mut joe], digit);
        let rows = partner_rows(host.state(), &EN);
        let picks = crate::app::gamepad::scheme::Picks {
            list: PickList::Partners,
            rows: rows.clone(),
        };
        let pick = host.app.world().resource::<PadPick>();
        assert_eq!(rows[pick.row(&picks)].1, "Joe", "the cursor on Joe");
        moved |= rows
            .iter()
            .all(|(number, name)| name != "Joe" || *number != at);
    }
    assert!(moved, "some pick moved Joe to another seat");
}
