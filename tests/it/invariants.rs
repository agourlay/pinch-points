//! Rules that must hold however the game is played, hammered with a
//! thousand pseudorandom games.
//!
//! A seeded loop rather than a property-testing crate: the sim's own PRNG
//! generates the streams, so the crate stays dependency-free and every run
//! plays exactly the same thousand games, reproducing on the next machine.
//! What that gives up is shrinking; in exchange a failure prints the seed
//! and the tick, and the games are short enough to read.

use pinch_points::sim::{
    Board, BotLevel, DEFAULT_DELAY, Direction, GullState, InputMsg, Level, Lockstep, MAX_PLAYERS,
    MAX_SIGNPOSTS_PER_PLAYER, Pcg32, PlayerAction, SUBUNITS_PER_TILE, TileKind, bot_action,
    classic_arena_seeded, generate_arena,
};

/// Ticks per fuzzed game: eight seconds of play, long enough for crabs to
/// spawn, route, bank, and be eaten, short enough to run a thousand of.
const TICKS: u64 = 240;

/// A pseudorandom arena, cycling the shapes the game actually ships.
fn arena(seed: u64) -> Board {
    // The seat count varies with the seed: fuzzing four-castle boards alone
    // leaves everything that widens with the table (the level format, the
    // replay line, the state hash) untested past seat 3.
    let seats = 2 + (seed % (MAX_PLAYERS as u64 - 1)) as u8;
    match seed % 4 {
        // The handcrafted beach is a four-castle board whatever is asked.
        0 => classic_arena_seeded(seed, false, seats.min(4)),
        1 => generate_arena(seed, seats, 9, 7),
        2 => generate_arena(seed, seats, 12, 9),
        _ => generate_arena(seed, seats, 20, 13),
    }
}

/// One seat's action for one tick. Mostly idle, as hands are, with the rest
/// spread over placements anywhere on the board and the occasional removal,
/// including plenty of illegal ones, which the sim must simply refuse.
fn action(rng: &mut Pcg32, board: &Board) -> PlayerAction {
    let x = (rng.next_u32() % u32::from(board.width())) as u8;
    let y = (rng.next_u32() % u32::from(board.height())) as u8;
    match rng.next_u32() % 10 {
        0..=5 => PlayerAction::None,
        6..=8 => PlayerAction::Place {
            x,
            y,
            dir: Direction::from_letter(["U", "D", "L", "R"][(rng.next_u32() % 4) as usize])
                .expect("letter from the fixed list"),
        },
        _ => PlayerAction::Remove { x, y },
    }
}

/// A tick's worth of actions for every seat, some from the bots so the
/// stream includes plays that make sense as well as noise.
fn actions(rng: &mut Pcg32, board: &Board) -> [PlayerAction; MAX_PLAYERS] {
    std::array::from_fn(|seat| {
        if rng.next_u32().is_multiple_of(4) {
            bot_action(board, seat as u8, BotLevel::Normal)
        } else {
            action(rng, board)
        }
    })
}

/// Where a failure happened, printed only if one does.
///
/// A pair of numbers rather than a string, because assertion messages are
/// formatted on failure and nowhere else: building the label eagerly cost
/// a heap allocation on every one of a quarter of a million ticks, for a
/// line almost never read.
struct At {
    seed: u64,
    tick: u64,
}

impl std::fmt::Display for At {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "seed {} tick {}", self.seed, self.tick)
    }
}

