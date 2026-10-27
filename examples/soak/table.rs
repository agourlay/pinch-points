//! A soak table: whole online rounds played at virtual time over real
//! loopback sockets, with a counting, lossy proxy in front of every spoke.
//!
//! One step is one fixed tick on every machine, so nothing here sleeps:
//! a full 5400-tick round of six players and three watchers runs in
//! seconds. Every peer is the game's own `OnlineSession` driven through
//! `pump`, so what is measured is what ships; only the board each peer
//! ticks is a plain generated arena rather than the shell's.
//!
//! Shared by `examples/net_soak.rs`, which reads a scenario off the
//! environment and prints the numbers, and the integration suite, which
//! plays a short one and holds the table to bounds.

#![allow(dead_code)]

use pinch_points::app::net::OnlineSession;
use pinch_points::sim::{
    Board, Direction, Lockstep, MAX_PLAYERS, PlayerAction, TICKS_PER_SECOND, bot_action,
    delay_for_round_trip,
};
use pinch_points::transport::{IN_SEAT_ORDER, MatchTerms, NetMsg, UdpTransport, steer_ping_clock};
use std::collections::BTreeMap;
use std::net::{SocketAddr, UdpSocket};

/// What to play, and what the network does to it. Delays are in steps
/// (ticks), probabilities per datagram.
#[derive(Clone, Debug)]
pub struct Scenario {
    pub games: usize,
    pub players: u8,
    /// The AI seats after the players, which every peer plays itself
    /// off the agreed frame, as the game does.
    pub ai: u8,
    /// The seat each place sits in (the host, the joiners in order, then
    /// the AI), as a host's partner picks deal them: a seat order other
    /// than [`IN_SEAT_ORDER`] puts the lockstep over seats that are not
    /// 0..n, and the AI where the order says.
    pub order: [u8; MAX_PLAYERS],
    /// How the table is scored ([`MatchTerms::teams`]), which rides in the
    /// terms of every deal.
    pub teams: u8,
    pub watchers: usize,
    pub steps: u64,
    /// Independent loss on every leg.
    pub loss: f64,
    /// Mean steps between loss bursts on one leg (0: no bursts), and how
    /// many steps a burst drops everything for.
    pub burst_every: u64,
    pub burst_len: u64,
    /// Fixed one-way delay on every leg, plus up to `jitter` more.
    pub latency: u64,
    pub jitter: u64,
    /// Joiner 1 starts this many steps after everybody else.
    pub skew: u64,
    /// Joiner 1 stops ticking for `hitch` steps from step `hitch_at`.
    pub hitch_at: u64,
    pub hitch: u64,
    /// From step `pause_at`, the first `pause_storm` joiners call a pause
    /// a few steps apart, and joiner 1 lifts it `pause_len` steps later.
    pub pause_at: u64,
    pub pause_storm: usize,
    pub pause_len: u64,
    /// The input delay every peer plays with, or `None` for the one a
    /// host would pick for this network ([`Scenario::delay`]).
    pub delay: Option<u32>,
    pub seed: u64,
    /// How many frames the first round lasts when a second follows it
    /// (0: one round, `steps` long). The host shows its results card for
    /// `card` steps, deals the next round, and every peer stands in the
    /// interlude for `beat` steps before it plays it, as the game does.
    pub round_frames: u32,
    pub card: u64,
    pub beat: u64,
    /// The line-up and the count every peer stands through after the
    /// interlude, in steps, before it plays round 2.
    pub opening: u64,
    /// Whether a peer standing in the interlude, line-up and count keeps
    /// its session talking (`OnlineSession::before_the_round`), as the
    /// game does; `false` is the old silence, which lost a seat whose
    /// deal's `Start` went astray to the AI.
    pub listen: bool,
    /// The chance each input datagram on any leg, sent from a little
    /// before the host's round ends until it deals the next, is held back
    /// and let go only after the deal: the last round's stragglers, landing
    /// on peers already playing the next one.
    pub straggle: f64,
    /// The deal's `Start` to joiner 1 is lost, the first copy only.
    pub lose_deal_start: bool,
    /// The last watcher stops ticking, and hears nothing, for this many
    /// steps before round 1 ends: that far behind the table when the
    /// host's results card comes up.
    pub watcher_lag: u64,
}

impl Default for Scenario {
    fn default() -> Scenario {
        Scenario {
            games: 1,
            players: 6,
            ai: 0,
            order: IN_SEAT_ORDER,
            teams: 0,
            watchers: 3,
            steps: 5400,
            loss: 0.0,
            burst_every: 0,
            burst_len: 0,
            latency: 0,
            jitter: 0,
            skew: 0,
            hitch_at: 1800,
            hitch: 0,
            pause_at: 3000,
            pause_storm: 0,
            pause_len: 150,
            delay: None,
            seed: 0x9E37_79B9_7F4A_7C15,
            round_frames: 0,
            card: 90,
            beat: secs(pinch_points::app::INTERLUDE_SECS),
            opening: secs(pinch_points::app::LINEUP_SECS + pinch_points::app::COUNTDOWN_SECS),
            listen: true,
            straggle: 0.0,
            lose_deal_start: false,
            watcher_lag: 0,
        }
    }
}

/// Seconds of the game's in steps.
fn secs(secs: f32) -> u64 {
    (secs * TICKS_PER_SECOND as f32).round() as u64
}

