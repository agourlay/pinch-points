//! Hearing the hall: which beaches are on the air, in what order, and
//! which of them a cursor is pointing at.
//!
//! A beach leaves the list two ways: it says so, or it stops being heard.
//! The timeout underneath the farewell is for the exits that cannot be
//! announced.

use super::*;

pub(super) const HOST_TTL: f32 = 5.0;

/// How long a peer may go unheard before the lobby gives up on it. Four
/// greetings' worth: long enough to ride out a loss burst, short enough
/// that a table showing five players has five players at it.
pub(super) const PEER_TTL: f32 = 4.0;

/// The three timers have to stay in this order, and it is cheaper to say
/// so to the compiler than to test it: a peer must outlive a burst of lost
/// greetings, and must be given up before the beach it is sitting at is,
/// or a table would empty itself one player at a time while the beach it
/// belongs to is still being listed as open.
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
    /// says nothing about whose it is, which in a hall of eight is what
    /// somebody looking for their friend actually wants.
    pub host: String,
    /// The table as its host last described it. `seats` is 0 from a build
    /// that announced no occupancy, which the list reads as "unknown"
    /// rather than "full".
    pub taken: u8,
    pub seats: u8,
    /// The round has begun: not joinable now, but queueable if there is
    /// room, and the queue is admitted when the round ends.
    pub running: bool,
    pub age: f32,
}

impl HostEntry {
    /// Whether joining (or queueing for) this beach could come to anything.
    pub fn has_room(&self) -> bool {
        self.seats == 0 || self.taken < self.seats
    }

    /// What the beach is called. One that announced no name falls back to
    /// its host's, and one that announced neither to its address. That is
    /// what the whole list showed before names existed, and is now a column
    /// of its own, so it is the last thing worth repeating here.
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
/// at once, and one that has gone quiet for [`HOST_TTL`] drops off: a host
/// that quit should not sit in the join list forever.
///
/// The timeout is the backstop, not the mechanism. A host that leaves
/// properly says so and is gone within the frame; the ageing is for the
/// ones that cannot say anything: a killed process, a pulled cable, a
/// laptop lid.
///
/// Pure, so both rules are testable without sockets.
pub(super) fn refresh_hosts(
    hosts: &mut Vec<HostEntry>,
    heard: &[(SocketAddr, Beacon)],
    delta: f32,
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
            // age: a beach fills up, empties, and starts its round while
            // the list is watching, and a host first heard before it had a
            // name should not keep the gap.
            Beacon::Here {
                id,
                name,
                host,
                taken,
                seats,
                running,
            } => {
                let mut fresh = HostEntry {
                    addr,
                    id: *id,
                    name: name.clone(),
                    host: host.clone(),
                    taken: *taken,
                    seats: *seats,
                    running: *running,
                    age: 0.0,
                };
                match hosts.iter_mut().find(|host| same_beach(host, *id, addr)) {
                    Some(entry) => {
                        // Keep the address it was first heard from. The same
                        // beach arrives from two of them, the broadcast and
                        // the loopback copy, and swapping between them every
                        // second would be churn for no gain: either reaches it.
                        fresh.addr = entry.addr;
                        *entry = fresh;
                    }
                    None => hosts.push(fresh),
                }
            }
        }
    }
    hosts.retain(|host| host.age < HOST_TTL);
    // A stable order, so the list does not reshuffle under a player's
    // finger as beaches come and go. By name first, because that is what
    // the list shows and what someone is looking for; by address to break
    // ties, because two children will both call their beach "Sam".
    hosts.sort_by(|a, b| {
        fold_case(&a.name)
            .cmp(fold_case(&b.name))
            .then_with(|| (a.addr.ip(), a.addr.port()).cmp(&(b.addr.ip(), b.addr.port())))
    });
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
/// This is a comparator, and a comparator on a list of `n` runs `n log n`
/// times, every frame, because the list is refreshed every frame. The
/// obvious `to_lowercase().cmp(&to_lowercase())` allocates twice each time
/// it is asked, which costs nothing at two beaches and some fifty thousand
/// allocations a second at forty. The address is compared as `(ip, port)`
/// for the same reason: `SocketAddr` has no `Ord`, and `to_string` on one
/// is another two.
fn fold_case(name: &str) -> impl Iterator<Item = char> + '_ {
    name.chars().flat_map(char::to_lowercase)
}

/// Refresh the discovered-host list. Does nothing while hosting, which is
/// when the listening socket is handed back.
pub fn discover(time: Res<Time>, mut state: ResMut<LobbyState>) {
    let delta = time.delta_secs();
    let heard = state
        .discovery
        .as_mut()
        .map(Discovery::poll)
        .unwrap_or_default();
    refresh_hosts(&mut state.hosts, &heard, delta);
    state.settle_cursor();
}

