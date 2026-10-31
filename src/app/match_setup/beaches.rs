//! The shelf of handmade beaches, and how one goes over the wire.
//!
//! The setup screen only turns the dial; this is the half the online path
//! leans on: reading the player's levels off disk, the LZW packing that
//! has to fit a datagram, and the note that says why a beach is not
//! offered. Nothing here spawns UI.

use super::*;

/// The handmade beaches on this machine, as of the last time a screen that
/// offers them opened.
///
/// A resource and not a function call, because the two places that read it
/// are drawn every frame: as a call, `map_label` went to the disk and
/// parsed every level file sixty times a second.
#[derive(Resource, Default)]
pub struct CustomBeaches(pub Vec<Beach>);

/// One handmade beach on the shelf, with the bytes it would travel as.
///
/// Packed once when the shelf is read rather than once per invitation:
/// the size decides whether the beach can be sent at all, and that answer
/// is wanted by a menu label drawn every frame.
pub struct Beach {
    pub level: crate::sim::Level,
    pub(super) wire: Vec<u8>,
}

impl Beach {
    pub(crate) fn new(level: crate::sim::Level) -> Beach {
        let wire = crate::lzw::compress(level.to_text().as_bytes(), 8);
        Beach { level, wire }
    }

    /// Too big to fit an invitation. Such a beach is still perfectly
    /// playable at this table; it just cannot travel to another one, so
    /// the dial says so rather than letting the host find out by having
    /// nobody join.
    pub fn too_big_to_send(&self) -> bool {
        self.wire.len() > crate::transport::MAX_BEACH_BYTES
    }
}

/// A handmade beach a local round was set up on whose file was gone when
/// the shelf was read again to play it, by the name it was picked under:
/// the round plays the generated beach sized for the table instead, and
/// the feed says so ([`say_beach_gone`]). Written by every re-read.
#[derive(Resource, Default)]
pub struct BeachGone(pub Option<String>);

/// Re-read the shelf. Runs on entering the screens that offer a beach, so
/// one saved in the editor a moment ago is on the dial, and on entering
/// the arena, so a local round reads its beach off disk as it stands.
///
/// The config names its beach by place on the shelf, so a file added or
/// taken away meanwhile (by another copy of the game, or by hand) moved
/// the pick onto a different beach. It is found again by its file here
/// ([`keep_the_pick`]).
pub fn refresh_custom_beaches(
    mut beaches: ResMut<CustomBeaches>,
    mut config: ResMut<MatchConfig>,
    mut gone: ResMut<BeachGone>,
) {
    let fresh = CustomBeaches(
        crate::app::campaign::custom_arenas(crate::app::campaign::load_custom_levels())
            .into_iter()
            .map(Beach::new)
            .collect(),
    );
    gone.0 = keep_the_pick(&mut config, &beaches, &fresh);
    *beaches = fresh;
}

/// Point `config` at the beaches it named on the `old` shelf where they
/// sit on the `fresh` one, by the file each was read from: the map in
/// force and the pick kept while the map is moved off it. A beach in
/// force whose file is gone gives way to the map sized for the table,
/// as a pick no longer on the shelf does ([`settle_map`]), and its name
/// is returned to be said.
pub(super) fn keep_the_pick(
    config: &mut MatchConfig,
    old: &CustomBeaches,
    fresh: &CustomBeaches,
) -> Option<String> {
    let file = |beach: &Beach| beach.level.filing.as_ref().map(|filing| filing.key.clone());
    // Kept beside the Random card too, which keeps it the same way.
    if let Some(Card::Shelf(shelf)) = config.pick.kept() {
        let found = old
            .0
            .get(shelf)
            .and_then(file)
            .and_then(|key| fresh.0.iter().position(|b| file(b).as_ref() == Some(&key)))
            .map(Card::Shelf);
        config.pick = match config.pick {
            Pick::Random { .. } => Pick::Random { kept: found },
            Pick::InForce | Pick::Moved(_) => found.map_or(Pick::InForce, Pick::Moved),
        };
    }
    if config.map != MapChoice::Custom {
        return None;
    }
    let picked = old.fitting(config.seats).get(config.custom).copied()?;
    let key = file(picked)?;
    let found = fresh
        .fitting(config.seats)
        .iter()
        .position(|beach| file(beach).as_ref() == Some(&key));
    match found {
        Some(at) => {
            config.custom = at;
            None
        }
        None => {
            config.map = sized_for(config.seats);
            config.custom = 0;
            Some(picked.level.name.clone())
        }
    }
}

