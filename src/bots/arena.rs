//! `pinch-points arena`: play your bot against the game's AI, or against
//! itself, from a terminal.
//!
//! The arena opens the seats it was given, waits for bots to connect to
//! the rest, plays, and prints the standings with where the replay and the
//! decision log went. Without `--watch` no window opens and nothing of
//! Bevy starts: it runs as fast as the bots answer.

use super::cli::{self, Args, Beach};
use super::connstr::{ConnString, DEFAULT_PORT, Key, reachable_host};
use super::game::{self, Feed, GameResult, GameSpec, Seat, Sinks};
use super::listener::{Admission, BotId, Config, Event, Invite, Listener};
use super::protocol::Clock;
use crate::app::match_setup::RoundLength;
use crate::sim::{BotLevel, TICKS_PER_SECOND};
use std::io::Write;
use std::net::SocketAddr;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

pub const USAGE: &str = "\
pinch-points arena [flags]

Play bots against the game's AI or each other. Open seats wait for a bot
to connect; the arena prints the connection string to start it with.

  --seat ai:<level>        a seat for the game's AI (easy, normal, hard); repeatable
  --open N                 seats left for bots to connect to (default 0)
  --listen ADDR            where to wait (default 127.0.0.1:47710)
  --port P                 the port to wait on
  --key KEY                a fixed join key instead of a fresh one
  --map NAME|FILE          classic, generated, small, large, xl, ocean, or a level file
  --seats N                table size; seats beyond those given play ai:normal
  --seed S                 the first beach's seed (default: random, printed)
  --round short|standard|long
  --games N                play N seeds and print averages (default 1)
  --deadline MS            the reply deadline (default 33, one tick)
  --trace FILE             write every tick message and the actions after it
  --fair-cursor on|off     walk every non-human seat's cursor at a person's pace
  --watch                  draw the match in a window as it plays, in real time
";

/// What the arena was asked for.
struct Plan {
    ai: Vec<BotLevel>,
    open: u32,
    listen: SocketAddr,
    key: Option<Key>,
    beach: Beach,
    seats: u8,
    seed: u64,
    round: RoundLength,
    games: u32,
    deadline: u32,
    trace: Option<std::path::PathBuf>,
    fair: bool,
    watch: bool,
}

fn plan(args: Vec<String>) -> Result<Plan, String> {
    let mut args = Args::new(args);
    let ai = args
        .all("seat")?
        .iter()
        .map(|s| cli::ai_seat(s))
        .collect::<Result<Vec<_>, _>>()?;
    let open = args.parsed::<u32>("open")?.unwrap_or(0);
    let mut listen: SocketAddr = args
        .parsed("listen")?
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)));
    if let Some(port) = args.parsed::<u16>("port")? {
        listen.set_port(port);
    }
    let key =
        match args.value("key")? {
            Some(k) => Some(Key::parse(&k).ok_or_else(|| {
                format!("--key: {k:?} is not a join key (eight base32 characters)")
            })?),
            None => None,
        };
    let beach = match args.value("map")? {
        Some(m) => Beach::parse(&m)?,
        None => Beach::Map(crate::app::match_setup::MapChoice::Classic),
    };
    let given = ai.len() as u32 + open;
    let seats = args.parsed::<u8>("seats")?.map_or(given, u32::from);
    if seats < given {
        return Err(format!(
            "--seats {seats} is fewer than the {given} seats given"
        ));
    }
    let seed = args.parsed::<u64>("seed")?.unwrap_or_else(cli::fresh_seed);
    let round = match args.value("round")? {
        Some(r) => cli::round_length(&r)?,
        None => RoundLength::Standard,
    };
    let games = args.parsed::<u32>("games")?.unwrap_or(1).max(1);
    let deadline = args
        .parsed::<u32>("deadline")?
        .unwrap_or(1000 / TICKS_PER_SECOND);
    let deadline = if deadline == 0 { 1 } else { deadline };
    let trace = args.value("trace")?.map(std::path::PathBuf::from);
    let fair = args.on_off("fair-cursor", false)?;
    let watch = args.switch("watch");
    let rest = args.finish()?;
    if !rest.is_empty() {
        return Err(format!("unexpected {:?}", rest[0]));
    }
    if open == 0 && ai.is_empty() {
        return Err(
            "nothing to play: give --open N for bots and --seat ai:<level> for the AI".into(),
        );
    }
    if watch && deadline != 1000 / TICKS_PER_SECOND {
        return Err(
            "--watch plays on the live clock, and a live match has one tick: drop --deadline"
                .into(),
        );
    }
    let seats = u8::try_from(seats).map_err(|_| format!("{seats} seats is too many"))?;
    beach.check(seats)?;
    Ok(Plan {
        ai,
        open,
        listen,
        key,
        beach,
        seats,
        seed,
        round,
        games,
        deadline,
        trace,
        fair,
        watch,
    })
}

