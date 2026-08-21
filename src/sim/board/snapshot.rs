//! A complete board, written out and read back exactly.
//!
//! The level format describes a *starting* board: authoring data, tile
//! aligned, no creature mid-stride and no PRNG position. That is the right
//! shape for a level and the wrong shape for saving a round in progress,
//! which needs every field [`Board::state_hash`] covers or the board it
//! reloads is a different board from the next tick onward.
//!
//! So this is the other half: not pretty, not hand-authored, and complete.
//! `parse(to_snapshot(board))` has the same state hash as `board` for any
//! board, however far into a round it is.
//!
//! The completeness is held by the compiler, not by vigilance:
//! [`Board::parse_snapshot`] builds its result with a struct literal naming
//! every field, so a field added to `Board` fails to build here until
//! somebody decides how it travels.

use super::*;

/// Format marker. Bump it if the layout changes, so an old snapshot is
/// refused rather than half-read.
const HEADER: &str = "snapshot-v1";

impl Board {
    /// The whole board as text.
    pub fn to_snapshot(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let (rng_state, rng_inc) = self.rng.hash_state();
        let _ = writeln!(out, "{HEADER}");
        let _ = writeln!(out, "size: {} {}", self.width, self.height);
        let _ = writeln!(out, "seed: {}", self.seed);
        let _ = writeln!(out, "rng: {rng_state} {rng_inc}");
        let _ = writeln!(out, "tick: {}", self.tick);
        let _ = writeln!(
            out,
            "rule: {} {}",
            self.cap_policy.token(),
            self.signpost_cap
        );
        let _ = writeln!(
            out,
            "counters: {} {} {} {} {}",
            self.signpost_seq,
            self.next_crab_id,
            self.next_gull_id,
            self.crabs_banked,
            self.golden_banked
        );
        let scores: Vec<String> = self.scores.iter().map(u32::to_string).collect();
        let _ = writeln!(out, "scores: {}", scores.join(" "));
        let _ = writeln!(out, "gull_period: {}", self.gull_period);
        // Everything below is omitted at its default, as a board between
        // rounds mostly is.
        if let Some(len) = self.round_length {
            let _ = writeln!(out, "round: {len}");
        }
        if self.wrap {
            let _ = writeln!(out, "wrap: on");
        }
        if !self.castle_raids {
            let _ = writeln!(out, "raids: off");
        }
        if self.events_enabled {
            let _ = writeln!(out, "events: on");
        }
        if let Some((owner, ticks)) = self.lure {
            let _ = writeln!(out, "lure: {owner} {ticks}");
        }
        if self.lure_cooldown > 0 {
            let _ = writeln!(out, "cooldown: {}", self.lure_cooldown);
        }
        if let Some((mania, ticks)) = self.mania {
            let name = match mania {
                Mania::Crab => "crab",
                Mania::Gull => "gull",
            };
            let _ = writeln!(out, "mania: {name} {ticks}");
        }
        if let Some((tempo, ticks)) = self.tempo {
            let name = match tempo {
                Tempo::Fast => "fast",
                Tempo::Slow => "slow",
            };
            let _ = writeln!(out, "tempo: {name} {ticks}");
        }
        if let Some((event, at)) = self.last_event {
            let _ = writeln!(out, "last_event: {} {at}", event.index());
        }
        let _ = writeln!(out, "hwalls: {}", bits_to_hex(&self.h_walls));
        let _ = writeln!(out, "vwalls: {}", bits_to_hex(&self.v_walls));
        let tiles: Vec<String> = self.tiles.iter().map(|&kind| tile_token(kind)).collect();
        let _ = writeln!(out, "tiles: {}", tiles.join(" "));
        for (tile, slot) in self.signposts.iter().enumerate() {
            if let Some(post) = slot {
                let health = match post.health {
                    SignpostHealth::Full => "full",
                    SignpostHealth::Worn => "worn",
                };
                let _ = writeln!(
                    out,
                    "post: {tile} {} {} {health} {} {}",
                    post.dir.letter(),
                    post.owner,
                    post.seq,
                    post.placed
                );
            }
        }
        for crab in &self.crabs {
            let _ = writeln!(
                out,
                "crab: {} {} {} {} {} {} {} {} {}",
                crab.id,
                crab.tile,
                crab.dir.letter(),
                crab.progress,
                crab.prev_tile,
                crab.prev_progress,
                crab.prev_dir.letter(),
                crab.handed.token(),
                crab.kind.token()
            );
        }
        for gull in &self.gulls {
            // `remaining` is only meaningful mid-flight; a walking gull
            // writes a placeholder zero so every line has the same shape.
            let (state, remaining) = match gull.state {
                GullState::Walking => ("walk", 0),
                GullState::Flying { remaining } => ("fly", remaining),
            };
            let _ = writeln!(
                out,
                "gull: {} {} {} {} {} {} {} {} {state} {remaining} {}",
                gull.id,
                gull.tile,
                gull.dir.letter(),
                gull.progress,
                gull.prev_tile,
                gull.prev_progress,
                gull.prev_dir.letter(),
                gull.handed.token(),
                gull.takeoff_in
            );
        }
        out
    }

