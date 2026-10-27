//! Versus maps: today's beaches built exactly as before, every style and
//! handmade map playable at every table it seats, and the per-round
//! transform the same on every peer and in every replay.

use pinch_points::sim::{
    BeachStyle, Board, Direction, Level, TileKind, VERSUS_MAPS, classic_arena_seeded,
    generate_beach, generate_styled, surprise_size, unplayable, versus_map,
};

/// FNV-1a over a byte string, folded into a running digest.
fn fold(mut digest: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        digest ^= u64::from(b);
        digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
    }
    digest
}

/// The level text of every beach today's generator and the classic arena
/// build over a spread of seeds, tables, sizes and both edge rules, folded
/// into one number. Text rather than the state hash: the text names every
/// tile, wall, creature and rule, so a change anywhere in what the
/// generator lays down shows.
fn todays_beaches_digest() -> u64 {
    let mut digest = 0xcbf2_9ce4_8422_2325;
    for seed in 0..120u64 {
        let seed = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ seed;
        for seats in 2..=6u8 {
            for (w, h) in [(9u8, 7u8), (12, 9), (16, 11), (20, 13)] {
                for wraps in [false, true] {
                    let board = generate_beach(seed, seats, w, h, wraps);
                    let text = Level::from_board("beach", 3, board).to_text();
                    digest = fold(digest, text.as_bytes());
                }
            }
            let text =
                Level::from_board("classic", 3, classic_arena_seeded(seed, false, seats)).to_text();
            digest = fold(digest, text.as_bytes());
        }
    }
    digest
}

/// The generator's own output (before any per-round transform) is byte
/// for byte what it was before the styles were added: the digest was
/// taken at the commit before them, so every seed a player, a replay or a
/// daily has ever named still builds the beach it built.
#[test]
fn todays_generated_beaches_are_unchanged() {
    assert_eq!(
        todays_beaches_digest(),
        TODAYS_DIGEST,
        "{:#018x}",
        todays_beaches_digest()
    );
}

/// Taken at a18e244, the commit before the styles.
const TODAYS_DIGEST: u64 = 0xb7e1_f7ec_d93e_9527;

/// Whether `board` has any terrain at all: a style that fell back to the
/// bare frame has none.
fn has_terrain(board: &Board) -> bool {
    let (w, h) = (board.width(), board.height());
    board.tiles().any(|(x, y, kind)| {
        matches!(
            kind,
            TileKind::Rock | TileKind::Kelp | TileKind::Pool | TileKind::Turnstile { .. }
        ) || (x + 1 < w && board.wall_at(x, y, Direction::Right))
            || (y + 1 < h && board.wall_at(x, y, Direction::Down))
    })
}

/// Whether the terrain (castle owners aside) is its own image under both
/// mirrors, which is what makes the four corners one job. A log standing on
/// a mirror axis is its own image and is let be.
fn mirror_symmetric(board: &Board, both_ways: bool) -> bool {
    let (w, h) = (board.width(), board.height());
    let plain = |kind: TileKind| match kind {
        TileKind::Castle(_) => TileKind::Empty,
        other @ (TileKind::Empty
        | TileKind::Rock
        | TileKind::Spawner(_)
        | TileKind::Turnstile { .. }
        | TileKind::Kelp
        | TileKind::Pool) => other,
    };
    let flips: &[(bool, bool)] = match both_ways {
        true => &[(true, false), (false, true)],
        false => &[(true, true)],
    };
    let turned = |dir: Direction, fx: bool, fy: bool| {
        let horizontal = matches!(dir, Direction::Left | Direction::Right);
        match (fx && horizontal) || (fy && !horizontal) {
            true => dir.reverse(),
            false => dir,
        }
    };
    let image = |kind: TileKind, fx: bool, fy: bool| match kind {
        TileKind::Spawner(mut s) => {
            s.dir = turned(s.dir, fx, fy);
            TileKind::Spawner(s)
        }
        TileKind::Turnstile { next_right } => TileKind::Turnstile {
            next_right: next_right ^ fx ^ fy,
        },
        other @ (TileKind::Empty
        | TileKind::Rock
        | TileKind::Castle(_)
        | TileKind::Kelp
        | TileKind::Pool) => other,
    };
    flips.iter().all(|&(fx, fy)| {
        board.tiles().all(|(x, y, kind)| {
            let (ix, iy) = (
                if fx { w - 1 - x } else { x },
                if fy { h - 1 - y } else { y },
            );
            let same_tile = plain(image(kind, fx, fy)) == plain(board.tile_at(ix, iy))
                || (matches!(kind, TileKind::Turnstile { .. }) && (ix, iy) == (x, y));
            let same_walls = Direction::ALL
                .into_iter()
                .all(|dir| board.wall_at(x, y, dir) == board.wall_at(ix, iy, turned(dir, fx, fy)));
            same_tile && same_walls
        })
    })
}

