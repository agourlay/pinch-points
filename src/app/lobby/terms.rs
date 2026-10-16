//! The dials a host turns while the table fills up, and the beach they
//! describe once it is known.

use super::*;

/// A dial the host turns while the table fills up.
///
/// A subset of the match-setup screen's rows, because online two of them
/// mean nothing: the seat count is however many people turn up, and the
/// names are the players' own. Everything else about the round is the
/// host's to set, and travels to every peer in the invitation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dial {
    Map,
    Gulls,
    Round,
    Posts,
    Bots,
    Teams,
    Series,
    /// Whether a peer may join as a bot ("Bots welcome"). Said in the
    /// beacon, so the list shows it before anyone walks up.
    Welcome,
}

impl Dial {
    pub const ALL: [Dial; 8] = [
        Dial::Map,
        Dial::Gulls,
        Dial::Round,
        Dial::Posts,
        Dial::Bots,
        Dial::Teams,
        Dial::Series,
        Dial::Welcome,
    ];

    /// The row as it reads: what it sets, and what it is set to. A host
    /// passes the table its dials would deal now (`dealt`), a joiner the
    /// one its roster describes, and the map, AI and teams rows read that
    /// rather than the dials themselves. `turns` says the card is the
    /// host's own, whose teams row also says what it is dialled to.
    pub fn label(
        self,
        tr: &crate::app::i18n::Tr,
        config: &MatchConfig,
        teams: crate::app::teams::TeamMode,
        beaches: &crate::app::match_setup::CustomBeaches,
        dealt: Option<Dealt>,
        turns: bool,
    ) -> (&'static str, String) {
        use crate::app::cycle::Cycle;
        match self {
            // The note about beaches this table is too big for is not
            // appended here: this value cell clips, so a sentence on the
            // end would push the map's own name out of sight. It gets a
            // line of its own under the dials.
            Dial::Map => (
                tr.match_map,
                match dealt {
                    Some(dealt) => dealt_map_label(tr, config, dealt, beaches),
                    None => crate::app::match_setup::map_label(config, tr, beaches),
                },
            ),
            Dial::Gulls => (
                tr.match_gulls,
                tr.gull_names[config.gulls.index()].to_string(),
            ),
            Dial::Round => (
                tr.match_round,
                tr.round_names[config.round.index()].to_string(),
            ),
            Dial::Posts => (tr.match_posts, config.posts.to_string()),
            // With the level they play at, which no other row sets: it is
            // the couch's top AI seat's, and played unannounced it could
            // be one set there long ago.
            Dial::Bots => (tr.match_ai, {
                let bots = dealt.map_or(config.bots, |dealt| dealt.bots);
                match bots {
                    0 => bots.to_string(),
                    _ => format!(
                        "{bots} ({})",
                        tr.bot_levels[crate::app::match_setup::online_ai_level(config).index()]
                    ),
                }
            }),
            // What will be scored, which a mode the table does not fit
            // is not: the round falls back to free-for-all. The host, who
            // can dial a mode before the table fits it, is also told what
            // it dialled and when that starts.
            Dial::Teams => (tr.set_versus_mode, {
                let played = dealt.map_or(teams, |dealt| teams.played_by(dealt.seats));
                let from = dealt.and_then(|dealt| teams.fits_from(dealt.seats));
                match from.filter(|_| turns && played != teams) {
                    Some(from) => fill(
                        tr.team_mode_waits,
                        &[
                            ("m", tr.team_mode_names[teams.index()]),
                            ("n", &from.to_string()),
                        ],
                    ),
                    None => tr.team_modes[played.index()].to_string(),
                }
            }),
            Dial::Series => (
                tr.match_mode,
                tr.mode_names[config.series.index()].to_string(),
            ),
            Dial::Welcome => (
                tr.lobby_bots_dial,
                tr.lobby_bots_values[usize::from(!config.bots_welcome)].to_string(),
            ),
        }
    }

