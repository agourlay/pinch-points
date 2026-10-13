//! The pad as a keyboard on the menus: d-pad, South and East pressed
//! straight into the keyboard input resource, and let go of again.

use crate::app::Screen;
use bevy::prelude::*;

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

#[cfg(test)]
mod tests {
    use super::*;

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
