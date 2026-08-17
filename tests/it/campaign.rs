//! Solvability proof for every shipped level: replay the authored solution
//! and require a win within the puzzle tick limit (spec §5.4's validation
//! idea, applied to the §5.1 campaign).

use pinch_points::sim::{Direction, PuzzleOutcome, campaign_levels, challenge_levels};

#[test]
fn every_campaign_level_is_solvable_with_its_solution() {
    let levels = campaign_levels();
    assert_eq!(
        levels.len(),
        82,
        "20 spec, 5 terrain, 5 advanced-rule, 20 advanced, 20 with the gulls in, \
         12 that want four signposts"
    );
    for (i, level) in levels.iter().enumerate() {
        let n = i + 1;
        assert!(
            level.solution.len() <= level.posts as usize,
            "level {n} {:?}: solution uses more posts than the inventory",
            level.name
        );
        let mut board = level.board();
        for &(x, y, dir) in &level.solution {
            assert!(
                board.place_signpost(0, x, y, dir),
                "level {n} {:?}: solution placement at ({x},{y}) rejected",
                level.name
            );
        }
        let outcome = loop {
            board.tick_idle();
            match level.outcome(&board) {
                PuzzleOutcome::Running => {}
                done @ (PuzzleOutcome::Won | PuzzleOutcome::Lost) => break done,
            }
        };
        assert_eq!(
            outcome,
            PuzzleOutcome::Won,
            "level {n} {:?}: solution did not bank all crabs ({} left after {} ticks)",
            level.name,
            board.crabs().len(),
            board.ticks(),
        );
    }
}

/// Levels that grant signposts must actually need them: a level whose crabs
/// bank by themselves teaches a false lesson about its own solution.
#[test]
fn granted_signposts_are_necessary() {
    for (i, level) in campaign_levels().iter().enumerate() {
        if level.posts == 0 || level.solution.is_empty() {
            continue; // watch-levels solve themselves by design
        }
        let mut board = level.board();
        let outcome = loop {
            board.tick_idle();
            match level.outcome(&board) {
                PuzzleOutcome::Running => {}
                done @ (PuzzleOutcome::Won | PuzzleOutcome::Lost) => break done,
            }
        };
        assert_ne!(
            outcome,
            PuzzleOutcome::Won,
            "level {} {:?} is solvable with zero signposts",
            i + 1,
            level.name
        );
    }
}

// Every granted signpost must be load-bearing, and one more guard proves it:
// `granted_signposts_are_necessary` (above) only catches a level that needs
// *none* of its posts; a level that hands over two and falls to one is just
// as dishonest. Six levels did exactly that, their boards open enough that
// both crabs drifted onto one border circuit where a single post caught them
// all. Catching it means proving no cheaper solution exists, which is an
// exhaustive-per-level solver search: a minute in release across every core,
// far more as the `#[ignore]`d debug test it used to be. So it is not a test
// and not in CI - it lives in `examples/verify_levels.rs`, run by hand after
// editing a level file (`cargo run --release --example verify_levels`), since
// levels change far less often than code. It also calibrates
// `DEFAULT_NODE_BUDGET`: nothing else searches a real level under the ceiling
// the editor gives up at.

/// No level is beaten by pointing two signposts at each other.
///
/// A crab arriving at either one is sent to the other, and bounces between
/// the two until the tide. It satisfies a survive goal, it satisfies a
/// bank-nothing goal, and it does so on *any* board, which is what makes it
/// a cheat rather than an answer: the level's walls, weed and gulls stop
/// mattering the moment it is available.
///
/// `Kelp Keep` shipped exactly this. The three guards above all passed,
/// because the pair really does win and the board really does lose without
/// it. What none of them asked was whether the win had anything to do with
/// the beach it was won on.
///
/// A post aimed into a *wall* is a different thing and stays legal: the
/// wall reverses the crab (spec §9 open question 2, frozen), that reversal
/// is a property of where the wall is, and a level can be built to teach it.
#[test]
fn no_level_is_beaten_by_a_pair_of_signposts_facing_each_other() {
    for (i, level) in campaign_levels()
        .iter()
        .chain(challenge_levels().iter())
        .enumerate()
    {
        let posts: std::collections::HashMap<(u8, u8), Direction> = level
            .solution
            .iter()
            .map(|&(x, y, dir)| ((x, y), dir))
            .collect();
        for (&(x, y), &dir) in &posts {
            let (dx, dy) = dir.offset();
            let ahead = (
                x.wrapping_add_signed(dx as i8),
                y.wrapping_add_signed(dy as i8),
            );
            assert_ne!(
                posts.get(&ahead),
                Some(&dir.reverse()),
                "level {} {:?}: the signposts at ({x},{y}) and {ahead:?} point at each \
                 other, which traps a crab between them on any board at all",
                i + 1,
                level.name,
            );
        }
    }
}

/// A challenge stage has to ask something of the player. A stage whose goal
/// the board meets on its own is beaten by pressing Enter and watching,
/// which is not a score attack.
///
/// The campaign has had this guard from the start
/// (`granted_signposts_are_necessary`); Beach Day never did, and four of its
/// eight stages were winnable with no input at all.
#[test]
fn every_challenge_stage_asks_something_of_the_player() {
    for (i, stage) in challenge_levels().iter().enumerate() {
        let mut board = stage.board();
        let outcome = loop {
            board.tick_idle();
            match stage.outcome(&board) {
                PuzzleOutcome::Running => {}
                done @ (PuzzleOutcome::Won | PuzzleOutcome::Lost) => break done,
            }
        };
        assert_ne!(
            outcome,
            PuzzleOutcome::Won,
            "stage {} {:?} is won without placing anything: it banked {} on its own",
            i + 1,
            stage.name,
            board.crabs_banked(),
        );
    }
}

/// Beach Day stages must be beatable with their authored solutions before
/// the tide (goal-aware outcome).
#[test]
fn every_challenge_stage_is_beatable() {
    let stages = challenge_levels();
    assert!(stages.len() >= 8, "Beach Day ships at least 8 stages");
    for (i, stage) in stages.iter().enumerate() {
        let n = i + 1;
        let mut board = stage.board();
        for &(x, y, dir) in &stage.solution {
            assert!(
                board.place_signpost(0, x, y, dir),
                "stage {n} {:?}: solution placement at ({x},{y}) rejected",
                stage.name
            );
        }
        let outcome = loop {
            board.tick_idle();
            match stage.outcome(&board) {
                PuzzleOutcome::Running => {}
                done @ (PuzzleOutcome::Won | PuzzleOutcome::Lost) => break done,
            }
        };
        assert_eq!(
            outcome,
            PuzzleOutcome::Won,
            "stage {n} {:?} not beaten: banked {} of spawned {}, golden {}, alive {}, tick {}",
            stage.name,
            board.crabs_banked(),
            board.crabs_spawned(),
            board.golden_banked(),
            board.crabs().len(),
            board.ticks(),
        );
    }
}

#[test]
fn campaign_levels_have_unique_names() {
    let levels = campaign_levels();
    let mut names: Vec<&str> = levels.iter().map(|l| l.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), levels.len());
}
