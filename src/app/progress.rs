//! Which stages the player has cleared, and therefore which are still
//! locked. The original's stage list only opened the next stage once you
//! beat the one before it.
//!
//! Progress is keyed by level *name*, not by index, so inserting a level
//! into the middle of the campaign does not silently re-lock everything
//! after it. A player's own level off the shelf is keyed by its file
//! instead ([`crate::sim::Filing`]): two files may carry one name, and the
//! name the list shows each of them depends on which sorts first (see
//! [`crate::app::campaign::disambiguate`]).

use crate::app::Campaign;
use bevy::prelude::*;
use std::collections::HashSet;

/// The list name a `cleared:` line files the Tide Pool's clears under
/// (`cleared: tide:<name>`). Lines naming any other list are an older or
/// newer build's, and kept for it as they stand ([`Progress::foreign`]).
const LIST_KEY: &str = "tide";

/// The line a reset of the record writes its count under (`reset: <n>`).
const RESET_KEY: &str = "reset";

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

/// Kept as a set of bare names rather than of `list:name` keys: the stage
/// list asks after every tile every frame, and a key built per lookup is a
/// hundred-odd allocations a frame.
#[derive(Resource, Default)]
pub struct Progress {
    cleared: HashSet<String>,
    /// Lines this build cannot read, kept word for word and written back:
    /// clears on a list this build lacks, or a line of a kind it has not
    /// heard of. Read and dropped, a player who opened an older or newer
    /// build once lost them on its next save.
    foreign: crate::app::paths::ForeignLines,
    /// How many times the record has been reset, as of the file this one
    /// was last read from or written to. A copy of the game still open on
    /// the record from before a reset holds every clear in memory, and its
    /// next save merged them all back; a file whose count is ahead says
    /// the clears in hand were reset, and only those made since are kept
    /// ([`Progress::merge`]).
    reset: u64,
    /// The clears made in this copy since the record last met the file:
    /// the ones a reset elsewhere did not see, and so did not undo.
    /// Never written.
    marked: HashSet<String>,
}

impl Progress {
    pub fn is_cleared(&self, name: &str) -> bool {
        self.cleared.contains(name)
    }

    /// Whether `level` is cleared, asked by the key it is
    /// filed under ([`crate::sim::Level::progress_key`]): a player's level
    /// by its file, a shipped one by its name. Every reader of a stage's
    /// tick asks here, so none can ask by the name the list shows.
    pub fn level_cleared(&self, level: &crate::sim::Level) -> bool {
        self.is_cleared(level.progress_key())
    }

    /// Records a clear of `level`, under the key [`Progress::level_cleared`]
    /// reads; true when this was the first time.
    pub fn mark_level(&mut self, level: &crate::sim::Level) -> bool {
        self.mark(level.progress_key())
    }

    /// File the ticks an older build kept under a player level's shown
    /// name under its file instead, where the name can only have meant
    /// that level (see [`crate::app::campaign::disambiguate`]). True when
    /// anything moved, which is worth a save. The name's own line stays:
    /// it is what an older build still reads.
    ///
    /// Filed, not marked: it is an old clear under a new key, not one made
    /// here, and marked, a copy open on the record from before a reset
    /// elsewhere wrote it back over the reset ([`Progress::merge`]).
    pub fn adopt_legacy(&mut self, campaign: &Campaign) -> bool {
        let mut moved = false;
        for level in &campaign.levels {
            let Some(filing) = &level.filing else {
                continue;
            };
            if let Some(name) = &filing.legacy
                && self.is_cleared(name)
            {
                moved |= self.cleared.insert(filing.key.clone());
            }
        }
        moved
    }

    /// Records a clear; true when this was the first time.
    pub fn mark(&mut self, name: &str) -> bool {
        self.marked.insert(name.to_string());
        self.cleared.insert(name.to_string())
    }

    /// Take in another record's clears, and the lines it kept that this
    /// build cannot read: a union, since a stage once cleared stays
    /// cleared whichever copy of the game cleared it. Unless `other` was
    /// reset since this one was read: then what this one holds from
    /// before is gone, and only the clears it made since are added.
    fn merge(&mut self, other: Progress) {
        let Progress {
            cleared,
            foreign,
            reset,
            marked: _,
        } = other;
        if reset > self.reset {
            self.cleared.retain(|name| self.marked.contains(name));
            self.foreign.clear();
            self.reset = reset;
        }
        self.cleared.extend(cleared);
        self.foreign.extend(foreign);
    }

