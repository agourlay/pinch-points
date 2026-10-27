//! The bot protocol's messages: what a listener writes and how it reads
//! what comes back. Versions 1 and 2, as `docs/bot-protocol.md` publishes
//! them: 2 is 1 with each tick sent as what changed ([`delta`]).
//!
//! Everything is built and read through `serde_json::Value` into the fixed
//! shapes below and nothing else: no field a bot sends ever becomes a path,
//! a command or a format string.

pub use crate::sim::SeatKind;
use crate::sim::{
    Board, CrabKind, Direction, GullState, Handedness, MAX_PLAYERS, PlayerId, Refusal,
    SIGNPOST_LIFETIME, SignpostHealth, TideEvent, TileKind,
};
use serde_json::{Value, json};

/// The newest protocol version this build speaks: 2, where a tick is sent
/// as what changed since the last one the bot was sent ([`delta`]).
pub const PROTOCOL: u64 = 2;
/// The oldest it still speaks: 1, where every tick is sent whole. A bot
/// written against 1 is never sent a diff it does not know to apply.
pub const OLDEST_PROTOCOL: u64 = 1;
/// Under protocol 2, the most ticks a bot goes between two whole ones: a
/// bot that applied a diff wrongly, and never noticed, is put right
/// within five seconds without asking (`resync` asks at once).
pub const FULL_EVERY: u64 = 150;
/// The longest line a listener reads. Anything longer closes the
/// connection: a full tick message is about 10 KB, so a reply this size is
/// a bug or a flood.
pub const MAX_LINE: usize = 64 * 1024;
/// How much of a `note` is kept. Longer ones are cut, not refused, so a
/// long thought never costs a bot its connection.
pub const NOTE_CAP: usize = 4 * 1024;
/// How long a bot name may be, in characters.
pub const NAME_CAP: usize = 32;
/// How long the other display strings (owner, version) may be.
pub const LABEL_CAP: usize = 32;
/// How long a replay id a bot asks for may be: room for any cup's id
/// (`cup --id`, at most 24) and a game number.
pub const REPLAY_ID_CAP: usize = 48;

pub fn dir_token(dir: Direction) -> &'static str {
    match dir {
        Direction::Up => "up",
        Direction::Down => "down",
        Direction::Left => "left",
        Direction::Right => "right",
    }
}

/// The direction `dir_token` writes as `token`.
pub fn dir_from_token(token: &str) -> Option<Direction> {
    Direction::ALL.into_iter().find(|&d| dir_token(d) == token)
}

pub fn kind_token(kind: CrabKind) -> &'static str {
    kind.token()
}

fn claw_token(handed: Handedness) -> &'static str {
    match handed {
        Handedness::Left => "left",
        Handedness::Right => "right",
    }
}

/// The claw `claw_token` writes as `token`.
pub fn claw_from_token(token: &str) -> Option<Handedness> {
    [Handedness::Left, Handedness::Right]
        .into_iter()
        .find(|&h| claw_token(h) == token)
}

/// The protocol's name for a tide event.
pub fn event_token(event: TideEvent) -> &'static str {
    match event {
        TideEvent::CrabMania => "crab_mania",
        TideEvent::GullMania => "gull_mania",
        TideEvent::Monopoly => "monopoly",
        TideEvent::GullAttack => "gull_attack",
        TideEvent::SpeedUp => "speed_up",
        TideEvent::SlowDown => "slow_down",
        TideEvent::FreshSand => "fresh_sand",
        TideEvent::CastleSwap => "castle_swap",
        TideEvent::RightClaws => "right_claws",
    }
}

/// The tide event `event_token` writes as `token`.
pub fn event_from_token(token: &str) -> Option<TideEvent> {
    TideEvent::ALL
        .into_iter()
        .find(|&event| event_token(event) == token)
}

/// One character per tile for the `hello` board. Castles carry the owner
/// they start with; who holds them now is in every tick's `castles`.
fn tile_char(tile: TileKind) -> char {
    match tile {
        TileKind::Empty => '.',
        TileKind::Rock => '#',
        TileKind::Castle(owner) => char::from(b'0' + owner),
        TileKind::Spawner(_) => 'S',
        TileKind::Turnstile { .. } => 'T',
        TileKind::Kelp => 'K',
        TileKind::Pool => '~',
    }
}

/// The tile a `hello` character stands for, as far as one character can
/// say it. A spawner's direction and period, and which way a turnstile
/// turns next, are in the lists sent with it (`board.spawners`, a tick's
/// `turnstiles`): until those are read, an `S` is a spawner facing up
/// every tick and a `T` turns right next.
pub fn tile_from_char(c: char) -> Option<TileKind> {
    Some(match c {
        '.' => TileKind::Empty,
        '#' => TileKind::Rock,
        'S' => TileKind::Spawner(crate::sim::Spawner {
            dir: Direction::Up,
            period: 1,
        }),
        'T' => TileKind::Turnstile { next_right: true },
        'K' => TileKind::Kelp,
        '~' => TileKind::Pool,
        d => TileKind::Castle(u8::try_from(d.to_digit(10)?).ok()?),
    })
}

/// One crab as a tick shows it, on the beach or swept off it.
fn crab_json(board: &Board, crab: &crate::sim::Crab) -> Value {
    let (x, y) = board.coords_u8(crab.tile);
    json!({
        "id": crab.id, "x": x, "y": y, "dir": dir_token(crab.dir),
        "progress": crab.progress, "kind": kind_token(crab.kind),
        "claw": claw_token(crab.handed),
    })
}

