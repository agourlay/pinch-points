//! A board rebuilt from what a bot is shown: `hello` and one `tick`.
//!
//! This is the Rust starter kit's way in. A Rust bot links the crate and
//! runs the sim forward itself, from a [`Board`] built here, and the board
//! knows exactly what the `simulate` request knows: the observation carries
//! no PRNG state and no takeoff countdowns, so what they would decide is
//! drawn the way [`Board::lookahead`] draws it.

use super::lookahead::LOOKAHEAD_SEED;
use crate::sim::{Board, CrabKind, MAX_PLAYERS, Pcg32};
use serde_json::Value;
use std::fmt::Write;

fn field<'a>(v: &'a Value, path: &str) -> Result<&'a Value, String> {
    path.split('.')
        .try_fold(v, |v, key| v.get(key))
        .ok_or_else(|| format!("no `{path}`"))
}

fn num(v: &Value, path: &str) -> Result<u64, String> {
    field(v, path)?
        .as_u64()
        .ok_or_else(|| format!("`{path}` is not a number"))
}

fn text<'a>(v: &'a Value, path: &str) -> Result<&'a str, String> {
    field(v, path)?
        .as_str()
        .ok_or_else(|| format!("`{path}` is not a string"))
}

fn letter(token: &str) -> Result<char, String> {
    Ok(match token {
        "up" => 'U',
        "down" => 'D',
        "left" => 'L',
        "right" => 'R',
        other => return Err(format!("{other:?} is not a direction")),
    })
}

fn claw(token: &str) -> Result<&'static str, String> {
    match token {
        "left" => Ok("L"),
        "right" => Ok("R"),
        other => Err(format!("{other:?} is not a claw")),
    }
}

/// Walls as the snapshot writes them: hex nibbles, low bit first.
fn hex(bits: &[bool]) -> String {
    bits.chunks(4)
        .map(|chunk| {
            let nibble = chunk
                .iter()
                .enumerate()
                .fold(0u32, |acc, (i, &bit)| acc | (u32::from(bit) << i));
            char::from_digit(nibble, 16).unwrap_or('0')
        })
        .collect()
}

