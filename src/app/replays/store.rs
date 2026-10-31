//! The kept rounds as files on disk: where they live, what they are named,
//! what each one says when the shelf is listed, and pruning and deleting.

use super::*;

/// Where the library lives. `last.txt` sits beside it, the newest round
/// whether it was kept or not, because the `PINCH_REPLAY` dev hook reaches
/// for it by name.
pub fn library_dir() -> std::path::PathBuf {
    crate::app::paths::data_dir().join("replays")
}

/// One kept round: the file, and what to call it on screen.
pub struct Kept {
    pub path: std::path::PathBuf,
    /// When it was played, `dd/mm hh:mm`; for a file not named the way the
    /// shelf names one, the file's own name.
    pub when: String,
    /// Who took it, as the file is named: empty for a draw, whose word is
    /// the player's language's and is added as the row is drawn, and for a
    /// file not named the shelf's way.
    pub winner: String,
    /// Nobody took it. A flag rather than a word in the label, so a player
    /// who calls themselves "draw" is still a winner.
    pub draw: bool,
    /// How many played, read off the round's beach; `None` for a file that
    /// does not read as a round.
    pub seats: Option<u8>,
    /// What took it, when that was not a person: see [`won_by`].
    pub won_by: Option<WonBy>,
    /// What it was won with ([`crate::app::teams::top_total`]), or the
    /// score a draw tied on, from the final scores the recording keeps;
    /// `None` for a recording made before scores were kept, and for a
    /// file not named the shelf's way, whose winner the row cannot say.
    pub points: Option<u32>,
}

/// What held the seat, or every seat of the team, that took a kept round,
/// when it was not a person.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WonBy {
    /// The game's AI, at the level the recording names, or `None` for a
    /// recording made before levels were kept (or a team whose AI seats
    /// played at different levels).
    Ai(Option<BotLevel>),
    /// A bot over the protocol.
    Bot,
}

/// The file name for a round finished now, by who won it, or `None` for
/// a draw.
///
/// Named from the wall clock rather than a counter: a counter needs the
/// directory read to know what is next, and two copies of the game would
/// disagree about it. Seconds since the epoch sort correctly as text for the
/// next few hundred years.
///
/// A draw is the name with no winner part at all, which no winner's name
/// can produce: even an empty name leaves its `_`. It used to be filed under
/// the word "draw", and a player who had called themselves that was read
/// back as nobody.
pub fn file_name(stamp: u64, winner: Option<&str>) -> String {
    match winner {
        Some(winner) => format!("round_{stamp:010}_{}.txt", tidy(winner)),
        None => format!("round_{stamp:010}.txt"),
    }
}

/// A winner's name as a file name spells it: letters and digits kept,
/// anything else `_`, and no longer than twelve. What [`won_by`] matches
/// a seat's name against.
fn tidy(winner: &str) -> String {
    winner
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .take(12)
        .collect()
}

/// Where in `dir` to file a round finished at `stamp`: [`file_name`], or,
/// when that is taken, the same at the next free second.
///
/// Two rounds with one winner in one second are one file name, and the
/// second write replaced the first: two codes pasted in quick succession,
/// say. A second later on the label costs nothing, since the shelf shows
/// minutes, and the order stays the order they were filed in.
pub fn free_path(dir: &std::path::Path, stamp: u64, winner: Option<&str>) -> std::path::PathBuf {
    (stamp..)
        .map(|at| dir.join(file_name(at, winner)))
        .find(|path| !path.exists())
        .unwrap_or_else(|| dir.join(file_name(stamp, winner)))
}

/// Drop the oldest rounds until at most `cap` are kept.
///
/// Called after a round is filed, the one moment the shelf can have grown.
/// Best-effort like every other write here: a file that will not delete
/// stays on the shelf, and the next round tries again.
///
/// Only rounds are counted and only rounds are removed. `last.txt` and the
/// highlight reel live in the same directory and belong to the menu, not to
/// the shelf.
pub fn prune(cap: u8) {
    prune_in(&library_dir(), cap);
}

/// [`prune`] on a directory named outright, so a test can hand it a
/// scratch folder instead of the player's library.
pub fn prune_in(dir: &std::path::Path, cap: u8) {
    drop_oldest(listed_in(dir), usize::from(cap));
}