    /// Turn it. `humans` is how many are at the table, people and the bots
    /// that came to the door, and what the AI has to fit behind: a chair
    /// taken is not one the host can fill with the AI.
    ///
    /// The map and AI rows read what the table would be dealt
    /// ([`dealt`]), so they turn from that: a turn that moved the dial
    /// but not the row was a dead press.
    pub fn turn(
        self,
        turn: Turn,
        config: &mut MatchConfig,
        teams: &mut crate::app::teams::TeamMode,
        humans: u8,
        beaches: &crate::app::match_setup::CustomBeaches,
    ) {
        use crate::app::cycle::Cycle;
        let room = (MAX_PLAYERS as u8).saturating_sub(humans);
        match self {
            Dial::Map => {
                // Past the stops the table has outgrown: on Classic with
                // five aboard the row reads the large beach, and so does
                // every other four-castle stop.
                let on_air = |config: &MatchConfig| {
                    let dealt = dealt(config, humans);
                    (dealt.map, roster_beach(config, dealt.seats, beaches))
                };
                let shown = on_air(config);
                for _ in 0..64 {
                    crate::app::match_setup::cycle_map(config, turn, beaches);
                    if on_air(config) != shown {
                        break;
                    }
                }
            }
            Dial::Gulls => config.gulls = config.gulls.cycled(turn),
            Dial::Round => config.round = config.round.cycled(turn),
            Dial::Posts => {
                config.posts = crate::app::cycle::dial(
                    config.posts,
                    turn,
                    1,
                    crate::app::match_setup::POSTS_RANGE,
                );
            }
            Dial::Bots => {
                // From the count the card shows: a dial left at four after
                // three players sat down reads two, and one turn down
                // should read one, not still two.
                // A press that cannot move the count shown (a full table,
                // or already at the end) leaves the dial alone: written
                // back clamped, it wiped the host's setting with nothing
                // on the card to say so.
                let shown = dealt(config, humans).bots;
                let step = (i32::from(shown) + turn.signum()).clamp(0, i32::from(room)) as u8;
                if step != shown {
                    config.bots = step;
                }
            }
            // Every mode, whether this table fits it yet or not: a host
            // sets up pairs before the fourth player sits down, and the row
            // says it is free-for-all until then (`label`).
            Dial::Teams => *teams = teams.cycled(turn),
            Dial::Series => config.series = config.series.cycled(turn),
            Dial::Welcome => config.bots_welcome = !config.bots_welcome,
        }
    }
}

/// Everyone already at the host's table, as the card counts them: the
/// host, the players aboard, the bots that came to the door. What the AI
/// has to fit behind, and the one count the host's card, its dials and
/// the note under them deal from.
pub(super) fn humans_here(state: &LobbyState) -> u8 {
    (state.table.len().min(MAX_PLAYERS) as u8)
        .max(1 + state.hosted().map_or(0, Hosted::players_aboard) as u8)
}

/// The table the host's dials would deal as it sits now, or `None` when
/// this lobby is not hosting.
pub(super) fn hosts_dealt(state: &LobbyState, config: &MatchConfig) -> Option<Dealt> {
    state.hosting().then(|| dealt(config, humans_here(state)))
}

/// The line under the dials: the string a bot connects straight to this
/// beach with (route 2) while there is one, since a bot cannot come in
/// without it, else why the map dial offers none of the host's own
/// beaches. Only the host's: a joiner's card is the host's match, and
/// neither line is anything to do with it.
///
/// The beach note reads the table the launch deals. It read the couch's
/// seat count, for joiners too, and ahead of the bot string, so a host
/// whose couch was set for six lost the string to "none of your maps fits
/// 6 players" at a table of two.
pub(super) fn dials_note(
    state: &LobbyState,
    config: &MatchConfig,
    tr: &crate::app::i18n::Tr,
    beaches: &crate::app::match_setup::CustomBeaches,
) -> String {
    let (Some(hosted), Some(dealt)) = (state.hosted(), hosts_dealt(state, config)) else {
        return String::new();
    };
    hosted
        .bot_string
        .as_ref()
        .map(|string| format!("{}: {string}", tr.lobby_bots_dial))
        .or_else(|| crate::app::match_setup::beaches_note(dealt.seats, tr, beaches))
        .unwrap_or_default()
}

/// The name of the handmade beach the host's dials are on, for the
/// roster: the one the launch would send a table of `seats`
/// ([`crate::app::match_setup::beach_on_air`]), or empty when the dial is
/// on a built-in map or on no beach that will travel or seat the table
/// (both ends then build the generated arena, which [`hosts_map_label`]
/// names).
pub(super) fn roster_beach(
    config: &MatchConfig,
    seats: u8,
    beaches: &crate::app::match_setup::CustomBeaches,
) -> crate::transport::WireName {
    let name = crate::app::match_setup::beach_on_air(config, seats, beaches)
        .map_or("", |beach| beach.level.name.as_str());
    crate::transport::wire_name(name)
}

