use super::*;

/// A host speaking for a bot's seat commits an input for it on every
/// frame it commits its own, and a joiner plays that seat like any
/// other player's: frames advance on both, with the bot's moves in.
#[test]
fn a_host_speaks_for_its_bots_and_the_table_hears_them() {
    let players = vec![0, 1, 2];
    let mut host = Lockstep::new(0, players.clone(), DEFAULT_DELAY);
    host.speak_for(&[2, 7]);
    assert_eq!(host.speaks_for(), &[2], "only seats at the table");
    let mut joiner = Lockstep::new(1, players, DEFAULT_DELAY);
    let place = PlayerAction::Place {
        x: 3,
        y: 4,
        dir: Direction::Up,
    };
    let mut played = Vec::new();
    for _ in 0..20 {
        let mine = host
            .commit_local_with(PlayerAction::None, &[(2, place)])
            .expect("committed");
        let theirs = joiner.commit_local(PlayerAction::None).expect("committed");
        for msg in host.recent_commits().to_vec() {
            joiner.receive(msg);
        }
        host.receive(theirs);
        assert_eq!(mine.player, 0);
        while let Some(frame) = joiner.advance() {
            played.push(frame);
        }
        while host.advance().is_some() {}
    }
    assert!(played.len() > 10, "the joiner was never waiting on the bot");
    assert!(
        played.iter().any(|frame| frame[2] == place),
        "and played the bot's moves"
    );
    assert_eq!(host.frame(), joiner.frame());
}
use crate::sim::board::Board;
use crate::sim::{CrabKind, Handedness, Spawner, TileKind};

/// A spectator handed the board at frame `n` and a session from `n`
/// plays the rest of the round exactly as the table does: the snapshot
/// carries everything the sim reads, and the inputs from `n` on are
/// the ones the players' resend tails still hold.
#[test]
fn a_late_watcher_from_a_snapshot_keeps_step_with_the_table() {
    use crate::sim::{BotLevel, bot_action, generate_arena};
    let players = vec![0u8, 1, 2];
    let mut table = generate_arena(21, 4, 20, 13);
    let mut host = Lockstep::new(0, players.clone(), DEFAULT_DELAY);
    // Every input on the wire, as a stand-in for the resend tails.
    let mut sent: Vec<InputMsg> = Vec::new();
    let mut late: Option<(Lockstep, Board)> = None;
    for step in 0..900u32 {
        // Each seat's input for the frame it commits, as the bots would
        // play it; seat 3 is an AI the lockstep never carries.
        for &seat in &players {
            let msg = InputMsg {
                player: seat,
                frame: DEFAULT_DELAY + step,
                action: bot_action(&table, seat, BotLevel::Hard),
            };
            if seat == 0 {
                host.commit_local(msg.action);
            } else {
                host.receive(msg);
            }
            sent.push(msg);
            if let Some((watcher, _)) = &mut late {
                watcher.receive(msg);
            }
        }
        while let Some(mut actions) = host.advance() {
            actions[3] = bot_action(&table, 3, BotLevel::Normal);
            table.tick(&actions);
        }
        if step == 300 {
            // The board through its text, as the wire carries it.
            let board = Board::parse_snapshot(&table.to_snapshot()).expect("parses");
            assert_eq!(board.state_hash(), table.state_hash());
            let mut watcher = Lockstep::observer_from(players.clone(), DEFAULT_DELAY, host.frame());
            assert_eq!(watcher.frame(), host.frame());
            // The frames already committed, which the tails repeat.
            for &msg in &sent {
                watcher.receive(msg);
            }
            late = Some((watcher, board));
        }
        if let Some((watcher, board)) = &mut late {
            while let Some(mut actions) = watcher.advance() {
                actions[3] = bot_action(board, 3, BotLevel::Normal);
                board.tick(&actions);
            }
        }
    }
    let (mut watcher, board) = late.expect("the watcher arrived");
    assert!(host.frame() > 600, "the round ran: {}", host.frame());
    assert_eq!(watcher.frame(), host.frame(), "caught up to the table");
    assert_eq!(
        board.state_hash(),
        table.state_hash(),
        "and on the same beach"
    );
    assert!(
        watcher.commit_local(PlayerAction::None).is_none(),
        "it only watches"
    );
}