/// How the next tick is sent, and how long a bot has to answer it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Clock {
    /// Live: the next tick goes out on the wall clock, 30 a second, because
    /// somebody is watching. Fast-forward: as soon as every bot has answered
    /// or its deadline has passed.
    pub live: bool,
    pub deadline_ms: u32,
    /// How many ticks after the one it answers a reply takes effect.
    pub input_delay: u32,
}

/// Everything about a game that does not change while it is played: the
/// static half of the board, the rules, the clock and the table.
pub struct Table<'a> {
    pub game: u32,
    pub names: &'a [String],
    pub kinds: &'a [SeatKind],
    pub clock: Clock,
    /// The fair cursor rule.
    pub cursor: bool,
    /// Each seat's team, counting from 0, when the round is played in
    /// teams; `None` for every seat for itself, which `hello` says as
    /// every seat its own team.
    pub teams: Option<&'a [u8]>,
}

/// `hello`: the start of a game for `seat`.
pub fn hello(board: &Board, table: &Table, seat: PlayerId, resumed: bool) -> Value {
    let (w, h) = (board.width(), board.height());
    let tiles: Vec<String> = (0..h)
        .map(|y| (0..w).map(|x| tile_char(board.tile_at(x, y))).collect())
        .collect();
    // `h` rows of edges between rows, plus the borders: row `r` is the edge
    // above tile row `r`, and the last is the bottom border.
    let h_walls: Vec<String> = (0..=h)
        .map(|r| {
            (0..w)
                .map(|x| {
                    let wall = if r < h {
                        board.wall_at(x, r, Direction::Up)
                    } else {
                        board.wall_at(x, h - 1, Direction::Down)
                    };
                    if wall { '-' } else { '.' }
                })
                .collect()
        })
        .collect();
    let v_walls: Vec<String> = (0..h)
        .map(|y| {
            (0..=w)
                .map(|c| {
                    let wall = if c < w {
                        board.wall_at(c, y, Direction::Left)
                    } else {
                        board.wall_at(w - 1, y, Direction::Right)
                    };
                    if wall { '|' } else { '.' }
                })
                .collect()
        })
        .collect();
    let spawners: Vec<Value> = board
        .tiles()
        .filter_map(|(x, y, kind)| match kind {
            TileKind::Spawner(s) => Some(json!({
                "x": x, "y": y, "dir": dir_token(s.dir), "period": s.period
            })),
            TileKind::Empty
            | TileKind::Rock
            | TileKind::Castle(_)
            | TileKind::Turnstile { .. }
            | TileKind::Kelp
            | TileKind::Pool => None,
        })
        .collect();
    let (cap, policy) = board.signpost_rule();
    let mut msg = json!({
        "type": "hello",
        "game": table.game,
        "seat": seat,
        "seats": table.names.len(),
        "names": table.names,
        "kinds": table.kinds.iter().map(|k| k.token()).collect::<Vec<_>>(),
        "teams": match table.teams {
            Some(teams) => teams.to_vec(),
            None => (0..table.names.len() as u8).collect(),
        },
        "board": {
            "width": w, "height": h, "wrap": board.wrap(),
            "tiles": tiles,
            "walls": {"h": h_walls, "v": v_walls},
            "spawners": spawners,
        },
        "rules": {
            "signpost_cap": cap,
            "cap_policy": policy.token(),
            "signpost_lifetime": SIGNPOST_LIFETIME,
            "round_ticks": board.rules().round_length,
            "gull_period": board.rules().gull_period,
            "gull_cap": board.rules().gull_cap,
            "gull_turnover": board.rules().gull_turnover,
            "golden_every": board.rules().golden_every,
            "golden_value": board.crab_value(crate::sim::CrabKind::Golden),
            "timed_lock": board.rules().timed_lock,
            "sweep_flight": board.rules().sweep_flight,
            "sweep_keeps_worth": board.rules().sweep_keeps_worth,
            "whole_flock": board.rules().whole_flock,
            "fair_spawns": board.rules().fair_spawns,
            "castle_raids": board.rules().castle_raids,
            "events": board.events_enabled(),
        },
        "clock": {
            "mode": if table.clock.live { "live" } else { "fast_forward" },
            "deadline_ms": table.clock.deadline_ms,
            "input_delay": table.clock.input_delay,
        },
        "cursor": if table.cursor {
            json!({
                "fair": true,
                "lift": crate::sim::FAIR_LIFT,
                "ticks_per_tile": crate::sim::FAIR_TICKS_PER_TILE,
            })
        } else {
            json!({"fair": false})
        },
    });
    if resumed {
        msg["resumed"] = json!(true);
    }
    msg
}

/// `error`: what a bot is told when a request of its own is refused,
/// naming the game it was about if there is one. The connection stays.
pub fn error(message: &str, game: Option<u32>) -> Value {
    let mut msg = json!({"type": "error", "message": message});
    if let Some(game) = game {
        msg["game"] = json!(game);
    }
    msg
}

/// A fatal `error`: why the connection is about to close.
pub fn fatal(message: &str) -> Value {
    json!({"type": "error", "fatal": true, "message": message})
}

/// What became of a seat's last action, for `you.last`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Accepted,
    Refused(Refusal),
}

