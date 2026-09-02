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
    /// Series taken, of either length.
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
    /// Rounds hosted on the local network, counted per round rather than
    /// per match because that is where a round is tallied. The busy-LAN
    /// case is the point of the game, and somebody has to be the one who
    /// opens the beach.
    pub hosted: u32,
    /// The most seats a round the local player was in has ever filled.
    pub crowd: u32,
    /// One bit per seat the local player has won a round from, so the
    /// trophy asks for the whole table rather than four wins from seat 0.
    pub seats_won: u8,
    /// Stages cleared with signposts still in hand.
    pub under_par: u32,
    /// Stages cleared on the first attempt, with no retry in between.
    pub clean_solves: u32,
    /// Stages granting five signposts that have been cleared: the deep end
    /// of the campaign, which is a different thing from a count of solves.
    pub deep_solves: u32,
    /// Best daily score ever, as against `daily_best`, which is today's and
    /// starts over at midnight. A trophy cannot hang on a number that
    /// resets under it.
    pub daily_record: u32,
    /// Levels put on the clipboard as a share code.
    pub codes_shared: u32,
    /// Levels opened from somebody else's share code.
    pub codes_taken: u32,
}

/// What this round has done to the local seat so far: read when the round
/// ends, then thrown away. Its own resource rather than two fields in
/// [`Stats`], so that the save file is exactly `Stats` and nothing has to
/// remember to leave these out of it, or to zero them on the way in.
#[derive(Resource, Default, Debug)]
pub struct RoundScratch {
    /// Raids on the local castle this round.
    pub raids: u32,
    /// Crabs banked this round; feeds `best_round`.
    pub banked: u32,
}

