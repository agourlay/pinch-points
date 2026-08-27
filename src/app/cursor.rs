//! Per-player tile cursors and keyboard input (spec §8.2, §8.3).
//!
//! Two-stage model: move a cursor, commit a direction. Out of the box
//! player 0 uses WASD + arrow keys and player 1 (versus) IJKL + numpad;
//! every one of those keys is rebindable in settings (see `binds`). Cursors
//! are tinted to the player's flag colour and shaped differently per player
//! for colour-vision accessibility (§8.3). Gamepads (§8.1) slot in here
//! once bevy_gilrs is enabled; the commit path is identical.

use crate::app::layout::{self, TILE};
use crate::app::palette;
use crate::app::settings::{CommitScheme, GameSettings};
use crate::app::{Screen, Sim};
use crate::sim::Direction;
use bevy::prelude::*;

#[derive(Component)]
pub struct Cursor {
    pub player: u8,
    pub x: u8,
    pub y: u8,
    /// Hold-to-repeat state (spec §8.4 wants the rates adjustable later).
    repeat: Timer,
    /// Seconds left of the red denied-placement flash.
    pub flash: f32,
}

impl Cursor {
    /// A cursor for `player`, parked at the origin until something centres
    /// it on a board.
    ///
    /// A constructor because the repeat timer is private to this module
    /// and one seat's cursor is now built from two of them: here, and the
    /// pad code's tests, which need a seated table to decide whose
    /// controller a raid belongs to.
    pub(crate) fn seated(player: u8) -> Cursor {
        Cursor {
            player,
            x: 0,
            y: 0,
            repeat: Timer::from_seconds(0.0, TimerMode::Once),
            flash: 0.0,
        }
    }
}

pub struct KeyMap {
    pub moves: [(KeyCode, i16, i16); 4],
    pub places: [(KeyCode, Direction); 4],
    pub remove: KeyCode,
    pub clear_all: KeyCode,
}

/// How long a denied placement tints the cursor red.
pub(super) const FLASH_SECS: f32 = 0.25;

/// Keyboard layout for a seat, as bound in settings (spec §8.2 defaults:
/// seat 1 on WASD + arrows, seat 2 on IJKL + numpad).
///
/// `commit` is the §8.2 scheme in force: the one-hand preset overrides
/// seat 1's four commit keys with IJKL whatever they are bound to. It
/// exists for solo play, where the second seat's keys are free, so a
/// caller in versus passes [`CommitScheme::Arrows`] whatever is set.
pub fn keymap(settings: &GameSettings, player: u8, commit: CommitScheme) -> KeyMap {
    use crate::app::binds::Action;
    let seat = usize::from(player).min(crate::app::binds::BOUND_SEATS - 1);
    let binds = &settings.binds[seat];
    let places = if player == 0 && commit == CommitScheme::Ijkl {
        [
            (KeyCode::KeyI, Direction::Up),
            (KeyCode::KeyK, Direction::Down),
            (KeyCode::KeyJ, Direction::Left),
            (KeyCode::KeyL, Direction::Right),
        ]
    } else {
        [
            (binds.key(Action::PlaceUp), Direction::Up),
            (binds.key(Action::PlaceDown), Direction::Down),
            (binds.key(Action::PlaceLeft), Direction::Left),
            (binds.key(Action::PlaceRight), Direction::Right),
        ]
    };
    KeyMap {
        moves: [
            (binds.key(Action::MoveUp), 0, -1),
            (binds.key(Action::MoveDown), 0, 1),
            (binds.key(Action::MoveLeft), -1, 0),
            (binds.key(Action::MoveRight), 1, 0),
        ],
        places,
        remove: binds.key(Action::Remove),
        clear_all: binds.key(Action::ClearAll),
    }
}

/// Spawn one cursor per player. Shape differs per player (§8.3): player 0 is
/// an axis-aligned square, player 1 a diamond.
fn bracket_color(player: u8) -> Color {
    palette::player_color(player).lighter(0.08).with_alpha(0.9)
}