impl Scenario {
    /// Every seat at the table: the players and the AI.
    pub fn seats(&self) -> u8 {
        self.players + self.ai
    }

    /// The terms round `round` of game `game` is dealt on: the AI, the
    /// seat order and the scoring the scenario asks for. The second round
    /// is dealt by the host, from the terms it calls it with.
    fn terms(&self, game: usize, round: u32) -> MatchTerms {
        MatchTerms {
            bots: self.ai,
            order: match round {
                1 => self.order,
                _ => IN_SEAT_ORDER,
            },
            teams: self.teams,
            seed: match round {
                1 => MatchTerms::default().seed,
                _ => 0xD1CE + game as u64,
            },
            ..MatchTerms::default()
        }
    }

    /// The slowest round trip a host's pings would see here, in ticks:
    /// out and back through a proxy's two legs at their worst jitter, and
    /// the tick the answer waits to be read.
    pub fn worst_round_trip(&self) -> u64 {
        2 * (self.latency + self.jitter) + 1
    }

    /// The input delay the table plays with: the one asked for, or what a
    /// host measuring [`Self::worst_round_trip`] would pick.
    pub fn delay(&self) -> u32 {
        self.delay.unwrap_or_else(|| {
            let seconds = self.worst_round_trip() as f32 / TICKS_PER_SECOND as f32;
            delay_for_round_trip(seconds)
        })
    }
}

/// Seeded xorshift: the proxies' dice, so a scenario replays exactly.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn chance(&mut self, p: f64) -> bool {
        p > 0.0 && (self.next() % 1_000_000) as f64 / 1_000_000.0 < p
    }
}

/// Datagrams and bytes one way, by message kind, and the inputs the
/// input datagrams carried.
#[derive(Clone, Default, Debug)]
pub struct Flow {
    pub datagrams: u64,
    pub bytes: u64,
    pub max: usize,
    pub inputs: u64,
    pub by_kind: BTreeMap<&'static str, (u64, u64)>,
}

impl Flow {
    fn count(&mut self, bytes: &[u8]) {
        self.datagrams += 1;
        self.bytes += bytes.len() as u64;
        self.max = self.max.max(bytes.len());
        if let Some(NetMsg::Inputs(batch)) = NetMsg::decode(bytes) {
            self.inputs += batch.inputs.len() as u64;
        }
        let entry = self.by_kind.entry(kind(bytes)).or_default();
        entry.0 += 1;
        entry.1 += bytes.len() as u64;
    }

    /// The inputs an input datagram carried, on average.
    pub fn inputs_per_datagram(&self) -> f64 {
        let datagrams = self.by_kind.get("inputs").map_or(0, |(d, _)| *d);
        self.inputs as f64 / datagrams.max(1) as f64
    }

    fn add(&mut self, other: &Flow) {
        self.datagrams += other.datagrams;
        self.bytes += other.bytes;
        self.max = self.max.max(other.max);
        self.inputs += other.inputs;
        for (kind, (d, b)) in &other.by_kind {
            let entry = self.by_kind.entry(kind).or_default();
            entry.0 += d;
            entry.1 += b;
        }
    }
}

fn kind(bytes: &[u8]) -> &'static str {
    match NetMsg::decode(bytes) {
        Some(NetMsg::Inputs(_)) => "inputs",
        Some(NetMsg::Hash { .. }) => "hash",
        Some(NetMsg::Pause { .. }) => "pause",
        Some(NetMsg::Resume { .. }) => "resume",
        Some(NetMsg::Hello { .. } | NetMsg::Watch { .. }) => "greet",
        Some(NetMsg::Start(_)) => "start",
        Some(NetMsg::CatchUp { .. }) => "catchup",
        Some(_) => "other",
        None => "undecodable",
    }
}

/// One direction through a proxy: its dice, its burst, and what it is
/// holding back for later.
struct Leg {
    rng: Rng,
    burst_left: u64,
    held: Vec<(u64, SocketAddr, Vec<u8>)>,
    flow: Flow,
    /// Until this step, an input datagram may be held back past the next
    /// round's deal ([`Scenario::straggle`]), and let go at a step between
    /// these two.
    straggle_until: u64,
    release: (u64, u64),
    /// The next `Start` of a round after the first is lost.
    lose_start: bool,
    /// Steps everything is lost in, from the first to before the second.
    blackout: (u64, u64),
}

impl Leg {
    fn new(seed: u64) -> Leg {
        Leg {
            rng: Rng(seed | 1),
            burst_left: 0,
            held: Vec::new(),
            flow: Flow::default(),
            straggle_until: 0,
            release: (0, 0),
            lose_start: false,
            blackout: (0, 0),
        }
    }

    fn new_step(&mut self, scenario: &Scenario) {
        if self.burst_left > 0 {
            self.burst_left -= 1;
        } else if scenario.burst_every > 0 && self.rng.chance(1.0 / scenario.burst_every as f64) {
            self.burst_left = scenario.burst_len;
        }
    }

    /// A step to let a straggler go at.
    fn released(&mut self) -> u64 {
        let (from, to) = self.release;
        from + self.rng.next() % (to - from + 1)
    }

