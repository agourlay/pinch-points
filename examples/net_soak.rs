//! Soak the online game: whole rounds at virtual time over loopback, with
//! loss, bursts, delay and jitter on every spoke, and the numbers that say
//! how the table coped.
//!
//! Run: `cargo run --example net_soak`, steered by the environment (each
//! optional, defaults in `soak::table::Scenario`): `GAMES`, `PLAYERS`,
//! `WATCHERS`, `STEPS`, `LOSS` (0..1 per datagram), `BURST_EVERY` (mean
//! steps between bursts on a leg) and `BURST_LEN`, `LATENCY` and `JITTER`
//! (steps per leg), `SKEW` (joiner 1 starts late), `HITCH_AT` and `HITCH`
//! (joiner 1 freezes), `PAUSE_AT`, `PAUSE_STORM` (joiners pausing) and
//! `PAUSE_LEN`, `DELAY` (input delay; unset, the one a host would pick
//! from the worst round trip), `SEED`. A second round follows the first
//! when `ROUND_FRAMES` (the first's length) is set: `CARD` and `BEAT` are
//! the results card and the interlude in steps, `OPENING` the line-up and
//! count after it, `LISTEN=false` keeps every peer silent through those as
//! the game once did, `STRAGGLE` the chance an
//! input datagram near the end is held past the deal, and
//! `LOSE_DEAL_START=true` loses the deal's `Start` to joiner 1, and
//! `WATCHER_LAG` stops the last watcher that many steps before round 1
//! ends. `AI` seats that many AI after the players, `ORDER` deals the
//! places (the host, the joiners, then the AI) into the seats listed,
//! `0,2,1,3`, as a host's partner picks do, and `TEAMS` (solo, pairs or
//! trios) is the scoring the terms carry. `FROM_LOBBY=false` plays the
//! direct `PINCH_HOST` pair's rules rather than a lobby table's, and
//! `LATE` lists the steps a late watcher greets the host at, `300,900`:
//! handed the round as it stands mid-round, dealt the next between.

#[path = "soak/table.rs"]
mod table;

fn env<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// A switch, `true` or `1` and `false` or `0`: one that read as neither
/// used to fall back to its default without a word.
fn flag(name: &str, default: bool) -> bool {
    match std::env::var(name).ok().as_deref() {
        None => default,
        Some("true" | "1") => true,
        Some("false" | "0") => false,
        Some(other) => panic!("{name}: true or false, not {other}"),
    }
}

/// A seat order written as the seats of the places in turn, `0,2,1,3`:
/// the places not named keep their own seats.
fn seat_order(written: &str) -> [u8; 6] {
    let mut order = pinch_points::transport::IN_SEAT_ORDER;
    for (place, seat) in written.split(',').enumerate().take(order.len()) {
        order[place] = seat
            .trim()
            .parse()
            .expect("ORDER: seats as numbers, 0,2,1,3");
    }
    order
}

/// Steps written as a list, `300,900`, and none written as nothing.
fn steps(written: &str) -> Vec<u64> {
    written
        .split(',')
        .map(str::trim)
        .filter(|step| !step.is_empty())
        .map(|step| step.parse().expect("LATE: steps as numbers, 300,900"))
        .collect()
}

/// A scoring mode by name, as the terms number it.
fn team_mode(written: &str) -> u8 {
    match written {
        "solo" | "ffa" | "0" => 0,
        "pairs" | "1" => 1,
        "trios" | "2" => 2,
        _ => panic!("TEAMS: solo, pairs or trios"),
    }
}

fn main() {
    let base = table::Scenario::default();
    let scenario = table::Scenario {
        games: env("GAMES", base.games),
        players: env("PLAYERS", base.players),
        ai: env("AI", base.ai),
        order: std::env::var("ORDER")
            .ok()
            .map_or(base.order, |order| seat_order(&order)),
        teams: std::env::var("TEAMS")
            .ok()
            .map_or(base.teams, |teams| team_mode(&teams)),
        watchers: env("WATCHERS", base.watchers),
        steps: env("STEPS", base.steps),
        loss: env("LOSS", base.loss),
        burst_every: env("BURST_EVERY", base.burst_every),
        burst_len: env("BURST_LEN", base.burst_len),
        latency: env("LATENCY", base.latency),
        jitter: env("JITTER", base.jitter),
        skew: env("SKEW", base.skew),
        hitch_at: env("HITCH_AT", base.hitch_at),
        hitch: env("HITCH", base.hitch),
        pause_at: env("PAUSE_AT", base.pause_at),
        pause_storm: env("PAUSE_STORM", base.pause_storm),
        pause_len: env("PAUSE_LEN", base.pause_len),
        // A number, or nothing for the delay a host would pick.
        delay: std::env::var("DELAY").ok().and_then(|v| v.parse().ok()),
        seed: env("SEED", base.seed),
        round_frames: env("ROUND_FRAMES", base.round_frames),
        card: env("CARD", base.card),
        beat: env("BEAT", base.beat),
        opening: env("OPENING", base.opening),
        listen: flag("LISTEN", base.listen),
        straggle: env("STRAGGLE", base.straggle),
        lose_deal_start: flag("LOSE_DEAL_START", base.lose_deal_start),
        watcher_lag: env("WATCHER_LAG", base.watcher_lag),
        from_lobby: flag("FROM_LOBBY", base.from_lobby),
        late: std::env::var("LATE")
            .ok()
            .map_or(base.late, |late| steps(&late)),
    };
    let started = std::time::Instant::now();
    let report = table::play(&scenario);
    print!("{}", table::describe(&scenario, &report));
    println!("wall {:?}", started.elapsed());
}
