//! What the bot commands share: reading flags, building the beach a game
//! is played on, naming files by the clock, and printing a table.

use crate::app::match_setup::{GullPressure, MapChoice, RoundLength};
use crate::sim::{Board, BotLevel, CapPolicy, Level, MAX_PLAYERS, MAX_SIGNPOSTS_PER_PLAYER};
use std::path::{Path, PathBuf};

/// A command line, read flag by flag. Every flag is `--name value` or
/// `--name=value`, a switch is `--name`, and anything else is positional.
pub struct Args {
    rest: Vec<String>,
}

impl Args {
    pub fn new(args: impl IntoIterator<Item = String>) -> Args {
        Args {
            rest: args.into_iter().collect(),
        }
    }

    /// Every value given for `--name`, in order, taken off the line.
    pub fn all(&mut self, name: &str) -> Result<Vec<String>, String> {
        let flag = format!("--{name}");
        let eq = format!("--{name}=");
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.rest.len() {
            if self.rest[i] == flag {
                if i + 1 >= self.rest.len() {
                    return Err(format!("{flag} needs a value"));
                }
                out.push(self.rest.remove(i + 1));
                self.rest.remove(i);
            } else if let Some(value) = self.rest[i].strip_prefix(&eq) {
                out.push(value.to_string());
                self.rest.remove(i);
            } else {
                i += 1;
            }
        }
        Ok(out)
    }

    /// The value of `--name`, if given (the last one wins).
    pub fn value(&mut self, name: &str) -> Result<Option<String>, String> {
        Ok(self.all(name)?.pop())
    }

    /// `--name` parsed, if given.
    pub fn parsed<T: std::str::FromStr>(&mut self, name: &str) -> Result<Option<T>, String> {
        match self.value(name)? {
            Some(v) => v
                .parse()
                .map(Some)
                .map_err(|_| format!("--{name}: {v:?} is not a valid value")),
            None => Ok(None),
        }
    }

    /// Whether the switch `--name` is on the line, taken off it.
    pub fn switch(&mut self, name: &str) -> bool {
        let flag = format!("--{name}");
        let before = self.rest.len();
        self.rest.retain(|a| *a != flag);
        self.rest.len() != before
    }

    /// `--name on|off`.
    pub fn on_off(&mut self, name: &str, default: bool) -> Result<bool, String> {
        match self.value(name)?.as_deref() {
            None => Ok(default),
            Some("on") => Ok(true),
            Some("off") => Ok(false),
            Some(other) => Err(format!("--{name} is on or off, not {other:?}")),
        }
    }

    /// What is left: the positional arguments. A leftover flag is an error,
    /// so a typo is said rather than ignored.
    pub fn finish(self) -> Result<Vec<String>, String> {
        if let Some(flag) = self.rest.iter().find(|a| a.starts_with("--")) {
            return Err(format!("unknown flag {flag}"));
        }
        Ok(self.rest)
    }
}

/// `easy`, `normal` or `hard` (`fierce` is the name the menus use).
pub fn bot_level(text: &str) -> Result<BotLevel, String> {
    match text {
        "easy" => Ok(BotLevel::Easy),
        "normal" => Ok(BotLevel::Normal),
        "hard" | "fierce" => Ok(BotLevel::Hard),
        other => Err(format!("{other:?} is not an AI level (easy, normal, hard)")),
    }
}

/// `ai:<level>`.
pub fn ai_seat(text: &str) -> Result<BotLevel, String> {
    let level = text
        .strip_prefix("ai:")
        .ok_or_else(|| format!("{text:?} is not ai:<level>"))?;
    bot_level(level)
}

pub fn round_length(text: &str) -> Result<RoundLength, String> {
    match text {
        "short" => Ok(RoundLength::Short),
        "standard" => Ok(RoundLength::Standard),
        "long" => Ok(RoundLength::Long),
        other => Err(format!(
            "{other:?} is not a round length (short, standard, long)"
        )),
    }
}

