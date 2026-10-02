//! Bots in the game itself: seats on a couch held by bots, and the seat a
//! bot drives when it joins somebody's party through its author's game.
//!
//! The game never runs a bot (`docs/bot-seats.md`). It opens a doorway, a
//! listener on this machine with a key per chair, prints the string to
//! start each bot with, and once a bot has registered drives that bot's
//! seat from its replies: on the live clock, a tick sent each fixed step
//! and the newest reply in hand committed on the next, under the fair
//! cursor, since a person is at the table. A bot whose connection drops
//! idles for a grace period and then the game's AI stands in for it,
//! until it comes back with its token.

use crate::app::i18n::fill;
use crate::app::{Controllers, SeatController, SeatKinds, SeatNames, Sim};
use crate::bots::connstr::{ConnString, DEFAULT_PORT, Key};
use crate::bots::cursor::SeatCursor;
use crate::bots::listener::{Admission, BotId, Config, Event, GameLink, Invite, Listener, View};
use crate::bots::lookahead;
use crate::bots::protocol::{self, Act, Clock, Outcome, SeatKind, Table, You};
use crate::bots::seat::{BotDriver, Journal, Turn, act_of, outcome, placings, refusal};
use crate::sim::{Board, BotLevel, MAX_PLAYERS, PlayerAction, Refusal, bot_action};
use bevy::prelude::*;
use std::net::SocketAddr;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// How long a dropped bot's seat idles before the game's AI stands in.
pub const GRACE: Duration = Duration::from_secs(5);

/// The one deadline a bot at a table with people gets: a tick.
const LIVE_DEADLINE_MS: u32 = 1000 / crate::sim::TICKS_PER_SECOND;

/// One seat waiting for a bot, or held by one.
pub struct Chair {
    /// Which chair: a number of its own, so a chair keeps its key and its
    /// bot while others come and go around it.
    id: u32,
    pub seat: u8,
    pub key: Key,
    pub bot: Option<BotId>,
}

/// A listener on this machine for bots to register with, a chair per seat
/// that wants one.
pub struct Doorway {
    listener: Listener,
    /// Behind a lock so the doorway can live in a resource.
    events: std::sync::Mutex<Receiver<Event>>,
    /// Where it listens for this machine, and for the LAN once opened to it.
    here: SocketAddr,
    lan: Option<SocketAddr>,
    /// Whether the strings shown are the LAN's.
    pub showing_lan: bool,
    pub chairs: Vec<Chair>,
    next_chair: u32,
}

impl Drop for Doorway {
    fn drop(&mut self) {
        self.listener.close();
    }
}

