//! `pinch-points cup serve`: a competition between bots.
//!
//! The organiser starts a server and registration opens. Entrants connect
//! with the printed string; the game's AI can be entered as a house bot,
//! a fixed yardstick for any field. On `start` the cup is drawn (see
//! `draw`) and played, as many games at once as the entrants' `parallel`
//! capacities allow, and the standings rank every entrant by its average
//! points per game.

use super::cli::{self, Args, Beach};
use super::connstr::{ConnString, DEFAULT_PORT, Key, reachable_host};
use super::draw::{self, Fixture};
use super::game::{self, GameResult, GameSpec, Seat, SeatResult, Sinks};
use super::listener::{Admission, BotId, Config, Event, Invite, Listener};
use super::protocol::Clock;
use crate::app::match_setup::RoundLength;
use crate::sim::{BotLevel, TICKS_PER_SECOND};
use std::io::{BufRead, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

pub const USAGE: &str = "\
pinch-points cup serve [flags]

Run a cup: bots register with the printed string, `start` plays it.
Console commands while registration is open: start, invite NAME, list.

  --listen ADDR            where bots connect (default 0.0.0.0:47710)
  --port P                 the port to listen on
  --seeds N                beaches the cup is played on (default 10)
  --seats N                chairs a table (default 4)
  --add ai:<level>         enter the game's AI as a house bot; repeatable
  --tables N               tables drawn per beach (default: enough for everyone)
  --start-when N           start on its own once N bots have registered
  --invite NAME            print a string bound to that owner; repeatable
  --open-registration      no key: anyone who can reach the port may enter
  --per-owner N            bots one owner may enter (default 1)
  --parallel-cap N         games one bot may play at once, at most (default 8)
  --map NAME|FILE          classic, generated, small, large, xl, ocean, or a level file
  --round short|standard|long
  --deadline MS            the reply deadline (default 33, one tick)
  --fair-cursor on|off     walk every seat's cursor at a person's pace
  --forfeit-after S        how long a game waits for a missing bot (default 60)
  --seed S                 the first beach's seed and the draw's (default random)
  --id NAME                the cup's name in replay ids and the folder (default t1)
  --out DIR                where replays, logs and standings go (default ./cup-<id>)
";

/// What the organiser asked for.
struct Plan {
    listen: SocketAddr,
    seeds: u32,
    seats: u8,
    house: Vec<BotLevel>,
    tables: Option<usize>,
    start_when: Option<usize>,
    invites: Vec<String>,
    open: bool,
    per_owner: usize,
    parallel_cap: u32,
    beach: Beach,
    round: RoundLength,
    deadline: u32,
    fair: bool,
    forfeit_after: Duration,
    seed: u64,
    id: String,
    out: PathBuf,
}

fn plan(args: Vec<String>) -> Result<Plan, String> {
    let mut args = Args::new(args);
    let mut listen: SocketAddr = args
        .parsed("listen")?
        .unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], DEFAULT_PORT)));
    if let Some(port) = args.parsed::<u16>("port")? {
        listen.set_port(port);
    }
    let seeds = args.parsed::<u32>("seeds")?.unwrap_or(10).max(1);
    let seats = args.parsed::<u8>("seats")?.unwrap_or(4);
    let house = args
        .all("add")?
        .iter()
        .map(|s| cli::ai_seat(s))
        .collect::<Result<Vec<_>, _>>()?;
    let tables = args.parsed::<usize>("tables")?;
    let start_when = args.parsed::<usize>("start-when")?;
    let invites = args.all("invite")?;
    let open = args.switch("open-registration");
    let per_owner = args.parsed::<usize>("per-owner")?.unwrap_or(1).max(1);
    let parallel_cap = args.parsed::<u32>("parallel-cap")?.unwrap_or(8).max(1);
    let beach = match args.value("map")? {
        Some(m) => Beach::parse(&m)?,
        None => Beach::Map(crate::app::match_setup::MapChoice::Classic),
    };
    let round = match args.value("round")? {
        Some(r) => cli::round_length(&r)?,
        None => RoundLength::Standard,
    };
    let deadline = args
        .parsed::<u32>("deadline")?
        .unwrap_or(1000 / TICKS_PER_SECOND)
        .max(1);
    let fair = args.on_off("fair-cursor", false)?;
    let forfeit_after = Duration::from_secs(args.parsed::<u64>("forfeit-after")?.unwrap_or(60));
    let seed = args.parsed::<u64>("seed")?.unwrap_or_else(cli::fresh_seed);
    let id = args.value("id")?.unwrap_or_else(|| "t1".to_string());
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("--id is letters, digits, - and _".into());
    }
    let out = args
        .value("out")?
        .map_or_else(|| PathBuf::from(format!("cup-{id}")), PathBuf::from);
    let rest = args.finish()?;
    if !rest.is_empty() {
        return Err(format!("unexpected {:?}", rest[0]));
    }
    beach.check(seats)?;
    if open && !invites.is_empty() {
        return Err("--invite binds an owner to a key; --open-registration has no keys".into());
    }
    Ok(Plan {
        listen,
        seeds,
        seats,
        house,
        tables,
        start_when,
        invites,
        open,
        per_owner,
        parallel_cap,
        beach,
        round,
        deadline,
        fair,
        forfeit_after,
        seed,
        id,
        out,
    })
}

