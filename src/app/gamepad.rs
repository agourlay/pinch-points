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
use crate::app::{PendingActions, Phase, PlacementDenied, Screen, Sim};
use crate::sim::{Direction, MAX_PLAYERS, PlayerAction};
use bevy::input::gamepad::{GamepadRumbleIntensity, GamepadRumbleRequest};
use bevy::prelude::*;

/// Pad claims, in claim order, and every pad seen, in the order it was
/// first plugged in. Pad index `i` (the claimed pads, then the rest in
/// plug order) drives the `i`-th human seat **counting from the top**:
/// with cursors for P1 and P2, pad 0 is P2 and pad 1 is P1; with four
/// humans, pad 0 is P4. This makes keyboard+pad, two pads, and two
/// keyboards all work for two players with zero setup.
#[derive(Resource, Default)]
pub struct PadSeats(pub Vec<Entity>, PlugOrder);

/// Every pad seen, in the order it was first plugged in, unplugged ones
/// included until the round is over. A pad that drops out keeps its place
/// for the rest of the round, so the pads after it do
/// not each move up a seat: counted live, P1's pad took over P2's cursor
/// the moment P2's battery died, and kept it after P2 plugged back in.
/// The same pad back (the engine gives it its old entity) finds its place
/// still there; a different one takes the first place left empty.
#[derive(Default)]
pub struct PlugOrder(Vec<Entity>);

/// Keep the plug order, and let go of pads that are gone.
///
/// Only between rounds. Mid-round an unplugged pad's place (and claim)
/// holds its seat, empty, rather than handing the seat to the next pad
/// in line; afterwards both are let go, or a pad unplugged after a couch
/// match left every other pad dead for the rest of the session.
pub fn keep_pad_order(
    pads: Query<Entity, With<Gamepad>>,
    screen: Res<State<Screen>>,
    mut seats: ResMut<PadSeats>,
) {
    let PadSeats(claims, PlugOrder(order)) = &mut *seats;
    for pad in &pads {
        if order.contains(&pad) {
            continue;
        }
        // A claimed pad gone and a new one plugged in: the new one takes
        // over the claim, and the seat the claim held. Placed after every
        // claim instead, it left that seat empty for the rest of a series
        // and steered a keyboard player's seat besides.
        if let Some(at) = claims.iter().position(|&claimed| !pads.contains(claimed)) {
            let gone = std::mem::replace(&mut claims[at], pad);
            if let Some(place) = order.iter().position(|&seen| seen == gone) {
                order[place] = pad;
                continue;
            }
        }
        match order.iter().position(|&seen| !pads.contains(seen)) {
            Some(empty) => order[empty] = pad,
            None => order.push(pad),
        }
    }
    let in_a_round = matches!(
        screen.get(),
        Screen::Puzzle | Screen::Versus | Screen::Interlude
    );
    if !in_a_round {
        claims.retain(|&claimed| pads.contains(claimed));
        order.retain(|&seen| pads.contains(seen));
    }
}