/// The per-seat half of a tick message.
pub struct You {
    pub seat: PlayerId,
    pub last: Option<(&'static str, Outcome)>,
}

/// `tick`: everything a player can see on the screen, and nothing they
/// cannot.
pub fn tick(board: &Board, game: u32, you: &You, cursors: &[Option<(u8, u8)>]) -> Value {
    let crabs: Vec<Value> = board
        .crabs()
        .iter()
        .map(|crab| crab_json(board, crab))
        .collect();
    // Crab Monopoly's crabs on their way home: off the beach, out of reach
    // of gulls and arrows, and banked for `owner` on the tick `lands`.
    let sweeping: Vec<Value> = board
        .sweeping()
        .iter()
        .map(|flight| {
            let mut crab = crab_json(board, &flight.crab);
            crab["owner"] = json!(flight.owner);
            crab["launch"] = json!(flight.launch);
            crab["lands"] = json!(flight.lands);
            crab["claw_call"] = json!(flight.claw_call);
            crab
        })
        .collect();
    // `takeoff_in` is not here: the screen never shows it.
    let gulls: Vec<Value> = board
        .gulls()
        .iter()
        .map(|gull| {
            let (x, y) = board.coords_u8(gull.tile);
            let (state, hops) = match gull.state {
                GullState::Walking => ("walking", 0),
                GullState::Flying { remaining } => ("flying", remaining),
            };
            json!({
                "id": gull.id, "x": x, "y": y, "dir": dir_token(gull.dir),
                "progress": gull.progress, "state": state, "hops_left": hops,
                "claw": claw_token(gull.handed),
            })
        })
        .collect();
    // Shown as soon as the spawner sends for one, which is when the screen
    // starts drawing it: its shadow sweeps in over the edge and lies on the
    // landing tile from before half the lead until the gull lands on it
    // (under reduced motion it fades in on that tile from the start).
    let incoming: Vec<Value> = board
        .incoming_gulls()
        .iter()
        .map(|gull| {
            let (x, y) = board.coords_u8(gull.tile);
            json!({"x": x, "y": y, "dir": dir_token(gull.dir), "tick": gull.at})
        })
        .collect();
    let now = board.ticks();
    let signposts: Vec<Value> = board
        .signposts()
        .map(|(x, y, post)| {
            json!({
                "x": x, "y": y, "dir": dir_token(post.dir), "owner": post.owner,
                "worn": post.health == SignpostHealth::Worn,
                "age": now.saturating_sub(post.placed), "placed": post.placed,
            })
        })
        .collect();
    let mut castles = Vec::new();
    let mut turnstiles = Vec::new();
    for (x, y, kind) in board.tiles() {
        match kind {
            TileKind::Castle(owner) => castles.push(json!({"x": x, "y": y, "owner": owner})),
            TileKind::Turnstile { next_right } => turnstiles
                .push(json!({"x": x, "y": y, "next": if next_right { "right" } else { "left" }})),
            TileKind::Empty
            | TileKind::Rock
            | TileKind::Spawner(_)
            | TileKind::Kelp
            | TileKind::Pool => {}
        }
    }
    let seats = usize::from(board.seats_in_play()).clamp(1, MAX_PLAYERS);
    let last = you.last.map(|(act, outcome)| match outcome {
        Outcome::Accepted => json!({"act": act, "accepted": true}),
        Outcome::Refused(why) => json!({"act": act, "accepted": false, "reason": why.token()}),
    });
    json!({
        "type": "tick",
        "game": game,
        "tick": now,
        "remaining": board.remaining_ticks(),
        "scores": &board.scores()[..seats],
        "you": {
            "seat": you.seat,
            "signposts": board.signpost_count(you.seat),
            "last": last,
        },
        "cursors": cursors.iter().map(|c| c.map(|(x, y)| [x, y])).collect::<Vec<_>>(),
        "crabs": crabs,
        "sweeping": sweeping,
        "gulls": gulls,
        "incoming_gulls": incoming,
        "signposts": signposts,
        "castles": castles,
        "turnstiles": turnstiles,
        "event": board.running_event().map(|(event, left)| {
            json!({"name": event_token(event), "ticks_left": left})
        }),
        "last_event": board.last_event().map(|(event, at)| {
            json!({"name": event_token(event), "tick": at})
        }),
        "lure": board.lure().map(|(owner, left)| json!({"owner": owner, "ticks_left": left})),
        "lure_cooldown": board.lure_cooldown(),
        "claw_call": board.in_claw_call(),
        "golden_call": board.golden_call().map(|(at, x, y)| json!({"tick": at, "x": x, "y": y})),
        "surge": board.in_surge(),
    })
}

/// The lists in a tick that a diff sends entry by entry, and the fields
/// that name an entry: a crab or a gull by its `id`, everything that
/// stands on a tile by the tile. Every other field changes whole.
pub const KEYED: [(&str, &[&str]); 6] = [
    ("crabs", &["id"]),
    ("sweeping", &["id"]),
    ("gulls", &["id"]),
    ("signposts", &["x", "y"]),
    ("castles", &["x", "y"]),
    ("turnstiles", &["x", "y"]),
];

/// The one field a diff leaves a bot to work out: a post's `age`, which is
/// the tick less its `placed`. Sent, it would change on every post every
/// tick, and be the larger part of every diff. Every other field a diff
/// carries as it changes.
const AGE: (&str, &str, &str) = ("signposts", "age", "placed");

/// The field of the keyed list `field`'s entries a diff leaves out, if any.
fn derived(field: &str) -> Option<&'static str> {
    (field == AGE.0).then_some(AGE.1)
}

/// Every post's `age` worked out from its `placed`, for the tick `state`
/// is of ([`AGE`]).
fn age_posts(state: &mut serde_json::Map<String, Value>) {
    let Some(now) = state.get("tick").and_then(Value::as_u64) else {
        return;
    };
    let Some(posts) = state.get_mut(AGE.0).and_then(Value::as_array_mut) else {
        return;
    };
    for post in posts.iter_mut().filter_map(Value::as_object_mut) {
        if let Some(placed) = post.get(AGE.2).and_then(Value::as_u64) {
            post.insert(AGE.1.to_string(), json!(now.saturating_sub(placed)));
        }
    }
}

