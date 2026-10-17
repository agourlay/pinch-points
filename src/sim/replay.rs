//! Replays (spec §7.7): the starting level plus the per-tick input list.
//! Playing it back through `Board::tick` reproduces the match bit-for-bit.
//!
//! Text format: a `replay-v2` header, an optional `names:` line, an
//! optional `kinds:` line, the level text, then `inputs:` with one
//! line per tick: six hex digits per seat (full x, full y, op/dir byte), so
//! `MAX_PLAYERS` of them, where a bare `.` is the common all-idle tick. A
//! full byte per axis, like the wire (spec §7.6), so boards wider than 16
//! tiles replay exactly.

use crate::sim::board::{Board, MAX_PLAYERS, PlayerAction};
use crate::sim::level::Level;
// One codec for the 3-byte action, shared with the wire: a replay is the
// same bytes the lockstep carries, so widening one widens both.
use crate::sim::net::{decode_action, encode_action};

#[derive(Clone, Debug)]
pub struct Replay {
    pub level: Level,
    pub inputs: Vec<[PlayerAction; MAX_PLAYERS]>,
    /// What each seat was called when the round was played, empty for a
    /// seat that was never named.
    ///
    /// Not derivable from anything else in the file, and not the watcher's
    /// business to supply: a replay that fell back to the local couch names
    /// would put this machine's P1 on somebody else's crabs.
    pub names: [String; MAX_PLAYERS],
    /// What held each seat: a person, the game's AI, or a bot over the
    /// protocol. `watch` draws it beside the name, so nobody watching is
    /// surprised by what was playing.
    pub kinds: [SeatKind; MAX_PLAYERS],
    /// How the round was scored: the scoring mode's index
    /// (`app::teams::TeamMode::index`), 0 for every seat for itself.
    ///
    /// The sim never knows about teams, but who won does: watched with
    /// the watcher's own setting, a round played in pairs was crowned to
    /// its top single seat, and filed under one.
    pub teams: u8,
}

/// What kind of thing holds a seat, as the bot protocol, the replay and the
/// standings name it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SeatKind {
    #[default]
    Human,
    Ai,
    Bot,
}

impl SeatKind {
    pub fn token(self) -> &'static str {
        match self {
            SeatKind::Human => "human",
            SeatKind::Ai => "ai",
            SeatKind::Bot => "bot",
        }
    }

    pub fn from_token(token: &str) -> Option<SeatKind> {
        match token {
            "human" => Some(SeatKind::Human),
            "ai" => Some(SeatKind::Ai),
            "bot" => Some(SeatKind::Bot),
            _ => None,
        }
    }
}

/// The rules a recording was played under, not just the shape of the file.
///
/// A replay is inputs, and inputs only mean what the rules say they mean:
/// the roulette picks a face with one draw over `TideEvent::ALL`, so
/// adding a face in v2 (Right Claws) moved every draw after it. A v1
/// recording fed to these rules would not diverge loudly, it would quietly
/// play out a different round. Refused instead.
const HEADER: &str = "replay-v2";
const INPUTS_MARK: &str = "inputs:";
/// Seat names, one line, `|` between them. Read before the level text
/// rather than inside it: the level format knows nothing about who was
/// holding the controller, and should not have to.
const NAMES_MARK: &str = "names:";
/// Seat kinds, one line, `|` between them, after the names.
///
/// A line inside the header rather than a new format version, by the
/// rule the format already keeps: a build from before kinds were kept
/// hands this line to the level parser, which refuses a key it does not
/// know, so an older game refuses such a recording outright rather than
/// half-reading it. Written only when some seat was not a person, so a
/// plain couch round is byte for byte what it always was.
const KINDS_MARK: &str = "kinds:";
/// The scoring mode, one number, after the kinds: written, by the same
/// rule as the kinds, only for a round played in teams.
const TEAMS_MARK: &str = "teams:";

/// The most ticks a recording may hold: twenty minutes, four times the
/// longest round the setup screen offers (five). A recording is played
/// through whole wherever its result is wanted, and a pasted round code is
/// read on the spot to file it under its winner, so a code of two million
/// idle ticks (which packs into the 4 MiB a code may unpack to) held the
/// game still for as long as that took.
pub const MAX_TICKS: usize = 20 * 60 * crate::sim::board::TICKS_PER_SECOND as usize;

