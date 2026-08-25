//! End-to-end online lockstep over real loopback UDP: two peers, each with
//! their own socket, session, and board, must simulate identical matches and
//! agree on every exchanged state hash.

use pinch_points::sim::{
    Board, CrabKind, DEFAULT_DELAY, Direction, Handedness, Lockstep, PlayerAction, Spawner,
    TileKind,
};
use pinch_points::transport::{MatchTerms, NetMsg, UdpTransport};

fn arena(seed: u64) -> Board {
    let mut board = Board::new(12, 9, seed);
    board.set_tile(1, 1, TileKind::Castle(0));
    board.set_tile(10, 7, TileKind::Castle(1));
    board.set_tile(
        0,
        4,
        TileKind::Spawner(Spawner {
            dir: Direction::Right,
            period: 40,
        }),
    );
    board.spawn_crab(6, 6, Direction::Left, Handedness::Right, CrabKind::Giant);
    board.spawn_gull(5, 0, Direction::Down);
    board.set_gull_period(200);
    board
}

struct Peer {
    transport: UdpTransport,
    session: Lockstep,
    board: Board,
    peer_hashes: Vec<(u32, u64)>,
    own_hashes: Vec<(u32, u64)>,
}

impl Peer {
    fn step(&mut self, local_action: PlayerAction) {
        let _ = self.session.commit_local(local_action);
        // Redundant resend of the recent tail: survives packet loss and the
        // pre-handshake window where the host cannot send yet.
        for &msg in self.session.recent_commits() {
            self.transport.send(NetMsg::Input(msg));
        }
        for (msg, _) in self.transport.recv_all() {
            match msg {
                NetMsg::Hello { .. } | NetMsg::Watch => {}
                NetMsg::Input(input) => self.session.receive(input),
                NetMsg::Hash { frame, hash } => self.peer_hashes.push((frame, hash)),
                NetMsg::Pause { frame } => self.session.receive_pause(frame),
                NetMsg::Resume { frame } => {
                    self.session.receive_resume(frame);
                }
                NetMsg::Start { .. }
                | NetMsg::Incompatible { .. }
                | NetMsg::Queued { .. }
                | NetMsg::Chat { .. }
                | NetMsg::Roster { .. }
                | NetMsg::Abandoned { .. } => {}
            }
        }
        while let Some(actions) = self.session.advance() {
            self.board.tick(&actions);
            let frame = self.session.frame();
            if frame.is_multiple_of(30) {
                let hash = self.board.state_hash();
                self.own_hashes.push((frame, hash));
                self.transport.send(NetMsg::Hash { frame, hash });
            }
        }
    }
}

