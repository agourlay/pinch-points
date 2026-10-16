//! The sidebar's big tide clock: the same reading and the same colour
//! ramp as the clock up top, in a card of its own and at its own size.
//!
//! Both go through `hud::clock_into` and `hud::clock_color`, so they say
//! the same thing and turn red together. What they do not share is the
//! type: this one is the headline of a narrow column with nothing else in
//! its card, where the header's has a whole strip to sit in.

use super::card;
use crate::app::Sim;
use bevy::prelude::*;

/// The sidebar's big round clock.
#[derive(Component)]
pub struct SideClock;

pub(super) const CLOCK_TOP: f32 = 10.0;

/// Bigger than [`crate::app::menu_ui::type_scale::DISPLAY`], which the
/// header's clock uses, and off the scale on purpose: it is the only
/// thing in its card and the one number a player checks from across a
/// room. The card holds 64 of digits inside its frame, and this is what
/// fills them.
const CLOCK_PX: f32 = 44.0;

/// Drive the sidebar clock: mm:ss, red for the closing stretch of the
/// round and pulsing for the last of it. Versus rounds are two minutes and
/// up, so that stretch is the flat 30 s it has always been.
pub fn update_side_clock(
    sim: Res<Sim>,
    blink: crate::app::hud::Blink,
    mut clocks: Query<(&mut Text, &mut TextColor), With<SideClock>>,
    mut value: Local<String>,
) {
    let Some(ticks) = sim.0.remaining_ticks() else {
        return;
    };
    crate::app::hud::clock_into(&mut value, ticks);
    let (elapsed, blinking) = blink.read(&sim.0);
    let target = crate::app::hud::clock_color(ticks, sim.0.rules().round_length, elapsed, blinking);
    for (mut text, mut color) in &mut clocks {
        crate::app::menu_ui::set_text(&mut text, &value);
        crate::app::menu_ui::set_color(&mut color, target);
    }
}

/// How tall the clock card is: the digits' own 64 and the driftwood frame
/// round them.
pub(super) const CLOCK_H: f32 = 64.0 + 2.0 * crate::app::menu_ui::FRAME_PX;

/// Spawn the clock card at the top of the right sidebar, in the menus'
/// driftwood frame.
pub(super) fn spawn_clock(root: &mut ChildSpawnerCommands) {
    let (mut node, edge, fill, shadow) = card(CLOCK_TOP, Some(CLOCK_H));
    // The frame holds a card's contents clear of the wood at the top; the
    // same at the foot keeps the digits centred between the two.
    node.padding.bottom = Val::Px(crate::app::menu_ui::CARD_PAD);
    root.spawn((node, edge, fill, shadow, crate::app::menu_ui::Framed))
        .with_children(|card| {
            card.spawn((
                SideClock,
                Text::new(""),
                crate::app::menu_ui::display_font(CLOCK_PX),
                // The calm colour, which is what `update_side_clock` writes
                // on the first frame anyway.
                TextColor(crate::app::palette::CLOCK_CALM),
            ));
        });
}
