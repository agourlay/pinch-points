//! Best-of-three and best-of-five tournaments: a local series of versus
//! rounds on rotating maps.
//! Round wins accumulate here; the interlude screen bridges the rounds and
//! the results card grows a series block (and a champion headline once the
//! series is decided).

use crate::app::cycle::Cycle;
use crate::app::i18n::fill;
use crate::app::match_setup::MatchConfig;
use crate::app::menu_ui;
use crate::app::palette;
use crate::app::settings::GameSettings;
use crate::app::side_panels::leading_seats;
use crate::app::teams::TeamMode;
use crate::app::{Screen, Seats, Sim};
use crate::sim::MAX_PLAYERS;
use bevy::prelude::*;

/// How long a series runs: the three positions of the match screen's mode
/// dial.
///
/// One dial rather than a flag and a length, because the lengths differ in
/// nothing else: a match is one round, three, or five, and everything that
/// follows from that is arithmetic on [`SeriesLength::rounds`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SeriesLength {
    #[default]
    Single,
    BestOfThree,
    BestOfFive,
}

impl SeriesLength {
    pub const ALL: [SeriesLength; 3] = [
        SeriesLength::Single,
        SeriesLength::BestOfThree,
        SeriesLength::BestOfFive,
    ];

    /// Whether a series is being played at all. A single round is on the
    /// same dial because that is how the screen offers it, but it starts no
    /// tournament and shows no standings.
    pub fn is_series(self) -> bool {
        self != SeriesLength::Single
    }

    /// Rounds in a full series: the most that will be played.
    pub fn rounds(self) -> u8 {
        match self {
            SeriesLength::Single => 1,
            SeriesLength::BestOfThree => 3,
            SeriesLength::BestOfFive => 5,
        }
    }

    /// Rounds that take it, ending the series early: a majority, which is
    /// what "best of" means. Derived rather than written down twice, so a
    /// length added to the dial cannot disagree with itself.
    pub fn target(self) -> u8 {
        self.rounds() / 2 + 1
    }
}

/// Where a series stands, if there is one.
///
/// One value rather than the `active` and `finished` flags it replaces:
/// four readers each re-derived "running" as `active && !finished`, and a
/// pair of flags admits a fourth state (finished but not active) that
/// nothing means.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SeriesState {
    /// A single round, or no match at all: nothing to tally.
    #[default]
    Off,
    /// Rounds still to play.
    Running,
    /// Decided; the next Enter leaves for the menu.
    Decided,
}

#[derive(Resource, Default)]
pub struct Tournament {
    pub state: SeriesState,
    /// 1-based current round.
    pub round: u8,
    pub wins: [u8; MAX_PLAYERS],
    /// How long this one runs. Read only while `state` is not `Off`: a table that is
    /// not in a series has no length to speak of, and the default sits at
    /// [`SeriesLength::Single`] to say so.
    pub length: SeriesLength,
}

/// Who a series belongs to. Rounds are awarded per *seat*, because that is
/// how the wins are stored, but a team series is won by the team: both its
/// members hold the same tally, so asking which seat leads sees a tie and
/// answers nobody. The card needs to know which question was asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Champion {
    Seat(u8),
    Team(u8),
}

impl Champion {
    /// Whether `seat` is one of the winners: itself, or a member of the
    /// winning team.
    pub fn claims(self, seat: u8, mode: TeamMode) -> bool {
        match self {
            Champion::Seat(s) => s == seat,
            Champion::Team(t) => mode.team_of(seat) == t,
        }
    }
}

impl Tournament {
    pub fn start(length: SeriesLength) -> Self {
        Self::taken_up(length, 1, [0; MAX_PLAYERS])
    }

    /// The tournament a peer arms from the host's invitation: the series
    /// length the terms carry, at the round and tally the wire says. A
    /// single round arms nothing, so the host and every joiner run this
    /// same function and cannot disagree about whether there is a series;
    /// a peer admitted mid-series takes up the standing rather than
    /// starting its own tally at zero.
    pub fn taken_up(length: SeriesLength, round: u8, wins: [u8; MAX_PLAYERS]) -> Self {
        match length.is_series() {
            true => Tournament {
                state: SeriesState::Running,
                round: round.max(1),
                wins,
                length,
            },
            false => Tournament::default(),
        }
    }

    /// [`Tournament::taken_up`] read straight off the terms the wire
    /// carries.
    pub fn from_terms(
        terms: crate::transport::MatchTerms,
        round: u8,
        wins: [u8; MAX_PLAYERS],
    ) -> Self {
        Self::taken_up(
            SeriesLength::from_index(usize::from(terms.series)),
            round,
            wins,
        )
    }

