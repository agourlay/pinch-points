//! The settings the whole game reads (spec §8.4): keyboard preset and
//! bindings, cursor repeat tuning, audio, accessibility, and the
//! single-player speed assist. Persisted as a plain `settings.txt` under
//! the XDG config directory (see [`crate::app::paths::config_dir`]),
//! leniently parsed so an old or hand-edited file never breaks startup.
//!
//! The screen that edits them is [`screen`]; the systems that push a
//! changed setting out into Bevy (UI scale, sink volume, tick rate) stay
//! here, beside the values they read.

pub mod screen;

use crate::app::Screen;
use crate::app::audio::Music;
use crate::app::binds::{BOUND_SEATS, SeatBinds, default_binds};
use crate::app::i18n::Lang;
use crate::app::palette;
use crate::app::teams::TeamMode;
use crate::sim::MAX_PLAYERS;
use bevy::audio::Volume;
use bevy::prelude::*;

/// The settings file, under the XDG config directory.
fn settings_path() -> std::path::PathBuf {
    crate::app::paths::config_dir().join("settings.txt")
}

/// How many finished rounds the shelf keeps, and the range the dial turns
/// through. Zero is not offered: a library that keeps nothing is the menu's
/// Replay entry quietly doing nothing, which reads as a fault rather than
/// as a setting.
pub const REPLAY_CAP_MIN: u8 = 5;
pub const REPLAY_CAP_MAX: u8 = 99;
pub const REPLAY_CAP_DEFAULT: u8 = 20;

/// UI scale bounds, percent: small enough to fit the chrome on a short
/// laptop screen, large enough to read across a room.
/// Where the gamepad deadzone dial stops, percent of travel.
pub const DEADZONE_RANGE: std::ops::RangeInclusive<u8> = 20..=80;
/// Where the volume dials stop.
pub const VOLUME_RANGE: std::ops::RangeInclusive<u8> = 0..=100;
/// Hold-to-repeat bounds, seconds: the card steps inside these and the
/// parser clamps a saved value into them, so the two cannot disagree.
pub const REPEAT_DELAY_RANGE: std::ops::RangeInclusive<f32> = 0.1..=0.5;
pub const REPEAT_INTERVAL_RANGE: std::ops::RangeInclusive<f32> = 0.03..=0.2;
pub const UI_SCALE_MIN: u8 = 80;
pub const UI_SCALE_MAX: u8 = 150;

/// What drives one of the two keyboard seats. The first two seats are the
/// ones a couch pair sits in, and the pad order they inherit is not
/// obvious: with two pads and no ceremony, pad one drives P2 and pad two
/// drives P1, because pads fill the table from the top down. This is how
/// you say otherwise.
///
/// The keyboard stays live whatever this says, apart from [`Self::Keys`],
/// which is the one that turns pads off. Picking a controller says *which*
/// controller is yours, not that the keyboard has stopped working: a pad
/// that goes flat mid-round should not strand the player holding it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SeatInput {
    /// Pads fill the seats from the highest down, as they always have.
    #[default]
    Auto,
    /// Keyboard only: no pad drives this seat.
    Keys,
    /// The nth controller, counting in the order they were claimed.
    Pad(u8),
}

/// How many controllers a seat may name. Four is what the pad ceremony can
/// seat, so it is what the dial offers.
pub const NAMED_PADS: u8 = 4;

/// Where player 1's four commit keys live: the stock arrows, or the §8.2
/// one-hand preset on IJKL. A dial, like the seat inputs beside it, and a
/// token in settings.txt that an unknown word falls back from rather than
/// silently reading as "arrows".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CommitScheme {
    #[default]
    Arrows,
    Ijkl,
}

impl CommitScheme {
    pub fn key(self) -> &'static str {
        match self {
            CommitScheme::Arrows => "arrows",
            CommitScheme::Ijkl => "ijkl",
        }
    }

    pub fn from_key(key: &str) -> CommitScheme {
        use crate::app::cycle::Cycle;
        Self::VARIANTS
            .iter()
            .copied()
            .find(|scheme| scheme.key() == key)
            .unwrap_or_default()
    }
}

impl crate::app::cycle::Cycle for CommitScheme {
    const VARIANTS: &'static [CommitScheme] = &[CommitScheme::Arrows, CommitScheme::Ijkl];
}

