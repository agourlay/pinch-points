//! A round put down mid-play, and picked up again.
//!
//! The board itself travels as a [`Board::to_snapshot`], which is exact to
//! the last sub-tile and PRNG draw. What that cannot carry is the few things
//! the *shell* knows and the sim does not: how many seats are in play and
//! which of them an AI is holding. Those ride in a short header above it, and
//! this module is the whole of what a suspended round is.
//!
//! The same bytes go two ways. Written to [`save_path`] they are yesterday's
//! round waiting on the menu; put through [`crate::share`] they are a code
//! somebody else can pick the beach up from.

use crate::sim::{Board, BotLevel, MAX_PLAYERS};
use bevy::prelude::*;

/// Where a put-down round waits. One slot: picking a round back up is
/// resuming *the* round you left, not choosing from a shelf.
/// The put-down round, under the XDG data directory.
fn save_path() -> std::path::PathBuf {
    crate::app::paths::data_dir().join("suspended.txt")
}

const HEADER: &str = "suspended-v1";

/// A round in progress, whole.
pub struct Suspended {
    /// Seats in play, which the board's castles imply but the shell reads
    /// from here rather than re-deriving.
    pub seats: u8,
    /// Which seats an AI holds, and how fierce.
    pub bots: [Option<BotLevel>; MAX_PLAYERS],
    pub board: Board,
}

impl Suspended {
    pub fn to_text(&self) -> String {
        let bots: Vec<&str> = self.bots.iter().map(|slot| bot_token(*slot)).collect();
        format!(
            "{HEADER}\nseats: {}\nbots: {}\n{}",
            self.seats,
            bots.join(" "),
            self.board.to_snapshot()
        )
    }

    /// Read one back, or say why it is not one.
    pub fn parse(text: &str) -> Result<Suspended, String> {
        let mut seats = None;
        let mut bots = [None; MAX_PLAYERS];
        let mut lines = text.lines();
        match lines.next().map(str::trim) {
            Some(HEADER) => {}
            Some(other) => return Err(format!("not a suspended round: {other:?}")),
            None => return Err("nothing to resume".to_string()),
        }
        // The header is this module's; everything from `snapshot-v1` on is
        // the board's own business.
        let mut rest = String::new();
        for line in lines {
            if !rest.is_empty() {
                rest.push('\n');
                rest.push_str(line);
                continue;
            }
            match line.trim().split_once(':') {
                Some(("seats", value)) => {
                    seats = Some(
                        value
                            .trim()
                            .parse::<u8>()
                            .map_err(|_| format!("seats: bad number {value:?}"))?,
                    );
                }
                Some(("bots", value)) => {
                    for (seat, token) in value.split_whitespace().enumerate() {
                        let slot = bots
                            .get_mut(seat)
                            .ok_or_else(|| format!("bots: seat {seat} is past the table"))?;
                        *slot = bot_from_token(token)
                            .ok_or_else(|| format!("bots: bad level {token:?}"))?;
                    }
                }
                _ => rest.push_str(line),
            }
        }
        let board = Board::parse_snapshot(&rest)?;
        let seats = seats.ok_or("a suspended round says how many seats it has")?;
        if seats < 2 || usize::from(seats) > MAX_PLAYERS {
            return Err(format!("a round cannot be played by {seats}"));
        }
        Ok(Suspended { seats, bots, board })
    }
}

/// `-` for a human seat, otherwise the level's own name.
fn bot_token(level: Option<BotLevel>) -> &'static str {
    match level {
        None => "-",
        Some(BotLevel::Easy) => "easy",
        Some(BotLevel::Normal) => "normal",
        Some(BotLevel::Hard) => "fierce",
    }
}

fn bot_from_token(token: &str) -> Option<Option<BotLevel>> {
    match token {
        "-" => Some(None),
        "easy" => Some(Some(BotLevel::Easy)),
        "normal" => Some(Some(BotLevel::Normal)),
        "fierce" => Some(Some(BotLevel::Hard)),
        _ => None,
    }
}

/// Whether a round is waiting to be picked up.
pub fn waiting() -> bool {
    save_path().exists()
}

/// Put the round down. Best-effort, like the other save files.
pub fn put_down(round: &Suspended) -> Result<(), String> {
    crate::app::paths::write_atomic(&save_path(), round.to_text()).map_err(|e| e.to_string())
}

/// Pick it up, and take it off the shelf as we go: a resumed round is being
/// played again, and leaving the file would offer the same stale beach after
/// the tide has come in on it.
pub fn pick_up() -> Result<Suspended, String> {
    let path = save_path();
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let round = Suspended::parse(&text)?;
    let _ = std::fs::remove_file(path);
    Ok(round)
}

/// The round in progress, as a share code on the clipboard (`C`).
///
/// The same bytes the save slot holds, so a code somebody pastes puts them
/// on the beach you were standing on: mid-stride crabs, standing signposts,
/// the PRNG where you left it.
pub fn copy_round_code(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<crate::app::settings::GameSettings>,
    sim: Res<crate::app::Sim>,
    seats: Res<crate::app::Seats>,
    bots: Res<crate::app::Bots>,
    mut clipboard: ResMut<Clipboard>,
    mut feed: ResMut<crate::app::side_panels::EventLog>,
) {
    if !settings.keycaps.just_pressed(&keys, 'C') {
        return;
    }
    let round = Suspended {
        seats: seats.0,
        bots: bots.0,
        board: sim.0.clone(),
    };
    let tr = settings.tr();
    let line = crate::app::codes::copy_feedback(
        &mut clipboard,
        tr,
        crate::share::Kind::Beach,
        round.to_text().as_bytes(),
        tr.round_copied,
    );
    feed.push(line, crate::app::palette::PARCHMENT);
}

