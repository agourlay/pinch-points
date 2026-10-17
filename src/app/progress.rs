//! Which stages the player has cleared, and therefore which are still
//! locked. The original's stage list only opened the next stage once you
//! beat the one before it.
//!
//! Progress is keyed by campaign kind and level *name*, not by index, so
//! inserting a level into the middle of the campaign does not silently
//! re-lock everything after it. A player's own level off the shelf is
//! keyed by its file instead ([`crate::sim::Filing`]): two files may carry
//! one name, and the name the list shows each of them depends on which
//! sorts first (see [`crate::app::campaign::disambiguate`]).

use crate::app::{Campaign, CampaignKind};
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

/// The cleared-stage record, under the XDG data directory.
fn save_path() -> std::path::PathBuf {
    crate::app::paths::merged_record("progress.txt")
}

/// A campaign and how far up it the player has climbed: which stage is
/// current, and which are open to them.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Ladder<'w> {
    pub campaign: ResMut<'w, Campaign>,
    pub progress: Res<'w, Progress>,
}

impl Ladder<'_> {
    /// Make stage `at` the current one if it is open, answering whether it
    /// was. A locked stage leaves the campaign where it stood.
    pub fn climb_to(&mut self, at: usize) -> bool {
        let open = self.progress.unlocked(&self.campaign, at);
        if open {
            self.campaign.index = at;
        }
        open
    }
}

/// Kept as one set of names per list rather than one set of `kind:name`
/// keys: the stage list asks after every tile every frame, and a key built
/// per lookup is a hundred-odd allocations a frame.
#[derive(Resource, Default)]
pub struct Progress {
    cleared: HashMap<CampaignKind, HashSet<String>>,
    /// Lines this build cannot read, kept word for word and written back:
    /// a newer build's clears on a list this one lacks, or a line of a
    /// kind it has not heard of. Read and dropped, a player who opened an
    /// older build once lost them on its next save.
    foreign: crate::app::paths::ForeignLines,
}

impl Progress {
    pub fn is_cleared(&self, kind: CampaignKind, name: &str) -> bool {
        self.cleared
            .get(&kind)
            .is_some_and(|names| names.contains(name))
    }

    /// Whether `level` is cleared on `kind`'s list, asked by the key it is
    /// filed under ([`crate::sim::Level::progress_key`]): a player's level
    /// by its file, a shipped one by its name. Every reader of a stage's
    /// tick asks here, so none can ask by the name the list shows.
    pub fn level_cleared(&self, kind: CampaignKind, level: &crate::sim::Level) -> bool {
        self.is_cleared(kind, level.progress_key())
    }

    /// Records a clear of `level`, under the key [`Progress::level_cleared`]
    /// reads; true when this was the first time.
    pub fn mark_level(&mut self, kind: CampaignKind, level: &crate::sim::Level) -> bool {
        self.mark(kind, level.progress_key())
    }

    /// File the ticks an older build kept under a player level's shown
    /// name under its file instead, where the name can only have meant
    /// that level (see [`crate::app::campaign::disambiguate`]). True when
    /// anything moved, which is worth a save. The name's own line stays:
    /// it is what an older build still reads.
    pub fn adopt_legacy(&mut self, campaign: &Campaign) -> bool {
        let mut moved = false;
        for level in &campaign.levels {
            let Some(filing) = &level.filing else {
                continue;
            };
            if let Some(name) = &filing.legacy
                && self.is_cleared(campaign.kind, name)
            {
                moved |= self.mark(campaign.kind, &filing.key);
            }
        }
        moved
    }

    /// Records a clear; true when this was the first time.
    pub fn mark(&mut self, kind: CampaignKind, name: &str) -> bool {
        self.cleared
            .entry(kind)
            .or_default()
            .insert(name.to_string())
    }

    /// Take in another record's clears, and the lines it kept that this
    /// build cannot read: a union, since a stage once cleared stays
    /// cleared whichever copy of the game cleared it.
    fn merge(&mut self, other: Progress) {
        let Progress { cleared, foreign } = other;
        for (kind, names) in cleared {
            self.cleared.entry(kind).or_default().extend(names);
        }
        self.foreign.extend(foreign);
    }