/// `pinch-points cup ...`.
pub fn run(args: Vec<String>) -> Result<(), String> {
    let Some((sub, rest)) = args.split_first() else {
        print!("{USAGE}");
        return Ok(());
    };
    match sub.as_str() {
        "serve" if rest.iter().any(|a| a == "--help" || a == "-h") => {
            print!("{USAGE}");
            Ok(())
        }
        "serve" => serve(plan(rest.to_vec())?),
        "invite" => Err(
            "an invite is drawn by the running server: type `invite NAME` at its console, \
             or start it with --invite NAME"
                .into(),
        ),
        "--help" | "-h" | "help" => {
            print!("{USAGE}");
            Ok(())
        }
        other => Err(format!("no cup command {other:?}; try `cup serve`")),
    }
}

/// One entrant: a bot that registered, or the house AI.
#[derive(Clone)]
struct Entrant {
    name: String,
    seat: Seat,
    owner: Option<String>,
    owner_declared: bool,
}

/// What the console reads: the listener's news and the organiser's lines.
enum Input {
    Event(Event),
    Line(String),
    Closed,
}

fn serve(plan: Plan) -> Result<(), String> {
    let shared_key = Key::draw();
    let admission = if plan.open {
        Admission::Open
    } else {
        Admission::Keys(vec![Invite {
            key: shared_key.clone(),
            uses: None,
            owner: None,
            slot: None,
        }])
    };
    let mut config = Config::new(admission);
    config.per_owner = Some(plan.per_owner);
    config.parallel_cap = plan.parallel_cap;
    let (listener, events) = Listener::bind(plan.listen, config)
        .map_err(|e| format!("cannot listen on {}: {e}", plan.listen))?;
    let host = reachable_host(listener.local_addr());
    let port = listener.local_addr().port();
    let slow = plan.deadline > 1000 / TICKS_PER_SECOND;
    println!(
        "Cup {}: {} beaches, {} seats, deadline {} ms{}, fair cursor {}",
        plan.id,
        plan.seeds,
        plan.seats,
        plan.deadline,
        if slow { " (a slow match)" } else { "" },
        if plan.fair { "on" } else { "off" },
    );
    let start_hint = match plan.start_when {
        Some(n) => format!("starts once {n} bots have registered"),
        None => "type `start` to begin".to_string(),
    };
    println!("Registration open ({start_hint}). Entrants join with:\n");
    if plan.open {
        println!("  {}", ConnString::new(&host, port, None));
    } else {
        println!("  {}", ConnString::new(&host, port, Some(shared_key)));
    }
    for name in &plan.invites {
        print_invite(&listener, &host, port, name);
    }
    let mut house: Vec<Entrant> = Vec::new();
    for (i, &level) in plan.house.iter().enumerate() {
        let name = if plan.house.len() == 1 {
            "house".to_string()
        } else {
            format!("house-{}", i + 1)
        };
        println!("  + {name} (ai:{})", level_token(level));
        house.push(Entrant {
            name,
            seat: Seat::Ai(level),
            owner: None,
            owner_declared: false,
        });
    }
    let _ = std::io::stdout().flush();

    // One channel for both voices, so the console waits on either.
    let (tx, rx) = mpsc::channel();
    let forward = tx.clone();
    std::thread::spawn(move || {
        for event in events {
            if forward.send(Input::Event(event)).is_err() {
                return;
            }
        }
    });
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            match line {
                Ok(line) => {
                    if tx.send(Input::Line(line)).is_err() {
                        return;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = tx.send(Input::Closed);
    });
    let (bots, console_open) = register(&plan, &listener, &rx, &host, port)?;
    listener.close_registration();
    let mut entrants: Vec<Entrant> = bots
        .iter()
        .filter_map(|&id| listener.bot(id))
        .map(|info| Entrant {
            name: info.name,
            seat: Seat::Bot(info.id),
            owner: info.owner,
            owner_declared: info.owner_declared,
        })
        .collect();
    entrants.extend(house);
    play_cup(&plan, &listener, &entrants)?;
    // The listener stays up so the entrants can fetch their replays: until
    // the organiser says so, or for a while on a server nobody is watching.
    if bots.is_empty() {
        return Ok(());
    }
    if console_open {
        println!("Bots can fetch their replays until the cup is closed: type `quit`, or Ctrl-C.");
    } else {
        println!(
            "Bots can fetch their replays for the next {} s.",
            UNATTENDED_GRACE.as_secs()
        );
    }
    let _ = std::io::stdout().flush();
    let closing = Instant::now() + UNATTENDED_GRACE;
    loop {
        let input = if console_open {
            rx.recv().ok()
        } else {
            rx.recv_timeout(closing.saturating_duration_since(Instant::now()))
                .ok()
        };
        match input {
            None | Some(Input::Closed) => break,
            Some(Input::Line(line)) if line.trim() == "quit" => break,
            Some(Input::Line(_) | Input::Event(_)) => {}
        }
    }
    Ok(())
}

fn level_token(level: BotLevel) -> &'static str {
    match level {
        BotLevel::Easy => "easy",
        BotLevel::Normal => "normal",
        BotLevel::Hard => "hard",
    }
}

