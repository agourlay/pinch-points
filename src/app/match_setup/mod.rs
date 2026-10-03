//! Match setup: the intermediary screen between the menu and a local versus
//! round. Pick the seat count, how many seats the AI plays, the map, gull
//! pressure, round length, and how many arrows each player may have
//! standing; Enter starts the match.

use crate::app::cycle::{Cycle, Turn};
use crate::app::i18n::fill;
use crate::app::palette;
use crate::app::{Screen, SeatController};
use crate::sim::BotLevel;
use crate::sim::{MAX_PLAYERS, MAX_SIGNPOSTS_PER_PLAYER};
use crate::transport::MatchTerms;
use bevy::prelude::*;

mod beaches;
mod board;

pub use beaches::{
    Beach, CustomBeaches, beach_bytes, beach_from, beaches_note, cycle_map, map_label,
    refresh_custom_beaches,
};
pub use board::{board_for, board_from, bot_seats_from};

mod screen;
pub use screen::*;

/// The playable beach for a local match.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MapChoice {
    #[default]
    Classic,
    GenSmall,
    GenClassic,
    GenLarge,
    GenXl,
    /// Open ocean: a generated beach with no edges. Appended rather than
    /// slotted in beside its size, because the index is what a `Start`
    /// datagram carries, and reordering would silently change which beach a
    /// host is inviting everyone to.
    GenOcean,
    /// A beach somebody built in the editor. Which one is not on the wire:
    /// the beach itself rides along with the invitation, because no peer
    /// but the host has the file it came from.
    Custom,
}

impl MapChoice {
    pub const ALL: [MapChoice; 7] = [
        MapChoice::Classic,
        MapChoice::GenSmall,
        MapChoice::GenClassic,
        MapChoice::GenLarge,
        MapChoice::GenXl,
        MapChoice::GenOcean,
        MapChoice::Custom,
    ];

    pub fn size(self) -> (u8, u8) {
        match self {
            MapChoice::Classic | MapChoice::GenClassic => (12, 9),
            MapChoice::GenSmall => (9, 7),
            MapChoice::GenLarge => (16, 11),
            MapChoice::GenXl => (20, 13),
            MapChoice::GenOcean => (16, 11),
            // Whatever the level says; the board arrives built, so this is
            // only used to decide whether a table fits, and a handmade
            // beach is offered only when it does.
            MapChoice::Custom => (20, 13),
        }
    }

    /// Whether the beach has edges. Open ocean does not: a creature walking
    /// off one side comes back on the other, the same wrap the campaign
    /// teaches at level 26.
    pub fn wraps(self) -> bool {
        matches!(self, MapChoice::GenOcean)
    }
}

/// How aggressive the ambient gull spawner is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GullPressure {
    Calm,
    #[default]
    Normal,
    Frenzy,
}

impl GullPressure {
    pub const ALL: [GullPressure; 3] = [
        GullPressure::Calm,
        GullPressure::Normal,
        GullPressure::Frenzy,
    ];

    pub fn period(self) -> u32 {
        match self {
            GullPressure::Calm => 340,
            GullPressure::Normal => 240,
            GullPressure::Frenzy => 150,
        }
    }
}

/// Round length.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RoundLength {
    Short,
    #[default]
    Standard,
    Long,
}

impl RoundLength {
    pub const ALL: [RoundLength; 3] =
        [RoundLength::Short, RoundLength::Standard, RoundLength::Long];

    pub fn ticks(self) -> u32 {
        match self {
            RoundLength::Short => 2 * 60 * crate::sim::TICKS_PER_SECOND,
            RoundLength::Standard => 3 * 60 * crate::sim::TICKS_PER_SECOND,
            RoundLength::Long => 5 * 60 * crate::sim::TICKS_PER_SECOND,
        }
    }
}

impl Cycle for MapChoice {
    const VARIANTS: &'static [Self] = &Self::ALL;
}
impl Cycle for GullPressure {
    const VARIANTS: &'static [Self] = &Self::ALL;
}
impl Cycle for RoundLength {
    const VARIANTS: &'static [Self] = &Self::ALL;
}
impl Cycle for BotLevel {
    const VARIANTS: &'static [Self] = &BOT_LEVELS;
}
impl Cycle for crate::app::tournament::SeriesLength {
    const VARIANTS: &'static [Self] = &crate::app::tournament::SeriesLength::ALL;
}

/// One match-setup row; input and render both match on this. The AI-level
/// rows are per seat, one for each seat the AI can take, and the ones
/// beyond the current AI count are hidden.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Row {
    Players,
    Bots,
    /// Difficulty for the `n`-th AI seat, counting down from the top seat.
    BotLevel(u8),
    Map,
    Gulls,
    Round,
    /// How many arrows each player may have standing at once.
    Posts,
    Mode,
    /// What to call seat `n`. One row per seat in the match; typing into it
    /// renames that seat everywhere the game mentions it.
    Name(u8),
}

/// The range of the arrows dial. The default is the versus rule's three;
/// one is a knife fight, and past six a beach is more arrow than sand.
pub const POSTS_RANGE: std::ops::RangeInclusive<u8> = 1..=6;

/// The arrows a set of terms allows, inside the dial's range: the byte is
/// whatever a host put there, and a cap of zero is a round nobody can play.
/// Every peer clamps the same byte the same way, so they still agree.
pub fn posts_from(terms: &MatchTerms) -> u8 {
    terms.posts.clamp(*POSTS_RANGE.start(), *POSTS_RANGE.end())
}

/// The most seats the AI can hold: everyone but one human.
pub const MAX_BOTS: usize = MAX_PLAYERS - 1;

