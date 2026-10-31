//! The host picking each next round's map in an online series: the map
//! page the lobby opens at the start ([`crate::app::lobby`]) comes up over
//! the results card when the host goes on, greyed for the table the next
//! round is dealt, the cursor on the map just played. Enter on a card
//! calls the next round on it; Esc puts the page away, back on the results
//! card with the series where it was, so Enter there opens it again.
//!
//! The next round is called as it always has been, by the host's `Start`
//! to every peer ([`crate::app::net::OnlineSession::call_next_round`]),
//! now on the picked map rather than on the next one along: the joiners
//! walk in on it exactly as before, and every peer opens it on the
//! line-up ([`crate::app::lineup`]), which names it.

use crate::app::cycle::Cycle;
use crate::app::match_setup::{
    Close, CustomBeaches, HostPage, MapChoice, MapPress, MatchConfig, MatchMenu, TermsDials,
};
use crate::app::net::OnlineSession;
use bevy::prelude::*;

/// The host's map page between rounds, while it is up.
#[derive(Resource, Default)]
pub struct NextMap(Option<HostPage>);

impl NextMap {
    /// Whether the page is up.
    pub fn is_up(&self) -> bool {
        self.0.is_some()
    }
}

/// The pause card's condition: not while the page has the keys.
pub fn no_page_up(menu: Res<MatchMenu>) -> bool {
    !menu.host_page_up()
}

/// Put the page up for the round after this one: the table told, and
/// with that the next round's table frozen as it stands
/// ([`OnlineSession::say_picking`]), the setup's seats set to it
/// ([`OnlineSession::next_round_seats`]), the map just played picked (so
/// the cursor starts on it), and a fresh seed for the pictures, which the
/// round is played on.
pub fn open_next_map(
    next: &mut NextMap,
    session: &mut OnlineSession,
    config: &mut MatchConfig,
    menu: &mut MatchMenu,
    beaches: &CustomBeaches,
) {
    let before = config.clone();
    session.say_picking(true);
    let seats = session.next_round_seats(config.lobby_bots);
    let map = session.terms.map();
    config.seats = seats;
    config.clamp_bots();
    config.pick = crate::app::match_setup::Pick::InForce;
    config.map = map;
    config.custom = beaches
        .shelf_of(&session.beach)
        .filter(|_| map == MapChoice::Custom)
        .and_then(|shelf| beaches.among_fitting(seats, shelf))
        .unwrap_or(0);
    next.0 = Some(HostPage::open(before, config, menu, beaches));
}

/// Keep the page greyed for the table the next round is dealt, every frame
/// it is up: somebody frozen at it leaving (or asking to watch) shrinks
/// it, and the heading, the greying and the line under the cursor follow
/// at once. A pick the smaller table cannot play is greyed with the rest
/// and its line says why, so the host picks again rather than having the
/// call quietly swap it; the round is dealt the seats the page shows, so
/// the board previewed on its seed is the board played. Nobody new grows
/// it: they wait for the round after ([`OnlineSession::next_round_seats`]).
fn follow_the_table(session: &OnlineSession, config: &mut MatchConfig) {
    let seats = session.next_round_seats(config.lobby_bots);
    if config.seats != seats {
        config.seats = seats;
        config.clamp_bots();
    }
}

fn close(
    next: &mut NextMap,
    close: Close,
    config: &mut MatchConfig,
    menu: &mut MatchMenu,
    beaches: &CustomBeaches,
) {
    if let Some(page) = next.0.take() {
        page.close(close, config, menu, beaches);
    }
}