fn print_invite(listener: &Listener, host: &str, port: u16, owner: &str) {
    let key = Key::draw();
    listener.invite(Invite {
        key: key.clone(),
        uses: None,
        owner: Some(owner.to_string()),
        slot: None,
    });
    println!(
        "  {}   (for {owner})",
        ConnString::new(host, port, Some(key))
    );
}

/// Take registrations until the organiser says start, or the field is
/// big enough to start on its own.
fn register(
    plan: &Plan,
    listener: &Listener,
    rx: &Receiver<Input>,
    host: &str,
    port: u16,
) -> Result<(Vec<BotId>, bool), String> {
    let mut bots: Vec<BotId> = Vec::new();
    let mut console_open = true;
    let enough = |bots: &[BotId]| bots.len() + plan.house.len() >= usize::from(plan.seats);
    loop {
        if let Some(n) = plan.start_when
            && bots.len() >= n
            && enough(&bots)
        {
            return Ok((bots, console_open));
        }
        let Ok(input) = rx.recv() else {
            return Err("the console closed".into());
        };
        match input {
            Input::Event(Event::Registered(id)) => {
                bots.push(id);
                let asked = Instant::now();
                while listener.rtt(id).is_none() && asked.elapsed() < Duration::from_millis(300) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                if let Some(info) = listener.bot(id) {
                    let rtt = listener.rtt(id).map_or("-".to_string(), |d| {
                        format!("{} ms", cli::ms(d.as_secs_f64() * 1000.0))
                    });
                    let owner = match (&info.owner, info.owner_declared) {
                        (Some(o), true) => format!("  owner {o} (declared)"),
                        (Some(o), false) => format!("  owner {o}"),
                        (None, _) => String::new(),
                    };
                    println!(
                        "  + {}  {}  round trip {rtt}  {} game{} at once{owner}",
                        info.name,
                        info.addr,
                        info.parallel,
                        if info.parallel == 1 { "" } else { "s" }
                    );
                }
            }
            Input::Event(Event::Dropped(id)) => {
                let name = listener.bot(id).map(|b| b.name).unwrap_or_default();
                println!("  - {name} disconnected (its token still holds its place)");
            }
            Input::Event(Event::Reconnected(id)) => {
                let name = listener.bot(id).map(|b| b.name).unwrap_or_default();
                println!("  ~ {name} is back");
            }
            Input::Event(Event::Refused(peer, why)) => println!("  refused {peer}: {why}"),
            Input::Line(line) => {
                let line = line.trim();
                match line
                    .split_once(' ')
                    .map_or((line, ""), |(c, r)| (c, r.trim()))
                {
                    ("start", _) => {
                        if enough(&bots) {
                            return Ok((bots, console_open));
                        }
                        println!(
                            "  not yet: {} entrant(s) cannot fill a {}-seat table",
                            bots.len() + plan.house.len(),
                            plan.seats
                        );
                    }
                    ("invite", name) if !name.is_empty() => {
                        if plan.open {
                            println!("  registration is open to anyone; there are no keys to bind");
                        } else {
                            print_invite(listener, host, port, name);
                        }
                    }
                    ("list", _) => {
                        for id in &bots {
                            if let Some(info) = listener.bot(*id) {
                                let state = if listener.connected(*id) {
                                    ""
                                } else {
                                    " (disconnected)"
                                };
                                println!("  {}{state}", info.name);
                            }
                        }
                    }
                    ("", _) => {}
                    _ => println!("  commands: start, invite NAME, list"),
                }
                let _ = std::io::stdout().flush();
            }
            Input::Closed => {
                console_open = false;
                if plan.start_when.is_none() {
                    return Err("the console closed before `start`; give --start-when N to start on its own".into());
                }
            }
        }
    }
}