/// Every style lays a playable beach over a range of seeds at every table:
/// every castle reachable from every hole, no sand cut off, no castle boxed
/// in, the terrain mirror-symmetric both ways, and never the bare frame
/// for want of a layout that passed.
#[test]
fn every_style_is_playable_over_many_seeds() {
    for style in BeachStyle::ALL {
        for seats in 2..=6u8 {
            let (w, h) = surprise_size(seats);
            for seed in 0..60u64 {
                let board = generate_styled(seed, seats, w, h, false, style);
                let problems = unplayable(&board);
                let at = format!("{} at {seats} seats, seed {seed}", style.token());
                assert!(problems.is_empty(), "{at}: {problems:?}");
                assert!(has_terrain(&board), "{at} fell back to the bare frame");
                assert!(mirror_symmetric(&board, true), "{at} is lopsided");
                assert_eq!(board.castle_seats(), seats, "{at}");
            }
        }
    }
}

/// A style changes the terrain and nothing else: the castles, holes and
/// rules are the classic beach's at the same seed.
#[test]
fn a_style_keeps_the_frame() {
    for seed in [3u64, 77, 1234] {
        for seats in [2u8, 4, 6] {
            let (w, h) = surprise_size(seats);
            let classic = generate_beach(seed, seats, w, h, false);
            for style in BeachStyle::ALL {
                let styled = generate_styled(seed, seats, w, h, false, style);
                let frame = |board: &Board| {
                    board
                        .tiles()
                        .filter(|(_, _, k)| matches!(k, TileKind::Castle(_) | TileKind::Spawner(_)))
                        .collect::<Vec<_>>()
                };
                assert_eq!(frame(&styled), frame(&classic), "{style:?}");
                assert_eq!(styled.rules(), classic.rules(), "{style:?}");
            }
        }
    }
}

/// Whether an open beach is made of identical strips `roll` columns wide:
/// rolled round by one strip, every tile lands on its like (a castle on a
/// castle, whoever's) and every wall on a wall. Such a beach has no
/// castle better placed than another, which is what lets it seat an odd
/// table: the bots' figures cannot tell its castles apart, and neither
/// can a player.
fn strips_are_copies(board: &Board, roll: u8) -> bool {
    let w = board.width();
    let kind = |kind: TileKind| match kind {
        TileKind::Castle(_) => (6, ' '),
        TileKind::Spawner(s) => (2, s.dir.letter()),
        other @ (TileKind::Empty
        | TileKind::Rock
        | TileKind::Turnstile { .. }
        | TileKind::Kelp
        | TileKind::Pool) => (what_is_at(other).0, ' '),
    };
    board.wrap()
        && roll > 0
        && w.is_multiple_of(roll)
        && board.tiles().all(|(x, y, here)| {
            let rx = ((u16::from(x) + u16::from(roll)) % u16::from(w)) as u8;
            kind(here) == kind(board.tile_at(rx, y))
                && Direction::ALL
                    .into_iter()
                    .all(|dir| board.wall_at(x, y, dir) == board.wall_at(rx, y, dir))
        })
}

/// Whether the seam of an open beach takes no gull: the first and last
/// columns are rock or kelp from the second row to the second last, so
/// the edge spawner, which lands its gulls on the rim, lands them only on
/// the top and bottom rows, the same in every strip. A gull put down on
/// the seam's sand would favour whichever strip the round's roll set
/// beside it.
fn seam_takes_no_gull(board: &Board) -> bool {
    let (w, h) = (board.width(), board.height());
    (1..h - 1).all(|y| {
        [0, w - 1]
            .into_iter()
            .all(|x| matches!(board.tile_at(x, y), TileKind::Rock | TileKind::Kelp))
    })
}

/// Whether every odd table `level` names is seated fairly by its shape:
/// an open beach of identical strips with a seam no gull lands on, one
/// castle a strip and none left empty, so every castle is a copy of
/// every other. On a beach with edges a castle is put away at an odd
/// table, and the one across from the gap won 45-58% at three seats.
fn odd_tables_are_fair(level: &Level) -> bool {
    let full = level.board();
    level
        .tables
        .iter()
        .filter(|&&seats| seats % 2 == 1)
        .all(|&seats| {
            strips_are_copies(&full, level.roll)
                && seam_takes_no_gull(&full)
                && full.castle_seats() == seats
                && full.width() / level.roll == seats
        })
}