impl Doorway {
    /// A doorway on this machine: the bots' own port if it is free, any
    /// port if not (a second game on one machine, an arena running).
    pub fn open() -> std::io::Result<Doorway> {
        let config = Config::new(Admission::Keys(Vec::new()));
        let (listener, events) = Listener::bind(
            SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)),
            config.clone(),
        )
        .or_else(|_| Listener::bind(SocketAddr::from(([127, 0, 0, 1], 0)), config))?;
        let here = listener.local_addr();
        Ok(Doorway {
            listener,
            events: std::sync::Mutex::new(events),
            here,
            lan: None,
            showing_lan: false,
            chairs: Vec::new(),
            next_chair: 0,
        })
    }

    /// Chairs for exactly these seats: a chair already holding a bot for a
    /// seat still wanting one keeps it, and a seat new to the list gets a
    /// fresh, single-use key.
    pub fn seat_bots(&mut self, seats: &[u8]) {
        self.chairs.retain(|chair| seats.contains(&chair.seat));
        for &seat in seats {
            if self.chairs.iter().any(|chair| chair.seat == seat) {
                continue;
            }
            let key = match (self.next_chair, crate::app::dev::bot_key()) {
                (0, Some(key)) => key,
                _ => Key::draw(),
            };
            let chair = Chair {
                id: self.next_chair,
                seat,
                key,
                bot: None,
            };
            self.next_chair += 1;
            self.listener.invite(Invite {
                key: chair.key.clone(),
                uses: Some(1),
                owner: None,
                slot: Some(chair.id),
            });
            self.chairs.push(chair);
        }
        self.chairs.sort_by_key(|chair| chair.seat);
    }

    /// Take what the listener heard: each registration sits its bot in the
    /// chair its key was printed for.
    pub fn poll(&mut self) {
        let events = self
            .events
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while let Ok(event) = events.try_recv() {
            if let Event::Registered(id) = event
                && let Some(info) = self.listener.bot(id)
                && let Some(chair) = self.chairs.iter_mut().find(|c| Some(c.id) == info.slot)
            {
                chair.bot = Some(id);
            }
        }
    }

    /// Every chair has its bot.
    pub fn ready(&self) -> bool {
        self.chairs.iter().all(|chair| chair.bot.is_some())
    }

    /// Show the strings on the LAN address rather than this machine's,
    /// listening there too from the first time it is asked for.
    pub fn toggle_lan(&mut self) -> std::io::Result<()> {
        if self.lan.is_none() {
            self.lan = Some(
                self.listener
                    .also_listen(SocketAddr::from(([0, 0, 0, 0], 0)))?,
            );
        }
        self.showing_lan = !self.showing_lan;
        Ok(())
    }

    /// The string a chair's bot is started with.
    pub fn string(&self, chair: &Chair) -> String {
        let (host, port) = match (self.showing_lan, self.lan) {
            (true, Some(lan)) => (crate::bots::connstr::reachable_host(lan), lan.port()),
            _ => (self.here.ip().to_string(), self.here.port()),
        };
        ConnString::new(host, port, Some(chair.key.clone())).to_string()
    }

    /// What a chair's bot said it is: `Greedy 0.3`, so the author can tell
    /// it is the build they meant to start.
    pub fn bot_line(&self, chair: &Chair) -> Option<(String, Option<Duration>)> {
        let info = self.listener.bot(chair.bot?)?;
        let name = match info.version.is_empty() {
            true => info.name,
            false => format!("{} {}", info.name, info.version),
        };
        Some((name, self.listener.rtt(info.id)))
    }

    /// What the seat a bot holds is called: its registered name and its
    /// owner's, "Greedy (Ana's bot)", or "Greedy (bot)" if it gave none.
    pub fn seat_name(&self, tr: &crate::app::i18n::Tr, seat: u8) -> Option<String> {
        let chair = self.chairs.iter().find(|chair| chair.seat == seat)?;
        let info = self.listener.bot(chair.bot?)?;
        Some(match info.owner {
            Some(owner) => fill(tr.bot_owned, &[("bot", &info.name), ("o", &owner)]),
            None => fill(tr.bot_unowned, &[("bot", &info.name)]),
        })
    }
}

/// What is happening at a bot's seat that the table should be told.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum News {
    /// Its connection went and the grace is up: the AI is in its chair.
    StandIn,
    /// It came back, and has its seat again.
    Back,
}

