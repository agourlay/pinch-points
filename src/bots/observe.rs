//! A board rebuilt from what a bot is shown: `hello` and one `tick`.
//!
//! This is the Rust starter kit's way in. A Rust bot links the crate and
//! runs the sim forward itself, from a [`Board`] built here, and the board
//! knows exactly what the `simulate` request knows: the observation carries
//! no PRNG state and no takeoff countdowns, so what they would decide is
//! drawn the way [`Board::lookahead`] draws it.

use super::lookahead::LOOKAHEAD_SEED;
use super::protocol::{claw_from_token, dir_from_token, event_from_token, tile_from_char};
use crate::sim::{
    Board, CrabKind, Direction, Handedness, MAX_PLAYERS, Pcg32, Spawner, TideEvent, TileKind,
    bits_to_hex, tile_token,
};
use serde_json::Value;
use std::fmt::Write;

fn field<'a>(v: &'a Value, path: &str) -> Result<&'a Value, String> {
    path.split('.')
        .try_fold(v, |v, key| v.get(key))
        .ok_or_else(|| format!("no `{path}`"))
}

/// A field a listener older than it leaves out: `None` then, which each
/// reader turns into the rule that listener played by.
fn optional<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    field(v, path).ok().filter(|v| !v.is_null())
}

/// An optional switch, off where it is absent.
fn switch(v: &Value, path: &str) -> bool {
    optional(v, path).and_then(Value::as_bool) == Some(true)
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

/// A direction as the snapshot writes it, from the protocol's word.
fn letter(token: &str) -> Result<char, String> {
    dir_from_token(token)
        .map(Direction::letter)
        .ok_or_else(|| format!("{token:?} is not a direction"))
}

/// A claw as the snapshot writes it, from the protocol's word.
fn claw(token: &str) -> Result<&'static str, String> {
    claw_from_token(token)
        .map(Handedness::token)
        .ok_or_else(|| format!("{token:?} is not a claw"))
}

/// What a crab shows, on the beach or in the air, as the snapshot's
/// fields: its id, tile, direction, progress, claw and kind.
struct CrabFields<'a> {
    id: u64,
    tile: usize,
    dir: char,
    progress: u64,
    claw: &'static str,
    kind: &'a str,
}

