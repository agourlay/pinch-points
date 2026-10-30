//! The round's event feed: the rolling list of raids, gold, lures, tier-ups
//! and tide events down the right sidebar.
//!
//! Repeats collapse into one counted line, which is why this has state
//! worth testing.

use super::{CARD_BORDER, LOG_TOP, card};
use crate::app::i18n::fill;
use crate::app::menu_ui;
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
    /// What the line reads as, counter included once it has repeated,
    /// written into a buffer the caller keeps: nine lines repaint every
    /// frame and change only when something happens.
    pub fn render_into(&self, out: &mut String) {
        use std::fmt::Write;
        out.clear();
        out.push_str(&self.text);
        if self.count > 1 {
            let _ = write!(out, " ×{}", self.count);
        }
    }

    /// What the line reads as, counter included once it has repeated.
    #[cfg(test)]
    pub fn rendered(&self) -> String {
        let mut out = String::new();
        self.render_into(&mut out);
        out
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

/// How many entries the feed keeps: more than the shortest card shows, so
/// a card reaching down beside a tall board has lines to fill it with.
const LOG_LINES: usize = 16;

/// Fold what the spectators said into the same feed the round's own events use.
///
/// Drained from the session rather than read: a line is shown once and
/// then belongs to the feed, which keeps its own history and its own
/// nine-line window.
///
/// Idle ink, not a seat colour. The voice has no castle on the board, and
/// a line in a player's colour reads as that player having said it.
///
/// The room's own notices are worded here, in this machine's language,
/// from what the host said happened ([`crate::app::net::notice_line`]).
pub fn collect_chat(
    settings: Res<GameSettings>,
    mut online: ResMut<crate::app::net::Online>,
    mut log: ResMut<EventLog>,
) {
    let Some(session) = &mut online.0 else {
        return;
    };
    for (who, line) in session.heard.drain(..) {
        // A nameless speaker is the beach itself, which is how the lobby
        // already spells a notice that came from nobody.
        let said = match who.is_empty() {
            true => line,
            false => format!("{who}: {line}"),
        };
        log.push(said, palette::IDLE_ROW);
    }
    let tr = settings.tr();
    let names = &session.names;
    for notice in session.notices.drain(..) {
        let label = |seat: u8| crate::app::name_or_label(names, tr, seat);
        log.push(
            crate::app::net::notice_line(tr, &notice, label),
            palette::IDLE_ROW,
        );
    }
}

/// The feed's line for a raid on `owner` for `lost`: who steered the gull
/// when an arrow did, said plainly when it was the victim's own, and the
/// bare raid for a wild gull.
pub fn raid_line(
    tr: &crate::app::i18n::Tr,
    owner: u8,
    lost: u32,
    by: Option<u8>,
    seat_label: &impl Fn(u8) -> String,
) -> String {
    let n = lost.to_string();
    let victim = seat_label(owner);
    match by {
        None => fill(tr.log_raid, &[("p", &victim), ("n", &n)]),
        Some(by) if by == owner => fill(tr.log_raid_own, &[("p", &victim), ("n", &n)]),
        Some(by) => fill(
            tr.log_raid_by,
            &[("a", &seat_label(by)), ("p", &victim), ("n", &n)],
        ),
    }
}

/// The feed's line for a tide event and its ink: who started it, in their
/// seat's colour, when a sparkling crab spun it, and the bare name in the
/// tide's ink for one called down, which nobody at the table spun.
pub fn event_line(
    tr: &crate::app::i18n::Tr,
    event: crate::sim::TideEvent,
    by: Option<u8>,
    seat_label: &impl Fn(u8) -> String,
) -> (String, Color) {
    let name = crate::app::hud::event_name(tr, event);
    match by {
        Some(seat) => (
            fill(tr.log_event_by, &[("p", &seat_label(seat)), ("e", name)]),
            palette::seat_text(seat),
        ),
        None => (name.to_string(), palette::INK_TIDE),
    }
}

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
            SimEvent::CastleRaided {
                owner, lost, by, ..
            } => Some((
                raid_line(tr, *owner, *lost, *by, &seat_label),
                palette::INK_RAID,
            )),
            SimEvent::CrabBanked {
                owner,
                points,
                kind: CrabKind::Golden,
                ..
            } => Some((
                fill(
                    tr.log_golden,
                    &[("p", &seat_label(*owner)), ("n", &format!("{points:+}"))],
                ),
                palette::GOLD,
            )),
            SimEvent::LureStarted { owner } => Some((
                fill(tr.log_lure, &[("p", &seat_label(*owner))]),
                palette::INK_LURE,
            )),
            SimEvent::TierUp { owner } => Some((
                fill(tr.log_tier, &[("p", &seat_label(*owner))]),
                palette::seat_text(*owner),
            )),
            SimEvent::TideEventFired { event, by } => {
                Some(event_line(tr, *event, *by, &seat_label))
            }
            SimEvent::WheelLocked { owner } => Some((
                fill(tr.log_wheel_locked, &[("p", &seat_label(*owner))]),
                palette::INK_TIDE,
            )),
            SimEvent::LureRefused { owner, running } => Some((
                fill(
                    if *running {
                        tr.log_lure_running
                    } else {
                        tr.log_lure_resting
                    },
                    &[("p", &seat_label(*owner))],
                ),
                palette::INK_LURE,
            )),
            // One line per gull, written when it lands: its shadow on the
            // sand is the notice, and a line for each half of one arrival
            // is two lines for one bird.
            SimEvent::GullIncoming { .. } => None,
            SimEvent::GullArrived => Some((tr.log_gull.to_string(), palette::GULL_INK)),
            SimEvent::SurgeStarted => Some((tr.log_surge.to_string(), palette::INK_SURGE)),
            SimEvent::GoldenCalled { .. } => Some((tr.log_golden_call.to_string(), palette::GOLD)),
            SimEvent::GoldenOut { .. } => Some((tr.log_golden_out.to_string(), palette::GOLD)),
            SimEvent::CrabBanked { .. }
            | SimEvent::CrabEaten { .. }
            | SimEvent::CrabSpawned { .. }
            | SimEvent::GullTookOff
            | SimEvent::GullLanded { .. }
            | SimEvent::GullShooed { .. }
            | SimEvent::GullFlewOff { .. }
            | SimEvent::SignpostPlaced { .. }
            | SimEvent::SignpostRemoved { .. }
            | SimEvent::SignpostEvicted { .. }
            | SimEvent::RoundEnded => None,
        };
        if let Some((text, color)) = line {
            log.push(text, color);
        }
    }
}

