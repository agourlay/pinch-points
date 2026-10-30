//! The stuck-player hint: after a few failed runs, one signpost of a
//! solution, shown dimmed on the sand.
//!
//! Only ever one, and only on request: a puzzle that hands you the answer
//! is not a puzzle, and one you cannot see into is not a lesson. The levels
//! ship with a solution the tests already replay, so the game knows the
//! answer whether or not it says so.

use crate::app::i18n::fill;
use crate::app::{Campaign, Phase, Sim, art, layout, palette};
use crate::sim::Placement;
use bevy::prelude::*;

/// Failed runs on one level before the hint is offered.
pub const STUCK_AFTER: u32 = 3;

/// How stuck the player is on the level they are playing, and what they have
/// been shown.
#[derive(Resource, Default)]
pub struct Hints {
    /// The index of the level these counts belong to, so switching levels
    /// starts fresh. `None` until the first load since the puzzle screen
    /// was entered ([`reset_on_entry`]).
    level: Option<usize>,
    /// Failed runs on it.
    losses: u32,
    /// Whether the level carries a solution to take a step from. A
    /// player's own level and a pasted one have none, and offering H there
    /// promised a hint the key then could not give.
    answerable: bool,
    /// The placement being shown, if the player asked for one.
    shown: Option<Placement>,
}

impl Hints {
    /// Whether the player has failed enough to be offered a way in.
    pub fn offered(&self) -> bool {
        self.answerable && self.losses >= STUCK_AFTER
    }

    pub fn showing(&self) -> bool {
        self.shown.is_some()
    }

    /// Forget everything: a different level, or a fresh look at this one.
    /// A whole struct literal, so a new field cannot survive a reset unseen.
    fn reset(&mut self, campaign: &Campaign) {
        *self = Hints {
            level: Some(campaign.index),
            losses: 0,
            answerable: !campaign.current().solution.is_empty(),
            shown: None,
        };
    }
}

/// The dimmed arrow.
#[derive(Component)]
pub struct HintGhost;

/// How long "no signposts left" stays on the hint line, in seconds.
const DENIED_SECS: f32 = 2.5;

/// A refusal the player deserves a sentence about, counting down.
///
/// Only the spent-inventory one. Every other refusal is answered by aiming
/// somewhere else and the flash says enough; this one is answered by
/// picking a signpost back up.
#[derive(Resource, Default)]
pub struct DeniedNote(f32);

/// Catch the denials worth explaining and start the countdown: any
/// cursor's, as on a co-op stage each places for its own seat and P2's
/// hand runs out as P1's does.
pub fn note_denials(
    mut denials: MessageReader<crate::app::PlacementDenied>,
    mut note: ResMut<DeniedNote>,
) {
    if denials.read().any(|d| d.out_of_signposts) {
        note.0 = DENIED_SECS;
    }
}

/// Age it out. A level change clears it too: the sentence is about the
/// board that refused, and that board is gone.
pub fn tick_denied_note(time: Res<Time>, campaign: Res<Campaign>, mut note: ResMut<DeniedNote>) {
    if campaign.is_changed() {
        note.0 = 0.0;
        return;
    }
    note.0 = (note.0 - time.delta_secs()).max(0.0);
}

/// Entering the puzzle screen starts the count over, as it does the
/// attempt ([`crate::app::achievements::reset_puzzle_attempt`]): the list
/// behind the index can have been rebuilt on the way in (a pasted code,
/// a shelf another level now sorts into), so the same index is no proof
/// of the same level, and the first load then counts afresh for the
/// level it built.
pub fn reset_on_entry(mut hints: ResMut<Hints>) {
    *hints = Hints::default();
}

/// Count a failed run, and start over when the level changes.
pub fn record_loss(campaign: Res<Campaign>, mut hints: ResMut<Hints>) {
    if hints.level != Some(campaign.index) {
        hints.reset(&campaign);
    }
    hints.losses += 1;
    // A new attempt is a clean board; the ghost is re-shown on request.
    hints.shown = None;
}

