//! A press of the loaded brush and the stroke dragged from it, mirrored
//! fairly to every seat.

use super::*;

/// A brush held down and dragged: the tile the stroke began on, which
/// every tile the cursor then crosses is made like
/// ([`stroke_onto`]), and the last tile it touched.
#[derive(Clone, Debug)]
pub(super) struct Stroke {
    from: (u8, u8),
    last: (u8, u8),
    /// Every tile the stroke has made, its own and its mirror's copies.
    done: Vec<(u8, u8)>,
    /// Whether the stroke has an edit in the history yet, which its later
    /// steps carry on. A press that changed nothing has none, and the
    /// first tile its drag changes is its own step back: merged into the
    /// edit before it, one Z took back the drag and an unrelated edit.
    recorded: bool,
}

/// A press of the loaded brush at `at`, mirrored, which begins a stroke.
/// The two brushes that cycle step on past what the mirror cannot copy
/// fairly there, a castle to the next seat that it can and a hole to the
/// next way, and so to nothing when none can; a press that had to skip
/// begins no stroke, since what it left is not what its brush paints.
///
/// A press ends the stroke before it, whatever comes of it: a letter the
/// mirror refused left the old stroke going with the new brush, which
/// made each tile crossed like a first tile the new brush never painted.
pub(super) fn press(
    state: &mut EditorState,
    board: &mut Board,
    at: (u8, u8),
    period: u32,
    tr: &Tr,
) {
    state.stroke = None;
    let (fair, refused) = press_fairly(state, board, at, period);
    if let Some(why) = refused {
        state.feedback = unfair_line(why, tr);
    }
    if let Some(Fair { written, skipped }) = fair {
        let recorded = state.edited(board);
        if !skipped {
            state.stroke = Some(Stroke::at(at, written, recorded));
        }
    }
}

/// What a press of the loaded brush at `at` makes of `board` ([`press`]),
/// and the last way it was refused: the one answer the press and the
/// readout's "next:" both read (`palette::under_and_next`), so the line
/// cannot promise a castle the mirror will not let the key paint.
///
/// A crab or gull the mirror will not have facing across its line is
/// turned on round to a way it will, as a hole is: set down facing right,
/// the brush could otherwise never put one on the middle column.
pub(super) fn press_fairly(
    state: &EditorState,
    board: &mut Board,
    at: (u8, u8),
    period: u32,
) -> (Option<Fair>, Option<Unfair>) {
    let brush = state.brush;
    let tries = match brush {
        Brush::Hole => Direction::ALL.len() + 1,
        Brush::Castle => crate::sim::MAX_PLAYERS + 1,
        Brush::Crab | Brush::Gull => Direction::ALL.len(),
        Brush::Sand | Brush::Rock | Brush::Log | Brush::Weed | Brush::Pool => 1,
    };
    let creature = matches!(brush, Brush::Crab | Brush::Gull);
    let mut tried = 0;
    try_fairly(state, board, at, Layer::Both, |_, trial| {
        tried += 1;
        match (creature, tried) {
            (true, 2..) => turn_round(trial, at),
            (_, _) => paint(trial, at.0, at.1, brush, period),
        }
        tried < tries
    })
}

/// Turn what [`brush::facing`] reads on the tile at `at` a quarter round
/// clockwise.
fn turn_round(board: &mut Board, (x, y): (u8, u8)) {
    if let Some(dir) = brush::facing(board, x, y) {
        brush::face(board, x, y, dir.right());
    }
}

/// A change [`paint_fairly`] kept.
pub(super) struct Fair {
    /// The tiles the mirror wrote.
    written: Vec<(u8, u8)>,
    /// Whether a try before the one kept was refused (and said so).
    skipped: bool,
}

/// Make a change to the tile at `at` (`change`, on a copy of the board)
/// and its copies with the mirror, and keep it; or, when the copies could
/// not be fair to every seat (weighing `layer` of the tile), leave the
/// board as it was. `change` is asked again while it says there is another
/// way to try, each try going on from the last. A refusal is said on the
/// status line, which is why a castle or a hole would not go where it was
/// put.
pub(super) fn paint_fairly(
    state: &mut EditorState,
    board: &mut Board,
    at: (u8, u8),
    layer: Layer,
    tr: &Tr,
    change: impl FnMut(&EditorState, &mut Board) -> bool,
) -> Option<Fair> {
    let (fair, refused) = try_fairly(state, board, at, layer, change);
    if let Some(why) = refused {
        state.feedback = unfair_line(why, tr);
    }
    fair
}