    /// The host's round is over at `step`: from now until `until`, and
    /// among what this leg already holds, input datagrams may straggle
    /// past the deal, landing between `release.0` and `release.1`.
    fn straggle(&mut self, scenario: &Scenario, until: u64, release: (u64, u64)) {
        self.straggle_until = until;
        self.release = release;
        let mut held = std::mem::take(&mut self.held);
        for (at, _, bytes) in &mut held {
            if kind(bytes) == "inputs" && self.rng.chance(scenario.straggle) {
                *at = self.released();
            }
        }
        self.held = held;
    }

    fn take(&mut self, step: u64, scenario: &Scenario, to: SocketAddr, bytes: Vec<u8>) {
        self.flow.count(&bytes);
        if self.lose_start
            && matches!(NetMsg::decode(&bytes), Some(NetMsg::Start(start)) if start.deal > 0)
        {
            self.lose_start = false;
            return;
        }
        let blacked_out = step >= self.blackout.0 && step < self.blackout.1;
        if blacked_out || self.burst_left > 0 || self.rng.chance(scenario.loss) {
            return;
        }
        let jitter = match scenario.jitter {
            0 => 0,
            j => self.rng.next() % (j + 1),
        };
        let at = if step < self.straggle_until
            && kind(&bytes) == "inputs"
            && self.rng.chance(scenario.straggle)
        {
            self.released()
        } else {
            step + scenario.latency + jitter
        };
        self.held.push((at, to, bytes));
    }

    fn deliver(&mut self, step: u64, socket: &UdpSocket) {
        self.held.retain(|(at, to, bytes)| {
            let due = *at <= step;
            if due {
                let _ = socket.send_to(bytes, to);
            }
            !due
        });
    }
}

/// Sits between one spoke and the host: the spoke dials the proxy, and
/// the host knows the spoke by the proxy's address.
struct Proxy {
    socket: UdpSocket,
    host: SocketAddr,
    spoke: Option<SocketAddr>,
    up: Leg,
    down: Leg,
}

impl Proxy {
    fn new(host: SocketAddr, seed: u64) -> Proxy {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("proxy socket");
        socket.set_nonblocking(true).expect("non-blocking");
        Proxy {
            socket,
            host,
            spoke: None,
            up: Leg::new(seed),
            down: Leg::new(seed.rotate_left(17) ^ 0xD1CE),
        }
    }

    fn addr(&self) -> SocketAddr {
        self.socket.local_addr().expect("proxy address")
    }

    /// Take in whatever either end sent, and pass on whatever is due.
    fn pump(&mut self, step: u64, scenario: &Scenario) {
        let mut buf = [0u8; 2048];
        while let Ok((len, from)) = self.socket.recv_from(&mut buf) {
            let bytes = buf[..len].to_vec();
            if from == self.host {
                if let Some(spoke) = self.spoke {
                    self.down.take(step, scenario, spoke, bytes);
                }
            } else {
                self.spoke = Some(from);
                self.up.take(step, scenario, self.host, bytes);
            }
        }
        self.up.deliver(step, &self.socket);
        self.down.deliver(step, &self.socket);
    }
}

/// Who a peer is at the table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Host,
    Joiner,
    Watcher,
}

/// Where a peer is in a scenario of two rounds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// Playing round 1 or round 2.
    Playing(u32),
    /// On round 1's results card; the host deals the next at this step.
    Card { deal_at: u64 },
    /// Walked on to round 2, in the interlude, line-up and count until
    /// this step.
    Interlude { until: u64 },
}

struct Seat {
    role: Role,
    net: OnlineSession,
    board: Board,
    /// The state hashes this peer took, by round and frame.
    hashes: BTreeMap<(u32, u32), u64>,
    proxy: Option<Proxy>,
    phase: Phase,
    /// Steps in a row this player's frame has not moved, in round 1.
    stalled: u32,
}

/// What a played scenario measured.
#[derive(Default, Debug)]
pub struct Report {
    pub seconds: f64,
    /// Every peer's traffic, summed per role: what each sent (`up`) and
    /// what it was sent (`down`). The host's own are its spokes' downs and
    /// ups, read off the proxies.
    pub sent: BTreeMap<&'static str, Flow>,
    pub peers_of: BTreeMap<&'static str, usize>,
    /// Steps a player's frame did not move, out of the steps it was
    /// expected to, outside the scripted skew, hitch and pause.
    pub player_stalls: u64,
    pub player_steps: u64,
    pub watcher_stalls: u64,
    pub watcher_steps: u64,
    /// Steps that played two frames at once on a player: a visible lurch.
    pub double_steps: u64,
    /// Round 1's player stalls by how long each lasted, in steps in a row:
    /// 1, 2 to 3, 4 to 9 (up to a third of a second) and 10 or more.
    pub stall_runs: [u64; 4],
    /// The longest a player stood still in round 1, in steps.
    pub longest_stall: u32,
    /// Game 0's players' leads (next commit minus frame), sampled every
    /// 300 steps, with the player stalls of every game in those steps.
    pub leads: Vec<(u64, Vec<u32>, u64)>,
    /// The highest lead any player of any game held once the scripted
    /// skew and hitch had had 150 steps to settle.
    pub settled_lead: u32,
    pub max_lead: u32,
    /// Games that called a desync, and hash frames compared across peers
    /// with the ones that disagreed.
    pub desyncs: usize,
    pub hashes_compared: usize,
    pub hash_mismatches: usize,
    /// Lockstep datagrams every peer dropped for naming another round
    /// than its own (`OnlineSession::stale_rounds`): a two-round scenario's
    /// stragglers.
    pub stale_rounds: u64,
    /// The frames each peer of game 0 ended on.
    pub frames: Vec<u32>,
    /// The host's datagrams a second while the scripted pause held, and
    /// over the same span before it.
    pub host_dgps_paused: f64,
    pub host_dgps_before_pause: f64,
    /// How long each host's tick took (its pump, the board it advanced and
    /// every datagram it wrote), in microseconds: the host's CPU per tick.
    pub host_tick_us: Vec<u32>,
    /// Of a scenario of two rounds: the peers that reached round 2, of
    /// all of them, and how the second round went from 150 steps after
    /// its host began it (the stalls, the players' highest lead, and
    /// game 0's frames and input delay).
    pub peers: usize,
    pub reached_round_two: usize,
    pub round_two: RoundTwo,
    /// Peers walked on to round 2 before they had played round 1 out:
    /// its last frames never reached them, nor its results card.
    pub missed_card: usize,
    /// Seats a host gave to the AI for silence, in either round.
    pub abandoned: usize,
}