    /// Read a snapshot back, or say why it is not one.
    ///
    /// Strict where the level format is lenient: a snapshot is written by
    /// this build for this build, so a line it cannot read is a corrupt save
    /// rather than a hand edit to shrug at.
    pub fn parse_snapshot(text: &str) -> Result<Board, String> {
        let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
        match lines.next() {
            Some(HEADER) => {}
            Some(other) => return Err(format!("not a snapshot: {other:?}")),
            None => return Err("empty snapshot".to_string()),
        }
        let mut fields = Fields::default();
        for line in lines {
            let (key, value) = line
                .split_once(':')
                .ok_or_else(|| format!("no key in {line:?}"))?;
            fields.read(key.trim(), value.trim())?;
        }
        fields.build()
    }
}

/// A snapshot's lines, gathered before any of them is trusted.
///
/// The lines `to_snapshot` always writes are all `Option` here and all
/// unwrapped in [`Fields::build`]. Defaulting one instead would turn a
/// truncated save into a board that parses and plays differently, which is
/// the whole failure this format exists to avoid.
#[derive(Default)]
struct Fields {
    size: Option<(u8, u8)>,
    seed: Option<u64>,
    rng: Option<(u64, u64)>,
    tick: Option<u64>,
    rule: Option<(CapPolicy, u8)>,
    counters: Option<(u64, u32, u32, u32, u32)>,
    scores: Option<[u32; MAX_PLAYERS]>,
    gull_period: Option<u32>,
    round_length: Option<u32>,
    wrap: bool,
    /// Stored the way the wire stores it - the exception, not the rule -
    /// because `Default` here has to mean "the line was absent". Castle
    /// raids are the one board switch that is *on* by default, so a
    /// `castle_raids: bool` deriving `false` would quietly turn them off
    /// in every snapshot that omitted the line, which is every versus
    /// round there is.
    no_castle_raids: bool,
    events_enabled: bool,
    lure: Option<(PlayerId, u32)>,
    lure_cooldown: u32,
    mania: Option<(Mania, u32)>,
    tempo: Option<(Tempo, u32)>,
    last_event: Option<(TideEvent, u64)>,
    h_walls: Option<Vec<bool>>,
    v_walls: Option<Vec<bool>>,
    tiles: Option<Vec<TileKind>>,
    posts: Vec<(usize, Signpost)>,
    crabs: Vec<Crab>,
    gulls: Vec<Gull>,
}

