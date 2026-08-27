//! The named run conditions the schedule is written in.
//!
//! Every one is a `fn(..) -> bool` over the screen and phase states (and
//! the odd resource), and every one is read by `schedule.rs` alone. They
//! were split between there and `session.rs`; here the schedule reads as
//! prose against one list, and `session.rs` is left with what its own doc
//! says it is: boot, teardown, reload, tick, outcome.

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

/// Screens where a round is being played, a puzzle or a versus match, as
/// one named condition, so the schedule reads as prose.
/// The screens that stand on the postcard beach: the menu at full
/// daylight, every browsing screen behind the scrim. Board screens are
/// out: a round is played over its own sand, not over a second beach.
pub(super) fn postcard_screens(screen: Res<State<Screen>>) -> bool {
    super::postcard_screen(*screen.get())
}

pub(super) fn play_screens(screen: Res<State<Screen>>) -> bool {
    matches!(screen.get(), Screen::Puzzle | Screen::Versus)
}

/// Whether a player is typing words rather than pressing keys: a seat's
/// name, a beach's name, an address, a line of chat.
///
/// The global letter keys are mnemonics, and a mnemonic has to stand down
/// when the letter is one the player meant to write - M is in "Emma" and
/// in most sentences worth sending. It only started to matter when M
/// became the master mute: silencing the whole game halfway through
/// typing a name is a good deal louder than the theme stopping.
pub(super) fn text_entry_open(
    lobby: Res<lobby::LobbyState>,
    setup: Res<match_setup::MatchMenu>,
    editor: Res<editor::EditorState>,
) -> bool {
    lobby.typing.is_some() || setup.naming.is_some() || editor.is_naming()
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
        Screen::Editor => editor.is_testing(),
    }
}

/// The pause card is up, on any screen that can raise one.
pub(super) fn versus_running(screen: Res<State<Screen>>, phase: Res<State<VersusPhase>>) -> bool {
    *screen.get() == Screen::Versus && *phase.get() == VersusPhase::Running
}

pub(super) fn versus_over(screen: Res<State<Screen>>, phase: Res<State<VersusPhase>>) -> bool {
    *screen.get() == Screen::Versus && *phase.get() == VersusPhase::Over
}

pub(super) fn puzzle_setup(screen: Res<State<Screen>>, phase: Res<State<Phase>>) -> bool {
    *screen.get() == Screen::Puzzle && *phase.get() == Phase::Setup
}

pub(super) fn puzzle_running(screen: Res<State<Screen>>, phase: Res<State<Phase>>) -> bool {
    *screen.get() == Screen::Puzzle && *phase.get() == Phase::Running
}

pub(super) fn puzzle_done(screen: Res<State<Screen>>, phase: Res<State<Phase>>) -> bool {
    *screen.get() == Screen::Puzzle && matches!(*phase.get(), Phase::Won | Phase::Lost)
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
    /// so this asks it the same question the schedule does and reads the
    /// same answer, without a schedule in the way.
    fn world(screen: Screen, phase: Phase, vphase: VersusPhase) -> World {
        let mut world = World::new();
        world.insert_resource(State::new(screen));
        world.insert_resource(State::new(phase));
        world.insert_resource(State::new(vphase));
        world.insert_resource(editor::EditorState::default());
        world.insert_resource(lobby::LobbyState::default());
        world.insert_resource(match_setup::MatchMenu::default());
        world.insert_resource(Playback::default());
        world
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
    /// one. Everything hung off this condition reads live board state, and
    /// a screen wrongly included would have those systems probing whatever
    /// board was last loaded - which is how a stale sprite becomes a panic
    /// in `tile_at` rather than a stale picture.
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
    /// board and can even run one, but nothing that belongs to a match -
    /// the announcements, the pause card, the trophy counters - belongs to
    /// a level being built.
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
    /// Every arm of this is a different answer to "is time passing", and a
    /// wrong one is either a frozen beach or a puzzle whose crabs walk
    /// while the player is still placing. The editor is the subtle case:
    /// its board ticks only while it is being playtested.
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
            editor::Mode::Testing(Box::new(crate::sim::Board::new(9, 7, 1)));
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

    /// Watching is not playing. Everything that counts a deed - the
    /// trophies, the lifetime stats - hangs off this, so a replay that
    /// read as live play would award a round somebody else already won,
    /// every time it was watched.
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
    /// Three screens can have a name half-written on them, and each one is
    /// a separate latch: the global letter keys are mnemonics, and M is in
    /// "Emma". Any one of them being missed here means the master mute
    /// fires in the middle of typing a name.
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

        world.resource_mut::<editor::EditorState>().mode = editor::Mode::Naming;
        assert!(
            world.run_system_once(text_entry_open).expect("ran"),
            "a beach being named holds the keyboard"
        );
        world.resource_mut::<editor::EditorState>().mode = editor::Mode::Painting;

        assert!(
            !world.run_system_once(text_entry_open).expect("ran"),
            "and the keyboard comes back when the typing stops"
        );
    }
}
