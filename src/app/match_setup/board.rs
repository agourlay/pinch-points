//! Boards built from wire terms: what every peer does with a Start.
//!
//! Host and joiner alike rebuild the round's board from the same
//! MatchTerms (and beach bytes, when the map dial says custom), so these
//! live together and a guard in tests/it proves board_from with no beach
//! is board_for exactly.

use super::*;

/// The board a set of terms describes, with the host's handmade beach if
/// one came with them. Falls back to the terms alone when the bytes are
/// missing or unreadable: a round on the wrong beach is a desync, but a
/// round that never starts is worse, and the hash check will say so.
pub fn board_from(terms: &MatchTerms, seats: u8, beach: &[u8]) -> crate::sim::Board {
    let Some(level) = beach_from(beach) else {
        return board_for(terms, seats);
    };
    // A beach that cannot seat the table is no more use than none at all:
    // `castle_of` answers `None` for a seat with no castle, so that player
    // banks nothing for the whole round, quietly, with every peer agreeing
    // because every peer built the same too-small beach.
    //
    // The launch already drops such a beach (`beach_bytes`), but the round
    // after it does not: `call_next_round` re-sends the beach it played on
    // while working out `seats` afresh, and the queue admitted between
    // rounds makes that number bigger. Four players on a four-castle beach,
    // two more admitted, and seats 4 and 5 spend the round unable to score.
    //
    // Checked here because here is the one place both ends pass through,
    // with the same bytes and the same `seats`: host and joiner fall back
    // to the same generated arena on the same tick, so nobody desyncs over
    // it.
    if level.seats() < seats {
        return board_for(terms, seats);
    }
    let mut board = level.board();
    board.set_gull_period(GullPressure::from_index(usize::from(terms.gulls)).period());
    board.set_round_length(Some(
        RoundLength::from_index(usize::from(terms.round)).ticks(),
    ));
    board
}

pub fn board_for(terms: &MatchTerms, seats: u8) -> crate::sim::Board {
    let map = MapChoice::from_index(usize::from(terms.map));
    let (w, h) = map.size();
    let mut board = if map == MapChoice::Classic {
        crate::sim::classic_arena_seeded(terms.seed, false, seats)
    } else {
        crate::sim::generate_arena(terms.seed, seats, w, h)
    };
    board.set_wrap(map.wraps());
    board.set_gull_period(GullPressure::from_index(usize::from(terms.gulls)).period());
    board.set_round_length(Some(
        RoundLength::from_index(usize::from(terms.round)).ticks(),
    ));
    board
}

/// Which seats the AI holds under these terms: the top `bots` of `seats`.
pub fn bot_seats_from(terms: &MatchTerms, seats: u8) -> [Option<BotLevel>; MAX_PLAYERS] {
    let level = BotLevel::from_index(usize::from(terms.bot_level));
    let mut out = [None; MAX_PLAYERS];
    for seat in seats.saturating_sub(terms.bots)..seats {
        if let Some(slot) = out.get_mut(usize::from(seat)) {
            *slot = Some(level);
        }
    }
    out
}
