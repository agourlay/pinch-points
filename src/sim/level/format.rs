//! The level text format: parsing a level file into a [`Board`], and
//! writing one back out.
//!
//! Kept apart from the level *model* next door: the format is the only
//! thing that cares about glyphs, lattices and `key: value` headers, and it
//! is the half with a round-trip contract to keep (see `tests/format.rs`).

use super::{Goal, Level, LevelKind};
use crate::sim::board::{Board, CapPolicy, TileKind};
use crate::sim::crab::{CrabKind, Handedness};
use crate::sim::direction::Direction;
use crate::sim::solve::Placement;
use crate::sim::{PlayerId, Spawner};

impl Level {
    pub fn parse(text: &str) -> Result<Level, String> {
        let mut lines = text.lines();
        let header = parse_header(&mut lines)?;
        let mut board = parse_lattice(lines, header.seed)?;
        place_entities(&mut board, &header)?;
        let level = Level {
            name: header.name.ok_or("missing name:")?,
            posts: header.posts.ok_or("missing posts:")?,
            solution: header.solution,
            goal: header.goal,
            // Filled in below: a file written before the editor had a toggle
            // has no `kind:` line and has to be read off the board.
            kind: LevelKind::Puzzle,
            crab_count: header.crabs.len() as u32,
            explicit_rule: header.rule.is_some(),
            board,
        };
        let kind = header.kind.unwrap_or_else(|| inferred_kind(&level));
        Ok(level.with_kind(kind))
    }

    /// Wrap an editor-built board as a level. The board is snapshotted as-is
    /// (crabs, gulls, walls, timers included), and the kind is read off it -
    /// call [`Level::with_kind`] to say otherwise, which is what the editor's
    /// toggle does.
    pub fn from_board(name: impl Into<String>, posts: u8, board: Board) -> Level {
        let level = Level {
            name: name.into(),
            posts,
            solution: Vec::new(),
            goal: Goal::AllCrabs,
            kind: LevelKind::Puzzle,
            crab_count: board.crabs().len() as u32,
            // The board's live rule is authoritative and serialized as-is.
            explicit_rule: true,
            board,
        };
        let kind = inferred_kind(&level);
        level.with_kind(kind)
    }

