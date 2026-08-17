//! Text-format contracts: every shipped level and every replay must
//! round-trip through their serializers without drifting. These are the
//! tests that guard the token tables against a mismatched edit.

use pinch_points::sim::{
    Direction, Level, MAX_PLAYERS, PlayerAction, Replay, campaign_levels, challenge_levels,
    classic_arena,
};

/// parse(to_text(level)) must reach a fixed point (identical text and an
/// identical starting board) for the whole shipped corpus.
/// The format has to carry as many castles as the sim seats. It did not:
/// the writer emitted '4' and '5' for a six-seat board and the reader
/// rejected them, so every six-player replay was a parse error waiting to
/// happen: replays are stored as levels.
#[test]
fn a_board_with_every_seat_survives_the_format() {
    use pinch_points::sim::{MAX_PLAYERS, TileKind, generate_arena};

    let board = generate_arena(7, MAX_PLAYERS as u8, 20, 13);
    let castles = |b: &pinch_points::sim::Board| {
        let mut owners: Vec<u8> = b
            .tiles()
            .filter_map(|(_, _, kind)| match kind {
                TileKind::Castle(owner) => Some(owner),
                TileKind::Empty
                | TileKind::Rock
                | TileKind::Spawner(_)
                | TileKind::Turnstile { .. }
                | TileKind::Kelp
                | TileKind::Pool => None,
            })
            .collect();
        owners.sort_unstable();
        owners
    };
    let before = castles(&board);
    assert_eq!(before.len(), MAX_PLAYERS, "the arena seats everyone");

    let text = Level::from_board("Full house", 3, board).to_text();
    let parsed = Level::parse(&text)
        .expect("a full board must parse")
        .board();
    assert_eq!(castles(&parsed), before, "every castle came back");
}

#[test]
fn every_shipped_level_round_trips() {
    let mut corpus = campaign_levels();
    corpus.extend(challenge_levels());
    assert!(corpus.len() >= 33, "corpus went missing?");
    for level in corpus {
        let text = level.to_text();
        let reparsed = Level::parse(&text)
            .unwrap_or_else(|e| panic!("{:?} failed to reparse: {e}", level.name));
        assert_eq!(
            reparsed.to_text(),
            text,
            "{:?} is not a serialization fixed point",
            level.name
        );
        assert_eq!(
            reparsed.board().state_hash(),
            level.board().state_hash(),
            "{:?} rebuilds a different board after a round trip",
            level.name
        );
    }
}

/// A recorded round must survive the text format: to_text -> parse ->
/// playback lands on the same final board as the original.
#[test]
fn replays_round_trip_through_text() {
    let board = classic_arena(false, 4);
    let mut replay = Replay::new(Level::from_board("Round Trip", 3, board.clone()));
    assert_eq!(
        replay.level.board().state_hash(),
        board.state_hash(),
        "Level::from_board loses starting-board state before any input"
    );
    let mut live = board;
    for tick in 0..400u16 {
        let mut actions = [PlayerAction::None; MAX_PLAYERS];
        // A sprinkling of placements so the input lines are non-trivial.
        if tick % 90 == 0 {
            let seat = (tick / 90) as u8 % 4;
            actions[seat as usize] = PlayerAction::Place {
                x: 2 + seat,
                y: 4,
                dir: Direction::Up,
            };
        }
        replay.record(actions);
        live.tick(&actions);
    }
    let text = replay.to_text();
    let reparsed = Replay::parse(&text).expect("replay text must reparse");
    assert_eq!(reparsed.to_text(), text, "replay text is not a fixed point");
    assert_eq!(
        reparsed.playback().state_hash(),
        live.state_hash(),
        "replayed round diverged from the live one"
    );
}

/// The text formats are the one place a stranger's bytes reach the parsers:
/// a level or a round arrives as a share code someone read off another
/// screen, and a custom level is a file anyone can hand-edit. Every one of
/// these parsers is fallible on purpose and every caller handles the error,
/// so the contract is that malformed input comes back as `Err`: never as a
/// panic, and never as a board silently different from the one described.
///
/// Mutating real files rather than throwing random bytes at it: a corrupt
/// save is a good file with something wrong in it, and that is the shape
/// that finds the interesting bugs. A lone map border line, odd-sized and
/// so past the only size check there was, used to reach `Board::new` as a
/// zero-row board and panic.
#[test]
fn mangled_text_is_refused_rather_than_followed() {
    use pinch_points::sim::{Board, Pcg32};

    let seeds: Vec<String> = campaign_levels()
        .iter()
        .chain(challenge_levels().iter())
        .take(4)
        .map(Level::to_text)
        .chain([String::new(), "map:\n+-+\n|.|\n+-+\n".to_string()])
        .collect();
    let mut rng = Pcg32::new(0xF0FF_1234, 0x2468);
    let glyphs = b"+-|.#0123456789 \n:";
    for round in 0..60_000u32 {
        let mut bytes = seeds[(round as usize) % seeds.len()].clone().into_bytes();
        for _ in 0..(rng.next_u32() % 10) + 1 {
            if bytes.is_empty() {
                bytes.push(glyphs[(rng.next_u32() as usize) % glyphs.len()]);
                continue;
            }
            let at = (rng.next_u32() as usize) % bytes.len();
            match rng.next_u32() % 5 {
                0 => bytes[at] = (rng.next_u32() % 256) as u8,
                1 => drop(bytes.remove(at)),
                2 => bytes.insert(at, (rng.next_u32() % 256) as u8),
                3 => bytes.insert(at, glyphs[(rng.next_u32() as usize) % glyphs.len()]),
                _ => bytes.truncate(at),
            }
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        // A parsed level is also a board the sim has to be able to run.
        if let Ok(level) = Level::parse(&text) {
            assert!(level.board().width() > 0 && level.board().height() > 0);
        }
        let _ = Replay::parse(&text);
        let _ = Board::parse_snapshot(&text);
    }
}
