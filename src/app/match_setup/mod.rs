//! Match setup: the three steps between the menu and a local versus
//! round. Step 1 sets the game (players, the AI's level, round length,
//! arrows, gulls, scoring, series), step 2 picks the map from cards that
//! preview the very beach that will be played, and step 3 says who plays
//! each seat and lets friends sit down with a controller's Start.
//!
//! The three steps edit one [`MatchConfig`], so going back and forth
//! between them never loses a choice. This file is the model and its wire
//! form; [`steps`] is the keys, [`ui`] the cards, [`preview`] the map
//! pictures.

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
mod preview;
mod steps;
mod ui;

pub use beaches::{
    Beach, BeachGone, CustomBeaches, beach_bytes, beach_from, beach_in_play, beach_on_air,
    beaches_note, cycle_map, map_about, map_label, map_name, map_title, refresh_custom_beaches,
    say_beach_gone,
};
pub use board::{apply_terms, board_for, board_from, bot_seats_from, local_board, versus_board};
pub use preview::{
    GameCamera, MapPreviews, PREVIEW_PX, PREVIEW_TICKS, Pic, PreviewCamera, PreviewPipelines,
    draw_board_picture, preview_board, release_previews, render_previews, retire_previews,
    watch_preview_pipelines,
};
pub use steps::*;
#[cfg(test)]
pub(crate) use ui::card_note;
pub use ui::{
    MapBar, MatchUi, enter_match_setup, follow_map_bar, host_map_title, leave_match_setup,
    moved_note, paint_game_step, paint_map_step, paint_players_step, paint_start_button,
    paint_teams, start_button_label, update_host_map_ui, update_match_ui,
};

/// The playable beach for a local match.
///
/// The variants are in the order their index travels in (`MatchTerms.map`,
/// a settings file), new ones appended and never slotted in: reordering
/// would silently change which beach a host is inviting everyone to. The
/// order the dial turns through and step 2's cards show them in is
/// [`MapChoice::DIAL`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MapChoice {
    #[default]
    Classic,
    GenSmall,
    GenClassic,
    GenLarge,
    GenXl,
    /// Open ocean: a generated beach with no edges.
    GenOcean,
    /// A beach somebody built in the editor. Which one is not on the wire:
    /// the beach itself rides along with the invitation, because no peer
    /// but the host has the file it came from.
    Custom,
    /// The handmade versus maps ([`crate::sim::VERSUS_MAPS`], in this
    /// order). Every peer has them built in, so the index is all that
    /// travels.
    TheGates,
    TheMoat,
    LogJam,
    ThreeLanes,
    RoundTable,
    RoundTheWorld,
    Duel,
    /// A freshly generated beach every round, its style and size drawn
    /// from the round's seed ([`crate::sim::surprise_style`]), so every
    /// peer and every replay builds the same one.
    Surprise,
    /// More handmade maps, after Surprise because the wire order is
    /// appended to and never reordered: the dial lists them beside the
    /// others ([`MapChoice::DIAL`]).
    TheFountain,
    FourDoors,
    GullRoad,
    BackToBack,
}

impl MapChoice {
    /// Every map, in wire order.
    pub const ALL: [MapChoice; 19] = [
        MapChoice::Classic,
        MapChoice::GenSmall,
        MapChoice::GenClassic,
        MapChoice::GenLarge,
        MapChoice::GenXl,
        MapChoice::GenOcean,
        MapChoice::Custom,
        MapChoice::TheGates,
        MapChoice::TheMoat,
        MapChoice::LogJam,
        MapChoice::ThreeLanes,
        MapChoice::RoundTable,
        MapChoice::RoundTheWorld,
        MapChoice::Duel,
        MapChoice::Surprise,
        MapChoice::TheFountain,
        MapChoice::FourDoors,
        MapChoice::GullRoad,
        MapChoice::BackToBack,
    ];

    /// The order the map dial turns through, which is step 2's sections
    /// in turn ([`MapSection`]): the built-in maps (the handmade ones
    /// beside the classic arena, then the generated sizes and the open
    /// ocean), the player's own beaches, and Surprise.
    pub const DIAL: [MapChoice; 19] = [
        MapChoice::Classic,
        MapChoice::TheGates,
        MapChoice::TheMoat,
        MapChoice::LogJam,
        MapChoice::ThreeLanes,
        MapChoice::RoundTable,
        MapChoice::RoundTheWorld,
        MapChoice::Duel,
        MapChoice::TheFountain,
        MapChoice::FourDoors,
        MapChoice::GullRoad,
        MapChoice::BackToBack,
        MapChoice::GenSmall,
        MapChoice::GenClassic,
        MapChoice::GenLarge,
        MapChoice::GenXl,
        MapChoice::GenOcean,
        MapChoice::Custom,
        MapChoice::Surprise,
    ];