/// Every handmade map reads, is an arena, names its tables, and is
/// playable at each: mirror-symmetric both ways (Duel and the odd tables'
/// beaches a half turn), every castle reachable and open, opposite castles
/// numbered in pairs. An open beach is identical strips with a seam no
/// gull lands on, and only such a beach, one castle a strip, seats an odd
/// table.
#[test]
fn every_handmade_map_is_playable_at_every_table_it_seats() {
    assert_eq!(VERSUS_MAPS.len(), 11);
    for index in 0..VERSUS_MAPS.len() {
        let level = versus_map(index);
        let name = &level.name;
        assert_eq!(level.kind, pinch_points::sim::LevelKind::Arena, "{name}");
        assert!(!level.tables.is_empty(), "{name} names its tables");
        let full = level.board();
        if full.wrap() {
            assert!(strips_are_copies(&full, level.roll), "{name}: strips");
            assert!(seam_takes_no_gull(&full), "{name}: the seam");
        }
        assert!(odd_tables_are_fair(&level), "{name}: its odd tables");
        for &seats in &level.tables {
            assert!(level.fits(seats), "{name} at {seats}");
            let board = level.table_board(9, seats);
            let problems = unplayable(&board);
            assert!(problems.is_empty(), "{name} at {seats}: {problems:?}");
            assert_eq!(board.seats_in_play(), seats, "{name} at {seats}");
            // A half turn is all Duel needs, a two-castle beach, and all a
            // beach of identical strips for an odd table needs: its strips
            // are what makes the castles one job.
            let half_turn = level.tables == [2] || level.tables.iter().all(|seats| seats % 2 == 1);
            assert!(mirror_symmetric(&board, !half_turn), "{name} at {seats}");
            // Castles 2k and 2k+1 stand across the middle from each other.
            // An open beach has no middle: every castle of Round the World
            // is a copy of every other, and its pairs are a top and a
            // bottom castle in neighbouring strips.
            let (w, h) = (board.width(), board.height());
            for pair in (0..seats / 2).filter(|_| !board.wrap()) {
                let a = board.castle_of(pair * 2).expect("a castle");
                let b = board.castle_of(pair * 2 + 1).expect("a castle");
                assert_eq!((w - 1 - a.0, h - 1 - a.1), b, "{name}: pair {pair}");
            }
        }
        for seats in 2..=6u8 {
            assert_eq!(
                level.fits(seats),
                level.tables.contains(&seats),
                "{name} at {seats}"
            );
        }
    }
}

/// The two guards above catch what they are for: a strip edited on its
/// own, a gull able to land on the seam, and an odd table on a beach with
/// edges, each refused.
#[test]
fn a_lopsided_open_beach_is_caught() {
    let gull_road = versus_map(9);
    let mut board = gull_road.board();
    assert!(strips_are_copies(&board, gull_road.roll));
    board.set_tile(2, 4, TileKind::Rock);
    assert!(
        !strips_are_copies(&board, gull_road.roll),
        "one strip edited"
    );
    let mut board = gull_road.board();
    board.set_tile(0, 5, TileKind::Empty);
    assert!(!seam_takes_no_gull(&board), "sand on the seam");
    let edged = Level::parse(
        "name: B\nposts: 3\nkind: arena\nseats: 3\nmap:\n\
         +-+-+-+\n|0 . 1|\n+ + + +\n|. 2 .|\n+-+-+-+\n",
    )
    .expect("reads");
    assert!(
        !odd_tables_are_fair(&edged),
        "a beach with edges seating three"
    );
    let four_doors = VERSUS_MAPS[8];
    let row = "K . . . . . K K . . . . . K";
    assert!(four_doors.contains(row));
    let lopsided =
        Level::parse(&four_doors.replacen(row, "K . . . . . K K . # . . . K", 1)).expect("reads");
    assert!(
        !odd_tables_are_fair(&lopsided),
        "Four Doors with one strip edited"
    );
    assert!(odd_tables_are_fair(&versus_map(8)));
}

