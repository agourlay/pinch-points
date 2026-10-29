//! The running tide events, pinned at the top of the feed card: each one's
//! name, the seconds it has left, and a bar draining with them, stacked
//! when a spectators' call lands on top of a spun event.
//!
//! Speed Up, Slow Down and Right Claws change the rules for ten seconds
//! and leave nothing on the beach to say so: the banner is gone in two and
//! a half, and the header's line in three. A player banking a left claw at
//! second eight had no way to know the call was still on. The block reads
//! [`crate::sim::Board::running_events`], the same ticks the sim counts
//! down, so it freezes with the round under a pause and ends on the tick
//! the event does. Every running event gets its own block: the three run
//! in separate slots, and a block naming only the first left the other
//! running with nothing on screen.

use crate::app::i18n::{Tr, fill};
use crate::app::settings::GameSettings;
use crate::app::{Sim, menu_ui, palette};
use crate::sim::{Board, EVENT_TICKS, TICKS_PER_SECOND};
use bevy::prelude::*;

/// How many timed events can run at once, and so how many blocks there
/// are: one per slot (the mania, the tempo, the claw call).
const SLOTS: usize = 3;

/// One block, shown only while its running event does: the `n`th of
/// [`timer_lines`].
#[derive(Component)]
pub struct EventTimer(usize);

/// The `n`th running event's name.
#[derive(Component)]
pub struct EventTimerName(usize);

/// "7 s left".
#[derive(Component)]
pub struct EventTimerLeft(usize);

/// The bar's fill, as wide as the share of the event still to run.
#[derive(Component)]
pub struct EventTimerFill(usize);

/// The feed card's side padding, which the block shares with the lines.
pub(super) const FEED_PAD_X: f32 = 12.0;

/// The room a line in the feed card has: the sidebar, less the card's
/// inset from it, its border and its padding.
#[cfg(test)]
const FEED_TEXT_W: f32 =
    super::SIDEBAR_W - 2.0 * 10.0 - 2.0 * super::CARD_BORDER - 2.0 * FEED_PAD_X;

/// Type sizes: the name as a small headline, the seconds as a feed line.
const NAME_PX: f32 = menu_ui::type_scale::ROW;
const LEFT_PX: f32 = menu_ui::type_scale::BODY;

/// The least the bar keeps beside the seconds, so it is still a bar.
const BAR_MIN_W: f32 = 40.0;
const BAR_H: f32 = 6.0;

/// What the blocks say about `board`, one per running event in the order
/// [`Board::running_events`] gives them: the event's name, the seconds
/// left as a line, and the share of the event still to run. Empty when no
/// timed event runs, or once the round is over (an event cut short by the
/// whistle has nothing left to time).
///
/// The seconds are rounded up, as every countdown in the game is, and
/// `stepped` (reduced motion) holds the bar at whole seconds with them
/// rather than sliding.
pub fn timer_lines<'a>(tr: &'a Tr, board: &Board, stepped: bool) -> Vec<(&'a str, String, f32)> {
    if board.round_over() {
        return Vec::new();
    }
    board
        .running_events()
        .map(|(event, ticks)| {
            let secs = ticks.div_ceil(TICKS_PER_SECOND);
            let shown = if stepped {
                secs * TICKS_PER_SECOND
            } else {
                ticks
            };
            let share = (shown as f32 / EVENT_TICKS as f32).clamp(0.0, 1.0);
            (
                crate::app::hud::event_name(tr, event),
                fill(tr.event_left, &[("s", &secs.to_string())]),
                share,
            )
        })
        .collect()
}

/// Every event [`timer_lines`] can name: the ones that last.
#[cfg(test)]
fn timed_events() -> impl Iterator<Item = crate::sim::TideEvent> {
    crate::sim::TideEvent::ALL
        .into_iter()
        .filter(|event| event.lasts())
}

/// Spawn the blocks at the top of the feed card, one per slot, each hidden
/// until an event runs in it. The lines below give them room when they
/// show, as they do for every new line.
pub(super) fn spawn_event_timer(card: &mut ChildSpawnerCommands) {
    for slot in 0..SLOTS {
        spawn_block(card, slot);
    }
}

