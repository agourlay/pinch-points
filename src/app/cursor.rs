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
use crate::app::settings::GameSettings;
use crate::app::{
    Campaign, LoadLevel, Paused, PendingActions, Phase, PlacementDenied, Screen, Sim,
};
use crate::sim::{Direction, PlayerAction};
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

pub struct KeyMap {
    pub moves: [(KeyCode, i16, i16); 4],
    pub places: [(KeyCode, Direction); 4],
    pub remove: KeyCode,
    pub clear_all: KeyCode,
}

/// How long a denied placement tints the cursor red.
const FLASH_SECS: f32 = 0.25;

/// Keyboard layout for a seat, as bound in settings (spec §8.2 defaults:
/// seat 1 on WASD + arrows, seat 2 on IJKL + numpad).
///
/// `ijkl_commits` is the §8.2 single-hand preset, which overrides seat 1's
/// four commit keys with IJKL whatever they are bound to. It exists for
/// solo play, where the second seat's keys are free.
pub fn keymap(settings: &GameSettings, player: u8, ijkl_commits: bool) -> KeyMap {
    use crate::app::binds::Action;
    let seat = usize::from(player).min(crate::app::binds::BOUND_SEATS - 1);
    let binds = &settings.binds[seat];
    let places = if player == 0 && ijkl_commits {
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

/// Tint flashing (denied) cursors red and restore their colour after.
pub fn flash_cursors(time: Res<Time>, mut cursors: Query<(&mut Cursor, &mut Sprite)>) {
    for (mut cursor, mut sprite) in &mut cursors {
        if cursor.flash > 0.0 {
            cursor.flash -= time.delta_secs();
            sprite.color = Color::srgba(1.0, 0.25, 0.2, 0.95);
        } else {
            sprite.color = bracket_color(cursor.player);
        }
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
        Cursor {
            player,
            x: 0,
            y: 0,
            repeat: Timer::from_seconds(0.0, TimerMode::Once),
            flash: 0.0,
        },
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
    mut cursors: Query<(&mut Cursor, &mut Transform)>,
) {
    let board = &sim.0;
    let ijkl = settings.ijkl_commits && *screen.get() != Screen::Versus;
    for (mut cursor, mut transform) in &mut cursors {
        // Online: the one local cursor always answers to the primary keys.
        // Local: the keyboard has two seats; 3 and up are gamepad-only.
        let map = if online.0.is_some() {
            keymap(&settings, 0, ijkl)
        } else if cursor.player < 2 {
            keymap(&settings, cursor.player, ijkl)
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
            transform.translation = layout::tile_center(board, nx, ny).extend(layout::z::CURSOR);
        }
    }
}

/// Puzzle setup phase: spend the inventory directly on the board, Enter to
/// run, N/P to jump between levels.
#[allow(clippy::too_many_arguments)]
pub fn setup_input(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<GameSettings>,
    progress: Res<crate::app::progress::Progress>,
    mut sim: ResMut<Sim>,
    mut campaign: ResMut<Campaign>,
    mut next_phase: ResMut<NextState<Phase>>,
    mut load: MessageWriter<LoadLevel>,
    mut denied: MessageWriter<PlacementDenied>,
    mut cursors: Query<&mut Cursor>,
) {
    // Esc is the pause card's key; it handles leaving.
    let Some(mut cursor) = cursors.iter_mut().find(|c| c.player == 0) else {
        return;
    };
    let map = keymap(&settings, 0, settings.ijkl_commits);
    for (key, dir) in map.places {
        if keys.just_pressed(key) && !sim.0.place_signpost(0, cursor.x, cursor.y, dir) {
            // CapPolicy::Reject enforces the inventory; surface the no-op.
            cursor.flash = FLASH_SECS;
            denied.write(PlacementDenied { player: 0 });
        }
    }
    if keys.just_pressed(map.remove) {
        let _ = sim.0.remove_signpost(0, cursor.x, cursor.y);
    }
    if keys.just_pressed(KeyCode::Enter) {
        next_phase.set(Phase::Running);
    }
    if settings.keycaps.just_pressed(&keys, 'N') {
        // Browsing forward stops at the ladder: the stage list is the only
        // way past a stage you have not cleared.
        let next = (campaign.index + 1) % campaign.levels.len();
        if progress.unlocked(&campaign, next) {
            campaign.index = next;
            load.write(LoadLevel { keep_posts: false });
        } else {
            cursor.flash = FLASH_SECS;
            denied.write(PlacementDenied { player: 0 });
        }
    }
    if settings.keycaps.just_pressed(&keys, 'P') {
        campaign.index = (campaign.index + campaign.levels.len() - 1) % campaign.levels.len();
        load.write(LoadLevel { keep_posts: false });
    }
}

/// Puzzle running phase: R resets to setup (keeping placed posts), Esc pauses.
pub fn running_input(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<GameSettings>,
    mut paused: ResMut<Paused>,
    mut load: MessageWriter<LoadLevel>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        paused.0 = !paused.0;
    }
    if settings.keycaps.just_pressed(&keys, 'R') {
        load.write(LoadLevel { keep_posts: true });
    }
}

/// Puzzle won/lost phase: Enter advances (or retries after a loss), R replays.
pub fn done_input(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<GameSettings>,
    phase: Res<State<Phase>>,
    mut campaign: ResMut<Campaign>,
    mut next_screen: ResMut<NextState<Screen>>,
    mut load: MessageWriter<LoadLevel>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        // Back to the stage list, which is the door every puzzle is entered
        // through; Esc again from there reaches the menu.
        next_screen.set(Screen::StageSelect);
        return;
    }
    if keys.just_pressed(KeyCode::Enter) {
        if *phase.get() != Phase::Won {
            load.write(LoadLevel { keep_posts: true });
            return;
        }
        // The end of the list is the end of the list. Wrapping to level one
        // read as the game not having noticed: the card says "that was the
        // last level" and then puts you back on the first.
        if campaign.index + 1 == campaign.levels.len() {
            next_screen.set(Screen::Menu);
            return;
        }
        campaign.index += 1;
        load.write(LoadLevel { keep_posts: false });
    }
    if settings.keycaps.just_pressed(&keys, 'R') {
        load.write(LoadLevel { keep_posts: true });
    }
}

/// Versus: every player's commits go through `PendingActions` and take effect
/// on the next 30 Hz tick, the input path rollback netcode will feed.
/// One action per player per tick (spec §7.6); a later press in the same
/// window overwrites the earlier one.
pub fn versus_input(
    keys: Res<ButtonInput<KeyCode>>,
    sim: Res<Sim>,
    settings: Res<GameSettings>,
    online: Res<crate::app::net::Online>,
    mut pending: ResMut<PendingActions>,
    mut denied: MessageWriter<PlacementDenied>,
    mut cursors: Query<&mut Cursor>,
) {
    // Esc is the pause card's key; it handles leaving (including a dead
    // online session or truncated replay).
    let board = &sim.0;
    for mut cursor in &mut cursors {
        let p = cursor.player as usize;
        // Online: whatever your seat, your hands are on the primary layout.
        // Local: the keyboard seats two; pads drive seats 3 and up.
        let map = if online.0.is_some() {
            keymap(&settings, 0, false)
        } else if cursor.player < 2 {
            keymap(&settings, cursor.player, false)
        } else {
            continue;
        };
        // Online or not, a cursor carries the seat it places for: online
        // spawns one cursor and gives it the local seat.
        let seat = cursor.player;
        for (key, dir) in map.places {
            if keys.just_pressed(key) {
                if !board.can_place_signpost(seat, cursor.x, cursor.y) {
                    cursor.flash = FLASH_SECS;
                    denied.write(PlacementDenied { player: seat });
                    continue;
                }
                pending.0[p] = PlayerAction::Place {
                    x: cursor.x,
                    y: cursor.y,
                    dir,
                };
            }
        }
        if keys.just_pressed(map.remove) {
            pending.0[p] = PlayerAction::Remove {
                x: cursor.x,
                y: cursor.y,
            };
        }
        // Clear-all: queue one removal per tick until none remain, staying
        // inside the one-action-per-tick input model.
        if keys.pressed(map.clear_all)
            && let Some((x, y)) = board.first_signpost_of(cursor.player)
        {
            pending.0[p] = PlayerAction::Remove { x, y };
        }
    }
}

/// Versus results: Enter continues a running series, otherwise back to
/// the menu.
///
/// Online, the host's Enter calls the next round for the whole table,
/// admitting anyone who queued while this one played, and every peer is
/// walked back into the arena when the invitation lands. A joiner's Enter
/// is its own way out, since that is what leaving a match means.
pub fn versus_over_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut tournament: ResMut<crate::app::tournament::Tournament>,
    mut online: ResMut<crate::app::net::Online>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    let series_on = tournament.active && !tournament.finished;
    if let Some(session) = &mut online.0 {
        // The invitation may already have arrived, in which case nobody
        // pressed anything here: the host decided and this peer follows.
        if session.next_round {
            // Left set on purpose: `end_versus` reads it as "keep me", and
            // the interlude clears it once the session is safely through.
            next_screen.set(Screen::Interlude);
            return;
        }
        if keys.just_pressed(KeyCode::Enter) {
            if session.is_host() && series_on {
                // The host re-deals the seats for the next round and, with
                // them, the series tally: the returned standing is the same
                // wins moved onto the chairs their holders now sit in, which
                // this machine adopts so its own card agrees with the table.
                let (round, wins) = session.call_next_round(
                    crate::app::match_setup::next_round_terms(
                        session.terms,
                        session.seats,
                        crate::app::clock::fresh_seed(),
                    ),
                    tournament.round,
                    tournament.wins,
                );
                tournament.round = round;
                tournament.wins = wins;
                next_screen.set(Screen::Interlude);
            } else {
                next_screen.set(Screen::Menu);
            }
        }
        return;
    }
    if keys.just_pressed(KeyCode::Enter) {
        if series_on {
            next_screen.set(Screen::Interlude);
        } else {
            next_screen.set(Screen::Menu);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::settings::GameSettings;
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
            let map = keymap(&GameSettings::default(), player, false);
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
}