pub fn round_name(round: RoundLength) -> String {
    let secs = round.ticks() / crate::sim::TICKS_PER_SECOND;
    let name = match round {
        RoundLength::Short => "short",
        RoundLength::Standard => "standard",
        RoundLength::Long => "long",
    };
    format!("{name} round ({}:{:02})", secs / 60, secs % 60)
}

/// The beach a game is played on.
#[derive(Clone, Debug)]
pub enum Beach {
    Map(MapChoice),
    /// `generated`: the generated beach sized for the table.
    Generated,
    File(PathBuf, Box<Level>),
}

impl Beach {
    pub fn parse(text: &str) -> Result<Beach, String> {
        Ok(match text {
            "classic" => Beach::Map(MapChoice::Classic),
            "generated" => Beach::Generated,
            "small" => Beach::Map(MapChoice::GenSmall),
            "large" => Beach::Map(MapChoice::GenLarge),
            "xl" => Beach::Map(MapChoice::GenXl),
            "ocean" => Beach::Map(MapChoice::GenOcean),
            path => {
                let text =
                    std::fs::read_to_string(path).map_err(|e| format!("--map {path}: {e}"))?;
                let level = Level::parse(&text).map_err(|e| format!("--map {path}: {e}"))?;
                Beach::File(PathBuf::from(path), Box::new(level))
            }
        })
    }

    pub fn name(&self) -> String {
        match self {
            Beach::Map(MapChoice::Classic) => "classic".into(),
            Beach::Map(MapChoice::GenSmall) => "small".into(),
            Beach::Generated | Beach::Map(MapChoice::GenClassic) => "generated".into(),
            Beach::Map(MapChoice::GenLarge) => "large".into(),
            Beach::Map(MapChoice::GenXl) => "xl".into(),
            Beach::Map(MapChoice::GenOcean) => "ocean".into(),
            Beach::Map(MapChoice::Custom) => "custom".into(),
            Beach::File(path, _) => path.display().to_string(),
        }
    }

    /// Whether it seats `seats`, or why not.
    pub fn check(&self, seats: u8) -> Result<(), String> {
        if !(2..=MAX_PLAYERS as u8).contains(&seats) {
            return Err(format!("a table seats 2 to {MAX_PLAYERS}, not {seats}"));
        }
        match self {
            Beach::Map(MapChoice::Classic) if seats > 4 => {
                Err("the classic beach seats 2 to 4; use --map generated".into())
            }
            Beach::Map(map) if seats > 4 && map.size().0 < crate::app::match_setup::WIDE_ENOUGH => {
                Err(format!("the {} beach seats 2 to 4", self.name()))
            }
            Beach::File(_, level) if level.seats() < seats => Err(format!(
                "{} has castles for {} seats, not {seats}",
                self.name(),
                level.seats()
            )),
            Beach::Map(_) | Beach::Generated | Beach::File(..) => Ok(()),
        }
    }

    /// The board for one game, the way a match set up in the game builds
    /// it (`match_setup::board_for`): normal gulls, the round's tide, three
    /// arrows a seat.
    pub fn board(&self, seed: u64, seats: u8, round: RoundLength) -> Board {
        let mut board = match self {
            Beach::File(_, level) => level.board(),
            Beach::Map(MapChoice::Classic) => crate::sim::classic_arena_seeded(seed, false, seats),
            Beach::Generated | Beach::Map(_) => {
                let map = match self {
                    Beach::Map(map) => *map,
                    Beach::Generated | Beach::File(..) if seats > 4 => MapChoice::GenLarge,
                    Beach::Generated | Beach::File(..) => MapChoice::GenClassic,
                };
                let (w, h) = map.size();
                let mut board = crate::sim::generate_arena(seed, seats, w, h);
                board.set_wrap(map.wraps());
                board
            }
        };
        board.set_gull_period(GullPressure::Normal.period());
        board.set_round_length(Some(round.ticks()));
        board.set_signpost_rule(MAX_SIGNPOSTS_PER_PLAYER as u8, CapPolicy::Evict);
        board
    }
}