/// The box inside the feed card the lines stack in, and are clipped to.
#[derive(Component)]
pub struct LogLines;

/// How many of the lines, `heights` tall in order from the top with `gap`
/// between each, fit whole in `room`. A line that would run past the foot
/// is not drawn at all: half a line cut off by the frame read as a glitch.
fn lines_that_fit(heights: impl IntoIterator<Item = f32>, gap: f32, room: f32) -> usize {
    let mut used = 0.0;
    let mut fit = 0;
    for height in heights {
        let next = if fit == 0 {
            height
        } else {
            used + gap + height
        };
        if next > room + 0.5 {
            break;
        }
        used = next;
        fit += 1;
    }
    fit
}

/// Render the log lines, newest at the top, older ones dimmer, as many as
/// fit whole in the card. Measured from last frame's layout, which is the
/// one there is.
pub fn update_log(
    log: Res<EventLog>,
    mut lines: Query<(&LogLine, &mut Text, &mut TextColor, &ComputedNode)>,
    boxes: Query<&ComputedNode, With<LogLines>>,
    mut value: Local<String>,
    mut heights: Local<Vec<f32>>,
) {
    let shown = boxes.single().map_or(LOG_LINES, |room| {
        heights.clear();
        heights.resize(LOG_LINES, 0.0);
        for (line, _, _, node) in &lines {
            if let Some(height) = heights.get_mut(line.0) {
                *height = node.size.y;
            }
        }
        let gap = LOG_GAP / room.inverse_scale_factor;
        let filled = log.0.len().min(LOG_LINES);
        lines_that_fit(heights[..filled].iter().copied(), gap, room.size.y)
    });
    for (line, mut text, mut color, _) in &mut lines {
        let target = match log.0.get(line.0) {
            // A line that does not fit keeps its words, drawn clear: its
            // height is what says whether it fits, and an emptied line would
            // measure as fitting next frame and flicker back.
            Some(entry) => {
                let fade = (1.0 - line.0 as f32 * 0.09).max(LOG_FADE_FLOOR);
                entry.render_into(&mut value);
                if line.0 < shown {
                    entry.color.with_alpha(fade)
                } else {
                    Color::NONE
                }
            }
            None => {
                value.clear();
                Color::NONE
            }
        };
        menu_ui::set_text(&mut text, &value);
        menu_ui::set_color(&mut color, target);
    }
}

/// How many single-line entries the feed card is sized for. Longer ones
/// wrap and take two; what runs past the foot is clipped, and is the
/// oldest and dimmest anyway.
const LOG_ROWS: f32 = 8.0;

/// The gap between two entries.
const LOG_GAP: f32 = 6.0;

/// The dimmest an old entry is drawn: far down a tall card the fade would
/// otherwise run out before the lines do.
const LOG_FADE_FLOOR: f32 = 0.3;

/// The feed card's least height: its rows, the frame's room at the top and
/// the foot, and the card's own border at each, which Bevy counts inside
/// the height: without it the lines' box is 4 px short and the eighth entry
/// never fits.
const LOG_H: f32 = LOG_ROWS * menu_ui::type_scale::BODY * menu_ui::LINE_HEIGHT
    + (LOG_ROWS - 1.0) * LOG_GAP
    + 2.0 * menu_ui::CARD_PAD
    + 2.0 * CARD_BORDER;

/// The feed card, whose foot [`reach_the_board`] keeps level with the
/// board's.
#[derive(Component)]
pub struct FeedCard;

