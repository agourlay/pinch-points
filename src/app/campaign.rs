//! The level list a puzzle run walks, and the player-made levels that join
//! the built-in ones.

use crate::app::editor;
use crate::sim::{Level, LevelKind};
use bevy::prelude::*;

/// Which level list the player is running.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CampaignKind {
    TidePool,
    BeachDay,
}

impl CampaignKind {
    /// Stable save-file key, so progress in one list is not progress in the
    /// other.
    pub fn key(self) -> &'static str {
        match self {
            CampaignKind::TidePool => "tide",
            CampaignKind::BeachDay => "beach",
        }
    }

    /// The list a save-file key names, if this build has it.
    pub fn from_key(key: &str) -> Option<CampaignKind> {
        [CampaignKind::TidePool, CampaignKind::BeachDay]
            .into_iter()
            .find(|kind| kind.key() == key)
    }
}

#[derive(Resource)]
pub struct Campaign {
    pub kind: CampaignKind,
    pub levels: Vec<Level>,
    pub index: usize,
    /// How many leading entries of `levels` ship with the game. The rest are
    /// the player's own, which the stage list never locks.
    pub builtins: usize,
}

impl Campaign {
    pub fn current(&self) -> &Level {
        &self.levels[self.index]
    }

    /// Swap in a fresh level list and start from its first level.
    /// A whole struct literal, so a new field cannot survive a reset unseen.
    pub(crate) fn reset(&mut self, kind: CampaignKind, levels: Vec<Level>, builtins: usize) {
        let builtins = builtins.min(levels.len());
        *self = Campaign {
            kind,
            levels,
            index: 0,
            builtins,
        };
    }
}

/// The Tide Pool list: the shipped campaign, then whatever the player has
/// built, with the count of shipped levels (the ones the stage list gates).
pub(crate) fn tide_pool_levels() -> (Vec<Level>, usize) {
    let mut levels = crate::sim::campaign_levels();
    let builtins = levels.len();
    levels.extend(custom_puzzles(load_custom_levels()));
    disambiguate(&mut levels, builtins);
    (levels, builtins)
}

/// Give every level on the list a name of its own. Progress is filed by
/// name, and so are the translated names and the hints, so a player's
/// puzzle called "Welcome Ashore" would share the shipped one's tick, its
/// French name and its hint. The shipped list is unique already (a test
/// below says so); the player's levels are renamed on the way in, in
/// list order, with a count after the name: the file on disk keeps the
/// name it was saved under, only the list reads it apart.
pub(crate) fn disambiguate(levels: &mut [Level], builtins: usize) {
    let mut taken: std::collections::HashSet<String> = levels[..builtins.min(levels.len())]
        .iter()
        .map(|level| level.name.clone())
        .collect();
    for level in levels.iter_mut().skip(builtins) {
        if taken.insert(level.name.clone()) {
            continue;
        }
        // Claiming the name is the test for it.
        let renamed = (2..)
            .find_map(|n| {
                let candidate = format!("{} ({n})", level.name);
                taken.insert(candidate.clone()).then_some(candidate)
            })
            .unwrap_or_else(|| level.name.clone());
        level.name = renamed;
    }
}

/// The player's levels that are stages: the ones they built as puzzles, and
/// that have somebody to route. A beach they built for a match is not a
/// stage with an unusual number of castles, and putting it on the list made
/// the campaign end on someone's versus arena.
pub fn custom_puzzles(levels: Vec<Level>) -> Vec<Level> {
    levels
        .into_iter()
        .filter(|level| level.kind == LevelKind::Puzzle && level.crab_count() > 0)
        .collect()
}

/// The player's levels that are beaches to fight over. The seat count is
/// the map dial's to check: a two-castle arena is a beach, just not one a
/// table of four can sit at.
pub fn custom_arenas(levels: Vec<Level>) -> Vec<Level> {
    levels
        .into_iter()
        .filter(|level| level.kind == LevelKind::Arena)
        .collect()
}

/// Everything on the player's shelf: the editor's old save slot plus
/// anything dropped into `levels/custom/` under the XDG data directory.
/// Unparseable files are skipped with a log line rather than breaking
/// startup; which list a level joins is [`Level::kind`]'s to say.
pub(crate) fn load_custom_levels() -> Vec<Level> {
    load_custom_levels_in(&editor::legacy_save_path(), &editor::custom_dir())
}