/// How the second round of a scenario of two went, once settled.
#[derive(Default, Debug)]
pub struct RoundTwo {
    pub player_stalls: u64,
    pub player_steps: u64,
    pub watcher_stalls: u64,
    pub watcher_steps: u64,
    pub settled_lead: u32,
    pub delay: u32,
    pub frames: Vec<u32>,
    /// The seats each peer of game 0 holds the AI in for the round, by
    /// the terms it plays: one table, so one answer.
    pub ai_seats: Vec<Vec<u8>>,
}

impl RoundTwo {
    pub fn player_stall_ratio(&self) -> f64 {
        self.player_stalls as f64 / self.player_steps.max(1) as f64
    }

    pub fn watcher_stall_ratio(&self) -> f64 {
        self.watcher_stalls as f64 / self.watcher_steps.max(1) as f64
    }
}

impl Report {
    /// The datagrams the host of one game sent a tick, on average.
    pub fn host_datagrams_per_tick(&self) -> f64 {
        let games = self.peers_of.get("host out").copied().unwrap_or(1).max(1) as f64;
        let sent = self.sent.get("host out").map_or(0, |flow| flow.datagrams) as f64;
        sent / games / (self.seconds * 30.0).max(1.0)
    }

    /// The largest datagram anybody sent.
    pub fn max_datagram(&self) -> usize {
        self.sent.values().map(|flow| flow.max).max().unwrap_or(0)
    }

    pub fn player_stall_ratio(&self) -> f64 {
        self.player_stalls as f64 / self.player_steps.max(1) as f64
    }

    pub fn watcher_stall_ratio(&self) -> f64 {
        self.watcher_stalls as f64 / self.watcher_steps.max(1) as f64
    }
}

/// The action peer `i` takes at `step`: a placement now and then, spread
/// over the board so the rounds are not idle.
fn action(step: u64, i: usize) -> PlayerAction {
    if (step + i as u64 * 13).is_multiple_of(45) {
        PlayerAction::Place {
            x: ((step / 7 + i as u64) % 20) as u8,
            y: ((step / 3) % 13) as u8,
            dir: Direction::Down,
        }
    } else {
        PlayerAction::None
    }
}

/// The board game `game` plays round `round` on: the 20x13 generated
/// beach, built and turned for the round as a match deals it, every seat
/// building its own from the seed.
fn board_for(scenario: &Scenario, game: usize, round: u32) -> Board {
    pinch_points::app::match_setup::versus_board(
        pinch_points::app::match_setup::MapChoice::GenXl,
        0x5EA7 + game as u64 + 1000 * u64::from(round),
        scenario.seats(),
    )
}

/// The AI's moves into a frame the lockstep agreed, every seat the terms
/// give the AI that nobody committed for, as each peer of a round fills
/// them in the game ([`pinch_points::app::match_setup::bot_seats_from`]).
fn fill_ai(board: &Board, net: &OnlineSession, actions: &mut [PlayerAction; MAX_PLAYERS]) {
    let ai = pinch_points::app::match_setup::bot_seats_from(&net.terms, net.seats);
    for (seat, action) in actions.iter_mut().enumerate() {
        if let Some(level) = ai[seat]
            && matches!(action, PlayerAction::None)
        {
            *action = bot_action(board, seat as u8, level);
        }
    }
}

