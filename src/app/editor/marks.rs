//! What the bench draws over the board while it is edited: a solution's
//! signposts, and the mirror's lines.

use super::*;

/// A solution's signposts, drawn on the bench where they stand: faint, as
/// the hint draws its own, and only while the board is being edited, a
/// playtest being the author's own go at it.
#[derive(Component)]
pub struct SolutionGhost;

/// What the bench draws over the board and no level does: the column and
/// row numbers round its rim, which are the numbers a check's answer and a
/// level file name tiles by, and the solution it carries as faint
/// signposts. Static, so redrawn with the board after every edit, which is
/// also what takes the solution's away once an edit lets it go.
pub(super) fn spawn_bench_marks(
    commands: &mut Commands,
    board: &Board,
    state: &EditorState,
    art: &crate::app::art::Art,
) {
    use crate::app::board_render::{BoardStatic, FENCE_DEPTH};
    use crate::app::layout::{self, TILE};
    // Drawn larger and shown smaller, as the score pips are, so the
    // numbers stay sharp at the closest the camera comes.
    let oversample = layout::MAX_ZOOM;
    let ink = crate::app::palette::DARK_INK.with_alpha(0.6);
    let label = |commands: &mut Commands, text: String, at: Vec2| {
        commands.spawn((
            BoardStatic,
            Text2d::new(text),
            crate::app::menu_ui::display_font(18.0 * oversample),
            TextColor(ink),
            Transform::from_translation(at.extend(layout::z::WALL))
                .with_scale(Vec3::splat(1.0 / oversample)),
        ));
    };
    let rim = TILE / 2.0 + FENCE_DEPTH + 12.0;
    for x in 0..board.width() {
        let top = layout::tile_center(board, x, 0);
        label(commands, x.to_string(), top + Vec2::new(0.0, rim));
    }
    for y in 0..board.height() {
        let left = layout::tile_center(board, 0, y);
        label(commands, y.to_string(), left - Vec2::new(rim, 0.0));
    }
    if state.is_testing() {
        return;
    }
    spawn_mirror_lines(commands, board, state.symmetry);
    let size = Some(Vec2::splat(layout::SIGN_SIZE));
    for &(x, y, dir, seat) in &state.solution {
        if x >= board.width() || y >= board.height() {
            continue;
        }
        let color = crate::app::palette::player_color(seat).lighter(0.2);
        commands
            .spawn((
                BoardStatic,
                SolutionGhost,
                Sprite {
                    image: art.sign_shape.clone(),
                    color: color.with_alpha(0.22),
                    custom_size: size,
                    ..default()
                },
                Transform::from_translation(
                    layout::tile_center(board, x, y).extend(layout::z::SIGNPOST - 0.1),
                )
                .with_rotation(layout::dir_rotation(dir)),
            ))
            .with_child((
                Sprite {
                    image: art.sign_paint.clone(),
                    color: color.with_alpha(0.5),
                    custom_size: size,
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.01)),
            ));
    }
}

/// The lines the bench's mirror paints across ([`Symmetry`]), faint over
/// the sand while it is on: down the middle, across it, both, or a mark
/// at the centre a half turn goes round.
#[derive(Component)]
pub struct MirrorLine;

fn spawn_mirror_lines(commands: &mut Commands, board: &Board, symmetry: Symmetry) {
    use crate::app::board_render::BoardStatic;
    use crate::app::layout::{self, TILE};
    let first = layout::tile_center(board, 0, 0);
    let last = layout::tile_center(board, board.width() - 1, board.height() - 1);
    let centre = (first + last) / 2.0;
    let (w, h) = (
        f32::from(board.width()) * TILE,
        f32::from(board.height()) * TILE,
    );
    let ink = crate::app::palette::player_color(0).with_alpha(0.45);
    let mut line = |size: Vec2, turn: f32| {
        commands.spawn((
            BoardStatic,
            MirrorLine,
            Sprite {
                color: ink,
                custom_size: Some(size),
                ..default()
            },
            Transform::from_translation(centre.extend(layout::z::SIGNPOST - 0.2))
                .with_rotation(Quat::from_rotation_z(turn)),
        ));
    };
    let (down, across, mark) = match symmetry {
        Symmetry::Off => (false, false, false),
        Symmetry::LeftRight => (true, false, false),
        Symmetry::TopBottom => (false, true, false),
        Symmetry::Both => (true, true, false),
        Symmetry::HalfTurn => (false, false, true),
    };
    if down {
        line(Vec2::new(4.0, h), 0.0);
    }
    if across {
        line(Vec2::new(w, 4.0), 0.0);
    }
    if mark {
        line(Vec2::splat(TILE * 0.3), std::f32::consts::FRAC_PI_4);
    }
}