/// The AI seats a launch deals behind `humans`: the dial, as many as there
/// are chairs for, raised when needed so the beach has two castles (a
/// one-seat `Start` is refused by every joiner's decoder). The same floor
/// `call_next_round` keeps between rounds.
pub(super) fn launch_bots(humans: u8, dial: u8) -> u8 {
    dial.min(crate::sim::MAX_PLAYERS as u8 - humans)
        .max(2u8.saturating_sub(humans))
}

/// The table a launch deals under the host's dials with `humans` seated
/// (the host, the players aboard, the bots that came to the door): the AI
/// that join them and the map they play. The launch, the roster and the
/// host's card all read this one answer. Read off the raw dials, the card
/// said Classic and four AI while a fifth player's arrival had the round
/// dealt on the large beach with two.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dealt {
    pub bots: u8,
    pub seats: u8,
    pub map: crate::app::match_setup::MapChoice,
}

/// The table a joiner's card reads off the host's roster: `table` names
/// at it and the terms the host would launch them on, which already carry
/// the AI and the map as dealt.
pub(super) fn roster_dealt(table: usize, terms: &MatchTerms) -> Dealt {
    let bots = terms.bots.min(MAX_PLAYERS as u8);
    Dealt {
        bots,
        seats: (table.min(MAX_PLAYERS) as u8 + bots).min(MAX_PLAYERS as u8),
        map: crate::app::match_setup::TermsDials::map(terms),
    }
}

pub(super) fn dealt(config: &MatchConfig, humans: u8) -> Dealt {
    let bots = launch_bots(humans, config.bots);
    let seats = humans + bots;
    Dealt {
        bots,
        seats,
        map: map_for(config, seats),
    }
}

/// The map row on the host's card: what the table as it sits would play,
/// named the way a joiner's card names it off the roster, so the two
/// agree. Read back under the beach's own cap, as the joiner reads it
/// (`join_tick`): the player-name cap cut a long beach name to twelve
/// characters on the host's card and nowhere else.
fn dealt_map_label(
    tr: &crate::app::i18n::Tr,
    config: &MatchConfig,
    dealt: Dealt,
    beaches: &crate::app::match_setup::CustomBeaches,
) -> String {
    let beach = roster_beach(config, dealt.seats, beaches);
    hosts_map_label(tr, dealt.map, &roster_beach_name(&beach))
}

/// The name of the host's beach as the roster carries it, read back the
/// one way both ends read it.
pub(super) fn roster_beach_name(beach: &crate::transport::WireName) -> String {
    crate::transport::beach_name_from_wire(beach)
}

/// The map dial as a joiner reads it, off the host's terms and the name
/// the roster gave the host's beach.
///
/// Not [`crate::app::match_setup::map_label`], which looks the beach up on
/// *this* machine's shelf: `config_from_terms` can only say "a handmade
/// beach, the first one", so a joiner's card named a beach of its own, or
/// none, where the host's was going to be played.
pub(super) fn hosts_map_label(
    tr: &crate::app::i18n::Tr,
    map: crate::app::match_setup::MapChoice,
    beach: &str,
) -> String {
    use crate::app::match_setup::MapChoice;
    match (map, beach.is_empty()) {
        (MapChoice::Custom, false) => beach.to_string(),
        // A handmade beach that does not travel: both ends build the
        // generated arena in its place, as `map_label` says on the host.
        (MapChoice::Custom, true) => tr.map_names[MapChoice::GenXl.index()].to_string(),
        (map, _) => tr.map_names[map.index()].to_string(),
    }
}

/// The beach these terms describe, once the table is known.
///
/// The handcrafted arena seats four, and a fifth player arriving cannot be
/// given a castle on it. The match-setup screen drops the seat count, which
/// online is not its to drop, so the map gives way instead.
pub fn map_for(config: &MatchConfig, seats: u8) -> crate::app::match_setup::MapChoice {
    use crate::app::match_setup::{CLASSIC_SEATS, WIDE_ENOUGH, sized_for};
    match seats > CLASSIC_SEATS && config.map.size().0 < WIDE_ENOUGH {
        true => sized_for(seats),
        false => config.map,
    }
}

