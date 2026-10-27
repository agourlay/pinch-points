//! Persistence: achievements.txt in the XDG data directory, settings-style
//! lenient text.

use super::{ACHIEVEMENTS, Stats, Unlocked, save_path};
use bevy::prelude::*;

/// What a save file said that this build cannot read, kept so the next
/// save writes it back: a stat a newer build counts, and trophies it has
/// that this one does not. Read and dropped, one game in an older build
/// cost the player them for good.
#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct Unknown {
    /// Whole `key: value` lines under a key this build does not know.
    lines: crate::app::paths::ForeignLines,
    /// Trophy ids on the `unlocked:` line that name no trophy here.
    ids: crate::app::paths::ForeignLines,
}

pub(super) fn to_text(stats: &Stats, unlocked: &Unlocked, unknown: &Unknown) -> String {
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
    let Unknown {
        lines: unknown_lines,
        ids: unknown_ids,
    } = unknown;
    let mut ids: Vec<&str> = unlocked.0.iter().copied().collect();
    ids.sort_unstable();
    ids.extend(unknown_ids.iter());
    let mut text = format!(
        "banked: {}\ngolden: {}\nlures: {}\nevents: {}\ngulls_fed: {}\n\
         raids_taken: {}\nrounds: {}\nwins: {}\npuzzles: {}\ndry_wins: {}\n\
         daily_day: {}\ndaily_best: {}\ngiants: {}\nevents_seen: {}\n\
         best_round: {}\nseries_wins: {}\nonline_wins: {}\ndaily_days: {}\n\
         levels_built: {}\ncampaign_done: {}\nhosted: {}\n\
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
    );
    unknown_lines.append_to(&mut text);
    text
}

/// Lenient parse: bad values fall back to zero, and what this build does
/// not know (a key, a trophy id) is left out of the stats and the shelf.
pub fn parse(text: &str) -> (Stats, Unlocked) {
    let (stats, unlocked, _) = read(text);
    (stats, unlocked)
}

/// [`parse`], and what it left out kept aside ([`Unknown`]) for a save to
/// write back.
pub(super) fn read(text: &str) -> (Stats, Unlocked, Unknown) {
    let mut stats = Stats::default();
    let mut unlocked = Unlocked::default();
    let mut unknown = Unknown::default();
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
            "events_seen" => stats.events_seen = u16::try_from(num).unwrap_or(0),
            "best_round" => stats.best_round = num,
            "series_wins" => stats.series_wins = num,
            "online_wins" => stats.online_wins = num,
            "daily_days" => stats.daily_days = num,
            "levels_built" => stats.levels_built = num,
            "campaign_done" => stats.campaign_done = num,
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
                for id in value.split(',').map(str::trim).filter(|id| !id.is_empty()) {
                    match ACHIEVEMENTS.iter().find(|a| a.id == id) {
                        Some(known) => {
                            unlocked.0.insert(known.id);
                        }
                        None => unknown.ids.keep(id),
                    }
                }
            }
            "" => {}
            _ => unknown.lines.keep(line),
        }
    }
    // Today's best is by definition part of the all-time record, and a file
    // written before `daily_record` existed carries only the former. Without
    // this a returning player's standing daily score reads as zero, and the
    // backfill below would not see a trophy they had already earned.
    stats.daily_record = stats.daily_record.max(stats.daily_best);
    // Backfill: anything the stats already satisfy is earned, whether or
    // not the file remembers it. A trophy added after a player passed its
    // threshold would otherwise sit on the shelf reading "25/25" and
    // locked. No toast for these; they are not news.
    backfill(&stats, &mut unlocked);
    (stats, unlocked, unknown)
}

pub fn load(mut commands: Commands) {
    let (stats, unlocked) = std::fs::read_to_string(save_path())
        .map(|text| parse(&text))
        .unwrap_or_default();
    commands.insert_resource(stats);
    commands.init_resource::<super::RoundScratch>();
    commands.init_resource::<super::Unsaved>();
    commands.insert_resource(unlocked);
}

/// Write the record out, with whatever the file on disk holds that this
/// build cannot read carried over (see [`Unknown`]).
pub(super) fn save(stats: &mut Stats, unlocked: &mut Unlocked) {
    let _ = save_in(&save_path(), stats, unlocked);
}

/// [`save`] to a file named outright, with the error kept for a test.
///
/// The record is read once at startup and written back whole, so with two
/// copies of the game open each save wrote over the other's progress with
/// its own. The file as it stands is read back in first and merged into
/// the record in hand ([`merge`]): a trophy earned anywhere stays earned,
/// and a count never goes back down.
pub(super) fn save_in(
    path: &std::path::Path,
    stats: &mut Stats,
    unlocked: &mut Unlocked,
) -> std::io::Result<()> {
    let on_disk = std::fs::read_to_string(path).unwrap_or_default();
    let (disk_stats, disk_unlocked, unknown) = read(&on_disk);
    merge(stats, &disk_stats);
    unlocked.0.extend(disk_unlocked.0);
    backfill(stats, unlocked);
    crate::app::paths::write_atomic(path, to_text(stats, unlocked, &unknown))
}

