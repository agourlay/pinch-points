//! The shelf of handmade beaches, and how one goes over the wire.
//!
//! The setup screen only turns the dial; this is the half the online path
//! leans on: reading the player's levels off disk, the LZW packing that
//! has to fit a datagram, and the note that says why a beach is not
//! offered. Nothing here spawns UI.

use super::*;

/// The board those terms describe, built through the same path a local match
/// uses so online and offline cannot drift apart.
/// The handmade beaches on this machine, as of the last time a screen that
/// offers them opened.
///
/// A resource and not a function call, because the two places that read it
/// are drawn every frame: `map_label` used to go to the disk and parse
/// every level file sixty times a second, which is a strange thing for a
/// menu to do.
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
    pub(super) fn new(level: crate::sim::Level) -> Beach {
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

/// Re-read the shelf. Runs on entering the screens that offer a beach, so
/// one saved in the editor a moment ago is on the dial.
pub fn refresh_custom_beaches(mut beaches: ResMut<CustomBeaches>) {
    beaches.0 = crate::app::campaign::custom_arenas(crate::app::campaign::load_custom_levels())
        .into_iter()
        .map(Beach::new)
        .collect();
}

impl CustomBeaches {
    /// The ones that could host a match at this size: a versus arena wants
    /// a castle per seat, and a puzzle built for one crab has one.
    pub fn fitting(&self, seats: u8) -> Vec<&Beach> {
        self.0
            .iter()
            .filter(|beach| beach.level.seats() >= seats)
            .collect()
    }
}

/// The aside for the map row when the shelf has beaches on it and this
/// table is too big for every one of them.
///
/// The dial skipping an empty stop is right - a press that changes nothing
/// is a dead press - but it left the reason unsaid. A beach with two
/// castles simply stopped being offered the moment a third player joined
/// the table, which from the other side of the screen looks like a beach
/// the game has lost.
pub fn beaches_note(
    config: &MatchConfig,
    tr: &crate::app::i18n::Tr,
    beaches: &CustomBeaches,
) -> Option<String> {
    let all_too_small = !beaches.0.is_empty() && beaches.fitting(config.seats).is_empty();
    all_too_small.then(|| fill(tr.map_none_seats, &[("n", &config.seats.to_string())]))
}

/// Turn the map dial one step, walking the built-in beaches and then the
/// handmade ones. `Custom` is one stop on `MapChoice`, so the dial stays
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
    config.map = config.map.cycled(turn);
    if config.map == MapChoice::Custom {
        if beaches == 0 {
            config.map = config.map.cycled(turn);
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
/// launch and the wire build a generated 20x13 arena for it, which is the
/// XL beach, so that is the name shown. It used to say "Classic".
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
                || tr.map_names[MapChoice::GenXl.index()].to_string(),
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
    tr.map_names[config.map.index()].to_string()
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
    if config.map != MapChoice::Custom {
        return Vec::new();
    }
    beaches
        .fitting(config.seats)
        .get(config.custom)
        .filter(|beach| beach.level.seats() >= seats)
        .filter(|beach| {
            // A beach that will not fit a datagram is dropped here, where
            // the fallback is a generated arena everybody can build. Sent
            // anyway it would be truncated on arrival and refused, and the
            // joiner would wait out an invitation that never decoded.
            let sendable = !beach.too_big_to_send();
            if !sendable {
                warn!(
                    "{:?} is too big to send ({} bytes packed, {} allowed): \
                     the round falls back to a generated beach",
                    beach.level.name,
                    beach.wire.len(),
                    crate::transport::MAX_BEACH_BYTES
                );
            }
            sendable
        })
        .map(|beach| beach.wire.clone())
        .unwrap_or_default()
}

/// The beach those bytes describe, if they describe one.
pub fn beach_from(bytes: &[u8]) -> Option<crate::sim::Level> {
    let text = String::from_utf8(crate::lzw::decompress(bytes, 8)?).ok()?;
    crate::sim::Level::parse(&text).ok()
}