/// The host's arrows: up and down pick a dial, left and right turn it.
///
/// The two faces of the lobby never show at once, so the same keys can mean
/// the beach list on one and the dials on the other without either being
/// ambiguous.
pub(super) fn turn_the_dials(
    keys: &ButtonInput<KeyCode>,
    settings: &mut GameSettings,
    caps: &crate::app::keycaps::KeyCaps,
    config: &mut MatchConfig,
    state: &mut LobbyState,
    beaches: &crate::app::match_setup::CustomBeaches,
) {
    if state.hosting() {
        if keys.just_pressed(KeyCode::ArrowUp) {
            state.dial = (state.dial + Dial::ALL.len() - 1) % Dial::ALL.len();
        }
        if keys.just_pressed(KeyCode::ArrowDown) {
            state.dial = (state.dial + 1) % Dial::ALL.len();
        }
        let turn = match (
            keys.just_pressed(KeyCode::ArrowRight),
            keys.just_pressed(KeyCode::ArrowLeft),
        ) {
            (true, false) => Some(Turn::Right),
            (false, true) => Some(Turn::Left),
            _ => None,
        };
        if let Some(turn) = turn
            && let Some(dial) = Dial::ALL.get(state.dial).copied()
        {
            // Everyone already here is what the AI has to fit behind,
            // counted as the card counts them: the bots at the door too.
            let humans = humans_here(state);
            let mut teams = settings.team_mode;
            dial.turn(turn, config, &mut teams, humans, beaches);
            if teams != settings.team_mode {
                settings.team_mode = teams;
                settings.save(caps);
            }
        }
    }
}

#[cfg(test)]
mod dial_tests {
    use super::*;
    use crate::app::match_setup::{CLASSIC_SEATS, MapChoice};
    use crate::app::teams::TeamMode;

    /// Every dial says what it sets and what it is set to, in every
    /// language: a row with a blank half is a row nobody can use.
    #[test]
    fn every_dial_reads_as_a_row() {
        let config = MatchConfig::default();
        for lang in crate::app::i18n::ALL_LANGS {
            for dial in Dial::ALL {
                let (name, value) = dial.label(
                    lang.tr(),
                    &config,
                    TeamMode::Solo,
                    &Default::default(),
                    None,
                    true,
                );
                assert!(!name.is_empty(), "{dial:?} in {lang:?} has no name");
                assert!(!value.is_empty(), "{dial:?} in {lang:?} has no value");
            }
        }
    }

    /// Every dial turns, both ways, and turning one leaves the others
    /// alone. A dial that silently moved its neighbour would be the sort of
    /// thing nobody notices until a match starts on the wrong beach.
    #[test]
    fn every_dial_turns_and_only_itself() {
        for dial in Dial::ALL {
            for turn in [Turn::Right, Turn::Left] {
                // Off the floor, or turning the AI down is correctly a
                // no-op and the assertion below would be testing the
                // clamp rather than the dial.
                let mut config = MatchConfig {
                    bots: 1,
                    ..MatchConfig::default()
                };
                let mut teams = TeamMode::Solo;
                let before = (
                    config.map,
                    config.gulls,
                    config.round,
                    config.bots,
                    teams,
                    config.series,
                    config.posts,
                    config.bots_welcome,
                );
                // Two at the table and the AI one more: three seats, which
                // no team mode fits. The Teams dial turns all the same, so
                // a host can set up pairs before the fourth sits down.
                dial.turn(turn, &mut config, &mut teams, 2, &Default::default());
                let after = (
                    config.map,
                    config.gulls,
                    config.round,
                    config.bots,
                    teams,
                    config.series,
                    config.posts,
                    config.bots_welcome,
                );
                assert_ne!(before, after, "{dial:?} turned {turn:?} and did nothing");
                // Exactly one of the eight moved.
                let moved = [
                    before.0 != after.0,
                    before.1 != after.1,
                    before.2 != after.2,
                    before.3 != after.3,
                    before.4 != after.4,
                    before.5 != after.5,
                    before.6 != after.6,
                    before.7 != after.7,
                ];
                assert_eq!(
                    moved.iter().filter(|m| **m).count(),
                    1,
                    "{dial:?} moved more than itself"
                );
            }
        }
    }