/// One block: the name over its seconds and bar.
fn spawn_block(card: &mut ChildSpawnerCommands, slot: usize) {
    card.spawn((
        EventTimer(slot),
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            margin: UiRect::bottom(Val::Px(10.0)),
            flex_shrink: 0.0,
            display: Display::None,
            ..default()
        },
    ))
    .with_children(|block| {
        block.spawn((
            EventTimerName(slot),
            Text::new(""),
            menu_ui::display_font(NAME_PX),
            TextColor(palette::INK_TIDE),
            TextLayout::no_wrap(),
        ));
        block
            .spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    EventTimerLeft(slot),
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(LEFT_PX),
                        ..default()
                    },
                    TextColor(palette::PARCHMENT),
                    TextLayout::no_wrap(),
                ));
                row.spawn((
                    Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(BAR_MIN_W),
                        height: Val::Px(BAR_H),
                        border_radius: BorderRadius::all(Val::Px(BAR_H / 2.0)),
                        ..default()
                    },
                    // The tide's ink, faint: the card's own dark under a
                    // dark pill hid the track, and a bar with no visible
                    // full length does not say how much is gone.
                    BackgroundColor(palette::INK_TIDE.with_alpha(0.25)),
                ))
                .with_children(|track| {
                    track.spawn((
                        EventTimerFill(slot),
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            border_radius: BorderRadius::all(Val::Px(BAR_H / 2.0)),
                            ..default()
                        },
                        BackgroundColor(palette::INK_TIDE),
                    ));
                });
            });
    });
}

/// The pieces of the blocks a frame writes.
type TimerParts<'w, 's> = (
    Query<'w, 's, (&'static EventTimer, &'static mut Node), Without<EventTimerFill>>,
    Query<'w, 's, (&'static EventTimerName, &'static mut Text), Without<EventTimerLeft>>,
    Query<'w, 's, (&'static EventTimerLeft, &'static mut Text), Without<EventTimerName>>,
    Query<'w, 's, (&'static EventTimerFill, &'static mut Node), Without<EventTimer>>,
);

