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

/// Fold the sim event stream into the lifetime stats.
#[allow(clippy::too_many_arguments)]
pub fn track_events(
    mut commands: Commands,
    mut events: MessageReader<SimEvent>,
    online: Res<Online>,
    bots: Res<Bots>,
    settings: Res<GameSettings>,
    sounds: Option<Res<Sounds>>,
    muted: Res<Muted>,
    mut stats: ResMut<Stats>,
    mut scratch: ResMut<RoundScratch>,
    mut unlocked: ResMut<Unlocked>,
) {
    let Some(seat) = local_seat(&online, &bots) else {
        for _ in events.read() {}
        return;
    };
    let mut changed = false;
    for event in events.read() {
        match event {
            SimEvent::CrabBanked { owner, kind, .. } if *owner == seat => {
                stats.banked += 1;
                scratch.banked += 1;
                match kind {
                    CrabKind::Golden => stats.golden += 1,
                    CrabKind::Molting => stats.lures += 1,
                    CrabKind::Sparkling => stats.events += 1,
                    CrabKind::Giant => stats.giants += 1,
                    CrabKind::Common | CrabKind::Juvenile => {}
                }
                changed = true;
            }
            // The roulette trophy wants variety, so remember *which* events
            // have come up rather than how many. Any seat's sparkling crab
            // spins a wheel everyone plays under.
            SimEvent::TideEventFired { event } => {
                stats.events_seen |= 1 << event.index();
                changed = true;
            }
            SimEvent::CrabEaten { .. } => {
                stats.gulls_fed += 1;
                changed = true;
            }
            SimEvent::CastleRaided { owner, .. } if *owner == seat => {
                stats.raids_taken += 1;
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
        unlock_new(
            &mut commands,
            &stats,
            &mut unlocked,
            &settings,
            &muted,
            &sounds,
        );
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
/// A win with no raid taken is the Dry Castle trophy, the rule here that is
/// easy to get subtly wrong, since the raid counter has to be read before
/// it is cleared and cleared whatever the result. The round's banked count is
/// the same shape: read for the best-round record, then reset either way.
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
    settings: Res<GameSettings>,
    sounds: Option<Res<Sounds>>,
    muted: Res<Muted>,
    mut stats: ResMut<Stats>,
    mut scratch: ResMut<RoundScratch>,
    mut unlocked: ResMut<Unlocked>,
) {
    let Some(seat) = local_seat(&online, &bots) else {
        return;
    };
    stats.rounds += 1;
    // The busiest table this seat has ever sat at. Seats, not peers: an AI
    // seat still fills a chair and still has a castle to raid.
    stats.crowd = stats.crowd.max(u32::from(seats.0));
    if online
        .0
        .as_ref()
        .is_some_and(crate::app::net::OnlineSession::is_host)
    {
        stats.hosted += 1;
    }
    if daily.active {
        let today = crate::app::Daily::today();
        if stats.daily_day != today {
            // A day's daily played for the first time: today's best starts
            // over, and the habit counter ticks.
            stats.daily_day = today;
            stats.daily_best = 0;
            stats.daily_days += 1;
        }
        stats.daily_best = stats.daily_best.max(sim.0.scores()[seat as usize]);
        // And the all-time mark, which is what a trophy can hang on:
        // `daily_best` starts over at midnight and would take the trophy's
        // progress bar back down with it.
        stats.daily_record = stats.daily_record.max(stats.daily_best);
    }
    let mode = crate::app::teams::in_play(&settings, &online, seats.0);
    let winners = crate::app::side_panels::leading_seats(sim.0.scores(), seats.0, mode);
    let won = winners[seat as usize];
    credit_round(
        &mut stats,
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
    unlock_new(
        &mut commands,
        &stats,
        &mut unlocked,
        &settings,
        &muted,
        &sounds,
    );
    save(&stats, &unlocked);
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
    settings: Res<GameSettings>,
    sounds: Option<Res<Sounds>>,
    muted: Res<Muted>,
    mut stats: ResMut<Stats>,
    mut unlocked: ResMut<Unlocked>,
) {
    let built = saved.read().count() as u32;
    if built == 0 {
        return;
    }
    stats.levels_built += built;
    unlock_new(
        &mut commands,
        &stats,
        &mut unlocked,
        &settings,
        &muted,
        &sounds,
    );
    save(&stats, &unlocked);
}

/// Fresh round: the scratch starts over.
pub fn reset_round_scratch(mut scratch: ResMut<RoundScratch>) {
    *scratch = RoundScratch::default();
}

/// A puzzle was solved.
///
/// Ordered after `progress::record_cleared` so the stage just finished is
/// already counted: the last stage of a campaign is exactly the one this
/// would otherwise miss, and it is the one the trophy is for. Only the
/// built-in stages count, so a player who saved a level in the editor has
/// not thereby unfinished the campaign.
#[allow(clippy::too_many_arguments)]
pub fn record_puzzle(
    mut commands: Commands,
    settings: Res<GameSettings>,
    sounds: Option<Res<Sounds>>,
    muted: Res<Muted>,
    progress: Res<crate::app::progress::Progress>,
    campaign: Res<crate::app::Campaign>,
    sim: Res<crate::app::Sim>,
    attempt: Res<PuzzleAttempt>,
    mut stats: ResMut<Stats>,
    mut unlocked: ResMut<Unlocked>,
) {
    stats.puzzles += 1;
    // How it was cleared, not merely that it was. Read before the campaign
    // sweep below, which is about the whole list rather than this stage.
    let level = campaign.current();
    if sim.0.signpost_count(0) < usize::from(level.posts) {
        stats.under_par += 1;
    }
    if level.posts >= DEEP_POSTS {
        stats.deep_solves += 1;
    }
    if attempt.retries == 0 {
        stats.clean_solves += 1;
    }
    let builtins = &campaign.levels[..campaign.builtins.min(campaign.levels.len())];
    if !builtins.is_empty()
        && builtins
            .iter()
            .all(|level| progress.is_cleared(campaign.kind, &level.name))
    {
        match campaign.kind {
            crate::app::CampaignKind::TidePool => stats.campaign_done = 1,
            crate::app::CampaignKind::BeachDay => stats.beach_done = 1,
        }
    }
    unlock_new(
        &mut commands,
        &stats,
        &mut unlocked,
        &settings,
        &muted,
        &sounds,
    );
    save(&stats, &unlocked);
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
    mut attempt: ResMut<PuzzleAttempt>,
) {
    let mut loads = loads.read().count() as u32;
    if loads == 0 {
        return;
    }
    let name = &campaign.current().name;
    if attempt.stage != *name {
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
#[allow(clippy::too_many_arguments)]
pub fn record_codes(
    mut commands: Commands,
    mut shared: MessageReader<crate::app::CodeShared>,
    mut taken: MessageReader<crate::app::CodeTaken>,
    settings: Res<GameSettings>,
    sounds: Option<Res<Sounds>>,
    muted: Res<Muted>,
    mut stats: ResMut<Stats>,
    mut unlocked: ResMut<Unlocked>,
) {
    let out = shared.read().count() as u32;
    let inn = taken.read().count() as u32;
    if out == 0 && inn == 0 {
        return;
    }
    stats.codes_shared += out;
    stats.codes_taken += inn;
    unlock_new(
        &mut commands,
        &stats,
        &mut unlocked,
        &settings,
        &muted,
        &sounds,
    );
    save(&stats, &unlocked);
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
        // One hash per achievement: `insert` says whether the id was new,
        // so the `contains` that used to guard it is gone. Meeting the
        // threshold is a fn-pointer call and a compare, cheaper than
        // hashing the id, so it goes first and keeps the set untouched
        // for an achievement the player has not earned yet.
        if !achievement.met(stats) || !unlocked.0.insert(achievement.id) {
            continue;
        }
        spawn_toast(commands, tr.ach_names[index], tr.ach_descs[index]);
        if let Some(sounds) = sounds {
            play_chime(commands, sounds, sfx_gain(settings, muted));
        }
        save(stats, unlocked);
    }
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
        assert_eq!(stats.seats_won.count_ones(), 4, "the whole table");

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
