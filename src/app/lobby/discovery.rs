//! Hearing the hall: which beaches are on the air, in what order, and
//! which of them a cursor is pointing at.
//!
//! A beach leaves the list two ways: it says so, or it stops being heard.
//! The timeout underneath the farewell is for the exits that cannot be
//! announced.

use super::*;

/// How long a beach may go unheard before the list gives up on it, on a
/// listener that hears every one of its beacons: five of them lost in a
/// row. See [`host_ttl`] for a listener that hears fewer.
pub(super) const HOST_TTL: f32 = 5.0;

/// How long a beach may go unheard on a listener that hears one of its
/// beacons in `one_in` ([`crate::transport::beacons_heard_one_in`]): the
/// same five beacons lost in a row, counted in the ones that reach it.
///
/// A listener past the first lobby port hears a beach on another machine
/// only every fourth second, and given the first port's five it dropped
/// the beach at the first wide beacon lost and listed it again on the
/// next: a row that came and went under the player's cursor. A beach that
/// leaves properly is gone at once all the same, since its farewell goes
/// to every port; this is only how long a crashed one lingers.
pub(super) fn host_ttl(one_in: u32) -> f32 {
    HOST_TTL * one_in as f32
}

/// The most beaches the list holds at once.
///
/// A row is made for every beacon id heard, and an id is whatever the
/// datagram says: one sender making up a fresh one per packet grew the
/// list without end, and with it the sort the list is put through every
/// frame. Sixty-four is more beaches than any hall puts up, and far more
/// than the screen shows; past it a new id takes the place of the least
/// established row ([`the_row_to_give_up`]), never the one being played at
/// or pointed at.
pub(super) const MAX_BEACHES: usize = 64;

/// How long a peer may go unheard before the lobby gives up on it. Four
/// greetings' worth: long enough to ride out a loss burst, short enough
/// that a table showing five players has five players at it.
pub(super) const PEER_TTL: f32 = 4.0;

/// The three timers have to stay in this order, and it is cheaper to say so
/// to the compiler than to test it: a peer must outlive a burst of lost
/// greetings, and must be given up before the beach it is sitting at is.
const _: () = {
    assert!(PEER_TTL > 3.0 * ANNOUNCE_EVERY);
    assert!(PEER_TTL < HOST_TTL);
};

pub struct HostEntry {
    pub addr: SocketAddr,
    /// Who announced it, as opposed to from where; see [`Beacon::id`].
    /// Zero from a build that announced none, which is keyed by address.
    pub id: u64,
    /// What the host calls itself, from its beacon. Empty from a build that
    /// announced no name, or a host that never typed one. The list falls
    /// back to the address, as it always did.
    pub name: String,
    /// The player who put the beach up, from the same beacon. Empty from a
    /// build that announced none. "Room 3" is a good name for a game and
    /// says nothing about whose it is.
    pub host: String,
    /// The table as its host last described it. `seats` is 0 from a build
    /// that announced no occupancy, which the list reads as "unknown"
    /// rather than "full".
    pub taken: u8,
    pub seats: u8,
    /// The round has begun: not joinable now, but queueable if there is
    /// room, and the queue is admitted when the round ends.
    pub running: bool,
    /// It takes bots ("Bots welcome").
    pub bots: bool,
    /// Its socket has no room for anyone, watchers included.
    pub full: bool,
    pub age: f32,
    /// How many of its beacons this listener has heard. A real beach
    /// speaks every second for as long as it is up, and an id made up for
    /// one packet speaks once, which is what a full list goes by when it
    /// has to give a row up.
    pub heard: u32,
}

impl HostEntry {
    /// Whether joining (or queueing for) this beach could come to anything.
    pub fn has_room(&self) -> bool {
        !self.full && (self.seats == 0 || self.taken < self.seats)
    }

    /// What the beach is called. One that announced no name falls back to
    /// its host's, and one that announced neither to its address, which has
    /// a column of its own.
    pub fn who(&self) -> String {
        match (self.name.is_empty(), self.host.is_empty()) {
            (false, _) => self.name.clone(),
            (true, false) => self.host.clone(),
            (true, true) => self.addr.to_string(),
        }
    }

    /// Who put this beach up. Empty from a build that announced nobody, and
    /// from a beach listed under that host's name already, since repeating
    /// it in the next column along says nothing twice.
    pub fn creator(&self) -> &str {
        match self.name.is_empty() {
            true => "",
            false => &self.host,
        }
    }