/// Show each running event and its time left, and hide the spare blocks.
pub fn update_event_timer(
    sim: Res<Sim>,
    settings: Res<GameSettings>,
    phase: Res<State<crate::app::VersusPhase>>,
    mut parts: ParamSet<TimerParts>,
) {
    // Gone with the results card too, however the round ended: a table
    // reading who won has no use for a clock on an event in a round that
    // is over.
    let lines = match phase.get() {
        crate::app::VersusPhase::Over => Vec::new(),
        crate::app::VersusPhase::Countdown | crate::app::VersusPhase::Running => {
            timer_lines(settings.tr(), &sim.0, settings.reduced_motion)
        }
    };
    for (block, mut node) in &mut parts.p0() {
        let display = match lines.get(block.0) {
            Some(_) => Display::Flex,
            None => Display::None,
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (name, mut text) in &mut parts.p1() {
        if let Some((line, _, _)) = lines.get(name.0) {
            menu_ui::set_text(&mut text, line);
        }
    }
    for (left, mut text) in &mut parts.p2() {
        if let Some((_, line, _)) = lines.get(left.0) {
            menu_ui::set_text(&mut text, line);
        }
    }
    for (bar, mut node) in &mut parts.p3() {
        if let Some(&(_, _, share)) = lines.get(bar.0) {
            let width = Val::Percent(share * 100.0);
            if node.width != width {
                node.width = width;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::i18n::{ALL_LANGS, EN, metrics};
    use crate::sim::TideEvent;

    /// The block counts the sim's own ticks: the seconds follow the event
    /// down to its last tick, then the block is gone, and a round that is
    /// over shows nothing even with the tempo still set.
    #[test]
    fn the_timer_counts_the_events_own_ticks() {
        let mut board = Board::new(7, 5, 3);
        board.set_events_enabled(true);
        board.rules_mut().round_length = Some(10 * EVENT_TICKS);
        assert!(timer_lines(&EN, &board, false).is_empty(), "nothing runs");
        board.force_tide_event(TideEvent::SpeedUp, 0);
        let [(name, left, share)] = one(&board, false);
        assert_eq!(name, EN.events[TideEvent::SpeedUp.index()]);
        let full = (EVENT_TICKS / TICKS_PER_SECOND).to_string();
        assert_eq!(left, fill(EN.event_left, &[("s", &full)]));
        assert_eq!(share, 1.0);
        // A third of a second in: still the full count, rounded up, while
        // the smooth bar has moved and the stepped one has not.
        for _ in 0..TICKS_PER_SECOND / 3 {
            board.tick_idle();
        }
        let [(_, left, smooth)] = one(&board, false);
        assert_eq!(left, fill(EN.event_left, &[("s", &full)]));
        assert!(smooth < 1.0);
        let [(_, _, stepped)] = one(&board, true);
        assert_eq!(stepped, 1.0);
        while board.running_event().is_some() {
            let [(_, left, _)] = one(&board, false);
            assert_ne!(left, fill(EN.event_left, &[("s", "0")]), "zero while on");
            board.tick_idle();
        }
        assert!(timer_lines(&EN, &board, false).is_empty(), "gone with it");

        board.force_tide_event(TideEvent::RightClaws, 0);
        board.rules_mut().round_length = Some(board.ticks() as u32);
        assert!(board.round_over() && board.running_event().is_some());
        assert!(
            timer_lines(&EN, &board, false).is_empty(),
            "the round is over"
        );
    }

    /// The one block running on `board`.
    fn one(board: &Board, stepped: bool) -> [(&'static str, String, f32); 1] {
        timer_lines(&EN, board, stepped)
            .try_into()
            .expect("exactly one event runs")
    }

    /// Speed Up spun, then a Crab Mania called by the watchers three
    /// seconds in: both blocks show, each with its own seconds, the mania
    /// first, as the banner names it.
    #[test]
    fn overlapping_events_each_get_a_block() {
        let mut board = Board::new(7, 5, 3);
        board.set_events_enabled(true);
        board.rules_mut().round_length = Some(10 * EVENT_TICKS);
        board.force_tide_event(TideEvent::SpeedUp, 0);
        for _ in 0..3 * TICKS_PER_SECOND {
            board.tick_idle();
        }
        board.force_tide_event(TideEvent::CrabMania, 0);
        let full = EVENT_TICKS / TICKS_PER_SECOND;
        let lines = timer_lines(&EN, &board, false);
        let names: Vec<&str> = lines.iter().map(|line| line.0).collect();
        assert_eq!(
            names,
            vec![
                EN.events[TideEvent::CrabMania.index()],
                EN.events[TideEvent::SpeedUp.index()]
            ]
        );
        let secs = |n: u32| fill(EN.event_left, &[("s", &n.to_string())]);
        assert_eq!(lines[0].1, secs(full));
        assert_eq!(lines[1].1, secs(full - 3));
        assert!(lines[1].2 < lines[0].2, "Speed Up's bar has drained more");
        assert!(lines.len() <= SLOTS);
    }

    /// Both lines fit the feed card on one row each, in every language,
    /// for every event that lasts, at the longest count.
    #[test]
    fn the_timer_fits_the_feed_card_in_every_language() {
        let most = (EVENT_TICKS / TICKS_PER_SECOND).to_string();
        for lang in ALL_LANGS {
            let tr = lang.tr();
            for event in timed_events() {
                let name = crate::app::hud::event_name(tr, event);
                let wide = metrics::display_px(name, NAME_PX);
                assert!(
                    wide <= FEED_TEXT_W,
                    "{lang:?} {event:?}: {name} is {wide} px of {FEED_TEXT_W}"
                );
            }
            let left = fill(tr.event_left, &[("s", &most)]);
            let wide = metrics::text_px(&left, LEFT_PX);
            assert!(
                wide + BAR_MIN_W + 8.0 <= FEED_TEXT_W,
                "{lang:?}: {left} is {wide} px beside the bar"
            );
        }
    }
}