/// [`paint_fairly`] without the saying: the change kept, if one was, and
/// the last refusal on the way.
fn try_fairly(
    state: &EditorState,
    board: &mut Board,
    at: (u8, u8),
    layer: Layer,
    mut change: impl FnMut(&EditorState, &mut Board) -> bool,
) -> (Option<Fair>, Option<Unfair>) {
    let mut trial = board.clone();
    let mut refused = None;
    loop {
        let again = change(state, &mut trial);
        match state.mirror_tile(&mut trial, at, layer) {
            Ok(written) => {
                *board = trial;
                let skipped = refused.is_some();
                return (Some(Fair { written, skipped }), refused);
            }
            Err(why) => {
                refused = Some(why);
                if !again {
                    return (None, refused);
                }
            }
        }
    }
}

/// What the status line says of a tile the mirror could not copy fairly.
fn unfair_line(why: Unfair, tr: &Tr) -> String {
    match why {
        Unfair::Line => tr.ed_mirror_line.to_string(),
        Unfair::Across => tr.ed_mirror_across.to_string(),
        Unfair::SeatTwice => fill(
            tr.ed_mirror_seats,
            &[
                ("p", &crate::app::seat_label(tr, 4)),
                ("q", &crate::app::seat_label(tr, 5)),
            ],
        ),
    }
}

impl Stroke {
    /// A stroke begun on `tile` by a press that wrote its copies at
    /// `copies`, `recorded` if the press changed the level.
    fn at(tile: (u8, u8), copies: Vec<(u8, u8)>, recorded: bool) -> Self {
        let mut done = copies;
        done.push(tile);
        Stroke {
            from: tile,
            last: tile,
            done,
            recorded,
        }
    }

    /// Carry the stroke on as a new one from the tile it is on, if it made
    /// that tile ([`EditorState::edited`]), with no step in the history yet.
    pub(super) fn restart(&mut self) {
        if self.done.contains(&self.last) {
            self.from = self.last;
        }
        self.recorded = false;
    }
}

/// Carry a stroke on to the cursor's tile while its key is held: Space,
/// or the letter of the brush it is painting with (A on a pad, which holds
/// Space down for as long as it is). Each tile crossed is made like the
/// stroke's first ([`stroke_onto`]); letting go ends it.
///
/// A tile the stroke has already made, itself or as a mirror's copy, is
/// left as it is: made again from the stroke's first tile, a copy across
/// the line was painted the unmirrored way, and its own copy, written back
/// over the first tile, turned the stroke's castle into the other seat's.
pub(super) fn drag(
    state: &mut EditorState,
    board: &mut Board,
    editor_keys: &EditorKeys,
    keys: &ButtonInput<KeyCode>,
    here: (u8, u8),
    tr: &Tr,
) {
    let held = keys.pressed(KeyCode::Space)
        || editor_keys
            .brush(state.brush)
            .is_some_and(|letter| keys.pressed(letter));
    let Some(mut stroke) = state.stroke.take().filter(|_| held) else {
        return;
    };
    if stroke.last == here {
        state.stroke = Some(stroke);
        return;
    }
    stroke.last = here;
    if !stroke.done.contains(&here) {
        let brush = state.brush;
        let from = stroke.from;
        let written = paint_fairly(state, board, here, Layer::Both, tr, |_, trial| {
            stroke_onto(trial, from, here, brush);
            false
        });
        if let Some(Fair { written, .. }) = written {
            stroke.done.push(here);
            stroke.done.extend(written);
            stroke.recorded |= state.edit(board, stroke.recorded);
        }
    }
    state.stroke = Some(stroke);
}