#[test]
fn two_udp_peers_play_a_bit_identical_match() {
    // Port 0: the OS picks a free port; the joiner reads it back.
    let host_transport = UdpTransport::host(0).expect("bind host");
    let port = host_transport.local_addr().expect("local addr").port();
    let join_transport = UdpTransport::join(("127.0.0.1", port)).expect("join");
    join_transport.send(NetMsg::hello("Joiner"));

    let players = vec![0u8, 1u8];
    let mut host = Peer {
        transport: host_transport,
        session: Lockstep::new(0, players.clone(), DEFAULT_DELAY),
        board: arena(0xA11CE),
        peer_hashes: vec![],
        own_hashes: vec![],
    };
    let mut join = Peer {
        transport: join_transport,
        session: Lockstep::new(1, players, DEFAULT_DELAY),
        board: arena(0xA11CE),
        peer_hashes: vec![],
        own_hashes: vec![],
    };

    for step in 0u32..400 {
        let host_action = if step % 45 == 10 {
            PlayerAction::Place {
                x: (step % 12) as u8,
                y: 2,
                dir: Direction::Down,
            }
        } else {
            PlayerAction::None
        };
        let join_action = if step % 60 == 20 {
            PlayerAction::Place {
                x: 6,
                y: (step % 9) as u8,
                dir: Direction::Right,
            }
        } else {
            PlayerAction::None
        };
        host.step(host_action);
        join.step(join_action);
        // Loopback is fast but not instantaneous; tiny pause keeps datagrams
        // flowing without making the test slow.
        if step % 16 == 0 {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    // Drain the tail. Peers may legitimately end a frame apart (a commit
    // skipped at the lead cap offsets the streams), so the meaningful check
    // is hash agreement at every common frame, not frame equality.
    for _ in 0..30 {
        host.step(PlayerAction::None);
        join.step(PlayerAction::None);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    assert!(
        host.session.frame() > 350 && join.session.frame() > 350,
        "sessions progressed (host {}, join {})",
        host.session.frame(),
        join.session.frame()
    );
    // Cross-peer comparison: a hash the host RECEIVED (computed by join)
    // must equal what the HOST computed for the same frame, and vice versa.
    let mut compared = 0;
    for &(frame, join_hash) in &host.peer_hashes {
        if let Some(&(_, host_hash)) = host.own_hashes.iter().find(|(f, _)| *f == frame) {
            assert_eq!(join_hash, host_hash, "desync at frame {frame}");
            compared += 1;
        }
    }
    for &(frame, host_hash) in &join.peer_hashes {
        if let Some(&(_, join_hash)) = join.own_hashes.iter().find(|(f, _)| *f == frame) {
            assert_eq!(host_hash, join_hash, "desync at frame {frame}");
            compared += 1;
        }
    }
    assert!(
        compared >= 10,
        "hash exchanges actually happened ({compared})"
    );
    // Agreement must extend deep into the match, not just the quiet opening.
    let latest_agreed = host
        .peer_hashes
        .iter()
        .filter(|(f, _)| host.own_hashes.iter().any(|(of, _)| of == f))
        .map(|&(f, _)| f)
        .max()
        .unwrap_or(0);
    assert!(
        latest_agreed >= 390,
        "late-game frames were cross-checked (latest {latest_agreed})"
    );
}

/// Pausing an online match: the peers agree on a frame, both stop dead on
/// it, and resuming carries on bit-identical. Run over real loopback
/// sockets through the same `pump` the game uses, because surviving the
/// wire is the one thing the protocol is for.
#[test]
fn a_pause_stops_both_peers_on_the_same_frame() {
    use pinch_points::app::net::OnlineSession;

    let host_transport = UdpTransport::host(0).expect("bind host");
    let port = host_transport.local_addr().expect("local addr").port();
    let join_transport = UdpTransport::join(("127.0.0.1", port)).expect("join");
    join_transport.send(NetMsg::hello("Joiner"));
    let players = vec![0u8, 1u8];
    let terms = MatchTerms::default();
    let mut host = OnlineSession::new(
        host_transport,
        Lockstep::new(0, players.clone(), DEFAULT_DELAY),
        2,
        terms,
    );
    let mut join = OnlineSession::new(
        join_transport,
        Lockstep::new(1, players, DEFAULT_DELAY),
        2,
        terms,
    );
    std::thread::sleep(std::time::Duration::from_millis(5));

    let mut boards: Vec<Board> = (0..2).map(|_| arena(0x9A05E)).collect();
    let step_both = |host: &mut OnlineSession, join: &mut OnlineSession, boards: &mut [Board]| {
        for (index, session) in [host, join].into_iter().enumerate() {
            let board = &mut boards[index];
            session.pump(PlayerAction::None, |net| {
                while let Some(actions) = net.session.advance() {
                    board.tick(&actions);
                    net.after_frame(board.state_hash());
                }
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    };

    for _ in 0..60 {
        step_both(&mut host, &mut join, &mut boards);
    }
    let before = host.session.frame();
    assert!(before > 30, "the match got going first ({before})");

    // The host hits Escape.
    host.request_pause();
    for _ in 0..60 {
        step_both(&mut host, &mut join, &mut boards);
    }
    let pause_frame = host.session.pause_frame().expect("host is paused");
    assert_eq!(
        join.session.pause_frame(),
        Some(pause_frame),
        "the joiner heard the same pause frame"
    );
    assert!(host.session.frozen() && join.session.frozen(), "both froze");
    assert_eq!(host.session.frame(), pause_frame, "stopped on the frame");
    assert_eq!(join.session.frame(), pause_frame, "and so did the peer");
    assert_eq!(
        boards[0].state_hash(),
        boards[1].state_hash(),
        "frozen on identical state"
    );

    // Ten more ticks change nothing: a pause is a pause.
    let frozen_hash = boards[0].state_hash();
    for _ in 0..10 {
        step_both(&mut host, &mut join, &mut boards);
    }
    assert_eq!(host.session.frame(), pause_frame);
    assert_eq!(boards[0].state_hash(), frozen_hash);

    // The joiner is the one who presses Continue.
    join.request_resume();
    for step in 0..80 {
        step_both(&mut host, &mut join, &mut boards);
        // Playing on every tick from the resume, not most of them: the
        // last `Pause` echoes cross the `Resume`, and taken at face value
        // they re-paused the peer that had just resumed, which echoed
        // them back, and the pair flapped with a period of three ticks
        // (an eighty-tick check happened to land on the playing phase).
        if step >= 3 {
            assert!(
                !host.session.paused() && !join.session.paused(),
                "still flapping {step} ticks after the resume"
            );
        }
    }
    assert!(!host.session.paused() && !join.session.paused(), "playing");
    assert!(
        host.session.frame() > pause_frame + 30,
        "the match ran on after the pause ({} vs {pause_frame})",
        host.session.frame()
    );
    assert!(
        host.desync_at().is_none() && join.desync_at().is_none(),
        "pausing never desynced the peers"
    );
}

/// The full 4-player star: one host relays between three joiners over real
/// loopback sockets, using the same OnlineSession pump the game runs.
/// Previously this topology was only validated by manual multi-instance
/// dogfooding.
#[test]
fn four_player_star_relay_stays_bit_identical() {
    use pinch_points::app::net::OnlineSession;

    fn star_arena(seed: u64) -> Board {
        let mut board = arena(seed);
        board.set_tile(10, 1, TileKind::Castle(2));
        board.set_tile(1, 7, TileKind::Castle(3));
        board
    }

    let host_transport = UdpTransport::host(0).expect("bind host");
    let port = host_transport.local_addr().expect("local addr").port();
    let seats: Vec<u8> = vec![0, 1, 2, 3];
    let mut joiners: Vec<OnlineSession> = (1..4u8)
        .map(|seat| {
            let transport = UdpTransport::join(("127.0.0.1", port)).expect("join");
            transport.send(NetMsg::hello("Peer"));
            OnlineSession::new(
                transport,
                Lockstep::new(seat, seats.clone(), DEFAULT_DELAY),
                4,
                MatchTerms::default(),
            )
        })
        .collect();
    let mut host = OnlineSession::new(
        host_transport,
        Lockstep::new(0, seats.clone(), DEFAULT_DELAY),
        4,
        MatchTerms::default(),
    );
    // The host must register every joiner's socket before relaying works.
    std::thread::sleep(std::time::Duration::from_millis(5));

    let mut boards: Vec<Board> = (0..4).map(|_| star_arena(0x5EA7)).collect();
    let mut hashes: Vec<Vec<(u32, u64)>> = vec![vec![]; 4];

    for step in 0u32..360 {
        for seat in 0..4usize {
            let action = if step % (40 + seat as u32 * 7) == 5 {
                PlayerAction::Place {
                    x: (2 + seat * 2) as u8,
                    y: (2 + seat) as u8,
                    dir: Direction::Down,
                }
            } else {
                PlayerAction::None
            };
            let (session, board) = if seat == 0 {
                (&mut host, &mut boards[0])
            } else {
                (&mut joiners[seat - 1], &mut boards[seat])
            };
            let hash_log = &mut hashes[seat];
            session.pump(action, |net| {
                while let Some(actions) = net.session.advance() {
                    board.tick(&actions);
                    let frame = net.session.frame();
                    if frame.is_multiple_of(30) {
                        hash_log.push((frame, board.state_hash()));
                    }
                }
            });
        }
        if step % 8 == 0 {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    let min_frame = std::iter::once(host.session.frame())
        .chain(joiners.iter().map(|j| j.session.frame()))
        .min()
        .unwrap();
    assert!(
        min_frame > 300,
        "all four sessions progressed ({min_frame})"
    );
    assert!(
        host.desync_at().is_none() && joiners.iter().all(|j| j.desync_at().is_none()),
        "no session flagged a desync"
    );
    // Every pair of peers must agree at every common hash frame.
    let mut compared = 0;
    for a in 0..4 {
        for b in (a + 1)..4 {
            for &(frame, hash_a) in &hashes[a] {
                if let Some(&(_, hash_b)) = hashes[b].iter().find(|(f, _)| *f == frame) {
                    assert_eq!(hash_a, hash_b, "peers {a}/{b} desync at frame {frame}");
                    compared += 1;
                }
            }
        }
    }
    assert!(compared > 30, "hash overlap compared ({compared})");
}

/// Two players and an onlooker over real sockets: the watcher simulates the
/// same round from the relayed inputs, agrees with both players at every hash
/// frame, and (the property that makes a spectator safe) is never waited
/// for.
///
/// It joins with them rather than mid-round: a lockstep session replays from
/// frame zero and the resend tail is forty commits deep, so a peer that
/// arrives late can never fill the frames it missed. Watching from partway
/// through needs a mid-round snapshot, which the level format cannot carry
/// (see the testing note in docs/backlog.md).
#[test]
fn a_spectator_sees_the_same_round_without_holding_it_up() {
    use pinch_points::app::net::OnlineSession;

    let host_transport = UdpTransport::host(0).expect("bind host");
    let port = host_transport.local_addr().expect("local addr").port();
    let players: Vec<u8> = vec![0, 1];
    let join_transport = UdpTransport::join(("127.0.0.1", port)).expect("join");
    join_transport.send(NetMsg::hello("Joiner"));

    let terms = MatchTerms::default();
    let mut host = OnlineSession::new(
        host_transport,
        Lockstep::new(0, players.clone(), DEFAULT_DELAY),
        2,
        terms,
    );
    let mut join = OnlineSession::new(
        join_transport,
        Lockstep::new(1, players.clone(), DEFAULT_DELAY),
        2,
        terms,
    );
    std::thread::sleep(std::time::Duration::from_millis(5));

    let mut boards: Vec<Board> = (0..3).map(|_| arena(0x5EA7)).collect();
    let mut hashes: Vec<Vec<(u32, u64)>> = vec![vec![]; 3];
    let watch_transport = UdpTransport::join(("127.0.0.1", port)).expect("watch");
    watch_transport.send(NetMsg::Watch);
    let mut watcher = Some(OnlineSession::new(
        watch_transport,
        Lockstep::observer(players.clone(), DEFAULT_DELAY),
        2,
        terms,
    ));
    std::thread::sleep(std::time::Duration::from_millis(5));

    for step in 0u32..400 {
        for seat in 0..3usize {
            let action = if seat < 2 && step % (40 + seat as u32 * 7) == 5 {
                PlayerAction::Place {
                    x: (2 + seat * 2) as u8,
                    y: (2 + seat) as u8,
                    dir: Direction::Down,
                }
            } else {
                PlayerAction::None
            };
            let session = match seat {
                0 => Some(&mut host),
                1 => Some(&mut join),
                _ => watcher.as_mut(),
            };
            let Some(session) = session else {
                continue;
            };
            let board = &mut boards[seat];
            let hash_log = &mut hashes[seat];
            session.pump(action, |net| {
                while let Some(actions) = net.session.advance() {
                    board.tick(&actions);
                    let frame = net.session.frame();
                    if frame.is_multiple_of(30) {
                        hash_log.push((frame, board.state_hash()));
                    }
                }
            });
        }
        if step % 8 == 0 {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    let watcher = watcher.expect("the watcher joined");
    assert!(
        host.session.frame() > 300 && join.session.frame() > 300,
        "the players ran their round"
    );
    assert!(
        watcher.session.frame() > 300,
        "the watcher caught up ({})",
        watcher.session.frame()
    );
    assert!(
        host.desync_at().is_none() && join.desync_at().is_none() && watcher.desync_at().is_none(),
        "no session flagged a desync"
    );
    // The watcher's board is the players' board at every frame all three
    // reached.
    let common: Vec<(u32, u64)> = hashes[2]
        .iter()
        .copied()
        .filter(|(frame, _)| hashes[0].iter().any(|(f, _)| f == frame))
        .collect();
    assert!(common.len() > 5, "not enough shared frames to compare");
    for (frame, watched) in common {
        for (seat, log) in hashes.iter().take(2).enumerate() {
            if let Some((_, played)) = log.iter().find(|(f, _)| *f == frame) {
                assert_eq!(
                    watched, *played,
                    "watcher and seat {seat} disagreed at frame {frame}"
                );
            }
        }
    }
}

/// Peers whose boards genuinely differ must both be told so, loudly. The
/// check is symmetric by design: each side sends its own state hash every
/// `HASH_INTERVAL` frames and compares every one it receives, so neither
/// player plays on believing the round is fine while the other's screen
/// shows a different beach.
#[test]
fn a_peer_that_diverges_is_flagged_by_both_ends() {
    use pinch_points::app::net::OnlineSession;
    use pinch_points::sim::HASH_INTERVAL;

    let host_transport = UdpTransport::host(0).expect("bind host");
    let port = host_transport.local_addr().expect("local addr").port();
    let join_transport = UdpTransport::join(("127.0.0.1", port)).expect("join");
    join_transport.send(NetMsg::hello("Joiner"));
    let players = vec![0u8, 1u8];
    let terms = MatchTerms::default();
    let mut host = OnlineSession::new(
        host_transport,
        Lockstep::new(0, players.clone(), DEFAULT_DELAY),
        2,
        terms,
    );
    let mut join = OnlineSession::new(
        join_transport,
        Lockstep::new(1, players, DEFAULT_DELAY),
        2,
        terms,
    );
    std::thread::sleep(std::time::Duration::from_millis(5));

    // The divergence, there from frame zero: one rock the host's board
    // does not have, as if a level edit had failed to travel.
    let mut boards: Vec<Board> = (0..2).map(|_| arena(0xD1FF)).collect();
    boards[1].set_tile(4, 4, TileKind::Rock);

    for step in 0u32..150 {
        for (index, session) in [&mut host, &mut join].into_iter().enumerate() {
            let board = &mut boards[index];
            session.pump(PlayerAction::None, |net| {
                while let Some(actions) = net.session.advance() {
                    board.tick(&actions);
                    net.after_frame(board.state_hash());
                }
            });
        }
        if step % 8 == 0 {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    assert!(
        host.session.frame() > 2 * HASH_INTERVAL,
        "the match got far enough to exchange hashes ({})",
        host.session.frame()
    );
    let host_saw = host.desync_at().expect("the host flagged the desync");
    let join_saw = join.desync_at().expect("the joiner flagged it too");
    // Flagged on an exchanged hash frame at or after the divergence, which
    // sits at frame zero here, so the first exchange is the earliest frame
    // either side can prove anything about.
    for (who, flagged) in [("host", host_saw), ("joiner", join_saw)] {
        assert!(
            flagged.is_multiple_of(HASH_INTERVAL) && flagged >= HASH_INTERVAL,
            "the {who} flagged frame {flagged}, not an exchanged hash frame"
        );
    }
}

/// One launch, one board: the invitation (the terms and, for a handmade
/// beach, the level itself riding in the `Start`) must build every peer
/// the identical board, or the round desyncs on its first exchanged hash.
/// Built through the real datagram and the real builders, because the two
/// agreeing is the whole contract.
#[test]
fn a_launched_round_seats_every_peer_on_the_same_beach() {
    use pinch_points::app::match_setup::{self, MapChoice};
    use pinch_points::sim::MAX_PLAYERS;
    use pinch_points::transport::wire_name;

    // A two-castle beach somebody built: only the host has this text, so
    // it travels in the invitation, compressed the way the lobby packs it.
    let text = "name: Reef\nposts: 2\nkind: arena\ncrab: 1,1 R R common\nmap:\n\
                +-+-+-+-+-+\n|0 . . . .|\n+ + + + + +\n|. . . . 1|\n\
                + + + + + +\n|. . . . .|\n+-+-+-+-+-+\n";
    let level = pinch_points::sim::Level::parse(text).expect("a level");
    let packed = pinch_points::lzw::compress(level.to_text().as_bytes(), 8);
    let custom = MapChoice::ALL
        .iter()
        .position(|&map| map == MapChoice::Custom)
        .expect("on the dial") as u8;
    let terms = MatchTerms {
        map: custom,
        gulls: 2,
        round: 0,
        seed: 0xBEAC4,
        ..MatchTerms::default()
    };

    let invitation = NetMsg::Start {
        seats: 2,
        seat: Some(1),
        terms,
        names: std::array::from_fn(|i| wire_name(&format!("Seat {i}"))),
        standing: None,
        beach: packed.clone(),
    };
    let Some(NetMsg::Start {
        terms: heard,
        beach: sent,
        ..
    }) = NetMsg::decode(&invitation.encode())
    else {
        panic!("the invitation did not survive the wire");
    };
    assert_eq!(heard, terms, "the terms rode along unchanged");

    // The host builds from its own copy, the joiner from the datagram's.
    let mut table = [
        match_setup::board_from(&terms, 2, &packed),
        match_setup::board_from(&heard, 2, &sent),
    ];
    assert_eq!(table[0].width(), 5, "the handmade beach, not a fallback");
    assert_eq!(
        table[0].state_hash(),
        table[1].state_hash(),
        "the same board at frame zero"
    );
    for _ in 0..100 {
        for board in &mut table {
            board.tick(&[PlayerAction::None; MAX_PLAYERS]);
        }
    }
    assert_eq!(
        table[0].state_hash(),
        table[1].state_hash(),
        "and still the same after a hundred idle ticks"
    );

    // With no beach riding along, the two builders must be one builder:
    // `board_from` falls back to `board_for`, and hashing them against
    // each other on every map stop is what keeps an option one of them
    // sets (wrap, say, which only the open ocean turns on) from ever
    // drifting out of the other.
    for map in 0..MapChoice::ALL.len() as u8 {
        for seats in [2u8, 5] {
            let terms = MatchTerms {
                map,
                gulls: 2,
                round: 0,
                seed: 0x5EED + u64::from(map),
                ..MatchTerms::default()
            };
            assert_eq!(
                match_setup::board_from(&terms, seats, &[]).state_hash(),
                match_setup::board_for(&terms, seats).state_hash(),
                "map {map} for {seats} seats built differently with no beach"
            );
        }
    }
}