/// [`prune_in`] after filing the round at `kept`, which stays whatever its
/// name says: the oldest *other* rounds go until the shelf is down to
/// `cap`. A name carries the clock's stamp, and a machine whose clock is
/// behind (a flat battery, the files copied in from elsewhere) files the
/// new round under the oldest name, which was the one pruned while the
/// card said it was kept.
pub fn prune_sparing(dir: &std::path::Path, cap: u8, kept: &std::path::Path) {
    let others: Vec<_> = listed_in(dir)
        .into_iter()
        .filter(|path| path != kept)
        .collect();
    drop_oldest(others, usize::from(cap).saturating_sub(1));
}

/// Delete all of `listed`, newest first, past its first `keep`.
fn drop_oldest(listed: Vec<std::path::PathBuf>, keep: usize) {
    for old in listed.iter().skip(keep) {
        if let Err(e) = std::fs::remove_file(old) {
            warn!("could not drop the oldest round: {e}");
        }
    }
}

/// The "Rounds kept" setting turned from `was` to `now`, applied to the
/// shelf in `dir` when the setting is saved rather than at the next round
/// filed: a player who asked for fewer sees fewer. Only a lower cap
/// prunes, newest kept as [`prune_in`] keeps them; a raised one deletes
/// nothing, even on a shelf already over it (rounds copied in by hand).
pub fn cap_changed_in(dir: &std::path::Path, was: u8, now: u8) {
    if now < was {
        prune_in(dir, now);
    }
}

/// Delete the round at `path` from the shelf in `dir`.
///
/// Only a round on the shelf goes: the path is looked up in [`shelf_in`]
/// first, so `last.txt`, the highlight reel or anything else sharing the
/// folder cannot be removed through here, whatever path is handed in.
pub fn delete_in(dir: &std::path::Path, path: &std::path::Path) -> std::io::Result<()> {
    if !listed_in(dir).iter().any(|kept| kept == path) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "not a kept round",
        ));
    }
    std::fs::remove_file(path)
}

/// Everything on the shelf in `dir`, newest first. The directory is named
/// outright, for the same reason as [`prune_in`]: the data directory comes
/// from the process environment, and a test that changes that races every
/// other test in the binary.
///
/// Each round's file is read here, once, for its head (see [`kept`]), and
/// the rows are drawn from what that left in [`Kept`]: the screen redraws
/// every frame, and a shelf holds up to 99 rounds.
pub fn shelf_in(dir: &std::path::Path) -> Vec<Kept> {
    shelf_read(dir, |path| std::fs::read_to_string(path).ok())
}

/// [`shelf_in`] with the reading handed in, so a test can count it.
pub(super) fn shelf_read(
    dir: &std::path::Path,
    mut read: impl FnMut(&std::path::Path) -> Option<String>,
) -> Vec<Kept> {
    listed_in(dir)
        .into_iter()
        .map(|path| {
            let text = read(&path);
            kept(path, text.as_deref())
        })
        .collect()
}

/// The kept rounds' files in `dir`, newest first, by their names alone:
/// what pruning and deleting go by, which need no file opened.
fn listed_in(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Ok(dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut kept: Vec<std::path::PathBuf> = dir
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "txt")
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("round_"))
        })
        .collect();
    // Newest first: the names sort by timestamp, so this is just a reverse.
    kept.sort_by(|a, b| b.cmp(a));
    kept
}

/// A round on the shelf, from its file's name and `text`, the file's
/// contents, or `None` when it would not read.
///
/// Only the head of the recording is parsed ([`Replay::head`]): who sat
/// where and on which beach. The inputs are never decoded and the round
/// never played, so a full shelf lists in a blink.
pub(super) fn kept(path: std::path::PathBuf, text: Option<&str>) -> Kept {
    let named = name_of(&path);
    let head = text.and_then(|text| Replay::head(text).ok());
    let seats = head
        .as_ref()
        .map(|head| crate::app::session::replay_seats(&head.level.board()));
    let won_by = match (&head, seats, &named.filed) {
        (Some(head), Some(seats), Some(filed)) => won_by(head, seats, filed),
        _ => None,
    };
    let points = match (&head, seats) {
        (Some(head), Some(seats)) if named.filed.is_some() || named.draw => {
            head.scores.map(|scores| {
                let mode = crate::app::teams::recorded(head).played_by(seats);
                crate::app::teams::top_total(&scores, seats, mode)
            })
        }
        _ => None,
    };
    Kept {
        path,
        when: named.when,
        winner: named.filed.as_deref().map(shown).unwrap_or_default(),
        draw: named.draw,
        seats,
        won_by,
        points,
    }
}