/// Everything that must be true of a board after any tick, whatever was
/// asked of it. `at` names the game so a failure is reproducible.
fn check_board(board: &Board, at: &At) {
    let width = u16::from(board.width());
    let tiles = width * u16::from(board.height());

    // No crab is invented and none vanishes unaccounted for: whatever has
    // spawned is either banked, still walking, or was eaten.
    assert!(
        board.crabs_spawned() >= board.crabs_banked() + board.crabs().len() as u32,
        "{at}: {} spawned but {} banked and {} alive",
        board.crabs_spawned(),
        board.crabs_banked(),
        board.crabs().len(),
    );

    for crab in board.crabs() {
        assert!(crab.tile < tiles, "{at}: crab off the board");
        assert!(
            crab.progress < SUBUNITS_PER_TILE,
            "{at}: crab progress {} past a whole tile",
            crab.progress
        );
        let (x, y) = ((crab.tile % width) as u8, (crab.tile / width) as u8);
        let kind = board.tile_at(x, y);
        assert_ne!(kind, TileKind::Rock, "{at}: crab standing inside a rock");
        assert!(
            !matches!(kind, TileKind::Castle(_)),
            "{at}: crab resting on a castle instead of banking"
        );
    }

    for gull in board.gulls() {
        assert!(gull.tile < tiles, "{at}: gull off the board");
        let (x, y) = ((gull.tile % width) as u8, (gull.tile / width) as u8);
        if gull.state == GullState::Walking {
            // Kelp is a wall to a walking gull; one standing in it could
            // never have got there.
            assert_ne!(
                board.tile_at(x, y),
                TileKind::Kelp,
                "{at}: walking gull inside the kelp"
            );
        }
    }

    // Signposts: only ever on open sand, and never more than the cap.
    //
    // Counted in the same walk that checks what they stand on. Asking
    // `signpost_count` per seat is six scans of the whole board, and this
    // runs after every tick of every one of a thousand games. It was the
    // most expensive thing in the suite, by the profiler rather than by
    // guess.
    let mut held = [0usize; MAX_PLAYERS];
    for (x, y, kind) in board.tiles() {
        let Some(post) = board.signpost_at(x, y) else {
            continue;
        };
        assert_eq!(
            kind,
            TileKind::Empty,
            "{at}: signpost standing on {kind:?} at ({x},{y})"
        );
        held[usize::from(post.owner)] += 1;
    }
    for (seat, count) in held.iter().enumerate() {
        assert!(
            *count <= MAX_SIGNPOSTS_PER_PLAYER,
            "{at}: seat {seat} holds {count} signposts"
        );
    }
}

/// A thousand pseudorandom games, checked after every tick. The actions
/// include illegal placements on rocks, castles and rivals' signposts,
/// which the sim must refuse without ever reaching an impossible state.
#[test]
fn play_never_breaks_the_rules() {
    for seed in 0..1000u64 {
        let mut board = arena(seed);
        let mut rng = Pcg32::new(seed ^ 0x00F0_FF1E, 0x51EE);
        let (mut banked, mut spawned) = (0, 0);
        for tick in 0..TICKS {
            let at = &At { seed, tick };
            let plays = actions(&mut rng, &board);
            board.tick(&plays);
            check_board(&board, at);
            // The lifetime counters only ever climb.
            assert!(board.crabs_banked() >= banked, "{at}: banked count fell");
            assert!(board.crabs_spawned() >= spawned, "{at}: spawned count fell");
            banked = board.crabs_banked();
            spawned = board.crabs_spawned();
            assert_eq!(board.ticks(), tick + 1, "{at}: the clock skipped");
        }
    }
}

/// The determinism contract (spec §7.5) over a thousand different input
/// streams rather than the one the anchor test pins: same seed, same
/// actions, same state, and cloning a board never disturbs it.
#[test]
fn the_same_stream_always_replays_identically() {
    for seed in 0..1000u64 {
        let mut first = arena(seed);
        let mut second = arena(seed);
        let mut rng = Pcg32::new(seed, 0xD37);
        let mut stream = Vec::with_capacity(TICKS as usize);
        // A clone is the same board: rollback netcode will lean on this.
        // Checked once per seed rather than once per tick, at a moment that
        // moves with the seed: a thousand boards caught at a thousand
        // different points is the same breadth of evidence as a quarter of
        // a million, and it was costing a whole-board clone and two
        // whole-board hashes on every one of them.
        let clone_at = seed % TICKS;
        for tick in 0..TICKS {
            let plays = actions(&mut rng, &first);
            stream.push(plays);
            first.tick(&plays);
            if tick == clone_at {
                assert_eq!(
                    first.clone().state_hash(),
                    first.state_hash(),
                    "seed {seed}: cloning changed the state at tick {tick}"
                );
            }
        }
        for plays in &stream {
            second.tick(plays);
        }
        assert_eq!(
            first.state_hash(),
            second.state_hash(),
            "seed {seed}: the same stream diverged"
        );
    }
}

