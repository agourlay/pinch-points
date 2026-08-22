//! The wall clock, read in one place. Replay stamps, fresh match seeds and
//! the daily-challenge day all start here, so what happens on a clock set
//! before 1970 is one policy rather than a scattering of fallbacks.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the epoch. A clock from before 1970 reads as zero: still
/// a stamp, still sorts, just not a believable date.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The civil date a day count lands on, as `(day, month)`, UTC.
///
/// Howard Hinnant's algorithm, integer-only, and no date crate for two
/// fields. Shared because the shelf of kept rounds stamps them and the
/// daily challenge names its day, and two copies of a calendar is two
/// calendars.
pub fn civil_date(days: u32) -> (u32, u32) {
    let z = i64::from(days) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let _year = yoe + era * 400 + i64::from(month <= 2);
    (day as u32, month as u32)
}

/// A fresh seed for a locally configured round, from the clock's
/// nanoseconds. A clock with no idea gives a constant: a repeated seed
/// makes a familiar beach, not a broken one.
pub fn fresh_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xC0DE)
}

// --- the system time zone -------------------------------------------------

/// The UTC offset in force at `stamp`, in seconds east of UTC.
///
/// Read from the system zone file, because a player reads a replay's stamp
/// on the wall clock behind them: a round played at 06:36 in Paris was
/// listed as 04:36, which is a true statement about a clock nobody in the
/// room was looking at.
///
/// Per-stamp rather than one offset for the whole run, so a round kept in
/// August and one kept in January each read right on the same shelf. Zero
/// where there is no zone file to read - Windows keeps its zone in the
/// registry and is left on UTC, as it was before this existed.
pub fn local_offset(stamp: u64) -> i64 {
    static ZONE: std::sync::OnceLock<Option<Zone>> = std::sync::OnceLock::new();
    // Parsed once: the shelf asks this for every kept round each time it
    // opens, and the zone does not change under a running game.
    ZONE.get_or_init(|| zone_bytes().as_deref().and_then(parse_tzif))
        .as_ref()
        .map_or(0, |zone| i64::from(zone.offset_at(stamp as i64)))
}

/// A time zone as the shelf needs it: when the offset changed, and to what.
/// The names, leap seconds and standard/wall flags in the file are of no
/// use here and are skipped.
struct Zone {
    /// Transition instants, ascending, seconds since the epoch.
    at: Vec<i64>,
    /// The offset in force from the transition at the same index.
    offset: Vec<i32>,
    /// Before the first transition, and for a zone that has none.
    first: i32,
}

impl Zone {
    fn offset_at(&self, stamp: i64) -> i32 {
        match self.at.binary_search(&stamp) {
            Ok(i) => self.offset[i],
            Err(0) => self.first,
            Err(i) => self.offset[i - 1],
        }
    }
}

/// The zone file's bytes: what `TZ` names, or the system's own.
///
/// `TZ` holding a POSIX rule rather than a zone name (`EST5EDT`) names no
/// file, so it falls through to the system zone rather than being honoured.
/// Naming a rule inline is rare, and a shelf an hour out beats a shelf that
/// refuses to say anything.
fn zone_bytes() -> Option<Vec<u8>> {
    if let Some(tz) = std::env::var_os("TZ") {
        let tz = tz.to_str()?.trim_start_matches(':');
        // `..` in a zone name is not a zone name.
        if !tz.is_empty() && !tz.contains("..") {
            let path = match tz.starts_with('/') {
                true => PathBuf::from(tz),
                false => Path::new("/usr/share/zoneinfo").join(tz),
            };
            if let Ok(bytes) = std::fs::read(path) {
                return Some(bytes);
            }
        }
    }
    std::fs::read("/etc/localtime").ok()
}

/// The six counts in a TZif header, in the order they are written.
struct Counts {
    isut: usize,
    isstd: usize,
    leap: usize,
    time: usize,
    ttype: usize,
    chars: usize,
}

