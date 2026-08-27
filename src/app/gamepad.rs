//! Gamepad input (spec §8.1). The face buttons form a diamond of four
//! directions, so placement maps them directly: top places Up, bottom Down,
//! left Left, right Right. D-pad or left stick moves the cursor, L1 removes,
//! R1 clears own signposts, Start pauses.
//!
//! Pads fill the human seats **from the highest seat downward**, so every
//! two-player setup works without configuration: one pad drives P2 beside
//! a P1 keyboard, two pads drive P2 and P1, and no pads leaves both
//! keyboard layouts. With four humans, pads land on P4 and P3; with six,
//! on P6 down to P3. The keyboard always stays usable for seats 1-2; both
//! feed the identical commit paths as §8.2.

use crate::app::cursor::Cursor;
use crate::app::settings::{GameSettings, SeatInput};
use crate::app::{PendingActions, Phase, PlacementDenied, Sim};
use crate::sim::{Direction, MAX_PLAYERS, PlayerAction};
use bevy::input::gamepad::{GamepadRumbleIntensity, GamepadRumbleRequest};
use bevy::prelude::*;

/// Pad claims, in claim order. Pad index `i` (claimed, or connection order
/// when no ceremony ran) drives the `i`-th human seat **counting from the
/// top**: with cursors for P1 and P2, pad 0 is P2 and pad 1 is P1; with
/// four humans, pad 0 is P4. This makes keyboard+pad, two pads, and two
/// keyboards all work for two players with zero setup.
#[derive(Resource, Default)]
pub struct PadSeats(pub Vec<Entity>);

/// The pad entity at claim/connection index `index`.
///
/// Split from [`nth_pad`] because reading a pad wants the component and
/// rumbling one wants the entity, and the rule for *which* pad an index
/// means must not be written down twice.
fn nth_pad_entity(
    pads: &Query<(Entity, &Gamepad)>,
    claims: &PadSeats,
    index: usize,
) -> Option<Entity> {
    if claims.0.is_empty() {
        return pads.iter().nth(index).map(|(entity, _)| entity);
    }
    let entity = *claims.0.get(index)?;
    pads.contains(entity).then_some(entity)
}

/// The pad at claim/connection index `index`.
fn nth_pad<'a>(
    pads: &'a Query<(Entity, &Gamepad)>,
    claims: &PadSeats,
    index: usize,
) -> Option<&'a Gamepad> {
    let entity = nth_pad_entity(pads, claims, index)?;
    pads.get(entity).ok().map(|(_, pad)| pad)
}

/// Pad index for `player` among the live cursor set.
///
/// A seat that named a controller in settings gets that one, and nobody
/// else does. The rest rank from the top down (the highest seat takes the
/// lowest pad nobody claimed), which is the rule that makes keyboard+pad,
/// two pads and two keyboards all work with no setup at all. Online has a
/// single local cursor, which therefore takes the first free pad whatever
/// its seat.
fn pad_index_of(settings: &GameSettings, players: &[u8], player: u8) -> Option<usize> {
    match seat_choice(settings, player) {
        Some(SeatInput::Keys) => return None,
        Some(SeatInput::Pad(n)) => return Some(usize::from(n)),
        Some(SeatInput::Auto) | None => {}
    }
    // Auto seats, highest first, over the pads no seat asked for by name.
    let mut autos: Vec<u8> = players
        .iter()
        .copied()
        .filter(|&p| {
            !matches!(
                seat_choice(settings, p),
                Some(SeatInput::Pad(_) | SeatInput::Keys)
            )
        })
        .collect();
    autos.sort_unstable();
    let rank = autos.iter().rev().position(|&p| p == player)?;
    (0..).filter(|index| !named(settings, *index)).nth(rank)
}

/// What a seat asked for, or `None` for the seats past the bound two,
/// which have no keyboard of their own and so nothing to choose between.
fn seat_choice(settings: &GameSettings, player: u8) -> Option<SeatInput> {
    settings.seat_input.get(usize::from(player)).copied()
}

