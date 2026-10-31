//! Where a session came from and where it goes back to: the beach's
//! beacon, kept on the air while the round runs, the goodbye it owes the
//! network, and the table walking back to the lobby together.

use super::*;

/// The beach a session came from: its beacon, what it is called, and
/// whether there is a lobby to walk the table back to.
pub struct Home {
    /// Host side: the beacon, carried out of the lobby so the beach stays
    /// on the air while the round runs, listed as in progress with its
    /// occupancy, for anyone who wants the next one. Silent on a joiner,
    /// and on a direct `PINCH_HOST` pair, which never announced at all.
    pub(super) announcer: Farewell,
    /// What the beach is called, for the beacon it keeps up while the round
    /// runs. Not a player's name: the list is choosing between games.
    pub game_name: String,
    /// Whether the beach takes bots, for the beacon it keeps up.
    pub bots_welcome: bool,
    pub(super) announce_in: f32,
    /// Whether this session was formed in the beach lobby, which is the
    /// only place a finished match has to walk its table back to: the
    /// direct `PINCH_HOST` pair was never anywhere else to begin with.
    pub from_lobby: bool,
    /// Joiner side: whether it asked to watch, in the lobby it came from.
    ///
    /// Not whether it holds a chair: a player dealt none because the table
    /// was full is watching this round without having asked to, and
    /// greeting the host as a watcher (between rounds, and back in the
    /// lobby) told the host it had, so it was never dealt a chair again.
    pub wants_to_watch: bool,
    /// Joiner side: seconds until the next greeting while the results card
    /// is up.
    ///
    /// An established session says nothing at all between rounds: inputs
    /// and hashes belong to the round that ended, and the host has nothing
    /// to send until somebody calls the next one. Silence read as evidence
    /// is then a joiner walking out of a perfectly good table twenty
    /// seconds into a results card, so it keeps saying hello on the lobby's
    /// cadence, and the host's reply is what tells it the host is there.
    pub(super) greet_in: f32,
}

impl Home {
    /// Nowhere in particular: the direct pair, which never announced and
    /// has no lobby behind it.
    pub(super) fn nowhere() -> Home {
        Home {
            announcer: Farewell::silent(),
            game_name: String::new(),
            bots_welcome: false,
            announce_in: 0.0,
            from_lobby: false,
            wants_to_watch: false,
            greet_in: 0.0,
        }
    }
}

/// The goodbye a hosted beach owes the network when its session ends.
///
/// A hosted beach stops being announced when its session goes, whichever
/// way that happens: the match ending, the player quitting to the menu, a
/// desync giving up. Holding the duty here rather than at each of those
/// exits means no path drops an announcing session without either saying
/// goodbye or taking the announcer back out through
/// [`OnlineSession::back_to_the_lobby`], the one exit where the beach is
/// not going anywhere.
pub(super) struct Farewell {
    /// `None` on a joiner and on the direct pair, which never announced,
    /// and in the shell a session leaves behind when its beach goes home.
    announcer: Option<Announcer>,
    /// The game port the beacons named, taken when the announcer came
    /// aboard: the port never changes after the bind, and it has to be
    /// readable at drop time.
    port: u16,
}

impl Farewell {
    /// A session with nothing to announce and so nothing to owe.
    fn silent() -> Farewell {
        Farewell {
            announcer: None,
            port: 0,
        }
    }

    /// The announcer, while one is aboard.
    fn aboard(&self) -> Option<&Announcer> {
        self.announcer.as_ref()
    }

    /// Take the announcer back out, disarming the goodbye: the beach is
    /// going back on the air, not away.
    fn disarm(mut self) -> Option<Announcer> {
        self.announcer.take()
    }
}

impl Drop for Farewell {
    fn drop(&mut self) {
        if let Some(announcer) = &self.announcer {
            announcer.closing(self.port);
        }
    }
}