/// One round played with bots in it.
pub struct BotRound {
    link: GameLink,
    game: u32,
    drivers: Vec<BotDriver>,
    cursors: [Option<SeatCursor>; MAX_PLAYERS],
    last: [Option<(&'static str, Outcome)>; MAX_PLAYERS],
    dropped_at: [Option<Instant>; MAX_PLAYERS],
    standing_in: [bool; MAX_PLAYERS],
    names: Vec<String>,
    kinds: Vec<SeatKind>,
    clock: Clock,
    log: Box<dyn std::io::Write + Send + Sync>,
}

/// An act the seat layer committed this tick, and what the sim would say
/// to it, for the outcome once the tick has run.
pub struct Committed {
    seat: u8,
    act: Option<Act>,
    action: PlayerAction,
    refused: Option<Refusal>,
}

impl BotRound {
    fn table(&self) -> Table<'_> {
        Table {
            game: self.game,
            names: &self.names,
            kinds: &self.kinds,
            clock: self.clock,
            cursor: true,
        }
    }

    /// Take everything the bots have said since the last tick.
    fn drain(&mut self, t: u64, news: &mut Vec<(u8, News)>) {
        let mut console = |line: String| info!("{line}");
        let mut journal = Journal {
            log: &mut self.log,
            console: &mut console,
            echo_notes: true,
            game: self.game,
        };
        while let Some(msg) = self.link.try_recv() {
            let seat = match &msg {
                crate::bots::listener::GameMsg::Reply { seat, .. }
                | crate::bots::listener::GameMsg::Ready { seat }
                | crate::bots::listener::GameMsg::Garbled { seat, .. }
                | crate::bots::listener::GameMsg::Dropped { seat }
                | crate::bots::listener::GameMsg::Back { seat } => *seat,
            };
            if let Some(driver) = self.drivers.iter_mut().find(|d| d.seat == seat) {
                driver.take(msg, t, &mut journal);
            }
        }
        // A bot comes back by resuming: `hello` again before its next tick.
        let now = Instant::now();
        for driver in &mut self.drivers {
            let seat = usize::from(driver.seat);
            if driver.connected {
                if std::mem::take(&mut self.standing_in[seat]) {
                    news.push((driver.seat, News::Back));
                }
                self.dropped_at[seat] = None;
            } else {
                let since = *self.dropped_at[seat].get_or_insert(now);
                if !self.standing_in[seat] && now.duration_since(since) >= GRACE {
                    self.standing_in[seat] = true;
                    news.push((driver.seat, News::StandIn));
                }
            }
        }
    }

    /// Fill the bot seats' actions for the tick about to run from `board`.
    pub fn commit(
        &mut self,
        board: &Board,
        actions: &mut [PlayerAction; MAX_PLAYERS],
        news: &mut Vec<(u8, News)>,
    ) -> Vec<Committed> {
        let t = board.ticks();
        self.drain(t, news);
        let mut console = |line: String| info!("{line}");
        let mut journal = Journal {
            log: &mut self.log,
            console: &mut console,
            echo_notes: true,
            game: self.game,
        };
        let mut committed = Vec::new();
        for driver in &mut self.drivers {
            let seat = driver.seat;
            let slot = usize::from(seat);
            let (action, act) = if self.standing_in[slot] {
                // The AI walks by the same rule, read off the board.
                let action = bot_action(board, seat, BotLevel::Normal);
                (action, act_of(action))
            } else {
                let order = driver.order(&mut journal);
                match &mut self.cursors[slot] {
                    Some(cursor) => cursor.step(board, seat, order, true),
                    None => (PlayerAction::None, None),
                }
            };
            actions[slot] = action;
            committed.push(Committed {
                seat,
                act,
                action,
                refused: refusal(board, seat, action),
            });
        }
        committed
    }

    /// After the tick: what became of each act, then the next tick out.
    pub fn after(&mut self, board: &Board, committed: Vec<Committed>) {
        let t = board.ticks();
        for c in committed {
            let Some(act) = c.act else {
                continue;
            };
            if let Some(result) =
                outcome(act, c.action, c.refused, board, c.seat, t.saturating_sub(1))
            {
                self.last[usize::from(c.seat)] = Some((act.token(), result));
            }
        }
        let cursors: Vec<Option<(u8, u8)>> = (0..self.names.len())
            .map(|seat| self.cursors.get(seat).copied().flatten().map(|c| c.at))
            .collect();
        *self
            .link
            .view
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(View {
            board: board.clone(),
            cursors: cursors.clone(),
            budget: vec![lookahead::CAP_LIVE; self.names.len()],
        });
        let now = Instant::now();
        for i in 0..self.drivers.len() {
            let seat = self.drivers[i].seat;
            if std::mem::take(&mut self.drivers[i].rehello) && self.drivers[i].playing() {
                let hello = protocol::hello(board, &self.table(), seat, true);
                self.link.send(self.drivers[i].id, &hello);
            }
            if self.drivers[i].turn(t, now) != Turn::Send {
                continue;
            }
            let you = You {
                seat,
                last: self.last[usize::from(seat)],
            };
            let msg = protocol::tick(board, self.game, &you, &cursors);
            if self.link.send(self.drivers[i].id, &msg) {
                self.drivers[i].sent(t, now);
            }
        }
    }

    /// The round is over: every bot is told how it went.
    fn finish(&self, board: &Board) {
        let n = self.names.len();
        let scores: Vec<u32> = board.scores()[..n].to_vec();
        let places = placings(&scores, &vec![false; n]);
        for driver in &self.drivers {
            let msg = serde_json::json!({
                "type": "end", "game": self.game, "scores": scores,
                "placing": places[usize::from(driver.seat)], "replay": format!("p-g{}", self.game),
            });
            self.link.send(driver.id, &msg);
        }
    }
}

/// Bots in this game: the doorway they came in by, and the round being
/// played with them.
#[derive(Resource, Default)]
pub struct BotSeats {
    pub door: Option<Doorway>,
    round: Option<BotRound>,
    /// What the table should be told, for [`announce`] to word.
    pub news: Vec<(u8, News)>,
    /// A doorway card is up, and what it is for.
    pub waiting: Option<Waiting>,
    /// What the card has to say about the last key pressed on it.
    pub feedback: String,
    games: u32,
}

/// What a doorway card is waiting for its bots in order to do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Waiting {
    /// Start the match being set up on this couch.
    Couch,
    /// Join the beach picked in the lobby as a bot (route 1).
    Join,
}

