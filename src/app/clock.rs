//! The wall clock, read in one place. Replay stamps, fresh match seeds and
//! the daily-challenge day all start here, so what happens on a clock set
//! before 1970 is one policy rather than a scattering of fallbacks.

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