/// What survives of a session when its table walks back to the lobby
/// together at the end of a match: the socket, still connected, and the
/// bookkeeping the lobby needs to stand the beach back up. What that is
/// depends on which end of the table this was.
pub enum LobbyReturn {
    Host {
        /// The beacon, going back on the air as open. `None` for the
        /// direct `PINCH_HOST` pair, which never announced.
        announcer: Option<Announcer>,
        transport: UdpTransport,
        game_name: String,
        /// Every registered peer by its socket index, with the name its
        /// greeting carried and whether it watches, by the same rule the
        /// next round would have seated it. Nobody holds a chair any more:
        /// the lobby deals them again at the launch.
        peers: PeerBook,
        /// The round just played, as its lockstep ended, for a peer still
        /// short of its end: the host has stopped relaying, and the lobby
        /// sends what is missing for a while instead
        /// (`crate::app::lobby::RoundTail`).
        played: Box<Lockstep>,
        /// How many rounds this socket has dealt, the match just played
        /// included, for the next match to count on from
        /// ([`OnlineSession::rounds_dealt`]).
        rounds_dealt: u32,
        /// The seats the host's partner picks put its bots and the AI in
        /// ([`OnlineSession::arranged`]), for the next match's deal.
        arranged: Option<super::order::Arranged>,
    },
    Joiner {
        transport: UdpTransport,
        /// This peer came to watch, and greets with `Watch`.
        watching: bool,
        /// The seed of the round just played. A host still on its results
        /// card re-answers every greeting with that round's `Start`, and a
        /// lobby that has just walked out of it must read the repeat as
        /// stale rather than as an invitation straight back in.
        played_seed: u64,
        /// As a bot's seat (route 1): its name, to greet with.
        bot: Option<String>,
    },
}

impl OnlineSession {
    /// Keep the beach on the air while the round runs, so a lobby can list
    /// it as in progress rather than not at all. Hosts only, and the beacon
    /// says running, so nobody is offered a join that lockstep could not
    /// honour; what it offers is a place in the queue.
    ///
    /// `taken` counts the humans, not the table: an AI seat gives way to a
    /// player who wants it. The humans in line for the next round count
    /// too, since the chair each will take is spoken for, and a beacon that
    /// left the queue out let a sixth player in to be dealt a watching place with
    /// no notice.
    pub fn keep_announcing(&mut self, delta: f32) {
        if self.home.announcer.aboard().is_none()
            || !crate::app::lobby::once_a_second(&mut self.home.announce_in, delta)
        {
            return;
        }
        let taken = self.players_spoken_for();
        let (Some(announcer), Ok(addr)) =
            (self.home.announcer.aboard(), self.transport.local_addr())
        else {
            return;
        };
        announcer.running(
            addr.port(),
            crate::transport::OnAir {
                name: &self.home.game_name,
                // Seat zero is the host's, and the host is the only one that
                // announces.
                host: &self.names[0],
                taken,
                seats: MAX_PLAYERS as u8,
                bots: self.home.bots_welcome,
                // As the lobby says it (`host_tick`): a round in progress
                // fills its socket with watchers as readily as a lobby does.
                full: self.transport.peer_count() >= crate::transport::MAX_PEERS,
            },
        );
    }

    /// Host: carry the beacon out of the lobby, and with it the goodbye
    /// owed to the network from now on.
    pub fn stay_on_air(&mut self, announcer: Announcer) {
        // Port 0 if the socket will not say: the goodbye is matched by
        // the announcer's id first, so it still clears the right row.
        let port = self
            .transport
            .local_addr()
            .map(|addr| addr.port())
            .unwrap_or(0);
        self.home.announcer = Farewell {
            announcer: Some(announcer),
            port,
        };
    }

    /// Whether to greet as a watcher: what this player asked for, where it
    /// came from a lobby that asked; otherwise, with nothing asked, what
    /// it is.
    pub(crate) fn watch_wish(&self) -> bool {
        match self.home.from_lobby {
            true => self.home.wants_to_watch,
            false => self.watching(),
        }
    }