/// The two keys a handmade map adds to the level format read back as
/// written, and refuse what would make no sense: a table bigger than the
/// castles, a list out of order, a roll on a beach with edges or one that
/// does not divide it.
#[test]
fn the_seats_and_roll_keys_read_and_refuse() {
    let level = versus_map(5);
    assert_eq!(level.tables, [2, 4, 6]);
    assert_eq!(level.roll, 7);
    let again = Level::parse(&level.to_text()).expect("reads back");
    assert_eq!((again.tables.clone(), again.roll), (vec![2, 4, 6], 7));
    assert_eq!(again.to_text(), level.to_text());

    let beach = |extra: &str, wrap: bool| {
        let wrap = if wrap { "wrap: on\n" } else { "" };
        Level::parse(&format!(
            "name: B\nposts: 3\nkind: arena\n{wrap}{extra}map:\n\
             +-+-+-+-+\n|0 . . 1|\n+ + + + +\n|2 . . 3|\n+-+-+-+-+\n"
        ))
    };
    assert!(beach("seats: 2 4\n", false).is_ok());
    assert!(beach("seats: 2 4 6\n", false).unwrap_err().contains("6"));
    assert!(beach("seats: 4 2\n", false).is_err(), "out of order");
    assert!(beach("seats: 1\n", false).is_err());
    assert!(beach("seats:\n", false).is_err());
    assert!(
        beach("roll: 2\n", false)
            .unwrap_err()
            .contains("open beach")
    );
    assert!(beach("roll: 2\n", true).is_ok());
    assert!(beach("roll: 3\n", true).is_err(), "3 does not divide 4");
    let two_four = beach("seats: 2 4\n", false).expect("reads");
    assert!(two_four.fits(2) && !two_four.fits(3) && two_four.fits(4));
}

/// The beach `map` draws at `seed` for a table of `seats`, before the
/// round's turn, with the width of the strips an open beach rolls round
/// by (0 for one that does not roll): what `versus_board` turns.
pub(crate) fn drawn(
    map: pinch_points::app::match_setup::MapChoice,
    seed: u64,
    seats: u8,
) -> (Board, u8) {
    use pinch_points::app::match_setup::MapChoice;
    use pinch_points::sim::{deal_pairs, surprise_style};
    match map {
        MapChoice::Classic => (classic_arena_seeded(seed, false, seats), 0),
        MapChoice::GenSmall
        | MapChoice::GenClassic
        | MapChoice::GenLarge
        | MapChoice::GenXl
        | MapChoice::GenOcean
        | MapChoice::Custom => {
            let (w, h) = map.size();
            (generate_beach(seed, seats, w, h, map.wraps()), 0)
        }
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
            let level = versus_map(map.handmade().expect("a handmade map"));
            assert!(level.fits(seats), "{map:?} seats {seats}");
            let mut board = level.table_board(seed, seats);
            deal_pairs(&mut board, seats, seed);
            (board, level.roll)
        }
        MapChoice::Surprise => {
            let (w, h) = surprise_size(seats);
            let style = surprise_style(seed, seats);
            (generate_styled(seed, seats, w, h, false, style), 0)
        }
    }
}

/// What a tile is, for telling a turned beach from the one drawn: its
/// kind, and a castle's owner. A facing is left out, being mirrored with
/// the beach.
fn what_is_at(kind: TileKind) -> (u8, u8) {
    match kind {
        TileKind::Empty => (0, 0),
        TileKind::Rock => (1, 0),
        TileKind::Spawner(_) => (2, 0),
        TileKind::Turnstile { .. } => (3, 0),
        TileKind::Kelp => (4, 0),
        TileKind::Pool => (5, 0),
        TileKind::Castle(owner) => (6, owner),
    }
}

/// `turned` is `drawn` under `turn`: every tile of the beach drawn found
/// at its image, worked out here (rolled round, then mirrored) rather
/// than by `Orientation::apply`, so a board left as drawn fails it.
pub(crate) fn assert_turned(
    drawn: &Board,
    turned: &Board,
    turn: pinch_points::sim::Orientation,
    what: &str,
) {
    let (w, h) = (drawn.width(), drawn.height());
    assert_eq!((turned.width(), turned.height()), (w, h), "{what}");
    let at: std::collections::HashMap<(u8, u8), TileKind> =
        turned.tiles().map(|(x, y, kind)| ((x, y), kind)).collect();
    for (x, y, kind) in drawn.tiles() {
        let rolled = ((u16::from(x) + u16::from(turn.roll)) % u16::from(w)) as u8;
        let ix = if turn.flip_x { w - 1 - rolled } else { rolled };
        let iy = if turn.flip_y { h - 1 - y } else { y };
        assert_eq!(
            what_is_at(at[&(ix, iy)]),
            what_is_at(kind),
            "{what}: ({x},{y}) belongs at ({ix},{iy}) under {turn:?}"
        );
    }
}

