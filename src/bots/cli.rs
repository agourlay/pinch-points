//! What the bot commands share: reading flags, building the beach a game
//! is played on, naming files by the clock, and printing a table.

use super::connstr::DEFAULT_PORT;
use super::listener::{BotId, Listener};
use crate::app::match_setup::{GullPressure, MapChoice, RoundLength};
use crate::sim::{
    Board, BotLevel, CapPolicy, Level, MAX_PLAYERS, MAX_SIGNPOSTS_PER_PLAYER, PlayerId,
    TICKS_PER_SECOND,
};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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

/// What every table the bot commands set up is read from: where bots
/// connect, the beach, the round, the reply deadline and the cursor rule.
pub struct TableOpts {
    pub listen: SocketAddr,
    pub beach: Beach,
    pub round: RoundLength,
    /// The reply deadline in milliseconds: one tick unless asked, and
    /// never zero.
    pub deadline: u32,
    pub fair: bool,
}

impl TableOpts {
    /// `--listen`/`--port`, `--map`, `--round`, `--deadline` and
    /// `--fair-cursor`, taken off the line. Bots connect to `default_ip`
    /// on the bots' own port unless told otherwise.
    pub fn read(args: &mut Args, default_ip: [u8; 4]) -> Result<TableOpts, String> {
        let mut listen: SocketAddr = args
            .parsed("listen")?
            .unwrap_or_else(|| SocketAddr::from((default_ip, DEFAULT_PORT)));
        if let Some(port) = args.parsed::<u16>("port")? {
            listen.set_port(port);
        }
        let beach = match args.value("map")? {
            Some(m) => Beach::parse(&m)?,
            None => Beach::Map(MapChoice::Classic),
        };
        let round = match args.value("round")? {
            Some(r) => round_length(&r)?,
            None => RoundLength::Standard,
        };
        let deadline = args
            .parsed::<u32>("deadline")?
            .unwrap_or(ONE_TICK_MS)
            .max(1);
        let fair = args.on_off("fair-cursor", false)?;
        Ok(TableOpts {
            listen,
            beach,
            round,
            deadline,
            fair,
        })
    }
}

/// A tick in milliseconds: the deadline a live match has.
pub const ONE_TICK_MS: u32 = 1000 / TICKS_PER_SECOND;

/// What the header says of a deadline longer than a tick.
pub fn slow_note(deadline: u32) -> &'static str {
    if deadline > ONE_TICK_MS {
        " (a slow match)"
    } else {
        ""
    }
}