impl Fields {
    /// Take one `key: value` line.
    fn read(&mut self, key: &str, value: &str) -> Result<(), String> {
        let mut words = value.split_whitespace();
        match key {
            "size" => {
                let w = next_num::<u8>(&mut words, "size width")?;
                let h = next_num::<u8>(&mut words, "size height")?;
                if w == 0 || h == 0 {
                    return Err("a board is at least 1x1".to_string());
                }
                self.size = Some((w, h));
            }
            "seed" => self.seed = Some(next_num(&mut words, "seed")?),
            "rng" => {
                let state = next_num(&mut words, "rng state")?;
                self.rng = Some((state, next_num(&mut words, "rng inc")?));
            }
            "tick" => self.tick = Some(next_num(&mut words, "tick")?),
            "rule" => {
                let token = words.next().ok_or("rule: missing policy")?;
                let policy = CapPolicy::from_token(token)
                    .ok_or_else(|| format!("rule: bad policy {token:?}"))?;
                self.rule = Some((policy, next_num(&mut words, "rule cap")?));
            }
            "counters" => {
                self.counters = Some((
                    next_num(&mut words, "signpost_seq")?,
                    next_num(&mut words, "next_crab_id")?,
                    next_num(&mut words, "next_gull_id")?,
                    next_num(&mut words, "crabs_banked")?,
                    next_num(&mut words, "golden_banked")?,
                ));
            }
            "scores" => {
                let mut seats = [0u32; MAX_PLAYERS];
                for (seat, slot) in seats.iter_mut().enumerate() {
                    *slot = next_num(&mut words, "score")
                        .map_err(|e| format!("{e} for seat {seat}"))?;
                }
                self.scores = Some(seats);
            }
            "gull_period" => self.gull_period = Some(next_num(&mut words, "gull_period")?),
            "round" => self.round_length = Some(next_num(&mut words, "round")?),
            "wrap" => self.wrap = value == "on",
            "raids" => self.no_castle_raids = value == "off",
            "events" => self.events_enabled = value == "on",
            "lure" => {
                let owner = next_num::<PlayerId>(&mut words, "lure owner")?;
                self.lure = Some((owner, next_num(&mut words, "lure ticks")?));
            }
            "cooldown" => self.lure_cooldown = next_num(&mut words, "cooldown")?,
            "mania" => {
                let which = words.next().ok_or("mania: missing kind")?;
                let kind = match which {
                    "crab" => Mania::Crab,
                    "gull" => Mania::Gull,
                    other => return Err(format!("mania: bad kind {other:?}")),
                };
                self.mania = Some((kind, next_num(&mut words, "mania ticks")?));
            }
            "tempo" => {
                let which = words.next().ok_or("tempo: missing speed")?;
                let shift = match which {
                    "fast" => Tempo::Fast,
                    "slow" => Tempo::Slow,
                    other => return Err(format!("tempo: bad speed {other:?}")),
                };
                self.tempo = Some((shift, next_num(&mut words, "tempo ticks")?));
            }
            "last_event" => {
                let index = next_num::<usize>(&mut words, "last_event index")?;
                let event = *TideEvent::ALL
                    .get(index)
                    .ok_or_else(|| format!("last_event: no event {index}"))?;
                self.last_event = Some((event, next_num(&mut words, "last_event tick")?));
            }
            "hwalls" => self.h_walls = Some(hex_to_bits(value)?),
            "vwalls" => self.v_walls = Some(hex_to_bits(value)?),
            "tiles" => {
                self.tiles = Some(
                    value
                        .split_whitespace()
                        .map(tile_from_token)
                        .collect::<Result<_, _>>()?,
                );
            }
            "post" => self.posts.push(parse_post(&mut words)?),
            "crab" => self.crabs.push(parse_crab(&mut words)?),
            "gull" => self.gulls.push(parse_gull(&mut words)?),
            other => return Err(format!("unknown key {other:?}")),
        }
        Ok(())
    }