/// `pinch-points arena ...`. Returns the process's exit status.
pub fn run(args: Vec<String>) -> Result<(), String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return Ok(());
    }
    let plan = plan(args)?;
    if plan.watch {
        // The window has the main thread; the arena plays beside it and the
        // window draws each tick as it lands.
        let (feed, rx) = std::sync::mpsc::channel();
        let arena = std::thread::Builder::new()
            .name("arena".into())
            .spawn(move || {
                if let Err(e) = play(plan, Some(feed)) {
                    eprintln!("arena: {e}");
                }
            })
            .map_err(|e| e.to_string())?;
        crate::app::watch_live(rx);
        let _ = arena.join();
        return Ok(());
    }
    play(plan, None)
}

fn play(plan: Plan, feed: Option<Sender<Feed>>) -> Result<(), String> {
    let Plan {
        ai,
        open,
        listen,
        key,
        beach,
        seats,
        seed,
        round,
        games,
        deadline,
        trace,
        fair,
        watch,
    } = plan;
    // Open seats come first, then the AI, then any seats --seats added.
    let invites: Vec<Invite> = match &key {
        Some(key) => vec![Invite {
            key: key.clone(),
            uses: Some(open),
            owner: None,
            slot: None,
        }],
        None => (0..open)
            .map(|slot| Invite {
                key: Key::draw(),
                uses: Some(1),
                owner: None,
                slot: Some(slot),
            })
            .collect(),
    };
    let mut bots: Vec<Option<BotId>> = vec![None; open as usize];
    let listener = if open > 0 {
        let mut config = Config::new(Admission::Keys(invites.clone()));
        config.max_bots = Some(open as usize);
        let (listener, events) = Listener::bind(listen, config)
            .map_err(|e| format!("cannot listen on {listen}: {e}"))?;
        let host = reachable_host(listener.local_addr());
        let port = listener.local_addr().port();
        println!(
            "Waiting for {open} bot{}. Start {} with:\n",
            if open == 1 { "" } else { "s" },
            if open == 1 { "it" } else { "each" }
        );
        let strings: Vec<String> = match &key {
            Some(key) => vec![ConnString::new(&host, port, Some(key.clone())).to_string()],
            None => invites
                .iter()
                .map(|i| ConnString::new(&host, port, Some(i.key.clone())).to_string())
                .collect(),
        };
        for s in &strings {
            println!("  {s}");
        }
        println!();
        let _ = std::io::stdout().flush();
        let mut joined = 0u32;
        while joined < open {
            match events.recv() {
                Ok(Event::Registered(id)) => {
                    let info = listener.bot(id).ok_or("a bot vanished")?;
                    let slot = info
                        .slot
                        .map(|s| s as usize)
                        .or_else(|| bots.iter().position(Option::is_none))
                        .unwrap_or(0);
                    if let Some(chair) = bots.get_mut(slot) {
                        *chair = Some(id);
                    }
                    joined += 1;
                    // Give the registration's pings a moment to come back.
                    let asked = Instant::now();
                    while listener.rtt(id).is_none() && asked.elapsed() < Duration::from_millis(300)
                    {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    let version = if info.version.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", info.version)
                    };
                    println!(
                        "  {}{version} connected (round trip {})",
                        info.name,
                        listener.rtt(id).map_or("unknown".to_string(), |d| format!(
                            "{} ms",
                            cli::ms(d.as_secs_f64() * 1000.0)
                        ))
                    );
                }
                Ok(Event::Refused(peer, why)) => println!("  refused {peer}: {why}"),
                Ok(Event::Dropped(id)) => {
                    let name = listener.bot(id).map(|b| b.name).unwrap_or_default();
                    println!("  {name} disconnected before the start; waiting for it to come back");
                }
                Ok(Event::Reconnected(_)) => {}
                Err(_) => return Err("the listener stopped".into()),
            }
        }
        Some(listener)
    } else {
        None
    };
    let mut table: Vec<Seat> = bots.iter().map(|b| Seat::Bot(b.unwrap_or(0))).collect();
    table.extend(ai.iter().map(|&level| Seat::Ai(level)));
    while table.len() < usize::from(seats) {
        table.push(Seat::Ai(BotLevel::Normal));
    }
    let names: Vec<String> = table
        .iter()
        .map(|seat| match seat {
            Seat::Ai(level) => game::ai_name(*level).to_string(),
            Seat::Bot(id) => listener
                .as_ref()
                .and_then(|l| l.bot(*id))
                .map(|b| b.name)
                .unwrap_or_default(),
        })
        .collect();
    let clock = Clock {
        live: watch,
        deadline_ms: deadline,
        input_delay: 0,
    };
    let slow = deadline > 1000 / TICKS_PER_SECOND;
    println!(
        "Beach: {}, seed {seed}{}, {}, fair cursor {}, deadline {deadline} ms{}",
        beach.name(),
        if games > 1 {
            format!(" to {}", seed + u64::from(games) - 1)
        } else {
            String::new()
        },
        cli::round_name(round),
        if fair { "on" } else { "off" },
        if slow { " (a slow match)" } else { "" },
    );
    let dir = crate::app::paths::data_dir().join("arena");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    let stem = cli::stamp();
    let log_path = cli::free_path(&dir, &stem, "log");
    let mut log = std::io::BufWriter::new(
        std::fs::File::create(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?,
    );
    let mut trace_file = match &trace {
        Some(path) => Some(std::io::BufWriter::new(
            std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?,
        )),
        None => None,
    };
    let _ = writeln!(log, "arena {stem}: {}", names.join(", "));
    let mut results: Vec<GameResult> = Vec::new();
    let mut replays = Vec::new();
    let listener = match listener {
        Some(l) => l,
        // An all-AI arena still runs its games through a listener nobody
        // can reach: the seat layer is the same either way.
        None => {
            let (l, _) = Listener::bind(
                SocketAddr::from(([127, 0, 0, 1], 0)),
                Config::new(Admission::Keys(vec![])),
            )
            .map_err(|e| e.to_string())?;
            l
        }
    };
    for g in 0..games {
        let seed = seed + u64::from(g);
        if games > 1 {
            print!("Game {}/{games}, seed {seed}... ", g + 1);
        } else {
            print!("Playing...{}", if watch { "\n" } else { " " });
        }
        let _ = std::io::stdout().flush();
        let spec = GameSpec {
            game: g + 1,
            replay_id: format!("a-g{}", g + 1),
            board: beach.board(seed, seats, round),
            seats: table.clone(),
            names: names.clone(),
            clock,
            fair_cursor: fair,
            forfeit_after: Duration::from_secs(60),
            ready_within: Duration::from_secs(10),
        };
        let mut console = |line: String| println!("{line}");
        let mut sinks = Sinks {
            log: &mut log,
            trace: trace_file.as_mut().map(|t| t as &mut dyn Write),
            feed: feed.clone(),
            console: &mut console,
            echo_notes: watch,
        };
        let result = game::play(&listener, spec, &mut sinks);
        println!(
            "done in {:.1} s ({} ticks)",
            result.elapsed.as_secs_f64(),
            result.ticks
        );
        let suffix = if games > 1 {
            format!("{stem}-g{}", g + 1)
        } else {
            stem.clone()
        };
        let path = cli::free_path(&dir, &suffix, "replay");
        match crate::app::paths::write_atomic(&path, result.replay.to_text()) {
            Ok(()) => replays.push(path),
            Err(e) => eprintln!("could not save the replay: {e}"),
        }
        results.push(result);
    }
    let _ = log.flush();
    if let Some(t) = trace_file.as_mut() {
        let _ = t.flush();
    }
    println!();
    print!("{}", standings(&results, &table, &names, &listener));
    println!();
    match replays.as_slice() {
        [one] => println!("Replay:  {}", one.display()),
        [] => {}
        many => println!("Replays: {} ... ({} files)", many[0].display(), many.len()),
    }
    println!("Log:     {}", log_path.display());
    if let Some(path) = trace {
        println!("Trace:   {}", path.display());
    }
    Ok(())
}

/// The standings: one game's numbers, or the averages over several.
fn standings(
    results: &[GameResult],
    table: &[Seat],
    names: &[String],
    listener: &Listener,
) -> String {
    let games = results.len().max(1) as f64;
    let n = table.len();
    let sum = |f: &dyn Fn(&game::SeatResult) -> f64, seat: usize| {
        results.iter().map(|r| f(&r.seats[seat])).sum::<f64>()
    };
    let mut order: Vec<usize> = (0..n).collect();
    let avg_score = |seat: usize| sum(&|s| f64::from(s.score), seat) / games;
    let avg_place = |seat: usize| sum(&|s| f64::from(s.placing), seat) / games;
    order.sort_by(|&a, &b| {
        avg_place(a)
            .total_cmp(&avg_place(b))
            .then(avg_score(b).total_cmp(&avg_score(a)))
    });
    let one = results.len() == 1;
    let mut rows = vec![
        [
            "#",
            "Seat",
            "Player",
            "Kind",
            if one { "Score" } else { "Avg score" },
            "Banked",
            "Raided",
            "Rejected",
            "Late",
            "Reply p50/p99",
            "RTT",
        ]
        .map(String::from)
        .to_vec(),
    ];
    if !one {
        rows[0].insert(4, "Firsts".into());
    }
    for (rank, &seat) in order.iter().enumerate() {
        let fmt = |v: f64| {
            if one {
                format!("{v:.0}")
            } else {
                format!("{v:.1}")
            }
        };
        let times: Vec<f64> = results
            .iter()
            .flat_map(|r| r.seats[seat].reply_ms.iter().copied())
            .collect();
        let replies = match (cli::percentile(&times, 50.0), cli::percentile(&times, 99.0)) {
            (Some(p50), Some(p99)) => format!("{} / {} ms", cli::ms(p50), cli::ms(p99)),
            _ => "-".into(),
        };
        let rtt = match table[seat] {
            Seat::Bot(id) => listener.rtt(id).map_or("-".into(), |d| {
                format!("{} ms", cli::ms(d.as_secs_f64() * 1000.0))
            }),
            Seat::Ai(_) => "-".into(),
        };
        let late = sum(&|s| f64::from(s.late), seat);
        let mut row = vec![
            (rank + 1).to_string(),
            format!("P{}", seat + 1),
            names[seat].clone(),
            table[seat].kind().token().to_string(),
            fmt(avg_score(seat)),
            fmt(sum(&|s| f64::from(s.banked), seat) / games),
            fmt(sum(&|s| f64::from(s.raided), seat) / games),
            fmt(sum(&|s| f64::from(s.rejected), seat) / games),
            if matches!(table[seat], Seat::Ai(_)) {
                "-".into()
            } else {
                format!("{late:.0}")
            },
            replies,
            rtt,
        ];
        if !one {
            let firsts = results
                .iter()
                .filter(|r| r.seats[seat].placing == 1)
                .count();
            row.insert(4, firsts.to_string());
        }
        rows.push(row);
    }
    cli::table(&rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn an_arena_is_read_off_its_flags() {
        let p = plan(args("--seat ai:easy --open 1 --seed 7 --games 3")).expect("plan");
        assert_eq!(p.ai, vec![BotLevel::Easy]);
        assert_eq!((p.open, p.seats, p.seed, p.games), (1, 2, 7, 3));
        assert_eq!(p.listen.to_string(), "127.0.0.1:47710");
        assert_eq!(p.deadline, 33);
        assert!(!p.fair && !p.watch);
    }

    #[test]
    fn an_arena_that_cannot_be_played_says_why() {
        assert!(plan(args("")).is_err());
        assert!(
            plan(args(
                "--seat ai:easy --seat ai:easy --seat ai:easy --seat ai:easy --seat ai:easy"
            ))
            .is_err()
        );
        assert!(plan(args("--seat ai:silly --open 1")).is_err());
        assert!(plan(args("--open 1 --seats 1 --seat ai:easy")).is_err());
        assert!(plan(args("--open 1 --seat ai:easy --watch --deadline 200")).is_err());
        assert!(plan(args("--open 1 --seat ai:easy --frobnicate")).is_err());
    }
}
