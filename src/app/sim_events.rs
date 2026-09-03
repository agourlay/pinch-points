//! The single sim observer: diffs the headless simulation once per frame
//! and broadcasts everything that happened as [`SimEvent`] messages. Audio,
//! effects, and any future consumer read the same stream instead of each
//! re-deriving events from board state.

use crate::app::{Sim, layout};
use crate::sim::{
    Crab, CrabKind, Direction, GullState, MAX_PLAYERS, PlayerId, TideEvent, TileKind, castle_tier,
};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

/// Something observable happened in the sim since the last frame.
#[derive(Message, Clone, Debug)]
pub enum SimEvent {
    /// A crab walked into a castle. `value` is its score worth.
    ///
    /// `pos` is where the crab was last drawn and `keep` the middle of the
    /// wall it stepped through. Both, because the render layer walks it
    /// from one to the other, and the keep cannot be worked out later:
    /// these events are read a frame after they are observed (see the
    /// `Frame` sets), and a `CastleSwap` landing in that gap would send
    /// the crab to whichever castle its owner holds *now* - typically on
    /// the far side of the beach, into a keep it never touched.
    ///
    /// `id` is the crab's, so it can be drawn in the shell it walked in
    /// with rather than its kind's flat colour.
    CrabBanked {
        id: u32,
        owner: PlayerId,
        pos: Vec2,
        keep: Vec2,
        value: u32,
        kind: CrabKind,
    },
    /// The gulls got one.
    CrabEaten { pos: Vec2 },
    /// A crab appeared from a spawner hole.
    CrabSpawned { pos: Vec2 },
    /// A gull raided `owner`'s castle for `lost` banked crabs.
    CastleRaided {
        owner: PlayerId,
        pos: Vec2,
        lost: u32,
    },
    /// A new gull walked onto the beach.
    GullArrived,
    /// A walking gull took to the air.
    GullTookOff,
    /// A flying gull touched down here.
    GullLanded { pos: Vec2 },
    /// A signpost went up here, and whose it is.
    ///
    /// One per tile, never a headcount. A board-wide count is the wrong
    /// thing to listen to: one seat placing while another pulls nets to
    /// zero, and the beach falls silent for both of them.
    ///
    /// The seat used to stop at the differ, on the grounds that a
    /// placement sounds and looks the same whoever made it. It does not:
    /// on a six-seat beach the bots place far more often than the people
    /// do, and a knock for every one of them is a click track running
    /// under the whole match. It carries the owner now so a listener can
    /// ask whether the post was one of *its* player's.
    SignpostPlaced { owner: PlayerId, pos: Vec2 },
    /// A signpost left this tile and took its owner's count down with it:
    /// they pulled it, or it wore out. [`SimEvent::SignpostEvicted`] is the
    /// one departure that does not.
    ///
    /// Carries the owner for the same reason [`SimEvent::SignpostPlaced`]
    /// does, and it is the half that matters most: in versus every post
    /// wears out on its own, so a bot's post whose placement was kept
    /// quiet still knocked a few seconds later when it went. Silencing one
    /// end and not the other leaves the click track running at very nearly
    /// its old rate.
    SignpostRemoved { owner: PlayerId, pos: Vec2 },
    /// A signpost was pushed off the board to make room for a newer one:
    /// `owner` was at the cap under [`crate::sim::CapPolicy::Evict`] and
    /// placed anyway, so their oldest went. `pos` is where it stood.
    ///
    /// The versus and Beach Day rule, never the campaign's, which refuses
    /// the placement instead. It is the one way a player loses a signpost
    /// by their own hand without asking to, and until this event existed
    /// it happened in total silence.
    SignpostEvicted {
        owner: PlayerId,
        pos: Vec2,
        dir: Direction,
    },
    /// `owner`'s castle rose a tier.
    TierUp { owner: PlayerId },
    /// The tide roulette fired this event.
    TideEventFired { event: TideEvent },
    /// The final-scramble surge began.
    SurgeStarted,
    /// The round timer expired.
    RoundEnded,
}