/// The fields that name an entry of the keyed list `field`, if it is one.
fn keys_of(field: &str) -> Option<&'static [&'static str]> {
    KEYED
        .iter()
        .find(|(name, _)| *name == field)
        .map(|(_, keys)| *keys)
}

/// An entry's key: the values of its naming fields, or `None` for an
/// entry missing one.
fn key_of(entry: &Value, keys: &[&str]) -> Option<Vec<Value>> {
    keys.iter().map(|k| entry.get(*k).cloned()).collect()
}

/// A tick as what changed from `base`, the last tick the bot was sent of
/// the same game: protocol 2's diff. It names its own `tick` and the
/// `base` tick it applies to, and carries only the fields that differ.
/// A keyed list ([`KEYED`]) that changed comes as `{"set": [...], "gone":
/// [...]}`: `set` holds every entry that is new (whole) or changed (its
/// naming fields and the fields that changed), `gone` the naming fields of
/// every entry no longer there. Any other field that changed, a list
/// included, comes whole. A post's `age` is left out of `set` unless the
/// post is new: it is the tick less the post's `placed`.
///
/// [`apply_delta`] is the inverse, and the one rule a bot implements.
pub fn delta(base: &Value, now: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for field in ["type", "game", "tick"] {
        if let Some(v) = now.get(field) {
            out.insert(field.to_string(), v.clone());
        }
    }
    out.insert(
        "base".to_string(),
        base.get("tick").cloned().unwrap_or(Value::Null),
    );
    let (Some(before), Some(after)) = (base.as_object(), now.as_object()) else {
        return now.clone();
    };
    for (field, value) in after {
        if matches!(field.as_str(), "type" | "game" | "tick") || before.get(field) == Some(value) {
            continue;
        }
        let changed = match (keys_of(field), before.get(field), value) {
            (Some(keys), Some(Value::Array(old)), Value::Array(new)) => {
                match list_delta(old, new, keys, derived(field)) {
                    Some(ListDelta::Same) => continue,
                    Some(ListDelta::Changed(change)) => change,
                    None => value.clone(),
                }
            }
            _ => value.clone(),
        };
        out.insert(field.clone(), changed);
    }
    // A field the new tick no longer has reads as `null`, never as
    // unchanged.
    for field in before.keys() {
        if !after.contains_key(field) {
            out.insert(field.clone(), Value::Null);
        }
    }
    Value::Object(out)
}

/// What became of a keyed list between two ticks.
enum ListDelta {
    /// Nothing but its derived field ([`AGE`]): it is left out of the diff.
    Same,
    /// Its `set` and `gone`.
    Changed(Value),
}

/// Two entries the same, but for the field `skip`.
fn same_but(a: &Value, b: &Value, skip: Option<&str>) -> bool {
    match (a.as_object(), b.as_object(), skip) {
        (Some(a), Some(b), Some(skip)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(field, v)| field == skip || b.get(field) == Some(v))
        }
        _ => a == b,
    }
}

