//! What step 2's cards and notes say: a tile's words, a slot's note, and
//! the line a moved seat or a chosen card is told by.

use super::*;

/// What a tile reads.
pub(in crate::app::match_setup) struct TileWords {
    pub badge: &'static str,
    pub name: String,
    pub sub: String,
    /// The "seats N, not M" pill, over a card the table cannot play.
    pub pill: Option<String>,
}

/// The words on `card`'s tile at a table of `seats`, `online` the lobby
/// host's pick, where a beach that cannot travel is greyed too.
pub(in crate::app::match_setup) fn tile_words(
    tr: &crate::app::i18n::Tr,
    card: Card,
    seats: u8,
    beaches: &CustomBeaches,
    online: bool,
) -> TileWords {
    let range = card_seats(card, beaches);
    let span = |sub: &str, (w, h): (u8, u8)| {
        let (least, most) = range.unwrap_or((0, 0));
        fill(
            sub,
            &[
                ("w", &w.to_string()),
                ("h", &h.to_string()),
                ("a", &least.to_string()),
                ("b", &most.to_string()),
                ("n", &most.to_string()),
            ],
        )
    };
    let pill = match (card_fits(card, seats, beaches), range) {
        (true, _) if card_open(card, seats, beaches, online) => None,
        (true, _) => Some(tr.setup_local_only.to_string()),
        (false, Some(_)) => Some(fill(
            tr.setup_seats_not,
            &[("s", &seats_said(card, beaches)), ("n", &seats.to_string())],
        )),
        (false, None) => Some(tr.setup_seats_none.to_string()),
    };
    match card {
        Card::Map(MapChoice::Surprise) => TileWords {
            badge: tr.setup_badges[1],
            name: map_title(tr, MapChoice::Surprise),
            sub: tr.setup_surprise_sub.to_string(),
            pill,
        },
        Card::Map(map) => TileWords {
            badge: match map {
                MapChoice::Classic
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
                | MapChoice::BackToBack => tr.setup_badges[0],
                MapChoice::GenSmall
                | MapChoice::GenClassic
                | MapChoice::GenLarge
                | MapChoice::GenXl
                | MapChoice::GenOcean
                | MapChoice::Custom
                | MapChoice::Surprise => tr.setup_badges[1],
            },
            name: map_title(tr, map),
            // A map with no edges says so whatever its tables: crabs and
            // gulls walking off one side come back on the other, and
            // nothing else on the page says it.
            sub: match map.tables() {
                // The tables it is built for, as the pill and the note
                // say them: "2/4", never a range with an odd one inside.
                Some(_) => fill(
                    match map.wraps() {
                        true => tr.setup_map_sub_exact_wrap,
                        false => tr.setup_map_sub_exact,
                    },
                    &[
                        ("w", &map.size().0.to_string()),
                        ("h", &map.size().1.to_string()),
                        ("n", &seats_said(card, beaches)),
                    ],
                ),
                None => span(
                    match map.wraps() {
                        true => tr.setup_map_sub_wrap,
                        false => tr.setup_map_sub,
                    },
                    map.size(),
                ),
            },
            pill,
        },
        Card::Shelf(shelf) => {
            let beach = &beaches.0[shelf];
            let size = (beach.level.board().width(), beach.level.board().height());
            let mut sub = match range {
                Some(_) if card_lists_its_tables(card, beaches) => fill(
                    tr.setup_map_sub_exact,
                    &[
                        ("w", &size.0.to_string()),
                        ("h", &size.1.to_string()),
                        ("n", &seats_said(card, beaches)),
                    ],
                ),
                Some((least, most)) if least == most => span(tr.setup_map_sub_exact, size),
                Some(_) => span(tr.setup_map_sub, size),
                None => format!("{}x{}", size.0, size.1),
            };
            if beach.too_big_to_send() {
                sub = format!("{sub} · {}", tr.setup_local_only);
            }
            TileWords {
                badge: tr.setup_badges[2],
                name: card_name(&beach.level.name),
                sub,
                pill,
            }
        }
        Card::Random => TileWords {
            badge: tr.setup_badges[3],
            name: tr.setup_map_names[MapChoice::Custom.index()].to_string(),
            sub: fill(tr.setup_random_sub, &[("n", &seats.to_string())]),
            pill,
        },
    }
}

/// The words on place `slot`'s tile: a card's ([`tile_words`]), or the
/// player's own badge over a name and a line that send them to the editor.
pub(in crate::app::match_setup) fn slot_words(
    tr: &crate::app::i18n::Tr,
    slot: Slot,
    seats: u8,
    beaches: &CustomBeaches,
    online: bool,
) -> TileWords {
    match slot {
        Slot::Card(card) => tile_words(tr, card, seats, beaches, online),
        Slot::MakeOne => TileWords {
            badge: tr.setup_badges[2],
            name: tr.setup_make_one.to_string(),
            sub: tr.setup_make_one_sub.to_string(),
            pill: None,
        },
    }
}

