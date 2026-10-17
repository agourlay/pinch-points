//! The level list a puzzle run walks, and the player-made levels that join
//! the built-in ones.

use crate::app::editor;
use crate::app::i18n::Lang;
use crate::sim::{ArenaProblem, Level, LevelKind};
use bevy::prelude::*;

/// The one shipped level whose lesson is the keys themselves (see
/// `LEVEL_HINTS`); a test checks the table still says so.
pub(crate) const KEY_LESSON_LEVEL: &str = "Welcome Ashore";

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

/// Whether the Tide Pool is being played by two, side by side.
///
/// Co-op is a second pair of hands, not a second player: both cursors place
/// and lift the same seat's arrows, so the stage's inventory is one pool the
/// two of them share and either can re-point or pick up any arrow on the
/// beach. Every stage is proved solvable with exactly its arrows, whoever
/// puts them down, so all hundred work as they are.
///
/// Tide Pool only. Beach Day's rule evicts the oldest arrow to make room,
/// and "the oldest of a shared pool" is a rule nobody would guess.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Coop(pub bool);

impl Coop {
    /// Whether co-op applies to the list being played.
    pub fn in_play(self, campaign: &Campaign) -> bool {
        self.0 && campaign.kind == CampaignKind::TidePool
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

/// Where a level sits in its own section of the list ([`Campaign::place`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    /// Its number in the section, from 1.
    pub at: usize,
    /// How many levels the section holds.
    pub of: usize,
    /// Whether the section is the player's own levels, not the shipped list.
    pub custom: bool,
}

#[cfg(test)]
impl Campaign {
    /// The shipped Tide Pool, on its first stage.
    pub fn tide_pool() -> Self {
        Self::tide_pool_of(crate::sim::campaign_levels())
    }

    /// A Tide Pool of `levels`, every one shipped, on its first stage.
    pub fn tide_pool_of(levels: Vec<Level>) -> Self {
        Campaign {
            kind: CampaignKind::TidePool,
            builtins: levels.len(),
            levels,
            index: 0,
        }
    }
}

impl Campaign {
    pub fn current(&self) -> &Level {
        &self.levels[self.index]
    }

    /// Where the current level sits in its own section, and whether that
    /// section is the player's own: their levels are a shelf behind the
    /// shipped list rather than stage a hundred and one, so both the
    /// hundredth stage and the first driftwood read as one of their own
    /// count. The stage list draws the two sections apart the same way.
    pub fn place(&self) -> Place {
        match self.index < self.builtins {
            true => Place {
                at: self.index + 1,
                of: self.builtins,
                custom: false,
            },
            false => Place {
                at: self.index - self.builtins + 1,
                of: self.levels.len() - self.builtins,
                custom: true,
            },
        }
    }

    /// The name level `at` is shown under in `lang`.
    ///
    /// The translations are written for the shipped levels and keyed by
    /// their English names, on both lists at once, so only a shipped level
    /// is looked up. A player's level keeps the name they gave it, even
    /// one that happens to match a stage on the other list.
    pub fn shown_name(&self, at: usize, lang: Lang) -> &str {
        let name = &self.levels[at].name;
        match self.is_shipped(at) {
            true => lang.level_name(name),
            false => name,
        }
    }

    /// The teaching hint for level `at`, if it is a shipped level that has
    /// one. A player's level teaches what its author meant it to, which no
    /// hint was written for, so it borrows none on account of its name.
    fn hint(&self, at: usize, lang: Lang) -> Option<&'static str> {
        match self.is_shipped(at) {
            true => lang.level_hint(&self.levels[at].name),
            false => None,
        }
    }

