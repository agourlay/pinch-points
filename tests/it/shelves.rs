//! The three things the shell keeps on disk between runs, each read back
//! from a scratch directory: the replay library, the player's own levels,
//! and the cleared-stage record.
//!
//! Each goes through a `_in` seam that takes the directory outright. The
//! real paths hang off `$XDG_DATA_HOME`, and setting that from a test
//! would race every other test in this binary, which all run in one
//! process.

use pinch_points::app::campaign::{custom_arenas, custom_puzzles, load_custom_levels_in};
use pinch_points::app::progress::{Progress, load_in, save_in};
use pinch_points::app::replays::{file_name, prune_in, shelf_in};
use pinch_points::app::{Campaign, CampaignKind};
use pinch_points::sim::{
    CapPolicy, Direction, Level, LevelKind, MAX_PLAYERS, PlayerAction, Replay, campaign_levels,
    classic_arena,
};
use std::path::PathBuf;

/// A fresh, empty directory of our own under the system temp dir, named
/// by test and process so two runs cannot share one.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pinch-it-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// A short recorded round on the classic beach, with a few placements so
/// the input lines are not all idle, and the board it ended on.
fn recorded_round() -> (Replay, pinch_points::sim::Board) {
    let board = classic_arena(false, 4);
    let mut replay = Replay::new(Level::from_board("Kept", 3, board.clone()));
    let mut live = board;
    for tick in 0..300u16 {
        let mut actions = [PlayerAction::None; MAX_PLAYERS];
        if tick % 75 == 0 {
            let seat = (tick / 75) as u8 % 4;
            actions[seat as usize] = PlayerAction::Place {
                x: 2 + seat,
                y: 4,
                dir: Direction::Up,
            };
        }
        replay.record(actions);
        live.tick(&actions);
    }
    (replay, live)
}

