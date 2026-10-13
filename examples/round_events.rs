//! What the tide did to a recorded round: every roulette spin, when it
//! landed, and what was on the beach when it did.
//!
//! Run: `cargo run --example round_events -- [path]`, defaulting to the
//! last round the game kept (under `$XDG_DATA_HOME`, as the game reads
//! it). A replay is the round's inputs and the sim is
//! deterministic, so re-running one here reproduces the beach that was
//! played.

use pinch_points::app::sim_events::{SimEvent, Watch, diff};
use pinch_points::sim::{Replay, TICKS_PER_SECOND, TideEvent};

fn name(event: TideEvent) -> &'static str {
    match event {
        TideEvent::CrabMania => "Crab Mania",
        TideEvent::GullMania => "Gull Mania",
        TideEvent::Monopoly => "Crab Monopoly",
        TideEvent::GullAttack => "Gull Attack",
        TideEvent::SpeedUp => "Speed Up",
        TideEvent::SlowDown => "Slow Down",
        TideEvent::FreshSand => "Fresh Sand",
        TideEvent::CastleSwap => "Castle Swap",
        TideEvent::RightClaws => "Right Claws",
    }
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        pinch_points::app::replay_path()
            .to_string_lossy()
            .into_owned()
    });
    let text = std::fs::read_to_string(&path).expect("replay file");
    let replay = Replay::parse(&text).expect("replay parses");
    // `Replay::playback` runs the whole round and hands back the *final*
    // board; the round has to be re-run tick by tick from the level to be
    // watched happening.
    let mut board = replay.level.board();

    println!("{path}");
    println!(
        "{} ticks recorded, round of {:?}, {} seats",
        replay.inputs.len(),
        board.rules().round_length,
        board.castle_owners().max().map_or(0, |top| top + 1)
    );

    let mut last_seen: Option<u64> = None;
    let mut fired: Vec<(u64, TideEvent)> = Vec::new();
    // What the right-hand feed reports, read off the game's own event
    // stream rather than rebuilt from scores and headcounts: a gull walking
    // on, a castle raided, a castle crossing a tier. A score diff counted
    // a left claw banked under Right Claws as a raid, and a headcount
    // missed every gull that came in as another left. The banks the feed
    // also lists (gold, lures) are a couple of percent of crabs and cannot
    // make a wave; these three can.
    let mut arrivals = 0usize;
    let mut raids = 0usize;
    let mut tiers = 0usize;
    let mut buckets: Vec<(usize, usize, usize)> = Vec::new();
    let bucket_ticks = 150u64; // five seconds
    let mut watch = Watch::default();
    let _ = diff(&board, &mut watch);
    let mut here = (0usize, 0usize, 0usize);
    for (tick, actions) in replay.inputs.iter().enumerate() {
        board.tick(actions);
        if let Some((event, at)) = board.last_event()
            && last_seen != Some(at)
        {
            last_seen = Some(at);
            fired.push((at, event));
        }
        for event in diff(&board, &mut watch) {
            if matches!(event, SimEvent::GullArrived) {
                here.0 += 1;
                arrivals += 1;
            } else if matches!(event, SimEvent::CastleRaided { .. }) {
                here.1 += 1;
                raids += 1;
            } else if matches!(event, SimEvent::TierUp { .. }) {
                here.2 += 1;
                tiers += 1;
            }
        }
        if (tick as u64 + 1).is_multiple_of(bucket_ticks) {
            buckets.push(here);
            here = (0, 0, 0);
        }
    }
    // The round rarely divides evenly into buckets, and the remainder is
    // the *end* of it - the surge, the busiest stretch there is. Dropping
    // it hid exactly the part worth looking at.
    if here != (0, 0, 0) || !replay.inputs.len().is_multiple_of(bucket_ticks as usize) {
        buckets.push(here);
    }

    let minutes = replay.inputs.len() as f32 / TICKS_PER_SECOND as f32 / 60.0;
    println!(
        "\ntide events: {} ({:.1} a minute)",
        fired.len(),
        fired.len() as f32 / minutes.max(0.001)
    );
    let window = u64::from(pinch_points::sim::EVENT_TICKS);
    let mut stacked = 0usize;
    let mut prev = 0u64;
    for (at, event) in &fired {
        // An event's own effect runs for `EVENT_TICKS`. One that lands
        // inside the last one's window is playing on top of it: two
        // manias at once, or a banner cut off by the next banner.
        let on_top = prev > 0 && at - prev < window;
        if on_top {
            stacked += 1;
        }
        println!(
            "  {at:5}  +{:<5} {:<13}{}",
            at - prev,
            name(*event),
            if on_top {
                "  on top of the last one"
            } else {
                ""
            }
        );
        prev = *at;
    }
    println!("{stacked} of them landed inside the previous one's ten seconds");
    println!("\nfeed lines by five-second bucket (gulls / raids / tier-ups)");
    println!("   from     bar                                  g  r  t");
    for (i, (g, r, t)) in buckets.iter().enumerate() {
        let total = g + r + t;
        let at = i as u64 * bucket_ticks / u64::from(TICKS_PER_SECOND);
        println!(
            "  {:3}s     {:<36} {g:2} {r:2} {t:2}",
            at,
            "#".repeat(total.min(36))
        );
    }
    println!("\ntotals: {arrivals} gull arrivals, {raids} raids, {tiers} tier-ups");
    let busiest = buckets
        .iter()
        .enumerate()
        .max_by_key(|(_, (g, r, t))| g + r + t);
    if let Some((i, (g, r, t))) = busiest {
        println!(
            "busiest five seconds: {}s, {} lines ({g} gulls, {r} raids, {t} tier-ups)",
            i as u64 * bucket_ticks / u64::from(TICKS_PER_SECOND),
            g + r + t
        );
    }
}
