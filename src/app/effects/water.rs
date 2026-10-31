//! Water and sand underfoot: splashes into pools, the golden call's rings,
//! and the prints crabs leave.

use super::*;

/// Dry sand thrown up by something landing in it.
pub(crate) const SAND_KICK: Color = Color::srgba(0.93, 0.86, 0.70, 0.95);

/// The most splashes a frame throws: a stream of crabs wading into a pool
/// lane is one crab after another, and past a few a frame it is spray,
/// not splashes.
const SPLASHES_PER_FRAME: usize = 4;

/// A splash where something wades into a pool: a ring on the water and a
/// few drops thrown up out of it.
fn splash(commands: &mut Commands, rng: &mut VisualRng, art: &Art, pos: Vec2, size: f32) {
    ring(
        commands,
        art,
        pos,
        Color::srgba(0.86, 0.95, 1.0, 0.55),
        TILE * 0.28 * size,
        2.2,
        0.45,
    );
    burst(
        commands,
        rng,
        &Burst {
            image: art.puff.clone(),
            pos,
            color: Color::srgba(0.78, 0.90, 0.98, 0.9),
            count: (5.0 * size).round() as usize,
            size: 6.0 * size,
            speed: 55.0,
            gravity: 120.0,
        },
    );
}

/// Whether the tile at `index` is a pool.
fn in_pool(board: &crate::sim::Board, index: u16) -> bool {
    let (x, y) = board.coords_u8(index);
    board.tile_at(x, y) == TileKind::Pool
}

/// Whether a creature that was `was` in a pool last frame, and is `now`,
/// has just waded in: from dry sand to water, and not when it is first
/// seen, since nothing it is standing in then is news.
fn wades_in(was: Option<bool>, now: bool) -> bool {
    now && was == Some(false)
}

/// Crabs and walking gulls splash as they wade into a pool from dry sand.
///
/// Once on the way in, not on every tile of it: a pool lane is a run of
/// pool tiles, and a stream crossing one would be spray from end to end.
/// A creature first seen is taken to be where it has always been, so a
/// level that starts with a crab in a pool, or a board reloading, does not
/// splash everything on it. Render-side: the sim is untouched.
pub fn splash_ponds(
    mut commands: Commands,
    current: CurrentBoard,
    art: Res<Art>,
    settings: Res<crate::app::settings::GameSettings>,
    mut rng: ResMut<VisualRng>,
    mut wet: Local<BoardMemory<(bool, u32), bool>>,
    mut seen: Local<bevy::platform::collections::HashSet<(bool, u32)>>,
) {
    let board = &current.sim.0;
    let wet = wet.of(&current);
    seen.clear();
    let mut thrown = 0;
    let creatures = board
        .crabs()
        .iter()
        .map(|crab| ((false, crab.id), crab.tile, crab.dir, crab.progress, 1.0))
        .chain(board.gulls().iter().filter_map(|gull| {
            matches!(gull.state, crate::sim::GullState::Walking).then_some((
                (true, gull.id),
                gull.tile,
                gull.dir,
                gull.progress,
                0.8,
            ))
        }));
    for (key, tile, dir, progress, size) in creatures {
        seen.insert(key);
        let now = in_pool(board, tile);
        let was = wet.insert(key, now);
        if wades_in(was, now) && !settings.reduced_motion && thrown < SPLASHES_PER_FRAME {
            thrown += 1;
            let pos = layout::creature_pos(board, tile, dir, progress);
            splash(&mut commands, &mut rng, &art, pos, size);
        }
    }
    wet.retain(|key, _| seen.contains(key));
}

/// Seconds between the gold rings a called hole sends out.
const GOLDEN_RING_EVERY: f32 = 0.45;

