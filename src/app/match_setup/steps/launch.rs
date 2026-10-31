//! Starting the match: seating the table and setting the series going.

use super::*;

/// What starting the match sets going: the series, the screen, and the
/// doorway the table's bots come in by.
#[derive(bevy::ecs::system::SystemParam)]
pub struct MatchStart<'w> {
    tournament: ResMut<'w, crate::app::tournament::Tournament>,
    pub(super) next_screen: ResMut<'w, NextState<Screen>>,
    pub(super) bots: ResMut<'w, crate::app::bot_seats::BotSeats>,
}

impl MatchStart<'_> {
    /// Put the match on: armed, the series begun, the arena opened.
    fn launch(&mut self, config: &mut MatchConfig) {
        config.armed = true;
        *self.tournament = if config.series.is_series() {
            crate::app::tournament::Tournament::start(config.series)
        } else {
            crate::app::tournament::Tournament::default()
        };
        self.next_screen.set(Screen::Versus);
    }

    /// Whether the table's bots are all in, opening the doorway for any
    /// that are not. A table with no bot seat is ready at once.
    fn bots_in(&mut self, config: &MatchConfig, tr: &crate::app::i18n::Tr) -> bool {
        use crate::app::bot_seats::{DoorFor, Doorway};
        let seats = bot_seat_list(config);
        if seats.is_empty() {
            self.bots.door = None;
            return true;
        }
        if self.bots.door.is_none() {
            match Doorway::open() {
                Ok(door) => self.bots.door = Some(door),
                Err(e) => {
                    self.bots.feedback = fill(tr.door_could_not_listen, &[("e", &e.to_string())]);
                    return false;
                }
            }
        }
        let Some(door) = &mut self.bots.door else {
            return false;
        };
        door.seat_bots(&seats);
        door.poll();
        if door.ready() {
            return true;
        }
        self.bots.door_for = DoorFor::Couch;
        false
    }
}

/// Make the table the match is played at, as Start match does: every open
/// seat to the AI at step 1's level for this match (marked, so match
/// setup opens with it open again: [`open_the_seats_filled_at_start`]),
/// and the map the match is played on, drawn now under the Random card,
/// else on the seed its card was drawn with.
pub fn seat_the_match(
    config: &mut MatchConfig,
    open: &[u8],
    preview_seed: u64,
    beaches: &CustomBeaches,
    roll: u64,
) {
    for &seat in open {
        config.controllers[usize::from(seat)] = SeatController::Ai(config.ai_level);
        config.filled_at_start[usize::from(seat)] = true;
    }
    match config.pick {
        Pick::Random => {
            draw_map(config, beaches, roll);
            config.seed = None;
        }
        Pick::InForce | Pick::Moved(_) => config.seed = Some(preview_seed),
    }
}

/// The pads plugged in, and who they seat.
#[derive(bevy::ecs::system::SystemParam)]
pub struct SetupPads<'w, 's> {
    pub seats: ResMut<'w, PadSeats>,
    pub live: Query<'w, 's, Entity, With<Gamepad>>,
    /// The keys pads hold down, which are no keyboard player's.
    pub bridged: Res<'w, Bridged>,
}

impl SetupPads<'_, '_> {
    /// The pads seated at `config`'s person's seats, by the seat rule.
    pub fn seated(&self, config: &MatchConfig, settings: &GameSettings) -> Vec<SeatPad> {
        let live: Vec<Entity> = self.live.iter().collect();
        self.seats.seat_pads(&live, settings, &config.human_seats())
    }
}

/// Start match: once the bots are in, the table launched
/// ([`launch_the_table`]). Bots first: a match never starts with a seat
/// nothing is driving. Nothing is set for the match until it does start:
/// a doorway card backed out of leaves the open seats open and the Random
/// card undrawn, as step 3 showed them.
pub(super) fn start_match(
    config: &mut MatchConfig,
    menu: &MatchMenu,
    start: &mut MatchStart,
    pads: &mut SetupPads,
    settings: &GameSettings,
    beaches: &CustomBeaches,
) {
    if start.bots_in(config, settings.tr()) {
        launch_the_table(config, menu, start, pads, settings, beaches);
    }
}

/// The match goes on: the open seats to the AI, the map drawn or the
/// previewed seed kept ([`seat_the_match`]), and the round. Where Start
/// match lands with the bots in, and where the doorway card does once the
/// last one comes in.
pub(super) fn launch_the_table(
    config: &mut MatchConfig,
    menu: &MatchMenu,
    start: &mut MatchStart,
    pads: &mut SetupPads,
    settings: &GameSettings,
    beaches: &CustomBeaches,
) {
    let open = open_seats(
        config,
        &pads.seated(config, settings),
        pads.seats.p2_at_keys(),
        settings,
    );
    seat_the_match(
        config,
        &open,
        menu.seed,
        beaches,
        crate::app::clock::fresh_seed(),
    );
    if !open.is_empty() {
        pads.seats.table_set_by_hand();
    }
    start.launch(config);
}