impl BotSeats {
    /// The round in play, if bots hold seats in it.
    pub fn round_mut(&mut self) -> Option<&mut BotRound> {
        self.round.as_mut()
    }
}

/// What the fixed tick reads to fill every seat: who decides for each, and
/// the bots among them.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Drivers<'w> {
    pub controllers: Res<'w, Controllers>,
    pub bots: ResMut<'w, BotSeats>,
}

impl Drivers<'_> {
    /// Fill the bot seats' actions for the tick about to run from `board`.
    /// `None` when no bot holds a seat.
    pub fn commit(
        &mut self,
        board: &Board,
        actions: &mut [PlayerAction; MAX_PLAYERS],
    ) -> Option<Vec<Committed>> {
        let BotSeats { round, news, .. } = &mut *self.bots;
        Some(round.as_mut()?.commit(board, actions, news))
    }

    /// After the tick: outcomes, the next tick out, and the end of the
    /// round if this was it.
    pub fn after(&mut self, board: &Board, committed: Option<Vec<Committed>>) {
        let (Some(round), Some(committed)) = (self.bots.round.as_mut(), committed) else {
            return;
        };
        round.after(board, committed);
        if board.round_over() {
            round.finish(board);
        }
    }
}

/// Open the round's bot seats, on entering the arena: a driver and a hand
/// for every seat a bot holds, `hello` to each, and the seat named for its
/// bot. Chained after the names are resolved, which it overwrites for
/// those seats, and before the panels are drawn from them.
pub fn begin_round(
    controllers: Res<Controllers>,
    sim: Res<Sim>,
    settings: Res<crate::app::settings::GameSettings>,
    mut names: ResMut<SeatNames>,
    kinds: Res<SeatKinds>,
    mut bots: ResMut<BotSeats>,
    mut recorder: ResMut<crate::app::Recorder>,
) {
    bots.round = None;
    bots.news.clear();
    let seats: Vec<u8> = (0..MAX_PLAYERS as u8)
        .filter(|&seat| controllers.0[usize::from(seat)] == SeatController::Bot)
        .collect();
    if seats.is_empty() {
        return;
    }
    bots.games += 1;
    let game = bots.games;
    let Some(door) = &bots.door else {
        return;
    };
    let tr = settings.tr();
    for &seat in &seats {
        if let Some(name) = door.seat_name(tr, seat) {
            names.0[usize::from(seat)] = name;
        }
    }
    if let Some(replay) = &mut recorder.0 {
        replay.names = names.0.clone();
    }
    let board = &sim.0;
    let n = usize::from(board.seats_in_play()).max(2);
    let deadline = Duration::from_millis(u64::from(LIVE_DEADLINE_MS));
    let mut drivers = Vec::new();
    let mut routes = Vec::new();
    for &seat in &seats {
        let Some(id) = door
            .chairs
            .iter()
            .find(|c| c.seat == seat)
            .and_then(|c| c.bot)
        else {
            continue;
        };
        routes.push((id, seat));
        drivers.push(BotDriver::new(
            id,
            seat,
            names.0[usize::from(seat)].clone(),
            door.listener.connected(id),
            deadline,
        ));
    }
    let link = door.listener.open_game(game, routes);
    let mut cursors = [None; MAX_PLAYERS];
    for &seat in &seats {
        cursors[usize::from(seat)] = Some(SeatCursor::home(board, seat));
    }
    let round = BotRound {
        link,
        game,
        drivers,
        cursors,
        last: [None; MAX_PLAYERS],
        dropped_at: [None; MAX_PLAYERS],
        standing_in: [false; MAX_PLAYERS],
        names: (0..n).map(|seat| names.label(tr, seat as u8)).collect(),
        kinds: kinds.0[..n].to_vec(),
        clock: Clock {
            live: true,
            deadline_ms: LIVE_DEADLINE_MS,
            input_delay: 0,
        },
        log: Box::new(std::io::sink()),
    };
    for driver in &round.drivers {
        if driver.connected {
            let hello = protocol::hello(board, &round.table(), driver.seat, false);
            round.link.send(driver.id, &hello);
        }
    }
    bots.round = Some(round);
}

/// Leaving the arena: the round is over for the bots too.
pub fn end_round(mut bots: ResMut<BotSeats>) {
    bots.round = None;
}