impl crate::app::cycle::Cycle for SeatInput {
    const VARIANTS: &'static [SeatInput] = &[
        SeatInput::Auto,
        SeatInput::Keys,
        SeatInput::Pad(0),
        SeatInput::Pad(1),
        SeatInput::Pad(2),
        SeatInput::Pad(3),
    ];
}

impl SeatInput {
    /// The settings-file spelling, and its inverse.
    fn name(self) -> String {
        match self {
            SeatInput::Auto => "auto".to_string(),
            SeatInput::Keys => "keys".to_string(),
            SeatInput::Pad(n) => format!("pad{}", n + 1),
        }
    }

    fn from_name(text: &str) -> SeatInput {
        match text.trim() {
            "keys" => SeatInput::Keys,
            other => other
                .strip_prefix("pad")
                .and_then(|n| n.parse::<u8>().ok())
                .filter(|&n| (1..=NAMED_PADS).contains(&n))
                .map_or(SeatInput::Auto, |n| SeatInput::Pad(n - 1)),
        }
    }
}

#[derive(Resource, Clone, PartialEq, Debug)]
pub struct GameSettings {
    /// §8.2: which keys player 1 commits a signpost on. The one-hand
    /// preset puts them on IJKL (solo modes only: IJKL moves player 2 in
    /// shared-keyboard versus).
    pub commit: CommitScheme,
    /// What drives P1 and P2. Seats past the second are always `Auto`:
    /// they have no keyboard of their own to choose between.
    pub seat_input: [SeatInput; crate::app::binds::BOUND_SEATS],
    /// Cursor hold-to-repeat: initial delay and repeat interval, seconds.
    pub repeat_delay: f32,
    pub repeat_interval: f32,
    /// Whether the background music plays at all. Separate from the
    /// volume so switching it off and back on returns the player to the
    /// level they had set, rather than to whatever 0% was hiding.
    pub music_on: bool,
    /// Background music volume, 0–100.
    pub music_volume: u8,
    /// Whether sound effects play at all; the companion to `music_on`.
    pub sfx_on: bool,
    /// Sound-effect volume, 0–100 (0 is the old "off").
    pub sfx_volume: u8,
    /// Puzzle-mode simulation speed percent (100/75/50), the §8.4
    /// single-player speed reduction. Never applies to versus or online.
    pub puzzle_speed: u8,
    /// How a versus round is scored: free-for-all, pairs, or trios.
    /// Presentation and win condition only; the sim is unchanged, so it is
    /// lockstep-safe.
    pub team_mode: TeamMode,
    /// UI language; every player-facing string localizes through this.
    pub language: Lang,
    /// Controller rumble on raids, surges, and round end.
    ///
    /// The two board-wide ones buzz every pad in the room, which is right
    /// for them: the surge and the tide coming in happen to the whole
    /// beach (`audio::play_events`). A raid does not - it happens to one
    /// seat - so that one is aimed, and reaches only the hands of the
    /// player whose castle it was (`gamepad::rumble_on_raid`).
    pub rumble: bool,
    /// Left-stick deadzone percent (20-80) before it counts as a direction.
    pub pad_deadzone: u8,
    /// Use the colour-vision-safe player palette (no red/green pair).
    pub colorblind: bool,
    /// UI scale percent (80-150): every HUD, panel, and menu element.
    pub ui_scale: u8,
    /// Drop the decorative motion: particle bursts, confetti, footprints,
    /// raid flashes, bouncing numbers, the blinking clock.
    pub reduced_motion: bool,
    /// Per-seat keyboard bindings (spec §8.4 full remapping).
    pub binds: [SeatBinds; BOUND_SEATS],
    /// What each seat is called. Empty means "no name given", and the seat
    /// falls back to the localized P1/J1/S1 label, so a fresh install and
    /// a player who never opens the naming rows behave as they always did.
    pub names: [String; MAX_PLAYERS],
    /// How many finished rounds the shelf keeps before the oldest fall off.
    ///
    /// A round is filed after every match, and nothing ever removed one, so
    /// the library grew for as long as the game was played. A cap is the
    /// only thing that makes "keep every round" a promise the disk can go
    /// on keeping.
    pub replay_cap: u8,
    /// The last beach dialled by hand, `ip:port`, empty until one is.
    ///
    /// Kept because the machine that had to be dialled once will have to be
    /// dialled again (a LAN with broadcast turned off does not turn it back
    /// on between rounds) and re-typing an address is a poor way to spend
    /// the evening. Only ever written from an address that parsed.
    pub last_beach: String,
    /// Ask GitHub on start-up whether a newer release is out, and say so
    /// on a page of its own (see [`crate::app::update`]). The one thing
    /// the game says to the wider internet, so it is the player's to
    /// switch off.
    pub check_updates: bool,
    /// The keyboard the player says they have, or `None` for the game
    /// working it out (which is right almost always - see
    /// [`crate::app::keycaps::KeyCaps::force`] for the "almost").
    ///
    /// A preference, unlike the learned caps table, which is not one -
    /// nobody sets it - and so lives as its own resource,
    /// [`crate::app::keycaps::KeyCaps`]. The two still share the file:
    /// the save path takes both and writes the `keycaps:` line beside
    /// this one.
    pub keyboard: Option<crate::app::keycaps::Layout>,
}