/// A browser's arrows walk the beach list. Enter takes the one under the
/// cursor, and the digit keys stay as a shortcut to the first nine,
/// because "press 3" is the fastest thing in the world to shout across a
/// room.
///
/// Arrows only, not the W/S pairing the other menus also accept: W is the
/// watch toggle here, and has been since before there was a list to walk.
pub(super) fn walk_the_list(keys: &ButtonInput<KeyCode>, state: &mut LobbyState) {
    if !state.standing().at_a_beach() {
        if keys.just_pressed(KeyCode::ArrowUp) {
            state.step_cursor(false);
        }
        if keys.just_pressed(KeyCode::ArrowDown) {
            state.step_cursor(true);
        }
    }
}

#[cfg(test)]
mod list_tests {
    use super::*;
    use crate::app::i18n::EN;

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
            age: 0.0,
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

    /// A school hall can have more games than the list has rows, and they
    /// come and go while a child is reading it. The cursor holds onto the
    /// beach it names, not the row it sat in. Otherwise pressing Enter
    /// joins whichever game happened to slide into that row.
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
        state.step_cursor(true);
        assert_eq!(state.selected_index(), None);
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
        refresh_hosts(&mut hosts, &heard, 0.0);
        let order: Vec<&str> = hosts.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(
            order,
            ["Anna", "Bo", "Cy"],
            "heard in one order, listed in another"
        );

        // Heard again in yet another order, and the list does not budge.
        refresh_hosts(&mut hosts, &[heard[0].clone(), heard[2].clone()], 0.1);
        let order: Vec<&str> = hosts.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(order, ["Anna", "Bo", "Cy"]);
    }

    /// The order does not depend on case, and putting a name in a
    /// different case does not let a beach jump the queue. Compared
    /// without lowercasing anything, so the assertion is also a check that
    /// the allocation-free comparator agrees with the obvious one.
    #[test]
    fn names_sort_without_regard_to_case() {
        let mut hosts = Vec::new();
        let heard = [
            beach(1, "zoe", 1, 6, false),
            beach(2, "Alice", 1, 6, false),
            beach(3, "bob", 1, 6, false),
            beach(4, "ÉLODIE", 1, 6, false),
        ];
        refresh_hosts(&mut hosts, &heard, 0.0);
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
        refresh_hosts(&mut state.hosts, &heard, 0.0);
        assert_eq!(state.hosts.len(), 60, "every beach on the wire is listed");

        // Walk to the far end; the window follows and never runs past it.
        state.settle_cursor();
        for _ in 0..40 {
            state.step_cursor(true);
        }
        let at = state.selected_index().expect("still on something real");
        assert_eq!(at, 40);
        assert!(state.scroll <= at, "the cursor is not above the window");
        assert!(
            at < state.scroll + LIST_ROWS,
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

    /// The bug this was reported as: one game, listed twice. Every beacon
    /// goes out to the broadcast address *and* to loopback, because real
    /// broadcast does not reliably come back round to the machine that
    /// sent it and same-machine play has to work. A second instance on the
    /// host's own machine therefore hears both copies, one sourced from
    /// 127.0.0.1 and one from the LAN address, and keying the list by
    /// address made them two beaches.
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
                },
            )
        };
        // The same game port, reached two ways, as a listener really sees it.
        let heard = [named("127.0.0.1:49213"), named("192.168.1.8:49213")];
        refresh_hosts(&mut hosts, &heard, 0.0);
        assert_eq!(hosts.len(), 1, "one game, one row: {:?}", hosts.len());
        let kept = hosts[0].addr;

        // And it does not flip between the two addresses every beacon.
        refresh_hosts(&mut hosts, &heard, 0.1);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].addr, kept, "the row does not churn its address");

        // Its farewell takes the one row with it, whichever address it
        // happens to arrive from.
        refresh_hosts(
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
                },
            ),
        ];
        refresh_hosts(&mut hosts, &heard, 0.0);
        assert_eq!(hosts.len(), 2);
    }

    /// Two children will both call their beach Sam; the list still has to
    /// give them different rows, and the same rows every frame.
    #[test]
    fn two_beaches_of_the_same_name_keep_their_own_rows() {
        let mut hosts = Vec::new();
        let heard = [beach(9, "Sam", 1, 6, false), beach(4, "Sam", 1, 6, false)];
        refresh_hosts(&mut hosts, &heard, 0.0);
        assert_eq!(hosts.len(), 2, "one row each");
        let first = hosts[0].addr;
        refresh_hosts(&mut hosts, &heard, 0.1);
        assert_eq!(hosts[0].addr, first, "and the same one each, every time");
    }
}