/// Whether some seat named this pad index outright.
fn named(settings: &GameSettings, index: usize) -> bool {
    settings
        .seat_input
        .iter()
        .any(|choice| matches!(choice, SeatInput::Pad(n) if usize::from(*n) == index))
}

/// Seats the keyboard always owns; the join ceremony grows the table
/// above them.
const KEYBOARD_SEATS: usize = 2;
/// How many pads the ceremony can seat: everything the keyboard leaves.
const PAD_SEATS: usize = MAX_PLAYERS - KEYBOARD_SEATS;

/// Per-cursor repeat timer for held pad directions.
#[derive(Component)]
pub struct PadRepeat(Timer);

/// The four commit directions in face-button diamond order.
const PLACES: [(GamepadButton, Direction); 4] = [
    (GamepadButton::North, Direction::Up),
    (GamepadButton::South, Direction::Down),
    (GamepadButton::West, Direction::Left),
    (GamepadButton::East, Direction::Right),
];

fn held_direction(pad: &Gamepad, deadzone: f32) -> (i16, i16) {
    let mut dx = 0i16;
    let mut dy = 0i16;
    if pad.pressed(GamepadButton::DPadUp) {
        dy -= 1;
    }
    if pad.pressed(GamepadButton::DPadDown) {
        dy += 1;
    }
    if pad.pressed(GamepadButton::DPadLeft) {
        dx -= 1;
    }
    if pad.pressed(GamepadButton::DPadRight) {
        dx += 1;
    }
    let stick_x = pad.get(GamepadAxis::LeftStickX).unwrap_or(0.0);
    let stick_y = pad.get(GamepadAxis::LeftStickY).unwrap_or(0.0);
    if stick_x < -deadzone {
        dx -= 1;
    }
    if stick_x > deadzone {
        dx += 1;
    }
    // Stick Y is up-positive; board y grows downward.
    if stick_y > deadzone {
        dy -= 1;
    }
    if stick_y < -deadzone {
        dy += 1;
    }
    (dx.signum(), dy.signum())
}

/// Move cursors from pads (any screen with cursors). Gamepad N → player N.
pub fn pad_move_cursor(
    pads: Query<(Entity, &Gamepad)>,
    seats: Res<PadSeats>,
    time: Res<Time>,
    sim: Res<Sim>,
    settings: Res<GameSettings>,
    mut commands: Commands,
    mut cursors: Query<(Entity, &mut Cursor, Option<&mut PadRepeat>)>,
) {
    let board = &sim.0;
    let deadzone = settings.deadzone();
    let mut players: Vec<u8> = cursors.iter().map(|(_, c, ..)| c.player).collect();
    players.sort_unstable();
    for (entity, mut cursor, repeat) in &mut cursors {
        let Some(pad) = pad_index_of(&settings, &players, cursor.player)
            .and_then(|i| nth_pad(&pads, &seats, i))
        else {
            continue;
        };
        let (dx, dy) = held_direction(pad, deadzone);
        let Some(mut repeat) = repeat else {
            commands
                .entity(entity)
                .insert(PadRepeat(Timer::from_seconds(0.0, TimerMode::Once)));
            continue;
        };
        if dx == 0 && dy == 0 {
            // Released: next press steps immediately.
            repeat.0 = Timer::from_seconds(0.0, TimerMode::Once);
            continue;
        }
        repeat.0.tick(time.delta());
        if !repeat.0.is_finished() {
            continue;
        }
        let first_step = repeat.0.elapsed() == repeat.0.duration();
        repeat.0 = Timer::from_seconds(
            if first_step {
                settings.repeat_delay
            } else {
                settings.repeat_interval
            },
            TimerMode::Once,
        );
        let nx = (i16::from(cursor.x) + dx).clamp(0, i16::from(board.width()) - 1) as u8;
        let ny = (i16::from(cursor.y) + dy).clamp(0, i16::from(board.height()) - 1) as u8;
        cursor.x = nx;
        cursor.y = ny;
    }
}