/// Every versus map is turned per round from its seed: built twice from
/// one seed it is the same board (every peer builds it so), it is the
/// beach the map draws turned the way the seed says, tile by tile (the
/// classic arena, every generated size and the open ocean, every handmade
/// map with its castles dealt in pairs, and Surprise in the style the
/// seed draws), and over a run of seeds every mirror comes up with seat
/// 0's castle moving round with them.
#[test]
fn every_versus_round_is_turned_from_its_seed() {
    use pinch_points::app::match_setup::{MapChoice, holds, versus_board};
    use pinch_points::sim::Orientation;
    for map in MapChoice::ALL {
        if map == MapChoice::Custom {
            continue;
        }
        for seats in (2..=6u8).filter(|&seats| holds(map, seats)) {
            let mut turns = std::collections::BTreeSet::new();
            let mut homes = std::collections::BTreeSet::new();
            let mut turned_some = false;
            for seed in 0..40u64 {
                let board = versus_board(map, seed, seats);
                assert_eq!(
                    board.state_hash(),
                    versus_board(map, seed, seats).state_hash(),
                    "{map:?} at {seats}, seed {seed}"
                );
                let (beach, strip) = drawn(map, seed, seats);
                let turn = Orientation::for_round(seed, beach.width(), strip);
                assert_turned(
                    &beach,
                    &board,
                    turn,
                    &format!("{map:?} at {seats}, seed {seed}"),
                );
                turned_some |= !turn.is_identity() && board.state_hash() != beach.state_hash();
                turns.insert((turn.flip_x, turn.flip_y));
                homes.insert(board.castle_of(0));
            }
            assert!(turned_some, "{map:?} at {seats}: never turned");
            assert_eq!(turns.len(), 4, "{map:?} at {seats}: every mirror");
            assert!(homes.len() >= 2, "{map:?} at {seats}: seat 0 never moved");
        }
    }
}

/// The classic arena's opening crabs are turned with it: each walks the
/// mirrored way on the mirrored claw, so a mirrored beach is the drawn one
/// seen in a mirror, not one with its crabs walking into the walls.
#[test]
fn the_classic_arenas_crabs_turn_with_it() {
    use pinch_points::app::match_setup::{MapChoice, versus_board};
    use pinch_points::sim::{Handedness, Orientation};
    let seed = (0u64..)
        .find(|&seed| {
            let turn = Orientation::for_round(seed, 12, 0);
            turn.flip_x && !turn.flip_y
        })
        .expect("a left-right seed");
    let drawn = classic_arena_seeded(seed, false, 4);
    let played = versus_board(MapChoice::Classic, seed, 4);
    for (crab, image) in drawn.crabs().iter().zip(played.crabs()) {
        let (x, y) = drawn.coords_u8(crab.tile);
        assert_eq!(played.coords_u8(image.tile), (11 - x, y));
        assert_eq!(image.dir, crab.dir.reverse());
        assert_ne!(image.handed, crab.handed, "a mirror swaps the claw");
        assert!(matches!(image.handed, Handedness::Left | Handedness::Right));
    }
    assert_eq!(
        played.castle_of(0),
        Some((10, 1)),
        "seat 0's corner, mirrored"
    );
}

