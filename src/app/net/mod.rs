//! Online versus, shell side: a UDP lockstep session driving the same
//! versus mode local play uses (spec §7.6 fallback path; see sim::net for
//! why lockstep rather than GGRS rollback today).
//!
//! Dev-grade session setup: `PINCH_HOST=<port>` hosts as player 0,
//! `PINCH_JOIN=<addr:port>` joins as player 1. Both boot straight into the
//! arena; the match starts once the handshake completes.

use crate::sim::{HASH_INTERVAL, Lockstep, MAX_PLAYERS, PlayerAction};
use crate::transport::{
    Announcer, MatchTerms, NetMsg, SeriesStanding, UdpTransport, chat_from_wire, name_from_wire,
    table_from_wire, wire_table,
};
use bevy::prelude::*;
pub use peers::{Peer, PeerBook, Place};
use std::collections::VecDeque;

/// A bot connected straight to this host (route 2): the seat it holds
/// this round, which of the host's bots it is, and what it is called.
///
/// Named rather than a tuple: `(seat, id, ..)` is two small numbers next
/// to each other, kept apart only by their order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HostedBot {
    pub seat: u8,
    pub id: crate::bots::listener::BotId,
    pub name: String,
}

pub struct OnlineSession {
    pub transport: UdpTransport,
    pub session: Lockstep,
    /// Seats in the match, humans plus AI. The lockstep only carries the
    /// humans, so this is not `session.player_count()`.
    pub seats: u8,
    /// What the lobby agreed the match is: AI seats, map, gulls, round
    /// length, light, team scoring, and the board's seed. Every peer builds
    /// from these, so nothing about the round is a local opinion.
    pub terms: MatchTerms,
    /// The host's handmade beach, compressed, when the round is played on
    /// one. A generated arena travels as a seed in [`Self::terms`]; a
    /// level somebody built has to travel as itself, because no peer but
    /// the host has the file. Empty for the built-in maps.
    pub beach: Vec<u8>,
    /// The peers on the socket, one row each: the chair the launch plan
    /// gave it (`Queued` for one that turned up mid-round, and for every
    /// peer a joiner knows, since a joiner keeps no plan and answers
    /// nobody), what it calls itself, whether it asked to watch, and how
    /// long since it last said anything.
    ///
    /// The silence is the difference between a player who has *gone* and
    /// one who is merely behind: a machine still talking to us has not left
    /// the room, however far its inputs have fallen back. A joiner keeps
    /// the one row it has, the host, and calls the round off when that goes
    /// quiet.
    pub(crate) peers: PeerBook,
    /// This peer's seat is driven by a bot connected to this machine
    /// (route 1): it greets with the flag that says so, between rounds too.
    pub bot: bool,
    /// Joiner side: the host asked this peer to leave. Said with `Kicked`,
    /// which can arrive mid-round or on the results card as well as in the
    /// lobby, and walks the joiner out with the reason rather than leaving
    /// it greeting a host that has forgotten it.
    pub kicked: bool,
    /// Host side, route 2: the bots connected straight to this host, each
    /// with the seat it holds this round and what it is called. The host
    /// commits their inputs as well as its own.
    pub bots_here: Vec<HostedBot>,
    /// What each seat is called, agreed at the handshake so every peer
    /// shows the same table. Empty entries fall back to seat labels; local
    /// couch names never apply to an online round.
    pub names: [String; MAX_PLAYERS],
    /// What holds each seat, agreed with the names: a bot wears the robot
    /// on every screen at the table.
    pub kinds: [crate::sim::SeatKind; MAX_PLAYERS],
    /// The peers' state hashes against our own, and the first frame they
    /// disagreed on, if ever.
    hashes: HashCheck,
    /// Whether the picture has stopped moving, and on whom.
    stall: StallWatch,
    /// Seats given up on this round, each with the frame it was emptied
    /// from, so the shell can put an AI in each and say so once rather
    /// than every frame, and the host can keep telling the table.
    pub abandoned: Vec<(u8, u32)>,
    /// Lines said to the table since the shell last read them, each with
    /// who said it.
    ///
    /// Drained rather than kept: the round's own feed is where a line is
    /// shown, and the feed keeps its own history.
    pub heard: Vec<(String, String)>,
    /// What the people without a chair are doing: the tide vote, the
    /// crowd's size, the calls for the winner.
    pub stands: crate::app::spectators::Stands,
    /// The beach this session came from, and what it owes it.
    pub home: Home,
    /// A next round has been agreed and the session is armed for it; the
    /// shell reads this and walks everyone back into the arena. Cleared by
    /// whoever acts on it.
    pub next_round: bool,
    /// Where the series stands as the next round begins, as the host said
    /// it: the 1-based round number and the wins per seat, both re-dealt to
    /// this round's chairs. The shell folds it into its `Tournament` on the
    /// way into the arena, so a peer admitted mid-series joins the table's
    /// standings rather than starting its own. `None` outside a series.
    pub series_standing: Option<SeriesStanding>,
    /// Ticks of `Resume` still to repeat (see [`pump::RESUME_ECHOES`]).
    resume_echo: u8,
    /// A round joined late, or owed to someone who came late to it.
    pub catch_up: catch_up::CatchUp,
}

