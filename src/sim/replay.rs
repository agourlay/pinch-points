//! Replays (spec §7.7): the starting level plus the per-tick input list.
//! Playing it back through `Board::tick` reproduces the match bit-for-bit.
//!
//! Text format: a `replay-v3` header (`replay-v2` still read), an
//! optional `names:` line, an
//! optional `kinds:` line, an optional `ai:` line, an optional `teams:`
//! line, an optional `scores:` line, the level text, then `inputs:` with one
//! line per tick: six hex digits per seat (full x, full y, op/dir byte), so
//! `MAX_PLAYERS` of them, where a bare `.` is the common all-idle tick. A
//! full byte per axis, like the wire (spec §7.6), so boards wider than 16
//! tiles replay exactly.

use crate::sim::BotLevel;
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
    /// The level each of the game's AI seats played at, `None` for every
    /// other seat.
    ///
    /// Playback needs none of it, since it replays the inputs the AI made,
    /// but who beat whom does: the shelf says which AI took a round. A
    /// recording made before levels were kept has `None` for its AI seats
    /// too, and is shown as the AI without one.
    pub ai: [Option<BotLevel>; MAX_PLAYERS],
    /// How the round was scored: the scoring mode's index
    /// (`app::teams::TeamMode::index`), 0 for every seat for itself.
    ///
    /// The sim never knows about teams, but who won does: watched with
    /// the watcher's own setting, a round played in pairs was crowned to
    /// its top single seat, and filed under one.
    pub teams: u8,
    /// Every seat's score when the tide came in, or `None` for a recording
    /// that does not say (one made before scores were kept, or one cut off
    /// before its end).
    ///
    /// Playback needs none of it: the inputs reproduce it. The shelf does,
    /// to say what the winner won with, and it reads only a file's head:
    /// playing every kept round through to its last tick to list them
    /// costs a whole round's ticks for each.
    pub scores: Option<[u32; MAX_PLAYERS]>,
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

/// What every header starts with; the version follows it.
const HEADER_STEM: &str = "replay-v";
/// The version this build writes: the rules a recording was played under,
/// and the shape of its head.
///
/// A replay is inputs, and inputs only mean what the rules say they mean:
/// the roulette picks a face with one draw over `TideEvent::ALL`, so
/// adding a face in v2 (Right Claws) moved every draw after it. A v1
/// recording fed to these rules would not diverge loudly, it would quietly
/// play out a different round. Refused instead.
///
/// v3 plays under v2's rules and adds the `scores:` line, written for every
/// finished round. A v2 build reads the head's lines into the level parser,
/// which refuses the first key it does not know: under a v2 header that
/// was a level error about `scores`, under v3 it is the old build's own
/// "not a replay this build can play".
const VERSION: u32 = 3;
/// The oldest version this build plays: the first under today's rules.
/// Every line a v3 head may hold is optional, so a v2 file reads as one
/// with or without them.
const OLDEST: u32 = 2;
const INPUTS_MARK: &str = "inputs:";
/// Seat names, one line, `|` between them. Read before the level text
/// rather than inside it: the level format knows nothing about who was
/// holding the controller, and should not have to.
const NAMES_MARK: &str = "names:";
/// Seat kinds, one line, `|` between them, after the names.
///
/// A line inside the head, by the rule the format keeps: a build from
/// before kinds were kept hands this line to the level parser, which
/// refuses a key it does not know, so an older game refuses such a
/// recording outright rather than half-reading it. Written only when some
/// seat was not a person.
const KINDS_MARK: &str = "kinds:";
/// The AI seats' levels, one line, `|` between them, `-` for a seat the
/// AI did not hold, after the kinds: written, by the same rule as the
/// kinds, only when the AI held some seat.
const AI_MARK: &str = "ai:";
/// What stands in the `ai:` line for a seat the AI did not hold.
const NO_AI: &str = "-";
/// The scoring mode, one number, after the kinds: written, by the same
/// rule as the kinds, only for a round played in teams.
const TEAMS_MARK: &str = "teams:";
/// Every seat's final score, one line, `|` between them, after the teams:
/// written whenever the recording was finished with its final board at
/// hand, under a v3 header (see [`VERSION`]).
const SCORES_MARK: &str = "scores:";