/// The terms of the next round on the map the page has picked, for the
/// table it was greyed for: the Random card drawn now from the maps that
/// seat it and travel, the seed the pictures were drawn with, and the
/// host's handmade beach put on the session to travel with the `Start`.
fn picked_terms(
    session: &mut OnlineSession,
    config: &MatchConfig,
    seed: u64,
    beaches: &CustomBeaches,
) -> crate::transport::MatchTerms {
    let mut table = config.clone();
    if table.pick == crate::app::match_setup::Pick::Random {
        crate::app::match_setup::draw_online_map(
            &mut table,
            beaches,
            crate::app::clock::fresh_seed(),
        );
    }
    let seats = table.seats;
    session.beach = crate::app::match_setup::beach_bytes(&table, seats, beaches);
    // The AI is the host's dial again, which the call deals from the
    // table as it then sits, not the AI of the round just played.
    crate::transport::MatchTerms {
        map: crate::app::lobby::map_for(&table, seats).index() as u8,
        seed,
        bots: table.lobby_bots,
        ..session.terms
    }
}

/// What a press on the page between rounds did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NextMapPress {
    /// Nothing the caller acts on: walked the cards, or the page's first
    /// frame.
    Nothing,
    /// Esc: the page is down, nothing called, the results card as it was.
    Back,
    /// A card picked and the next round called on it, with the series'
    /// standing as the new round deals it.
    Called(Option<crate::transport::SeriesStanding>),
}

/// The page's keys between rounds: the map page's own
/// ([`crate::app::match_setup::map_page_keys`]), Esc putting it down with
/// nothing called, Enter on a card the table can play calling the next
/// round on it. The Enter that opened the page is let go.
pub fn next_map_keys(
    keys: &ButtonInput<KeyCode>,
    next: &mut NextMap,
    session: &mut OnlineSession,
    config: &mut MatchConfig,
    menu: &mut MatchMenu,
    beaches: &CustomBeaches,
    standing: Option<crate::transport::SeriesStanding>,
) -> NextMapPress {
    let Some(page) = next.0.as_mut() else {
        return NextMapPress::Nothing;
    };
    follow_the_table(session, config);
    if page.just_opened() {
        return NextMapPress::Nothing;
    }
    match crate::app::match_setup::map_page_keys(keys, menu, config, beaches) {
        MapPress::Nothing => NextMapPress::Nothing,
        MapPress::Back => {
            close(next, Close::PutBack, config, menu, beaches);
            session.say_picking(false);
            NextMapPress::Back
        }
        MapPress::Chosen(_) => {
            let terms = picked_terms(session, config, menu.seed, beaches);
            let standing = session.call_next_round(terms, standing);
            close(next, Close::Keep, config, menu, beaches);
            NextMapPress::Called(standing)
        }
    }
}