/// Read the fields every crab shows, wherever it is.
fn parse_crab_fields<'a>(
    c: &'a Value,
    at: impl Fn(u64, u64) -> usize,
) -> Result<CrabFields<'a>, String> {
    let tile = at(num(c, "x")?, num(c, "y")?);
    let dir = letter(text(c, "dir")?)?;
    let progress = num(c, "progress")?;
    let kind = text(c, "kind")?;
    if CrabKind::from_token(kind).is_none() {
        return Err(format!("no crab kind {kind:?}"));
    }
    Ok(CrabFields {
        id: num(c, "id")?,
        tile,
        dir,
        progress,
        claw: claw(text(c, "claw")?)?,
        kind,
    })
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
    // Turnstiles, castles and spawners are filled in below from the lists
    // that say more than one character can.
    let mut tiles: Vec<TileKind> = Vec::with_capacity(w * h);
    for row in &rows {
        for c in row.chars() {
            tiles.push(tile_from_char(c).ok_or_else(|| format!("no tile {c:?}"))?);
        }
    }
    let at = |x: u64, y: u64| (y as usize) * w + x as usize;
    for s in field(hello, "board.spawners")?
        .as_array()
        .into_iter()
        .flatten()
    {
        let i = at(num(s, "x")?, num(s, "y")?);
        let dir = dir_from_token(text(s, "dir")?).ok_or("a spawner faces no direction")?;
        let period = u32::try_from(num(s, "period")?)
            .ok()
            .filter(|&p| p > 0)
            .ok_or("a spawner's period is not a tick count")?;
        *tiles.get_mut(i).ok_or("a spawner off the board")? =
            TileKind::Spawner(Spawner { dir, period });
    }
    for c in field(tick, "castles")?.as_array().into_iter().flatten() {
        let i = at(num(c, "x")?, num(c, "y")?);
        let owner = u8::try_from(num(c, "owner")?)
            .ok()
            .filter(|&o| usize::from(o) < MAX_PLAYERS)
            .ok_or("a castle with no seat for an owner")?;
        *tiles.get_mut(i).ok_or("a castle off the board")? = TileKind::Castle(owner);
    }
    for t in field(tick, "turnstiles")?.as_array().into_iter().flatten() {
        let i = at(num(t, "x")?, num(t, "y")?);
        let next_right = text(t, "next")? == "right";
        *tiles.get_mut(i).ok_or("a turnstile off the board")? = TileKind::Turnstile { next_right };
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
    // Absent from a server older than the sweep's flight, which never has
    // any in the air.
    let sweeping: Vec<&Value> = tick
        .get("sweeping")
        .and_then(Value::as_array)
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
        next_id(&crabs).max(next_id(&sweeping)),
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
    // The rules below came one listener at a time, and an older one says
    // nothing of them: absent, each reads as the rule that listener
    // played, which is the board's default (the classic flock cap, every
    // switch off).
    if let Some(cap) = optional(hello, "rules.gull_cap").and_then(Value::as_u64) {
        let _ = writeln!(out, "gull_cap: {cap}");
    }
    if let Some(every) = optional(hello, "rules.golden_every")
        .and_then(Value::as_u64)
        .filter(|&n| n > 0)
    {
        let _ = writeln!(out, "golden_every: {every}");
    }
    let timed_lock = switch(hello, "rules.timed_lock");
    for (rule, on) in [
        ("gull_turnover", switch(hello, "rules.gull_turnover")),
        ("timed_lock", timed_lock),
        ("sweep_flight", switch(hello, "rules.sweep_flight")),
        ("whole_flock", switch(hello, "rules.whole_flock")),
        (
            "sweep_keeps_worth",
            switch(hello, "rules.sweep_keeps_worth"),
        ),
        ("fair_spawns", switch(hello, "rules.fair_spawns")),
    ] {
        if on {
            let _ = writeln!(out, "{rule}: on");
        }
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
        let name = text(event, "name")?;
        let line = match event_from_token(name) {
            Some(TideEvent::CrabMania) => "mania: crab",
            Some(TideEvent::GullMania) => "mania: gull",
            Some(TideEvent::SpeedUp) => "tempo: fast",
            Some(TideEvent::SlowDown) => "tempo: slow",
            Some(TideEvent::RightClaws) => "claw_call:",
            Some(
                TideEvent::Monopoly
                | TideEvent::GullAttack
                | TideEvent::FreshSand
                | TideEvent::CastleSwap,
            )
            | None => return Err(format!("no timed event {name:?}")),
        };
        let _ = writeln!(out, "{line} {left}");
    }
    // The roulette's quiet spell after an event is a clock anyone can read
    // off the last event's tick.
    if let Some(last) = tick.get("last_event").filter(|e| !e.is_null()) {
        let fired = num(last, "tick")?;
        let event = event_from_token(text(last, "name").unwrap_or("")).ok_or("no such event")?;
        let index = TideEvent::ALL
            .iter()
            .position(|&e| e == event)
            .ok_or("no such event")?;
        // An event fires during tick `fired`, after that tick's count
        // down, so the board one tick on still has the whole spell left.
        let since = now.saturating_sub(fired).saturating_sub(1);
        let quiet = match !timed_lock || event.lasts() {
            true => u64::from(crate::sim::EVENT_COOLDOWN).saturating_sub(since),
            // Under a timed lock an event over in a tick locks nothing, and
            // leaves the wheel as it found it: locked by a timed event
            // still running, if a vote landed this one on top of it. The
            // lock and an event are one length and start on one tick, but
            // the lock counts down at the start of a tick and the event at
            // its end, so the lock has one tick more to run.
            false => tick
                .get("event")
                .filter(|e| !e.is_null())
                .map_or(Ok(0), |running| num(running, "ticks_left").map(|n| n + 1))?,
        };
        if quiet > 0 {
            let _ = writeln!(out, "event_cooldown: {quiet}");
        }
        let _ = writeln!(out, "last_event: {index} {fired}");
    }
    let _ = writeln!(
        out,
        "hwalls: {}\nvwalls: {}",
        bits_to_hex(&h_walls),
        bits_to_hex(&v_walls)
    );
    let tiles: Vec<String> = tiles.into_iter().map(tile_token).collect();
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
        let CrabFields {
            id,
            tile,
            dir,
            progress,
            claw,
            kind,
        } = parse_crab_fields(c, at)?;
        let _ = writeln!(
            out,
            "crab: {id} {tile} {dir} {progress} {tile} {progress} {dir} {claw} {kind}",
        );
    }
    for c in &sweeping {
        // Absent from a listener older than the field, which is a board
        // that never reads it.
        let called = if switch(c, "claw_call") { " claws" } else { "" };
        let CrabFields {
            id,
            tile,
            dir,
            progress,
            claw,
            kind,
        } = parse_crab_fields(c, at)?;
        let _ = writeln!(
            out,
            "sweeping: {} {} {} {id} {tile} {dir} {progress} {tile} {progress} {dir} {claw} {kind}{called}",
            num(c, "owner")?,
            num(c, "launch")?,
            num(c, "lands")?,
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
    use crate::sim::{PlayerAction, TideEvent, classic_arena, generate_arena};

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
            teams: None,
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
            // Where each crab is, not its id: a crab spawned from here on
            // takes the next id, which counts crabs already gone from the
            // beach, banked or eaten, and those a bot never saw.
            let place = |b: &Board| -> Vec<(u16, u16)> {
                b.crabs().iter().map(|c| (c.tile, c.progress)).collect()
            };
            assert_eq!(place(&theirs), place(&ours), "at tick {}", ours.ticks());
        }
    }

    /// What a bot in seat 0 is shown of `board`: its `hello` and a `tick`.
    fn shown(board: &Board) -> (Value, Value) {
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
            teams: None,
        };
        let hello = protocol::hello(board, &table, 0, false);
        let you = You {
            seat: 0,
            last: None,
        };
        (hello, protocol::tick(board, 1, &you, &[]))
    }

    /// One line of a snapshot, by its key.
    fn line(snapshot: &str, key: &str) -> Option<String> {
        snapshot
            .lines()
            .find(|l| l.starts_with(key))
            .map(str::to_string)
    }

    /// A versus beach, with every rule versus switches on.
    fn versus_beach() -> Board {
        let mut board = classic_arena(false, 2);
        crate::sim::set_versus_rules(&mut board, 240);
        board.set_events_enabled(true);
        board
    }

    /// A tick with `event` called by the spectators, which fires mid-tick
    /// as a spin of the wheel does, not between ticks as a dev hook's does.
    fn call(board: &mut Board, event: TideEvent) {
        let mut actions = [PlayerAction::None; MAX_PLAYERS];
        actions[0] = PlayerAction::CallEvent(event);
        board.tick(&actions);
    }

    #[test]
    fn a_versus_beach_rebuilds_from_what_a_bot_sees() {
        rebuilt_matches_the_lookahead(versus_beach(), 700);
    }

    /// Every rule versus switches on comes through, and a Crab Monopoly
    /// swept under Right Claws keeps the call on each crab in the air.
    #[test]
    fn the_versus_rules_and_a_sweep_rebuild() {
        let mut board = versus_beach();
        for _ in 0..400 {
            board.tick_idle();
        }
        call(&mut board, TideEvent::RightClaws);
        call(&mut board, TideEvent::Monopoly);
        assert!(!board.sweeping().is_empty(), "crabs in the air");
        let (hello, tick) = shown(&board);
        let theirs = board_from(&hello, &tick).expect("rebuilds").to_snapshot();
        let ours = board.lookahead(LOOKAHEAD_SEED).to_snapshot();
        for key in [
            "gull_turnover:",
            "golden_every:",
            "timed_lock:",
            "sweep_flight:",
            "whole_flock:",
            "sweep_keeps_worth:",
            "fair_spawns:",
            "event_cooldown:",
            "claw_call:",
        ] {
            assert!(line(&ours, key).is_some(), "{key} in {ours}");
            assert_eq!(line(&theirs, key), line(&ours, key), "{key}");
        }
        // Who, when and which crab, and the call it was swept under; the
        // pose it left the sand in is the renderer's and not shown.
        let flights = |snapshot: &str| -> Vec<(String, bool)> {
            snapshot
                .lines()
                .filter(|l| l.starts_with("sweeping:"))
                .map(|l| {
                    let head: Vec<&str> = l.split_whitespace().take(5).collect();
                    (head.join(" "), l.ends_with(" claws"))
                })
                .collect()
        };
        assert!(flights(&ours).iter().all(|&(_, claws)| claws));
        assert_eq!(flights(&theirs), flights(&ours));
    }

    /// Under a timed lock an event over in a tick leaves the wheel free,
    /// and the rebuilt board does not invent a lock the sim never set; on
    /// top of a timed event it keeps that event's lock, to the tick.
    #[test]
    fn a_timed_lock_rebuilds_as_the_sim_keeps_it() {
        let mut board = versus_beach();
        for _ in 0..100 {
            board.tick_idle();
        }
        call(&mut board, TideEvent::FreshSand);
        let (hello, tick) = shown(&board);
        let theirs = board_from(&hello, &tick).expect("rebuilds").to_snapshot();
        assert_eq!(line(&theirs, "event_cooldown:"), None, "{theirs}");
        assert_eq!(
            line(
                &board.lookahead(LOOKAHEAD_SEED).to_snapshot(),
                "event_cooldown:"
            ),
            None
        );
        call(&mut board, TideEvent::SpeedUp);
        for _ in 0..40 {
            board.tick_idle();
        }
        call(&mut board, TideEvent::CastleSwap);
        let (hello, tick) = shown(&board);
        let theirs = board_from(&hello, &tick).expect("rebuilds").to_snapshot();
        let ours = board.lookahead(LOOKAHEAD_SEED).to_snapshot();
        assert!(line(&ours, "event_cooldown:").is_some(), "{ours}");
        assert_eq!(
            line(&theirs, "event_cooldown:"),
            line(&ours, "event_cooldown:")
        );
    }

    /// A listener older than a rule leaves it out of `hello`, and the board
    /// still rebuilds, on the rule that listener played by.
    #[test]
    fn a_hello_from_an_older_listener_still_rebuilds() {
        let board = classic_arena(false, 2);
        let (mut hello, tick) = shown(&board);
        let rules = hello["rules"].as_object_mut().expect("rules");
        for rule in [
            "gull_cap",
            "gull_turnover",
            "golden_every",
            "golden_value",
            "timed_lock",
            "sweep_flight",
            "whole_flock",
            "sweep_keeps_worth",
        ] {
            assert!(rules.remove(rule).is_some(), "{rule} was in the hello");
        }
        let theirs = board_from(&hello, &tick).expect("rebuilds");
        assert_eq!(theirs.rules().gull_cap, board.rules().gull_cap);
        assert!(
            !theirs.rules().timed_lock
                && !theirs.rules().sweep_flight
                && !theirs.rules().whole_flock
        );
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
            teams: None,
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
