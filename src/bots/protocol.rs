//! The bot protocol's messages: what a listener writes and how it reads
//! what comes back. Version 1, as `docs/bot-protocol.md` publishes it.
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

/// The protocol version this build speaks.
pub const PROTOCOL: u64 = 1;
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

pub fn dir_token(dir: Direction) -> &'static str {
    match dir {
        Direction::Up => "up",
        Direction::Down => "down",
        Direction::Left => "left",
        Direction::Right => "right",
    }
}

fn dir_from_token(token: &str) -> Option<Direction> {
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
            "round_ticks": board.round_length(),
            "gull_period": board.gull_period(),
            "castle_raids": board.castle_raids(),
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
        .map(|crab| {
            let (x, y) = board.coords_u8(crab.tile);
            json!({
                "id": crab.id, "x": x, "y": y, "dir": dir_token(crab.dir),
                "progress": crab.progress, "kind": kind_token(crab.kind),
                "claw": claw_token(crab.handed),
            })
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
    let now = board.ticks();
    let signposts: Vec<Value> = board
        .signposts()
        .map(|(x, y, post)| {
            json!({
                "x": x, "y": y, "dir": dir_token(post.dir), "owner": post.owner,
                "worn": post.health == SignpostHealth::Worn,
                "age": now.saturating_sub(post.placed),
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
        "gulls": gulls,
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
        "claw_call": board.in_claw_call(),
        "surge": board.in_surge(),
    })
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
    /// Not something this protocol says. Kept with the game it named, if
    /// it named one, so a reply that was garbled still costs its tick.
    Garbled {
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
/// names are drawn on screen and written into logs, never interpreted.
pub fn tidy(text: &str, cap: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control())
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
            token: v.get("token").and_then(Value::as_str).map(|t| tidy(t, 64)),
        }),
        Some("ready") => match game {
            Some(game) => Incoming::Ready { game },
            None => Incoming::Garbled {
                game: None,
                why: "`ready` names no game".to_string(),
            },
        },
        Some("pong") => Incoming::Pong {
            id: uint(&v, "id").unwrap_or(0),
        },
        Some("replay") => Incoming::Replay {
            id: v
                .get("id")
                .and_then(Value::as_str)
                .map(|id| tidy(id, 32))
                .unwrap_or_default(),
        },
        Some("simulate") => {
            let Some(game) = game else {
                return Incoming::Garbled {
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
                        return Incoming::Garbled {
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
}
