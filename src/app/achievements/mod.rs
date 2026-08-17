//! Lifetime stats and achievements, fed by the `SimEvent` stream and the
//! round/puzzle outcomes. Everything persists to `achievements.txt` next to
//! the game (lenient parse, like settings). Unlocks pop a toast and chime.
//!
//! Attribution: stats count the local player's seat, seat 0 in local play
//! and the session seat online. Bot seats never earn anything.

use crate::sim::TideEvent;
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
/// The trophy record, under the XDG data directory.
pub(super) fn save_path() -> std::path::PathBuf {
    crate::app::paths::data_dir().join("achievements.txt")
}

/// Lifetime counters for the local player.
#[derive(Resource, Default, Clone, PartialEq, Eq, Debug)]
pub struct Stats {
    pub banked: u32,
    pub golden: u32,
    pub lures: u32,
    pub events: u32,
    pub gulls_fed: u32,
    pub raids_taken: u32,
    pub rounds: u32,
    pub wins: u32,
    pub puzzles: u32,
    pub dry_wins: u32,
    /// The day (days since epoch, UTC) `daily_best` belongs to.
    pub daily_day: u32,
    /// Best local score in today's daily challenge.
    pub daily_best: u32,
    /// Giant crabs banked: the ten-pointers worth protecting.
    pub giants: u32,
    /// One bit per tide event ever spun (see [`TideEvent::index`]), so the
    /// roulette trophy needs *variety* rather than volume.
    pub events_seen: u8,
    /// Most crabs banked in a single round.
    pub best_round: u32,
    /// Best-of-five series taken.
    pub series_wins: u32,
    /// Rounds won against a live opponent over the wire.
    pub online_wins: u32,
    /// Distinct days the daily challenge was played.
    pub daily_days: u32,
    /// Levels saved out of the editor.
    pub levels_built: u32,
    /// 1 once every built-in Tide Pool stage has been cleared. A count of
    /// solves cannot answer this: it counts re-solves, and says nothing
    /// about which stages they were.
    pub campaign_done: u32,
    /// 1 once every Beach Day stage has been cleared.
    pub beach_done: u32,
    /// Raids on the local castle this round (round-scoped scratch).
    pub raids_this_round: u32,
    /// Crabs banked this round (round-scoped scratch, feeds `best_round`).
    pub banked_this_round: u32,
}

