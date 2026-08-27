//! Persistence: achievements.txt in the XDG data directory, settings-style
//! lenient text.

use super::{ACHIEVEMENTS, Stats, Unlocked, save_path};
use bevy::prelude::*;

pub fn to_text(stats: &Stats, unlocked: &Unlocked) -> String {
    // Destructured with no rest pattern: a new stat refuses to build here
    // until somebody decides whether it is saved or, like the round-scoped
    // scratch, deliberately left behind. The lenient `parse` below would
    // never notice one going unsaved.
    let Stats {
        banked,
        golden,
        lures,
        events,
        gulls_fed,
        raids_taken,
        rounds,
        wins,
        puzzles,
        dry_wins,
        daily_day,
        daily_best,
        giants,
        events_seen,
        best_round,
        series_wins,
        online_wins,
        daily_days,
        levels_built,
        campaign_done,
        beach_done,
        hosted,
        crowd,
        seats_won,
        under_par,
        clean_solves,
        deep_solves,
        daily_record,
        codes_shared,
        codes_taken,
    } = stats;
    let mut ids: Vec<&str> = unlocked.0.iter().copied().collect();
    ids.sort_unstable();
    format!(
        "banked: {}\ngolden: {}\nlures: {}\nevents: {}\ngulls_fed: {}\n\
         raids_taken: {}\nrounds: {}\nwins: {}\npuzzles: {}\ndry_wins: {}\n\
         daily_day: {}\ndaily_best: {}\ngiants: {}\nevents_seen: {}\n\
         best_round: {}\nseries_wins: {}\nonline_wins: {}\ndaily_days: {}\n\
         levels_built: {}\ncampaign_done: {}\nbeach_done: {}\nhosted: {}\n\
         crowd: {}\nseats_won: {}\nunder_par: {}\nclean_solves: {}\n\
         deep_solves: {}\ndaily_record: {}\ncodes_shared: {}\n\
         codes_taken: {}\nunlocked: {}\n",
        banked,
        golden,
        lures,
        events,
        gulls_fed,
        raids_taken,
        rounds,
        wins,
        puzzles,
        dry_wins,
        daily_day,
        daily_best,
        giants,
        events_seen,
        best_round,
        series_wins,
        online_wins,
        daily_days,
        levels_built,
        campaign_done,
        beach_done,
        hosted,
        crowd,
        seats_won,
        under_par,
        clean_solves,
        deep_solves,
        daily_record,
        codes_shared,
        codes_taken,
        ids.join(","),
    )
}

/// Lenient parse: unknown keys and bad values fall back to zero, unknown
/// achievement ids are dropped.
pub fn parse(text: &str) -> (Stats, Unlocked) {
    let mut stats = Stats::default();
    let mut unlocked = Unlocked::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let num = value.parse::<u32>().unwrap_or(0);
        match key.trim() {
            "banked" => stats.banked = num,
            "golden" => stats.golden = num,
            "lures" => stats.lures = num,
            "events" => stats.events = num,
            "gulls_fed" => stats.gulls_fed = num,
            "raids_taken" => stats.raids_taken = num,
            "rounds" => stats.rounds = num,
            "wins" => stats.wins = num,
            "puzzles" => stats.puzzles = num,
            "dry_wins" => stats.dry_wins = num,
            "daily_day" => stats.daily_day = num,
            "daily_best" => stats.daily_best = num,
            "giants" => stats.giants = num,
            // A bitmask, so anything past a byte is not ours.
            "events_seen" => stats.events_seen = u8::try_from(num).unwrap_or(0),
            "best_round" => stats.best_round = num,
            "series_wins" => stats.series_wins = num,
            "online_wins" => stats.online_wins = num,
            "daily_days" => stats.daily_days = num,
            "levels_built" => stats.levels_built = num,
            "campaign_done" => stats.campaign_done = num,
            "beach_done" => stats.beach_done = num,
            "hosted" => stats.hosted = num,
            "crowd" => stats.crowd = num,
            // A bitmask, so anything past a byte is not ours.
            "seats_won" => stats.seats_won = u8::try_from(num).unwrap_or(0),
            "under_par" => stats.under_par = num,
            "clean_solves" => stats.clean_solves = num,
            "deep_solves" => stats.deep_solves = num,
            "daily_record" => stats.daily_record = num,
            "codes_shared" => stats.codes_shared = num,
            "codes_taken" => stats.codes_taken = num,
            "unlocked" => {
                for id in value.split(',') {
                    if let Some(known) = ACHIEVEMENTS.iter().find(|a| a.id == id.trim()) {
                        unlocked.0.insert(known.id);
                    }
                }
            }
            _ => {}
        }
    }
    // Today's best is by definition part of the all-time record, and a file
    // written before `daily_record` existed carries only the former. Without
    // this a returning player's standing daily score reads as zero, and the
    // backfill below would not see a trophy they had already earned.
    stats.daily_record = stats.daily_record.max(stats.daily_best);
    // Backfill: anything the stats already satisfy is earned, whether or not
    // the file remembers it. A trophy added after a player passed its
    // threshold would otherwise sit on the shelf reading "25/25" and locked,
    // waiting for one more of something they had already done twenty-five
    // times. No toast for these; they are not news.
    for achievement in &ACHIEVEMENTS {
        if achievement.met(&stats) {
            unlocked.0.insert(achievement.id);
        }
    }
    (stats, unlocked)
}

