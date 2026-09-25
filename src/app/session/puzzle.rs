//! A puzzle run: the first load, the level reload that keeps or clears
//! the posts, and whether the run was won or lost.

use super::*;

pub(in crate::app) fn send_first_load(mut load: MessageWriter<LoadLevel>) {
    load.write(LoadLevel { keep_posts: false });
}

/// Leaving a puzzle mid-run leaves `Phase` where it was, and the next
/// puzzle entered would run its first frame under it: `sim_should_run`
/// says yes to `Running`, and the stale board ticks once before the load
/// swaps it. `end_versus` puts `VersusPhase` back for the same reason.
pub(in crate::app) fn reset_puzzle_phase(mut next_phase: ResMut<NextState<Phase>>) {
    next_phase.set(Phase::Setup);
}

/// Swap the sim to the campaign's current level and rebuild everything that
/// renders from board identity (statics, signposts, crabs, cursor bounds).
#[allow(clippy::too_many_arguments)]
pub(in crate::app) fn handle_load_level(
    mut commands: Commands,
    mut messages: MessageReader<LoadLevel>,
    campaign: Res<Campaign>,
    art: Res<art::Art>,
    mut pending: ResMut<PendingActions>,
    mut sim: ResMut<Sim>,
    mut next_phase: ResMut<NextState<Phase>>,
    mut paused: ResMut<Paused>,
    sprites: BoardSprites,
    mut cursors: Query<(&mut cursor::Cursor, &mut Transform)>,
) {
    let Some(message) = messages.read().last() else {
        return;
    };
    let mut board = campaign.current().board();
    if message.keep_posts {
        let old = &sim.0;
        for y in 0..old.height().min(board.height()) {
            for x in 0..old.width().min(board.width()) {
                if let Some(sp) = old.signpost_at(x, y) {
                    board.place_signpost(sp.owner, x, y, sp.dir);
                }
            }
        }
    }
    sim.0 = board;
    pending.0 = [PlayerAction::None; MAX_PLAYERS];

    sprites.despawn_all(&mut commands);
    board_render::spawn_static_board(&mut commands, &sim.0, &art);
    // Timed levels show the tide; update_waterline hides the bars when the
    // board has no round timer, so this is free for normal puzzles.
    board_render::spawn_waterline(&mut commands);
    board_render::spawn_water_foam(&mut commands, &art);
    // The middle of the beach, and in co-op the second pair of hands a tile
    // to its right rather than on top of it, where it would hide.
    for (mut cur, mut transform) in &mut cursors {
        let right = (sim.0.width() / 2 + cur.player).min(sim.0.width().saturating_sub(1));
        cur.x = right;
        cur.y = sim.0.height() / 2;
        transform.translation = layout::tile_center(&sim.0, cur.x, cur.y).extend(layout::z::CURSOR);
    }
    paused.0 = false;
    next_phase.set(Phase::Setup);
}

pub(in crate::app) fn check_outcome(
    sim: Res<Sim>,
    campaign: Res<Campaign>,
    mut next_phase: ResMut<NextState<Phase>>,
) {
    match campaign.current().outcome(&sim.0) {
        PuzzleOutcome::Running => {}
        PuzzleOutcome::Won => next_phase.set(Phase::Won),
        PuzzleOutcome::Lost => next_phase.set(Phase::Lost),
    }
}
