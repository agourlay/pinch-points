//! The doorway: a listener on this machine for bots to register with, a
//! chair per seat that wants one, and the string each bot is started with.

use crate::app::i18n::fill;
use crate::bots::connstr::{ConnString, DEFAULT_PORT, Key};
use crate::bots::listener::{Admission, BotId, Config, Event, Invite, Listener};
use std::net::SocketAddr;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// What a bot that came in but was dealt no seat is told as it goes.
const NO_SEAT: &str = "the table filled up before this bot had a seat";

/// What a bot is told as it goes when the seat it sat in, or was started
/// for, stops being a bot's seat.
const SEAT_GONE: &str = "the seat this bot was started for is no longer a bot's seat";

/// How long a bot that dropped keeps its chair, for it to come back by its
/// token (or, started again, by its key and name) before the chair is
/// given up. A round keeps a dropped bot's seat for good, standing the AI
/// in after a grace; without this the card and the lobby, which poll
/// every frame, let it go the frame it dropped, and the token the
/// protocol tells a bot to come back with was already spent.
const COME_BACK_WITHIN: Duration = Duration::from_secs(10);

/// One seat waiting for a bot, or held by one.
pub struct Chair {
    /// Which chair: a number of its own, so a chair keeps its key and its
    /// bot while others come and go around it.
    id: u32,
    pub seat: u8,
    pub key: Key,
    pub bot: Option<BotId>,
    /// When its bot dropped, while it is away: the chair stays its for
    /// [`COME_BACK_WITHIN`].
    away_since: Option<Instant>,
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
    kind: DoorKind,
    /// [`COME_BACK_WITHIN`], shortened by the tests that watch a chair
    /// given up.
    come_back_within: Duration,
}

/// What a doorway's chairs are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DoorKind {
    /// Each chair is a seat's (a couch's bot seats, a bot joining a
    /// party): a bot that goes leaves its seat wanting a bot, with a
    /// fresh key.
    Seats,
    /// A host's door (route 2): each chair is the next of however many
    /// come, and one that goes is gone. It listens on the LAN and nowhere
    /// else.
    Host,
}

impl Drop for Doorway {
    fn drop(&mut self) {
        self.listener.close();
    }
}

impl Doorway {
    /// The listener the doorway's bots are registered with.
    pub(super) fn listener(&self) -> &Listener {
        &self.listener
    }

    /// Where it listens for this machine.
    #[cfg(test)]
    pub(super) fn here(&self) -> SocketAddr {
        self.here
    }

    /// A doorway on this machine: the bots' own port if it is free, any
    /// port if not (a second game on one machine, an arena running).
    pub fn open() -> std::io::Result<Doorway> {
        Doorway::bind([127, 0, 0, 1], DoorKind::Seats)
    }

    /// A doorway on the LAN, for bots straight to a host (route 2): the
    /// bots' own port on every interface if it is free, any port if not.
    /// The key is the lock here, a single-use one per chair.
    pub fn open_lan() -> std::io::Result<Doorway> {
        Doorway::bind([0, 0, 0, 0], DoorKind::Host)
    }

    /// A host's door on this machine alone, for the tests.
    #[cfg(test)]
    pub(super) fn open_host_here() -> std::io::Result<Doorway> {
        Doorway::bind([127, 0, 0, 1], DoorKind::Host)
    }

    /// A doorway of `kind` listening on `ip`, the bots' own port if it is
    /// free and any port if not. Every interface is the LAN, and its
    /// strings are the LAN's.
    fn bind(ip: [u8; 4], kind: DoorKind) -> std::io::Result<Doorway> {
        let lan = std::net::Ipv4Addr::from(ip).is_unspecified();
        let config = Config::new(Admission::Keys(Vec::new()));
        let (listener, events) =
            Listener::bind(SocketAddr::from((ip, DEFAULT_PORT)), config.clone())
                .or_else(|_| Listener::bind(SocketAddr::from((ip, 0)), config))?;
        let here = listener.local_addr();
        Ok(Doorway {
            listener,
            events: std::sync::Mutex::new(events),
            here,
            lan: lan.then_some(here),
            showing_lan: lan,
            chairs: Vec::new(),
            next_chair: 0,
            kind,
            come_back_within: COME_BACK_WITHIN,
        })
    }