pub fn load(mut commands: Commands) {
    let (stats, unlocked) = std::fs::read_to_string(save_path())
        .map(|text| parse(&text))
        .unwrap_or_default();
    commands.insert_resource(stats);
    commands.init_resource::<super::RoundScratch>();
    commands.insert_resource(unlocked);
}

pub(super) fn save(stats: &Stats, unlocked: &Unlocked) {
    let _ = crate::app::paths::write_atomic(&save_path(), to_text(stats, unlocked));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything a player has ever done survives the trip to disk.
    ///
    /// `to_text` destructures `Stats` with no rest pattern, so a new field
    /// cannot be added without deciding whether it is saved - but nothing
    /// forces `parse` to be taught to read it back, and the parser is
    /// lenient by design: an unknown key is skipped in silence. So a stat
    /// written and never read comes back as zero, and the only symptom is
    /// a lifetime total that quietly resets between runs.
    #[test]
    fn a_lifetime_of_play_survives_the_trip_to_disk() {
        // Every field distinct and non-default, so a parser that dropped
        // one, or crossed two over, cannot come back matching by luck.
        let stats = Stats {
            banked: 4_211,
            golden: 37,
            lures: 12,
            events: 91,
            gulls_fed: 8,
            raids_taken: 54,
            rounds: 140,
            wins: 61,
            puzzles: 73,
            dry_wins: 5,
            daily_day: 20_412,
            daily_best: 88,
            giants: 19,
            events_seen: 0b1010_1101,
            best_round: 233,
            series_wins: 7,
            online_wins: 26,
            daily_days: 44,
            // At least `daily_best`, and deliberately so: `parse` lifts
            // today's best into the all-time record, so a save written
            // before that field existed does not lose it. A fixture with a
            // record below today's would come back *corrected* rather than
            // unchanged, and this test would be reading that migration
            // instead of the round trip.
            daily_record: 501,
            ..Stats::default()
        };
        let mut unlocked = Unlocked::default();
        for trophy in ACHIEVEMENTS.iter().take(5) {
            unlocked.0.insert(trophy.id);
        }

        let (back, earned) = parse(&to_text(&stats, &unlocked));
        assert_eq!(back, stats, "a stat went out and did not come back");
        // Nothing *lost*, rather than nothing gained: the shelf is derived
        // from the stats as well as read from the file, so a lifetime this
        // long lights more trophies on the way in than were written out.
        // That is the feature - a save from before a trophy existed earns
        // it on load - so the count is not the thing to assert.
        for trophy in ACHIEVEMENTS.iter().take(5) {
            assert!(
                earned.0.contains(trophy.id),
                "{} was earned, saved, and came home unearned",
                trophy.id
            );
        }
    }

    /// A save file written by a newer build has to be readable by this
    /// one. Trophies are named by id, and a version that added one writes
    /// a name this build has never heard of: refusing the file, or taking
    /// the unknown name as earned, would either wipe a player's shelf or
    /// light a trophy that does not exist here.
    #[test]
    fn a_trophy_from_the_future_is_ignored_rather_than_believed() {
        let stats = Stats {
            banked: 12,
            ..Stats::default()
        };
        let mut text = to_text(&stats, &Unlocked::default());
        text.push_str("unlocked: a_trophy_from_a_later_version\n");
        text.push_str("some_stat_nobody_here_knows: 9\n");

        let (back, earned) = parse(&text);
        assert_eq!(back.banked, 12, "a stat it did know was lost with the rest");
        assert!(
            earned
                .0
                .iter()
                .all(|id| ACHIEVEMENTS.iter().any(|a| a.id == *id)),
            "a trophy this build has never heard of was lit: {:?}",
            earned.0
        );
    }
}