    /// How many stages are cleared, over both lists.
    pub fn cleared_count(&self) -> usize {
        self.cleared.values().map(HashSet::len).sum()
    }

    /// Forget every cleared stage, both lists at once: they share this one
    /// record, and half a reset would leave Beach Day open on a beach the
    /// player just asked to start over.
    ///
    /// The lines this build cannot read go too: they are clears from a
    /// newer build's lists, and the player asked to start over.
    pub fn clear_all(&mut self) {
        self.cleared.clear();
        self.foreign.clear();
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
                .is_some_and(|level| self.level_cleared(campaign.kind, level))
        };
        cleared(index) || cleared(index - 1)
    }

    /// How many of this list's stages are cleared, over one shelf of it.
    /// The stage select counts the shipped campaign and the player's own
    /// levels separately: they are two lists on one screen.
    pub fn cleared_in(&self, campaign: &Campaign, range: std::ops::Range<usize>) -> usize {
        campaign
            .levels
            .get(range)
            .unwrap_or_default()
            .iter()
            .filter(|level| self.level_cleared(campaign.kind, level))
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
        let Self { cleared, foreign } = self;
        let mut keys: Vec<String> = cleared
            .iter()
            .flat_map(|(kind, names)| {
                names
                    .iter()
                    .map(move |name| format!("{}:{name}", kind.key()))
            })
            .collect();
        keys.sort_unstable();
        let mut out = keys.iter().fold(String::new(), |mut out, key| {
            out.push_str("cleared: ");
            out.push_str(key);
            out.push('\n');
            out
        });
        foreign.append_to(&mut out);
        out
    }

    /// Lenient parse, in the style of the other save files: nothing in the
    /// file is fatal. A `cleared:` line naming a list this build has is a
    /// clear; any other `key: value` line, a list from a newer build
    /// included, is kept as it stands for the next save to write back
    /// (see [`Progress::foreign`]). Only what is not a line of the file's
    /// shape at all, or a `cleared:` line naming nothing, is dropped.
    pub fn parse(text: &str) -> Progress {
        let mut progress = Progress::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            if key == "cleared" {
                // A `cleared:` line naming nothing is junk, not news.
                let Some((kind, name)) = value.split_once(':').filter(|(_, n)| !n.is_empty())
                else {
                    continue;
                };
                match CampaignKind::from_key(kind) {
                    Some(kind) => {
                        progress.mark(kind, name);
                    }
                    None => progress.foreign.keep(line),
                }
            } else if !key.is_empty() {
                progress.foreign.keep(line);
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
///
/// What is on disk now is read back in first (see [`save_in`]), and the
/// record in hand takes it in as well, so the stage list shows a clear
/// made by another copy of the game too.
pub fn save(progress: &mut Progress) {
    let _ = save_in(&save_path(), progress);
}

/// [`save`] to a file named outright, and with the error kept: the game
/// shrugs it off, a test wants to see it.
///
/// The record is read once at startup and written back whole, so with two
/// copies of the game open, each save wrote over the other's clears with
/// its own startup view. Merged with the file as it stands at the moment
/// of writing, a clear made anywhere stays made.
pub fn save_in(path: &std::path::Path, progress: &mut Progress) -> std::io::Result<()> {
    progress.merge(load_in(path));
    crate::app::paths::write_atomic(path, progress.to_text())
}

/// Write the record as it stands, over whatever is on disk: the reset,
/// which is the one change a merge would undo.
pub fn save_reset(progress: &Progress) {
    let _ = crate::app::paths::write_atomic(&save_path(), progress.to_text());
}

/// A cleared stage, saved the moment it is cleared: the stage list is the
/// player's record of the campaign, and a crash should not cost it.
pub fn record_cleared(campaign: Res<Campaign>, mut progress: ResMut<Progress>) {
    if progress.mark_level(campaign.kind, campaign.current()) {
        save(&mut progress);
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
            builtins,
            ..Campaign::tide_pool_of(levels)
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

    /// A record from a build with a list this one lacks keeps what it can
    /// read, and keeps what it cannot as well: the unknown list's clears
    /// are not this build's to show, but they are written back, so going
    /// back to the newer build finds them where it left them.
    #[test]
    fn a_list_this_build_lacks_is_kept_for_the_build_that_has_it() {
        let text = "cleared: reef:Far Shoal\ncleared: tide:Welcome Ashore\nstreak: 4\n";
        let mixed = Progress::parse(text);
        assert!(mixed.is_cleared(CampaignKind::TidePool, "Welcome Ashore"));
        assert_eq!(mixed.cleared_count(), 1, "the reef is not ours to count");
        assert_eq!(CampaignKind::from_key("reef"), None);
        assert_eq!(
            CampaignKind::from_key("beach"),
            Some(CampaignKind::BeachDay)
        );
        let back = mixed.to_text();
        assert!(back.contains("cleared: reef:Far Shoal\n"), "{back}");
        assert!(back.contains("streak: 4\n"), "{back}");
        assert_eq!(Progress::parse(&back).to_text(), back, "and stays put");
    }

    /// The file is written in one order whatever order the stages were
    /// cleared in, list first and then name, so two saves of the same
    /// record are the same bytes.
    #[test]
    fn the_record_is_written_in_one_order() {
        let mut progress = Progress::default();
        progress.mark(CampaignKind::TidePool, "Undertow");
        progress.mark(CampaignKind::TidePool, "Corner Pocket");
        progress.mark(CampaignKind::BeachDay, "First Flood");
        assert_eq!(
            progress.to_text(),
            "cleared: beach:First Flood\ncleared: tide:Corner Pocket\ncleared: tide:Undertow\n"
        );
        let mut reversed = Progress::default();
        reversed.mark(CampaignKind::BeachDay, "First Flood");
        reversed.mark(CampaignKind::TidePool, "Corner Pocket");
        reversed.mark(CampaignKind::TidePool, "Undertow");
        assert_eq!(reversed.to_text(), progress.to_text());
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

    /// The record goes to disk and comes back the same, and a file that is
    /// not there is a fresh start rather than a failure.
    #[test]
    fn the_record_survives_the_disk() {
        let dir = crate::app::paths::test_dir("progress");
        let path = dir.join("progress.txt");
        assert_eq!(load_in(&path).cleared_count(), 0, "nothing saved yet");
        let mut progress = Progress::default();
        progress.mark(CampaignKind::TidePool, "Welcome Ashore");
        save_in(&path, &mut progress).expect("saved");
        let back = load_in(&path);
        assert!(back.is_cleared(CampaignKind::TidePool, "Welcome Ashore"));
        assert_eq!(back.to_text(), progress.to_text());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Two copies of the game open at once each read the record at
    /// startup; a save from one does not take back a clear the other has
    /// written since, and the saver learns of it.
    #[test]
    fn a_save_keeps_a_clear_another_copy_wrote() {
        let dir = crate::app::paths::test_dir("progress-two");
        let path = dir.join("progress.txt");
        let mut first = load_in(&path);
        let mut second = load_in(&path);
        second.mark(CampaignKind::TidePool, "Undertow");
        save_in(&path, &mut second).expect("saved");
        first.mark(CampaignKind::BeachDay, "First Flood");
        save_in(&path, &mut first).expect("saved");
        let back = load_in(&path);
        assert!(back.is_cleared(CampaignKind::TidePool, "Undertow"), "kept");
        assert!(back.is_cleared(CampaignKind::BeachDay, "First Flood"));
        assert!(first.is_cleared(CampaignKind::TidePool, "Undertow"), "seen");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Clearing a stage marks the stage the campaign is on, in its own
    /// list, and only once however often the win is seen.
    #[test]
    fn a_cleared_stage_is_marked_where_the_campaign_stands() {
        let mut app = App::new();
        let mut on = campaign(10, Vec::new());
        on.index = 4;
        let name = on.current().name.clone();
        app.insert_resource(on);
        app.init_resource::<Progress>();
        app.add_systems(Update, record_cleared);
        app.update();
        app.update();
        let progress = app.world().resource::<Progress>();
        assert!(progress.is_cleared(CampaignKind::TidePool, &name));
        assert!(!progress.is_cleared(CampaignKind::BeachDay, &name));
        assert_eq!(progress.cleared_count(), 1);
    }
}