/// The line under step 2's cards for place `slot`: a card's
/// ([`card_note`]), or how to make a map of one's own, naming the menu's
/// editor entry and the editor's list of levels as they are named there.
pub(in crate::app::match_setup) fn slot_note(
    tr: &crate::app::i18n::Tr,
    slot: Slot,
    config: &MatchConfig,
    beaches: &CustomBeaches,
    online: bool,
) -> String {
    use crate::app::menu_scene::MenuEntry;
    match slot {
        Slot::Card(card) => card_note(tr, card, config, beaches, online),
        Slot::MakeOne => {
            let editor = MenuEntry::ALL
                .iter()
                .position(|entry| *entry == MenuEntry::Driftwood)
                .map_or("", |entry| tr.menu_names[entry]);
            fill(
                tr.setup_make_one_note,
                &[("e", editor), ("l", tr.ed_levels_title)],
            )
        }
    }
}

/// The tables `card` seats, as the pill, the size line and the note say
/// them: the list a handmade map or a shelf beach names ("2/4"), else the
/// most it seats.
pub(in crate::app::match_setup) fn seats_said(card: Card, beaches: &CustomBeaches) -> String {
    if card_lists_its_tables(card, beaches) {
        let tables: Vec<String> = card_tables(card, beaches)
            .iter()
            .map(u8::to_string)
            .collect();
        return tables.join("/");
    }
    card_seats(card, beaches).map_or_else(String::new, |(_, most)| most.to_string())
}

/// The line saying the seat count moved the map off the player's pick
/// ([`map_moved`]): the pick, the count it does not seat, and the map
/// played instead. `None` while the map in force is the pick.
pub fn moved_note(
    tr: &crate::app::i18n::Tr,
    config: &MatchConfig,
    beaches: &CustomBeaches,
) -> Option<String> {
    let (pick, now) = map_moved(config, beaches)?;
    let name = |card: Card| tile_words(tr, card, config.seats, beaches, false).name;
    Some(fill(
        tr.setup_map_moved,
        &[
            ("old", &name(pick)),
            ("new", &name(now)),
            ("n", &config.seats.to_string()),
        ],
    ))
}

/// The line under step 2's cards for the card under the cursor: what the
/// map is and how many it seats, or why a greyed one cannot be played,
/// or, on the map in force while the seat count keeps it off the pick,
/// that it moved ([`moved_note`]). `online`, the lobby host's pick: the
/// table is whoever turned up, so a greyed card names the table rather
/// than sending the host to a Players row it does not have, and a beach
/// that cannot travel says so.
pub(crate) fn card_note(
    tr: &crate::app::i18n::Tr,
    card: Card,
    config: &MatchConfig,
    beaches: &CustomBeaches,
    online: bool,
) -> String {
    let name = |card: Card| tile_words(tr, card, config.seats, beaches, online).name;
    if !online
        && card == picked_card(config, beaches)
        && let Some(note) = moved_note(tr, config, beaches)
    {
        return note;
    }
    let range = card_seats(card, beaches);
    let listed = card_lists_its_tables(card, beaches);
    let table = config.seats.to_string();
    if !card_fits(card, config.seats, beaches) {
        return match (range, listed, online) {
            (Some(_), true, false) => fill(
                tr.setup_greyed_tables,
                &[("m", &name(card)), ("s", &seats_said(card, beaches))],
            ),
            (Some((_, most)), false, false) => fill(
                tr.setup_greyed_note,
                &[("m", &name(card)), ("s", &most.to_string())],
            ),
            (Some(_), _, true) => fill(
                tr.host_map_greyed,
                &[
                    ("m", &name(card)),
                    ("s", &seats_said(card, beaches)),
                    ("n", &table),
                ],
            ),
            (None, _, _) => format!("{}: {}", name(card), tr.map_skips_castle),
        };
    }
    if !card_open(card, config.seats, beaches, online) {
        return fill(tr.host_map_too_big, &[("m", &name(card))]);
    }
    let seats = match range {
        Some(_) if listed => fill(tr.setup_seats_exact, &[("n", &seats_said(card, beaches))]),
        Some((least, most)) if least == most => {
            fill(tr.setup_seats_exact, &[("n", &most.to_string())])
        }
        Some((least, most)) => fill(
            tr.setup_seats_range,
            &[("a", &least.to_string()), ("b", &most.to_string())],
        ),
        None => String::new(),
    };
    let about = match card {
        Card::Map(MapChoice::Surprise) => {
            return format!(
                "{}: {}",
                name(card),
                fill(
                    tr.setup_surprise_note,
                    &[("n", &crate::sim::SURPRISE_LANES_MOST.to_string())]
                )
            );
        }
        Card::Map(map) => map_about(tr, map).to_string(),
        Card::Shelf(shelf) => match beaches.0[shelf].too_big_to_send() {
            true => format!("{} {}", tr.setup_custom_about, tr.setup_local_note),
            false => tr.setup_custom_about.to_string(),
        },
        Card::Random => {
            return format!(
                "{}: {}",
                name(card),
                fill(tr.setup_random_note, &[("n", &config.seats.to_string())])
            );
        }
    };
    format!("{}: {about} {seats}", name(card))
}