/// Versus commits from pads, through the same one-action-per-tick queue the
/// keyboard uses.
pub fn pad_versus_input(
    pads: Query<(Entity, &Gamepad)>,
    seats: Res<PadSeats>,
    settings: Res<GameSettings>,
    sim: Res<Sim>,
    mut pending: ResMut<PendingActions>,
    mut denied: MessageWriter<PlacementDenied>,
    mut cursors: Query<&mut Cursor>,
) {
    let board = &sim.0;
    let mut players: Vec<u8> = cursors.iter().map(|c| c.player).collect();
    players.sort_unstable();
    for mut cursor in &mut cursors {
        let Some(pad) = pad_index_of(&settings, &players, cursor.player)
            .and_then(|i| nth_pad(&pads, &seats, i))
        else {
            continue;
        };
        let p = cursor.player as usize;
        for (button, dir) in PLACES {
            if pad.just_pressed(button) {
                if !board.can_place_signpost(cursor.player, cursor.x, cursor.y) {
                    cursor.flash = 0.25;
                    denied.write(PlacementDenied {
                        player: cursor.player,
                        out_of_signposts: board.out_of_signposts(cursor.player, cursor.x, cursor.y),
                    });
                    continue;
                }
                pending.0[p] = PlayerAction::Place {
                    x: cursor.x,
                    y: cursor.y,
                    dir,
                };
            }
        }
        if pad.just_pressed(GamepadButton::LeftTrigger) {
            pending.0[p] = PlayerAction::Remove {
                x: cursor.x,
                y: cursor.y,
            };
        }
        if pad.pressed(GamepadButton::RightTrigger)
            && let Some((x, y)) = board.first_signpost_of(cursor.player)
        {
            pending.0[p] = PlayerAction::Remove { x, y };
        }
    }
}

/// Puzzle setup commits from P1's pad: place/remove directly, Start begins
/// the run (mirrors the keyboard Enter). P1's pad is whichever one
/// [`pad_index_of`] hands the seat - the one it named in settings, or the
/// first unclaimed one - the same answer `pad_move_cursor` gives, so the
/// pad that moves the cursor is the pad that places.
pub fn pad_setup_input(
    pads: Query<(Entity, &Gamepad)>,
    seats: Res<PadSeats>,
    settings: Res<GameSettings>,
    mut sim: ResMut<Sim>,
    mut next_phase: ResMut<NextState<Phase>>,
    mut denied: MessageWriter<PlacementDenied>,
    mut cursors: Query<&mut Cursor>,
) {
    let mut players: Vec<u8> = cursors.iter().map(|c| c.player).collect();
    players.sort_unstable();
    let Some(pad) = pad_index_of(&settings, &players, 0).and_then(|i| nth_pad(&pads, &seats, i))
    else {
        return;
    };
    let Some(mut cursor) = cursors.iter_mut().find(|c| c.player == 0) else {
        return;
    };
    for (button, dir) in PLACES {
        if pad.just_pressed(button) {
            let spent = sim.0.out_of_signposts(0, cursor.x, cursor.y);
            if !sim.0.place_signpost(0, cursor.x, cursor.y, dir) {
                cursor.flash = 0.25;
                denied.write(PlacementDenied {
                    player: 0,
                    out_of_signposts: spent,
                });
            }
        }
    }
    if pad.just_pressed(GamepadButton::LeftTrigger) {
        let _ = sim.0.remove_signpost(0, cursor.x, cursor.y);
    }
    if pad.just_pressed(GamepadButton::Start) {
        next_phase.set(Phase::Running);
    }
}

/// Menu navigation from any pad: d-pad mirrors W/S/A/D, South mirrors
/// Enter, East mirrors Escape, synthesized straight into the keyboard
/// input resource so every menu keeps a single input path. Registered only
/// on menu-like screens and result phases, never during play (East places
/// Right in a round).
pub fn pad_menu_bridge(pads: Query<&Gamepad>, mut keys: ResMut<ButtonInput<KeyCode>>) {
    const MAP: [(GamepadButton, KeyCode); 6] = [
        (GamepadButton::DPadUp, KeyCode::KeyW),
        (GamepadButton::DPadDown, KeyCode::KeyS),
        (GamepadButton::DPadLeft, KeyCode::KeyA),
        (GamepadButton::DPadRight, KeyCode::KeyD),
        (GamepadButton::South, KeyCode::Enter),
        (GamepadButton::East, KeyCode::Escape),
    ];
    for pad in &pads {
        for (button, key) in MAP {
            if pad.just_pressed(button) {
                keys.press(key);
            }
            if pad.just_released(button) {
                keys.release(key);
            }
        }
    }
}