#[derive(Resource, Default)]
pub struct Online(pub Option<OnlineSession>);

/// Pass `msg` from peer `from` on to every other peer. Star topology: the
/// spokes cannot hear each other, so whatever one says reaches the rest
/// only by the hub repeating it, and inputs, pauses, resumes and chat
/// are all repeated this same way.
pub(crate) fn relay(transport: &UdpTransport, from: usize, msg: NetMsg) {
    for other in 0..transport.peer_count() {
        if other != from {
            transport.send_to(other, msg.clone());
        }
    }
}

/// Drain the session while the results card is up. See
/// [`OnlineSession::poll_between_rounds`] for why this cannot wait for the
/// sim: the sim is exactly what has stopped.
pub fn poll_between_rounds(time: Res<Time>, mut online: ResMut<Online>) {
    if let Some(session) = &mut online.0 {
        session.poll_between_rounds(time.delta_secs());
    }
}

pub(crate) mod catch_up;
mod crowd;
mod home;
mod peers;
mod presence;
mod pump;
mod rounds;
pub use home::{Home, LobbyReturn};
use presence::StallWatch;
pub(crate) use presence::{abandon_the_departed, catch_up_again, leave_a_hostless_round};
use pump::HashCheck;
pub use rounds::Invitation;
pub(crate) use rounds::{lockstep_for, seat_kinds, seat_names, seat_the_hosts_bots};

impl OnlineSession {
    pub fn new(
        transport: UdpTransport,
        session: Lockstep,
        seats: u8,
        terms: MatchTerms,
    ) -> OnlineSession {
        OnlineSession {
            transport,
            session,
            seats,
            terms,
            beach: Vec::new(),
            peers: PeerBook::default(),
            bot: false,
            kicked: false,
            bots_here: Vec::new(),
            names: Default::default(),
            kinds: Default::default(),
            hashes: HashCheck::default(),
            stall: StallWatch::default(),
            abandoned: Vec::new(),
            heard: Vec::new(),
            stands: Default::default(),
            home: Home::nowhere(),
            next_round: false,
            series_standing: None,
            resume_echo: 0,
            catch_up: Default::default(),
        }
    }

    /// Is this session the star's hub? Seat 0 hosts and relays. Public
    /// because the shell has to know who calls the next round.
    pub fn is_host(&self) -> bool {
        self.session.seat() == Some(0)
    }

    /// Whether this peer watches rather than plays.
    pub fn watching(&self) -> bool {
        self.session.watching()
    }

    /// The humans at the table and in line for it: the players in the
    /// lockstep plus every peer owed a chair at the next deal, queued or
    /// left without one at the launch. What the running beacon reports as
    /// taken.
    pub(crate) fn players_spoken_for(&self) -> u8 {
        let queued = self
            .peers
            .iter()
            .filter(|peer| peer.wants_a_chair())
            .count();
        (self.session.player_count() + queued).min(MAX_PLAYERS) as u8
    }
}

