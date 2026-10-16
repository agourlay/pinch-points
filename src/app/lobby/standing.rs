//! Where the player stands in the lobby: choosing a beach, greeting one,
//! or hosting one, and everything that goes with standing there.

use super::*;

/// What a joiner says to a host, every second until it is answered: a
/// `Watch` if it came to look, else a `Hello` carrying its name. The same
/// greeting from the lobby and from a results card, so the host reads the
/// wish to watch the same way wherever it was greeted from.
pub(crate) fn greeting(watching: bool, name: &str) -> NetMsg {
    match watching {
        true => NetMsg::watch(name),
        false => NetMsg::hello(name),
    }
}

/// Where the player stands in the lobby, and everything that goes with
/// standing there.
///
/// One value rather than the four fields that used to say it (`hosting`,
/// `joining`, `watching`, and whether anything had been heard): a field
/// that only means anything in one standing (the host's peers, the
/// joiner's patience with a silent host) is a field that can be read in
/// the wrong one.
pub enum Standing {
    /// Choosing a beach, with W armed or not: armed, the next pick
    /// watches rather than plays.
    Choosing { watching: bool },
    /// Greeting a host, waiting to be given a seat.
    Joining(Joined),
    /// On the air, gathering a table.
    Hosting(Hosted),
}

impl Default for Standing {
    fn default() -> Self {
        Standing::Choosing { watching: false }
    }
}

impl Standing {
    /// Whether there is anybody at the other end: somebody to talk to, a
    /// table to show, and no list worth reading.
    pub fn at_a_beach(&self) -> bool {
        matches!(self, Standing::Joining(_) | Standing::Hosting(_))
    }

    /// A beach going on the air this frame.
    pub fn hosting(announcer: Announcer, transport: UdpTransport) -> Standing {
        Standing::Hosting(Hosted {
            announcer,
            transport,
            peers: PeerBook::default(),
            announce_in: 0.0,
            kicking: false,
            bot_string: None,
        })
    }
}

/// A beach on the air: its beacon, the game socket gathering the table,
/// and everyone who has turned up on it.
pub struct Hosted {
    pub announcer: Announcer,
    pub transport: UdpTransport,
    /// Everyone on the socket, by its index there: the name its greeting
    /// carried, whether it asked to watch, and how long since it was last
    /// heard from. A joiner re-greets every second for as long as it sits
    /// in the lobby, so silence is the only evidence of leaving there is:
    /// UDP will never say so. Nobody holds a chair here; the plan is dealt
    /// at the launch.
    pub peers: PeerBook,
    pub(super) announce_in: f32,
    /// The host pressed K and is picking whom to ask to leave: the next
    /// number is a seat at the table, Esc is never mind.
    pub kicking: bool,
    /// The string a bot connects straight to this beach with (route 2),
    /// while the host says bots are welcome and there is a chair for one.
    pub bot_string: Option<String>,
}

impl Hosted {
    /// Drop a peer from the socket and from the table, so whoever came
    /// after it moves up one in both.
    pub(super) fn forget_peer(&mut self, peer: usize) {
        self.transport.forget(peer);
        self.peers.forget(peer);
    }

    /// Ask a peer to leave, and keep it out of this beach.
    pub(super) fn turn_away(&mut self, peer: usize) {
        self.transport.turn_away(peer);
        self.peers.forget(peer);
    }

    /// Peers here to play, the ones who fill the seats. Capped at the five
    /// chairs beside the host's: seats run out long before the socket
    /// does, which takes [`MAX_PEERS`](crate::transport::MAX_PEERS), and
    /// `PeerBook::seat_plan` turns the surplus into onlookers, so a table of eight
    /// would-be rivals reads as "5 aboard".
    pub fn players_aboard(&self) -> usize {
        let seatable = MAX_PLAYERS - 1;
        self.peers
            .iter()
            .filter(|peer| !peer.watch)
            .count()
            .min(seatable)
    }

    /// Peers here to look rather than play.
    pub(super) fn watchers_aboard(&self) -> usize {
        self.peers.iter().filter(|peer| peer.watch).count()
    }
}