impl Replay {
    pub fn new(level: Level) -> Replay {
        Replay {
            level,
            inputs: Vec::new(),
            names: Default::default(),
            kinds: Default::default(),
            teams: 0,
        }
    }

    pub fn record(&mut self, actions: [PlayerAction; MAX_PLAYERS]) {
        self.inputs.push(actions);
    }

    /// Rebuild the starting board and run every recorded tick; returns the
    /// final board.
    pub fn playback(&self) -> Board {
        let mut board = self.level.board();
        for actions in &self.inputs {
            board.tick(actions);
        }
        board
    }

    pub fn to_text(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let _ = writeln!(out, "{HEADER}");
        // Written only when there is something to say, so a replay of an
        // unnamed local round is byte-for-byte what it always was.
        if self.names.iter().any(|n| !n.is_empty()) {
            // A name is somebody's to choose, a bot's included: the
            // separator and line breaks in one would cost the seats after
            // it their names, or the file its shape.
            let names: Vec<String> = self
                .names
                .iter()
                .map(|n| {
                    n.chars()
                        .map(|c| match c {
                            '|' => '/',
                            c if c.is_control() => ' ',
                            c => c,
                        })
                        .collect()
                })
                .collect();
            let _ = writeln!(out, "{NAMES_MARK} {}", names.join("|"));
        }
        if self.kinds.iter().any(|k| *k != SeatKind::Human) {
            let kinds: Vec<&str> = self.kinds.iter().map(|k| k.token()).collect();
            let _ = writeln!(out, "{KINDS_MARK} {}", kinds.join("|"));
        }
        if self.teams != 0 {
            let _ = writeln!(out, "{TEAMS_MARK} {}", self.teams);
        }
        out.push_str(&self.level.to_text());
        let _ = writeln!(out, "{INPUTS_MARK}");
        for actions in &self.inputs {
            if actions.iter().all(|a| matches!(a, PlayerAction::None)) {
                out.push_str(".\n");
                continue;
            }
            for action in actions {
                let [a, b, c] = encode_action(*action);
                let _ = write!(out, "{a:02x}{b:02x}{c:02x}");
            }
            out.push('\n');
        }
        out
    }