    /// Serialize back to the text format `parse` reads. Round-trips exactly
    /// for un-ticked boards (creature sub-tile state is not representable,
    /// by design, since levels describe starting states).
    pub fn to_text(&self) -> String {
        use std::fmt::Write;
        let board = &self.board;
        let mut out = String::new();
        let _ = writeln!(out, "name: {}", self.name);
        let _ = writeln!(out, "posts: {}", self.posts);
        // Always written, both values: what the author chose is not always
        // what the board looks like, and a file that leaves it out is read
        // by guessing at the castles.
        let _ = writeln!(out, "kind: {}", self.kind.token());
        let _ = writeln!(out, "seed: {}", board.seed());
        // Puzzle levels (no explicit rule) reconstruct their rule from
        // `posts:` on parse; writing one would freeze the wrong default.
        if self.explicit_rule {
            let (cap, policy) = board.signpost_rule();
            let _ = writeln!(out, "rule: {} {cap}", policy.token());
        }
        for (player, &score) in board.scores().iter().enumerate() {
            if score > 0 {
                let _ = writeln!(out, "score: {player} {score}");
            }
        }
        for crab in board.crabs() {
            let (x, y) = board.coords_u8(crab.tile);
            let _ = writeln!(
                out,
                "crab: {x},{y} {} {} {}",
                crab.dir.letter(),
                crab.handed.token(),
                crab.kind.token()
            );
        }
        for (x, y, kind) in board.tiles() {
            if let TileKind::Spawner(s) = kind {
                let _ = writeln!(out, "spawner: {x},{y} {} {}", s.dir.letter(), s.period);
            }
        }
        for gull in board.gulls() {
            let (x, y) = board.coords_u8(gull.tile);
            let _ = writeln!(out, "gull: {x},{y} {}", gull.dir.letter());
        }
        if board.gull_period() > 0 {
            let _ = writeln!(out, "gull_period: {}", board.gull_period());
        }
        if let Some(round) = board.round_length() {
            let _ = writeln!(out, "round: {round}");
        }
        if board.wrap() {
            let _ = writeln!(out, "wrap: on");
        }
        // Tide events are game state, not authoring flavour: dropping the
        // flag made replays of versus rounds silently diverge.
        if board.events_enabled() {
            let _ = writeln!(out, "events: on");
        }
        match self.goal {
            Goal::AllCrabs => {}
            Goal::Bank(n) => {
                let _ = writeln!(out, "goal: bank {n}");
            }
            Goal::Survive => {
                let _ = writeln!(out, "goal: survive");
            }
            Goal::Golden => {
                let _ = writeln!(out, "goal: golden");
            }
        }
        if !self.solution.is_empty() {
            let parts: Vec<String> = self
                .solution
                .iter()
                .map(|&(x, y, dir)| format!("{x},{y} {}", dir.letter()))
                .collect();
            let _ = writeln!(out, "solution: {}", parts.join("; "));
        }
        let _ = writeln!(out, "map:");

        // Lattice: (2h+1) rows × (2w+1) columns.
        let (w, h) = (board.width() as usize, board.height() as usize);
        let mut lattice = vec![vec![' '; 2 * w + 1]; 2 * h + 1];
        for row in lattice.iter_mut().step_by(2) {
            for cell in row.iter_mut().step_by(2) {
                *cell = '+';
            }
        }
        for (x, y, kind) in board.tiles() {
            let (lx, ly) = (2 * x as usize + 1, 2 * y as usize + 1);
            lattice[ly][lx] = tile_glyph(kind);
            if board.wall_at(x, y, Direction::Up) {
                lattice[ly - 1][lx] = '-';
            }
            if board.wall_at(x, y, Direction::Left) {
                lattice[ly][lx - 1] = '|';
            }
            if board.wall_at(x, y, Direction::Down) {
                lattice[ly + 1][lx] = '-';
            }
            if board.wall_at(x, y, Direction::Right) {
                lattice[ly][lx + 1] = '|';
            }
        }
        for row in lattice {
            let line: String = row.into_iter().collect();
            let _ = writeln!(out, "{}", line.trim_end());
        }
        out
    }
}

/// What a level with nothing to say about itself must be: castles for two
/// seats or more is a beach nobody plays alone. Only reached for text
/// written before the format carried a `kind:` line, and for boards handed
/// straight to [`Level::from_board`] (a recorded versus round, which is an
/// arena, and an editor snapshot, whose kind the editor then states).
fn inferred_kind(level: &Level) -> LevelKind {
    match level.seats() >= 2 {
        true => LevelKind::Arena,
        false => LevelKind::Puzzle,
    }
}

fn parse_xy(s: &str) -> Result<(u8, u8, &str), String> {
    let (xy, rest) = match s.split_once(char::is_whitespace) {
        Some((xy, rest)) => (xy, rest),
        None => (s, ""),
    };
    let (x, y) = xy
        .split_once(',')
        .ok_or_else(|| format!("expected x,y in {s:?}"))?;
    let x = x.trim().parse::<u8>().map_err(|e| format!("x: {e}"))?;
    let y = y.trim().parse::<u8>().map_err(|e| format!("y: {e}"))?;
    Ok((x, y, rest))
}

/// Everything the `key: value` header section declares, gathered before
/// the map lattice is read.
struct Header {
    name: Option<String>,
    posts: Option<u8>,
    solution: Vec<Placement>,
    crabs: Vec<(u8, u8, Direction, Handedness, CrabKind)>,
    spawners: Vec<(u8, u8, Direction, u32)>,
    gulls: Vec<(u8, u8, Direction)>,
    gull_period: u32,
    events: bool,
    round: Option<u32>,
    seed: u64,
    rule: Option<(u8, CapPolicy)>,
    scores: Vec<(u8, u32)>,
    goal: Goal,
    wrap: bool,
    /// `None` when the file predates the editor's puzzle/arena toggle.
    kind: Option<LevelKind>,
}