/// A keyed list as its changes, leaving out the field `skip` of an entry
/// that was there before; or `None` when an entry has no key or two share
/// one: the list is then sent whole, which a bot applies the same way as
/// any other field.
fn list_delta(
    old: &[Value],
    new: &[Value],
    keys: &[&str],
    skip: Option<&str>,
) -> Option<ListDelta> {
    let mut before = std::collections::HashMap::new();
    for entry in old {
        if before.insert(key_of(entry, keys)?, entry).is_some() {
            return None;
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut set = Vec::new();
    for entry in new {
        let key = key_of(entry, keys)?;
        if !seen.insert(key.clone()) {
            return None;
        }
        match before.get(&key) {
            Some(was) if same_but(was, entry, skip) => {}
            Some(was) => {
                let mut part = serde_json::Map::new();
                for (field, v) in entry.as_object()? {
                    let named = keys.contains(&field.as_str());
                    if named || (Some(field.as_str()) != skip && was.get(field) != Some(v)) {
                        part.insert(field.clone(), v.clone());
                    }
                }
                set.push(Value::Object(part));
            }
            None => set.push(entry.clone()),
        }
    }
    let gone: Vec<Value> = old
        .iter()
        .filter_map(|entry| {
            let key = key_of(entry, keys)?;
            (!seen.contains(&key)).then(|| {
                Value::Object(
                    keys.iter()
                        .zip(key)
                        .map(|(k, v)| ((*k).to_string(), v))
                        .collect(),
                )
            })
        })
        .collect();
    Some(if set.is_empty() && gone.is_empty() {
        ListDelta::Same
    } else {
        ListDelta::Changed(json!({"set": set, "gone": gone}))
    })
}

/// The tick a protocol 2 message stands for, given `state`, the last tick
/// this bot holds of the same game: a whole tick (one without `base`) is
/// itself; a diff is applied to `state` when it names `state`'s tick as
/// its `base`, and is `None` otherwise, which is the bot's cue to ask for
/// a `resync`. The inverse of [`delta`], for a Rust bot; the Python bots
/// carry the same few lines.
///
/// A keyed list keeps its entries' order, the new ones after: the order of
/// a list is not part of the board. Every post's `age` is then worked out
/// afresh from its `placed`, the one field a diff leaves to the bot.
pub fn apply_delta(state: Option<&Value>, msg: &Value) -> Option<Value> {
    let Some(base) = msg.get("base") else {
        return Some(msg.clone());
    };
    let state = state?;
    if state.get("tick") != Some(base) || state.get("game") != msg.get("game") {
        return None;
    }
    let mut next = state.as_object()?.clone();
    for (field, value) in msg.as_object()? {
        if field == "base" {
            continue;
        }
        let patched = match (keys_of(field), value) {
            (Some(keys), Value::Object(change)) => match next.get(field) {
                Some(Value::Array(old)) => Value::Array(patch_list(old, change, keys)?),
                _ => return None,
            },
            _ => value.clone(),
        };
        next.insert(field.clone(), patched);
    }
    age_posts(&mut next);
    Some(Value::Object(next))
}

/// A keyed list with its `set` and `gone` applied.
fn patch_list(
    old: &[Value],
    change: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<Vec<Value>> {
    let gone: Vec<Vec<Value>> = change
        .get("gone")?
        .as_array()?
        .iter()
        .map(|entry| key_of(entry, keys))
        .collect::<Option<_>>()?;
    let mut list: Vec<Value> = old
        .iter()
        .filter(|entry| key_of(entry, keys).is_none_or(|key| !gone.contains(&key)))
        .cloned()
        .collect();
    for part in change.get("set")?.as_array()? {
        let key = key_of(part, keys)?;
        match list
            .iter_mut()
            .find(|entry| key_of(entry, keys).as_ref() == Some(&key))
        {
            Some(entry) => {
                let entry = entry.as_object_mut()?;
                for (field, v) in part.as_object()? {
                    entry.insert(field.clone(), v.clone());
                }
            }
            None => list.push(part.clone()),
        }
    }
    Some(list)
}

/// What a bot asked its seat to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Act {
    #[default]
    None,
    Place {
        x: u8,
        y: u8,
        dir: Direction,
    },
    Remove {
        x: u8,
        y: u8,
    },
    /// The clear-all key: the seat's first signpost, wherever it stands.
    Clear,
    /// Put the cursor on a tile and do nothing there.
    Move {
        x: u8,
        y: u8,
    },
}

impl Act {
    pub fn token(self) -> &'static str {
        match self {
            Act::None => "none",
            Act::Place { .. } => "place",
            Act::Remove { .. } => "remove",
            Act::Clear => "clear",
            Act::Move { .. } => "move",
        }
    }

    /// The act as the protocol writes it, for the log and the trace.
    pub fn describe(self) -> String {
        match self {
            Act::None | Act::Clear => self.token().to_string(),
            Act::Place { x, y, dir } => format!("place {x},{y} {}", dir_token(dir)),
            Act::Remove { x, y } => format!("remove {x},{y}"),
            Act::Move { x, y } => format!("move {x},{y}"),
        }
    }

    /// The tile it is aimed at, if it has one.
    pub fn target(self) -> Option<(u8, u8)> {
        match self {
            Act::Place { x, y, .. } | Act::Remove { x, y } | Act::Move { x, y } => Some((x, y)),
            Act::None | Act::Clear => None,
        }
    }

    pub fn to_json(self) -> Value {
        match self {
            Act::None | Act::Clear => json!({"act": self.token()}),
            Act::Place { x, y, dir } => {
                json!({"act": "place", "x": x, "y": y, "dir": dir_token(dir)})
            }
            Act::Remove { x, y } | Act::Move { x, y } => {
                json!({"act": self.token(), "x": x, "y": y})
            }
        }
    }
}

/// A reply to a tick.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reply {
    pub game: u32,
    pub tick: u64,
    pub act: Act,
    /// Do not send me the next `wait` ticks.
    pub wait: u32,
    pub note: Option<String>,
    /// Why the act was read as `none`, when the bot sent something else.
    pub garbled: Option<String>,
}

/// A registration, as the bot sent it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Register {
    pub protocol: u64,
    pub name: String,
    pub version: String,
    pub owner: Option<String>,
    pub parallel: u32,
    pub key: Option<String>,
    pub token: Option<String>,
}

/// One planned action in a `simulate` request.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Planned {
    pub at: u32,
    pub act: Act,
}

/// Anything a bot sends.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Incoming {
    Register(Register),
    Ready {
        game: u32,
    },
    Reply(Reply),
    Simulate {
        game: u32,
        ticks: u32,
        plan: Vec<Planned>,
    },
    Replay {
        id: String,
    },
    Pong {
        id: u64,
    },
    /// Protocol 2: send me the next tick of this game whole. A bot asks
    /// when a diff names a `base` it does not hold.
    Resync {
        game: u32,
    },
    /// Not something this protocol says. Kept with the game it named, if
    /// it named one, so a reply that was garbled still costs its tick.
    Garbled {
        game: Option<u32>,
        why: String,
    },
    /// A request the listener answers with an error (a `simulate` it cannot
    /// read, a `ready` for no game): never a reply to a tick, so it costs
    /// the bot nothing but the error.
    BadRequest {
        game: Option<u32>,
        why: String,
    },
}

fn uint(v: &Value, field: &str) -> Option<u64> {
    v.get(field).and_then(Value::as_u64)
}

fn coord(v: &Value, field: &str) -> Result<u8, String> {
    uint(v, field)
        .and_then(|n| u8::try_from(n).ok())
        .ok_or_else(|| format!("`{field}` must be a tile coordinate"))
}