/// A finished game, for the standings.
struct Played {
    fixture: Fixture,
    result: GameResult,
}

fn play_cup(plan: &Plan, listener: &Listener, entrants: &[Entrant]) -> Result<(), String> {
    let seats = usize::from(plan.seats);
    let beaches = plan.seeds as usize;
    let tables = plan
        .tables
        .unwrap_or_else(|| draw::tables_for(entrants.len(), seats, beaches));
    let owners: Vec<Option<String>> = entrants.iter().map(|e| e.owner.clone()).collect();
    let fixtures = draw::draw(&owners, seats, beaches, tables, plan.seed)?;
    std::fs::create_dir_all(&plan.out).map_err(|e| format!("{}: {e}", plan.out.display()))?;
    let schedule = schedule_text(plan, entrants, &fixtures, tables);
    let _ = crate::app::paths::write_atomic(&plan.out.join("schedule.txt"), &schedule);
    println!(
        "{} entrants, {tables} table{} a beach, {beaches} beaches, {seats} rotations each: {} games (draw seed {})",
        entrants.len(),
        if tables == 1 { "" } else { "s" },
        fixtures.len(),
        plan.seed
    );
    let total = fixtures.len();
    let started = Instant::now();
    let (done_tx, done_rx) = mpsc::channel::<Played>();
    let mut queue: Vec<(usize, Fixture)> = fixtures.into_iter().enumerate().collect();
    queue.reverse();
    let mut busy: Vec<u32> = vec![0; entrants.len()];
    let capacity: Vec<u32> = entrants
        .iter()
        .map(|e| match e.seat {
            Seat::Bot(id) => listener.bot(id).map_or(1, |b| b.parallel),
            Seat::Ai(_) => u32::MAX,
        })
        .collect();
    let mut running = 0usize;
    let mut finished: Vec<Played> = Vec::new();
    progress(started, 0, total);
    std::thread::scope(|scope| {
        while finished.len() < total {
            // Start everything whose entrants all have a game to spare, in
            // schedule order.
            let mut i = queue.len();
            while i > 0 {
                i -= 1;
                let fits = queue[i].1.chairs.iter().all(|&e| busy[e] < capacity[e]);
                if !fits || running >= MAX_RUNNING {
                    continue;
                }
                let (index, fixture) = queue.remove(i);
                for &e in &fixture.chairs {
                    busy[e] += 1;
                }
                running += 1;
                let tx = done_tx.clone();
                scope.spawn(move || {
                    let result = play_fixture(plan, listener, entrants, index, &fixture);
                    let _ = tx.send(Played { fixture, result });
                });
            }
            let Ok(played) = done_rx.recv() else {
                break;
            };
            running -= 1;
            for &e in &played.fixture.chairs {
                busy[e] -= 1;
            }
            finished.push(played);
            progress(started, finished.len(), total);
        }
    });
    println!();
    let table = standings(plan, listener, entrants, &finished);
    print!("\n{table}");
    let _ = crate::app::paths::write_atomic(&plan.out.join("standings.txt"), &table);
    println!("\nReplays and logs: {}/", plan.out.display());
    Ok(())
}

