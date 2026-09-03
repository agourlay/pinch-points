//! Which stages the player has cleared, and therefore which are still
//! locked. The original's stage list only opened the next stage once you
//! beat the one before it.
//!
//! Progress is keyed by campaign kind and level *name*, not by index, so
//! inserting a level into the middle of the campaign does not silently
//! re-lock everything after it. Names are therefore unique on the list:
//! a player's level that shares a shipped one's name is renamed as it is
//! loaded (see [`crate::app::campaign::disambiguate`]), or the two would
//! share one tick.

use crate::app::{Campaign, CampaignKind};
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

/// The cleared-stage record, under the XDG data directory.
fn save_path() -> std::path::PathBuf {
    crate::app::paths::data_dir().join("progress.txt")
}

/// Kept as one set of names per list rather than one set of `kind:name`
/// keys: the stage list asks after every tile every frame, and a key
/// built for each lookup was a hundred-odd allocations a frame on a
/// screen where nothing moves.
#[derive(Resource, Default)]
pub struct Progress {
    cleared: HashMap<CampaignKind, HashSet<String>>,
}

impl Progress {
    pub fn is_cleared(&self, kind: CampaignKind, name: &str) -> bool {
        self.cleared
            .get(&kind)
            .is_some_and(|names| names.contains(name))
    }

    /// Records a clear; true when this was the first time.
    pub fn mark(&mut self, kind: CampaignKind, name: &str) -> bool {
        self.cleared
            .entry(kind)
            .or_default()
            .insert(name.to_string())
    }

    /// How many stages are cleared, over both lists.
    pub fn cleared_count(&self) -> usize {
        self.cleared.values().map(HashSet::len).sum()
    }

    /// Forget every cleared stage, both lists at once: they share this one
    /// record, and half a reset would leave Beach Day open on a beach the
    /// player just asked to start over.
    pub fn clear_all(&mut self) {
        self.cleared.clear();
    }

    /// The classic rule: the first stage is always open, so is any stage
    /// you have already cleared, so is the one right after a cleared stage.
    /// Player-made levels sit past the built-ins and are never locked:
    /// they are yours, and the editor plays them anyway.
    pub fn unlocked(&self, campaign: &Campaign, index: usize) -> bool {
        if index == 0 || index >= campaign.builtins {
            return true;
        }
        let cleared = |at: usize| {
            campaign
                .levels
                .get(at)
                .is_some_and(|level| self.is_cleared(campaign.kind, &level.name))
        };
        cleared(index) || cleared(index - 1)
    }

    /// How many of this list's stages are cleared, over one shelf of it.
    /// The stage select counts the shipped campaign and the player's own
    /// levels separately: they are two lists on one screen, and one bar
    /// over both of them measured nothing in particular.
    pub fn cleared_in(&self, campaign: &Campaign, range: std::ops::Range<usize>) -> usize {
        campaign
            .levels
            .get(range)
            .unwrap_or_default()
            .iter()
            .filter(|level| self.is_cleared(campaign.kind, &level.name))
            .count()
    }

    /// The furthest stage that is open to play, for resuming the list.
    pub fn furthest_open(&self, campaign: &Campaign) -> usize {
        (0..campaign.levels.len())
            .filter(|&index| self.unlocked(campaign, index))
            .take_while(|&index| index < campaign.builtins)
            .last()
            .unwrap_or(0)
    }

    pub fn to_text(&self) -> String {
        // Destructured with no rest pattern: a new field refuses to build
        // here until it is written out. `parse` is lenient and would never
        // notice one going unsaved.
        let Self { cleared } = self;
        let mut keys: Vec<String> = cleared
            .iter()
            .flat_map(|(kind, names)| {
                names
                    .iter()
                    .map(move |name| format!("{}:{name}", kind.key()))
            })
            .collect();
        keys.sort_unstable();
        keys.iter().fold(String::new(), |mut out, key| {
            out.push_str("cleared: ");
            out.push_str(key);
            out.push('\n');
            out
        })
    }

    /// Lenient parse, in the style of the other save files: anything that is
    /// not a `cleared:` line naming a list this build has is ignored rather
    /// than fatal.
    pub fn parse(text: &str) -> Progress {
        let mut progress = Progress::default();
        for line in text.lines() {
            if let Some((key, value)) = line.split_once(':')
                && key.trim() == "cleared"
                && let Some((kind, name)) = value.trim().split_once(':')
                && let Some(kind) = CampaignKind::from_key(kind)
                && !name.is_empty()
            {
                progress.mark(kind, name);
            }
        }
        progress
    }
}