/// A session from frame zero still prefills its first frames; one from
/// later prefills nothing, those frames having been played by others.
#[test]
fn a_session_from_a_later_frame_waits_for_real_inputs() {
    let fresh = Lockstep::observer_from(vec![0, 1], DEFAULT_DELAY, 0);
    assert!(fresh.awaiting().is_empty(), "frame zero is agreed idle");
    let mut late = Lockstep::observer_from(vec![0, 1], DEFAULT_DELAY, 500);
    assert_eq!(late.awaiting(), vec![0, 1]);
    assert!(late.advance().is_none());
    // Inputs from before its first frame are nothing to it.
    late.receive(InputMsg {
        player: 0,
        frame: 499,
        action: PlayerAction::None,
    });
    for player in [0, 1] {
        late.receive(InputMsg {
            player,
            frame: 500,
            action: PlayerAction::None,
        });
    }
    assert!(late.advance().is_some());
    assert_eq!(late.frame(), 501);
}

/// A stalled peer pushes back on local commits at the lead cap, and the
/// queue reopens as soon as frames advance.
#[test]
fn commit_lead_is_bounded_and_recovers() {
    let mut session = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
    let mut accepted = 0;
    for _ in 0..100 {
        if session.commit_local(PlayerAction::None).is_some() {
            accepted += 1;
        }
    }
    assert_eq!(
        accepted, MAX_COMMIT_LEAD as usize,
        "commits stop at the lead cap while the sim is stalled"
    );
    // The peer's inputs arrive; frames advance; commits reopen.
    for frame in DEFAULT_DELAY..DEFAULT_DELAY + 5 {
        session.receive(InputMsg {
            player: 1,
            frame,
            action: PlayerAction::None,
        });
    }
    let mut advanced = 0;
    while session.advance().is_some() {
        advanced += 1;
    }
    assert_eq!(advanced, DEFAULT_DELAY as usize + 5);
    assert!(session.commit_local(PlayerAction::None).is_some());
}

/// The pause is an agreement about a frame, not a local freeze: both
/// peers stop committing at it, so neither can complete it, so the sim
/// halts on the same frame on both, and resuming picks straight back up
/// with no gap and no repeated frame.
#[test]
fn a_pause_halts_both_peers_on_one_frame() {
    let players = vec![0u8, 1];
    let mut a = Lockstep::new(0, players.clone(), DEFAULT_DELAY);
    let mut b = Lockstep::new(1, players, DEFAULT_DELAY);
    let mut board_a = test_board(3);
    let mut board_b = test_board(3);
    let mut pause_at = None;

    let run = |a: &mut Lockstep, b: &mut Lockstep, ba: &mut Board, bb: &mut Board| {
        let msgs: Vec<_> = [
            a.commit_local(PlayerAction::None),
            b.commit_local(PlayerAction::None),
        ]
        .into_iter()
        .flatten()
        .collect();
        for msg in msgs {
            if msg.player == 0 {
                b.receive(msg);
            } else {
                a.receive(msg);
            }
        }
        while let Some(actions) = a.advance() {
            ba.tick(&actions);
        }
        while let Some(actions) = b.advance() {
            bb.tick(&actions);
        }
    };

    for step in 0..80 {
        if step == 20 {
            // Peer A hits Escape; the pause frame rides the wire.
            let frame = a.request_pause().expect("a player may pause");
            b.receive_pause(frame);
            pause_at = Some(frame);
        }
        run(&mut a, &mut b, &mut board_a, &mut board_b);
    }
    let pause_at = pause_at.expect("a pause was called");
    assert!(a.frozen() && b.frozen(), "both peers came to rest");
    assert_eq!(a.frame(), pause_at, "stopped on the agreed frame");
    assert_eq!(b.frame(), pause_at, "and so did the peer");
    assert_eq!(board_a.state_hash(), board_b.state_hash());

    // Peer B presses Continue; both play on from where they stopped.
    b.resume();
    a.resume();
    for _ in 0..40 {
        run(&mut a, &mut b, &mut board_a, &mut board_b);
    }
    assert!(a.frame() > pause_at + 20, "the match ran on");
    assert_eq!(a.frame(), b.frame());
    assert_eq!(
        board_a.state_hash(),
        board_b.state_hash(),
        "pausing desynced the peers"
    );
}