/// What a kept round's file name says.
struct Named {
    /// When, or the file's own name when it is not named the shelf's way.
    when: String,
    /// The winner's part of the name, as [`tidy`] spelled it.
    filed: Option<String>,
    draw: bool,
}

/// `round_0000012345_Anna.txt` reads as "Anna, 12345", and
/// `round_0000012345.txt`, with no winner part, as a draw at 12345 (see
/// [`file_name`]). The timestamp is shown as a date only if the clock
/// agrees it is one; a file copied from another machine is still listed,
/// just plainly.
fn name_of(path: &std::path::Path) -> Named {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("round")
        .to_string();
    let mut parts = stem.splitn(3, '_');
    let (_, stamp, winner) = (parts.next(), parts.next(), parts.next());
    match (stamp.and_then(|s| s.parse::<u64>().ok()), winner) {
        (Some(stamp), Some(winner)) => Named {
            when: clock(stamp),
            filed: Some(winner.to_string()),
            draw: false,
        },
        (Some(stamp), None) => Named {
            when: clock(stamp),
            filed: None,
            draw: true,
        },
        (None, _) => Named {
            when: stem,
            filed: None,
            draw: false,
        },
    }
}

/// A winner's filed name as the shelf shows it: the file name's `_` back
/// to spaces.
fn shown(filed: &str) -> String {
    filed.replace('_', " ")
}

/// What took the round, when it was not a person, from the round's head
/// and the winner's name in its file name, `filed`.
///
/// The file names the winner as the results card did: a seat's name or its
/// "P{n}" label, or under a team mode the team's joined, in the language of
/// the machine that filed it. So every seat, or every team, is spelled in
/// every language, and the one whose spelling is the file's is the winner.
/// A name two of them spell (two seats called alike) settles only when
/// both were held alike; otherwise the row says nothing rather than guess.
/// The round is never played through to find out: that costs a whole
/// round's ticks for every file on the shelf.
///
/// A seat is marked by what held it ([`Replay::kinds`]); a team only when
/// all its seats were held alike, so a person paired with the AI is a
/// person's win.
fn won_by(head: &Replay, seats: u8, filed: &str) -> Option<WonBy> {
    let mode = crate::app::teams::recorded(head).played_by(seats);
    let mut found = None;
    for team in 0..mode.teams(seats) {
        let spelled = crate::app::i18n::ALL_LANGS.iter().any(|lang| {
            let label = crate::app::teams::team_label(mode, team, seats, |seat| {
                crate::app::name_or_label(&head.names, lang.tr(), seat)
            });
            tidy(&label) == filed
        });
        if !spelled {
            continue;
        }
        let held = held_by(head, &mode.seats_of(team, seats));
        match found {
            None => found = Some(held),
            Some(other) if other != held => return None,
            Some(_) => {}
        }
    }
    found.flatten()
}

/// What held every one of `seats`, when that was not a person: the AI, at
/// its level if they all played at one, or a bot.
fn held_by(head: &Replay, seats: &[u8]) -> Option<WonBy> {
    let kinds: Vec<SeatKind> = seats.iter().map(|&s| head.kinds[usize::from(s)]).collect();
    let levels: Vec<Option<BotLevel>> = seats.iter().map(|&s| head.ai[usize::from(s)]).collect();
    if kinds.iter().all(|&kind| kind == SeatKind::Bot) {
        return Some(WonBy::Bot);
    }
    if !kinds.iter().all(|&kind| kind == SeatKind::Ai) {
        return None;
    }
    let level = levels[0].filter(|_| levels.iter().all(|&l| l == levels[0]));
    Some(WonBy::Ai(level))
}

/// A stamp as `dd/mm hh:mm`, on the clock in the room. No date library and
/// no need for one: days since the epoch convert with the same arithmetic
/// the daily challenge already uses, and the offset to add first comes from
/// the system zone (see [`crate::app::clock::local_offset`]).
///
/// The offset is taken at the stamp rather than now, so a round is shown on
/// the clock that was on the wall when it was played.
fn clock(stamp: u64) -> String {
    stamp_text(stamp, crate::app::clock::local_offset(stamp))
}