    /// The map's name on a command line and in a dev hook: the bots'
    /// `--map`, `PINCH_SKIRMISH`. `custom` names no beach in particular.
    pub fn token(self) -> &'static str {
        match self {
            MapChoice::Classic => "classic",
            MapChoice::GenSmall => "small",
            MapChoice::GenClassic => "generated",
            MapChoice::GenLarge => "large",
            MapChoice::GenXl => "xl",
            MapChoice::GenOcean => "ocean",
            MapChoice::Custom => "custom",
            MapChoice::TheGates => "gates",
            MapChoice::TheMoat => "moat",
            MapChoice::LogJam => "log-jam",
            MapChoice::ThreeLanes => "three-lanes",
            MapChoice::RoundTable => "round-table",
            MapChoice::RoundTheWorld => "round-the-world",
            MapChoice::Duel => "duel",
            MapChoice::Surprise => "surprise",
            MapChoice::TheFountain => "fountain",
            MapChoice::FourDoors => "four-doors",
            MapChoice::GullRoad => "gull-road",
            MapChoice::BackToBack => "back-to-back",
        }
    }

    /// The map a [`MapChoice::token`] names. `kelp-keep`, The Gates'
    /// name before it was renamed, still names it, so a script or a
    /// command line written for the old name plays the same beach.
    pub fn from_token(token: &str) -> Option<MapChoice> {
        let token = match token {
            "kelp-keep" => MapChoice::TheGates.token(),
            token => token,
        };
        MapChoice::ALL.into_iter().find(|map| map.token() == token)
    }

    /// Which of [`crate::sim::VERSUS_MAPS`] the map is, for a handmade one.
    pub fn handmade(self) -> Option<usize> {
        match self {
            MapChoice::TheGates => Some(0),
            MapChoice::TheMoat => Some(1),
            MapChoice::LogJam => Some(2),
            MapChoice::ThreeLanes => Some(3),
            MapChoice::RoundTable => Some(4),
            MapChoice::RoundTheWorld => Some(5),
            MapChoice::Duel => Some(6),
            MapChoice::TheFountain => Some(7),
            MapChoice::FourDoors => Some(8),
            MapChoice::GullRoad => Some(9),
            MapChoice::BackToBack => Some(10),
            MapChoice::Classic
            | MapChoice::GenSmall
            | MapChoice::GenClassic
            | MapChoice::GenLarge
            | MapChoice::GenXl
            | MapChoice::GenOcean
            | MapChoice::Custom
            | MapChoice::Surprise => None,
        }
    }

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
            // The level files' own sizes (a test holds them to it).
            MapChoice::TheGates | MapChoice::TheMoat | MapChoice::LogJam => (13, 9),
            MapChoice::ThreeLanes => (15, 9),
            MapChoice::RoundTable => (15, 11),
            MapChoice::RoundTheWorld => (21, 12),
            MapChoice::Duel => (11, 7),
            MapChoice::TheFountain => (13, 11),
            MapChoice::FourDoors => (21, 11),
            MapChoice::GullRoad => (25, 13),
            MapChoice::BackToBack => (21, 12),
            // Up to four seats; five and six play at 16x11
            // ([`crate::sim::surprise_size`]), and Surprise seats every
            // table, so this is never what decides a fit.
            MapChoice::Surprise => crate::sim::surprise_size(CLASSIC_SEATS),
        }
    }

    /// Whether the beach has edges. Open ocean, Round the World, Four
    /// Doors, Gull Road and Back to Back do not: a creature walking off one
    /// side comes back on the other, the same wrap the campaign teaches at
    /// level 26.
    pub fn wraps(self) -> bool {
        match self {
            MapChoice::GenOcean
            | MapChoice::RoundTheWorld
            | MapChoice::FourDoors
            | MapChoice::GullRoad
            | MapChoice::BackToBack => true,
            MapChoice::Classic
            | MapChoice::GenSmall
            | MapChoice::GenClassic
            | MapChoice::GenLarge
            | MapChoice::GenXl
            | MapChoice::Custom
            | MapChoice::TheGates
            | MapChoice::TheMoat
            | MapChoice::LogJam
            | MapChoice::ThreeLanes
            | MapChoice::RoundTable
            | MapChoice::Duel
            | MapChoice::TheFountain
            | MapChoice::Surprise => false,
        }
    }

    /// The tables a handmade map is built for (its level's `seats:` line,
    /// which a test holds this to), `None` for a map that seats any table
    /// it has room for ([`holds`]). An odd table is on the list only for a
    /// beach with no edges made of identical strips, one castle each, so
    /// no castle is ever left empty: on a fixed layout with a castle put
    /// away, the castle across from the gap won 45-58% at three seats.
    pub fn tables(self) -> Option<&'static [u8]> {
        match self {
            MapChoice::TheGates
            | MapChoice::TheMoat
            | MapChoice::LogJam
            | MapChoice::ThreeLanes
            | MapChoice::TheFountain => Some(&[2, 4]),
            MapChoice::RoundTable | MapChoice::RoundTheWorld | MapChoice::BackToBack => {
                Some(&[2, 4, 6])
            }
            MapChoice::Duel => Some(&[2]),
            MapChoice::FourDoors => Some(&[3]),
            MapChoice::GullRoad => Some(&[5]),
            MapChoice::Classic
            | MapChoice::GenSmall
            | MapChoice::GenClassic
            | MapChoice::GenLarge
            | MapChoice::GenXl
            | MapChoice::GenOcean
            | MapChoice::Custom
            | MapChoice::Surprise => None,
        }
    }

    /// The most players the beach seats: the biggest table a handmade map
    /// is built for, four on the four-castle beaches, six on the ones wide
    /// enough for the two extra castles ([`holds`]).
    pub fn most_seats(self) -> u8 {
        if let Some(tables) = self.tables() {
            return tables.iter().copied().max().unwrap_or(CLASSIC_SEATS);
        }
        match holds(self, MAX_PLAYERS as u8) {
            true => MAX_PLAYERS as u8,
            false => CLASSIC_SEATS,
        }
    }

    /// The next map along the dial ([`MapChoice::DIAL`]) in `turn`'s
    /// direction, wrapping round.
    pub fn turned(self, turn: Turn) -> MapChoice {
        let n = MapChoice::DIAL.len();
        let at = MapChoice::DIAL
            .iter()
            .position(|&map| map == self)
            .unwrap_or(0);
        let step = match turn {
            Turn::Right => 1,
            Turn::Left => n - 1,
        };
        MapChoice::DIAL[(at + step) % n]
    }

    /// Whether a series or a table that outgrew its map may be dealt this
    /// one without asking. The 20x13 is chosen, never dealt (see
    /// [`sized_for`]), Surprise is a way of playing rather than a beach to
    /// step onto, and the player's own beaches are walked by the dial.
    fn dealt(self) -> bool {
        match self {
            MapChoice::GenXl | MapChoice::Surprise | MapChoice::Custom => false,
            MapChoice::Classic
            | MapChoice::GenSmall
            | MapChoice::GenClassic
            | MapChoice::GenLarge
            | MapChoice::GenOcean
            | MapChoice::TheGates
            | MapChoice::TheMoat
            | MapChoice::LogJam
            | MapChoice::ThreeLanes
            | MapChoice::RoundTable
            | MapChoice::RoundTheWorld
            | MapChoice::Duel
            | MapChoice::TheFountain
            | MapChoice::FourDoors
            | MapChoice::GullRoad
            | MapChoice::BackToBack => true,
        }
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

    /// The gull period on the classic 12x9 beach. A bigger beach scales it
    /// with its area through [`crate::sim::set_versus_rules`], which is
    /// how every board is handed it.
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
    Quick,
    Short,
    #[default]
    Standard,
    Long,
}

impl RoundLength {
    pub const ALL: [RoundLength; 4] = [
        RoundLength::Quick,
        RoundLength::Short,
        RoundLength::Standard,
        RoundLength::Long,
    ];