fn table(scenario: &Scenario, game: usize) -> Vec<Seat> {
    let players = scenario.players;
    let terms = scenario.terms(game, 1);
    let mut dealt: Vec<u8> = scenario.order[..usize::from(scenario.seats())].to_vec();
    dealt.sort_unstable();
    assert_eq!(
        dealt,
        (0..scenario.seats()).collect::<Vec<u8>>(),
        "the order {:?} seats each place of {} in a seat of its own",
        scenario.order,
        scenario.seats()
    );
    let seats: Vec<u8> = terms.player_seats(scenario.seats());
    let board = board_for(scenario, game, 1);
    let host_socket = UdpTransport::host(0).expect("host socket");
    let host_addr: SocketAddr = format!(
        "127.0.0.1:{}",
        host_socket.local_addr().expect("host address").port()
    )
    .parse()
    .expect("host address");
    let mut table = vec![Seat {
        role: Role::Host,
        net: OnlineSession::new(
            host_socket,
            Lockstep::new(terms.seat_of_place(0), seats.clone(), scenario.delay()),
            scenario.seats(),
            terms,
        ),
        board: board.clone(),
        hashes: BTreeMap::new(),
        proxy: None,
        phase: Phase::Playing(1),
        stalled: 0,
    }];
    let peers = usize::from(players) + scenario.watchers;
    for i in 1..peers {
        let seed = scenario.seed ^ ((game as u64) << 32) ^ (i as u64 * 0x9E37_79B9);
        let mut proxy = Proxy::new(host_addr, seed);
        if i == peers - 1 {
            proxy.down.blackout = watcher_lag(scenario);
        }
        let socket = UdpTransport::join(proxy.addr()).expect("spoke socket");
        let (role, lockstep) = if i < usize::from(players) {
            socket.send(NetMsg::hello("P"));
            (
                Role::Joiner,
                Lockstep::new(
                    terms.seat_of_place(i as u8),
                    seats.clone(),
                    scenario.delay(),
                ),
            )
        } else {
            socket.send(NetMsg::watch("W"));
            (
                Role::Watcher,
                Lockstep::observer(seats.clone(), scenario.delay()),
            )
        };
        table.push(Seat {
            role,
            net: OnlineSession::new(socket, lockstep, scenario.seats(), terms),
            board: board.clone(),
            hashes: BTreeMap::new(),
            proxy: Some(proxy),
            phase: Phase::Playing(1),
            stalled: 0,
        });
    }
    table
}

/// The steps the last watcher lags in ([`Scenario::watcher_lag`]), from
/// the first to before the second.
fn watcher_lag(scenario: &Scenario) -> (u64, u64) {
    let end = u64::from(scenario.round_frames);
    match scenario.watchers {
        0 => (0, 0),
        _ => (end.saturating_sub(scenario.watcher_lag), end),
    }
}

/// Whether peer `i` of `peers` sits this step out: joiner 1's late start
/// and its hitch, and the last watcher's lag before round 1 ends.
fn sits_out(scenario: &Scenario, i: usize, peers: usize, step: u64) -> bool {
    let (from, until) = watcher_lag(scenario);
    let lags = i == peers - 1 && step >= from && step < until;
    lags || i == 1
        && (step < scenario.skew
            || (step >= scenario.hitch_at && step < scenario.hitch_at + scenario.hitch))
}

/// Whether `step` is one a stall is expected on: the warm-up and the late
/// start, the hitch itself, and the pause and its wake.
fn excused(scenario: &Scenario, step: u64) -> bool {
    let pausing = scenario.pause_storm > 0
        && step >= scenario.pause_at
        && step < scenario.pause_at + scenario.pause_len + 60;
    let hitched = step >= scenario.hitch_at && step < scenario.hitch_at + scenario.hitch;
    step < 10 + scenario.skew || hitched || pausing
}

/// Whether `step` is one the leads should have settled by.
fn settled(scenario: &Scenario, step: u64) -> bool {
    let after_skew = step >= scenario.skew + 150;
    let hitched = scenario.hitch > 0
        && step >= scenario.hitch_at
        && step < scenario.hitch_at + scenario.hitch + 150;
    let pausing = scenario.pause_storm > 0
        && step >= scenario.pause_at
        && step < scenario.pause_at + scenario.pause_len + 150;
    after_skew && !hitched && !pausing
}

/// Seat the table as a lobby's launch would: every greeting through to
/// the host, sure of arriving, and each peer dealt the place its session
/// already has, joiner `i` seat `i` and the watchers none.
fn seat(scenario: &Scenario, table: &mut [Seat]) {
    let calm = Scenario {
        loss: 0.0,
        burst_every: 0,
        latency: 0,
        jitter: 0,
        ..scenario.clone()
    };
    for seat in table.iter_mut() {
        if let Some(proxy) = &mut seat.proxy {
            proxy.pump(0, &calm);
        }
    }
    // Heard as a host hears them, so it knows who came to watch, which
    // the deal of a second round goes by.
    let host = &mut table[0].net;
    host.poll_between_rounds(0.0);
    assert_eq!(
        host.transport.peer_count(),
        table.len() - 1,
        "every greeting is in"
    );
    let plan: Vec<Option<u8>> = table[1..]
        .iter()
        .map(|seat| seat.net.session.seat())
        .collect();
    table[0].net.seat_the_table(&plan);
    // The host remembers where its partner picks put the AI, which deals
    // the next round's AI back into those seats ([`Scenario::order`]).
    if scenario.order != IN_SEAT_ORDER {
        let host = &mut table[0].net;
        host.arranged = Some(pinch_points::app::net::order::Arranged {
            bots: Vec::new(),
            ai: host.terms.ai_seats(scenario.seats()),
        });
    }
}

/// The frame round `round` of a scenario ends on: the first of two ends
/// at [`Scenario::round_frames`], and a lone round or the second plays
/// on to the last step.
fn last_frame(scenario: &Scenario, round: u32) -> u32 {
    match (round, scenario.round_frames) {
        (1, frames) if frames > 0 => frames,
        _ => u32::MAX,
    }
}