/// Say on the feed that the beach a round set up here was picked on is
/// gone, over the beach it is played on instead. After the sidebars,
/// which start the feed afresh.
pub fn say_beach_gone(
    mut gone: ResMut<BeachGone>,
    table: Res<crate::app::rematch::LocalTable>,
    settings: Res<crate::app::settings::GameSettings>,
    mut log: ResMut<crate::app::side_panels::EventLog>,
) {
    let Some(name) = gone.0.take() else {
        return;
    };
    if table.set_here() {
        let line = crate::app::i18n::fill(settings.tr().beach_gone, &[("name", &name)]);
        log.push(line, crate::app::palette::IDLE_ROW);
    }
}

impl CustomBeaches {
    /// Which beach on the shelf travels as `bytes`, the ones an online
    /// round was sent: the host's own, found again to put the cursor on it.
    pub fn shelf_of(&self, bytes: &[u8]) -> Option<usize> {
        (!bytes.is_empty())
            .then(|| self.0.iter().position(|beach| beach.wire == bytes))
            .flatten()
    }

    /// The ones that could host a match at this size: a versus arena wants
    /// a castle per seat, and a puzzle built for one crab has one.
    pub fn fitting(&self, seats: u8) -> Vec<&Beach> {
        self.0
            .iter()
            .filter(|beach| beach.level.fits(seats))
            .collect()
    }

    /// Where on the whole shelf the `custom`-th beach fitting `seats` sits.
    pub fn on_shelf(&self, seats: u8, custom: usize) -> Option<usize> {
        self.0
            .iter()
            .enumerate()
            .filter(|(_, beach)| beach.level.fits(seats))
            .nth(custom)
            .map(|(shelf, _)| shelf)
    }

    /// The other way round: which of the beaches fitting `seats` the one
    /// at `shelf` is, if it fits at all.
    pub fn among_fitting(&self, seats: u8, shelf: usize) -> Option<usize> {
        let beach = self.0.get(shelf)?;
        beach.level.fits(seats).then(|| {
            self.0[..shelf]
                .iter()
                .filter(|beach| beach.level.fits(seats))
                .count()
        })
    }
}

/// The aside for the map row when the shelf has beaches on it and a table
/// of `seats` is too big for every one of them.
///
/// The dial skipping an empty stop is right, a press that changes nothing
/// being a dead press, but unsaid it looks like a beach the game has lost:
/// two castles stop being offered the moment a third player sits down.
///
/// By seat count rather than off a [`MatchConfig`]: on the couch the table
/// is the config's seats, but online it is whoever turned up, and the
/// lobby read the couch's count against a table it had nothing to do
/// with.
pub fn beaches_note(
    seats: u8,
    tr: &crate::app::i18n::Tr,
    beaches: &CustomBeaches,
) -> Option<String> {
    if beaches.0.is_empty() || !beaches.fitting(seats).is_empty() {
        return None;
    }
    // A beach whose castles skip a number fits no table at all (see
    // `Level::fits`), so when that is every beach on the shelf, "none fits
    // four" sent the player looking for a smaller table that would not
    // have helped either.
    let all_skip = beaches
        .0
        .iter()
        .all(|beach| beach.level.table_seats().is_none());
    Some(match all_skip {
        true => tr.map_skips_castle.to_string(),
        false => fill(tr.map_none_seats, &[("n", &seats.to_string())]),
    })
}

