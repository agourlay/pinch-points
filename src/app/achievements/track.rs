//! Gameplay tracking: fold the SimEvent stream and round/puzzle
//! outcomes into the lifetime stats, and pop unlock toasts.

use super::save::save;
use super::ui::spawn_toast;
use super::{ACHIEVEMENTS, Stats, Unlocked};
use crate::app::Bots;
use crate::app::audio::{Sounds, play_chime};
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
    mut stats: ResMut<Stats>,
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
                stats.banked_this_round += 1;
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
                stats.raids_this_round += 1;
                changed = true;
            }
            SimEvent::CrabBanked { .. }
            | SimEvent::CastleRaided { .. }
            | SimEvent::CrabSpawned { .. }
            | SimEvent::GullArrived
            | SimEvent::GullTookOff
            | SimEvent::GullLanded { .. }
            | SimEvent::SignpostsChanged { .. }
            | SimEvent::SignpostEvicted { .. }
            | SimEvent::TierUp { .. }
            | SimEvent::SurgeStarted
            | SimEvent::RoundEnded => {}
        }
    }
    if changed {
        unlock_new(&mut commands, &stats, &mut unlocked, &settings, &sounds);
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
}

/// Tally a finished round for the local seat and clear the round scratch.
///
/// A win with no raid taken is the Dry Castle trophy, the rule here that is
/// easy to get subtly wrong, since the raid counter has to be read before
/// it is cleared and cleared whatever the result. The round's banked count is
/// the same shape: read for the best-round record, then reset either way.
fn credit_round(stats: &mut Stats, outcome: RoundOutcome) {
    stats.best_round = stats.best_round.max(stats.banked_this_round);
    if outcome.won {
        stats.wins += 1;
        if stats.raids_this_round == 0 {
            stats.dry_wins += 1;
        }
        if outcome.online {
            stats.online_wins += 1;
        }
        if outcome.series {
            stats.series_wins += 1;
        }
    }
    stats.raids_this_round = 0;
    stats.banked_this_round = 0;
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
    mut stats: ResMut<Stats>,
    mut unlocked: ResMut<Unlocked>,
) {
    let Some(seat) = local_seat(&online, &bots) else {
        return;
    };
    stats.rounds += 1;
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
    }
    let mode = crate::app::teams::in_play(&settings, &online, seats.0);
    let winners = crate::app::side_panels::leading_seats(sim.0.scores(), seats.0, mode);
    let won = winners[seat as usize];
    credit_round(
        &mut stats,
        RoundOutcome {
            won,
            online: online.0.is_some(),
            // `record_series_round` runs first, so the series verdict is in.
            // Asked by mode, because a series won in teams is won by every
            // seat on the team - and a per-seat search would find nobody.
            series: tournament.finished
                && tournament
                    .winner(mode, seats.0)
                    .is_some_and(|champion| champion.claims(seat, mode)),
        },
    );
    unlock_new(&mut commands, &stats, &mut unlocked, &settings, &sounds);
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
    mut stats: ResMut<Stats>,
    mut unlocked: ResMut<Unlocked>,
) {
    let built = saved.read().count() as u32;
    if built == 0 {
        return;
    }
    stats.levels_built += built;
    unlock_new(&mut commands, &stats, &mut unlocked, &settings, &sounds);
    save(&stats, &unlocked);
}

/// Fresh round: reset the round-scoped scratch.
pub fn reset_round_scratch(mut stats: ResMut<Stats>) {
    stats.raids_this_round = 0;
    stats.banked_this_round = 0;
}

/// A puzzle was solved.
///
/// Ordered after `progress::record_cleared` so the stage just finished is
/// already counted: the last stage of a campaign is exactly the one this
/// would otherwise miss, and it is the one the trophy is for. Only the
/// built-in stages count, so a player who saved a level in the editor has
/// not thereby unfinished the campaign.
pub fn record_puzzle(
    mut commands: Commands,
    settings: Res<GameSettings>,
    sounds: Option<Res<Sounds>>,
    progress: Res<crate::app::progress::Progress>,
    campaign: Res<crate::app::Campaign>,
    mut stats: ResMut<Stats>,
    mut unlocked: ResMut<Unlocked>,
) {
    stats.puzzles += 1;
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
    unlock_new(&mut commands, &stats, &mut unlocked, &settings, &sounds);
    save(&stats, &unlocked);
}

fn unlock_new(
    commands: &mut Commands,
    stats: &Stats,
    unlocked: &mut Unlocked,
    settings: &GameSettings,
    sounds: &Option<Res<Sounds>>,
) {
    let tr = settings.tr();
    for (index, achievement) in ACHIEVEMENTS.iter().enumerate() {
        if unlocked.0.contains(achievement.id) || !achievement.met(stats) {
            continue;
        }
        unlocked.0.insert(achievement.id);
        spawn_toast(commands, tr.ach_names[index], tr.ach_descs[index]);
        if let Some(sounds) = sounds {
            play_chime(commands, sounds, settings.sfx_gain());
        }
        save(stats, unlocked);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win() -> RoundOutcome {
        RoundOutcome {
            won: true,
            ..RoundOutcome::default()
        }
    }

    /// Winning untouched earns the dry-castle trophy; winning after a raid
    /// does not; and either way the round's raid count starts over.
    #[test]
    fn a_dry_win_needs_an_unraided_castle() {
        let mut stats = Stats::default();
        credit_round(&mut stats, win());
        assert_eq!((stats.wins, stats.dry_wins), (1, 1));

        stats.raids_this_round = 2;
        credit_round(&mut stats, win());
        assert_eq!((stats.wins, stats.dry_wins), (2, 1), "raided: no trophy");
        assert_eq!(stats.raids_this_round, 0, "the scratch resets");

        // A loss counts neither, and still clears the scratch.
        stats.raids_this_round = 5;
        credit_round(&mut stats, RoundOutcome::default());
        assert_eq!((stats.wins, stats.dry_wins), (2, 1));
        assert_eq!(stats.raids_this_round, 0);
    }

    /// The best-round record is the high-water mark of a scratch counter,
    /// so it has to be read before the reset and survive a losing round.
    #[test]
    fn the_best_round_is_a_high_water_mark() {
        let mut stats = Stats {
            banked_this_round: 30,
            ..Stats::default()
        };
        credit_round(&mut stats, RoundOutcome::default());
        assert_eq!(stats.best_round, 30, "a losing round still counts");
        assert_eq!(stats.banked_this_round, 0);

        stats.banked_this_round = 12;
        credit_round(&mut stats, win());
        assert_eq!(stats.best_round, 30, "a worse round does not lower it");

        stats.banked_this_round = 51;
        credit_round(&mut stats, win());
        assert_eq!(stats.best_round, 51);
    }

    /// Online and series wins only count when the round was won, and only
    /// under the circumstances that earned them.
    #[test]
    fn online_and_series_wins_need_the_win() {
        let mut stats = Stats::default();
        // Losing an online series decider earns nothing.
        credit_round(
            &mut stats,
            RoundOutcome {
                won: false,
                online: true,
                series: true,
            },
        );
        assert_eq!((stats.online_wins, stats.series_wins), (0, 0));

        credit_round(
            &mut stats,
            RoundOutcome {
                won: true,
                online: true,
                series: true,
            },
        );
        assert_eq!((stats.online_wins, stats.series_wins), (1, 1));

        // A plain local win moves neither.
        credit_round(&mut stats, win());
        assert_eq!((stats.online_wins, stats.series_wins), (1, 1));
        assert_eq!(stats.wins, 2);
    }
}