/// Longest seat name we keep. The score chips and the event feed are laid
/// out in a fixed-width sidebar; a twelve-character name is the most that
/// fits beside a four-digit score.
pub const NAME_MAX: usize = 12;

impl GameSettings {
    /// The active string table.
    pub fn tr(&self) -> &'static crate::app::i18n::Tr {
        self.language.tr()
    }

    /// Set the keyboard the game reads the caps off, or `None` to have
    /// it work that out again. The one path that changes it, and it asks
    /// for the caps table outright - a separate resource now - so the
    /// table can never disagree with the row on the card.
    pub fn set_keyboard(
        &mut self,
        keyboard: Option<crate::app::keycaps::Layout>,
        caps: &mut crate::app::keycaps::KeyCaps,
    ) {
        self.keyboard = keyboard;
        caps.force(keyboard);
    }

    /// Set the interface language, and with it the keyboard the game
    /// presumes: a player reading in French is typing on AZERTY until a
    /// press says otherwise (see [`crate::app::keycaps::Layout::of`]).
    /// Every path that changes the language comes through here, and has
    /// to bring the caps table with it, so the presumption can never be
    /// left behind by the words on screen.
    pub fn set_language(
        &mut self,
        language: crate::app::i18n::Lang,
        caps: &mut crate::app::keycaps::KeyCaps,
    ) {
        self.language = language;
        caps.presume(crate::app::keycaps::Layout::of(language));
    }

    /// Whether the player has moved any key off its factory binding. The
    /// on-screen key legends are written for the stock layout, so they step
    /// aside when this is true rather than teach the wrong keys.
    pub fn custom_binds(&self) -> bool {
        self.binds != default_binds()
    }

    /// Whether the stock key legends ("WASD move, arrows place") describe
    /// what the keys do right now: they do not once a key is rebound, and
    /// not with the one-hand preset either, which puts placement on IJKL
    /// whatever the bindings say. Anything that teaches keys asks this
    /// rather than [`Self::custom_binds`] alone.
    pub fn stock_legend(&self) -> bool {
        !self.custom_binds() && self.commit == CommitScheme::Arrows
    }

    /// Sound-effect gain, 0.0–1.0, with the on/off switch folded in. Zero
    /// means silence, so callers test the gain rather than asking about
    /// the switch and the slider separately - and a one-shot at zero gain
    /// is never spawned at all.
    pub fn sfx_gain(&self) -> f32 {
        if self.sfx_on {
            f32::from(self.sfx_volume) / 100.0
        } else {
            0.0
        }
    }

    /// Music gain, 0.0–1.0: the sink volume, wherever it is set. Switched
    /// off reads as zero, the same as the slider at the bottom.
    pub fn music_gain(&self) -> f32 {
        if self.music_on {
            f32::from(self.music_volume) / 100.0
        } else {
            0.0
        }
    }

    /// UI scale as the ratio Bevy wants (1.0 is 100%).
    pub fn ui_ratio(&self) -> f32 {
        f32::from(self.ui_scale) / 100.0
    }

    /// Left-stick deadzone as the 0.0–1.0 magnitude it is compared against.
    pub fn deadzone(&self) -> f32 {
        f32::from(self.pad_deadzone) / 100.0
    }

    /// The couch-table name for a seat. Round displays go through
    /// [`crate::app::SeatNames`] instead, which knows when an online table
    /// overrides these; this stays as the reference for the typing tests.
    #[cfg(test)]
    pub fn seat_name(&self, seat: u8) -> String {
        match self.names.get(usize::from(seat)) {
            Some(name) if !name.is_empty() => name.clone(),
            _ => crate::app::seat_label(self.tr(), seat),
        }
    }

    /// Type one character into a seat's name, dropping anything that would
    /// make a mess of the save file or the sidebar.
    pub fn push_name_char(&mut self, seat: u8, ch: char) {
        let Some(name) = self.names.get_mut(usize::from(seat)) else {
            return;
        };
        // `|` separates names in the save file and `:` its keys; control
        // characters would corrupt the line outright.
        let ok = !ch.is_control() && ch != '|' && ch != ':' && (ch != ' ' || !name.is_empty());
        if ok && name.chars().count() < NAME_MAX {
            name.push(ch);
        }
    }

    /// Rub out the last character of a seat's name.
    pub fn pop_name_char(&mut self, seat: u8) {
        if let Some(name) = self.names.get_mut(usize::from(seat)) {
            name.pop();
        }
    }

    /// Tidy a seat's name once the player is done typing: a name of nothing
    /// but spaces is no name at all.
    pub fn tidy_name(&mut self, seat: u8) {
        if let Some(name) = self.names.get_mut(usize::from(seat)) {
            *name = name.trim().to_string();
        }
    }
}

