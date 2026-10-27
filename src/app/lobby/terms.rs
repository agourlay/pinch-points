//! The dials a host turns while the table fills up, and the beach they
//! describe once it is known.

use super::*;

/// A row of the terms card, most of them dials the host turns while the
/// table fills up.
///
/// A subset of the match-setup screen's rows, because online two of them
/// mean nothing: the seat count is however many people turn up, and the
/// names are the players' own. Everything else about the round is the
/// host's to set, and travels to every peer in the invitation. The map is
/// not turned here: the host picks it on the map page once it starts the
/// match and the table is frozen ([`super::pick`]), and the row says so.
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

    /// The rows the host's cursor walks and turns: every one but the
    /// map's ([`Dial::turns`]).
    pub const TURNS: [Dial; 7] = [
        Dial::Gulls,
        Dial::Round,
        Dial::Posts,
        Dial::Bots,
        Dial::Teams,
        Dial::Series,
        Dial::Welcome,
    ];

    /// Whether the host turns this row on the lobby card: all but the
    /// map's, which is picked at the start.
    pub fn turns(self) -> bool {
        Dial::TURNS.contains(&self)
    }

    /// The row as it reads: what it sets, and what it is set to, on
    /// `card`. The map, AI and teams rows read the table the card deals
    /// rather than the dials themselves; a joiner still waiting for the
    /// host's terms has no values to show.
    pub fn label(self, tr: &crate::app::i18n::Tr, card: &TermsCard) -> (&'static str, String) {
        (self.name(tr), self.value(tr, card).unwrap_or_default())
    }

    /// What the row sets.
    fn name(self, tr: &crate::app::i18n::Tr) -> &'static str {
        match self {
            Dial::Map => tr.match_map,
            Dial::Gulls => tr.match_gulls,
            Dial::Round => tr.match_round,
            Dial::Posts => tr.match_posts,
            Dial::Bots => tr.match_ai,
            Dial::Teams => tr.set_versus_mode,
            Dial::Series => tr.match_mode,
            Dial::Welcome => tr.lobby_bots_dial,
        }
    }

    /// What the row is set to on `card`, or `None` while there are no
    /// terms to read.
    fn value(self, tr: &crate::app::i18n::Tr, card: &TermsCard) -> Option<String> {
        use crate::app::cycle::Cycle;
        let (config, teams, dealt) = card.terms()?;
        // Only the host's own card says what its teams row is dialled to.
        let turns = matches!(card, TermsCard::Hosting { .. });
        Some(match self {
            // The note about beaches this table is too big for is not
            // appended here: this value cell clips, so a sentence on the
            // end would push the map's own name out of sight. It gets a
            // line of its own under the dials.
            // Picked by the host once the table is frozen, at the start:
            // the same word on every card.
            Dial::Map => tr.lobby_map_at_start.to_string(),
            Dial::Gulls => tr.gull_names[config.gulls.index()].to_string(),
            Dial::Round => tr.round_names[config.round.index()].to_string(),
            Dial::Posts => config.posts.to_string(),
            // With the level they play at, which no other row sets: step
            // 1's AI level ([`crate::app::match_setup::online_ai_level`]),
            // and played unannounced it could be one set there long ago.
            Dial::Bots => {
                let bots = dealt.map_or(config.lobby_bots, |dealt| dealt.bots);
                match bots {
                    0 => bots.to_string(),
                    _ => format!(
                        "{bots} ({})",
                        tr.bot_levels[crate::app::match_setup::online_ai_level(config).index()]
                    ),
                }
            }
            // What will be scored, which a mode the table does not fit
            // is not: the round falls back to free-for-all. The host, who
            // can dial a mode before the table fits it, is also told what
            // it dialled and when that starts.
            Dial::Teams => match dealt {
                Some(dealt) => crate::app::teams::row_value(tr, teams, dealt.seats, turns),
                None => tr.team_modes[teams.index()].to_string(),
            },
            Dial::Series => tr.mode_names[config.series.index()].to_string(),
            Dial::Welcome => tr.lobby_bots_values[usize::from(!config.bots_welcome)].to_string(),
        })
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
    ) {
        use crate::app::cycle::Cycle;
        let room = (MAX_PLAYERS as u8).saturating_sub(humans);
        match self {
            // Not a dial ([`Dial::turns`]): the map page picks it.
            Dial::Map => {}
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
                    config.lobby_bots = step;
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

/// What the terms card shows, one of four things.
pub enum TermsCard<'a> {
    /// Not at any beach yet: this machine's own setup, as a guide to what
    /// hosting would put on the air.
    Choosing {
        config: &'a MatchConfig,
        mode: crate::app::teams::TeamMode,
    },
    /// The host's own dials, read as the table they would deal now.
    Hosting {
        config: &'a MatchConfig,
        mode: crate::app::teams::TeamMode,
        dealt: Dealt,
    },
    /// A joiner the host has not sent its terms yet (the second or so
    /// before the first roster): there is no match to show, and its own
    /// setup painted in the meantime read as the host's.
    Waiting,
    /// A joiner's card: the host's dials off the roster, not its own setup
    /// screen's, since the card is meant to show the match being joined.
    Joined {
        config: MatchConfig,
        mode: crate::app::teams::TeamMode,
        dealt: Dealt,
    },
}

impl TermsCard<'_> {
    /// The dials the card reads, the team mode, and the table they deal
    /// when there is one; `None` while waiting.
    fn terms(&self) -> Option<(&MatchConfig, crate::app::teams::TeamMode, Option<Dealt>)> {
        match self {
            TermsCard::Choosing { config, mode } => Some((config, *mode, None)),
            TermsCard::Hosting {
                config,
                mode,
                dealt,
            } => Some((config, *mode, Some(*dealt))),
            TermsCard::Waiting => None,
            TermsCard::Joined {
                config,
                mode,
                dealt,
            } => Some((config, *mode, Some(*dealt))),
        }
    }
}