    /// Dismantle a finished match into what the lobby needs to stand the
    /// beach back up, goodbye disarmed: this table is going back to the
    /// lobby together, not away.
    pub fn back_to_the_lobby(mut self) -> LobbyReturn {
        let host = self.is_host();
        let watching = self.watch_wish();
        // Each peer under the name its chair carried, which is the one
        // every screen showed, and the plan's chairs given up: back in the
        // lobby the plan is dealt again, and only who watches survives it.
        for peer in 0..self.peers.len() {
            if let Some(name) = self.peer_name(peer) {
                let name = name.to_string();
                self.peers.row(peer).name = name;
            }
        }
        self.peers.unseat();
        let Self {
            transport,
            session,
            seats: _,
            terms,
            beach: _,
            peers,
            bot,
            put_out: _,
            bots_here: _,
            names: _,
            kinds: _,
            hashes: _,
            stall: _,
            abandoned: _,
            heard: _,
            notices: _,
            stands: _,
            home,
            next_round: _,
            series_standing: _,
            ping_in: _,
            pace: _,
            rounds_dealt,
            crowned: _,
            catch_up: _,
            stale_rounds: _,
            picking_map: _,
            arranged,
        } = self;
        let Home {
            announcer,
            game_name,
            bots_welcome: _,
            announce_in: _,
            from_lobby: _,
            wants_to_watch: _,
            greet_in: _,
        } = home;
        match host {
            true => LobbyReturn::Host {
                announcer: announcer.disarm(),
                transport,
                game_name,
                peers,
                played: Box::new(session),
                rounds_dealt,
                arranged,
            },
            false => LobbyReturn::Joiner {
                transport,
                watching,
                played_seed: terms.seed,
                bot,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::net::spectator_tests::hosting_session;
    use crate::sim::DEFAULT_DELAY;
    use crate::transport::{Beacon, Discovery};

    /// Beacons from other tests share the machine, so every packet is
    /// judged by whether it names our game port.
    fn heard_for(discovery: &mut Discovery, port: u16) -> Vec<Beacon> {
        let mut heard = Vec::new();
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            heard.extend(
                discovery
                    .poll()
                    .into_iter()
                    .filter(|(addr, _)| addr.port() == port)
                    .map(|(_, beacon)| beacon),
            );
            if !heard.is_empty() {
                break;
            }
        }
        heard
    }

    /// The beacon of a round in progress counts the chairs spoken for,
    /// the players in line for the next round with the ones at the table:
    /// one that left the line out let a sixth player in to be dealt a
    /// watching place with no notice.
    #[test]
    fn the_running_beacon_counts_the_line_for_the_next_round() {
        use crate::transport::Air;
        let mut discovery = Discovery::bind_on(&[0]).expect("a port");
        let listening = discovery.port().expect("bound");
        let mut session = hosting_session(0);
        let port = session.transport.local_addr().expect("addr").port();
        session.stay_on_air(Announcer::on(0x5EA7, Air::loopback(&[listening])).expect("air"));
        // Seats 0 and 1 play, and somebody waits in line for a chair.
        session.peers.reach(1);
        assert!(session.peers.iter().all(|peer| peer.wants_a_chair()));
        session.keep_announcing(1.0);
        let heard = heard_for(&mut discovery, port);
        let Some(Beacon::Here {
            taken,
            running,
            full,
            ..
        }) = heard.first()
        else {
            panic!("no beacon: {heard:?}");
        };
        assert!(*running);
        assert_eq!(*taken, 3, "the two at the table and the one in line");
        assert!(!full);
    }

    /// The goodbye still fires on every ordinary way out: quitting to the
    /// menu, a desync giving up, the process letting go.
    #[test]
    fn a_dropped_session_still_says_goodbye() {
        let mut discovery = Discovery::bind().expect("lobby port");
        let mut session = hosting_session(0);
        let port = session.transport.local_addr().expect("addr").port();
        session.stay_on_air(Announcer::new(0xD00D).expect("announcer"));
        drop(session);
        let heard = heard_for(&mut discovery, port);
        let beacon = heard.first().expect("the goodbye went out");
        assert!(matches!(beacon, Beacon::Closing { .. }), "{beacon:?}");
    }

    /// The one exit where the beach is not going anywhere: walking the
    /// table back to the lobby keeps the socket, keeps the beacon, and
    /// says no goodbye, because the same beach is about to announce
    /// itself open again.
    #[test]
    fn the_way_back_to_the_lobby_says_no_goodbye() {
        let mut discovery = Discovery::bind().expect("lobby port");
        let mut session = hosting_session(42);
        let port = session.transport.local_addr().expect("addr").port();
        session.stay_on_air(Announcer::new(0xB0A7).expect("announcer"));
        session.home.game_name = "Room 3".into();
        session.home.from_lobby = true;
        session.rounds_dealt = 4;
        let LobbyReturn::Host {
            announcer,
            transport,
            game_name,
            peers: _,
            played: _,
            rounds_dealt,
            arranged: _,
        } = session.back_to_the_lobby()
        else {
            panic!("the host comes home as the host");
        };
        assert_eq!(game_name, "Room 3");
        assert_eq!(
            rounds_dealt, 4,
            "the next match counts its rounds on from these"
        );
        assert_eq!(
            transport.local_addr().expect("addr").port(),
            port,
            "the same socket every peer is still talking to"
        );
        assert!(
            announcer.is_some(),
            "the beacon rides home to announce again"
        );
        std::thread::sleep(std::time::Duration::from_millis(40));
        assert!(
            discovery
                .poll()
                .into_iter()
                .all(|(addr, _)| addr.port() != port),
            "no goodbye for a beach that is coming home"
        );
    }

    /// What the lobby gets back to seat people with: each peer's name as
    /// its greeting carried it, and the watchers by the same rule the next
    /// round would have used - a spectator's `None` in the launch plan, or
    /// a watch wish greeted mid-round.
    #[test]
    fn the_table_walks_back_with_its_names_and_watchers() {
        let mut session = hosting_session(0);
        let port = session.transport.local_addr().expect("addr").port();
        // Two peers, registered one at a time so their indices are known:
        // Bo plays, and somebody nameless watches.
        let bo = UdpTransport::join(("127.0.0.1", port)).expect("join");
        bo.send(NetMsg::hello("Bo"));
        let watcher = UdpTransport::join(("127.0.0.1", port)).expect("join");
        for want in [1, 2] {
            if want == 2 {
                watcher.send(NetMsg::watch("Dee"));
            }
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                for (msg, from) in session.transport.recv_all() {
                    match msg {
                        NetMsg::Hello { name, .. } => {
                            let told = name_from_wire(&name);
                            session.remember_peer_name(from, &told);
                        }
                        NetMsg::Watch { .. } => session.note_watch_wish(from),
                        NetMsg::Inputs(_)
                        | NetMsg::Hash { .. }
                        | NetMsg::Start(_)
                        | NetMsg::Pause { .. }
                        | NetMsg::Resume { .. }
                        | NetMsg::Queued { .. }
                        | NetMsg::SpectatorVote { .. }
                        | NetMsg::SpectatorTally { .. }
                        | NetMsg::Chat { .. }
                        | NetMsg::Notice(_)
                        | NetMsg::Roster { .. }
                        | NetMsg::Abandoned { .. }
                        | NetMsg::CatchUp { .. }
                        | NetMsg::SpectatorPick { .. }
                        | NetMsg::CrowdPicks { .. }
                        | NetMsg::Kicked
                        | NetMsg::NoBots
                        | NetMsg::Full
                        | NetMsg::Leaving
                        | NetMsg::Ping { .. }
                        | NetMsg::Pong { .. }
                        | NetMsg::Clock { .. }
                        | NetMsg::Incompatible { .. } => {}
                    }
                }
                if session.transport.peer_count() >= want {
                    break;
                }
            }
            assert_eq!(session.transport.peer_count(), want, "peer registered");
        }
        session.peers.deal(&[Some(1), None]);
        let LobbyReturn::Host { peers, .. } = session.back_to_the_lobby() else {
            panic!("the host comes home as the host");
        };
        let names: Vec<&str> = peers.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            ["Bo", ""],
            "each chair keeps the name its greeting carried"
        );
        let watching: Vec<bool> = peers.iter().map(Peer::watches).collect();
        assert_eq!(watching, [false, true], "and a watcher stays a watcher");
    }

