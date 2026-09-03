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

/// How long "no signposts left" stays on the hint line, in seconds.
const DENIED_SECS: f32 = 2.5;

/// A refusal the player deserves a sentence about, counting down.
///
/// Only the spent-inventory one. Every other refusal is answered by
/// aiming somewhere else and the flash says enough; this one is answered
/// by picking a signpost back up, and nothing on screen said so.
#[derive(Resource, Default)]
pub struct DeniedNote(f32);

/// Catch the denials worth explaining and start the countdown.
pub fn note_denials(
    mut denials: MessageReader<crate::app::PlacementDenied>,
    mut note: ResMut<DeniedNote>,
) {
    if denials.read().any(|d| d.player == 0 && d.out_of_signposts) {
        note.0 = DENIED_SECS;
    }
}

/// Age it out. A level change clears it too: the sentence is about the
/// board that refused, and that board is gone.
pub fn tick_denied_note(
    time: Res<Time>,
    campaign: Res<crate::app::Campaign>,
    mut note: ResMut<DeniedNote>,
) {
    if campaign.is_changed() {
        note.0 = 0.0;
        return;
    }
    note.0 = (note.0 - time.delta_secs()).max(0.0);
}

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
    caps: Res<crate::app::keycaps::KeyCaps>,
    campaign: Res<Campaign>,
    sim: Res<Sim>,
    mut hints: ResMut<Hints>,
) {
    if !caps.just_pressed(&keys, 'H') || !hints.offered() {
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
    mut drawn_on: Local<(u8, u8)>,
) {
    // Redrawn when the hint changes, or when the board's size does: the
    // ghost sits at a tile centre, which only the size moves. Redrawing
    // on every change to the sim tore the sprite down and put it back
    // thirty times a second for as long as a hint was up during a run.
    // Every level load writes `hints`, so a swapped board of the same
    // size is caught there.
    let size = (sim.0.width(), sim.0.height());
    if !hints.is_changed() && !(sim.is_changed() && *drawn_on != size) {
        return;
    }
    *drawn_on = size;
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
pub fn hint_line(
    tr: &crate::app::i18n::Tr,
    hints: &Hints,
    denied: &DeniedNote,
    phase: &Phase,
) -> Option<String> {
    // The denial answers the key that was just pressed, so it outranks both
    // the stuck-hint and the level's lesson for as long as it lasts.
    if denied.0 > 0.0 {
        return Some(tr.denied_no_posts.to_string());
    }
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
        app.init_resource::<crate::app::keycaps::KeyCaps>();
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

    /// Running out of signposts says so, and says it differently from
    /// every other refusal.
    ///
    /// Both kinds of no fire the same flash and the same knock, so until
    /// this line existed "you have none left" and "not on that tile" were
    /// indistinguishable - which is what players hit when a level handed
    /// out fewer signposts than they wanted.
    #[test]
    fn a_spent_inventory_gets_a_sentence_of_its_own() {
        use crate::app::i18n::EN;
        let hints = Hints::default();
        assert!(!hints.offered(), "not stuck: no line of its own to compete");

        assert_eq!(
            hint_line(&EN, &hints, &DeniedNote(0.0), &Phase::Setup),
            None
        );
        assert_eq!(
            hint_line(&EN, &hints, &DeniedNote(1.0), &Phase::Setup).as_deref(),
            Some(EN.denied_no_posts),
        );
        // It outranks the stuck-hint, which is about the level rather than
        // about the key just pressed.
        let stuck = Hints {
            losses: STUCK_AFTER,
            ..Default::default()
        };
        assert_eq!(
            hint_line(&EN, &stuck, &DeniedNote(0.0), &Phase::Setup).as_deref(),
            Some(EN.hint_offer),
        );
        assert_eq!(
            hint_line(&EN, &stuck, &DeniedNote(1.0), &Phase::Setup).as_deref(),
            Some(EN.denied_no_posts),
        );
    }

    /// Only the inventory refusal raises it. A tile that simply cannot take
    /// a signpost is answered by aiming elsewhere, and the flash says so.
    #[test]
    fn only_the_inventory_refusal_is_worth_a_sentence() {
        use crate::app::PlacementDenied;
        use bevy::ecs::system::RunSystemOnce;

        let mut app = App::new();
        app.init_resource::<DeniedNote>();
        app.add_message::<PlacementDenied>();

        app.world_mut().write_message(PlacementDenied {
            player: 0,
            out_of_signposts: false,
        });
        let _ = app.world_mut().run_system_once(note_denials);
        assert_eq!(app.world().resource::<DeniedNote>().0, 0.0, "a bad tile");

        app.world_mut().write_message(PlacementDenied {
            player: 0,
            out_of_signposts: true,
        });
        let _ = app.world_mut().run_system_once(note_denials);
        assert!(
            app.world().resource::<DeniedNote>().0 > 0.0,
            "a spent inventory"
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