/// [`load_custom_levels`] with both places named outright: the old single
/// save slot and the shelf directory. The real ones hang off the data
/// directory, which is read from the process environment, and a test that
/// changed that would race every other test in the binary.
pub fn load_custom_levels_in(legacy: &std::path::Path, custom: &std::path::Path) -> Vec<Level> {
    let mut paths = vec![legacy.to_path_buf()];
    if let Ok(dir) = std::fs::read_dir(custom) {
        let mut extra: Vec<_> = dir
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "txt"))
            .collect();
        extra.sort();
        paths.extend(extra);
    }
    let mut levels = Vec::new();
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue; // absent is normal (nothing saved yet)
        };
        match Level::parse(&text) {
            Ok(level) => {
                info!(
                    "loaded custom {} {:?} from {}",
                    level.kind.token(),
                    level.name,
                    path.display()
                );
                // Said here, beside the line that read the file, and once
                // per read: the filters below are asked the same question
                // by two screens and would answer it twice over.
                if level.kind == LevelKind::Puzzle && level.crab_count() == 0 {
                    warn!("{}: a puzzle with no crabs to route", path.display());
                }
                if level.kind == LevelKind::Arena && level.seats() < 2 {
                    warn!(
                        "{}: a beach with castles for {} - no table can sit at it",
                        path.display(),
                        level.seats()
                    );
                }
                levels.push(level);
            }
            Err(e) => warn!("skipping {}: {e}", path.display()),
        }
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(name: &str, kind: &str, crabs: bool) -> Level {
        let crab = if crabs { "crab: 0,0 R L common\n" } else { "" };
        Level::parse(&format!(
            "name: {name}\nposts: 2\nkind: {kind}\n{crab}map:\n+-+-+-+\n|0 . 1|\n+-+-+-+\n"
        ))
        .expect("a level")
    }

    /// The shelf splits by what the author chose, not by what the board
    /// looks like: both of these have two castles and only one is a beach.
    #[test]
    fn the_shelf_splits_by_kind() {
        let shelf = || {
            vec![
                level("Stage", "puzzle", true),
                level("Beach", "arena", true),
                level("Empty Beach", "arena", false),
                level("Crabless", "puzzle", false),
            ]
        };
        let names = |levels: Vec<Level>| -> Vec<String> {
            levels.into_iter().map(|level| level.name).collect()
        };
        assert_eq!(names(custom_puzzles(shelf())), ["Stage"]);
        // A beach fed only by holes has no crab standing on it at the
        // start, which is not a reason to keep it off the map dial.
        assert_eq!(names(custom_arenas(shelf())), ["Beach", "Empty Beach"]);
    }

    /// The names progress is filed under have to be one each on the
    /// shipped lists, or two stages would share a tick.
    #[test]
    fn shipped_level_names_are_unique() {
        for levels in [
            crate::sim::campaign_levels(),
            crate::sim::challenge_levels(),
        ] {
            let mut seen = std::collections::HashSet::new();
            for level in &levels {
                assert!(seen.insert(level.name.clone()), "{:?} twice", level.name);
            }
        }
    }

    /// A player's level named like a shipped one, or like another of
    /// theirs, is told apart on the list, so it does not inherit the
    /// other's cleared tick. The shipped ones are left alone.
    #[test]
    fn same_named_player_levels_are_told_apart() {
        let mut levels = vec![
            level("Welcome Ashore", "puzzle", true),
            level("Gull Alley", "puzzle", true),
            level("Welcome Ashore", "puzzle", true),
            level("Mine", "puzzle", true),
            level("Mine", "puzzle", true),
            level("Mine", "puzzle", true),
            level("Mine (2)", "puzzle", true),
        ];
        disambiguate(&mut levels, 2);
        let names: Vec<&str> = levels.iter().map(|level| level.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Welcome Ashore",
                "Gull Alley",
                "Welcome Ashore (2)",
                "Mine",
                "Mine (2)",
                "Mine (3)",
                "Mine (2) (2)",
            ]
        );
    }
}
