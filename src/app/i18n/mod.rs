//! Localization: every player-facing string, in English, French, German,
//! Spanish, Italian, Dutch, Russian, and Japanese.
//!
//! One [`Tr`] table per language; a missing translation is a compile error.
//! Parameterized strings carry `{x}` markers, filled at the call site by
//! [`fill`] - which is the only way they are filled, so a template with two
//! markers reads as one call rather than a chain of replacements. Every
//! translated voice is informal (tu / du / tú / tu /
//! je / ты, and Japanese's plain friendly register), since the game is
//! for kids as much as anyone.

/// A supported UI language. Persisted in settings.txt by its `key`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Lang {
    #[default]
    En,
    Fr,
    De,
    Es,
    It,
    Nl,
    Ru,
    Ja,
}

pub const ALL_LANGS: [Lang; 8] = [
    Lang::En,
    Lang::Fr,
    Lang::De,
    Lang::Es,
    Lang::It,
    Lang::Nl,
    Lang::Ru,
    Lang::Ja,
];

impl crate::app::cycle::Cycle for Lang {
    const VARIANTS: &'static [Self] = &ALL_LANGS;
}

/// Fill a template's `{marker}`s. `fill(tr.wins, &[("p", "2")])`.
pub fn fill(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in args {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

impl Lang {
    pub fn tr(self) -> &'static Tr {
        match self {
            Lang::En => &EN,
            Lang::Fr => &FR,
            Lang::De => &DE,
            Lang::Es => &ES,
            Lang::It => &IT,
            Lang::Nl => &NL,
            Lang::Ru => &RU,
            Lang::Ja => &JA,
        }
    }

    /// Stable settings-file key.
    pub fn key(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Fr => "fr",
            Lang::De => "de",
            Lang::Es => "es",
            Lang::It => "it",
            Lang::Nl => "nl",
            Lang::Ru => "ru",
            Lang::Ja => "ja",
        }
    }

    pub fn from_key(key: &str) -> Lang {
        match key {
            "fr" => Lang::Fr,
            "de" => Lang::De,
            "es" => Lang::Es,
            "it" => Lang::It,
            "nl" => Lang::Nl,
            "ru" => Lang::Ru,
            "ja" => Lang::Ja,
            _ => Lang::En,
        }
    }

    /// The language's own name, shown in the settings row.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Fr => "Français",
            Lang::De => "Deutsch",
            Lang::Es => "Español",
            Lang::It => "Italiano",
            Lang::Nl => "Nederlands",
            Lang::Ru => "Русский",
            Lang::Ja => "日本語",
        }
    }

    /// This language's column in the level tables, English first. Both
    /// tables order their columns this way, so the order lives here once
    /// rather than being counted out again at each lookup.
    fn column(self) -> usize {
        match self {
            Lang::En => 0,
            Lang::Fr => 1,
            Lang::De => 2,
            Lang::Es => 3,
            Lang::It => 4,
            Lang::Nl => 5,
            Lang::Ru => 6,
            Lang::Ja => 7,
        }
    }

    /// Teaching hint for a level, if it has one. Column 0 is the English
    /// name the row is keyed by, so the hints sit one place further along.
    pub fn level_hint(self, en_name: &str) -> Option<&'static str> {
        LEVEL_HINTS
            .iter()
            .find(|row| row[0] == en_name)
            .map(|row| row[1 + self.column()])
    }

    /// Localized name for a built-in level; custom level names pass through.
    /// English is both the key and its own translation, so it needs no
    /// lookup at all.
    pub fn level_name(self, en: &str) -> &str {
        if self == Lang::En {
            return en;
        }
        LEVEL_NAMES
            .iter()
            .find(|row| row[0] == en)
            .map_or(en, |row| row[self.column()])
    }
}