/// A watcher simulates the same frames from the players' inputs, commits
/// nothing, and (the part that matters) is never waited for: the two
/// players advance whether or not it keeps up.
#[test]
fn a_watcher_follows_without_being_waited_for() {
    let players = vec![0u8, 1];
    let mut a = Lockstep::new(0, players.clone(), DEFAULT_DELAY);
    let mut b = Lockstep::new(1, players.clone(), DEFAULT_DELAY);
    let mut watcher = Lockstep::observer(players, DEFAULT_DELAY);
    assert!(watcher.watching());
    assert_eq!(watcher.seat(), None);
    assert_eq!(a.seat(), Some(0));

    // Whatever it is handed, it commits nothing and sends nothing.
    assert!(
        watcher
            .commit_local(PlayerAction::Remove { x: 1, y: 1 })
            .is_none()
    );
    assert!(watcher.recent_commits().is_empty());

    let place = PlayerAction::Place {
        x: 2,
        y: 3,
        dir: Direction::Up,
    };
    for frame in 0..20u32 {
        let from_a = a.commit_local(if frame == 4 {
            place
        } else {
            PlayerAction::None
        });
        let from_b = b.commit_local(PlayerAction::None);
        for msg in [from_a, from_b].into_iter().flatten() {
            a.receive(msg);
            b.receive(msg);
            watcher.receive(msg);
        }
        // The players never stall on the watcher.
        assert!(a.advance().is_some(), "player a stalled at {frame}");
        assert!(b.advance().is_some(), "player b stalled at {frame}");
    }
    // The watcher replays exactly what they played, in order.
    let mut seen = Vec::new();
    while let Some(actions) = watcher.advance() {
        seen.push(actions);
    }
    // The first `delay` frames are agreed-empty by construction, so a
    // fresh session can always run those before any input arrives.
    assert_eq!(
        seen.len(),
        20 + DEFAULT_DELAY as usize,
        "the watcher saw every frame"
    );
    assert!(
        seen.iter().any(|frame| frame[0] == place),
        "and the placement among them"
    );
    // It cannot stop the match, either.
    assert_eq!(watcher.request_pause(), None, "nothing to broadcast");
    assert!(watcher.pause_frame().is_none(), "a watcher cannot pause");
}

/// Two players hitting Escape in the same breath must not each freeze on
/// their own frame: the earlier proposal wins on every peer.
#[test]
fn simultaneous_pauses_settle_on_the_earlier_frame() {
    let mut session = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
    let mine = session.request_pause().expect("a player may pause");
    session.receive_pause(mine + 4);
    assert_eq!(session.pause_frame(), Some(mine), "later proposal ignored");
    session.receive_pause(mine - 4);
    assert_eq!(session.pause_frame(), Some(mine - 4), "earlier one wins");
    // Repeats of the same pause (they are resent every tick) are inert.
    session.receive_pause(mine - 4);
    assert_eq!(session.pause_frame(), Some(mine - 4));
    session.resume();
    assert!(!session.paused() && !session.frozen());
}

/// Three peers exchanging inputs (as the host relay would deliver them)
/// must stay bit-identical: the protocol itself is seat-count agnostic.
#[test]
fn three_player_lockstep_stays_bit_identical() {
    let players = vec![0u8, 1, 2];
    let mut sessions: Vec<Lockstep> = (0..3u8)
        .map(|p| Lockstep::new(p, players.clone(), DEFAULT_DELAY))
        .collect();
    let mut boards: Vec<Board> = (0..3).map(|_| test_board(4)).collect();
    for step in 0u32..300 {
        let mut outgoing = Vec::new();
        for (i, session) in sessions.iter_mut().enumerate() {
            let action = if step % (20 + i as u32 * 7) == 3 {
                PlayerAction::Place {
                    x: (step % 12) as u8,
                    y: (i as u8 * 2 + 1) % 9,
                    dir: Direction::Down,
                }
            } else {
                PlayerAction::None
            };
            outgoing.extend(session.commit_local(action));
        }
        for msg in outgoing {
            for (i, session) in sessions.iter_mut().enumerate() {
                if i as u8 != msg.player {
                    session.receive(msg);
                }
            }
        }
        for (session, board) in sessions.iter_mut().zip(&mut boards) {
            while let Some(actions) = session.advance() {
                board.tick(&actions);
            }
        }
    }
    assert!(sessions[0].frame() > 250, "made progress");
    assert_eq!(boards[0].state_hash(), boards[1].state_hash());
    assert_eq!(boards[1].state_hash(), boards[2].state_hash());
}

