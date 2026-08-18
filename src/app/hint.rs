//! The stuck-player hint: after a few failed runs, one signpost of a
//! solution, shown dimmed on the sand.
//!
//! Only ever one, and only on request. A puzzle that hands you the answer is
//! not a puzzle, but a puzzle you cannot see into is not a lesson either,
//! and the levels ship with a solution the tests already replay, so the
//! game knows the answer whether or not it says so.

use crate::app::campaign::CampaignKind;
use crate::app::{Campaign, Phase, Sim, art, layout, palette};
use crate::sim::Placement;
use bevy::prelude::*;

/// Failed runs on one level before the hint is offered.
pub const STUCK_AFTER: u32 = 3;

/// How stuck the player is on the level they are playing, and what they have
/// been shown.
#[derive(Resource, Default)]
pub struct Hints {
    /// The level these counts belong to, so switching levels starts fresh.
    /// The list as well as the index: Tide Pool and Beach Day both have a
    /// fourth level, and being stuck on one is not being stuck on the
    /// other. `None` before any level has been played.
    level: Option<(CampaignKind, usize)>,
    /// Failed runs on it.
    losses: u32,
    /// The placement being shown, if the player asked for one.
    shown: Option<Placement>,
}

impl Hints {
    /// Whether the player has failed enough to be offered a way in.
    pub fn offered(&self) -> bool {
        self.losses >= STUCK_AFTER
    }

    pub fn showing(&self) -> bool {
        self.shown.is_some()
    }

    /// Forget everything: a different level, or a fresh look at this one.
    /// A whole struct literal, so a new field cannot survive a reset unseen.
    fn reset(&mut self, level: (CampaignKind, usize)) {
        *self = Hints {
            level: Some(level),
            losses: 0,
            shown: None,
        };
    }
}

/// The level the campaign is on, as the hint counts it.
fn level_of(campaign: &Campaign) -> (CampaignKind, usize) {
    (campaign.kind, campaign.index)
}

/// The dimmed arrow.
#[derive(Component)]
pub struct HintGhost;

/// Count a failed run, and start over when the level changes.
pub fn record_loss(campaign: Res<Campaign>, mut hints: ResMut<Hints>) {
    if hints.level != Some(level_of(&campaign)) {
        hints.reset(level_of(&campaign));
    }
    hints.losses += 1;
    // A new attempt is a clean board; the ghost is re-shown on request.
    hints.shown = None;
}

/// Drop the hint when the level does.
pub fn reset_on_level(campaign: Res<Campaign>, mut hints: ResMut<Hints>) {
    if hints.level != Some(level_of(&campaign)) {
        hints.reset(level_of(&campaign));
    } else {
        hints.shown = None;
    }
}

/// The placement to show: the first one of the level's solution that the
/// player has not already made. Somebody halfway there is shown their *next*
/// step rather than the one they got right.
fn next_step(campaign: &Campaign, sim: &Sim) -> Option<Placement> {
    campaign
        .current()
        .solution
        .iter()
        .copied()
        .find(|&(x, y, dir)| sim.0.signpost_at(x, y).is_none_or(|sp| sp.dir != dir))
}

/// H, once the player has failed enough, reveals one signpost.
pub fn hint_input(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<crate::app::settings::GameSettings>,
    campaign: Res<Campaign>,
    sim: Res<Sim>,
    mut hints: ResMut<Hints>,
) {
    if !settings.keycaps.just_pressed(&keys, 'H') || !hints.offered() {
        return;
    }
    hints.shown = next_step(&campaign, &sim);
}

/// Keep the ghost arrow matching what the hint is showing.
pub fn draw_hint(
    mut commands: Commands,
    hints: Res<Hints>,
    sim: Res<Sim>,
    art: Res<art::Art>,
    ghosts: Query<Entity, With<HintGhost>>,
) {
    if !hints.is_changed() && !sim.is_changed() {
        return;
    }
    for ghost in &ghosts {
        commands.entity(ghost).despawn();
    }
    let Some((x, y, dir)) = hints.shown else {
        return;
    };
    let pos = layout::tile_center(&sim.0, x, y);
    commands.spawn((
        HintGhost,
        Sprite {
            image: art.arrow.clone(),
            color: palette::PARCHMENT.with_alpha(0.35),
            custom_size: Some(Vec2::splat(layout::TILE * 0.88)),
            ..default()
        },
        Transform::from_translation(pos.extend(layout::z::SIGNPOST - 0.1))
            .with_rotation(layout::dir_rotation(dir)),
    ));
}

pub fn clear_hint_ghosts(mut commands: Commands, ghosts: Query<Entity, With<HintGhost>>) {
    for ghost in &ghosts {
        commands.entity(ghost).despawn();
    }
}