    /// Everything gathered, checked against everything else, as a board.
    fn build(self) -> Result<Board, String> {
        let (width, height) = self.size.ok_or("snapshot has no size")?;
        let (rng_state, rng_inc) = self.rng.ok_or("snapshot has no rng")?;
        let (cap_policy, signpost_cap) = self.rule.ok_or("snapshot has no rule")?;
        let (signpost_seq, next_crab_id, next_gull_id, crabs_banked, golden_banked) =
            self.counters.ok_or("snapshot has no counters")?;
        let seed = self.seed.ok_or("snapshot has no seed")?;
        let tick = self.tick.ok_or("snapshot has no tick")?;
        let scores = self.scores.ok_or("snapshot has no scores")?;
        let gull_period = self.gull_period.ok_or("snapshot has no gull_period")?;
        let (w, h) = (width as usize, height as usize);
        let tiles = self.tiles.ok_or("snapshot has no tiles")?;
        if tiles.len() != w * h {
            return Err(format!("{} tiles for a {w}x{h} board", tiles.len()));
        }
        let h_walls = sized(
            self.h_walls.ok_or("snapshot has no hwalls")?,
            (h + 1) * w,
            "hwalls",
        )?;
        let v_walls = sized(
            self.v_walls.ok_or("snapshot has no vwalls")?,
            h * (w + 1),
            "vwalls",
        )?;
        let mut signposts = vec![None; w * h];
        for (tile, post) in self.posts {
            let slot = signposts
                .get_mut(tile)
                .ok_or_else(|| format!("post: tile {tile} is off a {w}x{h} board"))?;
            *slot = Some(post);
        }
        for crab in &self.crabs {
            if usize::from(crab.tile) >= w * h {
                return Err(format!("crab on tile {}, off the board", crab.tile));
            }
        }
        for gull in &self.gulls {
            if usize::from(gull.tile) >= w * h {
                return Err(format!("gull on tile {}, off the board", gull.tile));
            }
        }

        // Named in full on purpose: a new `Board` field stops compiling here
        // until it is decided how, or whether, it survives a save.
        Ok(Board {
            width,
            height,
            seed,
            h_walls,
            v_walls,
            tiles,
            signposts,
            crabs: self.crabs,
            scores,
            rng: Pcg32::from_state(rng_state, rng_inc),
            tick,
            signpost_seq,
            next_crab_id,
            signpost_cap,
            cap_policy,
            gulls: self.gulls,
            next_gull_id,
            gull_period,
            round_length: self.round_length,
            lure: self.lure,
            lure_cooldown: self.lure_cooldown,
            crabs_banked,
            golden_banked,
            castle_raids: !self.no_castle_raids,
            events_enabled: self.events_enabled,
            mania: self.mania,
            tempo: self.tempo,
            last_event: self.last_event,
            wrap: self.wrap,
            // Drained within the tick that fills it, so a snapshot taken
            // between ticks never has one to carry.
            event_queue: Vec::new(),
        })
    }
}

fn sized(bits: Vec<bool>, want: usize, what: &str) -> Result<Vec<bool>, String> {
    if bits.len() < want {
        return Err(format!("{what}: {} bits, wanted {want}", bits.len()));
    }
    let mut bits = bits;
    bits.truncate(want);
    Ok(bits)
}

fn next_num<T: std::str::FromStr>(
    words: &mut std::str::SplitWhitespace,
    what: &str,
) -> Result<T, String> {
    let word = words.next().ok_or_else(|| format!("{what}: missing"))?;
    word.parse()
        .map_err(|_| format!("{what}: bad number {word:?}"))
}

fn next_dir(words: &mut std::str::SplitWhitespace, what: &str) -> Result<Direction, String> {
    let word = words.next().ok_or_else(|| format!("{what}: missing"))?;
    Direction::from_letter(word).ok_or_else(|| format!("{what}: bad direction {word:?}"))
}