    /// Keep one chair open for the next bot while there is room for it:
    /// once a bot has taken the open one, a fresh key is drawn for the next,
    /// so a key read off the screen is worth nothing once it is spent.
    /// With no room the open chair is withdrawn, key and all, so the
    /// string stops being shown and a bot started with it is refused
    /// rather than registered to a seat that is not there.
    pub fn keep_a_chair_open(&mut self, room: bool) {
        if !room {
            for chair in self.chairs.iter().filter(|chair| chair.bot.is_none()) {
                self.listener.withdraw(chair.id);
            }
            self.chairs.retain(|chair| chair.bot.is_some());
            return;
        }
        let open = self.chairs.iter().any(|chair| chair.bot.is_none());
        if !open {
            let id = self.next_chair;
            self.next_chair += 1;
            let chair = self.keyed_chair(id, self.chairs.len() as u8);
            self.chairs.push(chair);
        }
    }

    /// Chair `id` for `seat`, empty, with a fresh single-use key let in.
    fn keyed_chair(&self, id: u32, seat: u8) -> Chair {
        let key = match (id, crate::app::dev::bot_key()) {
            (0, Some(key)) => key,
            _ => Key::draw(),
        };
        self.listener.invite(Invite {
            key: key.clone(),
            uses: Some(1),
            owner: None,
            slot: Some(id),
        });
        Chair {
            id,
            seat,
            key,
            bot: None,
            away_since: None,
        }
    }

    /// The string for the chair still waiting for its bot, if one is.
    pub fn open_string(&self) -> Option<String> {
        self.chairs
            .iter()
            .find(|chair| chair.bot.is_none())
            .map(|chair| self.string(chair))
    }

    /// The bots that have come in, in the order they came: each with what
    /// its seat is called, from the name and the owner it declared. The
    /// host cannot check the owner, which the design says out loud.
    pub fn arrived(&self, tr: &crate::app::i18n::Tr) -> Vec<(BotId, String)> {
        self.chairs
            .iter()
            .filter_map(|chair| {
                // Only those still here: one gone is not seated.
                let id = chair.bot.filter(|&id| self.listener.connected(id))?;
                let info = self.listener.bot(id)?;
                Some((info.id, bot_name(tr, &info.name, info.owner.as_deref())))
            })
            .collect()
    }

    /// Ask a bot to go: its chair is given up and its connection closed.
    pub fn let_go(&mut self, id: BotId) {
        self.chairs.retain(|chair| chair.bot != Some(id));
        self.listener.disconnect(id);
    }

    /// A host's round has been dealt, with these of its bots seated
    /// (route 2). Every other bot that came in is told it has no seat and
    /// let go: the people and the bots before it took the chairs, and left
    /// registered it would hear nothing but pings. The open chair is
    /// withdrawn too, since nobody is dealt in until the table is back in
    /// the lobby, which opens one again.
    pub fn seat_the_table(&mut self, seated: &[BotId]) {
        let unseated: Vec<BotId> = self
            .chairs
            .iter()
            .filter_map(|chair| chair.bot)
            .filter(|id| !seated.contains(id))
            .collect();
        for id in unseated {
            self.chairs.retain(|chair| chair.bot != Some(id));
            self.listener.dismiss(id, NO_SEAT);
        }
        self.keep_a_chair_open(false);
    }

    /// Chairs for exactly these seats: a chair already holding a bot for a
    /// seat still wanting one keeps it, and a seat new to the list gets a
    /// fresh, single-use key. A chair whose seat is off the list goes with
    /// its key, so a bot started with that string is refused, and the bot
    /// in it, if one sat there, is told why and let go rather than left
    /// registered to hear nothing but pings.
    pub fn seat_bots(&mut self, seats: &[u8]) {
        let (kept, gone): (Vec<Chair>, Vec<Chair>) = std::mem::take(&mut self.chairs)
            .into_iter()
            .partition(|chair| seats.contains(&chair.seat));
        self.chairs = kept;
        for chair in gone {
            self.listener.withdraw(chair.id);
            if let Some(id) = chair.bot {
                self.listener.dismiss(id, SEAT_GONE);
            }
        }
        for &seat in seats {
            if self.chairs.iter().any(|chair| chair.seat == seat) {
                continue;
            }
            let id = self.next_chair;
            self.next_chair += 1;
            let chair = self.keyed_chair(id, seat);
            self.chairs.push(chair);
        }
        self.chairs.sort_by_key(|chair| chair.seat);
    }