fn roundtrip(action: PlayerAction) {
    let decoded = decode_action(encode_action(action));
    assert_eq!(format!("{action:?}"), format!("{decoded:?}"));
}

/// The spectators' call rides in a seat's input, and that is the whole
/// reason it is an action at all: a peer cannot simulate a frame
/// without every seat's input for it, so the call arrives with the
/// frame it belongs to or the frame does not run. A side channel would
/// have been echoed and hoped for, and a peer that missed every echo
/// would have played a different round without knowing.
#[test]
fn a_called_event_travels_in_the_frame_it_belongs_to() {
    for event in TideEvent::ALL {
        roundtrip(PlayerAction::CallEvent(event));
    }

    // Two peers, one frame, the call in seat 0's slot: both run it.
    let mut a = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
    let mut b = Lockstep::new(1, vec![0, 1], DEFAULT_DELAY);
    let called = PlayerAction::CallEvent(TideEvent::FreshSand);
    for frame in 0..=DEFAULT_DELAY {
        let from_a = a.commit_local(if frame == DEFAULT_DELAY {
            called
        } else {
            PlayerAction::None
        });
        let from_b = b.commit_local(PlayerAction::None);
        if let Some(msg) = from_a {
            b.receive(msg);
        }
        if let Some(msg) = from_b {
            a.receive(msg);
        }
    }
    let mut seen = Vec::new();
    while let (Some(one), Some(two)) = (a.advance(), b.advance()) {
        assert_eq!(
            format!("{one:?}"),
            format!("{two:?}"),
            "both peers run the same frame"
        );
        seen.push(one[0]);
    }
    assert!(
        seen.iter()
            .any(|a| format!("{a:?}") == format!("{called:?}")),
        "the call came out of the frame it went into: {seen:?}"
    );
}

#[test]
fn packed_input_round_trips() {
    roundtrip(PlayerAction::None);
    for dir in [
        Direction::Up,
        Direction::Right,
        Direction::Down,
        Direction::Left,
    ] {
        roundtrip(PlayerAction::Place { x: 11, y: 8, dir });
    }
    roundtrip(PlayerAction::Remove { x: 0, y: 15 });
    // Past 16 wide, where a nibble per axis would have wrapped: the XL
    // beach is 20.
    for x in 16..20u8 {
        roundtrip(PlayerAction::Place {
            x,
            y: 11,
            dir: Direction::Right,
        });
        roundtrip(PlayerAction::Remove { x, y: 12 });
    }
    let msg = InputMsg {
        player: 3,
        frame: 123_456,
        action: PlayerAction::Place {
            x: 7,
            y: 2,
            dir: Direction::Left,
        },
    };
    assert_eq!(InputMsg::decode(msg.encode()), msg);
}

fn test_board(seed: u64) -> Board {
    let mut board = Board::new(12, 9, seed);
    board.set_tile(11, 4, TileKind::Castle(0));
    board.set_tile(0, 4, TileKind::Castle(1));
    board.set_tile(
        5,
        0,
        TileKind::Spawner(Spawner {
            dir: Direction::Down,
            period: 25,
        }),
    );
    board.spawn_crab(2, 2, Direction::Right, Handedness::Left, CrabKind::Common);
    board.spawn_gull(9, 7, Direction::Left);
    board
}

