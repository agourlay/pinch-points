//! The co-op stages: each is proved to need both players, each hand's
//! arrows minimal, and the claw split itself, by the solver run to the end
//! on every way of dealing the stage's arrows (`Deal`).

use pinch_points::sim::{
    Deal, Effort, Placement, PuzzleOutcome, SolveOutcome, campaign_levels, coop_levels, solve_with,
    winning_sets,
};

/// Every co-op stage is one: two hands, a stage rather than a beach, and
/// a name of its own beside the Tide Pool's, which progress is filed by.
#[test]
fn every_coop_stage_is_a_stage_for_two_with_a_name_of_its_own() {
    let levels = coop_levels();
    assert_eq!(levels.len(), 20, "the co-op set");
    let mut names: std::collections::HashSet<String> = campaign_levels()
        .into_iter()
        .map(|level| level.name)
        .collect();
    for level in &levels {
        let hands = level
            .hands()
            .unwrap_or_else(|| panic!("{:?} has no hands", level.name));
        assert!(
            hands[0] > 0 && hands[1] > 0,
            "{:?}: an empty hand",
            level.name
        );
        assert_eq!(level.kind, pinch_points::sim::LevelKind::Puzzle);
        assert!(names.insert(level.name.clone()), "{:?} twice", level.name);
    }
}

/// The shelf runs by grant, fewest arrows first: a tile's edge colour is
/// its grant, and the key under the grid reads it as how hard the stage
/// is, so a colour stepping back down along an easiest-first shelf said
/// two things at once.
#[test]
fn the_coop_shelf_never_grants_fewer_arrows_than_the_stage_before() {
    let grants: Vec<u8> = coop_levels().iter().map(|level| level.posts).collect();
    assert!(
        grants.windows(2).all(|pair| pair[0] <= pair[1]),
        "grants along the shelf: {grants:?}"
    );
}

/// The authored solution wins, every arrow placed by the hand it names,
/// and the beach with no arrows loses.
#[test]
fn every_coop_stage_is_won_by_its_solution_and_lost_without_one() {
    for level in coop_levels() {
        let mut board = level.solved_board().unwrap_or_else(|(x, y, _, seat)| {
            panic!("{:?}: seat {seat}'s arrow at ({x},{y}) refused", level.name)
        });
        assert_eq!(
            level.play_out(&mut board).0,
            PuzzleOutcome::Won,
            "{:?}: its solution",
            level.name
        );
        assert_ne!(
            level.play_out(&mut level.board()).0,
            PuzzleOutcome::Won,
            "{:?}: wins with no arrows",
            level.name
        );
    }
}

/// The guards that make a co-op stage one for two, each an exhaustive
/// search: the fewest arrows that win are the grant, each hand is
/// minimal (one fewer in either loses), neither player wins alone with
/// every arrow in their hand, and ordinary arrows that turn every crab do
/// not win either, so the stage is not a solo puzzle with a quota.
///
/// Ignored in the everyday run: the four-arrow stages with gulls take
/// many minutes in a debug build. Run it after editing a co-op stage in a
/// release build without LTO, under half a minute for the twenty:
/// `CARGO_PROFILE_RELEASE_LTO=off cargo test --release --target-dir
/// target/nolto --test it coop -- --ignored`, or read the same verdicts off
/// `cargo run --release --example author -- --exhaustive levels/coop/*.txt`.
#[test]
#[ignore = "exhaustive: run it in a release build"]
fn every_coop_stage_needs_both_players() {
    std::thread::scope(|scope| {
        for level in coop_levels() {
            scope.spawn(move || {
                let hands = level.hands().expect("a co-op stage");
                match solve_with(&level, Effort::Exhaustive) {
                    SolveOutcome::Found(found) => assert_eq!(
                        found.len(),
                        usize::from(level.posts),
                        "{:?}: wins with fewer arrows",
                        level.name
                    ),
                    other @ (SolveOutcome::Unsolvable | SolveOutcome::GaveUp) => {
                        panic!("{:?}: {other:?}", level.name)
                    }
                }
                for deal in Deal::all(hands) {
                    assert_eq!(
                        solve_with(&deal.level(&level), Effort::Exhaustive),
                        SolveOutcome::Unsolvable,
                        "{:?} is won as {deal:?}",
                        level.name
                    );
                }
            });
        }
    });
}

/// No co-op stage's authored solution puts two arrows face to face, which
/// shuttle a crab between them on any board (see the campaign's own
/// guard). This reads the authored solution only; other winning sets are
/// what the authoring probe checks.
#[test]
fn no_coop_solution_puts_two_arrows_face_to_face() {
    for level in coop_levels() {
        for &(x, y, dir, _) in &level.solution {
            let (dx, dy) = dir.offset();
            let ahead = (i32::from(x) + dx, i32::from(y) + dy);
            assert!(
                !level.solution.iter().any(|&(a, b, d, _)| {
                    (i32::from(a), i32::from(b)) == ahead && d == dir.reverse()
                }),
                "{:?}: arrows at ({x},{y}) face each other",
                level.name
            );
        }
    }
}