impl Default for GameSettings {
    fn default() -> Self {
        GameSettings {
            commit: CommitScheme::Arrows,
            seat_input: [SeatInput::Auto; crate::app::binds::BOUND_SEATS],
            repeat_delay: 0.28,
            repeat_interval: 0.09,
            music_on: true,
            music_volume: 45,
            sfx_on: true,
            sfx_volume: 80,
            puzzle_speed: 100,
            team_mode: TeamMode::default(),
            language: Lang::default(),
            rumble: true,
            pad_deadzone: 50,
            colorblind: false,
            ui_scale: 100,
            reduced_motion: false,
            binds: default_binds(),
            names: std::array::from_fn(|_| String::new()),
            replay_cap: REPLAY_CAP_DEFAULT,
            last_beach: String::new(),
            check_updates: true,
            keyboard: None,
        }
    }
}

impl GameSettings {
    /// The caps come in as a parameter because they are a resource of
    /// their own, not a field: forgetting them here would leave `caps`
    /// unused, which the warning pass refuses, the same way the
    /// destructure below refuses a field left unwritten.
    pub fn to_text(&self, caps: &crate::app::keycaps::KeyCaps) -> String {
        // Destructured with no rest pattern: a new setting refuses to build
        // here until it is written out. The lenient `parse` below would
        // never notice one going unsaved.
        let Self {
            commit,
            seat_input,
            repeat_delay,
            repeat_interval,
            music_on,
            music_volume,
            sfx_on,
            sfx_volume,
            puzzle_speed,
            team_mode,
            language,
            rumble,
            pad_deadzone,
            colorblind,
            ui_scale,
            reduced_motion,
            binds,
            names,
            replay_cap,
            last_beach,
            check_updates,
            keyboard,
        } = self;
        format!(
            "commit_scheme: {}\nrepeat_delay: {:.2}\nrepeat_interval: {:.2}\n\
             music_on: {}\nmusic: {}\nsfx_on: {}\nsfx: {}\n\
             puzzle_speed: {}\nteams: {}\nlanguage: {}\n\
             rumble: {}\npad_deadzone: {}\npalette: {}\nui_scale: {}\n\
             reduced_motion: {}\nnames: {}\nreplay_cap: {}\nbeach: {}\n\
             updates: {}\nkeycaps: {}\nkeyboard: {}\n\
             p1_input: {}\np2_input: {}\n{}",
            commit.key(),
            repeat_delay,
            repeat_interval,
            if *music_on { "on" } else { "off" },
            music_volume,
            if *sfx_on { "on" } else { "off" },
            sfx_volume,
            puzzle_speed,
            team_mode.key(),
            language.key(),
            if *rumble { "on" } else { "off" },
            pad_deadzone,
            if *colorblind { "colorblind" } else { "classic" },
            ui_scale,
            if *reduced_motion { "on" } else { "off" },
            names.join("|"),
            replay_cap,
            last_beach,
            if *check_updates { "on" } else { "off" },
            caps.to_text(),
            keyboard.map_or("auto", crate::app::keycaps::Layout::key),
            seat_input[0].name(),
            seat_input[1].name(),
            crate::app::binds::to_text(binds),
        )
    }