/// A beach being greeted: the socket greeting it, and what has come back.
pub struct Joined {
    pub transport: UdpTransport,
    /// We asked to watch, so we greet with `Watch` and expect no seat back.
    pub watching: bool,
    /// How many are in front of us in line for the next round, once the
    /// host has said we are in one. `None` while there is still a lobby
    /// to be in: a round has to be under way before anybody queues.
    pub queued: Option<u8>,
    pub(super) hello_in: f32,
    /// Seconds since the host last said anything, and whether it has ever
    /// said anything at all.
    ///
    /// Dialling opens a socket, which on UDP proves nothing: there may be
    /// no machine at that address, or no game on it, or a firewall in the
    /// way, and all three look just like a host who has not started the
    /// round yet. These two are how the difference gets onto the screen.
    pub(super) host_silence: f32,
    pub(super) host_answered: bool,
    /// Seconds since the host said it was going (`NetMsg::Leaving`), or
    /// `None` if it never did or has said anything since.
    ///
    /// A goodbye is a claim nothing checks, and believed on arrival one
    /// forged datagram emptied a lobby table. So it is believed only once
    /// it has stood for [`super::joining::LOBBY_GOODBYE_GRACE`], longer than
    /// the host's once-a-second roster, which takes it back.
    pub(super) host_goodbye: Option<f32>,
    /// The seed of the round just played, set when a finished match walks
    /// back into this lobby still connected. The host may still be on its
    /// results card, where it re-answers every greeting with that round's
    /// `Start`; without this the lobby would read the repeat as an
    /// invitation and walk straight back into the finished round.
    played_seed: Option<u64>,
    /// The host's dials and its beach's name, from its [`NetMsg::Roster`];
    /// `None` until the first roster lands.
    pub host_terms: Option<HostTerms>,
    /// The round as it stands, arriving in parts, for a watcher who
    /// greeted a beach mid-round (`net::catch_up`).
    pub(super) catching_up: crate::app::net::catch_up::Assembly,
    /// Joining as a bot (route 1): the seat's name, "Greedy (Ana)",
    /// greeted with the flag that says a bot will drive it.
    pub bot: Option<String>,
    /// The beacon id of the beach this is, once known: taken from its row
    /// when it was picked off the list, or learnt from the row that
    /// matches its address. Rows are found by it rather than by address,
    /// because one beach arrives from two addresses (the broadcast and the
    /// loopback copy) and the list keeps whichever it heard first.
    pub beach: Option<u64>,
}

/// What a joiner knows of the match it is joining, from the host's
/// roster: the two always arrive together.
pub struct HostTerms {
    /// The host's dials. The terms card paints these rather than this
    /// machine's own [`MatchConfig`], which is nothing to do with the match
    /// being joined.
    pub terms: MatchTerms,
    /// What the host's handmade beach is called, when its terms are on
    /// one: empty otherwise.
    pub beach_name: String,
}

impl Joined {
    /// A beach dialled this frame: calling, not aboard, since nothing has
    /// answered yet and on UDP an open socket is no evidence that anything
    /// will.
    pub fn dialled(transport: UdpTransport, watching: bool) -> Joined {
        Joined {
            transport,
            watching,
            queued: None,
            hello_in: ANNOUNCE_EVERY,
            host_silence: 0.0,
            host_answered: false,
            host_goodbye: None,
            played_seed: None,
            host_terms: None,
            catching_up: Default::default(),
            bot: None,
            beach: None,
        }
    }

    /// A beach walked back into from a finished round: the host was
    /// talking moments ago, so this is a beach, not an unanswered address,
    /// until the silence rule says otherwise, and it greets again at once.
    pub fn returned(
        transport: UdpTransport,
        watching: bool,
        played_seed: u64,
        bot: Option<String>,
    ) -> Joined {
        Joined {
            transport,
            watching,
            queued: None,
            hello_in: 0.0,
            host_silence: 0.0,
            host_answered: true,
            host_goodbye: None,
            played_seed: Some(played_seed),
            host_terms: None,
            catching_up: Default::default(),
            bot,
            beach: None,
        }
    }

    /// What this peer greets the host with: a bot's seat says so.
    pub fn greeting(&self, name: &str) -> NetMsg {
        match self.bot.is_some() {
            true => NetMsg::hello_bot(self.speaking_as(name)),
            false => greeting(self.watching, name),
        }
    }

    /// The name this peer goes by at the table, `me` being the player's
    /// own: a bot's seat name when it joined as one, which is what its
    /// greeting gave the host and so what the host vouches its chat
    /// under.
    pub fn speaking_as<'a>(&'a self, me: &'a str) -> &'a str {
        self.bot.as_deref().unwrap_or(me)
    }

    /// The seed of the round this table walked out of, if it did.
    pub fn played_seed(&self) -> Option<u64> {
        self.played_seed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both greetings say who is greeting. The watcher's did not, and the
    /// host has two uses for a name it never got: the feed, which
    /// announced every player and no spectator at all, and the check on
    /// who a chat line claims to be from, which a nameless peer walks
    /// straight through.
    #[test]
    fn a_spectator_greets_by_name_like_everybody_else() {
        assert_eq!(
            greeting(true, "Dee"),
            NetMsg::Watch {
                name: crate::transport::wire_name("Dee")
            },
        );
        assert_eq!(
            greeting(false, "Bo"),
            NetMsg::Hello {
                name: crate::transport::wire_name("Bo"),
                bot: false,
            },
        );
    }

    /// A peer that joined as a bot speaks under the seat name it greeted
    /// with, which is the name the host vouches its lines under: its own
    /// echo named the player at the keyboard, and the rest of the table
    /// read the bot's.
    #[test]
    fn a_bot_chats_under_the_name_it_greeted_with() {
        let socket = UdpTransport::host(0).expect("socket");
        let joined = Joined::returned(socket, false, 0, Some("Greedy (Ana)".into()));
        let NetMsg::Hello { name, bot: true } = joined.greeting("Bo") else {
            panic!("a bot greets as one");
        };
        // The host writes the greeting's name down, and vouches by it.
        let mut peers = crate::app::net::PeerBook::default();
        peers.row(0).name = crate::transport::name_from_wire(&name);
        let vouched = peers.vouch(0, crate::transport::wire_name("Bo"));
        assert_eq!(
            crate::transport::name_from_wire(&vouched),
            crate::transport::name_as_sent(joined.speaking_as("Bo")),
            "the echo and the table read one name"
        );
    }
}