/// Leaving the round's screen with the page up (the host walked out on
/// the pause card's way, or the table went home): the page goes, the
/// setup put back.
pub fn put_the_page_away(
    mut next: ResMut<NextMap>,
    mut config: ResMut<MatchConfig>,
    mut menu: ResMut<MatchMenu>,
    beaches: Res<CustomBeaches>,
) {
    if next.is_up() {
        close(&mut next, Close::PutBack, &mut config, &mut menu, &beaches);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{DEFAULT_DELAY, Lockstep};
    use crate::transport::{MatchTerms, NetMsg, UdpTransport};

    /// A series' terms on the classic arena.
    fn series(seed: u64) -> MatchTerms {
        MatchTerms {
            seed,
            series: 1,
            ..MatchTerms::default()
        }
    }

    /// A host on its results card with `names` at the table, each a
    /// joiner's own session greeting it.
    fn table(names: &[&str]) -> (OnlineSession, Vec<OnlineSession>) {
        let mut host = OnlineSession::new(
            UdpTransport::host(0).expect("host socket"),
            Lockstep::new(0, (0..=names.len() as u8).collect(), DEFAULT_DELAY),
            1 + names.len() as u8,
            series(1),
        );
        let port = host.transport.local_addr().expect("addr").port();
        let mut joiners = Vec::new();
        for (at, name) in names.iter().enumerate() {
            let seat = at as u8 + 1;
            let joiner = OnlineSession::new(
                UdpTransport::join(("127.0.0.1", port)).expect("join"),
                Lockstep::new(seat, (0..=names.len() as u8).collect(), DEFAULT_DELAY),
                1 + names.len() as u8,
                series(1),
            );
            joiner.transport.send(NetMsg::hello(name));
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                host.poll_between_rounds(0.0);
                if host.transport.peer_count() > at {
                    break;
                }
            }
            joiners.push(joiner);
        }
        let plan: Vec<Option<u8>> = (1..=names.len() as u8).map(Some).collect();
        host.peers.deal(&plan);
        (host, joiners)
    }

    /// Every joiner polls its socket for a while.
    fn hear(joiners: &mut [OnlineSession]) {
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            for joiner in joiners.iter_mut() {
                joiner.poll_between_rounds(0.0);
            }
        }
    }

    /// The page with its cursor put on `card`, and Enter pressed there.
    fn pick(
        next: &mut NextMap,
        host: &mut OnlineSession,
        config: &mut MatchConfig,
        menu: &mut MatchMenu,
        card: crate::app::match_setup::Card,
    ) -> NextMapPress {
        let shelf = CustomBeaches::default();
        let keys = ButtonInput::<KeyCode>::default();
        // The page's first frame, on the Enter that opened it.
        next_map_keys(&keys, next, host, config, menu, &shelf, None);
        menu.slot = crate::app::match_setup::map_slots(&shelf)
            .iter()
            .position(|(_, slot)| *slot == crate::app::match_setup::Slot::Card(card))
            .expect("a card");
        let mut enter = ButtonInput::<KeyCode>::default();
        enter.press(KeyCode::Enter);
        next_map_keys(&enter, next, host, config, menu, &shelf, None)
    }

    /// The host's page between rounds: the table hears it is picking, the
    /// page is greyed for the next round's table with the cursor on the
    /// map just played, and the card picked is the map every peer's next
    /// round is dealt, on the seed its picture was drawn with, which the
    /// line-up then names.
    #[test]
    fn the_next_map_reaches_the_joiners() {
        use crate::app::match_setup::Card;
        let (mut host, mut joiners) = table(&["Bo"]);
        let (mut next, mut config, mut menu) = (
            NextMap::default(),
            MatchConfig::default(),
            MatchMenu::default(),
        );
        let shelf = CustomBeaches::default();
        open_next_map(&mut next, &mut host, &mut config, &mut menu, &shelf);
        assert_eq!(config.seats, 2, "greyed for the next round's table");
        assert_eq!(
            crate::app::match_setup::picked_card(&config, &shelf),
            Card::Map(MapChoice::Classic),
            "the cursor on the map just played"
        );
        hear(&mut joiners);
        assert!(joiners[0].picking_map, "the table is told");
        let seed = menu.seed;
        let press = pick(
            &mut next,
            &mut host,
            &mut config,
            &mut menu,
            Card::Map(MapChoice::TheMoat),
        );
        assert!(matches!(press, NextMapPress::Called(_)));
        assert!(!next.is_up() && !menu.host_page_up(), "the page is down");
        assert_eq!(
            (host.terms.map(), host.terms.seed),
            (MapChoice::TheMoat, seed)
        );
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            joiners[0].poll_between_rounds(0.0);
            if joiners[0].next_round {
                break;
            }
        }
        let joiner = &joiners[0];
        assert!(joiner.next_round, "the call reached the joiner");
        assert_eq!(joiner.terms, host.terms, "the same beach on every peer");
        assert!(!joiner.picking_map, "and the wait is over");
        let (name, _) = crate::app::lineup::map_words(&crate::app::i18n::EN, joiner);
        assert_eq!(
            name,
            crate::app::match_setup::map_name(&crate::app::i18n::EN, MapChoice::TheMoat),
            "the line-up names the new map"
        );
    }

    /// A host that played round one alone against the AI the table needed
    /// for a second castle, and a friend who turned up meanwhile: the next
    /// round is dealt its AI from the table as it then sits, by the host's
    /// dial, so the friend takes the chair and the AI is gone. Dealt from
    /// round one's AI, it stayed on beside them.
    #[test]
    fn the_next_round_deals_its_ai_from_the_table_as_it_sits() {
        use crate::app::match_setup::Card;
        let mut host = OnlineSession::new(
            UdpTransport::host(0).expect("host socket"),
            Lockstep::new(0, vec![0], DEFAULT_DELAY),
            2,
            MatchTerms {
                bots: 1,
                ..series(1)
            },
        );
        let port = host.transport.local_addr().expect("addr").port();
        let cy = UdpTransport::join(("127.0.0.1", port)).expect("join");
        cy.send(NetMsg::hello("Cy"));
        host_hears(&mut host);
        assert_eq!(host.transport.peer_count(), 1, "Cy is in line");
        let (mut next, mut config, mut menu) = (
            NextMap::default(),
            MatchConfig::default(),
            MatchMenu::default(),
        );
        let shelf = CustomBeaches::default();
        open_next_map(&mut next, &mut host, &mut config, &mut menu, &shelf);
        assert_eq!(config.seats, 2, "the host and Cy, and no AI");
        let press = pick(
            &mut next,
            &mut host,
            &mut config,
            &mut menu,
            Card::Map(MapChoice::Classic),
        );
        assert!(matches!(press, NextMapPress::Called(_)));
        assert_eq!(
            (host.seats, host.terms.bots),
            (2, 0),
            "Cy in the AI's chair"
        );
        assert_eq!(host.peers.seat_of(0), Some(1));
    }

    /// A table in pairs whose host picked partners keeps them round after
    /// round: Bo back in P3 and the AI in P2, as the pick put them, and
    /// the order every peer seats the round by says so.
    #[test]
    fn the_partners_picked_hold_into_the_next_round() {
        use crate::app::match_setup::Card;
        let (mut host, _joiners) = table(&["Bo"]);
        host.terms.teams = crate::app::teams::TeamMode::Pairs.index() as u8;
        host.arranged = Some(crate::app::net::order::Arranged {
            bots: Vec::new(),
            ai: vec![1, 3],
        });
        host.peers.row(0).seat_wish = Some(2);
        let (mut next, mut config, mut menu) = (
            NextMap::default(),
            MatchConfig {
                lobby_bots: 2,
                ..MatchConfig::default()
            },
            MatchMenu::default(),
        );
        let shelf = CustomBeaches::default();
        open_next_map(&mut next, &mut host, &mut config, &mut menu, &shelf);
        let press = pick(
            &mut next,
            &mut host,
            &mut config,
            &mut menu,
            Card::Map(MapChoice::Classic),
        );
        assert!(matches!(press, NextMapPress::Called(_)));
        assert_eq!(host.seats, 4);
        assert_eq!(host.peers.seat_of(0), Some(2), "Bo where the pick put him");
        assert_eq!(host.terms.ai_seats(4), [1, 3]);
        assert_eq!(host.terms.player_seats(4), [0, 2]);
        assert_eq!(host.names[2], "Bo");
    }

    /// The page's first frame, on the Enter that opened it.
    fn first_frame(
        next: &mut NextMap,
        host: &mut OnlineSession,
        config: &mut MatchConfig,
        menu: &mut MatchMenu,
    ) {
        let keys = ButtonInput::<KeyCode>::default();
        let shelf = CustomBeaches::default();
        next_map_keys(&keys, next, host, config, menu, &shelf, None);
    }

    /// The host hears whatever is on its socket for a while.
    fn host_hears(host: &mut OnlineSession) {
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            host.poll_between_rounds(0.1);
        }
    }

    /// Somebody frozen at the table leaving while the host picks greys the
    /// page for the smaller table at once: a map that seats four and not
    /// three is greyed, its line saying why, and Enter on it calls nothing;
    /// the host picks again, and the round is dealt on that pick for the
    /// seats the page showed. The page used to stay greyed for four, and
    /// the call swapped the pick for some map that seats three.
    #[test]
    fn a_leaver_during_the_pick_greys_the_page_again() {
        use crate::app::match_setup::{Card, card_note};
        let (mut host, mut joiners) = table(&["Bo", "Cy", "Di"]);
        let (mut next, mut config, mut menu) = (
            NextMap::default(),
            MatchConfig::default(),
            MatchMenu::default(),
        );
        let shelf = CustomBeaches::default();
        open_next_map(&mut next, &mut host, &mut config, &mut menu, &shelf);
        first_frame(&mut next, &mut host, &mut config, &mut menu);
        assert_eq!(config.seats, 4);
        let gone = joiners.pop().expect("Di");
        gone.transport.send(NetMsg::Leaving);
        drop(gone);
        host_hears(&mut host);
        assert_eq!(host.next_round_seats(0), 3, "Di is gone");
        let round_table = Card::Map(MapChoice::RoundTable);
        let press = pick(&mut next, &mut host, &mut config, &mut menu, round_table);
        assert_eq!(press, NextMapPress::Nothing, "a greyed card calls nothing");
        assert!(next.is_up());
        assert_eq!(config.seats, 3, "greyed for the table as it now is");
        let note = card_note(&crate::app::i18n::EN, round_table, &config, &shelf, true);
        assert!(note.contains("this table has 3"), "{note}");
        let seed = menu.seed;
        let press = pick(
            &mut next,
            &mut host,
            &mut config,
            &mut menu,
            Card::Map(MapChoice::Classic),
        );
        assert!(matches!(press, NextMapPress::Called(_)));
        assert_eq!(host.seats, 3, "dealt without Di");
        assert_eq!(
            (host.terms.map(), host.terms.seed),
            (MapChoice::Classic, seed),
            "on the pick, and the seed its picture was drawn with"
        );
    }

    /// Somebody turning up while the host picks waits for the round after,
    /// as at the lobby's start: the page stays greyed for the table it
    /// opened for, the round is dealt that table with the newcomer
    /// watching it, and the deal after seats the newcomer.
    #[test]
    fn a_newcomer_during_the_pick_plays_the_round_after() {
        use crate::app::match_setup::Card;
        let (mut host, _joiners) = table(&["Bo"]);
        let (mut next, mut config, mut menu) = (
            NextMap::default(),
            MatchConfig::default(),
            MatchMenu::default(),
        );
        let shelf = CustomBeaches::default();
        open_next_map(&mut next, &mut host, &mut config, &mut menu, &shelf);
        first_frame(&mut next, &mut host, &mut config, &mut menu);
        let port = host.transport.local_addr().expect("addr").port();
        let cy = UdpTransport::join(("127.0.0.1", port)).expect("join");
        cy.send(NetMsg::hello("Cy"));
        host_hears(&mut host);
        assert_eq!(host.transport.peer_count(), 2, "Cy is on the socket");
        assert_eq!(host.next_round_seats(0), 2, "but not at this table");
        let press = pick(
            &mut next,
            &mut host,
            &mut config,
            &mut menu,
            Card::Map(MapChoice::TheMoat),
        );
        assert!(matches!(press, NextMapPress::Called(_)));
        assert_eq!(config.seats, 2);
        assert_eq!(host.seats, 2, "dealt the table the map was picked for");
        assert_eq!(host.terms.map(), MapChoice::TheMoat);
        assert_eq!(host.peers.seat_of(1), None, "Cy watches this round");
        assert_eq!(
            host.peers.seat_plan(2),
            [Some(1), Some(2)],
            "and is dealt a chair in the next"
        );
    }
}