    /// Take what the listener heard: each registration sits its bot in the
    /// chair its key was printed for, and a bot that went keeps its chair
    /// for [`COME_BACK_WITHIN`], to come back by its token or, started
    /// again, by its string and name. Past that it gives the chair up,
    /// which finishes with it: started again it registers afresh. A seat's
    /// chair gets a fresh key for the next bot; at a host's door the chair
    /// goes, and the next bot takes the one kept open.
    pub fn poll(&mut self) {
        let heard: Vec<Event> = {
            let events = self
                .events
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::iter::from_fn(|| events.try_recv().ok()).collect()
        };
        for event in heard {
            match event {
                Event::Registered(id) => {
                    let slot = self.listener.bot(id).and_then(|info| info.slot);
                    match self
                        .chairs
                        .iter_mut()
                        .find(|c| Some(c.id) == slot && c.bot.is_none())
                    {
                        Some(chair) => chair.bot = Some(id),
                        // Its chair was withdrawn after it registered and
                        // before this heard so: told so and let go, rather
                        // than left registered to hear nothing but pings.
                        // A seat's chair goes when the seat stops being a
                        // bot's; any other when the table fills or a round
                        // is dealt.
                        None => self.listener.dismiss(
                            id,
                            match self.kind {
                                DoorKind::Seats => SEAT_GONE,
                                DoorKind::Host => NO_SEAT,
                            },
                        ),
                    }
                }
                // Read late: a bot that has come back since keeps its chair.
                Event::Dropped(id) if !self.listener.connected(id) => {
                    if let Some(chair) = self.chairs.iter_mut().find(|c| c.bot == Some(id)) {
                        chair.away_since.get_or_insert_with(Instant::now);
                    }
                }
                Event::Dropped(_) | Event::Reconnected(_) | Event::Refused(..) => {}
            }
        }
        self.give_up_the_gone();
    }

    /// Settle the chairs whose bots dropped: one back is back, and one
    /// away past [`COME_BACK_WITHIN`] is let go. A seat's chair then gets
    /// a fresh key for the next bot; at a host's door the chair goes.
    fn give_up_the_gone(&mut self) {
        let mut at = 0;
        while at < self.chairs.len() {
            let chair = &mut self.chairs[at];
            let (Some(id), Some(since)) = (chair.bot, chair.away_since) else {
                at += 1;
                continue;
            };
            if self.listener.connected(id) {
                chair.away_since = None;
            } else if since.elapsed() >= self.come_back_within {
                self.listener.retire(id);
                match self.kind {
                    DoorKind::Seats => {
                        let (chair, seat) = (chair.id, chair.seat);
                        self.chairs[at] = self.keyed_chair(chair, seat);
                    }
                    DoorKind::Host => {
                        self.chairs.remove(at);
                        continue;
                    }
                }
            }
            at += 1;
        }
    }

    /// Every chair has its bot, there now: one that dropped holds its
    /// chair for a while, but plays nothing until it is back.
    pub fn ready(&self) -> bool {
        self.chairs
            .iter()
            .all(|chair| chair.bot.is_some_and(|id| self.listener.connected(id)))
    }