    /// Whether a series is on at all, running or decided: the results card
    /// shows the standings either way.
    pub fn in_series(&self) -> bool {
        self.state != SeriesState::Off
    }

    /// Whether there are rounds still to play.
    pub fn is_running(&self) -> bool {
        self.state == SeriesState::Running
    }

    /// Whether the series has been decided.
    pub fn is_decided(&self) -> bool {
        self.state == SeriesState::Decided
    }

    /// The unique holder of the most round wins, if there is one. Only
    /// meaningful free-for-all; [`Tournament::winner`] is the one to ask.
    fn champion(&self) -> Option<u8> {
        crate::app::side_panels::unique_max(&self.wins).map(|s| s as u8)
    }

    /// Who took the series, under the mode it was played in. In team play
    /// the tally is read one seat per team (every member is awarded the
    /// same round) so the comparison is between teams and a shared tally
    /// is a win, not a tie.
    pub fn winner(&self, mode: TeamMode, seats: u8) -> Option<Champion> {
        if mode == TeamMode::Solo {
            return self.champion().map(Champion::Seat);
        }
        let tallies: Vec<u8> = (0..mode.teams(seats))
            .map(|team| {
                mode.seats_of(team, seats)
                    .first()
                    .map_or(0, |&seat| self.wins[usize::from(seat)])
            })
            .collect();
        crate::app::side_panels::unique_max(&tallies).map(|team| Champion::Team(team as u8))
    }
}

/// Award the finished round to its unique leader (or, in team play, to
/// every seat on the leading team) and decide the series. Runs before the results card spawns.
pub fn record_series_round(
    sim: Res<Sim>,
    seats: Res<Seats>,
    settings: Res<GameSettings>,
    online: Res<crate::app::net::Online>,
    mut tournament: ResMut<Tournament>,
) {
    if !tournament.is_running() {
        return;
    }
    // Every peer counts the round it just watched, so its results card is
    // right even for the final round, which is followed by no invitation.
    // Online, a re-deal of the seats between rounds would leave this tally
    // pinned to the old chairs; the host's next invitation carries the
    // authoritative wins moved onto the new ones, and `enter_interlude`
    // overwrites this with them, so a mid-series seat change corrects
    // itself without this having to know the mapping.
    let mode = crate::app::teams::in_play(&settings, &online, seats.0);
    let leaders = leading_seats(sim.0.scores(), seats.0, mode);
    for (seat, led) in leaders.iter().enumerate() {
        if *led {
            tournament.wins[seat] += 1;
        }
    }
    let best = *tournament.wins.iter().max().unwrap_or(&0);
    if best >= tournament.length.target() || tournament.round >= tournament.length.rounds() {
        tournament.state = SeriesState::Decided;
    }
}

/// The series tally, one row per contender: who, and a star per round won.
/// Solo rows are seats; team rows are teams, because a pair holding two
/// rounds between them has won two, not four.
pub fn standings(
    settings: &GameSettings,
    names: &crate::app::SeatNames,
    tournament: &Tournament,
    mode: TeamMode,
    seats: u8,
) -> Vec<(String, Color)> {
    (0..mode.teams(seats))
        .map(|team| {
            let face = crate::app::teams::face_of(mode, team, seats);
            let wins = tournament.wins[usize::from(face)];
            let who = if mode == TeamMode::Solo {
                names.label(settings.tr(), face)
            } else {
                crate::app::teams::label(settings, names, mode, team, seats)
            };
            (
                format!("{who}  {}", "*".repeat(usize::from(wins))),
                palette::player_color(face),
            )
        })
        .collect()
}

/// The champion's name for the headline, and the seat whose colour it wears.
pub fn champion_name(
    settings: &GameSettings,
    names: &crate::app::SeatNames,
    mode: TeamMode,
    champion: Champion,
    seats: u8,
) -> (String, u8) {
    match champion {
        Champion::Seat(seat) => (names.label(settings.tr(), seat), seat),
        Champion::Team(team) => (
            crate::app::teams::label(settings, names, mode, team, seats),
            crate::app::teams::face_of(mode, team, seats),
        ),
    }
}

/// Marker for the between-rounds interlude card.
#[derive(Component)]
pub struct InterludeUi;

/// Auto-advance clock on the interlude card.
#[derive(Component)]
pub struct InterludeTimer(pub Timer);