/// How long a cup nobody is typing at keeps serving replays once it ends.
const UNATTENDED_GRACE: Duration = Duration::from_secs(60);

/// How many games run at once, whatever the capacities say: a cup of
/// house bots alone would otherwise start all of them together.
const MAX_RUNNING: usize = 64;

fn progress(started: Instant, done: usize, total: usize) {
    let secs = started.elapsed().as_secs();
    let width = 24;
    let filled = (done * width).checked_div(total).unwrap_or(width);
    print!(
        "\r[{:02}:{:02}:{:02}] {}{} {done}/{total}",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60,
        "=".repeat(filled),
        " ".repeat(width - filled)
    );
    let _ = std::io::stdout().flush();
}

fn play_fixture(
    plan: &Plan,
    listener: &Listener,
    entrants: &[Entrant],
    index: usize,
    fixture: &Fixture,
) -> GameResult {
    let number = index as u32 + 1;
    let seed = plan.seed + fixture.beach as u64;
    let names: Vec<String> = fixture
        .chairs
        .iter()
        .map(|&e| entrants[e].name.clone())
        .collect();
    let spec = GameSpec {
        game: number,
        replay_id: format!("{}-g{number}", plan.id),
        board: plan.beach.board(seed, plan.seats, plan.round),
        seats: fixture.chairs.iter().map(|&e| entrants[e].seat).collect(),
        names,
        clock: Clock {
            live: false,
            deadline_ms: plan.deadline,
            input_delay: 0,
        },
        fair_cursor: plan.fair,
        forfeit_after: plan.forfeit_after,
        ready_within: Duration::from_secs(10),
    };
    let mut log: Box<dyn Write> =
        match std::fs::File::create(plan.out.join(format!("g{number}.log"))) {
            Ok(file) => Box::new(std::io::BufWriter::new(file)),
            Err(_) => Box::new(std::io::sink()),
        };
    let mut console = |line: String| println!("\n{line}");
    let mut sinks = Sinks {
        log: &mut *log,
        trace: None,
        feed: None,
        console: &mut console,
        echo_notes: false,
    };
    let result = game::play(listener, spec, &mut sinks);
    let _ = log.flush();
    let _ = crate::app::paths::write_atomic(
        &plan.out.join(format!("g{number}.replay")),
        result.replay.to_text(),
    );
    result
}