/// Which stage the player is on and how many times they have reloaded it.
///
/// The first-try trophy is the only thing that needs it, and it cannot be
/// a stat: a retry is not a lifetime total but a fact about the attempt in
/// progress, and it has to be forgotten on the way out of the screen.
#[derive(Resource, Default)]
pub struct PuzzleAttempt {
    /// The stage the count belongs to. Cleared on entering the screen, so
    /// coming back to a stage starts a fresh attempt rather than inheriting
    /// the retries of the last visit.
    stage: String,
    pub retries: u32,
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

pub const ACHIEVEMENTS: [Achievement; 50] = [
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
        id: "series_5",
        stat: |s| s.series_wins,
        threshold: 5,
    },
    // Second rungs on ladders that had only a first, and the two counters
    // the shelf never asked anything of. Nothing here needs new tracking:
    // every one of these numbers was already being kept.
    Achievement {
        id: "raids_25",
        stat: |s| s.raids_taken,
        threshold: 25,
    },
    Achievement {
        id: "lure_50",
        stat: |s| s.lures,
        threshold: 50,
    },
    Achievement {
        id: "event_50",
        stat: |s| s.events,
        threshold: 50,
    },
    Achievement {
        id: "rounds_500",
        stat: |s| s.rounds,
        threshold: 500,
    },
    Achievement {
        id: "puzzle_100",
        stat: |s| s.puzzles,
        threshold: 100,
    },
    Achievement {
        id: "built_25",
        stat: |s| s.levels_built,
        threshold: 25,
    },
    // The table: who you played with, how many of you there were, and
    // whether you have sat everywhere.
    Achievement {
        id: "online_10",
        stat: |s| s.online_wins,
        threshold: 10,
    },
    Achievement {
        id: "hosted_1",
        stat: |s| s.hosted,
        threshold: 1,
    },
    Achievement {
        id: "crowd_4",
        stat: |s| s.crowd,
        threshold: 4,
    },
    Achievement {
        // Variety again, like the roulette: four bits, not four wins.
        id: "all_seats",
        stat: |s| s.seats_won.count_ones(),
        threshold: 4,
    },
    // Craft: the campaign asks to be finished, these ask how.
    Achievement {
        id: "under_par_1",
        stat: |s| s.under_par,
        threshold: 1,
    },
    Achievement {
        id: "clean_10",
        stat: |s| s.clean_solves,
        threshold: 10,
    },
    Achievement {
        id: "deep_1",
        stat: |s| s.deep_solves,
        threshold: 1,
    },
    Achievement {
        // Against the all-time record, not today's best: a trophy hung on
        // `daily_best` would come and go with the date.
        id: "daily_40",
        stat: |s| s.daily_record,
        threshold: 40,
    },
    // Both ends of sharing a level are worth a trophy.
    Achievement {
        id: "shared_1",
        stat: |s| s.codes_shared,
        threshold: 1,
    },
    Achievement {
        id: "taken_1",
        stat: |s| s.codes_taken,
        threshold: 1,
    },
    // Second rungs on the two of the new ladders that had somewhere left to
    // go: the widest table the seat count allows, and thrift as a habit
    // rather than a one-off.
    Achievement {
        id: "crowd_6",
        stat: |s| s.crowd,
        threshold: crate::sim::MAX_PLAYERS as u32,
    },
    Achievement {
        id: "under_par_10",
        stat: |s| s.under_par,
        threshold: 10,
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
    record_codes, record_level_built, record_puzzle, record_round, reset_puzzle_attempt,
    reset_round_scratch, save_now, track_events, track_puzzle_attempt,
};
pub use ui::{
    AchievementsUi, achievements_input, enter_achievements, update_shelf_scrollbar, update_toasts,
};

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
            hosted: 2,
            crowd: 4,
            seats_won: 0b0000_1011,
            under_par: 3,
            clean_solves: 12,
            deep_solves: 1,
            daily_record: 41,
            codes_shared: 5,
            codes_taken: 6,
        };
        let mut unlocked = Unlocked::default();
        unlocked.0.insert("first_bank");
        unlocked.0.insert("golden_1");
        let (reparsed, re_unlocked) = parse(&to_text(&stats, &unlocked));
        assert_eq!(reparsed.banked, 123);
        assert_eq!(reparsed.wins, 3);
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
        assert_eq!(reparsed.hosted, 2);
        assert_eq!(reparsed.crowd, 4);
        assert_eq!(reparsed.seats_won, 0b0000_1011, "the seats-sat-at bitmask");
        assert_eq!(reparsed.under_par, 3);
        assert_eq!(reparsed.clean_solves, 12);
        assert_eq!(reparsed.deep_solves, 1);
        // Today's best (42) is higher than the record the file carried (41),
        // so the parse lifts it: a save written before `daily_record`
        // existed carries no record at all, and its owner has still earned
        // whatever they scored today.
        assert_eq!(reparsed.daily_record, 42, "today's best counts as record");
        let (old_save, _) = parse("daily_best: 55\n");
        assert_eq!(
            old_save.daily_record, 55,
            "a save from before the record existed"
        );
        assert_eq!(reparsed.codes_shared, 5);
        assert_eq!(reparsed.codes_taken, 6);
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

    /// The description column is what is left of a 580px row once the mark,
    /// the fixed name column, the gaps and the progress count have taken
    /// theirs, and it clips like the name column does. Deliberately, so a
    /// long description never pushes the count off the end of the row; but
    /// a description clipped mid-word still reads as a bug, and nothing
    /// short of measuring it says which ones are.
    #[test]
    fn every_trophy_description_fits_what_is_left_of_its_row() {
        use crate::app::i18n::metrics::text_px;
        // `ui::spawn_trophy`'s own numbers.
        const ROW_PX: f32 = 580.0;
        const PADDING_PX: f32 = 8.0 * 2.0;
        const MARK_PX: f32 = 14.0;
        const NAME_PX: f32 = 204.0;
        const GAPS_PX: f32 = 8.0 * 3.0;
        let fine = crate::app::menu_ui::type_scale::FINE;
        let mut over = Vec::new();
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            for (index, achievement) in ACHIEVEMENTS.iter().enumerate() {
                // The count column takes its natural width, so the room the
                // description gets depends on this trophy's own numbers.
                let (_, goal) = achievement.progress(&Stats::default());
                let count = text_px(&format!("{goal}/{goal}"), fine);
                let room = ROW_PX - PADDING_PX - MARK_PX - NAME_PX - GAPS_PX - count;
                let desc = tr.ach_descs[index];
                let width = text_px(desc, fine);
                if width > room {
                    over.push(format!(
                        "{lang:?} {} {desc:?} {width:.0}px into {room:.0}px",
                        achievement.id
                    ));
                }
            }
        }
        assert!(over.is_empty(), "clipped:\n{}", over.join("\n"));
    }

    /// Two trophies with the same name is a shelf you cannot read: the row
    /// says you have done a thing you have not, and the toast that pops on
    /// unlocking names the wrong deed. Caught per language, because a
    /// translation can collapse a distinction English keeps.
    #[test]
    fn no_two_trophies_share_a_name() {
        let mut clashes = Vec::new();
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            let mut seen: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
            for (index, achievement) in ACHIEVEMENTS.iter().enumerate() {
                let name = tr.ach_names[index];
                if let Some(first) = seen.insert(name, achievement.id) {
                    clashes.push(format!("{lang:?} {name:?}: {first} and {}", achievement.id));
                }
            }
        }
        assert!(clashes.is_empty(), "shared names:\n{}", clashes.join("\n"));
    }

    /// The name column is a fixed 204px with `clip_x`, so a name too long
    /// for it loses its tail with nothing anywhere to say so. Measured in
    /// pixels rather than characters: Japanese draws about two-thirds wider
    /// per character than Latin, so a character count passes the very
    /// language most likely to overrun.
    #[test]
    fn every_trophy_name_fits_its_column() {
        // The row's own numbers, from `ui::spawn_trophy`.
        const NAME_COLUMN_PX: f32 = 204.0;
        let size = crate::app::menu_ui::type_scale::BODY;
        let mut over = Vec::new();
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            for (index, achievement) in ACHIEVEMENTS.iter().enumerate() {
                let name = tr.ach_names[index];
                let width = crate::app::i18n::metrics::text_px(name, size);
                if width > NAME_COLUMN_PX {
                    over.push(format!("{lang:?} {} {name:?} {width:.0}px", achievement.id));
                }
            }
        }
        assert!(
            over.is_empty(),
            "past a {NAME_COLUMN_PX:.0}px column:\n{}",
            over.join("\n")
        );
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