/// A peer that has been called to the next round walks into it: a new
/// board, and the interlude the game stands in before the round begins.
/// Whether it was walked on before its own round 1 was over, which is a
/// results card it never saw.
fn walk_on(scenario: &Scenario, game: usize, seat: &mut Seat, step: u64) -> bool {
    let missed = seat.phase == Phase::Playing(1);
    if seat.net.next_round {
        seat.net.next_round = false;
        seat.board = board_for(scenario, game, 2);
        seat.phase = Phase::Interlude {
            until: step + scenario.beat + scenario.opening,
        };
        return missed;
    }
    false
}

/// One step, in seconds.
const TICK: f32 = 1.0 / TICKS_PER_SECOND as f32;

/// How many frames before the end of round 1 its stragglers start: the
/// last half second of inputs on the wire may be held past the deal.
const STRAGGLE_FROM: u32 = 15;

/// The host of `table` is [`STRAGGLE_FROM`] frames from the end of round
/// 1, and will deal the next at about `deal_at`: the stragglers start,
/// and the deal's `Start` to joiner 1 is lost if the scenario says so.
fn round_one_ending(scenario: &Scenario, table: &mut [Seat], deal_at: u64) {
    let release = (deal_at + 1, deal_at + scenario.beat + 30);
    for (i, seat) in table.iter_mut().enumerate() {
        if let Some(proxy) = &mut seat.proxy {
            proxy.up.straggle(scenario, deal_at, release);
            proxy.down.straggle(scenario, deal_at, release);
            if i == 1 && scenario.lose_deal_start {
                proxy.down.lose_start = true;
            }
        }
    }
}