/// Take in another record's stats: the larger of each count, since every
/// count only climbs, and both sets of bits in a bitmask. The daily best
/// belongs to its day, so the later day's stands, and the better of the
/// two on the same day.
///
/// Two copies counting at once each count from where they started, so the
/// larger is not the sum of both; it is what neither copy can be said to
/// have taken back.
fn merge(stats: &mut Stats, other: &Stats) {
    // Destructured with no rest pattern, as in `to_text`: a new stat has
    // to say how it merges.
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
        hosted,
        crowd,
        seats_won,
        under_par,
        clean_solves,
        deep_solves,
        daily_record,
        codes_shared,
        codes_taken,
    } = other;
    for (mine, theirs) in [
        (&mut stats.banked, banked),
        (&mut stats.golden, golden),
        (&mut stats.lures, lures),
        (&mut stats.events, events),
        (&mut stats.gulls_fed, gulls_fed),
        (&mut stats.raids_taken, raids_taken),
        (&mut stats.rounds, rounds),
        (&mut stats.wins, wins),
        (&mut stats.puzzles, puzzles),
        (&mut stats.dry_wins, dry_wins),
        (&mut stats.giants, giants),
        (&mut stats.best_round, best_round),
        (&mut stats.series_wins, series_wins),
        (&mut stats.online_wins, online_wins),
        (&mut stats.daily_days, daily_days),
        (&mut stats.levels_built, levels_built),
        (&mut stats.campaign_done, campaign_done),
        (&mut stats.hosted, hosted),
        (&mut stats.crowd, crowd),
        (&mut stats.under_par, under_par),
        (&mut stats.clean_solves, clean_solves),
        (&mut stats.deep_solves, deep_solves),
        (&mut stats.daily_record, daily_record),
        (&mut stats.codes_shared, codes_shared),
        (&mut stats.codes_taken, codes_taken),
    ] {
        *mine = (*mine).max(*theirs);
    }
    stats.events_seen |= events_seen;
    stats.seats_won |= seats_won;
    match (*daily_day).cmp(&stats.daily_day) {
        std::cmp::Ordering::Greater => {
            stats.daily_day = *daily_day;
            stats.daily_best = *daily_best;
        }
        std::cmp::Ordering::Equal => stats.daily_best = stats.daily_best.max(*daily_best),
        std::cmp::Ordering::Less => {}
    }
}