/// Parse the dev env vars into a ready session, if requested.
/// `PINCH_HOST=47777` or `PINCH_JOIN=192.168.1.10:47777`.
pub fn session_from_env() -> Option<OnlineSession> {
    // The direct hooks skip the lobby, so there is no Start to agree on:
    // both processes must be told the same terms. `PINCH_BOTS=n` seats n AI
    // players behind the two humans and has to be set on both sides.
    let bots: u8 = crate::app::dev::bots().unwrap_or(0);
    // Two humans and the AI behind them; MAX_PLAYERS is the ceiling, and
    // the clamp is on the bots so a wild PINCH_BOTS cannot wrap the sum
    // back under the two humans.
    let humans = 2u8;
    let bots = bots.min(MAX_PLAYERS as u8 - humans);
    let seats = humans + bots;
    let terms = MatchTerms {
        bots,
        ..crate::app::match_setup::terms(
            &crate::app::match_setup::MatchConfig::default(),
            crate::app::teams::TeamMode::Solo,
            0,
        )
    };
    // The name this player goes by online: their P1 name from settings,
    // read straight off disk because the dev hooks run before the app's
    // resources are wired up.
    let own = crate::app::settings::GameSettings::load().0.names[0].clone();
    if let Some(port) = crate::app::dev::direct_host() {
        let port: u16 = port.parse().ok()?;
        let transport = UdpTransport::host(port).ok()?;
        info!("hosting on UDP port {port}, waiting for a peer ({seats} seats)");
        let mut session = OnlineSession::new(
            transport,
            Lockstep::new(0, vec![0, 1], crate::sim::DEFAULT_DELAY),
            seats,
            terms,
        );
        session.names[0] = own;
        return Some(session);
    }
    if let Some(addr) = crate::app::dev::direct_join() {
        let transport = UdpTransport::join(addr.as_str()).ok()?;
        transport.send(NetMsg::hello(&own));
        info!("joining {addr} ({seats} seats)");
        let mut session = OnlineSession::new(
            transport,
            Lockstep::new(1, vec![0, 1], crate::sim::DEFAULT_DELAY),
            seats,
            terms,
        );
        session.names[1] = own;
        return Some(session);
    }
    None
}

#[cfg(test)]
mod spectator_tests {
    use super::*;
    use crate::sim::DEFAULT_DELAY;

