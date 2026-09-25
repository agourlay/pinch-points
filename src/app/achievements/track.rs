//! Gameplay tracking: fold the SimEvent stream and round/puzzle
//! outcomes into the lifetime stats, and pop unlock toasts.

use super::save::save;
use super::ui::spawn_toast;
use super::{ACHIEVEMENTS, PuzzleAttempt, RoundScratch, Stats, Unlocked};
use crate::app::Bots;
use crate::app::audio::{Muted, Sounds, play_chime, sfx_gain};
use crate::app::net::Online;
use crate::app::settings::GameSettings;
use crate::app::sim_events::SimEvent;
use crate::sim::CrabKind;
use bevy::prelude::*;

/// The seat whose deeds count: the online session seat, else seat 0, and
/// never a bot seat.
fn local_seat(online: &Online, bots: &Bots) -> Option<u8> {
    // Watching someone else's match earns nothing.
    let seat = match &online.0 {
        Some(session) => session.session.seat()?,
        None => 0,
    };
    if bots.0[seat as usize].is_some() {
        None
    } else {
        Some(seat)
    }
}

/// What every system that can earn a trophy needs: the record it adds to,
/// the shelf of what is already earned, and what it takes to say so (the
/// words, the chime, and whether it may ring).
#[derive(bevy::ecs::system::SystemParam)]
pub struct Trophies<'w> {
    pub stats: ResMut<'w, Stats>,
    unlocked: ResMut<'w, Unlocked>,
    pub settings: Res<'w, GameSettings>,
    sounds: Option<Res<'w, Sounds>>,
    muted: Res<'w, Muted>,
}

impl Trophies<'_> {
    /// Unlock whatever the stats now earn, with a toast and a chime each,
    /// and save both: once, after the caller's own changes, because a save
    /// is a sync to disk on the frame thread.
    pub fn credit(&mut self, commands: &mut Commands) {
        unlock_new(
            commands,
            &self.stats,
            &mut self.unlocked,
            &self.settings,
            &self.muted,
            &self.sounds,
        );
        save(&self.stats, &self.unlocked);
    }
}

/// Fold the sim event stream into the lifetime stats.
pub fn track_events(
    mut commands: Commands,
    mut events: MessageReader<SimEvent>,
    online: Res<Online>,
    bots: Res<Bots>,
    mut trophies: Trophies,
    mut scratch: ResMut<RoundScratch>,
) {
    let Some(seat) = local_seat(&online, &bots) else {
        for _ in events.read() {}
        return;
    };
    let mut changed = false;
    for event in events.read() {
        match event {
            SimEvent::CrabBanked { owner, kind, .. } if *owner == seat => {
                trophies.stats.banked += 1;
                scratch.banked += 1;
                match kind {
                    CrabKind::Golden => trophies.stats.golden += 1,
                    CrabKind::Molting => trophies.stats.lures += 1,
                    CrabKind::Sparkling => trophies.stats.events += 1,
                    CrabKind::Giant => trophies.stats.giants += 1,
                    CrabKind::Common | CrabKind::Juvenile => {}
                }
                changed = true;
            }
            // The roulette trophy wants variety, so remember *which* events
            // have come up rather than how many. Any seat's sparkling crab
            // spins a wheel everyone plays under.
            SimEvent::TideEventFired { event } => {
                trophies.stats.events_seen |= 1 << event.index();
                changed = true;
            }
            SimEvent::CrabEaten { .. } => {
                trophies.stats.gulls_fed += 1;
                changed = true;
            }
            SimEvent::CastleRaided { owner, .. } if *owner == seat => {
                trophies.stats.raids_taken += 1;
                scratch.raids += 1;
                changed = true;
            }
            SimEvent::CrabBanked { .. }
            | SimEvent::CastleRaided { .. }
            | SimEvent::CrabSpawned { .. }
            | SimEvent::GullArrived
            | SimEvent::GullTookOff
            | SimEvent::GullLanded { .. }
            | SimEvent::SignpostPlaced { .. }
            | SimEvent::SignpostRemoved { .. }
            | SimEvent::SignpostEvicted { .. }
            | SimEvent::TierUp { .. }
            | SimEvent::SurgeStarted
            | SimEvent::RoundEnded => {}
        }
    }
    if changed {
        trophies.credit(&mut commands);
    }
}