/// Cut a display string to `cap` characters, dropping control characters:
/// names are drawn on screen and written into logs, never interpreted. A
/// `|` becomes a `/`, since a replay's `names:` line is split on it.
pub fn tidy(text: &str, cap: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .map(|c| if c == '|' { '/' } else { c })
        .take(cap)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Read an `act` and its coordinates.
pub fn parse_act(v: &Value) -> Result<Act, String> {
    let act = v.get("act").and_then(Value::as_str).unwrap_or("none");
    Ok(match act {
        "none" => Act::None,
        "clear" => Act::Clear,
        "place" => {
            let dir = v
                .get("dir")
                .and_then(Value::as_str)
                .and_then(dir_from_token)
                .ok_or("`place` needs a `dir` of up, down, left or right")?;
            Act::Place {
                x: coord(v, "x")?,
                y: coord(v, "y")?,
                dir,
            }
        }
        "remove" => Act::Remove {
            x: coord(v, "x")?,
            y: coord(v, "y")?,
        },
        "move" => Act::Move {
            x: coord(v, "x")?,
            y: coord(v, "y")?,
        },
        other => return Err(format!("there is no action {:?}", tidy(other, 24))),
    })
}

/// Read one line from a bot.
pub fn parse_incoming(line: &str) -> Incoming {
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Incoming::Garbled {
                game: None,
                why: format!("not JSON: {e}"),
            };
        }
    };
    let game = uint(&v, "game").and_then(|g| u32::try_from(g).ok());
    let kind = v.get("type").and_then(Value::as_str);
    match kind {
        Some("register") => Incoming::Register(Register {
            protocol: uint(&v, "protocol").unwrap_or(0),
            name: v
                .get("name")
                .and_then(Value::as_str)
                .map(|n| tidy(n, NAME_CAP))
                .unwrap_or_default(),
            version: v
                .get("version")
                .and_then(Value::as_str)
                .map(|n| tidy(n, LABEL_CAP))
                .unwrap_or_default(),
            owner: v
                .get("owner")
                .and_then(Value::as_str)
                .map(|n| tidy(n, LABEL_CAP))
                .filter(|n| !n.is_empty()),
            parallel: uint(&v, "parallel")
                .map_or(1, |n| u32::try_from(n).unwrap_or(u32::MAX))
                .max(1),
            key: v.get("key").and_then(Value::as_str).map(|k| tidy(k, 16)),
            // An empty token is no token: a bot that writes `"token": ""`
            // before it has one is registering afresh.
            token: v
                .get("token")
                .and_then(Value::as_str)
                .map(|t| tidy(t, 64))
                .filter(|t| !t.is_empty()),
        }),
        Some("ready") => match game {
            Some(game) => Incoming::Ready { game },
            None => Incoming::BadRequest {
                game: None,
                why: "`ready` names no game".to_string(),
            },
        },
        Some("resync") => match game {
            Some(game) => Incoming::Resync { game },
            None => Incoming::BadRequest {
                game: None,
                why: "`resync` names no game".to_string(),
            },
        },
        Some("pong") => Incoming::Pong {
            id: uint(&v, "id").unwrap_or(0),
        },
        Some("replay") => Incoming::Replay {
            id: v
                .get("id")
                .and_then(Value::as_str)
                .map(|id| tidy(id, REPLAY_ID_CAP))
                .unwrap_or_default(),
        },
        Some("simulate") => {
            let Some(game) = game else {
                return Incoming::BadRequest {
                    game: None,
                    why: "`simulate` names no game".to_string(),
                };
            };
            let ticks = uint(&v, "ticks").map_or(0, |t| u32::try_from(t).unwrap_or(u32::MAX));
            let mut plan = Vec::new();
            for step in v
                .get("plan")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let at =
                    uint(step, "at").map_or(u32::MAX, |t| u32::try_from(t).unwrap_or(u32::MAX));
                match parse_act(step) {
                    Ok(act) => plan.push(Planned { at, act }),
                    Err(why) => {
                        return Incoming::BadRequest {
                            game: Some(game),
                            why: format!("simulate plan: {why}"),
                        };
                    }
                }
            }
            Incoming::Simulate { game, ticks, plan }
        }
        // A reply carries no type: it is the one message a bot sends thirty
        // times a second, and the shortest one is the one it should be.
        None | Some("act") => {
            let (Some(game), Some(tick)) = (game, uint(&v, "tick")) else {
                return Incoming::Garbled {
                    game,
                    why: "a reply names its `game` and the `tick` it answers".to_string(),
                };
            };
            let (act, garbled) = match parse_act(&v) {
                Ok(act) => (act, None),
                Err(why) => (Act::None, Some(why)),
            };
            Incoming::Reply(Reply {
                game,
                tick,
                act,
                wait: uint(&v, "wait").map_or(0, |w| u32::try_from(w).unwrap_or(u32::MAX)),
                note: v.get("note").and_then(Value::as_str).map(|note| {
                    note.chars()
                        .filter(|c| *c == '\n' || !c.is_control())
                        .take(NOTE_CAP)
                        .collect()
                }),
                garbled,
            })
        }
        Some(other) => Incoming::Garbled {
            game,
            why: format!("there is no message type {:?}", tidy(other, 24)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::classic_arena;

    fn table<'a>(names: &'a [String], kinds: &'a [SeatKind]) -> Table<'a> {
        Table {
            game: 3,
            names,
            kinds,
            clock: Clock {
                live: false,
                deadline_ms: 33,
                input_delay: 0,
            },
            cursor: false,
            teams: None,
        }
    }

    /// Every token the protocol writes reads back as what it was written
    /// from: the board a bot rebuilds is the one it was shown.
    #[test]
    fn every_token_reads_back() {
        for dir in Direction::ALL {
            assert_eq!(dir_from_token(dir_token(dir)), Some(dir));
        }
        for claw in [Handedness::Left, Handedness::Right] {
            assert_eq!(claw_from_token(claw_token(claw)), Some(claw));
        }
        for event in TideEvent::ALL {
            assert_eq!(event_from_token(event_token(event)), Some(event));
        }
        for tile in [
            TileKind::Empty,
            TileKind::Rock,
            TileKind::Castle(3),
            TileKind::Turnstile { next_right: true },
            TileKind::Kelp,
            TileKind::Pool,
        ] {
            assert_eq!(tile_from_char(tile_char(tile)), Some(tile));
        }
    }

    #[test]
    fn hello_draws_the_beach_a_tile_to_a_character() {
        let board = classic_arena(false, 2);
        let names = vec!["a".to_string(), "b".to_string()];
        let kinds = [SeatKind::Bot, SeatKind::Ai];
        let msg = hello(&board, &table(&names, &kinds), 1, false);
        let tiles = msg["board"]["tiles"].as_array().expect("tiles");
        assert_eq!(tiles.len(), 9);
        assert!(
            tiles
                .iter()
                .all(|row| row.as_str().map(str::len) == Some(12))
        );
        assert_eq!(&tiles[4].as_str().expect("row")[5..7], "##");
        assert_eq!(
            msg["board"]["walls"]["h"].as_array().map(Vec::len),
            Some(10)
        );
        assert_eq!(
            msg["board"]["walls"]["v"][0].as_str().map(str::len),
            Some(13)
        );
        // The border is walled, and the classic ledge under (2,2) is there.
        assert!(
            msg["board"]["walls"]["h"][0]
                .as_str()
                .expect("top")
                .chars()
                .all(|c| c == '-')
        );
        assert_eq!(
            &msg["board"]["walls"]["h"][3].as_str().expect("row")[2..4],
            "--"
        );
        assert_eq!(msg["kinds"], json!(["bot", "ai"]));
        assert_eq!(msg["seat"], json!(1));
        assert!(msg.get("resumed").is_none());
    }

    #[test]
    fn a_tick_shows_the_board_and_hides_the_takeoff_clock() {
        let mut board = classic_arena(false, 2);
        for _ in 0..60 {
            board.tick_idle();
        }
        let you = You {
            seat: 0,
            last: Some(("place", Outcome::Refused(Refusal::Rock))),
        };
        let msg = tick(&board, 3, &you, &[Some((1, 1)), None]);
        assert_eq!(msg["tick"], json!(60));
        assert_eq!(msg["scores"].as_array().map(Vec::len), Some(2));
        assert_eq!(msg["you"]["last"]["reason"], json!("rock"));
        assert_eq!(msg["cursors"], json!([[1, 1], null]));
        assert!(!msg["crabs"].as_array().expect("crabs").is_empty());
        assert!(!msg.to_string().contains("takeoff"));
    }

    /// Six AI seats playing a generated beach, read the way a bot at seat
    /// 0 is sent it: every tick, but now and then a few at once, as a bot
    /// that asked to `wait` is.
    fn six_seat_round(ticks: u64) -> Vec<Value> {
        let mut board = crate::sim::generate_arena(7, 6, 16, 11);
        crate::sim::set_versus_rules(&mut board, 240);
        let you = You {
            seat: 0,
            last: None,
        };
        let cursors = vec![None; 6];
        let mut sent = vec![tick(&board, 1, &you, &cursors)];
        for t in 0..ticks {
            let mut actions = [crate::sim::PlayerAction::None; MAX_PLAYERS];
            for (seat, action) in actions.iter_mut().enumerate().take(6) {
                *action =
                    crate::sim::bot_action(&board, seat as PlayerId, crate::sim::BotLevel::Normal);
            }
            board.tick(&actions);
            if t % 11 != 5 {
                sent.push(tick(&board, 1, &you, &cursors));
            }
        }
        sent
    }

    /// A tick with its keyed lists in key order: the order of a list is
    /// not part of the board, and a diff applied puts new entries last.
    fn in_key_order(tick: &Value) -> Value {
        let mut tick = tick.clone();
        for (field, keys) in KEYED {
            if let Some(list) = tick.get_mut(field).and_then(Value::as_array_mut) {
                list.sort_by_key(|entry| {
                    keys.iter()
                        .map(|k| entry[*k].as_u64().unwrap_or(0))
                        .collect::<Vec<_>>()
                });
            }
        }
        tick
    }

    /// A diff applied to the tick it was made from is the tick it was made
    /// to, field for field and entry for entry: a bot that applies every
    /// diff it is sent holds exactly what a protocol 1 bot is sent whole,
    /// but for the order of a list. And it is a fraction of the size.
    #[test]
    fn a_diff_applied_is_the_tick_it_was_made_to() {
        let sent = six_seat_round(1800);
        let mut held = sent[0].clone();
        let (mut whole, mut diffs) = (0, 0);
        for pair in sent.windows(2) {
            let diff = delta(&pair[0], &pair[1]);
            assert_eq!(diff["base"], pair[0]["tick"]);
            held = apply_delta(Some(&held), &diff).expect("it applies");
            assert_eq!(
                in_key_order(&held),
                in_key_order(&pair[1]),
                "at tick {}",
                pair[1]["tick"]
            );
            whole += pair[1].to_string().len();
            diffs += diff.to_string().len();
        }
        assert!(diffs * 4 < whole, "diffs {diffs} B against {whole} B whole");
    }

    /// A diff names the tick it applies to, and a bot holding any other
    /// gets nothing from it: its cue to ask for a `resync`, never a board
    /// quietly wrong. A whole tick needs nothing held.
    #[test]
    fn a_diff_for_a_tick_not_held_is_refused() {
        let sent = six_seat_round(40);
        let diff = delta(&sent[10], &sent[11]);
        assert_eq!(apply_delta(Some(&sent[9]), &diff), None);
        assert_eq!(apply_delta(None, &diff), None);
        assert_eq!(apply_delta(None, &sent[11]), Some(sent[11].clone()));
        // Another game's tick of the same number is not it either.
        let mut other = sent[10].clone();
        other["game"] = json!(2);
        assert_eq!(apply_delta(Some(&other), &diff), None);
    }

    /// A keyed list goes entry by entry: an entry that changed sends its
    /// key and what changed, a new one goes whole, one gone goes by its
    /// key. A field that did not change is left out.
    #[test]
    fn a_keyed_list_goes_entry_by_entry() {
        let base = json!({
            "type": "tick", "game": 1, "tick": 4, "surge": false,
            "crabs": [{"id": 1, "x": 2, "progress": 10, "kind": "common"},
                      {"id": 2, "x": 5, "progress": 0, "kind": "giant"}],
            "scores": [1, 2],
        });
        let now = json!({
            "type": "tick", "game": 1, "tick": 5, "surge": false,
            "crabs": [{"id": 1, "x": 2, "progress": 22, "kind": "common"},
                      {"id": 3, "x": 0, "progress": 0, "kind": "golden"}],
            "scores": [1, 12],
        });
        let diff = delta(&base, &now);
        assert_eq!(
            diff,
            json!({
                "type": "tick", "game": 1, "tick": 5, "base": 4,
                "crabs": {"set": [{"id": 1, "progress": 22},
                                  {"id": 3, "x": 0, "progress": 0, "kind": "golden"}],
                          "gone": [{"id": 2}]},
                "scores": [1, 12],
            })
        );
        assert_eq!(apply_delta(Some(&base), &diff), Some(now));
    }

    #[test]
    fn replies_are_read_into_their_one_shape() {
        let read = |s: &str| parse_incoming(s);
        assert_eq!(
            read(
                r#"{"game": 17, "tick": 4, "act": "place", "x": 5, "y": 2, "dir": "left", "wait": 3}"#
            ),
            Incoming::Reply(Reply {
                game: 17,
                tick: 4,
                act: Act::Place {
                    x: 5,
                    y: 2,
                    dir: Direction::Left
                },
                wait: 3,
                note: None,
                garbled: None,
            })
        );
        // An action that does not exist is a reply all the same, read as
        // none and logged.
        let Incoming::Reply(reply) = read(r#"{"game": 1, "tick": 2, "act": "teleport"}"#) else {
            panic!("a reply all the same");
        };
        assert_eq!(reply.act, Act::None);
        assert!(reply.garbled.is_some());
        assert!(matches!(
            read("{nope"),
            Incoming::Garbled { game: None, .. }
        ));
        assert!(matches!(
            read(r#"{"game": 1, "tick": 2, "act": "place", "x": 300, "y": 1, "dir": "up"}"#),
            Incoming::Reply(Reply { act: Act::None, .. })
        ));
    }

    #[test]
    fn a_long_note_is_cut_not_refused() {
        let note = "x".repeat(NOTE_CAP * 2);
        let line = json!({"game": 1, "tick": 0, "act": "none", "note": note}).to_string();
        let Incoming::Reply(reply) = parse_incoming(&line) else {
            panic!("a reply");
        };
        assert_eq!(reply.note.map(|n| n.len()), Some(NOTE_CAP));
    }

    #[test]
    fn a_registration_is_tidied_on_the_way_in() {
        let line = json!({
            "type": "register", "protocol": 1, "name": "Greedy\u{7}\u{1b}[31m", "version": "0.3",
            "owner": "  ", "parallel": 0
        })
        .to_string();
        let Incoming::Register(r) = parse_incoming(&line) else {
            panic!("a registration");
        };
        assert_eq!(r.name, "Greedy[31m");
        assert_eq!(r.owner, None);
        assert_eq!(r.parallel, 1);
    }

    /// A bot that writes an empty token before it has one registers
    /// afresh, rather than being refused for a token nobody issued.
    #[test]
    fn an_empty_token_is_no_token() {
        let line =
            json!({"type": "register", "protocol": 1, "name": "New", "token": ""}).to_string();
        let Incoming::Register(r) = parse_incoming(&line) else {
            panic!("a registration");
        };
        assert_eq!(r.token, None);
    }

    /// A name or an owner ends up in a replay's `names:` line, which is
    /// split on `|`: one holding it would hand the seats after it the
    /// wrong names.
    #[test]
    fn a_name_cannot_carry_the_replay_separator() {
        let line = json!({
            "type": "register", "protocol": 1, "name": "a|b", "owner": "c|d",
        })
        .to_string();
        let Incoming::Register(r) = parse_incoming(&line) else {
            panic!("a registration");
        };
        assert!(!r.name.contains('|'), "{:?}", r.name);
        assert!(!r.owner.unwrap_or_default().contains('|'));
    }

    /// A `simulate` with a plan this protocol cannot read is a request
    /// answered with an error, not a reply to the tick: it must not stand
    /// for the bot's move.
    #[test]
    fn a_garbled_simulate_is_a_bad_request_not_an_answer() {
        let line = json!({
            "type": "simulate", "game": 7, "ticks": 60,
            "plan": [{"at": 0, "act": "place", "x": 5, "y": 2}],
        })
        .to_string();
        assert!(
            matches!(
                parse_incoming(&line),
                Incoming::BadRequest { game: Some(7), .. }
            ),
            "{:?}",
            parse_incoming(&line)
        );
    }
}