/// Whether `seat` is being played by somebody sitting at this machine.
///
/// A cursor is spawned for exactly the seats this keyboard, these pads and
/// this screen answer for: seat one alone in a puzzle, every human seat in
/// a local match, and precisely the local seat online. Bots have none, and
/// neither do rivals down a wire.
///
/// So it is the question "is this one mine?", and it is the question the
/// beach needs asked before it makes a noise: a knock for every post a bot
/// puts down is a click track, and hearing a rival's placements online is
/// hearing something that did not happen in this room.
///
/// A **replay** and a **spectated** match answer `false` for every seat,
/// because neither spawns a cursor at all - there is no chair here to
/// place from. That is deliberate rather than incidental: a recording of
/// a six-seat round has exactly the density that made the knock worth
/// removing, and it is nobody's beach to be told about. Banks, raids,
/// gulls and the horn still sound; posts go in and out in silence, and
/// their rings and puffs stand down with them so the picture and the
/// sound agree about what a replay is.
pub fn seated_here(cursors: &Query<&Cursor>, seat: u8) -> bool {
    cursors.iter().any(|cursor| cursor.player == seat)
}

/// Tint flashing (denied) cursors red, and dim a cursor standing on a tile
/// that will refuse it.
///
/// The refusal used to be told *after* the fact and only then: you pressed,
/// the brackets went red for a quarter of a second, and you worked out
/// why. A rock, a rival's post, or an empty inventory is knowable before
/// the press, and a cursor that has gone quiet says so without a word.
/// Only where placing is the verb: in the editor the cursor paints tiles,
/// and a refusal there would mean nothing.
pub fn flash_cursors(
    time: Res<Time>,
    sim: Res<Sim>,
    screen: Res<State<Screen>>,
    mut cursors: Query<(&mut Cursor, &mut Sprite)>,
) {
    let board = &sim.0;
    let placing = matches!(screen.get(), Screen::Puzzle | Screen::Versus);
    for (mut cursor, mut sprite) in &mut cursors {
        if cursor.flash > 0.0 {
            cursor.flash -= time.delta_secs();
            sprite.color = Color::srgba(1.0, 0.25, 0.2, 0.95);
            continue;
        }
        let color = bracket_color(cursor.player);
        // A board can shrink under a cursor (`glide_cursors` guards the
        // same way). Off the board both questions below answer "no" on
        // their bounds check, which reads as "this tile refuses you" and
        // leaves the brackets dimmed until something recentres them.
        if cursor.x >= board.width() || cursor.y >= board.height() {
            sprite.color = color;
            continue;
        }
        // Only when it is the *tile* refusing. An empty inventory refuses
        // every tile on the beach, and a cursor dimmed everywhere reads as
        // a broken cursor rather than as a message - the arrow count in
        // the header is already saying that one, and `out_of_signposts`
        // exists to tell the two refusals apart.
        let tile_refuses = placing
            && !board.can_place_signpost(cursor.player, cursor.x, cursor.y)
            && !board.out_of_signposts(cursor.player, cursor.x, cursor.y);
        sprite.color = if tile_refuses {
            color.with_alpha(0.34)
        } else {
            color
        };
    }
}

/// The post a seat has committed but the board has not taken yet.
///
/// Placing is two stages - move the cursor, commit a direction - and the
/// commit is instant, so there is no aiming to preview. What there *is*,
/// between the press and the post, is a queue: a placement sits in
/// [`crate::app::PendingActions`] until the sim takes it. Locally that is
/// at most one fixed step, which is a frame or two of confirmation. Online
/// it is until the lockstep commits the frame, which on a stalled peer is
/// as long as the stall - and that is exactly when a player most needs
/// telling that their press was heard and is on its way.
#[derive(Component)]
pub struct PostGhost;

