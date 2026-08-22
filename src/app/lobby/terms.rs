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
    Bots,
    Teams,
    Series,
}

impl Dial {
    pub const ALL: [Dial; 6] = [
        Dial::Map,
        Dial::Gulls,
        Dial::Round,
        Dial::Bots,
        Dial::Teams,
        Dial::Series,
    ];

    /// The row as it reads: what it sets, and what it is set to.
    pub fn label(
        self,
        tr: &crate::app::i18n::Tr,
        config: &MatchConfig,
        teams: crate::app::teams::TeamMode,
        beaches: &crate::app::match_setup::CustomBeaches,
    ) -> (&'static str, String) {
        use crate::app::cycle::Cycle;
        match self {
            // The note about beaches this table is too big for is not
            // appended here: this value cell clips what overruns it, so a
            // sentence on the end would push the map's own name out of
            // sight. It gets a line of its own under the dials.
            Dial::Map => (
                tr.match_map,
                crate::app::match_setup::map_label(config, tr, beaches),
            ),
            Dial::Gulls => (
                tr.match_gulls,
                tr.gull_names[config.gulls.index()].to_string(),
            ),
            Dial::Round => (
                tr.match_round,
                tr.round_names[config.round.index()].to_string(),
            ),
            Dial::Bots => (tr.match_ai, config.bots.to_string()),
            Dial::Teams => (tr.set_versus_mode, tr.team_modes[teams.index()].to_string()),
            Dial::Series => (
                tr.match_mode,
                tr.mode_names[config.series.index()].to_string(),
            ),
        }
    }

    /// Turn it. `humans` is how many people are at the table, and what the
    /// AI has to fit behind: a chair taken by a player is not one the
    /// host can fill with a bot.
    pub fn turn(
        self,
        right: bool,
        config: &mut MatchConfig,
        teams: &mut crate::app::teams::TeamMode,
        humans: u8,
        beaches: &crate::app::match_setup::CustomBeaches,
    ) {
        use crate::app::cycle::Cycle;
        let room = (MAX_PLAYERS as u8).saturating_sub(humans);
        match self {
            Dial::Map => crate::app::match_setup::cycle_map(config, right, beaches),
            Dial::Gulls => config.gulls = config.gulls.cycled(right),
            Dial::Round => config.round = config.round.cycled(right),
            Dial::Bots => {
                let step = i32::from(config.bots) + if right { 1 } else { -1 };
                config.bots = step.clamp(0, i32::from(room)) as u8;
            }
            Dial::Teams => *teams = teams.cycled(right),
            Dial::Series => config.series = config.series.cycled(right),
        }
    }
}

/// The beach these terms describe, once the table is known.
///
/// The handcrafted arena seats four, and a fifth player arriving cannot be
/// given a castle on it. The match-setup screen keeps that straight by
/// dropping the seat count, which online is not its to drop. So the map
/// gives way instead: whoever turned up keeps their chair.
pub fn map_for(config: &MatchConfig, seats: u8) -> crate::app::match_setup::MapChoice {
    use crate::app::match_setup::{CLASSIC_SEATS, MapChoice, WIDE_ENOUGH};
    match seats > CLASSIC_SEATS && config.map.size().0 < WIDE_ENOUGH {
        true => MapChoice::GenXl,
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
    config: &mut MatchConfig,
    state: &mut LobbyState,
    beaches: &crate::app::match_setup::CustomBeaches,
) {
    if state.standing() == Standing::Hosting {
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
            (true, false) => Some(true),
            (false, true) => Some(false),
            _ => None,
        };
        if let Some(right) = turn
            && let Some(dial) = Dial::ALL.get(state.dial).copied()
        {
            // The people already here are what the AI has to fit behind.
            let humans = 1 + state.players_aboard() as u8;
            let mut teams = settings.team_mode;
            dial.turn(right, config, &mut teams, humans, beaches);
            if teams != settings.team_mode {
                settings.team_mode = teams;
                settings.save();
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
                let (name, value) =
                    dial.label(lang.tr(), &config, TeamMode::Solo, &Default::default());
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
            for right in [true, false] {
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
                );
                dial.turn(right, &mut config, &mut teams, 2, &Default::default());
                let after = (
                    config.map,
                    config.gulls,
                    config.round,
                    config.bots,
                    teams,
                    config.series,
                );
                assert_ne!(before, after, "{dial:?} turned {right} and did nothing");
                // Exactly one of the six moved.
                let moved = [
                    before.0 != after.0,
                    before.1 != after.1,
                    before.2 != after.2,
                    before.3 != after.3,
                    before.4 != after.4,
                    before.5 != after.5,
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
            Dial::Bots.turn(true, &mut config, &mut teams, 4, &Default::default());
        }
        assert_eq!(config.bots, MAX_PLAYERS as u8 - 4);
        // And back down to none, not below it.
        for _ in 0..10 {
            Dial::Bots.turn(false, &mut config, &mut teams, 4, &Default::default());
        }
        assert_eq!(config.bots, 0);
        // A full table leaves the AI nothing at all.
        Dial::Bots.turn(
            true,
            &mut config,
            &mut teams,
            MAX_PLAYERS as u8,
            &Default::default(),
        );
        assert_eq!(config.bots, 0, "six people is six people");
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
            dial.turn(true, &mut config, &mut teams, 2, &Default::default());
        }
        let terms = crate::app::match_setup::terms(&config, teams, 7);
        assert_eq!(terms.map, config.map.index() as u8);
        assert_eq!(terms.gulls, config.gulls.index() as u8);
        assert_eq!(terms.round, config.round.index() as u8);
        assert_eq!(terms.bots, config.bots);
        assert_eq!(terms.teams, teams.index() as u8);
        assert_eq!(terms.series, config.series.index() as u8);
        assert_eq!(terms.seed, 7);
    }
}