    /// The AI fits behind the people. A chair somebody is sitting in is not
    /// one the host can fill with a bot, and the table does not grow past
    /// its seats however long the key is held.
    #[test]
    fn the_ai_takes_only_the_chairs_the_players_leave() {
        let mut config = MatchConfig::default();
        let mut teams = TeamMode::Solo;
        // Four humans at the table: two chairs left, and no more.
        for _ in 0..10 {
            Dial::Bots.turn(Turn::Right, &mut config, &mut teams, 4, &Default::default());
        }
        assert_eq!(config.bots, MAX_PLAYERS as u8 - 4);
        // And back down to none, not below it.
        for _ in 0..10 {
            Dial::Bots.turn(Turn::Left, &mut config, &mut teams, 4, &Default::default());
        }
        assert_eq!(config.bots, 0);
        // A full table leaves the AI nothing at all.
        Dial::Bots.turn(
            Turn::Right,
            &mut config,
            &mut teams,
            MAX_PLAYERS as u8,
            &Default::default(),
        );
        assert_eq!(config.bots, 0, "six people is six people");
        // And a press there changes nothing the card shows, so it changes
        // nothing: the host's dial is still what it was when the next
        // table sits down.
        config.bots = 3;
        for turn in [Turn::Right, Turn::Left] {
            Dial::Bots.turn(
                turn,
                &mut config,
                &mut teams,
                MAX_PLAYERS as u8,
                &Default::default(),
            );
            assert_eq!(config.bots, 3, "a dead press {turn:?}");
        }
    }

    /// A fifth player arriving at the handcrafted beach cannot be given a
    /// castle: it seats four. See [`map_for`] for why the map gives way
    /// rather than the seat count.
    #[test]
    fn a_small_beach_gives_way_to_the_people_who_turned_up() {
        let mut config = MatchConfig {
            map: MapChoice::Classic,
            ..MatchConfig::default()
        };
        assert!(config.map.size().0 < crate::app::match_setup::WIDE_ENOUGH);

        // Four or fewer: the host gets the beach it asked for.
        for seats in 2..=CLASSIC_SEATS {
            assert_eq!(map_for(&config, seats), MapChoice::Classic, "{seats} seats");
        }
        // Five or six: a beach with room for them, every time.
        for seats in CLASSIC_SEATS + 1..=MAX_PLAYERS as u8 {
            let map = map_for(&config, seats);
            assert!(
                map.size().0 >= crate::app::match_setup::WIDE_ENOUGH,
                "{seats} seats landed on {map:?}, which cannot hold them"
            );
        }
        // A beach already big enough is left alone whatever the table.
        config.map = MapChoice::GenLarge;
        for seats in 2..=MAX_PLAYERS as u8 {
            assert_eq!(map_for(&config, seats), MapChoice::GenLarge);
        }
    }

    /// Whatever the host sets is what every peer is told, since the terms
    /// are the only thing they build their beach from.
    #[test]
    fn what_the_host_turns_is_what_travels() {
        let mut config = MatchConfig::default();
        let mut teams = TeamMode::Solo;
        for dial in Dial::ALL {
            dial.turn(Turn::Right, &mut config, &mut teams, 2, &Default::default());
        }
        let terms = crate::app::match_setup::terms(&config, teams, 7);
        assert_eq!(terms.map, config.map.index() as u8);
        assert_eq!(terms.gulls, config.gulls.index() as u8);
        assert_eq!(terms.round, config.round.index() as u8);
        assert_eq!(terms.bots, config.bots);
        assert_eq!(terms.teams, teams.index() as u8);
        assert_eq!(terms.series, config.series.index() as u8);
        assert_eq!(terms.posts, config.posts);
        assert_eq!(terms.seed, 7);
    }

