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
    // On the round's seed, as a generated beach is, with the castles of
    // seats nobody sits in put away.
    let mut board = level.table_board(terms.seed, seats);
    apply_terms(&mut board, terms.gulls(), terms.round(), terms.posts());
    board
}

pub fn board_for(terms: &MatchTerms, seats: u8) -> crate::sim::Board {
    let mut board = versus_board(terms.map(), terms.seed, seats);
    apply_terms(&mut board, terms.gulls(), terms.round(), terms.posts());
    board
}

/// The board a match set up here is played on, at `seed`: the handmade
/// beach picked off the shelf, or the beach the map names, with the
/// table's rules laid over it. The one function a local round is built by
/// and step 2's map cards are drawn from, so the preview of a card is the
/// beach its match is played on.
///
/// The last word on a table too big for its beach, whatever let it get
/// there: the four-castle arenas clamp a table of five to four castles,
/// and the fifth player plays the whole match with nothing to defend.
/// [`reseat`] keeps the pick from reading such a map; this keeps the
/// launch from building one if anything else ever does.
pub fn local_board(config: &MatchConfig, beaches: &CustomBeaches, seed: u64) -> crate::sim::Board {
    let map = match holds(config.map, config.seats) {
        true => config.map,
        false => sized_for(config.seats),
    };
    // A beach somebody built. Locally there is nobody to send it to, so
    // it is read off the shelf, and built on this round's seed. Anything
    // else is the map's own beach: the handcrafted classic gets a fresh
    // random stream too, so one seed's luck (gull entry points, spawn
    // kinds) does not colour every round.
    let shelved = (map == MapChoice::Custom)
        .then(|| beaches.fitting(config.seats).into_iter().nth(config.custom))
        .flatten();
    let mut board = match shelved {
        Some(beach) => beach.level.table_board(seed, config.seats),
        None => versus_board(map, seed, config.seats),
    };
    apply_terms(&mut board, config.gulls, config.round, config.posts);
    board
}

/// The beach a map dial names, at `seed` for a table of `seats`: the
/// handcrafted classic arena, a handmade versus map, a generated beach at
/// the dial's size (with its edges open when the map has none), or under
/// Surprise one generated in the style the seed draws. Every versus round
/// that is not on a beach off the shelf starts here, online or on the
/// couch, so every peer builds the same one: the open ocean used to wrap
/// online only, the couch building its own and leaving the walls up.
///
/// Then the round's turn ([`crate::sim::Orientation::for_round`]): the
/// beach mirrored left-right and/or top-bottom from the seed, and an open
/// beach of strips rolled round by whole strips. Built here, it is the
/// board every peer plays, the replay records and step 2's card shows.
///
/// A handmade map deals its castles to the seats from the seed in opposite
/// pairs ([`crate::sim::deal_pairs`]), so who plays which castle moves
/// round to round while pairs still face each other for the team modes.
/// At a table it is not built for (nothing offers it one, but the wire is
/// a byte a peer chose) it gives way to the generated beach sized for the
/// table, as `Custom` falls back to the generated 20x13.
pub fn versus_board(map: MapChoice, seed: u64, seats: u8) -> crate::sim::Board {
    let generated = |map: MapChoice| {
        let (w, h) = map.size();
        crate::sim::generate_beach(seed, seats, w, h, map.wraps())
    };
    let (board, strip) = match map {
        MapChoice::Classic => (crate::sim::classic_arena_seeded(seed, false, seats), 0),
        MapChoice::GenSmall
        | MapChoice::GenClassic
        | MapChoice::GenLarge
        | MapChoice::GenXl
        | MapChoice::GenOcean
        | MapChoice::Custom => (generated(map), 0),
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
            let level = crate::sim::versus_map(map.handmade().unwrap_or(0));
            match level.fits(seats) {
                true => {
                    let mut board = level.table_board(seed, seats);
                    crate::sim::deal_pairs(&mut board, seats, seed);
                    (board, level.roll)
                }
                false => (generated(sized_for(seats)), 0),
            }
        }
        MapChoice::Surprise => {
            let (w, h) = crate::sim::surprise_size(seats);
            let style = crate::sim::surprise_style(seed, seats);
            (
                crate::sim::generate_styled(seed, seats, w, h, false, style),
                0,
            )
        }
    };
    crate::sim::Orientation::for_round(seed, board.width(), strip).apply(&board)
}

/// The table's rules laid over a versus beach: the gull pressure scaled to
/// its size ([`crate::sim::set_versus_rules`]), the round's length, the
/// arrows each player may have standing, the oldest giving way at the cap,
/// and the tide-event wheel turning. Every versus round is built through
/// here, couch and online, a beach off the shelf and the editor's
/// playtest alike, so every one of them plays the wheel: a handmade beach
/// file never says `events: on`, and its rounds used to play without it.
pub fn apply_terms(
    board: &mut crate::sim::Board,
    gulls: GullPressure,
    round: RoundLength,
    posts: u8,
) {
    crate::sim::set_versus_rules(board, gulls.period());
    board.set_events_enabled(true);
    board.rules_mut().round_length = Some(round.ticks());
    board.set_signpost_rule(posts, crate::sim::CapPolicy::Evict);
}

/// Which seats the AI holds under these terms: the last `bots` places of
/// `seats`, in the seats the order puts them ([`MatchTerms::ai_seats`]).
pub fn bot_seats_from(terms: &MatchTerms, seats: u8) -> [Option<BotLevel>; MAX_PLAYERS] {
    let level = terms.bot_level();
    let mut out = [None; MAX_PLAYERS];
    for seat in terms.ai_seats(seats) {
        if let Some(slot) = out.get_mut(usize::from(seat)) {
            *slot = Some(level);
        }
    }
    out
}