/// The level format against boards nobody authored: play a while, write the
/// board out, read it back, and compare what the format promises to carry:
/// the tile grid and the walls. (Not sub-tile creature progress: the format
/// stores crabs by tile, which is all a level needs.)
#[test]
fn any_played_board_survives_the_level_format() {
    for seed in 0..200u64 {
        let mut board = arena(seed);
        let mut rng = Pcg32::new(seed, 0x1E_7E10);
        for _ in 0..(seed % TICKS) {
            let plays = actions(&mut rng, &board);
            board.tick(&plays);
        }
        let text = Level::from_board("Fuzz", 3, board.clone()).to_text();
        let parsed = Level::parse(&text)
            .unwrap_or_else(|e| panic!("seed {seed}: own output rejected: {e}\n{text}"))
            .board();
        assert_eq!(parsed.width(), board.width(), "seed {seed}: width");
        assert_eq!(parsed.height(), board.height(), "seed {seed}: height");
        for (x, y, kind) in board.tiles() {
            assert_eq!(parsed.tile_at(x, y), kind, "seed {seed}: tile ({x},{y})");
            for dir in [
                Direction::Up,
                Direction::Down,
                Direction::Left,
                Direction::Right,
            ] {
                assert_eq!(
                    parsed.wall_at(x, y, dir),
                    board.wall_at(x, y, dir),
                    "seed {seed}: wall {dir:?} of ({x},{y})"
                );
            }
        }
    }
}

/// What the level format cannot promise, and the snapshot must: a board
/// caught mid-round carries sub-tile creature positions, standing signposts,
/// the PRNG's place in its stream, and whatever tide effects are running.
/// `state_hash` is the whole of that state, so comparing it is the whole
/// question.
///
/// Then both boards are played on with the same inputs. A snapshot that
/// restored everything except the PRNG position would pass the first
/// assertion and fail this one on the next draw.
///
/// What this loop does *not* reach, measured: a running mania, a tempo
/// shift, or a recorded last event. All three need a sparkling crab banked
/// inside the eight seconds, which happens in none of two hundred games.
/// Those are covered by `a_board_with_everything_set_survives_a_snapshot`
/// beside the code.
#[test]
fn any_played_board_survives_a_snapshot() {
    for seed in 0..200u64 {
        let mut board = arena(seed);
        let mut rng = Pcg32::new(seed, 0x5A4F_0715);
        for _ in 0..(seed % TICKS) {
            let plays = actions(&mut rng, &board);
            board.tick(&plays);
        }
        let text = board.to_snapshot();
        let mut restored = Board::parse_snapshot(&text)
            .unwrap_or_else(|e| panic!("seed {seed}: own output rejected: {e}\n{text}"));
        assert_eq!(
            restored.state_hash(),
            board.state_hash(),
            "seed {seed}: the snapshot came back a different board"
        );

        let mut played = board;
        for tick in 0..60 {
            let plays = actions(&mut rng, &played);
            played.tick(&plays);
            restored.tick(&plays);
            assert_eq!(
                restored.state_hash(),
                played.state_hash(),
                "seed {seed}: diverged {tick} ticks after resuming"
            );
        }
    }
}