/// The board `hello` and `tick` describe, ready to run forward.
pub fn board_from(hello: &Value, tick: &Value) -> Result<Board, String> {
    let w = num(hello, "board.width")? as usize;
    let h = num(hello, "board.height")? as usize;
    let now = num(tick, "tick")?;
    let rows: Vec<&str> = field(hello, "board.tiles")?
        .as_array()
        .ok_or("`board.tiles` is not a list")?
        .iter()
        .map(|r| r.as_str().unwrap_or(""))
        .collect();
    if rows.len() != h || rows.iter().any(|r| r.chars().count() != w) {
        return Err("`board.tiles` is not width by height".into());
    }
    let mut tiles: Vec<String> = Vec::with_capacity(w * h);
    for row in &rows {
        for c in row.chars() {
            tiles.push(match c {
                '.' => ".".into(),
                '#' => "#".into(),
                'K' => "K".into(),
                '~' => "~".into(),
                // Turnstiles, castles and spawners are filled in below from
                // the lists that say more than one character can.
                'T' => "T".into(),
                'S' => ".".into(),
                d if d.is_ascii_digit() => format!("c{d}"),
                other => return Err(format!("no tile {other:?}")),
            });
        }
    }
    let at = |x: u64, y: u64| (y as usize) * w + x as usize;
    for s in field(hello, "board.spawners")?
        .as_array()
        .into_iter()
        .flatten()
    {
        let i = at(num(s, "x")?, num(s, "y")?);
        *tiles.get_mut(i).ok_or("a spawner off the board")? =
            format!("s{}{}", letter(text(s, "dir")?)?, num(s, "period")?);
    }
    for c in field(tick, "castles")?.as_array().into_iter().flatten() {
        let i = at(num(c, "x")?, num(c, "y")?);
        *tiles.get_mut(i).ok_or("a castle off the board")? = format!("c{}", num(c, "owner")?);
    }
    for t in field(tick, "turnstiles")?.as_array().into_iter().flatten() {
        let i = at(num(t, "x")?, num(t, "y")?);
        let next = if text(t, "next")? == "right" {
            "T"
        } else {
            "t"
        };
        *tiles.get_mut(i).ok_or("a turnstile off the board")? = next.into();
    }
    let walls = |key: &str, wall: char| -> Result<Vec<bool>, String> {
        Ok(field(hello, &format!("board.walls.{key}"))?
            .as_array()
            .ok_or("walls are lists")?
            .iter()
            .flat_map(|row| row.as_str().unwrap_or("").chars().map(move |c| c == wall))
            .collect())
    };
    let (h_walls, v_walls) = (walls("h", '-')?, walls("v", '|')?);
    if h_walls.len() != (h + 1) * w || v_walls.len() != h * (w + 1) {
        return Err("the walls are not the board's size".into());
    }

    let mut out = String::new();
    let rng = Pcg32::new(LOOKAHEAD_SEED, 0x0005_eaba_55ed).hash_state();
    let _ = writeln!(out, "snapshot-v1\nsize: {w} {h}\nseed: {LOOKAHEAD_SEED}");
    let _ = writeln!(out, "rng: {} {}\ntick: {now}", rng.0, rng.1);
    let _ = writeln!(
        out,
        "rule: {} {}",
        text(hello, "rules.cap_policy")?,
        num(hello, "rules.signpost_cap")?
    );
    let mut posts: Vec<&Value> = field(tick, "signposts")?
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    // The oldest post has the lowest sequence number: it is the one the cap
    // takes first.
    posts.sort_by_key(|p| std::cmp::Reverse(p["age"].as_u64().unwrap_or(0)));
    let crabs: Vec<&Value> = field(tick, "crabs")?
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    let gulls: Vec<&Value> = field(tick, "gulls")?
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    let next_id = |list: &[&Value]| {
        list.iter()
            .filter_map(|c| c["id"].as_u64())
            .max()
            .map_or(0, |m| m + 1)
    };
    let _ = writeln!(
        out,
        "counters: {} {} {} 0 0",
        posts.len(),
        next_id(&crabs),
        next_id(&gulls)
    );
    let mut scores = [0u64; MAX_PLAYERS];
    for (slot, s) in scores
        .iter_mut()
        .zip(field(tick, "scores")?.as_array().into_iter().flatten())
    {
        *slot = s.as_u64().unwrap_or(0);
    }
    let scores: Vec<String> = scores.iter().map(u64::to_string).collect();
    let _ = writeln!(out, "scores: {}", scores.join(" "));
    let _ = writeln!(out, "gull_period: {}", num(hello, "rules.gull_period")?);
    if let Some(cap) = field(hello, "rules.gull_cap")?.as_u64() {
        let _ = writeln!(out, "gull_cap: {cap}");
    }
    if field(hello, "rules.gull_turnover")?.as_bool() == Some(true) {
        let _ = writeln!(out, "gull_turnover: on");
    }
    if let Some(every) = field(hello, "rules.golden_every")?
        .as_u64()
        .filter(|&n| n > 0)
    {
        let _ = writeln!(out, "golden_every: {every}");
    }
    if let Some(round) = field(hello, "rules.round_ticks")?.as_u64() {
        let _ = writeln!(out, "round: {round}");
    }
    if field(hello, "board.wrap")?.as_bool() == Some(true) {
        let _ = writeln!(out, "wrap: on");
    }
    if field(hello, "rules.castle_raids")?.as_bool() == Some(false) {
        let _ = writeln!(out, "raids: off");
    }
    if field(hello, "rules.events")?.as_bool() == Some(true) {
        let _ = writeln!(out, "events: on");
    }
    if let Some(call) = tick.get("golden_call").filter(|c| !c.is_null()) {
        let _ = writeln!(
            out,
            "golden_call: {} {} {}",
            num(call, "tick")?,
            num(call, "x")?,
            num(call, "y")?
        );
    }
    if let Some(lure) = tick.get("lure").filter(|l| !l.is_null()) {
        let _ = writeln!(
            out,
            "lure: {} {}",
            num(lure, "owner")?,
            num(lure, "ticks_left")?
        );
    }
    // Read as 0 from a listener older than the field, which is what it
    // always was read as.
    let quiet = tick
        .get("lure_cooldown")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if quiet > 0 {
        let _ = writeln!(out, "cooldown: {quiet}");
    }
    if let Some(event) = tick.get("event").filter(|e| !e.is_null()) {
        let left = num(event, "ticks_left")?;
        match text(event, "name")? {
            "crab_mania" => {
                let _ = writeln!(out, "mania: crab {left}");
            }
            "gull_mania" => {
                let _ = writeln!(out, "mania: gull {left}");
            }
            "speed_up" => {
                let _ = writeln!(out, "tempo: fast {left}");
            }
            "slow_down" => {
                let _ = writeln!(out, "tempo: slow {left}");
            }
            "right_claws" => {
                let _ = writeln!(out, "claw_call: {left}");
            }
            other => return Err(format!("no timed event {other:?}")),
        }
    }
    // The roulette's quiet spell after an event is a clock anyone can read
    // off the last event's tick.
    if let Some(last) = tick.get("last_event").filter(|e| !e.is_null()) {
        let fired = num(last, "tick")?;
        // An event fires during tick `fired`, after that tick's count
        // down, so the board one tick on still has the whole spell left.
        let since = now.saturating_sub(fired).saturating_sub(1);
        let quiet = u64::from(crate::sim::EVENT_COOLDOWN).saturating_sub(since);
        if quiet > 0 {
            let _ = writeln!(out, "event_cooldown: {quiet}");
        }
        let index = crate::sim::TideEvent::ALL
            .iter()
            .position(|&e| super::protocol::event_token(e) == text(last, "name").unwrap_or(""))
            .ok_or("no such event")?;
        let _ = writeln!(out, "last_event: {index} {fired}");
    }
    let _ = writeln!(out, "hwalls: {}\nvwalls: {}", hex(&h_walls), hex(&v_walls));
    let _ = writeln!(out, "tiles: {}", tiles.join(" "));
    for (seq, p) in posts.iter().enumerate() {
        let health = if p["worn"].as_bool() == Some(true) {
            "worn"
        } else {
            "full"
        };
        let _ = writeln!(
            out,
            "post: {} {} {} {health} {seq} {}",
            at(num(p, "x")?, num(p, "y")?),
            letter(text(p, "dir")?)?,
            num(p, "owner")?,
            now.saturating_sub(num(p, "age")?)
        );
    }
    for c in &crabs {
        let tile = at(num(c, "x")?, num(c, "y")?);
        let dir = letter(text(c, "dir")?)?;
        let progress = num(c, "progress")?;
        let kind = text(c, "kind")?;
        if CrabKind::from_token(kind).is_none() {
            return Err(format!("no crab kind {kind:?}"));
        }
        let _ = writeln!(
            out,
            "crab: {} {tile} {dir} {progress} {tile} {progress} {dir} {} {kind}",
            num(c, "id")?,
            claw(text(c, "claw")?)?,
        );
    }
    for g in &gulls {
        let tile = at(num(g, "x")?, num(g, "y")?);
        let dir = letter(text(g, "dir")?)?;
        let progress = num(g, "progress")?;
        let (state, hops) = match text(g, "state")? {
            "flying" => ("fly", num(g, "hops_left")?),
            _ => ("walk", 0),
        };
        // The countdown is not shown; the lookahead draws a fresh one.
        let _ = writeln!(
            out,
            "gull: {} {tile} {dir} {progress} {tile} {progress} {dir} {} {state} {hops} {}",
            num(g, "id")?,
            claw(text(g, "claw")?)?,
            crate::sim::TICKS_PER_SECOND * 5,
        );
    }
    Ok(Board::parse_snapshot(&out)?.lookahead(LOOKAHEAD_SEED))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::protocol::{self, Clock, SeatKind, Table, You};
    use crate::sim::{PlayerAction, classic_arena, generate_arena};

    /// The board a bot rebuilds plays forward exactly as the listener's own
    /// lookahead does: same crabs, same banks, same scores.
    fn rebuilt_matches_the_lookahead(mut board: Board, warmup: u32) {
        let names = vec!["a".to_string(); usize::from(board.seats_in_play())];
        let kinds = vec![SeatKind::Bot; names.len()];
        let table = Table {
            game: 1,
            names: &names,
            kinds: &kinds,
            clock: Clock {
                live: false,
                deadline_ms: 33,
                input_delay: 0,
            },
            cursor: false,
        };
        for _ in 0..warmup {
            let mut actions = [PlayerAction::None; MAX_PLAYERS];
            actions[0] = crate::sim::bot_action(&board, 0, crate::sim::BotLevel::Hard);
            actions[1] = crate::sim::bot_action(&board, 1, crate::sim::BotLevel::Normal);
            board.tick(&actions);
        }
        let hello = protocol::hello(&board, &table, 0, false);
        let tick = protocol::tick(
            &board,
            1,
            &You {
                seat: 0,
                last: None,
            },
            &[],
        );
        let mut theirs = board_from(&hello, &tick).expect("rebuilds");
        let mut ours = board.lookahead(LOOKAHEAD_SEED);
        for _ in 0..300 {
            theirs.tick_idle();
            ours.tick_idle();
            assert_eq!(theirs.scores(), ours.scores(), "at tick {}", ours.ticks());
            let place = |b: &Board| -> Vec<(u32, u16, u16)> {
                b.crabs()
                    .iter()
                    .map(|c| (c.id, c.tile, c.progress))
                    .collect()
            };
            assert_eq!(place(&theirs), place(&ours), "at tick {}", ours.ticks());
        }
    }

    #[test]
    fn a_classic_beach_rebuilds_from_what_a_bot_sees() {
        rebuilt_matches_the_lookahead(classic_arena(false, 2), 500);
    }

    #[test]
    fn a_generated_beach_rebuilds_from_what_a_bot_sees() {
        rebuilt_matches_the_lookahead(generate_arena(11, 4, 16, 11), 900);
    }

    /// The quiet spells after a lure and after a tide event are clocks
    /// anyone watching can read, and the rebuilt board keeps both to the
    /// tick: a molt banked in the lure's quiet spell starts no lure, and
    /// the roulette does not spin a tick early.
    #[test]
    fn the_quiet_spells_rebuild_to_the_tick() {
        let mut board = classic_arena(false, 2);
        for _ in 0..100 {
            board.tick_idle();
        }
        let fired = board.ticks() - 1;
        let text = format!(
            "{}events: on\ncooldown: 400\nevent_cooldown: {}\nlast_event: 0 {fired}\n",
            board.to_snapshot(),
            crate::sim::EVENT_COOLDOWN
        );
        let board = Board::parse_snapshot(&text).expect("a board in two quiet spells");
        let names = vec!["a".to_string(); 2];
        let kinds = vec![SeatKind::Bot; 2];
        let table = Table {
            game: 1,
            names: &names,
            kinds: &kinds,
            clock: Clock {
                live: false,
                deadline_ms: 33,
                input_delay: 0,
            },
            cursor: false,
        };
        let hello = protocol::hello(&board, &table, 0, false);
        let tick = protocol::tick(
            &board,
            1,
            &You {
                seat: 0,
                last: None,
            },
            &[],
        );
        let theirs = board_from(&hello, &tick).expect("rebuilds").to_snapshot();
        let ours = board.lookahead(LOOKAHEAD_SEED).to_snapshot();
        let line = |snapshot: &str, key: &str| {
            snapshot
                .lines()
                .find(|l| l.starts_with(key))
                .map(str::to_string)
        };
        for key in ["cooldown:", "event_cooldown:"] {
            assert!(line(&ours, key).is_some(), "{key} in {ours}");
            assert_eq!(line(&theirs, key), line(&ours, key), "{key}");
        }
    }
}