/// Turn the map dial one step, in the dial's order ([`MapChoice::DIAL`]):
/// the built-in beaches, the player's own, then Surprise, as step 2's
/// sections run. `Custom` is one stop on `MapChoice`, so the dial stays
/// on it while there are more beaches to walk through, and steps off when
/// it runs out. With nothing saved it is skipped entirely: an empty stop
/// on a dial is a dead press.
pub fn cycle_map(config: &mut MatchConfig, turn: Turn, beaches: &CustomBeaches) {
    let beaches = beaches.fitting(config.seats).len();
    if config.map == MapChoice::Custom {
        let next = config.custom as i64 + i64::from(turn.signum());
        if (0..beaches as i64).contains(&next) {
            config.custom = next as usize;
            return;
        }
    }
    config.map = config.map.turned(turn);
    if config.map == MapChoice::Custom {
        if beaches == 0 {
            config.map = config.map.turned(turn);
        } else {
            // Entering the run from the far side lands on its far end.
            config.custom = match turn {
                Turn::Right => 0,
                Turn::Left => beaches - 1,
            };
        }
    }
}

/// What the map dial reads, which for a handmade beach is its own name.
///
/// `Custom` with no beach that seats the table (the shelf changed, or the
/// table grew, since it was chosen; [`settle_map`] steps off it where it
/// can) reads as the beach that will actually be played: both the local
/// launch and the wire build a generated 20x13 arena, so it names the XL
/// beach.
pub fn map_label(
    config: &MatchConfig,
    tr: &crate::app::i18n::Tr,
    beaches: &CustomBeaches,
) -> String {
    if config.map == MapChoice::Custom {
        return beaches
            .fitting(config.seats)
            .get(config.custom)
            .map_or_else(
                || map_name(tr, MapChoice::GenXl),
                |beach| {
                    // A beach too big to travel is marked on the dial. It
                    // plays here either way, so the label says what is lost
                    // rather than hiding the beach the author chose.
                    let template = match beach.too_big_to_send() {
                        true => tr.map_custom_local,
                        false => tr.map_custom,
                    };
                    fill(template, &[("n", &beach.level.name)])
                },
            );
    }
    map_name(tr, config.map)
}

/// What a built-in map is called on a dial, a terms card or a roster:
/// the name with its size. `Custom` with no beach in hand reads as the
/// generated 20x13 every peer builds in its place.
pub fn map_name(tr: &crate::app::i18n::Tr, map: MapChoice) -> String {
    match map {
        MapChoice::Classic
        | MapChoice::GenSmall
        | MapChoice::GenClassic
        | MapChoice::GenLarge
        | MapChoice::GenXl
        | MapChoice::GenOcean => tr.map_names[map.index()].to_string(),
        MapChoice::Custom => tr.map_names[MapChoice::GenXl.index()].to_string(),
        MapChoice::TheGates
        | MapChoice::TheMoat
        | MapChoice::LogJam
        | MapChoice::ThreeLanes
        | MapChoice::RoundTable
        | MapChoice::RoundTheWorld
        | MapChoice::Duel
        | MapChoice::TheFountain
        | MapChoice::FourDoors
        | MapChoice::GullRoad
        | MapChoice::BackToBack => {
            let (w, h) = map.size();
            format!("{} ({w}x{h})", map_title(tr, map))
        }
        MapChoice::Surprise => map_title(tr, map),
    }
}

/// A built-in map's name as its card on step 2 says it, without its size.
pub fn map_title(tr: &crate::app::i18n::Tr, map: MapChoice) -> String {
    match (map, map.handmade()) {
        (_, Some(handmade)) => tr.setup_versus_maps[handmade].to_string(),
        (MapChoice::Surprise, None) => tr.setup_surprise.to_string(),
        (map, None) => tr.setup_map_names[map.index()].to_string(),
    }
}