    /// The host's card reads the table its dials would deal, not the dials:
    /// Classic and four AI, then five people sit down, is a round on the
    /// large beach with one AI, and that is what the card says. A turn
    /// down from there steps from the one it shows.
    #[test]
    fn the_hosts_card_reads_what_the_launch_deals() {
        use crate::app::i18n::EN;
        let mut config = MatchConfig {
            map: MapChoice::Classic,
            bots: 4,
            ..MatchConfig::default()
        };
        let shelf = Default::default();
        let dealt = dealt(&config, 5);
        assert_eq!((dealt.bots, dealt.seats), (1, 6));
        let row = |dial: Dial, config: &MatchConfig| {
            dial.label(&EN, config, TeamMode::Solo, &shelf, Some(dealt), true)
                .1
        };
        assert_eq!(row(Dial::Bots, &config), "1 (normal)");
        assert_eq!(
            row(Dial::Map, &config),
            EN.map_names[MapChoice::GenLarge.index()]
        );
        let mut teams = TeamMode::Solo;
        Dial::Bots.turn(Turn::Left, &mut config, &mut teams, 5, &shelf);
        assert_eq!(config.bots, 0, "one less than the one shown");

        // Five aboard on Classic read the large beach. A turn of the map
        // goes past every stop that would read the same, to one that
        // reads otherwise.
        config.map = MapChoice::Classic;
        let before = dealt_map_label(&EN, &config, super::dealt(&config, 5), &shelf);
        Dial::Map.turn(Turn::Right, &mut config, &mut teams, 5, &shelf);
        let after = dealt_map_label(&EN, &config, super::dealt(&config, 5), &shelf);
        assert_ne!(before, after, "a turn the card can see");
    }

    /// The line under the dials is the host's, read against the table
    /// the launch deals and not the couch's seat count, and the string a
    /// bot connects with is never pushed out by it.
    #[test]
    fn the_note_under_the_dials_reads_the_hosts_table() {
        use crate::app::i18n::EN;
        use crate::app::match_setup::{Beach, CustomBeaches};
        let two = crate::sim::Level::parse(
            "name: Two\nposts: 1\nkind: arena\ncrab: 0,1 R R common\nmap:\n\
             +-+-+-+\n|0 . .|\n+ + + +\n|. . 1|\n+ + + +\n|. . .|\n+-+-+-+\n",
        )
        .expect("a level");
        let shelf = CustomBeaches(vec![Beach::new(two)]);
        // The couch is set for six, which no beach on the shelf seats.
        let config = MatchConfig {
            seats: 6,
            bots: 0,
            ..MatchConfig::default()
        };
        let mut state = LobbyState {
            table: vec!["Ana".into(), "Bo".into()],
            ..LobbyState::default()
        };
        // Not hosting: nothing to say, whatever the couch is set to.
        assert_eq!(dials_note(&state, &config, &EN, &shelf), "");
        state.standing = Standing::hosting(
            crate::transport::Announcer::new(0xD1A1).expect("announcer"),
            crate::transport::UdpTransport::host(0).expect("socket"),
        );
        // Two at the table, and the beach seats two.
        assert_eq!(dials_note(&state, &config, &EN, &shelf), "");
        // Four at the table, and it does not.
        state.table.extend(["Cy".into(), "Dee".into()]);
        let note = dials_note(&state, &config, &EN, &shelf);
        assert!(note.contains('4'), "{note}");
        // And the bot's string outranks it.
        if let Standing::Hosting(hosted) = &mut state.standing {
            hosted.bot_string = Some("pinch://here".into());
        }
        let note = dials_note(&state, &config, &EN, &shelf);
        assert!(note.contains("pinch://here"), "{note}");
    }

    /// The teams row reads what will be scored. Pairs dialled at a table
    /// of three is a free-for-all round: a joiner's card says so, and the
    /// host's says what it dialled and from when it is played. The dial
    /// turns through every mode whatever the table, keeping what was
    /// dialled.
    #[test]
    fn the_teams_row_reads_what_will_be_played() {
        use crate::app::i18n::EN;
        let shelf = Default::default();
        let mut config = MatchConfig::default();
        let three = dealt(&config, 3);
        assert_eq!(three.seats, 3);
        let card = MatchConfig::default();
        let row = |teams: TeamMode, dealt: Dealt, turns: bool| {
            Dial::Teams
                .label(&EN, &card, teams, &shelf, Some(dealt), turns)
                .1
        };
        assert_eq!(
            row(TeamMode::Pairs, three, false),
            EN.team_modes[TeamMode::Solo.index()],
            "a joiner reads what is played"
        );
        assert_eq!(
            row(TeamMode::Pairs, three, true),
            "pairs (free-for-all until 4)",
            "the host reads what it dialled as well"
        );
        assert_eq!(
            row(TeamMode::Trios, three, true),
            "trios (free-for-all until 6)"
        );
        let five = Dealt { seats: 5, ..three };
        assert_eq!(
            row(TeamMode::Pairs, five, true),
            "pairs (free-for-all until 6)",
            "the next table that fits, not the first"
        );
        assert_eq!(
            row(TeamMode::Solo, three, true),
            EN.team_modes[TeamMode::Solo.index()]
        );

        // Every mode on the way round, at a table of three as at four.
        let mut teams = TeamMode::Solo;
        for want in [TeamMode::Pairs, TeamMode::Trios, TeamMode::Solo] {
            Dial::Teams.turn(Turn::Right, &mut config, &mut teams, 3, &shelf);
            assert_eq!(teams, want);
        }
        let four = dealt(&config, 4);
        assert_eq!(
            row(TeamMode::Pairs, four, true),
            EN.team_modes[TeamMode::Pairs.index()],
            "played as dialled once it fits"
        );

        // A joiner reads the same off the roster: the names at the table
        // and the AI the host's terms carry.
        let terms = MatchTerms {
            bots: 1,
            ..crate::app::match_setup::terms(&config, TeamMode::Pairs, 0)
        };
        assert_eq!(roster_dealt(2, &terms).seats, 3);
        assert_eq!(
            row(TeamMode::Pairs, roster_dealt(2, &terms), false),
            EN.team_modes[TeamMode::Solo.index()]
        );
    }