/// Play `scenario` to the end and measure it.
pub fn play(scenario: &Scenario) -> Report {
    let mut tables: Vec<Vec<Seat>> = (0..scenario.games).map(|g| table(scenario, g)).collect();
    let mut report = Report::default();
    for table in &mut tables {
        seat(scenario, table);
    }
    // The step each game's host began round 2 on.
    let mut round_two_from: Vec<Option<u64>> = vec![None; scenario.games];
    let mut straggling = vec![false; scenario.games];
    let pause_window = (
        scenario.pause_at + 20,
        scenario.pause_at + scenario.pause_len,
    );
    let span = pause_window.1.saturating_sub(pause_window.0);
    let mut host_out_paused = 0u64;
    let mut host_out_before = 0u64;
    for step in 0..scenario.steps {
        // The pings on the results card are timed by the table's own
        // clock, so the next round's delay is the one a host would pick
        // for this network, not for the microseconds a step takes here.
        steer_ping_clock((step * 1000 / u64::from(TICKS_PER_SECOND)) as u32);
        for (game, table) in tables.iter_mut().enumerate() {
            let host_out_was: u64 = host_out(table);
            for seat in table.iter_mut() {
                if let Some(proxy) = &mut seat.proxy {
                    proxy.up.new_step(scenario);
                    proxy.down.new_step(scenario);
                    proxy.pump(step, scenario);
                }
            }
            for i in 0..table.len() {
                if i == 1 {
                    // The host's word of this tick, out to the spokes
                    // before they tick: it ticked a moment before them.
                    for seat in table.iter_mut() {
                        if let Some(proxy) = &mut seat.proxy {
                            proxy.pump(step, scenario);
                        }
                    }
                }
                if sits_out(scenario, i, table.len(), step) {
                    continue;
                }
                let round = match table[i].phase {
                    Phase::Interlude { until } if step < until => {
                        if scenario.listen {
                            table[i].net.before_the_round(TICK);
                        }
                        continue;
                    }
                    Phase::Interlude { .. } => {
                        table[i].phase = Phase::Playing(2);
                        if i == 0 {
                            round_two_from[game] = Some(step);
                        }
                        2
                    }
                    Phase::Card { deal_at } => {
                        let seat = &mut table[i];
                        seat.net.poll_between_rounds(TICK);
                        if seat.role == Role::Host && step >= deal_at {
                            seat.net.call_next_round(scenario.terms(game, 2), None);
                        }
                        let _ = walk_on(scenario, game, seat, step);
                        continue;
                    }
                    Phase::Playing(round) => round,
                };
                if round == 1 {
                    storm(scenario, step, i, &mut table[i].net);
                }
                let last = last_frame(scenario, round);
                let seat = &mut table[i];
                // As the shell does every frame of a round: the silences
                // aged, and a seat silent past the host's patience given
                // to the AI.
                seat.net.keep_in_touch(TICK);
                report.abandoned += seat.net.abandon_stalled(TICK, false).len();
                let ticked = std::time::Instant::now();
                let before = seat.net.session.frame();
                let board = &mut seat.board;
                let hashes = &mut seat.hashes;
                seat.net.pump(action(step, i), |net| {
                    if net.session.frame() >= last {
                        return;
                    }
                    if let Some(mut actions) = net.session.advance() {
                        fill_ai(board, net, &mut actions);
                        board.tick(&actions);
                        let frame = net.session.frame();
                        net.after_frame(|| board.state_hash());
                        if frame.is_multiple_of(30) {
                            hashes.insert((round, frame), board.state_hash());
                        }
                    }
                });
                // Less than nothing for a peer walked on to the next round.
                let moved = seat.net.session.frame().saturating_sub(before);
                if seat.role == Role::Host {
                    let us = ticked.elapsed().as_micros();
                    report
                        .host_tick_us
                        .push(u32::try_from(us).unwrap_or(u32::MAX));
                }
                // A watcher commits nothing, and its lead is read as none.
                let lead = seat
                    .net
                    .session
                    .next_commit()
                    .saturating_sub(seat.net.session.frame());
                // Away from the round's end, where the frames stop for good.
                let midround = seat.net.session.frame().saturating_add(30) < last;
                if round == 1 {
                    let expected = !excused(scenario, step) && midround;
                    match seat.role {
                        Role::Host | Role::Joiner => {
                            report.max_lead = report.max_lead.max(lead);
                            if settled(scenario, step) && midround {
                                report.settled_lead = report.settled_lead.max(lead);
                            }
                            if expected {
                                report.player_steps += 1;
                                report.player_stalls += u64::from(moved == 0);
                                report.double_steps += u64::from(moved >= 2);
                            }
                            match moved {
                                0 => seat.stalled += 1,
                                _ if seat.stalled > 0 => {
                                    if expected {
                                        let run = seat.stalled;
                                        let bucket = match run {
                                            1 => 0,
                                            2..=3 => 1,
                                            4..=9 => 2,
                                            _ => 3,
                                        };
                                        report.stall_runs[bucket] += 1;
                                        report.longest_stall = report.longest_stall.max(run);
                                    }
                                    seat.stalled = 0;
                                }
                                _ => {}
                            }
                        }
                        Role::Watcher => {
                            if expected {
                                report.watcher_steps += 1;
                                report.watcher_stalls += u64::from(moved == 0);
                            }
                        }
                    }
                } else if round_two_from[game].is_some_and(|from| step >= from + 150) {
                    let two = &mut report.round_two;
                    match seat.role {
                        Role::Host | Role::Joiner => {
                            two.settled_lead = two.settled_lead.max(lead);
                            two.player_steps += 1;
                            two.player_stalls += u64::from(moved == 0);
                        }
                        Role::Watcher => {
                            two.watcher_steps += 1;
                            two.watcher_stalls += u64::from(moved == 0);
                        }
                    }
                }
                report.missed_card += usize::from(walk_on(scenario, game, seat, step));
                let host = seat.role == Role::Host;
                let frame = seat.net.session.frame();
                if seat.phase == Phase::Playing(1) && frame >= last {
                    table[i].phase = Phase::Card {
                        deal_at: step + scenario.card,
                    };
                }
                if host && round == 1 && !straggling[game] && frame + STRAGGLE_FROM >= last {
                    straggling[game] = true;
                    let deal_at = step + u64::from(STRAGGLE_FROM) + scenario.card;
                    round_one_ending(scenario, table, deal_at);
                }
            }
            let sent = host_out(table) - host_out_was;
            if step >= pause_window.0 && step < pause_window.1 {
                host_out_paused += sent;
            } else if step + span >= scenario.pause_at && step < scenario.pause_at {
                host_out_before += sent;
            }
        }
        if step % 300 == 299 {
            let leads = tables[0]
                .iter()
                .filter(|seat| seat.role != Role::Watcher)
                .map(|seat| seat.net.session.next_commit() - seat.net.session.frame())
                .collect();
            let stalls = report.player_stalls + report.round_two.player_stalls
                - report.leads.iter().map(|l| l.2).sum::<u64>();
            report.leads.push((step + 1, leads, stalls));
        }
    }
    // A last few steps with nobody committing, so the datagrams still in
    // the proxies land and the counts below are of a settled table.
    for table in &mut tables {
        for seat in table.iter_mut() {
            if let Some(proxy) = &mut seat.proxy {
                proxy.pump(scenario.steps + 1000, scenario);
            }
        }
    }
    report.seconds = scenario.steps as f64 / 30.0;
    let per_second = |datagrams: u64| datagrams as f64 * 30.0 / span.max(1) as f64;
    report.host_dgps_paused = per_second(host_out_paused) / scenario.games as f64;
    report.host_dgps_before_pause = per_second(host_out_before) / scenario.games as f64;
    for (g, table) in tables.iter().enumerate() {
        if table.iter().any(|seat| seat.net.desync_at().is_some()) {
            report.desyncs += 1;
        }
        report.stale_rounds += table
            .iter()
            .map(|seat| u64::from(seat.net.stale_rounds))
            .sum::<u64>();
        report.peers += table.len();
        report.reached_round_two += table
            .iter()
            .filter(|seat| seat.phase == Phase::Playing(2))
            .count();
        for seat in &table[1..] {
            for (frame, hash) in &seat.hashes {
                if let Some(host) = table[0].hashes.get(frame) {
                    report.hashes_compared += 1;
                    report.hash_mismatches += usize::from(host != hash);
                }
            }
        }
        if g == 0 {
            report.frames = table.iter().map(|seat| seat.net.session.frame()).collect();
            if scenario.round_frames > 0 {
                report.round_two.delay = table[0].net.session.delay();
                report.round_two.ai_seats = table
                    .iter()
                    .map(|seat| seat.net.terms.ai_seats(seat.net.seats))
                    .collect();
                report.round_two.frames = table
                    .iter()
                    .map(|seat| match seat.phase == Phase::Playing(2) {
                        true => seat.net.session.frame(),
                        false => 0,
                    })
                    .collect();
            }
        }
        for seat in &table[1..] {
            let proxy = seat.proxy.as_ref().expect("a spoke has a proxy");
            let (up, down) = match seat.role {
                Role::Joiner => ("joiner up", "joiner down"),
                Role::Watcher => ("watcher up", "watcher down"),
                Role::Host => unreachable!("the host has no proxy"),
            };
            report.sent.entry(up).or_default().add(&proxy.up.flow);
            report.sent.entry(down).or_default().add(&proxy.down.flow);
            report
                .sent
                .entry("host out")
                .or_default()
                .add(&proxy.down.flow);
            report
                .sent
                .entry("host in")
                .or_default()
                .add(&proxy.up.flow);
            *report.peers_of.entry(up).or_default() += 1;
            *report.peers_of.entry(down).or_default() += 1;
        }
        *report.peers_of.entry("host out").or_default() += 1;
        *report.peers_of.entry("host in").or_default() += 1;
    }
    report
}