    pub(super) fn hosting_session(seed: u64) -> OnlineSession {
        OnlineSession::new(
            UdpTransport::host(0).expect("game socket"),
            Lockstep::new(0, vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms {
                seed,
                ..MatchTerms::default()
            },
        )
    }

    /// Who counts as watching this round, which decides both whose vote
    /// the host takes and what number the table is shown.
    ///
    /// Not `Peer::watches`, which is a wish as much as a place: it is true
    /// of somebody in line for the *next* round, who is sent no frames and
    /// sees nothing, and of a seated player that has asked to watch next
    /// time while still playing this one. Counting either had a player in
    /// the audience for its own round, and let a peer with no board vote
    /// on what happened on it.
    #[test]
    fn only_someone_in_the_round_without_a_chair_is_watching_it() {
        let mut session = hosting_session(11);
        // Three peers in the launch plan: one seated, one watching, one
        // seated but asking to watch next round. Then a fourth in line.
        session.peers.row(0).place = Place::Seated(1);
        session.peers.row(1).place = Place::Watching;
        session.peers.row(2).place = Place::Seated(2);
        session.peers.row(2).watch = true;
        session.peers.row(3).place = Place::Queued;
        session.peers.row(3).watch = true;

        assert!(!session.watching_this_round(0), "a player");
        assert!(session.watching_this_round(1), "a watcher");
        assert!(
            !session.watching_this_round(2),
            "still playing, however it means to spend the next round"
        );
        assert!(
            !session.watching_this_round(3),
            "in line, sent no frames, watching nothing"
        );
        assert_eq!(session.watchers_in_round(), 1, "one of the four");
    }

    /// A pick is counted only from someone watching this round, and only
    /// by the host, which is the only peer that counts anything.
    #[test]
    fn a_call_for_the_winner_is_taken_from_a_watcher_while_the_calls_are_open() {
        use crate::sim::InputMsg;
        let mut session = hosting_session(5);
        session.peers.row(0).place = Place::Seated(1);
        session.peers.row(1).place = Place::Watching;
        session.peers.row(2).place = Place::LateWatching;
        session.peers.row(3).place = Place::Queued;
        session.take_pick(true, 0, 0);
        session.take_pick(true, 1, 1);
        session.take_pick(true, 2, 1);
        session.take_pick(true, 3, 0);
        session.take_pick(true, 1, 9);
        let picks = session.crowd_picks_now();
        assert_eq!(
            picks.counts,
            [0, 2, 0, 0, 0, 0],
            "two watchers; the player and the queue unheard"
        );
        assert!(picks.open > 0);
        // A spectator may change its mind, and a spoke counts nothing.
        session.take_pick(true, 1, 0);
        session.take_pick(false, 2, 0);
        assert_eq!(session.crowd_picks_now().counts, [1, 1, 0, 0, 0, 0]);

        // Past the first half minute the calls are in.
        let close = crate::app::spectators::PICKS_OPEN_FOR;
        for frame in 0..close {
            session.session.commit_local(PlayerAction::None);
            session.session.receive(InputMsg {
                player: 1,
                frame: DEFAULT_DELAY + frame,
                action: PlayerAction::None,
            });
            while session.session.advance().is_some() {}
        }
        assert!(session.session.frame() >= close);
        assert_eq!(session.crowd_picks_now().open, 0);
        session.take_pick(true, 1, 1);
        assert_eq!(
            session.crowd_picks_now().counts,
            [1, 1, 0, 0, 0, 0],
            "a call after the close is not taken"
        );
    }

    #[test]
    fn a_vote_is_taken_only_from_someone_watching() {
        let sand = crate::sim::TideEvent::FreshSand.index() as u8;
        let open = |session: &OnlineSession| session.stands.tide.votes.open().is_some();

        // Seated: a player may not call the tide down on its own table.
        let mut session = hosting_session(1);
        session.peers.row(0).place = Place::Seated(1);
        session.take_vote(true, 0, sand);
        assert!(!open(&session), "a player at the table");

        // In line for the next round: sent no frames, sees no board.
        let mut session = hosting_session(2);
        session.peers.row(0).place = Place::Queued;
        session.take_vote(true, 0, sand);
        assert!(!open(&session), "a peer in the queue");

        // Watching: heard.
        let mut session = hosting_session(3);
        session.peers.row(0).place = Place::Watching;
        session.take_vote(true, 0, sand);
        assert!(open(&session), "a spectator");

        // And a spoke counts nothing, whoever sent it: only the hub tallies.
        let mut session = hosting_session(4);
        session.peers.row(0).place = Place::Watching;
        session.take_vote(false, 0, sand);
        assert!(!open(&session), "not the host");
    }

    /// One watcher, one vote. The card can be reopened and pressed again
    /// inside the window, and every press used to count: one keen watcher
    /// outvoted a room. Now the latest press is the watcher changing its
    /// mind.
    #[test]
    fn a_watcher_pressing_again_changes_its_vote_rather_than_adding_one() {
        use crate::sim::TideEvent;
        let mut session = hosting_session(6);
        session.peers.row(0).place = Place::Watching;
        session.peers.row(1).place = Place::Watching;
        let (sand, swap, mania) = (
            TideEvent::FreshSand.index() as u8,
            TideEvent::CastleSwap.index() as u8,
            TideEvent::CrabMania.index() as u8,
        );
        // Peer 0 stuffs the box for a swap; peer 1 wants fresh sand.
        for _ in 0..5 {
            session.take_vote(true, 0, swap);
        }
        session.take_vote(true, 1, sand);
        // One each, a tie, which goes to the earlier event on the list.
        assert_eq!(
            session.stands.tide.votes.settle(10.0),
            Some(TideEvent::FreshSand),
            "five presses are one vote"
        );

        // And a vote moves with its voter when a peer before it goes.
        let mut session = hosting_session(7);
        for peer in 0..3 {
            session.peers.row(peer).place = Place::Watching;
        }
        session.take_vote(true, 0, mania);
        session.take_vote(true, 1, sand);
        session.take_vote(true, 2, sand);
        session.forget_peer(0);
        // Old peer 2 is now peer 1, and changes its mind: still two votes
        // in all, not three.
        session.take_vote(true, 1, mania);
        assert_eq!(
            session.stands.tide.votes.settle(10.0),
            Some(TideEvent::CrabMania)
        );
    }

    /// A direct `PINCH_HOST` pair keeps no launch plan, and
    /// `follows_the_round` reads an empty plan as "everybody hears" so the
    /// two keep talking to the one peer they have. Spectating is the place
    /// a peer was dealt, not something derived from that: read the other
    /// way, the host had its opponent down as a spectator, counted in the
    /// audience and allowed to vote on the game it was playing.
    #[test]
    fn a_direct_pair_has_no_spectators_in_it() {
        let mut session = hosting_session(5);
        // No plan was ever dealt, so the one peer keeps the default place.
        session.peers.reach(1);
        assert_eq!(session.peers.planned(), 0, "no plan at all");
        assert!(
            session.peers.follows_the_round(0),
            "and it is sent the round regardless, which is the point"
        );

        assert!(!session.watching_this_round(0), "it is the other player");
        assert_eq!(session.watchers_in_round(), 0, "an audience of nobody");

        session.take_vote(true, 0, crate::sim::TideEvent::FreshSand.index() as u8);
        assert!(
            session.stands.tide.votes.open().is_none(),
            "and it cannot call the tide onto the game it is playing"
        );
    }

    /// A spectator that walks away is forgotten, which nothing used to do:
    /// the round gives up only on *seats*, through `awaiting`, and a
    /// spectator is awaited by nobody. Left on the socket it was fed the
    /// round for the rest of it and counted in the audience on screen.
    #[test]
    fn a_spectator_that_goes_quiet_is_forgotten_and_the_rest_are_left_alone() {
        let mut session = hosting_session(6);
        session.peers.row(0).place = Place::Seated(1);
        session.peers.row(1).place = Place::Watching;
        session.peers.row(2).place = Place::Watching;
        session.peers.row(3).place = Place::Queued;
        // Everyone quiet a long while, and one of the watchers talking.
        for peer in 0..4 {
            session.peers.row(peer).silence = 60.0;
        }
        session.peers.row(1).silence = 0.0;
        let before = session.peers.len();

        session.forget_gone_watchers();

        assert_eq!(session.peers.len(), before - 1, "one of the four went");
        assert_eq!(
            session.peers.get(0).map(|p| p.place),
            Some(Place::Seated(1)),
            "the seat is the round's own business, not this sweep's"
        );
        assert_eq!(
            session.peers.get(1).map(|p| p.place),
            Some(Place::Watching),
            "the watcher that is still talking stays"
        );
        assert_eq!(
            session.peers.get(2).map(|p| p.place),
            Some(Place::Queued),
            "and the one in line costs nothing, so it waits for the card"
        );
    }

    /// An empty name is the beach's own voice here: it is what announces
    /// a called event. So a peer that has a name may not drop it and speak
    /// as the room, and the lobby has refused that for as long as it has
    /// had chat. The round took the wire's word for it.
    ///
    /// Host side only, though. A spoke takes the vetted line as it stands,
    /// or it would stamp the host's name onto the one line that is meant
    /// to carry none.
    #[test]
    fn a_named_peer_cannot_speak_as_the_beach_itself() {
        let mut session = hosting_session(7);
        session.peers.row(0).name = "Cy".into();
        let forged = crate::transport::wire_chat("Anna has left the beach");

        let said = session.take_chat(true, 0, crate::transport::wire_name(""), forged);
        assert_eq!(
            said,
            Some(("Cy".to_string(), "Anna has left the beach".to_string())),
            "the host gives a named peer its own name back"
        );

        // A peer it knows nothing about keeps whatever it claimed. There
        // is one left: a greeting the host never received, which is a peer
        // it is about to learn the name of anyway.
        let stranger = session.take_chat(true, 1, crate::transport::wire_name("Dee"), forged);
        assert_eq!(stranger.map(|(who, _)| who), Some("Dee".to_string()));

        // But one that claims nothing and is known as nothing is refused,
        // rather than repeated to the table as the beach talking. Found by
        // running it: a watcher with no name set said a line that came out
        // looking exactly like the notice a called event makes.
        let nameless = session.take_chat(true, 2, crate::transport::wire_name(""), forged);
        assert_eq!(nameless, None, "nobody speaks as the room");

        // And a spoke repeats what the hub handed it, untouched.
        let spoke = session.take_chat(false, 0, crate::transport::wire_name(""), forged);
        assert_eq!(
            spoke.map(|(who, _)| who),
            Some(String::new()),
            "the beach's own voice reaches a joiner nameless"
        );
    }

    /// A spectator is vetted like anybody else, which it could not be
    /// while its greeting carried no name.
    ///
    /// Found by watching a real one: `Watch` was the one greeting with
    /// nothing in it, so the host had no name to check a spectator's chat
    /// against and took the line's own word for who said it. That is the
    /// whole of what the check exists to stop, and it was open to exactly
    /// the peers nobody at the table can see.
    #[test]
    fn a_spectator_cannot_speak_under_a_players_name() {
        let mut session = hosting_session(7);
        // As the host writes it down from the greeting, watcher or not.
        session.remember_peer_name(0, "Dee");
        let forged = crate::transport::wire_chat("I fold, take my castle");

        let said = session.take_chat(true, 0, crate::transport::wire_name("Bo"), forged);
        assert_eq!(
            said,
            Some(("Dee".to_string(), "I fold, take my castle".to_string())),
            "the crowd speaks under the name it turned up with"
        );
    }
}

#[cfg(test)]
mod table_tests {
    use super::*;
    use crate::sim::DEFAULT_DELAY;