/// The press-Start-to-join ceremony on the match-setup screen: an
/// unclaimed pad pressing Start takes the next pad seat (seat 3, then 4,
/// up to the sixth), growing the seat count to fit. The keyboard keeps
/// seats 1-2, so the pads can claim every seat above them. Disconnected
/// pads lose their claim.
pub fn pad_claim_seats(
    pads: Query<(Entity, &Gamepad)>,
    mut seats: ResMut<PadSeats>,
    mut config: ResMut<crate::app::match_setup::MatchConfig>,
) {
    seats.0.retain(|&entity| pads.get(entity).is_ok());
    for (entity, pad) in &pads {
        if pad.just_pressed(GamepadButton::Start)
            && !seats.0.contains(&entity)
            && seats.0.len() < KEYBOARD_SEATS + PAD_SEATS
        {
            seats.0.push(entity);
            let needed = (KEYBOARD_SEATS + seats.0.len()) as u8;
            config.seats = config.seats.max(needed).min(MAX_PLAYERS as u8);
            config.bots = config.bots.min(config.seats - 1);
        }
    }
}

/// How hard, and for how long, a raid is felt.
///
/// Both motors, weighted to the low-frequency one: a gull carrying off
/// half a castle is a thump, not a buzz. Short enough that two raids in
/// quick succession read as two knocks rather than one long shudder.
const RAID_RUMBLE: GamepadRumbleIntensity = GamepadRumbleIntensity {
    strong_motor: 0.85,
    weak_motor: 0.45,
};
const RAID_RUMBLE_SECS: f32 = 0.32;