/// Declares [`Tr`], and with it a way to walk every string it holds.
///
/// The walk is generated from the same field list as the struct, so a string
/// added here is checked by `placeholders_agree_across_languages` the moment
/// it is declared: there is no second list to keep in step.
macro_rules! string_table {
    (
        $(#[$table_meta:meta])*
        struct $table:ident {
            $( $(#[$field_meta:meta])* pub $field:ident : $ty:ty, )*
        }
    ) => {
        $(#[$table_meta])*
        pub struct $table {
            $( $(#[$field_meta])* pub $field: $ty, )*
        }

        impl $table {
            /// Every string in the table, each paired with the field it came
            /// from. Array fields expand to one entry per element.
            #[cfg(test)]
            fn strings(&self) -> Vec<(String, &'static str)> {
                let mut out = Vec::new();
                $( TableField::push(&self.$field, stringify!($field), &mut out); )*
                out
            }
        }
    };
}

/// A field of [`Tr`]: one string, or a fixed array of them.
#[cfg(test)]
trait TableField {
    fn push(&self, field: &str, out: &mut Vec<(String, &'static str)>);
}

#[cfg(test)]
impl TableField for &'static str {
    fn push(&self, field: &str, out: &mut Vec<(String, &'static str)>) {
        out.push((field.to_string(), self));
    }
}

#[cfg(test)]
impl<const N: usize> TableField for [&'static str; N] {
    fn push(&self, field: &str, out: &mut Vec<(String, &'static str)>) {
        for (index, string) in self.iter().enumerate() {
            out.push((format!("{field}[{index}]"), string));
        }
    }
}

string_table! {
/// Every user-facing string. Grouped by screen; `{x}` markers are filled at
/// the call site.
struct Tr {
    // Menu
    pub tagline: &'static str,
    pub menu_names: [&'static str; 9],
    pub menu_blurbs: [&'static str; 9],
    /// The daily row's blurb, which names the day it is offering.
    pub menu_daily_blurb: &'static str,
    pub menu_prompt: &'static str,
    pub title_achievements: &'static str,
    pub ach_names: [&'static str; 32],
    pub ach_descs: [&'static str; 32],
    pub stats_footer: &'static str,
    pub daily_best: &'static str,
    /// Field-guide names for the six crab kinds, spawn-mix order.
    pub crab_names: [&'static str; 6],
    /// One-word traits shown beside the value ("fast", "lure!", ...).
    pub crab_notes: [&'static str; 6],
    // The first-boot language picker
    /// The screen the game opens on the very first run, before any
    /// settings file exists. Every line of it is written in the language
    /// the cursor is resting on, which is the whole point: the player
    /// reads the one they want and stops there.
    pub title_pick_language: &'static str,
    /// Header and prompt of the new-version page.
    pub title_new_version: &'static str,
    pub prompt_new_version: &'static str,
    pub prompt_pick_language: &'static str,
    /// Said under the header. A first screen that looks like a decision
    /// is a first screen people hesitate over; this one is not.
    pub pick_language_later: &'static str,
    // Mode titles
    pub title_tide_pool: &'static str,
    pub title_beach_day: &'static str,
    pub title_turf_war: &'static str,
    pub title_replay: &'static str,
    pub title_online: &'static str,
    pub title_vs_ai: &'static str,
    pub title_editor: &'static str,
    pub title_playtest: &'static str,
    pub title_lobby: &'static str,
    pub title_settings: &'static str,
    pub title_match_setup: &'static str,
    // The replay library
    pub title_replays: &'static str,
    pub prompt_replays: &'static str,
    pub replays_empty: &'static str,
    /// The heading over the shelf of kept rounds.
    pub replays_heading: &'static str,
    /// Playback speed readout, e.g. "x2".
    pub replay_speed: &'static str,
    // The stuck-player hint
    pub hint_offer: &'static str,
    pub hint_showing: &'static str,
    // Stage select
    pub title_stages: &'static str,
    pub prompt_stages: &'static str,
    pub stage_progress: &'static str,
    /// The label in front of the difficulty key under the grid.
    pub stage_key: &'static str,
    /// Names the shelf of levels the player built, both over their rows on
    /// the grid and in front of their number in the caption.
    pub stage_custom: &'static str,
    /// The count over the trophy grid, same shape as [`Tr::stage_progress`].
    pub ach_progress: &'static str,
    pub stage_cleared: &'static str,
    pub stage_open: &'static str,
    pub stage_free: &'static str,
    pub stage_locked: &'static str,
    pub team_banner: &'static str,
    // Status line
    pub the_gulls: &'static str,
    pub tide_event: &'static str,
    /// Shown for the first moments of a lure: names the trigger.
    pub lure_started: &'static str,
    /// Shown for the rest of the lure: live countdown.
    pub lure_banner: &'static str,
    pub desync: &'static str,
    pub waiting_peer: &'static str,
    /// The round has stopped because one seat's input has not arrived.
    /// Lockstep runs a frame only when every seat has spoken, so a still
    /// picture is somebody else's trouble, and a table that is not told
    /// whose assumes the game has crashed.
    pub waiting_for: &'static str,
    pub saved_count: &'static str,
    pub signposts_count: &'static str,
    // Puzzle goals
    pub goal_bank: &'static str,
    pub goal_survive: &'static str,
    pub goal_golden: &'static str,
    // Prompts
    pub prompt_setup: &'static str,
    pub prompt_setup_full: &'static str,
    /// Shown for a moment when a placement is refused because the level's
    /// signposts are all out, which every other refusal is not.
    pub denied_no_posts: &'static str,
    pub prompt_setup_no_posts: &'static str,
    pub prompt_running: &'static str,
    pub prompt_won: &'static str,
    pub prompt_lost: &'static str,
    pub prompt_versus_short: &'static str,
    pub prompt_versus_local: &'static str,
    /// Shown instead of the key legend once the player has rebound keys.
    pub prompt_setup_custom: &'static str,
    pub prompt_versus_custom: &'static str,
    pub prompt_enter_menu: &'static str,
    /// The way off an online results card: the whole table returns to the
    /// lobby it was formed in, still connected.
    pub prompt_enter_lobby: &'static str,
    /// For the screens that only read out: both keys leave, and Esc is the
    /// one every other screen offers.
    pub prompt_esc_menu: &'static str,
    /// The music toggle, added to every play screen's prompt.
    pub prompt_mute: &'static str,
    /// How a kept round with no winner reads in the library. The file name
    /// says "draw" in English on every machine; this is the reading of it.
    pub replay_draw: &'static str,
    pub pause_title: &'static str,
    pub pause_continue: &'static str,
    pub pause_to_menu: &'static str,
    pub pause_quit: &'static str,
    /// Said on the versus screen when a round is copied as a code.
    pub round_copied: &'static str,
    pub round_code_bad: &'static str,
    // The new-version page (see `app::update`)
    /// Headline naming the newer release, `{v}` its version.
    pub update_title: &'static str,
    /// The line under it naming the version being played.
    pub update_have: &'static str,
    /// Stands in for the release notes when the release has none.
    pub update_no_notes: &'static str,
    /// The question, and its two answers: open the release page, or not
    /// now.
    pub update_question: &'static str,
    pub update_yes: &'static str,
    pub update_no: &'static str,
    /// Under the card: the asking can be turned off.
    pub update_note: &'static str,
    /// Menu notices after a yes: the browser took it, or there was no
    /// browser to give it to and here is the address instead.
    pub update_opened: &'static str,
    pub update_open_failed: &'static str,
    pub prompt_settings: &'static str,
    pub prompt_match_setup: &'static str,
    /// The same line with the cursor on a name row, where Enter types a
    /// name rather than starting the match.
    pub prompt_match_name: &'static str,
    /// Short seat label: "P1" / "J1" / "S1".
    pub player_label: &'static str,
    // Tide events
    pub events: [&'static str; 8],
    /// What each event actually does, in event order: the line under the
    /// headline on the centre-screen announcement.
    pub event_blurbs: [&'static str; 8],
    // Centre-screen announcements
    pub ann_lure: &'static str,
    pub ann_lure_sub: &'static str,
    pub ann_surge: &'static str,
    pub ann_surge_sub: &'static str,
    // Results
    pub tide_is_in: &'static str,
    pub wins: &'static str,
    pub dead_heat: &'static str,
    /// "{t} wins!", where {t} is the team named by its members.
    pub team_wins: &'static str,
    pub tag_you: &'static str,
    pub tag_ai: &'static str,
    pub haul: &'static str,
    /// Where the round's highlight reel was written.
    pub highlight_saved: &'static str,
    pub all_safe: &'static str,
    /// The heading over the puzzle loss card: the run is over, and the
    /// board held under it says where.
    pub crabs_lost: &'static str,
    pub last_level: &'static str,
    /// The heading over the last level's card: the run is finished, not
    /// merely another level won.
    pub campaign_done: &'static str,
    // Settings rows
    /// Headings on the settings card.
    pub set_group_controls: &'static str,
    pub set_group_sound: &'static str,
    pub set_group_round: &'static str,
    pub set_group_look: &'static str,
    pub set_group_danger: &'static str,
    /// The game itself, as distinct from any round of it: the update check.
    pub set_group_game: &'static str,
    /// "P{n} controls" and the three answers it can give.
    pub set_seat_input: &'static str,
    pub val_input_auto: &'static str,
    pub val_input_keys: &'static str,
    pub val_input_pad: &'static str,
    pub set_commit_keys: &'static str,
    pub set_repeat_delay: &'static str,
    pub set_repeat_rate: &'static str,
    pub set_music_on: &'static str,
    pub set_music: &'static str,
    pub set_sfx_on: &'static str,
    pub set_sfx: &'static str,
    pub set_speed: &'static str,
    pub set_versus_mode: &'static str,
    pub set_language: &'static str,
    pub set_keyboard: &'static str,
    pub set_rumble: &'static str,
    pub set_deadzone: &'static str,
    pub set_palette: &'static str,
    pub set_ui_scale: &'static str,
    pub set_reduced_motion: &'static str,
    /// Settings row: whether start-up asks GitHub for a newer release.
    pub set_update_check: &'static str,
    /// Settings row: how many finished rounds the shelf keeps.
    pub set_replay_cap: &'static str,
    pub set_key_bindings: &'static str,
    pub set_reset_progress: &'static str,
    pub val_palette_classic: &'static str,
    pub val_palette_safe: &'static str,
    pub val_open: &'static str,
    /// The keyboard row's "work it out yourself".
    pub val_auto: &'static str,
    /// The reset row's three states: offering, awaiting the second press,
    /// and done.
    pub val_reset: &'static str,
    pub val_reset_confirm: &'static str,
    pub val_reset_done: &'static str,
    // Key-binding screen
    pub title_controls: &'static str,
    pub prompt_controls: &'static str,
    pub ctl_seat: &'static str,
    /// Headings on the key-binding card.
    pub ctl_group_seat: &'static str,
    pub ctl_group_move: &'static str,
    pub ctl_group_place: &'static str,
    pub ctl_group_posts: &'static str,
    /// The key column while a row is listening for a keypress.
    pub ctl_listening: &'static str,
    /// The key column on the reset row, which is a door not a binding.
    pub ctl_reset_key: &'static str,
    pub ctl_press_key: &'static str,
    pub ctl_taken: &'static str,
    pub ctl_reset: &'static str,
    /// One label per bindable action, in `binds::Action::ALL` order.
    pub bind_actions: [&'static str; 10],
    pub pad_help1: &'static str,
    pub pad_help2: &'static str,
    pub match_pad_joined: &'static str,
    pub match_pad_hint: &'static str,
    pub val_arrows: &'static str,
    pub val_ijkl: &'static str,
    pub val_on: &'static str,
    pub val_off: &'static str,
    /// Team modes in `TeamMode::ALL` order: free-for-all, pairs, trios.
    pub team_modes: [&'static str; 3],
    // Match setup rows
    /// The heading over the match setup card.
    pub match_heading: &'static str,
    pub match_players: &'static str,
    pub match_ai: &'static str,
    pub human_one: &'static str,
    pub human_many: &'static str,
    pub match_ai_level: &'static str,
    pub bot_levels: [&'static str; 3],
    pub match_map: &'static str,
    pub match_gulls: &'static str,
    pub match_round: &'static str,
    pub match_mode: &'static str,
    /// Seat-naming row: its label, what an unnamed seat shows, and the hint
    /// shown while the row is being typed into.
    pub match_name: &'static str,
    pub match_name_empty: &'static str,
    pub match_name_typing: &'static str,
    pub mode_names: [&'static str; 3],
    pub tour_round: &'static str,
    pub tour_champion: &'static str,
    pub tour_next: &'static str,
    // Versus sidebar event log
    pub log_raid: &'static str,
    pub log_golden: &'static str,
    pub log_lure: &'static str,
    pub log_tier: &'static str,
    pub log_gull: &'static str,
    pub log_surge: &'static str,
    pub map_names: [&'static str; 6],
    /// How a handmade beach reads on the map dial.
    pub map_custom: &'static str,
    /// The same, for one too big to fit an invitation: it plays at this
    /// table and cannot travel to another.
    pub map_custom_local: &'static str,
    /// Said beside the map dial when the player has beaches saved and none
    /// of them has a castle for every seat at this table.
    pub map_none_seats: &'static str,
    pub gull_names: [&'static str; 3],
    pub round_names: [&'static str; 3],
    // Lobby
    pub lobby_hosting: &'static str,
    pub lobby_hosting_noport: &'static str,
    pub lobby_rivals_one: &'static str,
    pub lobby_rivals_many: &'static str,
    pub lobby_aboard: &'static str,
    /// Appended to the host's status when AI seats will fill the rest.
    pub lobby_ai_seats: &'static str,
    /// Appended to the host's status when people are watching.
    pub lobby_watchers: &'static str,
    /// Shown to a peer that joined to watch rather than play.
    pub lobby_watching: &'static str,
    /// Shown once W has armed the next join to be a watch.
    pub lobby_watch_armed: &'static str,
    /// The header while watching someone else's match.
    pub title_watching: &'static str,
    pub lobby_listening: &'static str,
    pub lobby_none_yet: &'static str,
    /// Tacked onto a beach whose round has already begun, after its seat
    /// count: pressing its number queues for the next round rather than
    /// joining this one.
    pub lobby_in_progress: &'static str,
    /// A beach with no chair left, this round or the next.
    pub lobby_full_tag: &'static str,
    /// Shown under the chat feed when the keyboard is free.
    pub lobby_chat_hint: &'static str,
    /// Asked before a player may host or join, and before a beach is put
    /// on the air. An empty answer is refused rather than accepted.
    pub lobby_ask_player_name: &'static str,
    pub lobby_ask_game_name: &'static str,
    pub lobby_needs_name: &'static str,
    /// Asked by J: the address of a beach no beacon reached, because the
    /// network drops broadcasts or the host is on the other side of one.
    pub lobby_ask_address: &'static str,
    /// What was typed there is not `ip:port`. The example is half the
    /// message: "not an address" tells nobody what one looks like.
    pub lobby_bad_address: &'static str,
    /// The host's own address, shown while hosting so it can be read out
    /// to somebody who has to dial it by hand.
    pub lobby_hosting_at: &'static str,
    /// Dialled, and nobody has answered yet. Not the same as being at a
    /// beach, and saying so is the difference between "they have not
    /// started yet" and "there is nothing at that address".
    pub lobby_calling: &'static str,
    /// And nobody ever did.
    pub lobby_no_answer: &'static str,
    /// Headings over the two lobby panels.
    pub lobby_card_beaches: &'static str,
    pub lobby_card_chat: &'static str,
    /// Said in the feed by the lobby itself, not by a player.
    pub lobby_joined: &'static str,
    pub lobby_left: &'static str,
    /// Heading over the table, once you are at one.
    pub lobby_card_players: &'static str,
    /// Heading over the host's dials, which every peer can read.
    pub lobby_card_terms: &'static str,
    /// A player stopped sending and an AI took their chair, mid-round.
    pub online_seat_abandoned: &'static str,
    /// The host stopped sending, which no round survives: it relays
    /// every input and calls every round. Read on the menu the joiners
    /// are walked back to.
    pub online_host_gone: &'static str,
    /// Waiting for a match that is already under way: next in line, or
    /// with `{n}` people ahead.
    pub lobby_queued_next: &'static str,
    pub lobby_queued_behind: &'static str,
    /// Every chair at that beach is taken, this round and the next.
    pub lobby_beach_full: &'static str,
    pub lobby_join_list: &'static str,
    /// The same line when there is exactly one, which "1 beaches" is not.
    pub lobby_join_list_one: &'static str,
    pub lobby_broadcasting: &'static str,
    pub lobby_aboard_prompt: &'static str,
    pub lobby_could_not_host: &'static str,
    pub lobby_could_not_join: &'static str,
    pub lobby_version_clash: &'static str,
    // Editor
    pub ed_prompt: &'static str,
    /// The editor's brush palette, in [`Brush::ALL`] order.
    pub ed_brushes: [&'static str; 9],
    /// The editor's readout of what the cursor is standing on.
    pub ed_under: &'static str,
    pub ed_playtest_prompt: &'static str,
    pub ed_solvable: &'static str,
    pub ed_solvable_free: &'static str,
    pub ed_not_solvable: &'static str,
    pub ed_solver_gave_up: &'static str,
    pub ed_validating: &'static str,
    pub ed_already_validating: &'static str,
    /// What a freshly opened editor calls its level, and the row that
    /// lets it be renamed.
    /// Said after the beach is resized, which starts a fresh one.
    pub ed_resized: &'static str,
    pub ed_default_name: &'static str,
    pub ed_name_row: &'static str,
    pub ed_naming: &'static str,
    pub ed_saved_to: &'static str,
    pub ed_save_failed: &'static str,
    pub code_kind_beach: &'static str,
    pub code_kind_level: &'static str,
    pub code_kind_round: &'static str,
    pub code_wrong_kind: &'static str,
    pub code_copied: &'static str,
    pub code_copy_failed: &'static str,
    pub code_none_pasted: &'static str,
    pub code_level_checking: &'static str,
    pub code_level_bad: &'static str,
    pub code_round_saved: &'static str,
    pub code_round_bad: &'static str,
    pub ed_posts: &'static str,
    pub ed_fresh_sand: &'static str,
    pub ed_back: &'static str,
    pub ed_wrap_on: &'static str,
    pub ed_wrap_off: &'static str,
    pub ed_gulls_every: &'static str,
    pub ed_gulls_off: &'static str,
    /// Heading over the editor's puzzle/beach toggle.
    pub ed_kind_row: &'static str,
    /// The editor's status line for a beach, where the granted inventory
    /// means nothing: how many seats it has castles for.
    pub ed_seats: &'static str,
    /// Said on saving a level that will appear on neither list.
    pub ed_puzzle_no_crabs: &'static str,
    /// What is being built, as it reads in the editor's title bar.
    pub ed_kind_puzzle: &'static str,
    pub ed_kind_arena: &'static str,
    /// Said when the toggle is turned, one line each way: the kinds go to
    /// different lists, so the answer has to say where this one now lands.
    pub ed_now_puzzle: &'static str,
    pub ed_now_arena: &'static str,
    /// What checking a beach says, in place of the solver's verdict.
    pub ed_arena_ok: &'static str,
    pub ed_arena_needs_seats: &'static str,
    pub ed_arena_no_crabs: &'static str,
}
}

mod de;
mod en;
mod es;
mod fr;
mod it;
mod ja;
mod levels;
#[cfg(test)]
pub mod metrics;
mod nl;
mod ru;

pub use de::DE;
pub use en::EN;
pub use es::ES;
pub use fr::FR;
pub use it::IT;
pub use ja::JA;
use levels::{LEVEL_HINTS, LEVEL_NAMES};
pub use nl::NL;
pub use ru::RU;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::cycle::Cycle;

    /// Every parameterized template must carry the same `{markers}` in
    /// every language: a translation that drops one silently loses data
    /// (a lure banner without its {p}, a score line without its {b}).
    ///
    /// Both lists come from declarations rather than by hand - the fields
    /// from the struct, the languages from [`ALL_LANGS`] - so this covers
    /// the whole table in every language, and keeps covering both as they
    /// grow.
    #[test]
    fn placeholders_agree_across_languages() {
        fn markers(s: &str) -> std::collections::BTreeSet<String> {
            let mut out = std::collections::BTreeSet::new();
            let mut rest = s;
            while let Some(open) = rest.find('{') {
                let Some(close) = rest[open..].find('}') else {
                    break;
                };
                out.insert(rest[open + 1..open + close].to_string());
                rest = &rest[open + close + 1..];
            }
            out
        }
        let english = EN.strings();
        for lang in ALL_LANGS {
            for ((field, en), (_, translated)) in english.iter().zip(lang.tr().strings()) {
                assert_eq!(
                    markers(translated),
                    markers(en),
                    "{field} placeholders diverge in {lang:?}"
                );
            }
        }
    }

    /// Every Japanese character the game can put on screen has to be in
    /// the subset that ships, or that word draws as a row of nothing.
    /// Reword a Japanese string and this is the test that says to run
    /// `tools/gen_jp_font.py` again.
    ///
    /// That face is a subset - a whole CJK font is twenty megabytes for
    /// the few hundred characters this game says - and a character left
    /// out of it draws as nothing at all, with no warning anywhere. So
    /// this reads the `cmap` of the very bytes that ship and asks it.
    ///
    /// Only the characters DejaVu cannot draw are asked for, which is the
    /// line the tool cuts on too: the Latin and the digits in a Japanese
    /// prompt are drawn by the font the rest of the game uses.
    #[test]
    fn the_japanese_font_carries_every_character_the_tables_use() {
        let covered = metrics::characters_in_font(include_bytes!(
            "../../../assets/fonts/NotoSansMonoCJKjp-Subset.otf"
        ));
        assert!(
            covered.len() > 300,
            "only {} characters shipped",
            covered.len()
        );
        let column = Lang::Ja.column();
        let mut said: String = JA.strings().iter().map(|(_, line)| *line).collect();
        for row in &LEVEL_NAMES {
            said.push_str(row[column]);
        }
        for row in &LEVEL_HINTS {
            said.push_str(row[1 + column]);
        }
        // And the language's own name, which the picker draws in every
        // language, before a Japanese string has ever been asked for.
        said.push_str(Lang::Ja.native_name());
        for ch in said.chars().filter(|ch| *ch > '\u{4ff}') {
            assert!(covered.contains(&ch), "{ch:?} is not in the shipped subset");
        }
    }

    /// The old hand-written list checked 53 fields; the generated walk must
    /// reach far more than that, or the macro is not expanding what we think.
    #[test]
    fn the_walk_reaches_the_whole_table() {
        let strings = EN.strings();
        assert!(strings.len() > 300, "only {} strings walked", strings.len());
        assert!(strings.iter().any(|(field, _)| field == "tagline"));
        assert!(strings.iter().any(|(field, _)| field == "ach_names[31]"));
    }

    #[test]
    fn language_keys_round_trip() {
        for lang in ALL_LANGS {
            assert_eq!(Lang::from_key(lang.key()), lang);
        }
        assert_eq!(Lang::from_key("??"), Lang::En);
    }

    #[test]
    fn cycling_visits_every_language() {
        let mut lang = Lang::En;
        for _ in 0..ALL_LANGS.len() {
            lang = lang.cycled(true);
        }
        assert_eq!(lang, Lang::En);
        assert_eq!(Lang::En.cycled(false), *ALL_LANGS.last().unwrap());
    }

    #[test]
    fn builtin_level_names_translate_and_custom_pass_through() {
        assert_eq!(Lang::Fr.level_name("Gull Storm"), "Tempête de mouettes");
        assert_eq!(Lang::De.level_name("Gull Storm"), "Möwensturm");
        assert_eq!(Lang::Es.level_name("Gull Storm"), "Tormenta de gaviotas");
        assert_eq!(Lang::It.level_name("Gull Storm"), "Tempesta di gabbiani");
        assert_eq!(Lang::Nl.level_name("Gull Storm"), "Meeuwenstorm");
        assert_eq!(Lang::Ru.level_name("Gull Storm"), "Чаячий шторм");
        assert_eq!(Lang::Ja.level_name("Gull Storm"), "カモメの嵐");
        assert_eq!(Lang::En.level_name("Gull Storm"), "Gull Storm");
        assert_eq!(Lang::Fr.level_name("My Custom Beach"), "My Custom Beach");
    }

    /// Every campaign and challenge level ships a name in every language.
    #[test]
    fn every_builtin_level_is_covered() {
        for level in crate::sim::campaign_levels()
            .iter()
            .chain(crate::sim::challenge_levels().iter())
        {
            assert!(
                LEVEL_NAMES.iter().any(|row| row[0] == level.name),
                "missing translation row for level {:?}",
                level.name
            );
        }
    }

    /// No column of either level table may be left blank, which is what a
    /// row padded out to reach the new width would look like: the lookup
    /// would find the row, hand back an empty string, and draw a level
    /// with no name at all.
    #[test]
    fn every_level_column_is_filled() {
        for lang in ALL_LANGS {
            for row in &LEVEL_NAMES {
                assert!(
                    !row[lang.column()].trim().is_empty(),
                    "{:?} has no {lang:?} name",
                    row[0]
                );
            }
            for row in &LEVEL_HINTS {
                assert!(
                    !row[1 + lang.column()].trim().is_empty(),
                    "{:?} has no {lang:?} hint",
                    row[0]
                );
            }
        }
    }
}