/// Back at the menu: nobody is setting up a table, so the doorway closes
/// and its bots are let go.
pub fn close_door(mut bots: ResMut<BotSeats>) {
    *bots = BotSeats::default();
}

/// Tell the table what happened at a bot's seat.
pub fn announce(
    mut bots: ResMut<BotSeats>,
    names: Res<SeatNames>,
    settings: Res<crate::app::settings::GameSettings>,
    mut log: ResMut<crate::app::side_panels::EventLog>,
) {
    if bots.news.is_empty() {
        return;
    }
    let tr = settings.tr();
    for (seat, news) in std::mem::take(&mut bots.news) {
        let who = names.label(tr, seat);
        let line = match news {
            News::StandIn => fill(tr.bot_stand_in, &[("p", &who)]),
            News::Back => fill(tr.bot_back, &[("p", &who)]),
        };
        log.push(line, crate::app::palette::player_color(seat));
    }
}

// --- the doorway card ------------------------------------------------------

/// The doorway card: the one line each bot needs, a way to copy it, and
/// what has connected so far.
#[derive(Component)]
pub struct DoorCard;

/// One line of it, by its place on the card.
#[derive(Component)]
pub struct DoorLine(usize);

/// Lines the card holds: a heading, the instruction, a chair a seat, the
/// feedback, the keys.
const DOOR_LINES: usize = 3 + MAX_PLAYERS + 1;

/// What the card says, line by line, as it stands.
fn door_lines(bots: &BotSeats, tr: &crate::app::i18n::Tr, beach: &str) -> Vec<(String, Color)> {
    let mut lines = vec![(String::new(), Color::NONE); DOOR_LINES];
    let (Some(waiting), Some(door)) = (&bots.waiting, &bots.door) else {
        return lines;
    };
    let title = match waiting {
        Waiting::Couch => tr.door_title_table.to_string(),
        Waiting::Join => fill(tr.door_title_join, &[("b", beach)]),
    };
    lines[0] = (title, crate::app::palette::GOLD);
    lines[1] = (
        tr.door_start_with.to_string(),
        crate::app::palette::PARCHMENT,
    );
    for (i, chair) in door.chairs.iter().take(MAX_PLAYERS).enumerate() {
        let who = crate::app::seat_label(tr, chair.seat);
        let line = match door.bot_line(chair) {
            Some((name, rtt)) => {
                let ms = rtt.map_or("-".to_string(), |d| {
                    format!("{:.1}", d.as_secs_f64() * 1000.0)
                });
                format!(
                    "{who}  {}",
                    fill(tr.door_connected, &[("n", &name), ("ms", &ms)])
                )
            }
            None => format!("{who}  {}   {}", door.string(chair), tr.door_waiting),
        };
        let ink = match chair.bot {
            Some(_) => crate::app::palette::player_color(chair.seat).lighter(0.2),
            None => Color::WHITE,
        };
        lines[2 + i] = (line, ink);
    }
    lines[2 + MAX_PLAYERS] = (
        bots.feedback.clone(),
        crate::app::palette::PARCHMENT.with_alpha(0.8),
    );
    let keys = match door.showing_lan {
        true => tr.door_prompt_lan,
        false => tr.door_prompt,
    };
    lines[DOOR_LINES - 1] = (
        keys.to_string(),
        crate::app::palette::PARCHMENT.with_alpha(0.7),
    );
    lines
}

/// Put the card up while a doorway is waiting, keep its lines current,
/// and take it down when the wait is over.
pub fn door_card(
    mut commands: Commands,
    bots: Res<BotSeats>,
    settings: Res<crate::app::settings::GameSettings>,
    lobby: Res<crate::app::lobby::LobbyState>,
    cards: Query<Entity, With<DoorCard>>,
    mut lines: Query<(&DoorLine, &mut Text, &mut TextColor, &mut Node)>,
) {
    let up = bots.waiting.is_some() && bots.door.is_some();
    if !up {
        for card in &cards {
            commands.entity(card).despawn();
        }
        return;
    }
    if cards.is_empty() {
        commands
            .spawn((
                DoorCard,
                crate::app::menu_ui::centred_overlay(),
                BackgroundColor(Color::BLACK.with_alpha(0.55)),
                GlobalZIndex(40),
            ))
            .with_children(|wrap| {
                wrap.spawn(crate::app::menu_ui::screen_card())
                    .with_children(|card| {
                        for i in 0..DOOR_LINES {
                            card.spawn((
                                DoorLine(i),
                                Text::new(""),
                                if i == 0 {
                                    crate::app::menu_ui::display_font(
                                        crate::app::menu_ui::type_scale::DISPLAY * 0.7,
                                    )
                                } else {
                                    TextFont {
                                        font_size: FontSize::Px(
                                            crate::app::menu_ui::type_scale::BODY,
                                        ),
                                        ..default()
                                    }
                                },
                                TextColor(Color::WHITE),
                                Node {
                                    align_self: AlignSelf::FlexStart,
                                    ..default()
                                },
                            ));
                        }
                    });
            });
        return;
    }
    let beach = lobby.joining_beach().unwrap_or_default();
    let said = door_lines(&bots, settings.tr(), &beach);
    for (line, mut text, mut ink, mut node) in &mut lines {
        let (words, color) = &said[line.0];
        crate::app::menu_ui::set_text(&mut text, words);
        crate::app::menu_ui::set_color(&mut ink, *color);
        crate::app::menu_ui::set_shown(&mut node, !words.is_empty());
    }
}