/// The same, with the offset handed in, so the format can be tested on a
/// machine in any zone.
pub(super) fn stamp_text(stamp: u64, offset: i64) -> String {
    // A clock set before 1970 already reads as zero (`clock::now_secs`);
    // west of Greenwich the offset can push such a stamp below it, and the
    // arithmetic below is unsigned. It stays at the epoch.
    let local = (stamp as i64 + offset).max(0) as u64;
    let secs = local % 86_400;
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    let (day, month) = crate::app::clock::civil_date((local / 86_400) as u32);
    format!("{day:02}/{month:02} {h:02}:{m:02}")
}

#[cfg(test)]
mod tests {
    use super::super::tests::{stocked, winners};
    use super::*;

    #[test]
    fn round_names_sort_newest_last_and_survive_odd_winners() {
        let name = |stamp, winner| file_name(stamp, Some(winner));
        assert_eq!(name(12_345, "Anna"), "round_0000012345_Anna.txt");
        // A name with punctuation, or none at all, still makes a file name.
        assert_eq!(name(7, "Bo/../etc"), "round_0000000007_Bo____etc.txt");
        assert_eq!(name(7, ""), "round_0000000007_.txt");
        // A draw has no winner part, which no name can spell.
        assert_eq!(file_name(7, None), "round_0000000007.txt");
        // Zero-padded so plain text sorting is chronological.
        assert!(name(2, "a") < name(10, "a"));
        // And over-long winners are cut rather than making a silly path.
        assert!(name(1, &"x".repeat(40)).len() < 40);
    }

    /// Only a round on the shelf can be deleted: `last.txt`, the
    /// highlight reel and a round from another folder all stay.
    #[test]
    fn only_shelf_rounds_are_deletable() {
        let dir = stocked("delete-shelf-only", 1);
        let elsewhere = stocked("delete-elsewhere", 1);
        for path in [
            dir.join("last.txt"),
            dir.join("highlight.gif"),
            shelf_in(&elsewhere)[0].path.clone(),
        ] {
            assert!(delete_in(&dir, &path).is_err(), "{path:?}");
            assert!(path.exists(), "{path:?} kept");
        }
        // A round that is on the shelf goes, and a second try says why not.
        let round = shelf_in(&dir)[0].path.clone();
        delete_in(&dir, &round).expect("deleted");
        assert!(!round.exists());
        assert!(delete_in(&dir, &round).is_err());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&elsewhere);
    }

    /// Lowering "Rounds kept" trims the shelf at once to the newest; raising
    /// it deletes nothing, even on a shelf already over the new cap.
    #[test]
    fn a_lowered_cap_prunes_at_once_and_a_raised_one_never_does() {
        let dir = stocked("cap-changed", 12);
        cap_changed_in(&dir, 5, 10);
        assert_eq!(winners(&dir).len(), 12, "raised: nothing goes");
        cap_changed_in(&dir, 20, 20);
        assert_eq!(winners(&dir).len(), 12, "unchanged: nothing goes");
        cap_changed_in(&dir, 20, 5);
        assert_eq!(
            winners(&dir),
            ["P11", "P10", "P9", "P8", "P7"],
            "newest kept"
        );
        assert!(dir.join("last.txt").exists() && dir.join("highlight.gif").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A stamp reads on the clock that was on the wall, which is not the
    /// one in Greenwich. The offset is handed in here because the machine
    /// running this is in whatever zone it is in.
    #[test]
    fn a_stamp_reads_on_the_local_clock() {
        // 2025-07-01 00:00:00 UTC, day 20270.
        const NOON_LESS_TWELVE: u64 = 1_751_328_000;
        assert_eq!(stamp_text(NOON_LESS_TWELVE, 0), "01/07 00:00", "Greenwich");
        assert_eq!(stamp_text(NOON_LESS_TWELVE, 7200), "01/07 02:00", "Paris");
        // West of Greenwich the same instant is still the day before, which
        // is the case a bare `stamp % 86_400` gets wrong.
        assert_eq!(
            stamp_text(NOON_LESS_TWELVE, -4 * 3600),
            "30/06 20:00",
            "Halifax"
        );
        // And a stamp so early the offset would take it below the epoch
        // stays at the epoch rather than wrapping to the far future.
        assert_eq!(stamp_text(60, -4 * 3600), "01/01 00:00");
    }
}