/// What a finished round was worth to the local seat, and how it was won.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) struct RoundOutcome {
    pub won: bool,
    /// The round was played against a live opponent over the wire.
    pub online: bool,
    /// This round also decided a series, and the local seat took it.
    pub series: bool,
    /// Which chair the local player sat in, for the trophy that wants the
    /// whole table rather than four wins from the same seat.
    pub seat: u8,
}

/// Tally a finished round for the local seat and clear the round scratch.
///
/// A win with no raid taken is the Dry Castle trophy: the raid counter has
/// to be read before it is cleared, and cleared whatever the result. The
/// round's banked count is the same shape, read for the best-round record
/// and then reset either way.
fn credit_round(stats: &mut Stats, scratch: &mut RoundScratch, outcome: RoundOutcome) {
    stats.best_round = stats.best_round.max(scratch.banked);
    if outcome.won {
        stats.wins += 1;
        if scratch.raids == 0 {
            stats.dry_wins += 1;
        }
        if outcome.online {
            stats.online_wins += 1;
        }
        // One bit per chair. `checked_shl` rather than a shift: the seat
        // comes from a session the wire agreed on, and a byte only has
        // eight bits to give.
        stats.seats_won |= 1u8.checked_shl(u32::from(outcome.seat)).unwrap_or(0);
        if outcome.series {
            stats.series_wins += 1;
        }
    }
    *scratch = RoundScratch::default();
}

/// A versus round just ended: count it, the win, and the dry-castle win.
#[allow(clippy::too_many_arguments)]
pub fn record_round(
    mut commands: Commands,
    sim: Res<crate::app::Sim>,
    seats: Res<crate::app::Seats>,
    online: Res<Online>,
    bots: Res<Bots>,
    daily: Res<crate::app::Daily>,
    tournament: Res<crate::app::tournament::Tournament>,
    mut trophies: Trophies,
    mut scratch: ResMut<RoundScratch>,
) {
    let Some(seat) = local_seat(&online, &bots) else {
        return;
    };
    trophies.stats.rounds += 1;
    // The busiest table this seat has ever sat at. Seats, not peers: an AI
    // seat still fills a chair and still has a castle to raid.
    trophies.stats.crowd = trophies.stats.crowd.max(u32::from(seats.0));
    if online
        .0
        .as_ref()
        .is_some_and(crate::app::net::OnlineSession::is_host)
    {
        trophies.stats.hosted += 1;
    }
    if daily.active {
        let today = crate::app::Daily::today();
        if trophies.stats.daily_day != today {
            // A day's daily played for the first time: today's best starts
            // over, and the habit counter ticks.
            trophies.stats.daily_day = today;
            trophies.stats.daily_best = 0;
            trophies.stats.daily_days += 1;
        }
        trophies.stats.daily_best = trophies.stats.daily_best.max(sim.0.scores()[seat as usize]);
        // And the all-time mark, which is what a trophy can hang on:
        // `daily_best` starts over at midnight and would take the trophy's
        // progress bar back down with it.
        trophies.stats.daily_record = trophies.stats.daily_record.max(trophies.stats.daily_best);
    }
    let mode = crate::app::teams::in_play(&trophies.settings, &online, seats.0);
    let winners = crate::app::side_panels::leading_seats(sim.0.scores(), seats.0, mode);
    let won = winners[seat as usize];
    credit_round(
        &mut trophies.stats,
        &mut scratch,
        RoundOutcome {
            won,
            online: online.0.is_some(),
            // `record_series_round` runs first, so the series verdict is in.
            // Asked by mode, because a series won in teams is won by every
            // seat on the team - and a per-seat search would find nobody.
            series: tournament.is_decided()
                && tournament
                    .winner(mode, seats.0)
                    .is_some_and(|champion| champion.claims(seat, mode)),
            seat,
        },
    );
    trophies.credit(&mut commands);
}

/// Persist on leaving a play screen so a mid-round quit loses nothing.
pub fn save_now(stats: Res<Stats>, unlocked: Res<Unlocked>) {
    save(&stats, &unlocked);
}

/// A level was saved out of the editor: one trophy for building a beach of
/// your own. Its own system rather than a line in the editor, so the editor
/// stays ignorant of achievements.
pub fn record_level_built(
    mut commands: Commands,
    mut saved: MessageReader<crate::app::LevelSaved>,
    mut trophies: Trophies,
) {
    let built = saved.read().count() as u32;
    if built == 0 {
        return;
    }
    trophies.stats.levels_built += built;
    trophies.credit(&mut commands);
}