/// Seats a four-castle beach holds. Past this the match needs a generated
/// arena wide enough for the two long-edge castles.
pub const CLASSIC_SEATS: u8 = 4;
/// The narrowest board that seats five or six: the two extra castles sit
/// mid-edge, and they need room between the corners and the spawner holes.
pub const WIDE_ENOUGH: u8 = 16;

impl Row {
    pub const ALL: [Row; 7 + MAX_BOTS + MAX_PLAYERS] = [
        Row::Players,
        Row::Bots,
        Row::BotLevel(0),
        Row::BotLevel(1),
        Row::BotLevel(2),
        Row::BotLevel(3),
        Row::BotLevel(4),
        Row::Map,
        Row::Gulls,
        Row::Round,
        Row::Posts,
        Row::Mode,
        // Last on the list: the rows above are the ones every match needs,
        // and Enter on a name row types instead of starting the match.
        Row::Name(0),
        Row::Name(1),
        Row::Name(2),
        Row::Name(3),
        Row::Name(4),
        Row::Name(5),
    ];
}

const ROWS: usize = Row::ALL.len();

/// The configured local match. Persists between rounds so "again!" needs no
/// re-setup; `armed` is set when the player launches from this screen and
/// consumed by `load_versus`.
#[derive(Resource)]
pub struct MatchConfig {
    pub seats: u8,
    /// How many of the top seats are not played at this machine's keys.
    /// A count rather than a flag a seat, because the people sit low and
    /// everything else high: the keyboard and the pads deal the human
    /// seats from P1 upward.
    pub bots: u8,
    /// What holds each of those top seats: the AI at its own difficulty, so
    /// one player can spar with a fierce rival and an easy one at the same
    /// table. Only the seats `bots` counts are read; the rest keep their
    /// choice for when the count comes back up to them.
    pub controllers: [SeatController; MAX_PLAYERS],
    pub map: MapChoice,
    /// Which handmade beach, when `map` is [`MapChoice::Custom`]. An index
    /// into [`CustomBeaches`], which is read fresh each time the dial is
    /// turned: the editor may have saved another one since.
    pub custom: usize,
    pub gulls: GullPressure,
    pub round: RoundLength,
    /// Arrows each player may have standing. Placing one more takes the
    /// oldest away, as the versus rule always has; only the count moves.
    pub posts: u8,
    /// One round, best of three, or best of five.
    pub series: crate::app::tournament::SeriesLength,
    /// True when the next versus round should be built from this config.
    pub armed: bool,
    /// A beach hosted on these dials takes bots ("Bots welcome"). The
    /// host's to turn in the lobby, said in the beacon so the list shows
    /// it before anyone walks up.
    pub bots_welcome: bool,
}

impl Default for MatchConfig {
    fn default() -> Self {
        MatchConfig {
            seats: 2,
            bots: 0,
            controllers: [SeatController::Ai(BotLevel::Normal); MAX_PLAYERS],
            map: MapChoice::Classic,
            custom: 0,
            gulls: GullPressure::Normal,
            round: RoundLength::Standard,
            posts: MAX_SIGNPOSTS_PER_PLAYER as u8,
            series: crate::app::tournament::SeriesLength::Single,
            armed: false,
            bots_welcome: true,
        }
    }
}

impl MatchConfig {
    /// Who decides for `seat`: a person here for the low seats, and the
    /// seat's own choice for the top `bots`.
    pub fn controller(&self, seat: u8) -> SeatController {
        if seat < self.seats.saturating_sub(self.bots) {
            SeatController::Local
        } else {
            self.controllers[usize::from(seat).min(MAX_PLAYERS - 1)]
        }
    }

    /// The AI difficulty `seat` would play at: its own, or the default for
    /// a seat set to something other than the AI.
    pub fn level(&self, seat: u8) -> BotLevel {
        self.controllers[usize::from(seat).min(MAX_PLAYERS - 1)]
            .ai()
            .unwrap_or_default()
    }

    /// The daily challenge's table: today's arena, three fierce bots, a
    /// standard round. Its own value rather than a write into the player's
    /// config, which would come back from the daily showing this on the
    /// setup screen as what they had chosen.
    pub fn daily() -> Self {
        MatchConfig {
            seats: 4,
            bots: 3,
            controllers: [SeatController::Ai(BotLevel::Hard); MAX_PLAYERS],
            map: MapChoice::GenClassic,
            custom: 0,
            gulls: GullPressure::Normal,
            round: RoundLength::Standard,
            posts: MAX_SIGNPOSTS_PER_PLAYER as u8,
            series: crate::app::tournament::SeriesLength::Single,
            armed: true,
            bots_welcome: false,
        }
    }
}

pub const BOT_LEVELS: [BotLevel; 3] = [BotLevel::Easy, BotLevel::Normal, BotLevel::Hard];

/// The match being set up: its dials, and the shelf of handmade beaches
/// the map dial walks.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Dials<'w> {
    pub config: ResMut<'w, MatchConfig>,
    pub beaches: Res<'w, CustomBeaches>,
}

/// The wire form of this screen's choices, for a host to send and every peer
/// to build the same beach from. `teams` and `seed` are not on this screen:
/// teams is a setting, and the seed is drawn when the match launches.
pub fn terms(config: &MatchConfig, teams: crate::app::teams::TeamMode, seed: u64) -> MatchTerms {
    MatchTerms {
        bots: config.bots,
        // Every AI seat plays at the top AI seat's level: the wire carries
        // one difficulty, and a lobby has no per-seat rows to fill anyway.
        // A seat dialled to a bot on the couch has no level to give.
        bot_level: config.controllers[..usize::from(config.seats).min(MAX_PLAYERS)]
            .iter()
            .rev()
            .find_map(|controller| controller.ai())
            .unwrap_or_default()
            .index() as u8,
        map: config.map.index() as u8,
        gulls: config.gulls.index() as u8,
        round: config.round.index() as u8,
        teams: teams.index() as u8,
        seed,
        series: config.series.index() as u8,
        posts: config.posts,
    }
}

