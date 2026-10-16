//! Watching a recording: the speed it plays at, Space to stop and start
//! it, and the transport bar under the board that says how far through the
//! round it is.

use crate::app::settings::GameSettings;
use crate::app::{Playback, Screen, menu_ui, palette};
use bevy::prelude::*;

/// Playback speed while watching, so a three-minute round can be skimmed.
#[derive(Resource)]
pub struct PlaybackSpeed(pub u8);

impl Default for PlaybackSpeed {
    fn default() -> Self {
        PlaybackSpeed(1)
    }
}

impl PlaybackSpeed {
    /// The speeds the key steps through.
    pub const STEPS: [u8; 3] = [1, 2, 4];

    fn stepped(self) -> PlaybackSpeed {
        let next = Self::STEPS
            .iter()
            .position(|&s| s == self.0)
            .map_or(0, |at| (at + 1) % Self::STEPS.len());
        PlaybackSpeed(Self::STEPS[next])
    }
}

/// `S` cycles 1x, 2x, 4x while a replay is playing. The key whose cap
/// says S, as every other letter key is read: the prompt says "S".
pub fn playback_speed_input(
    keys: Res<ButtonInput<KeyCode>>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    playback: Res<Playback>,
    mut speed: ResMut<PlaybackSpeed>,
) {
    if playback.0.is_some() && caps.just_pressed(&keys, 'S') {
        *speed = PlaybackSpeed(speed.0).stepped();
    }
}

/// The row of controls under a replay: where the round has got to, whether
/// it is running, and how fast.
///
/// The shape people already know from every video player: a track that
/// fills, a state, and a clock.
#[derive(Component)]
pub struct ReplayBar;

/// The filled part of the track. Its width is the round's progress.
#[derive(Component)]
pub struct ReplayProgress;

/// The line under the track: the state, the speed, and the clock.
#[derive(Component)]
pub struct ReplayReadout;

/// How wide the track is drawn, in pixels.
const TRACK: f32 = 420.0;

/// Keep the bar up while a recording is playing, and only then.
///
/// Reconciled every frame rather than hung off the screen transition: a
/// replay is entered and left through the same versus screen a live match
/// uses, so there is no transition of its own to hang it on.
pub fn tend_replay_bar(
    mut commands: Commands,
    playback: Res<Playback>,
    screen: Res<State<Screen>>,
    bar: Query<Entity, With<ReplayBar>>,
) {
    let watching = playback.0.is_some() && *screen.get() == Screen::Versus;
    if !watching {
        for entity in &bar {
            commands.entity(entity).despawn();
        }
        return;
    }
    if !bar.is_empty() {
        return;
    }
    commands
        .spawn((
            ReplayBar,
            Node {
                position_type: PositionType::Absolute,
                // The band the chrome reserves at the foot of the board,
                // which the field guide stands down from while a
                // recording is playing.
                bottom: Val::Px(56.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(5.0),
                ..default()
            },
        ))
        .with_children(|column| {
            // The track: a sunk rail with a gold fill creeping along it.
            column
                .spawn((
                    Node {
                        width: Val::Px(TRACK),
                        height: Val::Px(6.0),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(palette::BAR_TRACK),
                ))
                .with_children(|rail| {
                    rail.spawn((
                        ReplayProgress,
                        Node {
                            width: Val::Percent(0.0),
                            height: Val::Percent(100.0),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(palette::GOLD),
                    ));
                });
            column
                .spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(12.0), Val::Px(3.0)),
                        border_radius: BorderRadius::all(Val::Px(10.0)),
                        ..default()
                    },
                    BackgroundColor(palette::PILL_FILL),
                ))
                .with_children(|pill| {
                    pill.spawn((
                        ReplayReadout,
                        Text::new(String::new()),
                        TextFont {
                            font_size: FontSize::Px(menu_ui::type_scale::FINE),
                            ..default()
                        },
                        TextLayout::no_wrap(),
                        TextColor(palette::PARCHMENT),
                    ));
                });
        });
}

/// Minutes and seconds of a tick count, at the sim's fixed rate.
fn stamp(ticks: usize) -> String {
    let seconds = ticks / crate::sim::TICKS_PER_SECOND as usize;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// Write the track and the readout from where the recording has got to.
pub fn update_replay_bar(
    playback: Res<Playback>,
    paused: Res<crate::app::Paused>,
    speed: Res<PlaybackSpeed>,
    settings: Res<GameSettings>,
    mut track: Query<&mut Node, With<ReplayProgress>>,
    mut readout: Query<&mut Text, With<ReplayReadout>>,
) {
    let Some((replay, at)) = &playback.0 else {
        return;
    };
    let total = replay.inputs.len().max(1);
    let through = (*at).min(total);
    let width = Val::Percent(through as f32 / total as f32 * 100.0);
    for mut node in &mut track {
        // Written only when it moves: a `Node` marked changed relays out
        // the whole interface, every frame the replay plays.
        if node.width != width {
            node.width = width;
        }
    }
    let tr = settings.tr();
    let line = format!(
        "{}  {}x   {} / {}",
        if paused.0 {
            tr.replay_paused
        } else {
            tr.replay_playing
        },
        speed.0,
        stamp(through),
        stamp(total)
    );
    for mut text in &mut readout {
        menu_ui::set_text(&mut text, &line);
    }
}

/// Space stops and starts a recording.
///
/// Only while one is playing: in a live match the same key pulls a
/// signpost.
///
/// The pause card is kept off this by `keys_are_free` in the schedule,
/// which this system used to do for itself: the card sets `Paused` once,
/// on opening, and never re-asserts it, so a Space pressed under it would
/// start the recording running again with the card still up saying it is
/// stopped. Every other key on a play screen wanted the same rule.
pub fn playback_pause_input(
    keys: Res<ButtonInput<KeyCode>>,
    playback: Res<Playback>,
    mut paused: ResMut<crate::app::Paused>,
) {
    if playback.0.is_none() {
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        paused.0 = !paused.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The transport clock reads as a clock. It is the one place a player
    /// sees how far through a round they are, and `0:7` for seven seconds
    /// past the minute is the kind of thing that reads as a bug.
    #[test]
    fn the_transport_clock_pads_its_seconds() {
        assert_eq!(stamp(0), "0:00", "the very start");
        assert_eq!(stamp(crate::sim::TICKS_PER_SECOND as usize * 7), "0:07");
        assert_eq!(stamp(crate::sim::TICKS_PER_SECOND as usize * 59), "0:59");
        assert_eq!(stamp(crate::sim::TICKS_PER_SECOND as usize * 60), "1:00");
        // A three-minute Turf War, which is the longest round that ships.
        assert_eq!(stamp(crate::sim::TICKS_PER_SECOND as usize * 185), "3:05");
    }

    /// It counts in ticks, not in frames: the transport is drawn every
    /// frame and the sim advances thirty times a second, so a clock that
    /// read the wrong one would run at whatever the display happened to do.
    #[test]
    fn the_transport_clock_counts_in_ticks() {
        let one_second = crate::sim::TICKS_PER_SECOND as usize;
        assert_eq!(stamp(one_second - 1), "0:00", "part of a second is not one");
        assert_eq!(stamp(one_second), "0:01", "and a whole one is");
    }

    #[test]
    fn the_speed_key_cycles_the_steps() {
        let mut speed = PlaybackSpeed::default();
        assert_eq!(speed.0, 1);
        for want in [2, 4, 1, 2] {
            speed = PlaybackSpeed(speed.0).stepped();
            assert_eq!(speed.0, want);
        }
    }
}