pub fn load(mut commands: Commands) {
    commands.insert_resource(load_in(&save_path()));
}

/// The record read back from a file named outright; a missing or
/// unreadable one is a fresh start, as it is on a first run.
pub fn load_in(path: &std::path::Path) -> Progress {
    std::fs::read_to_string(path)
        .map(|text| Progress::parse(&text))
        .unwrap_or_default()
}

/// Write the record out. Best-effort, like the other save files: a disk that
/// will not take it is not worth interrupting play over.
pub fn save(progress: &Progress) {
    let _ = save_in(&save_path(), progress);
}

/// [`save`] to a file named outright, and with the error kept: the game
/// shrugs it off, a test wants to see it.
pub fn save_in(path: &std::path::Path, progress: &Progress) -> std::io::Result<()> {
    crate::app::paths::write_atomic(path, progress.to_text())
}

/// A cleared stage, saved the moment it is cleared: the stage list is the
/// player's record of the campaign, and a crash should not cost it.
pub fn record_cleared(campaign: Res<Campaign>, mut progress: ResMut<Progress>) {
    let name = campaign.current().name.clone();
    if progress.mark(campaign.kind, &name) {
        save(&progress);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Level, campaign_levels};

    fn campaign(builtins: usize, extra: Vec<Level>) -> Campaign {
        let mut levels = campaign_levels();
        levels.truncate(builtins);
        let builtins = levels.len();
        levels.extend(extra);
        Campaign {
            kind: CampaignKind::TidePool,
            levels,
            index: 0,
            builtins,
        }
    }

    /// One stage open at the start, and clearing a stage opens exactly one
    /// more: the ladder the original's stage list walks.
    #[test]
    fn clearing_a_stage_opens_the_next_one_only() {
        let campaign = campaign(5, vec![]);
        let mut progress = Progress::default();
        assert!(progress.unlocked(&campaign, 0));
        for index in 1..5 {
            assert!(!progress.unlocked(&campaign, index), "stage {index}");
        }

        let first = campaign.levels[0].name.clone();
        assert!(progress.mark(CampaignKind::TidePool, &first));
        assert!(!progress.mark(CampaignKind::TidePool, &first), "idempotent");
        assert!(progress.unlocked(&campaign, 1));
        assert!(!progress.unlocked(&campaign, 2), "no further than that");
        assert_eq!(progress.cleared_in(&campaign, 0..campaign.levels.len()), 1);
        assert_eq!(progress.furthest_open(&campaign), 1);
    }

    /// The two lists keep separate records, even for a shared level name.
    #[test]
    fn the_two_campaigns_do_not_share_progress() {
        let mut progress = Progress::default();
        progress.mark(CampaignKind::TidePool, "Gull Storm");
        assert!(progress.is_cleared(CampaignKind::TidePool, "Gull Storm"));
        assert!(!progress.is_cleared(CampaignKind::BeachDay, "Gull Storm"));
    }

    /// A level the player built is never locked behind the campaign.
    #[test]
    fn player_made_levels_are_always_open() {
        let mine =
            Level::parse(include_str!("../../levels/01_welcome_ashore.txt")).expect("parses");
        let campaign = campaign(5, vec![mine]);
        let progress = Progress::default();
        assert!(
            !progress.unlocked(&campaign, 4),
            "still behind the campaign"
        );
        assert!(progress.unlocked(&campaign, 5), "the player's own level");
    }

    #[test]
    fn progress_survives_a_save_and_load() {
        let mut progress = Progress::default();
        progress.mark(CampaignKind::TidePool, "Welcome Ashore");
        progress.mark(CampaignKind::BeachDay, "First Flood");
        let back = Progress::parse(&progress.to_text());
        assert!(back.is_cleared(CampaignKind::TidePool, "Welcome Ashore"));
        assert!(back.is_cleared(CampaignKind::BeachDay, "First Flood"));
        assert_eq!(back.cleared_count(), 2);
        // Junk lines are ignored, not fatal.
        let salvaged = Progress::parse("garbage\ncleared:\ncleared: tide:Welcome Ashore\n");
        assert!(salvaged.is_cleared(CampaignKind::TidePool, "Welcome Ashore"));
        assert_eq!(salvaged.cleared_count(), 1);
    }
}