fn parse_post(words: &mut std::str::SplitWhitespace) -> Result<(usize, Signpost), String> {
    let tile = next_num::<usize>(words, "post tile")?;
    let dir = next_dir(words, "post direction")?;
    let owner = next_num::<PlayerId>(words, "post owner")?;
    let word = words.next().ok_or("post health: missing")?;
    let health = match word {
        "full" => SignpostHealth::Full,
        "worn" => SignpostHealth::Worn,
        other => return Err(format!("post: bad health {other:?}")),
    };
    let seq = next_num(words, "post seq")?;
    let placed = next_num(words, "post placed")?;
    Ok((
        tile,
        Signpost {
            dir,
            owner,
            health,
            seq,
            placed,
        },
    ))
}

fn parse_crab(words: &mut std::str::SplitWhitespace) -> Result<Crab, String> {
    let id = next_num(words, "crab id")?;
    let tile = next_num(words, "crab tile")?;
    let dir = next_dir(words, "crab direction")?;
    let progress = next_num(words, "crab progress")?;
    let prev_tile = next_num(words, "crab prev_tile")?;
    let prev_progress = next_num(words, "crab prev_progress")?;
    let prev_dir = next_dir(words, "crab prev_dir")?;
    let word = words.next().ok_or("crab handedness: missing")?;
    let handed =
        Handedness::from_token(word).ok_or_else(|| format!("crab: bad handedness {word:?}"))?;
    let word = words.next().ok_or("crab kind: missing")?;
    let kind = CrabKind::from_token(word).ok_or_else(|| format!("crab: bad kind {word:?}"))?;
    Ok(Crab {
        id,
        tile,
        dir,
        progress,
        prev_tile,
        prev_progress,
        prev_dir,
        handed,
        kind,
    })
}

fn parse_gull(words: &mut std::str::SplitWhitespace) -> Result<Gull, String> {
    let id = next_num(words, "gull id")?;
    let tile = next_num(words, "gull tile")?;
    let dir = next_dir(words, "gull direction")?;
    let progress = next_num(words, "gull progress")?;
    let prev_tile = next_num(words, "gull prev_tile")?;
    let prev_progress = next_num(words, "gull prev_progress")?;
    let prev_dir = next_dir(words, "gull prev_dir")?;
    let word = words.next().ok_or("gull handedness: missing")?;
    let handed =
        Handedness::from_token(word).ok_or_else(|| format!("gull: bad handedness {word:?}"))?;
    let word = words.next().ok_or("gull state: missing")?;
    let remaining = next_num(words, "gull flight remaining")?;
    let state = match word {
        "walk" => GullState::Walking,
        "fly" => GullState::Flying { remaining },
        other => return Err(format!("gull: bad state {other:?}")),
    };
    let takeoff_in = next_num(words, "gull takeoff_in")?;
    Ok(Gull {
        id,
        tile,
        dir,
        progress,
        prev_tile,
        prev_progress,
        prev_dir,
        handed,
        state,
        takeoff_in,
    })
}

/// Walls as hex nibbles, least-significant bit first.
fn bits_to_hex(bits: &[bool]) -> String {
    bits.chunks(4)
        .map(|chunk| {
            let nibble = chunk
                .iter()
                .enumerate()
                .fold(0u32, |acc, (i, &bit)| acc | (u32::from(bit) << i));
            char::from_digit(nibble, 16).expect("a nibble is one hex digit")
        })
        .collect()
}

fn hex_to_bits(text: &str) -> Result<Vec<bool>, String> {
    let mut bits = Vec::with_capacity(text.len() * 4);
    for ch in text.chars() {
        let nibble = ch
            .to_digit(16)
            .ok_or_else(|| format!("walls: {ch:?} is not hex"))?;
        bits.extend((0..4).map(|i| nibble & (1 << i) != 0));
    }
    Ok(bits)
}