/// A [`MatchConfig`] and team mode that read back the same dials a set of
/// wire [`MatchTerms`] carries, for painting a joiner's terms card: it is
/// shown the match it is joining, not its own setup screen's idea of one.
/// The seat count is the humans plus AI the terms name; `custom`/`armed`
/// are display-only here. A `Custom` map cannot be named from the wire (the
/// beach travels as bytes, not an index), so it reads as the generated size
/// it will actually play on.
pub fn config_from_terms(terms: &MatchTerms) -> (MatchConfig, crate::app::teams::TeamMode) {
    let bots = terms.bots.min(MAX_PLAYERS as u8);
    let config = MatchConfig {
        seats: bots.max(2),
        bots,
        controllers: [SeatController::Ai(BotLevel::from_index(usize::from(terms.bot_level)));
            MAX_PLAYERS],
        map: MapChoice::from_index(usize::from(terms.map)),
        custom: 0,
        gulls: GullPressure::from_index(usize::from(terms.gulls)),
        round: RoundLength::from_index(usize::from(terms.round)),
        posts: posts_from(terms),
        series: crate::app::tournament::SeriesLength::from_index(usize::from(terms.series)),
        armed: false,
        bots_welcome: true,
    };
    (
        config,
        crate::app::teams::TeamMode::from_index(usize::from(terms.teams)),
    )
}

/// The same terms on a new beach: a fresh seed, and the map stepped on as
/// a local series steps it, through [`next_map`] and its guards. Everything
/// the table agreed (seat count, gull pressure, round length, scoring) is
/// kept, because a series is one match played several times, not several
/// matches.
///
/// `seats` is the table as it sits, which the terms themselves do not
/// carry: it is what keeps a table of five off the four-castle beaches.
/// The shelf is not consulted, because online a handmade beach rides in
/// the invitation itself (`net/rounds.rs` resends it), so with no shelf
/// the stepper walks past the `Custom` stop.
pub fn next_round_terms(terms: MatchTerms, seats: u8, seed: u64) -> MatchTerms {
    let mut config = MatchConfig {
        map: MapChoice::from_index(usize::from(terms.map)),
        seats,
        ..MatchConfig::default()
    };
    next_map(&mut config, &CustomBeaches::default());
    MatchTerms {
        map: config.map.index() as u8,
        seed,
        ..terms
    }
}

/// Whether `map` has room for a table of `seats`. Five and six castles
/// need a wide beach: the two extra sit mid-edge, and the handcrafted
/// classic arena and the small generated one hold four, clamping a bigger
/// table's castles down to it. A handmade beach answers by its castle
/// count instead (see [`CustomBeaches::fitting`]).
pub fn holds(map: MapChoice, seats: u8) -> bool {
    seats <= CLASSIC_SEATS || map.size().0 >= WIDE_ENOUGH
}

/// Step the map on for the next round of a series: the dial's own step
/// ([`cycle_map`], which walks the shelf and skips it when empty), and
/// then on again past any beach the table does not fit on.
///
/// The one place a series and the dial differ: the dial, turned onto a
/// small beach by hand, drops the seats it cannot hold, because the hand
/// on it asked for that beach. A series asked for nobody to leave the
/// table, so it keeps the seats and skips the beach. Stepped with a plain
/// `cycled(Turn::Right)`, a table of five landed on the classic arena with
/// two of them castle-less.
pub fn next_map(config: &mut MatchConfig, beaches: &CustomBeaches) {
    // Bounded by the number of stops there are: every wide beach holds
    // every table, so this returns well before the bound, but a loop over
    // a dial should not be able to spin.
    for _ in 0..MapChoice::ALL.len() + beaches.0.len() {
        cycle_map(config, Turn::Right, beaches);
        if holds(config.map, config.seats) {
            return;
        }
    }
}

/// Keep the map somewhere the table can play after something other than
/// the dial moved: the seat count, or the shelf between two visits to the
/// screen. Off `Custom` when no beach seats everyone (onto the stop after
/// it, as the dial itself steps), onto the widest beach when five or six
/// are seated and the map holds four. Without it the row reads a beach the
/// match will not be played on: `Custom` with nothing fitting launched a
/// generated 20x13 arena under the classic arena's name.
pub fn settle_map(config: &mut MatchConfig, beaches: &CustomBeaches) {
    if config.map == MapChoice::Custom {
        match beaches.fitting(config.seats).len() {
            0 => config.map = MapChoice::Custom.cycled(Turn::Right),
            fitting => config.custom = config.custom.min(fitting - 1),
        }
    }
    if !holds(config.map, config.seats) {
        config.map = MapChoice::GenXl;
    }
}