/// What a board looks like before its clock has run: the terrain and the
/// creatures it started with. Two boards both at tick 0 are told apart by
/// this rather than by the clock, which reads the same on both.
///
/// Signposts and scores are deliberately left out. A puzzle's Setup phase
/// never ticks, and placing a signpost there must still read as a change
/// to *this* board (it plays the placement sound), not as a swap.
#[derive(PartialEq, Debug)]
struct Origin {
    width: u8,
    height: u8,
    seed: u64,
    tiles: Vec<TileKind>,
    crabs: Vec<(u32, u16, Direction, CrabKind)>,
    gulls: Vec<(u32, u16)>,
}

impl Origin {
    /// Only boards at tick 0 need an identity; a ticked board's clock
    /// already tells it apart from whatever comes next.
    fn of(board: &crate::sim::Board) -> Option<Origin> {
        (board.ticks() == 0).then(|| Origin {
            width: board.width(),
            height: board.height(),
            seed: board.seed(),
            tiles: board.tiles().map(|(_, _, kind)| kind).collect(),
            crabs: board
                .crabs()
                .iter()
                .map(|c| (c.id, c.tile, c.dir, c.kind))
                .collect(),
            gulls: board.gulls().iter().map(|g| (g.id, g.tile)).collect(),
        })
    }
}

/// Last-seen sim facts for the diff.
#[derive(Default)]
pub struct Watch {
    ticks: u64,
    /// The beach itself, by size and seed: what tells a round picked back
    /// up from a pasted code, which arrives mid-clock, from the board this
    /// was last read from. The clock alone cannot, when the pasted round
    /// is further along than the board before it was.
    beach: (u8, u8, u64),
    origin: Option<Origin>,
    scores: [u32; MAX_PLAYERS],
    tiers: [u8; MAX_PLAYERS],
    /// Per seat, never a board-wide sum: an eviction leaves its owner's
    /// count where it was, and a sum cannot even see whose post moved.
    posts_by: [usize; MAX_PLAYERS],
    /// Which seat holds the signpost on each occupied tile, and which way
    /// it points. Only the seat decides whether a tile lost its post -
    /// re-pointing one in place keeps the seat and changes the direction,
    /// and that is not an eviction - but the direction has to be kept to
    /// draw the ghost of a post that is already off the board.
    posts_at: HashMap<(u8, u8), (PlayerId, Direction)>,
    gulls: usize,
    flying: usize,
    event_at: Option<u64>,
    last_event: Option<TideEvent>,
    surging: bool,
    over: bool,
    crabs: HashMap<u32, Crab>,
    /// Where each gull was in the air, `None` while it was on the sand: a
    /// gull that had a flight position and now has none has just landed.
    gulls_aloft: HashMap<u32, Option<Vec2>>,
}

impl Watch {
    /// Read every fact the differ compares, in one place. Built whole and
    /// assigned whole, so a new field cannot be compared-but-never-stored.
    fn of(board: &crate::sim::Board) -> Watch {
        let mut tiers = [0u8; MAX_PLAYERS];
        for (tier, &score) in tiers.iter_mut().zip(board.scores().iter()) {
            *tier = castle_tier(score);
        }
        Watch {
            ticks: board.ticks(),
            beach: (board.width(), board.height(), board.seed()),
            origin: Origin::of(board),
            scores: *board.scores(),
            tiers,
            posts_by: std::array::from_fn(|p| board.signpost_count(p as PlayerId)),
            posts_at: board
                .tiles()
                .filter_map(|(x, y, _)| {
                    let sp = board.signpost_at(x, y)?;
                    Some(((x, y), (sp.owner, sp.dir)))
                })
                .collect(),
            gulls: board.gulls().len(),
            flying: board
                .gulls()
                .iter()
                .filter(|g| matches!(g.state, GullState::Flying { .. }))
                .count(),
            event_at: board.last_event().map(|(_, at)| at),
            last_event: board.last_event().map(|(event, _)| event),
            surging: board.in_surge(),
            over: board.round_over(),
            crabs: board.crabs().iter().map(|c| (c.id, *c)).collect(),
            gulls_aloft: board
                .gulls()
                .iter()
                .map(|g| {
                    let aloft = matches!(g.state, GullState::Flying { .. })
                        .then(|| layout::creature_pos(board, g.tile, g.dir, g.progress));
                    (g.id, aloft)
                })
                .collect(),
        }
    }
}