/// A gull on your castle, in your hands.
///
/// The `rumble` setting has been in the menu, saved to disk and translated
/// into eight languages since before this existed, and nothing anywhere
/// read it: the switch turned nothing on or off. This is the first thing
/// it does.
///
/// Only the seat that was raided, and only if that seat is sitting at
/// *this* machine holding a pad. `pad_index_of` answers over the live
/// cursor set, which is what makes that true without a special case:
/// online there is one local cursor and the rivals have none here, and in
/// local play the AI seats have none either, so a bot losing a castle
/// buzzes nobody's hands.
pub fn rumble_on_raid(
    mut events: MessageReader<crate::app::sim_events::SimEvent>,
    settings: Res<GameSettings>,
    claims: Res<PadSeats>,
    pads: Query<(Entity, &Gamepad)>,
    cursors: Query<&Cursor>,
    mut seated: Local<Vec<u8>>,
    mut rumble: MessageWriter<GamepadRumbleRequest>,
) {
    use crate::app::sim_events::SimEvent;
    // No need to drain the reader on the way out: a message lives two
    // frames whoever reads it, so a raid this seat sat out has expired
    // long before the switch can be flipped back on.
    if !settings.rumble || events.is_empty() {
        return;
    }
    // Reused buffer, rebuilt on the frames a raid actually lands: this
    // runs on every frame of every round, and a fresh heap allocation to
    // list at most six seats is a poor way to spend one.
    seated.clear();
    seated.extend(cursors.iter().map(|cursor| cursor.player));
    seated.sort_unstable();
    for event in events.read() {
        let SimEvent::CastleRaided { owner, .. } = event else {
            continue;
        };
        // `pad_index_of` answers for any seat when that seat named a
        // controller outright, without ever consulting the cursor set: it
        // short-circuits on `SeatInput::Pad(n)`. So the "is this seat here"
        // question has to be asked first, or a player who bound P2 to a pad
        // for couch play feels every raid on seat two for ever after -
        // including online, where seat two is a stranger, and against AI,
        // where seat two is a bot.
        if seated.binary_search(owner).is_err() {
            continue;
        }
        let Some(pad) = pad_index_of(&settings, &seated, *owner)
            .and_then(|index| nth_pad_entity(&pads, &claims, index))
        else {
            continue;
        };
        rumble.write(GamepadRumbleRequest::Add {
            duration: std::time::Duration::from_secs_f32(RAID_RUMBLE_SECS),
            intensity: RAID_RUMBLE,
            gamepad: pad,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::settings::{GameSettings, SeatInput};
    use crate::app::sim_events::SimEvent;
    use crate::sim::classic_arena;

    fn with(choices: [SeatInput; 2]) -> GameSettings {
        GameSettings {
            seat_input: choices,
            ..GameSettings::default()
        }
    }

    /// Left alone, pads fill seats from the top down; a lone cursor (puzzle,
    /// online) takes the first pad whatever its seat number.
    #[test]
    fn pads_map_top_down() {
        let auto = with([SeatInput::Auto; 2]);
        assert_eq!(pad_index_of(&auto, &[0, 1], 1), Some(0));
        assert_eq!(pad_index_of(&auto, &[0, 1], 0), Some(1));
        assert_eq!(pad_index_of(&auto, &[0, 1, 2, 3], 3), Some(0));
        assert_eq!(pad_index_of(&auto, &[0, 1, 2, 3], 0), Some(3));
        assert_eq!(pad_index_of(&auto, &[2], 2), Some(0), "online single seat");
        assert_eq!(pad_index_of(&auto, &[0, 1], 2), None, "no cursor, no pad");
    }

    /// A seat that names a controller gets that one, and the seat that
    /// would have inherited it under the top-down rule does not.
    #[test]
    fn a_named_controller_belongs_to_the_seat_that_named_it() {
        let mine = with([SeatInput::Pad(0), SeatInput::Auto]);
        assert_eq!(pad_index_of(&mine, &[0, 1], 0), Some(0), "P1 asked for it");
        assert_eq!(
            pad_index_of(&mine, &[0, 1], 1),
            Some(1),
            "P2 takes the next one free, not the claimed one"
        );
    }

    /// Keyboard-only means no pad, and the pad it would have had goes to
    /// whoever is next in line rather than going spare.
    #[test]
    fn keyboard_only_gives_its_pad_up() {
        let quiet = with([SeatInput::Auto, SeatInput::Keys]);
        assert_eq!(pad_index_of(&quiet, &[0, 1], 1), None);
        assert_eq!(pad_index_of(&quiet, &[0, 1], 0), Some(0));
    }

    /// Both seats naming the same controller is a thing a player can do by
    /// walking the dial past it, and it must not panic or hand it to a
    /// third seat; they simply share it.
    #[test]
    fn two_seats_may_name_the_same_controller() {
        let both = with([SeatInput::Pad(1), SeatInput::Pad(1)]);
        assert_eq!(pad_index_of(&both, &[0, 1], 0), Some(1));
        assert_eq!(pad_index_of(&both, &[0, 1], 1), Some(1));
        // And a third seat skips the claimed one.
        assert_eq!(pad_index_of(&both, &[0, 1, 2], 2), Some(0));
    }

    /// A world with `seats` cursors and `pads` controllers plugged in.
    /// No window, no art: this checks who gets buzzed, not what it feels
    /// like.
    fn table(seats: u8, pads: usize) -> App {
        let mut app = App::new();
        app.insert_resource(Sim(classic_arena(false, seats.max(2))));
        app.insert_resource(GameSettings::default());
        app.init_resource::<PadSeats>();
        app.add_message::<SimEvent>();
        app.add_message::<GamepadRumbleRequest>();
        for player in 0..seats {
            app.world_mut().spawn(Cursor::seated(player));
        }
        for _ in 0..pads {
            app.world_mut().spawn(Gamepad::default());
        }
        app.add_systems(Update, rumble_on_raid);
        app
    }

    /// Every pad asked to rumble since this was last called.
    fn buzzed(app: &mut App) -> Vec<Entity> {
        let mut messages = app
            .world_mut()
            .resource_mut::<bevy::ecs::message::Messages<GamepadRumbleRequest>>();
        let out: Vec<Entity> = messages.drain().map(|request| request.gamepad()).collect();
        out
    }

    /// Raid `owner`'s castle and answer every rumble that came of it.
    fn raid(app: &mut App, owner: u8) -> Vec<Entity> {
        app.world_mut().write_message(SimEvent::CastleRaided {
            owner,
            pos: Vec2::ZERO,
            lost: 4,
        });
        app.update();
        buzzed(app)
    }

    /// The pad on the raided seat's own hands, and nobody else's. Pads
    /// fill seats from the top down, so with two seats and one pad the
    /// controller is seat two's: a raid on seat two is felt and a raid on
    /// seat one, who is on the keyboard, is not.
    #[test]
    fn a_raid_is_felt_by_the_seat_that_was_raided() {
        let mut app = table(2, 1);
        let pad = app
            .world_mut()
            .query_filtered::<Entity, With<Gamepad>>()
            .single(app.world())
            .expect("one pad");
        assert_eq!(raid(&mut app, 1), vec![pad], "seat two holds the pad");
        assert!(
            raid(&mut app, 0).is_empty(),
            "seat one is on the keyboard and has nothing to buzz"
        );
    }

    /// Two pads, two seats: each seat's own controller and only that one.
    #[test]
    fn two_pads_are_told_apart() {
        let mut app = table(2, 2);
        let top = raid(&mut app, 1);
        let next = raid(&mut app, 0);
        assert_eq!(top.len(), 1);
        assert_eq!(next.len(), 1);
        assert_ne!(top, next, "one raid must not buzz both hands");
    }

    /// Once the join ceremony has claimed pads, claim order decides which
    /// controller an index means - not the order the operating system
    /// happened to hand them over in. Reversing the claims must reverse
    /// which hands feel the raid.
    #[test]
    fn a_claimed_pad_beats_the_order_it_was_plugged_in() {
        let mut app = table(2, 2);
        let plugged: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<Gamepad>>()
            .iter(app.world())
            .collect();
        assert_eq!(plugged.len(), 2);
        let claimed: Vec<Entity> = plugged.iter().copied().rev().collect();
        app.world_mut().resource_mut::<PadSeats>().0 = claimed.clone();
        assert_eq!(
            raid(&mut app, 1),
            vec![claimed[0]],
            "the top seat holds whichever pad claimed first"
        );
    }

    /// A seat that named a controller in settings still has to be *here*
    /// to feel anything. `pad_index_of` answers for a named pad whoever
    /// holds the seat, so this is the case the cursor set has to veto:
    /// binding P2 to a pad on the couch must not buzz for seat two in an
    /// online match, where seat two is somebody else entirely.
    #[test]
    fn a_bound_pad_still_only_answers_for_a_seat_that_is_here() {
        // Two pads, one player. Seat two has named the first pad from some
        // earlier couch session; it is a bot or a stranger in this one.
        let mut app = table(1, 2);
        app.world_mut().resource_mut::<GameSettings>().seat_input[1] = SeatInput::Pad(0);
        assert!(
            raid(&mut app, 1).is_empty(),
            "seat two is bound to a pad here but is not sitting here"
        );
        assert_eq!(
            raid(&mut app, 0).len(),
            1,
            "and seat one still feels its own, on the pad seat two did not claim"
        );
    }

    /// A seat nobody is sitting at locally - a bot, or a rival on the
    /// other end of a wire - has no cursor here, so its castle falling
    /// buzzes nothing on this machine.
    #[test]
    fn a_seat_that_is_not_here_buzzes_nothing() {
        let mut app = table(2, 1);
        assert!(
            raid(&mut app, 3).is_empty(),
            "seat four is not at this table"
        );
    }

    /// The switch in the menu is the switch: off is silent, and it was
    /// wired to nothing at all before this.
    #[test]
    fn the_setting_turns_it_off() {
        let mut app = table(2, 1);
        app.world_mut().resource_mut::<GameSettings>().rumble = false;
        assert!(raid(&mut app, 1).is_empty());
    }
}