/// The pad entity at claim/connection index `index`.
///
/// Split from [`nth_pad`] because reading a pad wants the component and
/// rumbling one wants the entity, and the rule for *which* pad an index
/// means is written down once.
fn nth_pad_entity(
    pads: &Query<(Entity, &Gamepad)>,
    claims: &PadSeats,
    index: usize,
) -> Option<Entity> {
    let PadSeats(claimed, PlugOrder(order)) = claims;
    // A pad not yet in the plug order (plugged in this frame) still
    // counts, after every pad that is.
    let fresh = pads
        .iter()
        .map(|(entity, _)| entity)
        .filter(|pad| !order.contains(pad));
    let entity = claimed
        .iter()
        .copied()
        .chain(
            order
                .iter()
                .copied()
                .chain(fresh)
                .filter(|pad| !claimed.contains(pad)),
        )
        .nth(index)?;
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
    // A lone cursor is the one person at this machine, whatever seat the
    // network dealt it, and plays by P1's settings row: the keyboard does
    // (`keymap(&settings, 0, ..)` online), so the pad has to as well. Read
    // by the dealt seat, a joiner in seat two whose P2 row said "keys" had
    // a dead pad all match.
    if let [only] = players
        && *only == player
    {
        return match seat_choice(settings, 0) {
            Some(SeatInput::Keys) => None,
            Some(SeatInput::Pad(n)) => Some(usize::from(n)),
            Some(SeatInput::Auto) | None => Some(0),
        };
    }
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
        // The zero-length timer is the one a fresh press starts on. Asking
        // whether elapsed equals duration says yes for every finished timer,
        // since Bevy clamps a finished one, so the hold never gets past the
        // first-step delay.
        let first_step = repeat.0.duration().is_zero();
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
    // Each cursor answers to its own pad, and in co-op both place the one
    // seat's arrows (see `Coop`).
    for mut cursor in &mut cursors {
        let Some(pad) = pad_index_of(&settings, &players, cursor.player)
            .and_then(|i| nth_pad(&pads, &seats, i))
        else {
            continue;
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
        if pad.just_pressed(GamepadButton::RightTrigger) {
            crate::app::play_input::clear_setup(&mut sim);
        }
        if pad.just_pressed(GamepadButton::Start) {
            next_phase.set(Phase::Running);
        }
    }
}

/// Menu navigation from any pad: d-pad mirrors W/S/A/D, South mirrors
/// Enter, East mirrors Escape, synthesized straight into the keyboard
/// input resource so every menu keeps a single input path. Registered only
/// on menu-like screens and result phases, never during play (East places
/// Right in a round).
///
/// East is the exception, and [`Screen::Menu`] is where it bites: Escape
/// means "back" on every screen the bridge runs on except the landing menu,
/// which has nowhere back to go and leaves the game instead
/// (`menu_scene::menu_input`). So the menu gets the other five: the
/// keyboard keeps its Escape, spelled out in the prompt line, and the pad
/// keeps every way *in* it had.
///
/// The lobby is the other exception: its d-pad is the arrow keys. The
/// lobby reads only the arrows, for walking the beach list and turning the
/// host's dials, because W there is the watch toggle; the W/S/A/D the
/// other menus take left a pad unable to walk the list at all, and made
/// Up a switch that turned a player into a spectator without a word.
pub fn pad_menu_bridge(
    pads: Query<&Gamepad>,
    screen: Res<State<Screen>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut bridged: ResMut<Bridged>,
) {
    let quits = *screen.get() == Screen::Menu;
    let lobby = *screen.get() == Screen::Lobby;
    for pad in &pads {
        for (button, key, lobby_key) in BRIDGE {
            let muted = quits && button == GamepadButton::East;
            if pad.just_pressed(button) && !muted {
                let key = if lobby { lobby_key } else { key };
                keys.press(key);
                if !bridged.0.contains(&key) {
                    bridged.0.push(key);
                }
            }
        }
    }
}

/// Each bridged button with its key, and the key the lobby hears instead.
const BRIDGE: [(GamepadButton, KeyCode, KeyCode); 6] = [
    (GamepadButton::DPadUp, KeyCode::KeyW, KeyCode::ArrowUp),
    (GamepadButton::DPadDown, KeyCode::KeyS, KeyCode::ArrowDown),
    (GamepadButton::DPadLeft, KeyCode::KeyA, KeyCode::ArrowLeft),
    (GamepadButton::DPadRight, KeyCode::KeyD, KeyCode::ArrowRight),
    (GamepadButton::South, KeyCode::Enter, KeyCode::Enter),
    (GamepadButton::East, KeyCode::Escape, KeyCode::Escape),
];

/// The keys the bridge is holding down for a pad, to let go of when the
/// button comes up.
#[derive(Resource, Default)]
pub struct Bridged(Vec<KeyCode>);

/// Let go of a bridged key once no pad is holding its button, on every
/// screen.
///
/// The press is what the bridge's screens decide, never the release. A
/// press that changes the screen is let go of on the screen it opened,
/// often one the bridge does not run on (South on the stage list opens a
/// puzzle in setup), and a release left to the bridge stranded the
/// synthesized key in `pressed` for good: `ButtonInput::press` reports
/// `just_pressed` only for a key it was not already holding, so the next
/// Enter on the won card did nothing. Asked as "is any pad still holding
/// it" rather than "did a button just come up", because a pad unplugged
/// mid-press never reports the release: its held d-pad left W down, and
/// the next round's cursor slid up on its own. Only keys the bridge
/// pressed are let go, so a keyboard player's own held W is not.
pub fn pad_bridge_release(
    pads: Query<&Gamepad>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut bridged: ResMut<Bridged>,
) {
    if bridged.0.is_empty() {
        return;
    }
    // Both keys a button can mean count as held by it: one pressed on the
    // menu and released in the lobby is still holding the menu's key.
    let held = |key: KeyCode| {
        pads.iter().any(|pad| {
            BRIDGE
                .iter()
                .any(|&(button, menu, lobby)| (menu == key || lobby == key) && pad.pressed(button))
        })
    };
    let mut kept = Vec::with_capacity(bridged.0.len());
    for &key in &bridged.0 {
        match held(key) {
            true => kept.push(key),
            false => keys.release(key),
        }
    }
    bridged.0 = kept;
}

/// The press-Start-to-join ceremony on the match-setup screen: an
/// unclaimed pad pressing Start puts one more person at the table, in a
/// seat the AI had if it had any, else in a new one. Claims drive the
/// human seats from the top down (see [`PadSeats`]), the first claim the
/// highest. Up to four: the keyboard's two seats and the pads' four make
/// the six there are. Disconnected pads lose their claim.
pub fn pad_claim_seats(
    pads: Query<(Entity, &Gamepad)>,
    mut seats: ResMut<PadSeats>,
    mut config: ResMut<crate::app::match_setup::MatchConfig>,
) {
    seats.0.retain(|&entity| pads.get(entity).is_ok());
    for (entity, pad) in &pads {
        if pad.just_pressed(GamepadButton::Start)
            && !seats.0.contains(&entity)
            && seats.0.len() < PAD_SEATS
        {
            seats.0.push(entity);
            // One person more, and only one: the AI gives up a chair if it
            // holds one, or the table grows. Counting a seat for every
            // claim above the keyboard's two instead took the AI away from
            // a player alone against it and left P2 a human nobody played.
            if config.bots > 0 {
                config.bots -= 1;
            } else {
                config.seats = (config.seats + 1).min(MAX_PLAYERS as u8);
            }
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
/// Only the seat that was raided, and only if that seat is sitting at
/// *this* machine holding a pad. `pad_index_of` answers over the live
/// cursor set, which makes that true without a special case: online there
/// is one local cursor and the rivals have none here, and in local play the
/// AI seats have none either, so a bot losing a castle buzzes nobody.
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
        // `pad_index_of` answers for any seat that named a controller
        // outright, without consulting the cursor set. So "is this seat
        // here" is asked first, or a player who bound P2 to a pad for couch
        // play feels every raid on seat two: online, where seat two is a
        // stranger, and against AI, where it is a bot.
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

    /// Online the one cursor plays by P1's settings row whatever seat the
    /// network dealt it, as its keyboard does. Read by the dealt seat, a
    /// joiner in seat two whose P2 row said "keys" had a dead pad.
    #[test]
    fn a_lone_cursor_plays_by_the_first_row() {
        let pad_for_p1 = with([SeatInput::Auto, SeatInput::Keys]);
        assert_eq!(pad_index_of(&pad_for_p1, &[1], 1), Some(0));
        let keys_for_p1 = with([SeatInput::Keys, SeatInput::Auto]);
        assert_eq!(pad_index_of(&keys_for_p1, &[1], 1), None);
        let named = with([SeatInput::Pad(2), SeatInput::Keys]);
        assert_eq!(pad_index_of(&named, &[3], 3), Some(2));
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

    /// A pad's Start puts one person more at the table, in the AI's chair
    /// while it has one, and four pads can claim.
    #[test]
    fn a_claim_takes_a_seat_from_the_ai() {
        use crate::app::match_setup::MatchConfig;
        let mut app = App::new();
        app.init_resource::<PadSeats>();
        app.insert_resource(MatchConfig {
            seats: 4,
            bots: 2,
            ..MatchConfig::default()
        });
        for _ in 0..6 {
            app.world_mut().spawn(Gamepad::default());
        }
        app.add_systems(Update, pad_claim_seats);
        let pads: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<Gamepad>>()
            .iter(app.world())
            .collect();
        let start = |app: &mut App, pad: Entity| {
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("pad");
            gamepad.digital_mut().press(GamepadButton::Start);
            app.update();
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("pad");
            gamepad.digital_mut().release(GamepadButton::Start);
            gamepad.digital_mut().clear();
        };
        start(&mut app, pads[0]);
        let config = app.world().resource::<MatchConfig>();
        assert_eq!(
            (config.seats, config.bots),
            (4, 1),
            "seat three is a person"
        );
        for &pad in &pads[1..] {
            start(&mut app, pad);
        }
        assert_eq!(app.world().resource::<PadSeats>().0.len(), PAD_SEATS);
        let config = app.world().resource::<MatchConfig>();
        assert_eq!((config.seats, config.bots), (6, 0));

        // One player against the AI: a friend's Start takes the AI's chair,
        // rather than a third seat beside an empty second.
        app.insert_resource(MatchConfig {
            seats: 2,
            bots: 1,
            ..MatchConfig::default()
        });
        app.world_mut().resource_mut::<PadSeats>().0.clear();
        start(&mut app, pads[0]);
        let config = app.world().resource::<MatchConfig>();
        assert_eq!((config.seats, config.bots), (2, 0), "two people, no AI");
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
            .resource_mut::<Messages<GamepadRumbleRequest>>();
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

    /// A pad dropping out mid-round leaves the other where it was. Counted
    /// live, P1's pad moved onto P2's seat the moment P2's pad went, and
    /// stayed there after it came back.
    #[test]
    fn an_unplugged_pad_does_not_move_the_others_up() {
        let mut app = table(2, 2);
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::Versus));
        app.add_systems(Update, keep_pad_order.before(rumble_on_raid));
        app.update();
        let top = raid(&mut app, 1);
        let bottom = raid(&mut app, 0);
        assert_eq!((top.len(), bottom.len()), (1, 1));
        assert_ne!(top, bottom);

        app.world_mut().entity_mut(top[0]).remove::<Gamepad>();
        assert_eq!(raid(&mut app, 0), bottom, "P1 keeps its own pad");
        assert!(raid(&mut app, 1).is_empty(), "P2's seat waits, empty");

        app.world_mut()
            .entity_mut(top[0])
            .insert(Gamepad::default());
        assert_eq!(raid(&mut app, 1), top, "and P2's pad comes back to it");
        assert_eq!(raid(&mut app, 0), bottom);
    }

    /// A claimed pad that dies mid-series is replaced by the next pad
    /// plugged in, on the seat the claim held.
    #[test]
    fn a_new_pad_takes_over_a_dead_claim() {
        let mut app = table(2, 2);
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::Versus));
        app.add_systems(Update, keep_pad_order.before(rumble_on_raid));
        let plugged: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<Gamepad>>()
            .iter(app.world())
            .collect();
        app.world_mut().resource_mut::<PadSeats>().0 = plugged.clone();
        app.update();
        assert_eq!(
            raid(&mut app, 1),
            vec![plugged[0]],
            "the first claim, on top"
        );

        app.world_mut().entity_mut(plugged[0]).remove::<Gamepad>();
        let spare = app.world_mut().spawn(Gamepad::default()).id();
        app.update();
        assert_eq!(raid(&mut app, 1), vec![spare], "the spare takes P2");
        assert_eq!(raid(&mut app, 0), vec![plugged[1]], "and P1 keeps its own");
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
    /// controller an index means, not the order the operating system handed
    /// them over in.
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

    /// A seat that named a controller in settings still has to be *here* to
    /// feel anything: binding P2 to a pad on the couch must not buzz for
    /// seat two in an online match, where seat two is somebody else.
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

    /// One pad on `screen`, with the bridge wired up and nothing else.
    fn bridged(screen: Screen) -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(screen));
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<Bridged>();
        app.world_mut().spawn(Gamepad::default());
        app.add_systems(
            Update,
            (
                pad_menu_bridge.run_if(not(in_state(Screen::Puzzle))),
                pad_bridge_release,
            )
                .chain(),
        );
        app
    }

    /// Press `button` on the one pad and run a frame.
    fn press(app: &mut App, button: GamepadButton) {
        let pad = app
            .world_mut()
            .query_filtered::<Entity, With<Gamepad>>()
            .single(app.world())
            .expect("one pad");
        app.world_mut()
            .get_mut::<Gamepad>(pad)
            .expect("the pad")
            .digital_mut()
            .press(button);
        app.update();
    }

    /// B on the landing menu must not reach Escape, because Escape there
    /// is `AppExit` and nothing asks first: a player reaching for "back"
    /// would close the game.
    #[test]
    fn east_does_not_quit_from_the_menu() {
        let mut app = bridged(Screen::Menu);
        press(&mut app, GamepadButton::East);
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::Escape),
            "B on the menu must not synthesize the key that leaves the game"
        );
    }

    /// Everywhere else Escape means back, which is what B should do, so
    /// the menu's exception must not cost the pad its way out of a
    /// sub-screen.
    #[test]
    fn east_still_goes_back_from_a_sub_screen() {
        for screen in [Screen::Settings, Screen::Controls, Screen::StageSelect] {
            let mut app = bridged(screen);
            press(&mut app, GamepadButton::East);
            assert!(
                app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .pressed(KeyCode::Escape),
                "B is the way back from {screen:?}"
            );
        }
    }

    /// Leaving a sub-screen with B lands on the menu, where East is
    /// suppressed. Swallowing the *release* too leaves the synthesized
    /// Escape pressed for ever, and `ButtonInput::press` only reports
    /// `just_pressed` for a key it was not already holding.
    #[test]
    fn a_press_that_changes_screen_still_releases_its_key() {
        let mut app = bridged(Screen::Settings);
        let pad = app
            .world_mut()
            .query_filtered::<Entity, With<Gamepad>>()
            .single(app.world())
            .expect("one pad");
        // B on the settings screen: the bridge sends Escape, which is how
        // that screen goes back.
        app.world_mut()
            .get_mut::<Gamepad>(pad)
            .expect("the pad")
            .digital_mut()
            .press(GamepadButton::East);
        app.update();
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(KeyCode::Escape),
            "B goes back from a sub-screen"
        );
        // The screen it asked for arrives, and only then does the finger
        // come off the button.
        app.insert_resource(State::new(Screen::Menu));
        app.world_mut()
            .get_mut::<Gamepad>(pad)
            .expect("the pad")
            .digital_mut()
            .release(GamepadButton::East);
        app.update();
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::Escape),
            "the synthesized Escape must not be left held on the menu"
        );
    }

    /// The same when the screen opened is one the bridge does not run on:
    /// South on the stage list opens a puzzle in setup, and the Enter it
    /// left held there swallowed the next press of Enter on the won card.
    #[test]
    fn a_press_that_leaves_the_bridge_behind_still_releases_its_key() {
        let mut app = bridged(Screen::StageSelect);
        press(&mut app, GamepadButton::South);
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(KeyCode::Enter)
        );
        app.insert_resource(State::new(Screen::Puzzle));
        let pad = app
            .world_mut()
            .query_filtered::<Entity, With<Gamepad>>()
            .single(app.world())
            .expect("one pad");
        app.world_mut()
            .get_mut::<Gamepad>(pad)
            .expect("the pad")
            .digital_mut()
            .release(GamepadButton::South);
        app.update();
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::Enter),
            "let go of on a screen the bridge does not run on"
        );
    }

    /// A pad unplugged with a button down never reports the release, and
    /// the key the bridge held for it is let go all the same.
    #[test]
    fn an_unplugged_pad_lets_go_of_its_key() {
        let mut app = bridged(Screen::StageSelect);
        press(&mut app, GamepadButton::South);
        let pad = app
            .world_mut()
            .query_filtered::<Entity, With<Gamepad>>()
            .single(app.world())
            .expect("one pad");
        app.world_mut().entity_mut(pad).remove::<Gamepad>();
        app.update();
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::Enter)
        );
    }

    /// The menu keeps every other button: the exception is East alone,
    /// not the bridge.
    #[test]
    fn the_menu_keeps_the_rest_of_the_bridge() {
        for (button, key) in [
            (GamepadButton::DPadUp, KeyCode::KeyW),
            (GamepadButton::DPadDown, KeyCode::KeyS),
            (GamepadButton::DPadLeft, KeyCode::KeyA),
            (GamepadButton::DPadRight, KeyCode::KeyD),
            (GamepadButton::South, KeyCode::Enter),
        ] {
            let mut app = bridged(Screen::Menu);
            press(&mut app, button);
            assert!(
                app.world().resource::<ButtonInput<KeyCode>>().pressed(key),
                "{button:?} still drives the menu"
            );
        }
    }

    /// In the lobby the d-pad is the arrows, which walk the beach list,
    /// and never W, which there turns a player into a spectator.
    #[test]
    fn the_lobby_dpad_is_the_arrows() {
        for (button, key) in [
            (GamepadButton::DPadUp, KeyCode::ArrowUp),
            (GamepadButton::DPadDown, KeyCode::ArrowDown),
            (GamepadButton::DPadLeft, KeyCode::ArrowLeft),
            (GamepadButton::DPadRight, KeyCode::ArrowRight),
            (GamepadButton::South, KeyCode::Enter),
            (GamepadButton::East, KeyCode::Escape),
        ] {
            let mut app = bridged(Screen::Lobby);
            press(&mut app, button);
            let keys = app.world().resource::<ButtonInput<KeyCode>>();
            assert!(keys.pressed(key), "{button:?} is {key:?} in the lobby");
            assert!(
                !keys.pressed(KeyCode::KeyW),
                "{button:?} must not toggle watching"
            );
        }
    }

    /// Up pressed on the menu and let go in the lobby lets go of the
    /// menu's W, rather than of the lobby's arrow it never pressed.
    #[test]
    fn a_dpad_press_carried_into_the_lobby_is_released() {
        let mut app = bridged(Screen::Menu);
        press(&mut app, GamepadButton::DPadUp);
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW)
        );
        app.insert_resource(State::new(Screen::Lobby));
        let pad = app
            .world_mut()
            .query_filtered::<Entity, With<Gamepad>>()
            .single(app.world())
            .expect("one pad");
        app.world_mut()
            .get_mut::<Gamepad>(pad)
            .expect("the pad")
            .digital_mut()
            .release(GamepadButton::DPadUp);
        app.update();
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW),
            "the W pressed on the menu must not be left held in the lobby"
        );
    }
}