    /// A joiner's way back: no beacon to carry, but the socket, the seed
    /// it played on, and whether it was watching all come home with it.
    #[test]
    fn a_watching_joiner_comes_home_still_watching() {
        let session = OnlineSession::new(
            UdpTransport::join(("127.0.0.1", 47999)).expect("join"),
            Lockstep::observer(vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms {
                seed: 7,
                ..MatchTerms::default()
            },
        );
        let LobbyReturn::Joiner {
            watching,
            played_seed,
            ..
        } = session.back_to_the_lobby()
        else {
            panic!("a joiner comes home as a joiner, with no beacon");
        };
        assert!(watching, "the watcher is remembered");
        assert_eq!(played_seed, 7);
    }

    /// A bot's seat (route 1) the full table dealt no chair still goes
    /// home a bot, under the seat name it greeted with: read off its chair,
    /// it had none, and walked back greeting as a person under the
    /// player's own name, which the host wrote down as a person.
    #[test]
    fn a_bot_dealt_no_chair_comes_home_a_bot() {
        let mut session = OnlineSession::new(
            UdpTransport::join(("127.0.0.1", 47999)).expect("join"),
            Lockstep::observer(vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        session.bot = Some("Greedy (Ana)".to_string());
        assert_eq!(
            session.own_greeting(),
            NetMsg::hello_bot("Greedy (Ana)"),
            "between rounds"
        );
        let LobbyReturn::Joiner {
            transport,
            watching,
            played_seed,
            bot,
        } = session.back_to_the_lobby()
        else {
            panic!("a joiner comes home as a joiner");
        };
        assert_eq!(bot.as_deref(), Some("Greedy (Ana)"));
        let joined = crate::app::lobby::Joined::returned(transport, watching, played_seed, bot);
        assert_eq!(
            joined.greeting("Ana"),
            NetMsg::hello_bot("Greedy (Ana)"),
            "and back in the lobby"
        );
    }
}