/// The card's keys: C copies the first string still waiting for its bot,
/// L shows the LAN's strings or this machine's, Esc backs out and gives
/// the keys up.
pub fn door_input(
    keyboard: crate::app::keycaps::Keyboard,
    mut bots: ResMut<BotSeats>,
    mut clipboard: ResMut<Clipboard>,
    settings: Res<crate::app::settings::GameSettings>,
) {
    let crate::app::keycaps::Keyboard { keys, caps } = keyboard;
    if bots.waiting.is_none() {
        return;
    }
    let tr = settings.tr();
    if keys.just_pressed(KeyCode::Escape) {
        bots.waiting = None;
        bots.feedback.clear();
        bots.door = None;
        return;
    }
    let BotSeats {
        door,
        waiting,
        feedback,
        ..
    } = &mut *bots;
    let Some(door) = door else {
        *waiting = None;
        return;
    };
    door.poll();
    if caps.just_pressed(&keys, 'C')
        && let Some(chair) = door.chairs.iter().find(|chair| chair.bot.is_none())
    {
        let string = door.string(chair);
        *feedback = match clipboard.set_text(string.clone()) {
            Ok(()) => fill(tr.door_copied, &[("s", &string)]),
            Err(e) => fill(tr.code_copy_failed, &[("e", &e.to_string())]),
        };
    }
    if caps.just_pressed(&keys, 'L')
        && let Err(e) = door.toggle_lan()
    {
        *feedback = fill(tr.door_could_not_listen, &[("e", &e.to_string())]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};

    /// Register with a chair's string, the way a bot does.
    fn register(string: &str, name: &str, owner: &str) -> std::net::TcpStream {
        let parsed = ConnString::parse(string).expect("a string the card printed");
        let mut stream =
            std::net::TcpStream::connect((parsed.host.as_str(), parsed.port)).expect("connect");
        let key = parsed.key.map(|k| k.to_string()).unwrap_or_default();
        let line = serde_json::json!({
            "type": "register", "protocol": 1, "name": name, "owner": owner, "key": key,
        });
        writeln!(stream, "{line}").expect("write");
        let mut answer = String::new();
        BufReader::new(stream.try_clone().expect("clone"))
            .read_line(&mut answer)
            .expect("read");
        assert!(answer.contains("registered"), "{answer}");
        stream
    }

    #[test]
    fn a_doorway_seats_each_bot_in_the_chair_its_key_was_printed_for() {
        let mut door = Doorway::open().expect("a port on this machine");
        door.seat_bots(&[2, 3]);
        assert_eq!(door.chairs.len(), 2);
        assert!(!door.ready());
        let for_three = door.string(&door.chairs[1]);
        let _bot = register(&for_three, "Greedy", "Ana");
        for _ in 0..200 {
            door.poll();
            if door.chairs[1].bot.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(door.chairs[1].seat, 3);
        assert!(
            door.chairs[1].bot.is_some(),
            "the bot sat where its key said"
        );
        assert!(door.chairs[0].bot.is_none());
        let tr = &crate::app::i18n::EN;
        assert_eq!(door.seat_name(tr, 3).as_deref(), Some("Greedy (Ana's bot)"));
        // A seat that stops wanting a bot gives its chair up; one that
        // keeps wanting one keeps its bot.
        door.seat_bots(&[3]);
        assert_eq!(door.chairs.len(), 1);
        assert!(door.ready());
    }
}