/// Ids of unlocked achievements.
#[derive(Resource, Default)]
pub struct Unlocked(pub HashSet<&'static str>);

/// One achievement: id, stat selector, and the threshold to meet. Names and
/// descriptions live in the i18n tables at the same index.
pub struct Achievement {
    pub id: &'static str,
    stat: fn(&Stats) -> u32,
    threshold: u32,
}

pub const ACHIEVEMENTS: [Achievement; 32] = [
    Achievement {
        id: "first_bank",
        stat: |s| s.banked,
        threshold: 1,
    },
    Achievement {
        id: "bank_100",
        stat: |s| s.banked,
        threshold: 100,
    },
    Achievement {
        id: "bank_1000",
        stat: |s| s.banked,
        threshold: 1000,
    },
    Achievement {
        id: "golden_1",
        stat: |s| s.golden,
        threshold: 1,
    },
    Achievement {
        id: "golden_10",
        stat: |s| s.golden,
        threshold: 10,
    },
    Achievement {
        id: "lure_5",
        stat: |s| s.lures,
        threshold: 5,
    },
    Achievement {
        id: "event_5",
        stat: |s| s.events,
        threshold: 5,
    },
    Achievement {
        id: "win_1",
        stat: |s| s.wins,
        threshold: 1,
    },
    Achievement {
        id: "win_10",
        stat: |s| s.wins,
        threshold: 10,
    },
    Achievement {
        id: "puzzle_5",
        stat: |s| s.puzzles,
        threshold: 5,
    },
    Achievement {
        id: "puzzle_25",
        stat: |s| s.puzzles,
        threshold: 25,
    },
    Achievement {
        // The campaign is far longer than the ladder used to allow for; a
        // player who works through it deserves a rung past the twenty-fifth.
        id: "puzzle_50",
        stat: |s| s.puzzles,
        threshold: 50,
    },
    Achievement {
        id: "dry_castle",
        stat: |s| s.dry_wins,
        threshold: 1,
    },
    Achievement {
        id: "giant_25",
        stat: |s| s.giants,
        threshold: 25,
    },
    Achievement {
        // Variety, not volume: one bit per event, all eight lit.
        id: "events_all",
        stat: |s| s.events_seen.count_ones(),
        threshold: TideEvent::ALL.len() as u32,
    },
    Achievement {
        id: "haul_50",
        stat: |s| s.best_round,
        threshold: 50,
    },
    Achievement {
        id: "series_1",
        stat: |s| s.series_wins,
        threshold: 1,
    },
    Achievement {
        id: "online_1",
        stat: |s| s.online_wins,
        threshold: 1,
    },
    Achievement {
        id: "daily_7",
        stat: |s| s.daily_days,
        threshold: 7,
    },
    Achievement {
        id: "built_1",
        stat: |s| s.levels_built,
        threshold: 1,
    },
    Achievement {
        id: "rounds_100",
        stat: |s| s.rounds,
        threshold: 100,
    },
    // The long tail: second rungs on ladders whose first rung a steady
    // player passes in an evening, plus the two that ask for a whole
    // campaign rather than a count of anything.
    Achievement {
        id: "bank_5000",
        stat: |s| s.banked,
        threshold: 5_000,
    },
    Achievement {
        id: "golden_50",
        stat: |s| s.golden,
        threshold: 50,
    },
    Achievement {
        id: "giant_100",
        stat: |s| s.giants,
        threshold: 100,
    },
    Achievement {
        id: "haul_100",
        stat: |s| s.best_round,
        threshold: 100,
    },
    Achievement {
        id: "win_50",
        stat: |s| s.wins,
        threshold: 50,
    },
    Achievement {
        id: "daily_30",
        stat: |s| s.daily_days,
        threshold: 30,
    },
    Achievement {
        id: "built_10",
        stat: |s| s.levels_built,
        threshold: 10,
    },
    Achievement {
        // The one trophy you earn by losing. It was unreachable in practice
        // until gulls started catching the crabs they walked into.
        id: "fed_25",
        stat: |s| s.gulls_fed,
        threshold: 25,
    },
    Achievement {
        id: "campaign_done",
        stat: |s| s.campaign_done,
        threshold: 1,
    },
    Achievement {
        id: "beach_done",
        stat: |s| s.beach_done,
        threshold: 1,
    },
    Achievement {
        // Thirty-two, so the shelf is two columns of sixteen rather than
        // sixteen and fifteen with a gap at the bottom of one of them.
        id: "series_5",
        stat: |s| s.series_wins,
        threshold: 5,
    },
];

impl Achievement {
    pub fn met(&self, stats: &Stats) -> bool {
        (self.stat)(stats) >= self.threshold
    }

    /// Progress toward the threshold, for the browser screen.
    pub fn progress(&self, stats: &Stats) -> (u32, u32) {
        ((self.stat)(stats).min(self.threshold), self.threshold)
    }
}

mod save;
mod track;
mod ui;

pub use save::load;
#[cfg(test)]
use save::{parse, to_text};
pub use track::{
    record_level_built, record_puzzle, record_round, reset_round_scratch, save_now, track_events,
};
pub use ui::{AchievementsUi, achievements_input, enter_achievements, update_toasts};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_round_trip_and_lenient_parse() {
        let stats = Stats {
            banked: 123,
            golden: 4,
            lures: 5,
            events: 6,
            gulls_fed: 7,
            raids_taken: 8,
            rounds: 9,
            wins: 3,
            puzzles: 11,
            dry_wins: 1,
            daily_day: 20_600,
            daily_best: 42,
            giants: 26,
            events_seen: 0b1010_1010,
            best_round: 51,
            series_wins: 2,
            online_wins: 4,
            daily_days: 7,
            levels_built: 3,
            campaign_done: 1,
            beach_done: 0,
            // Scratch: not persisted.
            raids_this_round: 99,
            banked_this_round: 77,
        };
        let mut unlocked = Unlocked::default();
        unlocked.0.insert("first_bank");
        unlocked.0.insert("golden_1");
        let (reparsed, re_unlocked) = parse(&to_text(&stats, &unlocked));
        assert_eq!(reparsed.banked, 123);
        assert_eq!(reparsed.wins, 3);
        assert_eq!(reparsed.raids_this_round, 0, "scratch resets");
        assert_eq!(reparsed.banked_this_round, 0, "scratch resets");
        assert_eq!(reparsed.daily_day, 20_600);
        assert_eq!(reparsed.daily_best, 42);
        assert_eq!(
            reparsed.campaign_done, 1,
            "a finished campaign stays finished"
        );
        assert_eq!(reparsed.beach_done, 0);
        // Everything a trophy reads has to survive the trip.
        assert_eq!(reparsed.giants, 26);
        assert_eq!(reparsed.events_seen, 0b1010_1010, "the roulette bitmask");
        assert_eq!(reparsed.best_round, 51);
        assert_eq!(reparsed.series_wins, 2);
        assert_eq!(reparsed.online_wins, 4);
        assert_eq!(reparsed.daily_days, 7);
        assert_eq!(reparsed.levels_built, 3);
        assert!(re_unlocked.0.contains("first_bank"));
        assert!(re_unlocked.0.contains("golden_1"));
        // Unknown ids and junk are dropped, junk values zero out.
        let (s, u) = parse("banked: nope\nunlocked: bogus,win_1\nwibble: 3\n");
        assert_eq!(s.banked, 0);
        assert!(u.0.contains("win_1") && u.0.len() == 1);
    }

    /// The trophy list and the string tables are parallel arrays indexed by
    /// position, so a new achievement added at the wrong end silently
    /// mislabels every trophy after it. Every language has to line up.
    #[test]
    fn every_achievement_is_named_in_every_language() {
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            assert_eq!(
                tr.ach_names.len(),
                ACHIEVEMENTS.len(),
                "{lang:?} names the wrong number of trophies"
            );
            for (index, achievement) in ACHIEVEMENTS.iter().enumerate() {
                assert!(
                    !tr.ach_names[index].is_empty() && !tr.ach_descs[index].is_empty(),
                    "{lang:?} leaves {} unlabelled",
                    achievement.id
                );
            }
        }
        // Ids are the save-file keys: a duplicate would make one trophy
        // unlockable by another's progress.
        let mut ids: Vec<&str> = ACHIEVEMENTS.iter().map(|a| a.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate achievement id");
        // Every trophy must be reachable: a zero threshold would unlock on
        // the first frame, and nothing is earned by doing nothing.
        for achievement in &ACHIEVEMENTS {
            assert!(achievement.threshold > 0, "{} is free", achievement.id);
            assert!(
                !achievement.met(&Stats::default()),
                "{} unlocks on a fresh save",
                achievement.id
            );
        }
    }

    #[test]
    fn thresholds_gate_unlocks() {
        let mut stats = Stats::default();
        let bank_100 = &ACHIEVEMENTS[1];
        assert!(!bank_100.met(&stats));
        stats.banked = 99;
        assert!(!bank_100.met(&stats));
        assert_eq!(bank_100.progress(&stats), (99, 100));
        stats.banked = 100;
        assert!(bank_100.met(&stats));
        let dry = ACHIEVEMENTS.iter().find(|a| a.id == "dry_castle").unwrap();
        stats.dry_wins = 1;
        assert!(dry.met(&stats));
    }
}
