//! The three seconds before a round: the beach is laid out and holding
//! still, so everyone can find their castle and read the map before the
//! first crab moves, and a count in the middle of the screen says when.
//!
//! A phase of its own ([`VersusPhase::Countdown`]) rather than a flag, so
//! everything that waits for a round to be live (the sim, the placements,
//! the AI, the tide clock, the round-over check, the net's patience with a
//! silent seat) already waits for it, without being told.
//!
//! Online, every peer counts on its own clock from the moment its `Start`
//! lands, so the counts end as far apart as the `Start`s arrived, which is
//! the same skew a round has always opened with. Nothing is sent while a
//! peer counts: the lockstep keeps resending every commit a peer could be
//! missing, so the frames the first one out commits reach the others when
//! they come out too, and the round-holding checks only run once the round
//! is, so three seconds of quiet costs nobody their seat.

use crate::app::settings::GameSettings;
use crate::app::{VersusPhase, menu_ui, palette};
use bevy::prelude::*;

/// How long the beach holds still before the round starts.
pub const COUNTDOWN_SECS: f32 = 3.0;

/// How long the word that starts the round stays up, fading as it goes.
const GO_SECS: f32 = 0.7;

/// The count's size: a documented literal rather than one of the type
/// scale's, because nothing else on screen is meant to be read from across
/// a room. Drawn at one size and popped by scale, which keeps it to one
/// glyph atlas (see the score chips' pop for the cost of the other way).
const COUNT_PX: f32 = 160.0;

/// How far over its size a number lands before settling.
const POP: f32 = 0.45;

/// Seconds left on the count.
#[derive(Resource, Default)]
pub struct Countdown {
    left: f32,
}

impl Countdown {
    /// The number on screen: 3, 2, then 1, each for its own second.
    fn shown(&self) -> u32 {
        self.left.ceil().max(1.0) as u32
    }

    /// How far into its second the number on screen is, 0 to 1.
    fn settled(&self) -> f32 {
        (self.left.ceil() - self.left).clamp(0.0, 1.0)
    }
}

/// How a versus round opens: on the count, or at once (a recording, a
/// round joined or resumed part way, which is already moving).
#[derive(bevy::ecs::system::SystemParam)]
pub struct Opening<'w> {
    countdown: ResMut<'w, Countdown>,
    next_vphase: ResMut<'w, NextState<VersusPhase>>,
}

impl Opening<'_> {
    pub fn open(&mut self, count_in: bool) {
        self.countdown.left = match count_in {
            true => COUNTDOWN_SECS,
            false => 0.0,
        };
        self.next_vphase.set(match count_in {
            true => VersusPhase::Countdown,
            false => VersusPhase::Running,
        });
    }
}

/// The count in the middle of the screen, and then the word that starts
/// the round. `go` is how long that word has been up.
#[derive(Component)]
pub struct CountdownText {
    go: Option<f32>,
}

/// Put the count on screen, for a round that opens on one. Chained after
/// `load_versus`, which winds it.
pub fn show_countdown(mut commands: Commands, countdown: Res<Countdown>) {
    if countdown.left <= 0.0 {
        return;
    }
    commands
        .spawn((
            CountdownText { go: None },
            GlobalZIndex(menu_ui::layer::BANNER),
            menu_ui::centred_overlay(),
        ))
        .with_children(|wrap| {
            wrap.spawn((
                Text::new(countdown.shown().to_string()),
                menu_ui::display_font(COUNT_PX),
                TextColor(palette::GOLD),
                TextShadow {
                    offset: Vec2::new(4.0, 5.0),
                    color: Color::BLACK.with_alpha(0.6),
                },
                UiTransform::IDENTITY,
            ));
        });
}

/// Run the count down, and start the round when it is out. It holds while
/// the pause card is up: the card is a promise that nothing moves.
pub fn run_countdown(
    time: Res<Time>,
    menu: Res<crate::app::pause::PauseMenu>,
    mut countdown: ResMut<Countdown>,
    mut next_vphase: ResMut<NextState<VersusPhase>>,
) {
    if menu.open {
        return;
    }
    countdown.left -= time.delta_secs();
    if countdown.left <= 0.0 {
        next_vphase.set(VersusPhase::Running);
    }
}