/// Spawn the feed card below the clock, in the menus' driftwood frame. Its
/// height follows the board, never what is written in it: a card that grew
/// with each new line drew the eye every time something happened.
pub(super) fn spawn_feed(root: &mut ChildSpawnerCommands) {
    let (mut node, edge, fill, shadow) = card(LOG_TOP, Some(LOG_H));
    // Its rows stack from the top, so it drops the clock card's centring.
    node.flex_direction = FlexDirection::Column;
    node.align_items = AlignItems::default();
    node.justify_content = JustifyContent::default();
    // The frame holds the top and the sides clear of its wood; the foot is
    // ours to keep clear, or the last line runs under the bottom band.
    node.padding = UiRect::axes(Val::Px(super::event_timer::FEED_PAD_X), Val::Px(10.0));
    node.padding.bottom = Val::Px(menu_ui::CARD_PAD);
    // The lines sit in a box of their own that does the clipping, inside
    // the padding: clipped by the card itself, a line running past the
    // foot showed half under the frame's bottom band, and clipping the
    // card to its padding would cut off its own frame, which is a child.
    let lines = Node {
        flex_direction: FlexDirection::Column,
        flex_grow: 1.0,
        min_height: Val::Px(0.0),
        row_gap: Val::Px(LOG_GAP),
        overflow: Overflow::clip(),
        ..default()
    };
    // The card's own fill, kept rather than thinned: at 0.9 the sand behind
    // the sidebar comes through and the feed reads as a different material
    // from the clock card touching it above, (36, 34, 33) against the
    // clock's (18, 20, 28).
    root.spawn((FeedCard, node, edge, fill, shadow, menu_ui::Framed))
        .with_children(|card| {
            super::event_timer::spawn_event_timer(card);
            card.spawn((LogLines, lines)).with_children(|list| {
                for index in 0..LOG_LINES {
                    list.spawn((
                        LogLine(index),
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(menu_ui::type_scale::BODY),
                            ..default()
                        },
                        TextColor(Color::NONE),
                    ));
                }
            });
        });
}

/// The feed card's height for a board whose fence stands `foot` interface
/// pixels down the window: from the card's top to there, never shorter than
/// its rows need and never past the sidebar's own foot, `room` down.
fn feed_height(foot: f32, room: f32) -> f32 {
    let top = menu_ui::UNDER_HEADER + LOG_TOP;
    (foot - top).min(room - top).max(LOG_H)
}

/// Keep the feed card's foot level with the foot of the board's fence, so
/// the right-hand column ends where the beach does, whatever the board and
/// the window.
pub fn reach_the_board(
    sim: Res<crate::app::Sim>,
    screen: Res<State<crate::app::Screen>>,
    ui_scale: Res<UiScale>,
    windows: Query<&Window>,
    mut cards: Query<&mut Node, With<FeedCard>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let ui = ui_scale.0.max(f32::EPSILON);
    let foot = crate::app::boot::board_foot(window, ui, *screen.get(), &sim.0);
    let room = window.height() / ui - super::SIDEBAR_FOOT;
    let height = Val::Px(feed_height(foot, room));
    for mut node in &mut cards {
        if node.height != height {
            node.height = height;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The card reaches the fence's foot, but keeps its rows on a short
    /// board and stops at the sidebar's foot on a tall one.
    #[test]
    fn the_feed_reaches_down_to_the_board() {
        let top = menu_ui::UNDER_HEADER + LOG_TOP;
        assert_eq!(feed_height(top + 500.0, top + 600.0), 500.0);
        assert_eq!(
            feed_height(top + 10.0, top + 600.0),
            LOG_H,
            "never under its rows"
        );
        assert_eq!(
            feed_height(top + 900.0, top + 600.0),
            600.0,
            "never past the sidebar"
        );
    }

    /// Lines fit while they fit whole, gaps included; the first that would
    /// run past the foot is dropped, and so is everything after it.
    #[test]
    fn only_whole_lines_are_drawn() {
        assert_eq!(lines_that_fit([20.0, 20.0, 20.0], 5.0, 70.0), 3);
        assert_eq!(lines_that_fit([20.0, 20.0, 20.0], 5.0, 69.0), 2);
        assert_eq!(lines_that_fit([40.0, 20.0, 10.0], 5.0, 60.0), 1);
        assert_eq!(lines_that_fit([], 5.0, 60.0), 0);
        assert_eq!(lines_that_fit([80.0], 5.0, 60.0), 0, "one too tall");
    }

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

    /// A raid says whose gull it was: a rival's arrow names the thief, the
    /// victim's own arrow says so plainly, and a wild gull is the bare raid.
    #[test]
    fn a_raid_line_names_the_steerer() {
        let tr = &crate::app::i18n::EN;
        let name = |seat: u8| ["Maya", "Bob"][usize::from(seat)].to_string();
        assert_eq!(
            raid_line(tr, 1, 23, Some(0), &name),
            "Maya's gull raided Bob! -23"
        );
        assert_eq!(
            raid_line(tr, 1, 23, Some(1), &name),
            "Bob steered a gull into their own castle! -23"
        );
        assert_eq!(raid_line(tr, 1, 23, None, &name), "Bob raided! -23");
    }
}
