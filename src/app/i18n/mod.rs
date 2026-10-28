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

/// Every language, in display order. The level tables order their
/// columns the same way, English first, so a language's column there is
/// its [`Cycle::index`](crate::app::cycle::Cycle::index).
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

    /// Teaching hint for a shipped level, if it has one. Column 0 is the
    /// English name the row is keyed by, so the hints sit one place further
    /// along. Keyed by name alone, so a level on a list is asked about
    /// through [`crate::app::Campaign::hint`], which knows which levels
    /// are shipped ones.
    pub fn level_hint(self, en_name: &str) -> Option<&'static str> {
        LEVEL_HINTS
            .iter()
            .find(|row| row[0] == en_name)
            .map(|row| row[1 + crate::app::cycle::Cycle::index(self)])
    }

    /// Localized name for a built-in level; other names pass through.
    /// English is both the key and its own translation, so it needs no
    /// lookup at all. A player's level can share a shipped one's name, so
    /// a level on a list is named through
    /// [`crate::app::Campaign::shown_name`], which only asks this for the
    /// shipped ones.
    pub fn level_name(self, en: &str) -> &str {
        if self == Lang::En {
            return en;
        }
        LEVEL_NAMES
            .iter()
            .find(|row| row[0] == en)
            .map_or(en, |row| row[crate::app::cycle::Cycle::index(self)])
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
            pub(crate) fn strings(&self) -> Vec<(String, &'static str)> {
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
    pub menu_names: [&'static str; 8],
    pub menu_blurbs: [&'static str; 8],
    /// The daily row's blurb, which names the day it is offering.
    pub menu_daily_blurb: &'static str,
    pub menu_prompt: &'static str,
    pub title_achievements: &'static str,
    pub ach_names: [&'static str; 48],
    pub ach_descs: [&'static str; 48],
    pub stats_footer: &'static str,
    pub daily_best: &'static str,
    /// Short traits shown beside the value ("fast", "jackpot", ...). The
    /// molting crab's says its lure is followed by a rest of `{s}`
    /// seconds, filled from `LURE_COOLDOWN_SECONDS`.
    pub crab_notes: [&'static str; 6],
    // The first-boot language picker
    /// The screen the game opens on the very first run, before any
    /// settings file exists. Every line of it is written in the language
    /// the cursor is resting on, so the player reads the one they want and
    /// stops there.
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
    /// The two states on the replay transport bar.
    pub replay_playing: &'static str,
    pub replay_paused: &'static str,
    /// The transport line under a recording being watched.
    pub prompt_replay_transport: &'static str,
    // The stuck-player hint
    pub hint_offer: &'static str,
    pub hint_showing: &'static str,
    // Stage select
    pub title_stages: &'static str,
    pub prompt_stages: &'static str,
    /// The same on a stage of the player's own, which E opens in the editor
    /// and F3 copies the code of.
    pub prompt_stages_custom: &'static str,
    /// Said on the stage list while co-op is on. `{p2}` is the second
    /// seat's label: true on a co-op stage and on a Tide Pool one alike.
    pub stage_coop_on: &'static str,
    /// The co-op stages' section: its heading on the stage list, and the
    /// header's and the cards' word for where a co-op stage sits.
    pub stage_coop: &'static str,
    /// Enter on a co-op stage with nobody seated as P2 (`{p2}`), and the
    /// key that seats them: the stage needs two and is not opened.
    pub stage_needs_two: &'static str,
    /// A co-op stage's caption tail: two players, and each hand's arrows
    /// (`{p1}` `{a}`, `{p2}` `{b}`).
    pub stage_open_hands: &'static str,
    /// A co-op stage's arrow count in the status line: each hand's arrows
    /// down of those it holds (`{p1}` `{a}/{b}`, `{p2}` `{c}/{d}`).
    pub hands_count: &'static str,
    /// The claw rule as a co-op stage teaches it under the header: whose
    /// arrows turn which crabs (`{p1}` the left claw, `{p2}` the right).
    pub coop_rule: &'static str,
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
    /// [`Tr::stage_locked`] on the co-op shelf, which counts from one of
    /// its own: the co-op stage (`{n}`) that opens this one.
    pub stage_locked_coop: &'static str,
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
    /// picture is somebody else's trouble, and the table is told whose.
    pub waiting_for: &'static str,
    pub saved_count: &'static str,
    pub signposts_count: &'static str,
    // Puzzle goals
    pub goal_bank: &'static str,
    pub goal_survive: &'static str,
    pub goal_golden: &'static str,
    // Prompts
    pub prompt_setup: &'static str,
    /// [`Tr::prompt_setup`] under the mirrored placing keys
    /// (`CommitScheme::Wasd`): the arrow keys move, WASD places.
    pub prompt_setup_swapped: &'static str,
    /// The setup prompt in co-op: both players' keys, one line.
    pub prompt_setup_coop: &'static str,
    /// [`Tr::prompt_setup_coop`] with P1 on the mirrored placing keys.
    pub prompt_setup_coop_swapped: &'static str,
    pub prompt_setup_full: &'static str,
    /// Shown for a moment when a placement is refused because the level's
    /// signposts are all out, which every other refusal is not.
    pub denied_no_posts: &'static str,
    pub prompt_setup_no_posts: &'static str,
    pub prompt_running: &'static str,
    pub prompt_won: &'static str,
    /// The won prompt for a stage whose goal lets crabs go (bank a number,
    /// bank the golden one), where "all crabs safe" is not what happened.
    pub prompt_won_goal: &'static str,
    pub prompt_lost: &'static str,
    /// The loss prompt under `goal_missed`.
    pub prompt_lost_goal: &'static str,
    pub prompt_versus_short: &'static str,
    pub prompt_versus_local: &'static str,
    /// The two versus legends with P1 on the mirrored placing keys.
    pub prompt_versus_short_swapped: &'static str,
    pub prompt_versus_local_swapped: &'static str,
    /// Shown instead of the key legend once the player has rebound keys.
    pub prompt_setup_custom: &'static str,
    pub prompt_versus_custom: &'static str,
    pub prompt_enter_menu: &'static str,
    /// The way off an online results card: the whole table returns to the
    /// lobby it was formed in, still connected.
    pub prompt_enter_lobby: &'static str,
    /// R on a couch round's results card: the same table again.
    pub prompt_rematch: &'static str,
    /// The same key after a round of a series: the whole series again, from
    /// its first round.
    pub prompt_rematch_series: &'static str,
    /// R pressed once on the card of a series still being played: R again
    /// restarts the series.
    pub prompt_rematch_series_confirm: &'static str,
    /// The results card's offer to put the round on the replay shelf,
    /// led by `replays::KEEP_CAP`: nothing is kept unless asked.
    pub prompt_keep_replay: &'static str,
    /// What the offer turns into once the round is on the shelf.
    pub replay_kept: &'static str,
    /// The way out for somebody who is only watching a round: Escape
    /// opens their own card, since a spectator cannot pause a table it is
    /// not playing at, and Enter does nothing at all.
    pub prompt_esc_menu: &'static str,
    /// The trophy shelf: it scrolls now that it is longer than the screen,
    /// so its prompt says so rather than only how to leave. The last reader
    /// of the old read-out-only `prompt_esc_menu`, which went with it.
    pub prompt_achievements: &'static str,
    /// The music toggle, added to every play screen's prompt.
    pub prompt_mute: &'static str,
    /// How a kept round with no winner reads in the library. The file name
    /// says "draw" in English on every machine; this is the reading of it.
    pub replay_draw: &'static str,
    /// After a kept round's winner when the game's AI took it, with the
    /// level it played at: `{level}` is one of `bot_levels`. Opens as
    /// `tag_ai` does, so the AI is called one thing on every screen.
    pub replay_tag_ai_level: &'static str,
    /// After a kept round's winner when a bot over the protocol took it:
    /// the word the match setup calls one (`match_bot`), in brackets.
    pub replay_tag_bot: &'static str,
    /// How many played a kept round, `{n}` from 2 to `MAX_PLAYERS`. Worded
    /// to read right at every count, Russian's two plural forms included.
    pub replay_players: &'static str,
    /// What a kept round was won with: `{n}` points, the winner's score
    /// or the winning team's total. Short, to sit in a column of its own.
    pub replay_points: &'static str,
    pub pause_title: &'static str,
    /// The same card's title for a spectator, whose match does not stop.
    pub pause_menu_title: &'static str,
    pub pause_continue: &'static str,
    /// The pause card's row over a couch round: this round again from its
    /// countdown.
    pub pause_restart: &'static str,
    pub pause_to_menu: &'static str,
    pub pause_quit: &'static str,
    /// The main menu's quit card, which a pad's Start raises: the question
    /// it asks.
    pub quit_ask: &'static str,
    /// The prompt line while the pause card has the keyboard: its keys,
    /// not the beach's.
    pub prompt_paused: &'static str,
    /// The heading on the spectators' event list, and the two lines their prompt
    /// carries: the offer to call one, and the wait after it has.
    pub spectator_call_title: &'static str,
    pub spectator_call_hint: &'static str,
    pub spectator_called: &'static str,
    /// What the crowd is told while a vote runs: how long is left to join
    /// one, and how long until the next may be called. Both in seconds.
    pub spectator_vote_open: &'static str,
    pub spectator_wait: &'static str,
    /// How many are watching, said to the whole table so a called event
    /// is not the first anyone hears of a crowd.
    pub spectator_count: &'static str,
    /// The heading over the seats a spectator can call to win.
    pub spectator_pick_title: &'static str,
    /// The spectator's prompt while the calls are open, `{n}` seconds left.
    pub spectator_pick_hint: &'static str,
    /// The same once a call is made: `{p}` whom, `{n}` seconds left to change it.
    pub spectator_picked: &'static str,
    /// The crowd's calls, `{l}` the list ("Maya ×2 · Theo ×1"): said to the
    /// feed as the calls close, and on the results card.
    pub crowd_picked: &'static str,
    /// A spectator's call came in: `{a}` right of `{b}` made this session.
    pub call_right: &'static str,
    /// It did not: `{p}` whom they called, then the same record.
    pub call_wrong: &'static str,
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
    /// The keys of match setup's three steps: the game, the map, the
    /// players.
    pub prompt_setup_game: &'static str,
    pub prompt_setup_map: &'static str,
    pub prompt_setup_players: &'static str,
    /// Step 3's keys on player 1's row, which A/D does not turn: the
    /// person working the menu stays a person.
    pub prompt_setup_players_p1: &'static str,
    /// Step 3's partner key, after the line's other keys under a team mode.
    pub prompt_setup_partners: &'static str,
    /// The prompt while a line is being typed and Esc throws it away: the
    /// screen's own keys are letters until the box closes.
    pub prompt_typing: &'static str,
    /// The main menu's quit card at the keyboard: Esc puts it away, Enter
    /// leaves the game.
    pub prompt_quitting: &'static str,
    /// The same while every way out keeps what was typed, as a seat's
    /// name does on the match setup.
    pub prompt_typing_keep: &'static str,
    /// Short seat label: "P1" / "J1" / "S1".
    pub player_label: &'static str,
    // Tide events
    pub events: [&'static str; 9],
    /// What each event actually does, in event order: the line under the
    /// headline on the centre-screen announcement.
    pub event_blurbs: [&'static str; 9],
    /// Castle Swap's line in place of its blurb when the seat that spun it
    /// keeps its castle (three castles or more). `{p}` that seat.
    pub event_blurb_swap_kept: &'static str,
    /// Over a spun event's headline: whose sparkling crab started it.
    /// `{p}` that seat.
    pub ann_spun_by: &'static str,
    /// The time left of a timed event, under its name in the feed card.
    /// `{s}` whole seconds, rounded up.
    pub event_left: &'static str,
    // Centre-screen announcements
    pub ann_lure: &'static str,
    /// `{p}` the lure's owner, `{n}` seconds it runs.
    pub ann_lure_sub: &'static str,
    pub ann_surge: &'static str,
    pub ann_surge_sub: &'static str,
    /// A called golden crab is about to come out of a spawner hole.
    pub ann_golden: &'static str,
    /// Under it; `{n}` is what the crab is worth.
    pub ann_golden_sub: &'static str,
    // Results
    pub tide_is_in: &'static str,
    pub wins: &'static str,
    pub dead_heat: &'static str,
    /// "{t} wins!", where {t} is the team named by its members.
    pub team_wins: &'static str,
    pub tag_you: &'static str,
    pub tag_ai: &'static str,
    pub haul: &'static str,
    /// `haul` for exactly one crab, which most of these languages say
    /// differently.
    pub haul_one: &'static str,
    /// The round's awards under the standings (`app::awards`). `{p}` is
    /// who, one name or several joined by " & " on a tie; `{n}` the figure.
    /// The round winner who was furthest behind the leader, and by how much.
    pub award_comeback: &'static str,
    /// The most banked during one of your own lures.
    pub award_lure: &'static str,
    /// The most arrows planted.
    pub award_arrows: &'static str,
    /// The award for the seat whose steered gulls took the most from other
    /// castles; `{n}` is the points.
    pub award_stolen: &'static str,
    /// The most banked score lost, to gulls or to the left claw.
    pub award_hit: &'static str,
    /// Where the round's highlight reel was written.
    pub highlight_saved: &'static str,
    pub all_safe: &'static str,
    /// The won card's headline for the same stages as `prompt_won_goal`.
    pub goal_met: &'static str,
    /// The heading over the puzzle loss card: the run is over, and the
    /// board held under it says where.
    pub crabs_lost: &'static str,
    /// The same heading for a stage that asks for a number of crabs or the
    /// golden one: those are lost only on the clock, with every crab on
    /// the beach maybe still safe.
    pub goal_missed: &'static str,
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
    /// Settings row: borderless fullscreen or a window.
    pub set_fullscreen: &'static str,
    pub set_ui_scale: &'static str,
    pub set_reduced_motion: &'static str,
    /// Settings row: smoothing the edges of the beach (multisampling).
    pub set_antialiasing: &'static str,
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
    /// The first press of a reset, waiting for the second: `{k}` is the
    /// key, or for a pad's player the button, that takes it up.
    pub ctl_reset_confirm: &'static str,
    /// One label per bindable action, in `binds::Action::ALL` order.
    pub bind_actions: [&'static str; 10],
    pub val_arrows: &'static str,
    pub val_ijkl: &'static str,
    /// The mirrored placing keys: WASD places, the arrow keys move.
    pub val_wasd: &'static str,
    pub val_on: &'static str,
    pub val_off: &'static str,
    /// Team modes in `TeamMode::ALL` order: free-for-all, pairs, trios.
    pub team_modes: [&'static str; 3],
    /// The same modes by name alone, in the same order, for a line that
    /// has no room for what each one means.
    pub team_mode_names: [&'static str; 3],
    /// The host's teams row when the mode dialled does not fit the table
    /// yet: `{m}` the mode dialled (`team_mode_names`), `{n}` the seats it
    /// starts at. Until then the round is scored free-for-all.
    pub team_mode_waits: &'static str,
    // Match setup rows
    pub match_players: &'static str,
    /// The lobby's AI row.
    pub match_ai: &'static str,
    pub match_ai_level: &'static str,
    pub bot_levels: [&'static str; 3],
    pub match_map: &'static str,
    pub match_gulls: &'static str,
    pub match_round: &'static str,
    /// The arrows-per-player dial on the match setup card.
    pub match_posts: &'static str,
    /// The word that starts a round, after the count (see `countdown`).
    pub countdown_go: &'static str,
    /// The rules tips, one shown under the count before a versus round
    /// (`countdown::tips`). `{a}` is a castle's score and `{b}` what a raid on
    /// it takes.
    pub tip_raid: &'static str,
    /// `{n}` seconds an arrow stands.
    pub tip_fade: &'static str,
    /// `{n}` arrows a player may have standing.
    pub tip_cap: &'static str,
    /// `{n}` seconds a lure runs.
    pub tip_lure: &'static str,
    /// `{n}` points a golden crab banks for on this beach.
    pub tip_golden: &'static str,
    pub tip_events: &'static str,
    pub match_mode: &'static str,
    /// What a seat with no name shows on step 3 of match setup, and the
    /// hint shown while one is being typed.
    pub match_name_empty: &'static str,
    pub match_name_typing: &'static str,
    // Match setup's three steps
    /// The step indicator: the game, the map, the players.
    pub setup_steps: [&'static str; 3],
    /// The asides after step 1's players and AI level values.
    pub setup_players_aside: &'static str,
    pub setup_ai_aside: &'static str,
    pub setup_scoring: &'static str,
    pub setup_series: &'static str,
    /// What each of step 1's rows is for, in `GameRow::ALL` order. The
    /// players one takes `{a}` and `{b}`, the fewest and most seats.
    pub setup_game_notes: [&'static str; 7],
    /// Step 1's button, which Enter presses.
    pub setup_next: &'static str,
    /// Step 2's badges: a fixed map, one drawn anew each round, the
    /// player's own, picked at start.
    pub setup_badges: [&'static str; 4],
    /// Step 2's card names in `MapChoice` order, the Random card's in the
    /// handmade slot.
    pub setup_map_names: [&'static str; 7],
    /// A card's size line: `{w}x{h}` and the seats `{a}` to `{b}`, or one
    /// count `{n}`; the open ocean's says it has no edges.
    pub setup_map_sub: &'static str,
    pub setup_map_sub_wrap: &'static str,
    pub setup_map_sub_exact: &'static str,
    /// The Random card's size line: any map seating the `{n}` at the table.
    pub setup_random_sub: &'static str,
    /// Added to a handmade map's size line when it is too big to send.
    pub setup_local_only: &'static str,
    /// The pill over a greyed card: it seats `{s}`, the table is `{n}`.
    pub setup_seats_not: &'static str,
    /// The same for a handmade map whose castles skip one.
    pub setup_seats_none: &'static str,
    /// What each built-in map is, in `MapChoice` order, for the line under
    /// the cards.
    pub setup_map_about: [&'static str; 6],
    /// The handmade versus maps' names, in `MapChoice::handmade` order:
    /// the card's name, and with its size the dial's.
    pub setup_versus_maps: [&'static str; 11],
    /// What each handmade versus map is, for the line under the cards.
    pub setup_versus_about: [&'static str; 11],
    /// The Surprise card: its name, its size line, and the line under the
    /// cards (Lanes is dealt at `{n}` players or fewer).
    pub setup_surprise: &'static str,
    pub setup_surprise_sub: &'static str,
    pub setup_surprise_note: &'static str,
    /// The line under a greyed handmade map: `{m}` is built for the tables
    /// `{s}` (as "2/4"), and the Players row has to name one of them.
    pub setup_greyed_tables: &'static str,
    pub setup_custom_about: &'static str,
    /// Step 2's section headings, in `MapSection::ALL` order: the maps
    /// that ship with the game, the player's own, Random and Surprise.
    pub setup_sections: [&'static str; 3],
    /// The card in the player's own section while they have no map: its
    /// name, its line, and the line under the cards, naming the menu's
    /// editor entry `{e}` and the editor's list of levels `{l}`.
    pub setup_make_one: &'static str,
    pub setup_make_one_sub: &'static str,
    pub setup_make_one_note: &'static str,
    pub setup_local_note: &'static str,
    /// How many a map seats, `{a}` to `{b}` or `{n}`.
    pub setup_seats_range: &'static str,
    pub setup_seats_exact: &'static str,
    /// The Random card's line: a map seating `{n}`, drawn every round.
    pub setup_random_note: &'static str,
    /// The line under a greyed card: `{m}` seats `{s}`.
    pub setup_greyed_note: &'static str,
    /// The line under a pick the player count moved: `{old}` does not
    /// seat `{n}`, so `{new}` is picked.
    pub setup_map_moved: &'static str,
    /// Step 3's column heads: seat, name, played by.
    pub setup_columns: [&'static str; 3],
    /// Who plays a seat: `{p}`'s keys, controller `{n}`, nobody yet, `{p2}`
    /// or a pad, the AI at `{l}`, a bot.
    pub setup_by_keys: &'static str,
    pub setup_by_pad: &'static str,
    pub setup_by_open: &'static str,
    pub setup_by_open_keys: &'static str,
    pub setup_by_ai: &'static str,
    pub setup_by_bot: &'static str,
    /// A seat's status: ready (at the keys, or on a pad dealt the seat),
    /// joined (on a pad whose player pressed Start for it), the AI coming
    /// in at the start.
    pub setup_status: [&'static str; 3],
    /// Under the seats: what `{k}` does to one.
    pub setup_seat_hint: &'static str,
    /// Under step 3's seats in pairs: `{k}` the partner key, `{p}` the seat
    /// under the cursor.
    pub setup_partner_hint: &'static str,
    /// The same in trios, two teammates each.
    pub setup_teammates_hint: &'static str,
    /// Why a pick leaves P2 where they are: `{p}` plays at the keyboard's
    /// second keys, which are wired to that seat.
    pub setup_pinned_keys: &'static str,
    /// The same for P2 set in Settings to controller `{n}`.
    pub setup_pinned_pad: &'static str,
    /// Why a pick leaves P2's seat where it is while P2 does not play it
    /// (the AI does, or nobody yet): `{p}`'s seat is kept for the
    /// keyboard's second keys.
    pub setup_kept_keys: &'static str,
    /// The same for a seat kept for controller `{n}` set in Settings.
    pub setup_kept_pad: &'static str,
    /// The open seats `{list}`, which the AI at `{l}` takes at the start.
    pub setup_open_line: &'static str,
    /// How a friend sits down, when a pad here could.
    pub setup_join_hint: &'static str,
    /// Step 3's button, with `{k}` the key that presses it in the hands
    /// last used ([`crate::app::match_setup::start_button_label`]).
    pub setup_start: &'static str,
    /// The keyboard's Enter, as this language's prompts name it: what a
    /// line with `{k}` in it says at a keyboard, where a pad's player is
    /// told the pad's button.
    pub key_enter: &'static str,
    pub mode_names: [&'static str; 3],
    pub tour_round: &'static str,
    pub tour_champion: &'static str,
    /// A decided series with no one ahead: rounds lost to dead heats, or
    /// a tally tied after the last round.
    pub tour_drawn: &'static str,
    pub tour_next: &'static str,
    /// The online host's Enter mid-series: its map page for the next round.
    pub tour_next_map: &'static str,
    // Versus sidebar event log
    pub log_raid: &'static str,
    /// A raid by a gull somebody's arrow steered: `{a}` steered it, `{p}`
    /// lost `{n}`.
    pub log_raid_by: &'static str,
    /// A gull steered into its own steerer's castle.
    pub log_raid_own: &'static str,
    pub log_golden: &'static str,
    pub log_lure: &'static str,
    pub log_tier: &'static str,
    pub log_gull: &'static str,
    pub log_surge: &'static str,
    pub log_golden_call: &'static str,
    /// A sparkling crab banked while the event wheel was locked. `{p}`.
    pub log_wheel_locked: &'static str,
    /// The feed's line for a spun event: `{p}` the seat whose sparkling
    /// crab spun it, `{e}` the event's name.
    pub log_event_by: &'static str,
    /// A molting crab banked while a lure was still running, so it
    /// started none. `{p}` banked it.
    pub log_lure_running: &'static str,
    /// A molting crab banked in the quiet spell after a lure, so it
    /// started none. `{p}` banked it.
    pub log_lure_resting: &'static str,
    /// A golden crab came out of its hole.
    pub log_golden_out: &'static str,
    pub map_names: [&'static str; 6],
    /// How a handmade beach reads on the map dial.
    pub map_custom: &'static str,
    /// The same, for one too big to fit an invitation: it plays at this
    /// table and cannot travel to another.
    pub map_custom_local: &'static str,
    /// Said beside the map dial when the player has beaches saved and none
    /// of them has a castle for every seat at this table.
    pub map_none_seats: &'static str,
    /// Said there instead when every saved beach skips a castle number
    /// (castles 0 and 2 with no 1): no table of any size can sit at one,
    /// so naming the table's size would point at the wrong fix.
    pub map_skips_castle: &'static str,
    pub gull_names: [&'static str; 3],
    pub round_names: [&'static str; 4],
    // Lobby
    pub lobby_hosting: &'static str,
    pub lobby_hosting_noport: &'static str,
    pub lobby_rivals_one: &'static str,
    pub lobby_rivals_many: &'static str,
    /// The host's status with only spectators aboard: Enter still starts,
    /// against the AI, which the line's suffix counts.
    pub lobby_rivals_none: &'static str,
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
    /// What the name box holds for a player who has never given one, so a
    /// pad, which cannot type, can still say who it is: "Crab {n}", `{n}`
    /// a two-digit number that tells two of them apart.
    pub lobby_suggest_name: &'static str,
    /// The same for a beach nobody has named yet, `{p}` its host.
    pub lobby_suggest_beach: &'static str,
    /// The lines a pad can say without a keyboard: North opens the chat
    /// line with the first, and each North after it moves to the next.
    pub quick_chat: [&'static str; 6],
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
    /// The same news for somebody who came to watch rather than to play.
    /// Its own line because "{p} joined" over a spectator reads as one
    /// more rival, and the table counts its rivals.
    pub lobby_watching_joined: &'static str,
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
    /// On the menu after a shipped build caught a panic mid-round and
    /// stopped the round rather than the game (`app::failsafe`).
    pub round_failed: &'static str,
    /// The host gave up on this seat: nothing it sent got through for
    /// `{s}` seconds, as long as the host waits before handing a seat to
    /// an AI. Read on the menu, like `online_host_gone`.
    pub online_dropped: &'static str,
    /// Waiting for a match that is already under way: next in line, or
    /// with `{n}` people ahead.
    pub lobby_queued_next: &'static str,
    pub lobby_queued_behind: &'static str,
    /// The same news for somebody who came to watch. No place in the
    /// queue, because a watcher is not in one: the crowd has room
    /// whoever is playing, so there is nobody ahead of them.
    pub lobby_queued_watching: &'static str,
    /// A would-be player in line while the host picks the next round's
    /// map: the round is dealt the table the map is picked for, so this
    /// peer watches it and plays the one after.
    pub lobby_queued_picking: &'static str,
    /// Every chair at that beach is taken, this round and the next.
    pub lobby_beach_full: &'static str,
    /// The host answered that its socket has no room for anyone more,
    /// watchers included (`PutOut::Full`).
    pub lobby_no_room: &'static str,
    /// A would-be player past the table's last chair, as the host said
    /// (`NetMsg::Roster::chair`): the launch seats it watching.
    pub lobby_no_chair: &'static str,
    /// The prompt under [`Self::lobby_no_chair`].
    pub lobby_no_chair_prompt: &'static str,
    pub lobby_join_list: &'static str,
    /// The same line when there is exactly one, which "1 beaches" is not.
    pub lobby_join_list_one: &'static str,
    pub lobby_broadcasting: &'static str,
    /// The host's prompt while picking partners: `{n}` the table's last seat.
    pub lobby_partner_prompt: &'static str,
    /// The heading of the lobby host's map page, over its cards: `{n}` the
    /// seats the table was frozen at when the host pressed start.
    pub host_map_title: &'static str,
    /// The line under a greyed card on the lobby host's map page: `{m}`
    /// the map, `{s}` the tables it seats, `{n}` this table.
    pub host_map_greyed: &'static str,
    /// The same for a beach too big for an invitation: `{m}` its name.
    pub host_map_too_big: &'static str,
    /// The terms card's map row in the lobby: the host picks the map once
    /// the table is frozen, when it starts the match.
    pub lobby_map_at_start: &'static str,
    /// A joiner's status line while the host is on the map page.
    pub lobby_host_picking: &'static str,
    /// The notice to the room when somebody at the table left while the
    /// host was picking the map, which calls the start off.
    pub lobby_pick_called_off: &'static str,
    /// The same when it was one of the bots that came straight to this
    /// beach that went: dropped from its door, or let go.
    pub lobby_pick_called_off_bot: &'static str,
    /// The host's prompt on the map page.
    pub lobby_pick_prompt: &'static str,
    /// The host's prompt on the map page between the rounds of a series.
    pub series_pick_prompt: &'static str,
    /// The same two prompts with the cursor on a card Enter does nothing
    /// on (greyed, or the one saying how to make a map): without Enter.
    pub lobby_pick_prompt_no_pick: &'static str,
    pub series_pick_prompt_no_pick: &'static str,
    /// The results card's status line on a joiner while the host picks the
    /// next round's map.
    pub host_picking_next: &'static str,
    /// The heading of the line-up shown to every peer before an online
    /// match's first round.
    pub lineup_title: &'static str,
    /// The line-up's line for a peer watching the match.
    pub lineup_watching: &'static str,
    pub lobby_aboard_prompt: &'static str,
    /// The same line for a spectator, who is not aboard: the status line
    /// above it already says "watching", and two lines disagreeing about
    /// which of those you are is worse than either.
    pub lobby_watching_prompt: &'static str,
    /// And for a peer in line for the next round, which has not started
    /// when the host launches: it started without them.
    ///
    /// It says where they are standing rather than what they will get: a
    /// chair is free next round only if one of the players does not come
    /// back, which nobody knows yet (see `PeerBook::seat_plan`).
    pub lobby_queued_prompt: &'static str,
    /// The same line for one of those who came to watch, who *can* be
    /// promised what comes next: the crowd always has room.
    pub lobby_queued_watch_prompt: &'static str,
    pub lobby_could_not_host: &'static str,
    pub lobby_could_not_join: &'static str,
    pub lobby_version_clash: &'static str,
    // Editor
    /// Fills `{walls}`, `{turn}` and `{check}` with the keys those do,
    /// which move off their letters when P1's keys take them
    /// (`editor::Command`). The settings' keys are on the settings card.
    pub ed_prompt: &'static str,
    /// The editor's brush palette, in [`Brush::ALL`] order. The first is
    /// bare sand, the brush that clears a tile, named for what it does.
    pub ed_brushes: [&'static str; 9],
    /// The editor's readout of what the cursor is standing on.
    pub ed_under: &'static str,
    /// What the loaded brush would make of it, when that is different.
    pub ed_next: &'static str,
    /// A tile with nothing on it, as the readout says it.
    pub ed_sand: &'static str,
    /// The crab kinds as the readout names them, in `CrabKind` order:
    /// common, juvenile, giant, molting, golden, sparkling.
    pub ed_crab_kinds: [&'static str; 6],
    /// A crab's big claw, left then right.
    pub ed_claws: [&'static str; 2],
    /// A crab in the readout: `{k}` its kind, `{c}` its claw and `{d}`
    /// an arrow the way it faces.
    pub ed_crab_desc: &'static str,
    /// The settings card's rows (`editor::card`).
    pub ed_set_size: &'static str,
    pub ed_set_seats: &'static str,
    /// The card's balance row on a beach (`editor::validate::balance`):
    /// its label, and its values: even, uneven, and cut off (a seat some
    /// hole's crabs cannot reach at all).
    pub ed_set_balance: &'static str,
    pub ed_balances: [&'static str; 3],
    pub ed_set_posts: &'static str,
    /// The settings card's row for who plays a stage: one player, or two
    /// with a hand of claw-keyed arrows each.
    pub ed_set_players: &'static str,
    pub ed_set_goal: &'static str,
    pub ed_set_time: &'static str,
    pub ed_set_gulls: &'static str,
    pub ed_set_holes: &'static str,
    pub ed_set_wrap: &'static str,
    /// The settings card's mirror row: the copies the bench paints on as
    /// well (`editor::Symmetry`), named in [`Tr::ed_mirrors`] in its order:
    /// off, left-right, top-bottom, both, the half turn.
    pub ed_set_mirror: &'static str,
    pub ed_mirrors: [&'static str; 5],
    /// A length of time, `{s}` seconds.
    pub ed_secs: &'static str,
    /// A rate: one every `{s}` seconds.
    pub ed_every: &'static str,
    /// The goals on the card's goal row; `{n}` the crabs to score.
    pub ed_goal_all: &'static str,
    pub ed_goal_survive: &'static str,
    pub ed_goal_golden: &'static str,
    pub ed_goal_bank: &'static str,
    /// The settings card's foot at a keyboard: `{next}` is the key that
    /// chooses the next row.
    pub ed_settings_keys: &'static str,
    /// A beach's playtest, under way. Fills `{place}` and `{remove}`, as
    /// `ed_prompt` fills `{walls}`: see `editor::spell_editor_keys`.
    pub ed_playtest_prompt: &'static str,
    /// A stage's playtest, setting up: its arrows, Enter to run it.
    pub ed_test_setup: &'static str,
    /// [`Tr::ed_test_setup`] on a co-op stage, with both players' keys
    /// as the stage's own line names them ([`Tr::prompt_setup_coop`]).
    pub ed_test_setup_coop: &'static str,
    /// [`Tr::ed_test_setup_coop`] with P1 on the mirrored placing keys
    /// ([`Tr::prompt_setup_coop_swapped`]).
    pub ed_test_setup_coop_swapped: &'static str,
    /// [`Tr::ed_test_setup_coop`] with any key rebound, where the stock
    /// names would be wrong ([`Tr::prompt_setup_custom`]).
    pub ed_test_setup_coop_custom: &'static str,
    /// A stage running or decided, or a beach's round over: R back to its
    /// start.
    pub ed_test_retry: &'static str,
    /// The arrow keys and Space, in the words the editor's two prompts
    /// name them by while they are the stock keys.
    pub ed_arrow_keys: &'static str,
    pub ed_space_key: &'static str,
    /// A check's way through: `{n}` arrows of the `{g}` granted, the
    /// fewest that win, at the tiles in `{placements}`.
    /// After a co-op stage's way through: neither player wins it alone.
    pub ed_needs_both: &'static str,
    /// After a co-op stage's way through: one player wins it alone with
    /// every arrow, so it does not need two.
    pub ed_one_alone: &'static str,
    pub ed_solvable: &'static str,
    pub ed_solvable_free: &'static str,
    pub ed_not_solvable: &'static str,
    /// Not winnable with the `{n}` arrows granted, and winnable with `{m}`:
    /// the fewest a second search past the grant found.
    pub ed_not_solvable_needs: &'static str,
    pub ed_solver_gave_up: &'static str,
    pub ed_validating: &'static str,
    pub ed_already_validating: &'static str,
    /// Said after F5 resizes the beach, with what is built kept centred.
    pub ed_resized: &'static str,
    /// The same, when what was built ran off a side that shrank: undo
    /// brings it back.
    pub ed_resized_cut: &'static str,
    /// A beach's castle, or a hole facing across, painted on the mirror's
    /// line, which is its own copy and so cannot be fair: refused.
    pub ed_mirror_line: &'static str,
    /// A beach's log, or a crab or gull facing across, on the mirror's
    /// line, which is its own copy and so cannot be fair: refused.
    pub ed_mirror_across: &'static str,
    /// A castle of the two seats both mirrors keep on the long edges,
    /// `{p}` and `{q}`, painted off the middle column: its copy would give
    /// the seat two castles. Refused.
    pub ed_mirror_seats: &'static str,
    /// Led before the resize line when the beach's middle moved half a
    /// tile and the mirror, which no longer lines up, was turned off.
    pub ed_mirror_moved: &'static str,
    /// Z and Shift+Z: an edit taken back and made again, or none there.
    pub ed_undone: &'static str,
    pub ed_redone: &'static str,
    pub ed_no_undo: &'static str,
    pub ed_no_redo: &'static str,
    /// What a freshly opened editor calls its level.
    pub ed_default_name: &'static str,
    pub ed_naming: &'static str,
    /// `{file}` is the file's name alone: the whole path ran off the
    /// header.
    pub ed_saved_to: &'static str,
    pub ed_save_failed: &'static str,
    /// Esc pressed once over an unsaved level: a second press leaves.
    pub ed_leave_confirm: &'static str,
    /// Enter pressed once on "My levels" over unsaved work: a second press
    /// opens the level chosen.
    pub ed_open_confirm: &'static str,
    /// "My levels" (F4): its heading, its two rows that are not levels, its
    /// keys, and what opening and deleting a level say.
    pub ed_levels_title: &'static str,
    pub ed_levels_new: &'static str,
    pub ed_levels_paste: &'static str,
    /// The starters "My levels" offers beside a blank level: a small
    /// stage, and a mirrored versus map for `{n}` players.
    pub ed_template_puzzle: &'static str,
    pub ed_template_beach: &'static str,
    pub ed_levels_keys: &'static str,
    /// The editor's card of every key and pad button (Shift+F1): its
    /// heading, the heads of its keyboard and pad columns, the headings of
    /// its building, playtest and stage list sections, the line for the
    /// brushes' letters, and the prompt while any card is up.
    pub ed_help_title: &'static str,
    pub ed_help_heads: [&'static str; 2],
    pub ed_help_sections: [&'static str; 3],
    pub ed_help_brushes: &'static str,
    pub ed_card_close: &'static str,
    /// The tips a first visit to the editor is shown once: `{help}` the
    /// key or button for the card of every key, `{mirror}` the card's
    /// mirror row as it names it.
    pub ed_tips_title: &'static str,
    pub ed_tips: [&'static str; 5],
    pub ed_opened: &'static str,
    pub ed_level_deleted: &'static str,
    /// The menu's card for a level code pasted with V: its keys, and what S
    /// says over a file holding a level of that name already.
    pub level_card_keys: &'static str,
    pub level_card_taken: &'static str,
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
    pub ed_fresh_sand: &'static str,
    pub ed_back: &'static str,
    pub ed_wrap_on: &'static str,
    /// Heading over the editor's puzzle/beach toggle.
    pub ed_kind_row: &'static str,
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
    /// A beach whose castles skip a player: `{n}` is the player, counted
    /// from 1, with no castle while a later one has. No table sits there.
    pub ed_arena_skips_seat: &'static str,
    pub ed_arena_no_crabs: &'static str,
    /// A beach's check when its seats walk unequal ways from the holes:
    /// `{p}` the nearest seat at `{n}` steps, `{q}` the furthest at `{m}`.
    pub ed_fair_uneven: &'static str,
    /// A beach's check when some hole's crabs cannot walk to seat `{p}`.
    pub ed_fair_cut_off: &'static str,
    /// Said on saving a stage with crabs and no castle, which the stage
    /// list shows and nobody can win.
    pub ed_puzzle_no_castle: &'static str,

    // --- bot seats (docs/bot-seats.md) ---
    /// The AI row's value for a seat a bot holds: a program somebody
    /// wrote, connected over the network (`docs/bot-seats.md`).
    pub match_bot: &'static str,
    /// The doorway card's heading on a couch: bots to fill this table.
    pub door_title_table: &'static str,
    /// The doorway card's heading when joining a beach as a bot.
    pub door_title_join: &'static str,
    /// Over the connection strings: what to do with them.
    pub door_start_with: &'static str,
    /// A chair whose bot has not registered yet.
    pub door_waiting: &'static str,
    /// A chair whose bot has: its name and build, and its round trip.
    pub door_connected: &'static str,
    /// A chair whose bot dropped and may still come back to it: its name.
    pub door_away: &'static str,
    /// The doorway card's keys, showing this machine's strings.
    pub door_prompt: &'static str,
    /// The doorway card's keys, showing the LAN's.
    pub door_prompt_lan: &'static str,
    /// A string went on the clipboard.
    pub door_copied: &'static str,
    /// The doorway could not open: no port to listen on.
    pub door_could_not_listen: &'static str,
    /// What a bot's seat is called: its name and its owner's, the robot
    /// beside it saying the rest. The game sets this, never the bot, and it
    /// travels in a name's twelve characters, so it is kept short.
    pub bot_owned: &'static str,
    /// A bot that gave no owner.
    pub bot_unowned: &'static str,
    /// A bot's connection went and the grace ran out: the AI took its
    /// chair.
    pub bot_stand_in: &'static str,
    /// And it came back.
    pub bot_back: &'static str,
    /// The host's dial for letting bots in, and its two positions.
    pub lobby_bots_dial: &'static str,
    pub lobby_bots_values: [&'static str; 2],
    /// Tried to join as a bot a beach that is not taking any.
    pub lobby_no_bots: &'static str,
    /// The host is choosing whom to ask to leave.
    pub lobby_kick_ask: &'static str,
    /// After the K prompt's names, when the peers off the table take more
    /// than one page of numbers: Tab turns to the next. `{p}` is the page
    /// shown, `{n}` how many there are.
    pub lobby_kick_more: &'static str,
    /// The feed, when somebody was asked to leave.
    pub lobby_kicked_feed: &'static str,
    /// The status line of the one who was.
    pub lobby_kicked_you: &'static str,
    /// A joiner's host went off the air while it waited in the lobby.
    pub lobby_host_closed: &'static str,
    /// The replay shelf: any replay file can be watched by dropping it.
    pub replay_drop_hint: &'static str,
    /// A dropped file that is not a replay this build plays.
    pub replay_drop_bad: &'static str,
    /// Why a recording made by a later build is refused
    /// ([`crate::sim::NEEDS_NEWER`]), in place of the sim's own
    /// English, inside `replay_drop_bad` or `code_round_bad`.
    pub replay_newer: &'static str,
    /// The first press of delete on the replay shelf: `{k}` is the key
    /// or pad button that answers, `{r}` the round as the shelf names it.
    pub replay_delete_confirm: &'static str,
    /// The round `{r}` is gone from the shelf.
    pub replay_deleted: &'static str,
    /// The round `{r}` would not delete; `{e}` is the system's reason,
    /// untranslated.
    pub replay_delete_failed: &'static str,
    /// F2 on a name whose file this bench did not write this session: the
    /// first press only says so, naming the level there as `{name}`, and a
    /// second replaces it.
    pub ed_save_taken: &'static str,
    /// Something failed in the editor and was caught: any playtest is
    /// stopped, and the level being built is kept.
    pub ed_failed: &'static str,
    /// The lobby could not listen for beacons on any of its ports; `{e}`
    /// is the system's reason, untranslated.
    pub lobby_discovery_unavailable: &'static str,
    /// What the controller prompts say: see [`PadTr`].
    pub pad: PadTr,
}
}

string_table! {
/// The words of the controller prompts (`gamepad::scheme`), shown in place
/// of the keyboard's whenever a pad was the last thing touched. The pad's
/// buttons go by the letters printed on them, the same in every language,
/// so these are the words after them, the lines in front of them, and the
/// few sentences that name a button inside them (`{b}`).
struct PadTr {
    /// The d-pad, and the right stick pushed to a side, as a prompt names
    /// them.
    pub dpad: &'static str,
    pub right_stick: &'static str,
    /// One control and what it does: `{b}` the button, `{a}` the words.
    pub bind: &'static str,
    /// In front of a box being typed into: how a Steam Deck brings up its
    /// on-screen keyboard, which types into the game's window.
    pub steam_keyboard: &'static str,
    // What each control does, as the prompt says it after the button.
    pub choose: &'static str,
    pub choose_adjust: &'static str,
    pub confirm: &'static str,
    /// On to match setup's next step.
    pub next: &'static str,
    pub play: &'static str,
    pub back: &'static str,
    pub menu: &'static str,
    pub save_back: &'static str,
    pub paste_round: &'static str,
    pub mute: &'static str,
    pub rebind: &'static str,
    pub defaults: &'static str,
    pub start_match: &'static str,
    pub join: &'static str,
    /// Step 3's partner pick, under a team mode.
    pub partners: &'static str,
    pub rename: &'static str,
    pub coop: &'static str,
    pub watch: &'static str,
    pub copy_code: &'static str,
    pub paste_code: &'static str,
    /// Deleting the round under the cursor on the replay shelf.
    pub delete: &'static str,
    pub scroll: &'static str,
    pub not_now: &'static str,
    pub go_on: &'static str,
    pub cancel: &'static str,
    /// On an open line of chat: the next of the quick phrases.
    pub next_phrase: &'static str,
    pub copy: &'static str,
    pub lan_open: &'static str,
    pub lan_close: &'static str,
    pub pick: &'static str,
    pub as_bot: &'static str,
    pub host: &'static str,
    pub by_address: &'static str,
    pub play_instead: &'static str,
    pub set_round: &'static str,
    pub talk: &'static str,
    pub ask_to_leave: &'static str,
    pub more: &'static str,
    pub never_mind: &'static str,
    pub moves: &'static str,
    pub place: &'static str,
    pub remove: &'static str,
    pub clear: &'static str,
    pub start: &'static str,
    pub run: &'static str,
    pub level: &'static str,
    pub hint: &'static str,
    pub reset: &'static str,
    pub pause: &'static str,
    pub next_level: &'static str,
    pub try_again: &'static str,
    /// Back to the stage list, from a puzzle's won or lost card.
    pub stages: &'static str,
    pub back_to_menu: &'static str,
    pub speed: &'static str,
    pub vote: &'static str,
    pub pick_winner: &'static str,
    pub close: &'static str,
    pub lobby: &'static str,
    pub next_round: &'static str,
    /// The online host's map page for the next round of a series.
    pub pick_map: &'static str,
    pub rematch: &'static str,
    pub rematch_series: &'static str,
    /// [`Self::rematch_series`] with the button pressed once already.
    pub rematch_series_confirm: &'static str,
    pub keep_replay: &'static str,
    // The editor's.
    pub paint: &'static str,
    pub brush: &'static str,
    pub walls: &'static str,
    /// The settings card's chosen row turned, a trigger each way.
    pub setting: &'static str,
    /// The creature or hole under the editor's cursor turned.
    pub turn: &'static str,
    /// The editor's Select layer: the right stick left and right.
    pub undo: &'static str,
    /// Opening the level chosen on "My levels", and the button that puts
    /// the list up.
    pub open: &'static str,
    pub my_levels: &'static str,
    /// A level of the player's own opened in the editor.
    pub edit: &'static str,
    pub level_card_taken: &'static str,
    pub check: &'static str,
    pub test: &'static str,
    /// Select, held for the editor's second row of commands.
    pub hold_more: &'static str,
    pub save: &'static str,
    pub name: &'static str,
    pub size: &'static str,
    pub kind: &'static str,
    /// The settings card's choice moved a row, a trigger each way.
    pub pick_setting: &'static str,
    /// The foot of the editor's settings card on a pad: `{b}` is the
    /// Select layer's triggers that choose a row.
    pub settings_card: &'static str,
    pub end_test: &'static str,
    /// The editor's card of every key and button, on Select+Start.
    pub help: &'static str,
    /// Leaving the game, from the main menu's quit card.
    pub quit: &'static str,
    // The lines in front of a face's controls, which the keyboard's
    // prompt says with its keys run through them.
    pub running: &'static str,
    pub won: &'static str,
    pub won_goal: &'static str,
    pub last_level: &'static str,
    pub lost: &'static str,
    pub lost_goal: &'static str,
    pub no_posts: &'static str,
    pub full: &'static str,
    pub none_yet: &'static str,
    pub open_games: &'static str,
    pub open_game: &'static str,
    pub watch_armed: &'static str,
    pub hosting: &'static str,
    pub joined: &'static str,
    pub watching: &'static str,
    pub queued: &'static str,
    pub queued_watch: &'static str,
    pub no_chair: &'static str,
    /// The host picking whom to ask to leave, `{p}` the one the pad's
    /// pick stands on.
    pub kick_who: &'static str,
    /// The winner can still be called, for `{n}` more seconds.
    pub pick_open: &'static str,
    // Sentences with a button in them, `{b}`.
    pub hint_offer: &'static str,
    pub hint_showing: &'static str,
    /// Under the settings card: the round's controls, and the menus'.
    pub help_round: &'static str,
    pub help_menus: &'static str,
    pub leave_confirm: &'static str,
    pub open_confirm: &'static str,
    pub save_taken: &'static str,
    pub reset_confirm: &'static str,
    /// The settings card's two doors, `{b}` the button that opens them.
    pub val_open: &'static str,
    pub val_reset: &'static str,
    pub replay_speed: &'static str,
}
}

#[cfg(test)]
impl TableField for PadTr {
    fn push(&self, field: &str, out: &mut Vec<(String, &'static str)>) {
        for (name, string) in self.strings() {
            out.push((format!("{field}.{name}"), string));
        }
    }
}

/// The word joiner: holds the characters either side of it on one line,
/// and is drawn as nothing at all (neither face has a glyph for it, and
/// the shaper hides it as the default-ignorable character it is). The
/// prompt sets one after a `/` or `-` the line must not break at
/// (`hud::banded`), and the width guards count it as nothing.
pub const WORD_JOINER: char = '\u{2060}';

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
    use crate::app::cycle::{Cycle, Turn};

    /// Every parameterized template must carry the same `{markers}` in
    /// every language: a translation that drops one silently loses data
    /// (a lure banner without its {p}, a score line without its {b}).
    ///
    /// Both lists come from declarations rather than by hand (the fields
    /// from the struct, the languages from [`ALL_LANGS`]), so this keeps
    /// covering the whole table as both grow.
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

    /// The co-op lines call the two seats what the rest of the language
    /// calls them: French said P1 and P2 beside a scoreboard of J1 and J2.
    #[test]
    fn the_pair_is_named_by_each_languages_own_labels() {
        for lang in ALL_LANGS {
            let tr = lang.tr();
            let (one, two) = (crate::app::seat_label(tr, 0), crate::app::seat_label(tr, 1));
            for line in [
                tr.prompt_setup_coop,
                tr.prompt_setup_coop_swapped,
                tr.ed_test_setup_coop_swapped,
            ] {
                let coop = crate::app::name_the_pair(tr, line);
                assert!(
                    coop.contains(&one) && coop.contains(&two),
                    "{lang:?}: {coop}"
                );
            }
            let open = crate::app::name_the_pair(tr, tr.setup_by_open_keys);
            assert!(open.contains(&two), "{lang:?}: {open}");
        }
    }

    /// The modes are called what every game calls them, and one mode has
    /// one name in a language: a trophy still saying "tide pools" or
    /// "duel" names a mode the player cannot find on the menu (a duel is
    /// two players; versus takes six). These are the old names each
    /// language used, matched without case. The tide pool on the board
    /// is terrain and keeps its name, so only words that were ever a
    /// mode's are listed.
    #[test]
    fn no_line_uses_a_retired_mode_name() {
        let retired: [(Lang, &[&str]); 8] = [
            (Lang::En, &["turf war", "tide pool stage", "driftwood"]),
            (Lang::Fr, &["duel", "toutes les mares"]),
            (Lang::De, &["duell", "kampf", "jeden tümpel"]),
            (Lang::Es, &["duelo", "charcas de marea"]),
            (Lang::It, &["in sfida"]),
            (Lang::Nl, &["duel", "strijd"]),
            (Lang::Ru, &["дуэл"]),
            (Lang::Ja, &[]),
        ];
        for (lang, words) in retired {
            for (field, line) in lang.tr().strings() {
                let line = line.to_lowercase();
                for word in words {
                    assert!(
                        !line.contains(word),
                        "{lang:?} {field} still says {word:?}: {line}"
                    );
                }
            }
        }
    }

    /// Every Japanese character the game can put on screen has to be in
    /// the subset that ships, or that word draws as a row of nothing.
    /// Reword a Japanese string and this is the test that says to run
    /// `tools/gen_jp_font.py` again.
    ///
    /// That face is a subset, a whole CJK font being twenty megabytes for
    /// the few hundred characters this game says, and a character left out
    /// draws as nothing with no warning. So this reads the `cmap` of the
    /// very bytes that ship.
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
        let column = Lang::Ja.index();
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

    /// The display face draws the header, clocks, scores, banners and
    /// headings, any of which may be any string in any table. Nunito is a
    /// whole font rather than a subset, but a character it lacks would draw
    /// as nothing all the same, so every character every language says is
    /// held to it, bar the Japanese that falls back to the subset.
    #[test]
    fn the_display_font_carries_every_character_but_japanese() {
        let display = metrics::display_covers();
        let kanji = metrics::characters_in_font(include_bytes!(
            "../../../assets/fonts/NotoSansMonoCJKjp-Subset.otf"
        ));
        for lang in ALL_LANGS {
            let column = lang.index();
            let mut said: String = lang.tr().strings().iter().map(|(_, line)| *line).collect();
            for row in &LEVEL_NAMES {
                said.push_str(row[column]);
            }
            for row in &LEVEL_HINTS {
                said.push_str(row[1 + column]);
            }
            said.push_str(lang.native_name());
            for ch in said.chars().filter(|ch| !ch.is_control()) {
                assert!(
                    display.contains(&ch) || kanji.contains(&ch),
                    "{lang:?} says {ch:?}, which neither the display face nor the subset draws"
                );
            }
        }
    }

    /// Two lines name their key as the first thing in every language,
    /// "F6" over the editor's kind toggle and "Tab" on a seat with no name,
    /// and a pad's player is shown the pad's button in its place. A
    /// translation that spelled the key otherwise would keep the key's name
    /// on a pad's screen.
    #[test]
    fn the_keys_a_pad_respells_are_spelled_alike_everywhere() {
        for lang in ALL_LANGS {
            let tr = lang.tr();
            assert!(tr.ed_kind_row.starts_with("F6"), "{lang:?}");
            assert!(tr.match_name_empty.contains("Tab"), "{lang:?}");
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
            lang = lang.cycled(Turn::Right);
        }
        assert_eq!(lang, Lang::En);
        assert_eq!(Lang::En.cycled(Turn::Left), *ALL_LANGS.last().unwrap());
    }

    #[test]
    fn builtin_level_names_translate_and_custom_pass_through() {
        assert_eq!(Lang::Fr.level_name("Gull Gauntlet"), "Haie de mouettes");
        assert_eq!(Lang::De.level_name("Gull Gauntlet"), "Möwenspalier");
        assert_eq!(Lang::Es.level_name("Gull Gauntlet"), "Pasillo de gaviotas");
        assert_eq!(
            Lang::It.level_name("Gull Gauntlet"),
            "Corridoio di gabbiani"
        );
        assert_eq!(Lang::Nl.level_name("Gull Gauntlet"), "Meeuwenhaag");
        assert_eq!(Lang::Ru.level_name("Gull Gauntlet"), "Сквозь чаек");
        assert_eq!(Lang::Ja.level_name("Gull Gauntlet"), "カモメの関門");
        assert_eq!(Lang::En.level_name("Gull Gauntlet"), "Gull Gauntlet");
        assert_eq!(Lang::Fr.level_name("My Custom Beach"), "My Custom Beach");
    }

    /// The shipped stages, Tide Pool and co-op alike: the levels that
    /// take their names from [`LEVEL_NAMES`].
    fn shipped_levels() -> Vec<crate::sim::Level> {
        let mut levels = crate::sim::campaign_levels();
        levels.extend(crate::sim::coop_levels());
        levels
    }

    /// Every shipped stage, Tide Pool and co-op, has a row of names: a
    /// stage without one would fall back to English in every language
    /// and nothing else would notice.
    #[test]
    fn every_builtin_level_is_covered() {
        for level in &shipped_levels() {
            assert!(
                LEVEL_NAMES.iter().any(|row| row[0] == level.name),
                "missing translation row for level {:?}",
                level.name
            );
        }
    }

    /// No two shipped stages share a name in any language: two stages a
    /// player can pick on one screen must not read the same. Progress is
    /// filed by the English name, which this holds unique too.
    #[test]
    fn shipped_level_names_are_unique_in_every_language() {
        let levels = shipped_levels();
        for lang in ALL_LANGS {
            let mut seen = std::collections::HashMap::new();
            for level in &levels {
                let shown = lang.level_name(&level.name);
                if let Some(other) = seen.insert(shown, level.name.as_str()) {
                    panic!(
                        "{lang:?}: {:?} and {other:?} are both called {shown:?}",
                        level.name
                    );
                }
            }
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
                    !row[lang.index()].trim().is_empty(),
                    "{:?} has no {lang:?} name",
                    row[0]
                );
            }
            for row in &LEVEL_HINTS {
                assert!(
                    !row[1 + lang.index()].trim().is_empty(),
                    "{:?} has no {lang:?} hint",
                    row[0]
                );
            }
        }
    }
}