    /// The host's waiting teams row is no wider than the widest map name
    /// the terms card already holds in any language: the value cell clips,
    /// and a row that ran past it would lose the number it waits for.
    #[test]
    fn the_waiting_teams_row_fits_where_the_map_names_do() {
        use crate::app::i18n::metrics::text_px;
        let widest = crate::app::i18n::ALL_LANGS
            .iter()
            .flat_map(|lang| lang.tr().map_names.iter())
            .map(|name| text_px(name, 18.0))
            .fold(0.0, f32::max);
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            for mode in [TeamMode::Pairs, TeamMode::Trios] {
                let row = fill(
                    tr.team_mode_waits,
                    &[("m", tr.team_mode_names[mode.index()]), ("n", "6")],
                );
                let wide = text_px(&row, 18.0);
                assert!(
                    wide <= widest,
                    "{lang:?}: {row:?} is {wide:.0}px, the widest map name {widest:.0}px"
                );
            }
        }
    }

    /// Every language says the host's waiting teams row with both of its
    /// placeholders filled.
    #[test]
    fn the_waiting_teams_row_is_said_in_every_language() {
        let three = dealt(&MatchConfig::default(), 3);
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            let row = Dial::Teams
                .label(
                    tr,
                    &MatchConfig::default(),
                    TeamMode::Pairs,
                    &Default::default(),
                    Some(three),
                    true,
                )
                .1;
            assert!(!row.contains('{'), "{lang:?}: {row}");
            assert!(row.contains('4'), "{lang:?}: {row}");
            assert!(
                row.contains(tr.team_mode_names[TeamMode::Pairs.index()]),
                "{lang:?}: {row}"
            );
        }
    }

    /// A handmade beach with a name longer than a player's reads whole on
    /// the host's card, and the same as a joiner reads it off the roster.
    #[test]
    fn host_and_joiner_name_a_long_beach_alike() {
        use crate::app::i18n::EN;
        use crate::app::match_setup::{Beach, CustomBeaches};
        let long = "Crabby Lagoon at Dusk";
        assert!(long.chars().count() > crate::transport::NAME_CHARS);
        let level = crate::sim::Level::parse(&format!(
            "name: {long}\nposts: 1\nkind: arena\ncrab: 0,1 R R common\nmap:\n\
             +-+-+-+\n|0 . .|\n+ + + +\n|. . 1|\n+ + + +\n|. . .|\n+-+-+-+\n"
        ))
        .expect("a level");
        let shelf = CustomBeaches(vec![Beach::new(level)]);
        let config = MatchConfig {
            map: MapChoice::Custom,
            seats: 2,
            ..MatchConfig::default()
        };
        let dealt = dealt(&config, 2);
        let host = dealt_map_label(&EN, &config, dealt, &shelf);
        assert_eq!(host, long, "the host's card names it whole");
        // What the roster carries, read the way `join_tick` reads it.
        let wire = roster_beach(&config, dealt.seats, &shelf);
        let joiner = hosts_map_label(&EN, dealt.map, &roster_beach_name(&wire));
        assert_eq!(host, joiner);
    }
}