    pub fn ticks(self) -> u32 {
        match self {
            RoundLength::Quick => 60 * crate::sim::TICKS_PER_SECOND,
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

/// The range of the arrows dial. The default is the versus rule's three;
/// one is a knife fight, and past six a beach is more arrow than sand.
pub const POSTS_RANGE: std::ops::RangeInclusive<u8> = 1..=6;

/// The range of the players dial: a match needs two, and the beaches seat
/// six at most.
pub const SEATS_RANGE: std::ops::RangeInclusive<u8> = 2..=MAX_PLAYERS as u8;

/// The dials a set of wire terms names, read back as the app's own types.
///
/// [`MatchTerms`] carries bare indices because it lives in `transport`,
/// which knows nothing of the app; every read of one goes through here, so
/// an index means the same dial wherever it is read.
pub trait TermsDials {
    fn map(&self) -> MapChoice;
    fn gulls(&self) -> GullPressure;
    fn round(&self) -> RoundLength;
    fn bot_level(&self) -> BotLevel;
    fn series(&self) -> crate::app::tournament::SeriesLength;
    fn teams(&self) -> crate::app::teams::TeamMode;
    /// The arrows the terms allow, inside the dial's range: the byte is
    /// whatever a host put there, and a cap of zero is a round nobody can
    /// play. Every peer clamps the same byte the same way, so they still
    /// agree.
    fn posts(&self) -> u8;
}

impl TermsDials for MatchTerms {
    fn map(&self) -> MapChoice {
        MapChoice::from_index(usize::from(self.map))
    }

    fn gulls(&self) -> GullPressure {
        GullPressure::from_index(usize::from(self.gulls))
    }

    fn round(&self) -> RoundLength {
        RoundLength::from_index(usize::from(self.round))
    }

    fn bot_level(&self) -> BotLevel {
        BotLevel::from_index(usize::from(self.bot_level))
    }

    fn series(&self) -> crate::app::tournament::SeriesLength {
        crate::app::tournament::SeriesLength::from_index(usize::from(self.series))
    }

    fn teams(&self) -> crate::app::teams::TeamMode {
        crate::app::teams::TeamMode::from_index(usize::from(self.teams))
    }

    fn posts(&self) -> u8 {
        self.posts.clamp(*POSTS_RANGE.start(), *POSTS_RANGE.end())
    }
}

/// The most seats the AI can hold: everyone but one human.
pub const MAX_BOTS: usize = MAX_PLAYERS - 1;

/// Seats a four-castle beach holds. Past this the match needs a generated
/// arena wide enough for the two long-edge castles.
pub const CLASSIC_SEATS: u8 = 4;
/// The narrowest board that seats five or six: the two extra castles sit
/// mid-edge, and they need room between the corners and the spawner holes.
pub const WIDE_ENOUGH: u8 = 16;

/// The configured local match. Persists between rounds so "again!" needs no
/// re-setup; `armed` is set when the player launches from the setup steps
/// and consumed by `load_versus`.
#[derive(Resource, Clone)]
pub struct MatchConfig {
    pub seats: u8,
    /// Who plays each seat: a person at this machine ([`SeatController::Local`]),
    /// the AI at its own difficulty, or a bot. Seat by seat, as step 3 sets
    /// it, so one player can spar with a fierce rival and an easy one at the
    /// same table. Seat 0 is always a person: player 1 is whoever is working
    /// the menu ([`MatchConfig::clamp_bots`]). Only the first `seats` are
    /// read; the rest keep their choice for when the count comes back up.
    pub controllers: [SeatController; MAX_PLAYERS],
    /// The AI's level: what every AI seat is set to when step 1's row
    /// turns, what an open seat is filled with when the match starts, and
    /// the one level an online table's AI plays at.
    pub ai_level: BotLevel,
    /// The open seats the AI was put in when the match started
    /// ([`seat_the_match`]). The AI plays them for that match, its series
    /// and its rematches, and match setup opens with them open again
    /// ([`settle`]): the table is left as the player set it, so P2's keys
    /// or a friend's Start can still take the seat next time.
    pub filled_at_start: [bool; MAX_PLAYERS],
    /// The map in force: the one the next round is played on. Set from
    /// step 2's card and kept on one the table fits ([`settle_map`]); a
    /// series, a rematch and the Random card move it round to round.
    pub map: MapChoice,
    /// What the player picked on step 2, against the map in force.
    pub pick: Pick,
    /// Which handmade beach, when `map` is [`MapChoice::Custom`]. An index
    /// into [`CustomBeaches::fitting`] for the table, which is read fresh
    /// each time the setup steps open: the editor may have saved another
    /// one since.
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
    /// The lobby's AI dial: how many AI a beach hosted on these dials adds
    /// behind the people who turn up. The couch's own AI seats are the
    /// seats `controllers` gives the AI; the two tables are not the same
    /// size, so they are not the same count either.
    pub lobby_bots: u8,
    /// The seed the next round launched from the setup steps is played
    /// on: the one its map card was drawn with, so the beach previewed is
    /// the beach played. `None` draws a fresh one, as every round after
    /// it does (a series' next round and a rematch clear it).
    pub seed: Option<u64>,
    /// Who sits in each seat as partners were picked on step 3, each
    /// person by the settings row their name is saved on
    /// ([`MatchConfig::seat_names`]). A pick moves people for the match,
    /// never the names saved in settings, which stay on their own rows.
    pub people: crate::app::partners::Who,
}

/// What the player picked on step 2 ([`MatchConfig::pick`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    /// The map in force is the card picked.
    InForce,
    /// The card picked (a map or a shelf beach), kept while the seat count
    /// has moved the map in force off it. The map follows the pick, not
    /// the last map it was moved to: five players move a pick of the small
    /// beach to the large one, and three put it back ([`settle_map`]).
    /// Step 2 and step 3 say so while it is moved ([`map_moved`]).
    Moved(Card),
    /// Step 2's Random card: every round launched from this config draws
    /// `map` (and `custom`) afresh from the maps that seat the table
    /// ([`draw_map`]). A local choice only: the round is built, recorded
    /// and sent as the concrete map the draw landed on.
    ///
    /// Until the draw the map in force is a placeholder, and the seat count
    /// moves it as it moves any pick: `kept` is the card it was moved off,
    /// as [`Pick::Moved`] keeps it, so the count going back puts it back.
    Random { kept: Option<Card> },
}

impl Pick {
    /// Whether step 2's Random card is picked.
    pub fn is_random(self) -> bool {
        matches!(self, Pick::Random { .. })
    }

    /// The card kept while the seat count has moved the map off it.
    pub fn kept(self) -> Option<Card> {
        match self {
            Pick::Moved(pick) => Some(pick),
            Pick::Random { kept } => kept,
            Pick::InForce => None,
        }
    }
}

impl Default for MatchConfig {
    fn default() -> Self {
        MatchConfig {
            seats: 2,
            controllers: [SeatController::Local; MAX_PLAYERS],
            ai_level: BotLevel::Normal,
            filled_at_start: [false; MAX_PLAYERS],
            map: MapChoice::Classic,
            pick: Pick::InForce,
            custom: 0,
            gulls: GullPressure::Normal,
            round: RoundLength::Standard,
            posts: MAX_SIGNPOSTS_PER_PLAYER as u8,
            series: crate::app::tournament::SeriesLength::Single,
            armed: false,
            bots_welcome: true,
            lobby_bots: 0,
            seed: None,
            people: crate::app::partners::Who::default(),
        }
    }
}

impl MatchConfig {
    /// A table of `seats` whose top `ai` seats the AI plays at
    /// [`MatchConfig::ai_level`], the rest people: the shape every table
    /// had before seats were set one by one, and what the dev hooks and
    /// the daily ask for. At least one seat stays a person.
    pub fn with_ai(seats: u8, ai: u8) -> MatchConfig {
        let mut config = MatchConfig {
            seats: seats.clamp(*SEATS_RANGE.start(), *SEATS_RANGE.end()),
            ..MatchConfig::default()
        };
        let ai = ai.min(config.seats - 1);
        for seat in config.seats - ai..config.seats {
            config.controllers[usize::from(seat)] = SeatController::Ai(config.ai_level);
        }
        config
    }

    /// The settings row the name of whoever sits in `seat` is saved on
    /// ([`MatchConfig::people`]).
    pub fn name_row(&self, seat: u8) -> u8 {
        self.people
            .0
            .get(usize::from(seat))
            .copied()
            .unwrap_or(seat)
    }

    /// The names the seats go by at this table, from the names `saved` in
    /// settings: each seat named after the person partner picks put there.
    pub fn seat_names(&self, saved: &[String; MAX_PLAYERS]) -> [String; MAX_PLAYERS] {
        std::array::from_fn(|seat| saved[usize::from(self.name_row(seat as u8))].clone())
    }

    /// The seats played at this machine's keys and pads, in seat order.
    pub fn human_seats(&self) -> Vec<u8> {
        (0..self.seats)
            .filter(|&seat| self.controller(seat) == SeatController::Local)
            .collect()
    }

    /// How many seats are played at this machine.
    pub fn humans(&self) -> u8 {
        self.human_seats().len() as u8
    }

    /// How many seats the AI or a bot plays.
    pub fn ai_seats(&self) -> u8 {
        self.seats - self.humans()
    }

    /// Keep player 1 a person: seat 0 is whoever is working the menu, and
    /// every count of humans relies on there being one. The lobby's AI
    /// dial is held to what an online table can seat beside its host.
    pub fn clamp_bots(&mut self) {
        self.controllers[0] = SeatController::Local;
        self.lobby_bots = self.lobby_bots.min(MAX_BOTS as u8);
    }

    /// Who decides for `seat`.
    pub fn controller(&self, seat: u8) -> SeatController {
        self.controllers[usize::from(seat).min(MAX_PLAYERS - 1)]
    }

    /// The daily challenge's table: today's arena, three fierce bots, a
    /// standard round. Its own value rather than a write into the player's
    /// config, which would come back from the daily showing this on the
    /// setup steps as what they had chosen.
    pub fn daily() -> Self {
        MatchConfig {
            ai_level: BotLevel::Hard,
            map: MapChoice::GenClassic,
            armed: true,
            bots_welcome: false,
            ..MatchConfig::with_ai(4, 3).levelled(BotLevel::Hard)
        }
    }

    /// The same table with every AI seat at `level`.
    fn levelled(mut self, level: BotLevel) -> Self {
        self.ai_level = level;
        for controller in &mut self.controllers {
            if controller.ai().is_some() {
                *controller = SeatController::Ai(level);
            }
        }
        self
    }
}

pub const BOT_LEVELS: [BotLevel; 3] = [BotLevel::Easy, BotLevel::Normal, BotLevel::Hard];

/// The match being set up: its dials, and the shelf of handmade beaches
/// the map cards show.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Dials<'w> {
    pub config: ResMut<'w, MatchConfig>,
    pub beaches: Res<'w, CustomBeaches>,
}

/// The one level every AI seat of an online table plays at: step 1's AI
/// level, since the wire carries one difficulty and a lobby has no seats
/// of its own to set. The lobby's AI row names it, so a level set here
/// is not played unannounced.
pub fn online_ai_level(config: &MatchConfig) -> BotLevel {
    config.ai_level
}

/// The wire form of these dials, for a host to send and every peer to
/// build the same beach from. `teams` and `seed` are not on them: teams
/// is a setting, and the seed is drawn when the match launches. The AI is
/// the lobby's dial, not the couch's seats, and the seats in their own
/// order: who sits where is the host's deal to say.
pub fn terms(config: &MatchConfig, teams: crate::app::teams::TeamMode, seed: u64) -> MatchTerms {
    MatchTerms {
        bots: config.lobby_bots,
        bot_level: online_ai_level(config).index() as u8,
        map: config.map.index() as u8,
        gulls: config.gulls.index() as u8,
        round: config.round.index() as u8,
        teams: teams.index() as u8,
        seed,
        series: config.series.index() as u8,
        posts: config.posts,
        order: crate::transport::IN_SEAT_ORDER,
    }
}

/// A [`MatchConfig`] and team mode that read back the same dials a set of
/// wire [`MatchTerms`] carries, for painting a joiner's terms card: it is
/// shown the match it is joining, not its own setup's idea of one. The
/// seat count is the AI the terms name, at least two; `custom`/`armed`
/// are display-only here. A `Custom` map cannot be named from the wire (the
/// beach travels as bytes, not an index), so it reads as the generated size
/// it will actually play on.
pub fn config_from_terms(terms: &MatchTerms) -> (MatchConfig, crate::app::teams::TeamMode) {
    let bots = terms.bots.min(MAX_PLAYERS as u8);
    let config = MatchConfig {
        seats: bots.max(2),
        lobby_bots: bots,
        ai_level: terms.bot_level(),
        map: terms.map(),
        gulls: terms.gulls(),
        round: terms.round(),
        posts: terms.posts(),
        series: terms.series(),
        ..MatchConfig::default()
    };
    (config, terms.teams())
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
/// the invitation itself (`net/rounds.rs` resends it).
///
/// A series on the host's own beach stays on it. Stepped like the rest,
/// the empty shelf walked the map off `Custom` onto a generated one while
/// `call_next_round` re-sent the same handmade beach and every board was
/// built from it: from round two the terms named a map nobody was
/// playing, which is what a peer in line read off its roster card.
pub fn next_round_terms(terms: MatchTerms, seats: u8, seed: u64) -> MatchTerms {
    if terms.map() == MapChoice::Custom {
        return MatchTerms { seed, ..terms };
    }
    let mut config = MatchConfig {
        map: terms.map(),
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

/// The same terms, on a beach that holds `seats`: unchanged when the map
/// already does, else stepped on as [`next_map`] steps a series. For a
/// table that grew after its map was picked, as admitting the queue
/// between two online rounds grows it.
///
/// The host's handmade beach (`beach`, the bytes the round travels with)
/// holds the table by its castle count ([`beach_in_play`]). One the table
/// has outgrown gives way to the generated beach sized for it, named in
/// the terms: they used to say `Custom` round after round while every
/// board fell back to a generated arena, so the terms card and the roster
/// named a beach nobody was on.
pub fn fit_terms(terms: MatchTerms, seats: u8, beach: &[u8]) -> MatchTerms {
    let map = terms.map();
    if map == MapChoice::Custom {
        return match beach_in_play(map, beach_from(beach), seats) {
            Some(_) => terms,
            None => MatchTerms {
                map: sized_for(seats).index() as u8,
                ..terms
            },
        };
    }
    if holds(map, seats) {
        return terms;
    }
    next_round_terms(terms, seats, terms.seed)
}

/// Whether the terms put the round on the host's handmade beach.
pub fn is_custom(terms: &MatchTerms) -> bool {
    terms.map() == MapChoice::Custom
}

/// Whether `map` has room for a table of `seats`. Five and six castles
/// need a wide beach: the two extra sit mid-edge, and the handcrafted
/// classic arena and the small generated one hold four, clamping a bigger
/// table's castles down to it. A handmade versus map seats the tables it
/// is built for ([`MapChoice::tables`]), and a beach off the shelf answers
/// by its castle count instead (see [`CustomBeaches::fitting`]).
pub fn holds(map: MapChoice, seats: u8) -> bool {
    match (map, map.tables()) {
        // Built at the size the table needs ([`crate::sim::surprise_size`]).
        (MapChoice::Surprise, _) => true,
        (_, Some(tables)) => tables.contains(&seats),
        (_, None) => seats <= CLASSIC_SEATS || map.size().0 >= WIDE_ENOUGH,
    }
}

/// The generated beach sized for a table of `seats`: about thirty tiles a
/// player, from the 9x7 for two to the 16x11 for five or six. The 20x13
/// is never dealt, only chosen: at two players it is 130 tiles a seat,
/// and a table that did not ask for that much sand found it empty.
pub fn sized_for(seats: u8) -> MapChoice {
    match seats {
        0..=2 => MapChoice::GenSmall,
        3..=CLASSIC_SEATS => MapChoice::GenClassic,
        _ => MapChoice::GenLarge,
    }
}

/// Step the map on for the next round of a series: the dial's own step
/// ([`cycle_map`], which walks the shelf and skips it when empty), and
/// then on again past any beach the table does not fit on.
///
/// A series asked for nobody to leave the table, so it keeps the seats and
/// skips the beach. Stepped with a plain `cycled(Turn::Right)`, a table of
/// five landed on the classic arena with two of them castle-less.
pub fn next_map(config: &mut MatchConfig, beaches: &CustomBeaches) {
    // Surprise is a new beach every round already, on the round's seed.
    if config.map == MapChoice::Surprise {
        return;
    }
    // Bounded by the number of stops there are: every wide beach holds
    // every table, so this returns well before the bound, but a loop over
    // a dial should not be able to spin.
    for _ in 0..MapChoice::ALL.len() + beaches.0.len() {
        cycle_map(config, Turn::Right, beaches);
        // The 20x13 is chosen, never dealt (see [`sized_for`]): stepped
        // onto it, a pair playing a best of five got 130 tiles each. Nor
        // is Surprise, which a series stepped onto would never step off.
        let dealt = config.map.dealt() || config.map == MapChoice::Custom;
        if holds(config.map, config.seats) && dealt {
            return;
        }
    }
}

/// Every map a table of `seats` can be dealt, as `(map, custom)`: each
/// built-in beach that holds it but the 20x13, which is only ever chosen
/// ([`sized_for`]), and Surprise, which is a way of playing rather than a
/// beach, then each beach off the shelf that seats it (`custom` its place
/// among the ones that fit). The ones the Random card draws from.
pub fn maps_that_seat(seats: u8, beaches: &CustomBeaches) -> Vec<(MapChoice, usize)> {
    MapChoice::DIAL
        .into_iter()
        .filter(|&map| map.dealt() && holds(map, seats))
        .map(|map| (map, 0))
        .chain((0..beaches.fitting(seats).len()).map(|custom| (MapChoice::Custom, custom)))
        .collect()
}

/// The Random card's draw: one of the maps that seat the table
/// ([`maps_that_seat`]), picked by `roll`, written into `map` and
/// `custom` so the round that follows is built, recorded and sent as that
/// concrete map. `pick` stays [`Pick::Random`], so the next round draws again.
pub fn draw_map(config: &mut MatchConfig, beaches: &CustomBeaches, roll: u64) {
    let maps = maps_that_seat(config.seats, beaches);
    if let Some(&(map, custom)) = maps.get((roll % maps.len().max(1) as u64) as usize) {
        config.map = map;
        config.custom = custom;
    }
}

/// The Random card's draw for an online table: as [`draw_map`], from the
/// maps that seat the table less the beaches off the shelf too big to
/// travel to it, which would leave the round on a generated beach the card
/// never offered.
pub fn draw_online_map(config: &mut MatchConfig, beaches: &CustomBeaches, roll: u64) {
    let fitting = beaches.fitting(config.seats);
    let maps: Vec<(MapChoice, usize)> = maps_that_seat(config.seats, beaches)
        .into_iter()
        .filter(|&(map, custom)| {
            map != MapChoice::Custom
                || fitting
                    .get(custom)
                    .is_some_and(|beach| !beach.too_big_to_send())
        })
        .collect();
    if let Some(&(map, custom)) = maps.get((roll % maps.len().max(1) as u64) as usize) {
        config.map = map;
        config.custom = custom;
    }
}

/// The map of the next round a local table plays, a series' or a
/// rematch's: drawn again under the Random card, stepped on as a series
/// steps it otherwise ([`next_map`]). `roll` is the draw's die, fresh
/// for every round.
pub fn map_for_next_round(config: &mut MatchConfig, beaches: &CustomBeaches, roll: u64) {
    match config.pick {
        Pick::Random { .. } => draw_map(config, beaches, roll),
        Pick::InForce | Pick::Moved(_) => next_map(config, beaches),
    }
}

/// Seat a table of `seats` (held to [`SEATS_RANGE`]), and keep the map one
/// it can be played on. The one way the seat count changes, whether by
/// step 1's Players row or by a pad's Start (or a pad giving its seat
/// back): a pad claim that grew the table without it put five on the
/// four-castle arena, the fifth with no castle all match.
///
/// A seat added back keeps what it was set to before the count went down
/// ([`MatchConfig::controllers`]); one never set is a person's seat with
/// nobody in it yet, which step 3 shows open and a launch fills with the
/// AI ([`open_seats`]).
///
/// The map follows the player's pick, not the last map the count moved
/// it to ([`settle_map`]): read before the count changes, since a
/// handmade beach is named by its place among the ones the old count fit.
pub fn reseat(config: &mut MatchConfig, seats: u8, beaches: &CustomBeaches) {
    let pick = current_pick(config, beaches);
    config.seats = seats.clamp(*SEATS_RANGE.start(), *SEATS_RANGE.end());
    config.clamp_bots();
    place_pick(config, pick, beaches);
}

/// Make the config one the setup steps can seat, on the way in: the
/// seats the AI filled at the last start open again, and the map back on
/// the player's pick where the table fits it. The shelf may have changed
/// since the last visit (a beach deleted, or resaved with fewer castles):
/// a config still pointing at it is moved on, so the map picked is what
/// will be played.
pub fn settle(config: &mut MatchConfig, beaches: &CustomBeaches) {
    open_the_seats_filled_at_start(config);
    config.clamp_bots();
    settle_map(config, beaches);
}

/// Give the seats the AI filled at the last start back to nobody, as
/// step 3 had them: the AI was there for that match only. Read off the
/// mark, at whatever level the AI plays there now: matched against the
/// AI level, a level turned since (the lobby's, or step 1's row turning
/// every AI seat) left the seat to the AI for good. A seat set to a bot
/// or a person since keeps it.
pub fn open_the_seats_filled_at_start(config: &mut MatchConfig) {
    let filled = std::mem::take(&mut config.filled_at_start);
    for (seat, filled) in filled.into_iter().enumerate() {
        if filled && config.controllers[seat].ai().is_some() {
            config.controllers[seat] = SeatController::Local;
        }
    }
}

/// Put the map in force where the player's pick and the seat count say
/// ([`map_in_force`]), after something other than step 2 moved: the seat
/// count, or the shelf or a series between two visits.
pub fn settle_map(config: &mut MatchConfig, beaches: &CustomBeaches) {
    let pick = current_pick(config, beaches);
    place_pick(config, pick, beaches);
}

/// The player's pick: the one kept while the map is moved off it, else
/// the map in force's own card. A handmade beach no longer on the shelf
/// for this table names nothing, and gives way to the beach dealt to it.
fn current_pick(config: &MatchConfig, beaches: &CustomBeaches) -> Card {
    if let Some(pick) = config
        .pick
        .kept()
        .filter(|pick| on_the_shelf(*pick, beaches))
    {
        return pick;
    }
    card_in_force(config, beaches).unwrap_or(Card::Map(sized_for(config.seats)))
}

/// The card of the map in force. `None` for a handmade beach no longer on
/// the shelf for this table, which has no card; each caller says what
/// stands in for it.
fn card_in_force(config: &MatchConfig, beaches: &CustomBeaches) -> Option<Card> {
    Some(match config.map {
        MapChoice::Custom => Card::Shelf(beaches.on_shelf(config.seats, config.custom)?),
        map @ (MapChoice::Classic
        | MapChoice::GenSmall
        | MapChoice::GenClassic
        | MapChoice::GenLarge
        | MapChoice::GenXl
        | MapChoice::GenOcean
        | MapChoice::TheGates
        | MapChoice::TheMoat
        | MapChoice::LogJam
        | MapChoice::ThreeLanes
        | MapChoice::RoundTable
        | MapChoice::RoundTheWorld
        | MapChoice::Duel
        | MapChoice::TheFountain
        | MapChoice::FourDoors
        | MapChoice::GullRoad
        | MapChoice::BackToBack
        | MapChoice::Surprise) => Card::Map(map),
    })
}

/// Whether `card` names a beach there is: a handmade one must still be on
/// the shelf.
fn on_the_shelf(card: Card, beaches: &CustomBeaches) -> bool {
    match card {
        Card::Shelf(shelf) => shelf < beaches.0.len(),
        Card::Map(_) | Card::Random => true,
    }
}

/// The map in force for `pick` at this table, with `pick` kept only while
/// the map is moved off it. Under the Random card the map in force is
/// only the beach dealt until the draw, and the card stays picked, with
/// the pick kept beside it the same way.
fn place_pick(config: &mut MatchConfig, pick: Card, beaches: &CustomBeaches) {
    let (map, custom) = map_in_force(pick, config.seats, beaches);
    config.map = map;
    config.custom = custom;
    let moved = match pick {
        Card::Map(picked) => picked != map,
        Card::Shelf(_) => map != MapChoice::Custom,
        Card::Random => false,
    };
    config.pick = match (config.pick, moved) {
        (Pick::Random { .. }, moved) => Pick::Random {
            kept: moved.then_some(pick),
        },
        (Pick::InForce | Pick::Moved(_), true) => Pick::Moved(pick),
        (Pick::InForce | Pick::Moved(_), false) => Pick::InForce,
    };
}

/// The map a table of `seats` plays when the player picked `pick`, as
/// `(map, custom)`: the pick itself while it seats the table, and only
/// when it cannot, the closest generated map (or the classic arena) that
/// does. Closest by area, so a four-castle beach gives way to the large
/// one for five, and the 20x13 is never dealt ([`sized_for`]). Never a
/// handmade versus map, which is built for whole pairs: a table it does
/// not seat is one no other handmade map is fair at either. Random has no
/// one map, and is dealt the beach sized for the table until it draws.
pub fn map_in_force(pick: Card, seats: u8, beaches: &CustomBeaches) -> (MapChoice, usize) {
    let (w, h) = match pick {
        Card::Map(map) if holds(map, seats) => return (map, 0),
        Card::Map(map) => map.size(),
        Card::Shelf(shelf) => {
            if let Some(custom) = beaches.among_fitting(seats, shelf) {
                return (MapChoice::Custom, custom);
            }
            beaches
                .0
                .get(shelf)
                .map_or(MapChoice::Custom.size(), |beach| {
                    (beach.level.board().width(), beach.level.board().height())
                })
        }
        Card::Random => return (sized_for(seats), 0),
    };
    let area = |(w, h): (u8, u8)| i32::from(w) * i32::from(h);
    let wanted = area((w, h));
    let closest = MapChoice::ALL
        .into_iter()
        .filter(|&map| map.dealt() && map.tables().is_none() && holds(map, seats))
        .min_by_key(|&map| (area(map.size()) - wanted).abs())
        .unwrap_or(sized_for(seats));
    (closest, 0)
}

/// The pick and the map in force, while the seat count keeps them apart:
/// what step 2's note and step 3's line say, naming the pick, why, and
/// the map played instead. `None` under the Random card, which has no
/// pick to move.
pub fn map_moved(config: &MatchConfig, beaches: &CustomBeaches) -> Option<(Card, Card)> {
    let pick = match config.pick {
        Pick::Moved(pick) => Some(pick).filter(|pick| on_the_shelf(*pick, beaches))?,
        Pick::InForce | Pick::Random { .. } => return None,
    };
    Some((pick, picked_card(config, beaches)))
}

/// Make the map in force the pick: a map turned to by a dial other than
/// step 2's (the dev hook's) is the player's choice as it stands.
pub fn pick_the_map_in_force(config: &mut MatchConfig) {
    config.pick = Pick::InForce;
}

#[cfg(test)]
mod flow_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// The guide's line on big tables, read off [`holds`]: five players
    /// need a generated map at least [`WIDE_ENOUGH`] tiles across (the
    /// open ocean among them), Surprise or Gull Road, built for five, and
    /// six may also have the handmade maps built for six, Round Table (15
    /// across), Round the World and Back to Back. Not "16 across" for
    /// every map: Round Table is 15.
    #[test]
    fn the_maps_that_seat_five_and_six_are_the_guides() {
        let seating = |seats| -> Vec<MapChoice> {
            MapChoice::ALL
                .into_iter()
                .filter(|&map| map != MapChoice::Custom && holds(map, seats))
                .collect()
        };
        let wide: Vec<MapChoice> = MapChoice::ALL
            .into_iter()
            .filter(|&map| map.tables().is_none() && map.size().0 >= WIDE_ENOUGH)
            .filter(|&map| !matches!(map, MapChoice::Custom | MapChoice::Surprise))
            .collect();
        assert_eq!(
            wide,
            [MapChoice::GenLarge, MapChoice::GenXl, MapChoice::GenOcean]
        );
        let mut five = wide.clone();
        five.extend([MapChoice::Surprise, MapChoice::GullRoad]);
        assert_eq!(seating(5), five);
        let mut six = vec![
            MapChoice::RoundTable,
            MapChoice::RoundTheWorld,
            MapChoice::BackToBack,
            MapChoice::Surprise,
        ];
        six.extend(wide);
        let mut seat_six = seating(6);
        seat_six.sort_by_key(|map| MapChoice::ALL.iter().position(|m| m == map));
        six.sort_by_key(|map| MapChoice::ALL.iter().position(|m| m == map));
        assert_eq!(seat_six, six);
        assert_eq!(MapChoice::RoundTable.size().0, 15);
        let guide = include_str!("../../../docs/guide.md").replace("\n  ", " ");
        assert!(
            guide.contains(&format!(
                "five players need a generated map {WIDE_ENOUGH} tiles across or more"
            )),
            "the guide says the rule"
        );
    }

    /// The lobby's AI plays at step 1's level: the wire carries one level,
    /// and it is the one the setup said every AI seat plays at, whatever a
    /// single seat on the couch was turned to.
    #[test]
    fn the_lobbys_ai_plays_at_step_ones_level() {
        let mut config = MatchConfig::with_ai(3, 2);
        config.ai_level = BotLevel::Hard;
        config.controllers[2] = SeatController::Bot;
        config.controllers[1] = SeatController::Ai(BotLevel::Easy);
        config.lobby_bots = 2;
        let terms = terms(&config, crate::app::teams::TeamMode::Solo, 1);
        assert_eq!(terms.bot_level, BotLevel::Hard.index() as u8);
        assert_eq!(
            terms.bots, 2,
            "the lobby's own count, not the couch's seats"
        );
        let (back, _) = config_from_terms(&terms);
        assert_eq!((back.lobby_bots, back.ai_level), (2, BotLevel::Hard));
    }

    /// Player 1 stays a person whatever asked otherwise, and the lobby's
    /// AI dial is held to the seats an online table has beside its host.
    #[test]
    fn player_one_stays_a_person() {
        let mut config = MatchConfig {
            lobby_bots: 9,
            ..MatchConfig::with_ai(2, 1)
        };
        config.controllers[0] = SeatController::Ai(BotLevel::Easy);
        settle(&mut config, &CustomBeaches::default());
        assert_eq!(config.controller(0), SeatController::Local);
        assert_eq!(config.lobby_bots, MAX_BOTS as u8);
        assert_eq!(config.humans(), 1, "somebody still plays");
        assert_eq!(
            MatchConfig::with_ai(4, 9).ai_seats(),
            3,
            "never all of them"
        );
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
            p2_pressed(&keys, &crate::app::gamepad::Bridged::default(), settings)
        };
        let stock = GameSettings::default();
        assert!(pressed(KeyCode::KeyI, &stock), "P2's up");
        assert!(!pressed(KeyCode::KeyW, &stock), "P1's up");
        assert!(!pressed(KeyCode::Enter, &stock), "the menu's start");
        let one_hand = GameSettings {
            commit: CommitScheme::Ijkl,
            ..GameSettings::default()
        };
        // The versus round reads the keyboard as shared, so the one-hand
        // preset stands down there and IJKL is P2's to join with.
        assert!(
            pressed(KeyCode::KeyI, &one_hand),
            "P2's up under the preset, as the round plays it"
        );
        assert!(
            !pressed(KeyCode::ArrowUp, &one_hand),
            "P1's placement under the preset, as the round plays it"
        );

        // The menu walks on W/A/S/D and the arrows whoever holds them: with
        // P1's up moved to E and W given to P2, walking up is no arrival,
        // but P2's other keys still are.
        let mut walked = GameSettings::default();
        walked.binds[0].set(crate::app::binds::Action::MoveUp, KeyCode::KeyE);
        walked.binds[1].set(crate::app::binds::Action::Remove, KeyCode::KeyW);
        assert!(!pressed(KeyCode::KeyW, &walked), "the menu's up");
        assert!(pressed(KeyCode::KeyI, &walked), "P2's up");
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
        // And the roster, which names the beach before the launch, names
        // the same one the launch sends: none, for five. It used to skip
        // the count and name the beach the generated arena replaced.
        assert_eq!(
            beach_on_air(&config, 2, &shelf).map(|beach| beach.level.name.as_str()),
            Some("Two")
        );
        assert!(beach_on_air(&config, 5, &shelf).is_none());
    }

    /// Between rounds the terms move off a handmade beach the table has
    /// outgrown, onto the generated beach sized for it, so the terms name
    /// the beach that is played. One that still seats the table stays,
    /// and so does every built-in map that holds it.
    #[test]
    fn the_terms_give_up_a_beach_the_table_outgrew() {
        let two = crate::sim::Level::parse(
            "name: Two\nposts: 1\ncrab: 0,1 R R common\nmap:\n\
             +-+-+-+\n|0 . .|\n+ + + +\n|. . 1|\n+ + + +\n|. . .|\n+-+-+-+\n",
        )
        .expect("a level");
        let packed = crate::lzw::compress(two.to_text().as_bytes(), 8);
        let custom = MatchTerms {
            map: MapChoice::Custom.index() as u8,
            ..MatchTerms::default()
        };
        assert_eq!(
            fit_terms(custom, 2, &packed),
            custom,
            "two seats, two castles"
        );
        for seats in 3..=6u8 {
            let fitted = fit_terms(custom, seats, &packed);
            assert_eq!(
                MapChoice::from_index(usize::from(fitted.map)),
                sized_for(seats),
                "{seats} seats"
            );
            assert_eq!(
                MatchTerms {
                    map: custom.map,
                    ..fitted
                },
                custom,
                "nothing else"
            );
        }
        // No beach at all (the launch dropped it): the same.
        let fitted = fit_terms(custom, 2, &[]);
        assert_eq!(MapChoice::from_index(usize::from(fitted.map)), sized_for(2));
    }

    /// A handmade beach plays the tide-event wheel as every versus round
    /// does, on the couch and online: its file never says `events: on`,
    /// and it used to play without, its sparkling crabs spinning nothing.
    /// The replay of such a round says so.
    #[test]
    fn a_handmade_beach_turns_the_wheel() {
        let level = crate::sim::Level::parse(
            "name: Two\nposts: 1\nmap:\n\
             +-+-+-+\n|0 . .|\n+ + + +\n|. . 1|\n+ + + +\n|. . .|\n+-+-+-+\n",
        )
        .expect("a level");
        assert!(!level.board().events_enabled(), "the file says nothing");
        let packed = crate::lzw::compress(level.to_text().as_bytes(), 8);
        let terms = MatchTerms {
            map: MapChoice::Custom.index() as u8,
            ..MatchTerms::default()
        };
        let online = board_from(&terms, 2, &packed);
        assert_eq!((online.width(), online.height()), (3, 3), "on the beach");
        let config = MatchConfig {
            map: MapChoice::Custom,
            seats: 2,
            ..MatchConfig::default()
        };
        let shelf = CustomBeaches(vec![Beach::new(level)]);
        let couch = local_board(&config, &shelf, terms.seed);
        assert_eq!((couch.width(), couch.height()), (3, 3), "on the beach");
        for mut board in [online, couch] {
            assert!(board.events_enabled(), "the wheel is on");
            let replay = crate::sim::Level::from_board("Replay", 1, board.clone()).to_text();
            assert!(replay.contains("events: on"), "{replay}");
            // A sparkling crab walked into a castle spins it.
            let (x, y) = board.castle_of(1).expect("seat 1 has a castle");
            let (from, facing) = match x {
                0 => (x + 1, crate::sim::Direction::Left),
                _ => (x - 1, crate::sim::Direction::Right),
            };
            board.spawn_crab(
                from,
                y,
                facing,
                crate::sim::Handedness::Left,
                crate::sim::CrabKind::Sparkling,
            );
            for _ in 0..120 {
                board.tick_idle();
                if board.last_event().is_some() {
                    break;
                }
            }
            assert!(board.last_event().is_some(), "the bank spun an event");
        }
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

    /// A handmade beach with castles to spare is played by a smaller
    /// table, with the spare castles put away on the board every peer
    /// builds: nobody banks for a seat nobody plays. One whose castles
    /// skip a player is offered to no table at all.
    #[test]
    fn spare_castles_are_put_away_and_a_skipped_one_is_never_offered() {
        let beach = |row: &str| {
            crate::sim::Level::parse(&format!(
                "name: B\nposts: 1\nkind: arena\ncrab: 0,1 R R common\nmap:\n\
                 +-+-+-+\n|{row}|\n+ + + +\n|. . .|\n+-+-+-+\n"
            ))
            .expect("a level")
        };
        let three = beach("0 1 2");
        let packed = crate::lzw::compress(three.to_text().as_bytes(), 8);
        let terms = MatchTerms {
            map: MapChoice::Custom.index() as u8,
            ..MatchTerms::default()
        };
        let played = board_from(&terms, 2, &packed);
        assert_eq!((played.width(), played.height()), (3, 2), "the beach built");
        assert_eq!(played.seats_in_play(), 2, "the third castle put away");
        assert_eq!(played.tile_at(2, 0), crate::sim::TileKind::Empty);
        assert_eq!(board_from(&terms, 3, &packed).seats_in_play(), 3);

        let shelf = CustomBeaches(vec![Beach::new(three), Beach::new(beach("0 . 2"))]);
        for seats in 2..=3 {
            let fitting: Vec<&str> = shelf
                .fitting(seats)
                .iter()
                .map(|b| b.level.name.as_str())
                .collect();
            assert_eq!(fitting.len(), 1, "the gapped beach offered to {seats}");
        }
        assert!(beach_in_play(MapChoice::Custom, Some(&shelf.0[1].level), 2).is_none());
    }

    /// Which chairs the AI holds under a set of terms.
    ///
    /// Host and joiner each work this out from the same wire terms rather
    /// than being told, so it has to answer the same for both. It also
    /// indexes a per-seat array straight off a byte a peer chose, so a
    /// `bots` count larger than the table must reach no further.
    #[test]
    fn the_ai_holds_the_last_places_and_reaches_no_further() {
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
        // A host that picked partners puts the AI where the order says:
        // here the AI's two places in seats 1 and 3, the players in 0 and 2.
        let arranged = MatchTerms {
            bots: 2,
            order: [0, 2, 1, 3, 4, 5],
            ..MatchTerms::default()
        };
        let held: Vec<bool> = bot_seats_from(&arranged, 4)
            .iter()
            .map(Option::is_some)
            .collect();
        assert_eq!(held, [false, true, false, true, false, false]);
        assert_eq!(arranged.player_seats(4), [0, 2]);
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
        assert_eq!(beaches_note(2, &EN, &shelf), None, "it is on the dial");
        let note = beaches_note(4, &EN, &shelf).expect("four cannot sit at it");
        assert!(note.contains('4'), "{note}");

        // With nothing saved there is nothing to explain: an empty shelf is
        // not a shelf whose beaches are the wrong size.
        assert_eq!(beaches_note(4, &EN, &CustomBeaches::default()), None);

        // Castles 0 and 2 with no 1 seat no table at all, so the reason is
        // the skipped castle and not the size of this one.
        let gap = || {
            Beach::new(
                crate::sim::Level::parse(
                    "name: Gap\nposts: 1\nkind: arena\ncrab: 0,1 R R common\nmap:\n\
                     +-+-+-+\n|0 . .|\n+ + + +\n|. . 2|\n+ + + +\n|. . .|\n+-+-+-+\n",
                )
                .expect("a level"),
            )
        };
        assert_eq!(
            beaches_note(2, &EN, &CustomBeaches(vec![gap()])).as_deref(),
            Some(EN.map_skips_castle)
        );
        // Beside one that is merely too small, the table's size is the
        // reason, and a smaller table would fix it.
        let mut both = shelf;
        both.0.push(gap());
        let note = beaches_note(4, &EN, &both).expect("four cannot sit at either");
        assert!(note.contains('4'), "{note}");
    }

    /// The skipped-castle note sits where the seats note does, so it is no
    /// wider than the widest seats note any language already puts there.
    #[test]
    fn the_skipped_castle_note_fits_where_the_seats_note_does() {
        use crate::app::i18n::metrics::text_px;
        let size = 18.0;
        let widest = crate::app::i18n::ALL_LANGS
            .iter()
            .map(|lang| text_px(&fill(lang.tr().map_none_seats, &[("n", "6")]), size))
            .fold(0.0, f32::max);
        for lang in crate::app::i18n::ALL_LANGS {
            let note = lang.tr().map_skips_castle;
            let wide = text_px(note, size);
            assert!(
                wide <= widest,
                "{lang:?}: {note:?} is {wide:.0}px of {widest:.0}px"
            );
        }
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
            MapChoice::GullRoad,
            "past Custom, Classic and the handmade maps for other tables"
        );
        assert_eq!(config.seats, 5, "nobody left the table");
        next_map(&mut config, &shelf);
        assert_eq!(
            config.map,
            MapChoice::GenLarge,
            "past Back to Back, Small, 12x9"
        );
        next_map(&mut config, &shelf);
        assert_eq!(config.map, MapChoice::GenOcean, "past the 20x13");

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
            MapChoice::GullRoad
        );
        assert_eq!(next.seed, 2);
        let next = next_round_terms(terms, 2, 3);
        assert_eq!(
            MapChoice::from_index(usize::from(next.map)),
            MapChoice::Classic
        );
    }

    /// An online series on the host's handmade beach keeps naming it: the
    /// beach itself is re-sent with every round, so the terms must say
    /// `Custom` too, at any table size. (Until the table outgrows the
    /// beach's castles, which [`fit_terms`] decides with the beach in hand:
    /// see `the_terms_give_up_a_beach_the_table_outgrew`.)
    #[test]
    fn a_series_on_a_handmade_beach_stays_on_it() {
        let terms = MatchTerms {
            map: MapChoice::Custom.index() as u8,
            seed: 1,
            ..MatchTerms::default()
        };
        for seats in 2..=6 {
            let next = next_round_terms(terms, seats, 9);
            assert_eq!(
                MapChoice::from_index(usize::from(next.map)),
                MapChoice::Custom,
                "{seats} seats"
            );
            assert_eq!(next, MatchTerms { seed: 9, ..terms }, "only the seed moves");
        }
    }

    /// A table that grew after its map was picked is moved off a beach too
    /// small for it, and one that still fits stays put.
    #[test]
    fn a_grown_table_is_fitted_to_a_beach_that_holds_it() {
        let classic = MatchTerms {
            map: MapChoice::Classic.index() as u8,
            seed: 7,
            ..MatchTerms::default()
        };
        assert_eq!(
            fit_terms(classic, 4, &[]),
            classic,
            "four fit the classic arena"
        );
        let six = fit_terms(classic, 6, &[]);
        assert!(holds(MapChoice::from_index(usize::from(six.map)), 6));
        assert_eq!(six.seed, 7, "only the map moves");
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
        assert_eq!(
            config.map,
            sized_for(3),
            "no beach to name: the one dealt to the table"
        );

        config.map = MapChoice::Custom;
        config.seats = 5;
        settle_map(&mut config, &shelf);
        assert_eq!(config.map, MapChoice::GenLarge, "and wide enough for five");
    }

    /// A series deals every beach but the 20x13, whatever the table: a
    /// pair stepped onto it got 130 tiles each.
    #[test]
    fn a_series_never_deals_the_largest_beach() {
        let shelf = CustomBeaches::default();
        for seats in 2..=MAX_PLAYERS as u8 {
            let mut config = MatchConfig {
                seats,
                ..MatchConfig::default()
            };
            for _ in 0..2 * MapChoice::ALL.len() {
                next_map(&mut config, &shelf);
                assert_ne!(config.map, MapChoice::GenXl, "{seats} seats");
                assert!(holds(config.map, seats));
            }
        }
    }

    /// A re-read of the shelf keeps the config on the beach it named, by
    /// its file, wherever the beach now sorts; a file gone gives way to
    /// the map sized for the table, and says which beach went.
    #[test]
    fn a_reread_shelf_keeps_the_picked_beach_by_its_file() {
        let beach = |name: &str| {
            let text = format!(
                "name: {name}\nposts: 1\nkind: arena\ncrab: 0,1 R R common\nmap:\n\
                 +-+-+-+\n|0 . 1|\n+ + + +\n|. . .|\n+ + + +\n|. . .|\n+-+-+-+\n"
            );
            let mut level = crate::sim::Level::parse(&text).expect("a level");
            level.filing = Some(crate::sim::Filing {
                key: format!("file:custom/{name}.txt"),
                legacy: None,
            });
            Beach::new(level)
        };
        let old = CustomBeaches(vec![beach("b"), beach("c")]);
        let mut config = MatchConfig {
            map: MapChoice::Custom,
            seats: 2,
            custom: 1,
            ..MatchConfig::default()
        };
        // A file sorting ahead of the pick turns up meanwhile.
        let fresh = CustomBeaches(vec![beach("a"), beach("b"), beach("c")]);
        assert_eq!(beaches::keep_the_pick(&mut config, &old, &fresh), None);
        assert_eq!(config.map, MapChoice::Custom);
        assert_eq!(fresh.fitting(2)[config.custom].level.name, "c");
        // And then the pick's own file goes: no other beach is played.
        let fewer = CustomBeaches(vec![beach("a"), beach("b")]);
        let said = beaches::keep_the_pick(&mut config, &fresh, &fewer);
        assert_eq!(said.as_deref(), Some("c"));
        assert_eq!(config.map, sized_for(2));
        assert!(holds(config.map, 2));
        assert_eq!(
            beaches::keep_the_pick(&mut config, &fewer, &fewer),
            None,
            "said once"
        );
        // A shelf pick kept beside the Random card, while the seat count
        // has the map off it, follows its file the same way, and the card
        // stays picked when the file is gone.
        let mut config = MatchConfig {
            pick: Pick::Random {
                kept: Some(Card::Shelf(1)),
            },
            ..MatchConfig::default()
        };
        assert_eq!(beaches::keep_the_pick(&mut config, &old, &fresh), None);
        assert_eq!(
            config.pick,
            Pick::Random {
                kept: Some(Card::Shelf(2))
            }
        );
        let _ = beaches::keep_the_pick(&mut config, &fresh, &fewer);
        assert_eq!(config.pick, Pick::Random { kept: None });
    }

    /// The seat count turning keeps a map that was chosen for what it is:
    /// the 20x13 picked by hand, and the same handmade beach while it
    /// still seats everyone, though it now sits elsewhere in the list of
    /// the ones that fit.
    #[test]
    fn reseating_keeps_the_beach_that_was_chosen() {
        let beach = |name: &str, castles: &str| {
            let text = format!(
                "name: {name}\nposts: 1\nkind: arena\ncrab: 0,1 R R common\nmap:\n\
                 +-+-+-+\n|{castles}|\n+ + + +\n|. . .|\n+ + + +\n|. . .|\n+-+-+-+\n"
            );
            Beach::new(crate::sim::Level::parse(&text).expect("a level"))
        };
        let shelf = CustomBeaches(vec![beach("Pair", "0 . 1"), beach("Trio", "0 1 2")]);
        let mut config = MatchConfig {
            map: MapChoice::Custom,
            seats: 2,
            custom: 1,
            ..MatchConfig::default()
        };
        reseat(&mut config, 3, &shelf);
        assert_eq!((config.map, config.seats), (MapChoice::Custom, 3));
        let fitting = shelf.fitting(config.seats);
        assert_eq!(fitting[config.custom].level.name, "Trio");
        reseat(&mut config, 2, &shelf);
        assert_eq!(shelf.fitting(2)[config.custom].level.name, "Trio");
        reseat(&mut config, 4, &shelf);
        assert!(holds(config.map, 4) && config.map != MapChoice::Custom);

        let mut xl = MatchConfig {
            map: MapChoice::GenXl,
            seats: 4,
            ..MatchConfig::default()
        };
        reseat(&mut xl, 2, &shelf);
        assert_eq!(xl.map, MapChoice::GenXl, "chosen by hand, kept");
        let mut small = MatchConfig {
            map: MapChoice::GenSmall,
            seats: 2,
            ..MatchConfig::default()
        };
        reseat(&mut small, 5, &shelf);
        assert_eq!(small.map, sized_for(5));
        let mut classic = MatchConfig {
            map: MapChoice::Classic,
            ..MatchConfig::with_ai(4, 3)
        };
        reseat(&mut classic, 5, &shelf);
        assert!(holds(classic.map, 5));
        assert_eq!(
            classic.controller(4),
            SeatController::Local,
            "a seat added is a person's, open until somebody sits there"
        );
        reseat(&mut classic, 2, &shelf);
        assert_eq!(
            classic.ai_seats(),
            1,
            "the AI holds at most every seat but one"
        );
    }

    /// A seat dialled away and back keeps who it was set to: the
    /// controllers past the count are kept for when it comes back up.
    #[test]
    fn a_seat_dialled_away_and_back_keeps_its_choice() {
        let shelf = CustomBeaches::default();
        let mut config = MatchConfig {
            seats: 4,
            ..MatchConfig::default()
        };
        config.controllers[3] = SeatController::Ai(BotLevel::Hard);
        config.controllers[2] = SeatController::Bot;
        reseat(&mut config, 2, &shelf);
        reseat(&mut config, 4, &shelf);
        assert_eq!(config.controller(3), SeatController::Ai(BotLevel::Hard));
        assert_eq!(config.controller(2), SeatController::Bot);
    }

    /// The beach dealt to a table seats it, and grows with it: two players
    /// get the smallest, and nobody is dealt the 20x13 without asking.
    #[test]
    fn the_beach_sized_for_a_table_seats_it() {
        let mut last = 0;
        for seats in 2..=MAX_PLAYERS as u8 {
            let map = sized_for(seats);
            assert!(holds(map, seats), "{seats} seats on {map:?}");
            assert_ne!(map, MapChoice::GenXl, "{seats} seats");
            let (w, h) = map.size();
            let area = u32::from(w) * u32::from(h);
            assert!(area >= last, "{seats} seats got a smaller beach");
            last = area;
        }
        assert_eq!(sized_for(2), MapChoice::GenSmall);
    }

    /// The terms' team mode reaches the board every peer builds, where the
    /// AI and Gull Attack read it, and survives a snapshot and a recording;
    /// a mode the table does not fit is free-for-all there too.
    #[test]
    fn the_terms_split_the_board_into_teams() {
        use crate::app::teams::TeamMode;
        let terms = |mode: TeamMode| MatchTerms {
            teams: mode.index() as u8,
            seed: 5,
            ..MatchTerms::default()
        };
        let pairs = board_for(&terms(TeamMode::Pairs), 4);
        assert_eq!(pairs.rules().teams, Some([0, 0, 1, 1, 2, 2]));
        assert!(pairs.rules().allies(0, 1) && !pairs.rules().allies(1, 2));
        assert_eq!(
            board_from(&terms(TeamMode::Pairs), 4, &[]).rules().teams,
            pairs.rules().teams
        );
        assert_eq!(board_for(&terms(TeamMode::Pairs), 3).rules().teams, None);
        assert_eq!(board_for(&terms(TeamMode::Solo), 4).rules().teams, None);
        let trios = board_for(&terms(TeamMode::Trios), 6);
        assert_eq!(trios.rules().teams, Some([0, 1, 0, 1, 0, 1]));
        let restored = crate::sim::Board::parse_snapshot(&trios.to_snapshot()).expect("snapshot");
        assert_eq!(restored.rules().teams, trios.rules().teams);
        let level = crate::sim::Level::from_board("Versus", 3, trios.clone());
        let replayed = crate::sim::Level::parse(&level.to_text()).expect("level");
        assert_eq!(replayed.board().rules().teams, trios.rules().teams);
        assert_eq!(replayed.board().state_hash(), trios.state_hash());
    }

    /// Open ocean, Round the World, Four Doors, Gull Road and Back to Back
    /// are the beaches with no edges, each at a table it seats. Every other
    /// map choice stays walled, or a beach changes shape under everyone.
    #[test]
    fn only_the_open_beaches_have_no_edges() {
        for map in MapChoice::ALL {
            let terms = MatchTerms {
                map: map.index() as u8,
                seed: 99,
                ..MatchTerms::default()
            };
            let seats = map.tables().map_or(4, |tables| tables[0]);
            let board = board_for(&terms, seats);
            assert_eq!(
                board.wrap(),
                matches!(
                    map,
                    MapChoice::GenOcean
                        | MapChoice::RoundTheWorld
                        | MapChoice::FourDoors
                        | MapChoice::GullRoad
                        | MapChoice::BackToBack
                ),
                "{map:?} wraps: {}",
                board.wrap()
            );
            assert_eq!(board.wrap(), map.wraps(), "{map:?}");
        }
        // And it is appended, so an older settings file or a `Start` from
        // another machine still names the beach it meant.
        for (index, map) in MapChoice::ALL.iter().enumerate() {
            assert_eq!(MapChoice::from_index(index), *map);
        }
        assert_eq!(MapChoice::from_index(4), MapChoice::GenXl, "xl stayed put");
    }
}