/// Fresh round: the scratch starts over.
pub fn reset_round_scratch(mut scratch: ResMut<RoundScratch>) {
    *scratch = RoundScratch::default();
}

/// A puzzle was solved.
///
/// Ordered after `progress::record_cleared` so the stage just finished is
/// already counted: the last stage of a campaign is the one this would
/// otherwise miss, and the one the trophy is for. Only the built-in stages
/// count, so saving a level in the editor does not unfinish the campaign.
pub fn record_puzzle(
    mut commands: Commands,
    progress: Res<crate::app::progress::Progress>,
    campaign: Res<crate::app::Campaign>,
    sim: Res<crate::app::Sim>,
    attempt: Res<PuzzleAttempt>,
    mut trophies: Trophies,
) {
    // A solve is a solve, and the trophies over this one say "solve 100
    // puzzles": re-solving a stage is one of those.
    trophies.stats.puzzles += 1;
    // How it was cleared, not merely that it was, and only on a stage that
    // was still unbeaten when the attempt began: these three say "stages",
    // and a stage already in the book is not another one. Read before the
    // campaign sweep below, which is about the whole list rather than this
    // stage.
    let level = campaign.current();
    if attempt.unbeaten {
        if sim.0.signpost_count(0) < usize::from(level.posts) {
            trophies.stats.under_par += 1;
        }
        if level.posts >= DEEP_POSTS {
            trophies.stats.deep_solves += 1;
        }
        if attempt.retries == 0 {
            trophies.stats.clean_solves += 1;
        }
    }
    let builtins = &campaign.levels[..campaign.builtins.min(campaign.levels.len())];
    if !builtins.is_empty()
        && builtins
            .iter()
            .all(|level| progress.is_cleared(campaign.kind, &level.name))
    {
        match campaign.kind {
            crate::app::CampaignKind::TidePool => trophies.stats.campaign_done = 1,
            crate::app::CampaignKind::BeachDay => trophies.stats.beach_done = 1,
        }
    }
    trophies.credit(&mut commands);
}

/// The signpost grant that marks the deep end of the campaign. The late
/// stages hand out five; nothing earlier does.
const DEEP_POSTS: u8 = 5;

/// Count the reloads of the stage in play, so the first-try trophy can tell
/// a clean solve from a solve on the ninth go.
///
/// The first load of a stage is not a retry, which is what the name check
/// is for: entering a stage sends the same message that restarting it does,
/// and only the stage it names tells them apart.
pub fn track_puzzle_attempt(
    mut loads: MessageReader<crate::app::LoadLevel>,
    campaign: Res<crate::app::Campaign>,
    progress: Res<crate::app::progress::Progress>,
    mut attempt: ResMut<PuzzleAttempt>,
) {
    let mut loads = loads.read().count() as u32;
    if loads == 0 {
        return;
    }
    let name = &campaign.current().name;
    if attempt.stage != *name {
        // Read here rather than at the clear, which is the moment it stops
        // being true: the stage trophies ask what this attempt walked up
        // to, not what it left behind.
        attempt.unbeaten = !progress.is_cleared(campaign.kind, name);
        attempt.stage = name.clone();
        attempt.retries = 0;
        loads -= 1;
    }
    attempt.retries += loads;
}

/// Entering the puzzle screen starts a fresh attempt: coming back to a
/// stage later is a new go at it, not a continuation of the last one.
pub fn reset_puzzle_attempt(mut attempt: ResMut<PuzzleAttempt>) {
    *attempt = PuzzleAttempt::default();
}

/// A level went out as a share code, or came in as one. Its own system for
/// the reason [`record_level_built`] is one: the editor writes a message
/// and knows nothing about what reads it.
pub fn record_codes(
    mut commands: Commands,
    mut shared: MessageReader<crate::app::CodeShared>,
    mut taken: MessageReader<crate::app::CodeTaken>,
    mut trophies: Trophies,
) {
    let out = shared.read().count() as u32;
    let inn = taken.read().count() as u32;
    if out == 0 && inn == 0 {
        return;
    }
    trophies.stats.codes_shared += out;
    trophies.stats.codes_taken += inn;
    trophies.credit(&mut commands);
}

