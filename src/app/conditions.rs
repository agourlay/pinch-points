//! The named run conditions the schedule is written in.
//!
//! Every one is a `fn(..) -> bool` over the screen and phase states (and
//! the odd resource), and every one is read by `schedule.rs` alone. Split
//! between there and `session.rs`, neither read as one list.

use super::{Phase, Playback, Screen, VersusPhase, editor, lobby, match_setup};
use bevy::prelude::*;

/// Screens that render a live board (settings and the lobby are clean).
pub(super) fn board_screens(screen: Res<State<Screen>>) -> bool {
    match screen.get() {
        Screen::Menu => false,
        Screen::Puzzle | Screen::Versus | Screen::Editor => true,
        Screen::Lobby
        | Screen::Settings
        | Screen::Controls
        | Screen::MatchSetup
        | Screen::Achievements
        | Screen::StageSelect
        | Screen::Replays
        | Screen::Interlude
        | Screen::Language
        | Screen::NewVersion => false,
    }
}

/// The screens that stand on the postcard beach: the menu at full daylight,
/// every browsing screen behind the scrim. Board screens are out: a round
/// is played over its own sand, not over a second beach.
pub(super) fn postcard_screens(screen: Res<State<Screen>>) -> bool {
    super::postcard_screen(*screen.get())
}

/// Screens where a round is being played, a puzzle or a versus match.
pub(super) fn play_screens(screen: Res<State<Screen>>) -> bool {
    matches!(screen.get(), Screen::Puzzle | Screen::Versus)
}

/// Whether the keyboard still belongs to the beach, rather than to the
/// pause card in front of it.
///
/// The card is an overlay and not a `Screen`, and it freezes the sim
/// through a resource of its own, so neither the screen nor the phase
/// changes when it opens: every input system's own condition stays true
/// and it keeps reading keys behind the card. That let W and S move the
/// board cursor while they walked the card's rows, let an arrow queue a
/// placement that fired the moment the round resumed, and let N load a
/// different stage under a card still saying PAUSED.
///
/// `playback_pause_input` found this first and refused the card by hand;
/// this is that rule, once, for everything that reads a key while a round
/// is up.
pub(super) fn keys_are_free(menu: Res<crate::app::pause::PauseMenu>) -> bool {
    !menu.is_open()
}

/// Whether a spectator's chat line reads the keyboard: as [`keys_are_free`]
/// has it, and also under a card while a line is open.
///
/// A peer's pause raises the card on a spectator mid-line, and the hands
/// typing it do not stop: the letters went to the card instead, each S
/// walking it down and the Enter meant to send the line taking "To menu"
/// or "Quit". The line keeps the keys until it is said or dropped, and the
/// card stands down meanwhile (`pause_input`).
pub(super) fn chat_keys_free(
    menu: Res<crate::app::pause::PauseMenu>,
    chat: Res<crate::app::spectators::SpectatorChat>,
) -> bool {
    !menu.is_open() || chat.open()
}

/// Whether a player is typing words rather than pressing keys: a seat's
/// name, a beach's name, an address, a line of chat.
///
/// The global letter keys are mnemonics, and a mnemonic has to stand down
/// when the letter is one the player meant to write: M is in "Emma", and
/// as the master mute it silences the whole game halfway through a name.
pub(super) fn text_entry_open(
    lobby: Res<lobby::LobbyState>,
    setup: Res<match_setup::MatchMenu>,
    editor: Res<editor::EditorState>,
    spectators: Res<crate::app::spectators::SpectatorChat>,
) -> bool {
    lobby.typing.is_some() || setup.naming.is_some() || editor.is_naming() || spectators.open()
}

/// Whether the round on screen is being played rather than watched: a
/// replay rewatched is not a round won again, so nothing that counts
/// deeds (stats, trophies) runs while a recording is being fed.
pub(super) fn not_watching(playback: Res<Playback>) -> bool {
    playback.0.is_none()
}