/// A crab left the board: it either reached a castle or a gull got it.
///
/// The sim does not say which, since a crab simply stops existing, so this
/// reads the tile it was on and the one it was walking into. Either being a
/// castle means it banked there; anything else means it was eaten. Getting
/// this wrong plays the wrong sound and the wrong particle burst on the
/// most-noticed moment in the game, so it has tests of its own.
///
/// The exception is the one departure the tiles cannot explain. `Monopoly`
/// banks half the loose crabs where they stand, so they go from open sand
/// with no castle at either end of the step, and reading the tiles alone
/// called every one of them a death: eighty-two of them at once, measured,
/// answered with eighty-two death sounds, eight hundred feathers and a
/// screen shake, while the seat that had just taken half the beach got no
/// hop, no bounce and no floating score. The sim writes those down as it
/// makes them ([`Board::swept_home`](crate::sim::Board::swept_home)), and
/// they are asked after first.
///
/// Both tile reads are bounds-checked rather than trusting the crab's
/// coordinates: `prev` was recorded on last frame's board, and should that
/// board have been larger than this one (a swap the differ failed to
/// notice) an asserting `tile_at` would take the game down at level start.
fn crab_departure(board: &crate::sim::Board, prev: &Crab) -> SimEvent {
    let pos = layout::creature_pos(board, prev.tile, prev.dir, prev.progress);
    let (x, y) = board.coords_u8(prev.tile);
    let (dx, dy) = prev.dir.offset();
    let tile_or_empty = |x: i32, y: i32| {
        if x >= 0 && y >= 0 && x < i32::from(board.width()) && y < i32::from(board.height()) {
            board.tile_at(x as u8, y as u8)
        } else {
            TileKind::Empty
        }
    };
    // The tide's own doing, which no tile records. Its keep is where the
    // crab is flown to, and a banker with no castle to fly it to is not a
    // thing the sim makes, so the tiles have the last word if it happens.
    if let Some(owner) = board.swept_home(prev.id)
        && let Some((kx, ky)) = board.castle_of(owner)
    {
        return SimEvent::CrabBanked {
            id: prev.id,
            owner,
            pos,
            keep: layout::tile_center(board, kx, ky),
            value: prev.kind.value(),
            kind: prev.kind,
        };
    }
    let here = tile_or_empty(i32::from(x), i32::from(y));
    let ahead = (i32::from(x) + dx, i32::from(y) + dy);
    let entering = tile_or_empty(ahead.0, ahead.1);
    // Which of the two tiles is the castle is also *where* it is, and this
    // is the only moment anybody knows: the board it was read from is this
    // frame's, and the event is consumed on the next.
    let keep_at = |at: (i32, i32)| layout::tile_center(board, at.0 as u8, at.1 as u8);
    match (here, entering) {
        (TileKind::Castle(owner), _) => SimEvent::CrabBanked {
            id: prev.id,
            owner,
            pos,
            keep: keep_at((i32::from(x), i32::from(y))),
            value: prev.kind.value(),
            kind: prev.kind,
        },
        (_, TileKind::Castle(owner)) => SimEvent::CrabBanked {
            id: prev.id,
            owner,
            pos,
            keep: keep_at(ahead),
            value: prev.kind.value(),
            kind: prev.kind,
        },
        _ => SimEvent::CrabEaten { pos },
    }
}

/// Crabs that left the board since the last frame, and crabs that arrived
/// out of a spawner hole.
fn crab_events(board: &crate::sim::Board, watch: &Watch, events: &mut Vec<SimEvent>) {
    for (id, prev) in watch.crabs.iter() {
        if board.crabs().iter().any(|crab| crab.id == *id) {
            continue;
        }
        events.push(crab_departure(board, prev));
    }
    for crab in board.crabs() {
        if watch.crabs.contains_key(&crab.id) {
            continue;
        }
        let (x, y) = board.coords_u8(crab.tile);
        if matches!(board.tile_at(x, y), TileKind::Spawner(_)) {
            events.push(SimEvent::CrabSpawned {
                pos: layout::creature_pos(board, crab.tile, crab.dir, crab.progress),
            });
        }
    }
}