/// Draw one faint arrow per queued placement, the way a hint draws its own
/// ghost: the same art, the same near-transparency, under the real posts.
pub fn ghost_pending_posts(
    mut commands: Commands,
    sim: Res<Sim>,
    art: Res<crate::app::art::Art>,
    pending: Res<crate::app::PendingActions>,
    ghosts: Query<Entity, With<PostGhost>>,
) {
    // Rebuilt rather than diffed: there are at most six of them, and
    // `PendingActions` is taken mutably by the fixed-step driver every
    // tick, so change detection on it says nothing useful.
    for ghost in &ghosts {
        commands.entity(ghost).despawn();
    }
    let board = &sim.0;
    // Nothing is queued on a frozen board: `advance_sim` stops taking the
    // queue at the wave, so a placement pressed on the round's last frames
    // would otherwise leave its ghost on the held board for the whole of
    // the results card, advertising a post that will never go in.
    if board.round_over() {
        return;
    }
    for (seat, action) in pending.0.iter().enumerate() {
        let crate::sim::PlayerAction::Place { x, y, dir } = *action else {
            continue;
        };
        if x >= board.width() || y >= board.height() {
            continue;
        }
        commands.spawn((
            PostGhost,
            Sprite {
                image: art.arrow.clone(),
                color: palette::player_color(seat as u8)
                    .lighter(0.2)
                    .with_alpha(0.4),
                custom_size: Some(Vec2::splat(TILE * 0.88)),
                ..default()
            },
            Transform::from_translation(
                layout::tile_center(board, x, y).extend(layout::z::SIGNPOST - 0.1),
            )
            .with_rotation(layout::dir_rotation(dir)),
        ));
    }
}

/// Carry each cursor to the tile it now sits on, and breathe.
///
/// The cursor used to be teleported by whichever input system moved it,
/// which is correct to the tile and wrong to the eye: at the hold-repeat
/// rate it reads as a thing being redrawn rather than a thing being
/// steered. It is placed here instead, once, from the tile the input
/// systems agreed on.
///
/// Long jumps still snap. A round starting, a level loading and a board
/// changing size all move a cursor halfway across the beach, and sliding
/// it there would draw a line through a board it is no longer on.
pub fn glide_cursors(
    time: Res<Time>,
    sim: Res<Sim>,
    settings: Res<GameSettings>,
    mut cursors: Query<(&Cursor, &mut Transform)>,
) {
    let board = &sim.0;
    // Frame-rate independent: the same fraction of the way there per
    // second whatever the frame took.
    let closed = 1.0 - (-time.delta_secs() * 26.0).exp();
    let t = time.elapsed_secs();
    for (cursor, mut transform) in &mut cursors {
        if cursor.x >= board.width() || cursor.y >= board.height() {
            continue;
        }
        let target = layout::tile_center(board, cursor.x, cursor.y);
        let here = transform.translation.truncate();
        let at = if settings.reduced_motion || here.distance(target) > TILE * 2.0 {
            target
        } else {
            here.lerp(target, closed)
        };
        transform.translation = at.extend(layout::z::CURSOR);
        // A slow breath, phased per seat so six cursors on one beach do
        // not pulse as one.
        transform.scale = Vec3::splat(if settings.reduced_motion {
            1.0
        } else {
            1.0 + 0.035 * (t * 3.4 + f32::from(cursor.player)).sin()
        });
    }
}

/// One player's tile cursor: corner brackets in the owner's colour. Odd
/// seats rotate 45 degrees (a diamond) so overlapping cursors stay distinct.
fn cursor_sprite(commands: &mut Commands, art: &crate::app::art::Art, player: u8) {
    let rotation = if player % 2 == 1 {
        Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)
    } else {
        Quat::IDENTITY
    };
    commands.spawn((
        Cursor::seated(player),
        Sprite {
            image: art.bracket.clone(),
            color: bracket_color(player),
            custom_size: Some(Vec2::splat(TILE * if player % 2 == 1 { 0.80 } else { 1.0 })),
            ..default()
        },
        Transform::from_translation(Vec3::new(0.0, 0.0, layout::z::CURSOR)).with_rotation(rotation),
    ));
}