/// A seed nobody chose: the clock's nanoseconds, cut short enough to type.
pub fn fresh_seed() -> u64 {
    crate::app::clock::fresh_seed() % 100_000
}

/// `2026-09-29-1402`, the local wall clock, for naming a run's files.
pub fn stamp() -> String {
    let now = crate::app::clock::now_secs();
    let local = now as i64 + crate::app::clock::local_offset(now);
    let days = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    let (y, m, d) = crate::app::clock::civil_ymd(days.max(0) as u32);
    format!(
        "{y:04}-{m:02}-{d:02}-{:02}{:02}",
        secs / 3600,
        (secs / 60) % 60
    )
}

/// `dir/stem.ext`, or `dir/stem-2.ext` and on when that is taken, so two
/// runs in one minute keep both.
pub fn free_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{stem}-{n}.{ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// Lay rows out in columns: the first row is the header, text columns are
/// left-aligned and numbers right-aligned.
pub fn table(rows: &[Vec<String>]) -> String {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let width = |c: usize| {
        rows.iter()
            .filter_map(|r| r.get(c))
            .map(|s| s.chars().count())
            .max()
            .unwrap_or(0)
    };
    let widths: Vec<usize> = (0..cols).map(width).collect();
    let numeric = |c: usize| {
        rows.iter()
            .skip(1)
            .filter_map(|r| r.get(c))
            .all(|s| s.starts_with(|ch: char| ch.is_ascii_digit() || ch == '-' || ch == '+'))
    };
    let mut out = String::new();
    for row in rows {
        out.push_str("  ");
        for (c, cell) in row.iter().enumerate() {
            let pad = widths[c].saturating_sub(cell.chars().count());
            if numeric(c) {
                out.push_str(&" ".repeat(pad));
                out.push_str(cell);
            } else {
                out.push_str(cell);
                if c + 1 < row.len() {
                    out.push_str(&" ".repeat(pad));
                }
            }
            if c + 1 < row.len() {
                out.push_str("  ");
            }
        }
        out.push('\n');
    }
    out
}

/// The `p`-th percentile of some reply times, in milliseconds.
pub fn percentile(times: &[f64], p: f64) -> Option<f64> {
    if times.is_empty() {
        return None;
    }
    let mut sorted = times.to_vec();
    sorted.sort_by(f64::total_cmp);
    let rank = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted.get(rank).copied()
}

/// A duration in milliseconds the way the standings print one.
pub fn ms(value: f64) -> String {
    if value < 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_are_read_in_both_spellings_and_typos_are_said() {
        let mut args = Args::new(
            [
                "--seat",
                "ai:easy",
                "--seat=ai:hard",
                "--open",
                "2",
                "--watch",
                "file",
            ]
            .map(String::from),
        );
        assert_eq!(
            args.all("seat"),
            Ok(vec!["ai:easy".to_string(), "ai:hard".to_string()])
        );
        assert_eq!(args.parsed::<u32>("open"), Ok(Some(2)));
        assert!(args.switch("watch"));
        assert_eq!(args.finish(), Ok(vec!["file".to_string()]));
        let args = Args::new(["--sead", "1"].map(String::from));
        assert!(args.finish().is_err());
        assert!(Args::new(["--open".to_string()]).all("open").is_err());
    }

    #[test]
    fn a_percentile_is_one_of_the_times() {
        let times = [5.0, 1.0, 3.0, 2.0, 4.0];
        assert_eq!(percentile(&times, 50.0), Some(3.0));
        assert_eq!(percentile(&times, 99.0), Some(5.0));
        assert_eq!(percentile(&[], 50.0), None);
    }

    #[test]
    fn a_beach_that_cannot_seat_the_table_says_so() {
        assert!(Beach::Map(MapChoice::Classic).check(5).is_err());
        assert!(Beach::Generated.check(6).is_ok());
        assert!(Beach::Generated.check(7).is_err());
        let board = Beach::Generated.board(3, 6, RoundLength::Short);
        assert_eq!(board.castle_seats(), 6);
    }
}