/// Two peers, out-of-order delivery with lag spikes: both must simulate
/// identical frames and end bit-identical.
#[test]
fn lockstep_peers_stay_bit_identical() {
    let players = vec![0u8, 1u8];
    let mut a = Lockstep::new(0, players.clone(), DEFAULT_DELAY);
    let mut b = Lockstep::new(1, players, DEFAULT_DELAY);
    let mut board_a = test_board(9);
    let mut board_b = test_board(9);
    let (mut queue_ab, mut queue_ba): (Vec<InputMsg>, Vec<InputMsg>) = (vec![], vec![]);

    for step in 0u32..600 {
        // Local "input sampling": scripted, different per peer.
        let act_a = if step % 50 == 7 {
            PlayerAction::Place {
                x: (step % 12) as u8,
                y: 3,
                dir: Direction::Down,
            }
        } else {
            PlayerAction::None
        };
        let act_b = if step % 70 == 11 {
            PlayerAction::Place {
                x: 4,
                y: (step % 9) as u8,
                dir: Direction::Left,
            }
        } else {
            PlayerAction::None
        };
        queue_ab.extend(a.commit_local(act_a));
        queue_ba.extend(b.commit_local(act_b));

        // Artificial network: hold messages back during "lag spikes",
        // deliver newest-first (reordering) otherwise.
        if !(step % 90 < 8) {
            while let Some(msg) = queue_ab.pop() {
                b.receive(msg);
            }
            while let Some(msg) = queue_ba.pop() {
                a.receive(msg);
            }
        }

        // Each peer simulates as many frames as it can.
        while let Some(actions) = a.advance() {
            board_a.tick(&actions);
        }
        while let Some(actions) = b.advance() {
            board_b.tick(&actions);
        }
    }
    // Flush and drain: both converge on the same final frame.
    while let Some(msg) = queue_ab.pop() {
        b.receive(msg);
    }
    while let Some(msg) = queue_ba.pop() {
        a.receive(msg);
    }
    while let Some(actions) = a.advance() {
        board_a.tick(&actions);
    }
    while let Some(actions) = b.advance() {
        board_b.tick(&actions);
    }
    let min_frame = a.frame().min(b.frame());
    assert!(min_frame > 500, "sessions made progress ({min_frame})");
    // Winding the faster board back is impossible, so instead both
    // drained all available frames; with every message delivered, the
    // frames are equal.
    assert_eq!(a.frame(), b.frame());
    assert_eq!(
        board_a.state_hash(),
        board_b.state_hash(),
        "lockstep peers diverged"
    );
}

mod abandon_tests {
    use super::*;

    /// A player that stops sending holds up everybody, because a frame
    /// simulates only when every seat's action is known. Giving up on them
    /// has to unstick the frame already waiting, not merely the ones after
    /// it, or the round stays frozen on the frame that proved they had
    /// gone.
    #[test]
    fn abandoning_a_player_unsticks_the_frame_they_were_holding() {
        let mut session = Lockstep::new(0, vec![0, 1], 0);
        session.commit_local(PlayerAction::None);
        assert_eq!(session.awaiting(), vec![1], "held up by the one who left");
        assert!(session.advance().is_none(), "and going nowhere");

        session.abandon(1, 0);
        assert!(session.awaiting().is_empty(), "nobody left to wait for");
        assert!(session.advance().is_some(), "the round moves again");
    }

    /// And it keeps moving: the seat is filled as an absent one from then
    /// on, so the next frame does not stall all over again.
    #[test]
    fn an_abandoned_seat_stays_abandoned() {
        let mut session = Lockstep::new(0, vec![0, 1], 0);
        session.abandon(1, 0);
        for frame in 0..30 {
            session.commit_local(PlayerAction::None);
            assert!(
                session.advance().is_some(),
                "stalled again at frame {frame}"
            );
        }
    }

    /// The seat is emptied, not filled: what the sim gets for it is a plain
    /// no-op, so the shell can drop an AI in on top without the two of them
    /// fighting over the same slot.
    #[test]
    fn an_abandoned_seat_is_handed_over_empty() {
        let mut session = Lockstep::new(0, vec![0, 1], 0);
        session.abandon(1, 0);
        session.commit_local(PlayerAction::None);
        let actions = session.advance().expect("moves");
        assert_eq!(actions[1], PlayerAction::None);
    }

    /// Everyone else is untouched: giving up on one player must not drop
    /// the round for the rest.
    #[test]
    fn the_others_are_still_waited_for() {
        let mut session = Lockstep::new(0, vec![0, 1, 2], 0);
        session.commit_local(PlayerAction::None);
        session.abandon(1, 0);
        assert_eq!(session.awaiting(), vec![2], "still owed seat two");
        assert!(session.advance().is_none());
        session.receive(InputMsg {
            player: 2,
            frame: 0,
            action: PlayerAction::None,
        });
        assert!(session.advance().is_some());
    }
}

mod loss_tests {
    use super::*;