/// The consequence the format bug actually had: a recorded round stores its
/// starting board *as a level*, so anything the format drops is something
/// the replay plays differently from the round you watched. Generated
/// arenas mirror their turnstiles, since a reflection swaps left and right,
/// so half of them start pivoting the other way, and dropping that flipped
/// them on the way to disk.
#[test]
fn a_recorded_round_replays_the_round_that_was_played() {
    for seed in 0..200u64 {
        let start = arena(seed);
        // The shape the shell records: the starting board, as a level.
        let recorded = Level::from_board("Turf War", 3, start.clone()).to_text();
        let mut watched = Level::parse(&recorded)
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"))
            .board();
        let mut played = start;
        // `Level::board` applies the level's signpost rule; the recorded
        // rule is the one the round was played under, so both sides agree.
        let mut rng = Pcg32::new(seed, 0xEE1);
        for tick in 0..TICKS {
            let plays = actions(&mut rng, &played);
            played.tick(&plays);
            watched.tick(&plays);
            assert_eq!(
                played.scores(),
                watched.scores(),
                "seed {seed} tick {tick}: the replay scored differently"
            );
            assert_eq!(
                played.crabs().len(),
                watched.crabs().len(),
                "seed {seed} tick {tick}: the replay lost or gained a crab"
            );
        }
    }
}

/// Lockstep under every delivery schedule the loop can think of: packets
/// held back, delivered out of order, and repeated. The protocol promises
/// that peers either agree or stall, never that they guess, so whatever
/// the network does to the order, the two boards must stay bit-identical.
///
/// There is one hand-written lag pattern in `sim::net`'s own tests; this is
/// two hundred more, and it is the kind of coverage a netcode wants.
#[test]
fn lockstep_survives_any_delivery_schedule() {
    for seed in 0..200u64 {
        let players = vec![0u8, 1];
        let mut sessions = [
            Lockstep::new(0, players.clone(), DEFAULT_DELAY),
            Lockstep::new(1, players, DEFAULT_DELAY),
        ];
        let mut boards = [arena(seed), arena(seed)];
        let mut rng = Pcg32::new(seed, 0x5C_9ED2);
        // Messages in flight, each with the peer it is bound for.
        let mut wire: Vec<(usize, InputMsg)> = Vec::new();

        for _ in 0..300 {
            for peer in 0..2usize {
                let play = action(&mut rng, &boards[peer]);
                if let Some(msg) = sessions[peer].commit_local(play) {
                    // Sometimes send it twice: a resend must be harmless.
                    let copies = 1 + usize::from(rng.next_u32().is_multiple_of(5));
                    for _ in 0..copies {
                        wire.push((1 - peer, msg));
                    }
                }
            }
            // Deliver a pseudorandom slice of the wire, in a pseudorandom
            // order, holding the rest back for a later step.
            let deliver = if wire.is_empty() {
                0
            } else {
                (rng.next_u32() as usize) % (wire.len() + 1)
            };
            for _ in 0..deliver {
                let at = (rng.next_u32() as usize) % wire.len();
                let (to, msg) = wire.swap_remove(at);
                sessions[to].receive(msg);
            }
            for peer in 0..2usize {
                while let Some(plays) = sessions[peer].advance() {
                    boards[peer].tick(&plays);
                }
            }
        }
        // Flush everything and let both drain: with all inputs delivered,
        // both peers must have simulated exactly the same frames.
        for (to, msg) in wire.drain(..) {
            sessions[to].receive(msg);
        }
        for peer in 0..2usize {
            while let Some(plays) = sessions[peer].advance() {
                boards[peer].tick(&plays);
            }
        }
        assert_eq!(
            sessions[0].frame(),
            sessions[1].frame(),
            "seed {seed}: peers ended on different frames"
        );
        assert_eq!(
            boards[0].state_hash(),
            boards[1].state_hash(),
            "seed {seed}: peers diverged"
        );
        assert!(
            sessions[0].frame() > 100,
            "seed {seed}: the session made no real progress ({})",
            sessions[0].frame()
        );
    }
}