    /// Show the strings on the LAN address rather than this machine's,
    /// listening there too from the first time it is asked for.
    /// Turned back, the LAN socket closes again: a card showing this
    /// machine's strings is not still taking bots from the network.
    pub fn toggle_lan(&mut self) -> std::io::Result<()> {
        match (self.kind, self.lan) {
            // A host's doorway listens on the LAN and nowhere else.
            (DoorKind::Host, _) => {}
            (DoorKind::Seats, Some(lan)) => {
                self.listener.stop_listening(lan);
                self.lan = None;
            }
            (DoorKind::Seats, None) => {
                self.lan = Some(
                    self.listener
                        .also_listen(SocketAddr::from(([0, 0, 0, 0], 0)))?,
                );
            }
        }
        self.showing_lan = self.lan.is_some();
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
    /// it is the build they meant to start, and whether it is there now.
    /// A bot that dropped holds its chair for [`COME_BACK_WITHIN`], and
    /// read as connected through that, its card showed a stale round trip
    /// while the start was refused for want of it.
    pub fn bot_line(&self, chair: &Chair) -> Option<BotLine> {
        let info = self.listener.bot(chair.bot?)?;
        let name = match info.version.is_empty() {
            true => info.name,
            false => format!("{} {}", info.name, info.version),
        };
        Some(match self.listener.connected(info.id) {
            true => BotLine::Here {
                name,
                rtt: self.listener.rtt(info.id),
            },
            false => BotLine::Away { name },
        })
    }

    /// The seat name for a bot joining a party from this machine (route
    /// 1): its owner is whoever is at this machine, not what it declared.
    pub fn named_for(&self, tr: &crate::app::i18n::Tr, owner: &str) -> Option<String> {
        let info = self.listener.bot(self.chairs.first()?.bot?)?;
        let owner = Some(owner.trim()).filter(|owner| !owner.is_empty());
        Some(bot_name(tr, &info.name, owner))
    }

    /// What the seat a bot holds is called: its registered name and its
    /// owner's, "Greedy (Ana)", or its name alone if it gave none. The
    /// robot beside it on every screen says it is a bot.
    pub fn seat_name(&self, tr: &crate::app::i18n::Tr, seat: u8) -> Option<String> {
        let chair = self.chairs.iter().find(|chair| chair.seat == seat)?;
        let info = self.listener.bot(chair.bot?)?;
        Some(bot_name(tr, &info.name, info.owner.as_deref()))
    }
}

/// What the card says of a chair's bot: there, with its round trip, or
/// dropped and holding the chair while it may come back.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BotLine {
    Here { name: String, rtt: Option<Duration> },
    Away { name: String },
}