    /// Two peers over a lossy link, driven by a delivery schedule: each
    /// step both commit, then whatever `deliver` allows crosses, then both
    /// simulate what they can. Returns the frames they reached.
    fn run(steps: usize, mut deliver: impl FnMut(usize, PlayerId) -> bool) -> (Lockstep, Lockstep) {
        let players = vec![0u8, 1];
        let mut a = Lockstep::new(0, players.clone(), DEFAULT_DELAY);
        let mut b = Lockstep::new(1, players, DEFAULT_DELAY);
        for step in 0..steps {
            a.commit_local(PlayerAction::None);
            b.commit_local(PlayerAction::None);
            // Every tick resends the whole tail, as the shell does.
            let from_a: Vec<InputMsg> = a.recent_commits().to_vec();
            let from_b: Vec<InputMsg> = b.recent_commits().to_vec();
            if deliver(step, 0) {
                for msg in from_a {
                    b.receive(msg);
                }
            }
            if deliver(step, 1) {
                for msg in from_b {
                    a.receive(msg);
                }
            }
            while a.advance().is_some() {}
            while b.advance().is_some() {}
        }
        (a, b)
    }

    /// One direction of the link goes dark for longer than a lead, while
    /// the other keeps flowing. The commit the quiet peer is stuck on is
    /// then two leads old on the loud one, which a fixed forty-deep tail
    /// had already let go, freezing the session for good with both peers
    /// "still talking".
    #[test]
    fn a_one_way_loss_burst_does_not_deadlock_the_session() {
        let blackout = 20..90;
        let (a, b) = run(300, |step, from| from != 0 || !blackout.contains(&step));
        assert_eq!(a.frame(), b.frame(), "back in step");
        assert!(a.frame() > 250, "and the round ran on ({})", a.frame());
    }

    /// The tail is not unbounded either: it holds what a peer could be
    /// missing, and no more.
    #[test]
    fn the_resend_tail_stays_within_two_leads() {
        let (a, _) = run(200, |_, _| true);
        let span = resend_span(DEFAULT_DELAY) as usize;
        assert!(
            a.recent_commits().len() <= 2 * span,
            "{}",
            a.recent_commits().len()
        );
    }
}

#[cfg(test)]
mod pause_echo_tests {
    use super::*;

    /// A peer that hears the resume, then a `Pause` echo still in flight
    /// from before it, must not pause again: peers repeat their pause frame
    /// every tick, so those echoes always cross the resume, and taken at
    /// face value the two flap between paused and playing for good.
    #[test]
    fn a_stale_pause_echo_after_the_resume_is_ignored() {
        let mut a = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        let at = a.request_pause().expect("a player may pause");
        // Frozen on it, as a peer that heard the pause in time would be.
        while a.frame() < at {
            a.commit_local(PlayerAction::None);
            a.receive(InputMsg {
                player: 1,
                frame: a.frame(),
                action: PlayerAction::None,
            });
            assert!(a.advance().is_some());
        }
        assert!(a.frozen());
        // The peer resumes; its last echo of the pause is still on the wire.
        a.receive_resume(at);
        assert!(!a.paused());
        a.receive_pause(at);
        assert!(!a.paused(), "an echo of a lifted pause is not a pause");
        // Nor is one from a pause that never reached us until after the
        // resume was heard: our own frame is already past it.
        a.receive_pause(at.saturating_sub(1));
        assert!(!a.paused());
        // A fresh pause is a fresh pause.
        let again = a.request_pause().expect("a player may pause");
        assert!(again > at);
        assert!(a.paused());
    }

    /// The same, when the resume comes first and the peer never heard the
    /// pause at all: the frame the resume names is enough to know the
    /// echo is stale.
    #[test]
    fn a_resume_names_the_pause_it_lifts() {
        let mut a = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        a.receive_resume(40);
        assert_eq!(a.lifted_pause(), Some(40));
        a.receive_pause(40);
        assert!(!a.paused());
        a.receive_pause(41);
        assert!(a.paused(), "a later frame is a new pause");
        // Resuming locally lifts through the highest frame known.
        assert_eq!(a.resume(), 41);
    }