/// The terms card this lobby shows: the host's dials and the table they
/// deal, a joiner's host's terms off the roster, or this machine's own
/// setup (`config`, `teams`) while choosing.
pub(super) fn card_of<'a>(
    state: &'a LobbyState,
    config: &'a MatchConfig,
    teams: crate::app::teams::TeamMode,
) -> TermsCard<'a> {
    if let Some(dealt) = hosts_dealt(state, config) {
        return TermsCard::Hosting {
            config,
            mode: teams,
            dealt,
        };
    }
    let Some(joined) = state.joined() else {
        return TermsCard::Choosing {
            config,
            mode: teams,
        };
    };
    let Some(host) = joined.host_terms.as_ref() else {
        return TermsCard::Waiting;
    };
    let (mut config, mode) = crate::app::match_setup::config_from_terms(&host.terms);
    // Whether the beach takes bots is not one of the terms; a joiner reads
    // it off the beacon that listed the beach.
    config.bots_welcome = state.joined_entry().is_some_and(|entry| entry.bots);
    TermsCard::Joined {
        config,
        mode,
        // The table the round would be dealt as it sits now, off the roster.
        dealt: roster_dealt(state.table.len(), &host.terms),
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
/// without it. Only the host's: a joiner's card is the host's match, and
/// the line is nothing to do with it. Which of the host's own beaches a
/// table can play is the map page's to say, once the table is frozen.
pub(super) fn dials_note(state: &LobbyState, tr: &crate::app::i18n::Tr) -> String {
    state
        .hosted()
        .and_then(|hosted| hosted.bot_string.as_ref())
        .map(|string| format!("{}: {string}", tr.lobby_bots_dial))
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
    dial.min(MAX_PLAYERS as u8 - humans)
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
    let bots = launch_bots(humans, config.lobby_bots);
    let seats = humans + bots;
    Dealt {
        bots,
        seats,
        map: map_for(config, seats),
    }
}

/// The beach these terms describe, once the table is known.
///
/// The handcrafted arena seats four, and a fifth player arriving cannot be
/// given a castle on it; a handmade versus map seats only the tables it is
/// built for. The match-setup screen drops the seat count, which
/// online is not its to drop, so the map gives way instead.
pub fn map_for(config: &MatchConfig, seats: u8) -> crate::app::match_setup::MapChoice {
    use crate::app::match_setup::{holds, sized_for};
    match holds(config.map, seats) {
        true => config.map,
        false => sized_for(seats),
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
) {
    if state.hosting() {
        let rows = Dial::TURNS.len();
        if keys.just_pressed(KeyCode::ArrowUp) {
            state.dial = (state.dial + rows - 1) % rows;
        }
        if keys.just_pressed(KeyCode::ArrowDown) {
            state.dial = (state.dial + 1) % rows;
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
            && let Some(dial) = Dial::TURNS.get(state.dial).copied()
        {
            // Everyone already here is what the AI has to fit behind,
            // counted as the card counts them: the bots at the door too.
            let humans = humans_here(state);
            let mut teams = settings.team_mode;
            dial.turn(turn, config, &mut teams, humans);
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
                let card = TermsCard::Choosing {
                    config: &config,
                    mode: TeamMode::Solo,
                };
                let (name, value) = dial.label(lang.tr(), &card);
                assert!(!name.is_empty(), "{dial:?} in {lang:?} has no name");
                assert!(!value.is_empty(), "{dial:?} in {lang:?} has no value");
            }
        }
    }

    /// A joiner waiting for the host's first roster reads every row's
    /// name and no value, and once the terms come its map row says the
    /// host picks the map at the start.
    #[test]
    fn a_joiners_card_reads_the_hosts_terms_or_nothing() {
        let tr = &crate::app::i18n::EN;
        for dial in Dial::ALL {
            let (name, value) = dial.label(tr, &TermsCard::Waiting);
            assert!(!name.is_empty(), "{dial:?}");
            assert!(
                value.is_empty(),
                "{dial:?} reads {value:?} before any terms"
            );
        }
        let config = MatchConfig::default();
        let card = TermsCard::Joined {
            dealt: dealt(&config, 2),
            config,
            mode: TeamMode::Solo,
        };
        assert_eq!(Dial::Map.label(tr, &card).1, tr.lobby_map_at_start);
    }

    /// Every dial turns, both ways, and turning one leaves the others
    /// alone. A dial that silently moved its neighbour would be the sort of
    /// thing nobody notices until a match starts on the wrong beach.
    #[test]
    fn every_dial_turns_and_only_itself() {
        for dial in Dial::TURNS {
            for turn in [Turn::Right, Turn::Left] {
                // Off the floor, or turning the AI down is correctly a
                // no-op and the assertion below would be testing the
                // clamp rather than the dial.
                let mut config = MatchConfig {
                    lobby_bots: 1,
                    ..MatchConfig::default()
                };
                let mut teams = TeamMode::Solo;
                let before = (
                    config.map,
                    config.gulls,
                    config.round,
                    config.lobby_bots,
                    teams,
                    config.series,
                    config.posts,
                    config.bots_welcome,
                );
                // Two at the table and the AI one more: three seats, which
                // no team mode fits. The Teams dial turns all the same, so
                // a host can set up pairs before the fourth sits down.
                dial.turn(turn, &mut config, &mut teams, 2);
                let after = (
                    config.map,
                    config.gulls,
                    config.round,
                    config.lobby_bots,
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
            Dial::Bots.turn(Turn::Right, &mut config, &mut teams, 4);
        }
        assert_eq!(config.lobby_bots, MAX_PLAYERS as u8 - 4);
        // And back down to none, not below it.
        for _ in 0..10 {
            Dial::Bots.turn(Turn::Left, &mut config, &mut teams, 4);
        }
        assert_eq!(config.lobby_bots, 0);
        // A full table leaves the AI nothing at all.
        Dial::Bots.turn(Turn::Right, &mut config, &mut teams, MAX_PLAYERS as u8);
        assert_eq!(config.lobby_bots, 0, "six people is six people");
        // And a press there changes nothing the card shows, so it changes
        // nothing: the host's dial is still what it was when the next
        // table sits down.
        config.lobby_bots = 3;
        for turn in [Turn::Right, Turn::Left] {
            Dial::Bots.turn(turn, &mut config, &mut teams, MAX_PLAYERS as u8);
            assert_eq!(config.lobby_bots, 3, "a dead press {turn:?}");
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
        for dial in Dial::TURNS {
            dial.turn(Turn::Right, &mut config, &mut teams, 2);
        }
        let terms = crate::app::match_setup::terms(&config, teams, 7);
        assert_eq!(terms.map, config.map.index() as u8);
        assert_eq!(terms.gulls, config.gulls.index() as u8);
        assert_eq!(terms.round, config.round.index() as u8);
        assert_eq!(terms.bots, config.lobby_bots);
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
            lobby_bots: 4,
            ..MatchConfig::default()
        };
        let dealt = dealt(&config, 5);
        assert_eq!((dealt.bots, dealt.seats), (1, 6));
        let row = |dial: Dial, config: &MatchConfig| {
            let card = TermsCard::Hosting {
                config,
                mode: TeamMode::Solo,
                dealt,
            };
            dial.label(&EN, &card).1
        };
        assert_eq!(row(Dial::Bots, &config), "1 (normal)");
        // The map is picked at the start, for the table as it is then.
        assert_eq!(row(Dial::Map, &config), EN.lobby_map_at_start);
        assert_eq!(
            dealt.map,
            MapChoice::GenLarge,
            "five aboard outgrow Classic"
        );
        let mut teams = TeamMode::Solo;
        Dial::Bots.turn(Turn::Left, &mut config, &mut teams, 5);
        assert_eq!(config.lobby_bots, 0, "one less than the one shown");
    }

    /// The line under the dials is the host's, and only says the string a
    /// bot connects with: which beaches a table can play is the map page's
    /// to say, once the table is frozen at the start.
    #[test]
    fn the_note_under_the_dials_is_the_bots_string() {
        use crate::app::i18n::EN;
        let mut state = LobbyState::default();
        assert_eq!(dials_note(&state, &EN), "", "not hosting");
        state.standing = hosting_lobby();
        assert_eq!(dials_note(&state, &EN), "");
        if let Standing::Hosting(hosted) = &mut state.standing {
            hosted.bot_string = Some("pinch://here".into());
        }
        let note = dials_note(&state, &EN);
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
        let mut config = MatchConfig::default();
        let three = dealt(&config, 3);
        assert_eq!(three.seats, 3);
        let card = MatchConfig::default();
        let row = |mode: TeamMode, dealt: Dealt, turns: bool| {
            let card = match turns {
                true => TermsCard::Hosting {
                    config: &card,
                    mode,
                    dealt,
                },
                false => TermsCard::Joined {
                    config: MatchConfig::default(),
                    mode,
                    dealt,
                },
            };
            Dial::Teams.label(&EN, &card).1
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
            Dial::Teams.turn(Turn::Right, &mut config, &mut teams, 3);
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
            let card = TermsCard::Hosting {
                config: &MatchConfig::default(),
                mode: TeamMode::Pairs,
                dealt: three,
            };
            let row = Dial::Teams.label(tr, &card).1;
            assert!(!row.contains('{'), "{lang:?}: {row}");
            assert!(row.contains('4'), "{lang:?}: {row}");
            assert!(
                row.contains(tr.team_mode_names[TeamMode::Pairs.index()]),
                "{lang:?}: {row}"
            );
        }
    }
}
