//! Replays (spec §7.7): the starting level plus the per-tick input list.
//! Playing it back through `Board::tick` reproduces the match bit-for-bit.
//!
//! Text format: a `replay-v1` header, an optional `names:` line, the level
//! text, then `inputs:` with one
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
    /// business to supply: a round played online was played by whoever the
    /// table agreed on, and a replay that fell back to the local couch
    /// names put this machine's P1 on somebody else's crabs.
    pub names: [String; MAX_PLAYERS],
}

const HEADER: &str = "replay-v1";
const INPUTS_MARK: &str = "inputs:";
/// Seat names, one line, `|` between them. Read before the level text
/// rather than inside it: the level format knows nothing about who was
/// holding the controller, and should not have to.
const NAMES_MARK: &str = "names:";

impl Replay {
    pub fn new(level: Level) -> Replay {
        Replay {
            level,
            inputs: Vec::new(),
            names: Default::default(),
        }
    }

    /// Record who was playing. Called once, at the moment the round starts,
    /// because that is when the shell knows.
    pub fn named(mut self, names: [String; MAX_PLAYERS]) -> Replay {
        self.names = names;
        self
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
            let _ = writeln!(out, "{NAMES_MARK} {}", self.names.join("|"));
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
            .ok_or("not a replay-v1 file")?
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Who was playing survives the trip, and a file written before names
    /// were kept still reads.
    #[test]
    fn a_replay_remembers_who_was_playing() {
        let level = Level::parse(
            "name: T\nposts: 1\ncrab: 0,0 R R common\nmap:\n\
             +-+-+\n|. 0|\n+ + +\n|. .|\n+-+-+\n",
        )
        .expect("level");
        let mut replay = Replay::new(level).named([
            "Anna".into(),
            "Bo".into(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]);
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

    #[test]
    fn parse_rejects_garbage() {
        assert!(super::Replay::parse("not a replay").is_err());
        assert!(super::Replay::parse("").is_err());
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
        board.set_gull_period(200);
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