/// Drop the hint when the level does, or when the posts it was a next
/// step from are cleared. A retry keeps a hint asked for: the Lost card
/// offers H, and the Enter it then asks for is a retry, which used to take
/// the step off the board before it could be placed. Kept as a request,
/// worked out again from the board the retry laid: that is the arrows the
/// run began with, not the ones left standing when it was lost, so the
/// step taken off the lost board could be one the retry puts back.
///
/// Runs after the load has built the board.
pub fn reset_on_level(
    campaign: Res<Campaign>,
    sim: Res<Sim>,
    mut loads: MessageReader<crate::app::LoadLevel>,
    mut hints: ResMut<Hints>,
) {
    let cleared = loads.read().any(|load| !load.keep_posts);
    if hints.level != Some(campaign.index) {
        hints.reset(&campaign);
    } else if cleared {
        hints.shown = None;
    } else if hints.shown.is_some() {
        hints.shown = next_step(&campaign, &sim);
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
        .find(|&(x, y, dir, seat)| {
            sim.0
                .signpost_at(x, y)
                .is_none_or(|sp| sp.dir != dir || sp.owner != seat)
        })
}

/// H (or the face's pad button for it), once the player has failed
/// enough, reveals one signpost.
pub fn hint_input(
    keyboard: crate::app::keycaps::Keyboard,
    pads: crate::app::gamepad::face::PadsOnFace,
    campaign: Res<Campaign>,
    sim: Res<Sim>,
    mut hints: ResMut<Hints>,
) {
    let asked = keyboard.global('H') || pads.pressed(crate::app::gamepad::scheme::Does::Hint);
    if !asked || !hints.offered() {
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
    // ghost sits at a tile centre, which only the size moves. Redrawing on
    // every change to the sim tore the sprite down and put it back thirty
    // times a second. Every level load writes `hints`, so a swapped board
    // of the same size is caught there.
    let size = (sim.0.width(), sim.0.height());
    if !hints.is_changed() && !(sim.is_changed() && *drawn_on != size) {
        return;
    }
    *drawn_on = size;
    for ghost in &ghosts {
        commands.entity(ghost).despawn();
    }
    let Some((x, y, dir, seat)) = hints.shown else {
        return;
    };
    let pos = layout::tile_center(&sim.0, x, y);
    // The signpost that goes here, as a ghost: the board's outline faint
    // and its arrow a little less so, in no seat's colour, except on a
    // co-op stage, where whose hand places it is half the answer.
    let paint = match sim.0.rules().claw_hands {
        Some(_) => palette::seat_paint(seat).with_alpha(0.6),
        None => palette::PARCHMENT.with_alpha(0.45),
    };
    let size = Some(Vec2::splat(layout::SIGN_SIZE));
    commands
        .spawn((
            HintGhost,
            Sprite {
                image: art.sign_shape.clone(),
                color: palette::PARCHMENT.with_alpha(0.2),
                custom_size: size,
                ..default()
            },
            Transform::from_translation(pos.extend(layout::z::SIGNPOST - 0.1))
                .with_rotation(layout::dir_rotation(dir)),
        ))
        .with_child((
            Sprite {
                image: art.sign_paint.clone(),
                color: paint,
                custom_size: size,
                ..default()
            },
            Transform::from_translation(Vec3::new(0.0, 0.0, 0.01)),
        ));
}

pub fn clear_hint_ghosts(mut commands: Commands, ghosts: Query<Entity, With<HintGhost>>) {
    for ghost in &ghosts {
        commands.entity(ghost).despawn();
    }
}

/// The line under the header while the hint is available or showing.
/// `pad` is the pad button that asks for it, for a pad's player, who is
/// told that button rather than the key.
pub fn hint_line(
    tr: &crate::app::i18n::Tr,
    hints: &Hints,
    denied: &DeniedNote,
    phase: &Phase,
    pad: Option<&str>,
) -> Option<String> {
    // The denial answers the key that was just pressed, so it outranks both
    // the stuck-hint and the level's lesson for as long as it lasts.
    if denied.0 > 0.0 {
        return Some(tr.denied_no_posts.to_string());
    }
    if !hints.offered() || !matches!(phase, Phase::Setup | Phase::Lost) {
        return None;
    }
    Some(match (pad, hints.showing()) {
        (Some(button), true) => fill(tr.pad.hint_showing, &[("b", button)]),
        (Some(button), false) => fill(tr.pad.hint_offer, &[("b", button)]),
        (None, true) => tr.hint_showing.to_string(),
        (None, false) => tr.hint_offer.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Level;
    use bevy::ecs::system::RunSystemOnce;

    fn campaign_at(index: usize) -> Campaign {
        Campaign {
            index,
            ..Campaign::tide_pool()
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

        let (x, y, dir, _) = level.solution[0];
        assert!(sim.0.place_signpost(0, x, y, dir));
        assert_eq!(
            next_step(&campaign, &sim),
            Some(level.solution[1]),
            "the first step is done, so show the second"
        );

        let (x, y, dir, _) = level.solution[1];
        assert!(sim.0.place_signpost(0, x, y, dir));
        assert_eq!(next_step(&campaign, &sim), None, "nothing left to show");

        // A post pointing the wrong way is not the step.
        let mut wrong = Sim(level.board());
        let (x, y, dir, _) = level.solution[0];
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
        app.init_resource::<crate::app::gamepad::PadFace>();
        app.init_resource::<crate::app::gamepad::LastDevice>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<crate::app::cursor::LiveSeatKeys>();
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
        let (x, y, dir, _) = level.solution[0];
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
    /// Both kinds of no fire the same flash and the same knock, so without
    /// this line "you have none left" and "not on that tile" are
    /// indistinguishable.
    #[test]
    fn a_spent_inventory_gets_a_sentence_of_its_own() {
        use crate::app::i18n::EN;
        let hints = Hints::default();
        assert!(!hints.offered(), "not stuck: no line of its own to compete");

        assert_eq!(
            hint_line(&EN, &hints, &DeniedNote(0.0), &Phase::Setup, None),
            None
        );
        assert_eq!(
            hint_line(&EN, &hints, &DeniedNote(1.0), &Phase::Setup, None).as_deref(),
            Some(EN.denied_no_posts),
        );
        // It outranks the stuck-hint, which is about the level rather than
        // about the key just pressed.
        let stuck = Hints {
            losses: STUCK_AFTER,
            answerable: true,
            ..Default::default()
        };
        assert_eq!(
            hint_line(&EN, &stuck, &DeniedNote(0.0), &Phase::Setup, None).as_deref(),
            Some(EN.hint_offer),
        );
        assert_eq!(
            hint_line(&EN, &stuck, &DeniedNote(1.0), &Phase::Setup, None).as_deref(),
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

        // P2's hand on a co-op stage, which places for seat 1.
        app.insert_resource(DeniedNote(0.0));
        app.world_mut()
            .resource_mut::<Messages<PlacementDenied>>()
            .clear();
        app.world_mut().write_message(PlacementDenied {
            player: 1,
            out_of_signposts: true,
        });
        let _ = app.world_mut().run_system_once(note_denials);
        assert!(
            app.world().resource::<DeniedNote>().0 > 0.0,
            "P2's spent hand"
        );
    }

    /// Losses only count toward the level they happened on.
    #[test]
    fn switching_levels_starts_the_count_over() {
        let mut hints = Hints {
            answerable: true,
            ..Hints::default()
        };
        for _ in 0..STUCK_AFTER {
            hints.level = Some(4);
            hints.losses += 1;
        }
        assert!(hints.offered());
        hints.reset(&campaign_at(5));
        assert!(!hints.offered(), "a new level is not a stuck one");
    }

    /// A level with no solution to take a step from (the player's own, or
    /// a pasted one) never offers H, however stuck: the key has nothing
    /// to show there.
    #[test]
    fn no_solution_no_offer() {
        // Stage six: the earlier ones with no arrows to place carry none.
        let mut campaign = campaign_at(5);
        let mut hints = Hints::default();
        hints.reset(&campaign);
        hints.losses = STUCK_AFTER;
        assert!(hints.offered(), "a shipped stage has its answer");
        let index = campaign.index;
        campaign.levels[index].solution.clear();
        hints.reset(&campaign);
        hints.losses = STUCK_AFTER;
        assert!(!hints.offered());
    }

    /// Hints for the level `campaign` is on, stuck on it and showing a step.
    fn stuck_and_showing(campaign: &Campaign) -> Hints {
        Hints {
            level: Some(campaign.index),
            losses: STUCK_AFTER,
            answerable: true,
            shown: Some((1, 1, crate::sim::Direction::Up, 0)),
        }
    }

    /// Once a step is on the board the line says how to read it, and the
    /// line keeps quiet while the run plays out or after it is won: the
    /// hint is for a board that is still being set up, or just lost.
    #[test]
    fn the_hint_line_says_what_is_shown_and_only_where_it_helps() {
        use crate::app::i18n::EN;
        let hints = stuck_and_showing(&campaign_at(3));
        let quiet = DeniedNote(0.0);
        assert_eq!(
            hint_line(&EN, &hints, &quiet, &Phase::Setup, None).as_deref(),
            Some(EN.hint_showing)
        );
        assert_eq!(
            hint_line(&EN, &hints, &quiet, &Phase::Lost, None).as_deref(),
            Some(EN.hint_showing)
        );
        for phase in [Phase::Running, Phase::Won] {
            assert_eq!(
                hint_line(&EN, &hints, &quiet, &phase, None),
                None,
                "{phase:?}"
            );
        }
    }

    /// A retry keeps the posts and the step shown from them; going back to
    /// the same level with the posts cleared takes the step off but keeps
    /// the count, so the offer stands; a different level starts clean.
    #[test]
    fn a_reload_keeps_the_count_and_a_new_level_clears_it() {
        // Level 28 has two steps. The retry lays the first back down (the
        // arrows the run began with), so the step kept is the second,
        // whatever the lost board was shown.
        let campaign = campaign_at(27);
        let solution = campaign.current().solution.clone();
        let mut retried = campaign.current().board();
        let (x, y, dir, _) = solution[0];
        assert!(retried.place_signpost(0, x, y, dir));
        let mut app = App::new();
        let mut hints = stuck_and_showing(&campaign);
        hints.shown = Some(solution[0]);
        app.insert_resource(hints);
        app.insert_resource(campaign);
        app.insert_resource(Sim(retried));
        app.add_message::<crate::app::LoadLevel>();
        app.add_systems(Update, reset_on_level);
        app.world_mut()
            .write_message(crate::app::LoadLevel { keep_posts: true });
        app.update();
        assert_eq!(
            app.world().resource::<Hints>().shown,
            Some(solution[1]),
            "a retry keeps the hint, worked out on the board it laid"
        );
        app.world_mut()
            .write_message(crate::app::LoadLevel { keep_posts: false });
        app.update();
        let hints = app.world().resource::<Hints>();
        assert!(!hints.showing(), "the step is taken off");
        assert!(hints.offered(), "and the count stands");
        app.world_mut().resource_mut::<Campaign>().index = 28;
        app.update();
        let hints = app.world().resource::<Hints>();
        assert!(!hints.offered(), "a new level starts from nothing");
    }

    /// Coming back in at the same index is no proof of the same level (a
    /// pasted code, a shelf another level sorted into): entering the
    /// screen starts the count over, and the load reads the level's own
    /// solution for whether a hint can be given.
    #[test]
    fn entering_the_puzzle_counts_afresh_at_the_same_index() {
        let campaign = campaign_at(27);
        let mut app = App::new();
        app.insert_resource(stuck_and_showing(&campaign));
        app.insert_resource(Sim(campaign.current().board()));
        app.insert_resource(campaign);
        app.add_message::<crate::app::LoadLevel>();
        app.add_systems(Update, reset_on_level);
        app.world_mut()
            .run_system_once(reset_on_entry)
            .expect("entered");
        app.world_mut()
            .write_message(crate::app::LoadLevel { keep_posts: false });
        app.update();
        let hints = app.world().resource::<Hints>();
        assert!(!hints.offered() && !hints.showing(), "nothing carried in");
        assert_eq!(hints.level, Some(27));
        assert!(hints.answerable, "read from the level the load built");
    }

    /// The denial fades with time, and goes at once when the level changes:
    /// it was about a board that is no longer there.
    #[test]
    fn the_denial_fades_and_goes_with_the_level() {
        let mut app = App::new();
        app.insert_resource(campaign_at(3));
        app.init_resource::<Time>();
        app.insert_resource(DeniedNote(2.0));
        app.add_systems(Update, tick_denied_note);
        // The first frame sees the campaign as new, which clears it; set it
        // again to watch it fade.
        app.update();
        app.insert_resource(DeniedNote(2.0));
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(500));
        app.update();
        let left = app.world().resource::<DeniedNote>().0;
        assert!((left - 1.5).abs() < 1e-3, "{left}");
        app.world_mut().resource_mut::<Campaign>().index = 5;
        app.update();
        assert_eq!(app.world().resource::<DeniedNote>().0, 0.0);
    }
}
