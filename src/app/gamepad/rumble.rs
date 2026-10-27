//! A raid felt in the raided player's hands.

use super::PadSeats;
use crate::app::cursor::Cursor;
use crate::app::settings::GameSettings;
use bevy::input::gamepad::{GamepadRumbleIntensity, GamepadRumbleRequest};
use bevy::prelude::*;

/// How hard, and for how long, a raid is felt.
///
/// Both motors, weighted to the low-frequency one: a gull carrying off
/// a third of a castle is a thump, not a buzz. Short enough that two raids in
/// quick succession read as two knocks rather than one long shudder.
const RAID_RUMBLE: GamepadRumbleIntensity = GamepadRumbleIntensity {
    strong_motor: 0.85,
    weak_motor: 0.45,
};
const RAID_RUMBLE_SECS: f32 = 0.32;

/// A gull on your castle, in your hands.
///
/// Only the seat that was raided, and only if that seat is sitting at
/// *this* machine holding a pad. [`PadSeats::pad_entity_for`] answers over the live
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
        // `PadSeats::pad_entity_for` answers for any seat that named a controller
        // outright, without consulting the cursor set. So "is this seat
        // here" is asked first, or a player who bound P2 to a pad for couch
        // play feels every raid on seat two: online, where seat two is a
        // stranger, and against AI, where it is a bot.
        if seated.binary_search(owner).is_err() {
            continue;
        }
        let Some(pad) = claims.pad_entity_for(&pads, &settings, &seated, *owner) else {
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
    use super::super::keep_pad_order;
    use super::*;
    use crate::app::settings::SeatInput;
    use crate::app::sim_events::SimEvent;
    use crate::app::{Screen, Sim};
    use crate::sim::classic_arena;

    /// A world with `seats` cursors and `pads` controllers plugged in.
    /// No window, no art: this checks who gets buzzed, not what it feels
    /// like.
    fn table(seats: u8, pads: usize) -> App {
        let mut app = App::new();
        app.insert_resource(Sim(classic_arena(false, seats.max(2))));
        app.insert_resource(GameSettings::default());
        app.init_resource::<PadSeats>();
        // What a Start on the setup screen claims a seat at.
        app.init_resource::<crate::app::match_setup::MatchConfig>();
        app.init_resource::<crate::app::match_setup::CustomBeaches>();
        app.init_resource::<crate::app::bot_seats::BotSeats>();
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

    /// `pad` presses Start on the setup screen, which claims it a seat
    /// there by the system that reads the press.
    fn sit_down(app: &mut App, pad: Entity) {
        use bevy::ecs::system::RunSystemOnce;
        app.world_mut()
            .get_resource_or_init::<crate::app::match_setup::MatchMenu>();
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("pad");
        gamepad.digital_mut().press(GamepadButton::Start);
        app.world_mut()
            .run_system_once(super::super::pad_claim_seats)
            .expect("runs");
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("pad");
        gamepad.digital_mut().release(GamepadButton::Start);
        gamepad.digital_mut().clear();
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
            by: None,
        });
        app.update();
        buzzed(app)
    }

    /// The pad on the raided seat's own hands, and nobody else's. Pads
    /// fill seats from P2 upwards, so with two seats and one pad the
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
        app.update();
        sit_down(&mut app, plugged[0]);
        assert_eq!(raid(&mut app, 1), vec![plugged[0]], "the claim, on P2");

        app.world_mut().entity_mut(plugged[0]).remove::<Gamepad>();
        let spare = app.world_mut().spawn(Gamepad::default()).id();
        app.update();
        assert_eq!(raid(&mut app, 1), vec![spare], "the spare takes P2");
        assert_eq!(
            raid(&mut app, 0),
            vec![plugged[1]],
            "P1 keeps the pad it was dealt: the friend's Start needed its own"
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

    /// Once the join ceremony has claimed a pad, the claim decides which
    /// controller a seat means, not the order the operating system handed
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
        assert_eq!(raid(&mut app, 1), vec![plugged[0]], "unclaimed, plug order");
        sit_down(&mut app, plugged[1]);
        assert_eq!(
            raid(&mut app, 1),
            vec![plugged[1]],
            "P2 holds the pad that claimed it"
        );
        assert_eq!(
            raid(&mut app, 0),
            vec![plugged[0]],
            "and the other is dealt P1, the seat left over"
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
}