/// Entering the interlude advances the series bookkeeping (next round,
/// rotated map, config re-armed) and shows the standings for a beat.
#[allow(clippy::too_many_arguments)]
pub fn enter_interlude(
    mut commands: Commands,
    settings: Res<GameSettings>,
    names: Res<crate::app::SeatNames>,
    seats: Res<Seats>,
    mut online: ResMut<crate::app::net::Online>,
    mut config: ResMut<MatchConfig>,
    beaches: Res<crate::app::match_setup::CustomBeaches>,
    mut tournament: ResMut<Tournament>,
) {
    let mode = crate::app::teams::in_play(&settings, &online, seats.0.max(2));
    // The session is through the doorway; the flag has done its work of
    // carrying it past `end_versus`. Online, the map and the seed are the
    // host's to say and arrive in the terms; the config steps below are
    // for a local series, and `load_versus` ignores them for an online one.
    //
    // The round number and the tally are the host's word online: it re-deals
    // them to the next round's seats and sends them out, so both are adopted
    // here rather than counted locally, which would advance the round twice
    // and credit a moved seat's wins to whoever now sits in it.
    match &mut online.0 {
        Some(session) => {
            session.next_round = false;
            if let Some((round, wins)) = session.series_standing.take() {
                tournament.round = round;
                tournament.wins = wins;
            }
        }
        None => tournament.round += 1,
    }
    // The dial's own step, with the dial's own guards: past the shelf when
    // nothing on it seats the table, past the four-castle beaches when
    // five or six are sitting.
    crate::app::match_setup::next_map(&mut config, &beaches);
    config.armed = true;
    let tr = settings.tr();
    commands
        .spawn((
            InterludeUi,
            InterludeTimer(Timer::from_seconds(2.4, TimerMode::Once)),
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(12.0),
                ..menu_ui::centred_overlay()
            },
            BackgroundColor(palette::CARD_BG),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(fill(tr.tour_round, &[("n", &tournament.round.to_string())])),
                TextFont {
                    font_size: FontSize::Px(34.0),
                    ..default()
                },
                TextColor(palette::GOLD),
            ));
            for (line, color) in standings(&settings, &names, &tournament, mode, seats.0.max(2)) {
                card.spawn((
                    Text::new(line),
                    TextFont {
                        font_size: FontSize::Px(24.0),
                        ..default()
                    },
                    TextColor(color),
                ));
            }
        });
}

/// The interlude rolls into the next round on its own (or on Enter).
pub fn interlude_tick(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut timers: Query<&mut InterludeTimer>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    for mut timer in &mut timers {
        timer.0.tick(time.delta());
        if timer.0.is_finished() || keys.just_pressed(KeyCode::Enter) {
            next_screen.set(Screen::Versus);
        }
    }
}