/// A gold ring out of the hole a golden crab is called to, every so often
/// for as long as the call is public (`Board::golden_call`): the banner
/// says a golden crab is coming, and this says from where, on a beach with
/// up to six holes. Reduced motion keeps the swollen hole and loses the
/// rings.
pub fn golden_call_rings(
    mut commands: Commands,
    sim: Res<crate::app::Sim>,
    art: Res<Art>,
    time: Res<Time>,
    settings: Res<crate::app::settings::GameSettings>,
    round: crate::app::round_state::RoundState,
    mut since: Local<f32>,
) {
    let Some((_, x, y)) = sim.0.golden_call() else {
        *since = GOLDEN_RING_EVERY;
        return;
    };
    // The round's time, so no rings go out over a frozen beach.
    round.run_clock(&sim.0, &mut since, time.delta_secs());
    if settings.reduced_motion || *since < GOLDEN_RING_EVERY {
        return;
    }
    *since = 0.0;
    let pos = layout::tile_center(&sim.0, x, y);
    ring(
        &mut commands,
        &art,
        pos,
        palette::GOLD,
        TILE * 0.5,
        2.4,
        0.5,
    );
}

/// Walking crabs scuff the sand: tiny alternating footprints that linger
/// and fade, plus the occasional kicked-up grain. Footfalls are paced by
/// distance walked, tracked per crab id. Pure decoration, sim untouched.
pub fn crab_trails(
    mut commands: Commands,
    current: CurrentBoard,
    art: Res<Art>,
    settings: Res<crate::app::settings::GameSettings>,
    mut rng: ResMut<VisualRng>,
    mut footfalls: Local<BoardMemory<u32, Footfall>>,
    mut seen: Local<bevy::platform::collections::HashSet<u32>>,
) {
    if settings.reduced_motion {
        return;
    }
    const STRIDE: f32 = 20.0;
    let board = &current.sim.0;
    let footfalls = footfalls.of(&current);
    seen.clear();
    for crab in board.crabs() {
        seen.insert(crab.id);
        let pos = layout::creature_pos(board, crab.tile, crab.dir, crab.progress);
        // A crab seen for the first time lands on a zero-length step
        // below, which the stride check skips.
        let fall = footfalls.entry(crab.id).or_insert(Footfall {
            last: pos,
            foot: Foot::Right,
        });
        let step = pos - fall.last;
        if step.length() < STRIDE {
            continue;
        }
        let side = step.normalize_or_zero().perp() * fall.foot.side();
        fall.last = pos;
        fall.foot = fall.foot.other();
        let (x, y) = board.coords_u8(crab.tile);
        if board.tile_at(x, y) != TileKind::Empty {
            continue; // prints only show on dry sand
        }
        let life = rng.range(1.4, 2.2);
        let size = rng.range(3.0, 4.2);
        commands.spawn((
            Particle {
                life,
                // A print is pressed into the sand, not dropped on it: it
                // arrives over a moment as the weight goes on.
                fade_in: 0.05,
                // Its size is its quad's scale, which the particle keeps.
                size,
                // The spawn alpha is honoured (see `Particle::shade`), so
                // a lighter print dims every print in the game.
                peak: Some(PRINT.alpha()),
                ..default()
            },
            art.shaded.quad(
                &art.shaded.prints,
                Transform::from_translation((pos + side).extend(layout::z::WET + 0.05)),
                Vec2::splat(size),
            ),
            crate::app::materials::fade_tag(PRINT.alpha()),
        ));
        // Now and then a grain of sand kicks up behind the crab.
        if rng.range(0.0, 1.0) < 0.3 {
            commands.spawn((
                Particle {
                    velocity: -step.normalize_or_zero() * rng.range(12.0, 26.0),
                    gravity: Vec2::new(0.0, -60.0),
                    spin: rng.range(-3.0, 3.0),
                    grow: 0.5,
                    ramp: Some((SAND_DRY, SAND_DAMP)),
                    life: rng.range(0.25, 0.4),
                    ..default()
                },
                layout::image_sprite(&art.puff, SAND_DRY, Vec2::splat(rng.range(5.0, 8.0))),
                Transform::from_translation(pos.extend(layout::z::PARTICLE)),
            ));
        }
    }
    // A set, not a list. This sweeps one entry per crab and asks after
    // each, so a linear scan makes it quadratic on the busiest boards -
    // and a busy six-seat beach carries well over a hundred crabs, every
    // frame, purely to take out the ones that have gone.
    footfalls.retain(|id, _| seen.contains(id));
}

