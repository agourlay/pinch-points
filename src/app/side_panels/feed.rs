//! The round's event feed: the rolling list of raids, gold, lures, tier-ups
//! and tide events down the right sidebar.
//!
//! Repeats collapse into one counted line, which is why this has state
//! worth testing.

use super::{LOG_TOP, card};
use crate::app::i18n::fill;
use crate::app::palette;
use crate::app::settings::GameSettings;
use crate::app::sim_events::SimEvent;
use crate::sim::CrabKind;
use bevy::prelude::*;
use std::collections::VecDeque;

/// One line of the event log (0 = newest, at the top).
#[derive(Component)]
pub struct LogLine(pub usize);

/// One line of the feed. Identical lines collapse into a single entry
/// carrying a repeat counter instead of scrolling the interesting ones off
/// the top ("a gull swoops in ×9").
pub struct LogEntry {
    pub text: String,
    pub color: Color,
    pub count: u32,
}

impl LogEntry {
    /// What the line reads as, counter included once it has repeated.
    pub fn rendered(&self) -> String {
        if self.count > 1 {
            format!("{} ×{}", self.text, self.count)
        } else {
            self.text.clone()
        }
    }
}

/// The rolling event log for the current round: newest first.
#[derive(Resource, Default)]
pub struct EventLog(pub VecDeque<LogEntry>);

impl EventLog {
    /// Add a line. A repeat of something already on the feed bumps that
    /// entry's counter and floats it back to the top rather than pushing a
    /// duplicate: nine gulls are one line, not nine.
    pub fn push(&mut self, text: String, color: Color) {
        if let Some(index) = self.0.iter().position(|entry| entry.text == text) {
            let mut entry = self.0.remove(index).expect("index came from the deque");
            entry.count += 1;
            self.0.push_front(entry);
            return;
        }
        self.0.push_front(LogEntry {
            text,
            color,
            count: 1,
        });
        self.0.truncate(LOG_LINES);
    }
}

const LOG_LINES: usize = 9;

/// Fold notable sim events into the log: raids, gold, lures, tier-ups,
/// tide events, gull arrivals, and the surge. Common banks are too chatty
/// to list.
pub fn collect_log(
    mut events: MessageReader<SimEvent>,
    settings: Res<GameSettings>,
    names: Res<crate::app::SeatNames>,
    mut log: ResMut<EventLog>,
) {
    let tr = settings.tr();
    let seat_label = |seat: u8| names.label(tr, seat);
    for event in events.read() {
        let line = match event {
            SimEvent::CastleRaided { owner, lost, .. } => Some((
                fill(
                    tr.log_raid,
                    &[("p", &seat_label(*owner)), ("n", &lost.to_string())],
                ),
                palette::INK_RAID,
            )),
            SimEvent::CrabBanked {
                owner,
                value,
                kind: CrabKind::Golden,
                ..
            } => Some((
                fill(
                    tr.log_golden,
                    &[("p", &seat_label(*owner)), ("n", &value.to_string())],
                ),
                palette::GOLD,
            )),
            SimEvent::CrabBanked {
                owner,
                kind: CrabKind::Molting,
                ..
            } => Some((
                fill(tr.log_lure, &[("p", &seat_label(*owner))]),
                palette::INK_LURE,
            )),
            SimEvent::TierUp { owner } => Some((
                fill(tr.log_tier, &[("p", &seat_label(*owner))]),
                palette::player_color(*owner).lighter(0.15),
            )),
            SimEvent::TideEventFired { event } => Some((
                crate::app::hud::event_name(tr, *event).to_string(),
                palette::INK_TIDE,
            )),
            SimEvent::GullArrived => {
                Some((tr.log_gull.to_string(), Color::srgba(0.85, 0.88, 0.92, 0.9)))
            }
            SimEvent::SurgeStarted => Some((tr.log_surge.to_string(), palette::INK_SURGE)),
            SimEvent::CrabBanked { .. }
            | SimEvent::CrabEaten { .. }
            | SimEvent::CrabSpawned { .. }
            | SimEvent::GullTookOff
            | SimEvent::GullLanded { .. }
            | SimEvent::SignpostsChanged { .. }
            | SimEvent::RoundEnded => None,
        };
        if let Some((text, color)) = line {
            log.push(text, color);
        }
    }
}

/// Render the log lines, newest at the top, older ones dimmer.
pub fn update_log(log: Res<EventLog>, mut lines: Query<(&LogLine, &mut Text, &mut TextColor)>) {
    for (line, mut text, mut color) in &mut lines {
        let (value, target) = match log.0.get(line.0) {
            Some(entry) => {
                let fade = 1.0 - line.0 as f32 * 0.09;
                (entry.rendered(), entry.color.with_alpha(fade))
            }
            None => (String::new(), Color::NONE),
        };
        crate::app::menu_ui::set_text(&mut text, &value);
        crate::app::menu_ui::set_color(&mut color, target);
    }
}

/// Spawn the feed card, filling the sidebar below the clock.
pub(super) fn spawn_feed(root: &mut ChildSpawnerCommands) {
    // Its rows stack from the top, so it drops the clock card's centring.
    let (mut node, edge, _) = card(LOG_TOP, None);
    node.flex_direction = FlexDirection::Column;
    node.align_items = AlignItems::default();
    node.justify_content = JustifyContent::default();
    node.row_gap = Val::Px(6.0);
    node.padding = UiRect::axes(Val::Px(12.0), Val::Px(10.0));
    node.overflow = Overflow::clip();
    root.spawn((
        node,
        edge,
        BackgroundColor(palette::CARD_BG.with_alpha(0.9)),
    ))
    .with_children(|list| {
        for index in 0..LOG_LINES {
            list.spawn((
                LogLine(index),
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(Color::NONE),
            ));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Repeats collapse into one counted line instead of flooding the feed,
    /// and the counted line floats back to the top when it happens again.
    #[test]
    fn repeated_lines_merge_with_a_counter() {
        let mut log = EventLog::default();
        for _ in 0..9 {
            log.push("a gull swoops in".into(), Color::WHITE);
        }
        assert_eq!(log.0.len(), 1);
        assert_eq!(log.0[0].rendered(), "a gull swoops in ×9");

        log.push("P1 raided! -4".into(), Color::WHITE);
        assert_eq!(log.0[0].rendered(), "P1 raided! -4");
        log.push("a gull swoops in".into(), Color::WHITE);
        assert_eq!(log.0.len(), 2, "the gull line was bumped, not duplicated");
        assert_eq!(log.0[0].rendered(), "a gull swoops in ×10");
    }

    /// Distinct lines still scroll, and the feed never grows past its slots.
    #[test]
    fn distinct_lines_scroll_and_stay_bounded() {
        let mut log = EventLog::default();
        for n in 0..LOG_LINES + 5 {
            log.push(format!("line {n}"), Color::WHITE);
        }
        assert_eq!(log.0.len(), LOG_LINES);
        assert_eq!(log.0[0].rendered(), format!("line {}", LOG_LINES + 4));
    }
}