/// The sim ticks while a puzzle runs or a versus round is live.
pub(super) fn sim_should_run(
    screen: Res<State<Screen>>,
    phase: Res<State<Phase>>,
    vphase: Res<State<VersusPhase>>,
    editor: Res<editor::EditorState>,
) -> bool {
    match screen.get() {
        // The menu is a still postcard; its little animations are not sim.
        Screen::Menu => false,
        Screen::Lobby
        | Screen::Settings
        | Screen::Controls
        | Screen::MatchSetup
        | Screen::Achievements
        | Screen::StageSelect
        | Screen::Replays
        | Screen::Interlude
        | Screen::Language
        | Screen::NewVersion => false,
        Screen::Puzzle => *phase.get() == Phase::Running,
        Screen::Versus => *vphase.get() == VersusPhase::Running,
        // A playtest's board ticks while it runs: not over a stage's
        // setup, nor once it is won, lost or its round is over.
        Screen::Editor => editor.test_running(),
    }
}

/// A running puzzle's board has not been decided yet: the tick that wins
/// or loses the stage is the last one played.
///
/// Read before every fixed tick, not once a frame. `check_outcome` judges
/// the board in `Update`, after however many ticks the frame covered, and
/// an untimed stage does not freeze at its deadline the way a round's
/// clock freezes a timed one: a frame covering ticks 1800 and 1801 judged
/// the board a tick past the limit, which turned a Survive won on 1800
/// into one lost to a gull on 1801. The solver's `play_out` stops at the
/// first tick that decides the stage, and so does the game.
///
/// A stage played in the editor's playtest stops on its deciding tick the
/// same way, judged by the level under test.
pub(super) fn puzzle_undecided(
    screen: Res<State<Screen>>,
    campaign: Option<Res<super::Campaign>>,
    sim: Option<Res<super::Sim>>,
    editor: Option<Res<editor::EditorState>>,
) -> bool {
    match (screen.get(), campaign, sim, editor) {
        (Screen::Puzzle, Some(campaign), Some(sim), _) => {
            campaign.current().outcome(&sim.0) == crate::sim::PuzzleOutcome::Running
        }
        (Screen::Editor, _, Some(sim), Some(editor)) => {
            editor.test().is_none_or(|test| test.undecided(&sim.0))
        }
        _ => true,
    }
}

/// A versus round in play, as against one already over.
pub(super) fn versus_running(screen: Res<State<Screen>>, phase: Res<State<VersusPhase>>) -> bool {
    *screen.get() == Screen::Versus && *phase.get() == VersusPhase::Running
}

/// Whether the board cursors answer the keys and the pads: off the menu,
/// not while the editor's keys are its own, not under the pause card, and
/// not under the host's map page between rounds, whose keys (and the
/// pad's d-pad, bridged to them) walk its cards; the cursor behind it
/// walked with them.
pub(super) fn board_cursor_free(
    screen: Res<State<Screen>>,
    editor: Res<editor::EditorState>,
    pause: Res<crate::app::pause::PauseMenu>,
    menu: Res<match_setup::MatchMenu>,
) -> bool {
    *screen.get() != Screen::Menu
        && !editor.has_keys()
        && !pause.is_open()
        && !crate::app::next_map::page_up(&menu)
}

/// Whether a versus round is dealt but not yet played: the interlude
/// before it, or its line-up and count.
pub(super) fn versus_before_the_round(
    screen: Res<State<Screen>>,
    phase: Res<State<VersusPhase>>,
) -> bool {
    match screen.get() {
        Screen::Interlude => true,
        Screen::Versus => *phase.get() == VersusPhase::Countdown,
        Screen::Menu
        | Screen::Puzzle
        | Screen::Editor
        | Screen::Lobby
        | Screen::Settings
        | Screen::Controls
        | Screen::MatchSetup
        | Screen::Achievements
        | Screen::StageSelect
        | Screen::Replays
        | Screen::Language
        | Screen::NewVersion => false,
    }
}

pub(super) fn versus_over(screen: Res<State<Screen>>, phase: Res<State<VersusPhase>>) -> bool {
    *screen.get() == Screen::Versus && *phase.get() == VersusPhase::Over
}

pub(super) fn puzzle_setup(screen: Res<State<Screen>>, phase: Res<State<Phase>>) -> bool {
    in_puzzle_setup(screen.get(), phase.get())
}

/// Whether a puzzle is being laid out, its run not begun: what
/// [`puzzle_setup`] runs under, and what the pause card asks before
/// taking a pad's Start, which means "begin the run" there.
pub(in crate::app) fn in_puzzle_setup(screen: &Screen, phase: &Phase) -> bool {
    *screen == Screen::Puzzle && *phase == Phase::Setup
}