impl Default for Header {
    fn default() -> Header {
        Header {
            name: None,
            posts: None,
            solution: Vec::new(),
            crabs: Vec::new(),
            spawners: Vec::new(),
            gulls: Vec::new(),
            gull_period: 0,
            events: false,
            round: None,
            // An arbitrary but fixed default, so a level file without a
            // `seed:` still replays identically everywhere.
            seed: 0x7ead_0001,
            rule: None,
            scores: Vec::new(),
            goal: Goal::AllCrabs,
            wrap: false,
            kind: None,
        }
    }
}

/// What one header line did: carried a value, or ended the header.
enum HeaderLine {
    Read,
    /// `map:`, after which the lattice starts on the next line.
    MapFollows,
}

impl Header {
    /// Fold one `key: value` line in. Each key parses its own value and
    /// says so in its own error message, which is the sort of thing a
    /// hand-edited level file needs to hear.
    fn read_line(&mut self, key: &str, value: &str) -> Result<HeaderLine, String> {
        match key {
            "name" => self.name = Some(value.to_string()),
            "posts" => {
                self.posts = Some(value.parse::<u8>().map_err(|e| format!("posts: {e}"))?);
            }
            "crab" => {
                let (x, y, rest) = parse_xy(value)?;
                let mut parts = rest.split_whitespace();
                let dir = parse_dir(parts.next().ok_or("crab: missing direction")?)?;
                let handed = parts.next().ok_or("crab: missing handedness")?;
                let handed = Handedness::from_token(handed)
                    .ok_or_else(|| format!("crab: bad handedness {handed:?}"))?;
                let kind = parts.next().ok_or("crab: missing kind")?;
                let kind =
                    CrabKind::from_token(kind).ok_or_else(|| format!("crab: bad kind {kind:?}"))?;
                self.crabs.push((x, y, dir, handed, kind));
            }
            "spawner" => {
                let (x, y, rest) = parse_xy(value)?;
                let mut parts = rest.split_whitespace();
                let dir = parse_dir(parts.next().ok_or("spawner: missing direction")?)?;
                let period = parts
                    .next()
                    .ok_or("spawner: missing period")?
                    .parse::<u32>()
                    .map_err(|e| format!("spawner period: {e}"))?;
                self.spawners.push((x, y, dir, period));
            }
            "gull" => {
                let (x, y, rest) = parse_xy(value)?;
                let dir = parse_dir(rest.trim())?;
                self.gulls.push((x, y, dir));
            }
            "gull_period" => {
                self.gull_period = value
                    .parse::<u32>()
                    .map_err(|e| format!("gull_period: {e}"))?;
            }
            "round" => {
                self.round = Some(value.parse::<u32>().map_err(|e| format!("round: {e}"))?);
            }
            "seed" => {
                self.seed = value.parse::<u64>().map_err(|e| format!("seed: {e}"))?;
            }
            "goal" => {
                self.goal = match value.split_once(' ') {
                    Some(("bank", n)) => {
                        Goal::Bank(n.trim().parse().map_err(|e| format!("goal bank: {e}"))?)
                    }
                    None if value == "survive" => Goal::Survive,
                    None if value == "golden" => Goal::Golden,
                    None if value == "all" => Goal::AllCrabs,
                    _ => return Err(format!("goal: bad value {value:?}")),
                };
            }
            "kind" => {
                self.kind = Some(
                    LevelKind::from_token(value)
                        .ok_or_else(|| format!("kind: bad value {value:?}"))?,
                );
            }
            "wrap" => self.wrap = value == "on",
            "events" => self.events = value == "on",
            "rule" => {
                let (policy, cap) = value
                    .split_once(' ')
                    .ok_or("rule: expected `<evict|reject> <cap>`")?;
                let cap = cap
                    .trim()
                    .parse::<u8>()
                    .map_err(|e| format!("rule cap: {e}"))?;
                let policy = policy.trim();
                let policy = CapPolicy::from_token(policy)
                    .ok_or_else(|| format!("rule: bad policy {policy:?}"))?;
                self.rule = Some((cap, policy));
            }
            "score" => {
                let (player, score) = value
                    .split_once(' ')
                    .ok_or("score: expected `<player> <score>`")?;
                let player = player
                    .trim()
                    .parse::<u8>()
                    .map_err(|e| format!("score: {e}"))?;
                let score = score
                    .trim()
                    .parse::<u32>()
                    .map_err(|e| format!("score: {e}"))?;
                if crate::sim::board::seat(player).is_none() {
                    return Err(format!("score: player {player} out of range"));
                }
                self.scores.push((player, score));
            }
            "solution" => {
                for placement in value.split(';') {
                    let (x, y, rest) = parse_xy(placement.trim())?;
                    let dir = parse_dir(rest.trim())?;
                    self.solution.push((x, y, dir));
                }
            }
            "map" => return Ok(HeaderLine::MapFollows),
            other => return Err(format!("unknown key {other:?}")),
        }
        Ok(HeaderLine::Read)
    }
}