    /// Lenient parse: unknown keys and bad values fall back to defaults, so
    /// an old or hand-edited file never breaks startup. Both halves of the
    /// file come back together: the preferences, and the learned caps
    /// table that rides in it as the `keycaps:` line.
    pub fn parse(text: &str) -> (GameSettings, crate::app::keycaps::KeyCaps) {
        let mut settings = GameSettings::default();
        let mut caps = crate::app::keycaps::KeyCaps::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "commit_scheme" => settings.commit = CommitScheme::from_key(value),
                "p1_input" => settings.seat_input[0] = SeatInput::from_name(value),
                "p2_input" => settings.seat_input[1] = SeatInput::from_name(value),
                "repeat_delay" => {
                    if let Ok(v) = value.parse() {
                        settings.repeat_delay =
                            f32::clamp(v, *REPEAT_DELAY_RANGE.start(), *REPEAT_DELAY_RANGE.end());
                    }
                }
                "repeat_interval" => {
                    if let Ok(v) = value.parse() {
                        settings.repeat_interval = f32::clamp(
                            v,
                            *REPEAT_INTERVAL_RANGE.start(),
                            *REPEAT_INTERVAL_RANGE.end(),
                        );
                    }
                }
                "music_on" => settings.music_on = value != "off",
                "sfx_on" => settings.sfx_on = value != "off",
                "music" => {
                    if let Ok(v) = value.parse::<u8>() {
                        settings.music_volume = v.min(*VOLUME_RANGE.end());
                    }
                }
                // Was an on/off toggle before it became a slider; the old
                // words still parse so an existing settings.txt keeps working.
                "sfx" => {
                    settings.sfx_volume = match value {
                        "off" => 0,
                        "on" => 100,
                        _ => value
                            .parse::<u8>()
                            .map_or(settings.sfx_volume, |v| v.min(*VOLUME_RANGE.end())),
                    };
                }
                "replay_cap" => {
                    if let Ok(v) = value.parse::<u8>() {
                        settings.replay_cap = v.clamp(REPLAY_CAP_MIN, REPLAY_CAP_MAX);
                    }
                }
                "teams" => settings.team_mode = TeamMode::from_key(value),
                "language" => settings.language = Lang::from_key(value),
                "rumble" => settings.rumble = value != "off",
                "pad_deadzone" => {
                    if let Ok(v) = value.parse::<u8>() {
                        settings.pad_deadzone =
                            v.clamp(*DEADZONE_RANGE.start(), *DEADZONE_RANGE.end());
                    }
                }
                "palette" => settings.colorblind = value == "colorblind",
                "ui_scale" => {
                    if let Ok(v) = value.parse::<u8>() {
                        settings.ui_scale = v.clamp(UI_SCALE_MIN, UI_SCALE_MAX);
                    }
                }
                "reduced_motion" => settings.reduced_motion = value == "on",
                // `a|b|c|d`, so a name may hold spaces. Missing trailing
                // fields simply stay unnamed.
                "names" => {
                    for (seat, name) in value.split('|').take(MAX_PLAYERS).enumerate() {
                        let name = name.trim();
                        settings.names[seat] = String::new();
                        for ch in name.chars() {
                            settings.push_name_char(seat as u8, ch);
                        }
                    }
                }
                // Only kept if it still parses as an address: the file is
                // hand-editable, and a line of nonsense here would come
                // back as a pre-filled answer the player has to rub out.
                "beach" => {
                    if value.parse::<std::net::SocketAddr>().is_ok() {
                        settings.last_beach = value.to_string();
                    }
                }
                "updates" => settings.check_updates = value != "off",
                "keycaps" => caps = crate::app::keycaps::KeyCaps::parse(value),
                "keyboard" => settings.keyboard = crate::app::keycaps::Layout::from_key(value),
                "keys_p1" | "keys_p2" => {
                    let seat = usize::from(key.trim() == "keys_p2");
                    if let Some(seat_binds) = crate::app::binds::parse_seat(value) {
                        settings.binds[seat] = seat_binds;
                    }
                }
                "puzzle_speed" => {
                    if let Ok(v) = value.parse::<u8>() {
                        settings.puzzle_speed = match v {
                            0..=62 => 50,
                            63..=87 => 75,
                            88.. => 100,
                        };
                    }
                }
                _ => {}
            }
        }
        // The two keyboard seats share one keyboard: a file that binds the
        // same key twice across seats would leave a seat shadowed, so the
        // whole table goes back to stock rather than half-work.
        if !crate::app::binds::all_distinct(&settings.binds) {
            settings.binds = default_binds();
        }
        // After the loop, not in the arms that read them: the keycaps
        // line may be read after either, and the presumption is only
        // taken where the learned caps leave room for it.
        let (language, keyboard) = (settings.language, settings.keyboard);
        settings.set_language(language, &mut caps);
        settings.set_keyboard(keyboard, &mut caps);
        (settings, caps)
    }

    /// The saved settings and caps, or `None` when there is no file to read.
    ///
    /// The distinction is the whole of the first-run language picker:
    /// defaults and a saved English are the same settings, and only one of
    /// them means nobody has been asked yet. One read rather than a
    /// [`Path::exists`] beside a load, which could disagree with it.
    ///
    /// [`Path::exists`]: std::path::Path::exists
    pub fn load_saved() -> Option<(GameSettings, crate::app::keycaps::KeyCaps)> {
        std::fs::read_to_string(settings_path())
            .ok()
            .map(|text| GameSettings::parse(&text))
    }

    pub fn load() -> (GameSettings, crate::app::keycaps::KeyCaps) {
        GameSettings::load_saved().unwrap_or_default()
    }

    /// Write the file, caps and all: one file on disk, so one write, and
    /// a caller without the caps table has no way to call it.
    pub fn save(&self, caps: &crate::app::keycaps::KeyCaps) {
        let _ = crate::app::paths::write_atomic(&settings_path(), self.to_text(caps));
    }
}

