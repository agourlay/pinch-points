//! The sidebar's big tide clock: the same digits and colour ramp as the
//! puzzle clock up top, in a card of its own.

use super::card;
use crate::app::Sim;
use crate::app::settings::GameSettings;
use bevy::prelude::*;

/// The sidebar's big round clock.
#[derive(Component)]
pub struct SideClock;

const CLOCK_TOP: f32 = 10.0;

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
                    font_size: FontSize::Px(44.0),
                    ..default()
                },
                TextColor(crate::app::palette::SIDE_CLOCK),
            ));
        });
}
