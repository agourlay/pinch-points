//! Step 2's map cards: the sections and slots they sit in, the rules each
//! card states, and picking one.

use super::*;

/// One of step 2's cards: a built-in map, a beach off the player's shelf
/// (by its place on the whole shelf), or Random.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Card {
    Map(MapChoice),
    Shelf(usize),
    Random,
}

/// Step 2's three sections, top to bottom, each a heading over a grid of
/// cards: the maps that ship with the game, the player's own, and the two
/// cards that pick the map for the player.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapSection {
    /// The handmade maps and the generated sizes.
    BuiltIn,
    /// Every beach on the player's shelf, or the card saying how to make
    /// one while there is none.
    Yours,
    /// Surprise and Random.
    Chance,
}

impl MapSection {
    pub const ALL: [MapSection; 3] = [MapSection::BuiltIn, MapSection::Yours, MapSection::Chance];

    pub fn index(self) -> usize {
        match self {
            MapSection::BuiltIn => 0,
            MapSection::Yours => 1,
            MapSection::Chance => 2,
        }
    }

    /// The section a map's card sits in, which is also its stretch of the
    /// dial ([`MapChoice::DIAL`]).
    pub fn of(map: MapChoice) -> MapSection {
        match map {
            MapChoice::Classic
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
            | MapChoice::BackToBack => MapSection::BuiltIn,
            MapChoice::Custom => MapSection::Yours,
            MapChoice::Surprise => MapSection::Chance,
        }
    }
}

/// One place in step 2's grid: a card, or the card standing in for the
/// player's own maps while the shelf is empty, which says how to make one.
/// That one is not a map: the cursor may rest on it so its line can
/// explain, but it is never picked and Enter does not go on from it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Card(Card),
    MakeOne,
}

/// Step 2's places in the order they are laid out, each with its section:
/// the built-in maps in the dial's order ([`MapChoice::DIAL`]: the
/// handmade maps, then the generated sizes), every beach on the player's
/// shelf (fitting the table or not, so a beach too small is greyed rather
/// than missing) or, with none, the card saying how to make one, then
/// Surprise and Random.
pub fn map_slots(beaches: &CustomBeaches) -> Vec<(MapSection, Slot)> {
    let maps_in = |section: MapSection| {
        MapChoice::DIAL
            .into_iter()
            .filter(move |&map| map != MapChoice::Custom && MapSection::of(map) == section)
            .map(move |map| (section, Slot::Card(Card::Map(map))))
    };
    let yours: Vec<(MapSection, Slot)> = match beaches.0.is_empty() {
        true => vec![(MapSection::Yours, Slot::MakeOne)],
        false => (0..beaches.0.len())
            .map(|shelf| (MapSection::Yours, Slot::Card(Card::Shelf(shelf))))
            .collect(),
    };
    maps_in(MapSection::BuiltIn)
        .chain(yours)
        .chain(maps_in(MapSection::Chance))
        .chain(std::iter::once((
            MapSection::Chance,
            Slot::Card(Card::Random),
        )))
        .collect()
}

/// Step 2's cards, in the order they are laid out ([`map_slots`]), without
/// the card saying how to make a map.
pub fn cards(beaches: &CustomBeaches) -> Vec<Card> {
    map_slots(beaches)
        .into_iter()
        .filter_map(|(_, slot)| match slot {
            Slot::Card(card) => Some(card),
            Slot::MakeOne => None,
        })
        .collect()
}

/// How many players `card` seats, least and most, or `None` for a
/// handmade beach whose castles skip a number and so seat no table. Read
/// off [`card_tables`], so the label, the picture and the greying all go
/// by the one rule.
pub fn card_seats(card: Card, beaches: &CustomBeaches) -> Option<(u8, u8)> {
    let tables = card_tables(card, beaches);
    Some((*tables.first()?, *tables.last()?))
}

/// Every table `card` can be played by, smallest first: the counts in
/// [`SEATS_RANGE`] that [`card_fits`] takes, which for a beach off the
/// shelf is what [`crate::sim::Level::fits`] says, its `seats:` line
/// included. Counted off the castles alone, a `seats: 4` beach was
/// labelled "2-4" while greyed for two, and drawn for two, which it does
/// not seat, so its card had no picture.
pub fn card_tables(card: Card, beaches: &CustomBeaches) -> Vec<u8> {
    SEATS_RANGE
        .filter(|&seats| card_fits(card, seats, beaches))
        .collect()
}