    fn session(local: Option<u8>, port: u16) -> OnlineSession {
        let transport = match local {
            Some(0) => UdpTransport::host(0).expect("host socket"),
            _ => UdpTransport::join(("127.0.0.1", port)).expect("join"),
        };
        OnlineSession::new(
            transport,
            match local {
                Some(seat) => Lockstep::new(seat, vec![0, 1], DEFAULT_DELAY),
                None => Lockstep::observer(vec![0, 1], DEFAULT_DELAY),
            },
            2,
            MatchTerms::default(),
        )
    }

    /// The running beacon says how many chairs are spoken for: the humans
    /// in the lockstep and everyone in line who did not ask to watch.
    #[test]
    fn the_beacon_counts_the_queue_as_taken() {
        let mut host = session(Some(0), 0);
        assert_eq!(host.players_spoken_for(), 2, "the two at the table");
        host.peers.row(0).name = "Bo".into();
        assert_eq!(host.players_spoken_for(), 3, "one in line for a chair");
        host.peers.row(1).watch = true;
        assert_eq!(host.players_spoken_for(), 3, "a watcher wants no chair");
        for peer in 2..8 {
            host.peers.row(peer).name = "Cy".into();
        }
        assert_eq!(
            host.players_spoken_for(),
            MAX_PLAYERS as u8,
            "and never more chairs than the table has"
        );
    }

    /// A joiner reads its own seat in the host's abandonment notice: that
    /// is how it learns it was dropped, rather than waiting out the host
    /// and being told the host had left. Somebody else's seat, or the
    /// host's own view, says nothing of the kind.
    #[test]
    fn a_seat_told_of_its_own_abandonment_knows_it_was_dropped() {
        let host = session(Some(0), 0);
        let port = host.transport.local_addr().expect("addr").port();
        let mut joiner = session(Some(1), port);
        assert!(!joiner.dropped(), "nothing said yet");
        joiner.abandoned.push((0, 30));
        assert!(!joiner.dropped(), "another seat's abandonment is theirs");
        joiner.abandoned.push((1, 30));
        assert!(joiner.dropped(), "its own is the word it was waiting for");
        let mut host = host;
        host.abandoned.push((0, 30));
        assert!(!host.dropped(), "the host drops nobody it is");
        let mut watcher = session(None, port);
        watcher.abandoned.push((1, 30));
        assert!(
            !watcher.dropped(),
            "a spectator holds no seat to be dropped from"
        );
    }
}