fn tile_token(kind: TileKind) -> String {
    match kind {
        TileKind::Empty => ".".to_string(),
        TileKind::Rock => "#".to_string(),
        TileKind::Castle(owner) => format!("c{owner}"),
        TileKind::Spawner(s) => format!("s{}{}", s.dir.letter(), s.period),
        TileKind::Turnstile { next_right: true } => "T".to_string(),
        TileKind::Turnstile { next_right: false } => "t".to_string(),
        TileKind::Kelp => "K".to_string(),
        TileKind::Pool => "~".to_string(),
    }
}

fn tile_from_token(token: &str) -> Result<TileKind, String> {
    match token {
        "." => Ok(TileKind::Empty),
        "#" => Ok(TileKind::Rock),
        "T" => Ok(TileKind::Turnstile { next_right: true }),
        "t" => Ok(TileKind::Turnstile { next_right: false }),
        "K" => Ok(TileKind::Kelp),
        "~" => Ok(TileKind::Pool),
        _ => {
            let (tag, rest) = token.split_at_checked(1).ok_or("empty tile token")?;
            match tag {
                "c" => {
                    let owner: PlayerId = rest
                        .parse()
                        .map_err(|_| format!("tile: bad castle owner in {token:?}"))?;
                    if seat(owner).is_none() {
                        return Err(format!("tile: no seat {owner}"));
                    }
                    Ok(TileKind::Castle(owner))
                }
                "s" => {
                    let (letter, period) = rest
                        .split_at_checked(1)
                        .ok_or_else(|| format!("tile: bad spawner {token:?}"))?;
                    let dir = Direction::from_letter(letter)
                        .ok_or_else(|| format!("tile: bad spawner direction in {token:?}"))?;
                    let period: u32 = period
                        .parse()
                        .map_err(|_| format!("tile: bad spawner period in {token:?}"))?;
                    if period == 0 {
                        return Err("tile: a spawner period is at least 1 tick".to_string());
                    }
                    Ok(TileKind::Spawner(Spawner { dir, period }))
                }
                _ => Err(format!("tile: unknown token {token:?}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::crab::{CrabKind, Handedness};

    /// A board with every field pushed off its default, including the three
    /// the fuzz loop never reaches, because a mania, a tempo shift and a
    /// recorded last event all need a sparkling crab banked.
    ///
    /// Reaches in and sets the private fields directly rather than playing
    /// toward them: the point is coverage of the *format*, and a test that
    /// had to engineer a tide event to check one line would test the
    /// roulette instead.
    fn awkward_board() -> Board {
        let mut board = Board::new(5, 4, 0xABCD);
        board.set_tile(0, 0, TileKind::Castle(0));
        board.set_tile(4, 3, TileKind::Castle(1));
        board.set_tile(2, 1, TileKind::Rock);
        board.set_tile(1, 2, TileKind::Kelp);
        board.set_tile(3, 2, TileKind::Pool);
        board.set_tile(2, 2, TileKind::Turnstile { next_right: false });
        board.set_tile(
            0,
            2,
            TileKind::Spawner(Spawner {
                dir: Direction::Right,
                period: 37,
            }),
        );
        board.set_wrap(true);
        board.set_wall(1, 1, Direction::Up, true);
        board.set_castle_raids(false);
        board.set_events_enabled(true);
        board.set_round_length(Some(1234));
        board.set_gull_period(97);
        board.set_signpost_rule(2, CapPolicy::Reject);
        board.set_score(0, 17);
        board.set_score(3, 4);

        board.rng = Pcg32::from_state(0x1234_5678_9ABC_DEF0, 0x0FED_CBA9_8765_4321);
        board.tick = 4321;
        board.signpost_seq = 99;
        board.next_crab_id = 7;
        board.next_gull_id = 3;
        board.crabs_banked = 12;
        board.golden_banked = 2;
        board.lure = Some((1, 145));
        board.lure_cooldown = 60;
        board.mania = Some((Mania::Gull, 88));
        board.tempo = Some((Tempo::Slow, 44));
        board.last_event = Some((TideEvent::FreshSand, 3000));
        board.signposts[6] = Some(Signpost {
            dir: Direction::Left,
            owner: 1,
            health: SignpostHealth::Worn,
            seq: 5,
            placed: 300,
        });
        board.crabs.push(Crab {
            id: 4,
            tile: 8,
            dir: Direction::Down,
            progress: 133,
            prev_tile: 3,
            prev_progress: 200,
            prev_dir: Direction::Right,
            handed: Handedness::Right,
            kind: CrabKind::Golden,
        });
        board.gulls.push(Gull {
            id: 2,
            tile: 9,
            dir: Direction::Up,
            progress: 77,
            prev_tile: 14,
            prev_progress: 12,
            prev_dir: Direction::Left,
            handed: Handedness::Left,
            state: GullState::Flying { remaining: 3 },
            takeoff_in: 222,
        });
        board
    }

    #[test]
    fn a_board_with_everything_set_survives_a_snapshot() {
        let board = awkward_board();
        let text = board.to_snapshot();
        let back = Board::parse_snapshot(&text).expect("its own output");
        assert_eq!(
            back.state_hash(),
            board.state_hash(),
            "the snapshot came back a different board:\n{text}"
        );
        // The lines that only this test reaches are really being written,
        // rather than the hash matching because both sides dropped them.
        for expected in [
            "lure: 1 145",
            "cooldown: 60",
            "mania: gull 88",
            "tempo: slow 44",
            "last_event: 6 3000",
            "wrap: on",
            "raids: off",
            "events: on",
            "rule: reject 2",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in\n{text}");
        }
        // And the default direction, which is the one a missing line has to
        // mean: raids on writes nothing and must come back on.
        let mut raiding = awkward_board();
        raiding.set_castle_raids(true);
        let text = raiding.to_snapshot();
        assert!(
            !text.contains("raids:"),
            "a default is not written:\n{text}"
        );
        assert!(
            Board::parse_snapshot(&text)
                .expect("its own output")
                .castle_raids(),
            "an absent raids line must read as raids on:\n{text}"
        );
        assert!(text.contains(" worn "), "the worn signpost:\n{text}");
        assert!(text.contains(" fly 3 "), "the gull mid-flight:\n{text}");
    }

    /// A snapshot from another build, or no snapshot at all, is refused
    /// rather than half-read into a board that plays differently.
    #[test]
    fn what_is_not_a_snapshot_is_refused() {
        let good = awkward_board().to_snapshot();
        assert!(Board::parse_snapshot("").is_err(), "empty");
        assert!(Board::parse_snapshot("hello").is_err(), "not a snapshot");
        assert!(
            Board::parse_snapshot(&good.replace(HEADER, "snapshot-v2")).is_err(),
            "another version"
        );
        assert!(
            Board::parse_snapshot(&good.replace("tiles:", "tyles:")).is_err(),
            "a key this build does not know"
        );
        assert!(
            Board::parse_snapshot(&good.replace("size: 5 4", "size: 6 4")).is_err(),
            "a tile count that does not fit the size"
        );
        // A truncated save: every line but the header removed in turn.
        let lines: Vec<&str> = good.lines().collect();
        for drop in 1..lines.len() {
            let mut kept = lines.clone();
            let line = kept.remove(drop);
            // The optional round-state lines are absent at their defaults,
            // so dropping one is a legal (different) board, not a bad file.
            let optional = [
                "round:",
                "wrap:",
                "raids:",
                "events:",
                "lure:",
                "cooldown:",
                "mania:",
                "tempo:",
                "last_event:",
                "post:",
                "crab:",
                "gull:",
            ];
            if optional
                .iter()
                .any(|key| line.trim_start().starts_with(key))
            {
                continue;
            }
            assert!(
                Board::parse_snapshot(&kept.join("\n")).is_err(),
                "dropping {line:?} should not still parse"
            );
        }
    }
}