    /// Level `at`'s hint as this player may be shown it, wherever it is
    /// shown: the stage list and the puzzle's own line ask the same thing.
    /// The first level's lesson names the stock keys, so with the keys
    /// rebound or the one-hand preset on it would teach the wrong ones, and
    /// it is kept back. Asked as the prompt asks it: a co-op partner sets
    /// the preset aside.
    pub fn shown_hint(
        &self,
        at: usize,
        settings: &crate::app::settings::GameSettings,
        coop: Coop,
    ) -> Option<&'static str> {
        let hint = self.hint(at, settings.language)?;
        let names_keys = self.levels[at].name == KEY_LESSON_LEVEL;
        (!names_keys || settings.stock_legend(coop.in_play(self))).then_some(hint)
    }

    /// Whether level `at` ships with the game, rather than being one of
    /// the player's own behind it.
    fn is_shipped(&self, at: usize) -> bool {
        at < self.builtins
    }

    /// Whether the current level ends the run Enter is walking. Both
    /// sections end one: the shipped campaign stops at its last shipped
    /// stage rather than walking on into the player's own levels, and
    /// those stop at the end of the list.
    pub fn is_last(&self) -> bool {
        let Place { at, of, .. } = self.place();
        at == of
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

/// Give every level on the list a name of its own. Progress on a player's
/// level off the shelf is filed by its file ([`crate::sim::Filing`]), but
/// the stage list still shows two levels called the same as two
/// different ones, and a player's puzzle called "Welcome Ashore" beside
/// the shipped one would read as the same stage. The shipped list is
/// unique already; the player's levels are renamed on the way in, in list
/// order, with a count after the name, and the file on disk keeps the name
/// it was saved under.
///
/// Before files had keys the tick was filed under the name shown here,
/// and that is where an older clear still sits. It is this level's for
/// certain only when no other player level shares its name, since then
/// the renaming cannot have handed the shown name to another file; such a
/// level's filing names it as `legacy` for the stage list to adopt (see
/// `Progress::adopt_legacy`).
///
/// Only this list's names are reserved. A puzzle of the player's called
/// "First Flood", like a Beach Day stage, shares no tick with it, since
/// the two are filed under different lists; and it is not shown under
/// that stage's translated name or offered its hint, because those are
/// only looked up for shipped levels (see [`Campaign::shown_name`]).
/// Renaming it would cost the player the tick it has earned under its own
/// name.
pub(crate) fn disambiguate(levels: &mut [Level], builtins: usize) {
    let builtins = builtins.min(levels.len());
    let mut taken: std::collections::HashSet<String> = levels[..builtins]
        .iter()
        .map(|level| level.name.clone())
        .collect();
    let mut sharing: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for level in &levels[builtins..] {
        *sharing.entry(level.name.clone()).or_default() += 1;
    }
    for level in levels.iter_mut().skip(builtins) {
        let original = level.name.clone();
        if !taken.insert(level.name.clone()) {
            // Claiming the name is the test for it.
            level.name = (2..)
                .find_map(|n| {
                    let candidate = format!("{} ({n})", level.name);
                    taken.insert(candidate.clone()).then_some(candidate)
                })
                .unwrap_or_else(|| level.name.clone());
        }
        // Shared with no other player level, and not renamed onto the name
        // another of them was saved under.
        let alone = sharing.get(&original) == Some(&1)
            && (level.name == original || !sharing.contains_key(&level.name));
        if let Some(filing) = level.filing.as_mut() {
            filing.legacy = alone.then(|| level.name.clone());
        }
    }
}

/// The player's levels that are stages: the ones they built as puzzles, and
/// that have somebody to route. A beach built for a match is not a stage
/// with an unusual number of castles, and on the list it ends the
/// campaign.
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

/// The key a level read from `path` files its cleared tick under: the
/// file's name, so two files with the same level name keep a tick each,
/// and a new file sorting ahead of them takes neither. The old single
/// slot and a file in the shelf directory may share a file name, so the
/// shelf's are marked as being in it.
fn filing_key(legacy: &std::path::Path, path: &std::path::Path) -> String {
    let file = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    match path == legacy {
        true => format!("file:{file}"),
        false => format!("file:custom/{file}"),
    }
}

/// [`load_custom_levels`] with both places named outright: the old single
/// save slot and the shelf directory. The real ones hang off the data
/// directory, read from the process environment, which a test cannot change
/// without racing every other test in the binary.
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
            Ok(mut level) => {
                level.filing = Some(crate::sim::Filing {
                    key: filing_key(legacy, &path),
                    legacy: None,
                });
                info!(
                    "loaded custom {} {:?} from {}",
                    level.kind.token(),
                    level.name,
                    path.display()
                );
                // Said here, beside the line that read the file, and once
                // per read: the filters below are asked the same question
                // by two screens and would answer it twice over.
                match level.kind {
                    LevelKind::Arena => match level.arena_problem() {
                        Some(ArenaProblem::SkippedSeat(_)) => warn!(
                            "{}: a beach whose castles skip a player - no table can sit at it",
                            path.display()
                        ),
                        Some(ArenaProblem::TooFew(seats)) => warn!(
                            "{}: a beach with castles for {seats} - no table can sit at it",
                            path.display()
                        ),
                        None => {}
                    },
                    LevelKind::Puzzle if level.crab_count() == 0 => {
                        warn!("{}: a puzzle with no crabs to route", path.display());
                    }
                    LevelKind::Puzzle => {}
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

    /// A Tide Pool level of the player's named like a Beach Day stage keeps
    /// its name, and with it the tick progress filed under that name: the
    /// two lists file progress apart. It is shown under its own name all
    /// the same, untranslated and with no hint, since those are only for
    /// the shipped levels, and so is a Tide Pool level of theirs whose
    /// name a shipped one's translation happens to be keyed by.
    #[test]
    fn a_player_level_named_like_a_beach_day_stage_keeps_its_name() {
        let beach_day = crate::sim::challenge_levels()[0].name.clone();
        let tide_pool = crate::sim::campaign_levels()[0].name.clone();
        let mut levels = vec![
            level(&tide_pool, "puzzle", true),
            level(&beach_day, "puzzle", true),
        ];
        disambiguate(&mut levels, 1);
        assert_eq!(levels[0].name, tide_pool, "the shipped one is left alone");
        assert_eq!(levels[1].name, beach_day, "and so is the player's");

        let lang = Lang::Fr;
        assert_ne!(
            lang.level_name(&beach_day),
            beach_day,
            "the Beach Day stage has a French name for this to be about"
        );
        let campaign = Campaign {
            builtins: 1,
            ..Campaign::tide_pool_of(levels)
        };
        assert_eq!(campaign.shown_name(1, lang), beach_day, "untranslated");
        assert_eq!(campaign.hint(1, lang), None, "and with no hint");
        assert_eq!(
            campaign.shown_name(0, lang),
            lang.level_name(&tide_pool),
            "the shipped stage is still translated"
        );
        assert_eq!(campaign.hint(0, lang), lang.level_hint(&tide_pool));

        let mut progress = crate::app::progress::Progress::default();
        progress.mark(CampaignKind::TidePool, &beach_day);
        assert!(progress.is_cleared(CampaignKind::TidePool, &beach_day));
        assert!(
            !progress.is_cleared(CampaignKind::BeachDay, &beach_day),
            "the stage it is named like is not ticked by it"
        );
    }

    /// The Tide Pool list as the shelf in `dir` makes it, behind two
    /// shipped stages.
    fn listed(dir: &std::path::Path) -> Campaign {
        let mut levels = crate::sim::campaign_levels();
        levels.truncate(2);
        levels.extend(custom_puzzles(load_custom_levels_in(
            &dir.join("slot.txt"),
            &dir.join("custom"),
        )));
        disambiguate(&mut levels, 2);
        Campaign {
            builtins: 2,
            ..Campaign::tide_pool_of(levels)
        }
    }

    /// Two files saved under one name keep a tick each, and a third that
    /// sorts ahead of them, under the same name, takes neither: the list
    /// renames them "X (2)" in file order, and the tick was filed under
    /// that shown name, so it moved to whichever file now held it.
    #[test]
    fn a_tick_stays_with_its_file_when_a_namesake_sorts_ahead() {
        use crate::app::progress::Progress;
        let dir = crate::app::paths::test_dir("shelf-namesakes");
        let shelf = dir.join("custom");
        std::fs::create_dir_all(&shelf).expect("shelf");
        let text = |name: &str| level(name, "puzzle", true).to_text();
        std::fs::write(shelf.join("b.txt"), text("Twins")).expect("write");
        std::fs::write(shelf.join("c.txt"), text("Twins")).expect("write");
        let before = listed(&dir);
        let cleared = before
            .levels
            .iter()
            .position(|level| level.name == "Twins (2)")
            .expect("the second is told apart");
        let mut progress = Progress::default();
        progress.mark_level(CampaignKind::TidePool, &before.levels[cleared]);

        std::fs::write(shelf.join("a.txt"), text("Twins")).expect("write");
        let after = listed(&dir);
        let ticked: Vec<&str> = after
            .levels
            .iter()
            .filter(|level| progress.level_cleared(CampaignKind::TidePool, level))
            .map(|level| level.progress_key())
            .collect();
        assert_eq!(
            ticked,
            ["file:custom/c.txt"],
            "still the file that was cleared"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A tick an older build filed under a level's name is adopted by its
    /// file where the name could only have meant that one level, and left
    /// where two files share the name and either could have earned it.
    #[test]
    fn a_tick_filed_by_name_is_adopted_where_the_name_is_unambiguous() {
        use crate::app::progress::Progress;
        let dir = crate::app::paths::test_dir("shelf-legacy");
        let shelf = dir.join("custom");
        std::fs::create_dir_all(&shelf).expect("shelf");
        let text = |name: &str| level(name, "puzzle", true).to_text();
        std::fs::write(shelf.join("a.txt"), text("Alone")).expect("write");
        std::fs::write(shelf.join("b.txt"), text("Pair")).expect("write");
        std::fs::write(shelf.join("c.txt"), text("Pair")).expect("write");
        let campaign = listed(&dir);
        let mut progress = Progress::default();
        for name in ["Alone", "Pair", "Pair (2)"] {
            progress.mark(CampaignKind::TidePool, name);
        }
        assert!(progress.adopt_legacy(&campaign), "something moved");
        assert!(!progress.adopt_legacy(&campaign), "and only once");
        let ticked = |key: &str| {
            let level = campaign
                .levels
                .iter()
                .find(|level| level.progress_key() == key)
                .expect("on the list");
            progress.level_cleared(CampaignKind::TidePool, level)
        };
        assert!(ticked("file:custom/a.txt"), "the only Alone");
        assert!(!ticked("file:custom/b.txt"), "either Pair could have");
        assert!(!ticked("file:custom/c.txt"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The shelf reads the old save slot first and then every `.txt` in the
    /// folder in name order, skipping what is not a level and what is not a
    /// text file, rather than failing the whole shelf over one bad file.
    #[test]
    fn the_shelf_reads_its_folder_in_order_and_skips_what_it_cannot_read() {
        let dir = crate::app::paths::test_dir("shelf-read");
        let text = |name: &str| level(name, "puzzle", true).to_text();
        std::fs::write(dir.join("slot.txt"), text("Old Slot")).expect("write");
        let shelf = dir.join("custom");
        std::fs::create_dir_all(&shelf).expect("shelf");
        std::fs::write(shelf.join("b.txt"), text("Second")).expect("write");
        std::fs::write(shelf.join("a.txt"), text("First")).expect("write");
        std::fs::write(shelf.join("c.txt"), "not a level at all").expect("write");
        std::fs::write(shelf.join("d.png"), text("Not A Text File")).expect("write");
        let names: Vec<String> = load_custom_levels_in(&dir.join("slot.txt"), &shelf)
            .into_iter()
            .map(|level| level.name)
            .collect();
        assert_eq!(names, ["Old Slot", "First", "Second"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Nothing saved yet, anywhere, is an empty shelf rather than an error.
    #[test]
    fn a_first_run_has_an_empty_shelf() {
        let dir = crate::app::paths::test_dir("shelf-empty");
        assert!(load_custom_levels_in(&dir.join("absent.txt"), &dir.join("absent")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Co-op is the Tide Pool's alone: on Beach Day the switch changes
    /// nothing, and off it is off everywhere.
    #[test]
    fn coop_applies_to_the_tide_pool_only() {
        let (levels, builtins) = (crate::sim::campaign_levels(), 3);
        let mut campaign = Campaign {
            builtins,
            ..Campaign::tide_pool_of(levels)
        };
        assert!(Coop(true).in_play(&campaign));
        assert!(!Coop(false).in_play(&campaign));
        campaign.kind = CampaignKind::BeachDay;
        assert!(!Coop(true).in_play(&campaign));
    }

    /// A reset starts the new list from its first level, and a shipped
    /// count longer than the list is held to the list.
    #[test]
    fn a_reset_starts_over_and_holds_the_shipped_count_to_the_list() {
        let mut campaign = Campaign {
            index: 7,
            builtins: 100,
            ..Campaign::tide_pool()
        };
        let two = vec![level("One", "puzzle", true), level("Two", "puzzle", true)];
        campaign.reset(CampaignKind::BeachDay, two, 9);
        assert_eq!(campaign.kind, CampaignKind::BeachDay);
        assert_eq!(campaign.index, 0);
        assert_eq!(campaign.builtins, 2);
        assert_eq!(campaign.current().name, "One");
    }

    /// The key lesson is kept back under keys it does not describe, and
    /// the list asks the same function the puzzle's hint line does, so it
    /// cannot show it where the puzzle would not.
    #[test]
    fn the_key_lesson_is_shown_only_under_the_stock_keys() {
        use crate::app::settings::{CommitScheme, GameSettings};
        let levels = crate::sim::campaign_levels();
        let at = levels
            .iter()
            .position(|level| level.name == KEY_LESSON_LEVEL)
            .expect("the key lesson ships");
        let campaign = Campaign::tide_pool_of(levels);
        let stock = GameSettings::default();
        assert!(campaign.shown_hint(at, &stock, Coop(false)).is_some());
        let one_hand = GameSettings {
            commit: CommitScheme::Ijkl,
            ..GameSettings::default()
        };
        assert_eq!(campaign.shown_hint(at, &one_hand, Coop(false)), None);
        // Co-op sets the preset aside, so the stock keys are true again.
        assert!(campaign.shown_hint(at, &one_hand, Coop(true)).is_some());
    }

    /// The level whose hint is withheld under rebound keys is the one that
    /// names keys, and the only one: the others teach the beach, and stay
    /// up whatever the keyboard says.
    #[test]
    fn only_the_key_lesson_names_keys() {
        // "arrow key", not "arrow": the signposts are called arrows now, so
        // the bare word is the *object* and appears in several hints. Only
        // the keys are withheld under a rebound keyboard.
        let names_keys = |hint: &str| hint.contains("WASD") || hint.contains("arrow key");
        assert!(names_keys(
            crate::app::i18n::Lang::En
                .level_hint(KEY_LESSON_LEVEL)
                .unwrap()
        ));
        for level in crate::sim::campaign_levels() {
            if level.name != KEY_LESSON_LEVEL
                && let Some(hint) = Lang::En.level_hint(&level.name)
            {
                assert!(!names_keys(hint), "{:?} names keys too", level.name);
            }
        }
    }
}
