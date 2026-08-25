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