    /// The right-hand column: how full, and what pressing this row would
    /// actually do. A beach that described no table gets no count invented
    /// for it.
    pub fn table(&self, tr: &crate::app::i18n::Tr) -> String {
        match (self.seats, self.running, self.has_room()) {
            (0, ..) => String::new(),
            (seats, false, _) => format!("{}/{seats}", self.taken),
            (seats, true, true) => format!("{}/{seats}  {}", self.taken, tr.lobby_in_progress),
            (seats, true, false) => format!("{}/{seats}  {}", self.taken, tr.lobby_full_tag),
        }
    }

    /// Its colour, which says the same thing again for anyone reading the
    /// shape of the list rather than the words: gold is open, tide-blue is
    /// playing but has a chair, grey is neither.
    pub fn table_tone(&self) -> Color {
        match (self.running, self.has_room()) {
            (_, false) => palette::IDLE_ROW.darker(0.15),
            (false, true) => palette::GOLD,
            (true, true) => palette::INK_TIDE,
        }
    }
}

/// Fold freshly heard beacons into the host list: a host we can still hear
/// has its age reset, a new one joins the list, one that said goodbye goes
/// at once, and one that has gone quiet for `ttl` ([`host_ttl`]) drops
/// off: a host that quit should not sit in the join list forever.
///
/// The timeout is the backstop, not the mechanism: a host that leaves
/// properly says so and is gone within the frame, and the ageing is for a
/// killed process, a pulled cable, a laptop lid.
///
/// `held` are the rows a full list never gives up to a new id: the beach
/// this lobby is at, and the one under the cursor, by the address their
/// rows go by.
///
/// Pure, so both rules are testable without sockets.
pub(super) fn refresh_hosts(
    hosts: &mut Vec<HostEntry>,
    heard: &[(SocketAddr, Beacon)],
    delta: f32,
    ttl: f32,
    held: &[SocketAddr],
) {
    for entry in hosts.iter_mut() {
        entry.age += delta;
    }
    // In arrival order, so a beach that opened and closed inside one frame
    // ends up closed rather than listed.
    for (addr, beacon) in heard {
        let addr = *addr;
        match beacon {
            Beacon::Closing { id } => hosts.retain(|host| !same_beach(host, *id, addr)),
            // Everything the beacon carries is refreshed along with the
            // age: a beach fills up, empties and starts its round while the
            // list is watching.
            Beacon::Here {
                id,
                name,
                host,
                taken,
                seats,
                running,
                bots,
                full,
            } => {
                let mut fresh = HostEntry {
                    addr,
                    id: *id,
                    name: name.clone(),
                    host: host.clone(),
                    taken: *taken,
                    seats: *seats,
                    running: *running,
                    bots: *bots,
                    full: *full,
                    age: 0.0,
                    heard: 1,
                };
                let room = hosts.len() < MAX_BEACHES;
                match hosts.iter_mut().find(|host| same_beach(host, *id, addr)) {
                    Some(entry) => {
                        // Keep the address it was first heard from. The same
                        // beach arrives from two of them, the broadcast and
                        // the loopback copy, and swapping between them every
                        // second would be churn for no gain: either reaches it.
                        fresh.addr = entry.addr;
                        fresh.heard = entry.heard.saturating_add(1);
                        *entry = fresh;
                    }
                    None if room => hosts.push(fresh),
                    None => {
                        if let Some(at) = the_row_to_give_up(hosts, held) {
                            hosts[at] = fresh;
                        }
                    }
                }
            }
        }
    }
    hosts.retain(|host| host.age < ttl);
    // A stable order, so the list does not reshuffle under a player's
    // finger as beaches come and go. By name first, because that is what
    // the list shows; by address to break ties, because two children will
    // both call their beach "Sam".
    hosts.sort_by(|a, b| {
        fold_case(&a.name)
            .cmp(fold_case(&b.name))
            .then_with(|| (a.addr.ip(), a.addr.port()).cmp(&(b.addr.ip(), b.addr.port())))
    });
}

/// Which row a full list gives up to a new id, if any may go: one heard
/// only once, the one heard from longest ago. Never one in `held`, and
/// never a beach heard more than once.
///
/// Not simply the stalest. One sender making up a fresh id per packet has
/// every one of its rows at age zero, while a real beach is heard once a
/// second, or once every few up the port ladder, so the stalest is always
/// a real beach. A made-up id is heard once.
///
/// And not the fewest heard of all: with the list full of real beaches
/// that is a real beach, whose row a newcomer would take and lose back at
/// its next beacon, two rows swapping under the cursor every second. A
/// beach past a full list of real ones is listed once one of them leaves.
pub(super) fn the_row_to_give_up(hosts: &[HostEntry], held: &[SocketAddr]) -> Option<usize> {
    hosts
        .iter()
        .enumerate()
        .filter(|(_, host)| !held.contains(&host.addr) && host.heard <= 1)
        .min_by(|(_, a), (_, b)| a.heard.cmp(&b.heard).then(b.age.total_cmp(&a.age)))
        .map(|(at, _)| at)
}

