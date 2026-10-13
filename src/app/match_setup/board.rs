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
    // Only a beach that seats the table, under terms that name it
    // ([`beach_in_play`]). The launch drops one that does not, and so does
    // the round after it, which used to re-send the beach while working
    // out `seats` afresh, the queue admitted between rounds making that
    // number bigger.
    //
    // Checked here as well because here is the one place both ends pass
    // through, with the same bytes and the same `seats`, so host and
    // joiner fall back to the same generated arena on the same tick.
    let Some(level) = beach_in_play(terms.map(), beach_from(beach), seats) else {
        return board_for(terms, seats);
    };
    let mut board = level.board();
    apply_terms(&mut board, terms.gulls(), terms.round(), terms.posts());
    board
}

pub fn board_for(terms: &MatchTerms, seats: u8) -> crate::sim::Board {
    let mut board = versus_board(terms.map(), terms.seed, seats);
    apply_terms(&mut board, terms.gulls(), terms.round(), terms.posts());
    board
}

/// The beach a map dial names, at `seed` for a table of `seats`: the
/// handcrafted classic arena or a generated one at the dial's size, with
/// its edges open when the map has none. Every versus round that is not on
/// a handmade beach starts here, online or on the couch, so the open ocean
/// wraps on both: the couch used to build its own and left the walls up.
///
/// `Custom` builds the generated arena at its size, the fallback for a
/// handmade beach that cannot be had.
pub fn versus_board(map: MapChoice, seed: u64, seats: u8) -> crate::sim::Board {
    let (w, h) = map.size();
    let mut board = if map == MapChoice::Classic {
        crate::sim::classic_arena_seeded(seed, false, seats)
    } else {
        crate::sim::generate_arena(seed, seats, w, h)
    };
    board.set_wrap(map.wraps());
    board
}

/// The table's rules laid over a versus beach: the gull pressure scaled to
/// its size ([`crate::sim::set_versus_rules`]), the round's length, and the
/// arrows each player may have standing, the oldest giving way at the cap.
pub fn apply_terms(
    board: &mut crate::sim::Board,
    gulls: GullPressure,
    round: RoundLength,
    posts: u8,
) {
    crate::sim::set_versus_rules(board, gulls.period());
    board.set_round_length(Some(round.ticks()));
    board.set_signpost_rule(posts, crate::sim::CapPolicy::Evict);
}

/// Which seats the AI holds under these terms: the top `bots` of `seats`.
pub fn bot_seats_from(terms: &MatchTerms, seats: u8) -> [Option<BotLevel>; MAX_PLAYERS] {
    let level = terms.bot_level();
    let mut out = [None; MAX_PLAYERS];
    for seat in seats.saturating_sub(terms.bots)..seats {
        if let Some(slot) = out.get_mut(usize::from(seat)) {
            *slot = Some(level);
        }
    }
    out
}