/// What a stage does on `seed`: whether its authored solution wins, and
/// whether the beach wins bare or with either hand's half of the solution
/// alone.
fn on_seed(level: &pinch_points::sim::Level, seed: u64) -> [bool; 4] {
    let play = |hands: &[u8]| {
        let mut board = level.board_seeded(seed);
        for &(x, y, dir, seat) in &level.solution {
            if hands.contains(&seat) {
                assert!(
                    board.place_signpost(seat, x, y, dir),
                    "{:?}: ({x},{y}) refused on seed {seed}",
                    level.name
                );
            }
        }
        level.play_out(&mut board).0 == PuzzleOutcome::Won
    };
    [play(&[0, 1]), play(&[]), play(&[0]), play(&[1])]
}

/// A stage is the same puzzle on every seed: a gull's claw and the moment
/// it first takes off are drawn from the stage's seed, and neither is
/// shown, so a stage whose answer leans on either is won by luck. Its
/// authored solution wins, and the bare beach and each hand's half of the
/// solution lose, on every seed of `seeds`. A stage with no gull draws
/// nothing from its stream, which two seeds are enough to show.
fn holds_on_every_seed(seeds: u64) {
    for level in coop_levels() {
        let seeds = match level.board().gulls().is_empty() {
            true => 2,
            false => seeds,
        };
        for seed in 0..seeds {
            assert_eq!(
                on_seed(&level, seed),
                [true, false, false, false],
                "{:?} on seed {seed}: [solution, bare, P1's half, P2's half] won",
                level.name
            );
        }
    }
}

/// [`holds_on_every_seed`] over a handful of seeds, in the everyday run.
#[test]
fn every_coop_stage_holds_on_other_seeds() {
    holds_on_every_seed(12);
}

/// [`holds_on_every_seed`] over a thousand seeds. Ignored in the everyday
/// run; with the exhaustive guard after editing a stage, in a release
/// build: `CARGO_PROFILE_RELEASE_LTO=off cargo test --release --target-dir
/// target/nolto --test it coop -- --ignored`.
#[test]
#[ignore = "a thousand seeds: run it in a release build"]
fn every_coop_stage_holds_on_a_thousand_seeds() {
    holds_on_every_seed(1000);
}

/// Whether `set` wins `level` on `seed`.
fn set_wins(level: &pinch_points::sim::Level, set: &[Placement], seed: u64) -> bool {
    let mut board = level.board_seeded(seed);
    for &(x, y, dir, seat) in set {
        assert!(
            board.place_signpost(seat, x, y, dir),
            "{:?}: ({x},{y}) refused on seed {seed}",
            level.name
        );
    }
    level.play_out(&mut board).0 == PuzzleOutcome::Won
}

/// Every way to win a stage on its shipped seed wins on every other seed
/// of `seeds` too. [`holds_on_every_seed`] asks it of the authored
/// solution alone; a second winning set that leans on the seed's hidden
/// gull (its claw, its first takeoff) is one two players find and then
/// cannot repeat, and the mirror of it loses for no reason they can see.
fn every_win_holds_on_every_seed(seeds: u64) {
    std::thread::scope(|scope| {
        for level in coop_levels() {
            scope.spawn(move || {
                let seeds = match level.board().gulls().is_empty() {
                    true => 2,
                    false => seeds,
                };
                let sets = winning_sets(&level);
                assert!(!sets.is_empty(), "{:?}: no winning set", level.name);
                for set in &sets {
                    let lost: Vec<u64> =
                        (0..seeds).filter(|&s| !set_wins(&level, set, s)).collect();
                    assert!(
                        lost.is_empty(),
                        "{:?}: {set:?} wins on the shipped seed and loses on seeds {lost:?} \
                         ({} winning sets)",
                        level.name,
                        sets.len()
                    );
                }
            });
        }
    });
}

/// [`every_win_holds_on_every_seed`] over two hundred seeds. Ignored in the
/// everyday run, an exhaustive search per stage: with the exhaustive guard
/// after editing a stage, in a release build, `CARGO_PROFILE_RELEASE_LTO=off
/// cargo test --release --target-dir target/nolto --test it coop --
/// --ignored`.
#[test]
#[ignore = "exhaustive: run it in a release build"]
fn every_winning_set_holds_on_other_seeds() {
    every_win_holds_on_every_seed(200);
}