/// Whether a beacon names a beach already in the list.
///
/// By the host's own id where there is one, because one beach reaches a
/// listener from two addresses, the broadcast copy and the loopback copy,
/// and keying by address listed every same-machine game twice. Only a
/// build that announces no id falls back to the address.
pub(super) fn same_beach(host: &HostEntry, id: u64, addr: SocketAddr) -> bool {
    match id {
        0 => host.id == 0 && host.addr == addr,
        id => host.id == id,
    }
}

/// Case-insensitive order over a name, without building a lowercased copy
/// of it to get there.
///
/// A comparator on a list of `n` runs `n log n` times every frame, because
/// the list is refreshed every frame, and `to_lowercase().cmp(&to_lowercase())`
/// allocates twice each time: fifty thousand allocations a second at forty
/// beaches. The address is compared as `(ip, port)` for the same reason,
/// `SocketAddr` having no `Ord`.
fn fold_case(name: &str) -> impl Iterator<Item = char> + '_ {
    name.chars().flat_map(char::to_lowercase)
}

/// Refresh the discovered-host list. Does nothing while hosting, which is
/// when the listening socket is handed back.
pub fn discover(time: Res<Time>, settings: Res<GameSettings>, mut state: ResMut<LobbyState>) {
    let delta = time.delta_secs();
    let heard = state
        .discovery
        .as_mut()
        .map(Discovery::poll)
        .unwrap_or_default();
    let closed = host_closed(&state, &heard);
    let ttl = state
        .discovery
        .as_ref()
        .and_then(Discovery::port)
        .map_or(HOST_TTL, |port| {
            host_ttl(crate::transport::beacons_heard_one_in(port))
        });
    let held: Vec<SocketAddr> = state
        .joined_entry()
        .map(|host| host.addr)
        .into_iter()
        .chain(state.selected)
        .collect();
    refresh_hosts(&mut state.hosts, &heard, delta, ttl, &held);
    learn_the_beach(&mut state);
    if closed {
        state.let_go();
        state.feedback = settings.tr().lobby_host_closed.to_string();
    }
    state.settle_cursor();
}

/// A joiner that dialled by address learns its beach's beacon id from the
/// row at that address, the first time there is one, and finds the row by
/// the id from then on (see `Joined::beach`).
fn learn_the_beach(state: &mut LobbyState) {
    let Some(joined) = state.joined() else {
        return;
    };
    if joined.beach.is_some() {
        return;
    }
    let Some(id) = state
        .joined_entry()
        .map(|host| host.id)
        .filter(|&id| id != 0)
    else {
        return;
    };
    if let Some(joined) = state.joined_mut() {
        joined.beach = Some(id);
    }
}

/// [`refresh_hosts`] for a lobby holding no row, at the usual
/// [`HOST_TTL`]: what most tests of the list ask.
#[cfg(test)]
pub(super) fn refresh_open(hosts: &mut Vec<HostEntry>, heard: &[(SocketAddr, Beacon)], delta: f32) {
    refresh_hosts(hosts, heard, delta, HOST_TTL, &[]);
}

/// Whether the beach this joiner is waiting at just said goodbye on the
/// air. The beacon is the one word a host leaving the lobby sends; without
/// reading it here the joiner sat out the six seconds of silence and was
/// then told to check an address that had answered all along.
fn host_closed(state: &LobbyState, heard: &[(SocketAddr, Beacon)]) -> bool {
    let Some(beach) = state.joined_entry() else {
        return false;
    };
    heard.iter().any(
        |(addr, beacon)| matches!(beacon, Beacon::Closing { id } if same_beach(beach, *id, *addr)),
    )
}