    pub fn parse(text: &str) -> Result<Replay, String> {
        let rest = text
            .strip_prefix(HEADER)
            .ok_or("not a replay this build can play")?
            .trim_start_matches(['\r', '\n']);
        // A file written before names were kept simply has no such line,
        // and reads exactly as it always did.
        let (names, rest) = match rest.strip_prefix(NAMES_MARK) {
            Some(after) => {
                let (line, rest) = after.split_once('\n').unwrap_or((after, ""));
                let mut names: [String; MAX_PLAYERS] = Default::default();
                for (slot, name) in names.iter_mut().zip(line.trim().split('|')) {
                    *slot = name.trim().to_string();
                }
                (names, rest)
            }
            None => (Default::default(), rest),
        };
        let (kinds, rest) = match rest.strip_prefix(KINDS_MARK) {
            Some(after) => {
                let (line, rest) = after.split_once('\n').unwrap_or((after, ""));
                let mut kinds = [SeatKind::Human; MAX_PLAYERS];
                for (slot, kind) in kinds.iter_mut().zip(line.trim().split('|')) {
                    *slot = SeatKind::from_token(kind.trim())
                        .ok_or_else(|| format!("no seat kind {:?}", kind.trim()))?;
                }
                (kinds, rest)
            }
            None => (Default::default(), rest),
        };
        let (teams, rest) = match rest.strip_prefix(TEAMS_MARK) {
            Some(after) => {
                let (line, rest) = after.split_once('\n').unwrap_or((after, ""));
                let teams = line
                    .trim()
                    .parse::<u8>()
                    .map_err(|_| format!("no scoring mode {:?}", line.trim()))?;
                (teams, rest)
            }
            None => (0, rest),
        };
        let (level_text, input_text) = rest
            .split_once(INPUTS_MARK)
            .ok_or("missing inputs: section")?;
        let level = Level::parse(level_text)?;
        let mut inputs = Vec::new();
        for line in input_text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if inputs.len() == MAX_TICKS {
                return Err(format!(
                    "longer than the {MAX_TICKS} ticks a round can last"
                ));
            }
            if line == "." {
                inputs.push([PlayerAction::None; MAX_PLAYERS]);
                continue;
            }
            // The length is in bytes and the slicing below is too, so a
            // corrupt file with the right byte count but a multi-byte
            // character in it would split one and panic. Hex is ASCII.
            if line.len() != MAX_PLAYERS * 6 || !line.is_ascii() {
                return Err(format!("bad input line {line:?}"));
            }
            let mut actions = [PlayerAction::None; MAX_PLAYERS];
            for (i, action) in actions.iter_mut().enumerate() {
                let hex = &line[i * 6..i * 6 + 6];
                let a = u8::from_str_radix(&hex[0..2], 16).map_err(|e| e.to_string())?;
                let b = u8::from_str_radix(&hex[2..4], 16).map_err(|e| e.to_string())?;
                let c = u8::from_str_radix(&hex[4..6], 16).map_err(|e| e.to_string())?;
                *action = decode_action([a, b, c]);
            }
            inputs.push(actions);
        }
        Ok(Replay {
            level,
            inputs,
            names,
            kinds,
            teams,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scoring mode survives the trip, and a free-for-all writes no
    /// line for it, so such a file is what it always was.
    #[test]
    fn a_replay_remembers_how_it_was_scored() {
        let level = Level::parse("name: T\nposts: 1\nmap:\n+-+-+\n|0 1|\n+-+-+\n").expect("level");
        let mut replay = Replay::new(level);
        replay.record([PlayerAction::None; MAX_PLAYERS]);
        assert!(!replay.to_text().contains(TEAMS_MARK));
        replay.kinds[1] = SeatKind::Bot;
        replay.teams = 2;
        let back = Replay::parse(&replay.to_text()).expect("round trip");
        assert_eq!((back.teams, back.kinds[1]), (2, SeatKind::Bot));
        assert!(Replay::parse(&replay.to_text().replace("teams: 2", "teams: x")).is_err());
    }

    /// Who was playing survives the trip, and a file written before names
    /// were kept still reads.
    #[test]
    fn a_replay_remembers_who_was_playing() {
        let level = Level::parse(
            "name: T\nposts: 1\ncrab: 0,0 R R common\nmap:\n\
             +-+-+\n|. 0|\n+ + +\n|. .|\n+-+-+\n",
        )
        .expect("level");
        let mut replay = Replay::new(level);
        replay.names[0] = "Anna".into();
        replay.names[1] = "Bo".into();
        replay.record([PlayerAction::None; MAX_PLAYERS]);
        let text = replay.to_text();
        let back = Replay::parse(&text).expect("round trip");
        assert_eq!(back.names[0], "Anna");
        assert_eq!(back.names[1], "Bo");
        assert_eq!(back.names[2], "", "a seat nobody named stays unnamed");
        assert_eq!(back.inputs.len(), 1, "and the round itself is untouched");

        // An unnamed round writes no line at all, so the file is what it
        // always was, and reads back the same way.
        let plain = Replay::new(back.level.clone());
        assert!(!plain.to_text().contains("names:"));
        let plain_back = Replay::parse(&plain.to_text()).expect("round trip");
        assert!(plain_back.names.iter().all(String::is_empty));
    }

    /// A name holding the line's own separator, or a line break, is written
    /// so the seats after it keep theirs: a bot names itself.
    #[test]
    fn a_name_cannot_break_the_names_line() {
        let mut replay = Replay::new(arena());
        replay.names[0] = "a|b\nc".into();
        replay.names[1] = "Bo".into();
        replay.record([PlayerAction::None; MAX_PLAYERS]);
        let back = Replay::parse(&replay.to_text()).expect("round trip");
        assert!(!back.names[0].contains('|'), "{:?}", back.names[0]);
        assert!(back.names[0].starts_with('a'), "{:?}", back.names[0]);
        assert_eq!(back.names[1], "Bo");
        assert_eq!(back.inputs.len(), 1);
    }

    /// What held each seat survives the trip, a couch round writes no line
    /// for it, and the line is one an older build refuses rather than
    /// skips: its level parser does not know the key.
    #[test]
    fn a_replay_remembers_what_held_each_seat() {
        let level = arena();
        let mut replay = Replay::new(level.clone());
        replay.names[0] = "Greedy".into();
        replay.kinds[0] = SeatKind::Bot;
        replay.kinds[1] = SeatKind::Ai;
        replay.record([PlayerAction::None; MAX_PLAYERS]);
        let text = replay.to_text();
        assert!(text.contains("kinds: bot|ai|human"));
        let back = Replay::parse(&text).expect("round trip");
        assert_eq!(
            back.kinds[..3],
            [SeatKind::Bot, SeatKind::Ai, SeatKind::Human]
        );
        assert!(!Replay::new(level).to_text().contains("kinds:"));
        let level_part = text
            .split_once("kinds:")
            .map(|(_, rest)| rest)
            .expect("kinds");
        assert!(Level::parse(&format!("kinds:{level_part}")).is_err());
        assert!(Replay::parse(&text.replace("kinds: bot", "kinds: robot")).is_err());
    }

    /// A recording longer than any round could be is refused when read,
    /// not played through.
    #[test]
    fn a_recording_longer_than_any_round_is_refused() {
        let level = Level::parse(
            "name: T\nposts: 1\ncrab: 0,0 R R common\nmap:\n\
             +-+-+\n|. 0|\n+ + +\n|. .|\n+-+-+\n",
        )
        .expect("level");
        let mut replay = Replay::new(level);
        for _ in 0..MAX_TICKS {
            replay.record([PlayerAction::None; MAX_PLAYERS]);
        }
        let longest = Replay::parse(&replay.to_text()).expect("the longest that reads");
        assert_eq!(longest.inputs.len(), MAX_TICKS);
        replay.record([PlayerAction::None; MAX_PLAYERS]);
        assert!(Replay::parse(&replay.to_text()).is_err(), "one tick over");
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(Replay::parse("not a replay").is_err());
        assert!(Replay::parse("").is_err());
    }

    use crate::sim::direction::Direction;
    use crate::sim::{CrabKind, Handedness, Spawner, TileKind};

    fn arena() -> Level {
        let mut board = Board::new(12, 9, 77);
        board.set_tile(11, 4, TileKind::Castle(0));
        board.set_tile(
            0,
            4,
            TileKind::Spawner(Spawner {
                dir: Direction::Right,
                period: 30,
            }),
        );
        board.spawn_crab(3, 3, Direction::Down, Handedness::Right, CrabKind::Giant);
        board.spawn_gull(6, 6, Direction::Up);
        board.rules_mut().gull_period = 200;
        Level::from_board("Arena", 3, board)
    }

    #[test]
    fn replay_reproduces_the_match_bit_for_bit() {
        let level = arena();
        let mut replay = Replay::new(level.clone());
        let mut live = level.board();
        for t in 0u32..900 {
            let mut actions = [PlayerAction::None; MAX_PLAYERS];
            if t % 40 == 5 {
                actions[0] = PlayerAction::Place {
                    x: (t % 12) as u8,
                    y: 5,
                    dir: Direction::Up,
                };
            }
            if t % 90 == 30 {
                actions[1] = PlayerAction::Remove {
                    x: (t % 12) as u8,
                    y: 5,
                };
            }
            replay.record(actions);
            live.tick(&actions);
        }
        // Round-trip through the text format, then play back.
        let text = replay.to_text();
        let parsed = Replay::parse(&text).unwrap_or_else(|e| panic!("parse: {e}"));
        assert_eq!(parsed.inputs.len(), 900);
        let replayed = parsed.playback();
        assert_eq!(
            replayed.state_hash(),
            live.state_hash(),
            "replay must reproduce the live match exactly"
        );
    }
}