fn unlock_new(
    commands: &mut Commands,
    stats: &Stats,
    unlocked: &mut Unlocked,
    settings: &GameSettings,
    muted: &Muted,
    sounds: &Option<Res<Sounds>>,
) {
    let tr = settings.tr();
    for (index, achievement) in ACHIEVEMENTS.iter().enumerate() {
        // `insert` says whether the id was new, so no `contains` before
        // it. The threshold goes first: it is a compare, cheaper than
        // hashing the id, and it keeps the set out of it entirely.
        if !achievement.met(stats) || !unlocked.0.insert(achievement.id) {
            continue;
        }
        spawn_toast(commands, tr.ach_names[index], tr.ach_descs[index]);
        if let Some(sounds) = sounds {
            play_chime(commands, sounds, sfx_gain(settings, muted));
        }
    }
    // Saved by the caller, once: every one of them writes the stats it just
    // changed anyway, and a save is a sync to disk on the frame thread.
    // Saving here as well cost two per unlock and three for a double.
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    fn win() -> RoundOutcome {
        RoundOutcome {
            won: true,
            ..RoundOutcome::default()
        }
    }

    fn scratch(raids: u32, banked: u32) -> RoundScratch {
        RoundScratch { raids, banked }
    }

    /// Winning untouched earns the dry-castle trophy; winning after a raid
    /// does not; and either way the round's raid count starts over.
    #[test]
    fn a_dry_win_needs_an_unraided_castle() {
        let mut stats = Stats::default();
        let mut round = RoundScratch::default();
        credit_round(&mut stats, &mut round, win());
        assert_eq!((stats.wins, stats.dry_wins), (1, 1));

        let mut round = scratch(2, 0);
        credit_round(&mut stats, &mut round, win());
        assert_eq!((stats.wins, stats.dry_wins), (2, 1), "raided: no trophy");
        assert_eq!(round.raids, 0, "the scratch resets");

        // A loss counts neither, and still clears the scratch.
        let mut round = scratch(5, 0);
        credit_round(&mut stats, &mut round, RoundOutcome::default());
        assert_eq!((stats.wins, stats.dry_wins), (2, 1));
        assert_eq!(round.raids, 0);
    }

    /// The best-round record is the high-water mark of a scratch counter,
    /// so it has to be read before the reset and survive a losing round.
    #[test]
    fn the_best_round_is_a_high_water_mark() {
        let mut stats = Stats::default();
        let mut round = scratch(0, 30);
        credit_round(&mut stats, &mut round, RoundOutcome::default());
        assert_eq!(stats.best_round, 30, "a losing round still counts");
        assert_eq!(round.banked, 0);

        credit_round(&mut stats, &mut scratch(0, 12), win());
        assert_eq!(stats.best_round, 30, "a worse round does not lower it");

        credit_round(&mut stats, &mut scratch(0, 51), win());
        assert_eq!(stats.best_round, 51);
    }

    /// Two trophies count stages and say so: "clear 10 stages with one to
    /// spare", "clear 10 stages first try". They used to count clears, so
    /// ten goes at stage one were ten stages, and both fell to the easiest
    /// stage in the campaign played over and over.
    #[test]
    fn a_stage_already_beaten_is_not_another_stage() {
        use crate::app::progress::Progress;
        use crate::app::{Campaign, CampaignKind, LoadLevel};
        let mut levels = crate::sim::campaign_levels();
        levels.truncate(2);
        let builtins = levels.len();
        let first = levels[0].name.clone();
        let mut app = App::new();
        app.add_message::<LoadLevel>();
        app.init_resource::<PuzzleAttempt>();
        app.init_resource::<Progress>();
        app.insert_resource(Campaign {
            kind: CampaignKind::TidePool,
            levels,
            index: 0,
            builtins,
        });
        app.add_systems(Update, track_puzzle_attempt);
        let load = |app: &mut App| {
            app.world_mut()
                .write_message(LoadLevel { keep_posts: false });
            app.update();
        };
        let unbeaten = |app: &App| app.world().resource::<PuzzleAttempt>().unbeaten;

        load(&mut app);
        assert!(unbeaten(&app), "nothing cleared yet");

        // Clear it, leave, and come back: the same stage, no longer a new one.
        app.world_mut()
            .resource_mut::<Progress>()
            .mark(CampaignKind::TidePool, &first);
        let _ = app.world_mut().run_system_once(reset_puzzle_attempt);
        load(&mut app);
        assert!(!unbeaten(&app), "a stage already in the book");

        // The one behind it is still its own stage.
        app.world_mut().resource_mut::<Campaign>().index = 1;
        load(&mut app);
        assert!(unbeaten(&app), "a stage never played");
    }

    /// Entering a stage sends the same message that restarting it does, so
    /// the first-try trophy hangs entirely on telling them apart. Backwards
    /// either way it is worthless: it would go to everybody, or to nobody.
    #[test]
    fn the_first_load_of_a_stage_is_not_a_retry() {
        use crate::app::{Campaign, CampaignKind, LoadLevel};
        let mut levels = crate::sim::campaign_levels();
        levels.truncate(2);
        let builtins = levels.len();
        let mut app = App::new();
        app.add_message::<LoadLevel>();
        app.init_resource::<PuzzleAttempt>();
        app.init_resource::<crate::app::progress::Progress>();
        app.insert_resource(Campaign {
            kind: CampaignKind::TidePool,
            levels,
            index: 0,
            builtins,
        });
        app.add_systems(Update, track_puzzle_attempt);
        let load = |app: &mut App| {
            app.world_mut()
                .write_message(LoadLevel { keep_posts: false });
            app.update();
        };
        let retries = |app: &App| app.world().resource::<PuzzleAttempt>().retries;

        load(&mut app);
        assert_eq!(retries(&app), 0, "arriving at a stage is not a retry");
        load(&mut app);
        load(&mut app);
        assert_eq!(retries(&app), 2, "two restarts");

        // The next stage is a fresh attempt, not a continuation.
        app.world_mut().resource_mut::<Campaign>().index = 1;
        load(&mut app);
        assert_eq!(retries(&app), 0);
        load(&mut app);
        assert_eq!(retries(&app), 1);

        // And coming back to a stage already played starts over, which is
        // what entering the screen clears the name for.
        let _ = app.world_mut().run_system_once(reset_puzzle_attempt);
        app.world_mut().resource_mut::<Campaign>().index = 0;
        load(&mut app);
        assert_eq!(retries(&app), 0, "a second visit is a fresh attempt");
    }

    /// The whole-table trophy wants four different chairs, so four wins
    /// from seat 0 light one bit and losing a seat lights none.
    #[test]
    fn the_seat_bitmask_counts_chairs_not_wins() {
        let mut stats = Stats::default();
        for _ in 0..4 {
            credit_round(&mut stats, &mut RoundScratch::default(), win());
        }
        assert_eq!(stats.wins, 4);
        assert_eq!(stats.seats_won, 0b0001, "four wins, one chair");

        for seat in 1..4 {
            credit_round(
                &mut stats,
                &mut RoundScratch::default(),
                RoundOutcome { seat, ..win() },
            );
        }
        assert_eq!(stats.seats_won.count_ones(), 4, "four of the six chairs");

        // A loss lights nothing, whatever chair it was in.
        credit_round(
            &mut stats,
            &mut RoundScratch::default(),
            RoundOutcome {
                seat: 5,
                ..RoundOutcome::default()
            },
        );
        assert_eq!(stats.seats_won.count_ones(), 4);
    }

    /// Online and series wins only count when the round was won, and only
    /// under the circumstances that earned them.
    #[test]
    fn online_and_series_wins_need_the_win() {
        let mut stats = Stats::default();
        // Losing an online series decider earns nothing.
        credit_round(
            &mut stats,
            &mut RoundScratch::default(),
            RoundOutcome {
                won: false,
                online: true,
                series: true,
                seat: 0,
            },
        );
        assert_eq!((stats.online_wins, stats.series_wins), (0, 0));

        credit_round(
            &mut stats,
            &mut RoundScratch::default(),
            RoundOutcome {
                won: true,
                online: true,
                series: true,
                seat: 0,
            },
        );
        assert_eq!((stats.online_wins, stats.series_wins), (1, 1));

        // A plain local win moves neither.
        credit_round(&mut stats, &mut RoundScratch::default(), win());
        assert_eq!((stats.online_wins, stats.series_wins), (1, 1));
        assert_eq!(stats.wins, 2);
    }
}