/// Stage 1: the `key: value` lines up to (and consuming) `map:`.
fn parse_header(lines: &mut std::str::Lines) -> Result<Header, String> {
    let mut header = Header::default();
    for line in lines.by_ref() {
        let line = line.trim();
        // Comments are only possible before the map (map rows use '#').
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            return Err(format!("expected `key: value` before map, got {line:?}"));
        };
        if let HeaderLine::MapFollows = header.read_line(key.trim(), value.trim())? {
            break;
        }
    }
    Ok(header)
}

/// Stage 2: the odd-sized wall lattice into a board of tiles and walls.
fn parse_lattice<'a>(lines: impl Iterator<Item = &'a str>, seed: u64) -> Result<Board, String> {
    let lattice: Vec<&str> = lines.filter(|l: &&str| !l.trim().is_empty()).collect();
    if lattice.is_empty() {
        return Err("missing map section".into());
    }
    let lat_h = lattice.len();
    let lat_w = lattice[0].chars().count();
    if lat_h.is_multiple_of(2) || lat_w.is_multiple_of(2) {
        return Err(format!(
            "map lattice must be odd-sized, got {lat_w}×{lat_h}"
        ));
    }
    // A lattice spends `2n + 1` lines on `n` tiles, so three is the
    // narrowest that holds one and `2·255 + 1` the widest a `u8` dimension
    // can name. Odd-sized alone let both ends through: a lone border line
    // made a zero-sized board, which `Board::new` refuses with a panic, and
    // an over-wide one wrapped the cast: a 300-tile beach loading silently
    // as 44, which is worse than any rejection.
    if lat_w < 3 || lat_h < 3 {
        return Err(format!(
            "map lattice must hold at least one tile, got {lat_w}×{lat_h}"
        ));
    }
    let (tiles_w, tiles_h) = (lat_w / 2, lat_h / 2);
    let (Ok(width), Ok(height)) = (u8::try_from(tiles_w), u8::try_from(tiles_h)) else {
        return Err(format!(
            "map is {tiles_w}×{tiles_h} tiles, past the {} a side the format can name",
            u8::MAX
        ));
    };
    // Short rows pad with spaces rather than erroring: text editors trim
    // trailing whitespace, and wrap-map rows legitimately end in spaces
    // (open edges) - to_text itself emits them trimmed.
    let grid: Vec<Vec<char>> = lattice
        .iter()
        .map(|row| {
            let mut chars: Vec<char> = row.chars().collect();
            chars.resize(lat_w, ' ');
            chars
        })
        .collect();

    // The seed matters only where the PRNG is consumed (spawner and gull
    // handedness, takeoff timers); serialized levels carry it so replays
    // rebuild bit-identical boards.
    // Both were just checked against the lattice; `Board::new` panics on a
    // zero, which is how a lone border line once took the game down.
    debug_assert!(width > 0 && height > 0, "a {width}x{height} board");
    let mut board = Board::new(width, height, seed);
    for y in 0..height {
        for x in 0..width {
            let c = grid[y as usize * 2 + 1][x as usize * 2 + 1];
            match tile_from_glyph(c) {
                Some(TileKind::Empty) => {}
                Some(tile) => board.set_tile(x, y, tile),
                None => return Err(format!("bad tile char {c:?} at ({x},{y})")),
            }
            // Up edge of (x, y) is lattice row 2y, column 2x+1.
            if grid[y as usize * 2][x as usize * 2 + 1] == '-' {
                board.set_wall(x, y, Direction::Up, true);
            }
            // Left edge is lattice row 2y+1, column 2x.
            if grid[y as usize * 2 + 1][x as usize * 2] == '|' {
                board.set_wall(x, y, Direction::Left, true);
            }
        }
    }
    // Bottom and right borders of the lattice.
    for x in 0..width {
        if grid[height as usize * 2][x as usize * 2 + 1] == '-' {
            board.set_wall(x, height - 1, Direction::Down, true);
        }
    }
    for y in 0..height {
        if grid[y as usize * 2 + 1][width as usize * 2] == '|' {
            board.set_wall(width - 1, y, Direction::Right, true);
        }
    }
    Ok(board)
}

