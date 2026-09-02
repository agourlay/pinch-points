//! The sidebar's big tide clock: the same reading and the same colour
//! ramp as the clock up top, in a card of its own and at its own size.
//!
//! Both go through `hud::clock_into` and `hud::clock_color`, so they say
//! the same thing and turn red together. What they do not share is the
//! type: this one is the headline of a narrow column with nothing else in
//! its card, where the header's has a whole strip to sit in. The doc used
//! to claim "the same digits", which read as the same size and was not.

use super::card;
use crate::app::Sim;
use crate::app::settings::GameSettings;
use bevy::prelude::*;

/// The sidebar's big round clock.
#[derive(Component)]
pub struct SideClock;

const CLOCK_TOP: f32 = 10.0;

/// Bigger than [`crate::app::menu_ui::type_scale::DISPLAY`], which the
/// header's clock uses, and off the scale on purpose: it is the only
/// thing in its card and the one number a player checks from across a
/// room. The card is 64 tall and this is what fills it.
const CLOCK_PX: f32 = 44.0;

/// Drive the sidebar clock: mm:ss, red for the closing stretch of the
/// round and pulsing for the last of it. Versus rounds are two minutes and
/// up, so that stretch is the flat 30 s it has always been.
pub fn update_side_clock(
    sim: Res<Sim>,
    time: Res<Time>,
    settings: Res<GameSettings>,
    mut clocks: Query<(&mut Text, &mut TextColor), With<SideClock>>,
    mut value: Local<String>,
) {
    let Some(ticks) = sim.0.remaining_ticks() else {
        return;
    };
    crate::app::hud::clock_into(&mut value, ticks);
    let target = crate::app::hud::clock_color(
        ticks,
        sim.0.round_length(),
        time.elapsed_secs(),
        !settings.reduced_motion,
    );
    for (mut text, mut color) in &mut clocks {
        crate::app::menu_ui::set_text(&mut text, &value);
        crate::app::menu_ui::set_color(&mut color, target);
    }
}

/// Spawn the clock card at the top of the right sidebar.
pub(super) fn spawn_clock(root: &mut ChildSpawnerCommands) {
    root.spawn(card(CLOCK_TOP, Some(64.0)))
        .with_children(|card| {
            card.spawn((
                SideClock,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(CLOCK_PX),
                    ..default()
                },
                // The calm colour, which is what `update_side_clock` will
                // write on the first frame anyway. It used to spawn in a
                // `SIDE_CLOCK` of its own - a second calm ink, a percent
                // away from this one, that no frame ever drew.
                TextColor(crate::app::palette::CLOCK_CALM),
            ));
        });
}