/// Draw the count: the number for this second, landing big and settling,
/// and once the round is live the word that started it, fading out.
pub fn draw_countdown(
    mut commands: Commands,
    time: Res<Time>,
    countdown: Res<Countdown>,
    phase: Res<State<VersusPhase>>,
    settings: Res<GameSettings>,
    mut overlays: Query<(Entity, &mut CountdownText, &Children)>,
    mut texts: Query<(&mut Text, &mut TextColor, &mut UiTransform)>,
) {
    let calm = settings.reduced_motion;
    for (entity, mut overlay, children) in &mut overlays {
        let (word, ink, settle) = match phase.get() {
            VersusPhase::Countdown => (
                countdown.shown().to_string(),
                palette::GOLD,
                countdown.settled(),
            ),
            VersusPhase::Running => {
                let up = overlay.go.unwrap_or(0.0) + time.delta_secs();
                overlay.go = Some(up);
                if up >= GO_SECS {
                    commands.entity(entity).despawn();
                    continue;
                }
                let fade = 1.0 - up / GO_SECS;
                (
                    settings.tr().countdown_go.to_string(),
                    palette::PARCHMENT.with_alpha(fade),
                    up / GO_SECS,
                )
            }
            // The tide came in under it, which a three-second count cannot
            // outlast, but nothing should be left standing on the card.
            VersusPhase::Over => {
                commands.entity(entity).despawn();
                continue;
            }
        };
        let scale = match calm {
            true => 1.0,
            // Lands over its size and eases down, the way a stamp does.
            false => 1.0 + POP * (1.0 - settle).powi(3),
        };
        for child in children {
            let Ok((mut text, mut color, mut transform)) = texts.get_mut(*child) else {
                continue;
            };
            menu_ui::set_text(&mut text, &word);
            menu_ui::set_color(&mut color, ink);
            let pop = Vec2::splat(scale);
            if transform.scale != pop {
                transform.scale = pop;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three, two, one, each for a second, and never a zero: the round
    /// starts on the word, not on a number nobody counts to.
    #[test]
    fn the_count_reads_three_two_one() {
        let at = |left: f32| Countdown { left }.shown();
        assert_eq!(at(3.0), 3);
        assert_eq!(at(2.01), 3);
        assert_eq!(at(2.0), 2);
        assert_eq!(at(1.5), 2);
        assert_eq!(at(0.5), 1);
        assert_eq!(at(0.0), 1);
        assert_eq!(at(-0.1), 1);
    }

    /// The beach holds still for the count, and only for the count: the
    /// sim does not tick, the round starts when it is out, and the pause
    /// card holds it.
    #[test]
    fn the_round_starts_when_the_count_is_out() {
        use bevy::state::app::StatesPlugin;
        let mut app = App::new();
        app.add_plugins(StatesPlugin);
        app.init_state::<VersusPhase>();
        app.insert_resource(State::new(VersusPhase::Countdown));
        app.init_resource::<Countdown>();
        app.init_resource::<crate::app::pause::PauseMenu>();
        app.insert_resource(Time::<()>::default());
        app.world_mut().resource_mut::<Countdown>().left = COUNTDOWN_SECS;
        app.add_systems(Update, run_countdown);
        let step = |app: &mut App, secs: f32| {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(secs));
            app.update();
        };
        let phase = |app: &App| *app.world().resource::<State<VersusPhase>>().get();

        step(&mut app, 1.0);
        step(&mut app, 1.0);
        assert_eq!(phase(&app), VersusPhase::Countdown, "two seconds in");
        app.world_mut()
            .resource_mut::<crate::app::pause::PauseMenu>()
            .open = true;
        step(&mut app, 5.0);
        assert_eq!(phase(&app), VersusPhase::Countdown, "held by the card");
        app.world_mut()
            .resource_mut::<crate::app::pause::PauseMenu>()
            .open = false;
        step(&mut app, 1.1);
        // The state change lands on the next frame's transition.
        app.update();
        assert_eq!(phase(&app), VersusPhase::Running, "and out");
    }
}