/// The line under the header while the hint is available or showing.
pub fn hint_line(tr: &crate::app::i18n::Tr, hints: &Hints, phase: &Phase) -> Option<String> {
    if !hints.offered() || !matches!(phase, Phase::Setup | Phase::Lost) {
        return None;
    }
    Some(if hints.showing() {
        tr.hint_showing.to_string()
    } else {
        tr.hint_offer.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::CampaignKind;
    use crate::sim::{Level, campaign_levels};

    fn campaign_at(index: usize) -> Campaign {
        let levels = campaign_levels();
        let builtins = levels.len();
        Campaign {
            kind: CampaignKind::TidePool,
            levels,
            index,
            builtins,
        }
    }

    /// The hint shows the next step, not the first one: a player who has
    /// placed half a solution is shown what they are missing.
    #[test]
    fn the_hint_skips_what_is_already_placed() {
        // Level 28 is the one with two placements.
        let campaign = campaign_at(27);
        let level: &Level = campaign.current();
        assert_eq!(level.solution.len(), 2, "picked a two-post level");
        let mut sim = Sim(level.board());
        assert_eq!(next_step(&campaign, &sim), Some(level.solution[0]));

        let (x, y, dir) = level.solution[0];
        assert!(sim.0.place_signpost(0, x, y, dir));
        assert_eq!(
            next_step(&campaign, &sim),
            Some(level.solution[1]),
            "the first step is done, so show the second"
        );

        let (x, y, dir) = level.solution[1];
        assert!(sim.0.place_signpost(0, x, y, dir));
        assert_eq!(next_step(&campaign, &sim), None, "nothing left to show");

        // A post pointing the wrong way is not the step.
        let mut wrong = Sim(level.board());
        let (x, y, dir) = level.solution[0];
        assert!(wrong.0.place_signpost(0, x, y, dir.right()));
        assert_eq!(next_step(&campaign, &wrong), Some(level.solution[0]));
    }

    /// The flow end to end: three failed runs open the offer, and only then
    /// does H show one step: the first one the player is missing.
    #[test]
    fn three_losses_open_the_hint_and_h_shows_one_step() {
        use bevy::ecs::system::RunSystemOnce;

        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::settings::GameSettings>();
        app.init_resource::<Hints>();
        app.insert_resource(campaign_at(27));
        let level = campaign_at(27).current().clone();
        app.insert_resource(Sim(level.board()));

        let lose = |app: &mut App| {
            let _ = app.world_mut().run_system_once(record_loss);
        };
        let ask = |app: &mut App| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::KeyH);
            let _ = app.world_mut().run_system_once(hint_input);
        };

        // Two failures and a hopeful H: nothing yet.
        lose(&mut app);
        lose(&mut app);
        ask(&mut app);
        assert!(!app.world().resource::<Hints>().offered());
        assert!(!app.world().resource::<Hints>().showing());

        // The third opens it, and H takes it.
        lose(&mut app);
        assert!(app.world().resource::<Hints>().offered());
        ask(&mut app);
        assert_eq!(
            app.world().resource::<Hints>().shown,
            Some(level.solution[0]),
            "one signpost, and the first one they are missing"
        );

        // Placing that one and asking again moves the hint along.
        let (x, y, dir) = level.solution[0];
        assert!(
            app.world_mut()
                .resource_mut::<Sim>()
                .0
                .place_signpost(0, x, y, dir)
        );
        ask(&mut app);
        assert_eq!(
            app.world().resource::<Hints>().shown,
            Some(level.solution[1]),
            "the hint follows the player"
        );
    }

    /// Losses only count toward the level they happened on.
    #[test]
    fn switching_levels_starts_the_count_over() {
        let mut hints = Hints::default();
        for _ in 0..STUCK_AFTER {
            hints.level = Some((CampaignKind::TidePool, 4));
            hints.losses += 1;
        }
        assert!(hints.offered());
        hints.reset((CampaignKind::TidePool, 5));
        assert!(!hints.offered(), "a new level is not a stuck one");
    }

    /// The same index in the other list is a different level: three
    /// losses on Tide Pool's fourth do not open a hint on Beach Day's.
    #[test]
    fn the_other_list_at_the_same_index_is_another_level() {
        use bevy::ecs::system::RunSystemOnce;

        let mut app = App::new();
        app.init_resource::<Hints>();
        app.insert_resource(campaign_at(4));
        for _ in 0..STUCK_AFTER {
            let _ = app.world_mut().run_system_once(record_loss);
        }
        assert!(app.world().resource::<Hints>().offered());
        app.world_mut().resource_mut::<Campaign>().kind = CampaignKind::BeachDay;
        let _ = app.world_mut().run_system_once(reset_on_level);
        assert!(
            !app.world().resource::<Hints>().offered(),
            "Beach Day's fourth level is not the one they were stuck on"
        );
    }
}
