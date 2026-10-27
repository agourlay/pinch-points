//! Small hand-written levels for the format tests: the level-file keys no
//! shipped stage happens to use, so a mismatched edit to their writer or
//! reader is caught although the campaign never says them.

use pinch_points::sim::Level;

/// Spawners on an evicting cap with gulls on a timer, castle raids off.
const SPAWNERS_EVICT_GULLS: &str = "\
name: Fixture Flood
posts: 3
rule: evict 3
round: 900
goal: bank 15
spawner: 0,0 R 10
spawner: 8,2 L 10
gull_period: 220
raids: off
map:
+-+-+-+-+-+-+-+-+-+
|. . . . . . . . .|
+ + + + + + + + + +
|# # # # 0 # # # #|
+ + + + + + + + + +
|. . . . . . . . .|
+-+-+-+-+-+-+-+-+-+
";

/// A rejecting cap with castle raids left on, said out loud, and a spawner
/// facing down.
const RAIDS_ON_REJECT: &str = "\
name: Fixture Raids
posts: 2
rule: reject 2
raids: on
spawner: 3,0 D 6
gull: 0,2 R
map:
+-+-+-+-+-+-+-+
|. . . . . . .|
+ + + + + + + +
|. . . . . . .|
+ + + + + + + +
|. . . 0 . . .|
+-+-+-+-+-+-+-+
";

/// The fixture levels, parsed.
pub fn rule_levels() -> Vec<Level> {
    [SPAWNERS_EVICT_GULLS, RAIDS_ON_REJECT]
        .iter()
        .map(|text| Level::parse(text).unwrap_or_else(|e| panic!("fixture refused: {e}")))
        .collect()
}

/// The fixtures say every key they are here for, so the tests over them
/// cover what they claim to.
#[test]
fn the_fixtures_say_the_keys_they_are_for() {
    let texts: Vec<String> = rule_levels().iter().map(Level::to_text).collect();
    let all = texts.join("\n");
    for key in ["spawner:", "rule: evict", "raids: off", "gull_period:"] {
        assert!(all.contains(key), "no fixture writes {key:?}");
    }
    let rules = *rule_levels()[0].board().rules();
    assert!(!rules.castle_raids, "raids: off is read");
    assert_eq!(rules.gull_period, 220, "gull_period is read");
    assert!(
        rule_levels()[1].board().rules().castle_raids,
        "raids: on is read"
    );
}