/// Whether `card` names the tables it is built for, a handmade map's own
/// list or a shelf beach's `seats:` line, which its card says as that
/// list ("2/4") rather than as a range with an odd one inside.
pub fn card_lists_its_tables(card: Card, beaches: &CustomBeaches) -> bool {
    match card {
        Card::Map(map) => map.tables().is_some(),
        Card::Shelf(shelf) => beaches
            .0
            .get(shelf)
            .is_some_and(|beach| !beach.level.tables.is_empty()),
        Card::Random => false,
    }
}

/// Whether `card` can be played by a table of `seats`. A card that cannot
/// is greyed: the cursor may rest on it, so its line can say why, but it
/// is not picked and Enter does not go on from it.
pub fn card_fits(card: Card, seats: u8, beaches: &CustomBeaches) -> bool {
    match card {
        Card::Map(map) => holds(map, seats),
        Card::Shelf(shelf) => beaches.among_fitting(seats, shelf).is_some(),
        Card::Random => true,
    }
}

/// Whether `card` can be sent to every peer at an online table: a beach
/// off the shelf too big for an invitation plays on this machine only
/// ([`Beach::too_big_to_send`]).
pub fn card_travels(card: Card, beaches: &CustomBeaches) -> bool {
    match card {
        Card::Shelf(shelf) => beaches
            .0
            .get(shelf)
            .is_some_and(|beach| !beach.too_big_to_send()),
        Card::Map(_) | Card::Random => true,
    }
}

/// Whether `card` can be picked for a table of `seats`: it seats the table
/// ([`card_fits`]) and, `online`, it travels ([`card_travels`]). The one
/// rule the greying, the pick and the line under the cards read.
pub fn card_open(card: Card, seats: u8, beaches: &CustomBeaches, online: bool) -> bool {
    card_fits(card, seats, beaches) && (!online || card_travels(card, beaches))
}

/// The card step 2 lights as picked: Random under the Random card, else
/// the card of the map in force, which is the player's pick unless the
/// seat count moved it ([`MatchConfig::pick`]).
///
/// A handmade beach no longer on the shelf for the table lights the XL
/// card, not the beach [`settle_map`] would deal it: until something
/// settles the map, both the local launch and the wire build `Custom` as
/// a generated 20x13, which [`map_label`] names the same way.
pub fn picked_card(config: &MatchConfig, beaches: &CustomBeaches) -> Card {
    match config.pick {
        Pick::Random { .. } => Card::Random,
        Pick::InForce | Pick::Moved(_) => {
            card_in_force(config, beaches).unwrap_or(Card::Map(MapChoice::GenXl))
        }
    }
}

/// Pick `card`, when the table fits it: the map in force is then the
/// pick itself, and nothing is said to have moved.
pub fn pick_card(config: &mut MatchConfig, card: Card, beaches: &CustomBeaches) {
    if !card_fits(card, config.seats, beaches) {
        return;
    }
    match card {
        Card::Map(map) => {
            config.map = map;
            config.pick = Pick::InForce;
        }
        Card::Shelf(shelf) => {
            if let Some(custom) = beaches.among_fitting(config.seats, shelf) {
                config.map = MapChoice::Custom;
                config.custom = custom;
                config.pick = Pick::InForce;
            }
        }
        // A pick the seat count moved the map off stays kept beside it.
        Card::Random => {
            config.pick = Pick::Random {
                kept: config.pick.kept(),
            }
        }
    }
}

/// The config a card's match is played on, for drawing its preview: the
/// config with the card's map picked. `None` for Random and Surprise,
/// which have no one beach to show, and for a card that seats no table. A card the table
/// does not fit is drawn for the least table it does fit, whatever the
/// table's count, so the count moving does not redraw it.
pub fn card_config(
    config: &MatchConfig,
    card: Card,
    beaches: &CustomBeaches,
) -> Option<MatchConfig> {
    let seats = match card_fits(card, config.seats, beaches) {
        true => config.seats,
        false => card_seats(card, beaches)?.0,
    };
    let mut shown = MatchConfig {
        seats,
        pick: Pick::InForce,
        ..config.clone()
    };
    match card {
        // A new beach every round: its card has art of its own rather
        // than one round's picture.
        Card::Map(MapChoice::Surprise) => return None,
        Card::Map(map) => shown.map = map,
        Card::Shelf(shelf) => {
            shown.map = MapChoice::Custom;
            shown.custom = beaches.among_fitting(seats, shelf)?;
        }
        Card::Random => return None,
    }
    Some(shown)
}