#[cfg(test)]
mod tests {
    use super::super::tests::{drawn, stage};
    use super::*;

    /// Run `system` once on `first`, then once on `second` laid out in its
    /// place, and count what the second frame drew.
    fn drawn_on_the_next_board<M>(
        system: impl IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, M>,
        first: crate::sim::Board,
        second: crate::sim::Board,
    ) -> usize {
        let mut app = stage(1, false);
        app.insert_resource(crate::app::Sim(first));
        app.add_systems(
            Update,
            (crate::app::sim_events::track_board, system).chain(),
        );
        app.update();
        drawn(&mut app);
        app.insert_resource(crate::app::Sim(second));
        app.update();
        drawn(&mut app)
    }

    /// Crab ids start again at zero on every board. Remembered by id
    /// alone, the new board's first crab inherited the old one's: dry sand
    /// there and a pool here was a splash on the first frame.
    #[test]
    fn a_crab_new_to_the_beach_has_not_waded_in() {
        use crate::sim::{CrabKind, Direction, Handedness};
        let mut dry = crate::sim::Board::new(6, 4, 7);
        dry.spawn_crab(1, 1, Direction::Right, Handedness::Left, CrabKind::Common);
        let mut wet = crate::sim::Board::new(6, 4, 8);
        wet.set_tile(1, 1, TileKind::Pool);
        wet.spawn_crab(1, 1, Direction::Right, Handedness::Left, CrabKind::Common);
        assert_eq!(dry.crabs()[0].id, wet.crabs()[0].id, "the same id");
        assert_eq!(drawn_on_the_next_board(splash_ponds, dry, wet), 0);
    }

    /// And the footprints: the new board's crab across the beach from the
    /// old one's namesake pressed a print halfway between on its first
    /// frame, from a step it never took.
    #[test]
    fn a_crab_new_to_the_beach_has_not_walked_there() {
        use crate::sim::{CrabKind, Direction, Handedness};
        let mut west = crate::sim::Board::new(6, 4, 7);
        west.spawn_crab(0, 1, Direction::Right, Handedness::Left, CrabKind::Common);
        let mut east = crate::sim::Board::new(6, 4, 8);
        east.spawn_crab(5, 2, Direction::Left, Handedness::Left, CrabKind::Common);
        assert_eq!(west.crabs()[0].id, east.crabs()[0].id, "the same id");
        assert_eq!(drawn_on_the_next_board(crab_trails, west, east), 0);
    }

    /// A splash asks the tile a creature is on, by the board's own count:
    /// the pool it is in, and not the sand beside it or a row over.
    #[test]
    fn in_pool_reads_the_creatures_own_tile() {
        let mut board = crate::sim::Board::new(5, 4, 1);
        board.set_tile(2, 1, TileKind::Pool);
        let index = |x: u16, y: u16| y * 5 + x;
        assert!(in_pool(&board, index(2, 1)));
        assert!(!in_pool(&board, index(1, 1)), "the sand beside it");
        assert!(!in_pool(&board, index(2, 2)), "the row below");
    }

    /// A splash is the step from sand into water, once: not every tile of
    /// a pool lane, not the step back out, and not a crab that was already
    /// wading when it was first seen.
    #[test]
    fn a_splash_is_wading_in_and_only_that() {
        assert!(wades_in(Some(false), true), "sand to water");
        assert!(!wades_in(Some(true), true), "water to water, along a lane");
        assert!(!wades_in(Some(true), false), "climbing out");
        assert!(!wades_in(Some(false), false), "sand to sand");
        assert!(!wades_in(None, true), "first seen, already in");
    }
}