pub fn spawn_cursors(commands: &mut Commands, art: &crate::app::art::Art, players: u8) {
    for player in 0..players {
        cursor_sprite(commands, art, player);
    }
}

pub fn spawn_puzzle_cursor(mut commands: Commands, art: Res<crate::app::art::Art>) {
    spawn_cursors(&mut commands, &art, 1);
}

/// Versus cursors: online spawns only the local seat's cursor (rivals'
/// placements arrive over the wire); local play seats two keyboard players
/// plus one per extra connected gamepad.
pub fn spawn_versus_cursors(
    mut commands: Commands,
    art: Res<crate::app::art::Art>,
    online: Res<crate::app::net::Online>,
    playback: Res<crate::app::Playback>,
    config: Res<crate::app::match_setup::MatchConfig>,
    pads: Query<&Gamepad>,
) {
    // A replay has no cursor, for the same reason a spectator has none:
    // there is no seat here to place from. Cursor movement is live input
    // and was never recorded, so one drawn over a replay simply sits in the
    // middle of the board for the whole round looking broken.
    if playback.0.is_some() {
        return;
    }
    if online.0.is_none() && config.armed {
        // Configured match: one cursor per human seat.
        spawn_cursors(&mut commands, &art, config.seats - config.bots);
        return;
    }
    if let Some(session) = &online.0 {
        // A spectator has no cursor: there is no seat for it to place from.
        if let Some(seat) = session.session.seat() {
            cursor_sprite(&mut commands, &art, seat);
        }
        return;
    }
    spawn_cursors(&mut commands, &art, (pads.iter().count() as u8).max(2));
}

/// Snap every cursor to the board centre (used right after a mode spawns its
/// cursors over a fresh board).
pub fn center_cursors(sim: Res<Sim>, mut cursors: Query<(&mut Cursor, &mut Transform)>) {
    center_on(&sim.0, &mut cursors);
}

/// The centring itself, for callers that swap the board mid-screen (the
/// editor's resize and paste) and are not a system of their own. Movement
/// only clamps on a step, so a cursor left standing on a tile the new
/// board does not have would stay there until it was moved, and be
/// painted on in the meantime.
pub fn center_on(board: &crate::sim::Board, cursors: &mut Query<(&mut Cursor, &mut Transform)>) {
    for (mut cursor, mut transform) in cursors {
        cursor.x = board.width() / 2;
        cursor.y = board.height() / 2;
        transform.translation =
            layout::tile_center(board, cursor.x, cursor.y).extend(layout::z::CURSOR);
    }
}