/// Why a recording a later build wrote is refused: its rules or its head
/// are ones this build does not know. Named so the shell can say it in the
/// player's language (`app::replays::why_not`).
pub const NEEDS_NEWER: &str = "this replay needs a newer version of the game";

/// What a recording calls a seat: the name as given, its line's separator
/// turned to `/`, control characters to spaces, and the ends trimmed.
///
/// The one spelling of a seat's name, for the writer, the reader and the
/// round being played: a round is filed under its winner's name as the
/// card says it, and the shelf finds the winner's seat again by spelling
/// every seat's name from the file. A trailing space the reader trimmed
/// and the filing kept put "Bo " on the shelf as "Bo_", which is how
/// "Bo!" spells, and the tag went to the wrong seat.
pub fn seat_name(raw: &str) -> String {
    raw.chars()
        .map(|c| match c {
            '|' => '/',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

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
            ai: [None; MAX_PLAYERS],
            teams: 0,
            scores: None,
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
        let _ = writeln!(out, "{HEADER_STEM}{VERSION}");
        // Written only when there is something to say: an unnamed local
        // round carries no names line.
        if self.names.iter().any(|n| !n.is_empty()) {
            // A name is somebody's to choose, a bot's included: the
            // separator and line breaks in one would cost the seats after
            // it their names, or the file its shape.
            let names: Vec<String> = self.names.iter().map(|n| seat_name(n)).collect();
            let _ = writeln!(out, "{NAMES_MARK} {}", names.join("|"));
        }
        if self.kinds.iter().any(|k| *k != SeatKind::Human) {
            let kinds: Vec<&str> = self.kinds.iter().map(|k| k.token()).collect();
            let _ = writeln!(out, "{KINDS_MARK} {}", kinds.join("|"));
        }
        if self.ai.iter().any(Option::is_some) {
            let ai: Vec<&str> = self
                .ai
                .iter()
                .map(|level| level.map_or(NO_AI, BotLevel::token))
                .collect();
            let _ = writeln!(out, "{AI_MARK} {}", ai.join("|"));
        }
        if self.teams != 0 {
            let _ = writeln!(out, "{TEAMS_MARK} {}", self.teams);
        }
        if let Some(scores) = &self.scores {
            let scores: Vec<String> = scores.iter().map(u32::to_string).collect();
            let _ = writeln!(out, "{SCORES_MARK} {}", scores.join("|"));
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
        let (mut replay, input_text) = Self::parse_head(text)?;
        for line in input_text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if replay.inputs.len() == MAX_TICKS {
                return Err(format!(
                    "longer than the {MAX_TICKS} ticks a round can last"
                ));
            }
            if line == "." {
                replay.inputs.push([PlayerAction::None; MAX_PLAYERS]);
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
            replay.inputs.push(actions);
        }
        Ok(replay)
    }

    /// Everything in a recording but its inputs: who played, at what
    /// table, on which beach. Read as [`parse`](Self::parse) reads it, so
    /// a file either side refuses the other refuses too, but without
    /// decoding a tick: the replay shelf reads every kept round's head
    /// when it is listed, and never needs the round itself.
    pub fn head(text: &str) -> Result<Replay, String> {
        Self::parse_head(text).map(|(head, _)| head)
    }

    /// The head, with no inputs yet, and the text of the inputs after it.
    fn parse_head(text: &str) -> Result<(Replay, &str), String> {
        let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
        let version = first
            .trim_end_matches('\r')
            .strip_prefix(HEADER_STEM)
            .and_then(|version| version.parse::<u32>().ok());
        match version {
            Some(version) if (OLDEST..=VERSION).contains(&version) => {}
            // A later build's recording: its rules or its head are ones
            // this build does not know, so it is refused, saying why.
            Some(version) if version > VERSION => return Err(NEEDS_NEWER.to_string()),
            Some(_) | None => return Err("not a replay this build can play".to_string()),
        }
        let rest = rest.trim_start_matches(['\r', '\n']);
        // A file written before names were kept simply has no such line,
        // and reads exactly as it always did.
        let (names, rest) = match rest.strip_prefix(NAMES_MARK) {
            Some(after) => {
                let (line, rest) = after.split_once('\n').unwrap_or((after, ""));
                let mut names: [String; MAX_PLAYERS] = Default::default();
                for (slot, name) in names.iter_mut().zip(line.trim().split('|')) {
                    *slot = seat_name(name);
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
        // A file written before levels were kept has no such line: its AI
        // seats are the AI at a level nobody wrote down.
        let (ai, rest) = match rest.strip_prefix(AI_MARK) {
            Some(after) => {
                let (line, rest) = after.split_once('\n').unwrap_or((after, ""));
                let mut ai = [None; MAX_PLAYERS];
                for (slot, token) in ai.iter_mut().zip(line.trim().split('|')) {
                    *slot = match token.trim() {
                        NO_AI => None,
                        token => Some(
                            BotLevel::from_token(token)
                                .ok_or_else(|| format!("no AI level {token:?}"))?,
                        ),
                    };
                }
                (ai, rest)
            }
            None => ([None; MAX_PLAYERS], rest),
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
        // A file written before scores were kept has no such line, and
        // says nothing about how the round ended.
        let (scores, rest) = match rest.strip_prefix(SCORES_MARK) {
            Some(after) => {
                let (line, rest) = after.split_once('\n').unwrap_or((after, ""));
                let mut scores = [0; MAX_PLAYERS];
                for (slot, score) in scores.iter_mut().zip(line.trim().split('|')) {
                    *slot = score
                        .trim()
                        .parse::<u32>()
                        .map_err(|_| format!("no score {:?}", score.trim()))?;
                }
                (Some(scores), rest)
            }
            None => (None, rest),
        };
        let (level_text, input_text) = rest
            .split_once(INPUTS_MARK)
            .ok_or("missing inputs: section")?;
        let level = Level::parse(level_text)?;
        let head = Replay {
            level,
            inputs: Vec::new(),
            names,
            kinds,
            ai,
            teams,
            scores,
        };
        Ok((head, input_text))
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

    /// The level each AI seat played at survives the trip, read whole or
    /// by its head alone; a round with no AI writes no line for it; and a
    /// recording made before levels were kept reads with its AI seats at
    /// no level, its inputs untouched.
    #[test]
    fn a_replay_remembers_each_ai_level() {
        let mut replay = Replay::new(arena());
        replay.record([PlayerAction::None; MAX_PLAYERS]);
        assert!(!replay.to_text().contains(AI_MARK), "no AI, no line");
        replay.kinds[1] = SeatKind::Ai;
        replay.kinds[3] = SeatKind::Ai;
        replay.ai[1] = Some(BotLevel::Hard);
        replay.ai[3] = Some(BotLevel::Easy);
        let text = replay.to_text();
        assert!(text.contains("ai: -|fierce|-|easy|-|-"), "{text}");
        for back in [
            Replay::parse(&text).expect("round trip"),
            Replay::head(&text).expect("its head"),
        ] {
            assert_eq!(back.ai, replay.ai);
            assert_eq!(back.kinds, replay.kinds);
        }
        assert_eq!(Replay::parse(&text).expect("whole").inputs.len(), 1);
        assert!(Replay::head(&text).expect("head").inputs.is_empty());
        // Written before levels were kept: the same file without the line.
        let old = text.replace("ai: -|fierce|-|easy|-|-\n", "");
        let back = Replay::parse(&old).expect("an older recording");
        assert_eq!(back.kinds[1], SeatKind::Ai);
        assert_eq!(back.ai, [None; MAX_PLAYERS]);
        assert_eq!(back.inputs.len(), 1);
        // A level this build does not know is refused, like a kind.
        assert!(Replay::parse(&text.replace("fierce", "furious")).is_err());
        assert!(Replay::head(&text.replace("fierce", "furious")).is_err());
    }

    /// The final scores survive the trip, read whole or by the head alone;
    /// a recording that kept none writes no line; one made before scores
    /// were kept reads with none, its inputs untouched; and the line is
    /// one an older build refuses, as it does `kinds:`.
    #[test]
    fn a_replay_remembers_how_the_round_ended() {
        let mut replay = Replay::new(arena());
        replay.teams = 1;
        replay.record([PlayerAction::None; MAX_PLAYERS]);
        assert!(
            !replay.to_text().contains(SCORES_MARK),
            "none kept, no line"
        );
        replay.scores = Some([87, 42, 30, 12, 0, 0]);
        let text = replay.to_text();
        assert!(
            text.contains("teams: 1\nscores: 87|42|30|12|0|0\n"),
            "{text}"
        );
        for back in [
            Replay::parse(&text).expect("round trip"),
            Replay::head(&text).expect("its head"),
        ] {
            assert_eq!(back.scores, replay.scores);
            assert_eq!(back.teams, 1);
        }
        assert_eq!(Replay::parse(&text).expect("whole").inputs.len(), 1);
        assert!(Replay::head(&text).expect("head").inputs.is_empty());
        let old = text.replace("scores: 87|42|30|12|0|0\n", "");
        let back = Replay::parse(&old).expect("an older recording");
        assert_eq!(back.scores, None);
        assert_eq!(back.inputs.len(), 1);
        assert!(Replay::head(&text.replace("|42|", "|x|")).is_err());
        let level_part = text
            .split_once(SCORES_MARK)
            .map(|(_, rest)| rest)
            .expect("scores");
        assert!(Level::parse(&format!("{SCORES_MARK}{level_part}")).is_err());
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

    /// This build writes v3 and reads v2 and v3 alike, a v2 file with or
    /// without the optional lines; a v1 file is refused as one it cannot
    /// play, and a later version's says a newer game is wanted.
    #[test]
    fn the_header_says_which_builds_can_play_it() {
        let mut replay = Replay::new(arena());
        replay.names[0] = "Anna".into();
        replay.scores = Some([4, 1, 0, 0, 0, 0]);
        replay.record([PlayerAction::None; MAX_PLAYERS]);
        let text = replay.to_text();
        assert!(text.starts_with("replay-v3\n"), "{text}");
        let v2 = text.replacen("replay-v3", "replay-v2", 1);
        let back = Replay::parse(&v2).expect("a v2 file with every line");
        assert_eq!(
            (back.names[0].as_str(), back.scores),
            ("Anna", replay.scores)
        );
        let bare = Replay::new(arena())
            .to_text()
            .replacen("replay-v3", "replay-v2", 1);
        let back = Replay::parse(&bare).expect("a v2 file with none");
        assert_eq!(back.scores, None);
        let crlf = text.replacen("replay-v3\n", "replay-v3\r\n", 1);
        assert!(Replay::parse(&crlf).is_ok(), "a file that crossed Windows");
        let v1 = text.replacen("replay-v3", "replay-v1", 1);
        assert_eq!(
            Replay::parse(&v1).unwrap_err(),
            "not a replay this build can play"
        );
        for later in ["replay-v4", "replay-v30"] {
            let text = text.replacen("replay-v3", later, 1);
            assert_eq!(Replay::parse(&text).unwrap_err(), NEEDS_NEWER, "{later}");
            assert!(Replay::head(&text).is_err());
        }
    }

    /// A name is spelled one way on the way out and on the way back in:
    /// trimmed, its line's separator and control characters replaced.
    #[test]
    fn a_name_reads_back_as_seat_name_spells_it() {
        let mut replay = Replay::new(arena());
        replay.names = ["Bo ", " Bo!", "a|b", "\tx", "", ""].map(String::from);
        replay.record([PlayerAction::None; MAX_PLAYERS]);
        let back = Replay::parse(&replay.to_text()).expect("round trip");
        for (raw, read) in replay.names.iter().zip(&back.names) {
            assert_eq!(read, &seat_name(raw), "{raw:?}");
        }
        assert_eq!(back.names[0], "Bo");
        assert_eq!(seat_name(&seat_name("Bo ")), "Bo", "settled once spelled");
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