/// The window the interface was laid out for. Every card, gutter and
/// offset on every screen is a number of these pixels, so a window smaller
/// than this shrinks the whole interface to fit rather than letting the
/// cards run off the edges.
pub const DESIGN_W: f32 = 1280.0;
pub const DESIGN_H: f32 = 720.0;

/// How much the interface has to shrink to fit `window`. Never above 1: a
/// larger window is given more room rather than a bigger interface, which
/// is also what the board does, and the UI scale setting is there for
/// anyone who wants it bigger anyway.
pub fn fit_ratio(window: &Window) -> f32 {
    (window.width() / DESIGN_W)
        .min(window.height() / DESIGN_H)
        .clamp(0.1, 1.0)
}

/// Push the accessibility choices that live outside this resource: the
/// player palette (read from a static by every renderer) and Bevy's global
/// UI scale. The palette follows the settings; the scale also follows the
/// window, so it is recomputed every frame and written only when it moves.
pub fn apply_accessibility(
    settings: Res<GameSettings>,
    windows: Query<&Window>,
    mut ui_scale: ResMut<UiScale>,
) {
    if settings.is_changed() {
        palette::set_colorblind(settings.colorblind);
    }
    let fit = windows.iter().next().map_or(1.0, fit_ratio);
    let scale = settings.ui_ratio() * fit;
    if (ui_scale.0 - scale).abs() > f32::EPSILON {
        ui_scale.0 = scale;
    }
}

/// Push the music volume to the sink whenever settings change.
pub fn apply_music_volume(
    settings: Res<GameSettings>,
    mut sinks: Query<&mut AudioSink, With<Music>>,
) {
    if !settings.is_changed() {
        return;
    }
    for mut sink in &mut sinks {
        sink.set_volume(Volume::Linear(settings.music_gain()));
    }
}