pub(super) fn puzzle_running(screen: Res<State<Screen>>, phase: Res<State<Phase>>) -> bool {
    *screen.get() == Screen::Puzzle && *phase.get() == Phase::Running
}

pub(super) fn puzzle_done(screen: Res<State<Screen>>, phase: Res<State<Phase>>) -> bool {
    puzzle_settled(screen.get(), phase.get())
}

/// Whether a puzzle is won or lost and its card up: what [`puzzle_done`]
/// runs under, what the pause card stays shut on, and a puzzle's half of
/// a round having ended ([`RoundState::ended`]).
///
/// [`RoundState::ended`]: crate::app::round_state::RoundState::ended
pub(in crate::app) fn puzzle_settled(screen: &Screen, phase: &Phase) -> bool {
    *screen == Screen::Puzzle && matches!(phase, Phase::Won | Phase::Lost)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Screen, VersusPhase};
    use bevy::ecs::system::RunSystemOnce;
    use bevy::prelude::{State, World};

    /// A world holding just the states and resources the conditions read.
    ///
    /// Run through `run_system_once` rather than by hanging a marker system
    /// off each condition: a run condition *is* a system returning `bool`,
    /// so this asks it the same question the schedule does.
    fn world(screen: Screen, phase: Phase, vphase: VersusPhase) -> World {
        let mut world = World::new();
        world.insert_resource(State::new(screen));
        world.insert_resource(State::new(phase));
        world.insert_resource(State::new(vphase));
        world.insert_resource(editor::EditorState::default());
        world.insert_resource(lobby::LobbyState::default());
        world.insert_resource(match_setup::MatchMenu::default());
        world.insert_resource(Playback::default());
        world.insert_resource(crate::app::spectators::SpectatorChat::default());
        world
    }

    /// An untimed stage stops stepping on the tick that decides it, the
    /// one the solver's `play_out` stops on, rather than on whatever tick
    /// the frame that covers it ends at.
    #[test]
    fn a_decided_puzzle_takes_no_more_ticks() {
        use crate::sim::{Level, PUZZLE_TICK_LIMIT};
        let level = Level::parse(
            "name: T\nposts: 0\ngoal: survive\nmap:\n+-+-+\n|. .|\n+ + +\n|. .|\n+-+-+\n",
        )
        .expect("level");
        let mut board = level.board();
        for _ in 1..PUZZLE_TICK_LIMIT {
            board.tick_idle();
        }
        let mut w = world(Screen::Puzzle, Phase::Running, VersusPhase::Running);
        w.insert_resource(crate::app::Campaign {
            builtins: 1,
            ..crate::app::Campaign::tide_pool_of(vec![level.clone()])
        });
        w.insert_resource(crate::app::Sim(board.clone()));
        assert!(w.run_system_once(puzzle_undecided).expect("ran"));
        board.tick_idle();
        w.insert_resource(crate::app::Sim(board));
        assert!(
            !w.run_system_once(puzzle_undecided).expect("ran"),
            "decided"
        );
        // Anywhere else the sim's own conditions decide.
        let mut w = world(Screen::Versus, Phase::Running, VersusPhase::Running);
        assert!(w.run_system_once(puzzle_undecided).expect("ran"));
    }

    /// Every screen the game has, so the sweeps below are exhaustive by
    /// construction rather than by whoever wrote them remembering.
    const EVERY_SCREEN: [Screen; 14] = [
        Screen::Menu,
        Screen::Puzzle,
        Screen::Versus,
        Screen::Editor,
        Screen::Lobby,
        Screen::Settings,
        Screen::Controls,
        Screen::MatchSetup,
        Screen::Achievements,
        Screen::StageSelect,
        Screen::Replays,
        Screen::Interlude,
        Screen::Language,
        Screen::NewVersion,
    ];

    fn ask(
        screen: Screen,
        phase: Phase,
        vphase: VersusPhase,
        what: fn(&mut World) -> bool,
    ) -> bool {
        what(&mut world(screen, phase, vphase))
    }

    /// The three screens that draw a board are exactly the three that have
    /// one. Everything hung off this condition reads live board state, so a
    /// screen wrongly included has those systems probing whatever board was
    /// last loaded, which is a panic in `tile_at` rather than a stale
    /// picture.
    #[test]
    fn a_board_is_drawn_on_the_three_screens_that_have_one() {
        for screen in EVERY_SCREEN {
            let wanted = matches!(screen, Screen::Puzzle | Screen::Versus | Screen::Editor);
            let got = ask(screen, Phase::Setup, VersusPhase::Running, |w| {
                w.run_system_once(board_screens).expect("ran")
            });
            assert_eq!(got, wanted, "{screen:?}");
        }
    }

    /// A round is being *played* on two screens only. The editor draws a
    /// board and can even run one, but nothing that belongs to a match (the
    /// announcements, the pause card, the trophy counters) belongs to a
    /// level being built.
    #[test]
    fn only_two_screens_are_a_round_in_progress() {
        for screen in EVERY_SCREEN {
            let wanted = matches!(screen, Screen::Puzzle | Screen::Versus);
            let got = ask(screen, Phase::Running, VersusPhase::Running, |w| {
                w.run_system_once(play_screens).expect("ran")
            });
            assert_eq!(got, wanted, "{screen:?}");
        }
    }

    /// The postcard stands behind every browsing screen and behind none of
    /// the board ones: a round is played over its own sand, not over a
    /// second beach.
    #[test]
    fn the_postcard_stands_behind_the_screens_that_are_not_a_board() {
        for screen in EVERY_SCREEN {
            let board = ask(screen, Phase::Setup, VersusPhase::Running, |w| {
                w.run_system_once(board_screens).expect("ran")
            });
            let postcard = ask(screen, Phase::Setup, VersusPhase::Running, |w| {
                w.run_system_once(postcard_screens).expect("ran")
            });
            assert!(
                !(board && postcard),
                "{screen:?} draws a board *and* the postcard behind it"
            );
        }
    }

    /// The one that decides whether the game is running at all.
    ///
    /// Every arm is a different answer to "is time passing", and a wrong one
    /// is either a frozen beach or a puzzle whose crabs walk while the
    /// player is still placing. The editor's board ticks only while it is
    /// being playtested.
    #[test]
    fn time_passes_on_exactly_the_screens_and_phases_that_are_playing() {
        let run = |screen, phase, vphase| {
            ask(screen, phase, vphase, |w| {
                w.run_system_once(sim_should_run).expect("ran")
            })
        };
        // A puzzle ticks while running, and in none of its other phases:
        // not while the player is placing, and not once it is decided.
        assert!(run(Screen::Puzzle, Phase::Running, VersusPhase::Running));
        for phase in [Phase::Setup, Phase::Won, Phase::Lost] {
            assert!(
                !run(Screen::Puzzle, phase, VersusPhase::Running),
                "a puzzle ticked in {phase:?}"
            );
        }
        // A versus round ticks until the tide, and stops at the results.
        assert!(run(Screen::Versus, Phase::Setup, VersusPhase::Running));
        assert!(!run(Screen::Versus, Phase::Setup, VersusPhase::Over));
        // The menu is a still postcard; its little animations are not sim.
        for screen in EVERY_SCREEN {
            if matches!(screen, Screen::Puzzle | Screen::Versus | Screen::Editor) {
                continue;
            }
            assert!(
                !run(screen, Phase::Running, VersusPhase::Running),
                "{screen:?} is not a round and must not tick"
            );
        }
    }

    /// The editor's board holds still until it is being playtested. It is
    /// the only screen whose answer depends on something other than the
    /// screen and the phase.
    #[test]
    fn the_editors_board_ticks_only_while_it_is_being_tested() {
        let mut world = world(Screen::Editor, Phase::Setup, VersusPhase::Running);
        assert!(
            !world.run_system_once(sim_should_run).expect("ran"),
            "a board being painted must not walk its crabs"
        );
        // Playtesting carries the board it is testing, which is the only
        // thing that tells this mode from painting.
        world.resource_mut::<editor::EditorState>().mode =
            editor::Mode::testing(crate::sim::Board::new(9, 7, 1));
        assert!(
            world.run_system_once(sim_should_run).expect("ran"),
            "and must, the moment it is played"
        );
    }

    /// The versus phase gates are a pair and must never both answer yes:
    /// one drives the round, the other the results card over it.
    #[test]
    fn a_versus_round_is_either_running_or_over_and_never_both() {
        for screen in EVERY_SCREEN {
            for vphase in [VersusPhase::Running, VersusPhase::Over] {
                let running = ask(screen, Phase::Setup, vphase, |w| {
                    w.run_system_once(versus_running).expect("ran")
                });
                let over = ask(screen, Phase::Setup, vphase, |w| {
                    w.run_system_once(versus_over).expect("ran")
                });
                assert!(!(running && over), "{screen:?} in {vphase:?} is both");
                let on_the_beach = screen == Screen::Versus;
                assert_eq!(running, on_the_beach && vphase == VersusPhase::Running);
                assert_eq!(over, on_the_beach && vphase == VersusPhase::Over);
            }
        }
    }

    /// And the puzzle's three gates partition its four phases, on the
    /// puzzle screen and nowhere else.
    #[test]
    fn the_puzzle_gates_partition_its_phases() {
        for screen in EVERY_SCREEN {
            for phase in [Phase::Setup, Phase::Running, Phase::Won, Phase::Lost] {
                let gates = [
                    ask(screen, phase, VersusPhase::Running, |w| {
                        w.run_system_once(puzzle_setup).expect("ran")
                    }),
                    ask(screen, phase, VersusPhase::Running, |w| {
                        w.run_system_once(puzzle_running).expect("ran")
                    }),
                    ask(screen, phase, VersusPhase::Running, |w| {
                        w.run_system_once(puzzle_done).expect("ran")
                    }),
                ];
                let open = gates.iter().filter(|gate| **gate).count();
                if screen == Screen::Puzzle {
                    assert_eq!(open, 1, "{phase:?} matched {open} gates, not exactly one");
                } else {
                    assert_eq!(open, 0, "{screen:?} answered a puzzle gate in {phase:?}");
                }
            }
        }
    }

    /// Watching is not playing. Everything that counts a deed (the
    /// trophies, the lifetime stats) hangs off this, so a replay read as
    /// live play awards a round somebody else won, every time it is
    /// watched.
    #[test]
    fn a_round_being_watched_is_not_a_round_being_played() {
        let mut world = world(Screen::Versus, Phase::Running, VersusPhase::Running);
        assert!(
            world.run_system_once(not_watching).expect("ran"),
            "an empty playback slot is live play"
        );
        let level = crate::sim::campaign_levels()
            .first()
            .expect("a campaign level")
            .clone();
        world.insert_resource(Playback(Some((crate::sim::Replay::new(level), 0))));
        assert!(
            !world.run_system_once(not_watching).expect("ran"),
            "a loaded recording is being watched, not played"
        );
    }

    /// While a player is typing, a letter is a letter.
    ///
    /// Four places can have a line half-written in them, and each is a
    /// separate latch: miss one and the master mute fires in the middle of
    /// typing.
    #[test]
    fn any_half_typed_name_takes_the_keyboard() {
        let mut world = world(Screen::Lobby, Phase::Setup, VersusPhase::Running);
        assert!(
            !world.run_system_once(text_entry_open).expect("ran"),
            "nobody is typing to begin with"
        );

        world.resource_mut::<match_setup::MatchMenu>().naming = Some(0);
        assert!(
            world.run_system_once(text_entry_open).expect("ran"),
            "a seat being named holds the keyboard"
        );
        world.resource_mut::<match_setup::MatchMenu>().naming = None;

        world.resource_mut::<editor::EditorState>().mode =
            editor::Mode::Naming { was: String::new() };
        assert!(
            world.run_system_once(text_entry_open).expect("ran"),
            "a beach being named holds the keyboard"
        );
        world.resource_mut::<editor::EditorState>().mode = editor::Mode::Painting;

        world
            .resource_mut::<crate::app::spectators::SpectatorChat>()
            .0 = Some(String::new());
        assert!(
            world.run_system_once(text_entry_open).expect("ran"),
            "and a line a spectator is saying to the table holds it too"
        );
        world
            .resource_mut::<crate::app::spectators::SpectatorChat>()
            .0 = None;

        assert!(
            !world.run_system_once(text_entry_open).expect("ran"),
            "and the keyboard comes back when the typing stops"
        );
    }
}
