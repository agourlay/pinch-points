//! The doorway card: the one line each bot needs, a way to copy it, and
//! what has connected so far.

use super::{BotSeats, DoorFor};
use crate::app::i18n::fill;
use crate::sim::MAX_PLAYERS;
use bevy::prelude::*;

/// The doorway card: the one line each bot needs, a way to copy it, and
/// what has connected so far.
#[derive(Component)]
pub struct DoorCard;

/// One line of it, by its place on the card.
#[derive(Component)]
pub struct DoorLine(usize);

/// Lines the card holds: a heading, the instruction, a chair a seat, the
/// feedback, the keys.
const DOOR_LINES: usize = 3 + MAX_PLAYERS + 1;

/// What the card says, line by line, as it stands. Its last line names
/// the keys, or for a pad's player the card's own pad table.
fn door_lines(
    bots: &BotSeats,
    tr: &crate::app::i18n::Tr,
    beach: &str,
    device: crate::app::gamepad::LastDevice,
) -> Vec<(String, Color)> {
    let mut lines = vec![(String::new(), Color::NONE); DOOR_LINES];
    let Some(door) = &bots.door else {
        return lines;
    };
    let title = match bots.door_for {
        DoorFor::Couch => tr.door_title_table.to_string(),
        DoorFor::Join(_) => fill(tr.door_title_join, &[("b", beach)]),
        DoorFor::Nothing | DoorFor::Joined(_) => return lines,
    };
    lines[0] = (title, crate::app::palette::GOLD);
    lines[1] = (
        tr.door_start_with.to_string(),
        crate::app::palette::PARCHMENT,
    );
    for (i, chair) in door.chairs.iter().take(MAX_PLAYERS).enumerate() {
        let who = crate::app::seat_label(tr, chair.seat);
        let line = match door.bot_line(chair) {
            Some(super::BotLine::Here { name, rtt }) => {
                let ms = rtt.map_or("-".to_string(), |d| {
                    format!("{:.1}", d.as_secs_f64() * 1000.0)
                });
                format!(
                    "{who}  {}",
                    fill(tr.door_connected, &[("n", &name), ("ms", &ms)])
                )
            }
            Some(super::BotLine::Away { name }) => {
                format!("{who}  {}", fill(tr.door_away, &[("n", &name)]))
            }
            None => format!("{who}  {}   {}", door.string(chair), tr.door_waiting),
        };
        let ink = match chair.bot {
            Some(_) => crate::app::palette::seat_text(chair.seat),
            None => Color::WHITE,
        };
        lines[2 + i] = (line, ink);
    }
    lines[2 + MAX_PLAYERS] = (
        bots.feedback.clone(),
        crate::app::palette::PARCHMENT.with_alpha(0.8),
    );
    let keys = match (device, door.showing_lan) {
        (crate::app::gamepad::LastDevice::Pad, lan) => {
            crate::app::gamepad::scheme::legend(tr, &crate::app::gamepad::Face::Door { lan })
        }
        (crate::app::gamepad::LastDevice::Keyboard, true) => tr.door_prompt_lan.to_string(),
        (crate::app::gamepad::LastDevice::Keyboard, false) => tr.door_prompt.to_string(),
    };
    lines[DOOR_LINES - 1] = (keys, crate::app::palette::PARCHMENT.with_alpha(0.7));
    lines
}

/// Put the card up while a doorway is waiting, keep its lines current,
/// and take it down when the wait is over.
pub fn door_card(
    mut commands: Commands,
    bots: Res<BotSeats>,
    settings: Res<crate::app::settings::GameSettings>,
    lobby: Res<crate::app::lobby::LobbyState>,
    device: Res<crate::app::gamepad::LastDevice>,
    cards: Query<Entity, With<DoorCard>>,
    mut lines: Query<(&DoorLine, &mut Text, &mut TextColor, &mut Node)>,
) {
    let up = bots.door_for.card_up() && bots.door.is_some();
    if !up {
        for card in &cards {
            commands.entity(card).despawn();
        }
        return;
    }
    if cards.is_empty() {
        commands
            .spawn((
                DoorCard,
                crate::app::menu_ui::centred_overlay(),
                BackgroundColor(Color::BLACK.with_alpha(0.55)),
                GlobalZIndex(40),
            ))
            .with_children(|wrap| {
                wrap.spawn(crate::app::menu_ui::screen_card())
                    .with_children(|card| {
                        for i in 0..DOOR_LINES {
                            card.spawn((
                                DoorLine(i),
                                Text::new(""),
                                if i == 0 {
                                    crate::app::menu_ui::display_font(
                                        crate::app::menu_ui::type_scale::DISPLAY * 0.7,
                                    )
                                } else {
                                    TextFont {
                                        font_size: FontSize::Px(
                                            crate::app::menu_ui::type_scale::BODY,
                                        ),
                                        ..default()
                                    }
                                },
                                TextColor(Color::WHITE),
                                Node {
                                    align_self: AlignSelf::FlexStart,
                                    ..default()
                                },
                            ));
                        }
                    });
            });
        return;
    }
    let beach = lobby.joining_beach().unwrap_or_default();
    let said = door_lines(&bots, settings.tr(), &beach, *device);
    for (line, mut text, mut ink, mut node) in &mut lines {
        let (words, color) = &said[line.0];
        crate::app::menu_ui::set_text(&mut text, words);
        crate::app::menu_ui::set_color(&mut ink, *color);
        crate::app::menu_ui::set_shown(&mut node, !words.is_empty());
    }
}

/// The card's keys: C copies the first string still waiting for its bot,
/// L shows the LAN's strings or this machine's, Esc backs out and gives
/// the keys up.
pub fn door_input(
    keyboard: crate::app::keycaps::Keyboard,
    mut bots: ResMut<BotSeats>,
    mut clipboard: ResMut<Clipboard>,
    settings: Res<crate::app::settings::GameSettings>,
) {
    if !bots.door_for.card_up() {
        return;
    }
    let tr = settings.tr();
    if keyboard.keys.just_pressed(KeyCode::Escape) {
        bots.door_for = DoorFor::Nothing;
        bots.feedback.clear();
        bots.door = None;
        return;
    }
    let BotSeats {
        door,
        door_for,
        feedback,
        ..
    } = &mut *bots;
    let Some(door) = door else {
        *door_for = DoorFor::Nothing;
        return;
    };
    door.poll();
    if keyboard.global('C')
        && let Some(chair) = door.chairs.iter().find(|chair| chair.bot.is_none())
    {
        let string = door.string(chair);
        *feedback = match clipboard.set_text(string.clone()) {
            Ok(()) => fill(tr.door_copied, &[("s", &string)]),
            Err(e) => fill(tr.code_copy_failed, &[("e", &e.to_string())]),
        };
    }
    if keyboard.just('L')
        && let Err(e) = door.toggle_lan()
    {
        *feedback = fill(tr.door_could_not_listen, &[("e", &e.to_string())]);
    }
}