/// A bot's seat name: "Greedy (Ana)", or "Greedy" with no owner.
fn bot_name(tr: &crate::app::i18n::Tr, name: &str, owner: Option<&str>) -> String {
    match owner {
        Some(owner) => fill(tr.bot_owned, &[("bot", name), ("o", owner)]),
        None => fill(tr.bot_unowned, &[("bot", name)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::bot_seats::test_bots::{TestBot, poll_until, register, seated, try_register};
    use std::io::{BufRead, BufReader};

    /// A full table withdraws its open chair: the string stops being
    /// shown, and a bot started with it anyway is refused rather than
    /// registered to a seat that is not there.
    #[test]
    fn a_full_table_withdraws_its_open_chair() {
        let mut door = Doorway::open_host_here().expect("a port on this machine");
        door.keep_a_chair_open(true);
        let string = door.open_string().expect("a chair is open");
        door.keep_a_chair_open(false);
        assert_eq!(door.open_string(), None, "no string is shown");
        assert!(door.chairs.is_empty());
        let answer = try_register(&string, "Late");
        assert!(answer.contains("error"), "refused: {answer}");
        // Room again: a fresh chair, with a key of its own.
        door.keep_a_chair_open(true);
        let fresh = door.open_string().expect("a chair is open again");
        assert_ne!(fresh, string, "the old key is not reissued");
    }

    /// A bot that registered just before its chair was withdrawn, and
    /// before the doorway heard of it, is told it has no seat and let go,
    /// rather than left registered to a chair that is not there.
    #[test]
    fn a_bot_whose_chair_went_as_it_came_in_is_told() {
        let mut door = Doorway::open_host_here().expect("a port on this machine");
        door.keep_a_chair_open(true);
        let bot = register(&door.open_string().expect("open"), "Just Late", "Ana");
        // The table fills before the registration is heard.
        door.keep_a_chair_open(false);
        // Its registration is heard a moment after it was answered.
        for _ in 0..60 {
            door.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(door.chairs.is_empty(), "no chair came back for it");
        bot.set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        let mut said = String::new();
        let mut reader = BufReader::new(bot);
        while reader.read_line(&mut said).is_ok_and(|n| n > 0) {
            if said.contains(NO_SEAT) {
                break;
            }
        }
        assert!(said.contains(NO_SEAT), "told why: {said}");
    }

    /// A bot that came in but was dealt no seat is told why and let go,
    /// and the chair a bot could still come in by is withdrawn while the
    /// round is played.
    #[test]
    fn a_bot_dealt_no_seat_is_told_and_let_go() {
        let mut door = Doorway::open_host_here().expect("a port on this machine");
        door.keep_a_chair_open(true);
        let first = register(&door.open_string().expect("open"), "First", "Ana");
        seated(&mut door, 0);
        door.keep_a_chair_open(true);
        let second = register(&door.open_string().expect("open"), "Second", "Bo");
        seated(&mut door, 1);
        door.keep_a_chair_open(true);
        let late = door.open_string().expect("a third chair is open");
        let tr = &crate::app::i18n::EN;
        let ids: Vec<BotId> = door.arrived(tr).into_iter().map(|(id, _)| id).collect();
        // The table had room for the first bot only.
        door.seat_the_table(&ids[..1]);
        assert_eq!(door.arrived(tr).len(), 1, "the second bot was let go");
        assert_eq!(door.open_string(), None, "and no chair is open mid-round");
        second
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        let mut said = String::new();
        let mut reader = BufReader::new(second);
        while reader.read_line(&mut said).is_ok_and(|n| n > 0) {
            if said.contains("error") {
                break;
            }
        }
        assert!(said.contains(NO_SEAT), "told why: {said}");
        let answer = try_register(&late, "Late");
        assert!(
            answer.contains("error"),
            "the open key was taken back: {answer}"
        );
        drop(first);
    }

    /// Opened to the LAN and turned back, the card stops listening there:
    /// showing this machine's strings again is not still taking bots from
    /// the network.
    #[test]
    fn turning_the_lan_off_closes_its_socket() {
        let mut door = Doorway::open().expect("a port on this machine");
        door.toggle_lan().expect("listen on the LAN");
        let lan = door.lan.expect("a LAN socket");
        assert!(door.showing_lan);
        let wake = SocketAddr::from(([127, 0, 0, 1], lan.port()));
        assert!(std::net::TcpStream::connect_timeout(&wake, Duration::from_millis(200)).is_ok());
        door.toggle_lan().expect("back to this machine");
        assert!(!door.showing_lan);
        let mut refused = false;
        for _ in 0..100 {
            if std::net::TcpStream::connect_timeout(&wake, Duration::from_millis(50)).is_err() {
                refused = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(refused, "nobody listens on {wake} any more");
        // And on again, on a socket of its own.
        door.toggle_lan().expect("listen on the LAN again");
        assert!(door.showing_lan);
        assert!(door.lan.is_some());
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
        assert_eq!(door.seat_name(tr, 3).as_deref(), Some("Greedy (Ana)"));
        // A seat that stops wanting a bot gives its chair up; one that
        // keeps wanting one keeps its bot.
        door.seat_bots(&[3]);
        assert_eq!(door.chairs.len(), 1);
        assert!(door.ready());
    }

    /// A seat that stops being a bot's takes its chair's key back, and lets
    /// the bot sitting there go with the reason, rather than leaving it
    /// registered to hear nothing but pings.
    #[test]
    fn a_seat_that_stops_wanting_a_bot_lets_its_bot_and_its_key_go() {
        let mut door = Doorway::open().expect("a port on this machine");
        door.seat_bots(&[2, 3]);
        let for_two = door.string(&door.chairs[0]);
        let bot = register(&door.string(&door.chairs[1]), "Greedy", "Ana");
        seated(&mut door, 1);
        door.seat_bots(&[1]);
        assert_eq!(door.chairs.len(), 1);
        assert_eq!(door.chairs[0].seat, 1);
        bot.set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        let mut said = String::new();
        let mut reader = BufReader::new(bot);
        while reader.read_line(&mut said).is_ok_and(|n| n > 0) {
            if said.contains("error") {
                break;
            }
        }
        assert!(said.contains(SEAT_GONE), "told why: {said}");
        let answer = try_register(&for_two, "Late");
        assert!(
            answer.contains("error") && !answer.contains(NO_SEAT),
            "the key for seat 2 was taken back: {answer}"
        );
    }

    /// A bot that goes before the match starts, and stays gone, gives its
    /// chair up: the card stops counting it, and its seat gets a fresh key.
    #[test]
    fn a_bot_that_goes_gives_its_chair_up() {
        let mut door = Doorway::open().expect("a port on this machine");
        door.come_back_within = Duration::ZERO;
        door.seat_bots(&[1]);
        let spent = door.string(&door.chairs[0]);
        let bot = TestBot::register(&spent);
        assert!(poll_until(&mut door, |d| d.ready()));
        drop(bot);
        // Not ready the moment the listener sees it go; the chair given up
        // when the poll reads the drop, a moment later.
        assert!(
            poll_until(&mut door, |d| d.chairs[0].bot.is_none()),
            "a dead bot kept its chair"
        );
        assert!(!door.ready());
        assert!(door.arrived(&crate::app::i18n::EN).is_empty());
        assert_eq!(door.chairs.len(), 1, "the seat still wants a bot");
        assert_ne!(door.string(&door.chairs[0]), spent, "a fresh key");
        // And a bot can take it with that key.
        let _next = TestBot::register(&door.string(&door.chairs[0]));
        assert!(poll_until(&mut door, |d| d.ready()));
    }

    /// The same at a host's door (route 2), where chairs are not seats:
    /// the dead bot's chair goes, and one chair stays open for the next.
    #[test]
    fn a_bot_that_leaves_a_hosts_door_is_not_seated() {
        let mut door = Doorway::open_host_here().expect("a port on this machine");
        door.come_back_within = Duration::ZERO;
        door.keep_a_chair_open(true);
        let bot = TestBot::register(&door.open_string().expect("an open chair"));
        assert!(poll_until(&mut door, |d| d
            .arrived(&crate::app::i18n::EN)
            .len()
            == 1));
        door.keep_a_chair_open(true);
        drop(bot);
        // `arrived` stops counting the bot the moment the listener sees it
        // go; its chair goes when the poll reads the drop, a moment later.
        let settled = |d: &Doorway| {
            d.arrived(&crate::app::i18n::EN).is_empty()
                && d.chairs.len() == 1
                && d.chairs[0].bot.is_none()
        };
        for _ in 0..400 {
            door.poll();
            door.keep_a_chair_open(true);
            if settled(&door) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            settled(&door),
            "a dead bot is still at the beach: {} chairs",
            door.chairs.len()
        );
    }

    /// A bot that drops while the card is up and is started again with the
    /// string it was first started with keeps its chair: the card has
    /// stopped counting it, then counts it again, on the same key. It used
    /// to be let go the frame it dropped, its key redrawn, and the restart
    /// refused as a wrong key.
    #[test]
    fn a_bot_started_again_comes_back_to_its_chair() {
        let mut door = Doorway::open().expect("a port on this machine");
        door.seat_bots(&[1]);
        let string = door.string(&door.chairs[0]);
        let bot = TestBot::register(&string);
        assert!(poll_until(&mut door, |d| d.ready()));
        drop(bot);
        assert!(poll_until(&mut door, |d| !d.ready()), "away, not ready");
        assert!(door.chairs[0].bot.is_some(), "but the chair is still its");
        // And the card says so, rather than a round trip from before.
        assert!(
            matches!(door.bot_line(&door.chairs[0]), Some(BotLine::Away { .. })),
            "{:?}",
            door.bot_line(&door.chairs[0])
        );
        let _again = TestBot::register(&string);
        assert!(poll_until(&mut door, |d| d.ready()), "back in its chair");
        assert!(matches!(
            door.bot_line(&door.chairs[0]),
            Some(BotLine::Here { .. })
        ));
        assert_eq!(door.string(&door.chairs[0]), string, "on the same key");
    }

    /// A spent key is a bot started again too late, not somebody trying
    /// keys: refused, but no strike against the address, so the fresh key
    /// on screen still lets this machine in after any number of them.
    #[test]
    fn a_spent_key_is_no_strike() {
        let mut door = Doorway::open().expect("a port on this machine");
        door.come_back_within = Duration::ZERO;
        door.seat_bots(&[1]);
        let spent = door.string(&door.chairs[0]);
        drop(TestBot::register(&spent));
        assert!(poll_until(&mut door, |d| d.string(&d.chairs[0]) != spent));
        for _ in 0..5 {
            let answer = try_register(&spent, "Other");
            assert!(answer.contains("spent"), "{answer}");
        }
        let _fresh = TestBot::register(&door.string(&door.chairs[0]));
        assert!(poll_until(&mut door, |d| d.ready()));
    }
}