/// Light every trophy the stats already meet, without a toast: see the
/// end of [`read`].
fn backfill(stats: &Stats, unlocked: &mut Unlocked) {
    for achievement in &ACHIEVEMENTS {
        if achievement.met(stats) {
            unlocked.0.insert(achievement.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything a player has ever done survives the trip to disk.
    ///
    /// `to_text` destructures `Stats` with no rest pattern, so a new field
    /// cannot be added without deciding whether it is saved, but nothing
    /// forces `parse` to read it back and an unknown key is skipped in
    /// silence. A stat written and never read comes back as zero, and the
    /// only symptom is a lifetime total that resets between runs.
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
            // At least `daily_best`, deliberately: `parse` lifts today's
            // best into the all-time record, so a fixture with a record
            // below today's comes back *corrected* and this test would be
            // reading that migration instead of the round trip.
            daily_record: 501,
            ..Stats::default()
        };
        let mut unlocked = Unlocked::default();
        for trophy in ACHIEVEMENTS.iter().take(5) {
            unlocked.0.insert(trophy.id);
        }

        let (back, earned) = parse(&to_text(&stats, &unlocked, &Unknown::default()));
        assert_eq!(back, stats, "a stat went out and did not come back");
        // Nothing *lost*, rather than nothing gained: the shelf is derived
        // from the stats as well as read from the file, so a lifetime this
        // long lights more trophies on the way in than were written out.
        // That is the feature, so the count is not the thing to assert.
        for trophy in ACHIEVEMENTS.iter().take(5) {
            assert!(
                earned.0.contains(trophy.id),
                "{} was earned, saved, and came home unearned",
                trophy.id
            );
        }
    }

    /// A save file written by a newer build has to be readable by this one.
    /// Trophies are named by id, and a version that added one writes a name
    /// this build has never heard of: refusing the file would wipe a
    /// player's shelf, and taking it as earned would light a trophy that
    /// does not exist here.
    #[test]
    fn a_trophy_from_the_future_is_ignored_rather_than_believed() {
        let stats = Stats {
            banked: 12,
            ..Stats::default()
        };
        let mut text = to_text(&stats, &Unlocked::default(), &Unknown::default());
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

    /// And not lit is not lost: a save from this build writes back the
    /// trophy and the stat it could not read, so the newer build finds
    /// its shelf as it left it.
    #[test]
    fn a_trophy_from_the_future_is_kept_for_the_future() {
        let dir = crate::app::paths::test_dir("trophies");
        let path = dir.join("achievements.txt");
        let stats = Stats {
            banked: 12,
            ..Stats::default()
        };
        let mut text = to_text(&stats, &Unlocked::default(), &Unknown::default());
        text.push_str("unlocked: a_trophy_from_a_later_version\n");
        text.push_str("some_stat_nobody_here_knows: 9\n");
        crate::app::paths::write_atomic(&path, &text).expect("written");

        let (mut stats, mut unlocked) = parse(&text);
        stats.banked += 1;
        save_in(&path, &mut stats, &mut unlocked).expect("saved");
        let back = std::fs::read_to_string(&path).expect("read back");
        let (_, _, unknown) = read(&back);
        let ids: Vec<&str> = unknown.ids.iter().collect();
        assert_eq!(ids, ["a_trophy_from_a_later_version"], "{back}");
        let lines: Vec<&str> = unknown.lines.iter().collect();
        assert_eq!(lines, ["some_stat_nobody_here_knows: 9"], "{back}");
        assert_eq!(parse(&back).0.banked, 13, "and this build's own news");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A file from a build that had the score-attack challenges still
    /// loads: their trophies and counts name nothing here, so they are
    /// neither lit nor lost, and the next save writes them back as they
    /// stood, the convention for anything this build cannot read.
    #[test]
    fn the_challenge_trophies_of_an_older_build_are_kept_unlit() {
        let dir = crate::app::paths::test_dir("trophies-challenges");
        let path = dir.join("achievements.txt");
        let text = "banked: 7\nbeach_done: 1\nchallenge_golds: 8\nunder_par: 2\n\
                    unlocked: beach_done,gold_1,gold_all,under_par_1,under_par_10\n";
        crate::app::paths::write_atomic(&path, text).expect("written");
        let (mut stats, mut unlocked) = parse(text);
        assert_eq!((stats.banked, stats.under_par), (7, 2));
        let mut lit: Vec<&str> = unlocked.0.iter().copied().collect();
        lit.sort_unstable();
        assert_eq!(lit, ["first_bank", "under_par_1"], "only what is here");
        save_in(&path, &mut stats, &mut unlocked).expect("saved");
        let back = std::fs::read_to_string(&path).expect("read back");
        let (_, _, unknown) = read(&back);
        let ids: Vec<&str> = unknown.ids.iter().collect();
        assert_eq!(ids, ["beach_done", "gold_1", "gold_all", "under_par_10"]);
        let lines: Vec<&str> = unknown.lines.iter().collect();
        assert_eq!(lines, ["beach_done: 1", "challenge_golds: 8"], "{back}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Two copies of the game open at once: a save from one keeps what the
    /// other has written since, a trophy and a count alike, and learns of
    /// them itself.
    #[test]
    fn a_save_keeps_what_another_copy_wrote() {
        let dir = crate::app::paths::test_dir("trophies-two");
        let path = dir.join("achievements.txt");
        let (mut first, mut first_shelf) = (Stats::default(), Unlocked::default());
        let (mut second, mut second_shelf) = (Stats::default(), Unlocked::default());
        second.banked = 40;
        second.events_seen = 0b01;
        second.daily_day = 9;
        second.daily_best = 30;
        let trophy = ACHIEVEMENTS[0].id;
        second_shelf.0.insert(trophy);
        save_in(&path, &mut second, &mut second_shelf).expect("saved");

        first.golden = 3;
        first.banked = 5;
        first.events_seen = 0b10;
        first.daily_day = 8;
        first.daily_best = 99;
        save_in(&path, &mut first, &mut first_shelf).expect("saved");

        let (back, shelf) = parse(&std::fs::read_to_string(&path).expect("read"));
        assert_eq!((back.banked, back.golden), (40, 3), "neither taken back");
        assert_eq!(back.events_seen, 0b11, "both events seen");
        assert_eq!((back.daily_day, back.daily_best), (9, 30), "the later day");
        assert!(shelf.0.contains(trophy), "the other copy's trophy");
        assert_eq!(first.banked, 40, "and the saver knows it now");
        assert!(first_shelf.0.contains(trophy));
        let _ = std::fs::remove_dir_all(dir);
    }
}