/// Diff the sim against the last frame and emit one message per happening.
/// A board swap (fresh level or round, detected by the tick clock rolling
/// back or by an un-ticked board's identity changing) resyncs silently so
/// loading never fires a burst of stale events.
pub fn observe_sim(sim: Res<Sim>, mut watch: Local<Watch>, mut events: MessageWriter<SimEvent>) {
    // An untouched sim is the board the watch was read from, so the diff
    // would find nothing. Reading one costs every tile, both creature
    // maps, and on an unticked board a copy of the terrain; a puzzle in
    // Setup and the editor never tick at all.
    if !sim.is_changed() {
        return;
    }
    for event in diff(&sim.0, &mut watch) {
        events.write(event);
    }
}

/// The pure diff-and-resync core of [`observe_sim`], separated so the
/// bank-vs-eaten classification and the reset silence are testable
/// without a Bevy runtime. Mutates `watch` to the board's current facts
/// and returns everything that happened since the last call.
pub fn diff(board: &crate::sim::Board, watch: &mut Watch) -> Vec<SimEvent> {
    let next = Watch::of(board);
    // A board swap usually rolls the tick clock back; everything the old
    // board was doing is not news about the new one. Between two boards
    // that have never ticked (a puzzle's Setup phase, skipping levels from
    // there, an editor resize) the clock reads 0 on both sides, so those
    // are told apart by their origin instead: same clock, different board.
    let swapped = board.ticks() < watch.ticks
        || next.beach != watch.beach
        || (board.ticks() == 0 && next.origin != watch.origin);
    let events = if swapped {
        Vec::new()
    } else {
        changes(board, watch, &next)
    };
    *watch = next;
    events
}