/// Back at the menu, any running series is abandoned.
pub fn reset_on_menu(mut tournament: ResMut<Tournament>) {
    *tournament = Tournament::default();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both members of the leading pair hold the same tally, so a per-seat
    /// search sees a tie and only the team search finds the winner.
    #[test]
    fn a_team_series_is_won_by_the_team() {
        let mut t = Tournament::start(SeriesLength::BestOfFive);
        // Two rounds to the pair {0,1}, one to the pair {2,3}.
        t.wins = [2, 2, 1, 1, 0, 0];
        assert_eq!(t.champion(), None, "the seats are tied, and always will be");
        assert_eq!(t.winner(TeamMode::Pairs, 4), Some(Champion::Team(0)));
        assert_eq!(t.winner(TeamMode::Solo, 4), None);
        // A drawn series still crowns nobody.
        t.wins = [2, 2, 2, 2, 0, 0];
        assert_eq!(t.winner(TeamMode::Pairs, 4), None);
        // Trios read the interleaved halves, not the blocks.
        t.wins = [1, 3, 1, 3, 1, 3];
        assert_eq!(t.winner(TeamMode::Trios, 6), Some(Champion::Team(1)));
    }

    /// The series bookkeeping itself, which nothing exercised: a round goes
    /// to its leader, three of them end the series early, and a team round
    /// is awarded to every seat on the winning team.
    #[test]
    fn three_rounds_take_the_series() {
        use crate::sim::classic_arena;

        let mut app = App::new();
        app.insert_resource(Sim(classic_arena(false, 4)));
        app.insert_resource(Seats(4));
        app.insert_resource(GameSettings::default());
        app.init_resource::<crate::app::net::Online>();
        app.insert_resource(Tournament::start(SeriesLength::BestOfFive));
        app.add_systems(Update, record_series_round);

        // Seat 1 banks the most, three rounds running.
        {
            let mut sim = app.world_mut().resource_mut::<Sim>();
            for (seat, score) in [(0, 0), (1, 9), (2, 3), (3, 1)] {
                sim.0.set_score(seat, score);
            }
        }
        for round in 1..=3u8 {
            app.world_mut().resource_mut::<Tournament>().round = round;
            app.update();
        }
        let tour = app.world().resource::<Tournament>();
        assert_eq!(tour.wins, [0, 3, 0, 0, 0, 0]);
        assert!(
            tour.is_decided(),
            "first to three ends it before round five"
        );
        assert_eq!(tour.winner(TeamMode::Solo, 4), Some(Champion::Seat(1)));

        // A finished series stops counting, however many more rounds run.
        app.update();
        assert_eq!(app.world().resource::<Tournament>().wins[1], 3);

        // In pairs the round goes to both members of the leading pair.
        app.insert_resource(Tournament::start(SeriesLength::BestOfFive));
        app.world_mut().resource_mut::<GameSettings>().team_mode = TeamMode::Pairs;
        app.update();
        assert_eq!(
            app.world().resource::<Tournament>().wins,
            [1, 1, 0, 0, 0, 0],
            "the pair map is {{0,1}} {{2,3}}: seat 1's nine takes the round for \
             seat 0 too, who banked nothing"
        );
    }

    /// The two lengths are one dial and one piece of arithmetic: a best of
    /// n is taken by a majority of n, and a single round is neither.
    #[test]
    fn best_of_n_is_taken_by_a_majority_of_n() {
        assert_eq!(SeriesLength::BestOfThree.rounds(), 3);
        assert_eq!(SeriesLength::BestOfThree.target(), 2, "two of three");
        assert_eq!(SeriesLength::BestOfFive.rounds(), 5);
        assert_eq!(SeriesLength::BestOfFive.target(), 3, "three of five");
        assert!(!SeriesLength::Single.is_series());
        assert!(SeriesLength::BestOfThree.is_series());
        assert!(SeriesLength::BestOfFive.is_series());
    }

    /// A short series is over a round sooner than a long one, on the same
    /// tally. The long series ran through `three_rounds_take_the_series`
    /// above; this is the one the new dial position adds.
    #[test]
    fn two_rounds_take_a_best_of_three() {
        let decided = |length: SeriesLength, wins: [u8; MAX_PLAYERS], round: u8| {
            let mut t = Tournament::start(length);
            t.wins = wins;
            t.round = round;
            let best = *t.wins.iter().max().unwrap_or(&0);
            best >= t.length.target() || t.round >= t.length.rounds()
        };
        assert!(
            decided(SeriesLength::BestOfThree, [2, 0, 0, 0, 0, 0], 2),
            "two rounds take a best of three"
        );
        assert!(
            !decided(SeriesLength::BestOfFive, [2, 0, 0, 0, 0, 0], 2),
            "and leave a best of five still running"
        );
        // A tally that never reaches the target still ends at the last round.
        assert!(
            decided(SeriesLength::BestOfThree, [1, 1, 1, 0, 0, 0], 3),
            "the third round ends it however it stands"
        );
    }

    #[test]
    fn champion_needs_a_unique_top() {
        let mut t = Tournament::start(SeriesLength::BestOfFive);
        assert_eq!(t.champion(), None, "no wins yet");
        t.wins = [2, 1, 0, 0, 0, 0];
        assert_eq!(t.champion(), Some(0));
        t.wins = [2, 2, 0, 0, 0, 0];
        assert_eq!(t.champion(), None, "tied series has no champion");
    }

    /// The wire carries the series as a dial index, and the app reads it
    /// through [`MatchTerms::is_series`]. The two must agree for every
    /// position of the dial: they did not, once, and a best-of-five went
    /// out to every joiner as a single round.
    #[test]
    fn the_wire_flag_agrees_with_the_dial() {
        for (index, length) in SeriesLength::ALL.iter().enumerate() {
            let terms = crate::transport::MatchTerms {
                series: index as u8,
                ..Default::default()
            };
            assert_eq!(terms.is_series(), length.is_series(), "{length:?}");
            let armed = Tournament::from_terms(terms, 1, [0; MAX_PLAYERS]);
            assert_eq!(
                armed.in_series(),
                length.is_series(),
                "{length:?} arms a tournament"
            );
            assert_eq!(armed.length, *length);
        }
        // Taken up mid-series, the standing is the host's, not a fresh one.
        let mut wins = [0; MAX_PLAYERS];
        wins[2] = 2;
        let late = Tournament::taken_up(SeriesLength::BestOfFive, 4, wins);
        assert_eq!((late.round, late.wins[2]), (4, 2));
        assert!(!Tournament::taken_up(SeriesLength::Single, 4, wins).in_series());
    }
}