/// Movement with hold-to-repeat, for every cursor; active in every phase.
pub fn move_cursor(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    sim: Res<Sim>,
    settings: Res<GameSettings>,
    screen: Res<State<Screen>>,
    online: Res<crate::app::net::Online>,
    mut cursors: Query<&mut Cursor>,
) {
    let board = &sim.0;
    // Versus shares the keyboard, so IJKL belongs to the second seat there
    // whatever the one-hand preset says.
    let commit = if *screen.get() == Screen::Versus {
        CommitScheme::Arrows
    } else {
        settings.commit
    };
    for mut cursor in &mut cursors {
        // Online: the one local cursor always answers to the primary keys.
        // Local: the keyboard has two seats; 3 and up are gamepad-only.
        let map = if online.0.is_some() {
            keymap(&settings, 0, commit)
        } else if cursor.player < 2 {
            keymap(&settings, cursor.player, commit)
        } else {
            continue;
        };
        let any_just = map.moves.iter().any(|(k, ..)| keys.just_pressed(*k));
        let any_held = map.moves.iter().any(|(k, ..)| keys.pressed(*k));
        let mut step = false;
        if any_just {
            step = true;
            cursor.repeat = Timer::from_seconds(settings.repeat_delay, TimerMode::Once);
        } else if any_held {
            cursor.repeat.tick(time.delta());
            if cursor.repeat.is_finished() {
                step = true;
                cursor.repeat = Timer::from_seconds(settings.repeat_interval, TimerMode::Once);
            }
        }
        if step {
            let (mut dx, mut dy) = (0i16, 0i16);
            for (key, kx, ky) in map.moves {
                if keys.pressed(key) {
                    dx += kx;
                    dy += ky;
                }
            }
            let nx = (i16::from(cursor.x) + dx).clamp(0, i16::from(board.width()) - 1) as u8;
            let ny = (i16::from(cursor.y) + dy).clamp(0, i16::from(board.height()) - 1) as u8;
            cursor.x = nx;
            cursor.y = ny;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::play_input::*;
    use crate::app::settings::GameSettings;
    use crate::app::{PendingActions, PlacementDenied};
    use crate::sim::PlayerAction;
    use crate::sim::{TileKind, classic_arena};
    use bevy::prelude::KeyCode;

    /// A board, a settings resource, and one cursor per seat: the smallest
    /// world the input systems will run in. No art, so no sprites: these
    /// check the decisions, not the drawing.
    fn beach(seats: u8) -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::Versus));
        app.insert_resource(Sim(classic_arena(false, seats.max(2))));
        app.insert_resource(GameSettings::default());
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<Time>();
        app.init_resource::<PendingActions>();
        app.init_resource::<crate::app::net::Online>();
        app.add_message::<PlacementDenied>();
        for player in 0..seats {
            app.world_mut().spawn((
                Cursor {
                    player,
                    x: 5,
                    y: 4,
                    repeat: Timer::from_seconds(0.0, TimerMode::Once),
                    flash: 0.0,
                },
                Transform::default(),
            ));
        }
        app
    }

    fn tap(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(key);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
    }

    fn cursor_at(app: &mut App, player: u8) -> (u8, u8) {
        app.world_mut()
            .query::<&Cursor>()
            .iter(app.world())
            .find(|c| c.player == player)
            .map(|c| (c.x, c.y))
            .expect("seat has a cursor")
    }

    /// `seated_here` is the rule three separate systems now ask before
    /// they act: whether a post knocks, whether its ring is drawn, and
    /// whose pad a raid reaches. It answers for the seats with a cursor
    /// and no others, and the seats without one are exactly the bots, the
    /// rivals down a wire, and a replay.
    #[test]
    fn only_the_seats_with_a_cursor_are_ours() {
        #[derive(Resource, Default)]
        struct Answers([bool; crate::sim::MAX_PLAYERS]);

        let mut app = beach(2);
        app.init_resource::<Answers>();
        app.add_systems(
            Update,
            |cursors: Query<&Cursor>, mut answers: ResMut<Answers>| {
                for seat in 0..crate::sim::MAX_PLAYERS {
                    answers.0[seat] = seated_here(&cursors, seat as u8);
                }
            },
        );
        app.update();
        let answers = &app.world().resource::<Answers>().0;
        assert!(answers[0] && answers[1], "the two seated players are ours");
        assert!(
            answers[2..].iter().all(|here| !here),
            "and nobody else is: {answers:?}"
        );
    }

    /// The cursor is carried to its tile rather than teleported, but a
    /// jump longer than two tiles still snaps: a round starting, a level
    /// loading and a board changing size all move it halfway across the
    /// beach, and sliding it there would draw a line through a board it is
    /// no longer on.
    #[test]
    fn a_long_jump_snaps_and_a_short_one_glides() {
        let mut app = beach(2);
        app.add_systems(Update, glide_cursors);
        // The board never changes under this test, so every tile centre it
        // needs can be read once, before the borrow checker gets involved.
        let (home, next_door, corner) = {
            let board = &app.world().resource::<Sim>().0;
            (
                layout::tile_center(board, 5, 4),
                layout::tile_center(board, 6, 4),
                layout::tile_center(board, 0, 0),
            )
        };
        // A frame has to pass for the glide to have a delta to work with.
        app.update();

        let mut place = |x: u8, y: u8| {
            let mut seats = app.world_mut().query::<&mut Cursor>();
            let world = app.world_mut();
            for mut cursor in seats.iter_mut(world) {
                if cursor.player == 0 {
                    cursor.x = x;
                    cursor.y = y;
                }
            }
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_millis(16));
            app.update();
            let mut at = app.world_mut().query::<(&Cursor, &Transform)>();
            let world = app.world();
            at.iter(world)
                .find(|(cursor, _)| cursor.player == 0)
                .map(|(_, t)| t.translation.truncate())
                .expect("seat one has a cursor")
        };

        // One tile over: on its way, not there yet.
        let stepped = place(6, 4);
        assert!(stepped != home, "it did not move at all");
        assert!(
            stepped.distance(next_door) > 1.0,
            "one tile should glide, not snap: {stepped:?}"
        );
        // Right across the board: there in one frame.
        let jumped = place(0, 0);
        assert!(
            jumped.distance(corner) < 0.01,
            "a long jump has to snap: {jumped:?} vs {corner:?}"
        );
    }

    /// The keyboard drives two seats and only two: seats three and up wait
    /// for a pad rather than mirroring seat two's hands.
    #[test]
    fn the_keyboard_seats_two_and_stops() {
        let mut app = beach(4);
        app.add_systems(Update, move_cursor);

        // Seat 1 on W, seat 2 on I: the two default layouts.
        tap(&mut app, KeyCode::KeyW);
        assert_eq!(cursor_at(&mut app, 0), (5, 3), "seat 1 moved up");
        assert_eq!(cursor_at(&mut app, 1), (5, 4), "and only seat 1");

        tap(&mut app, KeyCode::KeyI);
        assert_eq!(cursor_at(&mut app, 1), (5, 3), "seat 2 moved up");
        assert_eq!(
            (cursor_at(&mut app, 2), cursor_at(&mut app, 3)),
            ((5, 4), (5, 4)),
            "seats 3 and 4 are pad seats and ignored the keyboard"
        );
    }

    /// Online there is one cursor, it may hold any seat, and it answers to
    /// the primary layout: a P4 online still plays on WASD.
    #[test]
    fn an_online_seat_plays_on_the_primary_keys() {
        let mut app = beach(0);
        app.world_mut().spawn((
            Cursor {
                player: 3,
                x: 5,
                y: 4,
                repeat: Timer::from_seconds(0.0, TimerMode::Once),
                flash: 0.0,
            },
            Transform::default(),
        ));
        app.add_systems(Update, move_cursor);

        // Offline, seat 4 is a pad seat and the keyboard does nothing.
        tap(&mut app, KeyCode::KeyA);
        assert_eq!(cursor_at(&mut app, 3), (5, 4));

        // A session that talks to nobody: `move_cursor` only asks whether
        // one exists, and an ephemeral socket keeps the test off the wire.
        let transport = crate::transport::UdpTransport::host(0).expect("ephemeral port");
        let lockstep = crate::sim::Lockstep::new(3, vec![0, 3], crate::sim::DEFAULT_DELAY);
        app.insert_resource(crate::app::net::Online(Some(
            crate::app::net::OnlineSession::new(
                transport,
                lockstep,
                4,
                crate::transport::MatchTerms::default(),
            ),
        )));
        tap(&mut app, KeyCode::KeyA);
        assert_eq!(cursor_at(&mut app, 3), (4, 4), "the same key now moves it");
    }

    /// A commit key queues a placement for the seat that pressed it, and a
    /// refused tile queues nothing, flashes the cursor, and says so.
    #[test]
    fn a_refused_placement_flashes_instead_of_queueing() {
        let mut app = beach(2);
        app.add_systems(Update, versus_input);
        app.world_mut()
            .resource_mut::<Sim>()
            .0
            .set_tile(5, 4, TileKind::Empty);

        // Empty sand: the arrow keys are seat 1's commit keys.
        tap(&mut app, KeyCode::ArrowUp);
        assert_eq!(
            app.world().resource::<PendingActions>().0[0],
            PlayerAction::Place {
                x: 5,
                y: 4,
                dir: Direction::Up
            }
        );
        assert_eq!(
            app.world().resource::<PendingActions>().0[1],
            PlayerAction::None,
            "seat 2 pressed nothing"
        );

        // A rock refuses, and the refusal is reported rather than silently
        // dropped.
        app.world_mut().resource_mut::<PendingActions>().0[0] = PlayerAction::None;
        app.world_mut()
            .resource_mut::<Sim>()
            .0
            .set_tile(5, 4, TileKind::Rock);
        tap(&mut app, KeyCode::ArrowUp);
        assert_eq!(
            app.world().resource::<PendingActions>().0[0],
            PlayerAction::None,
            "nothing was queued onto a rock"
        );
        let flash = app
            .world_mut()
            .query::<&Cursor>()
            .iter(app.world())
            .find(|c| c.player == 0)
            .map(|c| c.flash)
            .unwrap();
        assert!(flash > 0.0, "the cursor flashed red");
        assert_eq!(
            app.world()
                .resource::<Messages<PlacementDenied>>()
                .iter_current_update_messages()
                .count(),
            1
        );
    }

    /// Under the default scheme the two keyboard seats share no keys; the
    /// single-hand preset deliberately reuses IJKL and is guarded to solo
    /// modes elsewhere.
    #[test]
    fn default_keyboard_seats_do_not_collide() {
        let collect = |player: u8| -> Vec<KeyCode> {
            let map = keymap(&GameSettings::default(), player, CommitScheme::Arrows);
            let mut keys: Vec<KeyCode> = map.moves.iter().map(|(k, ..)| *k).collect();
            keys.extend(map.places.iter().map(|(k, _)| *k));
            keys.push(map.remove);
            keys.push(map.clear_all);
            keys
        };
        let p1 = collect(0);
        for key in collect(1) {
            assert!(!p1.contains(&key), "{key:?} is bound for both seats");
        }
    }

    /// A finished online match with the results card up, over a real
    /// socket: seat 0 hosts (with the beacon it carried out of the lobby),
    /// anything else joins.
    fn results_card(seat: u8, from_lobby: bool, series_on: bool) -> App {
        use crate::app::net::{Online, OnlineSession};
        use crate::app::tournament::Tournament;
        use crate::sim::{DEFAULT_DELAY, Lockstep};
        use crate::transport::{Announcer, MatchTerms, UdpTransport};

        let mut app = beach(2);
        app.init_resource::<Tournament>();
        app.init_resource::<crate::app::lobby::Homecoming>();
        app.add_systems(Update, versus_over_input);
        let host = seat == 0;
        let transport = match host {
            true => UdpTransport::host(0).expect("game socket"),
            false => UdpTransport::join(("127.0.0.1", 47999)).expect("join"),
        };
        let mut session = OnlineSession::new(
            transport,
            Lockstep::new(seat, vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        session.home.from_lobby = from_lobby;
        // A host formed in the lobby always carries the beacon out of it:
        // the two are set together at the launch.
        if host && from_lobby {
            session.stay_on_air(Announcer::new(0xB0A7).expect("announcer"));
        }
        if series_on {
            *app.world_mut().resource_mut::<Tournament>() =
                Tournament::start(crate::app::tournament::SeriesLength::BestOfFive);
        }
        app.world_mut().resource_mut::<Online>().0 = Some(session);
        app
    }

    /// Where a tap on the results card actually lands, transition applied.
    fn pressing_enter(app: &mut App) -> Screen {
        tap(app, KeyCode::Enter);
        // One more, to let the state transition apply.
        app.update();
        *app.world().resource::<State<Screen>>().get()
    }

    /// The end of a lobby match is a door back to the lobby, not out to
    /// the menu. The table came in together and goes back together, still
    /// connected, so the next game is a keypress rather than a
    /// rediscovery - which is the whole reason the session is handed over
    /// whole rather than dropped.
    #[test]
    fn the_end_of_a_lobby_match_walks_the_table_back_to_the_lobby() {
        use crate::app::lobby::Homecoming;
        use crate::app::net::Online;

        for seat in [0, 1] {
            let mut app = results_card(seat, true, false);
            assert_eq!(pressing_enter(&mut app), Screen::Lobby, "seat {seat}");
            let homecoming = app.world().resource::<Homecoming>();
            let returned = homecoming.0.as_ref().expect("the table came home");
            assert_eq!(returned.host, seat == 0);
            assert!(
                app.world().resource::<Online>().0.is_none(),
                "the session was handed over whole, not left behind"
            );
            if seat == 0 {
                assert!(
                    returned.announcer.is_some(),
                    "and the beacon came with it, to announce the beach open again"
                );
            }
        }
    }

    /// The direct `PINCH_HOST`/`PINCH_JOIN` pair never went through a
    /// lobby, so there is none to go back to: its Enter still means the
    /// menu, and nothing is left waiting in `Homecoming` for a lobby that
    /// is never opened.
    #[test]
    fn a_direct_pair_has_no_lobby_to_go_back_to() {
        use crate::app::lobby::Homecoming;

        let mut app = results_card(0, false, false);
        assert_eq!(pressing_enter(&mut app), Screen::Menu);
        assert!(app.world().resource::<Homecoming>().0.is_none());
    }

    /// Mid-series the card is a doorway between rounds, and Enter keeps
    /// its old meanings: the host calls the next round, and a joiner's
    /// Enter is still its way out, since the series plays on without it.
    /// Only once the series is over does the table walk home together.
    #[test]
    fn a_running_series_still_owns_enter() {
        use crate::app::lobby::Homecoming;

        let mut app = results_card(1, true, true);
        assert_eq!(
            pressing_enter(&mut app),
            Screen::Menu,
            "leaving a series is leaving"
        );
        assert!(app.world().resource::<Homecoming>().0.is_none());

        let mut app = results_card(0, true, true);
        assert_eq!(
            pressing_enter(&mut app),
            Screen::Interlude,
            "the host calls the next round instead"
        );
        assert!(app.world().resource::<Homecoming>().0.is_none());
    }

    /// The Enter that ends the match must not also start the next one.
    ///
    /// A host lands back in the lobby with its peers still aboard, and
    /// there Enter is the launch key: one press that both left the
    /// results card and reached `host_tick` would deal a fresh round
    /// before anybody had read the scores. Nothing in this file prevents
    /// that - what does is the order the engine runs in, so that is what
    /// this checks, with the real input plugin rather than a hand-set
    /// resource: a press is cleared in `PreUpdate`, the state transition
    /// lands after it, and the screen a keypress arrives on is therefore
    /// the only screen that ever sees it.
    #[test]
    fn one_press_is_only_ever_read_by_one_screen() {
        use bevy::input::ButtonState;
        use bevy::input::keyboard::{Key, KeyboardInput};

        #[derive(Resource, Default)]
        struct SeenInTheLobby(bool);

        let mut app = App::new();
        app.add_plugins((bevy::state::app::StatesPlugin, bevy::input::InputPlugin));
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::Versus));
        app.init_resource::<SeenInTheLobby>();
        app.add_systems(
            Update,
            (
                // Standing in for the results card: Enter leaves for the
                // lobby, exactly as `versus_over_input` does.
                (|keys: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<Screen>>| {
                    if keys.just_pressed(KeyCode::Enter) {
                        next.set(Screen::Lobby);
                    }
                })
                .run_if(in_state(Screen::Versus)),
                // Standing in for `host_tick`, where Enter launches.
                (|keys: Res<ButtonInput<KeyCode>>, mut seen: ResMut<SeenInTheLobby>| {
                    seen.0 |= keys.just_pressed(KeyCode::Enter);
                })
                .run_if(in_state(Screen::Lobby)),
            ),
        );

        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        // The key is never released: a player's finger is still on it as
        // the lobby comes up, which is the whole worry.
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Lobby
        );
        assert!(
            !app.world().resource::<SeenInTheLobby>().0,
            "the lobby read the Enter that was meant for the results card"
        );
    }
}