/// A browser's arrows walk the beach list, and a number key puts the
/// cursor on that row; Enter takes the one under the cursor.
///
/// A number only points, never joins: the list is sorted by name, so a
/// beach coming on the air above the one meant moves every number down,
/// and "press 3" shouted across a room would join whichever beach had
/// slid into row three. Pointed at, the beach's name is under the cursor to be
/// read before Enter, and the cursor then follows that beach, not the row.
///
/// Arrows only, not the W/S pairing the other menus also accept: W is the
/// watch toggle here, and has been since before there was a list to walk.
pub(super) fn walk_the_list(keys: &ButtonInput<KeyCode>, state: &mut LobbyState) {
    if state.standing().at_a_beach() {
        return;
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        state.step_cursor(Nav::Up);
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        state.step_cursor(Nav::Down);
    }
    let digit = crate::app::menu_ui::number_pressed(keys, crate::app::menu_ui::NUMBER_KEYS.len());
    state.point_at(digit);
}

#[cfg(test)]
mod list_tests {
    use super::*;
    use crate::app::i18n::EN;

    /// A number key points at the row it names and joins nothing: the
    /// cursor then follows that beach, so one sorting in above it moves
    /// the row and not the choice, and a number past the list points at
    /// nothing.
    #[test]
    fn a_number_points_at_a_row_and_the_cursor_keeps_its_beach() {
        let mut state = LobbyState::default();
        refresh_open(
            &mut state.hosts,
            &[
                beach(1, "Cove", 1, 6, false),
                beach(2, "Dunes", 1, 6, false),
            ],
            0.0,
        );
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Digit2);
        walk_the_list(&keys, &mut state);
        assert!(state.joined().is_none(), "pointing is not joining");
        let dunes = state.hosts[1].addr;
        assert_eq!(state.selected, Some(dunes));
        refresh_open(&mut state.hosts, &[beach(3, "Atoll", 1, 6, false)], 0.0);
        state.settle_cursor();
        assert_eq!(state.selected, Some(dunes), "still on Dunes");
        assert_eq!(state.selected_index(), Some(2), "a row further down");
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Digit9);
        walk_the_list(&keys, &mut state);
        assert_eq!(state.selected, Some(dunes), "nine of three is no row");
    }

    /// A beach on the air, as `refresh_hosts` hears it. Hosted by "Sam"
    /// throughout: whose beach it is matters to the list, never to the
    /// bookkeeping under it, so only the tests about the columns say.
    fn beach(last: u8, name: &str, taken: u8, seats: u8, running: bool) -> (SocketAddr, Beacon) {
        (
            format!("10.0.0.{last}:47777").parse().expect("addr"),
            Beacon::Here {
                id: u64::from(last),
                name: name.to_string(),
                host: "Sam".to_string(),
                taken,
                seats,
                running,
                bots: false,
                full: false,
            },
        )
    }

    fn entry(last: u8, name: &str, taken: u8, seats: u8, running: bool) -> HostEntry {
        hosted_entry(last, name, "Sam", taken, seats, running)
    }

    fn hosted_entry(
        last: u8,
        name: &str,
        host: &str,
        taken: u8,
        seats: u8,
        running: bool,
    ) -> HostEntry {
        HostEntry {
            addr: format!("10.0.0.{last}:47777").parse().expect("addr"),
            id: u64::from(last),
            name: name.to_string(),
            host: host.to_string(),
            taken,
            seats,
            running,
            bots: false,
            full: false,
            age: 0.0,
            heard: 1,
        }
    }

    /// What a row says, in both its columns, and what colour says it
    /// again for anyone reading the shape of the list rather than the
    /// words.
    #[test]
    fn a_row_says_whose_beach_and_whether_there_is_a_way_in() {
        let open = entry(1, "Anna", 2, 6, false);
        assert_eq!(open.who(), "Anna");
        assert_eq!(open.table(&EN), "2/6");
        assert_eq!(open.table_tone(), palette::GOLD, "open for anyone");

        let playing = entry(1, "Anna", 4, 6, true);
        assert!(playing.table(&EN).contains(EN.lobby_in_progress));
        assert_eq!(playing.table_tone(), palette::INK_TIDE, "queueable");

        // Full and playing is not an offer, and says so rather than saying
        // "in progress" as if there were a chair behind it.
        let full = entry(1, "Anna", 6, 6, true);
        assert!(full.table(&EN).contains(EN.lobby_full_tag));
        assert!(!full.table(&EN).contains(EN.lobby_in_progress));
        assert_ne!(full.table_tone(), palette::INK_TIDE, "not an invitation");

        // A build that announced no table gets no count invented for it.
        assert_eq!(entry(1, "Anna", 0, 0, false).table(&EN), "");
    }

    /// The middle of a row: whose beach it is, which the name of the beach
    /// itself deliberately does not say. "Room 3" is a good name for a game
    /// and a useless one for finding the friend who is running it.
    #[test]
    fn a_row_also_says_who_put_the_beach_up() {
        let named = hosted_entry(1, "Room 3", "Anna", 2, 6, false);
        assert_eq!(named.who(), "Room 3");
        assert_eq!(named.creator(), "Anna", "and whose room it is");

        // A beach with no name of its own is listed under its host's, and
        // then the column beside it stays empty rather than saying "Anna"
        // twice across two inches of screen.
        let unnamed = hosted_entry(2, "", "Anna", 2, 6, false);
        assert_eq!(unnamed.who(), "Anna");
        assert_eq!(unnamed.creator(), "");

        // A build that announced neither is still findable by address,
        // as the whole list did before names existed.
        let old = hosted_entry(9, "", "", 1, 6, false);
        assert!(old.who().contains("10.0.0.9"));
        assert_eq!(old.creator(), "");
    }

    /// A hall can have more games than the list has rows, and they come and
    /// go while somebody is reading it, so the cursor holds onto the beach
    /// it names rather than the row it sat in.
    #[test]
    fn the_cursor_follows_the_beach_not_the_row() {
        let mut state = LobbyState::default();
        state.hosts = vec![
            entry(1, "Anna", 1, 6, false),
            entry(2, "Bo", 1, 6, false),
            entry(3, "Cy", 1, 6, false),
        ];
        state.selected = Some(state.hosts[2].addr);
        assert_eq!(state.selected_index(), Some(2));

        // Anna goes away. Cy is now row 1, and the cursor is still on Cy.
        state.hosts.remove(0);
        state.settle_cursor();
        assert_eq!(state.selected_index(), Some(1));
        assert_eq!(state.hosts[state.selected_index().unwrap()].name, "Cy");

        // Cy goes away too, and the cursor falls somewhere real rather than
        // pointing off the end.
        state.hosts.retain(|host| host.name != "Cy");
        state.settle_cursor();
        assert_eq!(state.selected_index(), Some(0));

        // The last beach goes: nothing is selected, and nothing panics.
        state.hosts.clear();
        state.settle_cursor();
        assert_eq!(state.selected_index(), None);
        state.step_cursor(Nav::Down);
        assert_eq!(state.selected_index(), None);
    }

    /// However many made-up beacon ids one sender sprays, the list stops at
    /// [`MAX_BEACHES`], and what makes way is a row heard once, never a
    /// beach that has been speaking all along, the one this lobby is at, or
    /// the one under the cursor. Every made-up id is younger than any real
    /// beach, so giving up the stalest row gave up exactly those.
    #[test]
    fn the_list_is_bounded_however_many_ids_arrive() {
        let mut next = 1000u64;
        let mut spray = |count: u64| -> Vec<(SocketAddr, Beacon)> {
            (0..count)
                .map(|_| {
                    let (addr, mut beacon) = beach(9, "spam", 1, 6, false);
                    if let Beacon::Here { id, .. } = &mut beacon {
                        *id = next;
                    }
                    next += 1;
                    (addr, beacon)
                })
                .collect()
        };
        // Names that sort last, so no row survives by being first in line.
        let established = || beach(3, "Zed", 1, 6, false);
        let joined = beach(2, "Yan", 1, 6, false);
        let pointed = beach(4, "Xu", 1, 6, false);
        let held = [joined.0, pointed.0];
        let ttl = host_ttl(4);
        let mut hosts = Vec::new();
        for _ in 0..3 {
            refresh_hosts(&mut hosts, &[established()], 1.0, ttl, &held);
        }
        refresh_hosts(&mut hosts, &[joined, pointed], 0.0, ttl, &held);
        refresh_hosts(&mut hosts, &spray(MAX_BEACHES as u64), 0.0, ttl, &held);
        assert_eq!(hosts.len(), MAX_BEACHES);

        // Ten seconds of steady spray, with the real beach heard once a
        // second as it really is, and the joined and pointed-at beaches
        // not heard again at all, as up the port ladder between wide
        // beacons.
        for frame in 0..40 {
            let mut heard = spray(50);
            if frame % 4 == 0 {
                heard.push(established());
            }
            refresh_hosts(&mut hosts, &heard, 0.25, ttl, &held);
            assert_eq!(hosts.len(), MAX_BEACHES, "and the list stops there");
            for name in ["Zed", "Yan", "Xu"] {
                assert!(
                    hosts.iter().any(|host| host.name == name),
                    "{name} lost its row to a made-up id at frame {frame}"
                );
            }
        }
    }

    /// A listener past the first lobby port hears a beach on another
    /// machine one beacon in four, and keeps it listed through a lost one:
    /// it gave up after the first port's five seconds, so a single lost
    /// wide beacon dropped the row and the next listed it again.
    #[test]
    fn a_listener_up_the_ladder_rides_out_a_lost_wide_beacon() {
        use crate::transport::{LOBBY_PORTS, beacons_heard_one_in};
        assert_eq!(beacons_heard_one_in(LOBBY_PORTS[0]), 1);
        let up_the_ladder = beacons_heard_one_in(LOBBY_PORTS[1]);
        assert!(up_the_ladder > 1, "only the wide beacons go past the first");
        let ttl = host_ttl(up_the_ladder);
        let mut hosts = Vec::new();
        refresh_hosts(&mut hosts, &[beach(1, "Anna", 1, 6, false)], 0.0, ttl, &[]);
        // One wide beacon lost: two gaps' worth of silence, and a little.
        let silence = 2.0 * up_the_ladder as f32 * ANNOUNCE_EVERY + 0.5;
        refresh_hosts(&mut hosts, &[], silence, ttl, &[]);
        assert_eq!(hosts.len(), 1, "still listed after {silence} s");
        assert_eq!(host_ttl(1), HOST_TTL, "the first port is as it was");
    }

    /// A list full of real beaches keeps them, every one, when one more
    /// comes on the air, rather than swapping the newcomer and the least
    /// heard in and out under the cursor every second.
    #[test]
    fn a_full_list_of_real_beaches_holds_still() {
        let real = |n: u64| {
            let (_, mut beacon) = beach(1, &format!("B{n:02}"), 1, 6, false);
            if let Beacon::Here { id, .. } = &mut beacon {
                *id = 100 + n;
            }
            (format!("10.0.1.{n}:47777").parse().expect("addr"), beacon)
        };
        let all: Vec<(SocketAddr, Beacon)> = (0..MAX_BEACHES as u64).map(real).collect();
        let mut hosts = Vec::new();
        for _ in 0..3 {
            refresh_open(&mut hosts, &all, 1.0);
        }
        let before: Vec<SocketAddr> = hosts.iter().map(|host| host.addr).collect();
        for _ in 0..5 {
            let mut heard = all.clone();
            heard.push(real(MAX_BEACHES as u64));
            refresh_open(&mut hosts, &heard, 1.0);
            let now: Vec<SocketAddr> = hosts.iter().map(|host| host.addr).collect();
            assert_eq!(now, before, "no real beach gave up its row");
        }
    }

    /// Beaches are listed in a stable order, so the numbers a child reads
    /// out loud still mean the same games a moment later.
    #[test]
    fn the_list_holds_its_order() {
        let mut hosts = Vec::new();
        let heard = [
            beach(3, "Cy", 1, 6, false),
            beach(1, "Anna", 1, 6, false),
            beach(2, "Bo", 1, 6, false),
        ];
        refresh_open(&mut hosts, &heard, 0.0);
        let order: Vec<&str> = hosts.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(
            order,
            ["Anna", "Bo", "Cy"],
            "heard in one order, listed in another"
        );

        // Heard again in yet another order, and the list does not budge.
        refresh_open(&mut hosts, &[heard[0].clone(), heard[2].clone()], 0.1);
        let order: Vec<&str> = hosts.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(order, ["Anna", "Bo", "Cy"]);
    }

    /// The order does not depend on case. Compared without lowercasing
    /// anything, so this also checks the allocation-free comparator against
    /// the obvious one.
    #[test]
    fn names_sort_without_regard_to_case() {
        let mut hosts = Vec::new();
        let heard = [
            beach(1, "zoe", 1, 6, false),
            beach(2, "Alice", 1, 6, false),
            beach(3, "bob", 1, 6, false),
            beach(4, "ÉLODIE", 1, 6, false),
        ];
        refresh_open(&mut hosts, &heard, 0.0);
        let order: Vec<&str> = hosts.iter().map(|h| h.name.as_str()).collect();
        let mut expected: Vec<&str> = heard
            .iter()
            .map(|(_, beacon)| match beacon {
                Beacon::Here { name, .. } => name.as_str(),
                Beacon::Closing { .. } => unreachable!(),
            })
            .collect();
        expected.sort_by_key(|name| name.to_lowercase());
        assert_eq!(
            order, expected,
            "the cheap comparator and the obvious one agree"
        );
    }

    /// A hall with far more games than rows still sorts, scrolls and keeps
    /// the cursor on the beach it named. The list is refreshed every frame,
    /// so this is also the shape the comparator runs at.
    #[test]
    fn a_crowded_wire_still_gives_a_usable_list() {
        let mut state = LobbyState::default();
        let heard: Vec<_> = (1..=60u8)
            .map(|n| beach(n, &format!("beach{n:02}"), n % 7, 6, n % 3 == 0))
            .collect();
        refresh_open(&mut state.hosts, &heard, 0.0);
        assert_eq!(state.hosts.len(), 60, "every beach on the wire is listed");

        // Walk to the far end; the window follows and never runs past it.
        state.settle_cursor();
        for _ in 0..40 {
            state.step_cursor(Nav::Down);
        }
        let at = state.selected_index().expect("still on something real");
        assert_eq!(at, 40);
        assert!(state.scroll <= at, "the cursor is not above the window");
        assert!(
            at < state.scroll + state.rows_shown(),
            "nor below it: row {at}, window from {}",
            state.scroll
        );

        // The beach under the cursor goes away; the cursor does not follow
        // whatever slides into that row.
        let was = state.hosts[at].addr;
        state.hosts.retain(|host| host.addr != was);
        state.settle_cursor();
        assert_ne!(state.selected, Some(was));
        let at = state.selected_index().expect("landed somewhere real");
        assert!(at < state.hosts.len());
    }

    /// Games going away from above a scrolled list pull the window back
    /// up: with fewer games left than rows, every one of them is in view,
    /// none hidden above blank rows, and the bar has nothing to show.
    #[test]
    fn games_leaving_from_above_pull_the_window_back() {
        use crate::app::menu_ui::scroll_bar::Scrolls;
        let mut state = LobbyState::default();
        let heard: Vec<_> = (1..=15u8)
            .map(|n| beach(n, &format!("beach{n:02}"), 1, 6, false))
            .collect();
        refresh_open(&mut state.hosts, &heard, 0.0);
        state.fit_rows(12);
        state.point_at(Some(14));
        assert_eq!(state.scroll, 3, "scrolled to the bottom");
        let gone: Vec<_> = state.hosts[..5].iter().map(|host| host.addr).collect();
        state.hosts.retain(|host| !gone.contains(&host.addr));
        state.settle_cursor();
        assert_eq!(state.selected_index(), Some(9), "still on its game");
        assert_eq!(state.scroll, 0, "nothing hidden above");
        assert!(state.listed(0).is_some());
        assert!(state.window().thumb().is_none(), "the whole list in view");
    }

    /// The beach list's scroll bar shows the window the rows are painted
    /// from: at the top on arrival, at the bottom once the cursor reaches
    /// the last beach, and gone when beaches go off the air and the rest
    /// fit the card.
    #[test]
    fn the_bar_follows_the_beach_list() {
        use crate::app::menu_ui::scroll_bar::{self, BarOf, reading};
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        let mut state = LobbyState::default();
        let heard: Vec<_> = (1..=30u8)
            .map(|n| beach(n, &format!("beach{n:02}"), 1, 6, false))
            .collect();
        refresh_open(&mut state.hosts, &heard, 0.0);
        state.settle_cursor();
        app.insert_resource(state);
        app.add_systems(Update, scroll_bar::follow::<LobbyState>);
        app.world_mut()
            .run_system_once(|mut commands: Commands| {
                commands.spawn(Node::default()).with_children(|list| {
                    scroll_bar::spawn(list, BarOf::<LobbyState>::default());
                });
            })
            .expect("the bar is built");
        let bar = |app: &mut App| {
            app.update();
            reading::<BarOf<LobbyState>>(app.world_mut())
        };
        let share = LobbyState::default().rows_shown() as f32 / 30.0;
        assert_eq!(bar(&mut app), Some([0.0, share, 1.0 - share]), "at the top");

        for _ in 0..29 {
            app.world_mut()
                .resource_mut::<LobbyState>()
                .step_cursor(Nav::Down);
        }
        assert_eq!(
            bar(&mut app),
            Some([1.0 - share, share, 0.0]),
            "at the bottom"
        );

        // Beaches go off the air until the rest fit: no bar.
        let mut state = app.world_mut().resource_mut::<LobbyState>();
        let rows = state.rows_shown();
        state.hosts.truncate(rows);
        state.settle_cursor();
        assert_eq!(bar(&mut app), None, "everything fits");
    }

    /// One game, listed twice. Every beacon goes out to the broadcast
    /// address *and* to loopback, since real broadcast does not reliably
    /// come back round to the machine that sent it, so a second instance on
    /// the host's own machine hears both copies and keying the list by
    /// address makes them two beaches.
    #[test]
    fn one_beach_heard_from_two_addresses_is_one_row() {
        let mut hosts = Vec::new();
        let named = |addr: &str| {
            (
                addr.parse::<SocketAddr>().expect("addr"),
                Beacon::Here {
                    id: 0x5EA5,
                    name: "Room 3".to_string(),
                    host: "Sam".to_string(),
                    taken: 2,
                    seats: 6,
                    running: false,
                    bots: false,
                    full: false,
                },
            )
        };
        // The same game port, reached two ways, as a listener really sees it.
        let heard = [named("127.0.0.1:49213"), named("192.168.1.8:49213")];
        refresh_open(&mut hosts, &heard, 0.0);
        assert_eq!(hosts.len(), 1, "one game, one row: {:?}", hosts.len());
        let kept = hosts[0].addr;

        // And it does not flip between the two addresses every beacon.
        refresh_open(&mut hosts, &heard, 0.1);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].addr, kept, "the row does not churn its address");

        // Its farewell takes the one row with it, whichever address it
        // happens to arrive from.
        refresh_open(
            &mut hosts,
            &[(
                "127.0.0.1:49213".parse().expect("addr"),
                Beacon::Closing { id: 0x5EA5 },
            )],
            0.0,
        );
        assert!(hosts.is_empty(), "and leaves nothing behind");
    }

    /// Two hosts that happen to draw the same ephemeral port are still two
    /// beaches: the id is what tells them apart, not the number.
    #[test]
    fn two_beaches_sharing_a_port_number_are_still_two() {
        let mut hosts = Vec::new();
        let heard = [
            (
                "10.0.0.1:49213".parse::<SocketAddr>().expect("addr"),
                Beacon::Here {
                    id: 1,
                    name: "one".into(),
                    host: "Sam".into(),
                    taken: 1,
                    seats: 6,
                    running: false,
                    bots: false,
                    full: false,
                },
            ),
            (
                "10.0.0.2:49213".parse::<SocketAddr>().expect("addr"),
                Beacon::Here {
                    id: 2,
                    name: "two".into(),
                    host: "Sam".into(),
                    taken: 1,
                    seats: 6,
                    running: false,
                    bots: false,
                    full: false,
                },
            ),
        ];
        refresh_open(&mut hosts, &heard, 0.0);
        assert_eq!(hosts.len(), 2);
    }

    /// Two children will both call their beach Sam; the list still has to
    /// give them different rows, and the same rows every frame.
    #[test]
    fn two_beaches_of_the_same_name_keep_their_own_rows() {
        let mut hosts = Vec::new();
        let heard = [beach(9, "Sam", 1, 6, false), beach(4, "Sam", 1, 6, false)];
        refresh_open(&mut hosts, &heard, 0.0);
        assert_eq!(hosts.len(), 2, "one row each");
        let first = hosts[0].addr;
        refresh_open(&mut hosts, &heard, 0.1);
        assert_eq!(hosts[0].addr, first, "and the same one each, every time");
    }

    /// A joiner waiting at a beach whose host says goodbye on the air
    /// hears it at once.
    #[test]
    fn a_joiner_hears_its_beach_close() {
        use crate::transport::UdpTransport;
        let there: SocketAddr = "127.0.0.1:47999".parse().expect("addr");
        let mut state = LobbyState::default();
        let joined = Joined::dialled(UdpTransport::join(there).expect("join"), false);
        state.standing = Standing::Joining(joined);
        state.hosts.push(HostEntry {
            addr: there,
            id: 7,
            name: "Bay".into(),
            host: "Sam".into(),
            taken: 2,
            seats: 4,
            running: true,
            bots: false,
            full: false,
            age: 0.0,
            heard: 1,
        });

        let elsewhere: SocketAddr = "10.0.0.9:47778".parse().expect("addr");
        assert!(!host_closed(
            &state,
            &[(elsewhere, Beacon::Closing { id: 8 })]
        ));
        assert!(host_closed(
            &state,
            &[(elsewhere, Beacon::Closing { id: 7 })]
        ));

        // The list kept the loopback copy of the beach while the joiner
        // dialled the broadcast one: found by its id all the same.
        learn_the_beach(&mut state);
        assert_eq!(state.joined().expect("joined").beach, Some(7));
        state.hosts[0].addr = "127.0.0.1:48000".parse().expect("addr");
        assert!(host_closed(
            &state,
            &[(elsewhere, Beacon::Closing { id: 7 })]
        ));
    }
}