/// What a map is, for the line under step 2's cards. Surprise's line
/// takes a number, and is said by the card's own note; a beach of the
/// player's own is what a shelf card says of it. Every map has its line:
/// indexed by the dial's place, `Custom` read past the end of the built-in
/// lines.
pub fn map_about(tr: &crate::app::i18n::Tr, map: MapChoice) -> &'static str {
    match map {
        MapChoice::Classic
        | MapChoice::GenSmall
        | MapChoice::GenClassic
        | MapChoice::GenLarge
        | MapChoice::GenXl
        | MapChoice::GenOcean => tr.setup_map_about[map.index()],
        MapChoice::TheGates
        | MapChoice::TheMoat
        | MapChoice::LogJam
        | MapChoice::ThreeLanes
        | MapChoice::RoundTable
        | MapChoice::RoundTheWorld
        | MapChoice::Duel
        | MapChoice::TheFountain
        | MapChoice::FourDoors
        | MapChoice::GullRoad
        | MapChoice::BackToBack => tr.setup_versus_about[map.handmade().unwrap_or(0)],
        MapChoice::Surprise => tr.setup_surprise_sub,
        MapChoice::Custom => tr.setup_custom_about,
    }
}

/// The handmade beach a table of `seats` plays under `map`, out of the one
/// at hand (`level`), or `None` when the round is on a generated beach.
///
/// The one place that is decided, for the shelf and the wire alike: the
/// bytes a launch sends ([`beach_bytes`]), the name the lobby's roster
/// gives ([`beach_on_air`]), the board every peer builds
/// ([`board_from`]), the name a round's roster gives, and the terms
/// of the next round ([`fit_terms`]). Decided in several places
/// once, the roster named a handmade beach the table had outgrown while
/// everyone played the generated arena in its place.
///
/// A beach with fewer castles than seats is no use: `castle_of` answers
/// `None` for a seat with no castle, so that player banks nothing all
/// round, with every peer agreeing because every peer built it. Nor is one
/// whose castles skip an owner, which leaves a seat in the middle of the
/// table with nothing to bank in ([`crate::sim::Level::fits`]). One with
/// more castles than seats plays, the spare ones put away when the board
/// is built ([`crate::sim::Level::table_board`]).
pub fn beach_in_play<L: std::borrow::Borrow<crate::sim::Level>>(
    map: MapChoice,
    level: Option<L>,
    seats: u8,
) -> Option<L> {
    level.filter(|level| map == MapChoice::Custom && level.borrow().fits(seats))
}

/// The shelf beach the host's dials put on the air for a table of
/// `seats`: the one the map dial names, when that table plays it and it
/// fits an invitation.
pub fn beach_on_air<'a>(
    config: &MatchConfig,
    seats: u8,
    beaches: &'a CustomBeaches,
) -> Option<&'a Beach> {
    let beach = beaches.fitting(config.seats).get(config.custom).copied()?;
    beach_in_play(config.map, Some(&beach.level), seats)?;
    (!beach.too_big_to_send()).then_some(beach)
}

/// The beach the host is sending, compressed, or empty when the round is
/// played on one both peers can build from a seed.
///
/// `seats` is how many turned up, not how many the host had in mind when
/// it picked: a handmade beach with two castles cannot hold a table of
/// five, and the seats without one could never score. A beach that no
/// longer fits is dropped here, and the round falls back to the generated
/// arena the terms name.
pub fn beach_bytes(config: &MatchConfig, seats: u8, beaches: &CustomBeaches) -> Vec<u8> {
    if let Some(beach) = beaches.fitting(config.seats).get(config.custom)
        && config.map == MapChoice::Custom
        && beach.too_big_to_send()
    {
        // A beach that will not fit a datagram is dropped, where the
        // fallback is a generated arena everybody can build: sent anyway
        // it is truncated on arrival and refused, and the joiner waits out
        // an invitation that never decoded.
        warn!(
            "{:?} is too big to send ({} bytes packed, {} allowed): \
             the round falls back to a generated beach",
            beach.level.name,
            beach.wire.len(),
            crate::transport::MAX_BEACH_BYTES
        );
    }
    beach_on_air(config, seats, beaches).map_or_else(Vec::new, |beach| beach.wire.clone())
}

/// The beach those bytes describe, if they describe one.
pub fn beach_from(bytes: &[u8]) -> Option<crate::sim::Level> {
    let text = String::from_utf8(crate::lzw::decompress(bytes, 8)?).ok()?;
    crate::sim::Level::parse(&text).ok()
}