/// The scripted pause storm: joiners 1..=n call a pause a few steps
/// apart, and joiner 1 lifts it.
fn storm(scenario: &Scenario, step: u64, i: usize, net: &mut OnlineSession) {
    if scenario.pause_storm == 0 || i == 0 || i > scenario.pause_storm {
        return;
    }
    if step == scenario.pause_at + 3 * (i as u64 - 1) {
        net.request_pause();
    }
    if i == 1 && step == scenario.pause_at + scenario.pause_len {
        net.request_resume();
    }
}

fn host_out(table: &[Seat]) -> u64 {
    table
        .iter()
        .filter_map(|seat| seat.proxy.as_ref())
        .map(|proxy| proxy.down.flow.datagrams)
        .sum()
}

/// The report as lines of text.
pub fn describe(scenario: &Scenario, report: &Report) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "{scenario:?}");
    let _ = writeln!(out, "input delay {}", scenario.delay());
    let _ = writeln!(
        out,
        "stalls: players {:.2}% watchers {:.2}%  double steps {}  lead max {} settled {}  desyncs {} hashes compared {} mismatched {}",
        100.0 * report.player_stall_ratio(),
        100.0 * report.watcher_stall_ratio(),
        report.double_steps,
        report.max_lead,
        report.settled_lead,
        report.desyncs,
        report.hashes_compared,
        report.hash_mismatches,
    );
    let _ = writeln!(
        out,
        "player stalls by length: 1 step {}, 2-3 {}, 4-9 {}, 10+ {}, longest {}",
        report.stall_runs[0],
        report.stall_runs[1],
        report.stall_runs[2],
        report.stall_runs[3],
        report.longest_stall,
    );
    let _ = writeln!(out, "game 0 frames {:?}", report.frames);
    let _ = writeln!(out, "stale-round datagrams dropped {}", report.stale_rounds);
    if scenario.round_frames > 0 {
        let two = &report.round_two;
        let _ = writeln!(
            out,
            "round 2: reached by {}/{} peers ({} missed round 1's end), seats given to the AI {}, delay {}, stalls players {:.2}% watchers {:.2}%, settled lead {}, game 0 frames {:?}, AI seats {:?}",
            report.reached_round_two,
            report.peers,
            report.missed_card,
            report.abandoned,
            two.delay,
            100.0 * two.player_stall_ratio(),
            100.0 * two.watcher_stall_ratio(),
            two.settled_lead,
            two.frames,
            two.ai_seats,
        );
    }
    let mut ticks = report.host_tick_us.clone();
    ticks.sort_unstable();
    let at =
        |q: f64| ticks.get(((ticks.len() as f64 * q) as usize).min(ticks.len().saturating_sub(1)));
    let _ = writeln!(
        out,
        "host tick us: median {:?} p99 {:?} max {:?}",
        at(0.5).copied().unwrap_or(0),
        at(0.99).copied().unwrap_or(0),
        ticks.last().copied().unwrap_or(0)
    );
    let _ = writeln!(
        out,
        "host out {:.2} dg/tick, largest datagram {} B",
        report.host_datagrams_per_tick(),
        report.max_datagram()
    );
    for (role, flow) in &report.sent {
        let peers = report.peers_of.get(role).copied().unwrap_or(1).max(1) as f64;
        let per = (report.seconds * peers).max(1e-9);
        let kinds: Vec<String> = flow
            .by_kind
            .iter()
            .map(|(kind, (d, b))| {
                format!(
                    "{kind} {:.1}/s {:.0}B",
                    *d as f64 / per,
                    *b as f64 / (*d).max(1) as f64
                )
            })
            .collect();
        let _ = writeln!(
            out,
            "{role:>13}: {:7.1} dg/s {:8.1} KB/s max {:4}B {:5.1} inputs/dg per peer | {}",
            flow.datagrams as f64 / per,
            flow.bytes as f64 / per / 1000.0,
            flow.max,
            flow.inputs_per_datagram(),
            kinds.join(", ")
        );
    }
    if scenario.pause_storm > 0 {
        let _ = writeln!(
            out,
            "host out during the pause {:.0} dg/s, before it {:.0} dg/s",
            report.host_dgps_paused, report.host_dgps_before_pause
        );
    }
    let _ = writeln!(
        out,
        "leads (game 0 players) and player stalls, every 300 steps:"
    );
    for (step, leads, stalls) in &report.leads {
        let _ = writeln!(out, "  {step:5}: {leads:?} {stalls}");
    }
    out
}