/// Bytes of a header, fixed by the format.
const TZIF_HEADER: usize = 44;

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// The header at `from`: the version byte and the six counts.
fn tzif_header(bytes: &[u8], from: usize) -> Option<(u8, Counts)> {
    if bytes.get(from..from + 4)? != b"TZif" {
        return None;
    }
    let version = *bytes.get(from + 4)?;
    let mut n = [0usize; 6];
    for (i, slot) in n.iter_mut().enumerate() {
        *slot = be32(bytes, from + 20 + i * 4)? as usize;
    }
    let counts = Counts {
        isut: n[0],
        isstd: n[1],
        leap: n[2],
        time: n[3],
        ttype: n[4],
        chars: n[5],
    };
    Some((version, counts))
}

/// How long the data block after a header runs, given the width of a
/// transition stamp in it.
fn block_len(counts: &Counts, stamp: usize) -> usize {
    counts.time * (stamp + 1)
        + counts.ttype * 6
        + counts.chars
        + counts.leap * (stamp + 4)
        + counts.isstd
        + counts.isut
}

/// Parse a TZif file (RFC 8536) down to its transitions.
///
/// A version 2 or later file carries everything twice: once with 32-bit
/// stamps for readers that predate the format, then again with 64-bit ones.
/// The wide block is the one read, and not only for the year 2038: the
/// "slim" files most distributions now ship leave the 32-bit block with no
/// transitions at all, so a reader that trusted it would answer every
/// question with the zone's pre-1900 local mean time.
fn parse_tzif(bytes: &[u8]) -> Option<Zone> {
    let (version, counts) = tzif_header(bytes, 0)?;
    let (at, counts, stamp) = match version >= b'2' {
        true => {
            let second = TZIF_HEADER + block_len(&counts, 4);
            let (_, wide) = tzif_header(bytes, second)?;
            (second + TZIF_HEADER, wide, 8)
        }
        false => (TZIF_HEADER, counts, 4),
    };
    // The type records first: the transitions index them.
    let types_at = at + counts.time * (stamp + 1);
    let mut offsets = Vec::with_capacity(counts.ttype);
    let mut standard = None;
    for i in 0..counts.ttype {
        let rec = types_at + i * 6;
        let utoff = be32(bytes, rec)? as i32;
        offsets.push(utoff);
        if *bytes.get(rec + 4)? == 0 && standard.is_none() {
            standard = Some(utoff);
        }
    }
    // A zone with no types at all is not a zone.
    let first = standard.or_else(|| offsets.first().copied())?;

    let mut zone = Zone {
        at: Vec::with_capacity(counts.time),
        offset: Vec::with_capacity(counts.time),
        first,
    };
    for i in 0..counts.time {
        let when = match stamp {
            8 => i64::from_be_bytes(bytes.get(at + i * 8..at + i * 8 + 8)?.try_into().ok()?),
            _ => i64::from(be32(bytes, at + i * 4)? as i32),
        };
        let index = usize::from(*bytes.get(at + counts.time * stamp + i)?);
        zone.at.push(when);
        zone.offset.push(*offsets.get(index)?);
    }
    Some(zone)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One TZif data block: transitions, the type each selects, the type
    /// records, one designation byte, and the standard/wall and UT/local
    /// flags real files carry and this reader has to step over.
    fn block(times: &[i64], picks: &[u8], types: &[(i32, u8)], wide: bool) -> Vec<u8> {
        let mut out = Vec::new();
        for t in times {
            match wide {
                true => out.extend_from_slice(&t.to_be_bytes()),
                false => out.extend_from_slice(&(*t as i32).to_be_bytes()),
            }
        }
        out.extend_from_slice(picks);
        for (offset, dst) in types {
            out.extend_from_slice(&offset.to_be_bytes());
            out.push(*dst);
            out.push(0);
        }
        out.push(0); // one designation byte: charcnt is 1
        out.extend(std::iter::repeat_n(0u8, types.len() * 2)); // isstd, isut
        out
    }

    fn header(version: u8, times: usize, types: usize) -> Vec<u8> {
        let mut out = b"TZif".to_vec();
        out.push(version);
        out.extend_from_slice(&[0u8; 15]);
        for n in [types, types, 0, times, types, 1] {
            out.extend_from_slice(&(n as u32).to_be_bytes());
        }
        out
    }

    /// Paris in 2026: +1 in winter, +2 from the last Sunday in March.
    const WINTER: i32 = 3600;
    const SUMMER: i32 = 7200;
    /// 2026-03-29 01:00 UTC and 2026-10-25 01:00 UTC.
    const SPRING_FORWARD: i64 = 1_774_745_940;
    const FALL_BACK: i64 = 1_793_499_600;

    /// A version 1 file, read straight through.
    #[test]
    fn a_plain_zone_file_gives_the_offset_of_the_moment() {
        let types = [(WINTER, 0u8), (SUMMER, 1)];
        let mut file = header(b'\0', 2, 2);
        file.extend(block(&[SPRING_FORWARD, FALL_BACK], &[1, 0], &types, false));
        let zone = parse_tzif(&file).expect("a version 1 file parses");

        assert_eq!(
            zone.offset_at(SPRING_FORWARD - 1),
            WINTER,
            "before the change"
        );
        assert_eq!(zone.offset_at(SPRING_FORWARD), SUMMER, "on the tick of it");
        assert_eq!(zone.offset_at(FALL_BACK - 1), SUMMER, "all summer");
        assert_eq!(zone.offset_at(FALL_BACK), WINTER, "and back again");
        assert_eq!(
            zone.offset_at(0),
            WINTER,
            "before the file starts, standard time"
        );
    }

    /// The trap this reader exists to avoid. Distributions ship "slim"
    /// files: version 2 or later, with the 32-bit block emptied out and
    /// every transition in the 64-bit one. A reader that took the first
    /// block would answer every question with the sole leftover type.
    #[test]
    fn a_slim_file_is_read_from_its_wide_block() {
        let lmt = [(561i32, 0u8)]; // Paris local mean time, the slim leftover
        let types = [(WINTER, 0u8), (SUMMER, 1)];
        let mut file = header(b'2', 0, 1);
        file.extend(block(&[], &[], &lmt, false));
        file.extend(header(b'2', 2, 2));
        file.extend(block(&[SPRING_FORWARD, FALL_BACK], &[1, 0], &types, true));
        file.extend_from_slice(b"\nCET-1CEST,M3.5.0,M10.5.0/3\n");
        let zone = parse_tzif(&file).expect("a version 2 file parses");

        assert_eq!(
            zone.offset_at(SPRING_FORWARD + 60),
            SUMMER,
            "summer, not 1891"
        );
        assert_eq!(zone.offset_at(FALL_BACK + 60), WINTER, "winter, not 1891");
        assert_ne!(
            zone.first, 561,
            "the wide block's own types, not the slim one's"
        );
    }

    /// Nothing here may panic on bytes it did not write: the file is
    /// whatever `TZ` happened to name.
    #[test]
    fn rubbish_is_declined_rather_than_believed() {
        assert!(parse_tzif(b"").is_none(), "empty");
        assert!(parse_tzif(b"not a zone file at all").is_none(), "no magic");
        let types = [(WINTER, 0u8)];
        let mut file = header(b'\0', 1, 1);
        file.extend(block(&[SPRING_FORWARD], &[0], &types, false));
        assert!(parse_tzif(&file).is_some(), "the whole file is fine");
        for cut in 0..file.len() {
            // Truncated anywhere: an answer or none, never a panic.
            let _ = parse_tzif(&file[..cut]);
        }
        // A transition naming a type that is not there.
        let mut bogus = header(b'\0', 1, 1);
        bogus.extend(block(&[SPRING_FORWARD], &[9], &types, false));
        assert!(parse_tzif(&bogus).is_none(), "a type index off the end");
    }
}