/// The library lists rounds and only rounds, newest first; pruning keeps
/// the newest and leaves `last.txt` alone; and a kept file is the round
/// it says it is, replaying to the board the round ended on.
#[test]
fn a_kept_round_comes_off_the_shelf_and_replays() {
    let dir = scratch("replays");
    let (replay, ended) = recorded_round();
    let text = replay.to_text();
    let stamps = [1_000u64, 2_000, 3_000, 4_000];
    for (stamp, winner) in stamps.iter().zip(["Anna", "Bo", "Cy", "Di"]) {
        std::fs::write(dir.join(file_name(*stamp, winner)), &text).unwrap();
    }
    std::fs::write(dir.join("last.txt"), &text).unwrap();
    std::fs::write(dir.join("notes.txt"), "not a round").unwrap();

    let kept = shelf_in(&dir);
    let names: Vec<String> = kept
        .iter()
        .map(|k| k.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        [
            "round_0000004000_Di.txt",
            "round_0000003000_Cy.txt",
            "round_0000002000_Bo.txt",
            "round_0000001000_Anna.txt",
        ],
        "four rounds, newest first, and nothing else"
    );
    assert!(
        kept.iter().all(|k| !k.label.is_empty()),
        "every kept round has something to call it on screen"
    );

    prune_in(&dir, 2);
    let after: Vec<String> = shelf_in(&dir)
        .iter()
        .map(|k| k.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        after,
        ["round_0000004000_Di.txt", "round_0000003000_Cy.txt"],
        "pruning to two keeps the two newest"
    );
    assert!(
        dir.join("last.txt").exists(),
        "last.txt is the menu's, not the shelf's"
    );
    assert!(
        dir.join("notes.txt").exists(),
        "a stray file is nobody's to delete"
    );

    let picked = std::fs::read_to_string(&shelf_in(&dir)[0].path).unwrap();
    let parsed = Replay::parse(&picked).expect("a kept round parses");
    assert_eq!(
        parsed.playback().state_hash(),
        ended.state_hash(),
        "the kept round replays to the board it ended on"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn level_text(name: &str, kind: &str) -> String {
    format!(
        "name: {name}\nposts: 2\nkind: {kind}\ncrab: 0,0 R L common\nmap:\n+-+-+-+\n|0 . 1|\n+-+-+-+\n"
    )
}

/// The player's shelf: the old save slot first, then the shelf directory
/// in name order, `.txt` only, and a file that will not parse is skipped
/// rather than taking the rest down with it. The kind on the file decides
/// which list the level joins.
#[test]
fn the_shelf_takes_the_good_files_and_skips_the_bad() {
    let dir = scratch("custom");
    let legacy = dir.join("custom.txt");
    let custom = dir.join("custom");
    std::fs::create_dir_all(&custom).unwrap();
    std::fs::write(&legacy, level_text("Old Slot", "puzzle")).unwrap();
    // Named so that file order and level name disagree with the order
    // they are written in: only the sort can put them right.
    std::fs::write(custom.join("later.txt"), level_text("Later Beach", "arena")).unwrap();
    std::fs::write(
        custom.join("early.txt"),
        level_text("Early Stage", "puzzle"),
    )
    .unwrap();
    let mut cut = level_text("Cut Short", "puzzle");
    cut.truncate(cut.find("map:\n").unwrap() + 5);
    std::fs::write(custom.join("broken.txt"), cut).unwrap();
    std::fs::write(custom.join("readme.md"), "# not a level\n").unwrap();

    let levels = load_custom_levels_in(&legacy, &custom);
    let names: Vec<&str> = levels.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Old Slot", "Early Stage", "Later Beach"]);

    let puzzles: Vec<String> = custom_puzzles(levels.clone())
        .into_iter()
        .map(|l| l.name)
        .collect();
    let arenas: Vec<String> = custom_arenas(levels).into_iter().map(|l| l.name).collect();
    assert_eq!(puzzles, ["Old Slot", "Early Stage"]);
    assert_eq!(arenas, ["Later Beach"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// What the editor writes for a puzzle: the board snapshot with the
/// inventory stamped on it as a hard cap (see `editor::level_here`). Read
/// back off the shelf, the level plays under that same rule.
#[test]
fn an_editor_saved_puzzle_keeps_its_signpost_rule() {
    let dir = scratch("editor-save");
    let custom = dir.join("custom");
    std::fs::create_dir_all(&custom).unwrap();
    let posts = 3;
    let mut snapshot = classic_arena(false, 2);
    snapshot.set_signpost_rule(posts, CapPolicy::Reject);
    let level = Level::from_board("Made Here", posts, snapshot).with_kind(LevelKind::Puzzle);
    std::fs::write(custom.join("made_here.txt"), level.to_text()).unwrap();

    let levels = load_custom_levels_in(&dir.join("absent.txt"), &custom);
    assert_eq!(levels.len(), 1, "the absent legacy slot is not an error");
    let reloaded = &levels[0];
    assert_eq!(reloaded.name, "Made Here");
    assert_eq!(reloaded.kind, LevelKind::Puzzle);
    assert_eq!(reloaded.board().signpost_rule(), (posts, CapPolicy::Reject));
    let _ = std::fs::remove_dir_all(&dir);
}

fn tide_pool(levels: Vec<Level>) -> Campaign {
    let builtins = levels.len();
    Campaign {
        kind: CampaignKind::TidePool,
        levels,
        index: 0,
        builtins,
    }
}

/// Progress is filed by name, so it survives a restart and a level slid
/// into the middle of the list: the cleared stages stay cleared, the
/// furthest open stage moves down by exactly the one inserted, and the
/// newcomer is open (it follows a cleared stage) but not cleared.
#[test]
fn progress_survives_a_restart_and_a_level_inserted_ahead_of_it() {
    let dir = scratch("progress");
    let path = dir.join("progress.txt");
    let campaign = tide_pool(campaign_levels());
    let cleared: Vec<String> = campaign.levels[..5]
        .iter()
        .map(|l| l.name.clone())
        .collect();

    let mut progress = Progress::default();
    for name in &cleared {
        assert!(progress.mark(CampaignKind::TidePool, name));
    }
    save_in(&path, &progress).expect("the record writes");
    let before = progress.furthest_open(&campaign);
    assert_eq!(before, 5, "the stage after the fifth cleared one is open");

    let restarted = load_in(&path);
    for name in &cleared {
        assert!(
            restarted.is_cleared(CampaignKind::TidePool, name),
            "{name} lost"
        );
    }
    assert_eq!(restarted.cleared_in(&campaign, 0..campaign.levels.len()), 5);

    let mut levels = campaign_levels();
    let inserted = Level::parse(&level_text("Slid In", "puzzle")).unwrap();
    levels.insert(2, inserted);
    let shifted = tide_pool(levels);
    for name in &cleared {
        let at = shifted.levels.iter().position(|l| &l.name == name).unwrap();
        assert!(
            restarted.unlocked(&shifted, at),
            "{name} re-locked by the insert"
        );
        assert!(restarted.is_cleared(CampaignKind::TidePool, name));
    }
    assert_eq!(restarted.furthest_open(&shifted), before + 1);
    assert!(!restarted.is_cleared(CampaignKind::TidePool, "Slid In"));
    assert!(
        restarted.unlocked(&shifted, 2),
        "the classic rule opens the stage after a cleared one, newcomer or not"
    );
    assert_eq!(restarted.cleared_in(&shifted, 0..shifted.levels.len()), 5);
    let _ = std::fs::remove_dir_all(&dir);
}