/// Give a fresh registration's pings a moment to come back, so there is a
/// round trip to print.
pub fn wait_for_rtt(listener: &Listener, id: BotId) {
    let asked = Instant::now();
    while listener.rtt(id).is_none() && asked.elapsed() < Duration::from_millis(300) {
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A round trip the way the console and the standings print one.
pub fn rtt_text(rtt: Duration) -> String {
    format!("{} ms", ms(rtt.as_secs_f64() * 1000.0))
}

/// Reply times as the standings print them: the median and the 99th
/// percentile, or a dash with none.
pub fn reply_text(times: &[f64]) -> String {
    match (percentile(times, 50.0), percentile(times, 99.0)) {
        (Some(p50), Some(p99)) => format!("{} / {} ms", ms(p50), ms(p99)),
        _ => "-".into(),
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

/// The word [`bot_level`] reads as `level`.
pub fn level_token(level: BotLevel) -> &'static str {
    match level {
        BotLevel::Easy => "easy",
        BotLevel::Normal => "normal",
        BotLevel::Hard => "hard",
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
        "quick" => Ok(RoundLength::Quick),
        "short" => Ok(RoundLength::Short),
        "standard" => Ok(RoundLength::Standard),
        "long" => Ok(RoundLength::Long),
        other => Err(format!(
            "{other:?} is not a round length (quick, short, standard, long)"
        )),
    }
}

pub fn round_name(round: RoundLength) -> String {
    let secs = round.ticks() / crate::sim::TICKS_PER_SECOND;
    let name = match round {
        RoundLength::Quick => "quick",
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
            Beach::File(_, level) => {
                // Seat `n` plays castle `n`, and the table's scores run to
                // the highest castle: a castle short is a seat with nothing
                // to bank in, and one past the seats is somebody's bank
                // nobody plays.
                let mut owners: Vec<PlayerId> = level.board().castle_owners().collect();
                owners.sort_unstable();
                owners.dedup();
                if owners.iter().copied().eq(0..seats) {
                    return Ok(());
                }
                let marked: Vec<String> = owners.iter().map(PlayerId::to_string).collect();
                Err(format!(
                    "{} has castles marked {}; a table of {seats} needs castles marked 0 to {} \
                     and no others",
                    self.name(),
                    if marked.is_empty() {
                        "nowhere".to_string()
                    } else {
                        marked.join(", ")
                    },
                    seats - 1
                ))
            }
            Beach::Map(_) | Beach::Generated => Ok(()),
        }
    }

    /// The board for one game, the way a match set up in the game builds
    /// it (`match_setup::board_for`): normal gulls, the round's tide, three
    /// arrows a seat.
    pub fn board(&self, seed: u64, seats: u8, round: RoundLength) -> Board {
        let mut board = match self {
            Beach::File(_, level) => level.board_seeded(seed),
            Beach::Map(MapChoice::Classic) => crate::sim::classic_arena_seeded(seed, false, seats),
            Beach::Generated | Beach::Map(_) => {
                // Past the seats a classic-sized beach holds, the game
                // gives way to the one sized for the table
                // (`match_setup::settle_map`), and so does the arena.
                let map = match self {
                    Beach::Map(map) => *map,
                    Beach::Generated | Beach::File(..)
                        if crate::app::match_setup::holds(MapChoice::GenClassic, seats) =>
                    {
                        MapChoice::GenClassic
                    }
                    Beach::Generated | Beach::File(..) => crate::app::match_setup::sized_for(seats),
                };
                let (w, h) = map.size();
                let mut board = crate::sim::generate_arena(seed, seats, w, h);
                board.set_wrap(map.wraps());
                board
            }
        };
        crate::sim::set_versus_rules(&mut board, GullPressure::Normal.period());
        board.rules_mut().round_length = Some(round.ticks());
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

    /// A beach file seats a table only with castles marked exactly 0 to
    /// one less than the seats: counting the owners passed {0, 2} for two
    /// seats, and a castle past the seats sent scores nobody was seated
    /// for.
    #[test]
    fn a_beach_file_needs_a_castle_for_each_seat_and_no_more() {
        let beach = |row: &str| {
            let text =
                format!("name: T\nposts: 0\ngoal: survive\nmap:\n+-+-+-+\n|{row}|\n+-+-+-+\n");
            let level = Level::parse(&text).expect("level");
            Beach::File(PathBuf::from("t.beach"), Box::new(level))
        };
        assert_eq!(beach("0 . 1").check(2), Ok(()));
        let err = beach("0 . 2").check(2).expect_err("no castle 1");
        assert!(err.contains("marked 0, 2"), "{err}");
        assert!(beach("0 1 2").check(2).is_err(), "a castle past the seats");
        assert!(beach("0 1 2").check(3).is_ok());
    }

    /// Every level's word reads back as that level.
    #[test]
    fn a_level_token_reads_back_as_its_level() {
        for level in [BotLevel::Easy, BotLevel::Normal, BotLevel::Hard] {
            assert_eq!(bot_level(level_token(level)), Ok(level));
        }
    }

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

    /// `--map generated` is the beach the game deals the same table: the
    /// classic-sized generator for four or fewer, and the one it gives way
    /// to past four (`match_setup::settle_map`, the lobby's `map_for`). A
    /// bot tuned in the arena meets the board it was tuned on.
    #[test]
    fn a_generated_arena_is_the_beach_the_game_deals() {
        for seats in 2..=MAX_PLAYERS as u8 {
            let board = Beach::Generated.board(3, seats, RoundLength::Short);
            let mut config = crate::app::match_setup::MatchConfig {
                map: MapChoice::GenClassic,
                seats,
                ..Default::default()
            };
            crate::app::match_setup::settle_map(&mut config, &Default::default());
            let (w, h) = config.map.size();
            // The generator may widen a beach by a column for five or six.
            let widened = seats >= 5 && board.width() == w + 1;
            assert!(
                board.width() == w || widened,
                "{seats} seats: {} wide, the game's is {w}",
                board.width()
            );
            assert_eq!(board.height(), h, "{seats} seats");
            assert_eq!(config.map.wraps(), board.wrap(), "{seats} seats");
        }
    }
}