/// The §8.4 speed assist: puzzles tick slower when asked; every other mode
/// (and online determinism) stays at the canonical 30 Hz.
pub fn apply_sim_speed(
    settings: Res<GameSettings>,
    screen: Res<State<Screen>>,
    mut fixed_time: ResMut<Time<Fixed>>,
) {
    let hz = if *screen.get() == Screen::Puzzle {
        f64::from(crate::sim::TICKS_PER_SECOND) * f64::from(settings.puzzle_speed) / 100.0
    } else {
        f64::from(crate::sim::TICKS_PER_SECOND)
    };
    if (fixed_time.timestep().as_secs_f64() - 1.0 / hz).abs() > 1e-9 {
        fixed_time.set_timestep_hz(hz);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_text() {
        let settings = GameSettings {
            commit: CommitScheme::Ijkl,
            repeat_delay: 0.35,
            // Both switches off, so the round trip is actually carrying
            // them: left at their default they would survive a `to_text`
            // that never wrote them and a `parse` that never read them.
            music_on: false,
            music_volume: 80,
            sfx_on: false,
            sfx_volume: 30,
            puzzle_speed: 75,
            team_mode: TeamMode::Trios,
            language: Lang::Fr,
            rumble: false,
            pad_deadzone: 70,
            colorblind: true,
            ui_scale: 130,
            reduced_motion: true,
            check_updates: false,
            binds: {
                let mut binds = default_binds();
                binds[0].set(crate::app::binds::Action::Remove, KeyCode::Backquote);
                binds[1].set(crate::app::binds::Action::ClearAll, KeyCode::End);
                binds
            },
            replay_cap: 35,
            names: [
                "Anna".into(),
                "Bo Diddley".into(),
                String::new(),
                "Dee".into(),
                "Eve".into(),
                String::new(),
            ],
            last_beach: "10.0.0.5:47777".into(),
            ..GameSettings::default()
        };
        let mut caps = crate::app::keycaps::KeyCaps::parse("KeyW=Z KeyA=Q");
        // Through the setter, so the language's presumed keyboard is in
        // place on this side too - `parse` takes it on the way back in.
        let mut settings = settings;
        settings.set_language(Lang::Fr, &mut caps);
        settings.set_keyboard(Some(crate::app::keycaps::Layout::Azerty), &mut caps);
        let (reparsed, recaps) = GameSettings::parse(&settings.to_text(&caps));
        assert_eq!(reparsed, settings);
        assert_eq!(recaps, caps, "the caps table rides the same file");
    }

    /// The `beach` line is kept only while it still parses as an address.
    /// See the arm that reads it for why.
    #[test]
    fn a_kept_beach_address_has_to_still_be_one() {
        assert_eq!(
            GameSettings::parse("beach: 192.168.1.5:47777\n")
                .0
                .last_beach,
            "192.168.1.5:47777"
        );
        assert_eq!(
            GameSettings::parse("beach: [::1]:47777\n").0.last_beach,
            "[::1]:47777",
            "including the six-legged sort"
        );
        for junk in ["beach: 192.168.1.5\n", "beach: over there\n", "beach:\n"] {
            assert_eq!(GameSettings::parse(junk).0.last_beach, "", "{junk:?}");
        }
    }

    /// A seat with no name of its own reads as the localized default label,
    /// and one with a name reads as the name, everywhere, since every
    /// mention of a seat goes through here.
    #[test]
    fn a_named_seat_answers_to_its_name() {
        let mut settings = GameSettings::default();
        assert_eq!(settings.seat_name(0), "P1");
        assert_eq!(settings.seat_name(3), "P4");
        settings.language = Lang::Fr;
        assert_eq!(settings.seat_name(1), "J2", "the default localizes");
        for ch in "Zoé".chars() {
            settings.push_name_char(1, ch);
        }
        assert_eq!(settings.seat_name(1), "Zoé");
        settings.pop_name_char(1);
        assert_eq!(settings.seat_name(1), "Zo");
        // A seat outside the table never panics (still French, above).
        assert_eq!(settings.seat_name(9), "J10");
    }

    /// Typing is filtered so a name can never break the save file, overflow
    /// the sidebar, or start with a space.
    #[test]
    fn typed_names_are_kept_clean() {
        let mut settings = GameSettings::default();
        for ch in " \n|:Ann|a: ".chars() {
            settings.push_name_char(0, ch);
        }
        assert_eq!(settings.names[0], "Anna ", "separators and controls drop");
        settings.tidy_name(0);
        assert_eq!(settings.names[0], "Anna", "trailing space trimmed away");

        settings.names[1] = String::new();
        for ch in "abcdefghijklmnopqrstuvwxyz".chars() {
            settings.push_name_char(1, ch);
        }
        assert_eq!(settings.names[1].chars().count(), NAME_MAX);

        // A name that is nothing but spaces is no name at all.
        settings.names[2] = "   ".into();
        settings.tidy_name(2);
        assert_eq!(settings.seat_name(2), "P3");

        // And a hand-edited file with a stray separator or an over-long name
        // is cleaned on the way in, not trusted.
        let parsed = GameSettings::parse("names: Anna|Bo:bby|aaaaaaaaaaaaaaaaaaaa\n").0;
        assert_eq!(parsed.names[0], "Anna");
        assert_eq!(parsed.names[1], "Bobby");
        assert_eq!(parsed.names[2].chars().count(), NAME_MAX);
        assert_eq!(parsed.names[3], "", "a missing field stays unnamed");
    }

    /// Bindings only load if the whole table is sane: a file that gives two
    /// seats the same key would leave one of them shadowed, so it is thrown
    /// out wholesale rather than half-applied.
    #[test]
    fn clashing_saved_bindings_fall_back_to_defaults() {
        let stock = default_binds();
        // Seat 2 claiming seat 1's W: legal within its own line, but not
        // across the keyboard.
        let text = "keys_p2: KeyW KeyK KeyJ KeyL Numpad8 Numpad5 Numpad4 Numpad6 \
                    Numpad0 NumpadEnter\n";
        assert_eq!(GameSettings::parse(text).0.binds, stock);
        // A line that is merely unreadable leaves that seat stock too.
        assert_eq!(GameSettings::parse("keys_p1: nonsense\n").0.binds, stock);
    }

    #[test]
    fn parse_is_lenient_and_clamps() {
        // Junk lines and out-of-range values fall back to sane settings.
        let settings = GameSettings::parse(
            "garbage\nmusic: 900\npad_deadzone: 99\nrepeat_delay: 9.0\nwibble: 3\n",
        )
        .0;
        // 900 does not even fit a u8: unparseable values keep the default.
        assert_eq!(settings.music_volume, GameSettings::default().music_volume);
        // Parseable but out-of-range values clamp.
        assert_eq!(GameSettings::parse("music: 101\n").0.music_volume, 100);
        assert_eq!(settings.pad_deadzone, 80);
        assert!(settings.repeat_delay <= 0.5);
        assert_eq!(settings.language, Lang::En);
    }

    /// A settings.txt from before the switches existed comes up with the
    /// sound on. Silence is the one wrong answer here: a file that predates
    /// a feature must not read as a player having turned it off, and this
    /// is a lenient parse, where a missing line is simply a default.
    #[test]
    fn a_file_without_the_switches_still_has_sound() {
        let old = "music: 60\nsfx: 40\n";
        let settings = GameSettings::parse(old).0;
        assert!(settings.music_on, "no music_on line means music");
        assert!(settings.sfx_on, "no sfx_on line means effects");
        assert_eq!(settings.music_volume, 60);
        assert_eq!(settings.sfx_volume, 40);

        // And a switch that is off silences its half without disturbing
        // the other, or the volume waiting underneath it.
        let quiet = GameSettings::parse("music: 60\nsfx: 40\nmusic_on: off\n").0;
        assert_eq!(quiet.music_gain(), 0.0, "the music is off");
        assert!(quiet.sfx_gain() > 0.0, "the effects are not");
        assert_eq!(quiet.music_volume, 60, "and the slider is where it was");
    }

    /// The sfx row was once an on/off toggle; a settings.txt written by that
    /// version must still load with the volume it implied.
    #[test]
    fn old_sfx_toggle_files_still_load() {
        assert_eq!(GameSettings::parse("sfx: off\n").0.sfx_volume, 0);
        assert_eq!(GameSettings::parse("sfx: on\n").0.sfx_volume, 100);
        assert_eq!(GameSettings::parse("sfx: 65\n").0.sfx_volume, 65);
        assert_eq!(GameSettings::parse("sfx: 900\n").0.sfx_volume, 80);
        assert_eq!(GameSettings::parse("sfx: 101\n").0.sfx_volume, 100);
    }
}