    /// The first resume of a match keeps echoing for a while, and an echo
    /// still on the wire when the next pause is called must not lift it:
    /// that echo names the old pause, and the new one is past it. Every
    /// pause after the first used to be cancelled within a tick this way.
    #[test]
    fn an_old_resume_does_not_lift_a_new_pause() {
        let mut a = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        let first = a.request_pause().expect("a player may pause");
        let echo = a.resume();
        assert_eq!(echo, first);
        let second = a.request_pause().expect("a player may pause");
        assert!(second > first, "{second} > {first}");
        assert_eq!(a.receive_resume(echo), None, "an echo is not news");
        assert_eq!(a.pause_frame(), Some(second), "the new pause holds");
        // The peer's own resume of the new pause still lifts it.
        assert_eq!(a.receive_resume(second), Some(second));
        assert!(!a.paused());
    }

    /// A pause proposal that would be read as an echo of a lifted one is
    /// pushed past it, so a second Escape shortly after a resume still
    /// pauses, everywhere.
    #[test]
    fn a_new_pause_is_always_past_the_lifted_one() {
        let mut a = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        a.receive_resume(50);
        let at = a.request_pause().expect("a player may pause");
        assert!(at > 50, "{at}");
        assert_eq!(a.pause_frame(), Some(at));
    }

    /// A resume names a frame off the wire, and the next local pause is
    /// proposed one past it. A peer naming the last frame there is made
    /// that addition overflow: a panic in debug, and in release a proposal
    /// that wrapped to zero and was broadcast anyway. Bounded to the
    /// horizon like every input frame.
    #[test]
    fn a_resume_from_the_end_of_time_cannot_break_the_next_pause() {
        let mut a = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        // With a pause in flight, which is when a resume past the horizon
        // is still taken (see `a_resume_past_the_horizon_lifts_nothing`).
        a.request_pause().expect("a player may pause");
        let lifted = a
            .receive_resume(u32::MAX)
            .expect("nothing was lifted before");
        assert!(lifted <= 2 * resend_span(DEFAULT_DELAY), "{lifted}");
        let at = a.request_pause().expect("a player may pause");
        assert!(at > lifted, "{at}");
        assert_eq!(a.pause_frame(), Some(at), "and the pause is really held");
        // A pause proposal from past the horizon is dropped the same way.
        let mut b = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        b.receive_pause(u32::MAX);
        assert_eq!(b.pause_frame(), None);
    }

    /// A resume still on the wire from the last round, naming a frame near
    /// its end, reaches a session begun again at frame zero. With no pause
    /// in flight there is nothing for it to lift, and taken, it raised
    /// `lifted` to the horizon: a peer's early pause in the new round then
    /// read as an echo of a lifted one, and was answered with a resume.
    #[test]
    fn a_resume_past_the_horizon_lifts_nothing() {
        let mut a = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        assert!(!a.within_horizon(1790));
        assert_eq!(a.receive_resume(1790), None, "last round's resume");
        assert_eq!(a.lifted_pause(), None, "and nothing was lifted");
        a.receive_pause(PAUSE_LEAD);
        assert_eq!(a.pause_frame(), Some(PAUSE_LEAD), "a peer's early pause");
        assert!(!a.is_lifted(PAUSE_LEAD));
        // A pause in flight is still lifted by one past the horizon.
        assert!(a.receive_resume(1790).is_some());
        assert!(!a.paused());
    }
}

#[cfg(test)]
mod abandon_frame_tests {
    use super::*;

    /// Two peers give up on the same seat from the same frame and end up
    /// with the same inputs for it, whatever each of them happened to hold:
    /// the one that held a later input from the departed player plays it
    /// empty like everyone else, rather than as sent.
    #[test]
    fn peers_holding_different_inputs_agree_after_the_abandonment() {
        let mut host = Lockstep::new(0, vec![0, 1, 2], 0);
        let mut peer = Lockstep::new(2, vec![0, 1, 2], 0);
        let place = PlayerAction::Place {
            x: 1,
            y: 1,
            dir: Direction::Up,
        };
        // Seat 1's frame 0 never reached the host (nor, therefore, the
        // peer), but its frame 2 reached the host, and the relay of it
        // reached the peer; the relay of frame 1 was lost on the way.
        for frame in [1, 2] {
            host.receive(InputMsg {
                player: 1,
                frame,
                action: place,
            });
        }
        peer.receive(InputMsg {
            player: 1,
            frame: 2,
            action: place,
        });
        // The host is held up on frame 0 and gives up there.
        let at = host.frame();
        host.abandon(1, at);
        peer.abandon(1, at);
        for frame in 0..3 {
            for session in [&mut host, &mut peer] {
                session.commit_local(PlayerAction::None);
                let other = if session.seat() == Some(0) { 2 } else { 0 };
                session.receive(InputMsg {
                    player: other,
                    frame,
                    action: PlayerAction::None,
                });
            }
            let from_host = host.advance().expect("host moves");
            let from_peer = peer.advance().expect("peer moves");
            assert_eq!(from_host[1], PlayerAction::None, "frame {frame}");
            assert_eq!(from_host, from_peer, "frame {frame}");
        }
    }