/// What changed between two readings of the same board.
fn changes(board: &crate::sim::Board, prev: &Watch, next: &Watch) -> Vec<SimEvent> {
    let mut events = Vec::new();
    crab_events(board, prev, &mut events);

    // Raids: any score drop, located at that player's castle.
    for (seat, (&now, &before)) in next.scores.iter().zip(prev.scores.iter()).enumerate() {
        if now >= before {
            continue;
        }
        if let Some((x, y)) = board.castle_of(seat as PlayerId) {
            events.push(SimEvent::CastleRaided {
                owner: seat as PlayerId,
                pos: layout::tile_center(board, x, y),
                lost: before - now,
            });
        }
    }

    // Gull population and flight transitions.
    if next.gulls > prev.gulls {
        events.push(SimEvent::GullArrived);
    }
    if next.flying > prev.flying {
        events.push(SimEvent::GullTookOff);
    }
    for gull in board.gulls() {
        let now_flying = matches!(gull.state, GullState::Flying { .. });
        if let Some(Some(_)) = prev.gulls_aloft.get(&gull.id)
            && !now_flying
        {
            events.push(SimEvent::GullLanded {
                pos: layout::creature_pos(board, gull.tile, gull.dir, gull.progress),
            });
        }
    }

    // Signposts tile by tile, in both directions. A tile whose seat is the
    // same on both sides has not changed hands: re-pointing a post in place
    // keeps the seat and swings the direction, and that is neither a
    // placement nor a departure.
    for (&(x, y), &(owner, _)) in &next.posts_at {
        if prev.posts_at.get(&(x, y)).is_some_and(|&(o, _)| o == owner) {
            continue;
        }
        events.push(SimEvent::SignpostPlaced {
            owner,
            pos: layout::tile_center(board, x, y),
        });
    }
    // An eviction is a signpost leaving a tile while its owner's count
    // holds: they were at the cap and placed a fourth, so the board took
    // their oldest in trade. Every other way a signpost goes - expiry, the
    // player pulling it, a gull finishing it off - takes the count down
    // with it, which is what tells the two apart. Placing on a frame where
    // one of yours also expired reads as an eviction; the cue is "one of
    // yours just went, here", which is true either way.
    for (&(x, y), &(owner, dir)) in &prev.posts_at {
        if next.posts_at.get(&(x, y)).is_some_and(|&(o, _)| o == owner) {
            continue;
        }
        let pos = layout::tile_center(board, x, y);
        events.push(
            if next.posts_by[owner as usize] < prev.posts_by[owner as usize] {
                SimEvent::SignpostRemoved { owner, pos }
            } else {
                SimEvent::SignpostEvicted { owner, pos, dir }
            },
        );
    }
    for (seat, (&now, &before)) in next.tiers.iter().zip(prev.tiers.iter()).enumerate() {
        if now > before {
            events.push(SimEvent::TierUp {
                owner: seat as PlayerId,
            });
        }
    }
    if next.event_at.is_some()
        && next.event_at != prev.event_at
        && let Some(event) = next.last_event
    {
        events.push(SimEvent::TideEventFired { event });
    }
    if next.surging && !prev.surging {
        events.push(SimEvent::SurgeStarted);
    }
    if next.over && !prev.over {
        events.push(SimEvent::RoundEnded);
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Board, CrabKind, Direction, Handedness};

    fn synced(board: &Board) -> Watch {
        let mut watch = Watch::default();
        diff(board, &mut watch);
        watch
    }

    /// A crab that disappears while entering a castle banked; one that
    /// disappears anywhere else was eaten.
    #[test]
    fn bank_and_eaten_classification() {
        let mut board = Board::new(6, 4, 7);
        board.set_tile(3, 1, crate::sim::TileKind::Castle(2));
        board.spawn_crab(2, 1, Direction::Right, Handedness::Left, CrabKind::Giant);
        let mut watch = synced(&board);
        let mut banked = None;
        for _ in 0..600 {
            board.tick_idle();
            let events = diff(&board, &mut watch);
            if let Some(SimEvent::CrabBanked { owner, value, .. }) = events
                .iter()
                .find(|e| matches!(e, SimEvent::CrabBanked { .. }))
            {
                banked = Some((*owner, *value));
                break;
            }
            assert!(
                !events
                    .iter()
                    .any(|e| matches!(e, SimEvent::CrabEaten { .. })),
                "a banking crab must never read as eaten"
            );
        }
        assert_eq!(
            banked,
            Some((2, 10)),
            "giant banks for its owner at value 10"
        );

        // Eaten: a gull dropped on the crab's tile removes it mid-sand.
        let mut board = Board::new(6, 4, 7);
        board.spawn_crab(1, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        let mut watch = synced(&board);
        board.spawn_gull(1, 2, Direction::Right);
        let mut eaten = false;
        for _ in 0..60 {
            board.tick_idle();
            let events = diff(&board, &mut watch);
            assert!(
                !events
                    .iter()
                    .any(|e| matches!(e, SimEvent::CrabBanked { .. })),
                "an eaten crab must never read as banked"
            );
            if events
                .iter()
                .any(|e| matches!(e, SimEvent::CrabEaten { .. }))
            {
                eaten = true;
                break;
            }
        }
        assert!(eaten, "the gull never registered its meal");
    }

    /// A board swap (tick clock rolling back) resyncs without emitting a
    /// burst of stale events.
    #[test]
    fn board_swap_is_silent() {
        let mut board = Board::new(6, 4, 7);
        board.spawn_crab(1, 1, Direction::Right, Handedness::Left, CrabKind::Common);
        let mut watch = synced(&board);
        for _ in 0..40 {
            board.tick_idle();
            diff(&board, &mut watch);
        }
        // A fresh, different board: everything changed, nothing fires.
        let fresh = Board::new(8, 6, 99);
        assert!(
            diff(&fresh, &mut watch).is_empty(),
            "a board swap must not fire stale events"
        );
    }

    /// Losing your oldest signpost to the cap fires; every other way one
    /// leaves the board does not.
    ///
    /// The versus rule takes a post in trade for the fourth you place, and
    /// it used to do so in silence - the count on the header does not even
    /// move, since one went for one. Pulling a post yourself, or letting
    /// one expire, is a count going *down*, and neither is news.
    #[test]
    fn a_signpost_traded_away_at_the_cap_says_so() {
        let mut board = Board::new(6, 4, 5);
        board.set_signpost_rule(3, crate::sim::CapPolicy::Evict);
        for x in 0..3u8 {
            assert!(board.place_signpost(0, x, 0, Direction::Up));
        }
        let mut watch = synced(&board);

        // The fourth: the oldest, at (0,0), is traded for it.
        assert!(board.place_signpost(0, 5, 3, Direction::Down));
        let events = diff(&board, &mut watch);
        let evicted: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                SimEvent::SignpostEvicted { owner, dir, .. } => Some((*owner, *dir)),
                SimEvent::CrabBanked { .. }
                | SimEvent::CrabEaten { .. }
                | SimEvent::CrabSpawned { .. }
                | SimEvent::CastleRaided { .. }
                | SimEvent::GullArrived
                | SimEvent::GullTookOff
                | SimEvent::GullLanded { .. }
                | SimEvent::SignpostPlaced { .. }
                | SimEvent::SignpostRemoved { .. }
                | SimEvent::TierUp { .. }
                | SimEvent::TideEventFired { .. }
                | SimEvent::SurgeStarted
                | SimEvent::RoundEnded => None,
            })
            .collect();
        assert_eq!(
            evicted,
            vec![(0, Direction::Up)],
            "the traded post and the way it pointed: {events:?}"
        );
        // And the placement that paid for it sounds too, from the tile it
        // landed on. The trade used to reach the player as the eviction
        // alone, because the board-wide count did not move - so a fourth
        // post went down in versus and nothing said it had.
        assert!(
            events.iter().any(|e| matches!(
                e,
                SimEvent::SignpostPlaced { pos, .. } if *pos == layout::tile_center(&board, 5, 3)
            )),
            "the fourth post went down at (5,3): {events:?}"
        );

        // Pulling one yourself is a removal, not a trade.
        assert!(board.remove_signpost(0, 1, 0));
        let events = diff(&board, &mut watch);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SimEvent::SignpostEvicted { .. })),
            "removing your own post is not an eviction: {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                SimEvent::SignpostRemoved { pos, .. } if *pos == layout::tile_center(&board, 1, 0)
            )),
            "but it is a removal, at its tile: {events:?}"
        );

        // Re-pointing one in place keeps the tile and the seat, so it is
        // none of the three: not a trade, and not a post coming or going.
        assert!(board.place_signpost(0, 2, 0, Direction::Left));
        let events = diff(&board, &mut watch);
        assert!(
            !events.iter().any(|e| matches!(
                e,
                SimEvent::SignpostEvicted { .. }
                    | SimEvent::SignpostPlaced { .. }
                    | SimEvent::SignpostRemoved { .. }
            )),
            "re-pointing is not a post changing hands: {events:?}"
        );
    }

    /// Two seats acting between one frame and the next each get their own
    /// cue, from their own tile.
    ///
    /// These events were once a single board-wide count, and a count is
    /// deaf to who moved: a placement and a removal in the same frame
    /// cancelled to a net zero and *neither* player heard anything. On a
    /// busy versus beach that is the common case, not the corner one.
    #[test]
    fn one_seat_placing_while_another_pulls_sounds_for_both() {
        let mut board = Board::new(6, 4, 11);
        assert!(board.place_signpost(1, 4, 2, Direction::Left));
        let mut watch = synced(&board);

        // The same frame: seat 1 pulls theirs, seat 0 puts one down.
        assert!(board.remove_signpost(1, 4, 2));
        assert!(board.place_signpost(0, 1, 1, Direction::Up));
        let events = diff(&board, &mut watch);

        assert!(
            events.iter().any(|e| matches!(
                e,
                SimEvent::SignpostPlaced { pos, .. } if *pos == layout::tile_center(&board, 1, 1)
            )),
            "seat 0's placement, at its tile: {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                SimEvent::SignpostRemoved { pos, .. } if *pos == layout::tile_center(&board, 4, 2)
            )),
            "seat 1's removal, at its tile: {events:?}"
        );
    }

    /// Two boards that have never ticked share a clock reading of 0, so a
    /// swap between them (skipping puzzle levels from the Setup phase, an
    /// editor resize) must be recognised by identity: no stale crab
    /// departures, and no out-of-bounds probe when the new board is the
    /// smaller one.
    #[test]
    fn swap_between_unticked_boards_is_silent() {
        let mut large = Board::new(9, 7, 3);
        large.spawn_crab(8, 6, Direction::Right, Handedness::Left, CrabKind::Common);
        large.spawn_crab(1, 1, Direction::Right, Handedness::Left, CrabKind::Giant);
        let mut watch = synced(&large);
        assert_eq!(watch.ticks, 0);

        // Smaller board, same seed, a different crab: the old crabs would
        // land off-board (the corner one) or read as eaten (the other).
        let mut small = Board::new(6, 4, 3);
        small.spawn_crab(2, 2, Direction::Left, Handedness::Right, CrabKind::Common);
        assert!(
            diff(&small, &mut watch).is_empty(),
            "a swap between un-ticked boards must be silent"
        );

        // Same size, same seed, differently populated: still a swap.
        let mut other = Board::new(6, 4, 3);
        other.spawn_crab(4, 1, Direction::Up, Handedness::Left, CrabKind::Common);
        assert!(
            diff(&other, &mut watch).is_empty(),
            "a same-sized un-ticked board with other crabs is a swap"
        );

        // But a change on the *same* un-ticked board is not a swap: a
        // signpost placed during Setup still announces itself.
        assert!(other.place_signpost(0, 1, 1, Direction::Down));
        let events = diff(&other, &mut watch);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SimEvent::SignpostPlaced { .. })),
            "setup-phase signposts must still fire, got {events:?}"
        );
    }

    /// The departure the tiles cannot explain. `Monopoly` banks half the
    /// loose crabs where they stand, so each goes from open sand with no
    /// castle at either end of its step - which read as a death, every
    /// time. Eighty-two of them on one measured tick: eighty-two death
    /// sounds, eight hundred feathers and a screen shake, while the seat
    /// that had just taken half the beach got no hop, no castle bounce and
    /// no floating score.
    #[test]
    fn a_crab_the_tide_banked_reads_as_banked_and_not_as_eaten() {
        let mut board = Board::new(9, 7, 5);
        board.set_tile(7, 5, crate::sim::TileKind::Castle(1));
        // Out in the open, walking nowhere near a castle: the shape the
        // tiles get wrong.
        board.spawn_crab(1, 1, Direction::Right, Handedness::Left, CrabKind::Common);
        board.spawn_crab(2, 1, Direction::Right, Handedness::Left, CrabKind::Common);
        let prev = board.crabs()[0];
        assert!(
            matches!(crab_departure(&board, &prev), SimEvent::CrabEaten { .. }),
            "off open sand, and nothing yet says otherwise"
        );

        board.force_tide_event(crate::sim::TideEvent::Monopoly, 1);
        let SimEvent::CrabBanked {
            id, owner, keep, ..
        } = crab_departure(&board, &prev)
        else {
            panic!("the tide put it in seat 1's keep");
        };
        assert_eq!(id, prev.id);
        assert_eq!(owner, 1, "and the seat that got it is the banker");
        assert_eq!(
            keep,
            layout::tile_center(&board, 7, 5),
            "flown to the keep it was banked into, not to the sand it left"
        );
    }

    /// Through the whole differ, which is what the game actually runs: the
    /// tick a sweep lands on reports banks and not a single death.
    #[test]
    fn a_sweep_puts_no_deaths_on_the_stream() {
        let mut board = Board::new(9, 7, 11);
        board.set_tile(7, 5, crate::sim::TileKind::Castle(0));
        for x in 1..5u8 {
            board.spawn_crab(x, 1, Direction::Right, Handedness::Left, CrabKind::Common);
        }
        let mut watch = Watch::default();
        let _ = diff(&board, &mut watch);
        board.force_tide_event(crate::sim::TideEvent::Monopoly, 0);
        board.tick_idle();
        let events = diff(&board, &mut watch);
        let banked = events
            .iter()
            .filter(|e| matches!(e, SimEvent::CrabBanked { owner: 0, .. }))
            .count();
        let eaten = events
            .iter()
            .filter(|e| matches!(e, SimEvent::CrabEaten { .. }))
            .count();
        assert_eq!(banked, 2, "half of four, every one of them a bank");
        assert_eq!(eaten, 0, "and the gulls got nothing: {events:?}");
    }

    /// A crab remembered at a tile the current board does not have (a
    /// swap the differ was not told about) must not probe off-board.
    #[test]
    fn crab_departure_survives_a_smaller_board() {
        let mut large = Board::new(9, 7, 3);
        large.spawn_crab(8, 6, Direction::Right, Handedness::Left, CrabKind::Common);
        let prev = large.crabs()[0];
        let small = Board::new(6, 4, 3);
        assert!(matches!(
            crab_departure(&small, &prev),
            SimEvent::CrabEaten { .. }
        ));
    }

    /// The app-layer harness pattern: drive the real system inside a
    /// headless Bevy App (no plugins beyond the schedule) and read the
    /// messages it wrote, proving the ECS wiring end to end.
    #[test]
    fn observe_sim_runs_headless_in_an_app() {
        let mut app = App::new();
        app.add_message::<SimEvent>();
        let mut board = Board::new(6, 4, 7);
        board.set_tile(3, 1, crate::sim::TileKind::Castle(1));
        board.spawn_crab(2, 1, Direction::Right, Handedness::Left, CrabKind::Common);
        app.insert_resource(crate::app::Sim(board));
        app.add_systems(Update, observe_sim);
        app.update(); // syncs the watch to the starting board

        let mut banked = false;
        for _ in 0..600 {
            app.world_mut()
                .resource_mut::<crate::app::Sim>()
                .0
                .tick_idle();
            app.update();
            let mut messages = app.world_mut().resource_mut::<Messages<SimEvent>>();
            if messages
                .drain()
                .any(|e| matches!(e, SimEvent::CrabBanked { owner: 1, .. }))
            {
                banked = true;
                break;
            }
        }
        assert!(banked, "the system never reported the bank through the App");
    }

    /// Frames where nobody touches the sim are skipped, and skipping them
    /// leaves the watch fit to report the next move: the board it holds is
    /// the board that is still there.
    #[test]
    fn an_untouched_sim_is_not_re_read() {
        let mut app = App::new();
        app.add_message::<SimEvent>();
        let mut board = Board::new(6, 4, 7);
        board.set_tile(3, 1, crate::sim::TileKind::Castle(1));
        board.spawn_crab(2, 1, Direction::Right, Handedness::Left, CrabKind::Common);
        app.insert_resource(crate::app::Sim(board));
        app.add_systems(Update, observe_sim);
        app.update(); // syncs the watch to the starting board

        for _ in 0..3 {
            app.update();
            let mut messages = app.world_mut().resource_mut::<Messages<SimEvent>>();
            assert_eq!(
                messages.drain().count(),
                0,
                "a still board reported something happening"
            );
        }

        let mut banked = false;
        for _ in 0..600 {
            app.world_mut()
                .resource_mut::<crate::app::Sim>()
                .0
                .tick_idle();
            app.update();
            let mut messages = app.world_mut().resource_mut::<Messages<SimEvent>>();
            if messages
                .drain()
                .any(|e| matches!(e, SimEvent::CrabBanked { owner: 1, .. }))
            {
                banked = true;
                break;
            }
        }
        assert!(banked, "the bank went unreported after the idle frames");
    }

    /// Tier-ups name the seat whose castle grew.
    #[test]
    fn tier_up_names_its_owner() {
        let mut board = Board::new(6, 4, 7);
        let mut watch = synced(&board);
        board.set_score(3, 10); // tier 0 -> 1
        board.tick_idle();
        let events = diff(&board, &mut watch);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SimEvent::TierUp { owner: 3 })),
            "expected TierUp for seat 3, got {events:?}"
        );
    }
}