#[cfg(test)]
use screen::{
    LABEL_W, ROW_FONT, VALUE_W, ai_seat, card_height, cycle_ai_level, has_an_opponent, live_rows,
    p2_pressed, row_text,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Hosting from a couch whose top seat was dialled to a bot: the wire
    /// carries one AI level, and it is the one an AI seat was set to, not
    /// the default a bot's seat reads as.
    #[test]
    fn a_bot_on_the_couch_does_not_set_the_lobbys_ai_level() {
        let mut config = MatchConfig {
            seats: 3,
            bots: 2,
            ..MatchConfig::default()
        };
        config.controllers[2] = SeatController::Bot;
        config.controllers[1] = SeatController::Ai(BotLevel::Hard);
        let terms = terms(&config, crate::app::teams::TeamMode::Solo, 1);
        assert_eq!(terms.bot_level, BotLevel::Hard.index() as u8);
    }

    /// A match needs somebody for player 1 to play: an AI, a pad that
    /// pressed Start, or a second person at P2's keys. Two humans on the
    /// dial with nobody else there is not a game.
    #[test]
    fn a_match_needs_an_opponent() {
        let alone = MatchConfig {
            seats: 2,
            bots: 0,
            ..MatchConfig::default()
        };
        assert!(!has_an_opponent(&alone, 0, false), "an empty chair");
        assert!(has_an_opponent(&alone, 1, false), "a pad joined");
        assert!(has_an_opponent(&alone, 0, true), "P2 pressed a key");
        let with_ai = MatchConfig { bots: 1, ..alone };
        assert!(has_an_opponent(&with_ai, 0, false), "the AI plays");
    }

    /// P2 joins by pressing one of P2's own keys, and never by a key the
    /// menu or player 1 uses: under the one-hand preset IJKL are player
    /// 1's commit keys, and pressing them is not a second person arriving.
    #[test]
    fn only_p2s_own_keys_say_p2_is_here() {
        use crate::app::settings::{CommitScheme, GameSettings};
        let pressed = |key: KeyCode, settings: &GameSettings| {
            let mut keys = ButtonInput::<KeyCode>::default();
            keys.press(key);
            p2_pressed(&keys, settings)
        };
        let stock = GameSettings::default();
        assert!(pressed(KeyCode::KeyI, &stock), "P2's up");
        assert!(!pressed(KeyCode::KeyW, &stock), "P1's up");
        assert!(!pressed(KeyCode::Enter, &stock), "the menu's start");
        let one_hand = GameSettings {
            commit: CommitScheme::Ijkl,
            ..GameSettings::default()
        };
        assert!(
            !pressed(KeyCode::KeyI, &one_hand),
            "P1's commit under the preset"
        );
    }

    /// Every row fits the two cells that hold it, in every language, on
    /// every stop of every dial.
    ///
    /// Measured in the same pixels the two cells are declared in, which is
    /// why they are declared that way: see [`LABEL_W`].
    #[test]
    fn every_row_fits_its_cell_in_every_language() {
        use crate::app::i18n::metrics::text_px;
        use crate::app::settings::{GameSettings, NAME_MAX};
        for lang in crate::app::i18n::ALL_LANGS {
            let mut settings = GameSettings {
                language: lang,
                ..GameSettings::default()
            };
            // A seat named to the hilt: the naming row shows the name, a
            // caret and the hint, and all three have to share the cell.
            settings.names[0] = "M".repeat(NAME_MAX);
            let tr = settings.tr();
            for seats in 2..=MAX_PLAYERS as u8 {
                for bots in 0..seats {
                    for (map, series) in MapChoice::ALL.into_iter().flat_map(|map| {
                        crate::app::tournament::SeriesLength::ALL.map(move |s| (map, s))
                    }) {
                        // A handmade beach's name is the player's, up to
                        // twenty-eight characters of it, and no cell on
                        // any screen is built to hold that. It clips, the
                        // way a typed name clips on the settings card.
                        if map == MapChoice::Custom {
                            continue;
                        }
                        let config = MatchConfig {
                            seats,
                            bots,
                            map,
                            gulls: GullPressure::Frenzy,
                            round: RoundLength::Long,
                            // Every position of the mode dial, not the
                            // longest guessed at: "best of 3" and "best of
                            // 5" are the same width in English and are not
                            // in every language this ships in.
                            series,
                            ..MatchConfig::default()
                        };
                        for row in Row::ALL {
                            // Both states of the one row that is typed
                            // into rather than stepped through.
                            for naming in [None, Some(0)] {
                                let (label, value) = row_text(
                                    tr,
                                    &config,
                                    &settings,
                                    &CustomBeaches::default(),
                                    naming,
                                    row,
                                );
                                let label_w = text_px(&label, ROW_FONT);
                                assert!(
                                    label_w <= LABEL_W,
                                    "{row:?} in {lang:?}: label {label:?} is \
                                     {label_w:.1}px, and the cell holds {LABEL_W}"
                                );
                                let value_w = text_px(&value, ROW_FONT);
                                assert!(
                                    value_w <= VALUE_W,
                                    "{row:?} in {lang:?}: value {value:?} is \
                                     {value_w:.1}px, and the cell holds {VALUE_W}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// The card has to fit the window the interface was drawn for, as the
    /// settings card does. One column rather than two, so there is room,
    /// but not enough to widen a cell without looking.
    #[test]
    fn the_card_fits_the_window_it_was_drawn_for() {
        let card = LABEL_W + VALUE_W + 2.0 * 10.0 + 2.0 * 22.0;
        // The crab and the gull stand in the same row as the card and do
        // not shrink, so the width they take is the card's to spare. Left
        // out, the card would be the one squeezed on a narrow window.
        let across = card;
        assert!(
            across <= crate::app::settings::DESIGN_W,
            "the match card and its company are {across}px of the {}px they \
             are allowed ({card}px of it the card)",
            crate::app::settings::DESIGN_W
        );
    }

    /// The most rows a table can show at once: six seats, five of them AI,
    /// is every name and every AI level besides the dials.
    fn fullest_table() -> usize {
        (2..=MAX_PLAYERS as u8)
            .flat_map(|seats| {
                (0..seats).map(move |bots| MatchConfig {
                    seats,
                    bots,
                    ..MatchConfig::default()
                })
            })
            .map(|config| live_rows(&config).iter().filter(|live| **live).count())
            .max()
            .unwrap_or(0)
    }

    /// A full table fits the card with every row at the height it lays out
    /// at. Sized for rows at their least, the card ran a table of six with
    /// five AI past its frame: the last name sat on the wood.
    #[test]
    fn the_fullest_table_fits_its_card() {
        let rows = fullest_table();
        assert_eq!(rows, ROWS, "a full table shows every row");
        let needed = 2.0 * crate::app::menu_ui::CARD_PAD
            + crate::app::menu_ui::HEADING_H
            + rows as f32 * crate::app::menu_ui::row_pitch(ROW_FONT);
        assert!(
            card_height() >= needed,
            "the card is {}px for {needed}px of rows",
            card_height()
        );
    }

    /// And on the shortest window the interface is drawn for, where the
    /// card has to shrink to the room between the bars, the rows can close
    /// up enough to fit it, with the lines under the card still below it.
    #[test]
    fn the_fullest_table_closes_up_to_fit_the_design_window() {
        use crate::app::menu_ui::{BAR_H, CARD_PAD, HEADING_H, ROW_H, type_scale};
        let squeezed = 2.0 * CARD_PAD + HEADING_H + fullest_table() as f32 * ROW_H;
        // The beach note, the two controller lines (a padded line each),
        // and the column's gap between the four.
        let footer = 20.0
            + 2.0 * (type_scale::BODY * crate::app::menu_ui::LINE_HEIGHT + 6.0)
            + 3.0 * super::screen::FOOTER_GAP;
        let room = crate::app::settings::DESIGN_H - 2.0 * BAR_H;
        assert!(
            squeezed + footer <= room,
            "{squeezed}px of card and {footer}px under it, in {room}px"
        );
    }

    /// The point of sending the beach at all: a level the joiner has never
    /// seen has to arrive whole, and arrive as the same board the host is
    /// playing on. A seed cannot describe a beach somebody drew.
    #[test]
    fn a_handmade_beach_survives_the_wire() {
        let text = "name: Sent Beach\nposts: 2\ncrab: 0,1 R R common\nmap:\n\
                    +-+-+-+-+-+\n|. . . . .|\n+ + + + + +\n|. . . . 0|\n\
                    + + + + + +\n|. . . . .|\n+-+-+-+-+-+\n";
        let level = crate::sim::Level::parse(text).expect("a level");
        let packed = crate::lzw::compress(level.to_text().as_bytes(), 8);
        // Small enough to ride in a datagram, which is the whole reason it
        // is compressed rather than sent as the text it came from.
        assert!(packed.len() < 512, "{} bytes", packed.len());
        let back = beach_from(&packed).expect("reads back");
        assert_eq!(back.name, "Sent Beach");
        assert_eq!(back.to_text(), level.to_text(), "byte for byte");
    }

    /// The two beaches at the ends of what the editor can build: the
    /// biggest sensible one has to travel, and the biggest possible one has
    /// to be refused rather than truncated on arrival.
    ///
    /// The wire test can only check the number [`MAX_BEACH_BYTES`]
    /// promises; this checks the promise covers beaches a player can
    /// actually paint.
    #[test]
    fn the_biggest_beaches_the_editor_builds_are_sent_or_refused() {
        use crate::sim::{Board, CrabKind, Direction, Handedness, LevelKind, Spawner, TileKind};
        let full_size = || Board::new(20, 13, 0xBEEF);
        let seat_it = |board: &mut Board| {
            for owner in 0..MAX_PLAYERS as u8 {
                board.set_tile(owner, 0, TileKind::Castle(owner));
            }
        };
        let as_beach = |board: Board| {
            Beach::new(
                crate::sim::Level::from_board("A Beach With A Long Name", 3, board)
                    .with_kind(LevelKind::Arena),
            )
        };

        // A busy beach: every seat, rocks, holes and walls all over it.
        let mut busy = full_size();
        seat_it(&mut busy);
        for x in 0..20u8 {
            for y in 1..13u8 {
                if (x + y).is_multiple_of(3) {
                    busy.set_tile(x, y, TileKind::Rock);
                }
                if (x + y).is_multiple_of(7) {
                    busy.set_tile(
                        x,
                        y,
                        TileKind::Spawner(Spawner {
                            dir: Direction::Right,
                            period: 60,
                        }),
                    );
                }
                if (x * y).is_multiple_of(5) {
                    busy.set_wall(x, y, Direction::Up, true);
                }
            }
        }
        let busy = as_beach(busy);
        assert!(
            !busy.too_big_to_send(),
            "a beach anyone would build must travel: {} bytes",
            busy.wire.len()
        );

        // And the worst case: a crab on every free tile, which the editor
        // will happily let an author paint. Each one is a header line of
        // its own, so the text runs to thousands of characters.
        let mut soup = full_size();
        seat_it(&mut soup);
        for x in 0..20u8 {
            for y in 0..13u8 {
                if soup.tile_at(x, y) == TileKind::Empty {
                    soup.spawn_crab(
                        x,
                        y,
                        Direction::Right,
                        Handedness::Left,
                        CrabKind::Sparkling,
                    );
                }
            }
        }
        let soup = as_beach(soup);
        assert!(
            soup.too_big_to_send(),
            "{} bytes was expected to be over the line",
            soup.wire.len()
        );
        let config = MatchConfig {
            map: MapChoice::Custom,
            seats: 2,
            custom: 0,
            ..MatchConfig::default()
        };
        assert!(
            beach_bytes(&config, 2, &CustomBeaches(vec![soup])).is_empty(),
            "an oversized beach is dropped, not sent in pieces"
        );
    }

    /// A beach the host picked for two cannot hold the five who turned up:
    /// three seats would have no castle and could never score. It is
    /// dropped at launch and the round falls back to the terms.
    #[test]
    fn a_beach_too_small_for_the_table_is_not_sent() {
        let two = crate::sim::Level::parse(
            "name: Two\nposts: 1\ncrab: 0,1 R R common\nmap:\n\
             +-+-+-+\n|0 . .|\n+ + + +\n|. . 1|\n+ + + +\n|. . .|\n+-+-+-+\n",
        )
        .expect("a level");
        let shelf = CustomBeaches(vec![Beach::new(two)]);
        let config = MatchConfig {
            map: MapChoice::Custom,
            seats: 2,
            custom: 0,
            ..MatchConfig::default()
        };
        assert!(!beach_bytes(&config, 2, &shelf).is_empty(), "fits a pair");
        assert!(
            beach_bytes(&config, 5, &shelf).is_empty(),
            "five turned up and it has two castles"
        );
    }

    /// And the same beach refused again at the far end, where the round is
    /// actually built. The launch drops one that cannot seat the table, but
    /// the round *after* it does not: `call_next_round` re-sends the beach
    /// and works out `seats` afresh, and the queue admitted between rounds
    /// makes that number bigger, seating players at a beach with no castle
    /// for them.
    ///
    /// Both ends run this check on the same bytes, so they fall back
    /// together and no peer desyncs over it.
    #[test]
    fn a_beach_too_small_for_the_table_is_not_played_on_either() {
        let two = crate::sim::Level::parse(
            "name: Two\nposts: 1\ncrab: 0,1 R R common\nmap:\n\
             +-+-+-+\n|0 . .|\n+ + + +\n|. . 1|\n+ + + +\n|. . .|\n+-+-+-+\n",
        )
        .expect("a level");
        assert_eq!(two.seats(), 2, "two castles and no more");
        let packed = crate::lzw::compress(two.to_text().as_bytes(), 8);
        let terms = MatchTerms {
            map: MapChoice::Custom.index() as u8,
            ..MatchTerms::default()
        };
        // The table it was picked for: the handmade beach, as drawn.
        let played = board_from(&terms, 2, &packed);
        assert_eq!(
            (played.width(), played.height()),
            (3, 3),
            "the beach somebody built"
        );
        // Four admitted between rounds, and the same bytes come round
        // again. The beach gives way rather than the two new players.
        let grown = board_from(&terms, 4, &packed);
        let generated = board_for(&terms, 4);
        assert_eq!(
            (grown.width(), grown.height()),
            (generated.width(), generated.height()),
            "back to a generated arena, which every peer builds alike"
        );
        assert_eq!(
            grown.castle_seats(),
            4,
            "and it has a castle for everyone who turned up"
        );
    }

    /// Which chairs the AI holds under a set of terms.
    ///
    /// Host and joiner each work this out from the same wire terms rather
    /// than being told, so it has to answer the same for both. It also
    /// indexes a per-seat array straight off a byte a peer chose, so a
    /// `bots` count larger than the table must reach no further.
    #[test]
    fn the_ai_holds_the_top_seats_and_reaches_no_further() {
        let terms = |bots| MatchTerms {
            bots,
            ..MatchTerms::default()
        };
        let held = |bots, seats| {
            bot_seats_from(&terms(bots), seats)
                .iter()
                .map(Option::is_some)
                .collect::<Vec<bool>>()
        };
        assert_eq!(held(0, 6), [false; 6], "no AI, no chairs");
        assert_eq!(
            held(2, 6),
            [false, false, false, false, true, true],
            "the AI comes in at the top and the people keep the low seats"
        );
        assert_eq!(held(6, 6), [true; 6], "a table played out by itself");
        // A table shorter than the array leaves the chairs past its end
        // alone: those are not seats anybody is playing.
        assert_eq!(held(2, 4), [false, false, true, true, false, false]);
        // And a byte bigger than the table fills the table, not past it.
        assert_eq!(held(u8::MAX, 4), [true, true, true, true, false, false]);
        for seats in 0..=u8::MAX {
            for bots in [0u8, 1, 6, 7, u8::MAX] {
                let _ = bot_seats_from(&terms(bots), seats);
            }
        }
        // Every AI seat plays at the level the terms name, since that is
        // the one thing about them the table agreed on.
        let fierce = MatchTerms {
            bots: 2,
            bot_level: 2,
            ..MatchTerms::default()
        };
        assert_eq!(bot_seats_from(&fierce, 6)[5], Some(BotLevel::Hard));
        assert_eq!(bot_seats_from(&fierce, 6)[0], None);
    }

    /// Nonsense on the wire must not stop the round: the beach falls back
    /// to the terms, and the hash check is what says the peers disagree.
    #[test]
    fn a_beach_that_will_not_read_is_not_fatal() {
        assert!(beach_from(&[]).is_none());
        assert!(beach_from(&[0xFF; 40]).is_none());
        let terms = MatchTerms::default();
        let board = board_from(&terms, 2, &[0xFF; 40]);
        assert_eq!(board.width(), board_for(&terms, 2).width());
    }

    /// A versus arena wants a castle each, so a puzzle built for one crab
    /// and one castle is not offered as a two-seat beach.
    #[test]
    fn a_beach_is_offered_only_when_it_has_the_castles() {
        let one = crate::sim::Level::parse(
            "name: One\nposts: 1\ncrab: 0,1 R R common\nmap:\n\
             +-+-+-+\n|. . .|\n+ + + +\n|. . 0|\n+ + + +\n|. . .|\n+-+-+-+\n",
        )
        .expect("a level");
        assert_eq!(one.seats(), 1);
        let two = crate::sim::Level::parse(
            "name: Two\nposts: 1\ncrab: 0,1 R R common\nmap:\n\
             +-+-+-+\n|0 . .|\n+ + + +\n|. . 1|\n+ + + +\n|. . .|\n+-+-+-+\n",
        )
        .expect("a level");
        assert_eq!(two.seats(), 2);
    }

    /// A beach that fits nobody at this table is not silently absent: the
    /// dial skips its stop, and the row beside it says why. Two castles
    /// stop being offered the moment a third player sits down.
    #[test]
    fn the_map_row_says_why_a_beach_is_not_on_offer() {
        use crate::app::i18n::EN;
        let two = crate::sim::Level::parse(
            "name: Two\nposts: 1\nkind: arena\ncrab: 0,1 R R common\nmap:\n\
             +-+-+-+\n|0 . .|\n+ + + +\n|. . 1|\n+ + + +\n|. . .|\n+-+-+-+\n",
        )
        .expect("a level");
        let shelf = CustomBeaches(vec![Beach::new(two)]);
        let at = |seats| MatchConfig {
            seats,
            ..MatchConfig::default()
        };

        assert_eq!(beaches_note(&at(2), &EN, &shelf), None, "it is on the dial");
        let note = beaches_note(&at(4), &EN, &shelf).expect("four cannot sit at it");
        assert!(note.contains('4'), "{note}");

        // With nothing saved there is nothing to explain: an empty shelf is
        // not a shelf whose beaches are the wrong size.
        assert_eq!(beaches_note(&at(4), &EN, &CustomBeaches::default()), None);
    }

    /// The dial walks the built-ins, then every handmade beach, and steps
    /// off the end rather than sticking. With none saved it never lands on
    /// the custom stop at all, because an empty stop is a dead press.
    #[test]
    fn the_map_dial_skips_a_stop_with_nothing_on_it() {
        let mut config = MatchConfig {
            map: MapChoice::GenOcean,
            ..MatchConfig::default()
        };
        // An empty shelf is the case that matters: the dial must not stop
        // on a beach that is not there.
        let shelf = CustomBeaches::default();
        cycle_map(&mut config, Turn::Right, &shelf);
        assert_ne!(config.map, MapChoice::Custom, "an empty stop is skipped");
    }

    /// A series steps the map under the dial's guards, keeping the table:
    /// five seated skip the empty shelf and the four-castle beaches rather
    /// than losing two castles on the classic arena. Locally and on the
    /// wire alike, since the wire form goes through the same stepper.
    #[test]
    fn a_series_steps_past_beaches_the_table_does_not_fit() {
        let mut config = MatchConfig {
            map: MapChoice::GenOcean,
            seats: 5,
            ..MatchConfig::default()
        };
        let shelf = CustomBeaches::default();
        next_map(&mut config, &shelf);
        assert_eq!(
            config.map,
            MapChoice::GenLarge,
            "past Custom, Classic, Small, 12x9"
        );
        assert_eq!(config.seats, 5, "nobody left the table");
        next_map(&mut config, &shelf);
        assert_eq!(config.map, MapChoice::GenXl);

        // Four seated walk every built-in beach in order.
        let mut four = MatchConfig {
            map: MapChoice::GenOcean,
            seats: 4,
            ..MatchConfig::default()
        };
        next_map(&mut four, &shelf);
        assert_eq!(four.map, MapChoice::Classic);

        let terms = MatchTerms {
            map: MapChoice::GenOcean.index() as u8,
            seed: 1,
            ..MatchTerms::default()
        };
        let next = next_round_terms(terms, 5, 2);
        assert_eq!(
            MapChoice::from_index(usize::from(next.map)),
            MapChoice::GenLarge
        );
        assert_eq!(next.seed, 2);
        let next = next_round_terms(terms, 2, 3);
        assert_eq!(
            MapChoice::from_index(usize::from(next.map)),
            MapChoice::Classic
        );
    }

    /// The seat count moving under the map: `Custom` with no beach seating
    /// the table steps off the shelf, and the label agrees with the launch
    /// in the meantime, naming the XL arena the match would generate.
    #[test]
    fn a_table_the_shelf_cannot_seat_moves_the_map_along() {
        use crate::app::i18n::EN;
        let two = crate::sim::Level::parse(
            "name: Two\nposts: 1\nkind: arena\ncrab: 0,1 R R common\nmap:\n\
             +-+-+-+\n|0 . .|\n+ + + +\n|. . 1|\n+ + + +\n|. . .|\n+-+-+-+\n",
        )
        .expect("a level");
        let shelf = CustomBeaches(vec![Beach::new(two)]);
        let mut config = MatchConfig {
            map: MapChoice::Custom,
            seats: 2,
            ..MatchConfig::default()
        };
        settle_map(&mut config, &shelf);
        assert_eq!(config.map, MapChoice::Custom, "two fit; nothing moves");
        assert!(map_label(&config, &EN, &shelf).contains("Two"));

        config.seats = 3;
        assert_eq!(
            map_label(&config, &EN, &shelf),
            EN.map_names[MapChoice::GenXl.index()],
            "the label names what would launch"
        );
        settle_map(&mut config, &shelf);
        assert_eq!(config.map, MapChoice::Classic, "the stop after Custom");

        config.map = MapChoice::Custom;
        config.seats = 5;
        settle_map(&mut config, &shelf);
        assert_eq!(config.map, MapChoice::GenXl, "and wide enough for five");
    }

    use crate::app::settings::GameSettings;

    /// Open ocean is the one beach with no edges. Every other map choice
    /// stays walled, or a beach changes shape under everyone.
    #[test]
    fn only_the_open_ocean_has_no_edges() {
        for map in MapChoice::ALL {
            let terms = MatchTerms {
                map: map.index() as u8,
                seed: 99,
                ..MatchTerms::default()
            };
            let board = board_for(&terms, 4);
            assert_eq!(
                board.wrap(),
                map == MapChoice::GenOcean,
                "{map:?} wraps: {}",
                board.wrap()
            );
        }
        // And it is appended, so an older settings file or a `Start` from
        // another machine still names the beach it meant.
        for (index, map) in MapChoice::ALL.iter().enumerate() {
            assert_eq!(MapChoice::from_index(index), *map);
        }
        assert_eq!(MapChoice::from_index(4), MapChoice::GenXl, "xl stayed put");
    }

    /// Naming a seat, end to end in a headless App: Enter on a name row
    /// takes the keyboard instead of starting the match, what is typed lands
    /// in the name, and Enter hands the keyboard back.
    #[test]
    fn typing_renames_a_seat_without_starting_the_match() {
        use bevy::input::ButtonState;
        use bevy::input::keyboard::{Key, KeyboardInput};

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.add_message::<KeyboardInput>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<MatchMenu>();
        app.init_resource::<MatchConfig>();
        app.init_resource::<crate::app::bot_seats::BotSeats>();
        app.init_resource::<CustomBeaches>();
        app.init_resource::<crate::app::tournament::Tournament>();
        app.init_resource::<crate::app::gamepad::PadSeats>();
        app.insert_resource(GameSettings::default());
        app.add_systems(Update, match_setup_input);

        let name_row = Row::ALL
            .iter()
            .position(|row| matches!(row, Row::Name(0)))
            .expect("a name row for seat 1");
        app.world_mut().resource_mut::<MatchMenu>().selected = name_row;

        let tap = |app: &mut App, key: KeyCode| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(key);
            app.update();
        };
        let type_char = |app: &mut App, ch: &str| {
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
        };

        // Tab opens the name box. Enter must not: the name rows are last
        // on the list, so a player who has just named everybody presses
        // Enter to start and would get the name box again.
        tap(&mut app, KeyCode::Tab);
        assert_eq!(app.world().resource::<MatchMenu>().naming, Some(0));
        assert!(
            !app.world().resource::<MatchConfig>().armed,
            "Tab named the seat instead of launching"
        );

        type_char(&mut app, "B");
        type_char(&mut app, "o");
        assert_eq!(app.world().resource::<GameSettings>().names[0], "Bo");
        assert_eq!(app.world().resource::<GameSettings>().seat_name(0), "Bo");

        tap(&mut app, KeyCode::Enter);
        assert_eq!(app.world().resource::<MatchMenu>().naming, None);
        assert!(
            !app.world().resource::<MatchConfig>().armed,
            "the Enter that finished the name did not launch either"
        );

        // And now Enter starts the match from that very row, rather than
        // reopening the box it just closed: with somebody to play, since
        // a table of two humans and no AI starts only once P2 is there.
        app.world_mut().resource_mut::<MatchMenu>().p2_here = true;
        assert!(
            matches!(Row::ALL[name_row], Row::Name(0)),
            "still on a name row"
        );
        tap(&mut app, KeyCode::Enter);
        assert!(
            app.world().resource::<MatchConfig>().armed,
            "Enter on a name row has to start the match"
        );
    }

    /// A name row per seat in the match, and none for the seats that are
    /// not playing.
    #[test]
    fn name_rows_follow_the_seat_count() {
        let config = MatchConfig {
            seats: 3,
            ..MatchConfig::default()
        };
        let live = live_rows(&config);
        let named: Vec<u8> = Row::ALL
            .iter()
            .enumerate()
            .filter(|&(row, _)| live[row])
            .filter_map(|(_, kind)| {
                if let Row::Name(seat) = kind {
                    Some(*seat)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(named, vec![0, 1, 2], "one row per seat, seat 4 sits out");
    }

    /// AI seats fill from the top down, one difficulty row each, and the
    /// unused rows stay hidden.
    #[test]
    fn ai_rows_track_the_seats_the_ai_holds() {
        let mut config = MatchConfig {
            seats: 4,
            bots: 2,
            ..MatchConfig::default()
        };
        assert_eq!(ai_seat(&config, 0), Some(3));
        assert_eq!(ai_seat(&config, 1), Some(2));
        assert_eq!(ai_seat(&config, 2), None, "only two AI seats are taken");

        let live = live_rows(&config);
        let hidden: Vec<bool> = Row::ALL
            .iter()
            .zip(live)
            .filter_map(|(row, live)| matches!(row, Row::BotLevel(_)).then_some(live))
            .collect();
        let mut want = vec![false; MAX_BOTS];
        want[0] = true;
        want[1] = true;
        assert_eq!(hidden, want, "one live row per AI seat, the rest folded");
        // An all-human match hides every AI row, and a table of four hides
        // the name rows of the two seats nobody is sitting in.
        config.bots = 0;
        let empty_seats = MAX_PLAYERS - usize::from(config.seats);
        assert_eq!(
            live_rows(&config).iter().filter(|live| **live).count(),
            ROWS - MAX_BOTS - empty_seats
        );
    }

    /// Each AI seat carries its own difficulty: turning one row must not
    /// drag the other seats with it.
    #[test]
    fn difficulties_are_per_seat() {
        let mut config = MatchConfig {
            seats: 4,
            bots: 3,
            ..MatchConfig::default()
        };
        cycle_ai_level(&mut config, 0, Turn::Right); // seat 4: normal -> fierce
        cycle_ai_level(&mut config, 2, Turn::Left); // seat 2: normal -> easy
        assert_eq!(config.controllers, {
            let mut want = [SeatController::Ai(BotLevel::Normal); MAX_PLAYERS];
            want[1] = SeatController::Ai(BotLevel::Easy); // seat 2, stepped down
            want[3] = SeatController::Ai(BotLevel::Hard); // seat 4, stepped up
            want
        });
        // A slot with no AI behind it is inert.
        config.bots = 1;
        cycle_ai_level(&mut config, 2, Turn::Right);
        assert_eq!(config.controllers[1], SeatController::Ai(BotLevel::Easy));
    }
}