fn schedule_text(plan: &Plan, entrants: &[Entrant], fixtures: &[Fixture], tables: usize) -> String {
    let mut out = format!(
        "cup {}: {} entrants, {} seats, {} beaches from seed {}, {tables} table(s) a beach, draw seed {}\n\n",
        plan.id,
        entrants.len(),
        plan.seats,
        plan.seeds,
        plan.seed,
        plan.seed
    );
    for (i, f) in fixtures.iter().enumerate() {
        let names: Vec<&str> = f
            .chairs
            .iter()
            .map(|&e| entrants[e].name.as_str())
            .collect();
        out.push_str(&format!(
            "g{}  beach {} (seed {})  {}\n",
            i + 1,
            f.beach + 1,
            plan.seed + f.beach as u64,
            names.join(" | ")
        ));
    }
    out
}

/// One entrant's numbers over the cup.
#[derive(Default)]
struct Tally {
    games: u32,
    points: f64,
    firsts: u32,
    score: f64,
    raided: u32,
    forfeits: u32,
    late: u32,
    sent: u32,
    reply_ms: Vec<f64>,
}

fn standings(plan: &Plan, listener: &Listener, entrants: &[Entrant], played: &[Played]) -> String {
    let mut tallies: Vec<Tally> = entrants.iter().map(|_| Tally::default()).collect();
    for game in played {
        let places: Vec<u32> = game.result.seats.iter().map(|s| s.placing).collect();
        let points = draw::points(&places);
        for (chair, &e) in game.fixture.chairs.iter().enumerate() {
            let seat: &SeatResult = &game.result.seats[chair];
            let t = &mut tallies[e];
            t.games += 1;
            t.points += points[chair];
            t.firsts += u32::from(seat.placing == 1);
            t.score += f64::from(seat.score);
            t.raided += seat.raided;
            t.forfeits += u32::from(seat.forfeit);
            t.late += seat.late;
            t.sent += seat.sent;
            t.reply_ms.extend_from_slice(&seat.reply_ms);
        }
    }
    let avg = |t: &Tally, v: f64| v / f64::from(t.games.max(1));
    let mut order: Vec<usize> = (0..entrants.len()).collect();
    order.sort_by(|&a, &b| {
        let (ta, tb) = (&tallies[a], &tallies[b]);
        avg(tb, tb.points)
            .total_cmp(&avg(ta, ta.points))
            .then(avg(tb, tb.score).total_cmp(&avg(ta, ta.score)))
    });
    let mut rows = vec![
        [
            "#",
            "Bot",
            "Owner",
            "Points",
            "Firsts",
            "Avg score",
            "Raided",
            "Forfeits",
            "Late",
            "Reply p50/p99",
            "RTT",
        ]
        .map(String::from)
        .to_vec(),
    ];
    let mut declared = false;
    for (rank, &e) in order.iter().enumerate() {
        let t = &tallies[e];
        let entrant = &entrants[e];
        let owner = match (&entrant.owner, entrant.owner_declared) {
            (Some(o), true) => {
                declared = true;
                format!("{o}*")
            }
            (Some(o), false) => o.clone(),
            (None, _) => "-".into(),
        };
        let (late, replies, rtt) = match entrant.seat {
            Seat::Bot(id) => (
                format!(
                    "{:.1}%",
                    100.0 * f64::from(t.late) / f64::from(t.sent.max(1))
                ),
                match (
                    cli::percentile(&t.reply_ms, 50.0),
                    cli::percentile(&t.reply_ms, 99.0),
                ) {
                    (Some(a), Some(b)) => format!("{} / {} ms", cli::ms(a), cli::ms(b)),
                    _ => "-".into(),
                },
                listener.rtt(id).map_or("-".into(), |d| {
                    format!("{} ms", cli::ms(d.as_secs_f64() * 1000.0))
                }),
            ),
            Seat::Ai(_) => ("-".into(), "-".into(), "-".into()),
        };
        rows.push(vec![
            (rank + 1).to_string(),
            entrant.name.clone(),
            owner,
            format!("{:.2}", avg(t, t.points)),
            t.firsts.to_string(),
            format!("{:.1}", avg(t, t.score)),
            format!("{:.1}", avg(t, f64::from(t.raided))),
            t.forfeits.to_string(),
            late,
            replies,
            rtt,
        ]);
    }
    let mut out = format!(
        "Cup {}: {} games, deadline {} ms{}, fair cursor {}, {}\n\n",
        plan.id,
        played.len(),
        plan.deadline,
        if plan.deadline > 1000 / TICKS_PER_SECOND {
            " (a slow match)"
        } else {
            ""
        },
        if plan.fair { "on" } else { "off" },
        cli::round_name(plan.round),
    );
    out.push_str(&cli::table(&rows));
    if declared {
        out.push_str("\n  * owner as the bot declared it, not checked: trust among friends\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn a_cup_is_read_off_its_flags() {
        let p = plan(args(
            "--seeds 20 --seats 4 --add ai:hard --start-when 3 --seed 5",
        ))
        .expect("plan");
        assert_eq!(
            (p.seeds, p.seats, p.start_when, p.seed),
            (20, 4, Some(3), 5)
        );
        assert_eq!(p.house, vec![BotLevel::Hard]);
        assert_eq!(p.per_owner, 1);
        assert_eq!(p.listen.to_string(), "0.0.0.0:47710");
        assert_eq!(p.out, PathBuf::from("cup-t1"));
        assert!(plan(args("--seats 9")).is_err());
        assert!(plan(args("--open-registration --invite ana")).is_err());
        assert!(plan(args("--id ../x")).is_err());
    }

    #[test]
    fn a_cup_of_house_bots_plays_and_ranks_by_points() {
        let dir = std::env::temp_dir().join(format!("pinch-cup-test-{}", std::process::id()));
        let p = Plan {
            listen: SocketAddr::from(([127, 0, 0, 1], 0)),
            seeds: 1,
            seats: 2,
            house: vec![BotLevel::Easy, BotLevel::Hard],
            tables: None,
            start_when: None,
            invites: vec![],
            open: true,
            per_owner: 1,
            parallel_cap: 8,
            beach: Beach::Map(crate::app::match_setup::MapChoice::Classic),
            round: RoundLength::Short,
            deadline: 33,
            fair: false,
            forfeit_after: Duration::from_secs(1),
            seed: 1,
            id: "x".into(),
            out: dir.clone(),
        };
        let (listener, _) = Listener::bind(p.listen, Config::new(Admission::Open)).expect("bind");
        let entrants = vec![
            Entrant {
                name: "easy".into(),
                seat: Seat::Ai(BotLevel::Easy),
                owner: None,
                owner_declared: false,
            },
            Entrant {
                name: "hard".into(),
                seat: Seat::Ai(BotLevel::Hard),
                owner: None,
                owner_declared: false,
            },
        ];
        play_cup(&p, &listener, &entrants).expect("plays");
        let standings = std::fs::read_to_string(dir.join("standings.txt")).expect("standings");
        assert!(standings.contains("2 games"), "{standings}");
        assert!(dir.join("g1.replay").exists() && dir.join("g2.replay").exists());
        let schedule = std::fs::read_to_string(dir.join("schedule.txt")).expect("schedule");
        assert!(schedule.contains("g1  beach 1") && schedule.contains("g2  beach 1"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