/// A round pasted onto the menu (`V`): somebody else's beach, picked up
/// where they left it.
///
/// Takes the pasted code rather than the clipboard so the three ways it can
/// fail (nothing readable, a code of the wrong sort, a round this build
/// cannot read) each get their own sentence and their own test.
pub fn round_from(
    pasted: Option<(crate::share::Kind, Vec<u8>)>,
    tr: &crate::app::i18n::Tr,
) -> Result<Suspended, String> {
    let text =
        crate::app::codes::payload_text(pasted, tr, crate::share::Kind::Beach, tr.round_code_bad)?;
    Suspended::parse(&text).map_err(|_| tr.round_code_bad.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{PlayerAction, bot_action, generate_arena};

    fn played_round() -> Suspended {
        let mut board = generate_arena(11, 4, 12, 9);
        // Play a while, so the board carries sub-tile positions, standing
        // signposts and a PRNG mid-stream: the parts a level cannot hold.
        for _ in 0..400 {
            let mut actions = [PlayerAction::None; MAX_PLAYERS];
            for seat in 1..4u8 {
                actions[seat as usize] = bot_action(&board, seat, BotLevel::Normal);
            }
            board.tick(&actions);
        }
        Suspended {
            seats: 4,
            bots: [
                None,
                Some(BotLevel::Normal),
                Some(BotLevel::Hard),
                Some(BotLevel::Easy),
                None,
                None,
            ],
            board,
        }
    }

    /// The round comes back the same round: the board to the last draw, and
    /// the seating with it. Resuming to a different beach, or to bots that
    /// changed difficulty, is the whole failure this format exists to avoid.
    #[test]
    fn a_put_down_round_is_picked_up_unchanged() {
        let round = played_round();
        let back = Suspended::parse(&round.to_text()).expect("its own output");
        assert_eq!(
            back.board.state_hash(),
            round.board.state_hash(),
            "the beach changed"
        );
        assert_eq!(back.seats, round.seats);
        assert_eq!(back.bots, round.bots);
        // And it keeps playing the same round from there.
        let (mut a, mut b) = (round.board, back.board);
        for tick in 0..120 {
            let mut actions = [PlayerAction::None; MAX_PLAYERS];
            for seat in 1..4u8 {
                actions[seat as usize] = bot_action(&a, seat, BotLevel::Normal);
            }
            a.tick(&actions);
            b.tick(&actions);
            assert_eq!(a.state_hash(), b.state_hash(), "diverged at tick {tick}");
        }
    }

    /// The three ways a pasted code is not a round, each answered its own
    /// way. Takes the pasted bytes rather than the clipboard, so no test
    /// ever reads what the person running the suite had copied.
    #[test]
    fn what_is_not_a_round_says_which_way_it_is_not() {
        let tr = &crate::app::i18n::EN;
        let round = played_round();

        // The real thing comes back playable.
        let code = crate::share::encode(crate::share::Kind::Beach, round.to_text().as_bytes());
        let pasted = crate::share::decode(&code);
        let back = round_from(pasted, tr).expect("its own code");
        assert_eq!(back.board.state_hash(), round.board.state_hash());
        assert_eq!(back.seats, round.seats);

        // Nothing on the clipboard.
        assert_eq!(
            round_from(None, tr).err().as_deref(),
            Some(tr.code_none_pasted)
        );

        // A code, but of another sort: say which, since "bad code" leaves a
        // player guessing whether they copied the wrong thing.
        let level = crate::share::encode(crate::share::Kind::Level, b"name: x");
        let complaint = round_from(crate::share::decode(&level), tr)
            .err()
            .expect("a level is not a round");
        assert!(complaint.contains(tr.code_kind_level), "{complaint}");
        assert!(complaint.contains(tr.code_kind_beach), "{complaint}");

        // The right sort of code, carrying something this build cannot read.
        let junk = crate::share::encode(crate::share::Kind::Beach, b"suspended-v9\n");
        assert_eq!(
            round_from(crate::share::decode(&junk), tr).err().as_deref(),
            Some(tr.round_code_bad)
        );
    }

    #[test]
    fn what_is_not_a_suspended_round_is_refused() {
        assert!(Suspended::parse("").is_err(), "empty");
        assert!(Suspended::parse("hello").is_err(), "not ours");
        let good = played_round().to_text();
        assert!(
            Suspended::parse(&good.replace("suspended-v1", "suspended-v2")).is_err(),
            "another version"
        );
        assert!(
            Suspended::parse(&good.replace("seats: 4", "seats: 9")).is_err(),
            "more seats than the table has"
        );
        assert!(
            Suspended::parse(&good.replace("bots: - normal", "bots: - wizard")).is_err(),
            "a difficulty this build does not know"
        );
        // The board underneath still has to be a board.
        assert!(
            Suspended::parse(&good.replace("snapshot-v1", "snapshot-v9")).is_err(),
            "a board this build cannot read"
        );
    }
}