/// A mirrored round replays exactly: the replay records the board as it
/// was played, turned, and played back from its text (and reloaded from
/// a file) it ends where the round ended. Over the classic arena with its
/// crabs mirrored, a handmade map, Round the World and Four Doors rolled
/// round, and a Surprise beach.
#[test]
fn a_mirrored_round_replays_exactly() {
    use pinch_points::app::match_setup::{GullPressure, RoundLength};
    use pinch_points::app::match_setup::{MapChoice, apply_terms, versus_board};
    use pinch_points::sim::{BotLevel, MAX_PLAYERS, Orientation, PlayerAction, Replay, bot_action};
    for (map, seats) in [
        (MapChoice::Classic, 4u8),
        (MapChoice::LogJam, 4),
        (MapChoice::RoundTheWorld, 6),
        (MapChoice::FourDoors, 3),
        (MapChoice::Surprise, 3),
    ] {
        let seed = (1u64..)
            .find(|&seed| {
                let board = versus_board(map, seed, seats);
                let turn = Orientation::for_round(seed, board.width(), 7);
                turn.flip_x != turn.flip_y && (!board.wrap() || turn.roll > 0)
            })
            .expect("a turning seed");
        let mut board = versus_board(map, seed, seats);
        apply_terms(&mut board, GullPressure::Frenzy, RoundLength::Quick, 3);
        board.rules_mut().round_length = Some(900);
        let mut replay = Replay::new(Level::from_board("Mirrored", 3, board.clone()));
        while !board.round_over() {
            let mut actions = [PlayerAction::None; MAX_PLAYERS];
            for seat in 0..seats {
                actions[usize::from(seat)] = bot_action(&board, seat, BotLevel::Hard);
            }
            board.tick(&actions);
            replay.record(actions);
        }
        let played = board.state_hash();
        assert_eq!(replay.playback().state_hash(), played, "{map:?}");
        let reloaded = Replay::parse(&replay.to_text()).expect("reloads");
        assert_eq!(
            reloaded.playback().state_hash(),
            played,
            "{map:?} from its file"
        );
    }
}

/// Surprise draws a style a round from the round's seed: every style it
/// deals comes up over a run of seeds, Lanes only at four or fewer, the
/// size the one for the table, and the round after (a series' or a
/// rematch's, on a new seed) is Surprise again, locally and on the wire.
#[test]
fn surprise_draws_a_fresh_beach_every_round() {
    use pinch_points::app::match_setup::{
        CustomBeaches, MapChoice, MatchConfig, map_for_next_round, next_round_terms, versus_board,
    };
    use pinch_points::sim::{SURPRISE_LANES_MOST, surprise_style, surprise_styles};
    use pinch_points::transport::MatchTerms;
    for seats in 2..=6u8 {
        let drawn: std::collections::BTreeSet<&str> = (0..200u64)
            .map(|seed| surprise_style(seed, seats).token())
            .collect();
        let dealt: std::collections::BTreeSet<&str> = surprise_styles(seats)
            .into_iter()
            .map(BeachStyle::token)
            .collect();
        assert_eq!(drawn, dealt, "{seats} seats");
        assert_eq!(drawn.contains("lanes"), seats <= SURPRISE_LANES_MOST);
        assert!(!drawn.contains("classic"));
        let (w, h) = surprise_size(seats);
        let board = versus_board(MapChoice::Surprise, 7, seats);
        assert_eq!(board.height(), h);
        assert!(
            board.width() == w || board.width() == w + 1,
            "{seats} seats"
        );
        assert_eq!(board.castle_seats(), seats);
    }
    // The next round is Surprise again, on its own seed.
    let mut config = MatchConfig {
        map: MapChoice::Surprise,
        seats: 4,
        ..MatchConfig::default()
    };
    map_for_next_round(&mut config, &CustomBeaches::default(), 3);
    assert_eq!(config.map, MapChoice::Surprise, "a local series or rematch");
    let surprise = MapChoice::ALL
        .iter()
        .position(|&map| map == MapChoice::Surprise)
        .expect("on the wire") as u8;
    let terms = MatchTerms {
        map: surprise,
        seed: 1,
        ..MatchTerms::default()
    };
    let next = next_round_terms(terms, 5, 2);
    assert_eq!((next.map, next.seed), (surprise, 2), "an online series");
    // Two seeds, two beaches: a round's draw is its own.
    let styles: std::collections::BTreeSet<_> = (0..12u64)
        .map(|seed| surprise_style(seed, 4).token())
        .collect();
    assert!(styles.len() >= 3, "{styles:?}");
}

/// Each handmade map is offered to the tables its file names, and the
/// dial's sizes and seat lists are the files' own.
#[test]
fn the_dial_knows_each_handmade_map_as_its_file_does() {
    use pinch_points::app::match_setup::{MapChoice, holds};
    for map in MapChoice::ALL {
        let Some(handmade) = map.handmade() else {
            assert!(map.tables().is_none(), "{map:?}");
            continue;
        };
        let level = versus_map(handmade);
        let board = level.board();
        assert_eq!(map.size(), (board.width(), board.height()), "{map:?}");
        assert_eq!(map.tables(), Some(level.tables.as_slice()), "{map:?}");
        assert_eq!(map.wraps(), board.wrap(), "{map:?}");
        for seats in 2..=6u8 {
            assert_eq!(holds(map, seats), level.fits(seats), "{map:?} at {seats}");
        }
    }
}