/// Stage 3: header-declared spawners, creatures, and board switches, with
/// bounds validation against the lattice-derived size.
fn place_entities(board: &mut Board, header: &Header) -> Result<(), String> {
    let (width, height) = (board.width(), board.height());
    let check = |what: &str, x: u8, y: u8| -> Result<(), String> {
        if x >= width || y >= height {
            return Err(format!(
                "{what} at ({x},{y}) is off the {width}x{height} board"
            ));
        }
        Ok(())
    };
    for &(x, y, dir, period) in &header.spawners {
        check("spawner", x, y)?;
        if period == 0 {
            return Err(format!("spawner at ({x},{y}): period must be at least 1"));
        }
        board.set_tile(x, y, TileKind::Spawner(Spawner { dir, period }));
    }
    for &(x, y, dir, handed, kind) in &header.crabs {
        check("crab", x, y)?;
        if board.tile_at(x, y) == TileKind::Rock {
            return Err(format!("crab at ({x},{y}) is standing on a rock"));
        }
        board.spawn_crab(x, y, dir, handed, kind);
    }
    for &(x, y, dir) in &header.gulls {
        check("gull", x, y)?;
        if board.tile_at(x, y) == TileKind::Rock {
            return Err(format!("gull at ({x},{y}) is standing on a rock"));
        }
        board.spawn_gull(x, y, dir);
    }
    board.set_gull_period(header.gull_period);
    board.set_round_length(header.round);
    if header.wrap {
        board.set_wrap(true);
    }
    if header.events {
        board.set_events_enabled(true);
    }
    if let Some((cap, policy)) = header.rule {
        board.set_signpost_rule(cap, policy);
    }
    for &(player, score) in &header.scores {
        board.set_score(player, score);
    }
    Ok(())
}

/// The map glyph for a tile; `tile_from_glyph` is its inverse (spawners
/// serialize in the header, so they read back as sand here).
fn tile_glyph(tile: TileKind) -> char {
    match tile {
        TileKind::Empty | TileKind::Spawner(_) => '.',
        TileKind::Rock => '#',
        TileKind::Castle(owner) => (b'0' + owner) as char,
        // Upper case is a log about to deflect right, lower case one about to
        // deflect left. The pivot has to survive: generated arenas mirror
        // their logs (a reflection swaps left and right), and a replay stores
        // its starting board *as* a level.
        TileKind::Turnstile { next_right: true } => 'T',
        TileKind::Turnstile { next_right: false } => 't',
        TileKind::Kelp => 'K',
        TileKind::Pool => '~',
    }
}

fn tile_from_glyph(c: char) -> Option<TileKind> {
    match c {
        '.' | ' ' => Some(TileKind::Empty),
        '#' => Some(TileKind::Rock),
        'T' => Some(TileKind::Turnstile { next_right: true }),
        't' => Some(TileKind::Turnstile { next_right: false }),
        'K' => Some(TileKind::Kelp),
        '~' => Some(TileKind::Pool),
        // As many seats as the sim has, not the four it had when the glyphs
        // were chosen: a six-castle board writes '4' and '5', and a reader
        // that rejects them turns every six-player replay into a parse error.
        '0'..='9' if crate::sim::board::seat(c as u8 - b'0').is_some() => {
            Some(TileKind::Castle((c as u8 - b'0') as PlayerId))
        }
        _ => None,
    }
}

fn parse_dir(s: &str) -> Result<Direction, String> {
    Direction::from_letter(s).ok_or_else(|| format!("bad direction {s:?}"))
}