/// Turn the creature or hole on a tile to the next way round, clockwise,
/// that it will still face once settled ([`settle_facings`]): a way a wall
/// would turn it straight back from is skipped. Says whether it turned.
pub(super) fn turn_here(state: &EditorState, board: &mut Board, x: u8, y: u8) -> bool {
    let Some(mut dir) = brush::facing(board, x, y) else {
        return false;
    };
    for _ in 0..3 {
        dir = dir.right();
        let mut trial = board.clone();
        brush::face(&mut trial, x, y, dir);
        settle_facings(state, &mut trial);
        if brush::facing(&trial, x, y) == Some(dir) {
            *board = trial;
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::super::tests::{hold, painting_app, step_to, tap};
    use super::*;
    use crate::sim::{CrabKind, Handedness, TileKind};

    /// A brush held down and moved paints every tile it crosses as it
    /// painted the first: the same seat's castle, not the next seat's on
    /// each tile; the same crab; and the eraser clears a row. Let go, the
    /// cursor moves without painting.
    #[test]
    fn holding_a_brush_down_paints_a_stroke() {
        let (mut app, cursor) = painting_app((1, 1));
        let tile = |app: &App, x, y| app.world().resource::<Sim>().0.tile_at(x, y);
        hold(&mut app, KeyCode::KeyC);
        assert_eq!(tile(&app, 1, 1), TileKind::Castle(0));
        step_to(&mut app, cursor, (2, 1));
        step_to(&mut app, cursor, (3, 1));
        for x in 1..=3 {
            assert_eq!(tile(&app, x, 1), TileKind::Castle(0), "x {x}");
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyC);
        step_to(&mut app, cursor, (4, 1));
        assert_eq!(tile(&app, 4, 1), TileKind::Empty, "let go");

        // Space drags the loaded brush: the eraser, back along the row.
        tap(&mut app, KeyCode::KeyX);
        hold(&mut app, KeyCode::Space);
        step_to(&mut app, cursor, (3, 1));
        step_to(&mut app, cursor, (2, 1));
        for x in 2..=4 {
            assert_eq!(tile(&app, x, 1), TileKind::Empty, "x {x}");
        }
        assert_eq!(tile(&app, 1, 1), TileKind::Castle(0), "short of the stroke");

        // A crab, copied kind, claw and all.
        step_to(&mut app, cursor, (1, 3));
        tap(&mut app, KeyCode::KeyB);
        hold(&mut app, KeyCode::KeyB);
        step_to(&mut app, cursor, (2, 3));
        let board = &app.world().resource::<Sim>().0;
        let crabs: Vec<_> = board.crabs().iter().map(|c| (c.kind, c.handed)).collect();
        assert_eq!(
            crabs,
            [
                (CrabKind::Common, Handedness::Right),
                (CrabKind::Common, Handedness::Right)
            ],
            "the second press's crab, twice"
        );
    }

    /// T turns the creature or hole under the cursor clockwise, past a way
    /// a wall would turn it straight back from, and the file reads it back
    /// facing the way the bench shows.
    #[test]
    fn t_turns_what_is_under_the_cursor() {
        let (mut app, cursor) = painting_app((2, 2));
        {
            let board = &mut app.world_mut().resource_mut::<Sim>().0;
            board.spawn_crab(2, 2, Direction::Right, Handedness::Left, CrabKind::Common);
            board.set_wall(2, 2, Direction::Down, true);
        }
        let facing = |app: &App, x, y| brush::facing(&app.world().resource::<Sim>().0, x, y);
        tap(&mut app, KeyCode::KeyT);
        assert_eq!(facing(&app, 2, 2), Some(Direction::Left), "past the wall");
        tap(&mut app, KeyCode::KeyT);
        assert_eq!(facing(&app, 2, 2), Some(Direction::Up));
        let world = app.world();
        let (state, bench) = (world.resource::<EditorState>(), &world.resource::<Sim>().0);
        let read = level_here(state, bench, "Turned").board();
        assert_eq!(read.crabs()[0].dir, Direction::Up, "the file agrees");

        // A hole turns and keeps its rate.
        step_to(&mut app, cursor, (4, 1));
        tap(&mut app, KeyCode::KeyH);
        tap(&mut app, KeyCode::KeyT);
        let tile = app.world().resource::<Sim>().0.tile_at(4, 1);
        let TileKind::Spawner(hole) = tile else {
            panic!("a hole: {tile:?}");
        };
        assert_eq!(
            (hole.dir, hole.period),
            (Direction::Down, brush::SPAWNER_PERIOD)
        );
        // Bare sand has nothing to turn.
        step_to(&mut app, cursor, (0, 4));
        tap(&mut app, KeyCode::KeyT);
        assert_eq!(
            app.world().resource::<Sim>().0.tile_at(0, 4),
            TileKind::Empty
        );
    }

    /// With the mirror on, a beach keeps fair: a castle will not go on the
    /// mirror's line (the status line says why), a hole there turns to a
    /// way its copy matches, a stroke across the line leaves its first
    /// tile and the copies it made as they were painted, and a resize that
    /// moves the middle half a tile turns the mirror off and says so.
    #[test]
    fn the_mirror_keeps_a_beach_fair() {
        let (mut app, cursor) = painting_app((4, 1));
        {
            let world = app.world_mut();
            world.resource_mut::<Sim>().0 = Board::new(9, 7, 3);
            let mut state = world.resource_mut::<EditorState>();
            state.kind = LevelKind::Arena;
            state.symmetry = Symmetry::LeftRight;
        }
        let tile = |app: &App, x, y| app.world().resource::<Sim>().0.tile_at(x, y);
        let said = |app: &App| app.world().resource::<EditorState>().feedback.clone();
        tap(&mut app, KeyCode::KeyC);
        assert_eq!(tile(&app, 4, 1), TileKind::Empty, "no castle on the line");
        assert_eq!(said(&app), crate::app::i18n::EN.ed_mirror_line);
        // And the readout does not promise the castle the key refused.
        let (_, next) = {
            let world = app.world();
            let state = world.resource::<EditorState>();
            palette::under_and_next(
                &world.resource::<Sim>().0,
                4,
                1,
                state,
                &crate::app::i18n::EN,
            )
        };
        assert_eq!(next, None, "nothing a press would change");
        tap(&mut app, KeyCode::KeyH);
        assert!(
            matches!(tile(&app, 4, 1), TileKind::Spawner(s) if s.dir == Direction::Down),
            "a hole on the line faces along it"
        );

        // A castle stroke from (3,3) over the line to (5,3), which is the
        // first tile's own copy: P1 stays P1, its copy P2.
        step_to(&mut app, cursor, (3, 3));
        hold(&mut app, KeyCode::KeyC);
        step_to(&mut app, cursor, (4, 3));
        step_to(&mut app, cursor, (5, 3));
        step_to(&mut app, cursor, (6, 3));
        assert_eq!(tile(&app, 3, 3), TileKind::Castle(0));
        assert_eq!(tile(&app, 5, 3), TileKind::Castle(1));
        assert_eq!(tile(&app, 6, 3), TileKind::Castle(0));
        assert_eq!(tile(&app, 2, 3), TileKind::Castle(1));
        assert_eq!(tile(&app, 4, 3), TileKind::Empty, "the line skipped");
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyC);
        step_to(&mut app, cursor, (6, 3));

        // 9 wide to 12: the middle moves half a tile.
        tap(&mut app, KeyCode::F5);
        let state = app.world().resource::<EditorState>();
        assert_eq!(state.symmetry, Symmetry::Off);
        assert!(
            state
                .feedback
                .starts_with(crate::app::i18n::EN.ed_mirror_moved)
        );
    }

    /// With the mirror on, a beach's crab, gull or log on the line is held
    /// to the line too: a log is refused, a crab or gull is set down
    /// facing along the line and T turns it only along it, the status line
    /// saying why; and T on a crab is judged by the crab, not by a hole
    /// under it that already faces across.
    #[test]
    fn the_mirror_keeps_creatures_and_logs_on_its_line_fair() {
        let (mut app, cursor) = painting_app((4, 1));
        {
            let world = app.world_mut();
            world.resource_mut::<Sim>().0 = Board::new(9, 7, 3);
            let mut state = world.resource_mut::<EditorState>();
            state.kind = LevelKind::Arena;
            state.symmetry = Symmetry::LeftRight;
        }
        let tile = |app: &App, x, y| app.world().resource::<Sim>().0.tile_at(x, y);
        let said = |app: &App| app.world().resource::<EditorState>().feedback.clone();
        let facing = |app: &App, x, y| brush::facing(&app.world().resource::<Sim>().0, x, y);
        let across = crate::app::i18n::EN.ed_mirror_across;
        tap(&mut app, KeyCode::KeyL);
        assert_eq!(tile(&app, 4, 1), TileKind::Empty, "no log on the line");
        assert_eq!(said(&app), across);

        step_to(&mut app, cursor, (4, 3));
        tap(&mut app, KeyCode::KeyB);
        assert_eq!(facing(&app, 4, 3), Some(Direction::Down), "along the line");
        assert_eq!(said(&app), across);
        tap(&mut app, KeyCode::KeyT);
        assert_eq!(facing(&app, 4, 3), Some(Direction::Up), "past Left");

        step_to(&mut app, cursor, (4, 5));
        tap(&mut app, KeyCode::KeyG);
        assert!(
            matches!(facing(&app, 4, 5), Some(Direction::Up | Direction::Down)),
            "{:?}",
            facing(&app, 4, 5)
        );

        // A hole facing across, put there with the mirror off, and a crab
        // on it: T turns the crab.
        {
            let world = app.world_mut();
            let mut sim = world.resource_mut::<Sim>();
            let hole = crate::sim::Spawner {
                dir: Direction::Right,
                period: 60,
            };
            sim.0.set_tile(4, 4, TileKind::Spawner(hole));
            sim.0
                .spawn_crab(4, 4, Direction::Down, Handedness::Left, CrabKind::Common);
        }
        assert_eq!(facing(&app, 4, 4), Some(Direction::Down));
        step_to(&mut app, cursor, (4, 4));
        tap(&mut app, KeyCode::KeyT);
        assert_eq!(facing(&app, 4, 4), Some(Direction::Up), "the crab turned");
    }

    /// A stroke begun on a tile its brush had already made is its own
    /// step back, not the edit before it carried on; and a wall put up
    /// while a brush is held is a step of its own too, which ends the
    /// stroke.
    #[test]
    fn a_stroke_is_one_step_back_of_its_own() {
        let (mut app, cursor) = painting_app((1, 1));
        let world = app.world_mut();
        let board = world.resource::<Sim>().0.clone();
        world.resource_mut::<EditorState>().begin_history(&board);
        let tile = |app: &App, x, y| app.world().resource::<Sim>().0.tile_at(x, y);
        tap(&mut app, KeyCode::KeyR);
        assert_eq!(tile(&app, 1, 1), TileKind::Rock);
        hold(&mut app, KeyCode::KeyR);
        step_to(&mut app, cursor, (2, 1));
        step_to(&mut app, cursor, (3, 1));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyR);
        step_to(&mut app, cursor, (3, 1));
        tap(&mut app, KeyCode::KeyZ);
        assert_eq!(tile(&app, 2, 1), TileKind::Empty, "the drag undone");
        assert_eq!(tile(&app, 1, 1), TileKind::Rock, "the rock before it stays");

        hold(&mut app, KeyCode::KeyC);
        step_to(&mut app, cursor, (4, 1));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowRight);
        app.update();
        assert!(
            app.world()
                .resource::<Sim>()
                .0
                .wall_at(4, 1, Direction::Right)
        );
        tap(&mut app, KeyCode::KeyZ);
        assert!(
            !app.world()
                .resource::<Sim>()
                .0
                .wall_at(4, 1, Direction::Right)
        );
        assert_eq!(tile(&app, 4, 1), TileKind::Castle(0), "the stroke stays");
    }

    /// A wall put up while a brush is held does not end the painting: the
    /// drag goes on painting after it, as a step back of its own.
    #[test]
    fn a_held_brush_paints_on_past_a_wall() {
        let (mut app, cursor) = painting_app((1, 1));
        let world = app.world_mut();
        let board = world.resource::<Sim>().0.clone();
        world.resource_mut::<EditorState>().begin_history(&board);
        let tile = |app: &App, x, y| app.world().resource::<Sim>().0.tile_at(x, y);
        let wall = |app: &App| {
            app.world()
                .resource::<Sim>()
                .0
                .wall_at(2, 1, Direction::Right)
        };
        hold(&mut app, KeyCode::KeyR);
        step_to(&mut app, cursor, (2, 1));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowRight);
        app.update();
        assert!(wall(&app));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::ArrowRight);
        step_to(&mut app, cursor, (3, 1));
        step_to(&mut app, cursor, (4, 1));
        assert_eq!(tile(&app, 3, 1), TileKind::Rock, "still painting");
        assert_eq!(tile(&app, 4, 1), TileKind::Rock);
        tap(&mut app, KeyCode::KeyZ);
        assert_eq!(tile(&app, 3, 1), TileKind::Empty, "the drag after the wall");
        assert!(wall(&app), "is its own step back");
        assert_eq!(tile(&app, 2, 1), TileKind::Rock);
    }

    /// A brush letter the mirror refuses ends the stroke held before it:
    /// left going with the new brush, it went on painting with a brush the
    /// mirror had just refused, over the castles it crossed.
    #[test]
    fn a_refused_brush_letter_ends_the_stroke_before_it() {
        let (mut app, cursor) = painting_app((3, 3));
        {
            let world = app.world_mut();
            let mut board = Board::new(9, 7, 3);
            board.set_tile(6, 3, TileKind::Castle(0));
            board.set_tile(2, 3, TileKind::Castle(1));
            world.resource_mut::<Sim>().0 = board;
            let mut state = world.resource_mut::<EditorState>();
            state.kind = LevelKind::Arena;
            state.symmetry = Symmetry::LeftRight;
        }
        let tile = |app: &App, x, y| app.world().resource::<Sim>().0.tile_at(x, y);
        tap(&mut app, KeyCode::KeyR);
        hold(&mut app, KeyCode::Space);
        step_to(&mut app, cursor, (4, 3));
        assert_eq!(tile(&app, 4, 3), TileKind::Rock, "a rock on the line");
        // L on the line, Space still held: the mirror refuses the log.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyL);
        app.update();
        assert_eq!(tile(&app, 4, 3), TileKind::Rock, "refused");
        step_to(&mut app, cursor, (6, 3));
        assert_eq!(tile(&app, 6, 3), TileKind::Castle(0), "left alone");
        assert_eq!(tile(&app, 2, 3), TileKind::Castle(1));
    }

    /// With the mirror on, a beach's castle stroke and a wall land on
    /// every copy, each castle its copy's seat, and Z takes the whole
    /// mirrored stroke back in one press, as it would the stroke alone.
    #[test]
    fn a_mirrored_stroke_is_one_edit_on_every_copy() {
        let (mut app, cursor) = painting_app((0, 0));
        {
            let mut state = app.world_mut().resource_mut::<EditorState>();
            state.kind = LevelKind::Arena;
            state.symmetry = Symmetry::Both;
        }
        let begin = |app: &mut App| {
            let world = app.world_mut();
            let board = world.resource::<Sim>().0.clone();
            world.resource_mut::<EditorState>().begin_history(&board);
        };
        begin(&mut app);
        let tile = |app: &App, x, y| app.world().resource::<Sim>().0.tile_at(x, y);
        hold(&mut app, KeyCode::KeyC);
        step_to(&mut app, cursor, (1, 0));
        // The bench is 6x5.
        for (x, y, seat) in [
            (0, 0, 0),
            (1, 0, 0),
            (5, 0, 2),
            (4, 0, 2),
            (0, 4, 3),
            (5, 4, 1),
        ] {
            assert_eq!(tile(&app, x, y), TileKind::Castle(seat), "({x},{y})");
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyC);
        step_to(&mut app, cursor, (1, 1));
        tap(&mut app, KeyCode::ArrowRight);
        let walls = |app: &App| {
            let board = &app.world().resource::<Sim>().0;
            [
                board.wall_at(1, 1, Direction::Right),
                board.wall_at(4, 1, Direction::Left),
                board.wall_at(1, 3, Direction::Right),
                board.wall_at(4, 3, Direction::Left),
            ]
        };
        assert_eq!(walls(&app), [true; 4]);
        tap(&mut app, KeyCode::KeyZ);
        assert_eq!(walls(&app), [false; 4], "the wall and its copies in one");
        assert_eq!(tile(&app, 5, 4), TileKind::Castle(1));
        tap(&mut app, KeyCode::KeyZ);
        for (x, y) in [(0, 0), (1, 0), (5, 0), (4, 0), (0, 4), (5, 4)] {
            assert_eq!(tile(&app, x, y), TileKind::Empty, "({x},{y}) undone");
        }
    }
}