    /// How many stages are cleared.
    pub fn cleared_count(&self) -> usize {
        self.cleared.len()
    }

    /// Forget every cleared stage.
    ///
    /// The lines this build cannot read go too: they are clears from
    /// another build's lists, and the player asked to start over.
    pub fn clear_all(&mut self) {
        self.cleared.clear();
        self.foreign.clear();
        self.marked.clear();
    }

    /// The classic rule: the first stage of a ladder is always open, so is
    /// any stage you have already cleared, so is the one right after a
    /// cleared stage. The Tide Pool and the co-op stages are a ladder each,
    /// so the first co-op stage is open from the start. Player-made levels
    /// sit past the built-ins and are never locked: they are yours, and
    /// the editor plays them anyway.
    pub fn unlocked(&self, campaign: &Campaign, index: usize) -> bool {
        use crate::app::campaign::Section;
        let section = campaign.section_of(index);
        if section == Section::Custom || index == campaign.section(section).start {
            return true;
        }
        let cleared = |at: usize| {
            campaign
                .levels
                .get(at)
                .is_some_and(|level| self.level_cleared(level))
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
            .filter(|level| self.level_cleared(level))
            .count()
    }

    /// The furthest stage that is open to play, for resuming the list.
    pub fn furthest_open(&self, campaign: &Campaign) -> usize {
        campaign
            .section(crate::app::campaign::Section::TidePool)
            .rev()
            .find(|&index| self.unlocked(campaign, index))
            .unwrap_or(0)
    }

    pub fn to_text(&self) -> String {
        // Destructured with no rest pattern: a new field refuses to build
        // here until it is written out. `parse` is lenient and would never
        // notice one going unsaved.
        let Self {
            cleared,
            foreign,
            reset,
            marked: _,
        } = self;
        let mut names: Vec<&String> = cleared.iter().collect();
        names.sort_unstable();
        let head = match reset {
            0 => String::new(),
            n => format!("{RESET_KEY}: {n}\n"),
        };
        let mut out = names.iter().fold(head, |mut out, name| {
            out.push_str("cleared: ");
            out.push_str(LIST_KEY);
            out.push(':');
            out.push_str(name);
            out.push('\n');
            out
        });
        foreign.append_to(&mut out);
        out
    }

    /// Lenient parse, in the style of the other save files: nothing in the
    /// file is fatal. A `cleared: tide:<name>` line is a clear; any other
    /// `key: value` line, a list this build lacks included, is kept as it
    /// stands for the next save to write back (see [`Progress::foreign`]).
    /// Only what is not a line of the file's shape at all, or a `cleared:`
    /// line naming nothing, is dropped.
    pub fn parse(text: &str) -> Progress {
        let mut progress = Progress::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            if key == "cleared" {
                // A `cleared:` line naming nothing is junk, not news.
                let Some((list, name)) = value.split_once(':').filter(|(_, n)| !n.is_empty())
                else {
                    continue;
                };
                match list == LIST_KEY {
                    true => {
                        progress.cleared.insert(name.to_string());
                    }
                    false => progress.foreign.keep(line),
                }
            } else if key == RESET_KEY
                && let Ok(n) = value.parse()
            {
                progress.reset = n;
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
    progress.marked.clear();
    crate::app::paths::write_atomic(path, progress.to_text())
}

/// Forget the clears filed under `keys`, in the record in hand and in the
/// file: a level the editor deleted. Its tick is filed by its file name,
/// which the next level saved under that name takes, and a brand-new level
/// came up cleared. Merged first like any save, then struck, so a clear
/// another copy filed meanwhile is kept, and this one is not merged back.
pub fn forget(progress: &mut Progress, keys: &[&str]) {
    let _ = forget_in(&save_path(), progress, keys);
}

/// [`forget`] in a file named outright, with the error kept.
pub fn forget_in(
    path: &std::path::Path,
    progress: &mut Progress,
    keys: &[&str],
) -> std::io::Result<()> {
    progress.merge(load_in(path));
    let mut struck = false;
    for key in keys {
        struck |= progress.cleared.remove(*key);
    }
    if !struck {
        return Ok(());
    }
    progress.marked.clear();
    crate::app::paths::write_atomic(path, progress.to_text())
}

/// Write the record as it stands, over whatever is on disk: the reset,
/// which is the one change a merge would undo. Counted in the file, one
/// past any count seen there or here, so another copy of the game open on
/// the record from before takes its clears as reset too.
pub fn save_reset(progress: &mut Progress) {
    let _ = reset_in(&save_path(), progress);
}

/// [`save_reset`] to a file named outright, with the error kept.
pub fn reset_in(path: &std::path::Path, progress: &mut Progress) -> std::io::Result<()> {
    progress.reset = progress.reset.max(load_in(path).reset) + 1;
    crate::app::paths::write_atomic(path, progress.to_text())
}

/// A cleared stage, saved the moment it is cleared: the stage list is the
/// player's record of the campaign, and a crash should not cost it.
///
/// Not a stage played from a pasted code: it is on no shelf, and its tick
/// went under its bare name, where a saved level of that name later took
/// it over as its own.
pub fn record_cleared(campaign: Res<Campaign>, mut progress: ResMut<Progress>) {
    if campaign.is_from_a_code(campaign.index) {
        return;
    }
    if progress.mark_level(campaign.current()) {
        save(&mut progress);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Level, campaign_levels};

    /// A stage played from a pasted code leaves no tick: it is filed
    /// nowhere, and its bare name was taken over by a saved level of the
    /// same name.
    #[test]
    fn a_stage_from_a_code_is_not_recorded() {
        use bevy::ecs::system::RunSystemOnce;
        let mut pasted = campaign_levels().remove(0);
        pasted.name = "Code Probe".into();
        let mut campaign = campaign(2, vec![pasted]);
        campaign.index = 2;
        let mut app = App::new();
        app.insert_resource(campaign);
        app.init_resource::<Progress>();
        let _ = app.world_mut().run_system_once(record_cleared);
        assert!(!app.world().resource::<Progress>().is_cleared("Code Probe"));
    }

    /// A deleted level's clear goes from the file as well as the record in
    /// hand, and nothing else does: the next save does not merge it back.
    #[test]
    fn a_forgotten_clear_stays_forgotten() {
        let path = crate::app::paths::test_dir("progress-forget").join("progress.txt");
        let mut on_disk = Progress::default();
        on_disk.mark("file:custom/gone.txt");
        on_disk.mark("file:custom/kept.txt");
        save_in(&path, &mut on_disk).expect("saved");
        let mut progress = Progress::default();
        forget_in(&path, &mut progress, &["file:custom/gone.txt"]).expect("forgot");
        assert!(!progress.is_cleared("file:custom/gone.txt"));
        assert!(progress.is_cleared("file:custom/kept.txt"), "merged in");
        save_in(&path, &mut progress).expect("saved again");
        let back = load_in(&path);
        assert!(!back.is_cleared("file:custom/gone.txt"));
        assert!(back.is_cleared("file:custom/kept.txt"));
    }

    /// A reset in one copy of the game is not undone by another copy
    /// still holding the clears from before it: that copy's next save
    /// keeps only what it cleared since, and the reset stands.
    #[test]
    fn a_reset_holds_against_a_copy_opened_before_it() {
        let path = crate::app::paths::test_dir("progress-reset").join("progress.txt");
        let mut first = Progress::default();
        first.mark("Old A");
        first.mark("Old B");
        save_in(&path, &mut first).expect("saved");
        let mut second = load_in(&path);
        assert!(second.is_cleared("Old A"));
        first.clear_all();
        reset_in(&path, &mut first).expect("reset");
        second.mark("New C");
        save_in(&path, &mut second).expect("saved over");
        let back = load_in(&path);
        assert!(!back.is_cleared("Old A") && !back.is_cleared("Old B"));
        assert!(back.is_cleared("New C"), "a clear made since stands");
        assert!(!second.is_cleared("Old A"), "nor shown in the copy");
        // And the copy that reset takes the new clear in as any other.
        first.mark("New D");
        save_in(&path, &mut first).expect("saved");
        assert!(first.is_cleared("New C") && first.is_cleared("New D"));
        // A second reset counts past the first.
        first.clear_all();
        reset_in(&path, &mut first).expect("reset again");
        assert_eq!(load_in(&path).reset, 2);
        assert_eq!(load_in(&path).cleared_count(), 0);
    }

    /// A copy opened before a reset that adopts an old name's tick under
    /// its file has not cleared anything: its next save keeps the reset.
    #[test]
    fn an_adopted_tick_does_not_outlive_a_reset() {
        let path = crate::app::paths::test_dir("progress-adopt-reset").join("progress.txt");
        let mut alone = crate::sim::Level::from_board("Alone", 1, crate::sim::Board::new(4, 4, 0));
        alone.filing = Some(crate::sim::Filing {
            key: "file:custom/alone.txt".into(),
            legacy: Some("Alone".into()),
        });
        let campaign = campaign(1, vec![alone]);
        let mut stale = Progress::default();
        stale.mark("Alone");
        save_in(&path, &mut stale).expect("saved");
        let mut first = load_in(&path);
        first.clear_all();
        reset_in(&path, &mut first).expect("reset");
        assert!(stale.adopt_legacy(&campaign), "the tick moves");
        save_in(&path, &mut stale).expect("saved over");
        let back = load_in(&path);
        assert!(!back.is_cleared("file:custom/alone.txt"));
        assert!(!back.is_cleared("Alone"));
    }

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

    /// A clear an older build kept under a player's level's name is not a
    /// clear of the shipped co-op stage of that name, and opens nothing on
    /// the co-op ladder, whether read as it stands or adopted by a level
    /// of the player's that is now told apart from the shipped one.
    #[test]
    fn a_name_kept_clear_is_not_a_coop_stages() {
        let coop = crate::sim::coop_levels();
        let first = campaign_levels().len();
        let mut levels = campaign_levels();
        levels.extend(coop.clone());
        let builtins = levels.len();
        let mut mine = coop[0].clone();
        mine.filing = Some(crate::sim::Filing {
            key: "file:custom/mine.txt".into(),
            legacy: None,
        });
        levels.push(mine);
        crate::app::campaign::disambiguate(&mut levels, builtins);
        let campaign = Campaign {
            builtins,
            coop: coop.len(),
            ..Campaign::tide_pool_of(levels)
        };
        let mut progress = Progress::default();
        progress.mark(&coop[0].name);
        let _ = progress.adopt_legacy(&campaign);
        assert!(!progress.level_cleared(&campaign.levels[first]));
        assert!(
            !progress.unlocked(&campaign, first + 1),
            "nor opens the next"
        );
        assert_eq!(
            progress.cleared_in(
                &campaign,
                campaign.section(crate::app::campaign::Section::Coop,)
            ),
            0
        );
        // A clear of the stage itself is kept, and read back.
        assert!(progress.mark_level(&campaign.levels[first]));
        let back = Progress::parse(&progress.to_text());
        assert!(back.level_cleared(&campaign.levels[first]));
        assert!(back.unlocked(&campaign, first + 1));
    }

    /// The eight co-op stages of the shelf before it was reordered, as a
    /// progress file keeps their clears: under `coop:` and the English
    /// name. The shelf has since been reordered and grown, which moved
    /// every one of these to another place and another file number; a
    /// clear is filed by name, so each must still read as cleared, and
    /// none of the stages added since may.
    #[test]
    fn a_coop_clear_survives_the_shelf_being_reordered() {
        const FIRST_EIGHT: [&str; 8] = [
            "Fork in the Road",
            "Give Way",
            "Head On",
            "Up and Over",
            "Round the Rock",
            "Gull in the Lane",
            "Shoo",
            "Turn Back",
        ];
        let saved: String = FIRST_EIGHT
            .iter()
            .map(|name| format!("cleared: tide:coop:{name}\n"))
            .collect();
        let coop = crate::sim::coop_levels();
        let mut levels = campaign_levels();
        levels.extend(coop.clone());
        let campaign = Campaign {
            builtins: levels.len(),
            coop: coop.len(),
            ..Campaign::tide_pool_of(levels)
        };
        let progress = Progress::parse(&saved);
        let shelf = campaign.section(crate::app::campaign::Section::Coop);
        for name in FIRST_EIGHT {
            let at = shelf
                .clone()
                .find(|&at| campaign.levels[at].name == name)
                .unwrap_or_else(|| panic!("{name:?} is no longer on the co-op shelf"));
            assert!(
                progress.level_cleared(&campaign.levels[at]),
                "{name:?} lost its clear"
            );
            assert!(progress.unlocked(&campaign, at), "{name:?} is locked");
        }
        assert_eq!(progress.cleared_in(&campaign, shelf.clone()), 8);
        // A stage added since opens when the one before it is cleared, as
        // any stage does, and is not cleared for the player.
        for at in shelf.skip(1) {
            if !FIRST_EIGHT.contains(&campaign.levels[at].name.as_str()) {
                assert!(!progress.level_cleared(&campaign.levels[at]));
                assert_eq!(
                    progress.unlocked(&campaign, at),
                    progress.level_cleared(&campaign.levels[at - 1]),
                    "{:?}",
                    campaign.levels[at].name
                );
            }
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
        assert!(progress.mark(&first));
        assert!(!progress.mark(&first), "idempotent");
        assert!(progress.unlocked(&campaign, 1));
        assert!(!progress.unlocked(&campaign, 2), "no further than that");
        assert_eq!(progress.cleared_in(&campaign, 0..campaign.levels.len()), 1);
        assert_eq!(progress.furthest_open(&campaign), 1);
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
    /// back to the build that has it finds them where it left them.
    #[test]
    fn a_list_this_build_lacks_is_kept_for_the_build_that_has_it() {
        let text = "cleared: reef:Far Shoal\ncleared: tide:Welcome Ashore\nstreak: 4\n";
        let mixed = Progress::parse(text);
        assert!(mixed.is_cleared("Welcome Ashore"));
        assert!(!mixed.is_cleared("Far Shoal"));
        assert_eq!(mixed.cleared_count(), 1, "the reef is not ours to count");
        let back = mixed.to_text();
        assert!(back.contains("cleared: reef:Far Shoal\n"), "{back}");
        assert!(back.contains("streak: 4\n"), "{back}");
        assert_eq!(Progress::parse(&back).to_text(), back, "and stays put");
    }

    /// A record written by a build that still had the score-attack list
    /// reads as it did: its Tide Pool clears are clears, and its lines for
    /// the other list, clears and best runs alike, are kept word for word
    /// for that build rather than dropped.
    #[test]
    fn a_record_with_challenge_lines_still_loads() {
        let text = "cleared: beach:First Flood\ncleared: tide:Undertow\n\
                    best: beach:gold 64:First Flood\n";
        let old = Progress::parse(text);
        assert!(old.is_cleared("Undertow"));
        assert!(!old.is_cleared("First Flood"));
        assert_eq!(old.cleared_count(), 1);
        let back = old.to_text();
        assert!(back.contains("cleared: tide:Undertow\n"), "{back}");
        assert!(back.contains("cleared: beach:First Flood\n"), "{back}");
        assert!(back.contains("best: beach:gold 64:First Flood\n"), "{back}");
        assert_eq!(Progress::parse(&back).to_text(), back, "and stays put");
    }

    /// The file is written in one order whatever order the stages were
    /// cleared in, so two saves of the same record are the same bytes.
    #[test]
    fn the_record_is_written_in_one_order() {
        let mut progress = Progress::default();
        progress.mark("Undertow");
        progress.mark("Corner Pocket");
        assert_eq!(
            progress.to_text(),
            "cleared: tide:Corner Pocket\ncleared: tide:Undertow\n"
        );
        let mut reversed = Progress::default();
        reversed.mark("Corner Pocket");
        reversed.mark("Undertow");
        assert_eq!(reversed.to_text(), progress.to_text());
    }

    #[test]
    fn progress_survives_a_save_and_load() {
        let mut progress = Progress::default();
        progress.mark("Welcome Ashore");
        progress.mark("Undertow");
        let back = Progress::parse(&progress.to_text());
        assert!(back.is_cleared("Welcome Ashore"));
        assert!(back.is_cleared("Undertow"));
        assert_eq!(back.cleared_count(), 2);
        // Junk lines are ignored, not fatal.
        let salvaged = Progress::parse("garbage\ncleared:\ncleared: tide:Welcome Ashore\n");
        assert!(salvaged.is_cleared("Welcome Ashore"));
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
        progress.mark("Welcome Ashore");
        save_in(&path, &mut progress).expect("saved");
        let back = load_in(&path);
        assert!(back.is_cleared("Welcome Ashore"));
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
        second.mark("Undertow");
        save_in(&path, &mut second).expect("saved");
        first.mark("Corner Pocket");
        save_in(&path, &mut first).expect("saved");
        let back = load_in(&path);
        assert!(back.is_cleared("Undertow"), "kept");
        assert!(back.is_cleared("Corner Pocket"));
        assert!(first.is_cleared("Undertow"), "seen");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Clearing a stage marks the stage the campaign is on, and only once however often the win is seen.
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
        assert!(progress.is_cleared(&name));
        assert_eq!(progress.cleared_count(), 1);
    }
}