    /// A peer behind the host when the host gives up on a seat waits for
    /// that seat's real inputs on the frames the host already played with
    /// them, and takes them from the host's repeat: filled empty instead,
    /// the peer played a frame the host never did.
    #[test]
    fn a_peer_behind_plays_the_departed_seats_last_inputs_as_the_host_did() {
        let mut host = Lockstep::new(0, vec![0, 1, 2], 0);
        let mut peer = Lockstep::new(2, vec![0, 1, 2], 0);
        let place = PlayerAction::Place {
            x: 1,
            y: 1,
            dir: Direction::Up,
        };
        // Frame 0 is played by both. Seat 1's input for frame 1 reaches
        // the host, which plays frame 1 with it; its relay to the peer is
        // lost. Then seat 1 goes quiet.
        let none = |player, frame| InputMsg {
            player,
            frame,
            action: PlayerAction::None,
        };
        for session in [&mut host, &mut peer] {
            session.commit_local(PlayerAction::None);
            session.commit_local(PlayerAction::None);
        }
        for frame in 0..2 {
            host.receive(none(2, frame));
            peer.receive(none(0, frame));
        }
        host.receive(none(1, 0));
        peer.receive(none(1, 0));
        host.receive(InputMsg {
            player: 1,
            frame: 1,
            action: place,
        });
        let host_frames = [host.advance().expect("0"), host.advance().expect("1")];
        assert_eq!(host_frames[1][1], place);
        assert!(peer.advance().is_some(), "the peer plays frame 0");
        // The host is held up on frame 2 and gives up there.
        let at = host.frame();
        host.abandon(1, at);
        peer.abandon(1, at);
        assert!(peer.advance().is_none(), "frame 1 still waits on seat 1");
        assert!(
            peer.awaiting().contains(&1),
            "and says so, for the line that names who a round waits on"
        );
        for owed in host.owed_by(1, at) {
            peer.receive(owed);
        }
        let played = peer.advance().expect("the repeat fills it");
        assert_eq!(played, host_frames[1], "as the host played it");
    }

    /// Inputs from the departed player that arrive after the abandonment
    /// are ignored, however far ahead they name.
    #[test]
    fn a_departed_seat_is_no_longer_heard() {
        let mut session = Lockstep::new(0, vec![0, 1], 0);
        session.abandon(1, 0);
        session.receive(InputMsg {
            player: 1,
            frame: 5,
            action: PlayerAction::Remove { x: 0, y: 0 },
        });
        for _ in 0..6 {
            session.commit_local(PlayerAction::None);
            let actions = session.advance().expect("moves");
            assert_eq!(actions[1], PlayerAction::None);
        }
    }

    /// A frame further ahead than any peer in step could have committed
    /// makes no slot: a stranger naming frames to the horizon would
    /// otherwise grow the table without bound.
    #[test]
    fn inputs_beyond_the_horizon_are_dropped() {
        let mut session = Lockstep::new(0, vec![0, 1], DEFAULT_DELAY);
        let before = session.pending.len();
        session.receive(InputMsg {
            player: 1,
            frame: u32::MAX,
            action: PlayerAction::None,
        });
        session.receive(InputMsg {
            player: 1,
            frame: 1000,
            action: PlayerAction::None,
        });
        assert_eq!(session.pending.len(), before);
        session.receive(InputMsg {
            player: 1,
            frame: 2 * resend_span(DEFAULT_DELAY),
            action: PlayerAction::None,
        });
        assert_eq!(
            session.pending.len(),
            before + 1,
            "the horizon itself is in"
        );
    }
}
